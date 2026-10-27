//! Space room event stream, BFF half (build plan WS02-S4).
//!
//! `GET /api/v1/spaces/{space_ref}/events` is a validating relay in front of model-gateway's
//! `GET /v1/spaces/:space_id/events`. It owns no durable state and no frame buffer: authority is
//! Control's decision plus Session Core's per-delivery gate. What this hop adds is everything a
//! browser-facing edge must own:
//!
//! * the rollout flag (`SPACE_REALTIME_STREAM`, 404 `space_stream_disabled` when off; never an
//!   authorization input),
//! * live organization membership, a per-org-and-user subscribe limiter (30 per minute) and a
//!   per-user concurrent-stream cap (12 per replica),
//! * the org Zero Data Retention short-circuit: `connected{mode:"poll_only"}` then
//!   `closing{zdr_poll_only}` with no upstream call at all (fails closed when org-core is down),
//! * the Control decision mint (audience `model-plane-space-stream`, fresh per connect). The
//!   decision and its token travel only gateway to model-gateway, as headers; they never reach
//!   the browser,
//! * a strict frame validator. Every upstream `event:` block is parsed and re-serialised from a
//!   closed struct: an unknown field (for example `content`), a foreign `space_id`, an event or
//!   kind outside the registry (pinned by `apps/Model Plane/api/contracts/space-events.json`) or an
//!   oversized string drops the frame and ends the stream with `closing{invalid_frame}`. A kind
//!   outside the registry is forwarded only as a payload-free `hidden` stub, so the client's
//!   contiguous-seq check still holds.
//!
//! Errors are `{error:{code,message,details}}`. None of these are ZDR refusals.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use axum::{
    body::Body,
    extract::{Extension, Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use bytes::Bytes;
use dashmap::DashMap;
use futures_util::StreamExt;
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::error_with_details,
    middleware::AuthenticatedUser,
    rate_limit::RateLimiter,
    upstream::proxy_json,
};

/// Control audience the stream decision is signed for. A read decision (Session Core audience)
/// is refused by the stream and the reverse, so a leaked token cannot cross over.
const STREAM_AUDIENCE: &str = "model-plane-space-stream";
const SUBSCRIBE_RPM: f64 = 30.0;
const SLOTS_PER_USER: usize = 12;
const MAX_SPACE_REF_BYTES: usize = 128;
const MAX_FIELD_BYTES: usize = 128;
const MAX_FRAME_BYTES: usize = 16 * 1024;
const MAX_DECISION_BYTES: usize = 4096;
const OPEN_TIMEOUT: Duration = Duration::from_secs(15);
const KEEP_ALIVE: Duration = Duration::from_secs(15);
const CAPACITY_RETRY_AFTER_SECONDS: u64 = 2;
const INVALID_FRAME_RETRY_MS: u64 = 5_000;
const UPSTREAM_LOST_RETRY_MS: u64 = 2_000;

const RUN_STATES: &[&str] = &[
    "queued",
    "running",
    "awaiting_approval",
    "completed",
    "failed",
    "cancelled",
    "other",
];
const UPDATED_FIELDS: &[&str] = &["title", "preview", "pinned", "persona"];
const CLOSING_CODES: &[&str] = &[
    "lease_expired",
    "reauthorize",
    "slow_consumer",
    "membership_revoked",
    "zdr_poll_only",
    "server_draining",
    "upstream_unavailable",
    "invalid_frame",
];
const ERROR_CODES: &[&str] = &["unauthorized", "space_stream_forbidden", "space_stream_unavailable"];

/// Per-router runtime: the knobs, the lazily built limiter and the per-user slot counters. It is
/// built once in [`routes`] and shared through an `Extension`, so no handler reads the process
/// environment and tests can build their own.
pub(super) struct StreamRuntime {
    subscribe_rpm: f64,
    slots_per_user: usize,
    limiter: OnceLock<RateLimiter>,
    slots: DashMap<String, usize>,
}

impl StreamRuntime {
    fn new() -> Self {
        Self {
            subscribe_rpm: SUBSCRIBE_RPM,
            slots_per_user: SLOTS_PER_USER,
            limiter: OnceLock::new(),
            slots: DashMap::new(),
        }
    }

    fn acquire_slot(self: &Arc<Self>, key: &str) -> Option<SlotGuard> {
        let mut entry = self.slots.entry(key.to_owned()).or_insert(0);
        if *entry >= self.slots_per_user {
            return None;
        }
        *entry += 1;
        drop(entry);
        Some(SlotGuard {
            runtime: Arc::clone(self),
            key: key.to_owned(),
        })
    }
}

/// One concurrent-stream slot. Moved into the response body so it is released when the stream
/// ends or the browser disconnects.
struct SlotGuard {
    runtime: Arc<StreamRuntime>,
    key: String,
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        if let Some(mut entry) = self.runtime.slots.get_mut(&self.key) {
            *entry = entry.saturating_sub(1);
        }
        self.runtime.slots.remove_if(&self.key, |_, count| *count == 0);
    }
}

/// The stream route. Merged through `module_routes()`, so it sits behind `require_session`.
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/spaces/{space_ref}/events", get(space_events))
        .layer(Extension(Arc::new(StreamRuntime::new())))
}

fn error_response(status: StatusCode, code: &'static str, message: &'static str) -> Response {
    (status, Json(error_with_details(code, message, json!({})))).into_response()
}

fn retry_response(
    status: StatusCode,
    code: &'static str,
    message: &'static str,
    retry_after_seconds: u64,
) -> Response {
    let mut response = error_response(status, code, message);
    if let Ok(value) = HeaderValue::from_str(&retry_after_seconds.to_string()) {
        response.headers_mut().insert(header::RETRY_AFTER, value);
    }
    response
}

fn json_response((status, Json(body)): (StatusCode, Json<Value>)) -> Response {
    (status, Json(body)).into_response()
}

/// `<generation>.<seq>`, digits only.
fn valid_cursor(value: &str) -> bool {
    let Some((generation, seq)) = value.split_once('.') else {
        return false;
    };
    let digits = |part: &str| {
        !part.is_empty() && part.len() <= 18 && part.bytes().all(|byte| byte.is_ascii_digit())
    };
    digits(generation) && digits(seq)
}

fn valid_space_ref(space_ref: &str) -> bool {
    !space_ref.is_empty()
        && space_ref.len() <= MAX_SPACE_REF_BYTES
        && !space_ref.chars().any(|ch| ch.is_control() || ch.is_whitespace())
}

fn sse_response(body: Body) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header("x-accel-buffering", "no")
        .body(body)
        .expect("static SSE response headers are valid")
}

fn sse_block(event: &str, id: Option<&str>, data: &Value) -> String {
    let mut block = format!("event: {event}\n");
    if let Some(id) = id {
        block.push_str(&format!("id: {id}\n"));
    }
    block.push_str(&format!("data: {data}\n\n"));
    block
}

fn closing_block(code: &str, reconnect: bool, retry_after_ms: u64) -> Bytes {
    Bytes::from(sse_block(
        "closing",
        None,
        &json!({ "code": code, "reconnect": reconnect, "retry_after_ms": retry_after_ms }),
    ))
}

/// The answer for an org (or request) under Zero Data Retention: a poll-only `connected` and an
/// immediate non-reconnecting `closing`. No decision is minted and model-gateway is not called.
fn poll_only_response(space_ref: &str) -> Response {
    let connected = sse_block(
        "connected",
        None,
        &json!({
            "v": 1,
            "space_id": space_ref,
            "generation": 0,
            "head": 0,
            "resumed": false,
            "lease_seconds": 0,
            "heartbeat_seconds": 0,
            "mode": "poll_only",
        }),
    );
    let mut payload = connected.into_bytes();
    payload.extend_from_slice(&closing_block("zdr_poll_only", false, 0));
    sse_response(Body::from(payload))
}

/// A signed Control decision for this stream: reference and token. Never serialised into any
/// browser response.
struct StreamDecision {
    decision_ref: HeaderValue,
    token: HeaderValue,
}

async fn mint_stream_decision(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
) -> Result<StreamDecision, Response> {
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let url = format!(
        "{}/api/v1/internal/spaces/thread-read-decision",
        state.user_core_url
    );
    let (status, Json(response)) = proxy_json(
        state,
        Method::POST,
        &url,
        Some(json!({
            "space_ref": space_ref,
            // A read has nothing to dedupe; the key only needs to be a unique operation name.
            "idempotency_key": format!("space-stream-{}", super::unix_nanos()),
            "service_audience": STREAM_AUDIENCE,
        })),
        Some(org_id),
        Some(&actor),
        None,
    )
    .await;
    // Control declines (not a member, not entitled, personal Space of someone else) or an older
    // Control without the audience. Both mean this caller may not stream.
    if status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND {
        return Err(error_response(
            StatusCode::FORBIDDEN,
            "space_stream_forbidden",
            "The Space event stream is not permitted for this account.",
        ));
    }
    if !status.is_success() {
        return Err(error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "space_stream_unavailable",
            "The Space event stream is temporarily unavailable.",
        ));
    }
    let data = crate::envelope::unwrap_data(&response);
    let decision_ref = data
        .get("decision")
        .and_then(Value::as_object)
        .and_then(|decision| decision.get("decision_ref"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned();
    let token = data
        .get("token")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned();
    let invalid = || {
        error_response(
            StatusCode::BAD_GATEWAY,
            "invalid_space_decision",
            "Control returned an invalid Space stream decision.",
        )
    };
    if decision_ref.is_empty()
        || token.is_empty()
        || decision_ref.len() > MAX_DECISION_BYTES
        || token.len() > MAX_DECISION_BYTES
    {
        return Err(invalid());
    }
    let (Ok(decision_ref), Ok(token)) = (
        HeaderValue::from_str(&decision_ref),
        HeaderValue::from_str(&token),
    ) else {
        return Err(invalid());
    };
    Ok(StreamDecision { decision_ref, token })
}

async fn space_events(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Extension(runtime): Extension<Arc<StreamRuntime>>,
    headers: HeaderMap,
    Path(space_ref): Path<String>,
) -> Response {
    // 1. Rollout flag. A rollout control only; Control and Session Core decide authority.
    if !state.space_features.realtime_stream {
        return error_response(
            StatusCode::NOT_FOUND,
            "space_stream_disabled",
            "The Space event stream is not enabled.",
        );
    }
    // 2. Live organization membership, then the request shape.
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    if org_id.trim().is_empty() {
        return error_response(
            StatusCode::FORBIDDEN,
            "active_membership_required",
            "An active organization membership is required to stream Space events.",
        );
    }
    let space_ref = space_ref.trim().to_owned();
    if !valid_space_ref(&space_ref) {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_space",
            "A valid Space reference is required.",
        );
    }
    let last_event_id = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .trim()
        .to_owned();
    if !last_event_id.is_empty() && !valid_cursor(&last_event_id) {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_cursor",
            "Last-Event-ID is not a Space event cursor.",
        );
    }
    // 3. Subscribe limiter (org and user), then the per-user concurrent-stream cap.
    let caller_key = format!("{}:{}", org_id.trim(), user.user_id);
    let limiter = runtime
        .limiter
        .get_or_init(|| RateLimiter::from_cache_with_rpm(&state.cache, runtime.subscribe_rpm));
    if let Err(retry_after) = limiter.try_acquire(&format!("space-stream:{caller_key}")).await {
        return retry_response(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "Too many Space stream connections. Retry shortly.",
            retry_after.max(1),
        );
    }
    let Some(slot) = runtime.acquire_slot(&caller_key) else {
        return retry_response(
            StatusCode::TOO_MANY_REQUESTS,
            "space_stream_capacity",
            "Too many open Space streams for this account.",
            CAPACITY_RETRY_AFTER_SECONDS,
        );
    };
    // 4. Zero Data Retention fails closed: the request marker, the org posture, or an org-core
    // outage all mean poll only. Nothing is minted and nothing is called upstream.
    if crate::zdr::request_zdr(&headers, None) || crate::zdr::org_zdr_enabled(&state, &user).await {
        return poll_only_response(&space_ref);
    }
    // 5. Fresh Control decision for this lease.
    let decision = match mint_stream_decision(&state, &user, &org_id, &space_ref).await {
        Ok(decision) => decision,
        Err(response) => return response,
    };
    // 6. Delegated service tokens, as the thread listing does.
    let session_token =
        match crate::domains::chat::shared::required_session_token(&state, &user, &headers).await {
            Ok(token) => token,
            Err(reason) => {
                return json_response(crate::domains::chat::shared::delegated_auth_unavailable(
                    reason,
                ))
            }
        };
    let model_token = crate::domains::chat::shared::model_token(&state, &user, &headers).await;

    // 7. Open the upstream stream and relay it through the validator.
    let url = format!(
        "{}/v1/spaces/{}/events",
        state.model_gateway_url,
        urlencoding::encode(&space_ref)
    );
    let user_role = user
        .auth_role
        .as_deref()
        .map(str::trim)
        .filter(|role| !role.is_empty())
        .unwrap_or("member");
    let mut request = state
        .streaming_client
        .request(Method::GET, &url)
        .header(header::ACCEPT, "text/event-stream")
        .header("x-user-id", &user.user_id)
        .header("x-user-role", user_role)
        .header("x-org-id", org_id.trim())
        .header("x-space-decision-ref", decision.decision_ref)
        .header("x-space-decision-token", decision.token);
    if let Some(token) = model_token.as_deref() {
        request = request.bearer_auth(token);
    }
    if let Some(value) =
        crate::domains::chat::shared::data_plane_authorization_value(&session_token)
    {
        request = request.header("x-session-authorization", value);
    }
    if !last_event_id.is_empty() {
        request = request.header("last-event-id", &last_event_id);
    }
    let upstream = match tokio::time::timeout(OPEN_TIMEOUT, request.send()).await {
        Ok(Ok(response)) => response,
        Ok(Err(_)) | Err(_) => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "space_stream_unavailable",
                "The Space event stream is temporarily unavailable.",
            )
        }
    };
    let upstream_status = upstream.status().as_u16();
    if upstream_status != 200 {
        // The upstream body is never read or forwarded: fixed words only.
        return match upstream_status {
            401 => error_response(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "Authentication is required.",
            ),
            403 => error_response(
                StatusCode::FORBIDDEN,
                "space_stream_forbidden",
                "The Space event stream is not permitted for this account.",
            ),
            400 => error_response(
                StatusCode::BAD_GATEWAY,
                "invalid_model_gateway_response",
                "Model Gateway rejected the Space stream request.",
            ),
            503 if upstream
                .headers()
                .get(header::RETRY_AFTER)
                .is_some() =>
            {
                retry_response(
                    StatusCode::TOO_MANY_REQUESTS,
                    "space_stream_capacity",
                    "The Space event stream is at capacity.",
                    CAPACITY_RETRY_AFTER_SECONDS,
                )
            }
            _ => error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "space_stream_unavailable",
                "The Space event stream is temporarily unavailable.",
            ),
        };
    }
    let is_event_stream = upstream
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.trim().to_ascii_lowercase().starts_with("text/event-stream"));
    if !is_event_stream {
        return error_response(
            StatusCode::BAD_GATEWAY,
            "invalid_model_gateway_response",
            "Model Gateway returned an invalid Space stream.",
        );
    }

    let body = Body::from_stream(async_stream::stream! {
        // Released when the stream ends or the browser disconnects.
        let _slot = slot;
        let mut upstream = upstream.bytes_stream();
        let mut parser = SseParser::default();
        let mut ticker = tokio::time::interval(KEEP_ALIVE);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ticker.tick().await; // discard the immediate first tick
        'relay: loop {
            tokio::select! {
                chunk = upstream.next() => match chunk {
                    Some(Ok(bytes)) => match parser.push(&bytes) {
                        Ok(events) => {
                            for event in events {
                                match validate_event(&event, &space_ref) {
                                    Ok(frame) => {
                                        yield Ok::<Bytes, std::io::Error>(Bytes::from(frame.text));
                                        if frame.closing {
                                            break 'relay;
                                        }
                                    }
                                    Err(()) => {
                                        yield Ok(closing_block("invalid_frame", true, INVALID_FRAME_RETRY_MS));
                                        break 'relay;
                                    }
                                }
                            }
                        }
                        Err(()) => {
                            yield Ok(closing_block("invalid_frame", true, INVALID_FRAME_RETRY_MS));
                            break 'relay;
                        }
                    },
                    // A failed or ended upstream that never said `closing`: tell the client to
                    // reconnect with its last id.
                    Some(Err(_)) | None => {
                        yield Ok(closing_block("upstream_unavailable", true, UPSTREAM_LOST_RETRY_MS));
                        break 'relay;
                    }
                },
                _ = ticker.tick() => {
                    yield Ok(Bytes::from_static(b": keep-alive\n\n"));
                }
            }
        }
    });
    sse_response(body)
}

/// One parsed SSE event block: field values only, no raw bytes are ever forwarded.
#[derive(Debug, Default)]
struct RawEvent {
    event: String,
    id: Option<String>,
    data: String,
}

/// Incremental SSE parser. Bounded: a line or an event larger than [`MAX_FRAME_BYTES`] is an
/// error, so a misbehaving upstream cannot grow the gateway's memory.
#[derive(Default)]
struct SseParser {
    buf: Vec<u8>,
    current: RawEvent,
    has_fields: bool,
}

impl SseParser {
    fn push(&mut self, bytes: &[u8]) -> Result<Vec<RawEvent>, ()> {
        self.buf.extend_from_slice(bytes);
        let mut events = Vec::new();
        loop {
            let Some(newline) = self.buf.iter().position(|byte| *byte == b'\n') else {
                if self.buf.len() > MAX_FRAME_BYTES {
                    return Err(());
                }
                break;
            };
            let mut line: Vec<u8> = self.buf.drain(..=newline).collect();
            line.pop(); // the newline
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if line.is_empty() {
                if self.has_fields {
                    events.push(std::mem::take(&mut self.current));
                    self.has_fields = false;
                }
                continue;
            }
            if line[0] == b':' {
                continue; // comment (an upstream keep-alive)
            }
            let line = String::from_utf8_lossy(&line).into_owned();
            let (field, value) = match line.split_once(':') {
                Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
                None => (line.as_str(), ""),
            };
            match field {
                "event" => {
                    self.current.event = value.to_owned();
                    self.has_fields = true;
                }
                "id" => {
                    self.current.id = Some(value.to_owned());
                    self.has_fields = true;
                }
                "data" => {
                    if !self.current.data.is_empty() {
                        self.current.data.push('\n');
                    }
                    self.current.data.push_str(value);
                    if self.current.data.len() > MAX_FRAME_BYTES {
                        return Err(());
                    }
                    self.has_fields = true;
                }
                _ => {} // `retry` and unknown fields are dropped
            }
        }
        Ok(events)
    }
}

/// A validated frame, re-serialised from its closed struct.
struct ValidFrame {
    text: String,
    /// The upstream said `closing`: forward it and end the stream.
    closing: bool,
}

fn ok_string(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_FIELD_BYTES && !value.chars().any(char::is_control)
}

fn closed_word(value: &str, words: &[&str]) -> bool {
    words.contains(&value)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConnectedIn {
    v: u64,
    space_id: String,
    generation: u64,
    head: u64,
    resumed: bool,
    lease_seconds: u64,
    heartbeat_seconds: u64,
    mode: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActorIn {
    #[serde(rename = "type")]
    actor_type: String,
    #[serde(default)]
    id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SpaceEventIn {
    v: u64,
    source: String,
    space_id: String,
    generation: u64,
    seq: u64,
    kind: String,
    at: String,
    actor: ActorIn,
    refs: Map<String, Value>,
    /// Accepted and stripped: the audience revision is a delivery-gate input, never a browser
    /// field.
    #[serde(default)]
    #[allow(dead_code, reason = "accepted so the frame validates, then dropped")]
    audience_revision: Option<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HeartbeatIn {
    head: u64,
    generation: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CursorTooOldIn {
    reason: String,
    head: u64,
    generation: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClosingIn {
    code: String,
    reconnect: bool,
    retry_after_ms: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ErrorIn {
    error: ErrorBodyIn,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ErrorBodyIn {
    code: String,
    message: String,
    #[serde(default)]
    #[allow(dead_code, reason = "accepted so the frame validates, then dropped")]
    details: Option<Value>,
}

/// The ref keys a registry kind may carry. `None` is a kind outside the registry.
fn kind_ref_keys(kind: &str) -> Option<&'static [&'static str]> {
    Some(match kind {
        "thread.created" => &["thread_id", "origin"],
        "message.appended" => &["thread_id", "message_id", "message_sequence", "role"],
        "thread.updated" => &["thread_id", "fields"],
        "thread.archived" | "thread.deleted" => &["thread_id"],
        "run.state_changed" => &[
            "run_id",
            "thread_id",
            "agent_id",
            "parent_run_id",
            "state",
            "prev",
        ],
        "hidden" | "redacted" => &[],
        _ => return None,
    })
}

/// Validates every ref against its key allowlist and value type, returning the clean map.
fn clean_refs(refs: &Map<String, Value>, allowed: &[&str]) -> Result<Map<String, Value>, ()> {
    let mut clean = Map::new();
    for (key, value) in refs {
        if !allowed.contains(&key.as_str()) {
            return Err(());
        }
        let valid = match key.as_str() {
            "message_sequence" => value.as_u64().is_some(),
            "state" | "prev" => value.as_str().is_some_and(|word| closed_word(word, RUN_STATES)),
            "fields" => value.as_array().is_some_and(|items| {
                !items.is_empty()
                    && items.len() <= UPDATED_FIELDS.len()
                    && items.iter().all(|item| {
                        item.as_str()
                            .is_some_and(|word| closed_word(word, UPDATED_FIELDS))
                    })
            }),
            _ => value.as_str().is_some_and(ok_string),
        };
        if !valid {
            return Err(());
        }
        clean.insert(key.clone(), value.clone());
    }
    Ok(clean)
}

fn parse<'a, T: Deserialize<'a>>(data: &'a str) -> Result<T, ()> {
    serde_json::from_str(data).map_err(|_| ())
}

/// An SSE `id` is `<generation>.<seq>` or absent; anything else is a contract violation.
fn clean_id(id: Option<&str>) -> Result<Option<String>, ()> {
    match id {
        None => Ok(None),
        Some(id) if valid_cursor(id) => Ok(Some(id.to_owned())),
        Some(_) => Err(()),
    }
}

fn validate_event(event: &RawEvent, space_ref: &str) -> Result<ValidFrame, ()> {
    let id = clean_id(event.id.as_deref())?;
    let data = event.data.as_str();
    let frame = |name: &str, id: Option<&str>, value: Value, closing: bool| ValidFrame {
        text: sse_block(name, id, &value),
        closing,
    };
    match event.event.as_str() {
        "connected" => {
            let connected: ConnectedIn = parse(data)?;
            if connected.v != 1
                || connected.space_id != space_ref
                || !closed_word(&connected.mode, &["live", "poll_only"])
            {
                return Err(());
            }
            Ok(frame(
                "connected",
                id.as_deref(),
                json!({
                    "v": 1,
                    "space_id": space_ref,
                    "generation": connected.generation,
                    "head": connected.head,
                    "resumed": connected.resumed,
                    "lease_seconds": connected.lease_seconds,
                    "heartbeat_seconds": connected.heartbeat_seconds,
                    "mode": connected.mode,
                }),
                false,
            ))
        }
        "space_event" => {
            let record: SpaceEventIn = parse(data)?;
            let at_ok = !record.at.is_empty()
                && record.at.len() <= 64
                && !record.at.chars().any(char::is_control);
            if record.v != 1
                || record.source != "session"
                || record.space_id != space_ref
                || !at_ok
                || !ok_string(&record.kind)
                || !closed_word(&record.actor.actor_type, &["user", "agent", "system"])
                || record.actor.id.as_deref().is_some_and(|actor| !ok_string(actor))
            {
                return Err(());
            }
            // The SSE id, when present, must be this frame's own cursor.
            if let Some(id) = id.as_deref() {
                if id != format!("{}.{}", record.generation, record.seq) {
                    return Err(());
                }
            }
            // A kind outside the registry passes only as a payload-free `hidden` stub; carrying
            // refs under an unknown kind is a violation.
            let (kind, refs) = match kind_ref_keys(&record.kind) {
                Some(allowed) => (record.kind.as_str(), clean_refs(&record.refs, allowed)?),
                None if record.refs.is_empty() => ("hidden", Map::new()),
                None => return Err(()),
            };
            let mut actor = Map::new();
            actor.insert("type".to_owned(), json!(record.actor.actor_type));
            if let Some(actor_id) = &record.actor.id {
                actor.insert("id".to_owned(), json!(actor_id));
            }
            Ok(frame(
                "space_event",
                id.as_deref(),
                json!({
                    "v": 1,
                    "source": "session",
                    "space_id": space_ref,
                    "generation": record.generation,
                    "seq": record.seq,
                    "kind": kind,
                    "at": record.at,
                    "actor": Value::Object(actor),
                    "refs": Value::Object(refs),
                }),
                false,
            ))
        }
        "heartbeat" => {
            let heartbeat: HeartbeatIn = parse(data)?;
            Ok(frame(
                "heartbeat",
                id.as_deref(),
                json!({ "head": heartbeat.head, "generation": heartbeat.generation }),
                false,
            ))
        }
        "cursor_too_old" => {
            let too_old: CursorTooOldIn = parse(data)?;
            if !closed_word(
                &too_old.reason,
                &["retention", "generation_changed", "ahead_of_head"],
            ) {
                return Err(());
            }
            Ok(frame(
                "cursor_too_old",
                None,
                json!({
                    "reason": too_old.reason,
                    "head": too_old.head,
                    "generation": too_old.generation,
                }),
                false,
            ))
        }
        "closing" => {
            let closing: ClosingIn = parse(data)?;
            if !closed_word(&closing.code, CLOSING_CODES) {
                return Err(());
            }
            Ok(frame(
                "closing",
                id.as_deref(),
                json!({
                    "code": closing.code,
                    "reconnect": closing.reconnect,
                    "retry_after_ms": closing.retry_after_ms,
                }),
                true,
            ))
        }
        "error" => {
            let error: ErrorIn = parse(data)?;
            if !closed_word(&error.error.code, ERROR_CODES) || error.error.message.len() > 256 {
                return Err(());
            }
            // Mid-stream error: the stream ends here. Details are dropped.
            Ok(frame(
                "error",
                None,
                json!({
                    "error": {
                        "code": error.error.code,
                        "message": error.error.message,
                        "details": {},
                    }
                }),
                true,
            ))
        }
        _ => Err(()),
    }
}
