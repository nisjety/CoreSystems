//! gRPC server implementing `SessionCore` on :9091.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Verifier as _, VerifyingKey};
use mp_contracts::dataplane::graph_v1::{
    self as graph_pb, graph_service_client::GraphServiceClient,
};
use mp_contracts::dataplane::knowledge_v2::{
    self as know_pb, knowledge_service_client::KnowledgeServiceClient,
};
use mp_contracts::dataplane::retrieval_v2::{
    self as ret_pb, retrieval_service_client::RetrievalServiceClient,
};
use mp_contracts::model_plane::v1::{
    self as pb,
    finetune_jobs_server::FinetuneJobsServer,
    managed_run_lifecycle_server::{ManagedRunLifecycle, ManagedRunLifecycleServer},
    memory_service_server::MemoryServiceServer,
    orchestration_core_service_server::OrchestrationCoreServiceServer,
    routing_policy_server::RoutingPolicyServer,
    run_service_server::RunServiceServer,
    session_core_server::{SessionCore, SessionCoreServer},
};
use mp_events::idempotency::derive_idempotency_hash;
use mp_ids::new_ulid;
use sha2::{Digest as _, Sha256};
use sqlx::PgPool;
use std::{collections::BTreeMap, env, time::Instant};
use tonic::transport::Channel;
use tonic::{Request, Response, Status};
use tracing::{info, warn};

use crate::auth::{
    authorize_operation, authorize_owner_row, authorize_scheduled_step_service,
    authorize_space_deletion_service, authorize_system_run_owner, identity, is_system_run_owner,
    DelegatedDataPlaneBearer, JwtVerifier, OwnerIntent, VerifiedIdentity,
    DATA_PLANE_AUTH_METADATA_KEY,
};
use crate::letta_adapter::{LettaMemoryAdapter, LettaSearchOutcome};
use crate::orchestration_grpc::{json_to_struct, struct_to_json};
use crate::terminalization;

/// Standard gRPC health names registered on the unauthenticated health-only
/// surface. Business RPCs remain independently intercepted.
pub const HEALTH_SERVICE_NAMES: [&str; 7] = [
    "model_plane.v1.SessionCore",
    "model_plane.v1.ManagedRunLifecycle",
    "model_plane.v1.OrchestrationCoreService",
    "model_plane.v1.FinetuneJobs",
    "model_plane.v1.MemoryService",
    "model_plane.v1.RoutingPolicy",
    "model_plane.v1.RunService",
];

const RUN_STARTED_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.RunStarted";
const STEP_COMPLETED_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.StepCompleted";
const RUN_TERMINAL_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.RunTerminal";
const CHECKPOINT_SAVED_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.CheckpointSaved";
const THREAD_CREATED_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.ThreadCreated";
const MESSAGE_APPENDED_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.MessageAppended";
const THREAD_PRESENTATION_UPDATED_TYPE_URL: &str =
    "type.googleapis.com/model_plane.v1.ThreadPresentationUpdated";
const THREAD_ARCHIVED_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.ThreadArchived";
const CONTEXT_MESSAGE_LIMIT: i64 = 20;
const CONTEXT_MEMORY_LIMIT: i64 = 64;
const DEFAULT_THREAD_LIST_LIMIT: i64 = 80;
const MAX_THREAD_LIST_LIMIT: i64 = 200;
const THREAD_TITLE_MAX_CHARS: usize = 96;
const THREAD_PREVIEW_MAX_CHARS: usize = 180;

/// EU data-residency default for Model-Plane processing (P0.4). Sweden Central
/// is the explicit EU region; runs are stamped with this unless an operator
/// overrides `MODEL_PLANE_RESIDENCY`.
const DEFAULT_RESIDENCY: &str = "swedencentral";
const MAX_USER_CHECKPOINT_ID_BYTES: usize = 200;
const MAX_USER_CHECKPOINT_STATE_BYTES: usize = 4 * 1024 * 1024;
const CONTROL_SPACE_DECISION_VERSION: &str = "v2";
const CONTROL_SPACE_DECISION_AUDIENCE: &str = "model-plane";
const CONTROL_THREAD_CREATE_ACTION: &str = "model.thread.create";
const CONTROL_THREAD_APPEND_ACTION: &str = "model.thread.append";
const CONTROL_THREAD_APPEND_SCHEMA: &str = "sha256:thread-append-v1";
const CONTROL_SCHEDULED_RUN_AUDIENCE: &str = "model-plane-capability-core";
const CONTROL_SCHEDULED_RUN_ACTION: &str = "model.schedule.run";
const CONTROL_SCHEDULED_RUN_SCHEMA: &str = "sha256:space-scheduled-run-v1";
const CONTROL_SCHEDULED_RUN_EXECUTION_AUDIENCE: &str = "model-plane-session-core";
const CONTROL_SCHEDULED_RUN_EXECUTION_ACTION: &str = "model.schedule.execute";
const CONTROL_SCHEDULED_RUN_EXECUTION_SCHEMA: &str = "sha256:space-scheduled-run-execute-v1";
const MAX_CONTROL_SPACE_DECISION_TOKEN_BYTES: usize = 16 * 1024;

#[allow(clippy::result_large_err)]
fn validate_user_checkpoint(checkpoint_id: &str, state: &[u8]) -> Result<(), Status> {
    if checkpoint_id.is_empty()
        || checkpoint_id != checkpoint_id.trim()
        || checkpoint_id.len() > MAX_USER_CHECKPOINT_ID_BYTES
        || checkpoint_id.chars().any(char::is_control)
    {
        return Err(Status::invalid_argument("invalid checkpoint_id"));
    }
    if checkpoint_id.starts_with(crate::compaction::AUTO_CHECKPOINT_PREFIX) {
        return Err(Status::permission_denied(
            "automatic checkpoint namespace is reserved",
        ));
    }
    if state.is_empty() || state.len() > MAX_USER_CHECKPOINT_STATE_BYTES {
        return Err(Status::invalid_argument("invalid checkpoint state size"));
    }
    Ok(())
}

/// The configured Model-Plane data-residency region, stamped onto each run at
/// `StartRun`. Defaults to the EU region [`DEFAULT_RESIDENCY`].
pub(crate) fn configured_residency() -> String {
    resolve_residency(std::env::var("MODEL_PLANE_RESIDENCY").ok())
}

/// Pure residency resolution: an explicit non-blank value wins, otherwise the
/// EU default [`DEFAULT_RESIDENCY`]. Split out so it is testable without env
/// mutation (the workspace forbids `unsafe`, which `std::env::set_var` requires).
fn resolve_residency(env_value: Option<String>) -> String {
    env_value
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_RESIDENCY.to_owned())
}

fn nanos_to_i32(nanos: u32) -> i32 {
    i32::try_from(nanos).unwrap_or(i32::MAX)
}

fn to_proto_timestamp(ts: DateTime<Utc>) -> prost_types::Timestamp {
    prost_types::Timestamp {
        seconds: ts.timestamp(),
        nanos: nanos_to_i32(ts.timestamp_subsec_nanos()),
    }
}

fn clamp_thread_limit(limit: u32) -> i64 {
    if limit == 0 {
        return DEFAULT_THREAD_LIST_LIMIT;
    }
    i64::from(limit).clamp(1, MAX_THREAD_LIST_LIMIT)
}

fn compact_thread_text(value: Option<String>, fallback: &str, max_chars: usize) -> String {
    let text = value
        .map(|value| value.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| fallback.to_owned());
    if text.chars().count() <= max_chars {
        return text;
    }
    let mut truncated = text
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    truncated = truncated.trim_end().to_owned();
    format!("{truncated}...")
}

/// Normalize a user-facing thread title or preview before durable storage.
///
/// An explicitly empty value clears the corresponding presentation override;
/// an omitted field is handled by the RPC layer and means "leave unchanged".
/// Values are rejected rather than silently truncated so a caller cannot claim
/// that a title/preview was preserved when it was materially altered.
#[allow(clippy::result_large_err)]
fn normalize_thread_presentation_text(
    value: &str,
    field: &'static str,
    max_chars: usize,
) -> Result<Option<String>, Status> {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() > max_chars {
        return Err(Status::invalid_argument(format!(
            "{field} must not exceed {max_chars} characters"
        )));
    }
    Ok((!normalized.is_empty()).then_some(normalized))
}

fn record_metrics(method: &'static str, started: Instant, is_ok: bool) {
    let status = if is_ok { "ok" } else { "error" };
    metrics::counter!("mp_session_grpc_requests_total", "method" => method, "status" => status)
        .increment(1);
    metrics::histogram!("mp_session_grpc_request_duration_seconds", "method" => method)
        .record(started.elapsed().as_secs_f64());
}

#[derive(Default)]
struct TopicBuckets {
    policy: Vec<String>,
    workspace: Vec<String>,
    agent: Vec<String>,
    user: Vec<String>,
    episodic: Vec<String>,
    skill_index: Vec<String>,
    skill_expansion: Vec<String>,
    retrieval: Vec<String>,
}

fn bucket_memory_segments(rows: Vec<(String, String)>) -> TopicBuckets {
    let mut buckets = TopicBuckets::default();

    for (topic, content) in rows {
        if content.is_empty() {
            continue;
        }

        match topic.as_str() {
            "POLICY" => buckets.policy.push(content),
            "WORKSPACE" => buckets.workspace.push(content),
            "AGENT" => buckets.agent.push(content),
            "USER" => buckets.user.push(content),
            "MEMORY" => buckets.episodic.push(content),
            "SKILL_INDEX" => buckets.skill_index.push(content),
            "SKILL_EXPANSION" => buckets.skill_expansion.push(content),
            "RETRIEVAL" => buckets.retrieval.push(content),
            _ => buckets.episodic.push(format!("{topic}: {content}")),
        }
    }

    buckets
}

pub struct SessionService {
    pool: PgPool,
    retrieval_client: Option<RetrievalServiceClient<Channel>>,
    graph_client: Option<GraphServiceClient<Channel>>,
    knowledge_client: Option<KnowledgeServiceClient<Channel>>,
    letta_memory: Option<LettaMemoryAdapter>,
    /// Transactional audit outbox. Tool-step intent is committed with the step;
    /// delivery retries independently until Audit Core acknowledges it.
    audit_publisher: Option<std::sync::Arc<crate::audit_publisher::AuditOutbox>>,
    /// Same verifier the interceptor uses, retained so a delegated Data Plane
    /// credential can be re-verified and bound to the caller before it is
    /// forwarded. `None` only in tests that never delegate; a delegation
    /// arriving without a verifier fails closed rather than being forwarded
    /// unverified.
    auth: Option<JwtVerifier>,
}

/// Additive managed-run protocol. Keeping this service separate preserves the
/// legacy `SessionCore` trait and its existing mocks while giving producers a
/// durable, source-bound terminal receipt contract.
pub struct ManagedRunLifecycleService {
    pool: PgPool,
}

/// Attach the caller's delegated Data Plane credential to an outbound Data
/// Plane v2 gRPC request.
///
/// Data Plane retrieval enforces per-user, private-until-shared authorization
/// with a post-filter keyed on this token's `sub`/`org_id`, so forwarding the
/// caller's own already-verified credential is exactly what keeps one user's
/// context assembly from reading another user's corpus. A shared internal key or
/// a session-core service token would collapse every user's view into one
/// identity, so no such substitute is accepted here: the only input is a
/// [`DelegatedDataPlaneBearer`], which cannot be constructed without passing
/// verification and caller binding.
///
/// # Errors
///
/// Returns `Unauthenticated` if the verified credential cannot be encoded as
/// gRPC metadata. Keeping `tonic::Status` preserves the auth boundary.
#[allow(clippy::result_large_err)]
fn authorize_dataplane<T>(
    message: T,
    bearer: &DelegatedDataPlaneBearer,
) -> Result<Request<T>, Status> {
    let value = tonic::metadata::MetadataValue::try_from(format!("Bearer {}", bearer.as_str()))
        .map_err(|_| Status::unauthenticated("verified bearer cannot be forwarded"))?;
    let mut request = Request::new(message);
    request.metadata_mut().insert("authorization", value);
    Ok(request)
}

async fn authorize_thread_owner(
    pool: &PgPool,
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

/// Require the verified caller to be the exact owner of a thread.
///
/// `authorize_owner_row` intentionally lets a service principal mutate a
/// human-owned row inside its org on a number of pre-existing service paths.
/// Destructive thread erasure is narrower: a gateway/service credential must
/// never be able to erase a person's conversation merely because it shares the
/// tenant. Human callers must match `threads.user_id`; the only service escape
/// hatch is an exact allowlisted system-run owner.
#[allow(clippy::result_large_err)]
async fn authorize_thread_deletion_owner(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    thread_id: &str,
) -> Result<(), Status> {
    let owner: Option<(String, String)> =
        sqlx::query_as("SELECT org_id, user_id FROM threads WHERE id = $1")
            .bind(thread_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
    let (org_id, user_id) = owner.ok_or_else(|| Status::not_found("thread not found"))?;
    caller.authorize_org(&org_id)?;
    match caller.user_id() {
        Some(caller_user) if caller_user == user_id => Ok(()),
        Some(_) => Err(Status::permission_denied("thread owner required")),
        None if is_system_run_owner(&user_id) && caller.principal_id() == user_id => Ok(()),
        None => Err(Status::permission_denied(
            "only the exact thread owner may erase a thread",
        )),
    }
}

/// Delete every durable row owned by one thread while `tx` holds the caller's
/// transaction. The order is explicit because the original schema predates
/// this operation and most foreign keys do not have `ON DELETE CASCADE`.
///
/// This helper intentionally removes audit/event rows as well as transcript
/// rows. A user-facing erase must not leave the content the user asked to
/// remove in replay, checkpoints, approvals, task artifacts, or the learning
/// outbox. It does not touch org/user-level memory, skills, billing, or other
/// threads.
/// Removes every content-bearing row hanging off one thread.
///
/// Returns the `agent_memory` rows that were deleted, because each has a
/// semantic twin on letta-bridge keyed by the same id, and once the durable row
/// is gone there is nothing left in Postgres to find that twin by. The caller
/// propagates after commit — see [`crate::memory_erasure`] for why this is not
/// done inside the transaction.
async fn delete_thread_rows(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    thread_id: &str,
) -> Result<Vec<crate::memory_erasure::ErasedMemory>, Status> {
    // Captured before the row is gone; `owner` is the user_id the semantic copy
    // was tagged with (memory_grpc::index_memory).
    let erased_memories: Vec<crate::memory_erasure::ErasedMemory> =
        sqlx::query_as::<_, (String, String)>(
            "DELETE FROM agent_memory WHERE session_id = $1 RETURNING id, owner",
        )
        .bind(thread_id)
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| Status::internal(error.to_string()))?
        .into_iter()
        .map(|(memory_id, owner)| crate::memory_erasure::ErasedMemory { memory_id, owner })
        .collect();

    // Direct thread evidence and content-bearing side ledgers.
    for query in [
        "DELETE FROM events
         WHERE run_id = $1
            OR run_id IN (SELECT id FROM runs WHERE thread_id = $1)
            OR resource_ref = 'thread:' || $1
            OR payload ->> 'thread_id' = $1",
        "DELETE FROM session_audit_outbox
         WHERE payload ->> 'thread_id' = $1
            OR payload ->> 'run_id' IN (SELECT id FROM runs WHERE thread_id = $1)
            OR payload ->> 'subject' IN (SELECT id FROM runs WHERE thread_id = $1)",
        "DELETE FROM dream_runs WHERE thread_id = $1
            OR run_id IN (SELECT id FROM runs WHERE thread_id = $1)",
        "DELETE FROM memory_index WHERE thread_id = $1",
    ] {
        sqlx::query(query)
            .bind(thread_id)
            .execute(&mut **tx)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
    }

    // Tasks are run-owned rather than thread-owned. Remove their descendants
    // before the task rows, and detach surviving task parents so a task from an
    // unrelated run cannot retain an FK to deleted work.
    for query in [
        "DELETE FROM task_dependencies
         WHERE task_id IN (SELECT id FROM tasks WHERE run_id IN (SELECT id FROM runs WHERE thread_id = $1))
            OR depends_on_id IN (SELECT id FROM tasks WHERE run_id IN (SELECT id FROM runs WHERE thread_id = $1))",
        "DELETE FROM task_events
         WHERE task_id IN (SELECT id FROM tasks WHERE run_id IN (SELECT id FROM runs WHERE thread_id = $1))",
        "DELETE FROM task_assignments
         WHERE task_id IN (SELECT id FROM tasks WHERE run_id IN (SELECT id FROM runs WHERE thread_id = $1))
            OR run_id IN (SELECT id FROM runs WHERE thread_id = $1)",
        "DELETE FROM task_artifacts
         WHERE task_id IN (SELECT id FROM tasks WHERE run_id IN (SELECT id FROM runs WHERE thread_id = $1))",
        "UPDATE cron_fires SET task_id = NULL
         WHERE task_id IN (SELECT id FROM tasks WHERE run_id IN (SELECT id FROM runs WHERE thread_id = $1))",
        "UPDATE tasks SET parent_task_id = NULL
         WHERE parent_task_id IN (SELECT id FROM tasks WHERE run_id IN (SELECT id FROM runs WHERE thread_id = $1))",
        "DELETE FROM tasks WHERE run_id IN (SELECT id FROM runs WHERE thread_id = $1)",
    ] {
        sqlx::query(query)
            .bind(thread_id)
            .execute(&mut **tx)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
    }

    // Plans and approval continuations hang off runs/plans and are not all
    // cascaded in the legacy schema. Remove the leaf records first.
    for query in [
        "DELETE FROM approval_continuation_outcomes
         WHERE receipt_id IN (
             SELECT receipt_id FROM approval_continuation_receipts
             WHERE run_id IN (SELECT id FROM runs WHERE thread_id = $1)
                OR approval_id IN (
                    SELECT id FROM approvals
                    WHERE run_id IN (SELECT id FROM runs WHERE thread_id = $1)
                       OR plan_id IN (SELECT id FROM plans WHERE thread_id = $1)
                )
         )",
        "DELETE FROM approval_continuation_descriptors
         WHERE run_id IN (SELECT id FROM runs WHERE thread_id = $1)
            OR approval_id IN (
                SELECT id FROM approvals
                WHERE run_id IN (SELECT id FROM runs WHERE thread_id = $1)
                   OR plan_id IN (SELECT id FROM plans WHERE thread_id = $1)
            )",
        "DELETE FROM approval_continuation_receipts
         WHERE run_id IN (SELECT id FROM runs WHERE thread_id = $1)
            OR approval_id IN (
                SELECT id FROM approvals
                WHERE run_id IN (SELECT id FROM runs WHERE thread_id = $1)
                   OR plan_id IN (SELECT id FROM plans WHERE thread_id = $1)
            )",
        "DELETE FROM approval_delivery_outbox
         WHERE run_id IN (SELECT id FROM runs WHERE thread_id = $1)
            OR approval_id IN (
                SELECT id FROM approvals
                WHERE run_id IN (SELECT id FROM runs WHERE thread_id = $1)
                   OR plan_id IN (SELECT id FROM plans WHERE thread_id = $1)
            )",
        "DELETE FROM approvals
         WHERE run_id IN (SELECT id FROM runs WHERE thread_id = $1)
            OR plan_id IN (SELECT id FROM plans WHERE thread_id = $1)",
        "DELETE FROM todos
         WHERE thread_id = $1
            OR plan_id IN (SELECT id FROM plans WHERE thread_id = $1)",
        "DELETE FROM plan_steps WHERE plan_id IN (SELECT id FROM plans WHERE thread_id = $1)",
        "DELETE FROM plans WHERE thread_id = $1",
    ] {
        sqlx::query(query)
            .bind(thread_id)
            .execute(&mut **tx)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
    }

    // Remaining run-owned records. The two tables with explicit cascades are
    // listed too, so the erasure contract stays obvious if their FK policy is
    // ever relaxed in a later migration.
    for query in [
        "DELETE FROM subagent_edges
         WHERE parent_run_id IN (SELECT id FROM runs WHERE thread_id = $1)
            OR child_run_id IN (SELECT id FROM runs WHERE thread_id = $1)",
        "DELETE FROM checkpoints WHERE run_id IN (SELECT id FROM runs WHERE thread_id = $1)",
        "DELETE FROM session_tool_audit_intents WHERE run_id IN (SELECT id FROM runs WHERE thread_id = $1)",
        "DELETE FROM managed_run_terminalization_outbox WHERE run_id IN (SELECT id FROM runs WHERE thread_id = $1)",
        "DELETE FROM runs WHERE thread_id = $1",
        "DELETE FROM messages WHERE thread_id = $1",
    ] {
        sqlx::query(query)
            .bind(thread_id)
            .execute(&mut **tx)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
    }

    Ok(erased_memories)
}

/// Require that `thread_id` is owned by EXACTLY `owner` in the caller's org.
///
/// [`authorize_owner_row`] is org-only for a service caller, which is right for
/// the ordinary service paths but too weak when a service is about to create a
/// row it will own: without this, any allowlisted workload could start a system
/// run inside an unrelated person's thread, and that thread's conversation would
/// then be feeding a run nobody in the UI can see.
#[allow(clippy::result_large_err)]
async fn authorize_thread_owner_exact(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    thread_id: &str,
    owner: &str,
) -> Result<(), Status> {
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT org_id, user_id FROM threads WHERE id = $1")
            .bind(thread_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
    let (org_id, user_id) = row.ok_or_else(|| Status::not_found("thread not found"))?;
    caller.authorize_org(&org_id)?;
    if user_id != owner {
        return Err(Status::permission_denied(
            "a system-owned run requires a thread owned by the same principal",
        ));
    }
    Ok(())
}

/// Require that a non-empty `parent_run_id` names a run owned by exactly `owner`.
///
/// `start_run_inner` binds the parent through `NULLIF($3, '')` with no ownership
/// check of its own, so this is the only thing standing between a system run and
/// a person's run tree.
#[allow(clippy::result_large_err)]
async fn authorize_parent_run_owner_exact(
    pool: &PgPool,
    parent_run_id: &str,
    org_id: &str,
    owner: &str,
) -> Result<(), Status> {
    if parent_run_id.trim().is_empty() {
        return Ok(());
    }
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT org_id, user_id FROM runs WHERE id = $1")
            .bind(parent_run_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
    let (parent_org, parent_owner) = row.ok_or_else(|| Status::not_found("run not found"))?;
    if parent_org != org_id || parent_owner != owner {
        return Err(Status::permission_denied(
            "a system-owned run cannot be attached to another principal's run",
        ));
    }
    Ok(())
}

async fn authorize_run_owner(
    pool: &PgPool,
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

/// Insert the `THREAD_CREATED` event row for a freshly created thread, within
/// the caller's open transaction.
async fn insert_thread_created_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    thread_id: &str,
    req: &pb::CreateThreadRequest,
    now: chrono::DateTime<Utc>,
) -> Result<(), Status> {
    let thread_resource = format!("thread:{thread_id}");
    let thread_idem = derive_idempotency_hash(
        "session-core",
        "THREAD_CREATED",
        &thread_resource,
        &format!("{thread_id}:created"),
    );
    sqlx::query(
        "INSERT INTO events (id, event_type, run_id, payload, ts, org_id, user_id, correlation_id, causation_id, idempotency_key, resource_ref, type_url, producer, schema_version)
         VALUES ($1, 'THREAD_CREATED', $2, $3, $4, $5, $6, $7, '', $8, $9, $10, 'session-core', 1)",
    )
    .bind(new_ulid())
    .bind(thread_id)
    .bind(serde_json::json!({
        "thread_id": thread_id,
        "session_key": &req.session_key,
        "space_id": &req.space_id,
        "space_decision_ref": &req.space_decision_ref,
        "recipient_audience_ref": &req.recipient_audience_ref,
        "recipient_audience_revision": req.recipient_audience_revision,
        "recipient_audience_hash": &req.recipient_audience_hash,
        "privacy_policy_ref": &req.privacy_policy_ref,
        "resource_authorization_ref": &req.resource_authorization_ref,
        "authority_revision": req.authority_revision,
        // The signed decision itself is a bearer artifact and must not enter
        // durable transcript/audit payloads. These three non-secret bindings
        // retain the exact authorized effect for replay and investigation.
        "action_schema_hash": &req.action_schema_hash,
        "payload_digest": &req.payload_digest,
        "idempotency_key": &req.idempotency_key,
    }))
    .bind(now)
    .bind(&req.org_id)
    .bind(&req.user_id)
    .bind(thread_id)
    .bind(&thread_idem)
    .bind(&thread_resource)
    .bind(THREAD_CREATED_TYPE_URL)
    .execute(&mut **tx)
    .await
    .map_err(|e| Status::internal(e.to_string()))?;
    Ok(())
}

#[derive(Debug, serde::Deserialize)]
struct ControlSpaceDecisionClaims {
    decision_ref: String,
    org_id: String,
    space_ref: String,
    subject_id: String,
    service_audience: String,
    action_id: String,
    action_schema_hash: String,
    payload_digest: String,
    idempotency_key: String,
    recipient_audience_ref: String,
    recipient_audience_revision: u64,
    recipient_audience_hash: String,
    privacy_policy_ref: String,
    resource_authorization_ref: String,
    purpose: String,
    lawful_basis: String,
    privacy_class: String,
    third_party_processing_allowed: bool,
    retention_class: String,
    residency: String,
    deletion_scope: String,
    zero_data_retention: bool,
    nonce: String,
    authority_revision: u64,
    membership_revision: u64,
    privacy_revision: u64,
    entitlement_revision: u64,
    permissions: Vec<String>,
    issued_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

fn scheduled_run_payload_digest(
    req: &pb::PrepareScheduledRunThreadRequest,
    claims: &ControlSpaceDecisionClaims,
) -> String {
    let mut digest = Sha256::new();
    digest.update(b"model.schedule.run\0v1\0");
    for (name, value) in [
        ("org_id", req.org_id.as_str()),
        ("user_id", req.human_subject_id.as_str()),
        ("space_id", req.space_id.as_str()),
        ("schedule_id", req.schedule_id.as_str()),
        ("fire_key", req.fire_key.as_str()),
        ("run_id", req.run_id.as_str()),
        ("system_thread_key", req.system_thread_key.as_str()),
        ("template_digest", req.template_digest.as_str()),
        (
            "recipient_audience_ref",
            req.recipient_audience_ref.as_str(),
        ),
        (
            "recipient_audience_hash",
            req.recipient_audience_hash.as_str(),
        ),
        ("privacy_policy_ref", req.privacy_policy_ref.as_str()),
        (
            "resource_authorization_ref",
            req.resource_authorization_ref.as_str(),
        ),
        ("action_schema_hash", req.action_schema_hash.as_str()),
        ("idempotency_key", req.idempotency_key.as_str()),
    ] {
        digest.update(name.as_bytes());
        digest.update([0]);
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
    }
    for (name, value) in [
        ("authority_revision", req.authority_revision),
        ("membership_revision", claims.membership_revision),
        ("privacy_revision", claims.privacy_revision),
        (
            "recipient_audience_revision",
            req.recipient_audience_revision,
        ),
        ("entitlement_revision", claims.entitlement_revision),
    ] {
        digest.update(name.as_bytes());
        digest.update([0]);
        digest.update(value.to_be_bytes());
    }
    format!("sha256:{:x}", digest.finalize())
}

fn validate_scheduled_run_preparation_bindings(
    req: &pb::PrepareScheduledRunThreadRequest,
) -> Result<(), Status> {
    if req.control_decision_token.len() > MAX_CONTROL_SPACE_DECISION_TOKEN_BYTES {
        return Err(Status::invalid_argument(
            "Control scheduled-run decision is too large",
        ));
    }
    let expected_thread_key = format!("schedule/{}/{}", req.schedule_id, req.fire_key);
    if req.org_id.trim().is_empty()
        || req.human_subject_id.trim().is_empty()
        || req.run_id.trim().is_empty()
        || req.schedule_id.contains('/')
        || req.fire_key.contains('/')
        || req.system_thread_key != expected_thread_key
        || req.action_schema_hash != CONTROL_SCHEDULED_RUN_SCHEMA
        || req.authority_revision == 0
        || req.recipient_audience_revision == 0
    {
        return Err(Status::invalid_argument(
            "scheduled-run preparation bindings are invalid",
        ));
    }
    Ok(())
}

fn verify_scheduled_run_decision(req: &pb::PrepareScheduledRunThreadRequest) -> Result<(), Status> {
    // Preserve boundary-validation precedence: malformed requests should not
    // depend on Control verifier configuration before being rejected.
    validate_scheduled_run_preparation_bindings(req)?;
    let keys = configured_control_space_decision_keys()?;
    verify_scheduled_run_decision_with_keys(req, &keys, Utc::now())
}

fn verify_scheduled_run_decision_with_keys(
    req: &pb::PrepareScheduledRunThreadRequest,
    keys: &BTreeMap<String, VerifyingKey>,
    now: DateTime<Utc>,
) -> Result<(), Status> {
    validate_scheduled_run_preparation_bindings(req)?;
    let parts: Vec<&str> = req.control_decision_token.split('.').collect();
    if parts.len() != 4 || parts[0] != CONTROL_SPACE_DECISION_VERSION {
        return Err(Status::permission_denied(
            "invalid Control scheduled-run decision envelope",
        ));
    }
    let key_id = URL_SAFE_NO_PAD
        .decode(parts[1])
        .ok()
        .and_then(|raw| String::from_utf8(raw).ok())
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| Status::permission_denied("untrusted Control Space decision key"))?;
    let key = keys
        .get(&key_id)
        .ok_or_else(|| Status::permission_denied("untrusted Control Space decision key"))?;
    let payload = URL_SAFE_NO_PAD
        .decode(parts[2])
        .map_err(|_| Status::permission_denied("invalid Control scheduled-run decision payload"))?;
    let signature = URL_SAFE_NO_PAD.decode(parts[3]).map_err(|_| {
        Status::permission_denied("invalid Control scheduled-run decision signature")
    })?;
    let signature = Signature::from_slice(&signature).map_err(|_| {
        Status::permission_denied("invalid Control scheduled-run decision signature")
    })?;
    key.verify(
        format!("{}.{}.{}", parts[0], parts[1], parts[2]).as_bytes(),
        &signature,
    )
    .map_err(|_| Status::permission_denied("invalid Control scheduled-run decision signature"))?;
    let claims: ControlSpaceDecisionClaims = serde_json::from_slice(&payload)
        .map_err(|_| Status::permission_denied("invalid Control scheduled-run decision claims"))?;
    let matches = claims.decision_ref == req.space_decision_ref
        && claims.org_id == req.org_id
        && claims.space_ref == req.space_id
        && claims.subject_id == req.human_subject_id
        && claims.service_audience == CONTROL_SCHEDULED_RUN_AUDIENCE
        && claims.action_id == CONTROL_SCHEDULED_RUN_ACTION
        && claims.action_schema_hash == req.action_schema_hash
        && claims.idempotency_key == req.idempotency_key
        && claims.recipient_audience_ref == req.recipient_audience_ref
        && claims.recipient_audience_revision == req.recipient_audience_revision
        && claims.recipient_audience_hash == req.recipient_audience_hash
        && claims.privacy_policy_ref == req.privacy_policy_ref
        && claims.resource_authorization_ref == req.resource_authorization_ref
        && claims.authority_revision == req.authority_revision
        && claims.payload_digest == req.payload_digest
        && claims.payload_digest == scheduled_run_payload_digest(req, &claims)
        && claims
            .permissions
            .iter()
            .any(|permission| permission == "schedule:run")
        && !claims.zero_data_retention;
    if !matches
        || claims.nonce.trim().is_empty()
        || claims.expires_at <= now
        || claims.issued_at > now + chrono::Duration::minutes(1)
    {
        return Err(Status::permission_denied(
            "Control decision does not authorize this scheduled run",
        ));
    }
    Ok(())
}

#[derive(Debug)]
struct ScheduledRunThreadContext {
    space_id: String,
    recipient_audience_ref: String,
    recipient_audience_revision: u64,
    recipient_audience_hash: String,
    privacy_policy_ref: String,
    resource_authorization_ref: String,
    authority_revision: u64,
}

fn scheduled_run_execution_payload_digest(
    req: &pb::StartScheduledRunRequest,
    claims: &ControlSpaceDecisionClaims,
) -> String {
    let mut digest = Sha256::new();
    digest.update(b"model.schedule.execute\0v1\0");
    let system_thread_key = format!("schedule/{}/{}", req.schedule_id, req.fire_key);
    for (name, value) in [
        ("org_id", req.org_id.as_str()),
        ("user_id", req.human_subject_id.as_str()),
        ("space_id", claims.space_ref.as_str()),
        ("schedule_id", req.schedule_id.as_str()),
        ("fire_key", req.fire_key.as_str()),
        ("run_id", req.run_id.as_str()),
        ("system_thread_key", system_thread_key.as_str()),
        ("template_digest", req.template_digest.as_str()),
        (
            "recipient_audience_ref",
            claims.recipient_audience_ref.as_str(),
        ),
        (
            "recipient_audience_hash",
            claims.recipient_audience_hash.as_str(),
        ),
        ("privacy_policy_ref", claims.privacy_policy_ref.as_str()),
        (
            "resource_authorization_ref",
            claims.resource_authorization_ref.as_str(),
        ),
        ("action_schema_hash", CONTROL_SCHEDULED_RUN_EXECUTION_SCHEMA),
        ("thread_id", req.thread_id.as_str()),
        ("idempotency_key", req.idempotency_key.as_str()),
    ] {
        digest.update(name.as_bytes());
        digest.update([0]);
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
    }
    for (name, value) in [
        ("authority_revision", claims.authority_revision),
        ("membership_revision", claims.membership_revision),
        ("privacy_revision", claims.privacy_revision),
        (
            "recipient_audience_revision",
            claims.recipient_audience_revision,
        ),
        ("entitlement_revision", claims.entitlement_revision),
    ] {
        digest.update(name.as_bytes());
        digest.update([0]);
        digest.update(value.to_be_bytes());
    }
    format!("sha256:{:x}", digest.finalize())
}

fn verify_scheduled_run_execution_decision(
    req: &pb::StartScheduledRunRequest,
    thread: &ScheduledRunThreadContext,
) -> Result<(), Status> {
    let keys = configured_control_space_decision_keys()?;
    verify_scheduled_run_execution_decision_with_keys(req, thread, &keys, Utc::now())
}

fn verify_scheduled_run_execution_decision_with_keys(
    req: &pb::StartScheduledRunRequest,
    thread: &ScheduledRunThreadContext,
    keys: &BTreeMap<String, VerifyingKey>,
    now: DateTime<Utc>,
) -> Result<(), Status> {
    if req.control_execution_decision_token.len() > MAX_CONTROL_SPACE_DECISION_TOKEN_BYTES {
        return Err(Status::invalid_argument(
            "Control scheduled-run execution decision is too large",
        ));
    }
    if !valid_scheduled_run_identifier(&req.run_id)
        || !valid_scheduled_run_identifier(&req.schedule_id)
        || !valid_scheduled_run_identifier(&req.fire_key)
        || req.thread_id.trim().is_empty()
        || req.human_subject_id.trim().is_empty()
        || req.idempotency_key.trim().is_empty()
        || !req.template_digest.starts_with("sha256:")
        || req.template_digest.len() != "sha256:".len() + 64
    {
        return Err(Status::invalid_argument(
            "scheduled-run execution bindings are invalid",
        ));
    }
    let parts: Vec<&str> = req.control_execution_decision_token.split('.').collect();
    if parts.len() != 4 || parts[0] != CONTROL_SPACE_DECISION_VERSION {
        return Err(Status::permission_denied(
            "invalid Control scheduled-run execution decision envelope",
        ));
    }
    let key_id = URL_SAFE_NO_PAD
        .decode(parts[1])
        .ok()
        .and_then(|raw| String::from_utf8(raw).ok())
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| Status::permission_denied("untrusted Control Space decision key"))?;
    let key = keys
        .get(&key_id)
        .ok_or_else(|| Status::permission_denied("untrusted Control Space decision key"))?;
    let payload = URL_SAFE_NO_PAD.decode(parts[2]).map_err(|_| {
        Status::permission_denied("invalid Control scheduled-run execution decision payload")
    })?;
    let signature = URL_SAFE_NO_PAD.decode(parts[3]).map_err(|_| {
        Status::permission_denied("invalid Control scheduled-run execution decision signature")
    })?;
    let signature = Signature::from_slice(&signature).map_err(|_| {
        Status::permission_denied("invalid Control scheduled-run execution decision signature")
    })?;
    key.verify(
        format!("{}.{}.{}", parts[0], parts[1], parts[2]).as_bytes(),
        &signature,
    )
    .map_err(|_| {
        Status::permission_denied("invalid Control scheduled-run execution decision signature")
    })?;
    let claims: ControlSpaceDecisionClaims = serde_json::from_slice(&payload).map_err(|_| {
        Status::permission_denied("invalid Control scheduled-run execution decision claims")
    })?;
    let matches = claims.org_id == req.org_id
        && claims.space_ref == thread.space_id
        && claims.subject_id == req.human_subject_id
        && claims.service_audience == CONTROL_SCHEDULED_RUN_EXECUTION_AUDIENCE
        && claims.action_id == CONTROL_SCHEDULED_RUN_EXECUTION_ACTION
        && claims.action_schema_hash == CONTROL_SCHEDULED_RUN_EXECUTION_SCHEMA
        && claims.idempotency_key == req.idempotency_key
        && claims.recipient_audience_ref == thread.recipient_audience_ref
        && claims.recipient_audience_revision == thread.recipient_audience_revision
        && claims.recipient_audience_hash == thread.recipient_audience_hash
        && claims.privacy_policy_ref == thread.privacy_policy_ref
        && claims.resource_authorization_ref == thread.resource_authorization_ref
        && claims.authority_revision == thread.authority_revision
        && claims.payload_digest == scheduled_run_execution_payload_digest(req, &claims)
        && claims
            .permissions
            .iter()
            .any(|permission| permission == "schedule:execute")
        && !claims.zero_data_retention;
    if !matches
        || claims.decision_ref.trim().is_empty()
        || claims.nonce.trim().is_empty()
        || claims.expires_at <= now
        || claims.issued_at > now + chrono::Duration::minutes(1)
    {
        return Err(Status::permission_denied(
            "Control decision does not authorize this scheduled-run execution",
        ));
    }
    Ok(())
}

fn valid_scheduled_run_identifier(value: &str) -> bool {
    !value.trim().is_empty()
        && value == value.trim()
        && value.len() <= 256
        && !value.contains(['/', '.', '*', '>', ' ', '\t', '\r', '\n'])
}

fn valid_scheduled_step_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[allow(clippy::result_large_err)]
fn validate_scheduled_step_request(req: &pb::ClaimScheduledStepRequest) -> Result<(), Status> {
    if !valid_scheduled_run_identifier(&req.run_id)
        || !valid_scheduled_run_identifier(&req.thread_id)
        || !valid_scheduled_run_identifier(&req.org_id)
        || !valid_scheduled_run_identifier(&req.space_id)
        || !valid_scheduled_run_identifier(&req.schedule_id)
        || !valid_scheduled_run_identifier(&req.fire_key)
        || !valid_scheduled_run_identifier(&req.step_id)
        || !valid_scheduled_run_identifier(&req.idempotency_key)
        || !valid_scheduled_step_digest(&req.template_digest)
        || !valid_scheduled_step_digest(&req.policy_digest)
    {
        return Err(Status::invalid_argument(
            "scheduled-step bindings are invalid",
        ));
    }
    Ok(())
}

fn scheduled_step_metadata_matches(
    metadata: &serde_json::Value,
    req: &pb::ClaimScheduledStepRequest,
) -> bool {
    [
        ("schedule_id", req.schedule_id.as_str()),
        ("fire_key", req.fire_key.as_str()),
        ("template_digest", req.template_digest.as_str()),
        ("space_id", req.space_id.as_str()),
    ]
    .into_iter()
    .all(|(name, expected)| {
        metadata.get(name).and_then(serde_json::Value::as_str) == Some(expected)
    })
}

#[allow(clippy::result_large_err)]
async fn claim_scheduled_step_inner(
    pool: &PgPool,
    req: pb::ClaimScheduledStepRequest,
) -> Result<Response<pb::ClaimScheduledStepResponse>, Status> {
    validate_scheduled_step_request(&req)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
    let run: Option<(String, String, String, String, String, serde_json::Value)> = sqlx::query_as(
        "SELECT r.org_id, r.thread_id, r.user_id, r.status, COALESCE(t.space_id, ''), r.metadata
         FROM runs r JOIN threads t ON t.id = r.thread_id
         WHERE r.id = $1 AND r.org_id = $2
         FOR UPDATE OF r, t",
    )
    .bind(&req.run_id)
    .bind(&req.org_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| Status::internal(error.to_string()))?;
    let Some((org_id, thread_id, owner, status, stored_space_id, metadata)) = run else {
        return Err(Status::not_found("scheduled run not found"));
    };
    if thread_id != req.thread_id
        || owner != crate::auth::system_run_owners()[0]
        || stored_space_id != req.space_id
        || !scheduled_step_metadata_matches(&metadata, &req)
    {
        return Err(Status::permission_denied(
            "scheduled step does not match the prepared run",
        ));
    }
    if is_terminal_run_status(&status) {
        return Err(Status::failed_precondition(
            "scheduled run is already terminal",
        ));
    }

    let inserted: Option<(String, String)> = sqlx::query_as(
        "INSERT INTO scheduled_step_receipts
             (run_id, step_id, org_id, thread_id, space_id, schedule_id, fire_key,
              template_digest, step_index, policy_digest, idempotency_key, receipt_id, status)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,'claimed')
         ON CONFLICT (run_id, step_id) DO NOTHING
         RETURNING receipt_id, status",
    )
    .bind(&req.run_id)
    .bind(&req.step_id)
    .bind(&org_id)
    .bind(&req.thread_id)
    .bind(&req.space_id)
    .bind(&req.schedule_id)
    .bind(&req.fire_key)
    .bind(&req.template_digest)
    .bind(i64::from(req.step_index))
    .bind(&req.policy_digest)
    .bind(&req.idempotency_key)
    .bind(format!("scheduled_step_{}", new_ulid()))
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| {
        if error
            .to_string()
            .contains("scheduled_step_receipts_org_id_idempotency_key_key")
        {
            Status::already_exists("scheduled-step idempotency key is bound to another step")
        } else {
            Status::internal(error.to_string())
        }
    })?;

    let (receipt_id, status, claimed) = match inserted {
        Some((receipt_id, status)) => (receipt_id, status, true),
        None => {
            let existing: Option<(
                String,
                String,
                String,
                String,
                String,
                String,
                i64,
                String,
                String,
            )> = sqlx::query_as(
                "SELECT receipt_id, status, thread_id, space_id, schedule_id, fire_key,
                        step_index, template_digest, policy_digest
                 FROM scheduled_step_receipts
                 WHERE run_id = $1 AND step_id = $2
                 FOR UPDATE",
            )
            .bind(&req.run_id)
            .bind(&req.step_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
            let Some((
                receipt_id,
                status,
                thread_id,
                space_id,
                schedule_id,
                fire_key,
                step_index,
                template_digest,
                policy_digest,
            )) = existing
            else {
                return Err(Status::already_exists(
                    "scheduled-step identity is already in use",
                ));
            };
            if thread_id != req.thread_id
                || space_id != req.space_id
                || schedule_id != req.schedule_id
                || fire_key != req.fire_key
                || step_index != i64::from(req.step_index)
                || template_digest != req.template_digest
                || policy_digest != req.policy_digest
            {
                return Err(Status::already_exists(
                    "scheduled-step identity is bound to different immutable input",
                ));
            }
            (receipt_id, status, false)
        }
    };
    tx.commit()
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
    Ok(Response::new(pb::ClaimScheduledStepResponse {
        claimed,
        receipt_id,
        status,
    }))
}

#[allow(clippy::result_large_err)]
async fn record_scheduled_step_receipt_inner(
    pool: &PgPool,
    req: pb::RecordScheduledStepReceiptRequest,
) -> Result<Response<pb::RecordScheduledStepReceiptResponse>, Status> {
    if !valid_scheduled_run_identifier(&req.run_id)
        || !valid_scheduled_run_identifier(&req.step_id)
        || !valid_scheduled_run_identifier(&req.org_id)
        || !valid_scheduled_run_identifier(&req.idempotency_key)
        || !valid_scheduled_run_identifier(&req.receipt_id)
    {
        return Err(Status::invalid_argument(
            "scheduled-step receipt bindings are invalid",
        ));
    }
    let allowed = matches!(
        req.status.as_str(),
        "completed" | "failed" | "unknown_outcome"
    );
    if !allowed || (req.unknown_outcome != (req.status == "unknown_outcome")) {
        return Err(Status::invalid_argument(
            "invalid scheduled-step receipt status",
        ));
    }
    if !req.output_digest.is_empty() && !valid_scheduled_step_digest(&req.output_digest) {
        return Err(Status::invalid_argument(
            "invalid scheduled-step output digest",
        ));
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
    let existing: Option<(String, String, String)> = sqlx::query_as(
        "SELECT receipt_id, status, idempotency_key
         FROM scheduled_step_receipts
         WHERE run_id = $1 AND step_id = $2 AND org_id = $3
         FOR UPDATE",
    )
    .bind(&req.run_id)
    .bind(&req.step_id)
    .bind(&req.org_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| Status::internal(error.to_string()))?;
    let Some((receipt_id, current_status, idempotency_key)) = existing else {
        return Err(Status::not_found("scheduled-step receipt not found"));
    };
    if receipt_id != req.receipt_id || idempotency_key != req.idempotency_key {
        return Err(Status::permission_denied(
            "scheduled-step receipt binding mismatch",
        ));
    }
    if current_status != "claimed" {
        if current_status == req.status {
            tx.commit()
                .await
                .map_err(|error| Status::internal(error.to_string()))?;
            return Ok(Response::new(pb::RecordScheduledStepReceiptResponse {
                receipt_id,
                status: current_status,
                recorded: false,
            }));
        }
        return Err(Status::already_exists(
            "scheduled-step receipt already has a different terminal outcome",
        ));
    }
    sqlx::query(
        "UPDATE scheduled_step_receipts
         SET status = $4, output_digest = $5, error_code = $6,
             unknown_outcome = $7, updated_at = now()
         WHERE run_id = $1 AND step_id = $2 AND org_id = $3",
    )
    .bind(&req.run_id)
    .bind(&req.step_id)
    .bind(&req.org_id)
    .bind(&req.status)
    .bind(&req.output_digest)
    .bind(&req.error_code)
    .bind(req.unknown_outcome)
    .execute(&mut *tx)
    .await
    .map_err(|error| Status::internal(error.to_string()))?;
    tx.commit()
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
    Ok(Response::new(pb::RecordScheduledStepReceiptResponse {
        receipt_id,
        status: req.status,
        recorded: true,
    }))
}

/// Bind the non-secret schedule identity to the deterministic run row. This
/// is separate from `StartRunRequest` so generic user runs cannot populate the
/// schedule namespace. A retry may observe the exact same binding, but a
/// different schedule/fire/template/policy can never retarget the run id.
#[allow(clippy::result_large_err)]
async fn persist_scheduled_run_bindings(
    pool: &PgPool,
    req: &pb::StartScheduledRunRequest,
) -> Result<(), Status> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
    let existing: Option<(String, String, serde_json::Value)> = sqlx::query_as(
        "SELECT r.org_id, COALESCE(t.space_id, ''), r.metadata
         FROM runs r JOIN threads t ON t.id = r.thread_id
         WHERE r.id = $1 AND r.thread_id = $2
           AND r.org_id = $3 AND r.user_id = $4
         FOR UPDATE OF r, t",
    )
    .bind(&req.run_id)
    .bind(&req.thread_id)
    .bind(&req.org_id)
    .bind(crate::auth::system_run_owners()[0])
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| Status::internal(error.to_string()))?;
    let Some((org_id, space_id, metadata)) = existing else {
        return Err(Status::permission_denied(
            "scheduled run owner binding is unavailable",
        ));
    };
    let expected = serde_json::json!({
        "source": "scheduled_run",
        "schedule_id": req.schedule_id,
        "fire_key": req.fire_key,
        "space_id": space_id,
        "subject_id": req.human_subject_id,
        "template_digest": req.template_digest,
        "policy_digest": req.policy_digest,
        "idempotency_key": req.idempotency_key,
    });
    let exact = [
        "schedule_id",
        "fire_key",
        "space_id",
        "subject_id",
        "template_digest",
        "policy_digest",
        "idempotency_key",
    ]
    .into_iter()
    .all(|key| metadata.get(key) == expected.get(key));
    if !metadata.is_object() || metadata.as_object().is_none() || metadata == serde_json::json!({})
    {
        sqlx::query(
            "UPDATE runs SET metadata = $2, updated_at = now() WHERE id = $1 AND org_id = $3",
        )
        .bind(&req.run_id)
        .bind(&expected)
        .bind(&org_id)
        .execute(&mut *tx)
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
    } else if !exact {
        return Err(Status::permission_denied(
            "scheduled run id is bound to different immutable schedule metadata",
        ));
    }
    tx.commit()
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
    Ok(())
}

/// Thread Space context is an all-or-nothing authority envelope. It is kept
/// distinct from `workspace_id`, which remains a content-selection hint only.
/// Any populated Space envelope additionally requires an operation-bound,
/// Ed25519-signed Control decision. Unscoped legacy threads are still allowed
/// only when *every* Space/binding field is empty.
fn validate_thread_space_context_shape(req: &pb::CreateThreadRequest) -> Result<bool, Status> {
    let values = [
        req.space_id.trim(),
        req.space_decision_ref.trim(),
        req.recipient_audience_ref.trim(),
        req.recipient_audience_hash.trim(),
        req.privacy_policy_ref.trim(),
        req.resource_authorization_ref.trim(),
        req.space_decision_token.trim(),
        req.action_schema_hash.trim(),
        req.payload_digest.trim(),
        req.idempotency_key.trim(),
    ];
    let present = values.iter().filter(|value| !value.is_empty()).count();
    if present == 0 && req.authority_revision == 0 && req.recipient_audience_revision == 0 {
        return Ok(false);
    }
    if present != values.len()
        || req.authority_revision == 0
        || req.recipient_audience_revision == 0
    {
        return Err(Status::invalid_argument(
            "Space context requires complete signed decision and operation bindings",
        ));
    }
    Ok(true)
}

/// An append uses the same complete, non-secret Space envelope as thread
/// creation, but a distinct action/schema/payload binding. A missing envelope
/// is only permitted for an unscoped thread (or the bootstrap first message
/// atomically following its already-verified create); it must never silently
/// downgrade a later scoped append.
fn validate_append_space_context_shape(req: &pb::AppendMessageRequest) -> Result<bool, Status> {
    let values = [
        req.space_id.trim(),
        req.space_decision_ref.trim(),
        req.recipient_audience_ref.trim(),
        req.recipient_audience_hash.trim(),
        req.privacy_policy_ref.trim(),
        req.resource_authorization_ref.trim(),
        req.space_decision_token.trim(),
        req.action_schema_hash.trim(),
        req.payload_digest.trim(),
        req.idempotency_key.trim(),
    ];
    let present = values.iter().filter(|value| !value.is_empty()).count();
    if present == 0 && req.authority_revision == 0 && req.recipient_audience_revision == 0 {
        return Ok(false);
    }
    if present != values.len()
        || req.authority_revision == 0
        || req.recipient_audience_revision == 0
    {
        return Err(Status::invalid_argument(
            "Space append requires a complete signed decision and operation bindings",
        ));
    }
    Ok(true)
}

/// Loads the current Control verification key set. During rotation the
/// deployment supplies `CONTROL_SPACE_DECISION_PUBLIC_KEYS_JSON` as a JSON map
/// of `{ key_id: url_safe_base64_ed25519_public_key }`; any token bearing an
/// unlisted key id fails closed. The singular variables remain a deliberately
/// compatible bootstrap form for deployments that have not started rotation.
fn configured_control_space_decision_keys() -> Result<BTreeMap<String, VerifyingKey>, Status> {
    if let Ok(raw) = env::var("CONTROL_SPACE_DECISION_PUBLIC_KEYS_JSON") {
        let encoded_keys: BTreeMap<String, String> = serde_json::from_str(&raw).map_err(|_| {
            Status::failed_precondition("Control Space decision public key set is invalid")
        })?;
        if encoded_keys.is_empty() {
            return Err(Status::failed_precondition(
                "Control Space decision public key set is empty",
            ));
        }
        let mut keys = BTreeMap::new();
        for (key_id, encoded) in encoded_keys {
            let key_id = key_id.trim();
            let encoded = encoded.trim();
            if key_id.is_empty() || encoded.is_empty() || encoded.chars().any(char::is_whitespace) {
                return Err(Status::failed_precondition(
                    "Control Space decision key configuration is invalid",
                ));
            }
            let bytes = URL_SAFE_NO_PAD.decode(encoded).map_err(|_| {
                Status::failed_precondition("Control Space decision public key is invalid")
            })?;
            let bytes: [u8; 32] = bytes.try_into().map_err(|_| {
                Status::failed_precondition("Control Space decision public key length is invalid")
            })?;
            let key = VerifyingKey::from_bytes(&bytes).map_err(|_| {
                Status::failed_precondition("Control Space decision public key is invalid")
            })?;
            if keys.insert(key_id.to_owned(), key).is_some() {
                return Err(Status::failed_precondition(
                    "Control Space decision public key IDs are not unique",
                ));
            }
        }
        return Ok(keys);
    }

    let key_id = env::var("CONTROL_SPACE_DECISION_KEY_ID").map_err(|_| {
        Status::failed_precondition("Control Space decision key ID is not configured")
    })?;
    let encoded = env::var("CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64").map_err(|_| {
        Status::failed_precondition("Control Space decision public key is not configured")
    })?;
    let key_id = key_id.trim();
    let encoded = encoded.trim();
    if key_id.is_empty() || encoded.is_empty() || encoded.chars().any(char::is_whitespace) {
        return Err(Status::failed_precondition(
            "Control Space decision key configuration is invalid",
        ));
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| Status::failed_precondition("Control Space decision public key is invalid"))?;
    let bytes: [u8; 32] = bytes.try_into().map_err(|_| {
        Status::failed_precondition("Control Space decision public key length is invalid")
    })?;
    let key = VerifyingKey::from_bytes(&bytes)
        .map_err(|_| Status::failed_precondition("Control Space decision public key is invalid"))?;
    Ok(BTreeMap::from([(key_id.to_owned(), key)]))
}

/// Canonical digest of every `CreateThread` field that changes the persisted
/// effect. The decision bearer and its digest are intentionally excluded: they
/// authorize this payload; they do not define it. Length-prefixing each value
/// makes the encoding unambiguous without relying on JSON map ordering.
fn thread_create_payload_digest(req: &pb::CreateThreadRequest) -> String {
    let mut digest = Sha256::new();
    digest.update(b"model.thread.create\0v1\0");
    for (name, value) in [
        ("org_id", req.org_id.as_str()),
        ("user_id", req.user_id.as_str()),
        ("session_key", req.session_key.as_str()),
        ("space_id", req.space_id.as_str()),
        ("space_decision_ref", req.space_decision_ref.as_str()),
        (
            "recipient_audience_ref",
            req.recipient_audience_ref.as_str(),
        ),
        (
            "recipient_audience_hash",
            req.recipient_audience_hash.as_str(),
        ),
        ("privacy_policy_ref", req.privacy_policy_ref.as_str()),
        (
            "resource_authorization_ref",
            req.resource_authorization_ref.as_str(),
        ),
        ("action_schema_hash", req.action_schema_hash.as_str()),
        ("idempotency_key", req.idempotency_key.as_str()),
    ] {
        digest.update(name.as_bytes());
        digest.update([0]);
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
    }
    digest.update(b"authority_revision\0");
    digest.update(req.authority_revision.to_be_bytes());
    digest.update(b"recipient_audience_revision\0");
    digest.update(req.recipient_audience_revision.to_be_bytes());
    let digest = digest.finalize();
    format!("sha256:{digest:x}")
}

fn thread_append_content_digest(content: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(content.as_bytes()))
}

/// Canonical digest shared with Control's `threadAppendPayloadDigest`. It is
/// deliberately computed from the final message bytes and the owner row,
/// rather than a BFF supplied digest, immediately before persistence.
fn thread_append_payload_digest(
    req: &pb::AppendMessageRequest,
    org_id: &str,
    user_id: &str,
) -> String {
    let mut digest = Sha256::new();
    digest.update(b"model.thread.append\0v1\0");
    let content_digest = thread_append_content_digest(&req.content);
    for (name, value) in [
        ("org_id", org_id),
        ("user_id", user_id),
        ("thread_id", req.thread_id.as_str()),
        ("space_id", req.space_id.as_str()),
        ("space_decision_ref", req.space_decision_ref.as_str()),
        (
            "recipient_audience_ref",
            req.recipient_audience_ref.as_str(),
        ),
        (
            "recipient_audience_hash",
            req.recipient_audience_hash.as_str(),
        ),
        ("privacy_policy_ref", req.privacy_policy_ref.as_str()),
        (
            "resource_authorization_ref",
            req.resource_authorization_ref.as_str(),
        ),
        ("content_digest", content_digest.as_str()),
        ("action_schema_hash", req.action_schema_hash.as_str()),
        ("idempotency_key", req.idempotency_key.as_str()),
    ] {
        digest.update(name.as_bytes());
        digest.update([0]);
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
    }
    digest.update(b"authority_revision\0");
    digest.update(req.authority_revision.to_be_bytes());
    digest.update(b"recipient_audience_revision\0");
    digest.update(req.recipient_audience_revision.to_be_bytes());
    format!("sha256:{:x}", digest.finalize())
}

fn verify_thread_space_decision_with_key(
    req: &pb::CreateThreadRequest,
    expected_key_id: &str,
    key: &VerifyingKey,
    now: DateTime<Utc>,
) -> Result<(), Status> {
    if req.space_decision_token.len() > MAX_CONTROL_SPACE_DECISION_TOKEN_BYTES {
        return Err(Status::invalid_argument(
            "Control Space decision is too large",
        ));
    }
    let parts: Vec<&str> = req.space_decision_token.split('.').collect();
    if parts.len() != 4 || parts[0] != CONTROL_SPACE_DECISION_VERSION {
        return Err(Status::permission_denied(
            "invalid Control Space decision envelope",
        ));
    }
    let _key_id = URL_SAFE_NO_PAD
        .decode(parts[1])
        .ok()
        .and_then(|raw| String::from_utf8(raw).ok())
        .filter(|id| id == expected_key_id)
        .ok_or_else(|| Status::permission_denied("untrusted Control Space decision key"))?;
    let payload = URL_SAFE_NO_PAD
        .decode(parts[2])
        .map_err(|_| Status::permission_denied("invalid Control Space decision payload"))?;
    let signature = URL_SAFE_NO_PAD
        .decode(parts[3])
        .map_err(|_| Status::permission_denied("invalid Control Space decision signature"))?;
    let signature = Signature::from_slice(&signature)
        .map_err(|_| Status::permission_denied("invalid Control Space decision signature"))?;
    let signing_input = format!("{}.{}.{}", parts[0], parts[1], parts[2]);
    key.verify(signing_input.as_bytes(), &signature)
        .map_err(|_| Status::permission_denied("invalid Control Space decision signature"))?;
    let claims: ControlSpaceDecisionClaims = serde_json::from_slice(&payload)
        .map_err(|_| Status::permission_denied("invalid Control Space decision claims"))?;
    let expected_payload_digest = thread_create_payload_digest(req);
    let matches_request = claims.decision_ref == req.space_decision_ref
        && claims.org_id == req.org_id
        && claims.space_ref == req.space_id
        && claims.subject_id == req.user_id
        && claims.service_audience == CONTROL_SPACE_DECISION_AUDIENCE
        && claims.action_id == CONTROL_THREAD_CREATE_ACTION
        && claims.action_schema_hash == req.action_schema_hash
        && claims.payload_digest == req.payload_digest
        && claims.payload_digest == expected_payload_digest
        && claims.idempotency_key == req.idempotency_key
        && claims.recipient_audience_ref == req.recipient_audience_ref
        && claims.recipient_audience_revision == req.recipient_audience_revision
        && claims.recipient_audience_hash == req.recipient_audience_hash
        && claims.privacy_policy_ref == req.privacy_policy_ref
        && claims.resource_authorization_ref == req.resource_authorization_ref
        && claims.authority_revision == req.authority_revision
        && claims
            .permissions
            .iter()
            .any(|permission| permission == "thread:create");
    if !matches_request {
        return Err(Status::permission_denied(
            "Control Space decision does not authorize this thread",
        ));
    }
    // A policy reference alone cannot establish a processing floor. Require the
    // signed decision to carry the complete privacy metadata before Model may
    // persist a thread that will later drive context, providers, or tools.
    let privacy_complete = [
        claims.purpose.as_str(),
        claims.lawful_basis.as_str(),
        claims.privacy_class.as_str(),
        claims.retention_class.as_str(),
        claims.residency.as_str(),
        claims.deletion_scope.as_str(),
    ]
    .iter()
    .all(|value| !value.trim().is_empty());
    // Boolean policy flags are required serde fields, so merely decoding the
    // claims establishes their presence; retain them for the later provider/
    // retention propagation slices without inventing a Model-local default.
    let _privacy_processing_flags = (
        claims.third_party_processing_allowed,
        claims.zero_data_retention,
    );
    if !privacy_complete || claims.nonce.trim().is_empty() {
        return Err(Status::permission_denied(
            "Control Space decision has incomplete privacy policy or nonce",
        ));
    }
    if claims.issued_at > now + chrono::Duration::minutes(1) || claims.expires_at <= now {
        return Err(Status::permission_denied(
            "Control Space decision is expired or not yet valid",
        ));
    }
    Ok(())
}

fn verify_thread_space_decision_with_keys(
    req: &pb::CreateThreadRequest,
    keys: &BTreeMap<String, VerifyingKey>,
    now: DateTime<Utc>,
) -> Result<(), Status> {
    let key_id = req
        .space_decision_token
        .split('.')
        .nth(1)
        .and_then(|part| URL_SAFE_NO_PAD.decode(part).ok())
        .and_then(|raw| String::from_utf8(raw).ok())
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| Status::permission_denied("untrusted Control Space decision key"))?;
    let key = keys
        .get(&key_id)
        .ok_or_else(|| Status::permission_denied("untrusted Control Space decision key"))?;
    verify_thread_space_decision_with_key(req, &key_id, key, now)
}

fn verify_thread_space_decision(req: &pb::CreateThreadRequest) -> Result<(), Status> {
    if !validate_thread_space_context_shape(req)? {
        return Ok(());
    }
    let keys = configured_control_space_decision_keys()?;
    verify_thread_space_decision_with_keys(req, &keys, Utc::now())
}

fn verify_append_space_decision_with_key(
    req: &pb::AppendMessageRequest,
    org_id: &str,
    user_id: &str,
    expected_key_id: &str,
    key: &VerifyingKey,
    now: DateTime<Utc>,
) -> Result<(), Status> {
    if req.space_decision_token.len() > MAX_CONTROL_SPACE_DECISION_TOKEN_BYTES {
        return Err(Status::invalid_argument(
            "Control Space decision is too large",
        ));
    }
    let parts: Vec<&str> = req.space_decision_token.split('.').collect();
    if parts.len() != 4 || parts[0] != CONTROL_SPACE_DECISION_VERSION {
        return Err(Status::permission_denied(
            "invalid Control Space decision envelope",
        ));
    }
    let token_key_id = URL_SAFE_NO_PAD
        .decode(parts[1])
        .ok()
        .and_then(|raw| String::from_utf8(raw).ok())
        .filter(|id| id == expected_key_id)
        .ok_or_else(|| Status::permission_denied("untrusted Control Space decision key"))?;
    let payload = URL_SAFE_NO_PAD
        .decode(parts[2])
        .map_err(|_| Status::permission_denied("invalid Control Space decision payload"))?;
    let signature = URL_SAFE_NO_PAD
        .decode(parts[3])
        .map_err(|_| Status::permission_denied("invalid Control Space decision signature"))?;
    let signature = Signature::from_slice(&signature)
        .map_err(|_| Status::permission_denied("invalid Control Space decision signature"))?;
    key.verify(
        format!("{}.{}.{}", parts[0], parts[1], parts[2]).as_bytes(),
        &signature,
    )
    .map_err(|_| Status::permission_denied("invalid Control Space decision signature"))?;
    let claims: ControlSpaceDecisionClaims = serde_json::from_slice(&payload)
        .map_err(|_| Status::permission_denied("invalid Control Space decision claims"))?;
    let matches_request = claims.decision_ref == req.space_decision_ref
        && claims.org_id == org_id
        && claims.subject_id == user_id
        && claims.space_ref == req.space_id
        && claims.service_audience == CONTROL_SPACE_DECISION_AUDIENCE
        && claims.action_id == CONTROL_THREAD_APPEND_ACTION
        && claims.action_schema_hash == CONTROL_THREAD_APPEND_SCHEMA
        && claims.action_schema_hash == req.action_schema_hash
        && claims.payload_digest == req.payload_digest
        && claims.payload_digest == thread_append_payload_digest(req, org_id, user_id)
        && claims.idempotency_key == req.idempotency_key
        && claims.recipient_audience_ref == req.recipient_audience_ref
        && claims.recipient_audience_revision == req.recipient_audience_revision
        && claims.recipient_audience_hash == req.recipient_audience_hash
        && claims.privacy_policy_ref == req.privacy_policy_ref
        && claims.resource_authorization_ref == req.resource_authorization_ref
        && claims.authority_revision == req.authority_revision
        && claims
            .permissions
            .iter()
            .any(|permission| permission == "thread:append");
    if !matches_request || token_key_id.trim().is_empty() {
        return Err(Status::permission_denied(
            "Control Space decision does not authorize this message append",
        ));
    }
    let privacy_complete = [
        claims.purpose.as_str(),
        claims.lawful_basis.as_str(),
        claims.privacy_class.as_str(),
        claims.retention_class.as_str(),
        claims.residency.as_str(),
        claims.deletion_scope.as_str(),
    ]
    .iter()
    .all(|value| !value.trim().is_empty());
    if !privacy_complete
        || claims.nonce.trim().is_empty()
        || claims.issued_at > now + chrono::Duration::minutes(1)
        || claims.expires_at <= now
    {
        return Err(Status::permission_denied(
            "Control Space append decision is expired or incomplete",
        ));
    }
    Ok(())
}

fn verify_append_space_decision(
    req: &pb::AppendMessageRequest,
    org_id: &str,
    user_id: &str,
) -> Result<(), Status> {
    if !validate_append_space_context_shape(req)? {
        return Ok(());
    }
    let keys = configured_control_space_decision_keys()?;
    let key_id = req
        .space_decision_token
        .split('.')
        .nth(1)
        .and_then(|part| URL_SAFE_NO_PAD.decode(part).ok())
        .and_then(|raw| String::from_utf8(raw).ok())
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| Status::permission_denied("untrusted Control Space decision key"))?;
    let key = keys
        .get(&key_id)
        .ok_or_else(|| Status::permission_denied("untrusted Control Space decision key"))?;
    verify_append_space_decision_with_key(req, org_id, user_id, &key_id, key, Utc::now())
}

/// Whether a decision-less assistant append may land in a Space thread. The
/// reply slot is open only directly after a user message: that user turn was
/// itself admitted by a verified create or append decision, and the assistant
/// reply is the second half of that same authorized exchange. An empty thread
/// has no verified turn to answer, and an assistant/system tail means the
/// exchange is already complete — both stay denied.
fn assistant_reply_slot_is_open(last_role: Option<&str>) -> bool {
    last_role.is_some_and(|role| role.trim() == "user")
}

/// Core of `SessionCore::create_thread`, factored out to keep the trait method
/// small. Inserts the thread row and its `THREAD_CREATED` event in one tx.
async fn create_thread_inner(
    pool: &PgPool,
    req: pb::CreateThreadRequest,
) -> Result<Response<pb::CreateThreadResponse>, Status> {
    verify_thread_space_decision(&req)?;
    create_thread_inner_preverified(pool, req).await
}

async fn create_thread_inner_preverified(
    pool: &PgPool,
    req: pb::CreateThreadRequest,
) -> Result<Response<pb::CreateThreadResponse>, Status> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
    // Reuse an existing thread with the same session key for the same owner.
    //
    // The id is minted here rather than supplied, so a retried create would
    // otherwise mint a SECOND thread and leak the first — and retries are normal:
    // a Temporal activity that creates a thread and then fails before its run is
    // recorded will be retried by the worker. Scoped to (org, owner,
    // session_key) so it can never hand one tenant's or one person's thread to
    // another, and skipped entirely for a blank session_key, which carries no
    // identity to be idempotent on.
    if !req.session_key.trim().is_empty() {
        // The prior read-then-insert sequence allowed two concurrent delivery
        // attempts for the same signed effect to both observe no thread and
        // create different ULIDs. A transaction-scoped advisory lock fences
        // that race without changing the legacy threads schema; it covers the
        // exact tenant/owner/session idempotency tuple and releases on commit
        // or rollback.
        let lock_key = format!(
            "{}\u{1f}{}\u{1f}{}",
            req.org_id, req.user_id, req.session_key
        );
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(lock_key)
            .execute(&mut *tx)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let existing: Option<(String,)> = sqlx::query_as(
            "SELECT id FROM threads WHERE org_id = $1 AND user_id = $2 AND session_key = $3 \
             ORDER BY id LIMIT 1",
        )
        .bind(&req.org_id)
        .bind(&req.user_id)
        .bind(&req.session_key)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
        if let Some((thread_id,)) = existing {
            return Ok(Response::new(pb::CreateThreadResponse {
                thread_id,
                created_at: None,
            }));
        }
    }

    // Support threads carry customer-authored transcript history. Their
    // validated namespace is an immutable security classification consumed by
    // the Frontend Gateway, so preserve that id instead of replacing it with a
    // ULID. Every other session key keeps the standard server-minted id.
    let thread_id = support_thread_id(&req.session_key)
        .map(str::to_owned)
        .unwrap_or_else(new_ulid);
    let now = Utc::now();
    let origin = resolve_thread_origin(&req)?;

    sqlx::query(
        "INSERT INTO threads (id, session_key, org_id, user_id, created_at, space_id, space_decision_ref, recipient_audience_ref, recipient_audience_revision, recipient_audience_hash, privacy_policy_ref, resource_authorization_ref, authority_revision, origin)
         VALUES ($1, $2, $3, $4, $5, NULLIF($6, ''), NULLIF($7, ''), NULLIF($8, ''), NULLIF($9, 0), NULLIF($10, ''), NULLIF($11, ''), NULLIF($12, ''), NULLIF($13, 0), $14)",
    )
    .bind(&thread_id)
    .bind(&req.session_key)
    .bind(&req.org_id)
    .bind(&req.user_id)
    .bind(now)
    .bind(&req.space_id)
    .bind(&req.space_decision_ref)
    .bind(&req.recipient_audience_ref)
    .bind(i64::try_from(req.recipient_audience_revision).map_err(|_| {
        Status::invalid_argument("recipient_audience_revision exceeds PostgreSQL BIGINT range")
    })?)
    .bind(&req.recipient_audience_hash)
    .bind(&req.privacy_policy_ref)
    .bind(&req.resource_authorization_ref)
    .bind(i64::try_from(req.authority_revision).map_err(|_| {
        Status::invalid_argument("authority_revision exceeds PostgreSQL BIGINT range")
    })?)
    .bind(origin)
    .execute(&mut *tx)
    .await
    .map_err(|e| Status::internal(e.to_string()))?;

    insert_thread_created_event(&mut tx, &thread_id, &req, now).await?;

    tx.commit()
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    info!(thread_id = %thread_id, "thread created");

    Ok(Response::new(pb::CreateThreadResponse {
        thread_id,
        created_at: Some(prost_types::Timestamp {
            seconds: now.timestamp(),
            nanos: nanos_to_i32(now.timestamp_subsec_nanos()),
        }),
    }))
}

/// Every value `origin` is allowed to hold. Kept as a single source rather
/// than repeating the list in the CHECK constraint, the proto comment, and
/// this validator.
const THREAD_ORIGINS: [&str; 5] = ["chat", "space", "agent_run", "support", "system"];

/// Resolve and validate `origin` for a thread about to be created.
///
/// A caller that declares `origin` gets it validated against
/// [`THREAD_ORIGINS`] and checked for consistency with `space_id` -- the two
/// columns state one fact between them (see migration 0034's
/// `threads_origin_space_consistency_chk`), so `origin="space"` without a
/// `space_id`, or a `space_id` under any other origin, is a contradiction
/// rejected at the RPC boundary rather than left for the CHECK constraint to
/// catch after the fact.
///
/// A caller that leaves `origin` empty predates this field, and gets the same
/// classification the backfill migration used: `space_id` set means "space";
/// the `support_` session-key convention means "support"; an `agent_run/`
/// session-key prefix (see [`agent_run_thread_key`]) means "agent_run";
/// anything else means "chat". This is a real default, not a placeholder --
/// it closes both the Support-assist and Agent Run Console leaks with zero
/// change to the `CreateThread` wire contract, as long as each surface's own
/// session-key convention is actually in place at the call site.
fn resolve_thread_origin(req: &pb::CreateThreadRequest) -> Result<&'static str, Status> {
    let declared = req.origin.trim();
    if !declared.is_empty() {
        let Some(origin) = THREAD_ORIGINS.iter().find(|allowed| **allowed == declared) else {
            return Err(Status::invalid_argument(format!(
                "origin {declared:?} is not one of {THREAD_ORIGINS:?}"
            )));
        };
        let declares_space = *origin == "space";
        let has_space_id = !req.space_id.trim().is_empty();
        if declares_space != has_space_id {
            return Err(Status::invalid_argument(
                "origin=\"space\" requires space_id to be set, and space_id requires \
                 origin=\"space\" -- the two must agree",
            ));
        }
        return Ok(origin);
    }
    if !req.space_id.trim().is_empty() {
        return Ok("space");
    }
    if support_thread_id(&req.session_key).is_some() {
        return Ok("support");
    }
    if agent_run_thread_key(&req.session_key) {
        return Ok("agent_run");
    }
    Ok("chat")
}

/// Session-key convention for a thread the Agent Run Console started with no
/// explicit thread/session key of its own -- the exact situation that used to
/// leak, since the resulting create-thread call was otherwise indistinguishable
/// from an ordinary new chat.
///
/// Deliberately looser than [`support_thread_id`]: a support session key must
/// survive as the thread's own id (it is the external channel's continuity
/// key), so its shape is validated strictly. An agent-run session key carries
/// no such requirement -- the thread id is still server-minted -- so only the
/// prefix the console actually sends is checked.
fn agent_run_thread_key(session_key: &str) -> bool {
    session_key.trim().starts_with("agent_run/")
}

fn support_thread_id(session_key: &str) -> Option<&str> {
    let candidate = session_key.trim();
    let uuid = candidate.strip_prefix("support_")?;
    if uuid.len() != 36 {
        return None;
    }
    for (index, byte) in uuid.bytes().enumerate() {
        let valid = if matches!(index, 8 | 13 | 18 | 23) {
            byte == b'-'
        } else {
            byte.is_ascii_hexdigit()
        };
        if !valid {
            return None;
        }
    }
    Some(candidate)
}

/// Core of `SessionCore::append_message`, factored out to keep the trait method
/// small. Inserts the message row and its `MESSAGE_APPENDED` event in one tx.
async fn append_message_inner(
    pool: &PgPool,
    letta: Option<&LettaMemoryAdapter>,
    retention: MemoryRetention,
    req: pb::AppendMessageRequest,
) -> Result<Response<pb::AppendMessageResponse>, Status> {
    let msg_id = new_ulid();
    let now = Utc::now();
    let mut letta_sync: Option<(
        String,
        String,
        Vec<crate::dreaming::DreamMemoryCandidate>,
        Vec<crate::dreaming::PersistedMemoryId>,
    )> = None;

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    let has_append_context = validate_append_space_context_shape(&req)?;
    let thread: (String, String, Option<String>) =
        sqlx::query_as("SELECT org_id, user_id, space_id FROM threads WHERE id = $1 FOR UPDATE")
            .bind(&req.thread_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("thread not found"))?;

    match thread.2.as_deref() {
        Some(stored_space_id) => {
            if !has_append_context && req.role.trim() == "user" {
                // A scoped create is verified before the thread exists, so its
                // first user message may be inserted by the same Gateway flow.
                // Every later append must carry a newly-issued append decision.
                let (message_count,): (i64,) =
                    sqlx::query_as("SELECT COUNT(*) FROM messages WHERE thread_id = $1")
                        .bind(&req.thread_id)
                        .fetch_one(&mut *tx)
                        .await
                        .map_err(|e| Status::internal(e.to_string()))?;
                if message_count != 0 {
                    return Err(Status::permission_denied(
                        "a fresh Control Space append decision is required",
                    ));
                }
            } else if !has_append_context && req.role.trim() == "assistant" {
                // An assistant reply carries no independent authority: it is
                // the second half of an exchange whose human turn was already
                // verified (the thread create or a fresh append decision).
                // Allow it only in that reply slot — directly after a user
                // message — where it inherits the thread's stored,
                // Control-verified authority refs. Every other unscoped
                // append into a Space thread stays denied.
                let last_role: Option<(String,)> = sqlx::query_as(
                    "SELECT role FROM messages WHERE thread_id = $1 ORDER BY sequence DESC LIMIT 1",
                )
                .bind(&req.thread_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
                if !assistant_reply_slot_is_open(last_role.as_ref().map(|(role,)| role.as_str())) {
                    return Err(Status::permission_denied(
                        "a Space assistant reply must directly follow a verified user turn",
                    ));
                }
            } else {
                if req.space_id != stored_space_id {
                    return Err(Status::permission_denied(
                        "Control Space append decision targets another Space",
                    ));
                }
                verify_append_space_decision(&req, &thread.0, &thread.1)?;
                sqlx::query(
                    "UPDATE threads SET space_decision_ref = $2, recipient_audience_ref = $3, \
                     recipient_audience_revision = $4, recipient_audience_hash = $5, privacy_policy_ref = $6, \
                     resource_authorization_ref = $7, authority_revision = $8 WHERE id = $1",
                )
                .bind(&req.thread_id)
                .bind(&req.space_decision_ref)
                .bind(&req.recipient_audience_ref)
                .bind(i64::try_from(req.recipient_audience_revision).map_err(|_| Status::invalid_argument("recipient audience revision out of range"))?)
                .bind(&req.recipient_audience_hash)
                .bind(&req.privacy_policy_ref)
                .bind(&req.resource_authorization_ref)
                .bind(i64::try_from(req.authority_revision).map_err(|_| Status::invalid_argument("authority revision out of range"))?)
                .execute(&mut *tx)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
            }
        }
        None if has_append_context => {
            return Err(Status::permission_denied(
                "an unscoped thread cannot accept a Space append decision",
            ));
        }
        None => {}
    }

    // `messages.metadata` has existed in the schema since 0001_init but was
    // never written, so a turn's evidence (grounding/citations/artifacts) died
    // with the stream that produced it. Persist it here and `list_conversation`
    // can hand it back on any device. An absent metadata stays `{}` rather than
    // NULL to match the column default.
    let metadata_json = req
        .metadata
        .as_ref()
        .map_or_else(|| serde_json::json!({}), struct_to_json);

    let row: (i64, Option<String>, Option<String>, Option<i64>, Option<String>, Option<i64>, Option<String>) = sqlx::query_as(
        "INSERT INTO messages
         (id, thread_id, role, content, created_at, agent_name, metadata, space_id, recipient_audience_ref,
          recipient_audience_revision, recipient_audience_hash, authority_revision, resource_authorization_ref)
         SELECT $1, t.id, $3, $4, $5, NULLIF($6, ''), $7, t.space_id, t.recipient_audience_ref,
                t.recipient_audience_revision, t.recipient_audience_hash, t.authority_revision, t.resource_authorization_ref
         FROM threads t WHERE t.id=$2
         RETURNING sequence, space_id, recipient_audience_ref, recipient_audience_revision,
                   recipient_audience_hash, authority_revision, resource_authorization_ref",
    )
    .bind(&msg_id)
    .bind(&req.thread_id)
    .bind(&req.role)
    .bind(&req.content)
    .bind(now)
    .bind(req.agent_name.trim())
    .bind(&metadata_json)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| Status::internal(e.to_string()))?;

    let sequence =
        u64::try_from(row.0).map_err(|_| Status::internal("message sequence out of range"))?;

    let msg_resource = format!("message:{}", &msg_id);
    let msg_idem = derive_idempotency_hash(
        "session-core",
        "MESSAGE_APPENDED",
        &msg_resource,
        &format!("{}:{}", &req.thread_id, sequence),
    );
    sqlx::query(
        "INSERT INTO events (id, event_type, run_id, payload, ts, org_id, user_id, correlation_id, causation_id, idempotency_key, resource_ref, type_url, producer, schema_version)
         SELECT $1, 'MESSAGE_APPENDED', $2, $3, $4, t.org_id, t.user_id, $5, '', $6, $7, $8, 'session-core', 1
         FROM threads t WHERE t.id = $2",
    )
    .bind(new_ulid())
    .bind(&req.thread_id)
    .bind(serde_json::json!({
        "thread_id": &req.thread_id,
        "message_id": &msg_id,
        "sequence": sequence,
        "role": &req.role,
        "space_id": row.1,
        "recipient_audience_ref": row.2,
        "recipient_audience_revision": row.3,
        "recipient_audience_hash": row.4,
        "authority_revision": row.5,
        "resource_authorization_ref": row.6,
    }))
    .bind(now)
    .bind(&req.thread_id)
    .bind(&msg_idem)
    .bind(&msg_resource)
    .bind(MESSAGE_APPENDED_TYPE_URL)
    .execute(&mut *tx)
    .await
    .map_err(|e| Status::internal(e.to_string()))?;

    let candidates =
        crate::dreaming::extract_memory_candidates(&req.role, &req.content, &req.thread_id);
    if !candidates.is_empty() {
        let (org_id, user_id): (String, String) =
            sqlx::query_as("SELECT org_id, user_id FROM threads WHERE id = $1")
                .bind(&req.thread_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
        let (saved, persisted) = crate::dreaming::persist_candidates(
            &mut tx,
            &org_id,
            &user_id,
            &req.thread_id,
            &msg_id,
            &candidates,
        )
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
        crate::dreaming::record_dream_run(
            &mut tx,
            &org_id,
            &req.thread_id,
            "append_message",
            i64::try_from(candidates.len()).unwrap_or(i64::MAX),
            saved,
            Some(&msg_id),
        )
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
        info!(
            thread_id = %req.thread_id,
            memories_found = candidates.len(),
            memories_saved = saved,
            "dream memory extraction completed"
        );
        letta_sync = Some((org_id, user_id, candidates, persisted));
    }

    tx.commit()
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    if let Some((org_id, user_id, candidates, persisted)) = letta_sync {
        if retention.permits_durable_memory() {
            crate::dreaming::sync_persisted_candidates_to_letta(
                letta,
                &org_id,
                &user_id,
                &req.thread_id,
                &candidates,
                &persisted,
            )
            .await;
        }
    }

    Ok(Response::new(pb::AppendMessageResponse { sequence }))
}

/// Core of `SessionCore::start_run`, factored out to keep the trait method
/// small. Inserts the run row and its `RUN_STARTED` event in one tx.
async fn start_run_inner(
    pool: &PgPool,
    req: pb::StartRunRequest,
    requested_run_id: Option<String>,
) -> Result<Response<pb::StartRunResponse>, Status> {
    let requested_run_id = requested_run_id.filter(|value| !value.trim().is_empty());
    let run_id = requested_run_id.clone().unwrap_or_else(new_ulid);
    let now = Utc::now();

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    // P0.4 residency: stamp the configured Model-Plane region (EU default,
    // Sweden Central) onto the run so its processing region is auditable.
    let residency = configured_residency();
    let inherited_space_context: Option<(Option<String>, Option<String>, Option<String>, Option<i64>, Option<String>, Option<String>, Option<String>, Option<i64>)> = sqlx::query_as(
        "INSERT INTO runs (id, thread_id, parent_run_id, agent_id, goal, mode, org_id, user_id, status, residency, created_at, updated_at,
                          space_id, space_decision_ref, recipient_audience_ref, recipient_audience_revision, recipient_audience_hash, privacy_policy_ref, resource_authorization_ref, authority_revision)
         SELECT $1, t.id, NULLIF($3, ''), $4, $5, $6, $7, $8, 'queued', $9, $10, $10,
                t.space_id, t.space_decision_ref, t.recipient_audience_ref, t.recipient_audience_revision, t.recipient_audience_hash, t.privacy_policy_ref, t.resource_authorization_ref, t.authority_revision
         FROM threads t
         WHERE t.id = $2 AND t.org_id = $7
         ON CONFLICT (id) DO NOTHING
         RETURNING space_id, space_decision_ref, recipient_audience_ref, recipient_audience_revision, recipient_audience_hash, privacy_policy_ref, resource_authorization_ref, authority_revision",
    )
    .bind(&run_id)
    .bind(&req.thread_id)
    .bind(&req.parent_run_id)
    .bind(&req.agent_id)
    .bind(&req.goal)
    .bind(&req.mode)
    .bind(&req.org_id)
    .bind(&req.user_id)
    .bind(&residency)
    .bind(now)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| Status::internal(e.to_string()))?;

    let inherited_space_context = match inherited_space_context {
        Some(context) => context,
        None if requested_run_id.is_some() => {
            let existing: Option<(String, String, String, DateTime<Utc>)> = sqlx::query_as(
                "SELECT thread_id, org_id, user_id, created_at FROM runs WHERE id = $1",
            )
            .bind(&run_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
            match existing {
                Some((thread_id, org_id, user_id, created_at))
                    if thread_id == req.thread_id
                        && org_id == req.org_id
                        && user_id == req.user_id =>
                {
                    tx.commit()
                        .await
                        .map_err(|e| Status::internal(e.to_string()))?;
                    return Ok(Response::new(pb::StartRunResponse {
                        run_id,
                        created_at: Some(prost_types::Timestamp {
                            seconds: created_at.timestamp(),
                            nanos: nanos_to_i32(created_at.timestamp_subsec_nanos()),
                        }),
                        owner_id: req.user_id,
                    }));
                }
                Some(_) => {
                    return Err(Status::permission_denied(
                        "scheduled run id is already bound to another owner",
                    ));
                }
                None => return Err(Status::not_found("thread not found for run creation")),
            }
        }
        None => return Err(Status::not_found("thread not found for run creation")),
    };

    let run_started_resource = format!("run:{}", &run_id);
    let run_started_idem = derive_idempotency_hash(
        "session-core",
        "RUN_STARTED",
        &run_started_resource,
        &format!("{}:started", &run_id),
    );
    sqlx::query(
        "INSERT INTO events (id, event_type, run_id, payload, ts, org_id, user_id, correlation_id, causation_id, idempotency_key, resource_ref, type_url, producer, schema_version)
         VALUES ($1, 'RUN_STARTED', $2, $3, $4, $5, $6, $7, '', $8, $9, $10, 'session-core', 1)",
    )
    .bind(new_ulid())
    .bind(&run_id)
    .bind(serde_json::json!({
        "goal": &req.goal,
        "mode": &req.mode,
        "agent_id": &req.agent_id,
        "space_id": inherited_space_context.0,
        "space_decision_ref": inherited_space_context.1,
        "recipient_audience_ref": inherited_space_context.2,
        "recipient_audience_revision": inherited_space_context.3,
        "recipient_audience_hash": inherited_space_context.4,
        "privacy_policy_ref": inherited_space_context.5,
        "resource_authorization_ref": inherited_space_context.6,
        "authority_revision": inherited_space_context.7,
    }))
    .bind(now)
    .bind(&req.org_id)
    .bind(&req.user_id)
    .bind(&run_id)
    .bind(&run_started_idem)
    .bind(&run_started_resource)
    .bind(RUN_STARTED_TYPE_URL)
    .execute(&mut *tx)
    .await
    .map_err(|e| Status::internal(e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    info!(run_id = %run_id, "run started");

    // Durable plan record for this run so the orchestration read path
    // (ListPlans / GetPlan) and the plan UI show real agent progress. Keyed
    // deterministically (`plan_{run_id}`) so complete_step can append steps
    // without a lookup. Best-effort — a plan-write hiccup must not fail the run.
    if let Err(error) = crate::orchestration_store::create_plan(
        pool,
        &format!("plan_{run_id}"),
        &req.thread_id,
        Some(&run_id),
        &req.goal,
        &req.org_id,
        &req.user_id,
        &serde_json::json!({ "source": "run_start" }),
    )
    .await
    {
        tracing::warn!(error = %error, run_id = %run_id, "durable plan create failed (best-effort)");
    } else if let Err(error) =
        crate::orchestration_store::update_plan_status(pool, &format!("plan_{run_id}"), "executing")
            .await
    {
        // The run is executing now, so advance its just-created plan out of
        // draft — record_run_terminal drives it to completed/failed at run end.
        // Best-effort and scoped to this run's own plan id.
        tracing::warn!(error = %error, run_id = %run_id, "durable plan executing-transition failed (best-effort)");
    }

    Ok(Response::new(pb::StartRunResponse {
        run_id,
        created_at: Some(prost_types::Timestamp {
            seconds: now.timestamp(),
            nanos: nanos_to_i32(now.timestamp_subsec_nanos()),
        }),
        // The owner actually persisted, which the caller overwrote with the
        // verified identity before reaching here. Echoed so a durable workflow
        // stamps its lifecycle envelopes with the same actor the row carries.
        owner_id: req.user_id,
    }))
}

#[derive(Debug)]
struct RecordedStep {
    ordinal: i64,
    event_id: String,
    created: bool,
}

fn is_terminal_run_status(status: &str) -> bool {
    matches!(status, "completed" | "failed" | "cancelled")
}

fn requested_terminal_status(req: &pb::CompleteStepRequest) -> Option<&'static str> {
    if !req.terminal {
        return None;
    }
    match req.status.as_str() {
        "completed" => Some("completed"),
        "failed" => Some("failed"),
        _ => None,
    }
}

async fn lock_run_status(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    run_id: &str,
) -> Result<(String, String), Status> {
    sqlx::query_as("SELECT status, org_id FROM runs WHERE id = $1 FOR UPDATE")
        .bind(run_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|error| Status::internal(error.to_string()))?
        .ok_or_else(|| Status::not_found("run not found"))
}

/// Lock and identify a managed-run terminalization obligation while the
/// caller already holds the run row lock. This uses the same lock order as
/// `record_terminal_outcome_inner` (run, then obligation), so a legacy
/// `CompleteStep` call cannot race a source-bound terminal receipt into
/// assigning a second terminal state.
async fn lock_managed_terminalization_obligation(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    run_id: &str,
) -> Result<bool, Status> {
    let obligation: Option<String> = sqlx::query_scalar(
        "SELECT run_id
         FROM managed_run_terminalization_outbox
         WHERE run_id = $1
         FOR UPDATE",
    )
    .bind(run_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| Status::internal(error.to_string()))?;
    Ok(obligation.is_some())
}

fn step_payload_matches(existing: &serde_json::Value, requested: &serde_json::Value) -> bool {
    if existing == requested {
        return true;
    }

    let (serde_json::Value::Object(existing), serde_json::Value::Object(mut requested)) =
        (existing, requested.clone())
    else {
        return false;
    };
    if existing.contains_key("terminal") {
        return false;
    }
    requested.remove("terminal");
    existing == &requested
}

async fn insert_or_replay_step(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    req: &pb::CompleteStepRequest,
    org_id: &str,
) -> Result<RecordedStep, Status> {
    let event_id = new_ulid();
    let step_resource = format!("run:{}:step:{}", &req.run_id, &req.step_id);
    let step_idem = derive_idempotency_hash(
        "session-core",
        "STEP_COMPLETED",
        &step_resource,
        &format!("{}:{}", &req.run_id, &req.step_id),
    );
    let payload = serde_json::json!({
        "step_id": &req.step_id,
        "status": &req.status,
        "output": &req.output,
        "error": &req.error,
        "terminal": req.terminal,
    });
    let inserted: Option<(i64,)> = sqlx::query_as(
        "INSERT INTO events (id, event_type, run_id, payload, ts, org_id, user_id, correlation_id, causation_id, idempotency_key, resource_ref, type_url, producer, schema_version)
         SELECT $1, 'STEP_COMPLETED', $2, $3, now(), r.org_id, r.user_id, $2, '', $4, $5, $6, 'session-core', 1
         FROM runs r WHERE r.id = $2
         ON CONFLICT (org_id, idempotency_key) WHERE idempotency_key <> '' DO NOTHING
         RETURNING step_ordinal",
    )
    .bind(&event_id)
    .bind(&req.run_id)
    .bind(&payload)
    .bind(&step_idem)
    .bind(&step_resource)
    .bind(STEP_COMPLETED_TYPE_URL)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| Status::internal(error.to_string()))?;
    if let Some((ordinal,)) = inserted {
        return Ok(RecordedStep {
            ordinal,
            event_id,
            created: true,
        });
    }

    let existing: Option<(String, serde_json::Value, i64)> = sqlx::query_as(
        "SELECT id, payload, step_ordinal
         FROM events
         WHERE org_id = $1 AND idempotency_key = $2
           AND run_id = $3 AND event_type = 'STEP_COMPLETED'",
    )
    .bind(org_id)
    .bind(&step_idem)
    .bind(&req.run_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| Status::internal(error.to_string()))?;
    let (existing_id, existing_payload, ordinal) = existing
        .ok_or_else(|| Status::already_exists("step idempotency identity is already in use"))?;
    if !step_payload_matches(&existing_payload, &payload) {
        return Err(Status::already_exists(
            "step identity is bound to different immutable input",
        ));
    }
    Ok(RecordedStep {
        ordinal,
        event_id: existing_id,
        created: false,
    })
}

/// Record a terminal run event (`RUN_COMPLETED` / `RUN_FAILED`) and flip the
/// run's status, within the caller's open transaction. `causation_event_id` is
/// the `STEP_COMPLETED` event that drove the terminal transition.
async fn record_run_terminal(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    req: &pb::CompleteStepRequest,
    causation_event_id: &str,
) -> Result<(), Status> {
    let (terminal_event_type, run_status) = if req.status == "failed" {
        ("RUN_FAILED", "failed")
    } else {
        ("RUN_COMPLETED", "completed")
    };

    let terminal_resource = format!("run:{}", &req.run_id);
    let terminal_idem = derive_idempotency_hash(
        "session-core",
        "RUN_TERMINAL",
        &terminal_resource,
        &format!("{}:terminal", &req.run_id),
    );
    let terminal_event_id = new_ulid();
    let terminal_insert = sqlx::query(
        "INSERT INTO events (id, event_type, run_id, payload, ts, org_id, user_id, correlation_id, causation_id, idempotency_key, resource_ref, type_url, producer, schema_version)
         SELECT $1, $2, $3, $4, now(), r.org_id, r.user_id, $3, $5, $8, $6, $7, 'session-core', 1
         FROM runs r WHERE r.id = $3
         ON CONFLICT (org_id, idempotency_key) WHERE idempotency_key <> '' DO NOTHING",
    )
    .bind(&terminal_event_id)
    .bind(terminal_event_type)
    .bind(&req.run_id)
    .bind(serde_json::json!({ "error": &req.error }))
    .bind(causation_event_id)
    .bind(&terminal_resource)
    .bind(RUN_TERMINAL_TYPE_URL)
    .bind(&terminal_idem)
    .execute(&mut **tx)
    .await
    .map_err(|e| Status::internal(e.to_string()))?;
    if terminal_insert.rows_affected() != 1 {
        return Err(Status::already_exists(
            "run terminal outcome is already assigned",
        ));
    }

    let transitioned = sqlx::query(
        "UPDATE runs SET status = $1, ended_at = now(), updated_at = now()
         WHERE id = $2 AND status NOT IN ('completed', 'failed', 'cancelled')",
    )
    .bind(run_status)
    .bind(&req.run_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| Status::internal(e.to_string()))?;
    if transitioned.rows_affected() != 1 {
        return Err(Status::already_exists(
            "run terminal outcome is already assigned",
        ));
    }

    // Drive the run's durable plan to its terminal state. `start_run` creates a
    // plan keyed `plan_{run_id}` in `draft`; without this it never left draft
    // ("plans never leave draft"). Only advance a plan that is still open
    // (draft/proposed/approved/executing) so a human decision (rejected/
    // superseded) is never overwritten. Same tx as the run flip => atomic.
    let plan_status = if run_status == "failed" {
        "failed"
    } else {
        "completed"
    };
    sqlx::query(
        "UPDATE plans SET status = $1, updated_at = now()
         WHERE id = $2 AND status IN ('draft','proposed','approved','executing')",
    )
    .bind(plan_status)
    .bind(format!("plan_{}", &req.run_id))
    .execute(&mut **tx)
    .await
    .map_err(|e| Status::internal(e.to_string()))?;

    // §1.1 learning loop, legacy/unmanaged arm. This `CompleteStep` path is
    // retained only for unmanaged runs (a managed run is rejected above and must
    // use `ManagedRunLifecycle.RecordTerminalOutcome`, which announces itself in
    // `terminalization::apply_managed_terminal_outcome`). Announce the same
    // `RUN_COMPLETED` here so an unmanaged completion is learned from too.
    // Enqueueing cannot fail this call by contract.
    if terminal_event_type == "RUN_COMPLETED" {
        crate::learning_events::enqueue_run_completed(
            tx,
            &req.run_id,
            &terminal_event_id,
            Utc::now(),
        )
        .await;
    }
    Ok(())
}

/// Core of `SessionCore::complete_step`, factored out to keep the trait method
/// small. Appends the `STEP_COMPLETED` event and, on a terminal status, the
/// run-terminal event + status flip, all in one tx.
async fn complete_step_inner(
    pool: &PgPool,
    audit_publisher: Option<&crate::audit_publisher::AuditOutbox>,
    req: pb::CompleteStepRequest,
) -> Result<Response<pb::CompleteStepResponse>, Status> {
    validate_tool_action_token(&req.run_id, "run_id", 128)?;
    validate_tool_action_token(&req.step_id, "step_id", 128)?;
    let desired_terminal_status = if req.terminal {
        Some(requested_terminal_status(&req).ok_or_else(|| {
            Status::invalid_argument("terminal step status must be completed or failed")
        })?)
    } else {
        None
    };
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
    let (run_status, org_id) = lock_run_status(&mut tx, &req.run_id).await?;

    // `CompleteStep` is retained for legacy unmanaged run compatibility, but
    // it is not a terminalization authority for a managed run. A managed
    // obligation is settled only by the separately authenticated,
    // source-bound `ManagedRunLifecycle.RecordTerminalOutcome` path.
    if desired_terminal_status.is_some()
        && lock_managed_terminalization_obligation(&mut tx, &req.run_id).await?
    {
        return Err(Status::failed_precondition(
            "managed run terminal outcome must use ManagedRunLifecycle.RecordTerminalOutcome",
        ));
    }

    let step = insert_or_replay_step(&mut tx, &req, &org_id).await?;

    if !step.created {
        if let Some(desired) = desired_terminal_status {
            if run_status != desired {
                return Err(Status::already_exists(
                    "terminal step replay conflicts with the durable run outcome",
                ));
            }
        }
        tx.commit()
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
        let step_index =
            u32::try_from(step.ordinal).map_err(|_| Status::internal("step index out of range"))?;
        return Ok(Response::new(pb::CompleteStepResponse { step_index }));
    }

    if is_terminal_run_status(&run_status) {
        return Err(Status::already_exists(
            "run terminal outcome is already assigned",
        ));
    }
    if desired_terminal_status.is_some() {
        record_run_terminal(&mut tx, &req, &step.event_id).await?;
    }

    enqueue_tool_action_audit(&mut tx, &req).await?;

    tx.commit()
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    // Mirror the step into the run's durable plan as a plan_step so the plan UI
    // reflects real execution progress. Best-effort + keyed to the run's plan
    // (`plan_{run_id}`, created at run start); a missing plan or write hiccup is
    // logged, never fails step completion.
    {
        let pstep_id = format!("pstep_{}", new_ulid());
        let payload = serde_json::json!({
            "step_id": req.step_id,
            "status": req.status,
            "output": req.output.chars().take(500).collect::<String>(),
            "error": req.error,
        });
        // `CompleteStep` records a step that has ALREADY finished (see
        // sessions.proto: status is "completed", "failed" or "skipped"), so the
        // catch-all must never leave the row reading as still-running. It used
        // to: "skipped" and any other value both fell to `_ => "running"`, so a
        // web_search step that came back `permission denied` sat at "running"
        // forever on a plan whose own state was `completed` — the plan panel
        // showed four perpetually-running steps for work that had failed.
        // Fall back on whether an error was reported, and log the unknown value
        // so contract drift is visible rather than silently mislabelled.
        let mapped = match req.status.as_str() {
            "completed" => "done",
            "failed" => "failed",
            "skipped" => "skipped",
            "awaiting_approval" => "awaiting_approval",
            other => {
                tracing::warn!(
                    status = %other,
                    run_id = %req.run_id,
                    step_id = %req.step_id,
                    "unrecognised CompleteStep status; classifying by error presence"
                );
                if req.error.trim().is_empty() { "done" } else { "failed" }
            }
        };
        match crate::orchestration_store::append_plan_step(
            pool,
            &pstep_id,
            &format!("plan_{}", req.run_id),
            "tool_execution",
            &payload,
            &serde_json::json!({}),
        )
        .await
        {
            Ok(_) => {
                if let Err(error) =
                    crate::orchestration_store::update_step_status(pool, &pstep_id, mapped).await
                {
                    tracing::warn!(error = %error, "plan_step status update failed (best-effort)");
                }
            }
            Err(error) => {
                tracing::debug!(error = %error, run_id = %req.run_id, "plan_step append skipped (no plan for run?)");
            }
        }
    }

    // The intent already committed with the step. Immediate dispatch reduces
    // latency; any failure remains durable for the background retry worker.
    if let Some(outbox) = audit_publisher {
        if let Err(error) = outbox.dispatch_one().await {
            tracing::warn!(%error, "tool_action audit retained for retry");
        }
    }

    let step_index =
        u32::try_from(step.ordinal).map_err(|_| Status::internal("step index out of range"))?;

    Ok(Response::new(pb::CompleteStepResponse { step_index }))
}

/// Enqueue the audit body in the same `PostgreSQL` transaction as `STEP_COMPLETED`.
/// Non-tool steps have no governed data-category prefix and create no row.
async fn enqueue_tool_action_audit(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    req: &pb::CompleteStepRequest,
) -> Result<(), Status> {
    let Some(detail) = crate::audit_publisher::parse_tool_action_detail(&req.output, &req.error)
    else {
        return Ok(());
    };

    let identity =
        sqlx::query_as::<_, (String, String)>("SELECT org_id, user_id FROM runs WHERE id = $1")
            .bind(&req.run_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
    let Some((org_id, user_id)) = identity else {
        return Err(Status::failed_precondition("run identity is unavailable"));
    };

    let tool = crate::audit_publisher::resolve_tool_name(&detail, &req.step_id);
    let body = crate::audit_publisher::build_tool_action_body(
        &org_id,
        &user_id,
        &req.run_id,
        &req.step_id,
        &req.status,
        &tool,
        &detail,
        Utc::now(),
    );
    let event_id = body["event_id"]
        .as_str()
        .ok_or_else(|| Status::internal("audit event identity missing"))?
        .to_owned();
    sqlx::query(
        "INSERT INTO session_audit_outbox (event_id, subject, payload)
         VALUES ($1, $2, $3)
         ON CONFLICT (event_id) DO NOTHING",
    )
    .bind(event_id)
    .bind(crate::audit_publisher::SUBJECT_MODEL_TOOL_ACTION)
    .bind(body)
    .execute(&mut **tx)
    .await
    .map_err(|error| Status::internal(error.to_string()))?;
    Ok(())
}

fn validate_tool_action_token(
    value: &str,
    field: &'static str,
    max_len: usize,
) -> Result<(), Status> {
    if value.is_empty()
        || value != value.trim()
        || value.len() > max_len
        || value.chars().any(char::is_control)
    {
        return Err(Status::invalid_argument(format!("invalid {field}")));
    }
    Ok(())
}

fn tool_action_detail(
    tool: &str,
    data_category: &str,
    zdr: bool,
) -> crate::audit_publisher::ToolActionDetail {
    crate::audit_publisher::ToolActionDetail {
        data_category: data_category.to_owned(),
        zdr,
        tool: Some(tool.to_owned()),
    }
}

async fn enqueue_inline_tool_audit(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: &str,
    user_id: &str,
    run_id: &str,
    action_id: &str,
    request_id: &str,
    outcome: &str,
    detail: &crate::audit_publisher::ToolActionDetail,
    phase: &str,
) -> Result<(), Status> {
    let tool = detail
        .tool
        .as_deref()
        .ok_or_else(|| Status::internal("tool action name missing"))?;
    let mut body = crate::audit_publisher::build_tool_action_body_for_phase(
        org_id,
        user_id,
        run_id,
        action_id,
        outcome,
        tool,
        detail,
        phase,
        Utc::now(),
    );
    if phase == "reserved" {
        body["event"] = serde_json::json!("tool_action_reserved");
    }
    body["details"]["request_id"] = serde_json::json!(request_id);
    let event_id = body["event_id"]
        .as_str()
        .ok_or_else(|| Status::internal("audit event identity missing"))?
        .to_owned();
    sqlx::query(
        "INSERT INTO session_audit_outbox (event_id, subject, payload)
         VALUES ($1, $2, $3)
         ON CONFLICT (event_id) DO NOTHING",
    )
    .bind(event_id)
    .bind(crate::audit_publisher::SUBJECT_MODEL_TOOL_ACTION)
    .bind(body)
    .execute(&mut **tx)
    .await
    .map_err(|error| Status::internal(error.to_string()))?;
    Ok(())
}

async fn reserve_tool_action_inner(
    pool: &PgPool,
    audit_publisher: Option<&crate::audit_publisher::AuditOutbox>,
    req: pb::ReserveToolActionRequest,
) -> Result<Response<pb::ReserveToolActionResponse>, Status> {
    validate_tool_action_token(&req.run_id, "run_id", 128)?;
    validate_tool_action_token(&req.action_id, "action_id", 128)?;
    validate_tool_action_token(&req.request_id, "request_id", 128)?;
    validate_tool_action_token(&req.tool, "tool", 96)?;
    validate_tool_action_token(&req.data_category, "data_category", 64)?;

    let mut tx = pool
        .begin()
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
    let inserted = sqlx::query(
        "INSERT INTO session_tool_audit_intents
            (run_id, action_id, request_id, tool, data_category, zdr)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (run_id, action_id) DO NOTHING",
    )
    .bind(&req.run_id)
    .bind(&req.action_id)
    .bind(&req.request_id)
    .bind(&req.tool)
    .bind(&req.data_category)
    .bind(req.zdr)
    .execute(&mut *tx)
    .await
    .map_err(|error| Status::internal(error.to_string()))?
    .rows_affected()
        == 1;

    let existing = sqlx::query_as::<_, (String, String, String, bool, String, String)>(
        "SELECT i.request_id, i.tool, i.data_category, i.zdr, r.org_id, r.user_id
         FROM session_tool_audit_intents i
         JOIN runs r ON r.id = i.run_id
         WHERE i.run_id = $1 AND i.action_id = $2
         FOR UPDATE OF i",
    )
    .bind(&req.run_id)
    .bind(&req.action_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| Status::internal(error.to_string()))?
    .ok_or_else(|| Status::failed_precondition("tool action run is unavailable"))?;
    if existing.0 != req.request_id
        || existing.1 != req.tool
        || existing.2 != req.data_category
        || existing.3 != req.zdr
    {
        return Err(Status::already_exists(
            "tool action identity is bound to different immutable input",
        ));
    }

    let detail = tool_action_detail(&req.tool, &req.data_category, req.zdr);
    enqueue_inline_tool_audit(
        &mut tx,
        &existing.4,
        &existing.5,
        &req.run_id,
        &req.action_id,
        &req.request_id,
        "reserved",
        &detail,
        "reserved",
    )
    .await?;
    tx.commit()
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
    if let Some(outbox) = audit_publisher {
        if let Err(error) = outbox.dispatch_one().await {
            tracing::warn!(%error, "reserved tool_action audit retained for retry");
        }
    }
    Ok(Response::new(pb::ReserveToolActionResponse {
        created: inserted,
    }))
}

async fn finalize_tool_action_inner(
    pool: &PgPool,
    audit_publisher: Option<&crate::audit_publisher::AuditOutbox>,
    req: pb::FinalizeToolActionRequest,
) -> Result<Response<pb::FinalizeToolActionResponse>, Status> {
    validate_tool_action_token(&req.run_id, "run_id", 128)?;
    validate_tool_action_token(&req.action_id, "action_id", 128)?;
    if !matches!(req.outcome.as_str(), "completed" | "failed") {
        return Err(Status::invalid_argument("invalid tool action outcome"));
    }

    let mut tx = pool
        .begin()
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
    let existing = sqlx::query_as::<_, (String, String, String, bool, String, String, String)>(
        "SELECT i.request_id, i.tool, i.data_category, i.zdr, i.status, r.org_id, r.user_id
         FROM session_tool_audit_intents i
         JOIN runs r ON r.id = i.run_id
         WHERE i.run_id = $1 AND i.action_id = $2
         FOR UPDATE OF i",
    )
    .bind(&req.run_id)
    .bind(&req.action_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| Status::internal(error.to_string()))?
    .ok_or_else(|| Status::failed_precondition("tool action was not reserved"))?;
    let updated = if existing.4 == "reserved" {
        sqlx::query(
            "UPDATE session_tool_audit_intents
             SET status = $3, finalized_at = now()
             WHERE run_id = $1 AND action_id = $2 AND status = 'reserved'",
        )
        .bind(&req.run_id)
        .bind(&req.action_id)
        .bind(&req.outcome)
        .execute(&mut *tx)
        .await
        .map_err(|error| Status::internal(error.to_string()))?
        .rows_affected()
            == 1
    } else if existing.4 == req.outcome {
        false
    } else {
        return Err(Status::already_exists(
            "tool action was finalized with a different outcome",
        ));
    };

    let detail = tool_action_detail(&existing.1, &existing.2, existing.3);
    enqueue_inline_tool_audit(
        &mut tx,
        &existing.5,
        &existing.6,
        &req.run_id,
        &req.action_id,
        &existing.0,
        &req.outcome,
        &detail,
        "final",
    )
    .await?;
    tx.commit()
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
    if let Some(outbox) = audit_publisher {
        if let Err(error) = outbox.dispatch_one().await {
            tracing::warn!(%error, "final tool_action audit retained for retry");
        }
    }
    Ok(Response::new(pb::FinalizeToolActionResponse { updated }))
}

/// One event row in the exact column order selected by `replay_thread_task`'s
/// query (see that function's `SELECT` for the field-by-field mapping).
type ReplayEventRow = (
    String,
    String,
    serde_json::Value,
    chrono::DateTime<chrono::Utc>,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    i32,
);

/// Map a replay event row to its proto `Event`.
fn replay_event_row_to_proto(row: ReplayEventRow) -> pb::Event {
    let (
        event_id,
        event_type,
        payload,
        ts,
        org_id,
        user_id,
        correlation_id,
        causation_id,
        type_url,
        idempotency_key,
        resource_ref,
        producer,
        schema_version,
    ) = row;
    pb::Event {
        event_id,
        event_type: event_type_to_i32(&event_type),
        schema_version: u32::try_from(schema_version).unwrap_or(0),
        ts: Some(prost_types::Timestamp {
            seconds: ts.timestamp(),
            nanos: nanos_to_i32(ts.timestamp_subsec_nanos()),
        }),
        producer,
        correlation_id,
        causation_id,
        idempotency_key,
        org_id,
        user_id,
        resource_ref,
        payload: Some(prost_types::Any {
            type_url,
            value: serde_json::to_vec(&payload).unwrap_or_default(),
        }),
        zdr: false,
    }
}

/// Spawned worker for `SessionCore::replay_thread`: replays a thread's events
/// (ordered by `(ts, id)` for determinism) onto the response stream.
async fn replay_thread_task(
    pool: PgPool,
    req: pb::ReplayThreadRequest,
    tx: tokio::sync::mpsc::Sender<Result<pb::Event, Status>>,
) {
    // Thread-created/message-appended events use the owning thread id in the
    // legacy `events.run_id` column because no run exists yet. Run lifecycle
    // events use the actual run id. Replay both shapes so a newly reloaded
    // conversation does not silently lose its opening user turn or its
    // assistant transcript from the canonical event stream.
    let rows = sqlx::query_as::<_, ReplayEventRow>(
        "SELECT e.id, e.event_type, e.payload, e.ts, e.org_id, e.user_id, e.correlation_id, e.causation_id, e.type_url, e.idempotency_key, e.resource_ref, e.producer, e.schema_version
         FROM events e
         LEFT JOIN runs r ON e.run_id = r.id
         WHERE (e.run_id = $1 OR r.thread_id = $1)
         AND ($2 = '' OR e.id > $2)
         ORDER BY e.ts ASC, e.id ASC
         LIMIT CASE WHEN $3 = 0 THEN 10000 ELSE $3 END",
    )
    .bind(&req.thread_id)
    .bind(&req.after_event_id)
    .bind(i64::from(req.limit))
    .fetch_all(&pool)
    .await;

    match rows {
        Ok(events) => {
            for row in events {
                if tx.send(Ok(replay_event_row_to_proto(row))).await.is_err() {
                    break;
                }
            }
        }
        Err(e) => {
            let _ = tx.send(Err(Status::internal(e.to_string()))).await;
        }
    }
}

/// Recent non-empty thread messages (oldest-first) for context assembly.
async fn load_thread_messages(
    pool: &PgPool,
    thread_id: &str,
) -> Result<Vec<(String, String)>, Status> {
    let mut msgs = sqlx::query_as::<_, (String, String)>(
        "SELECT role, content
         FROM (
             SELECT role, content, sequence
             FROM messages
             WHERE thread_id = $1
             ORDER BY sequence DESC
             LIMIT $2
         ) recent
         ORDER BY sequence ASC",
    )
    .bind(thread_id)
    .bind(CONTEXT_MESSAGE_LIMIT)
    .fetch_all(pool)
    .await
    .map_err(|e| Status::internal(e.to_string()))?;
    msgs.retain(|(_, content)| !content.is_empty());
    Ok(msgs)
}

fn semantic_memory_query(thread_messages: &[(String, String)]) -> String {
    thread_messages
        .iter()
        .rev()
        .take(6)
        .rev()
        .map(|(role, content)| format!("{role}: {content}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Durable-memory posture of the verified caller, as it applies to the Letta
/// memory boundary.
///
/// Letta is a content-persisting boundary that lives outside session-core's own
/// database, so the plane rule "Zero Data Retention must propagate through any
/// content-persisting boundary" has to be enforced here in its own right.
/// `authorize_operation` already refuses every non-`:read` session operation for
/// a ZDR credential, but that is a coarse RPC-level check on a different
/// concern; deriving the posture explicitly at the memory boundary means a
/// later change to the RPC gate cannot silently start persisting ZDR content in
/// Letta, and it makes the read side — which legitimately runs under
/// `session:read` — decidable at all.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MemoryRetention {
    /// The trusted issuer did not mark the credential Zero Data Retention.
    Durable,
    /// The trusted issuer marked the credential Zero Data Retention.
    ZeroDataRetention,
}

impl MemoryRetention {
    pub(crate) fn of(caller: &VerifiedIdentity) -> Self {
        if caller.zdr() {
            Self::ZeroDataRetention
        } else {
            Self::Durable
        }
    }

    /// Whether durable Letta memory may be touched at all, in either direction.
    ///
    /// Writes are the obvious half: a ZDR turn must leave nothing behind, and
    /// the skip happens here rather than relying on letta-bridge to refuse, so
    /// no ZDR content is ever put on the wire.
    ///
    /// Reads are denied too, deliberately. Two reasons. Consistency: the
    /// explicit `MemoryService` surface already refuses `memory:read` for a ZDR
    /// caller (`memory_grpc::authorize_memory_preflight`), so allowing it here
    /// would make one credential's reach depend on which RPC it happened to
    /// take. Substance: a durable memory is by construction a distillation of
    /// *other, retained* sessions, so injecting one would pull retained content
    /// into a turn whose caller was promised no retention, where the model
    /// re-processes it and echoes it into the answer — retained data leaking
    /// into a no-retention context. Reading persists nothing new, which is why
    /// this is a judgement call rather than a mechanical one, but a retention
    /// boundary fails closed: a ZDR turn gets no memory in and leaves none
    /// behind.
    pub(crate) const fn permits_durable_memory(self) -> bool {
        matches!(self, Self::Durable)
    }
}

/// The two ways session-core itself suppresses the durable Letta path. Unlike
/// every other reason on this path these are local policy decisions rather than
/// `LettaReadiness` values — the bridge is never called, so it has no readiness
/// to report — but they ride the same
/// `mp_session_semantic_memory_context_degraded_total` series so a retention
/// skip is exactly as visible as a bridge outage.
pub(crate) const ZDR_MEMORY_READ_SUPPRESSED: &str = "DEGRADED_LETTA_ZDR_READ_SUPPRESSED";
pub(crate) const ZDR_MEMORY_WRITE_SUPPRESSED: &str = "DEGRADED_LETTA_ZDR_WRITE_SUPPRESSED";

/// Bounded operational outcome of semantic augmentation in the default chat
/// context path. The reason originates from `LettaReadiness`, whose values are
/// static protocol codes rather than provider-supplied text, or from the
/// `ZDR_MEMORY_*_SUPPRESSED` policy codes above.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SemanticContextSearchStatus {
    Empty,
    Results,
    Degraded(&'static str),
}

fn semantic_context_search_status(
    entry_count: usize,
    degradation_reason: Option<&'static str>,
) -> SemanticContextSearchStatus {
    match degradation_reason {
        Some(reason) => SemanticContextSearchStatus::Degraded(reason),
        None if entry_count == 0 => SemanticContextSearchStatus::Empty,
        None => SemanticContextSearchStatus::Results,
    }
}

/// Single emitter for every degraded semantic-memory reason, so a retention skip
/// on the write side lands in the same series as a bridge outage on the read
/// side instead of needing a parallel mechanism.
fn record_semantic_memory_degraded(reason: &'static str) {
    metrics::counter!(
        "mp_session_semantic_memory_context_degraded_total",
        "reason" => reason,
    )
    .increment(1);
}

fn record_semantic_context_search(status: SemanticContextSearchStatus) {
    let outcome = match status {
        SemanticContextSearchStatus::Empty => "empty",
        SemanticContextSearchStatus::Results => "results",
        SemanticContextSearchStatus::Degraded(_) => "degraded",
    };
    metrics::counter!(
        "mp_session_semantic_memory_context_searches_total",
        "outcome" => outcome,
    )
    .increment(1);

    if let SemanticContextSearchStatus::Degraded(reason) = status {
        record_semantic_memory_degraded(reason);
        warn!(reason, "semantic memory context augmentation degraded");
    }
}

fn append_letta_search_outcome(
    memory_rows: &mut Vec<(String, String)>,
    outcome: LettaSearchOutcome,
    max_entries: usize,
) -> SemanticContextSearchStatus {
    let status = semantic_context_search_status(outcome.entries.len(), outcome.degradation_reason);
    record_semantic_context_search(status);

    let mut appended = 0_usize;
    for entry in outcome.entries {
        let content = entry.content.trim();
        if content.is_empty()
            || memory_rows
                .iter()
                .any(|(_, existing)| existing.trim() == content)
        {
            continue;
        }
        let topic = if entry.topic.trim().is_empty() {
            "MEMORY"
        } else {
            entry.topic.trim()
        };
        memory_rows.push(("MEMORY".to_owned(), format!("letta/{topic}: {content}")));
        appended += 1;
        if appended >= max_entries {
            break;
        }
    }

    status
}

/// Semantic-memory augmentation for the default chat context path.
///
/// Returns the recorded outcome so the retention gate is assertable structurally
/// rather than by scraping a log line. `None` means the path did not apply at
/// all — memory is not configured, or the thread has no org — which is not a
/// degradation and is deliberately left uncounted, matching prior behaviour.
async fn append_letta_memory_rows(
    letta: Option<&LettaMemoryAdapter>,
    retention: MemoryRetention,
    org_id: Option<&str>,
    thread_id: &str,
    // The thread's owner, so the semantic tier scopes per user the same way the
    // durable query does. `None` means no user filter, which is org-wide —
    // acceptable only because this path always also passes a thread id.
    owner_user_id: Option<&str>,
    thread_messages: &[(String, String)],
    memory_rows: &mut Vec<(String, String)>,
) -> Option<SemanticContextSearchStatus> {
    let (Some(letta), Some(org_id)) = (letta, org_id) else {
        return None;
    };

    // Zero Data Retention read gate. See `MemoryRetention::permits_durable_memory`
    // for why a ZDR turn is denied memory reads and not just memory writes. The
    // skip is reported as a degradation, never as an empty result, so it cannot
    // be mistaken for "this thread has no memories".
    if !retention.permits_durable_memory() {
        let status = SemanticContextSearchStatus::Degraded(ZDR_MEMORY_READ_SUPPRESSED);
        record_semantic_context_search(status);
        return Some(status);
    }

    let query = semantic_memory_query(thread_messages);
    let outcome = letta
        .search_detailed(
            org_id,
            thread_id,
            owner_user_id.unwrap_or_default(),
            &query,
            &[],
            8,
        )
        .await;
    Some(append_letta_search_outcome(memory_rows, outcome, 8))
}

async fn load_context_memory_rows(
    svc: &SessionService,
    retention: MemoryRetention,
    thread_id: &str,
    user_id: Option<&str>,
    thread_messages: &[(String, String)],
) -> Result<Vec<(String, String)>, Status> {
    let thread_org_id: Option<String> =
        sqlx::query_scalar("SELECT org_id FROM threads WHERE id = $1")
            .bind(thread_id)
            .fetch_optional(&svc.pool)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

    let mut rows = sqlx::query_as::<_, (String, String)>(
        "SELECT topic, content
         FROM memory_index
         WHERE thread_id = $1
         ORDER BY updated_at DESC
         LIMIT $2",
    )
    .bind(thread_id)
    .bind(CONTEXT_MEMORY_LIMIT)
    .fetch_all(&svc.pool)
    .await
    .map_err(|e| Status::internal(e.to_string()))?;

    let mut agent_rows = crate::dreaming::load_agent_memory_context_rows(
        &svc.pool,
        thread_id,
        user_id,
        crate::dreaming::AGENT_MEMORY_CONTEXT_LIMIT,
    )
    .await
    .map_err(|e| Status::internal(e.to_string()))?;
    rows.append(&mut agent_rows);
    append_letta_memory_rows(
        svc.letta_memory.as_ref(),
        retention,
        thread_org_id.as_deref(),
        thread_id,
        user_id,
        thread_messages,
        &mut rows,
    )
    .await;
    Ok(rows)
}

/// Core of `SessionCore::get_context_assembly`. Loads run metadata, recent
/// messages, and memory buckets; fans out to retrieval/knowledge/graph; then
/// assembles the budgeted context segments.
async fn get_context_assembly_inner(
    svc: &SessionService,
    req: pb::GetContextAssemblyRequest,
    retention: MemoryRetention,
    dataplane_bearer: Option<&DelegatedDataPlaneBearer>,
) -> Result<Response<pb::GetContextAssemblyResponse>, Status> {
    let thread_id = req.thread_id.clone();
    let run_id = req.run_id.clone();

    let run_meta: Option<(String, String, String)> = if run_id.is_empty() {
        None
    } else {
        sqlx::query_as::<_, (String, String, String)>(
            "SELECT agent_id, user_id, goal FROM runs WHERE id = $1",
        )
        .bind(&run_id)
        .fetch_optional(&svc.pool)
        .await
        .map_err(|e| Status::internal(e.to_string()))?
    };

    let (run_agent, user_id, prompt_goal) = match run_meta {
        Some((a, u, g)) => (Some(a), Some(u), Some(g)),
        None => (None, None, None),
    };

    let agent_id = if req.agent_id.is_empty() {
        run_agent
    } else {
        Some(req.agent_id.clone())
    };

    let thread_messages = load_thread_messages(&svc.pool, &thread_id).await?;
    let memory_rows = load_context_memory_rows(
        svc,
        retention,
        &thread_id,
        user_id.as_deref(),
        &thread_messages,
    )
    .await?;
    let memory = bucket_memory_segments(memory_rows);

    let evidence = svc
        .fetch_retrieval_segments(
            &thread_id,
            &thread_messages,
            req.max_tokens,
            &memory.retrieval,
            req.sovereign_required,
            dataplane_bearer,
        )
        .await;

    // Persist live Data Plane retrieval into memory_index so the next call's
    // `bucket_memory_segments` path picks it up even when Data Plane is
    // unreachable. Best-effort; never blocking.
    if evidence.source == RetrievalSource::DataPlane {
        svc.persist_retrieval_evidence(&thread_id, &evidence.segments)
            .await;
    }
    if evidence.low_confidence {
        info!(thread_id, "retrieval returned low_confidence");
    }

    let knowledge_segments = svc
        .fetch_knowledge_segments(&thread_id, &evidence.document_ids, dataplane_bearer)
        .await;
    let graph_segments = svc.fetch_graph_segments(&thread_id, dataplane_bearer).await;

    let inputs = AssemblyInputs {
        policy_id: req.policy_id,
        workspace_id: req.workspace_id,
        agent_id,
        user_id,
        prompt_goal,
        thread_messages,
        policy_segments: memory.policy,
        workspace_segments: memory.workspace,
        agent_segments: memory.agent,
        user_segments: memory.user,
        episodic_segments: memory.episodic,
        skill_index_segments: memory.skill_index,
        skill_expansion_segments: memory.skill_expansion,
        retrieval_segments: evidence.segments,
        knowledge_segments,
        graph_segments,
        run_id,
        max_tokens: req.max_tokens,
    };

    let (segments, estimated_tokens) = assemble_segments(&inputs);

    Ok(Response::new(pb::GetContextAssemblyResponse {
        segments,
        estimated_tokens,
    }))
}

#[tonic::async_trait]
impl SessionCore for SessionService {
    async fn create_thread(
        &self,
        request: Request<pb::CreateThreadRequest>,
    ) -> Result<Response<pb::CreateThreadResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:write")?;
            let mut req = request.into_inner();
            caller.authorize_org(&req.org_id)?;
            // A system run needs a thread and no human can create it for the
            // workflow (`runs.thread_id` is NOT NULL REFERENCES threads(id)), so
            // the same allowlisted principals may own a thread. The scope is
            // shared with start_run: the thread exists only to hold the run.
            let user_id = match caller.user_id() {
                Some(user_id) => user_id.to_owned(),
                None => authorize_system_run_owner(&caller)?,
            };
            req.user_id = user_id;
            create_thread_inner(&self.pool, req).await
        }
        .await;
        record_metrics("create_thread", started, result.is_ok());
        result
    }

    async fn append_message(
        &self,
        request: Request<pb::AppendMessageRequest>,
    ) -> Result<Response<pb::AppendMessageResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:write")?;
            let req = request.into_inner();
            authorize_thread_owner(&self.pool, &caller, &req.thread_id, OwnerIntent::Mutate)
                .await?;
            append_message_inner(
                &self.pool,
                self.letta_memory.as_ref(),
                MemoryRetention::of(&caller),
                req,
            )
            .await
        }
        .await;
        record_metrics("append_message", started, result.is_ok());
        result
    }

    async fn start_run(
        &self,
        request: Request<pb::StartRunRequest>,
    ) -> Result<Response<pb::StartRunResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            // Stays FIRST and keeps this exact scope argument: it is what keeps a
            // ZDR credential out of a durable write, and a run row is durable by
            // definition.
            authorize_operation(&caller, "session:write")?;
            let mut req = request.into_inner();
            caller.authorize_org(&req.org_id)?;
            // A run is normally owned by the person who asked for it. A durable
            // workflow fired by cron has no person — a Temporal activity can only
            // present a service credential, and auth-core mints no user
            // delegation — so an allowlisted service may own the run itself.
            let owner = match caller.user_id() {
                Some(user_id) => {
                    authorize_thread_owner(
                        &self.pool,
                        &caller,
                        &req.thread_id,
                        OwnerIntent::Mutate,
                    )
                    .await?;
                    user_id.to_owned()
                }
                None => {
                    let owner = authorize_system_run_owner(&caller)?;
                    // Deliberately STRICTER than authorize_thread_owner, which is
                    // org-only for a service caller and would otherwise let an
                    // allowlisted workload park a system run inside a human's
                    // thread.
                    authorize_thread_owner_exact(&self.pool, &caller, &req.thread_id, &owner)
                        .await?;
                    // Same reasoning one level up the tree: without this a system
                    // run could be grafted onto a person's run as a child.
                    authorize_parent_run_owner_exact(
                        &self.pool,
                        &req.parent_run_id,
                        &req.org_id,
                        &owner,
                    )
                    .await?;
                    owner
                }
            };
            req.user_id = owner;
            start_run_inner(&self.pool, req, None).await
        }
        .await;
        record_metrics("start_run", started, result.is_ok());
        result
    }

    async fn start_scheduled_run(
        &self,
        request: Request<pb::StartScheduledRunRequest>,
    ) -> Result<Response<pb::StartRunResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:write")?;
            if !caller.is_service() || caller.principal_id() != "service:orchestrator-core" {
                return Err(Status::permission_denied(
                    "only Orchestrator Core may start prepared scheduled runs",
                ));
            }
            let req = request.into_inner();
            caller.authorize_org(&req.org_id)?;
            if !valid_scheduled_run_identifier(&req.run_id)
                || !valid_scheduled_run_identifier(&req.schedule_id)
                || !valid_scheduled_run_identifier(&req.fire_key)
                || req.thread_id.trim().is_empty()
                || req.goal.trim().is_empty()
            {
                return Err(Status::invalid_argument(
                    "prepared scheduled-run bindings are invalid",
                ));
            }
            let owner = "service:orchestrator-core";
            let expected_thread_key = format!("schedule/{}/{}", req.schedule_id, req.fire_key);
            let exact: Option<(String, String, i64, String, String, String, i64)> = sqlx::query_as(
                "SELECT COALESCE(space_id, ''), COALESCE(recipient_audience_ref, ''),
                        COALESCE(recipient_audience_revision, 0), COALESCE(recipient_audience_hash, ''),
                        COALESCE(privacy_policy_ref, ''), COALESCE(resource_authorization_ref, ''),
                        COALESCE(authority_revision, 0)
                 FROM threads
                 WHERE id=$1 AND org_id=$2 AND user_id=$3 AND session_key=$4
                   AND COALESCE(space_id, '') <> ''",
            )
            .bind(&req.thread_id)
            .bind(&req.org_id)
            .bind(owner)
            .bind(expected_thread_key)
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
            if exact.is_none() {
                return Err(Status::permission_denied(
                    "scheduled run thread is not the exact prepared service thread",
                ));
            }
            let exact = exact.expect("checked above");
            let thread = ScheduledRunThreadContext {
                space_id: exact.0,
                recipient_audience_ref: exact.1,
                recipient_audience_revision: u64::try_from(exact.2).map_err(|_| {
                    Status::permission_denied("scheduled run thread has invalid audience revision")
                })?,
                recipient_audience_hash: exact.3,
                privacy_policy_ref: exact.4,
                resource_authorization_ref: exact.5,
                authority_revision: u64::try_from(exact.6).map_err(|_| {
                    Status::permission_denied("scheduled run thread has invalid authority revision")
                })?,
            };
            verify_scheduled_run_execution_decision(&req, &thread)?;
            let response = start_run_inner(
                &self.pool,
                pb::StartRunRequest {
                    thread_id: req.thread_id.clone(),
                    parent_run_id: String::new(),
                    agent_id: req.agent_id.clone(),
                    goal: req.goal.clone(),
                    mode: req.mode.clone(),
                    org_id: req.org_id.clone(),
                    user_id: owner.to_owned(),
                },
                Some(req.run_id.clone()),
            )
            .await?;
            persist_scheduled_run_bindings(&self.pool, &req).await?;
            Ok(response)
        }
        .await;
        record_metrics("start_scheduled_run", started, result.is_ok());
        result
    }

    async fn prepare_scheduled_run_thread(
        &self,
        request: Request<pb::PrepareScheduledRunThreadRequest>,
    ) -> Result<Response<pb::PrepareScheduledRunThreadResponse>, Status> {
        let caller = identity(&request)?;
        authorize_operation(&caller, "session:schedule-prepare")?;
        if !caller.is_service() || caller.principal_id() != "service:capability-core" {
            return Err(Status::permission_denied(
                "only Capability Core may prepare scheduled runs",
            ));
        }
        let req = request.into_inner();
        caller.authorize_org(&req.org_id)?;
        verify_scheduled_run_decision(&req)?;

        let owner = "service:orchestrator-core";
        let created = create_thread_inner_preverified(
            &self.pool,
            pb::CreateThreadRequest {
                session_key: req.system_thread_key.clone(),
                org_id: req.org_id.clone(),
                user_id: owner.to_owned(),
                space_id: req.space_id.clone(),
                space_decision_ref: req.space_decision_ref.clone(),
                recipient_audience_ref: req.recipient_audience_ref.clone(),
                recipient_audience_revision: req.recipient_audience_revision,
                recipient_audience_hash: req.recipient_audience_hash.clone(),
                privacy_policy_ref: req.privacy_policy_ref.clone(),
                resource_authorization_ref: req.resource_authorization_ref.clone(),
                authority_revision: req.authority_revision,
                action_schema_hash: req.action_schema_hash.clone(),
                payload_digest: req.payload_digest.clone(),
                idempotency_key: req.idempotency_key.clone(),
                ..Default::default()
            },
        )
        .await?
        .into_inner();

        // Control mints a fresh decision_ref/nonce for every reauthorization of
        // the same fire. Those values prove the preparation call, not durable
        // thread identity, so retries compare every stable authority binding
        // below while deliberately not requiring the previous decision_ref.
        let exact: Option<(String,)> =
            sqlx::query_as(
                "SELECT id FROM threads WHERE id=$1 AND org_id=$2 AND user_id=$3 AND session_key=$4
             AND space_id=$5 AND recipient_audience_ref=$6
             AND recipient_audience_revision=$7 AND recipient_audience_hash=$8
             AND privacy_policy_ref=$9 AND resource_authorization_ref=$10 AND authority_revision=$11
             AND action_schema_hash=$12 AND payload_digest=$13 AND idempotency_key=$14",
            )
            .bind(&created.thread_id)
            .bind(&req.org_id)
            .bind(owner)
            .bind(&req.system_thread_key)
            .bind(&req.space_id)
            .bind(&req.recipient_audience_ref)
            .bind(i64::try_from(req.recipient_audience_revision).map_err(|_| {
                Status::invalid_argument("recipient audience revision is too large")
            })?)
            .bind(&req.recipient_audience_hash)
            .bind(&req.privacy_policy_ref)
            .bind(&req.resource_authorization_ref)
            .bind(
                i64::try_from(req.authority_revision)
                    .map_err(|_| Status::invalid_argument("authority revision is too large"))?,
            )
            .bind(&req.action_schema_hash)
            .bind(&req.payload_digest)
            .bind(&req.idempotency_key)
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
        if exact.is_none() {
            return Err(Status::permission_denied(
                "existing scheduled-run thread has different authority bindings",
            ));
        }
        Ok(Response::new(pb::PrepareScheduledRunThreadResponse {
            thread_id: created.thread_id,
            run_id: req.run_id,
            owner_id: owner.to_owned(),
        }))
    }

    async fn complete_step(
        &self,
        request: Request<pb::CompleteStepRequest>,
    ) -> Result<Response<pb::CompleteStepResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:write")?;
            let req = request.into_inner();
            authorize_run_owner(&self.pool, &caller, &req.run_id, OwnerIntent::Mutate).await?;
            complete_step_inner(&self.pool, self.audit_publisher.as_deref(), req).await
        }
        .await;
        record_metrics("complete_step", started, result.is_ok());
        result
    }

    async fn reserve_tool_action(
        &self,
        request: Request<pb::ReserveToolActionRequest>,
    ) -> Result<Response<pb::ReserveToolActionResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:write")?;
            let req = request.into_inner();
            authorize_run_owner(&self.pool, &caller, &req.run_id, OwnerIntent::Mutate).await?;
            reserve_tool_action_inner(&self.pool, self.audit_publisher.as_deref(), req).await
        }
        .await;
        record_metrics("reserve_tool_action", started, result.is_ok());
        result
    }

    async fn finalize_tool_action(
        &self,
        request: Request<pb::FinalizeToolActionRequest>,
    ) -> Result<Response<pb::FinalizeToolActionResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:write")?;
            let req = request.into_inner();
            authorize_run_owner(&self.pool, &caller, &req.run_id, OwnerIntent::Mutate).await?;
            finalize_tool_action_inner(&self.pool, self.audit_publisher.as_deref(), req).await
        }
        .await;
        record_metrics("finalize_tool_action", started, result.is_ok());
        result
    }

    async fn save_checkpoint(
        &self,
        request: Request<pb::SaveCheckpointRequest>,
    ) -> Result<Response<pb::SaveCheckpointResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::SaveCheckpointResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:write")?;
            let req = request.into_inner();
            validate_user_checkpoint(&req.checkpoint_id, &req.state)?;
            authorize_run_owner(&self.pool, &caller, &req.run_id, OwnerIntent::Mutate).await?;
            let now = Utc::now();

            let mut tx = self
                .pool
                .begin()
                .await
                .map_err(|e| Status::internal(e.to_string()))?;

            let row: (i64,) = sqlx::query_as(
                "INSERT INTO checkpoints (id, run_id, state, created_at)
                 VALUES ($1, $2, $3, $4)
                 RETURNING ordinal",
            )
            .bind(&req.checkpoint_id)
            .bind(&req.run_id)
            .bind(&req.state[..])
            .bind(now)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

            let checkpoint_resource = format!("run:{}:checkpoint:{}", &req.run_id, &req.checkpoint_id);
            let checkpoint_idem = derive_idempotency_hash(
                "session-core",
                "CHECKPOINT_SAVED",
                &checkpoint_resource,
                &format!("{}:{}", &req.run_id, &req.checkpoint_id),
            );
            sqlx::query(
                "INSERT INTO events (id, event_type, run_id, payload, ts, org_id, user_id, correlation_id, causation_id, idempotency_key, resource_ref, type_url, producer, schema_version)
                 SELECT $1, 'CHECKPOINT_SAVED', $2, $3, $4, r.org_id, r.user_id, $2, '', $5, $6, $7, 'session-core', 1
                 FROM runs r WHERE r.id = $2",
            )
            .bind(new_ulid())
            .bind(&req.run_id)
            .bind(serde_json::json!({
                "checkpoint_id": &req.checkpoint_id,
                "size": req.state.len(),
            }))
            .bind(now)
            .bind(&checkpoint_idem)
            .bind(&checkpoint_resource)
            .bind(CHECKPOINT_SAVED_TYPE_URL)
            .execute(&mut *tx)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

            tx.commit()
                .await
                .map_err(|e| Status::internal(e.to_string()))?;

            let checkpoint_index = u32::try_from(row.0)
                .map_err(|_| Status::internal("checkpoint index out of range"))?;

            Ok(Response::new(pb::SaveCheckpointResponse {
                checkpoint_index,
                saved_at: Some(prost_types::Timestamp {
                    seconds: now.timestamp(),
                    nanos: nanos_to_i32(now.timestamp_subsec_nanos()),
                }),
            }))
        }
        .await;
        record_metrics("save_checkpoint", started, result.is_ok());
        result
    }

    type ReplayThreadStream = tokio_stream::wrappers::ReceiverStream<Result<pb::Event, Status>>;

    async fn replay_thread(
        &self,
        request: Request<pb::ReplayThreadRequest>,
    ) -> Result<Response<Self::ReplayThreadStream>, Status> {
        let started = Instant::now();
        let caller = identity(&request)?;
        authorize_operation(&caller, "session:read")?;
        let req = request.into_inner();
        authorize_thread_owner(&self.pool, &caller, &req.thread_id, OwnerIntent::Read).await?;
        let pool = self.pool.clone();

        let (tx, rx) = tokio::sync::mpsc::channel(64);
        tokio::spawn(replay_thread_task(pool, req, tx));

        let resp = Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
            rx,
        )));
        record_metrics("replay_thread", started, true);
        resp
    }

    async fn compact_now(
        &self,
        request: Request<pb::CompactNowRequest>,
    ) -> Result<Response<pb::CompactNowResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::CompactNowResponse>, Status> = async {
            let caller = identity(&request)?;
            // This RPC compacts all tenants and is never valid user authority.
            caller.require_service_scope("session:compact")?;
            let req = request.into_inner();
            let n = crate::compaction::compact_once(&self.pool)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
            let summary = if req.toon {
                let payload = serde_json::json!({
                    "compacted": n,
                    "service": "session-core",
                    "ts": Utc::now().to_rfc3339(),
                });
                mp_toon::encode(&payload)
            } else {
                format!("compaction complete: {n} checkpoint(s) inserted")
            };
            info!(compacted = n, toon = req.toon, "on-demand compaction");
            Ok(Response::new(pb::CompactNowResponse {
                compacted_count: n,
                summary,
            }))
        }
        .await;
        record_metrics("compact_now", started, result.is_ok());
        result
    }

    // G7 closed learning loop: persist a skill body into agent_skills. The
    // capability-core learning loop calls this with origin="background_review";
    // a human skill editor uses origin="user". Provenance is enforced in the
    // DB: a background_review upsert never overwrites a user-authored skill
    // (the `WHERE` guard on the conflict), surfaced as `skipped_protected`
    // rather than an error so the loop yields gracefully to the human.
    /// Flip one skill's injection switch without touching anything else.
    ///
    /// `upsert_agent_skill` is keyed by (org_id, name) and rewrites the whole
    /// row, so using it to pause a skill would blank the content and triggers it
    /// was meant to preserve. The quality policy needs a reversible pause, and a
    /// destructive one would make quarantine unrecoverable — which is the single
    /// property that policy is built around not having.
    async fn set_agent_skill_enabled(
        &self,
        request: Request<pb::SetAgentSkillEnabledRequest>,
    ) -> Result<Response<pb::SetAgentSkillEnabledResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::SetAgentSkillEnabledResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:skills:write")?;
            let req = request.into_inner();
            if req.org_id.trim().is_empty() || req.skill_id.trim().is_empty() {
                return Err(Status::invalid_argument("org_id and skill_id are required"));
            }
            caller.authorize_org(&req.org_id)?;

            // org_id in the predicate as well as the id: a skill id is a ULID and
            // therefore unguessable, but tenant scoping must never rest on
            // unguessability.
            let updated: Option<(bool,)> = sqlx::query_as(
                "UPDATE agent_skills SET enabled = $1, updated_at = now() \
                 WHERE id = $2 AND org_id = $3 RETURNING enabled",
            )
            .bind(req.enabled)
            .bind(&req.skill_id)
            .bind(&req.org_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;

            let Some((enabled,)) = updated else {
                // Not found is reported, not raised: a sweep acts on candidates
                // computed a moment earlier, and a skill deleted in between is a
                // race to report rather than a failure to retry.
                return Ok(Response::new(pb::SetAgentSkillEnabledResponse {
                    updated: false,
                    enabled: false,
                }));
            };
            tracing::info!(
                skill_id = %req.skill_id,
                org_id = %req.org_id,
                enabled,
                reason = %req.reason,
                actor = %caller.principal_id(),
                "agent skill injection switch changed"
            );
            Ok(Response::new(pb::SetAgentSkillEnabledResponse {
                updated: true,
                enabled,
            }))
        }
        .await;
        record_metrics("set_agent_skill_enabled", started, result.is_ok());
        result
    }

    async fn upsert_agent_skill(
        &self,
        request: Request<pb::UpsertAgentSkillRequest>,
    ) -> Result<Response<pb::UpsertAgentSkillResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::UpsertAgentSkillResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:skills:write")?;
            let req = request.into_inner();
            if req.org_id.is_empty() || req.name.is_empty() {
                return Err(Status::invalid_argument("org_id and name are required"));
            }
            caller.authorize_org(&req.org_id)?;
            let origin = match req.origin.as_str() {
                "" | "background_review" => "background_review",
                "user" => "user",
                other => {
                    return Err(Status::invalid_argument(format!("invalid origin: {other}")));
                }
            };
            let id = new_ulid();
            // None ⟺ the conflict guard rejected the write (target is a
            // protected user skill); a fresh insert or permitted update always
            // returns a row.
            let row: Option<(String, bool)> = sqlx::query_as(
                "INSERT INTO agent_skills
                    (id, org_id, name, description, content, trigger_keywords,
                     trigger_file_patterns, tool_restrictions, enabled, origin,
                     created_at, updated_at)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10, now(), now())
                 ON CONFLICT (org_id, name) DO UPDATE SET
                     description = EXCLUDED.description,
                     content = EXCLUDED.content,
                     trigger_keywords = EXCLUDED.trigger_keywords,
                     trigger_file_patterns = EXCLUDED.trigger_file_patterns,
                     tool_restrictions = EXCLUDED.tool_restrictions,
                     enabled = EXCLUDED.enabled,
                     updated_at = now()
                 WHERE agent_skills.origin <> 'user' OR EXCLUDED.origin = 'user'
                 RETURNING id, (xmax = 0) AS created",
            )
            .bind(&id)
            .bind(&req.org_id)
            .bind(&req.name)
            .bind(&req.description)
            .bind(&req.content)
            .bind(serde_json::json!(req.trigger_keywords))
            .bind(serde_json::json!(req.trigger_file_patterns))
            .bind(serde_json::json!(req.tool_restrictions))
            .bind(req.enabled)
            .bind(origin)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| {
                warn!(error = %e, "upsert_agent_skill failed");
                Status::internal(e.to_string())
            })?;

            let resp = match row {
                Some((id, created)) => pb::UpsertAgentSkillResponse {
                    id,
                    created,
                    skipped_protected: false,
                },
                None => pb::UpsertAgentSkillResponse {
                    id: String::new(),
                    created: false,
                    skipped_protected: true,
                },
            };
            Ok(Response::new(resp))
        }
        .await;
        record_metrics("upsert_agent_skill", started, result.is_ok());
        result
    }

    // ROADMAP P3 run modes: durably set a run's mode on the runs row. The
    // gateway's in-memory plan-mode cache write-throughs here so plan mode
    // survives restart (auditability/resume — GOAL.md §7).
    async fn set_run_mode(
        &self,
        request: Request<pb::SetRunModeRequest>,
    ) -> Result<Response<pb::SetRunModeResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::SetRunModeResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:write")?;
            let req = request.into_inner();
            if req.run_id.is_empty() || req.org_id.is_empty() {
                return Err(Status::invalid_argument("run_id and org_id are required"));
            }
            caller.authorize_org(&req.org_id)?;
            authorize_run_owner(&self.pool, &caller, &req.run_id, OwnerIntent::Mutate).await?;
            let mode = match req.mode.as_str() {
                "execute" | "plan" | "reactive" | "research" => req.mode.as_str(),
                other => {
                    return Err(Status::invalid_argument(format!(
                        "invalid run mode: {other}"
                    )));
                }
            };
            // An autonomy grant rides along on the mode change, because leaving
            // plan mode IS the grant. It is validated HERE as well as at the
            // gateway: the gateway is the caller that should have checked, and a
            // server that trusts its caller to have checked has no rule at all.
            //
            // The floor is READ_ONLY rather than the run's current rung on
            // purpose — this call cannot widen an existing grant incrementally,
            // it can only state the grant a fresh approval made, so validating
            // against the narrowest rung is the honest comparison.
            let granted = pb::AutonomyRung::try_from(req.granted_rung)
                .unwrap_or(pb::AutonomyRung::Unspecified);
            let grant = if granted == pb::AutonomyRung::Unspecified {
                // No grant stated: an ordinary mode change. Leaves whatever the
                // run already carried rather than revoking it silently.
                None
            } else {
                Some(
                    mp_contracts::autonomy::AutonomyEscalation::request(
                        pb::AutonomyRung::ReadOnly,
                        granted,
                        &req.justification,
                    )
                    .map_err(|refusal| Status::invalid_argument(refusal.message()))?,
                )
            };
            // Merged into `metadata` rather than written to a new column: the
            // JSONB is already there, already returned by GetRun, and already
            // carries per-run facts. `||` merges so an unrelated key is not
            // dropped by a mode change.
            let grant_patch = grant.as_ref().map_or_else(
                || serde_json::json!({}),
                |grant| {
                    serde_json::json!({
                        "autonomy_rung": mp_contracts::autonomy::label(grant.to()),
                        "autonomy_justification": grant.justification(),
                    })
                },
            );
            // Org-scoped UPDATE: a run is only mutable by its owning org, so a
            // caller can never flip the mode of another org's run (per-org
            // isolation invariant). A missing row ⟺ wrong org OR unknown run;
            // both surface as not_found without leaking which.
            let row: Option<(String, String)> = sqlx::query_as(
                "UPDATE runs
                    SET mode = $2,
                        metadata = COALESCE(metadata, '{}'::jsonb) || $4::jsonb,
                        updated_at = now()
                  WHERE id = $1 AND org_id = $3 RETURNING id, mode",
            )
            .bind(&req.run_id)
            .bind(mode)
            .bind(&req.org_id)
            .bind(&grant_patch)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| {
                warn!(error = %e, "set_run_mode failed");
                Status::internal(e.to_string())
            })?;
            let (run_id, mode) =
                row.ok_or_else(|| Status::not_found(format!("run {} not found", req.run_id)))?;
            Ok(Response::new(pb::SetRunModeResponse {
                run_id,
                mode,
                granted_rung: granted as i32,
            }))
        }
        .await;
        record_metrics("set_run_mode", started, result.is_ok());
        result
    }

    async fn claim_scheduled_step(
        &self,
        request: Request<pb::ClaimScheduledStepRequest>,
    ) -> Result<Response<pb::ClaimScheduledStepResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, crate::auth::SCHEDULED_STEP_SCOPE)?;
            authorize_scheduled_step_service(&caller)?;
            claim_scheduled_step_inner(&self.pool, request.into_inner()).await
        }
        .await;
        record_metrics("claim_scheduled_step", started, result.is_ok());
        result
    }

    async fn record_scheduled_step_receipt(
        &self,
        request: Request<pb::RecordScheduledStepReceiptRequest>,
    ) -> Result<Response<pb::RecordScheduledStepReceiptResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, crate::auth::SCHEDULED_STEP_SCOPE)?;
            authorize_scheduled_step_service(&caller)?;
            record_scheduled_step_receipt_inner(&self.pool, request.into_inner()).await
        }
        .await;
        record_metrics("record_scheduled_step_receipt", started, result.is_ok());
        result
    }

    // G7 read path (the loop's "last mile"): list an org's agent skills so the
    // gateway's MatchSkills cache can surface LEARNED skills, not just disk
    // ones. Org-scoped — only this org's rows. The jsonb array columns are
    // NOT NULL DEFAULT '[]', so they decode cleanly into Vec<String>.
    async fn list_agent_skills(
        &self,
        request: Request<pb::ListAgentSkillsRequest>,
    ) -> Result<Response<pb::ListAgentSkillsResponse>, Status> {
        type Row = (
            String,
            String,
            String,
            String,
            sqlx::types::Json<Vec<String>>,
            sqlx::types::Json<Vec<String>>,
            sqlx::types::Json<Vec<String>>,
            bool,
            String,
            String,
            String,
            sqlx::types::Json<Vec<String>>,
        );

        let started = Instant::now();
        let result: Result<Response<pb::ListAgentSkillsResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:read")?;
            let req = request.into_inner();
            if req.org_id.is_empty() {
                return Err(Status::invalid_argument("org_id is required"));
            }
            caller.authorize_org(&req.org_id)?;
            // Ownership/sharing (SKILL-1): mirrors capability-core's
            // SkillsHandler.listOrCreate, the correct reference
            // implementation this table's scope/owner_user_id/shared_with
            // columns were added for. That handler enforces this on read;
            // this RPC selected the columns but never filtered on them,
            // returning every org member's "private" skill to every caller.
            // An admin-governance carve-out (SkillsHandler also shows an
            // admin every shared-but-not-own skill) is deliberately omitted
            // here: this crate's VerifiedIdentity carries no org-role claim,
            // and omitting it only narrows admin visibility, never widens
            // anyone's.
            let caller_user_id = caller.user_id().unwrap_or_default().to_owned();
            let rows: Vec<Row> = sqlx::query_as(
                "SELECT id, name, description, content, trigger_keywords,
                        trigger_file_patterns, tool_restrictions, enabled, origin,
                        scope, owner_user_id, shared_with
                 FROM agent_skills
                 WHERE org_id = $1 AND (NOT $2 OR enabled)
                   AND (scope = 'org' OR owner_user_id = $3 OR shared_with ? $3)
                 ORDER BY name",
            )
            .bind(&req.org_id)
            .bind(req.enabled_only)
            .bind(&caller_user_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| {
                warn!(error = %e, "list_agent_skills failed");
                Status::internal(e.to_string())
            })?;
            let skills = rows
                .into_iter()
                .map(
                    |(
                        id,
                        name,
                        description,
                        content,
                        kw,
                        fp,
                        tr,
                        enabled,
                        origin,
                        scope,
                        owner_user_id,
                        shared_with,
                    )| {
                        pb::AgentSkill {
                            id,
                            name,
                            description,
                            content,
                            trigger_keywords: kw.0,
                            trigger_file_patterns: fp.0,
                            tool_restrictions: tr.0,
                            enabled,
                            origin,
                            scope,
                            owner_user_id,
                            shared_with: shared_with.0,
                        }
                    },
                )
                .collect();
            Ok(Response::new(pb::ListAgentSkillsResponse { skills }))
        }
        .await;
        record_metrics("list_agent_skills", started, result.is_ok());
        result
    }

    // G7 transcript source: list a thread's conversation in order. Org-scoped
    // via a JOIN on threads.org_id, so a caller can only read its own org's
    // conversation (the messages table has no org_id of its own).
    async fn list_conversation(
        &self,
        request: Request<pb::ListConversationRequest>,
    ) -> Result<Response<pb::ListConversationResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::ListConversationResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:read")?;
            let req = request.into_inner();
            if req.org_id.is_empty() || req.thread_id.is_empty() {
                return Err(Status::invalid_argument(
                    "org_id and thread_id are required",
                ));
            }
            caller.authorize_org(&req.org_id)?;
            authorize_thread_owner(&self.pool, &caller, &req.thread_id, OwnerIntent::Read).await?;
            let rows: Vec<(
                String,
                String,
                String,
                Option<String>,
                Option<serde_json::Value>,
            )> = sqlx::query_as(
                // `m.id` is selected so a caller can name one earlier turn
                // durably (message pinning). It was always in the table; this
                // read simply never returned it.
                "SELECT m.id::text, m.role, m.content, m.agent_name, m.metadata
                 FROM messages m
                 JOIN threads t ON t.id = m.thread_id
                 WHERE m.thread_id = $1 AND t.org_id = $2
                 ORDER BY m.sequence",
            )
                .bind(&req.thread_id)
                .bind(&req.org_id)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| {
                    warn!(error = %e, "list_thread_messages failed");
                    Status::internal(e.to_string())
                })?;
            let messages = rows
                .into_iter()
                .map(|(message_id, role, content, agent_name, metadata)| pb::SessionMessage {
                    message_id,
                    role,
                    content,
                    agent_name: agent_name.unwrap_or_default(),
                    // `{}` is the column default for every turn written before
                    // metadata was persisted; send None rather than an empty
                    // Struct so the caller can tell "no evidence recorded" from
                    // "evidence recorded and empty".
                    metadata: metadata
                        .filter(|value| !matches!(value, serde_json::Value::Null))
                        .filter(|value| value.as_object().is_none_or(|map| !map.is_empty()))
                        .as_ref()
                        .and_then(json_to_struct),
                })
                .collect();
            Ok(Response::new(pb::ListConversationResponse { messages }))
        }
        .await;
        record_metrics("list_conversation", started, result.is_ok());
        result
    }

    async fn list_threads(
        &self,
        request: Request<pb::ListThreadsRequest>,
    ) -> Result<Response<pb::ListThreadsResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::ListThreadsResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:read")?;
            let req = request.into_inner();
            if req.org_id.is_empty() || req.user_id.is_empty() {
                return Err(Status::invalid_argument("org_id and user_id are required"));
            }
            caller.authorize_org(&req.org_id)?;
            caller.authorize_user(&req.user_id)?;
            let limit = clamp_thread_limit(req.limit);
            let rows: Vec<(
                String,
                String,
                DateTime<Utc>,
                Option<String>,
                Option<String>,
                Option<DateTime<Utc>>,
                DateTime<Utc>,
                Option<String>,
                Option<String>,
                Option<String>,
                Option<DateTime<Utc>>,
                String,
            )> = sqlx::query_as(
                "SELECT
                    t.id,
                    t.session_key,
                    t.created_at,
                    COALESCE(t.presentation_title, first_user.content) AS title,
                    COALESCE(t.presentation_preview, last_message.content) AS preview,
                    t.pinned_at,
                    COALESCE(last_message.created_at, t.created_at) AS updated_at,
                    t.space_id,
                    latest_run.id AS latest_run_id,
                    latest_run.status AS latest_run_status,
                    latest_run.updated_at AS latest_run_updated_at,
                    t.origin
                 FROM threads t
                 LEFT JOIN LATERAL (
                    SELECT content
                    FROM messages
                    WHERE thread_id = t.id AND role = 'user'
                    ORDER BY sequence ASC
                    LIMIT 1
                 ) first_user ON TRUE
                 LEFT JOIN LATERAL (
                    SELECT content, created_at
                    FROM messages
                    WHERE thread_id = t.id
                    ORDER BY sequence DESC
                    LIMIT 1
                 ) last_message ON TRUE
                 LEFT JOIN LATERAL (
                    SELECT id, status, updated_at
                    FROM runs
                    WHERE thread_id = t.id
                    ORDER BY updated_at DESC, created_at DESC, id DESC
                    LIMIT 1
                 ) latest_run ON TRUE
                 WHERE t.org_id = $1 AND t.user_id = $2 AND t.archived_at IS NULL
                   AND (NULLIF($4, '') IS NULL OR t.space_id = $4)
                   AND (NULLIF($5, '') IS NULL OR t.origin = $5)
                 ORDER BY (t.pinned_at IS NOT NULL) DESC, COALESCE(last_message.created_at, t.created_at) DESC, t.created_at DESC, t.id DESC
                 LIMIT $3",
            )
            .bind(&req.org_id)
            .bind(&req.user_id)
            .bind(limit)
            .bind(&req.space_id)
            .bind(&req.origin)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| {
                warn!(error = %e, "list_threads failed");
                Status::internal(e.to_string())
            })?;

            let threads = rows
                .into_iter()
                .map(
                    |(thread_id, session_key, created_at, title, preview, pinned_at, updated_at, space_id, latest_run_id, latest_run_status, latest_run_updated_at, origin)| {
                        let fallback_title = if session_key.trim().is_empty() {
                            "Verevon Chat"
                        } else {
                            session_key.as_str()
                        };
                        let title =
                            compact_thread_text(title, fallback_title, THREAD_TITLE_MAX_CHARS);
                        let preview =
                            compact_thread_text(preview, "", THREAD_PREVIEW_MAX_CHARS);
                        pb::ThreadSummary {
                            thread_id,
                            session_key,
                            title,
                            preview,
                            created_at: Some(to_proto_timestamp(created_at)),
                            updated_at: Some(to_proto_timestamp(updated_at)),
                            pinned: pinned_at.is_some(),
                            space_id: space_id.unwrap_or_default(),
                            latest_run_id: latest_run_id.unwrap_or_default(),
                            latest_run_status: latest_run_status.unwrap_or_default(),
                            latest_run_updated_at: latest_run_updated_at.map(to_proto_timestamp),
                            origin,
                        }
                    },
                )
                .collect();

            Ok(Response::new(pb::ListThreadsResponse { threads }))
        }
        .await;
        record_metrics("list_threads", started, result.is_ok());
        result
    }

    async fn update_thread_presentation(
        &self,
        request: Request<pb::UpdateThreadPresentationRequest>,
    ) -> Result<Response<pb::UpdateThreadPresentationResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:write")?;
            if caller.zdr() {
                return Err(Status::failed_precondition(
                    "ZDR callers cannot persist thread presentation",
                ));
            }
            let req = request.into_inner();
            if req.org_id.trim().is_empty() || req.thread_id.trim().is_empty() {
                return Err(Status::invalid_argument(
                    "org_id and thread_id are required",
                ));
            }
            if req.title.is_none() && req.preview.is_none() && req.pinned.is_none() {
                return Err(Status::invalid_argument(
                    "at least one presentation field is required",
                ));
            }
            caller.authorize_org(&req.org_id)?;
            authorize_thread_owner(&self.pool, &caller, &req.thread_id, OwnerIntent::Mutate)
                .await?;

            let title = req
                .title
                .as_deref()
                .map(|value| {
                    normalize_thread_presentation_text(value, "title", THREAD_TITLE_MAX_CHARS)
                })
                .transpose()?;
            let preview = req
                .preview
                .as_deref()
                .map(|value| {
                    normalize_thread_presentation_text(value, "preview", THREAD_PREVIEW_MAX_CHARS)
                })
                .transpose()?;
            let now = Utc::now();
            let mut tx = self
                .pool
                .begin()
                .await
                .map_err(|error| Status::internal(error.to_string()))?;
            let changed: Option<(String, String)> = sqlx::query_as(
                "UPDATE threads
                 SET presentation_title = CASE WHEN $3 THEN $4 ELSE presentation_title END,
                     presentation_preview = CASE WHEN $5 THEN $6 ELSE presentation_preview END,
                     pinned_at = CASE
                         WHEN $7 THEN CASE WHEN $8 THEN $9 ELSE NULL END
                         ELSE pinned_at
                     END
                 WHERE id = $1 AND org_id = $2 AND archived_at IS NULL
                 RETURNING id, user_id",
            )
            .bind(&req.thread_id)
            .bind(&req.org_id)
            .bind(title.is_some())
            .bind(title.as_ref().and_then(|value| value.as_deref()))
            .bind(preview.is_some())
            .bind(preview.as_ref().and_then(|value| value.as_deref()))
            .bind(req.pinned.is_some())
            .bind(req.pinned.unwrap_or_default())
            .bind(now)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
            let (_, thread_owner) =
                changed.ok_or_else(|| Status::not_found("thread not found or archived"))?;

            let resource = format!("thread:{}", req.thread_id);
            let event_id = new_ulid();
            sqlx::query(
                "INSERT INTO events (id, event_type, run_id, payload, ts, org_id, user_id, correlation_id, causation_id, idempotency_key, resource_ref, type_url, producer, schema_version)
                 VALUES ($1, 'THREAD_PRESENTATION_UPDATED', $2, $3, $4, $5, $6, $7, '', $8, $9, $10, 'session-core', 1)",
            )
            .bind(&event_id)
            .bind(&req.thread_id)
            .bind(serde_json::json!({
                "thread_id": &req.thread_id,
                "title_overridden": title.is_some(),
                "preview_overridden": preview.is_some(),
                "pinned": req.pinned,
            }))
            .bind(now)
            .bind(&req.org_id)
            .bind(thread_owner)
            .bind(&req.thread_id)
            .bind(derive_idempotency_hash(
                "session-core",
                "THREAD_PRESENTATION_UPDATED",
                &resource,
                &format!("{}:{event_id}", req.thread_id),
            ))
            .bind(&resource)
            .bind(THREAD_PRESENTATION_UPDATED_TYPE_URL)
            .execute(&mut *tx)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
            tx.commit()
                .await
                .map_err(|error| Status::internal(error.to_string()))?;

            Ok(Response::new(pb::UpdateThreadPresentationResponse {
                thread_id: req.thread_id,
            }))
        }
        .await;
        record_metrics("update_thread_presentation", started, result.is_ok());
        result
    }

    async fn archive_thread(
        &self,
        request: Request<pb::ArchiveThreadRequest>,
    ) -> Result<Response<pb::ArchiveThreadResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:write")?;
            if caller.zdr() {
                return Err(Status::failed_precondition(
                    "ZDR callers cannot archive durable threads",
                ));
            }
            let req = request.into_inner();
            if req.org_id.trim().is_empty() || req.thread_id.trim().is_empty() {
                return Err(Status::invalid_argument(
                    "org_id and thread_id are required",
                ));
            }
            caller.authorize_org(&req.org_id)?;
            authorize_thread_owner(&self.pool, &caller, &req.thread_id, OwnerIntent::Mutate)
                .await?;
            let now = Utc::now();
            let mut tx = self
                .pool
                .begin()
                .await
                .map_err(|error| Status::internal(error.to_string()))?;
            let newly_archived_at: Option<(DateTime<Utc>,)> = sqlx::query_as(
                "UPDATE threads
                 SET archived_at = $3
                 WHERE id = $1 AND org_id = $2 AND archived_at IS NULL
                 RETURNING archived_at",
            )
            .bind(&req.thread_id)
            .bind(&req.org_id)
            .bind(now)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
            let archived_at = if let Some((archived_at,)) = newly_archived_at {
                let resource = format!("thread:{}", req.thread_id);
                sqlx::query(
                    "INSERT INTO events (id, event_type, run_id, payload, ts, org_id, user_id, correlation_id, causation_id, idempotency_key, resource_ref, type_url, producer, schema_version)
                     SELECT $1, 'THREAD_ARCHIVED', $2, $3, $4, t.org_id, t.user_id, $5, '', $6, $7, $8, 'session-core', 1
                     FROM threads t WHERE t.id = $2",
                )
                .bind(new_ulid())
                .bind(&req.thread_id)
                .bind(serde_json::json!({"thread_id": &req.thread_id}))
                .bind(archived_at)
                .bind(&req.thread_id)
                .bind(derive_idempotency_hash(
                    "session-core",
                    "THREAD_ARCHIVED",
                    &resource,
                    &format!("{}:archived", req.thread_id),
                ))
                .bind(&resource)
                .bind(THREAD_ARCHIVED_TYPE_URL)
                .execute(&mut *tx)
                .await
                .map_err(|error| Status::internal(error.to_string()))?;
                archived_at
            } else {
                sqlx::query_scalar::<_, DateTime<Utc>>(
                    "SELECT archived_at FROM threads WHERE id = $1 AND org_id = $2 AND archived_at IS NOT NULL",
                )
                .bind(&req.thread_id)
                .bind(&req.org_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|error| Status::internal(error.to_string()))?
                .ok_or_else(|| Status::not_found("thread not found"))?
            };
            tx.commit()
                .await
                .map_err(|error| Status::internal(error.to_string()))?;

            Ok(Response::new(pb::ArchiveThreadResponse {
                thread_id: req.thread_id,
                archived_at: Some(to_proto_timestamp(archived_at)),
            }))
        }
        .await;
        record_metrics("archive_thread", started, result.is_ok());
        result
    }

    async fn archive_threads(
        &self,
        request: Request<pb::ArchiveThreadsRequest>,
    ) -> Result<Response<pb::ArchiveThreadsResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:write")?;
            if caller.zdr() {
                return Err(Status::failed_precondition(
                    "ZDR callers cannot archive durable threads",
                ));
            }
            let req = request.into_inner();
            if req.org_id.trim().is_empty() || req.user_id.trim().is_empty() {
                return Err(Status::invalid_argument("org_id and user_id are required"));
            }
            caller.authorize_org(&req.org_id)?;
            caller.authorize_user(&req.user_id)?;
            let now = Utc::now();
            let mut tx = self
                .pool
                .begin()
                .await
                .map_err(|error| Status::internal(error.to_string()))?;
            let thread_ids: Vec<(String,)> = sqlx::query_as(
                "UPDATE threads
                 SET archived_at = $3
                 WHERE org_id = $1 AND user_id = $2 AND archived_at IS NULL
                 RETURNING id",
            )
            .bind(&req.org_id)
            .bind(&req.user_id)
            .bind(now)
            .fetch_all(&mut *tx)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;

            for (thread_id,) in &thread_ids {
                let resource = format!("thread:{thread_id}");
                sqlx::query(
                    "INSERT INTO events (id, event_type, run_id, payload, ts, org_id, user_id, correlation_id, causation_id, idempotency_key, resource_ref, type_url, producer, schema_version)
                     VALUES ($1, 'THREAD_ARCHIVED', $2, $3, $4, $5, $6, $7, '', $8, $9, $10, 'session-core', 1)",
                )
                .bind(new_ulid())
                .bind(thread_id)
                .bind(serde_json::json!({"thread_id": thread_id}))
                .bind(now)
                .bind(&req.org_id)
                .bind(&req.user_id)
                .bind(thread_id)
                .bind(derive_idempotency_hash(
                    "session-core",
                    "THREAD_ARCHIVED",
                    &resource,
                    &format!("{thread_id}:archived"),
                ))
                .bind(&resource)
                .bind(THREAD_ARCHIVED_TYPE_URL)
                .execute(&mut *tx)
                .await
                .map_err(|error| Status::internal(error.to_string()))?;
            }
            tx.commit()
                .await
                .map_err(|error| Status::internal(error.to_string()))?;

            Ok(Response::new(pb::ArchiveThreadsResponse {
                archived_count: u32::try_from(thread_ids.len()).unwrap_or(u32::MAX),
            }))
        }
        .await;
        record_metrics("archive_threads", started, result.is_ok());
        result
    }

    async fn delete_thread(
        &self,
        request: Request<pb::DeleteThreadRequest>,
    ) -> Result<Response<pb::DeleteThreadResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:write")?;
            let req = request.into_inner();
            if req.org_id.trim().is_empty() || req.thread_id.trim().is_empty() {
                return Err(Status::invalid_argument(
                    "org_id and thread_id are required",
                ));
            }
            caller.authorize_org(&req.org_id)?;
            authorize_thread_deletion_owner(&self.pool, &caller, &req.thread_id).await?;

            let mut tx = self
                .pool
                .begin()
                .await
                .map_err(|error| Status::internal(error.to_string()))?;
            // Lock the owner row before removing children. Appenders acquire a
            // key-share lock through the messages FK; taking FOR UPDATE first
            // makes an in-flight append wait and prevents it from racing the
            // final thread DELETE and resurrecting residue after this method's
            // child deletes have run.
            let owner: Option<(String, String)> =
                sqlx::query_as("SELECT org_id, user_id FROM threads WHERE id = $1 FOR UPDATE")
                    .bind(&req.thread_id)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(|error| Status::internal(error.to_string()))?;
            let (owner_org, owner_user) =
                owner.ok_or_else(|| Status::not_found("thread not found"))?;
            let caller_owner = caller.user_id().unwrap_or(caller.principal_id());
            if owner_org != req.org_id || owner_user != caller_owner {
                return Err(Status::permission_denied("thread owner required"));
            }
            let erased_memories = delete_thread_rows(&mut tx, &req.thread_id).await?;
            let deleted = sqlx::query("DELETE FROM threads WHERE id = $1 AND org_id = $2")
                .bind(&req.thread_id)
                .bind(&req.org_id)
                .execute(&mut *tx)
                .await
                .map_err(|error| Status::internal(error.to_string()))?
                .rows_affected()
                == 1;
            if !deleted {
                return Err(Status::not_found("thread not found"));
            }
            tx.commit()
                .await
                .map_err(|error| Status::internal(error.to_string()))?;

            // The durable rows are gone; their semantic twins are not, and
            // after the commit there is no id left in Postgres to find them by.
            // Best-effort by necessity, but never silent.
            self.propagate_semantic_erasure("delete_thread", &req.org_id, &erased_memories)
                .await;

            Ok(Response::new(pb::DeleteThreadResponse {
                thread_id: req.thread_id,
                deleted: true,
            }))
        }
        .await;
        record_metrics("delete_thread", started, result.is_ok());
        result
    }

    async fn delete_threads(
        &self,
        request: Request<pb::DeleteThreadsRequest>,
    ) -> Result<Response<pb::DeleteThreadsResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:write")?;
            let req = request.into_inner();
            if req.org_id.trim().is_empty() || req.user_id.trim().is_empty() {
                return Err(Status::invalid_argument("org_id and user_id are required"));
            }
            caller.authorize_org(&req.org_id)?;
            caller.authorize_user(&req.user_id)?;

            let mut tx = self
                .pool
                .begin()
                .await
                .map_err(|error| Status::internal(error.to_string()))?;
            // Lock the owner set before reading it so one bulk erase cannot
            // race a concurrent append/create into the same user's history.
            let thread_ids: Vec<(String,)> = sqlx::query_as(
                "SELECT id FROM threads WHERE org_id = $1 AND user_id = $2 ORDER BY id FOR UPDATE",
            )
            .bind(&req.org_id)
            .bind(&req.user_id)
            .fetch_all(&mut *tx)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;

            let mut erased_memories: Vec<crate::memory_erasure::ErasedMemory> = Vec::new();
            for (thread_id,) in &thread_ids {
                erased_memories.extend(delete_thread_rows(&mut tx, thread_id).await?);
                sqlx::query("DELETE FROM threads WHERE id = $1 AND org_id = $2 AND user_id = $3")
                    .bind(thread_id)
                    .bind(&req.org_id)
                    .bind(&req.user_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(|error| Status::internal(error.to_string()))?;
            }
            tx.commit()
                .await
                .map_err(|error| Status::internal(error.to_string()))?;

            self.propagate_semantic_erasure("delete_threads", &req.org_id, &erased_memories)
                .await;

            Ok(Response::new(pb::DeleteThreadsResponse {
                deleted_count: u32::try_from(thread_ids.len()).unwrap_or(u32::MAX),
            }))
        }
        .await;
        record_metrics("delete_threads", started, result.is_ok());
        result
    }

    async fn delete_space_threads(
        &self,
        request: Request<pb::DeleteSpaceThreadsRequest>,
    ) -> Result<Response<pb::DeleteSpaceThreadsResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            authorize_space_deletion_service(&caller)?;
            let req = request.into_inner();
            if req.org_id.trim().is_empty()
                || req.space_id.trim().is_empty()
                || req.owner_principal_id.trim().is_empty()
                || req.deletion_request_id.trim().is_empty()
            {
                return Err(Status::invalid_argument(
                    "org_id, space_id, owner_principal_id, and deletion_request_id are required",
                ));
            }
            caller.authorize_org(&req.org_id)?;

            let mut tx = self
                .pool
                .begin()
                .await
                .map_err(|error| Status::internal(error.to_string()))?;
            // Lock exactly the scoped owner rows before deleting their
            // descendants. This is idempotent: a retry after a committed
            // deletion observes an empty set and cannot touch an unscoped
            // thread in the same org.
            let thread_ids: Vec<(String,)> = sqlx::query_as(
                "SELECT id FROM threads
                 WHERE org_id = $1 AND user_id = $2 AND space_id = $3
                 ORDER BY id FOR UPDATE",
            )
            .bind(&req.org_id)
            .bind(&req.owner_principal_id)
            .bind(&req.space_id)
            .fetch_all(&mut *tx)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;

            // New background thread memories are mirrored into Letta with the
            // exact durable ID. Capture those IDs before the local deletion;
            // remote cleanup happens only after the local transaction commits
            // so a failed semantic bridge can never roll back or resurrect a
            // user's canonical erase.
            for (thread_id,) in &thread_ids {
                let ids: Vec<(String,)> = sqlx::query_as(
                    "SELECT id FROM agent_memory
                     WHERE org_id = $1 AND session_id = $2 AND scope = 'thread'",
                )
                .bind(&req.org_id)
                .bind(thread_id)
                .fetch_all(&mut *tx)
                .await
                .map_err(|error| Status::internal(error.to_string()))?;
                for (memory_id,) in &ids {
                    sqlx::query(
                        "INSERT INTO space_deletion_semantic_memory_receipts
                         (deletion_request_id, org_id, owner_principal_id, space_id, memory_id)
                         VALUES ($1, $2, $3, $4, $5)
                         ON CONFLICT (deletion_request_id, memory_id) DO NOTHING",
                    )
                    .bind(&req.deletion_request_id)
                    .bind(&req.org_id)
                    .bind(&req.owner_principal_id)
                    .bind(&req.space_id)
                    .bind(memory_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(|error| Status::internal(error.to_string()))?;
                }
                // Return value intentionally dropped: this path already
                // reconciles its semantic twins durably through
                // space_deletion_semantic_memory_receipts below, which survives
                // a crash and an idempotent retry. Routing it through the
                // best-effort helper as well would double-delete and could
                // downgrade a `confirmed` receipt to `unconfirmed`.
                let _ = delete_thread_rows(&mut tx, thread_id).await?;
                sqlx::query(
                    "DELETE FROM threads WHERE id = $1 AND org_id = $2 AND user_id = $3 AND space_id = $4",
                )
                .bind(thread_id)
                .bind(&req.org_id)
                .bind(&req.owner_principal_id)
                .bind(&req.space_id)
                .execute(&mut *tx)
                .await
                .map_err(|error| Status::internal(error.to_string()))?;
            }
            tx.commit()
                .await
                .map_err(|error| Status::internal(error.to_string()))?;

            // On an idempotent retry the canonical memory/thread rows have
            // already gone; the receipt ledger is the only safe source of the
            // exact IDs still needing an external reconciliation attempt.
            let semantic_memory_ids: Vec<(String,)> = sqlx::query_as(
                "SELECT memory_id FROM space_deletion_semantic_memory_receipts
                 WHERE deletion_request_id = $1 AND org_id = $2
                   AND owner_principal_id = $3 AND space_id = $4
                   AND status <> 'confirmed'
                 ORDER BY memory_id",
            )
            .bind(&req.deletion_request_id)
            .bind(&req.org_id)
            .bind(&req.owner_principal_id)
            .bind(&req.space_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
            let semantic_memory_attempted_count =
                u32::try_from(semantic_memory_ids.len()).unwrap_or(u32::MAX);
            if let Some(letta) = self.letta_memory.as_ref() {
                for (memory_id,) in &semantic_memory_ids {
                    let outcome = letta
                        .delete_detailed(&req.org_id, &req.owner_principal_id, memory_id)
                        .await;
                    // A `deleted=false` reply cannot distinguish an idempotent
                    // prior delete from an unknown/mismatched semantic record,
                    // so it remains unconfirmed until the future receipt ledger
                    // reconciles it. Never turn that ambiguity into success.
                    let degradation_reason = outcome.degradation_reason.clone();
                    let confirmed = outcome.deleted && degradation_reason.is_none();
                    let error = degradation_reason.unwrap_or_else(|| {
                        (!outcome.deleted)
                            .then_some("semantic_delete_not_confirmed")
                            .unwrap_or_default()
                    });
                    sqlx::query(
                        "UPDATE space_deletion_semantic_memory_receipts
                         SET status = $1, attempts = attempts + 1, last_error = $2, updated_at = now()
                         WHERE deletion_request_id = $3 AND memory_id = $4",
                    )
                    .bind(if confirmed { "confirmed" } else { "unconfirmed" })
                    .bind(&error)
                    .bind(&req.deletion_request_id)
                    .bind(memory_id)
                    .execute(&self.pool)
                    .await
                    .map_err(|error| Status::internal(error.to_string()))?;
                    if !confirmed {
                        warn!(
                            org_id = %req.org_id,
                            owner_principal_id = %req.owner_principal_id,
                            memory_id,
                            degradation = ?degradation_reason,
                            "Space deletion could not confirm an exact correlated Letta memory erase"
                        );
                    }
                }
            } else if !semantic_memory_ids.is_empty() {
                sqlx::query(
                    "UPDATE space_deletion_semantic_memory_receipts
                     SET status = 'unconfirmed', last_error = 'letta_not_configured', updated_at = now()
                     WHERE deletion_request_id = $1 AND status <> 'confirmed'",
                )
                .bind(&req.deletion_request_id)
                .execute(&self.pool)
                .await
                .map_err(|error| Status::internal(error.to_string()))?;
            }

            let (semantic_memory_unconfirmed_count,): (i64,) = sqlx::query_as(
                "SELECT count(*) FROM space_deletion_semantic_memory_receipts
                 WHERE deletion_request_id = $1 AND org_id = $2
                   AND owner_principal_id = $3 AND space_id = $4
                   AND status <> 'confirmed'",
            )
            .bind(&req.deletion_request_id)
            .bind(&req.org_id)
            .bind(&req.owner_principal_id)
            .bind(&req.space_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;

            Ok(Response::new(pb::DeleteSpaceThreadsResponse {
                deleted_count: u32::try_from(thread_ids.len()).unwrap_or(u32::MAX),
                semantic_memory_attempted_count,
                semantic_memory_unconfirmed_count: u32::try_from(semantic_memory_unconfirmed_count)
                    .unwrap_or(u32::MAX),
            }))
        }
        .await;
        record_metrics("delete_space_threads", started, result.is_ok());
        result
    }

    async fn get_context_assembly(
        &self,
        request: Request<pb::GetContextAssemblyRequest>,
    ) -> Result<Response<pb::GetContextAssemblyResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:read")?;
            // Data Plane grounding runs on the caller's own delegated
            // `aud=data-plane` credential. Resolve it while the request (and so
            // its metadata) is still intact.
            let dataplane_bearer = self.delegated_dataplane_bearer(&request, &caller)?;
            let req = request.into_inner();
            authorize_thread_owner(&self.pool, &caller, &req.thread_id, OwnerIntent::Mutate)
                .await?;
            // Run-level authorization only when a run is actually named. An
            // empty run_id means thread-scoped assembly — the inner function
            // explicitly supports it (`if run_id.is_empty() { None }`), and the
            // thread check above already authorizes that scope. Unguarded, this
            // lookup 404'd EVERY production request: the SPA's only caller
            // sends no run id, so `SELECT ... WHERE id = ''` matched nothing
            // and the whole context-inspector panel could never display data.
            if !req.run_id.trim().is_empty() {
                authorize_run_owner(&self.pool, &caller, &req.run_id, OwnerIntent::Mutate).await?;
            }
            get_context_assembly_inner(
                self,
                req,
                MemoryRetention::of(&caller),
                dataplane_bearer.as_ref(),
            )
            .await
        }
        .await;
        record_metrics("get_context_assembly", started, result.is_ok());
        result
    }
}

#[tonic::async_trait]
impl ManagedRunLifecycle for ManagedRunLifecycleService {
    async fn start_managed_run(
        &self,
        request: Request<pb::StartManagedRunRequest>,
    ) -> Result<Response<pb::StartManagedRunResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            let mut req = request.into_inner();
            caller.authorize_org(&req.org_id)?;
            let user_id = caller.user_id().ok_or_else(|| {
                Status::permission_denied("user-bound managed-run credential required")
            })?;
            req.user_id = user_id.to_owned();

            // ZDR users use the intentionally narrow metadata-only start
            // branch. It does not append a message or durable prompt; their
            // content remains exclusively in the live gateway request path.
            if !caller.zdr() {
                authorize_operation(&caller, "session:write")?;
                authorize_thread_owner(&self.pool, &caller, &req.thread_id, OwnerIntent::Mutate)
                    .await?;
            }
            terminalization::start_managed_run_inner(&self.pool, req, caller.zdr())
                .await
                .map(|started| Response::new(started.into_proto()))
        }
        .await;
        record_metrics("start_managed_run", started, result.is_ok());
        result
    }

    async fn record_terminal_outcome(
        &self,
        request: Request<pb::RecordTerminalOutcomeRequest>,
    ) -> Result<Response<pb::RecordTerminalOutcomeResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            let req = request.into_inner();
            let source = terminalization::source_from_wire(req.source)?;
            terminalization::authorize_terminalization_source(&caller, source)?;
            // The outbox deliberately carries no caller-controlled tenant
            // fields; bind the service token to the durable run owner instead.
            authorize_run_owner(&self.pool, &caller, &req.run_id, OwnerIntent::Mutate).await?;
            terminalization::record_terminal_outcome_inner(&self.pool, req)
                .await
                .map(|receipt| Response::new(receipt.into_proto()))
        }
        .await;
        record_metrics("record_terminal_outcome", started, result.is_ok());
        result
    }

    async fn heartbeat_managed_run(
        &self,
        request: Request<pb::HeartbeatManagedRunRequest>,
    ) -> Result<Response<pb::HeartbeatManagedRunResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            let req = request.into_inner();
            let source = terminalization::source_from_wire(req.source)?;
            terminalization::authorize_heartbeat_source(&caller, source)?;
            authorize_run_owner(&self.pool, &caller, &req.run_id, OwnerIntent::Mutate).await?;
            terminalization::heartbeat_managed_run_inner(&self.pool, req)
                .await
                .map(|heartbeat| Response::new(heartbeat.into_proto()))
        }
        .await;
        record_metrics("heartbeat_managed_run", started, result.is_ok());
        result
    }

    /// Persist a completed run's final answer on that run's own record.
    ///
    /// The only content-carrying call on this service, and gated accordingly:
    ///
    /// * **ZDR is refused, not silently skipped.** A zero-retention run was
    ///   promised no durable trace, and a caller that believes it stored an
    ///   answer would later report a conclusion nobody can read. Refusing says
    ///   so at the only moment the caller can still act on it.
    /// * **Owner-checked.** `runs.org_id`/`user_id` must match the caller, so a
    ///   run id cannot be used to write into another tenant's record.
    /// * **Server-bounded.** The response reports what was actually stored, so
    ///   truncation is reported rather than assumed away.
    async fn record_run_output(
        &self,
        request: Request<pb::RecordRunOutputRequest>,
    ) -> Result<Response<pb::RecordRunOutputResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            let req = request.into_inner();
            if caller.zdr() {
                return Err(Status::permission_denied(
                    "a zero-retention run has no durable output",
                ));
            }
            authorize_operation(&caller, "session:write")?;
            authorize_run_owner(&self.pool, &caller, &req.run_id, OwnerIntent::Mutate).await?;
            let output = req.output.trim();
            if output.is_empty() {
                return Err(Status::invalid_argument("output is required"));
            }
            let stored: String = output.chars().take(MAX_RUN_OUTPUT_CHARS).collect();
            let affected =
                sqlx::query("UPDATE runs SET final_output = $2, updated_at = now() WHERE id = $1")
                    .bind(&req.run_id)
                    .bind(&stored)
                    .execute(&self.pool)
                    .await
                    .map_err(|error| Status::internal(error.to_string()))?
                    .rows_affected();
            if affected == 0 {
                return Err(Status::not_found("run not found"));
            }
            Ok(Response::new(pb::RecordRunOutputResponse {
                run_id: req.run_id,
                stored_chars: u32::try_from(stored.chars().count()).unwrap_or(u32::MAX),
            }))
        }
        .await;
        record_metrics("record_run_output", started, result.is_ok());
        result
    }
}

/// Ceiling on a stored run answer. A run's conclusion, not its transcript — an
/// unbounded write here would turn `runs` into a second message store with none
/// of the retention machinery a thread has.
const MAX_RUN_OUTPUT_CHARS: usize = 16_384;

const RETRIEVAL_BUDGET_FRACTION: u32 = 4;
const RETRIEVAL_DEFAULT_TOP_K: i32 = 10;
const GRAPH_CONTRADICTIONS_LIMIT: i32 = 5;
const KNOWLEDGE_DOC_LIMIT: usize = 3;
const KNOWLEDGE_UNITS_PER_DOC_LIMIT: usize = 4;
const PERSIST_RETRIEVAL_LIMIT: usize = 5;

/// Outcome of a retrieval pass — text segments ready for the assembler
/// plus the structured metadata callers need to enrich context further
/// (knowledge units), persist the evidence (`memory_index`), and emit
/// learning-loop events (`mp.v1.retrieval.{used,low_confidence}`).
pub(crate) struct RetrievalEvidence {
    pub segments: Vec<String>,
    /// Distinct `document_ids` returned by the retrieval call, ordered by
    /// rank. Capped at top-K so downstream knowledge enrichment doesn't
    /// blow out latency.
    pub document_ids: Vec<String>,
    /// True iff the retrieval call returned `low_confidence: true` —
    /// signals the corpus has a gap and may want a wiki proposal.
    pub low_confidence: bool,
    /// Source of the segments. Distinguishes "live Data Plane" from
    /// "fell back to `memory_index`" for telemetry and tests.
    pub source: RetrievalSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetrievalSource {
    /// Returned by `RetrievalService.Retrieve`.
    DataPlane,
    /// Returned from `memory_index` rows persisted by an earlier call —
    /// happens when the Data Plane client is unconfigured, the call
    /// failed, or the thread's `org_id` is missing.
    LocalFallback,
}

/// Map a Data Plane `RetrieveResponse` into `RetrievalEvidence`, preferring the
/// `context_pack` facts, then raw candidate text, falling back to `local_fallback`.
fn map_retrieve_response(
    inner: ret_pb::RetrieveResponse,
    local_fallback: &[String],
) -> RetrievalEvidence {
    let low_confidence = inner.low_confidence;
    let mut document_ids: Vec<String> = inner
        .candidates
        .iter()
        .map(|c| c.document_id.clone())
        .filter(|d| !d.is_empty())
        .collect();
    document_ids.dedup();

    let segments = if let Some(pack) = inner.context_pack {
        let facts: Vec<String> = pack
            .facts
            .into_iter()
            .map(|f| format!("[{}] ({}): {}", f.source_title, f.source_type, f.text))
            .collect();
        if facts.is_empty() {
            local_fallback.to_vec()
        } else {
            facts
        }
    } else if inner.candidates.is_empty() {
        local_fallback.to_vec()
    } else {
        inner.candidates.into_iter().map(|c| c.text).collect()
    };

    if segments.is_empty() {
        return RetrievalEvidence {
            segments: local_fallback.to_vec(),
            document_ids: Vec::new(),
            low_confidence: false,
            source: RetrievalSource::LocalFallback,
        };
    }

    RetrievalEvidence {
        segments,
        document_ids,
        low_confidence,
        source: RetrievalSource::DataPlane,
    }
}

impl SessionService {
    /// Erase the semantic twins of memory rows a bulk delete just removed, and
    /// report honestly when it could not be established.
    ///
    /// Called AFTER the transaction commits, deliberately: the durable rows are
    /// the source of truth for existence and authorization, and a slow or
    /// degraded semantic tier must never hold a user's delete transaction open
    /// or roll it back. The consequence — a delete that Postgres completed and
    /// the vector store did not — is exactly what the warning and the
    /// `mp_session_semantic_erasure_unconfirmed_total` counter exist to make
    /// visible, rather than the silent orphaning this replaced.
    async fn propagate_semantic_erasure(
        &self,
        operation: &'static str,
        org_id: &str,
        erased: &[crate::memory_erasure::ErasedMemory],
    ) {
        if erased.is_empty() {
            return;
        }
        let outcome = crate::memory_erasure::erase_semantic_copies(
            self.letta_memory.as_ref(),
            org_id,
            erased,
        )
        .await;
        if outcome.is_complete() {
            return;
        }
        metrics::counter!(
            "mp_session_semantic_erasure_unconfirmed_total",
            "operation" => operation
        )
        .increment(outcome.unconfirmed);
        warn!(
            operation,
            org_id,
            considered = outcome.considered(),
            confirmed = outcome.confirmed,
            unconfirmed = outcome.unconfirmed,
            degradation = ?outcome.degradation_reason,
            "semantic memory erasure incomplete: durable rows are gone but their \
             semantic twins could not be confirmed erased"
        );
    }

    /// Re-verify the caller's delegated Data Plane credential, if any.
    ///
    /// # Errors
    ///
    /// Propagates the verifier's status for a malformed, unverifiable, or
    /// identity-mismatched delegation. When no verifier is configured, a
    /// delegation that was nonetheless supplied fails closed instead of being
    /// forwarded unverified.
    #[allow(clippy::result_large_err)]
    fn delegated_dataplane_bearer<T>(
        &self,
        request: &Request<T>,
        caller: &VerifiedIdentity,
    ) -> Result<Option<DelegatedDataPlaneBearer>, Status> {
        match &self.auth {
            Some(auth) => auth.delegated_data_plane_bearer(request, caller),
            None if request
                .metadata()
                .get(DATA_PLANE_AUTH_METADATA_KEY)
                .is_some() =>
            {
                Err(Status::unavailable(
                    "delegated Data Plane credential cannot be verified",
                ))
            }
            None => Ok(None),
        }
    }

    async fn fetch_retrieval_segments(
        &self,
        thread_id: &str,
        thread_messages: &[(String, String)],
        max_tokens: u32,
        local_fallback: &[String],
        // Already resolved by the calling gateway from its signed `sovereign`
        // claim and request-declared value — see
        // `GetContextAssemblyRequest.sovereign_required`'s doc. This crate holds
        // no claim of its own for the axis, so it is forwarded verbatim rather
        // than re-derived.
        sovereign_required: bool,
        bearer: Option<&DelegatedDataPlaneBearer>,
    ) -> RetrievalEvidence {
        let local = || RetrievalEvidence {
            segments: local_fallback.to_vec(),
            document_ids: Vec::new(),
            low_confidence: false,
            source: RetrievalSource::LocalFallback,
        };

        let mut client = match &self.retrieval_client {
            Some(c) => c.clone(),
            None => return local(),
        };

        // Data Plane requires the caller's own `aud=data-plane` bearer and there
        // is no honest substitute, so an undelegated call answers from durable
        // local memory instead of attempting a call that can only 401.
        let Some(bearer) = bearer else {
            tracing::debug!(
                thread_id,
                "no delegated Data Plane credential; using local memory for retrieval"
            );
            return local();
        };

        let query = thread_messages
            .iter()
            .rev()
            .find(|(role, _)| role == "user")
            .map(|(_, content)| content.as_str());

        let query = match query {
            Some(q) if !q.is_empty() => q,
            _ => return local(),
        };

        let org_id: Option<String> =
            match sqlx::query_scalar("SELECT org_id FROM threads WHERE id = $1")
                .bind(thread_id)
                .fetch_optional(&self.pool)
                .await
            {
                Ok(v) => v,
                Err(e) => {
                    warn!(error = %e, thread_id, "failed to look up org_id for retrieval");
                    return local();
                }
            };

        let org_id = match org_id {
            Some(id) if !id.is_empty() => id,
            _ => return local(),
        };

        let budget = if max_tokens > 0 {
            Some(i32::try_from(max_tokens / RETRIEVAL_BUDGET_FRACTION).unwrap_or(i32::MAX))
        } else {
            None
        };

        let retrieve_req = ret_pb::RetrieveRequest {
            org_id,
            query: query.to_owned(),
            filters: None,
            top_k: RETRIEVAL_DEFAULT_TOP_K,
            user_id: None,
            role: None,
            query_expansion: None,
            reranker_model: None,
            top_k_before_rerank: None,
            zdr_mode: None,
            context_budget_tokens: budget,
            context_format: Some("toon".to_owned()),
            agent_id: None,
            // Jurisdiction axis. Populated rather than left absent because Data
            // Plane v2 reads an absent value as `true` — fail-closed — which the
            // configured Azure-hosted embedding provider can never satisfy, so
            // an absent field made every one of these context-assembly
            // retrievals fail before retrieval ran (silently: the `Err` arm
            // below falls back to local context).
            //
            // Forwarded from `GetContextAssemblyRequest.sovereign_required`
            // (see this fn's `sovereign_required` param doc) rather than
            // re-derived: the calling gateway already resolved its signed
            // `sovereign` claim against its request-declared value, and this
            // crate has no claim of its own to add. A signed `sovereign = true`
            // on the forwarded bearer is still honoured regardless —
            // retrieval-engine-rs re-applies its own floor on receipt — so
            // this can only fail to raise the posture, never relax one.
            sovereign_required: Some(sovereign_required),
        };

        let request = match authorize_dataplane(retrieve_req, bearer) {
            Ok(request) => request,
            Err(e) => {
                warn!(error = %e, "retrieval credential cannot be forwarded, using local fallback");
                return local();
            }
        };

        match client.retrieve(request).await {
            Ok(resp) => map_retrieve_response(resp.into_inner(), local_fallback),
            Err(e) => {
                warn!(error = %e, "retrieval service call failed, using local fallback");
                local()
            }
        }
    }

    /// Fetch knowledge-unit context for the top-N retrieval `doc_ids`.
    /// `KnowledgeService.GetKnowledgeUnits` returns chunks adjacent to
    /// the retrieved candidates — useful when the candidate text alone
    /// is too narrow (e.g. it's the answer span but the model needs the
    /// surrounding paragraph to interpret it). Calls are dispatched
    /// concurrently so latency is `~max(per_call)` not `sum`.
    async fn fetch_knowledge_segments(
        &self,
        thread_id: &str,
        document_ids: &[String],
        bearer: Option<&DelegatedDataPlaneBearer>,
    ) -> Vec<String> {
        let client = match &self.knowledge_client {
            Some(c) => c.clone(),
            None => return Vec::new(),
        };
        if document_ids.is_empty() {
            return Vec::new();
        }
        let Some(bearer) = bearer else {
            tracing::debug!(
                thread_id,
                "no delegated Data Plane credential; skipping knowledge units"
            );
            return Vec::new();
        };
        let org_id: Option<String> =
            match sqlx::query_scalar("SELECT org_id FROM threads WHERE id = $1")
                .bind(thread_id)
                .fetch_optional(&self.pool)
                .await
            {
                Ok(v) => v,
                Err(e) => {
                    warn!(error = %e, thread_id, "failed to look up org_id for knowledge");
                    return Vec::new();
                }
            };
        let org_id = match org_id {
            Some(id) if !id.is_empty() => id,
            _ => return Vec::new(),
        };

        let take = document_ids.len().min(KNOWLEDGE_DOC_LIMIT);
        let mut requests = Vec::with_capacity(take);
        for doc_id in &document_ids[..take] {
            let req = know_pb::GetKnowledgeUnitsRequest {
                document_id: doc_id.clone(),
                org_id: org_id.clone(),
            };
            match authorize_dataplane(req, bearer) {
                Ok(request) => requests.push(request),
                Err(e) => {
                    warn!(error = %e, "knowledge credential cannot be forwarded");
                    return Vec::new();
                }
            }
        }
        let futures = requests.into_iter().map(|req| {
            let mut c = client.clone();
            async move { c.get_knowledge_units(req).await }
        });
        let results = futures::future::join_all(futures).await;

        let mut out = Vec::new();
        for resp in results {
            let Ok(resp) = resp else { continue };
            let units = resp.into_inner().units;
            for u in units.into_iter().take(KNOWLEDGE_UNITS_PER_DOC_LIMIT) {
                if u.text.is_empty() {
                    continue;
                }
                out.push(format!(
                    "[doc:{} chunk:{}] {}",
                    u.document_id, u.chunk_index, u.text
                ));
            }
        }
        out
    }

    /// Persist the top retrieval segments to `memory_index` with topic
    /// `RETRIEVAL` so subsequent context-assembly calls on this thread
    /// can fall back to them when Data Plane is unreachable, and so the
    /// model "remembers" what it saw across calls instead of refetching.
    /// Bounded by `PERSIST_RETRIEVAL_LIMIT` to keep `memory_index` lean.
    /// Failures are logged and ignored — persistence is best-effort.
    async fn persist_retrieval_evidence(&self, thread_id: &str, segments: &[String]) {
        if segments.is_empty() {
            return;
        }
        let org_id: Option<String> =
            match sqlx::query_scalar("SELECT org_id FROM threads WHERE id = $1")
                .bind(thread_id)
                .fetch_optional(&self.pool)
                .await
            {
                Ok(v) => v,
                Err(e) => {
                    warn!(error = %e, thread_id, "skip retrieval persist: org_id lookup failed");
                    return;
                }
            };
        let Some(org_id) = org_id.filter(|s| !s.is_empty()) else {
            return;
        };

        let take = segments.len().min(PERSIST_RETRIEVAL_LIMIT);
        for content in &segments[..take] {
            if content.is_empty() {
                continue;
            }
            let id = new_ulid();
            if let Err(e) = sqlx::query(
                "INSERT INTO memory_index (id, thread_id, topic, content, org_id) \
                 VALUES ($1, $2, 'RETRIEVAL', $3, $4)",
            )
            .bind(&id)
            .bind(thread_id)
            .bind(content)
            .bind(&org_id)
            .execute(&self.pool)
            .await
            {
                warn!(error = %e, "failed to persist retrieval evidence");
                return;
            }
        }
    }

    /// Fetch graph-aware context segments. Currently surfaces active contradictions
    /// for the thread's org so the model is aware of conflicting claims when reasoning
    /// over retrieval evidence. Returns an empty vec if the graph service is unconfigured
    /// or any call fails — graph context is opportunistic, never blocking.
    async fn fetch_graph_segments(
        &self,
        thread_id: &str,
        bearer: Option<&DelegatedDataPlaneBearer>,
    ) -> Vec<String> {
        let mut client = match &self.graph_client {
            Some(c) => c.clone(),
            None => return Vec::new(),
        };

        let Some(bearer) = bearer else {
            tracing::debug!(
                thread_id,
                "no delegated Data Plane credential; skipping graph contradictions"
            );
            return Vec::new();
        };

        let org_id: Option<String> =
            match sqlx::query_scalar("SELECT org_id FROM threads WHERE id = $1")
                .bind(thread_id)
                .fetch_optional(&self.pool)
                .await
            {
                Ok(v) => v,
                Err(e) => {
                    warn!(error = %e, thread_id, "failed to look up org_id for graph context");
                    return Vec::new();
                }
            };

        let org_id = match org_id {
            Some(id) if !id.is_empty() => id,
            _ => return Vec::new(),
        };

        let req = graph_pb::GetContradictionsRequest {
            org_id,
            entity_id: None,
            limit: GRAPH_CONTRADICTIONS_LIMIT,
            offset: 0,
        };

        let request = match authorize_dataplane(req, bearer) {
            Ok(request) => request,
            Err(e) => {
                warn!(error = %e, "graph credential cannot be forwarded");
                return Vec::new();
            }
        };

        match client.get_contradictions(request).await {
            Ok(resp) => resp
                .into_inner()
                .contradictions
                .into_iter()
                .map(|c| {
                    let prov = if c.provenance.is_empty() {
                        String::new()
                    } else {
                        format!(" [src: {}]", c.provenance)
                    };
                    format!("contradiction ({}): {}{}", c.status, c.text, prov)
                })
                .collect(),
            Err(e) => {
                warn!(error = %e, "graph contradictions call failed");
                Vec::new()
            }
        }
    }
}

pub(crate) struct AssemblyInputs {
    pub policy_id: String,
    pub workspace_id: String,
    pub agent_id: Option<String>,
    pub user_id: Option<String>,
    pub prompt_goal: Option<String>,
    pub thread_messages: Vec<(String, String)>,
    pub policy_segments: Vec<String>,
    pub workspace_segments: Vec<String>,
    pub agent_segments: Vec<String>,
    pub user_segments: Vec<String>,
    pub episodic_segments: Vec<String>,
    pub skill_index_segments: Vec<String>,
    pub skill_expansion_segments: Vec<String>,
    pub retrieval_segments: Vec<String>,
    pub knowledge_segments: Vec<String>,
    pub graph_segments: Vec<String>,
    pub run_id: String,
    pub max_tokens: u32,
}

fn push_all(
    kind: &str,
    items: &[String],
    budget: u32,
    segs: &mut Vec<pb::ContextSegment>,
    total: &mut u32,
) -> bool {
    for item in items {
        if !try_push(kind, item.clone(), budget, segs, total) {
            return false;
        }
    }
    true
}

/// Push one assembly tier: prefer explicit `segments`; otherwise fall back to a
/// single id-based segment (`{kind}:{id}`) when `fallback_id` is non-empty.
/// Returns `false` when the budget was exhausted and assembly should stop.
fn push_tier(
    kind: &str,
    segments: &[String],
    fallback_id: Option<&str>,
    budget: u32,
    segs: &mut Vec<pb::ContextSegment>,
    total: &mut u32,
) -> bool {
    if !segments.is_empty() {
        return push_all(kind, segments, budget, segs, total);
    }
    match fallback_id {
        Some(id) if !id.is_empty() => try_push(kind, format!("{kind}:{id}"), budget, segs, total),
        _ => true,
    }
}

/// Ceiling on the share of the budget that grounding + the run's own goal may
/// reserve ahead of raw chat history — expressed as `NUM/DEN` so history is
/// never starved in the opposite direction either.
const PROTECTED_BUDGET_NUM: u32 = 3;
const PROTECTED_BUDGET_DEN: u32 = 5;

/// Same charge `try_push` applies, exposed so a tier's cost can be reserved
/// before a lower-priority tier is allowed to spend it.
///
/// This is a real BPE count plus an explicit safety margin, NOT the old
/// `len() / 4`. That heuristic was wrong in both directions and the directions
/// fail differently: it over-charged English prose by ~60 % (silently trimming
/// grounding that would have fit) and under-charged Norwegian-with-numerals and
/// JSON tool output by 20-26 % (overflowing the provider's input limit). It also
/// divided BYTE length, so every æ/ø/å inflated the numerator. See `mp_tokens`.
fn estimated_tokens(content: &str) -> u32 {
    mp_tokens::count_for_budget(content)
}

fn try_push(
    kind: &str,
    content: String,
    budget: u32,
    segs: &mut Vec<pb::ContextSegment>,
    total: &mut u32,
) -> bool {
    let est = estimated_tokens(&content);
    if budget != 0 && total.saturating_add(est) > budget {
        return false;
    }
    *total = total.saturating_add(est);
    segs.push(pb::ContextSegment {
        kind: kind.to_owned(),
        content,
        estimated_tokens: est,
    });
    true
}

pub(crate) fn assemble_segments(inputs: &AssemblyInputs) -> (Vec<pb::ContextSegment>, u32) {
    let mut segments: Vec<pb::ContextSegment> = Vec::new();
    let mut total: u32 = 0;
    let budget = inputs.max_tokens;

    if !push_tier(
        "policy",
        &inputs.policy_segments,
        Some(&inputs.policy_id),
        budget,
        &mut segments,
        &mut total,
    ) {
        return (segments, total);
    }
    if !push_tier(
        "workspace",
        &inputs.workspace_segments,
        Some(&inputs.workspace_id),
        budget,
        &mut segments,
        &mut total,
    ) {
        return (segments, total);
    }
    if !push_tier(
        "agent",
        &inputs.agent_segments,
        inputs.agent_id.as_deref(),
        budget,
        &mut segments,
        &mut total,
    ) {
        return (segments, total);
    }
    if !push_tier(
        "user",
        &inputs.user_segments,
        inputs.user_id.as_deref(),
        budget,
        &mut segments,
        &mut total,
    ) {
        return (segments, total);
    }

    // Chat history is the one UNBOUNDED tier here (20 turns that now routinely
    // carry multi-KB tool results), and it used to be spent first and allowed to
    // `return` out of the whole assembly. So a rich ERP thread silently dropped
    // every tier below: retrieval, knowledge, graph — and the run's own goal,
    // which is pushed last of all. The assistant then answered a grounded
    // question with no grounding, and reported none, which is the exact failure
    // a cited-answer product cannot ship.
    //
    // So: reserve what grounding and the goal actually cost (capped at a share
    // of the budget so history keeps room too), let history fill only the rest,
    // and let it fall short WITHOUT aborting the tiers underneath it.
    let prompt = match &inputs.prompt_goal {
        Some(g) if !g.is_empty() => g.clone(),
        _ => format!("run:{}", inputs.run_id),
    };
    let protected_cost = inputs
        .retrieval_segments
        .iter()
        .chain(inputs.knowledge_segments.iter())
        .chain(inputs.graph_segments.iter())
        .map(|segment| estimated_tokens(segment))
        .fold(0_u32, u32::saturating_add)
        .saturating_add(estimated_tokens(&prompt));
    let thread_budget = if budget == 0 {
        0
    } else {
        let reservation_cap = budget
            .saturating_div(PROTECTED_BUDGET_DEN)
            .saturating_mul(PROTECTED_BUDGET_NUM);
        budget.saturating_sub(protected_cost.min(reservation_cap))
    };

    // Newest-first while filling, then restored to chronological order: the
    // messages that fall off must be the OLDEST. Iterating forward dropped the
    // most recent turns instead — the ones the user is actually replying to.
    let mut thread_kept: Vec<String> = Vec::new();
    let mut thread_spend = total;
    for (role, content) in inputs.thread_messages.iter().rev() {
        let rendered = format!("{role}: {content}");
        let est = estimated_tokens(&rendered);
        if thread_budget != 0 && thread_spend.saturating_add(est) > thread_budget {
            break;
        }
        thread_spend = thread_spend.saturating_add(est);
        thread_kept.push(rendered);
    }
    for rendered in thread_kept.into_iter().rev() {
        if !try_push("thread", rendered, budget, &mut segments, &mut total) {
            break;
        }
    }

    let tail_tiers: [(&str, &[String]); 6] = [
        ("episodic", &inputs.episodic_segments),
        ("skill_index", &inputs.skill_index_segments),
        ("skill_expansion", &inputs.skill_expansion_segments),
        ("retrieval", &inputs.retrieval_segments),
        ("knowledge", &inputs.knowledge_segments),
        ("graph", &inputs.graph_segments),
    ];
    for (kind, tier) in tail_tiers {
        // `break`, not `return`: one oversized tier must not swallow the run's
        // own goal, which is pushed after this loop.
        if !push_all(kind, tier, budget, &mut segments, &mut total) {
            break;
        }
    }

    // `prompt` was computed above so its cost could be reserved before history
    // spent the budget; pushing it is what that reservation was for.
    let _ = try_push("prompt", prompt, budget, &mut segments, &mut total);

    (segments, total)
}

fn event_type_to_i32(s: &str) -> i32 {
    match s {
        "SESSION_START" => 1,
        "SESSION_END" => 2,
        "RUN_STARTED" => 90,
        "RUN_COMPLETED" => 91,
        "RUN_FAILED" => 92,
        "STEP_COMPLETED" => 101,
        "CHECKPOINT_SAVED" => 110,
        "THREAD_CREATED" => 120,
        "MESSAGE_APPENDED" => 121,
        "BROWSER_ACTION_DISPATCHED" => 140,
        "BROWSER_OBSERVATION_RECEIVED" => 141,
        "BROWSER_RUN_PAUSED" => 142,
        "BROWSER_RUN_RESUMED" => 143,
        "BROWSER_ACTION_APPROVAL_REQUIRED" => 144,
        "BROWSER_ACTION_DECIDED" => 145,
        "APPROVAL_CONTINUATION_VERIFIED" => 146,
        _ => 0,
    }
}

/// Resolve a Data Plane endpoint, accepting either spelling of the variable.
///
/// This service historically read only `DATAPLANE_<TIER>_ADDR`, while
/// model-gateway (`state.rs::from_env`) reads `DATAPLANE_<TIER>_URL` first and
/// falls back to `_ADDR`. Compose was written against model-gateway's spelling,
/// so session-core silently received nothing and every tier fell back to
/// `localhost` — inside its own container. Because the clients are built with
/// `connect_lazy()` they still look configured, and the callers degrade to empty
/// on RPC failure by design, so the whole retrieval/knowledge/graph grounding
/// path went quiet without a single error.
///
/// Accepting both spellings here means a future compose edit cannot reintroduce
/// that drift. An empty value is treated as unset so `FOO=` in an env file does
/// not defeat the fallback.
fn dataplane_addr(tier: &str, default: &str) -> String {
    resolve_dataplane_addr(
        &["URL", "ADDR"].map(|s| std::env::var(format!("DATAPLANE_{tier}_{s}")).ok()),
        default,
    )
}

/// Pure precedence core of [`dataplane_addr`], split out so the ordering and the
/// empty-string handling are testable without mutating process env (which races
/// under a parallel test runner).
fn resolve_dataplane_addr(candidates: &[Option<String>], default: &str) -> String {
    candidates
        .iter()
        .flatten()
        .find(|v| !v.trim().is_empty())
        .map_or_else(|| default.to_string(), Clone::clone)
}

/// Start the gRPC server on :9091.
///
/// # Errors
///
/// Returns an error if the server fails to bind.
pub async fn serve(
    pool: PgPool,
    events_tx: tokio::sync::broadcast::Sender<mp_contracts::model_plane::v1::OrchestrationEvent>,
    letta_memory: Option<LettaMemoryAdapter>,
    auth: JwtVerifier,
) -> anyhow::Result<()> {
    let addr = "0.0.0.0:9091".parse()?;
    info!("gRPC listening on :9091");

    let retrieval_addr = dataplane_addr("RETRIEVAL", "http://localhost:50052");

    let retrieval_client = match Channel::from_shared(retrieval_addr.clone()) {
        Ok(ep) => {
            let ch = ep.connect_lazy();
            info!(addr = %retrieval_addr, "retrieval client configured (lazy connect)");
            Some(RetrievalServiceClient::new(ch))
        }
        Err(e) => {
            warn!(error = %e, "invalid DATAPLANE_RETRIEVAL_ADDR, skipping retrieval client");
            None
        }
    };

    let graph_addr = dataplane_addr("GRAPH", "http://localhost:50053");

    let graph_client = match Channel::from_shared(graph_addr.clone()) {
        Ok(ep) => {
            let ch = ep.connect_lazy();
            info!(addr = %graph_addr, "graph client configured (lazy connect)");
            Some(GraphServiceClient::new(ch))
        }
        Err(e) => {
            warn!(error = %e, "invalid DATAPLANE_GRAPH_ADDR, skipping graph client");
            None
        }
    };

    // Knowledge service typically lives on the same port as retrieval
    // (Data Plane retrieval-engine-rs serves both); separate env var so
    // operators can override if they're split.
    let knowledge_addr = dataplane_addr("KNOWLEDGE", &retrieval_addr);

    let knowledge_client = match Channel::from_shared(knowledge_addr.clone()) {
        Ok(ep) => {
            let ch = ep.connect_lazy();
            info!(addr = %knowledge_addr, "knowledge client configured (lazy connect)");
            Some(KnowledgeServiceClient::new(ch))
        }
        Err(e) => {
            warn!(error = %e, "invalid DATAPLANE_KNOWLEDGE_ADDR, skipping knowledge client");
            None
        }
    };

    let orchestration =
        crate::orchestration_grpc::OrchestrationGrpc::new_from_env(pool.clone(), events_tx).await;

    // Wave 7 — fine-tuning job state machine. Owns the `finetune_jobs` table
    // in this same Postgres. Provider HTTP (Azure OpenAI) lives in the gateway.
    let finetune = crate::finetune_grpc::FinetuneJobsService::new(pool.clone());
    let memory = crate::memory_grpc::MemoryGrpc::new(pool.clone(), letta_memory.clone());
    // Verevon intent layer ("model router") runtime policy. Owns the singleton
    // `routing_policy` JSONB row in this same Postgres; the BFF writes and
    // inference-core polls it.
    let routing = crate::routing_policy_grpc::RoutingPolicyService::new(pool.clone());
    // Run read model + cancel path for the runs-history UI. Owns the `runs` +
    // `events` tables in this same Postgres (read-only here, plus the cancel
    // status flip).
    let runs = crate::run_service_grpc::RunServiceImpl::new(pool.clone());

    // Tool-action intent is committed to Postgres with the step. The outbox
    // reconnects and retries NATS delivery independently.
    let nats_url = std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".into());
    let audit_outbox = crate::audit_publisher::AuditOutbox::new(pool.clone(), nats_url);
    audit_outbox.start();
    let audit_publisher = Some(audit_outbox);

    let managed_lifecycle = ManagedRunLifecycleService { pool: pool.clone() };

    let session = SessionService {
        pool,
        retrieval_client,
        graph_client,
        knowledge_client,
        letta_memory,
        audit_publisher,
        auth: Some(auth.clone()),
    };

    let (mut health_reporter, health_service) = tonic_health::server::health_reporter();
    for service_name in HEALTH_SERVICE_NAMES {
        health_reporter
            .set_service_status(service_name, tonic_health::ServingStatus::Serving)
            .await;
    }

    tonic::transport::Server::builder()
        .add_service(health_service)
        .add_service(SessionCoreServer::with_interceptor(session, auth.clone()))
        .add_service(ManagedRunLifecycleServer::with_interceptor(
            managed_lifecycle,
            auth.clone(),
        ))
        .add_service(OrchestrationCoreServiceServer::with_interceptor(
            orchestration,
            auth.clone(),
        ))
        .add_service(FinetuneJobsServer::with_interceptor(finetune, auth.clone()))
        .add_service(MemoryServiceServer::with_interceptor(memory, auth.clone()))
        .add_service(RoutingPolicyServer::with_interceptor(routing, auth.clone()))
        .add_service(RunServiceServer::with_interceptor(runs, auth))
        .serve(addr)
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        agent_run_thread_key, append_letta_memory_rows, assemble_segments,
        assistant_reply_slot_is_open, authorize_dataplane, claim_scheduled_step_inner,
        complete_step_inner, derive_idempotency_hash, finalize_tool_action_inner,
        normalize_thread_presentation_text, pb, record_scheduled_step_receipt_inner,
        reserve_tool_action_inner, resolve_dataplane_addr, resolve_residency,
        resolve_thread_origin, scheduled_run_execution_payload_digest,
        semantic_context_search_status, support_thread_id, thread_append_payload_digest,
        thread_create_payload_digest, valid_scheduled_run_identifier,
        validate_append_space_context_shape, validate_scheduled_step_request,
        validate_thread_space_context_shape, validate_user_checkpoint,
        verify_append_space_decision_with_key, verify_scheduled_run_decision_with_keys,
        verify_scheduled_run_execution_decision_with_keys, verify_thread_space_decision_with_key,
        verify_thread_space_decision_with_keys, AssemblyInputs, DelegatedDataPlaneBearer,
        LettaMemoryAdapter, MemoryRetention, ScheduledRunThreadContext,
        SemanticContextSearchStatus, VerifiedIdentity, CONTROL_SPACE_DECISION_AUDIENCE,
        CONTROL_SPACE_DECISION_VERSION, CONTROL_THREAD_APPEND_ACTION, CONTROL_THREAD_CREATE_ACTION,
        DEFAULT_RESIDENCY, HEALTH_SERVICE_NAMES, MAX_USER_CHECKPOINT_ID_BYTES,
        MAX_USER_CHECKPOINT_STATE_BYTES, MESSAGE_APPENDED_TYPE_URL, STEP_COMPLETED_TYPE_URL,
        THREAD_TITLE_MAX_CHARS, ZDR_MEMORY_READ_SUPPRESSED,
    };
    use crate::auth::{SPACE_DELETION_SCOPE, SPACE_DELETION_SERVICE};
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use chrono::{DateTime, Utc};
    use ed25519_dalek::{Signer as _, SigningKey};
    use mp_contracts::model_plane::v1::session_core_server::SessionCore;
    use std::collections::BTreeMap;
    use tonic::Request;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn assistant_reply_slot_opens_only_directly_after_a_user_turn() {
        // The decision-covered user turn opens exactly one reply slot.
        assert!(assistant_reply_slot_is_open(Some("user")));
        assert!(assistant_reply_slot_is_open(Some(" user ")));
        // No verified turn to answer: an empty Space thread stays closed.
        assert!(!assistant_reply_slot_is_open(None));
        // The exchange is already complete or was never user-authorized.
        assert!(!assistant_reply_slot_is_open(Some("assistant")));
        assert!(!assistant_reply_slot_is_open(Some("system")));
        assert!(!assistant_reply_slot_is_open(Some("tool")));
    }

    #[test]
    fn scheduled_step_claim_shape_is_strict_and_digest_bound() {
        let digest = format!("sha256:{}", "a".repeat(64));
        let request = pb::ClaimScheduledStepRequest {
            run_id: "run-1".to_owned(),
            thread_id: "thread-1".to_owned(),
            org_id: "org-1".to_owned(),
            space_id: "space-1".to_owned(),
            schedule_id: "schedule-1".to_owned(),
            fire_key: "fire-1".to_owned(),
            template_digest: digest.clone(),
            step_id: "step-1".to_owned(),
            step_index: 0,
            policy_digest: digest,
            idempotency_key: "run-1:step-1".to_owned(),
        };
        validate_scheduled_step_request(&request).expect("valid claim");
        let mut wildcard = request.clone();
        wildcard.org_id = "*".to_owned();
        assert!(validate_scheduled_step_request(&wildcard).is_err());
        let mut missing_policy = request;
        missing_policy.policy_digest.clear();
        assert!(validate_scheduled_step_request(&missing_policy).is_err());
    }

    /// Real-Postgres proof for the scheduled-step receipt contract. This keeps
    /// the workflow's retry boundary honest: a duplicate claim returns the
    /// original receipt, an ambiguous provider result is terminalized as
    /// `unknown_outcome`, and a second terminal outcome cannot overwrite it.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn scheduled_step_claim_and_unknown_receipt_are_idempotent_against_real_pg() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL unset");
            return;
        };
        let pool = sqlx::PgPool::connect(&url).await.expect("connect pg");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");

        let suffix = mp_ids::new_ulid();
        let run_id = format!("scheduled-run-{suffix}");
        let thread_id = format!("scheduled-thread-{suffix}");
        let org_id = format!("scheduled-org-{suffix}");
        let space_id = format!("scheduled-space-{suffix}");
        let schedule_id = format!("schedule-{suffix}");
        let fire_key = format!("fire-{suffix}");
        let step_id = format!("{run_id}:step:0");
        let idempotency_key = format!("{fire_key}:step:0");
        let template_digest = format!("sha256:{}", "a".repeat(64));
        let policy_digest = format!("sha256:{}", "b".repeat(64));
        let audience_hash = format!("sha256:{}", "c".repeat(64));
        let metadata = serde_json::json!({
            "source": "scheduled_run",
            "schedule_id": schedule_id,
            "fire_key": fire_key,
            "template_digest": template_digest,
            "space_id": space_id,
        });

        sqlx::query(
            "INSERT INTO threads
             (id, session_key, org_id, user_id, space_id, space_decision_ref,
              recipient_audience_ref, recipient_audience_revision,
              recipient_audience_hash, privacy_policy_ref,
              resource_authorization_ref, authority_revision)
             VALUES ($1,$2,$3,'service:orchestrator-core',$4,$5,$6,1,$7,$8,$9,1)",
        )
        .bind(&thread_id)
        .bind(&thread_id)
        .bind(&org_id)
        .bind(&space_id)
        .bind(format!("decision-{suffix}"))
        .bind(format!("audience-{suffix}"))
        .bind(&audience_hash)
        .bind(format!("privacy-{suffix}"))
        .bind(format!("resource-{suffix}"))
        .execute(&pool)
        .await
        .expect("seed scheduled thread");
        sqlx::query(
            "INSERT INTO runs
             (id, thread_id, goal, org_id, user_id, status, metadata,
              space_id, space_decision_ref, recipient_audience_ref,
              recipient_audience_revision, recipient_audience_hash,
              privacy_policy_ref, resource_authorization_ref, authority_revision)
             VALUES ($1,$2,'scheduled proof goal',$3,'service:orchestrator-core',
                     'queued',$4,$5,$6,$7,1,$8,$9,$10,1)",
        )
        .bind(&run_id)
        .bind(&thread_id)
        .bind(&org_id)
        .bind(metadata)
        .bind(&space_id)
        .bind(format!("decision-{suffix}"))
        .bind(format!("audience-{suffix}"))
        .bind(&audience_hash)
        .bind(format!("privacy-{suffix}"))
        .bind(format!("resource-{suffix}"))
        .execute(&pool)
        .await
        .expect("seed scheduled run");

        let request = pb::ClaimScheduledStepRequest {
            run_id: run_id.clone(),
            thread_id: thread_id.clone(),
            org_id: org_id.clone(),
            space_id: space_id.clone(),
            schedule_id: schedule_id.clone(),
            fire_key: fire_key.clone(),
            template_digest: template_digest.clone(),
            step_id: step_id.clone(),
            step_index: 0,
            policy_digest: policy_digest.clone(),
            idempotency_key: idempotency_key.clone(),
        };
        let first = claim_scheduled_step_inner(&pool, request.clone())
            .await
            .expect("first claim")
            .into_inner();
        assert!(first.claimed);
        assert_eq!(first.status, "claimed");
        assert!(!first.receipt_id.is_empty());

        let duplicate = claim_scheduled_step_inner(&pool, request)
            .await
            .expect("duplicate claim")
            .into_inner();
        assert!(!duplicate.claimed);
        assert_eq!(duplicate.receipt_id, first.receipt_id);
        assert_eq!(duplicate.status, "claimed");

        let unknown = record_scheduled_step_receipt_inner(
            &pool,
            pb::RecordScheduledStepReceiptRequest {
                run_id: run_id.clone(),
                step_id: step_id.clone(),
                org_id: org_id.clone(),
                idempotency_key: idempotency_key.clone(),
                receipt_id: first.receipt_id.clone(),
                status: "unknown_outcome".to_owned(),
                output_digest: String::new(),
                error_code: "transport_ambiguous".to_owned(),
                unknown_outcome: true,
            },
        )
        .await
        .expect("record unknown outcome")
        .into_inner();
        assert!(unknown.recorded);
        assert_eq!(unknown.status, "unknown_outcome");

        let duplicate_unknown = record_scheduled_step_receipt_inner(
            &pool,
            pb::RecordScheduledStepReceiptRequest {
                run_id: run_id.clone(),
                step_id: step_id.clone(),
                org_id: org_id.clone(),
                idempotency_key: idempotency_key.clone(),
                receipt_id: first.receipt_id.clone(),
                status: "unknown_outcome".to_owned(),
                output_digest: String::new(),
                error_code: "transport_ambiguous".to_owned(),
                unknown_outcome: true,
            },
        )
        .await
        .expect("duplicate unknown outcome")
        .into_inner();
        assert!(!duplicate_unknown.recorded);

        let overwrite = record_scheduled_step_receipt_inner(
            &pool,
            pb::RecordScheduledStepReceiptRequest {
                run_id: run_id.clone(),
                step_id: step_id.clone(),
                org_id: org_id.clone(),
                idempotency_key,
                receipt_id: first.receipt_id,
                status: "completed".to_owned(),
                output_digest: format!("sha256:{}", "d".repeat(64)),
                error_code: String::new(),
                unknown_outcome: false,
            },
        )
        .await
        .expect_err("terminal receipt must not be overwritten");
        assert_eq!(overwrite.code(), tonic::Code::AlreadyExists);

        sqlx::query("DELETE FROM scheduled_step_receipts WHERE run_id = $1")
            .bind(&run_id)
            .execute(&pool)
            .await
            .expect("clean receipts");
        sqlx::query("DELETE FROM runs WHERE id = $1")
            .bind(&run_id)
            .execute(&pool)
            .await
            .expect("clean run");
        sqlx::query("DELETE FROM threads WHERE id = $1")
            .bind(&thread_id)
            .execute(&pool)
            .await
            .expect("clean thread");
    }

    #[test]
    fn thread_presentation_text_preserves_explicit_clear_and_rejects_oversize_values() {
        assert_eq!(
            normalize_thread_presentation_text("  An   explicit title  ", "title", 96)
                .expect("valid title"),
            Some("An explicit title".to_owned())
        );
        assert_eq!(
            normalize_thread_presentation_text("   ", "title", 96).expect("explicit clear"),
            None
        );
        let oversized = "x".repeat(THREAD_TITLE_MAX_CHARS + 1);
        let error = normalize_thread_presentation_text(&oversized, "title", THREAD_TITLE_MAX_CHARS)
            .expect_err("oversize title must not be silently truncated");
        assert_eq!(error.code(), tonic::Code::InvalidArgument);
    }

    #[test]
    fn scheduled_run_identifiers_are_nats_safe_and_never_path_like() {
        assert!(valid_scheduled_run_identifier("task_scheduled_1"));
        assert!(valid_scheduled_run_identifier("schedule_1"));
        for invalid in [
            "", " task", "task ", "task.id", "task/id", "task>1", "task\n1",
        ] {
            assert!(
                !valid_scheduled_run_identifier(invalid),
                "scheduled-run identifier {invalid:?} must be refused"
            );
        }
    }

    #[test]
    fn scheduled_run_preparation_accepts_a_fresh_retry_decision_only_for_the_same_fire() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-08-15T12:00:00Z")
            .expect("test time")
            .with_timezone(&Utc);
        let signing_key = SigningKey::from_bytes(&[28_u8; 32]);
        let key_id = "control-scheduled-run-preparation".to_owned();
        let mut request = pb::PrepareScheduledRunThreadRequest {
            org_id: "org-1".to_owned(),
            human_subject_id: "user-1".to_owned(),
            space_id: "space-1".to_owned(),
            schedule_id: "schedule-1".to_owned(),
            fire_key: "fire_20260815t120000z".to_owned(),
            run_id: "task_scheduled_1".to_owned(),
            system_thread_key: "schedule/schedule-1/fire_20260815t120000z".to_owned(),
            template_digest: format!("sha256:{}", "a".repeat(64)),
            idempotency_key: "schedule-1:fire_20260815t120000z".to_owned(),
            space_decision_ref: "decision-1".to_owned(),
            recipient_audience_ref: "audience-1".to_owned(),
            recipient_audience_revision: 2,
            recipient_audience_hash: "sha256:audience".to_owned(),
            privacy_policy_ref: "privacy-1".to_owned(),
            resource_authorization_ref: "resource-1".to_owned(),
            authority_revision: 7,
            action_schema_hash: "sha256:space-scheduled-run-v1".to_owned(),
            ..Default::default()
        };
        let sign = |request: &mut pb::PrepareScheduledRunThreadRequest,
                    decision_ref: &str,
                    nonce: &str| {
            request.space_decision_ref = decision_ref.to_owned();
            let mut claims = serde_json::json!({
                "decision_ref": decision_ref,
                "org_id": "org-1",
                "space_ref": "space-1",
                "subject_id": "user-1",
                "service_audience": "model-plane-capability-core",
                "action_id": "model.schedule.run",
                "action_schema_hash": "sha256:space-scheduled-run-v1",
                "payload_digest": "",
                "idempotency_key": request.idempotency_key.as_str(),
                "recipient_audience_ref": "audience-1",
                "recipient_audience_revision": 2,
                "recipient_audience_hash": "sha256:audience",
                "privacy_policy_ref": "privacy-1",
                "resource_authorization_ref": "resource-1",
                "purpose": "agent_work",
                "lawful_basis": "contract",
                "privacy_class": "internal",
                "third_party_processing_allowed": false,
                "retention_class": "standard",
                "residency": "swedencentral",
                "deletion_scope": "space",
                "zero_data_retention": false,
                "nonce": nonce,
                "authority_revision": 7,
                "membership_revision": 4,
                "privacy_revision": 5,
                "entitlement_revision": 3,
                "permissions": ["schedule:run"],
                "issued_at": (now - chrono::Duration::seconds(1)).to_rfc3339(),
                "expires_at": (now + chrono::Duration::minutes(1)).to_rfc3339(),
            });
            let decoded: super::ControlSpaceDecisionClaims =
                serde_json::from_value(claims.clone()).expect("test claims");
            request.payload_digest = super::scheduled_run_payload_digest(request, &decoded);
            claims["payload_digest"] = serde_json::json!(request.payload_digest.as_str());
            let payload =
                URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).expect("claims encode"));
            let encoded_key = URL_SAFE_NO_PAD.encode(key_id.as_bytes());
            let signing_input = format!("{CONTROL_SPACE_DECISION_VERSION}.{encoded_key}.{payload}");
            request.control_decision_token = format!(
                "{signing_input}.{}",
                URL_SAFE_NO_PAD.encode(signing_key.sign(signing_input.as_bytes()).to_bytes())
            );
        };
        let keys = BTreeMap::from([(key_id.clone(), signing_key.verifying_key())]);

        sign(&mut request, "decision-1", "nonce-1");
        verify_scheduled_run_decision_with_keys(&request, &keys, now)
            .expect("original decision must authorize the prepared fire");

        sign(&mut request, "decision-2", "nonce-2");
        verify_scheduled_run_decision_with_keys(&request, &keys, now)
            .expect("fresh retry decision must authorize the same prepared fire");

        request.fire_key = "different_fire".to_owned();
        assert_eq!(
            verify_scheduled_run_decision_with_keys(&request, &keys, now)
                .expect_err("a decision may not move to a different fire")
                .code(),
            tonic::Code::InvalidArgument
        );
    }

    /// A preparation activity can succeed in Session Core and then lose its
    /// response before Capability Core records the handoff. A retry receives a
    /// fresh Control decision (and therefore a new decision_ref/nonce), but it
    /// must converge on the same durable service-owned thread for the stable
    /// (org, owner, system_thread_key) fire identity. This exercises the real
    /// Postgres idempotency fence rather than only the signed-envelope helper.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn scheduled_run_prepare_retry_with_fresh_decision_reuses_thread_against_real_pg() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL unset");
            return;
        };
        let pool = sqlx::PgPool::connect(&url).await.expect("connect pg");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");

        let suffix = mp_ids::new_ulid();
        let org_id = format!("org-scheduled-retry-{suffix}");
        let owner = "service:orchestrator-core".to_owned();
        let system_thread_key = format!("schedule/retry-{suffix}/fire-{suffix}");
        let first = pb::CreateThreadRequest {
            session_key: system_thread_key.clone(),
            org_id: org_id.clone(),
            user_id: owner.clone(),
            space_id: "space-retry".to_owned(),
            space_decision_ref: "decision-first".to_owned(),
            recipient_audience_ref: "audience-retry".to_owned(),
            recipient_audience_revision: 4,
            recipient_audience_hash: "sha256:audience-retry".to_owned(),
            privacy_policy_ref: "privacy-retry".to_owned(),
            resource_authorization_ref: "resource-retry".to_owned(),
            authority_revision: 9,
            action_schema_hash: super::CONTROL_SCHEDULED_RUN_SCHEMA.to_owned(),
            payload_digest: "sha256:payload-retry".to_owned(),
            idempotency_key: "schedule-retry-fire".to_owned(),
            ..Default::default()
        };
        let first_id = super::create_thread_inner_preverified(&pool, first)
            .await
            .expect("first preparation")
            .into_inner()
            .thread_id;

        // The stable fire bindings are unchanged; only the fresh Control
        // decision reference and nonce would differ at the RPC boundary.
        let retry = pb::CreateThreadRequest {
            session_key: system_thread_key.clone(),
            org_id: org_id.clone(),
            user_id: owner.clone(),
            space_id: "space-retry".to_owned(),
            space_decision_ref: "decision-fresh-retry".to_owned(),
            recipient_audience_ref: "audience-retry".to_owned(),
            recipient_audience_revision: 4,
            recipient_audience_hash: "sha256:audience-retry".to_owned(),
            privacy_policy_ref: "privacy-retry".to_owned(),
            resource_authorization_ref: "resource-retry".to_owned(),
            authority_revision: 9,
            action_schema_hash: super::CONTROL_SCHEDULED_RUN_SCHEMA.to_owned(),
            payload_digest: "sha256:payload-retry".to_owned(),
            idempotency_key: "schedule-retry-fire".to_owned(),
            ..Default::default()
        };
        let retry_id = super::create_thread_inner_preverified(&pool, retry)
            .await
            .expect("fresh-decision retry")
            .into_inner()
            .thread_id;
        assert_eq!(
            retry_id, first_id,
            "same scheduled fire must reuse its thread"
        );

        let (count,): (i64,) = sqlx::query_as(
            "SELECT count(*) FROM threads WHERE org_id = $1 AND user_id = $2 AND session_key = $3",
        )
        .bind(&org_id)
        .bind(&owner)
        .bind(&system_thread_key)
        .fetch_one(&pool)
        .await
        .expect("count prepared threads");
        assert_eq!(count, 1, "retry must not create a second service thread");

        sqlx::query("DELETE FROM events WHERE run_id = $1")
            .bind(&first_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM threads WHERE id = $1")
            .bind(&first_id)
            .execute(&pool)
            .await
            .ok();
    }

    #[test]
    fn scheduled_run_execution_decision_is_bound_to_the_exact_prepared_thread() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-08-15T12:00:00Z")
            .expect("test time")
            .with_timezone(&Utc);
        let signing_key = SigningKey::from_bytes(&[29_u8; 32]);
        let key_id = "control-scheduled-run-execution".to_owned();
        let mut request = pb::StartScheduledRunRequest {
            thread_id: "thread-scheduled-1".to_owned(),
            run_id: "task_scheduled_1".to_owned(),
            org_id: "org-1".to_owned(),
            schedule_id: "schedule-1".to_owned(),
            fire_key: "2026-08-15T12:00:00Z".to_owned(),
            goal: "summarise the queue".to_owned(),
            human_subject_id: "user-1".to_owned(),
            template_digest: format!("sha256:{}", "a".repeat(64)),
            idempotency_key: "schedule-1:2026-08-15T12:00:00Z".to_owned(),
            ..Default::default()
        };
        let thread = ScheduledRunThreadContext {
            space_id: "space-1".to_owned(),
            recipient_audience_ref: "audience-1".to_owned(),
            recipient_audience_revision: 2,
            recipient_audience_hash: "sha256:audience".to_owned(),
            privacy_policy_ref: "privacy-1".to_owned(),
            resource_authorization_ref: "resource-1".to_owned(),
            authority_revision: 7,
        };
        let mut claims = serde_json::json!({
            "decision_ref": "decision-1",
            "org_id": "org-1",
            "space_ref": "space-1",
            "subject_id": "user-1",
            "service_audience": "model-plane-session-core",
            "action_id": "model.schedule.execute",
            "action_schema_hash": "sha256:space-scheduled-run-execute-v1",
            "payload_digest": "",
            "idempotency_key": request.idempotency_key.as_str(),
            "recipient_audience_ref": "audience-1",
            "recipient_audience_revision": 2,
            "recipient_audience_hash": "sha256:audience",
            "privacy_policy_ref": "privacy-1",
            "resource_authorization_ref": "resource-1",
            "purpose": "agent_work",
            "lawful_basis": "contract",
            "privacy_class": "internal",
            "third_party_processing_allowed": false,
            "retention_class": "standard",
            "residency": "swedencentral",
            "deletion_scope": "space",
            "zero_data_retention": false,
            "nonce": "nonce-1",
            "authority_revision": 7,
            "membership_revision": 4,
            "privacy_revision": 5,
            "entitlement_revision": 3,
            "permissions": ["schedule:execute"],
            "issued_at": (now - chrono::Duration::seconds(1)).to_rfc3339(),
            "expires_at": (now + chrono::Duration::minutes(1)).to_rfc3339(),
        });
        let decoded: super::ControlSpaceDecisionClaims =
            serde_json::from_value(claims.clone()).expect("test claims");
        claims["payload_digest"] =
            serde_json::json!(scheduled_run_execution_payload_digest(&request, &decoded));
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).expect("claims encode"));
        let encoded_key = URL_SAFE_NO_PAD.encode(key_id.as_bytes());
        let signing_input = format!("{CONTROL_SPACE_DECISION_VERSION}.{encoded_key}.{payload}");
        request.control_execution_decision_token = format!(
            "{signing_input}.{}",
            URL_SAFE_NO_PAD.encode(signing_key.sign(signing_input.as_bytes()).to_bytes())
        );
        let keys = BTreeMap::from([(key_id, signing_key.verifying_key())]);
        verify_scheduled_run_execution_decision_with_keys(&request, &thread, &keys, now)
            .expect("exact current execution decision must verify");

        let mut another_thread = request;
        another_thread.thread_id = "thread-scheduled-2".to_owned();
        assert_eq!(
            verify_scheduled_run_execution_decision_with_keys(&another_thread, &thread, &keys, now)
                .expect_err("execution bearer may not move to another prepared thread")
                .code(),
            tonic::Code::PermissionDenied
        );
    }

    #[test]
    fn semantic_memory_deletion_ledger_is_request_bound_and_never_has_a_success_default() {
        let migration =
            include_str!("../migrations/0026_space_deletion_semantic_memory_receipts.sql");
        for required in [
            "PRIMARY KEY (deletion_request_id, memory_id)",
            "DEFAULT 'pending'",
            "'pending', 'confirmed', 'unconfirmed'",
            "owner_principal_id",
            "space_id",
        ] {
            assert!(
                migration.contains(required),
                "semantic deletion migration missing {required}"
            );
        }
        assert!(!migration.contains("DEFAULT 'confirmed'"));
    }

    /// A Letta adapter whose Auth Core is a mock and whose memory endpoint is
    /// the discard port. Every adapter surface mints an Auth Core token before
    /// it touches the bridge, so "did the caller reach the adapter at all?" is
    /// answerable structurally by counting requests the mock received — no log
    /// scraping, and no dependence on the bridge being reachable.
    async fn letta_adapter_with_observable_auth() -> (MockServer, LettaMemoryAdapter) {
        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/letta-bridge/internal-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "letta-token",
                "expiresInSeconds": 300,
                "audience": "letta-bridge"
            })))
            .mount(&auth)
            .await;
        let adapter = LettaMemoryAdapter::new_for_test(
            "http://127.0.0.1:9",
            &auth.uri(),
            "session-core",
            "session-core-test-credential",
        );
        (auth, adapter)
    }

    async fn auth_request_count(auth: &MockServer) -> usize {
        auth.received_requests()
            .await
            .expect("mock server records requests")
            .len()
    }

    #[test]
    fn support_thread_ids_are_preserved_only_for_the_validated_uuid_namespace() {
        let id = "support_123e4567-e89b-12d3-a456-426614174000";
        assert_eq!(support_thread_id(id), Some(id));
        assert_eq!(support_thread_id("support_not-a-uuid"), None);
        assert_eq!(support_thread_id("ordinary-session-key"), None);
        assert_eq!(
            support_thread_id("support_123e4567-e89b-12d3-a456-426614174000-extra"),
            None
        );
    }

    #[test]
    fn origin_defaults_to_chat_when_undeclared_and_unscoped() {
        let req = pb::CreateThreadRequest {
            session_key: "ordinary-session-key".to_owned(),
            ..pb::CreateThreadRequest::default()
        };
        assert_eq!(resolve_thread_origin(&req).unwrap(), "chat");
    }

    /// This is the case that closes the Support-assist leak with zero caller
    /// changes: the existing `support_` session-key convention now also
    /// determines origin, rather than being read for the thread id alone.
    #[test]
    fn agent_run_thread_key_recognizes_the_console_prefix() {
        assert!(agent_run_thread_key("agent_run/01J8Z0Y1"));
        assert!(agent_run_thread_key("  agent_run/anything  "));
        assert!(!agent_run_thread_key("ordinary-session-key"));
        assert!(!agent_run_thread_key(
            "support_123e4567-e89b-12d3-a456-426614174000"
        ));
    }

    /// The Agent Run Console leak: it invokes with no thread/session key of its
    /// own, which used to be indistinguishable from an ordinary new chat. Once
    /// the console sends an `agent_run/`-prefixed session key, this closes it
    /// with no wire contract change.
    #[test]
    fn origin_defaults_to_agent_run_from_the_session_key_convention() {
        let req = pb::CreateThreadRequest {
            session_key: "agent_run/01J8Z0Y1QK3R7VZC9WYX8H6N2P".to_owned(),
            ..pb::CreateThreadRequest::default()
        };
        assert_eq!(resolve_thread_origin(&req).unwrap(), "agent_run");
    }

    #[test]
    fn origin_defaults_to_support_from_the_session_key_convention() {
        let req = pb::CreateThreadRequest {
            session_key: "support_123e4567-e89b-12d3-a456-426614174000".to_owned(),
            ..pb::CreateThreadRequest::default()
        };
        assert_eq!(resolve_thread_origin(&req).unwrap(), "support");
    }

    #[test]
    fn origin_defaults_to_space_when_space_id_is_set_and_origin_undeclared() {
        let req = pb::CreateThreadRequest {
            space_id: "space-1".to_owned(),
            ..pb::CreateThreadRequest::default()
        };
        assert_eq!(resolve_thread_origin(&req).unwrap(), "space");
    }

    #[test]
    fn declared_origin_is_validated_against_the_allowed_set() {
        let req = pb::CreateThreadRequest {
            origin: "literally-anything".to_owned(),
            ..pb::CreateThreadRequest::default()
        };
        assert!(resolve_thread_origin(&req).is_err());
    }

    #[test]
    fn declared_origin_is_trimmed_before_matching() {
        let req = pb::CreateThreadRequest {
            origin: "  agent_run  ".to_owned(),
            ..pb::CreateThreadRequest::default()
        };
        assert_eq!(resolve_thread_origin(&req).unwrap(), "agent_run");
    }

    /// The regression this whole change targets: the Agent Run Console (and
    /// Support-assist before it used the session-key convention) invoked with
    /// no space_id, so a caller declaring origin="space" without one -- or the
    /// reverse, space_id set under any other origin -- is exactly the
    /// contradiction that let an unscoped thread masquerade as something it
    /// was not. Both directions must be rejected, not just one.
    #[test]
    fn declared_space_origin_requires_a_space_id() {
        let req = pb::CreateThreadRequest {
            origin: "space".to_owned(),
            ..pb::CreateThreadRequest::default()
        };
        assert!(resolve_thread_origin(&req).is_err());
    }

    #[test]
    fn a_space_id_requires_declared_space_origin() {
        let req = pb::CreateThreadRequest {
            origin: "agent_run".to_owned(),
            space_id: "space-1".to_owned(),
            ..pb::CreateThreadRequest::default()
        };
        assert!(resolve_thread_origin(&req).is_err());
    }

    #[test]
    fn declared_space_origin_with_a_space_id_is_consistent() {
        let req = pb::CreateThreadRequest {
            origin: "space".to_owned(),
            space_id: "space-1".to_owned(),
            ..pb::CreateThreadRequest::default()
        };
        assert_eq!(resolve_thread_origin(&req).unwrap(), "space");
    }

    /// The other regression this closes: the Agent Run Console invokes with no
    /// thread/session key at all, so origin cannot be inferred for it the way
    /// support threads can be -- it can only be closed by the caller declaring
    /// origin explicitly. Confirm the declaration path actually accepts it.
    #[test]
    fn declared_agent_run_origin_is_accepted_with_no_other_signal() {
        let req = pb::CreateThreadRequest {
            origin: "agent_run".to_owned(),
            ..pb::CreateThreadRequest::default()
        };
        assert_eq!(resolve_thread_origin(&req).unwrap(), "agent_run");
    }

    #[test]
    fn declared_system_origin_is_accepted() {
        let req = pb::CreateThreadRequest {
            origin: "system".to_owned(),
            ..pb::CreateThreadRequest::default()
        };
        assert_eq!(resolve_thread_origin(&req).unwrap(), "system");
    }

    #[test]
    fn memory_retention_follows_only_the_issuer_zdr_claim() {
        assert_eq!(
            MemoryRetention::of(&VerifiedIdentity::user_for_test_with_zdr(
                "org-1", "user-1", true
            )),
            MemoryRetention::ZeroDataRetention
        );
        assert_eq!(
            MemoryRetention::of(&VerifiedIdentity::user_for_test("org-1", "user-1")),
            MemoryRetention::Durable
        );
        assert!(!MemoryRetention::ZeroDataRetention.permits_durable_memory());
        assert!(MemoryRetention::Durable.permits_durable_memory());
    }

    /// A ZDR turn is denied memory reads as well: a durable memory distils other,
    /// retained sessions, so injecting one would leak retained content into a
    /// no-retention context. The skip must surface as a degradation so it is not
    /// mistaken for "no memories found".
    #[tokio::test]
    async fn zdr_caller_reads_no_durable_letta_memory_and_the_skip_stays_observable() {
        let (auth, adapter) = letta_adapter_with_observable_auth().await;
        let thread_messages = vec![("user".to_owned(), "what did we agree on?".to_owned())];
        let mut rows = Vec::new();

        assert_eq!(
            append_letta_memory_rows(
                Some(&adapter),
                MemoryRetention::of(&VerifiedIdentity::user_for_test_with_zdr(
                    "org-1", "user-1", true,
                )),
                Some("org-1"),
                "thread-1",
                Some("user-1"),
                &thread_messages,
                &mut rows,
            )
            .await,
            Some(SemanticContextSearchStatus::Degraded(
                ZDR_MEMORY_READ_SUPPRESSED
            )),
            "a suppressed ZDR read must not be reported as an empty result"
        );
        assert!(
            rows.is_empty(),
            "retained memory must not be injected into a ZDR turn"
        );
        assert_eq!(
            auth_request_count(&auth).await,
            0,
            "a ZDR caller must not reach the Letta read surface at all"
        );

        let status = append_letta_memory_rows(
            Some(&adapter),
            MemoryRetention::of(&VerifiedIdentity::user_for_test("org-1", "user-1")),
            Some("org-1"),
            "thread-1",
            Some("user-1"),
            &thread_messages,
            &mut rows,
        )
        .await;
        assert!(
            matches!(status, Some(SemanticContextSearchStatus::Degraded(reason))
                if reason != ZDR_MEMORY_READ_SUPPRESSED),
            "a non-ZDR caller must reach the bridge; here it is merely unreachable, got {status:?}"
        );
        assert_eq!(
            auth_request_count(&auth).await,
            1,
            "a non-ZDR caller must still read durable memory"
        );
    }

    /// Mirrors model-gateway's `data_plane_grpc_authorization_uses_only_verified_bearer`.
    /// The Data Plane `authorization` header must be exactly the caller's verified
    /// delegated credential — never a shared key, a session-core service token, or
    /// a caller-supplied identity header. `DelegatedDataPlaneBearer` is only
    /// constructible through `JwtVerifier::delegated_data_plane_bearer`, so this
    /// also pins that the forwarding path takes no other input.
    #[test]
    fn dataplane_grpc_authorization_uses_only_verified_bearer() {
        let bearer = DelegatedDataPlaneBearer::for_test("signed-user-jwt");
        let request = authorize_dataplane((), &bearer)
            .expect("verified bearer should be valid gRPC metadata");

        assert_eq!(
            request
                .metadata()
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer signed-user-jwt")
        );
        // No shared-secret or forged-identity fallbacks may ride along.
        for forbidden in [
            "x-api-key",
            "x-internal-key",
            "x-user-id",
            "x-org-id",
            "x-verevon-org-id",
        ] {
            assert!(
                request.metadata().get(forbidden).is_none(),
                "{forbidden} must never be sent to Data Plane"
            );
        }
    }

    #[test]
    fn residency_defaults_to_eu_region() {
        // P0.4: the EU default is Sweden Central, used when MODEL_PLANE_RESIDENCY
        // is unset or blank; an explicit value overrides it. Tested via the pure
        // resolver so no env mutation (which needs `unsafe`, forbidden here).
        assert_eq!(DEFAULT_RESIDENCY, "swedencentral");
        assert_eq!(
            resolve_residency(None),
            "swedencentral",
            "unset → EU default"
        );
        assert_eq!(
            resolve_residency(Some("   ".to_owned())),
            "swedencentral",
            "blank → EU default"
        );
        assert_eq!(
            resolve_residency(Some("northeurope".to_owned())),
            "northeurope",
            "explicit override wins"
        );
    }

    #[test]
    fn thread_space_context_is_atomic_and_revisioned() {
        let empty = pb::CreateThreadRequest::default();
        assert!(
            !validate_thread_space_context_shape(&empty).expect("unscoped thread remains valid")
        );

        let partial = pb::CreateThreadRequest {
            space_id: "space-1".to_owned(),
            authority_revision: 4,
            ..Default::default()
        };
        assert_eq!(
            validate_thread_space_context_shape(&partial)
                .expect_err("partial Space context must be rejected")
                .code(),
            tonic::Code::InvalidArgument
        );

        let revisionless = pb::CreateThreadRequest {
            space_id: "space-1".to_owned(),
            space_decision_ref: "decision-1".to_owned(),
            recipient_audience_ref: "audience-1".to_owned(),
            recipient_audience_hash: "sha256:audience-1".to_owned(),
            privacy_policy_ref: "privacy-v1".to_owned(),
            resource_authorization_ref: "resource-auth-1".to_owned(),
            ..Default::default()
        };
        assert!(validate_thread_space_context_shape(&revisionless).is_err());

        let complete = pb::CreateThreadRequest {
            authority_revision: 4,
            recipient_audience_revision: 2,
            space_decision_token: "v2.token.payload.signature".to_owned(),
            action_schema_hash: "sha256:thread-create-v1".to_owned(),
            payload_digest: "sha256:payload-1".to_owned(),
            idempotency_key: "thread-create:1".to_owned(),
            ..revisionless
        };
        assert!(validate_thread_space_context_shape(&complete).expect("full shape is valid"));
    }

    #[test]
    fn thread_append_context_is_atomic_and_content_bound() {
        let empty = pb::AppendMessageRequest::default();
        assert!(
            !validate_append_space_context_shape(&empty).expect("unscoped append remains valid")
        );
        let partial = pb::AppendMessageRequest {
            space_id: "space-1".to_owned(),
            ..Default::default()
        };
        assert!(validate_append_space_context_shape(&partial).is_err());
        let complete = pb::AppendMessageRequest {
            thread_id: "thread-1".to_owned(),
            content: "exact message".to_owned(),
            space_id: "space-1".to_owned(),
            space_decision_ref: "decision-1".to_owned(),
            recipient_audience_ref: "audience-1".to_owned(),
            recipient_audience_revision: 3,
            recipient_audience_hash: "sha256:audience".to_owned(),
            privacy_policy_ref: "privacy-1".to_owned(),
            authority_revision: 7,
            resource_authorization_ref: "resource-1".to_owned(),
            space_decision_token: "v2.key.payload.signature".to_owned(),
            action_schema_hash: "sha256:thread-append-v1".to_owned(),
            payload_digest: "sha256:payload".to_owned(),
            idempotency_key: "idem-1".to_owned(),
            ..Default::default()
        };
        assert!(validate_append_space_context_shape(&complete).expect("complete append shape"));
        let digest = thread_append_payload_digest(&complete, "org-1", "user-1");
        let changed = pb::AppendMessageRequest {
            content: "other message".to_owned(),
            ..complete.clone()
        };
        assert_ne!(
            digest,
            thread_append_payload_digest(&changed, "org-1", "user-1")
        );
        assert_ne!(
            digest, complete.payload_digest,
            "caller digest is not trusted as input"
        );
    }

    #[test]
    fn signed_control_space_decision_must_bind_the_exact_thread_effect() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-08-13T12:00:00Z")
            .expect("test time")
            .with_timezone(&Utc);
        let signing_key = SigningKey::from_bytes(&[7_u8; 32]);
        let (request, key_id) = signed_personal_thread_request(&signing_key, now, true, true);
        assert_eq!(
            request.payload_digest,
            "sha256:3486ffcc43f1c7e83003a6236d533faacf9c0249933180d0c1157a4992db7245",
            "Control and Model must share one canonical effect encoding"
        );
        verify_thread_space_decision_with_key(&request, &key_id, &signing_key.verifying_key(), now)
            .expect("signed Control decision must verify");

        let mut another_effect = request;
        another_effect.session_key = "another-session".to_owned();
        assert_eq!(
            verify_thread_space_decision_with_key(
                &another_effect,
                &key_id,
                &signing_key.verifying_key(),
                now,
            )
            .expect_err("decision may not authorize another persisted effect")
            .code(),
            tonic::Code::PermissionDenied
        );
    }

    #[test]
    fn signed_append_decision_binds_exact_message_bytes_and_thread() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-08-13T12:00:00Z")
            .expect("test time")
            .with_timezone(&Utc);
        let signing_key = SigningKey::from_bytes(&[10_u8; 32]);
        let (request, key_id) = signed_thread_append_request(&signing_key, now);
        verify_append_space_decision_with_key(
            &request,
            "org-1",
            "user-1",
            &key_id,
            &signing_key.verifying_key(),
            now,
        )
        .expect("signed append decision must verify");
        let another_message = pb::AppendMessageRequest {
            content: "different bytes".to_owned(),
            ..request
        };
        assert_eq!(
            verify_append_space_decision_with_key(
                &another_message,
                "org-1",
                "user-1",
                &key_id,
                &signing_key.verifying_key(),
                now,
            )
            .expect_err("append bearer may not authorize different content")
            .code(),
            tonic::Code::PermissionDenied
        );
    }

    #[test]
    fn rotation_key_set_accepts_current_and_previous_key_but_rejects_unknown_key_id() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-08-13T12:00:00Z")
            .expect("test time")
            .with_timezone(&Utc);
        let current = SigningKey::from_bytes(&[17_u8; 32]);
        let previous = SigningKey::from_bytes(&[18_u8; 32]);
        let (current_request, _) = signed_personal_thread_request(&current, now, true, true);
        let (previous_request, _) = signed_personal_thread_request(&previous, now, true, true);
        // The helper uses the fixed test id, so give the previous signed token
        // a distinct envelope id and register that same verifying key.
        let previous_id = "control-previous-key".to_owned();
        let parts: Vec<_> = previous_request.space_decision_token.split('.').collect();
        let payload_part = parts[2];
        let key_part = URL_SAFE_NO_PAD.encode(previous_id.as_bytes());
        let signing_input = format!("{CONTROL_SPACE_DECISION_VERSION}.{key_part}.{payload_part}");
        let mut previous_request = previous_request;
        previous_request.space_decision_token = format!(
            "{signing_input}.{}",
            URL_SAFE_NO_PAD.encode(previous.sign(signing_input.as_bytes()).to_bytes())
        );
        let keys = BTreeMap::from([
            ("control-test-key".to_owned(), current.verifying_key()),
            (previous_id, previous.verifying_key()),
        ]);
        verify_thread_space_decision_with_keys(&current_request, &keys, now)
            .expect("current key must verify during rotation");
        verify_thread_space_decision_with_keys(&previous_request, &keys, now)
            .expect("previous in-set key must verify during rotation");

        let mut unknown = previous_request;
        let parts: Vec<_> = unknown.space_decision_token.split('.').collect();
        unknown.space_decision_token = format!(
            "{CONTROL_SPACE_DECISION_VERSION}.{}.{}.{}",
            URL_SAFE_NO_PAD.encode(b"unknown-key"),
            parts[2],
            parts[3]
        );
        assert_eq!(
            verify_thread_space_decision_with_keys(&unknown, &keys, now)
                .expect_err("unknown key id must fail closed")
                .code(),
            tonic::Code::PermissionDenied
        );
    }

    #[test]
    fn signed_control_space_decision_requires_complete_privacy_claims() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-08-13T12:00:00Z")
            .expect("test time")
            .with_timezone(&Utc);
        let signing_key = SigningKey::from_bytes(&[8_u8; 32]);
        let (request, key_id) = signed_personal_thread_request(&signing_key, now, false, true);
        assert_eq!(
            verify_thread_space_decision_with_key(
                &request,
                &key_id,
                &signing_key.verifying_key(),
                now,
            )
            .expect_err("policy-empty decision must not authorize persistence")
            .code(),
            tonic::Code::PermissionDenied
        );
    }

    #[test]
    fn signed_control_space_decision_requires_a_nonce() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-08-13T12:00:00Z")
            .expect("test time")
            .with_timezone(&Utc);
        let signing_key = SigningKey::from_bytes(&[9_u8; 32]);
        let (request, key_id) = signed_personal_thread_request(&signing_key, now, true, false);
        assert_eq!(
            verify_thread_space_decision_with_key(
                &request,
                &key_id,
                &signing_key.verifying_key(),
                now,
            )
            .expect_err("nonce-less decision must not authorize persistence")
            .code(),
            tonic::Code::PermissionDenied
        );
    }

    fn signed_personal_thread_request(
        signing_key: &SigningKey,
        now: DateTime<Utc>,
        complete_privacy: bool,
        include_nonce: bool,
    ) -> (pb::CreateThreadRequest, String) {
        let key_id = "control-test-key".to_owned();
        let key_part = URL_SAFE_NO_PAD.encode(key_id.as_bytes());
        let request = pb::CreateThreadRequest {
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
            session_key: "session-1".to_owned(),
            space_id: "space-personal".to_owned(),
            space_decision_ref: "decision-1".to_owned(),
            recipient_audience_ref: "audience:space-personal:2".to_owned(),
            recipient_audience_revision: 2,
            recipient_audience_hash: "sha256:test-audience".to_owned(),
            privacy_policy_ref: "privacy:org-1:5".to_owned(),
            resource_authorization_ref: "control:space-personal:thread-create:7".to_owned(),
            authority_revision: 7,
            action_schema_hash: "sha256:thread-create-v1".to_owned(),
            payload_digest: String::new(),
            idempotency_key: "thread-create:1".to_owned(),
            ..Default::default()
        };
        let mut request = request;
        request.payload_digest = thread_create_payload_digest(&request);
        let mut payload = serde_json::json!({
            "decision_ref": request.space_decision_ref,
            "org_id": request.org_id,
            "space_ref": request.space_id,
            "subject_id": request.user_id,
            "service_audience": CONTROL_SPACE_DECISION_AUDIENCE,
            "action_id": CONTROL_THREAD_CREATE_ACTION,
            "action_schema_hash": request.action_schema_hash,
            "payload_digest": request.payload_digest,
            "idempotency_key": request.idempotency_key,
            "recipient_audience_ref": request.recipient_audience_ref,
            "recipient_audience_hash": request.recipient_audience_hash,
            "privacy_policy_ref": request.privacy_policy_ref,
            "resource_authorization_ref": request.resource_authorization_ref,
            "purpose": "assistant_collaboration",
            "lawful_basis": "contract",
            "privacy_class": "internal",
            "third_party_processing_allowed": false,
            "retention_class": "standard",
            "residency": "swedencentral",
            "deletion_scope": "space",
            "zero_data_retention": false,
            "nonce": "nonce-1",
            "authority_revision": request.authority_revision,
            "membership_revision": 4,
            "privacy_revision": 5,
            "recipient_audience_revision": request.recipient_audience_revision,
            "entitlement_revision": 3,
            "permissions": ["thread:create"],
            "issued_at": now.to_rfc3339(),
            "expires_at": (now + chrono::Duration::minutes(1)).to_rfc3339(),
        });
        if !complete_privacy {
            payload
                .as_object_mut()
                .expect("test claim object")
                .remove("purpose");
        }
        if !include_nonce {
            payload
                .as_object_mut()
                .expect("test claim object")
                .remove("nonce");
        }
        let payload_part =
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).expect("test claims"));
        let signing_input = format!("{CONTROL_SPACE_DECISION_VERSION}.{key_part}.{payload_part}");
        let signature = signing_key.sign(signing_input.as_bytes());
        let mut request = request;
        request.space_decision_token = format!(
            "{signing_input}.{}",
            URL_SAFE_NO_PAD.encode(signature.to_bytes())
        );
        (request, key_id)
    }

    fn signed_thread_append_request(
        signing_key: &SigningKey,
        now: DateTime<Utc>,
    ) -> (pb::AppendMessageRequest, String) {
        let key_id = "control-test-key".to_owned();
        let key_part = URL_SAFE_NO_PAD.encode(key_id.as_bytes());
        let mut request = pb::AppendMessageRequest {
            thread_id: "thread-1".to_owned(),
            role: "user".to_owned(),
            content: "exact message".to_owned(),
            space_id: "space-personal".to_owned(),
            space_decision_ref: "append-decision-1".to_owned(),
            recipient_audience_ref: "audience:space-personal:2".to_owned(),
            recipient_audience_revision: 2,
            recipient_audience_hash: "sha256:test-audience".to_owned(),
            privacy_policy_ref: "privacy:org-1:5".to_owned(),
            authority_revision: 7,
            resource_authorization_ref: "control:space-personal:thread-append:7".to_owned(),
            action_schema_hash: "sha256:thread-append-v1".to_owned(),
            idempotency_key: "thread-append:1".to_owned(),
            ..Default::default()
        };
        request.payload_digest = thread_append_payload_digest(&request, "org-1", "user-1");
        let payload = serde_json::json!({
            "decision_ref": request.space_decision_ref, "org_id": "org-1", "space_ref": request.space_id,
            "subject_id": "user-1", "service_audience": CONTROL_SPACE_DECISION_AUDIENCE,
            "action_id": CONTROL_THREAD_APPEND_ACTION, "action_schema_hash": request.action_schema_hash,
            "payload_digest": request.payload_digest, "idempotency_key": request.idempotency_key,
            "recipient_audience_ref": request.recipient_audience_ref, "recipient_audience_hash": request.recipient_audience_hash,
            "privacy_policy_ref": request.privacy_policy_ref, "resource_authorization_ref": request.resource_authorization_ref,
            "purpose": "assistant_collaboration", "lawful_basis": "contract", "privacy_class": "internal",
            "third_party_processing_allowed": false, "retention_class": "standard", "residency": "swedencentral",
            "deletion_scope": "space", "zero_data_retention": false, "nonce": "nonce-1",
            "authority_revision": request.authority_revision, "membership_revision": 4, "privacy_revision": 5,
            "recipient_audience_revision": request.recipient_audience_revision, "entitlement_revision": 3,
            "permissions": ["thread:append"], "issued_at": now.to_rfc3339(),
            "expires_at": (now + chrono::Duration::minutes(1)).to_rfc3339(),
        });
        let payload_part =
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).expect("test claims"));
        let signing_input = format!("{CONTROL_SPACE_DECISION_VERSION}.{key_part}.{payload_part}");
        request.space_decision_token = format!(
            "{signing_input}.{}",
            URL_SAFE_NO_PAD.encode(signing_key.sign(signing_input.as_bytes()).to_bytes())
        );
        (request, key_id)
    }

    #[test]
    fn standard_health_registers_every_business_service_name() {
        assert_eq!(
            HEALTH_SERVICE_NAMES,
            [
                "model_plane.v1.SessionCore",
                "model_plane.v1.ManagedRunLifecycle",
                "model_plane.v1.OrchestrationCoreService",
                "model_plane.v1.FinetuneJobs",
                "model_plane.v1.MemoryService",
                "model_plane.v1.RoutingPolicy",
                "model_plane.v1.RunService",
            ]
        );
    }

    #[test]
    fn semantic_context_search_distinguishes_empty_from_rpc_and_timeout_degradation() {
        assert_eq!(
            semantic_context_search_status(0, None),
            SemanticContextSearchStatus::Empty,
            "an empty verified semantic result is not a dependency failure"
        );
        assert_eq!(
            semantic_context_search_status(0, Some("DEGRADED_LETTA_UNAVAILABLE")),
            SemanticContextSearchStatus::Degraded("DEGRADED_LETTA_UNAVAILABLE"),
            "an RPC failure must remain observable on the context-assembly path"
        );
        assert_eq!(
            semantic_context_search_status(0, Some("DEGRADED_LETTA_TIMEOUT")),
            SemanticContextSearchStatus::Degraded("DEGRADED_LETTA_TIMEOUT"),
            "a timeout must not be collapsed into an empty result"
        );
    }

    #[test]
    fn user_checkpoints_cannot_preclaim_compaction_or_exhaust_storage() {
        let valid_state = vec![0_u8; MAX_USER_CHECKPOINT_STATE_BYTES];
        validate_user_checkpoint("checkpoint-1", &valid_state).expect("bounded user checkpoint");

        for (id, state) in [
            ("", &b"state"[..]),
            (" auto-compact-v1:forged", &b"state"[..]),
            ("auto-compact-v1:forged", &b"state"[..]),
            (&"x".repeat(MAX_USER_CHECKPOINT_ID_BYTES + 1), &b"state"[..]),
            ("checkpoint-1", &[][..]),
            (
                "checkpoint-1",
                &vec![0_u8; MAX_USER_CHECKPOINT_STATE_BYTES + 1],
            ),
        ] {
            assert!(
                validate_user_checkpoint(id, state).is_err(),
                "invalid checkpoint id/state was accepted"
            );
        }
    }

    fn base() -> AssemblyInputs {
        AssemblyInputs {
            policy_id: String::new(),
            workspace_id: String::new(),
            agent_id: None,
            user_id: None,
            prompt_goal: None,
            thread_messages: vec![],
            policy_segments: vec![],
            workspace_segments: vec![],
            agent_segments: vec![],
            user_segments: vec![],
            episodic_segments: vec![],
            skill_index_segments: vec![],
            skill_expansion_segments: vec![],
            retrieval_segments: vec![],
            knowledge_segments: vec![],
            graph_segments: vec![],
            run_id: String::new(),
            max_tokens: 0,
        }
    }

    /// Build a thread whose rendered messages alone would blow the budget.
    fn flooded_thread(turns: usize, chars: usize) -> Vec<(String, String)> {
        (0..turns)
            .map(|n| ("user".to_owned(), format!("{n}-{}", "x".repeat(chars))))
            .collect()
    }

    #[test]
    fn long_thread_cannot_starve_grounding_or_the_runs_own_goal() {
        // The regression this guards: history was spent first and `return`ed out
        // of assembly, so a rich ERP thread silently produced an answer with no
        // retrieval, no knowledge, no graph, and not even the question.
        let mut i = base();
        i.max_tokens = 512;
        i.thread_messages = flooded_thread(40, 400);
        i.retrieval_segments = vec!["retrieved: supplier invoice 4711".to_owned()];
        i.knowledge_segments = vec!["kb: stocktake policy".to_owned()];
        i.graph_segments = vec!["graph: supplier→sku".to_owned()];
        i.prompt_goal = Some("what is empty in the warehouse?".to_owned());

        let (segs, total) = assemble_segments(&i);
        let kinds: Vec<&str> = segs.iter().map(|s| s.kind.as_str()).collect();

        assert!(
            kinds.contains(&"retrieval"),
            "grounding was starved: {kinds:?}"
        );
        assert!(
            kinds.contains(&"knowledge"),
            "knowledge was starved: {kinds:?}"
        );
        assert!(kinds.contains(&"graph"), "graph was starved: {kinds:?}");
        assert!(
            kinds.contains(&"prompt"),
            "the run's own goal was dropped: {kinds:?}"
        );
        assert!(
            kinds.contains(&"thread"),
            "history should still get its share"
        );
        assert!(total <= i.max_tokens, "budget overspent: {total}");
    }

    #[test]
    fn thread_truncation_keeps_the_newest_turns() {
        // Dropping the tail discarded the turns the user is actually replying to.
        let mut i = base();
        i.max_tokens = 256;
        i.thread_messages = flooded_thread(30, 200);

        let (segs, _) = assemble_segments(&i);
        let threads: Vec<&str> = segs
            .iter()
            .filter(|s| s.kind == "thread")
            .map(|s| s.content.as_str())
            .collect();

        assert!(!threads.is_empty(), "history was dropped entirely");
        // Newest turn is index 29; the oldest (0) must be what falls off.
        assert!(
            threads.last().is_some_and(|last| last.contains("29-")),
            "newest turn missing; kept={:?}",
            threads
                .iter()
                .map(|t| &t[..8.min(t.len())])
                .collect::<Vec<_>>()
        );
        assert!(
            !threads.iter().any(|t| t.starts_with("user: 0-")),
            "oldest turn survived while newer ones were dropped"
        );
    }

    #[test]
    fn thread_segments_stay_in_chronological_order_after_truncation() {
        let mut i = base();
        i.max_tokens = 512;
        i.thread_messages = flooded_thread(20, 100);

        let (segs, _) = assemble_segments(&i);
        let indices: Vec<usize> = segs
            .iter()
            .filter(|s| s.kind == "thread")
            .filter_map(|s| {
                s.content
                    .trim_start_matches("user: ")
                    .split('-')
                    .next()?
                    .parse()
                    .ok()
            })
            .collect();

        assert!(indices.len() > 1, "need several turns to check ordering");
        assert!(
            indices.windows(2).all(|w| w[0] < w[1]),
            "history must read oldest→newest, got {indices:?}"
        );
    }

    #[test]
    fn grounding_reservation_still_leaves_history_room() {
        // The reservation is capped, so grounding cannot starve history either.
        let mut i = base();
        i.max_tokens = 1024;
        i.thread_messages = flooded_thread(10, 200);
        i.retrieval_segments = (0..40)
            .map(|n| format!("retrieved-{n}-{}", "y".repeat(200)))
            .collect();

        let (segs, _) = assemble_segments(&i);
        let kinds: Vec<&str> = segs.iter().map(|s| s.kind.as_str()).collect();
        assert!(
            kinds.contains(&"thread"),
            "oversized grounding starved history: {kinds:?}"
        );
    }

    #[test]
    fn ordering_canonical() {
        let mut i = base();
        i.policy_id = "p1".to_owned();
        i.workspace_id = "w1".to_owned();
        i.agent_id = Some("a1".to_owned());
        i.user_id = Some("u1".to_owned());
        i.thread_messages = vec![("user".to_owned(), "hi".to_owned())];
        i.episodic_segments = vec!["memo".to_owned()];
        i.prompt_goal = Some("goal".to_owned());
        let (segs, _) = assemble_segments(&i);
        let kinds: Vec<&str> = segs.iter().map(|s| s.kind.as_str()).collect();
        assert_eq!(
            kinds,
            vec![
                "policy",
                "workspace",
                "agent",
                "user",
                "thread",
                "episodic",
                "prompt"
            ]
        );
    }

    #[test]
    fn budget_truncates() {
        let mut i = base();
        i.max_tokens = 3;
        i.policy_id = "short".to_owned();
        i.workspace_id = "this-is-a-very-long-workspace-id-that-exceeds-budget".to_owned();
        i.prompt_goal = Some("goal".to_owned());
        let (segs, total) = assemble_segments(&i);
        assert!(total <= 3);
        assert!(segs.iter().all(|s| s.kind != "workspace"));
    }

    #[test]
    fn empty_ids_skip() {
        let (segs, _) = assemble_segments(&base());
        let kinds: Vec<&str> = segs.iter().map(|s| s.kind.as_str()).collect();
        assert_eq!(kinds, vec!["prompt"]);
    }

    #[test]
    fn prompt_fallback() {
        let mut i = base();
        i.run_id = "r1".to_owned();
        let (segs, _) = assemble_segments(&i);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].kind, "prompt");
        assert_eq!(segs[0].content, "run:r1");
    }

    #[test]
    fn materialized_context_sources_take_priority_over_identifier_fallbacks() {
        let mut i = base();
        i.policy_id = "policy-id".to_owned();
        i.workspace_id = "workspace-id".to_owned();
        i.agent_id = Some("agent-id".to_owned());
        i.user_id = Some("user-id".to_owned());
        i.policy_segments = vec!["policy body".to_owned()];
        i.workspace_segments = vec!["workspace body".to_owned()];
        i.agent_segments = vec!["agent body".to_owned()];
        i.user_segments = vec!["user body".to_owned()];
        i.episodic_segments = vec!["memory body".to_owned()];
        i.skill_index_segments = vec!["skill index body".to_owned()];
        i.skill_expansion_segments = vec!["skill expansion body".to_owned()];
        i.retrieval_segments = vec!["retrieval body".to_owned()];
        i.prompt_goal = Some("prompt body".to_owned());

        let (segs, _) = assemble_segments(&i);
        let contents: Vec<(&str, &str)> = segs
            .iter()
            .map(|seg| (seg.kind.as_str(), seg.content.as_str()))
            .collect();

        assert_eq!(
            contents,
            vec![
                ("policy", "policy body"),
                ("workspace", "workspace body"),
                ("agent", "agent body"),
                ("user", "user body"),
                ("episodic", "memory body"),
                ("skill_index", "skill index body"),
                ("skill_expansion", "skill expansion body"),
                ("retrieval", "retrieval body"),
                ("prompt", "prompt body"),
            ]
        );
    }

    #[test]
    fn full_context_order_includes_all_plan_segments() {
        let mut i = base();
        i.policy_segments = vec!["policy".to_owned()];
        i.workspace_segments = vec!["workspace".to_owned()];
        i.agent_segments = vec!["agent".to_owned()];
        i.user_segments = vec!["user".to_owned()];
        i.thread_messages = vec![("user".to_owned(), "thread".to_owned())];
        i.episodic_segments = vec!["episodic".to_owned()];
        i.skill_index_segments = vec!["skill-index".to_owned()];
        i.skill_expansion_segments = vec!["skill-expansion".to_owned()];
        i.retrieval_segments = vec!["retrieval".to_owned()];
        i.knowledge_segments = vec!["knowledge".to_owned()];
        i.graph_segments = vec!["graph".to_owned()];
        i.prompt_goal = Some("prompt".to_owned());

        let (segs, _) = assemble_segments(&i);
        let kinds: Vec<&str> = segs.iter().map(|s| s.kind.as_str()).collect();
        assert_eq!(
            kinds,
            vec![
                "policy",
                "workspace",
                "agent",
                "user",
                "thread",
                "episodic",
                "skill_index",
                "skill_expansion",
                "retrieval",
                "knowledge",
                "graph",
                "prompt",
            ]
        );
    }

    #[test]
    fn knowledge_segments_appear_after_retrieval() {
        // Regression for the context-retention pass: knowledge units fetched
        // from `KnowledgeService.GetKnowledgeUnits` must render between the
        // retrieval evidence (raw candidates) and the graph segment
        // (contradictions), giving the model: evidence → adjacent
        // chunks for that evidence → graph-level constraints.
        let mut i = base();
        i.retrieval_segments = vec!["doc1 hit".to_owned()];
        i.knowledge_segments = vec!["[doc:a chunk:0] surrounding".to_owned()];
        i.graph_segments = vec!["contradiction (active): X".to_owned()];

        let (segs, _) = assemble_segments(&i);
        let kinds: Vec<&str> = segs.iter().map(|s| s.kind.as_str()).collect();
        let pos = |k| kinds.iter().position(|x| *x == k).unwrap();
        assert!(pos("retrieval") < pos("knowledge"));
        assert!(pos("knowledge") < pos("graph"));
    }

    #[test]
    fn empty_knowledge_segments_skip_cleanly() {
        // No knowledge_client configured (or no doc_ids in retrieval) →
        // empty vec. Assembly must not insert a placeholder "knowledge"
        // segment in that case.
        let mut i = base();
        i.retrieval_segments = vec!["evidence".to_owned()];
        i.knowledge_segments = vec![];

        let (segs, _) = assemble_segments(&i);
        let kinds: Vec<&str> = segs.iter().map(|s| s.kind.as_str()).collect();
        assert!(!kinds.contains(&"knowledge"));
        assert!(kinds.contains(&"retrieval"));
    }

    // ---------------------------------------------------------------------
    // PR-8: Context assembly SLO — latency + token-budget compliance.
    // ---------------------------------------------------------------------
    //
    // Wires `mp-slo::harness::context_assembly` against the real
    // `assemble_segments` function with a realistic fixture (hundreds of
    // memory segments + long thread history).
    //
    // Two invariants enforced:
    //   1. Latency p95 < `context_assembly_latency` SLO target (default 2s).
    //   2. Every assembly result's token total ≤ requested `max_tokens`
    //      (operator-visible budget compliance).

    fn large_fixture() -> AssemblyInputs {
        let mut i = base();
        i.policy_id = "policy-slo".to_owned();
        i.workspace_id = "workspace-slo".to_owned();
        i.agent_id = Some("agent-slo".to_owned());
        i.user_id = Some("user-slo".to_owned());
        i.run_id = "run-slo".to_owned();
        i.max_tokens = 8_000;
        i.prompt_goal = Some("synthesize recent work and propose next steps".repeat(4));

        // 32 turns of thread history, each ~300 chars.
        i.thread_messages = (0..32)
            .map(|n| {
                let role = if n % 2 == 0 { "user" } else { "assistant" };
                (
                    role.to_owned(),
                    format!("[turn {n}] {}", "lorem ipsum ".repeat(25)),
                )
            })
            .collect();

        // Dense memory sprinkled across kinds.
        let seg = |prefix: &str, n: usize| -> Vec<String> {
            (0..n)
                .map(|i| format!("{prefix}-{i:03}: {}", "data ".repeat(20)))
                .collect()
        };
        i.policy_segments = seg("policy", 8);
        i.workspace_segments = seg("workspace", 16);
        i.agent_segments = seg("agent", 16);
        i.user_segments = seg("user", 16);
        i.episodic_segments = seg("episodic", 32);
        i.skill_index_segments = seg("skill-idx", 16);
        i.skill_expansion_segments = seg("skill-exp", 16);
        i.retrieval_segments = seg("retrieval", 32);
        i
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn context_assembly_meets_slo() {
        use mp_slo::{defaults, harness};
        type BoxedErr = Box<dyn std::error::Error + Send + Sync>;

        let iterations = 50usize;
        let fixture = large_fixture();
        let budget = fixture.max_tokens;

        let (recorder, tokens) = harness::context_assembly(iterations, |_| {
            let inputs = large_fixture();
            async move {
                let (_segs, total) = assemble_segments(&inputs);
                Ok::<usize, BoxedErr>(usize::try_from(total).unwrap_or(0))
            }
        })
        .await
        .expect("context_assembly harness ran");

        // Latency SLO.
        let sorted = recorder.sorted_durations();
        let verdict = defaults::context_assembly_latency().evaluate(&sorted);
        assert!(!verdict.breached, "{}", verdict.summary());
        assert_eq!(recorder.samples().len(), iterations);

        // Token-budget compliance. Every invocation's observed token count
        // must be ≤ requested max_tokens.
        for (idx, &t) in tokens.iter().enumerate() {
            assert!(
                u32::try_from(t).unwrap_or(u32::MAX) <= budget,
                "iteration {idx}: estimated_tokens {t} exceeds budget {budget}"
            );
        }

        // Sanity: the assembler actually produced something under this budget.
        let max_observed = tokens.iter().copied().max().unwrap_or(0);
        assert!(
            max_observed > 0,
            "expected non-empty assembly; fixture may be wrong"
        );
    }

    // P3 SetRunMode — durable run.mode, invalid-mode rejection, and per-org
    // isolation (a foreign org's UPDATE matches no row → not_found, no mutation).
    async fn assert_set_run_mode_isolation(
        svc: &super::SessionService,
        pool: &sqlx::PgPool,
        run_id: &str,
        org: &str,
        sfx: u32,
    ) {
        use mp_contracts::model_plane::v1 as pb;
        use mp_contracts::model_plane::v1::session_core_server::SessionCore;
        use tonic::Request;

        let resp = svc
            .set_run_mode(Request::new(pb::SetRunModeRequest {
                run_id: run_id.to_owned(),
                mode: "plan".into(),
                org_id: org.to_owned(),
                granted_rung: 0,
                justification: String::new(),
            }))
            .await
            .expect("set_run_mode")
            .into_inner();
        assert_eq!(resp.mode, "plan");
        assert!(svc
            .set_run_mode(Request::new(pb::SetRunModeRequest {
                run_id: run_id.to_owned(),
                mode: "bogus".into(),
                org_id: org.to_owned(),
                granted_rung: 0,
                justification: String::new(),
            }))
            .await
            .is_err());

        let cross = svc
            .set_run_mode(Request::new(pb::SetRunModeRequest {
                run_id: run_id.to_owned(),
                mode: "execute".into(),
                org_id: format!("attacker-{sfx}"),
                granted_rung: 0,
                justification: String::new(),
            }))
            .await;
        assert_eq!(
            cross.unwrap_err().code(),
            tonic::Code::NotFound,
            "cross-org set_run_mode must be rejected (per-org isolation)"
        );
        let persisted: (String,) = sqlx::query_as("SELECT mode FROM runs WHERE id = $1")
            .bind(run_id)
            .fetch_one(pool)
            .await
            .expect("read mode");
        assert_eq!(
            persisted.0, "plan",
            "cross-org attempt must not mutate the run"
        );
    }

    // G7 — fresh background_review insert succeeds; a background_review overwrite
    // of an existing user skill must be skipped (protected).
    async fn assert_upsert_skill_guards(svc: &super::SessionService, org: &str) {
        use mp_contracts::model_plane::v1 as pb;
        use mp_contracts::model_plane::v1::session_core_server::SessionCore;
        use tonic::Request;

        let skill = |name: &str, content: &str, origin: &str| pb::UpsertAgentSkillRequest {
            org_id: org.to_owned(),
            name: name.into(),
            description: "d".into(),
            content: content.into(),
            trigger_keywords: vec![],
            trigger_file_patterns: vec![],
            tool_restrictions: vec![],
            enabled: true,
            origin: origin.into(),
        };

        let r = svc
            .upsert_agent_skill(Request::new(skill("Cache", "c1", "background_review")))
            .await
            .expect("upsert")
            .into_inner();
        assert!(r.created && !r.skipped_protected);

        svc.upsert_agent_skill(Request::new(skill("Runbook", "human", "user")))
            .await
            .expect("user skill");
        let blocked = svc
            .upsert_agent_skill(Request::new(skill(
                "Runbook",
                "MACHINE",
                "background_review",
            )))
            .await
            .expect("guarded upsert")
            .into_inner();
        assert!(
            blocked.skipped_protected,
            "background_review must not overwrite a user skill"
        );
    }

    // Handler-level integration test against a REAL Postgres. Exercises the
    // actual gRPC handler code (validation + SQL + mapping), not just raw SQL.
    // #[ignore]d so plain `cargo test` (no DB) skips it; run explicitly with a
    // DB:  DATABASE_URL=… cargo test -p session-core --bin session-core
    //        set_run_mode_and_upsert_skill_against_real_pg -- --ignored
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a Postgres with session-core migrations"]
    // end-to-end PG integration test: sequential setup → exercise → assertions
    #[allow(clippy::too_many_lines)]
    async fn set_run_mode_and_upsert_skill_against_real_pg() {
        use super::SessionService;

        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL unset");
            return;
        };
        let pool = sqlx::PgPool::connect(&url).await.expect("connect pg");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");

        let sfx = std::process::id();
        let (thread_id, run_id, org) =
            (format!("t-{sfx}"), format!("r-{sfx}"), format!("org-{sfx}"));
        sqlx::query("INSERT INTO threads (id,session_key,org_id,user_id) VALUES ($1,$1,$2,'u1')")
            .bind(&thread_id)
            .bind(&org)
            .execute(&pool)
            .await
            .expect("seed thread");
        sqlx::query(
            "INSERT INTO runs (id,thread_id,goal,org_id,user_id) VALUES ($1,$2,'g',$3,'u1')",
        )
        .bind(&run_id)
        .bind(&thread_id)
        .bind(&org)
        .execute(&pool)
        .await
        .expect("seed run");

        let svc = SessionService {
            pool: pool.clone(),
            retrieval_client: None,
            graph_client: None,
            knowledge_client: None,
            letta_memory: None,
            audit_publisher: None,
            auth: None,
        };

        assert_set_run_mode_isolation(&svc, &pool, &run_id, &org, sfx).await;
        assert_upsert_skill_guards(&svc, &org).await;

        // G7 read path: list_agent_skills returns this org's skills (Cache +
        // Runbook), proving the learned-skill last mile is readable.
        let listed = svc
            .list_agent_skills(Request::new(pb::ListAgentSkillsRequest {
                org_id: org.clone(),
                enabled_only: true,
            }))
            .await
            .expect("list_agent_skills")
            .into_inner();
        let names: std::collections::BTreeSet<&str> =
            listed.skills.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains("Cache") && names.contains("Runbook"),
            "list must return the upserted skills, got {names:?}"
        );
        // Per-org isolation: another org sees none of them.
        let other = svc
            .list_agent_skills(Request::new(pb::ListAgentSkillsRequest {
                org_id: format!("other-{sfx}"),
                enabled_only: true,
            }))
            .await
            .expect("list other org")
            .into_inner();
        assert!(
            other.skills.is_empty(),
            "another org must not see this org's skills"
        );

        // G7 transcript source: list_thread_messages returns the conversation
        // in sequence order, org-scoped via the owning thread.
        for (i, (role, content)) in [("user", "hello"), ("assistant", "hi there")]
            .iter()
            .enumerate()
        {
            sqlx::query("INSERT INTO messages (id, thread_id, role, content) VALUES ($1,$2,$3,$4)")
                .bind(format!("m-{sfx}-{i}"))
                .bind(&thread_id)
                .bind(*role)
                .bind(*content)
                .execute(&pool)
                .await
                .expect("seed message");
        }
        let convo = svc
            .list_conversation(Request::new(pb::ListConversationRequest {
                org_id: org.clone(),
                thread_id: thread_id.clone(),
            }))
            .await
            .expect("list_thread_messages")
            .into_inner();
        assert_eq!(convo.messages.len(), 2, "expected 2 messages");
        assert_eq!(convo.messages[0].role, "user");
        assert_eq!(convo.messages[0].content, "hello");
        assert_eq!(convo.messages[1].role, "assistant", "ordered by sequence");
        // Per-org isolation: another org cannot read this thread's conversation.
        let cross_convo = svc
            .list_conversation(Request::new(pb::ListConversationRequest {
                org_id: format!("attacker-{sfx}"),
                thread_id: thread_id.clone(),
            }))
            .await
            .expect("list other-org convo")
            .into_inner();
        assert!(
            cross_convo.messages.is_empty(),
            "another org must not read this thread's messages"
        );

        // cleanup (messages first — FK to threads)
        sqlx::query("DELETE FROM messages WHERE thread_id=$1")
            .bind(&thread_id)
            .execute(&pool)
            .await
            .ok();
        for q in [
            "DELETE FROM agent_skills WHERE org_id=$1",
            "DELETE FROM approvals WHERE org_id=$1",
            "DELETE FROM runs WHERE org_id=$1",
            "DELETE FROM threads WHERE org_id=$1",
        ] {
            sqlx::query(q).bind(&org).execute(&pool).await.ok();
        }
    }

    /// Replay must include the thread-owned events emitted before a run exists.
    /// Those rows use `events.run_id = threads.id` for compatibility with the
    /// original non-null schema; a run-only join silently dropped them on
    /// reconnect. This integration test exercises the actual replay SQL against
    /// Postgres rather than asserting on the query text.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn replay_thread_includes_thread_owned_events_against_real_pg() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL unset");
            return;
        };
        let pool = sqlx::PgPool::connect(&url).await.expect("connect pg");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");

        let suffix = mp_ids::new_ulid();
        let thread_id = format!("thread-replay-{suffix}");
        let org_id = format!("org-replay-{suffix}");
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, $3)",
        )
        .bind(&thread_id)
        .bind(&org_id)
        .bind("user-replay")
        .execute(&pool)
        .await
        .expect("seed thread");

        // This is the shape produced by append_message_inner before a run is
        // created: run_id carries the thread id, not a run id.
        sqlx::query(
            "INSERT INTO events (id, event_type, run_id, payload, org_id, user_id, type_url, producer, schema_version)
             VALUES ($1, 'MESSAGE_APPENDED', $2, '{}', $3, 'user-replay', $4, 'session-core', 1)",
        )
        .bind(format!("event-replay-{suffix}"))
        .bind(&thread_id)
        .bind(&org_id)
        .bind(MESSAGE_APPENDED_TYPE_URL)
        .execute(&pool)
        .await
        .expect("seed thread event");

        let (tx, mut rx) = tokio::sync::mpsc::channel(4);
        super::replay_thread_task(
            pool.clone(),
            pb::ReplayThreadRequest {
                thread_id: thread_id.clone(),
                after_event_id: String::new(),
                limit: 0,
            },
            tx,
        )
        .await;
        let event_types: Vec<i32> = std::iter::from_fn(|| rx.try_recv().ok())
            .map(|event| event.expect("replay event"))
            .map(|event| event.event_type)
            .collect();
        assert_eq!(event_types, vec![121], "thread event must be replayed");

        sqlx::query("DELETE FROM events WHERE run_id = $1")
            .bind(&thread_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM threads WHERE id = $1")
            .bind(&thread_id)
            .execute(&pool)
            .await
            .ok();
    }

    /// Two concurrent deliveries of the same create effect must converge on
    /// one owner-bound thread. The advisory transaction lock in
    /// `create_thread_inner` must cover the check-and-insert window; a unique
    /// index alone would only turn the losing delivery into an error.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn concurrent_thread_creates_with_one_session_key_converge_in_real_pg() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL unset");
            return;
        };
        let pool = sqlx::PgPool::connect(&url).await.expect("connect pg");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");

        let suffix = mp_ids::new_ulid();
        let org_id = format!("org-create-race-{suffix}");
        let user_id = format!("user-create-race-{suffix}");
        let session_key = format!("session-create-race-{suffix}");
        let request = pb::CreateThreadRequest {
            session_key: session_key.clone(),
            org_id: org_id.clone(),
            user_id: user_id.clone(),
            ..Default::default()
        };

        let first_pool = pool.clone();
        let second_pool = pool.clone();
        let (first, second) = tokio::join!(
            super::create_thread_inner(&first_pool, request.clone()),
            super::create_thread_inner(&second_pool, request),
        );
        let first_id = first.expect("first create succeeds").into_inner().thread_id;
        let second_id = second
            .expect("second create converges rather than failing")
            .into_inner()
            .thread_id;
        assert_eq!(first_id, second_id, "same effect must return one thread");

        let (count,): (i64,) = sqlx::query_as(
            "SELECT count(*) FROM threads WHERE org_id = $1 AND user_id = $2 AND session_key = $3",
        )
        .bind(&org_id)
        .bind(&user_id)
        .bind(&session_key)
        .fetch_one(&pool)
        .await
        .expect("count converged thread");
        assert_eq!(count, 1, "concurrent retries must mint only one thread");

        sqlx::query("DELETE FROM events WHERE run_id = $1")
            .bind(&first_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM threads WHERE id = $1")
            .bind(&first_id)
            .execute(&pool)
            .await
            .ok();
    }

    /// Managed runs are the normal gateway execution path. They must inherit
    /// the verified thread context just like the legacy StartRun handler, or a
    /// scoped turn could execute with a run/event that appears unscoped.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn managed_run_inherits_thread_space_context_in_real_pg() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL unset");
            return;
        };
        let pool = sqlx::PgPool::connect(&url).await.expect("connect pg");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");

        let suffix = mp_ids::new_ulid();
        let org_id = format!("org-managed-space-{suffix}");
        let user_id = format!("user-managed-space-{suffix}");
        let thread_id = format!("thread-managed-space-{suffix}");
        let space_id = format!("space-managed-{suffix}");
        let decision_ref = format!("decision-managed-{suffix}");
        let audience_ref = format!("audience-managed-{suffix}");
        let audience_hash = format!("sha256:audience-managed-{suffix}");
        let privacy_ref = format!("privacy-managed-{suffix}");
        let resource_ref = format!("resource-managed-{suffix}");
        sqlx::query(
            "INSERT INTO threads
             (id, session_key, org_id, user_id, space_id, space_decision_ref,
              recipient_audience_ref, recipient_audience_revision, recipient_audience_hash, privacy_policy_ref, resource_authorization_ref,
              authority_revision)
             VALUES ($1, $1, $2, $3, $4, $5, $6, 4, $7, $8, $9, 9)",
        )
        .bind(&thread_id)
        .bind(&org_id)
        .bind(&user_id)
        .bind(&space_id)
        .bind(&decision_ref)
        .bind(&audience_ref)
        .bind(&audience_hash)
        .bind(&privacy_ref)
        .bind(&resource_ref)
        .execute(&pool)
        .await
        .expect("seed scoped thread");

        let managed = crate::terminalization::start_managed_run_inner(
            &pool,
            pb::StartManagedRunRequest {
                thread_id: thread_id.clone(),
                parent_run_id: String::new(),
                agent_id: "managed-space-agent".to_owned(),
                goal: "scoped goal".to_owned(),
                mode: "execute".to_owned(),
                org_id: org_id.clone(),
                user_id: user_id.clone(),
                start_key: format!("managed-space-start-{suffix}"),
                terminal_source: pb::ManagedRunSource::GatewayDirect as i32,
            },
            false,
        )
        .await
        .expect("start scoped managed run");

        let persisted: (
            Option<String>,
            Option<String>,
            Option<String>,
            Option<i64>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<i64>,
        ) = sqlx::query_as(
            "SELECT space_id, space_decision_ref, recipient_audience_ref, recipient_audience_revision, recipient_audience_hash,
                    privacy_policy_ref, resource_authorization_ref, authority_revision
             FROM runs WHERE id = $1",
        )
        .bind(&managed.run_id)
        .fetch_one(&pool)
        .await
        .expect("read inherited run context");
        assert_eq!(
            persisted,
            (
                Some(space_id.clone()),
                Some(decision_ref.clone()),
                Some(audience_ref.clone()),
                Some(4),
                Some(audience_hash.clone()),
                Some(privacy_ref.clone()),
                Some(resource_ref.clone()),
                Some(9),
            )
        );
        let payload: serde_json::Value = sqlx::query_scalar(
            "SELECT payload FROM events WHERE run_id = $1 AND event_type = 'RUN_STARTED'",
        )
        .bind(&managed.run_id)
        .fetch_one(&pool)
        .await
        .expect("read started event");
        assert_eq!(payload["space_id"], space_id);
        assert_eq!(payload["space_decision_ref"], decision_ref);
        assert_eq!(payload["recipient_audience_ref"], audience_ref);
        assert_eq!(payload["recipient_audience_revision"], 4);
        assert_eq!(payload["recipient_audience_hash"], audience_hash);
        assert_eq!(payload["privacy_policy_ref"], privacy_ref);
        assert_eq!(payload["resource_authorization_ref"], resource_ref);
        assert_eq!(payload["authority_revision"], 9);

        sqlx::query("DELETE FROM events WHERE run_id = $1")
            .bind(&managed.run_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM plans WHERE run_id = $1")
            .bind(&managed.run_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM managed_run_terminalization_outbox WHERE run_id = $1")
            .bind(&managed.run_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM runs WHERE id = $1")
            .bind(&managed.run_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM threads WHERE id = $1")
            .bind(&thread_id)
            .execute(&pool)
            .await
            .ok();
    }

    /// A duplicate managed-start delivery is normal after a worker timeout.
    /// Both callers must receive the first durable receipt, not a uniqueness
    /// error or a second run.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn concurrent_managed_starts_converge_on_one_receipt_in_real_pg() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL unset");
            return;
        };
        let pool = sqlx::PgPool::connect(&url).await.expect("connect pg");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");

        let suffix = mp_ids::new_ulid();
        let org_id = format!("org-managed-race-{suffix}");
        let user_id = format!("user-managed-race-{suffix}");
        let thread_id = format!("thread-managed-race-{suffix}");
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, $3)",
        )
        .bind(&thread_id)
        .bind(&org_id)
        .bind(&user_id)
        .execute(&pool)
        .await
        .expect("seed thread");
        let request = pb::StartManagedRunRequest {
            thread_id: thread_id.clone(),
            parent_run_id: String::new(),
            agent_id: "managed-race-agent".to_owned(),
            goal: "retry-safe managed goal".to_owned(),
            mode: "execute".to_owned(),
            org_id: org_id.clone(),
            user_id: user_id.clone(),
            start_key: format!("managed-race-start-{suffix}"),
            terminal_source: pb::ManagedRunSource::GatewayDirect as i32,
        };
        let first_pool = pool.clone();
        let second_pool = pool.clone();
        let (first, second) = tokio::join!(
            crate::terminalization::start_managed_run_inner(&first_pool, request.clone(), false),
            crate::terminalization::start_managed_run_inner(&second_pool, request, false),
        );
        let first = first.expect("first start succeeds");
        let second = second.expect("second start replays receipt");
        assert_eq!(first.run_id, second.run_id);
        assert_eq!(first.thread_id, second.thread_id);
        assert!(first.already_started ^ second.already_started);

        let (count,): (i64,) = sqlx::query_as(
            "SELECT count(*) FROM managed_run_terminalization_outbox
             WHERE org_id = $1 AND user_id = $2 AND start_key = $3",
        )
        .bind(&org_id)
        .bind(&user_id)
        .bind(format!("managed-race-start-{suffix}"))
        .fetch_one(&pool)
        .await
        .expect("count converged obligation");
        assert_eq!(count, 1, "one start key must have one durable obligation");

        sqlx::query("DELETE FROM events WHERE run_id = $1")
            .bind(&first.run_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM plans WHERE run_id = $1")
            .bind(&first.run_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM managed_run_terminalization_outbox WHERE run_id = $1")
            .bind(&first.run_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM runs WHERE id = $1")
            .bind(&first.run_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM threads WHERE id = $1")
            .bind(&thread_id)
            .execute(&pool)
            .await
            .ok();
    }

    /// A destructive thread erase is owner-bound and removes transcript,
    /// run, event, and thread rows atomically. The bulk form must remove only
    /// the authenticated user's threads, not every thread in the org.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    #[allow(clippy::too_many_lines)]
    async fn delete_thread_and_bulk_delete_are_owner_bound_against_real_pg() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL unset");
            return;
        };
        let pool = sqlx::PgPool::connect(&url).await.expect("connect pg");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");

        let suffix = mp_ids::new_ulid();
        let org_id = format!("org-delete-{suffix}");
        let owner = format!("user-delete-{suffix}");
        let foreign = format!("user-foreign-{suffix}");
        let thread_one = format!("thread-delete-one-{suffix}");
        let thread_two = format!("thread-delete-two-{suffix}");
        let foreign_thread = format!("thread-delete-foreign-{suffix}");
        let run_id = format!("run-delete-{suffix}");

        for (thread_id, user_id) in [
            (&thread_one, &owner),
            (&thread_two, &owner),
            (&foreign_thread, &foreign),
        ] {
            sqlx::query(
                "INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, $3)",
            )
            .bind(thread_id)
            .bind(&org_id)
            .bind(user_id)
            .execute(&pool)
            .await
            .expect("seed thread");
        }
        sqlx::query(
            "INSERT INTO runs (id, thread_id, goal, org_id, user_id) VALUES ($1, $2, 'secret', $3, $4)",
        )
        .bind(&run_id)
        .bind(&thread_one)
        .bind(&org_id)
        .bind(&owner)
        .execute(&pool)
        .await
        .expect("seed run");
        sqlx::query(
            "INSERT INTO messages (id, thread_id, role, content) VALUES ($1, $2, 'user', 'secret transcript')",
        )
        .bind(format!("message-{suffix}"))
        .bind(&thread_one)
        .execute(&pool)
        .await
        .expect("seed message");
        sqlx::query(
            "INSERT INTO events (id, event_type, run_id, payload, org_id, user_id) VALUES ($1, 'RUN_STARTED', $2, '{\"secret\":\"payload\"}', $3, $4)",
        )
        .bind(format!("event-{suffix}"))
        .bind(&run_id)
        .bind(&org_id)
        .bind(&owner)
        .execute(&pool)
        .await
        .expect("seed event");

        let svc = super::SessionService {
            pool: pool.clone(),
            retrieval_client: None,
            graph_client: None,
            knowledge_client: None,
            letta_memory: None,
            audit_publisher: None,
            auth: None,
        };

        let mut cross_request = Request::new(pb::DeleteThreadRequest {
            org_id: org_id.clone(),
            thread_id: thread_one.clone(),
        });
        cross_request
            .extensions_mut()
            .insert(VerifiedIdentity::user_for_test(&org_id, &foreign));
        assert_eq!(
            svc.delete_thread(cross_request)
                .await
                .expect_err("foreign user must not erase thread")
                .code(),
            tonic::Code::PermissionDenied
        );
        let (remaining,): (i64,) = sqlx::query_as("SELECT count(*) FROM threads WHERE id = $1")
            .bind(&thread_one)
            .fetch_one(&pool)
            .await
            .expect("check foreign delete rejection");
        assert_eq!(remaining, 1);

        let mut owner_request = Request::new(pb::DeleteThreadRequest {
            org_id: org_id.clone(),
            thread_id: thread_one.clone(),
        });
        owner_request
            .extensions_mut()
            .insert(VerifiedIdentity::user_for_test(&org_id, &owner));
        let receipt = svc
            .delete_thread(owner_request)
            .await
            .expect("owner delete")
            .into_inner();
        assert!(receipt.deleted);
        for (table, column, id) in [
            ("threads", "id", thread_one.as_str()),
            ("messages", "thread_id", thread_one.as_str()),
            ("runs", "thread_id", thread_one.as_str()),
            ("events", "run_id", run_id.as_str()),
        ] {
            let query = format!("SELECT count(*) FROM {table} WHERE {column} = $1");
            let (count,): (i64,) = sqlx::query_as(&query)
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("check erased row");
            assert_eq!(count, 0, "{table}.{column} retained erased content");
        }

        let mut bulk_request = Request::new(pb::DeleteThreadsRequest {
            org_id: org_id.clone(),
            user_id: owner.clone(),
        });
        bulk_request
            .extensions_mut()
            .insert(VerifiedIdentity::user_for_test(&org_id, &owner));
        let bulk = svc
            .delete_threads(bulk_request)
            .await
            .expect("owner bulk delete")
            .into_inner();
        assert_eq!(bulk.deleted_count, 1);
        let (owned_left,): (i64,) =
            sqlx::query_as("SELECT count(*) FROM threads WHERE org_id = $1 AND user_id = $2")
                .bind(&org_id)
                .bind(&owner)
                .fetch_one(&pool)
                .await
                .expect("check bulk erase");
        assert_eq!(owned_left, 0);
        let (foreign_left,): (i64,) =
            sqlx::query_as("SELECT count(*) FROM threads WHERE org_id = $1 AND user_id = $2")
                .bind(&org_id)
                .bind(&foreign)
                .fetch_one(&pool)
                .await
                .expect("check foreign thread retained");
        assert_eq!(foreign_left, 1);

        sqlx::query("DELETE FROM threads WHERE id = $1")
            .bind(&foreign_thread)
            .execute(&pool)
            .await
            .expect("cleanup foreign thread");
    }

    /// The cross-plane deletion coordinator is permitted to erase only the
    /// requested owner's records in the requested Space. A same-tenant row
    /// for another Space or another owner is a hard negative boundary.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn delete_space_threads_is_exactly_owner_and_space_bound_against_real_pg() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL unset");
            return;
        };
        let pool = sqlx::PgPool::connect(&url).await.expect("connect pg");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");

        let suffix = mp_ids::new_ulid();
        let org_id = format!("org-space-delete-{suffix}");
        let owner = format!("user-space-owner-{suffix}");
        let foreign = format!("user-space-foreign-{suffix}");
        let target_space = format!("space-target-{suffix}");
        let other_space = format!("space-other-{suffix}");
        let target_thread = format!("thread-space-target-{suffix}");
        let other_space_thread = format!("thread-space-other-{suffix}");
        let foreign_thread = format!("thread-space-foreign-{suffix}");
        let authority_ref = format!("authority-{suffix}");
        let audience_ref = format!("audience-{suffix}");
        let audience_hash = format!("audience-hash-{suffix}");
        let privacy_ref = format!("privacy-{suffix}");
        let resource_ref = format!("resource-{suffix}");

        for (thread_id, user_id, space_id) in [
            (&target_thread, &owner, &target_space),
            (&other_space_thread, &owner, &other_space),
            (&foreign_thread, &foreign, &target_space),
        ] {
            sqlx::query(
                "INSERT INTO threads (\
                    id, session_key, org_id, user_id, space_id, space_decision_ref, \
                    recipient_audience_ref, privacy_policy_ref, resource_authorization_ref, \
                    authority_revision, recipient_audience_revision, recipient_audience_hash\
                ) VALUES ($1, $1, $2, $3, $4, $5, $6, $7, $8, 1, 1, $9)",
            )
            .bind(thread_id)
            .bind(&org_id)
            .bind(user_id)
            .bind(space_id)
            .bind(&authority_ref)
            .bind(&audience_ref)
            .bind(&privacy_ref)
            .bind(&resource_ref)
            .bind(&audience_hash)
            .execute(&pool)
            .await
            .expect("seed scoped thread");
        }

        let svc = super::SessionService {
            pool: pool.clone(),
            retrieval_client: None,
            graph_client: None,
            knowledge_client: None,
            letta_memory: None,
            audit_publisher: None,
            auth: None,
        };
        let mut request = Request::new(pb::DeleteSpaceThreadsRequest {
            org_id: org_id.clone(),
            space_id: target_space.clone(),
            owner_principal_id: owner.clone(),
            deletion_request_id: format!("deletion-request-{suffix}"),
        });
        request
            .extensions_mut()
            .insert(VerifiedIdentity::service_for_test_as(
                &org_id,
                SPACE_DELETION_SERVICE,
                &[SPACE_DELETION_SCOPE],
                false,
            ));
        let receipt = svc
            .delete_space_threads(request)
            .await
            .expect("exactly scoped space deletion")
            .into_inner();
        assert_eq!(receipt.deleted_count, 1);

        for (thread_id, expected) in [
            (&target_thread, 0_i64),
            (&other_space_thread, 1_i64),
            (&foreign_thread, 1_i64),
        ] {
            let (count,): (i64,) = sqlx::query_as("SELECT count(*) FROM threads WHERE id = $1")
                .bind(thread_id)
                .fetch_one(&pool)
                .await
                .expect("check exact scope boundary");
            assert_eq!(count, expected, "unexpected deletion scope for {thread_id}");
        }

        for thread_id in [&other_space_thread, &foreign_thread] {
            sqlx::query("DELETE FROM threads WHERE id = $1")
                .bind(thread_id)
                .execute(&pool)
                .await
                .expect("cleanup preserved thread");
        }
    }

    #[tokio::test]
    async fn delete_space_threads_rejects_cross_org_coordinator_before_database_access() {
        let pool =
            sqlx::PgPool::connect_lazy("postgres://postgres:postgres@127.0.0.1/session_core")
                .expect("construct lazy pool");
        let svc = super::SessionService {
            pool,
            retrieval_client: None,
            graph_client: None,
            knowledge_client: None,
            letta_memory: None,
            audit_publisher: None,
            auth: None,
        };
        let mut request = Request::new(pb::DeleteSpaceThreadsRequest {
            org_id: "org-b".to_owned(),
            space_id: "space-b".to_owned(),
            owner_principal_id: "user-b".to_owned(),
            deletion_request_id: "delete-b".to_owned(),
        });
        request
            .extensions_mut()
            .insert(VerifiedIdentity::service_for_test_as(
                "org-a",
                SPACE_DELETION_SERVICE,
                &[SPACE_DELETION_SCOPE],
                false,
            ));

        assert_eq!(
            svc.delete_space_threads(request)
                .await
                .expect_err("cross-org coordinator must fail before SQL")
                .code(),
            tonic::Code::PermissionDenied
        );
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn legacy_terminal_step_cannot_settle_a_managed_run_in_postgres() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL unset");
            return;
        };
        let pool = sqlx::PgPool::connect(&url).await.expect("connect pg");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");

        let suffix = mp_ids::new_ulid();
        let thread_id = format!("managed-terminal-thread-{suffix}");
        let org_id = format!("managed-terminal-org-{suffix}");
        let legacy_run_id = format!("managed-terminal-legacy-{suffix}");
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id)
             VALUES ($1, $1, $2, 'user-1')",
        )
        .bind(&thread_id)
        .bind(&org_id)
        .execute(&pool)
        .await
        .expect("seed thread");
        sqlx::query(
            "INSERT INTO runs (id, thread_id, goal, org_id, user_id, status)
             VALUES ($1, $2, 'legacy goal', $3, 'user-1', 'running')",
        )
        .bind(&legacy_run_id)
        .bind(&thread_id)
        .bind(&org_id)
        .execute(&pool)
        .await
        .expect("seed legacy run");

        // Control: the legacy API remains compatible for a run with no
        // managed-terminalization obligation.
        complete_step_inner(
            &pool,
            None,
            pb::CompleteStepRequest {
                run_id: legacy_run_id.clone(),
                step_id: "legacy-final".to_owned(),
                status: "completed".to_owned(),
                output: String::new(),
                error: String::new(),
                terminal: true,
            },
        )
        .await
        .expect("legacy terminal completion remains supported");

        let managed = crate::terminalization::start_managed_run_inner(
            &pool,
            pb::StartManagedRunRequest {
                thread_id: thread_id.clone(),
                parent_run_id: String::new(),
                agent_id: "managed-agent".to_owned(),
                goal: "managed goal".to_owned(),
                mode: "execute".to_owned(),
                org_id: org_id.clone(),
                user_id: "user-1".to_owned(),
                start_key: format!("managed-terminal-{suffix}"),
                terminal_source: pb::ManagedRunSource::GatewayDirect as i32,
            },
            false,
        )
        .await
        .expect("start managed run");

        let error = complete_step_inner(
            &pool,
            None,
            pb::CompleteStepRequest {
                run_id: managed.run_id.clone(),
                step_id: "forged-final".to_owned(),
                status: "completed".to_owned(),
                output: "forged terminal output".to_owned(),
                error: String::new(),
                terminal: true,
            },
        )
        .await
        .expect_err("legacy terminal step must not bypass a managed receipt");
        assert_eq!(error.code(), tonic::Code::FailedPrecondition);

        let managed_status: String = sqlx::query_scalar("SELECT status FROM runs WHERE id = $1")
            .bind(&managed.run_id)
            .fetch_one(&pool)
            .await
            .expect("read managed status");
        assert_eq!(managed_status, "queued");
        let obligation: (String, Option<String>) = sqlx::query_as(
            "SELECT state, outcome
             FROM managed_run_terminalization_outbox
             WHERE run_id = $1",
        )
        .bind(&managed.run_id)
        .fetch_one(&pool)
        .await
        .expect("read managed obligation");
        assert_eq!(obligation, ("open".to_owned(), None));
        let terminal_events: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM events
             WHERE run_id = $1 AND event_type IN ('RUN_COMPLETED', 'RUN_FAILED')",
        )
        .bind(&managed.run_id)
        .fetch_one(&pool)
        .await
        .expect("count managed terminal events");
        assert_eq!(terminal_events, 0);

        sqlx::query("DELETE FROM events WHERE run_id IN ($1, $2)")
            .bind(&legacy_run_id)
            .bind(&managed.run_id)
            .execute(&pool)
            .await
            .expect("clean event fixtures");
        sqlx::query("DELETE FROM plans WHERE run_id = $1")
            .bind(&managed.run_id)
            .execute(&pool)
            .await
            .expect("clean managed plan");
        sqlx::query("DELETE FROM runs WHERE id IN ($1, $2)")
            .bind(&legacy_run_id)
            .bind(&managed.run_id)
            .execute(&pool)
            .await
            .expect("clean run fixtures");
        sqlx::query("DELETE FROM threads WHERE id = $1")
            .bind(&thread_id)
            .execute(&pool)
            .await
            .expect("clean thread fixture");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn inline_tool_audit_reservation_and_finalization_are_idempotent_in_postgres() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL unset");
            return;
        };
        let pool = sqlx::PgPool::connect(&url).await.expect("connect pg");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");

        let suffix = mp_ids::new_ulid();
        let thread_id = format!("tool-audit-thread-{suffix}");
        let run_id = format!("tool-audit-run-{suffix}");
        let plan_id = format!("plan_{run_id}");
        let org_id = format!("tool-audit-org-{suffix}");
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id)
             VALUES ($1, $1, $2, 'user-1')",
        )
        .bind(&thread_id)
        .bind(&org_id)
        .execute(&pool)
        .await
        .expect("seed thread");
        sqlx::query(
            "INSERT INTO runs (id, thread_id, goal, org_id, user_id, status)
             VALUES ($1, $2, 'g', $3, 'user-1', 'running')",
        )
        .bind(&run_id)
        .bind(&thread_id)
        .bind(&org_id)
        .execute(&pool)
        .await
        .expect("seed run");
        sqlx::query(
            "INSERT INTO plans (id, thread_id, run_id, status, org_id, user_id)
             VALUES ($1, $2, $3, 'draft', $4, 'user-1')",
        )
        .bind(&plan_id)
        .bind(&thread_id)
        .bind(&run_id)
        .bind(&org_id)
        .execute(&pool)
        .await
        .expect("seed plan");

        let legacy_step_id = "legacy-step";
        let legacy_resource = format!("run:{run_id}:step:{legacy_step_id}");
        let legacy_idempotency_key = derive_idempotency_hash(
            "session-core",
            "STEP_COMPLETED",
            &legacy_resource,
            &format!("{run_id}:{legacy_step_id}"),
        );
        let legacy_ordinal: i64 = sqlx::query_scalar(
            "INSERT INTO events (id, event_type, run_id, payload, ts, org_id, user_id, correlation_id, causation_id, idempotency_key, resource_ref, type_url, producer, schema_version)
             SELECT $1, 'STEP_COMPLETED', $2, $3, now(), r.org_id, r.user_id, $2, '', $4, $5, $6, 'session-core', 1
             FROM runs r WHERE r.id = $2
             RETURNING step_ordinal",
        )
        .bind(format!("legacy-step-event-{suffix}"))
        .bind(&run_id)
        .bind(serde_json::json!({
            "step_id": legacy_step_id,
            "status": "running",
            "output": "legacy output",
            "error": "",
        }))
        .bind(&legacy_idempotency_key)
        .bind(&legacy_resource)
        .bind(STEP_COMPLETED_TYPE_URL)
        .fetch_one(&pool)
        .await
        .expect("seed pre-terminal-field step event");
        let legacy_replay = complete_step_inner(
            &pool,
            None,
            pb::CompleteStepRequest {
                run_id: run_id.clone(),
                step_id: legacy_step_id.to_owned(),
                status: "running".to_owned(),
                output: "legacy output".to_owned(),
                error: String::new(),
                terminal: false,
            },
        )
        .await
        .expect("legacy exact step replay must remain idempotent")
        .into_inner();
        assert_eq!(
            legacy_replay.step_index,
            u32::try_from(legacy_ordinal).expect("legacy ordinal fits u32")
        );

        let reservation = pb::ReserveToolActionRequest {
            run_id: run_id.clone(),
            action_id: "inline-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .to_owned(),
            request_id: "request-1".to_owned(),
            tool: "web_search".to_owned(),
            data_category: "public_non_personal".to_owned(),
            zdr: false,
        };
        assert!(
            reserve_tool_action_inner(&pool, None, reservation.clone())
                .await
                .expect("reserve")
                .into_inner()
                .created
        );
        assert!(
            !reserve_tool_action_inner(&pool, None, reservation.clone())
                .await
                .expect("exact reserve replay")
                .into_inner()
                .created
        );
        let mut conflict = reservation.clone();
        conflict.request_id = "different-request".to_owned();
        assert_eq!(
            reserve_tool_action_inner(&pool, None, conflict)
                .await
                .expect_err("conflicting binding")
                .code(),
            tonic::Code::AlreadyExists
        );

        let finalization = pb::FinalizeToolActionRequest {
            run_id: run_id.clone(),
            action_id: reservation.action_id.clone(),
            outcome: "completed".to_owned(),
        };
        assert!(
            finalize_tool_action_inner(&pool, None, finalization.clone())
                .await
                .expect("finalize")
                .into_inner()
                .updated
        );
        assert!(
            !finalize_tool_action_inner(&pool, None, finalization.clone())
                .await
                .expect("exact finalization replay")
                .into_inner()
                .updated
        );
        let mut conflicting_final = finalization;
        conflicting_final.outcome = "failed".to_owned();
        assert_eq!(
            finalize_tool_action_inner(&pool, None, conflicting_final)
                .await
                .expect_err("conflicting finalization")
                .code(),
            tonic::Code::AlreadyExists
        );

        let ordinary_step = pb::CompleteStepRequest {
            run_id: run_id.clone(),
            step_id: "ordinary-step".to_owned(),
            status: "completed".to_owned(),
            output: "ordinary step output".to_owned(),
            error: String::new(),
            terminal: false,
        };
        complete_step_inner(&pool, None, ordinary_step.clone())
            .await
            .expect("non-terminal completed step");
        assert_eq!(
            complete_step_inner(
                &pool,
                None,
                pb::CompleteStepRequest {
                    terminal: true,
                    ..ordinary_step
                },
            )
            .await
            .expect_err("an explicit terminal mismatch is not an exact replay")
            .code(),
            tonic::Code::AlreadyExists
        );

        let lifecycle: (String, String) = sqlx::query_as(
            "SELECT r.status, p.status FROM runs r JOIN plans p ON p.run_id = r.id WHERE r.id = $1",
        )
        .bind(&run_id)
        .fetch_one(&pool)
        .await
        .expect("load run lifecycle");
        assert_eq!(lifecycle, ("running".to_owned(), "draft".to_owned()));

        let terminal = pb::CompleteStepRequest {
            run_id: run_id.clone(),
            step_id: "final".to_owned(),
            status: "completed".to_owned(),
            output: "final output".to_owned(),
            error: String::new(),
            terminal: true,
        };
        let first_terminal = complete_step_inner(&pool, None, terminal.clone())
            .await
            .expect("first terminal completion")
            .into_inner();
        let terminal_replay = complete_step_inner(&pool, None, terminal.clone())
            .await
            .expect("exact terminal replay must be idempotent")
            .into_inner();
        assert_eq!(terminal_replay.step_index, first_terminal.step_index);

        let conflicting_terminal = pb::CompleteStepRequest {
            step_id: "different-final-step".to_owned(),
            status: "failed".to_owned(),
            output: String::new(),
            error: "late conflicting failure".to_owned(),
            ..terminal
        };
        assert_eq!(
            complete_step_inner(&pool, None, conflicting_terminal)
                .await
                .expect_err("terminal outcome must be single-assignment")
                .code(),
            tonic::Code::AlreadyExists
        );

        let terminal_lifecycle: (String, String) = sqlx::query_as(
            "SELECT r.status, p.status FROM runs r JOIN plans p ON p.run_id = r.id WHERE r.id = $1",
        )
        .bind(&run_id)
        .fetch_one(&pool)
        .await
        .expect("load terminal lifecycle");
        assert_eq!(
            terminal_lifecycle,
            ("completed".to_owned(), "completed".to_owned())
        );
        let terminal_events: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM events
             WHERE run_id = $1 AND event_type IN ('RUN_COMPLETED', 'RUN_FAILED')",
        )
        .bind(&run_id)
        .fetch_one(&pool)
        .await
        .expect("count terminal events");
        assert_eq!(
            terminal_events, 1,
            "one run may have only one terminal event"
        );

        let phases: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT payload->'details'->>'phase', payload->>'outcome', payload->>'event'
             FROM session_audit_outbox
             WHERE payload->>'subject' = $1
             ORDER BY payload->'details'->>'phase'",
        )
        .bind(&run_id)
        .fetch_all(&pool)
        .await
        .expect("load audit phases");
        assert_eq!(
            phases,
            vec![
                (
                    "final".to_owned(),
                    "completed".to_owned(),
                    "tool_action".to_owned(),
                ),
                (
                    "reserved".to_owned(),
                    "reserved".to_owned(),
                    "tool_action_reserved".to_owned(),
                ),
            ]
        );

        sqlx::query("DELETE FROM session_audit_outbox WHERE payload->>'subject' = $1")
            .bind(&run_id)
            .execute(&pool)
            .await
            .expect("clean outbox fixture");
        sqlx::query("DELETE FROM events WHERE run_id = $1")
            .bind(&run_id)
            .execute(&pool)
            .await
            .expect("clean event fixture");
        sqlx::query("DELETE FROM plan_steps WHERE plan_id = $1")
            .bind(&plan_id)
            .execute(&pool)
            .await
            .expect("clean plan-step fixture");
        sqlx::query("DELETE FROM plans WHERE id = $1")
            .bind(&plan_id)
            .execute(&pool)
            .await
            .expect("clean plan fixture");
        sqlx::query("DELETE FROM runs WHERE id = $1")
            .bind(&run_id)
            .execute(&pool)
            .await
            .expect("clean run fixture");
        sqlx::query("DELETE FROM threads WHERE id = $1")
            .bind(&thread_id)
            .execute(&pool)
            .await
            .expect("clean thread fixture");
    }

    // True multi-service round-trip: a real SessionCoreClient calls a real
    // session-core gRPC server over TCP, persisting to real Postgres — the
    // EXACT client→wire→handler→DB path the gateway's plan-mode write-through
    // uses (gateway calls session_client.set_run_mode the same way). Verifies
    // the deployed transport + handler + durability together.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a Postgres with session-core migrations"]
    async fn grpc_set_run_mode_round_trip_over_the_wire() {
        use super::SessionService;
        use mp_contracts::model_plane::v1 as pb;
        use mp_contracts::model_plane::v1::session_core_client::SessionCoreClient;
        use mp_contracts::model_plane::v1::session_core_server::SessionCoreServer;
        use std::time::Duration;
        use tonic::transport::Server;

        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL unset");
            return;
        };
        let pool = sqlx::PgPool::connect(&url).await.expect("connect pg");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");

        let sfx = std::process::id();
        let (thread_id, run_id, org) = (
            format!("e2e-t-{sfx}"),
            format!("e2e-r-{sfx}"),
            format!("e2e-org-{sfx}"),
        );
        sqlx::query("INSERT INTO threads (id,session_key,org_id,user_id) VALUES ($1,$1,$2,'u1')")
            .bind(&thread_id)
            .bind(&org)
            .execute(&pool)
            .await
            .expect("seed thread");
        sqlx::query(
            "INSERT INTO runs (id,thread_id,goal,org_id,user_id) VALUES ($1,$2,'g',$3,'u1')",
        )
        .bind(&run_id)
        .bind(&thread_id)
        .bind(&org)
        .execute(&pool)
        .await
        .expect("seed run");

        let svc = SessionService {
            pool: pool.clone(),
            retrieval_client: None,
            graph_client: None,
            knowledge_client: None,
            letta_memory: None,
            audit_publisher: None,
            auth: None,
        };
        let addr: std::net::SocketAddr = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        tokio::spawn(async move {
            Server::builder()
                .add_service(SessionCoreServer::new(svc))
                .serve(addr)
                .await
                .ok();
        });

        // Connect with retry while the server binds.
        let mut client = None;
        for _ in 0..30 {
            if let Ok(c) = SessionCoreClient::connect(format!("http://{addr}")).await {
                client = Some(c);
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let mut client = client.expect("connect to session-core over the wire");

        // Over-the-wire call (the gateway's write-through mechanism).
        let resp = client
            .set_run_mode(pb::SetRunModeRequest {
                run_id: run_id.clone(),
                mode: "plan".into(),
                org_id: org.clone(),
                granted_rung: 0,
                justification: String::new(),
            })
            .await
            .expect("set_run_mode rpc over wire")
            .into_inner();
        assert_eq!(resp.mode, "plan");

        // And it actually persisted in Postgres.
        let (mode,): (String,) = sqlx::query_as("SELECT mode FROM runs WHERE id = $1")
            .bind(&run_id)
            .fetch_one(&pool)
            .await
            .expect("read back run mode");
        assert_eq!(
            mode, "plan",
            "run mode must be durable after the wire round-trip"
        );

        for q in [
            "DELETE FROM runs WHERE org_id=$1",
            "DELETE FROM threads WHERE org_id=$1",
        ] {
            sqlx::query(q).bind(&org).execute(&pool).await.ok();
        }
    }

    /// The regression this guards: compose supplies `DATAPLANE_*_URL` (written
    /// against model-gateway's spelling) while this service historically read
    /// only `_ADDR`, so every grounding tier fell back to localhost and went
    /// silently empty. Accepting either spelling is the fix; this pins it.
    #[test]
    fn dataplane_addr_accepts_either_spelling_and_ignores_blanks() {
        let url = Some("http://dpv2-retrieval-engine:50052".to_string());
        let addr = Some("http://legacy:50052".to_string());
        let fallback = "http://localhost:50052";

        // Candidates are passed in `[URL, ADDR]` order, so URL wins when both
        // are set -- matching model-gateway's precedence exactly.
        assert_eq!(
            resolve_dataplane_addr(&[url.clone(), addr.clone()], fallback),
            "http://dpv2-retrieval-engine:50052"
        );

        // The spelling compose does NOT currently use must still work, so an
        // operator with the older variable name is not silently ignored.
        assert_eq!(
            resolve_dataplane_addr(&[None, addr.clone()], fallback),
            "http://legacy:50052"
        );

        // `FOO=` in an env file is unset, not an endpoint. Without this, a blank
        // URL would shadow a good ADDR and reintroduce the outage.
        assert_eq!(
            resolve_dataplane_addr(&[Some("   ".to_string()), addr], fallback),
            "http://legacy:50052"
        );

        // Only with nothing configured do we reach the localhost default.
        assert_eq!(resolve_dataplane_addr(&[None, None], fallback), fallback);
        assert_eq!(
            resolve_dataplane_addr(&[Some(String::new()), None], fallback),
            fallback
        );
    }

    // -- list_agent_skills scope enforcement (real Postgres) -----------------
    //
    // SKILL-1 (0031_agent_skills_ownership.sql) added scope/owner_user_id/
    // shared_with, mirroring capability-core's SkillsHandler.listOrCreate --
    // the reference implementation that DOES enforce them on read. This RPC
    // selected the columns but never filtered on them, so every "private"
    // skill was returned to every org member. Rows are seeded directly via
    // SQL (not upsert_agent_skill, which only ever writes scope='org' for
    // its background-review origin) to exercise every scope/owner/share
    // combination precisely.
    //
    // #[ignore]d so plain `cargo test` (no DB) skips it; run with a DB:
    //   DATABASE_URL=… cargo test --bin session-core -- --ignored list_agent_skills_scope

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a Postgres with session-core migrations"]
    async fn list_agent_skills_scope_enforcement_against_real_pg() {
        use super::SessionService;

        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL unset");
            return;
        };
        let pool = sqlx::PgPool::connect(&url).await.expect("connect pg");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");

        let sfx = std::process::id();
        let org_id = format!("las-org-{sfx}");
        let (owner, sharee, stranger) = (
            format!("las-owner-{sfx}"),
            format!("las-sharee-{sfx}"),
            format!("las-stranger-{sfx}"),
        );

        for (name, scope, owner_user_id, shared_with) in [
            ("org-skill", "org", "", "[]"),
            ("private-skill", "user", owner.as_str(), "[]"),
            (
                "shared-skill",
                "user",
                owner.as_str(),
                &format!("[\"{sharee}\"]"),
            ),
        ] {
            sqlx::query(
                "INSERT INTO agent_skills
                    (id, org_id, name, description, content, trigger_keywords,
                     trigger_file_patterns, tool_restrictions, enabled, origin,
                     scope, owner_user_id, shared_with, created_at, updated_at)
                 VALUES ($1,$2,$3,'d','c','[]','[]','[]',true,'user',$4,$5,$6::jsonb,now(),now())
                 ON CONFLICT (org_id, name) DO NOTHING",
            )
            .bind(format!("{name}-{sfx}"))
            .bind(&org_id)
            .bind(name)
            .bind(scope)
            .bind(owner_user_id)
            .bind(shared_with)
            .execute(&pool)
            .await
            .expect("seed skill");
        }

        let svc = SessionService {
            pool: pool.clone(),
            retrieval_client: None,
            graph_client: None,
            knowledge_client: None,
            letta_memory: None,
            audit_publisher: None,
            auth: None,
        };

        let list_as = |user_id: String| {
            let org_id = org_id.clone();
            let svc = &svc;
            async move {
                let mut request = Request::new(pb::ListAgentSkillsRequest {
                    org_id: org_id.clone(),
                    enabled_only: false,
                });
                request
                    .extensions_mut()
                    .insert(VerifiedIdentity::user_for_test(&org_id, &user_id));
                svc.list_agent_skills(request)
                    .await
                    .expect("list_agent_skills")
                    .into_inner()
                    .skills
                    .into_iter()
                    .map(|s| s.name)
                    .collect::<std::collections::BTreeSet<_>>()
            }
        };

        let owner_view = list_as(owner.clone()).await;
        assert!(
            owner_view.contains("org-skill")
                && owner_view.contains("private-skill")
                && owner_view.contains("shared-skill"),
            "the owner must see the org skill, their own private skill, and their own shared skill: {owner_view:?}"
        );

        let sharee_view = list_as(sharee.clone()).await;
        assert!(
            sharee_view.contains("org-skill") && sharee_view.contains("shared-skill"),
            "the sharee must see the org skill and the skill shared with them: {sharee_view:?}"
        );
        assert!(
            !sharee_view.contains("private-skill"),
            "the sharee must NOT see a skill never shared with them: {sharee_view:?}"
        );

        let stranger_view = list_as(stranger.clone()).await;
        assert!(
            stranger_view.contains("org-skill"),
            "a stranger must still see org-wide skills: {stranger_view:?}"
        );
        assert!(
            !stranger_view.contains("private-skill") && !stranger_view.contains("shared-skill"),
            "a stranger must see neither the owner's private skill nor a skill shared with someone else: {stranger_view:?}"
        );
    }
}
