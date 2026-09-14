//! HA connection supervisor. A config revision cancels both connect and run,
//! including the reader task owned by HaWebSocketClient.
use super::{build_ws_url, ha_api, load_config, HaEntityStates};
use log::{info, warn};
use std::time::Duration;
use tauri::{Emitter, Manager};

fn disconnected(app: &tauri::AppHandle, clear: bool) {
    ha_api::set_connection_status(false);
    if clear {
        let states = app.state::<HaEntityStates>();
        if let Ok(mut guard) = states.0.lock() {
            guard.clear();
        }
        ha_api::force_emit_ha_filtered(app, &states.0);
    }
    let _ = app.emit("ha-connection-status", false);
}

pub(super) fn start(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut changes = ha_api::config_changes();
        loop {
            changes.borrow_and_update();
            let config = match load_config(&app) {
                Ok(config) => config,
                Err(error) => {
                    warn!("Cannot load HA configuration: {error}");
                    tokio::select! { _ = tokio::time::sleep(Duration::from_secs(5)) => {}, _ = changes.changed() => {} }
                    continue;
                }
            };
            if !config.ha_use_direct_api
                || config.ha_url.as_deref().is_none_or(|s| s.trim().is_empty())
                || config
                    .ha_longlived_token
                    .as_deref()
                    .is_none_or(|s| s.trim().is_empty())
            {
                disconnected(&app, true);
                let _ = changes.changed().await;
                continue;
            }
            let url = build_ws_url(config.ha_url.as_deref().unwrap_or(""), config.ha_port);
            let token = config.ha_longlived_token.as_deref().unwrap_or("");
            let states = app.state::<HaEntityStates>().0.clone();
            let result = tokio::select! {
                biased;
                _ = changes.changed() => { disconnected(&app, true); continue; }
                result = ha_api::HaWebSocketClient::connect(&url, token, app.clone(), states) => result
            };
            match result {
                Ok(mut client) => {
                    ha_api::clear_entity_skip_list();
                    ha_api::set_connection_status(true);
                    let _ = app.emit("ha-connection-status", true);
                    info!("HA WebSocket connected");
                    let changed = tokio::select! {
                        biased;
                        _ = changes.changed() => true,
                        _ = client.run() => false
                    };
                    drop(client);
                    disconnected(&app, changed);
                    if changed {
                        continue;
                    }
                }
                Err(error) => {
                    warn!("HA WebSocket connection failed: {error}");
                    disconnected(&app, false);
                }
            }
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(5)) => {},
                _ = changes.changed() => disconnected(&app, true),
            }
        }
    });
}
