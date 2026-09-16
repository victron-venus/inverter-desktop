use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    process::{Child, ChildStdin, Command, ExitStatus, Stdio},
    sync::mpsc::{self, Receiver},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tokio_tungstenite::tungstenite::{
    accept_hdr_with_config,
    handshake::server::{ErrorResponse, Request, Response},
    protocol::WebSocketConfig,
    Message, WebSocket,
};

const TOKEN: &str = "private-fixture-token-never-log";
const WAIT: Duration = Duration::from_secs(5);
const EXIT_WAIT: Duration = Duration::from_secs(2);
const MAX_FRAME: usize = 64 * 1024;

struct Process(Child);

impl Process {
    fn start() -> Self {
        Self(
            Command::new(env!("CARGO_BIN_EXE_inverter-home-assistant-worker"))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("start actual HA executable"),
        )
    }

    fn wait(&mut self, success: bool) -> ExitStatus {
        let until = Instant::now() + EXIT_WAIT;
        loop {
            if let Some(status) = self.0.try_wait().expect("inspect worker") {
                assert_eq!(status.success(), success, "unexpected worker exit");
                return status;
            }
            assert!(
                Instant::now() < until,
                "worker did not exit within two seconds"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Worker {
    process: Process,
    input: Option<ChildStdin>,
    frames: Receiver<Value>,
    output_task: Option<JoinHandle<()>>,
    error_task: Option<JoinHandle<String>>,
}

impl Worker {
    fn start() -> Self {
        let mut process = Process::start();
        let input = process.0.stdin.take();
        let output = process.0.stdout.take().expect("worker stdout");
        let errors = process.0.stderr.take().expect("worker stderr");
        let (send, frames) = mpsc::sync_channel(128);
        let output_task = thread::spawn(move || {
            let mut reader = BufReader::new(output);
            loop {
                let mut bytes = Vec::new();
                let length = reader
                    .by_ref()
                    .take(MAX_FRAME as u64 + 1)
                    .read_until(b'\n', &mut bytes)
                    .expect("read worker frame");
                if length == 0 {
                    break;
                }
                assert!(length <= MAX_FRAME && bytes.ends_with(b"\n"));
                let value = serde_json::from_slice(&bytes).expect("protocol JSON only");
                send.try_send(value).expect("test output remains bounded");
            }
        });
        let error_task = thread::spawn(move || {
            let mut result = String::new();
            errors.take(16 * 1024).read_to_string(&mut result).unwrap();
            result
        });
        Self {
            process,
            input,
            frames,
            output_task: Some(output_task),
            error_task: Some(error_task),
        }
    }

    fn send(&mut self, frame: &Value) {
        let input = self.input.as_mut().expect("open host pipe");
        writeln!(input, "{frame}").unwrap();
        input.flush().unwrap();
    }

    fn next(&self) -> Value {
        self.frames
            .recv_timeout(WAIT)
            .expect("worker protocol response")
    }

    fn hello(&mut self) {
        self.send(&hello());
        assert_eq!(
            self.next(),
            json!({"type":"ready","protocol_version":1,"host_api_version":"1.8.0",
                "plugin_id":"inverter-desktop.home-assistant"})
        );
    }

    fn configure(&mut self, fixture: &TcpListener, watch: &str) {
        self.send(&configuration(fixture, watch));
        assert_eq!(
            self.next(),
            json!({"type":"configuration_ready","revision":"test-1"})
        );
    }

    fn until(&self, predicate: impl Fn(&Value) -> bool) -> Value {
        let until = Instant::now() + WAIT;
        loop {
            let frame = self
                .frames
                .recv_timeout(until.saturating_duration_since(Instant::now()))
                .expect("expected contribution state");
            assert_eq!(frame["type"], "contributions");
            assert!(!frame.to_string().contains(TOKEN));
            assert!(frame["items"]
                .as_array()
                .unwrap()
                .iter()
                .all(|item| item["kind"] != "action"));
            if predicate(&frame) {
                return frame;
            }
        }
    }

    fn connection(&self, state: &str) -> Value {
        self.until(|frame| item(frame, "connection")["value"] == state)
    }

    fn finish(&mut self, success: bool) -> String {
        self.process.wait(success);
        self.output_task
            .take()
            .unwrap()
            .join()
            .expect("valid bounded stdout");
        self.error_task
            .take()
            .unwrap()
            .join()
            .expect("bounded stderr")
    }

    fn stop(&mut self, eof: bool) {
        if eof {
            self.input.take();
        } else {
            self.send(&json!({"type":"shutdown"}));
        }
        assert_eq!(self.finish(true), "");
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.process.0.kill();
        let _ = self.process.0.wait();
        if let Some(task) = self.output_task.take() {
            let _ = task.join();
        }
        if let Some(task) = self.error_task.take() {
            let _ = task.join();
        }
    }
}

fn hello() -> Value {
    json!({"type":"hello","protocol_version":1,"host_api_version":"1.8.0",
        "plugin_id":"inverter-desktop.home-assistant"})
}

fn configuration(fixture: &TcpListener, watch: &str) -> Value {
    json!({"type":"configuration","configuration":{"revision":"test-1",
        "values":{"ha_base_url":format!("http://{}/prefix",fixture.local_addr().unwrap()),
            "dashboard_layout":"","watch_entities":watch},"secrets":{"ha_token":TOKEN}}})
}

fn listener() -> TcpListener {
    let result = TcpListener::bind(("127.0.0.1", 0)).expect("private loopback fixture");
    result.set_nonblocking(true).unwrap();
    result
}

fn accept(fixture: &TcpListener) -> TcpStream {
    let until = Instant::now() + WAIT;
    loop {
        match fixture.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                stream.set_read_timeout(Some(WAIT)).unwrap();
                stream.set_write_timeout(Some(WAIT)).unwrap();
                return stream;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < until, "expected worker connection");
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("fixture accept failed: {error}"),
        }
    }
}

fn no_connection(fixture: &TcpListener, duration: Duration) {
    let until = Instant::now() + duration;
    loop {
        assert!(
            matches!(fixture.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
            "unexpected entity read or reconnect"
        );
        if Instant::now() >= until {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn ws_send(socket: &mut WebSocket<TcpStream>, value: Value) {
    socket
        .send(Message::Text(value.to_string().into()))
        .unwrap();
}

fn ws_json(socket: &mut WebSocket<TcpStream>) -> Value {
    let message = socket.read().expect("worker WebSocket request");
    assert!(message.is_text(), "expected a JSON protocol message");
    serde_json::from_str(message.to_text().unwrap()).unwrap()
}

// Tungstenite's callback requires its unboxed HTTP error response type.
#[allow(clippy::result_large_err)]
fn validate_websocket_request(
    request: &Request,
    response: Response,
) -> Result<Response, ErrorResponse> {
    assert_eq!(request.method(), "GET");
    assert_eq!(request.uri().path(), "/prefix/api/websocket");
    assert!(request.uri().query().is_none());
    Ok(response)
}

fn websocket(fixture: &TcpListener) -> WebSocket<TcpStream> {
    let stream = accept(fixture);
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_FRAME))
        .max_frame_size(Some(MAX_FRAME));
    let mut socket = accept_hdr_with_config(stream, validate_websocket_request, Some(config))
        .expect("WebSocket upgrade beneath explicit prefix");
    ws_send(
        &mut socket,
        json!({"type":"auth_required","ha_version":"fixture"}),
    );
    assert_eq!(
        ws_json(&mut socket),
        json!({"type":"auth","access_token":TOKEN})
    );
    socket
}

fn authorize(fixture: &TcpListener, watch: bool) -> WebSocket<TcpStream> {
    let mut socket = websocket(fixture);
    ws_send(
        &mut socket,
        json!({"type":"auth_ok","ha_version":"fixture"}),
    );
    if watch {
        let subscription = ws_json(&mut socket);
        assert_eq!(
            subscription,
            json!({"id":1,"type":"subscribe_events","event_type":"state_changed"})
        );
        ws_send(
            &mut socket,
            json!({"id":1,"type":"result","success":true,"result":null}),
        );
    }
    socket
}

fn rest_request(fixture: &TcpListener) -> (String, TcpStream) {
    let mut stream = accept(fixture);
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream
            .read_exact(&mut byte)
            .expect("complete bounded HTTP headers");
        bytes.push(byte[0]);
        assert!(bytes.len() < 16 * 1024);
    }
    let headers = String::from_utf8(bytes).unwrap();
    let request = headers.lines().next().unwrap().to_owned();
    let headers = headers.to_ascii_lowercase();
    assert!(headers.contains(&format!("\r\nhost: {}\r\n", fixture.local_addr().unwrap())));
    assert!(headers.contains(&format!("\r\nauthorization: bearer {TOKEN}\r\n")));
    (request, stream)
}

fn rest(fixture: &TcpListener, entity: &str) -> TcpStream {
    let (request, stream) = rest_request(fixture);
    assert_eq!(request, format!("GET /prefix/api/states/{entity} HTTP/1.1"));
    stream
}

fn respond(stream: &mut TcpStream, status: u16, body: Value) {
    let body = body.to_string();
    write!(stream, "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    stream.flush().unwrap();
}

fn entity(name: &str, value: &str) -> Value {
    json!({"entity_id":name,"state":value,"attributes":{
        "friendly_name":"<b>Selected</b>","unit_of_measurement":"kW","ignored_secret":TOKEN}})
}

fn live(socket: &mut WebSocket<TcpStream>, name: &str, state: Option<Value>) {
    ws_send(
        socket,
        json!({"id":1,"type":"event","event":{
        "event_type":"state_changed","data":{"entity_id":name,"old_state":null,"new_state":state}}}),
    );
}

fn item<'a>(frame: &'a Value, id: &str) -> &'a Value {
    frame["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == id)
        .expect("configured item")
}

#[test]
fn strict_handshake_rejects_wrong_identity_protocol_api_and_unknown_fields() {
    let mut wrong = Vec::new();
    for (key, value) in [
        ("plugin_id", json!("inverter-desktop.frigate")),
        ("protocol_version", json!(2)),
        ("host_api_version", json!("1.2.0")),
        ("host_api_version", json!("1.5.0")),
        ("host_api_version", json!("1.6.0")),
        ("host_api_version", json!("2.0.0")),
        ("host_api_version", json!("1.8.0-beta.1")),
        ("host_api_version", json!("invalid")),
        ("extra", json!(TOKEN)),
    ] {
        let mut frame = hello();
        frame[key] = value;
        wrong.push(frame);
    }
    wrong.push(json!({"type":"configuration","configuration":{}}));
    for frame in wrong {
        let mut worker = Worker::start();
        worker.send(&frame);
        assert_eq!(
            worker.finish(false),
            "Home Assistant worker session failed\n"
        );
        assert!(worker.frames.try_iter().next().is_none());
    }
}

#[test]
fn invalid_configuration_is_private_and_never_acknowledged_or_connected() {
    let fixture = listener();
    let mut invalid = Vec::new();
    for (field, value) in [
        (
            "ha_base_url",
            json!(format!(
                "http://user:{TOKEN}@{}/prefix",
                fixture.local_addr().unwrap()
            )),
        ),
        (
            "ha_base_url",
            json!(format!(
                "http://{}/prefix?token={TOKEN}",
                fixture.local_addr().unwrap()
            )),
        ),
        ("watch_entities", json!("sensor.invalid/path")),
        ("extra", json!(TOKEN)),
    ] {
        let mut frame = configuration(&fixture, "sensor.selected");
        frame["configuration"]["values"][field] = value;
        invalid.push(frame);
    }
    let mut token = configuration(&fixture, "");
    token["configuration"]["secrets"]["ha_token"] = json!("private token with spaces");
    invalid.push(token);
    for frame in invalid {
        let mut worker = Worker::start();
        worker.hello();
        worker.send(&frame);
        assert_eq!(
            worker.finish(false),
            "Home Assistant worker session failed\n"
        );
        assert!(worker.frames.try_iter().next().is_none());
        no_connection(&fixture, Duration::ZERO);
    }
}

#[test]
fn configuration_ack_precedes_network_and_only_selected_prefixed_reads_are_requested() {
    let fixture = listener();
    let mut worker = Worker::start();
    worker.hello();
    no_connection(&fixture, Duration::from_millis(50));
    worker.configure(&fixture, "input_boolean.do_not_supply_charger");
    let _socket = authorize(&fixture, true);
    let mut initial = rest(&fixture, "input_boolean.do_not_supply_charger");
    respond(
        &mut initial,
        200,
        entity("input_boolean.do_not_supply_charger", "off"),
    );
    let frame = worker.until(|frame| item(frame, "entity-0")["text"] == "off");
    assert_eq!(frame["items"].as_array().unwrap().len(), 2);
    assert_eq!(item(&frame, "entity-0")["title"], "<b>Selected</b>");
    assert_eq!(item(&frame, "connection")["value"], "Connected");
    no_connection(&fixture, Duration::from_millis(50));
    worker.stop(false);
}

#[test]
fn early_live_state_wins_over_delayed_rest_and_deletion_unwatched_events_remain_scoped() {
    let fixture = listener();
    let mut worker = Worker::start();
    worker.hello();
    worker.configure(&fixture, "sensor.selected");
    let mut socket = websocket(&fixture);
    ws_send(
        &mut socket,
        json!({"type":"auth_ok","ha_version":"fixture"}),
    );
    assert_eq!(
        ws_json(&mut socket),
        json!({"id":1,"type":"subscribe_events","event_type":"state_changed"})
    );
    live(
        &mut socket,
        "sensor.selected",
        Some(entity("sensor.selected", "98")),
    );
    worker.until(|frame| item(frame, "entity-0")["value"] == 98.0);
    no_connection(&fixture, Duration::from_millis(50));
    ws_send(
        &mut socket,
        json!({"id":1,"type":"result","success":true,"result":null}),
    );
    let mut initial = rest(&fixture, "sensor.selected");
    live(
        &mut socket,
        "sensor.selected",
        Some(entity("sensor.selected", "99")),
    );
    worker.until(|frame| item(frame, "entity-0")["value"] == 99.0);
    respond(&mut initial, 200, entity("sensor.selected", "1"));
    let until = Instant::now() + Duration::from_millis(500);
    while let Ok(frame) = worker
        .frames
        .recv_timeout(until.saturating_duration_since(Instant::now()))
    {
        assert_ne!(
            item(&frame, "entity-0")["value"],
            1.0,
            "late REST overwrote live state"
        );
    }
    live(&mut socket, "sensor.selected", None);
    worker.until(|frame| item(frame, "entity-0")["value"] == "Unavailable");
    live(
        &mut socket,
        "sensor.unwatched",
        Some(entity("sensor.unwatched", TOKEN)),
    );
    live(
        &mut socket,
        "sensor.selected",
        Some(entity("sensor.selected", "100")),
    );
    let frame = worker.until(|frame| item(frame, "entity-0")["value"] == 100.0);
    assert_eq!(frame["items"].as_array().unwrap().len(), 2);
    assert!(!frame.to_string().contains("sensor.unwatched"));
    worker.stop(false);
}

#[test]
fn blank_watchlist_authenticates_but_sends_no_subscription_or_entity_reads() {
    let fixture = listener();
    let mut worker = Worker::start();
    worker.hello();
    worker.configure(&fixture, " ,\n ");
    let mut socket = authorize(&fixture, false);
    let frame = worker.connection("Connected");
    assert_eq!(frame["items"].as_array().unwrap().len(), 1);
    socket
        .get_mut()
        .set_read_timeout(Some(Duration::from_millis(150)))
        .unwrap();
    assert!(
        matches!(socket.read(), Err(tokio_tungstenite::tungstenite::Error::Io(error))
        if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut))
    );
    no_connection(&fixture, Duration::from_millis(50));
    worker.stop(true);
}

#[test]
fn rejected_websocket_authentication_is_terminal_until_host_shutdown() {
    let fixture = listener();
    let mut worker = Worker::start();
    worker.hello();
    worker.configure(&fixture, "sensor.selected");
    let mut socket = websocket(&fixture);
    ws_send(&mut socket, json!({"type":"auth_invalid","message":TOKEN}));
    let frame = worker.connection("Authentication rejected");
    assert_eq!(item(&frame, "entity-0")["value"], "Unavailable");
    drop(socket);
    no_connection(&fixture, Duration::from_millis(1250));
    assert!(worker.process.0.try_wait().unwrap().is_none());
    worker.stop(false);
}

#[test]
fn rest_unauthorized_and_forbidden_are_not_retried_or_echoed() {
    for status in [401, 403] {
        let fixture = listener();
        let mut worker = Worker::start();
        worker.hello();
        worker.configure(&fixture, "sensor.selected");
        let _socket = authorize(&fixture, true);
        let mut request = rest(&fixture, "sensor.selected");
        respond(&mut request, status, json!({"message":TOKEN}));
        worker.connection("Authentication rejected");
        no_connection(&fixture, Duration::from_millis(1250));
        worker.stop(false);
    }
}

#[test]
fn reconnect_reauthenticates_resubscribes_and_replaces_previous_entity_state() {
    let fixture = listener();
    let mut worker = Worker::start();
    worker.hello();
    worker.configure(&fixture, "sensor.selected");
    let first = authorize(&fixture, true);
    respond(
        &mut rest(&fixture, "sensor.selected"),
        200,
        entity("sensor.selected", "7"),
    );
    worker.until(|frame| item(frame, "entity-0")["value"] == 7.0);
    drop(first);
    let disconnected = worker.connection("Disconnected");
    assert_eq!(item(&disconnected, "entity-0")["value"], "Unavailable");
    let _second = authorize(&fixture, true);
    respond(
        &mut rest(&fixture, "sensor.selected"),
        200,
        entity("sensor.selected", "8"),
    );
    worker.until(|frame| item(frame, "entity-0")["value"] == 8.0);
    worker.stop(false);
}

#[test]
fn initial_entity_reads_have_at_most_two_in_flight_requests() {
    let fixture = listener();
    let mut worker = Worker::start();
    worker.hello();
    worker.configure(&fixture, "sensor.first,sensor.second,sensor.third");
    let _socket = authorize(&fixture, true);
    let (first_path, mut first) = rest_request(&fixture);
    let (second_path, mut second) = rest_request(&fixture);
    let mut paths = vec![first_path.clone(), second_path.clone()];
    assert_ne!(first_path, second_path);
    no_connection(&fixture, Duration::from_millis(100));
    let name = |path: &str| {
        path.trim_start_matches("GET /prefix/api/states/")
            .trim_end_matches(" HTTP/1.1")
            .to_owned()
    };
    respond(&mut first, 200, entity(&name(&first_path), "1"));
    let (third_path, mut third) = rest_request(&fixture);
    paths.push(third_path.clone());
    paths.sort();
    assert_eq!(
        paths,
        ["first", "second", "third"]
            .map(|name| format!("GET /prefix/api/states/sensor.{name} HTTP/1.1"))
    );
    respond(&mut second, 200, entity(&name(&second_path), "2"));
    respond(&mut third, 200, entity(&name(&third_path), "3"));
    worker.until(|frame| {
        frame["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["kind"] == "metric")
            .count()
            == 3
    });
    no_connection(&fixture, Duration::from_millis(50));
    worker.stop(false);
}

#[test]
fn malformed_and_oversized_websocket_frames_disconnect_and_recover_without_echoing() {
    for bad in [format!("not-json-{TOKEN}"), "x".repeat(1024 * 1024 + 1)] {
        let fixture = listener();
        let mut worker = Worker::start();
        worker.hello();
        worker.configure(&fixture, "");
        let mut socket = authorize(&fixture, false);
        worker.connection("Connected");
        // The peer may reject the declared oversized frame before its body
        // finishes writing. Either transport outcome must disconnect safely.
        let _ = socket.send(Message::Text(bad.into()));
        worker.connection("Disconnected");
        drop(socket);
        let _recovered = authorize(&fixture, false);
        worker.connection("Connected");
        worker.stop(false);
    }
}

#[test]
fn oversized_http_body_is_rejected_before_waiting_for_or_exposing_payload() {
    let fixture = listener();
    let mut worker = Worker::start();
    worker.hello();
    worker.configure(&fixture, "sensor.selected");
    let _socket = authorize(&fixture, true);
    let mut response = rest(&fixture, "sensor.selected");
    write!(
        response,
        "HTTP/1.1 200 OK\r\nContent-Length: 1048577\r\n\r\n{TOKEN}"
    )
    .unwrap();
    response.flush().unwrap();
    let frame = worker.connection("Disconnected");
    assert_eq!(item(&frame, "entity-0")["value"], "Unavailable");
    worker.stop(false);
}

#[test]
fn eof_and_shutdown_cancel_stalled_websocket_upgrade_promptly() {
    for eof in [false, true] {
        let fixture = listener();
        let mut worker = Worker::start();
        worker.hello();
        worker.configure(&fixture, "");
        let _stalled = accept(&fixture);
        worker.stop(eof);
    }
}

#[test]
fn eof_and_shutdown_cancel_stalled_initial_http_body_promptly() {
    for eof in [false, true] {
        let fixture = listener();
        let mut worker = Worker::start();
        worker.hello();
        worker.configure(&fixture, "sensor.selected");
        let _socket = authorize(&fixture, true);
        let mut stalled = rest(&fixture, "sensor.selected");
        write!(
            stalled,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 100\r\n\r\n{{"
        )
        .unwrap();
        stalled.flush().unwrap();
        worker.stop(eof);
    }
}

fn blocked_output(eof: bool) {
    let fixture = listener();
    let mut process = Process::start();
    // The valid large build identifier fills stdout with Ready before the
    // configuration acknowledgement can flush. No reader drains that pipe.
    let mut frame = hello();
    frame["host_api_version"] = json!("1.8.0+");
    let padding = MAX_FRAME - serde_json::to_vec(&frame).unwrap().len() - 1;
    frame["host_api_version"] = json!(format!("1.8.0+{}", "a".repeat(padding)));
    let input = process.0.stdin.as_mut().unwrap();
    writeln!(input, "{frame}").unwrap();
    writeln!(input, "{}", configuration(&fixture, "sensor.selected")).unwrap();
    input.flush().unwrap();
    thread::sleep(Duration::from_millis(150));
    assert!(process.0.try_wait().unwrap().is_none());
    no_connection(&fixture, Duration::ZERO);
    if eof {
        process.0.stdin.take();
    } else {
        writeln!(
            process.0.stdin.as_mut().unwrap(),
            "{}",
            json!({"type":"shutdown"})
        )
        .unwrap();
    }
    process.wait(true);
}

#[test]
fn shutdown_cancels_backpressured_stdout_without_network_start() {
    blocked_output(false);
}

#[test]
fn eof_cancels_backpressured_stdout_without_network_start() {
    blocked_output(true);
}

#[test]
fn https_starts_tls_and_never_sends_a_plaintext_authentication_token() {
    let fixture = listener();
    let mut worker = Worker::start();
    worker.hello();
    let mut config = configuration(&fixture, "");
    config["configuration"]["values"]["ha_base_url"] =
        json!(format!("https://{}/prefix", fixture.local_addr().unwrap()));
    worker.send(&config);
    assert_eq!(worker.next()["type"], "configuration_ready");
    let mut stream = accept(&fixture);
    let mut prefix = [0; 3];
    stream.read_exact(&mut prefix).unwrap();
    assert_eq!(prefix[0], 0x16, "TLS ClientHello, not HTTP or JSON auth");
    assert_eq!(prefix[1], 3);
    worker.stop(false);
}

#[test]
fn compatible_newer_host_api_is_echoed_without_downgrading() {
    let mut worker = Worker::start();
    let mut frame = hello();
    frame["host_api_version"] = json!("1.9.0");
    worker.send(&frame);
    assert_eq!(
        worker.next(),
        json!({"type":"ready","protocol_version":1,"host_api_version":"1.9.0","plugin_id":"inverter-desktop.home-assistant"})
    );
    worker.stop(false);
}
