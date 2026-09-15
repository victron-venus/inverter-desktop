use super::*;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

fn configuration(base: &str) -> Validated {
    Validated {
        base: url::Url::parse(base).unwrap(),
        entities: vec!["sensor.selected".into()],
        action_entities: vec![],
        token: "fixture-token-only".into(),
    }
}

fn changed(entity: &str, state: Value) -> Value {
    json!({"type":"event","id":1,"event":{"event_type":"state_changed",
        "data":{"entity_id":entity,"new_state":state}}})
}

#[test]
fn packets_require_bounded_json_and_websocket_limits_are_explicit() {
    for invalid in [
        b"null".as_slice(),
        b"[]",
        b"{}",
        b"{\"type\":4}",
        b"secret-invalid-json",
    ] {
        assert_eq!(packet(invalid), Err(Failure::Retry));
    }
    assert_eq!(
        packet(&vec![b' '; MAX_RESPONSE_BYTES + 1]),
        Err(Failure::Retry)
    );
    assert_eq!(packet(br#"{"type":"pong","id":2}"#).unwrap()["id"], 2);
    let limits = websocket_limits();
    assert_eq!(limits.max_frame_size, Some(MAX_RESPONSE_BYTES));
    assert_eq!(limits.max_message_size, Some(MAX_RESPONSE_BYTES));
    assert_eq!(limits.max_write_buffer_size, 16 * 1024);
}

#[test]
fn live_events_discard_unselected_payloads_and_require_selected_identity() {
    let configuration = configuration("http://127.0.0.1/prefix/");
    let unrelated = changed(
        "sensor.unselected",
        json!({"private":"must not be inspected"}),
    );
    assert!(event(&configuration, &unrelated).unwrap().is_none());
    let deleted = changed("sensor.selected", Value::Null);
    assert_eq!(
        event(&configuration, &deleted).unwrap(),
        Some(("sensor.selected", None))
    );
    let selected = changed(
        "sensor.selected",
        json!({"entity_id":"sensor.selected","state":"12.5"}),
    );
    assert_eq!(
        event(&configuration, &selected)
            .unwrap()
            .unwrap()
            .1
            .unwrap()["state"],
        "12.5"
    );
    let mismatched = changed(
        "sensor.selected",
        json!({"entity_id":"sensor.private","state":"private"}),
    );
    assert_eq!(event(&configuration, &mismatched), Err(Failure::Retry));
    let mut other_subscription = mismatched;
    other_subscription["id"] = json!(2);
    assert!(event(&configuration, &other_subscription)
        .unwrap()
        .is_none());
}

#[test]
fn subscription_requires_correlated_success_and_classifies_authentication() {
    assert_eq!(
        subscription_result(&json!({"type":"result","id":1,"success":true})),
        Ok(true)
    );
    assert_eq!(
        subscription_result(&json!({"type":"result","id":2,"success":true})),
        Ok(false)
    );
    assert_eq!(
        subscription_result(&json!({"type":"event","id":1,"success":true})),
        Ok(false)
    );
    for code in ["unauthorized", "forbidden"] {
        assert_eq!(
            subscription_result(&json!({"type":"result","id":1,"success":false,
            "error":{"code":code,"message":"private server details"}})),
            Err(Failure::Authentication)
        );
    }
    for result in [
        json!({"type":"result","id":1,"success":"true"}),
        json!({"type":"result","id":1,"success":false,"error":{"code":"unknown_command"}}),
    ] {
        assert_eq!(subscription_result(&result), Err(Failure::Retry));
    }
}

#[test]
fn heartbeat_requires_exact_pong_and_cannot_extend_deadline_with_traffic() {
    let start = Instant::now();
    let mut heartbeat = Heartbeat::new(start);
    assert_eq!(
        heartbeat.due(start + PING_INTERVAL - Duration::from_nanos(1)),
        Ok(None)
    );
    assert_eq!(heartbeat.due(start + PING_INTERVAL), Ok(Some(2)));
    heartbeat.sent(2, start + PING_INTERVAL).unwrap();
    heartbeat.pong(None);
    heartbeat.pong(Some(1));
    heartbeat.pong(Some(3));
    assert_eq!(
        heartbeat.due(start + PING_INTERVAL + PONG_TIMEOUT),
        Err(Failure::Retry)
    );
    heartbeat.pong(Some(2));
    assert_eq!(heartbeat.due(start + PING_INTERVAL * 2), Ok(Some(3)));
    heartbeat.sent(3, start + PING_INTERVAL * 2).unwrap();
    heartbeat.pong(Some(2));
    assert_eq!(
        heartbeat.due(start + PING_INTERVAL * 2 + PONG_TIMEOUT),
        Err(Failure::Retry)
    );
    assert_eq!(heartbeat.sent(u64::MAX, start), Err(Failure::Retry));
}

async fn initial_response(response: Vec<u8>) -> (Result<Option<Value>, Failure>, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let configuration = configuration(&format!(
        "http://{}/ha-prefix/",
        listener.local_addr().unwrap()
    ));
    let server = async {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).await.unwrap();
            request.push(byte[0]);
            assert!(request.len() < 8192);
        }
        // An oversized response can be rejected before all fixture bytes are sent.
        let _ = stream.write_all(&response).await;
        String::from_utf8(request).unwrap()
    };
    let client = http_client().unwrap();
    time::timeout(Duration::from_secs(3), async {
        tokio::join!(
            initial_state(&client, &configuration, "sensor.selected"),
            server
        )
    })
    .await
    .unwrap()
}

fn response(status: &str, body: &[u8]) -> Vec<u8> {
    let mut bytes = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

#[tokio::test]
async fn initial_http_reads_only_selected_prefixed_path_with_bearer_authentication() {
    let (result, request) = initial_response(response(
        "200 OK",
        br#"{"entity_id":"sensor.selected","state":"2"}"#,
    ))
    .await;
    assert_eq!(result.unwrap().unwrap()["state"], "2");
    assert!(request.starts_with("GET /ha-prefix/api/states/sensor.selected HTTP/1.1\r\n"));
    assert!(request
        .to_ascii_lowercase()
        .contains("\r\nauthorization: bearer fixture-token-only\r\n"));
    assert!(!request.contains("api/states HTTP"));
    assert!(!request.contains("call_service"));
}

#[tokio::test]
async fn initial_http_classifies_missing_authentication_and_server_failures() {
    for (status, expected) in [
        ("404 Not Found", Ok(None)),
        ("401 Unauthorized", Err(Failure::Authentication)),
        ("403 Forbidden", Err(Failure::Authentication)),
        ("500 Internal Server Error", Err(Failure::Retry)),
    ] {
        assert_eq!(
            initial_response(response(status, b"private details"))
                .await
                .0,
            expected
        );
    }
    for body in [
        br#"{"entity_id":"sensor.private","state":"secret"}"#.as_slice(),
        b"not JSON",
        b"[]",
    ] {
        assert_eq!(
            initial_response(response("200 OK", body)).await.0,
            Err(Failure::Retry)
        );
    }
}

#[tokio::test]
async fn initial_http_never_follows_redirect_or_forwards_token_to_redirect_target() {
    let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let redirect = format!("HTTP/1.1 302 Found\r\nLocation: http://{}/elsewhere\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", target.local_addr().unwrap());
    assert_eq!(
        initial_response(redirect.into_bytes()).await.0,
        Err(Failure::Retry)
    );
    assert!(time::timeout(Duration::from_millis(30), target.accept())
        .await
        .is_err());
}

#[tokio::test]
async fn initial_http_enforces_both_declared_and_streaming_body_limit() {
    let declared = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        MAX_RESPONSE_BYTES + 1
    );
    assert_eq!(
        initial_response(declared.into_bytes()).await.0,
        Err(Failure::Retry)
    );
    let mut chunked =
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_vec();
    for size in [MAX_RESPONSE_BYTES, 1] {
        chunked.extend_from_slice(format!("{size:x}\r\n").as_bytes());
        chunked.extend(std::iter::repeat_n(b' ', size));
        chunked.extend_from_slice(b"\r\n");
    }
    chunked.extend_from_slice(b"0\r\n\r\n");
    assert_eq!(initial_response(chunked).await.0, Err(Failure::Retry));
}
