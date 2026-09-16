use crate::appliances::Appliances;
use crate::config::{ApplianceProfiles, ConfiguredAction};
use crate::discovery::Discovery;
use crate::numeric::{self, Observation};
use inverter_worker_protocol::Output;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::watch;

pub type Shared = Arc<Mutex<Book>>;

// Quotes/backslashes can double the encoded size. Together with the configured
// entity/control limits these bounds keep full snapshots within 64 KiB.
const MAX_TITLE_BYTES: usize = 64;
pub(crate) const MAX_TEXT_BYTES: usize = 256;

#[derive(Clone, Copy, Default)]
pub struct Connection {
    pub epoch: u64,
    pub connected: bool,
    pub authentication_rejected: bool,
    pub changed_at: Option<std::time::Instant>,
}

pub struct Book {
    entities: Vec<Entity>,
    discovery: Discovery,
    appliances: Appliances,
    connection: &'static str,
    tone: &'static str,
    revision: u64,
    published: Option<u64>,
    actions: Vec<ConfiguredAction>,
    advertised: Vec<bool>,
    inputs: Vec<Input>,
    link: watch::Sender<Connection>,
}

struct Input {
    action: ConfiguredAction,
    observation: Option<Observation>,
    revision: u64,
    exhausted: bool,
    advertised: Option<u64>,
}

impl Input {
    fn rotate(&mut self) {
        self.advertised = None;
        if let Some(revision) = self.revision.checked_add(1) {
            self.revision = revision;
        } else {
            self.exhausted = true;
        }
    }

    fn token(&self) -> String {
        format!("ha-input-{}", self.revision)
    }

    fn update(&mut self, observation: Option<Observation>) -> bool {
        if self.observation == observation {
            return false;
        }
        if self.observation.as_ref().map(|value| &value.constraints)
            != observation.as_ref().map(|value| &value.constraints)
        {
            self.rotate();
        }
        self.observation = observation;
        true
    }
}

struct Entity {
    name: String,
    live_seen: bool,
    item: Value,
    actionable: bool,
    unknown: bool,
    binary_known: bool,
    cover_features: u64,
    title: String,
}

pub(crate) fn bounded(value: &str, limit: usize) -> String {
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
    json!({"kind":"status","id":format!("entity-{index}"),"title":bounded(name, MAX_TITLE_BYTES),
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
        .map(|name| bounded(name, MAX_TITLE_BYTES))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| bounded(entity, MAX_TITLE_BYTES));
    if matches!(value, "unknown" | "unavailable") {
        return json!({"kind":"status","id":format!("entity-{index}"),"title":title,
            "value":if value=="unknown" {"Unknown"} else {"Unavailable"},"tone":"neutral"});
    }
    if entity
        .split_once('.')
        .is_some_and(|(domain, _)| domain == "weather")
    {
        return crate::weather::summary(value, &state["attributes"]).map_or_else(
            || unavailable(index, &title, "Unavailable"),
            |text| json!({"kind":"text","id":format!("entity-{index}"),"title":title,"text":text}),
        );
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
    let text = bounded(value, MAX_TEXT_BYTES);
    if text.is_empty() {
        return unavailable(index, &title, "Unavailable");
    }
    json!({"kind":"text","id":format!("entity-{index}"),"title":title,"text":text})
}

pub(crate) fn readonly_contribution(entity: &str, state: &Value) -> Value {
    contribution(0, entity, Some(state))
}

impl Book {
    #[cfg(test)]
    pub fn new(
        entities: &[String],
        actions: &[ConfiguredAction],
        inputs: &[ConfiguredAction],
    ) -> Shared {
        Self::with_discovery(entities, actions, inputs, &[])
    }

    #[cfg(test)]
    pub fn with_discovery(
        entities: &[String],
        actions: &[ConfiguredAction],
        inputs: &[ConfiguredAction],
        discovery_prefixes: &[String],
    ) -> Shared {
        Self::with_appliances(
            entities,
            actions,
            inputs,
            discovery_prefixes,
            &ApplianceProfiles::default(),
        )
    }

    pub fn with_appliances(
        entities: &[String],
        actions: &[ConfiguredAction],
        inputs: &[ConfiguredAction],
        discovery_prefixes: &[String],
        profiles: &ApplianceProfiles,
    ) -> Shared {
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
                    binary_known: false,
                    cover_features: 0,
                    title: bounded(name, MAX_TITLE_BYTES),
                })
                .collect(),
            discovery: Discovery::new(entities, discovery_prefixes),
            appliances: Appliances::new(entities, profiles),
            connection: "Connecting",
            tone: "neutral",
            revision: 0,
            published: None,
            actions: actions.to_vec(),
            advertised: vec![false; actions.len()],
            inputs: inputs
                .iter()
                .map(|action| Input {
                    action: action.clone(),
                    observation: None,
                    revision: 0,
                    exhausted: false,
                    advertised: None,
                })
                .collect(),
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
        for input in &mut self.inputs {
            input.rotate();
        }
    }

    pub fn begin_session(&mut self) {
        self.discovery.begin_session();
        self.appliances.clear();
        for input in &mut self.inputs {
            input.update(None);
        }
        for (index, entity) in self.entities.iter_mut().enumerate() {
            entity.live_seen = false;
            entity.item = unavailable(index, &entity.name, "Waiting");
            entity.actionable = false;
            entity.binary_known = false;
            entity.cover_features = 0;
        }
        self.connection("Connecting", "neutral");
    }

    pub fn connected(&mut self) {
        self.connection("Connected", "success");
    }

    fn clear(&mut self) {
        self.discovery.clear();
        self.appliances.clear();
        for input in &mut self.inputs {
            input.update(None);
        }
        for (index, entity) in self.entities.iter_mut().enumerate() {
            entity.item = unavailable(index, &entity.name, "Unavailable");
            entity.actionable = false;
            entity.binary_known = false;
            entity.cover_features = 0;
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

    pub fn discovery_snapshot(&mut self, states: &[Value]) {
        if self.discovery.snapshot(states) {
            self.revision = self.revision.wrapping_add(1);
        }
    }

    pub fn discovery_failed(&mut self) {
        if self.discovery.failed() {
            self.revision = self.revision.wrapping_add(1);
        }
    }

    fn update(&mut self, name: &str, state: Option<&Value>, live: bool) {
        let Some((index, entity)) = self
            .entities
            .iter_mut()
            .enumerate()
            .find(|(_, entity)| entity.name == name)
        else {
            if live && self.discovery.live(name, state) {
                self.revision = self.revision.wrapping_add(1);
            }
            return;
        };
        if !live && entity.live_seen {
            return;
        }
        entity.live_seen |= live;
        if self.appliances.update(
            index,
            state.filter(|state| state["entity_id"].as_str() == Some(name)),
        ) {
            self.revision = self.revision.wrapping_add(1);
        }
        for input in self
            .inputs
            .iter_mut()
            .filter(|input| input.action.entity == name)
        {
            if input.update(numeric::observe(input.action.operation, name, state)) {
                self.revision = self.revision.wrapping_add(1);
            }
        }
        let item = contribution(index, name, state);
        let actionable = state.is_some_and(|state| {
            state["entity_id"].as_str() == Some(name)
                && state["state"]
                    .as_str()
                    .is_some_and(|value| value != "unavailable")
        });
        let unknown = state.is_some_and(|state| state["state"].as_str() == Some("unknown"));
        let binary_known = state.is_some_and(|state| {
            state["entity_id"].as_str() == Some(name)
                && matches!(state["state"].as_str(), Some("on" | "off"))
        });
        let cover_features = state
            .filter(|state| {
                state["entity_id"].as_str() == Some(name)
                    && matches!(
                        state["state"].as_str(),
                        Some("open" | "closed" | "opening" | "closing")
                    )
            })
            .and_then(|state| state["attributes"]["supported_features"].as_u64())
            .unwrap_or(0);
        entity.title = item["title"].as_str().unwrap_or(name).to_owned();
        if entity.item != item
            || entity.actionable != actionable
            || entity.unknown != unknown
            || entity.binary_known != binary_known
            || entity.cover_features != cover_features
        {
            entity.item = item;
            entity.actionable = actionable;
            entity.unknown = unknown;
            entity.binary_known = binary_known;
            entity.cover_features = cover_features;
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
                && (!action.operation.requires_binary_state() || entity.binary_known)
                && action
                    .operation
                    .required_cover_feature()
                    .is_none_or(|feature| entity.cover_features & feature != 0)
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

    /// Numeric parameters are authorized against the last published grant and
    /// the current observation. A revocation cannot be undone by restoring its
    /// previous bounds before the next publication.
    pub fn input_target(
        &self,
        action_id: &str,
        params: &Value,
    ) -> Result<(ConfiguredAction, Value), &'static str> {
        let (revision, value) = numeric::params(params).ok_or("invalid_action")?;
        let input = self
            .inputs
            .iter()
            .find(|input| input.action.id == action_id)
            .ok_or("invalid_action")?;
        if !self.link.borrow().connected
            || input.exhausted
            || input.advertised != Some(input.revision)
            || revision != input.token()
        {
            return Err("unavailable");
        }
        let observation = input.observation.as_ref().ok_or("unavailable")?;
        if !observation.constraints.accepts(value) {
            return Err("invalid_action");
        }
        let value = numeric::service_value(value, observation.constraints.decimal_places);
        let body = match input.action.operation {
            crate::config::Operation::SetNumber => {
                json!({"entity_id":input.action.entity,"value":value})
            }
            crate::config::Operation::SetCoverPosition => {
                json!({"entity_id":input.action.entity,"position":value})
            }
            _ => return Err("invalid_action"),
        };
        Ok((input.action.clone(), body))
    }

    fn mark_published(&mut self) {
        self.advertised = (0..self.actions.len())
            .map(|index| self.action_entity(index).is_some())
            .collect();
        self.published = Some(self.revision);
        let connected = self.link.borrow().connected;
        for input in &mut self.inputs {
            input.advertised = (connected && !input.exhausted && input.observation.is_some())
                .then_some(input.revision);
        }
    }

    fn frame(&self) -> Value {
        let (connection, tone) = self.discovery.notice().map_or_else(
            || (self.connection.to_owned(), self.tone),
            |notice| (format!("{}; {notice}", self.connection), "warning"),
        );
        let mut items = vec![
            json!({"kind":"status","id":"connection","title":"Home Assistant",
            "value":connection,"tone":tone}),
        ];
        items.extend(self.entities.iter().enumerate().map(|(index, entity)| {
            self.appliances
                .contribution(index, &entity.item)
                .unwrap_or_else(|| entity.item.clone())
        }));
        items.extend(self.discovery.items().cloned());
        for index in 0..self.actions.len() {
            if let Some(entity) = self.action_entity(index) {
                let action = &self.actions[index];
                let verb = action.operation.label();
                let label = format!(
                    "{verb}{}",
                    bounded(&entity.title, MAX_TITLE_BYTES - verb.len())
                );
                items.push(json!({"kind":"action","id":action.id,
                    "title":entity.title,"action_id":action.id,
                    "label":label,"params":{},"state_id":entity.item["id"]}));
            }
        }
        if self.link.borrow().connected {
            for input in self.inputs.iter().filter(|input| !input.exhausted) {
                let Some(observation) = &input.observation else {
                    continue;
                };
                let Some(entity) = self
                    .entities
                    .iter()
                    .find(|entity| entity.name == input.action.entity)
                else {
                    continue;
                };
                let verb = input.action.operation.label();
                let label = format!(
                    "{verb}{}",
                    bounded(&entity.title, MAX_TITLE_BYTES - verb.len())
                );
                let constraints = &observation.constraints;
                items.push(json!({"kind":"number_input","id":input.action.id,
                    "title":entity.title,"action_id":input.action.id,"label":label,
                    "state_id":entity.item["id"],"unit":constraints.unit,"input_revision":input.token(),
                    "value_scaled":observation.value,"min_scaled":constraints.min,
                    "max_scaled":constraints.max,"step_scaled":constraints.step,
                    "decimal_places":constraints.decimal_places}));
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

    fn assert_grouped_frame(frame: &Value, config: &crate::config::Validated) {
        let items = frame["items"].as_array().unwrap();
        let controls = config
            .actions()
            .into_iter()
            .chain(config.inputs())
            .collect::<Vec<_>>();
        for item in items {
            if matches!(item["kind"].as_str(), Some("action" | "number_input")) {
                let control = controls
                    .iter()
                    .find(|control| item["action_id"] == control.id)
                    .unwrap();
                let index = config
                    .entities
                    .iter()
                    .position(|entity| *entity == control.entity)
                    .unwrap();
                assert_eq!(item["state_id"], format!("entity-{index}"));
                let state = items
                    .iter()
                    .find(|state| state["id"] == item["state_id"])
                    .unwrap();
                assert!(matches!(
                    state["kind"].as_str(),
                    Some("text" | "metric" | "status")
                ));
            } else {
                assert!(item.get("state_id").is_none());
            }
        }
    }

    #[test]
    fn grouping_uses_explicit_identity_without_changing_admission_or_read_only_rows() {
        let config: crate::config::Configuration = serde_json::from_value(json!({
            "revision":"grouped-identity", "values":{"ha_base_url":"http://localhost",
                "watch_entities":"number.read_only,number.a,button.b,button.a,button.b",
                "action_entities":"button.a,button.b", "number_entities":"number.a",
                "discovery_prefixes":"sensor."}, "secrets":{"ha_token":"fixture"}
        }))
        .unwrap();
        let config = config.validate().unwrap();
        let shared = Book::with_discovery(
            &config.entities,
            &config.actions(),
            &config.inputs(),
            &config.discovery_prefixes,
        );
        let mut book = shared.lock().unwrap();
        book.begin_session();
        book.connected();
        for entity in &config.entities {
            let mut state = number_state("0");
            state["entity_id"] = json!(entity);
            state["attributes"]["friendly_name"] = json!("Same title");
            book.live(entity, Some(&state));
        }
        book.discovery_snapshot(&[json!({"entity_id":"sensor.discovered","state":"0","attributes":{"friendly_name":"Same title"}})]);
        let frame = book.frame();
        assert_grouped_frame(&frame, &config);
        assert_eq!(frame["items"].as_array().unwrap().len(), 9);
        assert_eq!(input_item(&book, "ha-action-0")["state_id"], "entity-3");
        assert_eq!(input_item(&book, "ha-action-1")["state_id"], "entity-2");
        let number = input_item(&book, "ha-number-0-set");
        assert_eq!(number["state_id"], "entity-1");
        let params = json!({"input_revision":number["input_revision"],"value_scaled":0});
        assert!(book.action_target("ha-action-0").is_none());
        assert_eq!(
            book.input_target("ha-number-0-set", &params).err(),
            Some("unavailable")
        );
        book.mark_published();
        assert_eq!(
            book.action_target("ha-action-0").unwrap().entity,
            "button.a"
        );
        assert_eq!(
            book.action_target("ha-action-1").unwrap().entity,
            "button.b"
        );
        assert_eq!(
            book.input_target("ha-number-0-set", &params)
                .unwrap()
                .0
                .entity,
            "number.a"
        );
        for id in [
            "entity-0",
            "entity-1",
            "entity-2",
            "entity-3",
            "discovery-0",
        ] {
            assert!(book.action_target(id).is_none());
            assert_eq!(book.input_target(id, &params).err(), Some("invalid_action"));
        }
        book.disconnected();
        assert_grouped_frame(&book.frame(), &config);
        assert_eq!(book.frame()["items"].as_array().unwrap().len(), 5);
        assert_eq!(
            book.input_target("ha-number-0-set", &params).err(),
            Some("unavailable")
        );
        book.begin_session();
        book.connected();
        book.initial("number.a", Some(&number_state("0")));
        assert_grouped_frame(&book.frame(), &config);
        assert_eq!(
            input_item(&book, "ha-number-0-set")["state_id"],
            number["state_id"]
        );
        assert_ne!(
            input_item(&book, "ha-number-0-set")["input_revision"],
            number["input_revision"]
        );
        assert!(book.action_target("ha-action-0").is_none());
    }

    fn appliance_configuration(
        watch: &str,
        running: &str,
        duration: &str,
    ) -> crate::config::Validated {
        let config: crate::config::Configuration = serde_json::from_value(json!({
            "revision":"dishwasher", "values":{"ha_base_url":"http://localhost", "watch_entities":watch,
                "dishwasher_running_entity":running, "dishwasher_duration_entity":duration},
            "secrets":{"ha_token":"fixture"}
        })).unwrap();
        config.validate().unwrap()
    }

    fn appliance_book(config: &crate::config::Validated) -> Shared {
        Book::with_appliances(
            &config.entities,
            &config.actions(),
            &config.inputs(),
            &config.discovery_prefixes,
            &config.appliances,
        )
    }

    fn appliance_state(entity: &str, value: &str) -> Value {
        json!({"entity_id":entity,"state":value,"attributes":{"friendly_name":"Shared title","unit_of_measurement":"hours"}})
    }

    #[test]
    fn dishwasher_refreshes_from_both_sources_without_changing_raw_cards_or_source_freshness() {
        let config =
            appliance_configuration("sensor.runtime", "binary_sensor.running", "sensor.runtime");
        let shared = appliance_book(&config);
        let mut book = shared.lock().unwrap();
        book.begin_session();
        book.connected();
        book.initial(
            "binary_sensor.running",
            Some(&appliance_state("binary_sensor.running", "on")),
        );
        book.initial(
            "sensor.runtime",
            Some(&appliance_state("sensor.runtime", "1.50")),
        );
        assert_eq!(
            book.frame()["items"][2],
            json!({"kind":"text","id":"entity-1","title":"Shared title","text":"State: Running\nRuntime since midnight: 1.50"})
        );
        assert_eq!(book.entities[1].item["text"], "on");
        assert_eq!(book.frame()["items"][1], book.entities[0].item);
        let raw_runtime = book.entities[0].item.clone();
        let revision = book.revision;
        book.live(
            "sensor.runtime",
            Some(&appliance_state("sensor.runtime", "1.500")),
        );
        assert_eq!(
            book.entities[0].item, raw_runtime,
            "generic numeric state remains equal"
        );
        assert_ne!(
            book.revision, revision,
            "changed runtime spelling still republishes summary"
        );
        assert!(
            !book.entities[1].live_seen,
            "a duration event does not mark primary live"
        );
        book.initial(
            "binary_sensor.running",
            Some(&appliance_state("binary_sensor.running", "off")),
        );
        assert_eq!(
            book.frame()["items"][2]["text"],
            "State: Idle\nRuntime since midnight: 1.500"
        );
        book.live(
            "binary_sensor.running",
            Some(&appliance_state("binary_sensor.running", "on")),
        );
        let frame = book.frame();
        book.initial(
            "binary_sensor.running",
            Some(&appliance_state("binary_sensor.running", "off")),
        );
        book.initial(
            "sensor.runtime",
            Some(&appliance_state("sensor.runtime", "99")),
        );
        assert_eq!(
            book.frame(),
            frame,
            "both source guards reject stale initial state"
        );
        book.live("sensor.runtime", None);
        assert_eq!(book.frame()["items"][2]["text"], "State: Running");
        assert_eq!(book.frame()["items"][1]["value"], "Unavailable");
        book.initial(
            "sensor.runtime",
            Some(&appliance_state("sensor.runtime", "99")),
        );
        assert_eq!(book.frame()["items"][2]["text"], "State: Running");
        book.live(
            "sensor.runtime",
            Some(&appliance_state("sensor.runtime", "02:15:00")),
        );
        let mut renamed = appliance_state("binary_sensor.running", "Running");
        renamed["attributes"]["friendly_name"] = json!("Kitchen dishwasher");
        book.live("binary_sensor.running", Some(&renamed));
        assert_eq!(book.frame()["items"][2]["id"], "entity-1");
        assert_eq!(book.frame()["items"][2]["title"], "Kitchen dishwasher");
        assert_eq!(
            book.frame()["items"][2]["text"],
            "State: Running\nRuntime since midnight: 02:15:00"
        );
    }

    #[test]
    fn dishwasher_role_state_and_runtime_clear_independently_and_across_sessions() {
        let config = appliance_configuration("", "binary_sensor.running", "sensor.runtime");
        let shared = appliance_book(&config);
        let mut book = shared.lock().unwrap();
        book.begin_session();
        book.connected();
        book.live(
            "sensor.runtime",
            Some(&appliance_state("sensor.runtime", "01:23:45")),
        );
        assert_eq!(
            book.frame()["items"][1]["value"],
            "Waiting",
            "duration cannot invent primary status"
        );
        book.initial(
            "binary_sensor.running",
            Some(&appliance_state("binary_sensor.running", "on")),
        );
        for (value, expected) in [(" UNKNOWN ", "Unknown"), ("unavailable", "Unavailable")] {
            book.live(
                "binary_sensor.running",
                Some(&appliance_state("binary_sensor.running", value)),
            );
            assert_eq!(book.frame()["items"][1]["value"], expected);
            assert!(book.frame()["items"][1].get("text").is_none());
        }
        book.live(
            "binary_sensor.running",
            Some(&appliance_state("binary_sensor.running", "on")),
        );
        book.live("binary_sensor.running", None);
        assert_eq!(book.frame()["items"][1]["value"], "Unavailable");
        book.initial(
            "binary_sensor.running",
            Some(&appliance_state("binary_sensor.running", "on")),
        );
        assert_eq!(book.frame()["items"][1]["value"], "Unavailable");
        book.live(
            "binary_sensor.running",
            Some(&appliance_state("binary_sensor.running", "on")),
        );
        book.disconnected();
        assert!(book.frame()["items"][1].get("text").is_none());
        assert_eq!(book.frame()["items"][2]["value"], "Unavailable");
        book.begin_session();
        book.connected();
        book.initial(
            "binary_sensor.running",
            Some(&appliance_state("binary_sensor.running", "off")),
        );
        assert_eq!(
            book.frame()["items"][1]["text"],
            "State: Idle",
            "old runtime is not retained after reconnect"
        );
        book.initial(
            "sensor.runtime",
            Some(&appliance_state("sensor.runtime", "00:00:00")),
        );
        assert_eq!(
            book.frame()["items"][1]["text"],
            "State: Idle\nRuntime since midnight: 00:00:00"
        );
        book.authentication_rejected();
        assert!(book.frame()["items"][1].get("text").is_none());
        book.begin_session();
        book.connected();
        book.initial(
            "binary_sensor.running",
            Some(&appliance_state("binary_sensor.running", "on")),
        );
        assert_eq!(book.frame()["items"][1]["text"], "State: Running");
    }

    #[test]
    fn dishwasher_profiles_do_not_add_or_relax_explicit_action_or_numeric_grants() {
        let config: crate::config::Configuration = serde_json::from_value(json!({
            "revision":"dishwasher-authority", "values":{"ha_base_url":"http://localhost",
                "dishwasher_running_entity":"switch.running", "dishwasher_duration_entity":"number.runtime",
                "binary_entities":"switch.running", "number_entities":"number.runtime"}, "secrets":{"ha_token":"fixture"}
        })).unwrap();
        let config = config.validate().unwrap();
        let shared = appliance_book(&config);
        let mut book = shared.lock().unwrap();
        book.connected();
        let mut runtime = number_state("0");
        runtime["entity_id"] = json!("number.runtime");
        book.live("number.runtime", Some(&runtime));
        book.live(
            "switch.running",
            Some(&appliance_state("switch.running", " ON ")),
        );
        assert_eq!(
            book.frame()["items"][1]["text"],
            "State: Running\nRuntime since midnight: 0"
        );
        book.mark_published();
        assert!(
            book.action_target("ha-binary-0-on").is_none(),
            "summary normalization must not relax raw binary eligibility"
        );
        let input = input_item(&book, "ha-number-0-set");
        let params = json!({"input_revision":input["input_revision"],"value_scaled":0});
        assert!(book.input_target("ha-number-0-set", &params).is_ok());
        book.live(
            "switch.running",
            Some(&appliance_state("switch.running", "on")),
        );
        assert!(
            book.action_target("ha-binary-0-on").is_none(),
            "new raw availability still needs publication"
        );
        book.mark_published();
        assert_eq!(
            book.action_target("ha-binary-0-on").unwrap().entity,
            "switch.running"
        );
        assert_eq!(input_item(&book, "ha-binary-0-on")["state_id"], "entity-0");
        assert_eq!(input_item(&book, "ha-number-0-set")["state_id"], "entity-1");
        runtime["state"] = json!("0.0");
        book.live("number.runtime", Some(&runtime));
        assert_eq!(
            book.frame()["items"][1]["text"],
            "State: Running\nRuntime since midnight: 0.0"
        );
        assert_eq!(
            input_item(&book, "ha-number-0-set")["input_revision"],
            input["input_revision"]
        );
        assert!(book.input_target("ha-number-0-set", &params).is_ok());
        assert!(book.action_target("ha-binary-0-on").is_some());
        for id in ["entity-0", "entity-1", "switch.running", "number.runtime"] {
            assert!(book.action_target(id).is_none());
            assert_eq!(book.input_target(id, &params).err(), Some("invalid_action"));
        }
        book.live(
            "switch.running",
            Some(&appliance_state("switch.running", "unavailable")),
        );
        assert!(book.action_target("ha-binary-0-on").is_none());
        assert!(book.input_target("ha-number-0-set", &params).is_ok());
    }

    #[test]
    fn dishwasher_identity_guards_and_missing_profile_preserve_generic_and_discovered_cards() {
        let config =
            appliance_configuration("sensor.runtime", "binary_sensor.running", "sensor.runtime");
        let shared = appliance_book(&config);
        let mut book = shared.lock().unwrap();
        book.connected();
        book.live(
            "binary_sensor.running",
            Some(&appliance_state("binary_sensor.running", "on")),
        );
        book.live(
            "sensor.runtime",
            Some(&appliance_state("sensor.runtime", "2h")),
        );
        book.live(
            "sensor.runtime",
            Some(&appliance_state("sensor.other", "99h")),
        );
        assert_eq!(book.frame()["items"][2]["text"], "State: Running");
        book.live(
            "binary_sensor.running",
            Some(&appliance_state("binary_sensor.other", "off")),
        );
        assert_eq!(book.frame()["items"][2]["value"], "Unavailable");
        let shared = Book::with_discovery(&config.entities, &[], &[], &["sensor.".into()]);
        let mut unconfigured = shared.lock().unwrap();
        unconfigured.begin_session();
        unconfigured.connected();
        for name in &config.entities {
            let state = appliance_state(name, "on");
            unconfigured.live(name, Some(&state));
        }
        unconfigured
            .discovery_snapshot(&[appliance_state("sensor.dishwasher_runtime", "01:23:45")]);
        let frame = unconfigured.frame();
        for (index, name) in config.entities.iter().enumerate() {
            assert_eq!(
                frame["items"][index + 1],
                contribution(index, name, Some(&appliance_state(name, "on")))
            );
        }
        assert_eq!(frame["items"][3]["text"], "01:23:45");
        assert!(frame["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item.get("action_id").is_none()));
        let no_duration = appliance_configuration("", "sensor.running", "");
        let shared = appliance_book(&no_duration);
        let mut single = shared.lock().unwrap();
        single.connected();
        single.live(
            "sensor.running",
            Some(&appliance_state("sensor.running", "Paused")),
        );
        assert_eq!(single.frame()["items"].as_array().unwrap().len(), 2);
        assert_eq!(single.frame()["items"][1]["text"], "State: Paused");
    }

    #[tokio::test]
    async fn dishwasher_roles_share_the_existing_64_item_frame_and_preserve_whole_escaped_values() {
        let buttons = (0..16)
            .map(|index| format!("button.e{index}"))
            .collect::<Vec<_>>();
        let media = (0..4)
            .map(|index| format!("media_player.e{index}"))
            .collect::<Vec<_>>();
        let watch = (0..11)
            .map(|index| format!("sensor.e{index}"))
            .collect::<Vec<_>>();
        let config: crate::config::Configuration = serde_json::from_value(json!({
            "revision":"dishwasher-capacity", "values":{"ha_base_url":"http://localhost", "watch_entities":watch.join(","),
                "action_entities":buttons.join(","), "media_player_entities":media.join(","), "cover_entities":"cover.a",
                "dishwasher_running_entity":"sensor.e0", "dishwasher_duration_entity":"sensor.e1"}, "secrets":{"ha_token":"fixture"}
        })).unwrap();
        let config = config.validate().unwrap();
        let shared = appliance_book(&config);
        let frame = {
            let mut book = shared.lock().unwrap();
            book.connected();
            for name in &config.entities {
                let value = if name == "cover.a" {
                    "opening".to_owned()
                } else if name == "sensor.e0" || name == "sensor.e1" {
                    "\\\"".repeat(64)
                } else {
                    "\\\"".repeat(256)
                };
                book.live(name, Some(&json!({"entity_id":name,"state":value,"attributes":{"friendly_name":"\\\"".repeat(64),"supported_features":11}})));
            }
            book.frame()
        };
        let items = frame["items"].as_array().unwrap();
        assert_eq!(config.entities.len(), 32);
        assert_eq!(items.len(), 64);
        assert_eq!(
            items.iter().filter(|item| item["kind"] == "action").count(),
            31
        );
        assert_eq!(
            items[1]["text"],
            format!(
                "State: {}\nRuntime since midnight: {}",
                "\\\"".repeat(64),
                "\\\"".repeat(64)
            )
        );
        assert_eq!(items[2]["text"], "\\\"".repeat(64));
        assert!(items[1]["text"].as_str().unwrap().len() <= 512);
        assert_grouped_frame(&frame, &config);
        assert!(
            serde_json::to_vec(&frame).unwrap().len() < inverter_worker_protocol::MAX_FRAME_BYTES
        );
        Output::with_writer(std::io::sink())
            .send(frame)
            .await
            .unwrap();
    }

    #[test]
    fn shared_laundry_duration_refreshes_both_overlays_after_each_source_freshness_guard() {
        let config: crate::config::Configuration = serde_json::from_value(json!({
            "revision":"shared-laundry", "values":{"ha_base_url":"http://localhost", "watch_entities":"sensor.shared",
                "dishwasher_running_entity":"binary_sensor.running", "dishwasher_duration_entity":"sensor.shared",
                "washer_remaining_entity":"sensor.shared", "dryer_remaining_entity":"sensor.dryer"}, "secrets":{"ha_token":"fixture"}
        })).unwrap();
        let config = config.validate().unwrap();
        let shared = appliance_book(&config);
        let mut book = shared.lock().unwrap();
        book.begin_session();
        book.connected();
        book.initial(
            "binary_sensor.running",
            Some(&appliance_state("binary_sensor.running", "on")),
        );
        book.initial(
            "sensor.shared",
            Some(&appliance_state("sensor.shared", "1.50")),
        );
        book.initial("sensor.dryer", Some(&appliance_state("sensor.dryer", "10")));
        assert_eq!(book.frame()["items"][1]["text"], "Remaining time: 1.50");
        assert_eq!(
            book.frame()["items"][2]["text"],
            "State: Running\nRuntime since midnight: 1.50"
        );
        let raw = book.entities[0].item.clone();
        let revision = book.revision;
        book.live(
            "sensor.shared",
            Some(&appliance_state("sensor.shared", "1.500")),
        );
        assert_eq!(book.entities[0].item, raw, "generic numeric state is equal");
        assert_ne!(book.revision, revision);
        assert_eq!(book.frame()["items"][1]["text"], "Remaining time: 1.500");
        assert_eq!(
            book.frame()["items"][2]["text"],
            "State: Running\nRuntime since midnight: 1.500"
        );
        assert_eq!(book.frame()["items"][3]["text"], "Remaining time: 10");
        assert!(
            !book.entities[1].live_seen,
            "shared runtime cannot mark dishwasher primary live"
        );
        book.initial(
            "binary_sensor.running",
            Some(&appliance_state("binary_sensor.running", "off")),
        );
        assert_eq!(
            book.frame()["items"][2]["text"],
            "State: Idle\nRuntime since midnight: 1.500"
        );
        book.live(
            "binary_sensor.running",
            Some(&appliance_state("binary_sensor.running", "on")),
        );
        let frame = book.frame();
        book.initial(
            "sensor.shared",
            Some(&appliance_state("sensor.shared", "99")),
        );
        book.initial(
            "binary_sensor.running",
            Some(&appliance_state("binary_sensor.running", "off")),
        );
        assert_eq!(book.frame(), frame);
        book.live(
            "sensor.shared",
            Some(&appliance_state("sensor.other", "99")),
        );
        assert_eq!(book.frame()["items"][1]["value"], "Unavailable");
        assert_eq!(book.frame()["items"][2]["text"], "State: Running");
        book.live(
            "sensor.shared",
            Some(&appliance_state("sensor.shared", "2h")),
        );
        book.live("sensor.shared", None);
        book.initial(
            "sensor.shared",
            Some(&appliance_state("sensor.shared", "99")),
        );
        assert_eq!(book.frame()["items"][1]["value"], "Unavailable");
        assert_eq!(book.frame()["items"][2]["text"], "State: Running");
        book.disconnected();
        for item in book.frame()["items"].as_array().unwrap().iter().skip(1) {
            assert_eq!(item["value"], "Unavailable");
            assert!(item.get("text").is_none());
        }
        book.begin_session();
        book.connected();
        book.initial(
            "binary_sensor.running",
            Some(&appliance_state("binary_sensor.running", "on")),
        );
        assert_eq!(book.frame()["items"][2]["text"], "State: Running");
        assert_eq!(book.frame()["items"][1]["value"], "Waiting");
        assert_eq!(book.frame()["items"][3]["value"], "Waiting");
        book.initial(
            "sensor.shared",
            Some(&appliance_state("sensor.shared", "0")),
        );
        assert_eq!(book.frame()["items"][1]["text"], "Remaining time: 0");
        assert_eq!(
            book.frame()["items"][2]["text"],
            "State: Running\nRuntime since midnight: 0"
        );
        book.authentication_rejected();
        assert_eq!(book.frame()["items"][1]["value"], "Unavailable");
    }

    #[test]
    fn laundry_overlays_preserve_raw_control_authority_numeric_revisions_and_source_titles() {
        let config: crate::config::Configuration = serde_json::from_value(json!({
            "revision":"laundry-authority", "values":{"ha_base_url":"http://localhost",
                "washer_remaining_entity":"number.a", "dryer_remaining_entity":"switch.dryer",
                "number_entities":"number.a", "binary_entities":"switch.dryer"}, "secrets":{"ha_token":"fixture"}
        })).unwrap();
        let config = config.validate().unwrap();
        let shared = appliance_book(&config);
        let mut book = shared.lock().unwrap();
        book.connected();
        book.live("number.a", Some(&number_state("0")));
        book.live(
            "switch.dryer",
            Some(&appliance_state("switch.dryer", "off")),
        );
        assert_eq!(book.frame()["items"][1]["value"], "Idle");
        assert_eq!(book.frame()["items"][2]["text"], "Remaining time: 0");
        assert_eq!(book.entities[0].item["text"], "off");
        assert_eq!(book.entities[1].item["value"], 0.0);
        assert!(book.action_target("ha-binary-0-on").is_none());
        book.mark_published();
        assert_eq!(
            book.action_target("ha-binary-0-on").unwrap().entity,
            "switch.dryer"
        );
        assert_eq!(input_item(&book, "ha-binary-0-on")["state_id"], "entity-0");
        let input = input_item(&book, "ha-number-0-set");
        let params = json!({"input_revision":input["input_revision"],"value_scaled":0});
        assert!(book.input_target("ha-number-0-set", &params).is_ok());
        let revision = book.revision;
        let mut number = number_state("0.00");
        number["attributes"]["friendly_name"] = json!("Washer remaining");
        book.live("number.a", Some(&number));
        assert_ne!(book.revision, revision);
        assert_eq!(book.frame()["items"][2]["text"], "Remaining time: 0.00");
        assert_eq!(book.frame()["items"][2]["title"], "Washer remaining");
        assert_eq!(
            input_item(&book, "ha-number-0-set")["input_revision"],
            input["input_revision"]
        );
        assert!(book.input_target("ha-number-0-set", &params).is_ok());
        book.live(
            "switch.dryer",
            Some(&appliance_state("switch.dryer", " OFF ")),
        );
        assert_eq!(book.frame()["items"][1]["value"], "Idle");
        assert!(
            book.action_target("ha-binary-0-on").is_none(),
            "normalizing profile status cannot normalize binary command grants"
        );
        for id in ["entity-0", "entity-1", "number.a", "switch.dryer"] {
            assert!(book.action_target(id).is_none());
            assert_eq!(book.input_target(id, &params).err(), Some("invalid_action"));
        }
        book.live("number.a", None);
        assert_eq!(book.frame()["items"][2]["value"], "Unavailable");
        assert_eq!(
            book.input_target("ha-number-0-set", &params).err(),
            Some("unavailable")
        );
    }

    #[test]
    fn absent_laundry_profiles_and_unconfigured_discoveries_keep_their_generic_state_projection() {
        let config = appliance_configuration("sensor.washer,sensor.dryer", "", "");
        let shared = appliance_book(&config);
        let mut book = shared.lock().unwrap();
        book.connected();
        book.live(
            "sensor.washer",
            Some(&appliance_state("sensor.washer", "0")),
        );
        book.live(
            "sensor.dryer",
            Some(&appliance_state("sensor.dryer", "idle")),
        );
        assert_eq!(book.frame()["items"][1]["kind"], "metric");
        assert_eq!(book.frame()["items"][2]["text"], "idle");
        let config: crate::config::Configuration = serde_json::from_value(json!({
            "revision":"laundry-discovery", "values":{"ha_base_url":"http://localhost",
                "washer_remaining_entity":"sensor.washer", "discovery_prefixes":"sensor."}, "secrets":{"ha_token":"fixture"}
        })).unwrap();
        let config = config.validate().unwrap();
        let shared = appliance_book(&config);
        let mut book = shared.lock().unwrap();
        book.begin_session();
        book.connected();
        book.initial(
            "sensor.washer",
            Some(&appliance_state("sensor.washer", "00:30")),
        );
        book.discovery_snapshot(&[
            appliance_state("sensor.washer", "99"),
            appliance_state("sensor.dryer", "00:20"),
        ]);
        let frame = book.frame();
        assert_eq!(frame["items"][1]["text"], "Remaining time: 00:30");
        assert_eq!(frame["items"][2]["text"], "00:20");
        assert!(frame["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item.get("action_id").is_none()));
        let revision = book.revision;
        assert_eq!(
            book.frame(),
            frame,
            "rendering does not tick or infer countdown progress"
        );
        assert_eq!(book.revision, revision);
    }

    #[tokio::test]
    async fn dishwasher_and_laundry_share_all_existing_slots_with_complete_escaped_values() {
        let buttons = (0..16)
            .map(|index| format!("button.e{index}"))
            .collect::<Vec<_>>();
        let media = (0..4)
            .map(|index| format!("media_player.e{index}"))
            .collect::<Vec<_>>();
        let watch = (0..11)
            .map(|index| format!("sensor.e{index}"))
            .collect::<Vec<_>>();
        let config: crate::config::Configuration = serde_json::from_value(json!({
            "revision":"laundry-frame", "values":{"ha_base_url":"http://localhost", "watch_entities":watch.join(","),
                "action_entities":buttons.join(","), "media_player_entities":media.join(","), "cover_entities":"cover.a",
                "dishwasher_running_entity":"sensor.e0", "dishwasher_duration_entity":"sensor.e1",
                "washer_remaining_entity":"sensor.e1", "dryer_remaining_entity":"sensor.e2"}, "secrets":{"ha_token":"fixture"}
        })).unwrap();
        let config = config.validate().unwrap();
        let shared = appliance_book(&config);
        let literal = "\\\"".repeat(64);
        let frame = {
            let mut book = shared.lock().unwrap();
            book.connected();
            for name in &config.entities {
                let state = if name == "cover.a" {
                    "opening".to_owned()
                } else if ["sensor.e0", "sensor.e1", "sensor.e2"].contains(&name.as_str()) {
                    literal.clone()
                } else {
                    "\\\"".repeat(256)
                };
                book.live(name, Some(&json!({"entity_id":name,"state":state,"attributes":{"friendly_name":literal,"supported_features":11}})));
            }
            book.frame()
        };
        assert_eq!(config.entities.len(), 32);
        let items = frame["items"].as_array().unwrap();
        assert_eq!(items.len(), 64);
        assert_eq!(
            items.iter().filter(|item| item["kind"] == "action").count(),
            31
        );
        assert_eq!(
            items[1]["text"],
            format!("State: {literal}\nRuntime since midnight: {literal}")
        );
        for item in &items[2..4] {
            assert_eq!(item["text"], format!("Remaining time: {literal}"));
            assert!(item["text"].as_str().unwrap().len() <= 512);
            assert!(item.get("action_id").is_none());
        }
        assert_grouped_frame(&frame, &config);
        assert!(
            serde_json::to_vec(&frame).unwrap().len() < inverter_worker_protocol::MAX_FRAME_BYTES
        );
        Output::with_writer(std::io::sink())
            .send(frame)
            .await
            .unwrap();
    }

    fn weather_state(condition: &str, temperature: Value) -> Value {
        json!({"entity_id":"weather.home","state":condition,"attributes":{
            "friendly_name":"Home weather","temperature":temperature,"temperature_unit":"°C",
            "forecast":[{"datetime":"2026-09-16","condition":"cloudy","temperature":23,"templow":14}]
        }})
    }

    #[test]
    fn weather_initial_and_attribute_only_live_updates_preserve_identity_and_current_state() {
        let shared = Book::new(&["weather.home".into()], &[], &[]);
        let mut book = shared.lock().unwrap();
        book.begin_session();
        book.connected();
        book.initial("weather.home", Some(&weather_state("sunny", json!(21))));
        assert_eq!(
            book.frame()["items"][1],
            json!({"kind":"text","id":"entity-0","title":"Home weather",
            "text":"Condition: sunny; Temperature: 21 °C\nForecast: 2026-09-16, Condition: cloudy, High: 23 °C, Low: 14 °C"})
        );
        let revision = book.revision;
        let mut current = weather_state("sunny", json!(18));
        current["attributes"]["forecast"][0]["templow"] = json!(10);
        book.live("weather.home", Some(&current));
        assert_ne!(book.revision, revision);
        let frame = book.frame();
        assert!(frame["items"][1]["text"]
            .as_str()
            .unwrap()
            .contains("Temperature: 18 °C"));
        assert!(frame["items"][1]["text"]
            .as_str()
            .unwrap()
            .ends_with("Low: 10 °C"));
        book.initial("weather.home", Some(&weather_state("rainy", json!(30))));
        assert_eq!(
            book.frame(),
            frame,
            "late initial weather cannot replace live details"
        );
        let revision = book.revision;
        current["attributes"]["private_metadata"] = json!("must not appear");
        book.live("weather.home", Some(&current));
        assert_eq!(
            book.revision, revision,
            "unprojected attributes do not trigger publication"
        );
        book.mark_published();
        for id in ["entity-0", "weather.home", "ha-action-0"] {
            assert!(book.action_target(id).is_none());
        }
    }

    #[test]
    fn weather_unknown_unavailable_deletion_and_disconnect_withdraw_all_details() {
        let shared = Book::new(&["weather.home".into()], &[], &[]);
        let mut book = shared.lock().unwrap();
        book.begin_session();
        book.connected();
        for (state, expected) in [("unknown", "Unknown"), ("unavailable", "Unavailable")] {
            book.live("weather.home", Some(&weather_state("sunny", json!(21))));
            book.live("weather.home", Some(&weather_state(state, json!(21))));
            assert_eq!(
                book.frame()["items"][1],
                json!({"kind":"status","id":"entity-0","title":"Home weather","value":expected,"tone":"neutral"})
            );
        }
        book.live("weather.home", Some(&weather_state("sunny", json!(21))));
        book.live("weather.home", None);
        assert_eq!(book.frame()["items"][1]["value"], "Unavailable");
        assert!(book.frame()["items"][1].get("text").is_none());
        book.initial("weather.home", Some(&weather_state("rainy", json!(22))));
        assert_eq!(book.frame()["items"][1]["value"], "Unavailable");
        book.live("weather.home", Some(&weather_state("rainy", json!(22))));
        book.disconnected();
        assert_eq!(book.frame()["items"][1]["value"], "Unavailable");
        assert!(book.frame()["items"][1].get("text").is_none());
        book.begin_session();
        book.connected();
        assert_eq!(book.frame()["items"][1]["value"], "Waiting");
        book.initial("weather.home", Some(&weather_state("snowy", json!(-5))));
        assert!(book.frame()["items"][1]["text"]
            .as_str()
            .unwrap()
            .starts_with("Condition: snowy; Temperature: -5 °C"));
        book.authentication_rejected();
        assert!(book.frame()["items"][1].get("text").is_none());
    }

    #[test]
    fn weather_projection_is_exact_domain_only_and_never_creates_discovery_or_control_grants() {
        for (entity, observed, expected) in [
            (
                "sensor.weather",
                "21",
                json!({"kind":"metric","id":"entity-0","title":"Home weather","value":21.0,"unit":"kW"}),
            ),
            (
                "button.weather",
                "sunny",
                json!({"kind":"text","id":"entity-0","title":"Home weather","text":"sunny"}),
            ),
            (
                "weathered.home",
                "sunny",
                json!({"kind":"text","id":"entity-0","title":"Home weather","text":"sunny"}),
            ),
            (
                "weather.home",
                "21",
                json!({"kind":"text","id":"entity-0","title":"Home weather","text":"Condition: 21; Temperature: 21 °C\nForecast: 2026-09-16, Condition: cloudy, High: 23 °C, Low: 14 °C"}),
            ),
        ] {
            let mut state = weather_state(observed, json!(21));
            state["entity_id"] = json!(entity);
            state["attributes"]["unit_of_measurement"] = json!("kW");
            assert_eq!(contribution(0, entity, Some(&state)), expected);
        }
        let shared = Book::with_discovery(&["weather.home".into()], &[], &[], &["sensor.".into()]);
        let mut book = shared.lock().unwrap();
        book.begin_session();
        book.connected();
        book.discovery_snapshot(&[
            weather_state("sunny", json!(21)),
            json!({"entity_id":"weather.other","state":"sunny"}),
        ]);
        assert_eq!(book.frame()["items"].as_array().unwrap().len(), 2);
        let mut malformed = weather_state("sunny", json!(21));
        malformed["entity_id"] = json!("weather.other");
        book.live("weather.home", Some(&malformed));
        assert_eq!(book.frame()["items"][1]["value"], "Unavailable");
        malformed["entity_id"] = json!("weather.home");
        malformed["state"] = json!(false);
        book.live("weather.home", Some(&malformed));
        assert_eq!(book.frame()["items"][1]["value"], "Unavailable");
        book.mark_published();
        assert!(book.action_target("entity-0").is_none());
    }

    #[tokio::test]
    async fn sixty_four_weather_items_stay_within_the_actual_utf8_and_escaped_frame_limits() {
        let entities = (0..crate::config::MAX_ENTITIES)
            .map(|index| format!("weather.house_{index}"))
            .collect::<Vec<_>>();
        let shared = Book::new(&entities, &[], &[]);
        let frame = {
            let mut book = shared.lock().unwrap();
            book.connected();
            for (index, name) in entities.iter().enumerate() {
                let mut state = weather_state(&"\\\"".repeat(64), json!(21));
                state["entity_id"] = json!(name);
                state["attributes"]["friendly_name"] = json!("\\\"".repeat(128));
                state["attributes"]["forecast"] = json!((0..5).map(|day| json!({
                    "datetime":format!("2026-09-{:02}", day + 16), "condition":"\\\"".repeat(64),
                    "temperature":12345678901234567890123456789012_i128,"templow":-10
                })).collect::<Vec<_>>());
                book.live(name, Some(&state));
                assert_eq!(
                    book.frame()["items"][index + 1]["id"],
                    format!("entity-{index}")
                );
            }
            book.frame()
        };
        let items = frame["items"].as_array().unwrap();
        assert_eq!(items.len(), 65);
        for item in &items[1..] {
            assert_eq!(item["kind"], "text");
            let text = item["text"].as_str().unwrap();
            assert!(
                text.len() <= MAX_TEXT_BYTES
                    && text
                        .chars()
                        .all(|character| character == '\n' || !character.is_control())
            );
            assert!(item["title"].as_str().unwrap().len() <= 128);
            assert!(item.get("state_id").is_none());
            assert!(item.get("action_id").is_none());
        }
        assert!(
            serde_json::to_vec(&frame).unwrap().len() < inverter_worker_protocol::MAX_FRAME_BYTES
        );
        Output::with_writer(std::io::sink())
            .send(frame)
            .await
            .unwrap();
    }

    #[test]
    fn late_initial_state_cannot_replace_live_or_deleted_state() {
        let state = Book::new(&["sensor.a".into(), "sensor.b".into()], &[], &[]);
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
        let state = Book::new(&entities, &[], &[]);
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
        let state = Book::new(&names, &configured_actions(&names[..2], &[], &[], &[]), &[]);
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
            json!({"kind":"action","id":"ha-action-0","title":"A name","action_id":"ha-action-0","label":"Press A name","params":{},"state_id":"entity-0"})
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
        let state = Book::new(&names, &configured_actions(&names, &[], &[], &[]), &[]);
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
        let actions = configured_actions(&names[..2], &names[2..3], &[], &[]);
        let state = Book::new(&names, &actions, &[]);
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
                json!({"kind":"action","id":format!("ha-media-0-{verb}"),"title":"Living room","action_id":format!("ha-media-0-{verb}"),"label":label,"params":{},"state_id":"entity-2"})
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

    #[test]
    fn binary_actions_preserve_observed_state_and_offer_both_absolute_commands() {
        let names = [
            "switch.desk".into(),
            "input_boolean.do_not_supply_charger".into(),
            "light.room".into(),
            "switch.read_only".into(),
        ];
        let state = Book::new(&names, &configured_actions(&[], &[], &names[..3], &[]), &[]);
        let mut book = state.lock().unwrap();
        for name in &names {
            book.initial(name, Some(&json!({"entity_id":name,"state":"off","attributes":{"friendly_name":"Room control"}})));
        }
        assert_eq!(book.frame()["items"].as_array().unwrap().len(), 5);
        book.connected();
        for index in 0..3 {
            assert!(book
                .action_target(&format!("ha-binary-{index}-on"))
                .is_none());
        }
        book.mark_published();
        for observed in ["off", "on"] {
            for name in &names {
                book.live(name, Some(&json!({"entity_id":name,"state":observed,"attributes":{"friendly_name":"Room control"}})));
            }
            let frame = book.frame();
            let items = frame["items"].as_array().unwrap();
            assert_eq!(items.len(), 11);
            for item in &items[1..5] {
                assert_eq!(item["kind"], "text");
                assert_eq!(item["text"], observed);
            }
            for (index, name) in names[..3].iter().enumerate() {
                for (offset, verb) in ["on", "off"].into_iter().enumerate() {
                    let id = format!("ha-binary-{index}-{verb}");
                    assert_eq!(
                        items[5 + index * 2 + offset],
                        json!({
                            "kind":"action","id":id,"action_id":id,"title":"Room control",
                            "label":format!("Turn {verb} Room control"),"params":{},"state_id":format!("entity-{index}")
                        })
                    );
                    assert_eq!(book.action_target(&id).unwrap().entity, *name);
                }
            }
            assert_eq!(
                book.frame(),
                frame,
                "admission never synthesizes entity state"
            );
        }
        for id in [
            "ha-binary-00-on",
            "ha-binary-0-toggle",
            "ha-binary-0-turn_on",
            "ha-binary-3-on",
        ] {
            assert!(book.action_target(id).is_none());
        }
        book.live(&names[0], Some(&json!({"entity_id":names[0],"state":"on","attributes":{"friendly_name":"🌞\n".repeat(128)}})));
        let frame = book.frame();
        for (index, prefix) in ["Turn on 🌞", "Turn off 🌞"].into_iter().enumerate() {
            let label = frame["items"][5 + index]["label"].as_str().unwrap();
            assert!(label.starts_with(prefix));
            assert!(label.len() <= 128);
            assert!(!label.contains('\n'));
        }
    }

    #[test]
    fn binary_withdrawal_checks_raw_state_and_reconnection_requires_fresh_observation() {
        let names = ["switch.desk".into()];
        let state = Book::new(&names, &configured_actions(&[], &[], &names, &[]), &[]);
        let mut book = state.lock().unwrap();
        book.connected();
        book.mark_published();
        assert!(book.action_target("ha-binary-0-on").is_none());
        let known = json!({"entity_id":"switch.desk","state":"off"});
        let mut invalid = [
            "unknown",
            "unavailable",
            "ON",
            "off ",
            "off\n",
            " on",
            "true",
            "1",
            "playing",
            "",
        ]
        .into_iter()
        .map(|value| Some(json!({"entity_id":"switch.desk","state":value})))
        .collect::<Vec<_>>();
        invalid.extend([
            None,
            Some(json!({"entity_id":"switch.desk"})),
            Some(json!({"entity_id":"switch.desk","state":null})),
            Some(json!({"entity_id":"switch.desk","state":true})),
            Some(json!({"entity_id":"switch.desk","state":0})),
            Some(json!({"entity_id":"switch.desk","state":{"value":"off"}})),
            Some(json!({"entity_id":"switch.desk","state":["off"]})),
            Some(json!({"entity_id":"light.other","state":"off"})),
        ]);
        for value in invalid {
            book.live("switch.desk", Some(&known));
            assert!(book.action_target("ha-binary-0-on").is_none());
            book.mark_published();
            let revision = book.revision;
            assert!(book.action_target("ha-binary-0-on").is_some());
            book.live("switch.desk", value.as_ref());
            assert_ne!(
                book.revision, revision,
                "raw availability changes publish even when displayed text is identical"
            );
            assert!(book.action_target("ha-binary-0-on").is_none());
            assert!(book.action_target("ha-binary-0-off").is_none());
            assert_eq!(book.frame()["items"].as_array().unwrap().len(), 2);
            book.mark_published();
        }
        book.live("switch.desk", None);
        book.initial("switch.desk", Some(&known));
        book.mark_published();
        assert!(
            book.action_target("ha-binary-0-on").is_none(),
            "late REST state cannot revive a deleted target"
        );
        book.live("switch.desk", Some(&known));
        book.mark_published();
        assert!(book.action_target("ha-binary-0-on").is_some());
        book.disconnected();
        book.connected();
        book.mark_published();
        assert!(book.action_target("ha-binary-0-on").is_none());
        book.begin_session();
        book.connected();
        book.mark_published();
        assert!(book.action_target("ha-binary-0-on").is_none());
        book.initial("switch.desk", Some(&known));
        book.mark_published();
        assert!(book.action_target("ha-binary-0-on").is_some());
        book.authentication_rejected();
        assert!(book.action_target("ha-binary-0-on").is_none());
        assert_eq!(book.frame()["items"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn cover_actions_preserve_earlier_families_and_observed_stationary_or_moving_state() {
        let names = [
            "button.first".into(),
            "scene.night".into(),
            "media_player.den".into(),
            "switch.desk".into(),
            "cover.shade".into(),
            "cover.read_only".into(),
        ];
        let actions = configured_actions(&names[..2], &names[2..3], &names[3..4], &names[4..5]);
        let shared = Book::new(&names, &actions, &[]);
        let mut book = shared.lock().unwrap();
        for (name, value) in names
            .iter()
            .zip(["unknown", "unknown", "paused", "off", "closed", "open"])
        {
            book.initial(name, Some(&json!({"entity_id":name,"state":value,"attributes":{"friendly_name":"Shade","supported_features":11}})));
        }
        assert_eq!(book.frame()["items"].as_array().unwrap().len(), 7);
        book.connected();
        assert!(book.action_target("ha-cover-0-open").is_none());
        book.mark_published();
        for observed in ["open", "closed", "opening", "closing"] {
            book.live("cover.shade", Some(&json!({"entity_id":"cover.shade","state":observed,"attributes":{"friendly_name":"Shade","supported_features":11}})));
            let frame = book.frame();
            let items = frame["items"].as_array().unwrap();
            assert_eq!(items.len(), 17);
            assert_eq!(items[5]["text"], observed);
            for id in [
                "ha-action-0",
                "ha-action-1",
                "ha-media-0-play",
                "ha-media-0-pause",
                "ha-media-0-stop",
                "ha-binary-0-on",
                "ha-binary-0-off",
            ] {
                assert!(book.action_target(id).is_some());
            }
            for (offset, (verb, label)) in [
                ("open", "Open Shade"),
                ("close", "Close Shade"),
                ("stop", "Stop Shade"),
            ]
            .into_iter()
            .enumerate()
            {
                let id = format!("ha-cover-0-{verb}");
                assert_eq!(
                    items[14 + offset],
                    json!({"kind":"action","id":id,"action_id":id,"title":"Shade","label":label,"params":{},"state_id":"entity-4"})
                );
                assert_eq!(book.action_target(&id).unwrap().entity, "cover.shade");
            }
            assert_eq!(
                book.frame(),
                frame,
                "admitting cover commands must not synthesize state or position"
            );
        }
        for id in [
            "ha-cover-00-open",
            "ha-cover-1-open",
            "ha-cover-0-toggle",
            "ha-cover-0-position",
            "ha-cover-0-open_tilt",
        ] {
            assert!(book.action_target(id).is_none());
        }
        book.live("cover.shade", Some(&json!({"entity_id":"cover.shade","state":"open","attributes":{"friendly_name":"🌞\n".repeat(128),"supported_features":11}})));
        let frame = book.frame();
        for (offset, prefix) in ["Open 🌞", "Close 🌞", "Stop 🌞"].into_iter().enumerate() {
            let label = frame["items"][14 + offset]["label"].as_str().unwrap();
            assert!(label.starts_with(prefix));
            assert!(label.len() <= 128);
            assert!(!label.contains('\n'));
        }
    }

    #[test]
    fn cover_capability_only_updates_publish_and_recheck_each_required_bit() {
        let names = ["cover.shade".into()];
        let shared = Book::new(&names, &configured_actions(&[], &[], &[], &names), &[]);
        let mut book = shared.lock().unwrap();
        book.connected();
        book.mark_published();
        let mut previous = 0;
        for features in [0, 1, 2, 8, 3, 9, 10, 11, 4, 16, u64::MAX] {
            let revision = book.revision;
            book.live("cover.shade", Some(&json!({"entity_id":"cover.shade","state":"closed","attributes":{"supported_features":features}})));
            assert_ne!(
                book.revision, revision,
                "capability-only updates need a publication revision"
            );
            let frame = book.frame();
            assert_eq!(
                frame["items"][1],
                json!({"kind":"text","id":"entity-0","title":"cover.shade","text":"closed"})
            );
            let mut advertised = 0;
            for (verb, bit) in [("open", 1), ("close", 2), ("stop", 8)] {
                let id = format!("ha-cover-0-{verb}");
                let allowed = features & bit != 0;
                assert_eq!(
                    book.action_target(&id).is_some(),
                    allowed && previous & bit != 0,
                    "revocation is immediate; newly eligible actions must be published"
                );
                assert_eq!(
                    frame["items"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|item| item["id"] == id),
                    allowed
                );
                advertised += usize::from(allowed);
            }
            assert_eq!(frame["items"].as_array().unwrap().len(), 2 + advertised);
            book.mark_published();
            for (verb, bit) in [("open", 1), ("close", 2), ("stop", 8)] {
                assert_eq!(
                    book.action_target(&format!("ha-cover-0-{verb}")).is_some(),
                    features & bit != 0
                );
            }
            previous = features;
        }
    }

    #[test]
    fn cover_malformed_observations_withdraw_commands_and_sessions_clear_capabilities() {
        let names = ["cover.shade".into()];
        let shared = Book::new(&names, &configured_actions(&[], &[], &[], &names), &[]);
        let mut book = shared.lock().unwrap();
        book.connected();
        book.mark_published();
        assert!(book.action_target("ha-cover-0-open").is_none());
        let known = json!({"entity_id":"cover.shade","state":"closed","attributes":{"supported_features":11}});
        let mut invalid = ["unknown", "unavailable", "OPEN", "closed ", "opening\n", "on", "", "100"]
            .into_iter().map(|value| Some(json!({"entity_id":"cover.shade","state":value,"attributes":{"supported_features":11}}))).collect::<Vec<_>>();
        for features in [
            json!(null),
            json!(true),
            json!(-1),
            json!(11.0),
            json!("11"),
            json!([11]),
            json!({"mask":11}),
        ] {
            invalid.push(Some(json!({"entity_id":"cover.shade","state":"closed","attributes":{"supported_features":features}})));
        }
        for attributes in [
            json!(null),
            json!([]),
            json!(11),
            json!("attributes"),
            json!({}),
        ] {
            invalid.push(Some(
                json!({"entity_id":"cover.shade","state":"closed","attributes":attributes}),
            ));
        }
        invalid.extend([
            None,
            Some(json!({"entity_id":"cover.shade","state":"closed"})),
            Some(json!({"entity_id":"cover.shade","attributes":{"supported_features":11}})),
            Some(json!({"entity_id":"cover.shade","state":null,"attributes":{"supported_features":11}})),
            Some(json!({"entity_id":"cover.shade","state":false,"attributes":{"supported_features":11}})),
            Some(json!({"entity_id":"cover.shade","state":0,"attributes":{"supported_features":11}})),
            Some(json!({"entity_id":"cover.other","state":"closed","attributes":{"supported_features":11}})),
            Some(serde_json::from_str(r#"{"entity_id":"cover.shade","state":"closed","attributes":{"supported_features":18446744073709551616}}"#).unwrap()),
        ]);
        for value in invalid {
            book.live("cover.shade", Some(&known));
            book.mark_published();
            assert!(book.action_target("ha-cover-0-stop").is_some());
            let revision = book.revision;
            book.live("cover.shade", value.as_ref());
            assert_ne!(book.revision, revision);
            for verb in ["open", "close", "stop"] {
                assert!(book.action_target(&format!("ha-cover-0-{verb}")).is_none());
            }
            assert_eq!(book.frame()["items"].as_array().unwrap().len(), 2);
            book.mark_published();
        }
        book.live("cover.shade", None);
        book.initial("cover.shade", Some(&known));
        book.mark_published();
        assert!(
            book.action_target("ha-cover-0-open").is_none(),
            "late initial state must not revive a deleted cover"
        );
        book.live("cover.shade", Some(&known));
        book.mark_published();
        assert!(book.action_target("ha-cover-0-open").is_some());
        book.disconnected();
        book.connected();
        book.mark_published();
        assert!(book.action_target("ha-cover-0-open").is_none());
        book.begin_session();
        book.connected();
        book.mark_published();
        assert!(book.action_target("ha-cover-0-open").is_none());
        book.initial(
            "cover.shade",
            Some(&json!({"entity_id":"cover.shade","state":"open"})),
        );
        book.mark_published();
        assert!(
            book.action_target("ha-cover-0-open").is_none(),
            "a new session must not reuse old feature bits"
        );
        book.initial("cover.shade", Some(&known));
        book.mark_published();
        assert!(book.action_target("ha-cover-0-open").is_some());
        book.authentication_rejected();
        assert!(book.action_target("ha-cover-0-open").is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn maximum_cover_combinations_fit_the_actual_encoder_with_worst_escaped_values() {
        let literal = |domain: &str, index: usize| {
            let suffix = format!("_{index}");
            format!(
                "{domain}.{}{suffix}",
                "x".repeat(128 - domain.len() - 1 - suffix.len())
            )
        };
        // The single-cover case has 31 maximally escaped long state values and
        // 31 action contributions; additional covers require shorter state text.
        for (action_count, media_count, binary_count, cover_count) in
            [(16, 4, 0, 1), (7, 2, 3, 4), (0, 1, 8, 4)]
        {
            let watched = (0..32 - action_count - media_count - binary_count - cover_count)
                .map(|index| literal("sensor", index))
                .collect::<Vec<_>>();
            let actions = (0..action_count)
                .map(|index| literal(if index % 2 == 0 { "button" } else { "scene" }, index))
                .collect::<Vec<_>>();
            let media = (0..media_count)
                .map(|index| literal("media_player", index))
                .collect::<Vec<_>>();
            let binary = (0..binary_count)
                .map(|index| literal(["switch", "input_boolean", "light"][index % 3], index))
                .collect::<Vec<_>>();
            let covers = (0..cover_count)
                .map(|index| literal("cover", index))
                .collect::<Vec<_>>();
            let config: crate::config::Configuration = serde_json::from_value(json!({
                "revision":"maximum-covers",
                "values":{"ha_base_url":"http://localhost/ha/","watch_entities":watched.join(","),"action_entities":actions.join(","),"media_player_entities":media.join(","),"binary_entities":binary.join(","),"cover_entities":covers.join(",")},
                "secrets":{"ha_token":"fixture-token"}
            })).unwrap();
            let config = config.validate().unwrap();
            assert_eq!(config.entities.len(), 32);
            assert!(config.entities.iter().all(|entity| entity.len() == 128));
            let shared = Book::new(&config.entities, &config.actions(), &[]);
            let frame = {
                let mut book = shared.lock().unwrap();
                book.connected();
                for entity in &config.entities {
                    let observed = if covers.contains(entity) {
                        "opening".to_owned()
                    } else if binary.contains(entity) {
                        "off".to_owned()
                    } else {
                        "\\\"".repeat(1024)
                    };
                    book.live(entity, Some(&json!({"entity_id":entity,"state":observed,"attributes":{"friendly_name":"\\\"".repeat(256),"supported_features":11}})));
                }
                book.frame()
            };
            assert_grouped_frame(&frame, &config);
            let items = frame["items"].as_array().unwrap();
            assert_eq!(items.len(), 64);
            assert_eq!(
                items.iter().filter(|item| item["kind"] == "action").count(),
                31
            );
            let ids = items
                .iter()
                .map(|item| item["id"].as_str().unwrap())
                .collect::<std::collections::HashSet<_>>();
            assert_eq!(ids.len(), items.len());
            for (entity, item) in config.entities.iter().zip(&items[1..33]) {
                assert_eq!(item["title"].as_str().unwrap().len(), MAX_TITLE_BYTES);
                assert_eq!(
                    item["text"].as_str().unwrap().len(),
                    if covers.contains(entity) {
                        7
                    } else if binary.contains(entity) {
                        3
                    } else {
                        MAX_TEXT_BYTES
                    }
                );
            }
            for item in &items[33..] {
                assert_eq!(item["title"].as_str().unwrap().len(), MAX_TITLE_BYTES);
                assert_eq!(item["label"].as_str().unwrap().len(), MAX_TITLE_BYTES);
                assert_eq!(item["params"], json!({}));
            }
            assert!(
                serde_json::to_vec(&frame).unwrap().len()
                    < inverter_worker_protocol::MAX_FRAME_BYTES
            );
            Output::with_writer(std::io::sink())
                .send(frame)
                .await
                .unwrap();
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn maximum_binary_combinations_fit_the_actual_encoder_with_worst_escaped_values() {
        let literal = |domain: &str, index: usize| {
            let suffix = format!("_{index}");
            format!(
                "{domain}.{}{suffix}",
                "x".repeat(128 - domain.len() - 1 - suffix.len())
            )
        };
        // Include 63 contributions with one short binary state: its extra long
        // observed text can cost more encoded bytes than a 64-contribution frame.
        for (action_count, media_count, binary_count) in [(15, 4, 2), (16, 4, 1), (9, 2, 8)] {
            let watched = (0..32 - action_count - media_count - binary_count)
                .map(|index| literal("sensor", index))
                .collect::<Vec<_>>();
            let actions = (0..action_count)
                .map(|index| literal(if index % 2 == 0 { "button" } else { "scene" }, index))
                .collect::<Vec<_>>();
            let media = (0..media_count)
                .map(|index| literal("media_player", index))
                .collect::<Vec<_>>();
            let binary = (0..binary_count)
                .map(|index| literal(["switch", "input_boolean", "light"][index % 3], index))
                .collect::<Vec<_>>();
            let config: crate::config::Configuration = serde_json::from_value(json!({
                "revision":"maximum-binary",
                "values":{"ha_base_url":"http://localhost/ha/","watch_entities":watched.join(","),"action_entities":actions.join(","),"media_player_entities":media.join(","),"binary_entities":binary.join(",")},
                "secrets":{"ha_token":"fixture-token"}
            })).unwrap();
            let config = config.validate().unwrap();
            assert_eq!(config.entities.len(), 32);
            assert!(config.entities.iter().all(|entity| entity.len() == 128));
            let shared = Book::new(&config.entities, &config.actions(), &[]);
            let frame = {
                let mut book = shared.lock().unwrap();
                book.connected();
                for entity in &config.entities {
                    let observed = if binary.contains(entity) {
                        "off".to_owned()
                    } else {
                        "\\\"".repeat(1024)
                    };
                    // Binary controls need a real on/off state. Every other text
                    // and every title fills its limit with maximum JSON escaping.
                    book.live(entity, Some(&json!({"entity_id":entity,"state":observed,"attributes":{"friendly_name":"\\\"".repeat(256)}})));
                }
                book.frame()
            };
            assert_grouped_frame(&frame, &config);
            let items = frame["items"].as_array().unwrap();
            let action_buttons = action_count + 3 * media_count + 2 * binary_count;
            assert_eq!(items.len(), 33 + action_buttons);
            assert!(matches!(items.len(), 63 | 64));
            let ids = items
                .iter()
                .map(|item| item["id"].as_str().unwrap())
                .collect::<std::collections::HashSet<_>>();
            assert_eq!(ids.len(), items.len());
            for (entity, item) in config.entities.iter().zip(&items[1..33]) {
                assert_eq!(item["title"].as_str().unwrap().len(), MAX_TITLE_BYTES);
                assert_eq!(
                    item["text"].as_str().unwrap().len(),
                    if binary.contains(entity) {
                        3
                    } else {
                        MAX_TEXT_BYTES
                    }
                );
            }
            for item in &items[33..] {
                assert_eq!(item["kind"], "action");
                assert_eq!(item["title"].as_str().unwrap().len(), MAX_TITLE_BYTES);
                assert_eq!(item["label"].as_str().unwrap().len(), MAX_TITLE_BYTES);
                assert_eq!(item["params"], json!({}));
            }
            assert!(
                serde_json::to_vec(&frame).unwrap().len()
                    < inverter_worker_protocol::MAX_FRAME_BYTES
            );
            Output::with_writer(std::io::sink())
                .send(frame)
                .await
                .unwrap();
        }
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
        let state = Book::new(&config.entities, &config.actions(), &[]);
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
            assert_eq!(item["title"].as_str().unwrap().len(), MAX_TITLE_BYTES);
            assert_eq!(item["text"].as_str().unwrap().len(), MAX_TEXT_BYTES);
        }
        for item in &items[33..] {
            assert_eq!(item["title"].as_str().unwrap().len(), MAX_TITLE_BYTES);
            assert_eq!(item["label"].as_str().unwrap().len(), MAX_TITLE_BYTES);
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

    #[test]
    fn discovery_warnings_preserve_explicit_action_and_numeric_authority() {
        let config: crate::config::Configuration = serde_json::from_value(json!({
            "revision":"independent-discovery","values":{"ha_base_url":"http://localhost",
            "action_entities":"button.a","number_entities":"number.a","discovery_prefixes":"sensor."},
            "secrets":{"ha_token":"fixture"}})).unwrap();
        let config = config.validate().unwrap();
        let shared = Book::with_discovery(
            &config.entities,
            &config.actions(),
            &config.inputs(),
            &config.discovery_prefixes,
        );
        let mut book = shared.lock().unwrap();
        book.begin_session();
        book.connected();
        book.live(
            "button.a",
            Some(&json!({"entity_id":"button.a","state":"unknown"})),
        );
        book.live("number.a", Some(&number_state("-0.3")));
        book.mark_published();
        let link = book.subscribe_connection();
        let epoch = link.borrow().epoch;
        let input = input_item(&book, "ha-number-0-set");
        let params = json!({"input_revision":input["input_revision"],"value_scaled":-2});
        for index in 0..129 {
            book.live(&format!("sensor.buffer_{index}"), None);
        }
        assert_eq!(
            book.frame()["items"][0]["value"],
            "Connected; Discovery unavailable"
        );
        assert_eq!(book.frame()["items"][0]["tone"], "warning");
        assert!(link.borrow().connected);
        assert_eq!(link.borrow().epoch, epoch);
        assert!(book.action_target("ha-action-0").is_some());
        assert!(book.input_target("ha-number-0-set", &params).is_ok());
        assert_eq!(
            input_item(&book, "ha-number-0-set")["input_revision"],
            input["input_revision"]
        );
        book.discovery_snapshot(&[json!({"entity_id":"sensor.feed_ac_state","state":"on"})]);
        assert!(book.action_target("feed_ac_state").is_none());
        assert!(book.action_target("discovery-0").is_none());
        book.live("number.a", Some(&number_state("-0.2")));
        assert!(book.input_target("ha-number-0-set", &params).is_ok());
        assert_eq!(book.frame()["items"].as_array().unwrap().len(), 5);
    }

    #[test]
    fn discovery_bootstrap_and_reconnect_keep_explicit_state_and_live_tombstones_separate() {
        let shared =
            Book::with_discovery(&["sensor.explicit".into()], &[], &[], &["sensor.".into()]);
        let mut book = shared.lock().unwrap();
        book.begin_session();
        book.connected();
        let live = json!({"entity_id":"sensor.live","state":"2"});
        book.live(
            "sensor.explicit",
            Some(&json!({"entity_id":"sensor.explicit","state":"9"})),
        );
        book.live("sensor.live", Some(&live));
        book.live("sensor.deleted", None);
        book.discovery_snapshot(&[
            json!({"entity_id":"sensor.explicit","state":"100"}),
            json!({"entity_id":"sensor.deleted","state":"stale"}),
            json!({"entity_id":"sensor.live","state":"1"}),
        ]);
        assert_eq!(book.frame()["items"][1]["id"], "entity-0");
        assert_eq!(book.frame()["items"][1]["value"], 9.0);
        assert_eq!(book.frame()["items"].as_array().unwrap().len(), 3);
        let discovered = book.frame()["items"][2].clone();
        assert_eq!(discovered["value"], 2.0);
        book.initial(
            "sensor.live",
            Some(&json!({"entity_id":"sensor.live","state":"late"})),
        );
        assert_eq!(book.frame()["items"][2], discovered);
        book.live(
            "sensor.live",
            Some(&json!({"entity_id":"sensor.live","state":false})),
        );
        assert_eq!(book.frame()["items"].as_array().unwrap().len(), 2);
        book.discovery_snapshot(std::slice::from_ref(&live));
        assert_eq!(book.frame()["items"].as_array().unwrap().len(), 2);
        book.disconnected();
        assert!(!book.subscribe_connection().borrow().connected);
        book.discovery_snapshot(std::slice::from_ref(&live));
        assert_eq!(book.frame()["items"].as_array().unwrap().len(), 2);
        book.begin_session();
        book.connected();
        book.discovery_snapshot(&[live]);
        assert_eq!(book.frame()["items"][0]["value"], "Connected");
        assert_eq!(book.frame()["items"][1]["value"], "Waiting");
        assert_ne!(book.frame()["items"][2]["id"], discovered["id"]);
        book.authentication_rejected();
        assert_eq!(book.frame()["items"].as_array().unwrap().len(), 2);
        assert!(book.subscribe_connection().borrow().authentication_rejected);
    }

    #[tokio::test]
    async fn discoveries_and_all_control_slots_fit_the_actual_frame_limit() {
        let literal = |domain: &str, index: usize| {
            let suffix = format!("_{index}");
            format!(
                "{domain}.{}{suffix}",
                "x".repeat(128 - domain.len() - 1 - suffix.len())
            )
        };
        let buttons = (0..3)
            .map(|index| literal("button", index))
            .collect::<Vec<_>>();
        let media = (0..4)
            .map(|index| literal("media_player", index))
            .collect::<Vec<_>>();
        let covers = (0..4)
            .map(|index| literal("cover", index))
            .collect::<Vec<_>>();
        let config: crate::config::Configuration = serde_json::from_value(json!({
            "revision":"maximum-discovery","values":{"ha_base_url":"http://localhost",
            "action_entities":buttons.join(","),"media_player_entities":media.join(","),
            "cover_entities":covers.join(","),"cover_position_entities":covers.join(","),
            "discovery_prefixes":"sensor."},"secrets":{"ha_token":"fixture"}}))
        .unwrap();
        let config = config.validate().unwrap();
        assert_eq!(config.entities.len(), 11);
        let shared = Book::with_discovery(
            &config.entities,
            &config.actions(),
            &config.inputs(),
            &config.discovery_prefixes,
        );
        let frame = {
            let mut book = shared.lock().unwrap();
            book.begin_session();
            book.connected();
            for entity in &config.entities {
                let value = if covers.contains(entity) {
                    "opening".into()
                } else {
                    "\\\"".repeat(512)
                };
                book.live(entity, Some(&json!({"entity_id":entity,"state":value,"attributes":{
                    "friendly_name":"\\\"".repeat(128),"current_position":100,"supported_features":15}})));
            }
            let discoveries = (0..54)
                .map(|index| {
                    json!({"entity_id":literal("sensor", index),
                "state":"\\\"".repeat(512),"attributes":{"friendly_name":"\\\"".repeat(128)}})
                })
                .collect::<Vec<_>>();
            book.discovery_snapshot(&discoveries);
            for input in &mut book.inputs {
                input.revision = u64::MAX;
            }
            book.frame()
        };
        assert_grouped_frame(&frame, &config);
        let items = frame["items"].as_array().unwrap();
        assert_eq!(items.len(), 96);
        assert_eq!(
            items.iter().filter(|item| item["kind"] == "action").count(),
            27
        );
        assert_eq!(
            items
                .iter()
                .filter(|item| item["kind"] == "number_input")
                .count(),
            4
        );
        let discovered = items
            .iter()
            .filter(|item| item["id"].as_str().unwrap().starts_with("discovery-"))
            .collect::<Vec<_>>();
        assert_eq!(discovered.len(), 53);
        assert!(discovered
            .iter()
            .all(|item| item["kind"] == "text" && item.get("action_id").is_none()));
        assert_eq!(items[0]["value"], "Connected; Discovery limit reached");
        let bytes = serde_json::to_vec(&frame).unwrap().len();
        // Reserve the extra digits that monotonic discovery IDs may acquire.
        assert!(bytes + 53 * 20 < inverter_worker_protocol::MAX_FRAME_BYTES);
        Output::with_writer(std::io::sink())
            .send(frame)
            .await
            .unwrap();
    }

    fn numeric_book() -> Shared {
        let config: crate::config::Configuration = serde_json::from_value(json!({
            "revision":"numeric","values":{"ha_base_url":"http://localhost","number_entities":"number.a","cover_position_entities":"cover.a","cover_entities":"cover.a"},"secrets":{"ha_token":"fixture"}})).unwrap();
        let config = config.validate().unwrap();
        Book::new(&config.entities, &config.actions(), &config.inputs())
    }

    fn number_state(value: &str) -> Value {
        json!({"entity_id":"number.a","state":value,"attributes":{"min":-0.5,"max":0.5,"step":0.1,"unit_of_measurement":"kW"}})
    }

    fn input_item(book: &Book, id: &str) -> Value {
        book.frame()["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == id)
            .cloned()
            .unwrap()
    }

    #[test]
    fn numeric_revision_is_stable_for_values_but_rotates_for_constraints_and_eligibility() {
        let shared = numeric_book();
        let mut book = shared.lock().unwrap();
        book.connected();
        book.live("number.a", Some(&number_state("-0.3")));
        let id = "ha-number-0-set";
        let first = input_item(&book, id);
        let params = json!({"input_revision":first["input_revision"],"value_scaled":-2});
        assert_eq!(book.input_target(id, &params).err(), Some("unavailable"));
        book.mark_published();
        assert_eq!(
            book.input_target(id, &params).unwrap().1.to_string(),
            r#"{"entity_id":"number.a","value":-0.2}"#
        );
        let before = book.frame();
        assert!(book.input_target(id, &params).is_ok());
        assert_eq!(
            book.frame(),
            before,
            "admission does not synthesize observed state"
        );
        book.live("number.a", Some(&number_state("0.4")));
        assert_eq!(
            input_item(&book, id)["input_revision"],
            first["input_revision"]
        );
        assert_eq!(input_item(&book, id)["value_scaled"], 4);
        assert!(book.input_target(id, &params).is_ok());
        for (field, value) in [
            ("min", json!(-0.4)),
            ("max", json!(0.4)),
            ("step", json!(0.2)),
            ("unit_of_measurement", json!("W")),
        ] {
            let previous = input_item(&book, id)["input_revision"].clone();
            let mut state = number_state("-0.3");
            state["attributes"][field] = value;
            book.live("number.a", Some(&state));
            assert_ne!(input_item(&book, id)["input_revision"], previous);
            assert_eq!(book.input_target(id, &params).err(), Some("unavailable"));
            let next =
                json!({"input_revision":input_item(&book, id)["input_revision"],"value_scaled":-3});
            assert_eq!(book.input_target(id, &next).err(), Some("unavailable"));
            book.mark_published();
            assert!(book.input_target(id, &next).is_ok());
        }
        let previous = input_item(&book, id)["input_revision"].clone();
        book.live("number.a", None);
        book.live("number.a", Some(&number_state("0")));
        assert_ne!(input_item(&book, id)["input_revision"], previous);
        assert!(
            book.inputs[0].advertised.is_none(),
            "revoke and restore before publication must remain revoked"
        );
        book.mark_published();
        let previous = input_item(&book, id)["input_revision"].clone();
        book.disconnected();
        book.begin_session();
        book.connected();
        book.initial("number.a", Some(&number_state("0")));
        assert_ne!(input_item(&book, id)["input_revision"], previous);
        assert!(book.inputs[0].advertised.is_none());
    }

    #[test]
    fn numeric_actions_require_exact_parameters_grid_and_current_published_grants() {
        let shared = numeric_book();
        let mut book = shared.lock().unwrap();
        book.connected();
        book.live("number.a", Some(&number_state("0")));
        book.mark_published();
        let revision = input_item(&book, "ha-number-0-set")["input_revision"].clone();
        for params in [
            json!({}),
            json!({"input_revision":revision,"value_scaled":0,"service":"turn_on"}),
            json!({"input_revision":revision,"value_scaled":"0"}),
            json!({"input_revision":revision,"value_scaled":0.0}),
            json!({"input_revision":revision,"value_scaled":6}),
            json!({"input_revision":revision,"value_scaled":i64::MIN}),
        ] {
            assert_eq!(
                book.input_target("ha-number-0-set", &params).err(),
                Some("invalid_action")
            );
        }
        assert_eq!(
            book.input_target(
                "ha-number-00-set",
                &json!({"input_revision":revision,"value_scaled":0})
            )
            .err(),
            Some("invalid_action")
        );
        assert_eq!(
            book.input_target(
                "ha-number-0-set",
                &json!({"input_revision":"stale","value_scaled":0})
            )
            .err(),
            Some("unavailable")
        );
        let mut changed = number_state("-0.3");
        changed["attributes"]["step"] = json!(0.2);
        book.live("number.a", Some(&changed));
        book.mark_published();
        let revision = input_item(&book, "ha-number-0-set")["input_revision"].clone();
        assert_eq!(
            book.input_target(
                "ha-number-0-set",
                &json!({"input_revision":revision,"value_scaled":0})
            )
            .err(),
            Some("invalid_action")
        );
        book.inputs[0].revision = u64::MAX;
        book.inputs[0].rotate();
        book.mark_published();
        assert!(book.inputs[0].exhausted);
        assert!(book.frame()["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["id"] != "ha-number-0-set"));
    }

    #[test]
    fn cover_position_has_separate_authority_and_preserves_revision_for_position_or_unrelated_bits()
    {
        let shared = numeric_book();
        let mut book = shared.lock().unwrap();
        book.connected();
        let mut state = json!({"entity_id":"cover.a","state":"closed","attributes":{"current_position":20,"supported_features":15}});
        book.live("cover.a", Some(&state));
        book.mark_published();
        let id = "ha-cover-position-0-set";
        let initial = input_item(&book, id);
        let params = json!({"input_revision":initial["input_revision"],"value_scaled":37});
        assert_eq!(
            book.input_target(id, &params).unwrap().1,
            json!({"entity_id":"cover.a","position":37})
        );
        assert!(book.action_target("ha-cover-0-open").is_some());
        state["attributes"]["current_position"] = json!(70);
        state["attributes"]["supported_features"] = json!(4);
        state["state"] = json!("opening");
        book.live("cover.a", Some(&state));
        assert_eq!(
            input_item(&book, id)["input_revision"],
            initial["input_revision"]
        );
        assert!(book.input_target(id, &params).is_ok());
        assert!(book.action_target("ha-cover-0-open").is_none());
        state["attributes"]["supported_features"] = json!(11);
        book.live("cover.a", Some(&state));
        assert_eq!(book.input_target(id, &params).err(), Some("unavailable"));
        state["attributes"]["supported_features"] = json!(15);
        book.live("cover.a", Some(&state));
        assert_ne!(
            input_item(&book, id)["input_revision"],
            initial["input_revision"]
        );
        book.initial("cover.a", None);
        assert!(
            book.inputs[1].observation.is_some(),
            "late initial state cannot erase live position metadata"
        );
        book.authentication_rejected();
        assert!(book.inputs.iter().all(|input| input.observation.is_none()));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn maximum_numeric_frames_use_real_eligible_constraints_and_the_actual_encoder() {
        let literal = |domain: &str, index: usize| {
            let suffix = format!("_{index}");
            format!(
                "{domain}.{}{suffix}",
                "x".repeat(128 - domain.len() - 1 - suffix.len())
            )
        };
        for (buttons, media, binary, covers, numbers, positions) in [
            (16, 4, 16, 1, 0, 0),
            (16, 4, 16, 0, 3, 0),
            (16, 4, 8, 4, 3, 4),
            (11, 4, 10, 4, 4, 4),
        ] {
            let list = |domain, count| (0..count).map(|i| literal(domain, i)).collect::<Vec<_>>();
            let actions = list("button", buttons);
            let media = list("media_player", media);
            let binary = list("light", binary);
            let covers = list("cover", covers);
            let numbers = list("number", numbers);
            let positions = list("cover", positions);
            let mut targets = actions
                .iter()
                .chain(&media)
                .chain(&binary)
                .chain(&covers)
                .chain(&numbers)
                .chain(&positions)
                .cloned()
                .collect::<std::collections::HashSet<_>>();
            let sensors = list("sensor", crate::config::MAX_ENTITIES - targets.len());
            targets.extend(sensors.iter().cloned());
            let config: crate::config::Configuration = serde_json::from_value(json!({"revision":"maximum-numeric","values":{
                "ha_base_url":"http://localhost","watch_entities":sensors.join(","),"action_entities":actions.join(","),"media_player_entities":media.join(","),"binary_entities":binary.join(","),"cover_entities":covers.join(","),"number_entities":numbers.join(","),"cover_position_entities":positions.join(",")},"secrets":{"ha_token":"fixture"}})).unwrap();
            let config = config.validate().unwrap();
            assert_eq!(config.entities.len(), 64);
            let shared = Book::new(&config.entities, &config.actions(), &config.inputs());
            let frame = {
                let mut book = shared.lock().unwrap();
                book.connected();
                for entity in &config.entities {
                    let state = if numbers.contains(entity) {
                        "999999999.999999".into()
                    } else if binary.contains(entity) {
                        "off".into()
                    } else if covers.contains(entity) || positions.contains(entity) {
                        "opening".into()
                    } else {
                        "\\\"".repeat(512)
                    };
                    book.live(
                        entity,
                        Some(&json!({"entity_id":entity,"state":state,"attributes":{
                        "friendly_name":"\\\"".repeat(128),"unit_of_measurement":"\\\"".repeat(32),
                        "min":-999999999.999999,"max":999999999.999999,"step":999999999.999999,
                        "supported_features":15,"current_position":100}})),
                    );
                }
                // Exercise the longest revision this worker can advertise.
                for input in &mut book.inputs {
                    input.revision = u64::MAX;
                }
                book.frame()
            };
            assert_grouped_frame(&frame, &config);
            let items = frame["items"].as_array().unwrap();
            assert_eq!(items.len(), 128);
            assert_eq!(
                items
                    .iter()
                    .filter(|item| item["kind"] == "number_input")
                    .count(),
                numbers.len() + positions.len()
            );
            for input in items.iter().filter(|item| item["kind"] == "number_input") {
                assert_eq!(input["title"].as_str().unwrap().len(), MAX_TITLE_BYTES);
                assert_eq!(input["label"].as_str().unwrap().len(), MAX_TITLE_BYTES);
            }
            let bytes = serde_json::to_vec(&frame).unwrap().len();
            assert!(
                bytes + 64 * 20 < inverter_worker_protocol::MAX_FRAME_BYTES,
                "escaped snapshot uses {bytes} bytes"
            );
            Output::with_writer(std::io::sink())
                .send(frame)
                .await
                .unwrap();
        }
    }
}
