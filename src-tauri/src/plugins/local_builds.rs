//! Explicit same-account developer overrides; release declarations stay intact.
//! Files never come from the webview. Normal pinned verification and transactional
//! activation still apply, and changing the configured release pin supersedes them.

use super::package::{read_regular_file, MAX_ARCHIVE_BYTES};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    schema_version: u32,
    target: String,
    build: String,
    packages: Vec<LocalPackage>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LocalPackage {
    pub version: String,
    pub sha256: String,
    plugin_id: String,
    file: String,
    baseline_sha256: String,
}

pub(super) struct LocalArchive {
    pub package: LocalPackage,
    path: PathBuf,
}

impl LocalArchive {
    pub fn read(&self) -> Result<Vec<u8>, String> {
        checked_directory(self.path.parent().ok_or("Invalid local archive path")?)?;
        read_regular_file(&self.path, MAX_ARCHIVE_BYTES)
    }
}

fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn checked_directory(path: &Path) -> Result<(), String> {
    for parent in path.ancestors() {
        let metadata = std::fs::symlink_metadata(parent)
            .map_err(|_| "Cannot inspect local plugin directory")?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("Local plugin directories must not be symbolic links".into());
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err("Local plugin directories must not be reparse points".into());
            }
        }
    }
    Ok(())
}

pub(super) fn selected(
    root: &Path,
    target: &str,
    id: &str,
    baseline: &str,
) -> Result<Option<LocalArchive>, String> {
    let selection = root.join("local-plugin-overrides.json");
    match std::fs::symlink_metadata(&selection) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Cannot inspect local plugin overrides".into()),
        Ok(_) => (),
    }
    checked_directory(root)?;
    let selection: Selection = serde_json::from_slice(&read_regular_file(&selection, 16 * 1024)?)
        .map_err(|_| "Invalid local plugin overrides")?;
    if selection.schema_version != 1
        || selection.target != target
        || !hex(&selection.build, 32)
        || selection.packages.is_empty()
        || selection.packages.len() > 4
    {
        return Err("Invalid local plugin override identity".into());
    }
    let mut ids = BTreeSet::new();
    for package in &selection.packages {
        super::protocol::validate_plugin_id(&package.plugin_id)?;
        if !ids.insert(&package.plugin_id)
            || !hex(&package.sha256, 64)
            || !hex(&package.baseline_sha256, 64)
            || semver::Version::parse(&package.version).is_err()
            || package.file
                != format!(
                    "{}-{}-{}.idplugin",
                    package.plugin_id, package.version, target
                )
        {
            return Err("Invalid local plugin override package".into());
        }
    }
    Ok(selection
        .packages
        .into_iter()
        .find(|package| package.plugin_id == id && package.baseline_sha256 == baseline)
        .map(|package| LocalArchive {
            path: root
                .join("local-plugin-builds")
                .join(selection.build)
                .join(&package.file),
            package,
        }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn selection_rejects_wrong_target_duplicate_ids_and_path_traversal() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let target = "aarch64-apple-darwin";
        let id = "inverter-desktop.frigate";
        let baseline = "a".repeat(64);
        let valid = json!({"schema_version":1, "target":target, "build":"b".repeat(32),
            "packages":[{"plugin_id":id,"version":"1.0.0","sha256":"c".repeat(64),
                "baseline_sha256":baseline,"file":format!("{id}-1.0.0-{target}.idplugin")}]});
        let path = root.join("local-plugin-overrides.json");
        for mutation in ["target", "build", "file", "duplicate"] {
            let mut value = valid.clone();
            match mutation {
                "target" => value["target"] = json!("x86_64-apple-darwin"),
                "build" => value["build"] = json!("../outside"),
                "file" => value["packages"][0]["file"] = json!("../outside.idplugin"),
                _ => {
                    let package = value["packages"][0].clone();
                    value["packages"].as_array_mut().unwrap().push(package);
                }
            }
            std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
            assert!(
                selected(&root, target, id, &baseline).is_err(),
                "{mutation}"
            );
        }
        std::fs::write(&path, serde_json::to_vec(&valid).unwrap()).unwrap();
        assert!(selected(&root, target, id, &"d".repeat(64))
            .unwrap()
            .is_none());
        assert!(selected(&root, target, id, &baseline).unwrap().is_some());
        #[cfg(unix)]
        {
            std::fs::rename(&path, root.join("linked.json")).unwrap();
            std::os::unix::fs::symlink(root.join("linked.json"), &path).unwrap();
            assert!(selected(&root, target, id, &baseline).is_err());
        }
    }
}
