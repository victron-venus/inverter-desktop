//! Application-owned package store and review consent, independent of Tauri UI.

use super::installer::{InstalledPluginDetails, PackageManager};
use super::package::{TrustStore, VerifiedPackage};
use super::protocol::{validate_plugin_id, PluginPermission};
use super::runtime::{PluginHost, PluginSnapshot};
use serde::Serialize;
use std::collections::BTreeMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::Notify;

const PREVIEW_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_FAILURES: usize = 8;

struct SessionActivity(Arc<ApplicationInner>);

impl Drop for SessionActivity {
    fn drop(&mut self) {
        self.0.authorized_work.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Serialize)]
pub(crate) struct ManagerSnapshot {
    pub ready: bool,
    pub error: Option<String>,
    pub installation_available: bool,
    pub target: String,
    pub plugins: Vec<ManagedPlugin>,
}

#[derive(Serialize)]
pub(crate) struct ManagedPlugin {
    plugin_id: String,
    version: String,
    rollback_version: Option<String>,
    enabled: bool,
    permissions: Vec<PluginPermission>,
    runtime: Option<PluginSnapshot>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PackagePreview {
    pub token: String,
    pub plugin_id: String,
    version: String,
    current_version: Option<String>,
    permissions: Vec<PluginPermission>,
    target: String,
    host_api: String,
    publisher_key_id: String,
    restart_required: bool,
}

struct PendingReview {
    token: String,
    owner: String,
    epoch: u64,
    created: Instant,
    package: Option<VerifiedPackage>,
}

impl PendingReview {
    fn matches(&self, token: &str, owner: &str, epoch: u64) -> bool {
        self.token == token
            && self.owner == owner
            && self.epoch == epoch
            && self.created.elapsed() < PREVIEW_TTL
    }
}

enum StoreStatus {
    Opening,
    Ready(PackageManager),
    Failed(String),
}

struct ApplicationInner {
    host: PluginHost,
    target: String,
    installation_available: bool,
    store: Mutex<StoreStatus>,
    initialized: Notify,
    pending: Mutex<Option<PendingReview>>,
    failures: Mutex<BTreeMap<String, String>>,
    closing: AtomicBool,
    authorized_work: AtomicUsize,
    metadata_revision: AtomicU64,
    metadata: tokio::sync::Mutex<Option<(u64, Vec<InstalledPluginDetails>)>>,
    changed: Arc<dyn Fn() + Send + Sync>,
}

#[derive(Clone)]
pub(crate) struct PackageApplication(Arc<ApplicationInner>);

impl PackageApplication {
    pub(crate) fn new(
        host: PluginHost,
        target: String,
        installation_available: bool,
        changed: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        Self(Arc::new(ApplicationInner {
            host,
            target,
            installation_available,
            store: Mutex::new(StoreStatus::Opening),
            initialized: Notify::new(),
            pending: Mutex::new(None),
            failures: Mutex::new(BTreeMap::new()),
            closing: AtomicBool::new(false),
            authorized_work: AtomicUsize::new(0),
            metadata_revision: AtomicU64::new(0),
            metadata: tokio::sync::Mutex::new(None),
            changed,
        }))
    }

    /// Called once by application setup. Metadata recovery never executes a worker.
    pub(crate) async fn initialize(
        &self,
        root: Result<PathBuf, String>,
        trust: Result<TrustStore, String>,
    ) {
        let result = async {
            let root = root?;
            let trust = trust?;
            let parent = root.parent().ok_or("Invalid plugin store parent")?;
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            PackageManager::open(root, self.0.target.clone(), trust, self.0.host.clone()).await
        }
        .await;
        *self
            .0
            .store
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = match result {
            Ok(manager) => StoreStatus::Ready(manager),
            Err(error) => StoreStatus::Failed(error),
        };
        self.0.initialized.notify_waiters();
        (self.0.changed)();
    }

    fn check_epoch(&self, epoch: u64) -> Result<(), String> {
        if self.0.closing.load(Ordering::Acquire) || !self.0.host.is_authorized_epoch(epoch) {
            return Err("Plugin session changed or the application is closing".into());
        }
        Ok(())
    }

    fn manager(&self) -> Result<PackageManager, String> {
        match &*self
            .0
            .store
            .lock()
            .unwrap_or_else(|error| error.into_inner())
        {
            StoreStatus::Ready(manager) => Ok(manager.clone()),
            StoreStatus::Opening => Err("Plugin store is still opening".into()),
            StoreStatus::Failed(error) => Err(error.clone()),
        }
    }

    async fn wait_manager(&self) -> Result<PackageManager, String> {
        loop {
            let ready = self.0.initialized.notified();
            {
                let store = self
                    .0
                    .store
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                match &*store {
                    StoreStatus::Ready(manager) => return Ok(manager.clone()),
                    StoreStatus::Failed(error) => return Err(error.clone()),
                    StoreStatus::Opening => {}
                }
            }
            ready.await;
        }
    }

    pub(crate) async fn snapshot(&self, epoch: u64) -> Result<ManagerSnapshot, String> {
        self.check_epoch(epoch)?;
        self.expire_preview();
        let mut snapshot = ManagerSnapshot {
            ready: false,
            error: None,
            installation_available: self.0.installation_available,
            target: self.0.target.clone(),
            plugins: Vec::new(),
        };
        let manager = match self.manager() {
            Ok(manager) => manager,
            Err(error) => {
                snapshot.error = Some(error);
                return Ok(snapshot);
            }
        };
        // Worker contribution events may arrive at 20 Hz. Verify UI metadata
        // once per inventory/session revision, not on every telemetry refresh.
        // The installer still re-verifies archive and payload before each launch.
        let revision = self.0.metadata_revision.load(Ordering::Acquire);
        let details = {
            let mut cache = self.0.metadata.lock().await;
            if let Some((cached_revision, details)) = &*cache {
                if *cached_revision == revision {
                    details.clone()
                } else {
                    let details = manager.list_details_in_epoch(epoch).await?;
                    *cache = Some((revision, details.clone()));
                    details
                }
            } else {
                let details = manager.list_details_in_epoch(epoch).await?;
                *cache = Some((revision, details.clone()));
                details
            }
        };
        self.check_epoch(epoch)?;
        let runtimes = self.0.host.snapshots();
        let failures = self
            .0
            .failures
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        snapshot.ready = true;
        snapshot.plugins = details
            .into_iter()
            .map(|details| managed_plugin(details, &runtimes, &failures))
            .collect();
        Ok(snapshot)
    }

    pub(crate) fn begin_selection(&self, owner: &str, epoch: u64) -> Result<String, String> {
        self.check_epoch(epoch)?;
        self.manager()?;
        if owner != "config" || !self.0.installation_available {
            return Err("Package installation is unavailable in this window or build".into());
        }
        self.0.host.commit_in_epoch(epoch, || {
            let mut pending = self
                .0
                .pending
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if pending.as_ref().is_some_and(|review| {
                review.package.is_none() && review.created.elapsed() < PREVIEW_TTL
            }) {
                return Err("A package file selection is already in progress".into());
            }
            let token = uuid::Uuid::new_v4().to_string();
            *pending = Some(PendingReview {
                token: token.clone(),
                owner: owner.into(),
                epoch,
                created: Instant::now(),
                package: None,
            });
            Ok(token)
        })
    }

    fn check_selection(&self, token: &str, owner: &str, epoch: u64) -> Result<(), String> {
        self.check_epoch(epoch)?;
        if self
            .0
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .is_some_and(|review| review.matches(token, owner, epoch))
        {
            Ok(())
        } else {
            Err("Package review expired or was cancelled; select the package again".into())
        }
    }

    /// Only a native file-dialog result reaches this method. The webview has no path input.
    pub(crate) async fn finish_selection(
        &self,
        token: &str,
        owner: &str,
        epoch: u64,
        path: PathBuf,
    ) -> Result<PackagePreview, String> {
        self.check_selection(token, owner, epoch)?;
        if self
            .0
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .is_some_and(|review| review.package.is_some())
        {
            return Err("Package selection has already been reviewed".into());
        }
        let manager = self.manager()?;
        let package = manager.inspect_archive(path, epoch).await?;
        self.check_selection(token, owner, epoch)?;
        let current_version = manager
            .list()
            .await?
            .into_iter()
            .find(|record| record.plugin_id == package.manifest().plugin_id)
            .map(|record| record.active.version);
        self.check_selection(token, owner, epoch)?;
        self.0.host.commit_in_epoch(epoch, || {
            let mut pending = self
                .0
                .pending
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let review = pending
                .as_mut()
                .filter(|review| review.matches(token, owner, epoch) && review.package.is_none())
                .ok_or("Package review expired or was cancelled")?;
            let manifest = package.manifest();
            let preview = PackagePreview {
                token: token.into(),
                plugin_id: manifest.plugin_id.clone(),
                version: manifest.version.clone(),
                current_version,
                permissions: manifest.permissions.clone(),
                target: manifest.target.clone(),
                host_api: manifest.host_api.clone(),
                publisher_key_id: manifest
                    .signature
                    .as_ref()
                    .ok_or("Verified package is missing its publisher")?
                    .key_id
                    .clone(),
                restart_required: false,
            };
            review.package = Some(package);
            Ok(preview)
        })
    }

    pub(crate) fn discard(&self, token: &str, owner: &str, epoch: u64) {
        let mut pending = self
            .0
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if pending.as_ref().is_some_and(|review| {
            review.token == token && review.owner == owner && review.epoch == epoch
        }) {
            pending.take();
        }
    }

    pub(crate) fn window_closed(&self, owner: &str) {
        let mut pending = self
            .0
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if pending.as_ref().is_some_and(|review| review.owner == owner) {
            pending.take();
        }
    }

    pub(crate) fn expire_preview(&self) {
        let mut pending = self
            .0
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if pending
            .as_ref()
            .is_some_and(|review| review.created.elapsed() >= PREVIEW_TTL)
        {
            pending.take();
        }
    }

    pub(crate) fn has_session_work(&self) -> bool {
        self.0.authorized_work.load(Ordering::Acquire) != 0
            || self
                .0
                .pending
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .is_some()
    }

    fn clear_preview(&self) {
        self.0
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
    }

    pub(crate) async fn install_review(
        &self,
        token: &str,
        owner: &str,
        epoch: u64,
        enable: bool,
    ) -> Result<(), String> {
        // Keep expiry checks active before consuming the last pending preview.
        let activity = self.begin_activity();
        self.check_selection(token, owner, epoch)?;
        let manager = self.manager()?;
        let package = self.0.host.commit_in_epoch(epoch, || {
            let mut pending = self
                .0
                .pending
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if !pending.as_ref().is_some_and(|review| {
                review.matches(token, owner, epoch) && review.package.is_some()
            }) {
                return Err("Package review is unavailable; select the package again".into());
            }
            pending
                .take()
                .and_then(|review| review.package)
                .ok_or_else(|| "Package review is unavailable".into())
        })?;
        let id = package.manifest().plugin_id.clone();
        self.run_operation(id, epoch, activity, async move {
            manager
                .install_verified_in_epoch(package, enable, epoch)
                .await
                .map(|_| ())
        })
        .await
    }

    pub(crate) async fn set_enabled(
        &self,
        id: &str,
        enabled: bool,
        epoch: u64,
    ) -> Result<(), String> {
        let activity = self.begin_activity();
        self.prepare_mutation(id, epoch)?;
        let manager = self.manager()?;
        let id = id.to_owned();
        self.run_operation(id.clone(), epoch, activity, async move {
            if enabled {
                manager.enable_in_epoch(&id, epoch).await
            } else {
                manager.disable_in_epoch(&id, epoch).await
            }
            .map(|_| ())
        })
        .await
    }

    pub(crate) async fn rollback(&self, id: &str, epoch: u64) -> Result<(), String> {
        let activity = self.begin_activity();
        self.prepare_mutation(id, epoch)?;
        let manager = self.manager()?;
        let id = id.to_owned();
        self.run_operation(id.clone(), epoch, activity, async move {
            manager.rollback_in_epoch(&id, epoch).await.map(|_| ())
        })
        .await
    }

    pub(crate) async fn uninstall(&self, id: &str, epoch: u64) -> Result<(), String> {
        let activity = self.begin_activity();
        self.prepare_mutation(id, epoch)?;
        let manager = self.manager()?;
        let id = id.to_owned();
        self.run_operation(id.clone(), epoch, activity, async move {
            manager.remove_in_epoch(&id, epoch).await
        })
        .await
    }

    fn begin_activity(&self) -> SessionActivity {
        self.0.authorized_work.fetch_add(1, Ordering::AcqRel);
        SessionActivity(self.0.clone())
    }

    async fn run_operation<F>(
        &self,
        id: String,
        epoch: u64,
        activity: SessionActivity,
        operation: F,
    ) -> Result<(), String>
    where
        F: Future<Output = Result<(), String>> + Send + 'static,
    {
        let service = self.clone();
        run_owned(activity, async move {
            let result = operation.await;
            service.record_result(&id, epoch, &result);
            result
        })
        .await
    }

    fn prepare_mutation(&self, id: &str, epoch: u64) -> Result<(), String> {
        self.check_epoch(epoch)?;
        validate_plugin_id(id)?;
        self.0.host.commit_in_epoch(epoch, || {
            self.clear_preview();
            Ok(())
        })
    }

    fn record_result(&self, id: &str, epoch: u64, result: &Result<(), String>) {
        self.0.metadata_revision.fetch_add(1, Ordering::AcqRel);
        let _ = self.0.host.commit_in_epoch(epoch, || {
            let mut failures = self
                .0
                .failures
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if let Err(error) = result {
                if failures.len() < MAX_FAILURES || failures.contains_key(id) {
                    failures.insert(id.into(), error.clone());
                }
            } else {
                failures.remove(id);
            }
            Ok(())
        });
        (self.0.changed)();
    }

    /// Session revocation is synchronous; restoration is a separate, epoch-bound task.
    pub(crate) fn session_changed(&self, unlocked: bool) -> Option<u64> {
        let epoch = self.0.host.revoke_epoch();
        self.0.metadata_revision.fetch_add(1, Ordering::AcqRel);
        self.clear_preview();
        self.0
            .failures
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
        if unlocked && !self.0.closing.load(Ordering::Acquire) && self.0.host.resume_in_epoch(epoch)
        {
            Some(epoch)
        } else {
            None
        }
    }

    pub(crate) async fn restore(&self, epoch: u64) -> Result<(), String> {
        let activity = self.begin_activity();
        let service = self.clone();
        run_owned(activity, async move {
            let manager = service.wait_manager().await?;
            service.check_epoch(epoch)?;
            for result in manager.restore_enabled_in_epoch(epoch).await? {
                service.record_result(&result.plugin_id, epoch, &result.error.map_or(Ok(()), Err));
            }
            service.check_epoch(epoch)
        })
        .await
    }

    pub(crate) fn begin_shutdown(&self) {
        self.0.closing.store(true, Ordering::Release);
        self.0.host.revoke();
        self.clear_preview();
    }

    pub(crate) async fn close(&self) -> Result<(), String> {
        self.begin_shutdown();
        // Failed initialization owns no store lease. Successful initialization
        // must finish before close can release its lease and allow app exit.
        if let Ok(manager) = self.wait_manager().await {
            manager.close().await?;
        }
        self.0.host.shutdown().await;
        Ok(())
    }
}

/// Installer transactions outlive a cancelled IPC caller. Their expiry-watch
/// activity and completion notification must have the same owned lifetime.
async fn run_owned<F>(activity: SessionActivity, operation: F) -> Result<(), String>
where
    F: Future<Output = Result<(), String>> + Send + 'static,
{
    tokio::spawn(async move {
        let _activity = activity;
        operation.await
    })
    .await
    .map_err(|error| format!("Plugin operation task failed: {error}"))?
}

fn managed_plugin(
    details: InstalledPluginDetails,
    runtimes: &[PluginSnapshot],
    failures: &BTreeMap<String, String>,
) -> ManagedPlugin {
    let id = details.record.plugin_id;
    let runtime = runtimes
        .iter()
        .find(|worker| worker.plugin_id == id)
        .cloned();
    let error = details
        .error
        .or_else(|| failures.get(&id).cloned())
        .or_else(|| {
            runtime
                .as_ref()
                .and_then(|worker| worker.last_error.clone())
        });
    ManagedPlugin {
        plugin_id: id,
        version: details.record.active.version,
        rollback_version: details.record.rollback.map(|version| version.version),
        enabled: details.record.enabled,
        permissions: details
            .manifest
            .map(|manifest| manifest.permissions)
            .unwrap_or_default(),
        runtime,
        error,
    }
}

#[cfg(test)]
#[path = "application_tests.rs"]
mod tests;
