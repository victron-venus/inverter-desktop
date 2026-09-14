use super::*;
use crate::plugins::package::PublisherTrust;
use crate::plugins::packaging::build_package;
use crate::plugins::protocol::{PluginManifest, PluginPermission};
use crate::plugins::runtime::WorkerState;
use ed25519_dalek::SigningKey;
use serde_json::json;
use std::fs;
use std::path::Path;
use std::sync::OnceLock;

const PLUGIN: &str = "test.application";
const PUBLISHER: &str = "application-tests-only";

fn trust() -> TrustStore {
    TrustStore::new(vec![PublisherTrust::new(
        PUBLISHER.into(),
        SigningKey::from_bytes(&[29; 32]).verifying_key().to_bytes(),
        vec![PLUGIN.into()],
    )
    .unwrap()])
    .unwrap()
}

fn fixture() -> PathBuf {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY
        .get_or_init(|| {
            let directory = std::env::temp_dir().join(format!(
                "inverter-application-worker-{}",
                std::process::id()
            ));
            fs::create_dir_all(&directory).unwrap();
            let binary = directory.join(format!("worker{}", std::env::consts::EXE_SUFFIX));
            let status = std::process::Command::new(
                std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()),
            )
            .arg("--edition=2021")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/plugin_worker.rs"))
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
            assert!(
                status.status.success(),
                "{}",
                String::from_utf8_lossy(&status.stderr)
            );
            binary
        })
        .clone()
}

fn archive(directory: &Path, version: &str) -> PathBuf {
    configured_archive(directory, version, None, "worker")
}

fn configured_archive(
    directory: &Path,
    version: &str,
    schema: Option<Value>,
    mode: &str,
) -> PathBuf {
    let source = directory.join(format!("payload-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&source).unwrap();
    let entrypoint = format!("{mode}{}", std::env::consts::EXE_SUFFIX);
    fs::copy(fixture(), source.join(&entrypoint)).unwrap();
    let metadata = PluginManifest {
        schema_version: 1,
        plugin_id: PLUGIN.into(),
        version: version.into(),
        host_api: "^1.0".into(),
        target: env!("INVERTER_DESKTOP_TARGET").into(),
        entrypoint,
        permissions: if schema.is_some() {
            vec![
                PluginPermission::DashboardContributions,
                PluginPermission::PluginConfiguration,
            ]
        } else {
            vec![PluginPermission::DashboardContributions]
        },
        config_schema: schema.unwrap_or_else(|| json!({"type":"object"})),
        inventory: Vec::new(),
        signature: None,
    };
    let bytes = build_package(
        metadata,
        &source,
        PUBLISHER,
        &SigningKey::from_bytes(&[29; 32]),
    )
    .unwrap();
    let path = directory.join(format!("{}.idplugin", uuid::Uuid::new_v4()));
    fs::write(&path, bytes).unwrap();
    path
}

async fn application(directory: &Path) -> (PackageApplication, PluginHost, u64) {
    let host = PluginHost::default();
    let service = PackageApplication::new(
        host.clone(),
        env!("INVERTER_DESKTOP_TARGET").into(),
        true,
        Arc::new(|| {}),
    );
    service
        .initialize(Ok(directory.join("store")), Ok(trust()))
        .await;
    let epoch = service.session_changed(true).unwrap();
    (service, host, epoch)
}

async fn review(service: &PackageApplication, path: PathBuf, epoch: u64) -> PackagePreview {
    let token = service.begin_selection("config", epoch).unwrap();
    service
        .finish_selection(&token, "config", epoch, path)
        .await
        .unwrap()
}

async fn install(service: &PackageApplication, path: PathBuf, epoch: u64, enabled: bool) {
    let preview = review(service, path, epoch).await;
    service
        .install_review(&preview.token, "config", epoch, enabled)
        .await
        .unwrap();
}

#[tokio::test]
async fn cancelled_caller_keeps_session_expiry_active_until_transaction_finishes() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let (service, host, epoch) = application(&root).await;
    let manager = service.manager().unwrap();
    let package = manager
        .inspect_archive(archive(&root, "1.0.0"), epoch)
        .await
        .unwrap();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let (finished_tx, finished_rx) = tokio::sync::oneshot::channel();
    let caller_service = service.clone();
    let caller = tokio::spawn(async move {
        let activity = caller_service.begin_activity();
        caller_service
            .run_operation(PLUGIN.into(), epoch, activity, async move {
                started_tx.send(()).unwrap();
                release_rx.await.unwrap();
                let result = manager
                    .install_verified_in_epoch(package, false, epoch)
                    .await
                    .map(|_| ());
                finished_tx.send(result.clone()).unwrap();
                result
            })
            .await
    });
    started_rx.await.unwrap();
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    service.window_closed("config");
    assert!(host.snapshots().is_empty());
    assert!(service.0.pending.lock().unwrap().is_none());
    assert!(service.has_session_work());
    // The bridge watchdog continues checking auth even with no preview or worker.
    service.session_changed(false);
    release_tx.send(()).unwrap();
    assert!(finished_rx.await.unwrap().is_err());
    tokio::time::timeout(Duration::from_secs(2), async {
        while service.has_session_work() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let next = service.session_changed(true).unwrap();
    assert!(service.snapshot(next).await.unwrap().plugins.is_empty());
    service.close().await.unwrap();
}

#[tokio::test]
async fn reviewed_bytes_survive_file_replacement_and_consent_is_single_use() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let (service, host, epoch) = application(&root).await;
    let original = archive(&root, "1.0.0");
    let preview = review(&service, original.clone(), epoch).await;
    assert_eq!(preview.version, "1.0.0");
    assert_eq!(preview.publisher_key_id, PUBLISHER);
    fs::copy(archive(&root, "2.0.0"), original).unwrap();
    let (first, second) = tokio::join!(
        service.install_review(&preview.token, "config", epoch, true),
        service.install_review(&preview.token, "config", epoch, true),
    );
    assert_ne!(first.is_ok(), second.is_ok());
    let snapshot = service.snapshot(epoch).await.unwrap();
    assert_eq!(snapshot.plugins[0].version, "1.0.0");
    assert_eq!(
        snapshot.plugins[0].runtime.as_ref().unwrap().state,
        WorkerState::Running
    );
    assert!(!host.snapshots().is_empty());
    service.uninstall(PLUGIN, epoch).await.unwrap();
    service.close().await.unwrap();
}

#[tokio::test]
async fn delayed_dialog_and_review_cannot_cross_logout_or_closed_window() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let (service, _, epoch) = application(&root).await;
    let path = archive(&root, "1.0.0");
    let token = service.begin_selection("config", epoch).unwrap();
    assert!(service.begin_selection("config", epoch).is_err());
    service.session_changed(false);
    let next = service.session_changed(true).unwrap();
    assert!(service
        .finish_selection(&token, "config", epoch, path.clone())
        .await
        .is_err());
    let current = review(&service, path.clone(), next).await;
    assert!(service
        .install_review(&current.token, "main", next, true)
        .await
        .is_err());
    service.window_closed("config");
    assert!(service
        .install_review(&current.token, "config", next, true)
        .await
        .is_err());
    let pending = service.begin_selection("config", next).unwrap();
    service.window_closed("config");
    assert!(service
        .finish_selection(&pending, "config", next, path)
        .await
        .is_err());
    assert!(service.snapshot(next).await.unwrap().plugins.is_empty());
    service.close().await.unwrap();
}

#[tokio::test]
async fn preview_replacement_expiry_and_cancel_never_reuse_old_consent() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let (service, _, epoch) = application(&root).await;
    let path = archive(&root, "1.0.0");
    let first = review(&service, path.clone(), epoch).await;
    let next = review(&service, path.clone(), epoch).await;
    service.discard(&first.token, "config", epoch);
    assert!(service
        .install_review(&first.token, "config", epoch, false)
        .await
        .is_err());
    assert!(service
        .finish_selection(&next.token, "config", epoch, path)
        .await
        .is_err());
    service.0.pending.lock().unwrap().as_mut().unwrap().created = Instant::now() - PREVIEW_TTL;
    assert!(service
        .install_review(&next.token, "config", epoch, false)
        .await
        .is_err());
    service.expire_preview();
    assert!(service.0.pending.lock().unwrap().is_none());
    service.close().await.unwrap();
}

#[tokio::test]
async fn authenticated_restart_restores_enabled_only_and_shutdown_releases_lease() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let (service, host, epoch) = application(&root).await;
    install(&service, archive(&root, "1.0.0"), epoch, true).await;
    service.session_changed(false);
    assert!(host
        .snapshots()
        .iter()
        .all(|worker| worker.contributions.is_empty()));
    let next = service.session_changed(true).unwrap();
    service.restore(next).await.unwrap();
    assert_eq!(
        service.snapshot(next).await.unwrap().plugins[0]
            .runtime
            .as_ref()
            .unwrap()
            .state,
        WorkerState::Running
    );
    service.set_enabled(PLUGIN, false, next).await.unwrap();
    service.close().await.unwrap();
    let (reopened, second_host, second_epoch) = application(&root).await;
    reopened.restore(second_epoch).await.unwrap();
    assert!(second_host.snapshots().is_empty());
    assert!(!reopened.snapshot(second_epoch).await.unwrap().plugins[0].enabled);
    reopened.uninstall(PLUGIN, second_epoch).await.unwrap();
    let (left, right) = tokio::join!(reopened.close(), reopened.close());
    left.unwrap();
    right.unwrap();
}

#[tokio::test]
async fn package_mutations_invalidate_review_and_refresh_metadata_without_core_config() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let legacy = root.join("legacy-settings.json");
    fs::write(
        &legacy,
        br#"{"ha_url":"https://example.invalid","camera_enabled":true}"#,
    )
    .unwrap();
    let before = fs::read(&legacy).unwrap();
    let (service, _, epoch) = application(&root).await;
    service.restore(epoch).await.unwrap();
    assert!(service.snapshot(epoch).await.unwrap().plugins.is_empty());
    install(&service, archive(&root, "1.0.0"), epoch, false).await;
    assert_eq!(
        service.snapshot(epoch).await.unwrap().plugins[0].version,
        "1.0.0"
    );
    let preview = review(&service, archive(&root, "2.0.0"), epoch).await;
    service.set_enabled(PLUGIN, true, epoch).await.unwrap();
    assert!(service
        .install_review(&preview.token, "config", epoch, true)
        .await
        .is_err());
    install(&service, archive(&root, "2.0.0"), epoch, true).await;
    assert_eq!(
        service.snapshot(epoch).await.unwrap().plugins[0]
            .rollback_version
            .as_deref(),
        Some("1.0.0")
    );
    service.rollback(PLUGIN, epoch).await.unwrap();
    assert_eq!(
        service.snapshot(epoch).await.unwrap().plugins[0].version,
        "1.0.0"
    );
    service.uninstall(PLUGIN, epoch).await.unwrap();
    assert!(service.snapshot(epoch).await.unwrap().plugins.is_empty());
    assert_eq!(fs::read(legacy).unwrap(), before);
    service.close().await.unwrap();
}

#[tokio::test]
async fn initial_auth_restore_and_quit_wait_for_store_initialization() {
    let directory = tempfile::tempdir().unwrap();
    let host = PluginHost::default();
    let service = PackageApplication::new(
        host.clone(),
        env!("INVERTER_DESKTOP_TARGET").into(),
        false,
        Arc::new(|| {}),
    );
    let epoch = service.session_changed(true).unwrap();
    let restore = tokio::spawn({
        let service = service.clone();
        async move { service.restore(epoch).await }
    });
    let close = tokio::spawn({
        let service = service.clone();
        async move { service.close().await }
    });
    tokio::task::yield_now().await;
    assert!(!close.is_finished());
    service
        .initialize(
            Ok(directory.path().join("store")),
            Ok(TrustStore::default()),
        )
        .await;
    close.await.unwrap().unwrap();
    assert!(restore.await.unwrap().is_err());
    assert!(host.snapshots().is_empty());
}

#[tokio::test]
async fn empty_policy_and_store_failure_are_visible_without_installation_fallback() {
    let directory = tempfile::tempdir().unwrap();
    let host = PluginHost::default();
    let service = PackageApplication::new(
        host,
        env!("INVERTER_DESKTOP_TARGET").into(),
        false,
        Arc::new(|| {}),
    );
    service
        .initialize(
            Ok(directory.path().join("store")),
            Ok(TrustStore::default()),
        )
        .await;
    let epoch = service.session_changed(true).unwrap();
    let snapshot = service.snapshot(epoch).await.unwrap();
    assert!(snapshot.ready);
    assert!(!snapshot.installation_available);
    assert!(service.begin_selection("config", epoch).is_err());
    service.close().await.unwrap();
    let failed = PackageApplication::new(
        PluginHost::default(),
        env!("INVERTER_DESKTOP_TARGET").into(),
        false,
        Arc::new(|| {}),
    );
    failed
        .initialize(
            Err("Store location unavailable".into()),
            Ok(TrustStore::default()),
        )
        .await;
    let epoch = failed.session_changed(true).unwrap();
    let snapshot = failed.snapshot(epoch).await.unwrap();
    assert!(!snapshot.ready);
    assert_eq!(
        snapshot.error.as_deref(),
        Some("Store location unavailable")
    );
    failed.close().await.unwrap();
}

fn settings_schema() -> Value {
    json!({"type":"object","additionalProperties":false,"properties":{
        "endpoint":{"type":"string","default":"https://example.invalid"},
        "token":{"type":"string","writeOnly":true,"minLength":1}
    },"required":["token"]})
}

fn settings_test_key() -> SettingsKeyProvider {
    let key = rand::random::<[u8; 32]>();
    Arc::new(move || Ok(key.to_vec()))
}

async fn settings_application(
    directory: &Path,
    key: SettingsKeyProvider,
) -> (PackageApplication, PluginHost, u64) {
    let host = PluginHost::default();
    let service = PackageApplication::new(
        host.clone(),
        env!("INVERTER_DESKTOP_TARGET").into(),
        true,
        Arc::new(|| {}),
    );
    service
        .initialize_with_key(Ok(directory.join("store")), Ok(trust()), key)
        .await;
    let epoch = service.session_changed(true).unwrap();
    (service, host, epoch)
}

async fn settings_view(service: &PackageApplication, epoch: u64) -> Value {
    serde_json::to_value(service.get_settings(PLUGIN, epoch).await.unwrap()).unwrap()
}

async fn save_fixture_settings(
    service: &PackageApplication,
    epoch: u64,
    revision: &Value,
) -> SettingsSaveResult {
    service
        .save_settings(
            PLUGIN,
            epoch,
            revision.as_str().unwrap().into(),
            BTreeMap::new(),
            BTreeMap::from([("token".into(), Some("fixture-secret".into()))]),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn settings_save_keeps_disabled_workers_stopped_and_restarts_enabled_workers() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let (service, host, epoch) = settings_application(&root, settings_test_key()).await;
    install(
        &service,
        configured_archive(&root, "1.0.0", Some(settings_schema()), "configuration"),
        epoch,
        false,
    )
    .await;
    let initial = settings_view(&service, epoch).await;
    assert_eq!(initial["secret_present"]["token"], false);
    let saved = save_fixture_settings(&service, epoch, &initial["revision"]).await;
    assert!(saved.restart_error.is_none());
    assert!(host.snapshots().is_empty());
    let public = serde_json::to_value(saved).unwrap();
    assert_eq!(public["settings"]["secret_present"]["token"], true);
    assert!(!public.to_string().contains("fixture-secret"));
    service.set_enabled(PLUGIN, true, epoch).await.unwrap();
    let before = host
        .action_in_epoch(PLUGIN, "echo", json!({}), Duration::from_secs(2), epoch)
        .await
        .unwrap();
    assert_eq!(before["configuration_secret_matches"], true);
    let revision = &public["settings"]["revision"];
    let saved = service
        .save_settings(
            PLUGIN,
            epoch,
            revision.as_str().unwrap().into(),
            BTreeMap::from([("endpoint".into(), json!("https://updated.invalid"))]),
            BTreeMap::new(),
        )
        .await
        .unwrap();
    assert!(saved.restart_error.is_none());
    let after = host
        .action_in_epoch(PLUGIN, "echo", json!({}), Duration::from_secs(2), epoch)
        .await
        .unwrap();
    assert_ne!(before["pid"], after["pid"]);
    assert_ne!(
        before["configuration_revision"],
        after["configuration_revision"]
    );
    assert_eq!(after["configuration_secret_matches"], true);
    service.session_changed(false);
    assert!(service.get_settings(PLUGIN, epoch).await.is_err());
    let next_epoch = service.session_changed(true).unwrap();
    service.restore(next_epoch).await.unwrap();
    let restored = host
        .action_in_epoch(
            PLUGIN,
            "echo",
            json!({}),
            Duration::from_secs(2),
            next_epoch,
        )
        .await
        .unwrap();
    assert_eq!(
        restored["configuration_revision"],
        after["configuration_revision"]
    );
    service.close().await.unwrap();
}

#[tokio::test]
async fn stale_settings_and_package_revisions_cannot_overwrite_current_values() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let (service, _, epoch) = settings_application(&root, settings_test_key()).await;
    install(
        &service,
        configured_archive(&root, "1.0.0", Some(settings_schema()), "configuration"),
        epoch,
        false,
    )
    .await;
    let initial = settings_view(&service, epoch).await;
    save_fixture_settings(&service, epoch, &initial["revision"]).await;
    assert!(service
        .save_settings(
            PLUGIN,
            epoch,
            initial["revision"].as_str().unwrap().into(),
            BTreeMap::new(),
            BTreeMap::new()
        )
        .await
        .is_err());
    let current = settings_view(&service, epoch).await;
    install(
        &service,
        configured_archive(&root, "1.1.0", Some(settings_schema()), "configuration"),
        epoch,
        false,
    )
    .await;
    assert!(service
        .save_settings(
            PLUGIN,
            epoch,
            current["revision"].as_str().unwrap().into(),
            BTreeMap::new(),
            BTreeMap::new()
        )
        .await
        .is_err());
    let updated = settings_view(&service, epoch).await;
    assert_eq!(updated["version"], "1.1.0");
    assert_eq!(updated["secret_present"]["token"], true);
    assert_ne!(updated["revision"], current["revision"]);
    service.close().await.unwrap();
}

#[tokio::test]
async fn uninstall_retains_settings_unless_explicitly_deleted() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let (service, _, epoch) = settings_application(&root, settings_test_key()).await;
    let path = configured_archive(&root, "1.0.0", Some(settings_schema()), "configuration");
    install(&service, path.clone(), epoch, false).await;
    let initial = settings_view(&service, epoch).await;
    save_fixture_settings(&service, epoch, &initial["revision"]).await;
    service
        .uninstall_with_settings(PLUGIN, false, epoch)
        .await
        .unwrap();
    assert_eq!(
        service
            .settings_store()
            .unwrap()
            .read(PLUGIN)
            .unwrap()
            .secrets["token"],
        "fixture-secret"
    );
    assert!(service.get_settings(PLUGIN, epoch).await.is_err());
    install(&service, path.clone(), epoch, false).await;
    assert_eq!(
        settings_view(&service, epoch).await["secret_present"]["token"],
        true
    );
    service
        .uninstall_with_settings(PLUGIN, true, epoch)
        .await
        .unwrap();
    assert!(service
        .settings_store()
        .unwrap()
        .read(PLUGIN)
        .unwrap()
        .secrets
        .is_empty());
    install(&service, path, epoch, false).await;
    assert_eq!(
        settings_view(&service, epoch).await["secret_present"]["token"],
        false
    );
    service.close().await.unwrap();
}

#[tokio::test]
async fn saved_settings_survive_restart_failure_and_enabled_intent_is_preserved() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let fail_at = Arc::new(AtomicUsize::new(usize::MAX));
    let key_calls = calls.clone();
    let key_failure = fail_at.clone();
    let test_key = rand::random::<[u8; 32]>();
    let (service, host, epoch) = settings_application(
        &root,
        Arc::new(move || {
            if key_calls.fetch_add(1, Ordering::SeqCst) >= key_failure.load(Ordering::SeqCst) {
                Err("provider unavailable".into())
            } else {
                Ok(test_key.to_vec())
            }
        }),
    )
    .await;
    install(
        &service,
        configured_archive(&root, "1.0.0", Some(settings_schema()), "configuration"),
        epoch,
        false,
    )
    .await;
    let initial = settings_view(&service, epoch).await;
    save_fixture_settings(&service, epoch, &initial["revision"]).await;
    service.set_enabled(PLUGIN, true, epoch).await.unwrap();
    let current = settings_view(&service, epoch).await;
    // Reading the current record and preparing ciphertext succeed. Restart's
    // credential lookup fails after the authorized rename has committed.
    fail_at.store(calls.load(Ordering::SeqCst) + 2, Ordering::SeqCst);
    let saved = service
        .save_settings(
            PLUGIN,
            epoch,
            current["revision"].as_str().unwrap().into(),
            BTreeMap::from([("endpoint".into(), json!("https://persisted.invalid"))]),
            BTreeMap::new(),
        )
        .await
        .unwrap();
    assert!(saved.restart_error.is_some());
    assert!(host.snapshots().is_empty());
    assert!(service.manager().unwrap().list().await.unwrap()[0].enabled);
    fail_at.store(usize::MAX, Ordering::SeqCst);
    assert_eq!(
        settings_view(&service, epoch).await["values"]["endpoint"],
        "https://persisted.invalid"
    );
    service.set_enabled(PLUGIN, true, epoch).await.unwrap();
    assert_eq!(host.snapshots()[0].state, WorkerState::Running);
    service.close().await.unwrap();
}
