use super::{load_config, FullConfig, CAMERA_VIDEO_LABEL_PREFIX};
use log::{info, warn};
use std::time::Duration;
use tauri::Manager;

const CAMERA_CLIP_SUBDIR: &str = "inverter-desktop-camera";

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

    let client = reqwest::Client::builder()
        // A camera redirect must not carry HA credentials to another origin.
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;

    let auth = camera_download_bearer_token(app, url_trim);

    // Frigate recording segments can take ~10s after `end`+`has_clip`; cover that
    // window plus brief network blips. Kerberos URLs rarely hit these statuses.
    const MAX_ATTEMPTS: u32 = 8;
    const BACKOFF_MS: [u64; 7] = [1000, 2000, 3000, 4000, 5000, 5000, 5000];
    let mut last_err = String::new();

    for attempt in 1..=MAX_ATTEMPTS {
        let mut req = client.get(url_trim);
        if let Some(ref bearer) = auth {
            req = req.header(reqwest::header::AUTHORIZATION, bearer);
        }
        let response = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                last_err = format!("Failed to download camera clip: {e} ({video_url})");
                if attempt < MAX_ATTEMPTS {
                    let delay = BACKOFF_MS[(attempt as usize - 1).min(BACKOFF_MS.len() - 1)];
                    warn!(
                        "Camera clip download attempt {attempt}/{MAX_ATTEMPTS} failed (send/connect): {e}; retrying in {delay}ms url={video_url}"
                    );
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                    continue;
                }
                return Err(last_err);
            }
        };

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            last_err = format_camera_clip_http_error(status, video_url, &body);
            if attempt < MAX_ATTEMPTS && camera_clip_http_status_retryable(status) {
                let delay = BACKOFF_MS[(attempt as usize - 1).min(BACKOFF_MS.len() - 1)];
                warn!(
                    "Camera clip download attempt {attempt}/{MAX_ATTEMPTS} got HTTP {status}; retrying in {delay}ms url={video_url} body={}",
                    short_http_body_snippet(&body)
                );
                tokio::time::sleep(Duration::from_millis(delay)).await;
                continue;
            }
            return Err(last_err);
        }

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let ext = camera_media_extension(content_type.as_deref(), url_trim);

        let bytes = match response.bytes().await {
            Ok(b) => b,
            Err(e) => {
                last_err = format!("Failed to read camera clip body: {e} ({video_url})");
                if attempt < MAX_ATTEMPTS {
                    let delay = BACKOFF_MS[(attempt as usize - 1).min(BACKOFF_MS.len() - 1)];
                    warn!(
                        "Camera clip download attempt {attempt}/{MAX_ATTEMPTS} failed reading body: {e}; retrying in {delay}ms url={video_url}"
                    );
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                    continue;
                }
                return Err(last_err);
            }
        };

        if bytes.is_empty() {
            last_err = format!("Downloaded camera clip is empty ({video_url})");
            if attempt < MAX_ATTEMPTS {
                let delay = BACKOFF_MS[(attempt as usize - 1).min(BACKOFF_MS.len() - 1)];
                warn!(
                    "Camera clip download attempt {attempt}/{MAX_ATTEMPTS} got empty body; retrying in {delay}ms url={video_url}"
                );
                tokio::time::sleep(Duration::from_millis(delay)).await;
                continue;
            }
            return Err(last_err);
        }

        if attempt > 1 {
            info!(
                "Camera clip download succeeded on attempt {attempt}/{MAX_ATTEMPTS} url={video_url}"
            );
        }

        let file_name = format!("clip-{}.{ext}", uuid::Uuid::new_v4());
        let dest = clip_dir.join(file_name);
        std::fs::write(&dest, &bytes)
            .map_err(|e| format!("Failed to write camera clip to temp file: {e}"))?;

        return Ok(dest);
    }

    Err(last_err)
}

#[cfg(test)]
mod camera_clip_download_tests {
    use super::*;

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
