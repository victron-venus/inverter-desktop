//! Encrypted configuration with platform credential storage and lossless migration.
use super::FullConfig;
use aead::{Aead, KeyInit, Payload};
use aes_gcm::Aes256Gcm;
use base64::{engine::general_purpose, Engine as _};
use rand::RngExt;
use std::sync::Mutex;
use tauri::Manager;

mod document;

// This gate is independent of CONFIG_UPDATE_GATE: storage never acquires an
// application/session gate, so auth guards and already-locked saves can use it.
static STORAGE_GATE: Mutex<()> = Mutex::new(());

static ENCRYPTION_KEY_CACHE: Mutex<Option<Vec<u8>>> = Mutex::new(None);

#[cfg(any(target_os = "macos", all(test, target_os = "linux")))]
mod macos_key;

#[cfg(any(test, not(any(target_os = "android", target_os = "ios"))))]
fn decode_key(value: &str) -> Result<Vec<u8>, String> {
    let key = general_purpose::STANDARD
        .decode(value.trim())
        .map_err(|e| e.to_string())?;
    if key.len() != 32 {
        return Err("Invalid encryption key length".into());
    }
    Ok(key)
}

#[cfg(target_os = "macos")]
fn platform_key(app: &tauri::AppHandle) -> Result<Vec<u8>, String> {
    let directory = app.path().app_data_dir().map_err(|e| e.to_string())?;
    // Constructing/accessing Keychain is deliberately inside the fallback:
    // a valid file cache must survive an ad-hoc-signed app replacement unaided.
    macos_key::load_or_create(&directory, || {
        let entry = keyring::Entry::new("inverter-desktop", "victron")
            .map_err(|_| "macOS Keychain is unavailable")?;
        match entry.get_password() {
            Ok(value) => decode_key(&value).map(Some),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err("Cannot read the existing encryption key from macOS Keychain".into()),
        }
    })
}

#[cfg(not(any(target_os = "macos", target_os = "android", target_os = "ios")))]
fn platform_key(app: &tauri::AppHandle) -> Result<Vec<u8>, String> {
    let path = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("config.key");
    let entry = keyring::Entry::new("inverter-desktop", "victron")
        .map_err(|e| format!("Credential store unavailable: {e}"))?;
    migrate_or_create_key(
        &path,
        || match entry.get_password() {
            Ok(value) => decode_key(&value).map(Some),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(format!("Credential store unavailable: {e}")),
        },
        |key| {
            entry
                .set_password(&general_purpose::STANDARD.encode(key))
                .map_err(|e| format!("Cannot persist encryption key: {e}"))
        },
    )
}

/// Keep the old file until the credential store has read back the identical key.
/// An unavailable store is an explicit error, never a new key or plaintext fallback.
#[cfg(any(
    test,
    not(any(target_os = "macos", target_os = "android", target_os = "ios"))
))]
fn migrate_or_create_key(
    path: &std::path::Path,
    mut read: impl FnMut() -> Result<Option<Vec<u8>>, String>,
    mut write: impl FnMut(&[u8]) -> Result<(), String>,
) -> Result<Vec<u8>, String> {
    let legacy = match std::fs::read_to_string(path) {
        Ok(value) => Some(decode_key(&value)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.to_string()),
    };
    if let Some(key) = legacy {
        write(&key)?;
        if read()?.as_deref() != Some(key.as_slice()) {
            return Err("Credential migration verification failed".into());
        }
        std::fs::remove_file(path)
            .map_err(|e| format!("Cannot remove legacy encryption key: {e}"))?;
        return Ok(key);
    }
    if let Some(key) = read()? {
        return Ok(key);
    }
    let mut key = [0u8; 32];
    rand::rng().fill(&mut key);
    write(&key)?;
    if read()?.as_deref() != Some(key.as_slice()) {
        return Err("Credential persistence verification failed".into());
    }
    Ok(key.to_vec())
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn platform_key(app: &tauri::AppHandle) -> Result<Vec<u8>, String> {
    crate::mobile_credentials::encryption_key(app)
}

fn get_or_create_encryption_key(app: &tauri::AppHandle) -> Result<Vec<u8>, String> {
    let mut cache = ENCRYPTION_KEY_CACHE.lock().map_err(|e| e.to_string())?;
    if let Some(key) = cache.as_ref() {
        return Ok(key.clone());
    }
    let key = platform_key(app)?;
    if key.len() != 32 {
        return Err("Invalid encryption key length".into());
    }
    *cache = Some(key.clone());
    Ok(key)
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn legacy_mobile_key() -> Vec<u8> {
    // Read-only legacy compatibility. Never used for new encryption.
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    "inverter-desktop-victron-encryption-key".hash(&mut hasher);
    let hash = hasher.finish();

    let mut key = [0u8; 32];
    for (i, b) in hash.to_le_bytes().iter().cycle().take(32).enumerate() {
        key[i] = *b;
    }
    key.to_vec()
}

const CONFIG_V2_PREFIX: &str = "inverter-config:v2:";
const CONFIG_V2_AAD: &[u8] = b"inverter-desktop/config/v2";

fn encrypt_config(config: &FullConfig, key: &[u8]) -> Result<String, String> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| format!("Invalid key: {}", e))?;
    let plaintext = serde_json::to_vec(config).map_err(|e| e.to_string())?;

    let mut nonce_bytes = [0u8; 12];
    rand::rng().fill(&mut nonce_bytes);
    let nonce = <aes_gcm::Nonce<aead::consts::U12>>::try_from(nonce_bytes.as_slice())
        .map_err(|e| format!("Nonce error: {}", e))?;

    let versioned = config.auth_password_verifier.is_some();
    let aad: &[u8] = if versioned { CONFIG_V2_AAD } else { &[] };
    let ciphertext = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: &plaintext,
                aad,
            },
        )
        .map_err(|e| format!("Encryption failed: {}", e))?;

    let mut result = nonce_bytes.to_vec();
    result.extend_from_slice(&ciphertext);
    let encoded = general_purpose::STANDARD.encode(&result);
    Ok(if versioned {
        format!("{CONFIG_V2_PREFIX}{encoded}")
    } else {
        encoded
    })
}

fn ciphertext_payload(encrypted: &str) -> Result<(&str, &[u8]), String> {
    if let Some(payload) = encrypted.strip_prefix(CONFIG_V2_PREFIX) {
        return Ok((payload, CONFIG_V2_AAD));
    }
    if encrypted.starts_with("inverter-config:") {
        return Err("Unsupported configuration format version".into());
    }
    Ok((encrypted, &[]))
}

fn decrypt_config(encrypted: &str, key: &[u8]) -> Result<FullConfig, String> {
    let (payload, aad) = ciphertext_payload(encrypted)?;
    let data = general_purpose::STANDARD
        .decode(payload)
        .map_err(|e| format!("Base64 decode failed: {}", e))?;

    if data.len() < 12 {
        return Err("Invalid encrypted data: too short".to_string());
    }

    let (nonce_bytes, ciphertext) = data.split_at(12);
    let nonce = <aes_gcm::Nonce<aead::consts::U12>>::try_from(nonce_bytes)
        .map_err(|e| format!("Nonce error: {}", e))?;

    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| format!("Invalid key: {}", e))?;
    let plaintext = cipher
        .decrypt(
            &nonce,
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|e| format!("Decryption failed: {}", e))?;

    serde_json::from_slice(&plaintext).map_err(|e| format!("JSON parse failed: {}", e))
}

fn config_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("config.json"))
        .map_err(|_| "Cannot locate configuration directory".into())
}

pub(super) fn has_saved_config(app: &tauri::AppHandle) -> Result<bool, String> {
    let _storage = STORAGE_GATE
        .lock()
        .map_err(|_| "Configuration storage lock failed")?;
    Ok(document::read(&config_path(app)?)?
        .values
        .contains_key("config"))
}

fn load_path(
    path: &std::path::Path,
    key: &[u8],
    legacy_key: Option<&[u8]>,
) -> Result<FullConfig, String> {
    load_path_with(path, key, legacy_key, document::save)
}

fn load_path_with(
    path: &std::path::Path,
    key: &[u8],
    legacy_key: Option<&[u8]>,
    save: impl FnOnce(&std::path::Path, &document::Document) -> Result<document::CommitOutcome, String>,
) -> Result<FullConfig, String> {
    let mut document = document::read(path)?;
    let Some(value) = document.values.get("config") else {
        return Ok(FullConfig::default());
    };
    let (mut config, mut changed) = if let Some(encrypted) = value.as_str() {
        // Unknown versions never enter legacy-key fallback. Versioned documents
        // are written only with the installation key, never the old mobile key.
        let (_, aad) = ciphertext_payload(encrypted)?;
        let fallback_key = if aad.is_empty() { legacy_key } else { None };
        match decrypt_config(encrypted, key) {
            Ok(config) => {
                let needs_version = config.auth_password_verifier.is_some() && aad.is_empty();
                (config, needs_version)
            }
            Err(error) => match fallback_key {
                Some(legacy) => (decrypt_config(encrypted, legacy).map_err(|_| error)?, true),
                None => return Err(error),
            },
        }
    } else {
        (
            serde_json::from_value(value.clone()).map_err(|_| "Invalid legacy configuration")?,
            true,
        )
    };
    changed |= crate::auth_password::migrate(&mut config)?;
    if changed {
        document.values.insert(
            "config".into(),
            serde_json::Value::String(encrypt_config(&config, key)?),
        );
        finish_commit(save(path, &document)?);
    }
    Ok(config)
}

pub(super) fn load_config(app: &tauri::AppHandle) -> Result<FullConfig, String> {
    let _storage = STORAGE_GATE
        .lock()
        .map_err(|_| "Configuration storage lock failed")?;
    let key = get_or_create_encryption_key(app)?;
    #[cfg(any(target_os = "android", target_os = "ios"))]
    let legacy_key = Some(legacy_mobile_key());
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let legacy_key: Option<Vec<u8>> = None;
    load_path(&config_path(app)?, &key, legacy_key.as_deref())
}

pub(super) fn save_config_encrypted(
    app: &tauri::AppHandle,
    config: &FullConfig,
) -> Result<(), String> {
    let _storage = STORAGE_GATE
        .lock()
        .map_err(|_| "Configuration storage lock failed")?;
    let key = get_or_create_encryption_key(app)?;
    let path = config_path(app)?;
    save_path_with(&path, &key, config, document::save)
}

// A post-rename warning is a committed success, not a retryable save failure.
// Keeping this mapping at the shared boundary lets every auth/plugin caller
// finish session revocation, desired-state updates and notifications normally.
fn finish_commit(outcome: document::CommitOutcome) {
    if outcome == document::CommitOutcome::DirectorySyncUnconfirmed {
        log::warn!("Configuration committed, but directory durability could not be confirmed");
    }
}

fn save_path_with(
    path: &std::path::Path,
    key: &[u8],
    config: &FullConfig,
    save: impl FnOnce(&std::path::Path, &document::Document) -> Result<document::CommitOutcome, String>,
) -> Result<(), String> {
    let mut document = document::read(path)?;
    let mut protected = config.clone();
    crate::auth_password::migrate(&mut protected)?;
    document.values.insert(
        "config".into(),
        serde_json::Value::String(encrypt_config(&protected, key)?),
    );
    finish_commit(save(path, &document)?);
    Ok(())
}

/// Desktop plugin records share the existing application encryption key, with
/// their own authenticated encryption domain and storage format.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) fn plugin_settings_key(app: &tauri::AppHandle) -> Result<Vec<u8>, String> {
    get_or_create_encryption_key(app)
}

// Real filesystem fault injection for authentication/storage integration tests.
#[cfg(test)]
pub(super) mod test_support {
    use super::*;
    use std::io::Write;

    #[derive(Clone, Copy, Debug)]
    pub(crate) enum Fault {
        None,
        FileSync,
        DirectorySync,
    }

    pub(crate) fn save(
        path: &std::path::Path,
        key: &[u8],
        config: &FullConfig,
        fault: Fault,
    ) -> Result<(), String> {
        save_path_with(path, key, config, |path, document| {
            document::save_with(
                path,
                document,
                |file, bytes| file.write_all(bytes),
                |file| match fault {
                    Fault::FileSync => Err(std::io::Error::other("injected file sync failure")),
                    _ => file.sync_all(),
                },
                |from, to| std::fs::rename(from, to),
                |_| match fault {
                    Fault::DirectorySync => {
                        Err(std::io::Error::other("injected directory sync failure"))
                    }
                    _ => Ok(()),
                },
            )
        })
    }

    pub(crate) fn load(path: &std::path::Path, key: &[u8]) -> FullConfig {
        load_path(path, key, None).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn all_legacy_envelopes_migrate_password_once_without_losing_settings() {
        let key = [3u8; 32];
        let old_key = [7u8; 32];
        let config = FullConfig {
            auth_enabled: Some(true),
            auth_username: Some("operator".into()),
            auth_password: Some(" legacy пароль\n".into()),
            mqtt_password: Some("outbound credential".into()),
            ..Default::default()
        };
        let cases = [
            serde_json::to_value(&config).unwrap(),
            serde_json::json!(encrypt_config(&config, &key).unwrap()),
            serde_json::json!(encrypt_config(&config, &old_key).unwrap()),
        ];
        for value in cases {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("config.json");
            let original = serde_json::json!({"config":value,"unrelated":{"keep":true}});
            std::fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();
            let loaded = load_path(&path, &key, Some(&old_key)).unwrap();
            assert!(loaded.auth_password.is_none());
            assert!(loaded
                .auth_password_verifier
                .as_ref()
                .unwrap()
                .matches(" legacy пароль\n")
                .unwrap());
            assert_eq!(loaded.mqtt_password.as_deref(), Some("outbound credential"));
            let bytes = std::fs::read(&path).unwrap();
            let envelope: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(envelope["unrelated"], original["unrelated"]);
            let stored = decrypt_config(envelope["config"].as_str().unwrap(), &key).unwrap();
            assert!(serde_json::to_value(&stored)
                .unwrap()
                .get("auth_password")
                .is_none());
            assert_eq!(stored.auth_password_verifier, loaded.auth_password_verifier);
            load_path(&path, &key, Some(&old_key)).unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
    }

    // This is the old reader's security-relevant projection. Serde ignores the
    // newly added verifier field; absent plaintext previously became "".
    #[derive(serde::Deserialize)]
    struct LegacyAuth {
        auth_enabled: Option<bool>,
        auth_username: Option<String>,
        auth_password: Option<String>,
    }

    fn legacy_read(encrypted: &str, key: &[u8]) -> Result<LegacyAuth, String> {
        let data = general_purpose::STANDARD
            .decode(encrypted)
            .map_err(|e| e.to_string())?;
        let (nonce, ciphertext) = data.split_at(12);
        let nonce = aes_gcm::Nonce::<aead::consts::U12>::try_from(nonce).unwrap();
        let plaintext = Aes256Gcm::new_from_slice(key)
            .unwrap()
            .decrypt(&nonce, ciphertext)
            .map_err(|e| e.to_string())?;
        serde_json::from_slice(&plaintext).map_err(|e| e.to_string())
    }

    fn legacy_encrypt(config: &FullConfig, key: &[u8]) -> String {
        let nonce = aes_gcm::Nonce::<aead::consts::U12>::try_from([9u8; 12].as_slice()).unwrap();
        let plaintext = serde_json::to_vec(config).unwrap();
        let mut bytes = vec![9u8; 12];
        bytes.extend(
            Aes256Gcm::new_from_slice(key)
                .unwrap()
                .encrypt(&nonce, plaintext.as_slice())
                .unwrap(),
        );
        general_purpose::STANDARD.encode(bytes)
    }

    #[test]
    fn versioned_verifier_envelope_blocks_legacy_empty_password_downgrade() {
        let key = [3; 32];
        let mut config = FullConfig {
            auth_enabled: Some(true),
            auth_username: Some("operator".into()),
            auth_password: Some("original password".into()),
            ..Default::default()
        };
        crate::auth_password::migrate(&mut config).unwrap();
        let unversioned = legacy_encrypt(&config, &key);
        let old = legacy_read(&unversioned, &key).unwrap();
        assert!(old.auth_enabled.unwrap_or(false));
        // Exact old comparison would accept the empty password after ignoring
        // auth_password_verifier: this establishes the regression's baseline.
        assert_eq!(old.auth_username.as_deref().unwrap_or(""), "operator");
        assert_eq!(old.auth_password.as_deref().unwrap_or(""), "");
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.json");
        std::fs::write(&path, serde_json::json!({"config":unversioned}).to_string()).unwrap();
        let loaded = load_path(&path, &key, None).unwrap();
        assert!(loaded
            .auth_password_verifier
            .as_ref()
            .unwrap()
            .matches("original password")
            .unwrap());
        let bytes = std::fs::read(&path).unwrap();
        let envelope: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let encrypted = envelope["config"].as_str().unwrap();
        assert!(encrypted.starts_with(CONFIG_V2_PREFIX));
        assert!(legacy_read(encrypted, &key).is_err());
        // The version is authenticated too; removing its text does not permit
        // a legacy decrypt or silently drop the verifier under the old schema.
        let stripped = encrypted.strip_prefix(CONFIG_V2_PREFIX).unwrap();
        assert!(legacy_read(stripped, &key).is_err());
        assert!(decrypt_config(stripped, &key).is_err());
        load_path(&path, &key, None).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn unknown_version_and_versioned_legacy_key_cannot_fall_back() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.json");
        let key = [3; 32];
        let old_key = [7; 32];
        let mut config = FullConfig {
            auth_password: Some("password".into()),
            ..Default::default()
        };
        crate::auth_password::migrate(&mut config).unwrap();
        let encrypted = encrypt_config(&config, &old_key).unwrap();
        for payload in [
            encrypted.clone(),
            encrypted.replacen(CONFIG_V2_PREFIX, "inverter-config:v3:", 1),
        ] {
            let bytes = serde_json::json!({"config":payload})
                .to_string()
                .into_bytes();
            std::fs::write(&path, &bytes).unwrap();
            assert!(load_path(&path, &key, Some(&old_key)).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
    }

    #[test]
    fn migration_write_failure_returns_no_configuration_and_can_retry() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.json");
        let config = FullConfig {
            auth_enabled: Some(true),
            auth_username: Some("operator".into()),
            auth_password: Some("legacy".into()),
            ..Default::default()
        };
        let bytes = serde_json::to_vec(&serde_json::json!({"config":config})).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let result = load_path_with(&path, &[3; 32], None, |_, _| {
            Err("injected persistence failure".into())
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let loaded = load_path(&path, &[3; 32], None).unwrap();
        assert!(loaded
            .auth_password_verifier
            .unwrap()
            .matches("legacy")
            .unwrap());
    }

    #[test]
    fn migration_directory_sync_warning_returns_committed_policy_and_is_idempotent() {
        use std::io::Write;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.json");
        let config = FullConfig {
            auth_enabled: Some(true),
            auth_username: Some("operator".into()),
            auth_password: Some("legacy".into()),
            ..Default::default()
        };
        std::fs::write(&path, serde_json::json!({"config":config}).to_string()).unwrap();
        let loaded = load_path_with(&path, &[3; 32], None, |path, doc| {
            document::save_with(
                path,
                doc,
                |file, bytes| file.write_all(bytes),
                std::fs::File::sync_all,
                |from, to| std::fs::rename(from, to),
                |_| Err(std::io::Error::other("injected directory sync failure")),
            )
        })
        .unwrap();
        assert!(loaded.auth_password.is_none());
        assert!(loaded
            .auth_password_verifier
            .as_ref()
            .unwrap()
            .matches("legacy")
            .unwrap());
        let committed = std::fs::read(&path).unwrap();
        let reread = load_path(&path, &[3; 32], None).unwrap();
        assert_eq!(loaded.auth_password_verifier, reread.auth_password_verifier);
        assert_eq!(std::fs::read(&path).unwrap(), committed);
    }

    #[test]
    fn invalid_verifier_does_not_fall_back_to_legacy_password_or_replace_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.json");
        let mut value = serde_json::to_value(FullConfig::default()).unwrap();
        value["auth_password"] = serde_json::json!("legacy");
        value["auth_password_verifier"] = serde_json::json!("invalid");
        let config: FullConfig = serde_json::from_value(value).unwrap();
        let bytes = serde_json::to_vec(
            &serde_json::json!({"config":encrypt_config(&config,&[3;32]).unwrap()}),
        )
        .unwrap();
        std::fs::write(&path, &bytes).unwrap();
        assert!(load_path(&path, &[3; 32], None).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }

    fn legacy_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("inverter-key-migration-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn migration_removes_file_only_after_verified_store_write() {
        let path = legacy_path();
        let key = vec![7; 32];
        std::fs::write(&path, general_purpose::STANDARD.encode(&key)).unwrap();
        let stored = RefCell::new(None::<Vec<u8>>);
        let actual = migrate_or_create_key(
            &path,
            || Ok(stored.borrow().clone()),
            |key| {
                *stored.borrow_mut() = Some(key.to_vec());
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(actual, key);
        assert!(!path.exists());
        assert_eq!(stored.borrow().as_ref(), Some(&key));
    }

    #[test]
    fn failed_store_preserves_legacy_key_for_retry() {
        let path = legacy_path();
        let value = general_purpose::STANDARD.encode([7; 32]);
        std::fs::write(&path, &value).unwrap();
        assert!(migrate_or_create_key(&path, || Ok(None), |_| Err("locked".into())).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), value);
        assert!(migrate_or_create_key(&path, || Ok(None), |_| Ok(())).is_err());
        assert!(path.exists());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn new_install_persists_random_key_without_a_plaintext_file() {
        let path = legacy_path();
        let stored = RefCell::new(None::<Vec<u8>>);
        let first = migrate_or_create_key(
            &path,
            || Ok(stored.borrow().clone()),
            |key| {
                *stored.borrow_mut() = Some(key.to_vec());
                Ok(())
            },
        )
        .unwrap();
        let second = migrate_or_create_key(
            &path,
            || Ok(stored.borrow().clone()),
            |_| panic!("existing key must not be replaced"),
        )
        .unwrap();
        assert_eq!(first.len(), 32);
        assert_eq!(first, second);
        assert!(!path.exists());
    }

    #[test]
    fn encrypted_config_roundtrips_and_rejects_other_key() {
        use aead::Generate;

        let config = FullConfig {
            modules: crate::module_config::test_namespaces(),
            mqtt_password: Some("test-credential".into()),
            show_batteries: Some(false),
            desktop_plugins: crate::plugin_config::test_declarations(),
            ..Default::default()
        };
        let key = aead::Key::<Aes256Gcm>::generate();
        let mut other_key = key;
        other_key[0] ^= 1;
        let encrypted = encrypt_config(&config, &key).unwrap();
        assert!(!encrypted.contains("test-credential"));
        assert!(!encrypted.contains("test-module-secret"));
        assert_eq!(
            decrypt_config(&encrypted, &key).unwrap().modules,
            config.modules
        );
        assert_eq!(
            decrypt_config(&encrypted, &key).unwrap().mqtt_password,
            config.mqtt_password
        );
        assert!(decrypt_config(&encrypted, &other_key).is_err());
        assert_eq!(
            decrypt_config(&encrypted, &key).unwrap().show_batteries,
            Some(false)
        );
        assert_eq!(
            decrypt_config(&encrypted, &key).unwrap().desktop_plugins,
            config.desktop_plugins
        );
    }
}
