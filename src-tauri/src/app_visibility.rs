//! Shared visibility state for core telemetry and optional UI integrations.

use std::sync::atomic::AtomicBool;

pub(crate) static WINDOW_HIDDEN: AtomicBool = AtomicBool::new(false);
