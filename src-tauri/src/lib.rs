pub mod mqtt;

use mqtt::{MqttClient, InverterState};
use std::sync::{Arc, Mutex};
use tauri::{Manager, State};

// Global state for the MQTT client
type MqttState = Arc<Mutex<Option<MqttClient>>>;

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
fn get_config() -> Result<serde_json::Value, String> {
    let config_paths = [
        std::path::PathBuf::from("config.json"),
        std::path::PathBuf::from("../Resources/config.json"),
    ];

    let config_content = config_paths.iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
        .ok_or_else(|| "Config file not found".to_string())?;

    let config: serde_json::Value = serde_json::from_str(&config_content)
        .map_err(|e| format!("Failed to parse config: {}", e))?;

    Ok(config)
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
        .manage(mqtt_state)
        .invoke_handler(tauri::generate_handler![
            get_state,
            send_command,
            connect_mqtt,
            disconnect_mqtt,
            get_config
        ])
        .setup(|app| {
            // Setup system tray
            let _tray = tauri::tray::TrayIconBuilder::new()
                .menu(&tauri::menu::Menu::with_items(app, &[
                    &tauri::menu::MenuItem::with_id(app, "show", "Show", true, None::<&str>)?,
                    &tauri::menu::MenuItem::with_id(app, "hide", "Hide", true, None::<&str>)?,
                    &tauri::menu::MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?,
                ])?)
                .on_menu_event(move |app, event| {
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