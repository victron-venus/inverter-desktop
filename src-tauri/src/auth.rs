//! App-wide unlock session shared by trusted local windows. Every custom IPC
//! command is checked centrally; an overlay alone is not an authorization boundary.
use super::{load_config, FullConfig};
use serde::Serialize;
#[cfg(desktop)]
use std::sync::MutexGuard;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};
use tauri::Emitter;

const SESSION_TTL: Duration = Duration::from_secs(15 * 60);
static SESSION: LazyLock<Mutex<Option<Session>>> = LazyLock::new(|| Mutex::new(None));

struct Session {
    token: String,
    created: Instant,
}

impl Session {
    fn valid_at(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.created) < SESSION_TTL
    }
}

pub(super) fn public_command(command: &str) -> bool {
    matches!(
        command,
        "auth_status"
            | "get_release_info"
            | "auth_login"
            | "auth_check"
            | "auth_logout"
            | "auth_biometric_available"
            | "auth_biometric"
            | "close_config_window"
    )
}

fn unlocked() -> Result<bool, String> {
    let mut session = SESSION.lock().map_err(|e| e.to_string())?;
    Ok(session_unlocked_at(&mut session, Instant::now()))
}

fn session_unlocked_at(session: &mut Option<Session>, now: Instant) -> bool {
    if session.as_ref().is_some_and(|s| !s.valid_at(now)) {
        *session = None;
    }
    session.is_some()
}

/// Keep logout from changing the checked session before a synchronous resume.
#[cfg(desktop)]
pub(super) struct SessionAuthority<'a> {
    _session: MutexGuard<'a, Option<Session>>,
}

#[cfg(desktop)]
fn session_authority_at<'a>(
    config: &FullConfig,
    sessions: &'a Mutex<Option<Session>>,
    now: Instant,
) -> Result<SessionAuthority<'a>, String> {
    let mut session = sessions.lock().map_err(|e| e.to_string())?;
    if !session_unlocked_at(&mut session, now) && config.auth_enabled.unwrap_or(false) {
        return Err("Authentication required".into());
    }
    Ok(SessionAuthority { _session: session })
}

#[cfg(desktop)]
pub(super) fn session_authority(config: &FullConfig) -> Result<SessionAuthority<'static>, String> {
    session_authority_at(config, &SESSION, Instant::now())
}

pub(super) fn require_session(app: &tauri::AppHandle) -> Result<(), String> {
    if !load_config(app)?.auth_enabled.unwrap_or(false) || unlocked()? {
        Ok(())
    } else {
        Err("Authentication required".into())
    }
}

pub(super) fn validate_policy(config: &FullConfig) -> Result<(), String> {
    if config.auth_enabled.unwrap_or(false)
        && (config
            .auth_username
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
            || config.auth_password_verifier.is_none())
    {
        return Err("A username and password are required when authentication is enabled".into());
    }
    Ok(())
}

fn policy_changed(before: &FullConfig, after: &FullConfig) -> bool {
    before.auth_enabled != after.auth_enabled
        || before.auth_username != after.auth_username
        || before.auth_password != after.auth_password
        || before.auth_password_verifier != after.auth_password_verifier
        || before.auth_biometric != after.auth_biometric
}

fn notify_session_changed(app: &tauri::AppHandle) {
    #[cfg(desktop)]
    crate::plugins::bridge::authentication_changed(app);
    let _ = app.emit("auth-state-changed", ());
}

pub(super) fn revoke_if_policy_changed(
    app: &tauri::AppHandle,
    before: &FullConfig,
    after: &FullConfig,
) -> Result<(), String> {
    revoke_policy_session(&SESSION, before, after, || notify_session_changed(app))
}

fn revoke_policy_session(
    sessions: &Mutex<Option<Session>>,
    before: &FullConfig,
    after: &FullConfig,
    notify: impl FnOnce(),
) -> Result<(), String> {
    if policy_changed(before, after) {
        *sessions.lock().map_err(|e| e.to_string())? = None;
        notify();
    }
    Ok(())
}

fn start_session() -> Result<String, String> {
    let token = uuid::Uuid::new_v4().to_string();
    *SESSION.lock().map_err(|e| e.to_string())? = Some(Session {
        token: token.clone(),
        created: Instant::now(),
    });
    Ok(token)
}

#[derive(Serialize)]
pub(crate) struct AuthStatus {
    enabled: bool,
    unlocked: bool,
}

#[tauri::command]
pub(crate) fn auth_status(app: tauri::AppHandle) -> Result<AuthStatus, String> {
    let enabled = load_config(&app)?.auth_enabled.unwrap_or(false);
    let status = AuthStatus {
        enabled,
        unlocked: !enabled || unlocked()?,
    };
    #[cfg(desktop)]
    if status.unlocked {
        crate::plugins::bridge::recover_session_if_needed(&app);
    }
    Ok(status)
}

#[tauri::command]
pub(crate) fn auth_login(
    username: String,
    password: String,
    app: tauri::AppHandle,
) -> Result<String, String> {
    login_with_config(
        &crate::CONFIG_UPDATE_GATE,
        || load_config(&app),
        &username,
        &password,
        start_session,
        || notify_session_changed(&app),
    )
}

// The same lock covers the policy snapshot, expensive verification and session
// commit. Notifications happen only after this helper releases it.
fn login_with_config(
    gate: &Mutex<()>,
    load: impl FnOnce() -> Result<FullConfig, String>,
    username: &str,
    password: &str,
    grant: impl FnOnce() -> Result<String, String>,
    notify: impl FnOnce(),
) -> Result<String, String> {
    let _update = gate.lock().map_err(|_| "Config update lock failed")?;
    let config = load()?;
    if !config.auth_enabled.unwrap_or(false) {
        return Ok("disabled".into());
    }
    let matches = match &config.auth_password_verifier {
        Some(verifier) => verifier.matches(password)?,
        None => false,
    };
    if username != config.auth_username.as_deref().unwrap_or("") || !matches {
        return Err("Invalid credentials".into());
    }
    let token = grant()?;
    drop(_update);
    notify();
    Ok(token)
}

#[tauri::command]
pub(crate) fn auth_check(token: String) -> Result<bool, String> {
    let _ = unlocked()?;
    Ok(SESSION
        .lock()
        .map_err(|e| e.to_string())?
        .as_ref()
        .is_some_and(|s| s.token == token))
}

#[tauri::command]
pub(crate) fn auth_logout(app: tauri::AppHandle) -> Result<(), String> {
    *SESSION.lock().map_err(|e| e.to_string())? = None;
    notify_session_changed(&app);
    Ok(())
}

fn biometric_enabled(config: &FullConfig) -> bool {
    config.auth_enabled.unwrap_or(false) && config.auth_biometric.unwrap_or(false)
}

#[tauri::command]
pub(crate) fn auth_biometric_available(app: tauri::AppHandle) -> Result<bool, String> {
    if !biometric_enabled(&load_config(&app)?) {
        return Ok(false);
    }
    #[cfg(target_os = "macos")]
    {
        Ok(unsafe { super::biometric_available() })
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(false)
    }
}

#[tauri::command]
pub(crate) async fn auth_biometric(app: tauri::AppHandle) -> Result<String, String> {
    let owned_app = app.clone();
    let policy = tauri::async_runtime::spawn_blocking(move || load_config(&owned_app))
        .await
        .map_err(|e| e.to_string())??;
    if !biometric_enabled(&policy) {
        return Err("Biometric authentication is disabled".into());
    }
    #[cfg(target_os = "macos")]
    {
        let ok = tauri::async_runtime::spawn_blocking(|| {
            let reason = std::ffi::CString::new("Authenticate to access Inverter Desktop")
                .expect("static reason");
            unsafe { super::biometric_authenticate(reason.as_ptr()) }
        })
        .await
        .map_err(|e| e.to_string())?;
        // The policy may have changed while the native prompt was open.
        let owned_app = app.clone();
        let token = tauri::async_runtime::spawn_blocking(move || {
            let _update = crate::CONFIG_UPDATE_GATE
                .lock()
                .map_err(|_| "Config update lock failed")?;
            if !ok || policy_changed(&policy, &load_config(&owned_app)?) {
                return Err("Biometric authentication failed or was cancelled".into());
            }
            start_session()
        })
        .await
        .map_err(|e| e.to_string())??;
        notify_session_changed(&app);
        Ok(token)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("Biometric authentication is only supported on macOS".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn password_config() -> FullConfig {
        let mut config = FullConfig {
            auth_enabled: Some(true),
            auth_username: Some("operator".into()),
            auth_password: Some("original password".into()),
            ..Default::default()
        };
        crate::auth_password::migrate(&mut config).unwrap();
        config
    }

    #[test]
    fn login_policy_is_locked_until_session_commit_and_released_afterward() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{mpsc, Arc};
        let gate = Arc::new(Mutex::new(()));
        let changed = Arc::new(AtomicBool::new(false));
        let (waiting_tx, waiting_rx) = mpsc::channel();
        let other_gate = gate.clone();
        let other_changed = changed.clone();
        let config = password_config();
        let mut worker = None;
        let token = login_with_config(
            &gate,
            || {
                worker = Some(std::thread::spawn(move || {
                    waiting_tx.send(()).unwrap();
                    let _update = other_gate.lock().unwrap();
                    other_changed.store(true, Ordering::SeqCst);
                }));
                waiting_rx.recv().unwrap();
                Ok(config)
            },
            "operator",
            "original password",
            || {
                assert!(gate.try_lock().is_err());
                assert!(!changed.load(Ordering::SeqCst));
                Ok("session".into())
            },
            || assert!(gate.try_lock().is_ok()),
        )
        .unwrap();
        assert_eq!(token, "session");
        worker.unwrap().join().unwrap();
        assert!(changed.load(Ordering::SeqCst));
        assert!(gate.try_lock().is_ok());
    }

    #[test]
    fn committed_storage_warning_still_revokes_session_and_finishes_desired_effects() {
        use crate::config_store::test_support::{self, Fault};
        use std::cell::Cell;
        let before = password_config();
        let mut after = before.clone();
        after.auth_password_verifier = None;
        after.auth_password = Some("new password".into());
        crate::auth_password::prepare_save(&mut after, &before).unwrap();
        for fault in [Fault::None, Fault::FileSync, Fault::DirectorySync] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("config.json");
            let key = [3; 32];
            test_support::save(&path, &key, &before, Fault::None).unwrap();
            let original = std::fs::read(&path).unwrap();
            let sessions = Mutex::new(Some(Session {
                token: "existing session".into(),
                created: Instant::now(),
            }));
            let notified = Cell::new(false);
            let desired_completed = Cell::new(false);
            // Same ?-ordered native boundary as core save and plugin commits:
            // precommit errors stop, committed outcomes continue finalization.
            let result = (|| {
                test_support::save(&path, &key, &after, fault)?;
                revoke_policy_session(&sessions, &before, &after, || notified.set(true))?;
                desired_completed.set(true);
                Ok::<(), String>(())
            })();
            if matches!(fault, Fault::FileSync) {
                assert!(result.is_err());
                assert_eq!(std::fs::read(&path).unwrap(), original);
                assert!(sessions.lock().unwrap().is_some());
                assert!(!notified.get());
                assert!(!desired_completed.get());
            } else {
                assert!(result.is_ok(), "{fault:?}: {result:?}");
                assert_eq!(
                    test_support::load(&path, &key).auth_password_verifier,
                    after.auth_password_verifier
                );
                assert!(sessions.lock().unwrap().is_none());
                assert!(notified.get());
                assert!(desired_completed.get());
            }
        }
    }

    #[test]
    fn disabled_login_preserves_sentinel_without_session_or_notification() {
        let token = login_with_config(
            &Mutex::new(()),
            || Ok(FullConfig::default()),
            "ignored",
            "ignored",
            || panic!("disabled authentication must not grant a session"),
            || panic!("disabled authentication must not notify"),
        )
        .unwrap();
        assert_eq!(token, "disabled");
    }

    #[test]
    fn wrong_or_missing_password_never_grants_session() {
        let gate = Mutex::new(());
        let mut config = password_config();
        for (username, password) in [("other", "original password"), ("operator", "wrong")] {
            assert!(login_with_config(
                &gate,
                || Ok(config.clone()),
                username,
                password,
                || panic!("invalid credentials must not grant a session"),
                || panic!("invalid credentials must not notify")
            )
            .is_err());
        }
        config.auth_password_verifier = None;
        assert!(login_with_config(
            &gate,
            || Ok(config),
            "operator",
            "original password",
            || panic!("missing verifier must fail closed"),
            || panic!("missing verifier must not notify")
        )
        .is_err());
    }

    #[test]
    fn privileged_commands_are_not_public() {
        for command in [
            "get_config",
            "save_config",
            "backup_config",
            "export_tariff",
            "restore_config",
            "perform_action",
            "get_setpoint_override",
            "set_setpoint_override",
            "get_plugin_snapshot",
            "plugin_action",
            "get_plugin_manager_snapshot",
            "get_plugin_settings",
            "get_plugin_settings_choices",
            "get_plugin_groups",
            "set_plugin_group_enabled",
            "save_plugin_settings",
            "get_retained_plugin_data",
            "delete_retained_plugin_data",
            "preview_plugin_package",
            "install_plugin_package",
            "discard_plugin_package",
            "set_plugin_enabled",
            "rollback_plugin_package",
            "uninstall_plugin_package",
            "connect_mqtt",
            "connect_gateway",
            "open_config_window",
            "auth_fake",
        ] {
            assert!(!public_command(command), "{command}");
        }
        assert!(public_command("auth_status"));
        assert!(public_command("auth_login"));
    }
    #[test]
    fn removed_provider_commands_have_no_public_authority() {
        for command in ["close_camera_video_window", "disconnect_ha_mqtt"] {
            assert!(!public_command(command));
        }
    }
    #[test]
    fn session_expires_at_deadline() {
        let now = Instant::now();
        let session = Session {
            token: "test".into(),
            created: now,
        };
        assert!(session.valid_at(now + SESSION_TTL - Duration::from_secs(1)));
        assert!(!session.valid_at(now + SESSION_TTL));
    }
    #[cfg(desktop)]
    #[test]
    fn recovery_authority_requires_current_policy_and_unexpired_session() {
        let now = Instant::now();
        let sessions = Mutex::new(None);
        let mut config = FullConfig::default();
        // Disabled authentication permits startup recovery without a login.
        let authority = session_authority_at(&config, &sessions, now).unwrap();
        assert!(sessions.try_lock().is_err());
        drop(authority);
        config.auth_enabled = Some(true);
        assert!(session_authority_at(&config, &sessions, now).is_err());
        *sessions.lock().unwrap() = Some(Session {
            token: "local-session".into(),
            created: now,
        });
        let authority = session_authority_at(
            &config,
            &sessions,
            now + SESSION_TTL - Duration::from_secs(1),
        )
        .unwrap();
        // Logout cannot replace the checked session until resume releases it.
        assert!(sessions.try_lock().is_err());
        drop(authority);
        assert!(session_authority_at(&config, &sessions, now + SESSION_TTL).is_err());
        assert!(sessions.lock().unwrap().is_none());
    }
    #[test]
    fn policy_changes_revoke_but_display_changes_do_not() {
        let before = FullConfig::default();
        let mut after = before.clone();
        after.color_scheme = Some("light".into());
        assert!(!policy_changed(&before, &after));
        after.auth_password = Some("new-password".into());
        assert!(policy_changed(&before, &after));
        after = before.clone();
        after.auth_biometric = Some(true);
        assert!(policy_changed(&before, &after));
    }
    #[test]
    fn biometric_requires_both_policy_flags() {
        let mut config = FullConfig {
            auth_enabled: Some(true),
            ..Default::default()
        };
        assert!(!biometric_enabled(&config));
        assert!(validate_policy(&config).is_err());
        config.auth_biometric = Some(true);
        assert!(biometric_enabled(&config));
        config.auth_enabled = Some(false);
        assert!(!biometric_enabled(&config));
    }
}
