//! Actual worker pipes and private HA services; no production server or appliance.

use serde_json::{json, Value};
use std::{
    collections::{HashSet, VecDeque},
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tokio_tungstenite::tungstenite::{
    accept_hdr_with_config,
    handshake::server::{ErrorResponse, Request, Response},
    protocol::WebSocketConfig,
    Message, WebSocket,
};

const TOKEN: &str = "disposable-action-fixture-token";
const PRIVATE_BODY: &str = "private-service-response-never-echo";
const WAIT: Duration = Duration::from_secs(5);
const MAX_FRAME: usize = 64 * 1024;

struct Worker {
    process: Child,
    input: Option<ChildStdin>,
    frames: Receiver<Value>,
    deferred: VecDeque<Value>,
    paused: Arc<AtomicBool>,
    allow_partial_exit: Arc<AtomicBool>,
    stdout: Option<JoinHandle<()>>,
    stderr: Option<JoinHandle<String>>,
}

impl Worker {
    fn start() -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_inverter-home-assistant-worker"));
        command
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        if let Some(root) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", root);
        }
        let mut process = command.spawn().expect("start actual HA worker");
        let input = process.stdin.take();
        let output = process.stdout.take().unwrap();
        let errors = process.stderr.take().unwrap();
        let (sender, frames) = mpsc::sync_channel(256);
        let paused = Arc::new(AtomicBool::new(false));
        let reader_paused = paused.clone();
        let allow_partial_exit = Arc::new(AtomicBool::new(false));
        let reader_partial_exit = allow_partial_exit.clone();
        let stdout = thread::spawn(move || {
            let mut reader = BufReader::new(output);
            loop {
                while reader_paused.load(Ordering::Acquire) {
                    thread::sleep(Duration::from_millis(5));
                }
                let mut bytes = Vec::new();
                let length = reader
                    .by_ref()
                    .take(MAX_FRAME as u64 + 1)
                    .read_until(b'\n', &mut bytes)
                    .unwrap();
                if length == 0 {
                    break;
                }
                if !bytes.ends_with(b"\n") && reader_partial_exit.load(Ordering::Acquire) {
                    // Exiting with a full OS pipe can interrupt its last write.
                    assert!(length < MAX_FRAME);
                    break;
                }
                assert!(length <= MAX_FRAME && bytes.ends_with(b"\n"));
                let text = String::from_utf8(bytes).unwrap();
                assert!(!text.contains(TOKEN));
                assert!(!text.contains(PRIVATE_BODY));
                sender
                    .try_send(serde_json::from_str(&text).unwrap())
                    .expect("bounded test output");
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
            deferred: VecDeque::new(),
            paused,
            allow_partial_exit,
            stdout: Some(stdout),
            stderr: Some(stderr),
        }
    }

    fn send(&mut self, frame: Value) {
        let input = self.input.as_mut().expect("open host stdin");
        writeln!(input, "{frame}").unwrap();
        input.flush().unwrap();
    }

    fn next(&self) -> Value {
        self.frames.recv_timeout(WAIT).expect("worker response")
    }

    fn until(&mut self, predicate: impl Fn(&Value) -> bool) -> Value {
        if let Some(index) = self.deferred.iter().position(&predicate) {
            return self.deferred.remove(index).unwrap();
        }
        let until = Instant::now() + WAIT;
        loop {
            let frame = self
                .frames
                .recv_timeout(until.saturating_duration_since(Instant::now()))
                .expect("expected worker result or contribution");
            if predicate(&frame) {
                return frame;
            }
            if frame["type"] != "contributions" {
                self.deferred.push_back(frame);
                assert!(self.deferred.len() <= 32);
            }
        }
    }

    fn configure(&mut self, fixture: &TcpListener, watch: &str, actions: Option<&str>) {
        self.configure_frame(configuration(fixture, watch, actions));
    }

    fn configure_frame(&mut self, frame: Value) {
        self.send(hello());
        assert_eq!(
            self.next(),
            json!({"type":"ready","protocol_version":1,
            "host_api_version":"1.4.0","plugin_id":"inverter-desktop.home-assistant"})
        );
        self.send(frame);
        assert_eq!(
            self.next(),
            json!({"type":"configuration_ready","revision":"actions-1"})
        );
    }

    fn action(&mut self, request: &str, action: &str, deadline_ms: u64) {
        self.send(action_frame(request, action, json!({}), deadline_ms));
    }

    fn result(&mut self, request: &str) -> Value {
        self.until(|frame| frame["request_id"] == request)
    }

    fn error(&mut self, request: &str, code: &str) {
        let frame = self.result(request);
        assert_eq!(frame["type"], "action_error");
        assert_eq!(frame["code"], code);
        assert!(!frame["message"].as_str().unwrap().is_empty());
        assert!(frame["message"].as_str().unwrap().len() <= 512);
    }

    fn success(&mut self, request: &str) {
        assert_eq!(
            self.result(request),
            json!({"type":"action_result",
            "request_id":request,"value":{}})
        );
    }

    fn finish(&mut self, success: bool) {
        let until = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(status) = self.process.try_wait().unwrap() {
                assert_eq!(status.success(), success, "unexpected worker exit");
                break;
            }
            assert!(Instant::now() < until, "worker must stop promptly");
            thread::sleep(Duration::from_millis(10));
        }
        self.paused.store(false, Ordering::Release);
        self.stdout.take().unwrap().join().expect("valid output");
        let stderr = self.stderr.take().unwrap().join().unwrap();
        assert_eq!(
            stderr,
            if success {
                ""
            } else {
                "Home Assistant worker session failed\n"
            }
        );
    }

    fn stop(&mut self, eof: bool) {
        self.allow_partial_exit
            .store(self.paused.load(Ordering::Acquire), Ordering::Release);
        if eof {
            self.input.take();
        } else {
            self.send(json!({"type":"shutdown"}));
        }
        self.finish(true);
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
        self.paused.store(false, Ordering::Release);
        if let Some(task) = self.stdout.take() {
            let _ = task.join();
        }
        if let Some(task) = self.stderr.take() {
            let _ = task.join();
        }
    }
}

fn hello() -> Value {
    json!({"type":"hello","protocol_version":1,"host_api_version":"1.4.0",
        "plugin_id":"inverter-desktop.home-assistant"})
}

fn configuration(fixture: &TcpListener, watch: &str, actions: Option<&str>) -> Value {
    let mut frame = json!({"type":"configuration","configuration":{
        "revision":"actions-1","values":{
            "ha_base_url":format!("http://{}/reverse/proxy/ha",fixture.local_addr().unwrap()),
            "watch_entities":watch},"secrets":{"ha_token":TOKEN}}});
    if let Some(actions) = actions {
        frame["configuration"]["values"]["action_entities"] = json!(actions);
    }
    frame
}

fn action_frame(request: &str, action: &str, params: Value, deadline_ms: u64) -> Value {
    json!({"type":"action","request_id":request,"action_id":action,
        "params":params,"deadline_ms":deadline_ms})
}

fn listener() -> TcpListener {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    listener
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
                assert!(Instant::now() < until, "expected private HA request");
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("fixture accept failed: {error}"),
        }
    }
}

fn no_request(fixture: &TcpListener, duration: Duration) {
    let until = Instant::now() + duration;
    loop {
        assert!(
            matches!(fixture.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
            "unexpected HA request: no action queue, redirects or automatic retries"
        );
        if Instant::now() >= until {
            break;
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
    let message = socket.read().expect("HA WebSocket packet");
    serde_json::from_str(message.to_text().unwrap()).unwrap()
}

// Tungstenite specifies this unboxed HTTP response in its callback contract.
#[allow(clippy::result_large_err)]
fn upgrade(request: &Request, response: Response) -> Result<Response, ErrorResponse> {
    assert_eq!(request.method(), "GET");
    assert_eq!(request.uri().path(), "/reverse/proxy/ha/api/websocket");
    assert!(request.uri().query().is_none());
    Ok(response)
}

fn authorize(fixture: &TcpListener, selected: bool) -> WebSocket<TcpStream> {
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_FRAME))
        .max_frame_size(Some(MAX_FRAME));
    let mut socket = accept_hdr_with_config(accept(fixture), upgrade, Some(config)).unwrap();
    ws_send(
        &mut socket,
        json!({"type":"auth_required","ha_version":"fixture"}),
    );
    assert_eq!(
        ws_json(&mut socket),
        json!({"type":"auth","access_token":TOKEN})
    );
    ws_send(
        &mut socket,
        json!({"type":"auth_ok","ha_version":"fixture"}),
    );
    if selected {
        assert_eq!(
            ws_json(&mut socket),
            json!({"id":1,"type":"subscribe_events","event_type":"state_changed"})
        );
        ws_send(
            &mut socket,
            json!({"id":1,"type":"result","success":true,"result":null}),
        );
    }
    socket
}

struct HttpRequest {
    stream: TcpStream,
    line: String,
    body: Vec<u8>,
}

fn request(fixture: &TcpListener) -> HttpRequest {
    let mut stream = accept(fixture);
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        bytes.push(byte[0]);
        assert!(bytes.len() <= 16 * 1024);
    }
    let headers = String::from_utf8(bytes).unwrap();
    let line = headers.lines().next().unwrap().to_owned();
    let mut length = 0;
    let mut authorization = 0;
    for header in headers.lines().skip(1).filter(|line| !line.is_empty()) {
        let (name, value) = header.split_once(':').expect("HTTP header");
        match name.to_ascii_lowercase().as_str() {
            "authorization" => {
                authorization += 1;
                assert_eq!(value.trim(), format!("Bearer {TOKEN}"));
            }
            "host" => assert_eq!(value.trim(), fixture.local_addr().unwrap().to_string()),
            "content-length" => length = value.trim().parse::<usize>().unwrap(),
            "content-type" if line.starts_with("POST ") => {
                assert_eq!(value.trim(), "application/json")
            }
            "transfer-encoding" => panic!("small fixed request must have a bounded content length"),
            _ => {}
        }
    }
    assert_eq!(authorization, 1);
    assert!(length <= 1024);
    let mut body = vec![0; length];
    stream.read_exact(&mut body).unwrap();
    HttpRequest { stream, line, body }
}

fn respond(stream: &mut TcpStream, status: u16, body: Value) {
    let body = body.to_string();
    write!(stream, "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    stream.flush().unwrap();
}

fn entity(name: &str, state: &str) -> Value {
    json!({"entity_id":name,"state":state,"attributes":{"friendly_name":name}})
}

fn live(socket: &mut WebSocket<TcpStream>, name: &str, state: Option<Value>) {
    ws_send(
        socket,
        json!({"id":1,"type":"event","event":{
        "event_type":"state_changed","data":{"entity_id":name,"old_state":null,"new_state":state}}}),
    );
}

fn item<'a>(frame: &'a Value, id: &str) -> Option<&'a Value> {
    frame["items"]
        .as_array()?
        .iter()
        .find(|item| item["id"] == id)
}

fn actions(frame: &Value) -> Vec<&Value> {
    frame["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["kind"] == "action")
        .collect()
}

fn connected(worker: &mut Worker, count: usize) -> Value {
    worker.until(|frame| {
        frame["type"] == "contributions"
            && item(frame, "connection").is_some_and(|item| item["value"] == "Connected")
            && actions(frame).len() == count
    })
}

fn initialize(
    worker: &mut Worker,
    fixture: &TcpListener,
    watch: &str,
    selected: Option<&str>,
) -> WebSocket<TcpStream> {
    initialize_configuration(worker, fixture, configuration(fixture, watch, selected))
}

fn media_configuration(
    fixture: &TcpListener,
    watch: &str,
    selected: Option<&str>,
    media: Option<&str>,
) -> Value {
    let mut frame = configuration(fixture, watch, selected);
    if let Some(media) = media {
        frame["configuration"]["values"]["media_player_entities"] = json!(media);
    }
    frame
}

fn initialize_configuration(
    worker: &mut Worker,
    fixture: &TcpListener,
    frame: Value,
) -> WebSocket<TcpStream> {
    worker.configure_frame(frame.clone());
    let mut names = Vec::new();
    for field in ["watch_entities", "action_entities", "media_player_entities"] {
        for name in frame["configuration"]["values"][field]
            .as_str()
            .unwrap_or("")
            .split([',', '\n'])
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    let socket = authorize(fixture, !names.is_empty());
    let mut observed = HashSet::new();
    for _ in &names {
        let mut request = request(fixture);
        let name = request
            .line
            .strip_prefix("GET /reverse/proxy/ha/api/states/")
            .and_then(|name| name.strip_suffix(" HTTP/1.1"))
            .expect("only explicitly selected state reads");
        assert!(names.contains(&name));
        assert!(
            observed.insert(name.to_owned()),
            "ordered union must not read duplicates"
        );
        assert!(request.body.is_empty());
        let state = if name.starts_with("media_player.") {
            "paused"
        } else {
            "unknown"
        };
        respond(&mut request.stream, 200, entity(name, state));
    }
    socket
}

fn service(fixture: &TcpListener, domain: &str, entity: &str) -> TcpStream {
    let method = if domain == "button" {
        "press"
    } else {
        "turn_on"
    };
    exact_service(fixture, domain, method, entity)
}

fn exact_service(fixture: &TcpListener, domain: &str, method: &str, entity: &str) -> TcpStream {
    let request = request(fixture);
    assert_eq!(
        request.line,
        format!("POST /reverse/proxy/ha/api/services/{domain}/{method} HTTP/1.1")
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&request.body).unwrap(),
        json!({"entity_id":entity})
    );
    request.stream
}

#[test]
fn omitted_and_empty_action_selection_keep_literal_read_targets_read_only() {
    for selected in [None, Some("")] {
        let fixture = listener();
        let mut worker = Worker::start();
        let _socket = initialize(
            &mut worker,
            &fixture,
            "button.do_not_supply_charger,scene.inverter_on",
            selected,
        );
        let frame = worker
            .until(|frame| item(frame, "entity-1").is_some_and(|item| item["value"] == "Unknown"));
        assert!(actions(&frame).is_empty());
        worker.action("read-only", "ha-action-0", 5000);
        worker.error("read-only", "invalid_action");
        no_request(&fixture, Duration::from_millis(100));
        worker.stop(false);
    }
}

#[test]
fn explicit_targets_use_ordered_union_and_exact_self_contained_service_presets() {
    let fixture = listener();
    let mut worker = Worker::start();
    let _socket = initialize(
        &mut worker,
        &fixture,
        "sensor.power,button.do_not_supply_charger",
        Some("scene.inverter_on,button.do_not_supply_charger"),
    );
    let frame = connected(&mut worker, 2);
    for (index, name) in [
        "sensor.power",
        "button.do_not_supply_charger",
        "scene.inverter_on",
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(
            item(&frame, &format!("entity-{index}")).unwrap()["title"],
            *name
        );
    }
    for (index, domain, name, label) in [
        (0, "scene", "scene.inverter_on", "Activate"),
        (1, "button", "button.do_not_supply_charger", "Press"),
    ] {
        let action_id = format!("ha-action-{index}");
        let advertised = actions(&frame)
            .into_iter()
            .find(|item| item["action_id"] == action_id)
            .unwrap();
        assert_eq!(advertised["params"], json!({}));
        assert_eq!(advertised["label"], format!("{label} {name}"));
        assert_eq!(advertised["title"], name);
        worker.action(&format!("service-{index}"), &action_id, 5000);
        let mut pending = service(&fixture, domain, name);
        respond(&mut pending, 200, json!([{ "ignored":PRIVATE_BODY }]));
        worker.success(&format!("service-{index}"));
    }
    no_request(&fixture, Duration::from_millis(100));
    worker.stop(false);
}

#[test]
fn invalid_action_configuration_never_connects_or_acknowledges() {
    let fixture = listener();
    let too_many = (0..17)
        .map(|index| format!("button.b{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let watch = (0..32)
        .map(|index| format!("sensor.s{index}"))
        .collect::<Vec<_>>()
        .join(",");
    for (watch, selected) in [
        ("", "switch.inverter"),
        ("", "input_boolean.do_not_supply_charger"),
        ("", "button.press/other"),
        ("", "scene.%2e%2e"),
        ("", "Button.upper"),
        ("", too_many.as_str()),
        (watch.as_str(), "scene.extra"),
    ] {
        let mut worker = Worker::start();
        worker.send(hello());
        assert_eq!(worker.next()["type"], "ready");
        worker.send(configuration(&fixture, watch, Some(selected)));
        worker.finish(false);
        assert!(worker.frames.try_iter().next().is_none());
        no_request(&fixture, Duration::ZERO);
    }
}

#[test]
fn sixteen_unique_action_targets_and_overlapping_duplicates_preserve_fixed_indices() {
    let fixture = listener();
    let mut worker = Worker::start();
    let selected = (0..16)
        .map(|index| format!("button.b{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let selected = format!("{selected},button.b0,button.b15");
    let _socket = initialize(&mut worker, &fixture, "button.b15", Some(&selected));
    let frame = connected(&mut worker, 16);
    assert_eq!(frame["items"].as_array().unwrap().len(), 33);
    assert_eq!(item(&frame, "entity-0").unwrap()["title"], "button.b15");
    assert_eq!(item(&frame, "ha-action-0").unwrap()["title"], "button.b0");
    assert_eq!(item(&frame, "ha-action-15").unwrap()["title"], "button.b15");
    worker.action("last-index", "ha-action-15", 5000);
    let mut pending = service(&fixture, "button", "button.b15");
    respond(&mut pending, 200, json!([]));
    worker.success("last-index");
    no_request(&fixture, Duration::ZERO);
    worker.stop(false);
}

#[test]
fn selected_target_is_unavailable_before_first_state_and_after_initial_not_found() {
    let fixture = listener();
    let mut worker = Worker::start();
    worker.configure(&fixture, "", Some("scene.missing"));
    let _socket = authorize(&fixture, true);
    let mut initial = request(&fixture);
    assert_eq!(
        initial.line,
        "GET /reverse/proxy/ha/api/states/scene.missing HTTP/1.1"
    );
    connected(&mut worker, 0);
    worker.action("before-state", "ha-action-0", 5000);
    worker.error("before-state", "unavailable");
    no_request(&fixture, Duration::ZERO);
    respond(
        &mut initial.stream,
        404,
        json!({"message":"Entity not found"}),
    );
    worker
        .until(|frame| item(frame, "entity-0").is_some_and(|item| item["value"] == "Unavailable"));
    worker.action("missing", "ha-action-0", 5000);
    worker.error("missing", "unavailable");
    no_request(&fixture, Duration::ZERO);
    worker.stop(false);
}

#[test]
fn cancel_before_action_prevents_submission_and_remains_correlated() {
    let fixture = listener();
    let mut worker = Worker::start();
    let _socket = initialize(&mut worker, &fixture, "", Some("button.selected"));
    connected(&mut worker, 1);
    worker.send(json!({"type":"cancel","request_id":"pre-canceled"}));
    // A distinct rejected action proves the earlier Cancel has been consumed.
    worker.action("barrier", "missing", 5000);
    worker.error("barrier", "invalid_action");
    worker.action("pre-canceled", "ha-action-0", 5000);
    worker.error("pre-canceled", "canceled");
    no_request(&fixture, Duration::ZERO);
    worker.stop(false);
}

#[test]
fn changed_params_arbitrary_targets_and_unadvertised_ids_never_issue_requests() {
    let fixture = listener();
    let mut worker = Worker::start();
    let _socket = initialize(&mut worker, &fixture, "", Some("button.selected"));
    connected(&mut worker, 1);
    for (index, action, params) in [
        (0, "ha-action-0", json!({"entity_id":"button.other"})),
        (1, "ha-action-0", json!({"service":"scene.turn_on"})),
        (2, "ha-action-1", json!({})),
        (3, "button.press", json!({})),
        (4, "ha-action-00", json!({})),
    ] {
        let request = format!("invalid-{index}");
        worker.send(action_frame(&request, action, params, 5000));
        worker.error(&request, "invalid_action");
        no_request(&fixture, Duration::ZERO);
    }
    worker.stop(false);
}

#[test]
fn unavailable_deleted_and_disconnected_targets_revoke_actions() {
    let fixture = listener();
    let mut worker = Worker::start();
    let mut socket = initialize(&mut worker, &fixture, "", Some("button.selected"));
    connected(&mut worker, 1);
    for (index, state) in [Some(entity("button.selected", "unavailable")), None]
        .into_iter()
        .enumerate()
    {
        live(&mut socket, "button.selected", state);
        connected(&mut worker, 0);
        worker.action(&format!("unavailable-{index}"), "ha-action-0", 5000);
        worker.error(&format!("unavailable-{index}"), "unavailable");
        no_request(&fixture, Duration::ZERO);
        live(
            &mut socket,
            "button.selected",
            Some(entity("button.selected", "unknown")),
        );
        connected(&mut worker, 1);
    }
    socket.close(None).unwrap();
    drop(socket);
    let frame = worker.until(|frame| {
        item(frame, "connection").is_some_and(|item| item["value"] == "Disconnected")
    });
    assert!(actions(&frame).is_empty());
    worker.action("disconnected", "ha-action-0", 5000);
    worker.error("disconnected", "unavailable");
    no_request(&fixture, Duration::ZERO);
    worker.stop(false);
}

#[test]
fn unsuccessful_service_statuses_are_private_unknown_outcomes_without_retries() {
    for status in [400, 404, 429, 500, 503] {
        let fixture = listener();
        let mut worker = Worker::start();
        let _socket = initialize(&mut worker, &fixture, "", Some("button.selected"));
        connected(&mut worker, 1);
        worker.action("failure", "ha-action-0", 5000);
        let mut pending = service(&fixture, "button", "button.selected");
        respond(
            &mut pending,
            status,
            json!({"error":PRIVATE_BODY,"token":TOKEN}),
        );
        worker.error("failure", "outcome_unknown");
        no_request(&fixture, Duration::from_millis(150));
        worker.stop(false);
    }
}

#[test]
fn service_redirect_never_forwards_credentials_or_repeats_the_post() {
    let fixture = listener();
    let destination = listener();
    let mut worker = Worker::start();
    let _socket = initialize(&mut worker, &fixture, "", Some("scene.selected"));
    connected(&mut worker, 1);
    for (index, status) in [301, 302, 307, 308].into_iter().enumerate() {
        let id = format!("redirect-{index}");
        worker.action(&id, "ha-action-0", 5000);
        let mut pending = service(&fixture, "scene", "scene.selected");
        write!(pending, "HTTP/1.1 {status} Redirect\r\nLocation: http://{}/capture\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", destination.local_addr().unwrap()).unwrap();
        pending.flush().unwrap();
        worker.error(&id, "outcome_unknown");
        no_request(&destination, Duration::from_millis(50));
        no_request(&fixture, Duration::ZERO);
    }
    worker.stop(false);
}

#[test]
fn service_auth_rejection_revokes_actions_and_cancels_other_submitted_requests() {
    for status in [401, 403] {
        let fixture = listener();
        let mut worker = Worker::start();
        let _socket = initialize(&mut worker, &fixture, "", Some("button.selected"));
        connected(&mut worker, 1);
        worker.action("auth", "ha-action-0", 5000);
        let mut rejected = service(&fixture, "button", "button.selected");
        worker.action("sibling", "ha-action-0", 5000);
        let _sibling = service(&fixture, "button", "button.selected");
        respond(&mut rejected, status, json!({"error":PRIVATE_BODY}));
        worker.error("auth", "outcome_unknown");
        worker.error("sibling", "outcome_unknown");
        let frame = worker.until(|frame| {
            item(frame, "connection").is_some_and(|item| item["value"] == "Authentication rejected")
        });
        assert!(actions(&frame).is_empty());
        worker.action("after-auth", "ha-action-0", 5000);
        worker.error("after-auth", "unavailable");
        no_request(&fixture, Duration::from_millis(150));
        worker.stop(false);
    }
}

#[test]
fn lost_response_is_never_retried_and_new_request_requires_explicit_host_action() {
    let fixture = listener();
    let mut worker = Worker::start();
    let _socket = initialize(&mut worker, &fixture, "", Some("button.selected"));
    connected(&mut worker, 1);
    worker.action("lost", "ha-action-0", 5000);
    drop(service(&fixture, "button", "button.selected"));
    worker.error("lost", "outcome_unknown");
    no_request(&fixture, Duration::from_millis(1200));
    worker.action("lost", "ha-action-0", 5000);
    worker.error("lost", "invalid_action");
    no_request(&fixture, Duration::ZERO);
    worker.action("explicit-next", "ha-action-0", 5000);
    let mut pending = service(&fixture, "button", "button.selected");
    respond(&mut pending, 200, json!([]));
    worker.success("explicit-next");
    worker.stop(false);
}

#[test]
fn oversized_declared_and_streamed_service_responses_are_bounded_unknown_outcomes() {
    for declared in [true, false] {
        let fixture = listener();
        let mut worker = Worker::start();
        let _socket = initialize(&mut worker, &fixture, "", Some("button.selected"));
        connected(&mut worker, 1);
        worker.action("oversize", "ha-action-0", 5000);
        let mut pending = service(&fixture, "button", "button.selected");
        if declared {
            write!(
                pending,
                "HTTP/1.1 200 OK\r\nContent-Length: 1048577\r\n\r\n"
            )
            .unwrap();
            pending.flush().unwrap();
        } else {
            write!(
                pending,
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n"
            )
            .unwrap();
            let chunk = vec![b'x'; 64 * 1024];
            for _ in 0..17 {
                if write!(pending, "10000\r\n")
                    .and_then(|()| pending.write_all(&chunk))
                    .and_then(|()| pending.write_all(b"\r\n"))
                    .is_err()
                {
                    break;
                }
            }
            let _ = pending.flush();
        }
        worker.error("oversize", "outcome_unknown");
        no_request(&fixture, Duration::from_millis(100));
        worker.stop(false);
    }
}

#[test]
fn two_active_services_reject_overload_without_queue_and_live_reads_stay_responsive() {
    let fixture = listener();
    let mut worker = Worker::start();
    let mut socket = initialize(
        &mut worker,
        &fixture,
        "sensor.power",
        Some("button.selected"),
    );
    connected(&mut worker, 1);
    worker.action("first", "ha-action-0", 5000);
    let mut first = service(&fixture, "button", "button.selected");
    worker.action("second", "ha-action-0", 5000);
    let mut second = service(&fixture, "button", "button.selected");
    worker.action("overflow", "ha-action-0", 5000);
    worker.error("overflow", "overloaded");
    live(
        &mut socket,
        "sensor.power",
        Some(entity("sensor.power", "72")),
    );
    worker.until(|frame| item(frame, "entity-0").is_some_and(|item| item["value"] == 72.0));
    no_request(&fixture, Duration::ZERO);
    respond(&mut second, 200, json!([]));
    worker.success("second");
    respond(&mut first, 200, json!([]));
    worker.success("first");
    no_request(&fixture, Duration::from_millis(150));
    worker.action("fresh", "ha-action-0", 5000);
    let mut fresh = service(&fixture, "button", "button.selected");
    respond(&mut fresh, 200, json!([]));
    worker.success("fresh");
    worker.stop(false);
}

#[test]
fn cancel_and_original_deadline_stop_stalled_responses_without_retries() {
    for cancel in [false, true] {
        let fixture = listener();
        let mut worker = Worker::start();
        let _socket = initialize(&mut worker, &fixture, "", Some("button.selected"));
        connected(&mut worker, 1);
        let started = Instant::now();
        worker.action("stalled", "ha-action-0", if cancel { 5000 } else { 250 });
        let mut stalled = service(&fixture, "button", "button.selected");
        write!(stalled, "HTTP/1.1 200 OK\r\nContent-Length: 1024\r\n\r\n[").unwrap();
        stalled.flush().unwrap();
        if cancel {
            worker.send(json!({"type":"cancel","request_id":"stalled"}));
        }
        worker.error("stalled", "outcome_unknown");
        assert!(started.elapsed() < Duration::from_secs(2));
        no_request(&fixture, Duration::from_millis(150));
        worker.action("after-stall", "ha-action-0", 5000);
        let mut next = service(&fixture, "button", "button.selected");
        respond(&mut next, 200, json!([]));
        worker.success("after-stall");
        worker.stop(false);
    }
}

#[test]
fn websocket_disconnect_cancels_submitted_service_and_prevents_late_success() {
    let fixture = listener();
    let mut worker = Worker::start();
    let mut socket = initialize(&mut worker, &fixture, "", Some("scene.selected"));
    connected(&mut worker, 1);
    worker.action("disconnected-pending", "ha-action-0", 5000);
    let _pending = service(&fixture, "scene", "scene.selected");
    socket.close(None).unwrap();
    drop(socket);
    worker.error("disconnected-pending", "outcome_unknown");
    worker.action("new-disconnected", "ha-action-0", 5000);
    worker.error("new-disconnected", "unavailable");
    no_request(&fixture, Duration::ZERO);
    worker.stop(false);
}

#[test]
fn eof_and_shutdown_cancel_two_stalled_service_bodies_promptly() {
    for eof in [true, false] {
        let fixture = listener();
        let mut worker = Worker::start();
        let _socket = initialize(&mut worker, &fixture, "", Some("button.selected"));
        connected(&mut worker, 1);
        worker.action("one", "ha-action-0", 30000);
        let _one = service(&fixture, "button", "button.selected");
        worker.action("two", "ha-action-0", 30000);
        let _two = service(&fixture, "button", "button.selected");
        worker.stop(eof);
        no_request(&fixture, Duration::ZERO);
    }
}

#[test]
fn heartbeat_remains_responsive_during_a_submitted_service() {
    let fixture = listener();
    let mut worker = Worker::start();
    let mut socket = initialize(&mut worker, &fixture, "", Some("button.selected"));
    connected(&mut worker, 1);
    worker.action("heartbeat-pending", "ha-action-0", 30000);
    let mut pending = service(&fixture, "button", "button.selected");
    socket
        .get_mut()
        .set_read_timeout(Some(Duration::from_secs(24)))
        .unwrap();
    let heartbeat = ws_json(&mut socket);
    assert_eq!(heartbeat["type"], "ping");
    assert!(heartbeat["id"].as_u64().unwrap() >= 2);
    ws_send(&mut socket, json!({"type":"pong","id":heartbeat["id"]}));
    respond(&mut pending, 200, json!([]));
    worker.success("heartbeat-pending");
    no_request(&fixture, Duration::ZERO);
    worker.stop(false);
}

#[test]
fn strict_action_and_cancel_frames_fail_closed_before_any_service_request() {
    let fixture = listener();
    let mut extra = action_frame("extra", "ha-action-0", json!({}), 5000);
    extra["url"] = json!("http://127.0.0.1/forbidden");
    for invalid in [
        action_frame("zero", "ha-action-0", json!({}), 0),
        action_frame("too-long", "ha-action-0", json!({}), 30001),
        action_frame("bad request", "ha-action-0", json!({}), 5000),
        action_frame("array-params", "ha-action-0", json!([]), 5000),
        action_frame("null-params", "ha-action-0", Value::Null, 5000),
        extra,
        json!({"type":"cancel","request_id":"valid","extra":true}),
    ] {
        let mut worker = Worker::start();
        let _socket = initialize(&mut worker, &fixture, "", Some("button.selected"));
        connected(&mut worker, 1);
        worker.send(invalid);
        worker.finish(false);
        no_request(&fixture, Duration::ZERO);
    }
}

#[test]
fn output_pressure_does_not_block_eof_or_shutdown_with_submitted_services() {
    for eof in [true, false] {
        let fixture = listener();
        let mut worker = Worker::start();
        let watch = (0..31)
            .map(|index| format!("sensor.s{index}"))
            .collect::<Vec<_>>()
            .join(",");
        let mut socket = initialize(&mut worker, &fixture, &watch, Some("button.selected"));
        connected(&mut worker, 1);
        for index in 0..31 {
            let name = format!("sensor.s{index}");
            let mut value = entity(&name, &"x".repeat(512));
            value["attributes"]["friendly_name"] = json!("y".repeat(128));
            live(&mut socket, &name, Some(value));
        }
        worker.until(|frame| {
            item(frame, "entity-30").is_some_and(|item| item["text"] == "x".repeat(512))
        });
        worker.action("pressure-pending", "ha-action-0", 30000);
        let _pending = service(&fixture, "button", "button.selected");
        worker.paused.store(true, Ordering::Release);
        // Repeated large snapshots exceed ordinary platform pipe capacities.
        // The reader may finish one in-flight read before observing the pause.
        for index in 0..8 {
            live(
                &mut socket,
                "sensor.s0",
                Some(entity("sensor.s0", &format!("{index}{}", "x".repeat(500)))),
            );
            thread::sleep(Duration::from_millis(275));
        }
        assert!(worker.process.try_wait().unwrap().is_none());
        worker.stop(eof);
        no_request(&fixture, Duration::ZERO);
    }
}

#[test]
fn media_omitted_or_empty_selection_keeps_watched_players_read_only() {
    for selected in [None, Some("")] {
        let fixture = listener();
        let mut worker = Worker::start();
        let _socket = initialize_configuration(
            &mut worker,
            &fixture,
            media_configuration(
                &fixture,
                "media_player.do_not_supply_charger",
                None,
                selected,
            ),
        );
        let frame = worker
            .until(|frame| item(frame, "entity-0").is_some_and(|item| item["text"] == "paused"));
        assert!(actions(&frame).is_empty());
        for operation in ["play", "pause", "stop"] {
            worker.action(operation, &format!("ha-media-0-{operation}"), 5000);
            worker.error(operation, "invalid_action");
        }
        no_request(&fixture, Duration::from_millis(100));
        worker.stop(false);
    }
}

#[test]
fn media_exact_transport_presets_preserve_button_scene_ids_and_literal_targets() {
    let fixture = listener();
    let mut worker = Worker::start();
    let _socket = initialize_configuration(
        &mut worker,
        &fixture,
        media_configuration(
            &fixture,
            "sensor.power,media_player.inverter_on",
            Some("scene.wakeup,button.do_not_supply_charger"),
            Some("media_player.television,media_player.inverter_on"),
        ),
    );
    let frame = connected(&mut worker, 8);
    for (index, name) in [
        "sensor.power",
        "media_player.inverter_on",
        "scene.wakeup",
        "button.do_not_supply_charger",
        "media_player.television",
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(
            item(&frame, &format!("entity-{index}")).unwrap()["title"],
            *name
        );
    }
    for (index, name) in ["media_player.television", "media_player.inverter_on"]
        .iter()
        .enumerate()
    {
        for (operation, label) in [("play", "Play"), ("pause", "Pause"), ("stop", "Stop")] {
            let id = format!("ha-media-{index}-{operation}");
            let preset = item(&frame, &id).unwrap();
            assert_eq!(preset["action_id"], id);
            assert_eq!(preset["params"], json!({}));
            assert_eq!(preset["title"], *name);
            assert_eq!(preset["label"], format!("{label} {name}"));
            worker.action(&id, &id, 5000);
            let mut pending = exact_service(
                &fixture,
                "media_player",
                &format!("media_{operation}"),
                name,
            );
            respond(&mut pending, 200, json!([{ "private": PRIVATE_BODY }]));
            worker.success(&id);
        }
    }
    for (index, domain, name) in [
        (0, "scene", "scene.wakeup"),
        (1, "button", "button.do_not_supply_charger"),
    ] {
        let id = format!("ha-action-{index}");
        assert_eq!(item(&frame, &id).unwrap()["title"], name);
        worker.action(&id, &id, 5000);
        let mut pending = service(&fixture, domain, name);
        respond(&mut pending, 200, json!([]));
        worker.success(&id);
    }
    no_request(&fixture, Duration::ZERO);
    worker.stop(false);
}

#[test]
fn media_invalid_domains_overlong_lists_and_combined_watch_overflow_never_connect() {
    let fixture = listener();
    let five = (0..5)
        .map(|index| format!("media_player.p{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let watched = (0..31)
        .map(|index| format!("sensor.s{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let overlong = " ".repeat(4097);
    for (watch, media) in [
        ("", "button.selected"),
        ("", "scene.selected"),
        ("", "input_boolean.do_not_supply_charger"),
        ("", "media_player.invalid/path"),
        ("", "Media_player.upper"),
        ("", five.as_str()),
        ("", overlong.as_str()),
        (watched.as_str(), "media_player.one,media_player.two"),
    ] {
        let mut worker = Worker::start();
        worker.send(hello());
        assert_eq!(worker.next()["type"], "ready");
        worker.send(media_configuration(&fixture, watch, None, Some(media)));
        worker.finish(false);
        assert!(worker.frames.try_iter().next().is_none());
        no_request(&fixture, Duration::ZERO);
    }
    let watch = (0..16)
        .map(|index| format!("sensor.s{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let actions = (0..16)
        .map(|index| format!("button.b{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let mut worker = Worker::start();
    worker.send(hello());
    assert_eq!(worker.next()["type"], "ready");
    worker.send(media_configuration(
        &fixture,
        &watch,
        Some(&actions),
        Some("media_player.extra"),
    ));
    worker.finish(false);
    assert!(worker.frames.try_iter().next().is_none());
    no_request(&fixture, Duration::ZERO);
}

#[test]
fn media_four_unique_players_with_duplicates_fit_maximum_combined_snapshot() {
    let fixture = listener();
    let mut worker = Worker::start();
    let watch = (0..12)
        .map(|index| format!("sensor.s{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let watch = format!("media_player.p3,{watch},media_player.p3");
    let buttons = (0..16)
        .map(|index| format!("button.b{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let mut socket = initialize_configuration(
        &mut worker,
        &fixture,
        media_configuration(
            &fixture,
            &watch,
            Some(&buttons),
            Some(
                "media_player.p0,\nmedia_player.p1,media_player.p2,media_player.p3,media_player.p0",
            ),
        ),
    );
    let frame = connected(&mut worker, 28);
    assert_eq!(frame["items"].as_array().unwrap().len(), 61);
    assert_eq!(
        item(&frame, "entity-0").unwrap()["title"],
        "media_player.p3"
    );
    assert_eq!(item(&frame, "ha-action-15").unwrap()["title"], "button.b15");
    assert_eq!(
        item(&frame, "ha-media-0-play").unwrap()["title"],
        "media_player.p0"
    );
    assert_eq!(
        item(&frame, "ha-media-3-stop").unwrap()["title"],
        "media_player.p3"
    );
    for value in frame["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["id"].as_str().unwrap().starts_with("entity-"))
    {
        let name = value["title"].as_str().unwrap();
        let state = if name.starts_with("sensor.") {
            "x".repeat(512)
        } else {
            "playing".into()
        };
        let mut value = entity(name, &state);
        value["attributes"]["friendly_name"] = json!("🌞".repeat(128));
        live(&mut socket, name, Some(value));
    }
    let bounded = worker.until(|frame| {
        item(frame, "entity-31").is_some_and(|item| item["title"] == "🌞".repeat(32))
    });
    assert_eq!(bounded["items"].as_array().unwrap().len(), 61);
    assert!(serde_json::to_vec(&bounded).unwrap().len() + 1 < MAX_FRAME);
    let mut ids = HashSet::new();
    for value in bounded["items"].as_array().unwrap() {
        assert!(ids.insert(value["id"].as_str().unwrap()));
        assert!(value["title"].as_str().unwrap().len() <= 128);
        if value["kind"] == "action" {
            assert!(value["label"].as_str().unwrap().len() <= 128);
            assert_eq!(value["params"], json!({}));
        }
    }
    no_request(&fixture, Duration::ZERO);
    worker.stop(false);
}

#[test]
fn media_actions_withdraw_until_observed_and_for_unknown_unavailable_delete_disconnect() {
    let fixture = listener();
    let mut worker = Worker::start();
    worker.configure_frame(media_configuration(
        &fixture,
        "",
        None,
        Some("media_player.selected"),
    ));
    let mut socket = authorize(&fixture, true);
    let mut initial = request(&fixture);
    assert_eq!(
        initial.line,
        "GET /reverse/proxy/ha/api/states/media_player.selected HTTP/1.1"
    );
    connected(&mut worker, 0);
    worker.action("waiting", "ha-media-0-play", 5000);
    worker.error("waiting", "unavailable");
    respond(
        &mut initial.stream,
        404,
        json!({"message":"Entity not found"}),
    );
    worker
        .until(|frame| item(frame, "entity-0").is_some_and(|item| item["value"] == "Unavailable"));
    worker.action("missing", "ha-media-0-play", 5000);
    worker.error("missing", "unavailable");
    for (index, invalid) in [Some("unknown"), Some("unavailable"), None]
        .into_iter()
        .enumerate()
    {
        // Legacy transport controls do not depend on supported_features flags.
        let mut available = entity("media_player.selected", "off");
        available["attributes"]["supported_features"] = json!(0);
        live(&mut socket, "media_player.selected", Some(available));
        connected(&mut worker, 3);
        live(
            &mut socket,
            "media_player.selected",
            invalid.map(|state| entity("media_player.selected", state)),
        );
        connected(&mut worker, 0);
        for operation in ["play", "pause", "stop"] {
            let request = format!("withdrawn-{index}-{operation}");
            worker.action(&request, &format!("ha-media-0-{operation}"), 5000);
            worker.error(&request, "unavailable");
        }
        no_request(&fixture, Duration::ZERO);
    }
    live(
        &mut socket,
        "media_player.selected",
        Some(entity("media_player.selected", "playing")),
    );
    connected(&mut worker, 3);
    socket.close(None).unwrap();
    drop(socket);
    let frame = worker.until(|frame| {
        item(frame, "connection").is_some_and(|item| item["value"] == "Disconnected")
    });
    assert!(actions(&frame).is_empty());
    worker.action("disconnected-media", "ha-media-0-stop", 5000);
    worker.error("disconnected-media", "unavailable");
    no_request(&fixture, Duration::ZERO);
    worker.stop(false);
}

#[test]
fn media_changed_params_and_unadvertised_service_ids_never_issue_requests() {
    let fixture = listener();
    let mut worker = Worker::start();
    let _socket = initialize_configuration(
        &mut worker,
        &fixture,
        media_configuration(&fixture, "", None, Some("media_player.selected")),
    );
    connected(&mut worker, 3);
    for (index, action, params) in [
        (
            0,
            "ha-media-0-play",
            json!({"entity_id":"media_player.other"}),
        ),
        (
            1,
            "ha-media-0-play",
            json!({"service":"media_player.volume_set"}),
        ),
        (2, "ha-media-0-play", json!({"volume_level":1})),
        (3, "ha-media-0-volume", json!({})),
        (4, "ha-media-1-play", json!({})),
        (5, "ha-media-00-play", json!({})),
        (6, "ha-media-0-Play", json!({})),
        (7, "media_player.media_play", json!({})),
        (8, "ha-action-0", json!({})),
    ] {
        let request = format!("media-invalid-{index}");
        worker.send(action_frame(&request, action, params, 5000));
        worker.error(&request, "invalid_action");
        no_request(&fixture, Duration::ZERO);
    }
    worker.stop(false);
}

#[test]
fn media_lost_response_has_unknown_outcome_and_no_automatic_retry() {
    let fixture = listener();
    let mut worker = Worker::start();
    let _socket = initialize_configuration(
        &mut worker,
        &fixture,
        media_configuration(&fixture, "", None, Some("media_player.selected")),
    );
    connected(&mut worker, 3);
    worker.action("media-lost", "ha-media-0-play", 5000);
    drop(exact_service(
        &fixture,
        "media_player",
        "media_play",
        "media_player.selected",
    ));
    worker.error("media-lost", "outcome_unknown");
    no_request(&fixture, Duration::from_millis(1200));
    worker.action("media-lost", "ha-media-0-play", 5000);
    worker.error("media-lost", "invalid_action");
    no_request(&fixture, Duration::ZERO);
    worker.action("media-explicit-next", "ha-media-0-pause", 5000);
    let mut pending = exact_service(
        &fixture,
        "media_player",
        "media_pause",
        "media_player.selected",
    );
    respond(&mut pending, 200, json!([]));
    worker.success("media-explicit-next");
    worker.stop(false);
}

#[test]
fn media_and_button_requests_share_two_active_slots_and_cancellation_deadlines() {
    let fixture = listener();
    let mut worker = Worker::start();
    let _socket = initialize_configuration(
        &mut worker,
        &fixture,
        media_configuration(
            &fixture,
            "",
            Some("button.selected"),
            Some("media_player.selected"),
        ),
    );
    connected(&mut worker, 4);
    worker.action("media-pending", "ha-media-0-play", 5000);
    let _media = exact_service(
        &fixture,
        "media_player",
        "media_play",
        "media_player.selected",
    );
    worker.action("button-pending", "ha-action-0", 5000);
    let _button = service(&fixture, "button", "button.selected");
    worker.action("media-overloaded", "ha-media-0-stop", 5000);
    worker.error("media-overloaded", "overloaded");
    no_request(&fixture, Duration::ZERO);
    for request in ["button-pending", "media-pending"] {
        worker.send(json!({"type":"cancel","request_id":request}));
        worker.error(request, "outcome_unknown");
    }
    no_request(&fixture, Duration::from_millis(100));
    let started = Instant::now();
    worker.action("media-deadline", "ha-media-0-stop", 250);
    let _deadline = exact_service(
        &fixture,
        "media_player",
        "media_stop",
        "media_player.selected",
    );
    worker.error("media-deadline", "outcome_unknown");
    assert!(started.elapsed() < Duration::from_secs(2));
    no_request(&fixture, Duration::from_millis(100));
    worker.stop(false);
}

#[test]
fn media_auth_rejection_cancels_sibling_and_revokes_all_action_families() {
    let fixture = listener();
    let mut worker = Worker::start();
    let _socket = initialize_configuration(
        &mut worker,
        &fixture,
        media_configuration(
            &fixture,
            "",
            Some("scene.selected"),
            Some("media_player.selected"),
        ),
    );
    connected(&mut worker, 4);
    worker.action("media-auth", "ha-media-0-play", 5000);
    let mut media = exact_service(
        &fixture,
        "media_player",
        "media_play",
        "media_player.selected",
    );
    worker.action("scene-sibling", "ha-action-0", 5000);
    let _scene = service(&fixture, "scene", "scene.selected");
    respond(&mut media, 401, json!({"message":PRIVATE_BODY}));
    worker.error("media-auth", "outcome_unknown");
    worker.error("scene-sibling", "outcome_unknown");
    let frame = worker.until(|frame| {
        item(frame, "connection").is_some_and(|item| item["value"] == "Authentication rejected")
    });
    assert!(actions(&frame).is_empty());
    for (request, action) in [
        ("media-after-auth", "ha-media-0-pause"),
        ("scene-after-auth", "ha-action-0"),
    ] {
        worker.action(request, action, 5000);
        worker.error(request, "unavailable");
    }
    no_request(&fixture, Duration::from_millis(100));
    worker.stop(false);
}
