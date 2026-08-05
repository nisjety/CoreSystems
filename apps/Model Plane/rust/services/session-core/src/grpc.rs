//! gRPC server implementing `SessionCore` on :9091.

use chrono::{DateTime, Utc};
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
use sqlx::PgPool;
use std::time::Instant;
use tonic::transport::Channel;
use tonic::{Request, Response, Status};
use tracing::{info, warn};

use crate::auth::{
    authorize_operation, authorize_owner_row, authorize_system_run_owner, identity,
    DelegatedDataPlaneBearer, JwtVerifier, OwnerIntent, VerifiedIdentity,
    DATA_PLANE_AUTH_METADATA_KEY,
};
use crate::letta_adapter::{LettaMemoryAdapter, LettaSearchOutcome};
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

/// Core of `SessionCore::create_thread`, factored out to keep the trait method
/// small. Inserts the thread row and its `THREAD_CREATED` event in one tx.
async fn create_thread_inner(
    pool: &PgPool,
    req: pb::CreateThreadRequest,
) -> Result<Response<pb::CreateThreadResponse>, Status> {
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
        let existing: Option<(String,)> = sqlx::query_as(
            "SELECT id FROM threads WHERE org_id = $1 AND user_id = $2 AND session_key = $3 \
             ORDER BY id LIMIT 1",
        )
        .bind(&req.org_id)
        .bind(&req.user_id)
        .bind(&req.session_key)
        .fetch_optional(pool)
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

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    sqlx::query(
        "INSERT INTO threads (id, session_key, org_id, user_id, created_at) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(&thread_id)
    .bind(&req.session_key)
    .bind(&req.org_id)
    .bind(&req.user_id)
    .bind(now)
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

/// Persist dream-memory candidates to Letta unless the verified caller is Zero
/// Data Retention.
///
/// The gate lives here rather than inside `dreaming::sync_candidates_to_letta`
/// because that function is shared with the background dreaming worker, which
/// has no caller credential at all — it re-reads rows that already survived the
/// RPC-level write gate. Keeping "verified caller implies policy" on the request
/// path leaves the worker unchanged and keeps the decision next to the identity
/// it is derived from.
async fn sync_dream_memory_unless_zdr(
    letta: Option<&LettaMemoryAdapter>,
    retention: MemoryRetention,
    org_id: &str,
    user_id: &str,
    thread_id: &str,
    candidates: &[crate::dreaming::DreamMemoryCandidate],
) {
    if !retention.permits_durable_memory() {
        // Skipped outright: no write-then-delete, and no reliance on
        // letta-bridge's own authorizer refusing us. ZDR content never reaches
        // the wire.
        record_semantic_memory_degraded(ZDR_MEMORY_WRITE_SUPPRESSED);
        warn!(
            reason = ZDR_MEMORY_WRITE_SUPPRESSED,
            thread_id, "durable Letta memory write suppressed for a ZDR caller"
        );
        return;
    }
    crate::dreaming::sync_candidates_to_letta(letta, org_id, user_id, thread_id, candidates).await;
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
    let mut letta_sync: Option<(String, String, Vec<crate::dreaming::DreamMemoryCandidate>)> = None;

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    let row: (i64,) = sqlx::query_as(
        "INSERT INTO messages (id, thread_id, role, content, created_at)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING sequence",
    )
    .bind(&msg_id)
    .bind(&req.thread_id)
    .bind(&req.role)
    .bind(&req.content)
    .bind(now)
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
        let saved = crate::dreaming::persist_candidates(
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
        letta_sync = Some((org_id, user_id, candidates));
    }

    tx.commit()
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    if let Some((org_id, user_id, candidates)) = letta_sync {
        sync_dream_memory_unless_zdr(
            letta,
            retention,
            &org_id,
            &user_id,
            &req.thread_id,
            &candidates,
        )
        .await;
    }

    Ok(Response::new(pb::AppendMessageResponse { sequence }))
}

/// Core of `SessionCore::start_run`, factored out to keep the trait method
/// small. Inserts the run row and its `RUN_STARTED` event in one tx.
async fn start_run_inner(
    pool: &PgPool,
    req: pb::StartRunRequest,
) -> Result<Response<pb::StartRunResponse>, Status> {
    let run_id = new_ulid();
    let now = Utc::now();

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    // P0.4 residency: stamp the configured Model-Plane region (EU default,
    // Sweden Central) onto the run so its processing region is auditable.
    let residency = configured_residency();
    sqlx::query(
        "INSERT INTO runs (id, thread_id, parent_run_id, agent_id, goal, mode, org_id, user_id, status, residency, created_at, updated_at)
         VALUES ($1, $2, NULLIF($3, ''), $4, $5, $6, $7, $8, 'queued', $9, $10, $10)",
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
    .execute(&mut *tx)
    .await
    .map_err(|e| Status::internal(e.to_string()))?;

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
        let mapped = match req.status.as_str() {
            "completed" => "done",
            "failed" => "failed",
            "awaiting_approval" => "awaiting_approval",
            _ => "running",
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
    chrono::NaiveDateTime,
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
            seconds: ts.and_utc().timestamp(),
            nanos: nanos_to_i32(ts.and_utc().timestamp_subsec_nanos()),
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
    let rows = sqlx::query_as::<_, ReplayEventRow>(
        "SELECT e.id, e.event_type, e.payload, e.ts, e.org_id, e.user_id, e.correlation_id, e.causation_id, e.type_url, e.idempotency_key, e.resource_ref, e.producer, e.schema_version
         FROM events e
         JOIN runs r ON e.run_id = r.id
         WHERE r.thread_id = $1
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
        .search_detailed(org_id, thread_id, &query, &[], 8)
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
            start_run_inner(&self.pool, req).await
        }
        .await;
        record_metrics("start_run", started, result.is_ok());
        result
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
            // Org-scoped UPDATE: a run is only mutable by its owning org, so a
            // caller can never flip the mode of another org's run (per-org
            // isolation invariant). A missing row ⟺ wrong org OR unknown run;
            // both surface as not_found without leaking which.
            let row: Option<(String, String)> = sqlx::query_as(
                "UPDATE runs SET mode = $2, updated_at = now()
                 WHERE id = $1 AND org_id = $3 RETURNING id, mode",
            )
            .bind(&req.run_id)
            .bind(mode)
            .bind(&req.org_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| {
                warn!(error = %e, "set_run_mode failed");
                Status::internal(e.to_string())
            })?;
            let (run_id, mode) =
                row.ok_or_else(|| Status::not_found(format!("run {} not found", req.run_id)))?;
            Ok(Response::new(pb::SetRunModeResponse { run_id, mode }))
        }
        .await;
        record_metrics("set_run_mode", started, result.is_ok());
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
            let rows: Vec<Row> = sqlx::query_as(
                "SELECT id, name, description, content, trigger_keywords,
                        trigger_file_patterns, tool_restrictions, enabled, origin
                 FROM agent_skills
                 WHERE org_id = $1 AND (NOT $2 OR enabled)
                 ORDER BY name",
            )
            .bind(&req.org_id)
            .bind(req.enabled_only)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| {
                warn!(error = %e, "list_agent_skills failed");
                Status::internal(e.to_string())
            })?;
            let skills = rows
                .into_iter()
                .map(
                    |(id, name, description, content, kw, fp, tr, enabled, origin)| {
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
            let rows: Vec<(String, String)> = sqlx::query_as(
                "SELECT m.role, m.content
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
                .map(|(role, content)| pb::SessionMessage { role, content })
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
                DateTime<Utc>,
            )> = sqlx::query_as(
                "SELECT
                    t.id,
                    t.session_key,
                    t.created_at,
                    first_user.content AS title,
                    last_message.content AS preview,
                    COALESCE(last_message.created_at, t.created_at) AS updated_at
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
                 WHERE t.org_id = $1 AND t.user_id = $2
                 ORDER BY COALESCE(last_message.created_at, t.created_at) DESC, t.created_at DESC, t.id DESC
                 LIMIT $3",
            )
            .bind(&req.org_id)
            .bind(&req.user_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| {
                warn!(error = %e, "list_threads failed");
                Status::internal(e.to_string())
            })?;

            let threads = rows
                .into_iter()
                .map(
                    |(thread_id, session_key, created_at, title, preview, updated_at)| {
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
            authorize_run_owner(&self.pool, &caller, &req.run_id, OwnerIntent::Mutate).await?;
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
}

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
        append_letta_memory_rows, assemble_segments, authorize_dataplane, complete_step_inner,
        derive_idempotency_hash, finalize_tool_action_inner, pb, reserve_tool_action_inner,
        resolve_dataplane_addr, resolve_residency, semantic_context_search_status,
        support_thread_id, sync_dream_memory_unless_zdr, validate_user_checkpoint, AssemblyInputs,
        DelegatedDataPlaneBearer, LettaMemoryAdapter, MemoryRetention, SemanticContextSearchStatus,
        VerifiedIdentity, DEFAULT_RESIDENCY, HEALTH_SERVICE_NAMES, MAX_USER_CHECKPOINT_ID_BYTES,
        MAX_USER_CHECKPOINT_STATE_BYTES, STEP_COMPLETED_TYPE_URL, ZDR_MEMORY_READ_SUPPRESSED,
    };
    use crate::dreaming::DreamMemoryCandidate;
    use mp_contracts::model_plane::v1::session_core_server::SessionCore;
    use tonic::Request;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

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

    /// Mirrors `auth::issuer_zdr_blocks_every_durable_session_write` at the
    /// Letta boundary: memory is durable by definition, so a ZDR credential must
    /// not persist any. Structural — the adapter is never invoked, so its Auth
    /// Core mock sees zero requests — rather than asserting on a log line.
    #[tokio::test]
    async fn zdr_caller_never_reaches_the_durable_letta_write_surface() {
        let (auth, adapter) = letta_adapter_with_observable_auth().await;
        let candidates = vec![DreamMemoryCandidate {
            scope: "user",
            session_id: None,
            key: "preference:units".to_owned(),
            content: "prefers metric units".to_owned(),
            kind: "preference",
            confidence: 0.9,
            inferred: false,
        }];

        sync_dream_memory_unless_zdr(
            Some(&adapter),
            MemoryRetention::of(&VerifiedIdentity::user_for_test_with_zdr(
                "org-1", "user-1", true,
            )),
            "org-1",
            "user-1",
            "thread-1",
            &candidates,
        )
        .await;
        assert_eq!(
            auth_request_count(&auth).await,
            0,
            "ZDR content must never be put on the wire to letta-bridge"
        );

        sync_dream_memory_unless_zdr(
            Some(&adapter),
            MemoryRetention::of(&VerifiedIdentity::user_for_test("org-1", "user-1")),
            "org-1",
            "user-1",
            "thread-1",
            &candidates,
        )
        .await;
        assert_eq!(
            auth_request_count(&auth).await,
            1,
            "a non-ZDR caller must still persist durable memory"
        );
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
            }))
            .await
            .is_err());

        let cross = svc
            .set_run_mode(Request::new(pb::SetRunModeRequest {
                run_id: run_id.to_owned(),
                mode: "execute".into(),
                org_id: format!("attacker-{sfx}"),
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
}
