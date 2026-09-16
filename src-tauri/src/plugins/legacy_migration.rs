//! Pure, per-package planning for retained legacy settings. No I/O or actions.

use super::settings_store::SettingsData;
use crate::FullConfig;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

const HA: &str = "inverter-desktop.home-assistant";
const FRIGATE: &str = "inverter-desktop.frigate";
const KERBEROS: &str = "inverter-desktop.kerberos";
const RING: &str = "inverter-desktop.ring";
const FLAGS: &[&str] = &[
    "only_charging",
    "no_feed",
    "house_support",
    "charge_battery",
    "do_not_supply_charger",
    "set_limit_to_ev_charger",
    "minimize_charging",
];
const PRIMARY_DOMAINS: &[&str] = &[
    "switch",
    "light",
    "input_boolean",
    "fan",
    "cover",
    "lock",
    "media_player",
    "scene",
    "script",
    "number",
    "sensor",
    "binary_sensor",
    "climate",
    "button",
];

/// Contains credentials: deliberately neither Debug nor Serialize.
pub(crate) struct PluginSeed {
    pub plugin_id: String,
    pub values: BTreeMap<String, Value>,
    pub secrets: BTreeMap<String, String>,
    legacy: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MigrationError {
    MissingDaemonState,
    MissingCredentials,
    InvalidEndpoint,
    EndpointConflict,
    NeedsEntitySelection,
    InvalidSelection,
    UnsupportedControl,
    CapacityExceeded,
    UnsupportedCameraTopic,
    InvalidCameraMapping,
    UnsupportedNamespace,
}

impl MigrationError {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::MissingDaemonState => "legacy_migration_waiting_for_daemon",
            Self::MissingCredentials => "legacy_migration_missing_credentials",
            Self::InvalidEndpoint => "legacy_migration_endpoint_review_required",
            Self::EndpointConflict => "legacy_migration_endpoint_conflict",
            Self::NeedsEntitySelection => "legacy_migration_entity_selection_required",
            Self::InvalidSelection => "legacy_migration_invalid_selection",
            Self::UnsupportedControl => "legacy_migration_unsupported_control",
            Self::CapacityExceeded => "legacy_migration_capacity_exceeded",
            Self::UnsupportedCameraTopic => "legacy_migration_camera_topic_review_required",
            Self::InvalidCameraMapping => "legacy_migration_camera_mapping_review_required",
            Self::UnsupportedNamespace => "module_restore_schema_unsupported",
        }
    }
}

impl std::fmt::Display for MigrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}

pub(crate) fn plan_plugin(
    config: &FullConfig,
    daemon: Option<&Value>,
    plugin_id: &str,
) -> Result<Option<PluginSeed>, MigrationError> {
    let seed = if let Some(namespace) = config.modules.get(plugin_id) {
        if namespace.schema_version.get() != 1 {
            return Err(MigrationError::UnsupportedNamespace);
        }
        Some(PluginSeed {
            plugin_id: plugin_id.to_owned(),
            values: namespace
                .values
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            secrets: namespace.secrets.clone(),
            legacy: false,
        })
    } else {
        match plugin_id {
            HA => plan_ha(config, daemon)?,
            FRIGATE | KERBEROS | RING => plan_camera(config, plugin_id)?,
            _ => None,
        }
    };
    if let Some(seed) = &seed {
        let envelope =
            json!({"revision":"r".repeat(128),"values":seed.values,"secrets":seed.secrets});
        if serde_json::to_vec(&envelope)
            .map_err(|_| MigrationError::CapacityExceeded)?
            .len()
            > 32 * 1024
        {
            return Err(MigrationError::CapacityExceeded);
        }
    }
    Ok(seed)
}

/// Pure merge; the committing caller assigns a fresh revision and validates the
/// installed schema before persistence. A committed marker protects later removals.
pub(crate) fn merge_seed(
    current: &SettingsData,
    seed: &PluginSeed,
) -> Result<SettingsData, MigrationError> {
    if current.legacy_migration_version >= 1 {
        return Ok(current.clone());
    }
    let binding: &[&str] = if seed.plugin_id == HA {
        &["ha_base_url"]
    } else {
        &[
            "mqtt_host",
            "mqtt_port",
            "mqtt_tls",
            "snapshot_base_url",
            "frigate_base_url",
        ]
    };
    for field in binding {
        if let (Some(existing), Some(planned)) =
            (current.values.get(*field), seed.values.get(*field))
        {
            if !same_binding(field, existing, planned) {
                return Err(MigrationError::EndpointConflict);
            }
        }
    }
    // Never combine one account's saved username/template with another source's
    // missing password/token, even when the public origin happens to agree.
    for field in ["mqtt_username", "snapshot_url_template"] {
        if let (Some(existing), Some(planned)) =
            (current.secrets.get(field), seed.secrets.get(field))
        {
            if existing != planned
                && seed
                    .secrets
                    .keys()
                    .any(|key| !current.secrets.contains_key(key))
            {
                return Err(MigrationError::EndpointConflict);
            }
        }
    }
    let mut result = current.clone();
    for (key, value) in &seed.values {
        result
            .values
            .entry(key.clone())
            .or_insert_with(|| value.clone());
    }
    if seed.plugin_id == HA && seed.legacy {
        merge_legacy_ha_reads(&mut result.values, &seed.values)?;
    }
    for (key, value) in &seed.secrets {
        let explicitly_known = current.secret_fields.contains(key);
        result.secret_fields.insert(key.clone());
        if !explicitly_known {
            result
                .secrets
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
    }
    result.legacy_migration_version = 1;
    Ok(result)
}

/// Older installed HA workers can already have a partial watch list while the
/// bundled connection still supplies clamps and appliance reads. Complete that
/// handover once, preserving existing order and all explicit action selections.
fn merge_legacy_ha_reads(
    values: &mut BTreeMap<String, Value>,
    planned: &BTreeMap<String, Value>,
) -> Result<(), MigrationError> {
    let mut watched = Vec::new();
    for source in [values.get("watch_entities"), planned.get("watch_entities")] {
        append_read_list(&mut watched, source)?;
    }
    values.insert("watch_entities".into(), json!(watched.join(",")));
    // The worker's 64-read ceiling includes explicit control families, profile
    // roles, and layout targets even when absent from the ordinary watch list.
    let mut all = watched;
    for field in [
        "action_entities",
        "media_player_entities",
        "binary_entities",
        "cover_entities",
        "number_entities",
        "cover_position_entities",
        "washer_remaining_entity",
        "dryer_remaining_entity",
        "dishwasher_running_entity",
        "dishwasher_duration_entity",
    ] {
        append_read_list(&mut all, values.get(field))?;
    }
    if let Some(layout) = values.get("dashboard_layout") {
        let layout = layout.as_str().ok_or(MigrationError::InvalidSelection)?;
        if !layout.trim().is_empty() {
            let layout: Value =
                serde_json::from_str(layout).map_err(|_| MigrationError::InvalidSelection)?;
            if let Some(controls) = layout.get("controls").and_then(Value::as_array) {
                for control in controls {
                    append_read_list(&mut all, control.get("entity"))?;
                }
            }
            if let Some(appliances) = layout.get("appliances").and_then(Value::as_object) {
                for target in appliances.values() {
                    append_read_list(&mut all, Some(target))?;
                }
            }
        }
    }
    Ok(())
}

fn append_read_list(reads: &mut Vec<String>, value: Option<&Value>) -> Result<(), MigrationError> {
    let Some(value) = value else {
        return Ok(());
    };
    let value = value.as_str().ok_or(MigrationError::InvalidSelection)?;
    for target in value
        .split([',', '\n'])
        .map(str::trim)
        .filter(|target| !target.is_empty())
    {
        add_entity(reads, target).map_err(|error| {
            if error == MigrationError::CapacityExceeded {
                MigrationError::NeedsEntitySelection
            } else {
                error
            }
        })?;
    }
    Ok(())
}

fn same_binding(field: &str, existing: &Value, planned: &Value) -> bool {
    if existing == planned {
        return true;
    }
    if !matches!(
        field,
        "ha_base_url" | "snapshot_base_url" | "frigate_base_url"
    ) {
        return false;
    }
    let normalized = |value: &Value| {
        let mut url = http_url(value.as_str()?, false, false).ok()?;
        let path = format!("{}/", url.path().trim_end_matches('/'));
        url.set_path(&path);
        Some(url)
    };
    normalized(existing).is_some_and(|value| Some(value) == normalized(planned))
}

fn seed(plugin_id: &'static str) -> PluginSeed {
    PluginSeed {
        plugin_id: plugin_id.to_owned(),
        values: BTreeMap::new(),
        secrets: BTreeMap::new(),
        legacy: true,
    }
}

fn text(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

fn http_url(value: &str, query: bool, fragment: bool) -> Result<reqwest::Url, MigrationError> {
    if value.len() > 2048
        || value.trim() != value
        || value.chars().any(char::is_control)
        || value.contains('\\')
    {
        return Err(MigrationError::InvalidEndpoint);
    }
    let tail = value
        .split_once("://")
        .ok_or(MigrationError::InvalidEndpoint)?
        .1;
    let authority = tail.split(['/', '?', '#']).next().unwrap_or_default();
    let url = reqwest::Url::parse(value).map_err(|_| MigrationError::InvalidEndpoint)?;
    if authority.is_empty()
        || authority.contains('@')
        || !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || (!query && url.query().is_some())
        || (!fragment && url.fragment().is_some())
    {
        return Err(MigrationError::InvalidEndpoint);
    }
    Ok(url)
}

fn ha_endpoint(config: &FullConfig) -> Result<reqwest::Url, MigrationError> {
    let original = text(&config.ha_url).ok_or(MigrationError::MissingCredentials)?;
    let value = if original.contains("://") {
        original.to_owned()
    } else {
        format!("http://{original}")
    };
    let mut url = http_url(&value, false, false)?;
    // The bundled client discarded path prefixes. Preserving one now would
    // silently change its effective endpoint, so ambiguous legacy URLs need review.
    if url.path() != "/" {
        return Err(MigrationError::InvalidEndpoint);
    }
    let authority = value
        .split_once("://")
        .unwrap()
        .1
        .split('/')
        .next()
        .unwrap();
    let explicit = if authority.starts_with('[') {
        authority
            .split_once(']')
            .and_then(|(_, tail)| tail.strip_prefix(':'))
    } else {
        authority.rsplit_once(':').map(|(_, port)| port)
    };
    let port = match explicit {
        Some(value) => {
            let port = value
                .parse::<u16>()
                .map_err(|_| MigrationError::InvalidEndpoint)?;
            if config.ha_port.is_some_and(|configured| configured != port) {
                return Err(MigrationError::EndpointConflict);
            }
            port
        }
        None => config.ha_port.unwrap_or(8123),
    };
    if port == 0 {
        return Err(MigrationError::InvalidEndpoint);
    }
    url.set_port(Some(port))
        .map_err(|_| MigrationError::InvalidEndpoint)?;
    Ok(url)
}

/// Preserve the former camera downloader's optional, exact-origin credential
/// binding independently from the stricter HA endpoint migration review.
fn camera_ha_origin(config: &FullConfig) -> Option<reqwest::Url> {
    let original = text(&config.ha_url)?;
    let value = if original.contains("://") {
        original.to_owned()
    } else {
        format!("http://{original}")
    };
    let mut url = http_url(&value, true, true).ok()?;
    let authority = value.split_once("://")?.1.split(['/', '?', '#']).next()?;
    let explicit = if authority.starts_with('[') {
        authority
            .split_once(']')
            .is_some_and(|(_, tail)| tail.starts_with(':'))
    } else {
        authority.contains(':')
    };
    if !explicit {
        url.set_port(Some(config.ha_port.unwrap_or(8123))).ok()?;
    }
    Some(url)
}

#[derive(Deserialize)]
struct LegacyControl {
    id: String,
    label: String,
    entity: String,
    #[serde(default = "yes")]
    enabled: bool,
}
fn yes() -> bool {
    true
}

fn controls(
    config: &FullConfig,
    daemon: Option<&Value>,
    surface: &str,
) -> Result<Vec<LegacyControl>, MigrationError> {
    let configured = if surface == "header" {
        serde_json::to_value(&config.header_toggles_config)
    } else {
        serde_json::to_value(&config.ha_entities)
    }
    .map_err(|_| MigrationError::InvalidSelection)?;
    let value = if configured.as_array().is_some_and(|items| !items.is_empty()) {
        configured
    } else {
        let daemon = daemon.ok_or(MigrationError::MissingDaemonState)?;
        let key = if surface == "header" {
            "header_toggles"
        } else {
            "home_buttons"
        };
        daemon
            .get("ui_config")
            .and_then(|ui| ui.get(key))
            .filter(|value| !value.is_null())
            .cloned()
            .unwrap_or_else(|| json!([]))
    };
    serde_json::from_value(value).map_err(|_| MigrationError::InvalidSelection)
}

fn core_flag(entity: &str) -> bool {
    FLAGS.contains(&entity.strip_prefix("input_boolean.").unwrap_or(entity))
}
fn entity(value: &str) -> bool {
    let Some((domain, id)) = value.split_once('.') else {
        return false;
    };
    !domain.is_empty()
        && !id.is_empty()
        && value.len() <= 128
        && domain
            .bytes()
            .chain(id.bytes())
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
fn add_entity(reads: &mut Vec<String>, value: &str) -> Result<(), MigrationError> {
    if !entity(value) {
        return Err(MigrationError::InvalidSelection);
    }
    if !reads.iter().any(|existing| existing == value) {
        reads.push(value.to_owned());
    }
    if reads.len() > 64 {
        return Err(MigrationError::CapacityExceeded);
    }
    Ok(())
}
fn display_id(surface: &str, id: &str) -> String {
    let candidate = format!("{surface}-{id}");
    if !id.is_empty()
        && candidate.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
    {
        candidate
    } else {
        format!(
            "{surface}-{}",
            &super::package::sha256_hex(id.as_bytes())[..48]
        )
    }
}

fn plan_ha(
    config: &FullConfig,
    daemon: Option<&Value>,
) -> Result<Option<PluginSeed>, MigrationError> {
    if !config.ha_use_direct_api {
        return Ok(None);
    }
    let base = ha_endpoint(config)?;
    let token = text(&config.ha_longlived_token).ok_or(MigrationError::MissingCredentials)?;
    if token.len() > 4096 || token.chars().any(char::is_control) {
        return Err(MigrationError::MissingCredentials);
    }
    let mut result = seed(HA);
    result.values.insert(
        "ha_base_url".into(),
        json!(base.as_str().trim_end_matches('/')),
    );
    result.secrets.insert("ha_token".into(), token.to_owned());
    let mut selected = Vec::new();
    let mut actions = BTreeSet::new();
    let mut layout = Vec::new();
    let mut ids = BTreeSet::new();
    for surface in ["header", "home"] {
        let shown = if surface == "header" {
            config.show_header_toggles.unwrap_or(true)
        } else {
            config.show_home_section.unwrap_or(true)
        };
        // Hidden surfaces remain in retained configuration; no new action grant.
        if !shown {
            continue;
        }
        for (order, control) in controls(config, daemon, surface)?.into_iter().enumerate() {
            let target = control.entity.trim();
            if !control.enabled
                || core_flag(target)
                || (surface == "home"
                    && daemon
                        .and_then(|v| v.pointer("/features/ha"))
                        .and_then(Value::as_bool)
                        == Some(false))
            {
                continue;
            }
            if !entity(target) || !PRIMARY_DOMAINS.contains(&target.split_once('.').unwrap().0) {
                return Err(MigrationError::UnsupportedControl);
            }
            if control.label.trim().is_empty()
                || control.label.len() > 128
                || control.label.chars().any(char::is_control)
            {
                return Err(MigrationError::InvalidSelection);
            }
            let id = display_id(surface, &control.id);
            if !ids.insert(id.clone()) {
                return Err(MigrationError::InvalidSelection);
            }
            add_entity(&mut selected, target)?;
            actions.insert(target.to_owned());
            let lower = control.label.to_lowercase();
            let icon = if target.starts_with("light.") {
                "light"
            } else if ["laundry", "washer", "washing"]
                .iter()
                .any(|word| lower.contains(word))
            {
                "washer"
            } else {
                "plug"
            };
            layout.push(json!({"id":id,"surface":surface,"order":u16::try_from(order).map_err(|_|MigrationError::CapacityExceeded)?,"label":control.label,"entity":target,"icon":icon}));
        }
    }
    let mut sections = BTreeMap::new();
    let mut domains = Vec::new();
    for (name, shown, domain) in [
        ("sensors", config.show_ha_sensors, "sensor,binary_sensor"),
        ("numbers", config.show_ha_numbers, "number"),
        ("covers", config.show_ha_covers, "cover"),
        ("media", config.show_ha_media, "media_player"),
        ("scenes", config.show_ha_scenes, "scene"),
        ("weather", config.show_ha_weather, "weather"),
    ] {
        let shown = shown.unwrap_or(true);
        sections.insert(name, shown);
        if shown {
            if matches!(name, "numbers" | "covers" | "media" | "scenes") {
                return Err(MigrationError::NeedsEntitySelection);
            }
            domains.extend(domain.split(',').map(str::to_owned));
        }
    }
    for (name, shown) in [
        ("washer", config.show_washer),
        ("dryer", config.show_dryer),
        ("dishwasher", config.show_dishwasher),
    ] {
        sections.insert(name, shown.unwrap_or(true));
    }
    for (key, value) in [
        ("washer_remaining_entity", &config.ha_washer_entity),
        ("dryer_remaining_entity", &config.ha_dryer_entity),
        (
            "dishwasher_running_entity",
            &config.ha_dishwasher_running_entity,
        ),
        (
            "dishwasher_duration_entity",
            &config.ha_dishwasher_duration_entity,
        ),
    ] {
        if let Some(value) = text(value) {
            add_entity(&mut selected, value)?;
            result.values.insert(key.into(), json!(value));
        }
    }
    if text(&config.ha_dishwasher_duration_entity).is_some()
        && text(&config.ha_dishwasher_running_entity).is_none()
    {
        return Err(MigrationError::InvalidSelection);
    }
    for clamp in config
        .ha_consumption_clamps
        .iter()
        .flatten()
        .chain(config.ha_generation_clamps.iter().flatten())
    {
        let clamp = clamp.trim();
        if !clamp.is_empty() {
            add_entity(&mut selected, clamp)?;
        }
    }
    let mut appliances = BTreeMap::new();
    for (role, shown, value) in [
        (
            "washer_start",
            config.show_washer,
            &config.ha_washer_start_entity,
        ),
        (
            "washer_pause",
            config.show_washer,
            &config.ha_washer_pause_entity,
        ),
        (
            "dryer_start",
            config.show_dryer,
            &config.ha_dryer_start_entity,
        ),
        (
            "dryer_pause",
            config.show_dryer,
            &config.ha_dryer_pause_entity,
        ),
    ] {
        if shown == Some(false) {
            continue;
        }
        if let Some(value) = text(value) {
            add_entity(&mut selected, value)?;
            if !PRIMARY_DOMAINS.contains(&value.split_once('.').unwrap().0) {
                return Err(MigrationError::UnsupportedControl);
            }
            actions.insert(value.to_owned());
            appliances.insert(role, value);
        }
    }
    if layout.len() > 64 || actions.len() > 63 {
        return Err(MigrationError::CapacityExceeded);
    }
    let dashboard =
        json!({"version":1,"controls":layout,"sections":sections,"appliances":appliances})
            .to_string();
    if dashboard.len() > 24 * 1024 {
        return Err(MigrationError::CapacityExceeded);
    }
    result
        .values
        .insert("dashboard_layout".into(), json!(dashboard));
    result
        .values
        .insert("watch_entities".into(), json!(selected.join(",")));
    result
        .values
        .insert("discovery_domains".into(), json!(domains.join(",")));
    result.values.insert("notify_home".into(), json!(true));
    Ok(Some(result))
}

fn camera_identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.trim() == value
        && !value
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '+' | '#'))
}
fn selected_topics(config: &FullConfig, plugin: &str) -> Result<Vec<String>, MigrationError> {
    let mut topics = Vec::new();
    for topic in config
        .camera_topic
        .as_deref()
        .unwrap_or("")
        .split(';')
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        if topic == "kerberos/desktop/events" {
            if plugin == KERBEROS {
                for native in ["kerberos/agent/+", "kerberos/hub/+"] {
                    if !topics.iter().any(|v| v == native) {
                        topics.push(native.to_owned());
                    }
                }
            }
            continue;
        }
        let belongs = match plugin {
            KERBEROS => topic.starts_with("kerberos/"),
            RING => topic.starts_with("ring/"),
            FRIGATE => !topic.starts_with("kerberos/") && !topic.starts_with("ring/"),
            _ => false,
        };
        if !belongs {
            continue;
        }
        let identifier = |v: &str| v == "+" || camera_identity(v);
        let parts: Vec<_> = topic.split('/').collect();
        let valid = match (plugin, parts.as_slice()) {
            (KERBEROS, ["kerberos", "agent" | "hub", id]) => identifier(id),
            (RING, ["ring", location, "camera", device, event, "state"]) => {
                identifier(location)
                    && identifier(device)
                    && matches!(*event, "motion" | "ding" | "+")
            }
            (FRIGATE, _) => {
                topic.len() <= 256
                    && !topic
                        .chars()
                        .any(|c| c.is_control() || matches!(c, '+' | '#'))
            }
            _ => false,
        };
        if !valid || topic.len() > 512 {
            return Err(MigrationError::UnsupportedCameraTopic);
        }
        if !topics.iter().any(|value| value == topic) {
            topics.push(topic.to_owned());
        }
    }
    if topics.len() > 16 || topics.join(";").len() > 4096 || (plugin == FRIGATE && topics.len() > 1)
    {
        return Err(MigrationError::UnsupportedCameraTopic);
    }
    Ok(topics)
}

fn plan_camera(config: &FullConfig, plugin: &str) -> Result<Option<PluginSeed>, MigrationError> {
    let topics = selected_topics(config, plugin)?;
    if topics.is_empty() {
        return Ok(None);
    }
    let plugin = match plugin {
        FRIGATE => FRIGATE,
        KERBEROS => KERBEROS,
        RING => RING,
        _ => return Ok(None),
    };
    let mut result = seed(plugin);
    let host = text(&config.mqtt_ha_host).ok_or(MigrationError::MissingCredentials)?;
    let port = config.mqtt_ha_port.unwrap_or(1883);
    if port == 0 || host.contains(['/', '@']) || host.chars().any(char::is_control) {
        return Err(MigrationError::InvalidEndpoint);
    }
    result.values.extend([
        ("mqtt_host".into(), json!(host)),
        ("mqtt_port".into(), json!(port)),
        ("mqtt_tls".into(), json!(false)),
    ]);
    for (key, value) in [
        ("mqtt_username", &config.mqtt_ha_login),
        ("mqtt_password", &config.mqtt_ha_password),
    ] {
        if let Some(value) = value.as_deref().filter(|v| !v.is_empty()) {
            if value.len() > 1024 || value.chars().any(char::is_control) {
                return Err(MigrationError::MissingCredentials);
            }
            result.secrets.insert(key.into(), value.to_owned());
        }
    }
    if result.secrets.contains_key("mqtt_password") && !result.secrets.contains_key("mqtt_username")
    {
        return Err(MigrationError::MissingCredentials);
    }
    result.values.insert(
        if plugin == FRIGATE {
            "mqtt_topic"
        } else {
            "mqtt_topics"
        }
        .into(),
        json!(topics.join(";")),
    );
    if plugin == FRIGATE {
        if let Some(base) = text(&config.frigate_base_url) {
            http_url(base, false, false)?;
            result.values.insert("frigate_base_url".into(), json!(base));
        }
    }
    if plugin == KERBEROS && !config.camera_live_urls.is_empty() {
        if config.camera_live_urls.len() > 32 {
            return Err(MigrationError::InvalidCameraMapping);
        }
        for (id, url) in &config.camera_live_urls {
            if !camera_identity(id) {
                return Err(MigrationError::InvalidCameraMapping);
            }
            http_url(url, true, true).map_err(|_| MigrationError::InvalidCameraMapping)?;
        }
        let mapping = serde_json::to_string(&config.camera_live_urls)
            .map_err(|_| MigrationError::InvalidCameraMapping)?;
        if mapping.len() > 16384 {
            return Err(MigrationError::InvalidCameraMapping);
        }
        result.secrets.insert("camera_live_urls".into(), mapping);
    }
    if plugin == RING {
        if let Some(template) = text(&config.ring_snapshot_url_template) {
            let rendered = template
                .replace("{location_id}", "location")
                .replace("{device_id}", "device")
                .replace("{event}", "motion");
            if rendered.contains(['{', '}']) {
                return Err(MigrationError::InvalidCameraMapping);
            }
            let url = http_url(&rendered, true, false)
                .map_err(|_| MigrationError::InvalidCameraMapping)?;
            let authority = template
                .split_once("://")
                .ok_or(MigrationError::InvalidCameraMapping)?
                .1
                .split(['/', '?', '#'])
                .next()
                .unwrap_or_default();
            if authority.contains(['{', '}']) {
                return Err(MigrationError::InvalidCameraMapping);
            }
            let mut base = url.clone();
            base.set_query(None);
            base.set_fragment(None);
            // The origin is the stable authority even when placeholders occupy path segments.
            base.set_path("/");
            let kind = match url
                .path()
                .rsplit('.')
                .next()
                .unwrap_or("")
                .to_ascii_lowercase()
                .as_str()
            {
                "jpg" | "jpeg" => "jpeg",
                "png" => "png",
                "webp" => "webp",
                "mp4" => "video",
                _ if url.path().starts_with("/api/camera_proxy/") => "jpeg",
                _ => return Err(MigrationError::InvalidCameraMapping),
            };
            result.values.insert(
                "snapshot_base_url".into(),
                json!(base.as_str().trim_end_matches('/')),
            );
            result
                .values
                .insert("snapshot_media_kind".into(), json!(kind));
            result
                .secrets
                .insert("snapshot_url_template".into(), template.to_owned());
            if let Some(token) = text(&config.ha_longlived_token) {
                if camera_ha_origin(config).is_some_and(|ha| ha.origin() == url.origin()) {
                    result
                        .secrets
                        .insert("snapshot_bearer_token".into(), token.to_owned());
                }
            }
        }
    }
    Ok(Some(result))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configured() -> FullConfig {
        FullConfig {
            ha_use_direct_api: true,
            ha_url: Some("http://ha.test".into()),
            ha_port: Some(8123),
            ha_longlived_token: Some("fixture-token".into()),
            show_header_toggles: Some(false),
            show_home_section: Some(false),
            show_ha_sensors: Some(false),
            show_ha_numbers: Some(false),
            show_ha_covers: Some(false),
            show_ha_media: Some(false),
            show_ha_scenes: Some(false),
            show_ha_weather: Some(false),
            ..FullConfig::default()
        }
    }
    fn home(id: &str, entity: &str, enabled: bool) -> crate::HomeButtonConfig {
        serde_json::from_value(json!({"id":id,"label":format!("Label {id}"),"entity":entity,"enabled":enabled,"domain":entity.split('.').next().unwrap()})).unwrap()
    }
    fn dashboard(seed: &PluginSeed) -> Value {
        serde_json::from_str(seed.values["dashboard_layout"].as_str().unwrap()).unwrap()
    }

    #[test]
    fn household_controls_clamps_appliances_preserve_order_and_core_ownership() {
        let mut config = configured();
        config.show_home_section = Some(true);
        config.show_header_toggles = Some(true);
        let mut controls = vec![home("core", "input_boolean.do_not_supply_charger", true)];
        controls
            .extend((0..15).map(|i| home(&format!("home{i}"), &format!("switch.room_{i}"), true)));
        controls.push(home("hidden", "switch.hidden", false));
        config.ha_entities = Some(controls);
        config.header_toggles_config = Some(vec![crate::mqtt::HeaderToggle {
            id: "home0".into(),
            label: "Same target".into(),
            entity: "switch.room_0".into(),
            state_key: None,
        }]);
        config.ha_washer_entity = Some("sensor.washer_remaining".into());
        config.ha_dryer_entity = Some("sensor.dryer_remaining".into());
        config.ha_dishwasher_running_entity = Some("binary_sensor.dishwasher_running".into());
        config.ha_dishwasher_duration_entity = Some("sensor.dishwasher_duration".into());
        config.ha_washer_start_entity = Some("button.washer_start".into());
        config.ha_washer_pause_entity = Some("button.washer_pause".into());
        config.ha_dryer_start_entity = Some("button.dryer_start".into());
        config.ha_dryer_pause_entity = Some("button.dryer_pause".into());
        config.ha_consumption_clamps = Some((0..18).map(|i| format!("sensor.load_{i}")).collect());
        config.ha_generation_clamps = Some(vec!["sensor.pv_1".into(), "sensor.pv_2".into()]);
        config.ha_ev_soc_entity = Some("sensor.dormant_ev".into());
        let before = serde_json::to_value(&config).unwrap();
        let seed = plan_plugin(&config, None, HA).unwrap().unwrap();
        let layout = dashboard(&seed);
        assert_eq!(seed.values["ha_base_url"], "http://ha.test:8123");
        assert_eq!(
            seed.values["watch_entities"]
                .as_str()
                .unwrap()
                .split(',')
                .count(),
            43
        );
        assert_eq!(layout["controls"].as_array().unwrap().len(), 16);
        assert_eq!(layout["controls"][0]["id"], "header-home0");
        assert_eq!(layout["controls"][1]["id"], "home-home0");
        assert_eq!(layout["controls"][1]["order"], 1);
        assert_eq!(layout["controls"][1]["label"], "Label home0");
        assert_eq!(layout["appliances"]["washer_start"], "button.washer_start");
        assert_eq!(
            seed.values["washer_remaining_entity"],
            "sensor.washer_remaining"
        );
        assert_eq!(seed.values["notify_home"], true);
        let public = serde_json::to_string(&seed.values).unwrap();
        for absent in [
            "do_not_supply_charger",
            "switch.hidden",
            "sensor.dormant_ev",
            "fixture-token",
        ] {
            assert!(!public.contains(absent));
        }
        assert!(serde_json::to_value(&config).unwrap() == before);
    }

    #[test]
    fn daemon_fallback_and_nonempty_disabled_overrides_are_distinct() {
        let mut config = configured();
        config.show_home_section = Some(true);
        assert!(matches!(
            plan_plugin(&config, None, HA),
            Err(MigrationError::MissingDaemonState)
        ));
        let daemon = json!({"features":{"ha":true},"ui_config":{"home_buttons":[{"id":"daemon","label":"Daemon label","entity":"switch.daemon"}]}});
        let seed = plan_plugin(&config, Some(&daemon), HA).unwrap().unwrap();
        assert_eq!(dashboard(&seed)["controls"][0]["id"], "home-daemon");
        config.ha_entities = Some(vec![home("disabled", "switch.disabled", false)]);
        assert!(
            dashboard(&plan_plugin(&config, None, HA).unwrap().unwrap())["controls"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        config.ha_entities = Some(vec![]);
        assert_eq!(
            dashboard(&plan_plugin(&config, Some(&daemon), HA).unwrap().unwrap())["controls"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let off = json!({"features":{"ha":false},"ui_config":{"home_buttons":[{"id":"daemon","label":"Daemon label","entity":"switch.daemon"}]}});
        assert!(
            dashboard(&plan_plugin(&config, Some(&off), HA).unwrap().unwrap())["controls"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn namespaced_ids_are_deterministic_and_only_documented_core_aliases_are_omitted() {
        let mut config = configured();
        config.show_home_section = Some(true);
        config.ha_entities = Some(vec![
            home("ID with spaces", "switch.no_feed", true),
            home("core", "input_boolean.no_feed", true),
            home(&"a".repeat(100), "switch.other", true),
        ]);
        let first = dashboard(&plan_plugin(&config, None, HA).unwrap().unwrap());
        let second = dashboard(&plan_plugin(&config, None, HA).unwrap().unwrap());
        assert_eq!(first, second);
        assert_eq!(first["controls"].as_array().unwrap().len(), 2);
        assert_eq!(first["controls"][0]["entity"], "switch.no_feed");
        for control in first["controls"].as_array().unwrap() {
            assert!(control["id"].as_str().unwrap().len() <= 64);
        }
    }

    #[test]
    fn discovery_write_surfaces_require_explicit_entity_selection() {
        for field in [
            "show_ha_numbers",
            "show_ha_covers",
            "show_ha_media",
            "show_ha_scenes",
        ] {
            let mut value = serde_json::to_value(configured()).unwrap();
            value[field] = json!(true);
            let config = serde_json::from_value(value).unwrap();
            assert!(matches!(
                plan_plugin(&config, None, HA),
                Err(MigrationError::NeedsEntitySelection)
            ));
        }
        let mut config = configured();
        config.show_ha_sensors = Some(true);
        config.show_ha_weather = Some(true);
        let seed = plan_plugin(&config, None, HA).unwrap().unwrap();
        assert_eq!(
            seed.values["discovery_domains"],
            "sensor,binary_sensor,weather"
        );
    }

    #[test]
    fn invalid_roles_capacity_and_endpoint_disagreements_fail_without_input_echo() {
        let mut config = configured();
        config.ha_consumption_clamps = Some((0..65).map(|i| format!("sensor.load_{i}")).collect());
        assert!(matches!(
            plan_plugin(&config, None, HA),
            Err(MigrationError::CapacityExceeded)
        ));
        config.ha_consumption_clamps = None;
        config.ha_dishwasher_duration_entity = Some("sensor.duration".into());
        assert!(matches!(
            plan_plugin(&config, None, HA),
            Err(MigrationError::InvalidSelection)
        ));
        config.ha_dishwasher_duration_entity = None;
        for (url, error) in [
            ("http://ha.test:1234", MigrationError::EndpointConflict),
            ("http://ha.test/prefix", MigrationError::InvalidEndpoint),
            (
                "http://private:secret@ha.test",
                MigrationError::InvalidEndpoint,
            ),
        ] {
            config.ha_url = Some(url.into());
            let result = plan_plugin(&config, None, HA);
            assert!(matches!(result,Err(actual) if actual==error));
            assert!(!error.to_string().contains("secret"));
        }
        config.ha_url = Some("https://[::1]:8123/".into());
        assert_eq!(
            plan_plugin(&config, None, HA).unwrap().unwrap().values["ha_base_url"],
            "https://[::1]:8123"
        );
    }

    #[test]
    fn camera_seeds_partition_filters_and_preserve_private_live_destinations() {
        let mut config = configured();
        config.mqtt_ha_host = Some("broker.test".into());
        config.mqtt_ha_login = Some("camera-user".into());
        config.mqtt_ha_password = Some("camera-password".into());
        config.camera_topic=Some("kerberos/desktop/events;kerberos/desktop/events;frigate/events;ring/house/camera/+/+/state".into());
        config.camera_live_urls.insert(
            "front_camera".into(),
            "https://camera.test/live?token=private#view".into(),
        );
        config.frigate_base_url = Some("https://frigate.test:5000/prefix".into());
        let kerberos = plan_plugin(&config, None, KERBEROS).unwrap().unwrap();
        assert_eq!(
            kerberos.values["mqtt_topics"],
            "kerberos/agent/+;kerberos/hub/+"
        );
        assert_eq!(kerberos.values["mqtt_host"], "broker.test");
        assert_eq!(kerberos.values["mqtt_tls"], false);
        assert!(kerberos.secrets["camera_live_urls"].contains("private"));
        assert!(!serde_json::to_string(&kerberos.values)
            .unwrap()
            .contains("private"));
        assert!(!kerberos.secrets.contains_key("ha_token"));
        let frigate = plan_plugin(&config, None, FRIGATE).unwrap().unwrap();
        assert_eq!(frigate.values["mqtt_topic"], "frigate/events");
        let ring = plan_plugin(&config, None, RING).unwrap().unwrap();
        assert_eq!(ring.values["mqtt_topics"], "ring/house/camera/+/+/state");
        assert!(!ring.secrets.contains_key("snapshot_bearer_token"));
    }

    #[test]
    fn ring_snapshot_uses_only_exact_ha_origin_and_requires_known_media_kind() {
        let mut config = configured();
        config.camera_topic = Some("ring/+/camera/+/motion/state".into());
        config.mqtt_ha_host = Some("broker.test".into());
        config.ring_snapshot_url_template =
            Some("http://ha.test:8123/api/camera_proxy/camera.front?site={location_id}".into());
        let seed = plan_plugin(&config, None, RING).unwrap().unwrap();
        assert_eq!(seed.values["snapshot_base_url"], "http://ha.test:8123");
        assert_eq!(seed.values["snapshot_media_kind"], "jpeg");
        assert_eq!(seed.secrets["snapshot_bearer_token"], "fixture-token");
        config.ring_snapshot_url_template = Some("https://other.test/{device_id}.png".into());
        let seed = plan_plugin(&config, None, RING).unwrap().unwrap();
        assert!(!seed.secrets.contains_key("snapshot_bearer_token"));
        assert_eq!(seed.values["snapshot_media_kind"], "png");
        config.ring_snapshot_url_template = Some("https://other.test/unknown-format".into());
        assert!(matches!(
            plan_plugin(&config, None, RING),
            Err(MigrationError::InvalidCameraMapping)
        ));
    }

    #[test]
    fn camera_errors_and_unavailable_ha_defaults_are_independent() {
        let mut config = configured();
        config.show_home_section = Some(true);
        config.camera_topic = Some("kerberos/agent/+".into());
        config.mqtt_ha_host = Some("broker.test".into());
        assert!(matches!(
            plan_plugin(&config, None, HA),
            Err(MigrationError::MissingDaemonState)
        ));
        assert!(plan_plugin(&config, None, KERBEROS).unwrap().is_some());
        config.camera_topic = Some("kerberos/#".into());
        assert!(matches!(
            plan_plugin(&config, None, KERBEROS),
            Err(MigrationError::UnsupportedCameraTopic)
        ));
        config.camera_topic = Some("frigate/events;other/events".into());
        assert!(matches!(
            plan_plugin(&config, None, FRIGATE),
            Err(MigrationError::UnsupportedCameraTopic)
        ));
        config.camera_topic = Some("kerberos/agent/+".into());
        config
            .camera_live_urls
            .insert("bad/id".into(), "https://camera.test".into());
        assert!(matches!(
            plan_plugin(&config, None, KERBEROS),
            Err(MigrationError::InvalidCameraMapping)
        ));
    }

    #[test]
    fn explicit_namespace_is_authoritative_even_when_blank_and_secret_free() {
        let mut config = configured();
        config.modules=serde_json::from_value(json!({HA:{"schema_version":1,"values":{"ha_base_url":"https://restored.test","watch_entities":"","dashboard_layout":""},"secrets":{}}})).unwrap();
        let seed = plan_plugin(&config, None, HA).unwrap().unwrap();
        assert_eq!(seed.values["ha_base_url"], "https://restored.test");
        assert_eq!(seed.values["watch_entities"], "");
        assert!(seed.secrets.is_empty());
        assert!(!seed.values.contains_key("notify_home"));
        config.modules=serde_json::from_value(json!({"custom.module":{"schema_version":1,"values":{"future":42},"secrets":{"key":"fixture"}}})).unwrap();
        let seed = plan_plugin(&config, None, "custom.module")
            .unwrap()
            .unwrap();
        assert_eq!(seed.values["future"], 42);
        assert_eq!(seed.secrets["key"], "fixture");
        config.modules =
            serde_json::from_value(json!({HA:{"schema_version":2,"values":{}}})).unwrap();
        assert!(matches!(
            plan_plugin(&config, None, HA),
            Err(MigrationError::UnsupportedNamespace)
        ));
    }

    #[test]
    fn merged_seed_is_once_only_and_preserves_explicit_removals_and_credentials() {
        let seed = plan_plugin(&configured(), None, HA).unwrap().unwrap();
        let mut current = SettingsData::default();
        current
            .values
            .insert("ha_base_url".into(), json!("http://ha.test:8123/"));
        current.values.insert("watch_entities".into(), json!(""));
        current.secret_fields.insert("ha_token".into());
        let mut merged = merge_seed(&current, &seed).unwrap();
        assert_eq!(merged.legacy_migration_version, 1);
        assert_eq!(
            merged.values["watch_entities"],
            seed.values["watch_entities"]
        );
        assert!(!merged.secrets.contains_key("ha_token"));
        merged.values.insert("watch_entities".into(), json!(""));
        merged.values.remove("dashboard_layout");
        assert!(merge_seed(&merged, &seed).unwrap() == merged);
        let fresh = merge_seed(&SettingsData::default(), &seed).unwrap();
        assert_eq!(fresh.secrets["ha_token"], "fixture-token");
        current
            .values
            .insert("ha_base_url".into(), json!("https://elsewhere.test"));
        assert!(matches!(
            merge_seed(&current, &seed),
            Err(MigrationError::EndpointConflict)
        ));
    }

    #[test]
    fn existing_partial_watch_list_inherits_legacy_clamps_once_in_original_order() {
        let mut config = configured();
        config.ha_consumption_clamps = Some(vec!["sensor.clamp_a".into(), "sensor.clamp_b".into()]);
        config.ha_generation_clamps = Some(vec!["sensor.clamp_b".into(), "sensor.pv".into()]);
        let planned = plan_plugin(&config, None, HA).unwrap().unwrap();
        let mut current = SettingsData::default();
        current.values.insert(
            "watch_entities".into(),
            json!("sensor.saved, sensor.clamp_b\nsensor.saved"),
        );
        current
            .values
            .insert("action_entities".into(), json!("button.saved"));
        let mut merged = merge_seed(&current, &planned).unwrap();
        let watch = merged.values["watch_entities"].as_str().unwrap();
        assert!(watch.starts_with("sensor.saved,sensor.clamp_b,"));
        let targets: Vec<_> = watch.split(',').collect();
        assert_eq!(
            targets
                .iter()
                .filter(|target| **target == "sensor.clamp_b")
                .count(),
            1
        );
        assert!(targets.contains(&"sensor.clamp_a") && targets.contains(&"sensor.pv"));
        assert_eq!(merged.values["action_entities"], "button.saved");
        assert!(
            !targets.contains(&"button.saved"),
            "count action reads without rewriting their selection"
        );
        merged
            .values
            .insert("watch_entities".into(), json!("sensor.saved"));
        assert!(merge_seed(&merged, &planned).unwrap() == merged);

        config.modules = serde_json::from_value(
            json!({HA:{"schema_version":1,"values":{"watch_entities":""},"secrets":{}}}),
        )
        .unwrap();
        let explicit = plan_plugin(&config, None, HA).unwrap().unwrap();
        let restored = merge_seed(&SettingsData::default(), &explicit).unwrap();
        assert_eq!(
            restored.values["watch_entities"], "",
            "an explicit portable namespace never falls back to legacy reads"
        );
    }

    #[test]
    fn merged_legacy_read_capacity_includes_existing_controls_profiles_and_layout() {
        let mut planned = seed(HA);
        planned
            .values
            .insert("watch_entities".into(), json!("sensor.legacy"));
        let mut current = SettingsData::default();
        current.values.insert(
            "watch_entities".into(),
            json!((0..62)
                .map(|index| format!("sensor.saved_{index}"))
                .collect::<Vec<_>>()
                .join(",")),
        );
        current
            .values
            .insert("action_entities".into(), json!("button.saved"));
        assert!(
            merge_seed(&current, &planned).is_ok(),
            "63 watch reads plus one action fit"
        );
        for (field, value) in [
            ("binary_entities", json!("switch.extra")),
            ("washer_remaining_entity", json!("sensor.extra")),
            (
                "dashboard_layout",
                json!(json!({"version":1,"controls":[{"entity":"light.extra"}]}).to_string()),
            ),
            (
                "dashboard_layout",
                json!(
                    json!({"version":1,"appliances":{"washer_start":"button.extra"}}).to_string()
                ),
            ),
        ] {
            let mut over = current.clone();
            over.values.insert(field.into(), value);
            assert!(matches!(
                merge_seed(&over, &planned),
                Err(MigrationError::NeedsEntitySelection)
            ));
            assert_eq!(
                over.legacy_migration_version, 0,
                "planning cannot mark rejected input complete"
            );
        }
        current.values.insert(
            "watch_entities".into(),
            json!((0..64)
                .map(|index| format!("sensor.saved_{index}"))
                .collect::<Vec<_>>()
                .join(",")),
        );
        assert!(matches!(
            merge_seed(&current, &planned),
            Err(MigrationError::NeedsEntitySelection)
        ));
    }

    #[test]
    fn endpoint_credential_binding_conflicts_never_mix_accounts() {
        let mut planned = seed(RING);
        planned
            .values
            .insert("mqtt_host".into(), json!("broker.test"));
        planned.secrets.extend([
            ("mqtt_username".into(), "first".into()),
            ("mqtt_password".into(), "private".into()),
        ]);
        let mut current = SettingsData::default();
        current
            .values
            .insert("mqtt_host".into(), json!("broker.test"));
        current
            .secrets
            .insert("mqtt_username".into(), "second".into());
        assert!(matches!(
            merge_seed(&current, &planned),
            Err(MigrationError::EndpointConflict)
        ));
        assert!(!current.secrets.contains_key("mqtt_password"));
    }
}
