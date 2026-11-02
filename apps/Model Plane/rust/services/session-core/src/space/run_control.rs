//! `RunService.CancelRunAsRoomManager` (build plan WS09-S8): a room manager or owner stops another
//! member's live agent run.
//!
//! The run's owner can already stop it (`CancelRun`, owner-bound). A room manager is not the owner,
//! and nothing in model-gateway or the gateway may decide that they may stop someone else's turn:
//! Session Core is the only verifier. The authority is a Control `model.run.control` decision
//! (WS09-S7: role manager or owner, the org's `run_control_entitled` bit, current audience), and
//! this function checks it before it changes a row.
//!
//! Checks, in order, each one failing closed:
//!
//! 1. The caller is a user principal with `session:write`. A zero-retention credential is refused
//!    with the typed `zdr_durable_mutation_forbidden` (a stop writes a `RUN_CANCELLED` receipt).
//! 2. The Control decision verifies (signature, key, audience `model-plane`, subject = caller, org,
//!    Space, action `model.run.control`, permissions exactly `["run:cancel"]`, digest bound to this
//!    `run_id` and the operation `cancel`, the request's idempotency key, window) through
//!    [`crate::grpc::verify_run_control_space_decision`]. A decision that carries the zero-retention
//!    claim is refused with the same typed refusal.
//! 3. The caller holds no row in `space_membership_revocations` for this Space.
//! 4. The run is a ROOT run of a thread of THIS Space and org, inside the decision's audience
//!    ceiling. A run anywhere else, or beyond the ceiling, is `NotFound`: there is no oracle for a
//!    run id the caller may not see.
//! 5. The run's retention class is `persistent`; a zero-retention or waived run is refused with the
//!    typed refusal.
//!
//! Then the same transaction as `CancelRun` flips the run and writes the `RUN_CANCELLED` receipt,
//! which names `cancelled_by` (the manager), `acting_as: "room_manager"` and the decision
//! reference. The event key `run_cancelled:{run_id}` de-duplicates a retried or racing stop.

#![allow(clippy::result_large_err)]

use chrono::Utc;
use mp_contracts::model_plane::v1 as pb;
use mp_contracts::zdr_refusal::Refusal;
use sqlx::PgPool;
use tonic::Status;

use crate::auth::{authorize_operation, VerifiedIdentity};
use crate::run_service_grpc::{cancel_run_terminalize, ActingAs};

/// Longest id accepted on the request.
const MAX_ID_BYTES: usize = 128;

/// The reason recorded when the request names none, and the only words a request may name.
const DEFAULT_REASON: &str = "room_manager_stop";

/// What `acting_as` reads on the receipt of a manager's stop.
const ACTING_AS_ROOM_MANAGER: &str = "room_manager";

fn zdr_refusal() -> Status {
    Refusal::DURABLE_MUTATION_FORBIDDEN.grpc_status(tonic::Code::FailedPrecondition)
}

/// The receipt reason: a short token of lowercase letters, digits and underscores, else the
/// default. A free-text reason never reaches a durable event.
fn receipt_reason(requested: &str) -> &str {
    let requested = requested.trim();
    let token_ok = !requested.is_empty()
        && requested.len() <= 40
        && requested
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
    if token_ok {
        requested
    } else {
        DEFAULT_REASON
    }
}

/// One run joined to its thread, as the stop check needs it.
#[derive(sqlx::FromRow)]
struct StopRow {
    owner_org_id: String,
    owner_user_id: String,
    retention_class: String,
}

/// Stop one room run under a verified Control run-control decision.
pub(crate) async fn cancel_run_as_room_manager(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: &pb::CancelRunAsRoomManagerRequest,
) -> Result<pb::CancelRunAsRoomManagerResponse, Status> {
    // `session:write` also refuses a zero-retention credential with the typed refusal.
    authorize_operation(caller, "session:write")?;
    if caller.zdr() {
        return Err(zdr_refusal());
    }
    let run_id = req.run_id.trim();
    let space_id = req.space_id.trim();
    let idempotency_key = req.idempotency_key.trim();
    if run_id.is_empty() || run_id.len() > MAX_ID_BYTES {
        return Err(Status::invalid_argument("run_id is required"));
    }
    if space_id.is_empty() || space_id.len() > MAX_ID_BYTES {
        return Err(Status::invalid_argument("space_id is required"));
    }
    if idempotency_key.is_empty() || idempotency_key.len() > MAX_ID_BYTES {
        return Err(Status::invalid_argument("idempotency_key is required"));
    }
    let Some(user_id) = caller.user_id().filter(|id| !id.trim().is_empty()) else {
        return Err(Status::permission_denied(
            "a room run stop requires an authenticated user subject",
        ));
    };

    let now = Utc::now();
    let decision = crate::grpc::verify_run_control_space_decision(
        &req.space_control_decision_token,
        &req.space_control_decision_ref,
        caller.org_id(),
        user_id,
        space_id,
        run_id,
        idempotency_key,
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

    let row: StopRow = sqlx::query_as(
        "SELECT r.org_id AS owner_org_id, r.user_id AS owner_user_id, \
                r.retention_class AS retention_class \
         FROM runs r JOIN threads t ON t.id = r.thread_id \
         WHERE r.id = $1 AND r.org_id = $2 AND t.org_id = $2 AND t.space_id = $3 \
           AND r.parent_run_id IS NULL \
           AND (t.recipient_audience_revision IS NULL OR t.recipient_audience_revision <= $4)",
    )
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

    let acting = ActingAs {
        acting_as: ACTING_AS_ROOM_MANAGER,
        decision_ref: req.space_control_decision_ref.trim(),
    };
    let cancellation = cancel_run_terminalize(
        pool,
        run_id,
        receipt_reason(&req.reason),
        user_id,
        Some(&acting),
    )
    .await?;
    Ok(pb::CancelRunAsRoomManagerResponse {
        cancelled: cancellation.cancelled,
        receipt_id: cancellation.receipt_id,
        owner_org_id: row.owner_org_id,
        owner_user_id: row.owner_user_id,
    })
}
