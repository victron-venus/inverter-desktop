use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use url::Url;

pub const MAX_ENTITIES: usize = 32;
pub const MAX_ACTION_ENTITIES: usize = 16;
pub const MAX_MEDIA_PLAYER_ENTITIES: usize = 4;
pub const MAX_BINARY_ENTITIES: usize = 8;
pub const MAX_COVER_ENTITIES: usize = 4;
pub const MAX_NUMBER_ENTITIES: usize = 4;
pub const MAX_COVER_POSITION_ENTITIES: usize = 4;
pub const MAX_DISCOVERY_PREFIXES: usize = 8;
pub const MAX_DISCOVERY_PREFIX_BYTES: usize = 1024;
pub const MAX_ACTION_BUTTONS: usize = 31;
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
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub binary_entities: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cover_entities: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub number_entities: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cover_position_entities: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub discovery_prefixes: String,
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
    pub binary_entities: Vec<String>,
    pub cover_entities: Vec<String>,
    pub number_entities: Vec<String>,
    pub cover_position_entities: Vec<String>,
    pub discovery_prefixes: Vec<String>,
    pub token: String,
}

#[derive(Clone, Copy)]
pub enum BinaryDomain {
    Switch,
    InputBoolean,
    Light,
}

impl BinaryDomain {
    fn from_entity(entity: &str) -> Option<Self> {
        match entity.split_once('.') {
            Some(("switch", _)) => Some(Self::Switch),
            Some(("input_boolean", _)) => Some(Self::InputBoolean),
            Some(("light", _)) => Some(Self::Light),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
pub enum Operation {
    Press,
    Activate,
    Play,
    Pause,
    Stop,
    TurnOn(BinaryDomain),
    TurnOff(BinaryDomain),
    OpenCover,
    CloseCover,
    StopCover,
    SetNumber,
    SetCoverPosition,
}

impl Operation {
    pub fn label(self) -> &'static str {
        match self {
            Self::Press => "Press ",
            Self::Activate => "Activate ",
            Self::Play => "Play ",
            Self::Pause => "Pause ",
            Self::Stop => "Stop ",
            Self::TurnOn(_) => "Turn on ",
            Self::TurnOff(_) => "Turn off ",
            Self::OpenCover => "Open ",
            Self::CloseCover => "Close ",
            Self::StopCover => "Stop ",
            Self::SetNumber => "Set ",
            Self::SetCoverPosition => "Set position ",
        }
    }

    pub fn allows_unknown(self) -> bool {
        matches!(self, Self::Press | Self::Activate)
    }

    pub fn requires_binary_state(self) -> bool {
        matches!(self, Self::TurnOn(_) | Self::TurnOff(_))
    }

    pub fn required_cover_feature(self) -> Option<u64> {
        match self {
            Self::OpenCover => Some(1),
            Self::CloseCover => Some(2),
            Self::StopCover => Some(8),
            _ => None,
        }
    }

    fn path(self) -> &'static str {
        match self {
            Self::Press => "api/services/button/press",
            Self::Activate => "api/services/scene/turn_on",
            Self::Play => "api/services/media_player/media_play",
            Self::Pause => "api/services/media_player/media_pause",
            Self::Stop => "api/services/media_player/media_stop",
            Self::TurnOn(BinaryDomain::Switch) => "api/services/switch/turn_on",
            Self::TurnOff(BinaryDomain::Switch) => "api/services/switch/turn_off",
            Self::TurnOn(BinaryDomain::InputBoolean) => "api/services/input_boolean/turn_on",
            Self::TurnOff(BinaryDomain::InputBoolean) => "api/services/input_boolean/turn_off",
            Self::TurnOn(BinaryDomain::Light) => "api/services/light/turn_on",
            Self::TurnOff(BinaryDomain::Light) => "api/services/light/turn_off",
            Self::OpenCover => "api/services/cover/open_cover",
            Self::CloseCover => "api/services/cover/close_cover",
            Self::StopCover => "api/services/cover/stop_cover",
            Self::SetNumber => "api/services/number/set_value",
            Self::SetCoverPosition => "api/services/cover/set_cover_position",
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

pub fn configured_actions(
    actions: &[String],
    media_players: &[String],
    binary_entities: &[String],
    covers: &[String],
) -> Vec<ConfiguredAction> {
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
    for (index, entity) in binary_entities.iter().enumerate() {
        let Some(domain) = BinaryDomain::from_entity(entity) else {
            continue;
        };
        for (verb, operation) in [
            ("on", Operation::TurnOn(domain)),
            ("off", Operation::TurnOff(domain)),
        ] {
            configured.push(ConfiguredAction {
                id: format!("ha-binary-{index}-{verb}"),
                entity: entity.clone(),
                operation,
            });
        }
    }
    for (index, entity) in covers.iter().enumerate() {
        for (verb, operation) in [
            ("open", Operation::OpenCover),
            ("close", Operation::CloseCover),
            ("stop", Operation::StopCover),
        ] {
            configured.push(ConfiguredAction {
                id: format!("ha-cover-{index}-{verb}"),
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
        let binary_entities = entity_list(&self.values.binary_entities)?;
        if binary_entities.len() > MAX_BINARY_ENTITIES
            || binary_entities
                .iter()
                .any(|entity| BinaryDomain::from_entity(entity).is_none())
        {
            return Err("invalid HA binary entities");
        }
        let cover_entities = entity_list(&self.values.cover_entities)?;
        if cover_entities.len() > MAX_COVER_ENTITIES
            || cover_entities
                .iter()
                .any(|entity| !entity.starts_with("cover."))
        {
            return Err("invalid HA cover entities");
        }
        let number_entities = entity_list(&self.values.number_entities)?;
        if number_entities.len() > MAX_NUMBER_ENTITIES
            || number_entities
                .iter()
                .any(|entity| !entity.starts_with("number."))
        {
            return Err("invalid HA number entities");
        }
        let cover_position_entities = entity_list(&self.values.cover_position_entities)?;
        if cover_position_entities.len() > MAX_COVER_POSITION_ENTITIES
            || cover_position_entities
                .iter()
                .any(|entity| !entity.starts_with("cover."))
        {
            return Err("invalid HA cover position entities");
        }
        if action_entities.len()
            + 3 * media_player_entities.len()
            + 2 * binary_entities.len()
            + 3 * cover_entities.len()
            + number_entities.len()
            + cover_position_entities.len()
            > MAX_ACTION_BUTTONS
        {
            return Err("too many HA action buttons");
        }
        for entity in action_entities
            .iter()
            .chain(&media_player_entities)
            .chain(&binary_entities)
            .chain(&cover_entities)
            .chain(&number_entities)
            .chain(&cover_position_entities)
        {
            if !entities.contains(entity) {
                entities.push(entity.clone());
            }
        }
        if entities.len() > MAX_ENTITIES {
            return Err("too many watched entities");
        }
        let discovery_prefixes = discovery_prefixes(&self.values.discovery_prefixes)?;
        Ok(Validated {
            base: base_url(&self.values.ha_base_url)?,
            entities,
            action_entities,
            media_player_entities,
            binary_entities,
            cover_entities,
            number_entities,
            cover_position_entities,
            discovery_prefixes,
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

fn literal_entity(entity: &str) -> bool {
    entity.len() <= 128
        && entity.split_once('.').is_some_and(|(domain, object)| {
            !domain.is_empty()
                && !object.is_empty()
                && domain
                    .bytes()
                    .chain(object.bytes())
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        })
}

fn discovery_prefixes(value: &str) -> Result<Vec<String>, &'static str> {
    if value.len() > MAX_DISCOVERY_PREFIX_BYTES {
        return Err("discovery prefixes are too large");
    }
    let mut prefixes = Vec::new();
    for prefix in value
        .split([',', '\n'])
        .map(str::trim)
        .filter(|prefix| !prefix.is_empty())
    {
        if prefix.len() > 128
            || !prefix.split_once('.').is_some_and(|(domain, object)| {
                matches!(domain, "sensor" | "binary_sensor")
                    && object
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            })
        {
            return Err("invalid discovery prefix");
        }
        if !prefixes.iter().any(|known| known == prefix) {
            prefixes.push(prefix.to_owned());
            if prefixes.len() > MAX_DISCOVERY_PREFIXES {
                return Err("too many discovery prefixes");
            }
        }
    }
    Ok(prefixes)
}

pub fn matches_discovery(prefixes: &[String], entity: &str) -> bool {
    literal_entity(entity) && prefixes.iter().any(|prefix| entity.starts_with(prefix))
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
        if !literal_entity(entity) {
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
    pub fn discovery_enabled(&self) -> bool {
        !self.discovery_prefixes.is_empty() && self.entities.len() < MAX_ENTITIES
    }

    pub fn discovery_url(&self) -> Url {
        self.base.join("api/states").expect("validated URL")
    }

    pub fn discovery_matches(&self, entity: &str) -> bool {
        self.discovery_enabled() && matches_discovery(&self.discovery_prefixes, entity)
    }

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
        configured_actions(
            &self.action_entities,
            &self.media_player_entities,
            &self.binary_entities,
            &self.cover_entities,
        )
    }

    pub fn service_url(&self, operation: Operation) -> Url {
        self.base
            .join(operation.path())
            .expect("validated URL and fixed service path")
    }

    pub fn inputs(&self) -> Vec<ConfiguredAction> {
        [
            (&self.number_entities, "ha-number", Operation::SetNumber),
            (
                &self.cover_position_entities,
                "ha-cover-position",
                Operation::SetCoverPosition,
            ),
        ]
        .into_iter()
        .flat_map(|(entities, prefix, operation)| {
            entities
                .iter()
                .enumerate()
                .map(move |(index, entity)| ConfiguredAction {
                    id: format!("{prefix}-{index}-set"),
                    entity: entity.clone(),
                    operation,
                })
        })
        .collect()
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

    #[test]
    fn binary_selection_preserves_existing_indices_and_maps_only_six_fixed_routes() {
        let mut config = configuration(
            "https://ha.example:8443/proxy/ha",
            "switch.desk,button.first,media_player.den,sensor.a",
        );
        assert!(config.values.binary_entities.is_empty());
        config.values.action_entities = "scene.night,button.first".into();
        config.values.media_player_entities = "media_player.den,media_player.office".into();
        config.values.binary_entities =
            "input_boolean.do_not_supply_charger,switch.desk,\nlight.room,switch.desk".into();
        let config = config.validate().unwrap();
        assert_eq!(
            config.entities,
            [
                "switch.desk",
                "button.first",
                "media_player.den",
                "sensor.a",
                "scene.night",
                "media_player.office",
                "input_boolean.do_not_supply_charger",
                "light.room",
            ]
        );
        assert_eq!(
            config.binary_entities,
            [
                "input_boolean.do_not_supply_charger",
                "switch.desk",
                "light.room"
            ]
        );
        let actions = config.actions();
        assert_eq!(actions.len(), 14);
        assert_eq!(actions[0].id, "ha-action-0");
        assert_eq!(actions[0].entity, "scene.night");
        assert_eq!(actions[1].id, "ha-action-1");
        assert_eq!(actions[1].entity, "button.first");
        for (index, entity) in config.media_player_entities.iter().enumerate() {
            for (offset, verb) in ["play", "pause", "stop"].into_iter().enumerate() {
                assert_eq!(
                    actions[2 + index * 3 + offset].id,
                    format!("ha-media-{index}-{verb}")
                );
                assert_eq!(&actions[2 + index * 3 + offset].entity, entity);
            }
        }
        for (index, domain) in ["input_boolean", "switch", "light"].into_iter().enumerate() {
            for (offset, verb) in ["on", "off"].into_iter().enumerate() {
                let action = &actions[8 + index * 2 + offset];
                assert_eq!(action.id, format!("ha-binary-{index}-{verb}"));
                assert_eq!(action.entity, config.binary_entities[index]);
                assert_eq!(
                    config.service_url(action.operation).as_str(),
                    format!("https://ha.example:8443/proxy/ha/api/services/{domain}/turn_{verb}")
                );
            }
        }
    }

    #[test]
    fn binary_domain_list_and_union_limits_are_independent() {
        for selection in [
            "sensor.a",
            "button.a",
            "scene.a",
            "media_player.a",
            "cover.a",
            "switchx.a",
            "switch.*",
            "light.Upper",
            "light.a/b",
            "switch.a.b",
            "input_boolean.",
            "do_not_supply_charger",
            "switch.a;light.b",
        ] {
            let mut config = configuration("http://localhost", "");
            config.values.binary_entities = selection.into();
            assert!(config.validate().is_err(), "accepted {selection}");
        }
        let list = |domain: &str, count| {
            (0..count)
                .map(|index| format!("{domain}.e{index}"))
                .collect::<Vec<_>>()
                .join(",")
        };
        let mut config = configuration("http://localhost", "");
        config.values.binary_entities = list("switch", 9);
        assert!(config.validate().is_err());
        let mut config = configuration("http://localhost", "");
        config.values.binary_entities = " ".repeat(4097);
        assert!(config.validate().is_err());

        let mut config = configuration(
            "http://localhost",
            &format!("{},light.e0", list("sensor", 24)),
        );
        config.values.binary_entities = format!("{},light.e0", list("light", 8));
        let config = config.validate().unwrap();
        assert_eq!(config.entities.len(), MAX_ENTITIES);
        assert_eq!(config.binary_entities.len(), MAX_BINARY_ENTITIES);
        assert_eq!(config.actions().len(), 16);
        let mut config = configuration("http://localhost", &list("sensor", 25));
        config.values.binary_entities = list("light", 8);
        assert!(config.validate().is_err());

        let config = configuration(
            "http://localhost",
            "switch.read_only,input_boolean.read_only,light.read_only",
        )
        .validate()
        .unwrap();
        assert!(config.binary_entities.is_empty());
        assert!(config.actions().is_empty());
    }

    #[test]
    fn combined_action_budget_preserves_every_old_count_and_bounds_all_new_combinations() {
        let list = |domain: &str, count| {
            (0..count)
                .map(|index| format!("{domain}.e{index}"))
                .collect::<Vec<_>>()
                .join(",")
        };
        for actions in 0..=MAX_ACTION_ENTITIES {
            for media in 0..=MAX_MEDIA_PLAYER_ENTITIES {
                for binary in 0..=MAX_BINARY_ENTITIES {
                    for covers in 0..=MAX_COVER_ENTITIES {
                        let mut config = configuration("http://localhost", "");
                        config.values.action_entities = list("button", actions);
                        config.values.media_player_entities = list("media_player", media);
                        config.values.binary_entities = list("switch", binary);
                        config.values.cover_entities = list("cover", covers);
                        let count = actions + 3 * media + 2 * binary + 3 * covers;
                        let config = config.validate();
                        if count <= MAX_ACTION_BUTTONS {
                            let config = config.unwrap();
                            assert_eq!(config.actions().len(), count);
                            assert!(1 + MAX_ENTITIES + config.actions().len() <= 64);
                        } else {
                            assert!(
                                binary > 0 || covers > 0,
                                "previously accepted configuration rejected"
                            );
                            assert_eq!(config.err(), Some("too many HA action buttons"));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn omitted_or_empty_binary_selection_preserves_the_original_serialized_size_limit() {
        // These previously accepted whitespace-only lists consume their full
        // JSON escaping cost while selecting no entities or actions.
        let mut legacy = json!({
            "revision":"legacy-limit",
            "values":{
                "ha_base_url":"http://localhost",
                "watch_entities":"\u{b}".repeat(4096),
                "action_entities":"\u{b}".repeat(1200),
                "media_player_entities":""
            },
            "secrets":{"ha_token":"fixture-token"}
        });
        let padding = MAX_CONFIGURATION_BYTES - serde_json::to_vec(&legacy).unwrap().len();
        let token = format!("fixture-token{}", "x".repeat(padding));
        assert!(token.len() <= 4096);
        legacy["secrets"]["ha_token"] = token.into();
        assert_eq!(
            serde_json::to_vec(&legacy).unwrap().len(),
            MAX_CONFIGURATION_BYTES
        );
        let omitted: Configuration = serde_json::from_value(legacy.clone()).unwrap();
        assert!(omitted.validate().unwrap().actions().is_empty());
        legacy["values"]["binary_entities"] = "".into();
        let empty: Configuration = serde_json::from_value(legacy.clone()).unwrap();
        assert!(empty.validate().unwrap().actions().is_empty());
        legacy["secrets"]["ha_token"] =
            format!("{}x", legacy["secrets"]["ha_token"].as_str().unwrap()).into();
        let oversized: Configuration = serde_json::from_value(legacy).unwrap();
        assert_eq!(oversized.validate().err(), Some("invalid configuration"));
    }

    #[test]
    fn cover_selection_appends_after_binary_and_preserves_all_existing_action_ids() {
        let mut config = configuration(
            "https://ha.example:8443/proxy/ha",
            "cover.office,sensor.a,switch.desk,scene.night,media_player.den",
        );
        assert!(config.values.cover_entities.is_empty());
        config.values.action_entities = "button.first,scene.night".into();
        config.values.media_player_entities = "media_player.den,media_player.office".into();
        config.values.binary_entities = "switch.desk,light.room".into();
        config.values.cover_entities = "cover.blind,\ncover.office,cover.blind".into();
        let config = config.validate().unwrap();
        assert_eq!(
            config.entities,
            [
                "cover.office",
                "sensor.a",
                "switch.desk",
                "scene.night",
                "media_player.den",
                "button.first",
                "media_player.office",
                "light.room",
                "cover.blind",
            ]
        );
        assert_eq!(config.cover_entities, ["cover.blind", "cover.office"]);
        let actions = config.actions();
        let previous = configured_actions(
            &config.action_entities,
            &config.media_player_entities,
            &config.binary_entities,
            &[],
        );
        assert_eq!(actions.len(), previous.len() + 6);
        for (actual, previous) in actions.iter().zip(&previous) {
            assert_eq!(actual.id, previous.id);
            assert_eq!(actual.entity, previous.entity);
            assert_eq!(
                config.service_url(actual.operation),
                config.service_url(previous.operation)
            );
        }
        for (index, entity) in config.cover_entities.iter().enumerate() {
            for (offset, verb) in ["open", "close", "stop"].into_iter().enumerate() {
                let action = &actions[previous.len() + index * 3 + offset];
                assert_eq!(action.id, format!("ha-cover-{index}-{verb}"));
                assert_eq!(&action.entity, entity);
                assert_eq!(
                    config.service_url(action.operation).as_str(),
                    format!("https://ha.example:8443/proxy/ha/api/services/cover/{verb}_cover")
                );
            }
        }
    }

    #[test]
    fn cover_domain_list_and_union_limits_are_independent_of_action_capabilities() {
        for selection in [
            "sensor.a",
            "switch.a",
            "input_boolean.a",
            "light.a",
            "button.a",
            "media_player.a",
            "coverx.a",
            "cover.*",
            "cover.Upper",
            "cover.a/b",
            "cover.a.b",
            "cover.",
            "do_not_supply_charger",
        ] {
            let mut config = configuration("http://localhost", "");
            config.values.cover_entities = selection.into();
            assert!(config.validate().is_err(), "accepted {selection}");
        }
        let list = |domain: &str, count| {
            (0..count)
                .map(|index| format!("{domain}.e{index}"))
                .collect::<Vec<_>>()
                .join(",")
        };
        let mut config = configuration("http://localhost", "");
        config.values.cover_entities = list("cover", 5);
        assert!(config.validate().is_err());
        let mut config = configuration("http://localhost", "");
        config.values.cover_entities = " ".repeat(4097);
        assert!(config.validate().is_err());
        let mut config = configuration(
            "http://localhost",
            &format!("{},cover.e0", list("sensor", 28)),
        );
        config.values.cover_entities = format!("{},cover.e0", list("cover", 4));
        let config = config.validate().unwrap();
        assert_eq!(config.entities.len(), MAX_ENTITIES);
        assert_eq!(config.cover_entities.len(), MAX_COVER_ENTITIES);
        assert_eq!(
            config.actions().len(),
            12,
            "configuration reserves all three slots per cover before any state is observed"
        );
        let mut config = configuration("http://localhost", &list("sensor", 29));
        config.values.cover_entities = list("cover", 4);
        assert!(config.validate().is_err());
        let read_only = configuration("http://localhost", "cover.read_only")
            .validate()
            .unwrap();
        assert!(read_only.cover_entities.is_empty());
        assert!(read_only.actions().is_empty());
    }

    #[test]
    fn omitted_or_empty_cover_selection_preserves_a_previous_binary_config_at_the_size_limit() {
        let mut legacy = json!({
            "revision":"legacy-cover-limit",
            "values":{
                "ha_base_url":"http://localhost",
                "watch_entities":"\u{b}".repeat(4096),
                "action_entities":"\u{b}".repeat(1200),
                "media_player_entities":"",
                "binary_entities":"switch.desk"
            },
            "secrets":{"ha_token":"fixture-token"}
        });
        let padding = MAX_CONFIGURATION_BYTES - serde_json::to_vec(&legacy).unwrap().len();
        let token = format!("fixture-token{}", "x".repeat(padding));
        assert!(token.len() <= 4096);
        legacy["secrets"]["ha_token"] = token.into();
        assert_eq!(
            serde_json::to_vec(&legacy).unwrap().len(),
            MAX_CONFIGURATION_BYTES
        );
        let omitted: Configuration = serde_json::from_value(legacy.clone()).unwrap();
        assert_eq!(omitted.validate().unwrap().actions().len(), 2);
        legacy["values"]["cover_entities"] = "".into();
        let empty: Configuration = serde_json::from_value(legacy.clone()).unwrap();
        assert_eq!(empty.validate().unwrap().actions().len(), 2);
        legacy["secrets"]["ha_token"] =
            format!("{}x", legacy["secrets"]["ha_token"].as_str().unwrap()).into();
        let oversized: Configuration = serde_json::from_value(legacy).unwrap();
        assert_eq!(oversized.validate().err(), Some("invalid configuration"));
    }

    #[test]
    fn numeric_selections_are_explicit_ordered_bounded_and_preserve_old_ids() {
        let mut config = configuration(
            "https://ha.example/prefix",
            "cover.position,number.first,sensor.a",
        );
        config.values.action_entities = "button.a".into();
        config.values.media_player_entities = "media_player.a".into();
        config.values.binary_entities = "light.a".into();
        config.values.cover_entities = "cover.control".into();
        config.values.number_entities = "number.second,number.first,number.second".into();
        config.values.cover_position_entities = "cover.control,cover.position,cover.control".into();
        let config = config.validate().unwrap();
        assert_eq!(
            config.entities,
            [
                "cover.position",
                "number.first",
                "sensor.a",
                "button.a",
                "media_player.a",
                "light.a",
                "cover.control",
                "number.second"
            ]
        );
        assert_eq!(
            config
                .actions()
                .iter()
                .map(|action| action.id.as_str())
                .collect::<Vec<_>>(),
            [
                "ha-action-0",
                "ha-media-0-play",
                "ha-media-0-pause",
                "ha-media-0-stop",
                "ha-binary-0-on",
                "ha-binary-0-off",
                "ha-cover-0-open",
                "ha-cover-0-close",
                "ha-cover-0-stop"
            ]
        );
        let inputs = config.inputs();
        assert_eq!(
            inputs
                .iter()
                .map(|input| (input.id.as_str(), input.entity.as_str()))
                .collect::<Vec<_>>(),
            [
                ("ha-number-0-set", "number.second"),
                ("ha-number-1-set", "number.first"),
                ("ha-cover-position-0-set", "cover.control"),
                ("ha-cover-position-1-set", "cover.position")
            ]
        );
        assert_eq!(
            config.service_url(inputs[0].operation).as_str(),
            "https://ha.example/prefix/api/services/number/set_value"
        );
        assert_eq!(
            config.service_url(inputs[2].operation).as_str(),
            "https://ha.example/prefix/api/services/cover/set_cover_position"
        );
        let mut old = configuration("http://localhost", "number.a,cover.a");
        old.values.cover_entities = "cover.a".into();
        assert!(old.validate().unwrap().inputs().is_empty());
        for (field, domain) in [
            ("number_entities", "number"),
            ("cover_position_entities", "cover"),
        ] {
            for invalid in [
                "switch.a".into(),
                "number.*".into(),
                "cover.a/escape".into(),
                "do_not_supply_charger".into(),
                " ".repeat(4097),
                (0..5)
                    .map(|i| format!("{domain}.e{i}"))
                    .collect::<Vec<_>>()
                    .join(","),
            ] {
                let mut config =
                    serde_json::to_value(configuration("http://localhost", "")).unwrap();
                config["values"][field] = json!(invalid);
                assert!(serde_json::from_value::<Configuration>(config)
                    .unwrap()
                    .validate()
                    .is_err());
            }
        }
    }

    #[test]
    fn numeric_control_reservations_cover_every_existing_budget_and_watch_boundary() {
        let list = |domain: &str, count| {
            (0..count)
                .map(|i| format!("{domain}.e{i}"))
                .collect::<Vec<_>>()
                .join(",")
        };
        // Every old control total is achievable. Also exercise overlaps between
        // all four selected covers and independently selected positions.
        for previous in 0..=31 {
            for numbers in 0..=MAX_NUMBER_ENTITIES {
                for positions in 0..=MAX_COVER_POSITION_ENTITIES {
                    let mut config = configuration("http://localhost", "");
                    let covers = (previous / 3).min(4);
                    let remaining = previous - 3 * covers;
                    let media = (remaining / 3).min(4);
                    let buttons = remaining - 3 * media;
                    config.values.cover_entities = list("cover", covers);
                    config.values.media_player_entities = list("media_player", media);
                    config.values.action_entities = list("button", buttons);
                    config.values.number_entities = list("number", numbers);
                    config.values.cover_position_entities = list("cover", positions);
                    let config = config.validate();
                    let count = previous + numbers + positions;
                    if count <= MAX_ACTION_BUTTONS {
                        let config = config.unwrap();
                        assert_eq!(config.actions().len() + config.inputs().len(), count);
                        assert!(1 + MAX_ENTITIES + count <= 64);
                    } else {
                        assert_eq!(config.err(), Some("too many HA action buttons"));
                    }
                }
            }
        }
        for count in [28, 29] {
            let mut config = configuration("http://localhost", &list("sensor", count));
            config.values.number_entities = list("number", 4);
            assert_eq!(config.validate().is_ok(), count == 28);
        }
    }

    #[test]
    fn empty_numeric_fields_preserve_the_exact_previous_configuration_byte_boundary() {
        let mut legacy = json!({"revision":"legacy-numeric-limit","values":{
            "ha_base_url":"http://localhost","watch_entities":"\u{b}".repeat(4096),
            "action_entities":"\u{b}".repeat(1200),"media_player_entities":"",
            "binary_entities":"light.a","cover_entities":"cover.a"},"secrets":{"ha_token":"fixture-token"}});
        let padding = MAX_CONFIGURATION_BYTES - serde_json::to_vec(&legacy).unwrap().len();
        legacy["secrets"]["ha_token"] = json!(format!("fixture-token{}", "x".repeat(padding)));
        assert_eq!(
            serde_json::to_vec(&legacy).unwrap().len(),
            MAX_CONFIGURATION_BYTES
        );
        for field in [
            None,
            Some("number_entities"),
            Some("cover_position_entities"),
        ] {
            if let Some(field) = field {
                legacy["values"][field] = json!("");
            }
            let config: Configuration = serde_json::from_value(legacy.clone()).unwrap();
            assert_eq!(
                serde_json::to_vec(&config).unwrap().len(),
                MAX_CONFIGURATION_BYTES
            );
            let config = config.validate().unwrap();
            assert!(config.inputs().is_empty());
            assert_eq!(config.actions().len(), 5);
        }
        legacy["secrets"]["ha_token"] = json!(format!(
            "{}x",
            legacy["secrets"]["ha_token"].as_str().unwrap()
        ));
        assert_eq!(
            serde_json::from_value::<Configuration>(legacy)
                .unwrap()
                .validate()
                .err(),
            Some("invalid configuration")
        );
    }

    #[test]
    fn discovery_prefixes_are_opt_in_literal_bounded_and_do_not_change_explicit_authority() {
        let mut config = configuration("https://ha.example:8443/prefix/ha", "sensor.manual");
        config.values.action_entities = "button.a,scene.b".into();
        config.values.media_player_entities = "media_player.a".into();
        config.values.binary_entities = "light.a".into();
        config.values.cover_entities = "cover.a".into();
        config.values.number_entities = "number.a".into();
        config.values.cover_position_entities = "cover.a".into();
        let previous =
            serde_json::from_value::<Configuration>(serde_json::to_value(&config).unwrap())
                .unwrap()
                .validate()
                .unwrap();
        config.values.discovery_prefixes =
            " sensor.room_,\nbinary_sensor.,sensor.room_,sensor. ".into();
        let config = config.validate().unwrap();
        assert_eq!(
            config.discovery_prefixes,
            ["sensor.room_", "binary_sensor.", "sensor."]
        );
        assert!(config.discovery_enabled());
        assert_eq!(config.entities, previous.entities);
        assert_eq!(
            config
                .actions()
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            previous
                .actions()
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            config
                .inputs()
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            previous
                .inputs()
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            config.discovery_url().as_str(),
            "https://ha.example:8443/prefix/ha/api/states"
        );
        for entity in [
            "sensor.room_temperature",
            "sensor.other",
            "binary_sensor.door",
            "sensor.do_not_supply_charger",
        ] {
            assert!(config.discovery_matches(entity));
        }
        for entity in [
            "sensor.",
            "sensor.room.*",
            "sensor.room.a",
            "sensor.room/A",
            "sensor.Room",
            "binary_sensor.",
            "sensorx.a",
            "switch.a",
            "do_not_supply_charger",
        ] {
            assert!(!config.discovery_matches(entity), "{entity}");
        }
        let empty = configuration("http://localhost", "").validate().unwrap();
        assert!(empty.discovery_prefixes.is_empty());
        assert!(!empty.discovery_enabled());
    }

    #[test]
    fn discovery_prefix_limits_and_full_explicit_capacity_are_enforced() {
        for prefix in [
            "sensor",
            "binary_sensor",
            "switch.",
            "number.a",
            "cover.",
            "button.a",
            "sensor.*",
            "sensor.[a-z]",
            "sensor.a?",
            "sensor.a.b",
            "sensor.a/b",
            "sensor.a%20",
            "sensor.A",
            "sensor.a b",
            "sensor.é",
            "do_not_supply_charger",
        ] {
            assert!(discovery_prefixes(prefix).is_err(), "{prefix}");
        }
        let maximum = format!("sensor.{}", "a".repeat(121));
        assert_eq!(maximum.len(), 128);
        assert!(discovery_prefixes(&maximum).is_ok());
        assert!(discovery_prefixes(&format!("{maximum}a")).is_err());
        let eight = (0..8)
            .map(|i| format!("sensor.e{i}"))
            .collect::<Vec<_>>()
            .join(",");
        assert_eq!(
            discovery_prefixes(&format!("{eight},sensor.e0"))
                .unwrap()
                .len(),
            8
        );
        assert!(discovery_prefixes(&format!("{eight},sensor.e8")).is_err());
        let exact = format!("sensor.{}", " ".repeat(MAX_DISCOVERY_PREFIX_BYTES - 7));
        assert_eq!(exact.len(), MAX_DISCOVERY_PREFIX_BYTES);
        assert_eq!(discovery_prefixes(&exact).unwrap(), ["sensor."]);
        assert!(discovery_prefixes(&format!("{exact} ")).is_err());
        let selected = (0..32)
            .map(|i| format!("sensor.e{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let mut config = configuration("http://localhost", &selected);
        config.values.discovery_prefixes = "sensor.".into();
        let config = config.validate().unwrap();
        assert_eq!(config.entities.len(), 32);
        assert!(!config.discovery_enabled());
        assert!(!config.discovery_matches("sensor.additional"));
    }

    #[test]
    fn empty_discovery_selection_preserves_the_exact_numeric_configuration_byte_boundary() {
        let mut previous = json!({"revision":"previous-discovery-limit","values":{
            "ha_base_url":"http://localhost","watch_entities":"\u{b}".repeat(4096),"action_entities":"\u{b}".repeat(1200),"media_player_entities":"",
            "binary_entities":"light.a","cover_entities":"cover.a","number_entities":"number.a","cover_position_entities":"cover.a"},
            "secrets":{"ha_token":"fixture-token"}});
        let padding = MAX_CONFIGURATION_BYTES - serde_json::to_vec(&previous).unwrap().len();
        previous["secrets"]["ha_token"] = json!(format!("fixture-token{}", "x".repeat(padding)));
        for explicit_empty in [false, true] {
            if explicit_empty {
                previous["values"]["discovery_prefixes"] = json!("");
            }
            let config: Configuration = serde_json::from_value(previous.clone()).unwrap();
            assert_eq!(
                serde_json::to_vec(&config).unwrap().len(),
                MAX_CONFIGURATION_BYTES
            );
            let config = config.validate().unwrap();
            assert!(!config.discovery_enabled());
            assert_eq!(config.inputs().len(), 2);
        }
        previous["secrets"]["ha_token"] = json!(format!(
            "{}x",
            previous["secrets"]["ha_token"].as_str().unwrap()
        ));
        assert_eq!(
            serde_json::from_value::<Configuration>(previous)
                .unwrap()
                .validate()
                .err(),
            Some("invalid configuration")
        );
    }
}
