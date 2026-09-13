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
    let mut value = serde_json::to_value(config).map_err(|e| e.to_string())?;
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
            &["mqtt_host", "mqtt_port"][..],
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
    fn invalid_backup_does_not_become_default_settings() {
        assert!(restore("[]", &FullConfig::default()).is_err());
        assert!(restore("{}", &FullConfig::default()).is_err());
    }
}
