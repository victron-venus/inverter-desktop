use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::Emitter;
// Import config loading functions from lib
use crate::load_config;
mod lifecycle;
pub use lifecycle::{
    config_changes, connection_status, notify_config_changed, set_connection_status,
};

pub static WINDOW_HIDDEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Interactive entities refresh quickly; the larger sensor inventory has a
/// separate bounded cadence so a busy HA installation does not saturate WebKit.
const MIN_HA_FILTERED_EMIT_INTERVAL: Duration = Duration::from_millis(500);
const MIN_HA_SENSOR_EMIT_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Default)]
struct HaFilteredEmitCoalesce {
    last_emit: Option<Instant>,
    flush_scheduled: bool,
    revision: u64,
    ticket: u64,
}
impl HaFilteredEmitCoalesce {
    fn reset(&mut self, now: Instant, revision: u64) {
        self.last_emit = Some(now);
        self.flush_scheduled = false;
        self.revision = revision;
        self.ticket = self.ticket.wrapping_add(1);
    }
}
static HA_FILTERED_COALESCE: std::sync::LazyLock<Mutex<HaFilteredEmitCoalesce>> =
    std::sync::LazyLock::new(|| Mutex::new(HaFilteredEmitCoalesce::default()));
static HA_SENSOR_COALESCE: std::sync::LazyLock<Mutex<HaFilteredEmitCoalesce>> =
    std::sync::LazyLock::new(|| Mutex::new(HaFilteredEmitCoalesce::default()));

fn sensor_domain(entity_id: &str) -> bool {
    matches!(
        entity_id.split('.').next().unwrap_or(""),
        "sensor" | "binary_sensor"
    )
}
fn domain_triggers_live_filtered(entity_id: &str) -> bool {
    matches!(
        entity_id.split('.').next().unwrap_or(""),
        "sensor" | "binary_sensor" | "number" | "cover" | "media_player" | "scene" | "weather"
    )
}

fn emit_ha_filtered_now(
    app: &tauri::AppHandle,
    entity_states: &Arc<Mutex<HashMap<String, HaEntityEntry>>>,
    include_sensors: bool,
    revision: u64,
) {
    if WINDOW_HIDDEN.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    // Hold the entity-map lock through emit so supervisor clear + its empty
    // snapshot always follow any in-flight delivery from the old connection.
    let Ok(guard) = entity_states.lock() else {
        return;
    };
    if revision != lifecycle::current_revision() {
        return;
    }
    let mut filtered = compute_filtered_data(&guard);
    if !include_sensors {
        filtered.sensors.clear();
    }
    filtered.refresh_sensors = include_sensors;
    let _ = app.emit("ha-filtered-update", &filtered);
}

/// Force a full snapshot on connect, clear or visibility restoration.
pub fn force_emit_ha_filtered(
    app: &tauri::AppHandle,
    states: &Arc<Mutex<HashMap<String, HaEntityEntry>>>,
) {
    emit_ha_filtered_coalesced(app, states, true);
}

fn emit_ha_filtered_coalesced(
    app: &tauri::AppHandle,
    states: &Arc<Mutex<HashMap<String, HaEntityEntry>>>,
    force: bool,
) {
    let revision = lifecycle::current_revision();
    if force {
        for coalesce in [&*HA_FILTERED_COALESCE, &*HA_SENSOR_COALESCE] {
            if let Ok(mut pending) = coalesce.lock() {
                pending.reset(Instant::now(), revision);
            }
        }
        emit_ha_filtered_now(app, states, true, revision);
    } else {
        schedule_filtered_emit(app, states, false, revision);
    }
}

fn schedule_filtered_emit(
    app: &tauri::AppHandle,
    states: &Arc<Mutex<HashMap<String, HaEntityEntry>>>,
    sensors: bool,
    revision: u64,
) {
    if WINDOW_HIDDEN.load(std::sync::atomic::Ordering::Relaxed)
        || revision != lifecycle::current_revision()
    {
        return;
    }
    let (coalesce, interval): (&'static Mutex<HaFilteredEmitCoalesce>, Duration) = if sensors {
        (&HA_SENSOR_COALESCE, MIN_HA_SENSOR_EMIT_INTERVAL)
    } else {
        (&HA_FILTERED_COALESCE, MIN_HA_FILTERED_EMIT_INTERVAL)
    };
    let mut delay = None;
    let emit_now = {
        let mut pending = coalesce.lock().unwrap_or_else(|error| error.into_inner());
        let now = Instant::now();
        let elapsed = if pending.revision == revision {
            pending.last_emit.map(|last| now.duration_since(last))
        } else {
            None
        };
        if elapsed.is_some_and(|elapsed| elapsed < interval) {
            if !pending.flush_scheduled {
                pending.flush_scheduled = true;
                pending.ticket = pending.ticket.wrapping_add(1);
                delay = Some((
                    interval.saturating_sub(elapsed.unwrap_or_default()),
                    pending.ticket,
                ));
            }
            false
        } else {
            pending.reset(now, revision);
            true
        }
    };
    if emit_now {
        emit_ha_filtered_now(app, states, sensors, revision);
    }
    if let Some((delay, ticket)) = delay {
        let app = app.clone();
        let states = states.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(delay).await;
            if revision != lifecycle::current_revision() {
                return;
            }
            let should_emit = {
                let mut pending = coalesce.lock().unwrap_or_else(|error| error.into_inner());
                if pending.ticket != ticket || pending.revision != revision {
                    false
                } else {
                    pending.reset(Instant::now(), revision);
                    true
                }
            };
            if should_emit {
                emit_ha_filtered_now(&app, &states, sensors, revision);
            }
        });
    }
}

fn attr_str<'a>(attrs: Option<&'a serde_json::Value>, key: &str) -> Option<&'a str> {
    attrs.and_then(|a| a.get(key)).and_then(|v| v.as_str())
}

fn attr_f64(attrs: Option<&serde_json::Value>, key: &str) -> Option<f64> {
    attrs.and_then(|a| a.get(key)).and_then(|v| v.as_f64())
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HaState {
    pub entity_id: String,
    pub state: String,
    pub attributes: Option<serde_json::Value>,
    #[allow(dead_code)]
    pub last_changed: Option<String>,
    #[allow(dead_code)]
    pub last_updated: Option<String>,
}

// === Filtered HA entity display types (computed in Rust for CPU efficiency) ===

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HaSensorDisplay {
    pub entity_id: String,
    pub name: String,
    pub state: String,
    pub unit: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HaNumberDisplay {
    pub entity_id: String,
    pub name: String,
    pub value: f64,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub unit: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HaCoverDisplay {
    pub entity_id: String,
    pub name: String,
    pub position: i64,
    /// HA state: open / closed / opening / closing / unavailable / unknown
    #[serde(default)]
    pub state: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HaMediaPlayerDisplay {
    pub entity_id: String,
    pub name: String,
    pub state: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HaSceneDisplay {
    pub entity_id: String,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HaWeatherDisplay {
    pub entity_id: String,
    pub name: String,
    pub state: String,
    pub temperature: Option<f64>,
    pub unit: String,
    pub forecast: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HaFilteredData {
    pub sensors: Vec<HaSensorDisplay>,
    pub numbers: Vec<HaNumberDisplay>,
    pub covers: Vec<HaCoverDisplay>,
    pub media_players: Vec<HaMediaPlayerDisplay>,
    pub scenes: Vec<HaSceneDisplay>,
    pub weather: Option<HaWeatherDisplay>,
    /// Full snapshots, including coalesced live updates, replace `haSensors`.
    #[serde(default)]
    pub refresh_sensors: bool,
}

#[derive(Clone)]
pub struct HaEntityEntry {
    pub state: String,
    pub attributes: Option<serde_json::Value>,
}

pub fn compute_filtered_data(entity_states: &HashMap<String, HaEntityEntry>) -> HaFilteredData {
    let mut sensors = Vec::new();
    let mut numbers = Vec::new();
    let mut covers = Vec::new();
    let mut media_players = Vec::new();
    let mut scenes = Vec::new();
    let mut weather = None;

    for (entity_id, entry) in entity_states {
        let domain = entity_id.split('.').next().unwrap_or("");
        let is_unavailable = entry.state == "unavailable" || entry.state == "unknown";
        // Keep unavailable covers visible so the UI can style them; skip other domains.
        if is_unavailable && domain != "cover" {
            continue;
        }
        let attrs = entry.attributes.as_ref();
        let name = attr_str(attrs, "friendly_name").unwrap_or(entity_id);

        match domain {
            "sensor" | "binary_sensor" => {
                let unit = attr_str(attrs, "unit_of_measurement").unwrap_or("");
                sensors.push(HaSensorDisplay {
                    entity_id: entity_id.clone(),
                    name: name.to_string(),
                    state: entry.state.clone(),
                    unit: unit.to_string(),
                });
            }
            "number" => {
                let value = entry.state.parse::<f64>().unwrap_or(0.0);
                let min = attr_f64(attrs, "min").unwrap_or(0.0);
                let max = attr_f64(attrs, "max").unwrap_or(100.0);
                let step = attr_f64(attrs, "step").unwrap_or(1.0);
                let unit = attr_str(attrs, "unit_of_measurement").unwrap_or("");
                numbers.push(HaNumberDisplay {
                    entity_id: entity_id.clone(),
                    name: name.to_string(),
                    value,
                    min,
                    max,
                    step,
                    unit: unit.to_string(),
                });
            }
            "cover" => {
                let position = if is_unavailable {
                    0
                } else {
                    attrs
                        .and_then(|a| a.get("current_position"))
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0)
                };
                covers.push(HaCoverDisplay {
                    entity_id: entity_id.clone(),
                    name: name.to_string(),
                    position,
                    state: entry.state.clone(),
                });
            }
            "media_player" => {
                media_players.push(HaMediaPlayerDisplay {
                    entity_id: entity_id.clone(),
                    name: name.to_string(),
                    state: entry.state.clone(),
                });
            }
            "scene" => {
                let scene_name = attr_str(attrs, "friendly_name")
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| {
                        entity_id
                            .strip_prefix("scene.")
                            .unwrap_or(entity_id)
                            .replace('_', " ")
                    });
                scenes.push(HaSceneDisplay {
                    entity_id: entity_id.clone(),
                    name: scene_name,
                });
            }
            "weather" if weather.is_none() => {
                let weather_name = attr_str(attrs, "friendly_name").unwrap_or("Weather");
                let temperature = attr_f64(attrs, "temperature");
                let unit = attr_str(attrs, "temperature_unit")
                    .unwrap_or("°C")
                    .to_string();
                let forecast = attrs
                    .and_then(|a| a.get("forecast"))
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                weather = Some(HaWeatherDisplay {
                    entity_id: entity_id.clone(),
                    name: weather_name.to_string(),
                    state: entry.state.clone(),
                    temperature,
                    unit,
                    forecast,
                });
            }
            _ => {}
        }
    }

    HaFilteredData {
        sensors,
        numbers,
        covers,
        media_players,
        scenes,
        weather,
        refresh_sensors: true,
    }
}

/// Consecutive HTTP 404/410 strikes required before blacklisting an entity.
/// A single miss can be a fluke/timeout race; require sustained absence.
const HA_ENTITY_SKIP_STRIKES: u32 = 3;

/// Entity IDs blacklisted after [`HA_ENTITY_SKIP_STRIKES`] consecutive 404/410
/// responses from `/api/states/{id}`.
/// Process-wide because `HaApiClient` is constructed per REST invoke; cleared on
/// config save/restore and HA WebSocket reconnect so fixing an entity id in
/// config takes effect without a full app restart.
static HA_ENTITY_SKIP: std::sync::LazyLock<Mutex<HashSet<String>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashSet::new()));

/// Per-entity consecutive 404/410 failure counts (process-wide, like HA_ENTITY_SKIP).
static HA_ENTITY_FAIL_COUNT: std::sync::LazyLock<Mutex<HashMap<String, u32>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

fn is_entity_skipped(entity_id: &str) -> bool {
    HA_ENTITY_SKIP
        .lock()
        .map(|set| set.contains(entity_id))
        .unwrap_or(false)
}

/// Record a successful fetch for `entity_id` — resets its consecutive-miss counter.
fn record_entity_success(entity_id: &str) {
    if let Ok(mut counts) = HA_ENTITY_FAIL_COUNT.lock() {
        counts.remove(entity_id);
    }
}

/// Record an HTTP 404/410 for `entity_id`. Blacklists only after
/// [`HA_ENTITY_SKIP_STRIKES`] consecutive misses; any success resets the counter.
fn record_entity_missing(entity_id: &str) {
    if is_entity_skipped(entity_id) {
        return;
    }
    let strikes = {
        let Ok(mut counts) = HA_ENTITY_FAIL_COUNT.lock() else {
            return;
        };
        let entry = counts.entry(entity_id.to_string()).or_insert(0);
        *entry = entry.saturating_add(1);
        *entry
    };
    if strikes < HA_ENTITY_SKIP_STRIKES {
        log::debug!(
            "HA entity {} returned 404/410 (strike {}/{}); will retry",
            entity_id,
            strikes,
            HA_ENTITY_SKIP_STRIKES
        );
        return;
    }
    let newly_added = HA_ENTITY_SKIP
        .lock()
        .map(|mut set| set.insert(entity_id.to_string()))
        .unwrap_or(false);
    if newly_added {
        log::info!(
            "HA entity {} not found after {} consecutive 404/410 — disabled until config reload",
            entity_id,
            strikes
        );
    }
}

/// Clear the missing-entity killswitch and failure counters (config reload / HA reconnect).
pub fn clear_entity_skip_list() {
    if let Ok(mut set) = HA_ENTITY_SKIP.lock() {
        set.clear();
    }
    if let Ok(mut counts) = HA_ENTITY_FAIL_COUNT.lock() {
        counts.clear();
    }
}

#[derive(Clone)]
pub struct HaApiClient {
    base_url: String,
    token: String,
    client: Client,
}

impl HaApiClient {
    pub async fn new(url: &str, port: Option<u16>, token: &str) -> Result<Self, String> {
        let host = url.trim_end_matches('/').trim();
        let port = port.unwrap_or(8123);

        let (prefix, rest) = if let Some(stripped) = host.strip_prefix("https://") {
            ("https://", stripped)
        } else if let Some(stripped) = host.strip_prefix("http://") {
            ("http://", stripped)
        } else {
            ("http://", host)
        };
        let host_part = rest.split('/').next().unwrap_or(rest);
        let authority = if host_part.starts_with('[') {
            let bracket_end = host_part.find(']');
            let has_port = bracket_end.is_some_and(|i| {
                host_part.len() > i + 1 && host_part.as_bytes().get(i + 1) == Some(&b':')
            });
            if has_port {
                host_part.to_string()
            } else {
                format!("{}:{}", host_part, port)
            }
        } else if host_part.contains(':') {
            host_part.to_string()
        } else {
            format!("{}:{}", host_part, port)
        };

        let base_url = format!("{}{}", prefix, authority);

        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

        Ok(Self {
            base_url,
            token: token.to_string(),
            client,
        })
    }

    pub async fn test_connection(&self) -> Result<(), String> {
        let response = self
            .client
            .get(format!("{}/api/", self.base_url))
            .header("Authorization", format!("Bearer {}", self.token))
            .send()
            .await
            .map_err(|e| format!("Connection failed: {}", e))?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(format!(
                "HTTP {} - unauthorized or invalid",
                response.status()
            ))
        }
    }

    pub async fn get_states(&self) -> Result<Vec<HaState>, String> {
        let response = self
            .client
            .get(format!("{}/api/states", self.base_url))
            .header("Authorization", format!("Bearer {}", self.token))
            .send()
            .await
            .map_err(|e| format!("Failed to fetch states: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("HTTP {} error", response.status()));
        }

        let states: Vec<serde_json::Value> = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        let mut result = Vec::new();
        for item in states {
            if let (Some(entity_id), Some(state)) = (item.get("entity_id"), item.get("state")) {
                if let (Some(eid_str), Some(state_str)) = (entity_id.as_str(), state.as_str()) {
                    let attributes = item.get("attributes").cloned();
                    let last_changed = item
                        .get("last_changed")
                        .and_then(|v| v.as_str().map(String::from));
                    let last_updated = item
                        .get("last_updated")
                        .and_then(|v| v.as_str().map(String::from));
                    result.push(HaState {
                        entity_id: eid_str.to_string(),
                        state: state_str.to_string(),
                        attributes,
                        last_changed,
                        last_updated,
                    });
                }
            }
        }

        Ok(result)
    }

    pub async fn get_entities(&self, entity_ids: &[&str]) -> Result<Vec<HaState>, String> {
        let mut result = Vec::new();
        for &eid in entity_ids {
            if is_entity_skipped(eid) {
                continue;
            }
            let response = self
                .client
                .get(format!("{}/api/states/{}", self.base_url, eid))
                .header("Authorization", format!("Bearer {}", self.token))
                .send()
                .await
                .map_err(|e| format!("Failed to fetch entity {}: {}", eid, e))?;
            let status = response.status();
            if status == reqwest::StatusCode::UNAUTHORIZED {
                return Err("HA authentication failed (401)".to_string());
            }
            // Gone / missing entities: require HA_ENTITY_SKIP_STRIKES consecutive
            // 404/410 before blacklisting (cleared on config reload / HA reconnect).
            if status == reqwest::StatusCode::NOT_FOUND || status == reqwest::StatusCode::GONE {
                record_entity_missing(eid);
                continue;
            }
            if !status.is_success() {
                log::warn!("HA entity {} returned HTTP {}, skipping", eid, status);
                continue;
            }
            // HTTP success — entity exists; reset consecutive-miss counter.
            record_entity_success(eid);
            if let Ok(item) = response.json::<serde_json::Value>().await {
                if let (Some(entity_id), Some(state)) = (item.get("entity_id"), item.get("state")) {
                    if let (Some(eid_str), Some(state_str)) = (entity_id.as_str(), state.as_str()) {
                        result.push(HaState {
                            entity_id: eid_str.to_string(),
                            state: state_str.to_string(),
                            attributes: item.get("attributes").cloned(),
                            last_changed: item
                                .get("last_changed")
                                .and_then(|v| v.as_str().map(String::from)),
                            last_updated: item
                                .get("last_updated")
                                .and_then(|v| v.as_str().map(String::from)),
                        });
                    }
                }
            }
        }
        Ok(result)
    }

    pub async fn call_service(
        &self,
        entity_id: &str,
        domain: &str,
        service: &str,
        mut data: serde_json::Value,
    ) -> Result<(), String> {
        let url = format!("{}/api/services/{}/{}", self.base_url, domain, service);
        // Merge entity_id into data object
        let payload = if let Some(obj) = data.as_object_mut() {
            obj.insert(
                "entity_id".to_string(),
                serde_json::Value::String(entity_id.to_string()),
            );
            data
        } else {
            serde_json::json!({ "entity_id": entity_id })
        };
        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("Service call failed: {}", e))?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(format!("HTTP {} error", response.status()))
        }
    }

    pub async fn turn_on(&self, entity_id: &str) -> Result<(), String> {
        let domain = entity_id.split('.').next().unwrap_or("switch");
        self.call_service(entity_id, domain, "turn_on", serde_json::json!({}))
            .await
    }

    pub async fn turn_off(&self, entity_id: &str) -> Result<(), String> {
        let domain = entity_id.split('.').next().unwrap_or("switch");
        self.call_service(entity_id, domain, "turn_off", serde_json::json!({}))
            .await
    }

    pub async fn set_cover_position(&self, entity_id: &str, position: u8) -> Result<(), String> {
        self.call_service(
            entity_id,
            "cover",
            "set_cover_position",
            serde_json::json!({ "position": position.clamp(0, 100) }),
        )
        .await
    }

    pub async fn media_player_play(&self, entity_id: &str) -> Result<(), String> {
        self.call_service(
            entity_id,
            "media_player",
            "media_play",
            serde_json::json!({}),
        )
        .await
    }

    pub async fn media_player_pause(&self, entity_id: &str) -> Result<(), String> {
        self.call_service(
            entity_id,
            "media_player",
            "media_pause",
            serde_json::json!({}),
        )
        .await
    }

    pub async fn media_player_stop(&self, entity_id: &str) -> Result<(), String> {
        self.call_service(
            entity_id,
            "media_player",
            "media_stop",
            serde_json::json!({}),
        )
        .await
    }

    pub async fn scene_activate(&self, entity_id: &str) -> Result<(), String> {
        self.call_service(entity_id, "scene", "turn_on", serde_json::json!({}))
            .await
    }
}

/// Apply an HA state_changed payload, including HA's null new_state deletion.
fn apply_state_change(
    entity_states: &Arc<Mutex<HashMap<String, HaEntityEntry>>>,
    data: &serde_json::Value,
    revision: Option<u64>,
) -> Option<(String, Option<HaEntityEntry>)> {
    let new_state = data.get("new_state")?;
    let eid = data
        .get("entity_id")
        .and_then(|v| v.as_str())
        .or_else(|| new_state.get("entity_id").and_then(|v| v.as_str()))?
        .to_string();
    let entry = if new_state.is_null() {
        None
    } else {
        Some(HaEntityEntry {
            state: new_state.get("state")?.as_str()?.to_string(),
            attributes: new_state.get("attributes").cloned(),
        })
    };
    let mut states = entity_states.lock().ok()?;
    if revision.is_some_and(|revision| revision != lifecycle::current_revision()) {
        return None;
    }
    if let Some(entry) = &entry {
        states.insert(eid.clone(), entry.clone());
    } else {
        states.remove(&eid);
    }
    Some((eid, entry))
}

/// HA WebSocket client — subscribes to all state_changed events and emits to frontend.
pub struct HaWebSocketClient {
    task: tokio::task::JoinHandle<()>,
}

impl Drop for HaWebSocketClient {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl HaWebSocketClient {
    pub async fn connect(
        url: &str,
        token: &str,
        app: tauri::AppHandle,
        entity_states: std::sync::Arc<
            std::sync::Mutex<std::collections::HashMap<String, HaEntityEntry>>,
        >,
    ) -> Result<Self, String> {
        let mut revisions = config_changes();
        let connection_revision = *revisions.borrow_and_update();
        let (ws_stream, _) = tokio_tungstenite::connect_async(url)
            .await
            .map_err(|e| format!("WS connect failed: {}", e))?;

        let (mut write, mut read) = ws_stream.split();

        // HA WS protocol: server sends auth_required first
        let required = read
            .next()
            .await
            .ok_or("WS closed before auth_required")?
            .map_err(|e| format!("WS read failed: {}", e))?;
        let required_text = required
            .to_text()
            .map_err(|e| format!("WS not text: {}", e))?;
        let required_val: serde_json::Value =
            serde_json::from_str(required_text).map_err(|e| format!("WS parse failed: {}", e))?;
        if required_val.get("type").and_then(|v| v.as_str()) != Some("auth_required") {
            return Err(format!("Expected auth_required, got: {}", required_text));
        }

        // Now send auth
        let auth_msg = serde_json::json!({ "type": "auth", "access_token": token });
        write
            .send(tokio_tungstenite::tungstenite::Message::text(
                auth_msg.to_string(),
            ))
            .await
            .map_err(|e| format!("WS auth send failed: {}", e))?;

        // Wait for auth_ok
        let resp = read
            .next()
            .await
            .ok_or("WS closed before auth response")?
            .map_err(|e| format!("WS auth read failed: {}", e))?;
        let text = resp
            .to_text()
            .map_err(|e| format!("WS auth not text: {}", e))?;
        let parsed: serde_json::Value =
            serde_json::from_str(text).map_err(|e| format!("WS auth parse failed: {}", e))?;
        if parsed.get("type").and_then(|v| v.as_str()) != Some("auth_ok") {
            return Err(format!("HA WS auth failed: {}", text));
        }

        // Subscribe to all state_changed events
        let sub_id: u64 = 1;
        let sub_msg = serde_json::json!({
            "id": sub_id,
            "type": "subscribe_events",
            "event_type": "state_changed"
        });
        write
            .send(tokio_tungstenite::tungstenite::Message::text(
                sub_msg.to_string(),
            ))
            .await
            .map_err(|e| format!("WS subscribe failed: {}", e))?;

        // Wait for subscription confirmation
        let sub_resp = read
            .next()
            .await
            .ok_or("WS closed before subscription response")?
            .map_err(|e| format!("WS sub read failed: {}", e))?;
        let sub_text = sub_resp
            .to_text()
            .map_err(|e| format!("WS sub not text: {}", e))?;
        let sub_val: serde_json::Value =
            serde_json::from_str(sub_text).map_err(|e| format!("WS sub parse failed: {}", e))?;
        if sub_val.get("id") != Some(&serde_json::json!(sub_id))
            || sub_val.get("type").and_then(|v| v.as_str()) != Some("result")
            || sub_val.get("success") != Some(&serde_json::json!(true))
        {
            return Err(format!("HA WS subscription failed: {}", sub_text));
        }
        // Clear stale entity data from a previous connection/config before repopulating,
        // so entities removed or renamed in HA don't linger in the shared map forever.
        if let Ok(mut states_guard) = entity_states.lock() {
            if connection_revision != lifecycle::current_revision() {
                return Err("HA configuration changed during connection".into());
            }
            states_guard.clear();
        }
        // === Fetch initial state to prevent empty entity map on first events ===
        // WS URL: ws://host:port/api/websocket -> HTTP URL: http://host:port
        let http_base = url
            .replace("ws://", "http://")
            .replace("wss://", "https://")
            .replace("/api/websocket", "");

        let http_client = reqwest::Client::new();
        if let Ok(response) = http_client
            .get(format!("{}/api/states", http_base))
            .header("Authorization", format!("Bearer {}", token))
            .timeout(Duration::from_secs(10))
            .send()
            .await
        {
            if response.status().is_success() {
                if let Ok(states) = response.json::<Vec<serde_json::Value>>().await {
                    if let Ok(mut states_guard) = entity_states.lock() {
                        if connection_revision != lifecycle::current_revision() {
                            return Err("HA configuration changed during initial snapshot".into());
                        }
                        for state in states {
                            if let (Some(eid), Some(state_val)) =
                                (state.get("entity_id"), state.get("state"))
                            {
                                if let (Some(eid_str), Some(state_str)) =
                                    (eid.as_str(), state_val.as_str())
                                {
                                    let attrs = state.get("attributes").cloned();
                                    states_guard.insert(
                                        eid_str.to_string(),
                                        HaEntityEntry {
                                            state: state_str.to_string(),
                                            attributes: attrs,
                                        },
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        // Compute whitelist of entity IDs that the dashboard actually uses
        let whitelist = {
            // Load persisted config to determine which entities are needed
            if let Ok(config) = load_config(&app) {
                let mut set = HashSet::new();

                // Helper to insert an entity ID if it's not empty
                let mut insert_if_not_empty = |id: &Option<String>| {
                    if let Some(ref id_str) = id {
                        if !id_str.is_empty() {
                            set.insert(id_str.clone());
                        }
                    }
                };

                // Individual entities for appliances, EV, etc.
                insert_if_not_empty(&config.ha_dryer_entity);
                insert_if_not_empty(&config.ha_washer_entity);
                insert_if_not_empty(&config.ha_washer_start_entity);
                insert_if_not_empty(&config.ha_washer_pause_entity);
                insert_if_not_empty(&config.ha_dryer_start_entity);
                insert_if_not_empty(&config.ha_dryer_pause_entity);
                insert_if_not_empty(&config.ha_dishwasher_running_entity);
                insert_if_not_empty(&config.ha_dishwasher_duration_entity);
                insert_if_not_empty(&config.ha_ev_soc_entity);
                insert_if_not_empty(&config.ha_ev_charging_entity);
                insert_if_not_empty(&config.ha_ev_clamp_entity);

                // Consumption and generation clamps
                if let Some(ref clamps) = config.ha_consumption_clamps {
                    for id in clamps {
                        insert_if_not_empty(&Some(id.clone()));
                    }
                }
                if let Some(ref clamps) = config.ha_generation_clamps {
                    for id in clamps {
                        insert_if_not_empty(&Some(id.clone()));
                    }
                }

                // Home buttons (ha_entities) — skip inverter-control flags (MQTT-only).
                if let Some(ref entities) = config.ha_entities {
                    for entity in entities {
                        if crate::is_inverter_control_flag(&entity.entity) {
                            continue;
                        }
                        insert_if_not_empty(&Some(entity.entity.clone()));
                    }
                }

                // Header toggles — skip inverter-control flags (MQTT-only).
                if let Some(ref toggles) = config.header_toggles_config {
                    for toggle in toggles {
                        if crate::is_inverter_control_flag(&toggle.entity) {
                            continue;
                        }
                        insert_if_not_empty(&Some(toggle.entity.clone()));
                    }
                }

                set
            } else {
                // If config loading fails, fallback to empty whitelist (no updates)
                // This is safe but disables functionality; better to log and use empty set
                log::warn!("Failed to load config for HA whitelist, disabling entity filtering");
                HashSet::new()
            }
        };

        // Emit initial filtered data immediately after populating state map
        // This ensures frontend has full data on WS connect
        emit_ha_filtered_coalesced(&app, &entity_states, true);

        // Spawn read loop with timeout
        let app_clone = app.clone();
        let whitelist_clone = whitelist.clone();
        let task = tokio::spawn(async move {
            const READ_TIMEOUT_SECS: u64 = 60;
            loop {
                tokio::select! {
                    biased;
                    _ = revisions.changed() => break,
                    msg = read.next() => {
                        match msg {
                            Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                                    if val.get("type").and_then(|v| v.as_str()) == Some("event") {
                                        if let Some(data) = val.get("event").and_then(|event| event.get("data")) {
                                            if let Some((eid, entry)) = apply_state_change(&entity_states, data, Some(connection_revision)) {
                                                if !WINDOW_HIDDEN.load(std::sync::atomic::Ordering::Relaxed) {
                                                    if whitelist_clone.contains(&eid) {
                                                        let _states_guard = entity_states.lock();
                                                        if connection_revision != lifecycle::current_revision() { continue; }
                                                        let _ = app_clone.emit("ha-state-update", serde_json::json!({
                                                            "entity_id": eid,
                                                            "state": entry.as_ref().map(|entry| entry.state.as_str()).unwrap_or("unavailable"),
                                                            "attributes": entry.as_ref().and_then(|entry| entry.attributes.as_ref()),
                                                        }));
                                                    }
                                                    if domain_triggers_live_filtered(&eid) {
                                                        schedule_filtered_emit(&app_clone, &entity_states, sensor_domain(&eid), connection_revision);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            Some(Ok(tokio_tungstenite::tungstenite::Message::Ping(payload))) => {
                                if write.send(tokio_tungstenite::tungstenite::Message::Pong(payload)).await.is_err() {
                                    break;
                                }
                            }
                            Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) => break,
                            Some(Err(e)) => {
                                log::error!("HA WS read error: {}", e);
                                break;
                            }
                            None => break,
                            _ => {}
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_secs(READ_TIMEOUT_SECS)) => {
                        log::warn!(
                            "HA WS read timeout ({}s), reconnecting...",
                            READ_TIMEOUT_SECS
                        );
                        break;
                    }
                }
            }
        });

        Ok(Self { task })
    }

    /// Wait for the read loop to finish (blocks until connection drops or shutdown signal).
    pub async fn run(&mut self) {
        let _ = (&mut self.task).await;
    }
}

#[cfg(test)]
mod entity_skip_tests {
    use super::*;
    use std::sync::Mutex;

    // Process-global skip set + fail counters — serialize these tests.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn strike_until_skip(eid: &str) {
        for _ in 0..HA_ENTITY_SKIP_STRIKES {
            record_entity_missing(eid);
        }
    }

    #[test]
    fn requires_three_consecutive_misses_before_skip() {
        let _guard = TEST_LOCK.lock().unwrap();
        clear_entity_skip_list();
        let eid = "button.washer_start";
        assert!(!is_entity_skipped(eid));

        record_entity_missing(eid);
        assert!(!is_entity_skipped(eid), "1 miss must not blacklist");
        record_entity_missing(eid);
        assert!(!is_entity_skipped(eid), "2 misses must not blacklist");
        record_entity_missing(eid);
        assert!(
            is_entity_skipped(eid),
            "3 consecutive misses must blacklist"
        );

        // Further misses are a no-op (still skipped)
        record_entity_missing(eid);
        assert!(is_entity_skipped(eid));

        clear_entity_skip_list();
        assert!(!is_entity_skipped(eid));
    }

    #[test]
    fn success_resets_consecutive_miss_counter() {
        let _guard = TEST_LOCK.lock().unwrap();
        clear_entity_skip_list();
        let eid = "sensor.flaky";

        record_entity_missing(eid);
        record_entity_missing(eid);
        assert!(!is_entity_skipped(eid));

        record_entity_success(eid);

        // After reset, need a fresh 3 strikes
        record_entity_missing(eid);
        record_entity_missing(eid);
        assert!(!is_entity_skipped(eid));
        record_entity_missing(eid);
        assert!(is_entity_skipped(eid));

        clear_entity_skip_list();
    }

    #[test]
    fn clear_resets_skip_list_and_counters() {
        let _guard = TEST_LOCK.lock().unwrap();
        clear_entity_skip_list();

        strike_until_skip("sensor.dead_one");
        strike_until_skip("button.dead_two");
        assert!(is_entity_skipped("sensor.dead_one"));
        assert!(is_entity_skipped("button.dead_two"));

        // Partial strikes on a third entity, then clear everything
        record_entity_missing("sensor.partial");
        record_entity_missing("sensor.partial");
        clear_entity_skip_list();

        assert!(!is_entity_skipped("sensor.dead_one"));
        assert!(!is_entity_skipped("button.dead_two"));
        assert!(!is_entity_skipped("sensor.partial"));

        // Cleared counters: two more misses still not enough to skip
        record_entity_missing("sensor.partial");
        record_entity_missing("sensor.partial");
        assert!(!is_entity_skipped("sensor.partial"));

        clear_entity_skip_list();
    }
}

#[cfg(test)]
mod subscription_regression_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sensor_events_refresh_values_and_null_state_removes_deleted_entities() {
        let states = Arc::new(Mutex::new(HashMap::new()));
        for value in ["100", "250"] {
            let (eid, _) = apply_state_change(
                &states,
                &json!({
                    "entity_id": "sensor.power",
                    "new_state": {"entity_id": "sensor.power", "state": value,
                        "attributes": {"unit_of_measurement": "W"}}
                }),
                None,
            )
            .unwrap();
            assert!(domain_triggers_live_filtered(&eid));
            let snapshot = compute_filtered_data(&states.lock().unwrap());
            assert!(snapshot.refresh_sensors);
            assert_eq!(snapshot.sensors[0].state, value);
        }
        assert!(domain_triggers_live_filtered("binary_sensor.door"));
        apply_state_change(
            &states,
            &json!({"entity_id": "sensor.power", "new_state": null}),
            None,
        )
        .unwrap();
        assert!(compute_filtered_data(&states.lock().unwrap())
            .sensors
            .is_empty());
    }

    #[test]
    fn old_subscription_cannot_repopulate_map_after_config_revision() {
        let states = Arc::new(Mutex::new(HashMap::new()));
        let stale_revision = lifecycle::current_revision().wrapping_sub(1);
        let result = apply_state_change(
            &states,
            &json!({
                "entity_id": "sensor.old_server",
                "new_state": {"state": "99", "attributes": {}}
            }),
            Some(stale_revision),
        );
        assert!(result.is_none());
        assert!(states.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn cancelling_run_then_dropping_client_aborts_its_subscription() {
        struct SignalOnDrop(Option<tokio::sync::oneshot::Sender<()>>);
        impl Drop for SignalOnDrop {
            fn drop(&mut self) {
                let _ = self.0.take().unwrap().send(());
            }
        }
        let (started, ready) = tokio::sync::oneshot::channel();
        let (stopped, finished) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let _guard = SignalOnDrop(Some(stopped));
            let _ = started.send(());
            std::future::pending::<()>().await;
        });
        let mut client = HaWebSocketClient { task };
        tokio::select! {
            _ = ready => {},
            _ = client.run() => panic!("subscription unexpectedly completed"),
        }
        drop(client);
        tokio::time::timeout(Duration::from_secs(1), finished)
            .await
            .unwrap()
            .unwrap();
    }
}
