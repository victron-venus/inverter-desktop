//! Desktop worker host for trusted native integrations.
//!
//! The webview can read contributions and invoke advertised actions. It cannot
//! provide executable paths or trust keys. Authenticated desktop settings can
//! review and manage signed packages through the native application service;
//! workers start only from verified installed payloads under native policy.

pub(crate) mod application;
pub(crate) mod bridge;
pub mod installer;
pub mod package;
pub mod packaging;
pub mod protocol;
pub mod publishers;
pub mod runtime;
