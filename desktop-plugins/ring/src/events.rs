use crate::{config::Configuration, media::Snapshot};
use inverter_camera_common::{
    labels, title, valid_identity, Provider, MAX_CACHE_ENTRIES, MAX_PAYLOAD_BYTES,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    time::{Duration, Instant},
};

const COOLDOWN: Duration = Duration::from_secs(20);

pub fn valid_ring_identity(value: &str) -> bool {
    valid_identity(value) && !matches!(value, "." | "..") && !value.contains(['%', '\\'])
}

pub fn valid_camera_key(value: &str) -> bool {
    value.split_once('/').is_some_and(|(location, device)| {
        valid_ring_identity(location) && valid_ring_identity(device)
    })
}

pub struct Events {
    seen: HashMap<String, Instant>,
    labels: BTreeMap<String, String>,
    snapshot: Option<Snapshot>,
}

impl Events {
    pub fn new(config: &Configuration) -> Result<Self, &'static str> {
        Ok(Self {
            seen: HashMap::new(),
            labels: labels(config.values.camera_labels.as_deref(), valid_camera_key)?,
            snapshot: Snapshot::from_configuration(config)?,
        })
    }
}

impl Provider for Events {
    fn frames(
        &mut self,
        topic: &str,
        payload: &[u8],
        _retained: bool,
        now: Instant,
        _unix_seconds: u64,
    ) -> Vec<Value> {
        if payload.len() > MAX_PAYLOAD_BYTES {
            return vec![];
        }
        let parts: Vec<_> = topic.split('/').collect();
        let ["ring", location, "camera", device, event, "state"] = parts.as_slice() else {
            return vec![];
        };
        if !valid_ring_identity(location)
            || !valid_ring_identity(device)
            || !matches!(*event, "motion" | "ding")
        {
            return vec![];
        }
        let Ok(payload) = std::str::from_utf8(payload) else {
            return vec![];
        };
        if !matches!(
            payload.trim().to_ascii_uppercase().as_str(),
            "ON" | "TRUE" | "1"
        ) {
            return vec![];
        }
        let camera = format!("{location}/{device}");
        let key = format!("{camera}/{event}");
        self.seen
            .retain(|_, last| now.saturating_duration_since(*last) < COOLDOWN);
        // Ring's fixed cooldown does not extend when suppressed repeats arrive.
        if self.seen.contains_key(&key) || self.seen.len() >= MAX_CACHE_ENTRIES {
            return vec![];
        }
        let cooldown_id = format!("ring-{:x}", Sha256::digest(key.as_bytes()));
        self.seen.insert(key, now);
        let name = self.labels.get(&camera).cloned().unwrap_or_else(|| {
            format!(
                "{} ({device})",
                if *event == "ding" { "Ding" } else { "Motion" }
            )
        });
        let title = title("Ring", &name);
        let episode = uuid::Uuid::new_v4();
        let clip = self.snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .url(location, device, event)
                .map(|url| (snapshot, url))
        });
        let body = if clip.is_some() {
            "Camera motion clip available"
        } else {
            "Motion started"
        };
        let mut frames = vec![
            json!({"type":"notification","id":format!("ring-{episode}"),"title":title,"body":body}),
        ];
        if let Some((snapshot, url)) = clip {
            // The host also notifies for accepted media. Sharing the episode ID
            // suppresses that duplicate without coupling notice delivery to media admission.
            frames.push(json!({"type":"http_video","id":format!("ring-{episode}"),"url":url,"title":title,"media_kind":snapshot.kind,"cooldown_id":cooldown_id}));
        }
        frames
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn events() -> Events {
        Events::new(
            &serde_json::from_value(
                json!({"revision":"test","values":{"mqtt_host":"localhost"},"secrets":{}}),
            )
            .unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn fixed_cooldown_does_not_extend_and_event_location_identities_are_independent() {
        let mut events = events();
        let now = Instant::now();
        let topic = "ring/home/camera/front/motion/state";
        let first = events.frames(topic, b"ON", false, now, 0);
        assert_eq!(first.len(), 1);
        assert!(events
            .frames(topic, b"ON", false, now + Duration::from_secs(19), 0)
            .is_empty());
        assert_eq!(
            events
                .frames("ring/home/camera/front/ding/state", b"1", false, now, 0)
                .len(),
            1
        );
        assert_eq!(
            events
                .frames(
                    "ring/other/camera/front/motion/state",
                    b"true",
                    false,
                    now,
                    0
                )
                .len(),
            1
        );
        let next = events.frames(topic, b"ON", false, now + COOLDOWN, 0);
        assert_eq!(next.len(), 1);
        assert_ne!(first[0]["id"], next[0]["id"]);
    }
    #[test]
    fn only_exact_on_events_are_accepted_and_retained_behavior_is_preserved() {
        let now = Instant::now();
        for topic in [
            "ring/home/camera/front/motion/state/extra",
            "ring/home/camera/front/battery/state",
            "ring/home/camera/../ding/state",
            "kerberos/agent/front",
            "frigate/events",
            "ring//camera/front/motion/state",
        ] {
            assert!(events().frames(topic, b"ON", false, now, 0).is_empty());
        }
        for payload in [b"OFF".as_slice(), b"false", b"0", b"motion", &[0xff]] {
            assert!(events()
                .frames(
                    "ring/home/camera/front/motion/state",
                    payload,
                    false,
                    now,
                    0
                )
                .is_empty());
        }
        assert_eq!(
            events()
                .frames("ring/home/camera/front/motion/state", b" on ", true, now, 0)
                .len(),
            1
        );
    }
    #[test]
    fn snapshot_is_typed_and_only_explicit_camera_label_is_used() {
        let config=serde_json::from_value(json!({"revision":"test","values":{"mqtt_host":"localhost","camera_labels":r#"{"home/front":"Entrance"}"#,"snapshot_base_url":"https://ha.test/api","snapshot_media_kind":"png"},"secrets":{"snapshot_url_template":"https://ha.test/api/{device_id}?event={event}","snapshot_bearer_token":"private-token"}})).unwrap();
        let mut events = Events::new(&config).unwrap();
        let frames = events.frames(
            "ring/home/camera/front/ding/state",
            b"ON",
            false,
            Instant::now(),
            0,
        );
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0]["title"], "Ring Entrance camera motion detected");
        assert_eq!(frames[1]["media_kind"], "png");
        assert_eq!(frames[0]["id"], frames[1]["id"]);
        assert_eq!(frames[1]["url"], "https://ha.test/api/front?event=ding");
        assert!(!serde_json::to_string(&frames)
            .unwrap()
            .contains("private-token"));
        let motion = events.frames(
            "ring/home/camera/front/motion/state",
            b"ON",
            false,
            Instant::now(),
            0,
        );
        assert_eq!(motion[1]["title"], frames[1]["title"]);
        assert_ne!(motion[1]["cooldown_id"], frames[1]["cooldown_id"]);
    }
    #[test]
    fn active_identity_cache_is_bounded() {
        let mut events = events();
        let now = Instant::now();
        for i in 0..MAX_CACHE_ENTRIES {
            assert_eq!(
                events
                    .frames(
                        &format!("ring/home/camera/{i}/motion/state"),
                        b"ON",
                        false,
                        now,
                        0
                    )
                    .len(),
                1
            );
        }
        assert!(events
            .frames("ring/home/camera/extra/motion/state", b"ON", false, now, 0)
            .is_empty());
        assert_eq!(
            events
                .frames(
                    "ring/home/camera/extra/motion/state",
                    b"ON",
                    false,
                    now + COOLDOWN,
                    0
                )
                .len(),
            1
        );
    }
}
