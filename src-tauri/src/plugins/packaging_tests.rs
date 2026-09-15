use super::*;
use crate::plugins::package::{verify_archive_bytes, PublisherTrust, TrustStore};
use crate::plugins::protocol::{PluginPermission, MANIFEST_SCHEMA_VERSION};
use serde_json::json;
use std::fs::{File, FileTimes};
use std::io::Read;
use std::time::{Duration, UNIX_EPOCH};
use tempfile::TempDir;
use zip::ZipArchive;

fn manifest() -> PluginManifest {
    PluginManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        plugin_id: "test.publisher.monitor".into(),
        version: "1.2.3".into(),
        host_api: "^1.0.0".into(),
        target: "x86_64-unknown-linux-gnu".into(),
        entrypoint: "bin/worker".into(),
        config_schema: json!({"type": "object", "properties": {"server": {"type": "string"}}}),
        permissions: vec![PluginPermission::DashboardContributions],
        http_video: None,
        inventory: Vec::new(),
        signature: None,
    }
}

fn source() -> (TempDir, PathBuf) {
    let temporary = TempDir::new().unwrap();
    // macOS's temporary directory may use the /var symlink alias. Use the real
    // parent so the intentional no-symlink source contract also holds in tests.
    let root = temporary.path().canonicalize().unwrap().join("payload");
    fs::create_dir_all(root.join("bin")).unwrap();
    fs::write(root.join("bin/worker"), b"test worker payload").unwrap();
    (temporary, root)
}

fn key() -> SigningKey {
    // Deterministic fixture only; production trust is provided separately.
    SigningKey::from_bytes(&[7; 32])
}

fn build(root: &Path) -> Vec<u8> {
    build_package(manifest(), root, "test-key", &key()).unwrap()
}

#[test]
fn produced_archive_passes_the_installer_verifier_with_scoped_publisher_trust() {
    let (_temporary, root) = source();
    let trust = TrustStore::new(vec![PublisherTrust::new(
        "test-key".into(),
        key().verifying_key().to_bytes(),
        vec!["test.publisher.monitor".into()],
    )
    .unwrap()])
    .unwrap();
    assert!(verify_archive_bytes(build(&root), &trust, "x86_64-unknown-linux-gnu").is_ok());
}

#[test]
fn package_bytes_ignore_source_creation_order_permissions_and_timestamps() {
    let (_first, left) = source();
    let (_second, right) = source();
    fs::write(left.join("z.txt"), b"last").unwrap();
    fs::write(left.join("a.txt"), b"first").unwrap();
    fs::write(right.join("a.txt"), b"first").unwrap();
    fs::write(right.join("z.txt"), b"last").unwrap();
    File::options()
        .write(true)
        .open(left.join("a.txt"))
        .unwrap()
        .set_times(FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(500)))
        .unwrap();
    File::options()
        .write(true)
        .open(right.join("a.txt"))
        .unwrap()
        .set_times(FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(900)))
        .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(left.join("a.txt"), fs::Permissions::from_mode(0o600)).unwrap();
        fs::set_permissions(right.join("a.txt"), fs::Permissions::from_mode(0o755)).unwrap();
    }
    assert_eq!(build(&left), build(&right));
}

#[test]
fn manifest_is_first_payloads_are_sorted_and_actual_file_hashes_are_signed() {
    let (_temporary, root) = source();
    fs::write(root.join("z.txt"), b"last").unwrap();
    fs::write(root.join("a.txt"), b"first").unwrap();
    let archive = build(&root);
    let mut reader = ZipArchive::new(Cursor::new(&archive)).unwrap();
    let names: Vec<_> = (0..reader.len())
        .map(|index| reader.by_index(index).unwrap().name().to_string())
        .collect();
    assert_eq!(names, ["manifest.json", "a.txt", "bin/worker", "z.txt"]);
    let mut bytes = Vec::new();
    reader.by_index(0).unwrap().read_to_end(&mut bytes).unwrap();
    let parsed: PluginManifest = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(parsed.inventory.len(), 3);
    assert_eq!(parsed.inventory[0].path, "a.txt");
    assert_eq!(parsed.inventory[0].size, 5);
    assert_eq!(
        parsed.inventory[0].sha256,
        format!("{:x}", Sha256::digest(b"first"))
    );
    assert_eq!(bytes, canonical_manifest_bytes(&parsed).unwrap());
    assert_eq!(parsed.signature.as_ref().unwrap().key_id, "test-key");
    for index in 0..reader.len() {
        let file = reader.by_index(index).unwrap();
        assert_eq!(
            archive[file.central_header_start() as usize + 5],
            System::Unix as u8
        );
        assert_eq!(file.compression(), CompressionMethod::Stored);
        assert_eq!(
            file.last_modified().unwrap(),
            DateTime::from_date_and_time(1980, 1, 1, 0, 0, 0).unwrap()
        );
        assert_eq!(
            file.unix_mode(),
            Some(if file.name() == "bin/worker" {
                0o100755
            } else {
                0o100644
            })
        );
    }
    let signature = parsed.signature.as_ref().unwrap().signature.as_bytes();
    let mut decoded = [0_u8; 64];
    for (slot, pair) in decoded.iter_mut().zip(signature.as_chunks::<2>().0) {
        *slot = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
    }
    key()
        .verifying_key()
        .verify_strict(
            &manifest_signing_payload(&parsed).unwrap(),
            &ed25519_dalek::Signature::from_bytes(&decoded),
        )
        .unwrap();
}

#[test]
fn source_contents_replace_stale_inventory_and_change_the_signature() {
    let (_temporary, root) = source();
    let first = build(&root);
    let mut metadata = manifest();
    metadata.inventory.push(InventoryEntry {
        path: "old".into(),
        size: 123,
        sha256: "0".repeat(64),
    });
    assert_eq!(
        first,
        build_package(metadata, &root, "test-key", &key()).unwrap()
    );
    fs::write(root.join("bin/worker"), b"changed worker payload").unwrap();
    assert_ne!(first, build(&root));
}

#[test]
fn reserved_names_empty_entrypoints_and_oversized_sources_are_rejected() {
    let (_temporary, root) = source();
    fs::write(root.join("manifest.json"), b"not generated").unwrap();
    assert!(build_package(manifest(), &root, "test-key", &key()).is_err());
    fs::remove_file(root.join("manifest.json")).unwrap();
    fs::write(root.join("bin/worker"), b"").unwrap();
    assert!(build_package(manifest(), &root, "test-key", &key()).is_err());
    File::create(root.join("bin/worker"))
        .unwrap()
        .set_len(MAX_ARCHIVE_BYTES as u64)
        .unwrap();
    assert!(build_package(manifest(), &root, "test-key", &key()).is_err());
}

#[test]
fn unportable_source_names_and_parent_traversal_are_rejected() {
    let (_temporary, root) = source();
    fs::write(root.join(".hidden"), b"invalid").unwrap();
    assert!(build_package(manifest(), &root, "test-key", &key()).is_err());
    fs::remove_file(root.join(".hidden")).unwrap();
    // PathBuf::join normalizes `..` for Windows canonical (verbatim) roots.
    // Preserve the literal input so this fixture actually exercises rejection.
    let mut literal = root.as_os_str().to_os_string();
    literal.push(format!(
        "{}bin{}..",
        std::path::MAIN_SEPARATOR,
        std::path::MAIN_SEPARATOR
    ));
    let traversing = PathBuf::from(literal);
    assert!(traversing
        .components()
        .any(|component| component == Component::ParentDir));
    assert!(build_package(manifest(), &traversing, "test-key", &key()).is_err());
    assert_eq!(
        checked_directory(Path::new("bin/..")).unwrap_err(),
        "parent traversal is not allowed in source paths"
    );
    let mut metadata = manifest();
    metadata.entrypoint = "../worker".into();
    assert!(build_package(metadata, &root, "test-key", &key()).is_err());
}

#[test]
fn excessive_source_file_count_is_rejected_before_encoding() {
    let (_temporary, root) = source();
    for index in 0..MAX_SOURCE_FILES {
        fs::write(root.join(format!("file-{index}")), b"x").unwrap();
    }
    assert!(build_package(manifest(), &root, "test-key", &key()).is_err());
}

#[test]
fn private_signing_seed_or_a_hardlink_copy_cannot_be_a_payload() {
    let (temporary, root) = source();
    let seed_path = temporary
        .path()
        .canonicalize()
        .unwrap()
        .join("private-seed");
    fs::write(&seed_path, key().to_bytes()).unwrap();
    fs::hard_link(&seed_path, root.join("key-copy")).unwrap();
    assert!(build_package(manifest(), &root, "test-key", &key()).is_err());
    fs::remove_file(root.join("key-copy")).unwrap();
    fs::write(root.join("key-copy"), key().to_bytes()).unwrap();
    assert!(build_package(manifest(), &root, "test-key", &key()).is_err());
}

#[test]
fn atomic_output_refuses_overwrite_and_leaves_no_partial_temporary_file() {
    let (temporary, root) = source();
    let archive = build(&root);
    let parent = temporary.path().canonicalize().unwrap();
    let destination = parent.join("monitor.idplugin");
    write_package_atomic(&destination, &archive).unwrap();
    assert_eq!(fs::read(&destination).unwrap(), archive);
    assert!(write_package_atomic(&destination, b"replacement").is_err());
    assert_eq!(fs::read(&destination).unwrap(), archive);
    assert_eq!(fs::read_dir(parent).unwrap().count(), 2);
    assert!(write_package_atomic(&destination, &[]).is_err());
}

#[cfg(unix)]
#[test]
fn symlinks_in_files_directories_and_source_root_ancestors_are_rejected() {
    use std::os::unix::fs::symlink;
    let (temporary, root) = source();
    let parent = temporary.path().canonicalize().unwrap();
    let outside = parent.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("worker"), b"outside contents").unwrap();
    symlink(outside.join("worker"), root.join("linked-file")).unwrap();
    assert!(build_package(manifest(), &root, "test-key", &key()).is_err());
    fs::remove_file(root.join("linked-file")).unwrap();
    symlink(&outside, root.join("linked-directory")).unwrap();
    assert!(build_package(manifest(), &root, "test-key", &key()).is_err());
    fs::remove_file(root.join("linked-directory")).unwrap();
    symlink(&root, parent.join("root-link")).unwrap();
    assert!(build_package(manifest(), &parent.join("root-link"), "test-key", &key()).is_err());
    symlink(&parent, parent.join("parent-link")).unwrap();
    assert!(build_package(
        manifest(),
        &parent.join("parent-link/payload"),
        "test-key",
        &key()
    )
    .is_err());
    symlink(
        parent.join("output-target"),
        parent.join("monitor.idplugin"),
    )
    .unwrap();
    assert!(write_package_atomic(&parent.join("monitor.idplugin"), &build(&root)).is_err());
    assert!(!parent.join("output-target").exists());
}

#[cfg(unix)]
#[test]
fn special_source_files_are_rejected_without_opening_them() {
    use std::os::unix::net::UnixListener;
    let (_temporary, root) = source();
    let _socket = UnixListener::bind(root.join("socket")).unwrap();
    assert!(build_package(manifest(), &root, "test-key", &key()).is_err());
}

#[cfg(target_os = "linux")]
#[test]
fn case_colliding_directory_components_are_rejected() {
    let (_temporary, root) = source();
    fs::create_dir(root.join("Data")).unwrap();
    fs::create_dir(root.join("data")).unwrap();
    fs::write(root.join("Data/a"), b"a").unwrap();
    fs::write(root.join("data/b"), b"b").unwrap();
    assert!(build_package(manifest(), &root, "test-key", &key()).is_err());
}

#[cfg(windows)]
#[test]
fn junctions_in_source_root_ancestors_are_rejected() {
    let (temporary, root) = source();
    let parent = temporary.path().canonicalize().unwrap();
    let junction = parent.join("junction");
    let status = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&junction)
        .arg(&root)
        .output()
        .unwrap();
    assert!(status.status.success(), "cannot create test junction");
    assert!(build_package(manifest(), &junction, "test-key", &key()).is_err());
    fs::remove_dir(junction).unwrap();
}
