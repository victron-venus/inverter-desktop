use super::*;
use serde_json::json;

fn manifest(schema: Value) -> PluginManifest {
    PluginManifest {
        schema_version: 1,
        plugin_id: "org.example.settings".into(),
        version: "1.0.0".into(),
        host_api: "^1.1".into(),
        target: "aarch64-apple-darwin".into(),
        entrypoint: "worker".into(),
        config_schema: schema,
        permissions: vec![PluginPermission::PluginConfiguration],
        inventory: vec![],
        signature: None,
    }
}

fn configured_manifest() -> PluginManifest {
    manifest(
        json!({"type":"object","additionalProperties":false,"properties":{
        "endpoint":{"type":"string","minLength":1,"maxLength":100,"default":"https://example.invalid"},
        "token":{"type":"string","writeOnly":true,"minLength":1},
        "port":{"type":"integer","minimum":1,"maximum":65535,"default":443},
        "enabled":{"type":"boolean","default":true},
        "mode":{"type":"string","enum":["local","remote"],"default":"remote"}
    },"required":["endpoint","token"]}),
    )
}

fn saved(schema: &SettingsSchema) -> SettingsData {
    schema
        .merge(
            "archive",
            &SettingsData::default(),
            "archive:0",
            BTreeMap::new(),
            BTreeMap::from([("token".into(), Some("private-token-value".into()))]),
        )
        .unwrap()
}

#[test]
fn missing_settings_show_defaults_without_revealing_secrets_or_requiring_setup() {
    let metadata = configured_manifest();
    let schema = SettingsSchema::compile(&metadata).unwrap();
    let data = SettingsData::default();
    let view = serde_json::to_value(schema.view(&metadata, "archive", &data).unwrap()).unwrap();
    assert_eq!(view["values"]["endpoint"], "https://example.invalid");
    assert_eq!(view["values"]["port"], 443);
    assert_eq!(view["secret_present"]["token"], false);
    assert_eq!(view["revision"], "archive:0");
    assert!(view["values"].get("token").is_none());
    assert!(schema.configuration(&data).is_err());
}

#[test]
fn secret_replacement_keep_clear_and_required_validation() {
    let metadata = configured_manifest();
    let schema = SettingsSchema::compile(&metadata).unwrap();
    let initial = saved(&schema);
    let view =
        serde_json::to_string(&schema.view(&metadata, "archive", &initial).unwrap()).unwrap();
    assert!(!view.contains("private-token-value"));
    let config = schema.configuration(&initial).unwrap();
    assert_eq!(config.secrets["token"], "private-token-value");
    assert!(config.values.get("token").is_none());
    assert!(!format!("{config:?}").contains("private-token-value"));
    let unchanged = schema
        .merge(
            "archive",
            &initial,
            &format!("archive:{}", initial.revision),
            BTreeMap::new(),
            BTreeMap::new(),
        )
        .unwrap();
    assert_eq!(unchanged.revision, initial.revision);
    assert_eq!(unchanged.secrets, initial.secrets);
    assert!(schema
        .merge(
            "archive",
            &initial,
            &format!("archive:{}", initial.revision),
            BTreeMap::new(),
            BTreeMap::from([("token".into(), None)])
        )
        .is_err());
    let optional = SettingsSchema::compile(&manifest(
        json!({"type":"object","properties":{"token":{"type":"string","writeOnly":true}}}),
    ))
    .unwrap();
    let cleared = optional
        .merge(
            "archive",
            &initial,
            &format!("archive:{}", initial.revision),
            BTreeMap::new(),
            BTreeMap::from([("token".into(), None)]),
        )
        .unwrap();
    assert!(cleared.secrets.is_empty());
    assert!(cleared.secret_fields.contains("token"));
    assert_ne!(cleared.revision, initial.revision);
}

#[test]
fn optimistic_revision_binds_settings_and_installed_archive() {
    let schema = SettingsSchema::compile(&configured_manifest()).unwrap();
    let current = saved(&schema);
    for revision in [
        "archive:0".to_owned(),
        format!("other:{}", current.revision),
    ] {
        assert!(schema
            .merge(
                "archive",
                &current,
                &revision,
                BTreeMap::new(),
                BTreeMap::new()
            )
            .is_err());
    }
}

#[test]
fn secret_classification_survives_clear_and_schema_downgrade() {
    let schema = SettingsSchema::compile(&configured_manifest()).unwrap();
    let mut data = saved(&schema);
    data.secrets.clear();
    let public_manifest =
        manifest(json!({"type":"object","properties":{"token":{"type":"string"}}}));
    let public = SettingsSchema::compile(&public_manifest).unwrap();
    assert!(public.view(&public_manifest, "archive", &data).is_err());
    assert!(public.configuration(&data).is_err());
    assert!(public
        .merge(
            "archive",
            &data,
            &format!("archive:{}", data.revision),
            BTreeMap::new(),
            BTreeMap::new()
        )
        .is_err());
}

#[test]
fn unknown_stored_fields_survive_but_are_never_sent_to_worker_or_ui() {
    let metadata = configured_manifest();
    let schema = SettingsSchema::compile(&metadata).unwrap();
    let mut current = saved(&schema);
    current
        .values
        .insert("future-setting".into(), json!({"nested":42}));
    current
        .secrets
        .insert("future-token".into(), "future-secret".into());
    current.secret_fields.insert("future-token".into());
    let next = schema
        .merge(
            "archive",
            &current,
            &format!("archive:{}", current.revision),
            BTreeMap::new(),
            BTreeMap::new(),
        )
        .unwrap();
    assert_eq!(
        next.values["future-setting"],
        current.values["future-setting"]
    );
    assert_eq!(next.secrets["future-token"], "future-secret");
    assert!(schema
        .configuration(&next)
        .unwrap()
        .values
        .get("future-setting")
        .is_none());
    assert!(
        !serde_json::to_string(&schema.view(&metadata, "archive", &next).unwrap())
            .unwrap()
            .contains("future-")
    );
}

#[test]
fn rejects_cross_namespace_writes_and_never_quotes_secret_values_in_errors() {
    let schema = SettingsSchema::compile(&configured_manifest()).unwrap();
    let current = saved(&schema);
    let revision = format!("archive:{}", current.revision);
    for key in ["token", "unknown", "__proto__"] {
        assert!(schema
            .merge(
                "archive",
                &current,
                &revision,
                BTreeMap::from([(key.into(), json!("private"))]),
                BTreeMap::new()
            )
            .is_err());
    }
    for key in ["endpoint", "unknown"] {
        assert!(schema
            .merge(
                "archive",
                &current,
                &revision,
                BTreeMap::new(),
                BTreeMap::from([(key.into(), Some("private".into()))])
            )
            .is_err());
    }
    let secret = "unique-private".repeat(400);
    let error = schema
        .merge(
            "archive",
            &current,
            &revision,
            BTreeMap::new(),
            BTreeMap::from([("token".into(), Some(secret.clone()))]),
        )
        .err()
        .unwrap();
    assert!(!error.contains("unique-private"));
}

#[test]
fn primitive_types_enums_and_numeric_and_unicode_bounds_are_enforced() {
    let schema = SettingsSchema::compile(&configured_manifest()).unwrap();
    let current = saved(&schema);
    let revision = format!("archive:{}", current.revision);
    for (key, value) in [
        ("port", json!(1.5)),
        ("port", json!(0)),
        ("port", json!(65536)),
        ("enabled", json!("true")),
        ("mode", json!("unsupported")),
        ("endpoint", json!("")),
        ("endpoint", json!("x".repeat(101))),
    ] {
        assert!(schema
            .merge(
                "archive",
                &current,
                &revision,
                BTreeMap::from([(key.into(), value)]),
                BTreeMap::new()
            )
            .is_err());
    }
    let unicode = SettingsSchema::compile(&manifest(json!({"type":"object","properties":{"text":{"type":"string","minLength":2,"maxLength":2}}}))).unwrap();
    assert!(unicode
        .merge(
            "archive",
            &SettingsData::default(),
            "archive:0",
            BTreeMap::from([("text".into(), json!("ёж"))]),
            BTreeMap::new()
        )
        .is_ok());
}

#[test]
fn unsupported_or_ambiguous_schemas_fail_closed() {
    let fields = [
        json!({"type":"string","writeOnly":true,"default":null}),
        json!({"type":"string","default":null}),
        json!({"type":"string","enum":["a"],"minLength":2}),
        json!({"type":"array"}),
        json!({"type":"object"}),
        json!({"type":"string","pattern":".*"}),
        json!({"type":"string","$ref":"remote"}),
        json!({"type":"string","writeOnly":true,"default":"never-echo-this"}),
        json!({"type":"string","writeOnly":true,"enum":["never-echo-this"]}),
        json!({"type":"boolean","writeOnly":true}),
        json!({"type":"boolean","minimum":1}),
        json!({"type":"integer","maxLength":10}),
        json!({"type":"string","minLength":9,"maxLength":2}),
        json!({"type":"integer","minimum":9,"maximum":2}),
        json!({"type":"string","enum":["a","a"]}),
        json!({"type":"integer","default":"bad"}),
        json!({"type":"string","maxLength":4097}),
    ];
    for field in fields {
        let result = SettingsSchema::compile(&manifest(
            json!({"type":"object","properties":{"test":field}}),
        ));
        assert!(result.is_err());
        assert!(!result.err().unwrap().contains("never-echo-this"));
    }
    for schema in [
        json!({"type":"object","additionalProperties":true}),
        json!({"type":"object","required":["missing"]}),
        json!({"type":"object","allOf":[]}),
        json!({"type":"object","properties":{"__proto__":{"type":"string"}}}),
        json!({"type":"object","properties":{"_token":{"type":"string","writeOnly":true}}}),
    ] {
        assert!(SettingsSchema::compile(&manifest(schema)).is_err());
    }
    let mut metadata = configured_manifest();
    metadata.permissions.clear();
    assert!(SettingsSchema::compile(&metadata).is_err());
}

#[test]
fn integer_values_outside_javascript_safe_range_are_rejected() {
    let schema = SettingsSchema::compile(&manifest(
        json!({"type":"object","properties":{"count":{"type":"integer"}}}),
    ))
    .unwrap();
    assert!(schema
        .merge(
            "archive",
            &SettingsData::default(),
            "archive:0",
            BTreeMap::from([("count".into(), json!(9_007_199_254_740_992u64))]),
            BTreeMap::new()
        )
        .is_err());
}
