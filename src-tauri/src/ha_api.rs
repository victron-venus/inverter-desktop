use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tauri::Emitter;

pub static WINDOW_HIDDEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub static HA_WS_SHUTDOWN: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

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
        if entry.state == "unavailable" || entry.state == "unknown" {
            continue;
        }
        let domain = entity_id.split('.').next().unwrap_or("");
        let attrs = entry.attributes.as_ref();

        match domain {
            "sensor" | "binary_sensor" => {
                let name = attrs
                    .and_then(|a| a.get("friendly_name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or(entity_id);
                let unit = attrs
                    .and_then(|a| a.get("unit_of_measurement"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                sensors.push(HaSensorDisplay {
                    entity_id: entity_id.clone(),
                    name: name.to_string(),
                    state: entry.state.clone(),
                    unit: unit.to_string(),
                });
            }
            "number" => {
                let name = attrs
                    .and_then(|a| a.get("friendly_name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or(entity_id);
                let value = entry.state.parse::<f64>().unwrap_or(0.0);
                let min = attrs
                    .and_then(|a| a.get("min"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let max = attrs
                    .and_then(|a| a.get("max"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(100.0);
                let step = attrs
                    .and_then(|a| a.get("step"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(1.0);
                let unit = attrs
                    .and_then(|a| a.get("unit_of_measurement"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
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
                let name = attrs
                    .and_then(|a| a.get("friendly_name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or(entity_id);
                let position = attrs
                    .and_then(|a| a.get("current_position"))
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                covers.push(HaCoverDisplay {
                    entity_id: entity_id.clone(),
                    name: name.to_string(),
                    position,
                });
            }
            "media_player" => {
                let name = attrs
                    .and_then(|a| a.get("friendly_name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or(entity_id);
                media_players.push(HaMediaPlayerDisplay {
                    entity_id: entity_id.clone(),
                    name: name.to_string(),
                    state: entry.state.clone(),
                });
            }
            "scene" => {
                let name = attrs
                    .and_then(|a| a.get("friendly_name"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| {
                        entity_id
                            .strip_prefix("scene.")
                            .unwrap_or(entity_id)
                            .replace('_', " ")
                    });
                scenes.push(HaSceneDisplay {
                    entity_id: entity_id.clone(),
                    name,
                });
            }
            "weather" if weather.is_none() => {
                let name = attrs
                    .and_then(|a| a.get("friendly_name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("Weather");
                let temperature = attrs
                    .and_then(|a| a.get("temperature"))
                    .and_then(|v| v.as_f64());
                let unit = attrs
                    .and_then(|a| a.get("temperature_unit"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("°C")
                    .to_string();
                let forecast = attrs
                    .and_then(|a| a.get("forecast"))
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                weather = Some(HaWeatherDisplay {
                    entity_id: entity_id.clone(),
                    name: name.to_string(),
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
        let host = url.trim_end_matches('/');
        let port = port.unwrap_or(8123);
        let base_url = format!("{}:{}", host, port);

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
                if let (Ok(entity_id_str), Ok(state_str)) = (
                    serde_json::from_value::<String>(entity_id.clone()),
                    serde_json::from_value::<String>(state.clone()),
                ) {
                    let attributes = item.get("attributes").cloned();
                    let last_changed = item
                        .get("last_changed")
                        .and_then(|v| v.as_str().map(String::from));
                    let last_updated = item
                        .get("last_updated")
                        .and_then(|v| v.as_str().map(String::from));
                    result.push(HaState {
                        entity_id: entity_id_str,
                        state: state_str,
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
            let response = self
                .client
                .get(format!("{}/api/states/{}", self.base_url, eid))
                .header("Authorization", format!("Bearer {}", self.token))
                .send()
                .await
                .map_err(|e| format!("Failed to fetch entity {}: {}", eid, e))?;
            if response.status() == reqwest::StatusCode::UNAUTHORIZED {
                return Err("HA authentication failed (401)".to_string());
            }
            if !response.status().is_success() {
                log::warn!(
                    "HA entity {} returned HTTP {}, skipping",
                    eid,
                    response.status()
                );
                continue;
            }
            if let Ok(item) = response.json::<serde_json::Value>().await {
                if let (Some(entity_id), Some(state)) = (item.get("entity_id"), item.get("state")) {
                    if let (Ok(eid_str), Ok(state_str)) = (
                        serde_json::from_value::<String>(entity_id.clone()),
                        serde_json::from_value::<String>(state.clone()),
                    ) {
                        result.push(HaState {
                            entity_id: eid_str,
                            state: state_str,
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

/// HA WebSocket client — subscribes to all state_changed events and emits to frontend.
pub struct HaWebSocketClient {
    rx: Option<tokio::sync::oneshot::Receiver<()>>,
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
                        for state in states {
                            if let (Some(eid), Some(state_val)) =
                                (state.get("entity_id"), state.get("state"))
                            {
                                if let (Ok(eid_str), Ok(state_str)) = (
                                    serde_json::from_value::<String>(eid.clone()),
                                    serde_json::from_value::<String>(state_val.clone()),
                                ) {
                                    let attrs = state.get("attributes").cloned();
                                    states_guard.insert(
                                        eid_str,
                                        HaEntityEntry {
                                            state: state_str,
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

        // Emit initial filtered data immediately after populating state map
        // This ensures frontend has full data on WS connect
        if let Ok(states_guard) = entity_states.lock() {
            let filtered = compute_filtered_data(&states_guard);
            let _ = app.emit("ha-filtered-update", &filtered);
        }

        let (completion_tx, completion_rx) = tokio::sync::oneshot::channel::<()>();

        // Spawn read loop with timeout and shutdown signal
        let app_clone = app.clone();
        tokio::spawn(async move {
            const READ_TIMEOUT_SECS: u64 = 60;
            loop {
                // Check for external shutdown signal (from set_window_hidden)
                if HA_WS_SHUTDOWN.load(std::sync::atomic::Ordering::Relaxed) {
                    HA_WS_SHUTDOWN.store(false, std::sync::atomic::Ordering::Relaxed);
                    break;
                }

                tokio::select! {
                    msg = read.next() => {
                        match msg {
                            Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                                    if val.get("type").and_then(|v| v.as_str()) == Some("event") {
                                        if let Some(event) = val.get("event") {
                                            if let Some(new_state) = event.get("data").and_then(|d| d.get("new_state")) {
                                                if let (Some(entity_id), Some(state)) = (
                                                    new_state.get("entity_id").and_then(|v| v.as_str()),
                                                    new_state.get("state").and_then(|v| v.as_str()),
                                                ) {
                                                    let eid = entity_id.to_string();
                                                    let attrs = new_state.get("attributes").cloned();

                                                    // Update state map
                                                    if let Ok(mut states_guard) = entity_states.lock() {
                                                        states_guard.insert(eid.clone(), HaEntityEntry {
                                                            state: state.to_string(),
                                                            attributes: attrs.clone(),
                                                        });

                                                        // Skip expensive processing and emits when window is hidden
                                                        if !WINDOW_HIDDEN.load(std::sync::atomic::Ordering::Relaxed) {
                                                            // Emit individual update (backward compat for buttonStates, etc.)
                                                            let _ = app_clone.emit(
                                                                "ha-state-update",
                                                                serde_json::json!({
                                                                    "entity_id": eid,
                                                                    "state": state,
                                                                    "attributes": attrs.unwrap_or(serde_json::Value::Null),
                                                                }),
                                                            );

                                                            // Compute and emit pre-filtered entity data
                                                            let filtered = compute_filtered_data(&states_guard);
                                                            let _ = app_clone.emit("ha-filtered-update", &filtered);
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
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
            let _ = completion_tx.send(());
        });

        Ok(Self {
            rx: Some(completion_rx),
        })
    }

    /// Wait for the read loop to finish (blocks until connection drops or shutdown signal).
    pub async fn run(&mut self) {
        if let Some(rx) = self.rx.take() {
            let _ = rx.await;
        }
    }
}
