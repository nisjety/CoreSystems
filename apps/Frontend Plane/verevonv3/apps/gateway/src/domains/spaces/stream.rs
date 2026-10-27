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

#[cfg(test)]
mod tests {
    //! WS02-S4 tests. The gateway is a binary crate, so the router-level tests live here (an
    //! integration test under `tests/` cannot reach the router); `tests/space_stream_relay.rs`
    //! pins the vendored contract copy against its owners. No model is called: model-gateway is a
    //! wiremock server and every body is a canned SSE document.
    use super::*;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SPACE: &str = "space-room";
    const DECISION_REF: &str = "decision-ref-secret-1";
    const DECISION_TOKEN: &str = "decision-token-secret-xyzxyz";
    const GOLDEN: &str = include_str!("../../../tests/fixtures/contracts/space-events.json");

    struct Fx {
        state: AppState,
        _auth: MockServer,
        user_core: MockServer,
        _org_core: MockServer,
        model_gateway: MockServer,
    }

    fn org_posture(zdr: bool) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(
            json!({"data": {"metadata": {"interactiveRetention": {"zdr": zdr}}}}),
        )
    }

    fn decision_ok() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"data": {
            "decision": {"decision_ref": DECISION_REF, "space_ref": SPACE},
            "token": DECISION_TOKEN,
        }}))
    }

    fn sse(content: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_raw(content.as_bytes().to_vec(), "text/event-stream")
    }

    fn connected_block() -> String {
        sse_block(
            "connected",
            Some("1.5"),
            &json!({"v": 1, "space_id": SPACE, "generation": 1, "head": 5, "resumed": false,
                    "lease_seconds": 90, "heartbeat_seconds": 10, "mode": "live"}),
        )
    }

    fn space_event_data(seq: u64) -> Value {
        json!({"v": 1, "source": "session", "space_id": SPACE, "generation": 1, "seq": seq,
               "kind": "message.appended", "at": "2026-10-27T10:00:00Z",
               "actor": {"type": "user", "id": "user-2"},
               "refs": {"thread_id": "thread-1", "message_id": "m-1", "message_sequence": 3, "role": "user"}})
    }

    /// `org_core`: org-core's posture answer; `decision`: Control's answer to the stream decision;
    /// `upstream`: model-gateway's answer to the event stream. Flag ON.
    async fn fixture(org_core: ResponseTemplate, decision: ResponseTemplate, upstream: ResponseTemplate) -> Fx {
        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                "session": {"activeOrganizationId": "org-1"}
            })))
            .mount(&auth)
            .await;
        for slug in ["session-core", "model-plane"] {
            Mock::given(wm_method("GET"))
                .and(wm_path(format!("/api/{slug}/token")))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({"token": "plane-token"})),
                )
                .mount(&auth)
                .await;
        }
        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId": "user-1", "orgId": "org-1", "role": "member",
                "onboardingStatus": "COMPLETED"
            })))
            .mount(&user_core)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/internal/spaces/thread-read-decision"))
            .respond_with(decision)
            .mount(&user_core)
            .await;
        let org = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/organizations/org-1"))
            .respond_with(org_core)
            .mount(&org)
            .await;
        let model_gateway = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path(format!("/v1/spaces/{SPACE}/events")))
            .respond_with(upstream)
            .mount(&model_gateway)
            .await;
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.org_core_url = org.uri();
        state.model_gateway_url = model_gateway.uri();
        state.space_features.realtime_stream = true;
        Fx {
            state,
            _auth: auth,
            user_core,
            _org_core: org,
            model_gateway,
        }
    }

    async fn default_fixture(upstream: ResponseTemplate) -> Fx {
        fixture(org_posture(false), decision_ok(), upstream).await
    }

    fn request(extra: Option<(&str, &str)>) -> Request<Body> {
        let mut builder = Request::builder()
            .method("GET")
            .uri(format!("/api/v1/spaces/{SPACE}/events"))
            .header("cookie", "better-auth.session_token=session-1");
        if let Some((name, value)) = extra {
            builder = builder.header(name, value);
        }
        builder.body(Body::empty()).unwrap()
    }

    async fn get(app: &Router, extra: Option<(&str, &str)>) -> Response {
        app.clone().oneshot(request(extra)).await.unwrap()
    }

    fn app(state: &AppState) -> Router {
        crate::build_router(state.clone())
    }

    async fn text(response: Response) -> (u16, String) {
        let status = response.status().as_u16();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    async fn upstream_requests(fx: &Fx) -> Vec<wiremock::Request> {
        fx.model_gateway.received_requests().await.unwrap_or_default()
    }

    async fn control_decision_requests(fx: &Fx) -> Vec<wiremock::Request> {
        fx.user_core
            .received_requests()
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.url.path().starts_with("/api/v1/internal/spaces/"))
            .collect()
    }

    fn user_without_membership() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.com".to_owned(),
            user_name: "User One".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: None,
        }
    }

    // ---- route level --------------------------------------------------------------------

    #[tokio::test]
    async fn flag_off_is_404_space_stream_disabled() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut fx = default_fixture(sse(&connected_block())).await;
        fx.state.space_features.realtime_stream = false;
        let (status, body) = text(get(&app(&fx.state), None).await).await;
        assert_eq!(status, 404);
        assert!(body.contains("space_stream_disabled"), "{body}");
        assert!(control_decision_requests(&fx).await.is_empty());
        assert!(upstream_requests(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn no_membership_is_403_active_membership_required() {
        let state = crate::tests::test_state(false);
        let mut state = state;
        state.space_features.realtime_stream = true;
        let response = space_events(
            State(state),
            Extension(user_without_membership()),
            Extension(Arc::new(StreamRuntime::new())),
            HeaderMap::new(),
            Path(SPACE.to_owned()),
        )
        .await;
        let (status, body) = text(response).await;
        assert_eq!(status, 403);
        assert!(body.contains("active_membership_required"), "{body}");
    }

    #[tokio::test]
    async fn no_membership_through_the_router_is_refused_before_the_handler() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = default_fixture(sse(&connected_block())).await;
        // Session context without an org: the live membership authority refuses first.
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({"error": "no"})))
            .with_priority(1)
            .mount(&fx.user_core)
            .await;
        let (status, _) = text(get(&app(&fx.state), None).await).await;
        assert!(status == 403 || status == 503, "status {status}");
        assert!(upstream_requests(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn a_bad_space_ref_is_400_invalid_space() {
        let mut state = crate::tests::test_state(false);
        state.space_features.realtime_stream = true;
        let mut user = user_without_membership();
        user.authorized_membership = Some(crate::middleware::AuthorizedMembership {
            organization_id: "org-1".to_owned(),
            role: "member".to_owned(),
        });
        for bad in ["bad ref", "tab\tref", &"a".repeat(129), "  "] {
            let response = space_events(
                State(state.clone()),
                Extension(user.clone()),
                Extension(Arc::new(StreamRuntime::new())),
                HeaderMap::new(),
                Path(bad.to_owned()),
            )
            .await;
            let (status, body) = text(response).await;
            assert_eq!(status, 400, "{bad:?}");
            assert!(body.contains("invalid_space"), "{body}");
        }
    }

    #[tokio::test]
    async fn a_bad_last_event_id_is_400_invalid_cursor() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = default_fixture(sse(&connected_block())).await;
        let router = app(&fx.state);
        for bad in ["abc", "1.x", ".5", "1.", "1.2.3", "1", "-1.2", "1 .2"] {
            let (status, body) = text(get(&router, Some(("last-event-id", bad))).await).await;
            assert_eq!(status, 400, "{bad:?}");
            assert!(body.contains("invalid_cursor"), "{body}");
        }
        assert!(control_decision_requests(&fx).await.is_empty());
        assert!(upstream_requests(&fx).await.is_empty());
        // A well-formed cursor passes and is forwarded as Last-Event-ID.
        let (status, _) = text(get(&router, Some(("last-event-id", "7.42"))).await).await;
        assert_eq!(status, 200);
        let upstream = upstream_requests(&fx).await;
        assert_eq!(upstream.len(), 1);
        assert_eq!(
            upstream[0].headers.get("last-event-id").and_then(|v| v.to_str().ok()),
            Some("7.42")
        );
    }

    #[tokio::test]
    async fn the_31st_subscribe_in_a_minute_is_429_rate_limited_with_retry_after() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        // Org ZDR keeps every call cheap (no upstream) and releases its slot at once.
        let fx = fixture(org_posture(true), decision_ok(), sse(&connected_block())).await;
        let router = app(&fx.state);
        for attempt in 1..=30 {
            let (status, _) = text(get(&router, None).await).await;
            assert_eq!(status, 200, "attempt {attempt}");
        }
        let response = get(&router, None).await;
        assert_eq!(response.status().as_u16(), 429);
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .expect("Retry-After");
        assert!(retry_after >= 1);
        let (_, body) = text(response).await;
        assert!(body.contains("rate_limited"), "{body}");
    }

    #[tokio::test]
    async fn the_13th_concurrent_stream_is_429_capacity_and_the_slot_is_released_on_drop() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = default_fixture(sse(&connected_block())).await;
        let router = app(&fx.state);
        let mut open = Vec::new();
        for attempt in 1..=12 {
            let response = get(&router, None).await;
            assert_eq!(response.status().as_u16(), 200, "stream {attempt}");
            open.push(response); // body unread: the slot is held while the response lives
        }
        let response = get(&router, None).await;
        assert_eq!(response.status().as_u16(), 429);
        assert!(response.headers().get("retry-after").is_some());
        let (_, body) = text(response).await;
        assert!(body.contains("space_stream_capacity"), "{body}");
        // Dropping one stream frees its slot.
        drop(open.pop());
        let response = get(&router, None).await;
        assert_eq!(response.status().as_u16(), 200, "slot released on drop");
    }

    #[tokio::test]
    async fn org_zdr_gets_poll_only_then_closing_with_no_control_or_upstream_call() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(org_posture(true), decision_ok(), sse(&connected_block())).await;
        let (status, body) = text(get(&app(&fx.state), None).await).await;
        assert_eq!(status, 200);
        assert!(body.starts_with("event: connected\n"), "{body}");
        assert!(body.contains("\"mode\":\"poll_only\""), "{body}");
        assert!(body.contains("event: closing\n"), "{body}");
        assert!(body.contains("\"code\":\"zdr_poll_only\""), "{body}");
        assert!(body.contains("\"reconnect\":false"), "{body}");
        assert!(control_decision_requests(&fx).await.is_empty());
        assert!(upstream_requests(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn org_core_down_and_the_zdr_header_also_give_poll_only() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(ResponseTemplate::new(500), decision_ok(), sse(&connected_block())).await;
        let (status, body) = text(get(&app(&fx.state), None).await).await;
        assert_eq!(status, 200);
        assert!(body.contains("zdr_poll_only"), "fails closed: {body}");
        assert!(control_decision_requests(&fx).await.is_empty());
        assert!(upstream_requests(&fx).await.is_empty());

        let fx = default_fixture(sse(&connected_block())).await;
        let (status, body) = text(get(&app(&fx.state), Some(("x-zdr", "true"))).await).await;
        assert_eq!(status, 200);
        assert!(body.contains("zdr_poll_only"), "{body}");
        assert!(control_decision_requests(&fx).await.is_empty());
        assert!(upstream_requests(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn control_403_and_404_give_403_stream_forbidden_and_5xx_gives_503() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for control in [403_u16, 404] {
            let fx = fixture(
                org_posture(false),
                ResponseTemplate::new(control).set_body_json(json!({"error": "no"})),
                sse(&connected_block()),
            )
            .await;
            let (status, body) = text(get(&app(&fx.state), None).await).await;
            assert_eq!(status, 403, "control {control}");
            assert!(body.contains("space_stream_forbidden"), "{body}");
            assert!(upstream_requests(&fx).await.is_empty());
        }
        let fx = fixture(
            org_posture(false),
            ResponseTemplate::new(500).set_body_json(json!({"error": "down"})),
            sse(&connected_block()),
        )
        .await;
        let (status, body) = text(get(&app(&fx.state), None).await).await;
        assert_eq!(status, 503);
        assert!(body.contains("space_stream_unavailable"), "{body}");
        assert!(upstream_requests(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn the_mint_names_the_stream_audience_and_the_decision_only_travels_as_headers() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = default_fixture(sse(&connected_block())).await;
        let (status, body) = text(get(&app(&fx.state), None).await).await;
        assert_eq!(status, 200);
        assert!(!body.contains(DECISION_REF), "decision ref leaked: {body}");
        assert!(!body.contains(DECISION_TOKEN), "decision token leaked: {body}");
        assert!(!body.contains("plane-token"), "bearer leaked: {body}");

        let mints = control_decision_requests(&fx).await;
        assert_eq!(mints.len(), 1);
        assert_eq!(mints[0].url.path(), "/api/v1/internal/spaces/thread-read-decision");
        let sent: Value = serde_json::from_slice(&mints[0].body).unwrap();
        assert_eq!(sent["service_audience"], "model-plane-space-stream");
        assert_eq!(sent["space_ref"], SPACE);
        assert!(sent["idempotency_key"]
            .as_str()
            .unwrap_or_default()
            .starts_with("space-stream-"));

        let upstream = upstream_requests(&fx).await;
        assert_eq!(upstream.len(), 1);
        let header = |name: &str| {
            upstream[0]
                .headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        assert_eq!(header("x-space-decision-ref").as_deref(), Some(DECISION_REF));
        assert_eq!(header("x-space-decision-token").as_deref(), Some(DECISION_TOKEN));
        assert_eq!(header("x-org-id").as_deref(), Some("org-1"));
        assert_eq!(header("x-user-id").as_deref(), Some("user-1"));
        let body_text = String::from_utf8_lossy(&upstream[0].body);
        assert!(!body_text.contains(DECISION_TOKEN), "token must not ride in a body");
        assert!(upstream[0].url.query().unwrap_or("").find("token").is_none());
    }

    #[tokio::test]
    async fn upstream_statuses_map_to_fixed_browser_errors() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let cases: Vec<(ResponseTemplate, u16, &str, bool)> = vec![
            (ResponseTemplate::new(401).set_body_string("leak-401"), 401, "unauthorized", false),
            (ResponseTemplate::new(403).set_body_string("leak-403"), 403, "space_stream_forbidden", false),
            (ResponseTemplate::new(400).set_body_string("leak-400"), 502, "invalid_model_gateway_response", false),
            (
                ResponseTemplate::new(503)
                    .insert_header("retry-after", "3")
                    .set_body_string("leak-503"),
                429,
                "space_stream_capacity",
                true,
            ),
            (ResponseTemplate::new(503).set_body_string("leak-503b"), 503, "space_stream_unavailable", false),
            (ResponseTemplate::new(500).set_body_string("leak-500"), 503, "space_stream_unavailable", false),
        ];
        for (template, expected_status, expected_code, has_retry_after) in cases {
            let fx = default_fixture(template).await;
            let response = get(&app(&fx.state), None).await;
            assert_eq!(response.status().as_u16(), expected_status, "{expected_code}");
            assert_eq!(response.headers().get("retry-after").is_some(), has_retry_after);
            let (_, body) = text(response).await;
            assert!(body.contains(expected_code), "{body}");
            assert!(!body.contains("leak-"), "upstream body must never be forwarded: {body}");
        }
        // A 200 that is not an event stream is refused.
        let fx = default_fixture(
            ResponseTemplate::new(200).set_body_raw(b"{}".to_vec(), "application/json"),
        )
        .await;
        let (status, body) = text(get(&app(&fx.state), None).await).await;
        assert_eq!(status, 502);
        assert!(body.contains("invalid_model_gateway_response"), "{body}");
    }

    #[tokio::test]
    async fn a_valid_stream_is_relayed_reserialised_and_audience_revision_is_stripped() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut event = space_event_data(6);
        event["audience_revision"] = json!(9);
        let upstream = format!(
            "{}{}{}{}",
            connected_block(),
            sse_block("space_event", Some("1.6"), &event),
            sse_block("heartbeat", Some("1.6"), &json!({"head": 6, "generation": 1})),
            sse_block("closing", Some("1.6"), &json!({"code": "lease_expired", "reconnect": true, "retry_after_ms": 0})),
        );
        let fx = default_fixture(sse(&upstream)).await;
        let (status, body) = text(get(&app(&fx.state), None).await).await;
        assert_eq!(status, 200);
        assert!(body.contains("event: connected\nid: 1.5\n"), "{body}");
        assert!(body.contains("event: space_event\nid: 1.6\n"), "{body}");
        assert!(body.contains("event: heartbeat\n"), "{body}");
        assert!(body.trim_end().ends_with("\"retry_after_ms\":0}"), "{body}");
        assert!(body.contains("event: closing\n"), "{body}");
        assert!(!body.contains("audience_revision"), "{body}");
        assert!(!body.contains("invalid_frame"), "{body}");
    }

    #[tokio::test]
    async fn a_violating_frame_ends_the_stream_with_closing_invalid_frame() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut leaking = space_event_data(6);
        leaking["content"] = json!("the secret message text");
        let upstream = format!(
            "{}{}{}",
            connected_block(),
            sse_block("space_event", Some("1.6"), &leaking),
            sse_block("heartbeat", None, &json!({"head": 9, "generation": 1})),
        );
        let fx = default_fixture(sse(&upstream)).await;
        let (status, body) = text(get(&app(&fx.state), None).await).await;
        assert_eq!(status, 200);
        assert!(body.contains("event: connected\n"));
        assert!(!body.contains("secret message text"), "{body}");
        assert!(body.contains("event: closing\n"), "{body}");
        assert!(body.contains("\"code\":\"invalid_frame\""), "{body}");
        assert!(body.contains("\"reconnect\":true"), "{body}");
        assert!(!body.contains("event: heartbeat"), "nothing after the violation: {body}");
    }

    #[tokio::test]
    async fn an_upstream_that_ends_without_closing_gets_closing_upstream_unavailable() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = default_fixture(sse(&connected_block())).await;
        let (status, body) = text(get(&app(&fx.state), None).await).await;
        assert_eq!(status, 200);
        assert!(body.contains("event: connected\n"));
        assert!(body.contains("\"code\":\"upstream_unavailable\""), "{body}");
        assert!(body.contains("\"reconnect\":true"), "{body}");
    }

    // ---- validator ----------------------------------------------------------------------

    fn raw(event: &str, id: Option<&str>, data: &Value) -> RawEvent {
        RawEvent {
            event: event.to_owned(),
            id: id.map(str::to_owned),
            data: data.to_string(),
        }
    }

    fn rejected(event: &RawEvent) -> bool {
        validate_event(event, SPACE).is_err()
    }

    #[test]
    fn the_validator_drops_each_contract_violation() {
        // Extra `content` field.
        let mut with_content = space_event_data(1);
        with_content["content"] = json!("text");
        assert!(rejected(&raw("space_event", Some("1.1"), &with_content)));
        // Foreign space id.
        let mut foreign = space_event_data(1);
        foreign["space_id"] = json!("space-other");
        assert!(rejected(&raw("space_event", Some("1.1"), &foreign)));
        // Unknown kind carrying refs.
        let mut unknown_with_refs = space_event_data(1);
        unknown_with_refs["kind"] = json!("brand.new.kind");
        assert!(rejected(&raw("space_event", Some("1.1"), &unknown_with_refs)));
        // A non-registry event name.
        assert!(rejected(&raw("mystery", None, &json!({}))));
        assert!(rejected(&raw("", None, &json!({}))));
        // An oversized string.
        let mut oversized = space_event_data(1);
        oversized["refs"]["thread_id"] = json!("t".repeat(129));
        assert!(rejected(&raw("space_event", Some("1.1"), &oversized)));
        // An SSE id that is not this frame's cursor, or not a cursor at all.
        assert!(rejected(&raw("space_event", Some("1.2"), &space_event_data(1))));
        assert!(rejected(&raw("space_event", Some("junk"), &space_event_data(1))));
        // A ref outside the kind allowlist, and a ref of the wrong type.
        let mut stray_ref = space_event_data(1);
        stray_ref["refs"]["agent_id"] = json!("agent-1");
        assert!(rejected(&raw("space_event", Some("1.1"), &stray_ref)));
        let mut bad_type = space_event_data(1);
        bad_type["refs"]["message_sequence"] = json!("3");
        assert!(rejected(&raw("space_event", Some("1.1"), &bad_type)));
        // Version, source and actor type outside the contract.
        for (field, value) in [
            ("v", json!(2)),
            ("source", json!("application")),
            ("actor", json!({"type": "robot"})),
            ("at", json!("")),
        ] {
            let mut event = space_event_data(1);
            event[field] = value;
            assert!(rejected(&raw("space_event", Some("1.1"), &event)), "{field}");
        }
        // Malformed JSON.
        assert!(rejected(&RawEvent {
            event: "heartbeat".into(),
            id: None,
            data: "{not json".into(),
        }));
        // Closing: a code outside the closed set; connected: a foreign space or unknown mode.
        assert!(rejected(&raw("closing", None, &json!({"code": "other", "reconnect": true, "retry_after_ms": 0}))));
        assert!(rejected(&raw(
            "connected",
            None,
            &json!({"v": 1, "space_id": "space-other", "generation": 1, "head": 0, "resumed": false,
                    "lease_seconds": 1, "heartbeat_seconds": 1, "mode": "live"})
        )));
        assert!(rejected(&raw(
            "connected",
            None,
            &json!({"v": 1, "space_id": SPACE, "generation": 1, "head": 0, "resumed": false,
                    "lease_seconds": 1, "heartbeat_seconds": 1, "mode": "weird"})
        )));
        // Error frame with a code outside the set.
        assert!(rejected(&raw("error", None, &json!({"error": {"code": "boom", "message": "m"}}))));
        // cursor_too_old with an unknown reason.
        assert!(rejected(&raw("cursor_too_old", None, &json!({"reason": "because", "head": 1, "generation": 1}))));
    }

    #[test]
    fn an_unknown_kind_without_refs_is_forwarded_as_hidden() {
        let mut unknown = space_event_data(4);
        unknown["kind"] = json!("brand.new.kind");
        unknown["refs"] = json!({});
        let frame = validate_event(&raw("space_event", Some("1.4"), &unknown), SPACE).expect("forwarded");
        assert!(!frame.closing);
        assert!(frame.text.contains("\"kind\":\"hidden\""), "{}", frame.text);
        assert!(!frame.text.contains("brand.new.kind"));
        assert!(frame.text.starts_with("event: space_event\nid: 1.4\n"));
    }

    #[test]
    fn audience_revision_is_stripped_and_every_registry_kind_validates() {
        let mut event = space_event_data(2);
        event["audience_revision"] = json!(12);
        let frame = validate_event(&raw("space_event", Some("1.2"), &event), SPACE).unwrap();
        assert!(!frame.text.contains("audience_revision"));
        for kind in [
            "thread.created", "message.appended", "thread.updated", "thread.archived",
            "thread.deleted", "run.state_changed", "hidden", "redacted",
        ] {
            let refs = match kind {
                "thread.created" => json!({"thread_id": "t-1", "origin": "human"}),
                "message.appended" => json!({"thread_id": "t-1", "message_id": "m-1", "message_sequence": 1, "role": "user"}),
                "thread.updated" => json!({"thread_id": "t-1", "fields": ["title", "pinned"]}),
                "thread.archived" | "thread.deleted" => json!({"thread_id": "t-1"}),
                "run.state_changed" => json!({"run_id": "r-1", "thread_id": "t-1", "state": "running", "prev": "queued"}),
                _ => json!({}),
            };
            let mut event = space_event_data(3);
            event["kind"] = json!(kind);
            event["refs"] = refs;
            assert!(validate_event(&raw("space_event", Some("1.3"), &event), SPACE).is_ok(), "{kind}");
        }
        // Closing and error frames end the stream.
        let closing = validate_event(
            &raw("closing", None, &json!({"code": "membership_revoked", "reconnect": false, "retry_after_ms": 0})),
            SPACE,
        )
        .unwrap();
        assert!(closing.closing);
        let error = validate_event(
            &raw("error", None, &json!({"error": {"code": "unauthorized", "message": "m", "details": {"x": 1}}})),
            SPACE,
        )
        .unwrap();
        assert!(error.closing);
        assert!(error.text.contains("\"details\":{}"), "details dropped: {}", error.text);
    }

    // ---- SSE parser ---------------------------------------------------------------------

    #[test]
    fn the_parser_handles_split_chunks_crlf_and_comments() {
        let mut parser = SseParser::default();
        let mut events = Vec::new();
        for chunk in ["eve", "nt: heart", "beat\r\nid: 1.", "2\r\ndata: {\"a\"", ":1}\r\n", "\r\n: keep-alive\n\n"] {
            events.extend(parser.push(chunk.as_bytes()).expect("parse"));
        }
        assert_eq!(events.len(), 1, "{events:?}");
        assert_eq!(events[0].event, "heartbeat");
        assert_eq!(events[0].id.as_deref(), Some("1.2"));
        assert_eq!(events[0].data, "{\"a\":1}");
        // A comment-only block yields no event, and two data lines join with a newline.
        let events = parser.push(b": hi\n\nevent: x\ndata: one\ndata: two\n\n").unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "one\ntwo");
    }

    #[test]
    fn the_parser_bounds_a_line_and_an_event() {
        let mut parser = SseParser::default();
        assert!(parser.push("x".repeat(MAX_FRAME_BYTES + 1).as_bytes()).is_err(), "oversize line");
        let mut parser = SseParser::default();
        let mut result = Ok(Vec::new());
        for _ in 0..8 {
            result = parser.push(format!("data: {}\n", "y".repeat(4096)).as_bytes());
            if result.is_err() {
                break;
            }
        }
        assert!(result.is_err(), "oversize event");
    }

    // ---- golden -------------------------------------------------------------------------

    fn golden() -> Value {
        serde_json::from_str(GOLDEN).expect("vendored space-events.json parses")
    }

    fn words(value: &Value) -> std::collections::BTreeSet<String> {
        value
            .as_array()
            .expect("array")
            .iter()
            .map(|word| word.as_str().expect("string").to_owned())
            .collect()
    }

    fn owned(set: &[&str]) -> std::collections::BTreeSet<String> {
        set.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn the_kind_and_ref_tables_equal_the_golden() {
        let golden = golden();
        let kinds = golden["kinds"].as_object().expect("kinds");
        let mut all_refs = std::collections::BTreeSet::new();
        for (kind, entry) in kinds {
            let ours = kind_ref_keys(kind).unwrap_or_else(|| panic!("{kind} missing from stream.rs"));
            assert_eq!(owned(ours), words(&entry["refs"]), "refs of {kind}");
            all_refs.extend(words(&entry["refs"]));
        }
        assert_eq!(all_refs, words(&golden["ref_keys"]), "ref_keys is the union of the kind refs");
        assert!(kind_ref_keys("not.in.the.registry").is_none());
    }

    #[test]
    fn the_closed_word_tables_equal_the_golden() {
        let golden = golden();
        let closing: std::collections::BTreeSet<String> = golden["events"]["closing"]["codes"]
            .as_object()
            .expect("closing codes")
            .keys()
            .cloned()
            .collect();
        assert_eq!(owned(CLOSING_CODES), closing);
        assert_eq!(owned(ERROR_CODES), words(&golden["events"]["error"]["codes"]));
        assert_eq!(owned(RUN_STATES), words(&golden["closed_words"]["run_state"]));
        assert_eq!(owned(UPDATED_FIELDS), words(&golden["closed_words"]["thread_updated_fields"]));
        // Gateway-emitted closing codes keep their reconnect flag.
        assert_eq!(golden["events"]["closing"]["codes"]["invalid_frame"]["reconnect"], true);
        assert_eq!(golden["events"]["closing"]["codes"]["upstream_unavailable"]["reconnect"], true);
        assert_eq!(golden["events"]["closing"]["codes"]["zdr_poll_only"]["reconnect"], false);
        assert_eq!(golden["events"]["connected"]["mode"], json!(["live", "poll_only"]));
        assert_eq!(golden["events"]["cursor_too_old"]["reason"], json!(["retention", "generation_changed", "ahead_of_head"]));
    }

    #[test]
    fn the_cursor_grammar_is_generation_dot_seq() {
        assert!(valid_cursor("1.2"));
        assert!(valid_cursor("123456789012345678.0"));
        for bad in ["", "1", "1.", ".1", "a.b", "1.2.3", "1234567890123456789.1"] {
            assert!(!valid_cursor(bad), "{bad:?}");
        }
        assert_eq!(golden()["cursor"]["grammar"], "<generation>.<seq>");
    }
}
