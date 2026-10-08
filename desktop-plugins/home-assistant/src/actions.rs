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
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
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

#[derive(Clone)]
struct LockAdmission {
    book: Shared,
    revision: u64,
    epoch: u64,
    eligible: Arc<AtomicBool>,
}

// A canceled request may leave its body owned by a transport task. Revocation
// and body commitment therefore share the observation mutex, not future lifetime.
struct LockBodyPermit {
    book: Shared,
    eligible: Arc<AtomicBool>,
}

impl Drop for LockBodyPermit {
    fn drop(&mut self) {
        let _book = self.book.lock().unwrap_or_else(|error| error.into_inner());
        self.eligible.store(false, Ordering::Release);
    }
}

fn guarded_lock_body(
    admission: LockAdmission,
    action_id: String,
    bytes: Vec<u8>,
    deadline: Instant,
    submitted: Arc<AtomicBool>,
) -> impl futures_util::Stream<Item = Result<Vec<u8>, std::io::Error>> + Send {
    futures_util::stream::once(async move {
        let book = admission
            .book
            .lock()
            .map_err(|_| std::io::Error::other("lock admission unavailable"))?;
        if !admission.eligible.load(Ordering::Acquire)
            || Instant::now() >= deadline
            || book.connection_epoch() != admission.epoch
            || book.lock_revision(&action_id) != Some(admission.revision)
        {
            // With an explicit nonzero Content-Length this aborts an incomplete
            // request. Never yield an empty body or clean EOF on revocation.
            return Err(std::io::Error::other("lock admission revoked"));
        }
        // Local commit point: release the one complete, fixed JSON body while
        // holding the same mutex as observation/revocation. Later changes cannot
        // undo a submitted command; no remote atomicity is implied.
        submitted.store(true, Ordering::Release);
        Ok(bytes)
    })
}

fn lock_confirmed(admission: &LockAdmission, entity: &str, target: &str) -> Result<bool, ()> {
    let book = admission.book.lock().map_err(|_| ())?;
    // The outer connection watcher cannot atomically fence this observation
    // read on another runtime thread. A new session cannot confirm an old write.
    if book.connection_epoch() != admission.epoch {
        return Err(());
    }
    let (revision, observed) = book.lock_observation(entity).ok_or(())?;
    if revision != admission.revision && observed == target {
        return Ok(true);
    }
    if !matches!(observed, "locked" | "unlocked" | "locking" | "unlocking") {
        return Err(());
    }
    Ok(false)
}

async fn service(
    client: reqwest::Client,
    configuration: Arc<Validated>,
    action: ConfiguredAction,
    lock_admission: Option<LockAdmission>,
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
    let body_submitted = Arc::new(AtomicBool::new(false));
    let body_permit = lock_admission.as_ref().map(|admission| LockBodyPermit {
        book: admission.book.clone(),
        eligible: admission.eligible.clone(),
    });
    let operation = async {
        let mut selected = action.operation;
        let mut body = body;
        if matches!(selected, crate::config::Operation::Toggle(_))
            || selected.lock_target().is_some()
        {
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
            if let Some(target) = selected.lock_target() {
                if !matches!(value, "locked" | "unlocked") {
                    return Err(());
                }
                // Both current eligibility and the admission observation must
                // survive the read. An unsafe -> safe ABA cannot revive it.
                let admission = lock_admission.as_ref().ok_or(())?;
                let mut book = admission.book.lock().map_err(|_| ())?;
                if book.connection_epoch() != admission.epoch
                    || book.lock_revision(&action.id) != Some(admission.revision)
                {
                    return Err(());
                }
                if value == target {
                    // The explicit target already holds. Never invert intent
                    // or issue another actuation merely because state changed.
                    book.live(&action.entity, Some(&state));
                    return Ok(());
                }
            } else if let crate::config::Operation::Toggle(domain) = selected {
                selected = if value == "on" {
                    crate::config::Operation::TurnOff(domain)
                } else {
                    crate::config::Operation::TurnOn(domain)
                };
            }
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
        let request = client
            .post(configuration.service_url(selected))
            .bearer_auth(&configuration.token)
            .timeout(deadline.saturating_duration_since(Instant::now()));
        let request = if let Some(admission) = lock_admission.as_ref() {
            let bytes = serde_json::to_vec(&body).map_err(|_| ())?;
            let length = bytes.len();
            request
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .header(reqwest::header::CONTENT_LENGTH, length)
                .body(reqwest::Body::wrap_stream(guarded_lock_body(
                    admission.clone(),
                    action.id.clone(),
                    bytes,
                    deadline,
                    body_submitted.clone(),
                )))
        } else {
            submitted = true;
            request.json(&body)
        };
        let mut response = request.send().await.map_err(|_| ())?;
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
        if let Some(target) = selected.lock_target() {
            let admission = lock_admission.as_ref().ok_or(())?;
            loop {
                if lock_confirmed(admission, &action.entity, target)? {
                    return Ok(());
                }
                time::sleep(Duration::from_millis(25)).await;
            }
        }
        Ok(())
    };
    let success = completed_before_deadline(deadline, operation).await;
    // Serialize the terminal outcome with a transport-owned body: after this
    // revocation a no-submission result cannot be followed by body commitment.
    drop(body_permit);
    Completed {
        response: if success {
            json!({"type":"action_result","request_id":request_id,"value":{}})
        } else if !submitted && !body_submitted.load(Ordering::Acquire) {
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
    lock_entity: Option<String>,
    cancel: futures_util::future::AbortHandle,
    lock_permit: Option<LockBodyPermit>,
}

impl Active {
    fn abort(self) {
        // AbortHandle only wakes the service future. Revoke synchronously so a
        // transport task cannot commit before that future is polled again.
        drop(self.lock_permit);
        self.cancel.abort();
    }
}

type Pending = FuturesUnordered<BoxFuture<'static, (String, Option<Completed>)>>;

fn cancel_all(active: &mut Vec<Active>, responses: &mut VecDeque<Value>) {
    for operation in active.drain(..) {
        responses.push_back(unknown(&operation.id));
        operation.abort();
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
                            active.remove(index).abort();
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
                        let lock = configured_actions.iter().any(|action| action.id == action_id && action.operation.lock_target().is_some());
                        if (!known && !input) || (known && !lock && params != json!({})) || !(1..=30_000).contains(&deadline_ms) {
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
                            let target = if input {
                                book.input_target(&action_id, &params)
                            } else {
                                book.action_request(&action_id, &params)
                                    .map(|action| {
                                        let body = json!({"entity_id":action.entity});
                                        (action, body)
                                    }).ok_or("unavailable")
                            };
                            target.map(|(action, body)| {
                                let revision = book.lock_revision(&action.id);
                                (action, body, revision, book.connection_epoch())
                            })
                        };
                        let (action, body, lock_revision, connection_epoch) = match target {
                            Ok(target) => target,
                            Err(code) => {
                                responses.push_back(error(&request_id, code, "The numeric input or selected Home Assistant action is no longer eligible."));
                                continue;
                            }
                        };
                        let lock_entity = action.operation.lock_target().map(|_| action.entity.clone());
                        if lock_entity.is_some() && active.iter().any(|operation| operation.lock_entity == lock_entity) {
                            responses.push_back(error(&request_id, "overloaded", "An operation for this lock is already pending."));
                            continue;
                        }
                        if active.len() == MAX_ACTIVE {
                            responses.push_back(error(&request_id, "overloaded", "Two Home Assistant operations are already pending."));
                            continue;
                        }
                        let (cancel, registration) = futures_util::future::AbortHandle::new_pair();
                        let lock_admission = lock_revision.map(|revision| LockAdmission { book: book.clone(), revision, epoch: connection_epoch, eligible: Arc::new(AtomicBool::new(true)) });
                        let lock_permit = lock_admission.as_ref().map(|admission| LockBodyPermit {
                            book: admission.book.clone(), eligible: admission.eligible.clone()
                        });
                        let operation = service(client.clone(), configuration.clone(), action, lock_admission, body, request_id.clone(), deadline);
                        let id = request_id.clone();
                        pending.push(async move {
                            (id, futures_util::future::Abortable::new(operation, registration).await.ok())
                        }.boxed());
                        active.push(Active { id: request_id, lock_entity, cancel, lock_permit });
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

    async fn lock_admission() -> LockAdmission {
        let configuration = serde_json::from_value::<crate::config::Configuration>(json!({
            "revision":"body-gate-test",
            "values":{
                "ha_base_url":"http://localhost/",
                "dashboard_layout":json!({"version":1,"controls":[{
                    "id":"door", "surface":"home", "order":0,
                    "label":"Door", "entity":"lock.door", "icon":"lock"
                }]}).to_string()
            },
            "secrets":{"ha_token":"fixture"}
        }))
        .unwrap()
        .validate()
        .unwrap();
        let book = crate::state::Book::configured(&configuration);
        {
            let mut book = book.lock().unwrap();
            book.connected();
            book.live("lock.door", Some(&lock_state("unlocked")));
        }
        // Exercise the real publisher before granting the action, rather than
        // manufacturing private advertised state for this transport test.
        let publisher = tokio::spawn(crate::state::publish(
            book.clone(),
            Output::with_writer(std::io::sink()),
        ));
        time::timeout(Duration::from_secs(2), async {
            loop {
                if book
                    .lock()
                    .unwrap()
                    .lock_revision("ha-primary-0-lock")
                    .is_some()
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        publisher.abort();
        let _ = publisher.await;
        let current = book.lock().unwrap();
        let admission = LockAdmission {
            book: book.clone(),
            revision: current.lock_revision("ha-primary-0-lock").unwrap(),
            epoch: current.connection_epoch(),
            eligible: Arc::new(AtomicBool::new(true)),
        };
        drop(current);
        admission
    }

    fn lock_state(state: &str) -> Value {
        json!({"entity_id":"lock.door","state":state,"attributes":{}})
    }

    fn lock_body(
        admission: LockAdmission,
        submitted: Arc<AtomicBool>,
        deadline: Instant,
    ) -> impl futures_util::Stream<Item = Result<Vec<u8>, std::io::Error>> + Send {
        guarded_lock_body(
            admission,
            "ha-primary-0-lock".to_owned(),
            br#"{"entity_id":"lock.door"}"#.to_vec(),
            deadline,
            submitted,
        )
    }

    #[tokio::test]
    async fn lock_confirmation_rejects_target_observed_in_a_new_session() {
        let admission = lock_admission().await;
        assert_eq!(lock_confirmed(&admission, "lock.door", "locked"), Ok(false));
        admission
            .book
            .lock()
            .unwrap()
            .live("lock.door", Some(&lock_state("locked")));
        assert_eq!(lock_confirmed(&admission, "lock.door", "locked"), Ok(true));
        {
            let mut book = admission.book.lock().unwrap();
            book.begin_session();
            book.connected();
            book.live("lock.door", Some(&lock_state("locked")));
        }
        // No outer run/select loop is polled: the confirmation itself must
        // refuse this new-session target, even though its revision is newer.
        assert_eq!(lock_confirmed(&admission, "lock.door", "locked"), Err(()));
    }

    #[tokio::test]
    async fn lock_body_gate_rejects_revision_changes_after_preflight_including_aba() {
        for observations in [vec!["jammed"], vec!["locked"], vec!["jammed", "unlocked"]] {
            let admission = lock_admission().await;
            let submitted = Arc::new(AtomicBool::new(false));
            // Construct the body at the point where the fresh GET check passed,
            // then invalidate the grant before the transport asks for its bytes.
            let body = lock_body(
                admission.clone(),
                submitted.clone(),
                Instant::now() + Duration::from_secs(5),
            );
            for state in observations {
                admission
                    .book
                    .lock()
                    .unwrap()
                    .live("lock.door", Some(&lock_state(state)));
            }
            futures_util::pin_mut!(body);
            assert!(body.next().await.unwrap().is_err());
            assert!(!submitted.load(Ordering::Acquire));
            assert!(body.next().await.is_none());
        }
    }

    #[tokio::test]
    async fn lock_body_gate_rejects_epoch_deadline_and_revoked_transport_ownership() {
        for cause in ["epoch", "deadline", "owner_dropped"] {
            let admission = lock_admission().await;
            let submitted = Arc::new(AtomicBool::new(false));
            let permit = LockBodyPermit {
                book: admission.book.clone(),
                eligible: admission.eligible.clone(),
            };
            let deadline = if cause == "deadline" {
                Instant::now()
            } else {
                Instant::now() + Duration::from_secs(5)
            };
            let body = lock_body(admission.clone(), submitted.clone(), deadline);
            if cause == "epoch" {
                admission.book.lock().unwrap().connected();
            }
            if cause == "owner_dropped" {
                drop(permit);
            }
            futures_util::pin_mut!(body);
            assert!(body.next().await.unwrap().is_err(), "{cause}");
            assert!(!submitted.load(Ordering::Acquire), "{cause}");
        }
    }

    #[tokio::test]
    async fn lock_body_active_cancel_revokes_without_repolling_the_service_future() {
        let admission = lock_admission().await;
        let submitted = Arc::new(AtomicBool::new(false));
        let body = lock_body(
            admission.clone(),
            submitted.clone(),
            Instant::now() + Duration::from_secs(5),
        );
        let (cancel, registration) = futures_util::future::AbortHandle::new_pair();
        let active = Active {
            id: "request".to_owned(),
            lock_entity: Some("lock.door".to_owned()),
            cancel,
            lock_permit: Some(LockBodyPermit {
                book: admission.book.clone(),
                eligible: admission.eligible.clone(),
            }),
        };
        active.abort();
        // Keep the registration alive and deliberately never poll Abortable.
        // The separately owned transport body must already have lost its grant.
        futures_util::pin_mut!(body);
        assert!(body.next().await.unwrap().is_err());
        assert!(!submitted.load(Ordering::Acquire));
        drop(registration);
    }

    #[tokio::test]
    async fn lock_body_commit_is_one_shot_and_terminal_revocation_keeps_submitted() {
        let admission = lock_admission().await;
        let submitted = Arc::new(AtomicBool::new(false));
        let permit = LockBodyPermit {
            book: admission.book.clone(),
            eligible: admission.eligible.clone(),
        };
        let body = lock_body(
            admission.clone(),
            submitted.clone(),
            Instant::now() + Duration::from_secs(5),
        );
        futures_util::pin_mut!(body);
        assert_eq!(
            body.next().await.unwrap().unwrap(),
            br#"{"entity_id":"lock.door"}"#
        );
        assert!(submitted.load(Ordering::Acquire));
        // Body commitment must release the mutex before waiting for the HTTP
        // response or observed target. Later invalidation cannot undo it.
        admission
            .book
            .try_lock()
            .unwrap()
            .live("lock.door", Some(&lock_state("jammed")));
        drop(permit);
        assert!(submitted.load(Ordering::Acquire));
        assert!(body.next().await.is_none());
    }

    #[tokio::test]
    async fn lock_body_http_transport_delay_cannot_send_a_revoked_complete_request() {
        use tokio::{io::AsyncReadExt, net::TcpListener, sync::Notify};
        let admission = lock_admission().await;
        let submitted = Arc::new(AtomicBool::new(false));
        let body = lock_body(
            admission.clone(),
            submitted.clone(),
            Instant::now() + Duration::from_secs(5),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            time::timeout(Duration::from_secs(2), stream.read_to_end(&mut bytes))
                .await
                .unwrap()
                .unwrap();
            bytes
        });
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let delayed_body = futures_util::stream::once({
            let started = started.clone();
            let release = release.clone();
            async move {
                started.notify_one();
                release.notified().await;
                futures_util::pin_mut!(body);
                body.next().await.unwrap()
            }
        });
        let request = network::http_client()
            .unwrap()
            .post(format!("http://{address}/api/services/lock/lock"))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(
                reqwest::header::CONTENT_LENGTH,
                br#"{"entity_id":"lock.door"}"#.len(),
            )
            .body(reqwest::Body::wrap_stream(delayed_body));
        let response = tokio::spawn(async move { request.send().await });
        time::timeout(Duration::from_secs(2), started.notified())
            .await
            .unwrap();
        // The request is executing in reqwest and has yielded before its guarded
        // body poll. Revocation here is the gap a pre-send check cannot close.
        admission
            .book
            .lock()
            .unwrap()
            .live("lock.door", Some(&lock_state("jammed")));
        release.notify_one();
        assert!(time::timeout(Duration::from_secs(2), response)
            .await
            .unwrap()
            .unwrap()
            .is_err());
        let received = server.await.unwrap();
        assert!(!submitted.load(Ordering::Acquire));
        if let Some(boundary) = received.windows(4).position(|window| window == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&received[..boundary]).to_ascii_lowercase();
            assert!(headers.contains("content-length: 25"));
            assert!(received[boundary + 4..].is_empty());
        } else {
            assert!(received.is_empty());
        }
    }

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
