//! Portable settings exports intentionally omit credentials and local auth policy.
use super::FullConfig;
use serde_json::Value;

const PRIVATE_FIELDS: &[&str] = &[
    "mqtt_login",
    "mqtt_password",
    "mqtt_ha_login",
    "mqtt_ha_password",
    "ha_longlived_token",
    "gateway_access_client_id",
    "gateway_access_client_secret",
    "gateway_api_token",
    "auth_enabled",
    "auth_username",
    "auth_password",
    "auth_biometric",
];
// URLs/templates may contain embedded credentials or query tokens.
const PRIVATE_URL_FIELDS: &[&str] = &[
    "ha_url",
    "gateway_url",
    "frigate_base_url",
    "ring_snapshot_url_template",
];

fn sensitive_url(value: &str) -> bool {
    value.contains('@') || value.contains('?') || value.contains('#')
}

pub(super) fn redacted(config: &FullConfig) -> Result<Value, String> {
    let mut portable = config.clone();
    portable.modules = crate::module_config::portable(&config.modules);
    let mut value = serde_json::to_value(portable).map_err(|e| e.to_string())?;
    let object = value.as_object_mut().ok_or("Invalid config")?;
    for key in PRIVATE_FIELDS {
        object.remove(*key);
    }
    for key in PRIVATE_URL_FIELDS {
        if object
            .get(*key)
            .and_then(Value::as_str)
            .is_some_and(sensitive_url)
        {
            object.remove(*key);
        }
    }
    object.insert(
        "backup_format".into(),
        Value::String("settings-without-secrets-v1".into()),
    );
    Ok(value)
}

pub(super) fn restore(content: &str, current: &FullConfig) -> Result<FullConfig, String> {
    let mut incoming: Value =
        serde_json::from_str(content).map_err(|e| format!("Invalid backup: {e}"))?;
    let object = incoming.as_object_mut().ok_or("Backup must be an object")?;
    let existing = serde_json::to_value(current).map_err(|e| e.to_string())?;
    let incoming_modules = match object.get("modules") {
        Some(value) => serde_json::from_value(value.clone())
            .map_err(|_| "Invalid portable module settings".to_owned())?,
        None => crate::module_config::ModuleNamespaces::new(),
    };
    let modules = crate::module_config::restore_portable(incoming_modules, &current.modules)?;
    if !modules.is_empty() {
        object.insert(
            "modules".into(),
            serde_json::to_value(modules).map_err(|_| "Invalid portable module settings")?,
        );
    }
    // An imported file cannot change local authentication or replace credentials.
    // This rule also applies to legacy exports which included plaintext secrets.
    for key in PRIVATE_FIELDS {
        object.insert((*key).into(), existing[*key].clone());
    }
    for key in PRIVATE_URL_FIELDS {
        if !object.contains_key(*key) {
            object.insert((*key).into(), existing[*key].clone());
        }
    }
    // Never pair credentials from this installation with a newly imported server.
    for (endpoints, credentials) in [
        (
            &["mqtt_host", "mqtt_port", "mqtt_tls"][..],
            &["mqtt_login", "mqtt_password"][..],
        ),
        (
            &["mqtt_ha_host", "mqtt_ha_port"][..],
            &["mqtt_ha_login", "mqtt_ha_password"][..],
        ),
        (&["ha_url", "ha_port"][..], &["ha_longlived_token"][..]),
        (
            &["gateway_url"][..],
            &[
                "gateway_access_client_id",
                "gateway_access_client_secret",
                "gateway_api_token",
            ][..],
        ),
    ] {
        if endpoints
            .iter()
            .any(|key| object.get(*key) != existing.get(*key))
        {
            for key in credentials {
                object.insert((*key).into(), Value::Null);
            }
        }
    }
    object.remove("backup_format");
    serde_json::from_value(incoming).map_err(|e| format!("Invalid backup settings: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn backup_omits_secrets_and_restore_preserves_local_auth() {
        let current = FullConfig {
            mqtt_password: Some("test-mqtt".into()),
            ha_longlived_token: Some("test-ha".into()),
            auth_enabled: Some(true),
            auth_password: Some("test-auth".into()),
            ring_snapshot_url_template: Some("https://camera.invalid/?token=secret".into()),
            ..Default::default()
        };
        let backup = redacted(&current).unwrap();
        let text = backup.to_string();
        for secret in ["test-mqtt", "test-ha", "test-auth", "token=secret"] {
            assert!(!text.contains(secret));
        }
        let same_server = restore(&backup.to_string(), &current).unwrap();
        assert_eq!(same_server.mqtt_password, current.mqtt_password);
        let mut restored = backup;
        restored["mqtt_host"] = Value::String("new-host".into());
        restored["auth_enabled"] = Value::Bool(false);
        restored["auth_password"] = Value::String("imported-password".into());
        let config = restore(&restored.to_string(), &current).unwrap();
        assert_eq!(config.mqtt_host, "new-host");
        assert_eq!(config.auth_enabled, Some(true));
        assert_eq!(config.auth_password, current.auth_password);
        assert_eq!(config.mqtt_password, None);
        assert_eq!(
            config.ring_snapshot_url_template,
            current.ring_snapshot_url_template
        );
    }
    #[test]
    fn importing_a_tls_downgrade_clears_mqtt_credentials() {
        let current = FullConfig {
            mqtt_tls: true,
            mqtt_login: Some("test-user".into()),
            mqtt_password: Some("test-password".into()),
            ..FullConfig::default()
        };
        let mut backup = redacted(&current).unwrap();
        backup["mqtt_tls"] = Value::Bool(false);
        let restored = restore(&backup.to_string(), &current).unwrap();
        assert!(!restored.mqtt_tls);
        assert!(restored.mqtt_login.is_none());
        assert!(restored.mqtt_password.is_none());
    }

    #[test]
    fn invalid_backup_does_not_become_default_settings() {
        assert!(restore("[]", &FullConfig::default()).is_err());
        assert!(restore("{}", &FullConfig::default()).is_err());
    }

    #[test]
    fn module_backup_is_portable_and_matching_import_preserves_local_secrets() {
        let current = FullConfig {
            modules: crate::module_config::test_namespaces(),
            ..FullConfig::default()
        };
        let backup = redacted(&current).unwrap();
        let text = backup.to_string();
        assert!(!text.contains("test-module-secret"));
        assert!(backup["modules"]["example.future"].get("secrets").is_none());
        assert_eq!(backup["modules"]["example.future"]["schema_version"], 407);
        let same_installation = restore(&text, &current).unwrap();
        assert_eq!(same_installation.modules, current.modules);
        let other_installation = restore(&text, &FullConfig::default()).unwrap();
        assert!(other_installation.modules["example.future"]
            .secrets
            .is_empty());
        assert_eq!(
            other_installation.modules["example.future"].values,
            current.modules["example.future"].values
        );
    }

    #[test]
    fn older_or_partial_backups_cannot_remove_local_module_namespaces() {
        let current = FullConfig {
            modules: crate::module_config::test_namespaces(),
            ..FullConfig::default()
        };
        let legacy = redacted(&FullConfig::default()).unwrap();
        assert_eq!(
            restore(&legacy.to_string(), &current).unwrap().modules,
            current.modules
        );
        let mut partial = legacy;
        partial["modules"] = serde_json::json!({
            "example.new": {"schema_version": 600, "values": {"future": [null, true, 3]}}
        });
        let restored = restore(&partial.to_string(), &current).unwrap();
        assert_eq!(restored.modules.len(), 3);
        for (id, local) in &current.modules {
            assert_eq!(&restored.modules[id], local);
        }
    }

    #[test]
    fn module_conflict_rejects_the_entire_backup_without_changing_current_config() {
        let current = FullConfig {
            modules: crate::module_config::test_namespaces(),
            ..FullConfig::default()
        };
        let original = serde_json::to_value(&current).unwrap();
        let mut backup = redacted(&current).unwrap();
        backup["mqtt_host"] = Value::String("changed-cerbo".into());
        backup["modules"]["example.future"]["values"]["endpoint"] =
            Value::String("https://another-server.example.invalid".into());
        assert!(restore(&backup.to_string(), &current).is_err());
        assert_eq!(serde_json::to_value(&current).unwrap(), original);
    }

    #[test]
    fn imports_reject_module_credentials_and_unclassified_fields_without_echoing_them() {
        for namespace in [
            serde_json::json!({"schema_version": 1, "values": {}, "secrets": {"token": "private-test-value"}}),
            serde_json::json!({"schema_version": 1, "values": {}, "future_credentials": "private-test-value"}),
            serde_json::json!({"schema_version": "private-test-value", "values": {}}),
            Value::Null,
        ] {
            let mut backup = redacted(&FullConfig::default()).unwrap();
            backup["modules"] = serde_json::json!({"example.unknown": namespace});
            let error = restore(&backup.to_string(), &FullConfig::default()).unwrap_err();
            assert!(!error.contains("private-test-value"));
            assert!(!error.contains("future_credentials"));
        }
    }

    #[test]
    fn portable_backup_restores_package_declarations_without_credentials() {
        let source = FullConfig {
            desktop_plugins: crate::plugin_config::test_declarations(),
            mqtt_password: Some("source-mqtt-secret".into()),
            ha_longlived_token: Some("source-ha-secret".into()),
            ..FullConfig::default()
        };
        let backup = redacted(&source).unwrap();
        let text = backup.to_string();
        assert!(!text.contains("source-mqtt-secret"));
        assert!(!text.contains("source-ha-secret"));
        assert_eq!(
            backup["desktop_plugins"],
            serde_json::to_value(&source.desktop_plugins).unwrap()
        );
        let restored = restore(&text, &FullConfig::default()).unwrap();
        assert_eq!(restored.desktop_plugins, source.desktop_plugins);
        assert!(restored.mqtt_password.is_none());
        assert!(restored.ha_longlived_token.is_none());
    }

    #[test]
    fn imported_declarations_replace_existing_pins_and_preserve_disabled_state() {
        let mut current = FullConfig {
            desktop_plugins: crate::plugin_config::test_declarations(),
            ..FullConfig::default()
        };
        let imported = redacted(&current).unwrap();
        current.desktop_plugins[0].enabled = true;
        current.desktop_plugins[0].version = "9.0.0".into();
        let restored = restore(&imported.to_string(), &current).unwrap();
        assert!(!restored.desktop_plugins[0].enabled);
        assert_eq!(restored.desktop_plugins[0].version, "1.2.3");
        let legacy = redacted(&FullConfig::default()).unwrap();
        assert!(restore(&legacy.to_string(), &current)
            .unwrap()
            .desktop_plugins
            .is_empty());
    }
}
