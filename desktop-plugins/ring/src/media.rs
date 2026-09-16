use crate::config::Configuration;
use inverter_camera_common::http_url;
use url::Url;

// No Debug: templates can contain private query credentials.
pub struct Snapshot {
    base: Url,
    template: String,
    pub kind: String,
}

impl Snapshot {
    pub fn from_configuration(config: &Configuration) -> Result<Option<Self>, &'static str> {
        if !matches!(
            config.values.snapshot_media_kind.as_str(),
            "jpeg" | "png" | "webp" | "video"
        ) {
            return Err("invalid snapshot media kind");
        }
        let base = config
            .values
            .snapshot_base_url
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(|s| -> Result<Url, &'static str> {
                let url = http_url(s, false)?;
                validate_path(s)?;
                Ok(url)
            })
            .transpose()?;
        if let Some(token) = &config.secrets.snapshot_bearer_token {
            if token.len() > 4096
                || token.chars().any(|c| c.is_control() || !c.is_ascii())
                || token.trim() != token
            {
                return Err("invalid snapshot credentials");
            }
        }
        let Some(template) = config
            .secrets
            .snapshot_url_template
            .as_deref()
            .filter(|s| !s.trim().is_empty())
        else {
            if config
                .secrets
                .snapshot_bearer_token
                .as_deref()
                .is_some_and(|s| !s.is_empty())
            {
                return Err("snapshot credentials require a template");
            }
            return Ok(None);
        };
        let base = base.ok_or("snapshot template requires a base URL")?;
        if template.len() > 2048
            || template.trim() != template
            || template.chars().any(char::is_control)
        {
            return Err("invalid snapshot template");
        }
        let authority = template
            .split_once("://")
            .ok_or("invalid snapshot template")?
            .1
            .split(['/', '?', '#'])
            .next()
            .unwrap_or_default();
        if authority.contains(['{', '}']) {
            return Err("invalid snapshot template authority");
        }
        let snapshot = Self {
            base,
            template: template.to_owned(),
            kind: config.values.snapshot_media_kind.clone(),
        };
        snapshot
            .url("location", "device", "motion")
            .ok_or("invalid snapshot template")?;
        Ok(Some(snapshot))
    }

    pub fn url(&self, location: &str, device: &str, event: &str) -> Option<String> {
        if !crate::events::valid_ring_identity(location)
            || !crate::events::valid_ring_identity(device)
            || !matches!(event, "motion" | "ding")
        {
            return None;
        }
        let value = self
            .template
            .replace("{location_id}", &component(location))
            .replace("{device_id}", &component(device))
            .replace("{event}", event);
        if value.contains(['{', '}']) {
            return None;
        }
        let url = http_url(&value, true).ok()?;
        validate_path(&value).ok()?;
        let prefix = self.base.path().trim_end_matches('/');
        if url.origin() != self.base.origin() || !url.path().starts_with(&format!("{prefix}/")) {
            return None;
        }
        Some(url.into())
    }
}

fn component(value: &str) -> String {
    let mut result = String::new();
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            result.push(byte as char);
        } else {
            result.push('%');
            result.push(HEX[(byte >> 4) as usize] as char);
            result.push(HEX[(byte & 15) as usize] as char);
        }
    }
    result
}

fn validate_path(value: &str) -> Result<(), &'static str> {
    let tail = value.split_once("://").ok_or("invalid snapshot URL")?.1;
    let path = tail
        .split_once('/')
        .map(|(_, p)| p.split(['?', '#']).next().unwrap_or_default())
        .unwrap_or_default();
    for segment in path.split('/') {
        let lower = segment.to_ascii_lowercase();
        if matches!(segment, "." | "..")
            || ["%2e", "%2f", "%5c", "%25"]
                .iter()
                .any(|bad| lower.contains(bad))
        {
            return Err("invalid snapshot path");
        }
        let mut decoded = Vec::new();
        let mut bytes = segment.bytes();
        while let Some(byte) = bytes.next() {
            if byte == b'%' {
                let hi = bytes
                    .next()
                    .and_then(|b| (b as char).to_digit(16))
                    .ok_or("invalid snapshot path")?;
                let lo = bytes
                    .next()
                    .and_then(|b| (b as char).to_digit(16))
                    .ok_or("invalid snapshot path")?;
                decoded.push((hi * 16 + lo) as u8);
            } else {
                decoded.push(byte);
            }
        }
        if std::str::from_utf8(&decoded).map_or(true, |s| s.chars().any(char::is_control)) {
            return Err("invalid snapshot path");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    fn config() -> Value {
        json!({"revision":"test","values":{"mqtt_host":"localhost","snapshot_base_url":"https://ha.test:8123/api/camera_proxy"},"secrets":{"snapshot_url_template":"https://ha.test:8123/api/camera_proxy/{device_id}?event={event}&site={location_id}","snapshot_bearer_token":"private-token"}})
    }
    #[test]
    fn configured_origin_path_and_query_are_preserved_without_transmitting_token() {
        let c: Configuration = serde_json::from_value(config()).unwrap();
        let snapshot = Snapshot::from_configuration(&c).unwrap().unwrap();
        assert_eq!(
            snapshot.url("home east", "camera.front", "ding").unwrap(),
            "https://ha.test:8123/api/camera_proxy/camera.front?event=ding&site=home%20east"
        );
        assert!(snapshot.url("home", "..", "motion").is_none());
        assert!(snapshot.url("home", "%2fsecret", "motion").is_none());
    }
    #[test]
    fn unsafe_templates_and_tokens_fail_closed() {
        for template in [
            "https://evil.test/api/camera_proxy/{device_id}",
            "https://ha.test:8123/api/camera_proxy-other/{device_id}",
            "https://ha.test:8123/api/camera_proxy/../private",
            "https://ha.test:8123/api/camera_proxy/%252e",
            "https://ha.test:8123/api/camera_proxy/{unknown}",
            "https://{device_id}/api/camera_proxy/x",
            "https://u:p@ha.test:8123/api/camera_proxy/x",
            "https://ha.test:8123/api/camera_proxy/x#fragment",
        ] {
            let mut value = config();
            value["secrets"]["snapshot_url_template"] = json!(template);
            let c: Configuration = serde_json::from_value(value).unwrap();
            assert!(Snapshot::from_configuration(&c).is_err());
        }
        for token in [
            "secret\r\nX: injected".into(),
            "x".repeat(4097),
            " token".into(),
            "é".into(),
        ] {
            let mut value = config();
            value["secrets"]["snapshot_bearer_token"] = json!(token);
            assert!(serde_json::from_value::<Configuration>(value)
                .unwrap()
                .validate()
                .is_err());
        }
    }
    #[test]
    fn all_media_kinds_are_explicit_and_absent_template_is_notification_only() {
        for kind in ["jpeg", "png", "webp", "video"] {
            let mut value = config();
            value["values"]["snapshot_media_kind"] = json!(kind);
            assert_eq!(
                Snapshot::from_configuration(&serde_json::from_value(value).unwrap())
                    .unwrap()
                    .unwrap()
                    .kind,
                kind
            );
        }
        let mut value = config();
        value["values"]["snapshot_media_kind"] = json!("svg");
        assert!(serde_json::from_value::<Configuration>(value)
            .unwrap()
            .validate()
            .is_err());
        let value = json!({"revision":"test","values":{"mqtt_host":"localhost"},"secrets":{}});
        assert!(
            Snapshot::from_configuration(&serde_json::from_value(value).unwrap())
                .unwrap()
                .is_none()
        );
    }
}
