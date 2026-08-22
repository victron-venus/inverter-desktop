use rumqttc::{Client, MqttOptions, QoS};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::Emitter;

use crate::ha_api::HaEntityEntry;

const MQTT_KEEP_ALIVE_SECS: u64 = 60;
const KEEPALIVE_INTERVAL_SECS: u64 = 45;
const MQTT_QUEUE_CAPACITY: usize = 10;
const CONSOLE_MAX_LINES: usize = 50;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InverterState {
    pub gt: Option<f64>,
    pub g1: Option<f64>,
    pub g2: Option<f64>,
    pub tt: Option<f64>,
    pub t1: Option<f64>,
    pub t2: Option<f64>,
    pub solar_total: Option<f64>,
    pub mppt_total: Option<f64>,
    pub tasmota_total: Option<f64>,
    pub battery_soc: Option<f64>,
    pub battery_power: Option<f64>,
    pub battery_voltage: Option<f64>,
    pub battery_current: Option<f64>,
    pub setpoint: Option<f64>,
    pub inverter_state: Option<String>,
    pub version: Option<String>,
    pub dashboard_version: Option<String>,
    pub uptime: Option<u64>,
    pub ha_connected: Option<bool>,
    pub ha_direct_connected: Option<bool>,
    pub dry_run: Option<bool>,
    pub ess_mode: Option<EssMode>,
    pub booleans: Option<std::collections::HashMap<String, bool>>,
    pub features: Option<std::collections::HashMap<String, bool>>,
    pub mppt_individual: Option<Vec<f64>>,
    pub tasmota_individual: Option<Vec<f64>>,
    pub mppt_chargers: Option<Vec<MpptCharger>>,
    pub batteries: Option<Vec<Battery>>,
    pub loads: Option<std::collections::HashMap<String, f64>>,
    pub ui_config: Option<UiConfig>,
    pub daily_stats: Option<DailyStats>,
    pub ev_charging_kw: Option<f64>,
    pub ev_power: Option<f64>,
    pub car_soc: Option<f64>,
    pub water_level: Option<f64>,
    pub water_valve: Option<bool>,
    pub pump_switch: Option<bool>,
    pub dishwasher_running: Option<bool>,
    pub dishwasher_duration: Option<u64>,
    pub washer_time: Option<u64>,
    pub washer_power: Option<bool>,
    pub dryer_time: Option<u64>,
    pub dryer_power: Option<bool>,
    pub latest_version: Option<String>,
    pub console: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct RawInverterState {
    gt: Option<f64>,
    g1: Option<f64>,
    g2: Option<f64>,
    tt: Option<f64>,
    t1: Option<f64>,
    t2: Option<f64>,
    solar_total: Option<f64>,
    battery_soc: Option<f64>,
    battery_power: Option<f64>,
    battery_voltage: Option<f64>,
    battery_current: Option<f64>,
    setpoint: Option<f64>,
    inverter_state: Option<String>,
    version: Option<String>,
    dashboard_version: Option<String>,
    uptime: Option<u64>,
    ha_connected: Option<bool>,
    ha_direct_connected: Option<bool>,
    dry_run: Option<serde_json::Value>,
    ess_mode: Option<EssMode>,
    booleans: Option<std::collections::HashMap<String, serde_json::Value>>,
    features: Option<std::collections::HashMap<String, bool>>,
    mppt_individual: Option<Vec<f64>>,
    tasmota_individual: Option<Vec<f64>>,
    mppt_chargers: Option<Vec<MpptCharger>>,
    batteries: Option<Vec<Battery>>,
    loads: Option<std::collections::HashMap<String, f64>>,
    ui_config: Option<UiConfig>,
    daily_stats: Option<DailyStats>,
    ev_charging_kw: Option<f64>,
    ev_power: Option<f64>,
    car_soc: Option<f64>,
    water_level: Option<f64>,
    water_valve: Option<serde_json::Value>,
    pump_switch: Option<serde_json::Value>,
    dishwasher_running: Option<serde_json::Value>,
    dishwasher_duration: Option<u64>,
    washer_time: Option<u64>,
    washer_power: Option<serde_json::Value>,
    dryer_time: Option<u64>,
    dryer_power: Option<serde_json::Value>,
    latest_version: Option<String>,
    console: Option<Vec<String>>,
}

fn coerce_bool(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::String(s) => {
            let s_low = s.to_lowercase();
            s_low == "true" || s_low == "1" || s_low == "on" || s_low == "online"
        }
        serde_json::Value::Number(n) => n.as_f64().unwrap_or(0.0) != 0.0,
        _ => false,
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EssMode {
    pub mode_name: Option<String>,
    pub is_external: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MpptCharger {
    pub name: Option<String>,
    pub pv_voltage: Option<f64>,
    pub current: Option<f64>,
    pub power: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Battery {
    pub name: Option<String>,
    pub voltage: Option<f64>,
    pub current: Option<f64>,
    pub power: Option<f64>,
    pub soc: Option<f64>,
    pub state: Option<String>,
    pub time_to_go: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UiConfig {
    pub loads: Option<LoadsConfig>,
    pub home_buttons: Option<Vec<HomeButton>>,
    pub header_toggles: Option<Vec<HeaderToggle>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LoadsConfig {
    pub hidden: Option<Vec<String>>,
    pub min_watts: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HomeButton {
    pub id: String,
    pub label: String,
    pub entity: String,
    pub state_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaderToggle {
    pub id: String,
    pub label: String,
    pub entity: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DailyStats {
    pub produced_today: Option<f64>,
    pub produced_yesterday: Option<f64>,
    pub produced_dollars: Option<f64>,
    pub grid_kwh: Option<f64>,
    pub battery_in: Option<f64>,
    pub battery_out: Option<f64>,
    pub battery_in_yesterday: Option<f64>,
    pub battery_out_yesterday: Option<f64>,
    pub tasmota_daily: Option<Vec<f64>>,
    pub mppt_daily: Option<Vec<f64>>,
    pub pv_total_daily: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraEvent {
    pub agent_name: String,
    pub video_url: String,
    pub timestamp: Option<String>,
}

struct AlertState {
    triggered: bool,
    last_alert: Option<std::time::Instant>,
    last_notified_value: Option<f64>,
}

impl AlertState {
    fn new() -> Self {
        Self {
            triggered: false,
            last_alert: None,
            last_notified_value: None,
        }
    }

    fn should_alert(&mut self) -> bool {
        match self.last_alert {
            None => {
                self.triggered = true;
                self.last_alert = Some(std::time::Instant::now());
                true
            }
            Some(last) => {
                if last.elapsed() > std::time::Duration::from_secs(NOTIFICATION_COOLDOWN_SECS) {
                    self.last_alert = Some(std::time::Instant::now());
                    true
                } else {
                    false
                }
            }
        }
    }

    fn should_alert_value(&mut self, value: f64) -> bool {
        match self.last_notified_value {
            None => {
                self.triggered = true;
                self.last_notified_value = Some(value);
                true
            }
            Some(prev) => {
                if (prev - value).abs() > f64::EPSILON {
                    self.triggered = true;
                    self.last_notified_value = Some(value);
                    true
                } else {
                    false
                }
            }
        }
    }

    fn check_resolved(&mut self) {
        if self.triggered {
            self.triggered = false;
            self.last_alert = None;
            self.last_notified_value = None;
        }
    }
}

struct NotificationState {
    high_consumption: AlertState,
    low_water: AlertState,
    high_solar: AlertState,
    high_load: std::collections::HashMap<String, AlertState>,
}

pub struct MqttClient {
    client: Option<Client>,
    state: Arc<Mutex<InverterState>>,
    host: String,
    port: u16,
    username: Option<String>,
    password: Option<String>,
    app_handle: Option<tauri::AppHandle>,
    portal_id: Option<String>,
    camera_topic: Option<String>,
    notifications: Arc<Mutex<NotificationState>>,
    alarms: Arc<Mutex<HashMap<String, u8>>>,
    status_event: String,
    ha_entity_states: Option<Arc<Mutex<HashMap<String, HaEntityEntry>>>>,
}

/// Notification pushed by inverter-control on {prefix}/notifications.
#[derive(serde::Deserialize, serde::Serialize, Clone, Debug)]
pub struct MqttNotification {
    pub id: String,
    pub level: String,
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub ts: String,
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// "battery_512" -> "Battery 512", "vebus" -> "Vebus"
fn pretty_service_name(service: &str) -> String {
    match service.split_once('_') {
        Some((name, inst)) if !inst.is_empty() && inst.chars().all(|c| c.is_ascii_digit()) => {
            format!("{} {}", capitalize(name), inst)
        }
        _ => capitalize(service),
    }
}

/// Split CamelCase into words: "HighCellVoltage" -> ["High", "Cell", "Voltage"]
fn split_camel(s: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    for ch in s.chars() {
        if ch.is_uppercase() && !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
        current.push(ch);
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

/// "HighVoltage" -> "High voltage alarm"
fn pretty_alarm_name(alarm: &str) -> String {
    let mut words = split_camel(alarm);
    let mut out = words.first().cloned().unwrap_or_default();
    for w in words.drain(1..) {
        out.push(' ');
        out.push_str(&w.to_lowercase());
    }
    out.push_str(" alarm");
    out
}

fn match_mqtt_topic(topic: &str, pattern: &str) -> bool {
    if pattern == topic || pattern == "#" {
        return true;
    }
    let t_parts: Vec<&str> = topic.split('/').collect();
    let p_parts: Vec<&str> = pattern.split('/').collect();

    if pattern.ends_with("/#") {
        let prefix_len = p_parts.len() - 1;
        if t_parts.len() < prefix_len {
            return false;
        }
        return p_parts[..prefix_len]
            .iter()
            .zip(t_parts.iter())
            .all(|(p, t)| *p == "+" || *p == *t);
    }

    // Very basic MQTT wildcard matching for +
    if t_parts.len() != p_parts.len() {
        return false;
    }
    for (t, p) in t_parts.iter().zip(p_parts.iter()) {
        if *p != "+" && *p != *t {
            return false;
        }
    }
    true
}

use tauri_plugin_notification::NotificationExt;

const THRESHOLD_LOAD_W: f64 = 1500.0;
const THRESHOLD_CONSUMPTION_W: f64 = 1500.0;
const THRESHOLD_WATER_CM: f64 = 23.0;
const THRESHOLD_SOLAR_W: f64 = 3000.0;
const NOTIFICATION_COOLDOWN_SECS: u64 = 300;

fn fmt_watts(v: f64) -> String {
    if v >= 1000.0 {
        format!("{:.1}kW", v / 1000.0)
    } else {
        format!("{:.0}W", v)
    }
}

/// Resolve an HA entity's friendly_name, falling back to the entity_id.
fn entity_friendly_name(entry: &HaEntityEntry) -> Option<String> {
    entry
        .attributes
        .as_ref()
        .and_then(|a| a.get("friendly_name"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Find the HA friendly name for a load key (e.g. `stove` → `sensor.stove_power`'s friendly name).
/// Matches full entity ids, exact trailing segments, or ids containing the load as a segment,
/// preferring the most specific (shortest) entity id.
fn load_friendly_name(
    load: &str,
    entity_states: &HashMap<String, HaEntityEntry>,
) -> Option<String> {
    let load_lower = load.to_lowercase();
    let mut best: Option<(String, usize)> = None;
    for (entity_id, entry) in entity_states {
        let Some(name) = entity_friendly_name(entry) else {
            continue;
        };
        let eid_lower = entity_id.to_lowercase();
        let matches = eid_lower == load_lower
            || eid_lower.ends_with(&load_lower)
            || eid_lower.ends_with(&format!(".{}", load_lower))
            || eid_lower.contains(&format!(".{}", load_lower));
        if matches && best.as_ref().is_none_or(|(_, len)| entity_id.len() < *len) {
            best = Some((name, entity_id.len()));
        }
    }
    best.map(|(name, _)| name)
}

impl MqttClient {
    pub fn new(
        host: String,
        port: u16,
        username: Option<String>,
        password: Option<String>,
    ) -> Self {
        Self {
            client: None,
            state: Arc::new(Mutex::new(InverterState::default())),
            host,
            port,
            username,
            password,
            app_handle: None,
            portal_id: None,
            camera_topic: None,
            notifications: Arc::new(Mutex::new(NotificationState {
                high_consumption: AlertState::new(),
                low_water: AlertState::new(),
                high_solar: AlertState::new(),
                high_load: std::collections::HashMap::new(),
            })),
            alarms: Arc::new(Mutex::new(HashMap::new())),
            status_event: "mqtt-connection-status".to_string(),
            ha_entity_states: None,
        }
    }

    pub fn set_app_handle(&mut self, handle: tauri::AppHandle) {
        self.app_handle = Some(handle);
    }

    pub fn set_ha_entity_states(&mut self, states: Arc<Mutex<HashMap<String, HaEntityEntry>>>) {
        self.ha_entity_states = Some(states);
    }

    pub fn set_portal_id(&mut self, id: Option<String>) {
        self.portal_id = id;
    }

    pub fn set_camera_topic(&mut self, topic: Option<String>) {
        self.camera_topic = topic;
    }

    pub fn set_status_event(&mut self, event: String) {
        self.status_event = event;
    }

    pub fn get_state(&self) -> InverterState {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn connect(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let host = self.host.clone();
        let port = self.port;
        let username = self.username.clone();
        let password = self.password.clone();

        let state = self.state.clone();
        let app_handle = self.app_handle.clone();
        let portal_id = self.portal_id.clone();
        let cam_topic_owned = self.camera_topic.clone();
        let notifications = self.notifications.clone();
        let alarms = self.alarms.clone();
        let status_event = self.status_event.clone();
        let ha_entity_states = self.ha_entity_states.clone();

        tauri::async_runtime::spawn(async move {
            loop {
                // Log error separately so `result` drops before the await
                {
                    let is_err = Self::run_mqtt_loop(
                        &host,
                        port,
                        &username,
                        &password,
                        state.clone(),
                        app_handle.clone(),
                        portal_id.clone(),
                        cam_topic_owned.clone(),
                        notifications.clone(),
                        alarms.clone(),
                        ha_entity_states.clone(),
                        &status_event,
                    )
                    .await
                    .is_err();
                    if is_err {
                        log::error!("MQTT loop ended (err), reconnecting in 5s...");
                    } else {
                        log::info!("MQTT disconnected, reconnecting in 5s...");
                    }
                    // Connection lost or failed — wait before reconnecting
                    if let Some(ref handle) = app_handle {
                        let _ = handle.emit(&status_event, false);
                    }
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_mqtt_loop(
        host: &str,
        port: u16,
        username: &Option<String>,
        password: &Option<String>,
        state: Arc<Mutex<InverterState>>,
        app_handle: Option<tauri::AppHandle>,
        portal_id: Option<String>,
        camera_topic: Option<String>,
        notifications: Arc<Mutex<NotificationState>>,
        alarms: Arc<Mutex<HashMap<String, u8>>>,
        ha_entity_states: Option<Arc<Mutex<HashMap<String, HaEntityEntry>>>>,
        status_event: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let keepalive_secs = MQTT_KEEP_ALIVE_SECS;
        let queue_cap = MQTT_QUEUE_CAPACITY;

        let mut mqttoptions =
            MqttOptions::new("inverter-dashboard-desktop", (host.to_string(), port));
        mqttoptions.set_keep_alive(keepalive_secs as u16);

        if let (Some(u), Some(p)) = (username, password) {
            if !u.is_empty() && !p.is_empty() {
                mqttoptions.set_credentials(u, p.clone());
            }
        }

        let (client, mut connection) = Client::builder(mqttoptions).capacity(queue_cap).build();

        // Subscribe to topics using QoS 1 (AtLeastOnce)
        client.subscribe("inverter/state", QoS::AtLeastOnce)?;
        client.subscribe("inverter/console", QoS::AtLeastOnce)?;
        client.subscribe("inverter/notifications", QoS::AtLeastOnce)?;

        // Victron alarms relayed by the GX MQTT gateway from D-Bus /Alarms paths
        if let Some(ref id) = portal_id {
            if !id.is_empty() {
                let alarm_filter = format!("N/{}/+/Alarms/#", id);
                if let Err(e) = client.subscribe(&alarm_filter, QoS::AtLeastOnce) {
                    log::warn!("Failed to subscribe to {}: {:?}", alarm_filter, e);
                }
            }
        }

        if let Some(ref cam_topic) = camera_topic {
            if !cam_topic.is_empty() {
                client.subscribe(cam_topic, QoS::AtMostOnce)?;
            }
        }

        let keepalive_client = client.clone();

        // Spawn keep-alive publisher for Cerbo GX (runs in background)
        let pid = portal_id.clone();
        let ka_client = keepalive_client.clone();
        if let Some(ref id) = pid {
            let topic = format!("R/{}/keepalive", id);
            tauri::async_runtime::spawn(async move {
                let mut interval =
                    tokio::time::interval(Duration::from_secs(KEEPALIVE_INTERVAL_SECS));
                loop {
                    interval.tick().await;
                    let _ = ka_client.publish(&topic, QoS::AtMostOnce, false, "");
                }
            });
        }

        // NOTE: use tokio net (async) instead of blocking rumqttc sync iter.
        // Since rumqttc's AsyncClient/disconnection requires refactor, keep
        // spawn_blocking for backward compat but treat EOF as reconnect signal.
        let state_c = state.clone();
        let app_c = app_handle.clone();
        let cam_c = camera_topic.clone();
        let notif_c = notifications.clone();
        let alarms_c = alarms.clone();
        let ha_states_c = ha_entity_states.clone();
        let se = status_event.to_string();
        let con_result = tokio::task::spawn_blocking(move || {
            for event in connection.iter() {
                match event {
                    Ok(rumqttc::Event::Incoming(rumqttc::Packet::Publish(publish))) => {
                        // Use closures instead of closures capturing vars below
                        let topic = String::from_utf8_lossy(&publish.topic).to_string();
                        let payload = String::from_utf8(publish.payload.to_vec())
                            .unwrap_or_else(|_| String::new());

                        Self::handle_message(
                            &topic,
                            &payload,
                            &state_c,
                            &app_c,
                            &cam_c,
                            &notif_c,
                            &alarms_c,
                            &ha_states_c,
                        );
                    }
                    Ok(rumqttc::Event::Incoming(rumqttc::Packet::ConnAck(_))) => {
                        if let Some(ref handle) = app_c {
                            let _ = handle.emit(&se, true);
                        }
                    }
                    Ok(rumqttc::Event::Incoming(_)) => {}
                    Err(e) => {
                        log::error!("MQTT error: {:?}", e);
                        // Emit disconnect and return (exit for reconnect)
                        if let Some(ref handle) = app_c {
                            let _ = handle.emit(&se, false);
                        }
                        return Err(e.into());
                    }
                    _ => {}
                }
            }
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        })
        .await;

        match con_result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(e) => return Err(e.into()),
        }

        // Connection ended cleanly (EOF) — signal reconnect
        if let Some(ref handle) = app_handle {
            let _ = handle.emit(status_event, false);
        }
        Ok(())
    }

    fn handle_message(
        topic: &str,
        payload: &str,
        state: &Arc<Mutex<InverterState>>,
        app_handle: &Option<tauri::AppHandle>,
        camera_topic: &Option<String>,
        notifications: &Arc<Mutex<NotificationState>>,
        alarms: &Arc<Mutex<HashMap<String, u8>>>,
        ha_entity_states: &Option<Arc<Mutex<HashMap<String, HaEntityEntry>>>>,
    ) {
        if topic == "inverter/state" {
            if let Ok(raw) = serde_json::from_str::<RawInverterState>(payload) {
                Self::process_state_update(
                    raw,
                    state.clone(),
                    app_handle.clone(),
                    notifications.clone(),
                    ha_entity_states.clone(),
                );
            }
        } else if topic == "inverter/notifications" {
            match serde_json::from_str::<MqttNotification>(payload) {
                Ok(notification) => {
                    if let Some(ref handle) = app_handle {
                        let _ = handle.emit("mqtt-notification", &notification);
                        // Mirror to OS notification like local alerts
                        let _ = handle
                            .notification()
                            .builder()
                            .title(&notification.title)
                            .body(&notification.body)
                            .show();
                    }
                }
                Err(e) => log::warn!("Bad notification payload on {}: {}", topic, e),
            }
        } else if topic.starts_with("N/") && topic.contains("/Alarms/") {
            Self::handle_alarm_message(topic, payload, alarms, app_handle);
        } else if topic == "inverter/console" {
            if let Ok(mut guard) = state.lock() {
                let console = guard.console.get_or_insert_with(Vec::new);
                console.push(payload.to_string());
                if console.len() > CONSOLE_MAX_LINES {
                    console.remove(0);
                }
                if let Some(ref handle) = app_handle {
                    let _ = handle.emit("mqtt-state-update", &*guard);
                }
            }
        } else if let Some(ref cam_t) = camera_topic {
            if match_mqtt_topic(topic, cam_t) {
                if let Some(ref handle) = app_handle {
                    if let Ok(cam_event) = serde_json::from_str::<CameraEvent>(payload) {
                        let _ = handle.emit("camera-event", cam_event);
                    } else {
                        let _ = handle.emit(
                            "camera-event",
                            CameraEvent {
                                agent_name: "Unknown Camera".to_string(),
                                video_url: payload.to_string(),
                                timestamp: None,
                            },
                        );
                    }
                }
            }
        }
    }

    /// Track a Victron alarm topic (N/<portal>/<service>/Alarms/<Name>, value 0/1/2)
    /// and emit banner notifications on transitions. Value 0 clears the banner.
    fn handle_alarm_message(
        topic: &str,
        payload: &str,
        alarms: &Arc<Mutex<HashMap<String, u8>>>,
        app_handle: &Option<tauri::AppHandle>,
    ) {
        let value = serde_json::from_str::<serde_json::Value>(payload)
            .ok()
            .and_then(|v| v.get("value").and_then(|x| x.as_u64()))
            .unwrap_or(0) as u8;

        let prev = {
            let mut map = match alarms.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            let entry = map.entry(topic.to_string()).or_insert(0);
            let prev = *entry;
            *entry = value;
            prev
        };

        if prev == value {
            return;
        }

        let parts: Vec<&str> = topic.split('/').collect();
        // N/<portal>/<service_type>_<instance>/Alarms/<AlarmName>
        let service = parts.get(2).copied().unwrap_or("device");
        let alarm_name = parts.get(4).copied().unwrap_or(topic);
        let id = format!("victron-{}", topic);

        if let Some(ref handle) = app_handle {
            if value == 1 || value == 2 {
                let level = if value == 2 { "alarm" } else { "warning" };
                let state_txt = if value == 2 { "Alarm" } else { "Warning" };
                let _ = handle.emit(
                    "mqtt-notification",
                    MqttNotification {
                        id,
                        level: level.to_string(),
                        title: pretty_service_name(service),
                        body: format!("{}: {}", pretty_alarm_name(alarm_name), state_txt),
                        source: "victron".to_string(),
                        ts: String::new(),
                    },
                );
            } else {
                let _ = handle.emit("mqtt-notification-clear", serde_json::json!({ "id": id }));
            }
        }
    }

    /// Display name for a load in notifications: HA friendly name when available,
    /// otherwise the raw load key.
    fn load_display_name(
        load: &str,
        ha_entity_states: &Option<Arc<Mutex<HashMap<String, HaEntityEntry>>>>,
    ) -> String {
        if let Some(states) = ha_entity_states {
            if let Ok(guard) = states.lock() {
                if let Some(name) = load_friendly_name(load, &guard) {
                    return name;
                }
            }
        }
        load.to_string()
    }

    fn process_state_update(
        raw: RawInverterState,
        state: Arc<Mutex<InverterState>>,
        app_handle: Option<tauri::AppHandle>,
        notifications: Arc<Mutex<NotificationState>>,
        ha_entity_states: Option<Arc<Mutex<HashMap<String, HaEntityEntry>>>>,
    ) {
        let existing_console = state.lock().ok().and_then(|g| g.console.clone());

        let new_state = InverterState {
            gt: raw.gt,
            g1: raw.g1,
            g2: raw.g2,
            tt: raw.tt,
            t1: raw.t1,
            t2: raw.t2,
            solar_total: raw.solar_total,
            mppt_total: raw.mppt_individual.as_ref().map(|v| v.iter().sum()),
            tasmota_total: raw.tasmota_individual.as_ref().map(|v| v.iter().sum()),
            battery_soc: raw.battery_soc,
            battery_power: raw.battery_power,
            battery_voltage: raw.battery_voltage,
            battery_current: raw.battery_current,
            setpoint: raw.setpoint,
            inverter_state: raw.inverter_state,
            version: raw.version,
            dashboard_version: raw.dashboard_version,
            uptime: raw.uptime,
            ha_connected: raw.ha_connected,
            ha_direct_connected: raw.ha_direct_connected,
            dry_run: raw.dry_run.as_ref().map(coerce_bool),
            ess_mode: raw.ess_mode,
            booleans: raw
                .booleans
                .map(|map| map.into_iter().map(|(k, v)| (k, coerce_bool(&v))).collect()),
            features: raw.features,
            mppt_individual: raw.mppt_individual,
            tasmota_individual: raw.tasmota_individual,
            mppt_chargers: raw.mppt_chargers,
            batteries: raw.batteries,
            loads: raw.loads,
            ui_config: raw.ui_config,
            daily_stats: raw.daily_stats,
            ev_charging_kw: raw.ev_charging_kw,
            ev_power: raw.ev_power,
            car_soc: raw.car_soc,
            water_level: raw.water_level,
            water_valve: raw.water_valve.as_ref().map(coerce_bool),
            pump_switch: raw.pump_switch.as_ref().map(coerce_bool),
            dishwasher_running: raw.dishwasher_running.as_ref().map(coerce_bool),
            dishwasher_duration: raw.dishwasher_duration,
            washer_time: raw.washer_time,
            washer_power: raw.washer_power.as_ref().map(coerce_bool),
            dryer_time: raw.dryer_time,
            dryer_power: raw.dryer_power.as_ref().map(coerce_bool),
            latest_version: raw.latest_version,
            console: raw.console.or(existing_console),
        };

        // Skip alert/notification processing when window hidden (CPU/battery optimization)
        let hidden = crate::ha_api::WINDOW_HIDDEN.load(std::sync::atomic::Ordering::Relaxed);

        if !hidden {
            let mut alert_notifications: Vec<(String, String)> = Vec::new();
            if let Ok(mut alert_state) = notifications.lock() {
                let mut active_loads = std::collections::HashSet::new();
                if let Some(ref loads) = new_state.loads {
                    for (name, power) in loads {
                        if *power > THRESHOLD_LOAD_W {
                            active_loads.insert(name.clone());
                            let alert = alert_state
                                .high_load
                                .entry(name.clone())
                                .or_insert_with(AlertState::new);
                            if alert.should_alert() {
                                let display_name = Self::load_display_name(name, &ha_entity_states);
                                alert_notifications.push((
                                    "High Load".to_string(),
                                    format!("{}: {}", display_name, fmt_watts(*power)),
                                ));
                            }
                        }
                    }
                    alert_state
                        .high_load
                        .retain(|name, _| active_loads.contains(name));
                }

                if let Some(tt) = new_state.tt {
                    if tt > THRESHOLD_CONSUMPTION_W {
                        if alert_state.high_consumption.should_alert() {
                            alert_notifications.push((
                                "High Consumption".to_string(),
                                format!("Consumption: {}", fmt_watts(tt)),
                            ));
                        }
                    } else {
                        alert_state.high_consumption.check_resolved();
                    }
                }
                if let Some(wl) = new_state.water_level {
                    if wl < THRESHOLD_WATER_CM {
                        if alert_state.low_water.should_alert_value(wl) {
                            alert_notifications
                                .push(("Low Water".to_string(), format!("Water level: {} cm", wl)));
                        }
                    } else {
                        alert_state.low_water.check_resolved();
                    }
                }
                if let Some(st) = new_state.solar_total {
                    if st > THRESHOLD_SOLAR_W {
                        if alert_state.high_solar.should_alert() {
                            alert_notifications.push((
                                "High Solar".to_string(),
                                format!("Solar: {}", fmt_watts(st)),
                            ));
                        }
                    } else {
                        alert_state.high_solar.check_resolved();
                    }
                }
            }

            if let Some(ref handle) = app_handle {
                for (title, body) in &alert_notifications {
                    let _ = handle
                        .notification()
                        .builder()
                        .title(title)
                        .body(body)
                        .show();
                    let _ = handle.emit(
                        "notification",
                        serde_json::json!({ "title": title, "body": body }),
                    );
                }
            }
        }

        if let Some(ref handle) = app_handle {
            if !hidden {
                let _ = handle.emit("mqtt-state-update", &new_state);
            }
        }
        if let Ok(mut guard) = state.lock() {
            *guard = new_state;
        }
    }

    pub fn publish_command(
        &self,
        action: &str,
        payload: serde_json::Value,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(client) = &self.client {
            let topic = format!("inverter/cmd/{}", action);
            let payload_str = if payload.is_null() {
                String::new()
            } else {
                serde_json::to_string(&payload)?
            };
            client.publish(topic, QoS::AtLeastOnce, false, payload_str)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(friendly_name: &str) -> HaEntityEntry {
        HaEntityEntry {
            state: "on".to_string(),
            attributes: Some(serde_json::json!({ "friendly_name": friendly_name })),
        }
    }

    fn states() -> HashMap<String, HaEntityEntry> {
        let mut map = HashMap::new();
        map.insert("sensor.stove_power".to_string(), entry("Stove Power"));
        map.insert("switch.stove".to_string(), entry("Stove"));
        map.insert("sensor.washer_power_estimate".to_string(), entry("Washer"));
        map.insert("binary_sensor.dryer_running".to_string(), entry("Dryer"));
        map.insert("switch.shutoff_valve".to_string(), entry("Shutoff Valve"));
        map
    }

    #[test]
    fn resolves_friendly_name_from_entity_id() {
        let map = states();
        assert_eq!(
            load_friendly_name("sensor.washer_power_estimate", &map).as_deref(),
            Some("Washer")
        );
    }

    #[test]
    fn resolves_friendly_name_from_bare_load_key() {
        let map = states();
        assert_eq!(load_friendly_name("stove", &map).as_deref(), Some("Stove"));
        assert_eq!(load_friendly_name("dryer", &map).as_deref(), Some("Dryer"));
    }

    #[test]
    fn prefers_most_specific_matching_entity() {
        let map = states();
        // Both switch.stove (Stove) and sensor.stove_power (Stove Power) match "stove";
        // switch.stove is the shorter/more specific entity id.
        assert_eq!(load_friendly_name("stove", &map).as_deref(), Some("Stove"));
    }

    #[test]
    fn returns_none_when_no_match() {
        let map = states();
        assert_eq!(load_friendly_name("no_such_load", &map), None);
    }

    #[test]
    fn falls_back_when_friendly_name_missing() {
        let mut map = HashMap::new();
        map.insert(
            "sensor.plain".to_string(),
            HaEntityEntry {
                state: "on".to_string(),
                attributes: None,
            },
        );
        assert_eq!(load_friendly_name("plain", &map), None);
    }
}
