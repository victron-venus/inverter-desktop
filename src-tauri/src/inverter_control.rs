//! Commands owned by the inverter-control daemon on Cerbo.
//!
//! Flags are read from `inverter/state.booleans` and written through
//! `inverter/cmd/toggle`. Home Assistant may expose the same controls as MQTT
//! switches, but is not required to read or operate them from this application.

use serde_json::Value;

/// Keep aligned with inverter-control's `control_flags.CONTROL_FLAG_KEYS`.
pub(crate) const FLAG_KEYS: &[&str] = &[
    "only_charging",
    "no_feed",
    "house_support",
    "charge_battery",
    "do_not_supply_charger",
    "set_limit_to_ev_charger",
    "minimize_charging",
];

/// Bare keys are canonical. Accept the historical configuration prefix only;
/// e.g. `switch.only_charging` remains a genuine Home Assistant entity.
pub(crate) fn flag_key(entity_or_key: &str) -> Option<&str> {
    let entity_or_key = entity_or_key.trim();
    let key = entity_or_key
        .strip_prefix("input_boolean.")
        .unwrap_or(entity_or_key);
    FLAG_KEYS.contains(&key).then_some(key)
}

pub(crate) fn is_flag(entity_or_key: &str) -> bool {
    flag_key(entity_or_key).is_some()
}

/// Canonicalize legacy toggle IDs at the MQTT boundary. An explicit state is
/// authoritative; otherwise preserve Desktop's absolute on/off command based
/// on the last received flag value (initially off when not yet published).
/// This keeps repeated delivery from toggling a flag twice in inverter-control.
pub(crate) fn prepare_command(
    action: &str,
    payload: &mut Value,
    current_flag_state: impl FnOnce(&str) -> Option<bool>,
) {
    if action != "toggle" {
        return;
    }
    let Some(key) = payload
        .get("entity")
        .and_then(Value::as_str)
        .and_then(flag_key)
        .map(str::to_owned)
    else {
        return;
    };
    let Some(object) = payload.as_object_mut() else {
        return;
    };
    object.insert("entity".to_string(), Value::String(key.clone()));
    if !object.contains_key("state") {
        let next = if current_flag_state(&key).unwrap_or(false) {
            "off"
        } else {
            "on"
        };
        object.insert("state".to_string(), Value::String(next.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn known_flags_accept_only_canonical_keys_and_legacy_aliases() {
        for key in FLAG_KEYS {
            assert_eq!(flag_key(key), Some(*key));
            assert_eq!(flag_key(&format!("input_boolean.{key}")), Some(*key));
            assert_eq!(flag_key(&format!("switch.{key}")), None);
            assert_eq!(flag_key(&format!("sensor.{key}")), None);
        }
        assert_eq!(flag_key("input_boolean.guest_mode"), None);
        assert_eq!(flag_key("input_boolean.input_boolean.no_feed"), None);
        assert_eq!(flag_key(" no_feed "), Some("no_feed"));
        assert_eq!(flag_key(" input_boolean.no_feed "), Some("no_feed"));
    }

    #[test]
    fn legacy_toggle_is_normalized_and_uses_mqtt_flag_state() {
        let mut payload = json!({"entity": "input_boolean.no_feed"});
        prepare_command("toggle", &mut payload, |key| {
            assert_eq!(key, "no_feed");
            Some(true)
        });
        assert_eq!(payload, json!({"entity": "no_feed", "state": "off"}));
    }

    #[test]
    fn disabled_or_unknown_flags_preserve_initial_on_behavior() {
        for current in [Some(false), None] {
            let mut payload = json!({"entity": "house_support"});
            prepare_command("toggle", &mut payload, |_| current);
            assert_eq!(payload, json!({"entity": "house_support", "state": "on"}));
        }
    }

    #[test]
    fn explicit_state_is_never_replaced_by_a_cached_toggle() {
        for state in [
            json!("on"),
            json!("off"),
            json!(true),
            json!(false),
            json!(1),
            json!(0),
        ] {
            let mut payload = json!({"entity": "input_boolean.only_charging", "state": state});
            prepare_command("toggle", &mut payload, |_| {
                panic!("state lookup not needed")
            });
            assert_eq!(payload, json!({"entity": "only_charging", "state": state}));
        }
    }

    #[test]
    fn invalid_explicit_state_is_left_for_the_daemon_to_reject() {
        let mut payload = json!({"entity": "no_feed", "state": null});
        prepare_command("toggle", &mut payload, |_| {
            panic!("state lookup not needed")
        });
        assert_eq!(payload, json!({"entity": "no_feed", "state": null}));
    }

    #[test]
    fn home_entities_and_non_toggle_actions_are_unchanged() {
        for (action, mut payload) in [
            ("toggle", json!({"entity": "switch.only_charging"})),
            ("toggle", json!({"entity": "input_boolean.guest_mode"})),
            ("toggle", json!({"entity": "switch.garage"})),
            ("press", json!({"entity": "input_boolean.only_charging"})),
            ("water_mode", json!({"which": "pump", "mode": 1})),
            ("toggle", Value::Null),
        ] {
            let original = payload.clone();
            prepare_command(action, &mut payload, |_| panic!("state lookup not needed"));
            assert_eq!(payload, original);
        }
    }
}
