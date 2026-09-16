//! One notification owner per worker session; initial snapshots never announce changes.

use crate::state::bounded;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

#[derive(Default)]
pub(crate) struct Notifications {
    pub enabled: bool,
    previous: HashMap<String, String>,
    last: HashMap<String, Instant>,
    pending: VecDeque<Value>,
    sequence: u64,
}

impl Notifications {
    pub fn observe(&mut self, entity: &str, title: &str, state: Option<&str>, live: bool) {
        self.observe_at(entity, title, state, live, Instant::now());
    }

    fn observe_at(
        &mut self,
        entity: &str,
        title: &str,
        state: Option<&str>,
        live: bool,
        now: Instant,
    ) {
        if !self.enabled {
            return;
        }
        let Some(state) = state else {
            self.previous.remove(entity);
            return;
        };
        if !self.previous.contains_key(entity) && self.previous.len() >= crate::config::MAX_ENTITIES
        {
            return;
        }
        let previous = self.previous.insert(entity.to_owned(), state.to_owned());
        let domain = entity.split_once('.').map_or("", |(domain, _)| domain);
        if !live
            || previous.as_deref().is_none_or(|previous| previous == state)
            || !matches!(
                domain,
                "switch" | "input_boolean" | "light" | "fan" | "binary_sensor"
            )
            || domain == "binary_sensor" && state != "on"
            || self
                .last
                .get(entity)
                .is_some_and(|last| now.duration_since(*last) < Duration::from_secs(60))
            || self.pending.len() >= 16
        {
            return;
        }
        self.last
            .retain(|_, last| now.duration_since(*last) < Duration::from_secs(60));
        if !self.last.contains_key(entity) && self.last.len() >= crate::config::MAX_ENTITIES {
            return;
        }
        self.sequence = self.sequence.wrapping_add(1);
        self.last.insert(entity.to_owned(), now);
        self.pending.push_back(json!({"type":"notification","id":format!("ha-change-{}",self.sequence),
            "title":"Home Control","body":format!("{}: {}", bounded(title,128),bounded(&state.to_uppercase(),256))}));
    }

    pub fn clear(&mut self) {
        self.previous.clear();
        self.pending.clear();
        // Retain only a bounded cooldown set; reconnect cannot announce its baseline.
        if self.last.len() > crate::config::MAX_ENTITIES {
            self.last.clear();
        }
    }

    pub fn next(&self) -> Option<&Value> {
        self.pending.front()
    }
    pub fn sent(&mut self) {
        self.pending.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_suppression_domains_on_only_and_per_entity_cooldown_match_legacy() {
        let mut notifications = Notifications {
            enabled: true,
            ..Notifications::default()
        };
        let now = Instant::now();
        for entity in ["switch.a", "binary_sensor.b", "fan.c", "sensor.d"] {
            notifications.observe_at(entity, "Name", Some("off"), false, now);
        }
        assert!(notifications.next().is_none());
        for entity in ["switch.a", "binary_sensor.b", "fan.c", "sensor.d"] {
            notifications.observe_at(entity, "Name", Some("on"), true, now);
        }
        assert_eq!(notifications.pending.len(), 3);
        assert_eq!(notifications.pending[0]["body"], "Name: ON");
        notifications.pending.clear();
        notifications.observe_at(
            "switch.a",
            "Name",
            Some("off"),
            true,
            now + Duration::from_secs(59),
        );
        notifications.observe_at(
            "binary_sensor.b",
            "Name",
            Some("off"),
            true,
            now + Duration::from_secs(61),
        );
        assert!(notifications.next().is_none());
        notifications.observe_at(
            "switch.a",
            "Name",
            Some("on"),
            true,
            now + Duration::from_secs(60),
        );
        assert_eq!(notifications.pending.len(), 1);
        notifications.clear();
        notifications.observe_at(
            "switch.a",
            "Name",
            Some("off"),
            true,
            now + Duration::from_secs(120),
        );
        assert!(notifications.next().is_none());
    }
}
