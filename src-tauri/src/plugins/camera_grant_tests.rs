//! Native authority tests independent of camera workers and external services.

use super::generation::GenerationLease;
use super::http_video::VideoError;
use super::media::{MediaError, MediaEvent, MediaService};
use super::protocol::{
    HttpMediaKind, HttpVideoGrant, InventoryEntry, LiveViewGrant, PluginManifest, PluginPermission,
    WorkerConfiguration,
};
use super::runtime::QueuedHttpVideo;
use serde_json::json;
use std::collections::BTreeMap;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time::timeout;

fn manifest(provider: &str) -> PluginManifest {
    let source = match provider {
        "kerberos" => include_str!("../../../scripts/plugins/kerberos-manifest.json"),
        "ring" => include_str!("../../../scripts/plugins/ring-manifest.json"),
        "frigate" => include_str!("../../../scripts/plugins/frigate-manifest.json"),
        _ => unreachable!(),
    };
    let mut manifest: PluginManifest = serde_json::from_str(source).unwrap();
    manifest.target = "aarch64-apple-darwin".into();
    manifest.inventory = vec![InventoryEntry {
        path: manifest.entrypoint.clone(),
        size: 1,
        sha256: "0".repeat(64),
    }];
    manifest.validate().unwrap();
    manifest
}

fn configuration(base: &str) -> WorkerConfiguration {
    WorkerConfiguration {
        revision: "native-grant-test".into(),
        values: json!({"snapshot_base_url":base}),
        secrets: BTreeMap::new(),
    }
}

#[test]
fn camera_manifests_require_explicit_secret_classification_and_permissions() {
    for (provider, field, permission) in [
        ("kerberos", "camera_live_urls", PluginPermission::LiveView),
        ("ring", "snapshot_bearer_token", PluginPermission::HttpVideo),
    ] {
        let original = manifest(provider);
        for replacement in [json!(false), json!(null)] {
            let mut changed = original.clone();
            changed.config_schema["properties"][field]["writeOnly"] = replacement;
            assert!(changed.validate().is_err());
        }
        let mut changed = original.clone();
        changed.config_schema["properties"][field]["type"] = json!("number");
        assert!(changed.validate().is_err());
        let mut changed = original;
        changed.permissions.retain(|value| *value != permission);
        assert!(changed.validate().is_err());
    }
    let mut ring = manifest("ring");
    ring.config_schema["properties"]["snapshot_base_url"]["writeOnly"] = json!(true);
    assert!(ring.validate().is_err());
    for seconds in [0, 19, 301, u16::MAX] {
        let mut ring = manifest("ring");
        ring.http_video.as_mut().unwrap().cooldown_seconds = Some(seconds);
        assert!(ring.validate().is_err());
    }
}

#[test]
fn media_credentials_are_explicit_bounded_and_never_part_of_diagnostics() {
    let manifest = manifest("ring");
    let secret = format!("fixture-{}", uuid::Uuid::new_v4());
    let mut configuration = configuration("https://camera.invalid/api/snapshot");
    configuration.values["snapshot_bearer_token"] = json!(secret);
    configuration
        .secrets
        .insert("ha_token".into(), secret.clone());
    let grant = HttpVideoGrant::from_manifest_configuration(&manifest, Some(&configuration))
        .unwrap()
        .unwrap();
    assert_eq!(
        grant.bearer_token(),
        None,
        "neither public values nor legacy HA tokens grant credentials"
    );
    configuration
        .secrets
        .insert("snapshot_bearer_token".into(), secret.clone());
    let grant = HttpVideoGrant::from_manifest_configuration(&manifest, Some(&configuration))
        .unwrap()
        .unwrap();
    assert_eq!(grant.bearer_token(), Some(secret.as_str()));
    assert_eq!(grant.cooldown(), Duration::from_secs(20));
    assert_eq!(format!("{grant:?}"), "HttpVideoGrant { base: [redacted] }");
    for secret in [
        "x".repeat(4097),
        "private\r\nX-Injected: value".into(),
        "private\0value".into(),
    ] {
        configuration
            .secrets
            .insert("snapshot_bearer_token".into(), secret);
        assert_eq!(
            HttpVideoGrant::from_manifest_configuration(&manifest, Some(&configuration))
                .unwrap_err(),
            "Invalid media credential"
        );
    }
}

#[test]
fn query_opt_in_preserves_exact_origin_and_path_scope() {
    let mut manifest = manifest("ring");
    let configuration = configuration("https://camera.invalid:9443/api/snapshot");
    let grant = HttpVideoGrant::from_manifest_configuration(&manifest, Some(&configuration))
        .unwrap()
        .unwrap();
    assert!(grant
        .validate_url("https://camera.invalid:9443/api/snapshot/front?private=value")
        .is_ok());
    for url in [
        "http://camera.invalid:9443/api/snapshot/front?private=value",
        "https://other.invalid:9443/api/snapshot/front?private=value",
        "https://camera.invalid/api/snapshot/front?private=value",
        "https://camera.invalid:9443/api/snapshot-other/front?private=value",
        "https://camera.invalid:9443/api/snapshot/../private?private=value",
        "https://camera.invalid:9443/api/snapshot/%252e%252e/private?private=value",
        "https://camera.invalid:9443/api/snapshot/front#private",
        "https://user:secret@camera.invalid:9443/api/snapshot/front",
    ] {
        let error = grant.validate_url(url).unwrap_err();
        assert!(!error.contains("private") && !error.contains("secret"));
    }
    manifest.http_video.as_mut().unwrap().allow_query = false;
    let grant = HttpVideoGrant::from_manifest_configuration(&manifest, Some(&configuration))
        .unwrap()
        .unwrap();
    assert!(grant
        .validate_url("https://camera.invalid:9443/api/snapshot/front?private=value")
        .is_err());
    assert!(grant
        .validate_url("https://camera.invalid:9443/api/snapshot/front")
        .is_ok());
}

fn live_configuration(value: String) -> WorkerConfiguration {
    WorkerConfiguration {
        revision: "live-test".into(),
        values: json!({}),
        secrets: BTreeMap::from([("camera_live_urls".into(), value)]),
    }
}

#[test]
fn live_destinations_are_bounded_exact_private_mappings() {
    let manifest = manifest("kerberos");
    let private = "https://camera.invalid/live?private=value#front";
    let configuration = live_configuration(json!({"front-camera": private}).to_string());
    let grant = LiveViewGrant::from_manifest_configuration(&manifest, Some(&configuration))
        .unwrap()
        .unwrap();
    assert_eq!(grant.resolve("front-camera").unwrap().as_str(), private);
    assert!(grant.resolve("front").is_none());
    assert_eq!(
        format!("{grant:?}"),
        "LiveViewGrant { destinations: [redacted] }"
    );
    let maps: BTreeMap<_, _> = (0..32)
        .map(|index| (format!("camera-{index}"), private))
        .collect();
    let configuration = live_configuration(serde_json::to_string(&maps).unwrap());
    assert!(LiveViewGrant::from_manifest_configuration(&manifest, Some(&configuration)).is_ok());
    let mut maps = maps;
    maps.insert("camera-33".into(), private);
    let mut invalid = vec![
        "not-json".into(),
        "[]".into(),
        "x".repeat(16 * 1024 + 1),
        serde_json::to_string(&maps).unwrap(),
    ];
    for (id, url) in [
        ("", private),
        ("camera/+", private),
        ("camera\n", private),
        ("front", "https://user:private@camera.invalid/live"),
        ("front", "https://@camera.invalid/live"),
        ("front", "file:///private/camera"),
        ("front", "https://camera.invalid/\nprivate"),
    ] {
        invalid.push(json!({id: url}).to_string());
    }
    invalid.push(
        json!({"front": "https://camera.invalid/".to_owned() + &"a".repeat(2048)}).to_string(),
    );
    for value in invalid {
        let configuration = live_configuration(value);
        let error = LiveViewGrant::from_manifest_configuration(&manifest, Some(&configuration))
            .unwrap_err();
        assert!(!error.contains("private"));
    }
}

async fn ready(receiver: &mut tokio::sync::mpsc::Receiver<MediaEvent>) -> super::media::ReadyMedia {
    match timeout(Duration::from_secs(5), receiver.recv())
        .await
        .unwrap()
        .unwrap()
    {
        MediaEvent::Ready(ready) => ready,
        MediaEvent::Close { .. } => panic!("unexpected close before transfer completion"),
    }
}

#[tokio::test]
async fn bearer_is_not_forwarded_by_redirects_or_reused_by_another_plugin() {
    let source = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let redirect = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", source.local_addr().unwrap());
    let destination = format!("http://{}/private", redirect.local_addr().unwrap());
    let secret = format!("fixture-{}", uuid::Uuid::new_v4());
    let expected = secret.clone();
    let server = tokio::spawn(async move {
        for index in 0..2 {
            let (mut socket, _) = timeout(Duration::from_secs(5), source.accept())
                .await
                .unwrap()
                .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                timeout(Duration::from_secs(3), socket.read_exact(&mut byte))
                    .await
                    .unwrap()
                    .unwrap();
                request.push(byte[0]);
                assert!(request.len() <= 8192);
            }
            let headers = String::from_utf8(request).unwrap().to_ascii_lowercase();
            if index == 0 {
                assert!(headers.contains(&format!("\r\nauthorization: bearer {expected}\r\n")));
                socket.write_all(format!("HTTP/1.1 302 Found\r\nLocation: {destination}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
            } else {
                assert!(!headers.contains("authorization:") && !headers.contains("cookie:"));
                socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 12\r\nConnection: close\r\n\r\n\x89PNG\r\n\x1a\nDATA").await.unwrap();
            }
        }
    });
    let directory = tempfile::tempdir().unwrap();
    let (service, mut receiver) = MediaService::new();
    service
        .initialize(directory.path().canonicalize().unwrap().join("media"))
        .await
        .unwrap();
    let mut configuration = configuration(&base);
    configuration
        .secrets
        .insert("snapshot_bearer_token".into(), secret);
    let ring = HttpVideoGrant::from_manifest_configuration(&manifest("ring"), Some(&configuration))
        .unwrap()
        .unwrap();
    service
        .try_submit(QueuedHttpVideo {
            live_preview: false,
            lease: GenerationLease::new("inverter-desktop.ring".into(), 1, 1),
            grant: ring,
            id: uuid::Uuid::new_v4().to_string(),
            url: format!("{base}/snapshot/front?private=value"),
            title: "Front".into(),
            media_kind: HttpMediaKind::Png,
        })
        .unwrap();
    let first = ready(&mut receiver).await;
    assert_eq!(first.error, Some(MediaError::Transfer(VideoError::Http)));
    assert!(timeout(Duration::from_millis(50), redirect.accept())
        .await
        .is_err());
    service.window_failed(&first.media_id);
    let frigate = HttpVideoGrant::from_manifest_configuration(
        &manifest("frigate"),
        Some(&WorkerConfiguration {
            revision: "frigate-test".into(),
            values: json!({"frigate_base_url":base}),
            secrets: BTreeMap::new(),
        }),
    )
    .unwrap()
    .unwrap();
    service
        .try_submit(QueuedHttpVideo {
            live_preview: false,
            lease: GenerationLease::new("inverter-desktop.frigate".into(), 1, 1),
            grant: frigate,
            id: uuid::Uuid::new_v4().to_string(),
            url: format!("{base}/snapshot/front"),
            title: "Other provider".into(),
            media_kind: HttpMediaKind::Png,
        })
        .unwrap();
    let second = ready(&mut receiver).await;
    assert_eq!(second.error, None);
    service.window_failed(&second.media_id);
    server.await.unwrap();
    service.shutdown().await.unwrap();
}
