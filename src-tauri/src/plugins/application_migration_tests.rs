use super::*;

fn seed_data(current: &SettingsData) -> SettingsData {
    if current.legacy_migration_version >= 1 {
        return current.clone();
    }
    let mut next = current.clone();
    next.legacy_migration_version = 1;
    next.revision = uuid::Uuid::new_v4().to_string();
    next.values
        .insert("endpoint".into(), json!("https://restored.example.invalid"));
    next
}

async fn seeded_application(
    root: &Path,
    key: SettingsKeyProvider,
    seed: SettingsSeedProvider,
    host: PluginHost,
) -> (PackageApplication, u64) {
    let service = PackageApplication::new(
        host,
        env!("INVERTER_DESKTOP_TARGET").into(),
        false,
        Arc::new(|| {}),
    );
    service
        .initialize_with_seed(
            Ok(root.join("store")),
            Ok(TrustStore::default()),
            key,
            Some(seed),
        )
        .await;
    let epoch = service.session_configured(true, Ok(vec![])).unwrap();
    (service, epoch)
}

async fn install_disabled(service: &PackageApplication, epoch: u64, root: &Path) {
    let (mut declaration, bytes) = declared_package(root, "1.0.0", Some(settings_schema()));
    declaration.enabled = false;
    service.configure(Ok(vec![declaration]));
    reconcile_bytes(service, epoch, &bytes, &AtomicUsize::new(0)).await;
}

#[tokio::test]
async fn portable_seed_without_secrets_is_editable_and_explicit_saves_remain_authoritative() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let key = settings_test_key();
    let host = PluginHost::default();
    let provider: SettingsSeedProvider = Arc::new(|_, current| Ok(seed_data(current)));
    let (service, epoch) =
        seeded_application(&root, key.clone(), provider.clone(), host.clone()).await;
    install_disabled(&service, epoch, &root).await;
    let view = settings_view(&service, epoch).await;
    assert_eq!(
        view["values"]["endpoint"],
        "https://restored.example.invalid"
    );
    assert_eq!(view["secret_present"]["token"], false);
    assert!(host.snapshots().is_empty());
    assert_eq!(
        service
            .settings_store()
            .unwrap()
            .read(PLUGIN)
            .unwrap()
            .legacy_migration_version,
        1
    );
    let saved = service
        .save_settings(
            PLUGIN,
            epoch,
            view["revision"].as_str().unwrap().into(),
            BTreeMap::from([("endpoint".into(), json!("https://edited.example.invalid"))]),
            BTreeMap::from([("token".into(), Some("fixture-secret".into()))]),
        )
        .await
        .unwrap();
    assert!(saved.restart_error.is_none());
    let modules = service.export_modules(epoch).await.unwrap();
    assert_eq!(
        modules[PLUGIN].values["endpoint"],
        "https://edited.example.invalid"
    );
    assert!(modules[PLUGIN].secrets.is_empty());
    assert!(!serde_json::to_string(&modules)
        .unwrap()
        .contains("fixture-secret"));
    service.close().await.unwrap();
    let (service, epoch) = seeded_application(&root, key, provider, PluginHost::default()).await;
    assert_eq!(
        settings_view(&service, epoch).await["values"]["endpoint"],
        "https://edited.example.invalid"
    );
    service.close().await.unwrap();
}

#[tokio::test]
async fn rejected_or_revoked_seed_never_commits_and_the_editor_can_resolve_missing_defaults() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let host = PluginHost::default();
    let provider: SettingsSeedProvider =
        Arc::new(|_, _| Err("legacy_migration_waiting_for_daemon".into()));
    let (service, epoch) =
        seeded_application(&root, settings_test_key(), provider, host.clone()).await;
    install_disabled(&service, epoch, &root).await;
    assert_eq!(
        settings_view(&service, epoch).await["secret_present"]["token"],
        false
    );
    assert!(service.take_waiting_migration());
    assert!(!service.take_waiting_migration());
    assert_eq!(
        service
            .settings_store()
            .unwrap()
            .read(PLUGIN)
            .unwrap()
            .legacy_migration_version,
        0
    );
    *service.0.settings_seed.lock().unwrap() = Some(Arc::new(|_, current| {
        let mut next = seed_data(current);
        next.values.insert("endpoint".into(), json!(23));
        Ok(next)
    }));
    settings_view(&service, epoch).await;
    assert_eq!(
        service
            .settings_store()
            .unwrap()
            .read(PLUGIN)
            .unwrap()
            .legacy_migration_version,
        0
    );
    let revoke = host.clone();
    *service.0.settings_seed.lock().unwrap() = Some(Arc::new(move |_, current| {
        revoke.revoke();
        Ok(seed_data(current))
    }));
    assert!(service.get_settings(PLUGIN, epoch).await.is_err());
    assert_eq!(
        service
            .settings_store()
            .unwrap()
            .read(PLUGIN)
            .unwrap()
            .legacy_migration_version,
        0
    );
    service.close().await.unwrap();
}

#[tokio::test]
async fn edited_encrypted_settings_export_and_restore_replace_only_the_installed_shadow() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let key = settings_test_key();
    let provider: SettingsSeedProvider = Arc::new(|_, current| Ok(seed_data(current)));
    let (service, epoch) = seeded_application(&root, key, provider, PluginHost::default()).await;
    install_disabled(&service, epoch, &root).await;
    let view = settings_view(&service, epoch).await;
    service
        .save_settings(
            PLUGIN,
            epoch,
            view["revision"].as_str().unwrap().into(),
            BTreeMap::from([("endpoint".into(), json!("https://edited.example.invalid"))]),
            BTreeMap::from([("token".into(), Some("fixture-secret".into()))]),
        )
        .await
        .unwrap();
    let store = service.settings_store().unwrap();
    let before = store.read(PLUGIN).unwrap();
    let records: Vec<_> = fs::read_dir(root.join("store/settings"))
        .unwrap()
        .map(|entry| fs::read(entry.unwrap().path()).unwrap())
        .collect();
    assert_eq!(records.len(), 1);
    assert!(!String::from_utf8_lossy(&records[0]).contains("fixture-secret"));
    assert!(!String::from_utf8_lossy(&records[0]).contains("edited.example.invalid"));

    let mut current = crate::FullConfig {
        modules: crate::module_config::test_namespaces(),
        ..Default::default()
    };
    current.modules.insert(
        PLUGIN.into(),
        serde_json::from_value(json!({"schema_version":1,
            "values":{"endpoint":"https://obsolete.example.invalid"},
            "secrets":{"token":"obsolete-shadow-secret"}}))
        .unwrap(),
    );
    let mut source = current.clone();
    let installed = service.export_modules(epoch).await.unwrap();
    source.modules.extend(installed.clone());
    let mut backup = crate::config_backup::redacted(&source).unwrap();
    backup["show_batteries"] = json!(false);
    let content = backup.to_string();
    assert!(!content.contains("fixture-secret"));
    assert!(!content.contains("obsolete-shadow-secret"));
    assert!(crate::config_backup::restore(&content, &current).is_err());
    let retained = current.modules["example.future"].clone();
    let host = service.0.host.clone();
    let restored = service
        .with_portable_modules(epoch, move |installed| {
            let next =
                crate::module_config::restore_with_installed(&content, &current, &installed)?;
            host.commit_in_epoch(epoch, || Ok(next))
        })
        .await
        .unwrap();
    assert_eq!(restored.show_batteries, Some(false));
    assert_eq!(restored.modules[PLUGIN], installed[PLUGIN]);
    assert!(restored.modules[PLUGIN].secrets.is_empty());
    assert_eq!(restored.modules["example.future"], retained);
    assert!(store.read(PLUGIN).unwrap() == before);
    assert_eq!(
        settings_view(&service, epoch).await["secret_present"]["token"],
        true
    );
    service.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn portable_restore_snapshot_serializes_concurrent_encrypted_settings_save() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let provider: SettingsSeedProvider = Arc::new(|_, current| Ok(seed_data(current)));
    let (service, epoch) =
        seeded_application(&root, settings_test_key(), provider, PluginHost::default()).await;
    install_disabled(&service, epoch, &root).await;
    let view = settings_view(&service, epoch).await;
    let saved = service
        .save_settings(
            PLUGIN,
            epoch,
            view["revision"].as_str().unwrap().into(),
            BTreeMap::from([("endpoint".into(), json!("https://before.example.invalid"))]),
            BTreeMap::from([("token".into(), Some("fixture-secret".into()))]),
        )
        .await
        .unwrap();
    let revision = serde_json::to_value(saved).unwrap()["settings"]["revision"]
        .as_str()
        .unwrap()
        .to_owned();
    let store = service.settings_store().unwrap();
    let before = store.read(PLUGIN).unwrap();
    let mut source = crate::FullConfig {
        modules: service.export_modules(epoch).await.unwrap(),
        ..Default::default()
    };
    let backup = crate::config_backup::redacted(&source).unwrap().to_string();
    source.modules.get_mut(PLUGIN).unwrap().values["endpoint"] =
        json!("https://old-shadow.invalid");
    source
        .modules
        .get_mut(PLUGIN)
        .unwrap()
        .secrets
        .insert("token".into(), "old-shadow-secret".into());
    let persisted = Arc::new(Mutex::new(source));
    let (entered, snapshot_ready) = tokio::sync::oneshot::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    let restore = tokio::spawn({
        let service = service.clone();
        let core = persisted.clone();
        let content = backup.clone();
        let store = store.clone();
        let before = before.clone();
        async move {
            let host = service.0.host.clone();
            service
                .with_portable_modules(epoch, move |installed| {
                    entered.send(()).unwrap();
                    blocked.recv_timeout(Duration::from_secs(5)).unwrap();
                    // A competing save cannot change the encrypted authority between
                    // the installed snapshot and its synchronous restore consumer.
                    assert!(store.read(PLUGIN).unwrap() == before);
                    let mut current = core.lock().unwrap();
                    let next = crate::module_config::restore_with_installed(
                        &content, &current, &installed,
                    )?;
                    host.commit_in_epoch(epoch, || {
                        *current = next;
                        Ok(())
                    })
                })
                .await
        }
    });
    snapshot_ready.await.unwrap();
    let concurrent = service.save_settings(
        PLUGIN,
        epoch,
        revision,
        BTreeMap::from([("endpoint".into(), json!("https://after.example.invalid"))]),
        BTreeMap::new(),
    );
    tokio::pin!(concurrent);
    assert!(futures_util::poll!(&mut concurrent).is_pending());
    // Both application-owned operations have started; neither caller cancellation
    // nor an async scheduling delay can release the restore's operation guard.
    assert_eq!(service.0.authorized_work.load(Ordering::Acquire), 2);
    assert!(store.read(PLUGIN).unwrap() == before);
    release.send(()).unwrap();
    restore.await.unwrap().unwrap();
    concurrent.await.unwrap();
    assert_eq!(
        persisted.lock().unwrap().modules[PLUGIN].values["endpoint"],
        "https://before.example.invalid"
    );
    assert_eq!(
        store.read(PLUGIN).unwrap().values["endpoint"],
        "https://after.example.invalid"
    );
    assert_eq!(
        store.read(PLUGIN).unwrap().secrets["token"],
        "fixture-secret"
    );

    // A later attempt must validate the new authority and reject this old export
    // before touching even an unrelated core preference.
    let previous = serde_json::to_value(persisted.lock().unwrap().clone()).unwrap();
    let core = persisted.clone();
    let rejected = service
        .with_portable_modules(epoch, move |installed| {
            let mut current = core.lock().unwrap();
            let next = crate::module_config::restore_with_installed(&backup, &current, &installed)?;
            *current = next;
            Ok(())
        })
        .await;
    assert!(rejected.is_err());
    assert_eq!(
        serde_json::to_value(persisted.lock().unwrap().clone()).unwrap(),
        previous
    );
    service.close().await.unwrap();
}
