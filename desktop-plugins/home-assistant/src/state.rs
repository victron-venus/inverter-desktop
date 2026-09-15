use crate::config::ConfiguredAction;
use crate::numeric::{self, Observation};
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
    pub fn new(
        entities: &[String],
        actions: &[ConfiguredAction],
        inputs: &[ConfiguredAction],
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
                    title: name.clone(),
                })
                .collect(),
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
                let label = format!("{verb}{}", bounded(&entity.title, 128 - verb.len()));
                let constraints = &observation.constraints;
                items.push(json!({"kind":"number_input","id":input.action.id,
                    "title":entity.title,"action_id":input.action.id,"label":label,
                    "unit":constraints.unit,"input_revision":input.token(),
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
                            "label":format!("Turn {verb} Room control"),"params":{}
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
                    json!({"kind":"action","id":id,"action_id":id,"title":"Shade","label":label,"params":{}})
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
                assert_eq!(item["title"].as_str().unwrap().len(), 128);
                assert_eq!(
                    item["text"].as_str().unwrap().len(),
                    if covers.contains(entity) {
                        7
                    } else if binary.contains(entity) {
                        3
                    } else {
                        512
                    }
                );
            }
            for item in &items[33..] {
                assert_eq!(item["title"].as_str().unwrap().len(), 128);
                assert_eq!(item["label"].as_str().unwrap().len(), 128);
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
                assert_eq!(item["title"].as_str().unwrap().len(), 128);
                assert_eq!(
                    item["text"].as_str().unwrap().len(),
                    if binary.contains(entity) { 3 } else { 512 }
                );
            }
            for item in &items[33..] {
                assert_eq!(item["kind"], "action");
                assert_eq!(item["title"].as_str().unwrap().len(), 128);
                assert_eq!(item["label"].as_str().unwrap().len(), 128);
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
            (15, 4, 0, 1, 0, 1),
            (16, 4, 1, 0, 1, 0),
            (11, 4, 0, 0, 4, 4),
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
            let sensors = list("sensor", 32 - targets.len());
            targets.extend(sensors.iter().cloned());
            let config: crate::config::Configuration = serde_json::from_value(json!({"revision":"maximum-numeric","values":{
                "ha_base_url":"http://localhost","watch_entities":sensors.join(","),"action_entities":actions.join(","),"media_player_entities":media.join(","),"binary_entities":binary.join(","),"cover_entities":covers.join(","),"number_entities":numbers.join(","),"cover_position_entities":positions.join(",")},"secrets":{"ha_token":"fixture"}})).unwrap();
            let config = config.validate().unwrap();
            assert_eq!(config.entities.len(), 32);
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
            let items = frame["items"].as_array().unwrap();
            assert_eq!(items.len(), 64);
            assert_eq!(
                items
                    .iter()
                    .filter(|item| item["kind"] == "number_input")
                    .count(),
                numbers.len() + positions.len()
            );
            for input in items.iter().filter(|item| item["kind"] == "number_input") {
                assert_eq!(input["title"].as_str().unwrap().len(), 128);
                assert_eq!(input["label"].as_str().unwrap().len(), 128);
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
}
