use crate::config::ConfiguredAction;
use inverter_worker_protocol::Output;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::watch;

pub type Shared = Arc<Mutex<Book>>;

#[derive(Clone, Copy, Default)]
pub struct Connection {
    pub epoch: u64,
    pub connected: bool,
    pub authentication_rejected: bool,
    pub changed_at: Option<std::time::Instant>,
}

pub struct Book {
    entities: Vec<Entity>,
    connection: &'static str,
    tone: &'static str,
    revision: u64,
    published: Option<u64>,
    actions: Vec<ConfiguredAction>,
    advertised: Vec<bool>,
    link: watch::Sender<Connection>,
}

struct Entity {
    name: String,
    live_seen: bool,
    item: Value,
    actionable: bool,
    unknown: bool,
    title: String,
}

fn bounded(value: &str, limit: usize) -> String {
    let mut result = String::new();
    for character in value.chars().filter(|c| !c.is_control()) {
        if result.len() + character.len_utf8() > limit {
            break;
        }
        result.push(character);
    }
    result.trim().to_owned()
}

fn unavailable(index: usize, name: &str, status: &str) -> Value {
    json!({"kind":"status","id":format!("entity-{index}"),"title":name,
        "value":status,"tone":"neutral"})
}

fn contribution(index: usize, entity: &str, state: Option<&Value>) -> Value {
    let Some(state) = state.filter(|state| state["entity_id"].as_str() == Some(entity)) else {
        return unavailable(index, entity, "Unavailable");
    };
    let Some(value) = state["state"].as_str() else {
        return unavailable(index, entity, "Unavailable");
    };
    let title = state["attributes"]["friendly_name"]
        .as_str()
        .map(|name| bounded(name, 128))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| entity.to_owned());
    if matches!(value, "unknown" | "unavailable") {
        return json!({"kind":"status","id":format!("entity-{index}"),"title":title,
            "value":if value=="unknown" {"Unknown"} else {"Unavailable"},"tone":"neutral"});
    }
    if let Some(number) = value
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite())
    {
        let unit = state["attributes"]["unit_of_measurement"]
            .as_str()
            .map(|unit| bounded(unit, 32))
            .filter(|unit| !unit.is_empty());
        return json!({"kind":"metric","id":format!("entity-{index}"),"title":title,"value":number,"unit":unit});
    }
    // Keep every frame below the host's 64 KiB limit even with all 32 entities.
    let text = bounded(value, 512);
    if text.is_empty() {
        return unavailable(index, &title, "Unavailable");
    }
    json!({"kind":"text","id":format!("entity-{index}"),"title":title,"text":text})
}

impl Book {
    pub fn new(entities: &[String], actions: &[ConfiguredAction]) -> Shared {
        let (link, _) = watch::channel(Connection::default());
        Arc::new(Mutex::new(Self {
            entities: entities
                .iter()
                .enumerate()
                .map(|(index, name)| Entity {
                    name: name.clone(),
                    live_seen: false,
                    item: unavailable(index, name, "Waiting"),
                    actionable: false,
                    unknown: false,
                    title: name.clone(),
                })
                .collect(),
            connection: "Connecting",
            tone: "neutral",
            revision: 0,
            published: None,
            actions: actions.to_vec(),
            advertised: vec![false; actions.len()],
            link,
        }))
    }

    pub fn subscribe_connection(&self) -> watch::Receiver<Connection> {
        self.link.subscribe()
    }

    fn connection(&mut self, value: &'static str, tone: &'static str) {
        self.connection = value;
        self.tone = tone;
        self.revision = self.revision.wrapping_add(1);
        self.link.send_modify(|link| {
            link.epoch = link.epoch.wrapping_add(1);
            link.connected = value == "Connected";
            link.authentication_rejected = value == "Authentication rejected";
            link.changed_at = Some(std::time::Instant::now());
        });
        self.advertised.fill(false);
    }

    pub fn begin_session(&mut self) {
        for (index, entity) in self.entities.iter_mut().enumerate() {
            entity.live_seen = false;
            entity.item = unavailable(index, &entity.name, "Waiting");
            entity.actionable = false;
        }
        self.connection("Connecting", "neutral");
    }

    pub fn connected(&mut self) {
        self.connection("Connected", "success");
    }

    fn clear(&mut self) {
        for (index, entity) in self.entities.iter_mut().enumerate() {
            entity.item = unavailable(index, &entity.name, "Unavailable");
            entity.actionable = false;
        }
    }

    pub fn disconnected(&mut self) {
        self.clear();
        self.connection("Disconnected", "warning");
    }

    pub fn authentication_rejected(&mut self) {
        self.clear();
        self.connection("Authentication rejected", "error");
    }

    pub fn initial(&mut self, name: &str, state: Option<&Value>) {
        self.update(name, state, false);
    }

    pub fn live(&mut self, name: &str, state: Option<&Value>) {
        self.update(name, state, true);
    }

    fn update(&mut self, name: &str, state: Option<&Value>, live: bool) {
        let Some((index, entity)) = self
            .entities
            .iter_mut()
            .enumerate()
            .find(|(_, entity)| entity.name == name)
        else {
            return;
        };
        if !live && entity.live_seen {
            return;
        }
        entity.live_seen |= live;
        let item = contribution(index, name, state);
        let actionable = state.is_some_and(|state| {
            state["entity_id"].as_str() == Some(name)
                && state["state"]
                    .as_str()
                    .is_some_and(|value| value != "unavailable")
        });
        let unknown = state.is_some_and(|state| state["state"].as_str() == Some("unknown"));
        entity.title = item["title"].as_str().unwrap_or(name).to_owned();
        if entity.item != item || entity.actionable != actionable || entity.unknown != unknown {
            entity.item = item;
            entity.actionable = actionable;
            entity.unknown = unknown;
            self.revision = self.revision.wrapping_add(1);
        }
    }

    fn action_entity(&self, index: usize) -> Option<&Entity> {
        if !self.link.borrow().connected {
            return None;
        }
        let action = self.actions.get(index)?;
        self.entities.iter().find(|entity| {
            entity.name == action.entity
                && entity.actionable
                && (!entity.unknown || action.operation.allows_unknown())
        })
    }

    /// Recheck both publication and current entity availability at admission.
    pub fn action_target(&self, action_id: &str) -> Option<ConfiguredAction> {
        let index = self
            .actions
            .iter()
            .position(|action| action.id == action_id)?;
        if !self.advertised[index] {
            return None;
        }
        self.action_entity(index)?;
        Some(self.actions[index].clone())
    }

    fn mark_published(&mut self) {
        self.advertised = (0..self.actions.len())
            .map(|index| self.action_entity(index).is_some())
            .collect();
        self.published = Some(self.revision);
    }

    fn frame(&self) -> Value {
        let mut items = vec![
            json!({"kind":"status","id":"connection","title":"Home Assistant",
            "value":self.connection,"tone":self.tone}),
        ];
        items.extend(self.entities.iter().map(|entity| entity.item.clone()));
        for index in 0..self.actions.len() {
            if let Some(entity) = self.action_entity(index) {
                let action = &self.actions[index];
                let verb = action.operation.label();
                let label = format!("{verb}{}", bounded(&entity.title, 128 - verb.len()));
                items.push(json!({"kind":"action","id":action.id,
                    "title":entity.title,"action_id":action.id,
                    "label":label,"params":{}}));
            }
        }
        json!({"type":"contributions","items":items})
    }
}

pub async fn publish(shared: Shared, output: Output) -> Result<(), &'static str> {
    let mut interval = tokio::time::interval(Duration::from_millis(250));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        interval.tick().await;
        let mut book = shared.lock().map_err(|_| "state unavailable")?;
        if book.published == Some(book.revision) {
            continue;
        }
        if output.try_send(book.frame())? {
            book.mark_published();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::configured_actions;

    #[test]
    fn late_initial_state_cannot_replace_live_or_deleted_state() {
        let state = Book::new(&["sensor.a".into(), "sensor.b".into()], &[]);
        let mut book = state.lock().unwrap();
        book.begin_session();
        book.live(
            "sensor.a",
            Some(&json!({"entity_id":"sensor.a","state":"2"})),
        );
        book.initial(
            "sensor.a",
            Some(&json!({"entity_id":"sensor.a","state":"1"})),
        );
        book.live("sensor.b", None);
        book.initial(
            "sensor.b",
            Some(&json!({"entity_id":"sensor.b","state":"1"})),
        );
        assert_eq!(book.frame()["items"][1]["value"], 2.0);
        assert_eq!(book.frame()["items"][2]["value"], "Unavailable");
        let revision = book.revision;
        book.live("sensor.unwatched", Some(&json!({"state":"secret"})));
        assert_eq!(book.revision, revision);
        book.begin_session();
        book.initial(
            "sensor.a",
            Some(&json!({"entity_id":"sensor.a","state":"3"})),
        );
        assert_eq!(book.frame()["items"][1]["value"], 3.0);
        book.disconnected();
        assert_eq!(book.frame()["items"][1]["value"], "Unavailable");
    }

    #[test]
    fn contributions_are_bounded_plain_data_with_no_actions() {
        let entities = (0..32).map(|i| format!("sensor.e{i}")).collect::<Vec<_>>();
        let state = Book::new(&entities, &[]);
        let mut book = state.lock().unwrap();
        for entity in &entities {
            book.live(
                entity,
                Some(&json!({"entity_id":entity,"state":"é".repeat(8192),
                "attributes":{"friendly_name":"🌡\n".repeat(1024),"icon":"<script>bad</script>"}})),
            );
        }
        let frame = book.frame();
        assert!(serde_json::to_vec(&frame).unwrap().len() < 64 * 1024);
        for item in frame["items"].as_array().unwrap() {
            assert_ne!(item["kind"], "action");
            assert!(item["title"].as_str().unwrap().len() <= 128);
        }
        for (value, kind) in [
            ("NaN", "text"),
            ("inf", "text"),
            ("1e999", "text"),
            ("23.5", "metric"),
            ("unknown", "status"),
            ("unavailable", "status"),
        ] {
            assert_eq!(
                contribution(
                    0,
                    "sensor.e0",
                    Some(&json!({"entity_id":"sensor.e0","state":value}))
                )["kind"],
                kind
            );
        }
        assert_eq!(
            contribution(
                0,
                "sensor.a",
                Some(&json!({"entity_id":"sensor.b","state":"secret"}))
            )["value"],
            "Unavailable"
        );
    }

    #[test]
    fn actions_need_explicit_configuration_current_availability_and_publication() {
        let names = [
            "button.first".into(),
            "scene.night".into(),
            "button.read_only".into(),
        ];
        let state = Book::new(&names, &configured_actions(&names[..2], &[]));
        let mut book = state.lock().unwrap();
        for name in &names {
            book.initial(name, Some(&json!({"entity_id":name,"state":"unknown","attributes":{"friendly_name":"A name"}})));
        }
        assert_eq!(book.frame()["items"].as_array().unwrap().len(), 4);
        book.connected();
        let frame = book.frame();
        let actions = &frame["items"].as_array().unwrap()[4..];
        assert_eq!(actions.len(), 2);
        assert_eq!(
            actions[0],
            json!({"kind":"action","id":"ha-action-0","title":"A name","action_id":"ha-action-0","label":"Press A name","params":{}})
        );
        assert_eq!(actions[1]["label"], "Activate A name");
        assert!(book.action_target("ha-action-0").is_none());
        book.mark_published();
        assert_eq!(
            book.action_target("ha-action-0")
                .map(|action| action.entity),
            Some("button.first".into())
        );
        assert!(book.action_target("ha-action-00").is_none());
        assert!(book.action_target("ha-action-2").is_none());
        book.live(
            "button.first",
            Some(&json!({"entity_id":"button.first","state":"unavailable"})),
        );
        assert!(book.action_target("ha-action-0").is_none());
        book.live("scene.night", None);
        assert!(book.action_target("ha-action-1").is_none());
        book.initial(
            "scene.night",
            Some(&json!({"entity_id":"scene.night","state":"unknown"})),
        );
        assert!(
            book.action_target("ha-action-1").is_none(),
            "late REST state cannot revive a deleted action"
        );
        book.disconnected();
        assert_eq!(book.frame()["items"].as_array().unwrap().len(), 4);
        book.connected();
        assert!(book.action_target("ha-action-0").is_none());
    }

    #[test]
    fn action_names_stay_self_contained_and_bounded_with_multibyte_ha_titles() {
        let names = (0..16)
            .map(|index| format!("scene.e{index}"))
            .collect::<Vec<_>>();
        let state = Book::new(&names, &configured_actions(&names, &[]));
        let mut book = state.lock().unwrap();
        book.connected();
        for name in &names {
            book.live(name, Some(&json!({"entity_id":name,"state":"unknown","attributes":{"friendly_name":"🌞\n".repeat(128)}})));
        }
        let frame = book.frame();
        assert!(
            serde_json::to_vec(&frame).unwrap().len() < inverter_worker_protocol::MAX_FRAME_BYTES
        );
        for action in frame["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["kind"] == "action")
        {
            let label = action["label"].as_str().unwrap();
            assert!(label.starts_with("Activate 🌞"));
            assert!(label.len() <= 128);
            assert!(!label.contains('\n'));
            assert_eq!(action["params"], json!({}));
        }
    }

    #[test]
    fn media_actions_require_observed_known_state_and_preserve_button_scene_behavior() {
        let names = [
            "button.first".into(),
            "scene.evening".into(),
            "media_player.den".into(),
            "media_player.read_only".into(),
        ];
        let actions = configured_actions(&names[..2], &names[2..3]);
        let state = Book::new(&names, &actions);
        let mut book = state.lock().unwrap();
        book.connected();
        assert_eq!(book.frame()["items"].as_array().unwrap().len(), 5);
        for name in &names {
            book.initial(name, Some(&json!({"entity_id":name,"state":"unknown"})));
        }
        book.mark_published();
        assert!(book.action_target("ha-action-0").is_some());
        assert!(book.action_target("ha-action-1").is_some());
        assert!(book.action_target("ha-media-0-play").is_none());
        assert_eq!(book.frame()["items"].as_array().unwrap().len(), 7);
        book.live("media_player.den", Some(&json!({"entity_id":"media_player.den","state":"playing","attributes":{"friendly_name":"Living room"}})));
        book.live(
            "media_player.read_only",
            Some(&json!({"entity_id":"media_player.read_only","state":"paused"})),
        );
        assert!(
            book.action_target("ha-media-0-play").is_none(),
            "new availability must be published first"
        );
        let frame = book.frame();
        let items = frame["items"].as_array().unwrap();
        assert_eq!(items.len(), 10);
        for (offset, (verb, label)) in [
            ("play", "Play Living room"),
            ("pause", "Pause Living room"),
            ("stop", "Stop Living room"),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(
                items[7 + offset],
                json!({"kind":"action","id":format!("ha-media-0-{verb}"),"title":"Living room","action_id":format!("ha-media-0-{verb}"),"label":label,"params":{}})
            );
        }
        book.mark_published();
        for verb in ["play", "pause", "stop"] {
            assert_eq!(
                book.action_target(&format!("ha-media-0-{verb}"))
                    .unwrap()
                    .entity,
                "media_player.den"
            );
        }
        for id in [
            "ha-media-00-play",
            "ha-media-0-toggle",
            "ha-media-1-play",
            "ha-action-2",
        ] {
            assert!(book.action_target(id).is_none());
        }
        for value in ["unknown", "unavailable"] {
            book.live(
                "media_player.den",
                Some(&json!({"entity_id":"media_player.den","state":value})),
            );
            for verb in ["play", "pause", "stop"] {
                assert!(book.action_target(&format!("ha-media-0-{verb}")).is_none());
            }
        }
        book.live("media_player.den", None);
        book.initial(
            "media_player.den",
            Some(&json!({"entity_id":"media_player.den","state":"paused"})),
        );
        assert!(
            book.action_target("ha-media-0-play").is_none(),
            "late REST state cannot revive a deleted player"
        );
        book.live(
            "media_player.den",
            Some(&json!({"entity_id":"media_player.den","state":"idle"})),
        );
        book.mark_published();
        assert!(book.action_target("ha-media-0-play").is_some());
        book.disconnected();
        assert!(book.action_target("ha-media-0-play").is_none());
        book.begin_session();
        book.connected();
        assert!(
            book.action_target("ha-media-0-play").is_none(),
            "reconnection needs fresh observed state"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn maximum_combined_escaped_contributions_fit_the_real_output_frame_limit() {
        let literal = |domain: &str, index: usize| {
            let suffix = format!("_{index}");
            format!(
                "{domain}.{}{suffix}",
                "x".repeat(128 - domain.len() - 1 - suffix.len())
            )
        };
        let watched = (0..12)
            .map(|index| literal("sensor", index))
            .collect::<Vec<_>>();
        let actions = (0..16)
            .map(|index| literal(if index % 2 == 0 { "button" } else { "scene" }, index))
            .collect::<Vec<_>>();
        let media = (0..4)
            .map(|index| literal("media_player", index))
            .collect::<Vec<_>>();
        let config: crate::config::Configuration = serde_json::from_value(json!({
            "revision":"maximum-combined",
            "values":{"ha_base_url":"http://localhost/ha/","watch_entities":watched.join(","),"action_entities":actions.join(","),"media_player_entities":media.join(",")},
            "secrets":{"ha_token":"fixture-token"}
        })).unwrap();
        let config = config.validate().unwrap();
        assert_eq!(config.entities.len(), 32);
        assert!(config.entities.iter().all(|entity| entity.len() == 128));
        let state = Book::new(&config.entities, &config.actions());
        let frame = {
            let mut book = state.lock().unwrap();
            book.connected();
            for entity in &config.entities {
                // Quotes and backslashes take two JSON bytes per retained byte;
                // fill every title/text allowance with that worst escaping cost.
                book.live(entity, Some(&json!({"entity_id":entity,"state":"\\\"".repeat(1024),"attributes":{"friendly_name":"\\\"".repeat(256)}})));
            }
            book.frame()
        };
        let items = frame["items"].as_array().unwrap();
        assert_eq!(items.len(), 61);
        assert!(items.len() <= 64);
        assert_eq!(
            items.iter().filter(|item| item["kind"] == "action").count(),
            28
        );
        let ids = items
            .iter()
            .map(|item| item["id"].as_str().unwrap())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(ids.len(), items.len());
        for item in &items[1..33] {
            assert_eq!(item["title"].as_str().unwrap().len(), 128);
            assert_eq!(item["text"].as_str().unwrap().len(), 512);
        }
        for item in &items[33..] {
            assert_eq!(item["title"].as_str().unwrap().len(), 128);
            assert_eq!(item["label"].as_str().unwrap().len(), 128);
            assert_eq!(item["params"], json!({}));
        }
        let encoded = serde_json::to_vec(&frame).unwrap();
        assert!(encoded.len() < inverter_worker_protocol::MAX_FRAME_BYTES);
        // Exercise the actual transport encoder, queue and completed flush too.
        Output::with_writer(std::io::sink())
            .send(frame)
            .await
            .unwrap();
    }
}
