//! Desktop worker host for trusted native integrations.
//!
//! The webview can read contributions and invoke advertised actions. It cannot
//! provide executable paths or start workers. Signed package installation is a
//! separate delivery stage; the shipped host initially has no registered workers.

pub(crate) mod bridge;
pub mod protocol;
pub mod runtime;
