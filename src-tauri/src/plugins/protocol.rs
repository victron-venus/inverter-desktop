//! Bounded, declarative protocol for desktop plugin worker processes.
//!
//! Validation here establishes a wire contract, not a process sandbox, publisher
//! trust, or verification of the files described by a manifest.

use std::collections::HashSet;

use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: u32 = 1;
pub const HOST_API_VERSION: &str = "1.0.0";
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;
/// Includes the newline terminating a frame.
pub const MAX_FRAME_BYTES: usize = 64 * 1024;
pub const MAX_CONTRIBUTIONS: usize = 64;
pub const MAX_ACTION_PARAMS_BYTES: usize = 4 * 1024;
pub const MAX_ACTION_RESULT_BYTES: usize = 16 * 1024;
pub const MAX_ACTION_DEADLINE_MS: u64 = 60_000;
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;
const MAX_JSON_DEPTH: usize = 8;
const MAX_JSON_NODES: usize = 512;
const MAX_INVENTORY_FILES: usize = 128;
const MAX_INVENTORY_BYTES: u64 = 512 * 1024 * 1024;

pub const DESKTOP_TARGETS: &[&str] = &[
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
    "aarch64-pc-windows-msvc",
    "x86_64-pc-windows-msvc",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum HostMessage {
    Hello {
        protocol_version: u32,
        host_api_version: String,
        plugin_id: String,
    },
    Action {
        request_id: String,
        action_id: String,
        params: Value,
        /// Relative deadline, in milliseconds, also enforced by the host.
        deadline_ms: u64,
    },
    Cancel {
        request_id: String,
    },
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkerMessage {
    Ready {
        protocol_version: u32,
        host_api_version: String,
        plugin_id: String,
    },
    /// Replaces this worker's entire contribution snapshot.
    Contributions {
        items: Vec<DashboardContribution>,
    },
    ActionResult {
        request_id: String,
        value: Value,
    },
    ActionError {
        request_id: String,
        code: String,
        message: String,
    },
    /// Worker-scoped data only; never a host command or a Tauri event name.
    Event {
        name: String,
        data: Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DashboardContribution {
    Text {
        id: String,
        title: String,
        text: String,
    },
    Metric {
        id: String,
        title: String,
        value: f64,
        unit: Option<String>,
    },
    Status {
        id: String,
        title: String,
        value: String,
        tone: StatusTone,
    },
    Action {
        id: String,
        title: String,
        action_id: String,
        label: String,
        params: Value,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StatusTone {
    Neutral,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    pub schema_version: u32,
    pub plugin_id: String,
    pub version: String,
    /// Semantic version requirement for the host API, independent of app version.
    pub host_api: String,
    pub target: String,
    pub entrypoint: String,
    /// Bounded JSON Schema metadata. No schema is fetched or executed here.
    pub config_schema: Value,
    pub permissions: Vec<PluginPermission>,
    pub inventory: Vec<InventoryEntry>,
    /// Optional for local development. Presence does not establish authenticity.
    pub signature: Option<SignatureMetadata>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PluginPermission {
    DashboardContributions,
    PluginConfiguration,
    NetworkHttp,
    NetworkMqtt,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct InventoryEntry {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SignatureMetadata {
    pub algorithm: String,
    pub key_id: String,
    /// A lowercase hexadecimal Ed25519 signature. It is not verified here.
    pub signature: String,
}

fn token(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 128
        || !value.as_bytes()[0].is_ascii_alphanumeric()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte))
    {
        return Err(format!("invalid {label}"));
    }
    Ok(())
}

pub fn validate_plugin_id(value: &str) -> Result<(), String> {
    if value.len() > 128 || !value.contains('.') {
        return Err("plugin_id must be a namespaced lowercase identifier".into());
    }
    for part in value.split('.') {
        if part.is_empty()
            || !part.as_bytes()[0].is_ascii_lowercase()
            || part.ends_with('-')
            || !part
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err("invalid plugin_id component".into());
        }
    }
    Ok(())
}

fn label(value: &str, name: &str, limit: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > limit || value.chars().any(char::is_control) {
        return Err(format!("invalid {name}"));
    }
    Ok(())
}

fn text(value: &str, name: &str, limit: usize) -> Result<(), String> {
    if value.len() > limit
        || value
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(format!("invalid {name}"));
    }
    Ok(())
}

fn visit_json(value: &Value, depth: usize, nodes: &mut usize) -> Result<(), String> {
    *nodes += 1;
    if depth > MAX_JSON_DEPTH || *nodes > MAX_JSON_NODES {
        return Err("JSON value exceeds depth or node limit".into());
    }
    match value {
        Value::Array(values) => {
            for child in values {
                visit_json(child, depth + 1, nodes)?;
            }
        }
        Value::Object(values) => {
            for (key, child) in values {
                if key.len() > 128 || key.chars().any(char::is_control) {
                    return Err("invalid JSON object key".into());
                }
                visit_json(child, depth + 1, nodes)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn bounded_json(value: &Value, limit: usize) -> Result<(), String> {
    visit_json(value, 0, &mut 0)?;
    let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    if bytes.len() > limit {
        return Err("JSON value exceeds byte limit".into());
    }
    Ok(())
}

pub fn validate_action_params(params: &Value) -> Result<(), String> {
    if !params.is_object() {
        return Err("action params must be an object".into());
    }
    bounded_json(params, MAX_ACTION_PARAMS_BYTES)
}

fn validate_versions(protocol_version: u32, host_api_version: &str) -> Result<(), String> {
    if protocol_version != PROTOCOL_VERSION {
        return Err("unsupported worker protocol version".into());
    }
    // This version is the negotiated API version, not the worker's full supported
    // range. The manifest expresses that range before launching the worker.
    if host_api_version != HOST_API_VERSION {
        return Err("worker did not acknowledge the negotiated host API version".into());
    }
    Ok(())
}

fn bounded_serialized_frame<T: Serialize>(message: &T) -> Result<Vec<u8>, String> {
    let mut bytes = serde_json::to_vec(message).map_err(|error| error.to_string())?;
    if bytes.len() >= MAX_FRAME_BYTES {
        return Err("worker frame exceeds byte limit".into());
    }
    bytes.push(b'\n');
    Ok(bytes)
}

pub fn validate_host_message(message: &HostMessage) -> Result<(), String> {
    match message {
        HostMessage::Hello {
            protocol_version,
            host_api_version,
            plugin_id,
        } => {
            validate_versions(*protocol_version, host_api_version)?;
            validate_plugin_id(plugin_id)?;
        }
        HostMessage::Action {
            request_id,
            action_id,
            params,
            deadline_ms,
        } => {
            token(request_id, "request_id")?;
            token(action_id, "action_id")?;
            validate_action_params(params)?;
            if !(1..=MAX_ACTION_DEADLINE_MS).contains(deadline_ms) {
                return Err("action deadline is outside allowed range".into());
            }
        }
        HostMessage::Cancel { request_id } => token(request_id, "request_id")?,
        HostMessage::Shutdown => {}
    }
    bounded_serialized_frame(message)?;
    Ok(())
}

impl DashboardContribution {
    pub fn id(&self) -> &str {
        match self {
            Self::Text { id, .. }
            | Self::Metric { id, .. }
            | Self::Status { id, .. }
            | Self::Action { id, .. } => id,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        token(self.id(), "contribution id")?;
        let title = match self {
            Self::Text {
                title, text: body, ..
            } => {
                text(body, "contribution text", 4096)?;
                title
            }
            Self::Metric {
                title, value, unit, ..
            } => {
                if !value.is_finite() {
                    return Err("metric value must be finite".into());
                }
                if let Some(unit) = unit {
                    label(unit, "metric unit", 32)?;
                }
                title
            }
            Self::Status { title, value, .. } => {
                label(value, "status value", 4096)?;
                title
            }
            Self::Action {
                title,
                action_id,
                label: action_label,
                params,
                ..
            } => {
                token(action_id, "action_id")?;
                label(action_label, "action label", 128)?;
                validate_action_params(params)?;
                title
            }
        };
        label(title, "contribution title", 128)
    }
}

pub fn validate_contributions(items: &[DashboardContribution]) -> Result<(), String> {
    if items.len() > MAX_CONTRIBUTIONS {
        return Err("too many dashboard contributions".into());
    }
    let mut ids = HashSet::new();
    for item in items {
        item.validate()?;
        if !ids.insert(item.id()) {
            return Err("duplicate dashboard contribution id".into());
        }
    }
    Ok(())
}

pub fn validate_worker_message(message: &WorkerMessage) -> Result<(), String> {
    match message {
        WorkerMessage::Ready {
            protocol_version,
            host_api_version,
            plugin_id,
        } => {
            validate_versions(*protocol_version, host_api_version)?;
            validate_plugin_id(plugin_id)?;
        }
        WorkerMessage::Contributions { items } => validate_contributions(items)?,
        WorkerMessage::ActionResult { request_id, value } => {
            token(request_id, "request_id")?;
            bounded_json(value, MAX_ACTION_RESULT_BYTES)?;
        }
        WorkerMessage::ActionError {
            request_id,
            code,
            message,
        } => {
            token(request_id, "request_id")?;
            token(code, "error code")?;
            text(message, "error message", 1024)?;
        }
        WorkerMessage::Event { name, data } => {
            token(name, "worker event name")?;
            bounded_json(data, MAX_ACTION_RESULT_BYTES)?;
        }
    }
    bounded_serialized_frame(message)?;
    Ok(())
}

pub fn validate_handshake(expected_plugin_id: &str, message: &WorkerMessage) -> Result<(), String> {
    validate_plugin_id(expected_plugin_id)?;
    validate_worker_message(message)?;
    match message {
        WorkerMessage::Ready { plugin_id, .. } if plugin_id == expected_plugin_id => Ok(()),
        WorkerMessage::Ready { .. } => Err("worker plugin identity mismatch".into()),
        _ => Err("worker must acknowledge the handshake before sending data".into()),
    }
}

fn frame_payload(bytes: &[u8]) -> Result<&[u8], String> {
    if bytes.is_empty() || bytes.len() > MAX_FRAME_BYTES {
        return Err("empty or oversized worker frame".into());
    }
    let payload = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    let payload = payload.strip_suffix(b"\r").unwrap_or(payload);
    // Input from a line reader can omit the delimiter; it must still leave room
    // for the newline on the wire, and cannot smuggle additional frames.
    if payload.is_empty()
        || payload.len() >= MAX_FRAME_BYTES
        || payload.iter().any(|byte| matches!(byte, b'\r' | b'\n'))
    {
        return Err("invalid newline-delimited worker frame".into());
    }
    Ok(payload)
}

pub fn parse_worker_frame(bytes: &[u8]) -> Result<WorkerMessage, String> {
    let message = serde_json::from_slice(frame_payload(bytes)?)
        .map_err(|error| format!("invalid worker message: {error}"))?;
    validate_worker_message(&message)?;
    Ok(message)
}

pub fn encode_host_frame(message: &HostMessage) -> Result<Vec<u8>, String> {
    validate_host_message(message)?;
    bounded_serialized_frame(message)
}

pub fn parse_manifest(bytes: &[u8]) -> Result<PluginManifest, String> {
    if bytes.is_empty() || bytes.len() > MAX_MANIFEST_BYTES {
        return Err("empty or oversized plugin manifest".into());
    }
    let manifest: PluginManifest = serde_json::from_slice(bytes)
        .map_err(|error| format!("invalid plugin manifest: {error}"))?;
    manifest.validate()?;
    Ok(manifest)
}

fn relative_path(path: &str) -> Result<(), String> {
    if path.is_empty() || path.len() > 240 || path.contains('\\') || path.contains(':') {
        return Err("entrypoint and inventory paths must be portable relative paths".into());
    }
    for component in path.split('/') {
        if component.is_empty()
            || component == "."
            || component == ".."
            || component.starts_with('.')
            || component.ends_with('.')
            || component.ends_with(' ')
            || component
                .bytes()
                .any(|byte| !byte.is_ascii_alphanumeric() && !b"-_.".contains(&byte))
        {
            return Err("invalid entrypoint or inventory path component".into());
        }
        let stem = component.split('.').next().unwrap_or("");
        let uppercase = stem.to_ascii_uppercase();
        if matches!(uppercase.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ((uppercase.starts_with("COM") || uppercase.starts_with("LPT"))
                && uppercase.len() == 4
                && matches!(uppercase.as_bytes()[3], b'1'..=b'9'))
        {
            return Err("reserved platform filename in plugin inventory".into());
        }
    }
    Ok(())
}

fn lowercase_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn reject_schema_references(value: &Value) -> Result<(), String> {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                if matches!(key.as_str(), "$ref" | "$dynamicRef" | "$recursiveRef") {
                    return Err("configuration schema references are not supported".into());
                }
                reject_schema_references(value)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                reject_schema_references(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

impl PluginManifest {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != MANIFEST_SCHEMA_VERSION {
            return Err("unsupported plugin manifest schema".into());
        }
        validate_plugin_id(&self.plugin_id)?;
        label(&self.version, "plugin version", 128)?;
        Version::parse(&self.version).map_err(|_| "invalid plugin semantic version".to_string())?;
        label(&self.host_api, "host API requirement", 128)?;
        let requirement = VersionReq::parse(&self.host_api)
            .map_err(|_| "invalid host API semantic version requirement".to_string())?;
        let current = Version::parse(HOST_API_VERSION).map_err(|error| error.to_string())?;
        if !requirement.matches(&current) {
            return Err("plugin does not support this host API version".into());
        }
        if !DESKTOP_TARGETS.contains(&self.target.as_str()) {
            return Err("plugin target must be a supported desktop target".into());
        }
        relative_path(&self.entrypoint)?;
        bounded_json(&self.config_schema, MAX_ACTION_RESULT_BYTES)?;
        if !self.config_schema.is_object()
            || self.config_schema.get("type").and_then(Value::as_str) != Some("object")
        {
            return Err("config_schema must describe a JSON object".into());
        }
        reject_schema_references(&self.config_schema)?;
        let permissions: HashSet<_> = self.permissions.iter().collect();
        if permissions.len() != self.permissions.len() {
            return Err("duplicate plugin permission".into());
        }
        self.validate_inventory()?;
        if let Some(signature) = &self.signature {
            if signature.algorithm != "ed25519" || !lowercase_hex(&signature.signature, 128) {
                return Err("invalid signature metadata encoding".into());
            }
            token(&signature.key_id, "signature key_id")?;
        }
        if serde_json::to_vec(self)
            .map_err(|error| error.to_string())?
            .len()
            > MAX_MANIFEST_BYTES
        {
            return Err("plugin manifest exceeds byte limit".into());
        }
        Ok(())
    }

    fn validate_inventory(&self) -> Result<(), String> {
        if self.inventory.is_empty() || self.inventory.len() > MAX_INVENTORY_FILES {
            return Err("invalid plugin inventory file count".into());
        }
        let mut paths = HashSet::new();
        let mut total_size = 0_u64;
        let mut has_entrypoint = false;
        for item in &self.inventory {
            relative_path(&item.path)?;
            // Case-insensitive collisions cannot be portable across our targets.
            if !paths.insert(item.path.to_ascii_lowercase()) {
                return Err("duplicate plugin inventory path".into());
            }
            if !lowercase_hex(&item.sha256, 64) {
                return Err("invalid inventory sha256 encoding".into());
            }
            total_size = total_size
                .checked_add(item.size)
                .ok_or("plugin inventory size overflow")?;
            if total_size > MAX_INVENTORY_BYTES {
                return Err("plugin inventory exceeds size limit".into());
            }
            has_entrypoint |= item.path == self.entrypoint && item.size > 0;
        }
        if !has_entrypoint {
            return Err("entrypoint must name a nonempty file in the inventory".into());
        }
        Ok(())
    }

    pub fn validate_for_target(&self, target: &str) -> Result<(), String> {
        self.validate()?;
        if self.target != target {
            return Err("plugin package target does not match this host".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ready() -> WorkerMessage {
        WorkerMessage::Ready {
            protocol_version: PROTOCOL_VERSION,
            host_api_version: HOST_API_VERSION.into(),
            plugin_id: "org.example.demo".into(),
        }
    }

    fn manifest() -> PluginManifest {
        PluginManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            plugin_id: "org.example.demo".into(),
            version: "0.1.0".into(),
            host_api: ">=1.0.0, <2.0.0".into(),
            target: "aarch64-apple-darwin".into(),
            entrypoint: "bin/demo-worker".into(),
            config_schema: json!({"type":"object","properties":{}}),
            permissions: vec![PluginPermission::DashboardContributions],
            inventory: vec![InventoryEntry {
                path: "bin/demo-worker".into(),
                size: 100,
                sha256: "a".repeat(64),
            }],
            signature: None,
        }
    }

    fn text_card(id: &str) -> DashboardContribution {
        DashboardContribution::Text {
            id: id.into(),
            title: "Plain text".into(),
            text: "<script>is text, never HTML</script>".into(),
        }
    }

    #[test]
    fn handshake_round_trip_requires_expected_identity_and_negotiated_versions() {
        let bytes = bounded_serialized_frame(&ready()).unwrap();
        let decoded = parse_worker_frame(&bytes).unwrap();
        assert_eq!(decoded, ready());
        assert!(validate_handshake("org.example.demo", &decoded).is_ok());
        assert!(validate_handshake("org.example.other", &decoded).is_err());
        let mut wrong_protocol = ready();
        if let WorkerMessage::Ready {
            protocol_version, ..
        } = &mut wrong_protocol
        {
            *protocol_version = 2;
        }
        assert!(validate_worker_message(&wrong_protocol).is_err());
        let mut wrong_api = ready();
        if let WorkerMessage::Ready {
            host_api_version, ..
        } = &mut wrong_api
        {
            *host_api_version = "2.0.0".into();
        }
        assert!(validate_worker_message(&wrong_api).is_err());
        assert!(validate_handshake(
            "org.example.demo",
            &WorkerMessage::Contributions { items: Vec::new() }
        )
        .is_err());
    }

    #[test]
    fn rejects_unknown_messages_fields_and_multiple_lines() {
        for bytes in [
            br#"{"type":"mqtt_publish","topic":"write"}"#.as_slice(),
            br#"{"type":"ready","protocol_version":1,"host_api_version":"1.0.0","plugin_id":"org.example.demo","extra":true}"#,
            br#"{"type":"action_error","request_id":"1","code":"bad","message":"x","retry":true}"#,
            b"{}\n{}\n",
            b"{\n\"type\":\"shutdown\"\n}",
            b"",
            &[0xff],
        ] {
            assert!(parse_worker_frame(bytes).is_err());
        }
        assert!(parse_worker_frame(&vec![b' '; MAX_FRAME_BYTES + 1]).is_err());
    }

    #[test]
    fn actions_are_bounded_objects_with_a_correlated_deadline() {
        let mut action = HostMessage::Action {
            request_id: "request-1".into(),
            action_id: "refresh".into(),
            params: json!({"force":true}),
            deadline_ms: 5000,
        };
        let bytes = encode_host_frame(&action).unwrap();
        assert!(bytes.ends_with(b"\n"));
        assert_eq!(
            serde_json::from_slice::<HostMessage>(&bytes).unwrap(),
            action
        );
        if let HostMessage::Action { deadline_ms, .. } = &mut action {
            *deadline_ms = 0;
        }
        assert!(encode_host_frame(&action).is_err());
        assert!(validate_action_params(&json!([1, 2])).is_err());
        assert!(
            validate_action_params(&json!({"data":"x".repeat(MAX_ACTION_PARAMS_BYTES)})).is_err()
        );
        let mut nested = json!(true);
        for _ in 0..=MAX_JSON_DEPTH {
            nested = json!({"child":nested});
        }
        assert!(validate_action_params(&nested).is_err());
        assert!(validate_worker_message(&WorkerMessage::ActionResult {
            request_id: "../request".into(),
            value: Value::Null,
        })
        .is_err());
        assert!(validate_worker_message(&WorkerMessage::ActionResult {
            request_id: "request-1".into(),
            value: json!("x".repeat(MAX_ACTION_RESULT_BYTES)),
        })
        .is_err());
    }

    #[test]
    fn contributions_are_bounded_unique_and_strictly_declarative() {
        assert!(validate_contributions(&[text_card("first")]).is_ok());
        assert!(validate_contributions(&[text_card("first"), text_card("first")]).is_err());
        let items: Vec<_> = (0..=MAX_CONTRIBUTIONS)
            .map(|index| text_card(&format!("item-{index}")))
            .collect();
        assert!(validate_contributions(&items).is_err());
        assert!(serde_json::from_value::<DashboardContribution>(json!({
            "kind":"text","id":"card","title":"Title","text":"Hello","html":"<b>html</b>"
        }))
        .is_err());
        assert!(DashboardContribution::Metric {
            id: "metric".into(),
            title: "Title".into(),
            value: f64::NAN,
            unit: None,
        }
        .validate()
        .is_err());
        assert!(DashboardContribution::Action {
            id: "action".into(),
            title: "Title".into(),
            action_id: "do-work".into(),
            label: "Run".into(),
            params: json!(["unsupported"]),
        }
        .validate()
        .is_err());
        let items = (0..20)
            .map(|index| DashboardContribution::Text {
                id: format!("item-{index}"),
                title: "Title".into(),
                text: "x".repeat(4096),
            })
            .collect();
        assert!(validate_worker_message(&WorkerMessage::Contributions { items }).is_err());
    }

    #[test]
    fn manifest_accepts_compatible_desktop_metadata_without_claiming_authenticity() {
        let mut manifest = manifest();
        manifest.signature = Some(SignatureMetadata {
            algorithm: "ed25519".into(),
            key_id: "example-development".into(),
            signature: "0".repeat(128),
        });
        // A syntactically valid all-zero signature is deliberately not a trust check.
        assert!(manifest.validate().is_ok());
        let encoded = serde_json::to_vec(&manifest).unwrap();
        assert_eq!(parse_manifest(&encoded).unwrap(), manifest);
        assert!(manifest.validate_for_target("aarch64-apple-darwin").is_ok());
        assert!(manifest.validate_for_target("x86_64-apple-darwin").is_err());
        assert!(parse_manifest(&vec![b' '; MAX_MANIFEST_BYTES + 1]).is_err());
    }

    #[test]
    fn manifest_rejects_incompatible_api_mobile_targets_and_unknown_permissions() {
        let mut value = serde_json::to_value(manifest()).unwrap();
        for (field, invalid) in [
            ("schema_version", json!(2)),
            ("plugin_id", json!("../escape")),
            ("version", json!("latest")),
            ("host_api", json!(">=2.0.0")),
            ("target", json!("aarch64-linux-android")),
            ("config_schema", json!({"type":"array"})),
            ("permissions", json!(["mqtt_publish_core"])),
        ] {
            let old = value[field].clone();
            value[field] = invalid;
            assert!(
                parse_manifest(&serde_json::to_vec(&value).unwrap()).is_err(),
                "{field}"
            );
            value[field] = old;
        }
        value["unexpected"] = json!(true);
        assert!(parse_manifest(&serde_json::to_vec(&value).unwrap()).is_err());
    }

    #[test]
    fn manifest_inventory_paths_cannot_escape_or_collide_across_desktop_targets() {
        for path in [
            "../worker",
            "/worker",
            "C:/worker",
            "bin\\worker",
            "bin//worker",
            "bin/./worker",
            "bin/CON.exe",
            "bin/LPT1",
            "bin/worker.",
            "bin/.hidden",
            "bin/worker ",
        ] {
            let mut manifest = manifest();
            manifest.entrypoint = path.into();
            manifest.inventory[0].path = path.into();
            assert!(manifest.validate().is_err(), "{path}");
        }
        let mut manifest = manifest();
        let mut collision = manifest.inventory[0].clone();
        collision.path = "bin/DEMO-worker".into();
        manifest.inventory.push(collision);
        assert!(manifest.validate().is_err());
        manifest.inventory.pop();
        manifest.inventory[0].sha256 = "not-a-digest".into();
        assert!(manifest.validate().is_err());
        manifest.inventory[0].sha256 = "a".repeat(64);
        manifest.inventory[0].size = u64::MAX;
        assert!(manifest.validate().is_err());
        manifest.inventory[0].size = 0;
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn configuration_schema_references_and_duplicate_permissions_are_rejected() {
        let mut manifest = manifest();
        manifest.config_schema = json!({"type":"object","properties":{"host":{"$ref":"https://example.invalid/schema"}}});
        assert!(manifest.validate().is_err());
        manifest.config_schema = json!({"type":"object"});
        manifest
            .permissions
            .push(PluginPermission::DashboardContributions);
        assert!(manifest.validate().is_err());
    }
}
