//! gRPC handler for the `RunService` — the run read model + cancel path.
//!
//! session-core owns run metadata (the `runs` table + the append-only `events`
//! table). This service exposes that durable record to the runs-history UI:
//!
//! * `GetRun`   — one run's full [`pb::RunDetail`], with `steps_completed`
//!   derived from the run's `STEP_COMPLETED` event ordinals and
//!   `checkpoint_index` from its checkpoint count.
//! * `ListRuns` — a thread's runs, org-scoped, newest-first, with an
//!   `after_run_id` ULID cursor + `limit` and a `has_more` flag.
//! * `CancelRun` — flips a non-terminal run to `cancelled`, org-scoped, and
//!   appends the durable `RUN_CANCELLED` event in the same transaction. The
//!   returned event id is the cancellation receipt; the read model and audit
//!   record therefore cannot disagree about whether the stop was recorded.
//!
//! Token usage (`input_tokens` / `output_tokens`) is not tracked in session-core
//! — it lives on the inference path — so those fields are reported as 0 here.

// tonic::Status is the unavoidable large Err for gRPC; boxing breaks the service-trait contract.
#![allow(clippy::result_large_err)]

use chrono::Utc;
use mp_contracts::loop_fold::ParkReason;
use mp_contracts::model_plane::v1::{
    self as pb,
    run_service_server::{RunService, RunServiceServer},
};
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Instant;
use tonic::{Request, Response, Status};
use mp_ids::new_ulid;

use crate::auth::{
    authorize_operation, authorize_owner_row, authorize_run_action_authority_service,
    authorize_scheduled_step_authority_service, authorize_scheduled_step_service,
    authorize_thread_owner_row, identity, OwnerIntent, VerifiedIdentity,
};
use crate::orchestration_grpc::json_to_struct;

/// `space_membership_revoked` is true when the row has a non-null
/// `space_id` AND that (space_id, user_id) pair has a row in
/// `space_membership_revocations` (migration 0032, populated by
/// `space_membership_nats.rs`). A row with a null `space_id` never matches
/// the revocation join, so unscoped runs/threads are unaffected.
pub(crate) async fn authorize_run_owner(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    run_id: &str,
    intent: OwnerIntent,
) -> Result<(), Status> {
    let owner: Option<(String, String, bool)> = sqlx::query_as(
        "SELECT r.org_id, r.user_id, \
         EXISTS(SELECT 1 FROM space_membership_revocations rev \
                WHERE rev.space_ref = r.space_id AND rev.subject_id = r.user_id) \
         FROM runs r WHERE r.id = $1",
    )
    .bind(run_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| Status::internal(error.to_string()))?;
    let (org_id, user_id, space_membership_revoked) =
        owner.ok_or_else(|| Status::not_found("run not found"))?;
    if space_membership_revoked {
        return Err(Status::permission_denied(
            "Space membership required for this run",
        ));
    }
    authorize_owner_row(caller, &org_id, &user_id, intent)
}

async fn authorize_thread_owner(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    thread_id: &str,
    intent: OwnerIntent,
) -> Result<(), Status> {
    // `space_id` rides along because a system-owned thread inside a Space is
    // not org-readable by id (see `authorize_thread_owner_row`).
    let owner: Option<(String, String, String, bool)> = sqlx::query_as(
        "SELECT t.org_id, t.user_id, COALESCE(t.space_id, ''), \
         EXISTS(SELECT 1 FROM space_membership_revocations rev \
                WHERE rev.space_ref = t.space_id AND rev.subject_id = t.user_id) \
         FROM threads t WHERE t.id = $1",
    )
    .bind(thread_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| Status::internal(error.to_string()))?;
    let (org_id, user_id, space_id, space_membership_revoked) =
        owner.ok_or_else(|| Status::not_found("thread not found"))?;
    if space_membership_revoked {
        return Err(Status::permission_denied(
            "Space membership required for this thread",
        ));
    }
    authorize_thread_owner_row(caller, &org_id, &user_id, &space_id, intent)
}

/// Seam for `resolve_thread_owner`'s durable lookup, mirroring `memory_grpc.rs`'s
/// `ThreadOwnership` trait one file over. `resolve_run_owner` (the RPC this
/// handler is modeled on) queries `self.pool` directly with no injectable seam
/// and has zero unit-test coverage as a result; this new, security-critical
/// authorization RPC gets the testable shape instead of perpetuating that gap.
#[tonic::async_trait]
trait ThreadOwnerLookup: Send + Sync {
    async fn owner_matches(
        &self,
        thread_id: &str,
        org_id: &str,
        user_id: &str,
    ) -> Result<bool, Status>;
}

struct PgThreadOwnerLookup {
    pool: PgPool,
}

#[tonic::async_trait]
impl ThreadOwnerLookup for PgThreadOwnerLookup {
    async fn owner_matches(
        &self,
        thread_id: &str,
        org_id: &str,
        user_id: &str,
    ) -> Result<bool, Status> {
        // A revoked Space membership denies even a matching org+user row —
        // see authorize_run_owner's doc comment above for why: a null
        // space_id (unscoped thread) never matches the revocation join, so
        // this only tightens Space-scoped threads.
        let (authorized,): (bool,) = sqlx::query_as(
            "SELECT EXISTS(SELECT 1 FROM threads t WHERE t.id = $1 AND t.org_id = $2 AND t.user_id = $3 \
             AND NOT EXISTS(SELECT 1 FROM space_membership_revocations rev \
                             WHERE rev.space_ref = t.space_id AND rev.subject_id = t.user_id))",
        )
        .bind(thread_id)
        .bind(org_id)
        .bind(user_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| {
            tracing::error!(%error, "durable thread ownership lookup failed");
            Status::unavailable("thread ownership unavailable")
        })?;
        Ok(authorized)
    }
}

/// Hard cap on `ListRuns.limit` so a hostile or buggy caller cannot ask for an
/// unbounded scan. Mirrors the bounded-page convention used across the cores.
const MAX_LIST_LIMIT: i64 = 200;
const DEFAULT_LIST_LIMIT: i64 = 50;
const RUN_CANCELLED_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.RunCancelled";

fn record_metrics(method: &'static str, started: Instant, is_ok: bool) {
    let status = if is_ok { "ok" } else { "error" };
    metrics::counter!(
        "mp_session_run_service_grpc_requests_total",
        "method" => method,
        "status" => status,
    )
    .increment(1);
    metrics::histogram!(
        "mp_session_run_service_grpc_request_duration_seconds",
        "method" => method,
    )
    .record(started.elapsed().as_secs_f64());
}

/// One `runs` row plus the two derived counters (`steps_completed`,
/// `checkpoint_index`) projected by correlated sub-selects. Timestamps are
/// projected as Unix seconds so they map onto `prost_types::Timestamp` without
/// timezone gymnastics; `metadata` is the raw JSONB column.
#[derive(sqlx::FromRow)]
struct RunRow {
    id: String,
    thread_id: String,
    parent_run_id: Option<String>,
    agent_id: String,
    status: String,
    mode: String,
    goal: String,
    final_output: Option<String>,
    error: Option<String>,
    metadata: serde_json::Value,
    created_at: i64,
    updated_at: i64,
    steps_completed: i64,
    checkpoint_index: i64,
    /// The run's retention receipt. A fold is only ever resumable for `persistent`.
    retention_class: String,
    /// Who owns the run. It decides who may read `park_reason`; it is never put on
    /// the wire.
    owner_user_id: String,
    /// The durable loop fold's state, from a LEFT JOIN: `None` when the run has no
    /// fold. The fold's content and digests are never selected.
    fold_state: Option<String>,
    /// How the fold was last saved (`round`, `pre_effect`, `pause`, `park`). Unlike
    /// the state, it survives a claim, so a parked fold whose resume lease lapsed
    /// still reads as parked.
    fold_kind: Option<String>,
    /// Whether a `resuming` fold's lease has lapsed; `None` without a fold.
    fold_claim_expired: Option<bool>,
    /// The closed park token of a parked fold.
    fold_park_reason: Option<String>,
    /// For a `pause` fold only (`None` for every other kind and without a fold):
    /// whether the approval the pause is keyed by (its `pause_ref`) has been
    /// consumed. A stage-one continuation consumes it and leaves the fold in
    /// place, so a consumed pause fold is stale: nothing can continue from it.
    fold_pause_consumed: Option<bool>,
}

/// The deliberately content-free run/thread projection used only by Control's
/// exact action-authorizer before it issues an owner-targeted effect decision.
/// A query must satisfy both immutable run context and the currently stored
/// thread context; a legacy, terminal, service-owned, or partially scoped row
/// simply does not become an authority source.
#[derive(sqlx::FromRow)]
struct RunActionAuthorityRow {
    run_id: String,
    org_id: String,
    subject_id: String,
    thread_id: String,
    run_status: String,
    space_id: Option<String>,
    recipient_audience_ref: Option<String>,
    recipient_audience_revision: Option<i64>,
    recipient_audience_hash: Option<String>,
    privacy_policy_ref: Option<String>,
    thread_resource_authorization_ref: Option<String>,
    authority_revision: Option<i64>,
}

/// Durable scheduled-run bindings projected for Control's per-step authority
/// refresh. The metadata is intentionally the only source for schedule/fire
/// identity: callers cannot turn a valid prepared thread into a different
/// workload by restating those values.
#[derive(sqlx::FromRow)]
struct ScheduledStepAuthorityRow {
    run_id: String,
    thread_id: String,
    org_id: String,
    space_id: String,
    run_status: String,
    metadata: serde_json::Value,
}

fn valid_sha256_digest(value: &str) -> bool {
    let value = value.trim();
    value.len() == "sha256:".len() + 64
        && value.starts_with("sha256:")
        && value["sha256:".len()..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
}

fn scheduled_metadata_string(metadata: &serde_json::Value, key: &str) -> Option<String> {
    metadata
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn project_scheduled_step_authority(
    row: ScheduledStepAuthorityRow,
    req: &pb::ResolveScheduledStepAuthorityRequest,
) -> Option<pb::ResolveScheduledStepAuthorityResponse> {
    if is_terminal(&row.run_status)
        || row.space_id.trim().is_empty()
        || row.run_id != req.run_id
        || row.thread_id != req.thread_id
        || row.org_id != req.org_id
        || req.step_id != format!("{}:step:{}", req.run_id, req.step_index)
        || req.idempotency_key.trim().is_empty()
        || !valid_sha256_digest(&req.template_digest)
        || !valid_sha256_digest(&req.policy_digest)
    {
        return None;
    }

    let source = scheduled_metadata_string(&row.metadata, "source")?;
    let schedule_id = scheduled_metadata_string(&row.metadata, "schedule_id")?;
    let fire_key = scheduled_metadata_string(&row.metadata, "fire_key")?;
    let space_id = scheduled_metadata_string(&row.metadata, "space_id")?;
    let subject_id = scheduled_metadata_string(&row.metadata, "subject_id")?;
    let template_digest = scheduled_metadata_string(&row.metadata, "template_digest")?;
    let policy_digest = scheduled_metadata_string(&row.metadata, "policy_digest")?;
    let idempotency_key = scheduled_metadata_string(&row.metadata, "idempotency_key")?;
    if source != "scheduled_run"
        || space_id != row.space_id.trim()
        || schedule_id != req.schedule_id.trim()
        || fire_key != req.fire_key.trim()
        || template_digest != req.template_digest.trim()
        || policy_digest != req.policy_digest.trim()
        || idempotency_key != req.idempotency_key.trim()
    {
        return None;
    }

    Some(pb::ResolveScheduledStepAuthorityResponse {
        resolved: true,
        run_id: row.run_id,
        thread_id: row.thread_id,
        org_id: row.org_id,
        subject_id,
        space_id,
        schedule_id,
        fire_key,
        template_digest,
        policy_digest,
        step_id: req.step_id.clone(),
        step_index: req.step_index,
        idempotency_key,
        run_status: row.run_status,
        // The contract fields (WS20-S5) are not read from the run's metadata yet: empty means
        // "no contract" to every consumer, which is today's behaviour.
        ..Default::default()
    })
}

/// Project a query row only when it is safe for Control to use as the durable
/// context source for a future action decision. This function intentionally
/// returns `None` rather than a partial projection: the caller must fail
/// closed and re-resolve current Control facts, not infer missing authority.
fn project_run_action_authority(
    row: RunActionAuthorityRow,
) -> Option<pb::ResolveRunActionAuthorityResponse> {
    if matches!(
        row.run_status.as_str(),
        "completed" | "failed" | "cancelled"
    ) || row.subject_id.starts_with("service:")
    {
        return None;
    }
    let space_id = row.space_id?.trim().to_owned();
    let recipient_audience_ref = row.recipient_audience_ref?.trim().to_owned();
    let recipient_audience_hash = row.recipient_audience_hash?.trim().to_owned();
    let privacy_policy_ref = row.privacy_policy_ref?.trim().to_owned();
    let thread_resource_authorization_ref =
        row.thread_resource_authorization_ref?.trim().to_owned();
    let recipient_audience_revision = u64::try_from(row.recipient_audience_revision?).ok()?;
    let authority_revision = u64::try_from(row.authority_revision?).ok()?;
    if space_id.is_empty()
        || recipient_audience_ref.is_empty()
        || recipient_audience_hash.is_empty()
        || privacy_policy_ref.is_empty()
        || thread_resource_authorization_ref.is_empty()
        || recipient_audience_revision == 0
        || authority_revision == 0
    {
        return None;
    }
    Some(pb::ResolveRunActionAuthorityResponse {
        resolved: true,
        run_id: row.run_id,
        org_id: row.org_id,
        subject_id: row.subject_id,
        thread_id: row.thread_id,
        space_id,
        recipient_audience_ref,
        recipient_audience_revision,
        recipient_audience_hash,
        privacy_policy_ref,
        thread_resource_authorization_ref,
        authority_revision,
        run_status: row.run_status,
    })
}

fn unresolved_run_action_authority() -> pb::ResolveRunActionAuthorityResponse {
    pb::ResolveRunActionAuthorityResponse {
        resolved: false,
        ..Default::default()
    }
}

/// Column projection shared by `GetRun` and `ListRuns`. `steps_completed` is the
/// highest `step_ordinal` recorded for the run (0 when none) and
/// `checkpoint_index` is the run's checkpoint count, both via correlated
/// sub-selects so a single round trip hydrates the whole `RunDetail`.
const RUN_SELECT: &str = concat!("SELECT
        r.id,
        r.thread_id,
        r.parent_run_id,
        r.agent_id,
        ", crate::run_phase::run_phase_sql!("r"), " AS status,
        r.mode,
        r.goal,
        r.final_output,
        r.error,
        r.metadata,
        extract(epoch from r.created_at)::bigint AS created_at,
        extract(epoch from r.updated_at)::bigint AS updated_at,
        COALESCE((
            SELECT MAX(e.step_ordinal)
            FROM events e
            WHERE e.run_id = r.id AND e.step_ordinal IS NOT NULL
        ), 0)::bigint AS steps_completed,
        COALESCE((
            SELECT COUNT(*)
            FROM checkpoints c
            WHERE c.run_id = r.id
        ), 0)::bigint AS checkpoint_index,
        r.retention_class,
        r.user_id AS owner_user_id,
        lc.state AS fold_state,
        lc.kind AS fold_kind,
        (lc.claim_expires_at < now()) AS fold_claim_expired,
        lc.park_reason AS fold_park_reason,
        CASE WHEN lc.kind = 'pause' THEN EXISTS (
            SELECT 1 FROM approvals ap
            WHERE ap.org_id = r.org_id
              AND ap.user_id = r.user_id
              AND ap.run_id = r.id
              AND ap.idempotency_key = lc.pause_ref
              AND ap.consumed_at IS NOT NULL
        ) END AS fold_pause_consumed
     FROM runs r
     LEFT JOIN run_loop_checkpoints lc ON lc.run_id = r.id");

/// Whether a durable loop fold is in a state a continuation can claim: paused for
/// an approval, parked, or `resuming` under a lease that has lapsed. It mirrors
/// the claim gate in `loop_checkpoint::claim` and deliberately ignores the pause
/// reference, which a reader does not hold. An `active` fold is never claimable.
fn fold_is_claimable(state: Option<&str>, claim_expired: bool) -> bool {
    match state {
        Some("paused" | "parked") => true,
        Some("resuming") => claim_expired,
        _ => false,
    }
}

/// Whether `viewer` may read a run's `park_reason`. Reading the run's status is not
/// enough: the reason can reveal a delegated-credential window, so it goes only to
/// the run's owner and to a service principal (model-gateway reads it as one). A
/// person who can read a system-owned run through the org-wide read, or a Space
/// peer who can list a thread's runs, gets `resumable` but no reason.
fn may_read_park_reason(viewer: &VerifiedIdentity, owner_user_id: &str) -> bool {
    viewer.is_service()
        || (!owner_user_id.is_empty() && viewer.user_id() == Some(owner_user_id))
}

#[allow(clippy::cast_sign_loss)]
fn row_to_detail(row: RunRow, store_enabled: bool, viewer: &VerifiedIdentity) -> pb::RunDetail {
    // The derived counters are non-negative by construction (MAX over a
    // non-negative ordinal, COUNT(*)), so the unsigned cast cannot wrap.
    let steps_completed = u32::try_from(row.steps_completed).unwrap_or(u32::MAX);
    let checkpoint_index = u32::try_from(row.checkpoint_index).unwrap_or(u32::MAX);

    // Resumable only while the store is on, the run is persistent and not ended,
    // and its fold is claimable. A zero-retention or waived run is never resumable,
    // even if a fold row exists for it.
    //
    // And a pause fold whose approval has been consumed is not resumable: the
    // continuation that consumed it ran from the approval's own descriptor and
    // left the fold behind, and the approval cannot be consumed again.
    let resumable = store_enabled
        && row.retention_class == "persistent"
        && !is_terminal(&row.status)
        && fold_is_claimable(
            row.fold_state.as_deref(),
            row.fold_claim_expired.unwrap_or(false),
        )
        && !(row.fold_kind.as_deref() == Some("pause") && row.fold_pause_consumed == Some(true));
    // The reason follows the fold's kind, not its state: a claim moves a parked
    // fold to `resuming` but leaves the kind and the reason, so a parked run whose
    // resume lease lapsed still says why it was parked.
    //
    // Only a closed token is ever emitted: the column is written through `save`,
    // which refuses anything else, but the read side does not trust that, so a
    // value that got in some other way reads as no reason rather than reaching the
    // wire. And only a caller who may read it gets it (see `may_read_park_reason`).
    let park_reason = if resumable
        && row.fold_kind.as_deref() == Some("park")
        && may_read_park_reason(viewer, &row.owner_user_id)
    {
        row.fold_park_reason
            .as_deref()
            .and_then(ParkReason::parse)
            .map_or_else(String::new, |reason| reason.as_str().to_owned())
    } else {
        String::new()
    };

    pb::RunDetail {
        run_id: row.id,
        thread_id: row.thread_id,
        parent_run_id: row.parent_run_id.unwrap_or_default(),
        agent_id: row.agent_id,
        status: row.status,
        mode: row.mode,
        goal: row.goal,
        final_output: row.final_output.unwrap_or_default(),
        error: row.error.unwrap_or_default(),
        checkpoint_index,
        steps_completed,
        // Token usage is owned by the inference path, not session-core.
        input_tokens: 0,
        output_tokens: 0,
        created_at: Some(prost_types::Timestamp {
            seconds: row.created_at,
            nanos: 0,
        }),
        updated_at: Some(prost_types::Timestamp {
            seconds: row.updated_at,
            nanos: 0,
        }),
        metadata: json_to_struct(&row.metadata),
        resumable,
        park_reason,
    }
}

/// Statuses a run can no longer leave. `CancelRun` rejects these so a completed
/// or already-cancelled run is never silently "re-cancelled".
fn is_terminal(status: &str) -> bool {
    matches!(status, "completed" | "failed" | "cancelled")
}

pub struct RunServiceImpl {
    pool: PgPool,
    thread_owner_lookup: Arc<dyn ThreadOwnerLookup>,
}

impl RunServiceImpl {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self {
            thread_owner_lookup: Arc::new(PgThreadOwnerLookup { pool: pool.clone() }),
            pool,
        }
    }

    #[cfg(test)]
    fn new_with_thread_owner_lookup_for_test(
        pool: PgPool,
        thread_owner_lookup: Arc<dyn ThreadOwnerLookup>,
    ) -> Self {
        Self {
            pool,
            thread_owner_lookup,
        }
    }

    /// Convenience for `grpc.rs` so the wiring mirrors how the other services in
    /// this crate register their tonic servers.
    #[must_use]
    #[allow(dead_code)] // direct constructor retained for isolated service tests
    pub fn into_server(self) -> RunServiceServer<Self> {
        RunServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl RunService for RunServiceImpl {
    async fn get_run(
        &self,
        request: Request<pb::GetRunRequest>,
    ) -> Result<Response<pb::RunDetail>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::RunDetail>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:read")?;
            let req = request.into_inner();
            if req.run_id.is_empty() {
                return Err(Status::invalid_argument("run_id is required"));
            }
            authorize_run_owner(&self.pool, &caller, &req.run_id, OwnerIntent::Read).await?;

            let row: Option<RunRow> = sqlx::query_as(&format!("{RUN_SELECT} WHERE r.id = $1"))
                .bind(&req.run_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;

            let row =
                row.ok_or_else(|| Status::not_found(format!("run not found: {}", req.run_id)))?;
            Ok(Response::new(row_to_detail(
                row,
                crate::loop_checkpoint::enabled(),
                &caller,
            )))
        }
        .await;
        record_metrics("get_run", started, result.is_ok());
        result
    }

    async fn get_scheduled_step_context(
        &self,
        request: Request<pb::GetScheduledStepContextRequest>,
    ) -> Result<Response<pb::ScheduledStepContext>, Status> {
        let caller = identity(&request)?;
        authorize_operation(&caller, crate::auth::SCHEDULED_STEP_SCOPE)?;
        authorize_scheduled_step_service(&caller)?;
        let req = request.into_inner();
        if req.run_id.trim().is_empty()
            || req.thread_id.trim().is_empty()
            || req.org_id.trim().is_empty()
        {
            return Err(Status::invalid_argument(
                "scheduled-step context bindings are required",
            ));
        }
        caller.authorize_org(&req.org_id)?;
        let row: Option<(String, String, String, String, String)> = sqlx::query_as(
            "SELECT r.id, r.thread_id, r.org_id, r.goal, r.status
             FROM runs r
             JOIN threads t ON t.id = r.thread_id
             WHERE r.id = $1 AND r.thread_id = $2 AND r.org_id = $3
               AND r.user_id = $4 AND COALESCE(t.space_id, '') <> ''
               AND COALESCE(r.metadata->>'source', '') = 'scheduled_run'",
        )
        .bind(&req.run_id)
        .bind(&req.thread_id)
        .bind(&req.org_id)
        .bind(crate::auth::system_run_owners()[0])
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
        let Some((run_id, thread_id, org_id, goal, status)) = row else {
            return Err(Status::not_found("scheduled run context not found"));
        };
        Ok(Response::new(pb::ScheduledStepContext {
            run_id,
            thread_id,
            org_id,
            goal,
            status,
            // The contract body, digest and expiry (WS20-S5) are not served yet: empty means
            // "no contract".
            ..Default::default()
        }))
    }

    async fn resolve_scheduled_step_authority(
        &self,
        request: Request<pb::ResolveScheduledStepAuthorityRequest>,
    ) -> Result<Response<pb::ResolveScheduledStepAuthorityResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::ResolveScheduledStepAuthorityResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_scheduled_step_authority_service(&caller)?;
            let req = request.into_inner();
            if req.run_id.trim().is_empty()
                || req.thread_id.trim().is_empty()
                || req.org_id.trim().is_empty()
                || req.schedule_id.trim().is_empty()
                || req.fire_key.trim().is_empty()
                || req.step_id.trim().is_empty()
                || req.idempotency_key.trim().is_empty()
                || !valid_sha256_digest(&req.template_digest)
                || !valid_sha256_digest(&req.policy_digest)
            {
                return Err(Status::invalid_argument(
                    "scheduled-step authority bindings are required",
                ));
            }
            if req.step_id != format!("{}:step:{}", req.run_id, req.step_index) {
                return Err(Status::invalid_argument("scheduled step_id is invalid"));
            }
            caller.authorize_org(&req.org_id)?;

            // Only a prepared, service-owned scheduled run can answer this
            // query. The exact metadata comparison below prevents a caller
            // from reusing a valid thread for another schedule or template.
            let row: Option<ScheduledStepAuthorityRow> = sqlx::query_as(
                "SELECT r.id AS run_id, r.thread_id, r.org_id,
                        COALESCE(t.space_id, '') AS space_id,
                        r.status AS run_status, r.metadata
                 FROM runs r
                 JOIN threads t ON t.id = r.thread_id AND t.org_id = r.org_id
                 WHERE r.id = $1 AND r.thread_id = $2 AND r.org_id = $3
                   AND r.user_id = $4
                   AND COALESCE(r.metadata->>'source', '') = 'scheduled_run'",
            )
            .bind(req.run_id.trim())
            .bind(req.thread_id.trim())
            .bind(req.org_id.trim())
            .bind(crate::auth::system_run_owners()[0])
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| {
                tracing::error!(%error, "scheduled-step authority lookup failed");
                Status::unavailable("scheduled-step authority unavailable")
            })?;

            let response = row
                .and_then(|row| project_scheduled_step_authority(row, &req))
                .unwrap_or_else(|| pb::ResolveScheduledStepAuthorityResponse {
                    resolved: false,
                    ..Default::default()
                });
            Ok(Response::new(response))
        }
        .await;
        record_metrics("resolve_scheduled_step_authority", started, result.is_ok());
        result
    }

    /// Org-scoped listing of runs with no human owner.
    ///
    /// `list_runs` is thread-scoped, and a system run lives in a thread its own
    /// workload owns, so no person's thread listing can reach it — the run would
    /// exist and be fully authorized yet be unreachable by construction. This is
    /// the only way to see them.
    ///
    /// Read-only by design. The owner set is bound from the same constant the
    /// authorization predicate reads (`auth::system_run_owners`), not a `LIKE
    /// 'service:%'` pattern, so a row this query returns is exactly a row
    /// [`authorize_owner_row`] would admit for reading.
    ///
    /// Maintenance runs only. A system run inside a Space is a schedule's run: it
    /// carries one person's goal text and result, written into their own Space,
    /// so it is not the org's to list (the same rule `authorize_thread_owner_row`
    /// applies to its thread). Its owner reaches it through the Space's own
    /// read-decision routes. Filtered in SQL so `has_more` and the cursor stay
    /// correct.
    async fn list_system_runs(
        &self,
        request: Request<pb::ListSystemRunsRequest>,
    ) -> Result<Response<pb::ListRunsResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::ListRunsResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:read")?;
            let req = request.into_inner();
            // The org boundary is absolute here exactly as everywhere else: a
            // system run is org-readable, never cross-org readable.
            caller.authorize_org(&req.org_id)?;

            let limit = clamp_limit(req.limit);
            let fetch = limit + 1;

            let owners: Vec<String> = crate::auth::system_run_owners()
                .iter()
                .map(|owner| (*owner).to_owned())
                .collect();

            let mut query = format!("{RUN_SELECT} WHERE r.org_id = $1");
            query.push_str(" AND r.user_id = ANY($2)");
            query.push_str(" AND COALESCE(r.space_id, '') = ''");
            query.push_str(
                " AND NOT EXISTS (SELECT 1 FROM threads st \
                  WHERE st.id = r.thread_id AND COALESCE(st.space_id, '') <> '')",
            );
            query.push_str(concat!(" AND ($3 = '' OR ", crate::run_phase::run_phase_sql!("r"), " = $3)"));
            query.push_str(" AND ($4 = '' OR r.id < $4)");
            query.push_str(" ORDER BY r.id DESC LIMIT $5");

            let rows: Vec<RunRow> = sqlx::query_as(&query)
                .bind(&req.org_id)
                .bind(&owners)
                .bind(&req.status_filter)
                .bind(&req.after_run_id)
                .bind(fetch)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;

            let has_more = i64::try_from(rows.len()).unwrap_or(i64::MAX) > limit;
            let store_enabled = crate::loop_checkpoint::enabled();
            let runs: Vec<pb::RunDetail> = rows
                .into_iter()
                .take(usize::try_from(limit).unwrap_or(usize::MAX))
                .map(|row| row_to_detail(row, store_enabled, &caller))
                .collect();
            Ok(Response::new(pb::ListRunsResponse { runs, has_more }))
        }
        .await;
        record_metrics("list_system_runs", started, result.is_ok());
        result
    }

    async fn list_runs(
        &self,
        request: Request<pb::ListRunsRequest>,
    ) -> Result<Response<pb::ListRunsResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::ListRunsResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:read")?;
            let req = request.into_inner();
            // Two ways to be allowed to list runs, and only two. Without a
            // Control decision this is unchanged: name a thread you own. With
            // one, the runs returned are those of the threads that decision
            // already admits — so the room's Work tab reaches exactly as far as
            // its Chat tab and no further.
            let space_read = crate::grpc::verify_thread_read_space_decision(
                &req.space_read_decision_token,
                &req.space_read_decision_ref,
                &caller.org_id(),
                caller.user_id().unwrap_or_default(),
                &req.space_id,
            )?;
            if space_read.is_none() && req.thread_id.is_empty() {
                return Err(Status::invalid_argument("thread_id is required"));
            }
            if space_read.is_none() {
                authorize_thread_owner(&self.pool, &caller, &req.thread_id, OwnerIntent::Read)
                    .await?;
            }

            let limit = clamp_limit(req.limit);
            // Fetch one extra row to compute `has_more` without a second query.
            let fetch = limit + 1;

            // Runs are ordered newest-first by ULID id (lexicographic ULID order
            // == creation order). The cursor pages strictly older than the last
            // id the caller saw. Optional status filter; empty = all statuses.
            //
            // The Space branch joins `threads` rather than filtering runs by a
            // column of their own: a run has no audience snapshot, so the only
            // honest way to decide whether this reader may see it is to ask
            // whether they may see the thread it belongs to.
            let audience_ceiling = space_read.map(|read| read.recipient_audience_revision);
            let mut query = String::from(RUN_SELECT);
            if audience_ceiling.is_some() {
                query.push_str(
                    " JOIN threads t ON t.id = r.thread_id
                     WHERE t.org_id = $5 AND t.space_id = $6
                       AND (t.recipient_audience_revision IS NULL
                            OR t.recipient_audience_revision <= $7)
                       AND ($1 = '' OR r.thread_id = $1)",
                );
            } else {
                query.push_str(" WHERE r.thread_id = $1");
            }
            query.push_str(concat!(" AND ($2 = '' OR ", crate::run_phase::run_phase_sql!("r"), " = $2)"));
            query.push_str(" AND ($3 = '' OR r.id < $3)");
            query.push_str(" ORDER BY r.id DESC LIMIT $4");

            let mut sql = sqlx::query_as(&query)
                .bind(&req.thread_id)
                .bind(&req.status_filter)
                .bind(&req.after_run_id)
                .bind(fetch);
            if let Some(ceiling) = audience_ceiling {
                sql = sql
                    .bind(caller.org_id())
                    .bind(&req.space_id)
                    .bind(ceiling);
            }
            let rows: Vec<RunRow> = sql
                .fetch_all(&self.pool)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;

            let has_more = i64::try_from(rows.len()).unwrap_or(i64::MAX) > limit;
            let store_enabled = crate::loop_checkpoint::enabled();
            let runs: Vec<pb::RunDetail> = rows
                .into_iter()
                .take(usize::try_from(limit).unwrap_or(usize::MAX))
                .map(|row| row_to_detail(row, store_enabled, &caller))
                .collect();

            Ok(Response::new(pb::ListRunsResponse { runs, has_more }))
        }
        .await;
        record_metrics("list_runs", started, result.is_ok());
        result
    }

    async fn cancel_run(
        &self,
        request: Request<pb::CancelRunRequest>,
    ) -> Result<Response<pb::CancelRunResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::CancelRunResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:write")?;
            let req = request.into_inner();
            if req.run_id.is_empty() {
                return Err(Status::invalid_argument("run_id is required"));
            }
            authorize_run_owner(&self.pool, &caller, &req.run_id, OwnerIntent::Mutate).await?;

            // Read the current status to reject a re-cancel of a terminal run
            // before flipping it. A missing run is a 404.
            let current: Option<(String,)> =
                sqlx::query_as("SELECT status FROM runs WHERE id = $1")
                    .bind(&req.run_id)
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(|e| Status::internal(e.to_string()))?;

            let (status,) = current
                .ok_or_else(|| Status::not_found(format!("run not found: {}", req.run_id)))?;

            if is_terminal(&status) {
                // Already terminal — not cancellable. Report not-cancelled rather
                // than erroring so a double-cancel is idempotent at the UI. If a
                // cancellation receipt already exists, return it so a retry can
                // still render the same durable proof instead of losing it.
                let receipt: Option<(String,)> = sqlx::query_as(
                    "SELECT id FROM events
                     WHERE run_id = $1 AND event_type = 'RUN_CANCELLED'
                     ORDER BY ts DESC, id DESC LIMIT 1",
                )
                .bind(&req.run_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
                return Ok(Response::new(pb::CancelRunResponse {
                    cancelled: false,
                    receipt_id: receipt.map_or_else(String::new, |(id,)| id),
                }));
            }

            let mut tx = self
                .pool
                .begin()
                .await
                .map_err(|e| Status::internal(e.to_string()))?;

            // The status transition and receipt are one transaction. A caller
            // can never observe a cancelled run without the event that proves
            // who cancelled it and why.
            let updated: Option<(String,)> = sqlx::query_as(
                "UPDATE runs
                 SET status = 'cancelled', ended_at = now(), updated_at = now()
                 WHERE id = $1 AND status NOT IN ('completed', 'failed', 'cancelled')
                 RETURNING id",
            )
            .bind(&req.run_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

            let Some((run_id,)) = updated else {
                // A concurrent terminalization won the race. Re-read the
                // receipt through the same service contract rather than
                // claiming that this request performed the cancellation.
                tx.rollback()
                    .await
                    .map_err(|e| Status::internal(e.to_string()))?;
                let receipt: Option<(String,)> = sqlx::query_as(
                    "SELECT id FROM events
                     WHERE run_id = $1 AND event_type = 'RUN_CANCELLED'
                     ORDER BY ts DESC, id DESC LIMIT 1",
                )
                .bind(&req.run_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
                return Ok(Response::new(pb::CancelRunResponse {
                    cancelled: false,
                    receipt_id: receipt.map_or_else(String::new, |(id,)| id),
                }));
            };

            // A cancelled run never resumes, so its loop fold is dropped in this
            // transaction (build plan WS03 S4).
            crate::loop_checkpoint::purge_run(&mut tx, &run_id)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;

            let receipt_id = new_ulid();
            let now = Utc::now();
            let idempotency_key = format!("run_cancelled:{run_id}");
            sqlx::query(
                "INSERT INTO events
                    (id, event_type, run_id, payload, ts, org_id, user_id,
                     correlation_id, causation_id, idempotency_key, resource_ref,
                     type_url, producer, schema_version)
                 SELECT $1, 'RUN_CANCELLED', r.id, $2, $3, r.org_id, r.user_id,
                        r.id, '', $4, $5, $6, 'session-core', 1
                 FROM runs AS r WHERE r.id = $7
                 ON CONFLICT (org_id, idempotency_key)
                 WHERE idempotency_key <> '' DO NOTHING",
            )
            .bind(&receipt_id)
            .bind(serde_json::json!({
                "run_id": &run_id,
                "reason": req.reason.trim(),
                "cancelled_by": caller.user_id().unwrap_or_else(|| caller.principal_id()),
            }))
            .bind(now)
            .bind(&idempotency_key)
            .bind(format!("run:{run_id}"))
            .bind(RUN_CANCELLED_TYPE_URL)
            .bind(&run_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

            tx.commit()
                .await
                .map_err(|e| Status::internal(e.to_string()))?;

            Ok(Response::new(pb::CancelRunResponse {
                cancelled: true,
                receipt_id,
            }))
        }
        .await;
        record_metrics("cancel_run", started, result.is_ok());
        result
    }

    async fn resolve_run_owner(
        &self,
        request: Request<pb::ResolveRunOwnerRequest>,
    ) -> Result<Response<pb::ResolveRunOwnerResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::ResolveRunOwnerResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:read")?;
            let req = request.into_inner();
            if req.run_id.trim().is_empty()
                || req.org_id.trim().is_empty()
                || req.user_id.trim().is_empty()
            {
                return Err(Status::invalid_argument(
                    "run_id, org_id, and user_id are required",
                ));
            }
            caller.authorize_org(&req.org_id)?;
            if !caller.is_service() {
                caller.authorize_user(&req.user_id)?;
            }
            // Owner decision 17: the owner's run answers `authorized` only when the caller's posture is
            // the run's receipt. A posture mismatch is a typed refusal, not a `false`, so the caller
            // reports why. A run that is not the caller's answers `false` without the receipt being read.
            let authorized = crate::retention_receipt::run_owner_matches(
                &self.pool,
                &caller,
                &req.run_id,
                &req.org_id,
                &req.user_id,
            )
            .await?;
            Ok(Response::new(pb::ResolveRunOwnerResponse { authorized }))
        }
        .await;
        record_metrics("resolve_run_owner", started, result.is_ok());
        result
    }

    async fn resolve_thread_owner(
        &self,
        request: Request<pb::ResolveThreadOwnerRequest>,
    ) -> Result<Response<pb::ResolveThreadOwnerResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::ResolveThreadOwnerResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:read")?;
            let req = request.into_inner();
            if req.thread_id.trim().is_empty()
                || req.org_id.trim().is_empty()
                || req.user_id.trim().is_empty()
            {
                return Err(Status::invalid_argument(
                    "thread_id, org_id, and user_id are required",
                ));
            }
            caller.authorize_org(&req.org_id)?;
            if !caller.is_service() {
                caller.authorize_user(&req.user_id)?;
            }
            let authorized = self
                .thread_owner_lookup
                .owner_matches(req.thread_id.trim(), req.org_id.trim(), req.user_id.trim())
                .await?;
            Ok(Response::new(pb::ResolveThreadOwnerResponse { authorized }))
        }
        .await;
        record_metrics("resolve_thread_owner", started, result.is_ok());
        result
    }

    async fn resolve_run_action_authority(
        &self,
        request: Request<pb::ResolveRunActionAuthorityRequest>,
    ) -> Result<Response<pb::ResolveRunActionAuthorityResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::ResolveRunActionAuthorityResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_run_action_authority_service(&caller)?;
            let req = request.into_inner();
            if req.run_id.trim().is_empty() || req.org_id.trim().is_empty() {
                return Err(Status::invalid_argument("run_id and org_id are required"));
            }
            caller.authorize_org(&req.org_id)?;

            // `runs` inherits its Space bindings from the owner-bound thread in
            // its creation transaction. Requiring the two complete projections
            // to remain equal makes a stale or partial legacy row unavailable
            // instead of letting a caller choose which context to trust.
            let row: Option<RunActionAuthorityRow> = sqlx::query_as(
                "SELECT r.id AS run_id, r.org_id, r.user_id AS subject_id,
                        r.thread_id, r.status AS run_status,
                        r.space_id, r.recipient_audience_ref, r.recipient_audience_revision,
                        r.recipient_audience_hash, r.privacy_policy_ref,
                        r.resource_authorization_ref AS thread_resource_authorization_ref,
                        r.authority_revision
                 FROM runs r
                 JOIN threads t
                   ON t.id = r.thread_id AND t.org_id = r.org_id AND t.user_id = r.user_id
                 WHERE r.id = $1 AND r.org_id = $2
                   AND r.space_id IS NOT DISTINCT FROM t.space_id
                   AND r.recipient_audience_ref IS NOT DISTINCT FROM t.recipient_audience_ref
                   AND r.recipient_audience_revision IS NOT DISTINCT FROM t.recipient_audience_revision
                   AND r.recipient_audience_hash IS NOT DISTINCT FROM t.recipient_audience_hash
                   AND r.privacy_policy_ref IS NOT DISTINCT FROM t.privacy_policy_ref
                   AND r.resource_authorization_ref IS NOT DISTINCT FROM t.resource_authorization_ref
                   AND r.authority_revision IS NOT DISTINCT FROM t.authority_revision",
            )
            .bind(&req.run_id)
            .bind(&req.org_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| {
                tracing::error!(%error, "run action authority lookup failed");
                Status::unavailable("run action authority unavailable")
            })?;

            let response = row
                .and_then(project_run_action_authority)
                .unwrap_or_else(unresolved_run_action_authority);
            Ok(Response::new(response))
        }
        .await;
        record_metrics("resolve_run_action_authority", started, result.is_ok());
        result
    }
}

/// Clamp a wire `limit` (0 = caller left it unset) into `[1, MAX_LIST_LIMIT]`.
#[allow(clippy::cast_possible_wrap)]
fn clamp_limit(raw: u32) -> i64 {
    let requested = if raw == 0 {
        DEFAULT_LIST_LIMIT
    } else {
        i64::from(raw)
    };
    requested.clamp(1, MAX_LIST_LIMIT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn clamp_limit_defaults_when_zero() {
        assert_eq!(clamp_limit(0), DEFAULT_LIST_LIMIT);
    }

    #[test]
    fn clamp_limit_caps_at_max() {
        assert_eq!(clamp_limit(10_000), MAX_LIST_LIMIT);
    }

    #[test]
    fn clamp_limit_passes_through_in_range() {
        assert_eq!(clamp_limit(25), 25);
    }

    #[test]
    fn is_terminal_covers_done_failed_cancelled() {
        assert!(is_terminal("completed"));
        assert!(is_terminal("failed"));
        assert!(is_terminal("cancelled"));
        assert!(!is_terminal("running"));
        assert!(!is_terminal("queued"));
        assert!(!is_terminal("awaiting_approval"));
    }

    fn scheduled_step_request() -> pb::ResolveScheduledStepAuthorityRequest {
        pb::ResolveScheduledStepAuthorityRequest {
            run_id: "run-1".to_owned(),
            thread_id: "thread-1".to_owned(),
            org_id: "org-1".to_owned(),
            schedule_id: "schedule-1".to_owned(),
            fire_key: "fire-1".to_owned(),
            template_digest: format!("sha256:{}", "a".repeat(64)),
            policy_digest: format!("sha256:{}", "b".repeat(64)),
            step_id: "run-1:step:0".to_owned(),
            step_index: 0,
            idempotency_key: "fire-1:step:0".to_owned(),
        }
    }

    fn scheduled_step_row() -> ScheduledStepAuthorityRow {
        ScheduledStepAuthorityRow {
            run_id: "run-1".to_owned(),
            thread_id: "thread-1".to_owned(),
            org_id: "org-1".to_owned(),
            space_id: "space-1".to_owned(),
            run_status: "running".to_owned(),
            metadata: serde_json::json!({
                "source": "scheduled_run",
                "schedule_id": "schedule-1",
                "fire_key": "fire-1",
                "space_id": "space-1",
                "subject_id": "user-1",
                "template_digest": format!("sha256:{}", "a".repeat(64)),
                "policy_digest": format!("sha256:{}", "b".repeat(64)),
                "idempotency_key": "fire-1:step:0",
            }),
        }
    }

    #[test]
    fn scheduled_step_authority_requires_exact_prepared_metadata() {
        let request = scheduled_step_request();
        let response = project_scheduled_step_authority(scheduled_step_row(), &request)
            .expect("exact prepared metadata must resolve");
        assert!(response.resolved);
        assert_eq!(response.subject_id, "user-1");
        assert_eq!(response.space_id, "space-1");
    }

    #[test]
    fn scheduled_step_authority_rejects_retargeted_fire_or_idempotency() {
        let mut request = scheduled_step_request();
        request.fire_key = "other-fire".to_owned();
        assert!(project_scheduled_step_authority(scheduled_step_row(), &request).is_none());

        let mut request = scheduled_step_request();
        request.idempotency_key = "other-idempotency".to_owned();
        assert!(project_scheduled_step_authority(scheduled_step_row(), &request).is_none());
    }

    fn valid_run_action_authority_row() -> RunActionAuthorityRow {
        RunActionAuthorityRow {
            run_id: "run_1".to_owned(),
            org_id: "org_1".to_owned(),
            subject_id: "user_1".to_owned(),
            thread_id: "thread_1".to_owned(),
            run_status: "running".to_owned(),
            space_id: Some("space_1".to_owned()),
            recipient_audience_ref: Some("audience_1".to_owned()),
            recipient_audience_revision: Some(3),
            recipient_audience_hash: Some("sha256:audience".to_owned()),
            privacy_policy_ref: Some("privacy_1".to_owned()),
            thread_resource_authorization_ref: Some("thread-resource_1".to_owned()),
            authority_revision: Some(7),
        }
    }

    #[test]
    fn run_action_authority_projection_is_content_free_and_complete() {
        let response = project_run_action_authority(valid_run_action_authority_row())
            .expect("complete active human run is resolvable");
        assert!(response.resolved);
        assert_eq!(response.run_id, "run_1");
        assert_eq!(response.subject_id, "user_1");
        assert_eq!(
            response.thread_resource_authorization_ref,
            "thread-resource_1"
        );
        assert_eq!(response.authority_revision, 7);
        // The response contract intentionally has no goal, transcript, tool
        // input, output, approval, or bearer field to accidentally retain.
        assert_eq!(response.run_status, "running");
    }

    #[test]
    fn run_action_authority_projection_fails_closed_for_terminal_system_or_partial_rows() {
        let mut terminal = valid_run_action_authority_row();
        terminal.run_status = "completed".to_owned();
        assert!(project_run_action_authority(terminal).is_none());

        let mut system = valid_run_action_authority_row();
        system.subject_id = "service:orchestrator-core".to_owned();
        assert!(project_run_action_authority(system).is_none());

        let mut partial = valid_run_action_authority_row();
        partial.recipient_audience_revision = Some(0);
        assert!(project_run_action_authority(partial).is_none());
    }

    #[test]
    fn row_to_detail_maps_columns_and_derives_counters() {
        let row = RunRow {
            id: "run_01".to_owned(),
            thread_id: "thr_01".to_owned(),
            parent_run_id: None,
            agent_id: "general-v1".to_owned(),
            status: "running".to_owned(),
            mode: "execute".to_owned(),
            goal: "do the thing".to_owned(),
            final_output: None,
            error: None,
            metadata: serde_json::json!({"source": "run_start"}),
            created_at: 1_700_000_000,
            updated_at: 1_700_000_050,
            steps_completed: 3,
            checkpoint_index: 2,
            retention_class: "persistent".to_owned(),
            owner_user_id: "owner_1".to_owned(),
            fold_state: None,
            fold_kind: None,
            fold_claim_expired: None,
            fold_park_reason: None,
            fold_pause_consumed: None,
        };
        let detail = row_to_detail(row, false, &owner_viewer());
        assert_eq!(detail.run_id, "run_01");
        assert_eq!(detail.thread_id, "thr_01");
        assert_eq!(detail.parent_run_id, "");
        assert_eq!(detail.steps_completed, 3);
        assert_eq!(detail.checkpoint_index, 2);
        assert_eq!(detail.input_tokens, 0);
        assert_eq!(detail.output_tokens, 0);
        assert_eq!(detail.created_at.unwrap().seconds, 1_700_000_000);
        assert!(detail.metadata.is_some());
    }

    #[test]
    fn row_to_detail_flattens_optional_text_to_empty() {
        let row = RunRow {
            id: "run_02".to_owned(),
            thread_id: "thr_02".to_owned(),
            parent_run_id: Some("run_parent".to_owned()),
            agent_id: "general-v1".to_owned(),
            status: "completed".to_owned(),
            mode: "plan".to_owned(),
            goal: "g".to_owned(),
            final_output: Some("the answer".to_owned()),
            error: None,
            metadata: serde_json::Value::Null,
            created_at: 1,
            updated_at: 2,
            steps_completed: 0,
            checkpoint_index: 0,
            retention_class: "persistent".to_owned(),
            owner_user_id: "owner_1".to_owned(),
            fold_state: None,
            fold_kind: None,
            fold_claim_expired: None,
            fold_park_reason: None,
            fold_pause_consumed: None,
        };
        let detail = row_to_detail(row, false, &owner_viewer());
        assert_eq!(detail.parent_run_id, "run_parent");
        assert_eq!(detail.final_output, "the answer");
        assert_eq!(detail.error, "");
        // Null JSONB metadata maps to no Struct rather than an empty one.
        assert!(detail.metadata.is_none());
        // A run with no fold is not resumable and names no park reason.
        assert!(!detail.resumable);
        assert_eq!(detail.park_reason, "");
    }

    fn fold_row(
        status: &str,
        retention_class: &str,
        fold_state: Option<&str>,
        fold_kind: Option<&str>,
        fold_claim_expired: Option<bool>,
        fold_park_reason: Option<&str>,
    ) -> RunRow {
        RunRow {
            id: "run_f".to_owned(),
            thread_id: "thr_f".to_owned(),
            parent_run_id: None,
            agent_id: "general-v1".to_owned(),
            status: status.to_owned(),
            mode: "execute".to_owned(),
            goal: "g".to_owned(),
            final_output: None,
            error: None,
            metadata: serde_json::Value::Null,
            created_at: 1,
            updated_at: 2,
            steps_completed: 0,
            checkpoint_index: 0,
            retention_class: retention_class.to_owned(),
            owner_user_id: "owner_1".to_owned(),
            fold_state: fold_state.map(str::to_owned),
            fold_kind: fold_kind.map(str::to_owned),
            fold_claim_expired,
            fold_park_reason: fold_park_reason.map(str::to_owned),
            fold_pause_consumed: None,
        }
    }

    /// The run owner `fold_row` builds its rows for.
    fn owner_viewer() -> VerifiedIdentity {
        VerifiedIdentity::user_for_test("org_1", "owner_1")
    }

    #[test]
    fn loop_checkpoint_resumable_is_true_only_for_a_claimable_persistent_unended_fold_while_the_switch_is_on(
    ) {
        // (fold state, fold kind, claim lapsed, claimable). The stored reason is
        // supplied for every park-kind row, as the database holds it: a claim moves
        // the state to `resuming` but leaves the kind and the reason in place.
        let folds: [(Option<&str>, Option<&str>, Option<bool>, bool); 11] = [
            (None, None, None, false),
            (Some("active"), Some("round"), Some(false), false),
            (Some("paused"), Some("pause"), Some(false), true),
            (Some("parked"), Some("park"), Some(false), true),
            (Some("resuming"), Some("park"), Some(false), false),
            (Some("resuming"), Some("park"), Some(true), true),
            (Some("resuming"), Some("pause"), Some(true), true),
            (Some("resuming"), Some("park"), None, false),
            (Some("closed"), Some("park"), Some(false), false),
            (Some("active"), Some("park"), Some(true), false),
            (Some("resuming"), None, Some(true), true),
        ];
        for store_enabled in [false, true] {
            for retention in ["persistent", "zdr", "zdr_waived"] {
                for status in ["queued", "running", "awaiting_approval", "completed", "failed", "cancelled"] {
                    for (state, kind, expired, claimable) in folds {
                        let reason = (kind == Some("park")).then_some("provider_exhausted");
                        let detail = row_to_detail(
                            fold_row(status, retention, state, kind, expired, reason),
                            store_enabled,
                            &owner_viewer(),
                        );
                        let expected = store_enabled
                            && retention == "persistent"
                            && !matches!(status, "completed" | "failed" | "cancelled")
                            && claimable;
                        let case =
                            format!("{store_enabled} {retention} {status} {state:?} {kind:?} {expired:?}");
                        assert_eq!(detail.resumable, expected, "{case}");
                        let expected_reason = if expected && kind == Some("park") {
                            "provider_exhausted"
                        } else {
                            ""
                        };
                        assert_eq!(detail.park_reason, expected_reason, "{case}");
                    }
                }
            }
        }
    }

    #[test]
    fn loop_checkpoint_a_paused_fold_is_resumable_with_no_park_reason() {
        // A stray park reason on a pause-kind row is never surfaced.
        let detail = row_to_detail(
            fold_row(
                "awaiting_approval",
                "persistent",
                Some("paused"),
                Some("pause"),
                Some(false),
                Some("credential_window"),
            ),
            true,
            &owner_viewer(),
        );
        assert!(detail.resumable);
        assert_eq!(detail.park_reason, "");
    }

    #[test]
    fn loop_checkpoint_a_parked_fold_keeps_its_reason_after_its_resume_lease_lapses() {
        // claim() moves a parked fold to `resuming` and leaves kind and reason in
        // place. Once that lease lapses the run is resumable again, and it must
        // still say why it was parked: the reason follows the kind, not the state.
        let lapsed = row_to_detail(
            fold_row(
                "running",
                "persistent",
                Some("resuming"),
                Some("park"),
                Some(true),
                Some("credential_window"),
            ),
            true,
            &owner_viewer(),
        );
        assert!(lapsed.resumable);
        assert_eq!(lapsed.park_reason, "credential_window");

        // While the lease is live the run is not resumable and names no reason.
        let live = row_to_detail(
            fold_row(
                "running",
                "persistent",
                Some("resuming"),
                Some("park"),
                Some(false),
                Some("credential_window"),
            ),
            true,
            &owner_viewer(),
        );
        assert!(!live.resumable);
        assert_eq!(live.park_reason, "");
    }

    #[test]
    fn loop_checkpoint_a_park_reason_outside_the_closed_set_is_never_surfaced() {
        // The column is written only through `save`, which refuses anything but a
        // closed token. The read side does not rely on that: a value that got in some
        // other way (an old build, a manual write) is dropped, not copied to the wire.
        for stored in [
            "free text about a user",
            "token=abc",
            "CREDENTIAL_WINDOW",
            "credential_window ",
            " credential_window",
            "",
        ] {
            let detail = row_to_detail(
                fold_row(
                    "running",
                    "persistent",
                    Some("parked"),
                    Some("park"),
                    Some(false),
                    Some(stored),
                ),
                true,
                &owner_viewer(),
            );
            assert!(detail.resumable, "{stored:?}: the run is still resumable");
            assert_eq!(detail.park_reason, "", "{stored:?}");
        }
        for reason in ParkReason::ALL {
            let detail = row_to_detail(
                fold_row(
                    "running",
                    "persistent",
                    Some("parked"),
                    Some("park"),
                    Some(false),
                    Some(reason.as_str()),
                ),
                true,
                &owner_viewer(),
            );
            assert_eq!(detail.park_reason, reason.as_str());
        }
    }

    #[test]
    fn loop_checkpoint_park_reason_is_read_only_by_the_owner_or_a_service() {
        // park_reason can reveal a delegated-credential window, so reading a run's
        // status is not enough to read it. resumable is not secret: model-gateway
        // reads it as a service caller and a peer's UI may show a resume button.
        let parked = |owner: &str| {
            let mut row = fold_row(
                "running",
                "persistent",
                Some("parked"),
                Some("park"),
                Some(false),
                Some("credential_window"),
            );
            row.owner_user_id = owner.to_owned();
            row
        };
        let viewers: [(&str, &str, VerifiedIdentity, bool); 6] = [
            ("the owner", "owner_1", owner_viewer(), true),
            (
                "another member of the org",
                "owner_1",
                VerifiedIdentity::user_for_test("org_1", "peer_1"),
                false,
            ),
            (
                "a service principal",
                "owner_1",
                VerifiedIdentity::service_for_test("org_1", &["session:read"], false),
                true,
            ),
            (
                "a person reading a system run",
                "service:orchestrator-core",
                VerifiedIdentity::user_for_test("org_1", "peer_1"),
                false,
            ),
            (
                "the owning workload reading its own system run",
                "service:orchestrator-core",
                VerifiedIdentity::service_for_test_as(
                    "org_1",
                    "service:orchestrator-core",
                    &["session:read"],
                    false,
                ),
                true,
            ),
            (
                "a person whose id is empty",
                "",
                VerifiedIdentity::user_for_test("org_1", ""),
                false,
            ),
        ];
        for (who, owner, viewer, sees_reason) in &viewers {
            let detail = row_to_detail(parked(owner), true, viewer);
            assert!(detail.resumable, "{who}: resumable is never blanked");
            let expected = if *sees_reason { "credential_window" } else { "" };
            assert_eq!(detail.park_reason, expected, "{who}");
        }
    }

    #[test]
    fn loop_checkpoint_fold_is_claimable_matches_the_claim_gate() {
        // The claim gate in loop_checkpoint::claim: paused or parked, or resuming
        // with a lapsed lease. The stale-active branch is gone, so active is never
        // claimable however old.
        assert!(fold_is_claimable(Some("paused"), false));
        assert!(fold_is_claimable(Some("parked"), false));
        assert!(fold_is_claimable(Some("resuming"), true));
        assert!(!fold_is_claimable(Some("resuming"), false));
        assert!(!fold_is_claimable(Some("active"), false));
        assert!(!fold_is_claimable(Some("active"), true));
        assert!(!fold_is_claimable(Some("closed"), true));
        assert!(!fold_is_claimable(None, false));
        assert!(!fold_is_claimable(Some("unknown-state"), true));
    }

    #[test]
    fn loop_checkpoint_run_select_projects_no_fold_content() {
        for forbidden in ["ciphertext", "fold_digest", "authority_digest", "claim_token_hash"] {
            assert!(
                !RUN_SELECT.contains(forbidden),
                "RUN_SELECT must not project {forbidden}"
            );
        }
        // What it does project from the fold is the content-free flag inputs.
        for allowed in ["lc.state", "lc.kind", "lc.claim_expires_at", "lc.park_reason"] {
            assert!(RUN_SELECT.contains(allowed), "{allowed}");
        }
        // The Space branch of ListRuns appends a plain JOIN right after the FROM,
        // so the LEFT JOIN must be part of the shared projection and come first.
        assert!(RUN_SELECT.trim_end().ends_with("LEFT JOIN run_loop_checkpoints lc ON lc.run_id = r.id"));
    }

    // ------------------------------------------------------------------
    // S7c-A3 (D5): a pause fold whose approval has been consumed is not resumable.
    //
    // A stage-1 continuation consumes the pause's approval and no longer closes the
    // pause fold, so the fold stays `paused` and claimable although no continuation
    // can use it any more. The predicate below stops `RunDetail.resumable`
    // advertising it. `resumable` stays readable by every reader (it is a boolean
    // and never reaches a browser); `park_reason` stays owner-or-service.
    // ------------------------------------------------------------------

    fn consumed_pause(consumed: Option<bool>) -> RunRow {
        let mut row = fold_row(
            "awaiting_approval",
            "persistent",
            Some("paused"),
            Some("pause"),
            Some(false),
            None,
        );
        row.fold_pause_consumed = consumed;
        row
    }

    #[test]
    fn a_pause_fold_whose_approval_was_consumed_is_not_resumable() {
        for state in [("paused", Some(false)), ("resuming", Some(true))] {
            let mut row = fold_row(
                "awaiting_approval",
                "persistent",
                Some(state.0),
                Some("pause"),
                state.1,
                None,
            );
            row.fold_pause_consumed = Some(true);
            let detail = row_to_detail(row, true, &owner_viewer());
            assert!(
                !detail.resumable,
                "{state:?}: a consumed pause approval leaves nothing a continuation can use"
            );
            assert_eq!(detail.park_reason, "");
        }
        // The same answer for every reader: it is not blanked for one and shown to
        // another, and it never turns true for a peer or a service.
        for viewer in [
            owner_viewer(),
            VerifiedIdentity::user_for_test("org_1", "someone_else"),
            VerifiedIdentity::service_for_test("org_1", &["session:read"], false),
        ] {
            let detail = row_to_detail(consumed_pause(Some(true)), true, &viewer);
            assert!(!detail.resumable);
        }
    }

    #[test]
    fn a_pause_fold_with_an_unconsumed_approval_stays_resumable() {
        // The probe says unconsumed, or says nothing (no approval row matched, which
        // is the gateway's old behaviour: a wrong key costs a fallback, never safety).
        for consumed in [Some(false), None] {
            let detail = row_to_detail(consumed_pause(consumed), true, &owner_viewer());
            assert!(detail.resumable, "{consumed:?}");
            assert_eq!(detail.park_reason, "");
        }
        // Every reader sees the boolean.
        for viewer in [
            VerifiedIdentity::user_for_test("org_1", "someone_else"),
            VerifiedIdentity::service_for_test("org_1", &["session:read"], false),
        ] {
            assert!(row_to_detail(consumed_pause(Some(false)), true, &viewer).resumable);
        }
        // The old gates still apply on top: a switch that is off, an ended run, a
        // zero-retention run and a live lease are not resumable whatever the probe says.
        assert!(!row_to_detail(consumed_pause(Some(false)), false, &owner_viewer()).resumable);
        let mut ended = consumed_pause(Some(false));
        ended.status = "completed".to_owned();
        assert!(!row_to_detail(ended, true, &owner_viewer()).resumable);
        let mut zdr = consumed_pause(Some(false));
        zdr.retention_class = "zdr".to_owned();
        assert!(!row_to_detail(zdr, true, &owner_viewer()).resumable);
        let mut leased = consumed_pause(Some(false));
        leased.fold_state = Some("resuming".to_owned());
        leased.fold_claim_expired = Some(false);
        assert!(!row_to_detail(leased, true, &owner_viewer()).resumable);
    }

    #[test]
    fn a_park_fold_ignores_the_consumed_approval_rule() {
        // The probe is only ever computed for a pause fold, but the rule must not
        // depend on that: whatever it carries, a park fold is judged by the old gate.
        for consumed in [None, Some(false), Some(true)] {
            let mut row = fold_row(
                "awaiting_approval",
                "persistent",
                Some("parked"),
                Some("park"),
                Some(false),
                Some("provider_exhausted"),
            );
            row.fold_pause_consumed = consumed;
            let detail = row_to_detail(row, true, &owner_viewer());
            assert!(detail.resumable, "{consumed:?}");
            assert_eq!(detail.park_reason, "provider_exhausted", "{consumed:?}");
        }
        // A round or pre-effect fold or no fold at all is judged by the old gate too.
        for kind in [Some("round"), Some("pre_effect"), None] {
            let mut row = fold_row("running", "persistent", Some("resuming"), kind, Some(true), None);
            row.fold_pause_consumed = Some(true);
            assert!(row_to_detail(row, true, &owner_viewer()).resumable, "{kind:?}");
        }
        // The park-reason rule is the one it always was: owner or service only.
        let mut parked = fold_row(
            "awaiting_approval",
            "persistent",
            Some("parked"),
            Some("park"),
            Some(false),
            Some("credential_window"),
        );
        parked.fold_pause_consumed = Some(false);
        let peer = VerifiedIdentity::user_for_test("org_1", "someone_else");
        let detail = row_to_detail(parked, true, &peer);
        assert!(detail.resumable);
        assert_eq!(detail.park_reason, "", "a peer still gets no reason");
    }

    #[test]
    fn run_select_keeps_its_shape_with_the_consumed_probe() {
        let sql = RUN_SELECT.split_whitespace().collect::<Vec<_>>().join(" ");
        let probe = "CASE WHEN lc.kind = 'pause' THEN EXISTS ( SELECT 1 FROM approvals ap \
                     WHERE ap.org_id = r.org_id AND ap.user_id = r.user_id AND ap.run_id = r.id \
                     AND ap.idempotency_key = lc.pause_ref AND ap.consumed_at IS NOT NULL ) \
                     END AS fold_pause_consumed";
        assert!(sql.contains(probe), "{sql}");
        // It is a column of the projection, before the FROM, and the shared LEFT JOIN
        // is still the last line, so the Space branch's appended JOIN still composes.
        assert!(sql.find("END AS fold_pause_consumed").unwrap() < sql.find("FROM runs r").unwrap());
        assert!(RUN_SELECT.trim_end().ends_with("LEFT JOIN run_loop_checkpoints lc ON lc.run_id = r.id"));
        // The alias `ap` is the probe's alone: the aliases r, lc, e, c, t and st are
        // taken by the projection and by the appended Space and system-run clauses.
        let source = include_str!("run_service_grpc.rs").replace("\r\n", "\n");
        let production = &source[..source.find("\n#[cfg(test)]\nmod tests {").expect("tests marker")];
        assert_eq!(production.matches("approvals ap").count(), 1);
        assert_eq!(RUN_SELECT.matches("ap.").count(), 5, "the five predicates of the probe");
        // It reads only a flag and still projects no fold content.
        for forbidden in ["ciphertext", "fold_digest", "authority_digest", "claim_token_hash"] {
            assert!(!RUN_SELECT.contains(forbidden), "{forbidden}");
        }
        // Scoped by org, user and run so the user-led unique index can serve it, and
        // the probe is confined to a pause fold by the CASE.
        for scoped in ["ap.org_id = r.org_id", "ap.user_id = r.user_id", "ap.run_id = r.id"] {
            assert!(RUN_SELECT.contains(scoped), "{scoped}");
        }
        // The pause approval is keyed by the pause reference (run id and step id).
        assert!(RUN_SELECT.contains("ap.idempotency_key = lc.pause_ref"));
        // Composes with the Space branch's appended JOIN, which names t and st only.
        let composed = format!("{RUN_SELECT} JOIN threads t ON t.id = r.thread_id WHERE t.org_id = $5");
        assert!(composed.contains("LEFT JOIN run_loop_checkpoints lc ON lc.run_id = r.id JOIN threads t"));
    }

    /// Turns the fold store on for one test and off again afterwards, even when
    /// the test panics. Tests that hold it run with `--test-threads=1`.
    struct StoreOn;

    impl StoreOn {
        fn new() -> Self {
            std::env::set_var("SESSION_CORE_LOOP_CHECKPOINT_ENABLED", "true");
            Self
        }
    }

    impl Drop for StoreOn {
        fn drop(&mut self) {
            std::env::remove_var("SESSION_CORE_LOOP_CHECKPOINT_ENABLED");
        }
    }

    async fn get_run_as(pool: &PgPool, run_id: &str, org: &str, user: &str) -> pb::RunDetail {
        RunServiceImpl::new(pool.clone())
            .get_run(identity_request(
                pb::GetRunRequest {
                    run_id: run_id.to_owned(),
                },
                VerifiedIdentity::user_for_test(org, user),
            ))
            .await
            .expect("get run")
            .into_inner()
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres; sets the fold store switch, so run with --test-threads=1"]
    async fn loop_checkpoint_get_run_reports_a_parked_fold_as_resumable_with_its_reason() {
        use crate::loop_checkpoint::test_support::{cipher, scratch_pool, seed_fold, seed_run};
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let _on = StoreOn::new();
        let until = chrono::Utc::now() + chrono::Duration::hours(1);

        // Paused for an approval: resumable, and no park reason.
        let (paused, org, user) = seed_run(&pool, "persistent").await;
        seed_fold(&pool, &paused, "pause", "appr_1", "", None)
            .await
            .expect("pause fold");
        let detail = get_run_as(&pool, &paused, &org, &user).await;
        assert!(detail.resumable);
        assert_eq!(detail.park_reason, "");

        // Parked: resumable, with the stored closed token.
        let (parked, org, user) = seed_run(&pool, "persistent").await;
        seed_fold(&pool, &parked, "park", "appr_1", "provider_exhausted", Some(until))
            .await
            .expect("park fold");
        let detail = get_run_as(&pool, &parked, &org, &user).await;
        assert!(detail.resumable);
        assert_eq!(detail.park_reason, "provider_exhausted");

        // The listing carries the same flag, through the same projection.
        let thread_id: String = sqlx::query_scalar("SELECT thread_id FROM runs WHERE id = $1")
            .bind(&parked)
            .fetch_one(&pool)
            .await
            .expect("thread");
        let listed = RunServiceImpl::new(pool.clone())
            .list_runs(identity_request(
                pb::ListRunsRequest {
                    thread_id,
                    limit: 10,
                    ..Default::default()
                },
                VerifiedIdentity::user_for_test(&org, &user),
            ))
            .await
            .expect("list runs")
            .into_inner();
        assert_eq!(listed.runs.len(), 1);
        assert!(listed.runs[0].resumable);
        assert_eq!(listed.runs[0].park_reason, "provider_exhausted");

        // Resuming under a live lease: someone holds it, so it is not resumable.
        let (resuming, org, user) = seed_run(&pool, "persistent").await;
        seed_fold(&pool, &resuming, "park", "appr_1", "credential_window", Some(until))
            .await
            .expect("park fold");
        crate::loop_checkpoint::claim(&pool, &cipher(), &resuming, "appr_1", "token-a")
            .await
            .expect("claim");
        let detail = get_run_as(&pool, &resuming, &org, &user).await;
        assert!(!detail.resumable);
        assert_eq!(detail.park_reason, "");
        // Once the lease lapses it is resumable again.
        sqlx::query(
            "UPDATE run_loop_checkpoints SET claim_expires_at = now() - interval '1 second' WHERE run_id = $1",
        )
        .bind(&resuming)
        .execute(&pool)
        .await
        .expect("lapse the lease");
        let detail = get_run_as(&pool, &resuming, &org, &user).await;
        assert!(detail.resumable);
        assert_eq!(detail.park_reason, "credential_window");

        // An active fold belongs to a loop that is still running: not resumable.
        let (active, org, user) = seed_run(&pool, "persistent").await;
        seed_fold(&pool, &active, "round", "", "", None)
            .await
            .expect("round fold");
        assert!(!get_run_as(&pool, &active, &org, &user).await.resumable);

        // A row that outlived its run (written before the end-of-run guard) is not
        // resumable either.
        let (ended, org, user) = seed_run(&pool, "persistent").await;
        seed_fold(&pool, &ended, "pause", "appr_1", "", None)
            .await
            .expect("pause fold");
        crate::loop_checkpoint::test_support::end_run_without_purge(&pool, &ended).await;
        assert!(!get_run_as(&pool, &ended, &org, &user).await.resumable);

        // Switch off: nothing is resumable, whatever the stores hold.
        drop(_on);
        let detail = get_run_as(&pool, &parked, &org_of(&pool, &parked).await, "u1").await;
        assert!(!detail.resumable);
        assert_eq!(detail.park_reason, "");
    }

    async fn org_of(pool: &PgPool, run_id: &str) -> String {
        sqlx::query_scalar("SELECT org_id FROM runs WHERE id = $1")
            .bind(run_id)
            .fetch_one(pool)
            .await
            .expect("org")
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres; sets the fold store switch, so run with --test-threads=1"]
    async fn loop_checkpoint_get_run_never_reports_a_zdr_run_as_resumable() {
        use crate::loop_checkpoint::test_support::{scratch_pool, seed_run};
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let _on = StoreOn::new();
        for retention in ["zdr", "zdr_waived"] {
            let (run_id, org, user) = seed_run(&pool, retention).await;
            // `save` refuses these runs, so the row is inserted directly to prove the
            // read side is closed on its own: the fold is never reported.
            sqlx::query(
                "INSERT INTO run_loop_checkpoints
                    (run_id, org_id, user_id, seq, kind, state, pause_ref, park_reason, park_until,
                     authority_digest, segment_no, ciphertext, fold_digest)
                 VALUES ($1, $2, $3, 1, 'park', 'parked', 'appr_1', 'provider_exhausted',
                         now() + interval '1 hour', 'auth-1', 1, 'not-a-real-fold', repeat('a', 64))",
            )
            .bind(&run_id)
            .bind(&org)
            .bind(&user)
            .execute(&pool)
            .await
            .expect("insert a fold for a non-persistent run");

            let detail = get_run_as(&pool, &run_id, &org, &user).await;
            assert!(!detail.resumable, "{retention}");
            assert_eq!(detail.park_reason, "", "{retention}");
        }
    }

    /// A loop approval for `run_id` keyed by `idempotency_key` (the pause
    /// reference of a fold), granted, with a retained descriptor so it can be
    /// consumed. Needs the descriptor key in the environment.
    async fn granted_pause_approval(
        pool: &PgPool,
        run_id: &str,
        org: &str,
        user: &str,
        idempotency_key: &str,
    ) -> String {
        use base64::Engine as _;
        std::env::set_var(
            "SESSION_CORE_CONTINUATION_DESCRIPTOR_KEY",
            base64::engine::general_purpose::STANDARD.encode([7_u8; 32]),
        );
        let id = format!("appr_{}", mp_ids::new_ulid());
        let descriptor = serde_json::json!({
            "version": 1,
            "run_id": run_id,
            "org_id": org,
            "user_id": user,
            "step_id": "tool_1_call-1",
            "action_kind": "tool_call",
            "action_fingerprint": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
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
        crate::orchestration_store::request_approval(
            pool,
            &id,
            run_id,
            None,
            "tool_call",
            user,
            org,
            user,
            idempotency_key,
            &serde_json::json!({}),
            Some(&descriptor),
            Some(chrono::Utc::now() + chrono::Duration::minutes(30)),
            "loop",
        )
        .await
        .expect("request approval")
        .expect("inserted");
        crate::orchestration_store::decide_approval(pool, &id, org, Some(user), "granted", user, "ok")
            .await
            .expect("grant");
        id
    }

    /// The Space branch of `ListRuns` appends a plain JOIN to the shared projection.
    /// Its decision token cannot be forged here, so this composes the same SQL and
    /// reads the row the same way, to prove the probe resolves under that JOIN.
    async fn space_branch_row(pool: &PgPool, run_id: &str, org: &str) -> RunRow {
        sqlx::query_as::<_, RunRow>(&format!(
            "{RUN_SELECT} JOIN threads t ON t.id = r.thread_id \
             WHERE t.org_id = $1 AND r.id = $2"
        ))
        .bind(org)
        .bind(run_id)
        .fetch_one(pool)
        .await
        .expect("the Space-branch shape resolves the probe")
    }

    // S7c-A3 (D5). WRITTEN, NOT RUN by the author (disposable Postgres, lead).
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres; sets the fold store switch, so run with --test-threads=1"]
    async fn loop_checkpoint_a_stale_pause_fold_after_a_stage_one_continuation_reads_not_resumable() {
        use crate::loop_checkpoint::test_support::{scratch_pool, seed_fold, seed_run};
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let _on = StoreOn::new();
        let until = chrono::Utc::now() + chrono::Duration::hours(1);

        // The paused run: a pause fold keyed by run:step and the approval keyed the same.
        let (run_id, org, user) = seed_run(&pool, "persistent").await;
        let pause_ref = format!("{run_id}:step-1");
        seed_fold(&pool, &run_id, "pause", &pause_ref, "", None)
            .await
            .expect("pause fold");
        let approval = granted_pause_approval(&pool, &run_id, &org, &user, &pause_ref).await;
        let thread_id: String = sqlx::query_scalar("SELECT thread_id FROM runs WHERE id = $1")
            .bind(&run_id)
            .fetch_one(&pool)
            .await
            .expect("thread");
        let list_thread = |pool: PgPool, thread_id: String, org: String, user: String| async move {
            RunServiceImpl::new(pool)
                .list_runs(identity_request(
                    pb::ListRunsRequest {
                        thread_id,
                        limit: 10,
                        ..Default::default()
                    },
                    VerifiedIdentity::user_for_test(&org, &user),
                ))
                .await
                .expect("list runs")
                .into_inner()
        };

        // Granted and unconsumed: resumable on every read path.
        assert!(get_run_as(&pool, &run_id, &org, &user).await.resumable);
        let listed = list_thread(pool.clone(), thread_id.clone(), org.clone(), user.clone()).await;
        assert!(listed.runs[0].resumable);
        assert_eq!(
            space_branch_row(&pool, &run_id, &org).await.fold_pause_consumed,
            Some(false)
        );

        // A stage-one continuation consumes the approval and leaves the fold in place.
        assert!(
            crate::orchestration_store::consume_approval(
                &pool, &approval, &org, &run_id, &user, "", "claim-a"
            )
            .await
            .expect("consume")
            .expect("outcome")
            .consumed
        );
        let state: String = sqlx::query_scalar("SELECT state FROM run_loop_checkpoints WHERE run_id = $1")
            .bind(&run_id)
            .fetch_one(&pool)
            .await
            .expect("fold state");
        assert_eq!(state, "paused", "the stale fold is still there and still claimable by state");

        // Now no read path advertises it.
        let detail = get_run_as(&pool, &run_id, &org, &user).await;
        assert!(!detail.resumable);
        assert_eq!(detail.park_reason, "");
        let listed = list_thread(pool.clone(), thread_id, org.clone(), user.clone()).await;
        assert!(!listed.runs[0].resumable);
        let space_row = space_branch_row(&pool, &run_id, &org).await;
        assert_eq!(space_row.fold_pause_consumed, Some(true));
        assert!(!row_to_detail(space_row, true, &VerifiedIdentity::user_for_test(&org, &user)).resumable);

        // A consumed approval under another key does not mark a pause fold keyed
        // differently: the pairing is the pause reference, run and user.
        let (other_run, other_org, other_user) = seed_run(&pool, "persistent").await;
        seed_fold(&pool, &other_run, "pause", &format!("{other_run}:step-9"), "", None)
            .await
            .expect("pause fold");
        let unrelated =
            granted_pause_approval(&pool, &other_run, &other_org, &other_user, "some-other-key").await;
        assert!(
            crate::orchestration_store::consume_approval(
                &pool, &unrelated, &other_org, &other_run, &other_user, "", "claim-a"
            )
            .await
            .expect("consume")
            .expect("outcome")
            .consumed
        );
        assert!(get_run_as(&pool, &other_run, &other_org, &other_user).await.resumable);

        // A park fold is judged by the old gate, whatever approvals the run has consumed.
        let (parked, parked_org, parked_user) = seed_run(&pool, "persistent").await;
        let park_ref = format!("{parked}:step-2");
        seed_fold(&pool, &parked, "park", &park_ref, "provider_exhausted", Some(until))
            .await
            .expect("park fold");
        let keyed = granted_pause_approval(&pool, &parked, &parked_org, &parked_user, &park_ref).await;
        assert!(
            crate::orchestration_store::consume_approval(
                &pool, &keyed, &parked_org, &parked, &parked_user, "", "claim-a"
            )
            .await
            .expect("consume")
            .expect("outcome")
            .consumed
        );
        let detail = get_run_as(&pool, &parked, &parked_org, &parked_user).await;
        assert!(detail.resumable);
        assert_eq!(detail.park_reason, "provider_exhausted");
    }

    async fn park_reason_from_get_run(
        service: &RunServiceImpl,
        run_id: &str,
        caller: VerifiedIdentity,
    ) -> (bool, String) {
        let detail = service
            .get_run(identity_request(
                pb::GetRunRequest {
                    run_id: run_id.to_owned(),
                },
                caller,
            ))
            .await
            .expect("get run")
            .into_inner();
        (detail.resumable, detail.park_reason)
    }

    async fn park_reason_from_list_system_runs(
        service: &RunServiceImpl,
        org: &str,
        run_id: &str,
        caller: VerifiedIdentity,
    ) -> (bool, String) {
        let listed = service
            .list_system_runs(identity_request(
                pb::ListSystemRunsRequest {
                    org_id: org.to_owned(),
                    limit: 50,
                    ..Default::default()
                },
                caller,
            ))
            .await
            .expect("list system runs")
            .into_inner();
        let run = listed
            .runs
            .into_iter()
            .find(|run| run.run_id == run_id)
            .expect("the maintenance run is listed");
        (run.resumable, run.park_reason)
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres; sets the fold store switch, so run with --test-threads=1"]
    async fn loop_checkpoint_park_reason_reaches_only_the_owner_and_a_service() {
        use crate::loop_checkpoint::test_support::{scratch_pool, seed_fold};
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let _on = StoreOn::new();
        let until = chrono::Utc::now() + chrono::Duration::hours(1);
        let service = RunServiceImpl::new(pool.clone());

        // A parked run the owner (a person) can read: the owner sees the reason, a
        // service sees it, and a service-less peer cannot reach the run at all.
        let (owned, org, owner) =
            crate::loop_checkpoint::test_support::seed_run(&pool, "persistent").await;
        seed_fold(&pool, &owned, "park", "appr_1", "credential_window", Some(until))
            .await
            .expect("park fold");
        let got = park_reason_from_get_run(
            &service,
            &owned,
            VerifiedIdentity::user_for_test(&org, &owner),
        )
        .await;
        assert_eq!(got, (true, "credential_window".to_owned()), "the owner");
        let got = park_reason_from_get_run(
            &service,
            &owned,
            VerifiedIdentity::service_for_test(&org, &["session:read"], false),
        )
        .await;
        assert_eq!(got, (true, "credential_window".to_owned()), "a service");

        // A maintenance run is system-owned and org-readable, so a peer can read its
        // status. The peer is told it is resumable and is not told why it is parked.
        let sfx = new_ulid();
        let (maint_org, thread_id, run_id) = (
            format!("pr-org-{sfx}"),
            format!("pr-t-{sfx}"),
            format!("pr-r-{sfx}"),
        );
        seed_system_thread_and_run(
            &pool,
            &maint_org,
            &thread_id,
            &run_id,
            None,
            "nightly maintenance",
        )
        .await;
        seed_fold(&pool, &run_id, "park", "appr_1", "credential_window", Some(until))
            .await
            .expect("park fold");
        let peer = || VerifiedIdentity::user_for_test(&maint_org, "pr-peer");
        let service_caller =
            || VerifiedIdentity::service_for_test(&maint_org, &["session:read"], false);

        assert_eq!(
            park_reason_from_get_run(&service, &run_id, peer()).await,
            (true, String::new()),
            "GetRun, a peer"
        );
        assert_eq!(
            park_reason_from_get_run(&service, &run_id, service_caller()).await,
            (true, "credential_window".to_owned()),
            "GetRun, a service"
        );
        assert_eq!(
            park_reason_from_list_system_runs(&service, &maint_org, &run_id, peer()).await,
            (true, String::new()),
            "ListSystemRuns, a peer"
        );
        assert_eq!(
            park_reason_from_list_system_runs(&service, &maint_org, &run_id, service_caller())
                .await,
            (true, "credential_window".to_owned()),
            "ListSystemRuns, a service"
        );
        // The ListRuns Space-peer branch runs the same per-row projection; it needs a
        // Control read decision to reach, so the row-level cases are covered by the
        // `loop_checkpoint_park_reason_is_read_only_by_the_owner_or_a_service` unit test.
    }

    // -- Build plan WS03 S8-4 (tests only): RunDetail of a park row, and cancel (P9) --------------
    //
    // `RunDetail` and `CancelRun` are NOT changed by S8-4: these tests pin what they already do
    // with a park row under the park rules. The database ones are WRITTEN, NOT RUN by the author
    // (the lead runs them through the disposable-Postgres harness; `--test-threads=1`).

    /// A park row reads the same through `RunDetail` whatever the park switch says: it is
    /// resumable while its fold is claimable (before it is due as well), and the reason follows
    /// the viewer rule. Nothing in the row projection reads the park-recovery switch.
    #[test]
    fn a_park_row_reads_the_same_whatever_the_park_switch_says() {
        use crate::terminalization::park_test_switch;
        let read = |state: &str, claim_expired: Option<bool>, viewer: &VerifiedIdentity| {
            let row = fold_row(
                "running",
                "persistent",
                Some(state),
                Some("park"),
                claim_expired,
                Some("provider_exhausted"),
            );
            let detail = row_to_detail(row, true, viewer);
            (detail.resumable, detail.park_reason)
        };
        let peer = VerifiedIdentity::user_for_test("org_1", "someone_else");
        let rows = |on: bool| {
            let _park = park_test_switch::Forced::set(on);
            [
                read("parked", Some(false), &owner_viewer()),
                read("parked", Some(false), &peer),
                read("resuming", Some(false), &owner_viewer()),
                read("resuming", Some(true), &owner_viewer()),
                read("closed", Some(false), &owner_viewer()),
            ]
        };
        let on = rows(true);
        assert_eq!(on, rows(false), "the park switch changes nothing in the projection");
        assert_eq!(
            on,
            [
                (true, "provider_exhausted".to_owned()),
                (true, String::new()),
                (false, String::new()),
                (true, "provider_exhausted".to_owned()),
                (false, String::new()),
            ]
        );
    }

    /// A managed (obligation-carrying) persistent run for a fresh owner: its run id, org and user.
    async fn started_managed_run(pool: &PgPool, tag: &str) -> (String, String, String) {
        let sfx = new_ulid();
        let (thread_id, org, user) = (
            format!("pk-t-{tag}-{sfx}"),
            format!("pk-org-{tag}-{sfx}"),
            "u1".to_owned(),
        );
        sqlx::query("INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, $3)")
            .bind(&thread_id)
            .bind(&org)
            .bind(&user)
            .execute(pool)
            .await
            .expect("seed thread");
        let started = crate::terminalization::start_managed_run_inner(
            pool,
            pb::StartManagedRunRequest {
                thread_id,
                agent_id: "managed-agent".to_owned(),
                goal: "g".to_owned(),
                mode: "execute".to_owned(),
                org_id: org.clone(),
                user_id: user.clone(),
                start_key: format!("pk-start-{tag}-{sfx}"),
                terminal_source: pb::ManagedRunSource::ExecutionAgent as i32,
                ..Default::default()
            },
            false,
        )
        .await
        .expect("start managed run");
        (started.run_id, org, user)
    }

    async fn cancel_as_owner(
        pool: &PgPool,
        run_id: &str,
        org: &str,
        user: &str,
    ) -> pb::CancelRunResponse {
        RunServiceImpl::new(pool.clone())
            .cancel_run(identity_request(
                pb::CancelRunRequest {
                    run_id: run_id.to_owned(),
                    reason: "test".to_owned(),
                },
                VerifiedIdentity::user_for_test(org, user),
            ))
            .await
            .expect("cancel run")
            .into_inner()
    }

    async fn count_events(pool: &PgPool, run_id: &str, event_type: &str) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE run_id = $1 AND event_type = $2")
            .bind(run_id)
            .bind(event_type)
            .fetch_one(pool)
            .await
            .expect("count events")
    }

    async fn obligation_state(pool: &PgPool, run_id: &str) -> String {
        sqlx::query_scalar(
            "SELECT state FROM managed_run_terminalization_outbox WHERE run_id = $1",
        )
        .bind(run_id)
        .fetch_one(pool)
        .await
        .expect("obligation state")
    }

    /// A park stored through the real save under the park rules, then made due.
    async fn due_park(pool: &PgPool, run_id: &str) {
        use crate::loop_checkpoint::test_support::seed_fold;
        seed_fold(
            pool,
            run_id,
            "park",
            &mp_contracts::loop_fold::park_ref(run_id, 1),
            "provider_exhausted",
            Some(chrono::Utc::now() + chrono::Duration::hours(1)),
        )
        .await
        .expect("a park under the park rules");
        sqlx::query(
            "UPDATE run_loop_checkpoints SET park_until = now() - interval '1 second' WHERE run_id = $1",
        )
        .bind(run_id)
        .execute(pool)
        .await
        .expect("make the park due");
    }

    /// P9: a cancel ends a parked run in its own transaction (status, purge and receipt), the
    /// obligation is superseded by the next worker pass, and nothing ends the run twice.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres; sets the fold store switch, so run with --test-threads=1"]
    async fn cancel_purges_a_parked_fold_and_the_worker_supersedes_its_obligation() {
        use crate::loop_checkpoint::test_support::{cipher, fold_rows, scratch_pool};
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let _store = crate::loop_checkpoint::test_switch::Forced::set(true);
        let _park = crate::terminalization::park_test_switch::Forced::set(true);
        let (run_id, org, user) = started_managed_run(&pool, "cancel-park").await;
        due_park(&pool, &run_id).await;
        assert_eq!(fold_rows(&pool, &run_id).await, 1, "premise: a parked fold");
        assert_eq!(obligation_state(&pool, &run_id).await, "open", "premise: an open obligation");
        let before = get_run_as(&pool, &run_id, &org, &user).await;
        assert!(before.resumable, "premise: the park is resumable");
        assert_eq!(before.park_reason, "provider_exhausted");

        let cancelled = cancel_as_owner(&pool, &run_id, &org, &user).await;
        assert!(cancelled.cancelled);

        // One transaction: the status, the purge and the receipt are all there.
        let status: String = sqlx::query_scalar("SELECT status FROM runs WHERE id = $1")
            .bind(&run_id)
            .fetch_one(&pool)
            .await
            .expect("status");
        assert_eq!(status, "cancelled");
        assert_eq!(fold_rows(&pool, &run_id).await, 0, "the park went with the run");
        assert_eq!(count_events(&pool, &run_id, "RUN_CANCELLED").await, 1);
        let receipt_row: Option<(String,)> =
            sqlx::query_as("SELECT id FROM events WHERE run_id = $1 AND event_type = 'RUN_CANCELLED'")
                .bind(&run_id)
                .fetch_optional(&pool)
                .await
                .expect("receipt");
        assert_eq!(receipt_row.map(|(id,)| id), Some(cancelled.receipt_id.clone()));

        // The cancel does not touch the obligation: the next worker pass supersedes it, and
        // writes no terminal receipt of its own.
        assert_eq!(obligation_state(&pool, &run_id).await, "open");
        crate::terminalization::recover_due_terminalizations(&pool)
            .await
            .expect("a worker pass");
        assert_eq!(obligation_state(&pool, &run_id).await, "superseded");
        assert_eq!(
            count_events(&pool, &run_id, "RUN_TERMINAL").await,
            0,
            "a cancelled run has no terminal receipt from recovery"
        );

        // A later continue finds no park and no run to resume; a later cancel is idempotent.
        let claim = crate::loop_checkpoint::claim(&pool, &cipher(), &run_id, "", "token-a")
            .await
            .expect_err("a cancelled run is not claimable");
        assert_eq!(claim.message(), "loop_checkpoint_run_ended");
        let after = get_run_as(&pool, &run_id, &org, &user).await;
        assert!(!after.resumable);
        assert_eq!(after.park_reason, "");
        let again = cancel_as_owner(&pool, &run_id, &org, &user).await;
        assert!(!again.cancelled);
        assert_eq!(again.receipt_id, cancelled.receipt_id);
        assert_eq!(count_events(&pool, &run_id, "RUN_CANCELLED").await, 1);
    }

    /// A cancel that arrives while a continuation holds the park (it has claimed it, or has
    /// fenced it): the run ends, the fold goes, and the continuation's next write is refused.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres; sets the fold store switch, so run with --test-threads=1"]
    async fn cancel_during_a_park_resume_ends_the_run_and_its_fence_is_refused() {
        use crate::loop_checkpoint::test_support::{cipher, fold_for, fold_rows, fold_state, scratch_pool};
        use crate::loop_checkpoint::{claim, save, SaveInput};
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let _store = crate::loop_checkpoint::test_switch::Forced::set(true);
        let _park = crate::terminalization::park_test_switch::Forced::set(true);
        let write = |kind: &'static str, expected_seq: i64, run_id: String| {
            let pool = pool.clone();
            async move {
                let fold = fold_for(&run_id);
                save(
                    &pool,
                    &cipher(),
                    SaveInput {
                        run_id: &run_id,
                        expected_seq,
                        kind,
                        fold: &fold,
                        authority_digest: "auth-1",
                        pause_ref: "",
                        park_reason: "",
                        park_until: None,
                    },
                )
                .await
            }
        };

        // Before the fence: the resume has claimed the park, and the run is cancelled.
        let (run_id, org, user) = started_managed_run(&pool, "cancel-claimed").await;
        due_park(&pool, &run_id).await;
        let claimed = claim(&pool, &cipher(), &run_id, "", "token-a")
            .await
            .expect("the resume claims the due park");
        assert_eq!(fold_state(&pool, &run_id).await.as_deref(), Some("resuming"));
        assert!(cancel_as_owner(&pool, &run_id, &org, &user).await.cancelled);
        assert_eq!(fold_rows(&pool, &run_id).await, 0, "the claimed park went with the run");
        let fence = write("pre_effect", claimed.seq, run_id.clone())
            .await
            .expect_err("the fence of a cancelled run is refused");
        assert_eq!(fence.message(), "loop_checkpoint_run_ended");
        assert_eq!(fold_rows(&pool, &run_id).await, 0, "the refused fence wrote nothing");

        // After the fence: the fold is active (no longer a park), and the cancel still ends the run.
        let (run_id, org, user) = started_managed_run(&pool, "cancel-fenced").await;
        due_park(&pool, &run_id).await;
        let claimed = claim(&pool, &cipher(), &run_id, "", "token-a")
            .await
            .expect("the resume claims the due park");
        let fenced = write("pre_effect", claimed.seq, run_id.clone())
            .await
            .expect("the fence at the claimed seq wins");
        assert_eq!(fenced.seq, claimed.seq + 1);
        assert!(!fenced.park_accepted, "a fence is not a park");
        assert_eq!(fold_state(&pool, &run_id).await.as_deref(), Some("active"));
        assert!(cancel_as_owner(&pool, &run_id, &org, &user).await.cancelled);
        assert_eq!(fold_rows(&pool, &run_id).await, 0);
        let round = write("round", fenced.seq, run_id.clone())
            .await
            .expect_err("the loop's next fold on a cancelled run is refused");
        assert_eq!(round.message(), "loop_checkpoint_run_ended");
        assert_eq!(count_events(&pool, &run_id, "RUN_CANCELLED").await, 1);
    }

    // -- resolve_thread_owner ------------------------------------------------
    //
    // MEM-2: `resolve_thread_owner` is what capability-core's memory
    // authorization write-path gate calls before persisting a run/thread/
    // session-scoped `agent_memory` row (see `workplane_apis.go`'s
    // `authorizeResourceOwner`). It is modeled line-for-line on
    // `resolve_run_owner` above, which has zero test coverage in this file
    // today -- this suite exists so the new RPC does not inherit that gap.

    struct FakeThreadOwnerLookup {
        calls: AtomicUsize,
        result: Result<bool, ()>,
    }

    #[tonic::async_trait]
    impl ThreadOwnerLookup for FakeThreadOwnerLookup {
        async fn owner_matches(
            &self,
            _thread_id: &str,
            _org_id: &str,
            _user_id: &str,
        ) -> Result<bool, Status> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.result
                .map_err(|()| Status::unavailable("thread ownership unavailable"))
        }
    }

    fn thread_owner_request_for_test(
        thread_id: &str,
        org_id: &str,
        user_id: &str,
    ) -> pb::ResolveThreadOwnerRequest {
        pb::ResolveThreadOwnerRequest {
            thread_id: thread_id.to_owned(),
            org_id: org_id.to_owned(),
            user_id: user_id.to_owned(),
        }
    }

    fn service_with_thread_owner_result(
        result: Result<bool, ()>,
    ) -> (RunServiceImpl, Arc<FakeThreadOwnerLookup>) {
        // A lazily-connected pool never actually dials out: the fake lookup
        // never touches `self.pool`, so no real database is required. Mirrors
        // `memory_grpc.rs`'s `guarded_service` test helper.
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgresql://unreachable.invalid/session")
            .expect("lazy pool");
        let lookup = Arc::new(FakeThreadOwnerLookup {
            calls: AtomicUsize::new(0),
            result,
        });
        (
            RunServiceImpl::new_with_thread_owner_lookup_for_test(pool, lookup.clone()),
            lookup,
        )
    }

    fn identity_request<T>(value: T, identity: VerifiedIdentity) -> Request<T> {
        let mut request = Request::new(value);
        request.extensions_mut().insert(identity);
        request
    }

    #[tokio::test]
    async fn resolve_thread_owner_authorized_owner_returns_true() {
        let (service, lookup) = service_with_thread_owner_result(Ok(true));
        let response = service
            .resolve_thread_owner(identity_request(
                thread_owner_request_for_test("thread-1", "org-a", "user-a"),
                VerifiedIdentity::user_for_test("org-a", "user-a"),
            ))
            .await
            .expect("authorized owner resolves")
            .into_inner();
        assert!(response.authorized);
        assert_eq!(lookup.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn resolve_thread_owner_non_owner_returns_false_not_error() {
        let (service, lookup) = service_with_thread_owner_result(Ok(false));
        let response = service
            .resolve_thread_owner(identity_request(
                thread_owner_request_for_test("thread-1", "org-a", "user-a"),
                VerifiedIdentity::user_for_test("org-a", "user-a"),
            ))
            .await
            .expect("non-owner still resolves, does not error")
            .into_inner();
        assert!(!response.authorized);
        assert_eq!(lookup.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn resolve_thread_owner_unknown_thread_fails_closed_as_false_not_not_found() {
        // Same wire shape as "non-owner": the durable lookup collapses "row
        // missing" and "row belongs to someone else" into the same
        // `Ok(false)`, exercised identically here. The explicit assertion
        // that this is `Ok` (never `Status::not_found`) is the point: the
        // whole reason for the boolean-only contract (per `ResolveRunOwner`'s
        // proto comment, which this RPC mirrors) is to never disclose
        // resource existence across tenants.
        let (service, _lookup) = service_with_thread_owner_result(Ok(false));
        let response = service
            .resolve_thread_owner(identity_request(
                thread_owner_request_for_test("thread-missing", "org-a", "user-a"),
                VerifiedIdentity::user_for_test("org-a", "user-a"),
            ))
            .await
            .expect("unknown thread resolves to false, not an error");
        assert!(!response.into_inner().authorized);
    }

    #[tokio::test]
    async fn resolve_thread_owner_cross_user_request_rejected_before_db() {
        let (service, lookup) = service_with_thread_owner_result(Ok(true));
        let error = service
            .resolve_thread_owner(identity_request(
                thread_owner_request_for_test("thread-1", "org-a", "user-b"),
                VerifiedIdentity::user_for_test("org-a", "user-a"),
            ))
            .await
            .unwrap_err();
        assert_eq!(error.code(), tonic::Code::PermissionDenied);
        assert_eq!(lookup.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn resolve_thread_owner_cross_org_request_rejected_before_db() {
        let (service, lookup) = service_with_thread_owner_result(Ok(true));
        let error = service
            .resolve_thread_owner(identity_request(
                thread_owner_request_for_test("thread-1", "org-b", "user-a"),
                VerifiedIdentity::user_for_test("org-a", "user-a"),
            ))
            .await
            .unwrap_err();
        assert_eq!(error.code(), tonic::Code::PermissionDenied);
        assert_eq!(lookup.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn resolve_thread_owner_service_caller_with_session_read_may_ask_about_any_user_in_org() {
        let (service, lookup) = service_with_thread_owner_result(Ok(true));
        let response = service
            .resolve_thread_owner(identity_request(
                thread_owner_request_for_test("thread-1", "org-a", "user-z"),
                VerifiedIdentity::service_for_test("org-a", &["session:read"], false),
            ))
            .await
            .expect("service with session:read may ask about any user in its org")
            .into_inner();
        assert!(response.authorized);
        assert_eq!(lookup.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn resolve_thread_owner_service_caller_missing_scope_rejected_before_db() {
        let (service, lookup) = service_with_thread_owner_result(Ok(true));
        let error = service
            .resolve_thread_owner(identity_request(
                thread_owner_request_for_test("thread-1", "org-a", "user-z"),
                VerifiedIdentity::service_for_test("org-a", &["memory:read"], false),
            ))
            .await
            .unwrap_err();
        assert_eq!(error.code(), tonic::Code::PermissionDenied);
        assert_eq!(lookup.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn resolve_thread_owner_db_failure_fails_closed_never_authorized() {
        let (service, lookup) = service_with_thread_owner_result(Err(()));
        let error = service
            .resolve_thread_owner(identity_request(
                thread_owner_request_for_test("thread-1", "org-a", "user-a"),
                VerifiedIdentity::user_for_test("org-a", "user-a"),
            ))
            .await
            .unwrap_err();
        assert_eq!(error.code(), tonic::Code::Unavailable);
        assert_eq!(lookup.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn resolve_thread_owner_rejects_empty_fields() {
        let (service, lookup) = service_with_thread_owner_result(Ok(true));
        let error = service
            .resolve_thread_owner(identity_request(
                thread_owner_request_for_test("", "org-a", "user-a"),
                VerifiedIdentity::user_for_test("org-a", "user-a"),
            ))
            .await
            .unwrap_err();
        assert_eq!(error.code(), tonic::Code::InvalidArgument);
        assert_eq!(lookup.calls.load(Ordering::SeqCst), 0);
    }

    // -- Space membership revocation (real Postgres) -------------------------
    //
    // authorize_run_owner and PgThreadOwnerLookup::owner_matches's own SQL is
    // the thing under test here, not a fake — every test above bypasses it
    // via ThreadOwnerLookup's fake, and authorize_run_owner has no injectable
    // seam at all. Migration 0032's revocation join can only be proven
    // correct against a real database.
    //
    // #[ignore]d so plain `cargo test` (no DB) skips these; run with a DB:
    //   DATABASE_URL=… cargo test --bin session-core -- --ignored space_membership_revocation

    async fn migrated_test_pool() -> Option<PgPool> {
        let url = std::env::var("DATABASE_URL").ok()?;
        let pool = sqlx::PgPool::connect(&url).await.expect("connect pg");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");
        Some(pool)
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a Postgres with session-core migrations"]
    async fn space_membership_revocation_denies_a_run_owner_who_still_matches_org_and_user() {
        let Some(pool) = migrated_test_pool().await else {
            eprintln!("skipping: DATABASE_URL unset");
            return;
        };
        let sfx = std::process::id();
        let (run_id, thread_id, org_id, user_id, space_ref) = (
            format!("smr-run-{sfx}"),
            format!("smr-thread-{sfx}"),
            format!("smr-org-{sfx}"),
            format!("smr-user-{sfx}"),
            format!("smr-space-{sfx}"),
        );
        sqlx::query("INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, $3) ON CONFLICT (id) DO NOTHING")
            .bind(&thread_id)
            .bind(&org_id)
            .bind(&user_id)
            .execute(&pool)
            .await
            .expect("seed parent thread");
        sqlx::query(
            "INSERT INTO runs (id, thread_id, goal, org_id, user_id, space_id, space_decision_ref, \
             recipient_audience_ref, privacy_policy_ref, resource_authorization_ref, authority_revision, \
             recipient_audience_revision, recipient_audience_hash) \
             VALUES ($1, $2, 'g', $3, $4, $5, 'decision-1', 'audience-1', 'policy-1', 'resource-1', 1, 1, 'audience-hash-1') \
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(&run_id)
        .bind(&thread_id)
        .bind(&org_id)
        .bind(&user_id)
        .bind(&space_ref)
        .execute(&pool)
        .await
        .expect("seed run");

        let caller = VerifiedIdentity::user_for_test(&org_id, &user_id);

        // Before any revocation: org_id+user_id match and no revocation row
        // exists, so this must still authorize.
        authorize_run_owner(&pool, &caller, &run_id, OwnerIntent::Read)
            .await
            .expect("no revocation yet: run owner must be authorized");

        sqlx::query(
            "INSERT INTO space_membership_revocations (space_ref, org_id, subject_id) VALUES ($1, $2, $3) \
             ON CONFLICT (space_ref, subject_id) DO UPDATE SET revoked_at = NOW()",
        )
        .bind(&space_ref)
        .bind(&org_id)
        .bind(&user_id)
        .execute(&pool)
        .await
        .expect("seed revocation");

        let error = authorize_run_owner(&pool, &caller, &run_id, OwnerIntent::Read)
            .await
            .expect_err("a revoked Space membership must deny even a matching org+user run");
        assert_eq!(error.code(), tonic::Code::PermissionDenied);

        // A rejoin clears the revocation row (mirrors space_membership_nats.rs's
        // apply_membership_change on an active=true event) — authorization
        // must be restored, not permanently stuck denied.
        sqlx::query(
            "DELETE FROM space_membership_revocations WHERE space_ref = $1 AND subject_id = $2",
        )
        .bind(&space_ref)
        .bind(&user_id)
        .execute(&pool)
        .await
        .expect("clear revocation on rejoin");
        authorize_run_owner(&pool, &caller, &run_id, OwnerIntent::Read)
            .await
            .expect("a cleared revocation must restore authorization, not deny forever");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a Postgres with session-core migrations"]
    async fn space_membership_revocation_never_affects_an_unscoped_run_or_thread() {
        let Some(pool) = migrated_test_pool().await else {
            eprintln!("skipping: DATABASE_URL unset");
            return;
        };
        let sfx = std::process::id();
        let (run_id, thread_id, org_id, user_id) = (
            format!("smr-unscoped-run-{sfx}"),
            format!("smr-unscoped-thread-{sfx}"),
            format!("smr-unscoped-org-{sfx}"),
            format!("smr-unscoped-user-{sfx}"),
        );
        sqlx::query("INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, $3) ON CONFLICT (id) DO NOTHING")
            .bind(&thread_id)
            .bind(&org_id)
            .bind(&user_id)
            .execute(&pool)
            .await
            .expect("seed unscoped thread");
        sqlx::query("INSERT INTO runs (id, thread_id, goal, org_id, user_id) VALUES ($1, $2, 'g', $3, $4) ON CONFLICT (id) DO NOTHING")
            .bind(&run_id)
            .bind(&thread_id)
            .bind(&org_id)
            .bind(&user_id)
            .execute(&pool)
            .await
            .expect("seed unscoped run");

        let caller = VerifiedIdentity::user_for_test(&org_id, &user_id);
        // A revocation naming this exact user under a space_ref this run/
        // thread never used must not leak into an unscoped (space_id IS
        // NULL) row's authorization — NULL never equals a non-null revoked
        // space_ref, and this row has no space_ref at all to match.
        sqlx::query(
            "INSERT INTO space_membership_revocations (space_ref, org_id, subject_id) VALUES ('some-other-space', $1, $2)",
        )
        .bind(&org_id)
        .bind(&user_id)
        .execute(&pool)
        .await
        .expect("seed unrelated revocation");

        authorize_run_owner(&pool, &caller, &run_id, OwnerIntent::Read)
            .await
            .expect("unscoped run must be unaffected by any Space revocation");
        authorize_thread_owner(&pool, &caller, &thread_id, OwnerIntent::Read)
            .await
            .expect("unscoped thread must be unaffected by any Space revocation");

        let lookup = PgThreadOwnerLookup { pool: pool.clone() };
        let authorized = lookup
            .owner_matches(&thread_id, &org_id, &user_id)
            .await
            .expect("owner_matches must not error for an unscoped thread");
        assert!(authorized, "unscoped thread ownership must still match");

        let _ = sqlx::query(
            "DELETE FROM space_membership_revocations WHERE space_ref = 'some-other-space' AND org_id = $1",
        )
        .bind(&org_id)
        .execute(&pool)
        .await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a Postgres with session-core migrations"]
    async fn space_membership_revocation_denies_pg_thread_owner_lookup() {
        let Some(pool) = migrated_test_pool().await else {
            eprintln!("skipping: DATABASE_URL unset");
            return;
        };
        let sfx = std::process::id();
        let (thread_id, org_id, user_id, space_ref) = (
            format!("smr-lookup-thread-{sfx}"),
            format!("smr-lookup-org-{sfx}"),
            format!("smr-lookup-user-{sfx}"),
            format!("smr-lookup-space-{sfx}"),
        );
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id, space_id, space_decision_ref, \
             recipient_audience_ref, privacy_policy_ref, resource_authorization_ref, authority_revision, \
             recipient_audience_revision, recipient_audience_hash, origin) \
             VALUES ($1, $1, $2, $3, $4, 'decision-1', 'audience-1', 'policy-1', 'resource-1', 1, 1, 'audience-hash-1', 'space') \
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(&thread_id)
        .bind(&org_id)
        .bind(&user_id)
        .bind(&space_ref)
        .execute(&pool)
        .await
        .expect("seed thread");

        let lookup = PgThreadOwnerLookup { pool: pool.clone() };
        assert!(
            lookup
                .owner_matches(&thread_id, &org_id, &user_id)
                .await
                .expect("no revocation yet"),
            "org+user match with no revocation must authorize"
        );

        sqlx::query(
            "INSERT INTO space_membership_revocations (space_ref, org_id, subject_id) VALUES ($1, $2, $3)",
        )
        .bind(&space_ref)
        .bind(&org_id)
        .bind(&user_id)
        .execute(&pool)
        .await
        .expect("seed revocation");

        assert!(
            !lookup
                .owner_matches(&thread_id, &org_id, &user_id)
                .await
                .expect("query must still succeed, just return false"),
            "a revoked Space membership must deny even a matching org+user thread"
        );

        let _ = sqlx::query("DELETE FROM space_membership_revocations WHERE space_ref = $1")
            .bind(&space_ref)
            .execute(&pool)
            .await;
    }

    // -- D12: a Space-scoped system thread is not org-readable --------------
    //
    // A scheduled run is owned by `service:orchestrator-core` and lives in a
    // thread inside the person's own Space. The system-run allowance used to
    // make both readable by anyone in the org: `ListSystemRuns` returned the
    // goal text, and the thread was readable by id. Maintenance system runs
    // (no Space) keep the org-wide view they exist for.

    /// A system-owned run in its own thread; `space` makes it a scheduled one.
    #[allow(clippy::single_match_else)] // two parallel seed shapes read best side by side
    async fn seed_system_thread_and_run(
        pool: &PgPool,
        org_id: &str,
        thread_id: &str,
        run_id: &str,
        space: Option<&str>,
        goal: &str,
    ) {
        match space {
            Some(space_ref) => {
                sqlx::query(
                    "INSERT INTO threads (id, session_key, org_id, user_id, space_id, space_decision_ref, \
                     recipient_audience_ref, privacy_policy_ref, resource_authorization_ref, authority_revision, \
                     recipient_audience_revision, recipient_audience_hash, origin) \
                     VALUES ($1, $1, $2, 'service:orchestrator-core', $3, 'decision-1', 'audience-1', \
                     'policy-1', 'resource-1', 1, 1, 'audience-hash-1', 'space')",
                )
                .bind(thread_id)
                .bind(org_id)
                .bind(space_ref)
                .execute(pool)
                .await
                .expect("seed scheduled thread");
                sqlx::query(
                    "INSERT INTO runs (id, thread_id, goal, org_id, user_id, space_id, space_decision_ref, \
                     recipient_audience_ref, privacy_policy_ref, resource_authorization_ref, authority_revision, \
                     recipient_audience_revision, recipient_audience_hash, metadata) \
                     VALUES ($1, $2, $3, $4, 'service:orchestrator-core', $5, 'decision-1', 'audience-1', \
                     'policy-1', 'resource-1', 1, 1, 'audience-hash-1', '{\"source\":\"scheduled_run\"}'::jsonb)",
                )
                .bind(run_id)
                .bind(thread_id)
                .bind(goal)
                .bind(org_id)
                .bind(space_ref)
                .execute(pool)
                .await
                .expect("seed scheduled run");
            }
            None => {
                sqlx::query(
                    "INSERT INTO threads (id, session_key, org_id, user_id) \
                     VALUES ($1, $1, $2, 'service:orchestrator-core')",
                )
                .bind(thread_id)
                .bind(org_id)
                .execute(pool)
                .await
                .expect("seed maintenance thread");
                sqlx::query(
                    "INSERT INTO runs (id, thread_id, goal, org_id, user_id) \
                     VALUES ($1, $2, $3, $4, 'service:orchestrator-core')",
                )
                .bind(run_id)
                .bind(thread_id)
                .bind(goal)
                .bind(org_id)
                .execute(pool)
                .await
                .expect("seed maintenance run");
            }
        }
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a Postgres with session-core migrations"]
    async fn a_scheduled_run_is_not_listed_org_wide_and_its_thread_is_not_org_readable() {
        let Some(pool) = migrated_test_pool().await else {
            eprintln!("skipping: DATABASE_URL unset");
            return;
        };
        let sfx = new_ulid();
        let org_id = format!("d12-org-{sfx}");
        let (sched_thread, sched_run) = (format!("d12-st-{sfx}"), format!("d12-sr-{sfx}"));
        let (maint_thread, maint_run) = (format!("d12-mt-{sfx}"), format!("d12-mr-{sfx}"));
        let space_ref = format!("d12-space-{sfx}");
        seed_system_thread_and_run(
            &pool,
            &org_id,
            &sched_thread,
            &sched_run,
            Some(&space_ref),
            "weekly competitor research for me",
        )
        .await;
        seed_system_thread_and_run(
            &pool,
            &org_id,
            &maint_thread,
            &maint_run,
            None,
            "nightly maintenance",
        )
        .await;

        let service = RunServiceImpl::new(pool.clone());
        let peer = || VerifiedIdentity::user_for_test(&org_id, "d12-peer");

        // ListSystemRuns: the maintenance run is there, the schedule is not --
        // its goal text is one person's, not the org's.
        let listed = service
            .list_system_runs(identity_request(
                pb::ListSystemRunsRequest {
                    org_id: org_id.clone(),
                    status_filter: String::new(),
                    after_run_id: String::new(),
                    limit: 50,
                },
                peer(),
            ))
            .await
            .expect("an org member lists system runs")
            .into_inner();
        let ids: Vec<&str> = listed.runs.iter().map(|run| run.run_id.as_str()).collect();
        assert_eq!(
            ids,
            vec![maint_run.as_str()],
            "only the maintenance run is org-visible"
        );
        assert!(
            listed.runs.iter().all(|run| !run.goal.contains("competitor")),
            "a personal schedule's goal text must not reach an org-wide listing"
        );

        // The thread-keyed read: a peer cannot list the runs of a scheduled
        // thread by id (not found), but still can for a maintenance thread.
        let list_runs_of = |thread_id: &str| {
            service.list_runs(identity_request(
                pb::ListRunsRequest {
                    thread_id: thread_id.to_owned(),
                    limit: 10,
                    ..Default::default()
                },
                peer(),
            ))
        };
        assert_eq!(
            list_runs_of(&sched_thread)
                .await
                .expect_err("a peer must not read a scheduled thread's runs by thread id")
                .code(),
            tonic::Code::NotFound,
        );
        assert_eq!(
            list_runs_of(&maint_thread)
                .await
                .expect("maintenance threads keep the org-wide read")
                .into_inner()
                .runs
                .len(),
            1
        );

        for (thread_id, run_id) in [(&sched_thread, &sched_run), (&maint_thread, &maint_run)] {
            let _ = sqlx::query("DELETE FROM runs WHERE id = $1")
                .bind(run_id)
                .execute(&pool)
                .await;
            let _ = sqlx::query("DELETE FROM threads WHERE id = $1")
                .bind(thread_id)
                .execute(&pool)
                .await;
        }
    }
}
