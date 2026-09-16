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
                let frame = serde_json::from_str(&text).unwrap();
                assert_state_links(&frame);
                sender.try_send(frame).expect("bounded test output");
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
            "host_api_version":"1.8.0","plugin_id":"inverter-desktop.home-assistant"})
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

fn assert_state_links(frame: &Value) {
    if frame["type"] != "contributions" {
        return;
    }
    let items = frame["items"].as_array().unwrap();
    for control in items {
        if matches!(control["kind"].as_str(), Some("action" | "number_input")) {
            let state_id = control["state_id"].as_str().expect("explicit state link");
            assert!(state_id.starts_with("entity-"));
            let state = items.iter().find(|state| state["id"] == state_id).unwrap();
            assert!(matches!(
                state["kind"].as_str(),
                Some("text" | "metric" | "status")
            ));
        } else {
            assert!(control.get("state_id").is_none());
        }
    }
}

fn hello() -> Value {
    json!({"type":"hello","protocol_version":1,"host_api_version":"1.8.0",
        "plugin_id":"inverter-desktop.home-assistant"})
}

fn configuration(fixture: &TcpListener, watch: &str, actions: Option<&str>) -> Value {
    let mut frame = json!({"type":"configuration","configuration":{
        "revision":"actions-1","values":{
            "ha_base_url":format!("http://{}/reverse/proxy/ha",fixture.local_addr().unwrap()),
            "dashboard_layout":"","watch_entities":watch},"secrets":{"ha_token":TOKEN}}});
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
        "number_entities",
        "cover_position_entities",
        "dishwasher_running_entity",
        "dishwasher_duration_entity",
        "washer_remaining_entity",
        "dryer_remaining_entity",
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
        let selected = |field: &str| {
            frame["configuration"]["values"][field]
                .as_str()
                .unwrap_or("")
                .split([',', '\n'])
                .map(str::trim)
                .any(|selected| selected == name)
        };
        let number_selected = selected("number_entities");
        let position_selected = selected("cover_position_entities");
        let state = if cover_selected || position_selected {
            "closed"
        } else if binary_selected || selected("dishwasher_running_entity") {
            "off"
        } else if selected("dishwasher_duration_entity") {
            "01:23:45"
        } else if selected("washer_remaining_entity") {
            "00:25:00"
        } else if selected("dryer_remaining_entity") {
            "00:40:00"
        } else if name.starts_with("media_player.") {
            "paused"
        } else {
            "unknown"
        };
        let value = if number_selected {
            number_entity(name, "-0.3")
        } else if position_selected {
            position_entity(name, state, json!(15), json!(20))
        } else if cover_selected {
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

fn initialize_primary(
    worker: &mut Worker,
    fixture: &TcpListener,
    target: &str,
) -> WebSocket<TcpStream> {
    let mut frame = configuration(fixture, "", None);
    frame["configuration"]["values"]["dashboard_layout"]=json!(json!({"version":1,"controls":[
        {"id":"primary","surface":"home","order":3,"label":"Exact configured label","entity":target,"icon":"plug"}
    ]}).to_string());
    worker.configure_frame(frame);
    let socket = authorize(fixture, true);
    let mut seen = HashSet::new();
    for _ in 0..2 {
        let mut requested = request(fixture);
        assert!(seen.insert(requested.line.clone()));
        if requested.line == "GET /reverse/proxy/ha/api/states HTTP/1.1" {
            respond(&mut requested.stream, 200, json!([entity(target, "off")]));
        } else {
            assert_eq!(
                requested.line,
                format!("GET /reverse/proxy/ha/api/states/{target} HTTP/1.1")
            );
            respond(&mut requested.stream, 200, entity(target, "off"));
        }
    }
    let frame = connected(worker, 1);
    let control = frame["presentation"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == "primary")
        .unwrap();
    assert_eq!(control["action"], "ha-primary-0");
    assert_eq!(control["state"], "off");
    socket
}

#[test]
fn primary_controls_read_fresh_state_and_use_explicit_legacy_routes_without_cached_toggle() {
    for domain in [
        "switch",
        "input_boolean",
        "light",
        "fan",
        "media_player",
        "script",
        "climate",
        "lock",
        "sensor",
        "binary_sensor",
    ] {
        let fixture = listener();
        let mut worker = Worker::start();
        let target = format!("{domain}.primary");
        let _socket = initialize_primary(&mut worker, &fixture, &target);
        for (serial, state, method) in [
            (0, "on", "turn_off"),
            (1, "off", "turn_on"),
            (2, "unknown", "turn_on"),
        ] {
            let id = format!("fresh-{serial}");
            worker.action(&id, "ha-primary-0", 5000);
            let mut fresh = request(&fixture);
            assert_eq!(
                fresh.line,
                format!("GET /reverse/proxy/ha/api/states/{target} HTTP/1.1")
            );
            assert!(fresh.body.is_empty());
            respond(&mut fresh.stream, 200, entity(&target, state));
            let mut post = exact_service(&fixture, domain, method, &target);
            respond(&mut post, 200, json!([]));
            worker.success(&id);
            no_request(&fixture, Duration::ZERO);
        }
        worker.stop(false);
    }
}

#[test]
fn failed_mismatched_or_redirected_primary_read_never_submits_a_service_or_retries() {
    for (status, body) in [
        (404, json!({})),
        (503, json!({})),
        (200, entity("switch.unselected", "on")),
        (200, json!({"entity_id":"switch.primary","state":null})),
        (302, json!({})),
    ] {
        let fixture = listener();
        let mut worker = Worker::start();
        let _socket = initialize_primary(&mut worker, &fixture, "switch.primary");
        worker.action("failed-read", "ha-primary-0", 5000);
        let mut fresh = request(&fixture);
        assert_eq!(
            fresh.line,
            "GET /reverse/proxy/ha/api/states/switch.primary HTTP/1.1"
        );
        respond(&mut fresh.stream, status, body);
        worker.error("failed-read", "unavailable");
        no_request(&fixture, Duration::from_millis(30));
        worker.stop(false);
    }
}

#[test]
fn primary_button_scene_cover_and_number_preserve_exact_legacy_default_bodies() {
    for (target, path, body) in [
        (
            "button.start",
            "button/press",
            json!({"entity_id":"button.start"}),
        ),
        (
            "scene.evening",
            "scene/turn_on",
            json!({"entity_id":"scene.evening"}),
        ),
        (
            "cover.blind",
            "cover/set_cover_position",
            json!({"entity_id":"cover.blind","position":0}),
        ),
        (
            "number.limit",
            "number/set_value",
            json!({"entity_id":"number.limit","value":0}),
        ),
    ] {
        let fixture = listener();
        let mut worker = Worker::start();
        let _socket = initialize_primary(&mut worker, &fixture, target);
        worker.action("primary-default", "ha-primary-0", 5000);
        let mut posted = request(&fixture);
        assert_eq!(
            posted.line,
            format!("POST /reverse/proxy/ha/api/services/{path} HTTP/1.1")
        );
        assert_eq!(serde_json::from_slice::<Value>(&posted.body).unwrap(), body);
        respond(&mut posted.stream, 200, json!([]));
        worker.success("primary-default");
        no_request(&fixture, Duration::ZERO);
        worker.stop(false);
    }
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
    let too_many = (0..64)
        .map(|index| format!("button.b{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let watch = (0..64)
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
            item(frame, "entity-30").is_some_and(|item| item["text"] == "x".repeat(256))
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
    let five = (0..22)
        .map(|index| format!("media_player.p{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let watched = (0..63)
        .map(|index| format!("sensor.s{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let overlong = " ".repeat(8256);
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
    let watch = (0..48)
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
        item(frame, "entity-31").is_some_and(|item| item["title"] == "🌞".repeat(16))
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
    let nine = (0..32)
        .map(|index| format!("switch.s{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let overlong = " ".repeat(8256);
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
    let watch = (0..64)
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
    overflow["configuration"]["values"]["binary_entities"] = json!((0..16)
        .map(|i| format!("switch.overflow_{i}"))
        .collect::<Vec<_>>()
        .join(","));
    overflow["configuration"]["values"]["media_player_entities"] =
        json!("media_player.one,media_player.two,media_player.three,media_player.four");
    overflow["configuration"]["values"]["cover_entities"] = json!("cover.one,cover.two");
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
fn sixty_four_reads_and_sixty_three_controls_publish_and_keep_the_last_target_actionable() {
    let list = |domain: &str, count| {
        (0..count)
            .map(|index| format!("{domain}.expanded_{index}"))
            .collect::<Vec<_>>()
    };
    let fixture = listener();
    let mut worker = Worker::start();
    let mut watched = vec!["sensor.barrier".to_owned()];
    watched.extend(list("sensor", 26));
    let buttons = list("button", 16);
    let media = list("media_player", 4);
    let binary = list("switch", 16);
    let mut config = media_configuration(
        &fixture,
        &watched.join(","),
        Some(&buttons.join(",")),
        Some(&media.join(",")),
    );
    config["configuration"]["values"]["binary_entities"] = json!(binary.join(","));
    config["configuration"]["values"]["cover_entities"] = json!("cover.expanded");
    let mut socket = initialize_configuration(&mut worker, &fixture, config);
    let initial = connected(&mut worker, 63);
    assert_eq!(initial["items"].as_array().unwrap().len(), 128);
    for name in watched.iter().chain(&buttons).chain(&media).chain(&binary) {
        let state = if binary.contains(name) {
            "off".to_owned()
        } else {
            "\\\"".repeat(256)
        };
        let mut value = entity(name, &state);
        value["attributes"]["friendly_name"] = json!("\\\"".repeat(64));
        live(&mut socket, name, Some(value));
    }
    let frame = numeric_barrier(&mut worker, &mut socket, 2);
    assert_eq!(frame["items"].as_array().unwrap().len(), 128);
    assert_eq!(actions(&frame).len(), 63);
    assert!(serde_json::to_vec(&frame).unwrap().len() < MAX_FRAME);
    assert_eq!(
        item(&frame, "ha-binary-15-off").unwrap()["state_id"],
        "entity-62"
    );
    worker.action("last-expanded-target", "ha-binary-15-off", 5000);
    let mut pending = exact_service(&fixture, "switch", "turn_off", &binary[15]);
    respond(&mut pending, 200, json!([]));
    worker.success("last-expanded-target");
    no_request(&fixture, Duration::ZERO);
    worker.stop(false);
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
        item(frame, "entity-31").is_some_and(|item| item["title"] == "\"".repeat(64))
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
    let five = (0..22)
        .map(|index| format!("cover.c{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let overlong = " ".repeat(8256);
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
    let watch = (0..64)
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
    overflow["configuration"]["values"]["binary_entities"] = json!((0..16)
        .map(|i| format!("switch.overflow_{i}"))
        .collect::<Vec<_>>()
        .join(","));
    overflow["configuration"]["values"]["media_player_entities"] =
        json!("media_player.one,media_player.two,media_player.three,media_player.four");
    overflow["configuration"]["values"]["cover_entities"] = json!("cover.one,cover.two");
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
        item(frame, "entity-31").is_some_and(|item| item["title"] == "\"".repeat(64))
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

fn number_entity(name: &str, state: &str) -> Value {
    json!({"entity_id":name,"state":state,"attributes":{"friendly_name":name,"min":-0.5,"max":0.5,"step":0.1,"unit_of_measurement":"kW"}})
}

fn position_entity(name: &str, state: &str, features: Value, position: Value) -> Value {
    let mut entity = cover_entity(name, state, features);
    entity["attributes"]["current_position"] = position;
    entity
}

fn numeric_configuration(
    fixture: &TcpListener,
    numbers: Option<&str>,
    positions: Option<&str>,
) -> Value {
    let mut config = configuration(fixture, "sensor.barrier", None);
    if let Some(numbers) = numbers {
        config["configuration"]["values"]["number_entities"] = json!(numbers);
    }
    if let Some(positions) = positions {
        config["configuration"]["values"]["cover_position_entities"] = json!(positions);
    }
    config
}

fn inputs(frame: &Value) -> Vec<&Value> {
    frame["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["kind"] == "number_input")
        .collect()
}

fn numeric_connected(worker: &mut Worker, count: usize) -> Value {
    worker.until(|frame| {
        frame["type"] == "contributions"
            && item(frame, "connection").is_some_and(|item| item["value"] == "Connected")
            && inputs(frame).len() == count
    })
}

fn numeric_barrier(worker: &mut Worker, socket: &mut WebSocket<TcpStream>, serial: usize) -> Value {
    live(
        socket,
        "sensor.barrier",
        Some(entity("sensor.barrier", &serial.to_string())),
    );
    worker.until(|frame| {
        frame["type"] == "contributions"
            && item(frame, "entity-0")
                .is_some_and(|item| item["value"].as_f64() == Some(serial as f64))
    })
}

fn numeric_action(
    worker: &mut Worker,
    request: &str,
    descriptor: &Value,
    value: i64,
    deadline: u64,
) {
    worker.send(action_frame(
        request,
        descriptor["action_id"].as_str().unwrap(),
        json!({"input_revision":descriptor["input_revision"],"value_scaled":value}),
        deadline,
    ));
}

fn numeric_service(fixture: &TcpListener, domain: &str, method: &str, body: &str) -> TcpStream {
    let request = request(fixture);
    assert_eq!(
        request.line,
        format!("POST /reverse/proxy/ha/api/services/{domain}/{method} HTTP/1.1")
    );
    assert_eq!(String::from_utf8(request.body).unwrap(), body);
    request.stream
}

#[test]
fn numeric_controls_are_opt_in_and_api_17_is_required() {
    for empty in [false, true] {
        let fixture = listener();
        let mut worker = Worker::start();
        let mut frame = numeric_configuration(&fixture, empty.then_some(""), empty.then_some(""));
        frame["configuration"]["values"]["watch_entities"] =
            json!("sensor.barrier,number.read_only,cover.read_only");
        frame["configuration"]["values"]["cover_entities"] = json!("cover.read_only");
        let mut socket = initialize_configuration(&mut worker, &fixture, frame);
        live(
            &mut socket,
            "number.read_only",
            Some(number_entity("number.read_only", "0.1")),
        );
        live(
            &mut socket,
            "cover.read_only",
            Some(position_entity(
                "cover.read_only",
                "open",
                json!(15),
                json!(90),
            )),
        );
        let frame = numeric_barrier(&mut worker, &mut socket, 1);
        assert!(inputs(&frame).is_empty());
        assert_eq!(actions(&frame).len(), 3);
        for (request, id) in [
            ("number", "ha-number-0-set"),
            ("position", "ha-cover-position-0-set"),
        ] {
            worker.send(action_frame(
                request,
                id,
                json!({"input_revision":"invented","value_scaled":0}),
                5000,
            ));
            worker.error(request, "invalid_action");
        }
        no_request(&fixture, Duration::from_millis(50));
        worker.stop(false);
    }
    let mut worker = Worker::start();
    let mut frame = hello();
    frame["host_api_version"] = json!("1.5.0");
    worker.send(frame);
    worker.finish(false);
}

#[test]
fn numeric_services_send_exact_decimals_and_positions_preserving_old_actions_and_observed_state() {
    let fixture = listener();
    let mut worker = Worker::start();
    let mut config = numeric_configuration(&fixture, Some("number.limit"), Some("cover.shade"));
    config["configuration"]["values"]["action_entities"] = json!("button.a,scene.b");
    config["configuration"]["values"]["media_player_entities"] = json!("media_player.a");
    config["configuration"]["values"]["binary_entities"] = json!("switch.a");
    config["configuration"]["values"]["cover_entities"] = json!("cover.shade");
    let mut socket = initialize_configuration(&mut worker, &fixture, config);
    let frame = numeric_connected(&mut worker, 2);
    assert_eq!(
        actions(&frame)
            .iter()
            .map(|item| item["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "ha-action-0",
            "ha-action-1",
            "ha-media-0-play",
            "ha-media-0-pause",
            "ha-media-0-stop",
            "ha-binary-0-on",
            "ha-binary-0-off",
            "ha-cover-0-open",
            "ha-cover-0-close",
            "ha-cover-0-stop"
        ]
    );
    let number = item(&frame, "ha-number-0-set").unwrap().clone();
    let cover = item(&frame, "ha-cover-position-0-set").unwrap().clone();
    assert_eq!(number["label"], "Set number.limit");
    assert_eq!(cover["label"], "Set position cover.shade");
    assert_eq!(
        (
            number["value_scaled"].as_i64(),
            number["decimal_places"].as_u64()
        ),
        (Some(-3), Some(1))
    );
    numeric_action(&mut worker, "number-write", &number, -2, 5000);
    let mut pending = numeric_service(
        &fixture,
        "number",
        "set_value",
        r#"{"entity_id":"number.limit","value":-0.2}"#,
    );
    let during = numeric_barrier(&mut worker, &mut socket, 1);
    assert_eq!(
        item(&during, "ha-number-0-set").unwrap()["value_scaled"],
        -3
    );
    respond(
        &mut pending,
        200,
        json!([number_entity("number.limit", "0.5")]),
    );
    worker.success("number-write");
    let after = numeric_barrier(&mut worker, &mut socket, 2);
    assert_eq!(item(&after, "ha-number-0-set").unwrap()["value_scaled"], -3);
    numeric_action(&mut worker, "position-write", &cover, 37, 5000);
    let mut pending = numeric_service(
        &fixture,
        "cover",
        "set_cover_position",
        r#"{"entity_id":"cover.shade","position":37}"#,
    );
    respond(
        &mut pending,
        200,
        json!([position_entity(
            "cover.shade",
            "open",
            json!(15),
            json!(100)
        )]),
    );
    worker.success("position-write");
    let after = numeric_barrier(&mut worker, &mut socket, 3);
    assert_eq!(
        item(&after, "ha-cover-position-0-set").unwrap()["value_scaled"],
        20
    );
    live(
        &mut socket,
        "number.limit",
        Some(number_entity("number.limit", "-0.2")),
    );
    live(
        &mut socket,
        "cover.shade",
        Some(position_entity(
            "cover.shade",
            "opening",
            json!(15),
            json!(37),
        )),
    );
    let observed = numeric_barrier(&mut worker, &mut socket, 4);
    assert_eq!(
        item(&observed, "ha-number-0-set").unwrap()["value_scaled"],
        -2
    );
    assert_eq!(
        item(&observed, "ha-number-0-set").unwrap()["input_revision"],
        number["input_revision"]
    );
    assert_eq!(
        item(&observed, "ha-cover-position-0-set").unwrap()["value_scaled"],
        37
    );
    assert_eq!(
        item(&observed, "ha-cover-position-0-set").unwrap()["input_revision"],
        cover["input_revision"]
    );
    no_request(&fixture, Duration::from_millis(50));
    worker.stop(false);
}

#[test]
fn numeric_decimal_exponents_and_precision_are_exact_without_rounding_metadata() {
    let fixture = listener();
    let mut worker = Worker::start();
    let mut socket = initialize_configuration(
        &mut worker,
        &fixture,
        numeric_configuration(&fixture, Some("number.a"), None),
    );
    numeric_connected(&mut worker, 1);
    let mut state: Value = serde_json::from_str(r#"{"entity_id":"number.a","state":"-3e-1","attributes":{"min":-5e-1,"max":5e-1,"step":1e-1}}"#).unwrap();
    live(&mut socket, "number.a", Some(state.clone()));
    let frame = numeric_barrier(&mut worker, &mut socket, 1);
    let descriptor = item(&frame, "ha-number-0-set").unwrap();
    numeric_action(&mut worker, "scientific", descriptor, 1, 5000);
    let mut response = numeric_service(
        &fixture,
        "number",
        "set_value",
        r#"{"entity_id":"number.a","value":0.1}"#,
    );
    respond(&mut response, 200, json!([]));
    worker.success("scientific");
    state["attributes"]["step"] = serde_json::from_str("0.10000000000000001").unwrap();
    live(&mut socket, "number.a", Some(state.clone()));
    let frame = numeric_barrier(&mut worker, &mut socket, 2);
    assert!(inputs(&frame).is_empty());
    assert_eq!(item(&frame, "entity-1").unwrap()["kind"], "metric");
    state["attributes"]["step"] = serde_json::from_str("1e-6").unwrap();
    state["state"] = json!("1e-6");
    live(&mut socket, "number.a", Some(state.clone()));
    let frame = numeric_barrier(&mut worker, &mut socket, 3);
    let descriptor = item(&frame, "ha-number-0-set").unwrap();
    assert_eq!(descriptor["decimal_places"], 6);
    numeric_action(&mut worker, "microunit", descriptor, 1, 5000);
    let mut response = numeric_service(
        &fixture,
        "number",
        "set_value",
        r#"{"entity_id":"number.a","value":0.000001}"#,
    );
    respond(&mut response, 200, json!([]));
    worker.success("microunit");
    state["attributes"]["step"] = serde_json::from_str("1e-7").unwrap();
    live(&mut socket, "number.a", Some(state));
    let frame = numeric_barrier(&mut worker, &mut socket, 4);
    assert!(inputs(&frame).is_empty());
    worker.stop(false);
}

#[test]
fn numeric_revisions_reject_stale_constraints_unit_changes_and_revoke_restore() {
    let fixture = listener();
    let mut worker = Worker::start();
    let mut socket = initialize_configuration(
        &mut worker,
        &fixture,
        numeric_configuration(&fixture, Some("number.a"), Some("cover.a")),
    );
    let frame = numeric_connected(&mut worker, 2);
    let original = item(&frame, "ha-number-0-set").unwrap().clone();
    let mut state = number_entity("number.a", "-0.2");
    live(&mut socket, "number.a", Some(state.clone()));
    let frame = numeric_barrier(&mut worker, &mut socket, 1);
    assert_eq!(
        item(&frame, "ha-number-0-set").unwrap()["input_revision"],
        original["input_revision"]
    );
    state["attributes"]["min"] = json!(-0.4);
    state["attributes"]["max"] = json!(0.4);
    state["attributes"]["step"] = json!(0.2);
    live(&mut socket, "number.a", Some(state.clone()));
    let frame = numeric_barrier(&mut worker, &mut socket, 2);
    let changed = item(&frame, "ha-number-0-set").unwrap().clone();
    assert_ne!(changed["input_revision"], original["input_revision"]);
    numeric_action(&mut worker, "stale-bounds", &original, 0, 5000);
    worker.error("stale-bounds", "unavailable");
    numeric_action(&mut worker, "off-grid", &changed, 1, 5000);
    worker.error("off-grid", "invalid_action");
    state["attributes"]["unit_of_measurement"] = json!("W");
    live(&mut socket, "number.a", Some(state.clone()));
    let frame = numeric_barrier(&mut worker, &mut socket, 3);
    let unit_changed = item(&frame, "ha-number-0-set").unwrap().clone();
    assert_ne!(unit_changed["input_revision"], changed["input_revision"]);
    numeric_action(&mut worker, "stale-unit", &changed, 0, 5000);
    worker.error("stale-unit", "unavailable");
    live(&mut socket, "number.a", None);
    live(&mut socket, "number.a", Some(state));
    let frame = numeric_barrier(&mut worker, &mut socket, 4);
    let restored = item(&frame, "ha-number-0-set").unwrap();
    assert_ne!(restored["input_revision"], unit_changed["input_revision"]);
    numeric_action(&mut worker, "stale-restore", &unit_changed, 0, 5000);
    worker.error("stale-restore", "unavailable");
    numeric_action(&mut worker, "current", restored, 0, 5000);
    let mut response = numeric_service(
        &fixture,
        "number",
        "set_value",
        r#"{"entity_id":"number.a","value":0.0}"#,
    );
    respond(&mut response, 200, json!([]));
    worker.success("current");
    no_request(&fixture, Duration::from_millis(50));
    worker.stop(false);
}

#[test]
fn malformed_number_and_cover_position_metadata_withdraw_only_the_affected_input() {
    let fixture = listener();
    let mut worker = Worker::start();
    let mut socket = initialize_configuration(
        &mut worker,
        &fixture,
        numeric_configuration(&fixture, Some("number.a"), Some("cover.a")),
    );
    numeric_connected(&mut worker, 2);
    let mut serial = 0;
    let valid = number_entity("number.a", "-0.3");
    let mut invalid = vec![None];
    for value in [
        json!("unknown"),
        json!("unavailable"),
        json!("0.01"),
        json!("100"),
        json!("NaN"),
        json!("1e999"),
        json!(1),
        Value::Null,
    ] {
        let mut state = valid.clone();
        state["state"] = value;
        invalid.push(Some(state));
    }
    for (field, value) in [
        ("min", json!("-0.5")),
        ("min", json!(1)),
        ("max", Value::Null),
        ("step", json!(0)),
        ("step", json!(-1)),
        ("step", json!(false)),
        ("step", json!([])),
        ("step", json!({})),
        ("unit_of_measurement", json!(17)),
    ] {
        let mut state = valid.clone();
        state["attributes"][field] = value;
        invalid.push(Some(state));
    }
    let mut missing = valid.clone();
    missing["attributes"].as_object_mut().unwrap().remove("min");
    invalid.push(Some(missing));
    for state in invalid {
        live(&mut socket, "number.a", state);
        serial += 1;
        let frame = numeric_barrier(&mut worker, &mut socket, serial);
        assert!(item(&frame, "ha-number-0-set").is_none());
        assert!(item(&frame, "ha-cover-position-0-set").is_some());
        live(&mut socket, "number.a", Some(valid.clone()));
        serial += 1;
        let frame = numeric_barrier(&mut worker, &mut socket, serial);
        assert_eq!(inputs(&frame).len(), 2);
    }
    let valid = position_entity("cover.a", "closed", json!(4), json!(20));
    let mut invalid = vec![None];
    for value in [
        json!("unknown"),
        json!("unavailable"),
        json!("OPEN"),
        json!("closed "),
        json!(20),
    ] {
        let mut state = valid.clone();
        state["state"] = value;
        invalid.push(Some(state));
    }
    for (field, value) in [
        ("current_position", json!(-1)),
        ("current_position", json!(101)),
        ("current_position", json!(20.0)),
        ("current_position", json!("20")),
        ("current_position", json!(true)),
        ("current_position", Value::Null),
        ("supported_features", json!(11)),
        ("supported_features", json!(4.0)),
        ("supported_features", json!(-1)),
        ("supported_features", json!("4")),
    ] {
        let mut state = valid.clone();
        state["attributes"][field] = value;
        invalid.push(Some(state));
    }
    let mut missing = valid.clone();
    missing["attributes"]
        .as_object_mut()
        .unwrap()
        .remove("current_position");
    invalid.push(Some(missing));
    for state in invalid {
        live(&mut socket, "cover.a", state);
        serial += 1;
        let frame = numeric_barrier(&mut worker, &mut socket, serial);
        assert!(item(&frame, "ha-cover-position-0-set").is_none());
        assert!(item(&frame, "ha-number-0-set").is_some());
        live(&mut socket, "cover.a", Some(valid.clone()));
        serial += 1;
        assert_eq!(
            inputs(&numeric_barrier(&mut worker, &mut socket, serial)).len(),
            2
        );
    }
    no_request(&fixture, Duration::from_millis(50));
    worker.stop(false);
}

#[test]
fn numeric_parameters_are_exact_bounded_and_cannot_supply_services_or_targets() {
    let fixture = listener();
    let mut worker = Worker::start();
    let _socket = initialize_configuration(
        &mut worker,
        &fixture,
        numeric_configuration(&fixture, Some("number.a"), Some("cover.a")),
    );
    let frame = numeric_connected(&mut worker, 2);
    let descriptor = item(&frame, "ha-number-0-set").unwrap();
    let revision = descriptor["input_revision"].clone();
    for (index, params) in [
        json!({}),
        json!({"input_revision":revision}),
        json!({"value_scaled":0}),
        json!({"input_revision":revision,"value_scaled":0,"entity_id":"number.other"}),
        json!({"input_revision":revision,"value_scaled":0,"service":"toggle"}),
        json!({"input_revision":revision,"value_scaled":"0"}),
        json!({"input_revision":revision,"value_scaled":0.0}),
        json!({"input_revision":revision,"value_scaled":false}),
        json!({"input_revision":revision,"value_scaled":null}),
        json!({"input_revision":revision,"value_scaled":1000000000000001_i64}),
        json!({"input_revision":revision,"value_scaled":i64::MIN}),
        json!({"input_revision":revision,"value_scaled":6}),
    ]
    .into_iter()
    .enumerate()
    {
        let request = format!("invalid-{index}");
        worker.send(action_frame(&request, "ha-number-0-set", params, 5000));
        worker.error(&request, "invalid_action");
    }
    for (index, id) in [
        "ha-number-00-set",
        "ha-number-1-set",
        "ha-number-0-value",
        "ha-cover-position-00-set",
        "ha-cover-position-0-open",
    ]
    .into_iter()
    .enumerate()
    {
        let request = format!("id-{index}");
        worker.send(action_frame(
            &request,
            id,
            json!({"input_revision":revision,"value_scaled":0}),
            5000,
        ));
        worker.error(&request, "invalid_action");
    }
    no_request(&fixture, Duration::from_millis(50));
    worker.stop(false);
}

#[test]
fn numeric_lists_enforce_domains_shared_reservations_and_watch_limits() {
    let list = |domain: &str, count| {
        (0..count)
            .map(|i| format!("{domain}.e{i}"))
            .collect::<Vec<_>>()
            .join(",")
    };
    for (field, value) in [
        ("number_entities", "input_number.a".into()),
        ("number_entities", "number.*".into()),
        ("cover_position_entities", "light.a".into()),
        ("cover_position_entities", "cover.a/escape".into()),
        ("number_entities", list("number", 64)),
        ("cover_position_entities", list("cover", 64)),
        ("number_entities", " ".repeat(8256)),
        ("cover_position_entities", " ".repeat(8256)),
    ] {
        let fixture = listener();
        let mut worker = Worker::start();
        let mut frame = numeric_configuration(&fixture, None, None);
        frame["configuration"]["values"][field] = json!(value);
        worker.send(hello());
        worker.next();
        worker.send(frame);
        worker.finish(false);
    }
    for watch_overflow in [false, true] {
        let fixture = listener();
        let mut worker = Worker::start();
        let mut frame =
            numeric_configuration(&fixture, Some(&list("number", 4)), Some(&list("cover", 4)));
        if watch_overflow {
            frame["configuration"]["values"]["watch_entities"] = json!(list("sensor", 57));
        } else {
            frame["configuration"]["values"]["action_entities"] = json!(list("button", 16));
            frame["configuration"]["values"]["binary_entities"] = json!(list("switch", 16));
            frame["configuration"]["values"]["media_player_entities"] =
                json!(list("media_player", 4));
        }
        worker.send(hello());
        worker.next();
        worker.send(frame);
        worker.finish(false);
    }
}

#[test]
fn numeric_maximum_combination_publishes_64_bounded_contributions_and_deduplicated_ids() {
    let list = |domain: &str, count| {
        (0..count)
            .map(|i| format!("{domain}.e{i}"))
            .collect::<Vec<_>>()
    };
    let fixture = listener();
    let mut worker = Worker::start();
    let numbers = list("number", 4);
    let positions = list("cover", 4);
    let mut config = numeric_configuration(
        &fixture,
        Some(&format!("{},number.e0", numbers.join(","))),
        Some(&format!("{},cover.e0", positions.join(","))),
    );
    let mut watch = vec!["sensor.barrier".to_owned()];
    watch.extend(list("sensor", 8));
    config["configuration"]["values"]["watch_entities"] = json!(watch.join(","));
    config["configuration"]["values"]["action_entities"] = json!(list("button", 11).join(","));
    config["configuration"]["values"]["media_player_entities"] =
        json!(list("media_player", 4).join(","));
    let mut socket = initialize_configuration(&mut worker, &fixture, config);
    numeric_connected(&mut worker, 8);
    for name in numbers.iter().chain(&positions) {
        let mut state = if name.starts_with("number.") {
            number_entity(name, "0")
        } else {
            position_entity(name, "closed", json!(4), json!(100))
        };
        state["attributes"]["friendly_name"] = json!("\\\"".repeat(128));
        state["attributes"]["unit_of_measurement"] = json!("\\\"".repeat(32));
        live(&mut socket, name, Some(state));
    }
    let frame = numeric_barrier(&mut worker, &mut socket, 1);
    let items = frame["items"].as_array().unwrap();
    assert_eq!(items.len(), 64);
    assert_eq!(inputs(&frame).len(), 8);
    assert_eq!(actions(&frame).len(), 23);
    assert!(serde_json::to_vec(&frame).unwrap().len() < MAX_FRAME);
    for index in 0..4 {
        assert!(item(&frame, &format!("ha-number-{index}-set")).is_some());
        assert!(item(&frame, &format!("ha-cover-position-{index}-set")).is_some());
    }
    worker.stop(false);
}

#[test]
fn numeric_and_static_actions_share_slots_cancellation_deadlines_and_unknown_outcomes() {
    let fixture = listener();
    let mut worker = Worker::start();
    let mut config = numeric_configuration(&fixture, Some("number.a"), Some("cover.a"));
    config["configuration"]["values"]["action_entities"] = json!("button.a");
    let _socket = initialize_configuration(&mut worker, &fixture, config);
    let frame = numeric_connected(&mut worker, 2);
    let number = item(&frame, "ha-number-0-set").unwrap();
    let cover = item(&frame, "ha-cover-position-0-set").unwrap();
    numeric_action(&mut worker, "pending-number", number, 0, 5000);
    let first = numeric_service(
        &fixture,
        "number",
        "set_value",
        r#"{"entity_id":"number.a","value":0.0}"#,
    );
    worker.action("pending-button", "ha-action-0", 5000);
    let second = service(&fixture, "button", "button.a");
    numeric_action(&mut worker, "third", cover, 40, 5000);
    worker.error("third", "overloaded");
    no_request(&fixture, Duration::from_millis(50));
    worker.send(json!({"type":"cancel","request_id":"pending-number"}));
    worker.error("pending-number", "outcome_unknown");
    drop(first);
    numeric_action(&mut worker, "deadline", cover, 41, 250);
    let expired = numeric_service(
        &fixture,
        "cover",
        "set_cover_position",
        r#"{"entity_id":"cover.a","position":41}"#,
    );
    worker.error("deadline", "outcome_unknown");
    drop(expired);
    numeric_action(&mut worker, "lost", number, 1, 5000);
    let lost = numeric_service(
        &fixture,
        "number",
        "set_value",
        r#"{"entity_id":"number.a","value":0.1}"#,
    );
    drop(lost);
    worker.error("lost", "outcome_unknown");
    no_request(&fixture, Duration::from_millis(150));
    worker.send(json!({"type":"cancel","request_id":"pending-button"}));
    worker.error("pending-button", "outcome_unknown");
    drop(second);
    numeric_action(&mut worker, "eof", cover, 42, 5000);
    let pending = numeric_service(
        &fixture,
        "cover",
        "set_cover_position",
        r#"{"entity_id":"cover.a","position":42}"#,
    );
    worker.stop(true);
    drop(pending);
    no_request(&fixture, Duration::from_millis(50));
}

#[test]
fn numeric_authentication_rejection_cancels_sibling_and_withdraws_inputs() {
    let fixture = listener();
    let mut worker = Worker::start();
    let _socket = initialize_configuration(
        &mut worker,
        &fixture,
        numeric_configuration(&fixture, Some("number.a"), Some("cover.a")),
    );
    let frame = numeric_connected(&mut worker, 2);
    let number = item(&frame, "ha-number-0-set").unwrap();
    let cover = item(&frame, "ha-cover-position-0-set").unwrap();
    numeric_action(&mut worker, "number", number, 0, 5000);
    let mut rejected = numeric_service(
        &fixture,
        "number",
        "set_value",
        r#"{"entity_id":"number.a","value":0.0}"#,
    );
    numeric_action(&mut worker, "cover", cover, 50, 5000);
    let sibling = numeric_service(
        &fixture,
        "cover",
        "set_cover_position",
        r#"{"entity_id":"cover.a","position":50}"#,
    );
    respond(&mut rejected, 401, json!({"private":PRIVATE_BODY}));
    worker.error("number", "outcome_unknown");
    worker.error("cover", "outcome_unknown");
    let frame = worker.until(|frame| {
        frame["type"] == "contributions"
            && item(frame, "connection")
                .is_some_and(|item| item["value"] == "Authentication rejected")
    });
    assert!(inputs(&frame).is_empty());
    numeric_action(&mut worker, "revoked", number, 0, 5000);
    worker.error("revoked", "unavailable");
    drop(sibling);
    no_request(&fixture, Duration::from_millis(100));
    worker.stop(false);
}

#[test]
fn raw_numeric_parameter_objects_cannot_impersonate_integer_coefficients() {
    for key in [
        "$serde_json::private::Number",
        r"\u0024serde_json::private::Number",
    ] {
        let fixture = listener();
        let mut worker = Worker::start();
        let _socket = initialize_configuration(
            &mut worker,
            &fixture,
            numeric_configuration(&fixture, Some("number.a"), None),
        );
        let frame = numeric_connected(&mut worker, 1);
        let revision = item(&frame, "ha-number-0-set").unwrap()["input_revision"]
            .as_str()
            .unwrap();
        let raw = format!(
            r#"{{"type":"action","request_id":"spoof","action_id":"ha-number-0-set","params":{{"input_revision":"{revision}","value_scaled":{{"{key}":"0"}}}},"deadline_ms":5000}}"#
        );
        let input = worker.input.as_mut().unwrap();
        writeln!(input, "{raw}").unwrap();
        input.flush().unwrap();
        worker.finish(false);
        no_request(&fixture, Duration::from_millis(50));
    }
}

#[test]
fn numeric_disconnect_and_new_session_invalidate_previous_input_revisions() {
    let fixture = listener();
    let mut worker = Worker::start();
    let socket = initialize_configuration(
        &mut worker,
        &fixture,
        numeric_configuration(&fixture, Some("number.a"), Some("cover.a")),
    );
    let frame = numeric_connected(&mut worker, 2);
    let previous = item(&frame, "ha-number-0-set").unwrap().clone();
    numeric_action(&mut worker, "disconnect-pending", &previous, 0, 5000);
    let pending = numeric_service(
        &fixture,
        "number",
        "set_value",
        r#"{"entity_id":"number.a","value":0.0}"#,
    );
    drop(socket);
    worker.error("disconnect-pending", "outcome_unknown");
    let frame = worker.until(|frame| {
        frame["type"] == "contributions"
            && item(frame, "connection").is_some_and(|item| item["value"] == "Disconnected")
    });
    assert!(inputs(&frame).is_empty());
    numeric_action(&mut worker, "disconnected", &previous, 0, 5000);
    worker.error("disconnected", "unavailable");
    drop(pending);
    let _socket = authorize(&fixture, true);
    for _ in 0..3 {
        let mut req = request(&fixture);
        let name = req
            .line
            .strip_prefix("GET /reverse/proxy/ha/api/states/")
            .unwrap()
            .strip_suffix(" HTTP/1.1")
            .unwrap();
        let value = match name {
            "number.a" => number_entity(name, "-0.3"),
            "cover.a" => position_entity(name, "closed", json!(15), json!(20)),
            "sensor.barrier" => entity(name, "unknown"),
            _ => panic!("unexpected target"),
        };
        respond(&mut req.stream, 200, value);
    }
    let frame = numeric_connected(&mut worker, 2);
    let current = item(&frame, "ha-number-0-set").unwrap();
    assert_ne!(current["input_revision"], previous["input_revision"]);
    assert_eq!(current["state_id"], "entity-1");
    assert_eq!(current["state_id"], previous["state_id"]);
    numeric_action(&mut worker, "previous-session", &previous, 0, 5000);
    worker.error("previous-session", "unavailable");
    numeric_action(&mut worker, "new-session", current, 0, 5000);
    let mut response = numeric_service(
        &fixture,
        "number",
        "set_value",
        r#"{"entity_id":"number.a","value":0.0}"#,
    );
    respond(&mut response, 200, json!([]));
    worker.success("new-session");
    worker.stop(false);
}

fn discovery_configuration(fixture: &TcpListener, watch: &str, prefixes: Option<&str>) -> Value {
    let mut config = configuration(fixture, watch, None);
    if let Some(prefixes) = prefixes {
        config["configuration"]["values"]["discovery_prefixes"] = json!(prefixes);
    }
    config
}

fn discovery_items(frame: &Value) -> Vec<&Value> {
    frame["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| {
            item["id"]
                .as_str()
                .is_some_and(|id| id.starts_with("discovery-"))
        })
        .collect()
}

fn discovered<'a>(frame: &'a Value, title: &str) -> Option<&'a Value> {
    discovery_items(frame)
        .into_iter()
        .find(|item| item["title"] == title)
}

fn discovery_status(worker: &mut Worker, expected: &str) -> Value {
    worker.until(|frame| {
        frame["type"] == "contributions"
            && item(frame, "connection").is_some_and(|item| item["value"] == expected)
    })
}

fn start_discovery(
    worker: &mut Worker,
    fixture: &TcpListener,
    config: Value,
) -> (WebSocket<TcpStream>, TcpStream) {
    let mut names = Vec::new();
    for field in [
        "watch_entities",
        "action_entities",
        "media_player_entities",
        "binary_entities",
        "cover_entities",
        "number_entities",
        "cover_position_entities",
        "dishwasher_running_entity",
        "dishwasher_duration_entity",
        "washer_remaining_entity",
        "dryer_remaining_entity",
    ] {
        for name in config["configuration"]["values"][field]
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
    worker.configure_frame(config.clone());
    let socket = authorize(fixture, true);
    let mut snapshot = None;
    let mut observed = HashSet::new();
    for _ in 0..names.len() + 1 {
        let mut req = request(fixture);
        assert!(req.body.is_empty());
        if req.line == "GET /reverse/proxy/ha/api/states HTTP/1.1" {
            assert!(snapshot.is_none(), "only one collection per connection");
            snapshot = Some(req.stream);
        } else {
            let name = req
                .line
                .strip_prefix("GET /reverse/proxy/ha/api/states/")
                .and_then(|line| line.strip_suffix(" HTTP/1.1"))
                .expect("only fixed explicit reads and opted-in collection");
            assert!(names.contains(&name));
            assert!(observed.insert(name.to_owned()));
            let state = match name.split_once('.').unwrap().0 {
                "number" => number_entity(name, "-0.3"),
                "cover" => position_entity(name, "closed", json!(15), json!(20)),
                "media_player" => entity(name, "paused"),
                "switch" | "light" | "input_boolean" => entity(name, "off"),
                "button" | "scene" => entity(name, "unknown"),
                _ => entity(name, "1"),
            };
            respond(&mut req.stream, 200, state);
        }
    }
    (socket, snapshot.expect("opted-in collection request"))
}

#[test]
fn discovery_defaults_and_full_explicit_capacity_never_request_the_collection() {
    for prefixes in [None, Some("")] {
        let fixture = listener();
        let mut worker = Worker::start();
        let mut socket = initialize_configuration(
            &mut worker,
            &fixture,
            discovery_configuration(&fixture, "sensor.barrier,sensor.manual", prefixes),
        );
        live(
            &mut socket,
            "sensor.unselected",
            Some(entity("sensor.unselected", "9")),
        );
        let frame = numeric_barrier(&mut worker, &mut socket, 2);
        assert_eq!(frame["items"].as_array().unwrap().len(), 3);
        assert!(discovery_items(&frame).is_empty());
        no_request(&fixture, Duration::from_millis(80));
        worker.stop(false);
    }
    let fixture = listener();
    let mut worker = Worker::start();
    let mut selected = vec!["sensor.barrier".to_owned()];
    selected.extend((0..63).map(|i| format!("sensor.manual_{i}")));
    let mut socket = initialize_configuration(
        &mut worker,
        &fixture,
        discovery_configuration(&fixture, &selected.join(","), Some("sensor.")),
    );
    live(
        &mut socket,
        "sensor.unselected",
        Some(entity("sensor.unselected", "9")),
    );
    let frame = numeric_barrier(&mut worker, &mut socket, 2);
    assert_eq!(frame["items"].as_array().unwrap().len(), 65);
    assert!(discovery_items(&frame).is_empty());
    no_request(&fixture, Duration::from_millis(80));
    worker.stop(false);
}

#[test]
fn discovery_only_configuration_subscribes_and_sorts_read_only_prefix_matches() {
    let fixture = listener();
    let mut worker = Worker::start();
    let (mut socket, mut snapshot) = start_discovery(
        &mut worker,
        &fixture,
        discovery_configuration(
            &fixture,
            "",
            Some("sensor.room_,binary_sensor.door_,sensor.room_"),
        ),
    );
    respond(
        &mut snapshot,
        200,
        json!([
            entity("sensor.room_z", "unknown"),
            entity("switch.room_a", "on"),
            entity("sensor.unmatched", "3"),
            entity("binary_sensor.door_front", "on"),
            entity("sensor.room_a", "2"),
            entity("sensor.room_a.extra", "7"),
            entity("sensor.room_*", "7")
        ]),
    );
    let frame =
        worker.until(|frame| frame["type"] == "contributions" && discovery_items(frame).len() == 3);
    assert_eq!(
        discovery_items(&frame)
            .iter()
            .map(|item| item["title"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["binary_sensor.door_front", "sensor.room_a", "sensor.room_z"]
    );
    assert!(discovery_items(&frame)
        .iter()
        .all(|item| matches!(item["kind"].as_str(), Some("text" | "metric" | "status"))));
    assert!(actions(&frame).is_empty());
    assert!(inputs(&frame).is_empty());
    let old_id = discovered(&frame, "sensor.room_a").unwrap()["id"].clone();
    live(
        &mut socket,
        "sensor.room_a",
        Some(entity("sensor.room_a", "8")),
    );
    let next = worker.until(|frame| {
        frame["type"] == "contributions"
            && discovered(frame, "sensor.room_a").is_some_and(|item| item["value"] == 8.0)
    });
    assert_eq!(discovered(&next, "sensor.room_a").unwrap()["id"], old_id);
    for (index, id) in [
        old_id.as_str().unwrap(),
        "ha-action-0",
        "ha-binary-0-on",
        "ha-number-0-set",
    ]
    .into_iter()
    .enumerate()
    {
        let request = format!("readonly-{index}");
        worker.action(&request, id, 5000);
        worker.error(&request, "invalid_action");
    }
    no_request(&fixture, Duration::from_millis(100));
    worker.stop(false);
}

#[test]
fn discovery_live_updates_deletions_and_malformed_tombstones_override_late_snapshot() {
    let fixture = listener();
    let mut worker = Worker::start();
    let (mut socket, mut snapshot) = start_discovery(
        &mut worker,
        &fixture,
        discovery_configuration(&fixture, "sensor.barrier,sensor.manual", Some("sensor.")),
    );
    live(
        &mut socket,
        "sensor.manual",
        Some(entity("sensor.manual", "77")),
    );
    live(
        &mut socket,
        "sensor.updated",
        Some(entity("sensor.updated", "9")),
    );
    live(&mut socket, "sensor.deleted", None);
    live(
        &mut socket,
        "sensor.malformed",
        Some(json!({"entity_id":"sensor.other","state":"bad"})),
    );
    live(
        &mut socket,
        "sensor.live_only",
        Some(entity("sensor.live_only", "11")),
    );
    let before = numeric_barrier(&mut worker, &mut socket, 2);
    assert!(discovery_items(&before).is_empty());
    assert_eq!(item(&before, "entity-1").unwrap()["value"], 77.0);
    respond(
        &mut snapshot,
        200,
        json!([
            entity("sensor.manual", "1"),
            entity("sensor.manual", "2"),
            entity("sensor.updated", "1"),
            entity("sensor.deleted", "1"),
            entity("sensor.malformed", "1"),
            entity("sensor.snapshot_only", "3")
        ]),
    );
    let frame =
        worker.until(|frame| frame["type"] == "contributions" && discovery_items(frame).len() == 3);
    assert_eq!(discovered(&frame, "sensor.updated").unwrap()["value"], 9.0);
    assert_eq!(
        discovered(&frame, "sensor.live_only").unwrap()["value"],
        11.0
    );
    assert!(discovered(&frame, "sensor.deleted").is_none());
    assert!(discovered(&frame, "sensor.malformed").is_none());
    assert_eq!(item(&frame, "entity-1").unwrap()["value"], 77.0);
    let old = discovered(&frame, "sensor.updated").unwrap()["id"].clone();
    live(&mut socket, "sensor.updated", None);
    let removed = numeric_barrier(&mut worker, &mut socket, 3);
    assert!(discovered(&removed, "sensor.updated").is_none());
    live(
        &mut socket,
        "sensor.updated",
        Some(entity("sensor.updated", "10")),
    );
    let restored = numeric_barrier(&mut worker, &mut socket, 4);
    assert_ne!(discovered(&restored, "sensor.updated").unwrap()["id"], old);
    no_request(&fixture, Duration::from_millis(50));
    worker.stop(false);
}

#[test]
fn discovery_capacity_discards_unseen_state_and_free_slots_require_a_new_live_update() {
    let fixture = listener();
    let mut worker = Worker::start();
    let mut selected = vec!["sensor.barrier".to_owned()];
    selected.extend((0..61).map(|i| format!("sensor.manual_{i}")));
    let (mut socket, mut snapshot) = start_discovery(
        &mut worker,
        &fixture,
        discovery_configuration(&fixture, &selected.join(","), Some("sensor.")),
    );
    respond(
        &mut snapshot,
        200,
        json!([
            entity("sensor.z", "30"),
            entity("sensor.b", "2"),
            entity("sensor.a", "1"),
            entity("sensor.manual_0", "999")
        ]),
    );
    let frame = discovery_status(&mut worker, "Connected; Discovery limit reached");
    assert_eq!(
        discovery_items(&frame)
            .iter()
            .map(|item| item["title"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["sensor.a", "sensor.b"]
    );
    let b_id = discovered(&frame, "sensor.b").unwrap()["id"].clone();
    live(&mut socket, "sensor.z", Some(entity("sensor.z", "31")));
    live(&mut socket, "sensor.a", None);
    let removed = numeric_barrier(&mut worker, &mut socket, 2);
    assert_eq!(discovery_items(&removed).len(), 1);
    assert!(discovered(&removed, "sensor.z").is_none());
    live(&mut socket, "sensor.z", Some(entity("sensor.z", "32")));
    let added = numeric_barrier(&mut worker, &mut socket, 3);
    assert_eq!(discovery_items(&added).len(), 2);
    assert_eq!(discovered(&added, "sensor.z").unwrap()["value"], 32.0);
    assert_eq!(discovered(&added, "sensor.b").unwrap()["id"], b_id);
    assert_eq!(added["items"].as_array().unwrap().len(), 65);
    no_request(&fixture, Duration::from_millis(80));
    worker.stop(false);
}

#[test]
fn discovery_failure_never_revokes_explicit_actions_or_numeric_input_revisions() {
    let fixture = listener();
    let mut worker = Worker::start();
    let mut config = discovery_configuration(&fixture, "sensor.barrier", Some("sensor."));
    config["configuration"]["values"]["action_entities"] = json!("button.explicit");
    config["configuration"]["values"]["number_entities"] = json!("number.explicit");
    let (mut socket, mut snapshot) = start_discovery(&mut worker, &fixture, config);
    let before = numeric_connected(&mut worker, 1);
    let number = item(&before, "ha-number-0-set").unwrap().clone();
    worker.action("while-pending", "ha-action-0", 5000);
    let mut action = service(&fixture, "button", "button.explicit");
    respond(&mut snapshot, 500, json!({"private":PRIVATE_BODY}));
    let failed = discovery_status(&mut worker, "Connected; Discovery unavailable");
    assert_eq!(
        item(&failed, "ha-number-0-set").unwrap()["input_revision"],
        number["input_revision"]
    );
    assert!(item(&failed, "ha-action-0").is_some());
    respond(&mut action, 200, json!([]));
    worker.success("while-pending");
    numeric_action(&mut worker, "after-failure", &number, 0, 5000);
    let mut action = numeric_service(
        &fixture,
        "number",
        "set_value",
        r#"{"entity_id":"number.explicit","value":0.0}"#,
    );
    respond(&mut action, 200, json!([]));
    worker.success("after-failure");
    live(
        &mut socket,
        "sensor.ignored_after_failure",
        Some(entity("sensor.ignored_after_failure", "9")),
    );
    let frame = numeric_barrier(&mut worker, &mut socket, 2);
    assert!(discovery_items(&frame).is_empty());
    no_request(&fixture, Duration::from_millis(100));
    worker.stop(false);
}

#[test]
fn discovery_bootstrap_overflow_discards_snapshot_and_keeps_explicit_controls() {
    let fixture = listener();
    let mut worker = Worker::start();
    let mut config = discovery_configuration(&fixture, "sensor.barrier", Some("sensor."));
    config["configuration"]["values"]["action_entities"] = json!("button.explicit");
    let (mut socket, mut snapshot) = start_discovery(&mut worker, &fixture, config);
    for index in 0..129 {
        live(
            &mut socket,
            &format!("sensor.buffer_{index}"),
            Some(entity(
                &format!("sensor.buffer_{index}"),
                &index.to_string(),
            )),
        );
    }
    let failed = discovery_status(&mut worker, "Connected; Discovery unavailable");
    assert!(discovery_items(&failed).is_empty());
    assert!(item(&failed, "ha-action-0").is_some());
    respond(
        &mut snapshot,
        200,
        json!([entity("sensor.snapshot_only", "8")]),
    );
    let after = numeric_barrier(&mut worker, &mut socket, 2);
    assert!(discovery_items(&after).is_empty());
    worker.action("overflow-action", "ha-action-0", 5000);
    let mut action = service(&fixture, "button", "button.explicit");
    respond(&mut action, 200, json!([]));
    worker.success("overflow-action");
    no_request(&fixture, Duration::from_millis(80));
    worker.stop(false);
}

#[test]
fn discovery_malformed_duplicate_oversized_and_redirected_snapshots_fail_without_reconnect() {
    let too_many = Value::Array(
        (0..4097)
            .map(|i| entity(&format!("sensor.e{i}"), "1"))
            .collect(),
    );
    let cases = [
        json!({}),
        json!([null]),
        json!([{"state":"1"}]),
        json!([{"entity_id":42,"state":"1"}]),
        json!([{"entity_id":"sensor.missing"}]),
        json!([
            entity("sensor.duplicate", "1"),
            entity("sensor.duplicate", "2")
        ]),
        too_many,
    ];
    for body in cases {
        let fixture = listener();
        let mut worker = Worker::start();
        let mut config = discovery_configuration(&fixture, "sensor.barrier", Some("sensor."));
        config["configuration"]["values"]["action_entities"] = json!("button.explicit");
        let (_socket, mut snapshot) = start_discovery(&mut worker, &fixture, config);
        respond(&mut snapshot, 200, body);
        let failed = discovery_status(&mut worker, "Connected; Discovery unavailable");
        assert!(discovery_items(&failed).is_empty());
        assert!(item(&failed, "ha-action-0").is_some());
        no_request(&fixture, Duration::from_millis(60));
        worker.stop(false);
    }
    for streamed in [false, true] {
        let fixture = listener();
        let mut worker = Worker::start();
        let (_socket, mut snapshot) = start_discovery(
            &mut worker,
            &fixture,
            discovery_configuration(&fixture, "sensor.barrier", Some("sensor.")),
        );
        if streamed {
            write!(snapshot,"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n",1024*1024+1).unwrap();
            let _ = snapshot.write_all(&vec![b' '; 1024 * 1024 + 1]);
        } else {
            write!(
                snapshot,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                1024 * 1024 + 1
            )
            .unwrap();
        }
        let failed = discovery_status(&mut worker, "Connected; Discovery unavailable");
        assert!(discovery_items(&failed).is_empty());
        no_request(&fixture, Duration::from_millis(60));
        worker.stop(false);
    }
    let forbidden = listener();
    let fixture = listener();
    let mut worker = Worker::start();
    let (_socket, mut snapshot) = start_discovery(
        &mut worker,
        &fixture,
        discovery_configuration(&fixture, "sensor.barrier", Some("sensor.")),
    );
    write!(snapshot,"HTTP/1.1 302 Found\r\nLocation: http://{}/private\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",forbidden.local_addr().unwrap()).unwrap();
    discovery_status(&mut worker, "Connected; Discovery unavailable");
    no_request(&forbidden, Duration::from_millis(80));
    no_request(&fixture, Duration::from_millis(60));
    worker.stop(false);
}

#[test]
fn discovery_only_malformed_live_data_withdraws_rows_without_reconnecting() {
    let fixture = listener();
    let mut worker = Worker::start();
    let mut config = discovery_configuration(&fixture, "sensor.barrier", Some("sensor."));
    config["configuration"]["values"]["action_entities"] = json!("button.explicit");
    let (mut socket, mut snapshot) = start_discovery(&mut worker, &fixture, config);
    respond(&mut snapshot, 200, json!([entity("sensor.live", "1")]));
    worker.until(|frame| frame["type"] == "contributions" && discovery_items(frame).len() == 1);
    for (index, value) in [
        json!([]),
        json!("wrong"),
        json!({}),
        json!({"entity_id":"sensor.wrong","state":"1"}),
        json!({"entity_id":"sensor.live","state":false}),
    ]
    .into_iter()
    .enumerate()
    {
        live(&mut socket, "sensor.live", Some(value));
        let frame = numeric_barrier(&mut worker, &mut socket, 2 + index * 2);
        assert!(discovery_items(&frame).is_empty());
        assert!(item(&frame, "ha-action-0").is_some());
        live(&mut socket, "sensor.live", Some(entity("sensor.live", "2")));
        let restored = numeric_barrier(&mut worker, &mut socket, 3 + index * 2);
        assert_eq!(discovery_items(&restored).len(), 1);
    }
    worker.action("malformed-discovery-action", "ha-action-0", 5000);
    let mut action = service(&fixture, "button", "button.explicit");
    respond(&mut action, 200, json!([]));
    worker.success("malformed-discovery-action");
    no_request(&fixture, Duration::from_millis(80));
    worker.stop(false);
}

#[test]
fn discovery_auth_rejection_cancels_explicit_service_and_clears_session() {
    for status in [401, 403] {
        let fixture = listener();
        let mut worker = Worker::start();
        let mut config = discovery_configuration(&fixture, "sensor.barrier", Some("sensor."));
        config["configuration"]["values"]["action_entities"] = json!("button.explicit");
        let (_socket, mut snapshot) = start_discovery(&mut worker, &fixture, config);
        connected(&mut worker, 1);
        worker.action("active", "ha-action-0", 5000);
        let pending = service(&fixture, "button", "button.explicit");
        respond(&mut snapshot, status, json!({"private":PRIVATE_BODY}));
        worker.error("active", "outcome_unknown");
        let frame = discovery_status(&mut worker, "Authentication rejected");
        assert!(actions(&frame).is_empty());
        assert!(discovery_items(&frame).is_empty());
        worker.action("revoked", "ha-action-0", 5000);
        worker.error("revoked", "unavailable");
        drop(pending);
        no_request(&fixture, Duration::from_millis(80));
        worker.stop(false);
    }
}

#[test]
fn discovery_reconnect_resamples_with_new_ids_and_no_periodic_collection() {
    let fixture = listener();
    let mut worker = Worker::start();
    let (socket, mut snapshot) = start_discovery(
        &mut worker,
        &fixture,
        discovery_configuration(&fixture, "", Some("sensor.")),
    );
    respond(&mut snapshot, 200, json!([entity("sensor.old", "1")]));
    let first =
        worker.until(|frame| frame["type"] == "contributions" && discovery_items(frame).len() == 1);
    let old_id = discovered(&first, "sensor.old").unwrap()["id"].clone();
    drop(socket);
    let disconnected = discovery_status(&mut worker, "Disconnected");
    assert!(discovery_items(&disconnected).is_empty());
    let mut socket = authorize(&fixture, true);
    let mut snapshot = request(&fixture);
    assert_eq!(snapshot.line, "GET /reverse/proxy/ha/api/states HTTP/1.1");
    respond(
        &mut snapshot.stream,
        200,
        json!([entity("sensor.old", "2"), entity("sensor.new", "3")]),
    );
    let next =
        worker.until(|frame| frame["type"] == "contributions" && discovery_items(frame).len() == 2);
    assert_ne!(discovered(&next, "sensor.old").unwrap()["id"], old_id);
    live(&mut socket, "sensor.new", None);
    worker.until(|frame| frame["type"] == "contributions" && discovery_items(frame).len() == 1);
    no_request(&fixture, Duration::from_millis(150));
    worker.stop(false);
}

#[test]
fn discovery_snapshot_deadline_leaves_explicit_updates_and_application_heartbeat_live() {
    let fixture = listener();
    let mut worker = Worker::start();
    let (mut socket, _snapshot) = start_discovery(
        &mut worker,
        &fixture,
        discovery_configuration(&fixture, "sensor.barrier", Some("sensor.")),
    );
    let initial = numeric_barrier(&mut worker, &mut socket, 2);
    assert!(discovery_items(&initial).is_empty());
    // A held collection response must expire at its original 15-second deadline.
    // Read the heartbeat directly with a bounded socket deadline, then inspect
    // queued publications; Worker::until deliberately has a shorter 5s guard.
    socket
        .get_mut()
        .set_read_timeout(Some(Duration::from_secs(25)))
        .unwrap();
    let heartbeat = ws_json(&mut socket);
    assert_eq!(heartbeat["type"], "ping");
    ws_send(&mut socket, json!({"id":heartbeat["id"],"type":"pong"}));
    let failed = discovery_status(&mut worker, "Connected; Discovery unavailable");
    assert!(discovery_items(&failed).is_empty());
    let after = numeric_barrier(&mut worker, &mut socket, 3);
    assert_eq!(
        item(&after, "connection").unwrap()["value"],
        "Connected; Discovery unavailable"
    );
    no_request(&fixture, Duration::from_millis(80));
    worker.stop(false);
}

#[test]
fn discovery_prefix_configuration_is_strict_and_never_grants_action_domains() {
    let prefixes = (0..9)
        .map(|i| format!("sensor.e{i}"))
        .collect::<Vec<_>>()
        .join(",");
    for prefix in [
        "sensor".into(),
        "switch.".into(),
        "number.".into(),
        "cover.".into(),
        "button.".into(),
        "media_player.".into(),
        "sensor.*".into(),
        "sensor.[ab]".into(),
        "sensor.a.b".into(),
        "sensor.A".into(),
        format!("sensor.{}", "a".repeat(122)),
        " ".repeat(1025),
        prefixes,
    ] {
        let fixture = listener();
        let mut worker = Worker::start();
        let frame = discovery_configuration(&fixture, "", Some(&prefix));
        worker.send(hello());
        worker.next();
        worker.send(frame);
        worker.finish(false);
        no_request(&fixture, Duration::from_millis(10));
    }
}

#[test]
fn discovery_stalled_collection_shutdown_and_eof_do_not_wait_for_the_network_deadline() {
    for eof in [false, true] {
        let fixture = listener();
        let mut worker = Worker::start();
        let (_socket, _snapshot) = start_discovery(
            &mut worker,
            &fixture,
            discovery_configuration(&fixture, "", Some("sensor.")),
        );
        worker.stop(eof);
        no_request(&fixture, Duration::from_millis(50));
    }
}

#[test]
fn discovery_and_maximum_explicit_controls_fit_the_expanded_read_frame() {
    let fixture = listener();
    let mut worker = Worker::start();
    let buttons = (0..16).map(|i| format!("button.e{i}")).collect::<Vec<_>>();
    let media = (0..4)
        .map(|i| format!("media_player.e{i}"))
        .collect::<Vec<_>>();
    let mut config =
        discovery_configuration(&fixture, "sensor.barrier", Some("sensor.discovered_"));
    config["configuration"]["values"]["action_entities"] = json!(buttons.join(","));
    config["configuration"]["values"]["media_player_entities"] = json!(media.join(","));
    config["configuration"]["values"]["cover_entities"] = json!("cover.a");
    let (mut socket, mut snapshot) = start_discovery(&mut worker, &fixture, config);
    for name in buttons.iter().chain(&media) {
        let mut value = entity(name, &"\\\"".repeat(512));
        value["attributes"]["friendly_name"] = json!("\\\"".repeat(128));
        live(&mut socket, name, Some(value));
    }
    let mut states = (0..43)
        .map(|i| {
            entity(
                &format!("sensor.discovered_{i:02}_do_not_supply_charger"),
                &"\\\"".repeat(512),
            )
        })
        .collect::<Vec<_>>();
    for state in &mut states {
        state["attributes"]["friendly_name"] = json!("\\\"".repeat(128));
    }
    respond(&mut snapshot, 200, json!(states));
    discovery_status(&mut worker, "Connected; Discovery limit reached");
    let frame = numeric_barrier(&mut worker, &mut socket, 2);
    assert_eq!(frame["items"].as_array().unwrap().len(), 96);
    assert_eq!(actions(&frame).len(), 31);
    assert_eq!(discovery_items(&frame).len(), 42);
    assert!(serde_json::to_vec(&frame).unwrap().len() < MAX_FRAME);
    let ids = frame["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap())
        .collect::<HashSet<_>>();
    assert_eq!(ids.len(), 96);
    for item in discovery_items(&frame) {
        assert_eq!(item["kind"], "text");
        assert_eq!(item["title"].as_str().unwrap().len(), 64);
        assert_eq!(item["text"].as_str().unwrap().len(), 256);
    }
    worker.action(
        "discovery-no-service",
        discovery_items(&frame)[0]["id"].as_str().unwrap(),
        5000,
    );
    worker.error("discovery-no-service", "invalid_action");
    no_request(&fixture, Duration::from_millis(80));
    worker.stop(false);
}

#[test]
fn grouped_controls_follow_explicit_identity_across_duplicate_titles_and_live_withdrawal() {
    let fixture = listener();
    let mut worker = Worker::start();
    let mut config = discovery_configuration(
        &fixture,
        "sensor.barrier,cover.shade,number.second,switch.read_only,number.first,cover.shade",
        Some("sensor.discovered"),
    );
    for (field, value) in [
        ("action_entities", "button.first,scene.first"),
        ("media_player_entities", "media_player.den"),
        ("binary_entities", "switch.first"),
        ("cover_entities", "cover.shade"),
        ("number_entities", "number.first,number.second,number.first"),
        ("cover_position_entities", "cover.shade"),
    ] {
        config["configuration"]["values"][field] = json!(value);
    }
    let (mut socket, mut snapshot) = start_discovery(&mut worker, &fixture, config);
    for name in [
        "cover.shade",
        "number.second",
        "switch.read_only",
        "number.first",
        "button.first",
        "scene.first",
        "media_player.den",
        "switch.first",
    ] {
        let mut state = match name.split_once('.').unwrap().0 {
            "cover" => position_entity(name, "closed", json!(15), json!(20)),
            "number" => number_entity(name, "-0.3"),
            "switch" => entity(name, "off"),
            "media_player" => entity(name, "paused"),
            _ => entity(name, "unknown"),
        };
        state["attributes"]["friendly_name"] = json!("Same name");
        live(&mut socket, name, Some(state));
    }
    let mut discovered = entity("sensor.discovered_same_name", "off");
    discovered["attributes"]["friendly_name"] = json!("Same name");
    respond(&mut snapshot, 200, json!([discovered]));
    worker.until(|frame| frame["type"] == "contributions" && discovery_items(frame).len() == 1);
    let frame = numeric_barrier(&mut worker, &mut socket, 2);
    let mapping = [
        ("ha-action-0", "entity-5"),
        ("ha-action-1", "entity-6"),
        ("ha-media-0-play", "entity-7"),
        ("ha-media-0-pause", "entity-7"),
        ("ha-media-0-stop", "entity-7"),
        ("ha-binary-0-on", "entity-8"),
        ("ha-binary-0-off", "entity-8"),
        ("ha-cover-0-open", "entity-1"),
        ("ha-cover-0-close", "entity-1"),
        ("ha-cover-0-stop", "entity-1"),
        ("ha-number-0-set", "entity-4"),
        ("ha-number-1-set", "entity-2"),
        ("ha-cover-position-0-set", "entity-1"),
    ];
    assert_eq!(frame["items"].as_array().unwrap().len(), 24);
    assert_eq!(actions(&frame).len() + inputs(&frame).len(), mapping.len());
    for (id, state_id) in mapping {
        let control = item(&frame, id).unwrap();
        assert_eq!(control["state_id"], state_id);
        assert_eq!(control["title"], "Same name");
        assert_eq!(item(&frame, state_id).unwrap()["title"], "Same name");
    }
    assert!(frame["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item["state_id"] != "entity-3"));
    let first = item(&frame, "ha-number-0-set").unwrap().clone();
    let second = item(&frame, "ha-number-1-set").unwrap().clone();
    let position = item(&frame, "ha-cover-position-0-set").unwrap().clone();
    worker.action("grouped-button", "ha-action-0", 5000);
    let mut response = service(&fixture, "button", "button.first");
    respond(&mut response, 200, json!([]));
    worker.success("grouped-button");
    numeric_action(&mut worker, "grouped-second", &second, 0, 5000);
    let mut response = numeric_service(
        &fixture,
        "number",
        "set_value",
        r#"{"entity_id":"number.second","value":0.0}"#,
    );
    respond(&mut response, 200, json!([]));
    worker.success("grouped-second");
    worker.send(action_frame(
        "group-is-not-authority",
        "ha-action-0",
        json!({"state_id":"entity-3"}),
        5000,
    ));
    worker.error("group-is-not-authority", "invalid_action");
    live(&mut socket, "number.first", None);
    let mut state = position_entity("cover.shade", "opening", json!(4), json!(60));
    state["attributes"]["friendly_name"] = json!("Renamed shade");
    live(&mut socket, "cover.shade", Some(state));
    let frame = numeric_barrier(&mut worker, &mut socket, 3);
    assert!(item(&frame, "ha-number-0-set").is_none());
    assert!(item(&frame, "ha-cover-0-open").is_none());
    assert_eq!(item(&frame, "entity-4").unwrap()["value"], "Unavailable");
    assert_eq!(item(&frame, "ha-number-1-set").unwrap(), &second);
    let current_position = item(&frame, "ha-cover-position-0-set").unwrap();
    assert_eq!(current_position["state_id"], position["state_id"]);
    assert_eq!(
        current_position["input_revision"],
        position["input_revision"]
    );
    assert_eq!(current_position["title"], "Renamed shade");
    numeric_action(&mut worker, "withdrawn-group", &first, 0, 5000);
    worker.error("withdrawn-group", "unavailable");
    let mut restored = number_entity("number.first", "0");
    restored["attributes"]["friendly_name"] = json!("Renamed number");
    live(&mut socket, "number.first", Some(restored));
    let frame = numeric_barrier(&mut worker, &mut socket, 4);
    let restored = item(&frame, "ha-number-0-set").unwrap();
    assert_eq!(restored["state_id"], first["state_id"]);
    assert_ne!(restored["input_revision"], first["input_revision"]);
    assert_eq!(restored["title"], "Renamed number");
    assert_eq!(item(&frame, "ha-number-1-set").unwrap(), &second);
    no_request(&fixture, Duration::from_millis(50));
    worker.stop(false);
}

fn weather_entity(name: &str, condition: &str) -> Value {
    let mut value = entity(name, condition);
    value["attributes"]["temperature"] = json!(21.5);
    value["attributes"]["temperature_unit"] = json!("°C");
    value["attributes"]["unit_of_measurement"] = json!("must-not-be-used");
    value["attributes"]["forecast"] = json!([{
        "datetime":"2026-09-16T12:00:00+00:00", "condition":"cloudy",
        "temperature":23, "templow":14, "private":PRIVATE_BODY
    }]);
    value["attributes"]["private"] = json!(PRIVATE_BODY);
    value
}

fn initial_weather_states(
    worker: &mut Worker,
    fixture: &TcpListener,
    states: &[Value],
) -> WebSocket<TcpStream> {
    let names = states
        .iter()
        .map(|state| state["entity_id"].as_str().unwrap())
        .collect::<Vec<_>>();
    worker.configure(fixture, &names.join(","), None);
    let socket = authorize(fixture, true);
    let mut seen = HashSet::new();
    for _ in states {
        let mut request = request(fixture);
        let name = request
            .line
            .strip_prefix("GET /reverse/proxy/ha/api/states/")
            .and_then(|name| name.strip_suffix(" HTTP/1.1"))
            .expect("weather only reads explicitly selected states");
        assert!(seen.insert(name.to_owned()));
        assert!(request.body.is_empty());
        let state = states
            .iter()
            .find(|state| state["entity_id"] == name)
            .unwrap();
        respond(&mut request.stream, 200, state.clone());
    }
    socket
}

fn weather_text(worker: &mut Worker, id: &str, expected: &str) -> Value {
    worker.until(|frame| item(frame, id).is_some_and(|item| item["text"] == expected))
}

fn assert_weather_read_only(frame: &Value) {
    assert!(actions(frame).is_empty());
    assert!(inputs(frame).is_empty());
    assert!(frame["items"].as_array().unwrap().iter().all(|item| {
        matches!(item["kind"].as_str(), Some("text" | "metric" | "status"))
            && item.get("state_id").is_none()
    }));
}

#[test]
fn weather_explicit_rest_and_live_summaries_stay_read_only_and_keep_entity_identity() {
    let fixture = listener();
    let mut worker = Worker::start();
    let mut socket = initial_weather_states(
        &mut worker,
        &fixture,
        &[
            entity("sensor.barrier", "1"),
            weather_entity("weather.home", "sunny"),
        ],
    );
    let initial = weather_text(&mut worker, "entity-1", "Condition: sunny; Temperature: 21.5 °C\nForecast: 2026-09-16T12:00:00+00:00, Condition: cloudy, High: 23 °C, Low: 14 °C");
    assert_eq!(initial["items"].as_array().unwrap().len(), 3);
    assert_weather_read_only(&initial);
    live(
        &mut socket,
        "weather.unselected",
        Some(weather_entity("weather.unselected", "never-visible")),
    );
    live(
        &mut socket,
        "sensor.weather",
        Some(weather_entity("sensor.weather", "also-never-visible")),
    );
    let isolated = numeric_barrier(&mut worker, &mut socket, 2);
    assert_eq!(item(&isolated, "entity-1"), item(&initial, "entity-1"));
    assert!(!isolated.to_string().contains("never-visible"));
    let mut forecast_update = weather_entity("weather.home", "sunny");
    forecast_update["attributes"]["forecast"] = json!([{
        "datetime":"2026-09-17", "condition":"rainy", "temperature":19
    }]);
    live(&mut socket, "weather.home", Some(forecast_update.clone()));
    weather_text(&mut worker, "entity-1", "Condition: sunny; Temperature: 21.5 °C\nForecast: 2026-09-17, Condition: rainy, High: 19 °C");
    forecast_update["attributes"]
        .as_object_mut()
        .unwrap()
        .remove("forecast");
    live(&mut socket, "weather.home", Some(forecast_update));
    weather_text(
        &mut worker,
        "entity-1",
        "Condition: sunny; Temperature: 21.5 °C",
    );
    let mut update = weather_entity("weather.home", "rainy");
    update["attributes"]["friendly_name"] = json!("Same entity, new title");
    update["attributes"]["temperature"] = json!(-0.5);
    update["attributes"]
        .as_object_mut()
        .unwrap()
        .remove("forecast");
    live(&mut socket, "weather.home", Some(update));
    let changed = weather_text(
        &mut worker,
        "entity-1",
        "Condition: rainy; Temperature: -0.5 °C",
    );
    assert_eq!(
        item(&changed, "entity-1").unwrap()["title"],
        "Same entity, new title"
    );
    assert_weather_read_only(&changed);
    for (index, condition, expected) in
        [(0, "unknown", "Unknown"), (1, "unavailable", "Unavailable")]
    {
        live(
            &mut socket,
            "weather.home",
            Some(weather_entity("weather.home", condition)),
        );
        let frame = numeric_barrier(&mut worker, &mut socket, index + 3);
        let weather = item(&frame, "entity-1").unwrap();
        assert_eq!(weather["kind"], "status");
        assert_eq!(weather["value"], expected);
        assert!(weather.get("text").is_none());
        assert_weather_read_only(&frame);
    }
    live(&mut socket, "weather.home", None);
    let removed = numeric_barrier(&mut worker, &mut socket, 5);
    assert_eq!(item(&removed, "entity-1").unwrap()["value"], "Unavailable");
    live(
        &mut socket,
        "weather.home",
        Some(entity("weather.home", "cloudy")),
    );
    weather_text(&mut worker, "entity-1", "Condition: cloudy");
    for (index, action) in [
        "entity-1",
        "weather.home",
        "weather.get_forecasts",
        "ha-action-0",
        "ha-number-0-set",
    ]
    .iter()
    .enumerate()
    {
        let request = format!("weather-denied-{index}");
        worker.action(&request, action, 5000);
        worker.error(&request, "invalid_action");
    }
    // A weather-shaped attribute object on another domain retains its old state projection.
    live(
        &mut socket,
        "sensor.barrier",
        Some(weather_entity("sensor.barrier", "sunny")),
    );
    let generic = weather_text(&mut worker, "entity-0", "sunny");
    assert_weather_read_only(&generic);
    no_request(&fixture, Duration::from_millis(80));
    worker.stop(false);
}

#[test]
fn weather_temperature_metadata_requires_finite_json_numbers_and_an_explicit_source_unit() {
    let fixture = listener();
    let mut worker = Worker::start();
    let mut socket = initialize(&mut worker, &fixture, "sensor.barrier,weather.home", None);
    for (index, temperature) in [
        json!("21.5"),
        json!(true),
        Value::Null,
        json!([]),
        json!({}),
        serde_json::from_str::<Value>("1e999").unwrap(),
        serde_json::from_str::<Value>("0.123456789012345678901234567890125").unwrap(),
    ]
    .into_iter()
    .enumerate()
    {
        let mut value = entity("weather.home", "sunny");
        value["attributes"]["temperature"] = temperature;
        value["attributes"]["temperature_unit"] = json!("°C");
        live(&mut socket, "weather.home", Some(value));
        let frame = numeric_barrier(&mut worker, &mut socket, index + 1);
        assert_eq!(
            item(&frame, "entity-1").unwrap()["text"],
            "Condition: sunny"
        );
    }
    for (index, unit) in [
        None,
        Some(Value::Null),
        Some(json!("")),
        Some(json!("  \n\t")),
        Some(json!(false)),
        Some(json!("x".repeat(33))),
    ]
    .into_iter()
    .enumerate()
    {
        let mut value = entity("weather.home", "sunny");
        value["attributes"]["temperature"] = json!(21.5);
        value["attributes"]["unit_of_measurement"] = json!("°C");
        value["attributes"]["forecast"] =
            json!([{"datetime":"2026-09-16", "temperature":23, "templow":14}]);
        if let Some(unit) = unit {
            value["attributes"]["temperature_unit"] = unit;
        }
        live(&mut socket, "weather.home", Some(value));
        let frame = numeric_barrier(&mut worker, &mut socket, index + 20);
        assert_eq!(
            item(&frame, "entity-1").unwrap()["text"],
            "Condition: sunny"
        );
    }
    let mut value = entity("weather.home", "sunny");
    value["attributes"]["temperature"] = serde_json::from_str("2.15e1").unwrap();
    value["attributes"]["temperature_unit"] = json!("  °F\n ");
    live(&mut socket, "weather.home", Some(value));
    weather_text(
        &mut worker,
        "entity-1",
        "Condition: sunny; Temperature: 2.15e+1 °F",
    );
    for (index, condition) in [
        json!("\n\t"),
        json!("x".repeat(129)),
        json!(false),
        json!(12),
        Value::Null,
    ]
    .into_iter()
    .enumerate()
    {
        let mut value = weather_entity("weather.home", "sunny");
        value["state"] = condition;
        live(&mut socket, "weather.home", Some(value));
        let frame = numeric_barrier(&mut worker, &mut socket, index + 40);
        assert_eq!(item(&frame, "entity-1").unwrap()["value"], "Unavailable");
        assert_weather_read_only(&frame);
    }
    let mut missing = weather_entity("weather.home", "sunny");
    missing.as_object_mut().unwrap().remove("state");
    live(&mut socket, "weather.home", Some(missing));
    let frame = numeric_barrier(&mut worker, &mut socket, 50);
    assert_eq!(item(&frame, "entity-1").unwrap()["value"], "Unavailable");
    no_request(&fixture, Duration::from_millis(80));
    worker.stop(false);
}

#[test]
fn weather_legacy_forecasts_validate_dates_and_only_consider_the_first_five_source_entries() {
    let fixture = listener();
    let mut worker = Worker::start();
    let mut socket = initialize(&mut worker, &fixture, "sensor.barrier,weather.home", None);
    let mut value = entity("weather.home", "sunny");
    value["attributes"]["temperature_unit"] = json!("°C");
    value["attributes"]["forecast"] = json!([
        {"datetime":"2028-02-29", "condition":"cloudy"},
        {"datetime":"2026-09-16T12:00:00Z", "temperature":23},
        {"datetime":"2026-09-16T12:00:00.123456789+05:30", "templow":14},
        {"datetime":"2026-09-17", "condition":"rainy", "temperature":19, "templow":11},
        {"datetime":"2026-09-18", "condition":"windy"},
        {"datetime":"2026-09-19", "condition":"sixth-entry-must-not-appear"}
    ]);
    live(&mut socket, "weather.home", Some(value));
    let frame = weather_text(&mut worker, "entity-1", "Condition: sunny\nForecast: 2028-02-29, Condition: cloudy\nForecast: 2026-09-16T12:00:00Z, High: 23 °C\nForecast: 2026-09-16T12:00:00.123456789+05:30, Low: 14 °C\nForecast: 2026-09-17, Condition: rainy, High: 19 °C, Low: 11 °C");
    assert_weather_read_only(&frame);
    assert_eq!(
        item(&frame, "entity-1").unwrap()["text"]
            .as_str()
            .unwrap()
            .matches("Forecast:")
            .count(),
        4
    );
    for (index, date) in [
        "2026-02-29",
        "2026-09-31",
        "2026-00-01",
        "2026-01-00",
        "2026-09-16T25:00:00Z",
        "2026-09-16T12:60:00Z",
        "2026-09-16T12:00:00",
        "2026-09-16T12:00:00+24:00",
        "2026-09-16T12:00:00.1234567890Z",
        "2026-09-16\n",
        "not-a-date",
    ]
    .into_iter()
    .enumerate()
    {
        let mut value = entity("weather.home", "sunny");
        value["attributes"]["forecast"] =
            json!([{ "datetime":date, "condition":"must-not-appear" }]);
        live(&mut socket, "weather.home", Some(value));
        let frame = numeric_barrier(&mut worker, &mut socket, index + 1);
        assert_eq!(
            item(&frame, "entity-1").unwrap()["text"],
            "Condition: sunny"
        );
    }
    let mut value = entity("weather.home", "sunny");
    value["attributes"]["forecast"] = json!([
        null, {"datetime":"invalid","condition":"ignored"}, {"datetime":"2026-09-16"},
        {"datetime":false,"condition":"ignored"}, ["malformed"],
        {"datetime":"2026-09-17","condition":"do-not-backfill-the-first-five"}
    ]);
    live(&mut socket, "weather.home", Some(value));
    let frame = numeric_barrier(&mut worker, &mut socket, 30);
    assert_eq!(
        item(&frame, "entity-1").unwrap()["text"],
        "Condition: sunny"
    );
    no_request(&fixture, Duration::from_millis(80));
    worker.stop(false);
}

#[test]
fn weather_newer_live_attributes_override_initial_rest_and_reconnect_drops_old_summaries() {
    let fixture = listener();
    let mut worker = Worker::start();
    worker.configure(
        &fixture,
        "sensor.barrier,weather.home,sensor.after_weather",
        None,
    );
    let mut socket = authorize(&fixture, true);
    // The initial reader admits two requests. Hold the sensor response so the
    // third GET can only start after the weather completion reaches Book.initial.
    let mut initial = [request(&fixture), request(&fixture)];
    let weather_index = initial
        .iter()
        .position(|request| {
            request.line == "GET /reverse/proxy/ha/api/states/weather.home HTTP/1.1"
        })
        .unwrap();
    let sensor_index = 1 - weather_index;
    assert_eq!(
        initial[sensor_index].line,
        "GET /reverse/proxy/ha/api/states/sensor.barrier HTTP/1.1"
    );
    let mut newer = entity("weather.home", "rainy");
    newer["attributes"]["temperature"] = json!(7);
    newer["attributes"]["temperature_unit"] = json!("°C");
    live(&mut socket, "weather.home", Some(newer));
    weather_text(
        &mut worker,
        "entity-1",
        "Condition: rainy; Temperature: 7 °C",
    );
    respond(
        &mut initial[weather_index].stream,
        200,
        weather_entity("weather.home", "sunny"),
    );
    let mut third = request(&fixture);
    assert_eq!(
        third.line,
        "GET /reverse/proxy/ha/api/states/sensor.after_weather HTTP/1.1"
    );
    let after_rest = numeric_barrier(&mut worker, &mut socket, 5);
    assert_eq!(
        item(&after_rest, "entity-1").unwrap()["text"],
        "Condition: rainy; Temperature: 7 °C"
    );
    respond(
        &mut initial[sensor_index].stream,
        200,
        entity("sensor.barrier", "1"),
    );
    respond(&mut third.stream, 200, entity("sensor.after_weather", "3"));
    worker.until(|frame| item(frame, "entity-2").is_some_and(|item| item["value"] == 3.0));
    drop(socket);
    let disconnected = worker.until(|frame| {
        item(frame, "connection").is_some_and(|item| item["value"] == "Disconnected")
    });
    assert_eq!(
        item(&disconnected, "entity-1").unwrap()["value"],
        "Unavailable"
    );
    assert!(!disconnected.to_string().contains("Forecast:"));
    let mut socket = authorize(&fixture, true);
    let waiting = connected(&mut worker, 0);
    assert!(item(&waiting, "entity-1").unwrap().get("text").is_none());
    let mut seen = HashSet::new();
    for _ in 0..3 {
        let mut reinitial = request(&fixture);
        let name = reinitial
            .line
            .strip_prefix("GET /reverse/proxy/ha/api/states/")
            .and_then(|name| name.strip_suffix(" HTTP/1.1"))
            .unwrap();
        assert!(seen.insert(name.to_owned()));
        let state = match name {
            "weather.home" => "cloudy",
            "sensor.barrier" => "10",
            "sensor.after_weather" => "20",
            _ => panic!("reconnect must read only configured targets"),
        };
        respond(&mut reinitial.stream, 200, entity(name, state));
    }
    let frame = weather_text(&mut worker, "entity-1", "Condition: cloudy");
    assert_weather_read_only(&frame);
    live(
        &mut socket,
        "weather.home",
        Some(entity("weather.home", "sunny")),
    );
    weather_text(&mut worker, "entity-1", "Condition: sunny");
    no_request(&fixture, Duration::from_millis(80));
    worker.stop(true);
}

#[test]
fn weather_utf8_projection_fits_the_actual_64_item_frame_with_unchanged_control_budget() {
    let fixture = listener();
    let mut worker = Worker::start();
    let weather = (0..11)
        .map(|index| format!("weather.e{index}"))
        .collect::<Vec<_>>();
    let buttons = (0..16)
        .map(|index| format!("button.e{index}"))
        .collect::<Vec<_>>();
    let media = (0..4)
        .map(|index| format!("media_player.e{index}"))
        .collect::<Vec<_>>();
    let mut config = media_configuration(
        &fixture,
        &weather.join(","),
        Some(&buttons.join(",")),
        Some(&media.join(",")),
    );
    config["configuration"]["values"]["cover_entities"] = json!("cover.shade");
    let mut socket = initialize_configuration(&mut worker, &fixture, config);
    let condition = "🌦".repeat(32);
    let unit = "°".repeat(16);
    for name in &weather {
        let mut value = entity(name, &condition);
        value["attributes"]["friendly_name"] = json!("\"".repeat(128));
        value["attributes"]["temperature_unit"] = json!(unit);
        value["attributes"]["temperature"] =
            serde_json::from_str("12345678901234567890123456789012").unwrap();
        value["attributes"]["forecast"] = json!((0..5)
            .map(|index| json!({
                "datetime":format!("2026-09-{:02}", 16 + index),
                "condition":"\"".repeat(128),
                "temperature":23, "templow":14
            }))
            .collect::<Vec<_>>());
        live(&mut socket, name, Some(value));
    }
    let frame =
        worker.until(|frame| item(frame, "entity-10").is_some_and(|item| item["kind"] == "text"));
    assert_eq!(frame["items"].as_array().unwrap().len(), 64);
    assert_eq!(actions(&frame).len(), 31);
    let ids = frame["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap())
        .collect::<HashSet<_>>();
    assert_eq!(ids.len(), 64);
    assert!(serde_json::to_vec(&frame).unwrap().len() < MAX_FRAME);
    for index in 0..11 {
        let projected = item(&frame, &format!("entity-{index}")).unwrap();
        assert_eq!(projected["title"].as_str().unwrap().len(), 64);
        let text = projected["text"].as_str().unwrap();
        assert!(text.len() <= 256);
        assert!(text.starts_with(&format!(
            "Condition: {condition}; Temperature: 12345678901234567890123456789012 {unit}"
        )));
        assert!(
            !text.contains("Forecast:"),
            "oversized forecast segments must be omitted whole"
        );
        assert!(projected.get("state_id").is_none());
    }
    no_request(&fixture, Duration::from_millis(80));
    worker.stop(false);
}

fn dishwasher_configuration(
    fixture: &TcpListener,
    watch: &str,
    running: Option<&str>,
    duration: Option<&str>,
) -> Value {
    let mut frame = configuration(fixture, watch, None);
    for (field, value) in [
        ("dishwasher_running_entity", running),
        ("dishwasher_duration_entity", duration),
    ] {
        if let Some(value) = value {
            frame["configuration"]["values"][field] = json!(value);
        }
    }
    frame
}

fn profile_barrier(
    worker: &mut Worker,
    socket: &mut WebSocket<TcpStream>,
    id: &str,
    serial: usize,
) -> Value {
    live(
        socket,
        "sensor.barrier",
        Some(entity("sensor.barrier", &serial.to_string())),
    );
    worker.until(|frame| {
        item(frame, id).is_some_and(|item| item["value"].as_f64() == Some(serial as f64))
    })
}

#[test]
fn dishwasher_defaults_keep_ordinary_cards_and_role_only_configuration_reads_exact_deduplicated_targets(
) {
    for empty in [None, Some("")] {
        let fixture = listener();
        let mut worker = Worker::start();
        let config = dishwasher_configuration(
            &fixture,
            "sensor.barrier,binary_sensor.dishwasher,sensor.runtime",
            empty,
            empty,
        );
        let mut socket = initialize_configuration(&mut worker, &fixture, config);
        live(
            &mut socket,
            "binary_sensor.dishwasher",
            Some(entity("binary_sensor.dishwasher", "on")),
        );
        live(
            &mut socket,
            "sensor.runtime",
            Some(entity("sensor.runtime", "01:23:45")),
        );
        let frame = numeric_barrier(&mut worker, &mut socket, 1);
        assert_eq!(item(&frame, "entity-1").unwrap()["text"], "on");
        assert_eq!(item(&frame, "entity-2").unwrap()["text"], "01:23:45");
        assert_weather_read_only(&frame);
        no_request(&fixture, Duration::from_millis(30));
        worker.stop(false);
    }
    for (running, duration, expected, count) in [
        ("switch.dishwasher", None, "State: Idle", 2),
        (
            "binary_sensor.dishwasher",
            Some("sensor.runtime"),
            "State: Idle\nRuntime since midnight: 01:23:45",
            3,
        ),
    ] {
        let fixture = listener();
        let mut worker = Worker::start();
        let config = dishwasher_configuration(&fixture, "", Some(running), duration);
        let _socket = initialize_configuration(&mut worker, &fixture, config);
        let frame = weather_text(&mut worker, "entity-0", expected);
        assert_eq!(frame["items"].as_array().unwrap().len(), count);
        assert_weather_read_only(&frame);
        no_request(&fixture, Duration::from_millis(30));
        worker.stop(false);
    }
    let fixture = listener();
    let mut worker = Worker::start();
    let config = dishwasher_configuration(
        &fixture,
        "sensor.runtime,sensor.barrier,sensor.runtime",
        Some("binary_sensor.dishwasher"),
        Some("sensor.runtime"),
    );
    let mut socket = initialize_configuration(&mut worker, &fixture, config);
    let frame = weather_text(
        &mut worker,
        "entity-2",
        "State: Idle\nRuntime since midnight: 01:23:45",
    );
    assert_eq!(frame["items"].as_array().unwrap().len(), 4);
    assert_eq!(item(&frame, "entity-0").unwrap()["text"], "01:23:45");
    live(
        &mut socket,
        "binary_sensor.unselected",
        Some(entity("binary_sensor.unselected", "running")),
    );
    live(
        &mut socket,
        "sensor.unselected",
        Some(entity("sensor.unselected", "never-selected")),
    );
    let frame = profile_barrier(&mut worker, &mut socket, "entity-1", 2);
    assert_eq!(
        item(&frame, "entity-2").unwrap()["text"],
        "State: Idle\nRuntime since midnight: 01:23:45"
    );
    assert!(!frame.to_string().contains("never-selected"));
    assert_weather_read_only(&frame);
    for (index, action) in [
        "dishwasher",
        "dishwasher-profile",
        "entity-2",
        "entity-0",
        "binary_sensor.dishwasher",
        "ha-action-0",
    ]
    .iter()
    .enumerate()
    {
        let request = format!("dishwasher-denied-{index}");
        worker.action(&request, action, 5000);
        worker.error(&request, "invalid_action");
    }
    no_request(&fixture, Duration::from_millis(80));
    worker.stop(true);
}

#[test]
fn dishwasher_independent_role_updates_withdraw_stale_details_and_preserve_ordinary_duration_cards()
{
    let fixture = listener();
    let mut worker = Worker::start();
    let config = dishwasher_configuration(
        &fixture,
        "sensor.barrier",
        Some("binary_sensor.dishwasher"),
        Some("sensor.runtime"),
    );
    let mut socket = initialize_configuration(&mut worker, &fixture, config);
    weather_text(
        &mut worker,
        "entity-1",
        "State: Idle\nRuntime since midnight: 01:23:45",
    );
    for (index, state, expected) in [
        (0, "on", "Running"),
        (1, "running", "Running"),
        (2, "off", "Idle"),
        (3, "idle", "Idle"),
        (4, "rinsing", "rinsing"),
    ] {
        live(
            &mut socket,
            "binary_sensor.dishwasher",
            Some(entity("binary_sensor.dishwasher", state)),
        );
        let frame = numeric_barrier(&mut worker, &mut socket, index + 1);
        assert_eq!(
            item(&frame, "entity-1").unwrap()["text"],
            format!("State: {expected}\nRuntime since midnight: 01:23:45")
        );
    }
    let mut duration = entity("sensor.runtime", "37");
    duration["attributes"]["unit_of_measurement"] = json!("min");
    live(&mut socket, "sensor.runtime", Some(duration));
    let frame = numeric_barrier(&mut worker, &mut socket, 10);
    assert_eq!(
        item(&frame, "entity-1").unwrap()["text"],
        "State: rinsing\nRuntime since midnight: 37"
    );
    assert_eq!(item(&frame, "entity-2").unwrap()["kind"], "metric");
    assert_eq!(item(&frame, "entity-2").unwrap()["value"], 37.0);
    assert_eq!(item(&frame, "entity-2").unwrap()["unit"], "min");
    live(&mut socket, "sensor.runtime", None);
    let frame = numeric_barrier(&mut worker, &mut socket, 11);
    assert_eq!(item(&frame, "entity-1").unwrap()["text"], "State: rinsing");
    assert_eq!(item(&frame, "entity-2").unwrap()["value"], "Unavailable");
    for (index, state, expected) in [(0, "unknown", "Unknown"), (1, "unavailable", "Unavailable")] {
        live(
            &mut socket,
            "binary_sensor.dishwasher",
            Some(entity("binary_sensor.dishwasher", state)),
        );
        live(
            &mut socket,
            "sensor.runtime",
            Some(entity("sensor.runtime", "02:34:56")),
        );
        let frame = numeric_barrier(&mut worker, &mut socket, index + 20);
        assert_eq!(item(&frame, "entity-1").unwrap()["kind"], "status");
        assert_eq!(item(&frame, "entity-1").unwrap()["value"], expected);
        assert_eq!(item(&frame, "entity-2").unwrap()["text"], "02:34:56");
        assert!(item(&frame, "entity-1").unwrap().get("text").is_none());
    }
    live(&mut socket, "binary_sensor.dishwasher", None);
    live(
        &mut socket,
        "sensor.runtime",
        Some(entity("sensor.runtime", "03:00:00")),
    );
    let frame = numeric_barrier(&mut worker, &mut socket, 30);
    assert_eq!(item(&frame, "entity-1").unwrap()["value"], "Unavailable");
    let mut restored = entity("binary_sensor.dishwasher", "on");
    restored["attributes"]["friendly_name"] = json!("Renamed dishwasher");
    live(&mut socket, "binary_sensor.dishwasher", Some(restored));
    let frame = numeric_barrier(&mut worker, &mut socket, 31);
    assert_eq!(
        item(&frame, "entity-1").unwrap()["text"],
        "State: Running\nRuntime since midnight: 03:00:00"
    );
    assert_eq!(
        item(&frame, "entity-1").unwrap()["title"],
        "Renamed dishwasher"
    );
    assert_weather_read_only(&frame);
    no_request(&fixture, Duration::from_millis(80));
    worker.stop(false);
}

#[test]
fn dishwasher_runtime_is_a_complete_bounded_literal_without_units_countdowns_or_nonfinite_numbers()
{
    let fixture = listener();
    let mut worker = Worker::start();
    let config = dishwasher_configuration(
        &fixture,
        "sensor.barrier",
        Some("binary_sensor.dishwasher"),
        Some("sensor.runtime"),
    );
    let mut socket = initialize_configuration(&mut worker, &fixture, config);
    for (index, value) in [
        "",
        "unknown",
        "unavailable",
        "off",
        "idle",
        "NaN",
        "inf",
        "-inf",
        "1e999",
        "01:\n23:45",
        &"x".repeat(129),
    ]
    .into_iter()
    .enumerate()
    {
        live(
            &mut socket,
            "sensor.runtime",
            Some(entity("sensor.runtime", value)),
        );
        let frame = numeric_barrier(&mut worker, &mut socket, index + 1);
        assert_eq!(item(&frame, "entity-1").unwrap()["text"], "State: Idle");
    }
    for (index, raw, expected) in [
        (0, " 01:23:45 ", "01:23:45"),
        (1, "0", "0"),
        (2, "2.15e1", "2.15e1"),
        (3, "source supplied", "source supplied"),
    ] {
        let mut duration = entity("sensor.runtime", raw);
        duration["attributes"]["unit_of_measurement"] =
            json!("invented-unit-must-not-enter-summary");
        live(&mut socket, "sensor.runtime", Some(duration));
        let frame = numeric_barrier(&mut worker, &mut socket, index + 20);
        assert_eq!(
            item(&frame, "entity-1").unwrap()["text"],
            format!("State: Idle\nRuntime since midnight: {expected}")
        );
    }
    for (index, state) in [json!(false), json!(37), Value::Null]
        .into_iter()
        .enumerate()
    {
        let mut duration = entity("sensor.runtime", "01:23:45");
        duration["state"] = state;
        live(&mut socket, "sensor.runtime", Some(duration));
        let frame = numeric_barrier(&mut worker, &mut socket, index + 30);
        assert_eq!(item(&frame, "entity-1").unwrap()["text"], "State: Idle");
    }
    for (index, state) in [
        json!(false),
        json!(""),
        json!("on\nmalformed"),
        json!("x".repeat(129)),
    ]
    .into_iter()
    .enumerate()
    {
        let mut running = entity("binary_sensor.dishwasher", "on");
        running["state"] = state;
        live(&mut socket, "binary_sensor.dishwasher", Some(running));
        let frame = numeric_barrier(&mut worker, &mut socket, index + 40);
        assert_eq!(item(&frame, "entity-1").unwrap()["value"], "Unavailable");
    }
    no_request(&fixture, Duration::from_millis(80));
    worker.stop(false);
}

#[test]
fn dishwasher_configuration_rejects_multiple_or_invalid_roles_missing_primary_and_union_overflow() {
    let fixture = listener();
    for (running, duration) in [
        (None, Some("sensor.runtime")),
        (Some(""), Some("sensor.runtime")),
        (Some("sensor.same"), Some("sensor.same")),
        (Some(" sensor.same "), Some("sensor.same")),
        (Some("sensor.one,sensor.two"), None),
        (Some("sensor.one\nsensor.two"), None),
        (Some("sensor.*"), None),
        (Some("sensor.A"), None),
        (Some("sensor.a/b"), None),
        (Some("sensor.one"), Some("sensor.a,sensor.b")),
    ] {
        let mut worker = Worker::start();
        worker.send(hello());
        worker.next();
        worker.send(dishwasher_configuration(&fixture, "", running, duration));
        worker.finish(false);
    }
    let full = (0..64)
        .map(|index| format!("sensor.e{index}"))
        .collect::<Vec<_>>()
        .join(",");
    for config in [
        dishwasher_configuration(&fixture, &full, Some("sensor.extra"), None),
        dishwasher_configuration(&fixture, &full, Some("sensor.e0"), Some("sensor.extra")),
        dishwasher_configuration(
            &fixture,
            "",
            Some(&format!("{}sensor.a", " ".repeat(121))),
            None,
        ),
    ] {
        let mut worker = Worker::start();
        worker.send(hello());
        worker.next();
        worker.send(config);
        worker.finish(false);
    }
    no_request(&fixture, Duration::from_millis(50));
}

#[test]
fn dishwasher_both_late_initial_roles_are_processed_before_live_barriers_and_reconnect_clears_them()
{
    let fixture = listener();
    let mut worker = Worker::start();
    let watch = "binary_sensor.dishwasher,sensor.runtime,sensor.after_running,sensor.after_duration,sensor.barrier";
    worker.configure_frame(dishwasher_configuration(
        &fixture,
        watch,
        Some("binary_sensor.dishwasher"),
        Some("sensor.runtime"),
    ));
    let mut socket = authorize(&fixture, true);
    let mut initial = [request(&fixture), request(&fixture)];
    let running = initial
        .iter()
        .position(|request| {
            request.line == "GET /reverse/proxy/ha/api/states/binary_sensor.dishwasher HTTP/1.1"
        })
        .unwrap();
    let duration = 1 - running;
    assert_eq!(
        initial[duration].line,
        "GET /reverse/proxy/ha/api/states/sensor.runtime HTTP/1.1"
    );
    live(
        &mut socket,
        "binary_sensor.dishwasher",
        Some(entity("binary_sensor.dishwasher", "on")),
    );
    live(
        &mut socket,
        "sensor.runtime",
        Some(entity("sensor.runtime", "02:34:56")),
    );
    weather_text(
        &mut worker,
        "entity-0",
        "State: Running\nRuntime since midnight: 02:34:56",
    );
    respond(
        &mut initial[running].stream,
        200,
        entity("binary_sensor.dishwasher", "off"),
    );
    let mut third = request(&fixture);
    assert_eq!(
        third.line,
        "GET /reverse/proxy/ha/api/states/sensor.after_running HTTP/1.1"
    );
    let first_barrier = profile_barrier(&mut worker, &mut socket, "entity-4", 10);
    assert_eq!(
        item(&first_barrier, "entity-0").unwrap()["text"],
        "State: Running\nRuntime since midnight: 02:34:56"
    );
    respond(
        &mut initial[duration].stream,
        200,
        entity("sensor.runtime", "00:00:01"),
    );
    let mut fourth = request(&fixture);
    assert_eq!(
        fourth.line,
        "GET /reverse/proxy/ha/api/states/sensor.after_duration HTTP/1.1"
    );
    let second_barrier = profile_barrier(&mut worker, &mut socket, "entity-4", 11);
    assert_eq!(
        item(&second_barrier, "entity-0").unwrap()["text"],
        "State: Running\nRuntime since midnight: 02:34:56"
    );
    assert_eq!(
        item(&second_barrier, "entity-1").unwrap()["text"],
        "02:34:56"
    );
    respond(&mut third.stream, 200, entity("sensor.after_running", "1"));
    respond(
        &mut fourth.stream,
        200,
        entity("sensor.after_duration", "2"),
    );
    let mut last = request(&fixture);
    assert_eq!(
        last.line,
        "GET /reverse/proxy/ha/api/states/sensor.barrier HTTP/1.1"
    );
    respond(&mut last.stream, 200, entity("sensor.barrier", "0"));
    drop(socket);
    let disconnected = worker.until(|frame| {
        item(frame, "connection").is_some_and(|item| item["value"] == "Disconnected")
    });
    assert_eq!(
        item(&disconnected, "entity-0").unwrap()["value"],
        "Unavailable"
    );
    assert!(!disconnected.to_string().contains("Runtime since midnight"));
    let mut socket = authorize(&fixture, true);
    let waiting = connected(&mut worker, 0);
    assert!(item(&waiting, "entity-0").unwrap().get("text").is_none());
    let mut seen = HashSet::new();
    for _ in 0..5 {
        let mut request = request(&fixture);
        let name = request
            .line
            .strip_prefix("GET /reverse/proxy/ha/api/states/")
            .and_then(|name| name.strip_suffix(" HTTP/1.1"))
            .unwrap();
        assert!(seen.insert(name.to_owned()));
        assert!(watch.split(',').any(|selected| selected == name));
        let state = match name {
            "binary_sensor.dishwasher" => "off",
            "sensor.runtime" => "00:01:00",
            _ => "0",
        };
        respond(&mut request.stream, 200, entity(name, state));
    }
    let frame = weather_text(
        &mut worker,
        "entity-0",
        "State: Idle\nRuntime since midnight: 00:01:00",
    );
    assert_weather_read_only(&frame);
    live(&mut socket, "sensor.runtime", None);
    let frame = profile_barrier(&mut worker, &mut socket, "entity-4", 12);
    assert_eq!(item(&frame, "entity-0").unwrap()["text"], "State: Idle");
    no_request(&fixture, Duration::from_millis(50));
    worker.stop(true);
}

#[test]
fn dishwasher_profile_uses_existing_state_slots_and_keeps_31_controls_in_a_64_item_frame() {
    let fixture = listener();
    let mut worker = Worker::start();
    let watch = std::iter::once("sensor.barrier".to_owned())
        .chain((0..8).map(|index| format!("sensor.e{index}")))
        .collect::<Vec<_>>()
        .join(",");
    let mut config = dishwasher_configuration(
        &fixture,
        &watch,
        Some("binary_sensor.dishwasher"),
        Some("sensor.runtime"),
    );
    config["configuration"]["values"]["action_entities"] = json!((0..16)
        .map(|index| format!("button.e{index}"))
        .collect::<Vec<_>>()
        .join(","));
    config["configuration"]["values"]["media_player_entities"] = json!((0..4)
        .map(|index| format!("media_player.e{index}"))
        .collect::<Vec<_>>()
        .join(","));
    config["configuration"]["values"]["cover_entities"] = json!("cover.shade");
    let mut socket = initialize_configuration(&mut worker, &fixture, config);
    let literal = "🌦".repeat(32);
    let mut running = entity("binary_sensor.dishwasher", &literal);
    running["attributes"]["friendly_name"] = json!("\"".repeat(128));
    live(&mut socket, "binary_sensor.dishwasher", Some(running));
    live(
        &mut socket,
        "sensor.runtime",
        Some(entity("sensor.runtime", &"\"".repeat(128))),
    );
    let frame = numeric_barrier(&mut worker, &mut socket, 10);
    assert_eq!(frame["items"].as_array().unwrap().len(), 64);
    assert_eq!(actions(&frame).len(), 31);
    assert_eq!(
        item(&frame, "entity-30").unwrap()["title"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
    assert_eq!(
        item(&frame, "entity-30").unwrap()["text"],
        format!(
            "State: {literal}\nRuntime since midnight: {}",
            "\"".repeat(128)
        )
    );
    assert_eq!(item(&frame, "entity-31").unwrap()["text"], "\"".repeat(128));
    assert!(serde_json::to_vec(&frame).unwrap().len() < MAX_FRAME);
    assert_eq!(
        frame["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["id"].as_str().unwrap())
            .collect::<HashSet<_>>()
            .len(),
        64
    );
    worker.action("profile-card-not-action", "entity-30", 5000);
    worker.error("profile-card-not-action", "invalid_action");
    no_request(&fixture, Duration::from_millis(50));
    worker.stop(false);
}

fn laundry_configuration(
    fixture: &TcpListener,
    watch: &str,
    washer: Option<&str>,
    dryer: Option<&str>,
) -> Value {
    let mut frame = configuration(fixture, watch, None);
    for (field, value) in [
        ("washer_remaining_entity", washer),
        ("dryer_remaining_entity", dryer),
    ] {
        if let Some(value) = value {
            frame["configuration"]["values"][field] = json!(value);
        }
    }
    frame
}

#[test]
fn laundry_defaults_role_only_selections_and_watch_overlap_preserve_exact_read_scope() {
    for empty in [None, Some("")] {
        let fixture = listener();
        let mut worker = Worker::start();
        let config = laundry_configuration(
            &fixture,
            "sensor.barrier,sensor.washer,sensor.dryer",
            empty,
            empty,
        );
        let mut socket = initialize_configuration(&mut worker, &fixture, config);
        live(
            &mut socket,
            "sensor.washer",
            Some(entity("sensor.washer", "00:25:00")),
        );
        live(
            &mut socket,
            "sensor.dryer",
            Some(entity("sensor.dryer", "00:40:00")),
        );
        let frame = numeric_barrier(&mut worker, &mut socket, 1);
        assert_eq!(item(&frame, "entity-1").unwrap()["text"], "00:25:00");
        assert_eq!(item(&frame, "entity-2").unwrap()["text"], "00:40:00");
        assert_weather_read_only(&frame);
        no_request(&fixture, Duration::from_millis(30));
        worker.stop(false);
    }
    for (washer, dryer, expected) in [
        (Some("sensor.washer"), None, "Remaining time: 00:25:00"),
        (None, Some("sensor.dryer"), "Remaining time: 00:40:00"),
    ] {
        let fixture = listener();
        let mut worker = Worker::start();
        let config = laundry_configuration(&fixture, "", washer, dryer);
        let _socket = initialize_configuration(&mut worker, &fixture, config);
        let frame = weather_text(&mut worker, "entity-0", expected);
        assert_eq!(frame["items"].as_array().unwrap().len(), 2);
        assert_weather_read_only(&frame);
        no_request(&fixture, Duration::from_millis(30));
        worker.stop(false);
    }
    let fixture = listener();
    let mut worker = Worker::start();
    let config = laundry_configuration(
        &fixture,
        "sensor.dryer,sensor.barrier,sensor.dryer",
        Some("sensor.washer"),
        Some("sensor.dryer"),
    );
    let mut socket = initialize_configuration(&mut worker, &fixture, config);
    worker.until(|frame| {
        item(frame, "entity-2").is_some_and(|item| item["text"] == "Remaining time: 00:25:00")
            && item(frame, "entity-0")
                .is_some_and(|item| item["text"] == "Remaining time: 00:40:00")
    });
    live(
        &mut socket,
        "sensor.unselected",
        Some(entity("sensor.unselected", "never-selected")),
    );
    let frame = profile_barrier(&mut worker, &mut socket, "entity-1", 2);
    assert_eq!(frame["items"].as_array().unwrap().len(), 4);
    assert_eq!(
        item(&frame, "entity-0").unwrap()["text"],
        "Remaining time: 00:40:00"
    );
    assert!(!frame.to_string().contains("never-selected"));
    assert_weather_read_only(&frame);
    for (index, action) in [
        "washer",
        "dryer",
        "entity-0",
        "entity-2",
        "sensor.washer",
        "ha-action-0",
    ]
    .iter()
    .enumerate()
    {
        let request = format!("laundry-denied-{index}");
        worker.action(&request, action, 5000);
        worker.error(&request, "invalid_action");
    }
    no_request(&fixture, Duration::from_millis(50));
    worker.stop(true);
}

#[test]
fn laundry_updates_preserve_whole_literals_and_withdraw_each_role_independently() {
    let fixture = listener();
    let mut worker = Worker::start();
    let config = laundry_configuration(
        &fixture,
        "sensor.barrier",
        Some("sensor.washer"),
        Some("sensor.dryer"),
    );
    let mut socket = initialize_configuration(&mut worker, &fixture, config);
    worker.until(|frame| {
        item(frame, "entity-1").is_some_and(|item| item["text"] == "Remaining time: 00:25:00")
            && item(frame, "entity-2")
                .is_some_and(|item| item["text"] == "Remaining time: 00:40:00")
    });
    for (index, state, expected) in [
        (0, " 00:15:00 ", "00:15:00"),
        (1, "0", "0"),
        (2, "2.15e1", "2.15e1"),
        (3, "running", "running"),
        (4, "source supplied", "source supplied"),
    ] {
        let mut value = entity("sensor.washer", state);
        value["attributes"]["unit_of_measurement"] = json!("must-not-be-inferred");
        live(&mut socket, "sensor.washer", Some(value));
        let frame = numeric_barrier(&mut worker, &mut socket, index + 1);
        assert_eq!(
            item(&frame, "entity-1").unwrap()["text"],
            format!("Remaining time: {expected}")
        );
        assert_eq!(
            item(&frame, "entity-2").unwrap()["text"],
            "Remaining time: 00:40:00"
        );
    }
    for (index, state, expected) in [
        (0, "off", "Idle"),
        (1, "idle", "Idle"),
        (2, "unknown", "Unknown"),
        (3, "unavailable", "Unavailable"),
    ] {
        live(
            &mut socket,
            "sensor.washer",
            Some(entity("sensor.washer", state)),
        );
        let frame = numeric_barrier(&mut worker, &mut socket, index + 20);
        let washer = item(&frame, "entity-1").unwrap();
        assert_eq!(washer["kind"], "status");
        assert_eq!(washer["value"], expected);
        assert_eq!(washer["tone"], "neutral");
        assert!(washer.get("text").is_none());
        assert_eq!(
            item(&frame, "entity-2").unwrap()["text"],
            "Remaining time: 00:40:00"
        );
    }
    for (index, state) in [
        json!(false),
        json!(25),
        Value::Null,
        json!(""),
        json!("NaN"),
        json!("1e999"),
        json!("-inf"),
        json!("00:\n15:00"),
        json!("x".repeat(129)),
    ]
    .into_iter()
    .enumerate()
    {
        let mut value = entity("sensor.washer", "00:15:00");
        value["state"] = state;
        live(&mut socket, "sensor.washer", Some(value));
        let frame = numeric_barrier(&mut worker, &mut socket, index + 30);
        assert_eq!(item(&frame, "entity-1").unwrap()["value"], "Unavailable");
        assert_eq!(
            item(&frame, "entity-2").unwrap()["text"],
            "Remaining time: 00:40:00"
        );
    }
    live(&mut socket, "sensor.dryer", None);
    let frame = numeric_barrier(&mut worker, &mut socket, 50);
    assert_eq!(item(&frame, "entity-2").unwrap()["value"], "Unavailable");
    let mut restored = entity("sensor.washer", "00:10:00");
    restored["attributes"]["friendly_name"] = json!("Renamed washer");
    live(&mut socket, "sensor.washer", Some(restored));
    live(
        &mut socket,
        "sensor.dryer",
        Some(entity("sensor.dryer", "00:20:00")),
    );
    let frame = numeric_barrier(&mut worker, &mut socket, 51);
    assert_eq!(
        item(&frame, "entity-1").unwrap()["text"],
        "Remaining time: 00:10:00"
    );
    assert_eq!(item(&frame, "entity-1").unwrap()["title"], "Renamed washer");
    assert_eq!(
        item(&frame, "entity-2").unwrap()["text"],
        "Remaining time: 00:20:00"
    );
    assert_weather_read_only(&frame);
    no_request(&fixture, Duration::from_millis(50));
    worker.stop(false);
}

#[test]
fn laundry_rejects_invalid_single_roles_conflicting_overlays_and_union_overflow() {
    let fixture = listener();
    let mut configurations = Vec::new();
    for (washer, dryer) in [
        (Some("sensor.same"), Some("sensor.same")),
        (Some(" sensor.same "), Some("sensor.same")),
        (Some("sensor.one,sensor.two"), None),
        (None, Some("sensor.one\nsensor.two")),
        (Some("sensor.*"), None),
        (None, Some("sensor.A")),
        (Some("sensor.a/b"), None),
    ] {
        configurations.push(laundry_configuration(&fixture, "", washer, dryer));
    }
    for field in ["washer_remaining_entity", "dryer_remaining_entity"] {
        let mut duplicate = laundry_configuration(&fixture, "", None, None);
        duplicate["configuration"]["values"]["dishwasher_running_entity"] = json!("sensor.running");
        duplicate["configuration"]["values"][field] = json!("sensor.running");
        configurations.push(duplicate);
        let mut oversized = laundry_configuration(&fixture, "", None, None);
        oversized["configuration"]["values"][field] = json!(format!("{}sensor.a", " ".repeat(121)));
        configurations.push(oversized);
    }
    let full = (0..64)
        .map(|index| format!("sensor.e{index}"))
        .collect::<Vec<_>>()
        .join(",");
    configurations.push(laundry_configuration(
        &fixture,
        &full,
        Some("sensor.extra"),
        None,
    ));
    configurations.push(laundry_configuration(
        &fixture,
        &full,
        Some("sensor.e0"),
        Some("sensor.extra"),
    ));
    for config in configurations {
        let mut worker = Worker::start();
        worker.send(hello());
        worker.next();
        worker.send(config);
        worker.finish(false);
    }
    no_request(&fixture, Duration::from_millis(50));
}

#[test]
fn laundry_profile_overlaps_keep_raw_numeric_grants_and_only_explicit_buttons_can_write() {
    let fixture = listener();
    let mut worker = Worker::start();
    let mut config = laundry_configuration(
        &fixture,
        "sensor.barrier",
        Some("number.washer"),
        Some("sensor.shared_runtime"),
    );
    config["configuration"]["values"]["action_entities"] =
        json!("button.washer_start,button.washer_pause");
    config["configuration"]["values"]["number_entities"] = json!("number.washer");
    config["configuration"]["values"]["dishwasher_running_entity"] =
        json!("binary_sensor.dishwasher");
    config["configuration"]["values"]["dishwasher_duration_entity"] =
        json!("sensor.shared_runtime");
    let mut socket = initialize_configuration(&mut worker, &fixture, config);
    let frame = worker.until(|frame| {
        frame["type"] == "contributions"
            && actions(frame).len() == 2
            && inputs(frame).len() == 1
            && item(frame, "entity-4")
                .is_some_and(|item| item["text"] == "State: Idle\nRuntime since midnight: 01:23:45")
            && item(frame, "entity-5")
                .is_some_and(|item| item["text"] == "Remaining time: 01:23:45")
    });
    assert_eq!(frame["items"].as_array().unwrap().len(), 10);
    assert_eq!(actions(&frame).len(), 2);
    assert_eq!(inputs(&frame).len(), 1);
    assert_eq!(
        item(&frame, "entity-3").unwrap()["text"],
        "Remaining time: -0.3"
    );
    assert_eq!(
        item(&frame, "entity-4").unwrap()["text"],
        "State: Idle\nRuntime since midnight: 01:23:45"
    );
    assert_eq!(
        item(&frame, "entity-5").unwrap()["text"],
        "Remaining time: 01:23:45"
    );
    let input = item(&frame, "ha-number-0-set").unwrap().clone();
    assert_eq!(input["state_id"], "entity-3");
    live(
        &mut socket,
        "number.washer",
        Some(number_entity("number.washer", "0.1")),
    );
    live(
        &mut socket,
        "sensor.shared_runtime",
        Some(entity("sensor.shared_runtime", "02:00:00")),
    );
    let updated = numeric_barrier(&mut worker, &mut socket, 2);
    assert_eq!(
        item(&updated, "entity-3").unwrap()["text"],
        "Remaining time: 0.1"
    );
    assert_eq!(
        item(&updated, "ha-number-0-set").unwrap()["input_revision"],
        input["input_revision"]
    );
    assert_eq!(
        item(&updated, "entity-4").unwrap()["text"],
        "State: Idle\nRuntime since midnight: 02:00:00"
    );
    assert_eq!(
        item(&updated, "entity-5").unwrap()["text"],
        "Remaining time: 02:00:00"
    );
    worker.action("no-profile-action", "entity-3", 5000);
    worker.error("no-profile-action", "invalid_action");
    no_request(&fixture, Duration::from_millis(30));
    for (index, target) in ["button.washer_start", "button.washer_pause"]
        .iter()
        .enumerate()
    {
        let request = format!("explicit-washer-button-{index}");
        worker.action(&request, &format!("ha-action-{index}"), 5000);
        let mut response = service(&fixture, "button", target);
        respond(&mut response, 200, json!([]));
        worker.success(&request);
    }
    numeric_action(&mut worker, "explicit-number", &input, 0, 5000);
    let mut response = numeric_service(
        &fixture,
        "number",
        "set_value",
        r#"{"entity_id":"number.washer","value":0.0}"#,
    );
    respond(&mut response, 200, json!([]));
    worker.success("explicit-number");
    let frame = numeric_barrier(&mut worker, &mut socket, 3);
    assert_eq!(
        item(&frame, "entity-3").unwrap()["text"],
        "Remaining time: 0.1"
    );
    no_request(&fixture, Duration::from_millis(50));
    worker.stop(false);
}

#[test]
fn laundry_both_late_initial_roles_use_positive_completion_barriers_and_reconnect_clears_values() {
    let fixture = listener();
    let mut worker = Worker::start();
    let watch = "sensor.washer,sensor.dryer,sensor.after_washer,sensor.after_dryer,sensor.barrier";
    worker.configure_frame(laundry_configuration(
        &fixture,
        watch,
        Some("sensor.washer"),
        Some("sensor.dryer"),
    ));
    let mut socket = authorize(&fixture, true);
    let mut initial = [request(&fixture), request(&fixture)];
    let washer = initial
        .iter()
        .position(|request| {
            request.line == "GET /reverse/proxy/ha/api/states/sensor.washer HTTP/1.1"
        })
        .unwrap();
    let dryer = 1 - washer;
    assert_eq!(
        initial[dryer].line,
        "GET /reverse/proxy/ha/api/states/sensor.dryer HTTP/1.1"
    );
    live(
        &mut socket,
        "sensor.washer",
        Some(entity("sensor.washer", "00:11:00")),
    );
    live(
        &mut socket,
        "sensor.dryer",
        Some(entity("sensor.dryer", "00:22:00")),
    );
    weather_text(&mut worker, "entity-1", "Remaining time: 00:22:00");
    respond(
        &mut initial[washer].stream,
        200,
        entity("sensor.washer", "00:55:00"),
    );
    let mut third = request(&fixture);
    assert_eq!(
        third.line,
        "GET /reverse/proxy/ha/api/states/sensor.after_washer HTTP/1.1"
    );
    let first = profile_barrier(&mut worker, &mut socket, "entity-4", 10);
    assert_eq!(
        item(&first, "entity-0").unwrap()["text"],
        "Remaining time: 00:11:00"
    );
    assert_eq!(
        item(&first, "entity-1").unwrap()["text"],
        "Remaining time: 00:22:00"
    );
    respond(
        &mut initial[dryer].stream,
        200,
        entity("sensor.dryer", "00:44:00"),
    );
    let mut fourth = request(&fixture);
    assert_eq!(
        fourth.line,
        "GET /reverse/proxy/ha/api/states/sensor.after_dryer HTTP/1.1"
    );
    let second = profile_barrier(&mut worker, &mut socket, "entity-4", 11);
    assert_eq!(
        item(&second, "entity-0").unwrap()["text"],
        "Remaining time: 00:11:00"
    );
    assert_eq!(
        item(&second, "entity-1").unwrap()["text"],
        "Remaining time: 00:22:00"
    );
    respond(&mut third.stream, 200, entity("sensor.after_washer", "1"));
    respond(&mut fourth.stream, 200, entity("sensor.after_dryer", "2"));
    let mut last = request(&fixture);
    assert_eq!(
        last.line,
        "GET /reverse/proxy/ha/api/states/sensor.barrier HTTP/1.1"
    );
    respond(&mut last.stream, 200, entity("sensor.barrier", "0"));
    drop(socket);
    let disconnected = worker.until(|frame| {
        item(frame, "connection").is_some_and(|item| item["value"] == "Disconnected")
    });
    assert_eq!(
        item(&disconnected, "entity-0").unwrap()["value"],
        "Unavailable"
    );
    assert_eq!(
        item(&disconnected, "entity-1").unwrap()["value"],
        "Unavailable"
    );
    assert!(!disconnected.to_string().contains("Remaining time:"));
    let mut socket = authorize(&fixture, true);
    let waiting = connected(&mut worker, 0);
    assert!(item(&waiting, "entity-0").unwrap().get("text").is_none());
    assert!(item(&waiting, "entity-1").unwrap().get("text").is_none());
    let mut seen = HashSet::new();
    for _ in 0..5 {
        let mut request = request(&fixture);
        let name = request
            .line
            .strip_prefix("GET /reverse/proxy/ha/api/states/")
            .and_then(|name| name.strip_suffix(" HTTP/1.1"))
            .unwrap();
        assert!(seen.insert(name.to_owned()));
        assert!(watch.split(',').any(|selected| selected == name));
        let value = match name {
            "sensor.washer" => "00:10:00",
            "sensor.dryer" => "00:20:00",
            _ => "0",
        };
        respond(&mut request.stream, 200, entity(name, value));
    }
    weather_text(&mut worker, "entity-1", "Remaining time: 00:20:00");
    let frame = profile_barrier(&mut worker, &mut socket, "entity-4", 12);
    assert_eq!(
        item(&frame, "entity-0").unwrap()["text"],
        "Remaining time: 00:10:00"
    );
    assert_weather_read_only(&frame);
    no_request(&fixture, Duration::from_millis(50));
    worker.stop(true);
}

#[test]
fn laundry_profiles_keep_the_32_state_and_31_control_budget_inside_one_64_item_frame() {
    let fixture = listener();
    let mut worker = Worker::start();
    let watch = std::iter::once("sensor.barrier".to_owned())
        .chain((0..8).map(|index| format!("sensor.e{index}")))
        .collect::<Vec<_>>()
        .join(",");
    let mut config = laundry_configuration(
        &fixture,
        &watch,
        Some("sensor.washer"),
        Some("sensor.dryer"),
    );
    config["configuration"]["values"]["action_entities"] = json!((0..16)
        .map(|index| format!("button.e{index}"))
        .collect::<Vec<_>>()
        .join(","));
    config["configuration"]["values"]["media_player_entities"] = json!((0..4)
        .map(|index| format!("media_player.e{index}"))
        .collect::<Vec<_>>()
        .join(","));
    config["configuration"]["values"]["cover_entities"] = json!("cover.shade");
    let mut socket = initialize_configuration(&mut worker, &fixture, config);
    connected(&mut worker, 31);
    let washer = "🌦".repeat(32);
    let dryer = "\"".repeat(128);
    for (name, value) in [("sensor.washer", &washer), ("sensor.dryer", &dryer)] {
        let mut state = entity(name, value);
        state["attributes"]["friendly_name"] = json!("\"".repeat(128));
        live(&mut socket, name, Some(state));
    }
    let frame = numeric_barrier(&mut worker, &mut socket, 10);
    assert_eq!(frame["items"].as_array().unwrap().len(), 64);
    assert_eq!(actions(&frame).len(), 31);
    for (index, value) in [(30, washer), (31, dryer)] {
        let state = item(&frame, &format!("entity-{index}")).unwrap();
        assert_eq!(state["title"].as_str().unwrap().len(), 64);
        assert_eq!(state["text"], format!("Remaining time: {value}"));
        assert!(state.get("state_id").is_none());
    }
    assert!(serde_json::to_vec(&frame).unwrap().len() < MAX_FRAME);
    assert_eq!(
        frame["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["id"].as_str().unwrap())
            .collect::<HashSet<_>>()
            .len(),
        64
    );
    no_request(&fixture, Duration::from_millis(50));
    worker.stop(false);
}

#[test]
fn compact_catalog_and_opt_in_notifications_cross_real_worker_pipes_without_granting_commands() {
    let fixture = listener();
    let mut worker = Worker::start();
    let mut config = configuration(&fixture, "switch.desk", None);
    config["configuration"]["values"]
        .as_object_mut()
        .unwrap()
        .remove("dashboard_layout");
    config["configuration"]["values"]["notify_home"] = json!(true);
    worker.configure_frame(config);
    let mut socket = authorize(&fixture, true);
    for _ in 0..2 {
        let mut requested = request(&fixture);
        match requested.line.as_str() {
            "GET /reverse/proxy/ha/api/states HTTP/1.1" => respond(
                &mut requested.stream,
                200,
                json!([
                    entity("switch.desk", "off"),
                    entity("fan.catalog_only", "off")
                ]),
            ),
            "GET /reverse/proxy/ha/api/states/switch.desk HTTP/1.1" => {
                respond(&mut requested.stream, 200, entity("switch.desk", "off"))
            }
            other => panic!("unexpected request {other}"),
        }
    }
    let catalog =
        worker.until(|frame| frame["type"] == "event" && frame["name"] == "settings_choices");
    assert_eq!(catalog["data"]["offset"], 0);
    assert_eq!(catalog["data"]["complete"], true);
    assert_eq!(catalog["data"]["options"].as_array().unwrap().len(), 2);
    let initial =
        worker.until(|frame| item(frame, "entity-0").is_some_and(|item| item["text"] == "off"));
    assert!(actions(&initial).is_empty());
    assert_eq!(initial["items"].as_array().unwrap().len(), 2);
    assert!(!worker
        .deferred
        .iter()
        .any(|frame| frame["type"] == "notification"));
    let mut update = entity("switch.desk", "on");
    update["attributes"]["friendly_name"] = json!("My desk");
    live(&mut socket, "switch.desk", Some(update));
    let notification = worker.until(|frame| frame["type"] == "notification");
    assert_eq!(notification["title"], "Home Control");
    assert_eq!(notification["body"], "My desk: ON");
    worker.action("read-only", "ha-primary-0", 5000);
    worker.error("read-only", "invalid_action");
    no_request(&fixture, Duration::ZERO);
    worker.stop(false);
}
