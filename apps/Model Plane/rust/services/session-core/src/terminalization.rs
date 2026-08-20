//! Durable, metadata-only managed-run terminalization primitives.
//!
//! The implementation is deliberately separate from the legacy `SessionCore`
//! RPC surface. Managed runs have one source-bound terminalization obligation;
//! producers receive a stable receipt or a conflict, while the recovery loop
//! may only record an observable unknown-outcome failure.

use chrono::{DateTime, Utc};
use mp_contracts::model_plane::v1 as pb;
use mp_events::idempotency::derive_idempotency_hash;
use mp_ids::new_ulid;
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use std::time::Duration;
use tonic::Status;
use uuid::Uuid;

const RUN_STARTED_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.RunStarted";
const THREAD_CREATED_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.ThreadCreated";
const STEP_COMPLETED_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.StepCompleted";
const RUN_TERMINAL_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.RunTerminal";
const RECOVERY_FAILURE_CODE: &str = "outcome_unknown";
const MANAGED_RECOVERY_LEASE_SECS: i64 = 60;
const MANAGED_DEADLINE_SECS: i64 = 15 * 60;
const MANAGED_RECOVERY_BATCH: usize = 32;
const ACTIVITY_RETRY_SECS: i64 = 60;
const MAX_RUN_ID_BYTES: usize = 128;
const MAX_START_KEY_BYTES: usize = 128;
const MAX_GOAL_BYTES: usize = 64 * 1024;
const MAX_AGENT_ID_BYTES: usize = 256;
const MAX_MODE_BYTES: usize = 64;
const TERMINALIZE_SCOPE: &str = "session:terminalize";
const HEARTBEAT_SCOPE: &str = "session:heartbeat";
const MODEL_GATEWAY_PRINCIPAL: &str = "service:model-gateway";
const EXECUTION_CORE_PRINCIPAL: &str = "service:execution-core";

/// The only run-start values that may cross the durable boundary. For a ZDR
/// caller every requester-controlled content field is replaced before SQL or
/// event serialization; the live gateway retains the original prompt instead.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ManagedStartPersistence {
    pub thread_id: String,
    pub parent_run_id: String,
    pub agent_id: String,
    pub goal: String,
    pub mode: String,
    pub run_started_payload: serde_json::Value,
    pub create_metadata_thread: bool,
    pub create_plan: bool,
}

#[must_use]
pub(crate) fn managed_start_persistence(
    request: &pb::StartManagedRunRequest,
    zdr: bool,
) -> ManagedStartPersistence {
    if zdr {
        return ManagedStartPersistence {
            thread_id: String::new(),
            parent_run_id: String::new(),
            agent_id: "zdr-redacted".to_owned(),
            goal: String::new(),
            mode: "execute".to_owned(),
            run_started_payload: serde_json::json!({ "zdr": true }),
            create_metadata_thread: true,
            create_plan: false,
        };
    }

    ManagedStartPersistence {
        thread_id: request.thread_id.clone(),
        parent_run_id: request.parent_run_id.clone(),
        agent_id: request.agent_id.clone(),
        goal: request.goal.clone(),
        mode: request.mode.clone(),
        run_started_payload: serde_json::json!({
            "goal": &request.goal,
            "mode": &request.mode,
            "agent_id": &request.agent_id,
            "zdr": false,
        }),
        create_metadata_thread: false,
        create_plan: true,
    }
}

/// Metadata-only lifecycle receipt/heartbeat authority. This deliberately does
/// not call `authorize_operation`: that helper blocks every ZDR write, whereas
/// this narrow path writes only server-validated identifiers and fixed enums.
#[cfg(test)]
#[allow(clippy::result_large_err)]
pub(crate) fn authorize_metadata_only_terminalization(
    caller: &crate::auth::VerifiedIdentity,
) -> Result<(), Status> {
    caller.require_service_scope(TERMINALIZE_SCOPE)
}

/// Enforce source ownership after credential validation. A general
/// `session:terminalize` capability is deliberately insufficient: only the
/// workload identity that owns the source may settle that source's obligation.
#[allow(clippy::result_large_err)]
pub(crate) fn authorize_terminalization_source(
    caller: &crate::auth::VerifiedIdentity,
    source: pb::ManagedRunSource,
) -> Result<(), Status> {
    authorize_managed_source(caller, source, TERMINALIZE_SCOPE)
}

/// Heartbeats are deliberately split from terminal receipts so a producer
/// lease can be renewed without granting terminal-write authority. They retain
/// the same source-to-principal binding as a terminal receipt.
#[allow(clippy::result_large_err)]
pub(crate) fn authorize_heartbeat_source(
    caller: &crate::auth::VerifiedIdentity,
    source: pb::ManagedRunSource,
) -> Result<(), Status> {
    authorize_managed_source(caller, source, HEARTBEAT_SCOPE)
}

#[allow(clippy::result_large_err)]
fn authorize_managed_source(
    caller: &crate::auth::VerifiedIdentity,
    source: pb::ManagedRunSource,
    required_scope: &str,
) -> Result<(), Status> {
    caller.require_service_scope(required_scope)?;
    let expected_principal = match source {
        pb::ManagedRunSource::GatewayDirect
        | pb::ManagedRunSource::GatewayAgentDispatchRejected
        | pb::ManagedRunSource::GatewayBrowser => MODEL_GATEWAY_PRINCIPAL,
        pb::ManagedRunSource::ExecutionAgent => EXECUTION_CORE_PRINCIPAL,
        // Keep this enum member for forward-compatible wire evolution, but do
        // not make it a live authority until an Execution Core-owned browser
        // terminalizer and its end-to-end receipt contract exist.
        pb::ManagedRunSource::ExecutionBrowser => {
            return Err(Status::permission_denied(
                "execution browser terminalizer is not enabled",
            ));
        }
        pb::ManagedRunSource::Unspecified => {
            return Err(Status::invalid_argument(
                "managed terminal source is required",
            ));
        }
    };
    if caller.principal_id() != expected_principal {
        return Err(Status::permission_denied(
            "managed terminal source requires its owning workload identity",
        ));
    }
    Ok(())
}

/// Fixed, metadata-only terminal step identities. The source binding is
/// server-owned so a producer cannot choose an arbitrary historical step.
pub(crate) fn canonical_terminal_step(source: pb::ManagedRunSource) -> Option<&'static str> {
    match source {
        pb::ManagedRunSource::GatewayDirect => Some("model-gateway-direct-inference-final"),
        pb::ManagedRunSource::ExecutionAgent => Some("execution-core-agent-final"),
        pb::ManagedRunSource::ExecutionBrowser => Some("execution-core-browser-final"),
        pb::ManagedRunSource::GatewayAgentDispatchRejected => {
            Some("model-gateway-agent-dispatch-rejected")
        }
        pb::ManagedRunSource::GatewayBrowser => Some("model-gateway-browser-agent-final"),
        pb::ManagedRunSource::Unspecified => None,
    }
}

/// Sources that can create a normal managed-run obligation. Dispatch rejection
/// is a narrowly-scoped fallback for an execution-agent obligation, never a
/// primary run mode.
pub(crate) fn is_startable_source(source: pb::ManagedRunSource) -> bool {
    matches!(
        source,
        pb::ManagedRunSource::GatewayDirect
            | pb::ManagedRunSource::ExecutionAgent
            | pb::ManagedRunSource::GatewayBrowser
    )
}

/// Must stay in sync with the `failure_code` CHECK on
/// `managed_run_terminalization_outbox` (migration 0015, widened by 0033). A
/// code missing here is rejected before any database access; one missing from
/// the CHECK fails the write instead, so both lists move together.
/// `outcome_unknown` is deliberately absent: it is set only by reconciliation,
/// never submitted by a caller.
const ALLOWED_FAILURE_CODES: &[&str] = &[
    "browser_failed",
    "dispatch_rejected",
    "dispatch_unreachable",
    "execution_failed",
    "inference_failed",
    "provider_timeout",
    "provider_unavailable",
];

/// A terminal result deliberately contains no user/provider/tool data. The
/// borrowed failure classification has already passed the fixed allowlist.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ManagedOutcome<'a> {
    Completed,
    Failed(&'a str),
}

/// Validate the full public outcome grammar before any database access.
#[allow(clippy::result_large_err)]
pub(crate) fn validate_terminal_outcome(
    outcome: pb::TerminalOutcome,
    failure_code: &str,
) -> Result<ManagedOutcome<'_>, Status> {
    match outcome {
        pb::TerminalOutcome::Completed if failure_code.is_empty() => Ok(ManagedOutcome::Completed),
        pb::TerminalOutcome::Completed => Err(Status::invalid_argument(
            "completed terminal outcome cannot include a failure code",
        )),
        pb::TerminalOutcome::Failed
            if ALLOWED_FAILURE_CODES.contains(&failure_code)
                && failure_code.len() <= 64
                && failure_code == failure_code.trim() =>
        {
            Ok(ManagedOutcome::Failed(failure_code))
        }
        pb::TerminalOutcome::Failed => Err(Status::invalid_argument(
            "terminal failure code is not allowlisted",
        )),
        pb::TerminalOutcome::Unspecified => {
            Err(Status::invalid_argument("terminal outcome is required"))
        }
    }
}

/// Validate that the incoming source can settle the persisted obligation. A
/// gateway dispatch rejection is allowed only before Execution Core accepted
/// the agent run, and therefore only records the fixed failed outcome.
#[must_use]
pub(crate) fn source_accepts_outcome(
    configured: pb::ManagedRunSource,
    submitted: pb::ManagedRunSource,
    outcome: ManagedOutcome<'_>,
) -> bool {
    if configured == submitted {
        return true;
    }
    matches!(
        (configured, submitted, outcome),
        (
            pb::ManagedRunSource::ExecutionAgent,
            pb::ManagedRunSource::GatewayAgentDispatchRejected,
            ManagedOutcome::Failed("dispatch_rejected")
        )
    )
}

fn source_storage_name(source: pb::ManagedRunSource) -> Option<&'static str> {
    match source {
        pb::ManagedRunSource::GatewayDirect => Some("gateway_direct"),
        pb::ManagedRunSource::ExecutionAgent => Some("execution_agent"),
        pb::ManagedRunSource::ExecutionBrowser => Some("execution_browser"),
        pb::ManagedRunSource::GatewayAgentDispatchRejected => {
            Some("gateway_agent_dispatch_rejected")
        }
        pb::ManagedRunSource::GatewayBrowser => Some("gateway_browser"),
        pb::ManagedRunSource::Unspecified => None,
    }
}

#[allow(clippy::result_large_err)]
pub(crate) fn source_from_wire(value: i32) -> Result<pb::ManagedRunSource, Status> {
    let source = pb::ManagedRunSource::try_from(value).unwrap_or(pb::ManagedRunSource::Unspecified);
    if source == pb::ManagedRunSource::Unspecified {
        return Err(Status::invalid_argument(
            "managed terminal source is required",
        ));
    }
    Ok(source)
}

#[allow(clippy::result_large_err)]
fn source_from_storage(value: &str) -> Result<pb::ManagedRunSource, Status> {
    match value {
        "gateway_direct" => Ok(pb::ManagedRunSource::GatewayDirect),
        "execution_agent" => Ok(pb::ManagedRunSource::ExecutionAgent),
        "execution_browser" => Ok(pb::ManagedRunSource::ExecutionBrowser),
        "gateway_agent_dispatch_rejected" => Ok(pb::ManagedRunSource::GatewayAgentDispatchRejected),
        "gateway_browser" => Ok(pb::ManagedRunSource::GatewayBrowser),
        _ => Err(Status::internal(
            "managed terminalization source is invalid",
        )),
    }
}

fn outcome_storage_name(outcome: ManagedOutcome<'_>) -> &'static str {
    match outcome {
        ManagedOutcome::Completed => "completed",
        ManagedOutcome::Failed(_) => "failed",
    }
}

fn outcome_status(outcome: ManagedOutcome<'_>) -> &'static str {
    match outcome {
        ManagedOutcome::Completed => "completed",
        ManagedOutcome::Failed(_) => "failed",
    }
}

fn terminal_event_type(outcome: ManagedOutcome<'_>) -> &'static str {
    match outcome {
        ManagedOutcome::Completed => "RUN_COMPLETED",
        ManagedOutcome::Failed(_) => "RUN_FAILED",
    }
}

#[allow(clippy::result_large_err)]
fn validate_token(
    value: &str,
    label: &str,
    maximum: usize,
    allow_empty: bool,
) -> Result<(), Status> {
    if (value.is_empty() && !allow_empty)
        || value.len() > maximum
        || value != value.trim()
        || value.chars().any(char::is_control)
    {
        return Err(Status::invalid_argument(format!("invalid {label}")));
    }
    Ok(())
}

#[allow(clippy::result_large_err)]
fn validate_start_key(value: &str) -> Result<(), Status> {
    if value.is_empty()
        || value.len() > MAX_START_KEY_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(Status::invalid_argument("invalid start_key"));
    }
    Ok(())
}

#[allow(clippy::result_large_err)]
fn validate_managed_start_request(
    request: &pb::StartManagedRunRequest,
    zdr: bool,
) -> Result<pb::ManagedRunSource, Status> {
    validate_start_key(&request.start_key)?;
    if request.goal.len() > MAX_GOAL_BYTES || request.goal.chars().any(char::is_control) {
        return Err(Status::invalid_argument("invalid goal"));
    }
    let source = source_from_wire(request.terminal_source)?;
    if !is_startable_source(source) || canonical_terminal_step(source).is_none() {
        return Err(Status::invalid_argument(
            "unsupported managed terminal source",
        ));
    }
    if zdr {
        if !request.thread_id.is_empty() {
            return Err(Status::failed_precondition(
                "ZDR managed runs require a fresh metadata-only thread",
            ));
        }
        return Ok(source);
    }

    validate_token(&request.thread_id, "thread_id", MAX_RUN_ID_BYTES, false)?;
    validate_token(
        &request.parent_run_id,
        "parent_run_id",
        MAX_RUN_ID_BYTES,
        true,
    )?;
    validate_token(&request.agent_id, "agent_id", MAX_AGENT_ID_BYTES, false)?;
    validate_token(&request.mode, "mode", MAX_MODE_BYTES, false)?;
    Ok(source)
}

fn database_error(error: sqlx::Error) -> Status {
    tracing::error!(%error, "managed terminalization database operation failed");
    Status::internal("managed terminalization persistence failed")
}

fn to_proto_timestamp(timestamp: DateTime<Utc>) -> prost_types::Timestamp {
    prost_types::Timestamp {
        seconds: timestamp.timestamp(),
        nanos: i32::try_from(timestamp.timestamp_subsec_nanos()).unwrap_or(i32::MAX),
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ManagedRunStart {
    pub run_id: String,
    pub thread_id: String,
    pub created_at: DateTime<Utc>,
    pub terminal_step_id: String,
    pub already_started: bool,
}

impl ManagedRunStart {
    #[must_use]
    pub(crate) fn into_proto(self) -> pb::StartManagedRunResponse {
        pb::StartManagedRunResponse {
            run_id: self.run_id,
            created_at: Some(to_proto_timestamp(self.created_at)),
            terminal_step_id: self.terminal_step_id,
            already_started: self.already_started,
            thread_id: self.thread_id,
        }
    }
}

#[derive(FromRow)]
struct ExistingManagedStart {
    run_id: String,
    thread_id: String,
    created_at: DateTime<Utc>,
    configured_source: String,
    configured_terminal_step_id: String,
    zdr: bool,
    parent_run_id: Option<String>,
    agent_id: String,
    goal: String,
    mode: String,
}

fn existing_start_matches(
    existing: &ExistingManagedStart,
    request: &pb::StartManagedRunRequest,
    zdr: bool,
    source: pb::ManagedRunSource,
) -> bool {
    if existing.zdr != zdr
        || source_storage_name(source) != Some(existing.configured_source.as_str())
        || canonical_terminal_step(source) != Some(existing.configured_terminal_step_id.as_str())
    {
        return false;
    }
    // A ZDR start intentionally persists no request content to compare. The
    // opaque start key is therefore the retry identity; a random key must not
    // be reused for a distinct live request.
    if zdr {
        return request.thread_id.is_empty();
    }
    existing.thread_id == request.thread_id
        && existing.parent_run_id.as_deref().unwrap_or_default() == request.parent_run_id
        && existing.agent_id == request.agent_id
        && existing.goal == request.goal
        && existing.mode == request.mode
}

async fn insert_zdr_thread(
    transaction: &mut Transaction<'_, Postgres>,
    org_id: &str,
    user_id: &str,
    now: DateTime<Utc>,
) -> Result<String, Status> {
    let thread_id = new_ulid();
    let session_key = format!("zdr-{}", new_ulid());
    sqlx::query(
        "INSERT INTO threads (id, session_key, org_id, user_id, created_at)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(&thread_id)
    .bind(&session_key)
    .bind(org_id)
    .bind(user_id)
    .bind(now)
    .execute(&mut **transaction)
    .await
    .map_err(database_error)?;

    let resource = format!("thread:{thread_id}");
    let idempotency_key = derive_idempotency_hash(
        "session-core",
        "THREAD_CREATED",
        &resource,
        &format!("{thread_id}:zdr-created"),
    );
    sqlx::query(
        "INSERT INTO events (id, event_type, run_id, payload, ts, org_id, user_id, correlation_id, causation_id, idempotency_key, resource_ref, type_url, producer, schema_version)
         VALUES ($1, 'THREAD_CREATED', $2, $3, $4, $5, $6, $7, '', $8, $9, $10, 'session-core', 1)",
    )
    .bind(new_ulid())
    .bind(&thread_id)
    .bind(serde_json::json!({ "thread_id": &thread_id, "zdr": true }))
    .bind(now)
    .bind(org_id)
    .bind(user_id)
    .bind(&thread_id)
    .bind(idempotency_key)
    .bind(resource)
    .bind(THREAD_CREATED_TYPE_URL)
    .execute(&mut **transaction)
    .await
    .map_err(database_error)?;
    Ok(thread_id)
}

async fn insert_managed_run_started(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
    thread_id: &str,
    request: &pb::StartManagedRunRequest,
    persisted: &ManagedStartPersistence,
    source: pb::ManagedRunSource,
    zdr: bool,
    now: DateTime<Utc>,
) -> Result<(), Status> {
    let residency = super::grpc::configured_residency();
    // The thread was created only after verifying its signed Space decision.
    // Copy that durable, non-secret context here instead of trusting any
    // managed-run caller to restate it. The exact tenant/owner predicate is a
    // second boundary check for direct callers of this inner persistence path.
    let inherited_space_context: (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i64>,
    ) = sqlx::query_as(
        "INSERT INTO runs (id, thread_id, parent_run_id, agent_id, goal, mode, org_id, user_id, status, residency, created_at, updated_at,
                          space_id, space_decision_ref, recipient_audience_ref, recipient_audience_revision, recipient_audience_hash, privacy_policy_ref, resource_authorization_ref, authority_revision)
         SELECT $1, t.id, NULLIF($3, ''), $4, $5, $6, $7, $8, 'queued', $9, $10, $10,
                t.space_id, t.space_decision_ref, t.recipient_audience_ref, t.recipient_audience_revision, t.recipient_audience_hash, t.privacy_policy_ref, t.resource_authorization_ref, t.authority_revision
         FROM threads AS t
         WHERE t.id = $2 AND t.org_id = $7 AND t.user_id = $8
         RETURNING space_id, space_decision_ref, recipient_audience_ref, recipient_audience_revision, recipient_audience_hash, privacy_policy_ref, resource_authorization_ref, authority_revision",
    )
    .bind(run_id)
    .bind(thread_id)
    .bind(&persisted.parent_run_id)
    .bind(&persisted.agent_id)
    .bind(&persisted.goal)
    .bind(&persisted.mode)
    .bind(&request.org_id)
    .bind(&request.user_id)
    .bind(residency)
    .bind(now)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(database_error)?
    .ok_or_else(|| Status::not_found("thread not found for managed run creation"))?;

    let mut run_started_payload = persisted.run_started_payload.clone();
    let payload = run_started_payload
        .as_object_mut()
        .ok_or_else(|| Status::internal("managed run payload must be an object"))?;
    payload.insert(
        "space_id".to_owned(),
        serde_json::json!(inherited_space_context.0),
    );
    payload.insert(
        "space_decision_ref".to_owned(),
        serde_json::json!(inherited_space_context.1),
    );
    payload.insert(
        "recipient_audience_ref".to_owned(),
        serde_json::json!(inherited_space_context.2),
    );
    payload.insert(
        "recipient_audience_revision".to_owned(),
        serde_json::json!(inherited_space_context.3),
    );
    payload.insert(
        "recipient_audience_hash".to_owned(),
        serde_json::json!(inherited_space_context.4),
    );
    payload.insert(
        "privacy_policy_ref".to_owned(),
        serde_json::json!(inherited_space_context.5),
    );
    payload.insert(
        "resource_authorization_ref".to_owned(),
        serde_json::json!(inherited_space_context.6),
    );
    payload.insert(
        "authority_revision".to_owned(),
        serde_json::json!(inherited_space_context.7),
    );

    let resource = format!("run:{run_id}");
    let idempotency_key = derive_idempotency_hash(
        "session-core",
        "RUN_STARTED",
        &resource,
        &format!("{run_id}:started"),
    );
    sqlx::query(
        "INSERT INTO events (id, event_type, run_id, payload, ts, org_id, user_id, correlation_id, causation_id, idempotency_key, resource_ref, type_url, producer, schema_version)
         VALUES ($1, 'RUN_STARTED', $2, $3, $4, $5, $6, $7, '', $8, $9, $10, 'session-core', 1)",
    )
    .bind(new_ulid())
    .bind(run_id)
    .bind(&run_started_payload)
    .bind(now)
    .bind(&request.org_id)
    .bind(&request.user_id)
    .bind(run_id)
    .bind(idempotency_key)
    .bind(resource)
    .bind(RUN_STARTED_TYPE_URL)
    .execute(&mut **transaction)
    .await
    .map_err(database_error)?;

    let source_name = source_storage_name(source)
        .ok_or_else(|| Status::invalid_argument("unsupported managed terminal source"))?;
    let step_id = canonical_terminal_step(source)
        .ok_or_else(|| Status::invalid_argument("unsupported managed terminal source"))?;
    sqlx::query(
        "INSERT INTO managed_run_terminalization_outbox
         (run_id, org_id, user_id, start_key, configured_source, configured_terminal_step_id, zdr, deadline_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(run_id)
    .bind(&request.org_id)
    .bind(&request.user_id)
    .bind(&request.start_key)
    .bind(source_name)
    .bind(step_id)
    .bind(zdr)
    .bind(now + chrono::Duration::seconds(MANAGED_DEADLINE_SECS))
    .execute(&mut **transaction)
    .await
    .map_err(database_error)?;
    Ok(())
}

/// Atomically start a run and install its one terminalization obligation. The
/// caller identity has already been validated and user-pinned by the gRPC
/// boundary; this inner function never trusts a caller-provided identity.
#[allow(clippy::result_large_err)]
pub(crate) async fn start_managed_run_inner(
    pool: &PgPool,
    request: pb::StartManagedRunRequest,
    zdr: bool,
) -> Result<ManagedRunStart, Status> {
    let source = validate_managed_start_request(&request, zdr)?;
    let persisted = managed_start_persistence(&request, zdr);
    let mut transaction = pool.begin().await.map_err(database_error)?;

    // `UNIQUE (org_id, user_id, start_key)` prevents duplicate obligations,
    // but a concurrent read-then-insert would still make one normal retry fail
    // with a constraint error. Serialize the exact durable effect identity so
    // the waiter sees and returns the winner's immutable receipt instead.
    let lock_key = format!(
        "{}\u{1f}{}\u{1f}{}",
        request.org_id, request.user_id, request.start_key
    );
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(lock_key)
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;

    let existing: Option<ExistingManagedStart> = sqlx::query_as(
        "SELECT o.run_id, r.thread_id, r.created_at, o.configured_source,
                o.configured_terminal_step_id, o.zdr, r.parent_run_id,
                r.agent_id, r.goal, r.mode
         FROM managed_run_terminalization_outbox AS o
         JOIN runs AS r ON r.id = o.run_id
         WHERE o.org_id = $1 AND o.user_id = $2 AND o.start_key = $3
         FOR UPDATE OF o, r",
    )
    .bind(&request.org_id)
    .bind(&request.user_id)
    .bind(&request.start_key)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(database_error)?;
    if let Some(existing) = existing {
        if !existing_start_matches(&existing, &request, zdr, source) {
            return Err(Status::already_exists(
                "managed start key is bound to different immutable input",
            ));
        }
        transaction.commit().await.map_err(database_error)?;
        return Ok(ManagedRunStart {
            run_id: existing.run_id,
            thread_id: existing.thread_id,
            created_at: existing.created_at,
            terminal_step_id: existing.configured_terminal_step_id,
            already_started: true,
        });
    }

    let now = Utc::now();
    let thread_id = if persisted.create_metadata_thread {
        insert_zdr_thread(&mut transaction, &request.org_id, &request.user_id, now).await?
    } else {
        persisted.thread_id.clone()
    };
    let run_id = new_ulid();
    insert_managed_run_started(
        &mut transaction,
        &run_id,
        &thread_id,
        &request,
        &persisted,
        source,
        zdr,
        now,
    )
    .await?;
    transaction.commit().await.map_err(database_error)?;

    // Existing StartRun semantics create the UI plan after the atomic run
    // transaction. A ZDR run deliberately has no durable plan body.
    if persisted.create_plan {
        let plan_id = format!("plan_{run_id}");
        if let Err(error) = crate::orchestration_store::create_plan(
            pool,
            &plan_id,
            &thread_id,
            Some(&run_id),
            &persisted.goal,
            &request.org_id,
            &request.user_id,
            &serde_json::json!({ "source": "managed_run_start" }),
        )
        .await
        {
            tracing::warn!(%error, run_id = %run_id, "managed run plan create failed (best-effort)");
        } else if let Err(error) =
            crate::orchestration_store::update_plan_status(pool, &plan_id, "executing").await
        {
            tracing::warn!(%error, run_id = %run_id, "managed run plan executing-transition failed (best-effort)");
        }
    }

    Ok(ManagedRunStart {
        run_id,
        thread_id,
        created_at: now,
        terminal_step_id: canonical_terminal_step(source)
            .expect("validated startable managed source")
            .to_owned(),
        already_started: false,
    })
}

#[derive(FromRow)]
struct ManagedObligationRow {
    run_id: String,
    configured_source: String,
    configured_terminal_step_id: String,
    state: String,
    outcome: Option<String>,
    failure_code: Option<String>,
    applied_source: Option<String>,
    applied_terminal_step_id: Option<String>,
    receipt_id: Option<String>,
    step_index: Option<i64>,
    applied_at: Option<DateTime<Utc>>,
    lease_token_hash: Option<String>,
    lease_expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub(crate) struct ManagedTerminalReceipt {
    pub run_id: String,
    pub source: pb::ManagedRunSource,
    pub terminal_step_id: String,
    pub step_index: u32,
    pub receipt_id: String,
    pub applied_at: DateTime<Utc>,
    pub already_applied: bool,
    pub reconciliation_required: bool,
}

impl ManagedTerminalReceipt {
    #[must_use]
    pub(crate) fn into_proto(self) -> pb::RecordTerminalOutcomeResponse {
        pb::RecordTerminalOutcomeResponse {
            run_id: self.run_id,
            source: self.source as i32,
            terminal_step_id: self.terminal_step_id,
            step_index: self.step_index,
            receipt_id: self.receipt_id,
            applied_at: Some(to_proto_timestamp(self.applied_at)),
            already_applied: self.already_applied,
            reconciliation_required: self.reconciliation_required,
        }
    }
}

fn receipt_from_applied_row(
    row: &ManagedObligationRow,
    already_applied: bool,
) -> Result<ManagedTerminalReceipt, Status> {
    let source = row
        .applied_source
        .as_deref()
        .ok_or_else(|| Status::internal("managed terminal receipt is incomplete"))
        .and_then(source_from_storage)?;
    let step_index = row
        .step_index
        .ok_or_else(|| Status::internal("managed terminal receipt is incomplete"))
        .and_then(|value| {
            u32::try_from(value)
                .map_err(|_| Status::internal("managed terminal step index is invalid"))
        })?;
    Ok(ManagedTerminalReceipt {
        run_id: row.run_id.clone(),
        source,
        terminal_step_id: row
            .applied_terminal_step_id
            .clone()
            .ok_or_else(|| Status::internal("managed terminal receipt is incomplete"))?,
        step_index,
        receipt_id: row
            .receipt_id
            .clone()
            .ok_or_else(|| Status::internal("managed terminal receipt is incomplete"))?,
        applied_at: row
            .applied_at
            .ok_or_else(|| Status::internal("managed terminal receipt is incomplete"))?,
        already_applied,
        reconciliation_required: row.state == "reconciliation_required",
    })
}

async fn lock_run_status(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
) -> Result<String, Status> {
    sqlx::query_scalar("SELECT status FROM runs WHERE id = $1 FOR UPDATE")
        .bind(run_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(database_error)?
        .ok_or_else(|| Status::not_found("run not found"))
}

async fn lock_managed_obligation(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
) -> Result<ManagedObligationRow, Status> {
    sqlx::query_as(
        "SELECT run_id, configured_source, configured_terminal_step_id, state,
                outcome, failure_code, applied_source, applied_terminal_step_id,
                receipt_id, step_index, applied_at,
                lease_token_hash, lease_expires_at
         FROM managed_run_terminalization_outbox
         WHERE run_id = $1
         FOR UPDATE",
    )
    .bind(run_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(database_error)?
    .ok_or_else(|| Status::failed_precondition("run is not managed"))
}

fn managed_step_payload(step_id: &str, outcome: ManagedOutcome<'_>) -> serde_json::Value {
    serde_json::json!({
        "step_id": step_id,
        "status": outcome_status(outcome),
        "terminal": true,
        "managed_terminalization": true,
        "failure_code": match outcome {
            ManagedOutcome::Completed => None,
            ManagedOutcome::Failed(code) => Some(code),
        },
    })
}

async fn insert_or_replay_managed_terminal_step(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
    step_id: &str,
    outcome: ManagedOutcome<'_>,
    now: DateTime<Utc>,
) -> Result<(String, i64), Status> {
    let event_id = new_ulid();
    let resource = format!("run:{run_id}:step:{step_id}");
    let idempotency_key = derive_idempotency_hash(
        "session-core",
        "STEP_COMPLETED",
        &resource,
        &format!("{run_id}:{step_id}"),
    );
    let payload = managed_step_payload(step_id, outcome);
    let inserted: Option<(String, i64)> = sqlx::query_as(
        "INSERT INTO events (id, event_type, run_id, payload, ts, org_id, user_id, correlation_id, causation_id, idempotency_key, resource_ref, type_url, producer, schema_version)
         SELECT $1, 'STEP_COMPLETED', $2, $3, $4, r.org_id, r.user_id, $2, '', $5, $6, $7, 'session-core', 1
         FROM runs AS r WHERE r.id = $2
         ON CONFLICT (org_id, idempotency_key) WHERE idempotency_key <> '' DO NOTHING
         RETURNING id, step_ordinal",
    )
    .bind(&event_id)
    .bind(run_id)
    .bind(&payload)
    .bind(now)
    .bind(&idempotency_key)
    .bind(&resource)
    .bind(STEP_COMPLETED_TYPE_URL)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(database_error)?;
    if let Some(row) = inserted {
        return Ok(row);
    }

    let existing: Option<(String, serde_json::Value, i64)> = sqlx::query_as(
        "SELECT id, payload, step_ordinal
         FROM events
         WHERE run_id = $1 AND event_type = 'STEP_COMPLETED' AND idempotency_key = $2",
    )
    .bind(run_id)
    .bind(&idempotency_key)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(database_error)?;
    let (existing_id, existing_payload, ordinal) = existing.ok_or_else(|| {
        Status::already_exists("managed terminal step identity is already in use")
    })?;
    if existing_payload != payload {
        return Err(Status::already_exists(
            "managed terminal step identity is bound to different immutable input",
        ));
    }
    Ok((existing_id, ordinal))
}

async fn insert_managed_run_terminal_event(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
    outcome: ManagedOutcome<'_>,
    causation_event_id: &str,
    now: DateTime<Utc>,
) -> Result<String, Status> {
    let event_id = new_ulid();
    let resource = format!("run:{run_id}");
    let idempotency_key = derive_idempotency_hash(
        "session-core",
        "RUN_TERMINAL",
        &resource,
        &format!("{run_id}:terminal"),
    );
    let payload = match outcome {
        ManagedOutcome::Completed => serde_json::json!({}),
        ManagedOutcome::Failed(failure_code) => serde_json::json!({ "failure_code": failure_code }),
    };
    let inserted: Option<(String,)> = sqlx::query_as(
        "INSERT INTO events (id, event_type, run_id, payload, ts, org_id, user_id, correlation_id, causation_id, idempotency_key, resource_ref, type_url, producer, schema_version)
         SELECT $1, $2, $3, $4, $5, r.org_id, r.user_id, $3, $6, $7, $8, $9, 'session-core', 1
         FROM runs AS r WHERE r.id = $3
         ON CONFLICT (org_id, idempotency_key) WHERE idempotency_key <> '' DO NOTHING
         RETURNING id",
    )
    .bind(&event_id)
    .bind(terminal_event_type(outcome))
    .bind(run_id)
    .bind(payload)
    .bind(now)
    .bind(causation_event_id)
    .bind(&idempotency_key)
    .bind(&resource)
    .bind(RUN_TERMINAL_TYPE_URL)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(database_error)?;
    inserted
        .map(|(id,)| id)
        .ok_or_else(|| Status::already_exists("run terminal outcome is already assigned"))
}

async fn transition_managed_run_terminal(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
    outcome: ManagedOutcome<'_>,
    now: DateTime<Utc>,
) -> Result<(), Status> {
    let transitioned = sqlx::query(
        "UPDATE runs SET status = $1, ended_at = $2, updated_at = $2
         WHERE id = $3 AND status NOT IN ('completed', 'failed', 'cancelled')",
    )
    .bind(outcome_status(outcome))
    .bind(now)
    .bind(run_id)
    .execute(&mut **transaction)
    .await
    .map_err(database_error)?;
    if transitioned.rows_affected() != 1 {
        return Err(Status::already_exists(
            "run terminal outcome is already assigned",
        ));
    }

    let plan_status = if matches!(outcome, ManagedOutcome::Completed) {
        "completed"
    } else {
        "failed"
    };
    sqlx::query(
        "UPDATE plans SET status = $1, updated_at = $2
         WHERE id = $3 AND status IN ('draft', 'proposed', 'approved', 'executing')",
    )
    .bind(plan_status)
    .bind(now)
    .bind(format!("plan_{run_id}"))
    .execute(&mut **transaction)
    .await
    .map_err(database_error)?;
    Ok(())
}

async fn apply_managed_terminal_outcome(
    transaction: &mut Transaction<'_, Postgres>,
    obligation: &ManagedObligationRow,
    source: pb::ManagedRunSource,
    step_id: &str,
    outcome: ManagedOutcome<'_>,
    reconciliation_required: bool,
    now: DateTime<Utc>,
) -> Result<ManagedTerminalReceipt, Status> {
    let (step_event_id, step_ordinal) = insert_or_replay_managed_terminal_step(
        transaction,
        &obligation.run_id,
        step_id,
        outcome,
        now,
    )
    .await?;
    let receipt_id = insert_managed_run_terminal_event(
        transaction,
        &obligation.run_id,
        outcome,
        &step_event_id,
        now,
    )
    .await?;
    transition_managed_run_terminal(transaction, &obligation.run_id, outcome, now).await?;

    let persisted_state = if reconciliation_required {
        "reconciliation_required"
    } else {
        "applied"
    };
    let persisted_outcome = if reconciliation_required {
        "unknown"
    } else {
        outcome_storage_name(outcome)
    };
    let failure_code = match outcome {
        ManagedOutcome::Completed => None,
        ManagedOutcome::Failed(code) => Some(code),
    };
    let source_name = source_storage_name(source)
        .ok_or_else(|| Status::invalid_argument("unsupported managed terminal source"))?;
    let updated = sqlx::query(
        "UPDATE managed_run_terminalization_outbox
         SET state = $1, outcome = $2, failure_code = $3, applied_source = $4,
             applied_terminal_step_id = $5, step_event_id = $6, receipt_id = $7,
             step_index = $8, applied_at = $9, lease_owner = NULL,
             lease_token_hash = NULL, lease_expires_at = NULL, updated_at = $9
         WHERE run_id = $10",
    )
    .bind(persisted_state)
    .bind(persisted_outcome)
    .bind(failure_code)
    .bind(source_name)
    .bind(step_id)
    .bind(&step_event_id)
    .bind(&receipt_id)
    .bind(step_ordinal)
    .bind(now)
    .bind(&obligation.run_id)
    .execute(&mut **transaction)
    .await
    .map_err(database_error)?;
    if updated.rows_affected() != 1 {
        return Err(Status::internal(
            "managed terminal receipt was not persisted",
        ));
    }

    // §1.1 learning loop: announce the completed run on `mp.v1.run.<id>.event`
    // so capability-core's session review can distil skills from the transcript.
    // This is the path a PLAIN CHAT turn takes — model-gateway terminalizes a
    // direct-inference run as `GatewayDirect` through
    // `ManagedRunLifecycle.RecordTerminalOutcome` — so it is the high-volume
    // surface, not just the agentic one. Only a genuine completion is announced:
    // a failure, and the recovery worker's reconciled `outcome_unknown`, are not
    // learning material. Enqueueing cannot fail the caller by contract; see
    // `learning_events::enqueue_run_completed`.
    if matches!(outcome, ManagedOutcome::Completed) && !reconciliation_required {
        crate::learning_events::enqueue_run_completed(
            transaction,
            &obligation.run_id,
            &receipt_id,
            now,
        )
        .await;
    }

    let step_index = u32::try_from(step_ordinal)
        .map_err(|_| Status::internal("managed terminal step index is invalid"))?;
    Ok(ManagedTerminalReceipt {
        run_id: obligation.run_id.clone(),
        source,
        terminal_step_id: step_id.to_owned(),
        step_index,
        receipt_id,
        applied_at: now,
        already_applied: false,
        reconciliation_required,
    })
}

fn is_terminal_run_status(status: &str) -> bool {
    matches!(status, "completed" | "failed" | "cancelled")
}

async fn supersede_obligation(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
    now: DateTime<Utc>,
) -> Result<(), Status> {
    sqlx::query(
        "UPDATE managed_run_terminalization_outbox
         SET state = 'superseded', lease_owner = NULL, lease_token_hash = NULL,
             lease_expires_at = NULL, updated_at = $2
         WHERE run_id = $1 AND state IN ('open', 'processing')",
    )
    .bind(run_id)
    .bind(now)
    .execute(&mut **transaction)
    .await
    .map_err(database_error)?;
    Ok(())
}

/// Persist and apply a producer outcome in one database transaction. A lost
/// RPC response is safe: an exact retry returns the stable immutable receipt.
#[allow(clippy::result_large_err)]
pub(crate) async fn record_terminal_outcome_inner(
    pool: &PgPool,
    request: pb::RecordTerminalOutcomeRequest,
) -> Result<ManagedTerminalReceipt, Status> {
    validate_token(&request.run_id, "run_id", MAX_RUN_ID_BYTES, false)?;
    let submitted_source = source_from_wire(request.source)?;
    let outcome =
        pb::TerminalOutcome::try_from(request.outcome).unwrap_or(pb::TerminalOutcome::Unspecified);
    let outcome = validate_terminal_outcome(outcome, &request.failure_code)?;
    if canonical_terminal_step(submitted_source).is_none() {
        return Err(Status::invalid_argument(
            "unsupported managed terminal source",
        ));
    }

    let mut transaction = pool.begin().await.map_err(database_error)?;
    let run_status = lock_run_status(&mut transaction, &request.run_id).await?;
    let obligation = lock_managed_obligation(&mut transaction, &request.run_id).await?;

    if obligation.state == "applied" {
        let source_name = source_storage_name(submitted_source)
            .ok_or_else(|| Status::invalid_argument("unsupported managed terminal source"))?;
        if obligation.applied_source.as_deref() == Some(source_name)
            && obligation.outcome.as_deref() == Some(outcome_storage_name(outcome))
            && obligation.failure_code.as_deref()
                == match outcome {
                    ManagedOutcome::Completed => None,
                    ManagedOutcome::Failed(code) => Some(code),
                }
        {
            let receipt = receipt_from_applied_row(&obligation, true)?;
            transaction.commit().await.map_err(database_error)?;
            return Ok(receipt);
        }
        return Err(Status::already_exists(
            "managed terminal outcome is already assigned",
        ));
    }
    if obligation.state == "reconciliation_required" || obligation.state == "superseded" {
        return Err(Status::already_exists(
            "managed terminal outcome is already assigned",
        ));
    }
    if obligation.state == "processing" {
        return Err(Status::unavailable(
            "managed terminalization recovery is in progress; retry receipt request",
        ));
    }
    if is_terminal_run_status(&run_status) {
        supersede_obligation(&mut transaction, &request.run_id, Utc::now()).await?;
        transaction.commit().await.map_err(database_error)?;
        return Err(Status::already_exists(
            "run terminal outcome is already assigned",
        ));
    }

    let configured_source = source_from_storage(&obligation.configured_source)?;
    if !source_accepts_outcome(configured_source, submitted_source, outcome) {
        return Err(Status::permission_denied(
            "managed terminal source is not authorized for this run",
        ));
    }
    let step_id = canonical_terminal_step(submitted_source)
        .ok_or_else(|| Status::invalid_argument("unsupported managed terminal source"))?;
    let receipt = apply_managed_terminal_outcome(
        &mut transaction,
        &obligation,
        submitted_source,
        step_id,
        outcome,
        false,
        Utc::now(),
    )
    .await?;
    transaction.commit().await.map_err(database_error)?;
    Ok(receipt)
}

#[derive(Debug, Clone)]
pub(crate) struct ManagedHeartbeat {
    pub renewed_until: Option<DateTime<Utc>>,
    pub already_terminal: bool,
}

impl ManagedHeartbeat {
    #[must_use]
    pub(crate) fn into_proto(self) -> pb::HeartbeatManagedRunResponse {
        pb::HeartbeatManagedRunResponse {
            renewed_until: self.renewed_until.map(to_proto_timestamp),
            already_terminal: self.already_terminal,
        }
    }
}

/// Renew a fixed server-owned liveness deadline. A worker may lease a due row;
/// a producer heartbeat can release that un-applied lease, but never re-open an
/// applied, reconciled, superseded, or cancelled outcome.
#[allow(clippy::result_large_err)]
pub(crate) async fn heartbeat_managed_run_inner(
    pool: &PgPool,
    request: pb::HeartbeatManagedRunRequest,
) -> Result<ManagedHeartbeat, Status> {
    validate_token(&request.run_id, "run_id", MAX_RUN_ID_BYTES, false)?;
    let source = source_from_wire(request.source)?;
    if !is_startable_source(source) {
        return Err(Status::invalid_argument(
            "unsupported managed heartbeat source",
        ));
    }
    let mut transaction = pool.begin().await.map_err(database_error)?;
    let run_status = lock_run_status(&mut transaction, &request.run_id).await?;
    let obligation = lock_managed_obligation(&mut transaction, &request.run_id).await?;
    if is_terminal_run_status(&run_status)
        || matches!(
            obligation.state.as_str(),
            "applied" | "reconciliation_required" | "superseded"
        )
    {
        if is_terminal_run_status(&run_status) && obligation.state != "applied" {
            supersede_obligation(&mut transaction, &request.run_id, Utc::now()).await?;
        }
        transaction.commit().await.map_err(database_error)?;
        return Ok(ManagedHeartbeat {
            renewed_until: None,
            already_terminal: true,
        });
    }
    let configured_source = source_from_storage(&obligation.configured_source)?;
    if configured_source != source {
        return Err(Status::permission_denied(
            "managed heartbeat source is not authorized for this run",
        ));
    }
    let now = Utc::now();
    let renewed_until = now + chrono::Duration::seconds(MANAGED_DEADLINE_SECS);
    let renewed = sqlx::query(
        "UPDATE managed_run_terminalization_outbox
         SET state = 'open', deadline_at = $1, next_attempt_at = $2,
             lease_owner = NULL, lease_token_hash = NULL, lease_expires_at = NULL,
             updated_at = $2
         WHERE run_id = $3 AND state IN ('open', 'processing')",
    )
    .bind(renewed_until)
    .bind(now)
    .bind(&request.run_id)
    .execute(&mut *transaction)
    .await
    .map_err(database_error)?;
    if renewed.rows_affected() != 1 {
        return Err(Status::unavailable(
            "managed terminalization heartbeat could not renew; retry",
        ));
    }
    transaction.commit().await.map_err(database_error)?;
    Ok(ManagedHeartbeat {
        renewed_until: Some(renewed_until),
        already_terminal: false,
    })
}

#[derive(FromRow)]
struct ClaimedRecoveryRow {
    run_id: String,
}

struct ClaimedRecovery {
    row: ClaimedRecoveryRow,
    lease_token: String,
}

fn new_lease_token() -> String {
    Uuid::new_v4().simple().to_string()
}

fn hash_lease_token(token: &str) -> String {
    blake3::hash(token.as_bytes()).to_hex().to_string()
}

async fn supersede_terminalized_obligations(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE managed_run_terminalization_outbox AS o
         SET state = 'superseded', lease_owner = NULL, lease_token_hash = NULL,
             lease_expires_at = NULL, updated_at = now()
         FROM runs AS r
         WHERE o.run_id = r.id
           AND o.state IN ('open', 'processing')
           AND r.status IN ('completed', 'failed', 'cancelled')",
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

async fn claim_due_recovery(pool: &PgPool) -> Result<Option<ClaimedRecovery>, sqlx::Error> {
    let lease_token = new_lease_token();
    let lease_token_hash = hash_lease_token(&lease_token);
    let row = sqlx::query_as::<_, ClaimedRecoveryRow>(
        "WITH candidate AS (
             SELECT o.run_id
             FROM managed_run_terminalization_outbox AS o
             JOIN runs AS r ON r.id = o.run_id
             WHERE (
                    (o.state = 'open'
                     AND o.deadline_at <= now()
                     AND o.next_attempt_at <= now())
                 OR (o.state = 'processing'
                     AND o.lease_expires_at <= now())
             )
               AND r.status NOT IN ('completed', 'failed', 'cancelled')
               AND NOT EXISTS (
                    SELECT 1 FROM approvals AS a
                    WHERE a.run_id = o.run_id
                      AND a.org_id = o.org_id
                      AND a.user_id = o.user_id
                      AND a.status IN ('requested', 'granted')
               )
             ORDER BY o.deadline_at ASC, o.created_at ASC
             FOR UPDATE SKIP LOCKED
             LIMIT 1
         )
         UPDATE managed_run_terminalization_outbox AS o
         SET state = 'processing', attempts = o.attempts + 1,
             lease_owner = 'session-core-terminalizer',
             lease_token_hash = $1,
             lease_expires_at = now() + ($2::bigint * interval '1 second'),
             updated_at = now()
         FROM candidate
         WHERE o.run_id = candidate.run_id
         RETURNING o.run_id",
    )
    .bind(lease_token_hash)
    .bind(MANAGED_RECOVERY_LEASE_SECS)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| ClaimedRecovery { row, lease_token }))
}

async fn has_pending_approval(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
) -> Result<bool, Status> {
    sqlx::query_scalar(
        "SELECT EXISTS(
             SELECT 1 FROM approvals
             WHERE run_id = $1 AND status IN ('requested', 'granted')
         )",
    )
    .bind(run_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(database_error)
}

async fn release_recovery_for_activity(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
    now: DateTime<Utc>,
) -> Result<(), Status> {
    sqlx::query(
        "UPDATE managed_run_terminalization_outbox
         SET state = 'open', next_attempt_at = $2 + ($3::bigint * interval '1 second'),
             lease_owner = NULL, lease_token_hash = NULL, lease_expires_at = NULL,
             updated_at = $2
         WHERE run_id = $1 AND state = 'processing'",
    )
    .bind(run_id)
    .bind(now)
    .bind(ACTIVITY_RETRY_SECS)
    .execute(&mut **transaction)
    .await
    .map_err(database_error)?;
    Ok(())
}

async fn recover_claimed_terminalization(
    pool: &PgPool,
    claim: ClaimedRecovery,
) -> Result<bool, Status> {
    let now = Utc::now();
    let token_hash = hash_lease_token(&claim.lease_token);
    let mut transaction = pool.begin().await.map_err(database_error)?;
    let run_status = lock_run_status(&mut transaction, &claim.row.run_id).await?;
    let obligation = lock_managed_obligation(&mut transaction, &claim.row.run_id).await?;
    if obligation.state != "processing"
        || obligation.lease_token_hash.as_deref() != Some(token_hash.as_str())
        || obligation
            .lease_expires_at
            .is_none_or(|expires_at| expires_at <= now)
    {
        transaction.commit().await.map_err(database_error)?;
        return Ok(false);
    }
    if is_terminal_run_status(&run_status) {
        supersede_obligation(&mut transaction, &claim.row.run_id, now).await?;
        transaction.commit().await.map_err(database_error)?;
        return Ok(false);
    }
    if has_pending_approval(&mut transaction, &claim.row.run_id).await? {
        release_recovery_for_activity(&mut transaction, &claim.row.run_id, now).await?;
        transaction.commit().await.map_err(database_error)?;
        return Ok(false);
    }
    let source = source_from_storage(&obligation.configured_source)?;
    let step_id = canonical_terminal_step(source)
        .ok_or_else(|| Status::internal("managed terminalization source is invalid"))?;
    if step_id != obligation.configured_terminal_step_id {
        return Err(Status::internal(
            "managed terminalization step mapping is invalid",
        ));
    }
    let receipt = apply_managed_terminal_outcome(
        &mut transaction,
        &obligation,
        source,
        step_id,
        ManagedOutcome::Failed(RECOVERY_FAILURE_CODE),
        true,
        now,
    )
    .await?;
    transaction.commit().await.map_err(database_error)?;
    metrics::counter!("mp_session_managed_terminalizations_recovered_total").increment(1);
    tracing::warn!(
        run_id = %receipt.run_id,
        receipt_id = %receipt.receipt_id,
        "managed run terminalized as outcome_unknown after producer loss"
    );
    Ok(true)
}

/// Process due terminalization obligations without external side effects. The
/// worker records only a fixed unknown-outcome failure; it never reruns a tool,
/// browser action, or provider call.
pub(crate) async fn recover_due_terminalizations(pool: &PgPool) -> Result<u64, Status> {
    supersede_terminalized_obligations(pool)
        .await
        .map_err(database_error)?;
    let mut recovered = 0_u64;
    for _ in 0..MANAGED_RECOVERY_BATCH {
        let Some(claim) = claim_due_recovery(pool).await.map_err(database_error)? else {
            break;
        };
        if recover_claimed_terminalization(pool, claim).await? {
            recovered += 1;
        }
    }
    Ok(recovered)
}

/// Session Core-owned recovery loop. Errors are logged and retried; durable
/// leases make restart safe and prevent a stale worker from completing a row.
pub(crate) async fn run_recovery_worker(pool: PgPool) -> anyhow::Result<()> {
    loop {
        match recover_due_terminalizations(&pool).await {
            Ok(recovered) => {
                metrics::gauge!("mp_session_managed_terminalizations_recovered_last_batch")
                    .set(recovered as f64);
            }
            Err(error) => {
                tracing::warn!(%error, "managed terminalization recovery deferred");
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        authorize_terminalization_source, canonical_terminal_step, is_startable_source,
        validate_terminal_outcome, ManagedOutcome,
    };
    use mp_contracts::model_plane::v1 as pb;

    #[test]
    fn source_selects_a_server_owned_canonical_terminal_step() {
        assert_eq!(
            canonical_terminal_step(pb::ManagedRunSource::GatewayDirect),
            Some("model-gateway-direct-inference-final")
        );
        assert_eq!(
            canonical_terminal_step(pb::ManagedRunSource::ExecutionAgent),
            Some("execution-core-agent-final")
        );
        assert_eq!(
            canonical_terminal_step(pb::ManagedRunSource::ExecutionBrowser),
            Some("execution-core-browser-final")
        );
        assert_eq!(
            canonical_terminal_step(pb::ManagedRunSource::GatewayBrowser),
            Some("model-gateway-browser-agent-final")
        );
        assert_eq!(
            canonical_terminal_step(pb::ManagedRunSource::GatewayAgentDispatchRejected),
            Some("model-gateway-agent-dispatch-rejected")
        );
        assert_eq!(
            canonical_terminal_step(pb::ManagedRunSource::Unspecified),
            None
        );
    }

    #[test]
    fn terminal_outcome_accepts_only_fixed_metadata_and_rejects_content_like_failure() {
        assert_eq!(
            validate_terminal_outcome(pb::TerminalOutcome::Completed, "")
                .expect("completed has no failure detail"),
            ManagedOutcome::Completed
        );
        assert_eq!(
            validate_terminal_outcome(pb::TerminalOutcome::Failed, "provider_timeout")
                .expect("fixed failure code"),
            ManagedOutcome::Failed("provider_timeout")
        );

        for (outcome, code) in [
            (pb::TerminalOutcome::Completed, "provider_timeout"),
            (pb::TerminalOutcome::Failed, "raw provider answer: secret"),
            (pb::TerminalOutcome::Failed, ""),
            (pb::TerminalOutcome::Unspecified, ""),
        ] {
            assert!(
                validate_terminal_outcome(outcome, code).is_err(),
                "outcome={outcome:?} code={code:?} must fail closed"
            );
        }
    }

    /// Gateway distinguishes a refused dispatch from one that never reached this
    /// service, so both codes must clear the allowlist — a code the allowlist
    /// does not know is rejected before any database access, which would turn
    /// the gateway's terminalization into `session_terminalization_failed` and
    /// strand the very run it was trying to close.
    #[test]
    fn both_gateway_dispatch_failure_codes_are_allowlisted() {
        for code in ["dispatch_rejected", "dispatch_unreachable"] {
            assert_eq!(
                validate_terminal_outcome(pb::TerminalOutcome::Failed, code)
                    .unwrap_or_else(|error| panic!("{code} must be allowlisted: {error}")),
                ManagedOutcome::Failed(code)
            );
        }
        assert!(
            validate_terminal_outcome(pb::TerminalOutcome::Failed, "outcome_unknown").is_err(),
            "outcome_unknown is set by reconciliation only, never submitted by a caller"
        );
    }

    #[test]
    fn gateway_dispatch_rejection_is_only_a_failed_dispatch_rejection() {
        assert!(super::source_accepts_outcome(
            pb::ManagedRunSource::ExecutionAgent,
            pb::ManagedRunSource::GatewayAgentDispatchRejected,
            ManagedOutcome::Failed("dispatch_rejected"),
        ));
        assert!(!super::source_accepts_outcome(
            pb::ManagedRunSource::ExecutionAgent,
            pb::ManagedRunSource::GatewayAgentDispatchRejected,
            ManagedOutcome::Completed,
        ));
        assert!(!super::source_accepts_outcome(
            pb::ManagedRunSource::GatewayDirect,
            pb::ManagedRunSource::GatewayAgentDispatchRejected,
            ManagedOutcome::Failed("dispatch_rejected"),
        ));
    }

    #[test]
    fn zdr_managed_start_persists_only_metadata_and_never_the_prompt() {
        let request = pb::StartManagedRunRequest {
            thread_id: String::new(),
            parent_run_id: "parent-with-content-is-not-accepted".to_owned(),
            agent_id: "agent-name-must-not-persist".to_owned(),
            goal: "highly sensitive customer prompt".to_owned(),
            mode: "research-with-sensitive-context".to_owned(),
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
            start_key: "opaque-start-1".to_owned(),
            terminal_source: pb::ManagedRunSource::GatewayDirect as i32,
        };

        let persisted = super::managed_start_persistence(&request, true);
        assert!(persisted.create_metadata_thread);
        assert!(!persisted.create_plan);
        assert_eq!(persisted.goal, "");
        assert_eq!(persisted.agent_id, "zdr-redacted");
        assert_eq!(persisted.mode, "execute");
        assert_eq!(persisted.parent_run_id, "");
        let serialized = serde_json::to_string(&persisted.run_started_payload)
            .expect("serializable metadata-only event");
        for forbidden in [
            "highly sensitive customer prompt",
            "agent-name-must-not-persist",
            "research-with-sensitive-context",
            "parent-with-content-is-not-accepted",
        ] {
            assert!(
                !serialized.contains(forbidden),
                "ZDR durable event leaked request content: {forbidden}"
            );
        }
        assert_eq!(
            persisted.run_started_payload,
            serde_json::json!({ "zdr": true })
        );
    }

    #[test]
    fn metadata_only_terminalization_requires_a_scoped_service_not_a_user() {
        let service = crate::auth::VerifiedIdentity::service_for_test(
            "org-1",
            &["session:terminalize"],
            true,
        );
        super::authorize_metadata_only_terminalization(&service)
            .expect("ZDR service may submit metadata-only receipt");

        let user = crate::auth::VerifiedIdentity::user_for_test_with_zdr("org-1", "user-1", true);
        assert_eq!(
            super::authorize_metadata_only_terminalization(&user)
                .expect_err("user credential must never terminalize a managed run")
                .code(),
            tonic::Code::PermissionDenied
        );
    }

    #[test]
    fn terminal_sources_are_bound_to_their_issuing_workload_principal() {
        let gateway = crate::auth::VerifiedIdentity::service_for_test_as(
            "org-1",
            "service:model-gateway",
            &["session:terminalize"],
            false,
        );
        let execution = crate::auth::VerifiedIdentity::service_for_test_as(
            "org-1",
            "service:execution-core",
            &["session:terminalize"],
            false,
        );
        let other = crate::auth::VerifiedIdentity::service_for_test_as(
            "org-1",
            "service:another-service",
            &["session:terminalize"],
            false,
        );

        for source in [
            pb::ManagedRunSource::GatewayDirect,
            pb::ManagedRunSource::GatewayAgentDispatchRejected,
            pb::ManagedRunSource::GatewayBrowser,
        ] {
            authorize_terminalization_source(&gateway, source)
                .expect("model gateway owns its source-bound terminalizers");
            assert_eq!(
                authorize_terminalization_source(&execution, source)
                    .expect_err("execution core must not impersonate model gateway")
                    .code(),
                tonic::Code::PermissionDenied
            );
        }

        authorize_terminalization_source(&execution, pb::ManagedRunSource::ExecutionAgent)
            .expect("execution core owns an agent-loop terminalizer");
        assert_eq!(
            authorize_terminalization_source(&gateway, pb::ManagedRunSource::ExecutionAgent)
                .expect_err("model gateway must not impersonate execution core")
                .code(),
            tonic::Code::PermissionDenied
        );
        assert_eq!(
            authorize_terminalization_source(&other, pb::ManagedRunSource::GatewayDirect)
                .expect_err("unrelated service must not terminalize")
                .code(),
            tonic::Code::PermissionDenied
        );

        assert!(!is_startable_source(pb::ManagedRunSource::ExecutionBrowser));
        assert_eq!(
            authorize_terminalization_source(&execution, pb::ManagedRunSource::ExecutionBrowser)
                .expect_err("future execution-browser source is not enabled")
                .code(),
            tonic::Code::PermissionDenied
        );
    }
}
