//! Bounded read-only appliance overlays; raw entity cards retain all authority.

use crate::config::{ApplianceProfiles, DishwasherProfile};
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
enum Observation {
    #[default]
    Unobserved,
    Status(&'static str),
    State(String),
}

impl Observation {
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
    running: Observation,
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
            running: Observation::Unobserved,
            runtime: None,
        })
    }

    pub(crate) fn clear(&mut self) {
        self.running = Observation::Unobserved;
        self.runtime = None;
    }

    // Book applies its per-entity stale-read and identity guards before caching
    // these small role observations. No upstream attributes are retained.
    pub(crate) fn update(&mut self, index: usize, state: Option<&Value>) -> bool {
        if index == self.running_index {
            let running = Observation::observe(state);
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
            Observation::Unobserved => None,
            Observation::Status(value) => Some(
                json!({"kind":"status","id":raw["id"],"title":raw["title"],"value":value,"tone":"neutral"}),
            ),
            Observation::State(state) => {
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

fn remaining(value: Option<&Value>) -> Observation {
    let Some(state) = complete_state(value) else {
        return Observation::Status("Unavailable");
    };
    match state.to_ascii_lowercase().as_str() {
        "unknown" => Observation::Status("Unknown"),
        "unavailable" => Observation::Status("Unavailable"),
        "off" | "idle" => Observation::Status("Idle"),
        _ if state.parse::<f64>().is_ok_and(|number| !number.is_finite()) => {
            Observation::Status("Unavailable")
        }
        _ => Observation::State(state.to_owned()),
    }
}

struct RemainingTime {
    index: usize,
    state: Observation,
}

impl RemainingTime {
    fn new(entities: &[String], name: &str) -> Option<Self> {
        Some(Self {
            index: entities.iter().position(|entity| entity == name)?,
            state: Observation::Unobserved,
        })
    }

    fn update(&mut self, index: usize, value: Option<&Value>) -> bool {
        if index != self.index {
            return false;
        }
        let state = remaining(value);
        if state == self.state {
            return false;
        }
        self.state = state;
        true
    }

    fn contribution(&self, index: usize, raw: &Value) -> Option<Value> {
        if index != self.index {
            return None;
        }
        match &self.state {
            Observation::Unobserved => None,
            Observation::Status(value) => Some(
                json!({"kind":"status","id":raw["id"],"title":raw["title"],"value":value,"tone":"neutral"}),
            ),
            Observation::State(value) => Some(
                json!({"kind":"text","id":raw["id"],"title":raw["title"],"text":format!("Remaining time: {value}")}),
            ),
        }
    }
}

pub(crate) struct Appliances {
    dishwasher: Option<Dishwasher>,
    laundry: Vec<RemainingTime>,
}

impl Appliances {
    pub(crate) fn new(entities: &[String], profiles: &ApplianceProfiles) -> Self {
        Self {
            dishwasher: profiles
                .dishwasher
                .as_ref()
                .and_then(|profile| Dishwasher::new(entities, profile)),
            laundry: profiles
                .washer_remaining_entity
                .iter()
                .chain(&profiles.dryer_remaining_entity)
                .filter_map(|name| RemainingTime::new(entities, name))
                .collect(),
        }
    }

    pub(crate) fn clear(&mut self) {
        if let Some(dishwasher) = &mut self.dishwasher {
            dishwasher.clear();
        }
        for profile in &mut self.laundry {
            profile.state = Observation::Unobserved;
        }
    }

    pub(crate) fn update(&mut self, index: usize, value: Option<&Value>) -> bool {
        let mut changed = false;
        if let Some(dishwasher) = &mut self.dishwasher {
            changed |= dishwasher.update(index, value);
        }
        // A source may be both a dishwasher runtime and a laundry primary.
        // Every cache must observe the event, including after an earlier change.
        for profile in &mut self.laundry {
            changed |= profile.update(index, value);
        }
        changed
    }

    pub(crate) fn contribution(&self, index: usize, raw: &Value) -> Option<Value> {
        self.dishwasher
            .as_ref()
            .and_then(|profile| profile.contribution(index, raw))
            .or_else(|| {
                self.laundry
                    .iter()
                    .find_map(|profile| profile.contribution(index, raw))
            })
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

    #[test]
    fn laundry_preserves_complete_remaining_literals_and_zero_without_inferred_activity_or_units() {
        let mut timer =
            RemainingTime::new(&["sensor.remaining".into()], "sensor.remaining").unwrap();
        let raw = json!({"kind":"metric","id":"entity-0","title":"Washer","value":0});
        let multibyte = "🌙".repeat(32);
        for value in [
            "0",
            "0.00",
            "01:23:45",
            "  25 min  ",
            "Paused",
            "on",
            "running",
            "1e-999",
            multibyte.as_str(),
        ] {
            timer.update(0, Some(&json!({"state":value,"attributes":{"unit_of_measurement":"hours","remaining":99}})));
            assert_eq!(
                timer.contribution(0, &raw).unwrap(),
                json!({"kind":"text","id":"entity-0","title":"Washer","text":format!("Remaining time: {}", value.trim())})
            );
        }
        for (value, expected) in [
            ("UNKNOWN", "Unknown"),
            (" unavailable ", "Unavailable"),
            (" OFF ", "Idle"),
            ("idle", "Idle"),
        ] {
            timer.update(0, Some(&json!({"state":value})));
            assert_eq!(
                timer.contribution(0, &raw).unwrap(),
                json!({"kind":"status","id":"entity-0","title":"Washer","value":expected,"tone":"neutral"})
            );
        }
        for value in [
            json!(null),
            json!(false),
            json!(0),
            json!([]),
            json!({}),
            json!(""),
            json!(" \n\t "),
            json!("12\n34"),
            json!("x".repeat(129)),
            json!("🌙".repeat(33)),
            json!("NaN"),
            json!("-inf"),
            json!("Infinity"),
            json!("1e999"),
        ] {
            timer.update(0, Some(&json!({"state":"5"})));
            assert!(timer.update(0, Some(&json!({"state":value}))));
            assert_eq!(
                timer.contribution(0, &raw).unwrap()["value"],
                "Unavailable",
                "{value}"
            );
        }
        timer.update(0, None);
        assert_eq!(timer.contribution(0, &raw).unwrap()["value"], "Unavailable");
    }

    #[test]
    fn laundry_and_dishwasher_shared_source_updates_are_eager_and_all_caches_clear() {
        let mut profiles = Appliances::new(
            &[
                "sensor.shared".into(),
                "binary_sensor.running".into(),
                "sensor.dryer".into(),
            ],
            &ApplianceProfiles {
                dishwasher: Some(DishwasherProfile {
                    running_entity: "binary_sensor.running".into(),
                    duration_entity: Some("sensor.shared".into()),
                }),
                washer_remaining_entity: Some("sensor.shared".into()),
                dryer_remaining_entity: Some("sensor.dryer".into()),
            },
        );
        let raw = |index| json!({"id":format!("entity-{index}"),"title":"Same title"});
        profiles.update(1, Some(&json!({"state":"on"})));
        profiles.update(2, Some(&json!({"state":"10"})));
        for value in ["1.50", "1.500"] {
            assert!(profiles.update(0, Some(&json!({"state":value}))));
            assert_eq!(
                profiles.contribution(0, &raw(0)).unwrap()["text"],
                format!("Remaining time: {value}")
            );
            assert_eq!(
                profiles.contribution(1, &raw(1)).unwrap()["text"],
                format!("State: Running\nRuntime since midnight: {value}")
            );
            assert_eq!(
                profiles.contribution(2, &raw(2)).unwrap()["text"],
                "Remaining time: 10"
            );
        }
        profiles.update(0, None);
        assert_eq!(
            profiles.contribution(0, &raw(0)).unwrap()["value"],
            "Unavailable"
        );
        assert_eq!(
            profiles.contribution(1, &raw(1)).unwrap()["text"],
            "State: Running"
        );
        profiles.clear();
        for index in 0..3 {
            assert!(profiles.contribution(index, &raw(index)).is_none());
        }
        profiles.update(1, Some(&json!({"state":"on"})));
        assert_eq!(
            profiles.contribution(1, &raw(1)).unwrap()["text"],
            "State: Running"
        );
    }
}
