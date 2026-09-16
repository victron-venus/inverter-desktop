//! Directly scoped HA state networking. No worker-output or host handles.

use crate::{config::Validated, state::Shared};
use futures_util::{stream, SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio::{
    net::TcpStream,
    time::{self, Instant},
};
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{self, protocol::WebSocketConfig, Message},
    MaybeTlsStream, WebSocketStream,
};

const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_DISCOVERY_STATES: usize = 4096;
const OPERATION_TIMEOUT: Duration = Duration::from_secs(15);
const PING_INTERVAL: Duration = Duration::from_secs(20);
const PONG_TIMEOUT: Duration = Duration::from_secs(10);
const SUBSCRIPTION_ID: u64 = 1;
const INITIAL_CONCURRENCY: usize = 2;
const BACKOFF_SECONDS: [u64; 6] = [1, 2, 4, 8, 16, 30];

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

// Classification only. Underlying errors, response bodies and credentials are
// never formatted into diagnostics or handed to the state publisher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    Retry,
    Authentication,
}

fn rejected(status: u16) -> bool {
    matches!(status, 401 | 403)
}

fn websocket_failure(error: tungstenite::Error) -> Failure {
    match error {
        tungstenite::Error::Http(response) if rejected(response.status().as_u16()) => {
            Failure::Authentication
        }
        _ => Failure::Retry,
    }
}

fn websocket_limits() -> WebSocketConfig {
    WebSocketConfig::default()
        .read_buffer_size(4096)
        .write_buffer_size(0)
        .max_write_buffer_size(16 * 1024)
        .max_message_size(Some(MAX_RESPONSE_BYTES))
        .max_frame_size(Some(MAX_RESPONSE_BYTES))
}

fn packet(bytes: &[u8]) -> Result<Value, Failure> {
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(Failure::Retry);
    }
    inverter_worker_protocol::reject_reserved_number_keys(bytes).map_err(|_| Failure::Retry)?;
    let value: Value = serde_json::from_slice(bytes).map_err(|_| Failure::Retry)?;
    if !value.is_object() || value.get("type").and_then(Value::as_str).is_none() {
        return Err(Failure::Retry);
    }
    Ok(value)
}

async fn send(socket: &mut Socket, value: Value) -> Result<(), Failure> {
    let bytes = serde_json::to_string(&value).map_err(|_| Failure::Retry)?;
    if bytes.len() > 8192 {
        return Err(Failure::Retry);
    }
    time::timeout(OPERATION_TIMEOUT, socket.send(Message::Text(bytes.into())))
        .await
        .map_err(|_| Failure::Retry)?
        .map_err(websocket_failure)
}

/// RFC control frames are separate from HA's correlated application heartbeat.
async fn receive(socket: &mut Socket) -> Result<Option<Value>, Failure> {
    match socket.next().await {
        Some(Ok(Message::Text(text))) => packet(text.as_bytes()).map(Some),
        Some(Ok(Message::Ping(bytes))) => {
            time::timeout(OPERATION_TIMEOUT, socket.send(Message::Pong(bytes)))
                .await
                .map_err(|_| Failure::Retry)?
                .map_err(websocket_failure)?;
            Ok(None)
        }
        Some(Ok(Message::Pong(_))) => Ok(None),
        Some(Err(error)) => Err(websocket_failure(error)),
        _ => Err(Failure::Retry),
    }
}

async fn handshake_packet(socket: &mut Socket) -> Result<Value, Failure> {
    time::timeout(OPERATION_TIMEOUT, async {
        loop {
            if let Some(value) = receive(socket).await? {
                return Ok(value);
            }
        }
    })
    .await
    .map_err(|_| Failure::Retry)?
}

fn event<'a>(
    configuration: &Validated,
    value: &'a Value,
) -> Result<Option<(&'a str, Option<&'a Value>)>, Failure> {
    if value.get("id").and_then(Value::as_u64) != Some(SUBSCRIPTION_ID) {
        return Ok(None);
    }
    let event = value.get("event").ok_or(Failure::Retry)?;
    if event.get("event_type").and_then(Value::as_str) != Some("state_changed") {
        return Ok(None);
    }
    let data = event.get("data").ok_or(Failure::Retry)?;
    let Some(entity) = data.get("entity_id").and_then(Value::as_str) else {
        return Ok(None);
    };
    let explicit = configuration
        .entities
        .iter()
        .any(|selected| selected == entity);
    // This is local filtering of a broad HA event stream. Discovery cannot
    // promote matching entities into any configured action list.
    if !explicit && !configuration.discovery_matches(entity) {
        return Ok(None);
    }
    if !explicit {
        // Malformed discovered data withdraws only that row. Buffering the
        // tombstone also prevents an older bulk snapshot from resurrecting it.
        let value = data.get("new_state").filter(|value| {
            value.is_object()
                && value.get("entity_id").and_then(Value::as_str) == Some(entity)
                && value.get("state").is_some_and(Value::is_string)
        });
        return Ok(Some((entity, value)));
    }
    let value = data.get("new_state").ok_or(Failure::Retry)?;
    if value.is_null() {
        return Ok(Some((entity, None)));
    }
    if !value.is_object() || value.get("entity_id").and_then(Value::as_str) != Some(entity) {
        return Err(Failure::Retry);
    }
    Ok(Some((entity, Some(value))))
}

fn apply_event(configuration: &Validated, state: &Shared, value: &Value) -> Result<(), Failure> {
    if let Some((entity, value)) = event(configuration, value)? {
        state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .live(entity, value);
    }
    Ok(())
}

fn subscription_result(value: &Value) -> Result<bool, Failure> {
    if value.get("type").and_then(Value::as_str) != Some("result")
        || value.get("id").and_then(Value::as_u64) != Some(SUBSCRIPTION_ID)
    {
        return Ok(false);
    }
    if value.get("success").and_then(Value::as_bool) == Some(true) {
        return Ok(true);
    }
    let unauthorized = value
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(Value::as_str)
        .is_some_and(|code| matches!(code, "unauthorized" | "forbidden"));
    Err(if unauthorized {
        Failure::Authentication
    } else {
        Failure::Retry
    })
}

async fn authenticate(
    socket: &mut Socket,
    configuration: &Validated,
    state: &Shared,
) -> Result<(), Failure> {
    let first = handshake_packet(socket).await?;
    match first.get("type").and_then(Value::as_str) {
        Some("auth_required") => {}
        Some("auth_invalid") => return Err(Failure::Authentication),
        _ => return Err(Failure::Retry),
    }
    send(
        socket,
        json!({"type":"auth","access_token":configuration.token}),
    )
    .await?;
    let authenticated = handshake_packet(socket).await?;
    match authenticated.get("type").and_then(Value::as_str) {
        Some("auth_ok") => {}
        Some("auth_invalid") => return Err(Failure::Authentication),
        _ => return Err(Failure::Retry),
    }
    if !configuration.entities.is_empty() || configuration.discovery_enabled() {
        send(
            socket,
            json!({"id":SUBSCRIPTION_ID,"type":"subscribe_events","event_type":"state_changed"}),
        )
        .await?;
        // The one deadline covers all interleaved control/event traffic; an
        // unacknowledged subscription cannot postpone initial reads forever.
        time::timeout(OPERATION_TIMEOUT, async {
            loop {
                let Some(value) = receive(socket).await? else {
                    continue;
                };
                if subscription_result(&value)? {
                    break Ok(());
                }
                match value.get("type").and_then(Value::as_str) {
                    Some("event") => apply_event(configuration, state, &value)?,
                    Some("auth_invalid") => return Err(Failure::Authentication),
                    _ => return Err(Failure::Retry),
                }
            }
        })
        .await
        .map_err(|_| Failure::Retry)??;
    }
    state
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .connected();
    Ok(())
}

async fn initial_state(
    client: &reqwest::Client,
    configuration: &Validated,
    entity: &str,
) -> Result<Option<Value>, Failure> {
    time::timeout(OPERATION_TIMEOUT, async {
        let response = client
            .get(configuration.state_url(entity))
            .bearer_auth(&configuration.token)
            .send()
            .await
            .map_err(|_| Failure::Retry)?;
        if rejected(response.status().as_u16()) {
            return Err(Failure::Authentication);
        }
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(Failure::Retry);
        }
        let value = bounded_json(response).await?;
        if !value.is_object() || value.get("entity_id").and_then(Value::as_str) != Some(entity) {
            return Err(Failure::Retry);
        }
        Ok(Some(value))
    })
    .await
    .map_err(|_| Failure::Retry)?
}

async fn bounded_json(mut response: reqwest::Response) -> Result<Value, Failure> {
    if response
        .content_length()
        .is_some_and(|bytes| bytes > MAX_RESPONSE_BYTES as u64)
    {
        return Err(Failure::Retry);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| Failure::Retry)? {
        if chunk.len() > MAX_RESPONSE_BYTES - bytes.len() {
            return Err(Failure::Retry);
        }
        bytes.extend_from_slice(&chunk);
    }
    inverter_worker_protocol::reject_reserved_number_keys(&bytes).map_err(|_| Failure::Retry)?;
    serde_json::from_slice(&bytes).map_err(|_| Failure::Retry)
}

pub(crate) async fn action_state(response: reqwest::Response) -> Result<Value, ()> {
    bounded_json(response).await.map_err(|_| ())
}

async fn discovery_states(
    client: &reqwest::Client,
    configuration: &Validated,
) -> Result<Vec<Value>, Failure> {
    time::timeout(OPERATION_TIMEOUT, async {
        let response = client
            .get(configuration.discovery_url())
            .bearer_auth(&configuration.token)
            .send()
            .await
            .map_err(|_| Failure::Retry)?;
        if rejected(response.status().as_u16()) {
            return Err(Failure::Authentication);
        }
        if !response.status().is_success() {
            return Err(Failure::Retry);
        }
        match bounded_json(response).await? {
            Value::Array(states) if states.len() <= MAX_DISCOVERY_STATES => Ok(states),
            _ => Err(Failure::Retry),
        }
    })
    .await
    .map_err(|_| Failure::Retry)?
}

struct Heartbeat {
    next_id: u64,
    next_ping: Instant,
    pending: Option<(u64, Instant)>,
}

impl Heartbeat {
    fn new(now: Instant) -> Self {
        Self {
            next_id: 2,
            next_ping: now + PING_INTERVAL,
            pending: None,
        }
    }
    fn due(&self, now: Instant) -> Result<Option<u64>, Failure> {
        if self.pending.is_some_and(|(_, deadline)| now >= deadline) {
            return Err(Failure::Retry);
        }
        Ok((self.pending.is_none() && now >= self.next_ping).then_some(self.next_id))
    }
    fn sent(&mut self, id: u64, now: Instant) -> Result<(), Failure> {
        self.next_id = id.checked_add(1).ok_or(Failure::Retry)?;
        self.next_ping = now + PING_INTERVAL;
        self.pending = Some((id, now + PONG_TIMEOUT));
        Ok(())
    }
    fn pong(&mut self, id: Option<u64>) {
        if self
            .pending
            .is_some_and(|(expected, _)| Some(expected) == id)
        {
            self.pending = None;
        }
    }
    fn wake(&self) -> Instant {
        self.pending
            .map_or(self.next_ping, |(_, deadline)| deadline)
    }
}

async fn session(
    client: &reqwest::Client,
    configuration: &Validated,
    state: &Shared,
) -> Result<(), Failure> {
    // tokio-tungstenite makes one direct upgrade request. Non-101 responses are
    // returned as errors; it does not follow an HTTP redirect to another origin.
    let (mut socket, _) = time::timeout(
        OPERATION_TIMEOUT,
        connect_async_with_config(
            configuration.websocket_url().as_str(),
            Some(websocket_limits()),
            false,
        ),
    )
    .await
    .map_err(|_| Failure::Retry)?
    .map_err(websocket_failure)?;
    time::timeout(
        OPERATION_TIMEOUT,
        authenticate(&mut socket, configuration, state),
    )
    .await
    .map_err(|_| Failure::Retry)??;
    let mut heartbeat = Heartbeat::new(Instant::now());
    let mut initial = stream::iter(configuration.entities.clone())
        .map(|entity| async move {
            let result = initial_state(client, configuration, &entity).await;
            (entity, result)
        })
        .buffer_unordered(INITIAL_CONCURRENCY);
    let mut initial_finished = configuration.entities.is_empty();
    let discovery = discovery_states(client, configuration);
    tokio::pin!(discovery);
    let mut discovery_finished =
        !configuration.discovery_enabled() && configuration.layout.is_none();
    loop {
        // Check deadlines before each frame, even under continuous server traffic.
        if let Some(id) = heartbeat.due(Instant::now())? {
            send(&mut socket, json!({"id":id,"type":"ping"})).await?;
            heartbeat.sent(id, Instant::now())?;
        }
        tokio::select! {
            _ = time::sleep_until(heartbeat.wake()) => {},
            message = receive(&mut socket) => {
                if let Some(value) = message? {
                    match value.get("type").and_then(Value::as_str) {
                        Some("event") => apply_event(configuration,state,&value)?,
                        Some("pong") => heartbeat.pong(value.get("id").and_then(Value::as_u64)),
                        Some("auth_invalid") => return Err(Failure::Authentication),
                        _ => return Err(Failure::Retry),
                    }
                }
            },
            item = initial.next(), if !initial_finished => {
                match item {
                    Some((entity,result)) => state.lock().unwrap_or_else(|error| error.into_inner()).initial(&entity,result?.as_ref()),
                    None => initial_finished = true,
                }
            },
            result = &mut discovery, if !discovery_finished => {
                discovery_finished = true;
                match result {
                    Ok(states) => state.lock().unwrap_or_else(|error| error.into_inner()).discovery_snapshot(&states),
                    Err(Failure::Authentication) => return Err(Failure::Authentication),
                    Err(Failure::Retry) => state.lock().unwrap_or_else(|error| error.into_inner()).discovery_failed(),
                }
            }
        }
    }
}

pub fn http_client() -> Result<reqwest::Client, &'static str> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .no_proxy()
        .connect_timeout(OPERATION_TIMEOUT)
        .timeout(OPERATION_TIMEOUT)
        .build()
        .map_err(|_| "cannot initialize HA network")
}

pub async fn run(configuration: Arc<Validated>, state: Shared) -> Result<(), &'static str> {
    let client = http_client()?;
    let mut connection = state
        .lock()
        .map_err(|_| "state unavailable")?
        .subscribe_connection();
    let mut retry = 0_usize;
    loop {
        if connection.borrow().authentication_rejected {
            return std::future::pending().await;
        }
        state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .begin_session();
        let started = Instant::now();
        let result = tokio::select! {
            biased;
            _ = connection.wait_for(|link| link.authentication_rejected) => Err(Failure::Authentication),
            result = session(&client, &configuration, &state) => result,
        };
        if result == Err(Failure::Authentication) {
            state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .authentication_rejected();
            // Invalid credentials remain visible until settings restart this
            // process. Never poll an invalid token or end the worker in a loop.
            return std::future::pending().await;
        }
        state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .disconnected();
        if started.elapsed() >= Duration::from_secs(60) {
            retry = 0;
        }
        let authentication_rejected = tokio::select! {
            biased;
            _ = connection.wait_for(|link| link.authentication_rejected) => true,
            _ = time::sleep(Duration::from_secs(BACKOFF_SECONDS[retry])) => false,
        };
        if authentication_rejected {
            return std::future::pending().await;
        }
        retry = (retry + 1).min(BACKOFF_SECONDS.len() - 1);
    }
}

#[cfg(test)]
#[path = "network_tests.rs"]
mod tests;
