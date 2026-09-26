//! Portable declarations only. Desktop code owns validation and installation;
//! mobile keeps this data intact without loading any plugin implementation.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct DesktopPluginConfig {
    pub plugin_id: String,
    pub version: String,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    pub artifacts: BTreeMap<String, DesktopPluginArtifact>,
    /// Preserve future metadata when a config passes through an older client.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct DesktopPluginArtifact {
    pub url: String,
    pub sha256: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

fn enabled_by_default() -> bool {
    true
}

/// Ordinary core editors carry passive declarations and must not overwrite a
/// newer plugin-manager decision. Explicit declaration edits use a full baseline
/// comparison under the caller's config-update lock before any write occurs.
pub(crate) fn reconcile_for_save(
    submitted: &mut Vec<DesktopPluginConfig>,
    current: &[DesktopPluginConfig],
    expected: Option<&[DesktopPluginConfig]>,
) -> Result<(), String> {
    match expected {
        None => *submitted = current.to_vec(),
        Some(baseline) if baseline != current => {
            return Err(
                "Plugin configuration changed; reload it before editing declarations".into(),
            );
        }
        Some(_) => {}
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn test_declarations() -> Vec<DesktopPluginConfig> {
    serde_json::from_value(serde_json::json!([
        {
            "plugin_id": "example.monitor",
            "version": "1.2.3",
            "enabled": false,
            "artifacts": {
                "aarch64-apple-darwin": {
                    "url": "https://packages.example.invalid/monitor-1.2.3-macos.idplugin",
                    "sha256": "a".repeat(64),
                    "future_artifact": {"retained": true}
                },
                "x86_64-pc-windows-msvc": {
                    "url": "https://packages.example.invalid/monitor-1.2.3-windows.idplugin",
                    "sha256": "b".repeat(64)
                }
            },
            "future_metadata": {"labels": ["home", "status"]}
        },
        {
            "plugin_id": "example.weather",
            "version": "2.0.0",
            "enabled": true,
            "artifacts": {
                "aarch64-apple-darwin": {
                    "url": "https://packages.example.invalid/weather-2.0.0.idplugin",
                    "sha256": "c".repeat(64)
                }
            }
        }
    ]))
    .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FullConfig;

    #[test]
    fn stale_core_save_does_not_restore_removed_or_disabled_declarations() {
        let mut submitted = test_declarations();
        let mut current = submitted.clone();
        current.remove(0);
        current[0].enabled = false;
        reconcile_for_save(&mut submitted, &current, None).unwrap();
        assert_eq!(submitted, current);

        reconcile_for_save(&mut submitted, &[], None).unwrap();
        assert!(submitted.is_empty());
    }

    #[test]
    fn core_save_preserves_declarations_added_after_its_snapshot() {
        let mut submitted = Vec::new();
        let current = test_declarations();
        assert!(reconcile_for_save(&mut submitted, &current, Some(&[])).is_err());
        assert!(submitted.is_empty());
        reconcile_for_save(&mut submitted, &current, None).unwrap();
        assert_eq!(submitted, current);
    }

    #[test]
    fn explicit_matching_baselines_allow_adding_disabling_and_removing_declarations() {
        let initial = test_declarations();
        let mut submitted = initial.clone();
        reconcile_for_save(&mut submitted, &[], Some(&[])).unwrap();
        assert_eq!(submitted, initial);

        submitted[1].enabled = false;
        let disabled = submitted.clone();
        reconcile_for_save(&mut submitted, &initial, Some(&initial)).unwrap();
        assert_eq!(submitted, disabled);

        submitted.clear();
        reconcile_for_save(&mut submitted, &disabled, Some(&disabled)).unwrap();
        assert!(submitted.is_empty());
    }

    #[test]
    fn stale_explicit_baselines_reject_changes_to_pins_state_or_future_metadata() {
        let baseline = test_declarations();
        let mut variants = vec![Vec::new()];
        for field in ["enabled", "version", "artifact", "extra"] {
            let mut current = baseline.clone();
            match field {
                "enabled" => current[0].enabled = !current[0].enabled,
                "version" => current[0].version = "9.0.0".into(),
                "artifact" => {
                    current[0].artifacts.values_mut().next().unwrap().sha256 = "d".repeat(64);
                }
                _ => {
                    current[0]
                        .extra
                        .insert("new_metadata".into(), Value::Bool(true));
                }
            }
            variants.push(current);
        }
        for current in variants {
            let mut submitted = baseline.clone();
            submitted[0].version = "3.0.0".into();
            let before = submitted.clone();
            assert!(reconcile_for_save(&mut submitted, &current, Some(&baseline)).is_err());
            assert_eq!(submitted, before);
        }
    }

    #[test]
    fn core_config_preserves_disabled_plugins_platforms_and_future_metadata() {
        let mut config = FullConfig {
            desktop_plugins: test_declarations(),
            ..FullConfig::default()
        };
        let original = serde_json::to_value(&config).unwrap()["desktop_plugins"].clone();
        config.show_batteries = Some(false);
        let decoded: FullConfig =
            serde_json::from_value(serde_json::to_value(config).unwrap()).unwrap();
        assert_eq!(decoded.show_batteries, Some(false));
        assert_eq!(
            serde_json::to_value(&decoded).unwrap()["desktop_plugins"],
            original
        );
        assert!(!decoded.desktop_plugins[0].enabled);
        assert_eq!(decoded.desktop_plugins[0].artifacts.len(), 2);
    }

    #[test]
    fn existing_config_without_declarations_stays_empty_and_omits_the_field() {
        let original = serde_json::to_value(FullConfig::default()).unwrap();
        assert!(original.get("desktop_plugins").is_none());
        let decoded: FullConfig = serde_json::from_value(original).unwrap();
        assert!(decoded.desktop_plugins.is_empty());
        assert!(serde_json::to_value(decoded)
            .unwrap()
            .get("desktop_plugins")
            .is_none());
    }

    #[test]
    fn omitted_enabled_defaults_to_true_without_losing_future_fields() {
        let mut encoded = serde_json::to_value(test_declarations().remove(0)).unwrap();
        encoded.as_object_mut().unwrap().remove("enabled");
        let decoded: DesktopPluginConfig = serde_json::from_value(encoded.clone()).unwrap();
        assert!(decoded.enabled);
        encoded["enabled"] = Value::Bool(true);
        assert_eq!(serde_json::to_value(decoded).unwrap(), encoded);
    }
}
