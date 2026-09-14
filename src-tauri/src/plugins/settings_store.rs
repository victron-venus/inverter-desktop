//! Encrypted per-plugin data owned by the package store, never by a package payload.
//!
//! Callers hold the package manager's lifetime lease and operation lock. Prepare
//! writes before taking the host authority lock; commit only in the original epoch.
//! Recovery runs once after acquiring that lease, before accepting operations.

use super::package::read_regular_file;
use super::protocol::validate_plugin_id;
use aead::{Aead, KeyInit, Payload};
use aes_gcm::Aes256Gcm;
use rand::RngExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

pub(crate) const MAX_SETTINGS_PLAINTEXT_BYTES: usize = 32 * 1024;
pub(crate) const MAX_SETTINGS_FILE_BYTES: usize = 64 * 1024;
const MAX_SETTINGS_RECORDS: usize = 64;
const MAX_PENDING_FILES: usize = 64;
const MAX_SETTINGS_DIRECTORY_BYTES: usize = 8 * 1024 * 1024;
const PENDING_PREFIX: &str = "settings-pending-";
const MAGIC: &[u8] = b"IDSET\x01";
const NONCE_BYTES: usize = 12;
const TAG_BYTES: usize = 16;
const AAD_DOMAIN: &[u8] = b"inverter-desktop/plugin-settings/v1\0";

pub(crate) type SettingsKeyProvider = Arc<dyn Fn() -> Result<Vec<u8>, String> + Send + Sync>;

/// Ciphertext metadata only; does not infer ownership or encrypted data validity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SettingsRecord {
    pub record_id: String,
    pub revision: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SettingsInventory {
    pub records: Vec<SettingsRecord>,
    /// Includes pending transactions because they consume the same storage quota.
    pub total_bytes: u64,
    pub max_records: usize,
    pub max_bytes: u64,
}

/// Secret field names remain classified even after a package schema changes.
/// Deliberately lacks Debug: neither diagnostics nor UI receive secret values.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SettingsData {
    pub revision: String,
    pub values: BTreeMap<String, Value>,
    pub secrets: BTreeMap<String, String>,
    pub secret_fields: BTreeSet<String>,
}

impl Default for SettingsData {
    fn default() -> Self {
        Self {
            revision: "0".into(),
            values: BTreeMap::new(),
            secrets: BTreeMap::new(),
            secret_fields: BTreeSet::new(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct SettingsStore {
    root: PathBuf,
    directory: PathBuf,
    key_provider: SettingsKeyProvider,
}

struct DirectoryUsage {
    records: usize,
    bytes: usize,
    pending: Vec<PathBuf>,
    inventory: Vec<SettingsRecord>,
}

pub(crate) struct PreparedSettingsWrite {
    store: SettingsStore,
    destination: PathBuf,
    pending: Option<PathBuf>,
    digest: [u8; 32],
}

impl SettingsStore {
    /// Construction neither touches the filesystem nor requests a credential key.
    pub(crate) fn new(package_root: PathBuf, key_provider: SettingsKeyProvider) -> Self {
        Self {
            directory: package_root.join("settings"),
            root: package_root,
            key_provider,
        }
    }

    /// Run only at startup with the package lease, before publishing the manager.
    /// Never call while a prepared write is alive: its file is intentionally pending.
    pub(crate) fn recover(&self) -> Result<(), String> {
        if !self.directory_exists()? {
            return Ok(());
        }
        let usage = self.scan()?;
        for pending in usage.pending {
            fs::remove_file(pending).map_err(|_| "Cannot recover plugin settings transaction")?;
        }
        sync_directory(&self.directory)
    }

    pub(crate) fn read(&self, plugin_id: &str) -> Result<SettingsData, String> {
        let path = self.record_path(plugin_id)?;
        if !self.directory_exists()? {
            return Ok(SettingsData::default());
        }
        self.scan()?;
        if !file_exists(&path)? {
            return Ok(SettingsData::default());
        }
        let encrypted = read_private_file(&path)?;
        let key = self.key()?;
        decrypt(plugin_id, &encrypted, &key)
    }

    /// Inspect bounded, safe records without decrypting or requesting a key.
    /// Corrupt ciphertext remains visible so an explicit cleanup can remove it.
    pub(crate) fn inventory(&self) -> Result<SettingsInventory, String> {
        let mut inventory = SettingsInventory {
            records: Vec::new(),
            total_bytes: 0,
            max_records: MAX_SETTINGS_RECORDS,
            max_bytes: MAX_SETTINGS_DIRECTORY_BYTES as u64,
        };
        if self.directory_exists()? {
            let usage = self.scan()?;
            inventory.records = usage.inventory;
            inventory.total_bytes = usage.bytes as u64;
        }
        Ok(inventory)
    }

    /// Call only under the package operation and authority locks after the
    /// manager has excluded every installed plugin identity from deletion.
    /// Inspect only the selected record; deletion cannot increase storage usage.
    pub(crate) fn remove_record(&self, record_id: &str, revision: &str) -> Result<(), String> {
        if !is_lowercase_digest(record_id) || !is_lowercase_digest(revision) {
            return Err("Invalid retained plugin settings record token".into());
        }
        let stale = "Retained plugin settings record changed; refresh before deleting";
        if !self.directory_exists()? {
            return Err(stale.into());
        }
        let path = self.directory.join(format!("{record_id}.enc"));
        if !file_exists(&path)? {
            return Err(stale.into());
        }
        let actual = format!("{:x}", Sha256::digest(read_private_file(&path)?));
        if actual != revision {
            return Err(stale.into());
        }
        fs::remove_file(path).map_err(|_| "Cannot remove retained plugin settings")?;
        sync_directory(&self.directory).map_err(|_| {
            "Retained plugin settings were removed but durability could not be confirmed".into()
        })
    }

    /// The caller supplies a fresh revision for changed data. No revision or
    /// secret is written unencrypted, and no destination changes before commit.
    pub(crate) fn prepare_write(
        &self,
        plugin_id: &str,
        data: &SettingsData,
    ) -> Result<PreparedSettingsWrite, String> {
        let destination = self.record_path(plugin_id)?;
        validate_data(data)?;
        let plaintext = encode_data(data)?;
        let encrypted = encrypt(plugin_id, &plaintext, &self.key()?)?;
        self.ensure_directory()?;
        let usage = self.scan()?;
        if usage.records >= MAX_SETTINGS_RECORDS && !file_exists(&destination)? {
            return Err("Retained plugin settings record limit reached".into());
        }
        if usage.pending.len() >= MAX_PENDING_FILES
            || usage.bytes.saturating_add(encrypted.len()) > MAX_SETTINGS_DIRECTORY_BYTES
        {
            return Err("Plugin settings storage limit reached".into());
        }
        let pending = self
            .directory
            .join(format!("{PENDING_PREFIX}{}", uuid::Uuid::new_v4()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&pending)
            .map_err(|_| "Cannot create plugin settings transaction")?;
        let prepared = PreparedSettingsWrite {
            store: self.clone(),
            destination,
            pending: Some(pending),
            digest: Sha256::digest(&encrypted).into(),
        };
        let staged = (|| {
            file.write_all(&encrypted)
                .map_err(|_| "Cannot write plugin settings transaction")?;
            file.sync_all()
                .map_err(|_| "Cannot persist plugin settings transaction")
        })();
        drop(file);
        staged?;
        Ok(prepared)
    }

    /// Explicit deletion only, under the package lease/operation/authority locks
    /// and after the corresponding worker has been reaped. Does not need a key.
    pub(crate) fn remove(&self, plugin_id: &str) -> Result<(), String> {
        let path = self.record_path(plugin_id)?;
        if !self.directory_exists()? || !file_exists(&path)? {
            return Ok(());
        }
        read_private_file(&path)?;
        fs::remove_file(path).map_err(|_| "Cannot remove plugin settings")?;
        sync_directory(&self.directory)
    }

    pub(crate) fn record_id(plugin_id: &str) -> Result<String, String> {
        validate_plugin_id(plugin_id).map_err(|_| "Invalid plugin settings identity")?;
        // Names such as con.example are valid plugin IDs but reserved filenames
        // on Windows. A fixed lowercase digest is portable on all target systems.
        Ok(format!("{:x}", Sha256::digest(plugin_id.as_bytes())))
    }

    fn record_path(&self, plugin_id: &str) -> Result<PathBuf, String> {
        Ok(self
            .directory
            .join(format!("{}.enc", Self::record_id(plugin_id)?)))
    }

    fn key(&self) -> Result<Vec<u8>, String> {
        let key = (self.key_provider)().map_err(|_| "Plugin settings key is unavailable")?;
        if key.len() != 32 {
            return Err("Invalid plugin settings key".into());
        }
        Ok(key)
    }

    fn directory_exists(&self) -> Result<bool, String> {
        if !self.root.is_absolute()
            || self.root.parent().is_none()
            || self
                .root
                .components()
                .any(|part| matches!(part, Component::ParentDir))
        {
            return Err("Invalid plugin settings location".into());
        }
        if !directory_exists(&self.root)? {
            return Ok(false);
        }
        directory_exists(&self.directory)
    }

    fn ensure_directory(&self) -> Result<(), String> {
        if self.directory_exists()? {
            return Ok(());
        }
        if !directory_exists(&self.root)? {
            return Err("Plugin package store is unavailable".into());
        }
        let builder = fs::DirBuilder::new();
        #[cfg(unix)]
        let builder = {
            use std::os::unix::fs::DirBuilderExt;
            let mut builder = builder;
            builder.mode(0o700);
            builder
        };
        builder
            .create(&self.directory)
            .map_err(|_| "Cannot create private plugin settings directory")?;
        sync_directory(&self.root)
    }

    fn scan(&self) -> Result<DirectoryUsage, String> {
        let mut usage = DirectoryUsage {
            records: 0,
            bytes: 0,
            pending: Vec::new(),
            inventory: Vec::new(),
        };
        for entry in
            fs::read_dir(&self.directory).map_err(|_| "Cannot inspect plugin settings directory")?
        {
            let entry = entry.map_err(|_| "Cannot inspect plugin settings entry")?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| "Invalid plugin settings filename")?;
            if let Some(id) = name.strip_prefix(PENDING_PREFIX) {
                let uuid = uuid::Uuid::parse_str(id)
                    .map_err(|_| "Invalid plugin settings transaction filename")?;
                if uuid.to_string() != id {
                    return Err("Invalid plugin settings transaction filename".into());
                }
                usage.pending.push(entry.path());
            } else if name.strip_suffix(".enc").is_some_and(is_lowercase_digest) {
                usage.records += 1;
            } else {
                return Err("Unexpected file in plugin settings directory".into());
            }
            if usage.records > MAX_SETTINGS_RECORDS || usage.pending.len() > MAX_PENDING_FILES {
                return Err("Plugin settings file limit exceeded".into());
            }
            let bytes = read_private_file(&entry.path())?;
            usage.bytes = usage
                .bytes
                .checked_add(bytes.len())
                .ok_or("Plugin settings storage limit exceeded")?;
            if usage.bytes > MAX_SETTINGS_DIRECTORY_BYTES {
                return Err("Plugin settings storage limit exceeded".into());
            }
            if let Some(record_id) = name.strip_suffix(".enc") {
                usage.inventory.push(SettingsRecord {
                    record_id: record_id.to_owned(),
                    revision: format!("{:x}", Sha256::digest(&bytes)),
                    bytes: bytes.len() as u64,
                });
            }
        }
        usage
            .inventory
            .sort_by(|left, right| left.record_id.cmp(&right.record_id));
        Ok(usage)
    }
}

fn is_lowercase_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

impl PreparedSettingsWrite {
    /// Call under the authority lock while still holding the package operation
    /// lock. Expensive encryption and staging have already completed.
    pub(crate) fn commit(mut self) -> Result<(), String> {
        if !self.store.directory_exists()? {
            return Err("Plugin settings directory disappeared before commit".into());
        }
        let pending = self
            .pending
            .as_ref()
            .ok_or("Missing plugin settings transaction")?;
        let actual: [u8; 32] = Sha256::digest(read_private_file(pending)?).into();
        if actual != self.digest {
            return Err("Plugin settings transaction changed before commit".into());
        }
        if file_exists(&self.destination)? {
            read_private_file(&self.destination)?;
        }
        fs::rename(pending, &self.destination)
            .map_err(|_| "Cannot commit plugin settings transaction")?;
        self.pending.take();
        sync_directory(&self.store.directory)
            .map_err(|_| "Plugin settings were saved but durability could not be confirmed".into())
    }
}

impl Drop for PreparedSettingsWrite {
    fn drop(&mut self) {
        if let Some(pending) = self.pending.take() {
            // This path was created exclusively for this transaction; never
            // follow a replaced parent into another directory during cleanup.
            if self.store.directory_exists().ok() == Some(true) {
                let _ = fs::remove_file(pending);
            }
        }
    }
}

fn validate_data(data: &SettingsData) -> Result<(), String> {
    if data.revision != "0"
        && uuid::Uuid::parse_str(&data.revision)
            .map(|id| id.to_string() != data.revision)
            .unwrap_or(true)
    {
        return Err("Invalid plugin settings revision".into());
    }
    if data
        .secrets
        .keys()
        .any(|key| !data.secret_fields.contains(key))
        || data
            .values
            .keys()
            .any(|key| data.secret_fields.contains(key))
    {
        return Err("Invalid plugin settings secret classification".into());
    }
    Ok(())
}

struct BoundedBytes(Vec<u8>);

impl Write for BoundedBytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > MAX_SETTINGS_PLAINTEXT_BYTES {
            return Err(std::io::Error::other(
                "Plugin settings exceed their byte limit",
            ));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn encode_data(data: &SettingsData) -> Result<Vec<u8>, String> {
    let mut bytes = BoundedBytes(Vec::new());
    serde_json::to_writer(&mut bytes, data).map_err(|_| "Cannot encode bounded plugin settings")?;
    Ok(bytes.0)
}

fn aad(plugin_id: &str) -> Vec<u8> {
    let mut result = AAD_DOMAIN.to_vec();
    result.extend_from_slice(plugin_id.as_bytes());
    result
}

fn encrypt(plugin_id: &str, plaintext: &[u8], key: &[u8]) -> Result<Vec<u8>, String> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| "Invalid plugin settings key")?;
    let mut nonce_bytes = [0u8; NONCE_BYTES];
    rand::rng().fill(&mut nonce_bytes);
    let nonce = <aes_gcm::Nonce<aead::consts::U12>>::try_from(nonce_bytes.as_slice())
        .map_err(|_| "Invalid plugin settings nonce")?;
    let ciphertext = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: plaintext,
                aad: &aad(plugin_id),
            },
        )
        .map_err(|_| "Cannot encrypt plugin settings")?;
    let mut encrypted = MAGIC.to_vec();
    encrypted.extend_from_slice(&nonce_bytes);
    encrypted.extend_from_slice(&ciphertext);
    if encrypted.len() > MAX_SETTINGS_FILE_BYTES {
        return Err("Encrypted plugin settings exceed their byte limit".into());
    }
    Ok(encrypted)
}

fn decrypt(plugin_id: &str, encrypted: &[u8], key: &[u8]) -> Result<SettingsData, String> {
    if encrypted.len() < MAGIC.len() + NONCE_BYTES + TAG_BYTES
        || !encrypted.starts_with(MAGIC)
        || encrypted.len() > MAX_SETTINGS_FILE_BYTES
    {
        return Err("Invalid encrypted plugin settings".into());
    }
    let nonce = <aes_gcm::Nonce<aead::consts::U12>>::try_from(
        &encrypted[MAGIC.len()..MAGIC.len() + NONCE_BYTES],
    )
    .map_err(|_| "Invalid plugin settings nonce")?;
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| "Invalid plugin settings key")?;
    let plaintext = cipher
        .decrypt(
            &nonce,
            Payload {
                msg: &encrypted[MAGIC.len() + NONCE_BYTES..],
                aad: &aad(plugin_id),
            },
        )
        .map_err(|_| "Plugin settings authentication failed")?;
    if plaintext.len() > MAX_SETTINGS_PLAINTEXT_BYTES {
        return Err("Decrypted plugin settings exceed their byte limit".into());
    }
    // Never expose serde's data-bearing diagnostics, which can echo a secret.
    let data: SettingsData =
        serde_json::from_slice(&plaintext).map_err(|_| "Invalid decrypted plugin settings")?;
    validate_data(&data)?;
    Ok(data)
}

fn directory_exists(path: &Path) -> Result<bool, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err("Cannot inspect plugin settings directory".into()),
    };
    if !metadata.is_dir() || is_link(&metadata) {
        return Err("Plugin settings directory is a link or non-directory".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("Plugin settings directory is not private".into());
        }
    }
    Ok(true)
}

fn file_exists(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err("Cannot inspect plugin settings file".into()),
    }
}

fn is_link(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    false
}

fn read_private_file(path: &Path) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "Cannot inspect plugin settings file")?;
    if !metadata.is_file() || is_link(&metadata) {
        return Err("Plugin settings file is a link or non-file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("Plugin settings file is not private".into());
        }
    }
    read_regular_file(path, MAX_SETTINGS_FILE_BYTES)
        .map_err(|_| "Plugin settings file must be bounded, regular, and unlinked".into())
}

fn sync_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    fs::File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|_| "Cannot persist plugin settings directory")?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
#[path = "settings_store_tests.rs"]
mod tests;
