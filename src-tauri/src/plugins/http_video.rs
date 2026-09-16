//! Bounded direct-media HTTP transfers. No application configuration or credentials.

use super::generation::GenerationLease;
use super::media::MediaFile;
use super::protocol::HttpMediaKind;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tokio::time::{self, Instant};

pub(crate) const MAX_CLIP_BYTES: u64 = 256 * 1024 * 1024;
pub(crate) const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;
const WRITE_CHUNK_BYTES: usize = 64 * 1024;
const IMAGE_SIGNATURE_BYTES: usize = 12;

pub(super) fn media_byte_limit(kind: HttpMediaKind) -> u64 {
    match kind {
        HttpMediaKind::Video => MAX_CLIP_BYTES,
        HttpMediaKind::Jpeg | HttpMediaKind::Png | HttpMediaKind::Webp => MAX_IMAGE_BYTES,
    }
}

pub(super) fn media_content_type(kind: HttpMediaKind) -> &'static str {
    match kind {
        HttpMediaKind::Video => "video/mp4",
        HttpMediaKind::Jpeg => "image/jpeg",
        HttpMediaKind::Png => "image/png",
        HttpMediaKind::Webp => "image/webp",
    }
}

pub(super) fn media_extension(kind: HttpMediaKind) -> &'static str {
    match kind {
        HttpMediaKind::Video => "mp4",
        HttpMediaKind::Jpeg => "jpg",
        HttpMediaKind::Png => "png",
        HttpMediaKind::Webp => "webp",
    }
}

fn matches_image_signature(kind: HttpMediaKind, prefix: &[u8]) -> bool {
    match kind {
        HttpMediaKind::Video => true,
        HttpMediaKind::Jpeg => prefix.starts_with(&[0xff, 0xd8, 0xff]),
        HttpMediaKind::Png => prefix.starts_with(b"\x89PNG\r\n\x1a\n"),
        HttpMediaKind::Webp => prefix.starts_with(b"RIFF") && prefix.get(8..12) == Some(b"WEBP"),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VideoError {
    Cancelled,
    Deadline,
    Network,
    Http,
    Empty,
    Oversized,
    InvalidMedia,
    Storage,
}

#[derive(Clone)]
pub(super) struct TransferPolicy {
    pub attempts: usize,
    pub delays: Vec<Duration>,
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    pub total_timeout: Duration,
    pub max_bytes: u64,
}

impl Default for TransferPolicy {
    fn default() -> Self {
        Self {
            attempts: 8,
            delays: [1, 2, 3, 4, 5, 5, 5]
                .into_iter()
                .map(Duration::from_secs)
                .collect(),
            connect_timeout: Duration::from_secs(15),
            read_timeout: Duration::from_secs(60),
            total_timeout: Duration::from_secs(10 * 60),
            max_bytes: MAX_CLIP_BYTES,
        }
    }
}

#[derive(Clone)]
pub(super) struct VideoTransfer {
    client: reqwest::Client,
    policy: TransferPolicy,
}

impl VideoTransfer {
    pub(super) fn new(policy: TransferPolicy) -> Result<Self, VideoError> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(policy.connect_timeout)
            .read_timeout(policy.read_timeout)
            .build()
            .map_err(|_| VideoError::Network)?;
        Ok(Self { client, policy })
    }

    pub(super) async fn download(
        &self,
        url: reqwest::Url,
        kind: HttpMediaKind,
        output: Arc<MediaFile>,
        lease: &GenerationLease,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<u64, VideoError> {
        let deadline = Instant::now() + self.policy.total_timeout;
        let mut last = VideoError::Network;
        for attempt in 0..self.policy.attempts {
            check_active(lease, &shutdown, deadline)?;
            // Disk tasks are awaited to completion even after cancellation. The
            // owned file cannot be removed while a Windows write still holds it.
            let file = output.clone();
            tokio::task::spawn_blocking(move || file.reset())
                .await
                .map_err(|_| VideoError::Storage)??;
            let result = self
                .attempt(&url, kind, &output, lease, &mut shutdown, deadline)
                .await;
            match result {
                Ok(bytes) => {
                    let file = output.clone();
                    tokio::task::spawn_blocking(move || file.flush())
                        .await
                        .map_err(|_| VideoError::Storage)??;
                    check_active(lease, &shutdown, deadline)?;
                    return Ok(bytes);
                }
                Err(AttemptError::Terminal(error)) => return Err(error),
                Err(AttemptError::Retry(error)) => last = error,
            }
            if attempt + 1 < self.policy.attempts {
                let delay = self
                    .policy
                    .delays
                    .get(attempt)
                    .or_else(|| self.policy.delays.last())
                    .copied()
                    .unwrap_or_default();
                cancellable(lease, &mut shutdown, deadline, time::sleep(delay)).await?;
            }
        }
        Err(last)
    }

    async fn attempt(
        &self,
        url: &reqwest::Url,
        kind: HttpMediaKind,
        output: &Arc<MediaFile>,
        lease: &GenerationLease,
        shutdown: &mut watch::Receiver<bool>,
        deadline: Instant,
    ) -> Result<u64, AttemptError> {
        let mut response = cancellable(
            lease,
            shutdown,
            deadline,
            self.client.get(url.clone()).send(),
        )
        .await
        .map_err(AttemptError::Terminal)?
        .map_err(|_| AttemptError::Retry(VideoError::Network))?;
        let status = response.status();
        if !status.is_success() {
            // An error body contains no useful native UI data; do not read or log
            // arbitrary upstream text, URLs, cookies, or proxy error pages.
            return Err(if retryable_status(status.as_u16()) {
                AttemptError::Retry(VideoError::Http)
            } else {
                AttemptError::Terminal(VideoError::Http)
            });
        }
        let max_bytes = self.policy.max_bytes.min(media_byte_limit(kind));
        if response
            .content_length()
            .is_some_and(|bytes| bytes > max_bytes)
        {
            return Err(AttemptError::Terminal(VideoError::Oversized));
        }
        let mut received = 0u64;
        let mut prefix = Vec::with_capacity(IMAGE_SIGNATURE_BYTES);
        while let Some(chunk) = cancellable(lease, shutdown, deadline, response.chunk())
            .await
            .map_err(AttemptError::Terminal)?
            .map_err(|_| AttemptError::Retry(VideoError::Network))?
        {
            let next = received
                .checked_add(chunk.len() as u64)
                .filter(|size| *size <= max_bytes)
                .ok_or(AttemptError::Terminal(VideoError::Oversized))?;
            if kind != HttpMediaKind::Video && prefix.len() < IMAGE_SIGNATURE_BYTES {
                let remaining = IMAGE_SIGNATURE_BYTES - prefix.len();
                prefix.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
                if prefix.len() == IMAGE_SIGNATURE_BYTES && !matches_image_signature(kind, &prefix)
                {
                    return Err(AttemptError::Terminal(VideoError::InvalidMedia));
                }
            }
            for bytes in chunk.chunks(WRITE_CHUNK_BYTES) {
                check_active(lease, shutdown, deadline).map_err(AttemptError::Terminal)?;
                let file = output.clone();
                let bytes = bytes.to_vec();
                let length = bytes.len() as u64;
                let offset = received;
                tokio::task::spawn_blocking(move || file.write(offset, &bytes))
                    .await
                    .map_err(|_| AttemptError::Terminal(VideoError::Storage))?
                    .map_err(AttemptError::Terminal)?;
                received += length;
            }
            debug_assert_eq!(received, next);
        }
        if received == 0 {
            Err(AttemptError::Retry(VideoError::Empty))
        } else if !matches_image_signature(kind, &prefix) {
            Err(AttemptError::Terminal(VideoError::InvalidMedia))
        } else {
            Ok(received)
        }
    }
}

enum AttemptError {
    Retry(VideoError),
    Terminal(VideoError),
}

fn retryable_status(status: u16) -> bool {
    matches!(status, 400 | 404 | 408 | 425 | 429 | 500..=599)
}

fn check_active(
    lease: &GenerationLease,
    shutdown: &watch::Receiver<bool>,
    deadline: Instant,
) -> Result<(), VideoError> {
    if !lease.is_active() || *shutdown.borrow() {
        Err(VideoError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(VideoError::Deadline)
    } else {
        Ok(())
    }
}

async fn cancellable<T>(
    lease: &GenerationLease,
    shutdown: &mut watch::Receiver<bool>,
    deadline: Instant,
    operation: impl std::future::Future<Output = T>,
) -> Result<T, VideoError> {
    check_active(lease, shutdown, deadline)?;
    tokio::select! {
        biased;
        _ = lease.cancelled() => Err(VideoError::Cancelled),
        _ = shutdown.wait_for(|closing| *closing) => Err(VideoError::Cancelled),
        result = time::timeout_at(deadline, operation) => result.map_err(|_| VideoError::Deadline),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_policy_matches_completed_frigate_recordings() {
        for status in [400, 404, 408, 425, 429, 500, 503, 599] {
            assert!(retryable_status(status));
        }
        for status in [200, 206, 301, 302, 401, 403, 410] {
            assert!(!retryable_status(status));
        }
        let policy = TransferPolicy::default();
        assert_eq!(policy.attempts, 8);
        assert_eq!(
            policy
                .delays
                .iter()
                .map(Duration::as_secs)
                .collect::<Vec<_>>(),
            [1, 2, 3, 4, 5, 5, 5]
        );
    }
}
