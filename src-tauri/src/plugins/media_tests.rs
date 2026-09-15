use super::*;
use crate::plugins::protocol::{HttpVideoGrant, PluginManifest, WorkerConfiguration};
use serde_json::json;
use std::collections::BTreeMap;
use std::io::Write;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time::{sleep, timeout};

fn policy() -> TransferPolicy {
    TransferPolicy {
        attempts: 3,
        delays: vec![Duration::from_millis(5)],
        connect_timeout: Duration::from_millis(200),
        read_timeout: Duration::from_millis(100),
        total_timeout: Duration::from_secs(3),
        max_bytes: MAX_CLIP_BYTES,
    }
}

fn lease() -> GenerationLease {
    GenerationLease::new("inverter-desktop.frigate".into(), 1, 1)
}

fn request(base: &str, lease: GenerationLease) -> QueuedHttpVideo {
    let manifest: PluginManifest = serde_json::from_str(include_str!(
        "../../../scripts/plugins/frigate-manifest.json"
    ))
    .unwrap();
    let configuration = WorkerConfiguration {
        revision: "test".into(),
        values: json!({"frigate_base_url": base}),
        secrets: BTreeMap::new(),
    };
    let grant = HttpVideoGrant::from_manifest_configuration(&manifest, Some(&configuration))
        .unwrap()
        .unwrap();
    QueuedHttpVideo {
        lease,
        grant,
        id: Uuid::new_v4().to_string(),
        url: format!("{base}/api/events/test/clip.mp4"),
        title: "Front camera".into(),
    }
}

async fn service(
    policy: TransferPolicy,
) -> (tempfile::TempDir, MediaService, mpsc::Receiver<MediaEvent>) {
    let root = tempfile::tempdir().unwrap();
    let (service, events) = MediaService::new();
    service
        .initialize_with_policy(root.path().canonicalize().unwrap().join("media"), policy)
        .await
        .unwrap();
    (root, service, events)
}

async fn ready(events: &mut mpsc::Receiver<MediaEvent>) -> ReadyMedia {
    match timeout(Duration::from_secs(5), events.recv())
        .await
        .unwrap()
        .unwrap()
    {
        MediaEvent::Ready(ready) => ready,
        MediaEvent::Close { .. } => panic!("expected completed clip before close"),
    }
}

async fn idle(service: &MediaService) {
    timeout(Duration::from_secs(3), async {
        while service.has_owned_work() {
            sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
}

fn response(status: &str, body: &[u8]) -> Vec<u8> {
    let mut bytes = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

async fn broker(responses: Vec<Vec<u8>>) -> (String, JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let mut requests = Vec::new();
        for response in responses {
            let (mut stream, _) = timeout(Duration::from_secs(3), listener.accept())
                .await
                .unwrap()
                .unwrap();
            // Read available header bytes together so serving a local response
            // does not require one socket operation per byte on Windows.
            let mut request = [0; 8192];
            let mut length = 0;
            while !request[..length].ends_with(b"\r\n\r\n") {
                let received = stream.read(&mut request[length..]).await.unwrap();
                assert_ne!(received, 0, "request closed before its header delimiter");
                length += received;
                assert!(
                    length < request.len(),
                    "request headers exceed fixture bound"
                );
            }
            requests.push(String::from_utf8(request[..length].to_vec()).unwrap());
            stream.write_all(&response).await.unwrap();
            stream.shutdown().await.unwrap();
        }
        requests
    });
    (format!("http://{address}"), task)
}

#[tokio::test]
async fn retries_pending_recording_empty_and_truncated_body_then_serves_exact_bytes() {
    let (base, server) = broker(vec![
        response("400 Bad Request", b"recording pending"),
        b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\nConnection: close\r\n\r\ntruncated".to_vec(),
        response("200 OK", b"complete-video"),
    ])
    .await;
    let (directory, service, mut events) = service(policy()).await;
    service.try_submit(request(&base, lease())).unwrap();
    let clip = ready(&mut events).await;
    assert_eq!(clip.error, None);
    let bytes = service
        .read_range(&clip.media_id, &clip.window_label, None, false)
        .await
        .unwrap();
    assert_eq!(bytes.bytes, b"complete-video");
    assert_eq!(bytes.status, 200);
    drop(bytes);
    let requests = server.await.unwrap();
    assert_eq!(requests.len(), 3);
    assert!(requests
        .iter()
        .all(|request| !request.to_lowercase().contains("authorization:")));
    service.window_destroyed(&clip.window_label);
    idle(&service).await;
    service.shutdown().await.unwrap();
    assert_eq!(
        fs::read_dir(directory.path().join("media"))
            .unwrap()
            .count(),
        0
    );
}

#[tokio::test]
async fn empty_body_is_retried_and_file_is_replaced_before_success() {
    let (base, server) = broker(vec![response("200 OK", b""), response("200 OK", b"video")]).await;
    let (directory, service, mut events) = service(policy()).await;
    service.try_submit(request(&base, lease())).unwrap();
    let clip = ready(&mut events).await;
    assert_eq!(clip.error, None);
    let bytes = service
        .read_range(&clip.media_id, &clip.window_label, None, false)
        .await
        .unwrap();
    assert_eq!(bytes.bytes, b"video");
    assert_eq!(bytes.status, 200);
    drop(bytes);
    assert_eq!(server.await.unwrap().len(), 2);
    service.window_failed(&clip.media_id);
    idle(&service).await;
    service.shutdown().await.unwrap();
    assert_eq!(
        fs::read_dir(directory.path().join("media"))
            .unwrap()
            .count(),
        0
    );
}

#[tokio::test]
async fn oversized_declared_or_progressive_bodies_do_not_retry_or_leave_files() {
    for response in [
        b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n".to_vec(),
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n8\r\n12345678\r\n0\r\n\r\n".to_vec(),
    ] {
        let (base, server) = broker(vec![response]).await;
        let mut limits = policy();
        limits.max_bytes = 7;
        let (directory, service, mut events) = service(limits).await;
        service.try_submit(request(&base, lease())).unwrap();
        let clip = ready(&mut events).await;
        assert_eq!(clip.error, Some(MediaError::Transfer(VideoError::Oversized)));
        assert_eq!(server.await.unwrap().len(), 1);
        assert_eq!(fs::read_dir(directory.path().join("media")).unwrap().count(), 0);
        service.window_failed(&clip.media_id);
        idle(&service).await;
        service.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn revocation_cancels_stalled_body_without_opening_a_window() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (started, observed) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut bytes = [0; 8192];
        assert!(stream.read(&mut bytes).await.unwrap() > 0);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nx")
            .await
            .unwrap();
        started.send(()).unwrap();
        let _ = stream.read(&mut bytes).await;
    });
    let (directory, service, mut events) = service(policy()).await;
    let generation = lease();
    service
        .try_submit(request(&base, generation.clone()))
        .unwrap();
    observed.await.unwrap();
    generation.revoke();
    idle(&service).await;
    assert!(events.try_recv().is_err());
    service.shutdown().await.unwrap();
    assert_eq!(
        fs::read_dir(directory.path().join("media"))
            .unwrap()
            .count(),
        0
    );
    timeout(Duration::from_secs(1), server)
        .await
        .unwrap()
        .unwrap();
}

async fn frozen_io<T>(phase: &str, operation: impl std::future::Future<Output = T>) -> T {
    let started = std::time::Instant::now();
    let virtual_started = tokio::time::Instant::now();
    tokio::pin!(operation);
    loop {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "fixture I/O stalled: {phase}"
        );
        tokio::select! {
            biased;
            result = &mut operation => {
                assert_eq!(tokio::time::Instant::now(), virtual_started);
                return result;
            }
            // Stay runnable so external socket and disk waits cannot advance test time.
            _ = tokio::task::yield_now() => {}
        }
    }
}

async fn frozen_request(stream: &mut tokio::net::TcpStream) -> String {
    frozen_io("request headers", async {
        let mut bytes = [0; 8192];
        let mut length = 0;
        while !bytes[..length].ends_with(b"\r\n\r\n") {
            assert!(length < bytes.len(), "request headers exceed fixture bound");
            let received = stream.read(&mut bytes[length..]).await.unwrap();
            assert_ne!(received, 0, "request closed before its header delimiter");
            length += received;
        }
        String::from_utf8(bytes[..length].to_vec()).unwrap()
    })
    .await
}

async fn frozen_prefix(path: &std::path::Path, expected: &[u8]) {
    frozen_io("persisted response prefix", async {
        loop {
            let path = path.to_owned();
            let bytes = tokio::task::spawn_blocking(move || fs::read(path))
                .await
                .unwrap()
                .unwrap();
            assert!(
                expected.starts_with(&bytes),
                "unexpected partial response bytes"
            );
            if bytes == expected {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn progressive_progress_survives_idle_limit_but_stall_retries() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let limits = policy();
    let (directory, service, mut events) =
        frozen_io("initialize media service", service(limits.clone())).await;
    service.try_submit(request(&base, lease())).unwrap();

    let (mut stalled, _) = frozen_io("first connection", listener.accept())
        .await
        .unwrap();
    let media_directory = directory.path().join("media");
    let mut files = frozen_io(
        "locate partial clip",
        tokio::task::spawn_blocking(move || {
            fs::read_dir(media_directory)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect::<Vec<_>>()
        }),
    )
    .await
    .unwrap();
    assert_eq!(files.len(), 1);
    let path = files.pop().unwrap();
    let mut requests = vec![frozen_request(&mut stalled).await];
    frozen_io(
        "incomplete response",
        stalled.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nx"),
    )
    .await
    .unwrap();
    frozen_prefix(&path, b"x").await;
    // Let ready tasks and the blocking disk write settle before advancing to the
    // idle timer. Immediate advance could run before the next read timer is armed.
    sleep(limits.read_timeout + Duration::from_millis(1)).await;
    let mut closed = [0; 1];
    assert_eq!(
        frozen_io("idle timeout closes client", stalled.read(&mut closed))
            .await
            .unwrap(),
        0
    );
    drop(stalled);
    sleep(limits.delays[0]).await;

    let (mut progressive, _) = frozen_io("retry connection", listener.accept())
        .await
        .unwrap();
    requests.push(frozen_request(&mut progressive).await);
    frozen_io(
        "retry headers",
        progressive.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\n"),
    )
    .await
    .unwrap();
    let progress_started = tokio::time::Instant::now();
    for (index, byte) in b"video".iter().enumerate() {
        if index != 0 {
            tokio::time::advance(Duration::from_millis(30)).await;
        }
        frozen_io("progressive body byte", progressive.write_all(&[*byte]))
            .await
            .unwrap();
        frozen_prefix(&path, &b"video"[..=index]).await;
    }
    assert_eq!(progress_started.elapsed(), Duration::from_millis(120));
    assert!(progress_started.elapsed() > limits.read_timeout);
    frozen_io("finish response", progressive.shutdown())
        .await
        .unwrap();
    drop(progressive);
    let clip = frozen_io("ready clip", ready(&mut events)).await;
    assert_eq!(clip.error, None);
    let bytes = frozen_io(
        "read completed clip",
        service.read_range(&clip.media_id, &clip.window_label, None, false),
    )
    .await
    .unwrap();
    assert_eq!(bytes.bytes, b"video");
    assert_eq!(bytes.status, 200);
    drop(bytes);
    assert_eq!(requests.len(), 2);
    assert!(requests
        .iter()
        .all(|request| request.starts_with("GET /api/events/test/clip.mp4 HTTP/1.1\r\n")));
    assert_eq!(
        listener.into_std().unwrap().accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    service.window_failed(&clip.media_id);
    frozen_io("media cleanup", async {
        while service.has_owned_work() {
            tokio::task::yield_now().await;
        }
    })
    .await;
    frozen_io("service shutdown", service.shutdown())
        .await
        .unwrap();
    assert_eq!(
        fs::read_dir(directory.path().join("media"))
            .unwrap()
            .count(),
        0
    );
}

#[tokio::test]
async fn overall_deadline_includes_backoff_and_redirects_are_not_followed() {
    for (reply, expected) in [
        (response("400 Bad Request", b"wait"), VideoError::Deadline),
        (b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/private\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(), VideoError::Http),
    ] {
        let (base, server) = broker(vec![reply]).await;
        let mut limits = policy();
        limits.total_timeout = Duration::from_millis(100);
        limits.delays = vec![Duration::from_secs(1)];
        let (_directory, service, mut events) = service(limits).await;
        service.try_submit(request(&base, lease())).unwrap();
        let clip = ready(&mut events).await;
        assert_eq!(clip.error, Some(MediaError::Transfer(expected)));
        assert_eq!(server.await.unwrap().len(), 1);
        service.window_failed(&clip.media_id);
        idle(&service).await;
        service.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn ranges_are_window_bound_bounded_and_revoked_immediately() {
    let body = vec![42; MAX_RANGE_BYTES + 100];
    let (base, server) = broker(vec![response("200 OK", &body)]).await;
    let (_directory, service, mut events) = service(policy()).await;
    let generation = lease();
    service
        .try_submit(request(&base, generation.clone()))
        .unwrap();
    let clip = ready(&mut events).await;
    assert!(service.window_ready(&clip.media_id, &clip.window_label));
    assert!(matches!(
        service
            .read_range(&clip.media_id, "main", Some("bytes=0-9"), false)
            .await,
        Err(MediaError::NotFound)
    ));
    assert!(matches!(
        service
            .read_range(&clip.media_id, &clip.window_label, None, false)
            .await,
        Err(MediaError::RangeRequired)
    ));
    let head = service
        .read_range(&clip.media_id, &clip.window_label, None, true)
        .await
        .unwrap();
    assert_eq!(head.content_length, body.len() as u64);
    assert!(head.bytes.is_empty());
    let mut held = Vec::new();
    for _ in 0..MAX_RANGE_READS {
        let response = service
            .read_range(&clip.media_id, &clip.window_label, Some("bytes=0-"), false)
            .await
            .unwrap();
        assert_eq!(response.bytes.len(), MAX_RANGE_BYTES);
        assert_eq!(response.status, 206);
        held.push(response);
    }
    assert!(matches!(
        service
            .read_range(&clip.media_id, &clip.window_label, Some("bytes=0-1"), false)
            .await,
        Err(MediaError::Busy)
    ));
    drop(held);
    let suffix = service
        .read_range(&clip.media_id, &clip.window_label, Some("bytes=-9"), false)
        .await
        .unwrap();
    assert_eq!(suffix.bytes, [42; 9]);
    drop(suffix);
    let invalid = service
        .read_range(
            &clip.media_id,
            &clip.window_label,
            Some("bytes=99999999-"),
            false,
        )
        .await
        .unwrap();
    assert_eq!(invalid.status, 416);
    assert_eq!(
        invalid.content_range,
        Some(format!("bytes */{}", body.len()))
    );
    generation.revoke();
    assert!(!service.is_window_active(&clip.media_id, &clip.window_label));
    assert!(matches!(
        service
            .read_range(&clip.media_id, &clip.window_label, Some("bytes=0-1"), false)
            .await,
        Err(MediaError::NotFound)
    ));
    match timeout(Duration::from_secs(1), events.recv())
        .await
        .unwrap()
        .unwrap()
    {
        MediaEvent::Close { window_label } => {
            assert_eq!(window_label, clip.window_label);
            service.window_destroyed(&window_label);
        }
        _ => panic!("expected generation-owned close"),
    }
    server.await.unwrap();
    idle(&service).await;
    service.shutdown().await.unwrap();
}

#[tokio::test]
async fn failed_window_preserves_sibling_and_shutdown_waits_for_close_acknowledgement() {
    let (base, server) = broker(vec![response("200 OK", b"one"), response("200 OK", b"two")]).await;
    let (directory, service, mut events) = service(policy()).await;
    service.try_submit(request(&base, lease())).unwrap();
    let first = ready(&mut events).await;
    service.try_submit(request(&base, lease())).unwrap();
    let second = ready(&mut events).await;
    service.window_failed(&first.media_id);
    assert_eq!(
        service
            .read_range(&second.media_id, &second.window_label, None, false)
            .await
            .unwrap()
            .bytes,
        b"two"
    );
    let closing = service.clone();
    let task = tokio::spawn(async move { closing.shutdown().await });
    let label = match timeout(Duration::from_secs(1), events.recv())
        .await
        .unwrap()
        .unwrap()
    {
        MediaEvent::Close { window_label } => window_label,
        _ => panic!("shutdown must close owned sibling"),
    };
    assert!(!task.is_finished());
    service.window_destroyed(&label);
    task.await.unwrap().unwrap();
    server.await.unwrap();
    assert_eq!(
        fs::read_dir(directory.path().join("media"))
            .unwrap()
            .count(),
        0
    );
}

#[tokio::test]
async fn native_close_failure_returns_error_retains_ownership_and_can_retry() {
    let (base, server) = broker(vec![response("200 OK", b"video")]).await;
    let (directory, service, mut events) = service(policy()).await;
    service.try_submit(request(&base, lease())).unwrap();
    let clip = ready(&mut events).await;
    let closing = service.clone();
    let first = tokio::spawn(async move { closing.shutdown().await });
    match timeout(Duration::from_secs(1), events.recv())
        .await
        .unwrap()
        .unwrap()
    {
        MediaEvent::Close { window_label } => service.window_cleanup_failed(&window_label),
        _ => panic!("expected native close attempt"),
    }
    assert!(timeout(Duration::from_secs(1), first)
        .await
        .unwrap()
        .unwrap()
        .is_err());
    assert!(service.has_owned_work());
    assert_eq!(service.0.tasks.lock().unwrap().len(), 1);
    assert_eq!(
        fs::read_dir(directory.path().join("media"))
            .unwrap()
            .count(),
        1
    );
    assert!(!service.is_window_active(&clip.media_id, &clip.window_label));
    let closing = service.clone();
    let second = tokio::spawn(async move { closing.shutdown().await });
    match timeout(Duration::from_secs(1), events.recv())
        .await
        .unwrap()
        .unwrap()
    {
        MediaEvent::Close { window_label } => {
            assert_eq!(window_label, clip.window_label);
            service.window_destroyed(&window_label);
        }
        _ => panic!("retry must close the same still-owned window"),
    }
    timeout(Duration::from_secs(1), second)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(!service.has_owned_work());
    assert_eq!(
        fs::read_dir(directory.path().join("media"))
            .unwrap()
            .count(),
        0
    );
    server.await.unwrap();
}

#[tokio::test]
async fn cancelled_shutdown_keeps_unjoined_tasks_for_later_drain() {
    let (base, server) = broker(vec![response("200 OK", b"video")]).await;
    let (_directory, service, mut events) = service(policy()).await;
    service.try_submit(request(&base, lease())).unwrap();
    let clip = ready(&mut events).await;
    let closing = service.clone();
    let attempt = tokio::spawn(async move { closing.shutdown().await });
    assert!(matches!(
        timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap(),
        Some(MediaEvent::Close { .. })
    ));
    attempt.abort();
    let _ = attempt.await;
    assert_eq!(service.0.tasks.lock().unwrap().len(), 1);
    service.window_destroyed(&clip.window_label);
    service.shutdown().await.unwrap();
    assert!(!service.has_owned_work());
    server.await.unwrap();
}

#[tokio::test]
async fn full_storage_keeps_eight_requests_queued_without_network_or_disk_work() {
    let (directory, service, _events) = service(policy()).await;
    {
        let mut state = service.0.state.lock().unwrap();
        for _ in 0..2 {
            state.items.insert(
                Uuid::new_v4().to_string(),
                Item {
                    lease: lease(),
                    label: "occupied".into(),
                    phase: Phase::Ready,
                    bytes: MAX_CLIP_BYTES,
                    file: None,
                    retired: watch::channel(false).0,
                    cleanup_failed: false,
                },
            );
        }
    }
    let generation = lease();
    for _ in 0..MAX_QUEUED {
        service
            .try_submit(request("http://127.0.0.1:1", generation.clone()))
            .unwrap();
    }
    assert_eq!(
        service.try_submit(request("http://127.0.0.1:1", generation.clone())),
        Err(MediaError::Busy)
    );
    tokio::task::yield_now().await;
    assert_eq!(
        fs::read_dir(directory.path().join("media"))
            .unwrap()
            .count(),
        0
    );
    generation.revoke();
    service
        .0
        .state
        .lock()
        .unwrap()
        .items
        .retain(|_, item| item.label != "occupied");
    idle(&service).await;
    service.shutdown().await.unwrap();
}

#[test]
fn ranges_reject_overflow_multiple_ranges_and_non_decimal_tokens() {
    for range in [
        "bytes=0-1,4-5",
        "bytes=-0",
        "bytes=2-1",
        "bytes=+1-2",
        "bytes=0-18446744073709551616",
        "items=0-2",
    ] {
        assert_eq!(select_range(Some(range), 10).unwrap(), None, "{range}");
    }
    assert_eq!(
        select_range(Some("bytes=-30"), 10).unwrap(),
        Some((0, 9, true))
    );
    assert_eq!(
        select_range(Some("bytes=4-100"), 10).unwrap(),
        Some((4, 9, true))
    );
}

#[tokio::test]
async fn eight_windows_backpressure_later_clips_until_native_close_is_acknowledged() {
    let (base, server) = broker((0..9).map(|_| response("200 OK", b"video")).collect()).await;
    let (_directory, service, mut events) = service(policy()).await;
    let mut clips = Vec::new();
    for _ in 0..MAX_WINDOWS {
        service.try_submit(request(&base, lease())).unwrap();
        clips.push(ready(&mut events).await);
    }
    service.try_submit(request(&base, lease())).unwrap();
    {
        let state = service.0.state.lock().unwrap();
        assert_eq!(
            state
                .items
                .values()
                .filter(|item| item.phase == Phase::Ready)
                .count(),
            MAX_WINDOWS
        );
        assert_eq!(
            state
                .items
                .values()
                .filter(|item| item.phase == Phase::Queued)
                .count(),
            1
        );
    }
    service.window_destroyed(&clips.remove(0).window_label);
    clips.push(ready(&mut events).await);
    for clip in clips {
        service.window_failed(&clip.media_id);
    }
    idle(&service).await;
    assert_eq!(server.await.unwrap().len(), 9);
    service.shutdown().await.unwrap();
}

#[tokio::test]
async fn two_transfers_hold_budget_and_revocation_cancels_third_queued_request() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (accepted, mut observing) = mpsc::channel(2);
    let server = tokio::spawn(async move {
        let mut streams = Vec::new();
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 8192];
            assert!(stream.read(&mut request).await.unwrap() > 0);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nx")
                .await
                .unwrap();
            streams.push(stream);
            accepted.send(()).await.unwrap();
        }
        std::future::pending::<()>().await;
        drop(streams);
    });
    let mut limits = policy();
    limits.read_timeout = Duration::from_secs(3);
    let (_directory, service, mut events) = service(limits).await;
    let generation = lease();
    for _ in 0..2 {
        service
            .try_submit(request(&base, generation.clone()))
            .unwrap();
    }
    observing.recv().await.unwrap();
    observing.recv().await.unwrap();
    service
        .try_submit(request(&base, generation.clone()))
        .unwrap();
    {
        let state = service.0.state.lock().unwrap();
        assert_eq!(
            state
                .items
                .values()
                .filter(|item| item.phase == Phase::Downloading)
                .count(),
            2
        );
        assert_eq!(
            state.items.values().map(|item| item.bytes).sum::<u64>(),
            MAX_MEDIA_BYTES
        );
        assert_eq!(
            state
                .items
                .values()
                .filter(|item| item.phase == Phase::Queued)
                .count(),
            1
        );
    }
    generation.revoke();
    idle(&service).await;
    assert!(events.try_recv().is_err());
    service.shutdown().await.unwrap();
    server.abort();
    let _ = server.await;
}

fn orphan(directory: &Path) -> PathBuf {
    let path = directory.join(format!("clip-{}.mp4", Uuid::new_v4()));
    let mut file = private_options()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    file.write_all(b"partial orphan").unwrap();
    path
}

#[tokio::test]
async fn initialization_recovers_only_safe_owned_orphans_and_preserves_unknown_files() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().canonicalize().unwrap().join("media");
    recover_directory(&directory).unwrap();
    let old = orphan(&directory);
    let (service, _events) = MediaService::new();
    service.initialize(directory.clone()).await.unwrap();
    assert!(!old.exists());
    service.shutdown().await.unwrap();
    let safe = orphan(&directory);
    fs::write(directory.join("unowned.txt"), b"preserve").unwrap();
    let (blocked, _events) = MediaService::new();
    assert!(blocked.initialize(directory.clone()).await.is_err());
    assert!(safe.exists());
    assert!(directory.join("unowned.txt").exists());
    assert!(!blocked.has_owned_work());
    blocked.shutdown().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn recovery_rejects_symlinks_hardlinks_and_nonprivate_files_without_deletion() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    for kind in ["symlink", "hardlink", "mode"] {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().canonicalize().unwrap().join("media");
        recover_directory(&directory).unwrap();
        let outside = root.path().join("outside");
        fs::write(&outside, b"preserve").unwrap();
        let path = directory.join(format!("clip-{}.mp4", Uuid::new_v4()));
        match kind {
            "symlink" => symlink(&outside, &path).unwrap(),
            "hardlink" => fs::hard_link(&outside, &path).unwrap(),
            _ => {
                fs::write(&path, b"preserve").unwrap();
                fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
            }
        }
        let (service, _events) = MediaService::new();
        assert!(service.initialize(directory).await.is_err());
        assert!(path.symlink_metadata().is_ok());
        assert_eq!(fs::read(&outside).unwrap(), b"preserve");
        service.shutdown().await.unwrap();
    }
}
