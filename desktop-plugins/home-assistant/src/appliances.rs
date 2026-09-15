//! Bounded read-only appliance overlays; raw entity cards retain all authority.

use crate::config::DishwasherProfile;
use serde_json::{json, Value};

const MAX_ROLE_STATE_BYTES: usize = 128;

fn complete_state(value: Option<&Value>) -> Option<&str> {
    let state = value?.get("state")?.as_str()?.trim();
    (!state.is_empty()
        && state.len() <= MAX_ROLE_STATE_BYTES
        && !state.chars().any(char::is_control))
    .then_some(state)
}

#[derive(Default, PartialEq, Eq)]
enum Running {
    #[default]
    Unobserved,
    Status(&'static str),
    State(String),
}

impl Running {
    fn observe(value: Option<&Value>) -> Self {
        let Some(state) = complete_state(value) else {
            return Self::Status("Unavailable");
        };
        match state.to_ascii_lowercase().as_str() {
            "on" | "running" => Self::State("Running".into()),
            "off" | "idle" => Self::State("Idle".into()),
            "unknown" => Self::Status("Unknown"),
            "unavailable" => Self::Status("Unavailable"),
            _ => Self::State(state.to_owned()),
        }
    }
}

fn runtime(value: Option<&Value>) -> Option<String> {
    let state = complete_state(value)?;
    if matches!(
        state.to_ascii_lowercase().as_str(),
        "unknown" | "unavailable" | "off" | "idle"
    ) || state.parse::<f64>().is_ok_and(|number| !number.is_finite())
    {
        return None;
    }
    Some(state.to_owned())
}

pub(crate) struct Dishwasher {
    running_index: usize,
    duration_index: Option<usize>,
    running: Running,
    runtime: Option<String>,
}

impl Dishwasher {
    pub(crate) fn new(entities: &[String], profile: &DishwasherProfile) -> Option<Self> {
        let running_index = entities
            .iter()
            .position(|entity| entity == &profile.running_entity)?;
        let duration_index = match &profile.duration_entity {
            Some(name) => Some(entities.iter().position(|entity| entity == name)?),
            None => None,
        };
        Some(Self {
            running_index,
            duration_index,
            running: Running::Unobserved,
            runtime: None,
        })
    }

    pub(crate) fn clear(&mut self) {
        self.running = Running::Unobserved;
        self.runtime = None;
    }

    // Book applies its per-entity stale-read and identity guards before caching
    // these small role observations. No upstream attributes are retained.
    pub(crate) fn update(&mut self, index: usize, state: Option<&Value>) -> bool {
        if index == self.running_index {
            let running = Running::observe(state);
            if self.running != running {
                self.running = running;
                return true;
            }
        } else if Some(index) == self.duration_index {
            let runtime = runtime(state);
            if self.runtime != runtime {
                self.runtime = runtime;
                return true;
            }
        }
        false
    }

    pub(crate) fn contribution(&self, index: usize, raw: &Value) -> Option<Value> {
        if index != self.running_index {
            return None;
        }
        match &self.running {
            Running::Unobserved => None,
            Running::Status(value) => Some(
                json!({"kind":"status","id":raw["id"],"title":raw["title"],"value":value,"tone":"neutral"}),
            ),
            Running::State(state) => {
                let mut text = format!("State: {state}");
                if let Some(runtime) = &self.runtime {
                    text.push_str("\nRuntime since midnight: ");
                    text.push_str(runtime);
                }
                Some(json!({"kind":"text","id":raw["id"],"title":raw["title"],"text":text}))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> Dishwasher {
        Dishwasher::new(
            &["sensor.runtime".into(), "binary_sensor.running".into()],
            &DishwasherProfile {
                running_entity: "binary_sensor.running".into(),
                duration_entity: Some("sensor.runtime".into()),
            },
        )
        .unwrap()
    }

    fn card(profile: &Dishwasher) -> Value {
        profile
            .contribution(1, &json!({"id":"entity-1","title":"Dishwasher"}))
            .unwrap()
    }

    #[test]
    fn known_running_states_are_normalized_without_interpreting_other_literals() {
        let mut profile = profile();
        assert!(profile.update(
            0,
            Some(&json!({"state":"01:23:45","attributes":{"unit_of_measurement":"wrong"}}))
        ));
        for (observed, expected) in [
            ("on", "Running"),
            (" RUNNING ", "Running"),
            ("OFF", "Idle"),
            (" idle ", "Idle"),
            ("Paused", "Paused"),
            ("Error 7", "Error 7"),
            ("0", "0"),
            ("false", "false"),
        ] {
            profile.update(1, Some(&json!({"state":observed})));
            assert_eq!(
                card(&profile),
                json!({"kind":"text","id":"entity-1","title":"Dishwasher","text":format!("State: {expected}\nRuntime since midnight: 01:23:45")})
            );
        }
        for (observed, expected) in [(" UNKNOWN ", "Unknown"), (" UnAvAiLaBlE ", "Unavailable")] {
            profile.update(1, Some(&json!({"state":observed})));
            assert_eq!(
                card(&profile),
                json!({"kind":"status","id":"entity-1","title":"Dishwasher","value":expected,"tone":"neutral"})
            );
        }
    }

    #[test]
    fn runtime_is_a_complete_literal_with_no_conversion_or_unit_inference() {
        let mut profile = profile();
        profile.update(1, Some(&json!({"state":"off"})));
        let multibyte = "🌙".repeat(32);
        for value in [
            "01:23:45",
            "1.500",
            "2h 10m",
            "1e-999",
            "  0  ",
            multibyte.as_str(),
        ] {
            profile.update(
                0,
                Some(&json!({"state":value,"attributes":{"unit_of_measurement":"hours"}})),
            );
            assert_eq!(
                card(&profile)["text"],
                format!("State: Idle\nRuntime since midnight: {}", value.trim())
            );
        }
        for state in [
            json!(null),
            json!(true),
            json!(12),
            json!([]),
            json!({}),
            json!("unknown"),
            json!("UNAVAILABLE"),
            json!("Off"),
            json!(" IDLE "),
            json!(""),
            json!(" \n\t "),
            json!("12\n34"),
            json!("12\t34"),
            json!("NaN"),
            json!("-inf"),
            json!("Infinity"),
            json!("1e999"),
            json!("x".repeat(129)),
            json!("🌙".repeat(33)),
        ] {
            profile.update(0, Some(&json!({"state":"1.500"})));
            assert!(profile.update(0, Some(&json!({"state":state}))));
            assert_eq!(card(&profile)["text"], "State: Idle", "runtime {state}");
        }
        profile.update(0, Some(&json!({"state":"1.5"})));
        assert!(profile.update(0, None));
        assert_eq!(card(&profile)["text"], "State: Idle");
    }

    #[test]
    fn malformed_or_overlong_primary_states_do_not_expose_partial_state_or_runtime() {
        let mut profile = profile();
        profile.update(0, Some(&json!({"state":"1.5"})));
        for state in [
            json!(null),
            json!(true),
            json!(12),
            json!([]),
            json!({}),
            json!(""),
            json!(" \n\t "),
            json!("run\nning"),
            json!("x".repeat(129)),
            json!("🌙".repeat(33)),
        ] {
            profile.update(1, Some(&json!({"state":"on"})));
            assert!(profile.update(1, Some(&json!({"state":state}))));
            assert_eq!(card(&profile)["value"], "Unavailable");
            assert!(card(&profile).get("text").is_none());
        }
        profile.update(1, Some(&json!({"state":"🌙".repeat(32)})));
        assert_eq!(
            card(&profile)["text"],
            format!("State: {}\nRuntime since midnight: 1.5", "🌙".repeat(32))
        );
        profile.update(1, None);
        assert_eq!(card(&profile)["value"], "Unavailable");
    }

    #[test]
    fn only_role_indices_are_observed_and_clear_drops_both_cached_values() {
        let mut profile = profile();
        assert!(!profile.update(2, Some(&json!({"state":"on"}))));
        assert!(profile.contribution(1, &json!({})).is_none());
        profile.update(1, Some(&json!({"state":"on"})));
        profile.update(0, Some(&json!({"state":"12:00"})));
        assert!(!profile.update(
            0,
            Some(&json!({"state":"12:00","attributes":{"new":"unused"}}))
        ));
        assert!(profile.contribution(0, &json!({"id":"entity-0"})).is_none());
        profile.clear();
        assert!(profile.contribution(1, &json!({})).is_none());
        profile.update(1, Some(&json!({"state":"on"})));
        assert_eq!(card(&profile)["text"], "State: Running");
    }
}
