use super::*;
use serde_json::json;
use std::sync::OnceLock;

const TEST_PLUGIN: &str = "test.fixture";

fn fixture() -> PathBuf {
    static EXECUTABLE: OnceLock<PathBuf> = OnceLock::new();
    EXECUTABLE
        .get_or_init(|| {
            let directory = std::env::temp_dir().join(format!(
                "inverter-plugin-worker-fixture-{}",
                std::process::id()
            ));
            std::fs::create_dir_all(&directory).unwrap();
            let path = directory.join(format!("worker{}", std::env::consts::EXE_SUFFIX));
            let source =
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/plugin_worker.rs");
            let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
            let output = std::process::Command::new(rustc)
                .arg("--edition=2021")
                .arg(source)
                .arg("-o")
                .arg(&path)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "fixture compile: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            path
        })
        .clone()
}

fn spec(mode: &str) -> WorkerSpec {
    WorkerSpec {
        plugin_id: TEST_PLUGIN.into(),
        executable: fixture(),
        args: vec![mode.into()],
        configuration: None,
    }
}

fn configured_spec(mode: &str) -> WorkerSpec {
    let mut worker = spec(mode);
    worker.configuration = Some(WorkerConfiguration {
        revision: "revision-1".into(),
        values: json!({"server":"https://plugin.example"}),
        secrets: [("token".into(), "fixture-secret".into())].into(),
    });
    worker
}

#[tokio::test]
async fn configuration_is_acknowledged_before_running_or_accepting_actions() {
    let host = PluginHost::default();
    host.start(configured_spec("configuration_delayed"))
        .await
        .unwrap();
    assert_eq!(
        action(&host, "echo").await.unwrap_err(),
        PluginError::Unavailable
    );
    let snapshot = ready(&host).await;
    let reply = action(&host, "echo").await.unwrap();
    assert_eq!(reply["configuration_revision"], "revision-1");
    assert_eq!(reply["configuration_secret_matches"], true);
    let public = serde_json::to_string(&snapshot).unwrap();
    assert!(!public.contains("fixture-secret"));
    assert!(!public.contains("plugin.example"));
    host.shutdown().await;
}

#[tokio::test]
async fn missing_wrong_early_and_duplicate_configuration_acknowledgements_fail() {
    for mode in [
        "configuration_wrong_ack",
        "configuration_early_data",
        "configuration_duplicate_ack",
        "configuration_no_ack",
    ] {
        let host = PluginHost::default();
        host.start(configured_spec(mode)).await.unwrap();
        let failed = wait_for(&host, |snapshot| snapshot.state == WorkerState::Failed).await;
        assert!(failed.contributions.is_empty(), "{mode}");
        assert!(
            failed.last_error.as_deref().is_some_and(|error| matches!(
                error,
                "worker_configuration_ack_invalid"
                    | "worker_duplicate_handshake"
                    | "worker_startup_timeout"
            )),
            "{mode}: {:?}",
            failed.last_error
        );
        assert_eq!(
            action(&host, "echo").await.unwrap_err(),
            PluginError::Unavailable
        );
        host.shutdown().await;
    }
}

#[tokio::test]
async fn configuration_writer_rejects_revoked_epoch_and_expired_deadline_before_write() {
    for expired in [false, true] {
        let (writer, mut reader) = tokio::io::duplex(8);
        let (outgoing, receiver) = mpsc::channel(2);
        let (_stop, stop_receiver) = watch::channel(false);
        let authority = Arc::new(Mutex::new(Authority {
            enabled: true,
            epoch: u64::from(!expired),
        }));
        outgoing
            .send(Outgoing::Configuration {
                encoded: b"secret must never be written\n".to_vec(),
                deadline: if expired {
                    Instant::now() - Duration::from_millis(1)
                } else {
                    Instant::now() + Duration::from_secs(1)
                },
            })
            .await
            .unwrap();
        drop(outgoing);
        let result = write_frames(writer, receiver, stop_receiver, authority, 0).await;
        if expired {
            assert_eq!(result.unwrap_err(), "worker_configuration_timeout");
        } else {
            result.unwrap();
        }
        let mut actual = Vec::new();
        reader.read_to_end(&mut actual).await.unwrap();
        assert!(actual.is_empty());
    }
}

#[tokio::test]
async fn interrupted_configuration_frame_never_allows_a_following_frame() {
    for revoked in [false, true] {
        let (writer, mut reader) = tokio::io::duplex(8);
        let (outgoing, receiver) = mpsc::channel(2);
        let (stop, stop_receiver) = watch::channel(false);
        let authority = Arc::new(Mutex::new(Authority {
            enabled: true,
            epoch: 0,
        }));
        outgoing
            .send(Outgoing::Configuration {
                encoded: b"configuration frame longer than the pipe\n".to_vec(),
                deadline: Instant::now()
                    + if revoked {
                        Duration::from_secs(1)
                    } else {
                        Duration::from_millis(20)
                    },
            })
            .await
            .unwrap();
        outgoing
            .send(Outgoing::Control(
                b"must never follow partial configuration\n".to_vec(),
            ))
            .await
            .unwrap();
        let task = tokio::spawn(write_frames(
            writer,
            receiver,
            stop_receiver,
            authority.clone(),
            0,
        ));
        time::sleep(Duration::from_millis(40)).await;
        if revoked {
            *authority.lock().unwrap() = Authority {
                enabled: false,
                epoch: 1,
            };
            stop.send(true).unwrap();
        }
        let result = time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap();
        if revoked {
            result.unwrap();
        } else {
            assert_eq!(result.unwrap_err(), "worker_configuration_timeout");
        }
        let mut actual = Vec::new();
        reader.read_to_end(&mut actual).await.unwrap();
        assert_eq!(actual, &b"configuration"[..8]);
    }
}

async fn wait_for(
    host: &PluginHost,
    predicate: impl Fn(&PluginSnapshot) -> bool,
) -> PluginSnapshot {
    time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(snapshot) = host.snapshots().into_iter().find(&predicate) {
                return snapshot;
            }
            time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("worker reached expected state")
}

async fn ready(host: &PluginHost) -> PluginSnapshot {
    wait_for(host, |snapshot| {
        snapshot.state == WorkerState::Running && !snapshot.contributions.is_empty()
    })
    .await
}

async fn action(host: &PluginHost, id: &str) -> Result<Value, PluginError> {
    host.action(TEST_PLUGIN, id, json!({}), Duration::from_secs(2))
        .await
}

#[tokio::test]
async fn separate_executable_handshakes_contributes_and_serves_multiple_windows() {
    let host = PluginHost::default();
    host.start(spec("normal")).await.unwrap();
    let snapshot = ready(&host).await;
    assert_eq!(snapshot.generation, 1);
    assert!(snapshot.contributions.iter().any(|item| matches!(item, DashboardContribution::Text { text, .. } if text == "Separate executable")));
    let other_window = host.clone();
    assert_eq!(
        other_window.start(spec("normal")).await.unwrap_err(),
        PluginError::AlreadyRegistered
    );
    let response = action(&other_window, "echo").await.unwrap();
    assert_eq!(response["ok"], true);
    assert_eq!(response["inherited_environment"], false);
    assert_ne!(
        response["pid"].as_u64().unwrap(),
        u64::from(std::process::id())
    );
    assert_eq!(
        action(&host, "perform_action").await.unwrap_err(),
        PluginError::UnknownAction
    );
    assert_eq!(
        host.action(
            TEST_PLUGIN,
            "echo",
            json!({"unexpected":true}),
            Duration::from_secs(1)
        )
        .await
        .unwrap_err(),
        PluginError::UnknownAction
    );
    host.shutdown().await;
    let stopped = host.snapshots().remove(0);
    assert_eq!(stopped.state, WorkerState::Stopped);
    assert!(stopped.contributions.is_empty());
    assert!(host
        .entry(TEST_PLUGIN)
        .unwrap()
        .task
        .lock()
        .unwrap()
        .is_none());
    assert_eq!(
        host.start(spec("normal")).await.unwrap_err(),
        PluginError::HostStopped
    );
}

#[tokio::test]
async fn package_start_cannot_cross_logout_and_relogin() {
    let host = PluginHost::default();
    let original_epoch = host.authority_epoch();
    host.revoke();
    host.resume();
    assert!(!host.is_authorized_epoch(original_epoch));
    let committed = AtomicBool::new(false);
    assert!(host
        .commit_in_epoch(original_epoch, || {
            committed.store(true, Ordering::Release);
            Ok(())
        })
        .is_err());
    assert!(!committed.load(Ordering::Acquire));
    assert_eq!(
        host.start_in_epoch(spec("normal"), original_epoch)
            .await
            .unwrap_err(),
        PluginError::Unavailable
    );
    assert!(host.snapshots().is_empty());
    let current_epoch = host.authority_epoch();
    assert!(host.is_authorized_epoch(current_epoch));
    host.start_in_epoch(spec("normal"), current_epoch)
        .await
        .unwrap();
    ready(&host).await;
    host.shutdown().await;
    assert!(!host.is_authorized_epoch(current_epoch));
}

#[tokio::test]
async fn uninstall_reaps_workers_and_releases_registry_capacity() {
    let host = PluginHost::default();
    for index in 0..=MAX_WORKERS {
        let mut worker = spec("normal");
        worker.plugin_id = format!("test.package{index}");
        let id = worker.plugin_id.clone();
        host.start(worker).await.unwrap();
        ready(&host).await;
        let entry = host.entry(&id).unwrap();
        host.remove(&id).await.unwrap();
        assert!(entry.reaped.load(Ordering::Acquire));
        assert!(*entry.done.borrow());
        assert!(entry.task.lock().unwrap().is_none());
        assert!(host.snapshots().is_empty());
    }
    host.shutdown().await;
}

#[tokio::test]
async fn deadlines_and_dropped_callers_cancel_and_ignore_late_results() {
    let host = PluginHost::default();
    host.start(spec("normal")).await.unwrap();
    ready(&host).await;
    let timed_out = host
        .action(TEST_PLUGIN, "hold", json!({}), Duration::from_millis(40))
        .await;
    assert_eq!(timed_out.unwrap_err(), PluginError::DeadlineExceeded);
    time::sleep(Duration::from_millis(40)).await;
    assert_eq!(action(&host, "cancel_count").await.unwrap(), 1);
    let caller = host.clone();
    let task = tokio::spawn(async move { action(&caller, "hold").await });
    time::sleep(Duration::from_millis(40)).await;
    task.abort();
    let _ = task.await;
    time::sleep(Duration::from_millis(40)).await;
    assert_eq!(action(&host, "cancel_count").await.unwrap(), 2);
    assert_eq!(action(&host, "echo").await.unwrap()["ok"], true);
    host.shutdown().await;
}

#[tokio::test]
async fn crash_recovers_in_a_fresh_generation_and_stops_after_finite_retries() {
    let host = PluginHost::default();
    host.start(spec("normal")).await.unwrap();
    ready(&host).await;
    for expected_generation in 2..=3 {
        let result = action(&host, "crash").await;
        assert_eq!(result.unwrap_err(), PluginError::Unavailable);
        let recovered = wait_for(&host, |snapshot| {
            snapshot.state == WorkerState::Running
                && snapshot.generation == expected_generation
                && !snapshot.contributions.is_empty()
        })
        .await;
        assert_eq!(recovered.restart_count, (expected_generation - 1) as u32);
        assert_eq!(action(&host, "echo").await.unwrap()["ok"], true);
    }
    assert_eq!(
        action(&host, "crash").await.unwrap_err(),
        PluginError::Unavailable
    );
    let failed = wait_for(&host, |snapshot| snapshot.state == WorkerState::Failed).await;
    assert_eq!(failed.generation, 3);
    assert_eq!(failed.restart_count, MAX_RESTARTS);
    assert!(failed.contributions.is_empty());
    host.shutdown().await;
}

#[tokio::test]
async fn rejects_bad_identity_version_oversized_frames_and_event_floods() {
    for (mode, expected) in [
        ("bad_identity", "worker_handshake_invalid"),
        ("bad_version", "worker_frame_invalid"),
        ("oversize", "worker_frame_too_large"),
        ("flood", "worker_rate_limit"),
    ] {
        let host = PluginHost::default();
        host.start(spec(mode)).await.unwrap();
        let failed = wait_for(&host, |snapshot| snapshot.state == WorkerState::Failed).await;
        assert_eq!(failed.last_error.as_deref(), Some(expected), "{mode}");
        assert!(failed.contributions.is_empty());
        assert_eq!(failed.generation, 1, "protocol failures are not restarted");
        host.shutdown().await;
    }
}

#[tokio::test]
async fn drains_stderr_and_forces_a_stalled_child_to_stop_and_reap() {
    let host = PluginHost::default();
    host.start(spec("stderr_flood")).await.unwrap();
    ready(&host).await;
    assert_eq!(action(&host, "echo").await.unwrap()["ok"], true);
    host.stop(TEST_PLUGIN).await.unwrap();
    host.start(spec("stalled")).await.unwrap();
    ready(&host).await;
    let started = Instant::now();
    time::timeout(Duration::from_secs(3), host.stop(TEST_PLUGIN))
        .await
        .unwrap()
        .unwrap();
    assert!(started.elapsed() >= STOP_GRACE);
    assert!(*host.entry(TEST_PLUGIN).unwrap().done.borrow());
    assert!(host
        .entry(TEST_PLUGIN)
        .unwrap()
        .task
        .lock()
        .unwrap()
        .is_none());
    host.shutdown().await;
}

#[tokio::test]
async fn revocation_clears_immediately_and_new_login_cannot_revive_old_generation() {
    let host = PluginHost::default();
    host.start(spec("normal")).await.unwrap();
    ready(&host).await;
    let caller = host.clone();
    let pending = tokio::spawn(async move { action(&caller, "hold").await });
    time::sleep(Duration::from_millis(20)).await;
    host.revoke();
    assert!(host.snapshots()[0].contributions.is_empty());
    host.resume();
    assert_eq!(
        action(&host, "echo").await.unwrap_err(),
        PluginError::Unavailable
    );
    assert_eq!(
        pending.await.unwrap().unwrap_err(),
        PluginError::Unavailable
    );
    host.stop(TEST_PLUGIN).await.unwrap();
    host.start(spec("normal")).await.unwrap();
    ready(&host).await;
    assert_eq!(action(&host, "echo").await.unwrap()["ok"], true);
    host.shutdown().await;
}

#[tokio::test]
async fn in_flight_capacity_rejects_excess_work_without_breaking_core_host() {
    let host = PluginHost::default();
    host.start(spec("normal")).await.unwrap();
    ready(&host).await;
    let mut requests = Vec::new();
    for _ in 0..MAX_IN_FLIGHT {
        let caller = host.clone();
        requests.push(tokio::spawn(async move { action(&caller, "hold").await }));
    }
    time::sleep(Duration::from_millis(50)).await;
    assert_eq!(action(&host, "hold").await.unwrap_err(), PluginError::Busy);
    for request in requests {
        request.abort();
        let _ = request.await;
    }
    time::sleep(Duration::from_millis(50)).await;
    assert_eq!(action(&host, "echo").await.unwrap()["ok"], true);
    host.shutdown().await;
}

#[tokio::test]
async fn startup_timeout_and_concurrent_stop_are_bounded() {
    let host = PluginHost::default();
    host.start(spec("no_hello")).await.unwrap();
    let failed = wait_for(&host, |snapshot| snapshot.state == WorkerState::Failed).await;
    assert_eq!(failed.last_error.as_deref(), Some("worker_startup_timeout"));
    host.stop(TEST_PLUGIN).await.unwrap();
    host.start(spec("stalled")).await.unwrap();
    ready(&host).await;
    let second = host.clone();
    let (first_result, second_result) =
        tokio::join!(host.stop(TEST_PLUGIN), second.stop(TEST_PLUGIN));
    first_result.unwrap();
    second_result.unwrap();
    assert!(*host.entry(TEST_PLUGIN).unwrap().done.borrow());
    host.shutdown().await;
}

#[tokio::test]
async fn dropping_last_host_signals_a_running_worker_to_stop() {
    let host = PluginHost::default();
    host.start(spec("stalled")).await.unwrap();
    ready(&host).await;
    let entry = host.entry(TEST_PLUGIN).unwrap();
    drop(host);
    time::timeout(Duration::from_secs(3), wait_stopped(&entry))
        .await
        .unwrap();
    assert_eq!(entry.snapshot().state, WorkerState::Stopped);
}

#[tokio::test]
async fn queued_writes_recheck_deadline_cancellation_and_revoked_epoch() {
    for invalidation in ["deadline", "cancel", "revoke"] {
        // Eight bytes deterministically back-pressure the actual production writer,
        // independently of the operating system's subprocess pipe capacity.
        let (writer, mut reader) = tokio::io::duplex(8);
        let (outgoing, receiver) = mpsc::channel(4);
        let (stop, stop_receiver) = watch::channel(false);
        let authority = Arc::new(Mutex::new(Authority {
            enabled: true,
            epoch: 7,
        }));
        let (cancel, cancellation) = watch::channel(false);
        let first_frame = b"first control frame is longer than eight bytes\n".to_vec();
        outgoing
            .send(Outgoing::Control(first_frame.clone()))
            .await
            .unwrap();
        outgoing
            .send(Outgoing::Action {
                encoded: b"must never be written\n".to_vec(),
                deadline: Instant::now()
                    + if invalidation == "deadline" {
                        Duration::from_millis(20)
                    } else {
                        Duration::from_secs(2)
                    },
                cancellation,
            })
            .await
            .unwrap();
        let task = tokio::spawn(write_frames(
            writer,
            receiver,
            stop_receiver,
            authority.clone(),
            7,
        ));
        time::sleep(Duration::from_millis(40)).await;
        match invalidation {
            "cancel" => {
                cancel.send(true).unwrap();
            }
            "revoke" => {
                *authority.lock().unwrap() = Authority {
                    enabled: false,
                    epoch: 8,
                };
                stop.send(true).unwrap();
            }
            _ => {}
        }
        drop(outgoing);
        let mut actual = Vec::new();
        time::timeout(Duration::from_secs(2), reader.read_to_end(&mut actual))
            .await
            .unwrap()
            .unwrap();
        task.await.unwrap().unwrap();
        assert!(
            !String::from_utf8_lossy(&actual).contains("must never"),
            "{invalidation}"
        );
        if invalidation != "revoke" {
            assert_eq!(actual, first_frame);
        }
    }
}

#[tokio::test]
async fn interruption_mid_action_frame_closes_writer_instead_of_writing_next_frame() {
    for invalidation in ["deadline", "cancel"] {
        let (writer, mut reader) = tokio::io::duplex(8);
        let (outgoing, receiver) = mpsc::channel(4);
        let (_stop, stop_receiver) = watch::channel(false);
        let authority = Arc::new(Mutex::new(Authority {
            enabled: true,
            epoch: 0,
        }));
        let (cancel, cancellation) = watch::channel(false);
        outgoing
            .send(Outgoing::Action {
                encoded: b"first action cannot complete while reader is stalled\n".to_vec(),
                deadline: Instant::now()
                    + if invalidation == "deadline" {
                        Duration::from_millis(20)
                    } else {
                        Duration::from_secs(2)
                    },
                cancellation,
            })
            .await
            .unwrap();
        outgoing
            .send(Outgoing::Control(
                b"second frame must not follow partial JSON\n".to_vec(),
            ))
            .await
            .unwrap();
        let task = tokio::spawn(write_frames(writer, receiver, stop_receiver, authority, 0));
        time::sleep(Duration::from_millis(40)).await;
        if invalidation == "cancel" {
            cancel.send(true).unwrap();
        }
        assert_eq!(
            time::timeout(Duration::from_secs(2), task)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err(),
            if invalidation == "cancel" {
                "worker_write_cancelled"
            } else {
                "worker_write_timeout"
            }
        );
        let mut actual = Vec::new();
        reader.read_to_end(&mut actual).await.unwrap();
        assert_eq!(actual, b"first ac");
    }
}

#[tokio::test]
async fn worker_count_is_bounded_and_failed_registrations_do_not_spawn() {
    let host = PluginHost::default();
    for index in 0..MAX_WORKERS {
        let mut worker = spec("normal");
        worker.plugin_id = format!("test.worker-{index}");
        host.start(worker).await.unwrap();
    }
    let mut excess = spec("normal");
    excess.plugin_id = "test.excess".into();
    assert_eq!(host.start(excess).await.unwrap_err(), PluginError::Capacity);
    let mut relative = spec("normal");
    relative.executable = "relative-worker".into();
    assert_eq!(
        host.start(relative).await.unwrap_err(),
        PluginError::InvalidWorker
    );
    assert_eq!(host.snapshots().len(), MAX_WORKERS);
    host.shutdown().await;
    assert!(host
        .snapshots()
        .iter()
        .all(|entry| entry.state == WorkerState::Stopped && entry.contributions.is_empty()));
}

#[tokio::test]
async fn a_buffered_reply_cannot_cross_logout_and_a_new_login() {
    use std::future::Future;
    use std::task::Poll;

    let host = PluginHost::default();
    host.start(spec("normal")).await.unwrap();
    ready(&host).await;
    let mut request = Box::pin(action(&host, "echo"));
    // Enqueue once, then leave the caller unpolled while the real child replies.
    std::future::poll_fn(|context| {
        assert!(request.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    time::sleep(Duration::from_millis(50)).await;
    let old_epoch = host.authority_epoch();
    host.revoke();
    host.resume();
    assert_ne!(host.authority_epoch(), old_epoch);
    assert_eq!(request.await.unwrap_err(), PluginError::Unavailable);
    host.wait_revoked().await;
    host.start(spec("normal")).await.unwrap();
    ready(&host).await;
    assert_eq!(action(&host, "echo").await.unwrap()["ok"], true);
    host.shutdown().await;
}

#[test]
fn an_older_concurrent_resume_cannot_undo_a_newer_revocation() {
    use std::sync::atomic::AtomicUsize;
    use std::sync::Barrier;

    let first_revoked = Arc::new(Barrier::new(2));
    let continue_first = Arc::new(Barrier::new(2));
    let changed = Arc::new(AtomicUsize::new(0));
    let host = PluginHost::new(Arc::new({
        let first_revoked = first_revoked.clone();
        let continue_first = continue_first.clone();
        move || {
            if changed.fetch_add(1, Ordering::AcqRel) == 0 {
                // Pause after the authority lock is released but before the
                // first revoke_epoch returns to its unlock continuation.
                first_revoked.wait();
                continue_first.wait();
            }
        }
    }));
    let older_host = host.clone();
    let older = std::thread::spawn(move || {
        let epoch = older_host.revoke_epoch();
        (epoch, older_host.resume_in_epoch(epoch))
    });
    first_revoked.wait();
    let newer_epoch = host.revoke_epoch();
    continue_first.wait();
    let (older_epoch, resumed) = older.join().unwrap();
    assert_ne!(older_epoch, newer_epoch);
    assert!(!resumed);
    assert_eq!(host.authority_epoch(), newer_epoch);
    assert!(!host.is_authorized_epoch(newer_epoch));

    assert!(host.resume_in_epoch(newer_epoch));
    assert!(!host.resume_in_epoch(older_epoch));
    assert!(host.is_authorized_epoch(newer_epoch));
}

#[tokio::test]
async fn an_old_session_action_cannot_reach_a_replacement_worker() {
    let host = PluginHost::default();
    host.start(spec("normal")).await.unwrap();
    ready(&host).await;
    let old_epoch = host.authority_epoch();
    // Keep this caller unpolled until logout and a new login have replaced the
    // worker. If dispatched, this advertised action would terminate that child.
    let old_request = host.action_in_epoch(
        TEST_PLUGIN,
        "crash",
        json!({}),
        Duration::from_secs(2),
        old_epoch,
    );
    let new_epoch = host.revoke_epoch();
    assert!(host.resume_in_epoch(new_epoch));
    host.wait_revoked().await;
    host.start_in_epoch(spec("normal"), new_epoch)
        .await
        .unwrap();
    let replacement = ready(&host).await;

    assert_eq!(old_request.await.unwrap_err(), PluginError::Unavailable);
    assert_eq!(
        host.action_in_epoch(
            TEST_PLUGIN,
            "echo",
            json!({}),
            Duration::from_secs(2),
            new_epoch,
        )
        .await
        .unwrap()["ok"],
        true
    );
    let after = host.snapshots().pop().unwrap();
    assert_eq!(after.generation, replacement.generation);
    assert_eq!(after.restart_count, replacement.restart_count);
    assert_eq!(after.state, WorkerState::Running);
    host.shutdown().await;
}

#[tokio::test]
async fn an_epoch_bound_resume_cannot_revive_a_shutdown_host() {
    let host = PluginHost::default();
    let epoch = host.revoke_epoch();
    assert!(host.resume_in_epoch(epoch));
    host.shutdown().await;
    let stopped_epoch = host.authority_epoch();
    assert!(!host.resume_in_epoch(stopped_epoch));
    assert!(!host.resume_in_epoch(epoch));
    assert!(!host.is_authorized_epoch(stopped_epoch));
}
