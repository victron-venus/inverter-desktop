use super::*;
use crate::plugins::settings_store::{SettingsData, SettingsStore};
use std::fs::{self, OpenOptions};
use std::os::unix::fs::{symlink, OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;
use std::sync::{Arc, Barrier};

fn ephemeral_key() -> Vec<u8> {
    use aead::Generate;
    aead::Key::<aes_gcm::Aes256Gcm>::generate().to_vec()
}

fn fixture() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    // macOS's /var alias is a symlink; exercise the real private directory.
    let path = temp.path().canonicalize().unwrap();
    (temp, path)
}

fn write_private(path: &Path, bytes: &[u8]) {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap()
        .write_all(bytes)
        .unwrap();
}

fn cached(path: &Path, key: &[u8]) {
    write_private(
        &path.join("config.key"),
        general_purpose::STANDARD.encode(key).as_bytes(),
    );
}

fn no_keychain() -> Result<Option<Vec<u8>>, String> {
    panic!("A present cache must never contact Keychain")
}

fn assert_no_pending(path: &Path) {
    assert!(fs::read_dir(path).unwrap().all(|entry| !entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".config-key-")));
}

#[test]
fn existing_cache_survives_restart_without_keychain() {
    let (_temp, path) = fixture();
    let key = ephemeral_key();
    cached(&path, &key);
    let before = fs::metadata(path.join("config.key")).unwrap();
    for _ in 0..3 {
        assert!(load_or_create(&path, no_keychain).unwrap() == key);
    }
    let after = fs::metadata(path.join("config.key")).unwrap();
    assert!(unchanged(&before, &after));
}

#[test]
fn keychain_migration_preserves_config_and_plugin_ciphertexts() {
    let (_temp, path) = fixture();
    let key = ephemeral_key();
    let config = crate::FullConfig::default();
    let encrypted = super::super::encrypt_config(&config, &key).unwrap();
    let config_bytes = serde_json::to_vec(&serde_json::json!({"config": encrypted})).unwrap();
    fs::write(path.join("config.json"), &config_bytes).unwrap();
    fs::create_dir(path.join("plugins")).unwrap();
    fs::set_permissions(path.join("plugins"), fs::Permissions::from_mode(0o700)).unwrap();
    let original_key = key.clone();
    let store = SettingsStore::new(
        path.join("plugins"),
        Arc::new(move || Ok(original_key.clone())),
    );
    let mut settings = SettingsData::default();
    settings
        .secrets
        .insert("password".into(), "test-fixture-password".into());
    settings.secret_fields.insert("password".into());
    store
        .prepare_write("test.frigate", &settings)
        .unwrap()
        .commit()
        .unwrap();
    let record = fs::read_dir(path.join("plugins/settings"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let plugin_bytes = fs::read(&record).unwrap();
    let mut calls = 0;
    let migrated = load_or_create(&path, || {
        calls += 1;
        Ok(Some(key.clone()))
    })
    .unwrap();
    assert_eq!(calls, 1);
    assert!(migrated == key);
    assert!(super::super::decrypt_config(&encrypted, &migrated).is_ok());
    let restarted_path = path.clone();
    let restarted_store = SettingsStore::new(
        path.join("plugins"),
        Arc::new(move || load_or_create(&restarted_path, no_keychain)),
    );
    assert!(restarted_store.read("test.frigate").unwrap() == settings);
    assert!(fs::read(path.join("config.json")).unwrap() == config_bytes);
    assert!(fs::read(record).unwrap() == plugin_bytes);
    assert_eq!(
        fs::metadata(path.join("config.key")).unwrap().mode() & 0o7777,
        0o600
    );
    assert_no_pending(&path);
}

#[test]
fn first_install_creates_one_private_key_and_reuses_it() {
    let (_temp, path) = fixture();
    let app_path = path.join("new/application");
    let key = load_or_create(&app_path, || Ok(None)).unwrap();
    assert_eq!(key.len(), 32);
    assert!(load_or_create(&app_path, no_keychain).unwrap() == key);
    let metadata = fs::metadata(app_path.join("config.key")).unwrap();
    assert_eq!(metadata.mode() & 0o7777, 0o600);
    assert_eq!(metadata.nlink(), 1);
    assert_eq!(metadata.uid(), owner());
    assert_eq!(fs::metadata(&app_path).unwrap().mode() & 0o777, 0o700);
    assert_no_pending(&app_path);
}

#[test]
fn no_key_is_generated_when_keychain_is_unavailable() {
    let (_temp, path) = fixture();
    assert!(load_or_create(&path, || Err("Denied".into())).is_err());
    assert!(!path.join("config.key").exists());
    assert_no_pending(&path);
}

#[test]
fn invalid_keychain_record_is_not_persisted() {
    let (_temp, path) = fixture();
    assert!(load_or_create(&path, || Ok(Some(vec![1; 31]))).is_err());
    assert!(!path.join("config.key").exists());
    assert_no_pending(&path);
}

#[test]
fn missing_key_cannot_replace_encrypted_config_or_malformed_records() {
    for record in [
        r#"{"config":"existing-ciphertext"}"#,
        r#"{"config":null}"#,
        "[1]",
        "invalid JSON",
    ] {
        let (_temp, path) = fixture();
        fs::write(path.join("config.json"), record).unwrap();
        assert!(load_or_create(&path, || Ok(None)).is_err());
        assert!(!path.join("config.key").exists());
        assert_eq!(
            fs::read_to_string(path.join("config.json")).unwrap(),
            record
        );
    }
}

#[test]
fn plaintext_legacy_config_and_empty_plugin_directory_allow_initialization() {
    let (_temp, path) = fixture();
    let config = crate::FullConfig {
        mqtt_host: "legacy-broker.example".into(),
        mqtt_password: Some("legacy-test-fixture".into()),
        ..crate::FullConfig::default()
    };
    let original = serde_json::to_vec(&serde_json::json!({"config": config})).unwrap();
    fs::write(path.join("config.json"), &original).unwrap();
    fs::create_dir_all(path.join("plugins/settings")).unwrap();
    let key = load_or_create(&path, || Ok(None)).unwrap();
    assert_eq!(key.len(), 32);
    assert!(fs::read(path.join("config.json")).unwrap() == original);
    let encrypted = super::super::encrypt_config(&config, &key).unwrap();
    let migrated = super::super::decrypt_config(&encrypted, &key).unwrap();
    assert!(serde_json::to_value(migrated).unwrap() == serde_json::to_value(config).unwrap());
}

#[test]
fn legacy_plaintext_config_does_not_hide_encrypted_plugin_settings() {
    let (_temp, path) = fixture();
    fs::write(
        path.join("config.json"),
        serde_json::to_vec(&serde_json::json!({"config": crate::FullConfig::default()})).unwrap(),
    )
    .unwrap();
    fs::create_dir_all(path.join("plugins/settings")).unwrap();
    fs::write(path.join("plugins/settings/retained.enc"), b"ciphertext").unwrap();
    assert!(load_or_create(&path, || Ok(None)).is_err());
    assert!(!path.join("config.key").exists());
}

#[test]
fn missing_key_cannot_replace_retained_or_pending_plugin_settings() {
    for name in [
        "retained.enc",
        "settings-pending-transaction",
        "unknown-record",
    ] {
        let (_temp, path) = fixture();
        fs::create_dir_all(path.join("plugins/settings")).unwrap();
        let record = path.join("plugins/settings").join(name);
        fs::write(&record, b"existing ciphertext").unwrap();
        assert!(load_or_create(&path, || Ok(None)).is_err());
        assert!(!path.join("config.key").exists());
        assert_eq!(fs::read(record).unwrap(), b"existing ciphertext");
    }
}

#[test]
fn invalid_cache_never_contacts_keychain_or_changes_existing_bytes() {
    for bytes in [
        Vec::new(),
        b"not base64".to_vec(),
        general_purpose::STANDARD.encode([1; 31]).into_bytes(),
        vec![b'A'; 129],
        vec![0xff; 44],
    ] {
        let (_temp, path) = fixture();
        write_private(&path.join("config.key"), &bytes);
        assert!(load_or_create(&path, no_keychain).is_err());
        assert!(fs::read(path.join("config.key")).unwrap() == bytes);
    }
}

#[test]
fn wrong_but_valid_cached_key_does_not_trigger_keychain_fallback() {
    let (_temp, path) = fixture();
    let original = ephemeral_key();
    let mut wrong_key = original.clone();
    wrong_key[0] ^= 1;
    let encrypted = super::super::encrypt_config(&crate::FullConfig::default(), &original).unwrap();
    fs::write(
        path.join("config.json"),
        serde_json::to_vec(&serde_json::json!({"config": encrypted})).unwrap(),
    )
    .unwrap();
    cached(&path, &wrong_key);
    let key = load_or_create(&path, no_keychain).unwrap();
    assert!(super::super::decrypt_config(&encrypted, &key).is_err());
    assert!(key == wrong_key);
}

#[test]
fn cache_requires_exact_private_permissions() {
    for mode in [0o644, 0o640, 0o666, 0o400, 0o1600] {
        let (_temp, path) = fixture();
        cached(&path, &ephemeral_key());
        fs::set_permissions(path.join("config.key"), fs::Permissions::from_mode(mode)).unwrap();
        assert!(load_or_create(&path, no_keychain).is_err());
        assert_eq!(
            fs::metadata(path.join("config.key")).unwrap().mode() & 0o7777,
            mode
        );
    }
}

#[test]
fn cache_rejects_symlink_hardlink_directory_and_fifo_without_keychain() {
    for kind in ["symlink", "hardlink", "directory", "fifo"] {
        let (_temp, path) = fixture();
        let key = path.join("config.key");
        match kind {
            "symlink" => symlink(path.join("missing"), &key).unwrap(),
            "hardlink" => {
                write_private(
                    &path.join("original"),
                    general_purpose::STANDARD.encode(ephemeral_key()).as_bytes(),
                );
                fs::hard_link(path.join("original"), &key).unwrap();
            }
            "directory" => fs::create_dir(&key).unwrap(),
            "fifo" => {
                let name = CString::new(key.as_os_str().as_bytes()).unwrap();
                assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
            }
            _ => unreachable!(),
        }
        assert!(load_or_create(&path, no_keychain).is_err());
        assert!(fs::symlink_metadata(&key).is_ok());
    }
}

#[test]
fn descriptor_metadata_rejects_a_foreign_owner() {
    let (_temp, path) = fixture();
    cached(&path, &ephemeral_key());
    let metadata = fs::metadata(path.join("config.key")).unwrap();
    assert!(validate_file(&metadata, true, MAX_KEY_BYTES, owner().wrapping_add(1)).is_err());
}

#[test]
fn app_directory_rejects_symlink_and_writable_parents() {
    let (_temp, path) = fixture();
    fs::create_dir(path.join("real")).unwrap();
    symlink(path.join("real"), path.join("alias")).unwrap();
    assert!(load_or_create(&path.join("alias"), no_keychain).is_err());
    fs::set_permissions(path.join("real"), fs::Permissions::from_mode(0o777)).unwrap();
    assert!(load_or_create(&path.join("real/application"), no_keychain).is_err());
    assert!(!path.join("real/application").exists());
}

#[test]
fn unsafe_existing_configuration_and_settings_fail_closed() {
    for name in ["config.json", "plugins", "plugins/settings"] {
        let (_temp, path) = fixture();
        if name == "plugins/settings" {
            fs::create_dir(path.join("plugins")).unwrap();
        }
        symlink(path.join("missing"), path.join(name)).unwrap();
        assert!(load_or_create(&path, || Ok(None)).is_err());
        assert!(!path.join("config.key").exists());
    }
    let (_temp, path) = fixture();
    let config = File::create(path.join("config.json")).unwrap();
    config.set_len(MAX_CONFIG_BYTES + 1).unwrap();
    assert!(load_or_create(&path, || Ok(None)).is_err());
    assert!(!path.join("config.key").exists());
}

#[test]
fn atomic_publication_adopts_existing_key_without_overwrite() {
    let (_temp, path) = fixture();
    let directory = app_directory(&path).unwrap();
    let original = persist_key(&directory, &ephemeral_key()).unwrap();
    let before = fs::metadata(path.join("config.key")).unwrap();
    assert!(persist_key(&directory, &ephemeral_key()).unwrap() == original);
    assert!(unchanged(
        &before,
        &fs::metadata(path.join("config.key")).unwrap()
    ));
    assert_no_pending(&path);
}

#[test]
fn publication_never_replaces_an_invalid_existing_target() {
    for directory_target in [false, true] {
        let (_temp, path) = fixture();
        if directory_target {
            fs::create_dir(path.join("config.key")).unwrap();
        } else {
            write_private(&path.join("config.key"), b"existing invalid cache");
        }
        let directory = app_directory(&path).unwrap();
        assert!(persist_key(&directory, &ephemeral_key()).is_err());
        if directory_target {
            assert!(path.join("config.key").is_dir());
        } else {
            assert_eq!(
                fs::read(path.join("config.key")).unwrap(),
                b"existing invalid cache"
            );
        }
        assert_no_pending(&path);
    }
}

#[test]
fn simultaneous_first_launches_publish_one_complete_durable_key() {
    let (_temp, path) = fixture();
    let count = 8;
    let ready = Arc::new(Barrier::new(count));
    let workers: Vec<_> = (0..count)
        .map(|_| {
            let path = path.clone();
            let ready = Arc::clone(&ready);
            std::thread::spawn(move || {
                load_or_create(&path, || {
                    ready.wait();
                    Ok(None)
                })
                .unwrap()
            })
        })
        .collect();
    let keys: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    let durable = load_or_create(&path, no_keychain).unwrap();
    assert!(keys.iter().all(|key| *key == durable));
    assert_eq!(fs::metadata(path.join("config.key")).unwrap().len(), 44);
    assert_eq!(
        fs::metadata(path.join("config.key")).unwrap().mode() & 0o7777,
        0o600
    );
    assert_no_pending(&path);
}

#[test]
fn concurrent_launch_can_publish_key_and_ciphertext_during_keychain_wait() {
    for denied in [false, true] {
        let (_temp, path) = fixture();
        let winner = ephemeral_key();
        let encrypted =
            super::super::encrypt_config(&crate::FullConfig::default(), &winner).unwrap();
        let original = serde_json::to_vec(&serde_json::json!({"config": encrypted})).unwrap();
        let actual = load_or_create(&path, || {
            persist_key(&app_directory(&path).unwrap(), &winner).unwrap();
            fs::write(path.join("config.json"), &original).unwrap();
            if denied {
                Err("Keychain access denied".into())
            } else {
                Ok(None)
            }
        })
        .unwrap();
        assert!(actual == winner);
        assert!(fs::read(path.join("config.json")).unwrap() == original);
        assert!(super::super::decrypt_config(&encrypted, &actual).is_ok());
        assert_no_pending(&path);
    }
}

#[test]
fn invalid_concurrently_published_cache_is_not_ignored() {
    let (_temp, path) = fixture();
    assert!(load_or_create(&path, || {
        write_private(&path.join("config.key"), b"invalid concurrent key");
        Ok(Some(ephemeral_key()))
    })
    .is_err());
    assert_eq!(
        fs::read(path.join("config.key")).unwrap(),
        b"invalid concurrent key"
    );
    assert_no_pending(&path);
}
