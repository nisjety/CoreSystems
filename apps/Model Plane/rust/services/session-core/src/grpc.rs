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
    message_search_server::MessageSearchServer,
    orchestration_core_service_server::OrchestrationCoreServiceServer,
    routing_policy_server::RoutingPolicyServer,
    run_service_server::RunServiceServer,
    session_core_server::{SessionCore, SessionCoreServer},
    session_retention_service_server::{SessionRetentionService, SessionRetentionServiceServer},
    thread_autonomy_server::{ThreadAutonomy, ThreadAutonomyServer},
};
use mp_contracts::run_origin::{RunOrigin, ORIGIN_METADATA_KEY};
use mp_contracts::zdr_refusal::Refusal;
use mp_events::idempotency::derive_idempotency_hash;
use mp_ids::new_ulid;
use sha2::{Digest as _, Sha256};
use sqlx::PgPool;
use std::{collections::BTreeMap, env, time::Instant};
use tonic::transport::Channel;
use tonic::{Request, Response, Status};
use tracing::{info, warn};

use crate::auth::{
    authorize_operation, authorize_owner_housekeeping, authorize_owner_row,
    authorize_scheduled_step_service, authorize_space_deletion_service, authorize_system_run_owner,
    authorize_thread_owner_row, identity, is_system_run_owner, DelegatedDataPlaneBearer,
    JwtVerifier, OwnerIntent, VerifiedIdentity, DATA_PLANE_AUTH_METADATA_KEY,
};
use crate::continuation_crypto::DescriptorCipher;
use crate::letta_adapter::{LettaMemoryAdapter, LettaSearchOutcome};
use crate::loop_checkpoint;
use crate::orchestration_grpc::{json_to_struct, struct_to_json};
use crate::retention_receipt::refuse_zdr_continuation;
use crate::terminalization;

/// Standard gRPC health names registered on the unauthenticated health-only
/// surface. Business RPCs remain independently intercepted.
pub const HEALTH_SERVICE_NAMES: [&str; 9] = [
    "model_plane.v1.SessionCore",
    "model_plane.v1.ManagedRunLifecycle",
    "model_plane.v1.OrchestrationCoreService",
    "model_plane.v1.FinetuneJobs",
    "model_plane.v1.MemoryService",
    "model_plane.v1.RoutingPolicy",
    "model_plane.v1.RunService",
    "model_plane.v1.SessionRetentionService",
    "model_plane.v1.ThreadAutonomy",
];

const RUN_STARTED_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.RunStarted";
const STEP_COMPLETED_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.StepCompleted";
const RUN_TERMINAL_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.RunTerminal";
pub(crate) const CHECKPOINT_SAVED_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.CheckpointSaved";
const THREAD_CREATED_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.ThreadCreated";
const MESSAGE_APPENDED_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.MessageAppended";
const THREAD_PRESENTATION_UPDATED_TYPE_URL: &str =
    "type.googleapis.com/model_plane.v1.ThreadPresentationUpdated";
const THREAD_ARCHIVED_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.ThreadArchived";
const CONTEXT_MESSAGE_LIMIT: i64 = 20;
const CONTEXT_MEMORY_LIMIT: i64 = 64;
const DEFAULT_THREAD_LIST_LIMIT: i64 = 80;
const MAX_THREAD_LIST_LIMIT: i64 = 200;
pub(crate) const THREAD_TITLE_MAX_CHARS: usize = 96;
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
const CONTROL_THREAD_READ_ACTION: &str = "model.thread.read";
const CONTROL_THREAD_READ_SCHEMA: &str = "sha256:thread-read-v1";
const CONTROL_SCHEDULED_RUN_AUDIENCE: &str = "model-plane-capability-core";
const CONTROL_SCHEDULED_RUN_ACTION: &str = "model.schedule.run";
const CONTROL_SCHEDULED_RUN_SCHEMA: &str = "sha256:space-scheduled-run-v1";
const CONTROL_SCHEDULED_RUN_EXECUTION_AUDIENCE: &str = "model-plane-session-core";
const CONTROL_SCHEDULED_RUN_EXECUTION_ACTION: &str = "model.schedule.execute";
const CONTROL_SCHEDULED_RUN_EXECUTION_SCHEMA: &str = "sha256:space-scheduled-run-execute-v1";
// A run bound to a stored-consent contract (WS20 S5) uses its own schema hash;
// the v1 digest stays byte-identical for every schedule without a contract.
const CONTROL_SCHEDULED_RUN_EXECUTION_CONTRACT_SCHEMA: &str =
    "sha256:space-scheduled-run-execute-v2";
const MAX_SCHEDULED_CONTRACT_BYTES: usize = 64 * 1024;
const MAX_CONTROL_SPACE_DECISION_TOKEN_BYTES: usize = 16 * 1024;
// Delegated-identity design, Gap B (03-design-2-token-exchange.md §2.5). A
// distinct envelope version from CONTROL_SPACE_DECISION_VERSION ("v2") --
// deliberately not reusing that tag or the full Space Decision claim shape
// (security-review §B-1/B-2): a mail-task-v1 token can never verify against
// the Space-decision verifier above, or vice versa, by construction.
const MAIL_TASK_DECISION_VERSION: &str = "mail-task-v1";
const MAIL_TASK_THREAD_PREPARE_AUDIENCE: &str = "model-plane-session-core";
const MAIL_TASK_THREAD_PREPARE_ACTION: &str = "mail_task:thread_prepare";

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

pub(crate) fn to_proto_timestamp(ts: DateTime<Utc>) -> prost_types::Timestamp {
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

/// The sort position a ListThreads page ended on. The list sorts pinned threads
/// first, then by last activity, then by id, so these three values identify the
/// row and the next page starts strictly after them.
#[derive(Debug, PartialEq)]
struct ThreadCursor {
    pinned: bool,
    last_activity_at: DateTime<Utc>,
    thread_id: String,
}

fn encode_thread_cursor(cursor: &ThreadCursor) -> String {
    // One-letter keys keep the cursor short when it travels in a query string.
    let json = serde_json::json!({
        "p": cursor.pinned,
        "a": cursor.last_activity_at.to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
        "i": cursor.thread_id,
    });
    URL_SAFE_NO_PAD.encode(json.to_string())
}

fn decode_thread_cursor(raw: &str) -> Result<ThreadCursor, Status> {
    let invalid = || Status::invalid_argument("cursor is not a thread list cursor");
    let bytes = URL_SAFE_NO_PAD.decode(raw).map_err(|_| invalid())?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    let pinned = value["p"].as_bool().ok_or_else(invalid)?;
    let last_activity_at = value["a"]
        .as_str()
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .map(|parsed| parsed.with_timezone(&Utc))
        .ok_or_else(invalid)?;
    let thread_id = value["i"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or_else(invalid)?
        .to_owned();
    Ok(ThreadCursor {
        pinned,
        last_activity_at,
        thread_id,
    })
}

#[cfg(test)]
mod thread_cursor_tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    #[test]
    fn cursor_round_trips_the_sort_position_to_the_microsecond() {
        let cursor = ThreadCursor {
            pinned: true,
            last_activity_at: Utc.with_ymd_and_hms(2026, 10, 1, 12, 30, 15).unwrap()
                + Duration::microseconds(123_456),
            thread_id: "01HZTHREAD".to_owned(),
        };
        let raw = encode_thread_cursor(&cursor);
        assert_eq!(decode_thread_cursor(&raw).unwrap(), cursor);
    }

    #[test]
    fn cursor_rejects_anything_that_is_not_a_thread_position() {
        let cases = vec![
            "!!!".to_owned(),
            URL_SAFE_NO_PAD.encode("[]"),
            URL_SAFE_NO_PAD.encode(r#"{"p":true,"a":"not-a-time","i":"t"}"#),
            URL_SAFE_NO_PAD.encode(r#"{"p":"yes","a":"2026-10-01T00:00:00Z","i":"t"}"#),
            URL_SAFE_NO_PAD.encode(r#"{"p":true,"a":"2026-10-01T00:00:00Z","i":""}"#),
        ];
        for raw in cases {
            let error = decode_thread_cursor(&raw).expect_err(&raw);
            assert_eq!(error.code(), tonic::Code::InvalidArgument, "{raw}");
        }
    }
}

pub(crate) fn compact_thread_text(
    value: Option<String>,
    fallback: &str,
    max_chars: usize,
) -> String {
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

pub(crate) fn record_metrics(method: &'static str, started: Instant, is_ok: bool) {
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
    /// Whether the thread grant columns of migration 0050 exist, checked once at boot
    /// ([`ThreadGrantColumns::detect`]). Gates the grant write of `SetRunMode`.
    grant_columns: ThreadGrantColumns,
}

/// Additive managed-run protocol. Keeping this service separate preserves the
/// legacy `SessionCore` trait and its existing mocks while giving producers a
/// durable, source-bound terminal receipt contract.
pub struct ManagedRunLifecycleService {
    pool: PgPool,
    /// `CompleteDirectRunWithMessage` saves an assistant message exactly as
    /// `AppendMessage` does, including mirroring its extracted memories into
    /// Letta after commit, so it needs the same adapter.
    letta_memory: Option<LettaMemoryAdapter>,
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
    // `space_id` rides along because a system-owned thread inside a Space is
    // not org-readable by id (see `authorize_thread_owner_row`).
    let owner: Option<(String, String, String)> = sqlx::query_as(
        "SELECT org_id, user_id, COALESCE(space_id, '') FROM threads WHERE id = $1",
    )
    .bind(thread_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| Status::internal(error.to_string()))?;
    let (org_id, user_id, space_id) =
        owner.ok_or_else(|| Status::not_found("thread not found"))?;
    authorize_thread_owner_row(caller, &org_id, &user_id, &space_id, intent)
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
        "DELETE FROM run_loop_checkpoints WHERE run_id IN (SELECT id FROM runs WHERE thread_id = $1)",
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
    // Absent on every decision issued without a stored-consent contract.
    #[serde(default)]
    contract_digest: Option<String>,
    #[serde(default)]
    contract_expires_at: Option<DateTime<Utc>>,
}

/// Delegated-identity design, Gap B (03-design-2-token-exchange.md §2.2/§2.5).
/// A LEAN, purpose-built claim family for exactly one RPC
/// (`prepare_user_owned_thread` when `origin == "mail_task"`) -- deliberately
/// NOT [`ControlSpaceDecisionClaims`]'s Space-authority shape, which has no
/// fields this RPC could even check (it carries no `space_id`). Field names
/// mirror user-core's `spaces.MailTaskThreadDecision` (Go) exactly; this is
/// the same signed envelope, verified here with the SAME Control Ed25519
/// keys `configured_control_space_decision_keys()` already loads for the
/// scheduled-run family -- no new key configuration.
#[derive(Debug, serde::Deserialize)]
struct MailTaskThreadDecisionClaims {
    decision_ref: String,
    org_id: String,
    user_id: String,
    alias_id: String,
    provider_message_id: String,
    session_key: String,
    origin: String,
    service_audience: String,
    action_id: String,
    nonce: String,
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

/// Verifies `req.control_decision_token` as a mail-task-v1 decision bound to
/// exactly this request's `(org_id, user_id, session_key, origin)` --
/// delegated-identity design, Gap B. Called ONLY when
/// `prepare_user_owned_thread`'s `origin == "mail_task"`; every other
/// current `USER_OWNED_THREAD_CALLERS` origin is untouched by this function
/// existing. This is the single most important check for the owner-only
/// ruling itself (security-review §4's named test): a decision minted for
/// user A must be rejected outright when the request names user B, because
/// `claims.user_id == req.user_id` is checked field-for-field below, not
/// merely "some valid decision was presented."
fn verify_mail_task_thread_decision(req: &pb::PrepareUserOwnedThreadRequest) -> Result<(), Status> {
    let keys = configured_control_space_decision_keys()?;
    verify_mail_task_thread_decision_with_keys(req, &keys, Utc::now())
}

fn verify_mail_task_thread_decision_with_keys(
    req: &pb::PrepareUserOwnedThreadRequest,
    keys: &BTreeMap<String, VerifyingKey>,
    now: DateTime<Utc>,
) -> Result<(), Status> {
    if req.control_decision_token.len() > MAX_CONTROL_SPACE_DECISION_TOKEN_BYTES {
        return Err(Status::invalid_argument(
            "Control mail-task thread decision is too large",
        ));
    }
    let parts: Vec<&str> = req.control_decision_token.split('.').collect();
    if parts.len() != 4 || parts[0] != MAIL_TASK_DECISION_VERSION {
        return Err(Status::permission_denied(
            "invalid Control mail-task thread decision envelope",
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
        Status::permission_denied("invalid Control mail-task thread decision payload")
    })?;
    let signature = URL_SAFE_NO_PAD.decode(parts[3]).map_err(|_| {
        Status::permission_denied("invalid Control mail-task thread decision signature")
    })?;
    let signature = Signature::from_slice(&signature).map_err(|_| {
        Status::permission_denied("invalid Control mail-task thread decision signature")
    })?;
    key.verify(
        format!("{}.{}.{}", parts[0], parts[1], parts[2]).as_bytes(),
        &signature,
    )
    .map_err(|_| Status::permission_denied("invalid Control mail-task thread decision signature"))?;
    let claims: MailTaskThreadDecisionClaims = serde_json::from_slice(&payload).map_err(|_| {
        Status::permission_denied("invalid Control mail-task thread decision claims")
    })?;
    // Every field the request itself carries is compared field-for-field --
    // a decision minted for one (org, user, session_key) can never be
    // pointed at a different one, and cross-family confusion is structurally
    // impossible because the envelope's version tag was already checked
    // above.
    let matches = claims.org_id == req.org_id
        && claims.user_id == req.user_id
        && claims.session_key == req.session_key
        && claims.origin == req.origin
        && claims.origin == "mail_task"
        && claims.service_audience == MAIL_TASK_THREAD_PREPARE_AUDIENCE
        && claims.action_id == MAIL_TASK_THREAD_PREPARE_ACTION
        && !claims.decision_ref.trim().is_empty()
        && !claims.alias_id.trim().is_empty()
        && !claims.provider_message_id.trim().is_empty();
    if !matches
        || claims.nonce.trim().is_empty()
        || claims.expires_at <= now
        || claims.issued_at > now + chrono::Duration::minutes(1)
    {
        return Err(Status::permission_denied(
            "Control decision does not authorize this mail-task thread",
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
    schema: &str,
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
        ("action_schema_hash", schema),
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
    if let (Some(contract_digest), Some(expires_at)) =
        (&claims.contract_digest, claims.contract_expires_at)
    {
        digest.update(b"contract_digest\0");
        digest.update((contract_digest.len() as u64).to_be_bytes());
        digest.update(contract_digest.as_bytes());
        digest.update(b"contract_expires_at\0");
        digest.update(expires_at.timestamp().to_be_bytes());
    }
    format!("sha256:{:x}", digest.finalize())
}

/// The stored-consent contract a scheduled run was created under, re-checked
/// on the direct hop. The digest must be the hash of the exact body received,
/// so a body that does not match what Control signed never reaches a run, and
/// an expired contract is refused before any decision is verified.
#[allow(clippy::result_large_err)]
fn scheduled_run_contract(
    req: &pb::StartScheduledRunRequest,
    now: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, Status> {
    if req.contract_json.is_empty()
        && req.contract_digest.is_empty()
        && req.contract_expires_at.is_empty()
    {
        return Ok(None);
    }
    if req.contract_json.len() > MAX_SCHEDULED_CONTRACT_BYTES
        || !valid_scheduled_step_digest(&req.contract_digest)
        || format!("sha256:{:x}", Sha256::digest(req.contract_json.as_bytes()))
            != req.contract_digest
    {
        return Err(Status::invalid_argument(
            "scheduled-run contract bindings are invalid",
        ));
    }
    let expires_at = DateTime::parse_from_rfc3339(&req.contract_expires_at)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| Status::invalid_argument("scheduled-run contract bindings are invalid"))?;
    if expires_at <= now {
        return Err(Status::failed_precondition("scheduled_contract_expired"));
    }
    Ok(Some(expires_at))
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
    let contract_expires_at = scheduled_run_contract(req, now)?;
    let schema = if contract_expires_at.is_some() {
        CONTROL_SCHEDULED_RUN_EXECUTION_CONTRACT_SCHEMA
    } else {
        CONTROL_SCHEDULED_RUN_EXECUTION_SCHEMA
    };
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
        && claims.action_schema_hash == schema
        && claims.contract_digest.as_deref()
            == contract_expires_at.map(|_| req.contract_digest.as_str())
        && claims.contract_expires_at == contract_expires_at
        && claims.idempotency_key == req.idempotency_key
        && claims.recipient_audience_ref == thread.recipient_audience_ref
        && claims.recipient_audience_revision == thread.recipient_audience_revision
        && claims.recipient_audience_hash == thread.recipient_audience_hash
        && claims.privacy_policy_ref == thread.privacy_policy_ref
        && claims.resource_authorization_ref == thread.resource_authorization_ref
        && claims.authority_revision == thread.authority_revision
        && claims.payload_digest == scheduled_run_execution_payload_digest(req, &claims, schema)
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

#[cfg(test)]
mod scheduled_contract_binding_tests {
    use super::*;

    fn request_with_contract(
        contract_json: &str,
        expires_at: DateTime<Utc>,
    ) -> pb::StartScheduledRunRequest {
        pb::StartScheduledRunRequest {
            contract_json: contract_json.to_owned(),
            contract_digest: format!("sha256:{:x}", Sha256::digest(contract_json.as_bytes())),
            contract_expires_at: expires_at.to_rfc3339(),
            ..Default::default()
        }
    }

    #[test]
    fn a_schedule_without_a_contract_keeps_the_legacy_binding() {
        let now = Utc::now();
        let req = pb::StartScheduledRunRequest::default();
        assert_eq!(scheduled_run_contract(&req, now).expect("legacy"), None);
    }

    #[test]
    fn a_live_contract_binds_its_expiry() {
        let now = Utc::now();
        let expires_at = now + chrono::Duration::hours(1);
        let req = request_with_contract(r#"{"v":1,"tools":[]}"#, expires_at);
        let bound = scheduled_run_contract(&req, now).expect("live contract");
        assert_eq!(bound.map(|value| value.timestamp()), Some(expires_at.timestamp()));
    }

    #[test]
    fn a_body_that_does_not_match_the_signed_digest_is_refused() {
        let now = Utc::now();
        let mut req = request_with_contract(r#"{"v":1,"tools":[]}"#, now + chrono::Duration::hours(1));
        req.contract_json = r#"{"v":1,"tools":[{"tool":"shell"}]}"#.to_owned();
        let status = scheduled_run_contract(&req, now).expect_err("forged body");
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
    }

    #[test]
    fn an_expired_contract_is_refused_before_any_decision_is_verified() {
        let now = Utc::now();
        let req = request_with_contract(r#"{"v":1,"tools":[]}"#, now - chrono::Duration::seconds(1));
        let status = scheduled_run_contract(&req, now).expect_err("expired contract");
        assert_eq!(status.message(), "scheduled_contract_expired");
    }
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

/// Complete a claimed scheduled step's receipt. With `settle_fire` (the result switch
/// is on) the fire learns the same terminal status in this transaction, so a failed
/// or unknown step no longer leaves its fire pending (build plan WS20 S1).
#[allow(clippy::result_large_err)]
async fn record_scheduled_step_receipt_inner(
    pool: &PgPool,
    req: pb::RecordScheduledStepReceiptRequest,
    settle_fire: bool,
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
    if settle_fire {
        sqlx::query(
            "UPDATE cron_fires AS cf
             SET status = $4, completed_at = now(), run_id = sr.run_id,
                 thread_id = sr.thread_id, outcome = $5
             FROM scheduled_step_receipts AS sr
             WHERE sr.run_id = $1 AND sr.step_id = $2 AND sr.org_id = $3
               AND cf.schedule_id = sr.schedule_id AND cf.fire_key = sr.fire_key
               AND cf.status IN ('pending', 'running')",
        )
        .bind(&req.run_id)
        .bind(&req.step_id)
        .bind(&req.org_id)
        .bind(scheduled_fire_status(&req.status))
        .bind(&req.status)
        .execute(&mut *tx)
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
    }
    tx.commit()
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
    Ok(Response::new(pb::RecordScheduledStepReceiptResponse {
        receipt_id,
        status: req.status,
        recorded: true,
    }))
}

/// The fire status a terminal step outcome settles to. An unknown outcome is not
/// a failure: the step may have run, so the fire says so, the same word the sweeper
/// uses for fires it cannot match (build plan WS20 S8).
fn scheduled_fire_status(step_status: &str) -> &'static str {
    match step_status {
        "completed" => "completed",
        "unknown_outcome" => "unknown",
        _ => "failed",
    }
}

/// The shape of a scheduled-step result: identifiers, a closed status that agrees
/// with the unknown-outcome flag, an answer only on a completed step, and a
/// representable withheld-effect count. Returns the count for persistence.
#[allow(clippy::result_large_err)]
fn validate_scheduled_run_result_request(
    req: &pb::RecordScheduledRunResultRequest,
) -> Result<i32, Status> {
    if !valid_scheduled_run_identifier(&req.run_id)
        || !valid_scheduled_run_identifier(&req.step_id)
        || !valid_scheduled_run_identifier(&req.org_id)
        || !valid_scheduled_run_identifier(&req.idempotency_key)
        || !valid_scheduled_run_identifier(&req.receipt_id)
    {
        return Err(Status::invalid_argument(
            "scheduled-step result bindings are invalid",
        ));
    }
    let allowed = matches!(
        req.status.as_str(),
        "completed" | "failed" | "unknown_outcome"
    );
    if !allowed || (req.unknown_outcome != (req.status == "unknown_outcome")) {
        return Err(Status::invalid_argument(
            "invalid scheduled-step result status",
        ));
    }
    if !req.output.trim().is_empty() && req.status != "completed" {
        return Err(Status::invalid_argument(
            "only a completed scheduled step carries an answer",
        ));
    }
    i32::try_from(req.withheld_effects)
        .map_err(|_| Status::invalid_argument("withheld effect count is out of range"))
}

/// Complete a claimed scheduled step with its answer (build plan WS20 S1). The
/// answer is appended to the scheduled thread, the receipt completes and the fire
/// row learns its outcome, in one transaction. Locks run and thread, then the
/// receipt, the order the other run writes use. A replay of the same terminal
/// outcome returns the message the first call appended.
#[allow(clippy::result_large_err, clippy::too_many_lines)]
async fn record_scheduled_run_result_inner(
    pool: &PgPool,
    req: pb::RecordScheduledRunResultRequest,
) -> Result<Response<pb::RecordScheduledRunResultResponse>, Status> {
    let withheld_effects = validate_scheduled_run_result_request(&req)?;

    let mut tx = pool
        .begin()
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
    let run: Option<(String, String, String, String, serde_json::Value)> = sqlx::query_as(
        "SELECT r.thread_id, r.user_id, r.status, r.retention_class, r.metadata
         FROM runs r JOIN threads t ON t.id = r.thread_id
         WHERE r.id = $1 AND r.org_id = $2
         FOR UPDATE OF r, t",
    )
    .bind(&req.run_id)
    .bind(&req.org_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| Status::internal(error.to_string()))?;
    let Some((run_thread_id, owner, run_status, retention_class, metadata)) = run else {
        return Err(Status::not_found("scheduled run not found"));
    };
    if owner != crate::auth::system_run_owners()[0]
        || metadata.get("source").and_then(serde_json::Value::as_str) != Some("scheduled_run")
    {
        return Err(Status::permission_denied(
            "scheduled step does not match the prepared run",
        ));
    }
    // A zero-retention or waived run keeps no answer (build plan WS20 S1).
    if !matches!(
        crate::retention_receipt::RetentionClass::from_storage(&retention_class),
        Some(crate::retention_receipt::RetentionClass::Persistent)
    ) {
        return Err(Status::failed_precondition(
            "a zero-retention or waived run has no durable scheduled result",
        ));
    }

    let receipt: Option<(String, String, String, String, String, String)> = sqlx::query_as(
        "SELECT receipt_id, status, idempotency_key, thread_id, schedule_id, fire_key
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
    let Some((receipt_id, current_status, idempotency_key, thread_id, schedule_id, fire_key)) =
        receipt
    else {
        return Err(Status::not_found("scheduled-step receipt not found"));
    };
    if receipt_id != req.receipt_id
        || idempotency_key != req.idempotency_key
        || thread_id != run_thread_id
    {
        return Err(Status::permission_denied(
            "scheduled-step receipt binding mismatch",
        ));
    }
    if current_status != "claimed" {
        if current_status != req.status {
            return Err(Status::already_exists(
                "scheduled-step receipt already has a different terminal outcome",
            ));
        }
        let message: Option<(String,)> = sqlx::query_as(
            "SELECT id FROM messages
             WHERE thread_id = $1 AND role = 'assistant'
               AND metadata->>'run_id' = $2 AND metadata->>'step_id' = $3
             ORDER BY sequence LIMIT 1",
        )
        .bind(&thread_id)
        .bind(&req.run_id)
        .bind(&req.step_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
        tx.commit()
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
        return Ok(Response::new(pb::RecordScheduledRunResultResponse {
            receipt_id,
            status: current_status,
            message_id: message.map(|(id,)| id).unwrap_or_default(),
        }));
    }
    if is_terminal_run_status(&run_status) {
        return Err(Status::failed_precondition(
            "scheduled run is already terminal",
        ));
    }

    let message_id = if req.status == "completed" && !req.output.trim().is_empty() {
        Some(
            append_scheduled_result_in_tx(
                &mut tx,
                &thread_id,
                &req.run_id,
                &req.step_id,
                &req.output,
            )
            .await?,
        )
    } else {
        None
    };
    let output_digest = if req.output.is_empty() {
        String::new()
    } else {
        format!(
            "sha256:{:x}",
            <sha2::Sha256 as sha2::Digest>::digest(req.output.as_bytes())
        )
    };
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
    .bind(&output_digest)
    .bind(&req.error_code)
    .bind(req.unknown_outcome)
    .execute(&mut *tx)
    .await
    .map_err(|error| Status::internal(error.to_string()))?;
    sqlx::query(
        "UPDATE cron_fires
         SET status = $3, completed_at = now(), run_id = $4, thread_id = $5,
             outcome = $6, withheld_effects = $7
         WHERE schedule_id = $1 AND fire_key = $2 AND status IN ('pending', 'running')",
    )
    .bind(&schedule_id)
    .bind(&fire_key)
    .bind(scheduled_fire_status(&req.status))
    .bind(&req.run_id)
    .bind(&thread_id)
    .bind(&req.status)
    .bind(withheld_effects)
    .execute(&mut *tx)
    .await
    .map_err(|error| Status::internal(error.to_string()))?;
    tx.commit()
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
    Ok(Response::new(pb::RecordScheduledRunResultResponse {
        receipt_id,
        status: req.status,
        message_id: message_id.unwrap_or_default(),
    }))
}

/// Append a scheduled run's answer to its thread inside the caller's transaction
/// (build plan WS20 S1). Unlike a chat append it runs no memory extraction: the
/// answer came from an unattended run and the thread is not a conversation. It
/// carries the thread's Space bindings as an assistant reply does, and writes the
/// same MESSAGE_APPENDED event so the thread's feed stays complete.
#[allow(clippy::result_large_err)]
async fn append_scheduled_result_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    thread_id: &str,
    run_id: &str,
    step_id: &str,
    content: &str,
) -> Result<String, Status> {
    let msg_id = new_ulid();
    let now = Utc::now();
    let (sequence, space_id, audience_ref, audience_revision, audience_hash, authority_revision, resource_ref): (
        i64,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<i64>,
        Option<String>,
    ) = sqlx::query_as(
        "INSERT INTO messages
         (id, thread_id, role, content, created_at, metadata, space_id, recipient_audience_ref,
          recipient_audience_revision, recipient_audience_hash, authority_revision, resource_authorization_ref)
         SELECT $1, t.id, 'assistant', $2, $3, $4, t.space_id, t.recipient_audience_ref,
                t.recipient_audience_revision, t.recipient_audience_hash, t.authority_revision, t.resource_authorization_ref
         FROM threads t WHERE t.id = $5
         RETURNING sequence, space_id, recipient_audience_ref, recipient_audience_revision,
                   recipient_audience_hash, authority_revision, resource_authorization_ref",
    )
    .bind(&msg_id)
    .bind(content)
    .bind(now)
    .bind(serde_json::json!({
        "source_scope": "scheduled_run",
        "run_id": run_id,
        "step_id": step_id,
    }))
    .bind(thread_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| Status::internal(e.to_string()))?;

    // The list's paging key moves with the result, as it does for a chat append,
    // so a scheduled answer lifts its thread to the top of the list.
    sqlx::query(
        "UPDATE threads SET last_activity_at = GREATEST(last_activity_at, $2) WHERE id = $1",
    )
    .bind(thread_id)
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(|e| Status::internal(e.to_string()))?;

    let sequence_u64 =
        u64::try_from(sequence).map_err(|_| Status::internal("message sequence out of range"))?;
    let msg_resource = format!("message:{}", &msg_id);
    let msg_idem = derive_idempotency_hash(
        "session-core",
        "MESSAGE_APPENDED",
        &msg_resource,
        &format!("{}:{}", thread_id, sequence_u64),
    );
    sqlx::query(
        "INSERT INTO events (id, event_type, run_id, payload, ts, org_id, user_id, correlation_id, causation_id, idempotency_key, resource_ref, type_url, producer, schema_version)
         SELECT $1, 'MESSAGE_APPENDED', $2, $3, $4, t.org_id, t.user_id, $5, '', $6, $7, $8, 'session-core', 1
         FROM threads t WHERE t.id = $2",
    )
    .bind(new_ulid())
    .bind(thread_id)
    .bind(serde_json::json!({
        "thread_id": thread_id,
        "message_id": &msg_id,
        "sequence": sequence_u64,
        "role": "assistant",
        "space_id": space_id,
        "recipient_audience_ref": audience_ref,
        "recipient_audience_revision": audience_revision,
        "recipient_audience_hash": audience_hash,
        "authority_revision": authority_revision,
        "resource_authorization_ref": resource_ref,
    }))
    .bind(now)
    .bind(thread_id)
    .bind(&msg_idem)
    .bind(&msg_resource)
    .bind(MESSAGE_APPENDED_TYPE_URL)
    .execute(&mut **tx)
    .await
    .map_err(|e| Status::internal(e.to_string()))?;
    Ok(msg_id)
}

/// The longest reason a learned-skill proposal keeps. The reviewer is asked for
/// at most this; the bound is enforced here because the reviewer's output is
/// untrusted and the reason is shown to the owner.
const MAX_PROPOSAL_REASON_CHARS: usize = 300;

/// Run and thread ids are provenance, not content: cap them so a hostile caller
/// cannot park arbitrary text in the row.
fn bounded_provenance(value: &str) -> String {
    value.trim().chars().take(128).collect()
}

/// A learned-skill write that did not happen, with the closed token that says
/// why. `skipped_protected` stays true for a human's row, as it was before the
/// token existed.
fn skill_write_skipped(reason: &str) -> pb::UpsertAgentSkillResponse {
    pb::UpsertAgentSkillResponse {
        id: String::new(),
        created: false,
        skipped_protected: reason == "protected",
        skip_reason: reason.to_owned(),
        revision: 0,
    }
}

/// Why a learned skill may not be written for this owner, or `None`. Their own
/// state (paused, off) and the organization's policy (memory disabled, learning
/// review not opted in, policy unreadable) both end it, with different tokens so
/// the producer can tell a person's choice from an organization's.
fn learned_skill_withheld(
    decision: &crate::memory_control::MemoryDecision,
) -> Option<&'static str> {
    use crate::memory_control::MemoryReason;
    if decision.learning_allowed {
        return None;
    }
    Some(match decision.reason {
        MemoryReason::UserPaused | MemoryReason::UserOff | MemoryReason::ThreadOff => {
            "memory_state"
        }
        MemoryReason::Zdr => "zdr",
        // `On` with learning refused is the organization not having opted in.
        MemoryReason::On | MemoryReason::OrgDisabled | MemoryReason::PolicyUnavailable => {
            "org_policy"
        }
    })
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
    let mut expected = serde_json::json!({
        "source": "scheduled_run",
        "schedule_id": req.schedule_id,
        "fire_key": req.fire_key,
        "space_id": space_id,
        "subject_id": req.human_subject_id,
        "template_digest": req.template_digest,
        "policy_digest": req.policy_digest,
        "idempotency_key": req.idempotency_key,
        "origin": RunOrigin::Schedule.as_str(),
    });
    if !req.contract_digest.is_empty() {
        let body: serde_json::Value = serde_json::from_str(&req.contract_json)
            .map_err(|_| Status::invalid_argument("scheduled-run contract body is invalid"))?;
        expected["contract_digest"] = serde_json::json!(req.contract_digest);
        expected["contract_expires_at"] = serde_json::json!(req.contract_expires_at);
        expected["contract"] = body;
    }
    let exact = [
        "schedule_id",
        "fire_key",
        "space_id",
        "subject_id",
        "template_digest",
        "policy_digest",
        "idempotency_key",
        "contract_digest",
        "contract_expires_at",
        "contract",
    ]
    .into_iter()
    .all(|key| metadata.get(key) == expected.get(key));
    // A run bound before the origin stamp existed carries none. It may take the
    // stamp, but a run already stamped with any other origin is never rebound.
    let stamped = metadata.get(ORIGIN_METADATA_KEY).is_some();
    let origin_bindable =
        !stamped || RunOrigin::from_metadata(&metadata) == Some(RunOrigin::Schedule);
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
    } else if !exact || !origin_bindable {
        return Err(Status::permission_denied(
            "scheduled run id is bound to different immutable schedule metadata",
        ));
    } else if !stamped {
        sqlx::query(
            "UPDATE runs SET metadata = metadata || jsonb_build_object($4::text, $2::text), updated_at = now()
             WHERE id = $1 AND org_id = $3",
        )
        .bind(&req.run_id)
        .bind(RunOrigin::Schedule.as_str())
        .bind(&org_id)
        .bind(ORIGIN_METADATA_KEY)
        .execute(&mut *tx)
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
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

/// What a verified Control `model.thread.read` decision admits.
///
/// Constructing one is the only way to skip [`authorize_thread_owner`] on a
/// read path, so the type itself is the boundary: a caller cannot accidentally
/// widen a query without holding proof that Control resolved this subject as a
/// current member AND current recipient of this Space.
#[derive(Debug, Clone, Copy)]
pub(crate) struct VerifiedSpaceRead {
    /// The recipient-audience revision the decision was issued under. Rows
    /// stamped with a NEWER revision are refused: they belong to an audience
    /// this reader was not part of when the decision was minted.
    pub(crate) recipient_audience_revision: i64,
}

/// Recompute Control's `model.thread.read` payload digest from the claims.
///
/// The signature already covers the whole payload, so this is not what stops a
/// forged token -- it pins the digest CONTRACT. If Control ever changes how it
/// encodes a read effect without this side changing with it, the mismatch
/// surfaces here as a denial instead of as two planes silently disagreeing
/// about what was authorized.
fn thread_read_payload_digest(claims: &ControlSpaceDecisionClaims) -> String {
    let mut digest = Sha256::new();
    digest.update(b"model.thread.read\0v1\0");
    for (name, value) in [
        ("org_id", claims.org_id.as_str()),
        ("user_id", claims.subject_id.as_str()),
        ("space_id", claims.space_ref.as_str()),
        ("space_decision_ref", claims.decision_ref.as_str()),
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
        ("action_schema_hash", CONTROL_THREAD_READ_SCHEMA),
        ("idempotency_key", claims.idempotency_key.as_str()),
    ] {
        digest.update(name.as_bytes());
        digest.update([0]);
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
    }
    digest.update(b"authority_revision\0");
    digest.update(claims.authority_revision.to_be_bytes());
    digest.update(b"recipient_audience_revision\0");
    digest.update(claims.recipient_audience_revision.to_be_bytes());
    format!("sha256:{:x}", digest.finalize())
}

#[allow(clippy::result_large_err)]
#[allow(clippy::too_many_arguments)]
fn verify_thread_read_space_decision_with_key(
    token: &str,
    decision_ref: &str,
    org_id: &str,
    user_id: &str,
    space_id: &str,
    expected_key_id: &str,
    key: &VerifyingKey,
    now: DateTime<Utc>,
) -> Result<VerifiedSpaceRead, Status> {
    if token.len() > MAX_CONTROL_SPACE_DECISION_TOKEN_BYTES {
        return Err(Status::invalid_argument(
            "Control Space read decision is too large",
        ));
    }
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 4 || parts[0] != CONTROL_SPACE_DECISION_VERSION {
        return Err(Status::permission_denied(
            "invalid Control Space read decision envelope",
        ));
    }
    let token_key_id = URL_SAFE_NO_PAD
        .decode(parts[1])
        .ok()
        .and_then(|raw| String::from_utf8(raw).ok())
        .filter(|id| id == expected_key_id)
        .ok_or_else(|| Status::permission_denied("untrusted Control Space decision key"))?;
    if token_key_id.trim().is_empty() {
        return Err(Status::permission_denied(
            "untrusted Control Space decision key",
        ));
    }
    let payload = URL_SAFE_NO_PAD
        .decode(parts[2])
        .map_err(|_| Status::permission_denied("invalid Control Space read decision payload"))?;
    let signature = URL_SAFE_NO_PAD
        .decode(parts[3])
        .map_err(|_| Status::permission_denied("invalid Control Space read decision signature"))?;
    let signature = Signature::from_slice(&signature)
        .map_err(|_| Status::permission_denied("invalid Control Space read decision signature"))?;
    key.verify(
        format!("{}.{}.{}", parts[0], parts[1], parts[2]).as_bytes(),
        &signature,
    )
    .map_err(|_| Status::permission_denied("invalid Control Space read decision signature"))?;
    let claims: ControlSpaceDecisionClaims = serde_json::from_slice(&payload)
        .map_err(|_| Status::permission_denied("invalid Control Space read decision claims"))?;

    let matches_request = claims.decision_ref == decision_ref
        && claims.org_id == org_id
        && claims.subject_id == user_id
        && claims.space_ref == space_id
        && claims.service_audience == CONTROL_SPACE_DECISION_AUDIENCE
        && claims.action_id == CONTROL_THREAD_READ_ACTION
        && claims.action_schema_hash == CONTROL_THREAD_READ_SCHEMA
        && claims.payload_digest == thread_read_payload_digest(&claims)
        && claims
            .permissions
            .iter()
            .any(|permission| permission == "thread:read");
    if !matches_request {
        return Err(Status::permission_denied(
            "Control Space decision does not authorize this conversation read",
        ));
    }
    // A read decision must never double as authority to write. Control issues
    // disjoint permission sets; this keeps that disjointness a property the
    // reader enforces rather than a promise it takes on trust.
    if claims
        .permissions
        .iter()
        .any(|permission| permission == "thread:append" || permission == "thread:create")
    {
        return Err(Status::permission_denied(
            "a Space read decision must not carry write permissions",
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
        || claims.recipient_audience_ref.trim().is_empty()
        || claims.recipient_audience_revision == 0
        || claims.issued_at > now + chrono::Duration::minutes(1)
        || claims.expires_at <= now
    {
        return Err(Status::permission_denied(
            "Control Space read decision is expired or incomplete",
        ));
    }
    let recipient_audience_revision =
        i64::try_from(claims.recipient_audience_revision).map_err(|_| {
            Status::permission_denied("Control Space read decision revision is invalid")
        })?;
    Ok(VerifiedSpaceRead {
        recipient_audience_revision,
    })
}

/// Resolve an optional shared-Space read authority for one request.
///
/// `Ok(None)` means no decision was offered and the caller stays on the
/// owner-bound path -- the historical behaviour for personal threads and for
/// every caller that has not opted in. A partially supplied decision is an
/// error rather than a silent fallback: half a token is a bug or an attempt,
/// and treating it as "no token" would hide both.
#[allow(clippy::result_large_err)]
pub(crate) fn verify_thread_read_space_decision(
    token: &str,
    decision_ref: &str,
    org_id: &str,
    user_id: &str,
    space_id: &str,
) -> Result<Option<VerifiedSpaceRead>, Status> {
    if token.trim().is_empty() && decision_ref.trim().is_empty() {
        return Ok(None);
    }
    if token.trim().is_empty() || decision_ref.trim().is_empty() || space_id.trim().is_empty() {
        return Err(Status::invalid_argument(
            "a Space read decision requires space_id, decision ref, and token together",
        ));
    }
    if user_id.trim().is_empty() {
        return Err(Status::permission_denied(
            "a Space read decision requires an authenticated user subject",
        ));
    }
    let keys = configured_control_space_decision_keys()?;
    let key_id = token
        .split('.')
        .nth(1)
        .and_then(|part| URL_SAFE_NO_PAD.decode(part).ok())
        .and_then(|raw| String::from_utf8(raw).ok())
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| Status::permission_denied("untrusted Control Space decision key"))?;
    let key = keys
        .get(&key_id)
        .ok_or_else(|| Status::permission_denied("untrusted Control Space decision key"))?;
    verify_thread_read_space_decision_with_key(
        token,
        decision_ref,
        org_id,
        user_id,
        space_id,
        &key_id,
        key,
        Utc::now(),
    )
    .map(Some)
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
const THREAD_ORIGINS: [&str; 6] = ["chat", "space", "agent_run", "support", "system", "mail_task"];

/// Trusted service principals allowed to call `prepare_user_owned_thread`
/// (chat-wave2 "schedule"/"mail-task" specs, 2026-09-28). Closed rather than
/// "any authenticated service" -- this RPC mints a thread under an arbitrary
/// user_id the caller names, which is exactly the kind of authority that must
/// never be handed to a service merely because it holds *some* credential.
///
/// Two entries, not one, and deliberately so:
/// - `service:model-gateway` -- the identity a live, user-delegated call
///   through model-gateway's ordinary gRPC ingress would carry, kept for any
///   future caller of this RPC that reuses that shared principal.
/// - `service:model-gateway-mail-task-bridge` -- the mail-task thread-prepare
///   bridge's OWN, distinct Auth Core principal (`mail_task_bridge_auth.rs`'s
///   `MailTaskBridgeTokenProvider`, default `MAIL_TASK_BRIDGE_SERVICE_ID`).
///   It is intentionally a separate principal from `service:model-gateway`
///   because the two need conflicting `retentionByAudience.session-core`
///   registry postures for the SAME audience (`zdr` for model-gateway's
///   existing terminalize/heartbeat credential vs. `persistent` for a real,
///   durably-stored thread) -- see `mail_task_bridge_auth.rs`'s module doc
///   (security-review B-4/T-6). Because it is a different Auth Core
///   `service_id`, its minted token's `sub`/`service_id` claim is a
///   different string too, so it MUST be allow-listed here in its own
///   right, or every real mail-task call fails closed with
///   `permission_denied` regardless of every registry row being correct.
const USER_OWNED_THREAD_CALLERS: [&str; 2] =
    ["service:model-gateway", "service:model-gateway-mail-task-bridge"];

/// The mail-task bridge's own principal (see [`USER_OWNED_THREAD_CALLERS`]).
const MAIL_TASK_BRIDGE_PRINCIPAL: &str = "service:model-gateway-mail-task-bridge";

/// Narrows the mail-task bridge principal to the one origin its decision gate
/// covers. `prepare_user_owned_thread` lets an allow-listed caller create a
/// thread for any user of its org, and only `mail_task` is gated on a signed
/// Control decision; without this the bridge's credential (or a compromised
/// route in front of it) could mint an ordinary `chat` thread for any user
/// with no decision at all. `service:model-gateway` keeps today's behavior.
fn authorize_user_owned_thread_origin(principal_id: &str, origin: &str) -> Result<(), Status> {
    if principal_id == MAIL_TASK_BRIDGE_PRINCIPAL && origin != "mail_task" {
        return Err(Status::permission_denied(
            "the mail-task bridge principal may only prepare mail_task-origin threads",
        ));
    }
    Ok(())
}

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
/// `author_subject_id` is the VERIFIED caller, never a request field. It is
/// recorded on user turns only: an assistant turn is attributed by its persona
/// (`agent_name`), and a system/tool turn has no human author to name. Passing
/// it in rather than reading it off the request is the point -- a client that
/// could name the author of a message could put words in someone's mouth.
async fn append_message_inner(
    pool: &PgPool,
    letta: Option<&LettaMemoryAdapter>,
    retention: MemoryRetention,
    req: pb::AppendMessageRequest,
    author_subject_id: &str,
) -> Result<Response<pb::AppendMessageResponse>, Status> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
    let appended = append_message_in_tx(&mut tx, &req, author_subject_id).await?;
    tx.commit()
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
    sync_appended_memory(pool, letta, retention, &req.thread_id, appended.memory).await;

    Ok(Response::new(pb::AppendMessageResponse {
        sequence: appended.sequence,
        message_id: appended.message_id,
        deduplicated: appended.deduplicated,
    }))
}

/// A caller's `client_message_id` has the shape the `messages` CHECK allows, so a malformed one is
/// refused as a bad argument here instead of surfacing as a database error from the CHECK.
fn valid_client_message_id(id: &str) -> bool {
    let mut bytes = id.bytes();
    bytes.next().is_some_and(|first| first.is_ascii_alphanumeric())
        && id.len() <= 128
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Memory candidates an append extracted inside its transaction, to mirror
/// into Letta only once that transaction has committed.
pub(crate) struct AppendedMemory {
    org_id: String,
    user_id: String,
    candidates: Vec<crate::dreaming::DreamMemoryCandidate>,
    persisted: Vec<crate::dreaming::PersistedMemoryId>,
}

/// One message appended inside a caller's still-open transaction.
pub(crate) struct AppendedMessage {
    pub(crate) sequence: u64,
    /// The stored row's id: the new row's, or the earlier row's when `deduplicated`.
    pub(crate) message_id: String,
    /// The caller's `client_message_id` was already stored, so nothing was written (no event, no
    /// memory extraction).
    pub(crate) deduplicated: bool,
    pub(crate) memory: Option<AppendedMemory>,
}

/// The whole durable half of an append -- thread lock, Space authority,
/// source scope, the message row, its `MESSAGE_APPENDED` event and memory
/// extraction -- inside the caller's transaction. Shared by `AppendMessage`
/// and `CompleteDirectRunWithMessage`, so an answer saved together with its
/// run's terminal receipt is stored exactly as an ordinary append stores it.
/// The caller commits, then passes `memory` to [`sync_appended_memory`].
// The body is the former `append_message_inner`, moved unchanged; splitting it
// would scatter checks that must all run under the one thread lock.
#[allow(clippy::too_many_lines)]
pub(crate) async fn append_message_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    req: &pb::AppendMessageRequest,
    author_subject_id: &str,
) -> Result<AppendedMessage, Status> {
    let author_subject_id = if req.role.trim() == "user" {
        author_subject_id.trim()
    } else {
        ""
    };
    let client_message_id = req.client_message_id.trim();
    if !client_message_id.is_empty() && !valid_client_message_id(client_message_id) {
        return Err(Status::invalid_argument(
            "client_message_id must match [A-Za-z0-9][A-Za-z0-9_-]{0,127}",
        ));
    }
    let msg_id = new_ulid();
    let now = Utc::now();
    let mut letta_sync: Option<AppendedMemory> = None;

    let has_append_context = validate_append_space_context_shape(req)?;
    let thread: (String, String, Option<String>, String) =
        sqlx::query_as("SELECT org_id, user_id, space_id, source_scope FROM threads WHERE id = $1 FOR UPDATE")
            .bind(&req.thread_id)
            .fetch_optional(&mut **tx)
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
                        .fetch_one(&mut **tx)
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
                .fetch_optional(&mut **tx)
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
                verify_append_space_decision(req, &thread.0, &thread.1)?;
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
                .execute(&mut **tx)
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
    let mut metadata_json = req
        .metadata
        .as_ref()
        .map_or_else(|| serde_json::json!({}), struct_to_json);

    // This scope is immutable after the first user message. Enforce it here,
    // under the thread lock, so alternate ingress paths cannot silently widen
    // a restricted task or relabel history that already used other sources.
    let requested_isolation = metadata_json["source_scope"] == "conversation";
    let stored_isolation = thread.3 == "conversation";
    if req.role == "user" {
        if stored_isolation && !requested_isolation {
            return Err(Status::failed_precondition("This conversation requires conversation-only source scope."));
        }
        if requested_isolation && !stored_isolation {
            let (has_history,): (bool,) = sqlx::query_as("SELECT EXISTS(SELECT 1 FROM messages WHERE thread_id = $1)")
                .bind(&req.thread_id).fetch_one(&mut **tx).await.map_err(|e| Status::internal(e.to_string()))?;
            if has_history {
                return Err(Status::failed_precondition("Start a new conversation to restrict its source scope."));
            }
            sqlx::query("UPDATE threads SET source_scope = 'conversation' WHERE id = $1")
                .bind(&req.thread_id).execute(&mut **tx).await.map_err(|e| Status::internal(e.to_string()))?;
        }
    }
    let conversation_only = stored_isolation || (req.role == "user" && requested_isolation);
    if conversation_only {
        metadata_json["source_scope"] = serde_json::json!("conversation");
        // Per-turn retrieval-source filter (chat-wave2 scope spec §2.4):
        // deliberately NOT part of the immutability check above — this axis
        // is per-turn, not thread-locked, and rides through verbatim in the
        // already-schemaless `messages.metadata` JSON via `struct_to_json`'s
        // generic conversion a few lines up, with no CHECK constraint and no
        // migration needed. The one thing that DOES need an explicit line:
        // `conversation_only` already excludes both source families
        // outright, so a stray `retrieval_scope` the caller also sent must
        // be dropped rather than persisted — it must never contradict the
        // turn's own `source_scope` in stored history.
        metadata_json.as_object_mut().map(|map| map.remove("retrieval_scope"));
    }

    // A repeat of a send the thread already holds (the caller's reply was lost, or it sends as a
    // new turn a mid-run message the run never took) is answered with the first row and writes
    // nothing: no second row, event or memory extraction. Looked up under the thread lock and
    // after every authority check above, so a replay is held to the same checks as the first
    // send and two concurrent repeats serialise here; the unique index is only the backstop.
    // Compared trimmed: the queue path stores the trimmed text and the resend carries the
    // composer's own.
    if !client_message_id.is_empty() {
        let existing: Option<(String, i64, String, String)> = sqlx::query_as(
            "SELECT id, sequence, role, content FROM messages
             WHERE thread_id = $1 AND client_message_id = $2",
        )
        .bind(&req.thread_id)
        .bind(client_message_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
        if let Some((existing_id, existing_sequence, existing_role, existing_content)) = existing {
            if existing_role != req.role || existing_content.trim() != req.content.trim() {
                return Err(Status::already_exists(
                    "client_message_id already names a different message in this thread",
                ));
            }
            return Ok(AppendedMessage {
                sequence: u64::try_from(existing_sequence)
                    .map_err(|_| Status::internal("message sequence out of range"))?,
                message_id: existing_id,
                deduplicated: true,
                memory: None,
            });
        }
    }

    let row: (i64, Option<String>, Option<String>, Option<i64>, Option<String>, Option<i64>, Option<String>) = sqlx::query_as(
        "INSERT INTO messages
         (id, thread_id, role, content, created_at, agent_name, metadata, space_id, recipient_audience_ref,
          recipient_audience_revision, recipient_audience_hash, authority_revision, resource_authorization_ref,
          author_subject_id)
         SELECT $1, t.id, $3, $4, $5, NULLIF($6, ''), $7, t.space_id, t.recipient_audience_ref,
                t.recipient_audience_revision, t.recipient_audience_hash, t.authority_revision, t.resource_authorization_ref,
                NULLIF($8, '')
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
    .bind(author_subject_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| Status::internal(e.to_string()))?;

    // A separate statement, and only when an id came with the send, so a send without one never
    // touches the column (the same INSERT as before the column existed).
    if !client_message_id.is_empty() {
        sqlx::query("UPDATE messages SET client_message_id = $2 WHERE id = $1")
            .bind(&msg_id)
            .bind(client_message_id)
            .execute(&mut **tx)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
    }

    // The list's paging key, written in the same transaction as the message so
    // the cursor moves exactly when the message does. Non-owner Space appends
    // come through here too.
    sqlx::query(
        "UPDATE threads SET last_activity_at = GREATEST(last_activity_at, $2) WHERE id = $1",
    )
    .bind(&req.thread_id)
    .bind(now)
    .execute(&mut **tx)
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
    .execute(&mut **tx)
    .await
    .map_err(|e| Status::internal(e.to_string()))?;

    let candidates = if conversation_only { Vec::new() } else {
        crate::dreaming::extract_memory_candidates(&req.role, &req.content, &req.thread_id)
    };
    // A paused or off user, or a thread set off, learns nothing from this turn:
    // no candidate is persisted and none reaches the Letta mirror. Asked under the
    // user's row lock, which `persist_candidates` takes again, so the pause cannot
    // land between the two.
    let candidates = if candidates.is_empty() {
        candidates
    } else if crate::memory_control::resolve_in_tx_for_thread(
        tx,
        false,
        &thread.0,
        &thread.1,
        &req.thread_id,
    )
        .await
        .map_err(|e| Status::internal(e.to_string()))?
        .generation_allowed
    {
        candidates
    } else {
        Vec::new()
    };
    if !candidates.is_empty() {
        let (org_id, user_id): (String, String) =
            sqlx::query_as("SELECT org_id, user_id FROM threads WHERE id = $1")
                .bind(&req.thread_id)
                .fetch_one(&mut **tx)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
        let (saved, persisted) = crate::dreaming::persist_candidates(
            tx,
            &org_id,
            &user_id,
            &req.thread_id,
            &msg_id,
            &candidates,
        )
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
        crate::dreaming::record_dream_run(
            tx,
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
        letta_sync = Some(AppendedMemory {
            org_id,
            user_id,
            candidates,
            persisted,
        });
    }

    Ok(AppendedMessage {
        sequence,
        message_id: msg_id,
        deduplicated: false,
        memory: letta_sync,
    })
}

/// Mirror an append's extracted memories into Letta after its transaction
/// committed, unless the retention posture forbids external memory indexing.
pub(crate) async fn sync_appended_memory(
    pool: &PgPool,
    letta: Option<&LettaMemoryAdapter>,
    retention: MemoryRetention,
    thread_id: &str,
    memory: Option<AppendedMemory>,
) {
    if let Some(memory) = memory {
        if retention.permits_external_memory_indexing() {
            crate::dreaming::sync_persisted_candidates_to_letta(
                pool,
                letta,
                &memory.org_id,
                &memory.user_id,
                thread_id,
                &memory.candidates,
                &memory.persisted,
            )
            .await;
        }
    }
}

#[cfg(test)]
mod source_scope_db_tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn source_scope_survives_storage_and_blocks_widening_and_learning() {
        let pool = PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required")).await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let suffix = new_ulid();
        let org = format!("scope-test-{suffix}");
        let make_thread = |key: String| pb::CreateThreadRequest { session_key: key, org_id: org.clone(), user_id: "test-user".into(), ..Default::default() };
        let isolated = create_thread_inner(&pool, make_thread(format!("isolated-{suffix}"))).await.unwrap().into_inner().thread_id;
        let ordinary = create_thread_inner(&pool, make_thread(format!("ordinary-{suffix}"))).await.unwrap().into_inner().thread_id;
        let request = |thread: &str, restricted: bool| pb::AppendMessageRequest {
            thread_id: thread.to_owned(), role: "user".into(), content: "Husk at demokoden er scope-test-amber.".into(),
            metadata: restricted.then(|| prost_types::Struct { fields: [("source_scope".into(), prost_types::Value { kind: Some(prost_types::value::Kind::StringValue("conversation".into())) })].into() }),
            ..Default::default()
        };
        append_message_inner(&pool, None, MemoryRetention::Durable, request(&isolated, true), "test-user").await.unwrap();
        let widened = append_message_inner(&pool, None, MemoryRetention::Durable, request(&isolated, false), "test-user").await.unwrap_err();
        assert_eq!(widened.code(), tonic::Code::FailedPrecondition);
        let (scope,): (String,) = sqlx::query_as("SELECT source_scope FROM threads WHERE id = $1").bind(&isolated).fetch_one(&pool).await.unwrap();
        assert_eq!(scope, "conversation");
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM messages WHERE thread_id = $1").bind(&isolated).fetch_one(&pool).await.unwrap();
        assert_eq!(count, 1, "rejected widening must not append content");
        crate::dreaming::dream_once(&pool, None, None, None, 1000).await.unwrap();
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM agent_memory WHERE org_id = $1").bind(&org).fetch_one(&pool).await.unwrap();
        assert_eq!(count, 0, "isolated text must not produce foreground or background memory");
        append_message_inner(&pool, None, MemoryRetention::Durable, request(&ordinary, false), "test-user").await.unwrap();
        let relabelled = append_message_inner(&pool, None, MemoryRetention::Durable, request(&ordinary, true), "test-user").await.unwrap_err();
        assert_eq!(relabelled.code(), tonic::Code::FailedPrecondition);
    }

    /// chat-wave2 scope spec §2.4/§2.1: `retrieval_scope` is deliberately NOT
    /// part of the immutability check above — it is a per-turn axis, not a
    /// thread-locked privacy commitment like `source_scope`. Contrasts
    /// directly with `source_scope_survives_storage_and_blocks_widening_and_learning`:
    /// the same two-messages-on-one-thread shape, but the second message's
    /// different value is accepted rather than rejected with
    /// `FailedPrecondition`, and each message's own row keeps the value it
    /// was written with (no retroactive relabeling, no shared thread-level
    /// column the way `threads.source_scope` is).
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn retrieval_scope_can_differ_turn_to_turn_while_source_scope_stays_locked() {
        let pool = PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required")).await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let suffix = new_ulid();
        let org = format!("retrieval-scope-test-{suffix}");
        let thread = create_thread_inner(&pool, pb::CreateThreadRequest {
            session_key: format!("retrieval-scope-{suffix}"), org_id: org.clone(), user_id: "test-user".into(), ..Default::default()
        }).await.unwrap().into_inner().thread_id;
        let request = |scope: &str| pb::AppendMessageRequest {
            thread_id: thread.clone(), role: "user".into(), content: format!("Turn under {scope}."),
            metadata: Some(prost_types::Struct { fields: [("retrieval_scope".into(), prost_types::Value { kind: Some(prost_types::value::Kind::StringValue(scope.into())) })].into() }),
            ..Default::default()
        };
        append_message_inner(&pool, None, MemoryRetention::Durable, request("knowledge_only"), "test-user").await.unwrap();
        // A different value on the SECOND message of the same thread must not
        // be rejected the way a widened source_scope is above.
        append_message_inner(&pool, None, MemoryRetention::Durable, request("web_only"), "test-user").await.unwrap();
        let scopes: Vec<String> = sqlx::query_scalar(
            "SELECT metadata->>'retrieval_scope' FROM messages WHERE thread_id = $1 ORDER BY created_at",
        )
        .bind(&thread)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(scopes, vec!["knowledge_only".to_owned(), "web_only".to_owned()], "each turn keeps its own scope — no thread-level lock, no retroactive relabeling");
        // The thread-level column that DOES lock (`source_scope`) is
        // unaffected — still the default, never touched by this axis.
        let (source_scope,): (String,) = sqlx::query_as("SELECT source_scope FROM threads WHERE id = $1").bind(&thread).fetch_one(&pool).await.unwrap();
        assert_eq!(source_scope, "workspace");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn forgotten_memory_cannot_return_from_pending_or_inflight_extraction() {
        let pool = PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required")).await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let org = format!("forget-test-{}", new_ulid());
        let user = "forget-user";
        let thread = create_thread_inner(&pool, pb::CreateThreadRequest {
            session_key: org.clone(), org_id: org.clone(), user_id: user.into(), ..Default::default()
        }).await.unwrap().into_inner().thread_id;
        let content = "Husk at minnetest-regresjon er ravgul.";
        append_message_inner(&pool, None, MemoryRetention::Durable, pb::AppendMessageRequest {
            thread_id: thread.clone(), role: "user".into(), content: content.into(), ..Default::default()
        }, user).await.unwrap();
        let id: String = sqlx::query_scalar("SELECT id FROM agent_memory WHERE org_id = $1 AND scope = 'user'")
            .bind(&org).fetch_one(&pool).await.unwrap();
        let message_id: String = sqlx::query_scalar("SELECT id FROM messages WHERE thread_id = $1")
            .bind(&thread).fetch_one(&pool).await.unwrap();
        let mut candidates = crate::dreaming::extract_memory_candidates("user", content, &thread);
        assert!(!candidates.is_empty());
        assert!(crate::dreaming::delete_user_memory(&pool, &org, user, &id).await.unwrap().deleted());
        // Simulate an LLM result that was already in flight when deletion won.
        // A different generated slot must not bypass the message cutoff.
        candidates[0].key = "user:llm:another_slot".into();
        let mut tx = pool.begin().await.unwrap();
        let (saved, _) = crate::dreaming::persist_candidates(&mut tx, &org, user, &thread, &message_id, &candidates).await.unwrap();
        assert_eq!(saved, 0);
        tx.commit().await.unwrap();
        crate::dreaming::dream_once(&pool, None, None, None, 1000).await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_memory WHERE org_id = $1")
            .bind(&org).fetch_one(&pool).await.unwrap();
        assert_eq!(count, 0, "the pending original message cannot recreate deleted memory");
        let entry = pb::MemoryEntry { memory_id: id.clone(), content: content.into(), ..Default::default() };
        let mut mirror = vec![entry.clone()];
        crate::memory_control::filter_forgotten(&pool, &org, user, &mut mirror).await.unwrap();
        assert!(mirror.is_empty(), "a late semantic mirror cannot surface a forgotten ID");
        let mut foreign = vec![entry];
        crate::memory_control::filter_forgotten(&pool, "another-org", user, &mut foreign).await.unwrap();
        assert_eq!(foreign.len(), 1, "forgetting is tenant-scoped");
        append_message_inner(&pool, None, MemoryRetention::Durable, pb::AppendMessageRequest {
            thread_id: thread, role: "user".into(), content: "Husk at minnetest-regresjon er sjøgrønn.".into(), ..Default::default()
        }, user).await.unwrap();
        let contents: Vec<String> = sqlx::query_scalar("SELECT content FROM agent_memory WHERE org_id = $1")
            .bind(&org).fetch_all(&pool).await.unwrap();
        assert!(contents.iter().any(|value| value.contains("sjøgrønn")), "new explicit learning remains available");
        assert!(contents.iter().all(|value| !value.contains("ravgul")));
    }
}

/// Durable send receipts (WS04 S6): `AppendMessage` with a `client_message_id`.
#[cfg(test)]
mod send_receipt_tests {
    use super::*;
    use mp_contracts::model_plane::v1::session_core_server::SessionCore;

    #[test]
    fn a_client_message_id_has_the_shape_the_messages_check_allows() {
        for good in ["a", "0", "msg_01-X", &"a".repeat(128), "Z9_-_-"] {
            assert!(valid_client_message_id(good), "{good}");
        }
        for bad in ["", "-a", "_a", "a b", "a.b", "a/b", "é", &"a".repeat(129), "a\n"] {
            assert!(!valid_client_message_id(bad), "{bad:?}");
        }
    }

    /// A zero-retention credential cannot append at all, so no message row and no client id of a
    /// zero-retention turn can exist. Served through the trait method: the refusal comes before
    /// any thread lookup (the pool goes nowhere) and is the closed durable-write refusal, never a
    /// dedupe answer.
    #[tokio::test]
    async fn a_zdr_credential_cannot_reach_append_message_with_a_client_id() {
        let service = SessionService {
            pool: sqlx::postgres::PgPoolOptions::new()
                .acquire_timeout(std::time::Duration::from_millis(250))
                .connect_lazy("postgresql://session-core@127.0.0.1:1/session_core")
                .expect("lazy pool"),
            retrieval_client: None,
            graph_client: None,
            knowledge_client: None,
            letta_memory: None,
            audit_publisher: None,
            auth: None,
            grant_columns: crate::grpc::ThreadGrantColumns::present(),
        };
        let mut request = Request::new(pb::AppendMessageRequest {
            thread_id: "thread-1".into(),
            role: "user".into(),
            content: "hello".into(),
            client_message_id: "msg-1".into(),
            ..Default::default()
        });
        request
            .extensions_mut()
            .insert(VerifiedIdentity::user_for_test_with_zdr("org-1", "u1", true));
        let refused = service
            .append_message(request)
            .await
            .expect_err("a zero-retention credential writes nothing");
        assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
    }

    async fn connect() -> PgPool {
        let pool = PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required"))
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    async fn thread(pool: &PgPool, org: &str, key: &str) -> String {
        create_thread_inner(
            pool,
            pb::CreateThreadRequest {
                session_key: key.into(),
                org_id: org.into(),
                user_id: "test-user".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .into_inner()
        .thread_id
    }

    fn send(thread: &str, role: &str, content: &str, id: &str) -> pb::AppendMessageRequest {
        pb::AppendMessageRequest {
            thread_id: thread.into(),
            role: role.into(),
            content: content.into(),
            client_message_id: id.into(),
            ..Default::default()
        }
    }

    async fn append(pool: &PgPool, req: pb::AppendMessageRequest) -> Result<pb::AppendMessageResponse, Status> {
        append_message_inner(pool, None, MemoryRetention::Durable, req, "test-user")
            .await
            .map(Response::into_inner)
    }

    async fn count(pool: &PgPool, sql: &str, bind: &str) -> i64 {
        sqlx::query_scalar(sql).bind(bind).fetch_one(pool).await.unwrap()
    }

    /// The plan's receipt test: a repeat of one id is one row, one `MESSAGE_APPENDED` event and
    /// one memory extraction, and answers with the first row's sequence and id.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn a_duplicate_send_is_one_row_one_event_and_one_memory_extraction() {
        let pool = connect().await;
        let suffix = new_ulid();
        let org = format!("receipt-test-{suffix}");
        let thread = thread(&pool, &org, &format!("receipt-{suffix}")).await;
        // The extractor learns from "Husk at ...": a second extraction would be a second memory row.
        let said = "Husk at kvitteringstesten er ravgul.";

        let first = append(&pool, send(&thread, "user", said, "msg-1")).await.unwrap();
        // The queue path stores the trimmed text and the resend carries the composer's own.
        let again = append(&pool, send(&thread, "user", &format!("  {said}\n"), "msg-1")).await.unwrap();

        assert!(!first.deduplicated && !first.message_id.is_empty());
        assert!(again.deduplicated);
        assert_eq!((again.sequence, &again.message_id), (first.sequence, &first.message_id));
        let rows = count(&pool, "SELECT COUNT(*) FROM messages WHERE thread_id = $1", &thread).await;
        assert_eq!(rows, 1, "one transcript row");
        let events = count(
            &pool,
            "SELECT COUNT(*) FROM events WHERE event_type = 'MESSAGE_APPENDED' AND run_id = $1",
            &thread,
        )
        .await;
        assert_eq!(events, 1, "one MESSAGE_APPENDED event");
        let memories = count(&pool, "SELECT COUNT(*) FROM agent_memory WHERE org_id = $1", &org).await;
        assert_eq!(memories, 1, "one memory extraction");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn the_same_id_with_other_content_or_role_is_a_conflict_and_writes_nothing() {
        let pool = connect().await;
        let suffix = new_ulid();
        let thread = thread(&pool, &format!("receipt-test-{suffix}"), &format!("conflict-{suffix}")).await;
        append(&pool, send(&thread, "user", "first words", "msg-1")).await.unwrap();

        for other in [send(&thread, "user", "other words", "msg-1"), send(&thread, "assistant", "first words", "msg-1")] {
            let refused = append(&pool, other).await.unwrap_err();
            assert_eq!(refused.code(), tonic::Code::AlreadyExists);
        }
        let rows = count(&pool, "SELECT COUNT(*) FROM messages WHERE thread_id = $1", &thread).await;
        assert_eq!(rows, 1);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn an_id_names_a_message_within_its_thread_and_a_send_without_one_is_never_deduplicated() {
        let pool = connect().await;
        let suffix = new_ulid();
        let org = format!("receipt-test-{suffix}");
        let (one, two) = (
            thread(&pool, &org, &format!("one-{suffix}")).await,
            thread(&pool, &org, &format!("two-{suffix}")).await,
        );
        let a = append(&pool, send(&one, "user", "same words", "msg-1")).await.unwrap();
        let b = append(&pool, send(&two, "user", "same words", "msg-1")).await.unwrap();
        assert!(!a.deduplicated && !b.deduplicated, "the same id on another thread is another message");
        assert_ne!(a.message_id, b.message_id);

        // No id: today's behaviour, two identical sends are two rows.
        append(&pool, send(&one, "user", "no id", "")).await.unwrap();
        let again = append(&pool, send(&one, "user", "no id", "")).await.unwrap();
        assert!(!again.deduplicated);
        let rows = count(&pool, "SELECT COUNT(*) FROM messages WHERE thread_id = $1", &one).await;
        assert_eq!(rows, 3);

        let malformed = append(&pool, send(&one, "user", "x", "has space")).await.unwrap_err();
        assert_eq!(malformed.code(), tonic::Code::InvalidArgument);
    }

    /// Concurrent repeats serialise on the thread lock: one writes, the rest are told it exists.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn concurrent_duplicates_serialise_on_the_thread_lock() {
        let pool = connect().await;
        let suffix = new_ulid();
        let thread = thread(&pool, &format!("receipt-test-{suffix}"), &format!("race-{suffix}")).await;
        let attempts = (0..8).map(|_| append(&pool, send(&thread, "user", "racing words", "msg-race")));
        let answers = futures::future::join_all(attempts).await;

        let answers: Vec<_> = answers.into_iter().map(|answer| answer.expect("every attempt is answered")).collect();
        assert_eq!(answers.iter().filter(|answer| !answer.deduplicated).count(), 1, "one writer");
        assert!(answers.iter().all(|answer| answer.message_id == answers[0].message_id));
        let rows = count(&pool, "SELECT COUNT(*) FROM messages WHERE thread_id = $1", &thread).await;
        assert_eq!(rows, 1);
    }
}

/// The `ListToolActions` trait method itself (not only `list_tool_actions_inner`) is wired:
/// it reads the caller's identity, refuses before any lookup and records its metrics.
#[cfg(test)]
mod list_tool_actions_wiring_tests {
    use super::*;
    use metrics::{
        Counter, CounterFn, Gauge, Histogram, HistogramFn, Key, KeyName, Metadata, Recorder,
        SharedString, Unit,
    };
    use mp_contracts::model_plane::v1::session_core_server::SessionCore;
    use std::sync::{Arc, Mutex};

    /// Every counter increment and histogram record, keyed `name{label=value,...}`.
    #[derive(Default)]
    struct Seen {
        counters: Arc<Mutex<Vec<String>>>,
        histograms: Arc<Mutex<Vec<String>>>,
    }

    struct Tap {
        key: String,
        into: Arc<Mutex<Vec<String>>>,
    }

    impl CounterFn for Tap {
        fn increment(&self, _value: u64) {
            self.into.lock().expect("tap").push(self.key.clone());
        }
        fn absolute(&self, _value: u64) {}
    }

    impl HistogramFn for Tap {
        fn record(&self, _value: f64) {
            self.into.lock().expect("tap").push(self.key.clone());
        }
    }

    fn rendered(key: &Key) -> String {
        let mut labels: Vec<String> = key
            .labels()
            .map(|label| format!("{}={}", label.key(), label.value()))
            .collect();
        labels.sort();
        format!("{}{{{}}}", key.name(), labels.join(","))
    }

    impl Recorder for Seen {
        fn describe_counter(&self, _key: KeyName, _unit: Option<Unit>, _description: SharedString) {}
        fn describe_gauge(&self, _key: KeyName, _unit: Option<Unit>, _description: SharedString) {}
        fn describe_histogram(&self, _key: KeyName, _unit: Option<Unit>, _description: SharedString) {}
        fn register_counter(&self, key: &Key, _metadata: &Metadata<'_>) -> Counter {
            Counter::from_arc(Arc::new(Tap {
                key: rendered(key),
                into: Arc::clone(&self.counters),
            }))
        }
        fn register_gauge(&self, _key: &Key, _metadata: &Metadata<'_>) -> Gauge {
            Gauge::noop()
        }
        fn register_histogram(&self, key: &Key, _metadata: &Metadata<'_>) -> Histogram {
            Histogram::from_arc(Arc::new(Tap {
                key: rendered(key),
                into: Arc::clone(&self.histograms),
            }))
        }
    }

    /// A service whose pool goes nowhere: every case below is refused before a lookup.
    fn service() -> SessionService {
        SessionService {
            pool: sqlx::postgres::PgPoolOptions::new()
                .acquire_timeout(std::time::Duration::from_millis(250))
                .connect_lazy("postgresql://session-core@127.0.0.1:1/session_core")
                .expect("lazy pool"),
            retrieval_client: None,
            graph_client: None,
            knowledge_client: None,
            letta_memory: None,
            audit_publisher: None,
            auth: None,
            grant_columns: crate::grpc::ThreadGrantColumns::present(),
        }
    }

    fn request(caller: Option<VerifiedIdentity>) -> Request<pb::ListToolActionsRequest> {
        let mut request = Request::new(pb::ListToolActionsRequest {
            run_id: "run-1".to_owned(),
            action_ids: vec!["act-1".to_owned()],
        });
        if let Some(caller) = caller {
            request.extensions_mut().insert(caller);
        }
        request
    }

    #[tokio::test]
    async fn the_method_needs_a_verified_identity_and_counts_the_refusal() {
        let seen = Seen::default();
        let _guard = metrics::set_default_local_recorder(&seen);
        let refused = service()
            .list_tool_actions(request(None))
            .await
            .expect_err("no identity");
        assert_eq!(refused.code(), tonic::Code::Unauthenticated);
        let counters = seen.counters.lock().expect("counters").clone();
        assert_eq!(
            counters,
            vec!["mp_session_grpc_requests_total{method=list_tool_actions,status=error}".to_owned()]
        );
        let histograms = seen.histograms.lock().expect("histograms").clone();
        assert_eq!(
            histograms,
            vec!["mp_session_grpc_request_duration_seconds{method=list_tool_actions}".to_owned()]
        );
    }

    #[tokio::test]
    async fn the_method_applies_the_callers_own_refusals_before_any_lookup() {
        let service = service();
        // A service without the read scope.
        let writer = VerifiedIdentity::service_for_test("org-1", &["session:write"], false);
        assert_eq!(
            service
                .list_tool_actions(request(Some(writer)))
                .await
                .expect_err("no session:read")
                .code(),
            tonic::Code::PermissionDenied
        );
        // A zero-retention human credential: the durable-mutation refusal, never a lookup.
        let zdr = VerifiedIdentity::user_for_test_with_zdr("org-1", "u1", true);
        let refused = service
            .list_tool_actions(request(Some(zdr)))
            .await
            .expect_err("a zdr human");
        let closed = Refusal::DURABLE_MUTATION_FORBIDDEN.grpc_status(tonic::Code::FailedPrecondition);
        assert_eq!((refused.code(), refused.message()), (closed.code(), closed.message()));
    }

    /// A caller that passes every check before the switch (the read scope, a persistent
    /// credential), so a call goes as far as the switch lets it.
    fn owner() -> VerifiedIdentity {
        VerifiedIdentity::user_for_test("org-1", "u1")
    }

    /// The switch refusal, served through the trait method itself. The switch is forced for this
    /// test's own thread (see `loop_checkpoint::test_switch`), so no process environment is set
    /// and no other test is raced. With the switch off the answer is the closed refusal and the
    /// pool, which goes nowhere, is never asked; with it on the call reaches the run lookup,
    /// which cannot connect, and answers `Internal`. A trait method that ignored the switch, or
    /// passed a constant to the handler, would fail one of the two halves.
    #[tokio::test]
    async fn the_method_answers_the_store_switch_it_reads_not_a_constant() {
        let closed = loop_checkpoint::disabled();
        {
            let _off = loop_checkpoint::test_switch::Forced::set(false);
            let refused = service()
                .list_tool_actions(request(Some(owner())))
                .await
                .expect_err("the store is off");
            assert_eq!((refused.code(), refused.message()), (closed.code(), closed.message()));
        }
        {
            let _on = loop_checkpoint::test_switch::Forced::set(true);
            let reached = service()
                .list_tool_actions(request(Some(owner())))
                .await
                .expect_err("the lookup cannot connect");
            assert_eq!(
                reached.code(),
                tonic::Code::Internal,
                "with the store on the call passes the switch and reaches the run lookup: {reached:?}"
            );
        }
    }

    /// The same, for the two fold RPCs that carry the switch (the close RPC has none: it only
    /// ever lets a loop that finished say so).
    #[tokio::test]
    async fn the_fold_rpcs_answer_the_store_switch_they_read_not_a_constant() {
        let closed = loop_checkpoint::disabled();
        let save = || {
            Request::new(pb::SaveLoopCheckpointRequest {
                run_id: "run-1".to_owned(),
                ..Default::default()
            })
        };
        let claim = || {
            Request::new(pb::ClaimLoopCheckpointRequest {
                run_id: "run-1".to_owned(),
                ..Default::default()
            })
        };
        fn with_caller<T>(mut request: Request<T>) -> Request<T> {
            request.extensions_mut().insert(owner());
            request
        }
        {
            let _off = loop_checkpoint::test_switch::Forced::set(false);
            let refused = service()
                .save_loop_checkpoint(with_caller(save()))
                .await
                .expect_err("the store is off");
            assert_eq!((refused.code(), refused.message()), (closed.code(), closed.message()));
            let refused = service()
                .claim_loop_checkpoint(with_caller(claim()))
                .await
                .expect_err("the store is off");
            assert_eq!((refused.code(), refused.message()), (closed.code(), closed.message()));
        }
        {
            let _on = loop_checkpoint::test_switch::Forced::set(true);
            let reached = service()
                .save_loop_checkpoint(with_caller(save()))
                .await
                .expect_err("the lookup cannot connect");
            assert_eq!(reached.code(), tonic::Code::Internal, "{reached:?}");
            let reached = service()
                .claim_loop_checkpoint(with_caller(claim()))
                .await
                .expect_err("the lookup cannot connect");
            assert_eq!(reached.code(), tonic::Code::Internal, "{reached:?}");
        }
    }
}

#[cfg(test)]
mod persona_db_tests {
    use super::*;

    fn test_service(pool: PgPool) -> SessionService {
        SessionService {
            pool,
            retrieval_client: None,
            graph_client: None,
            knowledge_client: None,
            letta_memory: None,
            audit_publisher: None,
            auth: None,
            grant_columns: crate::grpc::ThreadGrantColumns::present(),
        }
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn get_context_assembly_reports_persona_enabled_from_the_threads_row() {
        let pool = PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required")).await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let suffix = new_ulid();
        let org = format!("persona-assembly-test-{suffix}");
        let thread = create_thread_inner(&pool, pb::CreateThreadRequest {
            session_key: format!("persona-assembly-{suffix}"), org_id: org.clone(), user_id: "test-user".into(), ..Default::default()
        }).await.unwrap().into_inner().thread_id;

        // Freshly created: the migration's column default applies, so the
        // very first read already reports enabled without any write.
        let enabled = get_context_assembly_inner(
            &test_service(pool.clone()),
            pb::GetContextAssemblyRequest { thread_id: thread.clone(), ..Default::default() },
            MemoryRetention::Durable,
            None,
        ).await.unwrap().into_inner().persona_enabled;
        assert_eq!(enabled, Some(true));

        sqlx::query("UPDATE threads SET persona_enabled = false WHERE id = $1")
            .bind(&thread)
            .execute(&pool)
            .await
            .unwrap();

        let disabled = get_context_assembly_inner(
            &test_service(pool.clone()),
            pb::GetContextAssemblyRequest { thread_id: thread.clone(), ..Default::default() },
            MemoryRetention::Durable,
            None,
        ).await.unwrap().into_inner().persona_enabled;
        assert_eq!(disabled, Some(false));
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn update_thread_presentation_toggles_persona_enabled_independent_of_pin_and_title() {
        let pool = PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required")).await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let suffix = new_ulid();
        let org = format!("persona-toggle-test-{suffix}");
        let thread = create_thread_inner(&pool, pb::CreateThreadRequest {
            session_key: format!("persona-toggle-{suffix}"), org_id: org.clone(), user_id: "test-user".into(), ..Default::default()
        }).await.unwrap().into_inner().thread_id;
        let svc = test_service(pool.clone());

        let mut request = Request::new(pb::UpdateThreadPresentationRequest {
            org_id: org.clone(),
            thread_id: thread.clone(),
            persona_enabled: Some(false),
            ..Default::default()
        });
        request.extensions_mut().insert(VerifiedIdentity::user_for_test(&org, "test-user"));
        svc.update_thread_presentation(request).await.expect("persona-only update");

        let (persona_enabled, title, pinned_at): (bool, Option<String>, Option<DateTime<Utc>>) =
            sqlx::query_as("SELECT persona_enabled, presentation_title, pinned_at FROM threads WHERE id = $1")
                .bind(&thread)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(!persona_enabled, "the toggle must take effect");
        assert!(title.is_none(), "a persona-only write must not touch the title");
        assert!(pinned_at.is_none(), "a persona-only write must not touch the pin");

        // Flipping pin/title afterward must not silently revert persona back on.
        let mut pin_request = Request::new(pb::UpdateThreadPresentationRequest {
            org_id: org.clone(),
            thread_id: thread.clone(),
            pinned: Some(true),
            ..Default::default()
        });
        pin_request.extensions_mut().insert(VerifiedIdentity::user_for_test(&org, "test-user"));
        svc.update_thread_presentation(pin_request).await.expect("pin-only update");
        let (persona_enabled,): (bool,) =
            sqlx::query_as("SELECT persona_enabled FROM threads WHERE id = $1")
                .bind(&thread)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(!persona_enabled, "an unrelated presentation write must leave persona as the caller last set it");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn update_thread_presentation_lets_a_zdr_owner_set_the_controls_but_not_persist_text() {
        let pool = PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required")).await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let suffix = new_ulid();
        let org = format!("persona-zdr-test-{suffix}");
        let thread = create_thread_inner(&pool, pb::CreateThreadRequest {
            session_key: format!("persona-zdr-{suffix}"), org_id: org.clone(), user_id: "test-user".into(), ..Default::default()
        }).await.unwrap().into_inner().thread_id;
        let svc = test_service(pool.clone());

        let mut request = Request::new(pb::UpdateThreadPresentationRequest {
            org_id: org.clone(),
            thread_id: thread.clone(),
            persona_enabled: Some(false),
            ..Default::default()
        });
        request
            .extensions_mut()
            .insert(VerifiedIdentity::user_for_test_with_zdr(
                &org,
                "test-user",
                true,
            ));
        // The persona control is the owner's under every posture (owner decision 17).
        svc.update_thread_presentation(request)
            .await
            .expect("a ZDR owner may set the persona control");
        let (persona_enabled,): (bool,) =
            sqlx::query_as("SELECT persona_enabled FROM threads WHERE id = $1")
                .bind(&thread)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(!persona_enabled);

        // Conversation text is never persisted under ZDR: a title is refused as a durable mutation, typed,
        // and the title column stays as it was.
        let mut text = Request::new(pb::UpdateThreadPresentationRequest {
            org_id: org.clone(),
            thread_id: thread.clone(),
            title: Some("a title".into()),
            ..Default::default()
        });
        text.extensions_mut()
            .insert(VerifiedIdentity::user_for_test_with_zdr(
                &org,
                "test-user",
                true,
            ));
        let error = svc
            .update_thread_presentation(text)
            .await
            .expect_err("a ZDR owner may not persist a title");
        assert_eq!(error.code(), tonic::Code::FailedPrecondition);
        assert_eq!(
            mp_contracts::zdr_refusal::Refusal::from_status(&error).map(|refusal| refusal.code()),
            Some(mp_contracts::zdr_refusal::code::DURABLE_MUTATION_FORBIDDEN)
        );
        let (title,): (Option<String>,) =
            sqlx::query_as("SELECT presentation_title FROM threads WHERE id = $1")
                .bind(&thread)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(title, None);
    }

    /// WS07 S6: a thread set off neither recalls nor learns, and the same user's other thread is
    /// untouched. Needs a disposable Postgres.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn thread_off_blocks_read_and_write_for_that_thread_only() {
        let pool = PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required")).await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let suffix = new_ulid();
        let org = format!("thread-memory-test-{suffix}");
        let new_thread = |key: &str| {
            create_thread_inner(&pool, pb::CreateThreadRequest {
                session_key: format!("{key}-{suffix}"), org_id: org.clone(), user_id: "test-user".into(), ..Default::default()
            })
        };
        let off_thread = new_thread("off").await.unwrap().into_inner().thread_id;
        let other_thread = new_thread("other").await.unwrap().into_inner().thread_id;
        let svc = test_service(pool.clone());
        let set_mode = |thread: &str, mode: &str, identity: VerifiedIdentity| {
            let mut request = Request::new(pb::UpdateThreadPresentationRequest {
                org_id: org.clone(),
                thread_id: thread.to_owned(),
                memory_mode: Some(mode.to_owned()),
                ..Default::default()
            });
            request.extensions_mut().insert(identity);
            svc.update_thread_presentation(request)
        };
        let owner = || VerifiedIdentity::user_for_test(&org, "test-user");
        let append = |thread: &str, content: &str| {
            let pool = pool.clone();
            let request = pb::AppendMessageRequest {
                thread_id: thread.to_owned(),
                role: "user".to_owned(),
                content: content.to_owned(),
                ..Default::default()
            };
            async move {
                let mut tx = pool.begin().await.unwrap();
                append_message_in_tx(&mut tx, &request, "test-user").await.unwrap();
                tx.commit().await.unwrap();
            }
        };
        let memory = || async {
            sqlx::query_scalar::<_, String>("SELECT content FROM agent_memory WHERE org_id = $1")
                .bind(&org)
                .fetch_all(&pool)
                .await
                .unwrap()
        };
        let recalled = |thread: &str| {
            let thread = thread.to_owned();
            let svc = &svc;
            async move {
                load_context_memory_rows(svc, MemoryRetention::Durable, &thread, None, &[])
                    .await
                    .unwrap()
                    .into_iter()
                    .any(|(_, content)| content.contains("Seeded user fact"))
            }
        };
        sqlx::query(
            "INSERT INTO agent_memory (id, org_id, session_id, scope, key, content, owner) \
             VALUES ($1, $2, NULL, 'user', 'user:seed', 'Seeded user fact', 'test-user')",
        )
        .bind(new_ulid())
        .bind(&org)
        .execute(&pool)
        .await
        .unwrap();

        // The value is validated, and a ZDR caller cannot store the choice.
        let invalid = set_mode(&off_thread, "paused", owner()).await.expect_err("only inherit or off");
        assert_eq!(invalid.code(), tonic::Code::InvalidArgument);
        let zdr = set_mode(&off_thread, "off", VerifiedIdentity::user_for_test_with_zdr(&org, "test-user", true))
            .await
            .expect_err("a ZDR caller stores no memory choice");
        assert_eq!(zdr.code(), tonic::Code::FailedPrecondition);
        assert!(recalled(&off_thread).await, "inherit is the default");

        set_mode(&off_thread, "off", owner()).await.expect("owner sets the thread off");
        append(&other_thread, "Husk at jeg heter Kari.").await;
        append(&off_thread, "Husk at jeg heter Ima.").await;
        let rows = memory().await;
        assert!(rows.iter().any(|row| row.contains("Kari")), "the other thread still learns");
        assert!(rows.iter().all(|row| !row.contains("Ima")), "the thread set off learns nothing");
        assert!(!recalled(&off_thread).await, "the thread set off recalls nothing");
        assert!(recalled(&other_thread).await, "the other thread still recalls");

        // Back to inherit: the thread recalls and learns again.
        set_mode(&off_thread, "inherit", owner()).await.expect("owner restores the thread");
        append(&off_thread, "Husk at jeg heter Ola.").await;
        assert!(memory().await.iter().any(|row| row.contains("Ola")));
        assert!(recalled(&off_thread).await);
    }

    /// Owner decision 17: a zero-retention owner may archive and delete the threads they own. The owner
    /// checks still apply, and the rest of their threads are left alone. Needs a disposable Postgres.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn zdr_receipt_a_zdr_owner_may_archive_and_delete_their_threads() {
        let pool =
            PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required"))
                .await
                .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let suffix = new_ulid();
        let org = format!("zdr-housekeeping-test-{suffix}");
        let mut threads = Vec::new();
        for n in 0..2 {
            let thread = create_thread_inner(
                &pool,
                pb::CreateThreadRequest {
                    session_key: format!("zdr-housekeeping-{suffix}-{n}"),
                    org_id: org.clone(),
                    user_id: "test-user".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap()
            .into_inner()
            .thread_id;
            threads.push(thread);
        }
        let svc = test_service(pool.clone());
        let zdr_owner = VerifiedIdentity::user_for_test_with_zdr(&org, "test-user", true);

        let mut archive = Request::new(pb::ArchiveThreadRequest {
            org_id: org.clone(),
            thread_id: threads[0].clone(),
        });
        archive.extensions_mut().insert(zdr_owner.clone());
        svc.archive_thread(archive)
            .await
            .expect("a ZDR owner may archive their thread");
        let archived: bool =
            sqlx::query_scalar("SELECT archived_at IS NOT NULL FROM threads WHERE id = $1")
                .bind(&threads[0])
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(archived);

        let mut delete = Request::new(pb::DeleteThreadRequest {
            org_id: org.clone(),
            thread_id: threads[1].clone(),
        });
        delete.extensions_mut().insert(zdr_owner);
        svc.delete_thread(delete)
            .await
            .expect("a ZDR owner may delete their thread");
        let remains: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM threads WHERE id = $1)")
                .bind(&threads[1])
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(!remains);
    }
}

/// The run metadata stamped at birth (WS20 S2). A person's run is a chat run. A
/// system owner's run gets no stamp here: a scheduled run is stamped by its own
/// binding, and an unstamped run reads as chat.
fn run_origin_at_birth(user_id: &str) -> serde_json::Value {
    if crate::auth::is_system_run_owner(user_id) {
        serde_json::json!({})
    } else {
        serde_json::json!({ ORIGIN_METADATA_KEY: RunOrigin::Chat.as_str() })
    }
}

/// Core of `SessionCore::start_run`, factored out to keep the trait method
/// small. Inserts the run row and its `RUN_STARTED` event in one tx.
pub(crate) async fn start_run_inner(
    pool: &PgPool,
    req: pb::StartRunRequest,
    requested_run_id: Option<String>,
    retention: crate::retention_receipt::RunRetention,
) -> Result<Response<pb::StartRunResponse>, Status> {
    use crate::retention_receipt::{self, RunRetention};
    let requested_run_id = requested_run_id.filter(|value| !value.trim().is_empty());
    // A requested run id is the prepared scheduled run's idempotent identity.
    // Only the persistent scheduled lane uses it, so a receipt of any other
    // class has no business naming one.
    if requested_run_id.is_some() && retention != RunRetention::persistent() {
        return Err(Status::permission_denied(
            "a requested run id is only for persistent scheduled runs",
        ));
    }
    let run_id = requested_run_id.clone().unwrap_or_else(new_ulid);
    let now = Utc::now();

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    // A child run takes its parent's receipt, and the parent must be this
    // owner's own run in this org. See `RunRetention::inherit_from`.
    let retention = if req.parent_run_id.trim().is_empty() {
        retention
    } else {
        let parent = retention_receipt::load_parent_retention(
            &mut tx,
            &req.org_id,
            &req.user_id,
            &req.parent_run_id,
        )
        .await?;
        retention.inherit_from(Some(&parent))?
    };

    // P0.4 residency: stamp the configured Model-Plane region (EU default,
    // Sweden Central) onto the run so its processing region is auditable.
    let residency = configured_residency();
    let inherited_space_context: Option<(Option<String>, Option<String>, Option<String>, Option<i64>, Option<String>, Option<String>, Option<String>, Option<i64>)> = sqlx::query_as(
        "INSERT INTO runs (id, thread_id, parent_run_id, agent_id, goal, mode, org_id, user_id, status, residency, created_at, updated_at,
                          space_id, space_decision_ref, recipient_audience_ref, recipient_audience_revision, recipient_audience_hash, privacy_policy_ref, resource_authorization_ref, authority_revision,
                          retention_class, waiver_ack_id, waiver_connection_id, metadata)
         SELECT $1, t.id, NULLIF($3, ''), $4, $5, $6, $7, $8, 'queued', $9, $10, $10,
                t.space_id, t.space_decision_ref, t.recipient_audience_ref, t.recipient_audience_revision, t.recipient_audience_hash, t.privacy_policy_ref, t.resource_authorization_ref, t.authority_revision,
                $11, $12, $13, $14
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
    .bind(retention.class.as_str())
    .bind(retention.ack_id())
    .bind(retention.connection_id())
    .bind(run_origin_at_birth(&req.user_id))
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

    // Same durable audit as the managed start: the row commits with the run.
    if let Some(body) = retention_receipt::run_started_zdr_waived_body(
        &retention,
        &run_id,
        &req.org_id,
        &req.user_id,
        &req.thread_id,
        now,
    ) {
        retention_receipt::enqueue_run_started_zdr_waived(&mut tx, &run_id, &body).await?;
    }

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

    // A terminal run never resumes, so its loop fold is dropped in this transaction
    // (build plan WS03 S4).
    loop_checkpoint::purge_run(tx, &req.run_id)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

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
                if req.error.trim().is_empty() {
                    "done"
                } else {
                    "failed"
                }
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

pub(crate) fn validate_tool_action_token(
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

pub(crate) fn tool_action_detail(
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

pub(crate) async fn enqueue_inline_tool_audit(
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
    body["details"]["request_id"] = serde_json::json!(request_id);
    let event_id = body["event_id"]
        .as_str()
        .ok_or_else(|| Status::internal("audit event identity missing"))?
        .to_owned();
    // The row's subject names the event the body carries (`tool_action` for a
    // final outcome, `tool_action_reserved` for the reservation): audit-core
    // accepts a message only when its subject's last token equals the payload's
    // `event`, so the two must come from the same place.
    let subject = body["event"]
        .as_str()
        .and_then(crate::audit_publisher::audit_subject_for_event)
        .ok_or_else(|| Status::internal("audit event name is not a subject token"))?;
    sqlx::query(
        "INSERT INTO session_audit_outbox (event_id, subject, payload)
         VALUES ($1, $2, $3)
         ON CONFLICT (event_id) DO NOTHING",
    )
    .bind(event_id)
    .bind(subject)
    .bind(body)
    .execute(&mut **tx)
    .await
    .map_err(|error| Status::internal(error.to_string()))?;
    Ok(())
}

pub(crate) async fn reserve_tool_action_inner(
    pool: &PgPool,
    audit_publisher: Option<&crate::audit_publisher::AuditOutbox>,
    req: pb::ReserveToolActionRequest,
) -> Result<Response<pb::ReserveToolActionResponse>, Status> {
    validate_tool_action_token(&req.run_id, "run_id", 128)?;
    validate_tool_action_token(&req.action_id, "action_id", 128)?;
    validate_tool_action_token(&req.request_id, "request_id", 128)?;
    validate_tool_action_token(&req.tool, "tool", 96)?;
    validate_tool_action_token(&req.data_category, "data_category", 64)?;
    // The ledger's two optional fields (build plan WS03 S5). Empty means the caller
    // does not supply them; anything else must have the shared shape.
    if !req.args_digest.is_empty() && !mp_contracts::effect_ledger::is_args_digest(&req.args_digest)
    {
        return Err(Status::invalid_argument("invalid args_digest"));
    }
    if !req.effect_class.is_empty()
        && mp_contracts::effect_ledger::EffectClass::parse(&req.effect_class).is_none()
    {
        return Err(Status::invalid_argument("invalid effect_class"));
    }

    let mut tx = pool
        .begin()
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
    let inserted = sqlx::query(
        "INSERT INTO session_tool_audit_intents
            (run_id, action_id, request_id, tool, data_category, zdr, args_digest, effect_class)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         ON CONFLICT (run_id, action_id) DO NOTHING",
    )
    .bind(&req.run_id)
    .bind(&req.action_id)
    .bind(&req.request_id)
    .bind(&req.tool)
    .bind(&req.data_category)
    .bind(req.zdr)
    .bind((!req.args_digest.is_empty()).then_some(req.args_digest.as_str()))
    .bind((!req.effect_class.is_empty()).then_some(req.effect_class.as_str()))
    .execute(&mut *tx)
    .await
    .map_err(|error| Status::internal(error.to_string()))?
    .rows_affected()
        == 1;

    let existing = sqlx::query_as::<
        _,
        (
            String,
            String,
            String,
            bool,
            String,
            String,
            Option<String>,
            Option<String>,
        ),
    >(
        "SELECT i.request_id, i.tool, i.data_category, i.zdr, r.org_id, r.user_id,
                i.args_digest, i.effect_class
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
        || existing.6.as_deref().unwrap_or("") != req.args_digest
        || existing.7.as_deref().unwrap_or("") != req.effect_class
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

/// Read the effect ledger's rows for named actions of one run (build plan WS03 S7a).
///
/// Everything that can refuse does so before the ledger is read: the read scope, a
/// zero-retention credential (the ledger belongs to a run that persists), the loop
/// checkpoint switch (`store_enabled`, the same closed refusal the checkpoint RPCs
/// give while the store is off, decided before any table is touched), the shape of
/// the request, the caller's right to read the run (the same rule `GetRun` applies,
/// including the refusal of a run whose owner was removed from its Space) and the
/// run's retention (a run that is not `persistent` keeps no fold, so no resume needs
/// its ledger, and it gets the refusal the claim path gives it). The rows are
/// digest-only.
///
/// An action id that is absent from the answer is not evidence that the call never
/// ran: see [`crate::tool_ledger::list_actions`].
pub(crate) async fn list_tool_actions_inner(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: pb::ListToolActionsRequest,
    store_enabled: bool,
) -> Result<Response<pb::ListToolActionsResponse>, Status> {
    authorize_operation(caller, "session:read")?;
    refuse_zdr_continuation(caller)?;
    if !store_enabled {
        return Err(loop_checkpoint::disabled());
    }
    crate::tool_ledger::validate_list_request(&req.run_id, &req.action_ids)?;
    // Not this file's `authorize_run_owner`: that one has no Space-membership check, and a
    // user removed from a Space must not read through this RPC what `GetRun` refuses them.
    crate::run_service_grpc::authorize_run_owner(pool, caller, &req.run_id, OwnerIntent::Read)
        .await?;
    crate::tool_ledger::require_persistent_run(pool, &req.run_id).await?;
    let rows =
        crate::tool_ledger::list_actions(pool, caller.org_id(), &req.run_id, &req.action_ids)
            .await?;
    Ok(Response::new(pb::ListToolActionsResponse {
        actions: rows.into_iter().map(pb::ToolActionState::from).collect(),
    }))
}

/// The cipher the loop-checkpoint RPCs seal and open folds with.
fn loop_checkpoint_cipher() -> Result<DescriptorCipher, Status> {
    DescriptorCipher::from_env()
        .map_err(|_| Status::failed_precondition("loop checkpoint key unavailable"))
}

/// A caller's right to write, claim or close a run's fold: the rule `GetRun` and
/// `ListToolActions` apply, so the three fold RPCs and the ledger read cannot disagree.
///
/// Not this file's `authorize_run_owner`: that one has no Space-membership check, and a fold
/// that is claimable while its ledger is unreadable lets a removed member's run continue
/// through the one lane that skips the revocation. The refusal is the shared one, a
/// permission-denied "Space membership required for this run", for any caller, owner or
/// service, while the run's owner is removed from its Space; a rejoin restores access. The
/// terminal purges (`purge_run`) never pass through here, so a run ends and loses its fold
/// whatever the owner's membership.
async fn authorize_loop_checkpoint_run(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    run_id: &str,
) -> Result<(), Status> {
    crate::run_service_grpc::authorize_run_owner(pool, caller, run_id, OwnerIntent::Mutate).await
}

/// Store a run's fold (build plan WS03 S4). `store_enabled` is the loop checkpoint switch and
/// `cipher` the key lookup, both passed in so a test can serve the call without the process
/// environment; the trait method hands in the real ones.
pub(crate) async fn save_loop_checkpoint_inner(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: pb::SaveLoopCheckpointRequest,
    store_enabled: bool,
    cipher: impl FnOnce() -> Result<DescriptorCipher, Status>,
) -> Result<Response<pb::SaveLoopCheckpointResponse>, Status> {
    authorize_operation(caller, "session:write")?;
    refuse_zdr_continuation(caller)?;
    if !store_enabled {
        return Err(loop_checkpoint::disabled());
    }
    authorize_loop_checkpoint_run(pool, caller, &req.run_id).await?;
    let cipher = cipher()?;
    let expected_seq = i64::try_from(req.expected_seq)
        .map_err(|_| Status::invalid_argument("expected seq is out of range"))?;
    let park_until = req
        .park_until
        .as_ref()
        .map(|timestamp| {
            u32::try_from(timestamp.nanos)
                .ok()
                .and_then(|nanos| DateTime::<Utc>::from_timestamp(timestamp.seconds, nanos))
                .ok_or_else(|| Status::invalid_argument("park_until is out of range"))
        })
        .transpose()?;
    let saved = loop_checkpoint::save(
        pool,
        &cipher,
        loop_checkpoint::SaveInput {
            run_id: &req.run_id,
            expected_seq,
            kind: &req.kind,
            fold: &req.fold,
            authority_digest: &req.authority_digest,
            pause_ref: &req.pause_ref,
            park_reason: &req.park_reason,
            park_until,
        },
    )
    .await?;
    Ok(Response::new(pb::SaveLoopCheckpointResponse {
        seq: u64::try_from(saved.seq)
            .map_err(|_| Status::internal("loop checkpoint seq is negative"))?,
        // True only for a park the store took under the park rules (WS03 S8-4); false for
        // every other save, and for every save while park recovery is off.
        park_accepted: saved.park_accepted,
    }))
}

/// Claim a paused or parked run's fold. See [`save_loop_checkpoint_inner`] for the two seams.
pub(crate) async fn claim_loop_checkpoint_inner(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: pb::ClaimLoopCheckpointRequest,
    store_enabled: bool,
    cipher: impl FnOnce() -> Result<DescriptorCipher, Status>,
) -> Result<Response<pb::ClaimLoopCheckpointResponse>, Status> {
    authorize_operation(caller, "session:write")?;
    refuse_zdr_continuation(caller)?;
    if !store_enabled {
        return Err(loop_checkpoint::disabled());
    }
    authorize_loop_checkpoint_run(pool, caller, &req.run_id).await?;
    let cipher = cipher()?;
    let claimed = loop_checkpoint::claim(
        pool,
        &cipher,
        &req.run_id,
        &req.pause_ref,
        &req.claim_token,
    )
    .await?;
    Ok(Response::new(pb::ClaimLoopCheckpointResponse {
        seq: u64::try_from(claimed.seq)
            .map_err(|_| Status::internal("loop checkpoint seq is negative"))?,
        fold: claimed.fold,
        segment_no: u32::try_from(claimed.segment_no)
            .map_err(|_| Status::internal("loop checkpoint segment is negative"))?,
        authority_digest: claimed.authority_digest,
        kind: claimed.kind,
    }))
}

/// Close a run's fold once its loop has finished. Like the other two it is refused for a run
/// whose owner was removed from its Space: that run ends, and its fold is purged, through the
/// terminal paths, which do not ask this question.
pub(crate) async fn close_loop_checkpoint_inner(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: pb::CloseLoopCheckpointRequest,
) -> Result<Response<pb::CloseLoopCheckpointResponse>, Status> {
    authorize_operation(caller, "session:write")?;
    authorize_loop_checkpoint_run(pool, caller, &req.run_id).await?;
    let seq = i64::try_from(req.seq)
        .map_err(|_| Status::invalid_argument("seq is out of range"))?;
    loop_checkpoint::close(pool, &req.run_id, seq, &req.reason).await?;
    Ok(Response::new(pb::CloseLoopCheckpointResponse {}))
}

pub(crate) async fn finalize_tool_action_inner(
    pool: &PgPool,
    audit_publisher: Option<&crate::audit_publisher::AuditOutbox>,
    req: pb::FinalizeToolActionRequest,
) -> Result<Response<pb::FinalizeToolActionResponse>, Status> {
    validate_tool_action_token(&req.run_id, "run_id", 128)?;
    validate_tool_action_token(&req.action_id, "action_id", 128)?;
    if !mp_contracts::effect_ledger::is_final_outcome(&req.outcome) {
        return Err(Status::invalid_argument("invalid tool action outcome"));
    }
    // A provider receipt belongs to a call that completed.
    if !req.receipt_id.is_empty() {
        validate_tool_action_token(&req.receipt_id, "receipt_id", 256)?;
        if req.outcome != "completed" {
            return Err(Status::invalid_argument(
                "a receipt belongs to a completed call",
            ));
        }
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
             SET status = $3, finalized_at = now(), closed_by = 'executor',
                 provider_receipt_id = $4
             WHERE run_id = $1 AND action_id = $2 AND status = 'reserved'",
        )
        .bind(&req.run_id)
        .bind(&req.action_id)
        .bind(&req.outcome)
        .bind((!req.receipt_id.is_empty()).then_some(req.receipt_id.as_str()))
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
    /// A persistent credential under a connection-bound waiver (Z-17). The run
    /// is kept, so its reads and its own local rows are as for `Durable`, but
    /// nothing it produces is sent to another processor: the external semantic
    /// memory service (Letta) is never written for it, and background dreaming
    /// skips it (see `dreaming::load_pending_messages`).
    WaivedRun,
}

impl MemoryRetention {
    pub(crate) fn of(caller: &VerifiedIdentity) -> Self {
        if caller.zdr() {
            Self::ZeroDataRetention
        } else if caller.zdr_waiver().is_some() {
            Self::WaivedRun
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
        matches!(self, Self::Durable | Self::WaivedRun)
    }

    /// Whether this run's content may be written to the external semantic memory
    /// service (Letta, served on another processor). Only an ordinary persistent
    /// run may: a ZDR run may not be written at all, and a waived run is
    /// persistent for the user but its content is not sent to a processor the
    /// user did not accept for it (Z-17, decision D-B).
    pub(crate) const fn permits_external_memory_indexing(self) -> bool {
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
/// A waived run's memory is kept locally but is not indexed into the external
/// semantic memory service (Z-17). Reported like any other retention skip, so
/// the missing semantic copy is never silent.
pub(crate) const WAIVED_MEMORY_INDEX_SUPPRESSED: &str = "DEGRADED_LETTA_WAIVED_INDEX_SUPPRESSED";

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
    pool: &PgPool,
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
    let mut outcome = letta
        .search_detailed(
            org_id,
            thread_id,
            owner_user_id.unwrap_or_default(),
            &query,
            &[],
            8,
        )
        .await;
    if crate::memory_control::filter_forgotten(pool, org_id, owner_user_id.unwrap_or_default(), &mut outcome.entries).await.is_err() {
        let status = SemanticContextSearchStatus::Degraded("DEGRADED_MEMORY_DELETION_STATE");
        record_semantic_context_search(status);
        return Some(status);
    }
    Some(append_letta_search_outcome(memory_rows, outcome, 8))
}

async fn load_context_memory_rows(
    svc: &SessionService,
    retention: MemoryRetention,
    thread_id: &str,
    user_id: Option<&str>,
    thread_messages: &[(String, String)],
) -> Result<Vec<(String, String)>, Status> {
    let thread_owner: Option<(String, String)> =
        sqlx::query_as("SELECT org_id, user_id FROM threads WHERE id = $1")
            .bind(thread_id)
            .fetch_optional(&svc.pool)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
    let thread_org_id = thread_owner.as_ref().map(|(org, _)| org.clone());

    // The single memory gate, for the user this context is assembled for (the
    // run's user when it names one, else the thread's owner, as the rows below
    // are scoped). A thread that does not exist has no memory to gate.
    let access = match thread_owner.as_ref() {
        Some((org, thread_user)) => {
            let gated_user = user_id
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(thread_user.as_str());
            // `false`: the retention posture keeps its own handling below
            // (`append_letta_memory_rows`), unchanged.
            crate::memory_control::resolve_for_thread(&svc.pool, false, org, gated_user, thread_id)
                .await
                .map_err(|e| Status::internal(e.to_string()))?
                .access
        }
        None => crate::memory_control::MemoryAccess::Full,
    };

    // The thread's own memory index is the user's content, so it is only used
    // when the user's memory is.
    let mut rows = if access == crate::memory_control::MemoryAccess::Full {
        sqlx::query_as::<_, (String, String)>(
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
        .map_err(|e| Status::internal(e.to_string()))?
    } else {
        Vec::new()
    };

    let mut agent_rows = crate::dreaming::load_agent_memory_context_rows(
        &svc.pool,
        thread_id,
        user_id,
        crate::dreaming::AGENT_MEMORY_CONTEXT_LIMIT,
        access,
    )
    .await
    .map_err(|e| Status::internal(e.to_string()))?;
    rows.append(&mut agent_rows);
    // The semantic mirror holds the user's rows only, so it is skipped unless
    // the user's memory is in use.
    if access != crate::memory_control::MemoryAccess::Full {
        return Ok(rows);
    }
    append_letta_memory_rows(
        &svc.pool,
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

    // Persona spec — the per-thread override, cheap and keyed by `thread_id`
    // alone (works even on a request with no `run_id`, unlike the run-scoped
    // lookup above). `None` only when the thread row itself cannot be found
    // (e.g. a request for a thread ID that does not exist); model-gateway
    // treats an absent value as enabled, the same fail-open direction this
    // whole feature uses throughout.
    let persona_enabled: Option<bool> =
        sqlx::query_scalar::<_, bool>("SELECT persona_enabled FROM threads WHERE id = $1")
            .bind(&thread_id)
            .fetch_optional(&svc.pool)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

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
        persona_enabled,
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
            let author_subject_id = caller.user_id().unwrap_or_default().to_owned();
            append_message_inner(
                &self.pool,
                self.letta_memory.as_ref(),
                MemoryRetention::of(&caller),
                req,
                &author_subject_id,
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
            // Not a ZDR path: `authorize_operation` refused a ZDR credential above.
            // The receipt is the caller's own. A waived user's run is recorded as
            // waived; a system owner is a service credential, which carries no
            // waiver, so its run is persistent.
            let retention = crate::retention_receipt::RunRetention::from_caller(&caller);
            start_run_inner(&self.pool, req, None, retention).await
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
                // A prepared scheduled run is the persistent lane. Orchestrator
                // Core holds a service credential, which never carries a waiver.
                crate::retention_receipt::RunRetention::persistent(),
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

    /// The reusable "service creates or reuses a thread owned by user U,
    /// readable by U, with a named origin" seam (chat-wave2
    /// "schedule"/"mail-task" specs, 2026-09-28). See the RPC's own proto
    /// doc comment for how this differs from `prepare_scheduled_run_thread`.
    async fn prepare_user_owned_thread(
        &self,
        request: Request<pb::PrepareUserOwnedThreadRequest>,
    ) -> Result<Response<pb::PrepareUserOwnedThreadResponse>, Status> {
        let caller = identity(&request)?;
        authorize_operation(&caller, "session:user-owned-thread-prepare")?;
        if !caller.is_service() || !USER_OWNED_THREAD_CALLERS.contains(&caller.principal_id()) {
            return Err(Status::permission_denied(
                "only an allow-listed trusted service may prepare a user-owned thread",
            ));
        }
        let req = request.into_inner();
        caller.authorize_org(&req.org_id)?;
        if req.org_id.trim().is_empty()
            || req.user_id.trim().is_empty()
            || req.session_key.trim().is_empty()
        {
            return Err(Status::invalid_argument(
                "org_id, user_id and session_key are required",
            ));
        }
        // A service caller must never be allowed to hand a thread to a
        // service-shaped "owner" through this path -- that is exactly what
        // prepare_scheduled_run_thread is for, with its own Space-authority
        // verification. This RPC's entire value is that the owner IS a real
        // user, so it is refused outright for anything else.
        if req.user_id.trim().starts_with("service:") {
            return Err(Status::invalid_argument(
                "user_id must be a real user, not a service principal",
            ));
        }
        let origin = req.origin.trim();
        if origin.is_empty() || origin == "space" || !THREAD_ORIGINS.contains(&origin) {
            return Err(Status::invalid_argument(format!(
                "origin must be one of {THREAD_ORIGINS:?} and not \"space\" (this RPC never sets space_id)"
            )));
        }
        authorize_user_owned_thread_origin(caller.principal_id(), origin)?;
        // Delegated-identity design, Gap B: mail_task is the one origin that
        // additionally requires an independently-verified Control decision
        // binding this exact (org_id, user_id, session_key) -- every other
        // origin keeps today's allow-list-only behavior unchanged.
        if origin == "mail_task" {
            verify_mail_task_thread_decision(&req)?;
        }
        let created = create_thread_inner_preverified(
            &self.pool,
            pb::CreateThreadRequest {
                session_key: req.session_key.clone(),
                org_id: req.org_id.clone(),
                user_id: req.user_id.clone(),
                origin: origin.to_owned(),
                ..Default::default()
            },
        )
        .await?
        .into_inner();
        Ok(Response::new(pb::PrepareUserOwnedThreadResponse {
            thread_id: created.thread_id,
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

    async fn list_tool_actions(
        &self,
        request: Request<pb::ListToolActionsRequest>,
    ) -> Result<Response<pb::ListToolActionsResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            list_tool_actions_inner(
                &self.pool,
                &caller,
                request.into_inner(),
                loop_checkpoint::enabled(),
            )
            .await
        }
        .await;
        record_metrics("list_tool_actions", started, result.is_ok());
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

    async fn save_loop_checkpoint(
        &self,
        request: Request<pb::SaveLoopCheckpointRequest>,
    ) -> Result<Response<pb::SaveLoopCheckpointResponse>, Status> {
        let caller = identity(&request)?;
        save_loop_checkpoint_inner(
            &self.pool,
            &caller,
            request.into_inner(),
            loop_checkpoint::enabled(),
            loop_checkpoint_cipher,
        )
        .await
    }

    async fn claim_loop_checkpoint(
        &self,
        request: Request<pb::ClaimLoopCheckpointRequest>,
    ) -> Result<Response<pb::ClaimLoopCheckpointResponse>, Status> {
        let caller = identity(&request)?;
        claim_loop_checkpoint_inner(
            &self.pool,
            &caller,
            request.into_inner(),
            loop_checkpoint::enabled(),
            loop_checkpoint_cipher,
        )
        .await
    }

    async fn close_loop_checkpoint(
        &self,
        request: Request<pb::CloseLoopCheckpointRequest>,
    ) -> Result<Response<pb::CloseLoopCheckpointResponse>, Status> {
        let caller = identity(&request)?;
        close_loop_checkpoint_inner(&self.pool, &caller, request.into_inner()).await
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
            //
            // Only an accepted skill can be switched on: a proposal becomes active
            // by its owner's decision, never by a flag flip. Switching off is
            // always allowed, so a quarantine sweep is never refused.
            let updated: Option<(bool,)> = sqlx::query_as(
                "UPDATE agent_skills SET enabled = $1, updated_at = now() \
                 WHERE id = $2 AND org_id = $3 AND (NOT $1 OR review_state = 'accepted') \
                 RETURNING enabled",
            )
            .bind(req.enabled)
            .bind(&req.skill_id)
            .bind(&req.org_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;

            let Some((enabled,)) = updated else {
                if req.enabled {
                    let undecided: bool = sqlx::query_scalar(
                        "SELECT EXISTS (SELECT 1 FROM agent_skills \
                         WHERE id = $1 AND org_id = $2 AND review_state <> 'accepted')",
                    )
                    .bind(&req.skill_id)
                    .bind(&req.org_id)
                    .fetch_one(&self.pool)
                    .await
                    .map_err(|error| Status::internal(error.to_string()))?;
                    if undecided {
                        return Err(Status::failed_precondition("proposal_requires_decision"));
                    }
                }
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
            // A learned skill is a disabled proposal owned by one user, whatever
            // the caller asked for; a human write is accepted as it always was.
            let (scope, owner, review_state, enabled) = if origin == "background_review" {
                let owner = req.owner_user_id.trim();
                if owner.is_empty() {
                    return Ok(Response::new(skill_write_skipped("owner_required")));
                }
                // A user credential may only write for itself; a service
                // (the learning review) names the owner of the run it reviewed.
                if caller.user_id().is_some() {
                    caller.authorize_user(owner)?;
                }
                ("user", owner.to_owned(), "proposed", false)
            } else {
                match req.scope.as_str() {
                    "" | "org" => ("org", String::new(), "accepted", req.enabled),
                    "user" => {
                        let owner = match caller.user_id() {
                            Some(verified) => {
                                if !req.owner_user_id.is_empty() {
                                    caller.authorize_user(&req.owner_user_id)?;
                                }
                                verified.to_owned()
                            }
                            None => req.owner_user_id.trim().to_owned(),
                        };
                        if owner.is_empty() {
                            return Err(Status::invalid_argument(
                                "owner_user_id is required for a user-scoped skill",
                            ));
                        }
                        ("user", owner, "accepted", req.enabled)
                    }
                    other => {
                        return Err(Status::invalid_argument(format!("invalid scope: {other}")));
                    }
                }
            };
            let id = new_ulid();
            let mut tx = self
                .pool
                .begin()
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
            if origin == "background_review" {
                // Under the owner's row lock, so a pause that commits first is
                // seen here and one that waits lands after this write. The
                // announcement of the run was gated too; this is the gate where
                // the review's output is written.
                let decision =
                    crate::memory_control::resolve_in_tx(&mut tx, caller.zdr(), &req.org_id, &owner)
                        .await
                        .map_err(|e| Status::internal(e.to_string()))?;
                if let Some(reason) = learned_skill_withheld(&decision) {
                    return Ok(Response::new(skill_write_skipped(reason)));
                }
            }
            // The conflict key is (org, scope, owner, name). A learned skill
            // refreshes only a proposal it wrote itself and never touches
            // enabled, scope, owner or review_state; a human write keeps setting
            // `enabled` but cannot switch on a proposal nobody accepted.
            // None ⟺ the conflict guard rejected the write.
            let row: Option<(String, bool)> = if origin == "background_review" {
                sqlx::query_as(
                    "INSERT INTO agent_skills
                        (id, org_id, name, description, content, trigger_keywords,
                         trigger_file_patterns, tool_restrictions, enabled, origin,
                         scope, owner_user_id, review_state, source_run_id,
                         source_thread_id, proposal_reason, created_at, updated_at)
                     VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16, now(), now())
                     ON CONFLICT (org_id, scope, owner_user_id, name) DO UPDATE SET
                         description = EXCLUDED.description,
                         content = EXCLUDED.content,
                         trigger_keywords = EXCLUDED.trigger_keywords,
                         trigger_file_patterns = EXCLUDED.trigger_file_patterns,
                         tool_restrictions = EXCLUDED.tool_restrictions,
                         source_run_id = EXCLUDED.source_run_id,
                         source_thread_id = EXCLUDED.source_thread_id,
                         proposal_reason = EXCLUDED.proposal_reason,
                         updated_at = now()
                     WHERE agent_skills.origin = 'background_review'
                       AND agent_skills.review_state = 'proposed'
                     RETURNING id, (xmax = 0) AS created",
                )
            } else {
                sqlx::query_as(
                    "INSERT INTO agent_skills
                        (id, org_id, name, description, content, trigger_keywords,
                         trigger_file_patterns, tool_restrictions, enabled, origin,
                         scope, owner_user_id, review_state, source_run_id,
                         source_thread_id, proposal_reason, created_at, updated_at)
                     VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16, now(), now())
                     ON CONFLICT (org_id, scope, owner_user_id, name) DO UPDATE SET
                         description = EXCLUDED.description,
                         content = EXCLUDED.content,
                         trigger_keywords = EXCLUDED.trigger_keywords,
                         trigger_file_patterns = EXCLUDED.trigger_file_patterns,
                         tool_restrictions = EXCLUDED.tool_restrictions,
                         enabled = EXCLUDED.enabled AND agent_skills.review_state = 'accepted',
                         updated_at = now()
                     RETURNING id, (xmax = 0) AS created",
                )
            }
            .bind(&id)
            .bind(&req.org_id)
            .bind(&req.name)
            .bind(&req.description)
            .bind(&req.content)
            .bind(serde_json::json!(req.trigger_keywords))
            .bind(serde_json::json!(req.trigger_file_patterns))
            .bind(serde_json::json!(req.tool_restrictions))
            .bind(enabled)
            .bind(origin)
            .bind(scope)
            .bind(&owner)
            .bind(review_state)
            .bind(bounded_provenance(&req.source_run_id))
            .bind(bounded_provenance(&req.source_thread_id))
            .bind(
                req.proposal_reason
                    .trim()
                    .chars()
                    .take(MAX_PROPOSAL_REASON_CHARS)
                    .collect::<String>(),
            )
            .fetch_optional(&mut *tx)
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
                    skip_reason: String::new(),
                    revision: 0,
                },
                None => {
                    // Only a learned write can lose a conflict. Say why: the row
                    // is a human's, or its owner already decided on it.
                    let existing: Option<(String, String)> = sqlx::query_as(
                        "SELECT origin, review_state FROM agent_skills
                         WHERE org_id = $1 AND scope = $2 AND owner_user_id = $3 AND name = $4",
                    )
                    .bind(&req.org_id)
                    .bind(scope)
                    .bind(&owner)
                    .bind(&req.name)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(|e| Status::internal(e.to_string()))?;
                    let reason = match existing {
                        Some((origin, _)) if origin == "user" => "protected",
                        Some((_, state)) if state == "rejected" => "rejected_exists",
                        Some((_, state)) if state == "accepted" => "accepted_exists",
                        _ => "protected",
                    };
                    skill_write_skipped(reason)
                }
            };
            tx.commit()
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
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
            refuse_waived_grant(&caller, req.granted_rung)?;
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
            validate_grant_plan_id(&req.approved_plan_id, grant.is_some())?;
            // The run update and the thread's grant are ONE transaction: a grant
            // that reached the thread without the run recording it, or the other
            // way round, is an approval a later run reads differently depending
            // on which replica asks.
            let mut tx = self.pool.begin().await.map_err(|e| {
                warn!(error = %e, "set_run_mode failed");
                Status::internal(e.to_string())
            })?;
            // Org-scoped UPDATE: a run is only mutable by its owning org, so a
            // caller can never flip the mode of another org's run (per-org
            // isolation invariant). A missing row ⟺ wrong org OR unknown run;
            // both surface as not_found without leaking which.
            let row: Option<(String, String, String)> = sqlx::query_as(
                "UPDATE runs
                    SET mode = $2,
                        metadata = COALESCE(metadata, '{}'::jsonb) || $4::jsonb,
                        updated_at = now()
                  WHERE id = $1 AND org_id = $3 RETURNING id, mode, thread_id",
            )
            .bind(&req.run_id)
            .bind(mode)
            .bind(&req.org_id)
            .bind(&grant_patch)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| {
                warn!(error = %e, "set_run_mode failed");
                Status::internal(e.to_string())
            })?;
            let (run_id, mode, thread_id) =
                row.ok_or_else(|| Status::not_found(format!("run {} not found", req.run_id)))?;
            if let Some(grant) = grant.as_ref() {
                if self.grant_columns.records_grants() {
                    record_thread_grant(
                        &mut tx,
                        &req.org_id,
                        &thread_id,
                        grant,
                        &req.approved_plan_id,
                    )
                    .await?;
                } else {
                    // The 0050 columns are not there (the revert dropped them): the run keeps
                    // its own audit copy of the grant, and the thread keeps none. Failing the
                    // statement would roll the run update back with it.
                    warn!(
                        "set_run_mode stated a grant but the thread grant columns are absent;                          the thread's copy was skipped"
                    );
                }
            }
            tx.commit().await.map_err(|e| {
                warn!(error = %e, "set_run_mode failed");
                Status::internal(e.to_string())
            })?;
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

    // Build plan WS02: stamp how a run's context (instructions, history,
    // memory) was assembled onto the run record. Closed tokens and counts only.
    // Merged into `runs.metadata` like `set_run_mode` does, so no migration and
    // no other key is dropped; `GetRun` already returns it.
    async fn record_run_context(
        &self,
        request: Request<pb::RecordRunContextRequest>,
    ) -> Result<Response<pb::RecordRunContextResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::RecordRunContextResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:write")?;
            // A durable run mutation: a zero-retention or waived credential
            // never persists one (the run it describes keeps no content).
            if caller.zdr() || caller.zdr_waiver().is_some() {
                return Err(Refusal::DURABLE_MUTATION_FORBIDDEN
                    .grpc_status(tonic::Code::FailedPrecondition));
            }
            let req = request.into_inner();
            if req.run_id.is_empty() || req.org_id.is_empty() {
                return Err(Status::invalid_argument("run_id and org_id are required"));
            }
            let Some(status) = req.status.as_ref() else {
                return Err(Status::invalid_argument("status is required"));
            };
            caller.authorize_org(&req.org_id)?;
            authorize_run_owner(&self.pool, &caller, &req.run_id, OwnerIntent::Mutate).await?;
            let patch = serde_json::json!({
                "run_context": mp_contracts::run_context::status_json(status),
            });
            let updated = sqlx::query(
                "UPDATE runs
                    SET metadata = COALESCE(metadata, '{}'::jsonb) || $3::jsonb,
                        updated_at = now()
                  WHERE id = $1 AND org_id = $2",
            )
            .bind(&req.run_id)
            .bind(&req.org_id)
            .bind(&patch)
            .execute(&self.pool)
            .await
            .map_err(|e| {
                warn!(error = %e, "record_run_context failed");
                Status::internal(e.to_string())
            })?;
            if updated.rows_affected() == 0 {
                return Err(Status::not_found(format!("run {} not found", req.run_id)));
            }
            Ok(Response::new(pb::RecordRunContextResponse {}))
        }
        .await;
        record_metrics("record_run_context", started, result.is_ok());
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
            record_scheduled_step_receipt_inner(
                &self.pool,
                request.into_inner(),
                crate::terminalization::scheduled_result_persist_enabled(),
            )
            .await
        }
        .await;
        record_metrics("record_scheduled_step_receipt", started, result.is_ok());
        result
    }

    async fn record_scheduled_run_result(
        &self,
        request: Request<pb::RecordScheduledRunResultRequest>,
    ) -> Result<Response<pb::RecordScheduledRunResultResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, crate::auth::SCHEDULED_STEP_SCOPE)?;
            authorize_scheduled_step_service(&caller)?;
            if !crate::terminalization::scheduled_result_persist_enabled() {
                return Err(Status::failed_precondition(
                    "scheduled result persistence is disabled",
                ));
            }
            record_scheduled_run_result_inner(&self.pool, request.into_inner()).await
        }
        .await;
        record_metrics("record_scheduled_run_result", started, result.is_ok());
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
        #[derive(sqlx::FromRow)]
        struct SkillRow {
            id: String,
            name: String,
            description: String,
            content: String,
            trigger_keywords: sqlx::types::Json<Vec<String>>,
            trigger_file_patterns: sqlx::types::Json<Vec<String>>,
            tool_restrictions: sqlx::types::Json<Vec<String>>,
            enabled: bool,
            origin: String,
            scope: String,
            owner_user_id: String,
            shared_with: sqlx::types::Json<Vec<String>>,
            review_state: String,
            source_run_id: String,
            source_thread_id: String,
            proposal_reason: String,
            updated_at: DateTime<Utc>,
        }

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
            //
            // A service caller (the learning review) lists as the user whose run
            // it is reviewing; a user caller is always listed as itself, so
            // `for_user_id` can never be used to read another person's skills.
            let caller_user_id = if caller.is_service() && !req.for_user_id.trim().is_empty() {
                req.for_user_id.trim().to_owned()
            } else {
                caller.user_id().unwrap_or_default().to_owned()
            };
            // A grantee sees only a skill its owner has accepted: a proposal is
            // private to its owner even if a share was written onto the row.
            // `scope DESC` puts a user's own skill before an org skill of the
            // same name.
            let rows: Vec<SkillRow> = sqlx::query_as(
                "SELECT id, name, description, content, trigger_keywords,
                        trigger_file_patterns, tool_restrictions, enabled, origin,
                        scope, owner_user_id, shared_with, review_state,
                        source_run_id, source_thread_id, proposal_reason, updated_at
                 FROM agent_skills
                 WHERE org_id = $1 AND (NOT $2 OR enabled)
                   AND (scope = 'org' OR owner_user_id = $3
                        OR (shared_with ? $3 AND review_state = 'accepted'))
                 ORDER BY name, scope DESC",
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
                .map(|row| pb::AgentSkill {
                    id: row.id,
                    name: row.name,
                    description: row.description,
                    content: row.content,
                    trigger_keywords: row.trigger_keywords.0,
                    trigger_file_patterns: row.trigger_file_patterns.0,
                    tool_restrictions: row.tool_restrictions.0,
                    enabled: row.enabled,
                    origin: row.origin,
                    scope: row.scope,
                    owner_user_id: row.owner_user_id,
                    shared_with: row.shared_with.0,
                    review_state: row.review_state,
                    source_run_id: row.source_run_id,
                    source_thread_id: row.source_thread_id,
                    proposal_reason: row.proposal_reason,
                    updated_at: row.updated_at.to_rfc3339(),
                    revision: 0,
                })
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
            // Two ways to be allowed to read a conversation, and only two.
            // Without a Control decision this stays exactly as it was: you must
            // own the thread. With one, Control has already resolved you as a
            // current member AND current recipient of this Space, so ownership
            // stops being the question -- which is what lets a room have more
            // than one reader.
            let space_read = verify_thread_read_space_decision(
                &req.space_read_decision_token,
                &req.space_read_decision_ref,
                &req.org_id,
                caller.user_id().unwrap_or_default(),
                &req.space_id,
            )?;
            match space_read {
                None => {
                    authorize_thread_owner(&self.pool, &caller, &req.thread_id, OwnerIntent::Read)
                        .await?
                }
                Some(_) => {
                    // The decision authorizes one Space, not any thread the
                    // caller can name. A thread outside it -- or an unscoped
                    // one -- is not found rather than refused, matching what an
                    // unauthorized reader already sees from the owner path.
                    let scoped: Option<(String,)> = sqlx::query_as(
                        "SELECT id FROM threads WHERE id = $1 AND org_id = $2 AND space_id = $3",
                    )
                    .bind(&req.thread_id)
                    .bind(&req.org_id)
                    .bind(&req.space_id)
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(|error| Status::internal(error.to_string()))?;
                    if scoped.is_none() {
                        return Err(Status::not_found("thread not found"));
                    }
                }
            }
            // Newer-audience rows are dropped for a Space reader: they were
            // written for a recipient set this decision was not issued against.
            // A NULL revision predates audience snapshots entirely and is
            // covered by the thread-level Space check above.
            let audience_ceiling = space_read.map(|read| read.recipient_audience_revision);
            let rows: Vec<(
                String,
                String,
                String,
                Option<String>,
                Option<serde_json::Value>,
                Option<String>,
            )> = sqlx::query_as(
                // `m.id` is selected so a caller can name one earlier turn
                // durably (message pinning). It was always in the table; this
                // read simply never returned it.
                "SELECT m.id::text, m.role, m.content, m.agent_name, m.metadata, m.author_subject_id
                 FROM messages m
                 JOIN threads t ON t.id = m.thread_id
                 WHERE m.thread_id = $1 AND t.org_id = $2
                   AND ($3::BIGINT IS NULL
                        OR m.recipient_audience_revision IS NULL
                        OR m.recipient_audience_revision <= $3)
                 ORDER BY m.sequence",
            )
                .bind(&req.thread_id)
                .bind(&req.org_id)
                .bind(audience_ceiling)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| {
                    warn!(error = %e, "list_thread_messages failed");
                    Status::internal(e.to_string())
                })?;
            let messages = rows
                .into_iter()
                .map(
                    |(message_id, role, content, agent_name, metadata, author_subject_id)| {
                        pb::SessionMessage {
                            message_id,
                            role,
                            content,
                            agent_name: agent_name.unwrap_or_default(),
                            author_subject_id: author_subject_id.unwrap_or_default(),
                            // `{}` is the column default for every turn written before
                            // metadata was persisted; send None rather than an empty
                            // Struct so the caller can tell "no evidence recorded" from
                            // "evidence recorded and empty".
                            metadata: metadata
                                .filter(|value| !matches!(value, serde_json::Value::Null))
                                .filter(|value| value.as_object().is_none_or(|map| !map.is_empty()))
                                .as_ref()
                                .and_then(json_to_struct),
                        }
                    },
                )
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
            // A shared-Space listing is the same query with a different answer
            // to "whose threads". Without a decision it stays owner-bound, which
            // is why every member of one room used to see only their own posts
            // in it.
            let space_read = verify_thread_read_space_decision(
                &req.space_read_decision_token,
                &req.space_read_decision_ref,
                &req.org_id,
                &req.user_id,
                &req.space_id,
            )?;
            let audience_ceiling = space_read.map(|read| read.recipient_audience_revision);
            let limit = clamp_thread_limit(req.limit);
            let page_size = usize::try_from(limit).unwrap_or(usize::MAX);
            let archive_state = match pb::ArchiveFilter::try_from(req.archived) {
                Ok(pb::ArchiveFilter::Unspecified) => "active",
                Ok(pb::ArchiveFilter::Archived) => "archived",
                Ok(pb::ArchiveFilter::All) => "all",
                Err(_) => {
                    return Err(Status::invalid_argument(
                        "archived must be ARCHIVE_FILTER_ACTIVE, ARCHIVE_FILTER_ARCHIVED or ARCHIVE_FILTER_ALL",
                    ))
                }
            };
            let (cursor_pinned, cursor_activity, cursor_thread_id) = if req.cursor.is_empty() {
                (None, None, None)
            } else {
                let cursor = decode_thread_cursor(&req.cursor)?;
                (
                    Some(cursor.pinned),
                    Some(cursor.last_activity_at),
                    Some(cursor.thread_id),
                )
            };
            // One extra row tells the caller whether another page exists without
            // a second count query.
            let mut rows: Vec<(
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
                String,
                bool,
                Option<DateTime<Utc>>,
            )> = sqlx::query_as(
                "WITH page AS (
                    SELECT t.id, t.session_key, t.created_at, t.presentation_title,
                           t.presentation_preview, t.pinned_at, t.last_activity_at, t.space_id,
                           t.origin, t.user_id, t.persona_enabled, t.archived_at
                    FROM threads t
                    WHERE t.org_id = $1
                      AND ($6::BIGINT IS NULL
                           OR (t.recipient_audience_revision IS NULL
                               OR t.recipient_audience_revision <= $6))
                      AND ($6::BIGINT IS NOT NULL OR t.user_id = $2)
                      AND (NULLIF($4, '') IS NULL OR t.space_id = $4)
                      AND (NULLIF($5, '') IS NULL OR t.origin = $5)
                      AND (NULLIF($7, '') IS NULL OR t.id = $7)
                      AND ($11 = 'all' OR (($11 = 'archived') = (t.archived_at IS NOT NULL)))
                      AND ($8::BOOLEAN IS NULL
                           OR ((t.pinned_at IS NOT NULL), t.last_activity_at, t.id)
                              < ($8::BOOLEAN, $9::TIMESTAMPTZ, $10::TEXT))
                    ORDER BY (t.pinned_at IS NOT NULL) DESC, t.last_activity_at DESC, t.id DESC
                    LIMIT $3
                 )
                 SELECT
                    page.id,
                    page.session_key,
                    page.created_at,
                    COALESCE(page.presentation_title, first_user.content) AS title,
                    COALESCE(page.presentation_preview, last_message.content) AS preview,
                    page.pinned_at,
                    page.last_activity_at,
                    page.space_id,
                    latest_run.id AS latest_run_id,
                    latest_run.status AS latest_run_status,
                    latest_run.updated_at AS latest_run_updated_at,
                    page.origin,
                    page.user_id,
                    page.persona_enabled,
                    page.archived_at
                 FROM page
                 LEFT JOIN LATERAL (
                    SELECT content
                    FROM messages
                    WHERE thread_id = page.id AND role = 'user'
                    ORDER BY sequence ASC
                    LIMIT 1
                 ) first_user ON TRUE
                 LEFT JOIN LATERAL (
                    SELECT content
                    FROM messages
                    WHERE thread_id = page.id
                    ORDER BY sequence DESC
                    LIMIT 1
                 ) last_message ON TRUE
                 LEFT JOIN LATERAL (
                    SELECT id, status, updated_at
                    FROM runs
                    WHERE thread_id = page.id
                    ORDER BY updated_at DESC, created_at DESC, id DESC
                    LIMIT 1
                 ) latest_run ON TRUE
                 ORDER BY (page.pinned_at IS NOT NULL) DESC, page.last_activity_at DESC, page.id DESC",
            )
            .bind(&req.org_id)
            .bind(&req.user_id)
            .bind(limit + 1)
            .bind(&req.space_id)
            .bind(&req.origin)
            .bind(audience_ceiling)
            .bind(&req.thread_id)
            .bind(cursor_pinned)
            .bind(cursor_activity)
            .bind(cursor_thread_id)
            .bind(archive_state)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| {
                warn!(error = %e, "list_threads failed");
                Status::internal(e.to_string())
            })?;

            let has_more = rows.len() > page_size;
            let next_cursor = match rows.get(page_size - 1) {
                Some(last) if has_more => encode_thread_cursor(&ThreadCursor {
                    pinned: last.5.is_some(),
                    last_activity_at: last.6,
                    thread_id: last.0.clone(),
                }),
                _ => String::new(),
            };
            rows.truncate(page_size);

            let threads = rows
                .into_iter()
                .map(
                    |(thread_id, session_key, created_at, title, preview, pinned_at, last_activity_at, space_id, latest_run_id, latest_run_status, latest_run_updated_at, origin, owner_subject_id, persona_enabled, archived_at)| {
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
                            updated_at: Some(to_proto_timestamp(last_activity_at)),
                            pinned: pinned_at.is_some(),
                            space_id: space_id.unwrap_or_default(),
                            latest_run_id: latest_run_id.unwrap_or_default(),
                            latest_run_status: latest_run_status.unwrap_or_default(),
                            latest_run_updated_at: latest_run_updated_at.map(to_proto_timestamp),
                            origin,
                            owner_subject_id,
                            persona_enabled,
                            archived_at: archived_at.map(to_proto_timestamp),
                        }
                    },
                )
                .collect();

            Ok(Response::new(pb::ListThreadsResponse {
                threads,
                next_cursor,
            }))
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
            authorize_owner_housekeeping(&caller, "session:write")?;
            let req = request.into_inner();
            // Owner decision 17: the presentation controls (pinned, persona) are the owner's under every
            // posture. Title and preview are conversation text, which a ZDR credential never persists.
            if caller.zdr() && (req.title.is_some() || req.preview.is_some()) {
                return Err(Refusal::DURABLE_MUTATION_FORBIDDEN
                    .grpc_status(tonic::Code::FailedPrecondition));
            }
            // Memory is already off for a ZDR caller, so a per-thread choice has
            // nothing to say and is not stored for it.
            if caller.zdr() && req.memory_mode.is_some() {
                return Err(Refusal::DURABLE_MUTATION_FORBIDDEN
                    .grpc_status(tonic::Code::FailedPrecondition));
            }
            if req.org_id.trim().is_empty() || req.thread_id.trim().is_empty() {
                return Err(Status::invalid_argument(
                    "org_id and thread_id are required",
                ));
            }
            if req.title.is_none()
                && req.preview.is_none()
                && req.pinned.is_none()
                && req.persona_enabled.is_none()
                && req.archived.is_none()
                && req.memory_mode.is_none()
            {
                return Err(Status::invalid_argument(
                    "at least one presentation field is required",
                ));
            }
            let memory_mode = req
                .memory_mode
                .as_deref()
                .map(|value| {
                    crate::memory_control::ThreadMemoryMode::from_wire(value).ok_or_else(|| {
                        Status::invalid_argument("memory_mode must be \"inherit\" or \"off\"")
                    })
                })
                .transpose()?;
            // Archiving writes the THREAD_ARCHIVED evidence event in ArchiveThread; this
            // RPC only restores, so a true here would hide a thread without that event.
            if req.archived == Some(true) {
                return Err(Status::invalid_argument(
                    "archived=true is not accepted here; use ArchiveThread",
                ));
            }
            let restores = req.archived == Some(false);
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
                     END,
                     persona_enabled = CASE WHEN $10 THEN $11 ELSE persona_enabled END,
                     archived_at = CASE WHEN $12 THEN NULL ELSE archived_at END,
                     memory_mode = CASE WHEN $13 THEN $14 ELSE memory_mode END
                 WHERE id = $1 AND org_id = $2 AND (archived_at IS NULL OR $12)
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
            // Plain boolean, no tri-state NULL semantics needed — unlike
            // `pinned_at`'s optional timestamp, "unset" and "false" are the
            // same non-value for a column that is itself `NOT NULL`.
            .bind(req.persona_enabled.is_some())
            .bind(req.persona_enabled.unwrap_or(true))
            .bind(restores)
            .bind(memory_mode.is_some())
            .bind(memory_mode.map_or("inherit", |mode| mode.as_str()))
            .fetch_optional(&mut *tx)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
            let (_, thread_owner) =
                changed.ok_or_else(|| Status::not_found("thread not found or archived"))?;
            if memory_mode.is_some() {
                // The same lock a memory writer holds while it asks the gate, so an
                // extraction in flight finishes before the change commits.
                crate::memory_control::lock_user(&mut tx, &req.org_id, &thread_owner)
                    .await
                    .map_err(|error| Status::internal(error.to_string()))?;
            }

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
                "persona_enabled": req.persona_enabled,
                "archived": req.archived,
                "memory_mode": memory_mode.map(|mode| mode.as_str()),
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
            authorize_owner_housekeeping(&caller, "session:write")?;
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
                // The key is per archive, not per thread: a restore lets the thread be
                // archived again, and events is unique on (org_id, idempotency_key).
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
                    &format!("{}:archived:{}", req.thread_id, archived_at.to_rfc3339()),
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
            authorize_owner_housekeeping(&caller, "session:write")?;
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
            authorize_owner_housekeeping(&caller, "session:write")?;
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
            authorize_owner_housekeeping(&caller, "session:write")?;
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
            // The run's immutable retention receipt is the one the caller's own
            // verified claims give: zero retention, a waived persistent run on
            // its one connection, or an ordinary persistent run.
            let retention = crate::retention_receipt::RunRetention::from_caller(&caller);
            terminalization::start_managed_run_with_retention_inner(&self.pool, req, retention)
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

    /// Save a gateway-direct run's final assistant message and its Completed
    /// terminal receipt in one transaction.
    ///
    /// * **The receipt's authority, not a user's.** The caller must be the
    ///   workload that owns `GatewayDirect`, holding `session:terminalize`,
    ///   exactly as for `RecordTerminalOutcome`. A delegated user bearer is not
    ///   accepted, so a turn that outlives it can still be saved.
    /// * **Owner from the run.** Tenant and user come from the durable run, and
    ///   the message can go only into that run's own thread.
    /// * **Retention from the run, not the credential.** A zero-retention run
    ///   is refused inside the transaction, so the post-commit memory mirror
    ///   below is always durable. The service credential's own `zdr` is Auth
    ///   Core's fixed per-principal posture, `true` for every Model Gateway
    ///   token, so it cannot say whether this turn may be kept (see
    ///   `terminalization::authorize_direct_completion`).
    async fn complete_direct_run_with_message(
        &self,
        request: Request<pb::CompleteDirectRunWithMessageRequest>,
    ) -> Result<Response<pb::CompleteDirectRunWithMessageResponse>, Status> {
        let started = Instant::now();
        // For the refusal log only; bounded, since it is not validated yet.
        let run_id: String = request.get_ref().run_id.chars().take(128).collect();
        let result: Result<Response<pb::CompleteDirectRunWithMessageResponse>, Status> = async {
            let caller = identity(&request)?;
            let req = request.into_inner();
            terminalization::authorize_direct_completion(&caller)?;
            authorize_run_owner(&self.pool, &caller, &req.run_id, OwnerIntent::Mutate).await?;
            let thread_id = req.thread_id.clone();
            let completed =
                terminalization::complete_direct_run_with_message_inner(&self.pool, req).await?;
            // The mirror follows the RUN's receipt. This caller is the gateway's
            // service credential, which never carries a waiver.
            sync_appended_memory(
                &self.pool,
                self.letta_memory.as_ref(),
                completed.memory_retention,
                &thread_id,
                completed.memory,
            )
            .await;
            Ok(Response::new(completed.completion.into_proto()))
        }
        .await;
        if let Err(status) = &result {
            // Content-free: the status code and Session Core's own reason,
            // never the answer or its metadata.
            warn!(
                run_id = %run_id,
                code = ?status.code(),
                reason = status.message(),
                "CompleteDirectRunWithMessage refused"
            );
        }
        record_metrics("complete_direct_run_with_message", started, result.is_ok());
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

    let managed_lifecycle = ManagedRunLifecycleService {
        pool: pool.clone(),
        letta_memory: letta_memory.clone(),
    };
    let retention = SessionRetentionGrpc { pool: pool.clone() };
    // Checked once, here, after the migrations ran: a database whose 0050 columns were dropped
    // (the revert) must not make every grant-bearing SetRunMode fail with a missing column.
    let grant_columns = ThreadGrantColumns::detect(&pool).await;
    let thread_autonomy = ThreadAutonomyGrpc {
        pool: pool.clone(),
        grant_columns,
    };

    // Read once, at boot: the switch is a deploy setting (build plan WS17 S3).
    let message_search = crate::message_search::MessageSearchGrpc {
        pool: pool.clone(),
        enabled: crate::message_search::message_search_enabled(),
    };

    let session = SessionService {
        pool,
        retrieval_client,
        graph_client,
        knowledge_client,
        letta_memory,
        audit_publisher,
        auth: Some(auth.clone()),
        grant_columns,
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
        .add_service(SessionRetentionServiceServer::with_interceptor(
            retention,
            auth.clone(),
        ))
        .add_service(ThreadAutonomyServer::with_interceptor(
            thread_autonomy,
            auth.clone(),
        ))
        .add_service(MessageSearchServer::with_interceptor(
            message_search,
            auth.clone(),
        ))
        .add_service(RunServiceServer::with_interceptor(runs, auth))
        .serve(addr)
        .await?;

    Ok(())
}

/// The service scope the org-admin read of waived-run counts needs. A user
/// credential never satisfies it, even one that carries the claim: the check
/// requires a service principal. Granting it is a registry change, made apart
/// from this crate.
pub const WAIVED_RUN_COUNT_SCOPE: &str = "session:retention:read";

/// The one query behind `CountWaivedRuns`. Its predicate spells the class as a
/// literal rather than a bound parameter, so the partial index
/// `runs_zdr_waived_org_connection_idx` (migration 0045) is the index for it at
/// plan time. Every waived row names its connection
/// (`runs_retention_waiver_consistent`), so the grouping never sees a NULL.
const COUNT_WAIVED_RUNS_SQL: &str =
    "SELECT waiver_connection_id, user_id, COUNT(*), MAX(created_at)
     FROM runs
     WHERE org_id = $1 AND retention_class = 'zdr_waived'
     GROUP BY waiver_connection_id, user_id
     ORDER BY waiver_connection_id, user_id";

/// `SessionRetentionService`: read-only waived-run counts for the org admin
/// view. It is served behind the verifying interceptor like every service here,
/// and it stays beside `SessionCore`, so that trait and its mocks are unchanged.
pub struct SessionRetentionGrpc {
    pool: PgPool,
}

#[tonic::async_trait]
impl SessionRetentionService for SessionRetentionGrpc {
    async fn count_waived_runs(
        &self,
        request: Request<pb::CountWaivedRunsRequest>,
    ) -> Result<Response<pb::CountWaivedRunsResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::CountWaivedRunsResponse>, Status> = async {
            let caller = identity(&request)?;
            // A service credential holding the read scope, and nothing else. This
            // is not `authorize_operation`, which lets every user credential
            // through: the gateway's admin path checks the org-admin role itself.
            caller.require_service_scope(WAIVED_RUN_COUNT_SCOPE)?;
            let req = request.into_inner();
            caller.authorize_org(&req.org_id)?;
            let counts = count_waived_runs(&self.pool, &req.org_id).await?;
            Ok(Response::new(pb::CountWaivedRunsResponse { counts }))
        }
        .await;
        record_metrics("count_waived_runs", started, result.is_ok());
        result
    }
}

/// The columns migration 0050 adds to `threads`.
const THREAD_GRANT_COLUMN_NAMES: [&str; 5] = [
    "granted_rung",
    "granted_justification",
    "granted_plan_id",
    "granted_at",
    "granted_expires_at",
];

/// The thread read with the grant columns, and the same read without them.
const THREAD_GRANT_SELECT: &str = "SELECT org_id, user_id, COALESCE(space_id, ''),
                        granted_rung, granted_justification, granted_plan_id, granted_expires_at
                   FROM threads WHERE id = $1";
const THREAD_GRANT_SELECT_WITHOUT_COLUMNS: &str = "SELECT org_id, user_id, COALESCE(space_id, ''),
                        NULL::text, NULL::text, NULL::text, NULL::timestamptz
                   FROM threads WHERE id = $1";

/// Whether the thread grant columns of migration 0050 exist in this database.
///
/// Decided once when session-core starts ([`ThreadGrantColumns::detect`]), because the
/// documented revert of 0050 drops them while this service may be running a build that
/// writes them. Without the gate every grant-bearing `SetRunMode` would fail with "column
/// granted_rung does not exist", and the run update that shares its transaction would roll
/// back with it; `GetThreadGrant` would fail too. With it, a stated grant is skipped with a
/// warning (the run update proceeds) and the read answers "no grant" without naming the
/// columns. The check runs at boot, so the revert is: turn the gateway flag off, drop the
/// columns, restart session-core.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThreadGrantColumns {
    present: bool,
}

impl ThreadGrantColumns {
    /// The columns exist (a database migrated through 0050, the normal case).
    #[must_use]
    pub const fn present() -> Self {
        Self { present: true }
    }

    /// The columns do not exist (0050 reverted).
    #[must_use]
    pub const fn absent() -> Self {
        Self { present: false }
    }

    /// The answer for `found` of the five columns being in the table. All five or none of the
    /// gate: a partly reverted table is not one this build can write a grant to.
    #[must_use]
    pub const fn from_found(found: i64) -> Self {
        if found == THREAD_GRANT_COLUMN_NAMES.len() as i64 {
            Self::present()
        } else {
            Self::absent()
        }
    }

    /// Whether a stated grant is written to the thread.
    #[must_use]
    pub const fn records_grants(self) -> bool {
        self.present
    }

    /// The thread read for `GetThreadGrant`: it never names a column that is not there.
    #[must_use]
    pub const fn thread_grant_select(self) -> &'static str {
        if self.present {
            THREAD_GRANT_SELECT
        } else {
            THREAD_GRANT_SELECT_WITHOUT_COLUMNS
        }
    }

    /// Look the columns up in `information_schema`, in `schema` or, with `None`, in the
    /// connection's current schema. A lookup that fails answers "present": the behaviour of
    /// every earlier build, which fails loudly where a guess of "absent" would drop grants
    /// silently.
    pub async fn detect_in(pool: &PgPool, schema: Option<&str>) -> Self {
        let names: Vec<String> = THREAD_GRANT_COLUMN_NAMES
            .iter()
            .map(|name| (*name).to_owned())
            .collect();
        let found: Result<i64, sqlx::Error> = sqlx::query_scalar(
            "SELECT COUNT(*) FROM information_schema.columns
              WHERE table_schema = COALESCE($2::text, current_schema())
                AND table_name = 'threads'
                AND column_name::text = ANY($1::text[])",
        )
        .bind(&names)
        .bind(schema)
        .fetch_one(pool)
        .await;
        match found {
            Ok(found) => {
                let columns = Self::from_found(found);
                if !columns.present {
                    warn!(
                        found,
                        "the thread grant columns of migration 0050 are absent: plan grants are \
                         not recorded on threads and GetThreadGrant answers no grant"
                    );
                }
                columns
            }
            Err(error) => {
                warn!(%error, "could not look up the thread grant columns; assuming they exist");
                Self::present()
            }
        }
    }

    /// [`detect_in`](Self::detect_in) for the connection's current schema: the boot check.
    pub async fn detect(pool: &PgPool) -> Self {
        Self::detect_in(pool, None).await
    }
}

/// A grant is a durable record on the thread (rung, free-text justification, plan id): it is
/// refused for a zero-retention credential by the `session:write` gate before this runs, and
/// refused here for a WAIVED credential, which passes that gate (a persistent token of a ZDR
/// org with `zdr` false and a waiver). A mode change that states no grant is not refused here.
/// The same refusal and status as `record_run_context`.
#[allow(clippy::result_large_err)]
fn refuse_waived_grant(caller: &VerifiedIdentity, granted_rung: i32) -> Result<(), Status> {
    let stated = pb::AutonomyRung::try_from(granted_rung)
        .map_or(false, |rung| rung != pb::AutonomyRung::Unspecified);
    if stated && caller.zdr_waiver().is_some() {
        return Err(Refusal::DURABLE_MUTATION_FORBIDDEN.grpc_status(tonic::Code::FailedPrecondition));
    }
    Ok(())
}

/// How long a plan grant is good for on its thread. The gateway's in-memory grants
/// last the same (`MAX_PLAN_TTL_SECS` in model-gateway's coordinator), so reading the
/// durable grant does not change how long an approval authorizes.
const THREAD_GRANT_TTL_SECS: i64 = 86_400;

/// Longest plan id a grant records. Plan ids are short generated ids; the bound is
/// the same one every id column of this service is held to.
const MAX_GRANT_PLAN_ID_BYTES: usize = 256;

/// The plan a grant names, checked before anything is written. An id with no grant is
/// refused: a plan alone authorizes nothing, and recording one would make a later reader
/// believe an approval happened.
#[allow(clippy::result_large_err)]
fn validate_grant_plan_id(plan_id: &str, grant_stated: bool) -> Result<(), Status> {
    if plan_id.is_empty() {
        return Ok(());
    }
    if !grant_stated {
        return Err(Status::invalid_argument(
            "approved_plan_id needs granted_rung: a plan alone grants nothing",
        ));
    }
    if plan_id.len() > MAX_GRANT_PLAN_ID_BYTES || plan_id.chars().any(char::is_control) {
        return Err(Status::invalid_argument("approved_plan_id is not a valid plan id"));
    }
    Ok(())
}

/// Record an approved grant on its thread, inside the caller's transaction. The newest
/// approval replaces the previous one, exactly as the gateway's in-memory grant did, and
/// the expiry is counted from now by the database clock. Org-scoped: a thread of another
/// org matches nothing.
#[allow(clippy::result_large_err)]
async fn record_thread_grant(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: &str,
    thread_id: &str,
    grant: &mp_contracts::autonomy::AutonomyEscalation,
    plan_id: &str,
) -> Result<(), Status> {
    let updated = sqlx::query(
        "UPDATE threads
            SET granted_rung = $3,
                granted_justification = $4,
                granted_plan_id = $5,
                granted_at = now(),
                granted_expires_at = now() + ($6::bigint * interval '1 second')
          WHERE id = $1 AND org_id = $2",
    )
    .bind(thread_id)
    .bind(org_id)
    .bind(mp_contracts::autonomy::label(grant.to()))
    .bind(grant.justification())
    .bind(plan_id)
    .bind(THREAD_GRANT_TTL_SECS)
    .execute(&mut **tx)
    .await
    .map_err(|e| {
        warn!(error = %e, "record_thread_grant failed");
        Status::internal(e.to_string())
    })?;
    if updated.rows_affected() == 0 {
        return Err(Status::not_found("thread not found"));
    }
    Ok(())
}

/// The grant columns of a thread row, as stored.
struct StoredThreadGrant {
    rung: Option<String>,
    justification: Option<String>,
    plan_id: Option<String>,
    expires_at: Option<DateTime<Utc>>,
}

/// What a thread's stored grant answers at `now`: the live grant, or UNSPECIFIED with
/// nothing else set. A lapsed grant, a thread that never had one and a label this build
/// does not know all answer "no grant": the last fails closed, it never invents a rung.
fn live_thread_grant(stored: StoredThreadGrant, now: DateTime<Utc>) -> pb::GetThreadGrantResponse {
    let rung = stored
        .rung
        .as_deref()
        .and_then(mp_contracts::autonomy::rung_from_keyword)
        .filter(|rung| *rung != pb::AutonomyRung::Unspecified);
    match (rung, stored.expires_at) {
        (Some(rung), Some(expires_at)) if expires_at > now => pb::GetThreadGrantResponse {
            granted_rung: rung as i32,
            justification: stored.justification.unwrap_or_default(),
            approved_plan_id: stored.plan_id.unwrap_or_default(),
            expires_at: Some(to_proto_timestamp(expires_at)),
        },
        _ => pb::GetThreadGrantResponse::default(),
    }
}

/// `ThreadAutonomy`: the read half of the durable plan grant. Served behind the verifying
/// interceptor like every service here, and beside `SessionCore` so that trait and its
/// mocks are unchanged. The write half is `SetRunMode`.
pub struct ThreadAutonomyGrpc {
    pool: PgPool,
    /// Checked once at boot ([`ThreadGrantColumns::detect`]): without the columns the read
    /// answers "no grant" and never names them.
    grant_columns: ThreadGrantColumns,
}

#[tonic::async_trait]
impl ThreadAutonomy for ThreadAutonomyGrpc {
    async fn get_thread_grant(
        &self,
        request: Request<pb::GetThreadGrantRequest>,
    ) -> Result<Response<pb::GetThreadGrantResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::GetThreadGrantResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:read")?;
            let req = request.into_inner();
            if req.thread_id.is_empty() || req.org_id.is_empty() {
                return Err(Status::invalid_argument("thread_id and org_id are required"));
            }
            caller.authorize_org(&req.org_id)?;
            // One read serves both the ownership check and the answer. A thread of
            // another org is refused by the org check below before any grant is shown.
            let row: Option<(
                String,
                String,
                String,
                Option<String>,
                Option<String>,
                Option<String>,
                Option<DateTime<Utc>>,
            )> = sqlx::query_as(self.grant_columns.thread_grant_select())
            .bind(&req.thread_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| {
                warn!(error = %e, "get_thread_grant failed");
                Status::internal(e.to_string())
            })?;
            let (org_id, user_id, space_id, rung, justification, plan_id, expires_at) =
                row.ok_or_else(|| Status::not_found("thread not found"))?;
            authorize_thread_owner_row(&caller, &org_id, &user_id, &space_id, OwnerIntent::Read)?;
            Ok(Response::new(live_thread_grant(
                StoredThreadGrant {
                    rung,
                    justification,
                    plan_id,
                    expires_at,
                },
                Utc::now(),
            )))
        }
        .await;
        record_metrics("get_thread_grant", started, result.is_ok());
        result
    }
}

/// One org's waived runs, one row per (connection, user), each dated by its
/// latest run. The query reads receipt columns and start times only.
async fn count_waived_runs(pool: &PgPool, org_id: &str) -> Result<Vec<pb::WaivedRunCount>, Status> {
    let rows: Vec<(String, String, i64, DateTime<Utc>)> = sqlx::query_as(COUNT_WAIVED_RUNS_SQL)
        .bind(org_id)
        .fetch_all(pool)
        .await
        .map_err(|error| {
            warn!(error = %error, "count_waived_runs failed");
            Status::internal("waived-run counts are unavailable")
        })?;
    Ok(rows
        .into_iter()
        .map(
            |(connection_id, user_id, run_count, last_used_at)| pb::WaivedRunCount {
                connection_id,
                user_id,
                run_count,
                last_used_at: Some(to_proto_timestamp(last_used_at)),
            },
        )
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{
        agent_run_thread_key, append_letta_memory_rows, assemble_segments,
        assistant_reply_slot_is_open, authorize_dataplane, claim_scheduled_step_inner,
        complete_step_inner, derive_idempotency_hash, finalize_tool_action_inner,
        normalize_thread_presentation_text, pb, persist_scheduled_run_bindings,
        record_scheduled_run_result_inner,
        record_scheduled_step_receipt_inner, reserve_tool_action_inner, resolve_dataplane_addr,
        resolve_residency, resolve_thread_origin, run_origin_at_birth,
        scheduled_run_execution_payload_digest,
        semantic_context_search_status, support_thread_id, thread_append_payload_digest,
        thread_create_payload_digest, valid_scheduled_run_identifier,
        valid_scheduled_step_digest, validate_append_space_context_shape,
        validate_scheduled_run_result_request, validate_scheduled_step_request,
        thread_read_payload_digest, validate_thread_space_context_shape, validate_user_checkpoint,
        CONTROL_THREAD_READ_ACTION, CONTROL_THREAD_READ_SCHEMA,
        verify_thread_read_space_decision, verify_thread_read_space_decision_with_key,
        ControlSpaceDecisionClaims,
        verify_append_space_decision_with_key, verify_scheduled_run_decision_with_keys,
        verify_scheduled_run_execution_decision_with_keys, verify_thread_space_decision_with_key,
        verify_thread_space_decision_with_keys, AssemblyInputs, DelegatedDataPlaneBearer,
        LettaMemoryAdapter, MemoryRetention, ScheduledRunThreadContext,
        SemanticContextSearchStatus, VerifiedIdentity, CONTROL_SPACE_DECISION_AUDIENCE,
        CONTROL_SPACE_DECISION_VERSION, CONTROL_THREAD_APPEND_ACTION, CONTROL_THREAD_CREATE_ACTION,
        DEFAULT_RESIDENCY, HEALTH_SERVICE_NAMES, MAX_USER_CHECKPOINT_ID_BYTES,
        MAX_USER_CHECKPOINT_STATE_BYTES, MESSAGE_APPENDED_TYPE_URL, STEP_COMPLETED_TYPE_URL,
        THREAD_ORIGINS, THREAD_TITLE_MAX_CHARS, ZDR_MEMORY_READ_SUPPRESSED,
    };
    use crate::auth::{SPACE_DELETION_SCOPE, SPACE_DELETION_SERVICE};
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use chrono::{DateTime, Utc};
    use ed25519_dalek::{Signer as _, SigningKey, VerifyingKey};
    use mp_contracts::model_plane::v1::session_core_server::SessionCore;
    use sha2::{Digest as _, Sha256};
    use std::collections::BTreeMap;
    use tonic::Request;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // WS03 S8 review finding: RecordOrchestrationEvent persists RUN_PARKED and
    // RUN_RESUMED rows (orchestration_grpc.rs), but no `EventType` value exists
    // for them, so the durable replay reads them back as EVENT_TYPE_UNSPECIFIED
    // and the gateway's projection drops them. That is today's behaviour, pinned
    // here so nobody describes the persistence as a durable replay. Giving them
    // a ChatEvent is the design's S8-7/S8-8 and WS15 S7; when that adds enum
    // values this test is the one to change.
    #[test]
    fn replay_maps_the_park_event_types_to_unspecified() {
        for stored in ["RUN_PARKED", "RUN_RESUMED"] {
            assert_eq!(super::event_type_to_i32(stored), 0, "{stored}");
            let kind = pb::EventType::try_from(super::event_type_to_i32(stored))
                .expect("0 is a valid EventType");
            assert_eq!(kind, pb::EventType::Unspecified, "{stored}");
            assert_eq!(kind.as_str_name(), "EVENT_TYPE_UNSPECIFIED", "{stored}");
        }
    }

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

    #[test]
    fn scheduled_run_result_shape_is_strict_and_counts_withheld_effects() {
        let mut request = pb::RecordScheduledRunResultRequest {
            run_id: "run-1".to_owned(),
            step_id: "step-1".to_owned(),
            org_id: "org-1".to_owned(),
            idempotency_key: "fire-1:step:0".to_owned(),
            receipt_id: "scheduled_step_1".to_owned(),
            status: "completed".to_owned(),
            output: "answer".to_owned(),
            ..Default::default()
        };
        assert_eq!(
            validate_scheduled_run_result_request(&request).expect("valid result"),
            0
        );
        request.withheld_effects = 2;
        assert_eq!(
            validate_scheduled_run_result_request(&request).expect("counted"),
            2
        );
        request.withheld_effects = u32::MAX;
        assert!(validate_scheduled_run_result_request(&request).is_err());
        request.withheld_effects = 0;
        request.status = "failed".to_owned();
        assert!(
            validate_scheduled_run_result_request(&request).is_err(),
            "an answer is only persisted for a completed step"
        );
        request.output.clear();
        assert!(validate_scheduled_run_result_request(&request).is_ok());
        request.status = "unknown_outcome".to_owned();
        assert!(
            validate_scheduled_run_result_request(&request).is_err(),
            "the unknown flag must agree with the status"
        );
        request.unknown_outcome = true;
        assert!(validate_scheduled_run_result_request(&request).is_ok());
        request.run_id = "*".to_owned();
        assert!(validate_scheduled_run_result_request(&request).is_err());
    }

    /// Seed a service-owned scheduled thread and run, then claim its one step.
    /// Returns (run_id, thread_id, org_id, step_id, idempotency_key, receipt_id).
    async fn seeded_claimed_scheduled_step(
        pool: &sqlx::PgPool,
        suffix: &str,
    ) -> (String, String, String, String, String, String) {
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
              resource_authorization_ref, authority_revision, origin)
             VALUES ($1,$2,$3,'service:orchestrator-core',$4,$5,$6,1,$7,$8,$9,1,'space')",
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
        .execute(pool)
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
        .execute(pool)
        .await
        .expect("seed scheduled run");
        let claimed = claim_scheduled_step_inner(
            pool,
            pb::ClaimScheduledStepRequest {
                run_id: run_id.clone(),
                thread_id: thread_id.clone(),
                org_id: org_id.clone(),
                space_id,
                schedule_id,
                fire_key,
                template_digest,
                step_id: step_id.clone(),
                step_index: 0,
                policy_digest,
                idempotency_key: idempotency_key.clone(),
            },
        )
        .await
        .expect("claim scheduled step")
        .into_inner();
        (
            run_id,
            thread_id,
            org_id,
            step_id,
            idempotency_key,
            claimed.receipt_id,
        )
    }

    /// The schedule and fire row a claimed step belongs to, so the fire settles.
    async fn seed_scheduled_fire(pool: &sqlx::PgPool, suffix: &str) {
        let schedule_id = format!("schedule-{suffix}");
        sqlx::query(
            "INSERT INTO cron_schedules (id, name, schedule_expr, created_by)
             VALUES ($1, $1, '0 9 * * *', 'test')",
        )
        .bind(&schedule_id)
        .execute(pool)
        .await
        .expect("seed schedule");
        sqlx::query(
            "INSERT INTO cron_fires (id, schedule_id, status, fire_key)
             VALUES ($1, $2, 'pending', $3)",
        )
        .bind(format!("fire-row-{suffix}"))
        .bind(&schedule_id)
        .bind(format!("fire-{suffix}"))
        .execute(pool)
        .await
        .expect("seed fire");
    }

    /// Unit proof for WS20 S2: a person's run is stamped chat at birth, and a system
    /// owner's run carries no stamp, so a scheduled run takes its origin from its binding.
    #[test]
    fn run_origin_is_stamped_at_birth_for_people_only() {
        assert_eq!(
            run_origin_at_birth("user-123"),
            serde_json::json!({"origin": "chat"})
        );
        assert_eq!(
            run_origin_at_birth("service:orchestrator-core"),
            serde_json::json!({})
        );
    }

    /// The immutable binding a scheduled run must carry, as `persist_scheduled_run_bindings`
    /// reads it back. Without the `origin` key: that is the shape a legacy run has.
    fn scheduled_binding_metadata(suffix: &str) -> serde_json::Value {
        serde_json::json!({
            "source": "scheduled_run",
            "schedule_id": format!("schedule-{suffix}"),
            "fire_key": format!("fire-{suffix}"),
            "space_id": format!("scheduled-space-{suffix}"),
            "subject_id": format!("subject-{suffix}"),
            "template_digest": format!("sha256:{}", "a".repeat(64)),
            "policy_digest": format!("sha256:{}", "b".repeat(64)),
            "idempotency_key": format!("fire-{suffix}:step:0"),
        })
    }

    /// The bind request matching `scheduled_binding_metadata`, for the same run and thread.
    fn scheduled_bind_request(
        run_id: &str,
        thread_id: &str,
        org_id: &str,
        suffix: &str,
    ) -> pb::StartScheduledRunRequest {
        pb::StartScheduledRunRequest {
            thread_id: thread_id.to_owned(),
            run_id: run_id.to_owned(),
            org_id: org_id.to_owned(),
            schedule_id: format!("schedule-{suffix}"),
            fire_key: format!("fire-{suffix}"),
            goal: "scheduled proof goal".to_owned(),
            human_subject_id: format!("subject-{suffix}"),
            template_digest: format!("sha256:{}", "a".repeat(64)),
            policy_digest: format!("sha256:{}", "b".repeat(64)),
            idempotency_key: format!("fire-{suffix}:step:0"),
            ..Default::default()
        }
    }

    /// Seeds a scheduled thread and a bare run owned by Orchestrator Core carrying
    /// `metadata`, so `persist_scheduled_run_bindings` has a run to bind.
    async fn seeded_unbound_scheduled_run(
        pool: &sqlx::PgPool,
        suffix: &str,
        metadata: serde_json::Value,
    ) -> (String, String, String) {
        let run_id = format!("scheduled-run-{suffix}");
        let thread_id = format!("scheduled-thread-{suffix}");
        let org_id = format!("scheduled-org-{suffix}");
        let space_id = format!("scheduled-space-{suffix}");
        let audience_hash = format!("sha256:{}", "c".repeat(64));
        sqlx::query(
            "INSERT INTO threads
             (id, session_key, org_id, user_id, space_id, space_decision_ref,
              recipient_audience_ref, recipient_audience_revision,
              recipient_audience_hash, privacy_policy_ref,
              resource_authorization_ref, authority_revision, origin)
             VALUES ($1,$2,$3,'service:orchestrator-core',$4,$5,$6,1,$7,$8,$9,1,'space')",
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
        .execute(pool)
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
        .execute(pool)
        .await
        .expect("seed scheduled run");
        (run_id, thread_id, org_id)
    }

    /// Real-Postgres proof for WS20 S2: a scheduled binding stamps `origin: schedule`
    /// into the run's immutable metadata, and a legacy run that carries the exact
    /// binding but no stamp takes it in place, keeping every other key.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn scheduled_run_binding_stamps_origin() {
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
        let (run_id, thread_id, org_id) =
            seeded_unbound_scheduled_run(&pool, &suffix, serde_json::json!({})).await;
        persist_scheduled_run_bindings(
            &pool,
            &scheduled_bind_request(&run_id, &thread_id, &org_id, &suffix),
        )
        .await
        .expect("a fresh scheduled run binds");
        let origin: String =
            sqlx::query_scalar("SELECT metadata->>'origin' FROM runs WHERE id = $1")
                .bind(&run_id)
                .fetch_one(&pool)
                .await
                .expect("read origin");
        assert_eq!(origin, "schedule");

        let legacy = mp_ids::new_ulid();
        let (legacy_run, legacy_thread, legacy_org) =
            seeded_unbound_scheduled_run(&pool, &legacy, scheduled_binding_metadata(&legacy))
                .await;
        persist_scheduled_run_bindings(
            &pool,
            &scheduled_bind_request(&legacy_run, &legacy_thread, &legacy_org, &legacy),
        )
        .await
        .expect("a legacy run with the exact binding takes the stamp");
        let (stamped, schedule_id): (String, String) = sqlx::query_as(
            "SELECT metadata->>'origin', metadata->>'schedule_id' FROM runs WHERE id = $1",
        )
        .bind(&legacy_run)
        .fetch_one(&pool)
        .await
        .expect("read legacy metadata");
        assert_eq!(stamped, "schedule");
        assert_eq!(schedule_id, format!("schedule-{legacy}"));
    }

    /// Real-Postgres proof for WS20 S2: a run already stamped with another origin is
    /// never rebound to a schedule, and its stamp is left as it was. A run already
    /// stamped `schedule` with the same binding still binds, so a retry is idempotent.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn a_run_cannot_be_rebound_to_another_origin() {
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
        let mut chat_metadata = scheduled_binding_metadata(&suffix);
        chat_metadata["origin"] = serde_json::json!("chat");
        let (run_id, thread_id, org_id) =
            seeded_unbound_scheduled_run(&pool, &suffix, chat_metadata).await;
        let status = persist_scheduled_run_bindings(
            &pool,
            &scheduled_bind_request(&run_id, &thread_id, &org_id, &suffix),
        )
        .await
        .expect_err("a chat-stamped run must not bind as a schedule");
        assert_eq!(status.code(), tonic::Code::PermissionDenied);
        let origin: String =
            sqlx::query_scalar("SELECT metadata->>'origin' FROM runs WHERE id = $1")
                .bind(&run_id)
                .fetch_one(&pool)
                .await
                .expect("read origin");
        assert_eq!(origin, "chat");

        let retry = mp_ids::new_ulid();
        let mut scheduled_metadata = scheduled_binding_metadata(&retry);
        scheduled_metadata["origin"] = serde_json::json!("schedule");
        let (retry_run, retry_thread, retry_org) =
            seeded_unbound_scheduled_run(&pool, &retry, scheduled_metadata).await;
        persist_scheduled_run_bindings(
            &pool,
            &scheduled_bind_request(&retry_run, &retry_thread, &retry_org, &retry),
        )
        .await
        .expect("a retry of an already-stamped schedule binds again");
    }

    /// Real-Postgres proof for WS20 S1: a scheduled answer is appended to its thread
    /// once, the receipt completes with a digest, a replay returns the same message,
    /// and a different terminal outcome cannot overwrite the first one.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn record_scheduled_run_result_appends_once_and_completes_receipt() {
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
        let (run_id, thread_id, org_id, step_id, idempotency_key, receipt_id) =
            seeded_claimed_scheduled_step(&pool, &suffix).await;
        seed_scheduled_fire(&pool, &suffix).await;
        let request = pb::RecordScheduledRunResultRequest {
            run_id: run_id.clone(),
            step_id: step_id.clone(),
            org_id: org_id.clone(),
            idempotency_key: idempotency_key.clone(),
            receipt_id: receipt_id.clone(),
            status: "completed".to_owned(),
            output: "the scheduled answer".to_owned(),
            ..Default::default()
        };

        let first = record_scheduled_run_result_inner(&pool, request.clone())
            .await
            .expect("first result")
            .into_inner();
        assert_eq!(first.receipt_id, receipt_id);
        assert_eq!(first.status, "completed");
        assert!(!first.message_id.is_empty());
        let replay = record_scheduled_run_result_inner(&pool, request)
            .await
            .expect("replayed result")
            .into_inner();
        assert_eq!(replay.message_id, first.message_id);

        let (answers,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM messages
             WHERE thread_id = $1 AND role = 'assistant' AND metadata->>'run_id' = $2",
        )
        .bind(&thread_id)
        .bind(&run_id)
        .fetch_one(&pool)
        .await
        .expect("count answers");
        assert_eq!(answers, 1);
        let (content, scope): (String, String) = sqlx::query_as(
            "SELECT content, metadata->>'source_scope' FROM messages WHERE id = $1",
        )
        .bind(&first.message_id)
        .fetch_one(&pool)
        .await
        .expect("read answer");
        assert_eq!(content, "the scheduled answer");
        assert_eq!(scope, "scheduled_run");
        let (digest,): (String,) =
            sqlx::query_as("SELECT output_digest FROM scheduled_step_receipts WHERE receipt_id = $1")
                .bind(&receipt_id)
                .fetch_one(&pool)
                .await
                .expect("read receipt");
        assert!(valid_scheduled_step_digest(&digest));
        let (fire_status, fire_run): (String, String) = sqlx::query_as(
            "SELECT status, run_id FROM cron_fires WHERE schedule_id = $1 AND fire_key = $2",
        )
        .bind(format!("schedule-{suffix}"))
        .bind(format!("fire-{suffix}"))
        .fetch_one(&pool)
        .await
        .expect("read fire");
        assert_eq!(fire_status, "completed");
        assert_eq!(fire_run, run_id);

        let conflict = record_scheduled_run_result_inner(
            &pool,
            pb::RecordScheduledRunResultRequest {
                run_id,
                step_id,
                org_id,
                idempotency_key,
                receipt_id,
                status: "failed".to_owned(),
                error_code: "provider_error".to_owned(),
                ..Default::default()
            },
        )
        .await
        .expect_err("a different terminal outcome is refused");
        assert_eq!(conflict.code(), tonic::Code::AlreadyExists);
    }

    /// Real-Postgres proof for WS20 S1: a failed or unknown scheduled step settles its
    /// fire through the receipt when the result switch is on. An unknown outcome
    /// settles to `unknown`, never to `failed`.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn record_scheduled_step_receipt_settles_failed_and_unknown_fires() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL unset");
            return;
        };
        let pool = sqlx::PgPool::connect(&url).await.expect("connect pg");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");
        for (status, fire_status, unknown) in [
            ("failed", "failed", false),
            ("unknown_outcome", "unknown", true),
        ] {
            let suffix = mp_ids::new_ulid();
            let (run_id, thread_id, org_id, step_id, idempotency_key, receipt_id) =
                seeded_claimed_scheduled_step(&pool, &suffix).await;
            seed_scheduled_fire(&pool, &suffix).await;
            record_scheduled_step_receipt_inner(
                &pool,
                pb::RecordScheduledStepReceiptRequest {
                    run_id: run_id.clone(),
                    step_id,
                    org_id,
                    idempotency_key,
                    receipt_id,
                    status: status.to_owned(),
                    output_digest: String::new(),
                    error_code: "inference_failed".to_owned(),
                    unknown_outcome: unknown,
                },
                true,
            )
            .await
            .expect("record terminal outcome");

            let (settled, outcome, fire_run, fire_thread): (String, String, String, String) =
                sqlx::query_as(
                    "SELECT status, outcome, run_id, thread_id FROM cron_fires
                     WHERE schedule_id = $1 AND fire_key = $2",
                )
                .bind(format!("schedule-{suffix}"))
                .bind(format!("fire-{suffix}"))
                .fetch_one(&pool)
                .await
                .expect("read fire");
            assert_eq!(settled, fire_status);
            assert_eq!(outcome, status);
            assert_eq!(fire_run, run_id);
            assert_eq!(fire_thread, thread_id);
        }
    }

    /// Real-Postgres proof for WS20 S1: a zero-retention run never gets a durable
    /// scheduled answer, and the refusal writes nothing.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn record_scheduled_run_result_refuses_zdr_run() {
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
        let (run_id, thread_id, org_id, step_id, idempotency_key, receipt_id) =
            seeded_claimed_scheduled_step(&pool, &suffix).await;
        sqlx::query("UPDATE runs SET retention_class = 'zdr' WHERE id = $1")
            .bind(&run_id)
            .execute(&pool)
            .await
            .expect("mark run zero-retention");

        let refused = record_scheduled_run_result_inner(
            &pool,
            pb::RecordScheduledRunResultRequest {
                run_id: run_id.clone(),
                step_id,
                org_id,
                idempotency_key,
                receipt_id,
                status: "completed".to_owned(),
                output: "must not be stored".to_owned(),
                ..Default::default()
            },
        )
        .await
        .expect_err("zero-retention run is refused");
        assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
        let (answers,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM messages WHERE thread_id = $1 AND metadata->>'run_id' = $2",
        )
        .bind(&thread_id)
        .bind(&run_id)
        .fetch_one(&pool)
        .await
        .expect("count answers");
        assert_eq!(answers, 0);
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
              resource_authorization_ref, authority_revision, origin)
             VALUES ($1,$2,$3,'service:orchestrator-core',$4,$5,$6,1,$7,$8,$9,1,'space')",
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
            false,
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
            false,
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
            false,
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
            serde_json::json!(scheduled_run_execution_payload_digest(
                &request,
                &decoded,
                super::CONTROL_SCHEDULED_RUN_EXECUTION_SCHEMA
            ));
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

    /// mail-task spec §3.3: the inbound-email-triggered thread declares
    /// `origin="mail_task"` with no `space_id` -- like `agent_run`, it has no
    /// live session key to infer origin from, so it can only be closed by an
    /// explicit declaration, and (also like `agent_run`) it must NOT require a
    /// Space to exist.
    #[test]
    fn origin_accepts_mail_task_without_space_id() {
        let req = pb::CreateThreadRequest {
            origin: "mail_task".to_owned(),
            ..pb::CreateThreadRequest::default()
        };
        assert_eq!(resolve_thread_origin(&req).unwrap(), "mail_task");
    }

    /// Migration-content check (pattern per `0026`'s
    /// `semantic_memory_deletion_ledger_is_request_bound_...` test above): the
    /// widened CHECK constraint 0041 installs must legalize `mail_task`
    /// without dropping any of the five origins 0034 already legalized, and
    /// the validator's own allow-list (asserted immediately above and by
    /// `declared_origin_is_validated_against_the_allowed_set`) is what keeps
    /// an arbitrary 7th value illegal -- the migration and the Rust validator
    /// must agree on exactly the same six-element set.
    #[test]
    fn mail_task_origin_migration_widens_the_check_constraint_to_exactly_the_six_known_origins() {
        let migration = include_str!("../migrations/0041_mail_task_thread_origin.sql");
        assert!(migration.contains("threads_origin_chk"));
        assert!(migration.contains("'mail_task'"));
        for origin in THREAD_ORIGINS {
            assert!(
                migration.contains(&format!("'{origin}'")),
                "migration's CHECK constraint is missing existing origin {origin:?}"
            );
        }
        assert!(!migration.contains("'literally-anything'"));
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
                &sqlx::postgres::PgPoolOptions::new().connect_lazy("postgresql://localhost/unused").unwrap(),
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
            &sqlx::postgres::PgPoolOptions::new().connect_lazy("postgresql://localhost/unused").unwrap(),
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

    /// Mint a signed `model.thread.read` decision the way Control does, so the
    /// verifier is exercised against the real envelope rather than a stub.
    fn signed_thread_read_decision(
        signing_key: &SigningKey,
        now: DateTime<Utc>,
        permissions: &[&str],
        audience_revision: u64,
    ) -> (String, String, String) {
        let key_id = "control-test-key".to_owned();
        let key_part = URL_SAFE_NO_PAD.encode(key_id.as_bytes());
        let decision_ref = "read-decision-1".to_owned();
        let claims = serde_json::json!({
            "decision_ref": decision_ref,
            "org_id": "org-1",
            "space_ref": "space-room",
            "subject_id": "user-1",
            "service_audience": CONTROL_SPACE_DECISION_AUDIENCE,
            "action_id": CONTROL_THREAD_READ_ACTION,
            "action_schema_hash": CONTROL_THREAD_READ_SCHEMA,
            "payload_digest": "",
            "idempotency_key": "space-read-1",
            "recipient_audience_ref": "audience:space-room:4",
            "recipient_audience_hash": "sha256:test-audience",
            "privacy_policy_ref": "privacy:org-1:5",
            "resource_authorization_ref": "control:space-room:thread-read:7",
            "purpose": "assistant_collaboration",
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
            "recipient_audience_revision": audience_revision,
            "entitlement_revision": 3,
            "permissions": permissions,
            "issued_at": now.to_rfc3339(),
            "expires_at": (now + chrono::Duration::minutes(1)).to_rfc3339(),
        });
        // Control computes the digest over its own claims; recompute it the
        // same way so the fixture is self-consistent.
        let mut parsed: ControlSpaceDecisionClaims =
            serde_json::from_value(claims.clone()).expect("claims fixture");
        parsed.payload_digest = thread_read_payload_digest(&parsed);
        let mut claims = claims;
        claims["payload_digest"] = serde_json::json!(parsed.payload_digest);
        let encoded = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).expect("claims"));
        let signed = format!("{CONTROL_SPACE_DECISION_VERSION}.{key_part}.{encoded}");
        let signature = signing_key.sign(signed.as_bytes());
        let token = format!(
            "{signed}.{}",
            URL_SAFE_NO_PAD.encode(signature.to_bytes())
        );
        (token, decision_ref, key_id)
    }

    /// The read path's whole purpose: admit a member who does NOT own the
    /// thread, and only for the exact Space and subject Control resolved.
    #[test]
    fn signed_thread_read_decision_admits_only_its_own_space_and_subject() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-09-06T12:00:00Z")
            .expect("test time")
            .with_timezone(&Utc);
        let signing_key = SigningKey::from_bytes(&[31_u8; 32]);
        let (token, decision_ref, key_id) =
            signed_thread_read_decision(&signing_key, now, &["thread:read"], 4);

        let verified = verify_thread_read_space_decision_with_key(
            &token,
            &decision_ref,
            "org-1",
            "user-1",
            "space-room",
            &key_id,
            &signing_key.verifying_key(),
            now,
        )
        .expect("a current member's read decision must verify");
        assert_eq!(
            verified.recipient_audience_revision, 4,
            "the audience ceiling must travel with the decision"
        );

        for (org, user, space, why) in [
            ("org-2", "user-1", "space-room", "another tenant"),
            ("org-1", "user-2", "space-room", "another subject"),
            ("org-1", "user-1", "space-other", "another Space"),
        ] {
            assert_eq!(
                verify_thread_read_space_decision_with_key(
                    &token,
                    &decision_ref,
                    org,
                    user,
                    space,
                    &key_id,
                    &signing_key.verifying_key(),
                    now,
                )
                .expect_err(why)
                .code(),
                tonic::Code::PermissionDenied,
                "a read decision must not authorize {why}"
            );
        }
    }

    /// A reader's token must never be usable as a writer's. Control issues
    /// disjoint permission sets; this proves the reader refuses rather than
    /// trusting that it always will.
    #[test]
    fn thread_read_decision_refuses_write_permissions() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-09-06T12:00:00Z")
            .expect("test time")
            .with_timezone(&Utc);
        let signing_key = SigningKey::from_bytes(&[32_u8; 32]);
        let (token, decision_ref, key_id) =
            signed_thread_read_decision(&signing_key, now, &["thread:read", "thread:append"], 4);
        assert_eq!(
            verify_thread_read_space_decision_with_key(
                &token,
                &decision_ref,
                "org-1",
                "user-1",
                "space-room",
                &key_id,
                &signing_key.verifying_key(),
                now,
            )
            .expect_err("a read decision carrying append must be refused")
            .code(),
            tonic::Code::PermissionDenied
        );
    }

    /// An expired decision is not a slow one. Revocation works by letting the
    /// short lifetime run out, so accepting a stale token would quietly make
    /// removal from a room take effect never.
    #[test]
    fn thread_read_decision_expires() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-09-06T12:00:00Z")
            .expect("test time")
            .with_timezone(&Utc);
        let signing_key = SigningKey::from_bytes(&[33_u8; 32]);
        let (token, decision_ref, key_id) =
            signed_thread_read_decision(&signing_key, now, &["thread:read"], 4);
        assert_eq!(
            verify_thread_read_space_decision_with_key(
                &token,
                &decision_ref,
                "org-1",
                "user-1",
                "space-room",
                &key_id,
                &signing_key.verifying_key(),
                now + chrono::Duration::minutes(5),
            )
            .expect_err("an expired read decision must be refused")
            .code(),
            tonic::Code::PermissionDenied
        );
    }

    /// D12 wire vector. A personal Space's owner reads a scheduled thread only
    /// with a decision user-core's own-scheduled-thread fallback signs, and this
    /// verifier recomputes the payload digest and checks the claim shape on its
    /// own. The token below was issued and signed by user-core's Go code (the
    /// same literal is pinned in `spaces/own_scheduled_thread_read_wire_test.go`),
    /// so this runs the real verifier over a real Control decision instead of a
    /// Rust-built look-alike. If either side changes the encoding, one of the
    /// two tests fails.
    #[test]
    #[allow(clippy::result_large_err)] // the closure returns the verifier's `Status` error
    fn own_scheduled_thread_read_decision_issued_by_control_verifies() {
        const TOKEN: &str = "v2.Y29udHJvbC13aXJlLXZlY3Rvci1rZXk.eyJkZWNpc2lvbl9yZWYiOiJvd24tcmVhZC13aXJlLTEiLCJvcmdfaWQiOiJvcmctMSIsInNwYWNlX3JlZiI6InNwYWNlLXBlcnNvbmFsIiwic3ViamVjdF9pZCI6InVzZXItMSIsInNlcnZpY2VfYXVkaWVuY2UiOiJtb2RlbC1wbGFuZSIsImFjdGlvbl9pZCI6Im1vZGVsLnRocmVhZC5yZWFkIiwiYWN0aW9uX3NjaGVtYV9oYXNoIjoic2hhMjU2OnRocmVhZC1yZWFkLXYxIiwicGF5bG9hZF9kaWdlc3QiOiJzaGEyNTY6Y2E5YmM3ZjJiMDI5YzQzNWY1MGRiYjY1NWNmZDFmZGU3YTU4NjNhOTUwNDAzNjNlOGFlN2NmMzBkY2M5NjRiOCIsImlkZW1wb3RlbmN5X2tleSI6InNwYWNlLXJlYWQtd2lyZS0xIiwicmVjaXBpZW50X2F1ZGllbmNlX3JlZiI6ImF1ZGllbmNlOnNwYWNlLXBlcnNvbmFsOjIiLCJyZWNpcGllbnRfYXVkaWVuY2VfaGFzaCI6InNoYTI1Njp0ZXN0LWF1ZGllbmNlIiwicHJpdmFjeV9wb2xpY3lfcmVmIjoicHJpdmFjeTpvcmctMTo1IiwicmVzb3VyY2VfYXV0aG9yaXphdGlvbl9yZWYiOiJjb250cm9sOnNwYWNlLXBlcnNvbmFsOnRocmVhZC1yZWFkOjciLCJhdXRob3JpdHlfcmV2aXNpb24iOjcsIm1lbWJlcnNoaXBfcmV2aXNpb24iOjQsInByaXZhY3lfcmV2aXNpb24iOjUsInJlY2lwaWVudF9hdWRpZW5jZV9yZXZpc2lvbiI6MiwiZW50aXRsZW1lbnRfcmV2aXNpb24iOjMsInBlcm1pc3Npb25zIjpbInRocmVhZDpyZWFkIl0sInB1cnBvc2UiOiJhc3Npc3RhbnRfY29sbGFib3JhdGlvbiIsImxhd2Z1bF9iYXNpcyI6ImNvbnRyYWN0IiwicHJpdmFjeV9jbGFzcyI6ImludGVybmFsIiwidGhpcmRfcGFydHlfcHJvY2Vzc2luZ19hbGxvd2VkIjpmYWxzZSwicmV0ZW50aW9uX2NsYXNzIjoic3RhbmRhcmQiLCJyZXNpZGVuY3kiOiJzd2VkZW5jZW50cmFsIiwiZGVsZXRpb25fc2NvcGUiOiJzcGFjZSIsInplcm9fZGF0YV9yZXRlbnRpb24iOmZhbHNlLCJpc3N1ZWRfYXQiOiIyMDI2LTEwLTAxVDEyOjAwOjAwWiIsImV4cGlyZXNfYXQiOiIyMDI2LTEwLTAxVDEyOjAyOjAwWiIsIm5vbmNlIjoibm9uY2Utd2lyZS0xIn0.ChmOC42y-MpKrWV9T-CdsX8rjVeqepJ213aODu261XDk-AkAh13OxJXnhJ7lVYUXpdUxQjRIGC6qrxkp5ZVfCQ";
        const PUBLIC_KEY: &str = "lHXGzw7aNAV1KNhpR4HsySrWOdfivSd2HtPvdJJL6wc";
        const KEY_ID: &str = "control-wire-vector-key";

        let key_bytes: [u8; 32] = URL_SAFE_NO_PAD
            .decode(PUBLIC_KEY)
            .expect("public key encoding")
            .try_into()
            .expect("public key length");
        let key = VerifyingKey::from_bytes(&key_bytes).expect("public key");
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-01T12:00:30Z")
            .expect("test time")
            .with_timezone(&Utc);
        let verify = |org: &str, user: &str, space: &str, at: DateTime<Utc>| {
            verify_thread_read_space_decision_with_key(
                TOKEN,
                "own-read-wire-1",
                org,
                user,
                space,
                KEY_ID,
                &key,
                at,
            )
        };

        let verified = verify("org-1", "user-1", "space-personal", now)
            .expect("a decision Control signed for a personal Space's owner must verify");
        assert_eq!(
            verified.recipient_audience_revision, 2,
            "the audience ceiling must survive the Go to Rust round trip"
        );
        for (org, user, space, why) in [
            ("org-2", "user-1", "space-personal", "another tenant"),
            ("org-1", "user-2", "space-personal", "another person"),
            ("org-1", "user-1", "space-other", "another Space"),
        ] {
            assert_eq!(
                verify(org, user, space, now)
                    .expect_err(why)
                    .code(),
                tonic::Code::PermissionDenied,
                "the owner's decision must not open the thread for {why}"
            );
        }
        assert_eq!(
            verify(
                "org-1",
                "user-1",
                "space-personal",
                now + chrono::Duration::minutes(5)
            )
            .expect_err("expired")
            .code(),
            tonic::Code::PermissionDenied,
            "a read decision expires"
        );
    }

    /// No decision at all is the historical, owner-bound path -- not an error.
    /// Half a decision IS an error: treating it as "no token" would hide both
    /// a client bug and an attempt.
    #[test]
    fn partial_thread_read_decision_is_rejected_but_absent_one_is_not() {
        assert!(verify_thread_read_space_decision("", "", "org-1", "user-1", "")
            .expect("an absent decision must keep the owner-bound path")
            .is_none());
        assert_eq!(
            verify_thread_read_space_decision("token-without-ref", "", "org-1", "user-1", "space-room")
                .expect_err("a half-supplied decision must not be ignored")
                .code(),
            tonic::Code::InvalidArgument
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
                "model_plane.v1.SessionRetentionService",
                "model_plane.v1.ThreadAutonomy",
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

    /// A request as the mandatory interceptor hands it on: carrying the
    /// verified user the handler authorizes. Handlers fail closed without one.
    // Build plan WS02 S4: RecordRunContext stamps the assembly status onto
    // `runs.metadata.run_context` WITHOUT dropping other metadata keys, is
    // refused for a zero-retention credential before any write, and is scoped
    // to the owning org.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a Postgres with session-core migrations"]
    async fn record_run_context_merges_without_dropping_other_keys() {
        use super::SessionService;

        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL unset");
            return;
        };
        let pool = sqlx::PgPool::connect(&url).await.expect("connect pg");
        sqlx::migrate!("./migrations").run(&pool).await.expect("migrate");
        let sfx = std::process::id();
        let (thread_id, run_id, org) = (format!("rc-t-{sfx}"), format!("rc-r-{sfx}"), format!("rc-org-{sfx}"));
        sqlx::query("INSERT INTO threads (id,session_key,org_id,user_id) VALUES ($1,$1,$2,'u1')")
            .bind(&thread_id).bind(&org).execute(&pool).await.expect("seed thread");
        sqlx::query(
            "INSERT INTO runs (id,thread_id,goal,org_id,user_id,metadata) VALUES ($1,$2,'g',$3,'u1','{\"autonomy_rung\":\"workspace_write\"}'::jsonb)",
        )
        .bind(&run_id).bind(&thread_id).bind(&org).execute(&pool).await.expect("seed run");
        let svc = SessionService {
            pool: pool.clone(),
            retrieval_client: None,
            graph_client: None,
            knowledge_client: None,
            letta_memory: None,
            audit_publisher: None,
            auth: None,
            grant_columns: crate::grpc::ThreadGrantColumns::present(),
        };
        let status = pb::RunContextStatus {
            instructions: Some(pb::RunContextPart { state: pb::RunContextState::Included as i32, included: 1, omitted: 0, reason: String::new() }),
            history: Some(pb::RunContextPart { state: pb::RunContextState::Truncated as i32, included: 24, omitted: 6, reason: "cap".into() }),
            memory: Some(pb::RunContextPart { state: pb::RunContextState::SkippedByPolicy as i32, included: 0, omitted: 0, reason: "memory_off".into() }),
        };
        let request = |org: &str| pb::RecordRunContextRequest { run_id: run_id.clone(), org_id: org.to_owned(), status: Some(status.clone()) };

        svc.record_run_context(as_user(&org, "u1", request(&org))).await.expect("record_run_context");
        let (metadata,): (serde_json::Value,) = sqlx::query_as("SELECT metadata FROM runs WHERE id = $1")
            .bind(&run_id).fetch_one(&pool).await.expect("read metadata");
        assert_eq!(metadata["autonomy_rung"], "workspace_write", "an unrelated key must survive the merge");
        assert_eq!(metadata["run_context"]["history"]["state"], "truncated");
        assert_eq!(metadata["run_context"]["history"]["omitted"], 6);
        assert_eq!(metadata["run_context"]["memory"]["reason"], "memory_off");

        // A zero-retention credential never persists a run mutation.
        let mut zdr = Request::new(request(&org));
        zdr.extensions_mut().insert(VerifiedIdentity::user_for_test_with_zdr(&org, "u1", true));
        let refused = svc.record_run_context(zdr).await.expect_err("ZDR must be refused");
        // The operation scope already refuses a ZDR credential for `session:write`
        // (permission_denied); the explicit durable-mutation check behind it is
        // defence in depth for a credential that reaches it.
        assert!(
            matches!(
                refused.code(),
                tonic::Code::PermissionDenied | tonic::Code::FailedPrecondition
            ),
            "unexpected refusal: {refused:?}"
        );
        let (after,): (serde_json::Value,) = sqlx::query_as("SELECT metadata FROM runs WHERE id = $1")
            .bind(&run_id).fetch_one(&pool).await.expect("re-read metadata");
        assert_eq!(after, metadata, "a refused call must not write");

        // Another org's caller is refused by the tenant check.
        let other = format!("rc-other-{sfx}");
        let foreign = svc.record_run_context(as_user(&other, "u1", request(&org))).await;
        assert!(foreign.is_err(), "a foreign org must not stamp this run");
    }

    fn as_user<T>(org_id: &str, user_id: &str, message: T) -> Request<T> {
        let mut request = Request::new(message);
        request
            .extensions_mut()
            .insert(VerifiedIdentity::user_for_test(org_id, user_id));
        request
    }

    fn as_service<T>(org_id: &str, scopes: &[&str], message: T) -> Request<T> {
        let mut request = Request::new(message);
        request
            .extensions_mut()
            .insert(VerifiedIdentity::service_for_test_as(
                org_id,
                "service:capability-core",
                scopes,
                false,
            ));
        request
    }

    // P3 SetRunMode — durable run.mode, invalid-mode rejection, and per-org
    // isolation (a foreign org's caller is refused by the tenant check before
    // any UPDATE runs → permission_denied, no mutation).
    async fn assert_set_run_mode_isolation(
        svc: &super::SessionService,
        pool: &sqlx::PgPool,
        run_id: &str,
        org: &str,
        sfx: u32,
    ) {
        use mp_contracts::model_plane::v1 as pb;
        use mp_contracts::model_plane::v1::session_core_server::SessionCore;

        let resp = svc
            .set_run_mode(as_user(
                org,
                "u1",
                pb::SetRunModeRequest {
                    run_id: run_id.to_owned(),
                    mode: "plan".into(),
                    org_id: org.to_owned(),
                    granted_rung: 0,
                    justification: String::new(),
                    approved_plan_id: String::new(),
                },
            ))
            .await
            .expect("set_run_mode")
            .into_inner();
        assert_eq!(resp.mode, "plan");
        assert!(svc
            .set_run_mode(as_user(
                org,
                "u1",
                pb::SetRunModeRequest {
                    run_id: run_id.to_owned(),
                    mode: "bogus".into(),
                    org_id: org.to_owned(),
                    granted_rung: 0,
                    justification: String::new(),
                    approved_plan_id: String::new(),
                },
            ))
            .await
            .is_err());

        let attacker = format!("attacker-{sfx}");
        let cross = svc
            .set_run_mode(as_user(
                &attacker,
                "u1",
                pb::SetRunModeRequest {
                    run_id: run_id.to_owned(),
                    mode: "execute".into(),
                    org_id: attacker.clone(),
                    granted_rung: 0,
                    justification: String::new(),
                    approved_plan_id: String::new(),
                },
            ))
            .await;
        assert_eq!(
            cross.unwrap_err().code(),
            tonic::Code::PermissionDenied,
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

    // ---- build plan WS04 S4: the durable plan grant ----

    /// A plan id is only ever recorded WITH a grant, and only a sane one.
    #[test]
    fn a_plan_id_needs_a_stated_grant_and_a_sane_shape() {
        use super::validate_grant_plan_id;
        assert!(validate_grant_plan_id("", false).is_ok(), "no plan, no grant: an ordinary mode change");
        assert!(validate_grant_plan_id("", true).is_ok(), "a grant without a plan is a plain approval");
        assert!(validate_grant_plan_id("plan-1", true).is_ok());
        assert_eq!(
            validate_grant_plan_id("plan-1", false)
                .expect_err("a plan alone grants nothing")
                .code(),
            tonic::Code::InvalidArgument
        );
        assert!(validate_grant_plan_id(&"p".repeat(257), true).is_err(), "too long");
        assert!(validate_grant_plan_id("plan\n1", true).is_err(), "control characters");
    }

    fn stored_grant(
        rung: Option<&str>,
        expires_in_secs: Option<i64>,
        now: DateTime<Utc>,
    ) -> super::StoredThreadGrant {
        super::StoredThreadGrant {
            rung: rung.map(str::to_owned),
            justification: Some("writes the report".to_owned()),
            plan_id: Some("plan-7".to_owned()),
            expires_at: expires_in_secs.map(|secs| now + chrono::Duration::seconds(secs)),
        }
    }

    /// What a thread answers: the live grant in full, and "no grant" (UNSPECIFIED, nothing
    /// else set) for a grant that has lapsed, one never made, and a label this build does not
    /// know. The last never invents a rung.
    #[test]
    fn a_live_grant_reads_back_and_a_lapsed_or_absent_one_is_no_grant() {
        use super::live_thread_grant;
        let now = Utc::now();
        let live = live_thread_grant(stored_grant(Some("workspace_write"), Some(60), now), now);
        assert_eq!(live.granted_rung, pb::AutonomyRung::WorkspaceWrite as i32);
        assert_eq!(live.justification, "writes the report");
        assert_eq!(live.approved_plan_id, "plan-7");
        assert!(live.expires_at.is_some());

        let none = pb::GetThreadGrantResponse::default();
        for stored in [
            stored_grant(Some("workspace_write"), Some(-1), now),
            stored_grant(Some("workspace_write"), Some(0), now),
            stored_grant(Some("workspace_write"), None, now),
            stored_grant(None, Some(60), now),
            stored_grant(Some("unspecified"), Some(60), now),
            stored_grant(Some("root"), Some(60), now),
        ] {
            assert_eq!(live_thread_grant(stored, now), none);
        }
        assert_eq!(
            live_thread_grant(stored_grant(Some("danger_full_access"), Some(60), now), now)
                .granted_rung,
            pb::AutonomyRung::DangerFullAccess as i32
        );
    }

    /// `set_run_mode` passes this check before it writes anything, so a zero-retention
    /// turn records neither the run's grant nor the thread's.
    #[test]
    fn a_zdr_credential_cannot_record_a_grant() {
        let zdr = VerifiedIdentity::user_for_test_with_zdr("org-1", "user-1", true);
        assert_eq!(
            crate::auth::authorize_operation(&zdr, "session:write")
                .expect_err("set_run_mode is gated by this check before it writes anything")
                .code(),
            tonic::Code::FailedPrecondition
        );
    }

    /// WS04 S4 review defect 4: a WAIVED credential (a persistent token of a ZDR org with `zdr`
    /// false and a waiver) passes `session:write`, so the grant refusal has to be its own: a
    /// grant is a durable record (rung, free-text justification, plan id) on the thread.
    #[test]
    fn a_waived_credential_is_refused_a_stated_grant_and_only_a_stated_grant() {
        use super::refuse_waived_grant;
        let waived = VerifiedIdentity::user_for_test_with_waiver("org-1", "user-1", "conn-1", "ack-1");
        assert!(
            crate::auth::authorize_operation(&waived, "session:write").is_ok(),
            "the session:write gate does not stop a waived credential: this check has to"
        );
        for rung in [
            pb::AutonomyRung::ReadOnly,
            pb::AutonomyRung::WorkspaceWrite,
            pb::AutonomyRung::DangerFullAccess,
        ] {
            let error = refuse_waived_grant(&waived, rung as i32)
                .expect_err("a waived credential records no grant");
            assert_eq!(error.code(), tonic::Code::FailedPrecondition);
            assert_eq!(
                mp_contracts::zdr_refusal::Refusal::from_status(&error).map(|refusal| refusal.code()),
                Some(mp_contracts::zdr_refusal::code::DURABLE_MUTATION_FORBIDDEN)
            );
        }
        // No grant stated (an ordinary mode change, or a rung this build cannot name) is not a
        // grant: not refused here.
        for rung in [pb::AutonomyRung::Unspecified as i32, 99, -1] {
            assert!(refuse_waived_grant(&waived, rung).is_ok(), "rung {rung}");
        }
        // An ordinary credential is never refused by this check.
        let ordinary = VerifiedIdentity::user_for_test("org-1", "user-1");
        assert!(refuse_waived_grant(&ordinary, pb::AutonomyRung::WorkspaceWrite as i32).is_ok());
    }

    /// The same refusal through the handler: it comes before the run lookup, so a waived
    /// credential that states a grant is refused without touching the database.
    #[tokio::test]
    async fn set_run_mode_refuses_a_waived_credential_that_states_a_grant() {
        use mp_contracts::model_plane::v1::session_core_server::SessionCore;
        let svc = lazy_test_service();
        let mut request = Request::new(pb::SetRunModeRequest {
            run_id: "run-1".into(),
            mode: "execute".into(),
            org_id: "org-1".into(),
            granted_rung: pb::AutonomyRung::WorkspaceWrite as i32,
            justification: "writes the report into the workspace".into(),
            approved_plan_id: "plan-1".into(),
        });
        request.extensions_mut().insert(VerifiedIdentity::user_for_test_with_waiver(
            "org-1", "user-1", "conn-1", "ack-1",
        ));
        let error = svc.set_run_mode(request).await.expect_err("a waived credential records no grant");
        assert_eq!(error.code(), tonic::Code::FailedPrecondition);
        assert_eq!(
            mp_contracts::zdr_refusal::Refusal::from_status(&error).map(|refusal| refusal.code()),
            Some(mp_contracts::zdr_refusal::code::DURABLE_MUTATION_FORBIDDEN)
        );
    }

    /// WS04 S4 review defect 2: the gate on the thread grant columns. Present, a stated grant
    /// is written and the read names the columns; absent (0050 reverted), the write is skipped
    /// and the read never names a column that is not there, so it answers "no grant" instead of
    /// failing the statement (and, for the write, rolling the run update back with it).
    #[test]
    fn the_grant_columns_gate_decides_the_write_and_the_read() {
        use super::{ThreadGrantColumns, THREAD_GRANT_COLUMN_NAMES};
        let present = ThreadGrantColumns::present();
        let absent = ThreadGrantColumns::absent();
        assert!(present.records_grants());
        assert!(!absent.records_grants());

        // All five columns, or the gate is shut: a partly reverted table is not writable.
        assert_eq!(ThreadGrantColumns::from_found(5), present);
        for found in [0, 1, 4, 6] {
            assert_eq!(ThreadGrantColumns::from_found(found), absent, "{found} columns found");
        }

        for column in THREAD_GRANT_COLUMN_NAMES {
            assert!(
                present.thread_grant_select().contains(column) || column == "granted_at",
                "{column}"
            );
            assert!(
                !absent.thread_grant_select().contains(column),
                "the read without the columns never names {column}"
            );
        }
        // The two reads answer the same shape, so the handler maps either one.
        assert_eq!(
            present.thread_grant_select().matches(',').count(),
            absent.thread_grant_select().matches(',').count()
        );
        // Without the columns the stored row is "never had a grant": the read answers no grant.
        let none = super::live_thread_grant(
            super::StoredThreadGrant {
                rung: None,
                justification: None,
                plan_id: None,
                expires_at: None,
            },
            Utc::now(),
        );
        assert_eq!(none, pb::GetThreadGrantResponse::default());
    }

    /// DB-gated: the gate against a database that really has no `granted_*` columns. A scratch
    /// schema holds a `threads` table without them, so nothing in the real tables is dropped.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a Postgres with session-core migrations"]
    async fn the_column_check_finds_the_columns_where_they_are_and_not_where_they_are_not() {
        use super::ThreadGrantColumns;
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL unset");
            return;
        };
        let pool = sqlx::PgPool::connect(&url).await.expect("connect pg");
        sqlx::migrate!("./migrations").run(&pool).await.expect("migrate");
        assert_eq!(
            ThreadGrantColumns::detect(&pool).await,
            ThreadGrantColumns::present(),
            "a database migrated through 0050 has the columns"
        );
        let scratch = format!("grant_cols_{}", std::process::id());
        sqlx::query(&format!("CREATE SCHEMA {scratch}")).execute(&pool).await.expect("schema");
        sqlx::query(&format!("CREATE TABLE {scratch}.threads (id TEXT PRIMARY KEY)"))
            .execute(&pool)
            .await
            .expect("table");
        let without = ThreadGrantColumns::detect_in(&pool, Some(&scratch)).await;
        sqlx::query(&format!("DROP SCHEMA {scratch} CASCADE")).execute(&pool).await.expect("drop");
        assert_eq!(without, ThreadGrantColumns::absent(), "a threads table without the columns");
    }

    /// DB-gated: SetRunMode and GetThreadGrant with the gate shut, against a database that has
    /// the columns (the gate is what is under test, so the columns stay and are checked). The
    /// run update proceeds (mode and the run's own audit copy), the thread keeps no grant, and
    /// the read answers "no grant" even for a grant row written directly. A waived credential
    /// that states a grant changes nothing either.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a Postgres with session-core migrations"]
    #[allow(clippy::too_many_lines)]
    async fn set_run_mode_without_the_grant_columns_still_updates_the_run_against_real_pg() {
        use super::{SessionService, ThreadAutonomyGrpc, ThreadGrantColumns};
        use mp_contracts::model_plane::v1::session_core_server::SessionCore;
        use mp_contracts::model_plane::v1::thread_autonomy_server::ThreadAutonomy;

        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL unset");
            return;
        };
        let pool = sqlx::PgPool::connect(&url).await.expect("connect pg");
        sqlx::migrate!("./migrations").run(&pool).await.expect("migrate");
        let sfx = format!("nocol-{}", std::process::id());
        let (thread_id, run_id, org) = (format!("t-{sfx}"), format!("r-{sfx}"), format!("org-{sfx}"));
        sqlx::query("INSERT INTO threads (id,session_key,org_id,user_id) VALUES ($1,$1,$2,'u1')")
            .bind(&thread_id).bind(&org).execute(&pool).await.expect("seed thread");
        sqlx::query("INSERT INTO runs (id,thread_id,goal,org_id,user_id) VALUES ($1,$2,'g',$3,'u1')")
            .bind(&run_id).bind(&thread_id).bind(&org).execute(&pool).await.expect("seed run");
        let svc = SessionService {
            pool: pool.clone(),
            retrieval_client: None,
            graph_client: None,
            knowledge_client: None,
            letta_memory: None,
            audit_publisher: None,
            auth: None,
            grant_columns: ThreadGrantColumns::absent(),
        };
        let approve = |user: &str| {
            as_user(
                &org,
                user,
                pb::SetRunModeRequest {
                    run_id: run_id.clone(),
                    mode: "execute".into(),
                    org_id: org.clone(),
                    granted_rung: pb::AutonomyRung::WorkspaceWrite as i32,
                    justification: "writes the report into the workspace".into(),
                    approved_plan_id: "plan-1".into(),
                },
            )
        };
        let response = svc.set_run_mode(approve("u1")).await.expect("the run update proceeds").into_inner();
        assert_eq!(response.mode, "execute");
        let (mode, rung): (String, Option<String>) = sqlx::query_as(
            "SELECT r.mode, t.granted_rung FROM runs r JOIN threads t ON t.id = r.thread_id WHERE r.id = $1",
        )
        .bind(&run_id)
        .fetch_one(&pool)
        .await
        .expect("read back");
        assert_eq!(mode, "execute", "the run update was not rolled back");
        assert_eq!(rung, None, "the thread recorded no grant while the gate was shut");
        let (audit,): (Option<String>,) =
            sqlx::query_as("SELECT metadata->>'autonomy_rung' FROM runs WHERE id = $1")
                .bind(&run_id)
                .fetch_one(&pool)
                .await
                .expect("read metadata");
        assert_eq!(audit.as_deref(), Some("workspace_write"), "the run keeps its own audit copy");

        // A grant row that exists is not read while the gate is shut.
        sqlx::query(
            "UPDATE threads SET granted_rung='danger_full_access', granted_justification='x',
                    granted_plan_id='p', granted_at=now(), granted_expires_at=now()+interval '1 hour'
              WHERE id=$1",
        )
        .bind(&thread_id)
        .execute(&pool)
        .await
        .expect("plant a grant row");
        let reader = ThreadAutonomyGrpc { pool: pool.clone(), grant_columns: ThreadGrantColumns::absent() };
        let read = reader
            .get_thread_grant(as_user(
                &org,
                "u1",
                pb::GetThreadGrantRequest { thread_id: thread_id.clone(), org_id: org.clone() },
            ))
            .await
            .expect("the read answers without naming the columns")
            .into_inner();
        assert_eq!(read.granted_rung, pb::AutonomyRung::Unspecified as i32);
        // The ownership check still applies with the gate shut.
        let stranger = reader
            .get_thread_grant(as_user(
                &org,
                "u2",
                pb::GetThreadGrantRequest { thread_id: thread_id.clone(), org_id: org.clone() },
            ))
            .await
            .unwrap_err();
        assert_eq!(stranger.code(), tonic::Code::PermissionDenied);

        // Defect 4: a waived credential that states a grant is refused, with the gate open too.
        let open = SessionService { grant_columns: ThreadGrantColumns::present(), ..svc };
        let mut waived = approve("u1");
        waived
            .extensions_mut()
            .insert(VerifiedIdentity::user_for_test_with_waiver(&org, "u1", "conn-1", "ack-1"));
        assert_eq!(
            open.set_run_mode(waived).await.unwrap_err().code(),
            tonic::Code::FailedPrecondition,
            "a waived credential records no grant"
        );
        let (justification,): (Option<String>,) =
            sqlx::query_as("SELECT granted_justification FROM threads WHERE id = $1")
                .bind(&thread_id)
                .fetch_one(&pool)
                .await
                .expect("read thread");
        assert_eq!(justification.as_deref(), Some("x"), "the refused waived call wrote nothing");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a Postgres with session-core migrations"]
    #[allow(clippy::too_many_lines)]
    async fn the_plan_grant_is_written_with_the_approval_and_read_back_against_real_pg() {
        use super::{SessionService, ThreadAutonomyGrpc};
        use mp_contracts::model_plane::v1::session_core_server::SessionCore;
        use mp_contracts::model_plane::v1::thread_autonomy_server::ThreadAutonomy;

        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL unset");
            return;
        };
        let pool = sqlx::PgPool::connect(&url).await.expect("connect pg");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");

        let sfx = format!("grant-{}", std::process::id());
        let (thread_id, other_thread, run_id, org) = (
            format!("t-{sfx}"),
            format!("t2-{sfx}"),
            format!("r-{sfx}"),
            format!("org-{sfx}"),
        );
        for id in [&thread_id, &other_thread] {
            sqlx::query(
                "INSERT INTO threads (id,session_key,org_id,user_id) VALUES ($1,$1,$2,'u1')",
            )
            .bind(id)
            .bind(&org)
            .execute(&pool)
            .await
            .expect("seed thread");
        }
        sqlx::query(
            "INSERT INTO runs (id,thread_id,goal,org_id,user_id,mode) VALUES ($1,$2,'g',$3,'u1','plan')",
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
            grant_columns: crate::grpc::ThreadGrantColumns::present(),
        };
        let reader = ThreadAutonomyGrpc {
            pool: pool.clone(),
            grant_columns: super::ThreadGrantColumns::present(),
        };
        let read = |org_id: &str, user: &str, request_org: &str, thread: &str| {
            reader.get_thread_grant(as_user(
                org_id,
                user,
                pb::GetThreadGrantRequest {
                    thread_id: thread.to_owned(),
                    org_id: request_org.to_owned(),
                },
            ))
        };
        let approve = |plan_id: &str, rung: pb::AutonomyRung| {
            svc.set_run_mode(as_user(
                &org,
                "u1",
                pb::SetRunModeRequest {
                    run_id: run_id.clone(),
                    mode: "execute".into(),
                    org_id: org.clone(),
                    granted_rung: rung as i32,
                    justification: "writes the report into the workspace".into(),
                    approved_plan_id: plan_id.to_owned(),
                },
            ))
        };

        // Nothing granted yet: "no grant" is an answer, not an error.
        let before = read(&org, "u1", &org, &thread_id).await.expect("read").into_inner();
        assert_eq!(before.granted_rung, pb::AutonomyRung::Unspecified as i32);

        // A plan id without a grant is a malformed ask and writes nothing.
        let malformed = approve("plan-1", pb::AutonomyRung::Unspecified).await;
        assert_eq!(malformed.unwrap_err().code(), tonic::Code::InvalidArgument);
        let still = read(&org, "u1", &org, &thread_id).await.expect("read").into_inner();
        assert_eq!(still.granted_rung, pb::AutonomyRung::Unspecified as i32);

        // The approval writes the run and the thread together.
        let resp = approve("plan-1", pb::AutonomyRung::WorkspaceWrite)
            .await
            .expect("approve")
            .into_inner();
        assert_eq!(resp.mode, "execute");
        let grant = read(&org, "u1", &org, &thread_id).await.expect("read").into_inner();
        assert_eq!(grant.granted_rung, pb::AutonomyRung::WorkspaceWrite as i32);
        assert_eq!(grant.justification, "writes the report into the workspace");
        assert_eq!(grant.approved_plan_id, "plan-1");
        let expires = grant.expires_at.expect("a live grant says when it lapses").seconds;
        let now = Utc::now().timestamp();
        assert!(
            expires > now && expires <= now + 86_400 + 5,
            "the expiry is the 24 hour ceiling from now: {expires} vs {now}"
        );
        let (run_rung,): (Option<String>,) =
            sqlx::query_as("SELECT metadata->>'autonomy_rung' FROM runs WHERE id = $1")
                .bind(&run_id)
                .fetch_one(&pool)
                .await
                .expect("read run");
        assert_eq!(run_rung.as_deref(), Some("workspace_write"), "the run keeps its own copy");

        // Another thread of the same owner holds nothing: the grant is the thread's.
        let elsewhere = read(&org, "u1", &org, &other_thread).await.expect("read").into_inner();
        assert_eq!(elsewhere.granted_rung, pb::AutonomyRung::Unspecified as i32);

        // A newer approval replaces the older one, plan and rung.
        approve("", pb::AutonomyRung::DangerFullAccess).await.expect("re-approve");
        let newer = read(&org, "u1", &org, &thread_id).await.expect("read").into_inner();
        assert_eq!(newer.granted_rung, pb::AutonomyRung::DangerFullAccess as i32);
        assert_eq!(newer.approved_plan_id, "", "a plain approval carries no plan");

        // Reads are refused for another org (named or forged) and for another user.
        let attacker = format!("attacker-{sfx}");
        assert_eq!(
            read(&attacker, "u1", &attacker, &thread_id).await.unwrap_err().code(),
            tonic::Code::PermissionDenied,
            "another org's caller cannot read this thread's grant"
        );
        assert_eq!(
            read(&attacker, "u1", &org, &thread_id).await.unwrap_err().code(),
            tonic::Code::PermissionDenied,
            "a request that names this org with another org's credential is refused"
        );
        assert_eq!(
            read(&org, "u2", &org, &thread_id).await.unwrap_err().code(),
            tonic::Code::PermissionDenied,
            "another user of the same org does not own the thread"
        );
        assert_eq!(
            read(&org, "u1", &org, "no-such-thread").await.unwrap_err().code(),
            tonic::Code::NotFound
        );

        // A grant past its expiry is no grant.
        sqlx::query("UPDATE threads SET granted_expires_at = now() - interval '1 second' WHERE id = $1")
            .bind(&thread_id)
            .execute(&pool)
            .await
            .expect("lapse the grant");
        let lapsed = read(&org, "u1", &org, &thread_id).await.expect("read").into_inner();
        assert_eq!(lapsed.granted_rung, pb::AutonomyRung::Unspecified as i32);
        assert!(lapsed.expires_at.is_none());

        // A grant for a run of another org changes nothing, and neither does a ZDR credential.
        let foreign = svc
            .set_run_mode(as_user(
                &attacker,
                "u1",
                pb::SetRunModeRequest {
                    run_id: run_id.clone(),
                    mode: "execute".into(),
                    org_id: attacker.clone(),
                    granted_rung: pb::AutonomyRung::DangerFullAccess as i32,
                    justification: "forged".into(),
                    approved_plan_id: "plan-forged".into(),
                },
            ))
            .await;
        assert_eq!(foreign.unwrap_err().code(), tonic::Code::PermissionDenied);
        let mut zdr_request = Request::new(pb::SetRunModeRequest {
            run_id: run_id.clone(),
            mode: "execute".into(),
            org_id: org.clone(),
            granted_rung: pb::AutonomyRung::DangerFullAccess as i32,
            justification: "from a zdr turn".into(),
            approved_plan_id: "plan-zdr".into(),
        });
        zdr_request
            .extensions_mut()
            .insert(VerifiedIdentity::user_for_test_with_zdr(&org, "u1", true));
        assert_eq!(
            svc.set_run_mode(zdr_request).await.unwrap_err().code(),
            tonic::Code::FailedPrecondition,
            "a zero-retention credential records no grant"
        );
        let (rung, plan): (Option<String>, Option<String>) =
            sqlx::query_as("SELECT granted_rung, granted_plan_id FROM threads WHERE id = $1")
                .bind(&thread_id)
                .fetch_one(&pool)
                .await
                .expect("read thread");
        assert_eq!(rung.as_deref(), Some("danger_full_access"), "only the owner's approval was recorded");
        assert_eq!(plan.as_deref(), Some(""), "and the refused ones left no plan id behind");
    }

    fn skill_request(
        org: &str,
        name: &str,
        content: &str,
        origin: &str,
        scope: &str,
        owner: &str,
    ) -> mp_contracts::model_plane::v1::UpsertAgentSkillRequest {
        mp_contracts::model_plane::v1::UpsertAgentSkillRequest {
            org_id: org.to_owned(),
            name: name.into(),
            description: "d".into(),
            content: content.into(),
            trigger_keywords: vec![],
            trigger_file_patterns: vec![],
            tool_restrictions: vec![],
            // A hostile or confused producer asks for an enabled skill; a
            // learned one must come out disabled anyway.
            enabled: true,
            origin: origin.into(),
            scope: scope.into(),
            owner_user_id: owner.into(),
            review_state: if origin == "background_review" {
                "accepted".into()
            } else {
                String::new()
            },
            source_run_id: "run-1".into(),
            source_thread_id: "thread-1".into(),
            proposal_reason: "why".into(),
        }
    }

    async fn skill_row_state(
        pool: &sqlx::PgPool,
        org: &str,
        name: &str,
        scope: &str,
        owner: &str,
    ) -> Option<(bool, String, String, String)> {
        sqlx::query_as(
            "SELECT enabled, scope, owner_user_id, review_state FROM agent_skills              WHERE org_id = $1 AND name = $2 AND scope = $3 AND owner_user_id = $4",
        )
        .bind(org)
        .bind(name)
        .bind(scope)
        .bind(owner)
        .fetch_optional(pool)
        .await
        .expect("read skill row")
    }

    // G7 / WS07 S7 — a learned skill is written disabled, user-scoped and
    // proposed whatever the request says, a background write never overwrites a
    // human's skill, and a user cannot write a proposal for someone else.
    async fn assert_upsert_skill_guards(
        svc: &super::SessionService,
        pool: &sqlx::PgPool,
        org: &str,
    ) {
        use mp_contracts::model_plane::v1::session_core_server::SessionCore;

        // The caller is the learning review (a service) naming the run's owner.
        let as_review = |req| as_service(org, &["session:skills:write"], req);

        let r = svc
            .upsert_agent_skill(as_review(skill_request(
                org, "Cache", "c1", "background_review", "org", "u1",
            )))
            .await
            .expect("upsert")
            .into_inner();
        assert!(r.created && !r.skipped_protected && r.skip_reason.is_empty());
        assert_eq!(
            skill_row_state(pool, org, "Cache", "user", "u1").await,
            Some((false, "user".into(), "u1".into(), "proposed".into())),
            "forced to a disabled user-scoped proposal; the request's scope, enabled and review_state are ignored"
        );
        assert!(
            skill_row_state(pool, org, "Cache", "org", "").await.is_none(),
            "no org-scoped copy"
        );

        // A human skill of the same name under the same owner is protected.
        svc.upsert_agent_skill(as_user(
            org,
            "u1",
            skill_request(org, "Runbook", "human", "user", "user", ""),
        ))
        .await
        .expect("user skill");
        let blocked = svc
            .upsert_agent_skill(as_review(skill_request(
                org, "Runbook", "MACHINE", "background_review", "", "u1",
            )))
            .await
            .expect("guarded upsert")
            .into_inner();
        assert!(
            blocked.skipped_protected && blocked.skip_reason == "protected",
            "background_review must not overwrite a user skill"
        );

        // A write with no owner is skipped, not an error and not an org skill.
        let ownerless = svc
            .upsert_agent_skill(as_review(skill_request(
                org, "Ownerless", "x", "background_review", "", "",
            )))
            .await
            .expect("ownerless")
            .into_inner();
        assert_eq!(ownerless.skip_reason, "owner_required");
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
            grant_columns: crate::grpc::ThreadGrantColumns::present(),
        };

        assert_set_run_mode_isolation(&svc, &pool, &run_id, &org, sfx).await;
        assert_upsert_skill_guards(&svc, &pool, &org).await;

        // G7 read path: list_agent_skills returns this org's usable skills
        // (Runbook), proving the learned-skill last mile is readable. The
        // learned Cache skill is a disabled proposal: it is listed to its owner
        // when disabled rows are asked for, and to nobody else.
        let listed = svc
            .list_agent_skills(as_user(
                &org,
                "u1",
                pb::ListAgentSkillsRequest {
                    org_id: org.clone(),
                    enabled_only: true,
                    for_user_id: String::new(),
                },
            ))
            .await
            .expect("list_agent_skills")
            .into_inner();
        let names: std::collections::BTreeSet<&str> =
            listed.skills.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains("Runbook") && !names.contains("Cache"),
            "list must return the accepted skill and not the proposal, got {names:?}"
        );
        let all = svc
            .list_agent_skills(as_user(
                &org,
                "u1",
                pb::ListAgentSkillsRequest {
                    org_id: org.clone(),
                    enabled_only: false,
                    for_user_id: String::new(),
                },
            ))
            .await
            .expect("list_agent_skills all")
            .into_inner();
        let proposal = all
            .skills
            .iter()
            .find(|s| s.name == "Cache")
            .expect("the owner lists their proposal");
        assert_eq!(
            (proposal.review_state.as_str(), proposal.source_run_id.as_str(), proposal.proposal_reason.as_str()),
            ("proposed", "run-1", "why")
        );
        // Per-org isolation: another org sees none of them.
        let other_org = format!("other-{sfx}");
        let other = svc
            .list_agent_skills(as_user(
                &other_org,
                "u1",
                pb::ListAgentSkillsRequest {
                    org_id: other_org.clone(),
                    enabled_only: true,
                    for_user_id: String::new(),
                },
            ))
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
            .list_conversation(as_user(
                &org,
                "u1",
                pb::ListConversationRequest {
                    org_id: org.clone(),
                    thread_id: thread_id.clone(),
                    space_id: String::new(),
                    space_read_decision_ref: String::new(),
                    space_read_decision_token: String::new(),
                },
            ))
            .await
            .expect("list_thread_messages")
            .into_inner();
        assert_eq!(convo.messages.len(), 2, "expected 2 messages");
        assert_eq!(convo.messages[0].role, "user");
        assert_eq!(convo.messages[0].content, "hello");
        assert_eq!(convo.messages[1].role, "assistant", "ordered by sequence");
        // Per-org isolation: another org cannot read this thread's conversation.
        let attacker = format!("attacker-{sfx}");
        let cross_convo = svc
            .list_conversation(as_user(
                &attacker,
                "u1",
                pb::ListConversationRequest {
                    org_id: attacker.clone(),
                    thread_id: thread_id.clone(),
                    space_id: String::new(),
                    space_read_decision_ref: String::new(),
                    space_read_decision_token: String::new(),
                },
            ))
            .await;
        assert_eq!(
            cross_convo.expect_err("another org must not read this thread's messages").code(),
            tonic::Code::PermissionDenied,
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
              authority_revision, origin)
             VALUES ($1, $1, $2, $3, $4, $5, $6, 4, $7, $8, $9, 9, 'space')",
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
            grant_columns: crate::grpc::ThreadGrantColumns::present(),
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
                    authority_revision, recipient_audience_revision, recipient_audience_hash, origin\
                ) VALUES ($1, $1, $2, $3, $4, $5, $6, $7, $8, 1, 1, $9, 'space')",
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
            grant_columns: crate::grpc::ThreadGrantColumns::present(),
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
            grant_columns: crate::grpc::ThreadGrantColumns::present(),
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

    /// D12. A thread a trusted workload owns INSIDE a person's Space -- a
    /// schedule's result thread -- is that person's content, not the org's.
    /// `authorize_owner_row` used to admit any org member to any system-owned
    /// row by id, so a colleague could read it with nothing but the thread id.
    /// The only way in is now a verified Space read decision, which Control
    /// issues to the Space's own owner and nobody else. Maintenance system
    /// threads (no Space) keep their org-wide read.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    #[allow(clippy::too_many_lines)]
    async fn a_space_scoped_system_thread_is_readable_only_through_a_verified_space_decision() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL unset");
            return;
        };
        let pool = sqlx::PgPool::connect(&url).await.expect("connect pg");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");
        let svc = super::SessionService {
            pool: pool.clone(),
            retrieval_client: None,
            graph_client: None,
            knowledge_client: None,
            letta_memory: None,
            audit_publisher: None,
            auth: None,
            grant_columns: crate::grpc::ThreadGrantColumns::present(),
        };

        // The fixture decision is minted for org-1 / user-1 / space-room, the
        // way Control's own-scheduled-thread-read fallback mints one for a
        // personal Space's owner. Process env is shared, but this is one of a
        // handful of manually-run #[ignore]d tests that already set it.
        let signing_key = SigningKey::from_bytes(&[47_u8; 32]);
        let (token, decision_ref, key_id) =
            signed_thread_read_decision(&signing_key, Utc::now(), &["thread:read"], 4);
        std::env::remove_var("CONTROL_SPACE_DECISION_PUBLIC_KEYS_JSON");
        std::env::set_var("CONTROL_SPACE_DECISION_KEY_ID", &key_id);
        std::env::set_var(
            "CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64",
            URL_SAFE_NO_PAD.encode(signing_key.verifying_key().to_bytes()),
        );

        let sfx = super::new_ulid();
        let scheduled = format!("d12-sched-{sfx}");
        let maintenance = format!("d12-maint-{sfx}");
        let human = format!("d12-human-{sfx}");
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id, space_id, space_decision_ref, \
             recipient_audience_ref, privacy_policy_ref, resource_authorization_ref, authority_revision, \
             recipient_audience_revision, recipient_audience_hash, origin) \
             VALUES ($1, $1, 'org-1', 'service:orchestrator-core', 'space-room', 'decision-1', \
             'audience-1', 'policy-1', 'resource-1', 1, 1, 'audience-hash-1', 'space')",
        )
        .bind(&scheduled)
        .execute(&pool)
        .await
        .expect("seed scheduled thread");
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id) \
             VALUES ($1, $1, 'org-1', 'service:orchestrator-core')",
        )
        .bind(&maintenance)
        .execute(&pool)
        .await
        .expect("seed maintenance system thread");
        sqlx::query("INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, 'org-1', 'user-1')")
            .bind(&human)
            .execute(&pool)
            .await
            .expect("seed ordinary human thread");
        for thread in [&scheduled, &maintenance, &human] {
            sqlx::query(
                "INSERT INTO messages (id, thread_id, role, content) VALUES ($1, $2, 'assistant', 'draft result')",
            )
            .bind(format!("m-{thread}"))
            .bind(thread)
            .execute(&pool)
            .await
            .expect("seed message");
        }

        let read = |org: &str, user: &str, thread: &str, with_decision: bool| {
            let request = pb::ListConversationRequest {
                org_id: org.to_owned(),
                thread_id: thread.to_owned(),
                space_id: if with_decision {
                    "space-room".to_owned()
                } else {
                    String::new()
                },
                space_read_decision_ref: if with_decision {
                    decision_ref.clone()
                } else {
                    String::new()
                },
                space_read_decision_token: if with_decision {
                    token.clone()
                } else {
                    String::new()
                },
            };
            svc.list_conversation(as_user(org, user, request))
        };

        // An org peer with no decision: not found, exactly what a thread that
        // does not exist answers, so the id is not even confirmed.
        assert_eq!(
            read("org-1", "user-2", &scheduled, false)
                .await
                .expect_err("an org peer must not read a Space-scoped system thread by id")
                .code(),
            tonic::Code::NotFound,
        );
        // Not even the schedule's owner reads it by the ownership path: the
        // owner is a workload, and the person comes in through the decision.
        assert_eq!(
            read("org-1", "user-1", &scheduled, false)
                .await
                .expect_err("the decision is the only way into a scheduled thread")
                .code(),
            tonic::Code::NotFound,
        );
        // The owner with a verified decision reads it.
        let owner_read = read("org-1", "user-1", &scheduled, true)
            .await
            .expect("the Space's owner reads their scheduled thread with a verified decision")
            .into_inner();
        assert_eq!(owner_read.messages.len(), 1);
        assert_eq!(owner_read.messages[0].content, "draft result");
        // A decision is bound to the subject Control issued it to.
        assert_eq!(
            read("org-1", "user-2", &scheduled, true)
                .await
                .expect_err("someone else's decision must not open the thread")
                .code(),
            tonic::Code::PermissionDenied,
        );
        // The org boundary is unchanged and still checked first.
        assert_eq!(
            read("org-2", "user-1", &scheduled, false)
                .await
                .expect_err("another org stays refused")
                .code(),
            tonic::Code::PermissionDenied,
        );
        // A decision for this Space does not unlock a thread outside it.
        assert_eq!(
            read("org-1", "user-1", &maintenance, true)
                .await
                .expect_err("a Space decision must not reach an unscoped thread")
                .code(),
            tonic::Code::NotFound,
        );
        // Maintenance system threads (no Space) stay org-readable.
        let maintenance_read = read("org-1", "user-2", &maintenance, false)
            .await
            .expect("an unscoped system thread keeps its org-wide read")
            .into_inner();
        assert_eq!(maintenance_read.messages.len(), 1);
        // An ordinary person's thread is untouched: owner reads, peer refused.
        read("org-1", "user-1", &human, false)
            .await
            .expect("an owner reads their own thread");
        assert_eq!(
            read("org-1", "user-2", &human, false)
                .await
                .expect_err("a peer must not read another person's thread")
                .code(),
            tonic::Code::PermissionDenied,
        );

        for thread in [&scheduled, &maintenance, &human] {
            sqlx::query("DELETE FROM messages WHERE thread_id = $1")
                .bind(thread)
                .execute(&pool)
                .await
                .ok();
            sqlx::query("DELETE FROM threads WHERE id = $1")
                .bind(thread)
                .execute(&pool)
                .await
                .ok();
        }
    }

    fn lazy_test_service() -> super::SessionService {
        super::SessionService {
            pool: sqlx::PgPool::connect_lazy("postgres://postgres:postgres@127.0.0.1/session_core")
                .expect("construct lazy pool"),
            retrieval_client: None,
            graph_client: None,
            knowledge_client: None,
            letta_memory: None,
            audit_publisher: None,
            auth: None,
            grant_columns: crate::grpc::ThreadGrantColumns::present(),
        }
    }

    /// Delegated-identity design, Gap B (§2.2): identical formula to
    /// user-core's `spaces.MailTaskSessionKey` (Go) and the gateway BFF's
    /// `IngestRequest::session_key()` (`mail_task.rs`) -- `"mail_task/" +
    /// sha256(org_id + provider_message_id)` -- so all three independently
    /// written implementations produce field-for-field comparable session
    /// keys. Test-only: production code never recomputes this, it only
    /// compares the caller-supplied `session_key` against the signed
    /// decision's own claim.
    fn mail_task_session_key(org_id: &str, provider_message_id: &str) -> String {
        format!(
            "mail_task/{:x}",
            Sha256::digest(format!("{org_id}{provider_message_id}").as_bytes())
        )
    }

    /// Signs a syntactically-valid mail-task-v1 decision for the given
    /// claim overrides, defaulting every field to a mutually-consistent
    /// happy-path set so each test only needs to state what it deviates.
    struct MailTaskDecisionBuilder {
        signing_key: SigningKey,
        key_id: String,
        decision_ref: String,
        org_id: String,
        user_id: String,
        alias_id: String,
        provider_message_id: String,
        session_key: String,
        origin: String,
        service_audience: String,
        action_id: String,
        nonce: String,
        issued_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    }

    impl MailTaskDecisionBuilder {
        fn new(now: DateTime<Utc>) -> Self {
            let org_id = "org-1".to_owned();
            let provider_message_id = "provider-msg-1".to_owned();
            Self {
                signing_key: SigningKey::from_bytes(&[42_u8; 32]),
                key_id: "control-mail-task-test".to_owned(),
                decision_ref: "decision-1".to_owned(),
                session_key: mail_task_session_key(&org_id, &provider_message_id),
                org_id,
                user_id: "user-a".to_owned(),
                alias_id: "alias-1".to_owned(),
                provider_message_id,
                origin: "mail_task".to_owned(),
                service_audience: super::MAIL_TASK_THREAD_PREPARE_AUDIENCE.to_owned(),
                action_id: super::MAIL_TASK_THREAD_PREPARE_ACTION.to_owned(),
                nonce: "nonce-1".to_owned(),
                issued_at: now - chrono::Duration::seconds(1),
                expires_at: now + chrono::Duration::minutes(5),
            }
        }

        fn keys(&self) -> BTreeMap<String, VerifyingKey> {
            BTreeMap::from([(self.key_id.clone(), self.signing_key.verifying_key())])
        }

        /// The request a caller with a matching decision would actually
        /// send -- every field here defaults to the SAME value the decision
        /// itself claims, so a test only diverges the ONE field it means to.
        fn request(&self) -> pb::PrepareUserOwnedThreadRequest {
            pb::PrepareUserOwnedThreadRequest {
                org_id: self.org_id.clone(),
                user_id: self.user_id.clone(),
                origin: self.origin.clone(),
                session_key: self.session_key.clone(),
                control_decision_token: self.sign(),
            }
        }

        fn sign(&self) -> String {
            let claims = serde_json::json!({
                "decision_ref": self.decision_ref,
                "org_id": self.org_id,
                "user_id": self.user_id,
                "alias_id": self.alias_id,
                "provider_message_id": self.provider_message_id,
                "session_key": self.session_key,
                "origin": self.origin,
                "service_audience": self.service_audience,
                "action_id": self.action_id,
                "nonce": self.nonce,
                "issued_at": self.issued_at.to_rfc3339(),
                "expires_at": self.expires_at.to_rfc3339(),
            });
            let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).expect("claims encode"));
            let encoded_key = URL_SAFE_NO_PAD.encode(self.key_id.as_bytes());
            let signing_input = format!("{}.{encoded_key}.{payload}", super::MAIL_TASK_DECISION_VERSION);
            format!(
                "{signing_input}.{}",
                URL_SAFE_NO_PAD.encode(self.signing_key.sign(signing_input.as_bytes()).to_bytes())
            )
        }
    }

    #[test]
    fn mail_task_thread_decision_verifies_when_every_field_matches_the_request() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-09-28T12:00:00Z")
            .expect("test time")
            .with_timezone(&Utc);
        let decision = MailTaskDecisionBuilder::new(now);
        super::verify_mail_task_thread_decision_with_keys(&decision.request(), &decision.keys(), now)
            .expect("a fully matching mail-task-v1 decision must authorize the request");
    }

    /// prepare_user_owned_thread_rejects_a_decision_minted_for_user_A_when_the_request_names_user_B
    /// -- the single most important test for the owner-only ruling itself
    /// (security-review §4). Everything else about the request is a perfect,
    /// validly-signed match; ONLY the request's user_id has been swapped
    /// for a different user than the decision was minted for. This must
    /// fail -- proving the field-by-field claim comparison actually fires,
    /// not merely "some validly signed decision was presented."
    #[test]
    fn prepare_user_owned_thread_rejects_a_decision_minted_for_user_a_when_the_request_names_user_b() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-09-28T12:00:00Z")
            .expect("test time")
            .with_timezone(&Utc);
        let decision = MailTaskDecisionBuilder::new(now);
        let mut request = decision.request(); // signed for user "user-a"
        request.user_id = "user-b".to_owned(); // request now claims a DIFFERENT user
        assert_eq!(
            super::verify_mail_task_thread_decision_with_keys(&request, &decision.keys(), now)
                .expect_err("a decision minted for user A must never authorize a request naming user B")
                .code(),
            tonic::Code::PermissionDenied
        );
    }

    #[test]
    fn prepare_user_owned_thread_rejects_a_decision_whose_session_key_does_not_match_the_requests_session_key(
    ) {
        let now = chrono::DateTime::parse_from_rfc3339("2026-09-28T12:00:00Z")
            .expect("test time")
            .with_timezone(&Utc);
        let decision = MailTaskDecisionBuilder::new(now);
        let mut request = decision.request();
        request.session_key = "mail_task/a-different-inbound-message-digest".to_owned();
        assert_eq!(
            super::verify_mail_task_thread_decision_with_keys(&request, &decision.keys(), now)
                .expect_err("a decision bound to one inbound message must not authorize a different one")
                .code(),
            tonic::Code::PermissionDenied
        );
    }

    #[test]
    fn prepare_user_owned_thread_rejects_a_decision_minted_for_a_different_org() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-09-28T12:00:00Z")
            .expect("test time")
            .with_timezone(&Utc);
        let decision = MailTaskDecisionBuilder::new(now);
        let mut request = decision.request();
        request.org_id = "org-attacker".to_owned();
        assert_eq!(
            super::verify_mail_task_thread_decision_with_keys(&request, &decision.keys(), now)
                .expect_err("cross-org decision reuse must be rejected")
                .code(),
            tonic::Code::PermissionDenied
        );
    }

    #[test]
    fn prepare_user_owned_thread_rejects_a_missing_control_decision_token_for_mail_task_origin() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-09-28T12:00:00Z")
            .expect("test time")
            .with_timezone(&Utc);
        let decision = MailTaskDecisionBuilder::new(now);
        let mut request = decision.request();
        request.control_decision_token = String::new();
        assert_eq!(
            super::verify_mail_task_thread_decision_with_keys(&request, &decision.keys(), now)
                .expect_err("an empty decision token must be refused, not treated as \"no decision required\"")
                .code(),
            tonic::Code::PermissionDenied
        );
    }

    #[test]
    fn mail_task_thread_decision_rejects_expired_or_not_yet_issued_claims() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-09-28T12:00:00Z")
            .expect("test time")
            .with_timezone(&Utc);
        let mut expired = MailTaskDecisionBuilder::new(now);
        expired.expires_at = now - chrono::Duration::seconds(1);
        assert_eq!(
            super::verify_mail_task_thread_decision_with_keys(&expired.request(), &expired.keys(), now)
                .expect_err("an expired decision must be rejected")
                .code(),
            tonic::Code::PermissionDenied
        );

        let mut future_issued = MailTaskDecisionBuilder::new(now);
        future_issued.issued_at = now + chrono::Duration::minutes(5);
        future_issued.expires_at = now + chrono::Duration::minutes(10);
        assert_eq!(
            super::verify_mail_task_thread_decision_with_keys(&future_issued.request(), &future_issued.keys(), now)
                .expect_err("a decision issued more than a minute in the future must be rejected")
                .code(),
            tonic::Code::PermissionDenied
        );
    }

    /// Cross-family confusion pin (closes B-2/security-review's reason for
    /// the distinct "mail-task-v1" tag over reusing the Space decision's
    /// "v2" envelope): a syntactically valid, correctly-signed decision
    /// minted for a COMPLETELY different Control decision family (here, a
    /// "v2" Space decision envelope, mirroring `expected_decision`-shaped
    /// claims used elsewhere in this file) must never verify as a mail-task
    /// decision, even if an attacker could somehow get one of its bytes to
    /// line up. The reverse direction (a mail-task-v1 envelope rejected by
    /// the Space-decision verifier) is proven on the issuing side, in
    /// user-core's own
    /// TestMailTaskThreadDecisionIsRejectedByTheSpaceDecisionVerifierAndViceVersa.
    #[test]
    fn mail_task_thread_decision_verifier_rejects_a_v2_space_decision_envelope() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-09-28T12:00:00Z")
            .expect("test time")
            .with_timezone(&Utc);
        let decision = MailTaskDecisionBuilder::new(now);
        let signing_key = SigningKey::from_bytes(&[43_u8; 32]);
        let key_id = "control-space-v2-test".to_owned();
        // A "v2"-tagged envelope carrying otherwise-plausible mail-task-shaped
        // field NAMES (an attacker's best attempt to collide) -- the version
        // tag mismatch alone must be enough to reject it.
        let claims = serde_json::json!({
            "decision_ref": decision.decision_ref,
            "org_id": decision.org_id,
            "user_id": decision.user_id,
            "session_key": decision.session_key,
            "origin": decision.origin,
        });
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).expect("claims encode"));
        let encoded_key = URL_SAFE_NO_PAD.encode(key_id.as_bytes());
        let signing_input = format!("v2.{encoded_key}.{payload}");
        let mut request = decision.request();
        request.control_decision_token = format!(
            "{signing_input}.{}",
            URL_SAFE_NO_PAD.encode(signing_key.sign(signing_input.as_bytes()).to_bytes())
        );
        let keys = BTreeMap::from([(key_id, signing_key.verifying_key())]);
        assert_eq!(
            super::verify_mail_task_thread_decision_with_keys(&request, &keys, now)
                .expect_err("a \"v2\" Space-decision envelope must never verify as a mail-task-v1 decision")
                .code(),
            tonic::Code::PermissionDenied
        );
    }

    #[tokio::test]
    async fn prepare_user_owned_thread_rejects_a_caller_not_on_the_allowlist_before_database_access() {
        let svc = lazy_test_service();
        let mut request = Request::new(pb::PrepareUserOwnedThreadRequest {
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
            origin: "mail_task".to_owned(),
            session_key: "mail_task/abc".to_owned(),
            control_decision_token: String::new(),
        });
        // A real, correctly-scoped service credential -- just not one this
        // RPC allow-lists. Holding SOME valid service credential must never
        // be enough on its own.
        request
            .extensions_mut()
            .insert(VerifiedIdentity::service_for_test_as(
                "org-1",
                "service:some-other-workload",
                &["session:user-owned-thread-prepare"],
                false,
            ));
        assert_eq!(
            svc.prepare_user_owned_thread(request)
                .await
                .expect_err("an unlisted service must be refused")
                .code(),
            tonic::Code::PermissionDenied
        );
    }

    /// Pins the mail-task thread-prepare bridge's OWN, distinct Auth Core
    /// principal (`mail_task_bridge_auth.rs`'s `MailTaskBridgeTokenProvider`,
    /// default service id `model-gateway-mail-task-bridge` -- deliberately
    /// separate from `service:model-gateway` per security-review B-4/T-6)
    /// onto `USER_OWNED_THREAD_CALLERS`. Without this entry, EVERY real
    /// mail-task call fails closed with the same "only an allow-listed
    /// trusted service" error the previous test asserts for a genuinely
    /// untrusted caller -- indistinguishable from this RPC being entirely
    /// unreachable, even once every deploy-config registry row is correct.
    /// A plain constant assertion, not a full RPC-level test, matching this
    /// file's existing convention of keeping `prepare_user_owned_thread`
    /// tests that go through decision verification independent of global
    /// process env (see `prepare_user_owned_thread_accepts_a_valid_mail_task_decision_and_is_idempotent_on_retry`'s
    /// own comment on why): a full RPC call for a `mail_task` origin also
    /// requires `configured_control_space_decision_keys()`, which reads
    /// process-global env this test must not depend on or mutate.
    #[test]
    fn user_owned_thread_callers_allowlists_the_mail_task_bridges_own_distinct_service_principal() {
        assert!(
            super::USER_OWNED_THREAD_CALLERS.contains(&"service:model-gateway-mail-task-bridge"),
            "the mail-task bridge mints its OWN distinct service principal \
             (mail_task_bridge_auth.rs, default MAIL_TASK_BRIDGE_SERVICE_ID) \
             specifically to avoid widening model-gateway's existing ZDR-scoped \
             session-core credential (B-4/T-6) -- if that principal id is not \
             also allow-listed here, every real mail-task call is refused \
             regardless of every registry row being correct"
        );
    }

    /// The bridge principal exists to create mail_task threads behind a signed
    /// decision. Allow-listing it for the RPC must not also hand it the
    /// decision-free path every other origin has: any non-mail_task origin is
    /// refused for it, before any database access (the lazy pool would answer
    /// `Internal` if this were reached).
    #[tokio::test]
    async fn prepare_user_owned_thread_confines_the_mail_task_bridge_principal_to_the_mail_task_origin() {
        for origin in ["chat", "agent_run", "support", "system"] {
            let svc = lazy_test_service();
            let mut request = Request::new(pb::PrepareUserOwnedThreadRequest {
                org_id: "org-1".to_owned(),
                user_id: "user-victim".to_owned(),
                origin: origin.to_owned(),
                session_key: "chat/whatever".to_owned(),
                control_decision_token: String::new(),
            });
            request
                .extensions_mut()
                .insert(VerifiedIdentity::service_for_test_as(
                    "org-1",
                    "service:model-gateway-mail-task-bridge",
                    &["session:user-owned-thread-prepare"],
                    false,
                ));
            assert_eq!(
                svc.prepare_user_owned_thread(request)
                    .await
                    .expect_err("the bridge principal must not create a decision-free thread")
                    .code(),
                tonic::Code::PermissionDenied,
                "origin {origin:?}",
            );
        }
    }

    #[test]
    fn user_owned_thread_origin_guard_only_narrows_the_bridge_principal() {
        assert!(super::authorize_user_owned_thread_origin(
            "service:model-gateway-mail-task-bridge",
            "mail_task"
        )
        .is_ok());
        assert!(super::authorize_user_owned_thread_origin(
            "service:model-gateway-mail-task-bridge",
            "chat"
        )
        .is_err());
        // service:model-gateway keeps its existing, decision-free origins.
        assert!(super::authorize_user_owned_thread_origin("service:model-gateway", "chat").is_ok());
    }

    /// The RPC-level wiring of the mail_task decision gate. The
    /// `verify_mail_task_thread_decision_with_keys` tests above prove the
    /// verifier; this proves the handler CALLS it: a mail_task request with no
    /// decision must be refused before any database access. (Removing the gate
    /// would send it on to the lazy pool, which answers `Internal`.)
    #[tokio::test]
    async fn prepare_user_owned_thread_refuses_a_mail_task_request_with_no_decision_before_the_database() {
        let svc = lazy_test_service();
        let mut request = Request::new(pb::PrepareUserOwnedThreadRequest {
            org_id: "org-1".to_owned(),
            user_id: "user-a".to_owned(),
            origin: "mail_task".to_owned(),
            session_key: "mail_task/abc".to_owned(),
            control_decision_token: String::new(),
        });
        request
            .extensions_mut()
            .insert(VerifiedIdentity::service_for_test_as(
                "org-1",
                "service:model-gateway-mail-task-bridge",
                &["session:user-owned-thread-prepare"],
                false,
            ));
        let code = svc
            .prepare_user_owned_thread(request)
            .await
            .expect_err("a mail_task request needs a signed decision")
            .code();
        // failed_precondition when the Control verifier keys are not
        // configured in this process, permission_denied when they are: either
        // way the request stopped at the gate, never at the database.
        assert!(
            matches!(
                code,
                tonic::Code::PermissionDenied | tonic::Code::FailedPrecondition
            ),
            "unexpected code {code:?}: the decision gate did not run before the database"
        );
    }

    #[tokio::test]
    async fn prepare_user_owned_thread_rejects_a_live_user_bearer() {
        let svc = lazy_test_service();
        let mut request = Request::new(pb::PrepareUserOwnedThreadRequest {
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
            origin: "mail_task".to_owned(),
            session_key: "mail_task/abc".to_owned(),
            control_decision_token: String::new(),
        });
        request
            .extensions_mut()
            .insert(VerifiedIdentity::user_for_test("org-1", "user-1"));
        assert_eq!(
            svc.prepare_user_owned_thread(request)
                .await
                .expect_err("a live user's own bearer must never mint a thread for an arbitrary user_id")
                .code(),
            tonic::Code::PermissionDenied
        );
    }

    #[tokio::test]
    async fn prepare_user_owned_thread_rejects_a_service_shaped_owner() {
        let svc = lazy_test_service();
        let mut request = Request::new(pb::PrepareUserOwnedThreadRequest {
            org_id: "org-1".to_owned(),
            user_id: "service:orchestrator-core".to_owned(),
            origin: "mail_task".to_owned(),
            session_key: "mail_task/abc".to_owned(),
            control_decision_token: String::new(),
        });
        request
            .extensions_mut()
            .insert(VerifiedIdentity::service_for_test_as(
                "org-1",
                "service:model-gateway",
                &["session:user-owned-thread-prepare"],
                false,
            ));
        assert_eq!(
            svc.prepare_user_owned_thread(request)
                .await
                .expect_err("a service-shaped user_id must be refused -- that is prepare_scheduled_run_thread's job, with its own Space-authority checks")
                .code(),
            tonic::Code::InvalidArgument
        );
    }

    #[tokio::test]
    async fn prepare_user_owned_thread_rejects_an_unrecognized_or_space_origin() {
        let svc = lazy_test_service();
        for bad_origin in ["space", "not-a-real-origin", ""] {
            let mut request = Request::new(pb::PrepareUserOwnedThreadRequest {
                org_id: "org-1".to_owned(),
                user_id: "user-1".to_owned(),
                origin: bad_origin.to_owned(),
                session_key: "mail_task/abc".to_owned(),
                control_decision_token: String::new(),
            });
            request
                .extensions_mut()
                .insert(VerifiedIdentity::service_for_test_as(
                    "org-1",
                    "service:model-gateway",
                    &["session:user-owned-thread-prepare"],
                    false,
                ));
            assert_eq!(
                svc.prepare_user_owned_thread(request)
                    .await
                    .expect_err(&format!("origin {bad_origin:?} must be refused"))
                    .code(),
                tonic::Code::InvalidArgument,
                "origin {bad_origin:?}",
            );
        }
    }

    #[tokio::test]
    async fn prepare_user_owned_thread_rejects_a_cross_org_caller_before_database_access() {
        let svc = lazy_test_service();
        let mut request = Request::new(pb::PrepareUserOwnedThreadRequest {
            org_id: "org-victim".to_owned(),
            user_id: "user-1".to_owned(),
            origin: "mail_task".to_owned(),
            session_key: "mail_task/abc".to_owned(),
            control_decision_token: String::new(),
        });
        request
            .extensions_mut()
            .insert(VerifiedIdentity::service_for_test_as(
                "org-attacker",
                "service:model-gateway",
                &["session:user-owned-thread-prepare"],
                false,
            ));
        assert_eq!(
            svc.prepare_user_owned_thread(request)
                .await
                .expect_err("cross-org must fail before SQL")
                .code(),
            tonic::Code::PermissionDenied
        );
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    // Uses origin "chat", not "mail_task" -- this is deliberately the
    // backward-compatibility pin for the delegated-identity design (Gap B):
    // a non-mail_task USER_OWNED_THREAD_CALLERS origin must keep exactly
    // today's allow-list-only behavior, unaffected by the new
    // control_decision_token gate (empty here, on purpose). The mail_task
    // origin's OWN happy path, which DOES require a valid decision, is
    // covered separately below by
    // prepare_user_owned_thread_accepts_a_valid_mail_task_decision_and_is_idempotent_on_retry.
    async fn prepare_user_owned_thread_creates_a_user_owned_thread_and_is_idempotent_on_retry() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL unset");
            return;
        };
        let pool = sqlx::PgPool::connect(&url).await.expect("connect pg");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");
        let svc = super::SessionService {
            pool: pool.clone(),
            retrieval_client: None,
            graph_client: None,
            knowledge_client: None,
            letta_memory: None,
            audit_publisher: None,
            auth: None,
            grant_columns: crate::grpc::ThreadGrantColumns::present(),
        };
        let session_key = format!("chat/{}", super::new_ulid());
        let build_request = || {
            let mut request = Request::new(pb::PrepareUserOwnedThreadRequest {
                org_id: "org-1".to_owned(),
                user_id: "user-1".to_owned(),
                origin: "chat".to_owned(),
                session_key: session_key.clone(),
                control_decision_token: String::new(),
            });
            request
                .extensions_mut()
                .insert(VerifiedIdentity::service_for_test_as(
                    "org-1",
                    "service:model-gateway",
                    &["session:user-owned-thread-prepare"],
                    false,
                ));
            request
        };
        let first = svc
            .prepare_user_owned_thread(build_request())
            .await
            .expect("first prepare succeeds (no decision required for a non-mail_task origin)")
            .into_inner();
        assert!(!first.thread_id.is_empty());

        let (owner, origin): (String, String) = sqlx::query_as(
            "SELECT user_id, origin FROM threads WHERE id = $1",
        )
        .bind(&first.thread_id)
        .fetch_one(&pool)
        .await
        .expect("thread row exists");
        assert_eq!(owner, "user-1", "thread must be owned by the real user, not the calling service");
        assert_eq!(origin, "chat");

        // A retry with the identical (org, user, session_key) must resolve
        // to the SAME thread -- the whole point of the idempotency contract
        // mail-task's own retry-storm requirement depends on.
        let second = svc
            .prepare_user_owned_thread(build_request())
            .await
            .expect("retry succeeds")
            .into_inner();
        assert_eq!(second.thread_id, first.thread_id);

        sqlx::query("DELETE FROM threads WHERE id = $1")
            .bind(&first.thread_id)
            .execute(&pool)
            .await
            .expect("cleanup");
    }

    /// prepare_user_owned_thread_accepts_a_valid_mail_task_decision_and_is_idempotent_on_retry
    /// (delegated-identity design, Gap B "Exact tests"). Confirms the
    /// (org_id, user_id, session_key) advisory-lock idempotency
    /// create_thread_inner_preverified already provides holds even under
    /// decision-TOKEN replay within its window: a retry mints a FRESH
    /// decision (new decision_ref/nonce, same claims otherwise) and must
    /// still resolve to the same thread, never a second one.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn prepare_user_owned_thread_accepts_a_valid_mail_task_decision_and_is_idempotent_on_retry() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL unset");
            return;
        };
        let pool = sqlx::PgPool::connect(&url).await.expect("connect pg");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");
        let svc = super::SessionService {
            pool: pool.clone(),
            retrieval_client: None,
            graph_client: None,
            knowledge_client: None,
            letta_memory: None,
            audit_publisher: None,
            auth: None,
            grant_columns: crate::grpc::ThreadGrantColumns::present(),
        };

        let signing_key = SigningKey::from_bytes(&[41_u8; 32]);
        let key_id = "control-mail-task-thread-prepare".to_owned();
        // Global process env is only touched by this one #[ignore]d,
        // manually-run integration test in this file -- every other
        // mail-task-v1 test below exercises verify_mail_task_thread_decision_with_keys
        // directly, which takes its key set as a plain argument specifically
        // to avoid this kind of shared, order-sensitive global state.
        std::env::set_var("CONTROL_SPACE_DECISION_KEY_ID", &key_id);
        std::env::set_var(
            "CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64",
            URL_SAFE_NO_PAD.encode(signing_key.verifying_key().to_bytes()),
        );

        let org_id = "org-1".to_owned();
        let user_id = "user-1".to_owned();
        let provider_message_id = format!("provider-msg-{}", super::new_ulid());
        let session_key = mail_task_session_key(&org_id, &provider_message_id);
        let sign = |decision_ref: &str, nonce: &str| -> String {
            let claims = serde_json::json!({
                "decision_ref": decision_ref,
                "org_id": org_id,
                "user_id": user_id,
                "alias_id": "alias-1",
                "provider_message_id": provider_message_id,
                "session_key": session_key,
                "origin": "mail_task",
                "service_audience": super::MAIL_TASK_THREAD_PREPARE_AUDIENCE,
                "action_id": super::MAIL_TASK_THREAD_PREPARE_ACTION,
                "nonce": nonce,
                "issued_at": Utc::now().to_rfc3339(),
                "expires_at": (Utc::now() + chrono::Duration::minutes(5)).to_rfc3339(),
            });
            let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).expect("claims encode"));
            let encoded_key = URL_SAFE_NO_PAD.encode(key_id.as_bytes());
            let signing_input = format!("{}.{encoded_key}.{payload}", super::MAIL_TASK_DECISION_VERSION);
            format!(
                "{signing_input}.{}",
                URL_SAFE_NO_PAD.encode(signing_key.sign(signing_input.as_bytes()).to_bytes())
            )
        };
        let build_request = |decision_ref: &str, nonce: &str| {
            let mut request = Request::new(pb::PrepareUserOwnedThreadRequest {
                org_id: org_id.clone(),
                user_id: user_id.clone(),
                origin: "mail_task".to_owned(),
                session_key: session_key.clone(),
                control_decision_token: sign(decision_ref, nonce),
            });
            request
                .extensions_mut()
                .insert(VerifiedIdentity::service_for_test_as(
                    "org-1",
                    "service:model-gateway",
                    &["session:user-owned-thread-prepare"],
                    false,
                ));
            request
        };

        let first = svc
            .prepare_user_owned_thread(build_request("decision-1", "nonce-1"))
            .await
            .expect("first prepare with a valid mail-task-v1 decision succeeds")
            .into_inner();
        assert!(!first.thread_id.is_empty());

        // A retry mints a FRESH decision (new decision_ref/nonce) -- the same
        // "same fire, new decision" shape scheduled-run retries already rely
        // on -- and must still resolve to the identical thread.
        let second = svc
            .prepare_user_owned_thread(build_request("decision-2", "nonce-2"))
            .await
            .expect("retry with a fresh decision succeeds")
            .into_inner();
        assert_eq!(second.thread_id, first.thread_id);

        let (count,): (i64,) = sqlx::query_as(
            "SELECT count(*) FROM threads WHERE org_id = $1 AND user_id = $2 AND session_key = $3",
        )
        .bind(&org_id)
        .bind(&user_id)
        .bind(&session_key)
        .fetch_one(&pool)
        .await
        .expect("count prepared threads");
        assert_eq!(count, 1, "retry must not create a second thread");

        sqlx::query("DELETE FROM threads WHERE id = $1")
            .bind(&first.thread_id)
            .execute(&pool)
            .await
            .expect("cleanup");
        std::env::remove_var("CONTROL_SPACE_DECISION_KEY_ID");
        std::env::remove_var("CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64");
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

    /// A legacy terminal step on a run no managed obligation owns ends it, and the
    /// run's durable loop fold goes with it in the same transaction (WS03 S4).
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn loop_checkpoint_legacy_terminal_step_removes_the_fold() {
        use crate::loop_checkpoint::test_support::{fold_rows, scratch_pool, seed_fold, seed_run};
        let Some(pool) = scratch_pool().await else {
            return;
        };
        // An unmanaged run: no terminalization obligation, so the legacy step may end it.
        let (run_id, _org, _user) = seed_run(&pool, "persistent").await;
        seed_fold(&pool, &run_id, "round", "", "", None)
            .await
            .expect("seed fold");
        assert_eq!(fold_rows(&pool, &run_id).await, 1);

        complete_step_inner(
            &pool,
            None,
            pb::CompleteStepRequest {
                run_id: run_id.clone(),
                step_id: "legacy-final".to_owned(),
                status: "completed".to_owned(),
                output: String::new(),
                error: String::new(),
                terminal: true,
            },
        )
        .await
        .expect("legacy terminal completion");

        let status: String = sqlx::query_scalar("SELECT status FROM runs WHERE id = $1")
            .bind(&run_id)
            .fetch_one(&pool)
            .await
            .expect("read status");
        assert_eq!(status, "completed");
        assert_eq!(fold_rows(&pool, &run_id).await, 0, "the fold goes with the run");
    }

    /// A run whose owner was removed from its Space still ends, and its fold goes with it. The
    /// three fold RPCs refuse such a run (see `loop_checkpoint::revocation_tests`), so this is the
    /// other half: the terminal path purges in its own transaction and does not pass the owner
    /// authorisation, so a removal cannot strand a fold that no RPC may touch.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn loop_checkpoint_a_removed_members_run_still_ends_and_loses_its_fold() {
        use crate::loop_checkpoint::test_support::{
            fold_rows, put_run_in_space, remove_owner_from_space, scratch_pool, seed_fold, seed_run,
        };
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (run_id, _org, _user) = seed_run(&pool, "persistent").await;
        let space_ref = format!("lc-space-{}", mp_ids::new_ulid());
        put_run_in_space(&pool, &run_id, &space_ref).await;
        seed_fold(&pool, &run_id, "pause", "appr_1", "", None)
            .await
            .expect("seed a paused fold");
        remove_owner_from_space(&pool, &run_id, &space_ref).await;
        assert_eq!(fold_rows(&pool, &run_id).await, 1);

        complete_step_inner(
            &pool,
            None,
            pb::CompleteStepRequest {
                run_id: run_id.clone(),
                step_id: "legacy-final".to_owned(),
                status: "completed".to_owned(),
                output: String::new(),
                error: String::new(),
                terminal: true,
            },
        )
        .await
        .expect("a removed member's run still terminates");

        let status: String = sqlx::query_scalar("SELECT status FROM runs WHERE id = $1")
            .bind(&run_id)
            .fetch_one(&pool)
            .await
            .expect("read status");
        assert_eq!(status, "completed");
        assert_eq!(fold_rows(&pool, &run_id).await, 0, "the fold is purged with the run");
    }

    /// Deleting a thread removes the folds of its runs. This proves the OUTCOME: no
    /// fold outlives its thread. It does not prove which statement removed it, since
    /// the foreign key from `run_loop_checkpoints` to `runs` cascades and covers the
    /// fold as well.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn loop_checkpoint_thread_delete_removes_the_fold() {
        use crate::loop_checkpoint::test_support::{fold_rows, scratch_pool, seed_fold, seed_run};
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (run_id, _org, _user) = seed_run(&pool, "persistent").await;
        let thread_id: String = sqlx::query_scalar("SELECT thread_id FROM runs WHERE id = $1")
            .bind(&run_id)
            .fetch_one(&pool)
            .await
            .expect("run's thread");
        seed_fold(&pool, &run_id, "round", "", "", None)
            .await
            .expect("seed fold");
        assert_eq!(fold_rows(&pool, &run_id).await, 1);

        let mut tx = pool.begin().await.expect("begin");
        super::delete_thread_rows(&mut tx, &thread_id)
            .await
            .expect("delete the thread's rows");
        tx.commit().await.expect("commit");

        assert_eq!(fold_rows(&pool, &run_id).await, 0, "no fold outlives its thread");
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
            ..Default::default()
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
            ..Default::default()
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

        // audit-core accepts a message only when its subject's last token is the
        // payload's event, so each phase's row must be stored under the subject
        // its own event names (the reservation used to share `...tool_action`).
        let subjects: Vec<(String, String)> = sqlx::query_as(
            "SELECT payload->>'event', subject
             FROM session_audit_outbox
             WHERE payload->>'subject' = $1
             ORDER BY payload->>'event'",
        )
        .bind(&run_id)
        .fetch_all(&pool)
        .await
        .expect("load audit subjects");
        assert_eq!(
            subjects,
            vec![
                (
                    "tool_action".to_owned(),
                    "verevon.audit.v2.model.session-core.tool_action".to_owned(),
                ),
                (
                    "tool_action_reserved".to_owned(),
                    "verevon.audit.v2.model.session-core.tool_action_reserved".to_owned(),
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
            grant_columns: crate::grpc::ThreadGrantColumns::present(),
        };
        let addr: std::net::SocketAddr = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        // Production wraps the service in the auth interceptor; this one
        // installs the verified run owner it would have produced.
        let caller = VerifiedIdentity::user_for_test(&org, "u1");
        tokio::spawn(async move {
            Server::builder()
                .add_service(SessionCoreServer::with_interceptor(
                    svc,
                    move |mut request: Request<()>| {
                        request.extensions_mut().insert(caller.clone());
                        Ok(request)
                    },
                ))
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
                approved_plan_id: String::new(),
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

    // -- learned skills as owner-scoped proposals (WS07 S7) ------------------

    #[test]
    fn upsert_agent_skill_withheld_reason_table() {
        use crate::memory_control::{decide, MemoryState, OrgMemoryPolicy};

        let reason = |zdr, org, user| super::learned_skill_withheld(&decide(zdr, org, user));
        assert_eq!(reason(false, OrgMemoryPolicy::Allow, MemoryState::On), None);
        assert_eq!(
            reason(false, OrgMemoryPolicy::Allow, MemoryState::Paused),
            Some("memory_state")
        );
        assert_eq!(
            reason(false, OrgMemoryPolicy::Allow, MemoryState::Off),
            Some("memory_state")
        );
        assert_eq!(
            reason(false, OrgMemoryPolicy::Disabled, MemoryState::On),
            Some("org_policy")
        );
        assert_eq!(
            reason(false, OrgMemoryPolicy::Unavailable, MemoryState::On),
            Some("org_policy"),
            "an unreadable policy is not consent"
        );
        assert_eq!(reason(true, OrgMemoryPolicy::Allow, MemoryState::On), Some("zdr"));
        // The organization has not opted in to learning review: memory is allowed
        // and the user is on, but learning is not.
        let mut not_opted_in = decide(false, OrgMemoryPolicy::Allow, MemoryState::On);
        not_opted_in.learning_allowed = false;
        assert_eq!(super::learned_skill_withheld(&not_opted_in), Some("org_policy"));
    }

    #[test]
    fn upsert_agent_skill_skip_tokens_are_closed() {
        for token in [
            "memory_state",
            "org_policy",
            "zdr",
            "accepted_exists",
            "rejected_exists",
            "owner_required",
        ] {
            let response = super::skill_write_skipped(token);
            assert!(!response.skipped_protected && response.id.is_empty() && !response.created);
            assert_eq!(response.skip_reason, token);
        }
        let protected = super::skill_write_skipped("protected");
        assert!(protected.skipped_protected, "a human's row keeps the old flag");
    }

    /// The learned arm of the conflict update changes content and provenance
    /// only. Reading `enabled`, `scope`, `owner_user_id` or `review_state` into
    /// that SET list is how an admin-disabled or owner-decided row got
    /// re-enabled by the next review of a similar conversation.
    #[test]
    fn upsert_agent_skill_conflict_arm_never_touches_state_columns() {
        let source = include_str!("grpc.rs");
        let guard = source
            .find("WHERE agent_skills.origin = 'background_review'")
            .expect("the learned conflict guard");
        let set = source[..guard]
            .rfind("DO UPDATE SET")
            .expect("the learned conflict arm");
        let arm = &source[set..guard];
        for column in ["enabled", "scope", "owner_user_id", "review_state", "origin"] {
            assert!(
                !arm.contains(&format!("{column} =")),
                "the learned conflict arm must not assign {column}"
            );
        }
        assert!(
            source.contains("ON CONFLICT (org_id, scope, owner_user_id, name)"),
            "the conflict key carries the owner"
        );
    }

    async fn skills_service() -> Option<(super::SessionService, sqlx::PgPool, String)> {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL unset");
            return None;
        };
        let pool = sqlx::PgPool::connect(&url).await.expect("connect pg");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");
        let svc = super::SessionService {
            pool: pool.clone(),
            retrieval_client: None,
            graph_client: None,
            knowledge_client: None,
            letta_memory: None,
            audit_publisher: None,
            auth: None,
            grant_columns: crate::grpc::ThreadGrantColumns::present(),
        };
        Some((svc, pool, format!("skp-{}", mp_ids::new_ulid())))
    }

    async fn learned_skill_content(
        pool: &sqlx::PgPool,
        org: &str,
        owner: &str,
        name: &str,
    ) -> String {
        sqlx::query_scalar(
            "SELECT content FROM agent_skills WHERE org_id = $1 AND scope = 'user' AND owner_user_id = $2 AND name = $3",
        )
        .bind(org)
        .bind(owner)
        .bind(name)
        .fetch_one(pool)
        .await
        .expect("read learned skill")
    }

    async fn clear_skill_rows(pool: &sqlx::PgPool, org: &str) {
        for statement in [
            "DELETE FROM user_memory_controls WHERE org_id = $1",
            "DELETE FROM agent_skills WHERE org_id = $1",
        ] {
            sqlx::query(statement).bind(org).execute(pool).await.ok();
        }
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a Postgres with session-core migrations"]
    async fn background_review_is_forced_to_disabled_user_scope() {
        use mp_contracts::model_plane::v1::session_core_server::SessionCore;
        let Some((svc, pool, org)) = skills_service().await else {
            return;
        };
        // The producer asks for an enabled, org-wide, accepted skill.
        let written = svc
            .upsert_agent_skill(as_service(
                &org,
                &["session:skills:write"],
                skill_request(&org, "Weekly report", "steps", "background_review", "org", "owner-a"),
            ))
            .await
            .expect("upsert")
            .into_inner();
        assert!(written.created && written.skip_reason.is_empty());
        assert_eq!(
            skill_row_state(&pool, &org, "Weekly report", "user", "owner-a").await,
            Some((false, "user".into(), "owner-a".into(), "proposed".into()))
        );
        let provenance: (String, String, String) = sqlx::query_as(
            "SELECT source_run_id, source_thread_id, proposal_reason FROM agent_skills WHERE id = $1",
        )
        .bind(&written.id)
        .fetch_one(&pool)
        .await
        .expect("provenance");
        assert_eq!(provenance, ("run-1".into(), "thread-1".into(), "why".into()));

        // Switching a proposal on is the owner's decision, not a flag flip.
        let refused = svc
            .set_agent_skill_enabled(as_service(
                &org,
                &["session:skills:write"],
                mp_contracts::model_plane::v1::SetAgentSkillEnabledRequest {
                    org_id: org.clone(),
                    skill_id: written.id.clone(),
                    enabled: true,
                    reason: "sweep".into(),
                },
            ))
            .await
            .expect_err("a proposal cannot be enabled");
        assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
        assert_eq!(refused.message(), "proposal_requires_decision");

        // A paused owner gets no proposal at all.
        sqlx::query(
            "INSERT INTO user_memory_controls (org_id, user_id, memory_state) VALUES ($1, 'owner-p', 'paused')",
        )
        .bind(&org)
        .execute(&pool)
        .await
        .expect("pause");
        let paused = svc
            .upsert_agent_skill(as_service(
                &org,
                &["session:skills:write"],
                skill_request(&org, "Never", "x", "background_review", "", "owner-p"),
            ))
            .await
            .expect("paused upsert")
            .into_inner();
        assert_eq!(paused.skip_reason, "memory_state");
        assert!(skill_row_state(&pool, &org, "Never", "user", "owner-p")
            .await
            .is_none());

        clear_skill_rows(&pool, &org).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a Postgres with session-core migrations"]
    async fn user_b_cannot_overwrite_user_a_learned_skill() {
        use mp_contracts::model_plane::v1::session_core_server::SessionCore;
        let Some((svc, pool, org)) = skills_service().await else {
            return;
        };
        let review = |owner: &str, content: &str| {
            as_service(
                &org,
                &["session:skills:write"],
                skill_request(&org, "Same name", content, "background_review", "", owner),
            )
        };
        svc.upsert_agent_skill(review("user-a", "A's steps"))
            .await
            .expect("a");
        let b = svc
            .upsert_agent_skill(review("user-b", "B's steps"))
            .await
            .expect("b")
            .into_inner();
        assert!(b.created, "B gets a row of their own, not A's");
        assert_eq!(
            learned_skill_content(&pool, &org, "user-a", "Same name").await,
            "A's steps"
        );
        assert_eq!(
            learned_skill_content(&pool, &org, "user-b", "Same name").await,
            "B's steps"
        );

        // A user credential can only write for itself.
        let forged = svc
            .upsert_agent_skill(as_user(
                &org,
                "user-b",
                skill_request(&org, "Same name", "forged", "background_review", "", "user-a"),
            ))
            .await
            .expect_err("B may not write A's proposal");
        assert_eq!(forged.code(), tonic::Code::PermissionDenied);
        assert_eq!(
            learned_skill_content(&pool, &org, "user-a", "Same name").await,
            "A's steps"
        );

        clear_skill_rows(&pool, &org).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a Postgres with session-core migrations"]
    async fn conflict_never_reenables_an_admin_disabled_row() {
        use mp_contracts::model_plane::v1::session_core_server::SessionCore;
        let Some((svc, pool, org)) = skills_service().await else {
            return;
        };
        // An accepted learned skill that an admin switched off.
        sqlx::query(
            "INSERT INTO agent_skills
                (id, org_id, name, description, content, enabled, origin, scope, owner_user_id, review_state)
             VALUES ('sk-off', $1, 'Disabled', 'd', 'original', false, 'background_review', 'user', 'owner-a', 'accepted')",
        )
        .bind(&org)
        .execute(&pool)
        .await
        .expect("seed");
        let again = svc
            .upsert_agent_skill(as_service(
                &org,
                &["session:skills:write"],
                skill_request(&org, "Disabled", "rewritten", "background_review", "", "owner-a"),
            ))
            .await
            .expect("upsert")
            .into_inner();
        assert_eq!(again.skip_reason, "accepted_exists");
        assert_eq!(
            skill_row_state(&pool, &org, "Disabled", "user", "owner-a").await,
            Some((false, "user".into(), "owner-a".into(), "accepted".into())),
            "the row stays disabled and keeps its state"
        );

        // A proposal refreshed by a later review is still a disabled proposal.
        for content in ["first", "second"] {
            svc.upsert_agent_skill(as_service(
                &org,
                &["session:skills:write"],
                skill_request(&org, "Waiting", content, "background_review", "", "owner-a"),
            ))
            .await
            .expect("propose");
        }
        assert_eq!(
            skill_row_state(&pool, &org, "Waiting", "user", "owner-a").await,
            Some((false, "user".into(), "owner-a".into(), "proposed".into()))
        );
        assert_eq!(
            learned_skill_content(&pool, &org, "owner-a", "Waiting").await,
            "second"
        );

        // A human write onto the proposal asks for `enabled` and does not get it.
        svc.upsert_agent_skill(as_user(
            &org,
            "owner-a",
            skill_request(&org, "Waiting", "edited", "user", "user", ""),
        ))
        .await
        .expect("human write");
        assert_eq!(
            skill_row_state(&pool, &org, "Waiting", "user", "owner-a").await,
            Some((false, "user".into(), "owner-a".into(), "proposed".into())),
            "an undecided proposal is not switched on by a write"
        );

        clear_skill_rows(&pool, &org).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a Postgres with session-core migrations"]
    async fn accepted_row_is_not_rewritten() {
        use mp_contracts::model_plane::v1::session_core_server::SessionCore;
        let Some((svc, pool, org)) = skills_service().await else {
            return;
        };
        for (id, name, state) in [
            ("sk-acc", "Accepted", "accepted"),
            ("sk-rej", "Rejected", "rejected"),
        ] {
            sqlx::query(
                "INSERT INTO agent_skills
                    (id, org_id, name, description, content, enabled, origin, scope, owner_user_id, review_state)
                 VALUES ($1, $2, $3, 'd', 'original', $4, 'background_review', 'user', 'owner-a', $5)",
            )
            .bind(id)
            .bind(&org)
            .bind(name)
            .bind(state == "accepted")
            .bind(state)
            .execute(&pool)
            .await
            .expect("seed");
        }
        for (name, expected) in [
            ("Accepted", "accepted_exists"),
            ("Rejected", "rejected_exists"),
        ] {
            let response = svc
                .upsert_agent_skill(as_service(
                    &org,
                    &["session:skills:write"],
                    skill_request(&org, name, "rewritten", "background_review", "", "owner-a"),
                ))
                .await
                .expect("upsert")
                .into_inner();
            assert_eq!(response.skip_reason, expected);
            assert!(!response.created);
            assert_eq!(
                learned_skill_content(&pool, &org, "owner-a", name).await,
                "original"
            );
        }

        clear_skill_rows(&pool, &org).await;
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
    // combination precisely. WS07 S7 adds the learned-skill proposal: a
    // disabled, owner-scoped row that no one but its owner may list, even when
    // a share was written onto it, and a name an org skill and a user's own
    // skill both hold.
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
                 ON CONFLICT DO NOTHING",
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
        // A proposal shared (by a hand-written row) with the sharee, and a name
        // the org and the owner both hold.
        for (name, scope, owner_user_id, shared_with, enabled, review_state) in [
            (
                "proposal-skill",
                "user",
                owner.as_str(),
                &format!("[\"{sharee}\"]"),
                false,
                "proposed",
            ),
            ("dup-skill", "org", "", &"[]".to_owned(), true, "accepted"),
            ("dup-skill", "user", owner.as_str(), &"[]".to_owned(), true, "accepted"),
        ] {
            sqlx::query(
                "INSERT INTO agent_skills
                    (id, org_id, name, description, content, trigger_keywords,
                     trigger_file_patterns, tool_restrictions, enabled, origin,
                     scope, owner_user_id, shared_with, review_state, created_at, updated_at)
                 VALUES ($1,$2,$3,'d','c','[]','[]','[]',$7,'background_review',$4,$5,$6::jsonb,$8,now(),now())
                 ON CONFLICT DO NOTHING",
            )
            .bind(format!("{name}-{scope}-{sfx}"))
            .bind(&org_id)
            .bind(name)
            .bind(scope)
            .bind(owner_user_id)
            .bind(shared_with)
            .bind(enabled)
            .bind(review_state)
            .execute(&pool)
            .await
            .expect("seed proposal");
        }

        let svc = SessionService {
            pool: pool.clone(),
            retrieval_client: None,
            graph_client: None,
            knowledge_client: None,
            letta_memory: None,
            audit_publisher: None,
            auth: None,
            grant_columns: crate::grpc::ThreadGrantColumns::present(),
        };

        let list_as = |user_id: String| {
            let org_id = org_id.clone();
            let svc = &svc;
            async move {
                let mut request = Request::new(pb::ListAgentSkillsRequest {
                    org_id: org_id.clone(),
                    enabled_only: false,
                    for_user_id: String::new(),
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

        // A proposal is the owner's alone: a share written onto it does not
        // reach the sharee until the owner has accepted it.
        assert!(
            owner_view.contains("proposal-skill"),
            "the owner lists their proposal: {owner_view:?}"
        );
        assert!(
            !sharee_view.contains("proposal-skill") && !stranger_view.contains("proposal-skill"),
            "no one else lists a proposal: {sharee_view:?} {stranger_view:?}"
        );

        // `for_user_id` is honoured for a service caller and ignored for a user
        // caller, who can never list as someone else.
        let list_with = |identity: VerifiedIdentity, for_user_id: String| {
            let org_id = org_id.clone();
            let svc = &svc;
            async move {
                let mut request = Request::new(pb::ListAgentSkillsRequest {
                    org_id,
                    enabled_only: false,
                    for_user_id,
                });
                request.extensions_mut().insert(identity);
                svc.list_agent_skills(request)
                    .await
                    .expect("list_agent_skills")
                    .into_inner()
                    .skills
            }
        };
        let as_review = list_with(
            VerifiedIdentity::service_for_test_as(
                &org_id,
                "service:capability-core",
                &["session:read"],
                false,
            ),
            owner.clone(),
        )
        .await;
        assert!(
            as_review.iter().any(|s| s.name == "proposal-skill"),
            "the review lists the existing skills of the run's owner"
        );
        let spoof = list_with(VerifiedIdentity::user_for_test(&org_id, &stranger), owner.clone()).await;
        assert!(
            spoof.iter().all(|s| s.name != "private-skill" && s.name != "proposal-skill"),
            "a user caller cannot list as someone else"
        );

        // The owner's own skill sorts before the org skill of the same name.
        let owner_skills = list_with(VerifiedIdentity::user_for_test(&org_id, &owner), String::new()).await;
        let dups: Vec<&str> = owner_skills
            .iter()
            .filter(|s| s.name == "dup-skill")
            .map(|s| s.scope.as_str())
            .collect();
        assert_eq!(dups, vec!["user", "org"]);
    }
}

/// `CompleteDirectRunWithMessage` behind the production wiring
/// (`ManagedRunLifecycleServer::with_interceptor(.., JwtVerifier)`, over the
/// wire) with the credentials Auth Core really issues.
///
/// No database is needed: the pool points at a socket that never answers, so a
/// call that clears every credential check fails at the handler's first query
/// (`authorize_run_owner`) with a pool timeout, and a call refused on its
/// credential fails before it. The live regression was the second kind: a
/// trailers-only error a few milliseconds in, with no SQL.
#[cfg(test)]
mod direct_completion_wire_auth_tests {
    use super::*;
    use crate::auth::test_support::{
        model_gateway_terminalize_claims, service_principal_claims, sign, user_claims, verifier,
        KID,
    };
    use mp_contracts::model_plane::v1::managed_run_lifecycle_client::ManagedRunLifecycleClient;
    use std::time::Duration;

    /// A server behind the production interceptor whose database never
    /// answers. Keep the returned listener alive: it holds that socket.
    async fn lifecycle_client() -> (std::net::TcpListener, ManagedRunLifecycleClient<Channel>) {
        let silent_database = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .acquire_timeout(Duration::from_millis(250))
            .connect_lazy(&format!(
                "postgresql://session-core@{}/session_core",
                silent_database.local_addr().expect("address")
            ))
            .expect("lazy pool");
        let service = ManagedRunLifecycleService {
            pool,
            letta_memory: None,
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(ManagedRunLifecycleServer::with_interceptor(
                    service,
                    verifier("session-core"),
                ))
                .serve_with_incoming(
                    tonic::transport::server::TcpIncoming::from_listener(listener, true, None)
                        .expect("incoming"),
                )
                .await
                .expect("serve");
        });
        let channel = tonic::transport::Endpoint::from_shared(format!("http://{address}"))
            .expect("endpoint")
            .connect()
            .await
            .expect("connect");
        (silent_database, ManagedRunLifecycleClient::new(channel))
    }

    /// The call Model Gateway makes for the failed live turn, ids and all.
    async fn complete(
        client: &mut ManagedRunLifecycleClient<Channel>,
        claims: Option<&serde_json::Value>,
    ) -> Status {
        let mut request = Request::new(pb::CompleteDirectRunWithMessageRequest {
            run_id: "01M3AN90KN32RDEXZW3MYQE5FD".to_owned(),
            thread_id: "01M3AN90GZ8SHZPBGYB5GHT1CE".to_owned(),
            content: "Svar.".to_owned(),
            idempotency_key: "fixture-idempotency-key".to_owned(),
            ..Default::default()
        });
        if let Some(claims) = claims {
            request.metadata_mut().insert(
                "authorization",
                format!("Bearer {}", sign(claims, KID))
                    .parse()
                    .expect("metadata"),
            );
        }
        client
            .complete_direct_run_with_message(request)
            .await
            .expect_err("there is no database behind this server")
    }

    /// Regression, 2026-09-24: Model Gateway's real terminalize credential
    /// (`zdr: true` by registry policy) was refused FAILED_PRECONDITION before
    /// any SQL. It must reach the run-owner query; retention is decided later,
    /// from the run.
    #[tokio::test]
    async fn the_gateways_real_credential_reaches_the_run_owner_query() {
        let (_silent_database, mut client) = lifecycle_client().await;
        let status = complete(
            &mut client,
            Some(&model_gateway_terminalize_claims("org-1")),
        )
        .await;
        assert_eq!(status.code(), tonic::Code::Internal, "{status:?}");
        assert!(
            status.message().contains("pool timed out"),
            "refused before the first query: {status:?}"
        );
    }

    /// The credential checks that remain all refuse before any SQL.
    #[tokio::test]
    async fn every_other_credential_is_refused_before_any_query() {
        let (_silent_database, mut client) = lifecycle_client().await;
        let heartbeat = service_principal_claims(
            "session-core",
            "org-1",
            "model-gateway",
            &["session:heartbeat"],
            "model-gateway managed-run heartbeat",
            true,
        );
        let execution = service_principal_claims(
            "session-core",
            "org-1",
            "execution-core",
            &["session:terminalize"],
            "execution-core managed-run terminalization",
            false,
        );
        let user = user_claims("session-core", "org-1", "user-1", false);
        let mut quarry = model_gateway_terminalize_claims("org-1");
        quarry["aud"] = serde_json::json!("quarry");
        for (claims, expected) in [
            (Some(&heartbeat), tonic::Code::PermissionDenied),
            (Some(&execution), tonic::Code::PermissionDenied),
            (Some(&user), tonic::Code::PermissionDenied),
            (Some(&quarry), tonic::Code::Unauthenticated),
            (None, tonic::Code::Unauthenticated),
        ] {
            let status = complete(&mut client, claims).await;
            assert_eq!(status.code(), expected, "{claims:?}: {status:?}");
        }
    }
}

/// `SessionRetentionService.CountWaivedRuns` behind the production interceptor,
/// over a real socket, with the credentials Auth Core really issues. No database
/// is needed: a refusal happens before the first query, and the one call that
/// clears every check reaches a pool that never answers, so it fails with the
/// generic internal status and nothing more.
#[cfg(test)]
mod waived_run_count_wire_tests {
    use super::*;
    use crate::auth::test_support::{service_principal_claims, sign, user_claims, verifier, KID};
    use crate::retention_receipt::RetentionClass;
    use mp_contracts::model_plane::v1::session_retention_service_client::SessionRetentionServiceClient;
    use std::time::Duration;
    use tonic::Code;

    const ORG: &str = "org-count-a";
    const GATEWAY: &str = "verevon-gateway";

    async fn retention_client() -> (
        std::net::TcpListener,
        SessionRetentionServiceClient<Channel>,
    ) {
        let silent_database = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .acquire_timeout(Duration::from_millis(250))
            .connect_lazy(&format!(
                "postgresql://session-core@{}/session_core",
                silent_database.local_addr().expect("address")
            ))
            .expect("lazy pool");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(SessionRetentionServiceServer::with_interceptor(
                    SessionRetentionGrpc { pool },
                    verifier("session-core"),
                ))
                .serve_with_incoming(
                    tonic::transport::server::TcpIncoming::from_listener(listener, true, None)
                        .expect("incoming"),
                )
                .await
                .expect("serve");
        });
        let channel = tonic::transport::Endpoint::from_shared(format!("http://{address}"))
            .expect("endpoint")
            .connect()
            .await
            .expect("connect");
        (silent_database, SessionRetentionServiceClient::new(channel))
    }

    /// One `CountWaivedRuns` call as the gateway makes it: the bearer in
    /// `authorization`, signed the way Auth Core signs.
    async fn count(
        client: &mut SessionRetentionServiceClient<Channel>,
        claims: &serde_json::Value,
        org_id: &str,
    ) -> Result<pb::CountWaivedRunsResponse, Status> {
        let mut request = tonic::Request::new(pb::CountWaivedRunsRequest {
            org_id: org_id.to_owned(),
        });
        request.metadata_mut().insert(
            "authorization",
            format!("Bearer {}", sign(claims, KID))
                .parse()
                .expect("bearer metadata"),
        );
        client
            .count_waived_runs(request)
            .await
            .map(tonic::Response::into_inner)
    }

    /// The admin credential the gateway holds for `org`: the read scope only.
    fn admin_claims(org: &str) -> serde_json::Value {
        service_principal_claims(
            "session-core",
            org,
            GATEWAY,
            &[WAIVED_RUN_COUNT_SCOPE],
            "org admin waived-run counts",
            false,
        )
    }

    #[tokio::test]
    async fn a_user_credential_is_refused_even_when_it_claims_the_scope() {
        let (_silent_database, mut client) = retention_client().await;
        let mut claims = user_claims("session-core", ORG, "user-admin", false);
        claims["scopes"] = serde_json::json!([WAIVED_RUN_COUNT_SCOPE]);
        let status = count(&mut client, &claims, ORG)
            .await
            .expect_err("a user credential never reads waived-run counts");
        assert_eq!(status.code(), Code::PermissionDenied, "{status:?}");
        assert!(
            status.message().contains("service scope required"),
            "{status:?}"
        );
    }

    #[tokio::test]
    async fn a_service_without_the_read_scope_is_refused() {
        let (_silent_database, mut client) = retention_client().await;
        let claims = service_principal_claims(
            "session-core",
            ORG,
            GATEWAY,
            &["session:read"],
            "reads sessions only",
            false,
        );
        let status = count(&mut client, &claims, ORG)
            .await
            .expect_err("the sessions read scope is not the waived-run scope");
        assert_eq!(status.code(), Code::PermissionDenied, "{status:?}");
    }

    #[tokio::test]
    async fn a_scoped_service_cannot_count_another_orgs_waived_runs() {
        let (_silent_database, mut client) = retention_client().await;
        let status = count(&mut client, &admin_claims(ORG), "org-count-b")
            .await
            .expect_err("the credential names one org only");
        assert_eq!(status.code(), Code::PermissionDenied, "{status:?}");
        assert!(
            status.message().contains("tenant access denied"),
            "{status:?}"
        );
    }

    #[tokio::test]
    async fn an_empty_org_is_an_invalid_request() {
        let (_silent_database, mut client) = retention_client().await;
        let status = count(&mut client, &admin_claims(ORG), "")
            .await
            .expect_err("an org is required");
        assert_eq!(status.code(), Code::InvalidArgument, "{status:?}");
    }

    #[tokio::test]
    async fn a_scoped_service_reaches_the_query_and_gets_only_a_generic_failure() {
        let (_silent_database, mut client) = retention_client().await;
        let status = count(&mut client, &admin_claims(ORG), ORG)
            .await
            .expect_err("the silent database cannot answer");
        // Every credential check passed. The failure is the database, and the
        // caller learns nothing of it beyond that.
        assert_eq!(status.code(), Code::Internal, "{status:?}");
        assert_eq!(
            status.message(),
            "waived-run counts are unavailable",
            "database errors are not echoed to the caller"
        );
    }

    #[test]
    fn the_count_query_spells_the_receipt_class_the_index_is_built_for() {
        assert_eq!(RetentionClass::ZdrWaived.as_str(), "zdr_waived");
        assert!(COUNT_WAIVED_RUNS_SQL.contains("retention_class = 'zdr_waived'"));
    }
}

/// Postgres-backed tests for `CountWaivedRuns`. They write rows to a scratch
/// database, so they are `#[ignore]`d. The session-core receipts CI job runs every
/// `zdr_receipt_*` test with `DATABASE_URL` set and `REQUIRE_CONTRACT=1`.
#[cfg(test)]
mod waived_run_count_db_tests {
    use super::*;
    use crate::auth::test_support::{authenticate, service_principal_claims, sign, verifier, KID};
    use crate::retention_receipt::RunRetention;
    use crate::terminalization::start_managed_run_with_retention_inner;
    use chrono::{TimeZone, Utc};
    use mp_ids::new_ulid;

    const INDEX_COUNT_SQL: &str =
        "SELECT count(*) FROM pg_indexes WHERE indexname = 'runs_zdr_waived_org_connection_idx'";

    /// The scratch database. Without `DATABASE_URL` the test skips locally and
    /// fails under CI. The name must say it is a test database, because these
    /// tests write rows.
    async fn scratch_pool() -> Option<PgPool> {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            assert!(
                std::env::var_os("REQUIRE_CONTRACT").is_none() && std::env::var_os("CI").is_none(),
                "DATABASE_URL must name a scratch Postgres when REQUIRE_CONTRACT or CI is set"
            );
            eprintln!("skipping: DATABASE_URL unset");
            return None;
        };
        let database = url
            .rsplit('/')
            .next()
            .and_then(|tail| tail.split('?').next())
            .unwrap_or_default();
        assert!(
            database.contains("test"),
            "these tests write rows: DATABASE_URL must name a scratch database whose name contains \"test\""
        );
        let pool = PgPool::connect(&url)
            .await
            .expect("scratch Postgres is reachable");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("session-core migrations apply in order");
        Some(pool)
    }

    /// A thread the user owns in `org`, for that user's runs to start on.
    async fn thread_for(pool: &PgPool, org: &str, user: &str) -> String {
        let id = new_ulid();
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id, created_at)
             VALUES ($1, $2, $3, $4, now())",
        )
        .bind(&id)
        .bind(format!("count-{id}"))
        .bind(org)
        .bind(user)
        .execute(pool)
        .await
        .expect("thread seeded");
        id
    }

    /// One run started through the real start path, with the receipt
    /// `retention` names. A ZDR start names no thread: Session Core makes one.
    async fn start(
        pool: &PgPool,
        org: &str,
        user: &str,
        thread: &str,
        key: &str,
        retention: RunRetention,
    ) -> String {
        start_managed_run_with_retention_inner(
            pool,
            pb::StartManagedRunRequest {
                thread_id: thread.to_owned(),
                parent_run_id: String::new(),
                agent_id: "general-v1".to_owned(),
                goal: "count goal".to_owned(),
                mode: "execute".to_owned(),
                org_id: org.to_owned(),
                user_id: user.to_owned(),
                start_key: key.to_owned(),
                terminal_source: pb::ManagedRunSource::GatewayDirect as i32,
            },
            retention,
        )
        .await
        .expect("the run starts")
        .run_id
    }

    /// Pins a run's start time, so that its last-used date is a known instant.
    async fn dated(pool: &PgPool, run: &str, at: DateTime<Utc>) {
        sqlx::query("UPDATE runs SET created_at = $2 WHERE id = $1")
            .bind(run)
            .bind(at)
            .execute(pool)
            .await
            .expect("the start time is pinned");
    }

    /// `org`'s counts from the real handler, behind a real signed admin
    /// credential that the real interceptor accepted.
    async fn counts_for(pool: &PgPool, org: &str) -> Vec<pb::WaivedRunCount> {
        let claims = service_principal_claims(
            "session-core",
            org,
            "verevon-gateway",
            &[WAIVED_RUN_COUNT_SCOPE],
            "org admin waived-run counts",
            false,
        );
        let request = authenticate(
            &verifier("session-core"),
            &sign(&claims, KID),
            pb::CountWaivedRunsRequest {
                org_id: org.to_owned(),
            },
        )
        .expect("the admin credential is accepted");
        SessionRetentionGrpc { pool: pool.clone() }
            .count_waived_runs(request)
            .await
            .expect("the count reads")
            .into_inner()
            .counts
    }

    /// The down script opens and closes its own transaction. Inside ours that
    /// would commit early, so its control lines are dropped here.
    fn strip_transaction_control(script: &str) -> String {
        script
            .lines()
            .filter(|line| {
                !matches!(
                    line.trim().to_ascii_uppercase().as_str(),
                    "BEGIN;" | "COMMIT;"
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\" (session-core receipts CI job)"]
    async fn zdr_receipt_counts_each_connection_and_user_of_one_org_and_nothing_else() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let suffix = new_ulid();
        let org = format!("count-a-{suffix}");
        let other_org = format!("count-b-{suffix}");
        let first = format!("u1-{suffix}");
        let second = format!("u2-{suffix}");
        let outsider = format!("u3-{suffix}");
        let conn_1 = format!("conn-{suffix}-1");
        let conn_2 = format!("conn-{suffix}-2");
        let thread_1 = thread_for(&pool, &org, &first).await;
        let thread_2 = thread_for(&pool, &org, &second).await;
        let outsider_thread = thread_for(&pool, &other_org, &outsider).await;
        let at = |day: u32, hour: u32| {
            Utc.with_ymd_and_hms(2026, 10, day, hour, 0, 0)
                .single()
                .expect("a valid instant")
        };

        // Counted: two runs on (conn_1, first), one on (conn_2, first), and one
        // on (conn_1, second).
        let w1 = start(
            &pool,
            &org,
            &first,
            &thread_1,
            &format!("{suffix}-w1"),
            RunRetention::waived("ack-1", &conn_1),
        )
        .await;
        dated(&pool, &w1, at(1, 9)).await;
        let w2 = start(
            &pool,
            &org,
            &first,
            &thread_1,
            &format!("{suffix}-w2"),
            RunRetention::waived("ack-1", &conn_1),
        )
        .await;
        dated(&pool, &w2, at(3, 12)).await;
        let w3 = start(
            &pool,
            &org,
            &first,
            &thread_1,
            &format!("{suffix}-w3"),
            RunRetention::waived("ack-2", &conn_2),
        )
        .await;
        dated(&pool, &w3, at(2, 8)).await;
        let w4 = start(
            &pool,
            &org,
            &second,
            &thread_2,
            &format!("{suffix}-w4"),
            RunRetention::waived("ack-3", &conn_1),
        )
        .await;
        dated(&pool, &w4, at(2, 10)).await;

        // Not counted: an ordinary run dated after every counted one, a
        // zero-retention run, and another org's waived run on the same
        // connection id.
        let persistent = start(
            &pool,
            &org,
            &first,
            &thread_1,
            &format!("{suffix}-p1"),
            RunRetention::persistent(),
        )
        .await;
        dated(&pool, &persistent, at(5, 7)).await;
        start(
            &pool,
            &org,
            &first,
            "",
            &format!("{suffix}-z1"),
            RunRetention::zdr(),
        )
        .await;
        start(
            &pool,
            &other_org,
            &outsider,
            &outsider_thread,
            &format!("{suffix}-o1"),
            RunRetention::waived("ack-4", &conn_1),
        )
        .await;

        let mut got: Vec<(String, String, i64, i64)> = counts_for(&pool, &org)
            .await
            .into_iter()
            .map(|row| {
                let pb::WaivedRunCount {
                    connection_id,
                    user_id,
                    run_count,
                    last_used_at,
                } = row;
                let seconds = last_used_at.expect("every row is dated").seconds;
                (connection_id, user_id, run_count, seconds)
            })
            .collect();
        got.sort();
        let mut want = vec![
            (conn_1.clone(), first.clone(), 2, at(3, 12).timestamp()),
            (conn_1.clone(), second.clone(), 1, at(2, 10).timestamp()),
            (conn_2.clone(), first.clone(), 1, at(2, 8).timestamp()),
        ];
        want.sort();
        assert_eq!(
            got, want,
            "one row per (connection, user), dated by its latest waived run, this org only"
        );
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\" (session-core receipts CI job)"]
    async fn zdr_receipt_count_is_empty_for_an_org_with_no_waived_run() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let suffix = new_ulid();
        let org = format!("count-empty-{suffix}");
        let user = format!("u-{suffix}");
        let thread = thread_for(&pool, &org, &user).await;
        start(
            &pool,
            &org,
            &user,
            &thread,
            &format!("{suffix}-p"),
            RunRetention::persistent(),
        )
        .await;
        assert!(
            counts_for(&pool, &org).await.is_empty(),
            "an ordinary run is never counted as waived"
        );
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\" (session-core receipts CI job)"]
    async fn zdr_receipt_waived_index_is_partial_on_org_connection_and_user() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (definition,): (String,) = sqlx::query_as(
            "SELECT indexdef FROM pg_indexes
             WHERE tablename = 'runs' AND indexname = 'runs_zdr_waived_org_connection_idx'",
        )
        .fetch_one(&pool)
        .await
        .expect("migration 0045 created the index");
        assert!(
            definition.contains("(org_id, waiver_connection_id, user_id)"),
            "{definition}"
        );
        assert!(
            definition.contains("WHERE (retention_class = 'zdr_waived'::text)"),
            "{definition}"
        );
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\" (session-core receipts CI job)"]
    async fn zdr_receipt_migration_0045_reverts_and_reapplies_inside_a_rolled_back_transaction() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let revert = strip_transaction_control(include_str!(
            "../migrations/revert/0045_zdr_waived_runs_org_index.down.sql"
        ));
        let mut tx = pool.begin().await.expect("transaction");
        sqlx::raw_sql(&revert)
            .execute(&mut *tx)
            .await
            .expect("the revert applies");
        let (dropped,): (i64,) = sqlx::query_as(INDEX_COUNT_SQL)
            .fetch_one(&mut *tx)
            .await
            .expect("counted");
        assert_eq!(dropped, 0, "the revert drops the index");
        sqlx::raw_sql(include_str!(
            "../migrations/0045_zdr_waived_runs_org_index.sql"
        ))
        .execute(&mut *tx)
        .await
        .expect("the migration re-applies after its revert");
        let (recreated,): (i64,) = sqlx::query_as(INDEX_COUNT_SQL)
            .fetch_one(&mut *tx)
            .await
            .expect("counted");
        assert_eq!(recreated, 1, "the migration builds the index again");
        tx.rollback().await.expect("rolled back");

        let (kept,): (i64,) = sqlx::query_as(INDEX_COUNT_SQL)
            .fetch_one(&pool)
            .await
            .expect("counted");
        assert_eq!(
            kept, 1,
            "the rolled-back transaction leaves the index in place"
        );
    }
}

/// `ManagedRunLifecycle.CompleteDirectRunWithMessage` against a real Postgres.
///
/// DB: `DATABASE_URL=<disposable> cargo test -p session-core --bin session-core
/// -- --ignored direct_completion_db_tests`
#[cfg(test)]
mod direct_completion_db_tests {
    use super::*;
    use mp_contracts::model_plane::v1::run_service_server::RunService as _;

    const GATEWAY: &str = "service:model-gateway";

    async fn disposable_pool() -> Option<PgPool> {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL unset");
            return None;
        };
        let pool = PgPool::connect(&url).await.expect("connect pg");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");
        Some(pool)
    }

    #[allow(clippy::struct_field_names)]
    struct DirectRun {
        org_id: String,
        user_id: String,
        thread_id: String,
        run_id: String,
    }

    /// A thread with its user turn, and a managed run on it owned by `source`.
    async fn managed_run(pool: &PgPool, source: pb::ManagedRunSource, zdr: bool) -> DirectRun {
        let suffix = new_ulid();
        let org_id = format!("org-direct-{suffix}");
        let user_id = format!("user-direct-{suffix}");
        let mut thread_id = String::new();
        if !zdr {
            thread_id = create_thread_inner(
                pool,
                pb::CreateThreadRequest {
                    session_key: format!("direct-{suffix}"),
                    org_id: org_id.clone(),
                    user_id: user_id.clone(),
                    ..Default::default()
                },
            )
            .await
            .expect("create thread")
            .into_inner()
            .thread_id;
            append_message_inner(
                pool,
                None,
                MemoryRetention::Durable,
                pb::AppendMessageRequest {
                    thread_id: thread_id.clone(),
                    role: "user".into(),
                    content: "Hva er status?".into(),
                    ..Default::default()
                },
                &user_id,
            )
            .await
            .expect("user turn");
        }
        let started = crate::terminalization::start_managed_run_inner(
            pool,
            pb::StartManagedRunRequest {
                thread_id,
                agent_id: "model-gateway".into(),
                goal: "Hva er status?".into(),
                mode: "execute".into(),
                org_id: org_id.clone(),
                user_id: user_id.clone(),
                start_key: format!("direct-start-{suffix}"),
                terminal_source: source as i32,
                ..Default::default()
            },
            zdr,
        )
        .await
        .expect("start managed run");
        DirectRun {
            org_id,
            user_id,
            thread_id: started.thread_id,
            run_id: started.run_id,
        }
    }

    fn evidence() -> prost_types::Struct {
        json_to_struct(&serde_json::json!({
            "grounding": { "citations": [{ "title": "Kilde", "url": "https://example.test/a" }] },
        }))
        .expect("an object")
    }

    fn completion(
        run: &DirectRun,
        content: &str,
        key: &str,
    ) -> pb::CompleteDirectRunWithMessageRequest {
        pb::CompleteDirectRunWithMessageRequest {
            run_id: run.run_id.clone(),
            thread_id: run.thread_id.clone(),
            content: content.to_owned(),
            agent_name: " Verevon ".to_owned(),
            metadata: Some(evidence()),
            idempotency_key: key.to_owned(),
        }
    }

    fn lifecycle(pool: &PgPool) -> ManagedRunLifecycleService {
        ManagedRunLifecycleService {
            pool: pool.clone(),
            letta_memory: None,
        }
    }

    fn as_caller<T>(message: T, caller: VerifiedIdentity) -> Request<T> {
        let mut request = Request::new(message);
        request.extensions_mut().insert(caller);
        request
    }

    /// `message` carrying the terminalize credential Auth Core really issues
    /// Model Gateway for `org_id` (`zdr: true`, by registry policy), through
    /// the production interceptor, never a hand-built identity.
    fn as_gateway<T>(message: T, org_id: &str) -> Request<T> {
        use crate::auth::test_support::{
            authenticate, model_gateway_terminalize_claims, sign, verifier, KID,
        };
        let token = sign(&model_gateway_terminalize_claims(org_id), KID);
        let request = authenticate(&verifier("session-core"), &token, message)
            .expect("the gateway's Auth Core credential verifies");
        assert_eq!(
            identity(&request).expect("identity").principal_id(),
            GATEWAY
        );
        request
    }

    async fn complete(
        pool: &PgPool,
        request: pb::CompleteDirectRunWithMessageRequest,
        org_id: &str,
    ) -> Result<pb::CompleteDirectRunWithMessageResponse, Status> {
        lifecycle(pool)
            .complete_direct_run_with_message(as_gateway(request, org_id))
            .await
            .map(Response::into_inner)
    }

    async fn assistant_messages(pool: &PgPool, thread_id: &str) -> Vec<(i64, String)> {
        sqlx::query_as(
            "SELECT sequence, content FROM messages
             WHERE thread_id = $1 AND role = 'assistant' ORDER BY sequence",
        )
        .bind(thread_id)
        .fetch_all(pool)
        .await
        .expect("read messages")
    }

    async fn run_status(pool: &PgPool, run_id: &str) -> String {
        sqlx::query_scalar("SELECT status FROM runs WHERE id = $1")
            .bind(run_id)
            .fetch_one(pool)
            .await
            .expect("read run")
    }

    async fn run_events(pool: &PgPool, run_id: &str, event_type: &str) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM events WHERE run_id = $1 AND event_type = $2")
            .bind(run_id)
            .bind(event_type)
            .fetch_one(pool)
            .await
            .expect("count events")
    }

    async fn obligation_state(pool: &PgPool, run_id: &str) -> (String, Option<String>) {
        sqlx::query_as(
            "SELECT state, completion_key FROM managed_run_terminalization_outbox WHERE run_id = $1",
        )
        .bind(run_id)
        .fetch_one(pool)
        .await
        .expect("read obligation")
    }

    async fn cancel(pool: &PgPool, run: &DirectRun) -> bool {
        crate::run_service_grpc::RunServiceImpl::new(pool.clone())
            .cancel_run(as_caller(
                pb::CancelRunRequest {
                    run_id: run.run_id.clone(),
                    reason: "client_stream_cancelled".into(),
                },
                VerifiedIdentity::user_for_test(&run.org_id, &run.user_id),
            ))
            .await
            .expect("cancel run")
            .into_inner()
            .cancelled
    }

    /// The answer, stored exactly as `AppendMessage` stores it, and the
    /// Completed receipt commit together.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn completes_the_run_and_saves_its_answer_in_one_transaction() {
        let Some(pool) = disposable_pool().await else {
            return;
        };
        let run = managed_run(&pool, pb::ManagedRunSource::GatewayDirect, false).await;
        let key = new_ulid();

        let response = complete(&pool, completion(&run, "Alt er klart.", &key), &run.org_id)
            .await
            .expect("complete");

        assert!(!response.already_terminal);
        let receipt = response.receipt.expect("a Completed receipt");
        assert_eq!(receipt.run_id, run.run_id);
        assert_eq!(receipt.source, pb::ManagedRunSource::GatewayDirect as i32);
        assert_eq!(
            receipt.terminal_step_id,
            "model-gateway-direct-inference-final"
        );
        assert!(!receipt.already_applied);
        assert!(!receipt.receipt_id.is_empty());
        let saved = assistant_messages(&pool, &run.thread_id).await;
        assert_eq!(saved.len(), 1);
        assert_eq!(
            u64::try_from(saved[0].0).unwrap(),
            response.message_sequence
        );
        assert_eq!(saved[0].1, "Alt er klart.");
        let (agent_name, metadata, author): (Option<String>, serde_json::Value, Option<String>) =
            sqlx::query_as(
                "SELECT agent_name, metadata, author_subject_id FROM messages
                 WHERE thread_id = $1 AND role = 'assistant'",
            )
            .bind(&run.thread_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            agent_name.as_deref(),
            Some("Verevon"),
            "trimmed like an append"
        );
        assert_eq!(metadata, struct_to_json(&evidence()));
        assert_eq!(author, None, "an assistant turn names no human author");
        let appended: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM events WHERE run_id = $1 AND event_type = 'MESSAGE_APPENDED'",
        )
        .bind(&run.thread_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            appended, 2,
            "the user turn and the answer each have their event"
        );

        assert_eq!(run_status(&pool, &run.run_id).await, "completed");
        assert_eq!(run_events(&pool, &run.run_id, "STEP_COMPLETED").await, 1);
        assert_eq!(run_events(&pool, &run.run_id, "RUN_COMPLETED").await, 1);
        let receipt_event: String = sqlx::query_scalar(
            "SELECT id FROM events WHERE run_id = $1 AND event_type = 'RUN_COMPLETED'",
        )
        .bind(&run.run_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(receipt_event, receipt.receipt_id);
        assert_eq!(
            obligation_state(&pool, &run.run_id).await,
            ("applied".to_owned(), Some(key))
        );
    }

    /// A retry after a lost response returns the original receipt and
    /// sequence and saves no second message.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn an_exact_retry_returns_the_original_result_without_a_second_message() {
        let Some(pool) = disposable_pool().await else {
            return;
        };
        let run = managed_run(&pool, pb::ManagedRunSource::GatewayDirect, false).await;
        let key = new_ulid();

        let first = complete(&pool, completion(&run, "Svar.", &key), &run.org_id)
            .await
            .expect("first");
        let retry = complete(&pool, completion(&run, "Svar.", &key), &run.org_id)
            .await
            .expect("retry");

        assert!(!retry.already_terminal);
        assert_eq!(retry.message_sequence, first.message_sequence);
        let (first_receipt, retry_receipt) = (first.receipt.unwrap(), retry.receipt.unwrap());
        assert_eq!(retry_receipt.receipt_id, first_receipt.receipt_id);
        assert_eq!(retry_receipt.step_index, first_receipt.step_index);
        assert!(retry_receipt.already_applied, "the retry is a replay");
        assert_eq!(assistant_messages(&pool, &run.thread_id).await.len(), 1);
        assert_eq!(run_events(&pool, &run.run_id, "RUN_COMPLETED").await, 1);

        // Another operation finds the run completed and writes nothing.
        let other = complete(
            &pool,
            completion(&run, "Et annet svar.", &new_ulid()),
            &run.org_id,
        )
        .await
        .expect("other operation");
        assert!(other.already_terminal);
        assert!(other.receipt.is_none());
        assert_eq!(other.message_sequence, 0);
        assert_eq!(assistant_messages(&pool, &run.thread_id).await.len(), 1);
    }

    /// A cancelled run answers `already_terminal` and nothing is written.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn a_run_that_is_no_longer_active_saves_nothing() {
        let Some(pool) = disposable_pool().await else {
            return;
        };
        let run = managed_run(&pool, pb::ManagedRunSource::GatewayDirect, false).await;
        assert!(cancel(&pool, &run).await);

        let response = complete(
            &pool,
            completion(&run, "For sent.", &new_ulid()),
            &run.org_id,
        )
        .await
        .expect("an already-terminal run is an answer, not an error");

        assert!(response.already_terminal);
        assert!(response.receipt.is_none());
        assert!(assistant_messages(&pool, &run.thread_id).await.is_empty());
        assert_eq!(run_status(&pool, &run.run_id).await, "cancelled");
        assert_eq!(run_events(&pool, &run.run_id, "RUN_COMPLETED").await, 0);
        assert_eq!(run_events(&pool, &run.run_id, "STEP_COMPLETED").await, 0);
        assert_eq!(
            obligation_state(&pool, &run.run_id).await,
            ("superseded".to_owned(), None),
            "the obligation is retired as a receipt or heartbeat would"
        );
    }

    /// A direct completion ends the run, so the run's durable loop fold goes with it
    /// in the same transaction (build plan WS03 S4).
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn loop_checkpoint_direct_completion_removes_the_fold() {
        let Some(pool) = disposable_pool().await else {
            return;
        };
        let run = managed_run(&pool, pb::ManagedRunSource::GatewayDirect, false).await;
        crate::loop_checkpoint::test_support::seed_fold(&pool, &run.run_id, "round", "", "", None)
            .await
            .expect("seed fold");
        assert_eq!(
            crate::loop_checkpoint::test_support::fold_rows(&pool, &run.run_id).await,
            1
        );

        let response = complete(&pool, completion(&run, "Alt er klart.", &new_ulid()), &run.org_id)
            .await
            .expect("complete");

        assert!(!response.already_terminal);
        assert_eq!(run_status(&pool, &run.run_id).await, "completed");
        assert_eq!(
            crate::loop_checkpoint::test_support::fold_rows(&pool, &run.run_id).await,
            0,
            "the fold goes with the run"
        );
    }

    /// The managed terminal path of a run whose owner was removed from its Space: the run still
    /// ends and its fold goes with it, since the purge does not pass the owner authorisation
    /// the three fold RPCs now apply.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn loop_checkpoint_a_removed_members_direct_completion_still_removes_the_fold() {
        use crate::loop_checkpoint::test_support::{put_run_in_space, remove_owner_from_space};
        let Some(pool) = disposable_pool().await else {
            return;
        };
        let run = managed_run(&pool, pb::ManagedRunSource::GatewayDirect, false).await;
        crate::loop_checkpoint::test_support::seed_fold(
            &pool,
            &run.run_id,
            "pause",
            "appr_1",
            "",
            None,
        )
        .await
        .expect("seed fold");
        let space_ref = format!("lc-space-{}", new_ulid());
        put_run_in_space(&pool, &run.run_id, &space_ref).await;
        remove_owner_from_space(&pool, &run.run_id, &space_ref).await;
        assert_eq!(
            crate::loop_checkpoint::test_support::fold_rows(&pool, &run.run_id).await,
            1
        );

        let response = complete(&pool, completion(&run, "Alt er klart.", &new_ulid()), &run.org_id)
            .await
            .expect("a removed member's run still completes");

        assert!(!response.already_terminal);
        assert_eq!(run_status(&pool, &run.run_id).await, "completed");
        assert_eq!(
            crate::loop_checkpoint::test_support::fold_rows(&pool, &run.run_id).await,
            0,
            "the fold goes with the run"
        );
    }

    /// A cancel holding the run row makes the completion wait; once the cancel
    /// commits, the completion finds the run over and writes nothing.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn a_cancel_that_commits_first_leaves_no_answer_on_the_run() {
        let Some(pool) = disposable_pool().await else {
            return;
        };
        let run = managed_run(&pool, pb::ManagedRunSource::GatewayDirect, false).await;

        // The same row update `CancelRun` makes, held open.
        let mut cancel_tx = pool.begin().await.unwrap();
        let cancelled = sqlx::query(
            "UPDATE runs SET status = 'cancelled', ended_at = now(), updated_at = now()
             WHERE id = $1 AND status NOT IN ('completed', 'failed', 'cancelled')",
        )
        .bind(&run.run_id)
        .execute(&mut *cancel_tx)
        .await
        .unwrap();
        assert_eq!(cancelled.rows_affected(), 1);
        // And the fold purge `CancelRun` makes in the same transaction, so no test
        // path ends a run without it.
        crate::loop_checkpoint::purge_run(&mut cancel_tx, &run.run_id)
            .await
            .unwrap();

        let racing = {
            let pool = pool.clone();
            let request = completion(&run, "Svar.", &new_ulid());
            let org_id = run.org_id.clone();
            tokio::spawn(async move { complete(&pool, request, &org_id).await })
        };
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        assert!(
            !racing.is_finished(),
            "the completion waits for the run row"
        );
        cancel_tx.commit().await.unwrap();

        let response = racing.await.unwrap().expect("complete after the cancel");
        assert!(response.already_terminal);
        assert!(assistant_messages(&pool, &run.thread_id).await.is_empty());
        assert_eq!(run_status(&pool, &run.run_id).await, "cancelled");
        assert_eq!(run_events(&pool, &run.run_id, "RUN_COMPLETED").await, 0);
    }

    /// The real `CancelRun` racing the completion: every run ends one way or
    /// the other, never with an answer saved on a cancelled run.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn a_racing_cancel_never_leaves_an_answer_on_a_cancelled_run() {
        let Some(pool) = disposable_pool().await else {
            return;
        };
        let (mut completed, mut cancelled) = (0, 0);
        for _ in 0..20 {
            let run = managed_run(&pool, pb::ManagedRunSource::GatewayDirect, false).await;
            let (completion_result, cancel_result) = tokio::join!(
                complete(&pool, completion(&run, "Svar.", &new_ulid()), &run.org_id),
                cancel(&pool, &run),
            );
            let response = completion_result.expect("complete");
            let saved = assistant_messages(&pool, &run.thread_id).await.len();
            match run_status(&pool, &run.run_id).await.as_str() {
                "completed" => {
                    assert!(!response.already_terminal && !cancel_result);
                    assert_eq!(saved, 1);
                    completed += 1;
                }
                "cancelled" => {
                    assert!(response.already_terminal && cancel_result);
                    assert_eq!(saved, 0, "no answer on a cancelled run");
                    assert_eq!(run_events(&pool, &run.run_id, "RUN_COMPLETED").await, 0);
                    cancelled += 1;
                }
                other => panic!("unexpected run status {other}"),
            }
        }
        eprintln!("racing cancel: {completed} completed, {cancelled} cancelled");
    }

    /// Only the Gateway workload holding `session:terminalize`, in the run's
    /// own org, may complete a direct run. Nothing is written otherwise.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn only_the_gateway_terminalizer_of_the_runs_org_may_complete() {
        let Some(pool) = disposable_pool().await else {
            return;
        };
        let run = managed_run(&pool, pb::ManagedRunSource::GatewayDirect, false).await;
        let callers = [
            VerifiedIdentity::user_for_test(&run.org_id, &run.user_id),
            VerifiedIdentity::service_for_test_as(
                &run.org_id,
                GATEWAY,
                &["session:heartbeat"],
                false,
            ),
            VerifiedIdentity::service_for_test_as(&run.org_id, GATEWAY, &["session:write"], false),
            VerifiedIdentity::service_for_test_as(
                &run.org_id,
                "service:execution-core",
                &["session:terminalize"],
                false,
            ),
            VerifiedIdentity::service_for_test_as(
                "another-org",
                GATEWAY,
                &["session:terminalize"],
                false,
            ),
        ];
        for caller in callers {
            let refused = lifecycle(&pool)
                .complete_direct_run_with_message(as_caller(
                    completion(&run, "Svar.", &new_ulid()),
                    caller.clone(),
                ))
                .await
                .expect_err("refused");
            assert_eq!(refused.code(), tonic::Code::PermissionDenied, "{caller:?}");
        }
        // The gateway's credential for another org, as Auth Core issues it.
        let refused = lifecycle(&pool)
            .complete_direct_run_with_message(as_gateway(
                completion(&run, "Svar.", &new_ulid()),
                "another-org",
            ))
            .await
            .expect_err("another org's credential");
        assert_eq!(refused.code(), tonic::Code::PermissionDenied);

        assert!(assistant_messages(&pool, &run.thread_id).await.is_empty());
        assert_eq!(run_status(&pool, &run.run_id).await, "queued");
        assert_eq!(
            obligation_state(&pool, &run.run_id).await,
            ("open".to_owned(), None)
        );

        // Regression, 2026-09-24: that credential is `zdr: true` for every
        // turn, and the durable run is still the gateway's to complete.
        let response = complete(&pool, completion(&run, "Svar.", &new_ulid()), &run.org_id)
            .await
            .expect("the real gateway credential completes a durable run");
        assert!(!response.already_terminal);
        assert_eq!(assistant_messages(&pool, &run.thread_id).await.len(), 1);
        assert_eq!(run_status(&pool, &run.run_id).await, "completed");
    }

    /// A zero-retention run, another thread, and a run another producer owns
    /// are refused before anything is written.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn refuses_zdr_runs_foreign_threads_and_other_producers_without_writing() {
        let Some(pool) = disposable_pool().await else {
            return;
        };
        let zdr = managed_run(&pool, pb::ManagedRunSource::GatewayDirect, true).await;
        let refused = complete(&pool, completion(&zdr, "Svar.", &new_ulid()), &zdr.org_id)
            .await
            .expect_err("a ZDR run has no durable answer");
        assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
        assert_eq!(
            refused.message(),
            "a zero-retention run has no durable assistant message",
            "the run's own retention refuses it, not the credential's"
        );
        let zdr_messages: i64 =
            sqlx::query_scalar("SELECT count(*) FROM messages WHERE thread_id = $1")
                .bind(&zdr.thread_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(zdr_messages, 0);
        assert_ne!(run_status(&pool, &zdr.run_id).await, "completed");

        let run = managed_run(&pool, pb::ManagedRunSource::GatewayDirect, false).await;
        let elsewhere = managed_run(&pool, pb::ManagedRunSource::GatewayDirect, false).await;
        let mut misdirected = completion(&run, "Svar.", &new_ulid());
        misdirected.thread_id.clone_from(&elsewhere.thread_id);
        let refused = complete(&pool, misdirected, &run.org_id)
            .await
            .expect_err("only the run's own thread");
        assert_eq!(refused.code(), tonic::Code::PermissionDenied);
        assert!(assistant_messages(&pool, &elsewhere.thread_id)
            .await
            .is_empty());
        assert_eq!(run_status(&pool, &run.run_id).await, "queued");

        let agent = managed_run(&pool, pb::ManagedRunSource::ExecutionAgent, false).await;
        let refused = complete(
            &pool,
            completion(&agent, "Svar.", &new_ulid()),
            &agent.org_id,
        )
        .await
        .expect_err("an execution-agent run is not the Gateway's to complete");
        assert_eq!(refused.code(), tonic::Code::PermissionDenied);
        assert!(assistant_messages(&pool, &agent.thread_id).await.is_empty());
        assert_eq!(run_status(&pool, &agent.run_id).await, "queued");

        let bad_key = complete(&pool, completion(&run, "Svar.", "not a key"), &run.org_id)
            .await
            .expect_err("an idempotency key is an opaque token");
        assert_eq!(bad_key.code(), tonic::Code::InvalidArgument);
    }

    /// An empty answer completes the run without a message, as the two-call
    /// path did.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn an_empty_answer_completes_without_a_message() {
        let Some(pool) = disposable_pool().await else {
            return;
        };
        let run = managed_run(&pool, pb::ManagedRunSource::GatewayDirect, false).await;
        let response = complete(&pool, completion(&run, "  \n ", &new_ulid()), &run.org_id)
            .await
            .expect("complete");
        assert!(!response.already_terminal);
        assert_eq!(response.message_sequence, 0);
        assert!(response.receipt.is_some());
        assert!(assistant_messages(&pool, &run.thread_id).await.is_empty());
        assert_eq!(run_status(&pool, &run.run_id).await, "completed");
    }
}

#[cfg(test)]
mod thread_list_db_tests {
    use super::*;
    use chrono::{Duration, TimeZone};
    use mp_contracts::model_plane::v1::session_core_server::SessionCore;

    fn service(pool: PgPool) -> SessionService {
        SessionService {
            pool,
            retrieval_client: None,
            graph_client: None,
            knowledge_client: None,
            letta_memory: None,
            audit_publisher: None,
            auth: None,
            grant_columns: crate::grpc::ThreadGrantColumns::present(),
        }
    }

    async fn connect() -> PgPool {
        let pool =
            PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required"))
                .await
                .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    /// A thread whose activity is `activity` seconds after a fixed base, so the
    /// list order is known without reading the clock.
    async fn seed_thread(
        pool: &PgPool,
        org: &str,
        user: &str,
        id: &str,
        activity: i64,
        pinned: bool,
        archived: bool,
    ) {
        let base = Utc.with_ymd_and_hms(2026, 10, 1, 0, 0, 0).unwrap();
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id, last_activity_at, pinned_at, archived_at)
             VALUES ($1, $1, $2, $3, $4, $5, $6)",
        )
        .bind(id)
        .bind(org)
        .bind(user)
        .bind(base + Duration::seconds(activity))
        .bind(pinned.then_some(base))
        .bind(archived.then_some(base))
        .execute(pool)
        .await
        .unwrap();
    }

    async fn list(
        svc: &SessionService,
        org: &str,
        user: &str,
        request: pb::ListThreadsRequest,
    ) -> pb::ListThreadsResponse {
        let mut request = Request::new(pb::ListThreadsRequest {
            org_id: org.to_owned(),
            user_id: user.to_owned(),
            ..request
        });
        request
            .extensions_mut()
            .insert(VerifiedIdentity::user_for_test(org, user));
        svc.list_threads(request)
            .await
            .expect("list_threads")
            .into_inner()
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn list_threads_pages_without_gaps_or_repeats() {
        let pool = connect().await;
        let suffix = new_ulid();
        let org = format!("paging-test-{suffix}");
        let user = "paging-user";
        for n in 0..150 {
            seed_thread(
                &pool,
                &org,
                user,
                &format!("{suffix}-{n:03}"),
                n,
                n == 7,
                false,
            )
            .await;
        }
        let mut expected: Vec<(bool, i64, String)> = (0..150)
            .map(|n| (n == 7, n, format!("{suffix}-{n:03}")))
            .collect();
        expected.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        let expected_ids: Vec<String> = expected.into_iter().map(|(_, _, id)| id).collect();

        let svc = service(pool.clone());
        let mut seen = Vec::new();
        let mut pages = 0;
        let mut cursor = String::new();
        loop {
            let page = list(
                &svc,
                &org,
                user,
                pb::ListThreadsRequest {
                    limit: 50,
                    cursor: cursor.clone(),
                    ..Default::default()
                },
            )
            .await;
            pages += 1;
            seen.extend(page.threads.into_iter().map(|thread| thread.thread_id));
            if page.next_cursor.is_empty() {
                break;
            }
            cursor = page.next_cursor;
        }
        assert_eq!(pages, 3, "150 threads at 50 a page");
        assert_eq!(
            seen, expected_ids,
            "no gaps, no repeats, pinned first then newest activity"
        );
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn list_threads_by_id_finds_thread_beyond_80() {
        let pool = connect().await;
        let suffix = new_ulid();
        let org = format!("by-id-test-{suffix}");
        let user = "by-id-user";
        for n in 0..120 {
            seed_thread(
                &pool,
                &org,
                user,
                &format!("{suffix}-{n:03}"),
                n,
                false,
                false,
            )
            .await;
        }
        // Activity 0 is the oldest of the 120, so it sits past the 80-thread cap.
        let oldest = format!("{suffix}-000");
        let svc = service(pool.clone());
        let found = list(
            &svc,
            &org,
            user,
            pb::ListThreadsRequest {
                thread_id: oldest.clone(),
                ..Default::default()
            },
        )
        .await;
        assert_eq!(found.threads.len(), 1);
        assert_eq!(found.threads[0].thread_id, oldest);
        assert!(found.next_cursor.is_empty());
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn archived_filter_separates_active_archived_and_all() {
        let pool = connect().await;
        let suffix = new_ulid();
        let org = format!("archive-filter-test-{suffix}");
        let user = "archive-user";
        seed_thread(&pool, &org, user, &format!("{suffix}-a1"), 1, false, false).await;
        seed_thread(&pool, &org, user, &format!("{suffix}-a2"), 2, false, false).await;
        seed_thread(&pool, &org, user, &format!("{suffix}-x1"), 3, false, true).await;
        let svc = service(pool.clone());

        let active = list(&svc, &org, user, pb::ListThreadsRequest::default()).await;
        assert_eq!(active.threads.len(), 2, "ACTIVE is the default");
        assert!(active
            .threads
            .iter()
            .all(|thread| thread.archived_at.is_none()));

        let archived = list(
            &svc,
            &org,
            user,
            pb::ListThreadsRequest {
                archived: pb::ArchiveFilter::Archived as i32,
                ..Default::default()
            },
        )
        .await;
        assert_eq!(archived.threads.len(), 1);
        assert!(archived.threads[0].archived_at.is_some());

        let all = list(
            &svc,
            &org,
            user,
            pb::ListThreadsRequest {
                archived: pb::ArchiveFilter::All as i32,
                ..Default::default()
            },
        )
        .await;
        assert_eq!(all.threads.len(), 3);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn unarchive_restores_thread_in_list() {
        let pool = connect().await;
        let suffix = new_ulid();
        let org = format!("unarchive-test-{suffix}");
        let user = "unarchive-user";
        let archived_id = format!("{suffix}-x1");
        seed_thread(&pool, &org, user, &archived_id, 1, false, true).await;
        let svc = service(pool.clone());
        let update = |request: pb::UpdateThreadPresentationRequest| {
            let mut request = Request::new(pb::UpdateThreadPresentationRequest {
                org_id: org.clone(),
                thread_id: archived_id.clone(),
                ..request
            });
            request
                .extensions_mut()
                .insert(VerifiedIdentity::user_for_test(&org, user));
            svc.update_thread_presentation(request)
        };

        // Only a restore is accepted here: archiving goes through ArchiveThread.
        let refused = update(pb::UpdateThreadPresentationRequest {
            archived: Some(true),
            ..Default::default()
        })
        .await
        .expect_err("archived=true must be refused");
        assert_eq!(refused.code(), tonic::Code::InvalidArgument);

        // An archived thread still refuses ordinary presentation writes.
        let pin_on_archived = update(pb::UpdateThreadPresentationRequest {
            pinned: Some(true),
            ..Default::default()
        })
        .await
        .expect_err("a pin write must not reach an archived thread");
        assert_eq!(pin_on_archived.code(), tonic::Code::NotFound);

        update(pb::UpdateThreadPresentationRequest {
            archived: Some(false),
            ..Default::default()
        })
        .await
        .expect("restore");
        let active = list(&svc, &org, user, pb::ListThreadsRequest::default()).await;
        assert_eq!(active.threads.len(), 1);
        assert_eq!(active.threads[0].thread_id, archived_id);
        assert!(active.threads[0].archived_at.is_none());
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn re_archive_after_restore_writes_a_second_archived_event() {
        let pool = connect().await;
        let suffix = new_ulid();
        let org = format!("re-archive-test-{suffix}");
        let user = "re-archive-user";
        let thread_id = format!("{suffix}-r1");
        seed_thread(&pool, &org, user, &thread_id, 1, false, false).await;
        let svc = service(pool.clone());
        let archive = || {
            let mut request = Request::new(pb::ArchiveThreadRequest {
                org_id: org.clone(),
                thread_id: thread_id.clone(),
            });
            request
                .extensions_mut()
                .insert(VerifiedIdentity::user_for_test(&org, user));
            svc.archive_thread(request)
        };
        let restore = || {
            let mut request = Request::new(pb::UpdateThreadPresentationRequest {
                org_id: org.clone(),
                thread_id: thread_id.clone(),
                archived: Some(false),
                ..Default::default()
            });
            request
                .extensions_mut()
                .insert(VerifiedIdentity::user_for_test(&org, user));
            svc.update_thread_presentation(request)
        };

        archive().await.expect("first archive");
        restore().await.expect("restore");
        archive().await.expect("re-archive after restore");

        let archived_events: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM events
             WHERE org_id = $1 AND event_type = 'THREAD_ARCHIVED' AND resource_ref = $2",
        )
        .bind(&org)
        .bind(format!("thread:{thread_id}"))
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(archived_events, 2, "each archive writes its own event");
        let archived: bool =
            sqlx::query_scalar("SELECT archived_at IS NOT NULL FROM threads WHERE id = $1")
                .bind(&thread_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(archived);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn foreign_thread_id_is_not_found() {
        let pool = connect().await;
        let suffix = new_ulid();
        let org = format!("foreign-id-test-{suffix}");
        let foreign_thread = format!("{suffix}-foreign");
        seed_thread(&pool, &org, "owner-user", &foreign_thread, 1, false, false).await;
        let svc = service(pool.clone());

        // The by-id read is owner-bound: another member of the org gets an empty
        // listing, which the BFF reports as thread_not_found.
        let other = list(
            &svc,
            &org,
            "other-user",
            pb::ListThreadsRequest {
                thread_id: foreign_thread,
                ..Default::default()
            },
        )
        .await;
        assert!(other.threads.is_empty());
    }
}
