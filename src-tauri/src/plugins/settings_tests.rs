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
        http_video: None,
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

#[test]
fn omit_empty_requires_an_optional_public_string_with_an_explicit_empty_default() {
    for field in [
        json!({"type":"string","omitEmpty":true}),
        json!({"type":"string","default":null,"omitEmpty":true}),
        json!({"type":"string","default":"selected","omitEmpty":true}),
        json!({"type":"string","writeOnly":true,"default":"","omitEmpty":true}),
        json!({"type":"integer","default":0,"omitEmpty":true}),
        json!({"type":"number","default":0,"omitEmpty":true}),
        json!({"type":"boolean","default":false,"omitEmpty":true}),
        json!({"type":"string","default":"","omitEmpty":"true"}),
        json!({"type":"string","default":"","omitEmpty":null}),
    ] {
        assert!(SettingsSchema::compile(&manifest(json!({
            "type":"object","properties":{"selection":field}
        })))
        .is_err());
    }
    assert!(SettingsSchema::compile(&manifest(json!({
        "type":"object","required":["selection"],"properties":{
            "selection":{"type":"string","default":"","omitEmpty":true}
        }
    })))
    .is_err());
    for field in [
        json!({"type":"string","default":"","omitEmpty":false}),
        json!({"type":"string","default":"selected","omitEmpty":false}),
        json!({"type":"string","writeOnly":true,"omitEmpty":false}),
        json!({"type":"integer","default":0,"omitEmpty":false}),
        json!({"type":"boolean","default":false,"omitEmpty":false}),
    ] {
        assert!(SettingsSchema::compile(&manifest(json!({
            "type":"object","required":["selection"],"properties":{"selection":field}
        })))
        .is_ok());
    }
}

#[test]
fn omit_empty_preserves_effective_values_and_only_canonicalizes_opted_in_empty_strings() {
    let metadata = manifest(json!({"type":"object","properties":{
        "selection":{"type":"string","default":"","omitEmpty":true},
        "legacy":{"type":"string","default":""},
        "explicit_false":{"type":"string","default":"","omitEmpty":false}
    }}));
    let schema = SettingsSchema::compile(&metadata).unwrap();
    let mut initial = SettingsData::default();
    initial.values.insert("selection".into(), json!(""));
    initial
        .values
        .insert("unknown".into(), json!({"keep":true}));
    let view = serde_json::to_value(schema.view(&metadata, "archive", &initial).unwrap()).unwrap();
    assert_eq!(view["values"]["selection"], "");
    assert!(view["fields"].as_array().unwrap().iter().all(|field| {
        field.get("omitEmpty").is_none()
            && field.get("omit_empty").is_none()
            && field.get("default").is_none()
    }));
    assert_eq!(
        schema.configuration(&initial).unwrap().values,
        json!({
            "legacy":"","explicit_false":""
        })
    );
    assert_eq!(initial.values["selection"], "");
    let saved = schema
        .merge(
            "archive",
            &initial,
            "archive:0",
            BTreeMap::from([("selection".into(), json!(""))]),
            BTreeMap::new(),
        )
        .unwrap();
    assert!(!saved.values.contains_key("selection"));
    assert_eq!(saved.values["legacy"], "");
    assert_eq!(saved.values["explicit_false"], "");
    assert_eq!(saved.values["unknown"], initial.values["unknown"]);
    assert_eq!(
        schema.view(&metadata, "archive", &saved).unwrap().values["selection"],
        ""
    );
    for value in ["number.room", " "] {
        let updated = schema
            .merge(
                "archive",
                &saved,
                &format!("archive:{}", saved.revision),
                BTreeMap::from([("selection".into(), json!(value))]),
                BTreeMap::new(),
            )
            .unwrap();
        assert_eq!(updated.values["selection"], value);
        assert_eq!(
            schema.configuration(&updated).unwrap().values["selection"],
            value
        );
    }
}

fn ha_numeric_upgrade() -> (PluginManifest, SettingsSchema, SettingsData) {
    let package: Value = serde_json::from_str(include_str!(
        "../../../scripts/plugins/home-assistant-manifest.json"
    ))
    .unwrap();
    let metadata = manifest(package["config_schema"].clone());
    let mut previous_metadata = metadata.clone();
    let properties = previous_metadata.config_schema["properties"]
        .as_object_mut()
        .unwrap();
    for key in ["number_entities", "cover_position_entities"] {
        assert_eq!(metadata.config_schema["properties"][key]["omitEmpty"], true);
        properties.remove(key).unwrap();
    }
    let previous = SettingsSchema::compile(&previous_metadata).unwrap();
    let data = SettingsData {
        revision: "12345678-1234-1234-1234-123456789abc".into(),
        values: serde_json::from_value(json!({
            "ha_base_url":"http://localhost","watch_entities":"\u{b}".repeat(4096),
            "action_entities":"\u{b}".repeat(1200),"media_player_entities":"",
            "binary_entities":"light.a","cover_entities":"cover.a"
        }))
        .unwrap(),
        secrets: BTreeMap::from([("ha_token".into(), "fixture-token".into())]),
        secret_fields: BTreeSet::from(["ha_token".into()]),
    };
    (metadata, previous, data)
}

#[test]
fn ha_numeric_schema_preserves_the_exact_previous_worker_configuration_byte_boundary() {
    let (metadata, previous, mut data) = ha_numeric_upgrade();
    let maximum = super::super::protocol::MAX_CONFIGURATION_BYTES;
    let padding = maximum
        - serde_json::to_vec(&previous.configuration(&data).unwrap())
            .unwrap()
            .len();
    data.secrets
        .get_mut("ha_token")
        .unwrap()
        .push_str(&"x".repeat(padding));
    let original = serde_json::to_vec(&previous.configuration(&data).unwrap()).unwrap();
    assert_eq!(original.len(), maximum);
    let upgraded = SettingsSchema::compile(&metadata).unwrap();
    for explicit_empty in [false, true] {
        if explicit_empty {
            data.values.insert("number_entities".into(), json!(""));
            data.values
                .insert("cover_position_entities".into(), json!(""));
        }
        assert_eq!(
            serde_json::to_vec(&upgraded.configuration(&data).unwrap()).unwrap(),
            original
        );
    }
    data.secrets.get_mut("ha_token").unwrap().push('x');
    assert!(previous.configuration(&data).is_err());
    assert!(upgraded.configuration(&data).is_err());
}

#[test]
fn ha_numeric_upgrade_can_resave_a_previous_settings_record_at_the_storage_limit() {
    use super::super::settings_store::{SettingsStore, MAX_SETTINGS_PLAINTEXT_BYTES};
    use std::sync::Arc;

    let (metadata, previous, mut data) = ha_numeric_upgrade();
    let padding = MAX_SETTINGS_PLAINTEXT_BYTES - serde_json::to_vec(&data).unwrap().len();
    data.secrets
        .get_mut("ha_token")
        .unwrap()
        .push_str(&"x".repeat(padding));
    assert_eq!(
        serde_json::to_vec(&data).unwrap().len(),
        MAX_SETTINGS_PLAINTEXT_BYTES
    );
    let original = serde_json::to_vec(&previous.configuration(&data).unwrap()).unwrap();

    let directory = tempfile::tempdir().unwrap();
    let package_root = directory.path().join("package-store");
    let builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    let builder = {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = builder;
        builder.mode(0o700);
        builder
    };
    builder.create(&package_root).unwrap();
    let store = SettingsStore::new(
        package_root.canonicalize().unwrap(),
        Arc::new(|| Ok(vec![7; 32])),
    );
    store
        .prepare_write(&metadata.plugin_id, &data)
        .unwrap()
        .commit()
        .unwrap();
    let stored = store.read(&metadata.plugin_id).unwrap();
    assert!(stored == data);

    let mut unfiltered_metadata = metadata.clone();
    for key in ["number_entities", "cover_position_entities"] {
        unfiltered_metadata.config_schema["properties"][key]["omitEmpty"] = json!(false);
    }
    assert!(SettingsSchema::compile(&unfiltered_metadata)
        .unwrap()
        .configuration(&stored)
        .is_err());
    let upgraded = SettingsSchema::compile(&metadata).unwrap();
    assert_eq!(
        serde_json::to_vec(&upgraded.configuration(&stored).unwrap()).unwrap(),
        original
    );
    let view = upgraded.view(&metadata, "upgraded", &stored).unwrap();
    assert_eq!(view.values["number_entities"], "");
    assert_eq!(view.values["cover_position_entities"], "");
    let saved = upgraded
        .merge(
            "upgraded",
            &stored,
            &view.revision,
            view.values,
            BTreeMap::new(),
        )
        .unwrap();
    assert!(saved == stored);
    store
        .prepare_write(&metadata.plugin_id, &saved)
        .unwrap()
        .commit()
        .unwrap();
    assert!(store.read(&metadata.plugin_id).unwrap() == saved);
    let mut oversized = saved;
    oversized.secrets.get_mut("ha_token").unwrap().push('x');
    assert!(store
        .prepare_write(&metadata.plugin_id, &oversized)
        .is_err());
}
