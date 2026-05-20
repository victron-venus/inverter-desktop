pub mod mqtt;
mod ha_api;

use mqtt::{MqttClient, InverterState, HeaderToggle};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

const DEFAULT_MQTT_HOST: &str = "Cerbo";

use tauri::{Manager, State};
use tauri::menu::{Menu, Submenu, MenuItem, PredefinedMenuItem};
use tauri_plugin_store::StoreExt;

#[derive(Debug, Serialize, Deserialize, Clone)]
struct DiscoveredEntity {
    entity_id: String,
    friendly_name: String,
    domain: String,
    state: String,
}


// Global state for the MQTT client
type MqttState = Arc<Mutex<Option<MqttClient>>>;

#[allow(dead_code)]
struct AppTrayIcon(tauri::tray::TrayIcon);

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
    // keep existing fields for backward compatibility
    ha_boolean_entities: Option<serde_json::Value>,
    ha_switch_entities: Option<serde_json::Value>,
    ha_water_valve_entity: Option<String>,
    ha_pump_switch_entity: Option<String>,
    header_toggles: Option<serde_json::Value>,
    // new unified entities config
    ha_entities: Option<Vec<HaEntityConfig>>,
    header_toggles_config: Option<Vec<HeaderToggle>>,
    portal_id: Option<String>,
}

impl Default for FullConfig {
    fn default() -> Self {
        Self {
            mqtt_host: DEFAULT_MQTT_HOST.to_string(),
            mqtt_port: 1883,
            mqtt_login: None,
            mqtt_password: None,
            mqtt_ha_host: Some(DEFAULT_MQTT_HOST.to_string()),
            mqtt_ha_port: Some(1883),
            mqtt_ha_login: None,
            mqtt_ha_password: None,
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
            portal_id: None,
        }
    }
}

#[tauri::command]
fn get_state(mqtt_client: State<MqttState>) -> Result<InverterState, String> {
    let client = mqtt_client.lock()
        .map_err(|e| format!("Internal error: {}", e))?;
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
    let client = mqtt_client.lock()
        .map_err(|e| format!("Internal error: {}", e))?;
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

    let mut config = match store.get("config") {
        Some(value) => serde_json::from_value(value)
            .unwrap_or_default(),
        None => FullConfig::default(),
    };

    let mut changed = false;

    // Auto-fill HA Long-lived Token from env if not set
    if config.ha_longlived_token.as_ref().map_or(true, |s| s.is_empty()) {
        if let Ok(token) = std::env::var("HA_TOKEN") {
            if !token.is_empty() {
                config.ha_longlived_token = Some(token);
                changed = true;
            }
        }
    }

    // Auto-fill MQTT HA credentials from env if not set
    if config.mqtt_ha_login.as_ref().map_or(true, |s| s.is_empty()) {
        if let Ok(user) = std::env::var("HA_MQTT_USER") {
            if !user.is_empty() {
                config.mqtt_ha_login = Some(user);
                changed = true;
            }
        }
    }
    if config.mqtt_ha_password.as_ref().map_or(true, |s| s.is_empty()) {
        if let Ok(pwd) = std::env::var("HA_MQTT_PWD") {
            if !pwd.is_empty() {
                config.mqtt_ha_password = Some(pwd);
                changed = true;
            }
        }
    }

    // Default ports
    if config.ha_port.is_none() {
        config.ha_port = Some(8123);
        changed = true;
    }
    if config.mqtt_port == 0 {
        config.mqtt_port = 1883;
        changed = true;
    }
    if config.mqtt_ha_port.is_none() {
        config.mqtt_ha_port = Some(1883);
        changed = true;
    }

    if changed {
        store.set("config", serde_json::to_value(&config).map_err(|e| e.to_string())?);
        store.save().map_err(|e| format!("Failed to save config: {}", e))?;
    }

    Ok(config)
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
    portal_id: Option<String>,
    app: tauri::AppHandle,
    mqtt_client: State<MqttState>
) -> Result<(), String> {
    let mut client_guard = mqtt_client.lock()
        .map_err(|e| format!("Internal error: {}", e))?;
    let mut client = MqttClient::new(host, port);
    client.set_app_handle(app);
    client.set_portal_id(portal_id);
    client.connect().map_err(|e| e.to_string())?;
    *client_guard = Some(client);
    Ok(())
}

#[tauri::command]
fn disconnect_mqtt(mqtt_client: State<MqttState>) -> Result<(), String> {
    let mut client_guard = mqtt_client.lock()
        .map_err(|e| format!("Internal error: {}", e))?;
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
    for ha_state in states {
        let entity_id = ha_state.entity_id.clone();
        let domain = entity_id.split('.').next().map(String::from);
        let friendly_name = if let Some(attrs) = &ha_state.attributes {
            attrs.get("friendly_name")
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

#[tauri::command]
fn open_config_window(app: tauri::AppHandle) -> Result<(), String> {
    tauri::WebviewWindowBuilder::new(
        &app,
        "config",
        tauri::WebviewUrl::App("config".into())
    )
    .title("Configuration")
    .inner_size(800.0, 900.0)
    .resizable(true)
    .build()
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn close_config_window(window: tauri::Window) -> Result<(), String> {
    window.close().map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mqtt_state: MqttState = Arc::new(Mutex::new(None));

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_window_state::Builder::new().build())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
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
            toggle_ha_entity,
            open_config_window,
            close_config_window
        ])
        .setup(|app| {
            // Setup app menu with About
            let about_item = MenuItem::with_id(app, "about", "About Inverter Dashboard", true, None::<&str>)?;
            let app_submenu = Submenu::with_items(app, "Inverter Dashboard", true, &[
                &about_item,
                &PredefinedMenuItem::separator(app)?,
                &PredefinedMenuItem::quit(app, Some("Quit"))?,
            ])?;
            let menu = Menu::with_items(app, &[&app_submenu])?;
            app.set_menu(menu)?;

            // Setup system tray with configuration menu
            let tray = tauri::tray::TrayIconBuilder::new()
                .icon_as_template(false)
                .tooltip("Inverter Dashboard")
                .icon({
                    let icon_bytes = include_bytes!("../icons/32x32.png");
                    let img = image::load_from_memory(icon_bytes)
                        .expect("Failed to load tray icon")
                        .into_rgba8();
                    let (w, h) = img.dimensions();
                    tauri::image::Image::new_owned(img.into_raw(), w, h)
                })
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
                            let app = app.clone();
                            let _ = open_config_window(app);
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .build(app)
                .map_err(|e| format!("Failed to build tray: {}", e))?;
            app.manage(AppTrayIcon(tray));

            // Show window on startup
            let window = app.get_webview_window("main").unwrap();
            window.show().unwrap();

            // macOS: accessory mode keeps app in menu bar (tray icon visible) without dock icon
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // Handle app menu events
            app.on_menu_event(move |app_handle, event| {
                if event.id.as_ref() == "about" {
                    let _ = tauri::WebviewWindowBuilder::new(
                        app_handle,
                        "about",
                        tauri::WebviewUrl::App("about".into())
                    )
                    .title("About Inverter Dashboard")
                    .inner_size(380.0, 320.0)
                    .resizable(false)
                    .center()
                    .build();
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
