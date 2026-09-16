//! Native windows and a window-bound opaque media route for desktop plugins.

use super::bridge::media_access;
use super::media::{MediaError, MediaEvent, MediaService, ReadyMedia};
use super::protocol::HttpMediaKind;
use std::sync::{Arc, OnceLock};
use tauri::{http, Manager};
use tokio::sync::{mpsc, Semaphore};

const LABEL_PREFIX: &str = "plugin-video-";
static RANGE_REQUESTS: OnceLock<Arc<Semaphore>> = OnceLock::new();

pub(crate) fn media_id_for_label(label: &str) -> Option<String> {
    canonical_media_id(label.strip_prefix(LABEL_PREFIX)?)
}

fn canonical_media_id(value: &str) -> Option<String> {
    let id = uuid::Uuid::parse_str(value).ok()?;
    (id.to_string() == value).then(|| value.to_owned())
}

pub(crate) fn is_plugin_preview_label(label: &str) -> bool {
    label
        .strip_prefix("plugin-preview-")
        .and_then(canonical_media_id)
        .is_some()
}

pub(crate) fn is_plugin_video_label(label: &str) -> bool {
    media_id_for_label(label).is_some()
}

pub(crate) fn register(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder.register_asynchronous_uri_scheme_protocol(
        "plugin-media",
        |context, request, responder| {
            let Ok(permit) = RANGE_REQUESTS
                .get_or_init(|| Arc::new(Semaphore::new(4)))
                .clone()
                .try_acquire_owned()
            else {
                responder.respond(empty_response(503));
                return;
            };
            let app = context.app_handle().clone();
            let label = context.webview_label().to_owned();
            tauri::async_runtime::spawn(async move {
                let _permit = permit;
                let Some(id) = request_media_id(&label, request.uri()) else {
                    responder.respond(empty_response(404));
                    return;
                };
                let head = request.method() == http::Method::HEAD;
                if !head && request.method() != http::Method::GET {
                    responder.respond(empty_response(405));
                    return;
                }
                let access_app = app.clone();
                let media = tauri::async_runtime::spawn_blocking(move || media_access(&access_app))
                    .await
                    .ok()
                    .flatten();
                let Some(media) = media else {
                    responder.respond(empty_response(404));
                    return;
                };
                let range = match request.headers().get(http::header::RANGE) {
                    Some(value) => match value.to_str() {
                        Ok(value) => Some(value),
                        Err(_) => {
                            responder.respond(empty_response(400));
                            return;
                        }
                    },
                    None => None,
                };
                let mut result = match media.read_range(&id, &label, range, head).await {
                    Ok(result) => result,
                    Err(error) => {
                        responder.respond(empty_response(error_status(error)));
                        return;
                    }
                };
                // Recheck live policy after asynchronous disk work. The service also
                // rechecks the exact generation, retirement, and requesting window.
                let authorized =
                    tauri::async_runtime::spawn_blocking(move || media_access(&app).is_some())
                        .await
                        .unwrap_or(false);
                if !authorized || !media.is_window_active(&id, &label) {
                    responder.respond(empty_response(404));
                    return;
                }
                let mut response = http::Response::builder()
                    .status(result.status)
                    .header(http::header::CONTENT_TYPE, result.content_type)
                    .header(http::header::CONTENT_LENGTH, result.content_length)
                    .header(http::header::ACCEPT_RANGES, "bytes")
                    .header(http::header::CACHE_CONTROL, "no-store")
                    .header("X-Content-Type-Options", "nosniff");
                if let Some(value) = &result.content_range {
                    response = response.header(http::header::CONTENT_RANGE, value);
                }
                let response = response
                    .body(std::mem::take(&mut result.bytes))
                    .unwrap_or_else(|_| empty_response(500));
                // Retain the bounded read permit through the native handoff. Once
                // submitted, buffering and display timing belong to the webview.
                responder.respond(response);
            });
        },
    )
}

fn request_media_id(label: &str, uri: &http::Uri) -> Option<String> {
    if uri.query().is_some() {
        return None;
    }
    let id = canonical_media_id(uri.path().strip_prefix('/')?)?;
    (media_id_for_label(label).as_deref() == Some(id.as_str())).then_some(id)
}

fn error_status(error: MediaError) -> u16 {
    match error {
        MediaError::Unavailable | MediaError::NotFound => 404,
        MediaError::Busy => 503,
        MediaError::InvalidRequest | MediaError::RangeRequired => 400,
        _ => 500,
    }
}

fn empty_response(status: u16) -> http::Response<Vec<u8>> {
    let mut response = http::Response::new(Vec::new());
    *response.status_mut() =
        http::StatusCode::from_u16(status).unwrap_or(http::StatusCode::INTERNAL_SERVER_ERROR);
    response.headers_mut().insert(
        http::header::CACHE_CONTROL,
        http::HeaderValue::from_static("no-store"),
    );
    response
}

pub(crate) fn forward_events(
    app: tauri::AppHandle,
    media: MediaService,
    mut events: mpsc::Receiver<MediaEvent>,
) {
    // Keep receiving through shutdown: the media service waits for actual
    // Destroyed acknowledgements before releasing its files and lifetime lease.
    tauri::async_runtime::spawn(async move {
        while let Some(event) = events.recv().await {
            let event_app = app.clone();
            let event_media = media.clone();
            match event {
                MediaEvent::Ready(ready) => {
                    let media_id = ready.media_id.clone();
                    let window_label = ready.window_label.clone();
                    let failed = tauri::async_runtime::spawn_blocking(move || {
                        open_window(&event_app, &event_media, ready)
                    })
                    .await;
                    if !matches!(failed, Ok(Ok(()))) {
                        let cleanup_app = app.clone();
                        let cleanup_media = media.clone();
                        let _ = tauri::async_runtime::spawn_blocking(move || {
                            if let Some(window) = cleanup_app.get_webview_window(&window_label) {
                                if window.destroy().is_err() {
                                    cleanup_media.window_cleanup_failed(&window_label);
                                }
                            } else {
                                cleanup_media.window_failed(&media_id);
                            }
                        })
                        .await;
                    }
                }
                MediaEvent::Close { window_label } => {
                    let _ = tauri::async_runtime::spawn_blocking(move || {
                        if let Some(window) = event_app.get_webview_window(&window_label) {
                            if window.destroy().is_err() {
                                log::warn!("Plugin video window cleanup could not complete");
                                event_media.window_cleanup_failed(&window_label);
                            }
                        } else {
                            event_media.window_destroyed(&window_label);
                        }
                    })
                    .await;
                }
            }
        }
    });
}

fn open_window(app: &tauri::AppHandle, media: &MediaService, ready: ReadyMedia) -> Result<(), ()> {
    if media_access(app).is_none() || !media.is_window_active(&ready.media_id, &ready.window_label)
    {
        return Err(());
    }
    if let Some(url) = ready.live_url.clone() {
        return open_preview_window(app, media, ready, url);
    }
    let route = format!(
        "camera-video?pluginMedia={}&pluginMediaKind={}&name={}&error={}",
        ready.media_id,
        if ready.media_kind == HttpMediaKind::Video {
            "video"
        } else {
            "image"
        },
        crate::percent_encode_query(&ready.title),
        if ready.error.is_some() {
            "download"
        } else {
            ""
        },
    );
    let development_url = tauri::is_dev()
        .then(|| {
            app.config()
                .build
                .dev_url
                .as_ref()
                .and_then(|url| url.join("camera-video").ok())
        })
        .flatten();
    let builder = tauri::WebviewWindowBuilder::new(
        app,
        &ready.window_label,
        tauri::WebviewUrl::App(route.into()),
    )
    .title(&ready.title)
    .inner_size(crate::CAMERA_VIDEO_WINDOW_W, crate::CAMERA_VIDEO_WINDOW_H)
    .resizable(true)
    .visible(false)
    .decorations(false)
    .focused(false)
    .on_navigation(move |url| navigation_allowed(url, development_url.as_ref()));
    #[cfg(target_os = "macos")]
    let builder = builder
        .hidden_title(true)
        .title_bar_style(tauri::TitleBarStyle::Transparent);
    // The explicit harness must not reuse WKWebView's persistent default store.
    // Production setup leaves this native-only flag false, including feature builds.
    #[cfg(feature = "native-media-smoke")]
    let builder = builder.incognito(super::bridge::native_media_smoke_session(app));
    let window = builder.build().map_err(|_| ())?;
    let event_app = app.clone();
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            crate::reflow_camera_video_windows(&event_app);
        }
    });
    crate::apply_camera_video_window_defaults(app, &window);
    if media_access(app).is_none() || !media.window_ready(&ready.media_id, &ready.window_label) {
        return Err(());
    }
    show_without_focus(&window, media, &ready.media_id)?;
    // Native visibility is asynchronous. A revocation after the final check
    // still revokes media access immediately and queues destruction of this window.
    Ok(())
}

fn preview_navigation_allowed(candidate: &reqwest::Url, expected: &reqwest::Url) -> bool {
    candidate == expected
}

fn open_preview_window(
    app: &tauri::AppHandle,
    media: &MediaService,
    ready: ReadyMedia,
    url: reqwest::Url,
) -> Result<(), ()> {
    if !is_plugin_preview_label(&ready.window_label) {
        return Err(());
    }
    let expected = url.clone();
    let window = tauri::WebviewWindowBuilder::new(
        app,
        &ready.window_label,
        tauri::WebviewUrl::External(url),
    )
    .title(format!("{} — Live", ready.title))
    .inner_size(crate::CAMERA_VIDEO_WINDOW_W, crate::CAMERA_VIDEO_WINDOW_H)
    .resizable(true)
    .visible(false)
    .focused(false)
    .incognito(true)
    // A remote preview receives no capabilities, new windows, or custom IPC.
    // Redirects and navigation remain bound to the exact startup-granted URL.
    .on_navigation(move |candidate| preview_navigation_allowed(candidate, &expected))
    .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
    .build()
    .map_err(|_| ())?;
    let event_app = app.clone();
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            crate::reflow_camera_video_windows(&event_app);
        }
    });
    crate::apply_camera_video_window_defaults(app, &window);
    if media_access(app).is_none() || !media.window_ready(&ready.media_id, &ready.window_label) {
        return Err(());
    }
    show_without_focus(&window, media, &ready.media_id)
}

#[cfg(not(target_os = "macos"))]
fn show_without_focus(
    window: &tauri::WebviewWindow,
    media: &MediaService,
    media_id: &str,
) -> Result<(), ()> {
    if !media.is_window_active(media_id, window.label()) {
        return Err(());
    }
    window.show().map_err(|_| ())
}

#[cfg(target_os = "macos")]
fn show_without_focus(
    window: &tauri::WebviewWindow,
    media: &MediaService,
    media_id: &str,
) -> Result<(), ()> {
    // Tao's macOS show() calls makeKeyAndOrderFront even when the builder used
    // focused(false). Suppress key eligibility only during that native call;
    // restore it immediately so later user clicks and dragging still work.
    // Keep these operations together on the main thread, without refocusing a
    // previous window (which could belong to another application by this point).
    let showing = window.clone();
    let media = media.clone();
    let media_id = media_id.to_owned();
    let (sent, received) = std::sync::mpsc::sync_channel(1);
    window
        .run_on_main_thread(move || {
            // A delayed UI callback cannot revive already-retired media.
            if !media.is_window_active(&media_id, showing.label()) {
                let _ = sent.send(Err(()));
                return;
            }
            let shown = showing.set_focusable(false).and_then(|_| showing.show());
            let restored = showing.set_focusable(true);
            let _ = sent.send(shown.and(restored).map_err(|_| ()));
        })
        .map_err(|_| ())?;
    received.recv().map_err(|_| ())?
}

fn navigation_allowed(url: &reqwest::Url, development_url: Option<&reqwest::Url>) -> bool {
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    let packaged = url.port().is_none()
        && url.path() == "/camera-video"
        && ((url.scheme() == "tauri" && url.host_str() == Some("localhost"))
            || (matches!(url.scheme(), "http" | "https")
                && url.host_str() == Some("tauri.localhost")));
    packaged
        || development_url.is_some_and(|expected| {
            url.origin() == expected.origin() && url.path() == expected.path()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_routes_require_the_exact_owning_window_and_canonical_id() {
        let id = "acdb88b6-453a-4f88-8d52-5b902441fe4c";
        let label = format!("plugin-video-{id}");
        for url in [
            format!("plugin-media://localhost/{id}"),
            format!("http://plugin-media.localhost/{id}"),
        ] {
            assert_eq!(
                request_media_id(&label, &url.parse().unwrap()).as_deref(),
                Some(id)
            );
        }
        for label in ["main", "config", "camera-video-1", "plugin-video-other"] {
            assert!(request_media_id(label, &format!("/{id}").parse().unwrap()).is_none());
        }
        for path in [
            format!("/{id}?path=other"),
            format!("/{id}/other"),
            "/../clip.mp4".into(),
            format!("/{}", id.to_uppercase()),
        ] {
            assert!(request_media_id(&label, &path.parse().unwrap()).is_none());
        }
    }

    #[test]
    fn preview_identity_has_no_opaque_media_route_or_clickable_live_namespace() {
        let id = "acdb88b6-453a-4f88-8d52-5b902441fe4c";
        let label = format!("plugin-preview-{id}");
        assert!(is_plugin_preview_label(&label));
        assert!(!is_plugin_video_label(&label));
        assert!(media_id_for_label(&label).is_none());
        assert!(request_media_id(&label, &format!("/{id}").parse().unwrap()).is_none());
        for rejected in [
            format!("plugin-live-{id}"),
            format!("plugin-preview-{}", id.to_uppercase()),
            "plugin-preview-other".into(),
        ] {
            assert!(!is_plugin_preview_label(&rejected));
        }
    }

    #[test]
    fn preview_navigation_is_bound_to_the_exact_granted_url() {
        let expected = "https://camera.invalid/prefix/api/front?fps=2&height=360"
            .parse()
            .unwrap();
        assert!(preview_navigation_allowed(&expected, &expected));
        for value in [
            "https://camera.invalid/prefix/api/other?fps=2&height=360",
            "https://camera.invalid/prefix/api/front?fps=30&height=360",
            "https://camera.invalid/prefix/api/front?fps=2&height=360#other",
            "https://other.invalid/prefix/api/front?fps=2&height=360",
            "tauri://localhost/config",
            "about:blank",
        ] {
            assert!(!preview_navigation_allowed(
                &value.parse().unwrap(),
                &expected
            ));
        }
    }

    #[test]
    fn video_navigation_allows_only_packaged_or_active_development_view() {
        let dev = reqwest::Url::parse("http://localhost:1420/camera-video").unwrap();
        assert!(!navigation_allowed(&dev, None));
        assert!(navigation_allowed(&dev, Some(&dev)));
        for value in [
            "tauri://localhost/camera-video",
            "http://tauri.localhost/camera-video",
            "https://tauri.localhost/camera-video?pluginMedia=id",
        ] {
            assert!(navigation_allowed(&value.parse().unwrap(), None));
        }
        for value in [
            "tauri://other/camera-video",
            "ftp://tauri.localhost/camera-video",
            "https://tauri.localhost:9000/camera-video",
            "http://localhost:1421/camera-video",
            "http://localhost:1420/config",
            "https://remote.invalid/camera-video",
            "https://user@tauri.localhost/camera-video",
        ] {
            assert!(!navigation_allowed(&value.parse().unwrap(), Some(&dev)));
        }
    }
}
