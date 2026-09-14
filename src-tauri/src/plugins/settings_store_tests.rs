use super::*;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};

const PLUGIN: &str = "test.settings";
const SECRET: &str = "private-token-must-not-appear-in-files-or-errors";

fn fixture() -> (tempfile::TempDir, SettingsStore) {
    let directory = tempfile::tempdir().unwrap();
    let package_root = directory.path().join("package-store");
    let builder = fs::DirBuilder::new();
    #[cfg(unix)]
    let builder = {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = builder;
        builder.mode(0o700);
        builder
    };
    // Mirror the package manager's private store, independently of tempfile's
    // parent permissions and the user's process umask.
    builder.create(&package_root).unwrap();
    let key: [u8; 32] = rand::rng().random();
    let store = SettingsStore::new(
        package_root.canonicalize().unwrap(),
        Arc::new(move || Ok(key.to_vec())),
    );
    (directory, store)
}

fn data() -> SettingsData {
    SettingsData {
        revision: uuid::Uuid::new_v4().to_string(),
        values: BTreeMap::from([
            ("host".into(), json!("broker.example")),
            (
                "unknown_future_field".into(),
                json!({"keep": [1, true, null]}),
            ),
        ]),
        secrets: BTreeMap::from([("token".into(), SECRET.into())]),
        secret_fields: BTreeSet::from(["token".into(), "removed_secret".into()]),
    }
}

fn save(store: &SettingsStore, plugin_id: &str, data: &SettingsData) {
    store
        .prepare_write(plugin_id, data)
        .unwrap()
        .commit()
        .unwrap();
}

fn private_file(path: &Path, bytes: &[u8]) {
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).unwrap().write_all(bytes).unwrap();
}

#[test]
fn missing_reads_and_recovery_do_not_create_directories_or_request_a_key() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("not-created");
    let calls = Arc::new(AtomicUsize::new(0));
    let store = SettingsStore::new(
        root.clone(),
        Arc::new({
            let calls = calls.clone();
            move || {
                calls.fetch_add(1, Ordering::AcqRel);
                Err("should never request a key".into())
            }
        }),
    );
    assert!(!root.exists());
    assert!(store.read(PLUGIN).unwrap() == SettingsData::default());
    let inventory = store.inventory().unwrap();
    assert!(inventory.records.is_empty());
    assert_eq!(inventory.total_bytes, 0);
    assert_eq!(inventory.max_records, 64);
    assert_eq!(inventory.max_bytes, 8 * 1024 * 1024);
    let record_id = SettingsStore::record_id(PLUGIN).unwrap();
    let revision = format!("{:x}", Sha256::digest([]));
    assert!(store.remove_record(&record_id, &revision).is_err());
    store.recover().unwrap();
    store.remove(PLUGIN).unwrap();
    assert!(!root.exists());
    assert_eq!(calls.load(Ordering::Acquire), 0);
}

#[test]
fn encrypted_round_trip_preserves_unknown_values_and_ever_secret_fields() {
    let (_directory, store) = fixture();
    let original = data();
    save(&store, PLUGIN, &original);
    let encrypted = fs::read(store.record_path(PLUGIN).unwrap()).unwrap();
    for value in [
        SECRET,
        "broker.example",
        &original.revision,
        "removed_secret",
    ] {
        assert!(!encrypted
            .windows(value.len())
            .any(|bytes| bytes == value.as_bytes()));
    }
    assert!(encrypted.starts_with(MAGIC));
    assert!(store.read(PLUGIN).unwrap() == original);
    let reopened = SettingsStore::new(store.root.clone(), store.key_provider.clone());
    reopened.recover().unwrap();
    assert!(reopened.read(PLUGIN).unwrap() == original);
}

#[test]
fn changed_writes_preserve_supplied_revision_and_use_fresh_nonces() {
    let (_directory, store) = fixture();
    let mut original = data();
    save(&store, PLUGIN, &original);
    let path = store.record_path(PLUGIN).unwrap();
    let first = fs::read(&path).unwrap();
    save(&store, PLUGIN, &original);
    let second = fs::read(&path).unwrap();
    assert_ne!(first, second);
    assert_eq!(store.read(PLUGIN).unwrap().revision, original.revision);
    original.revision = uuid::Uuid::new_v4().to_string();
    original.values.insert("host".into(), json!("new.example"));
    save(&store, PLUGIN, &original);
    assert!(store.read(PLUGIN).unwrap() == original);
}

#[test]
fn wrong_key_and_cross_plugin_ciphertext_substitution_are_rejected() {
    let (_directory, store) = fixture();
    save(&store, PLUGIN, &data());
    let mut wrong_key = store.key().unwrap();
    wrong_key[0] ^= 1;
    let wrong = SettingsStore::new(store.root.clone(), Arc::new(move || Ok(wrong_key.clone())));
    assert!(wrong.read(PLUGIN).is_err());
    let other = "other.settings";
    let bytes = fs::read(store.record_path(PLUGIN).unwrap()).unwrap();
    private_file(&store.record_path(other).unwrap(), &bytes);
    assert!(store.read(other).is_err());
    assert!(store.read(PLUGIN).is_ok());
}

#[test]
fn corrupted_headers_nonces_ciphertexts_and_tags_never_fall_back_to_defaults() {
    let (_directory, store) = fixture();
    save(&store, PLUGIN, &data());
    let path = store.record_path(PLUGIN).unwrap();
    let original = fs::read(&path).unwrap();
    for position in [
        0,
        MAGIC.len(),
        MAGIC.len() + NONCE_BYTES,
        original.len() - 1,
    ] {
        let mut changed = original.clone();
        changed[position] ^= 0x40;
        fs::write(&path, changed).unwrap();
        assert!(store.read(PLUGIN).is_err(), "tampering offset {position}");
    }
    for length in [0, MAGIC.len(), MAGIC.len() + NONCE_BYTES + TAG_BYTES - 1] {
        fs::write(&path, &original[..length]).unwrap();
        assert!(store.read(PLUGIN).is_err(), "truncated length {length}");
    }
}

#[test]
fn malformed_authenticated_plaintext_never_echoes_secret_values_in_errors() {
    let (_directory, store) = fixture();
    store.ensure_directory().unwrap();
    let invalid =
        format!(r#"{{"revision":"0","values":{{}},"secrets":"{SECRET}","secret_fields":[]}}"#);
    private_file(
        &store.record_path(PLUGIN).unwrap(),
        &encrypt(PLUGIN, invalid.as_bytes(), &store.key().unwrap()).unwrap(),
    );
    let error = match store.read(PLUGIN) {
        Ok(_) => panic!("malformed settings must fail"),
        Err(error) => error,
    };
    assert_eq!(error, "Invalid decrypted plugin settings");
    assert!(!error.contains(SECRET));
}

#[test]
fn invalid_secret_classification_and_revisions_are_rejected_before_staging() {
    let (_directory, store) = fixture();
    let mut invalid = data();
    invalid
        .values
        .insert("removed_secret".into(), json!(SECRET));
    assert!(store.prepare_write(PLUGIN, &invalid).is_err());
    invalid = data();
    invalid.secret_fields.remove("token");
    assert!(store.prepare_write(PLUGIN, &invalid).is_err());
    invalid = data();
    invalid.revision = SECRET.into();
    assert!(store.prepare_write(PLUGIN, &invalid).is_err());
    assert!(!store.directory.exists());
}

#[test]
fn oversized_plaintext_ciphertext_and_decrypted_content_are_bounded() {
    let (_directory, store) = fixture();
    let mut oversized = data();
    oversized
        .secrets
        .insert("token".into(), "s".repeat(MAX_SETTINGS_PLAINTEXT_BYTES));
    assert!(store.prepare_write(PLUGIN, &oversized).is_err());
    assert!(!store.directory.exists());
    store.ensure_directory().unwrap();
    let path = store.record_path(PLUGIN).unwrap();
    private_file(&path, &vec![0; MAX_SETTINGS_FILE_BYTES + 1]);
    assert!(store.read(PLUGIN).is_err());
    let large = encrypt(
        PLUGIN,
        &vec![b' '; MAX_SETTINGS_PLAINTEXT_BYTES + 1],
        &store.key().unwrap(),
    )
    .unwrap();
    fs::write(&path, large).unwrap();
    assert!(store.read(PLUGIN).is_err());
}

#[test]
fn failed_key_access_leaks_no_provider_error_and_preserves_previous_bytes() {
    let (_directory, store) = fixture();
    save(&store, PLUGIN, &data());
    let path = store.record_path(PLUGIN).unwrap();
    let before = fs::read(&path).unwrap();
    let unavailable = SettingsStore::new(store.root.clone(), Arc::new(|| Err(SECRET.into())));
    let error = match unavailable.prepare_write(PLUGIN, &data()) {
        Ok(_) => panic!("unavailable key must fail"),
        Err(error) => error,
    };
    assert_eq!(error, "Plugin settings key is unavailable");
    assert_eq!(fs::read(path).unwrap(), before);
    assert!(store.scan().unwrap().pending.is_empty());
    let mut short_key = store.key().unwrap();
    short_key.truncate(31);
    let short = SettingsStore::new(store.root.clone(), Arc::new(move || Ok(short_key.clone())));
    assert!(short.read(PLUGIN).is_err());
}

#[test]
fn dropping_prepared_write_removes_only_its_temp_and_preserves_committed_data() {
    let (_directory, store) = fixture();
    let original = data();
    save(&store, PLUGIN, &original);
    let before = fs::read(store.record_path(PLUGIN).unwrap()).unwrap();
    let prepared = store.prepare_write(PLUGIN, &data()).unwrap();
    let pending = prepared.pending.clone().unwrap();
    assert!(pending.exists());
    // Ordinary reads must never mistake a live prepared write for stale recovery data.
    assert!(store.read(PLUGIN).unwrap() == original);
    assert!(pending.exists());
    drop(prepared);
    assert!(!pending.exists());
    assert_eq!(
        fs::read(store.record_path(PLUGIN).unwrap()).unwrap(),
        before
    );
}

#[test]
fn altered_pending_data_cannot_replace_previous_settings() {
    let (_directory, store) = fixture();
    save(&store, PLUGIN, &data());
    let path = store.record_path(PLUGIN).unwrap();
    let before = fs::read(&path).unwrap();
    let prepared = store.prepare_write(PLUGIN, &data()).unwrap();
    let pending = prepared.pending.clone().unwrap();
    fs::write(&pending, b"partial transaction").unwrap();
    assert!(prepared.commit().is_err());
    assert!(!pending.exists());
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn startup_recovers_only_valid_owned_pending_files_without_a_key() {
    let (_directory, store) = fixture();
    let original = data();
    save(&store, PLUGIN, &original);
    let before = fs::read(store.record_path(PLUGIN).unwrap()).unwrap();
    let pending = store
        .directory
        .join(format!("{PENDING_PREFIX}{}", uuid::Uuid::new_v4()));
    private_file(&pending, b"");
    let reopened = SettingsStore::new(
        store.root.clone(),
        Arc::new(|| panic!("recovery must not need a key")),
    );
    reopened.recover().unwrap();
    assert!(!pending.exists());
    assert_eq!(
        fs::read(store.record_path(PLUGIN).unwrap()).unwrap(),
        before
    );
    assert!(store.read(PLUGIN).unwrap() == original);
}

#[test]
fn unexpected_or_invalid_pending_files_abort_recovery_before_deleting_anything() {
    let (_directory, store) = fixture();
    store.ensure_directory().unwrap();
    let pending = store
        .directory
        .join(format!("{PENDING_PREFIX}{}", uuid::Uuid::new_v4()));
    private_file(&pending, b"partial");
    for name in ["unrelated.txt", "settings-pending-not-a-uuid"] {
        let unexpected = store.directory.join(name);
        private_file(&unexpected, b"keep");
        assert!(store.recover().is_err());
        assert!(pending.exists());
        assert_eq!(fs::read(&unexpected).unwrap(), b"keep");
        fs::remove_file(unexpected).unwrap();
    }
}

#[test]
fn retained_record_limit_allows_overwrite_but_rejects_new_identity() {
    let (_directory, store) = fixture();
    for index in 0..MAX_SETTINGS_RECORDS {
        save(&store, &format!("test.record-{index}"), &data());
    }
    assert!(store.prepare_write("test.excess", &data()).is_err());
    assert_eq!(store.scan().unwrap().records, MAX_SETTINGS_RECORDS);
    assert!(store.scan().unwrap().pending.is_empty());
    let replacement = data();
    save(&store, "test.record-0", &replacement);
    assert!(store.read("test.record-0").unwrap() == replacement);
    store.remove("test.record-1").unwrap();
    save(&store, "test.excess", &data());
    assert_eq!(store.scan().unwrap().records, MAX_SETTINGS_RECORDS);
}

#[test]
fn simultaneous_pending_files_are_bounded_and_drops_release_the_budget() {
    let (_directory, store) = fixture();
    save(&store, PLUGIN, &data());
    let pending: Vec<_> = (0..MAX_PENDING_FILES)
        .map(|_| store.prepare_write(PLUGIN, &data()).unwrap())
        .collect();
    assert!(store.prepare_write(PLUGIN, &data()).is_err());
    drop(pending);
    assert!(store.scan().unwrap().pending.is_empty());
    store
        .prepare_write(PLUGIN, &data())
        .unwrap()
        .commit()
        .unwrap();
}

#[test]
fn hashed_filenames_support_windows_reserved_ids_and_reject_path_input() {
    let (_directory, store) = fixture();
    let original = data();
    save(&store, "con.example", &original);
    let filename = store.record_path("con.example").unwrap();
    assert_eq!(filename.file_name().unwrap().len(), 68);
    assert_eq!(filename.extension().unwrap(), "enc");
    assert!(store.read("con.example").unwrap() == original);
    for id in ["../outside", "Test.invalid", "test/other", "test\\other"] {
        assert!(store.read(id).is_err());
        assert!(store.prepare_write(id, &original).is_err());
        assert!(store.remove(id).is_err());
    }
}

#[test]
fn explicit_removal_needs_no_key_and_preserves_other_namespaces() {
    let (_directory, store) = fixture();
    save(&store, PLUGIN, &data());
    let other = data();
    save(&store, "other.settings", &other);
    let unavailable = SettingsStore::new(
        store.root.clone(),
        Arc::new(|| panic!("delete must not need a key")),
    );
    unavailable.remove(PLUGIN).unwrap();
    unavailable.remove(PLUGIN).unwrap();
    assert!(unavailable.read(PLUGIN).unwrap() == SettingsData::default());
    assert!(store.read(PLUGIN).unwrap() == SettingsData::default());
    assert!(store.read("other.settings").unwrap() == other);
}

#[test]
fn linked_settings_files_are_rejected_without_touching_the_external_file() {
    let (_directory, store) = fixture();
    store.ensure_directory().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let external = outside.path().join("external");
    private_file(&external, b"external data");
    let path = store.record_path(PLUGIN).unwrap();
    fs::hard_link(&external, &path).unwrap();
    assert!(store.read(PLUGIN).is_err());
    assert!(store.prepare_write(PLUGIN, &data()).is_err());
    assert!(store.remove(PLUGIN).is_err());
    assert!(store.recover().is_err());
    assert_eq!(fs::read(&external).unwrap(), b"external data");
}

#[cfg(unix)]
#[test]
fn symlinked_directories_and_files_are_rejected_without_following_them() {
    use std::os::unix::fs::symlink;
    let (_directory, store) = fixture();
    let outside = tempfile::tempdir().unwrap();
    let external = outside.path().join("external");
    private_file(&external, b"external data");
    symlink(outside.path(), &store.directory).unwrap();
    assert!(store.read(PLUGIN).is_err());
    assert!(store.prepare_write(PLUGIN, &data()).is_err());
    assert!(store.recover().is_err());
    fs::remove_file(&store.directory).unwrap();
    store.ensure_directory().unwrap();
    let path = store.record_path(PLUGIN).unwrap();
    symlink(&external, &path).unwrap();
    assert!(store.read(PLUGIN).is_err());
    assert!(store.remove(PLUGIN).is_err());
    assert_eq!(fs::read(external).unwrap(), b"external data");
}

#[cfg(unix)]
#[test]
fn failed_commit_cleanup_never_follows_a_replaced_settings_directory() {
    use std::os::unix::fs::symlink;
    let (_directory, store) = fixture();
    let prepared = store.prepare_write(PLUGIN, &data()).unwrap();
    let filename = prepared
        .pending
        .as_ref()
        .unwrap()
        .file_name()
        .unwrap()
        .to_owned();
    let original = store.root.join("moved-settings");
    fs::rename(&store.directory, &original).unwrap();
    let outside = tempfile::tempdir().unwrap();
    let external = outside.path().join(&filename);
    private_file(&external, b"unrelated external file");
    symlink(outside.path(), &store.directory).unwrap();
    assert!(prepared.commit().is_err());
    assert_eq!(fs::read(external).unwrap(), b"unrelated external file");
    assert!(original.join(filename).exists());
}

#[cfg(unix)]
#[test]
fn settings_directories_and_files_are_private_and_nonprivate_inputs_fail() {
    use std::os::unix::fs::PermissionsExt;
    let (_directory, store) = fixture();
    save(&store, PLUGIN, &data());
    let path = store.record_path(PLUGIN).unwrap();
    assert_eq!(
        fs::metadata(&store.directory).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(store.read(PLUGIN).is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    fs::set_permissions(&store.directory, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(store.read(PLUGIN).is_err());
}

#[test]
fn inventory_is_sorted_redacted_and_read_only_without_a_key() {
    let (_directory, store) = fixture();
    save(&store, PLUGIN, &data());
    let encrypted = fs::read(store.record_path(PLUGIN).unwrap()).unwrap();
    let invalid = b"corrupt retained ciphertext";
    private_file(&store.record_path("unknown.owner").unwrap(), invalid);
    private_file(&store.record_path("empty.owner").unwrap(), b"");
    let prepared = store.prepare_write(PLUGIN, &data()).unwrap();
    let pending = prepared.pending.as_ref().unwrap();
    let pending_bytes = fs::read(pending).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let unavailable = SettingsStore::new(
        store.root.clone(),
        Arc::new({
            let calls = calls.clone();
            move || {
                calls.fetch_add(1, Ordering::AcqRel);
                Err(SECRET.into())
            }
        }),
    );
    let inventory = unavailable.inventory().unwrap();
    let mut expected = vec![
        SettingsRecord {
            record_id: SettingsStore::record_id(PLUGIN).unwrap(),
            revision: format!("{:x}", Sha256::digest(&encrypted)),
            bytes: encrypted.len() as u64,
        },
        SettingsRecord {
            record_id: SettingsStore::record_id("unknown.owner").unwrap(),
            revision: format!("{:x}", Sha256::digest(invalid)),
            bytes: invalid.len() as u64,
        },
        SettingsRecord {
            record_id: SettingsStore::record_id("empty.owner").unwrap(),
            revision: format!("{:x}", Sha256::digest([])),
            bytes: 0,
        },
    ];
    expected.sort_by(|left, right| left.record_id.cmp(&right.record_id));
    assert_eq!(inventory.records, expected);
    assert_eq!(
        inventory.total_bytes,
        (encrypted.len() + invalid.len() + pending_bytes.len()) as u64
    );
    let exposed = format!("{inventory:?}");
    for hidden in [SECRET, "broker.example", "unknown.owner", "removed_secret"] {
        assert!(!exposed.contains(hidden));
    }
    assert_eq!(unavailable.inventory().unwrap(), inventory);
    assert_eq!(fs::read(pending).unwrap(), pending_bytes);
    assert_eq!(
        fs::read(store.record_path(PLUGIN).unwrap()).unwrap(),
        encrypted
    );
    assert_eq!(calls.load(Ordering::Acquire), 0);
}

#[test]
fn corrupt_and_empty_unknown_records_can_be_removed_without_a_key() {
    let (_directory, store) = fixture();
    store.ensure_directory().unwrap();
    let invalid = [b"broken envelope".as_slice(), b"IDSET\x01", b""];
    for (index, bytes) in invalid.iter().enumerate() {
        private_file(
            &store
                .record_path(&format!("unknown.owner-{index}"))
                .unwrap(),
            bytes,
        );
    }
    let unavailable = SettingsStore::new(
        store.root.clone(),
        Arc::new(|| panic!("retained cleanup must not request a key")),
    );
    let inventory = unavailable.inventory().unwrap();
    assert_eq!(inventory.records.len(), 3);
    for record in inventory.records {
        unavailable
            .remove_record(&record.record_id, &record.revision)
            .unwrap();
        assert!(unavailable
            .remove_record(&record.record_id, &record.revision)
            .is_err());
    }
    assert!(unavailable.inventory().unwrap().records.is_empty());
    assert_eq!(unavailable.inventory().unwrap().total_bytes, 0);
}

#[test]
fn retained_deletion_requires_the_current_ciphertext_revision() {
    let (_directory, store) = fixture();
    save(&store, PLUGIN, &data());
    let path = store.record_path(PLUGIN).unwrap();
    let original = store.inventory().unwrap().records.remove(0);
    save(&store, PLUGIN, &data());
    let updated = fs::read(&path).unwrap();
    assert!(store
        .remove_record(&original.record_id, &original.revision)
        .unwrap_err()
        .contains("refresh"));
    assert_eq!(fs::read(&path).unwrap(), updated);
    let current = store.inventory().unwrap().records.remove(0);
    assert_ne!(current.revision, original.revision);
    let mut wrong_revision = current.revision.clone().into_bytes();
    wrong_revision[0] = if wrong_revision[0] == b'a' {
        b'b'
    } else {
        b'a'
    };
    let wrong_revision = String::from_utf8(wrong_revision).unwrap();
    assert!(store
        .remove_record(&current.record_id, &wrong_revision)
        .is_err());
    assert_eq!(fs::read(&path).unwrap(), updated);
    store
        .remove_record(&current.record_id, &current.revision)
        .unwrap();
    assert!(!path.exists());
}

#[test]
fn retained_record_tokens_reject_paths_extensions_and_noncanonical_hashes() {
    let (_directory, store) = fixture();
    save(&store, PLUGIN, &data());
    let path = store.record_path(PLUGIN).unwrap();
    let bytes = fs::read(&path).unwrap();
    let record = store.inventory().unwrap().records.remove(0);
    for invalid in [
        String::new(),
        "a".repeat(63),
        "a".repeat(65),
        "A".repeat(64),
        "g".repeat(64),
        "é".repeat(32),
        format!("{}.enc", record.record_id),
        format!("../{}", record.record_id),
        format!("..\\{}", record.record_id),
        format!("{PENDING_PREFIX}{}", uuid::Uuid::new_v4()),
    ] {
        assert!(store.remove_record(&invalid, &record.revision).is_err());
        assert!(store.remove_record(&record.record_id, &invalid).is_err());
    }
    assert_eq!(fs::read(path).unwrap(), bytes);
    let record_id = SettingsStore::record_id("con.example").unwrap();
    assert_eq!(record_id, format!("{:x}", Sha256::digest(b"con.example")));
    for invalid in ["../outside", "Test.invalid", "test/other", "test\\other"] {
        assert!(SettingsStore::record_id(invalid).is_err());
    }
}

#[test]
fn retained_cleanup_recovers_capacity_at_the_record_limit() {
    let (_directory, store) = fixture();
    store.ensure_directory().unwrap();
    for index in 0..MAX_SETTINGS_RECORDS {
        private_file(
            &store
                .record_path(&format!("unknown.owner-{index}"))
                .unwrap(),
            b"corrupt",
        );
    }
    let inventory = store.inventory().unwrap();
    assert_eq!(inventory.records.len(), inventory.max_records);
    assert_eq!(
        inventory.total_bytes,
        (MAX_SETTINGS_RECORDS * b"corrupt".len()) as u64
    );
    assert!(store.prepare_write(PLUGIN, &data()).is_err());
    let unavailable = SettingsStore::new(
        store.root.clone(),
        Arc::new(|| panic!("quota cleanup must not request a key")),
    );
    let removed = &inventory.records[0];
    unavailable
        .remove_record(&removed.record_id, &removed.revision)
        .unwrap();
    save(&store, PLUGIN, &data());
    assert_eq!(
        store.inventory().unwrap().records.len(),
        MAX_SETTINGS_RECORDS
    );
    assert!(store.read(PLUGIN).is_ok());
}

#[test]
fn retained_inventory_and_cleanup_reject_unsafe_directory_entries() {
    let (_directory, store) = fixture();
    save(&store, PLUGIN, &data());
    let path = store.record_path(PLUGIN).unwrap();
    let bytes = fs::read(&path).unwrap();
    let record = store.inventory().unwrap().records.remove(0);
    let unknown = store.directory.join("unexpected.json");
    private_file(&unknown, b"unrelated");
    assert!(store.inventory().is_err());
    // An unrelated entry cannot block cleanup of previously reviewed bytes.
    store
        .remove_record(&record.record_id, &record.revision)
        .unwrap();
    assert!(!path.exists());
    assert_eq!(fs::read(&unknown).unwrap(), b"unrelated");
    fs::remove_file(unknown).unwrap();
    private_file(&path, &vec![0; MAX_SETTINGS_FILE_BYTES + 1]);
    assert!(store.inventory().is_err());
    assert!(store
        .remove_record(&record.record_id, &record.revision)
        .is_err());
    assert_eq!(
        fs::metadata(&path).unwrap().len(),
        (MAX_SETTINGS_FILE_BYTES + 1) as u64
    );
    fs::remove_file(&path).unwrap();
    let external_directory = tempfile::tempdir().unwrap();
    let external = external_directory.path().join("keep");
    private_file(&external, &bytes);
    fs::hard_link(&external, &path).unwrap();
    assert!(store.inventory().is_err());
    assert!(store
        .remove_record(&record.record_id, &record.revision)
        .is_err());
    assert_eq!(fs::read(external).unwrap(), bytes);
    assert!(path.exists());
}

#[cfg(unix)]
#[test]
fn retained_inventory_and_cleanup_reject_symlinks_and_nonprivate_entries() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let (_directory, store) = fixture();
    save(&store, PLUGIN, &data());
    let path = store.record_path(PLUGIN).unwrap();
    let bytes = fs::read(&path).unwrap();
    let record = store.inventory().unwrap().records.remove(0);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(store.inventory().is_err());
    assert!(store
        .remove_record(&record.record_id, &record.revision)
        .is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    fs::set_permissions(&store.directory, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(store.inventory().is_err());
    assert!(store
        .remove_record(&record.record_id, &record.revision)
        .is_err());
    fs::set_permissions(&store.directory, fs::Permissions::from_mode(0o700)).unwrap();
    let external_directory = tempfile::tempdir().unwrap();
    let external = external_directory.path().join("keep");
    private_file(&external, &bytes);
    fs::remove_file(&path).unwrap();
    symlink(&external, &path).unwrap();
    assert!(store.inventory().is_err());
    assert!(store
        .remove_record(&record.record_id, &record.revision)
        .is_err());
    assert_eq!(fs::read(&external).unwrap(), bytes);
    fs::remove_file(&path).unwrap();
    fs::remove_dir(&store.directory).unwrap();
    symlink(external_directory.path(), &store.directory).unwrap();
    assert!(store.inventory().is_err());
    assert!(store
        .remove_record(&record.record_id, &record.revision)
        .is_err());
    assert_eq!(fs::read(external).unwrap(), bytes);
}
