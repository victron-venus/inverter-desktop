use super::*;
use crate::plugins::settings_store::SettingsData;

fn persist_config(
    saved: Arc<Mutex<crate::FullConfig>>,
    host: PluginHost,
    epoch: u64,
) -> impl FnMut(&str, PluginDesiredChange, &PackageApplication) -> Result<(), String> + Send {
    move |id, change, service| {
        host.commit_in_epoch(epoch, || {
            change.apply(&mut saved.lock().unwrap().desktop_plugins, id);
            service.plugin_desired_changed(id, change);
            Ok(())
        })
    }
}

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

fn core_fields(saved: &Mutex<crate::FullConfig>) -> Value {
    let mut value = serde_json::to_value(saved.lock().unwrap().clone()).unwrap();
    value.as_object_mut().unwrap().remove("desktop_plugins");
    value
}

async fn no_download_restore(service: &PackageApplication, epoch: u64) {
    service
        .reconcile_with(service.manager().unwrap(), epoch, |_| async {
            panic!("management intent must survive restore without another download")
        })
        .await
        .unwrap();
}

fn stored_settings(service: &PackageApplication, id: &str) -> SettingsData {
    let data = SettingsData {
        legacy_migration_version: 1,
        revision: uuid::Uuid::new_v4().to_string(),
        secrets: BTreeMap::from([("token".into(), "retained-fixture-credential".into())]),
        secret_fields: ["token".into()].into_iter().collect(),
        ..Default::default()
    };
    service
        .settings_store()
        .unwrap()
        .prepare_write(id, &data)
        .unwrap()
        .commit()
        .unwrap();
    data
}

#[tokio::test]
async fn managed_toggle_persists_and_reopens_without_restarting_unrelated_worker() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let target = grouped_declaration(&root, "test.selected", false);
    let other = grouped_declaration(&root, "test.other", false);
    let saved = saved_config(vec![target.0.clone(), other.0.clone()]);
    let unchanged = core_fields(&saved);
    let key = settings_test_key();
    let (service, host, epoch) =
        configured_application(&root, declarations(&saved), key.clone()).await;
    for (declaration, bytes) in [&target, &other] {
        install_group_fixture(&service, epoch, declaration, bytes).await;
    }
    let original = host
        .snapshots()
        .into_iter()
        .find(|entry| entry.plugin_id == other.0.plugin_id)
        .unwrap()
        .instance_id;
    for enabled in [false, true, false] {
        service
            .set_enabled_with_config(
                &target.0.plugin_id,
                enabled,
                epoch,
                persist_config(saved.clone(), host.clone(), epoch),
            )
            .await
            .unwrap();
        no_download_restore(&service, epoch).await;
        assert_eq!(declarations(&saved)[0].enabled, enabled);
        assert_eq!(declarations(&saved)[1], other.0);
        assert_eq!(core_fields(&saved), unchanged);
        assert_eq!(host.authority_epoch(), epoch);
        assert_eq!(
            host.snapshots()
                .into_iter()
                .find(|entry| entry.plugin_id == other.0.plugin_id)
                .unwrap()
                .instance_id,
            original
        );
        let record = service
            .manager()
            .unwrap()
            .list()
            .await
            .unwrap()
            .into_iter()
            .find(|entry| entry.plugin_id == target.0.plugin_id)
            .unwrap();
        assert_eq!(record.enabled, enabled);
    }
    service.close().await.unwrap();
    let (service, host, epoch) = configured_application(&root, declarations(&saved), key).await;
    no_download_restore(&service, epoch).await;
    assert!(host
        .snapshots()
        .iter()
        .all(|entry| entry.plugin_id != target.0.plugin_id));
    service.close().await.unwrap();
}

#[tokio::test]
async fn managed_uninstall_removes_only_selected_pin_and_retains_settings_across_reinstall() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let target = grouped_declaration(&root, "test.selected", false);
    let other = grouped_declaration(&root, "test.other", false);
    let saved = saved_config(vec![target.0.clone(), other.0.clone()]);
    let unchanged = core_fields(&saved);
    let key = settings_test_key();
    let (service, host, epoch) =
        configured_application(&root, declarations(&saved), key.clone()).await;
    for (declaration, bytes) in [&target, &other] {
        install_group_fixture(&service, epoch, declaration, bytes).await;
    }
    let retained = stored_settings(&service, &target.0.plugin_id);
    let other_instance = host
        .snapshots()
        .into_iter()
        .find(|entry| entry.plugin_id == other.0.plugin_id)
        .unwrap()
        .instance_id;
    service
        .uninstall_with_config(
            &target.0.plugin_id,
            false,
            epoch,
            persist_config(saved.clone(), host.clone(), epoch),
        )
        .await
        .unwrap();
    assert_eq!(declarations(&saved), vec![other.0.clone()]);
    assert_eq!(core_fields(&saved), unchanged);
    assert!(
        service
            .settings_store()
            .unwrap()
            .read(&target.0.plugin_id)
            .unwrap()
            == retained
    );
    no_download_restore(&service, epoch).await;
    assert_eq!(
        host.snapshots()
            .into_iter()
            .find(|entry| entry.plugin_id == other.0.plugin_id)
            .unwrap()
            .instance_id,
        other_instance
    );
    assert_eq!(service.manager().unwrap().list().await.unwrap().len(), 1);
    service.close().await.unwrap();

    let (service, host, epoch) = configured_application(&root, declarations(&saved), key).await;
    no_download_restore(&service, epoch).await;
    assert!(host
        .snapshots()
        .iter()
        .all(|entry| entry.plugin_id != target.0.plugin_id));
    assert!(
        service
            .settings_store()
            .unwrap()
            .read(&target.0.plugin_id)
            .unwrap()
            == retained
    );
    // A later explicit reinstall reuses the same encrypted namespace and marker.
    install_group_fixture(&service, epoch, &target.0, &target.1).await;
    assert!(
        service
            .settings_store()
            .unwrap()
            .read(&target.0.plugin_id)
            .unwrap()
            == retained
    );
    service
        .uninstall_with_config(
            &target.0.plugin_id,
            true,
            epoch,
            persist_config(saved.clone(), host.clone(), epoch),
        )
        .await
        .unwrap();
    assert!(
        service
            .settings_store()
            .unwrap()
            .read(&target.0.plugin_id)
            .unwrap()
            == SettingsData::default()
    );
    service.close().await.unwrap();
}

#[tokio::test]
async fn managed_persistence_failure_preserves_inventory_and_later_failure_keeps_disabled_pin() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let target = grouped_declaration(&root, "test.selected", false);
    let saved = saved_config(vec![target.0.clone()]);
    let (service, host, epoch) =
        configured_application(&root, declarations(&saved), settings_test_key()).await;
    install_group_fixture(&service, epoch, &target.0, &target.1).await;
    let retained = stored_settings(&service, &target.0.plugin_id);
    let original = host.snapshots()[0].instance_id.clone();
    assert!(service
        .set_enabled_with_config("test.missing", false, epoch, |_, _, _| panic!(
            "unknown identity must not persist"
        ))
        .await
        .is_err());
    assert!(service
        .set_enabled_with_config(&target.0.plugin_id, false, epoch, |_, _, _| Err(
            "save failed".into()
        ))
        .await
        .is_err());
    assert!(service
        .uninstall_with_config(&target.0.plugin_id, true, epoch, |_, _, _| Err(
            "save failed".into()
        ))
        .await
        .is_err());
    assert!(service.manager().unwrap().list().await.unwrap()[0].enabled);
    assert_eq!(host.snapshots()[0].instance_id, original);
    assert!(
        service
            .settings_store()
            .unwrap()
            .read(&target.0.plugin_id)
            .unwrap()
            == retained
    );
    let mut persist = persist_config(saved.clone(), host.clone(), epoch);
    assert!(service
        .uninstall_with_config(
            &target.0.plugin_id,
            true,
            epoch,
            move |id, change, packages| {
                if change == PluginDesiredChange::Removed {
                    return Err("pin removal save failed".into());
                }
                persist(id, change, packages)
            }
        )
        .await
        .is_err());
    assert!(!declarations(&saved)[0].enabled);
    assert!(!service.manager().unwrap().list().await.unwrap()[0].enabled);
    no_download_restore(&service, epoch).await;
    assert!(host
        .snapshots()
        .iter()
        .all(|entry| entry.state == WorkerState::Stopped));
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

#[tokio::test]
async fn managed_cleanup_failure_cannot_restart_an_unmanaged_remaining_package() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let target = grouped_declaration(&root, "test.selected", false);
    let saved = saved_config(vec![target.0.clone()]);
    let key = settings_test_key();
    let (service, host, epoch) =
        configured_application(&root, declarations(&saved), key.clone()).await;
    install_group_fixture(&service, epoch, &target.0, &target.1).await;
    stored_settings(&service, &target.0.plugin_id);
    let path = root.join("store/settings").join(format!(
        "{}.enc",
        SettingsStore::record_id(&target.0.plugin_id).unwrap()
    ));
    let held = root.join("settings-retained.enc");
    fs::rename(&path, &held).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(service
        .uninstall_with_config(
            &target.0.plugin_id,
            true,
            epoch,
            persist_config(saved.clone(), host.clone(), epoch)
        )
        .await
        .is_err());
    assert!(declarations(&saved).is_empty());
    assert!(!service.manager().unwrap().list().await.unwrap()[0].enabled);
    no_download_restore(&service, epoch).await;
    assert!(host
        .snapshots()
        .iter()
        .all(|entry| entry.state == WorkerState::Stopped));
    fs::remove_dir(&path).unwrap();
    fs::rename(&held, &path).unwrap();
    service.close().await.unwrap();
    let (service, host, epoch) = configured_application(&root, declarations(&saved), key).await;
    no_download_restore(&service, epoch).await;
    assert!(host.snapshots().is_empty());
    assert!(!service.manager().unwrap().list().await.unwrap()[0].enabled);
    service.close().await.unwrap();
}

#[tokio::test]
async fn managed_uninstall_waits_for_older_download_and_prevents_subsequent_restore() {
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
                    Ok(bytes)
                }
            })
            .await
    });
    started.notified().await;
    let removing = service.clone();
    let id = target.0.plugin_id.clone();
    let persist = persist_config(saved.clone(), host.clone(), epoch);
    let remove = tokio::spawn(async move {
        removing
            .uninstall_with_config(&id, false, epoch, persist)
            .await
    });
    tokio::time::timeout(Duration::from_secs(3), async {
        while !service.has_session_work() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(!remove.is_finished());
    assert_eq!(declarations(&saved), vec![target.0]);
    release.notify_one();
    restore.await.unwrap().unwrap();
    remove.await.unwrap().unwrap();
    assert!(declarations(&saved).is_empty());
    assert!(service.manager().unwrap().list().await.unwrap().is_empty());
    no_download_restore(&service, epoch).await;
    service.close().await.unwrap();
}

#[tokio::test]
async fn managed_owned_uninstall_survives_cancellation_but_rejected_waiters_never_persist() {
    for revoke in [0, 1, 2] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let target = grouped_declaration(&root, "test.selected", false);
        let saved = saved_config(vec![target.0.clone()]);
        let (service, host, epoch) =
            configured_application(&root, declarations(&saved), settings_test_key()).await;
        install_group_fixture(&service, epoch, &target.0, &target.1).await;
        let gate = service.0.reconciliation.operation.lock().await;
        let removing = service.clone();
        let persist = persist_config(saved.clone(), host.clone(), epoch);
        let task = tokio::spawn(async move {
            removing
                .uninstall_with_config(&target.0.plugin_id, false, epoch, persist)
                .await
        });
        tokio::time::timeout(Duration::from_secs(3), async {
            while !service.has_session_work() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        if revoke == 1 {
            service.session_configured(false, Ok(declarations(&saved)));
        }
        if revoke == 2 {
            service.begin_shutdown();
        }
        task.abort();
        drop(gate);
        tokio::time::timeout(Duration::from_secs(5), async {
            while service.has_session_work() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(declarations(&saved).is_empty(), revoke == 0);
        assert_eq!(
            service.manager().unwrap().list().await.unwrap().is_empty(),
            revoke == 0
        );
        service.close().await.unwrap();
    }
}
