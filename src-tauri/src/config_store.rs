//! Encrypted configuration with platform credential storage and lossless migration.
use super::FullConfig;
use aead::{Aead, KeyInit};
use aes_gcm::Aes256Gcm;
use base64::{engine::general_purpose, Engine as _};
use rand::RngExt;
use std::sync::Mutex;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use tauri::Manager;
use tauri_plugin_store::StoreExt;

static ENCRYPTION_KEY_CACHE: Mutex<Option<Vec<u8>>> = Mutex::new(None);

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

#[cfg(not(any(target_os = "android", target_os = "ios")))]
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
#[cfg(any(test, not(any(target_os = "android", target_os = "ios"))))]
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

fn encrypt_config(config: &FullConfig, key: &[u8]) -> Result<String, String> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| format!("Invalid key: {}", e))?;
    let plaintext = serde_json::to_vec(config).map_err(|e| e.to_string())?;

    let mut nonce_bytes = [0u8; 12];
    rand::rng().fill(&mut nonce_bytes);
    let nonce = <aes_gcm::Nonce<aead::consts::U12>>::try_from(nonce_bytes.as_slice())
        .map_err(|e| format!("Nonce error: {}", e))?;

    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_ref())
        .map_err(|e| format!("Encryption failed: {}", e))?;

    let mut result = nonce_bytes.to_vec();
    result.extend_from_slice(&ciphertext);
    Ok(general_purpose::STANDARD.encode(&result))
}

fn decrypt_config(encrypted: &str, key: &[u8]) -> Result<FullConfig, String> {
    let data = general_purpose::STANDARD
        .decode(encrypted)
        .map_err(|e| format!("Base64 decode failed: {}", e))?;

    if data.len() < 12 {
        return Err("Invalid encrypted data: too short".to_string());
    }

    let (nonce_bytes, ciphertext) = data.split_at(12);
    let nonce = <aes_gcm::Nonce<aead::consts::U12>>::try_from(nonce_bytes)
        .map_err(|e| format!("Nonce error: {}", e))?;

    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| format!("Invalid key: {}", e))?;
    let plaintext = cipher
        .decrypt(&nonce, ciphertext)
        .map_err(|e| format!("Decryption failed: {}", e))?;

    serde_json::from_slice(&plaintext).map_err(|e| format!("JSON parse failed: {}", e))
}

pub(super) fn load_config(app: &tauri::AppHandle) -> Result<FullConfig, String> {
    let key = get_or_create_encryption_key(app)?;

    let store = app
        .store_builder("config.json")
        .build()
        .map_err(|e| format!("Failed to build store: {}", e))?;

    match store.get("config") {
        Some(v) => {
            if let Some(encrypted_str) = v.as_str() {
                match decrypt_config(encrypted_str, &key) {
                    Ok(config) => Ok(config),
                    Err(error) => {
                        #[cfg(any(target_os = "android", target_os = "ios"))]
                        {
                            // One-way migration from the historic shared mobile key.
                            // Commit the new ciphertext before returning the migrated config.
                            if let Ok(config) = decrypt_config(encrypted_str, &legacy_mobile_key())
                            {
                                save_config_encrypted(app, &config)?;
                                return Ok(config);
                            }
                        }
                        Err(error)
                    }
                }
            } else {
                // Legacy unencrypted config - migrate
                let config: FullConfig =
                    serde_json::from_value(v).map_err(|e| format!("Invalid legacy config: {e}"))?;
                let encrypted = encrypt_config(&config, &key)?;
                store.set("config", serde_json::json!(encrypted));
                store
                    .save()
                    .map_err(|e| format!("Failed to migrate config: {e}"))?;
                Ok(config)
            }
        }
        None => Ok(FullConfig::default()),
    }
}

pub(super) fn save_config_encrypted(
    app: &tauri::AppHandle,
    config: &FullConfig,
) -> Result<(), String> {
    let key = get_or_create_encryption_key(app)?;
    let encrypted = encrypt_config(config, &key)?;

    let store = app
        .store_builder("config.json")
        .build()
        .map_err(|e| format!("Failed to build store: {}", e))?;

    let previous = store.get("config");
    store.set("config", serde_json::json!(encrypted));
    if let Err(error) = store.save() {
        match previous {
            Some(value) => store.set("config", value),
            None => {
                store.delete("config");
            }
        }
        return Err(format!("Failed to save config: {error}"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

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
        let config = FullConfig {
            mqtt_password: Some("test-credential".into()),
            show_batteries: Some(false),
            ..Default::default()
        };
        let encrypted = encrypt_config(&config, &[1; 32]).unwrap();
        assert!(!encrypted.contains("test-credential"));
        assert_eq!(
            decrypt_config(&encrypted, &[1; 32]).unwrap().mqtt_password,
            config.mqtt_password
        );
        assert!(decrypt_config(&encrypted, &[2; 32]).is_err());
        assert_eq!(
            decrypt_config(&encrypted, &[1; 32]).unwrap().show_batteries,
            Some(false)
        );
    }
}
