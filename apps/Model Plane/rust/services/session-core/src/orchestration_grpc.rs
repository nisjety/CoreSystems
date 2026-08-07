//! gRPC server for `OrchestrationCoreService` on :9091 (registered alongside
//! `SessionCore`).
//!
//! Owns the read-and-transition surface for plans/todos/approvals/lineage and
//! fans NATS-sourced `OrchestrationEvent`s out to per-run gRPC subscribers via
//! a tokio broadcast channel.
//!
//! Single writer: only this service mutates `plans`, `todos`, `approvals`, and
//! `subagent_edges`. Mutations land via `orchestration_store`, then a typed
//! event is broadcast on `events_tx` so streaming clients see the same view.

use std::collections::{HashMap, VecDeque};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use chrono::{DateTime, Utc};
use futures::{Stream, StreamExt};
use mp_contracts::model_plane::v1::{
    self as proto,
    orchestration_core_service_server::{OrchestrationCoreService, OrchestrationCoreServiceServer},
    orchestration_event,
};
use serde_json::Value as JsonValue;
use sqlx::Row;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tonic::{Request, Response, Status};
use tracing::warn;

use crate::auth::{
    OwnerIntent, VerifiedIdentity, authorize_operation, authorize_owner_row, identity,
};
use crate::orchestration_store as store;
use crate::store::Pool;

/// Channel capacity for broadcast of orchestration events to streaming clients.
pub const EVENTS_CHANNEL_CAPACITY: usize = 1024;

/// Maximum recent events retained per run for `Last-Event-Id` replay. Older
/// events are evicted; a client whose resume cursor predates the buffer must
/// fall back to a fresh snapshot (ListPlans/ListTodos/ListApprovals).
pub const REPLAY_BUFFER_PER_RUN: usize = 256;

/// TTL on Redis-backed per-run event lists.
const RUNEVENTS_TTL_SECS: i64 = 3600;
const RUNEVENTS_PREFIX: &str = "mp:sc:runevents:";

/// In-memory per-run ring buffer (single-replica / dev / Redis unavailable).
#[derive(Clone, Default)]
struct InMemoryReplay {
    inner: Arc<Mutex<HashMap<String, VecDeque<proto::OrchestrationEvent>>>>,
}

impl InMemoryReplay {
    fn push(&self, run_id: &str, ev: &proto::OrchestrationEvent) {
        if let Ok(mut map) = self.inner.lock() {
            let dq = map.entry(run_id.to_owned()).or_default();
            dq.push_back(ev.clone());
            while dq.len() > REPLAY_BUFFER_PER_RUN {
                dq.pop_front();
            }
        }
    }

    fn replay_after(&self, run_id: &str, after: &str) -> Vec<proto::OrchestrationEvent> {
        self.inner
            .lock()
            .ok()
            .and_then(|map| {
                map.get(run_id).map(|dq| {
                    dq.iter()
                        .filter(|e| after.is_empty() || e.event_id.as_str() > after)
                        .cloned()
                        .collect()
                })
            })
            .unwrap_or_default()
    }
}

/// Redis-backed per-run event list (multi-replica). A reconnect that lands on
/// a different session-core replica still resumes from the shared buffer.
#[derive(Clone)]
struct RedisReplay {
    conn: redis::aio::ConnectionManager,
}

impl RedisReplay {
    fn key(run_id: &str) -> String {
        format!("{RUNEVENTS_PREFIX}{run_id}")
    }

    async fn push(&self, run_id: &str, ev: &proto::OrchestrationEvent) {
        use base64::Engine as _;
        use prost::Message as _;

        let mut bytes = Vec::new();
        if ev.encode(&mut bytes).is_err() {
            return;
        }
        let entry = serde_json::json!({
            "event_id": ev.event_id,
            "b64": base64::engine::general_purpose::STANDARD.encode(&bytes),
        })
        .to_string();

        let key = Self::key(run_id);
        let mut conn = self.conn.clone();
        let result: redis::RedisResult<()> = redis::pipe()
            .rpush(&key, entry)
            .ignore()
            .ltrim(
                &key,
                -isize::try_from(REPLAY_BUFFER_PER_RUN).unwrap_or(isize::MAX),
                -1,
            )
            .ignore()
            .expire(&key, RUNEVENTS_TTL_SECS)
            .ignore()
            .query_async(&mut conn)
            .await;
        if let Err(e) = result {
            warn!(error = %e, run_id, "redis run-event push failed");
        }
    }

    async fn replay_after(&self, run_id: &str, after: &str) -> Vec<proto::OrchestrationEvent> {
        use base64::Engine as _;
        use prost::Message as _;
        use redis::AsyncCommands as _;

        let mut conn = self.conn.clone();
        let raw: redis::RedisResult<Vec<String>> = conn.lrange(Self::key(run_id), 0, -1).await;
        let entries = match raw {
            Ok(v) => v,
            Err(e) => {
                warn!(error = %e, run_id, "redis run-event replay failed");
                return Vec::new();
            }
        };
        entries
            .iter()
            .filter_map(|s| {
                let v: serde_json::Value = serde_json::from_str(s).ok()?;
                let event_id = v.get("event_id")?.as_str()?;
                if !after.is_empty() && event_id <= after {
                    return None;
                }
                let b64 = v.get("b64")?.as_str()?;
                let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
                proto::OrchestrationEvent::decode(bytes.as_slice()).ok()
            })
            .collect()
    }
}

/// Per-run replay buffer backing `Last-Event-Id` resume. Redis when
/// `REDIS_URL` is set, else in-memory.
#[derive(Clone)]
enum ReplayBuffer {
    Memory(InMemoryReplay),
    Redis(Box<RedisReplay>),
}

impl ReplayBuffer {
    fn memory() -> Self {
        Self::Memory(InMemoryReplay::default())
    }

    /// Connect to Redis when `REDIS_URL` is set, else in-memory. Never fails —
    /// a Redis outage degrades to single-replica replay.
    async fn from_env() -> Self {
        let Ok(url) = std::env::var("REDIS_URL") else {
            return Self::memory();
        };
        match redis::Client::open(url) {
            Ok(client) => match redis::aio::ConnectionManager::new(client).await {
                Ok(conn) => {
                    tracing::info!("run-event replay buffer: Redis backend active");
                    Self::Redis(Box::new(RedisReplay { conn }))
                }
                Err(e) => {
                    warn!(error = %e, "REDIS_URL set but connect failed; using in-memory replay buffer");
                    Self::memory()
                }
            },
            Err(e) => {
                warn!(error = %e, "invalid REDIS_URL; using in-memory replay buffer");
                Self::memory()
            }
        }
    }

    /// Synchronous dispatch from the broadcast hot path. The in-memory backend
    /// writes inline; the Redis backend spawns a fire-and-forget push (replay
    /// is best-effort with snapshot fallback, so a slightly-late write is fine
    /// and we never block the event broadcast on a Redis round-trip).
    fn push_event(&self, run_id: &str, ev: &proto::OrchestrationEvent) {
        match self {
            Self::Memory(m) => m.push(run_id, ev),
            Self::Redis(r) => {
                let r = r.clone();
                let run_id = run_id.to_owned();
                let ev = ev.clone();
                tokio::spawn(async move {
                    r.push(&run_id, &ev).await;
                });
            }
        }
    }

    async fn replay_after(&self, run_id: &str, after: &str) -> Vec<proto::OrchestrationEvent> {
        match self {
            Self::Memory(m) => m.replay_after(run_id, after),
            Self::Redis(r) => r.replay_after(run_id, after).await,
        }
    }
}

impl Default for ReplayBuffer {
    fn default() -> Self {
        Self::memory()
    }
}

#[allow(clippy::result_large_err)]
async fn authorize_run_owner(
    pool: &Pool,
    caller: &VerifiedIdentity,
    run_id: &str,
    intent: OwnerIntent,
) -> Result<(), Status> {
    let owner: Option<(String, String)> =
        sqlx::query_as("SELECT org_id, user_id FROM runs WHERE id = $1")
            .bind(run_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
    let (org_id, user_id) = owner.ok_or_else(|| Status::not_found("run not found"))?;
    authorize_owner_row(caller, &org_id, &user_id, intent)
}

async fn authorize_thread_owner(
    pool: &Pool,
    caller: &VerifiedIdentity,
    thread_id: &str,
    intent: OwnerIntent,
) -> Result<(), Status> {
    let owner: Option<(String, String)> =
        sqlx::query_as("SELECT org_id, user_id FROM threads WHERE id = $1")
            .bind(thread_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
    let (org_id, user_id) = owner.ok_or_else(|| Status::not_found("thread not found"))?;
    authorize_owner_row(caller, &org_id, &user_id, intent)
}

async fn authorize_plan_owner(
    pool: &Pool,
    caller: &VerifiedIdentity,
    plan_id: &str,
    intent: OwnerIntent,
) -> Result<(), Status> {
    let row = store::get_plan(pool, plan_id)
        .await
        .map_err(|error| Status::internal(error.to_string()))?
        .ok_or_else(|| Status::not_found("plan not found"))?;
    authorize_owner_row(caller, &row.org_id, &row.user_id, intent)
}

async fn authorize_todo_owner(
    pool: &Pool,
    caller: &VerifiedIdentity,
    todo_id: &str,
    intent: OwnerIntent,
) -> Result<(), Status> {
    let owner: Option<(String, String)> = sqlx::query_as(
        "SELECT p.org_id, p.user_id FROM todos t JOIN plans p ON p.id = t.plan_id WHERE t.id = $1",
    )
    .bind(todo_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| Status::internal(error.to_string()))?;
    let (org_id, user_id) = owner.ok_or_else(|| Status::not_found("todo not found"))?;
    authorize_owner_row(caller, &org_id, &user_id, intent)
}

#[allow(clippy::result_large_err)]
fn pin_create_approval_identity(
    caller: &VerifiedIdentity,
    request: &mut proto::CreateApprovalRequest,
) -> Result<(), Status> {
    caller.authorize_org(&request.org_id)?;
    caller
        .user_id()
        .ok_or_else(|| Status::permission_denied("user-bound approval credential required"))?
        .clone_into(&mut request.user_id);
    Ok(())
}

#[allow(clippy::result_large_err)]
fn pin_approval_decision_identity(
    caller: &VerifiedIdentity,
    request: &mut proto::DecideApprovalRequest,
) -> Result<(), Status> {
    caller.authorize_org(&request.org_id)?;
    caller.principal_id().clone_into(&mut request.decided_by);
    Ok(())
}

fn is_idempotent_approval_retry(
    approval: &store::ApprovalRow,
    status: &str,
    decided_by: &str,
    decision_reason: &str,
) -> bool {
    !decided_by.is_empty()
        && approval.status == status
        && approval.decided_by == decided_by
        && approval.decision_reason == decision_reason
}

// ---------------------------------------------------------------------------
// Service state
// ---------------------------------------------------------------------------

/// gRPC service state for `OrchestrationCoreService`.
pub struct OrchestrationGrpc {
    pool: Pool,
    events_tx: broadcast::Sender<proto::OrchestrationEvent>,
    replay: ReplayBuffer,
}

impl OrchestrationGrpc {
    /// Build the service.
    #[must_use]
    #[allow(dead_code)] // constructed by the binary entrypoint once the gRPC service is mounted
    pub fn new(pool: Pool, events_tx: broadcast::Sender<proto::OrchestrationEvent>) -> Self {
        Self {
            pool,
            events_tx,
            replay: ReplayBuffer::default(),
        }
    }

    /// Build the service, using a Redis-backed replay buffer when `REDIS_URL`
    /// is set (multi-replica) and falling back to in-memory otherwise.
    pub async fn new_from_env(
        pool: Pool,
        events_tx: broadcast::Sender<proto::OrchestrationEvent>,
    ) -> Self {
        Self {
            pool,
            events_tx,
            replay: ReplayBuffer::from_env().await,
        }
    }

    /// Wrap into a tonic server ready to register with `Server::builder`.
    #[must_use]
    #[allow(dead_code)] // direct constructor retained for isolated service tests
    pub fn into_server(self) -> OrchestrationCoreServiceServer<Self> {
        OrchestrationCoreServiceServer::new(self)
    }
}

// ---------------------------------------------------------------------------
// Metrics
// ---------------------------------------------------------------------------

fn record_metrics(method: &'static str, started: Instant, is_ok: bool) {
    let status = if is_ok { "ok" } else { "error" };
    metrics::counter!(
        "mp_session_orchestration_grpc_requests_total",
        "method" => method,
        "status" => status,
    )
    .increment(1);
    metrics::histogram!(
        "mp_session_orchestration_grpc_request_duration_seconds",
        "method" => method,
    )
    .record(started.elapsed().as_secs_f64());
}

// ---------------------------------------------------------------------------
// Enum mapping helpers (snake_case TEXT <-> proto i32)
// ---------------------------------------------------------------------------

pub(crate) fn plan_state_from_str(s: &str) -> i32 {
    match s {
        "draft" => proto::PlanState::Draft as i32,
        "proposed" => proto::PlanState::Proposed as i32,
        "approved" => proto::PlanState::Approved as i32,
        "rejected" => proto::PlanState::Rejected as i32,
        "executing" => proto::PlanState::Executing as i32,
        "completed" => proto::PlanState::Completed as i32,
        "failed" => proto::PlanState::Failed as i32,
        "superseded" => proto::PlanState::Superseded as i32,
        "archived" => proto::PlanState::Archived as i32,
        _ => proto::PlanState::Unspecified as i32,
    }
}

pub(crate) fn plan_state_to_str(code: i32) -> Option<&'static str> {
    match proto::PlanState::try_from(code).ok()? {
        proto::PlanState::Unspecified => None,
        proto::PlanState::Draft => Some("draft"),
        proto::PlanState::Proposed => Some("proposed"),
        proto::PlanState::Approved => Some("approved"),
        proto::PlanState::Rejected => Some("rejected"),
        proto::PlanState::Executing => Some("executing"),
        proto::PlanState::Completed => Some("completed"),
        proto::PlanState::Failed => Some("failed"),
        proto::PlanState::Superseded => Some("superseded"),
        proto::PlanState::Archived => Some("archived"),
    }
}

pub(crate) fn plan_step_state_from_str(s: &str) -> i32 {
    match s {
        "pending" => proto::PlanStepState::Pending as i32,
        "running" => proto::PlanStepState::Running as i32,
        "done" | "completed" => proto::PlanStepState::Done as i32,
        "skipped" => proto::PlanStepState::Skipped as i32,
        "failed" => proto::PlanStepState::Failed as i32,
        _ => proto::PlanStepState::Unspecified as i32,
    }
}

pub(crate) fn approval_kind_from_str(s: &str) -> i32 {
    match s {
        "plan" => proto::ApprovalKind::Plan as i32,
        "tool_call" => proto::ApprovalKind::ToolCall as i32,
        "permission" => proto::ApprovalKind::Permission as i32,
        "destructive" => proto::ApprovalKind::Destructive as i32,
        "cost" => proto::ApprovalKind::Cost as i32,
        _ => proto::ApprovalKind::Unspecified as i32,
    }
}

pub(crate) fn approval_kind_to_str(code: i32) -> Option<&'static str> {
    match proto::ApprovalKind::try_from(code).ok()? {
        proto::ApprovalKind::Unspecified => None,
        proto::ApprovalKind::Plan => Some("plan"),
        proto::ApprovalKind::ToolCall => Some("tool_call"),
        proto::ApprovalKind::Permission => Some("permission"),
        proto::ApprovalKind::Destructive => Some("destructive"),
        proto::ApprovalKind::Cost => Some("cost"),
    }
}

pub(crate) fn approval_state_from_str(s: &str) -> i32 {
    match s {
        "requested" => proto::ApprovalState::Requested as i32,
        "granted" => proto::ApprovalState::Granted as i32,
        "denied" => proto::ApprovalState::Denied as i32,
        "timed_out" | "expired" => proto::ApprovalState::TimedOut as i32,
        _ => proto::ApprovalState::Unspecified as i32,
    }
}

pub(crate) fn approval_state_to_str(code: i32) -> Option<&'static str> {
    match proto::ApprovalState::try_from(code).ok()? {
        proto::ApprovalState::Unspecified => None,
        proto::ApprovalState::Requested => Some("requested"),
        proto::ApprovalState::Granted => Some("granted"),
        proto::ApprovalState::Denied => Some("denied"),
        proto::ApprovalState::TimedOut => Some("timed_out"),
    }
}

#[allow(clippy::result_large_err)]
fn validate_approval_org(org_id: &str) -> Result<(), Status> {
    if org_id.trim().is_empty() {
        return Err(Status::invalid_argument("org_id is required"));
    }
    Ok(())
}

#[allow(clippy::result_large_err)]
fn validate_approval_decision_context(org_id: &str, decided_by: &str) -> Result<(), Status> {
    validate_approval_org(org_id)?;
    if decided_by.trim().is_empty() {
        return Err(Status::invalid_argument("decided_by is required"));
    }
    Ok(())
}

const MAX_CONTINUATION_DESCRIPTOR_BYTES: usize = 16 * 1024;

/// Validate a retained continuation as an exact, approval-scoped action
/// description. It cannot carry reusable credentials; execution remains
/// unavailable until a service-only dispatcher obtains separate authority.
#[allow(clippy::result_large_err)]
fn validate_continuation_descriptor(
    raw: &str,
    req: &proto::CreateApprovalRequest,
) -> Result<Option<JsonValue>, Status> {
    if raw.is_empty() {
        return Ok(None);
    }
    if raw.len() > MAX_CONTINUATION_DESCRIPTOR_BYTES {
        return Err(Status::invalid_argument(
            "continuation descriptor is too large",
        ));
    }
    let value: JsonValue = serde_json::from_str(raw)
        .map_err(|_| Status::invalid_argument("continuation descriptor must be JSON"))?;
    let object = value
        .as_object()
        .ok_or_else(|| Status::invalid_argument("continuation descriptor must be an object"))?;
    if object.get("version").and_then(JsonValue::as_u64) != Some(1) {
        return Err(Status::invalid_argument(
            "unsupported continuation descriptor version",
        ));
    }
    for (key, expected) in [
        ("run_id", req.run_id.as_str()),
        ("org_id", req.org_id.as_str()),
        ("user_id", req.user_id.as_str()),
    ] {
        if object.get(key).and_then(JsonValue::as_str) != Some(expected) {
            return Err(Status::permission_denied(
                "continuation descriptor scope mismatch",
            ));
        }
    }
    if object.get("action_kind").and_then(JsonValue::as_str) != Some("tool_call") {
        return Err(Status::invalid_argument(
            "continuation descriptor action kind is invalid",
        ));
    }
    let fingerprint = object
        .get("action_fingerprint")
        .and_then(JsonValue::as_str)
        .unwrap_or_default();
    if fingerprint.len() != 64 || !fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Status::invalid_argument(
            "continuation descriptor fingerprint is invalid",
        ));
    }
    if descriptor_contains_secret_key(&value) {
        return Err(Status::invalid_argument(
            "continuation descriptor must not contain credentials",
        ));
    }
    Ok(Some(value))
}

fn descriptor_contains_secret_key(value: &JsonValue) -> bool {
    const FORBIDDEN: &[&str] = &[
        "authorization",
        "access_token",
        "refresh_token",
        "client_secret",
        "password",
    ];
    match value {
        JsonValue::Object(object) => object.iter().any(|(key, child)| {
            FORBIDDEN.contains(&key.to_ascii_lowercase().as_str())
                || descriptor_contains_secret_key(child)
        }),
        JsonValue::Array(values) => values.iter().any(descriptor_contains_secret_key),
        _ => false,
    }
}

/// Approval delivery is an internal service-to-service capability, never a
/// user-facing approval read/write shortcut. The worker identity is derived
/// from the verified JWT and the tenant is pinned before any outbox query.
#[allow(clippy::result_large_err)]
fn authorize_approval_delivery_worker(
    caller: &VerifiedIdentity,
    org_id: &str,
) -> Result<String, Status> {
    validate_approval_org(org_id)?;
    authorize_operation(caller, "approval:deliver")?;
    if !caller.is_service() {
        return Err(Status::permission_denied(
            "service identity required for approval delivery",
        ));
    }
    caller.authorize_org(org_id)?;
    Ok(caller.principal_id().to_owned())
}

pub(crate) fn todo_state_from_str(s: &str) -> i32 {
    match s {
        "pending" => proto::TodoState::Pending as i32,
        "in_progress" => proto::TodoState::InProgress as i32,
        "blocked" => proto::TodoState::Blocked as i32,
        "completed" => proto::TodoState::Completed as i32,
        "cancelled" | "canceled" => proto::TodoState::Cancelled as i32,
        _ => proto::TodoState::Unspecified as i32,
    }
}

pub(crate) fn todo_state_to_str(code: i32) -> Option<&'static str> {
    match proto::TodoState::try_from(code).ok()? {
        proto::TodoState::Unspecified => None,
        proto::TodoState::Pending => Some("pending"),
        proto::TodoState::InProgress => Some("in_progress"),
        proto::TodoState::Blocked => Some("blocked"),
        proto::TodoState::Completed => Some("completed"),
        proto::TodoState::Cancelled => Some("cancelled"),
    }
}

pub(crate) fn todo_priority_from_str(s: &str) -> i32 {
    match s {
        "low" => proto::TodoPriority::Low as i32,
        "normal" => proto::TodoPriority::Normal as i32,
        "high" => proto::TodoPriority::High as i32,
        "urgent" => proto::TodoPriority::Urgent as i32,
        _ => proto::TodoPriority::Unspecified as i32,
    }
}

pub(crate) fn subagent_role_from_str(s: &str) -> i32 {
    match s {
        "coder" => proto::SubagentRole::Coder as i32,
        "reviewer" => proto::SubagentRole::Reviewer as i32,
        "researcher" => proto::SubagentRole::Researcher as i32,
        "explorer" => proto::SubagentRole::Explorer as i32,
        "generic" => proto::SubagentRole::Generic as i32,
        _ => proto::SubagentRole::Unspecified as i32,
    }
}

pub(crate) fn subagent_role_to_str(code: i32) -> &'static str {
    match proto::SubagentRole::try_from(code).unwrap_or(proto::SubagentRole::Unspecified) {
        proto::SubagentRole::Unspecified | proto::SubagentRole::Generic => "generic",
        proto::SubagentRole::Coder => "coder",
        proto::SubagentRole::Reviewer => "reviewer",
        proto::SubagentRole::Researcher => "researcher",
        proto::SubagentRole::Explorer => "explorer",
    }
}

// ---------------------------------------------------------------------------
// Timestamp + JSON helpers
// ---------------------------------------------------------------------------

fn ts(dt: DateTime<Utc>) -> prost_types::Timestamp {
    prost_types::Timestamp {
        seconds: dt.timestamp(),
        nanos: i32::try_from(dt.timestamp_subsec_nanos()).unwrap_or(i32::MAX),
    }
}

fn now_ts() -> prost_types::Timestamp {
    ts(Utc::now())
}

/// Convert `serde_json::Value` to `prost_types::Struct` for proto `Struct`
/// fields. Non-object values are wrapped under a single `value` key.
pub(crate) fn json_to_struct(v: &JsonValue) -> Option<prost_types::Struct> {
    match v {
        JsonValue::Object(map) => {
            let fields = map
                .iter()
                .map(|(k, val)| (k.clone(), json_to_value(val)))
                .collect();
            Some(prost_types::Struct { fields })
        }
        JsonValue::Null => None,
        other => {
            let mut fields = std::collections::BTreeMap::new();
            fields.insert("value".to_owned(), json_to_value(other));
            Some(prost_types::Struct { fields })
        }
    }
}

fn json_to_value(v: &JsonValue) -> prost_types::Value {
    use prost_types::value::Kind;
    let kind = match v {
        JsonValue::Null => Kind::NullValue(0),
        JsonValue::Bool(b) => Kind::BoolValue(*b),
        JsonValue::Number(n) => {
            if let Some(f) = n.as_f64() {
                Kind::NumberValue(f)
            } else {
                Kind::StringValue(n.to_string())
            }
        }
        JsonValue::String(s) => Kind::StringValue(s.clone()),
        JsonValue::Array(arr) => Kind::ListValue(prost_types::ListValue {
            values: arr.iter().map(json_to_value).collect(),
        }),
        JsonValue::Object(_) => Kind::StructValue(json_to_struct(v).unwrap_or_default()),
    };
    prost_types::Value { kind: Some(kind) }
}

fn metadata_str_field(meta: &JsonValue, key: &str) -> String {
    meta.get(key)
        .and_then(JsonValue::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn metadata_string_array(meta: &JsonValue, key: &str) -> Vec<String> {
    meta.get(key)
        .and_then(JsonValue::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Row -> proto mappers
// ---------------------------------------------------------------------------

fn plan_step_from_row(row: &store::PlanStepRow) -> proto::PlanStep {
    let title = metadata_str_field(&row.payload, "title");
    proto::PlanStep {
        id: row.id.clone(),
        title,
        operation: row.kind.clone(),
        state: plan_step_state_from_str(&row.status),
        created_at: Some(ts(row.created_at)),
        updated_at: Some(ts(row.updated_at)),
    }
}

fn plan_from_row(row: &store::PlanRow, steps: Vec<proto::PlanStep>) -> proto::Plan {
    let supersedes = metadata_str_field(&row.metadata, "supersedes");
    proto::Plan {
        id: row.id.clone(),
        run_id: row.run_id.clone().unwrap_or_default(),
        thread_id: row.thread_id.clone(),
        author: row.user_id.clone(),
        state: plan_state_from_str(&row.status),
        summary: row.goal.clone(),
        steps,
        supersedes,
        metadata: json_to_struct(&row.metadata),
        created_at: Some(ts(row.created_at)),
        updated_at: Some(ts(row.updated_at)),
    }
}

fn approval_from_row(row: &store::ApprovalRow) -> proto::Approval {
    proto::Approval {
        id: row.id.clone(),
        run_id: row.run_id.clone(),
        // `step_id` in the proto maps to the approvals.plan_id column.
        step_id: row.plan_id.clone().unwrap_or_default(),
        kind: approval_kind_from_str(&row.kind),
        state: approval_state_from_str(&row.status),
        requested_of: row.requested_by.clone(),
        decided_by: row.decided_by.clone(),
        decision_reason: row.decision_reason.clone(),
        context: json_to_struct(&row.metadata),
        requested_at: Some(ts(row.requested_at)),
        decided_at: row.decided_at.map(ts),
        expires_at: row.expires_at.map(ts),
        org_id: row.org_id.clone(),
    }
}

/// The evidence dimensions the vision's proof model names that this plane
/// cannot assert. Stated explicitly in every bundle rather than silently
/// omitted — a reader must be able to tell "not proven" from "nothing
/// happened". Assembling these needs each owning plane's own contract; there
/// is no shared database to join across.
fn unavailable_proof_sections() -> Vec<proto::UnavailableSection> {
    vec![
        proto::UnavailableSection {
            section: "known".to_string(),
            reason: "Retrieval sources and citations are owned by Data Plane and are not \
                     joinable from Model Plane; this bundle makes no claim about what the \
                     run knew."
                .to_string(),
        },
        proto::UnavailableSection {
            section: "charged".to_string(),
            reason: "Cost and token accounting are owned by cost-core and are not included \
                     in this bundle version."
                .to_string(),
        },
        proto::UnavailableSection {
            section: "retained".to_string(),
            reason: "Retention/ZDR posture is owned by Control Plane organization policy \
                     and is not restated here; reading it from this plane could report a \
                     posture that has since changed."
                .to_string(),
        },
    ]
}

/// Build a run's proof bundle from already-fetched, already-tenant-scoped
/// rows. Pure so the evidence-shaping rules — especially the ones about never
/// inventing a stage that did not happen — are unit-testable without Postgres.
fn build_run_proof_bundle(
    run_id: &str,
    org_id: &str,
    provenance: &store::RunProvenanceRow,
    approvals: &[store::ApprovalRow],
    evidence: &[store::ContinuationEvidenceRow],
) -> proto::RunProofBundle {
    let approvals = approvals
        .iter()
        .map(|approval| {
            // At most one continuation per approval: approval_delivery_outbox
            // holds a UNIQUE constraint on approval_id, and receipts are 1:1
            // with a delivery.
            let execution = evidence
                .iter()
                .find(|e| e.approval_id == approval.id)
                .map(continuation_execution_from_row);
            proto::ApprovalProof {
                approval_id: approval.id.clone(),
                kind: approval.kind.clone(),
                status: approval.status.clone(),
                requested_by: approval.requested_by.clone(),
                decided_by: approval.decided_by.clone(),
                decision_reason: approval.decision_reason.clone(),
                requested_at: Some(ts(approval.requested_at)),
                decided_at: approval.decided_at.map(ts),
                execution,
            }
        })
        .collect();

    proto::RunProofBundle {
        bundle_version: 1,
        run_id: run_id.to_string(),
        org_id: org_id.to_string(),
        generated_at: Some(now_ts()),
        run: Some(proto::RunProvenance {
            goal: provenance.goal.clone(),
            agent_id: provenance.agent_id.clone(),
            status: provenance.status.clone(),
            created_at: Some(ts(provenance.created_at)),
        }),
        approvals,
        unavailable: unavailable_proof_sections(),
    }
}

fn continuation_execution_from_row(
    row: &store::ContinuationEvidenceRow,
) -> proto::ContinuationExecution {
    // A receipt without an outcome row is a continuation that really started
    // and has not finalized. It must stay `None` rather than become a
    // fabricated terminal state.
    let outcome = row
        .outcome
        .as_ref()
        .map(|outcome| proto::ContinuationOutcome {
            outcome: outcome.clone(),
            provider_receipt_id: row.provider_receipt_id.clone().unwrap_or_default(),
            failure_code: row.failure_code.clone().unwrap_or_default(),
            finalized_at: row.finalized_at.map(ts),
            // The verification triple is stored all-or-nothing (migration 0020's
            // together-constraint); a worker that recorded none leaves this
            // absent instead of defaulting to a status.
            verification: row.verification_status.as_ref().map(|status| {
                proto::VerificationResult {
                    effect_id: row.receipt_id.clone(),
                    status: verification_status_to_proto(status),
                    method: row.verification_method.clone().unwrap_or_default(),
                    reason: row.verification_reason.clone().unwrap_or_default(),
                    verified_at: row.finalized_at.map(ts),
                }
            }),
        });

    proto::ContinuationExecution {
        receipt_id: row.receipt_id.clone(),
        delivery_id: row.delivery_id.clone(),
        action_fingerprint: row.action_fingerprint.clone(),
        execution_service_id: row.execution_service_id.clone(),
        descriptor_version: i32::from(row.descriptor_version),
        started_at: Some(ts(row.started_at)),
        outcome,
    }
}

/// Map the stored verification status string to the shared contract enum. An
/// unrecognized value becomes `UNKNOWN`, never a success variant — the
/// migration's CHECK constrains the column, so this is a defensive floor for
/// a future value this build does not know about.
fn verification_status_to_proto(status: &str) -> i32 {
    match status {
        "verified_success" => proto::VerificationStatus::VerifiedSuccess as i32,
        "verified_failure" => proto::VerificationStatus::VerifiedFailure as i32,
        "partially_verified" => proto::VerificationStatus::PartiallyVerified as i32,
        _ => proto::VerificationStatus::Unknown as i32,
    }
}

fn todo_from_row(row: &store::TodoRow) -> proto::Todo {
    let assignee = metadata_str_field(&row.metadata, "assignee");
    let description = metadata_str_field(&row.metadata, "description");
    let blocked_by = metadata_string_array(&row.metadata, "blocked_by");
    let run_id = metadata_str_field(&row.metadata, "run_id");
    let completed_at = if row.status == "completed" {
        Some(ts(row.updated_at))
    } else {
        None
    };
    proto::Todo {
        id: row.id.clone(),
        thread_id: row.thread_id.clone().unwrap_or_default(),
        run_id,
        assignee,
        title: row.content.clone(),
        description,
        state: todo_state_from_str(&row.status),
        priority: todo_priority_from_str(&row.priority),
        blocked_by,
        metadata: json_to_struct(&row.metadata),
        created_at: Some(ts(row.created_at)),
        updated_at: Some(ts(row.updated_at)),
        completed_at,
    }
}

#[allow(dead_code)] // used by the lineage RPC once list_lineage_by_thread is wired
fn lineage_edge_from_row(row: &store::SubagentEdgeRow) -> proto::LineageEdge {
    proto::LineageEdge {
        parent_run_id: row.parent_run_id.clone(),
        child_run_id: row.child_run_id.clone(),
        role: subagent_role_from_str(&row.role),
        spawned_at: Some(ts(row.attached_at)),
    }
}

fn compute_max_depth(edges: &[proto::LineageEdge]) -> u32 {
    use std::collections::{HashMap, HashSet};
    fn dfs<'a>(
        node: &'a str,
        children: &HashMap<&'a str, Vec<&'a str>>,
        seen: &mut HashSet<&'a str>,
    ) -> u32 {
        if !seen.insert(node) {
            return 0;
        }
        let mut deepest = 0u32;
        if let Some(kids) = children.get(node) {
            for k in kids {
                let d = dfs(k, children, seen);
                if d > deepest {
                    deepest = d;
                }
            }
        }
        deepest + 1
    }
    if edges.is_empty() {
        return 0;
    }
    let mut children: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut nodes: HashSet<&str> = HashSet::new();
    let mut has_parent: HashSet<&str> = HashSet::new();
    for e in edges {
        children
            .entry(e.parent_run_id.as_str())
            .or_default()
            .push(e.child_run_id.as_str());
        nodes.insert(&e.parent_run_id);
        nodes.insert(&e.child_run_id);
        has_parent.insert(&e.child_run_id);
    }
    let roots: Vec<&str> = nodes
        .iter()
        .copied()
        .filter(|n| !has_parent.contains(n))
        .collect();

    let mut max_depth = 0u32;
    for root in roots {
        let mut seen = HashSet::new();
        let d = dfs(root, &children, &mut seen);
        if d > max_depth {
            max_depth = d;
        }
    }
    // dfs counts nodes including root; depth == edges == nodes-1.
    max_depth.saturating_sub(1)
}

// ---------------------------------------------------------------------------
// Lineage helpers
// ---------------------------------------------------------------------------

async fn fetch_lineage_for_thread(
    pool: &Pool,
    thread_id: &str,
) -> Result<proto::SubagentLineage, sqlx::Error> {
    let rows = sqlx::query(
        "WITH RECURSIVE thread_runs AS ( \
             SELECT id FROM runs WHERE thread_id = $1 \
         ), \
         lineage AS ( \
             SELECT e.parent_run_id, e.child_run_id, e.role, e.status, \
                    e.metadata, e.attached_at, e.detached_at \
             FROM subagent_edges e \
             JOIN thread_runs tr ON tr.id = e.parent_run_id \
             UNION \
             SELECT e.parent_run_id, e.child_run_id, e.role, e.status, \
                    e.metadata, e.attached_at, e.detached_at \
             FROM subagent_edges e \
             JOIN lineage l ON l.child_run_id = e.parent_run_id \
         ) \
         SELECT parent_run_id, child_run_id, role, attached_at \
         FROM lineage \
         ORDER BY attached_at ASC",
    )
    .bind(thread_id)
    .fetch_all(pool)
    .await?;

    let edges: Vec<proto::LineageEdge> = rows
        .iter()
        .map(|r| proto::LineageEdge {
            parent_run_id: r.get::<String, _>("parent_run_id"),
            child_run_id: r.get::<String, _>("child_run_id"),
            role: subagent_role_from_str(r.get::<&str, _>("role")),
            spawned_at: Some(ts(r.get::<DateTime<Utc>, _>("attached_at"))),
        })
        .collect();

    let max_depth = compute_max_depth(&edges);
    Ok(proto::SubagentLineage {
        thread_id: thread_id.to_owned(),
        max_depth,
        edges,
    })
}

async fn assemble_plan(pool: &Pool, plan: &store::PlanRow) -> Result<proto::Plan, sqlx::Error> {
    let step_rows = store::list_steps_by_plan_full(pool, &plan.id)
        .await
        .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    let steps = step_rows.iter().map(plan_step_from_row).collect();
    Ok(plan_from_row(plan, steps))
}

// ---------------------------------------------------------------------------
// Event broadcast helpers
// ---------------------------------------------------------------------------

/// Broadcast an event to streaming clients and buffer it for replay. Returns
/// the server-assigned monotonic `event_id` so callers that need to echo it
/// back (e.g. `RecordOrchestrationEvent`) can do so.
fn broadcast_event(
    tx: &broadcast::Sender<proto::OrchestrationEvent>,
    replay: &ReplayBuffer,
    mut ev: proto::OrchestrationEvent,
) -> String {
    // Assign a monotonic id used as the SSE `id:` line and the resume cursor.
    if ev.event_id.is_empty() {
        ev.event_id = mp_ids::new_ulid();
    }
    let event_id = ev.event_id.clone();
    // Retain in the per-run replay buffer so a reconnecting client can resume
    // from Last-Event-Id. Only run-scoped events are buffered (the stream is
    // keyed by run_id; thread-only events are never delivered per run anyway).
    if let Some(run_id) = event_run_id(&ev).map(str::to_owned) {
        replay.push_event(&run_id, &ev);
    }
    // `send` only fails when there are zero subscribers; that's expected when
    // no streaming clients are attached. Don't propagate as an error.
    let _ = tx.send(ev);
    event_id
}

/// Emit approval decision events only for the request that won the durable
/// requested-state compare-and-set. Exact or concurrent replays observe the
/// committed row but must not mint fresh event ids. A grant is queued in the
/// durable delivery outbox; only a future execution delivery acknowledgement
/// may emit `RunResumedAfterApproval`.
fn broadcast_approval_decision_events(
    tx: &broadcast::Sender<proto::OrchestrationEvent>,
    replay: &ReplayBuffer,
    approval: &store::ApprovalRow,
    decision: i32,
    transition_applied: bool,
) {
    if !transition_applied {
        return;
    }

    broadcast_event(
        tx,
        replay,
        proto::OrchestrationEvent {
            event_id: String::new(),
            at: Some(now_ts()),
            event: Some(orchestration_event::Event::ApprovalStateChanged(
                orchestration_event::ApprovalStateChanged {
                    approval_id: approval.id.clone(),
                    run_id: approval.run_id.clone(),
                    approval_kind: approval_kind_from_str(&approval.kind),
                    to: decision,
                    decided_by: approval.decided_by.clone(),
                },
            )),
        },
    );
}

fn event_run_id(ev: &proto::OrchestrationEvent) -> Option<&str> {
    match ev.event.as_ref()? {
        orchestration_event::Event::PlanTransitioned(p) => Some(&p.run_id),
        orchestration_event::Event::TodoTransitioned(_)
        | orchestration_event::Event::SubagentStopped(_) => None,
        orchestration_event::Event::ApprovalStateChanged(p) => Some(&p.run_id),
        orchestration_event::Event::SubagentAttached(p) => Some(&p.parent_run_id),
        orchestration_event::Event::RunPausedForApproval(p) => Some(&p.run_id),
        orchestration_event::Event::RunResumedAfterApproval(p) => Some(&p.run_id),
        orchestration_event::Event::BrowserActionDispatched(p) => Some(&p.run_id),
        orchestration_event::Event::BrowserObservationReceived(p) => Some(&p.run_id),
        orchestration_event::Event::BrowserRunPaused(p) => Some(&p.run_id),
        orchestration_event::Event::BrowserRunResumed(p) => Some(&p.run_id),
        // Phase 5 — browser-specific HITL approval-gate detail events. Reuse
        // the same `RecordOrchestrationEvent` best-effort delivery path as
        // `BrowserActionDispatched`/`BrowserObservationReceived` above; the
        // durable approval itself is still created via the general
        // `CreateApproval`/`DecideApproval` RPCs (unchanged), these are just
        // the browser-specific companions carrying which action/why.
        orchestration_event::Event::BrowserActionApprovalRequired(p) => Some(&p.run_id),
        orchestration_event::Event::BrowserActionDecided(p) => Some(&p.run_id),
        orchestration_event::Event::ApprovalContinuationVerified(p) => Some(&p.run_id),
    }
}

// ---------------------------------------------------------------------------
// Service implementation
// ---------------------------------------------------------------------------

#[tonic::async_trait]
#[allow(clippy::too_many_lines)]
impl OrchestrationCoreService for OrchestrationGrpc {
    type StreamRunEventsStream =
        Pin<Box<dyn Stream<Item = Result<proto::OrchestrationEvent, Status>> + Send + 'static>>;

    async fn list_plans(
        &self,
        request: Request<proto::ListPlansRequest>,
    ) -> Result<Response<proto::ListPlansResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<proto::ListPlansResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "orchestration:read")?;
            let req = request.into_inner();
            if req.run_id.is_empty() {
                return Err(Status::invalid_argument("run_id is required"));
            }
            authorize_run_owner(&self.pool, &caller, &req.run_id, OwnerIntent::Read).await?;
            let plan_rows = store::list_plans_by_run(&self.pool, &req.run_id)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
            let mut plans = Vec::with_capacity(plan_rows.len());
            for row in &plan_rows {
                let plan = assemble_plan(&self.pool, row)
                    .await
                    .map_err(|e| Status::internal(e.to_string()))?;
                plans.push(plan);
            }
            Ok(Response::new(proto::ListPlansResponse { plans }))
        }
        .await;
        record_metrics("list_plans", started, result.is_ok());
        result
    }

    async fn get_plan(
        &self,
        request: Request<proto::GetPlanRequest>,
    ) -> Result<Response<proto::GetPlanResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<proto::GetPlanResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "orchestration:read")?;
            let req = request.into_inner();
            if req.plan_id.is_empty() {
                return Err(Status::invalid_argument("plan_id is required"));
            }
            authorize_plan_owner(&self.pool, &caller, &req.plan_id, OwnerIntent::Read).await?;
            let row = store::get_plan(&self.pool, &req.plan_id)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
            let plan = match row {
                None => None,
                Some(r) => Some(
                    assemble_plan(&self.pool, &r)
                        .await
                        .map_err(|e| Status::internal(e.to_string()))?,
                ),
            };
            Ok(Response::new(proto::GetPlanResponse { plan }))
        }
        .await;
        record_metrics("get_plan", started, result.is_ok());
        result
    }

    async fn transition_plan(
        &self,
        request: Request<proto::TransitionPlanRequest>,
    ) -> Result<Response<proto::TransitionPlanResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<proto::TransitionPlanResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "orchestration:write")?;
            let mut req = request.into_inner();
            if req.plan_id.is_empty() {
                return Err(Status::invalid_argument("plan_id is required"));
            }
            authorize_plan_owner(&self.pool, &caller, &req.plan_id, OwnerIntent::Mutate).await?;
            req.actor = caller.principal_id().to_owned();
            let new_status = plan_state_to_str(req.target_state)
                .ok_or_else(|| Status::invalid_argument("invalid target_state"))?;

            let before = store::get_plan(&self.pool, &req.plan_id)
                .await
                .map_err(|e| Status::internal(e.to_string()))?
                .ok_or_else(|| Status::not_found("plan not found"))?;
            let prev_state = plan_state_from_str(&before.status);

            store::update_plan_status(&self.pool, &req.plan_id, new_status)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;

            let after = store::get_plan(&self.pool, &req.plan_id)
                .await
                .map_err(|e| Status::internal(e.to_string()))?
                .ok_or_else(|| Status::internal("plan vanished after update"))?;
            let plan = assemble_plan(&self.pool, &after)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;

            broadcast_event(
                &self.events_tx,
                &self.replay,
                proto::OrchestrationEvent {
                    event_id: String::new(),
                    at: Some(now_ts()),
                    event: Some(orchestration_event::Event::PlanTransitioned(
                        orchestration_event::PlanTransitioned {
                            plan_id: after.id.clone(),
                            run_id: after.run_id.clone().unwrap_or_default(),
                            from: prev_state,
                            to: req.target_state,
                        },
                    )),
                },
            );

            Ok(Response::new(proto::TransitionPlanResponse {
                plan: Some(plan),
            }))
        }
        .await;
        record_metrics("transition_plan", started, result.is_ok());
        result
    }

    async fn list_todos(
        &self,
        request: Request<proto::ListTodosRequest>,
    ) -> Result<Response<proto::ListTodosResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<proto::ListTodosResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "orchestration:read")?;
            let req = request.into_inner();
            if req.thread_id.is_empty() {
                return Err(Status::invalid_argument("thread_id is required"));
            }
            authorize_thread_owner(&self.pool, &caller, &req.thread_id, OwnerIntent::Read).await?;
            let rows = store::list_todos_by_thread(&self.pool, &req.thread_id)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
            let todos: Vec<proto::Todo> = rows
                .iter()
                .filter(|r| {
                    if req.run_id.is_empty() {
                        true
                    } else {
                        metadata_str_field(&r.metadata, "run_id") == req.run_id
                    }
                })
                .map(todo_from_row)
                .collect();
            Ok(Response::new(proto::ListTodosResponse { todos }))
        }
        .await;
        record_metrics("list_todos", started, result.is_ok());
        result
    }

    async fn get_todo(
        &self,
        request: Request<proto::GetTodoRequest>,
    ) -> Result<Response<proto::GetTodoResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<proto::GetTodoResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "orchestration:read")?;
            let req = request.into_inner();
            if req.todo_id.is_empty() {
                return Err(Status::invalid_argument("todo_id is required"));
            }
            authorize_todo_owner(&self.pool, &caller, &req.todo_id, OwnerIntent::Read).await?;
            let row = store::get_todo(&self.pool, &req.todo_id)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
            Ok(Response::new(proto::GetTodoResponse {
                todo: row.as_ref().map(todo_from_row),
            }))
        }
        .await;
        record_metrics("get_todo", started, result.is_ok());
        result
    }

    async fn transition_todo(
        &self,
        request: Request<proto::TransitionTodoRequest>,
    ) -> Result<Response<proto::TransitionTodoResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<proto::TransitionTodoResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "orchestration:write")?;
            let mut req = request.into_inner();
            if req.todo_id.is_empty() {
                return Err(Status::invalid_argument("todo_id is required"));
            }
            authorize_todo_owner(&self.pool, &caller, &req.todo_id, OwnerIntent::Mutate).await?;
            req.actor = caller.principal_id().to_owned();
            let new_status = todo_state_to_str(req.target_state)
                .ok_or_else(|| Status::invalid_argument("invalid target_state"))?;

            let before = store::get_todo(&self.pool, &req.todo_id)
                .await
                .map_err(|e| Status::internal(e.to_string()))?
                .ok_or_else(|| Status::not_found("todo not found"))?;
            let prev_state = todo_state_from_str(&before.status);

            store::update_todo_status(&self.pool, &req.todo_id, new_status)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;

            let after = store::get_todo(&self.pool, &req.todo_id)
                .await
                .map_err(|e| Status::internal(e.to_string()))?
                .ok_or_else(|| Status::internal("todo vanished after update"))?;
            let todo = todo_from_row(&after);

            broadcast_event(
                &self.events_tx,
                &self.replay,
                proto::OrchestrationEvent {
                    event_id: String::new(),
                    at: Some(now_ts()),
                    event: Some(orchestration_event::Event::TodoTransitioned(
                        orchestration_event::TodoTransitioned {
                            todo_id: after.id.clone(),
                            thread_id: after.thread_id.clone().unwrap_or_default(),
                            from: prev_state,
                            to: req.target_state,
                        },
                    )),
                },
            );

            Ok(Response::new(proto::TransitionTodoResponse {
                todo: Some(todo),
            }))
        }
        .await;
        record_metrics("transition_todo", started, result.is_ok());
        result
    }

    async fn list_approvals(
        &self,
        request: Request<proto::ListApprovalsRequest>,
    ) -> Result<Response<proto::ListApprovalsResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<proto::ListApprovalsResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "orchestration:read")?;
            let req = request.into_inner();
            if req.run_id.is_empty() {
                return Err(Status::invalid_argument("run_id is required"));
            }
            validate_approval_org(&req.org_id)?;
            caller.authorize_org(&req.org_id)?;
            authorize_run_owner(&self.pool, &caller, &req.run_id, OwnerIntent::Read).await?;
            let rows = store::list_approvals_by_run_full_for_org(
                &self.pool,
                &req.run_id,
                &req.org_id,
                caller.user_id(),
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
            let approvals: Vec<proto::Approval> = rows
                .iter()
                .filter(|r| {
                    if req.step_id.is_empty() {
                        true
                    } else {
                        r.plan_id.as_deref() == Some(req.step_id.as_str())
                    }
                })
                .map(approval_from_row)
                .collect();
            Ok(Response::new(proto::ListApprovalsResponse { approvals }))
        }
        .await;
        record_metrics("list_approvals", started, result.is_ok());
        result
    }

    /// Assemble the Verevon Proof Bundle for one run.
    ///
    /// Read-only and tenant-scoped through exactly the same guards as
    /// `list_approvals`: an approval-evidence record is at least as sensitive
    /// as the approval it describes, so it must not be reachable on weaker
    /// terms.
    async fn get_run_proof_bundle(
        &self,
        request: Request<proto::GetRunProofBundleRequest>,
    ) -> Result<Response<proto::GetRunProofBundleResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<proto::GetRunProofBundleResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "orchestration:read")?;
            let req = request.into_inner();
            if req.run_id.is_empty() {
                return Err(Status::invalid_argument("run_id is required"));
            }
            validate_approval_org(&req.org_id)?;
            caller.authorize_org(&req.org_id)?;
            authorize_run_owner(&self.pool, &caller, &req.run_id, OwnerIntent::Read).await?;

            let provenance =
                store::get_run_provenance_for_org(&self.pool, &req.run_id, &req.org_id)
                    .await
                    .map_err(|e| Status::internal(e.to_string()))?
                    .ok_or_else(|| Status::not_found("run not found"))?;

            let approvals = store::list_approvals_by_run_full_for_org(
                &self.pool,
                &req.run_id,
                &req.org_id,
                caller.user_id(),
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

            let evidence =
                store::list_continuation_evidence_for_run(&self.pool, &req.run_id, &req.org_id)
                    .await
                    .map_err(|e| Status::internal(e.to_string()))?;

            let bundle = build_run_proof_bundle(
                &req.run_id,
                &req.org_id,
                &provenance,
                &approvals,
                &evidence,
            );
            Ok(Response::new(proto::GetRunProofBundleResponse {
                bundle: Some(bundle),
            }))
        }
        .await;
        record_metrics("get_run_proof_bundle", started, result.is_ok());
        result
    }

    async fn list_pending_approvals(
        &self,
        request: Request<proto::OrgPendingApprovalsRequest>,
    ) -> Result<Response<proto::OrgPendingApprovalsResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<proto::OrgPendingApprovalsResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "orchestration:read")?;
            let req = request.into_inner();
            validate_approval_org(&req.org_id)?;
            caller.authorize_org(&req.org_id)?;
            let rows = store::list_pending_approvals(&self.pool, &req.org_id, caller.user_id())
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
            let approvals: Vec<proto::Approval> = rows.iter().map(approval_from_row).collect();
            Ok(Response::new(proto::OrgPendingApprovalsResponse {
                approvals,
            }))
        }
        .await;
        record_metrics("list_pending_approvals", started, result.is_ok());
        result
    }

    async fn get_approval(
        &self,
        request: Request<proto::GetApprovalRequest>,
    ) -> Result<Response<proto::GetApprovalResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<proto::GetApprovalResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "orchestration:read")?;
            let req = request.into_inner();
            if req.approval_id.is_empty() {
                return Err(Status::invalid_argument("approval_id is required"));
            }
            validate_approval_org(&req.org_id)?;
            caller.authorize_org(&req.org_id)?;
            let row = store::get_approval_for_org(
                &self.pool,
                &req.approval_id,
                &req.org_id,
                caller.user_id(),
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
            Ok(Response::new(proto::GetApprovalResponse {
                approval: row.as_ref().map(approval_from_row),
            }))
        }
        .await;
        record_metrics("get_approval", started, result.is_ok());
        result
    }

    async fn create_approval(
        &self,
        request: Request<proto::CreateApprovalRequest>,
    ) -> Result<Response<proto::CreateApprovalResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<proto::CreateApprovalResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "orchestration:write")?;
            let mut req = request.into_inner();
            if req.run_id.is_empty() {
                return Err(Status::invalid_argument("run_id is required"));
            }
            pin_create_approval_identity(&caller, &mut req)?;
            authorize_run_owner(&self.pool, &caller, &req.run_id, OwnerIntent::Mutate).await?;
            let kind = approval_kind_to_str(req.kind)
                .ok_or_else(|| Status::invalid_argument("invalid approval kind"))?;
            let continuation_descriptor =
                validate_continuation_descriptor(&req.continuation_descriptor_json, &req)?;

            // Honor a caller-supplied id (matrix §4.1) so an upstream cache
            // (the gateway ApprovalStore) stays aligned with the durable
            // record; otherwise mint one. Empty stays the default path.
            let id = if req.client_approval_id.is_empty() {
                format!("appr_{}", mp_ids::new_ulid())
            } else {
                req.client_approval_id.clone()
            };
            // `step_id` names the gated step, NOT a plan. The approvals table
            // has no step_id column and plan_id is an FK → plans(id); binding
            // step_id to plan_id made every tool-step approval violate
            // `approvals_plan_id_fkey` and silently fail to persist (the gate
            // fired but no durable approval was ever recorded). Keep plan_id
            // NULL here and preserve step_id in metadata for traceability.
            let plan_id: Option<&str> = None;
            let expires_at = if req.expires_in_seconds > 0 {
                Some(Utc::now() + chrono::Duration::seconds(i64::from(req.expires_in_seconds)))
            } else {
                None
            };
            let mut metadata_map = serde_json::Map::new();
            if !req.reason.is_empty() {
                metadata_map.insert("reason".to_owned(), JsonValue::String(req.reason.clone()));
            }
            if !req.step_id.is_empty() {
                metadata_map.insert("step_id".to_owned(), JsonValue::String(req.step_id.clone()));
            }
            if let Some(descriptor) = continuation_descriptor {
                metadata_map.insert("continuation_descriptor".to_owned(), descriptor);
            }
            let metadata = JsonValue::Object(metadata_map);

            let created = store::request_approval(
                &self.pool,
                &id,
                &req.run_id,
                plan_id,
                kind,
                &req.requested_of,
                &req.org_id,
                &req.user_id,
                // Caller-supplied idempotency key (D-1). When set, a retried
                // request collapses onto the existing durable row via the
                // `ON CONFLICT (org_id, idempotency_key) DO NOTHING` guard
                // instead of creating a duplicate. Empty = no guard (a posture
                // gate may legitimately fire more than once per run/step).
                &req.idempotency_key,
                &metadata,
                expires_at,
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

            // On the `DO NOTHING` no-op path `created` is None and the existing
            // durable row keeps its ORIGINAL id (not the new caller id), so we
            // must resolve it by the idempotency key rather than by `id`.
            let row = match created {
                Some(new_id) => store::get_approval(&self.pool, &new_id)
                    .await
                    .map_err(|e| Status::internal(e.to_string()))?,
                None if !req.idempotency_key.is_empty() => store::get_approval_by_idempotency_key(
                    &self.pool,
                    &req.org_id,
                    &req.user_id,
                    &req.idempotency_key,
                )
                .await
                .map_err(|e| Status::internal(e.to_string()))?,
                None => store::get_approval(&self.pool, &id)
                    .await
                    .map_err(|e| Status::internal(e.to_string()))?,
            }
            .ok_or_else(|| Status::internal("approval vanished after insert"))?;
            let approval = approval_from_row(&row);

            // Operator surfaces (run-event feed, snapshot) consume both: the
            // approval entering REQUESTED and the run pausing for it.
            broadcast_event(
                &self.events_tx,
                &self.replay,
                proto::OrchestrationEvent {
                    event_id: String::new(),
                    at: Some(now_ts()),
                    event: Some(orchestration_event::Event::ApprovalStateChanged(
                        orchestration_event::ApprovalStateChanged {
                            approval_id: row.id.clone(),
                            run_id: row.run_id.clone(),
                            approval_kind: approval_kind_from_str(&row.kind),
                            to: proto::ApprovalState::Requested as i32,
                            decided_by: String::new(),
                        },
                    )),
                },
            );
            broadcast_event(
                &self.events_tx,
                &self.replay,
                proto::OrchestrationEvent {
                    event_id: String::new(),
                    at: Some(now_ts()),
                    event: Some(orchestration_event::Event::RunPausedForApproval(
                        orchestration_event::RunPausedForApproval {
                            run_id: row.run_id.clone(),
                            approval_id: row.id.clone(),
                        },
                    )),
                },
            );

            Ok(Response::new(proto::CreateApprovalResponse {
                approval: Some(approval),
            }))
        }
        .await;
        record_metrics("create_approval", started, result.is_ok());
        result
    }

    async fn decide_approval(
        &self,
        request: Request<proto::DecideApprovalRequest>,
    ) -> Result<Response<proto::DecideApprovalResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<proto::DecideApprovalResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "approval:decide")?;
            let mut req = request.into_inner();
            if req.approval_id.is_empty() {
                return Err(Status::invalid_argument("approval_id is required"));
            }
            pin_approval_decision_identity(&caller, &mut req)?;
            validate_approval_decision_context(&req.org_id, &req.decided_by)?;
            let new_status = approval_state_to_str(req.decision)
                .ok_or_else(|| Status::invalid_argument("invalid decision state"))?;
            if matches!(new_status, "requested") {
                return Err(Status::invalid_argument(
                    "decision must be granted, denied, or timed_out",
                ));
            }

            let decision_write = store::decide_approval(
                &self.pool,
                &req.approval_id,
                &req.org_id,
                caller.user_id(),
                new_status,
                &req.decided_by,
                &req.decision_reason,
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
            let updated = decision_write.updated;

            let after = store::get_approval_for_org(
                &self.pool,
                &req.approval_id,
                &req.org_id,
                caller.user_id(),
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
            if !updated {
                match after.as_ref() {
                    Some(approval)
                        if is_idempotent_approval_retry(
                            approval,
                            new_status,
                            &req.decided_by,
                            &req.decision_reason,
                        ) => {}
                    Some(_) => {
                        return Err(Status::failed_precondition("approval is already decided"));
                    }
                    None => return Err(Status::not_found("approval not found")),
                }
            }
            if new_status == "granted" && decision_write.delivery_id.is_none() {
                return Err(Status::unavailable(
                    "approval is granted but durable delivery state is unavailable",
                ));
            }
            let after = after.ok_or_else(|| Status::internal("approval vanished after update"))?;
            let approval = approval_from_row(&after);

            broadcast_approval_decision_events(
                &self.events_tx,
                &self.replay,
                &after,
                req.decision,
                updated,
            );

            Ok(Response::new(proto::DecideApprovalResponse {
                approval: Some(approval),
            }))
        }
        .await;
        record_metrics("decide_approval", started, result.is_ok());
        result
    }

    async fn claim_approval_deliveries(
        &self,
        request: Request<proto::ClaimApprovalDeliveriesRequest>,
    ) -> Result<Response<proto::ClaimApprovalDeliveriesResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<proto::ClaimApprovalDeliveriesResponse>, Status> = async {
            let caller = identity(&request)?;
            let req = request.into_inner();
            let worker_id = authorize_approval_delivery_worker(&caller, &req.org_id)?;
            let deliveries = crate::approval_delivery::claim_due_deliveries(
                &self.pool,
                &req.org_id,
                &worker_id,
                req.max_deliveries,
            )
            .await
            .map_err(|error| {
                warn!(error = %error, "approval delivery claim unavailable");
                Status::internal("approval delivery claim unavailable")
            })?;
            metrics::counter!("mp_session_approval_delivery_claimed_total")
                .increment(u64::try_from(deliveries.len()).unwrap_or(u64::MAX));
            let deliveries = deliveries
                .into_iter()
                .map(|delivery| proto::ApprovalDelivery {
                    delivery_id: delivery.delivery_id,
                    approval_id: delivery.approval_id,
                    run_id: delivery.run_id,
                    org_id: delivery.org_id,
                    user_id: delivery.user_id,
                    lease_token: delivery.lease_token,
                    attempt: delivery.attempt,
                    lease_expires_at: Some(ts(delivery.lease_expires_at)),
                })
                .collect();
            Ok(Response::new(proto::ClaimApprovalDeliveriesResponse {
                deliveries,
            }))
        }
        .await;
        record_metrics("claim_approval_deliveries", started, result.is_ok());
        result
    }

    async fn get_approval_continuation(
        &self,
        request: Request<proto::GetApprovalContinuationRequest>,
    ) -> Result<Response<proto::GetApprovalContinuationResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<proto::GetApprovalContinuationResponse>, Status> = async {
            let caller = identity(&request)?;
            let req = request.into_inner();
            let worker_id = authorize_approval_delivery_worker(&caller, &req.org_id)?;
            let descriptor = crate::approval_delivery::load_active_continuation_descriptor(
                &self.pool,
                &req.org_id,
                &worker_id,
                &req.delivery_id,
                &req.approval_id,
                &req.lease_token,
            )
            .await
            .map_err(|error| {
                warn!(error = %error, "approval continuation lookup unavailable");
                Status::internal("approval continuation lookup unavailable")
            })?;
            let response = match descriptor {
                Some(descriptor) => proto::GetApprovalContinuationResponse {
                    available: true,
                    continuation_descriptor_json: serde_json::to_string(&descriptor)
                        .map_err(|_| Status::data_loss("stored continuation is invalid"))?,
                },
                None => proto::GetApprovalContinuationResponse {
                    available: false,
                    continuation_descriptor_json: String::new(),
                },
            };
            Ok(Response::new(response))
        }
        .await;
        record_metrics("get_approval_continuation", started, result.is_ok());
        result
    }

    async fn record_approval_continuation_started(
        &self,
        request: Request<proto::RecordApprovalContinuationStartedRequest>,
    ) -> Result<Response<proto::RecordApprovalContinuationStartedResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<proto::RecordApprovalContinuationStartedResponse>, Status> =
            async {
                let caller = identity(&request)?;
                let req = request.into_inner();
                let worker_id = authorize_approval_delivery_worker(&caller, &req.org_id)?;
                let receipt = crate::approval_delivery::record_continuation_started(
                    &self.pool,
                    &req.org_id,
                    &worker_id,
                    &req.delivery_id,
                    &req.approval_id,
                    &req.lease_token,
                )
                .await
                .map_err(|error| {
                    warn!(error = %error, "approval continuation receipt unavailable");
                    Status::internal("approval continuation receipt unavailable")
                })?;
                let response = match receipt {
                    Some(receipt) => proto::RecordApprovalContinuationStartedResponse {
                        receipt_id: receipt.receipt_id,
                        already_started: receipt.already_started,
                    },
                    None => proto::RecordApprovalContinuationStartedResponse {
                        receipt_id: String::new(),
                        already_started: false,
                    },
                };
                Ok(Response::new(response))
            }
            .await;
        record_metrics(
            "record_approval_continuation_started",
            started,
            result.is_ok(),
        );
        result
    }

    async fn record_approval_continuation_outcome(
        &self,
        request: Request<proto::RecordApprovalContinuationOutcomeRequest>,
    ) -> Result<Response<proto::RecordApprovalContinuationOutcomeResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<proto::RecordApprovalContinuationOutcomeResponse>, Status> =
            async {
                let caller = identity(&request)?;
                let req = request.into_inner();
                let worker_id = authorize_approval_delivery_worker(&caller, &req.org_id)?;
                let outcome = match proto::ApprovalContinuationOutcome::try_from(req.outcome) {
                    Ok(proto::ApprovalContinuationOutcome::Completed) => {
                        crate::approval_delivery::ContinuationOutcome::Completed
                    }
                    Ok(proto::ApprovalContinuationOutcome::Failed) => {
                        crate::approval_delivery::ContinuationOutcome::Failed
                    }
                    Ok(proto::ApprovalContinuationOutcome::Cancelled) => {
                        crate::approval_delivery::ContinuationOutcome::Cancelled
                    }
                    Ok(proto::ApprovalContinuationOutcome::Unspecified) | Err(_) => {
                        return Err(Status::invalid_argument(
                            "continuation outcome must be completed, failed, or cancelled",
                        ));
                    }
                };
                // Verified Outcome Foundation (verevon-roadmap.md §3b): optional,
                // absent for a caller that doesn't yet produce one.
                // VERIFICATION_STATUS_UNSPECIFIED is treated the same as an
                // absent field — a caller sending it explicitly has not
                // performed any judgment, so it must not be stored as a claim.
                let verification_status = req.verification.as_ref().and_then(|v| {
                    match proto::VerificationStatus::try_from(v.status) {
                        Ok(proto::VerificationStatus::Unknown) => Some("unknown"),
                        Ok(proto::VerificationStatus::VerifiedSuccess) => Some("verified_success"),
                        Ok(proto::VerificationStatus::VerifiedFailure) => Some("verified_failure"),
                        Ok(proto::VerificationStatus::PartiallyVerified) => {
                            Some("partially_verified")
                        }
                        Ok(proto::VerificationStatus::Unspecified) | Err(_) => None,
                    }
                });
                let verification =
                    verification_status
                        .zip(req.verification.as_ref())
                        .map(|(status, v)| crate::approval_delivery::VerificationFields {
                            status,
                            method: &v.method,
                            reason: &v.reason,
                        });
                let result = crate::approval_delivery::record_continuation_outcome(
                    &self.pool,
                    &req.org_id,
                    &worker_id,
                    &req.delivery_id,
                    &req.approval_id,
                    &req.receipt_id,
                    &req.lease_token,
                    outcome,
                    &req.provider_receipt_id,
                    &req.failure_code,
                    verification,
                )
                .await
                .map_err(|error| {
                    warn!(error = %error, "approval continuation outcome unavailable");
                    Status::internal("approval continuation outcome unavailable")
                })?;
                // Verified Outcome Foundation (verevon-roadmap.md §3b): surface
                // the verification live on the run's own event stream — but
                // only for a FRESH write (never on the already_finalized
                // no-op replay path) and only when the caller actually
                // produced one (an older worker's outcome has none to show).
                if let (Some(result), Some(req_verification)) =
                    (result.as_ref(), req.verification.as_ref())
                {
                    if result.recorded && !result.run_id.is_empty() {
                        broadcast_event(
                            &self.events_tx,
                            &self.replay,
                            proto::OrchestrationEvent {
                                event_id: String::new(),
                                at: Some(now_ts()),
                                event: Some(
                                    orchestration_event::Event::ApprovalContinuationVerified(
                                        orchestration_event::ApprovalContinuationVerified {
                                            run_id: result.run_id.clone(),
                                            delivery_id: req.delivery_id.clone(),
                                            approval_id: req.approval_id.clone(),
                                            receipt_id: req.receipt_id.clone(),
                                            verification: Some(req_verification.clone()),
                                        },
                                    ),
                                ),
                            },
                        );
                    }
                }
                let response = match result {
                    Some(result) => proto::RecordApprovalContinuationOutcomeResponse {
                        recorded: result.recorded,
                        already_finalized: result.already_finalized,
                    },
                    None => proto::RecordApprovalContinuationOutcomeResponse {
                        recorded: false,
                        already_finalized: false,
                    },
                };
                Ok(Response::new(response))
            }
            .await;
        record_metrics(
            "record_approval_continuation_outcome",
            started,
            result.is_ok(),
        );
        result
    }

    async fn acknowledge_approval_delivery(
        &self,
        request: Request<proto::AcknowledgeApprovalDeliveryRequest>,
    ) -> Result<Response<proto::AcknowledgeApprovalDeliveryResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<proto::AcknowledgeApprovalDeliveryResponse>, Status> = async {
            let caller = identity(&request)?;
            let req = request.into_inner();
            let worker_id = authorize_approval_delivery_worker(&caller, &req.org_id)?;
            let acknowledgement =
                match proto::ApprovalDeliveryAcknowledgement::try_from(req.acknowledgement) {
                    Ok(proto::ApprovalDeliveryAcknowledgement::Retry) => {
                        crate::approval_delivery::DeliveryAcknowledgement::Retry
                    }
                    Ok(proto::ApprovalDeliveryAcknowledgement::Terminal) => {
                        crate::approval_delivery::DeliveryAcknowledgement::Terminal
                    }
                    Ok(proto::ApprovalDeliveryAcknowledgement::Settled) => {
                        crate::approval_delivery::DeliveryAcknowledgement::Settled
                    }
                    Ok(proto::ApprovalDeliveryAcknowledgement::Unspecified) | Err(_) => {
                        return Err(Status::invalid_argument(
                            "approval delivery acknowledgement must be retry, terminal, or settled",
                        ));
                    }
                };
            crate::approval_delivery::validate_acknowledgement_input(
                &req.delivery_id,
                &req.lease_token,
            )
            .map_err(|_| Status::invalid_argument("invalid approval delivery lease"))?;
            let acknowledged = crate::approval_delivery::acknowledge_delivery(
                &self.pool,
                &req.org_id,
                &worker_id,
                &req.delivery_id,
                &req.lease_token,
                acknowledgement,
                &req.failure_code,
                &req.continuation_receipt_id,
            )
            .await
            .map_err(|error| {
                warn!(error = %error, "approval delivery acknowledgement unavailable");
                Status::internal("approval delivery acknowledgement unavailable")
            })?;

            let response = match acknowledged {
                Some(acknowledged) => proto::AcknowledgeApprovalDeliveryResponse {
                    acknowledged: true,
                    terminal: acknowledged.terminal,
                    attempt: acknowledged.attempt,
                    next_attempt_at: acknowledged.next_attempt_at.map(ts),
                    settled: acknowledged.settled,
                },
                // Do not distinguish a stale/mismatched lease from an exact
                // retry. Both must be idempotent no-ops and must reveal no
                // additional outbox state.
                None => proto::AcknowledgeApprovalDeliveryResponse {
                    acknowledged: false,
                    terminal: false,
                    attempt: 0,
                    next_attempt_at: None,
                    settled: false,
                },
            };
            let acknowledgement_state = if response.acknowledged {
                if response.settled {
                    "settled"
                } else if response.terminal {
                    "terminal"
                } else {
                    "retry"
                }
            } else {
                "noop"
            };
            metrics::counter!(
                "mp_session_approval_delivery_acknowledgements_total",
                "outcome" => acknowledgement_state,
            )
            .increment(1);
            Ok(Response::new(response))
        }
        .await;
        record_metrics("acknowledge_approval_delivery", started, result.is_ok());
        result
    }

    async fn get_subagent_lineage(
        &self,
        request: Request<proto::GetSubagentLineageRequest>,
    ) -> Result<Response<proto::GetSubagentLineageResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<proto::GetSubagentLineageResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "orchestration:read")?;
            let req = request.into_inner();
            if req.thread_id.is_empty() {
                return Err(Status::invalid_argument("thread_id is required"));
            }
            authorize_thread_owner(&self.pool, &caller, &req.thread_id, OwnerIntent::Read).await?;
            let lineage = fetch_lineage_for_thread(&self.pool, &req.thread_id)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
            let payload = if lineage.edges.is_empty() {
                None
            } else {
                Some(lineage)
            };
            Ok(Response::new(proto::GetSubagentLineageResponse {
                lineage: payload,
            }))
        }
        .await;
        record_metrics("get_subagent_lineage", started, result.is_ok());
        result
    }

    async fn attach_subagent(
        &self,
        request: Request<proto::AttachSubagentRequest>,
    ) -> Result<Response<proto::AttachSubagentResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<proto::AttachSubagentResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "orchestration:write")?;
            let req = request.into_inner();
            if req.thread_id.is_empty() {
                return Err(Status::invalid_argument("thread_id is required"));
            }
            if req.parent_run_id.is_empty() || req.child_run_id.is_empty() {
                return Err(Status::invalid_argument(
                    "parent_run_id and child_run_id are required",
                ));
            }
            authorize_thread_owner(&self.pool, &caller, &req.thread_id, OwnerIntent::Mutate)
                .await?;
            authorize_run_owner(&self.pool, &caller, &req.parent_run_id, OwnerIntent::Mutate)
                .await?;
            authorize_run_owner(&self.pool, &caller, &req.child_run_id, OwnerIntent::Mutate)
                .await?;
            let role_str = subagent_role_to_str(req.role);

            store::attach_subagent(
                &self.pool,
                &req.parent_run_id,
                &req.child_run_id,
                role_str,
                &JsonValue::Object(serde_json::Map::new()),
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

            let edge = proto::LineageEdge {
                parent_run_id: req.parent_run_id.clone(),
                child_run_id: req.child_run_id.clone(),
                role: req.role,
                spawned_at: Some(now_ts()),
            };
            let lineage = fetch_lineage_for_thread(&self.pool, &req.thread_id)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;

            broadcast_event(
                &self.events_tx,
                &self.replay,
                proto::OrchestrationEvent {
                    event_id: String::new(),
                    at: Some(now_ts()),
                    event: Some(orchestration_event::Event::SubagentAttached(
                        orchestration_event::SubagentAttached {
                            parent_run_id: req.parent_run_id.clone(),
                            child_run_id: req.child_run_id.clone(),
                            role: req.role,
                        },
                    )),
                },
            );

            Ok(Response::new(proto::AttachSubagentResponse {
                edge: Some(edge),
                lineage: Some(lineage),
            }))
        }
        .await;
        record_metrics("attach_subagent", started, result.is_ok());
        result
    }

    async fn record_orchestration_event(
        &self,
        request: Request<proto::RecordOrchestrationEventRequest>,
    ) -> Result<Response<proto::RecordOrchestrationEventResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<proto::RecordOrchestrationEventResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "orchestration:write")?;
            let req = request.into_inner();
            let mut ev = req
                .event
                .ok_or_else(|| Status::invalid_argument("event is required"))?;
            if ev.event.is_none() {
                return Err(Status::invalid_argument("event payload is required"));
            }
            // Only run-scoped events are deliverable on the per-run stream;
            // reject ones we couldn't route so callers learn early.
            if event_run_id(&ev).is_none_or(str::is_empty) {
                return Err(Status::invalid_argument(
                    "event must carry a non-empty run_id",
                ));
            }
            let run_id = event_run_id(&ev).expect("validated run id");
            authorize_run_owner(&self.pool, &caller, run_id, OwnerIntent::Mutate).await?;
            // Server owns the timestamp; fill it in when the caller left it empty.
            if ev.at.is_none() {
                ev.at = Some(now_ts());
            }
            let event_id = broadcast_event(&self.events_tx, &self.replay, ev);
            Ok(Response::new(proto::RecordOrchestrationEventResponse {
                event_id,
            }))
        }
        .await;
        record_metrics("record_orchestration_event", started, result.is_ok());
        result
    }

    async fn stream_run_events(
        &self,
        request: Request<proto::StreamRunEventsRequest>,
    ) -> Result<Response<Self::StreamRunEventsStream>, Status> {
        let caller = identity(&request)?;
        authorize_operation(&caller, "orchestration:read")?;
        let req = request.into_inner();
        if req.run_id.is_empty() {
            return Err(Status::invalid_argument("run_id is required"));
        }
        authorize_run_owner(&self.pool, &caller, &req.run_id, OwnerIntent::Read).await?;
        let target = req.run_id;
        let after = req.after_event_id;

        // Subscribe BEFORE snapshotting the buffer so no event can slip through
        // the gap between reading the buffer and attaching the live tail.
        let rx = self.events_tx.subscribe();

        // Replay buffered events strictly after the resume cursor. An empty
        // cursor means "live tail only" — the client seeds current state via a
        // snapshot (ListPlans/ListTodos/ListApprovals) on its side.
        let replayed: Vec<proto::OrchestrationEvent> = if after.is_empty() {
            Vec::new()
        } else {
            self.replay.replay_after(&target, &after).await
        };
        // Highest id already replayed — the live tail skips anything up to and
        // including it so the buffer/live overlap can't produce duplicates.
        let last_replayed = replayed.last().map(|ev| ev.event_id.clone());

        let target_live = target.clone();
        let live = BroadcastStream::new(rx).filter_map(move |item| {
            let target = target_live.clone();
            let last_replayed = last_replayed.clone();
            async move {
                match item {
                    Ok(ev) => {
                        if event_run_id(&ev).is_none_or(|id| id != target) {
                            return None;
                        }
                        if let Some(ref last_id) = last_replayed {
                            if ev.event_id.as_str() <= last_id.as_str() {
                                return None;
                            }
                        }
                        Some(Ok(ev))
                    }
                    Err(err) => {
                        warn!(error = %err, "broadcast lag, dropping event");
                        None
                    }
                }
            }
        });

        let replay_stream = tokio_stream::iter(replayed.into_iter().map(Ok));
        let combined = replay_stream.chain(live);
        let boxed: Self::StreamRunEventsStream = Box::pin(combined);
        Ok(Response::new(boxed))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_identity_is_pinned_and_cross_tenant_requests_are_rejected() {
        let caller = crate::auth::VerifiedIdentity::user_for_test("org-1", "user-1");
        let mut create = proto::CreateApprovalRequest {
            org_id: "org-1".to_owned(),
            user_id: "forged-user".to_owned(),
            ..Default::default()
        };
        pin_create_approval_identity(&caller, &mut create).expect("same tenant");
        assert_eq!(create.user_id, "user-1");

        let mut decision = proto::DecideApprovalRequest {
            org_id: "org-1".to_owned(),
            decided_by: "forged-actor".to_owned(),
            ..Default::default()
        };
        pin_approval_decision_identity(&caller, &mut decision).expect("same tenant");
        assert_eq!(decision.decided_by, "user-1");

        create.org_id = "org-2".to_owned();
        assert_eq!(
            pin_create_approval_identity(&caller, &mut create)
                .unwrap_err()
                .code(),
            tonic::Code::PermissionDenied
        );
    }

    #[test]
    fn continuation_descriptor_requires_exact_scope_and_rejects_credentials() {
        let req = proto::CreateApprovalRequest {
            run_id: "run-1".to_owned(),
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
            ..Default::default()
        };
        let valid = serde_json::json!({
            "version": 1,
            "run_id": "run-1",
            "org_id": "org-1",
            "user_id": "user-1",
            "action_kind": "tool_call",
            "action_fingerprint": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            "input": { "subject": "Hello" },
        })
        .to_string();
        assert!(validate_continuation_descriptor(&valid, &req).is_ok());

        let wrong_org = valid.replace("\"org-1\"", "\"org-2\"");
        assert_eq!(
            validate_continuation_descriptor(&wrong_org, &req)
                .expect_err("cross-org descriptor must fail")
                .code(),
            tonic::Code::PermissionDenied
        );

        let credential = valid.replace("\"subject\":\"Hello\"", "\"access_token\":\"secret\"");
        assert_eq!(
            validate_continuation_descriptor(&credential, &req)
                .expect_err("credentials must never be persisted")
                .code(),
            tonic::Code::InvalidArgument
        );
    }

    #[test]
    fn plan_state_round_trip() {
        for s in [
            "draft",
            "proposed",
            "approved",
            "rejected",
            "executing",
            "completed",
            "failed",
            "superseded",
            "archived",
        ] {
            let code = plan_state_from_str(s);
            assert_eq!(
                plan_state_to_str(code),
                Some(s),
                "round-trip failed for {s}"
            );
        }
    }

    #[test]
    fn plan_state_unknown_maps_to_unspecified() {
        assert_eq!(
            plan_state_from_str("nonsense"),
            proto::PlanState::Unspecified as i32
        );
        assert_eq!(plan_state_to_str(0), None);
    }

    #[test]
    fn approval_state_round_trip() {
        for s in ["requested", "granted", "denied", "timed_out"] {
            let code = approval_state_from_str(s);
            assert_eq!(approval_state_to_str(code), Some(s));
        }
    }

    #[test]
    fn approval_state_expired_alias_maps_to_timed_out() {
        assert_eq!(
            approval_state_from_str("expired"),
            proto::ApprovalState::TimedOut as i32
        );
    }

    #[test]
    fn approval_mutations_require_tenant_and_actor() {
        assert!(validate_approval_decision_context("", "user_1").is_err());
        assert!(validate_approval_decision_context("   ", "user_1").is_err());
        assert!(validate_approval_decision_context("org_1", "").is_err());
        assert!(validate_approval_decision_context("org_1", "   ").is_err());
        assert!(validate_approval_decision_context("org_1", "user_1").is_ok());
    }

    #[test]
    fn approval_delivery_is_service_only_tenant_scoped_and_zdr_fail_closed() {
        let worker =
            crate::auth::VerifiedIdentity::service_for_test("org-1", &["approval:deliver"], false);
        assert_eq!(
            authorize_approval_delivery_worker(&worker, "org-1").expect("same tenant worker"),
            "service:session-core"
        );
        assert_eq!(
            authorize_approval_delivery_worker(&worker, "org-2")
                .expect_err("cross-tenant worker must fail")
                .code(),
            tonic::Code::PermissionDenied
        );

        let user = crate::auth::VerifiedIdentity::user_for_test("org-1", "user-1");
        assert_eq!(
            authorize_approval_delivery_worker(&user, "org-1")
                .expect_err("interactive user must not claim outbox work")
                .code(),
            tonic::Code::PermissionDenied
        );

        let zdr_worker =
            crate::auth::VerifiedIdentity::service_for_test("org-1", &["approval:deliver"], true);
        assert_eq!(
            authorize_approval_delivery_worker(&zdr_worker, "org-1")
                .expect_err("ZDR principal must not operate durable approval delivery")
                .code(),
            tonic::Code::FailedPrecondition
        );
    }

    #[test]
    fn exact_approval_decision_retry_is_idempotent_but_conflicts_are_not() {
        let row = store::ApprovalRow {
            id: "approval-1".to_owned(),
            run_id: "run-1".to_owned(),
            plan_id: None,
            kind: "tool_call".to_owned(),
            status: "granted".to_owned(),
            requested_by: "user-1".to_owned(),
            decided_by: "user-1".to_owned(),
            decision_reason: "approved".to_owned(),
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
            idempotency_key: "decision-1".to_owned(),
            metadata: JsonValue::Object(serde_json::Map::new()),
            requested_at: Utc::now(),
            decided_at: Some(Utc::now()),
            expires_at: None,
        };
        assert!(is_idempotent_approval_retry(
            &row, "granted", "user-1", "approved"
        ));
        assert!(!is_idempotent_approval_retry(
            &row, "denied", "user-1", "approved"
        ));
        assert!(!is_idempotent_approval_retry(
            &row, "granted", "user-2", "approved"
        ));
    }

    #[tokio::test]
    async fn approval_decision_replay_does_not_emit_duplicate_events() {
        let row = store::ApprovalRow {
            id: "approval-1".to_owned(),
            run_id: "run-1".to_owned(),
            plan_id: None,
            kind: "tool_call".to_owned(),
            status: "granted".to_owned(),
            requested_by: "user-1".to_owned(),
            decided_by: "user-1".to_owned(),
            decision_reason: "approved".to_owned(),
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
            idempotency_key: "decision-1".to_owned(),
            metadata: JsonValue::Object(serde_json::Map::new()),
            requested_at: Utc::now(),
            decided_at: Some(Utc::now()),
            expires_at: None,
        };
        let (events_tx, mut events_rx) = broadcast::channel(8);
        let replay = ReplayBuffer::default();

        broadcast_approval_decision_events(
            &events_tx,
            &replay,
            &row,
            proto::ApprovalState::Granted as i32,
            true,
        );
        assert!(matches!(
            events_rx.try_recv().expect("state-change event").event,
            Some(orchestration_event::Event::ApprovalStateChanged(_))
        ));
        assert!(matches!(
            events_rx.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));

        broadcast_approval_decision_events(
            &events_tx,
            &replay,
            &row,
            proto::ApprovalState::Granted as i32,
            false,
        );
        assert!(matches!(
            events_rx.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
        assert_eq!(
            replay.replay_after("run-1", "").await.len(),
            1,
            "a grant is not a delivered execution continuation"
        );
    }

    #[test]
    fn approval_reads_require_tenant() {
        assert!(validate_approval_org("").is_err());
        assert!(validate_approval_org("   ").is_err());
        assert!(validate_approval_org("org_1").is_ok());
    }

    #[test]
    fn todo_state_round_trip() {
        for s in [
            "pending",
            "in_progress",
            "blocked",
            "completed",
            "cancelled",
        ] {
            let code = todo_state_from_str(s);
            assert_eq!(todo_state_to_str(code), Some(s));
        }
    }

    #[test]
    fn subagent_role_unknown_falls_back_to_generic() {
        assert_eq!(
            subagent_role_from_str("nonsense"),
            proto::SubagentRole::Unspecified as i32
        );
        assert_eq!(
            subagent_role_to_str(proto::SubagentRole::Unspecified as i32),
            "generic"
        );
        assert_eq!(
            subagent_role_to_str(proto::SubagentRole::Coder as i32),
            "coder"
        );
    }

    #[test]
    fn json_to_struct_object_keeps_keys() {
        let v = serde_json::json!({"a": 1, "b": "hi", "c": [1, 2]});
        let s = json_to_struct(&v).expect("struct");
        assert!(s.fields.contains_key("a"));
        assert!(s.fields.contains_key("b"));
        assert!(s.fields.contains_key("c"));
    }

    #[test]
    fn json_to_struct_null_returns_none() {
        assert!(json_to_struct(&JsonValue::Null).is_none());
    }

    #[test]
    fn json_to_struct_scalar_wraps_under_value_key() {
        let s = json_to_struct(&serde_json::json!("hello")).expect("struct");
        assert_eq!(s.fields.len(), 1);
        assert!(s.fields.contains_key("value"));
    }

    #[test]
    fn metadata_helpers_extract_fields() {
        let v = serde_json::json!({
            "title": "do thing",
            "supersedes": "plan_old",
            "blocked_by": ["t1", "t2"]
        });
        assert_eq!(metadata_str_field(&v, "title"), "do thing");
        assert_eq!(metadata_str_field(&v, "supersedes"), "plan_old");
        assert_eq!(metadata_str_field(&v, "missing"), "");
        assert_eq!(
            metadata_string_array(&v, "blocked_by"),
            vec!["t1".to_owned(), "t2".to_owned()]
        );
        assert!(metadata_string_array(&v, "missing").is_empty());
    }

    #[test]
    fn compute_max_depth_empty() {
        assert_eq!(compute_max_depth(&[]), 0);
    }

    #[test]
    fn compute_max_depth_single_edge() {
        let edges = vec![proto::LineageEdge {
            parent_run_id: "a".into(),
            child_run_id: "b".into(),
            role: 0,
            spawned_at: None,
        }];
        assert_eq!(compute_max_depth(&edges), 1);
    }

    #[test]
    fn compute_max_depth_chain() {
        let edges = vec![
            proto::LineageEdge {
                parent_run_id: "a".into(),
                child_run_id: "b".into(),
                role: 0,
                spawned_at: None,
            },
            proto::LineageEdge {
                parent_run_id: "b".into(),
                child_run_id: "c".into(),
                role: 0,
                spawned_at: None,
            },
            proto::LineageEdge {
                parent_run_id: "c".into(),
                child_run_id: "d".into(),
                role: 0,
                spawned_at: None,
            },
        ];
        assert_eq!(compute_max_depth(&edges), 3);
    }

    #[tokio::test]
    async fn broadcast_event_with_no_subscribers_is_no_op() {
        let (tx, rx_drop) = broadcast::channel::<proto::OrchestrationEvent>(8);
        drop(rx_drop);
        let ev = proto::OrchestrationEvent {
            event_id: String::new(),
            at: Some(now_ts()),
            event: Some(orchestration_event::Event::SubagentStopped(
                orchestration_event::SubagentStopped {
                    child_run_id: "run_x".into(),
                    status: "completed".into(),
                },
            )),
        };
        broadcast_event(&tx, &ReplayBuffer::default(), ev);
    }

    #[tokio::test]
    async fn broadcast_event_assigns_id_and_buffers_per_run() {
        let (tx, _rx) = broadcast::channel::<proto::OrchestrationEvent>(8);
        let replay = ReplayBuffer::default();

        // Run-scoped event: gets an id and lands in the run's buffer.
        let run_ev = proto::OrchestrationEvent {
            event_id: String::new(),
            at: Some(now_ts()),
            event: Some(orchestration_event::Event::RunPausedForApproval(
                orchestration_event::RunPausedForApproval {
                    run_id: "run_1".into(),
                    approval_id: "appr_1".into(),
                },
            )),
        };
        broadcast_event(&tx, &replay, run_ev);

        // Thread-only event (no run_id): assigned an id but not buffered per run.
        let todo_ev = proto::OrchestrationEvent {
            event_id: String::new(),
            at: Some(now_ts()),
            event: Some(orchestration_event::Event::TodoTransitioned(
                orchestration_event::TodoTransitioned {
                    todo_id: "todo_1".into(),
                    thread_id: "thread_1".into(),
                    from: 0,
                    to: 0,
                },
            )),
        };
        broadcast_event(&tx, &replay, todo_ev);

        let run_buf = replay.replay_after("run_1", "").await;
        assert_eq!(run_buf.len(), 1, "one run-scoped event buffered");
        assert!(
            !run_buf[0].event_id.is_empty(),
            "broadcast assigned a non-empty event_id"
        );
        // Thread-only events are not stored under a run key.
        assert!(
            replay.replay_after("thread_1", "").await.is_empty(),
            "thread-only event not buffered under a run"
        );
    }

    #[tokio::test]
    async fn replay_buffer_evicts_oldest_beyond_cap() {
        let (tx, _rx) = broadcast::channel::<proto::OrchestrationEvent>(8);
        let replay = ReplayBuffer::default();

        for _ in 0..(REPLAY_BUFFER_PER_RUN + 10) {
            broadcast_event(
                &tx,
                &replay,
                proto::OrchestrationEvent {
                    event_id: String::new(),
                    at: Some(now_ts()),
                    event: Some(orchestration_event::Event::RunPausedForApproval(
                        orchestration_event::RunPausedForApproval {
                            run_id: "run_cap".into(),
                            approval_id: "appr".into(),
                        },
                    )),
                },
            );
        }

        let buf = replay.replay_after("run_cap", "").await;
        assert_eq!(
            buf.len(),
            REPLAY_BUFFER_PER_RUN,
            "buffer is bounded to REPLAY_BUFFER_PER_RUN",
        );
    }

    #[test]
    // One assertion per oneof variant — a single flat test keeps the mapping
    // exhaustively visible in one place instead of splitting trivial per-variant
    // fixtures across many tiny tests.
    #[allow(clippy::too_many_lines)]
    fn event_run_id_extracts_per_variant() {
        let plan = proto::OrchestrationEvent {
            event_id: String::new(),
            at: None,
            event: Some(orchestration_event::Event::PlanTransitioned(
                orchestration_event::PlanTransitioned {
                    plan_id: "p".into(),
                    run_id: "run_1".into(),
                    from: 0,
                    to: 0,
                },
            )),
        };
        assert_eq!(event_run_id(&plan), Some("run_1"));

        let approval = proto::OrchestrationEvent {
            event_id: String::new(),
            at: None,
            event: Some(orchestration_event::Event::ApprovalStateChanged(
                orchestration_event::ApprovalStateChanged {
                    approval_id: "a".into(),
                    run_id: "run_2".into(),
                    approval_kind: 0,
                    to: 0,
                    decided_by: String::new(),
                },
            )),
        };
        assert_eq!(event_run_id(&approval), Some("run_2"));

        let attach = proto::OrchestrationEvent {
            event_id: String::new(),
            at: None,
            event: Some(orchestration_event::Event::SubagentAttached(
                orchestration_event::SubagentAttached {
                    parent_run_id: "run_p".into(),
                    child_run_id: "run_c".into(),
                    role: 0,
                },
            )),
        };
        assert_eq!(event_run_id(&attach), Some("run_p"));

        // If a future verified continuation emits RunResumedAfterApproval, it
        // remains run-scoped so it lands in the per-run replay buffer (SSE
        // resume cursor). A grant alone must never create this event.
        let resumed = proto::OrchestrationEvent {
            event_id: String::new(),
            at: None,
            event: Some(orchestration_event::Event::RunResumedAfterApproval(
                orchestration_event::RunResumedAfterApproval {
                    run_id: "run_r".into(),
                    approval_id: "appr_r".into(),
                },
            )),
        };
        assert_eq!(event_run_id(&resumed), Some("run_r"));

        // Browser-agent progress events (B4) are run-scoped so they reach the
        // per-run SSE stream and replay buffer.
        let browser_action = proto::OrchestrationEvent {
            event_id: String::new(),
            at: None,
            event: Some(orchestration_event::Event::BrowserActionDispatched(
                orchestration_event::BrowserActionDispatched {
                    run_id: "run_b".into(),
                    plan_id: "plan_b".into(),
                    action_id: "act_0001".into(),
                    action_type: "goto".into(),
                    url: "https://example.com".into(),
                    reason: "navigating to the landing page".into(),
                },
            )),
        };
        assert_eq!(event_run_id(&browser_action), Some("run_b"));

        let browser_obs = proto::OrchestrationEvent {
            event_id: String::new(),
            at: None,
            event: Some(orchestration_event::Event::BrowserObservationReceived(
                orchestration_event::BrowserObservationReceived {
                    run_id: "run_o".into(),
                    plan_id: "plan_o".into(),
                    action_id: "act_0001".into(),
                    status: "success".into(),
                    page_url: "https://example.com/landing".into(),
                    page_title: "Example".into(),
                    screenshot_ref: "art_shot".into(),
                    dom_snapshot_ref: String::new(),
                },
            )),
        };
        assert_eq!(event_run_id(&browser_obs), Some("run_o"));

        let paused = proto::OrchestrationEvent {
            event_id: String::new(),
            at: None,
            event: Some(orchestration_event::Event::BrowserRunPaused(
                orchestration_event::BrowserRunPaused {
                    run_id: "run_p".into(),
                    plan_id: "plan_p".into(),
                },
            )),
        };
        assert_eq!(event_run_id(&paused), Some("run_p"));

        let resumed_run = proto::OrchestrationEvent {
            event_id: String::new(),
            at: None,
            event: Some(orchestration_event::Event::BrowserRunResumed(
                orchestration_event::BrowserRunResumed {
                    run_id: "run_p".into(),
                    plan_id: "plan_p".into(),
                },
            )),
        };
        assert_eq!(event_run_id(&resumed_run), Some("run_p"));
    }

    #[tokio::test]
    async fn record_orchestration_event_broadcasts_and_buffers_browser_event() {
        let (tx, mut rx) = broadcast::channel::<proto::OrchestrationEvent>(8);
        let replay = ReplayBuffer::default();

        let ev = proto::OrchestrationEvent {
            event_id: String::new(),
            at: Some(now_ts()),
            event: Some(orchestration_event::Event::BrowserActionDispatched(
                orchestration_event::BrowserActionDispatched {
                    run_id: "run_rec".into(),
                    plan_id: "plan_rec".into(),
                    action_id: "act_0001".into(),
                    action_type: "observe".into(),
                    url: String::new(),
                    reason: String::new(),
                },
            )),
        };
        let event_id = broadcast_event(&tx, &replay, ev);
        assert!(!event_id.is_empty(), "broadcast assigned an event_id");

        let received = rx.try_recv().expect("subscriber received the event");
        assert_eq!(received.event_id, event_id);
        assert_eq!(event_run_id(&received), Some("run_rec"));

        let buffered = replay.replay_after("run_rec", "").await;
        assert_eq!(buffered.len(), 1, "browser event buffered for replay");
    }

    // -----------------------------------------------------------------------
    // Proof bundle assembly
    // -----------------------------------------------------------------------

    fn test_provenance() -> store::RunProvenanceRow {
        store::RunProvenanceRow {
            goal: "book the shipment".to_owned(),
            agent_id: "general-v1".to_owned(),
            status: "completed".to_owned(),
            created_at: chrono::Utc::now(),
        }
    }

    fn test_approval(id: &str) -> store::ApprovalRow {
        store::ApprovalRow {
            id: id.to_owned(),
            run_id: "run_1".to_owned(),
            plan_id: None,
            kind: "tool".to_owned(),
            status: "granted".to_owned(),
            requested_by: "model".to_owned(),
            decided_by: "user-1".to_owned(),
            decision_reason: "looks right".to_owned(),
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
            idempotency_key: String::new(),
            metadata: serde_json::json!({}),
            requested_at: chrono::Utc::now(),
            decided_at: Some(chrono::Utc::now()),
            expires_at: None,
        }
    }

    fn test_evidence(approval_id: &str) -> store::ContinuationEvidenceRow {
        store::ContinuationEvidenceRow {
            approval_id: approval_id.to_owned(),
            receipt_id: "rcpt_1".to_owned(),
            delivery_id: "dlv_1".to_owned(),
            action_fingerprint: "a".repeat(64),
            execution_service_id: "execution-core".to_owned(),
            descriptor_version: 1,
            started_at: chrono::Utc::now(),
            outcome: None,
            provider_receipt_id: None,
            failure_code: None,
            finalized_at: None,
            verification_status: None,
            verification_method: None,
            verification_reason: None,
        }
    }

    #[test]
    fn proof_bundle_always_declares_the_sections_it_cannot_prove() {
        let bundle = build_run_proof_bundle("run_1", "org-1", &test_provenance(), &[], &[]);

        assert_eq!(bundle.bundle_version, 1);
        assert_eq!(bundle.run_id, "run_1");
        assert_eq!(bundle.org_id, "org-1");
        // A run with no approvals is a fact about the run, not a missing
        // section — but the cross-plane gaps must still be named.
        assert!(bundle.approvals.is_empty());
        let sections: Vec<&str> = bundle
            .unavailable
            .iter()
            .map(|s| s.section.as_str())
            .collect();
        assert_eq!(sections, vec!["known", "charged", "retained"]);
        for section in &bundle.unavailable {
            assert!(
                !section.reason.trim().is_empty(),
                "every unavailable section must state why: {}",
                section.section
            );
        }
    }

    #[test]
    fn an_approval_without_a_continuation_reports_no_execution() {
        let bundle = build_run_proof_bundle(
            "run_1",
            "org-1",
            &test_provenance(),
            &[test_approval("appr_1")],
            &[],
        );

        let proof = &bundle.approvals[0];
        assert_eq!(proof.approval_id, "appr_1");
        assert_eq!(proof.decided_by, "user-1");
        // Authority is present; execution must NOT be invented from it.
        assert!(
            proof.execution.is_none(),
            "an approval alone is authority, never evidence that work started"
        );
    }

    #[test]
    fn an_in_flight_continuation_reports_execution_but_no_outcome() {
        let bundle = build_run_proof_bundle(
            "run_1",
            "org-1",
            &test_provenance(),
            &[test_approval("appr_1")],
            &[test_evidence("appr_1")],
        );

        let execution = bundle.approvals[0]
            .execution
            .as_ref()
            .expect("receipt proves the work started");
        assert_eq!(execution.receipt_id, "rcpt_1");
        assert_eq!(execution.execution_service_id, "execution-core");
        assert!(
            execution.outcome.is_none(),
            "a receipt with no outcome row must not become a fabricated terminal state"
        );
    }

    #[test]
    fn a_verified_outcome_carries_the_shared_contract_status() {
        let mut evidence = test_evidence("appr_1");
        evidence.outcome = Some("completed".to_owned());
        evidence.provider_receipt_id = Some("LC652849244NO".to_owned());
        evidence.finalized_at = Some(chrono::Utc::now());
        evidence.verification_status = Some("verified_success".to_owned());
        evidence.verification_method = Some("structural".to_owned());
        evidence.verification_reason =
            Some("provider returned an authoritative receipt".to_owned());

        let bundle = build_run_proof_bundle(
            "run_1",
            "org-1",
            &test_provenance(),
            &[test_approval("appr_1")],
            &[evidence],
        );

        let outcome = bundle.approvals[0]
            .execution
            .as_ref()
            .and_then(|e| e.outcome.as_ref())
            .expect("finalized continuation has an outcome");
        assert_eq!(outcome.outcome, "completed");
        assert_eq!(outcome.provider_receipt_id, "LC652849244NO");

        let verification = outcome
            .verification
            .as_ref()
            .expect("a recorded verification must survive assembly");
        assert_eq!(
            verification.status,
            proto::VerificationStatus::VerifiedSuccess as i32
        );
        // `method` is what stops today's mechanical check being read as an
        // independent postcondition verification.
        assert_eq!(verification.method, "structural");
        assert_eq!(verification.effect_id, "rcpt_1");
    }

    #[test]
    fn an_outcome_without_a_recorded_verification_claims_none() {
        let mut evidence = test_evidence("appr_1");
        evidence.outcome = Some("failed".to_owned());
        evidence.failure_code = Some("provider_rejected".to_owned());
        evidence.finalized_at = Some(chrono::Utc::now());

        let bundle = build_run_proof_bundle(
            "run_1",
            "org-1",
            &test_provenance(),
            &[test_approval("appr_1")],
            &[evidence],
        );

        let outcome = bundle.approvals[0]
            .execution
            .as_ref()
            .and_then(|e| e.outcome.as_ref())
            .expect("finalized continuation has an outcome");
        assert_eq!(outcome.outcome, "failed");
        assert!(
            outcome.verification.is_none(),
            "absent verification must stay absent, never default to a status"
        );
    }

    #[test]
    fn an_unrecognized_verification_status_never_reads_as_success() {
        for value in ["", "definitely_fine", "success", "unknown"] {
            assert_eq!(
                verification_status_to_proto(value),
                proto::VerificationStatus::Unknown as i32,
                "unrecognized status {value:?} must floor to UNKNOWN"
            );
        }
    }

    #[test]
    fn evidence_is_matched_to_its_own_approval() {
        let bundle = build_run_proof_bundle(
            "run_1",
            "org-1",
            &test_provenance(),
            &[test_approval("appr_1"), test_approval("appr_2")],
            // Evidence belongs to appr_2 only.
            &[test_evidence("appr_2")],
        );

        assert!(
            bundle.approvals[0].execution.is_none(),
            "appr_1 has no continuation and must not borrow appr_2's evidence"
        );
        assert!(bundle.approvals[1].execution.is_some());
    }
}
