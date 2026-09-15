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
    actions: Vec<String>,
    advertised: Vec<bool>,
    link: watch::Sender<Connection>,
}

struct Entity {
    name: String,
    live_seen: bool,
    item: Value,
    actionable: bool,
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
    pub fn new(entities: &[String], actions: &[String]) -> Shared {
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
        entity.title = item["title"].as_str().unwrap_or(name).to_owned();
        if entity.item != item || entity.actionable != actionable {
            entity.item = item;
            entity.actionable = actionable;
            self.revision = self.revision.wrapping_add(1);
        }
    }

    fn action_entity(&self, index: usize) -> Option<&Entity> {
        if !self.link.borrow().connected {
            return None;
        }
        let name = self.actions.get(index)?;
        self.entities
            .iter()
            .find(|entity| entity.name == *name && entity.actionable)
    }

    /// Recheck both publication and current entity availability at admission.
    pub fn action_target(&self, action_id: &str) -> Option<String> {
        let index =
            self.actions.iter().enumerate().find_map(|(index, _)| {
                (action_id == format!("ha-action-{index}")).then_some(index)
            })?;
        if !self.advertised[index] {
            return None;
        }
        Some(self.action_entity(index)?.name.clone())
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
                let verb = if entity.name.starts_with("button.") {
                    "Press "
                } else {
                    "Activate "
                };
                let label = format!("{verb}{}", bounded(&entity.title, 128 - verb.len()));
                items.push(json!({"kind":"action","id":format!("ha-action-{index}"),
                    "title":entity.title,"action_id":format!("ha-action-{index}"),
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
        let state = Book::new(&names, &names[..2]);
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
            book.action_target("ha-action-0").as_deref(),
            Some("button.first")
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
        let state = Book::new(&names, &names);
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
}
