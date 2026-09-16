//! Passive, versioned module settings. This module never loads or enables a plugin.

use serde::{de::Error as _, Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use std::{collections::BTreeMap, fmt, num::NonZeroU32};

pub(crate) type ModuleNamespaces = BTreeMap<String, ModuleNamespace>;

/// The stable envelope is understood by core; each module owns its payload schema.
/// Portable values must not contain credentials. All credentials belong in secrets.
#[derive(Clone, PartialEq, Serialize)]
pub(crate) struct ModuleNamespace {
    pub schema_version: NonZeroU32,
    pub values: Map<String, Value>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub secrets: BTreeMap<String, String>,
}

impl fmt::Debug for ModuleNamespace {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ModuleNamespace")
            .field("schema_version", &self.schema_version)
            .field("value_count", &self.values.len())
            .field("secret_count", &self.secrets.len())
            .finish()
    }
}

impl<'de> Deserialize<'de> for ModuleNamespace {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Unknown envelope fields cannot be classified as safe for export. Future
        // module payload fields belong inside values, not alongside secrets.
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Envelope {
            schema_version: NonZeroU32,
            values: Map<String, Value>,
            #[serde(default)]
            secrets: BTreeMap<String, String>,
        }
        let value = Value::deserialize(deserializer)
            .map_err(|_| D::Error::custom("Invalid module settings envelope"))?;
        let envelope: Envelope = serde_json::from_value(value)
            .map_err(|_| D::Error::custom("Invalid module settings envelope"))?;
        Ok(Self {
            schema_version: envelope.schema_version,
            values: envelope.values,
            secrets: envelope.secrets,
        })
    }
}

/// An older/core-only writer cannot remove namespaces or credentials by omission.
/// Retaining omitted credentials is safe only when their complete public binding
/// is unchanged. A caller explicitly providing every local credential can replace
/// its record; module-owned deletion is not exposed by core saves.
pub(crate) fn merge_for_save(
    mut incoming: ModuleNamespaces,
    current: &ModuleNamespaces,
) -> Result<ModuleNamespaces, String> {
    for (id, local) in current {
        let next = incoming.entry(id.clone()).or_insert_with(|| local.clone());
        let omitted_secret = local
            .secrets
            .keys()
            .any(|key| !next.secrets.contains_key(key));
        if omitted_secret
            && (next.schema_version != local.schema_version || next.values != local.values)
        {
            return Err(
                "Module settings conflict with locally stored credentials; retained credentials require unchanged settings"
                    .into(),
            );
        }
        for (key, secret) in &local.secrets {
            next.secrets
                .entry(key.clone())
                .or_insert_with(|| secret.clone());
        }
    }
    Ok(incoming)
}

pub(crate) fn portable(current: &ModuleNamespaces) -> ModuleNamespaces {
    current
        .iter()
        .map(|(id, namespace)| {
            let mut public = namespace.clone();
            public.secrets.clear();
            (id.clone(), public)
        })
        .collect()
}

/// Import cannot replace credentials or bind a retained credential to new values.
/// Core cannot interpret endpoint fields in an unknown module, so any payload or
/// schema change conflicts while that local namespace contains credentials.
pub(crate) fn restore_portable(
    incoming: ModuleNamespaces,
    current: &ModuleNamespaces,
) -> Result<ModuleNamespaces, String> {
    for namespace in incoming.values() {
        if !namespace.secrets.is_empty() {
            return Err("Portable module settings must not contain secrets".into());
        }
    }
    merge_for_save(incoming, current)
}

#[cfg(test)]
pub(crate) fn test_namespaces() -> ModuleNamespaces {
    serde_json::from_value(serde_json::json!({
        "example.future": {
            "schema_version": 407,
            "values": {
                "endpoint": "https://future.example.invalid",
                "layout": [{"kind": "future-panel", "options": {"visible": false}}],
                "unknown_payload": {"null": null, "array": [true, 12.5, "Label"]}
            },
            "secrets": {"access_token": "test-module-secret"}
        },
        "example.offline": {
            "schema_version": 2,
            "values": {"label": "Offline module"}
        }
    }))
    .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FullConfig;
    use serde_json::json;

    #[test]
    fn core_roundtrip_preserves_unknown_namespaces_versions_and_nested_payloads() {
        let original = test_namespaces();
        let mut config = FullConfig {
            modules: original.clone(),
            ..FullConfig::default()
        };
        config.show_console = Some(false);
        let decoded: FullConfig =
            serde_json::from_value(serde_json::to_value(config).unwrap()).unwrap();
        assert_eq!(decoded.modules, original);
        assert_eq!(decoded.show_console, Some(false));
    }

    #[test]
    fn public_config_response_then_core_edit_preserves_native_module_credentials() {
        let current = FullConfig {
            modules: test_namespaces(),
            ..FullConfig::default()
        };
        let mut response = current.clone();
        response.modules = portable(&response.modules);
        let encoded = serde_json::to_string(&response).unwrap();
        assert!(!encoded.contains("test-module-secret"));
        assert!(!encoded.contains("access_token"));
        let mut edited: FullConfig = serde_json::from_str(&encoded).unwrap();
        assert!(edited
            .modules
            .values()
            .all(|namespace| namespace.secrets.is_empty()));
        assert_eq!(
            edited.modules["example.future"].values,
            current.modules["example.future"].values
        );
        edited.show_console = Some(false);
        edited.modules = merge_for_save(edited.modules, &current.modules).unwrap();
        assert_eq!(edited.modules, current.modules);
        assert_eq!(edited.show_console, Some(false));
    }

    #[test]
    fn legacy_core_settings_do_not_add_or_activate_modules() {
        let value = serde_json::to_value(FullConfig::default()).unwrap();
        assert!(value.get("modules").is_none());
        assert!(serde_json::from_value::<FullConfig>(value)
            .unwrap()
            .modules
            .is_empty());
    }

    #[test]
    fn omitted_namespaces_survive_core_saves_and_explicit_public_updates_remain_possible() {
        let current = test_namespaces();
        let incoming: ModuleNamespaces = serde_json::from_value(json!({
            "example.offline": {"schema_version": 99, "values": {"new": true}}
        }))
        .unwrap();
        let incoming = merge_for_save(incoming, &current).unwrap();
        assert_eq!(incoming["example.future"], current["example.future"]);
        assert_eq!(incoming["example.offline"].schema_version.get(), 99);
        let omitted = merge_for_save(ModuleNamespaces::new(), &current).unwrap();
        assert_eq!(omitted, current);
    }

    #[test]
    fn core_save_retains_omitted_secret_keys_only_with_their_original_binding() {
        let mut current = test_namespaces();
        current
            .get_mut("example.future")
            .unwrap()
            .secrets
            .insert("second_token".into(), "other-test-secret".into());
        let mut incoming = portable(&current);
        incoming
            .get_mut("example.future")
            .unwrap()
            .secrets
            .insert("access_token".into(), "replacement-test-secret".into());
        let merged = merge_for_save(incoming.clone(), &current).unwrap();
        assert_eq!(
            merged["example.future"].secrets["access_token"],
            "replacement-test-secret"
        );
        assert_eq!(
            merged["example.future"].secrets["second_token"],
            "other-test-secret"
        );
        incoming.get_mut("example.future").unwrap().values["endpoint"] =
            json!("https://different.example.invalid");
        assert!(merge_for_save(incoming.clone(), &current).is_err());
        // Providing all credentials explicitly owns the complete new binding.
        incoming
            .get_mut("example.future")
            .unwrap()
            .secrets
            .insert("second_token".into(), "replacement-second-secret".into());
        let merged = merge_for_save(incoming.clone(), &current).unwrap();
        assert_eq!(merged, incoming);
    }

    #[test]
    fn core_save_rejects_changed_schema_with_omitted_credentials_without_mutating_local_data() {
        let current = test_namespaces();
        let mut incoming = portable(&current);
        incoming.get_mut("example.future").unwrap().schema_version = NonZeroU32::new(408).unwrap();
        assert!(merge_for_save(incoming, &current).is_err());
        assert_eq!(current, test_namespaces());
    }

    #[test]
    fn export_omits_all_module_secrets_and_matching_import_retains_local_credentials() {
        let current = test_namespaces();
        let public = portable(&current);
        let encoded = serde_json::to_string(&public).unwrap();
        assert!(!encoded.contains("test-module-secret"));
        assert!(!encoded.contains("secrets"));
        assert_eq!(
            public["example.future"].values,
            current["example.future"].values
        );
        assert_eq!(restore_portable(public, &current).unwrap(), current);
        assert_eq!(
            restore_portable(ModuleNamespaces::new(), &current).unwrap(),
            current
        );
    }

    #[test]
    fn imports_update_secret_free_namespaces_and_add_unknown_modules() {
        let current = test_namespaces();
        let incoming = serde_json::from_value(json!({
            "example.offline": {"schema_version": 44, "values": {"future": [1, 2]}},
            "example.new": {"schema_version": 500, "values": {"label": "New"}}
        }))
        .unwrap();
        let restored = restore_portable(incoming, &current).unwrap();
        assert_eq!(restored["example.offline"].schema_version.get(), 44);
        assert_eq!(restored["example.new"].values["label"], "New");
        assert_eq!(restored["example.future"], current["example.future"]);
    }

    #[test]
    fn credential_bearing_imports_are_rejected_even_without_a_local_namespace() {
        let error = restore_portable(test_namespaces(), &ModuleNamespaces::new()).unwrap_err();
        assert!(!error.contains("test-module-secret"));
        assert!(!error.contains("example.future"));
    }

    #[test]
    fn changed_endpoint_or_unknown_schema_cannot_retarget_local_credentials() {
        let current = test_namespaces();
        for change_schema in [false, true] {
            let mut incoming = portable(&current);
            let namespace = incoming.get_mut("example.future").unwrap();
            if change_schema {
                namespace.schema_version = NonZeroU32::new(408).unwrap();
            } else {
                namespace.values["endpoint"] = json!("https://changed.example.invalid");
            }
            let error = restore_portable(incoming, &current).unwrap_err();
            assert!(!error.contains("example.invalid"));
            assert!(!error.contains("test-module-secret"));
        }
        assert_eq!(current, test_namespaces());
    }

    #[test]
    fn malformed_or_unclassified_envelopes_fail_without_echoing_payloads() {
        for invalid in [
            json!({"schema_version": 0, "values": {}}),
            json!({"schema_version": 1.5, "values": {}}),
            json!({"schema_version": 4_294_967_296_u64, "values": {}}),
            json!({"schema_version": "sensitive-value", "values": {}}),
            json!({"schema_version": 1, "values": "sensitive-value"}),
            json!({"schema_version": 1, "values": {}, "secrets": {"key": ["sensitive-value"]}}),
            json!({"schema_version": 1, "values": {}, "sensitive-value": "unclassified"}),
        ] {
            let error = serde_json::from_value::<ModuleNamespace>(invalid).unwrap_err();
            assert_eq!(error.to_string(), "Invalid module settings envelope");
        }
    }

    #[test]
    fn debug_never_renders_module_values_or_credentials() {
        let current = test_namespaces();
        let rendered = format!("{:?}", current["example.future"]);
        for private in [
            "example.invalid",
            "access_token",
            "test-module-secret",
            "Label",
            "endpoint",
        ] {
            assert!(!rendered.contains(private));
        }
        assert!(rendered.contains("schema_version: 407"));
    }
}
