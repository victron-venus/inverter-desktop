//! Explicit acceptance for an actual signed, separately built HA worker package.
//! The HA server and broker are disposable loopback fixtures. No core configuration,
//! production HA account, native keychain, or OS notification adapter is accessed.

use super::application::PackageApplication;
use super::frigate_integration_tests::{required_file, Broker};
use super::package::{PublisherTrust, TrustStore};
use super::packaging::{build_package, write_package_atomic};
use super::protocol::{DashboardContribution, PluginManifest};
use super::runtime::{PluginHost, WorkerState};
use ed25519_dalek::SigningKey;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::{sleep, timeout};
use tokio_tungstenite::tungstenite::Message;

const PLUGIN: &str = "inverter-desktop.home-assistant";
const WAIT: Duration = Duration::from_secs(15);
const FIRST_PREFIX: &str = "/proxy/ha/";
const SECOND_PREFIX: &str = "/updated%20ha/";
const PRIVATE_ATTRIBUTE: &str = "private-attribute-must-never-reach-host";

#[derive(Clone)]
struct Request {
    path: String,
    operation: &'static str,
}

#[derive(Default)]
struct Observations {
    requests: Vec<Request>,
    violations: Vec<&'static str>,
}

impl Observations {
    fn record(&mut self, path: &str, operation: &'static str) {
        assert!(self.requests.len() < 128, "bounded HA acceptance traffic");
        self.requests.push(Request {
            path: path.into(),
            operation,
        });
    }
}

struct ActiveSocket(Arc<AtomicUsize>);

impl Drop for ActiveSocket {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

struct HomeAssistant {
    address: String,
    first_token: String,
    second_token: String,
    observed: Arc<Mutex<Observations>>,
    active: Arc<AtomicUsize>,
    events: broadcast::Sender<Value>,
    task: JoinHandle<()>,
}

impl HomeAssistant {
    async fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let first_token = format!("disposable-{}", uuid::Uuid::new_v4());
        let second_token = format!("rotated-{}", uuid::Uuid::new_v4());
        let credentials = Arc::new(BTreeMap::from([
            (FIRST_PREFIX.to_owned(), first_token.clone()),
            (SECOND_PREFIX.to_owned(), second_token.clone()),
        ]));
        let observed = Arc::new(Mutex::new(Observations::default()));
        let active = Arc::new(AtomicUsize::new(0));
        let (events, _) = broadcast::channel(16);
        let observations = observed.clone();
        let active_sockets = active.clone();
        let broadcasts = events.clone();
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (stream, _) = accepted.unwrap();
                        let observed = observations.clone();
                        let credentials = credentials.clone();
                        let active = active_sockets.clone();
                        let events = broadcasts.subscribe();
                        connections.spawn(async move {
                            if serve_connection(stream, credentials, observed.clone(), active, events).await.is_err() {
                                observed.lock().unwrap().violations.push("HA fixture connection failed");
                            }
                        });
                    }
                    completed = connections.join_next(), if !connections.is_empty() => {
                        if completed.is_some_and(|result| result.is_err()) {
                            observations.lock().unwrap().violations.push("HA fixture task failed");
                        }
                    }
                }
            }
        });
        Self {
            address,
            first_token,
            second_token,
            observed,
            active,
            events,
            task,
        }
    }

    fn base(&self, prefix: &str) -> String {
        format!("{}{prefix}", self.address)
    }

    fn event(&self, entity: &str, state: Option<Value>) {
        assert!(
            self.events
                .send(json!({
                    "event_type":"state_changed","data":{"entity_id":entity,"new_state":state}
                }))
                .is_ok(),
            "an authenticated socket must be present"
        );
    }

    fn count(&self, operation: &str) -> usize {
        self.observed
            .lock()
            .unwrap()
            .requests
            .iter()
            .filter(|request| request.operation == operation)
            .count()
    }

    fn assert_safe(&self) {
        let observed = self.observed.lock().unwrap();
        assert!(observed.violations.is_empty(), "{:?}", observed.violations);
        for request in &observed.requests {
            assert!(
                request.path.starts_with(FIRST_PREFIX) || request.path.starts_with(SECOND_PREFIX)
            );
            if request.operation == "state" {
                assert!([
                    "sensor.temperature",
                    "input_boolean.do_not_supply_charger",
                    "sensor.missing",
                    "sensor.next"
                ]
                .iter()
                .any(|entity| request.path.ends_with(&format!("api/states/{entity}"))));
            }
        }
    }

    async fn no_sockets(&self) {
        until(|| self.active.load(Ordering::Acquire) == 0).await;
        self.assert_safe();
    }
}

impl Drop for HomeAssistant {
    fn drop(&mut self) {
        // Dropping the server's JoinSet cancels every owned socket task too.
        self.task.abort();
    }
}

async fn request_headers(stream: &TcpStream) -> Result<String, ()> {
    timeout(WAIT, async {
        let mut bytes = [0; 8192];
        loop {
            let length = stream.peek(&mut bytes).await.map_err(|_| ())?;
            if length == 0 || length == bytes.len() {
                return Err(());
            }
            if let Some(end) = bytes[..length]
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
            {
                return String::from_utf8(bytes[..end + 4].to_vec()).map_err(|_| ());
            }
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .map_err(|_| ())?
}

async fn serve_connection(
    mut stream: TcpStream,
    credentials: Arc<BTreeMap<String, String>>,
    observed: Arc<Mutex<Observations>>,
    active: Arc<AtomicUsize>,
    mut events: broadcast::Receiver<Value>,
) -> Result<(), ()> {
    let headers = request_headers(&stream).await?;
    let path = headers
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("GET "))
        .and_then(|line| line.strip_suffix(" HTTP/1.1"))
        .ok_or(())?
        .to_owned();
    let (prefix, expected_token) = credentials
        .iter()
        .find(|(prefix, _)| path.starts_with(prefix.as_str()))
        .ok_or(())?;
    let lower = headers.to_ascii_lowercase();
    if path == format!("{prefix}api/websocket") {
        if lower.contains("\r\nauthorization:") || lower.contains("\r\ncookie:") {
            return Err(());
        }
        let mut websocket = tokio_tungstenite::accept_async(stream)
            .await
            .map_err(|_| ())?;
        websocket
            .send(Message::Text(
                json!({"type":"auth_required","ha_version":"2026.9"})
                    .to_string()
                    .into(),
            ))
            .await
            .map_err(|_| ())?;
        let auth = timeout(WAIT, websocket.next())
            .await
            .map_err(|_| ())?
            .ok_or(())?
            .map_err(|_| ())?;
        let auth: Value = serde_json::from_str(auth.to_text().map_err(|_| ())?).map_err(|_| ())?;
        if auth["type"] != "auth"
            || auth["access_token"].as_str() != Some(expected_token.as_str())
            || auth.as_object().is_none_or(|value| value.len() != 2)
        {
            return Err(());
        }
        websocket
            .send(Message::Text(
                json!({"type":"auth_ok","ha_version":"2026.9"})
                    .to_string()
                    .into(),
            ))
            .await
            .map_err(|_| ())?;
        active.fetch_add(1, Ordering::AcqRel);
        let _socket = ActiveSocket(active);
        observed.lock().unwrap().record(&path, "auth");
        let mut subscribed = false;
        loop {
            tokio::select! {
                incoming = websocket.next() => {
                    let Some(incoming) = incoming else { return Ok(()) };
                    let incoming = match incoming { Ok(message) => message, Err(_) => return Ok(()) };
                    match incoming {
                        Message::Text(text) => {
                            let message: Value = serde_json::from_str(&text).map_err(|_| ())?;
                            match message["type"].as_str() {
                                Some("subscribe_events") if !subscribed && message == json!({"id":1,"type":"subscribe_events","event_type":"state_changed"}) => {
                                    subscribed = true;
                                    observed.lock().unwrap().record(&path, "subscribe");
                                    websocket.send(Message::Text(json!({"id":1,"type":"result","success":true,"result":null}).to_string().into())).await.map_err(|_| ())?;
                                }
                                Some("ping") if message["id"].as_u64().is_some_and(|id| id >= 2) => {
                                    websocket.send(Message::Text(json!({"id":message["id"],"type":"pong"}).to_string().into())).await.map_err(|_| ())?;
                                }
                                _ => return Err(()),
                            }
                        }
                        Message::Ping(bytes) => websocket.send(Message::Pong(bytes)).await.map_err(|_| ())?,
                        Message::Pong(_) => {},
                        Message::Close(_) => return Ok(()),
                        _ => return Err(()),
                    }
                }
                event = events.recv() => {
                    let event = event.map_err(|_| ())?;
                    if subscribed {
                        websocket.send(Message::Text(json!({"id":1,"type":"event","event":event}).to_string().into())).await.map_err(|_| ())?;
                    }
                }
            }
        }
    }
    if !path.starts_with(&format!("{prefix}api/states/")) || path.contains('?') {
        return Err(());
    }
    let authorized = headers.lines().any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.eq_ignore_ascii_case("authorization")
                && value.trim() == format!("Bearer {expected_token}")
        })
    });
    if !authorized || lower.contains("\r\ncookie:") {
        return Err(());
    }
    let entity = path
        .strip_prefix(&format!("{prefix}api/states/"))
        .ok_or(())?;
    let (status, value) = match entity {
        "sensor.temperature" => ("200 OK", entity_state(entity, "21.5", "Temperature")),
        "input_boolean.do_not_supply_charger" => {
            ("200 OK", entity_state(entity, "on", "Do not charge EV"))
        }
        "sensor.missing" => ("404 Not Found", json!({"message":"Entity not found"})),
        "sensor.next" => ("200 OK", entity_state(entity, "7", "Next sensor")),
        _ => return Err(()),
    };
    let mut consumed = vec![0; headers.len()];
    stream.read_exact(&mut consumed).await.map_err(|_| ())?;
    observed.lock().unwrap().record(&path, "state");
    let bytes = serde_json::to_vec(&value).unwrap();
    let response = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", bytes.len());
    stream
        .write_all(response.as_bytes())
        .await
        .map_err(|_| ())?;
    stream.write_all(&bytes).await.map_err(|_| ())?;
    stream.shutdown().await.map_err(|_| ())
}

fn entity_state(entity: &str, state: &str, name: &str) -> Value {
    json!({"entity_id":entity,"state":state,"attributes":{
        "friendly_name":name,"unit_of_measurement":"°C","private":PRIVATE_ATTRIBUTE
    }})
}

async fn until(mut predicate: impl FnMut() -> bool) {
    timeout(WAIT, async {
        while !predicate() {
            sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("HA acceptance condition must become true");
}

fn items(host: &PluginHost) -> Vec<Value> {
    host.snapshots()
        .into_iter()
        .filter(|snapshot| snapshot.plugin_id == PLUGIN && snapshot.state == WorkerState::Running)
        .flat_map(|snapshot| {
            snapshot
                .contributions
                .into_iter()
                .map(|item| serde_json::to_value(item).unwrap())
        })
        .collect()
}

async fn connection(host: &PluginHost) {
    until(|| {
        items(host)
            .iter()
            .any(|item| item["id"] == "connection" && item["value"] == "Connected")
    })
    .await;
}

async fn item_value(host: &PluginHost, id: &str, field: &str, value: Value) {
    until(|| {
        items(host)
            .iter()
            .any(|item| item["id"] == id && item[field] == value)
    })
    .await;
}

fn no_secrets(host: &PluginHost, origin: &HomeAssistant) {
    let snapshots = host.snapshots();
    let bytes = serde_json::to_string(&snapshots).unwrap();
    for private in [
        origin.first_token.as_str(),
        origin.second_token.as_str(),
        PRIVATE_ATTRIBUTE,
    ] {
        assert!(
            !bytes.contains(private),
            "private HA data must not enter host snapshots"
        );
    }
    for snapshot in snapshots {
        assert!(snapshot
            .contributions
            .iter()
            .all(|item| !matches!(item, DashboardContribution::Action { .. })));
    }
}

/// Real core MQTT state merging, deliberately without an AppHandle, HA overlay,
/// camera configuration, or native notification/UI integration.
struct CoreTelemetry(crate::mqtt::MqttClient);

impl CoreTelemetry {
    fn new(port: u16) -> Self {
        let mut client = crate::mqtt::MqttClient::new(
            "127.0.0.1".into(),
            port,
            None,
            None,
            format!("ha-core-probe-{}", uuid::Uuid::new_v4()),
        );
        client.connect().unwrap();
        Self(client)
    }

    async fn assert_receives(&self, broker: &Broker, marker: u64) {
        let expected_flag = marker.is_multiple_of(2);
        timeout(WAIT, async {
            loop {
                broker
                    .publish(
                        "inverter/state",
                        json!({"uptime":marker,"booleans":{"do_not_supply_charger":expected_flag}}),
                    )
                    .await;
                let state = self.0.get_state();
                if state.uptime == Some(marker) {
                    assert_eq!(
                        state
                            .booleans
                            .as_ref()
                            .and_then(|flags| flags.get("do_not_supply_charger")),
                        Some(&expected_flag)
                    );
                    break;
                }
            }
        })
        .await
        .expect("actual core MQTT telemetry and inverter-control flags remain independent of HA");
    }
}

impl Drop for CoreTelemetry {
    fn drop(&mut self) {
        self.0.stop();
    }
}

async fn install(root: &Path) -> (PackageApplication, PluginHost, u64) {
    let key = SigningKey::from_bytes(&rand::random::<[u8; 32]>());
    let publisher = "disposable-ha-acceptance";
    let trust = TrustStore::new(vec![PublisherTrust::new(
        publisher.into(),
        key.verifying_key().to_bytes(),
        vec![PLUGIN.into()],
    )
    .unwrap()])
    .unwrap();
    let host = PluginHost::default();
    let service = PackageApplication::new(
        host.clone(),
        env!("INVERTER_DESKTOP_TARGET").into(),
        true,
        Arc::new(|| {}),
    );
    let encryption = rand::random::<[u8; 32]>();
    service
        .initialize_with_key(
            Ok(root.join("store")),
            Ok(trust),
            Arc::new(move || Ok(encryption.to_vec())),
        )
        .await;
    let epoch = service.session_changed(true).unwrap();
    let mut manifest: PluginManifest = serde_json::from_str(include_str!(
        "../../../scripts/plugins/home-assistant-manifest.json"
    ))
    .unwrap();
    assert_eq!(manifest.plugin_id, PLUGIN);
    manifest.target = env!("INVERTER_DESKTOP_TARGET").into();
    manifest.entrypoint = format!(
        "bin/inverter-home-assistant-worker{}",
        std::env::consts::EXE_SUFFIX
    );
    let source = root.join("payload");
    fs::create_dir_all(source.join("bin")).unwrap();
    fs::copy(
        required_file("INVERTER_HOME_ASSISTANT_WORKER"),
        source.join(&manifest.entrypoint),
    )
    .unwrap();
    let archive = root.join("home-assistant.idplugin");
    write_package_atomic(
        &archive,
        &build_package(manifest, &source, publisher, &key).unwrap(),
    )
    .unwrap();
    let ticket = service.begin_selection("config", epoch).unwrap();
    let preview = service
        .finish_selection(&ticket, "config", epoch, archive)
        .await
        .unwrap();
    assert_eq!(preview.plugin_id, PLUGIN);
    service
        .install_review(&preview.token, "config", epoch, false)
        .await
        .unwrap();
    assert!(host.snapshots().is_empty());
    (service, host, epoch)
}

async fn configure(
    service: &PackageApplication,
    epoch: u64,
    origin: &HomeAssistant,
    prefix: &str,
    entities: Option<&str>,
    token: Option<&str>,
) {
    let view = serde_json::to_value(service.get_settings(PLUGIN, epoch).await.unwrap()).unwrap();
    let mut values = BTreeMap::from([("ha_base_url".into(), json!(origin.base(prefix)))]);
    if let Some(entities) = entities {
        values.insert("watch_entities".into(), json!(entities));
    }
    let changes = token
        .map(|token| BTreeMap::from([("ha_token".into(), Some(token.into()))]))
        .unwrap_or_default();
    let saved = service
        .save_settings(
            PLUGIN,
            epoch,
            view["revision"].as_str().unwrap().into(),
            values,
            changes,
        )
        .await
        .unwrap();
    let saved = serde_json::to_value(saved).unwrap();
    assert!(saved["restart_error"].is_null());
    assert_eq!(saved["settings"]["secret_present"]["ha_token"], true);
    let serialized = serde_json::to_string(&saved).unwrap();
    assert!(
        !serialized.contains(&origin.first_token) && !serialized.contains(&origin.second_token)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires explicitly built INVERTER_HOME_ASSISTANT_WORKER and local MOSQUITTO_BIN; CI runs this acceptance test"]
async fn signed_home_assistant_package_lifecycle() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let broker = Broker::new(&root).await;
    let core = CoreTelemetry::new(broker.port);
    let origin = HomeAssistant::new().await;
    let (service, host, epoch) = install(&root).await;
    core.assert_receives(&broker, 1).await;
    assert_eq!(origin.count("auth"), 0);
    let view = serde_json::to_value(service.get_settings(PLUGIN, epoch).await.unwrap()).unwrap();
    assert_eq!(view["values"]["watch_entities"], "");
    assert_eq!(view["secret_present"]["ha_token"], false);
    assert!(view["fields"]
        .as_array()
        .unwrap()
        .iter()
        .any(|field| field["key"] == "ha_token"
            && field["secret"] == true
            && field["required"] == true));

    // Exercise the shipped default: an empty watch list only authenticates.
    configure(
        &service,
        epoch,
        &origin,
        FIRST_PREFIX,
        None,
        Some(&origin.first_token),
    )
    .await;
    service.set_enabled(PLUGIN, true, epoch).await.unwrap();
    connection(&host).await;
    assert_eq!(items(&host).len(), 1);
    assert_eq!(origin.count("subscribe"), 0);
    assert_eq!(origin.count("state"), 0);
    core.assert_receives(&broker, 2).await;

    configure(
        &service,
        epoch,
        &origin,
        FIRST_PREFIX,
        Some("sensor.temperature,\ninput_boolean.do_not_supply_charger,sensor.missing,sensor.temperature"),
        None,
    )
    .await;
    connection(&host).await;
    item_value(&host, "entity-0", "value", json!(21.5)).await;
    item_value(&host, "entity-1", "text", json!("on")).await;
    item_value(&host, "entity-2", "value", json!("Unavailable")).await;
    assert_eq!(items(&host).len(), 4);
    assert_eq!(
        origin.count("state"),
        3,
        "deduplicated selected entities only"
    );
    // A literal HA alias remains on while the daemon's canonical flag is false.
    core.assert_receives(&broker, 3).await;
    assert!(
        items(&host)
            .iter()
            .any(|item| item["id"] == "entity-1" && item["text"] == "on"),
        "core flag telemetry must not overwrite the watched HA entity"
    );
    origin.event(
        "sensor.unselected",
        Some(entity_state("sensor.unselected", "999", "Never displayed")),
    );
    origin.event(
        "sensor.temperature",
        Some(entity_state("sensor.temperature", "22.25", "Temperature")),
    );
    item_value(&host, "entity-0", "value", json!(22.25)).await;
    origin.event("input_boolean.do_not_supply_charger", None);
    item_value(&host, "entity-1", "value", json!("Unavailable")).await;
    assert!(!serde_json::to_string(&items(&host))
        .unwrap()
        .contains("Never displayed"));
    no_secrets(&host, &origin);
    core.assert_receives(&broker, 4).await;

    // Replacing settings rotates the token, preserves the explicit port/prefix,
    // closes the old worker's socket, and replaces the contribution inventory.
    configure(
        &service,
        epoch,
        &origin,
        SECOND_PREFIX,
        Some("sensor.next"),
        Some(&origin.second_token),
    )
    .await;
    connection(&host).await;
    item_value(&host, "entity-0", "value", json!(7.0)).await;
    until(|| origin.active.load(Ordering::Acquire) == 1).await;
    assert_eq!(items(&host).len(), 2);
    origin.event(
        "sensor.temperature",
        Some(entity_state(
            "sensor.temperature",
            "999",
            "Old configuration",
        )),
    );
    origin.event(
        "sensor.next",
        Some(entity_state("sensor.next", "8", "Next sensor")),
    );
    item_value(&host, "entity-0", "value", json!(8.0)).await;
    assert!(!serde_json::to_string(&items(&host))
        .unwrap()
        .contains("Old configuration"));
    no_secrets(&host, &origin);

    service.set_enabled(PLUGIN, false, epoch).await.unwrap();
    origin.no_sockets().await;
    assert!(items(&host).is_empty());
    core.assert_receives(&broker, 5).await;
    service.set_enabled(PLUGIN, true, epoch).await.unwrap();
    connection(&host).await;
    item_value(&host, "entity-0", "value", json!(7.0)).await;
    assert!(service.session_changed(false).is_none());
    assert!(items(&host).is_empty());
    origin.no_sockets().await;
    core.assert_receives(&broker, 6).await;
    let next_epoch = service.session_changed(true).unwrap();
    service.restore(next_epoch).await.unwrap();
    connection(&host).await;
    no_secrets(&host, &origin);
    service
        .uninstall_with_settings(PLUGIN, true, next_epoch)
        .await
        .unwrap();
    origin.no_sockets().await;
    assert!(host.snapshots().is_empty());
    assert!(service
        .snapshot(next_epoch)
        .await
        .unwrap()
        .plugins
        .is_empty());
    assert!(fs::read_dir(root.join("store/settings"))
        .unwrap()
        .next()
        .is_none());
    core.assert_receives(&broker, 7).await;
    origin.assert_safe();
    service.close().await.unwrap();
}
