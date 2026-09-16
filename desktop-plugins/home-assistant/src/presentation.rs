//! Bounded native-rendered dashboard data. No executable view code or remote assets.

use crate::config::{self, BinaryDomain, ConfiguredAction, Operation};
use crate::state::bounded;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

pub const DISPLAY_DOMAINS: &[&str] = &[
    "switch",
    "light",
    "input_boolean",
    "fan",
    "cover",
    "lock",
    "media_player",
    "scene",
    "script",
    "number",
    "sensor",
    "binary_sensor",
    "climate",
    "button",
    "weather",
];
const ICONS: &[&str] = &[
    "home",
    "plug",
    "light",
    "washer",
    "dryer",
    "dishwasher",
    "thermometer",
    "gauge",
    "blinds",
    "play",
    "cloud",
];
const SECTIONS: &[&str] = &[
    "sensors",
    "numbers",
    "covers",
    "media",
    "scenes",
    "weather",
    "washer",
    "dryer",
    "dishwasher",
];
const APPLIANCE_ACTIONS: &[&str] = &["washer_start", "washer_pause", "dryer_start", "dryer_pause"];

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    pub version: u8,
    #[serde(default)]
    pub controls: Vec<Control>,
    #[serde(default)]
    pub sections: BTreeMap<String, bool>,
    #[serde(default)]
    pub appliances: BTreeMap<String, String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Control {
    pub id: String,
    pub surface: String,
    pub order: u16,
    pub label: String,
    pub entity: String,
    #[serde(default = "default_icon")]
    pub icon: String,
}

fn default_icon() -> String {
    "plug".into()
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
}

pub fn primary_operation(entity: &str) -> Option<Operation> {
    Some(match entity.split_once('.')?.0 {
        "button" => Operation::Press,
        "scene" => Operation::Activate,
        "cover" => Operation::PrimaryCover,
        "number" => Operation::PrimaryNumber,
        "switch" => Operation::Toggle(BinaryDomain::Switch),
        "input_boolean" => Operation::Toggle(BinaryDomain::InputBoolean),
        "light" => Operation::Toggle(BinaryDomain::Light),
        "fan" => Operation::Toggle(BinaryDomain::Fan),
        "media_player" => Operation::Toggle(BinaryDomain::MediaPlayer),
        "lock" => Operation::Toggle(BinaryDomain::Lock),
        "script" => Operation::Toggle(BinaryDomain::Script),
        "climate" => Operation::Toggle(BinaryDomain::Climate),
        "sensor" => Operation::Toggle(BinaryDomain::Sensor),
        "binary_sensor" => Operation::Toggle(BinaryDomain::BinarySensor),
        _ => return None,
    })
}

impl Layout {
    pub fn parse(value: &str) -> Result<Option<Self>, &'static str> {
        if value.len() > 24 * 1024 {
            return Err("dashboard layout is too large");
        }
        if value.trim().is_empty() {
            return Ok(None);
        }
        let mut layout: Self =
            serde_json::from_str(value).map_err(|_| "invalid dashboard layout")?;
        let mut ids = HashSet::new();
        if layout.version != 1
            || layout.controls.len() > 64
            || layout
                .sections
                .keys()
                .any(|key| !SECTIONS.contains(&key.as_str()))
            || layout.appliances.iter().any(|(key, entity)| {
                !APPLIANCE_ACTIONS.contains(&key.as_str())
                    || (!entity.trim().is_empty()
                        && (!config::literal_entity(entity) || primary_operation(entity).is_none()))
            })
            || layout.controls.iter().any(|control| {
                !identifier(&control.id)
                    || [
                        "ha-connection",
                        "ha-sensors",
                        "ha-numbers",
                        "ha-covers",
                        "ha-media",
                        "ha-scenes",
                        "ha-weather",
                        "ha-washer",
                        "ha-dryer",
                        "ha-dishwasher",
                    ]
                    .contains(&control.id.as_str())
                    || !ids.insert(&control.id)
                    || !matches!(control.surface.as_str(), "header" | "home")
                    || control.label.is_empty()
                    || control.label.len() > 128
                    || control.label.chars().any(char::is_control)
                    || !ICONS.contains(&control.icon.as_str())
                    || !config::literal_entity(&control.entity)
                    || primary_operation(&control.entity).is_none()
            })
        {
            return Err("invalid dashboard layout");
        }
        layout
            .appliances
            .retain(|_, entity| !entity.trim().is_empty());
        Ok(Some(layout))
    }

    pub fn entities(&self) -> impl Iterator<Item = &String> {
        self.controls
            .iter()
            .map(|control| &control.entity)
            .chain(self.appliances.values())
    }

    pub fn actions(&self) -> Vec<ConfiguredAction> {
        let mut seen = HashSet::new();
        self.entities()
            .filter(|entity| seen.insert(*entity))
            .enumerate()
            .map(|(index, entity)| ConfiguredAction {
                id: format!("ha-primary-{index}"),
                entity: entity.clone(),
                operation: primary_operation(entity).expect("validated primary operation"),
            })
            .collect()
    }

    pub fn shown(&self, section: &str) -> bool {
        self.sections.get(section).copied().unwrap_or(true)
    }
}

#[derive(Clone, PartialEq)]
pub(crate) struct Observation {
    pub state: String,
    pub unit: String,
    pub weather: Option<Value>,
    pub notification_title: String,
}

impl Observation {
    pub fn from_state(entity: &str, state: Option<&Value>) -> Option<Self> {
        let state = state.filter(|value| value["entity_id"].as_str() == Some(entity))?;
        let value = state["state"].as_str()?;
        if value.len() > 256 || value.chars().any(char::is_control) {
            return None;
        }
        Some(Self {
            state: value.to_owned(),
            notification_title: bounded(
                state["attributes"]["friendly_name"]
                    .as_str()
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| entity.rsplit('.').next().unwrap_or(entity)),
                128,
            ),
            unit: bounded(
                state["attributes"]["unit_of_measurement"]
                    .as_str()
                    .unwrap_or(""),
                32,
            ),
            weather: entity
                .starts_with("weather.")
                .then(|| crate::weather::structured(value, &state["attributes"]))
                .flatten(),
        })
    }

    pub fn available(&self) -> bool {
        !matches!(
            self.state.trim().to_ascii_lowercase().as_str(),
            "" | "unknown" | "unavailable"
        )
    }

    fn remaining(&self) -> Option<&str> {
        let state = self.state.trim();
        (self.available() && !matches!(state.to_ascii_lowercase().as_str(), "off" | "idle"))
            .then_some(state)
    }
}

pub(crate) struct Row<'a> {
    pub entity: &'a str,
    pub item: &'a Value,
    pub observation: Option<&'a Observation>,
}

fn action_reference(id: &str, label: &str, items: &[Value]) -> Option<Value> {
    items
        .iter()
        .any(|item| item["id"] == id && item["kind"] == "action")
        .then(|| json!({"id":id,"label":label}))
}

pub(crate) fn project(
    layout: &Layout,
    rows: &[Row<'_>],
    items: &[Value],
    actions: &[ConfiguredAction],
    profiles: &config::ApplianceProfiles,
    connected: bool,
) -> Vec<Value> {
    let mut output = vec![
        json!({"kind":"connection","id":"ha-connection","title":"Home Assistant","connected":connected}),
    ];
    let primary = layout.actions();
    for control in &layout.controls {
        let row = rows.iter().find(|row| row.entity == control.entity);
        let state = row
            .and_then(|row| row.observation)
            .map(|observation| observation.state.trim().to_ascii_lowercase());
        let state = if !connected {
            "unavailable"
        } else {
            match state.as_deref() {
                Some("on" | "open" | "opening" | "unlocked") => "on",
                Some("" | "unknown" | "unavailable") | None => "unavailable",
                _ => "off",
            }
        };
        let mut item = json!({"kind":"control","id":control.id,"surface":control.surface,"order":control.order,
            "title":control.label,"icon":control.icon,"state":state});
        if let Some(action) = primary
            .iter()
            .find(|action| action.entity == control.entity)
            // Unknown is a display state, not action authority. Legacy header/Home
            // controls remained clickable; the current published grant decides
            // whether a button, scene or fresh-read toggle is still permitted.
            .filter(|_| connected)
            .and_then(|action| action_reference(&action.id, "", items))
        {
            item["action"] = action["id"].clone();
        }
        output.push(item);
    }
    for (section, title, icon, order, domains) in [
        (
            "sensors",
            "Sensors",
            "gauge",
            20,
            &["sensor", "binary_sensor"][..],
        ),
        ("numbers", "Numbers", "gauge", 30, &["number"][..]),
        ("covers", "Covers", "blinds", 40, &["cover"][..]),
        ("media", "Media", "play", 50, &["media_player"][..]),
        ("scenes", "Scenes", "home", 70, &["scene"][..]),
    ] {
        if !layout.shown(section) {
            continue;
        }
        let group_rows: Vec<_> = rows.iter().filter(|row| domains.contains(&row.entity.split_once('.').map_or("", |(domain, _)| domain)))
            .filter(|row| section == "covers" || row.observation.is_some_and(Observation::available))
            .map(|row| {
                let refs: Vec<_> = actions.iter().filter(|action| action.entity == row.entity && !action.id.starts_with("ha-primary-"))
                    .filter_map(|action| action_reference(&action.id, action.operation.label().trim(), items)).collect();
                let mut value = json!({"id":row.item["id"],"title":row.item["title"],"value":row.item["id"],"actions":refs});
                if let Some(input) = items.iter().find(|item| item["kind"] == "number_input" && item["state_id"] == row.item["id"]) {
                    value["input"] = input["id"].clone();
                }
                value
            }).collect();
        if !group_rows.is_empty() {
            output.push(json!({"kind":"group","id":format!("ha-{section}"),"surface":"sidebar","order":order,
            "title":title,"icon":icon,"collapsed":true,"rows":group_rows}));
        }
    }
    for (role, title, icon, source) in [
        (
            "dishwasher",
            "Dishwasher",
            "dishwasher",
            profiles
                .dishwasher
                .as_ref()
                .map(|profile| profile.running_entity.as_str()),
        ),
        (
            "washer",
            "Washer",
            "washer",
            profiles.washer_remaining_entity.as_deref(),
        ),
        (
            "dryer",
            "Dryer",
            "dryer",
            profiles.dryer_remaining_entity.as_deref(),
        ),
    ] {
        let Some(source) = source else {
            continue;
        };
        let observation = rows
            .iter()
            .find(|row| row.entity == source)
            .and_then(|row| row.observation);
        let active = connected
            && observation.is_some_and(|observation| {
                if role == "dishwasher" {
                    matches!(
                        observation.state.trim().to_ascii_lowercase().as_str(),
                        "on" | "running"
                    )
                } else {
                    observation
                        .remaining()
                        .is_some_and(|text| text.bytes().any(|byte| matches!(byte, b'1'..=b'9')))
                }
            });
        let text = if role == "dishwasher" {
            profiles
                .dishwasher
                .as_ref()
                .and_then(|profile| profile.duration_entity.as_deref())
                .and_then(|name| rows.iter().find(|row| row.entity == name))
                .and_then(|row| row.observation)
                .and_then(Observation::remaining)
        } else {
            observation.and_then(Observation::remaining)
        }
        .unwrap_or("");
        let mut refs = Vec::new();
        for (suffix, label) in [("start", "Start"), ("pause", "Pause")] {
            if let Some(target) = layout.appliances.get(&format!("{role}_{suffix}")) {
                if let Some(action) = primary
                    .iter()
                    .find(|action| &action.entity == target)
                    .and_then(|action| action_reference(&action.id, label, items))
                {
                    refs.push(action);
                }
            }
        }
        output.push(json!({"kind":"summary","id":format!("ha-{role}"),"surface":"sidebar","order":60,
            "title":title,"icon":icon,"active":active,"visible":layout.shown(role)&&active,"text":text,"actions":refs}));
    }
    if layout.shown("weather") {
        if let Some(row) = rows.iter().find(|row| {
            row.observation
                .is_some_and(|observation| observation.available() && observation.weather.is_some())
        }) {
            let mut weather = row
                .observation
                .and_then(|observation| observation.weather.clone())
                .expect("selected weather");
            weather["kind"] = json!("weather");
            weather["id"] = json!("ha-weather");
            weather["surface"] = json!("sidebar");
            weather["order"] = json!(10);
            weather["title"] = row.item["title"].clone();
            output.push(weather);
        }
    }
    output
}
