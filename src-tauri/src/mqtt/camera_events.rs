//! Desktop camera MQTT adapters. No part of this module is compiled for mobile.
use super::{entity_friendly_name, HaEntityEntry};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraEvent {
    pub agent_name: String,
    pub video_url: String,
    pub timestamp: Option<String>,
}

/// Outcome of parsing a camera MQTT payload.
///
/// Frigate `type=new` is notify-only (no clip yet). Kerberos, raw URLs, and
/// Frigate `end`+`has_clip` open a clip window via [`CameraEvent`].
#[derive(Debug, Clone)]
pub(super) enum CameraMqttAction {
    /// Frigate motion start: OS notification only; do not emit `camera-event`.
    StartNotify { agent_name: String },
    /// Notify + emit `camera-event` (clip available).
    OpenClip(CameraEvent),
}

fn match_mqtt_topic(topic: &str, pattern: &str) -> bool {
    if pattern == topic || pattern == "#" {
        return true;
    }
    let t_parts: Vec<&str> = topic.split('/').collect();
    let p_parts: Vec<&str> = pattern.split('/').collect();

    if pattern.ends_with("/#") {
        let prefix_len = p_parts.len() - 1;
        if t_parts.len() < prefix_len {
            return false;
        }
        return p_parts[..prefix_len]
            .iter()
            .zip(t_parts.iter())
            .all(|(p, t)| *p == "+" || *p == *t);
    }

    // Very basic MQTT wildcard matching for +
    if t_parts.len() != p_parts.len() {
        return false;
    }
    for (t, p) in t_parts.iter().zip(p_parts.iter()) {
        if *p != "+" && *p != *t {
            return false;
        }
    }
    true
}

/// Split `camera_topic` on `;`, trim, drop empties.
/// Supports e.g. `kerberos/desktop/events;frigate/events`.
pub(super) fn split_camera_topics(camera_topic: &Option<String>) -> Vec<String> {
    camera_topic
        .as_deref()
        .unwrap_or("")
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

pub(super) fn camera_topic_matches(topic: &str, camera_topic: &Option<String>) -> bool {
    split_camera_topics(camera_topic)
        .iter()
        .any(|pattern| match_mqtt_topic(topic, pattern))
}

fn capitalize_agent_name(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

/// TTL for remembering Frigate event ids so the same id is never opened twice.
const FRIGATE_EVENT_ID_TTL: Duration = Duration::from_secs(10 * 60);
/// Per-camera quiet period after a successful Frigate clip open. Overlapping
/// sibling events (different ids, same walk-by) typically end within 1–2s.
const FRIGATE_CAMERA_COOLDOWN: Duration = Duration::from_secs(45);

/// Process-local Frigate clip open memory (event-id TTL + per-camera cooldown).
struct FrigateClipDedupeState {
    seen_ids: HashMap<String, Instant>,
    camera_last_open: HashMap<String, Instant>,
}

impl FrigateClipDedupeState {
    fn new() -> Self {
        Self {
            seen_ids: HashMap::new(),
            camera_last_open: HashMap::new(),
        }
    }

    fn prune(&mut self, now: Instant) {
        self.seen_ids
            .retain(|_, seen_at| now.duration_since(*seen_at) < FRIGATE_EVENT_ID_TTL);
        self.camera_last_open
            .retain(|_, last| now.duration_since(*last) < FRIGATE_CAMERA_COOLDOWN);
    }
}

static FRIGATE_CLIP_DEDUPE: std::sync::LazyLock<Mutex<FrigateClipDedupeState>> =
    std::sync::LazyLock::new(|| Mutex::new(FrigateClipDedupeState::new()));

/// Whether a Frigate event should fire (clip open or start notify).
///
/// Suppresses (1) the same event `id` within [`FRIGATE_EVENT_ID_TTL`] and
/// (2) any further admits for the same `camera` within [`FRIGATE_CAMERA_COOLDOWN`]
/// after a successful admit. On `true`, records id + camera time.
fn frigate_dedupe_should_admit(
    state: &mut FrigateClipDedupeState,
    id: &str,
    camera: &str,
    now: Instant,
    kind: &str,
) -> bool {
    state.prune(now);

    if let Some(seen_at) = state.seen_ids.get(id) {
        if now.duration_since(*seen_at) < FRIGATE_EVENT_ID_TTL {
            log::info!("Frigate {kind} skipped: duplicate event id (id={id}, camera={camera})");
            return false;
        }
    }

    if let Some(last) = state.camera_last_open.get(camera) {
        if now.duration_since(*last) < FRIGATE_CAMERA_COOLDOWN {
            log::info!("Frigate {kind} skipped: camera cooldown (camera={camera}, id={id})");
            return false;
        }
    }

    state.seen_ids.insert(id.to_string(), now);
    state.camera_last_open.insert(camera.to_string(), now);
    true
}

/// Whether a Frigate `end`+`has_clip` event should open a clip window.
fn frigate_clip_should_open(
    state: &mut FrigateClipDedupeState,
    id: &str,
    camera: &str,
    now: Instant,
) -> bool {
    frigate_dedupe_should_admit(state, id, camera, now, "clip")
}

/// Whether a Frigate `type=new` event should fire a start-of-motion notification.
fn frigate_start_should_notify(
    state: &mut FrigateClipDedupeState,
    id: &str,
    camera: &str,
    now: Instant,
) -> bool {
    frigate_dedupe_should_admit(state, id, camera, now, "start notify")
}

/// Process-local Frigate start-notify memory (separate from clip opens so a
/// start notify does not suppress the later `end`+`has_clip` window).
static FRIGATE_START_NOTIFY_DEDUPE: std::sync::LazyLock<Mutex<FrigateClipDedupeState>> =
    std::sync::LazyLock::new(|| Mutex::new(FrigateClipDedupeState::new()));

/// Serializes tests that mutate the process-global [`FRIGATE_CLIP_DEDUPE`].
/// Rust's default test harness runs cases in parallel; without this gate,
/// `reset_frigate_clip_dedupe_for_tests` and successful opens race.
#[cfg(test)]
static FRIGATE_DEDUPE_TEST_SERIAL: Mutex<()> = Mutex::new(());

#[cfg(test)]
fn reset_frigate_clip_dedupe_for_tests() {
    for lock in [&FRIGATE_CLIP_DEDUPE, &FRIGATE_START_NOTIFY_DEDUPE] {
        let mut guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        *guard = FrigateClipDedupeState::new();
    }
}

/// Parse a Frigate MQTT `frigate/events` JSON payload.
///
/// - `type == "new"`: start-of-motion OS notification (no clip window).
/// - `type == "end"` and `has_clip`: open clip after Frigate finishes the event.
///
/// Early `update` messages (including `has_clip` false→true) are skipped.
/// Note: `has_clip` means recording is expected, not that segments are on disk
/// yet — Frigate often returns HTTP 400 ("No recordings found…") until the
/// ~10s segment lands; download retries cover that residual race.
/// Returns `None` when the payload is not Frigate-shaped, should be skipped,
/// or (for clip open) `frigate_base_url` is missing/empty.
fn parse_frigate_camera_event(
    payload: &str,
    frigate_base_url: Option<&str>,
) -> Option<CameraMqttAction> {
    let v: serde_json::Value = serde_json::from_str(payload).ok()?;
    let event_type = v.get("type")?.as_str()?;
    let after = v.get("after")?;
    // Require Frigate-shaped fields so Kerberos/raw payloads don't match.
    let id = after.get("id")?.as_str()?;
    let camera = after.get("camera")?.as_str()?;
    if id.is_empty() || camera.is_empty() {
        return None;
    }
    let agent_name = format!("Frigate {}", capitalize_agent_name(camera));

    if event_type == "new" {
        {
            let mut dedupe = FRIGATE_START_NOTIFY_DEDUPE
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if !frigate_start_should_notify(&mut dedupe, id, camera, Instant::now()) {
                return None;
            }
        }
        log::info!("Frigate motion start notify (camera={camera}, id={id})");
        return Some(CameraMqttAction::StartNotify { agent_name });
    }

    let has_clip = after
        .get("has_clip")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    // Wait for event end so the clip is more likely fully available.
    let should_open = event_type == "end" && has_clip;
    if !should_open {
        return None;
    }
    let Some(base_raw) = frigate_base_url.map(str::trim).filter(|s| !s.is_empty()) else {
        log::warn!(
            "Frigate camera event skipped: frigate_base_url is not configured (camera={camera}, id={id})"
        );
        return None;
    };

    {
        let mut dedupe = FRIGATE_CLIP_DEDUPE
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if !frigate_clip_should_open(&mut dedupe, id, camera, Instant::now()) {
            return None;
        }
    }

    let base = base_raw.trim_end_matches('/');
    Some(CameraMqttAction::OpenClip(CameraEvent {
        agent_name,
        video_url: format!("{base}/api/events/{id}/clip.mp4"),
        timestamp: None,
    }))
}

/// Parse `ring/<location_id>/camera/<device_id>/(motion|ding)/state`.
fn parse_ring_camera_topic(topic: &str) -> Option<(&str, &str, &str)> {
    let parts: Vec<&str> = topic.split('/').collect();
    if parts.len() != 6 {
        return None;
    }
    if parts[0] != "ring" || parts[2] != "camera" || parts[5] != "state" {
        return None;
    }
    let event = match parts[4] {
        "motion" | "ding" => parts[4],
        _ => return None,
    };
    let location_id = parts[1];
    let device_id = parts[3];
    if location_id.is_empty() || device_id.is_empty() {
        return None;
    }
    Some((location_id, device_id, event))
}

fn ring_payload_is_on(payload: &str) -> bool {
    matches!(
        payload.trim().to_ascii_uppercase().as_str(),
        "ON" | "TRUE" | "1"
    )
}

/// Prefer a single Ring binary_sensor friendly name from HA; otherwise label by event + device id.
fn ring_agent_name(
    device_id: &str,
    event: &str,
    ha_entity_states: &Option<Arc<Mutex<HashMap<String, HaEntityEntry>>>>,
) -> String {
    let kind_label = if event == "ding" { "Ding" } else { "Motion" };
    if let Some(states) = ha_entity_states {
        if let Ok(guard) = states.lock() {
            let suffix = if event == "ding" { "ding" } else { "motion" };
            let mut matches: Vec<String> = Vec::new();
            for (id, entry) in guard.iter() {
                if !id.starts_with("binary_sensor.") || !id.ends_with(suffix) {
                    continue;
                }
                let is_ring = entry
                    .attributes
                    .as_ref()
                    .and_then(|a| a.get("attribution"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_ascii_lowercase().contains("ring"))
                    .unwrap_or(false);
                if !is_ring {
                    continue;
                }
                if let Some(name) = entity_friendly_name(entry) {
                    matches.push(name);
                }
            }
            if matches.len() == 1 {
                let name = &matches[0];
                let base = name
                    .trim_end_matches(" Motion")
                    .trim_end_matches(" Ding")
                    .trim_end_matches(" motion")
                    .trim_end_matches(" ding");
                return format!("Ring {base}");
            }
        }
    }
    format!("Ring {kind_label} ({device_id})")
}

const RING_CAMERA_COOLDOWN: Duration = Duration::from_secs(20);

struct RingDedupeState {
    camera_last_open: HashMap<String, Instant>,
}

impl RingDedupeState {
    fn new() -> Self {
        Self {
            camera_last_open: HashMap::new(),
        }
    }

    fn prune(&mut self, now: Instant) {
        self.camera_last_open
            .retain(|_, last| now.duration_since(*last) < RING_CAMERA_COOLDOWN);
    }
}

static RING_DEDUPE: std::sync::LazyLock<Mutex<RingDedupeState>> =
    std::sync::LazyLock::new(|| Mutex::new(RingDedupeState::new()));

#[cfg(test)]
static RING_DEDUPE_TEST_SERIAL: Mutex<()> = Mutex::new(());

#[cfg(test)]
fn reset_ring_dedupe_for_tests() {
    let mut guard = RING_DEDUPE.lock().unwrap_or_else(|e| e.into_inner());
    *guard = RingDedupeState::new();
}

fn ring_should_admit(device_id: &str, event: &str, now: Instant) -> bool {
    let mut state = RING_DEDUPE.lock().unwrap_or_else(|e| e.into_inner());
    state.prune(now);
    let key = format!("{device_id}:{event}");
    if let Some(last) = state.camera_last_open.get(&key) {
        if now.duration_since(*last) < RING_CAMERA_COOLDOWN {
            log::info!("Ring {event} skipped: camera cooldown (device={device_id})");
            return false;
        }
    }
    state.camera_last_open.insert(key, now);
    true
}

fn resolve_ring_snapshot_url(
    template: &str,
    location_id: &str,
    device_id: &str,
    event: &str,
) -> String {
    template
        .replace("{location_id}", location_id)
        .replace("{device_id}", device_id)
        .replace("{event}", event)
}

/// Parse Ring-MQTT motion/ding state payloads (ON/OFF).
///
/// Opens a snapshot window when `ring_snapshot_url_template` is set (HTTP/HTTPS
/// only — the camera Webview cannot play RTSP). Otherwise notifies only.
fn parse_ring_camera_event(
    topic: &str,
    payload: &str,
    ring_snapshot_url_template: Option<&str>,
    ha_entity_states: &Option<Arc<Mutex<HashMap<String, HaEntityEntry>>>>,
) -> Option<CameraMqttAction> {
    let (location_id, device_id, event) = parse_ring_camera_topic(topic)?;
    if !ring_payload_is_on(payload) {
        return None;
    }
    if !ring_should_admit(device_id, event, Instant::now()) {
        return None;
    }
    let agent_name = ring_agent_name(device_id, event, ha_entity_states);
    let Some(template_raw) = ring_snapshot_url_template
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        log::info!("Ring {event} notify-only (no ring_snapshot_url_template; device={device_id})");
        return Some(CameraMqttAction::StartNotify { agent_name });
    };
    let video_url = resolve_ring_snapshot_url(template_raw, location_id, device_id, event);
    if video_url.trim().is_empty() {
        return Some(CameraMqttAction::StartNotify { agent_name });
    }
    log::info!("Ring {event} open snapshot (device={device_id}, url={video_url})");
    Some(CameraMqttAction::OpenClip(CameraEvent {
        agent_name,
        video_url,
        timestamp: None,
    }))
}

/// Resolve a camera MQTT payload to a [`CameraMqttAction`].
/// Ring-MQTT state topics first; Kerberos JSON (`agent_name` + `video_url`);
/// Frigate next; raw HTTP(S) URL last.
pub(super) fn parse_camera_mqtt_payload(
    topic: &str,
    payload: &str,
    frigate_base_url: &Option<String>,
    ring_snapshot_url_template: &Option<String>,
    ha_entity_states: &Option<Arc<Mutex<HashMap<String, HaEntityEntry>>>>,
) -> Option<CameraMqttAction> {
    if parse_ring_camera_topic(topic).is_some() {
        return parse_ring_camera_event(
            topic,
            payload,
            ring_snapshot_url_template.as_deref(),
            ha_entity_states,
        );
    }
    if let Ok(mut ev) = serde_json::from_str::<CameraEvent>(payload) {
        if !ev.video_url.trim().is_empty() {
            if ev.agent_name.trim().is_empty() {
                ev.agent_name = "Camera".to_string();
            }
            return Some(CameraMqttAction::OpenClip(ev));
        }
    }
    // Peek: Frigate-shaped JSON should not fall through to raw-URL.
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(payload) {
        if v.get("type").is_some() && v.get("after").is_some() {
            return parse_frigate_camera_event(payload, frigate_base_url.as_deref());
        }
    }
    let trimmed = payload.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return Some(CameraMqttAction::OpenClip(CameraEvent {
            agent_name: "Camera".to_string(),
            video_url: trimmed.to_string(),
            timestamp: None,
        }));
    }
    None
}

#[cfg(test)]
mod camera_topic_tests {
    use super::*;

    #[test]
    fn split_camera_topics_semicolon() {
        let topics = split_camera_topics(&Some(
            "kerberos/desktop/events; frigate/events ; ;".to_string(),
        ));
        assert_eq!(
            topics,
            vec![
                "kerberos/desktop/events".to_string(),
                "frigate/events".to_string()
            ]
        );
        assert!(split_camera_topics(&None).is_empty());
        assert!(split_camera_topics(&Some("  ; ;".to_string())).is_empty());
    }

    #[test]
    fn camera_topic_matches_any_pattern() {
        let cfg = Some("kerberos/desktop/events;frigate/events".to_string());
        assert!(camera_topic_matches("frigate/events", &cfg));
        assert!(camera_topic_matches("kerberos/desktop/events", &cfg));
        assert!(!camera_topic_matches("other/topic", &cfg));
    }

    fn expect_open_clip(action: Option<CameraMqttAction>) -> CameraEvent {
        match action {
            Some(CameraMqttAction::OpenClip(ev)) => ev,
            other => panic!("expected OpenClip, got {other:?}"),
        }
    }

    fn expect_start_notify(action: Option<CameraMqttAction>) -> String {
        match action {
            Some(CameraMqttAction::StartNotify { agent_name }) => agent_name,
            other => panic!("expected StartNotify, got {other:?}"),
        }
    }

    #[test]
    fn parse_kerberos_camera_event_unchanged() {
        let payload = r#"{"agent_name":"Porch","video_url":"http://cam/clip.mp4","timestamp":"t"}"#;
        let ev = expect_open_clip(parse_camera_mqtt_payload(
            "kerberos/desktop/events",
            payload,
            &None,
            &None,
            &None,
        ));
        assert_eq!(ev.agent_name, "Porch");
        assert_eq!(ev.video_url, "http://cam/clip.mp4");
    }

    #[test]
    fn parse_frigate_end_with_clip() {
        let _serial = FRIGATE_DEDUPE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        reset_frigate_clip_dedupe_for_tests();
        let payload = r#"{
            "type":"end",
            "before":{"id":"abc","camera":"front","has_clip":false},
            "after":{"id":"abc","camera":"front","label":"person","has_clip":true}
        }"#;
        let base = Some("http://192.168.151.21:5005".to_string());
        let ev = expect_open_clip(parse_camera_mqtt_payload(
            "kerberos/desktop/events",
            payload,
            &base,
            &None,
            &None,
        ));
        assert_eq!(ev.agent_name, "Frigate Front");
        assert_eq!(
            ev.video_url,
            "http://192.168.151.21:5005/api/events/abc/clip.mp4"
        );
    }

    #[test]
    fn parse_frigate_new_is_start_notify_not_open_clip() {
        let _serial = FRIGATE_DEDUPE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        reset_frigate_clip_dedupe_for_tests();
        let payload = r#"{
            "type":"new",
            "before":null,
            "after":{"id":"abc","camera":"front","label":"person","has_clip":false}
        }"#;
        let base = Some("http://192.168.151.21:5005".to_string());
        let agent = expect_start_notify(parse_camera_mqtt_payload(
            "kerberos/desktop/events",
            payload,
            &base,
            &None,
            &None,
        ));
        assert_eq!(agent, "Frigate Front");
    }

    #[test]
    fn parse_frigate_new_does_not_require_base_url() {
        let _serial = FRIGATE_DEDUPE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        reset_frigate_clip_dedupe_for_tests();
        let payload = r#"{
            "type":"new",
            "after":{"id":"abc","camera":"front","has_clip":false}
        }"#;
        let agent = expect_start_notify(parse_camera_mqtt_payload(
            "kerberos/desktop/events",
            payload,
            &None,
            &None,
            &None,
        ));
        assert_eq!(agent, "Frigate Front");
    }

    #[test]
    fn parse_frigate_new_dedupes_same_id() {
        let _serial = FRIGATE_DEDUPE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        reset_frigate_clip_dedupe_for_tests();
        let payload = r#"{
            "type":"new",
            "after":{"id":"abc","camera":"front","has_clip":false}
        }"#;
        assert!(matches!(
            parse_camera_mqtt_payload("kerberos/desktop/events", payload, &None, &None, &None),
            Some(CameraMqttAction::StartNotify { .. })
        ));
        assert!(
            parse_camera_mqtt_payload("kerberos/desktop/events", payload, &None, &None, &None)
                .is_none(),
            "same Frigate event id must not start-notify twice"
        );
    }

    #[test]
    fn parse_frigate_new_then_end_still_opens_clip() {
        let _serial = FRIGATE_DEDUPE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        reset_frigate_clip_dedupe_for_tests();
        let base = Some("http://192.168.151.21:5005".to_string());
        let new_payload = r#"{
            "type":"new",
            "after":{"id":"abc","camera":"front","has_clip":false}
        }"#;
        let end_payload = r#"{
            "type":"end",
            "after":{"id":"abc","camera":"front","has_clip":true}
        }"#;
        expect_start_notify(parse_camera_mqtt_payload(
            "kerberos/desktop/events",
            new_payload,
            &base,
            &None,
            &None,
        ));
        let ev = expect_open_clip(parse_camera_mqtt_payload(
            "kerberos/desktop/events",
            end_payload,
            &base,
            &None,
            &None,
        ));
        assert_eq!(
            ev.video_url,
            "http://192.168.151.21:5005/api/events/abc/clip.mp4"
        );
    }

    #[test]
    fn parse_frigate_dedupes_same_id_and_sibling_camera_events() {
        let _serial = FRIGATE_DEDUPE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        reset_frigate_clip_dedupe_for_tests();
        let base = Some("http://192.168.151.21:5005".to_string());
        let first = r#"{
            "type":"end",
            "after":{"id":"evt-w7ji4q","camera":"front","has_clip":true}
        }"#;
        let same_id = r#"{
            "type":"end",
            "after":{"id":"evt-w7ji4q","camera":"front","has_clip":true}
        }"#;
        let sibling = r#"{
            "type":"end",
            "after":{"id":"evt-adhuwv","camera":"front","has_clip":true}
        }"#;
        assert!(matches!(
            parse_camera_mqtt_payload("kerberos/desktop/events", first, &base, &None, &None),
            Some(CameraMqttAction::OpenClip(_))
        ));
        assert!(
            parse_camera_mqtt_payload("kerberos/desktop/events", same_id, &base, &None, &None)
                .is_none(),
            "same Frigate event id must not open twice"
        );
        assert!(
            parse_camera_mqtt_payload("kerberos/desktop/events", sibling, &base, &None, &None)
                .is_none(),
            "overlapping sibling event on same camera must be suppressed"
        );
    }

    #[test]
    fn parse_frigate_new_does_not_open_clip() {
        let _serial = FRIGATE_DEDUPE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        reset_frigate_clip_dedupe_for_tests();
        let payload = r#"{
            "type":"new",
            "before":null,
            "after":{"id":"abc","camera":"front","has_clip":false}
        }"#;
        let base = Some("http://192.168.151.21:5005".to_string());
        assert!(
            !matches!(
                parse_camera_mqtt_payload("kerberos/desktop/events", payload, &base, &None, &None),
                Some(CameraMqttAction::OpenClip(_))
            ),
            "Frigate type=new must not open a clip"
        );
    }

    #[test]
    fn parse_frigate_skips_update_when_clip_becomes_true() {
        // Early update with has_clip flip is intentionally ignored; we wait for
        // type=="end" so Frigate can finish encoding before download.
        let payload = r#"{
            "type":"update",
            "before":{"id":"xyz","camera":"driveway","has_clip":false},
            "after":{"id":"xyz","camera":"driveway","has_clip":true}
        }"#;
        let base = Some("http://frigate.local:5000/".to_string());
        assert!(
            parse_camera_mqtt_payload("kerberos/desktop/events", payload, &base, &None, &None)
                .is_none()
        );
    }

    #[test]
    fn parse_frigate_skips_end_without_clip() {
        let payload = r#"{
            "type":"end",
            "before":{"id":"xyz","camera":"driveway","has_clip":false},
            "after":{"id":"xyz","camera":"driveway","has_clip":false}
        }"#;
        let base = Some("http://frigate.local:5000/".to_string());
        assert!(
            parse_camera_mqtt_payload("kerberos/desktop/events", payload, &base, &None, &None)
                .is_none()
        );
    }

    #[test]
    fn parse_frigate_skips_without_base_url() {
        let payload = r#"{
            "type":"end",
            "after":{"id":"abc","camera":"front","has_clip":true}
        }"#;
        assert!(
            parse_camera_mqtt_payload("kerberos/desktop/events", payload, &None, &None, &None)
                .is_none()
        );
        assert!(parse_camera_mqtt_payload(
            "kerberos/desktop/events",
            payload,
            &Some("".to_string()),
            &None,
            &None
        )
        .is_none());
    }

    #[test]
    fn parse_ring_motion_on_with_template_opens_snapshot() {
        let _serial = RING_DEDUPE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        reset_ring_dedupe_for_tests();
        let topic = "ring/18d65208-6816-4bbe-bf09-310b7a201feb/camera/54e019cac69d/motion/state";
        let template =
            Some("http://ha:8123/api/camera_proxy/camera.front_door_snapshot".to_string());
        let ev = expect_open_clip(parse_camera_mqtt_payload(
            topic, "ON", &None, &template, &None,
        ));
        assert_eq!(ev.agent_name, "Ring Motion (54e019cac69d)");
        assert_eq!(
            ev.video_url,
            "http://ha:8123/api/camera_proxy/camera.front_door_snapshot"
        );
    }

    #[test]
    fn parse_ring_motion_off_ignored() {
        let _serial = RING_DEDUPE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        reset_ring_dedupe_for_tests();
        let topic = "ring/18d65208-6816-4bbe-bf09-310b7a201feb/camera/54e019cac69d/motion/state";
        assert!(parse_camera_mqtt_payload(topic, "OFF", &None, &None, &None).is_none());
    }

    #[test]
    fn parse_ring_ding_without_template_is_start_notify() {
        let _serial = RING_DEDUPE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        reset_ring_dedupe_for_tests();
        let topic = "ring/18d65208-6816-4bbe-bf09-310b7a201feb/camera/54e019cac69d/ding/state";
        let agent =
            expect_start_notify(parse_camera_mqtt_payload(topic, "ON", &None, &None, &None));
        assert_eq!(agent, "Ring Ding (54e019cac69d)");
    }

    #[test]
    fn parse_ring_template_placeholders() {
        let _serial = RING_DEDUPE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        reset_ring_dedupe_for_tests();
        let topic = "ring/loc-1/camera/dev-2/motion/state";
        let template = Some("http://media/{location_id}/{device_id}/{event}.jpg".to_string());
        let ev = expect_open_clip(parse_camera_mqtt_payload(
            topic, "on", &None, &template, &None,
        ));
        assert_eq!(ev.video_url, "http://media/loc-1/dev-2/motion.jpg");
    }

    #[test]
    fn camera_topic_matches_ring_wildcard() {
        let cfg = Some(
            "kerberos/desktop/events;ring/+/camera/+/motion/state;ring/+/camera/+/ding/state"
                .to_string(),
        );
        assert!(camera_topic_matches(
            "ring/18d65208-6816-4bbe-bf09-310b7a201feb/camera/54e019cac69d/motion/state",
            &cfg
        ));
        assert!(camera_topic_matches(
            "ring/18d65208-6816-4bbe-bf09-310b7a201feb/camera/54e019cac69d/ding/state",
            &cfg
        ));
        assert!(!camera_topic_matches(
            "ring/18d65208-6816-4bbe-bf09-310b7a201feb/camera/54e019cac69d/snapshot/image",
            &cfg
        ));
    }

    #[test]
    fn parse_ring_does_not_break_kerberos() {
        let payload = r#"{"agent_name":"Porch","video_url":"http://cam/clip.mp4"}"#;
        let ev = expect_open_clip(parse_camera_mqtt_payload(
            "kerberos/desktop/events",
            payload,
            &None,
            &Some("http://ha/ignored".to_string()),
            &None,
        ));
        assert_eq!(ev.agent_name, "Porch");
        assert_eq!(ev.video_url, "http://cam/clip.mp4");
    }
}

#[cfg(test)]
mod frigate_clip_dedupe_tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn allows_first_open_then_blocks_same_id() {
        let mut state = FrigateClipDedupeState::new();
        let t0 = Instant::now();
        assert!(frigate_clip_should_open(&mut state, "id-a", "front", t0));
        assert!(!frigate_clip_should_open(
            &mut state,
            "id-a",
            "front",
            t0 + Duration::from_secs(1)
        ));
    }

    #[test]
    fn blocks_sibling_id_on_same_camera_within_cooldown() {
        let mut state = FrigateClipDedupeState::new();
        let t0 = Instant::now();
        assert!(frigate_clip_should_open(&mut state, "w7ji4q", "front", t0));
        assert!(!frigate_clip_should_open(
            &mut state,
            "adhuwv",
            "front",
            t0 + Duration::from_secs(2)
        ));
    }

    #[test]
    fn allows_different_camera_immediately() {
        let mut state = FrigateClipDedupeState::new();
        let t0 = Instant::now();
        assert!(frigate_clip_should_open(&mut state, "id-1", "front", t0));
        assert!(frigate_clip_should_open(
            &mut state,
            "id-2",
            "back",
            t0 + Duration::from_secs(1)
        ));
    }

    #[test]
    fn allows_same_camera_after_cooldown() {
        let mut state = FrigateClipDedupeState::new();
        let t0 = Instant::now();
        assert!(frigate_clip_should_open(&mut state, "id-1", "front", t0));
        assert!(frigate_clip_should_open(
            &mut state,
            "id-2",
            "front",
            t0 + FRIGATE_CAMERA_COOLDOWN + Duration::from_secs(1)
        ));
    }

    #[test]
    fn allows_same_id_after_ttl() {
        let mut state = FrigateClipDedupeState::new();
        let t0 = Instant::now();
        assert!(frigate_clip_should_open(&mut state, "id-a", "front", t0));
        // Advance past both camera cooldown and id TTL.
        let later = t0 + FRIGATE_EVENT_ID_TTL + Duration::from_secs(1);
        assert!(frigate_clip_should_open(&mut state, "id-a", "front", later));
    }

    #[test]
    fn start_notify_allows_first_then_blocks_same_id() {
        let mut state = FrigateClipDedupeState::new();
        let t0 = Instant::now();
        assert!(frigate_start_should_notify(&mut state, "id-a", "front", t0));
        assert!(!frigate_start_should_notify(
            &mut state,
            "id-a",
            "front",
            t0 + Duration::from_secs(1)
        ));
    }
}
