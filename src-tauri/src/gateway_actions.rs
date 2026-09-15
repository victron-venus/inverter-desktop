//! Allowlisted UI actions for the active HTTPS gateway transport.
//!
//! A POST acknowledges queueing only. Never retry it or fall back to another
//! transport: an interrupted response does not mean the command was rejected.

use crate::gateway::{GatewayHttpAuth, GatewayInstances, GatewaySnapshot};
use crate::inverter_control;
use serde_json::{json, Value};
use std::future::Future;

#[derive(Debug, PartialEq)]
enum Action {
    Toggle {
        entity: String,
        state: Option<bool>,
    },
    DryRun(bool),
    EssMode,
    WaterMode {
        instance: Option<u32>,
        valve: bool,
        mode: u8,
    },
}

fn flag_value(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(value) => Some(*value),
        Value::Number(value) if value.as_i64() == Some(0) => Some(false),
        Value::Number(value) if value.as_i64() == Some(1) => Some(true),
        Value::String(value) => match value.trim().to_ascii_lowercase().as_str() {
            "on" | "true" | "1" => Some(true),
            "off" | "false" | "0" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

pub(crate) fn water_payload(payload: &Value) -> Result<(&str, u8), String> {
    let invalid =
        || "Water control requires pump or valve and an integer mode (0, 1 or 2)".to_string();
    let object = payload
        .as_object()
        .filter(|obj| obj.len() == 2)
        .ok_or_else(invalid)?;
    let which = object
        .get("which")
        .and_then(Value::as_str)
        .filter(|which| matches!(*which, "pump" | "valve"))
        .ok_or_else(invalid)?;
    let mode = object
        .get("mode")
        .and_then(Value::as_u64)
        .filter(|mode| *mode <= 2)
        .ok_or_else(invalid)?;
    Ok((which, mode as u8))
}

fn parse(action: &str, payload: Value, water: GatewayInstances) -> Result<Action, String> {
    let invalid = || "Invalid gateway control payload".to_string();
    let object = payload.as_object().ok_or_else(invalid)?;
    match action {
        "toggle" => {
            if object.len() != 1 + usize::from(object.contains_key("state")) {
                return Err(invalid());
            }
            let entity = object
                .get("entity")
                .and_then(Value::as_str)
                .and_then(inverter_control::flag_key)
                .ok_or("Only inverter-control flags can be changed through IGW")?
                .to_string();
            let state = object
                .get("state")
                .map(|value| flag_value(value).ok_or_else(invalid))
                .transpose()?;
            Ok(Action::Toggle { entity, state })
        }
        "dry_run" if object.len() == 1 => object
            .get("value")
            .and_then(Value::as_bool)
            .map(Action::DryRun)
            .ok_or_else(invalid),
        "ess_mode" if object.is_empty() => Ok(Action::EssMode),
        "water_mode" => {
            let (which, mode) = water_payload(&payload)?;
            let instance = if which == "valve" {
                water.water_valve
            } else {
                water.water_pump
            };
            Ok(Action::WaterMode {
                instance,
                valve: which == "valve",
                mode,
            })
        }
        "dry_run" | "ess_mode" => Err(invalid()),
        _ => Err(
            "This control is not supported through IGW; use its configured device connection"
                .into(),
        ),
    }
}

async fn execute_with<S, P, SF, PF>(request: Action, snapshot: S, post: P) -> Result<(), String>
where
    S: FnOnce() -> SF,
    SF: Future<Output = Result<GatewaySnapshot, String>>,
    P: FnOnce(&'static str, Value) -> PF,
    PF: Future<Output = Result<(), String>>,
{
    let (name, body) = match request {
        Action::Toggle { entity, state } => {
            let enabled = match state {
                Some(value) => value,
                None => {
                    let snap = snapshot().await?;
                    let state = crate::gateway::snapshot_to_state(&snap);
                    !state.booleans.as_ref().and_then(|flags| flags.get(&entity)).copied()
                        .ok_or("Current inverter-control flag state is unavailable; wait for fresh IGW telemetry")?
                }
            };
            (
                "toggle",
                json!({"entity":entity,"state":if enabled { "on" } else { "off" }}),
            )
        }
        Action::DryRun(value) => ("dry_run", json!({"value":value})),
        Action::EssMode => ("ess_mode", json!({})),
        Action::WaterMode {
            instance,
            valve,
            mode,
        } => {
            let snap = snapshot().await?;
            if snap.capabilities.get("water_mode").and_then(Value::as_bool) != Some(true) {
                return Err("This IGW does not advertise Water control support".into());
            }
            let selected = crate::gateway::snapshot_instances(
                &snap,
                GatewayInstances {
                    water_pump: if valve { None } else { instance },
                    water_valve: if valve { instance } else { None },
                    ..Default::default()
                },
            );
            let instance = if valve {
                selected.water_valve
            } else {
                selected.water_pump
            }
            .ok_or("Water instance is unavailable")?;
            // Match IGW's native compatibility: absent Connected is allowed,
            // while a present value must be the numeric integer 1.
            let connected = snap
                .pump
                .get(&format!("{instance}/Connected"))
                .is_none_or(|value| value.as_u64() == Some(1));
            let current_mode = snap
                .pump
                .get(&format!("{instance}/Mode"))
                .and_then(Value::as_u64);
            if !connected || current_mode.is_none_or(|mode| mode > 2) {
                return Err(
                    "Selected Water device is unavailable; wait for fresh IGW telemetry".into(),
                );
            }
            ("water_mode", json!({"instance":instance,"mode":mode}))
        }
    };
    post(name, body).await
}

pub(crate) async fn perform(
    auth: &GatewayHttpAuth,
    action: &str,
    payload: Value,
    water: GatewayInstances,
) -> Result<(), String> {
    let request = parse(action, payload, water)?;
    execute_with(
        request,
        || crate::gateway::command_snapshot(auth),
        |name, body| crate::gateway::send_command(auth, name, body),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn controller(flags: Value) -> GatewaySnapshot {
        serde_json::from_value(json!({"inverter":{"booleans":flags}})).unwrap()
    }

    async fn request(
        action: &str,
        payload: Value,
        snapshot: Result<GatewaySnapshot, String>,
    ) -> (Result<(), String>, Vec<(&'static str, Value)>) {
        let calls = RefCell::new(Vec::new());
        let result = execute_with(
            parse(action, payload, GatewayInstances::default()).unwrap(),
            || async { snapshot },
            |name, body| {
                calls.borrow_mut().push((name, body));
                async { Ok(()) }
            },
        )
        .await;
        (result, calls.into_inner())
    }

    #[tokio::test]
    async fn implicit_flags_use_fresh_gateway_values_and_canonical_entities() {
        for key in inverter_control::FLAG_KEYS {
            for current in [false, true] {
                let (result, posts) = request(
                    "toggle",
                    json!({"entity":format!("input_boolean.{key}")}),
                    Ok(controller(json!({(*key):current}))),
                )
                .await;
                result.unwrap();
                assert_eq!(
                    posts,
                    vec![(
                        "toggle",
                        json!({"entity":key,"state":if current {"off"} else {"on"}})
                    )]
                );
            }
        }
    }

    #[tokio::test]
    async fn unknown_or_failed_controller_read_never_posts() {
        for snap in [
            Ok(GatewaySnapshot::default()),
            Ok(controller(json!({}))),
            Err("snapshot HTTP 503".into()),
        ] {
            let (result, posts) = request("toggle", json!({"entity":"no_feed"}), snap).await;
            assert!(result.is_err());
            assert!(posts.is_empty());
        }
    }

    #[tokio::test]
    async fn explicit_flags_and_stateless_header_actions_skip_snapshot() {
        for (action, payload, expected) in [
            ("dry_run", json!({"value":true}), json!({"value":true})),
            ("dry_run", json!({"value":false}), json!({"value":false})),
            ("ess_mode", json!({}), json!({})),
            (
                "toggle",
                json!({"entity":"input_boolean.no_feed","state":false}),
                json!({"entity":"no_feed","state":"off"}),
            ),
            (
                "toggle",
                json!({"entity":"no_feed","state":" ON "}),
                json!({"entity":"no_feed","state":"on"}),
            ),
        ] {
            let (result, posts) =
                request(action, payload, Err("snapshot must not be called".into())).await;
            result.unwrap();
            assert_eq!(posts, vec![(action, expected)]);
        }
    }

    #[test]
    fn arbitrary_entities_topics_and_invalid_bodies_are_rejected_locally() {
        for (action, payload) in [
            ("toggle", json!({"entity":"switch.no_feed"})),
            ("toggle", json!({"entity":"input_boolean.guest"})),
            ("toggle", json!({"entity":"no_feed","topic":"arbitrary"})),
            ("toggle", json!({"entity":"no_feed","state":null})),
            ("toggle", json!({"entity":"no_feed","state":"toggle"})),
            ("dry_run", json!({"value":1})),
            ("dry_run", json!({"value":true,"extra":0})),
            ("ess_mode", json!({"value":true})),
            ("ess_mode", Value::Null),
            ("setpoint_override", json!({"value":100})),
            ("../toggle", json!({})),
        ] {
            assert!(parse(action, payload, GatewayInstances::default()).is_err());
        }
    }

    #[tokio::test]
    async fn post_failure_is_returned_once_without_retry() {
        let count = RefCell::new(0);
        let result = execute_with(
            Action::EssMode,
            || async { panic!("no snapshot") },
            |_, _| {
                *count.borrow_mut() += 1;
                async { Err("response interrupted after enqueue".into()) }
            },
        )
        .await;
        assert_eq!(result.unwrap_err(), "response interrupted after enqueue");
        assert_eq!(count.into_inner(), 1);
    }

    #[tokio::test]
    async fn source_switch_during_snapshot_rejects_the_command_before_http_dispatch() {
        for (action, payload, snap) in [
            (
                "toggle",
                json!({"entity":"no_feed"}),
                controller(json!({"no_feed":false})),
            ),
            (
                "water_mode",
                json!({"which":"pump","mode":1}),
                serde_json::from_value(json!({
                    "capabilities":{"water_mode":true},"pump":{"1/Mode":0}
                }))
                .unwrap(),
            ),
        ] {
            let client = crate::gateway::idle_test_client();
            let auth = client.http_auth();
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = tokio::sync::oneshot::channel();
            let pending = execute_with(
                parse(action, payload, GatewayInstances::default()).unwrap(),
                || async {
                    started_tx.send(()).unwrap();
                    release_rx.await.unwrap();
                    Ok(snap)
                },
                |name, body| crate::gateway::send_command(&auth, name, body),
            );
            let switch = async {
                started_rx.await.unwrap();
                client.stop();
                release_tx.send(()).unwrap();
            };
            let (result, ()) = tokio::join!(pending, switch);
            // The real HTTP boundary checks the stopped client before even URL
            // validation; no request or fallback can be dispatched.
            assert_eq!(
                result.unwrap_err(),
                "Gateway connection changed; retry the action on the current connection"
            );
        }
    }

    #[test]
    fn water_rejects_truncation_or_defaults_from_bad_input() {
        for payload in [
            json!({"which":"pump"}),
            json!({"which":"tank","mode":0}),
            json!({"which":"pump","mode":256}),
            json!({"which":"pump","mode":-1}),
            json!({"which":"pump","mode":1.5}),
            json!({"which":"pump","mode":"1"}),
            json!({"which":"pump","mode":1,"instance":99}),
        ] {
            assert!(water_payload(&payload).is_err());
        }
    }

    #[tokio::test]
    async fn water_selection_matches_display_and_never_substitutes_an_unavailable_target() {
        let snap = serde_json::from_value::<GatewaySnapshot>(json!({
            "inverter":{"ui_config":{"water":{"pump_instance":7,"valve_instance":9}}},
            "capabilities":{"water_mode":true},
            "pump":{"1/Mode":0,"2/Mode":0,"7/Mode":0,"9/Mode":0,"11/Mode":0,"13/Mode":0}
        }))
        .unwrap();
        for (which, configured, expected) in [
            ("pump", GatewayInstances::default(), 7),
            ("valve", GatewayInstances::default(), 9),
            (
                "pump",
                GatewayInstances {
                    water_pump: Some(11),
                    ..Default::default()
                },
                11,
            ),
            (
                "valve",
                GatewayInstances {
                    water_valve: Some(13),
                    ..Default::default()
                },
                13,
            ),
        ] {
            execute_with(
                parse("water_mode", json!({"which":which,"mode":0}), configured).unwrap(),
                || async { Ok(snap.clone()) },
                |name, body| async move {
                    assert_eq!(name, "water_mode");
                    assert_eq!(body, json!({"instance":expected,"mode":0}));
                    Ok(())
                },
            )
            .await
            .unwrap();
        }
        let configured = GatewayInstances {
            water_pump: Some(99),
            ..Default::default()
        };
        let result = execute_with(
            parse("water_mode", json!({"which":"pump","mode":0}), configured).unwrap(),
            || async { Ok(snap) },
            |_, _| async { panic!("must not use another pump") },
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn water_requires_support_and_selected_connected_device_before_post() {
        let valid = json!({"capabilities":{"water_mode":true},"pump":{"1/Connected":1,"1/Mode":0,"2/Connected":1,"2/Mode":2}});
        for which in ["pump", "valve"] {
            let (result, posts) = request(
                "water_mode",
                json!({"which":which,"mode":1}),
                Ok(serde_json::from_value(valid.clone()).unwrap()),
            )
            .await;
            result.unwrap();
            assert_eq!(
                posts,
                vec![(
                    "water_mode",
                    json!({"instance":if which == "pump" {1} else {2},"mode":1})
                )]
            );
        }
        for invalid in [
            json!({}),
            json!({"capabilities":{"water_mode":false},"pump":{"1/Connected":1,"1/Mode":0}}),
            json!({"capabilities":{"water_mode":true},"pump":{"2/Connected":1,"2/Mode":0}}),
            json!({"capabilities":{"water_mode":true},"pump":{"1/Connected":0,"1/Mode":0}}),
            json!({"capabilities":{"water_mode":true},"pump":{"1/Connected":null,"1/Mode":0}}),
            json!({"capabilities":{"water_mode":true},"pump":{"1/Connected":true,"1/Mode":0}}),
            json!({"capabilities":{"water_mode":true},"pump":{"1/Connected":"1","1/Mode":0}}),
            json!({"capabilities":{"water_mode":true},"pump":{"1/Connected":1,"1/Mode":null}}),
        ] {
            let (result, posts) = request(
                "water_mode",
                json!({"which":"pump","mode":1}),
                Ok(serde_json::from_value(invalid).unwrap()),
            )
            .await;
            assert!(result.is_err());
            assert!(posts.is_empty());
        }
    }
}
