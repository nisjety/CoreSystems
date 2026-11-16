//! `SpaceBrowserRead`: the room-scoped browser run reads Session Core answers for model-gateway (build
//! plan WS20-S3). This file holds the two reads the start path and execution-core need before a run is
//! admitted or dispatched: the per-Space count and the run's Space. WS20-S4 adds the timeline, list and
//! evidence reads beside them.
//!
//! Both reads are read-only and carry the caller's verified user credential, forwarded by model-gateway
//! after it verified the Control browser-run decision. Both refuse a zero-retention credential first
//! (typed `browser_run`), check the org, and answer nothing about content: a count, or a Space id.

use mp_contracts::model_plane::v1 as pb;
use mp_contracts::model_plane::v1::space_browser_read_server::SpaceBrowserRead;
use mp_contracts::zdr_refusal::Refusal;
use sqlx::PgPool;
use tonic::{Request, Response, Status};

use crate::auth::{authorize_operation, identity, VerifiedIdentity};

/// The scope a service caller holds for these reads. A user credential needs no scope
/// (`authorize_operation`); its org and, for [`resolve_run_space`], its ownership are checked instead.
const SPACE_BROWSER_READ_SCOPE: &str = "session:read";

/// Browser runs of one Space in flight. Only runs the gateway's browser source started count
/// (`configured_source`), and a run in a terminal state (`completed`, `failed`, `cancelled`) does not.
/// `$3` is the start key to leave out (empty leaves none out).
const COUNT_ACTIVE_ROOM_BROWSER_RUNS_SQL: &str = "SELECT COUNT(*)::BIGINT \
    FROM runs r \
    JOIN managed_run_terminalization_outbox o ON o.run_id = r.id \
    WHERE r.org_id = $1 \
      AND r.space_id = $2 \
      AND o.configured_source = 'gateway_browser' \
      AND r.status NOT IN ('completed', 'failed', 'cancelled') \
      AND ($3 = '' OR o.start_key <> $3)";

/// The Space a run was started in, or the empty string for a run outside any Space.
const RUN_SPACE_SQL: &str = "SELECT COALESCE(space_id, '') FROM runs WHERE id = $1 AND org_id = $2";

/// The handler state: a pool handle. Reads only; nothing is written here.
#[derive(Clone)]
pub(crate) struct SpaceBrowserReadGrpc {
    pool: PgPool,
}

impl SpaceBrowserReadGrpc {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// A zero-retention credential is refused before any read: the typed `browser_run` refusal.
#[allow(clippy::result_large_err)]
fn refuse_zero_retention(caller: &VerifiedIdentity) -> Result<(), Status> {
    if caller.zdr() {
        return Err(Refusal::feature_not_supported("browser_run")
            .grpc_status(tonic::Code::FailedPrecondition));
    }
    Ok(())
}

#[tonic::async_trait]
impl SpaceBrowserRead for SpaceBrowserReadGrpc {
    async fn count_active_room_browser_runs(
        &self,
        request: Request<pb::CountActiveRoomBrowserRunsRequest>,
    ) -> Result<Response<pb::CountActiveRoomBrowserRunsResponse>, Status> {
        let caller = identity(&request)?;
        refuse_zero_retention(&caller)?;
        authorize_operation(&caller, SPACE_BROWSER_READ_SCOPE)?;
        let req = request.into_inner();
        caller.authorize_org(&req.org_id)?;
        if req.space_id.trim().is_empty() {
            return Err(Status::invalid_argument("space_id is required"));
        }
        // A count that cannot be read is an error, never zero: the caller answers 503 and admits nothing.
        let count = sqlx::query_scalar::<_, i64>(COUNT_ACTIVE_ROOM_BROWSER_RUNS_SQL)
            .bind(&req.org_id)
            .bind(&req.space_id)
            .bind(&req.exclude_start_key)
            .fetch_one(&self.pool)
            .await
            .map_err(|error| {
                tracing::warn!(error = %error, "count_active_room_browser_runs failed");
                Status::unavailable("browser run count is unavailable")
            })?;
        let active_runs = u32::try_from(count)
            .map_err(|_| Status::internal("browser run count is out of range"))?;
        Ok(Response::new(pb::CountActiveRoomBrowserRunsResponse { active_runs }))
    }

    async fn resolve_run_space(
        &self,
        request: Request<pb::ResolveRunSpaceRequest>,
    ) -> Result<Response<pb::ResolveRunSpaceResponse>, Status> {
        let caller = identity(&request)?;
        refuse_zero_retention(&caller)?;
        authorize_operation(&caller, SPACE_BROWSER_READ_SCOPE)?;
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
        // Only the run's owner learns its Space (the same owner check ResolveRunOwner makes).
        if !crate::retention_receipt::run_owner_matches(
            &self.pool,
            &caller,
            &req.run_id,
            &req.org_id,
            &req.user_id,
        )
        .await?
        {
            return Err(Status::permission_denied("run access denied"));
        }
        let space_id = sqlx::query_scalar::<_, String>(RUN_SPACE_SQL)
            .bind(&req.run_id)
            .bind(&req.org_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| {
                tracing::warn!(error = %error, "resolve_run_space failed");
                Status::unavailable("run Space is unavailable")
            })?
            .ok_or_else(|| Status::permission_denied("run access denied"))?;
        Ok(Response::new(pb::ResolveRunSpaceResponse { space_id }))
    }
}
