//! Minimal desktop-worker stdio transport. Identity, configuration validation,
//! host API negotiation and all network behavior belong to the consuming worker.
//! Frames and queues are bounded; protocol/configuration values have no Debug.

use serde::{de::DeserializeOwned, Deserialize};
use serde_json::Value;
use std::io::{BufRead, Write};
use tokio::sync::{mpsc, oneshot, watch};

pub const MAX_FRAME_BYTES: usize = 64 * 1024;
pub const QUEUE_CAPACITY: usize = 2;

// Deliberately no Debug: Configuration contains plaintext credentials.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum HostFrame<C> {
    Hello {
        protocol_version: u32,
        host_api_version: String,
        plugin_id: String,
    },
    Configuration {
        configuration: C,
    },
    Shutdown {},
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    Running,
    Shutdown,
    Eof,
    Invalid,
}

struct Outbound {
    bytes: Vec<u8>,
    written: oneshot::Sender<Result<(), &'static str>>,
}

/// Clones share one bounded queue and its single flush-owning writer thread.
#[derive(Clone)]
pub struct Output {
    sender: mpsc::Sender<Outbound>,
}

impl Output {
    /// Own a supplied writer with the same bounded transport used by stdio.
    /// The writer lives on its own thread; callers never use Tokio's blocking
    /// pool for potentially indefinitely backpressured output.
    pub fn with_writer<W: Write + Send + 'static>(writer: W) -> Self {
        let (sender, outgoing) = mpsc::channel(QUEUE_CAPACITY);
        std::thread::spawn(move || write_frames(writer, outgoing));
        Self { sender }
    }

    pub async fn send(&self, value: Value) -> Result<(), &'static str> {
        let bytes = encode_frame(value)?;
        let (written, result) = oneshot::channel();
        self.sender
            .send(Outbound { bytes, written })
            .await
            .map_err(|_| "host output closed")?;
        result.await.map_err(|_| "host output closed")?
    }

    /// Best-effort traffic must not block a worker's network event loop.
    /// Handshake/configuration/status use send() and wait for the actual flush.
    pub fn try_send(&self, value: Value) -> Result<bool, &'static str> {
        let bytes = encode_frame(value)?;
        let (written, _) = oneshot::channel();
        match self.sender.try_send(Outbound { bytes, written }) {
            Ok(()) => Ok(true),
            Err(mpsc::error::TrySendError::Full(_)) => Ok(false),
            Err(mpsc::error::TrySendError::Closed(_)) => Err("host output closed"),
        }
    }
}

fn encode_frame(value: Value) -> Result<Vec<u8>, &'static str> {
    let mut bytes = serde_json::to_vec(&value).map_err(|_| "cannot encode worker response")?;
    if bytes.len() >= MAX_FRAME_BYTES {
        return Err("worker response too large");
    }
    bytes.push(b'\n');
    Ok(bytes)
}

/// Own threads, rather than Tokio's blocking stdio pool, allow the process to
/// exit promptly when the host closes stdin while stdout is backpressured.
pub fn stdio<C: DeserializeOwned + Send + 'static>() -> (
    mpsc::Receiver<HostFrame<C>>,
    Output,
    watch::Receiver<StopReason>,
) {
    let (frames, incoming) = mpsc::channel(QUEUE_CAPACITY);
    let (stop, stopped) = watch::channel(StopReason::Running);
    std::thread::spawn(move || {
        let input = std::io::BufReader::with_capacity(4096, std::io::stdin().lock());
        read_frames(input, frames, stop);
    });
    let (sender, outgoing) = mpsc::channel(QUEUE_CAPACITY);
    std::thread::spawn(move || write_frames(std::io::stdout().lock(), outgoing));
    (incoming, Output { sender }, stopped)
}

fn read_frames<R: BufRead, C: DeserializeOwned>(
    mut input: R,
    frames: mpsc::Sender<HostFrame<C>>,
    stop: watch::Sender<StopReason>,
) {
    loop {
        match read_frame(&mut input) {
            Ok(Some(HostFrame::Shutdown {})) => {
                let _ = stop.send(StopReason::Shutdown);
                break;
            }
            Ok(Some(frame)) => {
                if frames.try_send(frame).is_err() {
                    let _ = stop.send(StopReason::Invalid);
                    break;
                }
            }
            Ok(None) => {
                let _ = stop.send(StopReason::Eof);
                break;
            }
            Err(_) => {
                let _ = stop.send(StopReason::Invalid);
                break;
            }
        }
    }
}

fn write_frames<W: Write>(mut output: W, mut outgoing: mpsc::Receiver<Outbound>) {
    while let Some(frame) = outgoing.blocking_recv() {
        let result = output
            .write_all(&frame.bytes)
            .and_then(|()| output.flush())
            .map_err(|_| "host output closed");
        let failed = result.is_err();
        let _ = frame.written.send(result);
        if failed {
            break;
        }
    }
}

pub fn read_frame<R: BufRead, C: DeserializeOwned>(
    reader: &mut R,
) -> Result<Option<HostFrame<C>>, &'static str> {
    let mut frame = Vec::new();
    loop {
        let chunk = reader.fill_buf().map_err(|_| "cannot read host frame")?;
        if chunk.is_empty() {
            return if frame.is_empty() {
                Ok(None)
            } else {
                Err("unterminated host frame")
            };
        }
        let newline = chunk.iter().position(|byte| *byte == b'\n');
        let length = newline.map_or(chunk.len(), |index| index + 1);
        if frame.len() + length > MAX_FRAME_BYTES {
            return Err("host frame too large");
        }
        frame.extend_from_slice(&chunk[..length]);
        reader.consume(length);
        if newline.is_some() {
            frame.pop();
            if frame.last() == Some(&b'\r') {
                frame.pop();
            }
            return serde_json::from_slice(&frame)
                .map(Some)
                .map_err(|_| "invalid host frame");
        }
    }
}

#[cfg(test)]
mod tests;
