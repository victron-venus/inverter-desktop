use inverter_worker_protocol::Output;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub type Shared = Arc<Mutex<Book>>;

pub struct Book {
    entities: Vec<Entity>,
    connection: &'static str,
    tone: &'static str,
    revision: u64,
    published: Option<u64>,
}

struct Entity {
    name: String,
    live_seen: bool,
    item: Value,
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
    pub fn new(entities: &[String]) -> Shared {
        Arc::new(Mutex::new(Self {
            entities: entities
                .iter()
                .enumerate()
                .map(|(index, name)| Entity {
                    name: name.clone(),
                    live_seen: false,
                    item: unavailable(index, name, "Waiting"),
                })
                .collect(),
            connection: "Connecting",
            tone: "neutral",
            revision: 0,
            published: None,
        }))
    }

    fn connection(&mut self, value: &'static str, tone: &'static str) {
        self.connection = value;
        self.tone = tone;
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn begin_session(&mut self) {
        for (index, entity) in self.entities.iter_mut().enumerate() {
            entity.live_seen = false;
            entity.item = unavailable(index, &entity.name, "Waiting");
        }
        self.connection("Connecting", "neutral");
    }

    pub fn connected(&mut self) {
        self.connection("Connected", "success");
    }

    fn clear(&mut self) {
        for (index, entity) in self.entities.iter_mut().enumerate() {
            entity.item = unavailable(index, &entity.name, "Unavailable");
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
        if entity.item != item {
            entity.item = item;
            self.revision = self.revision.wrapping_add(1);
        }
    }

    fn frame(&self) -> Value {
        let mut items = vec![
            json!({"kind":"status","id":"connection","title":"Home Assistant",
            "value":self.connection,"tone":self.tone}),
        ];
        items.extend(self.entities.iter().map(|entity| entity.item.clone()));
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
            book.published = Some(book.revision);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn late_initial_state_cannot_replace_live_or_deleted_state() {
        let state = Book::new(&["sensor.a".into(), "sensor.b".into()]);
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
        let state = Book::new(&entities);
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
}
