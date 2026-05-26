mod ha_api;
pub mod mqtt;

use log::{error, info};
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

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{Emitter, Manager, State};
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
    camera_topic: Option<String>,
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
            ha_boolean_entities: None,
            ha_switch_entities: None,
            ha_water_valve_entity: None,
            ha_pump_switch_entity: None,
            header_toggles: None,
            ha_entities: None,
            header_toggles_config: None,
            portal_id: None,
            camera_topic: Some("frigate/+/events".to_string()),
        }
    }
}

#[tauri::command]
fn get_state(mqtt_client: State<MqttState>) -> Result<InverterState, String> {
    let client = mqtt_client
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
    let store = app
        .store_builder("config.json")
        .build()
        .map_err(|e| format!("Failed to build store: {}", e))?;

    let config: FullConfig = match store.get("config") {
        Some(value) => serde_json::from_value(value).unwrap_or_default(),
        None => FullConfig::default(),
    };

    let entity_id = payload.get("entity").and_then(|v| v.as_str());

    if config.ha_use_direct_api {
        if let Some(entity) = entity_id {
            if is_ha_entity(entity) {
                let client = ha_api::HaApiClient::new(
                    config.ha_url.as_deref().unwrap_or(""),
                    config.ha_port,
                    config.ha_longlived_token.as_deref().unwrap_or(""),
                )
                .await?;

                // For HA toggles, we need the current state or just call toggle
                // Simplified: use toggle_ha_entity logic
                let states = client.get_states().await?;
                let state_opt = states.iter().find(|s| s.entity_id == entity);
                match state_opt {
                    Some(s) => {
                        if s.state == "on" {
                            client.turn_off(entity).await
                        } else {
                            client.turn_on(entity).await
                        }
                    }
                    None => Err(format!("Entity {} not found", entity)),
                }?;
                return Ok(());
            }
        }
    }

    let client = mqtt_client
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
    ];
    let domain = entity_id.split('.').next().unwrap_or("");
    domains.contains(&domain)
}

fn start_ha_polling(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(3));
        loop {
            interval.tick().await;

            let config_res = {
                let store = app.store_builder("config.json").build();
                match store {
                    Ok(s) => match s.get("config") {
                        Some(v) => serde_json::from_value::<FullConfig>(v).ok(),
                        None => None,
                    },
                    Err(_) => None,
                }
            };

            if let Some(config) = config_res {
                if config.ha_use_direct_api
                    && config.ha_url.is_some()
                    && config.ha_longlived_token.is_some()
                {
                    let client_res = ha_api::HaApiClient::new(
                        config.ha_url.as_deref().unwrap_or(""),
                        config.ha_port,
                        config.ha_longlived_token.as_deref().unwrap_or(""),
                    )
                    .await;

                    if let Ok(client) = client_res {
                        if let Ok(states) = client.get_states().await {
                            let mut map = std::collections::HashMap::new();
                            for s in states {
                                if s.state == "on" || s.state == "off" {
                                    map.insert(s.entity_id, s.state);
                                }
                            }
                            let _ = app.emit("ha-state-update", map);
                        }
                    }
                }
            }
        }
    });
}

#[tauri::command]
fn send_command(
    action: String,
    payload: serde_json::Value,
    mqtt_client: State<MqttState>,
) -> Result<(), String> {
    let client = mqtt_client
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
fn save_config(app: tauri::AppHandle, config: FullConfig) -> Result<(), String> {
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
fn connect_mqtt(
    host: String,
    port: u16,
    portal_id: Option<String>,
    camera_topic: Option<String>,
    app: tauri::AppHandle,
    mqtt_client: State<MqttState>,
) -> Result<(), String> {
    let mut client_guard = mqtt_client
        .lock()
        .map_err(|e| format!("Internal error: {}", e))?;
    let mut client = MqttClient::new(host, port);
    client.set_app_handle(app);
    client.set_portal_id(portal_id);
    client.set_camera_topic(camera_topic);
    client.connect().map_err(|e| e.to_string())?;
    *client_guard = Some(client);
    Ok(())
}

#[tauri::command]
fn disconnect_mqtt(mqtt_client: State<MqttState>) -> Result<(), String> {
    let mut client_guard = mqtt_client
        .lock()
        .map_err(|e| format!("Internal error: {}", e))?;
    *client_guard = None;
    Ok(())
}

#[tauri::command]
async fn test_ha_connection(url: String, port: Option<u16>, token: String) -> Result<(), String> {
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
    tauri::WebviewWindowBuilder::new(&app, "config", tauri::WebviewUrl::App("config".into()))
        .title("Configuration")
        .inner_size(CONFIG_WINDOW_W, CONFIG_WINDOW_H)
        .resizable(true)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn close_config_window(window: tauri::Window) -> Result<(), String> {
    window.close().map_err(|e| e.to_string())
}

use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mqtt_state: MqttState = Arc::new(Mutex::new(None));

    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_window_state::Builder::new().build())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .manage(mqtt_state)
        .invoke_handler(tauri::generate_handler![
            get_state,
            send_command,
            perform_action,
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
            // Start background HA polling
            start_ha_polling(app.handle().clone());

            // Setup app menu with About, Edit and Window menus
            let about_item =
                MenuItem::with_id(app, "about", "About Inverter Dashboard", true, None::<&str>)?;
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
            let tray_result = TrayIconBuilder::with_id("main-tray")
                .tooltip("Inverter Dashboard")
                .icon_as_template(true)
                .icon({
                    let icon_bytes = include_bytes!("../icons/icon.png");
                    let img = image::load_from_memory(icon_bytes)
                        .expect("Failed to load tray icon")
                        .into_rgba8();
                    let (w, h) = img.dimensions();
                    tauri::image::Image::new_owned(img.into_raw(), w, h)
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
                        &tauri::menu::MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?,
                    ],
                )?)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "config" => {
                        let app = app.clone();
                        let _ = open_config_window(app);
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
                        }
                    }
                })
                .build(app);
            if let Ok(tray) = tray_result {
                info!("Tray icon built successfully.");
                app.manage(AppTrayIcon(tray));
            } else if let Err(e) = tray_result {
                error!("Failed to build tray icon: {}", e);
            }

            // Show window on startup
            info!("Showing main window...");
            let window = app.get_webview_window("main").unwrap();
            window.show().unwrap();

            // macOS: accessory mode keeps app in menu bar (tray icon visible) without dock icon
            #[cfg(target_os = "macos")]
            {
                info!("Setting activation policy to Accessory...");
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }

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

            info!("Setup block completed successfully.");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
