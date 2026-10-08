//! Single-owner storage for the existing config.json envelope. Do not register
//! this file with plugin-store: its autosave/exit handlers use non-atomic writes.
use serde_json::{Map, Value};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub(super) struct Document {
    original: Option<Vec<u8>>,
    pub(super) values: Map<String, Value>,
}

fn read_bytes(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_file() => {
            return Err("Configuration must be a regular file".into());
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Cannot inspect configuration file".into()),
        Ok(_) => {}
    }
    fs::read(path)
        .map(Some)
        .map_err(|_| "Cannot read configuration file".into())
}

pub(super) fn read(path: &Path) -> Result<Document, String> {
    let original = read_bytes(path)?;
    let values = match &original {
        Some(bytes) => {
            serde_json::from_slice(bytes).map_err(|_| "Invalid configuration document")?
        }
        None => Map::new(),
    };
    Ok(Document { original, values })
}

struct Pending(PathBuf);

impl Drop for Pending {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum CommitOutcome {
    Durable,
    DirectorySyncUnconfirmed,
}

pub(super) fn save(path: &Path, document: &Document) -> Result<CommitOutcome, String> {
    save_with(
        path,
        document,
        |file, bytes| file.write_all(bytes),
        File::sync_all,
        |from, to| fs::rename(from, to),
        sync_directory,
    )
}

pub(super) fn save_with(
    path: &Path,
    document: &Document,
    write: impl FnOnce(&mut File, &[u8]) -> io::Result<()>,
    sync_file: impl FnOnce(&File) -> io::Result<()>,
    rename: impl FnOnce(&Path, &Path) -> io::Result<()>,
    sync_parent: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<CommitOutcome, String> {
    let parent = path.parent().ok_or("Invalid configuration directory")?;
    fs::create_dir_all(parent).map_err(|_| "Cannot create configuration directory")?;
    let pending = Pending(parent.join(format!(".config-{}.tmp", uuid::Uuid::new_v4())));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&pending.0)
        .map_err(|_| "Cannot stage configuration")?;
    let bytes =
        serde_json::to_vec(&document.values).map_err(|_| "Cannot encode configuration document")?;
    write(&mut file, &bytes).map_err(|_| "Cannot write staged configuration")?;
    sync_file(&file).map_err(|_| "Cannot persist staged configuration")?;
    drop(file);
    if read_bytes(path)? != document.original {
        return Err("Configuration changed before commit; retry the operation".into());
    }
    rename(&pending.0, path).map_err(|_| "Cannot commit configuration")?;
    // The rename is the commit point. Every caller must finish applying the
    // committed policy even when confirming crash durability is unavailable.
    Ok(match sync_parent(parent) {
        Ok(()) => CommitOutcome::Durable,
        Err(_) => CommitOutcome::DirectorySyncUnconfirmed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, PathBuf, Document) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.json");
        fs::write(&path, br#"{"config":"old","unrelated":{"preserve":true}}"#).unwrap();
        let mut document = read(&path).unwrap();
        document
            .values
            .insert("config".into(), Value::String("new".into()));
        (directory, path, document)
    }

    #[test]
    fn partial_write_and_rename_failure_preserve_original_document() {
        let (directory, path, document) = fixture();
        let original = fs::read(&path).unwrap();
        let error = save_with(
            &path,
            &document,
            |file, bytes| {
                file.write_all(&bytes[..5])?;
                Err(io::Error::other("injected partial write"))
            },
            File::sync_all,
            |from, to| fs::rename(from, to),
            sync_directory,
        );
        assert!(error.is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
        let error = save_with(
            &path,
            &document,
            |file, bytes| file.write_all(bytes),
            File::sync_all,
            |_, _| Err(io::Error::other("injected rename failure")),
            sync_directory,
        );
        assert!(error.is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
        save(&path, &document).unwrap();
        let saved = read(&path).unwrap();
        assert_eq!(saved.values["config"], "new");
        assert_eq!(saved.values["unrelated"]["preserve"], true);
    }

    #[test]
    fn post_commit_sync_failure_keeps_complete_new_document() {
        let (_directory, path, document) = fixture();
        let error = save_with(
            &path,
            &document,
            |file, bytes| file.write_all(bytes),
            File::sync_all,
            |from, to| fs::rename(from, to),
            |_| Err(io::Error::other("injected directory sync failure")),
        )
        .unwrap();
        assert_eq!(error, CommitOutcome::DirectorySyncUnconfirmed);
        assert_eq!(read(&path).unwrap().values["config"], "new");
    }

    #[test]
    fn stale_document_cannot_replace_newer_file() {
        let (_directory, path, document) = fixture();
        fs::write(&path, br#"{"config":"other"}"#).unwrap();
        assert!(save(&path, &document).is_err());
        assert_eq!(read(&path).unwrap().values["config"], "other");
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_rejected_without_touching_target() {
        let (directory, path, _document) = fixture();
        let link = directory.path().join("link.json");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(read(&link).is_err());
        assert_eq!(read(&path).unwrap().values["config"], "old");
    }
}
