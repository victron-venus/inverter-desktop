use crate::config::Configuration;
use inverter_camera_common::{
    labels, title, valid_identity, Provider, MAX_CACHE_ENTRIES, MAX_PAYLOAD_BYTES,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    time::{Duration, Instant},
};

const SILENCE: Duration = Duration::from_secs(15);

pub struct Events {
    seen: HashMap<String, Instant>,
    labels: BTreeMap<String, String>,
    live_urls: BTreeMap<String, String>,
}

impl Events {
    pub fn new(config: &Configuration) -> Result<Self, &'static str> {
        Ok(Self {
            seen: HashMap::new(),
            labels: labels(config.values.camera_labels.as_deref(), valid_identity)?,
            live_urls: config.live_urls()?,
        })
    }

    fn motion(
        &mut self,
        topic: &str,
        payload: &[u8],
        retained: bool,
        now: Instant,
        utc_now: u64,
    ) -> Option<Value> {
        if retained || payload.len() > MAX_PAYLOAD_BYTES {
            return None;
        }
        let mut parts = topic.split('/');
        if parts.next()? != "kerberos" {
            return None;
        }
        let kind = parts.next()?;
        let identity = parts.next()?;
        if parts.next().is_some() || !valid_identity(identity) {
            return None;
        }
        let camera = match kind {
            "agent" if std::str::from_utf8(payload).ok()?.trim() == "motion" => identity.to_owned(),
            "hub" => {
                let message: Value = serde_json::from_slice(payload).ok()?;
                if ["hidden", "encrypted"].iter().any(|flag| {
                    message
                        .get(flag)
                        .is_some_and(|value| value.as_bool() != Some(false))
                }) {
                    return None;
                }
                let body = message.get("payload")?;
                if body.get("action")?.as_str()? != "motion" {
                    return None;
                }
                let camera = body.get("device_id")?.as_str()?;
                if !valid_identity(camera)
                    || message
                        .get("device_id")
                        .is_some_and(|v| v.as_str() != Some(camera))
                {
                    return None;
                }
                let timestamp = body.get("value")?.get("timestamp")?.as_u64()?;
                if timestamp == 0
                    || utc_now.saturating_sub(timestamp) > 60
                    || timestamp.saturating_sub(utc_now) > 10
                {
                    return None;
                }
                camera.to_owned()
            }
            _ => return None,
        };
        self.seen
            .retain(|_, seen| now.saturating_duration_since(*seen) < SILENCE);
        if let Some(last) = self.seen.get_mut(&camera) {
            // Continuous movement extends the same episode, including suppressed repeats.
            *last = now;
            return None;
        }
        if self.seen.len() >= MAX_CACHE_ENTRIES {
            return None;
        }
        self.seen.insert(camera.clone(), now);
        let name = self
            .labels
            .get(&camera)
            .cloned()
            .unwrap_or_else(|| friendly_name(&camera));
        let mut frame = json!({"type":"notification","id":format!("kerberos-{}",uuid::Uuid::new_v4()),"title":title("Kerberos", &name),"body":"Motion started"});
        if self.live_urls.contains_key(&camera) {
            frame["live_view_id"] = json!(camera);
            frame["body"] = json!("Motion started. Click to view the live camera.");
        }
        Some(frame)
    }
}

fn friendly_name(identity: &str) -> String {
    match identity.to_ascii_lowercase().as_str() {
        "front_camera" => "Front".into(),
        "garage_camera" => "Garage".into(),
        "tenants_camera" => "Tenants".into(),
        "porch_camera" => "Porch".into(),
        _ => identity
            .split(['_', '-'])
            .filter(|s| !s.is_empty())
            .map(|s| {
                let mut chars = s.chars();
                chars
                    .next()
                    .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

impl Provider for Events {
    fn frames(
        &mut self,
        topic: &str,
        payload: &[u8],
        retained: bool,
        now: Instant,
        unix_seconds: u64,
    ) -> Vec<Value> {
        self.motion(topic, payload, retained, now, unix_seconds)
            .into_iter()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn events() -> Events {
        Events::new(
            &serde_json::from_value(
                json!({"revision":"fixture","values":{"mqtt_host":"localhost"},"secrets":{}}),
            )
            .unwrap(),
        )
        .unwrap()
    }
    fn hub(camera: &str, timestamp: u64) -> Vec<u8> {
        serde_json::to_vec(&json!({"device_id":camera,"hidden":false,"encrypted":false,"payload":{"action":"motion","device_id":camera,"value":{"timestamp":timestamp}}})).unwrap()
    }

    #[test]
    fn agent_hub_share_silence_episodes_and_repeats_extend_quiet_boundary() {
        let mut events = events();
        let start = Instant::now();
        let first = events
            .motion("kerberos/agent/front_camera", b"motion", false, start, 1000)
            .unwrap();
        assert_eq!(first["title"], "Kerberos Front camera motion detected");
        assert!(first.get("live_view_id").is_none());
        assert!(events
            .motion(
                "kerberos/hub/hub",
                &hub("front_camera", 1000),
                false,
                start + Duration::from_secs(14),
                1000
            )
            .is_none());
        assert!(events
            .motion(
                "kerberos/agent/front_camera",
                b"motion",
                false,
                start + Duration::from_secs(28),
                1000
            )
            .is_none());
        assert!(events
            .motion(
                "kerberos/agent/front_camera",
                b"motion",
                false,
                start + Duration::from_secs(42),
                1000
            )
            .is_none());
        let second = events
            .motion(
                "kerberos/agent/front_camera",
                b"motion",
                false,
                start + Duration::from_secs(57),
                1000,
            )
            .unwrap();
        assert_ne!(
            first["id"], second["id"],
            "host ID cache must not suppress a later episode"
        );
    }

    #[test]
    fn retained_invalid_and_stale_hub_events_cannot_extend_silence() {
        let start = Instant::now();
        for timestamp in [0, 939, 1011] {
            let mut events = events();
            assert!(events
                .motion(
                    "kerberos/hub/hub",
                    &hub("front", timestamp),
                    false,
                    start,
                    1000
                )
                .is_none());
            assert!(events
                .motion("kerberos/agent/front", b"motion", false, start, 1000)
                .is_some());
        }
        for timestamp in [940, 1010] {
            assert!(events()
                .motion(
                    "kerberos/hub/hub",
                    &hub("front", timestamp),
                    false,
                    start,
                    1000
                )
                .is_some());
        }
        let mut events = events();
        events
            .motion("kerberos/agent/front", b"motion", false, start, 1000)
            .unwrap();
        assert!(events
            .motion(
                "kerberos/agent/front",
                b"motion",
                true,
                start + Duration::from_secs(14),
                1000
            )
            .is_none());
        assert!(events
            .motion(
                "kerberos/agent/front",
                b"motion",
                false,
                start + SILENCE,
                1000
            )
            .is_some());
    }

    #[test]
    fn native_namespaces_never_accept_clip_urls_or_conflicting_envelopes() {
        let start = Instant::now();
        for payload in [
            b"https://example.test/clip.mp4".as_slice(),
            br#"{"agent_name":"a","video_url":"https://example.test/a"}"#,
            b"Motion",
            b"",
            &[0xff],
        ] {
            assert!(events()
                .motion("kerberos/agent/front", payload, false, start, 1000)
                .is_none());
        }
        for topic in [
            "kerberos/agent/",
            "kerberos/agent/front/extra",
            "kerberos/agent/+",
            "frigate/events",
            "ring/site/camera/front/motion/state",
        ] {
            assert!(events()
                .motion(topic, b"motion", false, start, 1000)
                .is_none());
        }
        for (field, value) in [
            ("hidden", json!(true)),
            ("encrypted", json!("false")),
            ("device_id", json!("other")),
        ] {
            let mut payload: Value = serde_json::from_slice(&hub("front", 1000)).unwrap();
            payload[field] = value;
            assert!(events()
                .motion(
                    "kerberos/hub/hub",
                    &serde_json::to_vec(&payload).unwrap(),
                    false,
                    start,
                    1000
                )
                .is_none());
        }
        assert!(events()
            .motion(
                "kerberos/agent/front",
                &vec![b' '; MAX_PAYLOAD_BYTES + 1],
                false,
                start,
                1000
            )
            .is_none());
    }

    #[test]
    fn configured_exact_live_identity_is_emitted_without_private_url() {
        let config: Configuration=serde_json::from_value(json!({"revision":"fixture","values":{"mqtt_host":"localhost","camera_labels":r#"{"front_camera":"Entrance"}"#},"secrets":{"camera_live_urls":r#"{"front_camera":"https://camera.test/live?token=private#view"}"#}})).unwrap();
        config.validate().unwrap();
        let mut events = Events::new(&config).unwrap();
        let now = Instant::now();
        let frame = events
            .motion("kerberos/agent/front_camera", b"motion", false, now, 1000)
            .unwrap();
        assert_eq!(frame["live_view_id"], "front_camera");
        assert_eq!(frame["title"], "Kerberos Entrance camera motion detected");
        assert!(!frame.to_string().contains("private"));
        assert!(events
            .motion("kerberos/agent/Front_camera", b"motion", false, now, 1000)
            .unwrap()
            .get("live_view_id")
            .is_none());
    }

    #[test]
    fn cache_is_bounded_and_old_entries_expire() {
        let mut events = events();
        let now = Instant::now();
        for i in 0..MAX_CACHE_ENTRIES {
            assert!(events
                .motion(&format!("kerberos/agent/{i}"), b"motion", false, now, 1000)
                .is_some());
        }
        assert!(events
            .motion("kerberos/agent/next", b"motion", false, now, 1000)
            .is_none());
        assert_eq!(events.seen.len(), MAX_CACHE_ENTRIES);
        assert!(events
            .motion("kerberos/agent/next", b"motion", false, now + SILENCE, 1000)
            .is_some());
    }
}
