//! Bounded read-only discovery; explicit entities remain independently authoritative.

use crate::config::{matches_discovery, MAX_ENTITIES};
use crate::state::readonly_contribution;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

const MAX_BOOTSTRAP_EVENTS: usize = 128;
const MAX_SNAPSHOT_ENTRIES: usize = 4096;

enum Phase {
    Dormant,
    Bootstrap(BTreeMap<String, Option<Value>>),
    Live,
    Failed,
}

pub(crate) struct Discovery {
    explicit: HashSet<String>,
    prefixes: Vec<String>,
    capacity: usize,
    phase: Phase,
    cards: BTreeMap<String, Value>,
    next_id: u64,
    limited: bool,
}

fn project(name: &str, state: Option<&Value>) -> Option<Value> {
    let state = state.filter(|state| {
        state["entity_id"].as_str() == Some(name) && state["state"].as_str().is_some()
    })?;
    Some(readonly_contribution(name, state))
}

impl Discovery {
    pub(crate) fn new(explicit: &[String], prefixes: &[String]) -> Self {
        Self {
            explicit: explicit.iter().cloned().collect(),
            prefixes: prefixes.to_vec(),
            capacity: MAX_ENTITIES.saturating_sub(explicit.len()),
            phase: Phase::Dormant,
            cards: BTreeMap::new(),
            next_id: 0,
            limited: false,
        }
    }

    pub(crate) fn begin_session(&mut self) {
        self.clear();
        if self.capacity != 0 && !self.prefixes.is_empty() {
            self.phase = Phase::Bootstrap(BTreeMap::new());
        }
    }

    pub(crate) fn clear(&mut self) {
        self.phase = Phase::Dormant;
        self.cards.clear();
        self.limited = false;
    }

    fn matches(&self, name: &str) -> bool {
        !self.explicit.contains(name) && matches_discovery(&self.prefixes, name)
    }

    pub(crate) fn failed(&mut self) -> bool {
        if matches!(self.phase, Phase::Dormant | Phase::Failed) {
            return false;
        }
        self.phase = Phase::Failed;
        self.cards.clear();
        self.limited = false;
        true
    }

    pub(crate) fn notice(&self) -> Option<&'static str> {
        if matches!(self.phase, Phase::Failed) {
            Some("Discovery unavailable")
        } else if self.limited {
            Some("Discovery limit reached")
        } else {
            None
        }
    }

    pub(crate) fn items(&self) -> impl Iterator<Item = &Value> {
        self.cards.values()
    }

    pub(crate) fn live(&mut self, name: &str, state: Option<&Value>) -> bool {
        if matches!(self.phase, Phase::Dormant | Phase::Failed) || !self.matches(name) {
            return false;
        }
        let item = project(name, state);
        match &mut self.phase {
            Phase::Bootstrap(buffer) => {
                if !buffer.contains_key(name) && buffer.len() >= MAX_BOOTSTRAP_EVENTS {
                    return self.failed();
                }
                // Buffer only projected cards or tombstones, never upstream attributes.
                buffer.insert(name.to_owned(), item);
                false
            }
            Phase::Live => {
                let Some(mut item) = item else {
                    return self.cards.remove(name).is_some();
                };
                if let Some(previous) = self.cards.get_mut(name) {
                    item["id"] = previous["id"].clone();
                    if *previous == item {
                        return false;
                    }
                    *previous = item;
                    return true;
                }
                if self.cards.len() == self.capacity {
                    let changed = !self.limited;
                    self.limited = true;
                    return changed;
                }
                self.insert(name.to_owned(), item)
            }
            Phase::Dormant | Phase::Failed => false,
        }
    }

    fn insert(&mut self, name: String, mut item: Value) -> bool {
        let Some(next_id) = self.next_id.checked_add(1) else {
            return self.failed();
        };
        item["id"] = Value::String(format!("discovery-{}", self.next_id));
        self.next_id = next_id;
        self.cards.insert(name, item);
        true
    }

    pub(crate) fn snapshot(&mut self, states: &[Value]) -> bool {
        let Phase::Bootstrap(buffer) = &self.phase else {
            return false;
        };
        let selection = self.select(states, buffer);
        let Ok((cards, limited)) = selection else {
            return self.failed();
        };
        if self.next_id.checked_add(cards.len() as u64).is_none() {
            return self.failed();
        }
        self.phase = Phase::Live;
        self.limited = limited;
        for (name, item) in cards {
            self.insert(name, item);
        }
        !self.cards.is_empty() || self.limited
    }

    fn select(
        &self,
        states: &[Value],
        buffer: &BTreeMap<String, Option<Value>>,
    ) -> Result<(BTreeMap<String, Value>, bool), ()> {
        if states.len() > MAX_SNAPSHOT_ENTRIES {
            return Err(());
        }
        // Borrow identities from the bounded source solely to reject duplicates.
        // Only the lexical first free-slot count is retained as projected cards.
        let mut seen = HashSet::new();
        let mut cards = BTreeMap::new();
        let mut count = 0usize;
        let mut include = |name: &str, item: Value| {
            count += 1;
            cards.insert(name.to_owned(), item);
            if cards.len() > self.capacity {
                cards.pop_last();
            }
        };
        for state in states {
            let name = state.get("entity_id").and_then(Value::as_str).ok_or(())?;
            if !self.matches(name) {
                continue;
            }
            if !seen.insert(name) {
                return Err(());
            }
            let initial = project(name, Some(state)).ok_or(())?;
            if let Some(item) = buffer.get(name).cloned().unwrap_or(Some(initial)) {
                include(name, item);
            }
        }
        for (name, item) in buffer {
            if seen.contains(name.as_str()) {
                continue;
            }
            if let Some(item) = item {
                include(name, item.clone());
            }
        }
        Ok((cards, count > self.capacity))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture(capacity: usize) -> Discovery {
        let explicit = (0..MAX_ENTITIES - capacity)
            .map(|index| format!("sensor.explicit_{index}"))
            .collect::<Vec<_>>();
        let mut discovery = Discovery::new(&explicit, &["sensor.".into(), "binary_sensor.".into()]);
        discovery.begin_session();
        discovery
    }

    fn state(name: &str, value: &str) -> Value {
        json!({"entity_id":name,"state":value,"attributes":{"friendly_name":name}})
    }

    fn names(discovery: &Discovery) -> Vec<&str> {
        discovery.cards.keys().map(String::as_str).collect()
    }

    #[test]
    fn snapshot_selects_lexical_free_slots_after_live_updates_and_tombstones() {
        let mut discovery = fixture(2);
        discovery.live("sensor.a", None);
        discovery.live("sensor.b", Some(&state("sensor.b", "22")));
        discovery.live("sensor.aa", Some(&state("sensor.aa", "33")));
        discovery.live("sensor.zz", Some(&state("sensor.zz", "44")));
        assert!(discovery.snapshot(&[
            state("sensor.z", "1"),
            state("sensor.a", "1"),
            state("sensor.c", "1"),
            state("sensor.b", "1"),
            state("sensor.explicit_0", "999"),
        ]));
        assert_eq!(names(&discovery), ["sensor.aa", "sensor.b"]);
        assert_eq!(discovery.cards["sensor.aa"]["value"], 33.0);
        assert_eq!(discovery.cards["sensor.b"]["value"], 22.0);
        assert_eq!(discovery.notice(), Some("Discovery limit reached"));
        assert!(matches!(discovery.phase, Phase::Live));
        assert!(!discovery.snapshot(&[state("sensor.a", "old snapshot")]));
        assert_eq!(names(&discovery), ["sensor.aa", "sensor.b"]);
    }

    #[test]
    fn duplicates_outside_top_k_and_malformed_relevant_rows_reject_the_whole_snapshot() {
        for states in [
            vec![
                state("sensor.a", "1"),
                state("sensor.z", "1"),
                state("sensor.z", "2"),
            ],
            vec![
                state("sensor.a", "1"),
                json!({"entity_id":"sensor.b","state":42}),
            ],
            vec![state("sensor.a", "1"), json!({"state":"missing identity"})],
            vec![state("sensor.a", "1"), json!(null)],
        ] {
            let mut discovery = fixture(1);
            discovery.live("sensor.z", None);
            assert!(discovery.snapshot(&states));
            assert!(discovery.cards.is_empty());
            assert_eq!(discovery.notice(), Some("Discovery unavailable"));
            assert!(!discovery.live("sensor.a", Some(&state("sensor.a", "later"))));
            assert!(!discovery.snapshot(&[state("sensor.a", "later")]));
        }
        let mut discovery = fixture(1);
        discovery.snapshot(&[
            state("sensor.explicit_0", "1"),
            state("sensor.explicit_0", "2"),
            json!({"entity_id":"light.ignored"}),
            state("sensor.a", "3"),
        ]);
        assert_eq!(names(&discovery), ["sensor.a"]);
        assert_eq!(discovery.notice(), None);
    }

    #[test]
    fn bootstrap_buffer_counts_distinct_ids_and_abandons_the_session_on_overflow() {
        let mut discovery = fixture(1);
        for index in 0..MAX_BOOTSTRAP_EVENTS {
            discovery.live(&format!("sensor.buffer_{index}"), None);
        }
        for value in 0..256 {
            discovery.live(
                "sensor.buffer_0",
                Some(&state("sensor.buffer_0", &value.to_string())),
            );
        }
        let Phase::Bootstrap(buffer) = &discovery.phase else {
            panic!("bootstrap remains active");
        };
        assert_eq!(buffer.len(), MAX_BOOTSTRAP_EVENTS);
        assert_eq!(buffer["sensor.buffer_0"].as_ref().unwrap()["value"], 255.0);
        assert!(discovery.live("sensor.overflow", None));
        assert!(matches!(discovery.phase, Phase::Failed));
        assert!(discovery.cards.is_empty());
        assert!(!discovery.snapshot(&[state("sensor.buffer_1", "stale")]));
        assert!(!discovery.live("sensor.later", Some(&state("sensor.later", "ignored"))));
        discovery.begin_session();
        assert_eq!(discovery.notice(), None);
        discovery.snapshot(&[state("sensor.reconnected", "1")]);
        assert_eq!(names(&discovery), ["sensor.reconnected"]);
    }

    #[test]
    fn buffered_observations_are_only_bounded_readonly_projections() {
        let mut discovery = fixture(4);
        for (name, value) in [
            ("sensor.text", "\\\"".repeat(4096)),
            ("sensor.metric", "12.5".into()),
            ("sensor.unknown", "unknown".into()),
            ("binary_sensor.flag", "on".into()),
        ] {
            discovery.live(name, Some(&json!({"entity_id":name,"state":value,
                "attributes":{"friendly_name":"\\\"".repeat(128),"unit_of_measurement":"kWh".repeat(100),
                "secret_raw_attribute":"never-retain-this".repeat(1000)}})));
        }
        let Phase::Bootstrap(buffer) = &discovery.phase else {
            panic!("bootstrap remains active");
        };
        for item in buffer.values().flatten() {
            assert!(matches!(
                item["kind"].as_str(),
                Some("text" | "metric" | "status")
            ));
            assert!(item["title"].as_str().unwrap().len() <= 128);
            assert!(item.get("attributes").is_none());
            assert!(item.get("action_id").is_none());
            let encoded = serde_json::to_string(item).unwrap();
            assert!(encoded.len() < 1600);
            assert!(!encoded.contains("never-retain-this"));
        }
        assert_eq!(
            buffer["sensor.text"].as_ref().unwrap()["text"]
                .as_str()
                .unwrap()
                .len(),
            512
        );
        assert_eq!(
            buffer["sensor.metric"].as_ref().unwrap()["unit"]
                .as_str()
                .unwrap()
                .len(),
            32
        );
        discovery.snapshot(&[]);
        assert_eq!(discovery.cards.len(), 4);
    }

    #[test]
    fn live_capacity_drops_unseen_rows_without_catalog_and_preserves_present_ids() {
        let mut discovery = fixture(2);
        discovery.snapshot(&[state("sensor.b", "1"), state("sensor.d", "2")]);
        let original_id = discovery.cards["sensor.d"]["id"].clone();
        assert!(discovery.live("sensor.a", Some(&state("sensor.a", "3"))));
        assert_eq!(discovery.notice(), Some("Discovery limit reached"));
        assert_eq!(names(&discovery), ["sensor.b", "sensor.d"]);
        assert!(discovery.live("sensor.b", None));
        assert_eq!(names(&discovery), ["sensor.d"]);
        discovery.live("sensor.c", Some(&state("sensor.c", "4")));
        discovery.live("sensor.d", Some(&state("sensor.d", "5")));
        assert_eq!(discovery.cards["sensor.d"]["id"], original_id);
        discovery.live(
            "sensor.d",
            Some(&json!({"entity_id":"sensor.other","state":"wrong"})),
        );
        assert!(!discovery.cards.contains_key("sensor.d"));
        discovery.live("sensor.d", Some(&state("sensor.d", "6")));
        assert_ne!(discovery.cards["sensor.d"]["id"], original_id);
        assert_eq!(names(&discovery), ["sensor.c", "sensor.d"]);
        assert_eq!(discovery.notice(), Some("Discovery limit reached"));
    }

    #[test]
    fn resets_clear_cards_and_tombstones_without_reusing_ids_or_accepting_late_results() {
        let mut discovery = fixture(2);
        discovery.live("sensor.a", None);
        discovery.snapshot(&[state("sensor.a", "old"), state("sensor.b", "1")]);
        let id = discovery.cards["sensor.b"]["id"].clone();
        discovery.clear();
        assert!(!discovery.snapshot(&[state("sensor.stale", "1")]));
        assert!(!discovery.live("sensor.stale", Some(&state("sensor.stale", "1"))));
        discovery.begin_session();
        discovery.snapshot(&[state("sensor.a", "new"), state("sensor.b", "2")]);
        assert_eq!(names(&discovery), ["sensor.a", "sensor.b"]);
        assert_ne!(discovery.cards["sensor.b"]["id"], id);
        discovery.live("sensor.a", None);
        discovery.next_id = u64::MAX;
        assert!(discovery.live("sensor.c", Some(&state("sensor.c", "3"))));
        assert!(discovery.cards.is_empty());
        assert_eq!(discovery.notice(), Some("Discovery unavailable"));
    }

    #[test]
    fn empty_prefixes_and_full_explicit_capacity_leave_discovery_inactive() {
        for mut discovery in [fixture(0), Discovery::new(&[], &[])] {
            discovery.begin_session();
            assert!(!discovery.live("sensor.a", Some(&state("sensor.a", "1"))));
            assert!(!discovery.snapshot(&[state("sensor.a", "1")]));
            assert!(!discovery.failed());
            assert!(discovery.cards.is_empty());
            assert_eq!(discovery.notice(), None);
        }
    }

    #[test]
    fn snapshot_source_count_is_bounded_even_when_every_row_is_unrelated() {
        let states = vec![state("light.ignored", "on"); MAX_SNAPSHOT_ENTRIES];
        let mut discovery = fixture(2);
        assert!(!discovery.snapshot(&states));
        assert!(matches!(discovery.phase, Phase::Live));
        discovery.begin_session();
        let mut oversized = states;
        oversized.push(state("light.more", "on"));
        assert!(discovery.snapshot(&oversized));
        assert_eq!(discovery.notice(), Some("Discovery unavailable"));
    }
}
