//! Durable, metadata-only managed-run terminalization primitives.
//!
//! The implementation is deliberately separate from the legacy `SessionCore`
//! RPC surface. Managed runs have one source-bound terminalization obligation;
//! producers receive a stable receipt or a conflict, while the recovery loop
//! may only record an observable unknown-outcome failure.

use crate::continuation_crypto::DescriptorCipher;
use crate::retention_receipt::{self, RetentionClass, RunRetention};
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
pub(crate) const MANAGED_DEADLINE_SECS: i64 = 15 * 60;
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
/// Build plan WS03 S8-0. Off by default; only the literal `true` turns it on (the strict
/// parser the other durable-loop switches share). See [`approval_expiry_enabled`].
const APPROVAL_EXPIRY_ENV: &str = "SESSION_CORE_RECOVERY_APPROVAL_EXPIRY_ENABLED";
/// Build plan WS03 S8-4. Off by default; only the literal `true` turns it on. See
/// [`park_recovery_enabled`] and [`park_recovery_active`].
const PARK_RECOVERY_ENV: &str = "SESSION_CORE_PARK_RECOVERY_ENABLED";
/// Build plan WS03 S8-B. Off by default; only the literal `true` turns it on. See
/// [`crash_park_enabled`] and [`crash_park_active`].
const CRASH_PARK_ENV: &str = "SESSION_CORE_CRASH_PARK_ENABLED";
/// Build plan WS20 S1. Off by default; off keeps the receipt-only scheduled path. See
/// [`scheduled_result_persist_enabled`].
const SCHEDULED_RESULT_PERSIST_ENV: &str = "SESSION_CORE_SCHEDULED_RESULT_PERSIST";

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

/// Credential authority for `CompleteDirectRunWithMessage`: the `GatewayDirect`
/// terminalizer, exactly as for that source's `RecordTerminalOutcome`.
///
/// The credential's `zdr` claim is deliberately not consulted. Auth Core sets a
/// service token's `zdr` from the principal registry's per-audience posture
/// (`retentionByAudience`), never from a user or a turn. Model Gateway's
/// `session-core` posture is `zdr`, so every terminalize token it can hold is
/// `zdr: true`, minted once per org and cached across durable and
/// zero-retention turns alike; refusing on it refused every save. The turn's
/// retention is the run's own `zdr`, which `StartManagedRun` records from the
/// user's verified credential (the claim `AppendMessage` reads off its caller),
/// and `complete_direct_run_with_message_inner` refuses a zero-retention run
/// inside the transaction, before anything is written.
#[allow(clippy::result_large_err)]
pub(crate) fn authorize_direct_completion(
    caller: &crate::auth::VerifiedIdentity,
) -> Result<(), Status> {
    authorize_terminalization_source(caller, pb::ManagedRunSource::GatewayDirect)
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
/// `managed_run_terminalization_outbox` (migration 0015, widened by 0033 and
/// 0069). A
/// code missing here is rejected before any database access; one missing from
/// the CHECK fails the write instead, so both lists move together.
/// `outcome_unknown` is deliberately absent: it is set only by reconciliation,
/// never submitted by a caller.
const ALLOWED_FAILURE_CODES: &[&str] = &[
    "browser_failed",
    "browser_permission_denied",
    "browser_resource_exhausted",
    "browser_timed_out",
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
            return Err(mp_contracts::zdr_refusal::Refusal::feature_not_supported("agent_run")
                .grpc_status(tonic::Code::FailedPrecondition));
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
    retention_class: String,
    waiver_ack_id: Option<String>,
    waiver_connection_id: Option<String>,
}

/// A retry is the same start only if it carries the same receipt. A waiver
/// token for one connection must never adopt a run that was started under
/// another connection's waiver, or under no waiver at all.
fn existing_retention_matches(existing: &ExistingManagedStart, retention: &RunRetention) -> bool {
    RunRetention::from_storage(
        &existing.retention_class,
        existing.waiver_ack_id.clone(),
        existing.waiver_connection_id.clone(),
    )
    .is_ok_and(|stored| stored == *retention)
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
    retention: &RunRetention,
    now: DateTime<Utc>,
) -> Result<(), Status> {
    let zdr = retention.class == RetentionClass::Zdr;
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
                          space_id, space_decision_ref, recipient_audience_ref, recipient_audience_revision, recipient_audience_hash, privacy_policy_ref, resource_authorization_ref, authority_revision,
                          retention_class, waiver_ack_id, waiver_connection_id)
         SELECT $1, t.id, NULLIF($3, ''), $4, $5, $6, $7, $8, 'queued', $9, $10, $10,
                t.space_id, t.space_decision_ref, t.recipient_audience_ref, t.recipient_audience_revision, t.recipient_audience_hash, t.privacy_policy_ref, t.resource_authorization_ref, t.authority_revision,
                $11, $12, $13
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
    .bind(retention.class.as_str())
    .bind(retention.ack_id())
    .bind(retention.connection_id())
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

    // A waived run's start is audited durably, on the same transaction as the
    // run, so the audit row exists if and only if the run does.
    if let Some(body) = retention_receipt::run_started_zdr_waived_body(
        retention,
        run_id,
        &request.org_id,
        &request.user_id,
        thread_id,
        now,
    ) {
        retention_receipt::enqueue_run_started_zdr_waived(transaction, run_id, &body).await?;
    }
    Ok(())
}

/// Start a run whose receipt is the one a bare ZDR flag or a persistent
/// credential gives: no waiver. A caller that holds a waiver uses
/// [`start_managed_run_with_retention_inner`].
#[allow(clippy::result_large_err)]
pub(crate) async fn start_managed_run_inner(
    pool: &PgPool,
    request: pb::StartManagedRunRequest,
    zdr: bool,
) -> Result<ManagedRunStart, Status> {
    let retention = if zdr {
        RunRetention::zdr()
    } else {
        RunRetention::persistent()
    };
    start_managed_run_with_retention_inner(pool, request, retention).await
}

/// Atomically start a run and install its one terminalization obligation. The
/// caller identity has already been validated and user-pinned by the gRPC
/// boundary; this inner function never trusts a caller-provided identity.
///
/// `retention` is the receipt the caller's verified claims give (see
/// [`RunRetention::from_caller`]). It is recorded immutably on the run, and a
/// child run must match its parent's receipt ([`RunRetention::inherit_from`]).
#[allow(clippy::result_large_err)]
pub(crate) async fn start_managed_run_with_retention_inner(
    pool: &PgPool,
    request: pb::StartManagedRunRequest,
    retention: RunRetention,
) -> Result<ManagedRunStart, Status> {
    let zdr = retention.class == RetentionClass::Zdr;
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
                r.agent_id, r.goal, r.mode,
                r.retention_class, r.waiver_ack_id, r.waiver_connection_id
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
        if !existing_start_matches(&existing, &request, zdr, source)
            || !existing_retention_matches(&existing, &retention)
        {
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
    // A child run takes its parent's receipt, and must be this caller's own run
    // in this org. A ZDR start never names a parent (see managed_start_persistence).
    let retention = if zdr || persisted.parent_run_id.is_empty() {
        retention
    } else {
        let parent = retention_receipt::load_parent_retention(
            &mut transaction,
            &request.org_id,
            &request.user_id,
            &persisted.parent_run_id,
        )
        .await?;
        retention.inherit_from(Some(&parent))?
    };
    let thread_id = if persisted.create_metadata_thread {
        insert_zdr_thread(&mut transaction, &request.org_id, &request.user_id, now).await?
    } else {
        persisted.thread_id.clone()
    };
    // One live root run per Space thread (WS09-S1). After the idempotent branch
    // above, so a replay of the same start key is never refused; skipped for a
    // child run and for the metadata thread a ZDR start creates.
    if persisted.parent_run_id.is_empty() && !persisted.create_metadata_thread {
        crate::run_phase::ensure_thread_idle(&mut transaction, &request.org_id, &thread_id).await?;
    }
    let run_id = new_ulid();
    insert_managed_run_started(
        &mut transaction,
        &run_id,
        &thread_id,
        &request,
        &persisted,
        source,
        &retention,
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
    // A terminal run never resumes, so its loop fold is dropped in the same
    // transaction that ends it (build plan WS03 S4).
    crate::loop_checkpoint::purge_run(transaction, run_id)
        .await
        .map_err(database_error)?;

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
    // A scheduled run's fire learns its truthful terminal status in the same
    // transaction that ends the run (build plan WS20 S1). Off: fires keep the
    // status the sweeper wrote.
    if scheduled_result_persist_enabled() {
        settle_scheduled_fire(transaction, run_id, plan_status, now).await?;
    }
    Ok(())
}

/// Link a scheduled run's fire row to the run and set its terminal status. The
/// key is the run's own immutable schedule metadata, so only the fire that
/// started this run is touched, and only while it has not settled yet.
async fn settle_scheduled_fire(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
    status: &str,
    now: DateTime<Utc>,
) -> Result<(), Status> {
    sqlx::query(
        "UPDATE cron_fires AS cf
            SET status = $2, completed_at = $3, run_id = r.id, thread_id = r.thread_id
           FROM runs r
          WHERE r.id = $1
            AND r.metadata->>'source' = 'scheduled_run'
            AND cf.schedule_id = r.metadata->>'schedule_id'
            AND cf.fire_key = r.metadata->>'fire_key'
            AND cf.status IN ('pending', 'running')",
    )
    .bind(run_id)
    .bind(status)
    .bind(now)
    .execute(&mut **transaction)
    .await
    .map_err(database_error)?;
    Ok(())
}

/// Whether a scheduled run's answer is persisted into its thread and its fire
/// records the run (build plan WS20 S1). Off by default; only the literal `true`
/// turns it on, parsed by the strict switch the other durable-loop switches share.
/// Session Core reads it here and in the scheduled-result RPC. Execution Core reads
/// the same name, so both sides of one deploy agree.
pub(crate) fn scheduled_result_persist_enabled() -> bool {
    mp_contracts::loop_fold::switch_from(
        std::env::var(SCHEDULED_RESULT_PERSIST_ENV).ok().as_deref(),
    )
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
    // A heartbeat from a run the worker crash parked says its loop is alive after all: the park
    // is not claimable until a grace period has passed without another one (WS03 S8-B).
    if crash_park_active() {
        crate::loop_checkpoint::push_crash_park(&mut transaction, &request.run_id)
            .await
            .map_err(database_error)?;
    }
    transaction.commit().await.map_err(database_error)?;
    Ok(ManagedHeartbeat {
        renewed_until: Some(renewed_until),
        already_terminal: false,
    })
}

/// Same grammar as a start key and the `completion_key` CHECK (migration
/// 0040): an opaque operation id, never content.
#[allow(clippy::result_large_err)]
fn validate_completion_key(value: &str) -> Result<(), Status> {
    if value.is_empty()
        || value.len() > MAX_START_KEY_BYTES
        || !value.as_bytes()[0].is_ascii_alphanumeric()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(Status::invalid_argument("invalid idempotency_key"));
    }
    Ok(())
}

/// What `CompleteDirectRunWithMessage` did.
#[derive(Debug, Clone)]
pub(crate) enum DirectRunCompletion {
    /// The run was no longer active. Nothing was written.
    AlreadyTerminal,
    /// The message (when there was content) and the Completed receipt are
    /// durable: committed now, or by an earlier call with the same key, in
    /// which case `receipt.already_applied` is set.
    Completed {
        receipt: ManagedTerminalReceipt,
        message_sequence: u64,
    },
}

impl DirectRunCompletion {
    #[must_use]
    pub(crate) fn into_proto(self) -> pb::CompleteDirectRunWithMessageResponse {
        match self {
            Self::AlreadyTerminal => pb::CompleteDirectRunWithMessageResponse {
                already_terminal: true,
                receipt: None,
                message_sequence: 0,
            },
            Self::Completed {
                receipt,
                message_sequence,
            } => pb::CompleteDirectRunWithMessageResponse {
                already_terminal: false,
                receipt: Some(receipt.into_proto()),
                message_sequence,
            },
        }
    }
}

/// A committed completion, plus the memory extraction the caller mirrors into
/// Letta after commit (see `grpc::sync_appended_memory`).
pub(crate) struct CommittedDirectCompletion {
    pub completion: DirectRunCompletion,
    pub memory: Option<crate::grpc::AppendedMemory>,
    /// The posture the mirror of `memory` must apply: the run's own receipt. The
    /// caller of this RPC is the gateway's service credential, which never carries
    /// a waiver, so its own posture cannot stand for the run's.
    pub memory_retention: crate::grpc::MemoryRetention,
}

#[derive(FromRow)]
struct DirectCompletionRow {
    zdr: bool,
    completion_key: Option<String>,
    completion_message_sequence: Option<i64>,
}

/// Save a gateway-direct run's final assistant message and record its
/// Completed terminal receipt in one transaction.
///
/// The two-call path (`AppendMessage`, then `RecordTerminalOutcome`) let a
/// durable cancel commit between the calls and leave a saved answer on a
/// cancelled run. Here the run row is locked before anything is written, so a
/// cancel either commits first (this call then answers `AlreadyTerminal` and
/// writes nothing) or waits and finds the run completed.
///
/// Lock order is thread, run, obligation: the thread first, as `AppendMessage`
/// and `DeleteThread` take it, then run before obligation, as
/// `RecordTerminalOutcome` does.
// One transaction, read top to bottom: every early return below writes
// nothing, which is easiest to check when the checks sit in one place.
#[allow(clippy::result_large_err, clippy::too_many_lines)]
pub(crate) async fn complete_direct_run_with_message_inner(
    pool: &PgPool,
    request: pb::CompleteDirectRunWithMessageRequest,
) -> Result<CommittedDirectCompletion, Status> {
    validate_token(&request.run_id, "run_id", MAX_RUN_ID_BYTES, false)?;
    validate_token(&request.thread_id, "thread_id", MAX_RUN_ID_BYTES, false)?;
    validate_completion_key(&request.idempotency_key)?;
    let source = pb::ManagedRunSource::GatewayDirect;
    let step_id = canonical_terminal_step(source)
        .ok_or_else(|| Status::internal("gateway-direct terminal step is not configured"))?;
    let source_name = source_storage_name(source)
        .ok_or_else(|| Status::internal("gateway-direct terminal source is not configured"))?;

    let mut transaction = pool.begin().await.map_err(database_error)?;
    let thread: Option<String> =
        sqlx::query_scalar("SELECT id FROM threads WHERE id = $1 FOR UPDATE")
            .bind(&request.thread_id)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(database_error)?;
    if thread.is_none() {
        return Err(Status::not_found("thread not found"));
    }
    let run: Option<(String, String, String)> = sqlx::query_as(
        "SELECT status, thread_id, retention_class FROM runs WHERE id = $1 FOR UPDATE",
    )
    .bind(&request.run_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(database_error)?;
    let (run_status, run_thread_id, run_retention_class) =
        run.ok_or_else(|| Status::not_found("run not found"))?;
    if run_thread_id != request.thread_id {
        return Err(Status::permission_denied(
            "the assistant message must go to the run's own thread",
        ));
    }
    let obligation = lock_managed_obligation(&mut transaction, &request.run_id).await?;
    let completion: DirectCompletionRow = sqlx::query_as(
        "SELECT zdr, completion_key, completion_message_sequence
         FROM managed_run_terminalization_outbox
         WHERE run_id = $1",
    )
    .bind(&request.run_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(database_error)?;

    if obligation.state == "applied" {
        // Only the exact operation that completed the run gets its result
        // back. Any other terminal state, and any other operation, finds a run
        // that is no longer active.
        let replay = obligation.applied_source.as_deref() == Some(source_name)
            && obligation.outcome.as_deref() == Some("completed")
            && completion.completion_key.as_deref() == Some(request.idempotency_key.as_str());
        let result = if replay {
            let message_sequence = completion
                .completion_message_sequence
                .and_then(|sequence| u64::try_from(sequence).ok())
                .ok_or_else(|| Status::internal("direct completion receipt is incomplete"))?;
            DirectRunCompletion::Completed {
                receipt: receipt_from_applied_row(&obligation, true)?,
                message_sequence,
            }
        } else {
            DirectRunCompletion::AlreadyTerminal
        };
        transaction.commit().await.map_err(database_error)?;
        return Ok(CommittedDirectCompletion {
            completion: result,
            memory: None,
            memory_retention: crate::grpc::MemoryRetention::Durable,
        });
    }
    if obligation.state == "reconciliation_required" || obligation.state == "superseded" {
        transaction.commit().await.map_err(database_error)?;
        return Ok(CommittedDirectCompletion {
            completion: DirectRunCompletion::AlreadyTerminal,
            memory: None,
            memory_retention: crate::grpc::MemoryRetention::Durable,
        });
    }
    if obligation.state == "processing" {
        return Err(Status::unavailable(
            "managed terminalization recovery is in progress; retry receipt request",
        ));
    }
    if is_terminal_run_status(&run_status) {
        // A cancel (or a legacy terminal step) ended the run without settling
        // the obligation. Retire the obligation, exactly as a terminal receipt
        // or heartbeat would, and save nothing.
        supersede_obligation(&mut transaction, &request.run_id, Utc::now()).await?;
        transaction.commit().await.map_err(database_error)?;
        return Ok(CommittedDirectCompletion {
            completion: DirectRunCompletion::AlreadyTerminal,
            memory: None,
            memory_retention: crate::grpc::MemoryRetention::Durable,
        });
    }
    if source_from_storage(&obligation.configured_source)? != source {
        return Err(Status::permission_denied(
            "managed terminal source is not authorized for this run",
        ));
    }
    // The retention gate of this call. The run's own posture, fixed at start
    // from the user's verified credential, not the service credential's (see
    // `authorize_direct_completion`).
    if completion.zdr {
        return Err(mp_contracts::zdr_refusal::Refusal::DURABLE_MUTATION_FORBIDDEN
            .grpc_status(tonic::Code::FailedPrecondition));
    }
    // The answer's memories are mirrored under the run's receipt. A waived run's
    // memories stay local: they never reach the external memory service (Z-17).
    let memory_retention = match RetentionClass::from_storage(&run_retention_class) {
        Some(RetentionClass::Persistent) => crate::grpc::MemoryRetention::Durable,
        Some(RetentionClass::ZdrWaived) => crate::grpc::MemoryRetention::WaivedRun,
        Some(RetentionClass::Zdr) | None => {
            return Err(Status::internal("run retention receipt is invalid"));
        }
    };

    // An empty answer completes the run without a message, as the two-call
    // path did (the Gateway skipped the append and still recorded Completed).
    let (message_sequence, memory) = if request.content.trim().is_empty() {
        (0, None)
    } else {
        let appended = crate::grpc::append_message_in_tx(
            &mut transaction,
            &pb::AppendMessageRequest {
                thread_id: request.thread_id.clone(),
                role: "assistant".to_owned(),
                content: request.content,
                metadata: request.metadata,
                agent_name: request.agent_name,
                ..Default::default()
            },
            "",
        )
        .await?;
        (appended.sequence, appended.memory)
    };
    let receipt = apply_managed_terminal_outcome(
        &mut transaction,
        &obligation,
        source,
        step_id,
        ManagedOutcome::Completed,
        false,
        Utc::now(),
    )
    .await?;
    let recorded = sqlx::query(
        "UPDATE managed_run_terminalization_outbox
         SET completion_key = $2, completion_message_sequence = $3
         WHERE run_id = $1",
    )
    .bind(&request.run_id)
    .bind(&request.idempotency_key)
    .bind(
        i64::try_from(message_sequence)
            .map_err(|_| Status::internal("message sequence out of range"))?,
    )
    .execute(&mut *transaction)
    .await
    .map_err(database_error)?;
    if recorded.rows_affected() != 1 {
        return Err(Status::internal(
            "direct completion receipt was not persisted",
        ));
    }
    transaction.commit().await.map_err(database_error)?;
    Ok(CommittedDirectCompletion {
        completion: DirectRunCompletion::Completed {
            receipt,
            message_sequence,
        },
        memory,
        memory_retention,
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

/// Whether an approval that has run past its `expires_at` stops sparing its run from
/// recovery (build plan WS03 S8-0). Off by default.
///
/// Off: any `requested` or `granted`, unconsumed approval holds the recovery worker back from
/// its run whatever `expires_at` says, exactly as committed. On: such an approval holds the run
/// back only until it expires (one with no `expires_at` never does), or while a delivery worker
/// holds a live lease on it; an expired, never-consumed one no longer does, so the run ends `outcome_unknown` at its deadline and its
/// fold is purged by the unchanged terminal path. The one answer is bound into both copies of
/// the clause (`claim_due_recovery` and `has_pending_approval`), so they cannot disagree.
pub(crate) fn approval_expiry_enabled() -> bool {
    // A test forces the answer for its own thread, so it does not race the tests that set the
    // process environment. Compiled out of the binary.
    #[cfg(test)]
    if let Some(forced) = expiry_test_switch::forced() {
        return forced;
    }
    mp_contracts::loop_fold::switch_from(std::env::var(APPROVAL_EXPIRY_ENV).ok().as_deref())
}

/// A per-thread override of [`approval_expiry_enabled`] for tests. It holds only for a test that
/// runs its calls on its own thread (a `#[tokio::test]` is single-threaded), and restores the
/// earlier value when the guard drops.
#[cfg(test)]
pub(crate) mod expiry_test_switch {
    use std::cell::Cell;

    thread_local! {
        static FORCED: Cell<Option<bool>> = const { Cell::new(None) };
    }

    pub(crate) fn forced() -> Option<bool> {
        FORCED.with(Cell::get)
    }

    pub(crate) struct Forced(Option<bool>);

    impl Forced {
        pub(crate) fn set(on: bool) -> Self {
            Self(FORCED.with(|cell| cell.replace(Some(on))))
        }
    }

    impl Drop for Forced {
        fn drop(&mut self) {
            FORCED.with(|cell| cell.set(self.0));
        }
    }
}

/// Whether session-core's side of parking is switched on (build plan WS03 S8-4). Off by
/// default; only the literal `true` turns it on (the strict parser the other durable-loop
/// switches share). Read it through [`park_recovery_active`], never by itself.
pub(crate) fn park_recovery_enabled() -> bool {
    // A test forces the answer for its own thread, so it does not race the tests that set the
    // process environment. Compiled out of the binary.
    #[cfg(test)]
    if let Some(forced) = park_test_switch::forced() {
        return forced;
    }
    mp_contracts::loop_fold::switch_from(std::env::var(PARK_RECOVERY_ENV).ok().as_deref())
}

/// Whether the park rules apply: the fold store is on AND park recovery is on. It is the one
/// answer the store's park save and park claim and the recovery worker's spare of a parked fold
/// all read. It is deliberately not coupled to [`approval_expiry_enabled`] (S8-0 is independent).
///
/// With it off, a park is stored and claimed exactly as committed, the store reports
/// `park_accepted` false for every save, and the worker treats a parked fold like any other fold:
/// it terminalizes the run at its ordinary deadline.
pub(crate) fn park_recovery_active() -> bool {
    crate::loop_checkpoint::enabled() && park_recovery_enabled()
}

/// A per-thread override of [`park_recovery_enabled`] for tests. It holds only for a test that
/// runs its calls on its own thread (a `#[tokio::test]` is single-threaded), and restores the
/// earlier value when the guard drops.
#[cfg(test)]
pub(crate) mod park_test_switch {
    use std::cell::Cell;

    thread_local! {
        static FORCED: Cell<Option<bool>> = const { Cell::new(None) };
    }

    pub(crate) fn forced() -> Option<bool> {
        FORCED.with(Cell::get)
    }

    pub(crate) struct Forced(Option<bool>);

    impl Forced {
        pub(crate) fn set(on: bool) -> Self {
            Self(FORCED.with(|cell| cell.replace(Some(on))))
        }
    }

    impl Drop for Forced {
        fn drop(&mut self) {
            FORCED.with(|cell| cell.set(self.0));
        }
    }
}

/// Whether the recovery worker may park a run whose loop died (build plan WS03 S8-B). Off by
/// default; only the literal `true` turns it on. Read it through [`crash_park_active`].
pub(crate) fn crash_park_enabled() -> bool {
    // A test forces the answer for its own thread, so it does not race the tests that set the
    // process environment. Compiled out of the binary.
    #[cfg(test)]
    if let Some(forced) = crash_test_switch::forced() {
        return forced;
    }
    mp_contracts::loop_fold::switch_from(std::env::var(CRASH_PARK_ENV).ok().as_deref())
}

/// Whether crash parking applies: the park rules are active AND the crash switch is on. The one
/// answer the worker's park of a dead loop, the round-fold thread stamp and the heartbeat's push
/// of a crash park all read. With it off none of them runs and the worker is the committed one.
pub(crate) fn crash_park_active() -> bool {
    park_recovery_active() && crash_park_enabled()
}

/// A per-thread override of [`crash_park_enabled`] for tests, with the same scope as
/// [`park_test_switch`].
#[cfg(test)]
pub(crate) mod crash_test_switch {
    use std::cell::Cell;

    thread_local! {
        static FORCED: Cell<Option<bool>> = const { Cell::new(None) };
    }

    pub(crate) fn forced() -> Option<bool> {
        FORCED.with(Cell::get)
    }

    pub(crate) struct Forced(Option<bool>);

    impl Forced {
        pub(crate) fn set(on: bool) -> Self {
            Self(FORCED.with(|cell| cell.replace(Some(on))))
        }
    }

    impl Drop for Forced {
        fn drop(&mut self) {
            FORCED.with(|cell| cell.set(self.0));
        }
    }
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
                      AND a.consumed_at IS NULL
                      AND (NOT $3::bool OR a.expires_at IS NULL OR a.expires_at > now()
                           OR EXISTS (SELECT 1 FROM approval_delivery_outbox AS d
                                      WHERE d.approval_id = a.id AND d.state = 'processing'
                                        AND d.lease_expires_at > now()))
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
    .bind(approval_expiry_enabled())
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
             SELECT 1 FROM approvals AS a
             WHERE a.run_id = $1 AND a.status IN ('requested', 'granted')
               AND a.consumed_at IS NULL
               AND (NOT $2::bool OR a.expires_at IS NULL OR a.expires_at > now()
                    OR EXISTS (SELECT 1 FROM approval_delivery_outbox AS d
                               WHERE d.approval_id = a.id AND d.state = 'processing'
                                 AND d.lease_expires_at > now()))
         )",
    )
    .bind(run_id)
    .bind(approval_expiry_enabled())
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

/// The moment a parked fold's recovery obligation is next due (build plan WS03 S8-4): the earlier
/// of the end of the park's wait (`updated_at`, the park save's age, plus the wait `$2`) and
/// `$3` seconds from now, so a long wait is looked at again every deadline period and a wait that
/// ends sooner is not overshot. A row only when the fold is a park (`parked`, or `resuming` under
/// a continuation's claim) whose wait has NOT run out; `FOR UPDATE` takes the fold's lock after
/// the run's and the obligation's, the order every other writer takes them in.
const PARK_SPARE_HORIZON_SQL: &str =
    "SELECT LEAST(updated_at + make_interval(secs => $2), now() + make_interval(secs => $3))
       FROM run_loop_checkpoints
      WHERE run_id = $1 AND kind = 'park' AND state IN ('parked','resuming')
        AND updated_at + make_interval(secs => $2) > now()
      FOR UPDATE";

/// Hand a spared run's obligation back, open and due at the horizon, its lease cleared.
const PARK_SPARE_SQL: &str = "UPDATE managed_run_terminalization_outbox
        SET state = 'open', deadline_at = $2, next_attempt_at = $2,
            lease_owner = NULL, lease_token_hash = NULL, lease_expires_at = NULL,
            updated_at = now()
      WHERE run_id = $1 AND state = 'processing'";

/// Whether the run's fold is a park, whatever its wait (read after [`spare_parked_run`] spared
/// nothing, to tell a run that is ended because its park ran out from any other recovery).
const PARK_HELD_SQL: &str = "SELECT EXISTS (
        SELECT 1 FROM run_loop_checkpoints
         WHERE run_id = $1 AND kind = 'park' AND state IN ('parked','resuming'))";

/// Spare a run whose fold is a park still inside its wait: its obligation goes back to `open`,
/// due at the horizon, and the caller commits and ends the pass without a terminal write.
/// `Ok(false)` leaves everything as it was.
#[allow(clippy::cast_precision_loss)]
async fn spare_parked_run(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
    ttl_secs: i64,
) -> Result<bool, Status> {
    let horizon: Option<DateTime<Utc>> = sqlx::query_scalar(PARK_SPARE_HORIZON_SQL)
        .bind(run_id)
        .bind(ttl_secs as f64)
        .bind(MANAGED_DEADLINE_SECS as f64)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(database_error)?;
    let Some(horizon) = horizon else {
        return Ok(false);
    };
    sqlx::query(PARK_SPARE_SQL)
        .bind(run_id)
        .bind(horizon)
        .execute(&mut **transaction)
        .await
        .map_err(database_error)?;
    Ok(true)
}

/// Park the active round fold of the run the worker holds, if the store takes it (build plan
/// WS03 S8-B). `Ok(true)`: the run was dealt with and the caller commits without ending it: parked
/// and handed back to its obligation like any park, or left alone because a writer holds its
/// thread (its producer is alive and finishing). `Ok(false)`: nothing was written and the run
/// takes the committed path, which is also what a missing fold key does.
async fn try_crash_park(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
    now: DateTime<Utc>,
) -> Result<bool, Status> {
    let Ok(cipher) = DescriptorCipher::from_env() else {
        tracing::warn!(
            run_id,
            "crash parking is on but the fold key is unavailable; the run is ended as committed"
        );
        return Ok(false);
    };
    match crate::loop_checkpoint::park_crashed(transaction, &cipher, run_id).await? {
        crate::loop_checkpoint::CrashPark::Parked(seq) => {
            spare_parked_run(transaction, run_id, crate::loop_checkpoint::park_ttl_seconds())
                .await?;
            metrics::counter!("mp_session_managed_runs_crash_parked_total").increment(1);
            tracing::warn!(run_id, seq, "managed run parked after producer loss");
            Ok(true)
        }
        crate::loop_checkpoint::CrashPark::Busy => {
            release_recovery_for_activity(transaction, run_id, now).await?;
            metrics::counter!("mp_session_managed_runs_crash_park_busy_total").increment(1);
            Ok(true)
        }
        crate::loop_checkpoint::CrashPark::Refused(reason) => {
            metrics::counter!("mp_session_managed_runs_crash_park_refused_total", "reason" => reason)
                .increment(1);
            Ok(false)
        }
    }
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
    // A run parked for a continuation (WS03 S8-4) is not an abandoned run: it is spared until
    // its wait runs out, counted from the park save, and then ended through the path below, the
    // same one any lost producer takes. Only a `park` fold is spared; an active, paused or
    // closed one takes the path below at once, as committed.
    let mut park_ran_out = false;
    if park_recovery_active() {
        if spare_parked_run(
            &mut transaction,
            &claim.row.run_id,
            crate::loop_checkpoint::park_ttl_seconds(),
        )
        .await?
        {
            transaction.commit().await.map_err(database_error)?;
            metrics::counter!("mp_session_managed_terminalizations_spared_parked_total")
                .increment(1);
            return Ok(false);
        }
        park_ran_out = sqlx::query_scalar(PARK_HELD_SQL)
            .bind(&claim.row.run_id)
            .fetch_one(&mut *transaction)
            .await
            .map_err(database_error)?;
        // A loop that died leaving an active round fold is parked instead of ended, when a
        // continuation could resume it (WS03 S8-B). Anything the store will not park takes the
        // path below, as committed.
        if !park_ran_out
            && crash_park_enabled()
            && try_crash_park(&mut transaction, &claim.row.run_id, now).await?
        {
            transaction.commit().await.map_err(database_error)?;
            return Ok(false);
        }
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
    if park_ran_out {
        metrics::counter!("mp_session_managed_parks_expired_total").increment(1);
    }
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
    use super::{approval_expiry_enabled, expiry_test_switch, APPROVAL_EXPIRY_ENV};
    use super::{
        park_recovery_active, park_recovery_enabled, park_test_switch, PARK_RECOVERY_ENV,
    };
    use super::{
        authorize_direct_completion, authorize_terminalization_source, canonical_terminal_step,
        is_startable_source, validate_terminal_outcome, ManagedOutcome,
    };
    use mp_contracts::model_plane::v1 as pb;

    /// The production text of this file (everything before its own `mod tests`), with LF line
    /// endings whatever the working tree has.
    fn production_source() -> String {
        let all = include_str!("terminalization.rs").replace("\r\n", "\n");
        let marker = concat!("#[cfg(test)]\n", "mod tests {");
        let end = all.find(marker).expect("the unit test module");
        all[..end].to_owned()
    }

    /// One whole top-level `async fn` of `source`: from its signature to the next closing brace
    /// at column zero.
    fn function_text<'a>(source: &'a str, signature: &str) -> Option<&'a str> {
        let start = source.find(signature)?;
        let end = source[start..].find("\n}\n")?;
        Some(&source[start..start + end + 2])
    }

    /// `text` with every run of whitespace collapsed to one space.
    fn squash(text: &str) -> String {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// The clause both copies of the approval spare carry, for the bind position `n`.
    fn expiry_clause(n: u32) -> String {
        format!(
            "AND (NOT ${n}::bool OR a.expires_at IS NULL OR a.expires_at > now() \
             OR EXISTS (SELECT 1 FROM approval_delivery_outbox AS d \
             WHERE d.approval_id = a.id AND d.state = 'processing' \
             AND d.lease_expires_at > now()))"
        )
    }

    /// What is wrong with how `source` carries the S8-0 expiry clause: each of the two copies of
    /// the approval spare (the due-recovery candidate query, bind 3, and the in-transaction
    /// re-check, bind 2) holds the whole clause, and binds the one switch read.
    fn expiry_clause_violations(source: &str) -> Vec<String> {
        let mut violations = Vec::new();
        for (signature, n) in [
            ("async fn claim_due_recovery(", 3_u32),
            ("async fn has_pending_approval(", 2_u32),
        ] {
            let Some(body) = function_text(source, signature) else {
                violations.push(format!("{signature} is not in the source"));
                continue;
            };
            if !squash(body).contains(&expiry_clause(n)) {
                violations.push(format!(
                    "{signature} does not carry the expiry and live-delivery clause bound as ${n}"
                ));
            }
            if body.matches(".bind(approval_expiry_enabled())").count() != 1 {
                violations.push(format!(
                    "{signature} must bind approval_expiry_enabled() exactly once"
                ));
            }
        }
        // The variable is named once, and read once, by the one switch fn.
        if source.matches(APPROVAL_EXPIRY_ENV).count() != 1
            || source.matches("std::env::var(APPROVAL_EXPIRY_ENV)").count() != 1
        {
            violations.push("the switch's variable is not named and read exactly once".to_owned());
        }
        violations
    }

    #[test]
    fn the_expiry_clause_is_bound_in_both_copies() {
        let source = production_source();
        assert_eq!(
            expiry_clause_violations(&source),
            Vec::<String>::new(),
            "both copies of the approval spare must carry the same bound clause"
        );

        // The control: each mutation takes the clause out of ONE copy and must be seen.
        let claim = function_text(&source, "async fn claim_due_recovery(").expect("claim copy");
        let recheck =
            function_text(&source, "async fn has_pending_approval(").expect("re-check copy");
        for (what, mutated) in [
            (
                "the clause is dropped from the candidate query",
                source.replacen(claim, &claim.replacen("NOT $3::bool", "NOT FALSE", 1), 1),
            ),
            (
                "the clause is dropped from the re-check",
                source.replacen(
                    recheck,
                    &recheck.replacen("NOT $2::bool", "NOT FALSE", 1),
                    1,
                ),
            ),
            (
                "the live-delivery lease is dropped from the re-check",
                source.replacen(
                    recheck,
                    &recheck.replacen("d.lease_expires_at > now()", "FALSE", 1),
                    1,
                ),
            ),
            (
                "the candidate query stops binding the switch",
                source.replacen(
                    claim,
                    &claim.replacen(".bind(approval_expiry_enabled())", "", 1),
                    1,
                ),
            ),
            (
                "the re-check stops binding the switch",
                source.replacen(
                    recheck,
                    &recheck.replacen(".bind(approval_expiry_enabled())", "", 1),
                    1,
                ),
            ),
        ] {
            assert_ne!(mutated, source, "{what}: the mutation changed nothing");
            assert!(
                !expiry_clause_violations(&mutated).is_empty(),
                "{what}: the guard did not flag it"
            );
        }
    }

    /// The switch is off unless the process says `true`, and a test's forced answer is
    /// restored when its guard drops, so one test cannot leak its choice into another.
    #[test]
    fn the_expiry_switch_is_off_by_default_and_a_forced_answer_is_restored() {
        if std::env::var_os(APPROVAL_EXPIRY_ENV).is_none() {
            assert!(!approval_expiry_enabled(), "off by default");
        }
        let before = approval_expiry_enabled();
        {
            let _on = expiry_test_switch::Forced::set(true);
            assert!(approval_expiry_enabled());
            {
                let _off = expiry_test_switch::Forced::set(false);
                assert!(!approval_expiry_enabled());
            }
            assert!(
                approval_expiry_enabled(),
                "the inner guard restored the outer one"
            );
        }
        assert_eq!(
            approval_expiry_enabled(),
            before,
            "the outer guard restored it"
        );
    }

    /// Park recovery is off unless the process says `true`, it is ACTIVE only with the fold store
    /// on as well, and a test's forced answer is restored when its guard drops (WS03 S8-4).
    #[test]
    fn park_recovery_is_off_by_default_and_active_only_with_the_store_on() {
        use crate::loop_checkpoint::test_switch as store_switch;
        if std::env::var_os(PARK_RECOVERY_ENV).is_none() {
            assert!(!park_recovery_enabled(), "off by default");
        }
        let before = park_recovery_enabled();
        let _store_off = store_switch::Forced::set(false);
        {
            let _on = park_test_switch::Forced::set(true);
            assert!(park_recovery_enabled());
            assert!(!park_recovery_active(), "the fold store is off");
        }
        let _store_on = store_switch::Forced::set(true);
        {
            let _on = park_test_switch::Forced::set(true);
            assert!(park_recovery_active());
            {
                let _off = park_test_switch::Forced::set(false);
                assert!(!park_recovery_enabled());
                assert!(!park_recovery_active(), "park recovery is off");
            }
            assert!(park_recovery_active(), "the inner guard restored the outer one");
        }
        assert_eq!(park_recovery_enabled(), before, "the outer guard restored it");
        // Not coupled to the approval-expiry switch (S8-0): that one changes nothing here.
        let _park = park_test_switch::Forced::set(true);
        for expiry in [false, true] {
            let _expiry = expiry_test_switch::Forced::set(expiry);
            assert!(park_recovery_active(), "expiry forced {expiry}");
        }
    }

    /// The text of the `const NAME: &str = "..";` item of `source` that starts at `head`, from
    /// the head to its closing `";`; empty when it is not there.
    fn const_text(source: &str, head: &str) -> String {
        source.find(head).map_or_else(String::new, |start| {
            let rest = &source[start..];
            rest[..rest.find("\";").map_or(rest.len(), |end| end + 2)].to_owned()
        })
    }

    /// What is wrong with how `source` carries the S8-4 park spare: the switch is read in one
    /// place, the spare sits after the approval spare and before the terminal write, it is gated
    /// on the active switch, and its two statements keep their predicates.
    fn park_spare_violations(source: &str) -> Vec<String> {
        let mut violations = Vec::new();
        let flat = squash(source);
        if source.matches("PARK_RECOVERY_ENV").count() != 2
            || source.matches("std::env::var(PARK_RECOVERY_ENV)").count() != 1
        {
            violations.push("the park switch's variable is not named and read exactly once".to_owned());
        }
        if !flat.contains(
            "pub(crate) fn park_recovery_active() -> bool { crate::loop_checkpoint::enabled() && park_recovery_enabled() }",
        ) {
            violations.push("park_recovery_active is not the store switch AND park recovery".to_owned());
        }
        let Some(body) = function_text(source, "async fn recover_claimed_terminalization(") else {
            violations.push("the recovery worker is not in the source".to_owned());
            return violations;
        };
        let place = |needle: &str| body.find(needle);
        match (
            place("has_pending_approval("),
            place("if park_recovery_active() {"),
            place("spare_parked_run("),
            place("apply_managed_terminal_outcome("),
        ) {
            (Some(approval), Some(gate), Some(spare), Some(terminal))
                if approval < gate && gate < spare && spare < terminal => {}
            _ => violations.push(
                "the park spare must follow the approval spare, sit behind `if park_recovery_active() {` and come before apply_managed_terminal_outcome".to_owned(),
            ),
        }
        for (what, needle) in [
            ("the spare is called once", "spare_parked_run("),
            ("the gate is read once", "park_recovery_active()"),
        ] {
            if body.matches(needle).count() != 1 {
                violations.push(format!("{what}: {needle} must appear exactly once in the worker"));
            }
        }
        let horizon = squash(&const_text(source, "const PARK_SPARE_HORIZON_SQL"));
        for needed in [
            "SELECT LEAST(updated_at + make_interval(secs => $2), now() + make_interval(secs => $3))",
            "WHERE run_id = $1 AND kind = 'park' AND state IN ('parked','resuming')",
            "AND updated_at + make_interval(secs => $2) > now()",
            "FOR UPDATE",
        ] {
            if !horizon.contains(needed) {
                violations.push(format!("the horizon statement lost `{needed}`"));
            }
        }
        let spare = squash(&const_text(source, "const PARK_SPARE_SQL"));
        for needed in [
            "SET state = 'open', deadline_at = $2, next_attempt_at = $2,",
            "lease_owner = NULL, lease_token_hash = NULL, lease_expires_at = NULL",
            "WHERE run_id = $1 AND state = 'processing'",
        ] {
            if !spare.contains(needed) {
                violations.push(format!("the spare statement lost `{needed}`"));
            }
        }
        violations
    }

    #[test]
    fn the_park_spare_follows_the_approval_spare_and_precedes_the_terminal_write() {
        let source = production_source();
        assert_eq!(
            park_spare_violations(&source),
            Vec::<String>::new(),
            "the park spare is wired as designed"
        );
        let worker = function_text(&source, "async fn recover_claimed_terminalization(")
            .expect("the worker");
        let reorder = |worker: &str| {
            let spare_at = worker.find("    // A run parked for a continuation").expect("the park block");
            let end = worker.find("    let source = source_from_storage(").expect("the terminal path");
            let block = &worker[spare_at..end];
            let mut moved = worker.replacen(block, "", 1);
            let tail_at = moved
                .find("    transaction.commit().await.map_err(database_error)?;\n    metrics::counter!(\"mp_session_managed_terminalizations_recovered_total\")")
                .expect("the commit after the write");
            moved.insert_str(tail_at, block);
            moved
        };
        for (what, mutated) in [
            (
                "the spare moved after the terminal write",
                source.replacen(worker, &reorder(worker), 1),
            ),
            (
                "the gate dropped",
                source.replacen(
                    worker,
                    &worker.replacen("if park_recovery_active() {", "if true {", 1),
                    1,
                ),
            ),
            (
                "the horizon statement spares a paused fold too",
                source.replacen("AND kind = 'park' AND state IN ('parked','resuming')\n        AND updated_at", "AND state IN ('parked','resuming')\n        AND updated_at", 1),
            ),
            (
                "the horizon statement spares a park whose wait ran out",
                source.replacen("AND updated_at + make_interval(secs => $2) > now()\n      FOR UPDATE", "FOR UPDATE", 1),
            ),
            (
                "the switch is read twice",
                source.replacen(
                    "mp_contracts::loop_fold::switch_from(std::env::var(PARK_RECOVERY_ENV).ok().as_deref())",
                    "mp_contracts::loop_fold::switch_from(std::env::var(PARK_RECOVERY_ENV).ok().as_deref()) || std::env::var(PARK_RECOVERY_ENV).is_ok()",
                    1,
                ),
            ),
        ] {
            assert_ne!(mutated, source, "{what}: the mutation changed nothing");
            assert!(
                !park_spare_violations(&mutated).is_empty(),
                "{what}: the guard did not flag it"
            );
        }
    }

    #[test]
    fn scheduled_result_persist_is_off_by_default_and_only_literal_true_enables() {
        use super::{scheduled_result_persist_enabled, SCHEDULED_RESULT_PERSIST_ENV};
        if std::env::var_os(SCHEDULED_RESULT_PERSIST_ENV).is_none() {
            assert!(!scheduled_result_persist_enabled(), "off by default");
        }
        assert!(mp_contracts::loop_fold::switch_from(Some("true")));
        assert!(!mp_contracts::loop_fold::switch_from(Some("1")));
        assert!(!mp_contracts::loop_fold::switch_from(Some("yes")));
    }

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

    /// Gateway keeps the classified browser failure instead of collapsing it to
    /// `browser_failed`, so each specific code must clear the allowlist too.
    #[test]
    fn the_classified_browser_failure_codes_are_allowlisted() {
        for code in [
            "browser_failed",
            "browser_permission_denied",
            "browser_resource_exhausted",
            "browser_timed_out",
        ] {
            assert_eq!(
                validate_terminal_outcome(pb::TerminalOutcome::Failed, code)
                    .unwrap_or_else(|error| panic!("{code} must be allowlisted: {error}")),
                ManagedOutcome::Failed(code)
            );
        }
        assert!(
            validate_terminal_outcome(
                pb::TerminalOutcome::Failed,
                "browser_agent_permission_denied"
            )
            .is_err(),
            "the gateway's internal classification is not a durable code"
        );
    }

    /// The allowlist and the CHECK move together: every allowlisted code, plus
    /// the reconciliation-only `outcome_unknown`, is in migration 0069, and its
    /// revert restores exactly the 0033 list.
    #[test]
    fn migration_0069_check_matches_the_allowlist_and_its_revert_restores_0033() {
        let up = include_str!("../migrations/0069_browser_failure_codes.sql");
        let down = include_str!("../migrations/revert/0069_browser_failure_codes.down.sql");
        // Only the CHECK clause counts: the revert's comment names the new codes.
        let check = |sql: &'static str| {
            sql.split_once("CHECK (")
                .expect("the migration re-adds the CHECK")
                .1
        };
        let (up_check, down_check) = (check(up), check(down));
        let quoted = |code: &str| format!("'{code}'");
        for code in super::ALLOWED_FAILURE_CODES
            .iter()
            .copied()
            .chain(["outcome_unknown"])
        {
            assert!(up_check.contains(&quoted(code)), "0069 must accept {code}");
        }
        for code in [
            "browser_failed",
            "dispatch_rejected",
            "dispatch_unreachable",
            "execution_failed",
            "inference_failed",
            "outcome_unknown",
            "provider_timeout",
            "provider_unavailable",
        ] {
            assert!(
                down_check.contains(&quoted(code)),
                "the revert must keep {code}"
            );
        }
        for code in [
            "browser_permission_denied",
            "browser_resource_exhausted",
            "browser_timed_out",
        ] {
            assert!(
                !down_check.contains(&quoted(code)),
                "the revert must drop {code}"
            );
        }
        for sql in [up, down] {
            assert!(sql.contains(
                "DROP CONSTRAINT IF EXISTS managed_run_terminalization_outbox_failure_code_check"
            ));
            assert!(sql
                .contains("ADD CONSTRAINT managed_run_terminalization_outbox_failure_code_check"));
        }
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

    /// Regression, 2026-09-24: the atomic save refused the terminalize token
    /// Auth Core really issues Model Gateway (FAILED_PRECONDITION, before any
    /// SQL), because that token is `zdr: true` by registry policy for every
    /// turn. Built through the production interceptor, not by hand, so the
    /// identity carries the claims the issuer actually signs.
    #[test]
    fn the_atomic_save_accepts_the_gateway_credential_auth_core_really_issues() {
        use crate::auth::test_support::{
            authenticate, model_gateway_terminalize_claims, service_principal_claims, sign,
            user_claims, verifier, KID,
        };
        let verifier = verifier("session-core");
        let caller = |claims: &serde_json::Value| {
            let request = authenticate(&verifier, &sign(claims, KID), ())
                .expect("an Auth Core-shaped credential verifies");
            crate::auth::identity(&request).expect("the interceptor installs an identity")
        };

        let gateway = caller(&model_gateway_terminalize_claims("org-1"));
        assert!(
            gateway.zdr(),
            "Auth Core marks every Model Gateway token ZDR"
        );
        authorize_direct_completion(&gateway)
            .expect("the gateway's real terminalize credential may complete its direct run");
        authorize_terminalization_source(&gateway, pb::ManagedRunSource::GatewayDirect)
            .expect("the same credential RecordTerminalOutcome accepts");

        for (claims, why) in [
            (
                service_principal_claims(
                    "session-core",
                    "org-1",
                    "model-gateway",
                    &["session:heartbeat"],
                    "model-gateway managed-run heartbeat",
                    true,
                ),
                "a heartbeat token carries no terminal authority",
            ),
            (
                service_principal_claims(
                    "session-core",
                    "org-1",
                    "execution-core",
                    &["session:terminalize"],
                    "execution-core managed-run terminalization",
                    false,
                ),
                "another workload's terminalizer, even a durable one",
            ),
            (
                user_claims("session-core", "org-1", "user-1", false),
                "a user, even the run's owner, is not the terminalizer",
            ),
        ] {
            assert_eq!(
                authorize_direct_completion(&caller(&claims))
                    .expect_err(why)
                    .code(),
                tonic::Code::PermissionDenied,
                "{why}"
            );
        }
    }
}

/// Build plan WS03 S7c-A2: the recovery race a spent click would otherwise lose.
/// WRITTEN, NOT RUN by the author: it needs the disposable-Postgres harness
/// (scripts/tests/session-core-db-tests.sh), which the lead runs. Its path starts
/// with `terminalization::spent_click_db_tests::`, so the harness filter
/// `terminalization` selects it.
///
/// While a granted loop approval is unconsumed, the recovery worker will not
/// terminalize its run (it holds the run back). Consuming the approval lifts that
/// hold, so a click spent after a long human pause (an hour here) would leave the
/// run recoverable at once, ahead of the executor about to run it. A spend-only
/// consume renews the obligation in its own transaction and so closes the gap; the
/// executor's own full consume deliberately does not, which is the control.
#[cfg(test)]
mod spent_click_db_tests {
    use super::*;
    use crate::orchestration_store as approvals;
    use base64::Engine as _;

    const FINGERPRINT: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

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
        // The retained descriptor is encrypted: the process needs its key.
        std::env::set_var(
            "SESSION_CORE_CONTINUATION_DESCRIPTOR_KEY",
            base64::engine::general_purpose::STANDARD.encode([7_u8; 32]),
        );
        Some(pool)
    }

    struct Paused {
        org: String,
        user: String,
        run_id: String,
        approval_id: String,
    }

    /// A persistent managed run (started through the real start path, so its
    /// obligation is the real one) paused on a GRANTED, unconsumed loop approval
    /// whose obligation fell due an hour ago: the shape of a click made after a
    /// long human pause.
    async fn paused_for_an_hour(pool: &PgPool, tag: &str) -> Paused {
        let sfx = new_ulid();
        let (org, user, thread) = (
            format!("sc-org-{tag}-{sfx}"),
            "u1".to_owned(),
            format!("sc-t-{tag}-{sfx}"),
        );
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id, created_at)
             VALUES ($1, $1, $2, $3, now())",
        )
        .bind(&thread)
        .bind(&org)
        .bind(&user)
        .execute(pool)
        .await
        .expect("thread seeded");
        let run_id = start_managed_run_with_retention_inner(
            pool,
            pb::StartManagedRunRequest {
                thread_id: thread,
                parent_run_id: String::new(),
                agent_id: "general-v1".to_owned(),
                goal: "spent click goal".to_owned(),
                mode: "execute".to_owned(),
                org_id: org.clone(),
                user_id: user.clone(),
                start_key: format!("sc-{tag}-{sfx}"),
                terminal_source: pb::ManagedRunSource::ExecutionAgent as i32,
            },
            RunRetention::persistent(),
        )
        .await
        .expect("the run starts")
        .run_id;

        let approval_id = format!("appr_{}", new_ulid());
        let descriptor = serde_json::json!({
            "version": 1,
            "run_id": run_id,
            "org_id": org,
            "user_id": user,
            "step_id": "tool_1_call-1",
            "action_kind": "tool_call",
            "action_fingerprint": FINGERPRINT,
            "tool_name": "shell",
            "input": {"command": "echo approved"},
            "permission_mode": "ask",
            "offered": {
                "tools": [{"name": "shell", "description": "run", "parameters_json": "{}"}],
                "authority": {
                    "mode": "ask", "plan_mode": false, "autonomy_rung": "", "space_id": "",
                    "min_privacy_tier": 0, "provider_hint": "", "max_rounds": 4,
                    "tool_names": ["shell"], "digest": "unchecked-in-this-test"
                }
            }
        });
        approvals::request_approval(
            pool,
            &approval_id,
            &run_id,
            None,
            "tool_call",
            &user,
            &org,
            &user,
            "",
            &serde_json::json!({}),
            Some(&descriptor),
            Some(Utc::now() + chrono::Duration::hours(2)),
            "loop",
        )
        .await
        .expect("request approval")
        .expect("inserted");
        approvals::decide_approval(pool, &approval_id, &org, Some(&user), "granted", &user, "ok")
            .await
            .expect("grant");
        // The human took an hour: the obligation fell due while the approval waited.
        sqlx::query(
            "UPDATE managed_run_terminalization_outbox
             SET deadline_at = now() - interval '1 hour', next_attempt_at = now() - interval '1 hour'
             WHERE run_id = $1",
        )
        .bind(&run_id)
        .execute(pool)
        .await
        .expect("age the obligation");
        Paused {
            org,
            user,
            run_id,
            approval_id,
        }
    }

    /// `claim_due_recovery` looks at every run in the database. Push every other
    /// live obligation (and every other expired lease) a day out, so the answer
    /// is about the run under test only. Scratch database only.
    async fn isolate(pool: &PgPool, keep_run_id: &str) {
        sqlx::query(
            "UPDATE managed_run_terminalization_outbox
             SET deadline_at = now() + interval '1 day',
                 lease_expires_at = CASE WHEN lease_expires_at IS NULL THEN NULL
                                         ELSE now() + interval '1 day' END
             WHERE run_id <> $1 AND state IN ('open', 'processing')",
        )
        .bind(keep_run_id)
        .execute(pool)
        .await
        .expect("isolate the other obligations");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\""]
    async fn a_spent_click_is_not_recovered_before_the_executor_heartbeats() {
        let Some(pool) = scratch_pool().await else {
            return;
        };

        // The click: a spend-only consume (the gateway's) of an hour-old pause.
        let spent = paused_for_an_hour(&pool, "spend").await;
        isolate(&pool, &spent.run_id).await;
        assert!(
            claim_due_recovery(&pool).await.expect("claim").is_none(),
            "premise: the unconsumed granted approval holds the run back"
        );
        let won = approvals::consume_approval_spend_only(
            &pool,
            &spent.approval_id,
            &spent.org,
            &spent.run_id,
            &spent.user,
            "",
            "claim-a",
        )
        .await
        .expect("consume")
        .expect("outcome");
        assert!(won.consumed);
        assert!(
            claim_due_recovery(&pool).await.expect("claim").is_none(),
            "the renewal moved the deadline: nothing is due for the spent run"
        );

        // The control: the same seed consumed with the executor's full consume is
        // recoverable at once (it deliberately renews nothing), which proves the
        // assertion above is the renewal's doing and not an artefact of the seed.
        let control = paused_for_an_hour(&pool, "full").await;
        isolate(&pool, &control.run_id).await;
        assert!(approvals::consume_approval(
            &pool,
            &control.approval_id,
            &control.org,
            &control.run_id,
            &control.user,
            "",
            "claim-a",
        )
        .await
        .expect("consume")
        .expect("outcome")
        .consumed);
        let claimed = claim_due_recovery(&pool)
            .await
            .expect("claim")
            .expect("a full consume leaves the run recoverable");
        assert_eq!(claimed.row.run_id, control.run_id);
    }
}

/// Build plan WS03 S8-0: an approval that has run past its `expires_at` no longer holds the
/// recovery worker back from its run, behind SESSION_CORE_RECOVERY_APPROVAL_EXPIRY_ENABLED.
/// WRITTEN, NOT RUN by the author: it needs the disposable-Postgres harness
/// (scripts/tests/session-core-db-tests.sh), which the lead runs. Its path starts with
/// `terminalization::park_recovery_db_tests::`, so the harness filter `terminalization`
/// selects it.
///
/// Each test forces the switch for its own thread (`expiry_test_switch`), so none of them
/// depends on, or sets, the process environment. The database is shared, so each scenario
/// first pushes every other live obligation a day out (`isolate`): the answer is then about
/// the run under test alone. Scratch database only.
#[cfg(test)]
mod park_recovery_db_tests {
    use super::*;
    use crate::loop_checkpoint::test_support::{fold_rows, seed_fold};
    use crate::orchestration_store as approvals;

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

    /// Push every other live obligation (and every other expired lease) a day out.
    async fn isolate(pool: &PgPool, keep_run_id: &str) {
        sqlx::query(
            "UPDATE managed_run_terminalization_outbox
             SET deadline_at = now() + interval '1 day',
                 next_attempt_at = now() + interval '1 day',
                 lease_expires_at = CASE WHEN lease_expires_at IS NULL THEN NULL
                                         ELSE now() + interval '1 day' END
             WHERE run_id <> $1 AND state IN ('open', 'processing')",
        )
        .bind(keep_run_id)
        .execute(pool)
        .await
        .expect("isolate the other obligations");
    }

    struct Seeded {
        run_id: String,
    }

    /// A persistent managed run (started through the real start path, so its obligation is the
    /// real one) that holds a paused fold and ONE unconsumed `requested` approval whose
    /// `expires_at` is `expires_in_secs` from now (negative: already past; `None`: no expiry),
    /// with its obligation overdue by an hour.
    async fn paused_with_approval(
        pool: &PgPool,
        tag: &str,
        expires_in_secs: Option<i64>,
    ) -> Seeded {
        let sfx = new_ulid();
        let (org, user, thread) = (
            format!("pr-org-{tag}-{sfx}"),
            "u1".to_owned(),
            format!("pr-t-{tag}-{sfx}"),
        );
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id, created_at)
             VALUES ($1, $1, $2, $3, now())",
        )
        .bind(&thread)
        .bind(&org)
        .bind(&user)
        .execute(pool)
        .await
        .expect("thread seeded");
        let run_id = start_managed_run_with_retention_inner(
            pool,
            pb::StartManagedRunRequest {
                thread_id: thread,
                parent_run_id: String::new(),
                agent_id: "general-v1".to_owned(),
                goal: "approval expiry goal".to_owned(),
                mode: "execute".to_owned(),
                org_id: org.clone(),
                user_id: user.clone(),
                start_key: format!("pr-{tag}-{sfx}"),
                terminal_source: pb::ManagedRunSource::ExecutionAgent as i32,
            },
            RunRetention::persistent(),
        )
        .await
        .expect("the run starts")
        .run_id;

        let approval_id = format!("appr_{}", new_ulid());
        // A cold approval (no continuation mode, no descriptor): this test is about the
        // recovery worker's reading of `expires_at`, not about how the approval is resumed.
        approvals::request_approval(
            pool,
            &approval_id,
            &run_id,
            None,
            "tool_call",
            &user,
            &org,
            &user,
            "",
            &serde_json::json!({}),
            None,
            expires_in_secs.map(|secs| Utc::now() + chrono::Duration::seconds(secs)),
            "",
        )
        .await
        .expect("request approval")
        .expect("inserted");
        seed_fold(pool, &run_id, "pause", &approval_id, "", None)
            .await
            .expect("the paused fold is stored");
        make_due(pool, &run_id).await;
        Seeded { run_id }
    }

    /// The obligation fell due an hour ago.
    async fn make_due(pool: &PgPool, run_id: &str) {
        sqlx::query(
            "UPDATE managed_run_terminalization_outbox
             SET deadline_at = now() - interval '1 hour', next_attempt_at = now() - interval '1 hour'
             WHERE run_id = $1",
        )
        .bind(run_id)
        .execute(pool)
        .await
        .expect("age the obligation");
    }

    /// A delivery worker holds (or held) a lease on the run's approval: `lease_secs` from now,
    /// negative for a lease that has already lapsed.
    async fn delivery_lease(pool: &PgPool, run_id: &str, lease_secs: i64) {
        sqlx::query(
            "INSERT INTO approval_delivery_outbox
                 (delivery_id, approval_id, run_id, org_id, user_id, state, attempts,
                  lease_owner, lease_token_hash, lease_expires_at, processing_at)
             SELECT $2, a.id, a.run_id, a.org_id, a.user_id, 'processing', 1,
                    'approval-delivery-worker', repeat('0', 64),
                    now() + make_interval(secs => $3), now()
               FROM approvals AS a WHERE a.run_id = $1",
        )
        .bind(run_id)
        .bind(format!("approval_delivery_{}", new_ulid()))
        .bind(lease_secs as f64)
        .execute(pool)
        .await
        .expect("delivery lease seeded");
    }

    async fn run_status(pool: &PgPool, run_id: &str) -> String {
        sqlx::query_scalar("SELECT status FROM runs WHERE id = $1")
            .bind(run_id)
            .fetch_one(pool)
            .await
            .expect("run status")
    }

    /// `(state, failure_code)` of the run's terminalization obligation.
    async fn obligation(pool: &PgPool, run_id: &str) -> (String, Option<String>) {
        sqlx::query_as(
            "SELECT state, failure_code FROM managed_run_terminalization_outbox WHERE run_id = $1",
        )
        .bind(run_id)
        .fetch_one(pool)
        .await
        .expect("obligation")
    }

    /// How many RUN_TERMINAL receipts the run holds in the event log (the run's own terminal
    /// event, whichever outcome it carries): the assertion that a run has at most one.
    async fn terminal_receipts(pool: &PgPool, run_id: &str) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM events WHERE run_id = $1 AND type_url = $2")
            .bind(run_id)
            .bind(RUN_TERMINAL_TYPE_URL)
            .fetch_one(pool)
            .await
            .expect("count the terminal receipts")
    }

    /// The `receipt_id` the obligation points at (`None` until a terminal receipt is written).
    async fn obligation_receipt(pool: &PgPool, run_id: &str) -> Option<String> {
        sqlx::query_scalar(
            "SELECT receipt_id FROM managed_run_terminalization_outbox WHERE run_id = $1",
        )
        .bind(run_id)
        .fetch_one(pool)
        .await
        .expect("obligation receipt")
    }

    /// Grant the run's one (cold) approval, as production does while it is still unexpired
    /// (which queues its delivery row), then let it run past its `expires_at`.
    async fn grant_then_expire(pool: &PgPool, run_id: &str) {
        let (approval_id, org, user): (String, String, String) =
            sqlx::query_as("SELECT id, org_id, user_id FROM approvals WHERE run_id = $1")
                .bind(run_id)
                .fetch_one(pool)
                .await
                .expect("the run's approval");
        approvals::decide_approval(pool, &approval_id, &org, Some(&user), "granted", &user, "ok")
            .await
            .expect("grant");
        sqlx::query("UPDATE approvals SET expires_at = now() - interval '1 hour' WHERE id = $1")
            .bind(&approval_id)
            .execute(pool)
            .await
            .expect("let the granted approval expire");
    }

    /// The delivery row a grant queued is claimed by a delivery worker: its lease runs
    /// `lease_secs` from now (negative: it has already lapsed).
    async fn set_delivery_lease(pool: &PgPool, run_id: &str, lease_secs: i64) {
        let updated = sqlx::query(
            "UPDATE approval_delivery_outbox
             SET state = 'processing', attempts = 1,
                 lease_owner = 'approval-delivery-worker', lease_token_hash = repeat('0', 64),
                 lease_expires_at = now() + make_interval(secs => $2), processing_at = now()
             WHERE run_id = $1",
        )
        .bind(run_id)
        .bind(lease_secs as f64)
        .execute(pool)
        .await
        .expect("delivery lease set");
        assert_eq!(updated.rows_affected(), 1, "premise: one queued delivery row");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\""]
    async fn an_expired_never_consumed_approval_no_longer_spares_the_run() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let seeded = paused_with_approval(&pool, "expired", Some(-3600)).await;
        isolate(&pool, &seeded.run_id).await;
        assert_eq!(fold_rows(&pool, &seeded.run_id).await, 1, "premise: a fold");

        // Switch off (the default): the committed sparing. The expired approval holds the
        // run back from the candidate query, and the worker's whole pass leaves it alone.
        {
            let _off = expiry_test_switch::Forced::set(false);
            assert!(
                claim_due_recovery(&pool).await.expect("claim").is_none(),
                "off: an expired approval still spares the run, as committed"
            );
            assert_eq!(recover_due_terminalizations(&pool).await.expect("pass"), 0);
            assert_eq!(fold_rows(&pool, &seeded.run_id).await, 1);
            assert!(!is_terminal_run_status(
                &run_status(&pool, &seeded.run_id).await
            ));
            assert_eq!(
                terminal_receipts(&pool, &seeded.run_id).await,
                0,
                "off: no terminal receipt is written"
            );
        }

        // Switch on, but the re-check (the second copy of the clause) read with the switch
        // off: the run is claimed (the candidate query lets it through), then spared by the
        // re-check and handed back to its obligation. Each copy answers on its own.
        {
            let _on = expiry_test_switch::Forced::set(true);
            let claim = claim_due_recovery(&pool)
                .await
                .expect("claim")
                .expect("on: the expired approval no longer holds the run back");
            assert_eq!(claim.row.run_id, seeded.run_id);
            let _off = expiry_test_switch::Forced::set(false);
            assert!(
                !recover_claimed_terminalization(&pool, claim)
                    .await
                    .expect("recheck"),
                "the re-check with the switch off still spares the run"
            );
            assert_eq!(obligation(&pool, &seeded.run_id).await.0, "open");
            assert_eq!(fold_rows(&pool, &seeded.run_id).await, 1);
            assert_eq!(terminal_receipts(&pool, &seeded.run_id).await, 0);
        }

        // Switch on: the run ends outcome_unknown at its deadline and its fold is purged by
        // the unchanged terminal path.
        make_due(&pool, &seeded.run_id).await;
        isolate(&pool, &seeded.run_id).await;
        {
            let _on = expiry_test_switch::Forced::set(true);
            assert_eq!(
                recover_due_terminalizations(&pool).await.expect("pass"),
                1,
                "on: exactly this run is recovered"
            );
        }
        assert_eq!(run_status(&pool, &seeded.run_id).await, "failed");
        let (state, failure) = obligation(&pool, &seeded.run_id).await;
        assert_eq!(state, "reconciliation_required");
        assert_eq!(failure.as_deref(), Some(RECOVERY_FAILURE_CODE));
        assert_eq!(
            fold_rows(&pool, &seeded.run_id).await,
            0,
            "the fold is purged with the run"
        );
        assert!(
            obligation_receipt(&pool, &seeded.run_id).await.is_some(),
            "the obligation records the receipt it wrote"
        );
        assert_eq!(
            terminal_receipts(&pool, &seeded.run_id).await,
            1,
            "exactly one RUN_TERMINAL receipt"
        );
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\""]
    async fn an_unexpired_or_live_delivery_approval_still_spares() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let _on = expiry_test_switch::Forced::set(true);

        // An approval that has not expired yet, an approval with no expiry at all, and an
        // expired one a delivery worker holds a live lease on: each still holds its run back,
        // both at the candidate query and across the worker's whole pass.
        for (tag, expires_in, lease) in [
            ("future", Some(3600), None),
            ("noexpiry", None, None),
            ("leased", Some(-3600), Some(600)),
        ] {
            let seeded = paused_with_approval(&pool, tag, expires_in).await;
            if let Some(lease_secs) = lease {
                delivery_lease(&pool, &seeded.run_id, lease_secs).await;
            }
            isolate(&pool, &seeded.run_id).await;
            assert!(
                claim_due_recovery(&pool).await.expect("claim").is_none(),
                "{tag}: the approval still spares the run"
            );
            assert_eq!(recover_due_terminalizations(&pool).await.expect("pass"), 0);
            assert!(!is_terminal_run_status(
                &run_status(&pool, &seeded.run_id).await
            ));
            assert_eq!(
                fold_rows(&pool, &seeded.run_id).await,
                1,
                "{tag}: fold kept"
            );
        }

        // The control: the same expired approval whose delivery lease has LAPSED no longer
        // spares its run, so the three assertions above are the clause's doing.
        let lapsed = paused_with_approval(&pool, "lapsed", Some(-3600)).await;
        delivery_lease(&pool, &lapsed.run_id, -600).await;
        isolate(&pool, &lapsed.run_id).await;
        let claimed = claim_due_recovery(&pool)
            .await
            .expect("claim")
            .expect("an expired approval with a lapsed delivery lease is recoverable");
        assert_eq!(claimed.row.run_id, lapsed.run_id);
    }

    /// The live-delivery-lease branch of the SECOND copy of the clause (`has_pending_approval`,
    /// the re-check inside `recover_claimed_terminalization`). Every sparing case in the test
    /// above is stopped by `claim_due_recovery`'s candidate query first, so none reaches it.
    /// Here the worker WINS the claim while the delivery lease has lapsed, and a delivery
    /// worker then takes a live lease before the re-check reads: only the re-check can spare
    /// the run now.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\""]
    async fn a_delivery_lease_taken_after_the_claim_spares_the_run_at_the_recheck() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let _on = expiry_test_switch::Forced::set(true);
        // A granted approval (so its delivery row exists, as in production) that has since
        // expired, whose delivery lease has lapsed: the candidate query lets the run through.
        let seeded = paused_with_approval(&pool, "racelease", Some(3600)).await;
        grant_then_expire(&pool, &seeded.run_id).await;
        set_delivery_lease(&pool, &seeded.run_id, -600).await;
        isolate(&pool, &seeded.run_id).await;
        let claim = claim_due_recovery(&pool)
            .await
            .expect("claim")
            .expect("an expired approval with a lapsed delivery lease does not hold the run back");
        assert_eq!(claim.row.run_id, seeded.run_id);

        // The race: a delivery worker takes a live lease after the claim.
        set_delivery_lease(&pool, &seeded.run_id, 600).await;
        assert!(
            !recover_claimed_terminalization(&pool, claim)
                .await
                .expect("recheck"),
            "the re-check sees the live delivery lease and spares the run"
        );
        assert_eq!(
            obligation(&pool, &seeded.run_id).await,
            ("open".to_owned(), None),
            "the obligation is handed back to its retry"
        );
        assert!(!is_terminal_run_status(
            &run_status(&pool, &seeded.run_id).await
        ));
        assert_eq!(fold_rows(&pool, &seeded.run_id).await, 1, "fold kept");
        assert_eq!(terminal_receipts(&pool, &seeded.run_id).await, 0);

        // The control: once that lease lapses too, the same run is recovered, so the sparing
        // above was the live lease's doing and not a stuck obligation.
        set_delivery_lease(&pool, &seeded.run_id, -600).await;
        make_due(&pool, &seeded.run_id).await;
        isolate(&pool, &seeded.run_id).await;
        assert_eq!(
            recover_due_terminalizations(&pool).await.expect("pass"),
            1,
            "with the lease lapsed the run is recovered"
        );
        assert_eq!(terminal_receipts(&pool, &seeded.run_id).await, 1);
    }

    // -- Build plan WS03 S8-4: the worker spares a parked fold until its wait runs out ---------
    //
    // WRITTEN, NOT RUN by the author (the lead runs them through the disposable-Postgres
    // harness). They force the fold store and park recovery for their own thread, read the wait
    // from `park_ttl_seconds()` rather than assuming it, and age a park by rewriting its
    // `updated_at` (the park's age, the anchor of its wait).

    use crate::loop_checkpoint::test_support::cipher;
    use crate::loop_checkpoint::test_switch as store_switch;
    use mp_contracts::loop_fold::park_ref;

    /// A persistent managed run, started through the real start path (so its obligation is the
    /// real one), with no fold yet. Returns the run id.
    async fn started_run(pool: &PgPool, tag: &str) -> String {
        let sfx = new_ulid();
        let (org, user, thread) = (
            format!("pk-org-{tag}-{sfx}"),
            "u1".to_owned(),
            format!("pk-t-{tag}-{sfx}"),
        );
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id, created_at)
             VALUES ($1, $1, $2, $3, now())",
        )
        .bind(&thread)
        .bind(&org)
        .bind(&user)
        .execute(pool)
        .await
        .expect("thread seeded");
        start_managed_run_with_retention_inner(
            pool,
            pb::StartManagedRunRequest {
                thread_id: thread,
                parent_run_id: String::new(),
                agent_id: "general-v1".to_owned(),
                goal: "park recovery goal".to_owned(),
                mode: "execute".to_owned(),
                org_id: org.clone(),
                user_id: user.clone(),
                start_key: format!("pk-{tag}-{sfx}"),
                terminal_source: pb::ManagedRunSource::ExecutionAgent as i32,
            },
            RunRetention::persistent(),
        )
        .await
        .expect("the run starts")
        .run_id
    }

    /// A started run holding a PARK fold, saved through the real store under the park rules
    /// (their switches forced on for this call only), whose age is `age_secs` and whose
    /// obligation fell due an hour ago.
    #[allow(clippy::cast_precision_loss)]
    async fn parked(pool: &PgPool, tag: &str, age_secs: i64) -> Seeded {
        let run_id = started_run(pool, tag).await;
        {
            let _store = store_switch::Forced::set(true);
            let _park = park_test_switch::Forced::set(true);
            let until = Utc::now() + chrono::Duration::seconds(3600);
            seed_fold(
                pool,
                &run_id,
                "park",
                &park_ref(&run_id, 1),
                "provider_exhausted",
                Some(until),
            )
            .await
            .expect("the park is stored under the park rules");
        }
        sqlx::query(
            "UPDATE run_loop_checkpoints SET updated_at = now() - make_interval(secs => $2)
              WHERE run_id = $1",
        )
        .bind(&run_id)
        .bind(age_secs as f64)
        .execute(pool)
        .await
        .expect("age the park");
        make_due(pool, &run_id).await;
        Seeded { run_id }
    }

    /// `(state, deadline in seconds from now, next attempt in seconds from now, lease cleared)`
    /// of the run's obligation.
    async fn obligation_timing(pool: &PgPool, run_id: &str) -> (String, f64, f64, bool) {
        sqlx::query_as(
            "SELECT state,
                    EXTRACT(EPOCH FROM (deadline_at - now()))::float8,
                    EXTRACT(EPOCH FROM (next_attempt_at - now()))::float8,
                    lease_owner IS NULL AND lease_token_hash IS NULL AND lease_expires_at IS NULL
               FROM managed_run_terminalization_outbox WHERE run_id = $1",
        )
        .bind(run_id)
        .fetch_one(pool)
        .await
        .expect("obligation timing")
    }

    async fn fold_state_of(pool: &PgPool, run_id: &str) -> Option<String> {
        crate::loop_checkpoint::test_support::fold_state(pool, run_id).await
    }

    /// The worker claims the run's obligation and runs its recheck: `Ok(true)` when it ended the
    /// run, `Ok(false)` when it spared it.
    async fn worker_pass_on(pool: &PgPool, run_id: &str) -> bool {
        make_due(pool, run_id).await;
        isolate(pool, run_id).await;
        let claim = claim_due_recovery(pool)
            .await
            .expect("claim")
            .expect("the run's obligation is due");
        assert_eq!(claim.row.run_id, run_id);
        recover_claimed_terminalization(pool, claim)
            .await
            .expect("the recheck")
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\""]
    async fn a_parked_fold_is_spared_until_the_ttl_then_terminalized_once() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let _store = store_switch::Forced::set(true);
        let _park = park_test_switch::Forced::set(true);
        let ttl = crate::loop_checkpoint::park_ttl_seconds();

        // Early in its wait: spared, and the obligation is handed back due at `now + 900 s`
        // (the deadline period), the nearer of the two ends.
        let early = parked(&pool, "early", 600).await;
        assert!(
            !worker_pass_on(&pool, &early.run_id).await,
            "a park inside its wait is spared"
        );
        let (state, deadline, next, lease_cleared) = obligation_timing(&pool, &early.run_id).await;
        assert_eq!(state, "open");
        assert!(lease_cleared, "the lease is cleared");
        let period = MANAGED_DEADLINE_SECS as f64;
        for (what, secs) in [("deadline", deadline), ("next attempt", next)] {
            assert!(
                secs > period - 60.0 && secs <= period,
                "{what} is a deadline period from now, not the wait's end: {secs}"
            );
        }
        assert!(!is_terminal_run_status(&run_status(&pool, &early.run_id).await));
        assert_eq!(fold_rows(&pool, &early.run_id).await, 1, "the park is kept");
        assert_eq!(terminal_receipts(&pool, &early.run_id).await, 0);
        // Not due again yet: a whole pass leaves it alone.
        assert_eq!(recover_due_terminalizations(&pool).await.expect("pass"), 0);

        // Near the end of its wait: the horizon is the wait's end, which is sooner than a
        // deadline period, so the obligation is not parked past it.
        let near = parked(&pool, "near", ttl - 120).await;
        assert!(!worker_pass_on(&pool, &near.run_id).await);
        let (state, deadline, next, _cleared) = obligation_timing(&pool, &near.run_id).await;
        assert_eq!(state, "open");
        for (what, secs) in [("deadline", deadline), ("next attempt", next)] {
            assert!(
                secs > 60.0 && secs <= 120.0,
                "{what} is the end of the wait, about two minutes away: {secs}"
            );
        }
        assert_eq!(fold_rows(&pool, &near.run_id).await, 1);

        // A park a continuation has claimed (resuming) is spared inside its wait too.
        let resuming = parked(&pool, "resuming", 600).await;
        sqlx::query(
            "UPDATE run_loop_checkpoints SET park_until = now() - interval '1 second' WHERE run_id = $1",
        )
        .bind(&resuming.run_id)
        .execute(&pool)
        .await
        .expect("make the park due");
        crate::loop_checkpoint::claim(&pool, &cipher(), &resuming.run_id, "", "token-a")
            .await
            .expect("a continuation claims the due park");
        assert_eq!(fold_state_of(&pool, &resuming.run_id).await.as_deref(), Some("resuming"));
        assert!(!worker_pass_on(&pool, &resuming.run_id).await);
        assert_eq!(obligation_timing(&pool, &resuming.run_id).await.0, "open");
        assert_eq!(fold_rows(&pool, &resuming.run_id).await, 1);
        assert_eq!(terminal_receipts(&pool, &resuming.run_id).await, 0);

        // Past its wait: the run ends outcome_unknown through the unchanged path, the fold is
        // purged with it, and the run has exactly one terminal receipt.
        let over = parked(&pool, "over", ttl + 60).await;
        isolate(&pool, &over.run_id).await;
        assert_eq!(
            recover_due_terminalizations(&pool).await.expect("pass"),
            1,
            "exactly this run is recovered"
        );
        assert_eq!(run_status(&pool, &over.run_id).await, "failed");
        let (state, failure) = obligation(&pool, &over.run_id).await;
        assert_eq!(state, "reconciliation_required");
        assert_eq!(failure.as_deref(), Some(RECOVERY_FAILURE_CODE));
        assert_eq!(fold_rows(&pool, &over.run_id).await, 0, "the fold is purged");
        assert!(obligation_receipt(&pool, &over.run_id).await.is_some());
        assert_eq!(terminal_receipts(&pool, &over.run_id).await, 1);
        // A second pass finds nothing to end: still one receipt.
        isolate(&pool, &over.run_id).await;
        assert_eq!(recover_due_terminalizations(&pool).await.expect("pass"), 0);
        assert_eq!(terminal_receipts(&pool, &over.run_id).await, 1);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\""]
    async fn park_recovery_off_is_the_committed_worker() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let ttl = crate::loop_checkpoint::park_ttl_seconds();

        // Park recovery off, the store on: a park inside its wait is an ordinary overdue run.
        // Store off, park recovery on: the same (the rules need both).
        for (what, store_on, park_on) in [
            ("park recovery off", true, false),
            ("the fold store off", false, true),
            ("both off", false, false),
        ] {
            let seeded = parked(&pool, "off", 600).await;
            isolate(&pool, &seeded.run_id).await;
            let _store = store_switch::Forced::set(store_on);
            let _park = park_test_switch::Forced::set(park_on);
            assert!(
                worker_pass_on(&pool, &seeded.run_id).await,
                "{what}: a park inside its wait is not spared"
            );
            assert_eq!(run_status(&pool, &seeded.run_id).await, "failed", "{what}");
            assert_eq!(obligation(&pool, &seeded.run_id).await.0, "reconciliation_required", "{what}");
            assert_eq!(fold_rows(&pool, &seeded.run_id).await, 0, "{what}: the fold is purged");
            assert_eq!(terminal_receipts(&pool, &seeded.run_id).await, 1, "{what}");
        }

        // With both on, the control: the same park IS spared, so the three above are the
        // switches' doing.
        let _store = store_switch::Forced::set(true);
        let _park = park_test_switch::Forced::set(true);
        let spared = parked(&pool, "control", 600).await;
        assert!(!worker_pass_on(&pool, &spared.run_id).await);
        assert_eq!(fold_rows(&pool, &spared.run_id).await, 1);
        assert!(ttl > 600, "premise: the wait is longer than the park's age");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\""]
    async fn an_active_or_paused_fold_is_never_spared_by_the_park_branch() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let _store = store_switch::Forced::set(true);
        let _park = park_test_switch::Forced::set(true);

        for kind in ["round", "pre_effect", "pause", "closed park"] {
            let run_id = started_run(&pool, "kinds").await;
            match kind {
                "closed park" => {
                    seed_fold(
                        &pool,
                        &run_id,
                        "park",
                        &park_ref(&run_id, 1),
                        "provider_exhausted",
                        Some(Utc::now() + chrono::Duration::seconds(3600)),
                    )
                    .await
                    .expect("a park");
                    crate::loop_checkpoint::close(&pool, &run_id, 1, "park_not_taken")
                        .await
                        .expect("close");
                }
                "pause" => {
                    seed_fold(&pool, &run_id, "pause", "appr_never", "", None)
                        .await
                        .expect("a pause fold with no approval row");
                }
                other => {
                    seed_fold(&pool, &run_id, other, "", "", None)
                        .await
                        .expect("an active fold");
                }
            }
            assert_eq!(fold_rows(&pool, &run_id).await, 1, "{kind}: premise: a fold");
            assert!(
                worker_pass_on(&pool, &run_id).await,
                "{kind}: the park branch leaves it to the committed path"
            );
            assert_eq!(run_status(&pool, &run_id).await, "failed", "{kind}");
            assert_eq!(fold_rows(&pool, &run_id).await, 0, "{kind}: purged by the unchanged path");
            assert_eq!(terminal_receipts(&pool, &run_id).await, 1, "{kind}");
        }
    }

    /// A continuation claims the park while the worker holds the run's obligation, at both ends
    /// of the wait. The run row's lock orders them: inside the wait the park is spared whoever is
    /// first, and past it the run ends exactly once and the claimant's fence is refused.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\""]
    async fn a_claim_racing_the_worker_never_double_terminalizes() {
        use crate::loop_checkpoint::test_support::fold_for;
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let _store = store_switch::Forced::set(true);
        let _park = park_test_switch::Forced::set(true);
        let ttl = crate::loop_checkpoint::park_ttl_seconds();

        for (what, age_secs, wait_over) in [("inside the wait", 600, false), ("past the wait", ttl + 60, true)] {
            let seeded = parked(&pool, "race", age_secs).await;
            sqlx::query(
                "UPDATE run_loop_checkpoints SET park_until = now() - interval '1 second' WHERE run_id = $1",
            )
            .bind(&seeded.run_id)
            .execute(&pool)
            .await
            .expect("make the park due");
            isolate(&pool, &seeded.run_id).await;
            let claim = claim_due_recovery(&pool)
                .await
                .expect("claim")
                .expect("the obligation is due");
            assert_eq!(claim.row.run_id, seeded.run_id);

            let key = cipher();
            let (worker, claimed) = tokio::join!(
                recover_claimed_terminalization(&pool, claim),
                crate::loop_checkpoint::claim(&pool, &key, &seeded.run_id, "", "token-a"),
            );
            let ended = worker.expect("the worker's pass");
            assert_eq!(ended, wait_over, "{what}");
            if wait_over {
                // Exactly one end. The claimant either won before it (and its fold went with the
                // run) or lost to it; either way the run is over and its fence is refused.
                match claimed {
                    Ok(won) => assert_eq!(won.kind, "park", "{what}"),
                    Err(lost) => assert_eq!(lost.message(), "loop_checkpoint_run_ended", "{what}"),
                }
                assert_eq!(run_status(&pool, &seeded.run_id).await, "failed", "{what}");
                assert_eq!(fold_rows(&pool, &seeded.run_id).await, 0, "{what}");
                let fold = fold_for(&seeded.run_id);
                let fence = crate::loop_checkpoint::save(
                    &pool,
                    &cipher(),
                    crate::loop_checkpoint::SaveInput {
                        run_id: &seeded.run_id,
                        expected_seq: 1,
                        kind: "pre_effect",
                        fold: &fold,
                        authority_digest: "auth-1",
                        pause_ref: "",
                        park_reason: "",
                        park_until: None,
                    },
                )
                .await
                .expect_err("a fence on an ended run");
                assert_eq!(fence.message(), "loop_checkpoint_run_ended", "{what}");
            } else {
                // Inside the wait the claim wins (the park is due) and the park is spared.
                assert_eq!(claimed.expect("the due park is claimed").kind, "park", "{what}");
                assert!(!is_terminal_run_status(&run_status(&pool, &seeded.run_id).await), "{what}");
                assert_eq!(fold_state_of(&pool, &seeded.run_id).await.as_deref(), Some("resuming"));
                assert_eq!(fold_rows(&pool, &seeded.run_id).await, 1, "{what}");
            }
            assert_eq!(terminal_receipts(&pool, &seeded.run_id).await, i64::from(wait_over), "{what}");
        }
    }

    // --- crash park (WS03 S8-B) ---------------------------------------------------------------
    //
    // The recovery worker parks the active round fold a dead loop left, instead of ending the run,
    // only when a continuation could resume it. Every test turns the three switches it needs on
    // for its own thread; the database is shared, so each scenario isolates its run first.

    use crate::loop_checkpoint::test_support::{
        crash_parkable_fold, crash_parkable_value, digest_and_bytes, plant_round_fold,
        seed_crash_fold,
    };

    struct CrashSwitches {
        _store: store_switch::Forced,
        _park: park_test_switch::Forced,
        _crash: crash_test_switch::Forced,
    }

    fn crash_switches(park: bool, crash: bool) -> CrashSwitches {
        CrashSwitches {
            _store: store_switch::Forced::set(true),
            _park: park_test_switch::Forced::set(park),
            _crash: crash_test_switch::Forced::set(crash),
        }
    }

    /// A started run under `retention`, with no fold yet.
    async fn started_run_as(pool: &PgPool, tag: &str, retention: RunRetention) -> String {
        let sfx = new_ulid();
        let (org, user) = (format!("cp-org-{tag}-{sfx}"), "u1".to_owned());
        let thread_id = if retention.class == RetentionClass::Zdr {
            String::new()
        } else {
            let thread = format!("cp-t-{tag}-{sfx}");
            sqlx::query(
                "INSERT INTO threads (id, session_key, org_id, user_id, created_at)
                 VALUES ($1, $1, $2, $3, now())",
            )
            .bind(&thread)
            .bind(&org)
            .bind(&user)
            .execute(pool)
            .await
            .expect("thread seeded");
            thread
        };
        start_managed_run_with_retention_inner(
            pool,
            pb::StartManagedRunRequest {
                thread_id,
                parent_run_id: String::new(),
                agent_id: "general-v1".to_owned(),
                goal: "crash park goal".to_owned(),
                mode: "execute".to_owned(),
                org_id: org,
                user_id: user,
                start_key: format!("cp-{tag}-{sfx}"),
                terminal_source: pb::ManagedRunSource::ExecutionAgent as i32,
            },
            retention,
        )
        .await
        .expect("the run starts")
        .run_id
    }

    /// A persistent run holding the active round fold a crash-parkable loop leaves (stamped with
    /// its thread position by the real save, so the switches are set before this is called).
    async fn crashed_run(pool: &PgPool, tag: &str) -> String {
        let run_id = started_run_as(pool, tag, RunRetention::persistent()).await;
        assert_eq!(seed_crash_fold(pool, &run_id, "round").await, 1);
        run_id
    }

    async fn run_thread(pool: &PgPool, run_id: &str) -> String {
        sqlx::query_scalar("SELECT thread_id FROM runs WHERE id = $1")
            .bind(run_id)
            .fetch_one(pool)
            .await
            .expect("the run's thread")
    }

    async fn add_message(pool: &PgPool, run_id: &str, role: &str) {
        sqlx::query("INSERT INTO messages (id, thread_id, role, content) VALUES ($1, $2, $3, 'text')")
            .bind(format!("cp-m-{}", new_ulid()))
            .bind(run_thread(pool, run_id).await)
            .bind(role)
            .execute(pool)
            .await
            .expect("a message lands on the thread");
    }

    /// `(kind, state, park_reason, seq, seconds until park_until, seconds since updated_at)`.
    async fn fold_row(
        pool: &PgPool,
        run_id: &str,
    ) -> (String, String, Option<String>, i64, Option<f64>, f64) {
        sqlx::query_as(
            "SELECT kind, state, park_reason, seq,
                    EXTRACT(EPOCH FROM (park_until - now()))::float8,
                    EXTRACT(EPOCH FROM (now() - updated_at))::float8
               FROM run_loop_checkpoints WHERE run_id = $1",
        )
        .bind(run_id)
        .fetch_one(pool)
        .await
        .expect("the fold row")
    }

    async fn heartbeat(pool: &PgPool, run_id: &str) {
        let beat = heartbeat_managed_run_inner(
            pool,
            pb::HeartbeatManagedRunRequest {
                run_id: run_id.to_owned(),
                source: pb::ManagedRunSource::ExecutionAgent as i32,
            },
        )
        .await
        .expect("heartbeat");
        assert!(!beat.already_terminal);
    }

    async fn set_park_until(pool: &PgPool, run_id: &str, secs_from_now: f64) {
        sqlx::query(
            "UPDATE run_loop_checkpoints SET park_until = now() + make_interval(secs => $2)
              WHERE run_id = $1",
        )
        .bind(run_id)
        .bind(secs_from_now)
        .execute(pool)
        .await
        .expect("move park_until");
    }

    async fn saved_events(pool: &PgPool, run_id: &str, seq: i64) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM events WHERE run_id = $1 AND resource_ref = $2")
            .bind(run_id)
            .bind(format!("run:{run_id}:loop_checkpoint:{seq}:CHECKPOINT_SAVED"))
            .fetch_one(pool)
            .await
            .expect("count the save events")
    }

    /// The run was ended by the committed path: failed `outcome_unknown`, its fold purged, one
    /// terminal receipt.
    async fn assert_ended_as_committed(pool: &PgPool, run_id: &str, what: &str) {
        assert_eq!(run_status(pool, run_id).await, "failed", "{what}");
        let (state, failure) = obligation(pool, run_id).await;
        assert_eq!(state, "reconciliation_required", "{what}");
        assert_eq!(failure.as_deref(), Some(RECOVERY_FAILURE_CODE), "{what}");
        assert_eq!(fold_rows(pool, run_id).await, 0, "{what}: the fold is purged");
        assert_eq!(terminal_receipts(pool, run_id).await, 1, "{what}");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\""]
    async fn recover_parks_a_crashed_run_is_parked_not_ended_and_a_continuation_claims_it() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let _on = crash_switches(true, true);
        let run_id = crashed_run(&pool, "crash").await;
        let (kind, state, ..) = fold_row(&pool, &run_id).await;
        assert_eq!((kind.as_str(), state.as_str()), ("round", "active"), "premise");

        // The worker finds the overdue run with an active round fold: it parks it.
        assert!(!worker_pass_on(&pool, &run_id).await, "parked, not ended");
        let (kind, state, reason, seq, until, age) = fold_row(&pool, &run_id).await;
        assert_eq!((kind.as_str(), state.as_str()), ("park", "parked"));
        assert_eq!(reason.as_deref(), Some("crash_interrupted"));
        assert_eq!(seq, 2, "re-sealed one seq above the fold the loop left");
        let until = until.expect("a resume time");
        assert!(until > 800.0 && until <= 900.0, "due after the grace period: {until}");
        assert!(age < 60.0, "the park's age is now, the anchor of its wait: {age}");
        assert!(!is_terminal_run_status(&run_status(&pool, &run_id).await));
        assert_eq!(terminal_receipts(&pool, &run_id).await, 0);
        assert_eq!(saved_events(&pool, &run_id, 2).await, 1, "one CHECKPOINT_SAVED for the park");
        let (obligation_state, deadline, next, lease_cleared) =
            obligation_timing(&pool, &run_id).await;
        assert_eq!(obligation_state, "open");
        assert!(lease_cleared);
        let period = MANAGED_DEADLINE_SECS as f64;
        assert!(deadline > period - 60.0 && deadline <= period, "{deadline}");
        assert!(next > period - 60.0 && next <= period, "{next}");

        // Not claimable before the grace has passed.
        let early = crate::loop_checkpoint::claim(&pool, &cipher(), &run_id, "", "tok-a")
            .await
            .expect_err("a crash park is not due yet");
        assert_eq!(early.message(), "park_not_due");

        // A heartbeat of a loop that is alive pushes the park on, when crash parking is on, and
        // never moves its age.
        set_park_until(&pool, &run_id, 60.0).await;
        {
            let _off = crash_test_switch::Forced::set(false);
            heartbeat(&pool, &run_id).await;
            assert!(fold_row(&pool, &run_id).await.4.expect("until") < 120.0, "off: no push");
        }
        heartbeat(&pool, &run_id).await;
        let (.., until, age) = fold_row(&pool, &run_id).await;
        assert!(until.expect("until") > 800.0, "on: pushed a grace period on");
        assert!(age < 120.0, "the heartbeat leaves the park's age alone: {age}");

        // Once due, a continuation claims it: the park fold, pending park 1, the route keys.
        set_park_until(&pool, &run_id, -1.0).await;
        let claimed = crate::loop_checkpoint::claim(&pool, &cipher(), &run_id, "", "tok-a")
            .await
            .expect("the due crash park is claimed");
        assert_eq!(claimed.kind, "park");
        assert_eq!(claimed.seq, 2);
        let fold: mp_contracts::loop_fold::LoopFoldV1 =
            serde_json::from_slice(&claimed.fold).expect("a fold");
        let pending = fold.pending.as_ref().expect("pending");
        assert_eq!(pending.kind, mp_contracts::loop_fold::PauseKind::Park);
        assert_eq!(pending.pause_ref, park_ref(&run_id, 1));
        assert_eq!(mp_contracts::loop_fold::parks_in(&fold), 1);
        assert_eq!(mp_contracts::loop_fold::route_model_in(&fold), Some("model-x"));
        assert_eq!(fold_state_of(&pool, &run_id).await.as_deref(), Some("resuming"));
        // A claimed park is a continuation's: a heartbeat leaves its time alone.
        let before = fold_row(&pool, &run_id).await.4;
        heartbeat(&pool, &run_id).await;
        assert!(fold_row(&pool, &run_id).await.4 <= before.map(|secs| secs + 1.0));

        // An unclaimed crash park ends outcome_unknown once its wait has run out.
        let stale = crashed_run(&pool, "ttl").await;
        assert!(!worker_pass_on(&pool, &stale).await, "parked");
        let ttl = crate::loop_checkpoint::park_ttl_seconds();
        sqlx::query(
            "UPDATE run_loop_checkpoints SET updated_at = now() - make_interval(secs => $2)
              WHERE run_id = $1",
        )
        .bind(&stale)
        .bind((ttl + 60) as f64)
        .execute(&pool)
        .await
        .expect("age the park");
        assert!(worker_pass_on(&pool, &stale).await, "past its wait: ended");
        assert_ended_as_committed(&pool, &stale, "ttl").await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\""]
    async fn recover_parks_crash_park_off_is_the_committed_worker() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        for (what, park_on, crash_on) in [
            ("crash park off", true, false),
            ("park recovery off", false, true),
            ("both off", false, false),
        ] {
            let _switches = crash_switches(park_on, crash_on);
            let run_id = crashed_run(&pool, "off").await;
            assert!(worker_pass_on(&pool, &run_id).await, "{what}: the run is ended");
            assert_ended_as_committed(&pool, &run_id, what).await;
        }
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\""]
    async fn recover_parks_a_fold_no_continuation_could_resume_is_not_parked() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let _on = crash_switches(true, true);
        // An expired approval no longer spares the run, so the crash park's own check sees it.
        let _expiry = expiry_test_switch::Forced::set(true);

        for case in [
            "no executor markers",
            "a pre_effect fold",
            "a message after the fold",
            "an open approval",
            "an effect after the fold",
            "an open reservation",
            "a zero-retention run",
            "a waived run",
        ] {
            let run_id = match case {
                "a zero-retention run" | "a waived run" => {
                    let retention = if case == "a zero-retention run" {
                        RunRetention::zdr()
                    } else {
                        RunRetention::waived("ack-1", "conn-1")
                    };
                    let run_id = started_run_as(&pool, "refuse", retention).await;
                    let (bytes, digest) = crash_parkable_fold(&run_id);
                    plant_round_fold(&pool, &run_id, &bytes, &digest).await;
                    run_id
                }
                "no executor markers" => {
                    let run_id = started_run_as(&pool, "refuse", RunRetention::persistent()).await;
                    let mut value = crash_parkable_value(&run_id);
                    for key in ["route_model", "route_connection_id", "child_ceiling"] {
                        value["extensions"].as_object_mut().unwrap().remove(key);
                    }
                    let (bytes, digest) = digest_and_bytes(&value);
                    crate::loop_checkpoint::save(
                        &pool,
                        &cipher(),
                        crate::loop_checkpoint::SaveInput {
                            run_id: &run_id,
                            expected_seq: 0,
                            kind: "round",
                            fold: &bytes,
                            authority_digest: &digest,
                            pause_ref: "",
                            park_reason: "",
                            park_until: None,
                        },
                    )
                    .await
                    .expect("stored");
                    run_id
                }
                "a pre_effect fold" => {
                    let run_id = started_run_as(&pool, "refuse", RunRetention::persistent()).await;
                    seed_crash_fold(&pool, &run_id, "pre_effect").await;
                    run_id
                }
                other => {
                    let run_id = crashed_run(&pool, "refuse").await;
                    match other {
                        "a message after the fold" => add_message(&pool, &run_id, "assistant").await,
                        "an open approval" => {
                            let (org, user): (String, String) = sqlx::query_as(
                                "SELECT org_id, user_id FROM runs WHERE id = $1",
                            )
                            .bind(&run_id)
                            .fetch_one(&pool)
                            .await
                            .expect("owner");
                            approvals::request_approval(
                                &pool,
                                &format!("appr_{}", new_ulid()),
                                &run_id,
                                None,
                                "tool_call",
                                &user,
                                &org,
                                &user,
                                "",
                                &serde_json::json!({}),
                                None,
                                Some(Utc::now() - chrono::Duration::seconds(3600)),
                                "",
                            )
                            .await
                            .expect("request approval")
                            .expect("inserted");
                        }
                        "an effect after the fold" => {
                            sqlx::query(
                                "INSERT INTO session_tool_audit_intents
                                     (run_id, action_id, request_id, tool, data_category, zdr, status)
                                 VALUES ($1, 'a-1', 'r-1', 'code_interpreter', 'cat', false, 'completed')",
                            )
                            .bind(&run_id)
                            .execute(&pool)
                            .await
                            .expect("a ledger row newer than the fold");
                        }
                        _ => {
                            sqlx::query(
                                "INSERT INTO session_tool_audit_intents
                                     (run_id, action_id, request_id, tool, data_category, zdr, status, reserved_at)
                                 VALUES ($1, 'a-1', 'r-1', 'code_interpreter', 'cat', false, 'reserved',
                                         now() - interval '1 hour')",
                            )
                            .bind(&run_id)
                            .execute(&pool)
                            .await
                            .expect("an open reservation older than the fold");
                        }
                    }
                    run_id
                }
            };
            assert!(worker_pass_on(&pool, &run_id).await, "{case}: the run is ended, not parked");
            assert_ended_as_committed(&pool, &run_id, case).await;
        }
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\""]
    async fn recover_parks_a_thread_held_by_a_writer_leaves_the_run_alone() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let _on = crash_switches(true, true);
        let run_id = crashed_run(&pool, "busy").await;
        let thread = run_thread(&pool, &run_id).await;

        // An answer append holds the thread: the producer is alive and finishing, so the worker
        // neither parks nor ends the run, and does not wait for the lock.
        let mut writer = pool.begin().await.expect("a writer's transaction");
        sqlx::query("SELECT id FROM threads WHERE id = $1 FOR UPDATE")
            .bind(&thread)
            .execute(&mut *writer)
            .await
            .expect("the writer takes the thread");
        assert!(!worker_pass_on(&pool, &run_id).await, "left alone");
        let (kind, state, ..) = fold_row(&pool, &run_id).await;
        assert_eq!((kind.as_str(), state.as_str()), ("round", "active"));
        assert!(!is_terminal_run_status(&run_status(&pool, &run_id).await));
        assert_eq!(obligation(&pool, &run_id).await.0, "open");
        assert_eq!(terminal_receipts(&pool, &run_id).await, 0);
        writer.commit().await.expect("the writer commits");

        // With the thread free again the same run is parked.
        assert!(!worker_pass_on(&pool, &run_id).await);
        assert_eq!(fold_row(&pool, &run_id).await.0, "park");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\""]
    async fn recover_parks_a_crash_park_is_not_claimed_once_its_thread_moved() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let _on = crash_switches(true, true);

        // A message that lands after the park: the claim closes the fold and refuses, and the
        // run is then ended by the committed path, never continued into a second answer.
        let run_id = crashed_run(&pool, "moved").await;
        assert!(!worker_pass_on(&pool, &run_id).await, "parked");
        set_park_until(&pool, &run_id, -1.0).await;
        add_message(&pool, &run_id, "assistant").await;
        let refused = crate::loop_checkpoint::claim(&pool, &cipher(), &run_id, "", "tok-a")
            .await
            .expect_err("the thread moved");
        assert_eq!(refused.message(), "loop_checkpoint_not_claimable");
        assert_eq!(fold_state_of(&pool, &run_id).await.as_deref(), Some("closed"));
        assert!(worker_pass_on(&pool, &run_id).await, "a closed fold is not spared");
        assert_ended_as_committed(&pool, &run_id, "moved").await;

        // An append in flight is waited for by nobody: the claim is refused and rolls back, so
        // the park is exactly as it was and can be claimed once the thread is free.
        let held = crashed_run(&pool, "held").await;
        assert!(!worker_pass_on(&pool, &held).await, "parked");
        set_park_until(&pool, &held, -1.0).await;
        let thread = run_thread(&pool, &held).await;
        let mut writer = pool.begin().await.expect("a writer's transaction");
        sqlx::query("SELECT id FROM threads WHERE id = $1 FOR UPDATE")
            .bind(&thread)
            .execute(&mut *writer)
            .await
            .expect("the writer takes the thread");
        let busy = crate::loop_checkpoint::claim(&pool, &cipher(), &held, "", "tok-a")
            .await
            .expect_err("the thread is held");
        assert_eq!(busy.message(), "loop_checkpoint_claim_held");
        assert_eq!(fold_state_of(&pool, &held).await.as_deref(), Some("parked"));
        writer.commit().await.expect("the writer commits");
        let won = crate::loop_checkpoint::claim(&pool, &cipher(), &held, "", "tok-a")
            .await
            .expect("claimed once the thread is free");
        assert_eq!(won.kind, "park");
    }
}

/// WS06-S1: a ZDR managed start that names an existing thread is the typed
/// `zdr_feature_not_supported` refusal for the `agent_run` feature.
#[cfg(test)]
mod zdr_refusal_tests {
    use super::*;
    use mp_contracts::zdr_refusal::Refusal;

    fn start(thread_id: &str) -> pb::StartManagedRunRequest {
        pb::StartManagedRunRequest {
            thread_id: thread_id.to_owned(),
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
            start_key: "opaque-start-1".to_owned(),
            terminal_source: pb::ManagedRunSource::GatewayDirect as i32,
            ..Default::default()
        }
    }

    #[test]
    fn zdr_refusal_managed_start_with_a_thread_names_the_agent_run_feature() {
        let status = validate_managed_start_request(&start("thread-1"), true)
            .expect_err("a ZDR run needs a fresh metadata-only thread");
        assert_eq!(status.code(), tonic::Code::FailedPrecondition);
        let refusal = Refusal::from_status(&status).expect("typed refusal");
        assert_eq!(refusal.code(), mp_contracts::zdr_refusal::code::FEATURE_NOT_SUPPORTED);
        assert_eq!(refusal.feature(), Some("agent_run"));
    }

    #[test]
    fn zdr_refusal_managed_start_without_a_thread_is_accepted() {
        assert!(validate_managed_start_request(&start(""), true).is_ok());
    }
}
