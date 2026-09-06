mod gateway;
mod ha_api;
pub(crate) mod mqtt;
#[cfg(target_os = "macos")]
mod tray_icon;

#[cfg(target_os = "macos")]
extern "C" {
    fn biometric_available() -> bool;
    fn biometric_authenticate(reason: *const std::os::raw::c_char) -> bool;
}

use aead::{Aead, KeyInit};
use aes_gcm::Aes256Gcm;
use base64::{engine::general_purpose, Engine as _};
use gateway::GatewayClient;
use log::{info, warn};
use mqtt::{HeaderToggle, InverterState, MqttClient};
use rand::RngExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::{Arc, Mutex};
use std::time::Duration;

// Desktop-only imports
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use keyring::Entry;

const KEYRING_SERVICE: &str = "inverter-desktop";
const KEYRING_USERNAME: &str = "victron";

// Desktop-only: OS keychain for encryption key.
// Key is cached in memory: keychain reads intermittently fail after sleep/wake
// (errSecNoSuchKeychain), and load_config runs on every action button press.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
static ENCRYPTION_KEY_CACHE: Mutex<Option<Vec<u8>>> = Mutex::new(None);

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn get_or_create_encryption_key() -> Result<Vec<u8>, String> {
    if let Ok(cache) = ENCRYPTION_KEY_CACHE.lock() {
        if let Some(key) = cache.as_ref() {
            return Ok(key.clone());
        }
    }

    let entry = Entry::new(KEYRING_SERVICE, KEYRING_USERNAME)
        .map_err(|e| format!("Keyring error: {}", e))?;

    let key: Vec<u8> = match entry.get_password() {
        Ok(key_b64) => {
            let key = general_purpose::STANDARD
                .decode(key_b64)
                .map_err(|e| format!("Failed to decode encryption key: {}", e))?;
            if key.len() != 32 {
                return Err("Invalid encryption key length".to_string());
            }
            key
        }
        Err(keyring::Error::NoEntry) => {
            // Generate new encryption key
            let mut key = [0u8; 32];
            rand::rng().fill(&mut key);
            let key_b64 = general_purpose::STANDARD.encode(key);
            entry
                .set_password(&key_b64)
                .map_err(|e| format!("Failed to save encryption key: {}", e))?;
            key.to_vec()
        }
        Err(e) => Err(format!("Keyring error: {}", e))?,
    };

    if let Ok(mut cache) = ENCRYPTION_KEY_CACHE.lock() {
        *cache = Some(key.clone());
    }
    Ok(key)
}

// Mobile fallback: use a fixed derivation (less secure but functional)
#[cfg(any(target_os = "android", target_os = "ios"))]
fn get_or_create_encryption_key() -> Result<Vec<u8>, String> {
    // For mobile, derive key from app identifier (deterministic, no keychain)
    // This is less secure but allows the app to function on mobile
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    "inverter-desktop-victron-encryption-key".hash(&mut hasher);
    let hash = hasher.finish();

    let mut key = [0u8; 32];
    for (i, b) in hash.to_le_bytes().iter().cycle().take(32).enumerate() {
        key[i] = *b;
    }
    Ok(key.to_vec())
}

fn encrypt_config(config: &FullConfig, key: &[u8]) -> Result<String, String> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| format!("Invalid key: {}", e))?;
    let plaintext = serde_json::to_vec(config).map_err(|e| e.to_string())?;

    let mut nonce_bytes = [0u8; 12];
    rand::rng().fill(&mut nonce_bytes);
    let nonce = <aes_gcm::Nonce<aead::consts::U12>>::try_from(nonce_bytes.as_slice())
        .map_err(|e| format!("Nonce error: {}", e))?;

    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_ref())
        .map_err(|e| format!("Encryption failed: {}", e))?;

    let mut result = nonce_bytes.to_vec();
    result.extend_from_slice(&ciphertext);
    Ok(general_purpose::STANDARD.encode(&result))
}

fn decrypt_config(encrypted: &str, key: &[u8]) -> Result<FullConfig, String> {
    let data = general_purpose::STANDARD
        .decode(encrypted)
        .map_err(|e| format!("Base64 decode failed: {}", e))?;

    if data.len() < 12 {
        return Err("Invalid encrypted data: too short".to_string());
    }

    let (nonce_bytes, ciphertext) = data.split_at(12);
    let nonce = <aes_gcm::Nonce<aead::consts::U12>>::try_from(nonce_bytes)
        .map_err(|e| format!("Nonce error: {}", e))?;

    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| format!("Invalid key: {}", e))?;
    let plaintext = cipher
        .decrypt(&nonce, ciphertext)
        .map_err(|e| format!("Decryption failed: {}", e))?;

    serde_json::from_slice(&plaintext).map_err(|e| format!("JSON parse failed: {}", e))
}

fn load_config(app: &tauri::AppHandle) -> Result<FullConfig, String> {
    let key = get_or_create_encryption_key()?;

    let store = app
        .store_builder("config.json")
        .build()
        .map_err(|e| format!("Failed to build store: {}", e))?;

    match store.get("config") {
        Some(v) => {
            if let Some(encrypted_str) = v.as_str() {
                decrypt_config(encrypted_str, &key)
            } else {
                // Legacy unencrypted config - migrate
                let config: FullConfig = serde_json::from_value(v).unwrap_or_default();
                if let Ok(encrypted) = encrypt_config(&config, &key) {
                    store.set("config", serde_json::json!(encrypted));
                    let _ = store.save();
                }
                Ok(config)
            }
        }
        None => Ok(FullConfig::default()),
    }
}

fn save_config_encrypted(app: &tauri::AppHandle, config: &FullConfig) -> Result<(), String> {
    let key = get_or_create_encryption_key()?;
    let encrypted = encrypt_config(config, &key)?;

    let store = app
        .store_builder("config.json")
        .build()
        .map_err(|e| format!("Failed to build store: {}", e))?;

    store.set("config", serde_json::json!(encrypted));
    store
        .save()
        .map_err(|e| format!("Failed to save config: {}", e))?;

    Ok(())
}

const DEFAULT_MQTT_HOST: &str = "Cerbo";
const DEFAULT_MQTT_PORT: u16 = 1883;
const DEFAULT_HA_PORT: u16 = 8123;
const HA_ENTITY_DOMAINS: &[&str] = &[
    "switch",
    "light",
    "input_boolean",
    "fan",
    "cover",
    "lock",
    "media_player",
    "scene",
    "script",
    "number",
    "sensor",
    "binary_sensor",
    "climate",
    "button",
];

/// Inverter-control flags owned by inverter-control. Always published to
/// Cerbo MQTT `inverter/cmd/toggle` — never Home Assistant REST, even when
/// `ha_use_direct_api` is on. Accepts `input_boolean.<key>` or bare `<key>`.
const INVERTER_CONTROL_FLAGS: &[&str] = &[
    "only_charging",
    "no_feed",
    "house_support",
    "charge_battery",
    "do_not_supply_charger",
    "set_limit_to_ev_charger",
    "minimize_charging",
];
const ABOUT_WINDOW_W: f64 = 380.0;
const ABOUT_WINDOW_H: f64 = 320.0;
const CONFIG_WINDOW_W: f64 = 850.0;
const CONFIG_WINDOW_H: f64 = 700.0;

use tauri::{Emitter, Manager, State, WindowEvent};
use tauri_plugin_store::StoreExt;

#[cfg(desktop)]
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};

#[derive(Debug, Serialize, Deserialize, Clone)]
struct DiscoveredEntity {
    entity_id: String,
    friendly_name: String,
    domain: String,
    state: String,
}

// Global state for the MQTT clients
struct MqttState(Arc<Mutex<Option<MqttClient>>>);
struct GatewayState(Arc<Mutex<Option<GatewayClient>>>);
struct HaMqttState(Arc<Mutex<Option<MqttClient>>>);
pub(crate) struct HaEntityStates(pub(crate) Arc<Mutex<HashMap<String, ha_api::HaEntityEntry>>>);

#[derive(Debug, Serialize, Deserialize, Clone)]
struct HaEntityConfig {
    id: String,
    label: String,
    entity: String,
    domain: String,
    enabled: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct FullConfig {
    mqtt_host: String,
    mqtt_port: u16,
    mqtt_login: Option<String>,
    mqtt_password: Option<String>,
    mqtt_ha_host: Option<String>,
    mqtt_ha_port: Option<u16>,
    mqtt_ha_login: Option<String>,
    mqtt_ha_password: Option<String>,
    ha_longlived_token: Option<String>,
    ha_url: Option<String>,
    ha_port: Option<u16>,
    ha_use_direct_api: bool,
    ha_dryer_entity: Option<String>,
    ha_washer_entity: Option<String>,
    ha_washer_start_entity: Option<String>,
    ha_washer_pause_entity: Option<String>,
    ha_dryer_start_entity: Option<String>,
    ha_dryer_pause_entity: Option<String>,
    ha_dishwasher_running_entity: Option<String>,
    ha_dishwasher_duration_entity: Option<String>,
    ha_ev_soc_entity: Option<String>,
    ha_ev_charging_entity: Option<String>,
    ha_ev_clamp_entity: Option<String>,
    // Live power tiles prefer Cerbo GX MQTT (system/vebus/shunt/acload/MPPT/PV/EV/water).
    // Daemon inverter/state still supplies: daily_stats, solar_forecast, booleans,
    // features, ess_mode, versions, dry_run, ui_config, console, HA connectivity flags.
    // HA entities cover washer/dryer/dishwasher (not merged from daemon).
    // Optional HA CT clamps if you prefer HA meters over Victron D-Bus:
    ha_consumption_clamps: Option<Vec<String>>,
    ha_generation_clamps: Option<Vec<String>>,
    color_scheme: Option<String>,
    // unified entities config
    ha_entities: Option<Vec<HaEntityConfig>>,
    header_toggles_config: Option<Vec<HeaderToggle>>,
    portal_id: Option<String>,
    #[serde(default)]
    water_tank_instance: Option<u32>,
    water_pump_instance: Option<u32>,
    water_valve_instance: Option<u32>,
    #[serde(default = "default_evcharger_instance")]
    evcharger_instance: Option<u32>,
    #[serde(default = "default_ev_instance")]
    ev_instance: Option<u32>,
    camera_topic: Option<String>,
    camera_enabled: bool,
    show_advanced_settings: Option<bool>,
    show_ha_sensors: Option<bool>,
    show_ha_numbers: Option<bool>,
    show_ha_covers: Option<bool>,
    show_ha_media: Option<bool>,
    show_ha_scenes: Option<bool>,
    show_ha_weather: Option<bool>,
    show_console: Option<bool>,
    ha_appliance_entities: Option<std::collections::HashMap<String, String>>,
    auto_start: Option<bool>,
    auth_enabled: Option<bool>,
    auth_username: Option<String>,
    auth_password: Option<String>,
    auth_biometric: Option<bool>,

    /// When true, prefer remote inverter-gateway (Cloudflare Access + bearer) over LAN-only MQTT path (future live data).
    #[serde(default)]
    gateway_enabled: bool,
    /// Public HTTPS base URL, e.g. https://victron.example.com (no trailing slash required).
    gateway_url: Option<String>,
    /// Cloudflare Access Service Token Client ID (CF-Access-Client-Id).
    gateway_access_client_id: Option<String>,
    /// Cloudflare Access Service Token Client Secret (CF-Access-Client-Secret).
    gateway_access_client_secret: Option<String>,
    /// Gateway API bearer (Authorization: Bearer … / GATEWAY_API_TOKEN).
    gateway_api_token: Option<String>,
}

fn default_evcharger_instance() -> Option<u32> {
    Some(40)
}

fn default_ev_instance() -> Option<u32> {
    Some(22)
}

impl Default for FullConfig {
    fn default() -> Self {
        Self {
            mqtt_host: DEFAULT_MQTT_HOST.to_string(),
            mqtt_port: DEFAULT_MQTT_PORT,
            mqtt_login: None,
            mqtt_password: None,
            mqtt_ha_host: Some(DEFAULT_MQTT_HOST.to_string()),
            mqtt_ha_port: Some(DEFAULT_MQTT_PORT),
            mqtt_ha_login: None,
            mqtt_ha_password: None,
            ha_longlived_token: None,
            ha_url: None,
            ha_port: None,
            ha_use_direct_api: false,
            ha_dryer_entity: None,
            ha_washer_entity: None,
            ha_washer_start_entity: None,
            ha_washer_pause_entity: None,
            ha_dryer_start_entity: None,
            ha_dryer_pause_entity: None,
            ha_dishwasher_running_entity: None,
            ha_dishwasher_duration_entity: None,
            ha_ev_soc_entity: None,
            ha_ev_charging_entity: None,
            ha_ev_clamp_entity: None,
            // Live tiles: Cerbo-first; daemon for stats/flags/config (see FullConfig note)
            ha_consumption_clamps: None,
            ha_generation_clamps: None,
            color_scheme: Some("dark".to_string()),
            ha_entities: None,
            header_toggles_config: None,
            portal_id: None,
            // dbus-pump defaults on the GX (see dbus-pump local_config.example.py)
            water_tank_instance: None,
            water_pump_instance: Some(1),
            water_valve_instance: Some(2),
            evcharger_instance: Some(40),
            ev_instance: Some(22),
            camera_topic: Some("frigate/+/events".to_string()),
            camera_enabled: false,
            show_advanced_settings: Some(false),
            show_ha_sensors: Some(true),
            show_ha_numbers: Some(true),
            show_ha_covers: Some(true),
            show_ha_media: Some(true),
            show_ha_scenes: Some(true),
            show_ha_weather: Some(true),
            show_console: Some(true),
            ha_appliance_entities: None,
            auto_start: Some(false),
            auth_enabled: Some(false),
            auth_username: None,
            auth_password: None,
            auth_biometric: Some(false),
            gateway_enabled: false,
            gateway_url: None,
            gateway_access_client_id: None,
            gateway_access_client_secret: None,
            gateway_api_token: None,
        }
    }
}

#[tauri::command]
fn get_state(
    mqtt_client: State<MqttState>,
    gateway_client: State<GatewayState>,
) -> Result<InverterState, String> {
    if let Ok(g) = gateway_client.0.lock() {
        if let Some(ref client) = *g {
            return Ok(client.get_state());
        }
    }
    let client = mqtt_client
        .0
        .lock()
        .map_err(|e| format!("Internal error: {}", e))?;
    if let Some(ref client) = *client {
        Ok(client.get_state())
    } else {
        Err("MQTT client not connected".to_string())
    }
}

/// If the action is `toggle` and the entity is an inverter-control flag,
/// inject an explicit `state` field so inverter-control's `_handle_toggle`
/// sets the absolute value instead of flipping whatever it last saw.
fn stamp_toggle_state(payload: &mut serde_json::Value, client: &MqttClient, action: &str) {
    if action != "toggle" {
        return;
    }
    let Some(entity) = payload.get("entity").and_then(|v| v.as_str()) else {
        return;
    };
    if !is_inverter_control_flag(entity) {
        return;
    }
    let key = entity.split('.').next_back().unwrap_or(entity);
    let current = client.flag_state(key).unwrap_or(false);
    let new_state = if !current { "on" } else { "off" };
    if let Some(obj) = payload.as_object_mut() {
        obj.insert(
            "state".to_string(),
            serde_json::Value::String(new_state.to_string()),
        );
    }
}

#[tauri::command]
async fn perform_action(
    action: String,
    payload: serde_json::Value,
    app: tauri::AppHandle,
    mqtt_client: State<'_, MqttState>,
) -> Result<(), String> {
    info!("perform_action: action={}, payload={}", action, payload);

    // Water control goes only through dbus-pump's writable /Mode via the GX
    // MQTT-API (single control plane) - never straight to Home Assistant.
    if action == "water_mode" {
        let which = payload.get("which").and_then(|v| v.as_str()).unwrap_or("");
        let mode = payload.get("mode").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
        let client = mqtt_client
            .0
            .lock()
            .map_err(|e| format!("Internal error: {}", e))?;
        return match client.as_ref() {
            Some(c) => {
                c.publish_command("water_mode_set", json!({ "which": which, "mode": mode }))
                    .map_err(|e| e.to_string())?;
                Ok(())
            }
            None => Err("MQTT client not connected".to_string()),
        };
    }

    let config = load_config(&app)?;

    let entity_id = payload.get("entity").and_then(|v| v.as_str());

    // HA REST is for home devices (garage, recliner, laundry, EV, covers, …).
    // Inverter-control flags always go to Cerbo MQTT — ha_use_direct_api does
    // not apply to them (inverter-control no longer reads HA for those 7).
    let ha_direct =
        config.ha_use_direct_api && config.ha_url.is_some() && config.ha_longlived_token.is_some();
    if should_use_ha_rest(entity_id, ha_direct) {
        if let Some(entity) = entity_id {
            let domain = entity.split('.').next().unwrap_or("");
            // For switch/input_boolean/light entities, always prefer HA API
            let client = ha_api::HaApiClient::new(
                config.ha_url.as_deref().unwrap_or(""),
                config.ha_port,
                config.ha_longlived_token.as_deref().unwrap_or(""),
            )
            .await?;

            match domain {
                "cover" => {
                    let position = payload
                        .get("position")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u8;
                    client.set_cover_position(entity, position).await?;
                }
                "media_player" => {
                    let mp_action = payload
                        .get("mp_action")
                        .and_then(|v| v.as_str())
                        .unwrap_or("toggle");
                    match mp_action {
                        "play" => client.media_player_play(entity).await?,
                        "pause" => client.media_player_pause(entity).await?,
                        "stop" => client.media_player_stop(entity).await?,
                        _ => {
                            // toggle: on/off
                            let states = client.get_states().await?;
                            let state = states.iter().find(|s| s.entity_id == entity);
                            if let Some(s) = state {
                                if s.state == "on" {
                                    client.turn_off(entity).await?
                                } else {
                                    client.turn_on(entity).await?
                                }
                            }
                        }
                    }
                }
                "number" => {
                    let value = payload.get("value").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    client
                        .call_service(
                            entity,
                            "number",
                            "set_value",
                            serde_json::json!({ "value": value }),
                        )
                        .await?;
                }
                "scene" => {
                    client.scene_activate(entity).await?;
                }
                "button" => {
                    client
                        .call_service(entity, "button", "press", serde_json::json!({}))
                        .await?;
                }
                _ => {
                    let states = client.get_states().await?;
                    let state = states.iter().find(|s| s.entity_id == entity);
                    match state {
                        Some(s) => {
                            if s.state == "on" {
                                client.turn_off(entity).await?
                            } else {
                                client.turn_on(entity).await?
                            }
                        }
                        None => {
                            // Entity not found in HA, fallback to MQTT
                            log::warn!("Entity {} not found in HA, falling back to MQTT", entity);
                            let mqtt_client = mqtt_client
                                .0
                                .lock()
                                .map_err(|e| format!("Lock error: {}", e))?;
                            let c = mqtt_client
                                .as_ref()
                                .ok_or_else(|| "MQTT client not connected".to_string())?;
                            c.publish_command(&action, payload.clone())
                                .map_err(|e| format!("MQTT error: {}", e))?;
                            return Ok(());
                        }
                    }
                }
            }
            return Ok(());
        }
    }

    info!("perform_action: MQTT fallback for action={}", action);
    let client = mqtt_client
        .0
        .lock()
        .map_err(|e| format!("Internal error: {}", e))?;
    let client = client
        .as_ref()
        .ok_or_else(|| "MQTT client not connected".to_string())?;

    // Inverter-control flags: send an explicit `state` so inverter-control's
    // _handle_toggle can set the absolute value, rather than flipping whatever
    // it last saw (and getting out of sync if a click was lost in flight).
    let mut payload = payload;
    stamp_toggle_state(&mut payload, client, &action);

    client
        .publish_command(&action, payload)
        .map_err(|e| e.to_string())
}

fn is_ha_entity(entity_id: &str) -> bool {
    let domain = entity_id.split('.').next().unwrap_or("");
    HA_ENTITY_DOMAINS.contains(&domain)
}

/// Bare key (`only_charging`) or HA-style id (`input_boolean.only_charging`).
pub(crate) fn is_inverter_control_flag(entity_or_id: &str) -> bool {
    let key = entity_or_id.split('.').next_back().unwrap_or("").trim();
    INVERTER_CONTROL_FLAGS.contains(&key)
}

/// HA REST is for home devices only — inverter-control flags always go to MQTT.
fn should_use_ha_rest(entity_id: Option<&str>, ha_direct: bool) -> bool {
    if !ha_direct {
        return false;
    }
    match entity_id {
        Some(e) => is_ha_entity(e) && !is_inverter_control_flag(e),
        None => false,
    }
}

/// Build HA WebSocket URL from config, handling host:port format properly.
fn build_ws_url(ha_url: &str, ha_port: Option<u16>) -> String {
    let url = ha_url.trim();

    // Determine ws:// or wss:// prefix
    let (prefix, rest) = if let Some(stripped) = url.strip_prefix("https://") {
        ("wss://", stripped)
    } else if let Some(stripped) = url.strip_prefix("http://") {
        ("ws://", stripped)
    } else {
        ("ws://", url)
    };

    // rest may be "host:port", "host", "[ipv6]:port", "[ipv6]", or "host/path"
    let host_part = rest.split('/').next().unwrap_or(rest);
    let port = if host_part.starts_with('[') {
        // IPv6: [::1]:port or [::1]
        let bracket_end = host_part.find(']');
        let has_port = bracket_end.is_some_and(|i| {
            host_part.len() > i + 1 && host_part.as_bytes().get(i + 1) == Some(&b':')
        });
        if has_port {
            String::new()
        } else {
            format!(":{}", ha_port.unwrap_or(8123))
        }
    } else if host_part.contains(':') {
        // IPv4 with port
        String::new()
    } else {
        format!(":{}", ha_port.unwrap_or(8123))
    };

    format!("{}{}{}/api/websocket", prefix, host_part, port)
}

fn start_ha_polling(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let config = match load_config(&app) {
                Ok(c) => c,
                Err(e) => {
                    warn!("Failed to load config for HA polling: {}", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
            };

            if !config.ha_use_direct_api
                || config.ha_url.is_none()
                || config.ha_longlived_token.is_none()
            {
                if let Ok(mut states_guard) = app.state::<HaEntityStates>().0.lock() {
                    states_guard.clear();
                }
                let _ = app.emit("ha-connection-status", false);
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }

            let base = config.ha_url.clone().unwrap();
            let token = config.ha_longlived_token.clone().unwrap();
            let ws_url = build_ws_url(&base, config.ha_port);

            let entity_states = app.state::<HaEntityStates>().0.clone();

            info!("HA WS connecting to {}", ws_url);
            match ha_api::HaWebSocketClient::connect(&ws_url, &token, app.clone(), entity_states)
                .await
            {
                Ok(mut ws_client) => {
                    // Retry previously-404 entities after a successful reconnect
                    // (HA may have added them; avoids re-spam during failed reconnect loops).
                    ha_api::clear_entity_skip_list();
                    info!("HA WebSocket connected");
                    let _ = app.emit("ha-connection-status", true);
                    let _ = app.emit("ha-state-update", serde_json::json!({ "connected": true }));
                    ws_client.run().await;
                    info!("HA WebSocket disconnected, reconnecting...");
                    let _ = app.emit("ha-connection-status", false);
                }
                Err(e) => {
                    warn!("HA WebSocket connect failed: {}, retrying in 5s", e);
                    let _ = app.emit("ha-connection-status", false);
                }
            }

            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
}

#[tauri::command]
fn get_config(app: tauri::AppHandle) -> Result<FullConfig, String> {
    let mut config = load_config(&app)?;

    // Check if this is first run (config not yet saved)
    let store = app
        .store_builder("config.json")
        .build()
        .map_err(|e| format!("Failed to build store: {}", e))?;

    let is_first_run = store.get("config").is_none();

    let mut changed = false;

    if is_first_run {
        info!("Config: First run detected. Checking environment variables for seeding...");
        // Auto-fill from env ONLY on first run
        if let Ok(server) = std::env::var("HA_SERVER") {
            if !server.is_empty() {
                info!("Config: Found HA_SERVER={}", server);
                let url_base = if server.contains("://") {
                    server.clone()
                } else {
                    format!("http://{}", server)
                };

                let host_part = url_base
                    .trim_start_matches("http://")
                    .trim_start_matches("https://");
                let host_only = host_part
                    .split('/')
                    .next()
                    .unwrap_or(host_part)
                    .split(':')
                    .next()
                    .unwrap_or(host_part);

                config.ha_url = Some(format!("http://{}", host_only));
                config.mqtt_ha_host = Some(host_only.to_string());

                if let Some(port_str) = host_part.split(':').nth(1) {
                    if let Ok(port) = port_str
                        .split('/')
                        .next()
                        .unwrap_or(port_str)
                        .parse::<u16>()
                    {
                        config.ha_port = Some(port);
                        info!("Config: Parsed port {} from HA_SERVER", port);
                    }
                }
                info!(
                    "Config: Seeded ha_url={:?}, mqtt_ha_host={:?}",
                    config.ha_url, config.mqtt_ha_host
                );
                changed = true;
            }
        }

        if let Ok(token) = std::env::var("HA_TOKEN") {
            if !token.is_empty() {
                info!("Config: Found HA_TOKEN (length={})", token.len());
                config.ha_longlived_token = Some(token);
                changed = true;
            }
        }

        if let Ok(user) = std::env::var("HA_MQTT_USER") {
            if !user.is_empty() {
                info!("Config: Found HA_MQTT_USER={}", user);
                config.mqtt_ha_login = Some(user);
                changed = true;
            }
        }

        if let Ok(pwd) = std::env::var("HA_MQTT_PWD") {
            if !pwd.is_empty() {
                info!("Config: Found HA_MQTT_PWD");
                config.mqtt_ha_password = Some(pwd);
                changed = true;
            }
        }

        if config.ha_url.is_some() && config.ha_longlived_token.is_some() {
            info!("Config: Auto-enabling direct HA API");
            config.ha_use_direct_api = true;
            changed = true;
        }
    }

    // Default values if missing (backward compatibility)
    if config.ha_port.is_none() {
        config.ha_port = Some(DEFAULT_HA_PORT);
        changed = true;
    }
    if config.mqtt_port == 0 {
        config.mqtt_port = DEFAULT_MQTT_PORT;
        changed = true;
    }
    if config.mqtt_ha_port.is_none() {
        config.mqtt_ha_port = Some(DEFAULT_MQTT_PORT);
        changed = true;
    }

    if changed {
        save_config_encrypted(&app, &config)?;
    }

    Ok(config)
}

#[tauri::command]
async fn save_config(app: tauri::AppHandle, config: FullConfig) -> Result<(), String> {
    save_config_encrypted(&app, &config)?;
    // Fixed / newly configured entity IDs should be polled again without app restart.
    ha_api::clear_entity_skip_list();
    Ok(())
}

#[tauri::command]
async fn backup_config(app: tauri::AppHandle) -> Result<bool, String> {
    use tauri_plugin_dialog::{DialogExt, FilePath};

    let file = app
        .dialog()
        .file()
        .set_file_name("config-backup.json")
        .add_filter("JSON", &["json"])
        .blocking_save_file();

    let path = match file {
        Some(FilePath::Path(p)) => p,
        // User cancelled the dialog
        _ => return Ok(false),
    };

    let config = load_config(&app)?;
    let json = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    std::fs::write(&path, json).map_err(|e| format!("Failed to write backup file: {}", e))?;
    info!("Config backed up to {}", path.display());
    Ok(true)
}

#[tauri::command]
async fn restore_config(app: tauri::AppHandle) -> Result<bool, String> {
    use tauri_plugin_dialog::{DialogExt, FilePath};

    let file = app
        .dialog()
        .file()
        .add_filter("JSON", &["json"])
        .blocking_pick_file();

    let path = match file {
        Some(FilePath::Path(p)) => p,
        // User cancelled the dialog
        _ => return Ok(false),
    };

    let content =
        std::fs::read_to_string(&path).map_err(|e| format!("Failed to read backup file: {}", e))?;
    let config: FullConfig =
        serde_json::from_str(&content).map_err(|e| format!("Invalid backup file: {}", e))?;
    save_config_encrypted(&app, &config)?;
    ha_api::clear_entity_skip_list();
    info!("Config restored from {}", path.display());
    Ok(true)
}

#[tauri::command]
fn acknowledge_victron_banner(id: String, mqtt_client: State<'_, MqttState>) -> Result<(), String> {
    // id: victron-platform-<inst>-<slot>
    let rest = id
        .strip_prefix("victron-platform-")
        .ok_or_else(|| format!("Not a Victron platform banner id: {id}"))?;
    let mut parts = rest.splitn(2, '-');
    let platform_instance: u32 = parts
        .next()
        .ok_or("missing platform instance")?
        .parse()
        .map_err(|e| format!("bad platform instance: {e}"))?;
    let slot: u32 = parts
        .next()
        .ok_or("missing slot")?
        .parse()
        .map_err(|e| format!("bad slot: {e}"))?;
    let client = mqtt_client
        .0
        .lock()
        .map_err(|e| format!("Internal error: {e}"))?;
    let client = client
        .as_ref()
        .ok_or_else(|| "MQTT client not connected".to_string())?;
    client
        .acknowledge_victron_notification(platform_instance, slot)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn connect_mqtt(
    host: String,
    port: u16,
    username: Option<String>,
    password: Option<String>,
    portal_id: Option<String>,
    water_tank_instance: Option<u32>,
    water_pump_instance: Option<u32>,
    water_valve_instance: Option<u32>,
    evcharger_instance: Option<u32>,
    ev_instance: Option<u32>,
    camera_topic: Option<String>,
    app: tauri::AppHandle,
    mqtt_client: State<'_, MqttState>,
    gateway_client: State<'_, GatewayState>,
) -> Result<(), String> {
    // Drop/stop any previous client first so its reconnect loop cannot keep
    // discovering the portal (xN) and racing the new connection.
    {
        let mut gw = gateway_client
            .0
            .lock()
            .map_err(|e| format!("Internal error: {}", e))?;
        if let Some(old) = gw.take() {
            old.stop();
        }
    }
    {
        let mut client_guard = mqtt_client
            .0
            .lock()
            .map_err(|e| format!("Internal error: {}", e))?;
        if let Some(old) = client_guard.take() {
            old.stop();
        }
    }
    let mut client = MqttClient::new(
        host,
        port,
        username,
        password,
        "inverter-dashboard-desktop".to_string(),
    );
    client.set_ha_entity_states(app.state::<HaEntityStates>().0.clone());
    client.set_app_handle(app);
    client.set_portal_id(portal_id);
    client.set_water_instances(Some((
        water_tank_instance,
        water_pump_instance,
        water_valve_instance,
    )));
    client.set_ev_instances(Some((ev_instance, evcharger_instance)));
    client.set_camera_topic(camera_topic);
    client.connect().map_err(|e| e.to_string())?;
    let mut client_guard = mqtt_client
        .0
        .lock()
        .map_err(|e| format!("Internal error: {}", e))?;
    *client_guard = Some(client);
    Ok(())
}

#[tauri::command]
async fn connect_gateway(
    url: String,
    access_client_id: String,
    access_client_secret: String,
    api_token: Option<String>,
    app: tauri::AppHandle,
    mqtt_client: State<'_, MqttState>,
    gateway_client: State<'_, GatewayState>,
) -> Result<(), String> {
    // Stop LAN MQTT so it cannot race remote updates.
    {
        let mut client_guard = mqtt_client
            .0
            .lock()
            .map_err(|e| format!("Internal error: {}", e))?;
        if let Some(old) = client_guard.take() {
            old.stop();
        }
    }
    {
        let mut gw = gateway_client
            .0
            .lock()
            .map_err(|e| format!("Internal error: {}", e))?;
        if let Some(old) = gw.take() {
            old.stop();
        }
        let client = gateway::start_gateway_client(
            app,
            url,
            access_client_id,
            access_client_secret,
            api_token.unwrap_or_default(),
        )?;
        *gw = Some(client);
    }
    Ok(())
}

#[tauri::command]
async fn test_ha_connection(url: String, port: Option<u16>, token: String) -> Result<(), String> {
    let client = ha_api::HaApiClient::new(&url, port, &token).await?;
    client.test_connection().await
}

#[derive(Debug, Serialize, Deserialize)]
struct GatewayHealthResult {
    status: String,
    mqtt_connected: Option<bool>,
}

/// Probe remote inverter-gateway `/health` with Cloudflare Access + optional bearer.
#[tauri::command]
async fn test_gateway_connection(
    url: String,
    access_client_id: String,
    access_client_secret: String,
    api_token: Option<String>,
) -> Result<GatewayHealthResult, String> {
    let base = url.trim().trim_end_matches('/').to_string();
    if base.is_empty() {
        return Err("Gateway URL is required".into());
    }
    if access_client_id.trim().is_empty() || access_client_secret.trim().is_empty() {
        return Err("Cloudflare Access Client ID and Secret are required".into());
    }
    let health_url = format!("{}/health", base);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let mut req = client
        .get(&health_url)
        .header("CF-Access-Client-Id", access_client_id.trim())
        .header("CF-Access-Client-Secret", access_client_secret.trim())
        .header("User-Agent", "inverter-desktop/gateway-test");
    if let Some(tok) = api_token {
        let tok = tok.trim();
        if !tok.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", tok));
        }
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;
    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| format!("Read body failed: {e}"))?;
    if !status.is_success() {
        return Err(format!(
            "HTTP {status}: {}",
            body.chars().take(200).collect::<String>()
        ));
    }
    let parsed: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("Invalid JSON: {e}; body={body}"))?;
    Ok(GatewayHealthResult {
        status: parsed
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("ok")
            .to_string(),
        mqtt_connected: parsed.get("mqtt_connected").and_then(|v| v.as_bool()),
    })
}

#[tauri::command]
async fn get_ha_appliance_states(
    url: String,
    port: Option<u16>,
    token: String,
) -> Result<Vec<ha_api::HaState>, String> {
    let client = ha_api::HaApiClient::new(&url, port, &token).await?;
    // Legacy fallback list for installs without section entity config.
    // Prefer get_ha_entity_states with configured IDs. Missing entities (404/410)
    // are killswitched in HaApiClient::get_entities so they are not polled forever.
    let entity_ids = [
        // Dishwasher
        "binary_sensor.dishwasher_running",
        "sensor.dishwasher_status",
        "switch.dishwasher",
        // Appliance states from individual sensors
        "sensor.dishwasher_duration",
        "sensor.washer_remaining_time",
        "sensor.dryer_remaining_time",
        "sensor.washer_power_estimate",
        "sensor.dryer_power_estimate",
        // Washer
        "binary_sensor.washer_running",
        "switch.washer",
        // Dryer
        "binary_sensor.dryer_running",
        "switch.dryer",
    ];
    client.get_entities(&entity_ids).await
}

#[tauri::command]
async fn get_ha_entity_states(
    url: String,
    port: Option<u16>,
    token: String,
    entity_ids: Vec<String>,
) -> Result<Vec<ha_api::HaState>, String> {
    let client = ha_api::HaApiClient::new(&url, port, &token).await?;
    let ids: Vec<&str> = entity_ids.iter().map(|s| s.as_str()).collect();
    client.get_entities(&ids).await
}

#[tauri::command]
async fn discover_ha_entities(
    url: String,
    port: Option<u16>,
    token: String,
) -> Result<Vec<DiscoveredEntity>, String> {
    let client = ha_api::HaApiClient::new(&url, port, &token).await?;
    let states = client.get_states().await?;
    let mut result = Vec::new();
    for ha_state in states {
        let entity_id = ha_state.entity_id.clone();
        let domain = entity_id.split('.').next().map(String::from);
        let friendly_name = if let Some(attrs) = &ha_state.attributes {
            attrs
                .get("friendly_name")
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or_else(|| entity_id.clone())
        } else {
            entity_id.clone()
        };
        if let Some(domain_str) = domain {
            if HA_ENTITY_DOMAINS.contains(&domain_str.as_str()) {
                result.push(DiscoveredEntity {
                    entity_id,
                    friendly_name,
                    domain: domain_str,
                    state: ha_state.state.clone(),
                });
            }
        }
    }
    Ok(result)
}

#[tauri::command]
async fn set_cover_position(
    url: String,
    port: Option<u16>,
    token: String,
    entity_id: String,
    position: u8,
) -> Result<(), String> {
    let client = ha_api::HaApiClient::new(&url, port, &token).await?;
    client.set_cover_position(&entity_id, position).await
}

#[tauri::command]
async fn open_config_window(app: tauri::AppHandle) -> Result<(), String> {
    // Already open? Bring it to front instead of failing on duplicate label.
    // (unminimize/focused are desktop-only APIs)
    #[cfg(desktop)]
    if let Some(window) = app.get_webview_window("config") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(());
    }
    #[allow(unused_mut)]
    let mut builder =
        tauri::WebviewWindowBuilder::new(&app, "config", tauri::WebviewUrl::App("config".into()))
            .title("Configuration")
            .inner_size(CONFIG_WINDOW_W, CONFIG_WINDOW_H)
            .resizable(true);
    #[cfg(desktop)]
    let builder = builder.focused(true);
    builder.build().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn close_config_window(window: tauri::Window) -> Result<(), String> {
    window.close().map_err(|e| e.to_string())
}

// === Auto-start management ===

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn get_autolaunch() -> Result<auto_launch::AutoLaunch, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe_path = exe.to_string_lossy().to_string();
    auto_launch::AutoLaunchBuilder::new()
        .set_app_name("Inverter Desktop")
        .set_app_path(&exe_path)
        .set_macos_launch_mode(auto_launch::MacOSLaunchMode::LaunchAgent)
        .build()
        .map_err(|e| format!("Failed to create auto-launch: {}", e))
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[tauri::command]
async fn set_auto_start(enable: bool) -> Result<(), String> {
    let auto = get_autolaunch()?;
    if enable {
        auto.enable()
            .map_err(|e| format!("Failed to enable auto-start: {}", e))?;
    } else {
        auto.disable()
            .map_err(|e| format!("Failed to disable auto-start: {}", e))?;
    }
    Ok(())
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[tauri::command]
async fn get_auto_start() -> Result<bool, String> {
    let auto = get_autolaunch()?;
    auto.is_enabled()
        .map_err(|e| format!("Failed to check auto-start: {}", e))
}

// Mobile stubs - auto-start not supported on mobile
#[cfg(any(target_os = "android", target_os = "ios"))]
#[tauri::command]
async fn set_auto_start(_enable: bool) -> Result<(), String> {
    Ok(())
}

#[cfg(any(target_os = "android", target_os = "ios"))]
#[tauri::command]
async fn get_auto_start() -> Result<bool, String> {
    Ok(false)
}

// === Authentication ===

use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

static AUTH_SESSIONS: LazyLock<RwLock<HashMap<String, AuthSession>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

struct AuthSession {
    #[allow(dead_code)]
    username: String,
    #[allow(dead_code)]
    created_at: std::time::Instant,
}

#[tauri::command]
async fn auth_login(
    username: String,
    password: String,
    app: tauri::AppHandle,
) -> Result<String, String> {
    let config = load_config(&app)?;
    if !config.auth_enabled.unwrap_or(false) {
        return Ok("disabled".to_string());
    }
    let expected_user = config.auth_username.as_deref().unwrap_or("");
    let expected_pass = config.auth_password.as_deref().unwrap_or("");
    if username == expected_user && password == expected_pass {
        let token = format!("sess_{}", uuid::Uuid::new_v4());
        let mut sessions = AUTH_SESSIONS.write().map_err(|e| e.to_string())?;
        sessions.insert(
            token.clone(),
            AuthSession {
                username,
                created_at: std::time::Instant::now(),
            },
        );
        Ok(token)
    } else {
        Err("Invalid credentials".to_string())
    }
}

#[tauri::command]
async fn auth_check(token: String) -> Result<bool, String> {
    let sessions = AUTH_SESSIONS.read().map_err(|e| e.to_string())?;
    Ok(sessions.contains_key(&token))
}

#[tauri::command]
async fn send_notification(
    app: tauri::AppHandle,
    title: String,
    body: String,
) -> Result<(), String> {
    use tauri_plugin_notification::NotificationExt;
    app.notification()
        .builder()
        .title(&title)
        .body(&body)
        .show()
        .map_err(|e| format!("Notification error: {}", e))?;
    Ok(())
}

#[tauri::command]
fn set_window_hidden(
    hidden: bool,
    app: tauri::AppHandle,
    mqtt_client: State<'_, MqttState>,
    ha_entity_states: State<'_, HaEntityStates>,
) {
    ha_api::WINDOW_HIDDEN.store(hidden, std::sync::atomic::Ordering::Relaxed);
    if !hidden {
        if let Ok(guard) = mqtt_client.0.lock() {
            if let Some(ref client) = *guard {
                let state = client.get_state();
                crate::mqtt::MqttClient::emit_state_update(&Some(app.clone()), &state, true);
            }
        }
        // Sensors are omitted from live ha-filtered ticks; force a full snapshot
        // (incl. sensors) whenever the window is shown again.
        ha_api::force_emit_ha_filtered(&app, &ha_entity_states.0);
    }
}

#[tauri::command]
fn get_ha_filtered_data(entity_states: tauri::State<'_, HaEntityStates>) -> ha_api::HaFilteredData {
    let guard = match entity_states.0.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    ha_api::compute_filtered_data(&guard)
}

#[tauri::command]
async fn auth_biometric_available() -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    {
        Ok(unsafe { biometric_available() })
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(false)
    }
}

#[tauri::command]
async fn auth_biometric(_app: tauri::AppHandle) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        let reason = std::ffi::CString::new("Authenticate to access Inverter Desktop")
            .map_err(|e| format!("CString error: {}", e))?;
        let ok = unsafe { biometric_authenticate(reason.as_ptr()) };
        if ok {
            let token = format!("sess_{}", uuid::Uuid::new_v4());
            let mut sessions = AUTH_SESSIONS.write().map_err(|e| e.to_string())?;
            sessions.insert(
                token.clone(),
                AuthSession {
                    username: "biometric".to_string(),
                    created_at: std::time::Instant::now(),
                },
            );
            Ok(token)
        } else {
            Err("Biometric authentication failed or was cancelled".to_string())
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("Biometric authentication is only supported on macOS".to_string())
    }
}

#[cfg(desktop)]
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};

#[tauri::command]
async fn connect_ha_mqtt(
    host: String,
    port: u16,
    username: Option<String>,
    password: Option<String>,
    camera_topic: Option<String>,
    app: tauri::AppHandle,
    mqtt_client: State<'_, HaMqttState>,
) -> Result<(), String> {
    // Drop old client first (stops its background loop)
    {
        let mut client_guard = mqtt_client
            .0
            .lock()
            .map_err(|e| format!("Internal error: {}", e))?;
        if let Some(old) = client_guard.take() {
            old.stop();
        }
    }
    let mut client = MqttClient::new(
        host,
        port,
        username,
        password,
        "inverter-dashboard-desktop-ha".to_string(),
    );
    client.set_ha_entity_states(app.state::<HaEntityStates>().0.clone());
    client.set_app_handle(app.clone());
    client.set_camera_topic(camera_topic);
    client.set_status_event("ha-mqtt-connection-status".to_string());
    client.connect().map_err(|e| e.to_string())?;
    let mut client_guard = mqtt_client
        .0
        .lock()
        .map_err(|e| format!("Internal error: {}", e))?;
    *client_guard = Some(client);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mqtt_state = MqttState(Arc::new(Mutex::new(None)));
    let ha_mqtt_state = HaMqttState(Arc::new(Mutex::new(None)));
    let gateway_state = GatewayState(Arc::new(Mutex::new(None)));

    let ha_entity_states = HaEntityStates(Arc::new(Mutex::new(HashMap::new())));

    let builder = tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                // Default TRACE from dependencies (tungstenite) floods the log
                // file with every HA WS frame and starves the UI thread/disk.
                .level(log::LevelFilter::Info)
                .level_for("tungstenite", log::LevelFilter::Warn)
                .level_for("tokio_tungstenite", log::LevelFilter::Warn)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(mqtt_state)
        .manage(ha_mqtt_state)
        .manage(gateway_state)
        .manage(ha_entity_states);

    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_window_state::Builder::new().build());

    builder
        .invoke_handler(tauri::generate_handler![
            get_state,
            perform_action,
            connect_mqtt,
            connect_gateway,
            acknowledge_victron_banner,
            connect_ha_mqtt,
            get_config,
            save_config,
            backup_config,
            restore_config,
            test_ha_connection,
            test_gateway_connection,
            get_ha_appliance_states,
            get_ha_entity_states,
            discover_ha_entities,
            set_cover_position,
            open_config_window,
            close_config_window,
            set_auto_start,
            get_auto_start,
            auth_login,
            auth_check,
            auth_biometric_available,
            auth_biometric,
            send_notification,
            set_window_hidden,
            get_ha_filtered_data
        ])
        .setup(|app| {
            // Start background HA polling
            start_ha_polling(app.handle().clone());

            #[cfg(desktop)]
            {
                // Setup app menu with About, Edit and Window menus
                let about_item =
                    MenuItem::with_id(app, "about", "About Inverter Desktop", true, None::<&str>)?;
                let app_submenu = Submenu::with_items(
                    app,
                    "Inverter Desktop",
                    true,
                    &[
                        &about_item,
                        &PredefinedMenuItem::separator(app)?,
                        &PredefinedMenuItem::quit(app, Some("Quit"))?,
                    ],
                )?;

                let edit_submenu = Submenu::with_items(
                    app,
                    "Edit",
                    true,
                    &[
                        &PredefinedMenuItem::undo(app, None)?,
                        &PredefinedMenuItem::redo(app, None)?,
                        &PredefinedMenuItem::separator(app)?,
                        &PredefinedMenuItem::cut(app, None)?,
                        &PredefinedMenuItem::copy(app, None)?,
                        &PredefinedMenuItem::paste(app, None)?,
                        &PredefinedMenuItem::select_all(app, None)?,
                    ],
                )?;

                let window_submenu = Submenu::with_items(
                    app,
                    "Window",
                    true,
                    &[
                        &PredefinedMenuItem::minimize(app, None)?,
                        &PredefinedMenuItem::separator(app)?,
                        &PredefinedMenuItem::close_window(app, None)?,
                    ],
                )?;

                let menu = Menu::with_items(app, &[&app_submenu, &edit_submenu, &window_submenu])?;
                app.set_menu(menu)?;

                // Setup system tray with configuration menu
                info!("Building system tray...");
                TrayIconBuilder::with_id("main-tray")
                    .tooltip("Inverter Desktop")
                    .icon({
                        #[cfg(target_os = "macos")]
                        {
                            let (rgba, w, h) = tray_icon::render(None, None);
                            tauri::image::Image::new_owned(rgba, w, h)
                        }
                        #[cfg(not(target_os = "macos"))]
                        {
                            let img = image::load_from_memory(include_bytes!("../icons/icon.png"))
                                .expect("Failed to load tray icon")
                                .into_rgba8();
                            let (w, h) = img.dimensions();
                            tauri::image::Image::new_owned(img.into_raw(), w, h)
                        }
                    })
                    .menu(&tauri::menu::Menu::with_items(
                        app,
                        &[
                            &tauri::menu::MenuItem::with_id(
                                app,
                                "show",
                                "Show Dashboard",
                                true,
                                None::<&str>,
                            )?,
                            &tauri::menu::MenuItem::with_id(
                                app,
                                "config",
                                "Settings...",
                                true,
                                None::<&str>,
                            )?,
                            &tauri::menu::PredefinedMenuItem::separator(app)?,
                            &tauri::menu::MenuItem::with_id(
                                app,
                                "quit",
                                "Quit",
                                true,
                                None::<&str>,
                            )?,
                        ],
                    )?)
                    .on_menu_event(|app, event| match event.id.as_ref() {
                        "show" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.unminimize();
                                let _ = window.show();
                                let _ = window.set_focus();
                                let _ = app.emit("window-shown", ());
                            }
                        }
                        "config" => {
                            let app = app.clone();
                            tauri::async_runtime::spawn(async move {
                                let _ = open_config_window(app).await;
                            });
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            ..
                        } = event
                        {
                            let app = tray.app_handle();
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.unminimize();
                                let _ = window.show();
                                let _ = window.set_focus();
                                let _ = app.emit("window-shown", ());
                            }
                        }
                    })
                    .build(app)?;
                info!("Tray icon built successfully.");

                // Background task: update tray icon with live MQTT state
                // macOS: renders custom bar-chart icon + tooltip
                // Other platforms: updates tooltip text only (no system font dependency)
                // Also monitors for critical alerts: low battery SoC, grid disconnection
                {
                    let mqtt_for_tray = app.state::<MqttState>().0.clone();
                    let app_for_tray = app.handle().clone();
                    tauri::async_runtime::spawn(async move {
                        let mut interval = tokio::time::interval(Duration::from_millis(1500));
                        // After sleep/wake, don't fire a burst of catch-up renders.
                        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                        // Track notification state to avoid spam
                        let mut low_battery_notified = false;
                        // One tray update. The loop below catches panics so a single
                        // bad tick can't silently kill the whole tray task.
                        let mut update_tray = || {
                            let state = {
                                // Poisoned lock is not a broken client: the state is a
                                // plain snapshot, keep going instead of skipping forever.
                                let guard = mqtt_for_tray.lock().unwrap_or_else(|p| p.into_inner());
                                guard.as_ref().map(|c| c.get_state())
                            };
                            if let Some(s) = state {
                                let solar = s.solar_total.unwrap_or(0.0) / 1000.0;
                                let batt = s.battery_soc.unwrap_or(0.0);
                                let grid_reading = s.gt.map(|v| v / 1000.0);
                                let grid = grid_reading.unwrap_or(0.0);
                                let tip = format!(
                                    "PV {:.1}kW  Battery {:.0}%  Grid {:+.1}kW",
                                    solar, batt, grid
                                );
                                if let Some(tray) = app_for_tray.tray_by_id("main-tray") {
                                    #[cfg(target_os = "macos")]
                                    {
                                        let (rgba, w, h) = tray_icon::render(s.solar_total, s.gt);
                                        let tauri_img = tauri::image::Image::new_owned(rgba, w, h);
                                        let _ = tray.set_title(None::<&str>);
                                        if let Err(e) = tray.set_icon(Some(tauri_img)) {
                                            log::warn!("Tray set_icon failed: {e}");
                                        }
                                    }
                                    if let Err(e) = tray.set_tooltip(Some(&tip)) {
                                        log::warn!("Tray set_tooltip failed: {e}");
                                    }
                                }

                                // Check for critical alerts
                                use tauri_plugin_notification::NotificationExt;

                                // Low battery alert (< 20%)
                                if batt > 0.0 && batt < 20.0 {
                                    if !low_battery_notified {
                                        let _ = app_for_tray
                                            .notification()
                                            .builder()
                                            .title("Inverter Desktop - Low Battery")
                                            .body(format!("Battery SoC dropped to {:.0}%!", batt))
                                            .show();
                                        low_battery_notified = true;
                                    }
                                } else {
                                    low_battery_notified = false;
                                }
                            }
                        };
                        loop {
                            interval.tick().await;
                            if let Err(p) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                                &mut update_tray,
                            )) {
                                let msg = p
                                    .downcast_ref::<String>()
                                    .map(String::as_str)
                                    .or_else(|| p.downcast_ref::<&str>().copied())
                                    .unwrap_or("unknown panic");
                                log::error!("Tray update tick panicked: {msg}");
                            }
                        }
                    });
                }
            }

            // Attempt to connect MQTT or remote gateway if configured
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Ok(config) = load_config(&app_handle) {
                    let connect_handle = app_handle.clone();
                    let mqtt_state = app_handle.state::<MqttState>();
                    let gateway_state = app_handle.state::<GatewayState>();
                    let gateway_url = config
                        .gateway_url
                        .as_deref()
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    let access_id = config.gateway_access_client_id.clone().unwrap_or_default();
                    let access_secret = config
                        .gateway_access_client_secret
                        .clone()
                        .unwrap_or_default();
                    if config.gateway_enabled
                        && !gateway_url.is_empty()
                        && !access_id.trim().is_empty()
                        && !access_secret.trim().is_empty()
                    {
                        let _ = connect_gateway(
                            gateway_url,
                            access_id,
                            access_secret,
                            config.gateway_api_token.clone(),
                            connect_handle,
                            mqtt_state,
                            gateway_state,
                        )
                        .await;
                        return;
                    }
                    let host = config.mqtt_host.trim().to_string();
                    if !host.is_empty() {
                        let port = if config.mqtt_port == 0 {
                            DEFAULT_MQTT_PORT
                        } else {
                            config.mqtt_port
                        };
                        let username = config.mqtt_login.clone();
                        let password = config.mqtt_password.clone();
                        let portal_id = config.portal_id.clone();
                        let water_tank_instance = config.water_tank_instance;
                        let water_pump_instance = config.water_pump_instance;
                        let water_valve_instance = config.water_valve_instance;
                        let evcharger_instance = config.evcharger_instance.or(Some(40));
                        let ev_instance = config.ev_instance.or(Some(22));
                        let camera_topic = config.camera_topic.clone();
                        let _ = connect_mqtt(
                            host,
                            port,
                            username,
                            password,
                            portal_id,
                            water_tank_instance,
                            water_pump_instance,
                            water_valve_instance,
                            evcharger_instance,
                            ev_instance,
                            camera_topic,
                            connect_handle,
                            mqtt_state,
                            gateway_state,
                        )
                        .await;
                    }
                }
            });

            // Show window on startup
            info!("Showing main window...");
            let window = app.get_webview_window("main").unwrap();

            #[cfg(desktop)]
            {
                // Close / Minimize → hide (keep app running in menu bar) + notification
                let window_hide = window.clone();
                let app_handle_hide = app.handle().clone();
                window.on_window_event(move |event| match event {
                    WindowEvent::CloseRequested { api, .. } => {
                        api.prevent_close();
                        let _ = window_hide.hide();
                        let _ = app_handle_hide.emit("window-hidden", ());
                        use tauri_plugin_notification::NotificationExt;
                        let _ = app_handle_hide
                            .notification()
                            .builder()
                            .title("Inverter Desktop")
                            .body("Continuing to work in background, minimized to tray")
                            .show();
                    }
                    WindowEvent::Focused(false) => {
                        // macOS: Accessory mode has no dock icon, so minimize to dock is useless.
                        // Convert minimize to hide-to-tray instead.
                        #[cfg(target_os = "macos")]
                        if let Ok(true) = window_hide.is_minimized() {
                            let _ = window_hide.unminimize();
                            let _ = window_hide.hide();
                            let _ = app_handle_hide.emit("window-hidden", ());
                            use tauri_plugin_notification::NotificationExt;
                            let _ = app_handle_hide
                                .notification()
                                .builder()
                                .title("Inverter Desktop")
                                .body("Continuing to work in background, minimized to tray")
                                .show();
                        }
                        let _ = app_handle_hide.emit("window-blurred", ());
                    }
                    WindowEvent::Focused(true) => {
                        let _ = app_handle_hide.emit("window-focused", ());
                    }
                    _ => {}
                });
            }

            window.show().unwrap();

            // macOS: accessory mode keeps app in menu bar (tray icon visible) without dock icon
            #[cfg(target_os = "macos")]
            {
                info!("Setting activation policy to Accessory...");
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }

            #[cfg(desktop)]
            {
                // Handle app menu events
                app.on_menu_event(move |app_handle, event| {
                    if event.id.as_ref() == "about" {
                        // Already open? Bring it to front instead of failing on duplicate label.
                        if let Some(window) = app_handle.get_webview_window("about") {
                            let _ = window.unminimize();
                            let _ = window.show();
                            let _ = window.set_focus();
                        } else {
                            let _ = tauri::WebviewWindowBuilder::new(
                                app_handle,
                                "about",
                                tauri::WebviewUrl::App("about".into()),
                            )
                            .title("About Inverter Desktop")
                            .inner_size(ABOUT_WINDOW_W, ABOUT_WINDOW_H)
                            .resizable(false)
                            .center()
                            .focused(true)
                            .build();
                        }
                    }
                });
            }

            info!("Setup block completed successfully.");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod perform_action_tests {
    use super::*;

    fn client_with_flags(flags: impl IntoIterator<Item = (&'static str, bool)>) -> MqttClient {
        let c = MqttClient::new("localhost".into(), 1883, None, None, "test".into());
        {
            let mut st = c.state.lock().unwrap();
            st.booleans = Some(flags.into_iter().map(|(k, v)| (k.into(), v)).collect());
        }
        c
    }

    #[test]
    fn stamp_toggle_adds_state_for_inverter_control_flag() {
        let client = client_with_flags([("only_charging", true)]);
        let mut payload = serde_json::json!({"entity": "only_charging"});
        stamp_toggle_state(&mut payload, &client, "toggle");
        assert_eq!(payload.get("state").and_then(|v| v.as_str()), Some("off"));
    }

    #[test]
    fn stamp_toggle_removes_flag_when_off() {
        let client = client_with_flags([("house_support", false)]);
        let mut payload = serde_json::json!({"entity": "house_support"});
        stamp_toggle_state(&mut payload, &client, "toggle");
        assert_eq!(payload.get("state").and_then(|v| v.as_str()), Some("on"));
    }

    #[test]
    fn stamp_toggle_handles_input_boolean_prefix() {
        let client = client_with_flags([("no_feed", false)]);
        let mut payload = serde_json::json!({"entity": "input_boolean.no_feed"});
        stamp_toggle_state(&mut payload, &client, "toggle");
        assert_eq!(payload.get("state").and_then(|v| v.as_str()), Some("on"));
    }

    #[test]
    fn stamp_toggle_ignores_non_toggle_actions() {
        let client = client_with_flags([("only_charging", true)]);
        let mut payload = serde_json::json!({"entity": "only_charging"});
        stamp_toggle_state(&mut payload, &client, "press");
        assert!(payload.get("state").is_none());
    }

    #[test]
    fn stamp_toggle_ignores_non_flag_entities() {
        let client = client_with_flags([("only_charging", true)]);
        let mut payload = serde_json::json!({"entity": "switch.garage"});
        stamp_toggle_state(&mut payload, &client, "toggle");
        assert!(payload.get("state").is_none());
    }

    #[test]
    fn stamp_toggle_defaults_to_on_when_unknown() {
        let client = MqttClient::new("localhost".into(), 1883, None, None, "test".into());
        let mut payload = serde_json::json!({"entity": "only_charging"});
        stamp_toggle_state(&mut payload, &client, "toggle");
        assert_eq!(payload.get("state").and_then(|v| v.as_str()), Some("on"));
    }

    #[test]
    fn is_inverter_control_flag_accepts_bare_key() {
        assert!(is_inverter_control_flag("only_charging"));
        assert!(is_inverter_control_flag("no_feed"));
        assert!(!is_inverter_control_flag("switch.garage"));
    }

    #[test]
    fn is_inverter_control_flag_accepts_input_boolean_prefix() {
        assert!(is_inverter_control_flag("input_boolean.only_charging"));
        assert!(is_inverter_control_flag("input_boolean.house_support"));
        assert!(!is_inverter_control_flag("input_boolean.garage"));
    }
}
