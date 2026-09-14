//! Explicit acceptance test for the separately built, signed Frigate package.
//!
//! Normal host tests do not build external crates or require a broker. CI runs
//! this ignored test explicitly after building the actual worker executable.

use super::application::PackageApplication;
use super::package::{PublisherTrust, TrustStore};
use super::packaging::{build_package, write_package_atomic};
use super::protocol::{DashboardContribution, PluginManifest};
use super::runtime::{PluginHost, WorkerState};
use ed25519_dalek::SigningKey;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{sleep, timeout};

const PLUGIN: &str = "inverter-desktop.frigate";
const TOPIC: &str = "integration/frigate/events";
const NEXT_TOPIC: &str = "integration/frigate/updated";
const WAIT: Duration = Duration::from_secs(15);

struct Broker {
    executable: PathBuf,
    config: PathBuf,
    log: PathBuf,
    port: u16,
    child: Option<Child>,
}

impl Broker {
    async fn new(root: &Path) -> Self {
        // These paths are test-only inputs. Neither is consulted by production
        // startup, installation, publisher policy, or webview IPC.
        let executable = required_file("MOSQUITTO_BIN");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let config = root.join("mosquitto.conf");
        fs::write(
            &config,
            format!(
                "listener {port} 127.0.0.1\nallow_anonymous true\npersistence false\nlog_type all\n"
            ),
        )
        .unwrap();
        let mut broker = Self {
            executable,
            config,
            log: root.join("mosquitto.log"),
            port,
            child: None,
        };
        broker.start().await;
        broker
    }

    async fn start(&mut self) {
        assert!(self.child.is_none());
        let output = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log)
            .unwrap();
        self.child = Some(
            Command::new(&self.executable)
                .arg("-c")
                .arg(&self.config)
                .stdin(Stdio::null())
                .stdout(output.try_clone().unwrap())
                .stderr(output)
                .spawn()
                .expect("start the explicitly selected local Mosquitto binary"),
        );
        timeout(WAIT, async {
            loop {
                assert!(self.child.as_mut().unwrap().try_wait().unwrap().is_none());
                if TcpStream::connect(("127.0.0.1", self.port)).await.is_ok() {
                    break;
                }
                sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("private loopback MQTT broker must become ready");
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            child.wait().unwrap();
        }
    }

    async fn publish(&self, topic: &str, payload: Value) {
        self.publish_bytes(topic, &serde_json::to_vec(&payload).unwrap(), false)
            .await;
    }

    async fn publish_bytes(&self, topic: &str, payload: &[u8], retained: bool) {
        timeout(WAIT, async {
            let mut stream = TcpStream::connect(("127.0.0.1", self.port)).await.unwrap();
            // A tiny MQTT 3.1.1 producer keeps this acceptance fixture independent
            // of the worker's MQTT client implementation and dependency graph.
            let mut connect = vec![0, 4, b'M', b'Q', b'T', b'T', 4, 2, 0, 10];
            let client = format!("acceptance-{}", uuid::Uuid::new_v4());
            mqtt_string(&mut connect, &client);
            stream.write_all(&packet(0x10, &connect)).await.unwrap();
            let mut ack = [0_u8; 4];
            stream.read_exact(&mut ack).await.unwrap();
            assert_eq!(ack, [0x20, 2, 0, 0], "local producer CONNACK");
            let mut publish = Vec::new();
            mqtt_string(&mut publish, topic);
            publish.extend_from_slice(payload);
            stream
                .write_all(&packet(if retained { 0x31 } else { 0x30 }, &publish))
                .await
                .unwrap();
            stream.write_all(&[0xe0, 0]).await.unwrap();
            stream.shutdown().await.unwrap();
        })
        .await
        .expect("local publisher must not block");
        sleep(Duration::from_millis(50)).await;
    }
}

impl Drop for Broker {
    fn drop(&mut self) {
        self.stop();
    }
}

fn required_file(name: &str) -> PathBuf {
    let value = std::env::var_os(name).unwrap_or_else(|| {
        panic!("{name} must name an explicit fixture executable; this test never silently skips")
    });
    let path = PathBuf::from(value).canonicalize().unwrap();
    assert!(path.is_file(), "{name} must be a regular fixture file");
    path
}

fn mqtt_string(output: &mut Vec<u8>, value: &str) {
    output.extend_from_slice(&u16::try_from(value.len()).unwrap().to_be_bytes());
    output.extend_from_slice(value.as_bytes());
}

fn packet(header: u8, payload: &[u8]) -> Vec<u8> {
    let mut result = vec![header];
    let mut remaining = payload.len();
    loop {
        let mut byte = (remaining % 128) as u8;
        remaining /= 128;
        if remaining != 0 {
            byte |= 128;
        }
        result.push(byte);
        if remaining == 0 {
            break;
        }
    }
    result.extend_from_slice(payload);
    result
}

fn motion(id: &str, camera: &str) -> Value {
    json!({"type":"new","after":{"id":id,"camera":camera}})
}

async fn wait_connection(host: &PluginHost, expected: &str) {
    timeout(WAIT, async {
        loop {
            let found = host.snapshots().iter().any(|snapshot| {
                snapshot.plugin_id == PLUGIN
                    && snapshot.state == WorkerState::Running
                    && snapshot.contributions.iter().any(|item| {
                        matches!(item, DashboardContribution::Status { id, value, .. }
                            if id == "connection" && value == expected)
                    })
            });
            if found {
                break;
            }
            sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("real Frigate worker must report MQTT {expected}"));
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Notice {
    id: String,
    title: String,
    body: String,
}

fn drain(host: &PluginHost, notices: &mut Vec<Notice>) {
    host.dispatch_notifications(|notification| {
        // Never reenter host/auth from this synchronous guarded delivery callback.
        assert_eq!(notification.plugin_id, PLUGIN);
        notices.push(Notice {
            id: notification.id.clone(),
            title: notification.title.clone(),
            body: notification.body.clone(),
        });
    });
}

async fn expect_count(host: &PluginHost, notices: &mut Vec<Notice>, expected: usize) {
    timeout(WAIT, async {
        loop {
            drain(host, notices);
            assert!(
                notices.len() <= expected,
                "unexpected duplicate notification"
            );
            if notices.len() == expected {
                break;
            }
            sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("real motion event must reach the native notification adapter");
}

async fn expect_quiet(host: &PluginHost, notices: &mut Vec<Notice>, expected: usize) {
    // Positive events before/after suppression checks prove the real MQTT worker
    // is processing traffic; this interval additionally catches delayed output.
    sleep(Duration::from_millis(500)).await;
    drain(host, notices);
    assert_eq!(notices.len(), expected);
}

async fn configure(service: &PackageApplication, epoch: u64, port: u16, topic: &str) {
    let view = serde_json::to_value(service.get_settings(PLUGIN, epoch).await.unwrap()).unwrap();
    let saved = service
        .save_settings(
            PLUGIN,
            epoch,
            view["revision"].as_str().unwrap().into(),
            BTreeMap::from([
                ("mqtt_host".into(), json!("127.0.0.1")),
                ("mqtt_port".into(), json!(port)),
                ("mqtt_tls".into(), json!(false)),
                ("mqtt_topic".into(), json!(topic)),
            ]),
            BTreeMap::new(),
        )
        .await
        .unwrap();
    let result = serde_json::to_value(saved).unwrap();
    assert!(result["restart_error"].is_null());
}

async fn installed_application(root: &Path) -> (PackageApplication, PluginHost, u64) {
    let key = SigningKey::from_bytes(&rand::random::<[u8; 32]>());
    let publisher = "disposable-frigate-integration";
    let trust = TrustStore::new(vec![PublisherTrust::new(
        publisher.into(),
        key.verifying_key().to_bytes(),
        vec![PLUGIN.into()],
    )
    .unwrap()])
    .unwrap();
    let host = PluginHost::default();
    let service = PackageApplication::new(
        host.clone(),
        env!("INVERTER_DESKTOP_TARGET").into(),
        true,
        Arc::new(|| {}),
    );
    let settings_key = rand::random::<[u8; 32]>();
    service
        .initialize_with_key(
            Ok(root.join("store")),
            Ok(trust),
            Arc::new(move || Ok(settings_key.to_vec())),
        )
        .await;
    let epoch = service.session_changed(true).unwrap();
    let source = root.join("payload");
    fs::create_dir_all(source.join("bin")).unwrap();
    let mut manifest: PluginManifest = serde_json::from_str(include_str!(
        "../../../scripts/plugins/frigate-manifest.json"
    ))
    .unwrap();
    manifest.target = env!("INVERTER_DESKTOP_TARGET").into();
    manifest.entrypoint = format!(
        "bin/inverter-frigate-worker{}",
        std::env::consts::EXE_SUFFIX
    );
    fs::copy(
        required_file("INVERTER_FRIGATE_WORKER"),
        source.join(&manifest.entrypoint),
    )
    .unwrap();
    let bytes = build_package(manifest, &source, publisher, &key).unwrap();
    let path = root.join("frigate.idplugin");
    write_package_atomic(&path, &bytes).unwrap();
    let pending = service.begin_selection("config", epoch).unwrap();
    let preview = service
        .finish_selection(&pending, "config", epoch, path)
        .await
        .unwrap();
    assert_eq!(preview.plugin_id, PLUGIN);
    service
        .install_review(&preview.token, "config", epoch, false)
        .await
        .unwrap();
    assert!(host.snapshots().is_empty());
    (service, host, epoch)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires explicitly built INVERTER_FRIGATE_WORKER and local MOSQUITTO_BIN; CI runs this acceptance test"]
async fn signed_frigate_package_real_mqtt_lifecycle() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let mut broker = Broker::new(&root).await;
    let (service, host, epoch) = installed_application(&root).await;
    configure(&service, epoch, broker.port, TOPIC).await;
    broker
        .publish_bytes(
            TOPIC,
            &serde_json::to_vec(&motion("retained", "retained")).unwrap(),
            true,
        )
        .await;
    service.set_enabled(PLUGIN, true, epoch).await.unwrap();
    wait_connection(&host, "Connected").await;
    let mut notices = Vec::new();
    expect_quiet(&host, &mut notices, 0).await;
    broker.publish_bytes(TOPIC, b"", true).await;
    broker
        .publish("frigate/events", motion("wrong-topic", "other"))
        .await;
    broker
        .publish(
            TOPIC,
            json!({"type":"end","after":{"id":"ended","camera":"front"}}),
        )
        .await;
    broker.publish(TOPIC, motion("first", "front")).await;
    expect_count(&host, &mut notices, 1).await;
    assert!(notices[0].title.contains("Front"));
    assert_eq!(notices[0].body, "Motion started");
    broker.publish(TOPIC, motion("first", "front")).await;
    broker.publish(TOPIC, motion("same-camera", "front")).await;
    broker.publish(TOPIC, motion("second", "garden")).await;
    expect_count(&host, &mut notices, 2).await;
    expect_quiet(&host, &mut notices, 2).await;
    assert_ne!(notices[0].id, notices[1].id);

    broker.stop();
    wait_connection(&host, "Disconnected").await;
    broker.start().await;
    wait_connection(&host, "Connected").await;
    broker.publish(TOPIC, motion("first", "front")).await;
    broker.publish(TOPIC, motion("third", "garage")).await;
    expect_count(&host, &mut notices, 3).await;
    expect_quiet(&host, &mut notices, 3).await;

    configure(&service, epoch, broker.port, NEXT_TOPIC).await;
    wait_connection(&host, "Connected").await;
    broker.publish(TOPIC, motion("old-settings", "side")).await;
    broker.publish(NEXT_TOPIC, motion("fourth", "side")).await;
    expect_count(&host, &mut notices, 4).await;
    expect_quiet(&host, &mut notices, 4).await;

    service.set_enabled(PLUGIN, false, epoch).await.unwrap();
    broker
        .publish(NEXT_TOPIC, motion("disabled", "driveway"))
        .await;
    expect_quiet(&host, &mut notices, 4).await;
    service.set_enabled(PLUGIN, true, epoch).await.unwrap();
    wait_connection(&host, "Connected").await;
    broker
        .publish(NEXT_TOPIC, motion("fifth", "driveway"))
        .await;
    expect_count(&host, &mut notices, 5).await;
    // Queue real motion but revoke before dispatch: auth must discard pending
    // notifications in addition to stopping subsequent MQTT activity.
    broker
        .publish(NEXT_TOPIC, motion("queued-at-logout", "porch"))
        .await;
    timeout(WAIT, async {
        while !host.has_pending_notifications() {
            sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("a real MQTT notification must be queued before revocation");
    assert!(service.session_changed(false).is_none());
    broker
        .publish(NEXT_TOPIC, motion("logged-out", "porch"))
        .await;
    expect_quiet(&host, &mut notices, 5).await;
    let next_epoch = service.session_changed(true).unwrap();
    service.restore(next_epoch).await.unwrap();
    wait_connection(&host, "Connected").await;
    broker.publish(NEXT_TOPIC, motion("sixth", "porch")).await;
    expect_count(&host, &mut notices, 6).await;
    service
        .uninstall_with_settings(PLUGIN, true, next_epoch)
        .await
        .unwrap();
    broker
        .publish(NEXT_TOPIC, motion("uninstalled", "entry"))
        .await;
    expect_quiet(&host, &mut notices, 6).await;
    assert!(host.snapshots().is_empty());
    assert!(service
        .snapshot(next_epoch)
        .await
        .unwrap()
        .plugins
        .is_empty());
    service.close().await.unwrap();
}
