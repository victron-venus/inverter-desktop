mod ha_api;
pub mod mqtt;
#[cfg(target_os = "macos")]
mod tray_icon;

#[cfg(target_os = "macos")]
extern "C" {
    fn biometric_available() -> bool;
    fn biometric_authenticate(reason: *const std::os::raw::c_char) -> bool;
}

use log::{info, warn};
use mqtt::{HeaderToggle, InverterState, MqttClient};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const DEFAULT_MQTT_HOST: &str = "Cerbo";
const DEFAULT_MQTT_PORT: u16 = 1883;
const DEFAULT_HA_PORT: u16 = 8123;
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
struct HaMqttState(Arc<Mutex<Option<MqttClient>>>);

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
    color_scheme: Option<String>,
    // unified entities config
    ha_entities: Option<Vec<HaEntityConfig>>,
    header_toggles_config: Option<Vec<HeaderToggle>>,
    portal_id: Option<String>,
    camera_topic: Option<String>,
    camera_enabled: bool,
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
            color_scheme: Some("dark".to_string()),
            ha_entities: None,
            header_toggles_config: None,
            portal_id: None,
            camera_topic: Some("frigate/+/events".to_string()),
            camera_enabled: false,
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
        }
    }
}

#[tauri::command]
fn get_state(mqtt_client: State<MqttState>) -> Result<InverterState, String> {
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

#[tauri::command]
async fn perform_action(
    action: String,
    payload: serde_json::Value,
    app: tauri::AppHandle,
    mqtt_client: State<'_, MqttState>,
) -> Result<(), String> {
    info!("perform_action: action={}, payload={}", action, payload);
    let store = app
        .store_builder("config.json")
        .build()
        .map_err(|e| format!("Failed to build store: {}", e))?;

    let config: FullConfig = match store.get("config") {
        Some(value) => serde_json::from_value(value).unwrap_or_default(),
        None => FullConfig::default(),
    };

    let entity_id = payload.get("entity").and_then(|v| v.as_str());

    // Always try HA path for known entity domains if HA is configured
    if config.ha_use_direct_api && config.ha_url.is_some() && config.ha_longlived_token.is_some() {
        if let Some(entity) = entity_id {
            let domain = entity.split('.').next().unwrap_or("");
            // For switch/input_boolean/light entities, always prefer HA API
            if is_ha_entity(entity) {
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
                                log::warn!(
                                    "Entity {} not found in HA, falling back to MQTT",
                                    entity
                                );
                                let mqtt_client = mqtt_client
                                    .0
                                    .lock()
                                    .map_err(|e| format!("Lock error: {}", e))?;
                                if let Some(ref c) = *mqtt_client {
                                    c.publish_command(&action, payload.clone())
                                        .map_err(|e| format!("MQTT error: {}", e))?;
                                }
                                return Ok(());
                            }
                        }
                    }
                }
                return Ok(());
            }
        }
    }

    info!("perform_action: MQTT fallback for action={}", action);
    let client = mqtt_client
        .0
        .lock()
        .map_err(|e| format!("Internal error: {}", e))?;
    if let Some(ref client) = *client {
        client
            .publish_command(&action, payload)
            .map_err(|e| e.to_string())
    } else {
        Err("MQTT client not connected".to_string())
    }
}

fn is_ha_entity(entity_id: &str) -> bool {
    let domains = [
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
    ];
    let domain = entity_id.split('.').next().unwrap_or("");
    domains.contains(&domain)
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
            // Skip reconnecting when window is hidden to save CPU/battery
            if ha_api::WINDOW_HIDDEN.load(std::sync::atomic::Ordering::Relaxed) {
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }

            let config = {
                let store = app.store_builder("config.json").build();
                match store {
                    Ok(s) => match s.get("config") {
                        Some(v) => serde_json::from_value::<FullConfig>(v).unwrap_or_default(),
                        None => FullConfig::default(),
                    },
                    Err(_) => FullConfig::default(),
                }
            };

            if !config.ha_use_direct_api
                || config.ha_url.is_none()
                || config.ha_longlived_token.is_none()
            {
                let _ = app.emit("ha-connection-status", false);
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }

            let base = config.ha_url.clone().unwrap();
            let token = config.ha_longlived_token.clone().unwrap();
            let ws_url = build_ws_url(&base, config.ha_port);

            info!("HA WS connecting to {}", ws_url);
            match ha_api::HaWebSocketClient::connect(&ws_url, &token, app.clone()).await {
                Ok(mut ws_client) => {
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
    let store = app
        .store_builder("config.json")
        .build()
        .map_err(|e| format!("Failed to build store: {}", e))?;

    let mut is_first_run = false;
    let mut config = match store.get("config") {
        Some(value) => serde_json::from_value(value).unwrap_or_default(),
        None => {
            is_first_run = true;
            FullConfig::default()
        }
    };

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
        store.set(
            "config",
            serde_json::to_value(&config).map_err(|e| e.to_string())?,
        );
        store
            .save()
            .map_err(|e| format!("Failed to save config: {}", e))?;
    }

    Ok(config)
}

#[tauri::command]
async fn save_config(app: tauri::AppHandle, config: FullConfig) -> Result<(), String> {
    let store = app
        .store_builder("config.json")
        .build()
        .map_err(|e| format!("Failed to build store: {}", e))?;

    store.set(
        "config",
        serde_json::to_value(&config).map_err(|e| e.to_string())?,
    );
    store
        .save()
        .map_err(|e| format!("Failed to save config: {}", e))?;

    Ok(())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn connect_mqtt(
    host: String,
    port: u16,
    username: Option<String>,
    password: Option<String>,
    portal_id: Option<String>,
    camera_topic: Option<String>,
    app: tauri::AppHandle,
    mqtt_client: State<'_, MqttState>,
) -> Result<(), String> {
    let mut client_guard = mqtt_client
        .0
        .lock()
        .map_err(|e| format!("Internal error: {}", e))?;
    let mut client = MqttClient::new(host, port, username, password);
    client.set_app_handle(app);
    client.set_portal_id(portal_id);
    client.set_camera_topic(camera_topic);
    client.connect().map_err(|e| e.to_string())?;
    *client_guard = Some(client);
    Ok(())
}

#[tauri::command]
async fn test_ha_connection(url: String, port: Option<u16>, token: String) -> Result<(), String> {
    let client = ha_api::HaApiClient::new(&url, port, &token).await?;
    client.test_connection().await
}

#[tauri::command]
async fn get_ha_appliance_states(
    url: String,
    port: Option<u16>,
    token: String,
) -> Result<Vec<ha_api::HaState>, String> {
    let client = ha_api::HaApiClient::new(&url, port, &token).await?;
    let entity_ids = [
        "binary_sensor.dishwasher_running",
        "sensor.dishwasher_duration",
        "sensor.washer_remaining_time",
        "sensor.dryer_remaining_time",
        "sensor.washer_power_estimate",
        "sensor.dryer_power_estimate",
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
    let togglable = [
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
    ];
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
            if togglable.contains(&domain_str.as_str()) {
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
    tauri::WebviewWindowBuilder::new(&app, "config", tauri::WebviewUrl::App("config".into()))
        .title("Configuration")
        .inner_size(CONFIG_WINDOW_W, CONFIG_WINDOW_H)
        .resizable(true)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn close_config_window(window: tauri::Window) -> Result<(), String> {
    window.close().map_err(|e| e.to_string())
}

// === Auto-start management ===

#[tauri::command]
async fn set_auto_start(enable: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let exe_path = exe.to_string_lossy().to_string();
        let label = "com.victron.inverter-desktop";
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>"#,
            label, exe_path
        );
        let home = std::env::var("HOME").map_err(|e| e.to_string())?;
        let launch_agents = format!("{}/Library/LaunchAgents", home);
        std::fs::create_dir_all(&launch_agents).map_err(|e| e.to_string())?;
        let plist_path = format!("{}/{}.plist", launch_agents, label);
        if enable {
            std::fs::write(&plist_path, plist).map_err(|e| e.to_string())?;
        } else {
            let _ = std::fs::remove_file(&plist_path);
        }
    }
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::*;
        use winreg::RegKey;
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let exe_path = exe.to_string_lossy().to_string();
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let path = r#"Software\Microsoft\Windows\CurrentVersion\Run"#;
        let (key, _) = hkcu.create_subkey(path).map_err(|e| e.to_string())?;
        if enable {
            key.set_value("InverterDesktop", &exe_path)
                .map_err(|e| e.to_string())?;
        } else {
            let _ = key.delete_value("InverterDesktop");
        }
    }
    #[cfg(target_os = "linux")]
    {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let exe_path = exe.to_string_lossy().to_string();
        let home = std::env::var("HOME").map_err(|e| e.to_string())?;
        let autostart_dir = format!("{}/.config/autostart", home);
        std::fs::create_dir_all(&autostart_dir).map_err(|e| e.to_string())?;
        let desktop_path = format!("{}/inverter-desktop.desktop", autostart_dir);
        if enable {
            let desktop = format!(
                r#"[Desktop Entry]
Type=Application
Name=Inverter Desktop
Exec={}
Terminal=false
X-GNOME-Autostart-enabled=true"#,
                exe_path
            );
            std::fs::write(&desktop_path, desktop).map_err(|e| e.to_string())?;
        } else {
            let _ = std::fs::remove_file(&desktop_path);
        }
    }
    Ok(())
}

#[tauri::command]
async fn get_auto_start() -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").map_err(|e| e.to_string())?;
        let plist_path = format!(
            "{}/Library/LaunchAgents/com.victron.inverter-desktop.plist",
            home
        );
        return Ok(std::path::Path::new(&plist_path).exists());
    }
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::*;
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let path = r#"Software\Microsoft\Windows\CurrentVersion\Run"#;
        let key = hkcu.open_subkey(path).map_err(|e| e.to_string())?;
        return Ok(key.get_value::<String, _>("InverterDesktop").is_ok());
    }
    #[cfg(target_os = "linux")]
    {
        let home = std::env::var("HOME").map_err(|e| e.to_string())?;
        let desktop_path = format!("{}/.config/autostart/inverter-desktop.desktop", home);
        return Ok(std::path::Path::new(&desktop_path).exists());
    }
    #[allow(unreachable_code)]
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
    let store = app
        .store_builder("config.json")
        .build()
        .map_err(|e| e.to_string())?;
    let config: FullConfig = match store.get("config") {
        Some(v) => serde_json::from_value(v).unwrap_or_default(),
        None => FullConfig::default(),
    };
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
fn set_window_hidden(hidden: bool) {
    ha_api::WINDOW_HIDDEN.store(hidden, std::sync::atomic::Ordering::Relaxed);
    if hidden {
        // Signal the HA WS read loop to stop (will be checked on next select! cycle)
        ha_api::HA_WS_SHUTDOWN.store(true, std::sync::atomic::Ordering::Relaxed);
    }
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
        let reason = std::ffi::CString::new("Authenticate to access Inverter Dashboard")
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
        *client_guard = None;
    }
    let mut client = MqttClient::new(host, port, username, password);
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

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .manage(mqtt_state)
        .manage(ha_mqtt_state);

    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_window_state::Builder::new().build());

    builder
        .invoke_handler(tauri::generate_handler![
            get_state,
            perform_action,
            connect_mqtt,
            connect_ha_mqtt,
            get_config,
            save_config,
            test_ha_connection,
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
            set_window_hidden
        ])
        .setup(|app| {
            // Start background HA polling
            start_ha_polling(app.handle().clone());

            #[cfg(desktop)]
            {
                // Setup app menu with About, Edit and Window menus
                let about_item = MenuItem::with_id(
                    app,
                    "about",
                    "About Inverter Dashboard",
                    true,
                    None::<&str>,
                )?;
                let app_submenu = Submenu::with_items(
                    app,
                    "Inverter Dashboard",
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
                    .tooltip("Inverter Dashboard")
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
                {
                    let mqtt_for_tray = app.state::<MqttState>().0.clone();
                    let app_for_tray = app.handle().clone();
                    tauri::async_runtime::spawn(async move {
                        let mut interval = tokio::time::interval(Duration::from_millis(1500));
                        loop {
                            interval.tick().await;
                            let state = {
                                let guard = mqtt_for_tray.lock().ok();
                                guard.and_then(|g| g.as_ref().map(|c| c.get_state()))
                            };
                            if let Some(s) = state {
                                let solar = s.solar_total.unwrap_or(0.0) / 1000.0;
                                let batt = s.battery_soc.unwrap_or(0.0);
                                let grid = s.gt.unwrap_or(0.0) / 1000.0;
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
                                        let _ = tray.set_icon(Some(tauri_img));
                                    }
                                    let _ = tray.set_tooltip(Some(&tip));
                                }
                            }
                        }
                    });
                }
            }

            // Show window on startup
            info!("Showing main window...");
            let window = app.get_webview_window("main").unwrap();

            #[cfg(desktop)]
            {
                // Close → hide (keep app running in menu bar)
                let window_hide = window.clone();
                let app_handle_hide = app.handle().clone();
                window.on_window_event(move |event| match event {
                    WindowEvent::CloseRequested { api, .. } => {
                        api.prevent_close();
                        let _ = window_hide.hide();
                        let _ = app_handle_hide.emit("window-hidden", ());
                    }
                    WindowEvent::Focused(false) => {
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
                        let _ = tauri::WebviewWindowBuilder::new(
                            app_handle,
                            "about",
                            tauri::WebviewUrl::App("about".into()),
                        )
                        .title("About Inverter Dashboard")
                        .inner_size(ABOUT_WINDOW_W, ABOUT_WINDOW_H)
                        .resizable(false)
                        .center()
                        .build();
                    }
                });
            }

            info!("Setup block completed successfully.");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
