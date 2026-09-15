//! Reproducible signed package creation for externally supplied publisher keys.
//!
//! This API only reads source files and creates archive bytes. It does not install
//! packages, register trusted publishers, or execute workers.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Component, Path, PathBuf};

use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, System, ZipWriter};

use super::package::{
    canonical_manifest_bytes, manifest_signing_payload, read_regular_file, MAX_ARCHIVE_BYTES,
};
use super::protocol::{
    validate_package_path, InventoryEntry, PluginManifest, SignatureMetadata, MAX_MANIFEST_BYTES,
};

const MAX_SOURCE_FILES: usize = 128;
const MAX_SOURCE_ENTRIES: usize = 512;
// Reserve the maximum manifest and classic ZIP metadata before reading payloads.
const MAX_SOURCE_BYTES: usize =
    MAX_ARCHIVE_BYTES - MAX_MANIFEST_BYTES - MAX_SOURCE_FILES * (76 + 2 * 240) - 128;

struct Sources {
    files: BTreeMap<String, Vec<u8>>,
    spelling: HashMap<String, String>,
    entries: usize,
    bytes: usize,
}

/// Build a deterministic package from all regular files beneath `source_root`.
///
/// The supplied inventory and signature are replaced with hashes of the actual
/// payload and an Ed25519 signature from `signing_key`. The manifest must reside
/// outside the payload directory, which cannot contain `manifest.json` or any
/// symlink/reparse point, including one in the source root's parent path. Unix
/// hard links are rejected. File timestamps and source permissions never enter
/// the archive; only the entrypoint gets 0755. Sources must remain unchanged while
/// being collected; this publisher utility is not a sandbox for hostile local code.
pub fn build_package(
    mut manifest: PluginManifest,
    source_root: &Path,
    key_id: &str,
    signing_key: &SigningKey,
) -> Result<Vec<u8>, String> {
    // Check metadata before reading a potentially large source tree. The real
    // inventory below replaces this placeholder before any signature is made.
    manifest.inventory = vec![InventoryEntry {
        path: manifest.entrypoint.clone(),
        size: 1,
        sha256: "0".repeat(64),
    }];
    manifest.signature = Some(SignatureMetadata {
        algorithm: "ed25519".into(),
        key_id: key_id.into(),
        signature: "0".repeat(128),
    });
    manifest.validate()?;
    let root = checked_directory(source_root)?;
    let mut sources = Sources {
        files: BTreeMap::new(),
        spelling: HashMap::new(),
        entries: 0,
        bytes: 0,
    };
    sources.collect(&root, "")?;
    let mut seed = signing_key.to_bytes();
    let contains_seed = sources
        .files
        .values()
        .any(|contents| contents.as_slice() == seed);
    seed.fill(0);
    if contains_seed {
        return Err("package payload must not contain the signing key file or a copy of it".into());
    }
    manifest.inventory = sources
        .files
        .iter()
        .map(|(path, contents)| InventoryEntry {
            path: path.clone(),
            size: contents.len() as u64,
            sha256: format!("{:x}", Sha256::digest(contents)),
        })
        .collect();
    manifest.validate()?;
    let signature = signing_key.sign(&manifest_signing_payload(&manifest)?);
    manifest.signature = Some(SignatureMetadata {
        algorithm: "ed25519".into(),
        key_id: key_id.into(),
        signature: signature
            .to_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    });
    manifest.validate()?;
    encode_archive(&manifest, sources.files)
}

impl Sources {
    fn collect(&mut self, directory: &Path, relative: &str) -> Result<(), String> {
        checked_directory(directory)?;
        for entry in fs::read_dir(directory).map_err(|_| "cannot read package source directory")? {
            self.entries += 1;
            if self.entries > MAX_SOURCE_ENTRIES {
                return Err("package source contains too many entries".into());
            }
            let entry = entry.map_err(|_| "cannot read package source entry")?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| "package source filenames must be portable ASCII")?;
            let path = if relative.is_empty() {
                name
            } else {
                format!("{relative}/{name}")
            };
            validate_package_path(&path)?;
            if path.eq_ignore_ascii_case("manifest.json") {
                return Err("manifest.json is reserved for generated package metadata".into());
            }
            let folded = path.to_ascii_lowercase();
            if let Some(previous) = self.spelling.insert(folded, path.clone()) {
                if previous != path {
                    return Err("package source paths collide across desktop platforms".into());
                }
            }
            let metadata = fs::symlink_metadata(entry.path())
                .map_err(|_| "cannot inspect package source entry")?;
            if is_link(&metadata) {
                return Err("package sources must not contain symlinks or reparse points".into());
            }
            if metadata.is_dir() {
                self.collect(&entry.path(), &path)?;
            } else if metadata.is_file() {
                self.read_file(entry.path(), path, metadata)?;
            } else {
                return Err(
                    "package sources must contain only regular files and directories".into(),
                );
            }
        }
        Ok(())
    }

    fn read_file(
        &mut self,
        source: PathBuf,
        path: String,
        metadata: fs::Metadata,
    ) -> Result<(), String> {
        if self.files.len() >= MAX_SOURCE_FILES {
            return Err("package source contains too many files".into());
        }
        let remaining = MAX_SOURCE_BYTES - self.bytes;
        if metadata.len() > remaining as u64 {
            return Err("package source exceeds the archive size limit".into());
        }
        let contents = read_regular_file(&source, remaining)?;
        if contents.len() > remaining || contents.len() as u64 != metadata.len() {
            return Err("package source changed size during collection".into());
        }
        checked_directory(source.parent().ok_or("missing package source parent")?)?;
        let final_metadata = fs::symlink_metadata(&source)
            .map_err(|_| "package source changed during collection")?;
        if is_link(&final_metadata)
            || !final_metadata.is_file()
            || !same_file(&metadata, &final_metadata)
        {
            return Err("package source changed during collection".into());
        }
        self.bytes += contents.len();
        self.files.insert(path, contents);
        Ok(())
    }
}

#[cfg(windows)]
fn is_link(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_link(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(unix)]
fn same_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(not(unix))]
fn same_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    left.len() == right.len()
        && left.created().ok() == right.created().ok()
        && left.modified().ok() == right.modified().ok()
}

/// Resolve a directory lexically while rejecting symlinks in every component.
fn checked_directory(path: &Path) -> Result<PathBuf, String> {
    // Joining onto a Windows verbatim working directory normalizes `..` away.
    // Reject lexical traversal before constructing the absolute path.
    if path
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return Err("parent traversal is not allowed in source paths".into());
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|_| "cannot locate current directory")?
            .join(path)
    };
    let mut checked = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => checked.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err("parent traversal is not allowed in source paths".into())
            }
            Component::Normal(name) => {
                checked.push(name);
                let metadata = fs::symlink_metadata(&checked)
                    .map_err(|_| "cannot inspect package directory")?;
                if is_link(&metadata) || !metadata.is_dir() {
                    return Err("package directory components must be real directories".into());
                }
            }
        }
    }
    Ok(checked)
}

fn encode_archive(
    manifest: &PluginManifest,
    sources: BTreeMap<String, Vec<u8>>,
) -> Result<Vec<u8>, String> {
    let fixed_time = DateTime::from_date_and_time(1980, 1, 1, 0, 0, 0)
        .map_err(|_| "invalid fixed package timestamp")?;
    let options = SimpleFileOptions::default()
        // ZIP otherwise records the build machine's OS (DOS on Windows), which
        // changes archive bytes even for identical signed metadata and payloads.
        .system(System::Unix)
        .compression_method(CompressionMethod::Stored)
        .last_modified_time(fixed_time)
        .unix_permissions(0o644)
        .large_file(false);
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file("manifest.json", options)
        .map_err(|_| "cannot create package manifest record")?;
    writer
        .write_all(&canonical_manifest_bytes(manifest)?)
        .map_err(|_| "cannot write package manifest")?;
    for (path, contents) in sources {
        let permissions = if path == manifest.entrypoint {
            0o755
        } else {
            0o644
        };
        writer
            .start_file(path, options.unix_permissions(permissions))
            .map_err(|_| "cannot create package payload record")?;
        writer
            .write_all(&contents)
            .map_err(|_| "cannot write package payload")?;
    }
    let archive = writer
        .finish()
        .map_err(|_| "cannot finalize package archive")?
        .into_inner();
    if archive.len() > MAX_ARCHIVE_BYTES {
        return Err("package archive exceeds its byte limit".into());
    }
    Ok(archive)
}

/// Publish a complete archive without replacing any existing file or symlink.
/// The temporary file is created in the destination directory and cleaned up on
/// failure; only fully written and synced bytes become visible at `destination`.
pub fn write_package_atomic(destination: &Path, archive: &[u8]) -> Result<(), String> {
    if archive.is_empty() || archive.len() > MAX_ARCHIVE_BYTES {
        return Err("empty or oversized package archive".into());
    }
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = checked_directory(parent)?;
    let name = destination
        .file_name()
        .ok_or("package output must name a file")?;
    let output = parent.join(name);
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|_| "cannot create temporary package output")?;
    temporary
        .write_all(archive)
        .map_err(|_| "cannot write package output")?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|_| "cannot sync package output")?;
    temporary
        .persist_noclobber(output)
        .map_err(|_| "cannot publish package output; destination must not already exist")?;
    Ok(())
}

#[cfg(test)]
#[path = "packaging_tests.rs"]
mod tests;
