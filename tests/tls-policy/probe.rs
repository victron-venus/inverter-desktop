//! Test-only actual ClientBuilder probes; no system or user trust-store writes.
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct Fixtures(PathBuf);

impl Drop for Fixtures {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn python() -> std::ffi::OsString {
    std::env::var_os("DESKTOP_TLS_PYTHON").unwrap_or_else(|| "python3".into())
}

fn helper() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .map(|parent| parent.join("tests/tls-policy/server.py"))
        .find(|path| path.is_file())
        .expect("test TLS server helper must be present in repository")
}

fn fixtures() -> Fixtures {
    let id = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "inverter-test-tls-{}-{timestamp}-{id}",
        std::process::id()
    ));
    std::fs::create_dir(&path).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let fixtures = Fixtures(path);
    let output = Command::new(python())
        .arg(helper())
        .arg("prepare")
        .arg(&fixtures.0)
        .output()
        .expect("Python and OpenSSL must be available for TLS policy tests");
    assert!(
        output.status.success(),
        "TLS fixture setup failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fixtures
}

fn read_json(reader: &mut impl BufRead) -> Value {
    let mut line = String::new();
    assert_ne!(
        reader.read_line(&mut line).unwrap(),
        0,
        "TLS helper ended without evidence"
    );
    serde_json::from_str(&line).expect("TLS helper output must be JSON")
}

pub(crate) async fn matrix(label: &str, builder: impl Fn() -> reqwest::ClientBuilder) {
    let fixtures = fixtures();
    let mut observations = Vec::new();
    let mut failures = Vec::new();
    // Apple currently rejects both PSS certificate fixtures before our key guard.
    // Keep the Windows compatibility assertions intact; the Apple gate covers
    // the documented RSA-PKCS1/ECDSA certificate profile and records exclusions.
    let compatibility_only: &[&str] = if cfg!(target_vendor = "apple") {
        &["strong-pss", "strong-pss-key"]
    } else {
        &[]
    };
    let profile = if cfg!(target_vendor = "apple") {
        "apple-rsa-pkcs1-ecdsa"
    } else {
        "native-rsa-ecdsa-pss"
    };
    for case in [
        "strong",
        "strong-ec",
        "strong-pss",
        "strong-pss-key",
        "weak-leaf",
        "weak-intermediate",
        "weak-root",
        "weak-2047-root",
        "weak-ec-intermediate",
        "weak-ec-root",
        "untrusted",
        "wrong-host",
    ] {
        if compatibility_only.contains(&case) {
            continue;
        }
        let child = Command::new(python())
            .arg(helper())
            .arg("serve")
            .arg(&fixtures.0)
            .args(["--case", case])
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("TLS fixture server must start");
        let mut server = Server(child);
        let mut output = BufReader::new(server.0.stdout.take().unwrap());
        let ready = read_json(&mut output);
        assert_eq!(ready["oracle"]["phase"], "application");
        assert!(ready["oracle"]["bytes"].as_u64().unwrap() > 0);
        let port = ready["port"].as_u64().unwrap() as u16;
        let mut client = builder();
        if case != "untrusted" {
            let ca = std::fs::read(ready["root"].as_str().unwrap()).unwrap();
            client = client.add_root_certificate(reqwest::Certificate::from_pem(&ca).unwrap());
        }
        let client = client
            // Keep the fixture on loopback even on hosts with a configured proxy.
            // This is a test transport seam, not a change to production policy.
            .no_proxy()
            .resolve("localhost", ([127, 0, 0, 1], port).into())
            .build()
            .expect("production TLS builder must initialize");
        let host = if case == "wrong-host" {
            "127.0.0.1"
        } else {
            "localhost"
        };
        let result = client
            .get(format!("https://{host}:{port}/actual-client"))
            .bearer_auth("disposable-tls-policy-test-token")
            .send()
            .await;
        let accepted = result
            .as_ref()
            .is_ok_and(|response| response.status().is_success());
        let error = result.err().map(|error| format!("{error:?}"));
        let received = read_json(&mut output);
        assert!(
            server.0.wait().unwrap().success(),
            "TLS helper failed after a connection"
        );
        let request = received["request"].as_str().unwrap();
        let token_received = request.contains("disposable-tls-policy-test-token");
        let expected = if case.starts_with("strong") {
            accepted && received["phase"] == "application" && token_received
        } else {
            !accepted
                && received["phase"] == "handshake"
                && received["bytes"] == 0
                && !token_received
        };
        if !expected {
            failures.push(case);
        }
        observations.push(json!({"case":case,"accepted":accepted,"client_error":error,"server":received,"oracle":ready["oracle"],"oracle_policy":ready["oracle_policy"],"openssl":ready["openssl"],"expected_strong_only":expected}));
    }
    let report = json!({"builder":label,"os":std::env::consts::OS,"profile":profile,"compatibility_cases_outside_gate":compatibility_only,"observations":observations});
    println!("TLS_POLICY_REPORT {report}");
    if let Some(directory) = std::env::var_os("DESKTOP_TLS_PROBE_OUTPUT_DIR") {
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            Path::new(&directory).join(format!("{label}.json")),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
    }
    assert!(
        failures.is_empty(),
        "TLS key policy mismatches for {label}: {failures:?}"
    );
}
