//! `RunService.AuthorizeRoomRunAttach` (build plan WS09-S4): may this room member watch this run live?
//!
//! The room's member live view reads a producer's stream buffer, which is keyed by the run OWNER's
//! identity. Nothing in model-gateway can decide whether another member may see it: Session Core is
//! the only verifier. This function is that verdict, asked once at subscribe and again every 15
//! seconds for as long as the stream lives. It returns ids, the honest run phase and a validity
//! window; it never returns run content.
//!
//! Checks, in order, each one failing closed:
//!
//! 1. The caller is a user principal with `session:read`, and a zero-retention credential is refused
//!    with the typed `agent_run` refusal (the room live view has no zero-retention form).
//! 2. The Control `model.thread.read` decision verifies (signature, key, audience `model-plane`,
//!    subject = caller, org, Space, no write permission, window) through
//!    [`crate::grpc::verify_space_read_claims`]. A decision that carries the zero-retention claim is
//!    refused with the same typed refusal.
//! 3. The viewer holds no row in `space_membership_revocations` for this Space. This is the path that
//!    closes a stream within one re-authorization interval of a removal.
//! 4. The run is a ROOT run of a thread of THIS Space and org, inside the decision's audience
//!    ceiling. A run anywhere else, or beyond the ceiling, is `NotFound`: there is no oracle for a
//!    run id the viewer may not see.
//! 5. The run's retention class is `persistent`; a zero-retention or waived run is refused with the
//!    typed refusal.
//!
//! No state is written.

#![allow(clippy::result_large_err)]

use chrono::Utc;
use mp_contracts::model_plane::v1 as pb;
use mp_contracts::zdr_refusal::Refusal;
use sqlx::PgPool;
use tonic::Status;

use crate::auth::{authorize_operation, VerifiedIdentity};

/// The most a validity window may reach past now, whatever the decision's own expiry: the caller
/// asks again long before this.
const MAX_VALID_SECONDS: i64 = 600;

/// Longest id accepted on the request.
const MAX_ID_BYTES: usize = 128;

/// The ZDR refusal feature key of the room live view.
const FEATURE: &str = "agent_run";

fn zdr_refusal() -> Status {
    Refusal::feature_not_supported(FEATURE).grpc_status(tonic::Code::FailedPrecondition)
}

/// One run joined to its thread, as the attach check needs it.
#[derive(sqlx::FromRow)]
struct AttachRow {
    thread_id: String,
    owner_subject_id: String,
    run_phase: String,
    retention_class: String,
    started_at_unix: i64,
}

/// The verdict. `Ok` carries the run's identity facts and how long the verdict holds.
pub(crate) async fn authorize_room_run_attach(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: &pb::AuthorizeRoomRunAttachRequest,
) -> Result<pb::AuthorizeRoomRunAttachResponse, Status> {
    authorize_operation(caller, "session:read")?;
    if caller.zdr() {
        return Err(zdr_refusal());
    }
    let run_id = req.run_id.trim();
    let space_id = req.space_id.trim();
    if run_id.is_empty() || run_id.len() > MAX_ID_BYTES {
        return Err(Status::invalid_argument("run_id is required"));
    }
    if space_id.is_empty() || space_id.len() > MAX_ID_BYTES {
        return Err(Status::invalid_argument("space_id is required"));
    }
    let Some(user_id) = caller.user_id().filter(|id| !id.trim().is_empty()) else {
        return Err(Status::permission_denied(
            "a room run attach requires an authenticated user subject",
        ));
    };

    let now = Utc::now();
    let decision = crate::grpc::verify_space_read_claims(
        &req.space_read_decision_token,
        &req.space_read_decision_ref,
        caller.org_id(),
        user_id,
        space_id,
        now,
    )?;
    if decision.zero_data_retention {
        return Err(zdr_refusal());
    }

    // A removed member is refused even while the decision they hold has not expired.
    let (revoked,): (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM space_membership_revocations \
         WHERE space_ref = $1 AND subject_id = $2)",
    )
    .bind(space_id)
    .bind(user_id)
    .fetch_one(pool)
    .await
    .map_err(|error| Status::internal(error.to_string()))?;
    if revoked {
        return Err(Status::permission_denied(
            "Space membership required for this run",
        ));
    }

    let query = concat!(
        "SELECT r.thread_id AS thread_id, r.user_id AS owner_subject_id, ",
        crate::run_phase::run_phase_sql!("r"),
        " AS run_phase, r.retention_class AS retention_class, \
         extract(epoch from r.created_at)::bigint AS started_at_unix \
         FROM runs r JOIN threads t ON t.id = r.thread_id \
         WHERE r.id = $1 AND r.org_id = $2 AND t.org_id = $2 AND t.space_id = $3 \
           AND r.parent_run_id IS NULL \
           AND (t.recipient_audience_revision IS NULL OR t.recipient_audience_revision <= $4)"
    );
    let row: AttachRow = sqlx::query_as(query)
        .bind(run_id)
        .bind(caller.org_id())
        .bind(space_id)
        .bind(decision.recipient_audience_revision)
        .fetch_optional(pool)
        .await
        .map_err(|error| Status::internal(error.to_string()))?
        .ok_or_else(|| Status::not_found("run not found"))?;

    if row.retention_class != "persistent" {
        return Err(zdr_refusal());
    }

    let valid_until = decision
        .expires_at
        .timestamp()
        .min(now.timestamp().saturating_add(MAX_VALID_SECONDS));
    Ok(pb::AuthorizeRoomRunAttachResponse {
        run_id: run_id.to_owned(),
        thread_id: row.thread_id,
        org_id: caller.org_id().to_owned(),
        owner_subject_id: row.owner_subject_id,
        run_phase: row.run_phase,
        started_at_unix: row.started_at_unix,
        valid_until_unix: valid_until,
    })
}
