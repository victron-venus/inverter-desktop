//! Verification of the deliberately small, stored-only `.idplugin` ZIP format.
//!
//! Authorization comes from native publisher policy or an exact configured
//! archive pin; a package cannot introduce a key or authorize its own digest.
//! See `docs/plugin-packages.md` for the signing bytes and archive restrictions.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::{Cursor, Read};
use std::ops::Range;
use std::path::Path;

use ed25519_dalek::{Signature, VerifyingKey};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::protocol::{
    parse_manifest, validate_package_path, validate_plugin_id, PluginManifest, MAX_MANIFEST_BYTES,
};

pub const MAX_ARCHIVE_BYTES: usize = 64 * 1024 * 1024;
const MAX_ARCHIVE_FILES: usize = 129;
const SIGNING_DOMAIN: &[u8] = b"inverter-desktop:idplugin:manifest:v1\0";
const MANIFEST_PATH: &str = "manifest.json";

/// A publisher key and its exact allowed plugin identities, from trusted policy.
#[derive(Debug)]
pub struct PublisherTrust {
    key_id: String,
    key: VerifyingKey,
    plugin_ids: HashSet<String>,
}

impl PublisherTrust {
    pub fn new(
        key_id: String,
        public_key: [u8; 32],
        plugin_ids: Vec<String>,
    ) -> Result<Self, String> {
        if key_id.is_empty()
            || key_id.len() > 128
            || !key_id.as_bytes()[0].is_ascii_alphanumeric()
            || !key_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte))
        {
            return Err("invalid publisher key identifier".into());
        }
        if plugin_ids.is_empty() || plugin_ids.len() > 128 {
            return Err(
                "publisher must authorize a bounded list of exact plugin identities".into(),
            );
        }
        let mut identities = HashSet::new();
        for plugin_id in plugin_ids {
            validate_plugin_id(&plugin_id)?;
            if !identities.insert(plugin_id) {
                return Err("duplicate publisher plugin identity".into());
            }
        }
        let key = VerifyingKey::from_bytes(&public_key)
            .map_err(|_| "invalid publisher public key".to_string())?;
        if key.is_weak() {
            return Err("weak publisher public key".into());
        }
        Ok(Self {
            key_id,
            key,
            plugin_ids: identities,
        })
    }
}

/// An empty store trusts no packages. It cannot be deserialized from plugin IPC.
#[derive(Debug, Default)]
pub struct TrustStore {
    publishers: HashMap<String, PublisherTrust>,
}

impl TrustStore {
    pub fn is_empty(&self) -> bool {
        self.publishers.is_empty()
    }

    pub fn new(publishers: Vec<PublisherTrust>) -> Result<Self, String> {
        if publishers.len() > 128 {
            return Err("publisher trust store exceeds key limit".into());
        }
        let mut store = Self::default();
        for publisher in publishers {
            if store
                .publishers
                .insert(publisher.key_id.clone(), publisher)
                .is_some()
            {
                return Err("duplicate publisher key identifier".into());
            }
        }
        Ok(store)
    }
}

/// An immutable, owned verification result. Installation must consume these
/// bytes rather than reopening the caller's original archive path.
pub struct VerifiedPackage {
    manifest: PluginManifest,
    archive: Vec<u8>,
    archive_sha256: String,
    files: Vec<ArchiveEntry>,
    archive_pin: bool,
}

impl std::fmt::Debug for VerifiedPackage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VerifiedPackage")
            .field("plugin_id", &self.manifest.plugin_id)
            .field("version", &self.manifest.version)
            .field("archive_sha256", &self.archive_sha256)
            .finish_non_exhaustive()
    }
}

impl VerifiedPackage {
    pub fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    pub fn archive_sha256(&self) -> &str {
        &self.archive_sha256
    }

    pub fn archive_bytes(&self) -> &[u8] {
        &self.archive
    }

    pub(crate) fn archive_pin(&self) -> bool {
        self.archive_pin
    }

    /// Preserve native authorization provenance while checking immutable bytes
    /// against the receiving manager's target and publisher policy.
    pub(crate) fn reverify(&self, trust: &TrustStore, target: &str) -> Result<Self, String> {
        if self.archive_pin {
            verify_pinned_archive_bytes(
                self.archive.clone(),
                &self.manifest.plugin_id,
                &self.manifest.version,
                target,
                &self.archive_sha256,
            )
        } else {
            verify_archive_bytes(self.archive.clone(), trust, target)
        }
    }

    /// Only inventory payloads; the signed manifest is available separately.
    pub fn files(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.files
            .iter()
            .skip(1)
            .map(|entry| (entry.path.as_str(), &self.archive[entry.data.clone()]))
    }
}

fn sort_json_objects(value: &mut Value) {
    match value {
        Value::Object(object) => {
            let sorted: BTreeMap<_, _> = std::mem::take(object).into_iter().collect();
            for (key, mut value) in sorted {
                sort_json_objects(&mut value);
                object.insert(key, value);
            }
        }
        Value::Array(values) => values.iter_mut().for_each(sort_json_objects),
        _ => {}
    }
}

/// Compact serde serialization in `PluginManifest` field order; configuration
/// object keys are recursively sorted, independent of serde's map features.
/// Arrays, including inventory and permissions, retain their explicit order.
pub fn canonical_manifest_bytes(manifest: &PluginManifest) -> Result<Vec<u8>, String> {
    manifest.validate()?;
    let mut canonical = manifest.clone();
    sort_json_objects(&mut canonical.config_schema);
    serde_json::to_vec(&canonical).map_err(|_| "cannot serialize plugin manifest".into())
}

/// Signature input is the literal domain (including NUL), followed by canonical
/// compact manifest JSON with the `signature` field set to JSON `null`.
pub fn manifest_signing_payload(manifest: &PluginManifest) -> Result<Vec<u8>, String> {
    let mut unsigned = manifest.clone();
    unsigned.signature = None;
    let mut payload = SIGNING_DOMAIN.to_vec();
    payload.extend(canonical_manifest_bytes(&unsigned)?);
    Ok(payload)
}

fn decode_signature(value: &str) -> Result<Signature, String> {
    let mut bytes = [0_u8; 64];
    if value.len() != bytes.len() * 2 {
        return Err("invalid publisher signature".into());
    }
    for (byte, pair) in bytes.iter_mut().zip(value.as_bytes().as_chunks::<2>().0) {
        let digit = |value: u8| match value {
            b'0'..=b'9' => Ok(value - b'0'),
            b'a'..=b'f' => Ok(value - b'a' + 10),
            _ => Err("invalid publisher signature".to_string()),
        };
        *byte = digit(pair[0])? * 16 + digit(pair[1])?;
    }
    Ok(Signature::from_bytes(&bytes))
}

fn verify_publisher(manifest: &PluginManifest, trust: &TrustStore) -> Result<(), String> {
    let signature = manifest
        .signature
        .as_ref()
        .ok_or("plugin package must have a publisher signature")?;
    let publisher = trust
        .publishers
        .get(&signature.key_id)
        .ok_or("plugin publisher is not trusted")?;
    if !publisher.plugin_ids.contains(&manifest.plugin_id) {
        return Err("publisher is not authorized for this plugin identity".into());
    }
    publisher
        .key
        .verify_strict(
            &manifest_signing_payload(manifest)?,
            &decode_signature(&signature.signature)?,
        )
        .map_err(|_| "plugin publisher signature verification failed".into())
}

/// Reads at most the package limit plus one sentinel byte before verification.
pub fn verify_archive(
    path: &Path,
    trust: &TrustStore,
    target: &str,
) -> Result<VerifiedPackage, String> {
    verify_archive_bytes(read_regular_file(path, MAX_ARCHIVE_BYTES)?, trust, target)
}

/// Open the final component without following symlinks/reparse points and read
/// an ordinary bounded file. Callers must also validate their parent directory
/// policy; this is not protection from arbitrary same-account native code.
pub fn read_regular_file(path: &Path, max_bytes: usize) -> Result<Vec<u8>, String> {
    if max_bytes > MAX_ARCHIVE_BYTES {
        return Err("plugin file read exceeds allowed limit".into());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_FLAG_OPEN_REPARSE_POINT: inspect the link itself, never its target.
        options.custom_flags(0x0020_0000);
    }
    let file = options
        .open(path)
        .map_err(|_| "cannot open plugin file".to_string())?;
    let metadata = file
        .metadata()
        .map_err(|_| "cannot inspect plugin file".to_string())?;
    if !metadata.is_file() || metadata.len() > max_bytes as u64 {
        return Err("plugin file must be a bounded regular file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Err("plugin file must not be a hard link".into());
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        };
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("plugin file must not be a reparse point".into());
        }
        let mut information = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: `file` keeps this ordinary file handle alive throughout the
        // call, and `information` is valid writable storage of the API's type.
        let success = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) };
        if success == 0 || information.nNumberOfLinks != 1 {
            return Err("plugin file must have exactly one filesystem link".into());
        }
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(max_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read plugin file".to_string())?;
    if bytes.len() > max_bytes {
        return Err("plugin file exceeds read limit".into());
    }
    Ok(bytes)
}

pub fn verify_archive_bytes(
    bytes: Vec<u8>,
    trust: &TrustStore,
    target: &str,
) -> Result<VerifiedPackage, String> {
    verify_archive_contents(bytes, target, false, |manifest, _| {
        verify_publisher(manifest, trust)
    })
}

/// Authorize one exact archive selected in native application configuration.
/// The local configuration and installed inventory share the same-user trust
/// boundary. A URL, package manifest, or downloaded checksum cannot grant this
/// authorization; the expected identity, version, target, and SHA-256 are inputs
/// from the configuration accepted before the download began.
pub fn verify_pinned_archive_bytes(
    bytes: Vec<u8>,
    plugin_id: &str,
    version: &str,
    target: &str,
    sha256: &str,
) -> Result<VerifiedPackage, String> {
    validate_plugin_id(plugin_id)?;
    if version.is_empty()
        || version.len() > 128
        || semver::Version::parse(version).is_err()
        || sha256.len() != 64
        || !sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("invalid configured plugin version or archive digest".into());
    }
    verify_archive_contents(bytes, target, true, |manifest, digest| {
        if manifest.plugin_id != plugin_id || manifest.version != version || digest != sha256 {
            return Err(
                "plugin package does not match its configured identity, version, or archive digest"
                    .into(),
            );
        }
        Ok(())
    })
}

fn verify_archive_contents(
    bytes: Vec<u8>,
    target: &str,
    archive_pin: bool,
    authorize: impl FnOnce(&PluginManifest, &str) -> Result<(), String>,
) -> Result<VerifiedPackage, String> {
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err("plugin package exceeds archive size limit".into());
    }
    let files = scan_archive(&bytes)?;
    let manifest_bytes = &bytes[files[0].data.clone()];
    let manifest = parse_manifest(manifest_bytes)?;
    manifest.validate_for_target(target)?;
    if canonical_manifest_bytes(&manifest)? != manifest_bytes {
        return Err("plugin manifest is not in canonical package encoding".into());
    }
    let archive_sha256 = sha256_hex(&bytes);
    authorize(&manifest, &archive_sha256)?;
    if manifest.inventory.len() != files.len() - 1 {
        return Err("plugin archive does not match its inventory".into());
    }
    let inventory: HashMap<_, _> = manifest
        .inventory
        .iter()
        .map(|item| (item.path.as_str(), item))
        .collect();
    for entry in files.iter().skip(1) {
        let item = inventory
            .get(entry.path.as_str())
            .ok_or("plugin archive contains an unlisted file")?;
        let data = &bytes[entry.data.clone()];
        if item.size != data.len() as u64 || item.sha256 != sha256_hex(data) {
            return Err("plugin file does not match its inventory size and digest".into());
        }
    }
    // The strict scanner bounds all metadata before the ZIP library allocates.
    // Independently read each entry to EOF, which checks ZIP CRC integrity.
    check_zip_reader(&bytes, &files)?;
    Ok(VerifiedPackage {
        manifest,
        archive: bytes,
        archive_sha256,
        files,
        archive_pin,
    })
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Debug)]
struct ArchiveEntry {
    path: String,
    header: Range<usize>,
    data: Range<usize>,
}

fn u16_at(bytes: &[u8], offset: usize) -> Result<u16, String> {
    let field = bytes
        .get(offset..offset + 2)
        .ok_or("truncated plugin ZIP metadata")?;
    Ok(u16::from_le_bytes([field[0], field[1]]))
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let field = bytes
        .get(offset..offset + 4)
        .ok_or("truncated plugin ZIP metadata")?;
    Ok(u32::from_le_bytes([field[0], field[1], field[2], field[3]]))
}

fn check_path_tree(paths: &[ArchiveEntry]) -> Result<(), String> {
    let mut tree: HashMap<String, (&str, bool)> = HashMap::new();
    for entry in paths {
        validate_package_path(&entry.path)?;
        for (end, _) in entry
            .path
            .match_indices('/')
            .chain(std::iter::once((entry.path.len(), "")))
        {
            let part = &entry.path[..end];
            let is_file = end == entry.path.len();
            let key = part.to_ascii_lowercase();
            if let Some((existing, existing_is_file)) = tree.get(&key) {
                if existing != &part || *existing_is_file || is_file {
                    return Err("duplicate, case-colliding, or conflicting plugin ZIP path".into());
                }
            } else {
                tree.insert(key, (part, is_file));
            }
        }
    }
    Ok(())
}

/// Admit only the contiguous classic ZIP subset produced by our packager.
/// This rejects prepended/trailing data, aliases and overlaps before parsing
/// central-directory counts or optional fields in the general ZIP reader.
fn scan_archive(bytes: &[u8]) -> Result<Vec<ArchiveEntry>, String> {
    let end = bytes.len().checked_sub(22).ok_or("truncated plugin ZIP")?;
    if u32_at(bytes, end)? != 0x0605_4b50
        || u16_at(bytes, end + 4)? != 0
        || u16_at(bytes, end + 6)? != 0
        || u16_at(bytes, end + 20)? != 0
    {
        return Err("plugin ZIP must have one disk and no comment or trailing bytes".into());
    }
    let count = usize::from(u16_at(bytes, end + 10)?);
    if !(2..=MAX_ARCHIVE_FILES).contains(&count) || usize::from(u16_at(bytes, end + 8)?) != count {
        return Err("invalid plugin ZIP entry count".into());
    }
    let central_size = u32_at(bytes, end + 12)? as usize;
    let central_start = u32_at(bytes, end + 16)? as usize;
    if central_start.checked_add(central_size) != Some(end) {
        return Err("invalid plugin ZIP central-directory bounds".into());
    }
    let mut files = Vec::with_capacity(count);
    let mut offset = 0;
    for index in 0..count {
        if u32_at(bytes, offset)? != 0x0403_4b50
            || !matches!(u16_at(bytes, offset + 4)?, 10 | 20)
            || u16_at(bytes, offset + 6)? & !0x0800 != 0
            || u16_at(bytes, offset + 8)? != 0
            || u16_at(bytes, offset + 28)? != 0
        {
            return Err("plugin ZIP requires stored entries without optional ZIP features".into());
        }
        let size = u32_at(bytes, offset + 18)? as usize;
        if size != u32_at(bytes, offset + 22)? as usize {
            return Err("stored plugin ZIP sizes disagree".into());
        }
        let name_len = usize::from(u16_at(bytes, offset + 26)?);
        if name_len == 0 || name_len > 240 {
            return Err("invalid plugin ZIP path length".into());
        }
        let data_start = offset
            .checked_add(30 + name_len)
            .ok_or("plugin ZIP size overflow")?;
        let data_end = data_start
            .checked_add(size)
            .ok_or("plugin ZIP size overflow")?;
        if data_end > central_start {
            return Err("overlapping or truncated plugin ZIP data".into());
        }
        let path = std::str::from_utf8(
            bytes
                .get(offset + 30..data_start)
                .ok_or("truncated plugin ZIP filename")?,
        )
        .map_err(|_| "plugin ZIP filename must be UTF-8".to_string())?
        .to_string();
        if index == 0 && (path != MANIFEST_PATH || size > MAX_MANIFEST_BYTES) {
            return Err("bounded manifest.json must be the first plugin ZIP entry".into());
        }
        files.push(ArchiveEntry {
            path,
            header: offset..data_start,
            data: data_start..data_end,
        });
        offset = data_end;
    }
    if offset != central_start {
        return Err("unlisted data before plugin ZIP central directory".into());
    }
    check_path_tree(&files)?;
    if files[1..]
        .windows(2)
        .any(|pair| pair[0].path >= pair[1].path)
    {
        return Err("plugin ZIP payload files must be sorted by portable path".into());
    }
    for entry in &files {
        let creator = u16_at(bytes, offset + 4)? >> 8;
        let external = u32_at(bytes, offset + 38)?;
        let mode = external >> 16;
        let regular_mode = mode & 0o170000 == 0o100000 && mode & 0o7000 == 0;
        let regular = match creator {
            3 => regular_mode,
            // ZIP writers on Windows can retain Unix mode bits while marking
            // the creator as DOS. Validate those bits; never ignore a symlink
            // or special-file mode just because the creator is not Unix.
            0 => mode == 0 || regular_mode,
            _ => false,
        };
        if u32_at(bytes, offset)? != 0x0201_4b50
            || !regular
            || external & 0x10 != 0
            || u16_at(bytes, offset + 30)? != 0
            || u16_at(bytes, offset + 32)? != 0
            || u16_at(bytes, offset + 34)? != 0
            || u16_at(bytes, offset + 36)? != 0
            || u32_at(bytes, offset + 42)? as usize != entry.header.start
        {
            return Err("plugin ZIP central entry is not a plain regular file".into());
        }
        let name_len = usize::from(u16_at(bytes, offset + 28)?);
        let next = offset
            .checked_add(46 + name_len)
            .ok_or("plugin ZIP central-directory size overflow")?;
        if next > end
            || bytes.get(offset + 46..next) != Some(entry.path.as_bytes())
            || bytes.get(offset + 6..offset + 28)
                != bytes.get(entry.header.start + 4..entry.header.start + 26)
        {
            return Err("plugin ZIP local and central metadata disagree".into());
        }
        offset = next;
    }
    if offset != end {
        return Err("unlisted plugin ZIP central-directory data".into());
    }
    Ok(files)
}

fn check_zip_reader(bytes: &[u8], entries: &[ArchiveEntry]) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| "invalid plugin ZIP archive".to_string())?;
    if archive.len() != entries.len() {
        return Err("plugin ZIP reader entry-count mismatch".into());
    }
    let mut buffer = [0_u8; 8192];
    for (index, entry) in entries.iter().enumerate() {
        let mut file = archive
            .by_index(index)
            .map_err(|_| "invalid plugin ZIP file".to_string())?;
        if file.name_raw() != entry.path.as_bytes()
            || file.size() != entry.data.len() as u64
            || file.data_start() != Some(entry.data.start as u64)
            || file.encrypted()
            || !file.is_file()
            || file.compression() != zip::CompressionMethod::Stored
        {
            return Err("plugin ZIP reader metadata mismatch".into());
        }
        let mut read_bytes = 0;
        loop {
            let count = file
                .read(&mut buffer)
                .map_err(|_| "plugin ZIP checksum verification failed".to_string())?;
            if count == 0 {
                break;
            }
            read_bytes += count;
            if read_bytes > entry.data.len() {
                return Err("plugin ZIP reader exceeded declared size".into());
            }
        }
        if read_bytes != entry.data.len() {
            return Err("plugin ZIP reader returned a truncated file".into());
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "package_tests.rs"]
mod tests;
