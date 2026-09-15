//! Declarative package sources and bounded, credential-free HTTPS downloads.

use std::collections::HashSet;
use std::time::Duration;

use reqwest::{Client, Url};
use semver::Version;

use super::package::MAX_ARCHIVE_BYTES;
use super::protocol::{validate_plugin_id, DESKTOP_TARGETS};
use crate::plugin_config::DesktopPluginConfig;

const MAX_DECLARATIONS: usize = 8;
const MAX_SOURCE_URL_BYTES: usize = 2048;
const MAX_REDIRECT_URL_BYTES: usize = 8192;

pub(crate) fn validate_declarations(declarations: &[DesktopPluginConfig]) -> Result<(), String> {
    if declarations.len() > MAX_DECLARATIONS {
        return Err("too many desktop plugin declarations".into());
    }
    let mut ids = HashSet::new();
    for declaration in declarations {
        validate_plugin_id(&declaration.plugin_id)?;
        if !ids.insert(&declaration.plugin_id) {
            return Err("duplicate desktop plugin declaration".into());
        }
        if !declaration.extra.is_empty() {
            return Err("unknown desktop plugin declaration field".into());
        }
        if declaration.version.len() > 128 {
            return Err("desktop plugin version must be an exact semantic version".into());
        }
        let version = Version::parse(&declaration.version)
            .map_err(|_| "desktop plugin version must be an exact semantic version")?;
        if version.to_string() != declaration.version {
            return Err("desktop plugin version must be an exact semantic version".into());
        }
        if declaration.artifacts.is_empty() || declaration.artifacts.len() > DESKTOP_TARGETS.len() {
            return Err("desktop plugin artifacts must list supported desktop targets".into());
        }
        for (target, artifact) in &declaration.artifacts {
            if !DESKTOP_TARGETS.contains(&target.as_str()) {
                return Err("desktop plugin artifact target is unsupported".into());
            }
            if !artifact.extra.is_empty() {
                return Err("unknown desktop plugin artifact field".into());
            }
            source_url(&artifact.url, &DownloadPolicy::default())?;
            if artifact.sha256.len() != 64
                || !artifact
                    .sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                return Err("desktop plugin artifact SHA-256 must be lowercase hexadecimal".into());
            }
        }
    }
    Ok(())
}

struct DownloadPolicy {
    max_bytes: usize,
    max_redirects: usize,
    connect_timeout: Duration,
    total_timeout: Duration,
    /// Local deterministic fixtures cannot weaken the production URL policy.
    #[cfg(test)]
    allow_loopback_http: bool,
}

impl Default for DownloadPolicy {
    fn default() -> Self {
        Self {
            max_bytes: MAX_ARCHIVE_BYTES,
            max_redirects: 5,
            connect_timeout: Duration::from_secs(10),
            total_timeout: Duration::from_secs(60),
            #[cfg(test)]
            allow_loopback_http: false,
        }
    }
}

impl DownloadPolicy {
    fn https_only(&self) -> bool {
        #[cfg(test)]
        if self.allow_loopback_http {
            return false;
        }
        true
    }

    fn accepts_scheme(&self, url: &Url) -> bool {
        if url.scheme() == "https" {
            return true;
        }
        #[cfg(test)]
        if self.allow_loopback_http && url.scheme() == "http" {
            return matches!(url.host_str(), Some("127.0.0.1" | "[::1]"));
        }
        false
    }
}

fn unsafe_url_text(value: &str) -> bool {
    value
        .chars()
        .any(|character| character.is_whitespace() || character.is_control() || character == '\\')
}

fn authority_has_credentials(value: &str) -> bool {
    value
        .split_once("://")
        .map(|(_, authority)| authority)
        .or_else(|| value.strip_prefix("//"))
        .is_some_and(|authority| {
            authority
                .split(['/', '?', '#'])
                .next()
                .is_some_and(|authority| authority.contains('@'))
        })
}

fn validate_url(url: &Url, policy: &DownloadPolicy, allow_query: bool) -> Result<(), String> {
    if !policy.accepts_scheme(url)
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || (!allow_query && url.query().is_some())
    {
        return Err("desktop plugin source requires a credential-free HTTPS URL".into());
    }
    Ok(())
}

fn source_url(value: &str, policy: &DownloadPolicy) -> Result<Url, String> {
    if value.len() > MAX_SOURCE_URL_BYTES
        || unsafe_url_text(value)
        || authority_has_credentials(value)
        || !value.contains("://")
    {
        return Err("invalid desktop plugin source URL".into());
    }
    let url = Url::parse(value).map_err(|_| "invalid desktop plugin source URL")?;
    validate_url(&url, policy, false)?;
    Ok(url)
}

fn redirect_url(base: &Url, location: &str, policy: &DownloadPolicy) -> Result<Url, String> {
    if location.is_empty()
        || location.len() > MAX_REDIRECT_URL_BYTES
        || unsafe_url_text(location)
        || authority_has_credentials(location)
    {
        return Err("invalid desktop plugin download redirect".into());
    }
    let url = base
        .join(location)
        .map_err(|_| "invalid desktop plugin download redirect")?;
    // Release servers commonly redirect to a short-lived signed HTTPS URL.
    // Queries are accepted only on redirects and are never surfaced in errors.
    if url.as_str().len() > MAX_REDIRECT_URL_BYTES || validate_url(&url, policy, true).is_err() {
        return Err("invalid desktop plugin download redirect".into());
    }
    Ok(url)
}

pub(crate) async fn download_archive(url: &str) -> Result<Vec<u8>, String> {
    download_with_policy(url, &DownloadPolicy::default()).await
}

async fn download_with_policy(value: &str, policy: &DownloadPolicy) -> Result<Vec<u8>, String> {
    let mut url = source_url(value, policy)?;
    let deadline = tokio::time::Instant::now() + policy.total_timeout;
    let client = Client::builder()
        .https_only(policy.https_only())
        .redirect(reqwest::redirect::Policy::none())
        .referer(false)
        .no_proxy()
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .connect_timeout(policy.connect_timeout)
        .build()
        .map_err(|_| "desktop plugin download client could not be initialized")?;

    tokio::time::timeout_at(deadline, async {
        for redirects in 0..=policy.max_redirects {
            let mut response = client
                .get(url.clone())
                .send()
                .await
                .map_err(|_| "desktop plugin download request failed")?;
            if matches!(response.status().as_u16(), 301 | 302 | 303 | 307 | 308) {
                if redirects == policy.max_redirects {
                    return Err("desktop plugin download exceeded redirect limit".into());
                }
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or("invalid desktop plugin download redirect")?;
                url = redirect_url(&url, location, policy)?;
                continue;
            }
            if !response.status().is_success() {
                // Do not read arbitrary error bodies or report upstream URLs.
                return Err("desktop plugin download returned an unsuccessful response".into());
            }
            if response
                .content_length()
                .is_some_and(|length| length > policy.max_bytes as u64)
            {
                return Err("desktop plugin archive exceeds size limit".into());
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| "desktop plugin archive transfer failed")?
            {
                if bytes
                    .len()
                    .checked_add(chunk.len())
                    .is_none_or(|length| length > policy.max_bytes)
                {
                    return Err("desktop plugin archive exceeds size limit".into());
                }
                bytes.extend_from_slice(&chunk);
            }
            if bytes.is_empty() {
                return Err("desktop plugin archive is empty".into());
            }
            return Ok(bytes);
        }
        Err("desktop plugin download exceeded redirect limit".into())
    })
    .await
    .map_err(|_| "desktop plugin download exceeded time limit".to_string())?
}

#[cfg(test)]
#[path = "download_tests.rs"]
mod tests;
