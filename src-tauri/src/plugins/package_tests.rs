use super::*;
use crate::plugins::protocol::{InventoryEntry, PluginPermission, SignatureMetadata};
use ed25519_dalek::{Signer, SigningKey};
use serde_json::json;
use std::io::Write;
use zip::write::SimpleFileOptions;

const TARGET: &str = "aarch64-apple-darwin";
const PLUGIN_ID: &str = "org.example.verification-test";
const KEY_ID: &str = "verification-test-key";
const WORKER: &[u8] = b"test worker bytes\n";

#[test]
fn sha256_encoding_matches_known_vectors_including_leading_zero_bytes() {
    // Fixed SHA-256 vectors independently checked with Python hashlib. Package
    // pins, inventory hashes and retained settings filenames require all 64 digits.
    for (input, expected) in [
        (
            b"".as_slice(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ),
        (
            b"abc".as_slice(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ),
        (
            b"286".as_slice(),
            "00328ce57bbc14b33bd6695bc8eb32cdf2fb5f3a7d89ec14a42825e15d39df60",
        ),
    ] {
        assert_eq!(sha256_hex(input), expected);
    }
}

fn signing_key() -> SigningKey {
    // Test fixture only; never exported to a host's production trust store.
    SigningKey::from_bytes(&[73_u8; 32])
}

fn publisher(key_id: &str, plugin_id: &str) -> PublisherTrust {
    PublisherTrust::new(
        key_id.into(),
        signing_key().verifying_key().to_bytes(),
        vec![plugin_id.into()],
    )
    .unwrap()
}

fn trust() -> TrustStore {
    TrustStore::new(vec![publisher(KEY_ID, PLUGIN_ID)]).unwrap()
}

fn unsigned_manifest() -> PluginManifest {
    PluginManifest {
        schema_version: 1,
        plugin_id: PLUGIN_ID.into(),
        version: "1.0.0".into(),
        host_api: "^1.0".into(),
        target: TARGET.into(),
        entrypoint: "bin/worker".into(),
        config_schema: json!({"type":"object","properties":{"zone":{"type":"string"}}}),
        permissions: vec![PluginPermission::DashboardContributions],
        http_video: None,
        inventory: vec![InventoryEntry {
            path: "bin/worker".into(),
            size: WORKER.len() as u64,
            sha256: sha256_hex(WORKER),
        }],
        signature: None,
    }
}

fn sign(manifest: &mut PluginManifest) {
    let signature = signing_key().sign(&manifest_signing_payload(manifest).unwrap());
    manifest.signature = Some(SignatureMetadata {
        algorithm: "ed25519".into(),
        key_id: KEY_ID.into(),
        signature: signature
            .to_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    });
}

fn signed_manifest() -> PluginManifest {
    let mut manifest = unsigned_manifest();
    sign(&mut manifest);
    manifest
}

fn zip_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
    zip_entries_for_system(
        entries,
        if cfg!(windows) {
            zip::System::Dos
        } else {
            zip::System::Unix
        },
    )
}

fn zip_entries_for_system(entries: &[(&str, &[u8])], system: zip::System) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .system(system)
        .compression_method(zip::CompressionMethod::Stored)
        .last_modified_time(zip::DateTime::from_date_and_time(1980, 1, 1, 0, 0, 0).unwrap())
        .unix_permissions(0o755);
    for (path, data) in entries {
        writer.start_file(*path, options).unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn package(manifest: &PluginManifest, worker: &[u8]) -> Vec<u8> {
    zip_entries(&[
        (MANIFEST_PATH, &canonical_manifest_bytes(manifest).unwrap()),
        ("bin/worker", worker),
    ])
}

fn valid_package() -> Vec<u8> {
    package(&signed_manifest(), WORKER)
}

fn assert_rejected(bytes: Vec<u8>) {
    assert!(
        verify_archive_bytes(bytes, &trust(), TARGET).is_err(),
        "malformed or unauthentic package was accepted"
    );
}

fn set_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn set_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn central_start(bytes: &[u8]) -> usize {
    u32_at(bytes, bytes.len() - 6).unwrap() as usize
}

#[test]
fn real_signature_verifies_and_result_owns_exact_inventory_and_archive() {
    let bytes = valid_package();
    let expected_hash = sha256_hex(&bytes);
    let verified = verify_archive_bytes(bytes.clone(), &trust(), TARGET).unwrap();
    assert_eq!(verified.manifest().plugin_id, PLUGIN_ID);
    assert_eq!(verified.archive_sha256(), expected_hash);
    assert_eq!(verified.archive_bytes(), bytes);
    assert_eq!(
        verified.files().collect::<Vec<_>>(),
        [("bin/worker", WORKER)]
    );
    assert!(!verified.archive_pin());
    assert!(verified.reverify(&TrustStore::default(), TARGET).is_err());
}

#[test]
fn configured_pin_authorizes_only_the_exact_unsigned_archive() {
    let bytes = package(&unsigned_manifest(), WORKER);
    let digest = sha256_hex(&bytes);
    let verified =
        verify_pinned_archive_bytes(bytes.clone(), PLUGIN_ID, "1.0.0", TARGET, &digest).unwrap();
    assert!(verified.archive_pin());
    assert!(verified.manifest().signature.is_none());
    assert_eq!(verified.archive_bytes(), bytes);
    assert_eq!(
        verified.files().collect::<Vec<_>>(),
        [("bin/worker", WORKER)]
    );
    assert!(verified
        .reverify(&TrustStore::default(), TARGET)
        .unwrap()
        .archive_pin());
    assert!(verify_archive_bytes(bytes.clone(), &trust(), TARGET).is_err());
    for (id, version, target, expected_digest) in [
        ("org.example.other", "1.0.0", TARGET, digest.clone()),
        (PLUGIN_ID, "1.0.1", TARGET, digest.clone()),
        (
            PLUGIN_ID,
            "1.0.0",
            "x86_64-unknown-linux-gnu",
            digest.clone(),
        ),
        (PLUGIN_ID, "1.0.0", TARGET, "0".repeat(64)),
        (PLUGIN_ID, "1.0.0", TARGET, digest.to_uppercase()),
        (PLUGIN_ID, "1.0.0", TARGET, "a".repeat(63)),
        (PLUGIN_ID, "*", TARGET, digest.clone()),
        ("../other", "1.0.0", TARGET, digest.clone()),
    ] {
        assert!(
            verify_pinned_archive_bytes(bytes.clone(), id, version, target, &expected_digest)
                .is_err(),
            "mismatched or malformed configured pin was accepted"
        );
    }
    assert!(verified
        .reverify(&trust(), "x86_64-unknown-linux-gnu")
        .is_err());
}

#[test]
fn matching_pins_never_bypass_manifest_inventory_or_archive_validation() {
    let manifest = unsigned_manifest();
    let canonical = canonical_manifest_bytes(&manifest).unwrap();
    let incompatible = String::from_utf8(canonical.clone())
        .unwrap()
        .replace("\"host_api\":\"^1.0\"", "\"host_api\":\"^2.0\"");
    let pretty = serde_json::to_vec_pretty(&manifest).unwrap();
    let mut trailing = package(&manifest, WORKER);
    trailing.push(0);
    for bytes in [
        package(&manifest, b"different worker bytes"),
        zip_entries(&[(MANIFEST_PATH, &canonical)]),
        zip_entries(&[(MANIFEST_PATH, &canonical), ("../worker", WORKER)]),
        zip_entries(&[
            (MANIFEST_PATH, incompatible.as_bytes()),
            ("bin/worker", WORKER),
        ]),
        zip_entries(&[(MANIFEST_PATH, &pretty), ("bin/worker", WORKER)]),
        zip_entries(&[
            (MANIFEST_PATH, &canonical),
            ("bin/worker", WORKER),
            ("extra", b"extra"),
        ]),
        trailing,
    ] {
        let digest = sha256_hex(&bytes);
        assert!(verify_pinned_archive_bytes(bytes, PLUGIN_ID, "1.0.0", TARGET, &digest).is_err());
    }
}

#[test]
fn windows_zip_creator_with_unix_regular_modes_verifies_on_every_host() {
    let manifest = canonical_manifest_bytes(&signed_manifest()).unwrap();
    let bytes = zip_entries_for_system(
        &[(MANIFEST_PATH, &manifest), ("bin/worker", WORKER)],
        zip::System::Dos,
    );
    let central = central_start(&bytes);
    assert_eq!(u16_at(&bytes, central + 4).unwrap() >> 8, 0);
    assert_eq!(u32_at(&bytes, central + 38).unwrap() >> 16, 0o100755);
    let verified = verify_archive_bytes(bytes, &trust(), TARGET).unwrap();
    assert_eq!(verified.files().next().unwrap().1, WORKER);

    // A classic DOS record without Unix mode bits remains an ordinary file.
    let mut classic = verified.archive_bytes().to_vec();
    set_u32(&mut classic, central + 38, 0);
    verify_archive_bytes(classic, &trust(), TARGET).unwrap();
}

#[test]
fn dos_creator_cannot_hide_symlinks_special_files_or_special_permissions() {
    let manifest = canonical_manifest_bytes(&signed_manifest()).unwrap();
    let original = zip_entries_for_system(
        &[(MANIFEST_PATH, &manifest), ("bin/worker", WORKER)],
        zip::System::Dos,
    );
    let central = central_start(&original);
    for mode in [
        0o120777, 0o040755, 0o060600, 0o020600, 0o010600, 0o140600, 0o104755, 0o102755, 0o101755,
    ] {
        let mut bytes = original.clone();
        set_u32(&mut bytes, central + 38, mode << 16);
        let error = verify_archive_bytes(bytes, &trust(), TARGET).unwrap_err();
        assert!(error.contains("not a plain regular file"), "{error}");
    }
    for mode in [0, 0o100755 << 16] {
        let mut directory = original.clone();
        set_u32(&mut directory, central + 38, mode | 0x10);
        assert_rejected(directory);
    }
}

#[test]
fn signing_bytes_have_fixed_domain_null_signature_and_sorted_schema() {
    let manifest = signed_manifest();
    let payload = manifest_signing_payload(&manifest).unwrap();
    let expected = format!(
        "inverter-desktop:idplugin:manifest:v1\0{{\"schema_version\":1,\"plugin_id\":\"{PLUGIN_ID}\",\"version\":\"1.0.0\",\"host_api\":\"^1.0\",\"target\":\"{TARGET}\",\"entrypoint\":\"bin/worker\",\"config_schema\":{{\"properties\":{{\"zone\":{{\"type\":\"string\"}}}},\"type\":\"object\"}},\"permissions\":[\"dashboard_contributions\"],\"inventory\":[{{\"path\":\"bin/worker\",\"size\":{},\"sha256\":\"{}\"}}],\"signature\":null}}",
        WORKER.len(),
        sha256_hex(WORKER),
    );
    assert_eq!(payload, expected.as_bytes());
    assert_eq!(
        payload,
        manifest_signing_payload(&unsigned_manifest()).unwrap()
    );
}

#[test]
fn unsigned_untrusted_wrong_scope_and_wrong_key_packages_are_rejected() {
    assert_rejected(package(&unsigned_manifest(), WORKER));
    let bytes = valid_package();
    assert!(verify_archive_bytes(bytes.clone(), &TrustStore::default(), TARGET).is_err());
    let wrong_scope = TrustStore::new(vec![publisher(KEY_ID, "org.other.plugin")]).unwrap();
    assert!(verify_archive_bytes(bytes.clone(), &wrong_scope, TARGET).is_err());
    let unknown_key = TrustStore::new(vec![publisher("other-key", PLUGIN_ID)]).unwrap();
    assert!(verify_archive_bytes(bytes.clone(), &unknown_key, TARGET).is_err());
    let different_key = PublisherTrust::new(
        KEY_ID.into(),
        SigningKey::from_bytes(&[74_u8; 32])
            .verifying_key()
            .to_bytes(),
        vec![PLUGIN_ID.into()],
    )
    .unwrap();
    assert!(verify_archive_bytes(
        bytes,
        &TrustStore::new(vec![different_key]).unwrap(),
        TARGET
    )
    .is_err());
}

#[test]
fn manifest_metadata_and_payload_tampering_are_rejected() {
    let mut manifest = signed_manifest();
    manifest.version = "1.0.1".into();
    assert_rejected(package(&manifest, WORKER));
    let mut altered = WORKER.to_vec();
    altered[0] ^= 1;
    assert_rejected(package(&signed_manifest(), &altered));
    let mut manifest = signed_manifest();
    manifest.inventory[0].sha256 = sha256_hex(&altered);
    assert_rejected(package(&manifest, &altered));
    let mut manifest = signed_manifest();
    manifest.signature.as_mut().unwrap().signature = "00".repeat(64);
    assert_rejected(package(&manifest, WORKER));
}

#[test]
fn incompatible_host_target_schema_and_api_are_rejected() {
    assert!(verify_archive_bytes(valid_package(), &trust(), "x86_64-apple-darwin").is_err());
    assert!(verify_archive_bytes(valid_package(), &trust(), "aarch64-linux-android").is_err());
    let original = canonical_manifest_bytes(&signed_manifest()).unwrap();
    for (before, after) in [
        ("\"schema_version\":1", "\"schema_version\":2"),
        ("\"host_api\":\"^1.0\"", "\"host_api\":\"^2.0\""),
        (TARGET, "aarch64-apple-ios"),
        ("\"ed25519\"", "\"other-algorithm\""),
    ] {
        let invalid = String::from_utf8(original.clone())
            .unwrap()
            .replace(before, after);
        assert_rejected(zip_entries(&[
            (MANIFEST_PATH, invalid.as_bytes()),
            ("bin/worker", WORKER),
        ]));
    }
}

#[test]
fn ambiguous_and_noncanonical_manifest_json_is_rejected() {
    let manifest = signed_manifest();
    let canonical = String::from_utf8(canonical_manifest_bytes(&manifest).unwrap()).unwrap();
    for invalid in [
        serde_json::to_string_pretty(&manifest).unwrap(),
        format!("{canonical}\n"),
        canonical.replace(
            "\"schema_version\":1",
            "\"schema_version\":1,\"schema_version\":1",
        ),
        canonical.replace(
            "\"type\":\"string\"",
            "\"type\":\"number\",\"type\":\"string\"",
        ),
    ] {
        assert_rejected(zip_entries(&[
            (MANIFEST_PATH, invalid.as_bytes()),
            ("bin/worker", WORKER),
        ]));
    }
}

#[test]
fn missing_extra_and_wrong_size_inventory_entries_are_rejected() {
    let manifest = signed_manifest();
    let encoded = canonical_manifest_bytes(&manifest).unwrap();
    assert_rejected(zip_entries(&[(MANIFEST_PATH, &encoded)]));
    assert_rejected(zip_entries(&[
        (MANIFEST_PATH, &encoded),
        ("bin/worker", WORKER),
        ("unlisted", b"extra"),
    ]));
    assert_rejected(zip_entries(&[
        (MANIFEST_PATH, &encoded),
        ("bin/other", WORKER),
    ]));
    let mut manifest = unsigned_manifest();
    manifest.inventory[0].size += 1;
    sign(&mut manifest);
    assert_rejected(package(&manifest, WORKER));
}

#[test]
fn path_traversal_case_aliases_device_names_and_file_directory_conflicts_are_rejected() {
    let manifest = canonical_manifest_bytes(&signed_manifest()).unwrap();
    for path in [
        "../worker",
        "/bin/worker",
        "bin\\worker",
        "C:/worker",
        "bin/CON.txt",
        "bin/LPT9",
        "bin/worker.",
        "bin/worker ",
        "bin//worker",
        "bin/.worker",
        "MANIFEST.json",
    ] {
        assert_rejected(zip_entries(&[(MANIFEST_PATH, &manifest), (path, WORKER)]));
    }
    for paths in [
        ["Bin/a", "bin/b"],
        ["bin", "bin/worker"],
        ["bin/worker", "bin"],
    ] {
        assert_rejected(zip_entries(&[
            (MANIFEST_PATH, &manifest),
            (paths[0], WORKER),
            (paths[1], WORKER),
        ]));
    }
    // The writer disallows exact duplicates, so mutate a same-length sibling in
    // both header sets to exercise the verifier independently of that safeguard.
    let mut bytes = zip_entries(&[
        (MANIFEST_PATH, &manifest),
        ("bin/worker", WORKER),
        ("bin/workez", WORKER),
    ]);
    let original = b"bin/workez";
    for offset in 0..bytes.len() - original.len() {
        if &bytes[offset..offset + original.len()] == original {
            bytes[offset..offset + original.len()].copy_from_slice(b"bin/worker");
        }
    }
    assert_rejected(bytes);
}

#[test]
fn symlinks_directories_special_files_and_special_permission_bits_are_rejected() {
    for mode in [
        0o120777, 0o040755, 0o060600, 0o010600, 0o104755, 0o102755, 0o101755,
    ] {
        let mut bytes = valid_package();
        let central = central_start(&bytes);
        set_u32(&mut bytes, central + 38, mode << 16);
        assert_rejected(bytes);
    }
    let mut bytes = valid_package();
    let central = central_start(&bytes);
    set_u16(&mut bytes, central + 4, 20); // DOS creator
    set_u32(&mut bytes, central + 38, 0x10); // Directory attribute
    assert_rejected(bytes);
}

#[test]
fn compression_encryption_descriptors_extra_fields_and_zip64_are_rejected() {
    for (offset, value) in [(4, 0), (4, 45), (6, 1), (6, 8), (6, 64), (8, 8), (28, 4)] {
        let mut bytes = valid_package();
        set_u16(&mut bytes, offset, value);
        assert_rejected(bytes);
    }
    for relative in [30, 32, 34, 36] {
        let mut bytes = valid_package();
        let central = central_start(&bytes);
        set_u16(&mut bytes, central + relative, 1);
        assert_rejected(bytes);
    }
}

#[test]
fn local_central_mismatches_overlaps_and_zip_prefix_suffix_are_rejected() {
    let mut trailing = valid_package();
    trailing.extend(b"trailing");
    assert_rejected(trailing);
    let mut prefixed = b"prefix".to_vec();
    prefixed.extend(valid_package());
    assert_rejected(prefixed);
    let mut bytes = valid_package();
    let central = central_start(&bytes);
    bytes[central + 46] ^= 1; // Central filename differs.
    assert_rejected(bytes);
    let mut bytes = valid_package();
    let central = central_start(&bytes);
    set_u32(&mut bytes, central + 42, 1); // Header alias.
    assert_rejected(bytes);
    let mut bytes = valid_package();
    set_u32(&mut bytes, 18, u32::MAX);
    set_u32(&mut bytes, 22, u32::MAX);
    assert_rejected(bytes);
    let mut bytes = valid_package();
    let end = bytes.len() - 22;
    set_u32(&mut bytes, end + 16, u32::MAX);
    assert_rejected(bytes);
}

#[test]
fn crc_is_checked_even_when_signed_inventory_data_is_intact() {
    let mut bytes = valid_package();
    let central = central_start(&bytes);
    set_u32(&mut bytes, 14, 0);
    set_u32(&mut bytes, central + 16, 0);
    let error = verify_archive_bytes(bytes, &trust(), TARGET).unwrap_err();
    assert!(error.contains("checksum"), "unexpected failure: {error}");
}

#[test]
fn malformed_short_archives_never_panic_and_size_and_count_are_bounded() {
    let original = valid_package();
    for size in 0..original.len() {
        assert_rejected(original[..size].to_vec());
    }
    assert_rejected(vec![0; MAX_ARCHIVE_BYTES + 1]);
    let mut bytes = original;
    let end = bytes.len() - 22;
    set_u16(&mut bytes, end + 8, 130);
    set_u16(&mut bytes, end + 10, 130);
    assert_rejected(bytes);
}

#[test]
fn trust_configuration_rejects_ambiguous_and_weak_authority() {
    assert!(TrustStore::new(vec![
        publisher(KEY_ID, PLUGIN_ID),
        publisher(KEY_ID, PLUGIN_ID)
    ])
    .is_err());
    assert!(PublisherTrust::new(KEY_ID.into(), [0; 32], vec![PLUGIN_ID.into()]).is_err());
    assert!(PublisherTrust::new(
        KEY_ID.into(),
        signing_key().verifying_key().to_bytes(),
        vec![]
    )
    .is_err());
    assert!(PublisherTrust::new(
        KEY_ID.into(),
        signing_key().verifying_key().to_bytes(),
        vec!["org.example.*".into()]
    )
    .is_err());
}

#[test]
fn file_verification_owns_bytes_and_limits_reads() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("worker.idplugin");
    std::fs::write(&source, valid_package()).unwrap();
    let verified = verify_archive(&source, &trust(), TARGET).unwrap();
    std::fs::write(&source, b"replaced after verification").unwrap();
    assert_eq!(verified.files().next().unwrap().1, WORKER);
    assert!(read_regular_file(&source, 2).is_err());
    assert!(read_regular_file(directory.path(), MAX_ARCHIVE_BYTES).is_err());
}

#[cfg(unix)]
#[test]
fn filesystem_symlinks_and_fifos_are_rejected_without_blocking() {
    use std::os::unix::fs::symlink;
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("archive.idplugin");
    let alias = directory.path().join("alias.idplugin");
    std::fs::write(&source, valid_package()).unwrap();
    symlink(&source, &alias).unwrap();
    assert!(verify_archive(&alias, &trust(), TARGET).is_err());
    let fifo = directory.path().join("fifo.idplugin");
    assert!(std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success());
    assert!(verify_archive(&fifo, &trust(), TARGET).is_err());
}

#[test]
fn filesystem_hardlinks_are_rejected_on_every_desktop_target() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("archive.idplugin");
    let alias = directory.path().join("alias.idplugin");
    std::fs::write(&source, valid_package()).unwrap();
    std::fs::hard_link(&source, &alias).unwrap();
    assert!(verify_archive(&source, &trust(), TARGET).is_err());
    assert!(verify_archive(&alias, &trust(), TARGET).is_err());
}
