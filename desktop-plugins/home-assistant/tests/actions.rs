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

fn cover_entity(name: &str, state: &str, features: Value) -> Value {
    let mut value = entity(name, state);
    value["attributes"]["supported_features"] = features;
    value
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

fn binary_configuration(fixture: &TcpListener, watch: &str, selected: Option<&str>) -> Value {
    let mut frame = configuration(fixture, watch, None);
    if let Some(selected) = selected {
        frame["configuration"]["values"]["binary_entities"] = json!(selected);
    }
    frame
}

fn cover_configuration(fixture: &TcpListener, watch: &str, selected: Option<&str>) -> Value {
    let mut frame = configuration(fixture, watch, None);
    if let Some(selected) = selected {
        frame["configuration"]["values"]["cover_entities"] = json!(selected);
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
    for field in [
        "watch_entities",
        "action_entities",
        "media_player_entities",
        "binary_entities",
        "cover_entities",
    ] {
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
        let binary_selected = frame["configuration"]["values"]["binary_entities"]
            .as_str()
            .unwrap_or("")
            .split([',', '\n'])
            .map(str::trim)
            .any(|selected| selected == name);
        let cover_selected = frame["configuration"]["values"]["cover_entities"]
            .as_str()
            .unwrap_or("")
            .split([',', '\n'])
            .map(str::trim)
            .any(|selected| selected == name);
        let state = if cover_selected {
            "closed"
        } else if binary_selected {
            "off"
        } else if name.starts_with("media_player.") {
            "paused"
        } else {
            "unknown"
        };
        let value = if cover_selected {
            cover_entity(name, state, json!(11))
        } else {
            entity(name, state)
        };
        respond(&mut request.stream, 200, value);
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

#[test]
fn binary_omitted_and_empty_selection_keep_observed_on_off_entities_read_only() {
    for selected in [None, Some("")] {
        let fixture = listener();
        let mut worker = Worker::start();
        let names = [
            "switch.selected",
            "input_boolean.do_not_supply_charger",
            "light.selected",
        ];
        let mut socket = initialize_configuration(
            &mut worker,
            &fixture,
            binary_configuration(&fixture, &names.join(","), selected),
        );
        for name in names {
            live(&mut socket, name, Some(entity(name, "on")));
        }
        let frame =
            worker.until(|frame| item(frame, "entity-2").is_some_and(|item| item["text"] == "on"));
        assert!(actions(&frame).is_empty());
        for operation in ["on", "off"] {
            worker.action(operation, &format!("ha-binary-0-{operation}"), 5000);
            worker.error(operation, "invalid_action");
        }
        no_request(&fixture, Duration::from_millis(100));
        worker.stop(false);
    }
}

#[test]
fn binary_all_six_fixed_routes_preserve_other_action_ids_and_literal_flag_targets() {
    let fixture = listener();
    let mut worker = Worker::start();
    let names = [
        "switch.inverter_on",
        "input_boolean.do_not_supply_charger",
        "light.reading",
    ];
    let mut config = media_configuration(
        &fixture,
        "sensor.power,input_boolean.do_not_supply_charger",
        Some("scene.evening,button.trigger"),
        Some("media_player.television"),
    );
    config["configuration"]["values"]["binary_entities"] = json!(names.join(","));
    let mut socket = initialize_configuration(&mut worker, &fixture, config);
    let frame = connected(&mut worker, 11);
    for (index, name) in [
        "sensor.power",
        names[1],
        "scene.evening",
        "button.trigger",
        "media_player.television",
        names[0],
        names[2],
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(
            item(&frame, &format!("entity-{index}")).unwrap()["title"],
            *name
        );
    }
    assert_eq!(
        item(&frame, "ha-action-0").unwrap()["title"],
        "scene.evening"
    );
    assert_eq!(
        item(&frame, "ha-action-1").unwrap()["title"],
        "button.trigger"
    );
    assert_eq!(
        item(&frame, "ha-media-0-play").unwrap()["title"],
        "media_player.television"
    );
    for (index, name) in names.iter().enumerate() {
        let domain = name.split_once('.').unwrap().0;
        for operation in ["on", "off"] {
            let id = format!("ha-binary-{index}-{operation}");
            let preset = item(&frame, &id).unwrap();
            assert_eq!(preset["action_id"], id);
            assert_eq!(preset["params"], json!({}));
            assert_eq!(preset["label"], format!("Turn {operation} {name}"));
            worker.action(&id, &id, 5000);
            let mut pending = exact_service(&fixture, domain, &format!("turn_{operation}"), name);
            respond(&mut pending, 200, json!([]));
            worker.success(&id);
        }
        live(&mut socket, name, Some(entity(name, "on")));
    }
    let on = worker.until(|frame| item(frame, "entity-6").is_some_and(|item| item["text"] == "on"));
    assert_eq!(
        actions(&on).len(),
        11,
        "both absolute commands remain available while on"
    );
    for (id, domain, operation, name) in [
        ("ha-action-0", "scene", "turn_on", "scene.evening"),
        ("ha-action-1", "button", "press", "button.trigger"),
        (
            "ha-media-0-play",
            "media_player",
            "media_play",
            "media_player.television",
        ),
    ] {
        worker.action(id, id, 5000);
        let mut pending = exact_service(&fixture, domain, operation, name);
        respond(&mut pending, 200, json!([]));
        worker.success(id);
    }
    no_request(&fixture, Duration::ZERO);
    worker.stop(false);
}

fn success_without_state_change(worker: &mut Worker, request: &str, expected: &str) {
    let until = Instant::now() + WAIT;
    loop {
        let frame = worker
            .frames
            .recv_timeout(until.saturating_duration_since(Instant::now()))
            .unwrap();
        if frame["type"] == "action_result" {
            assert_eq!(
                frame,
                json!({"type":"action_result","request_id":request,"value":{}})
            );
            break;
        }
        assert_eq!(frame["type"], "contributions");
        assert_eq!(item(&frame, "entity-0").unwrap()["text"], expected);
    }
    // Inspect every snapshot across a publication interval, including any that
    // arrived before the result; never discard an optimistic state update.
    let until = Instant::now() + Duration::from_millis(400);
    loop {
        match worker
            .frames
            .recv_timeout(until.saturating_duration_since(Instant::now()))
        {
            Ok(frame) => {
                assert_eq!(frame["type"], "contributions");
                assert_eq!(item(&frame, "entity-0").unwrap()["text"], expected);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => break,
            Err(error) => panic!("worker must remain alive after success: {error}"),
        }
    }
}

#[test]
fn binary_success_and_response_state_do_not_replace_observed_state_before_server_event() {
    let fixture = listener();
    let mut worker = Worker::start();
    let name = "input_boolean.do_not_supply_charger";
    let mut socket = initialize_configuration(
        &mut worker,
        &fixture,
        binary_configuration(&fixture, "", Some(name)),
    );
    let initial = connected(&mut worker, 2);
    assert_eq!(item(&initial, "entity-0").unwrap()["text"], "off");
    for (operation, observed) in [("on", "off"), ("off", "on")] {
        let id = format!("ha-binary-0-{operation}");
        worker.action(&id, &id, 5000);
        let mut pending = exact_service(
            &fixture,
            "input_boolean",
            &format!("turn_{operation}"),
            name,
        );
        respond(&mut pending, 200, json!([entity(name, operation)]));
        success_without_state_change(&mut worker, &id, observed);
        live(&mut socket, name, Some(entity(name, operation)));
        let updated = worker
            .until(|frame| item(frame, "entity-0").is_some_and(|item| item["text"] == operation));
        assert_eq!(actions(&updated).len(), 2);
    }
    no_request(&fixture, Duration::ZERO);
    worker.stop(false);
}

#[test]
fn binary_missing_deleted_unknown_and_malformed_states_withdraw_both_commands() {
    let fixture = listener();
    let mut worker = Worker::start();
    let name = "switch.selected";
    worker.configure_frame(binary_configuration(&fixture, "", Some(name)));
    let mut socket = authorize(&fixture, true);
    let mut initial = request(&fixture);
    assert_eq!(
        initial.line,
        "GET /reverse/proxy/ha/api/states/switch.selected HTTP/1.1"
    );
    connected(&mut worker, 0);
    worker.action("waiting", "ha-binary-0-on", 5000);
    worker.error("waiting", "unavailable");
    respond(
        &mut initial.stream,
        404,
        json!({"message":"Entity not found"}),
    );
    worker
        .until(|frame| item(frame, "entity-0").is_some_and(|item| item["value"] == "Unavailable"));
    worker.action("missing", "ha-binary-0-on", 5000);
    worker.error("missing", "unavailable");
    let mut invalid = [
        json!("unknown"),
        json!("unavailable"),
        json!("ON"),
        json!("off "),
        json!(""),
        json!("playing"),
        json!(true),
        json!(1),
        Value::Null,
    ]
    .into_iter()
    .map(|state| Some(json!({"entity_id":name,"state":state})))
    .collect::<Vec<_>>();
    invalid.push(Some(json!({"entity_id":name})));
    invalid.push(None);
    for (index, invalid) in invalid.into_iter().enumerate() {
        live(
            &mut socket,
            name,
            Some(entity(name, if index % 2 == 0 { "on" } else { "off" })),
        );
        connected(&mut worker, 2);
        live(&mut socket, name, invalid);
        connected(&mut worker, 0);
        for operation in ["on", "off"] {
            let id = format!("withdrawn-{index}-{operation}");
            worker.action(&id, &format!("ha-binary-0-{operation}"), 5000);
            worker.error(&id, "unavailable");
        }
        no_request(&fixture, Duration::ZERO);
    }
    worker.stop(false);
}

#[test]
fn binary_invalid_configuration_rejects_domains_counts_union_and_combined_action_budget() {
    let fixture = listener();
    let nine = (0..9)
        .map(|index| format!("switch.s{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let overlong = " ".repeat(4097);
    let mut invalid = [
        "sensor.selected",
        "button.selected",
        "scene.selected",
        "media_player.selected",
        "input_number.selected",
        "switch.invalid/path",
        "Switch.upper",
        nine.as_str(),
        overlong.as_str(),
    ]
    .into_iter()
    .map(|selected| binary_configuration(&fixture, "", Some(selected)))
    .collect::<Vec<_>>();
    let watch = (0..32)
        .map(|index| format!("sensor.s{index}"))
        .collect::<Vec<_>>()
        .join(",");
    invalid.push(binary_configuration(&fixture, &watch, Some("switch.extra")));
    let buttons = (0..16)
        .map(|index| format!("button.b{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let media = (0..4)
        .map(|index| format!("media_player.p{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let mut overflow = media_configuration(&fixture, "", Some(&buttons), Some(&media));
    overflow["configuration"]["values"]["binary_entities"] = json!("switch.one,light.two");
    invalid.push(overflow);
    for frame in invalid {
        let mut worker = Worker::start();
        worker.send(hello());
        assert_eq!(worker.next()["type"], "ready");
        worker.send(frame);
        worker.finish(false);
        assert!(worker.frames.try_iter().next().is_none());
        no_request(&fixture, Duration::ZERO);
    }
}

#[test]
fn binary_eight_unique_targets_and_31_mixed_actions_fit_64_bounded_contributions() {
    let fixture = listener();
    let mut worker = Worker::start();
    let watch = (0..11)
        .map(|index| format!("sensor.s{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let buttons = (0..12)
        .map(|index| format!("button.b{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let binary = (0..8)
        .map(|index| format!("switch.s{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let mut config = media_configuration(
        &fixture,
        &watch,
        Some(&buttons),
        Some("media_player.selected"),
    );
    config["configuration"]["values"]["binary_entities"] =
        json!(format!("{binary},switch.s0,\nswitch.s7"));
    let mut socket = initialize_configuration(&mut worker, &fixture, config);
    let frame = connected(&mut worker, 31);
    assert_eq!(frame["items"].as_array().unwrap().len(), 64);
    for (id, expected) in [
        ("entity-22", "button.b11"),
        ("entity-23", "media_player.selected"),
        ("entity-24", "switch.s0"),
        ("entity-31", "switch.s7"),
        ("ha-action-11", "button.b11"),
        ("ha-media-0-stop", "media_player.selected"),
        ("ha-binary-7-off", "switch.s7"),
    ] {
        assert_eq!(item(&frame, id).unwrap()["title"], expected);
    }
    for value in frame["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["id"].as_str().unwrap().starts_with("entity-"))
    {
        let name = value["title"].as_str().unwrap();
        let state = if name.starts_with("sensor.") {
            "\\".repeat(512)
        } else if name.starts_with("switch.") {
            "on".into()
        } else {
            "playing".into()
        };
        let mut value = entity(name, &state);
        value["attributes"]["friendly_name"] = json!("\"".repeat(128));
        live(&mut socket, name, Some(value));
    }
    let bounded = worker.until(|frame| {
        item(frame, "entity-31").is_some_and(|item| item["title"] == "\"".repeat(128))
    });
    assert_eq!(bounded["items"].as_array().unwrap().len(), 64);
    assert!(serde_json::to_vec(&bounded).unwrap().len() + 1 < MAX_FRAME);
    let mut ids = HashSet::new();
    for value in bounded["items"].as_array().unwrap() {
        assert!(ids.insert(value["id"].as_str().unwrap()));
        assert!(value["title"].as_str().unwrap().len() <= 128);
        if value["kind"] == "action" {
            assert_eq!(value["params"], json!({}));
            assert!(value["label"].as_str().unwrap().len() <= 128);
        }
    }
    worker.action("last-binary", "ha-binary-7-off", 5000);
    let mut pending = exact_service(&fixture, "switch", "turn_off", "switch.s7");
    respond(&mut pending, 200, json!([]));
    worker.success("last-binary");
    no_request(&fixture, Duration::ZERO);
    worker.stop(false);
}

#[test]
fn binary_changed_params_and_unadvertised_actions_never_issue_service_requests() {
    let fixture = listener();
    let mut worker = Worker::start();
    let _socket = initialize_configuration(
        &mut worker,
        &fixture,
        binary_configuration(&fixture, "", Some("light.selected")),
    );
    connected(&mut worker, 2);
    for (index, action, params) in [
        (0, "ha-binary-0-on", json!({"entity_id":"light.other"})),
        (1, "ha-binary-0-on", json!({"service":"switch.turn_off"})),
        (2, "ha-binary-0-on", json!({"brightness":255})),
        (3, "ha-binary-0-toggle", json!({})),
        (4, "ha-binary-1-on", json!({})),
        (5, "ha-binary-00-on", json!({})),
        (6, "ha-binary-0-turn_on", json!({})),
        (7, "light.turn_on", json!({})),
    ] {
        let request = format!("binary-invalid-{index}");
        worker.send(action_frame(&request, action, params, 5000));
        worker.error(&request, "invalid_action");
        no_request(&fixture, Duration::ZERO);
    }
    worker.stop(false);
}

#[test]
fn binary_transport_preserves_no_retry_deadlines_and_shared_two_request_limit() {
    let fixture = listener();
    let mut worker = Worker::start();
    let mut config = binary_configuration(&fixture, "", Some("switch.selected"));
    config["configuration"]["values"]["action_entities"] = json!("button.selected");
    let _socket = initialize_configuration(&mut worker, &fixture, config);
    connected(&mut worker, 3);
    worker.action("binary-lost", "ha-binary-0-on", 5000);
    drop(exact_service(
        &fixture,
        "switch",
        "turn_on",
        "switch.selected",
    ));
    worker.error("binary-lost", "outcome_unknown");
    no_request(&fixture, Duration::from_millis(1200));
    worker.action("binary-lost", "ha-binary-0-on", 5000);
    worker.error("binary-lost", "invalid_action");
    let started = Instant::now();
    worker.action("binary-deadline", "ha-binary-0-off", 250);
    let _deadline = exact_service(&fixture, "switch", "turn_off", "switch.selected");
    worker.error("binary-deadline", "outcome_unknown");
    assert!(started.elapsed() < Duration::from_secs(2));
    worker.action("binary-pending", "ha-binary-0-on", 5000);
    let _binary = exact_service(&fixture, "switch", "turn_on", "switch.selected");
    worker.action("button-pending", "ha-action-0", 5000);
    let mut button = service(&fixture, "button", "button.selected");
    worker.action("binary-overloaded", "ha-binary-0-off", 5000);
    worker.error("binary-overloaded", "overloaded");
    worker.send(json!({"type":"cancel","request_id":"binary-pending"}));
    worker.error("binary-pending", "outcome_unknown");
    respond(&mut button, 200, json!([]));
    worker.success("button-pending");
    no_request(&fixture, Duration::from_millis(100));
    worker.stop(false);
}

fn cover_snapshot(worker: &mut Worker, state: &str, operations: &[&str]) -> Value {
    let expected = operations
        .iter()
        .map(|operation| format!("ha-cover-0-{operation}"))
        .collect::<Vec<_>>();
    worker.until(|frame| {
        frame["type"] == "contributions"
            && item(frame, "connection").is_some_and(|item| item["value"] == "Connected")
            && item(frame, "entity-0").is_some_and(|item| item["text"] == state)
            && actions(frame)
                .iter()
                .filter_map(|item| item["action_id"].as_str())
                .filter(|id| id.starts_with("ha-cover-"))
                .map(str::to_owned)
                .collect::<Vec<_>>()
                == expected
    })
}

#[test]
fn cover_omitted_and_empty_selection_keep_capable_watched_covers_read_only() {
    for selected in [None, Some("")] {
        let fixture = listener();
        let mut worker = Worker::start();
        let name = "cover.inverter_on";
        let mut socket = initialize_configuration(
            &mut worker,
            &fixture,
            cover_configuration(&fixture, name, selected),
        );
        live(
            &mut socket,
            name,
            Some(cover_entity(name, "opening", json!(11))),
        );
        let frame = worker
            .until(|frame| item(frame, "entity-0").is_some_and(|item| item["text"] == "opening"));
        assert!(actions(&frame).is_empty());
        for operation in ["open", "close", "stop"] {
            worker.action(operation, &format!("ha-cover-0-{operation}"), 5000);
            worker.error(operation, "invalid_action");
        }
        no_request(&fixture, Duration::from_millis(100));
        worker.stop(false);
    }
}

#[test]
fn cover_exact_presets_preserve_literal_targets_old_ids_and_server_confirmed_state() {
    let fixture = listener();
    let mut worker = Worker::start();
    let name = "cover.inverter_on";
    let other = "cover.do_not_supply_charger";
    let mut config = media_configuration(
        &fixture,
        &format!("{name},sensor.power"),
        Some("scene.evening,button.trigger"),
        Some("media_player.television"),
    );
    config["configuration"]["values"]["binary_entities"] = json!("light.reading");
    config["configuration"]["values"]["cover_entities"] = json!(format!("{name},{other}"));
    let mut socket = initialize_configuration(&mut worker, &fixture, config);
    let frame = connected(&mut worker, 13);
    for (id, title) in [
        ("entity-0", name),
        ("entity-5", "light.reading"),
        ("entity-6", other),
        ("ha-action-0", "scene.evening"),
        ("ha-action-1", "button.trigger"),
        ("ha-media-0-play", "media_player.television"),
        ("ha-binary-0-off", "light.reading"),
    ] {
        assert_eq!(item(&frame, id).unwrap()["title"], title);
    }
    for (index, title) in [name, other].iter().enumerate() {
        for (operation, verb) in [("open", "Open"), ("close", "Close"), ("stop", "Stop")] {
            let id = format!("ha-cover-{index}-{operation}");
            let preset = item(&frame, &id).unwrap();
            assert_eq!(preset["action_id"], id);
            assert_eq!(preset["params"], json!({}));
            assert_eq!(preset["label"], format!("{verb} {title}"));
        }
    }
    for (operation, observed, next) in [
        ("open", "closed", "opening"),
        ("close", "opening", "closing"),
        ("stop", "closing", "closed"),
    ] {
        let id = format!("ha-cover-0-{operation}");
        worker.action(&id, &id, 5000);
        let mut pending = exact_service(&fixture, "cover", &format!("{operation}_cover"), name);
        respond(
            &mut pending,
            200,
            json!([cover_entity(name, next, json!(11))]),
        );
        success_without_state_change(&mut worker, &id, observed);
        live(&mut socket, name, Some(cover_entity(name, next, json!(11))));
        let updated =
            worker.until(|frame| item(frame, "entity-0").is_some_and(|item| item["text"] == next));
        assert_eq!(actions(&updated).len(), 13);
    }
    worker.action("other-cover", "ha-cover-1-stop", 5000);
    let mut pending = exact_service(&fixture, "cover", "stop_cover", other);
    respond(&mut pending, 200, json!([]));
    worker.success("other-cover");
    for (index, action, params) in [
        (0, "ha-cover-0-open", json!({"entity_id":"cover.other"})),
        (1, "ha-cover-0-close", json!({"service":"cover.stop_cover"})),
        (2, "ha-cover-0-open", json!({"position":50})),
        (3, "ha-cover-0-stop", json!({"tilt_position":25})),
        (4, "ha-cover-0-open", json!({"speed":1})),
        (5, "ha-cover-0-set_position", json!({})),
        (6, "ha-cover-2-open", json!({})),
        (7, "ha-cover-00-open", json!({})),
        (8, "cover.open_cover", json!({})),
        (9, "inverter_on", json!({})),
    ] {
        let request = format!("cover-invalid-{index}");
        worker.send(action_frame(&request, action, params, 5000));
        worker.error(&request, "invalid_action");
        no_request(&fixture, Duration::ZERO);
    }
    worker.stop(false);
}

#[test]
fn cover_feature_only_updates_publish_exact_presets_and_revoke_previous_commands() {
    let fixture = listener();
    let mut worker = Worker::start();
    let name = "cover.selected";
    let mut socket = initialize_configuration(
        &mut worker,
        &fixture,
        cover_configuration(&fixture, "", Some(name)),
    );
    cover_snapshot(&mut worker, "closed", &["open", "close", "stop"]);
    for (index, (features, expected)) in [
        (1u64, vec!["open"]),
        (2, vec!["close"]),
        (8, vec!["stop"]),
        (11, vec!["open", "close", "stop"]),
        (0, vec![]),
        (12, vec!["stop"]),
        (4, vec![]),
        (u64::MAX, vec!["open", "close", "stop"]),
    ]
    .into_iter()
    .enumerate()
    {
        // State and title remain unchanged; only capability bits differ.
        live(
            &mut socket,
            name,
            Some(cover_entity(name, "closed", json!(features))),
        );
        let frame = cover_snapshot(&mut worker, "closed", &expected);
        assert_eq!(item(&frame, "entity-0").unwrap()["title"], name);
        for operation in ["open", "close", "stop"] {
            let request = format!("feature-{index}-{operation}");
            worker.action(&request, &format!("ha-cover-0-{operation}"), 5000);
            if expected.contains(&operation) {
                let mut pending =
                    exact_service(&fixture, "cover", &format!("{operation}_cover"), name);
                respond(&mut pending, 200, json!([]));
                worker.success(&request);
            } else {
                worker.error(&request, "unavailable");
                no_request(&fixture, Duration::ZERO);
            }
        }
    }
    worker.stop(false);
}

#[test]
fn cover_observation_validation_withdraws_commands_and_accepts_moving_states() {
    let fixture = listener();
    let mut worker = Worker::start();
    let name = "cover.selected";
    worker.configure_frame(cover_configuration(&fixture, "", Some(name)));
    let mut socket = authorize(&fixture, true);
    let mut initial = request(&fixture);
    assert_eq!(
        initial.line,
        "GET /reverse/proxy/ha/api/states/cover.selected HTTP/1.1"
    );
    connected(&mut worker, 0);
    worker.action("cover-waiting", "ha-cover-0-stop", 5000);
    worker.error("cover-waiting", "unavailable");
    respond(
        &mut initial.stream,
        404,
        json!({"message":"Entity not found"}),
    );
    worker
        .until(|frame| item(frame, "entity-0").is_some_and(|item| item["value"] == "Unavailable"));
    worker.action("cover-missing", "ha-cover-0-open", 5000);
    worker.error("cover-missing", "unavailable");
    for state in ["open", "closed", "opening", "closing"] {
        live(
            &mut socket,
            name,
            Some(cover_entity(name, state, json!(11))),
        );
        cover_snapshot(&mut worker, state, &["open", "close", "stop"]);
    }
    let mut invalid = [
        json!("unknown"),
        json!("unavailable"),
        json!("OPEN"),
        json!("open "),
        json!(""),
        json!("on"),
        json!(true),
        json!(1),
        Value::Null,
    ]
    .into_iter()
    .map(|state| {
        Some(json!({"entity_id":name,"state":state,"attributes":{"supported_features":11}}))
    })
    .collect::<Vec<_>>();
    invalid.extend([
        Some(json!({"entity_id":name,"attributes":{"supported_features":11}})),
        Some(entity(name, "closed")),
        Some(json!({"entity_id":name,"state":"closed"})),
        Some(json!({"entity_id":name,"state":"closed","attributes":[]})),
        None,
    ]);
    for features in [
        json!(-1),
        json!(1.0),
        json!("11"),
        json!(true),
        Value::Null,
        json!([]),
        json!({}),
    ] {
        invalid.push(Some(cover_entity(name, "closed", features)));
    }
    for (index, invalid) in invalid.into_iter().enumerate() {
        live(
            &mut socket,
            name,
            Some(cover_entity(name, "opening", json!(11))),
        );
        cover_snapshot(&mut worker, "opening", &["open", "close", "stop"]);
        live(&mut socket, name, invalid);
        connected(&mut worker, 0);
        for operation in ["open", "close", "stop"] {
            let request = format!("cover-withdrawn-{index}-{operation}");
            worker.action(&request, &format!("ha-cover-0-{operation}"), 5000);
            worker.error(&request, "unavailable");
        }
        no_request(&fixture, Duration::ZERO);
    }
    live(
        &mut socket,
        name,
        Some(cover_entity(name, "closing", json!(11))),
    );
    cover_snapshot(&mut worker, "closing", &["open", "close", "stop"]);
    socket.close(None).unwrap();
    drop(socket);
    let frame = worker.until(|frame| {
        item(frame, "connection").is_some_and(|item| item["value"] == "Disconnected")
    });
    assert!(actions(&frame).is_empty());
    worker.action("cover-disconnected", "ha-cover-0-stop", 5000);
    worker.error("cover-disconnected", "unavailable");
    no_request(&fixture, Duration::ZERO);
    worker.stop(false);
}

#[test]
fn cover_invalid_configuration_rejects_domains_counts_union_and_reserved_action_overflow() {
    let fixture = listener();
    let five = (0..5)
        .map(|index| format!("cover.c{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let overlong = " ".repeat(4097);
    let mut invalid = [
        "sensor.selected",
        "button.selected",
        "scene.selected",
        "media_player.selected",
        "switch.selected",
        "input_boolean.selected",
        "light.selected",
        "inverter_on",
        "Cover.upper",
        "cover.invalid/path",
        "cover.",
        five.as_str(),
        overlong.as_str(),
    ]
    .into_iter()
    .map(|selected| cover_configuration(&fixture, "", Some(selected)))
    .collect::<Vec<_>>();
    for selected in [Value::Null, json!(["cover.selected"])] {
        let mut config = cover_configuration(&fixture, "", None);
        config["configuration"]["values"]["cover_entities"] = selected;
        invalid.push(config);
    }
    let watch = (0..32)
        .map(|index| format!("sensor.s{index}"))
        .collect::<Vec<_>>()
        .join(",");
    invalid.push(cover_configuration(&fixture, &watch, Some("cover.extra")));
    let buttons = (0..16)
        .map(|index| format!("button.b{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let mut overflow = media_configuration(
        &fixture,
        "",
        Some(&buttons),
        Some("media_player.one,media_player.two"),
    );
    overflow["configuration"]["values"]["binary_entities"] = json!("switch.one,light.two");
    overflow["configuration"]["values"]["cover_entities"] = json!("cover.one,cover.two");
    // Reserve all three cover commands before observing any supported_features.
    invalid.push(overflow);
    for config in invalid {
        let mut worker = Worker::start();
        worker.send(hello());
        assert_eq!(worker.next()["type"], "ready");
        worker.send(config);
        worker.finish(false);
        assert!(worker.frames.try_iter().next().is_none());
        no_request(&fixture, Duration::ZERO);
    }
}

#[test]
fn cover_four_unique_targets_and_all_action_families_fit_64_bounded_contributions() {
    let fixture = listener();
    let mut worker = Worker::start();
    let sensors = (0..16)
        .map(|index| format!("sensor.s{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let buttons = (0..6)
        .map(|index| format!("button.b{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let binary = (0..5)
        .map(|index| format!("switch.s{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let mut config = media_configuration(
        &fixture,
        &format!("cover.c3,{sensors},cover.c3"),
        Some(&buttons),
        Some("media_player.selected"),
    );
    config["configuration"]["values"]["binary_entities"] = json!(binary);
    config["configuration"]["values"]["cover_entities"] =
        json!("cover.c0,\ncover.c1,cover.c2,cover.c3,cover.c0,cover.c3");
    let mut socket = initialize_configuration(&mut worker, &fixture, config);
    let frame = connected(&mut worker, 31);
    assert_eq!(frame["items"].as_array().unwrap().len(), 64);
    for (id, title) in [
        ("entity-0", "cover.c3"),
        ("entity-22", "button.b5"),
        ("entity-23", "media_player.selected"),
        ("entity-28", "switch.s4"),
        ("entity-29", "cover.c0"),
        ("entity-31", "cover.c2"),
        ("ha-action-5", "button.b5"),
        ("ha-media-0-stop", "media_player.selected"),
        ("ha-binary-4-off", "switch.s4"),
        ("ha-cover-3-stop", "cover.c3"),
    ] {
        assert_eq!(item(&frame, id).unwrap()["title"], title);
    }
    for value in frame["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["id"].as_str().unwrap().starts_with("entity-"))
    {
        let name = value["title"].as_str().unwrap();
        let mut value = if name.starts_with("cover.") {
            cover_entity(name, "opening", json!(11))
        } else {
            let state = if name.starts_with("sensor.") {
                "\\".repeat(512)
            } else if name.starts_with("switch.") {
                "on".into()
            } else {
                "playing".into()
            };
            entity(name, &state)
        };
        value["attributes"]["friendly_name"] = json!("\"".repeat(128));
        live(&mut socket, name, Some(value));
    }
    let bounded = worker.until(|frame| {
        item(frame, "entity-31").is_some_and(|item| item["title"] == "\"".repeat(128))
    });
    assert_eq!(bounded["items"].as_array().unwrap().len(), 64);
    assert!(serde_json::to_vec(&bounded).unwrap().len() + 1 < MAX_FRAME);
    let mut ids = HashSet::new();
    for value in bounded["items"].as_array().unwrap() {
        assert!(ids.insert(value["id"].as_str().unwrap()));
        assert!(value["title"].as_str().unwrap().len() <= 128);
        if value["kind"] == "action" {
            assert_eq!(value["params"], json!({}));
            assert!(value["label"].as_str().unwrap().len() <= 128);
        }
    }
    worker.action("last-cover", "ha-cover-3-stop", 5000);
    let mut pending = exact_service(&fixture, "cover", "stop_cover", "cover.c3");
    respond(&mut pending, 200, json!([]));
    worker.success("last-cover");
    no_request(&fixture, Duration::ZERO);
    worker.stop(false);
}

#[test]
fn cover_cancellation_and_deadlines_never_send_stop_and_share_two_active_slots() {
    let fixture = listener();
    let mut worker = Worker::start();
    let name = "cover.selected";
    let mut config = cover_configuration(&fixture, "", Some(name));
    config["configuration"]["values"]["binary_entities"] = json!("switch.selected");
    let mut socket = initialize_configuration(&mut worker, &fixture, config);
    connected(&mut worker, 5);
    worker.send(json!({"type":"cancel","request_id":"cover-pre-canceled"}));
    worker.action("cover-barrier", "missing", 5000);
    worker.error("cover-barrier", "invalid_action");
    worker.action("cover-pre-canceled", "ha-cover-0-open", 5000);
    worker.error("cover-pre-canceled", "canceled");
    no_request(&fixture, Duration::ZERO);
    worker.action("cover-pending", "ha-cover-0-open", 5000);
    let _pending = exact_service(&fixture, "cover", "open_cover", name);
    worker.action("binary-pending", "ha-binary-0-off", 5000);
    let mut binary = exact_service(&fixture, "switch", "turn_off", "switch.selected");
    worker.action("cover-overloaded", "ha-cover-0-stop", 5000);
    worker.error("cover-overloaded", "overloaded");
    live(
        &mut socket,
        name,
        Some(cover_entity(name, "opening", json!(11))),
    );
    let moving =
        worker.until(|frame| item(frame, "entity-1").is_some_and(|item| item["text"] == "opening"));
    assert_eq!(actions(&moving).len(), 5);
    worker.send(json!({"type":"cancel","request_id":"cover-pending"}));
    worker.error("cover-pending", "outcome_unknown");
    respond(&mut binary, 200, json!([]));
    worker.success("binary-pending");
    no_request(&fixture, Duration::from_millis(100));
    let started = Instant::now();
    worker.action("cover-deadline", "ha-cover-0-close", 250);
    let _deadline = exact_service(&fixture, "cover", "close_cover", name);
    worker.error("cover-deadline", "outcome_unknown");
    assert!(started.elapsed() < Duration::from_secs(2));
    no_request(&fixture, Duration::from_millis(1200));
    // Only a fresh explicit Stop may issue stop_cover after cancellation/expiry.
    worker.action("explicit-cover-stop", "ha-cover-0-stop", 5000);
    let mut stop = exact_service(&fixture, "cover", "stop_cover", name);
    respond(&mut stop, 200, json!([]));
    worker.success("explicit-cover-stop");
    no_request(&fixture, Duration::from_millis(100));
    worker.stop(false);
}
