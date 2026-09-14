use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub const MAX_PAYLOAD_BYTES: usize = 16 * 1024;
const ID_TTL: Duration = Duration::from_secs(600);
const CAMERA_COOLDOWN: Duration = Duration::from_secs(45);
const MAX_CACHE_ENTRIES: usize = 512;

#[derive(Deserialize)]
struct FrigateEvent {
    #[serde(rename = "type")]
    kind: String,
    after: EventDetails,
}
#[derive(Deserialize)]
struct EventDetails {
    id: String,
    camera: String,
    start_time: Option<f64>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Motion {
    pub id: String,
    pub title: String,
}

#[derive(Default)]
pub struct MotionEvents {
    ids: HashMap<String, Instant>,
    cameras: HashMap<String, Instant>,
}

impl MotionEvents {
    pub fn parse(
        &mut self,
        payload: &[u8],
        retained: bool,
        now: Instant,
        unix_seconds: f64,
    ) -> Option<Motion> {
        if retained || payload.len() > MAX_PAYLOAD_BYTES {
            return None;
        }
        let event: FrigateEvent = serde_json::from_slice(payload).ok()?;
        let after = event.after;
        if event.kind != "new"
            || after.id.is_empty()
            || after.id.len() > 128
            || after.camera.trim().is_empty()
            || after.camera.len() > 128
        {
            return None;
        }
        // Retained events are never live motion. Older timestamped replays are
        // also ignored; legacy publishers without start_time remain supported.
        if after.start_time.is_some_and(|start| {
            !start.is_finite() || start < unix_seconds - 600.0 || start > unix_seconds + 60.0
        }) {
            return None;
        }
        let camera: String = after.camera.chars().filter(|ch| !ch.is_control()).collect();
        let camera = camera.split_whitespace().collect::<Vec<_>>().join(" ");
        if camera.is_empty() {
            return None;
        }
        self.ids
            .retain(|_, seen| now.saturating_duration_since(*seen) < ID_TTL);
        self.cameras
            .retain(|_, seen| now.saturating_duration_since(*seen) < CAMERA_COOLDOWN);
        if self.ids.contains_key(&after.id)
            || self.cameras.contains_key(&after.camera)
            || self.ids.len() >= MAX_CACHE_ENTRIES
            || self.cameras.len() >= MAX_CACHE_ENTRIES
        {
            return None;
        }
        self.ids.insert(after.id.clone(), now);
        self.cameras.insert(after.camera, now);
        let mut chars = camera.chars();
        let capitalized = chars.next()?.to_uppercase().collect::<String>() + chars.as_str();
        let mut name = String::new();
        let max_name = 128 - "Frigate  camera motion detected".len();
        for ch in capitalized.chars() {
            if name.len() + ch.len_utf8() > max_name {
                break;
            }
            name.push(ch);
        }
        Some(Motion {
            id: format!("frigate-{:x}", Sha256::digest(after.id.as_bytes())),
            title: format!("Frigate {name} camera motion detected"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn event(id: &str, camera: &str) -> Vec<u8> {
        serde_json::to_vec(&json!({"type":"new","after":{"id":id,"camera":camera}})).unwrap()
    }
    #[test]
    fn notification_shape_and_legacy_cooldowns() {
        let now = Instant::now();
        let mut events = MotionEvents::default();
        let first = events
            .parse(&event("event-a", "front"), false, now, 1000.0)
            .unwrap();
        assert_eq!(first.title, "Frigate Front camera motion detected");
        assert!(crate::config::valid_token(&first.id));
        assert!(events
            .parse(&event("event-a", "other"), false, now, 1000.0)
            .is_none());
        assert!(events
            .parse(
                &event("event-b", "front"),
                false,
                now + Duration::from_secs(44),
                1000.0
            )
            .is_none());
        assert!(events
            .parse(&event("event-c", "back"), false, now, 1000.0)
            .is_some());
        assert!(events
            .parse(
                &event("event-b", "front"),
                false,
                now + CAMERA_COOLDOWN,
                1000.0
            )
            .is_some());
        assert!(events
            .parse(&event("event-a", "front"), false, now + ID_TTL, 1000.0)
            .is_some());
    }
    #[test]
    fn ignores_retained_old_future_non_start_and_malformed_events() {
        let now = Instant::now();
        let mut events = MotionEvents::default();
        assert!(events
            .parse(&event("a", "front"), true, now, 1000.0)
            .is_none());
        for kind in ["end", "update"] {
            let value = json!({"type":kind,"after":{"id":"a","camera":"front","has_clip":true}});
            assert!(events
                .parse(&serde_json::to_vec(&value).unwrap(), false, now, 1000.0)
                .is_none());
        }
        for start in [399.0, 1061.0] {
            let value =
                json!({"type":"new","after":{"id":"a","camera":"front","start_time":start}});
            assert!(events
                .parse(&serde_json::to_vec(&value).unwrap(), false, now, 1000.0)
                .is_none());
        }
        for value in [
            b"invalid".to_vec(),
            event("", "front"),
            event("a", ""),
            vec![b' '; MAX_PAYLOAD_BYTES + 1],
        ] {
            assert!(events.parse(&value, false, now, 1000.0).is_none());
        }
        assert!(events
            .parse(&event("a", "front"), false, now, 1000.0)
            .is_some());
    }
    #[test]
    fn sanitized_text_and_caches_remain_bounded() {
        let now = Instant::now();
        let mut events = MotionEvents::default();
        let motion = events
            .parse(&event("id\n", &("é".repeat(50) + "\n")), false, now, 1000.0)
            .unwrap();
        assert!(motion.title.len() <= 128 && !motion.title.chars().any(char::is_control));
        for index in 0..MAX_CACHE_ENTRIES + 10 {
            let _ = events.parse(
                &event(&format!("id-{index}"), &format!("camera-{index}")),
                false,
                now,
                1000.0,
            );
        }
        assert_eq!(events.ids.len(), MAX_CACHE_ENTRIES);
        assert!(events.cameras.len() <= MAX_CACHE_ENTRIES);
        assert!(events
            .parse(&event("fresh", "fresh"), false, now + ID_TTL, 1000.0)
            .is_some());
    }
}
