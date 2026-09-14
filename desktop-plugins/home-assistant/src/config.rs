use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use url::Url;

pub const MAX_ENTITIES: usize = 32;
pub const MAX_CONFIGURATION_BYTES: usize = 32 * 1024;

// Configuration and credentials deliberately have no Debug implementation.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    pub revision: String,
    pub values: Values,
    pub secrets: Secrets,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Values {
    pub ha_base_url: String,
    #[serde(default)]
    pub watch_entities: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Secrets {
    pub ha_token: String,
}

pub struct Validated {
    pub base: Url,
    pub entities: Vec<String>,
    pub token: String,
}

impl Configuration {
    pub fn validate(self) -> Result<Validated, &'static str> {
        let revision = &self.revision;
        if revision.is_empty()
            || revision.len() > 128
            || !revision.as_bytes()[0].is_ascii_alphanumeric()
            || !revision
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
            || serde_json::to_vec(&self)
                .map_err(|_| "invalid configuration")?
                .len()
                > MAX_CONFIGURATION_BYTES
        {
            return Err("invalid configuration");
        }
        let token = &self.secrets.ha_token;
        if token.is_empty() || token.len() > 4096 || !token.bytes().all(|b| b.is_ascii_graphic()) {
            return Err("invalid HA token");
        }
        Ok(Validated {
            base: base_url(&self.values.ha_base_url)?,
            entities: entity_list(&self.values.watch_entities)?,
            token: self.secrets.ha_token,
        })
    }
}

pub fn base_url(value: &str) -> Result<Url, &'static str> {
    if value.is_empty()
        || value.len() > 2048
        || value.trim() != value
        || value
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '\\')
    {
        return Err("invalid HA base URL");
    }
    let (_, authority_path) = value.split_once("://").ok_or("invalid HA base URL")?;
    let (authority, path) = authority_path
        .split_once('/')
        .unwrap_or((authority_path, ""));
    let mut url = Url::parse(value).map_err(|_| "invalid HA base URL")?;
    if authority.is_empty()
        || authority.contains('@')
        || !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !safe_path(path)
    {
        return Err("invalid HA base URL");
    }
    if !url.path().ends_with('/') {
        url.path_segments_mut()
            .map_err(|_| "invalid HA base URL")?
            .push("");
    }
    Ok(url)
}

fn safe_path(path: &str) -> bool {
    path.split('/').all(|part| {
        let lower = part.to_ascii_lowercase();
        !matches!(part, "." | "..")
            && !["%2e", "%2f", "%5c", "%25"]
                .iter()
                .any(|value| lower.contains(value))
            && percent_decode_str(part).decode_utf8().is_ok_and(|decoded| {
                !decoded
                    .chars()
                    .any(|c| c.is_control() || matches!(c, '%' | '/' | '\\'))
            })
    })
}

fn entity_list(value: &str) -> Result<Vec<String>, &'static str> {
    if value.len() > 4096 {
        return Err("entity list is too large");
    }
    let mut seen = HashSet::new();
    let mut entities = Vec::new();
    for entity in value
        .split([',', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let parts: Vec<_> = entity.split('.').collect();
        if entity.len() > 128
            || parts.len() != 2
            || parts.iter().any(|part| {
                part.is_empty()
                    || !part
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            })
        {
            return Err("invalid HA entity ID");
        }
        if seen.insert(entity.to_owned()) {
            entities.push(entity.to_owned());
            if entities.len() > MAX_ENTITIES {
                return Err("too many watched entities");
            }
        }
    }
    Ok(entities)
}

impl Validated {
    pub fn websocket_url(&self) -> Url {
        let mut url = self.base.join("api/websocket").expect("validated URL");
        let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
        url.set_scheme(scheme).expect("matching WebSocket scheme");
        url
    }

    pub fn state_url(&self, entity: &str) -> Url {
        let mut url = self.base.join("api/states/").expect("validated URL");
        url.path_segments_mut()
            .expect("validated URL")
            .pop_if_empty()
            .push(entity);
        url
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn configuration(base: &str, entities: &str) -> Configuration {
        serde_json::from_value(json!({"revision":"r1","values":{
            "ha_base_url":base,"watch_entities":entities},"secrets":{"ha_token":"fixture-token"}}))
        .unwrap()
    }

    #[test]
    fn direct_prefix_port_and_literal_entities_are_preserved() {
        let value = configuration(
            "https://ha.example:8443/proxy/ha",
            "sensor.temperature,\ninput_boolean.do_not_supply_charger,sensor.temperature",
        )
        .validate()
        .unwrap();
        assert_eq!(
            value.websocket_url().as_str(),
            "wss://ha.example:8443/proxy/ha/api/websocket"
        );
        assert_eq!(
            value.state_url(&value.entities[1]).as_str(),
            "https://ha.example:8443/proxy/ha/api/states/input_boolean.do_not_supply_charger"
        );
        assert_eq!(
            value.entities,
            ["sensor.temperature", "input_boolean.do_not_supply_charger"]
        );
        assert!(configuration("http://127.0.0.1:8123", "")
            .validate()
            .unwrap()
            .entities
            .is_empty());
        let encoded = configuration("https://ha.example/space%20prefix", "sensor.a")
            .validate()
            .unwrap();
        assert_eq!(
            encoded.websocket_url().as_str(),
            "wss://ha.example/space%20prefix/api/websocket"
        );
    }

    #[test]
    fn ambiguous_urls_and_unbounded_or_invalid_values_fail_privately() {
        for value in [
            "https:///host",
            "https:host",
            "https://user:pass@host",
            "https://host?token=x",
            "https://host/#x",
            "https://host/a/../b",
            "https://host/%2e",
            "https://host/%2F",
            "https://host/%252e",
            "https://host/%5c",
            "https://host/%00",
            "https://host/%ff",
            "https://host/%invalid",
            "https://host/with space",
            "https://host/\\path",
            "http://host\n",
            "ftp://host",
        ] {
            assert!(base_url(value).is_err());
        }
        for value in [
            "sensor.a/b",
            "sensor.Upper",
            "do_not_supply_charger",
            "sensor.a.b",
            ".a",
            "a.",
            "sensor.a\0",
        ] {
            assert!(configuration("http://localhost", value).validate().is_err());
        }
        let entities = (0..33)
            .map(|n| format!("sensor.e{n}"))
            .collect::<Vec<_>>()
            .join(",");
        assert!(configuration("http://localhost", &entities)
            .validate()
            .is_err());
        for token in ["", "contains whitespace", "line\nbreak", "é"] {
            let mut value = configuration("http://localhost", "");
            value.secrets.ha_token = token.into();
            assert!(value.validate().is_err());
        }
    }
}
