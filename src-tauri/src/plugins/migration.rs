//! Native-only, one-time settings handover. No legacy transport is started here.

use super::application::SettingsSeedProvider;
use super::legacy_migration::{merge_seed, plan_plugin};
use super::protocol::PluginManifest;
use super::settings_store::SettingsData;
use crate::FullConfig;
use serde_json::Value;
use std::sync::Arc;
use tauri::Manager;

pub(crate) fn provider(app: tauri::AppHandle) -> SettingsSeedProvider {
    Arc::new(move |manifest, current| {
        if current.legacy_migration_version >= 1 {
            return Ok(current.clone());
        }
        // Config I/O and daemon snapshots never run while the host authority is
        // locked. The caller separately commits the validated result in its epoch.
        let config = crate::load_config(&app)
            .map_err(|_| "Plugin migration configuration is unavailable")?;
        let daemon = daemon_snapshot(&app);
        let mut next = settings_seed(manifest, current, &config, daemon.as_ref())?;
        if next != *current {
            next.revision = uuid::Uuid::new_v4().to_string();
        }
        Ok(next)
    })
}

fn settings_seed(
    manifest: &PluginManifest,
    current: &SettingsData,
    config: &FullConfig,
    daemon: Option<&Value>,
) -> Result<SettingsData, String> {
    if current.legacy_migration_version >= 1 {
        return Ok(current.clone());
    }
    let legacy = !config.modules.contains_key(&manifest.plugin_id);
    let supports = |field: &str| {
        manifest
            .config_schema
            .get("properties")
            .and_then(Value::as_object)
            .is_some_and(|properties| properties.contains_key(field))
    };
    // An app-only upgrade must keep an older pinned HA worker running with its
    // existing settings. Its full handover waits for a package that can express
    // the layout, rather than requiring daemon defaults it cannot consume yet.
    if legacy
        && manifest.plugin_id == "inverter-desktop.home-assistant"
        && [
            "dashboard_layout",
            "watch_entities",
            "discovery_domains",
            "notify_home",
        ]
        .iter()
        .any(|field| !supports(field))
    {
        return Ok(current.clone());
    }
    let Some(seed) = plan_plugin(config, daemon, &manifest.plugin_id)
        .map_err(|error| error.code().to_string())?
    else {
        return Ok(current.clone());
    };
    // Field availability is the only compatibility exemption. Invalid values,
    // endpoint conflicts, and explicit namespace restores still fail closed in
    // the planner or the caller's verified schema validation before any commit.
    if seed
        .values
        .keys()
        .chain(seed.secrets.keys())
        .any(|key| !supports(key))
    {
        return if legacy {
            Ok(current.clone())
        } else {
            Err("module_restore_settings_unsupported".into())
        };
    }
    merge_seed(current, &seed).map_err(|error| error.code().to_string())
}

fn selected_snapshot(
    gateway: Option<crate::mqtt::InverterState>,
    mqtt: Option<crate::mqtt::InverterState>,
) -> Option<serde_json::Value> {
    // Match the dashboard: an owned gateway takes precedence, even while its
    // first daemon frame is pending. Never seed from a stale secondary client.
    let snapshot = gateway.or(mqtt)?;
    snapshot.ui_config.as_ref()?;
    serde_json::to_value(snapshot).ok()
}

fn daemon_snapshot(app: &tauri::AppHandle) -> Option<serde_json::Value> {
    let gateway = app.try_state::<crate::GatewayState>().and_then(|state| {
        state
            .0
            .lock()
            .ok()
            .and_then(|client| client.as_ref().map(|client| client.get_state()))
    });
    let mqtt = app.try_state::<crate::MqttState>().and_then(|state| {
        state
            .0
            .lock()
            .ok()
            .and_then(|client| client.as_ref().map(|client| client.get_state()))
    });
    selected_snapshot(gateway, mqtt)
}

pub(crate) fn daemon_ready(app: &tauri::AppHandle) -> bool {
    daemon_snapshot(app).is_some()
}

#[cfg(test)]
mod tests {
    use super::super::settings::SettingsSchema;
    use super::*;
    use serde_json::json;

    fn manifest(plugin: &str) -> PluginManifest {
        serde_json::from_str(match plugin {
            "ha" => include_str!("../../../scripts/plugins/home-assistant-manifest.json"),
            "frigate" => include_str!("../../../scripts/plugins/frigate-manifest.json"),
            _ => unreachable!(),
        })
        .unwrap()
    }

    fn configured_ha() -> FullConfig {
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

    #[test]
    fn older_ha_pin_keeps_current_settings_until_a_capable_package_is_installed() {
        let mut old = manifest("ha");
        for field in ["dashboard_layout", "discovery_domains", "notify_home"] {
            old.config_schema["properties"]
                .as_object_mut()
                .unwrap()
                .remove(field);
        }
        let mut current = SettingsData::default();
        current
            .values
            .insert("ha_base_url".into(), json!("http://ha.test:8123"));
        current
            .values
            .insert("watch_entities".into(), json!("sensor.existing"));
        current
            .secrets
            .insert("ha_token".into(), "fixture-token".into());
        current.secret_fields.insert("ha_token".into());
        let mut config = configured_ha();
        config.show_home_section = Some(true);
        // The old package cannot express the handover and must not start waiting
        // for daemon defaults during an otherwise compatible app-only upgrade.
        let preserved = settings_seed(&old, &current, &config, None).unwrap();
        assert!(preserved == current);
        assert_eq!(preserved.legacy_migration_version, 0);
        assert!(SettingsSchema::compile(&old)
            .unwrap()
            .configuration(&preserved)
            .is_ok());
        config.show_home_section = Some(false);
        config.ha_consumption_clamps = Some(vec!["sensor.clamp".into()]);
        let next = settings_seed(&manifest("ha"), &preserved, &config, None).unwrap();
        assert_eq!(next.legacy_migration_version, 1);
        assert_eq!(
            next.values["watch_entities"],
            "sensor.existing,sensor.clamp"
        );
        assert!(SettingsSchema::compile(&manifest("ha"))
            .unwrap()
            .configuration(&next)
            .is_ok());
    }

    #[test]
    fn explicit_namespace_restore_is_never_treated_as_legacy_compatibility() {
        let mut old = manifest("ha");
        old.config_schema["properties"]
            .as_object_mut()
            .unwrap()
            .remove("dashboard_layout");
        let mut config = FullConfig::default();
        config.modules.insert(old.plugin_id.clone(), serde_json::from_value(json!({
            "schema_version": 1,
            "values": {"dashboard_layout": "{\"version\":1,\"controls\":[],\"sections\":{},\"appliances\":{}}"}
        })).unwrap());
        // Existing encrypted records may retain unknown fields for rollback.
        // A fresh imported namespace instead rejects them before a marker or
        // record can be committed by PackageApplication.
        assert!(matches!(
            settings_seed(&old, &SettingsData::default(), &config, None),
            Err(error) if error == "module_restore_settings_unsupported"
        ));
    }

    #[test]
    fn compatible_camera_schema_migrates_and_new_seed_value_errors_are_not_hidden() {
        let mut old = manifest("frigate");
        old.version = "0.2.0".into();
        old.host_api = "^1.3".into();
        old.http_video.as_mut().unwrap().live_preview = None;
        let config = FullConfig {
            camera_topic: Some("frigate/events".into()),
            mqtt_ha_host: Some("broker.test".into()),
            ..FullConfig::default()
        };
        let next = settings_seed(&old, &SettingsData::default(), &config, None).unwrap();
        assert_eq!(next.legacy_migration_version, 1);
        assert_eq!(next.values["mqtt_topic"], "frigate/events");
        assert!(SettingsSchema::compile(&old)
            .unwrap()
            .configuration(&next)
            .is_ok());
        let mut current = SettingsData::default();
        current.values.insert("watch_entities".into(), json!(42));
        assert!(settings_seed(&manifest("ha"), &current, &configured_ha(), None).is_err());
        let mut current = SettingsData::default();
        current.values.insert("notify_home".into(), json!(42));
        let next = settings_seed(&manifest("ha"), &current, &configured_ha(), None).unwrap();
        assert!(SettingsSchema::compile(&manifest("ha"))
            .unwrap()
            .seed_configuration(&next)
            .is_err());
    }

    #[test]
    fn gateway_only_daemon_defaults_are_ready_without_core_mqtt() {
        let ready = crate::mqtt::InverterState {
            ui_config: Some(crate::mqtt::UiConfig::default()),
            ..Default::default()
        };
        assert!(selected_snapshot(Some(ready.clone()), None).is_some());
        assert!(selected_snapshot(None, Some(ready.clone())).is_some());
        assert!(selected_snapshot(Some(Default::default()), Some(ready)).is_none());
        assert!(selected_snapshot(None, None).is_none());
    }
}
