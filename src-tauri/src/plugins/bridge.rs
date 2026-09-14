//! App session and window authority around the desktop worker host.

use super::runtime::{PluginHost, PluginSnapshot, WorkerState};
use crate::auth;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{Emitter, Manager, State};

pub(crate) struct DesktopPlugins {
    host: PluginHost,
    exit: ExitGate,
}

#[derive(Default)]
struct ExitGate {
    started: AtomicBool,
    finished: AtomicBool,
}

impl ExitGate {
    /// Returns whether to prevent exit and whether this request owns cleanup.
    fn request(&self) -> (bool, bool) {
        if self.finished.load(Ordering::Acquire) {
            return (false, false);
        }
        (true, !self.started.swap(true, Ordering::AcqRel))
    }
}

fn trusted_window(label: &str) -> bool {
    matches!(label, "main" | "config")
}

fn require_access(
    app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
    state: &DesktopPlugins,
) -> Result<(), String> {
    if !trusted_window(window.label()) || state.exit.started.load(Ordering::Acquire) {
        return Err("Plugin access is unavailable".into());
    }
    auth::require_session(app).inspect_err(|_| state.host.revoke())
}

#[tauri::command]
pub(crate) fn get_plugin_snapshot(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: State<'_, DesktopPlugins>,
) -> Result<Vec<PluginSnapshot>, String> {
    require_access(&app, &window, &state)?;
    Ok(state.host.snapshots())
}

#[tauri::command]
pub(crate) async fn plugin_action(
    plugin_id: String,
    action_id: String,
    params: Value,
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: State<'_, DesktopPlugins>,
) -> Result<Value, String> {
    require_access(&app, &window, &state)?;
    let epoch = state.host.authority_epoch();
    let result = state
        .host
        .action(&plugin_id, &action_id, params, Duration::from_secs(5))
        .await
        .map_err(|error| error.to_string())?;
    require_access(&app, &window, &state)?;
    if state.host.authority_epoch() != epoch {
        return Err("Plugin session changed".into());
    }
    Ok(result)
}

pub(crate) fn install(app: &tauri::AppHandle) {
    let changes = Arc::new(tokio::sync::Notify::new());
    let notify = changes.clone();
    let host = PluginHost::new(Arc::new(move || {
        notify.notify_one();
    }));
    app.manage(DesktopPlugins {
        host,
        exit: ExitGate::default(),
    });
    forward_changes(app.clone(), changes);
    authentication_changed(app);
    watch_session_expiry(app.clone());
}

fn forward_changes(app: tauri::AppHandle, changes: Arc<tokio::sync::Notify>) {
    tauri::async_runtime::spawn(async move {
        loop {
            changes.notified().await;
            // Coalesce all workers into at most 20 UI refresh signals per second.
            // Notify retains one pending permit instead of an unbounded event queue.
            tokio::time::sleep(Duration::from_millis(50)).await;
            if app
                .state::<DesktopPlugins>()
                .exit
                .started
                .load(Ordering::Acquire)
            {
                break;
            }
            // The fixed event carries no worker data; each window rechecks its session.
            let _ = app.emit("plugin-host-update", ());
        }
    });
}

fn watch_session_expiry(app: tauri::AppHandle) {
    // Session expiry must stop background workers even when no webview is making
    // requests. Empty/stopped hosts do not read configuration on each tick.
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tick.tick().await;
            let state = app.state::<DesktopPlugins>();
            if state.exit.started.load(Ordering::Acquire) {
                break;
            }
            let active = state.host.snapshots().iter().any(|worker| {
                matches!(
                    worker.state,
                    WorkerState::Starting | WorkerState::Running | WorkerState::Restarting
                )
            });
            if active && auth::require_session(&app).is_err() {
                state.host.revoke();
                let _ = app.emit("auth-state-changed", ());
            }
        }
    });
}

pub(crate) fn authentication_changed(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<DesktopPlugins>() {
        // Revoke synchronously before the frontend receives auth-state-changed.
        // Resume never revives the previous epoch's processes or queued actions.
        state.host.revoke();
        if !state.exit.started.load(Ordering::Acquire) && auth::require_session(app).is_ok() {
            state.host.resume();
        }
    }
}

pub(crate) fn on_run_event(app: &tauri::AppHandle, event: tauri::RunEvent) {
    let Some(state) = app.try_state::<DesktopPlugins>() else {
        return;
    };
    if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
        let (prevent, cleanup) = state.exit.request();
        if prevent {
            api.prevent_exit();
        }
        if cleanup {
            state.host.revoke();
            let host = state.host.clone();
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                host.shutdown().await;
                app.state::<DesktopPlugins>()
                    .exit
                    .finished
                    .store(true, Ordering::Release);
                app.exit(code.unwrap_or(0));
            });
        }
    } else if matches!(event, tauri::RunEvent::Exit) {
        state.host.revoke();
    }
}

#[cfg(test)]
mod tests {
    use super::{trusted_window, ExitGate, Ordering};

    #[test]
    fn repeated_quit_waits_until_worker_cleanup_finishes() {
        let gate = ExitGate::default();
        assert_eq!(gate.request(), (true, true));
        assert_eq!(gate.request(), (true, false));
        gate.finished.store(true, Ordering::Release);
        assert_eq!(gate.request(), (false, false));
    }

    #[test]
    fn only_dashboard_and_settings_windows_can_access_workers() {
        assert!(trusted_window("main"));
        assert!(trusted_window("config"));
        for label in [
            "about",
            "camera-video-123",
            "plugin-custom",
            "",
            "main/other",
        ] {
            assert!(!trusted_window(label), "{label}");
        }
    }
}
