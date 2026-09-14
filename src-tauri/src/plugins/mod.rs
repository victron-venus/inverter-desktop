//! Desktop worker host for trusted native integrations.
//!
//! The webview can read contributions and invoke advertised actions. It cannot
//! provide executable paths or trust keys. Authenticated desktop settings can
//! review and manage signed packages through the native application service;
//! workers start only from verified installed payloads under native policy.

pub(crate) mod application;
pub(crate) mod bridge;
pub(crate) mod generation;
mod http_video;
pub mod installer;
pub(crate) mod media;
pub(crate) mod media_windows;
#[cfg(feature = "native-media-smoke")]
pub(crate) mod native_media_smoke;
mod native_notifications;
pub mod package;
pub mod packaging;
pub mod protocol;
pub mod publishers;
pub mod runtime;
pub(crate) mod settings;
pub(crate) mod settings_store;

#[cfg(test)]
mod frigate_integration_tests;
#[cfg(test)]
mod home_assistant_integration_tests;
