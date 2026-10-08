//! Host-owned compact views. References never grant action authority.

use super::protocol::DashboardContribution;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    Header,
    Home,
    Sidebar,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Icon {
    Home,
    Plug,
    Light,
    Washer,
    Dryer,
    Dishwasher,
    Thermometer,
    Gauge,
    Blinds,
    Play,
    Cloud,
    Lock,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlState {
    On,
    Off,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LockState {
    Locked,
    Unlocked,
    Locking,
    Unlocking,
    Jammed,
    Unknown,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionReference {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub id: String,
    pub title: String,
    pub value: String,
    pub actions: Vec<ActionReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Forecast {
    pub datetime: String,
    pub condition: String,
    pub temperature: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub templow: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Presentation {
    Control {
        id: String,
        surface: Surface,
        order: u16,
        title: String,
        icon: Icon,
        state: ControlState,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lock_state: Option<LockState>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        state_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        action: Option<String>,
    },
    Group {
        id: String,
        surface: Surface,
        order: u16,
        title: String,
        icon: Icon,
        collapsed: bool,
        rows: Vec<Row>,
    },
    Summary {
        id: String,
        surface: Surface,
        order: u16,
        title: String,
        icon: Icon,
        visible: bool,
        active: bool,
        text: String,
        actions: Vec<ActionReference>,
    },
    Weather {
        id: String,
        surface: Surface,
        order: u16,
        title: String,
        condition: String,
        temperature: String,
        unit: String,
        forecast: Vec<Forecast>,
    },
    Connection {
        id: String,
        title: String,
        connected: bool,
    },
}

fn bounded(value: &str, limit: usize) -> Result<(), String> {
    if value.len() > limit || value.chars().any(|c| c.is_control() && c != '\n') {
        return Err("Invalid plugin presentation text".into());
    }
    Ok(())
}

fn identifier(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
    {
        return Err("Invalid plugin presentation identity".into());
    }
    Ok(())
}

pub fn validate(views: &[Presentation], items: &[DashboardContribution]) -> Result<(), String> {
    if views.len() > 96 {
        return Err("Too many plugin presentations".into());
    }
    let mut ids = HashSet::new();
    let item = |id: &str| {
        items
            .iter()
            .find(|v| v.id() == id)
            .ok_or_else(|| "Unknown presentation reference".to_string())
    };
    let action = |id: &str| -> Result<(), String> {
        if !matches!(item(id)?, DashboardContribution::Action { .. }) {
            return Err("Invalid presentation action reference".into());
        }
        Ok(())
    };
    let actions = |refs: &[ActionReference]| -> Result<(), String> {
        if refs.len() > 4 {
            return Err("Too many presentation actions".into());
        }
        for reference in refs {
            bounded(&reference.label, 128)?;
            action(&reference.id)?;
        }
        Ok(())
    };
    let sidebar = |surface: &Surface| -> Result<(), String> {
        if *surface != Surface::Sidebar {
            return Err("Invalid presentation surface".into());
        }
        Ok(())
    };
    for view in views {
        let (id, title) = match view {
            Presentation::Control { id, title, .. }
            | Presentation::Group { id, title, .. }
            | Presentation::Summary { id, title, .. }
            | Presentation::Weather { id, title, .. }
            | Presentation::Connection { id, title, .. } => (id, title),
        };
        identifier(id)?;
        bounded(title, 128)?;
        if !ids.insert(id) {
            return Err("Duplicate plugin presentation identity".into());
        }
        match view {
            Presentation::Control {
                surface,
                action: reference,
                state,
                lock_state,
                state_id,
                ..
            } => {
                if *surface == Surface::Sidebar {
                    return Err("Invalid control surface".into());
                }
                if let Some(reference) = reference {
                    action(reference)?;
                }
                match (lock_state, state_id) {
                    (None, None) => {}
                    (Some(lock_state), Some(state_id)) => {
                        identifier(state_id)?;
                        if !matches!(
                            item(state_id)?,
                            DashboardContribution::Text { .. }
                                | DashboardContribution::Status { .. }
                        ) {
                            return Err("Invalid lock state reference".into());
                        }
                        let expected = match lock_state {
                            LockState::Locked => ControlState::On,
                            LockState::Unlocked => ControlState::Off,
                            _ => ControlState::Unavailable,
                        };
                        if *state != expected
                            || (expected == ControlState::Unavailable && reference.is_some())
                        {
                            return Err("Invalid lock control state".into());
                        }
                        if let Some(reference) = reference {
                            if !matches!(item(reference)?, DashboardContribution::Action { state_id: Some(action_state), .. } if action_state == state_id)
                            {
                                return Err("Invalid lock action state reference".into());
                            }
                        }
                    }
                    _ => return Err("Incomplete lock state presentation".into()),
                }
            }
            Presentation::Group { surface, rows, .. } => {
                sidebar(surface)?;
                if rows.len() > 64 {
                    return Err("Too many presentation rows".into());
                }
                let mut rows_seen = HashSet::new();
                for row in rows {
                    identifier(&row.id)?;
                    if !rows_seen.insert(&row.id) {
                        return Err("Duplicate presentation row".into());
                    }
                    bounded(&row.title, 128)?;
                    if matches!(
                        item(&row.value)?,
                        DashboardContribution::Action { .. }
                            | DashboardContribution::NumberInput { .. }
                    ) {
                        return Err("Invalid presentation value reference".into());
                    }
                    actions(&row.actions)?;
                    if let Some(input) = &row.input {
                        if !matches!(item(input)?, DashboardContribution::NumberInput { .. }) {
                            return Err("Invalid presentation input reference".into());
                        }
                    }
                }
            }
            Presentation::Summary {
                surface,
                text,
                actions: refs,
                ..
            } => {
                sidebar(surface)?;
                bounded(text, 256)?;
                actions(refs)?;
            }
            Presentation::Weather {
                surface,
                condition,
                temperature,
                unit,
                forecast,
                ..
            } => {
                sidebar(surface)?;
                bounded(condition, 128)?;
                bounded(temperature, 32)?;
                bounded(unit, 32)?;
                if forecast.len() > 5 {
                    return Err("Too many forecast periods".into());
                }
                for period in forecast {
                    bounded(&period.datetime, 64)?;
                    bounded(&period.condition, 128)?;
                    bounded(&period.temperature, 32)?;
                    if let Some(low) = &period.templow {
                        bounded(low, 32)?;
                    }
                }
            }
            Presentation::Connection { .. } => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn item(value: Value) -> DashboardContribution {
        serde_json::from_value(value).unwrap()
    }
    fn view(value: Value) -> Presentation {
        serde_json::from_value(value).unwrap()
    }

    fn control() -> Presentation {
        view(
            json!({"kind":"control","id":"home-kitchen","surface":"home","order":7,"title":"Kitchen","icon":"light","state":"on","action":"toggle"}),
        )
    }
    fn items() -> Vec<DashboardContribution> {
        vec![
            item(json!({"kind":"text","id":"state","title":"Kitchen","text":"On"})),
            item(
                json!({"kind":"action","id":"toggle","title":"Kitchen","label":"Toggle","action_id":"dispatch","params":{}}),
            ),
        ]
    }

    #[test]
    fn compact_references_are_local_and_never_infer_action_authority() {
        let controls = items();
        validate(&[control()], &controls).unwrap();
        assert!(validate(&[control()], &controls[..1]).is_err());
        let mut forged = control();
        if let Presentation::Control { action, .. } = &mut forged {
            *action = Some("dispatch".into());
        }
        assert!(
            validate(&[forged], &controls).is_err(),
            "action IDs are not contribution references"
        );
        let mut unavailable = control();
        if let Presentation::Control { state, .. } = &mut unavailable {
            *state = ControlState::Unavailable;
        }
        validate(&[unavailable.clone()], &controls).unwrap();
        assert!(validate(&[unavailable.clone()], &controls[..1]).is_err());
        if let Presentation::Control { action, .. } = &mut unavailable {
            *action = None;
        }
        validate(&[unavailable], &[]).unwrap();
    }

    #[test]
    fn lock_controls_bind_stable_states_and_actions_to_one_readonly_entity() {
        let contributions = vec![
            item(
                json!({"kind":"status","id":"lock-state","title":"Front door","value":"Locked","tone":"success"}),
            ),
            item(
                json!({"kind":"action","id":"unlock","state_id":"lock-state","title":"Front door","label":"Unlock","action_id":"unlock-door","params":{}}),
            ),
        ];
        let control = json!({"kind":"control","id":"lock","surface":"home","order":1,"title":"Front door","icon":"lock","state":"on","lock_state":"locked","state_id":"lock-state","action":"unlock"});
        validate(&[view(control.clone())], &contributions).unwrap();
        let mut invalid = control.clone();
        invalid["state"] = json!("off");
        assert!(validate(&[view(invalid)], &contributions).is_err());
        let mut invalid = control.clone();
        invalid["state_id"] = json!("unlock");
        assert!(validate(&[view(invalid)], &contributions).is_err());
        let mut invalid = control.clone();
        invalid.as_object_mut().unwrap().remove("state_id");
        assert!(validate(&[view(invalid)], &contributions).is_err());
        let mut wrong_action = contributions.clone();
        if let DashboardContribution::Action { state_id, .. } = &mut wrong_action[1] {
            *state_id = None;
        }
        assert!(validate(&[view(control.clone())], &wrong_action).is_err());
        for state in ["locking", "unlocking", "jammed", "unknown", "unavailable"] {
            let mut transitional = control.clone();
            transitional["state"] = json!("unavailable");
            transitional["lock_state"] = json!(state);
            assert!(validate(&[view(transitional.clone())], &contributions).is_err());
            transitional.as_object_mut().unwrap().remove("action");
            validate(&[view(transitional)], &contributions).unwrap();
        }
        let mut unknown_enum = control;
        unknown_enum["lock_state"] = json!("open");
        assert!(serde_json::from_value::<Presentation>(unknown_enum).is_err());
    }

    #[test]
    fn duplicate_ids_wrong_surfaces_and_oversized_views_are_rejected() {
        assert!(validate(&[control(), control()], &items()).is_err());
        let mut wrong = control();
        if let Presentation::Control { surface, .. } = &mut wrong {
            *surface = Surface::Sidebar;
        }
        assert!(validate(&[wrong], &items()).is_err());
        let many: Vec<_> = (0..97)
            .map(|n| Presentation::Connection {
                id: format!("connection-{n}"),
                title: "Link".into(),
                connected: true,
            })
            .collect();
        assert!(validate(&many, &[]).is_err());
        validate(&many[..96], &[]).unwrap();
        let mut long = control();
        if let Presentation::Control { title, .. } = &mut long {
            *title = "é".repeat(65);
        }
        assert!(
            validate(&[long], &items()).is_err(),
            "bounds count encoded bytes"
        );
    }

    #[test]
    fn group_rows_cannot_use_actions_as_values_or_reads_as_actions() {
        let group = json!({"kind":"group","id":"lights","surface":"sidebar","order":0,"title":"Lights","icon":"light","collapsed":true,
            "rows":[{"id":"row","title":"Kitchen","value":"state","actions":[{"id":"toggle","label":"Toggle"}]}]});
        validate(&[view(group.clone())], &items()).unwrap();
        let mut wrong = group.clone();
        wrong["rows"][0]["value"] = json!("toggle");
        assert!(validate(&[view(wrong)], &items()).is_err());
        let mut wrong = group;
        wrong["rows"][0]["actions"][0]["id"] = json!("state");
        assert!(validate(&[view(wrong)], &items()).is_err());
    }
}
