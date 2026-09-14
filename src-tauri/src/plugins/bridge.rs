//! App session and window authority around the desktop worker host.

use super::application::{
    ManagerSnapshot, PackageApplication, PackagePreview, RetainedPluginData, SettingsSaveResult,
};
use super::publishers::embedded_trust;
use super::runtime::{PluginHost, PluginSnapshot, WorkerState};
use super::settings::PluginSettingsView;
use crate::auth;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{Emitter, Manager, State};

pub(crate) struct DesktopPlugins {
    host: PluginHost,
    packages: PackageApplication,
    session_gate: Mutex<()>,
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
    auth::require_session(app).inspect_err(|_| authentication_changed(app))
}

fn management_window(label: &str) -> bool {
    label == "config"
}

fn management_epoch(
    app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
    state: &DesktopPlugins,
) -> Result<u64, String> {
    let epoch = state.host.authority_epoch();
    require_access(app, window, state)?;
    if !management_window(window.label()) {
        return Err("Packages can only be managed from the settings window".into());
    }
    if !state.host.is_authorized_epoch(epoch) {
        return Err("Plugin session changed".into());
    }
    Ok(epoch)
}

fn finish_management(
    app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
    state: &DesktopPlugins,
    epoch: u64,
) -> Result<(), String> {
    if management_epoch(app, window, state)? != epoch {
        return Err("Plugin session changed".into());
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn get_plugin_manager_snapshot(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: State<'_, DesktopPlugins>,
) -> Result<ManagerSnapshot, String> {
    let epoch = management_epoch(&app, &window, &state)?;
    let snapshot = state.packages.snapshot(epoch).await?;
    finish_management(&app, &window, &state, epoch)?;
    Ok(snapshot)
}

#[tauri::command]
pub(crate) async fn preview_plugin_package(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: State<'_, DesktopPlugins>,
) -> Result<Option<PackagePreview>, String> {
    use tauri_plugin_dialog::{DialogExt, FilePath};
    let epoch = management_epoch(&app, &window, &state)?;
    let token = state.packages.begin_selection(window.label(), epoch)?;
    let result = async {
        let (selected, result) = tokio::sync::oneshot::channel();
        app.dialog()
            .file()
            .set_parent(&window)
            .set_title("Select a signed desktop plugin package")
            .add_filter("Inverter Desktop plugin", &["idplugin"])
            .pick_file(move |file| {
                let _ = selected.send(file);
            });
        let file = result
            .await
            .map_err(|_| "Package file selection was interrupted")?;
        finish_management(&app, &window, &state, epoch)?;
        match file {
            None => Ok(None),
            Some(FilePath::Path(path)) => state
                .packages
                .finish_selection(&token, window.label(), epoch, path)
                .await
                .map(Some),
            Some(_) => Err("Select a local plugin package file".into()),
        }
    }
    .await;
    if !matches!(result, Ok(Some(_))) {
        state.packages.discard(&token, window.label(), epoch);
    }
    let preview = result?;
    if let Err(error) = finish_management(&app, &window, &state, epoch) {
        state.packages.discard(&token, window.label(), epoch);
        return Err(error);
    }
    Ok(preview)
}

#[tauri::command]
pub(crate) fn discard_plugin_package(
    token: String,
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: State<'_, DesktopPlugins>,
) -> Result<(), String> {
    let epoch = management_epoch(&app, &window, &state)?;
    state.packages.discard(&token, window.label(), epoch);
    Ok(())
}

#[tauri::command]
pub(crate) async fn install_plugin_package(
    token: String,
    enable: bool,
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: State<'_, DesktopPlugins>,
) -> Result<(), String> {
    let epoch = management_epoch(&app, &window, &state)?;
    state
        .packages
        .install_review(&token, window.label(), epoch, enable)
        .await?;
    finish_management(&app, &window, &state, epoch)
}

#[tauri::command]
pub(crate) async fn set_plugin_enabled(
    plugin_id: String,
    enabled: bool,
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: State<'_, DesktopPlugins>,
) -> Result<(), String> {
    let epoch = management_epoch(&app, &window, &state)?;
    state
        .packages
        .set_enabled(&plugin_id, enabled, epoch)
        .await?;
    finish_management(&app, &window, &state, epoch)
}

#[tauri::command]
pub(crate) async fn rollback_plugin_package(
    plugin_id: String,
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: State<'_, DesktopPlugins>,
) -> Result<(), String> {
    let epoch = management_epoch(&app, &window, &state)?;
    state.packages.rollback(&plugin_id, epoch).await?;
    finish_management(&app, &window, &state, epoch)
}

#[tauri::command]
pub(crate) async fn uninstall_plugin_package(
    plugin_id: String,
    delete_settings: Option<bool>,
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: State<'_, DesktopPlugins>,
) -> Result<(), String> {
    let epoch = management_epoch(&app, &window, &state)?;
    state
        .packages
        .uninstall_with_settings(&plugin_id, delete_settings.unwrap_or(false), epoch)
        .await?;
    finish_management(&app, &window, &state, epoch)
}

#[tauri::command]
pub(crate) async fn get_plugin_settings(
    plugin_id: String,
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: State<'_, DesktopPlugins>,
) -> Result<PluginSettingsView, String> {
    let epoch = management_epoch(&app, &window, &state)?;
    let settings = state.packages.get_settings(&plugin_id, epoch).await?;
    finish_management(&app, &window, &state, epoch)?;
    Ok(settings)
}

#[tauri::command]
pub(crate) async fn get_retained_plugin_data(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: State<'_, DesktopPlugins>,
) -> Result<RetainedPluginData, String> {
    let epoch = management_epoch(&app, &window, &state)?;
    let data = state.packages.retained_data(epoch).await?;
    finish_management(&app, &window, &state, epoch)?;
    Ok(data)
}

#[tauri::command]
pub(crate) async fn delete_retained_plugin_data(
    record_id: String,
    revision: String,
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: State<'_, DesktopPlugins>,
) -> Result<(), String> {
    let epoch = management_epoch(&app, &window, &state)?;
    state
        .packages
        .delete_retained_data(&record_id, &revision, epoch)
        .await?;
    finish_management(&app, &window, &state, epoch)
}

#[tauri::command]
pub(crate) async fn save_plugin_settings(
    plugin_id: String,
    revision: String,
    values: BTreeMap<String, Value>,
    secret_changes: BTreeMap<String, Option<String>>,
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: State<'_, DesktopPlugins>,
) -> Result<SettingsSaveResult, String> {
    let epoch = management_epoch(&app, &window, &state)?;
    let result = state
        .packages
        .save_settings(&plugin_id, epoch, revision, values, secret_changes)
        .await?;
    finish_management(&app, &window, &state, epoch)?;
    Ok(result)
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
    let epoch = state.host.authority_epoch();
    require_access(&app, &window, &state)?;
    let result = state
        .host
        .action_in_epoch(
            &plugin_id,
            &action_id,
            params,
            Duration::from_secs(5),
            epoch,
        )
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
    let trust = embedded_trust();
    let notify = changes.clone();
    let packages = PackageApplication::new(
        host.clone(),
        env!("INVERTER_DESKTOP_TARGET").into(),
        trust.as_ref().is_ok_and(|trust| !trust.is_empty()),
        Arc::new(move || notify.notify_one()),
    );
    let root = app
        .path()
        .app_local_data_dir()
        .map(|path| path.join("plugins"))
        .map_err(|error| error.to_string());
    app.manage(DesktopPlugins {
        host,
        packages: packages.clone(),
        session_gate: Mutex::new(()),
        exit: ExitGate::default(),
    });
    let key_app = app.clone();
    tauri::async_runtime::spawn(async move {
        packages
            .initialize_with_key(
                root,
                trust,
                Arc::new(move || crate::config_store::plugin_settings_key(&key_app)),
            )
            .await;
    });
    forward_changes(app.clone(), changes);
    authentication_changed(app);
    watch_session_expiry(app.clone());
}

const NOTIFICATION_SIGNAL_CAPACITY: usize = 1;

fn forward_changes(app: tauri::AppHandle, changes: Arc<tokio::sync::Notify>) {
    let (notifications, requests) = tokio::sync::mpsc::channel(NOTIFICATION_SIGNAL_CAPACITY);
    let notification_app = app.clone();
    tauri::async_runtime::spawn(drain_notification_requests(requests, move || {
        let app = notification_app.clone();
        async move {
            // One awaited blocking dispatcher keeps native calls off the async
            // executor. Its own exit/auth checks run only once the task starts.
            let _ = tauri::async_runtime::spawn_blocking(move || {
                forward_notifications(&app);
            })
            .await;
        }
    }));
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
            if app
                .state::<DesktopPlugins>()
                .host
                .has_pending_notifications()
            {
                // A single pending signal coalesces bursts while the separate
                // dispatcher waits for the OS; UI refreshes never await it.
                let _ = notifications.try_send(());
            }
            // The fixed event carries no worker data; each window rechecks its session.
            let _ = app.emit("plugin-host-update", ());
        }
    });
}

async fn drain_notification_requests<F, Submission>(
    mut requests: tokio::sync::mpsc::Receiver<()>,
    mut submit: F,
) where
    F: FnMut() -> Submission,
    Submission: std::future::Future<Output = ()>,
{
    // There is one receiver for the app lifetime, at most one pending signal,
    // and no overlapping submission tasks. Dropping the producer on shutdown
    // closes this loop after its already-owned dispatch finishes.
    while requests.recv().await.is_some() {
        submit().await;
    }
}

fn forward_notifications(app: &tauri::AppHandle) {
    let state = app.state::<DesktopPlugins>();
    if state.exit.started.load(Ordering::Acquire) || !state.host.has_pending_notifications() {
        return;
    }
    // Validate before entering the host authority guard; auth transitions reenter
    // the host. The dispatcher then checks each generation and stop signal.
    if auth::require_session(app).is_err() {
        authentication_changed(app);
        let _ = app.emit("auth-state-changed", ());
        return;
    }
    state.host.dispatch_notifications(|message| {
        if state.exit.started.load(Ordering::Acquire) {
            return;
        }
        // Tauri's notification wrapper schedules another task. Submit directly
        // through the same native backend while this generation is authorized.
        // Display timing and already submitted notifications belong to the OS.
        if super::native_notifications::submit(app, message).is_err() {
            log::warn!(
                "Desktop notification delivery failed for plugin {}",
                message.plugin_id
            );
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
            state.packages.expire_preview();
            let active = state.host.snapshots().iter().any(|worker| {
                matches!(
                    worker.state,
                    WorkerState::Starting | WorkerState::Running | WorkerState::Restarting
                )
            });
            if (active || state.packages.has_session_work()) && auth::require_session(&app).is_err()
            {
                authentication_changed(&app);
                let _ = app.emit("auth-state-changed", ());
            }
        }
    });
}

pub(crate) fn authentication_changed(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<DesktopPlugins>() {
        // Sample the live policy/session under the same gate as the transition.
        // A delayed login notification cannot replay an old `unlocked` value
        // after a completed logout notification.
        let _transition = state
            .session_gate
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        // Revoke synchronously before the frontend receives auth-state-changed.
        // Resume never revives the previous epoch's processes or queued actions.
        let unlocked =
            !state.exit.started.load(Ordering::Acquire) && auth::require_session(app).is_ok();
        if let Some(epoch) = state.packages.session_changed(unlocked) {
            let packages = state.packages.clone();
            tauri::async_runtime::spawn(async move {
                // Errors remain visible in the manager snapshot. A revoked
                // restoration is expected when logout or quit wins the race.
                let _ = packages.restore(epoch).await;
            });
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
            state.packages.begin_shutdown();
            let packages = state.packages.clone();
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = packages.close().await {
                    log::error!("Plugin cleanup could not complete: {error}");
                    // Keep the application and lease alive; a later quit may
                    // retry cleanup, but no new package work is admitted.
                    app.state::<DesktopPlugins>()
                        .exit
                        .started
                        .store(false, Ordering::Release);
                    return;
                }
                app.state::<DesktopPlugins>()
                    .exit
                    .finished
                    .store(true, Ordering::Release);
                app.exit(code.unwrap_or(0));
            });
        }
    } else if let tauri::RunEvent::WindowEvent {
        label,
        event: tauri::WindowEvent::Destroyed,
        ..
    } = event
    {
        state.packages.window_closed(&label);
    } else if matches!(event, tauri::RunEvent::Exit) {
        state.packages.begin_shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        drain_notification_requests, management_window, trusted_window, ExitGate, Ordering,
        NOTIFICATION_SIGNAL_CAPACITY,
    };

    #[tokio::test]
    async fn slow_notification_delivery_coalesces_without_blocking_or_overlapping() {
        use std::sync::atomic::{AtomicBool, AtomicUsize};
        use std::sync::Arc;
        use std::time::Duration;

        let (signals, requests) = tokio::sync::mpsc::channel(NOTIFICATION_SIGNAL_CAPACITY);
        let (started, receiving) = tokio::sync::oneshot::channel();
        let (release, blocked) = tokio::sync::oneshot::channel();
        let calls = Arc::new(AtomicUsize::new(0));
        let active = Arc::new(AtomicBool::new(false));
        let mut first = Some((started, blocked));
        let count = calls.clone();
        let in_flight = active.clone();
        let drain = tokio::spawn(drain_notification_requests(requests, move || {
            let first = first.take();
            let count = count.clone();
            let in_flight = in_flight.clone();
            async move {
                assert!(!in_flight.swap(true, Ordering::AcqRel));
                count.fetch_add(1, Ordering::AcqRel);
                if let Some((started, blocked)) = first {
                    let _ = started.send(());
                    blocked.await.unwrap();
                }
                in_flight.store(false, Ordering::Release);
            }
        }));
        signals.try_send(()).unwrap();
        receiving.await.unwrap();

        // The producer remains free to forward UI events while the native
        // submission is blocked. A burst retains only one follow-up request.
        let mut accepted = 0;
        for _ in 0..100 {
            accepted += usize::from(signals.try_send(()).is_ok());
        }
        assert_eq!(accepted, 1);
        assert_eq!(calls.load(Ordering::Acquire), 1);
        assert!(!drain.is_finished());
        drop(signals);
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(1), drain)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(calls.load(Ordering::Acquire), 2);
        assert!(!active.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn idle_notification_dispatcher_never_starts_native_or_auth_work() {
        let (signals, requests) = tokio::sync::mpsc::channel(NOTIFICATION_SIGNAL_CAPACITY);
        drop(signals);
        drain_notification_requests(requests, || async {
            panic!("an idle dispatcher must not inspect auth or submit notifications");
        })
        .await;
    }

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

    #[test]
    fn only_settings_can_manage_packages() {
        assert!(management_window("config"));
        for label in [
            "main",
            "about",
            "camera-video-123",
            "plugin-custom",
            "",
            "config/other",
        ] {
            assert!(!management_window(label));
        }
    }
}
