//! Group membership comes only from verified installed manifests.

use super::{run_owned, PackageApplication};
use crate::plugins::installer::InstalledPluginDetails;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct PluginGroupSnapshot {
    pub id: String,
    pub title: String,
    pub icon: String,
    /// Mixed groups remain active until all installed members are disabled.
    pub enabled: bool,
    pub plugin_ids: Vec<String>,
}

pub(super) fn project(details: &[InstalledPluginDetails]) -> Vec<PluginGroupSnapshot> {
    let mut groups = BTreeMap::<String, PluginGroupSnapshot>::new();
    for detail in details {
        let Some(group) = detail
            .manifest
            .as_ref()
            .and_then(|manifest| manifest.group.as_ref())
        else {
            continue;
        };
        let entry = groups
            .entry(group.id.clone())
            .or_insert_with(|| PluginGroupSnapshot {
                id: group.id.clone(),
                title: group.title.clone(),
                icon: group.icon.clone(),
                enabled: false,
                plugin_ids: Vec::new(),
            });
        entry.enabled |= detail.record.enabled;
        entry.plugin_ids.push(detail.record.plugin_id.clone());
    }
    for group in groups.values_mut() {
        group.plugin_ids.sort();
    }
    groups.into_values().collect()
}

impl PackageApplication {
    pub(crate) async fn groups(&self, epoch: u64) -> Result<Vec<PluginGroupSnapshot>, String> {
        Ok(self.snapshot(epoch).await?.groups)
    }

    /// Persist desired state under the caller's config save gate before changing
    /// inventory. The owned task and reconciliation gate outlive IPC cancellation.
    pub(crate) async fn set_group_enabled<F>(
        &self,
        group_id: String,
        enabled: bool,
        epoch: u64,
        persist: F,
    ) -> Result<(), String>
    where
        F: FnOnce(&[String], &PackageApplication) -> Result<(), String> + Send + 'static,
    {
        let activity = self.begin_activity();
        self.check_epoch(epoch)?;
        let service = self.clone();
        run_owned(activity, async move {
            let _operation = service.0.reconciliation.operation.lock().await;
            service.check_epoch(epoch)?;
            let manager = service.manager()?;
            let details = manager.list_details_in_epoch(epoch).await?;
            let group = project(&details)
                .into_iter()
                .find(|group| group.id == group_id)
                .ok_or("Plugin group has no verified installed members")?;
            persist(&group.plugin_ids, &service)?;
            service.check_epoch(epoch)?;
            let mut first_error = None;
            for id in group.plugin_ids {
                service.prepare_mutation(&id, epoch)?;
                let result = if enabled {
                    manager.enable_in_epoch(&id, epoch).await
                } else {
                    manager.disable_in_epoch(&id, epoch).await
                }
                .map(|_| ());
                service.record_result(&id, epoch, &result);
                service.configured_status(
                    &id,
                    epoch,
                    if result.is_err() {
                        "error"
                    } else if enabled {
                        "ready"
                    } else {
                        "disabled"
                    },
                    result.as_ref().err().cloned(),
                );
                if let Err(error) = result {
                    first_error.get_or_insert(error);
                }
            }
            service.check_epoch(epoch)?;
            first_error.map_or(Ok(()), Err)
        })
        .await
    }
}
