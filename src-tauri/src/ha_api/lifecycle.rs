//! Configuration revisions interrupt long-lived HA connections immediately.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::LazyLock;
use tokio::sync::watch;

static CONFIG_REVISION: LazyLock<watch::Sender<u64>> = LazyLock::new(|| watch::channel(0).0);
static CONNECTED: AtomicBool = AtomicBool::new(false);

pub fn config_changes() -> watch::Receiver<u64> {
    CONFIG_REVISION.subscribe()
}
pub fn notify_config_changed() {
    set_connection_status(false);
    CONFIG_REVISION.send_modify(|revision| *revision = revision.wrapping_add(1));
}
pub fn connection_status() -> bool {
    CONNECTED.load(Ordering::Acquire)
}
pub fn set_connection_status(connected: bool) {
    CONNECTED.store(connected, Ordering::Release);
}
pub(super) fn current_revision() -> u64 {
    *CONFIG_REVISION.borrow()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn configuration_change_wakes_subscription_and_resets_status() {
        let mut changes = config_changes();
        let before = *changes.borrow_and_update();
        set_connection_status(true);
        notify_config_changed();
        tokio::time::timeout(std::time::Duration::from_secs(1), changes.changed())
            .await
            .unwrap()
            .unwrap();
        assert_ne!(*changes.borrow(), before);
        assert!(!connection_status());
    }
}
