use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use url::Url;

pub const MAX_ENTITIES: usize = 32;
pub const MAX_ACTION_ENTITIES: usize = 16;
pub const MAX_MEDIA_PLAYER_ENTITIES: usize = 4;
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
    #[serde(default)]
    pub action_entities: String,
    #[serde(default)]
    pub media_player_entities: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Secrets {
    pub ha_token: String,
}

pub struct Validated {
    pub base: Url,
    pub entities: Vec<String>,
    pub action_entities: Vec<String>,
    pub media_player_entities: Vec<String>,
    pub token: String,
}

#[derive(Clone, Copy)]
pub enum Operation {
    Press,
    Activate,
    Play,
    Pause,
    Stop,
}

impl Operation {
    pub fn label(self) -> &'static str {
        match self {
            Self::Press => "Press ",
            Self::Activate => "Activate ",
            Self::Play => "Play ",
            Self::Pause => "Pause ",
            Self::Stop => "Stop ",
        }
    }

    pub fn allows_unknown(self) -> bool {
        matches!(self, Self::Press | Self::Activate)
    }

    fn path(self) -> &'static str {
        match self {
            Self::Press => "api/services/button/press",
            Self::Activate => "api/services/scene/turn_on",
            Self::Play => "api/services/media_player/media_play",
            Self::Pause => "api/services/media_player/media_pause",
            Self::Stop => "api/services/media_player/media_stop",
        }
    }
}

/// Derived only from validated configuration; the host never supplies a target
/// or service through action parameters.
#[derive(Clone)]
pub struct ConfiguredAction {
    pub id: String,
    pub entity: String,
    pub operation: Operation,
}

pub fn configured_actions(actions: &[String], media_players: &[String]) -> Vec<ConfiguredAction> {
    let mut configured = Vec::new();
    for (index, entity) in actions.iter().enumerate() {
        let operation = match entity.split_once('.') {
            Some(("button", _)) => Operation::Press,
            Some(("scene", _)) => Operation::Activate,
            _ => continue,
        };
        configured.push(ConfiguredAction {
            id: format!("ha-action-{index}"),
            entity: entity.clone(),
            operation,
        });
    }
    for (index, entity) in media_players.iter().enumerate() {
        for (verb, operation) in [
            ("play", Operation::Play),
            ("pause", Operation::Pause),
            ("stop", Operation::Stop),
        ] {
            configured.push(ConfiguredAction {
                id: format!("ha-media-{index}-{verb}"),
                entity: entity.clone(),
                operation,
            });
        }
    }
    configured
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
        let mut entities = entity_list(&self.values.watch_entities)?;
        let action_entities = entity_list(&self.values.action_entities)?;
        if action_entities.len() > MAX_ACTION_ENTITIES
            || action_entities
                .iter()
                .any(|entity| !matches!(entity.split_once('.'), Some(("button" | "scene", _))))
        {
            return Err("invalid HA action entities");
        }
        let media_player_entities = entity_list(&self.values.media_player_entities)?;
        if media_player_entities.len() > MAX_MEDIA_PLAYER_ENTITIES
            || media_player_entities
                .iter()
                .any(|entity| !entity.starts_with("media_player."))
        {
            return Err("invalid HA media player entities");
        }
        for entity in action_entities.iter().chain(&media_player_entities) {
            if !entities.contains(entity) {
                entities.push(entity.clone());
            }
        }
        if entities.len() > MAX_ENTITIES {
            return Err("too many watched entities");
        }
        Ok(Validated {
            base: base_url(&self.values.ha_base_url)?,
            entities,
            action_entities,
            media_player_entities,
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

    pub fn actions(&self) -> Vec<ConfiguredAction> {
        configured_actions(&self.action_entities, &self.media_player_entities)
    }

    pub fn service_url(&self, operation: Operation) -> Url {
        self.base
            .join(operation.path())
            .expect("validated URL and fixed service path")
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

    #[test]
    fn action_opt_in_preserves_order_deduplicates_and_only_maps_fixed_services() {
        let mut config = configuration(
            "https://ha.example:8443/proxy/ha",
            "sensor.a,scene.night,button.first",
        );
        assert!(config.values.action_entities.is_empty());
        config.values.action_entities =
            "button.second,scene.night,button.second,button.first".into();
        let config = config.validate().unwrap();
        assert_eq!(
            config.entities,
            ["sensor.a", "scene.night", "button.first", "button.second"]
        );
        assert_eq!(
            config.action_entities,
            ["button.second", "scene.night", "button.first"]
        );
        assert_eq!(
            config.service_url(Operation::Press).as_str(),
            "https://ha.example:8443/proxy/ha/api/services/button/press"
        );
        assert_eq!(
            config.service_url(Operation::Activate).as_str(),
            "https://ha.example:8443/proxy/ha/api/services/scene/turn_on"
        );
        assert_eq!(config.actions().len(), 3);
        for actions in [
            "switch.a",
            "input_boolean.do_not_supply_charger",
            "button.*",
            "scene.a/b",
            "scene.a.b",
            "button.Upper",
            "button.",
        ] {
            let mut config = configuration("http://localhost", "");
            config.values.action_entities = actions.into();
            assert!(config.validate().is_err());
        }
    }

    #[test]
    fn action_and_union_limits_are_independent() {
        let list = |domain: &str, count| {
            (0..count)
                .map(|index| format!("{domain}.e{index}"))
                .collect::<Vec<_>>()
                .join(",")
        };
        let mut config = configuration("http://localhost", &list("sensor", 16));
        config.values.action_entities = list("button", 16);
        assert_eq!(config.validate().unwrap().entities.len(), 32);
        let mut config = configuration("http://localhost", &list("sensor", 17));
        config.values.action_entities = list("button", 16);
        assert!(config.validate().is_err());
        let mut config = configuration("http://localhost", "");
        config.values.action_entities = list("scene", 17);
        assert!(config.validate().is_err());
    }

    #[test]
    fn media_selection_preserves_union_order_existing_indices_and_fixed_service_mapping() {
        let mut config = configuration(
            "https://ha.example:8443/proxy/ha",
            "media_player.den,sensor.a,button.first",
        );
        assert!(config.values.media_player_entities.is_empty());
        config.values.action_entities = "scene.evening,button.first,scene.evening".into();
        config.values.media_player_entities =
            "media_player.den,\nmedia_player.office,media_player.den".into();
        let config = config.validate().unwrap();
        assert_eq!(
            config.entities,
            [
                "media_player.den",
                "sensor.a",
                "button.first",
                "scene.evening",
                "media_player.office"
            ]
        );
        assert_eq!(config.action_entities, ["scene.evening", "button.first"]);
        assert_eq!(
            config.media_player_entities,
            ["media_player.den", "media_player.office"]
        );
        let actions = config.actions();
        assert_eq!(actions.len(), 8);
        assert_eq!(actions[0].id, "ha-action-0");
        assert_eq!(actions[0].entity, "scene.evening");
        assert_eq!(
            config.service_url(actions[0].operation).as_str(),
            "https://ha.example:8443/proxy/ha/api/services/scene/turn_on"
        );
        assert_eq!(actions[1].id, "ha-action-1");
        assert_eq!(actions[1].entity, "button.first");
        assert_eq!(
            config.service_url(actions[1].operation).as_str(),
            "https://ha.example:8443/proxy/ha/api/services/button/press"
        );
        for (index, entity) in config.media_player_entities.iter().enumerate() {
            for (offset, verb) in ["play", "pause", "stop"].into_iter().enumerate() {
                let action = &actions[2 + index * 3 + offset];
                assert_eq!(action.id, format!("ha-media-{index}-{verb}"));
                assert_eq!(&action.entity, entity);
                assert_eq!(
                    config.service_url(action.operation).as_str(),
                    format!(
                        "https://ha.example:8443/proxy/ha/api/services/media_player/media_{verb}"
                    )
                );
            }
        }
    }

    #[test]
    fn media_domain_count_and_combined_watch_limits_are_enforced() {
        for selection in [
            "button.a",
            "scene.a",
            "media_playerx.a",
            "media_player.*",
            "media_player.Upper",
            "media_player.a/b",
            "media_player.a.b",
            "media_player.",
        ] {
            let mut config = configuration("http://localhost", "");
            config.values.media_player_entities = selection.into();
            assert!(config.validate().is_err());
        }
        let list = |domain: &str, count| {
            (0..count)
                .map(|index| format!("{domain}.e{index}"))
                .collect::<Vec<_>>()
                .join(",")
        };
        let mut config = configuration("http://localhost", "");
        config.values.media_player_entities = list("media_player", 5);
        assert!(config.validate().is_err());
        let mut config = configuration("http://localhost", &list("sensor", 12));
        config.values.action_entities = list("button", 16);
        config.values.media_player_entities =
            format!("{},media_player.e0", list("media_player", 4));
        let config = config.validate().unwrap();
        assert_eq!(config.entities.len(), MAX_ENTITIES);
        assert_eq!(
            config.media_player_entities.len(),
            MAX_MEDIA_PLAYER_ENTITIES
        );
        assert_eq!(config.actions().len(), 28);
        let mut config = configuration("http://localhost", &list("sensor", 13));
        config.values.action_entities = list("scene", 16);
        config.values.media_player_entities = list("media_player", 4);
        assert!(config.validate().is_err());
        let config = configuration("http://localhost", "media_player.read_only")
            .validate()
            .unwrap();
        assert!(config.media_player_entities.is_empty());
        assert!(config.actions().is_empty());
    }
}
