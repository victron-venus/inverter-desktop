//! Recovery tests use local session/configuration substitutes, never Keychain.

use super::{recover_session, PackageApplication, PluginHost};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

struct Recovery {
    host: PluginHost,
    packages: PackageApplication,
    config_gate: Mutex<()>,
    gate: Mutex<()>,
    exiting: AtomicBool,
}

impl Recovery {
    fn new() -> Self {
        let host = PluginHost::default();
        let packages = PackageApplication::new(
            host.clone(),
            env!("INVERTER_DESKTOP_TARGET").into(),
            false,
            Arc::new(|| {}),
        );
        // Startup could not read the encrypted policy/configuration.
        assert!(packages
            .session_configured(false, Err("Configuration is unavailable".into()))
            .is_none());
        Self {
            host,
            packages,
            config_gate: Mutex::new(()),
            gate: Mutex::new(()),
            exiting: AtomicBool::new(false),
        }
    }

    fn refresh(&self, access: impl FnOnce() -> Result<(), String>) -> Option<u64> {
        recover_session(
            &self.host,
            &self.packages,
            &self.config_gate,
            &self.gate,
            &self.exiting,
            || {
                access()?;
                Ok((Vec::new(), ()))
            },
        )
    }
}

#[test]
fn failed_configuration_rechecks_leave_revoked_epoch_unchanged() {
    let recovery = Recovery::new();
    let epoch = recovery.host.authority_epoch();
    for error in ["Configuration is unavailable", "Authentication required"] {
        assert!(recovery.refresh(|| Err(error.into())).is_none());
        assert_eq!(recovery.host.authority_epoch(), epoch);
        assert!(!recovery.host.is_authorized_epoch(epoch));
    }
}

#[test]
fn concurrent_successful_polls_resume_once_and_healthy_polls_skip_reload() {
    let recovery = Arc::new(Recovery::new());
    let original_epoch = recovery.host.authority_epoch();
    let reads = AtomicUsize::new(0);
    let results = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            recovery.refresh(|| {
                reads.fetch_add(1, Ordering::AcqRel);
                Ok(())
            })
        });
        let second = scope.spawn(|| {
            recovery.refresh(|| {
                reads.fetch_add(1, Ordering::AcqRel);
                Ok(())
            })
        });
        [first.join().unwrap(), second.join().unwrap()]
    });
    assert_eq!(results.iter().filter(|epoch| epoch.is_some()).count(), 1);
    assert_eq!(reads.load(Ordering::Acquire), 1);
    let epoch = results.into_iter().flatten().next().unwrap();
    assert!(recovery.host.is_authorized_epoch(epoch));
    assert!(!recovery.host.is_authorized_epoch(original_epoch));
    assert!(recovery
        .packages
        .begin_selection("config", original_epoch)
        .is_err());
    assert!(recovery
        .refresh(|| panic!("healthy polling must not reload or restart"))
        .is_none());
    assert_eq!(recovery.host.authority_epoch(), epoch);
}

#[test]
fn delayed_refresh_reads_authority_after_completed_logout() {
    let recovery = Recovery::new();
    let unlocked = AtomicBool::new(true);
    let gate = recovery.gate.lock().unwrap();
    let (started, waiting) = std::sync::mpsc::channel();
    std::thread::scope(|scope| {
        let refresh = scope.spawn(|| {
            started.send(()).unwrap();
            recovery.refresh(|| {
                if unlocked.load(Ordering::Acquire) {
                    Ok(())
                } else {
                    Err("Authentication required".into())
                }
            })
        });
        waiting.recv().unwrap();
        unlocked.store(false, Ordering::Release);
        recovery.packages.session_configured(false, Ok(Vec::new()));
        let logout_epoch = recovery.host.authority_epoch();
        drop(gate);
        assert!(refresh.join().unwrap().is_none());
        assert_eq!(recovery.host.authority_epoch(), logout_epoch);
        assert!(!recovery.host.is_authorized_epoch(logout_epoch));
    });
}

#[test]
fn recovery_waits_for_policy_save_before_sampling_authority() {
    let recovery = Recovery::new();
    let auth_enabled = AtomicBool::new(false);
    let save = recovery.config_gate.lock().unwrap();
    let (started, waiting) = std::sync::mpsc::channel();
    std::thread::scope(|scope| {
        let refresh = scope.spawn(|| {
            started.send(()).unwrap();
            recovery.refresh(|| {
                assert!(recovery.config_gate.try_lock().is_err());
                if auth_enabled.load(Ordering::Acquire) {
                    Err("Authentication required".into())
                } else {
                    Ok(())
                }
            })
        });
        waiting.recv().unwrap();
        // Core saves retain this gate while persisting policy and notifying
        // the plugin host. Recovery must acquire these gates in the same order.
        auth_enabled.store(true, Ordering::Release);
        let transition = recovery.gate.lock().unwrap();
        recovery.packages.session_configured(false, Ok(Vec::new()));
        let policy_epoch = recovery.host.authority_epoch();
        drop(transition);
        drop(save);
        assert!(refresh.join().unwrap().is_none());
        assert_eq!(recovery.host.authority_epoch(), policy_epoch);
        assert!(!recovery.host.is_authorized_epoch(policy_epoch));
    });
}

#[test]
fn shutdown_before_or_during_revalidation_cannot_resume() {
    let recovery = Recovery::new();
    recovery.exiting.store(true, Ordering::Release);
    assert!(recovery
        .refresh(|| panic!("exit must not read configuration"))
        .is_none());

    let recovery = Recovery::new();
    assert!(recovery
        .refresh(|| {
            recovery.exiting.store(true, Ordering::Release);
            Ok(())
        })
        .is_none());
    assert!(!recovery
        .host
        .is_authorized_epoch(recovery.host.authority_epoch()));

    let recovery = Recovery::new();
    assert!(recovery
        .refresh(|| {
            // The package service also rejects recovery after terminal cleanup
            // begins, even if an exit retry has reset the bridge's flag.
            recovery.packages.begin_shutdown();
            Ok(())
        })
        .is_none());
    assert!(!recovery
        .host
        .is_authorized_epoch(recovery.host.authority_epoch()));
}
