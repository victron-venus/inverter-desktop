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
    fn core_config_preserves_disabled_plugins_platforms_and_future_metadata() {
        let mut config = FullConfig {
            desktop_plugins: test_declarations(),
            ..FullConfig::default()
        };
        let original = serde_json::to_value(&config).unwrap()["desktop_plugins"].clone();
        config.show_console = Some(false);
        let decoded: FullConfig =
            serde_json::from_value(serde_json::to_value(config).unwrap()).unwrap();
        assert_eq!(decoded.show_console, Some(false));
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
