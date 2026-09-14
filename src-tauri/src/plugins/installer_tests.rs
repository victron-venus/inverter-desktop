use super::super::package::PublisherTrust;
use super::super::packaging::build_package;
use super::super::protocol::{PluginManifest, PluginPermission};
use super::*;
use ed25519_dalek::SigningKey;
use serde_json::json;
use std::collections::HashMap;
use std::sync::OnceLock;

const PLUGIN: &str = "test.installer";
const KEY_ID: &str = "installer-fixture-only";

struct TestDirectory(PathBuf);
impl TestDirectory {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("inverter-package-test-{}", uuid::Uuid::new_v4()));
        create_private_directory(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
}
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = remove_owned_tree(&self.0);
    }
}

fn host_target() -> String {
    let arch = std::env::consts::ARCH;
    let suffix = match std::env::consts::OS {
        "macos" => "apple-darwin",
        "linux" => "unknown-linux-gnu",
        "windows" => "pc-windows-msvc",
        other => panic!("unsupported fixture host: {other}"),
    };
    format!("{arch}-{suffix}")
}

fn key() -> SigningKey {
    SigningKey::from_bytes(&[97_u8; 32])
}

fn trust() -> TrustStore {
    TrustStore::new(vec![PublisherTrust::new(
        KEY_ID.into(),
        key().verifying_key().to_bytes(),
        vec![PLUGIN.into()],
    )
    .unwrap()])
    .unwrap()
}

fn fixture(mode: &str) -> PathBuf {
    static FIXTURES: OnceLock<Mutex<HashMap<String, PathBuf>>> = OnceLock::new();
    let mut fixtures = FIXTURES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap();
    if let Some(path) = fixtures.get(mode) {
        return path.clone();
    }
    let directory = std::env::temp_dir().join(format!(
        "inverter-installer-fixture-{}-{mode}",
        std::process::id()
    ));
    fs::create_dir_all(&directory).unwrap();
    let executable = directory.join(format!("worker{}", std::env::consts::EXE_SUFFIX));
    let manifest_source =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/plugin_worker.rs");
    let source_path = if manifest_source.exists() {
        manifest_source
    } else {
        Path::new(file!())
            .ancestors()
            .nth(3)
            .unwrap()
            .join("tests/fixtures/plugin_worker.rs")
    };
    let source = fs::read_to_string(source_path).unwrap().replacen(
        "\"normal\".into()",
        &format!("\"{mode}\".into()"),
        1,
    );
    let source_file = directory.join("worker.rs");
    fs::write(&source_file, source).unwrap();
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let result = std::process::Command::new(rustc)
        .arg("--edition=2021")
        .arg(source_file)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    fixtures.insert(mode.into(), executable.clone());
    executable
}

fn package(directory: &Path, version: &str, mode: &str) -> PathBuf {
    let payload = directory.join(format!("source-{}", uuid::Uuid::new_v4()));
    create_private_directory(&payload).unwrap();
    let entrypoint = format!("worker{}", std::env::consts::EXE_SUFFIX);
    fs::copy(fixture(mode), payload.join(&entrypoint)).unwrap();
    let manifest = PluginManifest {
        schema_version: 1,
        plugin_id: PLUGIN.into(),
        version: version.into(),
        host_api: "^1.0".into(),
        target: host_target(),
        entrypoint,
        config_schema: json!({"type":"object","properties":{}}),
        permissions: vec![PluginPermission::DashboardContributions],
        inventory: Vec::new(),
        signature: None,
    };
    let bytes = build_package(manifest, &payload, KEY_ID, &key()).unwrap();
    let archive = directory.join(format!("archive-{}.idplugin", uuid::Uuid::new_v4()));
    fs::write(&archive, bytes).unwrap();
    archive
}

async fn manager(directory: &TestDirectory) -> (PackageManager, PluginHost) {
    let host = PluginHost::default();
    let manager = PackageManager::open(
        directory.0.join("store"),
        host_target(),
        trust(),
        host.clone(),
    )
    .await
    .unwrap();
    (manager, host)
}

async fn wait_running(host: &PluginHost) {
    time::timeout(Duration::from_secs(5), async {
        loop {
            if host.snapshots().iter().any(|snapshot| {
                snapshot.plugin_id == PLUGIN
                    && snapshot.state == WorkerState::Running
                    && !snapshot.contributions.is_empty()
            }) {
                break;
            }
            time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn signed_install_update_rollback_disable_enable_and_remove_use_real_worker() {
    let directory = TestDirectory::new();
    let (manager, host) = manager(&directory).await;
    let first = manager
        .install(package(&directory.0, "1.0.0", "normal"), true)
        .await
        .unwrap();
    assert!(first.enabled);
    wait_running(&host).await;
    assert_eq!(
        host.action(PLUGIN, "echo", json!({}), Duration::from_secs(2))
            .await
            .unwrap()["ok"],
        true
    );
    let second = manager
        .install(package(&directory.0, "1.1.0", "normal"), true)
        .await
        .unwrap();
    assert_eq!(second.rollback.as_ref(), Some(&first.active));
    assert_eq!(manager.rollback(PLUGIN).await.unwrap().active, first.active);
    assert_eq!(
        manager.rollback(PLUGIN).await.unwrap().active,
        second.active
    );
    let disabled = manager.disable(PLUGIN).await.unwrap();
    assert!(!disabled.enabled);
    assert!(host.snapshots().is_empty());
    manager.enable(PLUGIN).await.unwrap();
    wait_running(&host).await;
    let third = manager
        .install(package(&directory.0, "1.2.0", "normal"), true)
        .await
        .unwrap();
    assert_eq!(third.rollback, Some(second.active));
    assert_eq!(
        fs::read_dir(manager.0.root.join("content").join(PLUGIN))
            .unwrap()
            .count(),
        2
    );
    assert!(!manager.version_path(PLUGIN, &first.active).exists());
    manager.remove(PLUGIN).await.unwrap();
    assert!(manager.list().await.unwrap().is_empty());
    assert!(host.snapshots().is_empty());
    assert!(!manager.0.root.join("content").join(PLUGIN).exists());
    manager.close().await.unwrap();
}

#[tokio::test]
async fn failed_signature_or_handshake_preserves_previous_active_version_and_worker() {
    let directory = TestDirectory::new();
    let (manager, host) = manager(&directory).await;
    let first = manager
        .install(package(&directory.0, "1.0.0", "normal"), true)
        .await
        .unwrap();
    let corrupt = package(&directory.0, "1.1.0", "normal");
    let mut bytes = fs::read(&corrupt).unwrap();
    bytes[90] ^= 1;
    fs::write(&corrupt, bytes).unwrap();
    assert!(manager.install(corrupt, true).await.is_err());
    assert_eq!(manager.list().await.unwrap(), vec![first.clone()]);
    assert!(manager
        .install(package(&directory.0, "1.1.0", "bad_identity"), true)
        .await
        .is_err());
    assert_eq!(manager.list().await.unwrap(), vec![first]);
    wait_running(&host).await;
    assert_eq!(
        host.action(PLUGIN, "echo", json!({}), Duration::from_secs(2))
            .await
            .unwrap()["ok"],
        true
    );
    manager.close().await.unwrap();
}

#[tokio::test]
async fn reopened_enabled_inventory_never_autoexecutes_and_rechecks_payload_bytes() {
    let directory = TestDirectory::new();
    let (manager, host) = manager(&directory).await;
    let first = manager
        .install(package(&directory.0, "1.0.0", "normal"), true)
        .await
        .unwrap();
    manager.close().await.unwrap();
    assert!(host.snapshots().is_empty());
    let reopened = PackageManager::open(
        directory.0.join("store"),
        host_target(),
        trust(),
        host.clone(),
    )
    .await
    .unwrap();
    assert!(reopened.list().await.unwrap()[0].enabled);
    assert!(host.snapshots().is_empty());
    let archive = reopened
        .version_path(PLUGIN, &first.active)
        .join("archive.idplugin");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&archive, fs::Permissions::from_mode(0o600)).unwrap();
    }
    fs::write(&archive, b"corrupt").unwrap();
    assert!(reopened.enable(PLUGIN).await.is_err());
    assert!(host.snapshots().is_empty());
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn extracted_payload_and_extra_inventory_are_checked_before_launch() {
    let directory = TestDirectory::new();
    let (manager, host) = manager(&directory).await;
    let record = manager
        .install(package(&directory.0, "1.0.0", "normal"), false)
        .await
        .unwrap();
    let payload = manager.version_path(PLUGIN, &record.active).join("payload");
    let executable = payload.join(format!("worker{}", std::env::consts::EXE_SUFFIX));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    }
    fs::write(&executable, b"changed executable").unwrap();
    assert!(manager.enable(PLUGIN).await.is_err());
    assert!(host.snapshots().is_empty());
    manager.close().await.unwrap();
}

#[tokio::test]
async fn store_lease_rejects_other_instances_and_closed_manager_cannot_operate() {
    let directory = TestDirectory::new();
    let (manager, _) = manager(&directory).await;
    assert!(PackageManager::open(
        directory.0.join("store"),
        host_target(),
        trust(),
        PluginHost::default()
    )
    .await
    .is_err());
    manager.close().await.unwrap();
    assert!(manager.list().await.is_err());
    let second = PackageManager::open(
        directory.0.join("store"),
        host_target(),
        trust(),
        PluginHost::default(),
    )
    .await
    .unwrap();
    second.close().await.unwrap();
}

#[tokio::test]
async fn interrupted_staging_and_unreferenced_versions_are_recovered_without_starting() {
    let directory = TestDirectory::new();
    let (manager, host) = manager(&directory).await;
    let first = manager
        .install(package(&directory.0, "1.0.0", "normal"), false)
        .await
        .unwrap();
    let staging = manager
        .0
        .root
        .join("staging")
        .join(format!("stage-{}", uuid::Uuid::new_v4()));
    create_private_directory(&staging).unwrap();
    fs::write(staging.join("partial"), b"interrupted extraction").unwrap();
    let pending = manager
        .0
        .root
        .join(format!("state-pending-{}", uuid::Uuid::new_v4()));
    fs::write(&pending, b"incomplete inventory").unwrap();
    let orphan = manager
        .0
        .root
        .join("content")
        .join(PLUGIN)
        .join("b".repeat(64));
    create_private_directory(&orphan).unwrap();
    fs::write(orphan.join("partial"), b"uncommitted package").unwrap();
    manager.close().await.unwrap();
    let reopened = PackageManager::open(
        directory.0.join("store"),
        host_target(),
        trust(),
        host.clone(),
    )
    .await
    .unwrap();
    assert_eq!(reopened.list().await.unwrap(), vec![first]);
    assert!(!staging.exists());
    assert!(!pending.exists());
    assert!(!orphan.exists());
    assert!(host.snapshots().is_empty());
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn revocation_during_handshake_cannot_activate_under_a_later_login() {
    let directory = TestDirectory::new();
    let (manager, host) = manager(&directory).await;
    let archive = package(&directory.0, "1.0.0", "no_hello");
    let installer = manager.clone();
    let installing = tokio::spawn(async move { installer.install(archive, true).await });
    time::timeout(Duration::from_secs(5), async {
        loop {
            if host
                .snapshots()
                .iter()
                .any(|snapshot| snapshot.plugin_id == PLUGIN)
            {
                break;
            }
            time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    host.revoke();
    host.resume();
    assert!(installing.await.unwrap().is_err());
    assert!(manager.list().await.unwrap().is_empty());
    assert!(host.snapshots().is_empty());
    manager.close().await.unwrap();
}

#[tokio::test]
async fn cancelled_caller_does_not_interrupt_transaction_or_drop_store_lease() {
    let directory = TestDirectory::new();
    let (manager, host) = manager(&directory).await;
    let archive = package(&directory.0, "1.0.0", "no_hello");
    let installer = manager.clone();
    let installing = tokio::spawn(async move { installer.install(archive, true).await });
    time::timeout(Duration::from_secs(5), async {
        while host.snapshots().is_empty() {
            time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    installing.abort();
    let _ = installing.await;
    // list waits for the detached transaction to finish restoring inventory.
    assert!(manager.list().await.unwrap().is_empty());
    assert!(host.snapshots().is_empty());
    manager.close().await.unwrap();
}

#[tokio::test]
async fn rejects_mobile_targets_unowned_directories_and_unsigned_or_unscoped_keys() {
    let directory = TestDirectory::new();
    assert!(PackageManager::open(
        directory.0.join("mobile"),
        "aarch64-linux-android".into(),
        trust(),
        PluginHost::default()
    )
    .await
    .is_err());
    let unrelated = directory.0.join("unrelated");
    create_private_directory(&unrelated).unwrap();
    fs::write(unrelated.join("keep"), b"private user content").unwrap();
    assert!(PackageManager::open(
        unrelated.clone(),
        host_target(),
        trust(),
        PluginHost::default()
    )
    .await
    .is_err());
    assert_eq!(
        fs::read(unrelated.join("keep")).unwrap(),
        b"private user content"
    );
    let manager = PackageManager::open(
        directory.0.join("empty-trust"),
        host_target(),
        TrustStore::default(),
        PluginHost::default(),
    )
    .await
    .unwrap();
    assert!(manager
        .install(package(&directory.0, "1.0.0", "normal"), false)
        .await
        .is_err());
    assert!(manager.list().await.unwrap().is_empty());
    manager.close().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn symlinked_store_paths_are_rejected_without_deleting_external_files() {
    use std::os::unix::fs::symlink;
    let directory = TestDirectory::new();
    let (manager, _) = manager(&directory).await;
    let record = manager
        .install(package(&directory.0, "1.0.0", "normal"), false)
        .await
        .unwrap();
    let external = directory.0.join("external");
    create_private_directory(&external).unwrap();
    fs::write(external.join("keep"), b"not package data").unwrap();
    let package_path = manager.version_path(PLUGIN, &record.active);
    remove_owned_tree(&package_path).unwrap();
    symlink(&external, &package_path).unwrap();
    assert!(manager.enable(PLUGIN).await.is_err());
    assert!(manager.remove(PLUGIN).await.is_err());
    assert_eq!(
        fs::read(external.join("keep")).unwrap(),
        b"not package data"
    );
    fs::remove_file(&package_path).unwrap();
    manager.close().await.unwrap();
}

#[tokio::test]
async fn initialization_resumes_after_each_owned_empty_creation_step() {
    for step in 0..4 {
        let directory = TestDirectory::new();
        let root = directory.0.join("store");
        create_private_directory(&root).unwrap();
        fs::write(root.join("store.lock"), []).unwrap();
        if step >= 1 {
            create_private_directory(&root.join(STORE_SENTINEL)).unwrap();
        }
        if step >= 2 {
            create_private_directory(&root.join("content")).unwrap();
        }
        if step >= 3 {
            create_private_directory(&root.join("staging")).unwrap();
        }
        let manager = PackageManager::open(root, host_target(), trust(), PluginHost::default())
            .await
            .unwrap();
        assert!(manager.list().await.unwrap().is_empty());
        manager.close().await.unwrap();
    }
}

#[tokio::test]
async fn missing_inventory_never_erases_existing_package_content() {
    let directory = TestDirectory::new();
    let (manager, _) = manager(&directory).await;
    let record = manager
        .install(package(&directory.0, "1.0.0", "normal"), false)
        .await
        .unwrap();
    let archive = manager
        .version_path(PLUGIN, &record.active)
        .join("archive.idplugin");
    manager.close().await.unwrap();
    fs::remove_file(directory.0.join("store/state.json")).unwrap();
    assert!(PackageManager::open(
        directory.0.join("store"),
        host_target(),
        trust(),
        PluginHost::default()
    )
    .await
    .is_err());
    assert!(archive.exists());
}

#[tokio::test]
async fn queued_lifecycle_operations_cannot_cross_logout_and_relogin() {
    let directory = TestDirectory::new();
    let (manager, host) = manager(&directory).await;
    let original = manager
        .install(package(&directory.0, "1.0.0", "normal"), false)
        .await
        .unwrap();
    for remove in [false, true] {
        let operation = manager.0.operation.lock().await;
        let caller = manager.clone();
        let pending = tokio::spawn(async move {
            if remove {
                caller.remove(PLUGIN).await
            } else {
                caller.disable(PLUGIN).await.map(|_| ())
            }
        });
        // Poll the public method so it captures the current epoch and then waits
        // behind the deliberately held transaction lock.
        tokio::task::yield_now().await;
        host.revoke();
        host.resume();
        drop(operation);
        assert!(pending.await.unwrap().is_err());
        assert_eq!(manager.list().await.unwrap(), vec![original.clone()]);
    }
    manager.close().await.unwrap();
}

#[test]
fn store_lock_probe() {
    let Some(path) = std::env::var_os("INVERTER_INSTALLER_LOCK_PROBE") else {
        return;
    };
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap();
    assert!(
        file.try_lock().is_err(),
        "a separate process must not obtain the live store lease"
    );
}

#[tokio::test]
async fn store_lease_is_exclusive_across_real_processes() {
    let directory = TestDirectory::new();
    let (manager, _) = manager(&directory).await;
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("store_lock_probe")
        .arg("--nocapture")
        .env(
            "INVERTER_INSTALLER_LOCK_PROBE",
            manager.0.root.join("store.lock"),
        )
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    manager.close().await.unwrap();
}

#[tokio::test]
async fn dropping_manager_keeps_lease_until_its_worker_is_reaped() {
    let directory = TestDirectory::new();
    let (manager, host) = manager(&directory).await;
    manager
        .install(package(&directory.0, "1.0.0", "stalled"), true)
        .await
        .unwrap();
    drop(manager);
    assert!(PackageManager::open(
        directory.0.join("store"),
        host_target(),
        trust(),
        host.clone()
    )
    .await
    .is_err());
    let reopened = time::timeout(Duration::from_secs(4), async {
        loop {
            match PackageManager::open(
                directory.0.join("store"),
                host_target(),
                trust(),
                host.clone(),
            )
            .await
            {
                Ok(manager) => break manager,
                Err(_) => time::sleep(Duration::from_millis(10)).await,
            }
        }
    })
    .await
    .unwrap();
    assert!(host.snapshots().is_empty());
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn recovery_never_collects_content_still_owned_by_a_worker() {
    let directory = TestDirectory::new();
    let (manager, host) = manager(&directory).await;
    let record = manager
        .install(package(&directory.0, "1.0.0", "normal"), true)
        .await
        .unwrap();
    let active = manager.version_path(PLUGIN, &record.active);
    // An interrupted candidate has the same invariant: its worker is owned,
    // while the committed inventory does not yet reference its package bytes.
    assert!(manager.recover(&StoreState::default()).is_err());
    assert!(active.exists());
    wait_running(&host).await;
    manager.close().await.unwrap();
}

#[tokio::test]
async fn update_refuses_to_retain_corrupt_previous_active_as_rollback() {
    let directory = TestDirectory::new();
    let (manager, host) = manager(&directory).await;
    let original = manager
        .install(package(&directory.0, "1.0.0", "normal"), true)
        .await
        .unwrap();
    wait_running(&host).await;
    let generation = host.snapshots()[0].generation;
    let archive = manager
        .version_path(PLUGIN, &original.active)
        .join("archive.idplugin");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&archive, fs::Permissions::from_mode(0o600)).unwrap();
    }
    fs::write(archive, b"corrupt retained archive").unwrap();
    assert!(manager
        .install(package(&directory.0, "1.1.0", "normal"), true)
        .await
        .is_err());
    assert_eq!(manager.list().await.unwrap(), vec![original]);
    assert_eq!(host.snapshots()[0].generation, generation);
    assert_eq!(
        host.action(PLUGIN, "echo", json!({}), Duration::from_secs(2))
            .await
            .unwrap()["ok"],
        true
    );
    manager.close().await.unwrap();
}

#[tokio::test]
async fn entry_quota_refuses_install_before_staging_or_inventory_changes() {
    let directory = TestDirectory::new();
    let (manager, host) = manager(&directory).await;
    let archive = package(&directory.0, "1.0.0", "normal");
    let filler = manager.0.root.join("quota-fixture");
    create_private_directory(&filler).unwrap();
    let (_, initial_count) = tree_usage(&manager.0.root).unwrap();
    for index in initial_count..MAX_STORE_FILES - 2 {
        File::create(filler.join(format!("entry-{index}"))).unwrap();
    }
    assert_eq!(tree_usage(&manager.0.root).unwrap().1, MAX_STORE_FILES - 2);
    let inventory = fs::read(manager.0.root.join("state.json")).unwrap();
    let error = manager.install(archive, false).await.unwrap_err();
    assert!(error.contains("file quota"), "{error}");
    assert_eq!(
        fs::read(manager.0.root.join("state.json")).unwrap(),
        inventory
    );
    assert!(manager.list().await.unwrap().is_empty());
    assert!(fs::read_dir(manager.0.root.join("staging"))
        .unwrap()
        .next()
        .is_none());
    assert!(fs::read_dir(manager.0.root.join("content"))
        .unwrap()
        .next()
        .is_none());
    assert!(host.snapshots().is_empty());
    manager.close().await.unwrap();
    let reopened = PackageManager::open(directory.0.join("store"), host_target(), trust(), host)
        .await
        .unwrap();
    reopened.close().await.unwrap();
    remove_owned_tree(&filler).unwrap();
}

#[tokio::test]
async fn signed_deep_inventory_cannot_create_excessive_implicit_directories() {
    use super::super::package::{canonical_manifest_bytes, manifest_signing_payload};
    use super::super::protocol::{InventoryEntry, SignatureMetadata};
    use ed25519_dalek::Signer;
    use sha2::{Digest, Sha256};
    use std::io::Cursor;
    use zip::write::SimpleFileOptions;

    let directory = TestDirectory::new();
    let (manager, _) = manager(&directory).await;
    let inventory: Vec<_> = (0..128)
        .map(|index| InventoryEntry {
            path: format!("p{index:03}/{}payload", "d/".repeat(65)),
            size: 1,
            sha256: format!("{:x}", Sha256::digest(b"x")),
        })
        .collect();
    let mut manifest = PluginManifest {
        schema_version: 1,
        plugin_id: PLUGIN.into(),
        version: "1.0.0".into(),
        host_api: "^1.0".into(),
        target: host_target(),
        entrypoint: inventory[0].path.clone(),
        config_schema: json!({"type":"object"}),
        permissions: Vec::new(),
        inventory,
        signature: None,
    };
    let signature = key().sign(&manifest_signing_payload(&manifest).unwrap());
    manifest.signature = Some(SignatureMetadata {
        algorithm: "ed25519".into(),
        key_id: KEY_ID.into(),
        signature: signature
            .to_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    });
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .unix_permissions(0o755);
    zip.start_file("manifest.json", options).unwrap();
    zip.write_all(&canonical_manifest_bytes(&manifest).unwrap())
        .unwrap();
    for item in &manifest.inventory {
        zip.start_file(&item.path, options).unwrap();
        zip.write_all(b"x").unwrap();
    }
    let bytes = zip.finish().unwrap().into_inner();
    let verified = verify_archive_bytes(bytes.clone(), &trust(), &host_target()).unwrap();
    assert!(projected_package_entries(&verified).is_err());
    let archive = directory.0.join("deep.idplugin");
    fs::write(&archive, bytes).unwrap();
    let error = manager.install(archive, false).await.unwrap_err();
    assert!(error.contains("file quota"), "{error}");
    assert!(manager.list().await.unwrap().is_empty());
    assert!(fs::read_dir(manager.0.root.join("staging"))
        .unwrap()
        .next()
        .is_none());
    assert!(fs::read_dir(manager.0.root.join("content"))
        .unwrap()
        .next()
        .is_none());
    manager.close().await.unwrap();
}

#[tokio::test]
async fn ownership_sentinel_is_atomic_directory_and_rejects_partial_files() {
    let directory = TestDirectory::new();
    let (manager, _) = manager(&directory).await;
    let sentinel = manager.0.root.join(STORE_SENTINEL);
    assert!(sentinel.is_dir());
    assert!(fs::read_dir(&sentinel).unwrap().next().is_none());
    manager.close().await.unwrap();
    fs::remove_dir(&sentinel).unwrap();
    fs::write(&sentinel, b"partial marker").unwrap();
    assert!(PackageManager::open(
        directory.0.join("store"),
        host_target(),
        trust(),
        PluginHost::default()
    )
    .await
    .is_err());
    assert_eq!(fs::read(&sentinel).unwrap(), b"partial marker");
}
