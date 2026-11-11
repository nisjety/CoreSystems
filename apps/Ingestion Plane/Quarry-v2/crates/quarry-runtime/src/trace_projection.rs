//! Default-deny projection of Quarry agent events onto the Model Plane run-trace
//! envelope (WS10.S4).
//!
//! Pure functions, no I/O and no switch: the publisher (`model_trace_publisher`,
//! WS10.S5) decides whether anything is sent. This module only decides WHAT may
//! leave the process for one event:
//!
//! * a ZDR run yields nothing (the run trace is durable on the Model side);
//! * only the event types listed in [`model_event_type`] map; every other type
//!   (actions, observations, crawl, scrape, ...) yields nothing, and a new
//!   `EventType` variant stays unmapped until someone adds a row here;
//! * each mapped type has a payload allowlist; unknown keys are dropped, not
//!   redacted, and only scalar values survive (no nested objects or arrays);
//! * strings longer than [`MAX_STRING_CHARS`] are dropped;
//! * URLs leave only as an origin (`scheme://host[:port]`): path, query,
//!   fragment and typed text never do;
//! * a serialized payload over [`MAX_PAYLOAD_BYTES`] becomes
//!   `{"truncated": true, "browser_run_id": ...}`.
//!
//! The envelope is the JSON twin of `model_plane.v1.Event` as
//! `mp_events::envelope::Envelope` decodes it (snake_case keys, `ts` RFC 3339,
//! enum names without their `EVENT_TYPE_` / `RUN_ORIGIN_` prefix), with `zdr`
//! written explicitly as `false`: session-core's `validate_trace_envelope`
//! refuses an envelope whose `zdr` key is absent.
//!
//! `origin` and `agent` are labels copied from `Event::trace`; they are never
//! authority. Org and user come from [`PublishCtx`], which the caller fills from
//! the run's verified token.

use quarry_core::event::{Event, EventType};
use quarry_core::trace::{valid_token, AgentRef, TRACE_TOKEN_MAX};
use serde_json::{Map, Value};

pub use crate::nats_event_bus::is_agent_class;

/// The largest serialized payload (bytes) sent as-is; anything larger is replaced
/// by a `{"truncated": true, ...}` marker. The Model side caps at 8192.
pub const MAX_PAYLOAD_BYTES: usize = 4096;

/// The longest string value (in chars) a payload key may carry; longer is dropped.
pub const MAX_STRING_CHARS: usize = 256;

/// The `producer` every Quarry trace event carries.
pub const TRACE_PRODUCER: &str = "quarry-edge";

/// The envelope schema version for every mapped type.
pub const TRACE_SCHEMA_VERSION: u32 = 1;

/// The longest org id the Model side accepts.
const MAX_ORG_ID_LEN: usize = 128;

/// The longest correlation / causation label the Model side accepts.
const MAX_LABEL_LEN: usize = 128;

/// The run-level context the projection needs and an event does not carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishCtx {
    /// The Model Plane run id (the subject token and the envelope `run_id`).
    pub model_run_id: String,
    /// The run's org, from the verified token.
    pub org_id: String,
    /// The acting principal, from the verified token.
    pub actor_id: String,
    /// The Quarry browser (agent) run id.
    pub browser_run_id: String,
    /// The run's Zero Data Retention posture; a ZDR run projects nothing.
    pub zdr: bool,
}

/// The Model Plane event type name (without `EVENT_TYPE_`) for a Quarry event
/// type, or `None` when the type does not leave the process (default deny).
#[must_use]
pub fn model_event_type(event_type: &EventType) -> Option<&'static str> {
    match event_type {
        EventType::AgentStarted | EventType::BrowserSessionOpened => Some("BROWSER_SESSION_OPENED"),
        EventType::AgentCompleted
        | EventType::AgentFailed
        | EventType::AgentExpired
        | EventType::BrowserSessionClosed => Some("BROWSER_SESSION_CLOSED"),
        EventType::BrowserControlChanged => Some("BROWSER_CONTROL_CHANGED"),
        EventType::BrowserTabChanged => Some("BROWSER_TAB_CHANGED"),
        EventType::StepReceiptRecorded => Some("BROWSER_STEP_RECEIPT_RECORDED"),
        EventType::EgressDecided => Some("BROWSER_EGRESS_DECIDED"),
        EventType::ArtifactRecorded => Some("BROWSER_ARTIFACT_RECORDED"),
        // Default deny: actions, observations, budget, attach, crawl and scrape
        // types (and every future variant) stay inside Quarry.
        _ => None,
    }
}

/// Project one Quarry event onto the Model Plane run-trace envelope.
///
/// `None` when the run is ZDR, when `ctx.model_run_id` is not 1-64 chars of
/// `[A-Za-z0-9_-]`, when the org id is empty or too long, when the event is not
/// numbered (`seq` 0), or when the type does not map.
#[must_use]
pub fn to_model_envelope(event: &Event, ctx: &PublishCtx) -> Option<Value> {
    if ctx.zdr {
        return None;
    }
    if !valid_token(&ctx.model_run_id, TRACE_TOKEN_MAX) {
        return None;
    }
    if ctx.org_id.is_empty() || ctx.org_id.len() > MAX_ORG_ID_LEN {
        return None;
    }
    if event.seq == 0 {
        return None;
    }
    let event_type = model_event_type(&event.event_type)?;

    let trace = event.trace.as_ref();
    let causation_id = trace
        .and_then(|t| t.causation_event_id.as_deref())
        .map(bare_event_id)
        .filter(|id| id.len() <= MAX_LABEL_LEN && !id.contains('\0'))
        .unwrap_or_default();

    let mut envelope = Map::new();
    envelope.insert(
        "event_id".into(),
        Value::String(event.event_id.ulid().to_string()),
    );
    envelope.insert("event_type".into(), Value::String(event_type.into()));
    envelope.insert("schema_version".into(), Value::from(TRACE_SCHEMA_VERSION));
    envelope.insert(
        "ts".into(),
        Value::String(
            event
                .ts
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        ),
    );
    envelope.insert("producer".into(), Value::String(TRACE_PRODUCER.into()));
    envelope.insert(
        "correlation_id".into(),
        Value::String(ctx.model_run_id.clone()),
    );
    envelope.insert("causation_id".into(), Value::String(causation_id));
    envelope.insert(
        "idempotency_key".into(),
        Value::String(format!("quarry:{}:{}", ctx.browser_run_id, event.seq)),
    );
    envelope.insert("org_id".into(), Value::String(ctx.org_id.clone()));
    envelope.insert("user_id".into(), Value::String(ctx.actor_id.clone()));
    envelope.insert(
        "resource_ref".into(),
        Value::String(format!("browser_run/{}", ctx.browser_run_id)),
    );
    envelope.insert(
        "payload".into(),
        project_payload(&event.event_type, &event.payload, ctx),
    );
    // Always explicit: the Model side refuses an envelope without the key.
    envelope.insert("zdr".into(), Value::Bool(false));
    envelope.insert("producer_seq".into(), Value::from(event.seq));
    if let Some(agent) = trace.and_then(|t| t.agent.as_ref()).and_then(agent_value) {
        envelope.insert("agent".into(), agent);
    }
    if let Some(origin) = trace.and_then(|t| t.origin) {
        envelope.insert("origin".into(), Value::String(origin.as_model_str().into()));
    }
    envelope.insert("run_id".into(), Value::String(ctx.model_run_id.clone()));
    if let Some(request_id) = trace
        .and_then(|t| t.request_id.as_deref())
        .filter(|r| valid_token(r, TRACE_TOKEN_MAX))
    {
        envelope.insert("request_id".into(), Value::String(request_id.into()));
    }
    Some(Value::Object(envelope))
}

/// A Quarry event id without its `evt_` kind prefix (a bare ULID). Other
/// strings pass through unchanged.
fn bare_event_id(id: &str) -> String {
    id.strip_prefix("evt_").unwrap_or(id).to_string()
}

/// The `agent` object with only valid tokens, or `None` when nothing is left.
fn agent_value(agent: &AgentRef) -> Option<Value> {
    let mut out = Map::new();
    for (key, value) in [
        ("id", agent.id.as_deref()),
        ("role", agent.role.as_deref()),
        ("parent_id", agent.parent_id.as_deref()),
    ] {
        if let Some(v) = value.filter(|v| valid_token(v, TRACE_TOKEN_MAX)) {
            out.insert(key.into(), Value::String(v.into()));
        }
    }
    (!out.is_empty()).then_some(Value::Object(out))
}

/// The per-type payload allowlist (keys besides `browser_run_id`, which every
/// mapped payload carries from the context).
fn allowlist(event_type: &EventType) -> &'static [&'static str] {
    match event_type {
        EventType::AgentStarted | EventType::BrowserSessionOpened => {
            &["execution_tier", "driver", "profile_scope", "control_mode"]
        }
        EventType::AgentCompleted
        | EventType::AgentFailed
        | EventType::AgentExpired
        | EventType::BrowserSessionClosed => &["reason", "steps", "duration_ms"],
        EventType::BrowserControlChanged => &["mode", "initiated_by", "control_epoch"],
        EventType::BrowserTabChanged => &["operation", "tab_id", "active_tab_id"],
        EventType::StepReceiptRecorded => &[
            "receipt_id",
            "step",
            "action",
            "outcome",
            "latency_ms",
            "error_code",
            "cost_micro_usd",
            "url_origin",
            "verified",
            "initiated_by",
            "control_epoch",
        ],
        EventType::EgressDecided => &[
            "step",
            "decision",
            "policy",
            "origin",
            "tab_id",
            "egress_seq_from",
            "egress_seq_to",
            "allowed",
            "blocked",
        ],
        EventType::ArtifactRecorded => &["step", "artifact_id", "kind", "bytes", "content_type"],
        _ => &[],
    }
}

/// The closed set of session-close reasons the Model side shows.
const CLOSE_REASONS: [&str; 4] = ["closed", "expired", "failed", "cancelled"];

/// Build the allowlisted payload for one event.
fn project_payload(event_type: &EventType, raw: &Value, ctx: &PublishCtx) -> Value {
    let mut out = Map::new();
    if let Some(source) = raw.as_object() {
        for key in allowlist(event_type) {
            let Some(value) = source.get(*key) else {
                continue;
            };
            let kept = match *key {
                "url_origin" | "origin" => value.as_str().and_then(url_origin).map(Value::String),
                "decision" => value
                    .as_str()
                    .filter(|d| matches!(*d, "block" | "summary"))
                    .map(|d| Value::String(d.into())),
                "reason" => value
                    .as_str()
                    .filter(|r| CLOSE_REASONS.contains(r))
                    .map(|r| Value::String(r.into())),
                _ => scalar(value),
            };
            if let Some(kept) = kept {
                out.insert((*key).to_string(), kept);
            }
        }
    }
    // A close without a recognised reason takes the reason its type implies.
    if matches!(
        event_type,
        EventType::AgentCompleted | EventType::AgentFailed | EventType::AgentExpired
    ) && !out.contains_key("reason")
    {
        let reason = match event_type {
            EventType::AgentFailed => "failed",
            EventType::AgentExpired => "expired",
            _ => "closed",
        };
        out.insert("reason".into(), Value::String(reason.into()));
    }
    let browser_run_id = browser_run_id_value(ctx);
    out.insert("browser_run_id".into(), browser_run_id.clone());

    let payload = Value::Object(out);
    let size = serde_json::to_vec(&payload)
        .map(|b| b.len())
        .unwrap_or(usize::MAX);
    if size > MAX_PAYLOAD_BYTES {
        let mut marker = Map::new();
        marker.insert("truncated".into(), Value::Bool(true));
        marker.insert("browser_run_id".into(), browser_run_id);
        return Value::Object(marker);
    }
    payload
}

/// The browser run id as a payload value, subject to the same string cap.
fn browser_run_id_value(ctx: &PublishCtx) -> Value {
    if ctx.browser_run_id.chars().count() <= MAX_STRING_CHARS {
        Value::String(ctx.browser_run_id.clone())
    } else {
        Value::Null
    }
}

/// Keep a scalar (bool, number, string of at most [`MAX_STRING_CHARS`] chars).
/// Objects, arrays, nulls and long strings are dropped.
fn scalar(value: &Value) -> Option<Value> {
    match value {
        Value::Bool(_) | Value::Number(_) => Some(value.clone()),
        Value::String(s) if s.chars().count() <= MAX_STRING_CHARS => Some(value.clone()),
        _ => None,
    }
}

/// `scheme://host[:port]` of an http(s) URL; `None` for anything else, so a
/// path, query, fragment or credentials never leave the process.
fn url_origin(raw: &str) -> Option<String> {
    let parsed = url::Url::parse(raw.trim()).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let origin = parsed.origin();
    if !origin.is_tuple() {
        return None;
    }
    let serialized = origin.ascii_serialization();
    (serialized.chars().count() <= MAX_STRING_CHARS).then_some(serialized)
}
