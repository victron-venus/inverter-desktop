//! Desktop worker host for trusted native integrations.
//!
//! The webview can read contributions and invoke advertised actions. It cannot
//! provide executable paths or trust keys. Authenticated desktop settings can
//! review and manage signed packages through the native application service;
//! workers start only from verified installed payloads under native policy.

pub(crate) mod application;
pub(crate) mod bridge;
pub mod installer;
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
