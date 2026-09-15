//! Real subprocess TLS proof, with no system trust-store changes or insecure client.

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
#[cfg(target_os = "linux")]
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::mpsc,
    time::{sleep, timeout, Instant},
};
use tokio_rustls::{
    rustls::{
        self,
        pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer},
    },
    server::TlsStream,
    TlsAcceptor,
};
use tokio_tungstenite::{
    tungstenite::{
        handshake::server::{ErrorResponse, Request, Response},
        Message,
    },
    WebSocketStream,
};

const TOKEN: &str = "disposable-tls-fixture-token";
const WAIT: Duration = Duration::from_secs(10);
const CA: &[u8] = include_bytes!("fixtures/tls/ca.pem");
static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TemporaryCa {
    directory: PathBuf,
    active: bool,
}

impl TemporaryCa {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        for _ in 0..64 {
            let directory = std::env::temp_dir().join(format!(
                "inverter-ha-tls-{}-{stamp}-{}",
                std::process::id(),
                NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
            ));
            match create_directory(&directory) {
                Ok(()) => {
                    let owned = Self {
                        directory,
                        active: true,
                    };
                    let mut options = OpenOptions::new();
                    options.write(true).create_new(true);
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::OpenOptionsExt;
                        options.mode(0o600);
                    }
                    options.open(owned.path()).unwrap().write_all(CA).unwrap();
                    return owned;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("cannot create private TLS fixture directory: {error}"),
            }
        }
        panic!("cannot reserve unique TLS fixture directory");
    }

    fn path(&self) -> PathBuf {
        self.directory.join("ca.pem")
    }

    fn close(mut self) {
        // The subprocess is reaped before removal, including on Windows.
        fs::remove_file(self.path()).unwrap();
        fs::remove_dir(&self.directory).unwrap();
        self.active = false;
    }
}

fn create_directory(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(path)
    }
    #[cfg(windows)]
    fs::create_dir(path)
}

impl Drop for TemporaryCa {
    fn drop(&mut self) {
        if self.active {
            // Remove only our known file and empty owned directory, never a tree.
            let _ = fs::remove_file(self.path());
            let _ = fs::remove_dir(&self.directory);
        }
    }
}

struct Worker {
    process: Child,
    input: ChildStdin,
    frames: mpsc::Receiver<Value>,
    stdout: Option<JoinHandle<()>>,
    stderr: Option<JoinHandle<String>>,
}

impl Worker {
    fn start(ca: Option<&Path>) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_inverter-home-assistant-worker"));
        command
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Windows TLS providers may need the OS directory; no user/network or
        // certificate environment is inherited into this isolated process.
        #[cfg(windows)]
        if let Some(root) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", root);
        }
        if let Some(ca) = ca {
            command.env("SSL_CERT_FILE", ca);
        }
        let mut process = command
            .spawn()
            .expect("start real HA worker with isolated environment");
        let input = process.stdin.take().unwrap();
        let output = process.stdout.take().unwrap();
        let errors = process.stderr.take().unwrap();
        let (sender, frames) = mpsc::channel(32);
        let stdout = thread::spawn(move || {
            let mut reader = BufReader::new(output);
            loop {
                let mut bytes = Vec::new();
                let count = reader
                    .by_ref()
                    .take(65_537)
                    .read_until(b'\n', &mut bytes)
                    .unwrap();
                if count == 0 {
                    break;
                }
                assert!(count <= 65_536 && bytes.ends_with(b"\n"));
                assert!(!String::from_utf8_lossy(&bytes).contains(TOKEN));
                sender
                    .try_send(serde_json::from_slice(&bytes).unwrap())
                    .expect("bounded TLS fixture output");
            }
        });
        let stderr = thread::spawn(move || {
            let mut result = String::new();
            errors.take(4096).read_to_string(&mut result).unwrap();
            result
        });
        Self {
            process,
            input,
            frames,
            stdout: Some(stdout),
            stderr: Some(stderr),
        }
    }

    fn send(&mut self, frame: Value) {
        writeln!(self.input, "{frame}").unwrap();
        self.input.flush().unwrap();
    }

    async fn next(&mut self) -> Value {
        timeout(WAIT, self.frames.recv())
            .await
            .unwrap()
            .expect("worker remains alive")
    }

    async fn configure(&mut self, listener: &TcpListener, selected: bool) {
        self.send(
            json!({"type":"hello","plugin_id":"inverter-desktop.home-assistant",
            "protocol_version":1,"host_api_version":"1.5.0"}),
        );
        assert_eq!(self.next().await["type"], "ready");
        self.send(
            json!({"type":"configuration","configuration":{"revision":"tls-1",
            "values":{"ha_base_url":format!("https://{}/tls-prefix",listener.local_addr().unwrap()),
                "watch_entities":if selected {"sensor.selected"} else {""}},
            "secrets":{"ha_token":TOKEN}}}),
        );
        assert_eq!(
            self.next().await,
            json!({"type":"configuration_ready","revision":"tls-1"})
        );
    }

    async fn until(&mut self, id: &str, field: &str, expected: Value) {
        timeout(WAIT, async {
            loop {
                let frame = self.next().await;
                assert_eq!(frame["type"], "contributions");
                if frame["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|item| item["id"] == id && item[field] == expected)
                {
                    break;
                }
            }
        })
        .await
        .expect("expected TLS contribution");
    }

    async fn stop(&mut self) {
        self.send(json!({"type":"shutdown"}));
        let until = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(status) = self.process.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(Instant::now() < until, "TLS work did not cancel promptly");
            sleep(Duration::from_millis(10)).await;
        }
        self.stdout
            .take()
            .unwrap()
            .join()
            .expect("bounded private stdout");
        assert_eq!(self.stderr.take().unwrap().join().unwrap(), "");
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
        if let Some(task) = self.stdout.take() {
            let _ = task.join();
        }
        if let Some(task) = self.stderr.take() {
            let _ = task.join();
        }
    }
}

fn acceptor() -> TlsAcceptor {
    let certificate =
        CertificateDer::from_pem_slice(include_bytes!("fixtures/tls/server.pem")).unwrap();
    let key = PrivateKeyDer::from_pem_slice(include_bytes!("fixtures/tls/server-key.pem")).unwrap();
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(vec![certificate], key)
    .unwrap();
    TlsAcceptor::from(Arc::new(config))
}

async fn tls(
    listener: &TcpListener,
    acceptor: &TlsAcceptor,
) -> std::io::Result<TlsStream<TcpStream>> {
    let (stream, _) = timeout(WAIT, listener.accept()).await.unwrap().unwrap();
    let mut record = [0; 3];
    timeout(WAIT, async {
        loop {
            if stream.peek(&mut record).await.unwrap() == record.len() {
                break;
            }
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        record[0], 0x16,
        "TLS handshake required before HTTP or credentials"
    );
    assert_eq!(record[1], 3);
    timeout(WAIT, acceptor.accept(stream)).await.unwrap()
}

type Socket = WebSocketStream<TlsStream<TcpStream>>;

async fn send(socket: &mut Socket, value: Value) {
    timeout(WAIT, socket.send(Message::Text(value.to_string().into())))
        .await
        .unwrap()
        .unwrap();
}

async fn next(socket: &mut Socket) -> Value {
    let message = timeout(WAIT, socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    serde_json::from_str(message.to_text().unwrap()).unwrap()
}

// Tungstenite's callback requires its unboxed HTTP error response type.
#[allow(clippy::result_large_err)]
fn validate_websocket_request(
    request: &Request,
    response: Response,
) -> Result<Response, ErrorResponse> {
    assert_eq!(request.method(), "GET");
    assert_eq!(request.uri().path(), "/tls-prefix/api/websocket");
    assert!(request.uri().query().is_none());
    assert!(!request.headers().contains_key("authorization"));
    Ok(response)
}

async fn websocket(listener: &TcpListener, acceptor: &TlsAcceptor, selected: bool) -> Socket {
    let stream = tls(listener, acceptor)
        .await
        .expect("client trusts only the explicit fixture CA");
    let mut socket = timeout(
        WAIT,
        tokio_tungstenite::accept_hdr_async(stream, validate_websocket_request),
    )
    .await
    .unwrap()
    .unwrap();
    send(&mut socket, json!({"type":"auth_required"})).await;
    assert_eq!(
        next(&mut socket).await,
        json!({"type":"auth","access_token":TOKEN})
    );
    send(&mut socket, json!({"type":"auth_ok"})).await;
    if selected {
        assert_eq!(
            next(&mut socket).await,
            json!({"id":1,"type":"subscribe_events","event_type":"state_changed"})
        );
        send(
            &mut socket,
            json!({"id":1,"type":"result","success":true,"result":null}),
        )
        .await;
    }
    socket
}

#[tokio::test]
async fn default_worker_rejects_untrusted_tls_before_http_or_ha_token() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut worker = Worker::start(None);
    worker.configure(&listener, false).await;
    assert!(
        tls(&listener, &acceptor()).await.is_err(),
        "disposable CA must not be trusted by default"
    );
    worker
        .until("connection", "value", json!("Disconnected"))
        .await;
    worker.stop().await;
}

#[tokio::test]
async fn isolated_ca_allows_verified_wss_authentication_without_system_trust_changes() {
    let ca = TemporaryCa::new();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut worker = Worker::start(Some(&ca.path()));
    worker.configure(&listener, false).await;
    let mut socket = websocket(&listener, &acceptor(), false).await;
    worker
        .until("connection", "value", json!("Connected"))
        .await;
    assert!(timeout(Duration::from_millis(100), listener.accept())
        .await
        .is_err());
    worker.stop().await;
    assert!(timeout(WAIT, socket.next())
        .await
        .unwrap()
        .is_none_or(|result| result.is_err() || matches!(result, Ok(Message::Close(_)))));
    drop(worker);
    ca.close();
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn linux_isolated_ca_verifies_both_wss_and_selected_https_rest() {
    let ca = TemporaryCa::new();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut worker = Worker::start(Some(&ca.path()));
    worker.configure(&listener, true).await;
    let acceptor = acceptor();
    let _socket = websocket(&listener, &acceptor, true).await;
    let mut stream = tls(&listener, &acceptor)
        .await
        .expect("Linux HTTPS honors only the subprocess CA override");
    let mut headers = Vec::new();
    timeout(WAIT, async {
        while !headers.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).await.unwrap();
            headers.push(byte[0]);
            assert!(headers.len() <= 8192);
        }
    })
    .await
    .unwrap();
    let headers = String::from_utf8(headers).unwrap();
    assert!(headers.starts_with("GET /tls-prefix/api/states/sensor.selected HTTP/1.1\r\n"));
    assert!(headers
        .to_ascii_lowercase()
        .contains(&format!("\r\nauthorization: bearer {TOKEN}\r\n")));
    let body = br#"{"entity_id":"sensor.selected","state":"42"}"#;
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    timeout(WAIT, async {
        stream.write_all(response.as_bytes()).await.unwrap();
        stream.write_all(body).await.unwrap();
        stream.shutdown().await.unwrap();
    })
    .await
    .unwrap();
    worker.until("entity-0", "value", json!(42.0)).await;
    worker.stop().await;
    drop(worker);
    ca.close();
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[tokio::test]
async fn native_https_verifier_rejects_fixture_ca_even_after_verified_wss() {
    let ca = TemporaryCa::new();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut worker = Worker::start(Some(&ca.path()));
    worker.configure(&listener, true).await;
    let acceptor = acceptor();
    let _socket = websocket(&listener, &acceptor, true).await;
    // Reqwest uses OS verification here, unlike WSS's native-root loader.
    // SSL_CERT_FILE must not install the disposable CA into the OS trust store.
    assert!(tls(&listener, &acceptor).await.is_err());
    worker
        .until("connection", "value", json!("Disconnected"))
        .await;
    worker.stop().await;
    drop(worker);
    ca.close();
}
