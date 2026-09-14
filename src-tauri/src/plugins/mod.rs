//! Desktop worker host for trusted native integrations.
//!
//! The webview can read contributions and invoke advertised actions. It cannot
//! provide executable paths or start workers. Signed package APIs are available
//! to trusted native callers; the shipped application does not load packages yet.

pub(crate) mod bridge;
pub mod installer;
pub mod package;
pub mod packaging;
pub mod protocol;
pub mod publishers;
pub mod runtime;
