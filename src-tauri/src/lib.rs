pub mod mqtt;

use mqtt::{MqttClient, InverterState};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
//use std::path::PathBuf;
use tauri::{Manager, State};
use tauri_plugin_store::StoreExt;

// Global state for the MQTT client
type MqttState = Arc<Mutex<Option<MqttClient>>>;

#[derive(Debug, Serialize, Deserialize, Clone)]
struct FullConfig {
    mqtt_host: String,
    mqtt_port: u16,
    mqtt_login: Option<String>,
    mqtt_password: Option<String>,
    ha_longlived_token: Option<String>,
    color_scheme: Option<String>,
    // keep existing fields
    ha_boolean_entities: Option<serde_json::Value>,
    ha_switch_entities: Option<serde_json::Value>,
    ha_water_valve_entity: Option<String>,
    ha_pump_switch_entity: Option<String>,
    header_toggles: Option<serde_json::Value>,
}

impl Default for FullConfig {
    fn default() -> Self {
        Self {
            mqtt_host: "192.168.160.150".to_string(),
            mqtt_port: 1883,
            mqtt_login: None,
            mqtt_password: None,
            ha_longlived_token: None,
            color_scheme: Some("dark".to_string()),
            ha_boolean_entities: None,
            ha_switch_entities: None,
            ha_water_valve_entity: None,
            ha_pump_switch_entity: None,
            header_toggles: None,
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
            save_config
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
