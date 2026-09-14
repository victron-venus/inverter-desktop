#[cfg(desktop)]
use super::CAMERA_VIDEO_LABEL_PREFIX;
use super::{load_config, FullConfig};
use log::{info, warn};
use std::time::Duration;
use tauri::Manager;

const CAMERA_CLIP_SUBDIR: &str = "inverter-desktop-camera";
const CAMERA_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const CAMERA_READ_TIMEOUT: Duration = Duration::from_secs(60);
const CAMERA_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(180);
const CAMERA_MAX_ATTEMPTS: u32 = 8;
const CAMERA_BACKOFF_MS: [u64; 7] = [1000, 2000, 3000, 4000, 5000, 5000, 5000];

struct CameraDownloadPolicy<'a> {
    max_attempts: u32,
    backoff_ms: &'a [u64],
    overall_timeout: Duration,
}

fn camera_clip_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let temp = app
        .path()
        .temp_dir()
        .map_err(|e| format!("Failed to resolve temp dir: {e}"))?;
    Ok(temp.join(CAMERA_CLIP_SUBDIR))
}

#[cfg(desktop)]
pub(super) fn remove_camera_clip_file(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
    if let Some(parent) = path.parent() {
        // Best-effort: drop the temp dir when the last clip file is gone.
        let _ = std::fs::remove_dir(parent);
    }
}

#[cfg(desktop)]
pub(super) fn is_camera_video_label(label: &str) -> bool {
    label.starts_with(CAMERA_VIDEO_LABEL_PREFIX) || label == "camera-video"
}

/// Truncate an error/response snippet for UI and logs.
fn short_http_body_snippet(body: &str) -> String {
    const MAX: usize = 180;
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for (i, ch) in trimmed.chars().enumerate() {
        if i >= MAX {
            out.push('…');
            break;
        }
        // Keep the message on one line for the camera-video error query.
        if ch.is_whitespace() {
            out.push(' ');
        } else {
            out.push(ch);
        }
    }
    out
}

/// HTTP statuses that often mean "try again shortly" for Frigate clip URLs.
///
/// Frigate's `/api/events/{id}/clip.mp4` builds the MP4 from recording segments.
/// `has_clip` only means recording is enabled for the event — segments are written
/// in ~10s chunks, so a just-ended event commonly returns **400** with
/// `No recordings found for the specified time range` until the segment lands.
/// 404 covers "clip not available" / missing event; 408/425/429/5xx are transient.
fn camera_clip_http_status_retryable(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 400 | 404 | 408 | 425 | 429) || status.is_server_error()
}

fn format_camera_clip_http_error(
    status: reqwest::StatusCode,
    video_url: &str,
    body: &str,
) -> String {
    let snippet = short_http_body_snippet(body);
    if snippet.is_empty() {
        format!("Failed to download camera clip: HTTP {status} ({video_url})")
    } else {
        format!("Failed to download camera clip: HTTP {status} ({video_url}): {snippet}")
    }
}

/// Reqwest labels any failed body transfer as a decode error. Include the source
/// chain so a timeout or truncated HTTP response is distinguishable from it.
fn format_camera_transfer_error(stage: &str, error: reqwest::Error, video_url: &str) -> String {
    use std::error::Error;

    // Reqwest already appends the URL to Display; add it only once below.
    let error = error.without_url();
    let mut detail = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        detail.push_str(": ");
        detail.push_str(&cause.to_string());
        source = cause.source();
    }
    format!("{stage}: {detail} ({video_url})")
}

/// Resolve the same HA origin used by the REST client. Credentials must never
/// be selected by searching the complete camera URL for a hostname.
fn camera_bearer_for_config(config: &FullConfig, video_url: &str) -> Option<String> {
    let token = config.ha_longlived_token.as_deref()?.trim();
    if token.is_empty() {
        return None;
    }
    let base = config.ha_url.as_deref()?.trim();
    let mut ha = reqwest::Url::parse(&if base.contains("://") {
        base.to_owned()
    } else {
        format!("http://{base}")
    })
    .ok()?;
    if !matches!(ha.scheme(), "http" | "https")
        || !ha.username().is_empty()
        || ha.password().is_some()
    {
        return None;
    }
    // An explicit authority port wins; otherwise use the HA REST port.
    let authority = base.split("://").last()?.split(['/', '?', '#']).next()?;
    let explicit_port = if authority.starts_with('[') {
        authority
            .split(']')
            .nth(1)
            .is_some_and(|rest| rest.starts_with(':'))
    } else {
        authority.contains(':')
    };
    if !explicit_port {
        ha.set_port(Some(config.ha_port.unwrap_or(8123))).ok()?;
    }
    let video = reqwest::Url::parse(video_url).ok()?;
    if !video.username().is_empty() || video.password().is_some() || video.origin() != ha.origin() {
        return None;
    }
    Some(format!("Bearer {token}"))
}

fn camera_download_bearer_token(app: &tauri::AppHandle, video_url: &str) -> Option<String> {
    camera_bearer_for_config(&load_config(app).ok()?, video_url)
}

fn camera_media_extension(content_type: Option<&str>, video_url: &str) -> &'static str {
    let ct = content_type.unwrap_or("").to_ascii_lowercase();
    if ct.contains("image/jpeg") || ct.contains("image/jpg") {
        return "jpg";
    }
    if ct.contains("image/png") {
        return "png";
    }
    if ct.contains("image/webp") {
        return "webp";
    }
    if ct.contains("image/") {
        return "img";
    }
    let path = video_url
        .split('?')
        .next()
        .unwrap_or(video_url)
        .to_ascii_lowercase();
    if path.ends_with(".jpg") || path.ends_with(".jpeg") {
        return "jpg";
    }
    if path.ends_with(".png") {
        return "png";
    }
    if path.ends_with(".webp") {
        return "webp";
    }
    "mp4"
}

fn build_camera_http_client(
    builder: reqwest::ClientBuilder,
    connect_timeout: Duration,
    read_timeout: Duration,
) -> Result<reqwest::Client, String> {
    builder
        // A camera redirect must not carry HA credentials to another origin.
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(connect_timeout)
        // Frigate streams generated clips without a Content-Length. A total
        // request deadline can abort a healthy long download; fail only when
        // the connection itself stops making progress.
        .read_timeout(read_timeout)
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))
}

fn camera_retry_delay(policy: &CameraDownloadPolicy<'_>, attempt: u32) -> Duration {
    let index = (attempt as usize).saturating_sub(1);
    Duration::from_millis(
        policy
            .backoff_ms
            .get(index)
            .or_else(|| policy.backoff_ms.last())
            .copied()
            .unwrap_or_default(),
    )
}

async fn fetch_camera_clip_with_policy(
    client: &reqwest::Client,
    video_url: &str,
    auth: Option<&str>,
    policy: CameraDownloadPolicy<'_>,
) -> Result<(Vec<u8>, &'static str), String> {
    let operation = async {
        let mut last_err = String::new();

        for attempt in 1..=policy.max_attempts {
            let mut req = client.get(video_url);
            if let Some(bearer) = auth {
                req = req.header(reqwest::header::AUTHORIZATION, bearer);
            }
            let response = match req.send().await {
                Ok(response) => response,
                Err(error) => {
                    last_err = format_camera_transfer_error(
                        "Failed to download camera clip",
                        error,
                        video_url,
                    );
                    if attempt < policy.max_attempts {
                        let delay = camera_retry_delay(&policy, attempt);
                        warn!(
                            "Camera clip download attempt {attempt}/{} failed (send/connect); retrying in {}ms: {last_err}",
                            policy.max_attempts,
                            delay.as_millis()
                        );
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    return Err(last_err);
                }
            };

            let status = response.status();
            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                last_err = format_camera_clip_http_error(status, video_url, &body);
                if attempt < policy.max_attempts && camera_clip_http_status_retryable(status) {
                    let delay = camera_retry_delay(&policy, attempt);
                    warn!(
                        "Camera clip download attempt {attempt}/{} got HTTP {status}; retrying in {}ms url={video_url} body={}",
                        policy.max_attempts,
                        delay.as_millis(),
                        short_http_body_snippet(&body)
                    );
                    tokio::time::sleep(delay).await;
                    continue;
                }
                return Err(last_err);
            }

            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string);
            let ext = camera_media_extension(content_type.as_deref(), video_url);
            let version = response.version();
            let content_length = response.content_length();

            let bytes = match response.bytes().await {
                Ok(bytes) => bytes,
                Err(error) => {
                    last_err = format_camera_transfer_error(
                        "Failed to read camera clip body",
                        error,
                        video_url,
                    );
                    warn!(
                        "Camera clip body transfer failed on attempt {attempt}/{} (status={status}, protocol={version:?}, content_length={content_length:?}, content_type={content_type:?}): {last_err}",
                        policy.max_attempts
                    );
                    if attempt < policy.max_attempts {
                        let delay = camera_retry_delay(&policy, attempt);
                        warn!(
                            "Camera clip download attempt {attempt}/{} failed reading body; retrying in {}ms url={video_url}",
                            policy.max_attempts,
                            delay.as_millis()
                        );
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    return Err(last_err);
                }
            };

            if bytes.is_empty() {
                last_err = format!("Downloaded camera clip is empty ({video_url})");
                if attempt < policy.max_attempts {
                    let delay = camera_retry_delay(&policy, attempt);
                    warn!(
                        "Camera clip download attempt {attempt}/{} got empty body; retrying in {}ms url={video_url}",
                        policy.max_attempts,
                        delay.as_millis()
                    );
                    tokio::time::sleep(delay).await;
                    continue;
                }
                return Err(last_err);
            }

            if attempt > 1 {
                info!(
                    "Camera clip download succeeded on attempt {attempt}/{} url={video_url}",
                    policy.max_attempts
                );
            }
            return Ok((bytes.to_vec(), ext));
        }

        Err(last_err)
    };

    tokio::time::timeout(policy.overall_timeout, operation)
        .await
        .unwrap_or_else(|_| {
            Err(format!(
                "Camera clip download timed out after {:?} ({video_url})",
                policy.overall_timeout
            ))
        })
}

pub(super) async fn download_camera_clip(
    app: &tauri::AppHandle,
    video_url: &str,
) -> Result<std::path::PathBuf, String> {
    let url_trim = video_url.trim();
    if url_trim.to_ascii_lowercase().starts_with("rtsp://") {
        return Err(
            "RTSP is not supported by the camera window (HTTP/HTTPS clips or snapshots only). \
Expose ring-mqtt on the LAN and set ring_snapshot_url_template to an HTTP snapshot/HLS URL \
(see docs/ring-mqtt.md)."
                .into(),
        );
    }

    let clip_dir = camera_clip_dir(app)?;
    std::fs::create_dir_all(&clip_dir)
        .map_err(|e| format!("Failed to create camera clip temp dir: {e}"))?;
    // Do not wipe the clip dir — other camera windows may still be playing.

    let client = build_camera_http_client(
        reqwest::Client::builder(),
        CAMERA_CONNECT_TIMEOUT,
        CAMERA_READ_TIMEOUT,
    )?;

    let auth = camera_download_bearer_token(app, url_trim);
    // Frigate recording segments can take ~10s after `end`+`has_clip`; cover that
    // window plus brief network blips without allowing retries to run forever.
    let (bytes, ext) = fetch_camera_clip_with_policy(
        &client,
        url_trim,
        auth.as_deref(),
        CameraDownloadPolicy {
            max_attempts: CAMERA_MAX_ATTEMPTS,
            backoff_ms: &CAMERA_BACKOFF_MS,
            overall_timeout: CAMERA_DOWNLOAD_TIMEOUT,
        },
    )
    .await?;

    let file_name = format!("clip-{}.{ext}", uuid::Uuid::new_v4());
    let dest = clip_dir.join(file_name);
    std::fs::write(&dest, &bytes)
        .map_err(|e| format!("Failed to write camera clip to temp file: {e}"))?;

    Ok(dest)
}

#[cfg(test)]
mod camera_clip_download_tests {
    use super::*;

    fn read_http_request(socket: &mut std::net::TcpStream) {
        use std::io::Read;

        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        let mut byte = [0];
        while !request.ends_with(b"\r\n\r\n") {
            socket.read_exact(&mut byte).unwrap();
            request.push(byte[0]);
        }
    }

    #[tokio::test]
    async fn body_read_timeout_resets_while_streaming_makes_progress() {
        use std::io::Write;

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/clip.mp4", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            read_http_request(&mut socket);
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: video/mp4\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
            socket.flush().unwrap();
            for byte in b"clip" {
                std::thread::sleep(Duration::from_millis(350));
                socket.write_all(&[*byte]).unwrap();
                socket.flush().unwrap();
            }
        });
        let client = build_camera_http_client(
            reqwest::Client::builder().no_proxy(),
            Duration::from_secs(1),
            Duration::from_secs(1),
        )
        .unwrap();
        let started = std::time::Instant::now();
        let response = client.get(&url).send().await.unwrap();
        let bytes = response.bytes().await.unwrap();

        assert_eq!(bytes.as_ref(), b"clip");
        assert!(started.elapsed() > Duration::from_secs(1));
        server.join().unwrap();
    }

    #[tokio::test]
    async fn stalled_body_is_retried_and_the_next_response_succeeds() {
        use std::io::Write;

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/clip.mp4", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for attempt in 1..=2 {
                let (mut socket, _) = listener.accept().unwrap();
                read_http_request(&mut socket);
                if attempt == 1 {
                    socket
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nContent-Type: video/mp4\r\nConnection: close\r\n\r\n")
                        .unwrap();
                    socket.flush().unwrap();
                    std::thread::sleep(Duration::from_millis(250));
                } else {
                    socket
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nContent-Type: video/mp4\r\nConnection: close\r\n\r\nclip")
                        .unwrap();
                }
            }
        });
        let client = build_camera_http_client(
            reqwest::Client::builder().no_proxy(),
            Duration::from_secs(1),
            Duration::from_millis(100),
        )
        .unwrap();
        let (bytes, ext) = fetch_camera_clip_with_policy(
            &client,
            &url,
            None,
            CameraDownloadPolicy {
                max_attempts: 2,
                backoff_ms: &[200],
                overall_timeout: Duration::from_secs(2),
            },
        )
        .await
        .unwrap();

        assert_eq!(bytes, b"clip");
        assert_eq!(ext, "mp4");
        server.join().unwrap();
    }

    #[tokio::test]
    async fn overall_timeout_includes_retry_backoff() {
        use std::io::Write;

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/clip.mp4", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            read_http_request(&mut socket);
            socket
                .write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .unwrap();
        });
        let client = build_camera_http_client(
            reqwest::Client::builder().no_proxy(),
            Duration::from_secs(1),
            Duration::from_secs(1),
        )
        .unwrap();
        let started = std::time::Instant::now();
        let error = fetch_camera_clip_with_policy(
            &client,
            &url,
            None,
            CameraDownloadPolicy {
                max_attempts: 2,
                backoff_ms: &[5000],
                overall_timeout: Duration::from_millis(50),
            },
        )
        .await
        .unwrap_err();

        assert!(error.contains("timed out after 50ms"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(1));
        server.join().unwrap();
    }

    #[tokio::test]
    async fn truncated_http_body_reports_cause_and_url_once() {
        use std::io::Write;

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/clip.mp4", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            read_http_request(&mut socket);
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nContent-Type: video/mp4\r\nConnection: close\r\n\r\npartial")
                .unwrap();
        });
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let response = client.get(&url).send().await.unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let error = response.bytes().await.unwrap_err();
        assert!(error.is_decode());
        let message = format_camera_transfer_error("Failed to read camera clip body", error, &url);
        assert!(
            message.contains("end of file before message length reached"),
            "{message}"
        );
        assert_eq!(message.matches(&url).count(), 1, "{message}");
        server.join().unwrap();
    }

    #[test]
    fn camera_token_is_limited_to_exact_ha_origin() {
        let config = FullConfig {
            ha_url: Some("https://ha.example.test".into()),
            ha_port: Some(443),
            ha_longlived_token: Some("test-token".into()),
            ..Default::default()
        };
        assert_eq!(
            camera_bearer_for_config(
                &config,
                "https://ha.example.test/api/camera_proxy/camera.front"
            ),
            Some("Bearer test-token".into())
        );
        for url in [
            "https://other.invalid/?ha=ha.example.test",
            "https://ha.example.test.other.invalid/",
            "http://ha.example.test/",
            "https://ha.example.test:8443/",
            "https://ha.example.test@other.invalid/",
            "https://other.invalid/https://ha.example.test",
        ] {
            assert_eq!(camera_bearer_for_config(&config, url), None, "{url}");
        }
    }

    #[test]
    fn camera_token_respects_configured_http_port() {
        let config = FullConfig {
            ha_url: Some("http://ha.example.test".into()),
            ha_port: Some(8123),
            ha_longlived_token: Some("test-token".into()),
            ..Default::default()
        };
        assert!(
            camera_bearer_for_config(&config, "http://ha.example.test:8123/snapshot").is_some()
        );
        assert!(camera_bearer_for_config(&config, "http://ha.example.test/snapshot").is_none());
    }

    #[test]
    fn camera_origin_follows_https_ha_port_and_explicit_default_ports() {
        let mut config = FullConfig {
            ha_url: Some("https://ha.example.test".into()),
            ha_port: Some(8123),
            ha_longlived_token: Some("test-token".into()),
            ..Default::default()
        };
        assert!(
            camera_bearer_for_config(&config, "https://ha.example.test:8123/snapshot").is_some()
        );
        assert!(camera_bearer_for_config(&config, "https://ha.example.test/snapshot").is_none());
        config.ha_url = Some("http://ha.example.test:80".into());
        assert!(camera_bearer_for_config(&config, "http://ha.example.test/snapshot").is_some());
        assert!(
            camera_bearer_for_config(&config, "http://ha.example.test:8123/snapshot").is_none()
        );
        config.ha_url = Some("https://ha.example.test:443".into());
        assert!(camera_bearer_for_config(&config, "https://ha.example.test/snapshot").is_some());
        assert!(
            camera_bearer_for_config(&config, "https://ha.example.test:8123/snapshot").is_none()
        );
    }

    #[test]
    fn retries_frigate_no_recordings_400() {
        let status = reqwest::StatusCode::from_u16(400).unwrap();
        assert!(camera_clip_http_status_retryable(status));
    }

    #[test]
    fn retries_404_and_5xx_still() {
        assert!(camera_clip_http_status_retryable(
            reqwest::StatusCode::from_u16(404).unwrap()
        ));
        assert!(camera_clip_http_status_retryable(
            reqwest::StatusCode::from_u16(503).unwrap()
        ));
    }

    #[test]
    fn does_not_retry_403() {
        assert!(!camera_clip_http_status_retryable(
            reqwest::StatusCode::from_u16(403).unwrap()
        ));
    }

    #[test]
    fn http_error_includes_status_url_and_body_snippet() {
        let status = reqwest::StatusCode::from_u16(400).unwrap();
        let msg = format_camera_clip_http_error(
            status,
            "http://192.168.167.25:5005/api/events/abc/clip.mp4",
            r#"{"success":false,"message":"No recordings found for the specified time range"}"#,
        );
        assert!(msg.contains("HTTP 400 Bad Request"));
        assert!(msg.contains("192.168.167.25:5005"));
        assert!(msg.contains("No recordings found"));
    }
}
