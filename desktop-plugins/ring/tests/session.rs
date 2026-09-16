#[path = "../../camera-common/tests/support/mod.rs"]
mod support;
use serde_json::json;
use std::{
    io::{Read, Write},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use support::*;
const PROVIDER: &str = "ring";
const TITLE: &str = "Ring";
const BINARY: &str = env!("CARGO_BIN_EXE_inverter-ring-worker");
const TOPICS: &[&str] = &["ring/+/camera/+/motion/state", "ring/+/camera/+/ding/state"];
const TOPIC: &str = "ring/home/camera/front/motion/state";
const OTHER: &str = "ring/home/camera/back/motion/state";
const PAYLOAD: &[u8] = b"ON";
fn start() -> Worker {
    Worker::start(BINARY, PROVIDER, TITLE)
}

#[test]
fn authenticated_snapshot_is_host_owned_and_retained_ring_on_is_preserved() {
    let listener = listener();
    let mut worker = start();
    worker.hello();
    let mut config = configuration(listener.local_addr().unwrap().port());
    config["configuration"]["values"]["snapshot_base_url"] =
        json!("https://ha.invalid/api/camera_proxy");
    config["configuration"]["values"]["snapshot_media_kind"] = json!("webp");
    config["configuration"]["secrets"]["snapshot_url_template"] =
        json!("https://ha.invalid/api/camera_proxy/{device_id}?event={event}");
    config["configuration"]["secrets"]["snapshot_bearer_token"] = json!("private-bearer");
    worker.configure_frame(config);
    let mut stream = subscribe(&listener, PROVIDER, TOPICS);
    worker.status("Connected");
    publish(&mut stream, TOPIC, PAYLOAD, true);
    let notice = worker.frame();
    let snapshot = worker.frame();
    assert_eq!(notice["type"], "notification");
    assert_eq!(snapshot["type"], "http_video");
    assert_eq!(
        snapshot["url"],
        "https://ha.invalid/api/camera_proxy/front?event=motion"
    );
    assert_eq!(snapshot["media_kind"], "webp");
    assert!(!snapshot.to_string().contains("private-bearer"));
    publish(
        &mut stream,
        "ring/home/camera/front/ding/state",
        b"TRUE",
        false,
    );
    assert_eq!(worker.frame()["type"], "notification");
    assert!(worker.frame()["url"]
        .as_str()
        .unwrap()
        .ends_with("event=ding"));
    worker.shutdown();
}

#[test]
fn invalid_snapshot_authority_fails_without_connecting_or_echoing_secrets() {
    let listener = listener();
    let mut worker = start();
    worker.hello();
    let mut config = configuration(listener.local_addr().unwrap().port());
    config["configuration"]["values"]["snapshot_base_url"] = json!("https://ha.invalid/api");
    config["configuration"]["secrets"]["snapshot_url_template"] =
        json!("https://elsewhere.invalid/api/front?token=private");
    worker.send(config);
    worker.exit(false);
    assert!(listener.accept().is_err());
    let mut stderr = String::new();
    worker
        .child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert_eq!(stderr, "Ring worker session failed\n");
}

#[test]
fn subscribes_only_after_configuration_and_filters_unrelated_publishes() {
    let listener = listener();
    let mut worker = start();
    worker.hello();
    assert!(listener.accept().is_err());
    worker.configure(listener.local_addr().unwrap().port());
    let mut stream = subscribe(&listener, PROVIDER, TOPICS);
    worker.status("Connected");
    publish(&mut stream, "frigate/events", PAYLOAD, false);
    publish(&mut stream, "ring/other/topic", PAYLOAD, false);
    publish(&mut stream, TOPIC, PAYLOAD, false);
    let frame = worker.frame();
    assert_eq!(frame["type"], "notification");
    assert!(frame["id"].as_str().unwrap().starts_with(PROVIDER));
    assert!(frame["title"].as_str().unwrap().starts_with(TITLE));
    publish(&mut stream, TOPIC, PAYLOAD, false);
    assert!(worker
        .frames
        .recv_timeout(Duration::from_millis(150))
        .is_err());
    worker.shutdown();
}

#[test]
fn reconnect_requires_resubscription_and_preserves_dedupe() {
    let listener = listener();
    let mut worker = start();
    worker.hello();
    worker.configure(listener.local_addr().unwrap().port());
    let mut stream = subscribe(&listener, PROVIDER, TOPICS);
    worker.status("Connected");
    publish(&mut stream, TOPIC, PAYLOAD, false);
    assert_eq!(worker.frame()["type"], "notification");
    drop(stream);
    worker.status("Disconnected");
    worker.status("Connecting");
    let mut stream = subscribe(&listener, PROVIDER, TOPICS);
    worker.status("Connected");
    publish(&mut stream, TOPIC, PAYLOAD, false);
    publish(&mut stream, OTHER, PAYLOAD, false);
    assert_eq!(worker.frame()["type"], "notification");
    assert!(worker
        .frames
        .recv_timeout(Duration::from_millis(150))
        .is_err());
    worker.shutdown();
}

#[test]
fn rejects_wrong_missing_or_failed_subscription_acknowledgements() {
    for mode in ["wrong-id", "missing-result", "failure"] {
        let listener = listener();
        let mut worker = start();
        worker.hello();
        worker.configure(listener.local_addr().unwrap().port());
        let (mut stream, pkid) = begin_subscribe(&listener, PROVIDER, TOPICS);
        let mut ack = vec![0x90, 4, pkid[0], pkid[1], 0, 0];
        match mode {
            "wrong-id" => ack[3] = pkid[1].wrapping_add(1),
            "missing-result" => {
                ack.pop();
                ack[1] = 3;
            }
            _ => ack[5] = 0x80,
        }
        stream.write_all(&ack).unwrap();
        worker.status("Disconnected");
        worker.shutdown();
    }
}

#[test]
fn rejects_older_host_and_foreign_plugin_identity() {
    for (api, id) in [
        ("1.7.0", format!("inverter-desktop.{PROVIDER}")),
        ("1.8.0", "inverter-desktop.frigate".into()),
    ] {
        let mut worker = start();
        worker.send(
            json!({"type":"hello","protocol_version":1,"host_api_version":api,"plugin_id":id}),
        );
        worker.exit(false);
    }
}

#[test]
fn eof_cancels_connection_wait_and_configuration_errors_do_not_leak_credentials() {
    let listener = listener();
    let mut worker = start();
    worker.hello();
    worker.configure(listener.local_addr().unwrap().port());
    let mut stream = accept(&listener);
    let _ = packet(&mut stream);
    worker.input.take();
    worker.exit(true);
    let mut byte = [0];
    assert_eq!(stream.read(&mut byte).unwrap(), 0);
    let mut invalid = start();
    invalid.hello();
    let mut frame = configuration(listener.local_addr().unwrap().port());
    frame["configuration"]["values"]["mqtt_topics"] = json!("#");
    invalid.send(frame);
    invalid.exit(false);
    let mut errors = String::new();
    invalid
        .child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut errors)
        .unwrap();
    assert_eq!(errors, "Ring worker session failed\n");
    assert!(matches!(listener.accept(),Err(error) if error.kind()==std::io::ErrorKind::WouldBlock));
}

#[test]
fn tls_never_falls_back_to_plaintext_mqtt() {
    let listener = listener();
    let mut worker = start();
    worker.hello();
    let mut frame = configuration(listener.local_addr().unwrap().port());
    frame["configuration"]["values"]["mqtt_tls"] = json!(true);
    worker.send(frame);
    assert_eq!(worker.frame()["type"], "configuration_ready");
    worker.status("Connecting");
    let mut stream = accept(&listener);
    let mut header = [0; 3];
    stream.read_exact(&mut header).unwrap();
    assert_eq!(header[0], 0x16, "TLS handshake, never MQTT CONNECT");
    assert_eq!(header[1], 3);
    stream.write_all(&[0x20, 2, 0, 0]).unwrap();
    drop(stream);
    worker.status("Disconnected");
    worker.shutdown();
}

fn blocked_output_session(eof: bool) {
    struct Guard(Child);
    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let listener = listener();
    let mut worker = Guard(
        Command::new(BINARY)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    // A valid large semantic-version build identifier produces a full-size
    // Ready frame. Retain the read handle without draining it, then queue the
    // configuration: its acknowledgement cannot fit in the occupied pipe.
    let mut hello = json!({"type":"hello","protocol_version":1,"host_api_version":"1.8.0+","plugin_id":format!("inverter-desktop.{PROVIDER}")});
    let padding = 65536 - serde_json::to_vec(&hello).unwrap().len() - 1;
    hello["host_api_version"] = json!(format!("1.8.0+{}", "a".repeat(padding)));
    let input = worker.0.stdin.as_mut().unwrap();
    writeln!(input, "{hello}").unwrap();
    writeln!(
        input,
        "{}",
        configuration(listener.local_addr().unwrap().port())
    )
    .unwrap();
    input.flush().unwrap();
    std::thread::sleep(Duration::from_millis(150));
    assert!(worker.0.try_wait().unwrap().is_none());
    assert!(matches!(listener.accept(),Err(error) if error.kind()==std::io::ErrorKind::WouldBlock));
    if eof {
        worker.0.stdin.take();
    } else {
        writeln!(
            worker.0.stdin.as_mut().unwrap(),
            "{}",
            json!({"type":"shutdown"})
        )
        .unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(status) = worker.0.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < deadline, "stdout must not delay shutdown");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn shutdown_does_not_wait_for_backpressured_stdout() {
    blocked_output_session(false);
}

#[test]
fn eof_does_not_wait_for_backpressured_stdout() {
    blocked_output_session(true);
}
