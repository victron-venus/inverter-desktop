//! App-wide, desktop-only supervision for explicitly trusted worker executables.
//!
//! This is a process boundary, not an OS sandbox. Package discovery and executable
//! authorization belong to the signed package layer; the webview cannot start
//! an executable. Workers receive no inherited environment or core service handles.

use super::protocol::{
    encode_host_frame, parse_worker_frame, validate_handshake, validate_plugin_id,
    DashboardContribution, HostMessage, WorkerMessage, HOST_API_VERSION, MAX_ACTION_DEADLINE_MS,
    MAX_FRAME_BYTES, PROTOCOL_VERSION,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::{self, Instant};

const MAX_WORKERS: usize = 8;
const MAX_IN_FLIGHT: usize = 16;
const CONTROL_QUEUE_CAPACITY: usize = 32;
const PIPE_QUEUE_CAPACITY: usize = 16;
const MAX_RESTARTS: u32 = 2;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(3);
const REAP_TIMEOUT: Duration = Duration::from_secs(2);
const STOP_GRACE: Duration = Duration::from_millis(200);
const RESTART_BACKOFF: Duration = Duration::from_millis(100);
const MAX_FRAMES_PER_SECOND: usize = 120;
const MAX_CONTRIBUTIONS_PER_SECOND: usize = 10;
const DEADLINE_TICK: Duration = Duration::from_millis(10);

type ChangeCallback = Arc<dyn Fn() + Send + Sync>;
type ActionReply = oneshot::Sender<Result<Value, PluginError>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerState {
    Starting,
    Running,
    Restarting,
    Stopped,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
pub struct PluginSnapshot {
    pub plugin_id: String,
    pub state: WorkerState,
    pub generation: u64,
    pub restart_count: u32,
    pub contributions: Vec<DashboardContribution>,
    /// Host-owned diagnostic codes only. Worker stderr and response text never enter this field.
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginError {
    InvalidWorker,
    AlreadyRegistered,
    Capacity,
    HostStopped,
    UnknownPlugin,
    Unavailable,
    UnknownAction,
    Busy,
    InvalidRequest,
    DeadlineExceeded,
    Cancelled,
    WorkerError,
}

impl std::fmt::Display for PluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

/// Constructed only by trusted native code. Never deserialize this from IPC.
#[derive(Clone)]
pub struct WorkerSpec {
    pub plugin_id: String,
    pub executable: PathBuf,
    pub args: Vec<OsString>,
}

struct WorkerEntry {
    snapshot: Mutex<PluginSnapshot>,
    commands: mpsc::Sender<Control>,
    stop: watch::Sender<bool>,
    task: Mutex<Option<JoinHandle<()>>>,
    authority: Arc<Mutex<Authority>>,
    epoch: u64,
    done: watch::Receiver<bool>,
    reaped: AtomicBool,
    changed: ChangeCallback,
}

impl WorkerEntry {
    fn authorized(&self) -> bool {
        let authority = self.authority.lock().unwrap_or_else(|e| e.into_inner());
        authority.enabled && authority.epoch == self.epoch
    }

    fn snapshot(&self) -> PluginSnapshot {
        self.snapshot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn update(&self, update: impl FnOnce(&mut PluginSnapshot)) {
        update(&mut self.snapshot.lock().unwrap_or_else(|e| e.into_inner()));
        (self.changed)();
    }
}

struct Authority {
    enabled: bool,
    epoch: u64,
}

struct HostInner {
    authority: Arc<Mutex<Authority>>,
    entries: Mutex<HashMap<String, Arc<WorkerEntry>>>,
    stopped: AtomicBool,
    next_request: AtomicU64,
    changed: ChangeCallback,
}

impl Drop for HostInner {
    fn drop(&mut self) {
        // A separate watch channel makes stop independent of a saturated action queue.
        for entry in self
            .entries
            .get_mut()
            .unwrap_or_else(|e| e.into_inner())
            .values()
        {
            let _ = entry.stop.send(true);
        }
    }
}

/// One instance is managed by Tauri and shared by every window. Clones share the registry.
#[derive(Clone)]
pub struct PluginHost(Arc<HostInner>);

impl Default for PluginHost {
    fn default() -> Self {
        Self::new(Arc::new(|| {}))
    }
}

impl PluginHost {
    pub fn new(changed: ChangeCallback) -> Self {
        Self(Arc::new(HostInner {
            authority: Arc::new(Mutex::new(Authority {
                enabled: true,
                epoch: 0,
            })),
            entries: Mutex::new(HashMap::new()),
            stopped: AtomicBool::new(false),
            next_request: AtomicU64::new(1),
            changed,
        }))
    }

    pub fn authority_epoch(&self) -> u64 {
        self.0
            .authority
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .epoch
    }

    /// Check the authority captured before an asynchronous package operation.
    pub fn is_authorized_epoch(&self, epoch: u64) -> bool {
        let authority = self.0.authority.lock().unwrap_or_else(|e| e.into_inner());
        !self.0.stopped.load(Ordering::Acquire) && authority.enabled && authority.epoch == epoch
    }

    /// Linearize a small synchronous package-state commit with session revocation.
    /// The callback must not call the host or wait for asynchronous work.
    pub(crate) fn commit_in_epoch<T>(
        &self,
        epoch: u64,
        commit: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let authority = self.0.authority.lock().unwrap_or_else(|e| e.into_inner());
        if self.0.stopped.load(Ordering::Acquire) || !authority.enabled || authority.epoch != epoch
        {
            return Err("Plugin operation authorization changed".into());
        }
        commit()
    }

    pub fn snapshots(&self) -> Vec<PluginSnapshot> {
        let mut snapshots: Vec<_> = self
            .0
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .map(|entry| entry.snapshot())
            .collect();
        snapshots.sort_by(|a, b| a.plugin_id.cmp(&b.plugin_id));
        snapshots
    }

    /// Register a worker exactly once and begin its bounded startup/restart lifecycle.
    /// The returned snapshot is `starting`; readiness is signalled through snapshots.
    pub async fn start(&self, spec: WorkerSpec) -> Result<PluginSnapshot, PluginError> {
        self.register(spec, None)
    }

    /// Register only in the session that authorized a package operation.
    /// The epoch check and registration share the same authority lock.
    pub async fn start_in_epoch(
        &self,
        spec: WorkerSpec,
        epoch: u64,
    ) -> Result<PluginSnapshot, PluginError> {
        self.register(spec, Some(epoch))
    }

    fn register(
        &self,
        spec: WorkerSpec,
        expected_epoch: Option<u64>,
    ) -> Result<PluginSnapshot, PluginError> {
        if self.0.stopped.load(Ordering::Acquire) {
            return Err(PluginError::HostStopped);
        }
        if !spec.executable.is_absolute() || validate_plugin_id(&spec.plugin_id).is_err() {
            return Err(PluginError::InvalidWorker);
        }
        let authority = self.0.authority.lock().unwrap_or_else(|e| e.into_inner());
        if !authority.enabled || expected_epoch.is_some_and(|epoch| epoch != authority.epoch) {
            return Err(PluginError::Unavailable);
        }
        let (commands, receiver) = mpsc::channel(CONTROL_QUEUE_CAPACITY);
        let (stop, stop_receiver) = watch::channel(false);
        let (finished, done) = watch::channel(false);
        let entry = Arc::new(WorkerEntry {
            snapshot: Mutex::new(PluginSnapshot {
                plugin_id: spec.plugin_id.clone(),
                state: WorkerState::Starting,
                generation: 0,
                restart_count: 0,
                contributions: Vec::new(),
                last_error: None,
            }),
            commands,
            stop,
            task: Mutex::new(None),
            changed: self.0.changed.clone(),
            authority: self.0.authority.clone(),
            epoch: authority.epoch,
            done,
            reaped: AtomicBool::new(true),
        });
        {
            let mut entries = self.0.entries.lock().unwrap_or_else(|e| e.into_inner());
            if self.0.stopped.load(Ordering::Acquire) {
                return Err(PluginError::HostStopped);
            }
            if let Some(previous) = entries.get(&spec.plugin_id) {
                if !matches!(
                    previous.snapshot().state,
                    WorkerState::Stopped | WorkerState::Failed
                ) || !*previous.done.borrow()
                    || !previous.reaped.load(Ordering::Acquire)
                {
                    return Err(PluginError::AlreadyRegistered);
                }
                entry
                    .snapshot
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .generation = previous.snapshot().generation;
            }
            if !entries.contains_key(&spec.plugin_id) && entries.len() >= MAX_WORKERS {
                return Err(PluginError::Capacity);
            }
            entries.insert(spec.plugin_id.clone(), entry.clone());
            let task_entry = entry.clone();
            let task = tokio::spawn(async move {
                supervise(spec, task_entry, receiver, stop_receiver).await;
                let _ = finished.send(true);
            });
            *entry.task.lock().unwrap_or_else(|e| e.into_inner()) = Some(task);
        }
        drop(authority);
        (self.0.changed)();
        Ok(entry.snapshot())
    }

    pub async fn action(
        &self,
        plugin_id: &str,
        action_id: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, PluginError> {
        if self.0.stopped.load(Ordering::Acquire) {
            return Err(PluginError::HostStopped);
        }
        if timeout.is_zero() || timeout > Duration::from_millis(MAX_ACTION_DEADLINE_MS) {
            return Err(PluginError::InvalidRequest);
        }
        let entry = self.entry(plugin_id)?;
        let snapshot = entry.snapshot();
        if snapshot.state != WorkerState::Running {
            return Err(PluginError::Unavailable);
        }
        if !advertises(&snapshot.contributions, action_id, &params) {
            return Err(PluginError::UnknownAction);
        }
        let request_id = self
            .0
            .next_request
            .fetch_add(1, Ordering::Relaxed)
            .to_string();
        let message = HostMessage::Action {
            request_id: request_id.clone(),
            action_id: action_id.to_owned(),
            params: params.clone(),
            deadline_ms: timeout.as_millis().max(1) as u64,
        };
        let encoded = encode_host_frame(&message).map_err(|_| PluginError::InvalidRequest)?;
        let (reply, response) = oneshot::channel();
        let (cancel, cancellation) = watch::channel(false);
        let deadline = Instant::now() + timeout;
        {
            let authority = entry.authority.lock().unwrap_or_else(|e| e.into_inner());
            if !authority.enabled || authority.epoch != entry.epoch {
                return Err(PluginError::Unavailable);
            }
            entry
                .commands
                .try_send(Control::Action {
                    request_id: request_id.clone(),
                    generation: snapshot.generation,
                    action_id: action_id.to_owned(),
                    params,
                    encoded,
                    cancellation,
                    deadline,
                    reply,
                })
                .map_err(|error| match error {
                    mpsc::error::TrySendError::Full(_) => PluginError::Busy,
                    mpsc::error::TrySendError::Closed(_) => PluginError::Unavailable,
                })?;
        }
        let _cancel_on_drop = CancelOnDrop {
            commands: entry.commands.clone(),
            request_id,
            cancel,
        };
        let result = time::timeout_at(deadline, response).await;
        if !entry.authorized() {
            return Err(PluginError::Unavailable);
        }
        match result {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(PluginError::Unavailable),
            Err(_) => Err(PluginError::DeadlineExceeded),
        }
    }

    pub async fn stop(&self, plugin_id: &str) -> Result<(), PluginError> {
        let entry = self.entry(plugin_id)?;
        let _ = entry.stop.send(true);
        wait_stopped(&entry).await;
        if !entry.reaped.load(Ordering::Acquire) {
            return Err(PluginError::WorkerError);
        }
        Ok(())
    }

    /// Stop and reap before releasing a plugin's registry slot and contributions.
    /// A concurrent replacement must never be removed on behalf of an old entry.
    pub async fn remove(&self, plugin_id: &str) -> Result<(), PluginError> {
        let entry = self.entry(plugin_id)?;
        let _ = entry.stop.send(true);
        wait_stopped(&entry).await;
        if !entry.reaped.load(Ordering::Acquire) {
            return Err(PluginError::WorkerError);
        }
        {
            let mut entries = self.0.entries.lock().unwrap_or_else(|e| e.into_inner());
            match entries.get(plugin_id) {
                Some(current) if Arc::ptr_eq(current, &entry) => {
                    entries.remove(plugin_id);
                }
                Some(_) => return Err(PluginError::AlreadyRegistered),
                None => return Ok(()),
            }
        }
        (self.0.changed)();
        Ok(())
    }

    /// Revoke synchronously before publishing an application logout or policy change.
    /// Old generations stay unauthorized even if a later login resumes the host.
    pub fn revoke(&self) {
        let mut authority = self.0.authority.lock().unwrap_or_else(|e| e.into_inner());
        authority.enabled = false;
        authority.epoch = authority.epoch.wrapping_add(1);
        let entries: Vec<_> = self
            .0
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect();
        for entry in entries {
            let _ = entry.stop.send(true);
            let mut snapshot = entry.snapshot.lock().unwrap_or_else(|e| e.into_inner());
            snapshot.state = WorkerState::Stopped;
            snapshot.contributions.clear();
        }
        drop(authority);
        (self.0.changed)();
    }

    /// Resume native registration after a newly authorized session, without restarting workers.
    pub fn resume(&self) {
        let mut authority = self.0.authority.lock().unwrap_or_else(|e| e.into_inner());
        if !self.0.stopped.load(Ordering::Acquire) {
            authority.enabled = true;
        }
    }

    /// Stop and reap registered children; later explicit starts remain possible after resume.
    pub async fn stop_all(&self) {
        self.revoke();
        let entries: Vec<_> = self
            .0
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect();
        for entry in entries {
            wait_stopped(&entry).await;
        }
    }

    /// Wait only for revoked generations, without changing a newer authenticated session.
    pub async fn wait_revoked(&self) {
        let entries: Vec<_> = {
            let authority = self.0.authority.lock().unwrap_or_else(|e| e.into_inner());
            self.0
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .filter(|entry| !authority.enabled || entry.epoch != authority.epoch)
                .cloned()
                .collect()
        };
        for entry in entries {
            wait_stopped(&entry).await;
        }
    }

    /// Terminal shutdown. New registrations and actions are rejected thereafter.
    pub async fn shutdown(&self) {
        self.0.stopped.store(true, Ordering::Release);
        self.stop_all().await;
    }

    fn entry(&self, id: &str) -> Result<Arc<WorkerEntry>, PluginError> {
        self.0
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .cloned()
            .ok_or(PluginError::UnknownPlugin)
    }
}

async fn wait_stopped(entry: &WorkerEntry) {
    let mut done = entry.done.clone();
    let _ = done.wait_for(|finished| *finished).await;
    let task = entry.task.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(task) = task {
        let _ = task.await;
    }
}

fn advertises(items: &[DashboardContribution], action_id: &str, params: &Value) -> bool {
    items.iter().any(|item| matches!(item, DashboardContribution::Action { action_id: id, params: advertised, .. } if id == action_id && advertised == params))
}

struct CancelOnDrop {
    commands: mpsc::Sender<Control>,
    request_id: String,
    cancel: watch::Sender<bool>,
}
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        let _ = self.cancel.send(true);
        // Expiration remains enforced by the supervisor even when cancellation meets a full queue.
        let _ = self
            .commands
            .try_send(Control::Cancel(self.request_id.clone()));
    }
}

enum Control {
    Action {
        request_id: String,
        generation: u64,
        action_id: String,
        params: Value,
        encoded: Vec<u8>,
        cancellation: watch::Receiver<bool>,
        deadline: Instant,
        reply: ActionReply,
    },
    Cancel(String),
}

struct Pending {
    deadline: Instant,
    reply: ActionReply,
}

enum Outgoing {
    Control(Vec<u8>),
    Action {
        encoded: Vec<u8>,
        deadline: Instant,
        cancellation: watch::Receiver<bool>,
    },
}

async fn write_frames<W: AsyncWrite + Unpin>(
    mut stdin: W,
    mut outgoing: mpsc::Receiver<Outgoing>,
    mut stop: watch::Receiver<bool>,
    authority: Arc<Mutex<Authority>>,
    epoch: u64,
) -> Result<(), &'static str> {
    loop {
        let message = tokio::select! {
            biased;
            _ = stop.changed() => {
                // No partially written frame here. Bypass queued actions for cooperative shutdown.
                let shutdown = encode_host_frame(&HostMessage::Shutdown).map_err(|_| "host_frame_invalid")?;
                let _ = time::timeout(Duration::from_millis(20), stdin.write_all(&shutdown)).await;
                return Ok(());
            }
            message = outgoing.recv() => match message { Some(message) => message, None => return Ok(()) },
        };
        match message {
            Outgoing::Control(bytes) => {
                tokio::select! {
                    biased;
                    _ = stop.changed() => return Ok(()),
                    result = time::timeout(Duration::from_secs(1), stdin.write_all(&bytes)) => {
                        result.map_err(|_| "worker_write_timeout")?.map_err(|_| "worker_write_failed")?;
                    }
                }
            }
            Outgoing::Action {
                encoded,
                deadline,
                mut cancellation,
            } => {
                let authorized = {
                    let guard = authority.lock().unwrap_or_else(|e| e.into_inner());
                    guard.enabled && guard.epoch == epoch
                };
                if !authorized
                    || *stop.borrow()
                    || *cancellation.borrow()
                    || Instant::now() >= deadline
                {
                    continue;
                }
                tokio::select! {
                    biased;
                    _ = stop.changed() => return Ok(()),
                    _ = cancellation.changed() => return Err("worker_write_cancelled"),
                    _ = time::sleep_until(deadline) => return Err("worker_write_timeout"),
                    result = stdin.write_all(&encoded) => result.map_err(|_| "worker_write_failed")?,
                }
                // A cancelled partial frame ends this writer/generation; queued frames never follow it.
            }
        }
    }
}

struct WorkerPipes {
    writer: mpsc::Sender<Outgoing>,
    frames: mpsc::Receiver<Result<WorkerMessage, &'static str>>,
    tasks: Vec<JoinHandle<()>>,
}

impl WorkerPipes {
    fn new(child: &mut Child, entry: &WorkerEntry, stop: watch::Receiver<bool>) -> Option<Self> {
        let stdin = child.stdin.take()?;
        let stdout = child.stdout.take()?;
        let mut stderr = child.stderr.take()?;
        let (writer, outgoing) = mpsc::channel(PIPE_QUEUE_CAPACITY);
        let (incoming, frames) = mpsc::channel(PIPE_QUEUE_CAPACITY);
        let writer_errors = incoming.clone();
        let authority = entry.authority.clone();
        let epoch = entry.epoch;
        let write_task = tokio::spawn(async move {
            if let Err(code) = write_frames(stdin, outgoing, stop, authority, epoch).await {
                let _ = writer_errors.send(Err(code)).await;
            }
        });
        let read_task = tokio::spawn(async move {
            let mut reader = BufReader::with_capacity(4096, stdout);
            loop {
                let result = read_frame(&mut reader).await;
                let failed = result.is_err();
                if incoming.send(result).await.is_err() || failed {
                    return;
                }
            }
        });
        let stderr_task = tokio::spawn(async move {
            // Drain with constant memory. Never log plugin-controlled text or inherited secrets.
            let mut buffer = [0_u8; 4096];
            while let Ok(count) = stderr.read(&mut buffer).await {
                if count == 0 {
                    break;
                }
            }
        });
        Some(Self {
            writer,
            frames,
            tasks: vec![write_task, read_task, stderr_task],
        })
    }

    fn send(&self, message: &HostMessage) -> Result<(), &'static str> {
        let bytes = encode_host_frame(message).map_err(|_| "host_frame_invalid")?;
        self.writer
            .try_send(Outgoing::Control(bytes))
            .map_err(|_| "worker_write_queue_full")
    }
}

impl Drop for WorkerPipes {
    fn drop(&mut self) {
        // Cancel blocked async pipe I/O, including inherited descriptors in a worker descendant.
        for task in &self.tasks {
            task.abort();
        }
    }
}

async fn read_frame<R: AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
) -> Result<WorkerMessage, &'static str> {
    let mut frame = Vec::new();
    loop {
        let buffer = reader.fill_buf().await.map_err(|_| "worker_read_failed")?;
        if buffer.is_empty() {
            return Err("worker_eof");
        }
        let newline = buffer.iter().position(|&byte| byte == b'\n');
        let count = newline.map_or(buffer.len(), |index| index + 1);
        if frame.len() + count > MAX_FRAME_BYTES {
            return Err("worker_frame_too_large");
        }
        frame.extend_from_slice(&buffer[..count]);
        reader.consume(count);
        if newline.is_some() {
            return parse_worker_frame(&frame).map_err(|_| "worker_frame_invalid");
        }
    }
}

fn spawn_generation(
    spec: &WorkerSpec,
    entry: &WorkerEntry,
    restart_count: u32,
) -> Result<Option<Child>, &'static str> {
    let authority = entry.authority.lock().unwrap_or_else(|e| e.into_inner());
    if !authority.enabled || authority.epoch != entry.epoch {
        return Ok(None);
    }
    {
        let mut snapshot = entry.snapshot.lock().unwrap_or_else(|e| e.into_inner());
        snapshot.state = if restart_count == 0 {
            WorkerState::Starting
        } else {
            WorkerState::Restarting
        };
        snapshot.generation += 1;
        snapshot.restart_count = restart_count;
        snapshot.contributions.clear();
    }
    let child = Command::new(&spec.executable)
        .args(&spec.args)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| "worker_spawn_failed")?;
    entry.reaped.store(false, Ordering::Release);
    drop(authority);
    (entry.changed)();
    Ok(Some(child))
}

async fn supervise(
    spec: WorkerSpec,
    entry: Arc<WorkerEntry>,
    mut commands: mpsc::Receiver<Control>,
    mut stop: watch::Receiver<bool>,
) {
    let mut restart_count = 0;
    loop {
        if *stop.borrow() || !entry.authorized() {
            break;
        }
        let mut child = match spawn_generation(&spec, &entry, restart_count) {
            Ok(Some(child)) => child,
            Ok(None) => break,
            Err(code) => {
                fail_entry(&entry, code);
                return;
            }
        };
        let Some(mut pipes) = WorkerPipes::new(&mut child, &entry, stop.clone()) else {
            entry
                .reaped
                .store(terminate(&mut child, None).await, Ordering::Release);
            fail_entry(&entry, "worker_pipe_failed");
            return;
        };
        let outcome = run_generation(
            &spec,
            &entry,
            &mut child,
            &mut pipes,
            &mut commands,
            &mut stop,
        )
        .await;
        // Remove stale contributions before awaiting grace, cancellation, or restart backoff.
        entry.update(|snapshot| {
            snapshot.contributions.clear();
            snapshot.state = if matches!(outcome, Outcome::Stopped) {
                WorkerState::Stopped
            } else {
                WorkerState::Restarting
            };
            snapshot.last_error = outcome.error().map(str::to_owned);
        });
        let reaped = terminate(&mut child, Some(&pipes)).await;
        entry.reaped.store(reaped, Ordering::Release);
        drop(pipes);
        if !reaped {
            fail_entry(&entry, "worker_reap_failed");
            return;
        }
        match outcome {
            Outcome::Stopped => break,
            Outcome::Failed(code) => {
                fail_entry(&entry, code);
                return;
            }
            Outcome::Exited(code) if restart_count >= MAX_RESTARTS => {
                fail_entry(&entry, code);
                return;
            }
            Outcome::Exited(_) => {}
        }
        restart_count += 1;
        let backoff = time::sleep(RESTART_BACKOFF * restart_count);
        tokio::pin!(backoff);
        loop {
            tokio::select! {
                biased;
                _ = stop.changed() => break,
                _ = &mut backoff => break,
                control = commands.recv() => reject_control(control),
            }
        }
    }
    entry.update(|snapshot| {
        snapshot.state = WorkerState::Stopped;
        snapshot.contributions.clear();
        snapshot.last_error = None;
    });
    while let Ok(control) = commands.try_recv() {
        reject_control(Some(control));
    }
}

enum Outcome {
    Stopped,
    Exited(&'static str),
    Failed(&'static str),
}
impl Outcome {
    fn error(&self) -> Option<&'static str> {
        match self {
            Self::Stopped => None,
            Self::Exited(code) | Self::Failed(code) => Some(code),
        }
    }
}

fn fail_entry(entry: &WorkerEntry, code: &str) {
    entry.update(|snapshot| {
        snapshot.state = WorkerState::Failed;
        snapshot.last_error = Some(code.to_owned());
        snapshot.contributions.clear();
    });
}

async fn run_generation(
    spec: &WorkerSpec,
    entry: &WorkerEntry,
    child: &mut Child,
    pipes: &mut WorkerPipes,
    commands: &mut mpsc::Receiver<Control>,
    stop: &mut watch::Receiver<bool>,
) -> Outcome {
    let hello = HostMessage::Hello {
        protocol_version: PROTOCOL_VERSION,
        host_api_version: HOST_API_VERSION.to_owned(),
        plugin_id: spec.plugin_id.clone(),
    };
    if pipes.send(&hello).is_err() {
        return Outcome::Failed("host_hello_failed");
    }
    let generation = entry.snapshot().generation;
    let startup_deadline = Instant::now() + STARTUP_TIMEOUT;
    let mut ready = false;
    let mut rate_window = Instant::now();
    let mut frame_count = 0;
    let mut contribution_count = 0;
    let mut pending = HashMap::new();
    let mut ticks = time::interval(DEADLINE_TICK);
    ticks.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
    let outcome = loop {
        tokio::select! {
            biased;
            _ = stop.changed() => break Outcome::Stopped,
            exit = child.wait() => {
                break Outcome::Exited(if exit.is_ok() { "worker_exited" } else { "worker_wait_failed" });
            }
            _ = ticks.tick() => {
                if !ready && Instant::now() >= startup_deadline { break Outcome::Failed("worker_startup_timeout"); }
                expire_pending(&mut pending, pipes);
            }
            frame = pipes.frames.recv() => {
                if !entry.authorized() { break Outcome::Stopped; }
                if rate_window.elapsed() >= Duration::from_secs(1) {
                    rate_window = Instant::now(); frame_count = 0; contribution_count = 0;
                }
                frame_count += 1;
                if matches!(&frame, Some(Ok(WorkerMessage::Contributions { .. }))) { contribution_count += 1; }
                if frame_count > MAX_FRAMES_PER_SECOND || contribution_count > MAX_CONTRIBUTIONS_PER_SECOND {
                    break Outcome::Failed("worker_rate_limit");
                }
                match handle_frame(frame, spec, entry, &mut ready, &mut pending) {
                    Ok(()) => {},
                    Err(outcome) => break outcome,
                }
            }
            control = commands.recv() => {
                match control {
                    Some(control) => handle_control(control, entry, generation, ready, pipes, &mut pending),
                    None => break Outcome::Stopped,
                }
            }
        }
    };
    for (_, request) in pending {
        let _ = request.reply.send(Err(PluginError::Unavailable));
    }
    outcome
}

fn handle_frame(
    frame: Option<Result<WorkerMessage, &'static str>>,
    spec: &WorkerSpec,
    entry: &WorkerEntry,
    ready: &mut bool,
    pending: &mut HashMap<String, Pending>,
) -> Result<(), Outcome> {
    let message = match frame {
        Some(Ok(message)) => message,
        Some(Err("worker_eof" | "worker_write_failed")) | None => {
            return Err(Outcome::Exited("worker_pipe_closed"))
        }
        Some(Err(code)) => return Err(Outcome::Failed(code)),
    };
    if !*ready {
        validate_handshake(&spec.plugin_id, &message)
            .map_err(|_| Outcome::Failed("worker_handshake_invalid"))?;
        let authority = entry.authority.lock().unwrap_or_else(|e| e.into_inner());
        if !authority.enabled || authority.epoch != entry.epoch {
            return Err(Outcome::Stopped);
        }
        *ready = true;
        {
            let mut snapshot = entry.snapshot.lock().unwrap_or_else(|e| e.into_inner());
            snapshot.state = WorkerState::Running;
            snapshot.last_error = None;
        }
        drop(authority);
        (entry.changed)();
        return Ok(());
    }
    let authority = entry.authority.lock().unwrap_or_else(|e| e.into_inner());
    if !authority.enabled || authority.epoch != entry.epoch {
        return Err(Outcome::Stopped);
    }
    let mut notify = false;
    match message {
        WorkerMessage::Contributions { items } => {
            entry
                .snapshot
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .contributions = items;
            notify = true;
        }
        WorkerMessage::ActionResult { request_id, value } => {
            if let Some(request) = pending.remove(&request_id) {
                let result = if Instant::now() >= request.deadline {
                    Err(PluginError::DeadlineExceeded)
                } else {
                    Ok(value)
                };
                let _ = request.reply.send(result);
            }
        }
        WorkerMessage::ActionError { request_id, .. } => {
            if let Some(request) = pending.remove(&request_id) {
                let _ = request.reply.send(Err(PluginError::WorkerError));
            }
        }
        WorkerMessage::Event { .. } => { /* Reserved worker-scoped data; no arbitrary app event forwarding. */
        }
        WorkerMessage::Ready { .. } => return Err(Outcome::Failed("worker_duplicate_handshake")),
    }
    drop(authority);
    if notify {
        (entry.changed)();
    }
    Ok(())
}

fn handle_control(
    control: Control,
    entry: &WorkerEntry,
    generation: u64,
    ready: bool,
    pipes: &WorkerPipes,
    pending: &mut HashMap<String, Pending>,
) {
    match control {
        Control::Cancel(id) => {
            if let Some(request) = pending.remove(&id) {
                let _ = pipes.send(&HostMessage::Cancel { request_id: id });
                let _ = request.reply.send(Err(PluginError::Cancelled));
            }
        }
        Control::Action {
            request_id,
            generation: expected,
            action_id,
            params,
            encoded,
            cancellation,
            deadline,
            reply,
        } => {
            let authority = entry.authority.lock().unwrap_or_else(|e| e.into_inner());
            let error = if !authority.enabled
                || authority.epoch != entry.epoch
                || !ready
                || expected != generation
            {
                Some(PluginError::Unavailable)
            } else if Instant::now() >= deadline {
                Some(PluginError::DeadlineExceeded)
            } else if !advertises(&entry.snapshot().contributions, &action_id, &params) {
                Some(PluginError::UnknownAction)
            } else if pending.len() >= MAX_IN_FLIGHT {
                Some(PluginError::Busy)
            } else {
                None
            };
            if let Some(error) = error {
                let _ = reply.send(Err(error));
                return;
            }
            if pipes
                .writer
                .try_send(Outgoing::Action {
                    encoded,
                    deadline,
                    cancellation,
                })
                .is_err()
            {
                let _ = reply.send(Err(PluginError::Busy));
                return;
            }
            pending.insert(request_id, Pending { deadline, reply });
        }
    }
}

fn expire_pending(pending: &mut HashMap<String, Pending>, pipes: &WorkerPipes) {
    let expired: Vec<_> = pending
        .iter()
        .filter(|(_, request)| request.reply.is_closed() || Instant::now() >= request.deadline)
        .map(|(id, _)| id.clone())
        .collect();
    for id in expired {
        if let Some(request) = pending.remove(&id) {
            let _ = pipes.send(&HostMessage::Cancel { request_id: id });
            let _ = request.reply.send(Err(PluginError::DeadlineExceeded));
        }
    }
}

fn reject_control(control: Option<Control>) {
    if let Some(Control::Action { reply, .. }) = control {
        let _ = reply.send(Err(PluginError::Unavailable));
    }
}

async fn terminate(child: &mut Child, pipes: Option<&WorkerPipes>) -> bool {
    if let Some(pipes) = pipes {
        let _ = pipes.send(&HostMessage::Shutdown);
    }
    if matches!(time::timeout(STOP_GRACE, child.wait()).await, Ok(Ok(_))) {
        return true;
    }
    let _ = child.start_kill();
    // A kernel-delayed reap is reported as failed; that registration cannot be replaced.
    // kill_on_drop keeps Tokio's fallback kill/reap behavior on this exceptional path.
    matches!(time::timeout(REAP_TIMEOUT, child.wait()).await, Ok(Ok(_)))
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
