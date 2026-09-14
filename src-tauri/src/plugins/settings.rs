//! Bounded declarative settings, independent of the webview and core configuration.

use super::protocol::{PluginManifest, PluginPermission, WorkerConfiguration};
use super::settings_store::SettingsData;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const MAX_FIELDS: usize = 32;
const MAX_STRING: usize = 4096;

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum SettingType {
    String,
    Boolean,
    Number,
    Integer,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSchema {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    properties: BTreeMap<String, RawField>,
    #[serde(default)]
    required: Vec<String>,
    #[serde(rename = "additionalProperties")]
    additional_properties: Option<bool>,
    title: Option<String>,
    description: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawField {
    #[serde(rename = "type")]
    kind: SettingType,
    title: Option<String>,
    description: Option<String>,
    #[serde(default, rename = "writeOnly")]
    secret: bool,
    #[serde(rename = "enum")]
    choices: Option<Vec<String>>,
    minimum: Option<f64>,
    maximum: Option<f64>,
    #[serde(rename = "minLength")]
    min_length: Option<usize>,
    #[serde(rename = "maxLength")]
    max_length: Option<usize>,
    #[serde(default, deserialize_with = "present_default")]
    default: Option<Value>,
}

fn present_default<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}

#[derive(Clone, Serialize)]
pub(crate) struct SettingField {
    key: String,
    title: String,
    description: Option<String>,
    #[serde(rename = "type")]
    kind: SettingType,
    required: bool,
    secret: bool,
    #[serde(rename = "enum")]
    choices: Option<Vec<String>>,
    minimum: Option<f64>,
    maximum: Option<f64>,
    min_length: Option<usize>,
    max_length: Option<usize>,
    #[serde(skip)]
    default: Option<Value>,
}

#[derive(Serialize)]
pub(crate) struct PluginSettingsView {
    plugin_id: String,
    version: String,
    revision: String,
    fields: Vec<SettingField>,
    values: BTreeMap<String, Value>,
    secret_present: BTreeMap<String, bool>,
}

pub(crate) struct SettingsSchema {
    fields: Vec<SettingField>,
}

fn bounded_text(text: &Option<String>, maximum: usize) -> bool {
    text.as_ref().is_none_or(|text| text.len() <= maximum)
}

impl SettingsSchema {
    pub(crate) fn compile(manifest: &PluginManifest) -> Result<Self, String> {
        if !manifest
            .permissions
            .contains(&PluginPermission::PluginConfiguration)
        {
            return Err("Plugin does not declare configuration permission".into());
        }
        // Never return serde errors: malformed schema defaults could contain secrets.
        let raw: RawSchema = serde_json::from_value(manifest.config_schema.clone())
            .map_err(|_| "Unsupported plugin settings schema")?;
        if raw.kind != "object"
            || raw.additional_properties == Some(true)
            || raw.properties.len() > MAX_FIELDS
            || raw.required.len() > MAX_FIELDS
            || !bounded_text(&raw.title, 128)
            || !bounded_text(&raw.description, 512)
        {
            return Err("Unsupported plugin settings schema".into());
        }
        let required: BTreeSet<_> = raw.required.iter().collect();
        if required.len() != raw.required.len()
            || required
                .iter()
                .any(|key| !raw.properties.contains_key(*key))
        {
            return Err("Invalid required plugin settings fields".into());
        }
        let mut fields = Vec::new();
        for (key, raw) in raw.properties {
            if key.is_empty()
                || key.len() > 64
                || !key.as_bytes()[0].is_ascii_alphanumeric()
                || !key
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_-.".contains(&c))
                || matches!(key.as_str(), "__proto__" | "constructor" | "prototype")
                || !bounded_text(&raw.title, 128)
                || !bounded_text(&raw.description, 512)
                || (raw.secret
                    && (raw.kind != SettingType::String
                        || raw.default.is_some()
                        || raw.choices.is_some()))
                || (raw.kind != SettingType::String
                    && (raw.choices.is_some()
                        || raw.min_length.is_some()
                        || raw.max_length.is_some()))
                || (!matches!(raw.kind, SettingType::Number | SettingType::Integer)
                    && (raw.minimum.is_some() || raw.maximum.is_some()))
                || raw.min_length.is_some_and(|n| n > MAX_STRING)
                || raw.max_length.is_some_and(|n| n > MAX_STRING)
                || raw
                    .min_length
                    .zip(raw.max_length)
                    .is_some_and(|(min, max)| min > max)
                || raw
                    .minimum
                    .zip(raw.maximum)
                    .is_some_and(|(min, max)| min > max)
                || raw.minimum.is_some_and(|n| !n.is_finite())
                || raw.maximum.is_some_and(|n| !n.is_finite())
                || raw.choices.as_ref().is_some_and(|items| {
                    items.is_empty()
                        || items.len() > 32
                        || items.iter().any(|item| item.len() > MAX_STRING)
                        || items.iter().collect::<BTreeSet<_>>().len() != items.len()
                })
            {
                return Err("Unsupported plugin settings field".into());
            }
            let field = SettingField {
                required: required.contains(&key),
                title: raw.title.unwrap_or_else(|| key.clone()),
                key,
                description: raw.description,
                kind: raw.kind,
                secret: raw.secret,
                choices: raw.choices,
                minimum: raw.minimum,
                maximum: raw.maximum,
                min_length: raw.min_length,
                max_length: raw.max_length,
                default: raw.default,
            };
            if let Some(choices) = &field.choices {
                for choice in choices {
                    field.validate(&Value::String(choice.clone()))?;
                }
            }
            if let Some(default) = &field.default {
                field.validate(default)?;
            }
            fields.push(field);
        }
        Ok(Self { fields })
    }

    fn check_classification(&self, data: &SettingsData) -> Result<(), String> {
        if self.fields.iter().any(|field| {
            !field.secret
                && (data.secret_fields.contains(&field.key)
                    || data.secrets.contains_key(&field.key))
        }) {
            return Err("A previously secret setting cannot become a public field".into());
        }
        Ok(())
    }

    fn public_values(&self, data: &SettingsData) -> BTreeMap<String, Value> {
        self.fields
            .iter()
            .filter(|field| !field.secret)
            .filter_map(|field| {
                data.values
                    .get(&field.key)
                    .or(field.default.as_ref())
                    .map(|value| (field.key.clone(), value.clone()))
            })
            .collect()
    }

    pub(crate) fn view(
        &self,
        manifest: &PluginManifest,
        digest: &str,
        data: &SettingsData,
    ) -> Result<PluginSettingsView, String> {
        self.check_classification(data)?;
        Ok(PluginSettingsView {
            plugin_id: manifest.plugin_id.clone(),
            version: manifest.version.clone(),
            revision: format!("{digest}:{}", data.revision),
            fields: self.fields.clone(),
            values: self.public_values(data),
            secret_present: self
                .fields
                .iter()
                .filter(|field| field.secret)
                .map(|field| (field.key.clone(), data.secrets.contains_key(&field.key)))
                .collect(),
        })
    }

    pub(crate) fn merge(
        &self,
        digest: &str,
        current: &SettingsData,
        revision: &str,
        values: BTreeMap<String, Value>,
        changes: BTreeMap<String, Option<String>>,
    ) -> Result<SettingsData, String> {
        self.check_classification(current)?;
        if revision != format!("{digest}:{}", current.revision) {
            return Err("Plugin or settings changed; reopen settings before saving".into());
        }
        if values.len() > MAX_FIELDS
            || changes.len() > MAX_FIELDS
            || values.keys().any(|key| {
                !self
                    .fields
                    .iter()
                    .any(|field| &field.key == key && !field.secret)
            })
            || changes.keys().any(|key| {
                !self
                    .fields
                    .iter()
                    .any(|field| &field.key == key && field.secret)
            })
        {
            return Err("Unknown or incorrectly classified plugin setting".into());
        }
        let mut next = current.clone();
        for field in &self.fields {
            // Preserve fields absent from this schema for rollback and future migration.
            next.values.remove(&field.key);
            if field.secret {
                next.secret_fields.insert(field.key.clone());
                if let Some(change) = changes.get(&field.key) {
                    match change {
                        Some(value) => {
                            next.secrets.insert(field.key.clone(), value.clone());
                        }
                        None => {
                            next.secrets.remove(&field.key);
                        }
                    }
                }
            } else if let Some(value) = values.get(&field.key).or(field.default.as_ref()) {
                next.values.insert(field.key.clone(), value.clone());
            }
        }
        self.validate_data(&next)?;
        if next.values != current.values
            || next.secrets != current.secrets
            || next.secret_fields != current.secret_fields
        {
            next.revision = uuid::Uuid::new_v4().to_string();
        }
        Ok(next)
    }

    fn validate_data(&self, data: &SettingsData) -> Result<(), String> {
        self.check_classification(data)?;
        for field in &self.fields {
            let secret;
            let value = if field.secret {
                secret = data
                    .secrets
                    .get(&field.key)
                    .map(|value| Value::String(value.clone()));
                secret.as_ref()
            } else {
                data.values.get(&field.key).or(field.default.as_ref())
            };
            match value {
                Some(value) => field.validate(value)?,
                None if field.required => {
                    return Err(format!("Required plugin setting is missing: {}", field.key))
                }
                None => {}
            }
        }
        Ok(())
    }

    pub(crate) fn configuration(&self, data: &SettingsData) -> Result<WorkerConfiguration, String> {
        self.validate_data(data)?;
        let configuration = WorkerConfiguration {
            revision: data.revision.clone(),
            values: serde_json::to_value(self.public_values(data))
                .map_err(|_| "Cannot encode plugin settings")?,
            secrets: self
                .fields
                .iter()
                .filter(|field| field.secret)
                .filter_map(|field| {
                    data.secrets
                        .get(&field.key)
                        .map(|value| (field.key.clone(), value.clone()))
                })
                .collect(),
        };
        configuration.validate()?;
        Ok(configuration)
    }
}

impl SettingField {
    fn validate(&self, value: &Value) -> Result<(), String> {
        let valid = match self.kind {
            SettingType::String => value.as_str().is_some_and(|value| {
                let length = value.chars().count();
                value.len() <= MAX_STRING
                    && self.min_length.is_none_or(|min| length >= min)
                    && self.max_length.is_none_or(|max| length <= max)
                    && self
                        .choices
                        .as_ref()
                        .is_none_or(|items| items.iter().any(|item| item == value))
            }),
            SettingType::Boolean => value.is_boolean(),
            SettingType::Number | SettingType::Integer => value.as_f64().is_some_and(|value| {
                value.is_finite()
                    && (self.kind != SettingType::Integer
                        || (value.fract() == 0.0 && value.abs() <= 9_007_199_254_740_991.0))
                    && self.minimum.is_none_or(|min| value >= min)
                    && self.maximum.is_none_or(|max| value <= max)
            }),
        };
        if valid {
            Ok(())
        } else {
            Err(format!("Invalid plugin setting: {}", self.key))
        }
    }
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
