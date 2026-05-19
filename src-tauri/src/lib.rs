pub mod mqtt;
mod ha_api;
use ha_api::HaState;

use mqtt::{MqttClient, InverterState, HeaderToggle};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
//use std::path::PathBuf;
use tauri::{Manager, State};
use tauri_plugin_store::StoreExt;

#[derive(Debug, Serialize, Deserialize, Clone)]
struct DiscoveredEntity {
    entity_id: String,
    friendly_name: String,
    domain: String,
}


// Global state for the MQTT client
type MqttState = Arc<Mutex<Option<MqttClient>>>;

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
    ha_longlived_token: Option<String>,
    ha_url: Option<String>,
    ha_port: Option<u16>,
    ha_use_direct_api: bool,
    color_scheme: Option<String>,
    // keep existing fields for backward compatibility
    ha_boolean_entities: Option<serde_json::Value>,
    ha_switch_entities: Option<serde_json::Value>,
    ha_water_valve_entity: Option<String>,
    ha_pump_switch_entity: Option<String>,
    header_toggles: Option<serde_json::Value>,
    // new unified entities config
    ha_entities: Option<Vec<HaEntityConfig>>,
    header_toggles_config: Option<Vec<HeaderToggle>>,
}

impl Default for FullConfig {
    fn default() -> Self {
        Self {
            mqtt_host: "192.168.160.150".to_string(),
            mqtt_port: 1883,
            mqtt_login: None,
            mqtt_password: None,
            ha_longlived_token: None,
            ha_url: None,
            ha_port: None,
            ha_use_direct_api: false,
            color_scheme: Some("dark".to_string()),
            ha_boolean_entities: None,
            ha_switch_entities: None,
            ha_water_valve_entity: None,
            ha_pump_switch_entity: None,
            header_toggles: None,
            ha_entities: None,
            header_toggles_config: None,
        }
    }
}

#[tauri::command]
fn get_state(mqtt_client: State<MqttState>) -> Result<InverterState, String> {
    let client = mqtt_client.lock().unwrap();
    if let Some(ref client) = *client {
        Ok(client.get_state())
    } else {
        Err("MQTT client not connected".to_string())
    }
}

#[tauri::command]
fn send_command(
    action: String,
    payload: serde_json::Value,
    mqtt_client: State<MqttState>
) -> Result<(), String> {
    let client = mqtt_client.lock().unwrap();
    if let Some(ref client) = *client {
        client.publish_command(&action, payload).map_err(|e| e.to_string())
    } else {
        Err("MQTT client not connected".to_string())
    }
}

#[tauri::command]
fn get_config(app: tauri::AppHandle) -> Result<FullConfig, String> {
    let store = app.store_builder("config.json")
        .build()
        .map_err(|e| format!("Failed to build store: {}", e))?;

    match store.get("config") {
        Some(value) => {
            let config: FullConfig = serde_json::from_value(value)
                .map_err(|e| format!("Failed to parse config: {}", e))?;
            Ok(config)
        }
        None => Ok(FullConfig::default()),
    }
}

#[tauri::command]
fn save_config(app: tauri::AppHandle, config: FullConfig) -> Result<(), String> {
    let store = app.store_builder("config.json")
        .build()
        .map_err(|e| format!("Failed to build store: {}", e))?;

    store.set("config", serde_json::to_value(&config).map_err(|e| e.to_string())?);
    store.save().map_err(|e| format!("Failed to save config: {}", e))?;

    Ok(())
}

#[tauri::command]
fn connect_mqtt(
    host: String,
    port: u16,
    mqtt_client: State<MqttState>
) -> Result<(), String> {
    let mut client_guard = mqtt_client.lock().unwrap();
    let mut client = MqttClient::new(host, port);
    client.connect().map_err(|e| e.to_string())?;
    *client_guard = Some(client);
    Ok(())
}

#[tauri::command]
fn disconnect_mqtt(mqtt_client: State<MqttState>) -> Result<(), String> {
    let mut client_guard = mqtt_client.lock().unwrap();
    *client_guard = None;
    Ok(())
}

#[tauri::command]
async fn test_ha_connection(
    url: String,
    port: Option<u16>,
    token: String,
) -> Result<(), String> {
    let client = ha_api::HaApiClient::new(&url, port, &token).await?;
    client.test_connection().await
}

#[tauri::command]
async fn get_ha_states(
    url: String,
    port: Option<u16>,
    token: String,
) -> Result<Vec<ha_api::HaState>, String> {
    let client = ha_api::HaApiClient::new(&url, port, &token).await?;
    client.get_states().await
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
        "switch", "light", "input_boolean", "fan", "cover", "lock", "media_player",
        "scene", "script", "number", "sensor", "binary_sensor"
    ];
    let mut result = Vec::new();
    for state in states {
        if let Some(domain) = state.entity_id.split('.').next() {
            if togglable.contains(&domain) {
                let friendly_name = state.attributes
                    .and_then(|attrs| attrs.get("friendly_name"))
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .unwrap_or_else(|| state.entity_id.clone());
                result.push(DiscoveredEntity {
                    entity_id: state.entity_id,
                    friendly_name,
                    domain: domain.to_string(),
                });
            }
        }
    }
    Ok(result)
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
        "switch", "light", "input_boolean", "fan", "cover", "lock", "media_player",
        "scene", "script", "number", "sensor", "binary_sensor"
    ];
    let mut result = Vec::new();
    for state in states {
        if let Some(domain) = state.entity_id.split('.').next() {
            if togglable.contains(&domain) {
                let friendly_name = state.attributes
                    .and_then(|attrs| attrs.get("friendly_name"))
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .unwrap_or_else(|| state.entity_id.clone());
                result.push(DiscoveredEntity {
                    entity_id: state.entity_id,
                    friendly_name,
                    domain: domain.to_string(),
                });
            }
        }
    }
    Ok(result)
}

#[tauri::command]
async fn toggle_ha_entity(
    url: String,
    port: Option<u16>,
    token: String,
    entity_id: String,
) -> Result<(), String> {
    let client = ha_api::HaApiClient::new(&url, port, &token).await?;
    let states = client.get_states().await?;
    let state_opt = states.iter().find(|s| s.entity_id == entity_id);
    match state_opt {
        Some(s) => {
            if s.state == "on" {
                client.turn_off(&entity_id).await
            } else {
                client.turn_on(&entity_id).await
            }
        }
        None => Err(format!("Entity {} not found", entity_id)),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mqtt_state: MqttState = Arc::new(Mutex::new(None));

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_window_state::Builder::new().build())
        .plugin(tauri_plugin_store::Builder::new().build())
        .manage(mqtt_state)
        .invoke_handler(tauri::generate_handler![
            get_state,
            send_command,
            connect_mqtt,
            disconnect_mqtt,
            get_config,
            save_config,
            test_ha_connection,
            get_ha_states,
            discover_ha_entities,
            toggle_ha_entity
        ])
        .setup(|app| {
            // Setup system tray with configuration menu
            let _tray = tauri::tray::TrayIconBuilder::new()
                //.icon(app.icon())
        .menu(&tauri::menu::Menu::with_items(app, &[
                    &tauri::menu::MenuItem::with_id(app, "show", "Show", true, None::<&str>)?,
                    &tauri::menu::MenuItem::with_id(app, "hide", "Hide", true, None::<&str>)?,
                    &tauri::menu::MenuItem::with_id(app, "config", "Configuration", true, None::<&str>)?,
                    &tauri::menu::MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?,
                ])?)
                .on_menu_event(|app, event| {
                    match event.id.as_ref() {
                        "show" => {
                            let window = app.get_webview_window("main").unwrap();
                            window.show().unwrap();
                            window.set_focus().unwrap();
                        }
                        "hide" => {
                            let window = app.get_webview_window("main").unwrap();
                            window.hide().unwrap();
                        }
                        "config" => {
                            let _window = tauri::WebviewWindowBuilder::new(
                                app,
                                "config",
                                tauri::WebviewUrl::App("config".into())
                            )
                            .title("Configuration")
                            .inner_size(400.0, 500.0)
                            .resizable(true)
                            .build()
                            .unwrap();
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .build(app)?;

            // Show window on startup
            let window = app.get_webview_window("main").unwrap();
            window.show().unwrap();

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
