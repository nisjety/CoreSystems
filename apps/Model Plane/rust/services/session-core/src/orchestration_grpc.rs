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
    authorize_operation, authorize_owner_row, authorize_thread_owner_row, identity, OwnerIntent,
    VerifiedIdentity,
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
    // `space_id` rides along because a system-owned thread inside a Space is
    // not org-readable by id (see `authorize_thread_owner_row`).
    let owner: Option<(String, String, String)> = sqlx::query_as(
        "SELECT org_id, user_id, COALESCE(space_id, '') FROM threads WHERE id = $1",
    )
    .bind(thread_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| Status::internal(error.to_string()))?;
    let (org_id, user_id, space_id) = owner.ok_or_else(|| Status::not_found("thread not found"))?;
    authorize_thread_owner_row(caller, &org_id, &user_id, &space_id, intent)
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

/// The domain crate's state for a wire `PlanState` code, so the RPC can ask
/// the plan lifecycle's own transition table (`mp_orchestration::Plan::
/// is_allowed`) instead of keeping a second, partial copy of it here.
fn domain_plan_state(code: i32) -> Option<mp_orchestration::PlanState> {
    use mp_orchestration::PlanState as Domain;
    Some(match proto::PlanState::try_from(code).ok()? {
        proto::PlanState::Unspecified => return None,
        proto::PlanState::Draft => Domain::Draft,
        proto::PlanState::Proposed => Domain::Proposed,
        proto::PlanState::Approved => Domain::Approved,
        proto::PlanState::Rejected => Domain::Rejected,
        proto::PlanState::Executing => Domain::Executing,
        proto::PlanState::Completed => Domain::Completed,
        proto::PlanState::Failed => Domain::Failed,
        proto::PlanState::Superseded => Domain::Superseded,
        proto::PlanState::Archived => Domain::Archived,
    })
}

/// plan-gate §2.2's transition legality rule for `TransitionPlan`, factored
/// out so it can be unit-tested without a database:
///
/// * Edits (`edited_steps` non-empty) are legal only when the plan is
///   currently `PLAN_STATE_PROPOSED` — a plan already EXECUTING or terminal
///   may have steps with real recorded outcomes, and rewriting them there
///   would corrupt the audit trail an edited plan is supposed to strengthen.
/// * `PLAN_STATE_APPROVED` is only reachable from `PLAN_STATE_PROPOSED` —
///   approving IS granting the run authority to act on this plan, so it must
///   come from a plan a human actually reviewed, never a bare DRAFT (zero
///   steps) or an already-terminal plan.
/// * Every other transition must be one the plan lifecycle allows
///   (`mp_orchestration::Plan::is_allowed`): the proto documents the target
///   as "must be reachable from the current state", but the check used to
///   cover APPROVED alone, so a decided plan could be REJECTED again (or an
///   EXECUTING/COMPLETED one rejected after the fact), rewriting the very
///   record `RUN_PLAN_MODE_EXITED.plan_id` points at, and a REJECTED plan
///   could be walked back to PROPOSED — the revise-after-reject the owner
///   ruled out of this wave.
#[allow(clippy::result_large_err)]
pub(crate) fn validate_plan_transition_legality(
    prev_state: i32,
    target_state: i32,
    has_edits: bool,
) -> Result<(), Status> {
    if has_edits && prev_state != proto::PlanState::Proposed as i32 {
        return Err(Status::failed_precondition(
            "edited_steps can only be applied to a plan in PLAN_STATE_PROPOSED",
        ));
    }
    if target_state == proto::PlanState::Approved as i32
        && prev_state != proto::PlanState::Proposed as i32
    {
        return Err(Status::failed_precondition(
            "a plan can only be approved from PLAN_STATE_PROPOSED",
        ));
    }
    let (Some(from), Some(to)) = (domain_plan_state(prev_state), domain_plan_state(target_state))
    else {
        return Err(Status::failed_precondition(
            "the plan or target state is not a known plan state",
        ));
    };
    if !mp_orchestration::Plan::is_allowed(from, to) {
        return Err(Status::failed_precondition(format!(
            "a plan cannot move from {} to {}",
            from.as_str(),
            to.as_str()
        )));
    }
    Ok(())
}

/// Who moved a plan into `new_status`, and why, as a `metadata` patch merged
/// in the same write as the transition.
///
/// Keys are per-state (`approved_by`, `approved_reason`, `executing_by`, …)
/// so the later `EXECUTING` claim does not overwrite the `APPROVED` decision:
/// the plan then carries its own approval receipt — the person and their
/// stated ground — instead of depending on the run-exit event that is only
/// published while the gateway still remembers the run being in plan mode.
/// `actor` is the verified principal (the handler overwrites the request's),
/// not caller-supplied text; `reason` is bounded like every other free text
/// on a plan.
fn plan_decision_metadata(new_status: &str, actor: &str, reason: &str) -> JsonValue {
    let mut patch = serde_json::Map::new();
    if !actor.is_empty() {
        patch.insert(format!("{new_status}_by"), JsonValue::String(actor.to_owned()));
    }
    let reason = reason.trim();
    if !reason.is_empty() {
        patch.insert(
            format!("{new_status}_reason"),
            JsonValue::String(reason.chars().take(1_000).collect()),
        );
    }
    JsonValue::Object(patch)
}

/// Ceilings on the text of a plan, enforced HERE because the two writers of
/// step text — the model (`plan.propose`, untrusted: it reads web pages and
/// documents) and the browser (`edited_steps`) — are both outside this
/// service's trust boundary, and nothing else bounds them: the columns are
/// unbounded `TEXT`/`JSONB`, and every step is later copied into the
/// executing run's context (`approved_plan_context`).
pub(crate) const MAX_PLAN_STEPS: usize = 100;
/// See [`MAX_PLAN_STEPS`]. Entries in `edited_steps` include removed and
/// newly added steps, so the bound is wider than the step count itself.
pub(crate) const MAX_PLAN_STEP_EDITS: usize = 2 * MAX_PLAN_STEPS;
/// See [`MAX_PLAN_STEPS`].
pub(crate) const MAX_PLAN_TITLE_CHARS: usize = 500;
/// See [`MAX_PLAN_STEPS`].
pub(crate) const MAX_PLAN_OPERATION_CHARS: usize = 120;
/// See [`MAX_PLAN_STEPS`].
pub(crate) const MAX_PLAN_SUMMARY_CHARS: usize = 4_000;
/// See [`MAX_PLAN_STEPS`].
pub(crate) const MAX_PLAN_SUPERSEDES_CHARS: usize = 128;

/// The shape a model-proposed plan must have before it is stored.
#[allow(clippy::result_large_err)]
pub(crate) fn validate_proposed_plan_input(
    summary: &str,
    steps: &[proto::ProposedPlanStep],
    supersedes: &str,
) -> Result<(), Status> {
    if summary.trim().is_empty() {
        return Err(Status::invalid_argument("summary is required"));
    }
    if summary.chars().count() > MAX_PLAN_SUMMARY_CHARS {
        return Err(Status::invalid_argument(format!(
            "summary is longer than {MAX_PLAN_SUMMARY_CHARS} characters"
        )));
    }
    if steps.is_empty() {
        return Err(Status::invalid_argument("at least one step is required"));
    }
    if steps.len() > MAX_PLAN_STEPS {
        return Err(Status::invalid_argument(format!(
            "a plan may have at most {MAX_PLAN_STEPS} steps"
        )));
    }
    for step in steps {
        if step.title.trim().is_empty() {
            return Err(Status::invalid_argument("every step needs a title"));
        }
        if step.title.chars().count() > MAX_PLAN_TITLE_CHARS {
            return Err(Status::invalid_argument(format!(
                "a step title is longer than {MAX_PLAN_TITLE_CHARS} characters"
            )));
        }
        if step.operation.chars().count() > MAX_PLAN_OPERATION_CHARS {
            return Err(Status::invalid_argument(format!(
                "a step operation is longer than {MAX_PLAN_OPERATION_CHARS} characters"
            )));
        }
    }
    if supersedes.chars().count() > MAX_PLAN_SUPERSEDES_CHARS {
        return Err(Status::invalid_argument("supersedes is not a plan id"));
    }
    Ok(())
}

/// Resolve the user's `edited_steps` against the plan's real step ids
/// (plan-gate §2.2), or refuse them.
///
/// The array is the COMPLETE new step list, in order: every existing step
/// appears exactly once (a removed one with `removed = true`), new steps have
/// an empty `step_id`. This used to be assumed rather than checked, and the
/// gaps were silent:
///
/// * a step left out of the array kept its old ordinal and stayed `pending`,
///   so it was still in the plan the run would be told it had approved — or
///   collided with an edited step's new ordinal and failed the whole
///   approval with an opaque 500;
/// * an unknown `step_id` was quietly inserted as a NEW step (a browser
///   holding the steps of a different or superseded plan turned into extra
///   steps in this one);
/// * a blank title, or an approval with every step removed, went through —
///   the "rubber stamp" the confirm gate exists to prevent, enforced only by
///   a disabled button in the browser.
///
/// `require_remaining` is true for an approval: at least one step must
/// survive. A step that was added and removed in the same editing session
/// (empty `step_id`, `removed`) never belonged to the proposal, so it is
/// dropped rather than recorded as a decided-against step.
#[allow(clippy::result_large_err)]
pub(crate) fn resolve_plan_step_edits(
    existing_ids: &[&str],
    edits: &[proto::PlanStepEdit],
    require_remaining: bool,
    mut mint_step_id: impl FnMut() -> String,
) -> Result<Vec<store::PlanStepEditRow>, Status> {
    if edits.len() > MAX_PLAN_STEP_EDITS {
        return Err(Status::invalid_argument(format!(
            "edited_steps may have at most {MAX_PLAN_STEP_EDITS} entries"
        )));
    }
    let existing: std::collections::HashSet<&str> = existing_ids.iter().copied().collect();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut resolved = Vec::with_capacity(edits.len());
    for edit in edits {
        let title = edit.title.trim();
        let operation = edit.operation.trim();
        if title.chars().count() > MAX_PLAN_TITLE_CHARS {
            return Err(Status::invalid_argument(format!(
                "a step title is longer than {MAX_PLAN_TITLE_CHARS} characters"
            )));
        }
        if operation.chars().count() > MAX_PLAN_OPERATION_CHARS {
            return Err(Status::invalid_argument(format!(
                "a step operation is longer than {MAX_PLAN_OPERATION_CHARS} characters"
            )));
        }
        let step_id = edit.step_id.trim();
        if step_id.is_empty() {
            if edit.removed {
                continue;
            }
            if title.is_empty() {
                return Err(Status::invalid_argument("a new step needs a title"));
            }
            resolved.push(store::PlanStepEditRow {
                id: mint_step_id(),
                is_new: true,
                title: title.to_owned(),
                operation: operation.to_owned(),
                removed: false,
            });
            continue;
        }
        if !existing.contains(step_id) {
            return Err(Status::invalid_argument(format!(
                "edited_steps names a step that is not in this plan: {step_id}"
            )));
        }
        if !seen.insert(step_id) {
            return Err(Status::invalid_argument(format!(
                "edited_steps lists a step twice: {step_id}"
            )));
        }
        if !edit.removed && title.is_empty() {
            return Err(Status::invalid_argument("a step needs a title"));
        }
        resolved.push(store::PlanStepEditRow {
            id: step_id.to_owned(),
            is_new: false,
            title: title.to_owned(),
            operation: operation.to_owned(),
            removed: edit.removed,
        });
    }
    if let Some(missing) = existing_ids.iter().find(|id| !seen.contains(**id)) {
        return Err(Status::invalid_argument(format!(
            "edited_steps must list every existing step (missing {missing}); \
             mark a removed step removed=true"
        )));
    }
    if require_remaining && resolved.iter().all(|row| row.removed) {
        return Err(Status::invalid_argument(
            "a plan cannot be approved with every step removed",
        ));
    }
    Ok(resolved)
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
    // A retained action payload must have a finite lifecycle. `0` means
    // "no deadline" for proposal-only approvals, but allowing it here would
    // retain action input indefinitely; the validated descriptor is encrypted
    // into the dedicated continuation table before it is persisted.
    if req.expires_in_seconds == 0 {
        return Err(Status::invalid_argument(
            "continuation descriptor requires a finite approval expiry",
        ));
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

/// The `subagent_edges.metadata` JSON for one attached edge.
///
/// `goal_summary`/`batch_id` (wide-research fan-out §2.4) have no dedicated
/// columns — they ride the `metadata` JSONB column `attach_subagent` already
/// accepted (previously always an empty object), which avoids a migration for
/// two additive, optional fields. An unset field is ABSENT rather than stored
/// as an empty string, so an edge from a caller that predates the fields is
/// byte-identical to what it was before them.
fn subagent_edge_metadata(goal_summary: &str, batch_id: &str) -> JsonValue {
    let mut metadata = serde_json::Map::new();
    if !goal_summary.is_empty() {
        metadata.insert(
            "goal_summary".to_owned(),
            JsonValue::String(goal_summary.to_owned()),
        );
    }
    if !batch_id.is_empty() {
        metadata.insert(
            "batch_id".to_owned(),
            JsonValue::String(batch_id.to_owned()),
        );
    }
    JsonValue::Object(metadata)
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

/// Inverse of [`ts`]. `None` for a value with no valid Postgres representation
/// (out of `chrono`'s range) rather than silently clamping to some default -
/// the caller must reject the request explicitly.
fn prost_timestamp_to_chrono(value: prost_types::Timestamp) -> Option<DateTime<Utc>> {
    DateTime::from_timestamp(value.seconds, u32::try_from(value.nanos).ok()?)
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

/// Inverse of [`json_to_struct`], for writing an inbound proto `Struct` field
/// into a JSONB column. Used by `append_message_inner` to persist a turn's
/// evidence metadata; keep the two in step so a value survives the round trip.
pub(crate) fn struct_to_json(s: &prost_types::Struct) -> JsonValue {
    JsonValue::Object(
        s.fields
            .iter()
            .map(|(k, v)| (k.clone(), prost_value_to_json(v)))
            .collect(),
    )
}

fn prost_value_to_json(v: &prost_types::Value) -> JsonValue {
    use prost_types::value::Kind;
    match &v.kind {
        // A `Value` with no kind set is the proto default, which is null.
        Some(Kind::NullValue(_)) | None => JsonValue::Null,
        Some(Kind::BoolValue(b)) => JsonValue::Bool(*b),
        Some(Kind::NumberValue(n)) => serde_json::Number::from_f64(*n)
            .map_or(JsonValue::Null, JsonValue::Number),
        Some(Kind::StringValue(s)) => JsonValue::String(s.clone()),
        Some(Kind::StructValue(inner)) => struct_to_json(inner),
        Some(Kind::ListValue(list)) => {
            JsonValue::Array(list.values.iter().map(prost_value_to_json).collect())
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
    // `complete_step` writes execution steps with no `title`, so this was
    // always empty for them and every step surfaced as its bare `kind`. Fall
    // back to the tool the step ran, and carry the outcome out in `detail`.
    let stored_title = metadata_str_field(&row.payload, "title");
    let title = if stored_title.is_empty() {
        plan_step_tool_name(&row.payload).unwrap_or_default()
    } else {
        stored_title
    };
    proto::PlanStep {
        id: row.id.clone(),
        title,
        operation: row.kind.clone(),
        state: plan_step_state_from_str(&row.status),
        detail: plan_step_detail(&row.payload).unwrap_or_default(),
        created_at: Some(ts(row.created_at)),
        updated_at: Some(ts(row.updated_at)),
    }
}

/// The tool an execution step ran, recovered from the bracket envelope the
/// tool layer prefixes to its output and errors
/// (`[data_category=… zdr=false tool=web_search] …`).
///
/// Best-effort by necessity: `CompleteStepRequest` carries no tool field, so
/// the name exists nowhere else on the record. Returns `None` rather than
/// guessing when the envelope is absent, which leaves the previous behaviour
/// (caller falls back to the step's `kind`) untouched.
fn plan_step_tool_name(payload: &JsonValue) -> Option<String> {
    let haystack = ["error", "output"]
        .iter()
        .filter_map(|key| payload.get(*key).and_then(JsonValue::as_str))
        .find(|value| value.contains("tool="))?;
    let after = haystack.split("tool=").nth(1)?;
    let name: String = after
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-' || *c == '.')
        .collect();
    (!name.is_empty()).then_some(name)
}

/// One line describing how a step went: its error if it has one, otherwise a
/// trimmed slice of its output. The bracket envelope is stripped so the reader
/// sees the message rather than the policy annotation that precedes it.
fn plan_step_detail(payload: &JsonValue) -> Option<String> {
    const MAX_DETAIL_CHARS: usize = 240;
    let raw = ["error", "output"]
        .iter()
        .filter_map(|key| payload.get(*key).and_then(JsonValue::as_str))
        .map(str::trim)
        .find(|value| !value.is_empty())?;
    let stripped = raw
        .strip_prefix('[')
        .and_then(|rest| rest.split_once(']'))
        .map_or(raw, |(_, tail)| tail)
        .trim();
    if stripped.is_empty() {
        return None;
    }
    let detail: String = stripped.chars().take(MAX_DETAIL_CHARS).collect();
    Some(detail)
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

/// Dimensions this metric set deliberately does not claim, same discipline as
/// `unavailable_proof_sections`.
fn unavailable_verification_metrics() -> Vec<proto::UnavailableSection> {
    vec![
        proto::UnavailableSection {
            section: "cost_per_verified_outcome".to_string(),
            reason: "Cost is owned by cost-core, a separate database this plane cannot join; \
                     cost_entries carries a run_id, so a cross-service correlation is possible \
                     future work, not fabricated here."
                .to_string(),
        },
        proto::UnavailableSection {
            section: "unnecessary_approval".to_string(),
            reason: "Whether a granted approval was ever truly necessary is not inferable from \
                     this schema - it would require a counterfactual (would the action have \
                     been safe without human review) this plane has no data source for."
                .to_string(),
        },
        proto::UnavailableSection {
            section: "rollback_rate".to_string(),
            reason: "No rollback mechanism exists yet (roadmap P1 item 4, stateful simulators \
                     and CI release gates, is unbuilt); there is nothing to count."
                .to_string(),
        },
    ]
}

/// Aggregate one organization's Verified Outcome Foundation metrics from
/// already-fetched, already-tenant-scoped rows. Pure and DB-free by
/// construction, mirroring `build_run_proof_bundle` — every rate below is
/// computed here so the rule can be unit-tested without a live Postgres.
fn build_verification_metrics(
    org_id: &str,
    since: Option<DateTime<Utc>>,
    continuations: &[store::ContinuationMetricsRow],
    approvals: &[store::ApprovalMetricsRow],
) -> proto::VerificationMetrics {
    let total_completed = continuations
        .iter()
        .filter(|c| c.outcome == "completed")
        .count() as i64;
    let total_failed = continuations
        .iter()
        .filter(|c| c.outcome == "failed")
        .count() as i64;
    let total_cancelled = continuations
        .iter()
        .filter(|c| c.outcome == "cancelled")
        .count() as i64;

    // False success is the headline this whole layer exists to produce: the
    // dispatcher recorded a structural `completed`, but the independent
    // postcondition check came back `verified_failure` - the boundary said
    // yes, the system of record said no.
    let false_success_count = continuations
        .iter()
        .filter(|c| {
            c.outcome == "completed" && c.verification_status.as_deref() == Some("verified_failure")
        })
        .count() as i64;
    let verified_success_count = continuations
        .iter()
        .filter(|c| c.verification_status.as_deref() == Some("verified_success"))
        .count() as i64;
    let partially_verified_count = continuations
        .iter()
        .filter(|c| c.verification_status.as_deref() == Some("partially_verified"))
        .count() as i64;
    let unknown_verification_count = continuations
        .iter()
        .filter(|c| c.verification_status.as_deref() == Some("unknown"))
        .count() as i64;
    // NULL, not "unknown": rows with no verification_status recorded at all.
    // See the proto doc - this means "predates the instrumentation," since
    // every live dispatch persists at least a structural `unknown`.
    let unverified_count = continuations
        .iter()
        .filter(|c| c.verification_status.is_none())
        .count() as i64;

    let structural_method_count = continuations
        .iter()
        .filter(|c| c.verification_method.as_deref() == Some("structural"))
        .count() as i64;
    let postcondition_method_count = continuations
        .iter()
        .filter(|c| c.verification_method.as_deref() == Some("postcondition"))
        .count() as i64;

    let median_seconds_receipt_to_outcome = median_seconds(
        continuations
            .iter()
            .map(|c| (c.finalized_at - c.started_at).num_seconds()),
    );

    let approvals_requested = approvals.len() as i64;
    let approvals_granted = approvals.iter().filter(|a| a.status == "granted").count() as i64;
    let approvals_denied = approvals.iter().filter(|a| a.status == "denied").count() as i64;
    let median_seconds_approval_decision = median_seconds(approvals.iter().filter_map(|a| {
        a.decided_at
            .map(|decided_at| (decided_at - a.requested_at).num_seconds())
    }));

    proto::VerificationMetrics {
        org_id: org_id.to_string(),
        since: since.map(ts),
        generated_at: Some(now_ts()),
        total_completed,
        total_failed,
        total_cancelled,
        verified_success_count,
        false_success_count,
        partially_verified_count,
        unverified_count,
        unknown_verification_count,
        structural_method_count,
        postcondition_method_count,
        approvals_requested,
        approvals_granted,
        approvals_denied,
        median_seconds_receipt_to_outcome,
        median_seconds_approval_decision,
        unavailable: unavailable_verification_metrics(),
    }
}

/// Median of a whole-seconds duration iterator. `0` for an empty input -
/// callers must read that alongside the matching count field (e.g.
/// `total_completed`) rather than mistaking it for "instant."
///
/// Sorts rather than using a mean specifically so one slow outlier (a stuck
/// worker, a provider incident) cannot dominate the reported figure the way
/// an average would.
fn median_seconds(values: impl Iterator<Item = i64>) -> i64 {
    let mut values: Vec<i64> = values.collect();
    if values.is_empty() {
        return 0;
    }
    values.sort_unstable();
    let mid = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[mid - 1] + values[mid]) / 2
    } else {
        values[mid]
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

/// Persist the safe browser-agent event projection in the canonical event log.
///
/// Browser progress historically lived only in the per-run Redis/in-memory
/// replay ring. That is enough for a reconnecting tab, but not for a reload
/// after the ring TTL or a process restart. Keep the durable record deliberately
/// narrow: action metadata, page metadata, and artifact references are useful
/// to the Work canvas; raw DOM/text/credentials are never copied into this
/// path. The generic event table supplies the run's tenant/user from `runs`, so
/// the caller cannot forge ownership through the event payload.
async fn persist_browser_event(
    pool: &Pool,
    event_id: &str,
    event: &proto::OrchestrationEvent,
) -> Result<(), sqlx::Error> {
    let (event_type, run_id, payload, type_url) = match event.event.as_ref() {
        Some(orchestration_event::Event::BrowserActionDispatched(value)) => (
            "BROWSER_ACTION_DISPATCHED",
            value.run_id.as_str(),
            serde_json::json!({
                "run_id": value.run_id,
                "plan_id": value.plan_id,
                "action_id": value.action_id,
                "action_type": value.action_type,
                "url": value.url,
                "reason": value.reason,
            }),
            "type.googleapis.com/model_plane.v1.BrowserActionDispatched",
        ),
        Some(orchestration_event::Event::BrowserObservationReceived(value)) => (
            "BROWSER_OBSERVATION_RECEIVED",
            value.run_id.as_str(),
            serde_json::json!({
                "run_id": value.run_id,
                "plan_id": value.plan_id,
                "action_id": value.action_id,
                "status": value.status,
                "page_url": value.page_url,
                "page_title": value.page_title,
                "screenshot_ref": value.screenshot_ref,
                "dom_snapshot_ref": value.dom_snapshot_ref,
            }),
            "type.googleapis.com/model_plane.v1.BrowserObservationReceived",
        ),
        Some(orchestration_event::Event::BrowserRunPaused(value)) => (
            "BROWSER_RUN_PAUSED",
            value.run_id.as_str(),
            serde_json::json!({ "run_id": value.run_id, "plan_id": value.plan_id }),
            "type.googleapis.com/model_plane.v1.BrowserRunPaused",
        ),
        Some(orchestration_event::Event::BrowserRunResumed(value)) => (
            "BROWSER_RUN_RESUMED",
            value.run_id.as_str(),
            serde_json::json!({ "run_id": value.run_id, "plan_id": value.plan_id }),
            "type.googleapis.com/model_plane.v1.BrowserRunResumed",
        ),
        Some(orchestration_event::Event::BrowserActionApprovalRequired(value)) => (
            "BROWSER_ACTION_APPROVAL_REQUIRED",
            value.run_id.as_str(),
            serde_json::json!({
                "run_id": value.run_id,
                "plan_id": value.plan_id,
                "action_id": value.action_id,
                "action_type": value.action_type,
                "url": value.url,
                "selector": value.selector,
                "reason": value.reason,
                "risk_category": value.risk_category,
                "approval_id": value.approval_id,
            }),
            "type.googleapis.com/model_plane.v1.BrowserActionApprovalRequired",
        ),
        Some(orchestration_event::Event::BrowserActionDecided(value)) => (
            "BROWSER_ACTION_DECIDED",
            value.run_id.as_str(),
            serde_json::json!({
                "run_id": value.run_id,
                "plan_id": value.plan_id,
                "action_id": value.action_id,
                "approval_id": value.approval_id,
                "decision": value.decision,
                "decided_by": value.decided_by,
            }),
            "type.googleapis.com/model_plane.v1.BrowserActionDecided",
        ),
        Some(orchestration_event::Event::ApprovalContinuationVerified(value)) => (
            "APPROVAL_CONTINUATION_VERIFIED",
            value.run_id.as_str(),
            {
                let verification = value.verification.as_ref();
                serde_json::json!({
                    "run_id": value.run_id,
                    "delivery_id": value.delivery_id,
                    "approval_id": value.approval_id,
                    "receipt_id": value.receipt_id,
                    "verification": verification.map(|result| serde_json::json!({
                        "status": result.status,
                        "method": result.method,
                        "reason": result.reason,
                    })),
                })
            },
            "type.googleapis.com/model_plane.v1.ApprovalContinuationVerified",
        ),
        _ => return Ok(()),
    };
    if run_id.trim().is_empty() {
        return Ok(());
    }
    let timestamp = event.at.as_ref().and_then(|value| {
        DateTime::<Utc>::from_timestamp(value.seconds, u32::try_from(value.nanos).ok()?)
    });
    let resource_ref = format!("run:{run_id}");
    let idempotency_key = format!("orchestration:{event_id}");
    sqlx::query(
        "INSERT INTO events (id, event_type, run_id, payload, ts, org_id, user_id, correlation_id, causation_id, idempotency_key, resource_ref, type_url, producer, schema_version)
         SELECT $1, $2, r.id, $3, COALESCE($4, now()), r.org_id, r.user_id, $5, '', $6, $7, $8, 'session-core', 1
         FROM runs r
         WHERE r.id = $9
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(event_id)
    .bind(event_type)
    .bind(payload)
    .bind(timestamp)
    .bind(run_id)
    .bind(idempotency_key)
    .bind(resource_ref)
    .bind(type_url)
    .bind(run_id)
    .execute(pool)
    .await?;
    Ok(())
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
        orchestration_event::Event::TodoTransitioned(_) => None,
        // A stop is run-scoped only when it names the run that delegated the
        // child (the wide-research live panel's per-item cards leave "running"
        // on it). Before that field existed a stop named no run at all, so it
        // was never deliverable on any run's stream — and an event recorded
        // without it still is not.
        orchestration_event::Event::SubagentStopped(p) => {
            (!p.parent_run_id.is_empty()).then_some(p.parent_run_id.as_str())
        }
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
            // `thread_id` lists across run boundaries for a reader that has a
            // conversation but no run: a model-proposed plan is proposed and
            // approved on a DIFFERENT (earlier, plan-mode) run of the thread
            // than the one that consumes it, so a run_id-scoped lookup cannot
            // find it. (The gateway's own hand-over of an approved plan does
            // not list: it fetches the one approved plan by id.)
            let plan_rows = if !req.run_id.is_empty() {
                authorize_run_owner(&self.pool, &caller, &req.run_id, OwnerIntent::Read).await?;
                store::list_plans_by_run(&self.pool, &req.run_id)
                    .await
                    .map_err(|e| Status::internal(e.to_string()))?
            } else if !req.thread_id.is_empty() {
                authorize_thread_owner(&self.pool, &caller, &req.thread_id, OwnerIntent::Read)
                    .await?;
                store::list_plans_by_thread_full(&self.pool, &req.thread_id)
                    .await
                    .map_err(|e| Status::internal(e.to_string()))?
            } else {
                return Err(Status::invalid_argument(
                    "either run_id or thread_id is required",
                ));
            };
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

            validate_plan_transition_legality(
                prev_state,
                req.target_state,
                !req.edited_steps.is_empty(),
            )?;

            let resolved = if req.edited_steps.is_empty() {
                Vec::new()
            } else {
                let existing = store::list_steps_by_plan_full(&self.pool, &req.plan_id)
                    .await
                    .map_err(|e| Status::internal(e.to_string()))?;
                let existing_ids: Vec<&str> = existing.iter().map(|s| s.id.as_str()).collect();
                resolve_plan_step_edits(
                    &existing_ids,
                    &req.edited_steps,
                    req.target_state == proto::PlanState::Approved as i32,
                    || format!("step_{}", mp_ids::new_ulid()),
                )?
            };

            // One compare-and-swap: the edits and the new status are written
            // together, under a row lock, and only if the plan is STILL in
            // the state the legality check above was made against. Two
            // racing approvals (or an edit arriving after an approval) can
            // no longer both pass — the loser writes nothing.
            match store::transition_plan_atomic(
                &self.pool,
                &req.plan_id,
                &before.status,
                new_status,
                &resolved,
                &plan_decision_metadata(new_status, &req.actor, &req.reason),
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            {
                store::PlanTransitionOutcome::Applied => {}
                store::PlanTransitionOutcome::Conflict { actual } => {
                    return Err(Status::failed_precondition(format!(
                        "the plan was decided by another request first (it is now {})",
                        actual.as_deref().unwrap_or("gone")
                    )));
                }
            }

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

    /// plan-gate §2.1-2.2: create a plan directly in `PLAN_STATE_PROPOSED`
    /// with real steps, called by execution-core's `plan.propose` control
    /// tool. Distinct from the zero-step `draft` plan auto-created at run
    /// start for the retrospective execution log — this is a SEPARATE plan
    /// row a user actually reviews, so it gets its own ULID-based id rather
    /// than the deterministic `plan_<run_id>` the run-start plan uses.
    async fn propose_plan(
        &self,
        request: Request<proto::ProposePlanRequest>,
    ) -> Result<Response<proto::ProposePlanResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<proto::ProposePlanResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "orchestration:write")?;
            let req = request.into_inner();
            if req.thread_id.is_empty() {
                return Err(Status::invalid_argument("thread_id is required"));
            }
            if req.org_id.is_empty() || req.user_id.is_empty() {
                return Err(Status::invalid_argument("org_id and user_id are required"));
            }
            validate_proposed_plan_input(&req.summary, &req.steps, &req.supersedes)?;
            // Owner check against the THREAD's own durable owner row, not the
            // request body's claimed org_id/user_id — the same defense the
            // sibling `attach_subagent` RPC uses, so a caller cannot forge
            // tenancy by lying in the request.
            authorize_thread_owner(&self.pool, &caller, &req.thread_id, OwnerIntent::Mutate)
                .await?;
            caller.authorize_org(&req.org_id)?;
            // `user_id` becomes the plan's OWNER column, and owner is what
            // every later read and transition (including the approval that
            // grants the run authority) is checked against. Left as the
            // request body's claim, a user-bound caller could author a plan
            // owned by someone else. A user credential therefore has to BE the
            // user it names; a service caller (execution-core, acting for the
            // run's user) is trusted to name it, as everywhere else in this
            // service.
            if !caller.is_service() {
                caller.authorize_user(&req.user_id)?;
            }
            // The run the plan hangs off must be a run OF this thread: the FK
            // only proves the run exists, so without this a plan could be
            // attached to (and listed under) another conversation's run.
            if !req.run_id.is_empty() {
                let run_thread: Option<(String,)> =
                    sqlx::query_as("SELECT thread_id FROM runs WHERE id = $1")
                        .bind(&req.run_id)
                        .fetch_optional(&self.pool)
                        .await
                        .map_err(|e| Status::internal(e.to_string()))?;
                match run_thread {
                    Some((thread_id,)) if thread_id == req.thread_id => {}
                    Some(_) => {
                        return Err(Status::permission_denied(
                            "run_id does not belong to thread_id",
                        ));
                    }
                    None => return Err(Status::not_found("run not found")),
                }
            }

            let plan_id = format!("plan_{}", mp_ids::new_ulid());
            // `metadata.author` records the caller's stated author label
            // ("agent" for a model-proposed plan). It is deliberately NOT
            // written into the `user_id` column: that column doubles as the
            // plan's owner for `authorize_plan_owner` (every read/transition
            // check), so it must hold the real end user, exactly like every
            // other plan today (`create_plan`'s `author: row.user_id.clone()`
            // already reflects this — `Plan.author` has never meant "who
            // authored the text", only "who owns it").
            let mut metadata = serde_json::Map::new();
            if !req.author.is_empty() {
                metadata.insert(
                    "author".to_owned(),
                    JsonValue::String(req.author.chars().take(64).collect()),
                );
            }
            if !req.supersedes.is_empty() {
                metadata.insert(
                    "supersedes".to_owned(),
                    JsonValue::String(req.supersedes.clone()),
                );
            }
            let steps: Vec<(String, String, String)> = req
                .steps
                .iter()
                .map(|s| {
                    (
                        format!("step_{}", mp_ids::new_ulid()),
                        s.title.trim().to_owned(),
                        s.operation.trim().to_owned(),
                    )
                })
                .collect();

            store::create_proposed_plan(
                &self.pool,
                &plan_id,
                &req.thread_id,
                (!req.run_id.is_empty()).then_some(req.run_id.as_str()),
                &req.summary,
                &req.org_id,
                &req.user_id,
                &steps,
                &JsonValue::Object(metadata),
                (!req.supersedes.is_empty()).then_some(req.supersedes.as_str()),
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

            let row = store::get_plan(&self.pool, &plan_id)
                .await
                .map_err(|e| Status::internal(e.to_string()))?
                .ok_or_else(|| Status::internal("plan vanished after insert"))?;
            let plan = assemble_plan(&self.pool, &row)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;

            // Same live-visibility wiring `transition_plan` already gives
            // every state change, so a run-console panel or future live
            // listener sees a proposed plan the moment it exists, not only
            // once it is later approved or rejected.
            broadcast_event(
                &self.events_tx,
                &self.replay,
                proto::OrchestrationEvent {
                    event_id: String::new(),
                    at: Some(now_ts()),
                    event: Some(orchestration_event::Event::PlanTransitioned(
                        orchestration_event::PlanTransitioned {
                            plan_id: plan.id.clone(),
                            run_id: plan.run_id.clone(),
                            from: proto::PlanState::Unspecified as i32,
                            to: proto::PlanState::Proposed as i32,
                        },
                    )),
                },
            );

            Ok(Response::new(proto::ProposePlanResponse { plan: Some(plan) }))
        }
        .await;
        record_metrics("propose_plan", started, result.is_ok());
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

    async fn get_verification_metrics(
        &self,
        request: Request<proto::GetVerificationMetricsRequest>,
    ) -> Result<Response<proto::GetVerificationMetricsResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<proto::GetVerificationMetricsResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "orchestration:read")?;
            let req = request.into_inner();
            validate_approval_org(&req.org_id)?;
            caller.authorize_org(&req.org_id)?;
            let since = req
                .since
                .map(|since| {
                    prost_timestamp_to_chrono(since)
                        .ok_or_else(|| Status::invalid_argument("since is not a valid timestamp"))
                })
                .transpose()?;

            let continuations =
                store::list_continuation_metrics_rows_for_org(&self.pool, &req.org_id, since)
                    .await
                    .map_err(|e| Status::internal(e.to_string()))?;
            let approvals =
                store::list_approval_metrics_rows_for_org(&self.pool, &req.org_id, since)
                    .await
                    .map_err(|e| Status::internal(e.to_string()))?;

            let metrics =
                build_verification_metrics(&req.org_id, since, &continuations, &approvals);
            Ok(Response::new(proto::GetVerificationMetricsResponse {
                metrics: Some(metrics),
            }))
        }
        .await;
        record_metrics("get_verification_metrics", started, result.is_ok());
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
                continuation_descriptor.as_ref(),
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
                let verification_status =
                    req.verification.as_ref().and_then(
                        |v| match proto::VerificationStatus::try_from(v.status) {
                            Ok(proto::VerificationStatus::Unknown) => Some("unknown"),
                            Ok(proto::VerificationStatus::VerifiedSuccess) => {
                                Some("verified_success")
                            }
                            Ok(proto::VerificationStatus::VerifiedFailure) => {
                                Some("verified_failure")
                            }
                            Ok(proto::VerificationStatus::PartiallyVerified) => {
                                Some("partially_verified")
                            }
                            Ok(proto::VerificationStatus::Unspecified) | Err(_) => None,
                        },
                    );
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

            // `goal_summary`/`batch_id` (wide-research fan-out §2.4) have no
            // dedicated columns yet — reusing the existing `metadata` JSONB
            // column `attach_subagent` already accepted (previously always
            // an empty object) avoids a migration for two additive, optional
            // fields. `LineageEdge`/`ListChildren` readback is unchanged in
            // this pass (not needed by the live run panel, which reads the
            // broadcast event below); only the live SSE path carries them.
            store::attach_subagent(
                &self.pool,
                &req.parent_run_id,
                &req.child_run_id,
                role_str,
                &subagent_edge_metadata(&req.goal_summary, &req.batch_id),
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
                            goal_summary: req.goal_summary.clone(),
                            batch_id: req.batch_id.clone(),
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
            let event_id = broadcast_event(&self.events_tx, &self.replay, ev.clone());
            // Browser-agent progress is emitted through this additive RPC and
            // therefore does not pass through the normal NATS event consumer.
            // Persist its safe projection before the RPC returns so a later
            // reload can recover the same Work evidence. The browser executor
            // treats this bridge as best-effort, so a storage outage is logged
            // while the live event remains visible to already-connected tabs.
            if let Err(error) = persist_browser_event(&self.pool, &event_id, &ev).await {
                warn!(%error, %event_id, "failed to persist browser orchestration event");
            }
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
            expires_in_seconds: 3600,
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

        let no_expiry = proto::CreateApprovalRequest {
            expires_in_seconds: 0,
            ..req
        };
        assert_eq!(
            validate_continuation_descriptor(&valid, &no_expiry)
                .expect_err("descriptor retention must have a finite expiry")
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

    // -----------------------------------------------------------------
    // plan-gate: transition legality + step-edit id resolution
    // -----------------------------------------------------------------

    #[test]
    fn proposed_to_approved_with_edits_is_accepted() {
        assert!(validate_plan_transition_legality(
            proto::PlanState::Proposed as i32,
            proto::PlanState::Approved as i32,
            true,
        )
        .is_ok());
    }

    #[test]
    fn approved_from_any_state_other_than_proposed_is_refused() {
        for other in [
            proto::PlanState::Draft,
            proto::PlanState::Executing,
            proto::PlanState::Completed,
            proto::PlanState::Rejected,
            proto::PlanState::Superseded,
        ] {
            let error = validate_plan_transition_legality(
                other as i32,
                proto::PlanState::Approved as i32,
                false,
            )
            .expect_err(&format!("{other:?} -> APPROVED must be refused"));
            assert_eq!(error.code(), tonic::Code::FailedPrecondition);
        }
    }

    #[test]
    fn executing_and_completed_plans_reject_further_edited_steps() {
        for terminalish in [proto::PlanState::Executing, proto::PlanState::Completed] {
            let error = validate_plan_transition_legality(
                terminalish as i32,
                // Target state does not matter here — the edit itself is
                // what must be refused, e.g. an attempt to append edits
                // while also (re)rejecting an already-decided plan.
                proto::PlanState::Rejected as i32,
                true,
            )
            .expect_err(&format!("{terminalish:?} must refuse edited_steps"));
            assert_eq!(error.code(), tonic::Code::FailedPrecondition);
        }
    }

    #[test]
    fn edited_steps_are_refused_on_a_move_the_lifecycle_would_otherwise_allow() {
        // The test above cannot see the edit guard: every move it tries
        // (EXECUTING/COMPLETED -> REJECTED) is refused by the lifecycle table
        // anyway, so deleting the guard leaves it green. These are the moves the
        // table DOES allow. With edits attached they must still be refused, or
        // an already-approved plan's steps could be rewritten under the rung
        // that was granted against the steps as approved (the claim
        // APPROVED -> EXECUTING is the one such move the gateway makes itself).
        for (from, to) in [
            (proto::PlanState::Approved, proto::PlanState::Executing),
            (proto::PlanState::Approved, proto::PlanState::Superseded),
            (proto::PlanState::Executing, proto::PlanState::Completed),
            (proto::PlanState::Executing, proto::PlanState::Failed),
        ] {
            assert!(
                validate_plan_transition_legality(from as i32, to as i32, false).is_ok(),
                "{from:?} -> {to:?} is a legal move without edits"
            );
            let error = validate_plan_transition_legality(from as i32, to as i32, true)
                .expect_err(&format!("{from:?} -> {to:?} must refuse edited_steps"));
            assert_eq!(error.code(), tonic::Code::FailedPrecondition);
            assert!(
                error.message().contains("edited_steps"),
                "the refusal must name the edits, not the transition: {}",
                error.message()
            );
        }
    }

    #[test]
    fn proposed_to_rejected_needs_no_edits_and_is_accepted() {
        assert!(validate_plan_transition_legality(
            proto::PlanState::Proposed as i32,
            proto::PlanState::Rejected as i32,
            false,
        )
        .is_ok());
    }

    fn edit(step_id: &str, title: &str, removed: bool) -> proto::PlanStepEdit {
        proto::PlanStepEdit {
            step_id: step_id.to_owned(),
            title: title.to_owned(),
            operation: String::new(),
            removed,
        }
    }

    fn resolve(
        existing: &[&str],
        edits: &[proto::PlanStepEdit],
        require_remaining: bool,
    ) -> Result<Vec<store::PlanStepEditRow>, Status> {
        let mut minted = 0;
        resolve_plan_step_edits(existing, edits, require_remaining, || {
            minted += 1;
            format!("step_new_{minted}")
        })
    }

    #[test]
    fn plan_step_edits_resolve_new_vs_existing_ids_in_array_order() {
        let rows = resolve(
            &["step_1", "step_2"],
            &[
                edit("step_2", " Send it ", false),
                edit("", "Follow up", false),
                edit("step_1", "Draft the email", true),
            ],
            true,
        )
        .expect("a complete, well-formed edit set resolves");
        let shape: Vec<(&str, bool, &str, bool)> = rows
            .iter()
            .map(|r| (r.id.as_str(), r.is_new, r.title.as_str(), r.removed))
            .collect();
        assert_eq!(
            shape,
            vec![
                // Existing step keeps its id; the title is trimmed.
                ("step_2", false, "Send it", false),
                // An empty step_id is a NEW step and gets a minted id.
                ("step_new_1", true, "Follow up", false),
                // A removed step stays in the list (marked, never dropped).
                ("step_1", false, "Draft the email", true),
            ]
        );
    }

    #[test]
    fn plan_step_edits_refuse_an_unknown_step_id_instead_of_inserting_it() {
        // Before: a stale/foreign id was silently treated as "new", so a
        // browser holding the steps of a different or superseded plan turned
        // into extra steps in this one.
        let error = resolve(
            &["step_1"],
            &[edit("step_1", "Keep", false), edit("step_other_plan", "Ghost", false)],
            true,
        )
        .expect_err("an id that is not in this plan must be refused");
        assert_eq!(error.code(), tonic::Code::InvalidArgument);
    }

    #[test]
    fn plan_step_edits_must_list_every_existing_step() {
        // Before: an omitted step kept its old ordinal and stayed `pending` —
        // still in the plan the executing run is told was approved — or
        // collided with an edited step's new ordinal and 500'd the approval.
        let error = resolve(&["step_1", "step_2"], &[edit("step_2", "Only this", false)], true)
            .expect_err("a step missing from the edit set must be refused");
        assert_eq!(error.code(), tonic::Code::InvalidArgument);
        assert!(error.message().contains("step_1"), "{}", error.message());
    }

    #[test]
    fn plan_step_edits_refuse_a_step_listed_twice() {
        let error = resolve(
            &["step_1"],
            &[edit("step_1", "One", false), edit("step_1", "Again", false)],
            true,
        )
        .expect_err("the same step twice must be refused");
        assert_eq!(error.code(), tonic::Code::InvalidArgument);
    }

    #[test]
    fn plan_step_edits_refuse_blank_titles_on_kept_steps() {
        assert!(resolve(&["step_1"], &[edit("step_1", "   ", false)], true).is_err());
        assert!(resolve(&["step_1"], &[edit("step_1", "Keep", false), edit("", "", false)], true)
            .is_err());
        // A REMOVED step's title is irrelevant — it is being decided against.
        assert!(resolve(
            &["step_1", "step_2"],
            &[edit("step_1", "", true), edit("step_2", "Keep", false)],
            true
        )
        .is_ok());
    }

    #[test]
    fn plan_step_edits_refuse_an_approval_with_every_step_removed() {
        let all_removed = [edit("step_1", "A", true), edit("step_2", "B", true)];
        let error = resolve(&["step_1", "step_2"], &all_removed, true)
            .expect_err("approving a plan with nothing left to do is a rubber stamp");
        assert_eq!(error.code(), tonic::Code::InvalidArgument);
        // Not an approval (e.g. a reject that carries edits): nothing to guard.
        assert!(resolve(&["step_1", "step_2"], &all_removed, false).is_ok());
    }

    #[test]
    fn plan_step_edits_drop_a_step_added_and_removed_in_one_session() {
        let rows = resolve(
            &["step_1"],
            &[edit("step_1", "Keep", false), edit("", "Never real", true)],
            true,
        )
        .expect("resolves");
        assert_eq!(rows.len(), 1, "a never-proposed step leaves no audit row");
        assert_eq!(rows[0].id, "step_1");
    }

    #[test]
    fn plan_step_edits_are_bounded() {
        let too_many: Vec<proto::PlanStepEdit> = (0..=MAX_PLAN_STEP_EDITS)
            .map(|i| edit("", &format!("step {i}"), false))
            .collect();
        assert!(resolve(&[], &too_many, false).is_err());
        let long = "x".repeat(MAX_PLAN_TITLE_CHARS + 1);
        assert!(resolve(&["step_1"], &[edit("step_1", &long, false)], true).is_err());
    }

    fn proposed(title: &str) -> proto::ProposedPlanStep {
        proto::ProposedPlanStep {
            title: title.to_owned(),
            operation: String::new(),
        }
    }

    #[test]
    fn proposed_plan_input_is_bounded_and_complete() {
        assert!(validate_proposed_plan_input("Send it", &[proposed("Draft")], "").is_ok());
        assert!(validate_proposed_plan_input("  ", &[proposed("Draft")], "").is_err());
        assert!(validate_proposed_plan_input("Send it", &[], "").is_err());
        assert!(validate_proposed_plan_input("Send it", &[proposed("  ")], "").is_err());
        let many: Vec<_> = (0..=MAX_PLAN_STEPS).map(|i| proposed(&format!("s{i}"))).collect();
        assert!(validate_proposed_plan_input("Send it", &many, "").is_err());
        let long_title = proposed(&"x".repeat(MAX_PLAN_TITLE_CHARS + 1));
        assert!(validate_proposed_plan_input("Send it", &[long_title], "").is_err());
        let long_summary = "y".repeat(MAX_PLAN_SUMMARY_CHARS + 1);
        assert!(validate_proposed_plan_input(&long_summary, &[proposed("Draft")], "").is_err());
        let long_ref = "z".repeat(MAX_PLAN_SUPERSEDES_CHARS + 1);
        assert!(validate_proposed_plan_input("Send it", &[proposed("Draft")], &long_ref).is_err());
    }

    #[test]
    fn a_decided_plan_cannot_be_decided_again() {
        // Rejecting is a decision on a PROPOSED plan. It used to be accepted
        // from any state, so an approved (granted, being executed) or already
        // completed plan could be flipped to REJECTED afterwards.
        for decided in [
            proto::PlanState::Approved,
            proto::PlanState::Executing,
            proto::PlanState::Completed,
            proto::PlanState::Rejected,
            proto::PlanState::Superseded,
        ] {
            let error = validate_plan_transition_legality(
                decided as i32,
                proto::PlanState::Rejected as i32,
                false,
            )
            .expect_err(&format!("{decided:?} -> REJECTED must be refused"));
            assert_eq!(error.code(), tonic::Code::FailedPrecondition);
        }
    }

    #[test]
    fn a_rejected_plan_cannot_be_revived() {
        // Owner ruling (2026-09-27): no revise-after-reject in this wave.
        for revived in [
            proto::PlanState::Proposed,
            proto::PlanState::Draft,
            proto::PlanState::Executing,
        ] {
            assert!(
                validate_plan_transition_legality(
                    proto::PlanState::Rejected as i32,
                    revived as i32,
                    false,
                )
                .is_err(),
                "REJECTED -> {revived:?} must be refused"
            );
        }
    }

    #[test]
    fn the_lifecycle_transitions_the_gate_relies_on_stay_legal() {
        // propose -> approve, propose -> reject, propose -> supersede (the
        // model revising itself) and approve -> executing (consumption by
        // the next run) are the moves plan-gate itself makes.
        for (from, to) in [
            (proto::PlanState::Proposed, proto::PlanState::Approved),
            (proto::PlanState::Proposed, proto::PlanState::Rejected),
            (proto::PlanState::Proposed, proto::PlanState::Superseded),
            (proto::PlanState::Approved, proto::PlanState::Executing),
            (proto::PlanState::Approved, proto::PlanState::Superseded),
        ] {
            assert!(
                validate_plan_transition_legality(from as i32, to as i32, false).is_ok(),
                "{from:?} -> {to:?} must stay legal"
            );
        }
    }

    #[test]
    fn a_decision_leaves_a_per_state_receipt_on_the_plan() {
        let approved = plan_decision_metadata("approved", "user_42", "  writes the report  ");
        assert_eq!(approved["approved_by"], "user_42");
        assert_eq!(approved["approved_reason"], "writes the report");
        // The later EXECUTING claim must not overwrite the approval's receipt.
        let executing = plan_decision_metadata("executing", "user_42", "consumed by the run");
        assert!(executing.get("approved_by").is_none());
        assert_eq!(executing["executing_by"], "user_42");
        // Nothing to say is nothing written — no empty-string keys.
        let bare = plan_decision_metadata("rejected", "", "   ");
        assert_eq!(bare, serde_json::json!({}));
        // Free text is bounded.
        let long = plan_decision_metadata("rejected", "u", &"r".repeat(5_000));
        assert_eq!(long["rejected_reason"].as_str().map(str::len), Some(1_000));
    }

    #[test]
    fn a_transition_to_the_same_state_is_refused_so_consumption_is_single_use() {
        // approved_plan_context marks a plan EXECUTING before injecting it; a
        // second run racing for the same plan must lose that transition.
        assert!(validate_plan_transition_legality(
            proto::PlanState::Executing as i32,
            proto::PlanState::Executing as i32,
            false,
        )
        .is_err());
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
    fn a_subagent_edge_persists_its_goal_and_batch_only_when_set() {
        assert_eq!(
            subagent_edge_metadata("look up carrier docs", "run-1:tool_1_fan-1"),
            serde_json::json!({
                "goal_summary": "look up carrier docs",
                "batch_id": "run-1:tool_1_fan-1",
            })
        );
        // A standalone `subagent.task` has no batch to group under.
        assert_eq!(
            subagent_edge_metadata("summarise Q3", ""),
            serde_json::json!({ "goal_summary": "summarise Q3" })
        );
        // A ZDR run (execution-core blanks the goal) or a caller that predates
        // the fields stores the empty object it always did — no empty strings.
        assert_eq!(subagent_edge_metadata("", ""), serde_json::json!({}));
        assert_eq!(
            subagent_edge_metadata("", "run-1:tool_1_fan-1"),
            serde_json::json!({ "batch_id": "run-1:tool_1_fan-1" })
        );
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
                    parent_run_id: "run_p".into(),
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

    /// The end-to-end shape of the fix: a stop that names its parent lands in
    /// THAT run's replay buffer (and so on its live stream), and a legacy one
    /// with no parent lands nowhere — the state every stop was in before the
    /// field existed, which is why no card could ever leave "running".
    #[tokio::test]
    async fn a_subagent_stop_reaches_its_parent_runs_stream_only_when_it_names_it() {
        let (tx, _rx) = broadcast::channel::<proto::OrchestrationEvent>(8);
        let replay = ReplayBuffer::default();
        let stop = |parent: &str| proto::OrchestrationEvent {
            event_id: String::new(),
            at: Some(now_ts()),
            event: Some(orchestration_event::Event::SubagentStopped(
                orchestration_event::SubagentStopped {
                    child_run_id: "run_c".into(),
                    status: "completed".into(),
                    parent_run_id: parent.into(),
                },
            )),
        };

        broadcast_event(&tx, &replay, stop("run_p"));
        broadcast_event(&tx, &replay, stop(""));

        let buffered = replay.replay_after("run_p", "").await;
        assert_eq!(buffered.len(), 1, "only the stop that named a parent is routed");
        assert!(
            replay.replay_after("", "").await.is_empty(),
            "an unparented stop is never buffered under an empty run key"
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
                    goal_summary: String::new(),
                    batch_id: String::new(),
                },
            )),
        };
        assert_eq!(event_run_id(&attach), Some("run_p"));

        // A stop that names the run that delegated the child is run-scoped, so
        // it reaches that run's stream (wide-research live panel cards).
        let stopped = proto::OrchestrationEvent {
            event_id: String::new(),
            at: None,
            event: Some(orchestration_event::Event::SubagentStopped(
                orchestration_event::SubagentStopped {
                    child_run_id: "run_c".into(),
                    status: "completed".into(),
                    parent_run_id: "run_p".into(),
                },
            )),
        };
        assert_eq!(event_run_id(&stopped), Some("run_p"));

        // One recorded without a parent (before the field existed) names no run
        // and stays undeliverable per run — never routed under an empty key.
        let stopped_unparented = proto::OrchestrationEvent {
            event_id: String::new(),
            at: None,
            event: Some(orchestration_event::Event::SubagentStopped(
                orchestration_event::SubagentStopped {
                    child_run_id: "run_c".into(),
                    status: "completed".into(),
                    parent_run_id: String::new(),
                },
            )),
        };
        assert_eq!(event_run_id(&stopped_unparented), None);

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

    // --- Verification metrics (roadmap P1 item 5) -------------------------

    fn metrics_row(
        outcome: &str,
        verification_status: Option<&str>,
        verification_method: Option<&str>,
        elapsed_secs: i64,
    ) -> store::ContinuationMetricsRow {
        let started_at = chrono::Utc::now() - chrono::Duration::seconds(elapsed_secs);
        store::ContinuationMetricsRow {
            outcome: outcome.to_owned(),
            verification_status: verification_status.map(str::to_owned),
            verification_method: verification_method.map(str::to_owned),
            started_at,
            finalized_at: started_at + chrono::Duration::seconds(elapsed_secs),
        }
    }

    fn approval_row(
        status: &str,
        requested_secs_ago: i64,
        decision_lag_secs: Option<i64>,
    ) -> store::ApprovalMetricsRow {
        let requested_at = chrono::Utc::now() - chrono::Duration::seconds(requested_secs_ago);
        store::ApprovalMetricsRow {
            status: status.to_owned(),
            requested_at,
            decided_at: decision_lag_secs.map(|lag| requested_at + chrono::Duration::seconds(lag)),
        }
    }

    #[test]
    fn median_seconds_handles_empty_single_even_and_odd() {
        assert_eq!(median_seconds(std::iter::empty()), 0);
        assert_eq!(median_seconds([7].into_iter()), 7);
        assert_eq!(median_seconds([10, 20].into_iter()), 15);
        assert_eq!(median_seconds([1, 100, 4].into_iter()), 4);
    }

    #[test]
    fn median_seconds_ignores_a_single_slow_outlier_more_than_a_mean_would() {
        let with_outlier = median_seconds([10, 12, 11, 9, 10_000].into_iter());
        assert_eq!(
            with_outlier, 11,
            "the outlier must not drag the median toward it"
        );
    }

    #[test]
    fn false_success_counts_only_completed_rows_refuted_by_postcondition() {
        let rows = vec![
            metrics_row(
                "completed",
                Some("verified_failure"),
                Some("postcondition"),
                5,
            ),
            // A genuinely failed continuation reporting verified_failure is an
            // honest structural failure the postcondition layer agreed with -
            // not a false success, since nothing claimed success here.
            metrics_row("failed", Some("verified_failure"), Some("postcondition"), 5),
            metrics_row(
                "completed",
                Some("verified_success"),
                Some("postcondition"),
                5,
            ),
        ];
        let metrics = build_verification_metrics("org-1", None, &rows, &[]);
        assert_eq!(
            metrics.false_success_count, 1,
            "only the completed+refuted row is a false success"
        );
        assert_eq!(metrics.verified_success_count, 1);
    }

    #[test]
    fn unverified_and_unknown_are_distinct_buckets() {
        let rows = vec![
            // NULL columns: predates instrumentation.
            metrics_row("completed", None, None, 1),
            // Recorded, but the dispatcher's own honest "nothing stronger to
            // show" - a live row, not a historical gap.
            metrics_row("completed", Some("unknown"), Some("structural"), 1),
        ];
        let metrics = build_verification_metrics("org-1", None, &rows, &[]);
        assert_eq!(metrics.unverified_count, 1);
        assert_eq!(metrics.unknown_verification_count, 1);
    }

    #[test]
    fn totals_and_method_counts_partition_by_their_own_field() {
        let rows = vec![
            metrics_row(
                "completed",
                Some("verified_success"),
                Some("postcondition"),
                1,
            ),
            metrics_row("failed", Some("unknown"), Some("structural"), 1),
            metrics_row("cancelled", None, None, 1),
        ];
        let metrics = build_verification_metrics("org-1", None, &rows, &[]);
        assert_eq!(metrics.total_completed, 1);
        assert_eq!(metrics.total_failed, 1);
        assert_eq!(metrics.total_cancelled, 1);
        assert_eq!(metrics.structural_method_count, 1);
        assert_eq!(metrics.postcondition_method_count, 1);
    }

    #[test]
    fn approval_counts_and_median_decision_latency_are_correct() {
        let approvals = vec![
            approval_row("requested", 100, None), // still pending, no decision yet
            approval_row("granted", 100, Some(10)),
            approval_row("granted", 100, Some(30)),
            approval_row("denied", 100, Some(20)),
        ];
        let metrics = build_verification_metrics("org-1", None, &[], &approvals);
        assert_eq!(
            metrics.approvals_requested, 4,
            "the pending one still counts as requested"
        );
        assert_eq!(metrics.approvals_granted, 2);
        assert_eq!(metrics.approvals_denied, 1);
        assert_eq!(
            metrics.median_seconds_approval_decision, 20,
            "the still-pending approval (no decided_at) must not enter the latency calculation"
        );
    }

    #[test]
    fn median_time_to_outcome_uses_receipt_start_not_query_time() {
        let rows = vec![
            metrics_row(
                "completed",
                Some("verified_success"),
                Some("postcondition"),
                4,
            ),
            metrics_row(
                "completed",
                Some("verified_success"),
                Some("postcondition"),
                8,
            ),
        ];
        let metrics = build_verification_metrics("org-1", None, &rows, &[]);
        assert_eq!(metrics.median_seconds_receipt_to_outcome, 6);
    }

    #[test]
    fn unavailable_names_exactly_the_dimensions_this_plane_cannot_prove() {
        let metrics = build_verification_metrics("org-1", None, &[], &[]);
        let sections: Vec<&str> = metrics
            .unavailable
            .iter()
            .map(|s| s.section.as_str())
            .collect();
        assert_eq!(
            sections,
            vec![
                "cost_per_verified_outcome",
                "unnecessary_approval",
                "rollback_rate"
            ]
        );
        assert!(metrics.unavailable.iter().all(|s| !s.reason.is_empty()));
    }

    #[test]
    fn empty_input_is_all_zero_not_an_error() {
        let metrics = build_verification_metrics("org-1", None, &[], &[]);
        assert_eq!(metrics.total_completed, 0);
        assert_eq!(metrics.approvals_requested, 0);
        assert_eq!(metrics.median_seconds_receipt_to_outcome, 0);
        assert_eq!(metrics.org_id, "org-1");
    }

    // =====================================================================
    // plan-gate against a REAL Postgres (chat gap-closure plan, task B10)
    // =====================================================================
    //
    // Everything above proves the plan gate's rules against fixtures. The SQL
    // that enforces them -- the `FOR UPDATE` compare-and-swap in
    // `transition_plan_atomic`, the two-pass ordinal parking in
    // `apply_plan_edits`, the supersede statements in `create_proposed_plan`
    // and the `(plan_id, ordinal)` unique index -- never ran against Postgres.
    // These tests call the real gRPC handlers on a real database.
    //
    // They are #[ignore]d so a plain `cargo test` (no database) skips them. Run
    // them against a SCRATCH database, never the live dev one:
    //
    //   DATABASE_URL=postgres://.../plan_gate_test \
    //     cargo test -p session-core --bin session-core plan_gate -- --ignored
    //
    // Every test works under its own org, thread and run ids, so they run in
    // parallel on one database; the harness refuses a database whose name does
    // not contain "test", because the tests write rows.
    mod plan_gate_pg {
        use super::*;
        use std::str::FromStr as _;
        use std::time::Duration;

        /// `(ordinal, id, status, title, operation)` of one `plan_steps` row.
        type StepRow = (i64, String, String, String, String);

        fn scratch_database_url() -> String {
            let url = std::env::var("DATABASE_URL")
                .expect("DATABASE_URL must point at a scratch Postgres, e.g. .../plan_gate_test");
            let database = url
                .split('?')
                .next()
                .unwrap_or_default()
                .rsplit('/')
                .next()
                .unwrap_or_default();
            assert!(
                database.contains("test"),
                "refusing to run the plan-gate tests against database {database:?}: they write \
                 rows. Use a scratch database whose name contains \"test\"."
            );
            url
        }

        fn lower_ulid() -> String {
            mp_ids::new_ulid().to_lowercase()
        }

        fn as_person<T>(org_id: &str, user_id: &str, message: T) -> Request<T> {
            let mut request = Request::new(message);
            request
                .extensions_mut()
                .insert(VerifiedIdentity::user_for_test(org_id, user_id));
            request
        }

        fn plan_edit(
            step_id: &str,
            title: &str,
            operation: &str,
            removed: bool,
        ) -> proto::PlanStepEdit {
            proto::PlanStepEdit {
                step_id: step_id.to_owned(),
                title: title.to_owned(),
                operation: operation.to_owned(),
                removed,
            }
        }

        /// One tenant's worth of rows plus the real service on a real pool.
        #[derive(Clone)]
        struct PlanGateDb {
            svc: Arc<OrchestrationGrpc>,
            pool: Pool,
            app_name: String,
            org: String,
            user: String,
            thread: String,
        }

        impl PlanGateDb {
            async fn connect() -> Self {
                let url = scratch_database_url();
                let tag = lower_ulid();
                let app_name = format!("plan_gate_{tag}");
                let options = sqlx::postgres::PgConnectOptions::from_str(&url)
                    .expect("DATABASE_URL parses")
                    .application_name(&app_name);
                let pool = sqlx::postgres::PgPoolOptions::new()
                    .max_connections(8)
                    .connect_with(options)
                    .await
                    .expect("connect pg");
                sqlx::migrate!("./migrations")
                    .run(&pool)
                    .await
                    .expect("migrate");
                let (events_tx, _events_rx) = broadcast::channel(64);
                let db = Self {
                    svc: Arc::new(OrchestrationGrpc::new(pool.clone(), events_tx)),
                    pool,
                    app_name,
                    org: format!("org-pg-{tag}"),
                    user: format!("user-pg-{tag}"),
                    thread: format!("thread-pg-{tag}"),
                };
                db.seed_thread(&db.thread).await;
                db
            }

            async fn seed_thread(&self, id: &str) {
                sqlx::query(
                    "INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, $3)",
                )
                .bind(id)
                .bind(&self.org)
                .bind(&self.user)
                .execute(&self.pool)
                .await
                .expect("seed thread");
            }

            async fn new_thread(&self) -> String {
                let id = format!("thread-pg-{}", lower_ulid());
                self.seed_thread(&id).await;
                id
            }

            async fn new_run(&self, thread: &str) -> String {
                let id = format!("run-pg-{}", lower_ulid());
                sqlx::query(
                    "INSERT INTO runs (id, thread_id, goal, org_id, user_id) VALUES ($1, $2, 'g', $3, $4)",
                )
                .bind(&id)
                .bind(thread)
                .bind(&self.org)
                .bind(&self.user)
                .execute(&self.pool)
                .await
                .expect("seed run");
                id
            }

            /// execution-core's identity, as `plan.propose` calls `ProposePlan`.
            fn as_exec_core<T>(&self, message: T) -> Request<T> {
                let mut request = Request::new(message);
                request
                    .extensions_mut()
                    .insert(VerifiedIdentity::service_for_test_as(
                        &self.org,
                        "service:execution-core",
                        &["orchestration:write"],
                        false,
                    ));
                request
            }

            /// The person who owns the thread, as the approving browser calls
            /// `TransitionPlan`.
            fn as_owner<T>(&self, message: T) -> Request<T> {
                as_person(&self.org, &self.user, message)
            }

            async fn try_propose(
                &self,
                thread: &str,
                run: &str,
                steps: &[(&str, &str)],
                supersedes: &str,
            ) -> Result<proto::Plan, Status> {
                let request = proto::ProposePlanRequest {
                    thread_id: thread.to_owned(),
                    run_id: run.to_owned(),
                    org_id: self.org.clone(),
                    user_id: self.user.clone(),
                    author: "agent".to_owned(),
                    summary: "Plan under test".to_owned(),
                    steps: steps
                        .iter()
                        .map(|(title, operation)| proto::ProposedPlanStep {
                            title: (*title).to_owned(),
                            operation: (*operation).to_owned(),
                        })
                        .collect(),
                    supersedes: supersedes.to_owned(),
                };
                self.svc
                    .propose_plan(self.as_exec_core(request))
                    .await
                    .map(|response| response.into_inner().plan.expect("plan in response"))
            }

            async fn propose_with_ops(
                &self,
                thread: &str,
                run: &str,
                steps: &[(&str, &str)],
                supersedes: &str,
            ) -> proto::Plan {
                self.try_propose(thread, run, steps, supersedes)
                    .await
                    .expect("ProposePlan")
            }

            async fn propose(
                &self,
                thread: &str,
                run: &str,
                titles: &[&str],
                supersedes: &str,
            ) -> proto::Plan {
                let steps: Vec<(&str, &str)> = titles.iter().map(|t| (*t, "")).collect();
                self.propose_with_ops(thread, run, &steps, supersedes).await
            }

            /// A plan on the owner's own thread with three untitled-operation
            /// steps ("Step one" .. "Step three").
            async fn propose_three(&self) -> proto::Plan {
                self.propose(
                    &self.thread.clone(),
                    "",
                    &["Step one", "Step two", "Step three"],
                    "",
                )
                .await
            }

            async fn decide(
                &self,
                plan_id: &str,
                target: proto::PlanState,
                reason: &str,
                edits: Vec<proto::PlanStepEdit>,
            ) -> Result<proto::Plan, Status> {
                let request = proto::TransitionPlanRequest {
                    plan_id: plan_id.to_owned(),
                    target_state: target as i32,
                    actor: "forged-actor".to_owned(),
                    reason: reason.to_owned(),
                    edited_steps: edits,
                };
                self.svc
                    .transition_plan(self.as_owner(request))
                    .await
                    .map(|response| response.into_inner().plan.expect("plan in response"))
            }

            async fn status_of(&self, plan_id: &str) -> String {
                sqlx::query_scalar("SELECT status FROM plans WHERE id = $1")
                    .bind(plan_id)
                    .fetch_one(&self.pool)
                    .await
                    .expect("plan status")
            }

            async fn metadata_of(&self, plan_id: &str) -> JsonValue {
                sqlx::query_scalar("SELECT metadata FROM plans WHERE id = $1")
                    .bind(plan_id)
                    .fetch_one(&self.pool)
                    .await
                    .expect("plan metadata")
            }

            async fn steps_of(&self, plan_id: &str) -> Vec<StepRow> {
                sqlx::query_as(
                    "SELECT ordinal, id, status, COALESCE(payload->>'title', ''), kind \
                     FROM plan_steps WHERE plan_id = $1 ORDER BY ordinal",
                )
                .bind(plan_id)
                .fetch_all(&self.pool)
                .await
                .expect("plan steps")
            }

            /// Take a row lock on the plan, exactly as a rival writer would, and
            /// keep it until the returned transaction is ended.
            async fn hold_plan_row(
                &self,
                plan_id: &str,
            ) -> sqlx::Transaction<'static, sqlx::Postgres> {
                let mut hold = self.pool.begin().await.expect("begin");
                sqlx::query("SELECT id FROM plans WHERE id = $1 FOR UPDATE")
                    .bind(plan_id)
                    .fetch_one(&mut *hold)
                    .await
                    .expect("lock plan row");
                hold
            }

            /// Wait until `expected` of this test's own backends are blocked on a
            /// row lock, so a race is provably queued on the lock rather than
            /// assumed to be.
            async fn wait_for_lock_waiters(&self, expected: i64) {
                for _ in 0..400 {
                    let waiting: i64 = sqlx::query_scalar(
                        "SELECT count(*) FROM pg_stat_activity \
                         WHERE application_name = $1 AND wait_event_type = 'Lock'",
                    )
                    .bind(&self.app_name)
                    .fetch_one(&self.pool)
                    .await
                    .expect("pg_stat_activity");
                    if waiting >= expected {
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
                panic!("{expected} statements never queued on the plan row lock");
            }

            async fn cleanup(&self) {
                for statement in [
                    "DELETE FROM plan_steps WHERE plan_id IN (SELECT id FROM plans WHERE org_id = $1)",
                    "DELETE FROM plans WHERE org_id = $1",
                    "DELETE FROM runs WHERE org_id = $1",
                    "DELETE FROM threads WHERE org_id = $1",
                ] {
                    sqlx::query(statement)
                        .bind(&self.org)
                        .execute(&self.pool)
                        .await
                        .expect("cleanup");
                }
            }
        }

        fn state(plan: &proto::Plan) -> i32 {
            plan.state
        }

        fn ordinals(rows: &[StepRow]) -> Vec<i64> {
            rows.iter().map(|row| row.0).collect()
        }

        /// A decision's outcome pair -> `(winner index, the loser's refusal)`.
        /// Anything but exactly one winner is the defect under test.
        fn single_winner(
            first: Result<proto::Plan, Status>,
            second: Result<proto::Plan, Status>,
        ) -> (usize, Status) {
            match (first, second) {
                (Ok(_), Err(refusal)) => (0, refusal),
                (Err(refusal), Ok(_)) => (1, refusal),
                (Ok(_), Ok(_)) => panic!(
                    "both racing decisions succeeded: the compare-and-swap let two requests \
                     decide one plan"
                ),
                (Err(a), Err(b)) => {
                    panic!("both racing decisions failed (first: {a}; second: {b})")
                }
            }
        }

        /// Fire two decisions on one plan at once. With `hold_lock` the plan row
        /// is locked first, so both handlers are certain to have READ the plan
        /// in its old state (a plain read is not blocked by a row lock) and to be
        /// queued on the `FOR UPDATE` before either can commit: the loser must
        /// then be stopped by the status re-check under the lock, not by the
        /// handler's earlier legality check.
        async fn race(
            db: &PlanGateDb,
            plan_id: &str,
            target: proto::PlanState,
            first: (&str, Vec<proto::PlanStepEdit>),
            second: (&str, Vec<proto::PlanStepEdit>),
            hold_lock: bool,
        ) -> (Result<proto::Plan, Status>, Result<proto::Plan, Status>) {
            let hold = if hold_lock {
                Some(db.hold_plan_row(plan_id).await)
            } else {
                None
            };
            let spawn_decision = |(reason, edits): (&str, Vec<proto::PlanStepEdit>)| {
                let db = db.clone();
                let plan_id = plan_id.to_owned();
                let reason = reason.to_owned();
                tokio::spawn(async move { db.decide(&plan_id, target, &reason, edits).await })
            };
            let first = spawn_decision(first);
            let second = spawn_decision(second);
            if let Some(hold) = hold {
                db.wait_for_lock_waiters(2).await;
                assert!(
                    !first.is_finished() && !second.is_finished(),
                    "a decision finished while the plan row was locked: it is not waiting on the lock"
                );
                hold.rollback().await.expect("release plan row");
            }
            (
                first.await.expect("first decision task"),
                second.await.expect("second decision task"),
            )
        }

        // -----------------------------------------------------------------
        // propose
        // -----------------------------------------------------------------

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        #[ignore = "requires DATABASE_URL to a SCRATCH Postgres (name contains \"test\")"]
        async fn plan_gate_propose_stores_a_proposed_plan_with_ordered_pending_steps() {
            let db = PlanGateDb::connect().await;
            let run = db.new_run(&db.thread).await;
            let plan = db
                .propose_with_ops(
                    &db.thread,
                    &run,
                    &[
                        ("Draft the report", "draft"),
                        ("Email it", "email.send"),
                        ("File a copy", ""),
                    ],
                    "",
                )
                .await;

            assert_eq!(state(&plan), proto::PlanState::Proposed as i32);
            assert_eq!(plan.run_id, run);
            assert_eq!(
                plan.author, db.user,
                "the owner column holds the real end user, never the agent label"
            );
            let shape: Vec<(&str, &str, i32)> = plan
                .steps
                .iter()
                .map(|s| (s.title.as_str(), s.operation.as_str(), s.state))
                .collect();
            let pending = proto::PlanStepState::Pending as i32;
            assert_eq!(
                shape,
                vec![
                    ("Draft the report", "draft", pending),
                    ("Email it", "email.send", pending),
                    ("File a copy", "", pending),
                ]
            );
            let rows = db.steps_of(&plan.id).await;
            assert_eq!(ordinals(&rows), vec![1, 2, 3]);
            assert_eq!(db.metadata_of(&plan.id).await["author"], "agent");
            db.cleanup().await;
        }

        // -----------------------------------------------------------------
        // approve with edits: rewrite, reorder, remove, add
        // -----------------------------------------------------------------

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        #[ignore = "requires DATABASE_URL to a SCRATCH Postgres (name contains \"test\")"]
        async fn plan_gate_approval_applies_rewrite_reorder_remove_and_add() {
            let db = PlanGateDb::connect().await;
            let plan = db
                .propose_with_ops(
                    &db.thread,
                    "",
                    &[
                        ("Draft the report", "draft"),
                        ("Email the report", "email.send"),
                        ("File a copy", "files.write"),
                        ("Archive", "archive"),
                    ],
                    "",
                )
                .await;
            let [a, b, c, d] = [0, 1, 2, 3].map(|i| plan.steps[i].id.clone());

            let approved = db
                .decide(
                    &plan.id,
                    proto::PlanState::Approved,
                    "looks right",
                    vec![
                        // reworded and moved to the front
                        plan_edit(&d, "Archive the thread", "archive", false),
                        plan_edit(&a, "Draft the summary", "draft", false),
                        // removed: the browser's copy of the title must not overwrite the proposal
                        plan_edit(&b, "NOT WHAT WAS PROPOSED", "", true),
                        // brand new, between existing steps
                        plan_edit("", "Notify the team", "chat.post", false),
                        plan_edit(&c, "File a copy", "files.write", false),
                    ],
                )
                .await
                .expect("approval with edits");

            assert_eq!(state(&approved), proto::PlanState::Approved as i32);
            let rows = db.steps_of(&plan.id).await;
            assert_eq!(
                ordinals(&rows),
                vec![1, 2, 3, 4, 5],
                "contiguous, positive ordinals: no parked (negative) ordinal may survive"
            );
            assert_eq!(rows.len(), 5, "a removed step is skipped, never deleted");
            let new_id = rows[3].1.clone();
            assert!(
                new_id.starts_with("step_") && ![&a, &b, &c, &d].contains(&&new_id),
                "the new step gets a freshly minted id, got {new_id}"
            );
            let shape: Vec<(&str, &str, &str, &str)> = rows
                .iter()
                .map(|r| (r.1.as_str(), r.2.as_str(), r.3.as_str(), r.4.as_str()))
                .collect();
            assert_eq!(
                shape,
                vec![
                    (d.as_str(), "pending", "Archive the thread", "archive"),
                    (a.as_str(), "pending", "Draft the summary", "draft"),
                    (b.as_str(), "skipped", "Email the report", "email.send"),
                    (new_id.as_str(), "pending", "Notify the team", "chat.post"),
                    (c.as_str(), "pending", "File a copy", "files.write"),
                ],
                "the removed step keeps the title and operation it was proposed with"
            );
            // the response carries the same steps, in the same order and states
            let response_shape: Vec<(&str, i32)> = approved
                .steps
                .iter()
                .map(|s| (s.id.as_str(), s.state))
                .collect();
            let skipped = proto::PlanStepState::Skipped as i32;
            let pending = proto::PlanStepState::Pending as i32;
            assert_eq!(
                response_shape,
                vec![
                    (d.as_str(), pending),
                    (a.as_str(), pending),
                    (b.as_str(), skipped),
                    (new_id.as_str(), pending),
                    (c.as_str(), pending),
                ]
            );
            // the approval is a receipt on the plan itself, under the VERIFIED actor
            let metadata = db.metadata_of(&plan.id).await;
            assert_eq!(metadata["approved_by"], db.user.as_str());
            assert_eq!(metadata["approved_reason"], "looks right");
            db.cleanup().await;
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        #[ignore = "requires DATABASE_URL to a SCRATCH Postgres (name contains \"test\")"]
        async fn plan_gate_approval_without_edits_leaves_the_steps_untouched() {
            let db = PlanGateDb::connect().await;
            let plan = db.propose_three().await;
            let before = db.steps_of(&plan.id).await;
            let approved = db
                .decide(&plan.id, proto::PlanState::Approved, "", Vec::new())
                .await
                .expect("plain approval");
            assert_eq!(state(&approved), proto::PlanState::Approved as i32);
            assert_eq!(db.steps_of(&plan.id).await, before);
            assert_eq!(db.status_of(&plan.id).await, "approved");
            db.cleanup().await;
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        #[ignore = "requires DATABASE_URL to a SCRATCH Postgres (name contains \"test\")"]
        async fn plan_gate_rejection_with_every_step_removed_skips_them_all() {
            let db = PlanGateDb::connect().await;
            let plan = db.propose_three().await;
            let ids: Vec<String> = plan.steps.iter().map(|s| s.id.clone()).collect();
            let rejected = db
                .decide(
                    &plan.id,
                    proto::PlanState::Rejected,
                    "not now",
                    ids.iter().map(|id| plan_edit(id, "", "", true)).collect(),
                )
                .await
                .expect("rejection may carry removals");
            assert_eq!(state(&rejected), proto::PlanState::Rejected as i32);
            let rows = db.steps_of(&plan.id).await;
            assert_eq!(rows.len(), 3);
            assert!(rows.iter().all(|r| r.2 == "skipped"), "{rows:?}");
            assert_eq!(
                rows.iter().map(|r| r.3.as_str()).collect::<Vec<_>>(),
                vec!["Step one", "Step two", "Step three"]
            );
            assert_eq!(db.metadata_of(&plan.id).await["rejected_reason"], "not now");
            db.cleanup().await;
        }

        // -----------------------------------------------------------------
        // (plan_id, ordinal) unique index survives reorders combined with removals
        // -----------------------------------------------------------------

        /// Approve a plan of `titles.len()` steps with the edit list `order`
        /// (`(original index, removed)` per position), and check the stored
        /// steps follow it with contiguous ordinals.
        async fn approve_in_order(db: &PlanGateDb, titles: &[&str], order: &[(usize, bool)]) {
            let plan = db.propose(&db.thread, "", titles, "").await;
            let ids: Vec<String> = plan.steps.iter().map(|s| s.id.clone()).collect();
            let edits: Vec<proto::PlanStepEdit> = order
                .iter()
                .map(|&(index, removed)| {
                    plan_edit(
                        &ids[index],
                        if removed { "" } else { titles[index] },
                        "",
                        removed,
                    )
                })
                .collect();
            db.decide(&plan.id, proto::PlanState::Approved, "", edits)
                .await
                .unwrap_or_else(|e| {
                    panic!("reorder {order:?} must not collide on (plan_id, ordinal): {e}")
                });
            let rows = db.steps_of(&plan.id).await;
            assert_eq!(
                ordinals(&rows),
                (1..=i64::try_from(titles.len()).expect("small")).collect::<Vec<_>>(),
                "order {order:?}"
            );
            let got: Vec<(&str, &str, &str)> = rows
                .iter()
                .map(|r| (r.1.as_str(), r.2.as_str(), r.3.as_str()))
                .collect();
            let want: Vec<(&str, &str, &str)> = order
                .iter()
                .map(|&(index, removed)| {
                    (
                        ids[index].as_str(),
                        if removed { "skipped" } else { "pending" },
                        titles[index],
                    )
                })
                .collect();
            assert_eq!(got, want, "order {order:?}");
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        #[ignore = "requires DATABASE_URL to a SCRATCH Postgres (name contains \"test\")"]
        async fn plan_gate_reorder_after_a_removal_does_not_collide_on_the_ordinal_index() {
            let db = PlanGateDb::connect().await;
            let three = ["A", "B", "C"];
            // reverse, with the middle step removed
            approve_in_order(&db, &three, &[(2, false), (1, true), (0, false)]).await;
            // rotate left; the step that wraps to the end is the removed one
            approve_in_order(&db, &three, &[(1, false), (2, false), (0, true)]).await;
            // the first step removed where it stands while the other two swap
            approve_in_order(&db, &three, &[(0, true), (2, false), (1, false)]).await;
            // the last step moved to the front and removed
            approve_in_order(&db, &three, &[(2, true), (0, false), (1, false)]).await;
            // a six-step full reversal with two removals: every ordinal moves
            let six = ["s1", "s2", "s3", "s4", "s5", "s6"];
            approve_in_order(
                &db,
                &six,
                &[
                    (5, false),
                    (4, true),
                    (3, false),
                    (2, false),
                    (1, true),
                    (0, false),
                ],
            )
            .await;
            db.cleanup().await;
        }

        // -----------------------------------------------------------------
        // supersede
        // -----------------------------------------------------------------

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        #[ignore = "requires DATABASE_URL to a SCRATCH Postgres (name contains \"test\")"]
        #[allow(clippy::too_many_lines)]
        async fn plan_gate_propose_twice_supersedes_the_older_plan_and_only_the_newest_is_approvable(
        ) {
            let db = PlanGateDb::connect().await;
            let thread = db.thread.clone();
            let run1 = db.new_run(&thread).await;
            let run2 = db.new_run(&thread).await;
            let run3 = db.new_run(&thread).await;

            // A proposal on ANOTHER run of the same thread is not this run's
            // predecessor and must survive everything below.
            let bystander = db.propose(&thread, &run2, &["Bystander"], "").await;

            // (a) last-proposed-wins within one run, without the model naming
            // the predecessor.
            let first = db.propose(&thread, &run1, &["Draft"], "").await;
            let second = db.propose(&thread, &run1, &["Draft v2", "Send"], "").await;
            assert_eq!(db.status_of(&first.id).await, "superseded");
            assert_eq!(db.status_of(&second.id).await, "proposed");
            assert_eq!(
                second.supersedes, first.id,
                "the immediate predecessor is recorded even when the model did not name it"
            );
            assert_eq!(
                db.status_of(&bystander.id).await,
                "proposed",
                "another run's proposal is not superseded"
            );

            // the superseded plan can no longer be decided, and is left exactly as it was
            let stale = db
                .decide(
                    &first.id,
                    proto::PlanState::Approved,
                    "stale card",
                    Vec::new(),
                )
                .await
                .expect_err("a superseded plan must not be approvable");
            assert_eq!(stale.code(), tonic::Code::FailedPrecondition);
            assert_eq!(db.status_of(&first.id).await, "superseded");
            assert!(db.metadata_of(&first.id).await.get("approved_by").is_none());

            // only the newest is approvable
            let approved = db
                .decide(
                    &second.id,
                    proto::PlanState::Approved,
                    "ship it",
                    Vec::new(),
                )
                .await
                .expect("the newest plan is approvable");
            assert_eq!(state(&approved), proto::PlanState::Approved as i32);

            // (b) a named supersede on the same thread (no run involved)
            let named_old = db.propose(&thread, "", &["Old"], "").await;
            let named_new = db.propose(&thread, "", &["New"], &named_old.id).await;
            assert_eq!(db.status_of(&named_old.id).await, "superseded");
            assert_eq!(named_new.supersedes, named_old.id);
            assert_eq!(db.status_of(&named_new.id).await, "proposed");

            // (c) a hostile or confused `supersedes` touches nothing and is
            // struck from the chain: another thread's proposed plan, and a plan
            // that is no longer PROPOSED.
            let other_thread = db.new_thread().await;
            let foreign = db.propose(&other_thread, "", &["Foreign"], "").await;
            let hostile = db.propose(&thread, "", &["Hostile"], &foreign.id).await;
            assert_eq!(
                db.status_of(&foreign.id).await,
                "proposed",
                "supersede is scoped to the caller's own thread"
            );
            assert_eq!(hostile.supersedes, "", "an untrue claim is not recorded");
            assert!(db
                .metadata_of(&hostile.id)
                .await
                .get("supersedes")
                .is_none());
            let decided_target = db
                .propose(&thread, "", &["Aimed at an approved plan"], &second.id)
                .await;
            assert_eq!(
                db.status_of(&second.id).await,
                "approved",
                "an approved plan is never knocked back to superseded"
            );
            assert_eq!(decided_target.supersedes, "");

            // (d) a claim that matches nothing is struck, and the run's REAL
            // predecessor is recorded in its place.
            let before = db.propose(&thread, &run3, &["Before"], "").await;
            let after = db
                .propose(&thread, &run3, &["After"], "plan_does_not_exist")
                .await;
            assert_eq!(db.status_of(&before.id).await, "superseded");
            assert_eq!(after.supersedes, before.id);

            assert_eq!(
                db.status_of(&bystander.id).await,
                "proposed",
                "still untouched at the end"
            );
            db.cleanup().await;
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        #[ignore = "requires DATABASE_URL to a SCRATCH Postgres (name contains \"test\")"]
        async fn plan_gate_concurrent_proposals_on_one_run_leave_exactly_one_proposed_plan() {
            let db = PlanGateDb::connect().await;
            for round in 0..24 {
                let run = db.new_run(&db.thread).await;
                let propose = |title: &'static str| {
                    let db = db.clone();
                    let run = run.clone();
                    tokio::spawn(
                        async move { db.propose(&db.thread.clone(), &run, &[title], "").await },
                    )
                };
                let (a, b) = (propose("A"), propose("B"));
                let (a, b) = (a.await.expect("task a"), b.await.expect("task b"));
                let proposed: i64 = sqlx::query_scalar(
                    "SELECT count(*) FROM plans WHERE run_id = $1 AND status = 'proposed'",
                )
                .bind(&run)
                .fetch_one(&db.pool)
                .await
                .expect("count");
                assert_eq!(
                    proposed, 1,
                    "round {round}: two concurrent proposals on one run must leave exactly one \
                     PROPOSED plan (last-proposed-wins); {} and {} are both approvable",
                    a.id, b.id
                );
            }
            db.cleanup().await;
        }

        // -----------------------------------------------------------------
        // compare-and-swap: concurrent approvals and execution claims
        // -----------------------------------------------------------------

        /// Two rival edit sets over a plan's three steps, with what each leaves
        /// behind as `(id, title, status)` in ordinal order.
        fn rival_edits(
            ids: &[String],
            variant: usize,
        ) -> (
            Vec<proto::PlanStepEdit>,
            Vec<(String, String, &'static str)>,
        ) {
            if variant == 0 {
                (
                    vec![
                        plan_edit(&ids[2], "X third", "", false),
                        plan_edit(&ids[0], "X first", "", false),
                        plan_edit(&ids[1], "ignored", "", true),
                    ],
                    vec![
                        (ids[2].clone(), "X third".to_owned(), "pending"),
                        (ids[0].clone(), "X first".to_owned(), "pending"),
                        (ids[1].clone(), "Step two".to_owned(), "skipped"),
                    ],
                )
            } else {
                (
                    vec![
                        plan_edit(&ids[1], "Y second", "", false),
                        plan_edit(&ids[2], "Y third", "", false),
                        plan_edit(&ids[0], "Y first", "", false),
                    ],
                    vec![
                        (ids[1].clone(), "Y second".to_owned(), "pending"),
                        (ids[2].clone(), "Y third".to_owned(), "pending"),
                        (ids[0].clone(), "Y first".to_owned(), "pending"),
                    ],
                )
            }
        }

        async fn assert_only_the_winners_edits_landed(
            db: &PlanGateDb,
            plan: &proto::Plan,
            winner: usize,
            want: &[Vec<(String, String, &'static str)>; 2],
        ) {
            let rows = db.steps_of(&plan.id).await;
            let got: Vec<(String, String, &str)> = rows
                .iter()
                .map(|r| (r.1.clone(), r.3.clone(), r.2.as_str()))
                .collect();
            let want: Vec<(String, String, &str)> = want[winner].clone();
            assert_eq!(got, want, "only request {winner}'s edits may be stored");
            assert_eq!(ordinals(&rows), vec![1, 2, 3]);
            assert_eq!(
                db.metadata_of(&plan.id).await["approved_reason"],
                if winner == 0 {
                    "decided by X"
                } else {
                    "decided by Y"
                },
                "the approval receipt belongs to the winner"
            );
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        #[ignore = "requires DATABASE_URL to a SCRATCH Postgres (name contains \"test\")"]
        async fn plan_gate_two_approvals_queued_on_the_row_lock_have_exactly_one_winner() {
            let db = PlanGateDb::connect().await;
            let plan = db.propose_three().await;
            let ids: Vec<String> = plan.steps.iter().map(|s| s.id.clone()).collect();
            let (edits_x, want_x) = rival_edits(&ids, 0);
            let (edits_y, want_y) = rival_edits(&ids, 1);

            let (first, second) = race(
                &db,
                &plan.id,
                proto::PlanState::Approved,
                ("decided by X", edits_x),
                ("decided by Y", edits_y),
                true,
            )
            .await;
            let (winner, refusal) = single_winner(first, second);

            assert_eq!(refusal.code(), tonic::Code::FailedPrecondition);
            assert!(
                refusal
                    .message()
                    .contains("decided by another request first"),
                "the loser must be stopped by the compare-and-swap under the row lock (both \
                 requests had read the plan as PROPOSED), got: {}",
                refusal.message()
            );
            assert_eq!(db.status_of(&plan.id).await, "approved");
            assert_only_the_winners_edits_landed(&db, &plan, winner, &[want_x, want_y]).await;
            db.cleanup().await;
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        #[ignore = "requires DATABASE_URL to a SCRATCH Postgres (name contains \"test\")"]
        async fn plan_gate_free_running_concurrent_approvals_have_exactly_one_winner() {
            let db = PlanGateDb::connect().await;
            let mut compare_and_swap_refusals = 0;
            for round in 0..16 {
                let plan = db.propose_three().await;
                let ids: Vec<String> = plan.steps.iter().map(|s| s.id.clone()).collect();
                let (edits_x, want_x) = rival_edits(&ids, 0);
                let (edits_y, want_y) = rival_edits(&ids, 1);
                let (first, second) = race(
                    &db,
                    &plan.id,
                    proto::PlanState::Approved,
                    ("decided by X", edits_x),
                    ("decided by Y", edits_y),
                    false,
                )
                .await;
                let (winner, refusal) = single_winner(first, second);
                assert_eq!(
                    refusal.code(),
                    tonic::Code::FailedPrecondition,
                    "round {round}: {refusal}"
                );
                if refusal
                    .message()
                    .contains("decided by another request first")
                {
                    compare_and_swap_refusals += 1;
                }
                assert_eq!(db.status_of(&plan.id).await, "approved", "round {round}");
                assert_only_the_winners_edits_landed(&db, &plan, winner, &[want_x, want_y]).await;
            }
            eprintln!(
                "plan_gate: {compare_and_swap_refusals}/16 free-running losers were stopped by the \
                 compare-and-swap itself (the rest by the handler's legality check)"
            );
            db.cleanup().await;
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        #[ignore = "requires DATABASE_URL to a SCRATCH Postgres (name contains \"test\")"]
        async fn plan_gate_two_execution_claims_have_a_single_winner() {
            let db = PlanGateDb::connect().await;

            // queued on the row lock: both read APPROVED, one claim wins
            let plan = db.propose_three().await;
            db.decide(&plan.id, proto::PlanState::Approved, "go", Vec::new())
                .await
                .expect("approve");
            let (first, second) = race(
                &db,
                &plan.id,
                proto::PlanState::Executing,
                ("claim one", Vec::new()),
                ("claim two", Vec::new()),
                true,
            )
            .await;
            let (_, refusal) = single_winner(first, second);
            assert_eq!(refusal.code(), tonic::Code::FailedPrecondition);
            assert!(
                refusal
                    .message()
                    .contains("decided by another request first")
                    && refusal.message().contains("executing"),
                "the second claim must lose the compare-and-swap, got: {}",
                refusal.message()
            );
            assert_eq!(db.status_of(&plan.id).await, "executing");
            let metadata = db.metadata_of(&plan.id).await;
            assert_eq!(metadata["executing_by"], db.user.as_str());
            assert!(
                metadata["executing_reason"] == "claim one"
                    || metadata["executing_reason"] == "claim two"
            );
            assert_eq!(
                metadata["approved_reason"], "go",
                "the claim must not overwrite the approval receipt"
            );

            // free-running rounds
            for round in 0..16 {
                let plan = db.propose_three().await;
                db.decide(&plan.id, proto::PlanState::Approved, "", Vec::new())
                    .await
                    .expect("approve");
                let (first, second) = race(
                    &db,
                    &plan.id,
                    proto::PlanState::Executing,
                    ("claim one", Vec::new()),
                    ("claim two", Vec::new()),
                    false,
                )
                .await;
                let (_, refusal) = single_winner(first, second);
                assert_eq!(
                    refusal.code(),
                    tonic::Code::FailedPrecondition,
                    "round {round}: {refusal}"
                );
                assert_eq!(db.status_of(&plan.id).await, "executing", "round {round}");
            }
            db.cleanup().await;
        }

        // -----------------------------------------------------------------
        // the compare-and-swap itself (store level)
        // -----------------------------------------------------------------

        fn step_edit_row(id: &str, title: &str, removed: bool) -> store::PlanStepEditRow {
            store::PlanStepEditRow {
                id: id.to_owned(),
                is_new: false,
                title: title.to_owned(),
                operation: String::new(),
                removed,
            }
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        #[ignore = "requires DATABASE_URL to a SCRATCH Postgres (name contains \"test\")"]
        async fn plan_gate_transition_rechecks_the_status_under_the_row_lock_and_writes_nothing_on_conflict(
        ) {
            let db = PlanGateDb::connect().await;
            let plan = db.propose_three().await;
            let ids: Vec<String> = plan.steps.iter().map(|s| s.id.clone()).collect();
            let before = db.steps_of(&plan.id).await;
            let metadata_before = db.metadata_of(&plan.id).await;

            // A rival decides the plan while our transition (made against the
            // status it read earlier: proposed) is already waiting for the row.
            let mut rival = db.hold_plan_row(&plan.id).await;
            let waiting = {
                let pool = db.pool.clone();
                let plan_id = plan.id.clone();
                let edits = vec![
                    step_edit_row(&ids[2], "Reordered", false),
                    step_edit_row(&ids[1], "ignored", true),
                    step_edit_row(&ids[0], "Rewritten", false),
                ];
                tokio::spawn(async move {
                    store::transition_plan_atomic(
                        &pool,
                        &plan_id,
                        "proposed",
                        "approved",
                        &edits,
                        &serde_json::json!({ "approved_by": "late-arrival" }),
                    )
                    .await
                })
            };
            db.wait_for_lock_waiters(1).await;
            assert!(
                !waiting.is_finished(),
                "the transition must be blocked on the row lock"
            );
            sqlx::query("UPDATE plans SET status = 'rejected' WHERE id = $1")
                .bind(&plan.id)
                .execute(&mut *rival)
                .await
                .expect("rival decision");
            rival.commit().await.expect("rival commit");

            let outcome = waiting
                .await
                .expect("task")
                .expect("a lost race is an outcome, not an SQL error");
            assert_eq!(
                outcome,
                store::PlanTransitionOutcome::Conflict {
                    actual: Some("rejected".to_owned())
                },
                "the status must be re-read AFTER the lock is granted"
            );
            assert_eq!(db.status_of(&plan.id).await, "rejected");
            assert_eq!(
                db.steps_of(&plan.id).await,
                before,
                "the losing edit set must write nothing"
            );
            assert_eq!(db.metadata_of(&plan.id).await, metadata_before);

            // a stale expectation on an unlocked plan loses the same way
            let stale = store::transition_plan_atomic(
                &db.pool,
                &plan.id,
                "proposed",
                "approved",
                &[step_edit_row(&ids[0], "Late edit", false)],
                &serde_json::json!({}),
            )
            .await
            .expect("conflict is not an error");
            assert_eq!(
                stale,
                store::PlanTransitionOutcome::Conflict {
                    actual: Some("rejected".to_owned())
                }
            );
            // and a plan that is not there at all
            let missing = store::transition_plan_atomic(
                &db.pool,
                "plan_does_not_exist",
                "proposed",
                "approved",
                &[],
                &serde_json::json!({}),
            )
            .await
            .expect("conflict is not an error");
            assert_eq!(
                missing,
                store::PlanTransitionOutcome::Conflict { actual: None }
            );
            assert_eq!(db.steps_of(&plan.id).await, before);
            db.cleanup().await;
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        #[ignore = "requires DATABASE_URL to a SCRATCH Postgres (name contains \"test\")"]
        async fn plan_gate_a_failing_edit_rolls_the_whole_transition_back() {
            let db = PlanGateDb::connect().await;
            let plan = db.propose_three().await;
            let ids: Vec<String> = plan.steps.iter().map(|s| s.id.clone()).collect();
            let before = db.steps_of(&plan.id).await;
            let metadata_before = db.metadata_of(&plan.id).await;

            // The reorder succeeds statement by statement; the last entry is a
            // "new" step whose id already exists, so its INSERT violates the
            // primary key and the transaction must take everything back.
            let outcome = store::transition_plan_atomic(
                &db.pool,
                &plan.id,
                "proposed",
                "approved",
                &[
                    step_edit_row(&ids[2], "Reordered", false),
                    step_edit_row(&ids[1], "Reordered", false),
                    step_edit_row(&ids[0], "Reordered", false),
                    store::PlanStepEditRow {
                        id: ids[0].clone(),
                        is_new: true,
                        title: "Duplicate id".to_owned(),
                        operation: String::new(),
                        removed: false,
                    },
                ],
                &serde_json::json!({ "approved_by": "nobody" }),
            )
            .await;
            assert!(
                outcome.is_err(),
                "a duplicate step id must fail the transition"
            );

            assert_eq!(db.status_of(&plan.id).await, "proposed");
            assert_eq!(
                db.steps_of(&plan.id).await,
                before,
                "ordinals, titles and statuses must be exactly as proposed"
            );
            assert_eq!(db.metadata_of(&plan.id).await, metadata_before);
            // and the plan is still decidable afterwards: no lock was leaked
            db.decide(&plan.id, proto::PlanState::Approved, "", Vec::new())
                .await
                .expect("the plan is still approvable after the failed attempt");
            db.cleanup().await;
        }

        // -----------------------------------------------------------------
        // a decided plan stays as decided; only the owner decides
        // -----------------------------------------------------------------

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        #[ignore = "requires DATABASE_URL to a SCRATCH Postgres (name contains \"test\")"]
        async fn plan_gate_edits_after_approval_are_refused_and_change_nothing() {
            let db = PlanGateDb::connect().await;
            let plan = db.propose_three().await;
            let ids: Vec<String> = plan.steps.iter().map(|s| s.id.clone()).collect();
            db.decide(&plan.id, proto::PlanState::Approved, "go", Vec::new())
                .await
                .expect("approve");
            let approved_steps = db.steps_of(&plan.id).await;
            let approved_metadata = db.metadata_of(&plan.id).await;

            // edits riding on the claim that starts execution
            let late_edit = db
                .decide(
                    &plan.id,
                    proto::PlanState::Executing,
                    "",
                    vec![
                        plan_edit(&ids[0], "Swapped under the grant", "", false),
                        plan_edit(&ids[1], "", "", true),
                        plan_edit(&ids[2], "Step three", "", false),
                    ],
                )
                .await
                .expect_err("an approved plan's steps cannot be rewritten");
            assert_eq!(late_edit.code(), tonic::Code::FailedPrecondition);
            assert!(late_edit.message().contains("edited_steps"), "{late_edit}");

            // approving again, and rejecting after the fact
            for target in [proto::PlanState::Approved, proto::PlanState::Rejected] {
                let refusal = db
                    .decide(&plan.id, target, "again", Vec::new())
                    .await
                    .expect_err("a decided plan cannot be decided again");
                assert_eq!(
                    refusal.code(),
                    tonic::Code::FailedPrecondition,
                    "{target:?}"
                );
            }

            assert_eq!(db.status_of(&plan.id).await, "approved");
            assert_eq!(db.steps_of(&plan.id).await, approved_steps);
            assert_eq!(db.metadata_of(&plan.id).await, approved_metadata);
            db.cleanup().await;
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        #[ignore = "requires DATABASE_URL to a SCRATCH Postgres (name contains \"test\")"]
        async fn plan_gate_only_the_owner_can_propose_on_or_decide_the_plan() {
            let db = PlanGateDb::connect().await;
            let plan = db.propose_three().await;
            let request = |target: proto::PlanState| proto::TransitionPlanRequest {
                plan_id: plan.id.clone(),
                target_state: target as i32,
                ..Default::default()
            };

            // another person in the same organisation
            let colleague = db
                .svc
                .transition_plan(as_person(
                    &db.org,
                    "someone-else",
                    request(proto::PlanState::Approved),
                ))
                .await
                .expect_err("a colleague cannot approve another person's plan");
            assert_eq!(colleague.code(), tonic::Code::PermissionDenied);
            // the same person id in another organisation
            let outsider = db
                .svc
                .transition_plan(as_person(
                    "another-org",
                    &db.user,
                    request(proto::PlanState::Approved),
                ))
                .await
                .expect_err("another tenant cannot approve this plan");
            assert_eq!(outsider.code(), tonic::Code::PermissionDenied);
            // and a colleague cannot put a plan on this person's thread
            let proposal = proto::ProposePlanRequest {
                thread_id: db.thread.clone(),
                org_id: db.org.clone(),
                user_id: "someone-else".to_owned(),
                author: "agent".to_owned(),
                summary: "Planted".to_owned(),
                steps: vec![proto::ProposedPlanStep {
                    title: "Do harm".to_owned(),
                    operation: String::new(),
                }],
                ..Default::default()
            };
            let planted = db
                .svc
                .propose_plan(as_person(&db.org, "someone-else", proposal))
                .await
                .expect_err("a colleague cannot plant a plan on another person's thread");
            assert_eq!(planted.code(), tonic::Code::PermissionDenied);

            assert_eq!(db.status_of(&plan.id).await, "proposed");
            let plans: i64 = sqlx::query_scalar("SELECT count(*) FROM plans WHERE org_id = $1")
                .bind(&db.org)
                .fetch_one(&db.pool)
                .await
                .expect("count");
            assert_eq!(plans, 1, "the refused proposal must not have been stored");
            // the owner still can
            db.decide(&plan.id, proto::PlanState::Approved, "", Vec::new())
                .await
                .expect("the owner approves");
            db.cleanup().await;
        }
    }
}
