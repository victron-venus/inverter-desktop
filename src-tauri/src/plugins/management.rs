//! Single-package intent is persisted before its installed state changes.

use super::{run_owned, PackageApplication};
use crate::plugin_config::DesktopPluginConfig;
use crate::plugins::protocol::validate_plugin_id;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PluginDesiredChange {
    Enabled(bool),
    Removed,
}

impl PluginDesiredChange {
    /// Change only the selected declaration, preserving order and all other pins.
    pub(crate) fn apply(self, declarations: &mut Vec<DesktopPluginConfig>, id: &str) -> bool {
        match self {
            Self::Enabled(enabled) => {
                let mut changed = false;
                for declaration in declarations
                    .iter_mut()
                    .filter(|entry| entry.plugin_id == id)
                {
                    changed |= declaration.enabled != enabled;
                    declaration.enabled = enabled;
                }
                changed
            }
            Self::Removed => {
                let before = declarations.len();
                declarations.retain(|entry| entry.plugin_id != id);
                before != declarations.len()
            }
        }
    }
}

impl PackageApplication {
    /// Join restoration's gate so a previously cloned declaration cannot undo
    /// the user's persisted intent. The owned task survives IPC cancellation.
    pub(crate) async fn set_enabled_with_config<F>(
        &self,
        id: &str,
        enabled: bool,
        epoch: u64,
        persist: F,
    ) -> Result<(), String>
    where
        F: FnOnce(&str, PluginDesiredChange, &PackageApplication) -> Result<(), String>
            + Send
            + 'static,
    {
        let activity = self.begin_activity();
        self.check_epoch(epoch)?;
        validate_plugin_id(id)?;
        let id = id.to_owned();
        let service = self.clone();
        run_owned(activity, async move {
            let _operation = service.0.reconciliation.operation.lock().await;
            service.check_epoch(epoch)?;
            let manager = service.manager()?;
            if !manager
                .list()
                .await?
                .iter()
                .any(|entry| entry.plugin_id == id)
            {
                return Err("plugin is not installed".into());
            }
            let result = async {
                service.check_epoch(epoch)?;
                persist(&id, PluginDesiredChange::Enabled(enabled), &service)?;
                service.prepare_mutation(&id, epoch)?;
                if enabled {
                    manager.enable_in_epoch(&id, epoch).await
                } else {
                    manager.disable_in_epoch(&id, epoch).await
                }
                .map(|_| ())
            }
            .await;
            service.record_result(&id, epoch, &result);
            service.configured_status(
                &id,
                epoch,
                if result.is_err() {
                    "failed"
                } else if enabled {
                    "ready"
                } else {
                    "disabled"
                },
                result.as_ref().err().cloned(),
            );
            result
        })
        .await
    }

    /// Disable durably before releasing declaration ownership. If pin removal
    /// or settings cleanup fails, a remaining installed record cannot restart.
    pub(crate) async fn uninstall_with_config<F>(
        &self,
        id: &str,
        delete_settings: bool,
        epoch: u64,
        mut persist: F,
    ) -> Result<(), String>
    where
        F: FnMut(&str, PluginDesiredChange, &PackageApplication) -> Result<(), String>
            + Send
            + 'static,
    {
        let activity = self.begin_activity();
        self.check_epoch(epoch)?;
        validate_plugin_id(id)?;
        let id = id.to_owned();
        let service = self.clone();
        run_owned(activity, async move {
            let _operation = service.0.reconciliation.operation.lock().await;
            service.check_epoch(epoch)?;
            let manager = service.manager()?;
            let store = service.settings_store()?;
            if !manager
                .list()
                .await?
                .iter()
                .any(|entry| entry.plugin_id == id)
            {
                return Err("plugin is not installed".into());
            }
            let result = async {
                service.check_epoch(epoch)?;
                persist(&id, PluginDesiredChange::Enabled(false), &service)?;
                service.prepare_mutation(&id, epoch)?;
                manager.disable_in_epoch(&id, epoch).await?;
                service.configured_status(&id, epoch, "disabled", None);
                service.check_epoch(epoch)?;
                persist(&id, PluginDesiredChange::Removed, &service)?;
                service.check_epoch(epoch)?;
                let cleanup = if delete_settings {
                    let settings_id = id.clone();
                    Some(Box::new(move || store.remove(&settings_id))
                        as Box<dyn FnOnce() -> Result<(), String> + Send>)
                } else {
                    None
                };
                manager
                    .remove_with_settings_in_epoch(&id, epoch, cleanup)
                    .await
            }
            .await;
            service.record_result(&id, epoch, &result);
            result
        })
        .await
    }
}
