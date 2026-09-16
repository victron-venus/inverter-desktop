use super::*;
use crate::plugins::application::management::declaration_revision;
use crate::plugins::settings_store::SettingsData;

fn saved_config(declarations: Vec<DesktopPluginConfig>) -> Arc<Mutex<crate::FullConfig>> {
    Arc::new(Mutex::new(crate::FullConfig {
        desktop_plugins: declarations,
        color_scheme: Some("dark".into()),
        setup_completed: true,
        ..Default::default()
    }))
}

fn declarations(saved: &Mutex<crate::FullConfig>) -> Vec<DesktopPluginConfig> {
    saved.lock().unwrap().desktop_plugins.clone()
}

fn persist_removal(
    saved: Arc<Mutex<crate::FullConfig>>,
    host: PluginHost,
    epoch: u64,
) -> impl FnOnce(&str, &str, &PackageApplication) -> Result<(), String> + Send {
    move |id, expected, service| {
        let mut config = saved.lock().unwrap();
        let mut next = config.clone();
        remove_configured_declaration(&mut next.desktop_plugins, id, expected)?;
        host.commit_in_epoch(epoch, || {
            *config = next;
            service.plugin_desired_changed(id, PluginDesiredChange::Removed);
            Ok(())
        })
    }
}

async fn no_download_restore(service: &PackageApplication, epoch: u64) {
    service
        .reconcile_with(service.manager().unwrap(), epoch, |_| async {
            panic!("removed declarations and cached unrelated packages must not download")
        })
        .await
        .unwrap();
}

async fn wait_for_activity(service: &PackageApplication, active: bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while service.has_session_work() != active {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn configured_removal_preserves_settings_other_pins_and_worker_across_cold_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let target = grouped_declaration(&root, "test.selected", false);
    let other = grouped_declaration(&root, "test.other", false);
    let saved = saved_config(vec![target.0.clone(), other.0.clone()]);
    let mut expected_config = saved.lock().unwrap().clone();
    expected_config.desktop_plugins.remove(0);
    let key = settings_test_key();
    let (service, host, epoch) =
        configured_application(&root, declarations(&saved), key.clone()).await;
    install_group_fixture(&service, epoch, &other.0, &other.1).await;
    let other_instance = host.snapshots()[0].instance_id.clone();
    let retained = SettingsData {
        legacy_migration_version: 1,
        revision: uuid::Uuid::new_v4().to_string(),
        secrets: BTreeMap::from([("token".into(), "retained-fixture-credential".into())]),
        secret_fields: ["token".into()].into_iter().collect(),
        ..Default::default()
    };
    let store = service.settings_store().unwrap();
    store
        .prepare_write(&target.0.plugin_id, &retained)
        .unwrap()
        .commit()
        .unwrap();
    let record_path = root.join("store/settings").join(format!(
        "{}.enc",
        SettingsStore::record_id(&target.0.plugin_id).unwrap()
    ));
    let ciphertext = fs::read(&record_path).unwrap();
    service.configured_status(&target.0.plugin_id, epoch, "failed", Some("offline".into()));
    service
        .remove_configured_with_config(
            &target.0.plugin_id,
            &declaration_revision(&target.0),
            epoch,
            persist_removal(saved.clone(), host.clone(), epoch),
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(saved.lock().unwrap().clone()).unwrap(),
        serde_json::to_value(expected_config).unwrap()
    );
    assert_eq!(fs::read(&record_path).unwrap(), ciphertext);
    assert!(store.read(&target.0.plugin_id).unwrap() == retained);
    no_download_restore(&service, epoch).await;
    assert_eq!(host.snapshots()[0].instance_id, other_instance);
    assert_eq!(service.manager().unwrap().list().await.unwrap().len(), 1);
    assert_eq!(service.snapshot(epoch).await.unwrap().configured.len(), 1);
    service.close().await.unwrap();

    let (service, host, epoch) = configured_application(&root, declarations(&saved), key).await;
    no_download_restore(&service, epoch).await;
    assert_eq!(host.snapshots().len(), 1);
    assert_eq!(host.snapshots()[0].plugin_id, other.0.plugin_id);
    assert_eq!(fs::read(record_path).unwrap(), ciphertext);
    assert!(
        service
            .settings_store()
            .unwrap()
            .read(&target.0.plugin_id)
            .unwrap()
            == retained
    );
    service.close().await.unwrap();
}

#[test]
fn configured_revision_binds_every_field_and_preserved_future_metadata() {
    let original = crate::plugin_config::test_declarations()[0].clone();
    let baseline = declaration_revision(&original);
    assert_eq!(baseline.len(), 64);
    let mut variants = Vec::new();
    let mut changed = original.clone();
    changed.enabled = !changed.enabled;
    variants.push(changed);
    let mut changed = original.clone();
    changed.version = "2.0.0".into();
    variants.push(changed);
    let mut changed = original.clone();
    changed.artifacts.values_mut().next().unwrap().url =
        "https://other.invalid/archive.idplugin".into();
    variants.push(changed);
    let mut changed = original.clone();
    changed.artifacts.values_mut().next().unwrap().sha256 = "b".repeat(64);
    variants.push(changed);
    let mut changed = original.clone();
    changed
        .artifacts
        .values_mut()
        .next()
        .unwrap()
        .extra
        .insert("future".into(), json!({"revision": 2}));
    variants.push(changed);
    let mut changed = original.clone();
    changed
        .extra
        .insert("future".into(), json!({"revision": 2}));
    variants.push(changed);
    let mut changed = original.clone();
    changed.artifacts.pop_first();
    variants.push(changed);
    for changed in variants {
        assert_ne!(declaration_revision(&changed), baseline);
        let mut current = vec![changed.clone()];
        assert!(
            remove_configured_declaration(&mut current, &original.plugin_id, &baseline).is_err()
        );
        assert_eq!(current, vec![changed]);
    }
    let roundtrip: DesktopPluginConfig =
        serde_json::from_slice(&serde_json::to_vec(&original).unwrap()).unwrap();
    assert_eq!(declaration_revision(&roundtrip), baseline);
    assert!(
        remove_configured_declaration(&mut Vec::new(), &original.plugin_id, &baseline).is_err()
    );
}

#[tokio::test]
async fn configured_removal_checks_authoritative_same_version_repin_and_absent_declaration() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let target = grouped_declaration(&root, "test.selected", false);
    let saved = saved_config(vec![target.0.clone()]);
    let (service, host, epoch) =
        configured_application(&root, declarations(&saved), settings_test_key()).await;
    // Simulate a persisted re-pin before its new in-memory snapshot is visible.
    saved.lock().unwrap().desktop_plugins[0]
        .artifacts
        .values_mut()
        .next()
        .unwrap()
        .sha256 = "b".repeat(64);
    let repinned = declarations(&saved);
    let error = service
        .remove_configured_with_config(
            &target.0.plugin_id,
            &declaration_revision(&target.0),
            epoch,
            persist_removal(saved.clone(), host.clone(), epoch),
        )
        .await
        .unwrap_err();
    assert!(error.contains("changed"));
    assert_eq!(declarations(&saved), repinned);
    assert_eq!(service.snapshot(epoch).await.unwrap().configured.len(), 1);
    assert!(service.manager().unwrap().list().await.unwrap().is_empty());
    saved.lock().unwrap().desktop_plugins.clear();
    let error = service
        .remove_configured_with_config(
            &target.0.plugin_id,
            &declaration_revision(&target.0),
            epoch,
            persist_removal(saved.clone(), host.clone(), epoch),
        )
        .await
        .unwrap_err();
    assert!(error.contains("no longer configured"));
    service.close().await.unwrap();
}

#[tokio::test]
async fn configured_removal_save_failure_and_commit_revocation_preserve_desired_intent() {
    for revoke in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let target = grouped_declaration(&root, "test.selected", false);
        let saved = saved_config(vec![target.0.clone()]);
        let (service, host, epoch) =
            configured_application(&root, declarations(&saved), settings_test_key()).await;
        let authority = host.clone();
        let saved_callback = saved.clone();
        let result = service
            .remove_configured_with_config(
                &target.0.plugin_id,
                &declaration_revision(&target.0),
                epoch,
                move |id, expected, packages| {
                    let mut next = saved_callback.lock().unwrap().clone();
                    remove_configured_declaration(&mut next.desktop_plugins, id, expected)?;
                    if revoke {
                        packages.session_configured(false, Ok(declarations(&saved_callback)));
                    }
                    authority.commit_in_epoch(epoch, || {
                        if !revoke {
                            return Err("config save failed".into());
                        }
                        *saved_callback.lock().unwrap() = next;
                        packages.plugin_desired_changed(id, PluginDesiredChange::Removed);
                        Ok(())
                    })
                },
            )
            .await;
        assert!(result.is_err());
        assert_eq!(declarations(&saved), vec![target.0.clone()]);
        assert!(service.0.reconciliation.contains(&target.0.plugin_id));
        assert!(service.manager().unwrap().list().await.unwrap().is_empty());
        service.close().await.unwrap();
    }
}

#[tokio::test]
async fn configured_removal_waits_for_download_and_rejects_an_install_that_wins() {
    for download_succeeds in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let target = grouped_declaration(&root, "test.selected", false);
        let saved = saved_config(vec![target.0.clone()]);
        let (service, host, epoch) =
            configured_application(&root, declarations(&saved), settings_test_key()).await;
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let restoring = service.clone();
        let announced = started.clone();
        let blocked = release.clone();
        let bytes = target.1;
        let restore = tokio::spawn(async move {
            restoring
                .reconcile_with(restoring.manager().unwrap(), epoch, |_| {
                    announced.notify_one();
                    let blocked = blocked.clone();
                    let bytes = bytes.clone();
                    async move {
                        blocked.notified().await;
                        if download_succeeds {
                            Ok(bytes)
                        } else {
                            Err("offline fixture".into())
                        }
                    }
                })
                .await
        });
        started.notified().await;
        let removing = service.clone();
        let id = target.0.plugin_id.clone();
        let revision = declaration_revision(&target.0);
        let persist = persist_removal(saved.clone(), host.clone(), epoch);
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        let remove = tokio::spawn(async move {
            removing
                .remove_configured_with_config(
                    &id,
                    &revision,
                    epoch,
                    move |id, revision, packages| {
                        called.fetch_add(1, Ordering::SeqCst);
                        persist(id, revision, packages)
                    },
                )
                .await
        });
        wait_for_activity(&service, true).await;
        assert!(!remove.is_finished());
        assert_eq!(declarations(&saved), vec![target.0]);
        release.notify_one();
        restore.await.unwrap().unwrap();
        let result = remove.await.unwrap();
        if download_succeeds {
            assert!(result.unwrap_err().contains("installed"));
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            assert_eq!(declarations(&saved).len(), 1);
            assert_eq!(service.manager().unwrap().list().await.unwrap().len(), 1);
        } else {
            result.unwrap();
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert!(declarations(&saved).is_empty());
            assert!(service.manager().unwrap().list().await.unwrap().is_empty());
        }
        no_download_restore(&service, epoch).await;
        service.close().await.unwrap();
    }
}

#[tokio::test]
async fn configured_removal_owned_task_survives_cancellation_but_not_revocation_or_shutdown() {
    for revoke in [0, 1, 2] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let target = grouped_declaration(&root, "test.selected", false);
        let saved = saved_config(vec![target.0.clone()]);
        let (service, host, epoch) =
            configured_application(&root, declarations(&saved), settings_test_key()).await;
        let gate = service.0.reconciliation.operation.lock().await;
        let removing = service.clone();
        let id = target.0.plugin_id.clone();
        let revision = declaration_revision(&target.0);
        let persist = persist_removal(saved.clone(), host.clone(), epoch);
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        let task = tokio::spawn(async move {
            removing
                .remove_configured_with_config(
                    &id,
                    &revision,
                    epoch,
                    move |id, revision, packages| {
                        called.fetch_add(1, Ordering::SeqCst);
                        persist(id, revision, packages)
                    },
                )
                .await
        });
        wait_for_activity(&service, true).await;
        if revoke == 1 {
            service.session_configured(false, Ok(declarations(&saved)));
        }
        if revoke == 2 {
            service.begin_shutdown();
        }
        task.abort();
        drop(gate);
        wait_for_activity(&service, false).await;
        assert_eq!(calls.load(Ordering::SeqCst), usize::from(revoke == 0));
        assert_eq!(declarations(&saved).is_empty(), revoke == 0);
        assert!(service.manager().unwrap().list().await.unwrap().is_empty());
        service.close().await.unwrap();
    }
}

#[tokio::test]
async fn configured_status_revision_tracks_enabled_changes_and_survives_failed_status() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let (mut declaration, _) = declared_package(&root, "1.0.0", None);
    let (service, host, epoch) =
        configured_application(&root, vec![declaration.clone()], settings_test_key()).await;
    let original = declaration_revision(&declaration);
    assert_eq!(
        configured_status(&service, epoch).await["declaration_revision"],
        original
    );
    host.commit_in_epoch(epoch, || {
        service.group_desired_changed(&[declaration.plugin_id.clone()], false);
        Ok(())
    })
    .unwrap();
    declaration.enabled = false;
    let disabled = declaration_revision(&declaration);
    assert_ne!(original, disabled);
    assert_eq!(
        configured_status(&service, epoch).await["declaration_revision"],
        disabled
    );
    service.configured_status(
        &declaration.plugin_id,
        epoch,
        "failed",
        Some("offline".into()),
    );
    assert_eq!(
        configured_status(&service, epoch).await["declaration_revision"],
        disabled
    );
    host.commit_in_epoch(epoch, || {
        service.plugin_desired_changed(&declaration.plugin_id, PluginDesiredChange::Enabled(true));
        Ok(())
    })
    .unwrap();
    assert_eq!(
        configured_status(&service, epoch).await["declaration_revision"],
        original
    );
    service.close().await.unwrap();
}
