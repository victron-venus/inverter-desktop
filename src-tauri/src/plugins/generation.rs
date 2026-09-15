//! Revocable native ownership for one actual worker process. Only its opaque
//! identity enters dashboard snapshots to bind clicks to the displayed instance.
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

struct GenerationState {
    plugin_id: String,
    epoch: u64,
    generation: u64,
    instance_id: uuid::Uuid,
    active: Mutex<bool>,
    cancelled: watch::Sender<bool>,
}

#[derive(Clone)]
pub(crate) struct GenerationLease(Arc<GenerationState>);

impl GenerationLease {
    pub(super) fn new(plugin_id: String, epoch: u64, generation: u64) -> Self {
        Self(Arc::new(GenerationState {
            plugin_id,
            epoch,
            generation,
            instance_id: uuid::Uuid::new_v4(),
            active: Mutex::new(true),
            cancelled: watch::channel(false).0,
        }))
    }

    pub fn plugin_id(&self) -> &str {
        &self.0.plugin_id
    }
    pub fn epoch(&self) -> u64 {
        self.0.epoch
    }
    pub fn generation(&self) -> u64 {
        self.0.generation
    }
    pub fn instance_id(&self) -> uuid::Uuid {
        self.0.instance_id
    }
    pub fn is_active(&self) -> bool {
        *self
            .0
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    pub async fn cancelled(&self) {
        let mut receiver = self.0.cancelled.subscribe();
        let _ = receiver.wait_for(|cancelled| *cancelled).await;
    }

    /// Only short native metadata commits. Never call host/auth, perform I/O,
    /// await, or dispatch native UI while holding this guard. Runtime lock order
    /// is authority -> generation; service code must never reverse that order.
    pub fn commit_if_active<T>(&self, commit: impl FnOnce() -> T) -> Result<T, String> {
        let active = self
            .0
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !*active {
            return Err("Plugin generation was revoked".into());
        }
        Ok(commit())
    }

    pub(super) fn revoke(&self) {
        let mut active = self
            .0
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        *active = false;
        self.0.cancelled.send_replace(true);
    }
}

/// A supervisor must hold this until process termination, including unwind/drop.
pub(super) struct RevokeOnDrop(pub GenerationLease);
impl Drop for RevokeOnDrop {
    fn drop(&mut self) {
        self.0.revoke();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn generation_identity_and_revocation_survive_counter_reuse_and_guard_drop() {
        let first = GenerationLease::new("test.worker".into(), 1, 1);
        let second = GenerationLease::new("test.worker".into(), 1, 1);
        assert_ne!(first.instance_id(), second.instance_id());
        drop(RevokeOnDrop(first.clone()));
        first.cancelled().await;
        assert!(!first.is_active());
        assert!(first.commit_if_active(|| panic!("revoked commit")).is_err());
        assert!(second.commit_if_active(|| ()).is_ok());
    }
}
