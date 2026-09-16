//! Explicit, non-retrying service operations owned by one worker session.

use crate::{
    config::{ConfiguredAction, Validated},
    network,
    state::Shared,
    Frame,
};
use futures_util::{future::BoxFuture, stream::FuturesUnordered, FutureExt, StreamExt};
use inverter_worker_protocol::{HostFrame, Output};
use serde_json::{json, Value};
use std::{collections::VecDeque, sync::Arc, time::Duration};
use tokio::{
    sync::mpsc,
    time::{self, Instant},
};

const MAX_ACTIVE: usize = 2;
const MAX_HISTORY: usize = 256;
const MAX_RESPONSES: usize = 16;
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

fn error(request_id: &str, code: &str, message: &str) -> Value {
    json!({"type":"action_error","request_id":request_id,"code":code,"message":message})
}

fn unknown(request_id: &str) -> Value {
    error(request_id, "outcome_unknown", "The service outcome is unknown. Check Home Assistant before trying again; cancellation cannot undo an accepted operation.")
}

#[derive(Default)]
struct History(VecDeque<(String, bool)>);

impl History {
    fn seen(&self, id: &str) -> Option<bool> {
        self.0
            .iter()
            .find(|(known, _)| known == id)
            .map(|(_, canceled)| *canceled)
    }

    fn remember(&mut self, id: String, canceled: bool) {
        if let Some(entry) = self.0.iter_mut().find(|(known, _)| *known == id) {
            entry.1 |= canceled;
            return;
        }
        if self.0.len() == MAX_HISTORY {
            self.0.pop_front();
        }
        self.0.push_back((id, canceled));
    }
}

struct Completed {
    response: Value,
    authentication_rejected: bool,
}

async fn completed_before_deadline(
    deadline: Instant,
    operation: impl std::future::Future<Output = Result<(), ()>>,
) -> bool {
    // Tokio polls the operation before its timer. A ready result can therefore
    // arrive after the original deadline when a poll does not yield promptly.
    matches!(time::timeout_at(deadline, operation).await, Ok(Ok(()))) && Instant::now() < deadline
}

async fn service(
    client: reqwest::Client,
    configuration: Arc<Validated>,
    action: ConfiguredAction,
    body: Value,
    request_id: String,
    deadline: Instant,
) -> Completed {
    if deadline <= Instant::now() {
        return Completed {
            response: error(
                &request_id,
                "timeout",
                "The request expired before submission.",
            ),
            authentication_rejected: false,
        };
    }
    let mut authentication_rejected = false;
    let mut submitted = false;
    let operation = async {
        let mut selected = action.operation;
        let mut body = body;
        if let crate::config::Operation::Toggle(domain) = selected {
            // Resolve the primary operation from a fresh read, never from the
            // displayed cache. Failure cannot authorize a write or a core fallback.
            let response = client
                .get(configuration.state_url(&action.entity))
                .bearer_auth(&configuration.token)
                .timeout(deadline.saturating_duration_since(Instant::now()))
                .send()
                .await
                .map_err(|_| ())?;
            authentication_rejected = matches!(response.status().as_u16(), 401 | 403);
            if !response.status().is_success() {
                return Err(());
            }
            let state = network::action_state(response).await?;
            if state["entity_id"].as_str() != Some(action.entity.as_str()) {
                return Err(());
            }
            let value = state["state"].as_str().ok_or(())?;
            selected = if value == "on" {
                crate::config::Operation::TurnOff(domain)
            } else {
                crate::config::Operation::TurnOn(domain)
            };
        }
        match selected {
            crate::config::Operation::PrimaryCover => {
                body["position"] = json!(0);
            }
            crate::config::Operation::PrimaryNumber => {
                body["value"] = json!(0);
            }
            _ => {}
        }
        if deadline <= Instant::now() {
            return Err(());
        }
        // The body was derived from the selected literal entity and either an
        // immutable preset or an exactly validated published numeric grant.
        submitted = true;
        let mut response = client
            .post(configuration.service_url(selected))
            .bearer_auth(&configuration.token)
            .json(&body)
            .timeout(deadline.saturating_duration_since(Instant::now()))
            .send()
            .await
            .map_err(|_| ())?;
        authentication_rejected = matches!(response.status().as_u16(), 401 | 403);
        if !response.status().is_success()
            || response
                .content_length()
                .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return Err(());
        }
        let mut size = 0;
        while let Some(chunk) = response.chunk().await.map_err(|_| ())? {
            size += chunk.len();
            if size > MAX_RESPONSE_BYTES {
                return Err(());
            }
        }
        Ok(())
    };
    let success = completed_before_deadline(deadline, operation).await;
    Completed {
        response: if success {
            json!({"type":"action_result","request_id":request_id,"value":{}})
        } else if !submitted {
            error(&request_id, "unavailable", "The current Home Assistant state could not be read; no service operation was submitted.")
        } else {
            // Errors can arrive after HA accepted a write. Never infer that a
            // transport error, deadline or non-success response means no effect.
            unknown(&request_id)
        },
        authentication_rejected,
    }
}

struct Active {
    id: String,
    cancel: futures_util::future::AbortHandle,
}

type Pending = FuturesUnordered<BoxFuture<'static, (String, Option<Completed>)>>;

fn cancel_all(active: &mut Vec<Active>, responses: &mut VecDeque<Value>) {
    for operation in active.drain(..) {
        operation.cancel.abort();
        responses.push_back(unknown(&operation.id));
    }
}

pub async fn run(
    incoming: &mut mpsc::Receiver<Frame>,
    output: &Output,
    configuration: Arc<Validated>,
    book: Shared,
) -> Result<(), &'static str> {
    let client = network::http_client()?;
    let configured_actions = configuration.actions();
    let configured_inputs = configuration.inputs();
    let mut connection = book
        .lock()
        .map_err(|_| "state unavailable")?
        .subscribe_connection();
    let mut connection_epoch = connection.borrow().epoch;
    let mut admitted_after = Instant::now();
    let mut active: Vec<Active> = Vec::new();
    let mut pending = Pending::new();
    let mut history = History::default();
    let mut responses: VecDeque<Value> = VecDeque::new();
    let mut flush = time::interval(Duration::from_millis(25));
    flush.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
    loop {
        // Output pressure never blocks reads, cancellation or service deadlines.
        // A host that cannot drain this bounded result backlog loses the session.
        if responses.len() > MAX_RESPONSES {
            return Err("host output overloaded");
        }
        if let Some(response) = responses.front() {
            if output.try_send(response.clone())? {
                responses.pop_front();
            }
        }
        tokio::select! {
            biased;
            changed = connection.changed() => {
                changed.map_err(|_| "state unavailable")?;
                let current = *connection.borrow_and_update();
                if current.epoch != connection_epoch {
                    connection_epoch = current.epoch;
                    admitted_after = current.changed_at.map(Instant::from_std).unwrap_or_else(Instant::now);
                    cancel_all(&mut active, &mut responses);
                }
            },
            completed = pending.next(), if !pending.is_empty() => {
                let (id, completed) = completed.ok_or("service task unavailable")?;
                active.retain(|operation| operation.id != id);
                if let Some(completed) = completed {
                    if completed.authentication_rejected {
                        book.lock().map_err(|_| "state unavailable")?.authentication_rejected();
                    }
                    responses.push_back(completed.response);
                }
            },
            frame = incoming.recv() => {
                match frame.ok_or("host input closed")? {
                    HostFrame::Cancel { request_id } => {
                        history.remember(request_id.clone(), true);
                        if let Some(index) = active.iter().position(|operation| operation.id == request_id) {
                            active.remove(index).cancel.abort();
                            responses.push_back(unknown(&request_id));
                        }
                    }
                    HostFrame::Action { request_id, action_id, params, deadline_ms, received_at } => {
                        if let Some(canceled) = history.seen(&request_id) {
                            responses.push_back(error(&request_id, if canceled { "canceled" } else { "invalid_action" }, "The request is no longer eligible."));
                            continue;
                        }
                        history.remember(request_id.clone(), false);
                        let known = configured_actions.iter().any(|action| action.id == action_id);
                        let input = configured_inputs.iter().any(|input| input.id == action_id);
                        if (!known && !input) || (known && params != json!({})) || !(1..=30_000).contains(&deadline_ms) {
                            responses.push_back(error(&request_id, "invalid_action", "The action does not match its configured preset."));
                            continue;
                        }
                        let received_at = Instant::from_std(received_at);
                        let deadline = received_at + Duration::from_millis(deadline_ms);
                        if deadline <= Instant::now() {
                            responses.push_back(error(&request_id, "timeout", "The request expired before submission."));
                            continue;
                        }
                        if received_at < admitted_after {
                            responses.push_back(error(&request_id, "unavailable", "The selected Home Assistant action is unavailable."));
                            continue;
                        }
                        let target = {
                            let book = book.lock().map_err(|_| "state unavailable")?;
                            if input {
                                book.input_target(&action_id, &params)
                            } else {
                                book.action_target(&action_id)
                                    .map(|action| {
                                        let body = json!({"entity_id":action.entity});
                                        (action, body)
                                    }).ok_or("unavailable")
                            }
                        };
                        let (action, body) = match target {
                            Ok(target) => target,
                            Err(code) => {
                                responses.push_back(error(&request_id, code, "The numeric input or selected Home Assistant action is no longer eligible."));
                                continue;
                            }
                        };
                        if active.len() == MAX_ACTIVE {
                            responses.push_back(error(&request_id, "overloaded", "Two Home Assistant operations are already pending."));
                            continue;
                        }
                        let (cancel, registration) = futures_util::future::AbortHandle::new_pair();
                        let operation = service(client.clone(), configuration.clone(), action, body, request_id.clone(), deadline);
                        let id = request_id.clone();
                        pending.push(async move {
                            (id, futures_util::future::Abortable::new(operation, registration).await.ok())
                        }.boxed());
                        active.push(Active { id: request_id, cancel });
                    }
                    _ => return Err("unexpected host command"),
                }
            },
            _ = flush.tick() => {},
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn non_cooperative_completion_cannot_report_success_after_original_deadline() {
        let deadline = Instant::now() + Duration::from_millis(1);
        let completed = completed_before_deadline(deadline, async {
            // Reproduce one late ready poll rather than a timer that yields.
            std::thread::sleep(Duration::from_millis(20));
            Ok(())
        })
        .await;
        assert!(!completed);
        assert!(
            completed_before_deadline(Instant::now() + Duration::from_secs(1), async { Ok(()) })
                .await
        );
        assert!(
            !completed_before_deadline(Instant::now() + Duration::from_secs(1), async { Err(()) })
                .await
        );
    }

    #[test]
    fn request_and_cancel_history_stays_bounded_and_preserves_recent_cancellation() {
        let mut history = History::default();
        for index in 0..MAX_HISTORY * 3 {
            history.remember(format!("request-{index}"), false);
        }
        assert_eq!(history.0.len(), MAX_HISTORY);
        assert_eq!(history.seen("request-0"), None);
        let recent = format!("request-{}", MAX_HISTORY * 3 - 1);
        history.remember(recent.clone(), true);
        history.remember(recent.clone(), false);
        assert_eq!(history.seen(&recent), Some(true));
        assert_eq!(history.0.len(), MAX_HISTORY);
    }

    #[test]
    fn uncertain_results_are_generic_and_never_claim_cancellation_undid_a_write() {
        let response = unknown("request-1");
        assert_eq!(response["type"], "action_error");
        assert_eq!(response["code"], "outcome_unknown");
        assert!(response["message"]
            .as_str()
            .unwrap()
            .contains("cannot undo"));
        assert!(!response.to_string().contains("entity_id"));
    }
}
