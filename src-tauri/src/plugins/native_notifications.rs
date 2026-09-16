//! Native submission while the worker's authority guard remains held.

use super::runtime::DesktopNotification;

#[cfg(any(not(target_os = "macos"), test))]
fn notification_body(text: &str, markup: bool) -> String {
    if markup {
        // Freedesktop bodies accept markup; worker text stays literal.
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    } else {
        text.to_owned()
    }
}

#[cfg(not(target_os = "macos"))]
fn native_notification(
    app: &tauri::AppHandle,
    message: &DesktopNotification,
) -> Result<notify_rust::Notification, ()> {
    validate_fallback(message)?;
    let mut notification = notify_rust::Notification::new();
    notification
        .summary(&message.title)
        .body(&notification_body(&message.body, cfg!(unix)))
        .appname(
            app.config()
                .product_name
                .as_deref()
                .unwrap_or("Inverter Desktop"),
        )
        .auto_icon();
    #[cfg(windows)]
    {
        // Match Tauri's existing backend: installed apps have a registered ID.
        if tauri::utils::platform::current_exe()
            .ok()
            .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
            .is_some_and(|directory| {
                !directory.ends_with("target/debug") && !directory.ends_with("target/release")
            })
        {
            notification.app_id(&app.config().identifier);
        }
    }
    Ok(notification)
}

#[cfg(any(not(target_os = "macos"), test))]
fn validate_fallback(message: &DesktopNotification) -> Result<(), ()> {
    let Some(request) = &message.live_view else {
        return Ok(());
    };
    // These backends currently display an ordinary notification. The private
    // destination remains native, and its authority must still be current.
    if !request.lease.is_active()
        || request.lease.plugin_id() != message.plugin_id
        || super::protocol::validate_live_id(&request.id).is_err()
        || !matches!(request.url.scheme(), "http" | "https")
        || request.url.host_str().is_none()
        || !request.url.username().is_empty()
        || request.url.password().is_some()
    {
        return Err(());
    }
    Ok(())
}

#[cfg(any(all(unix, not(target_os = "macos")), test))]
async fn bounded_submission(
    submission: impl std::future::Future<Output = Result<(), ()>>,
    deadline: std::time::Duration,
) -> Result<(), ()> {
    tokio::time::timeout(deadline, submission)
        .await
        .map_err(|_| ())?
}

#[cfg(all(unix, not(target_os = "macos")))]
pub(super) fn submit(app: &tauri::AppHandle, message: &DesktopNotification) -> Result<(), ()> {
    let notification = native_notification(app, message)?;
    // Called on the single blocking dispatcher thread. Dropping the timed-out
    // future cancels our wait; no detached task can submit another notification.
    tauri::async_runtime::block_on(bounded_submission(
        async { notification.show_async().await.map(|_| ()).map_err(|_| ()) },
        std::time::Duration::from_secs(2),
    ))
}

#[cfg(windows)]
pub(super) fn submit(app: &tauri::AppHandle, message: &DesktopNotification) -> Result<(), ()> {
    native_notification(app, message)?
        .show()
        .map(|_| ())
        .map_err(|_| ())
}

#[cfg(target_os = "macos")]
fn application_ready(
    result: mac_notification_sys::error::NotificationResult<()>,
) -> Result<(), ()> {
    use mac_notification_sys::error::{ApplicationError, Error};

    // This backend initializes once for the entire application, including core
    // notifications. An existing initialization is also valid for this plugin.
    match result {
        Ok(()) | Err(Error::Application(ApplicationError::AlreadySet(_))) => Ok(()),
        Err(_) => Err(()),
    }
}

#[cfg(target_os = "macos")]
pub(super) fn submit(app: &tauri::AppHandle, message: &DesktopNotification) -> Result<(), ()> {
    if message.live_view.is_some() {
        return super::live_view::submit(app, message);
    }
    application_ready(mac_notification_sys::set_application(if tauri::is_dev() {
        "com.apple.Terminal"
    } else {
        &app.config().identifier
    }))?;
    // Use the same backend directly: notify-rust logs the complete payload in
    // its handle destructor and discards submission errors. Asynchronous here
    // means no interaction wait; submission still occurs before send returns,
    // with this backend's bounded two-second delivery confirmation. That wait
    // may time out without returning an error: success is best-effort native
    // submission, never proof that the notification was displayed.
    mac_notification_sys::Notification::default()
        .title(&message.title)
        .message(&message.body)
        .asynchronous(true)
        .send()
        .map(|_| ())
        .map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::{bounded_submission, notification_body};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    #[test]
    fn ordinary_fallback_requires_a_current_well_formed_private_action() {
        use crate::plugins::generation::GenerationLease;
        use crate::plugins::runtime::{DesktopNotification, LiveViewRequest};

        let lease = GenerationLease::new("test.camera".into(), 1, 1);
        let mut message = DesktopNotification {
            plugin_id: "test.camera".into(),
            id: "motion".into(),
            title: "Camera".into(),
            body: "Motion started".into(),
            live_view: None,
        };
        assert_eq!(super::validate_fallback(&message), Ok(()));
        message.live_view = Some(LiveViewRequest {
            lease: lease.clone(),
            id: "front-camera".into(),
            url: reqwest::Url::parse("https://camera.invalid/live?private=value#camera").unwrap(),
        });
        assert_eq!(super::validate_fallback(&message), Ok(()));
        for invalid in ["", "camera/+", "camera\n"] {
            message.live_view.as_mut().unwrap().id = invalid.into();
            assert_eq!(super::validate_fallback(&message), Err(()));
        }
        message.live_view.as_mut().unwrap().id = "front-camera".into();
        for invalid in ["file:///tmp/camera", "https://user:secret@camera.invalid/"] {
            message.live_view.as_mut().unwrap().url = reqwest::Url::parse(invalid).unwrap();
            assert_eq!(super::validate_fallback(&message), Err(()));
        }
        message.live_view.as_mut().unwrap().url =
            reqwest::Url::parse("http://camera.invalid/").unwrap();
        message.plugin_id = "other.camera".into();
        assert_eq!(super::validate_fallback(&message), Err(()));
        message.plugin_id = "test.camera".into();
        lease.revoke();
        assert_eq!(super::validate_fallback(&message), Err(()));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn mac_application_setup_accepts_existing_core_or_plugin_initialization() {
        use mac_notification_sys::error::{ApplicationError, Error, NotificationError};

        // First plugin delivery initializes successfully; later deliveries and
        // the core-first path both receive AlreadySet from the shared backend.
        assert_eq!(super::application_ready(Ok(())), Ok(()));
        for _ in 0..2 {
            assert_eq!(
                super::application_ready(Err(Error::Application(ApplicationError::AlreadySet(
                    "com.apple.Terminal".into()
                )))),
                Ok(())
            );
        }
        assert_eq!(
            super::application_ready(Err(Error::Application(ApplicationError::CouldNotSet(
                "invalid.application".into()
            )))),
            Err(())
        );
        assert_eq!(
            super::application_ready(Err(Error::Notification(NotificationError::UnableToDeliver))),
            Err(())
        );
    }

    #[test]
    fn worker_notification_bodies_remain_literal_on_markup_backends() {
        let body = "<b>Motion</b> & camera > garage";
        assert_eq!(
            notification_body(body, true),
            "&lt;b&gt;Motion&lt;/b&gt; &amp; camera &gt; garage"
        );
        assert_eq!(notification_body(body, false), body);
        assert_eq!(notification_body("&lt;", true), "&amp;lt;");
    }

    #[tokio::test]
    async fn timeout_drops_an_unresponsive_submission_instead_of_detaching_it() {
        struct Pending<'a>(&'a AtomicBool);
        impl Drop for Pending<'_> {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }
        let dropped = AtomicBool::new(false);
        let pending = async {
            let _pending = Pending(&dropped);
            std::future::pending::<Result<(), ()>>().await
        };
        assert_eq!(
            bounded_submission(pending, Duration::from_millis(5)).await,
            Err(())
        );
        assert!(dropped.load(Ordering::Acquire));
        assert_eq!(
            bounded_submission(async { Ok(()) }, Duration::from_secs(1)).await,
            Ok(())
        );
    }
}
