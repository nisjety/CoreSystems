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
//!    run id the caller may not see. Under a `since_join` history floor (R-sws-01) it is also stamped
//!    at or after the floor, and its thread holds a message the caller may see from the floor on,
//!    the rule ListRuns applies, so a late joiner cannot stop a run it may not list.
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
use crate::space::audience_window::floor_gate_thread_sql;

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

/// The statement of the stop read (R-sws-01). Parameters: $1 run id, $2 org, $3 Space, $4 the audience ceiling
/// and, only under a `since_join` floor, $5 the floor. A floored read adds the run-stamp floor and the thread gate
/// that ListRuns' Space branch applies; with no floor the text is the one it always was.
fn stop_row_sql(floored: bool) -> String {
    let mut sql = String::from(
        "SELECT r.org_id AS owner_org_id, r.user_id AS owner_user_id, \
                r.retention_class AS retention_class \
         FROM runs r JOIN threads t ON t.id = r.thread_id \
         WHERE r.id = $1 AND r.org_id = $2 AND t.org_id = $2 AND t.space_id = $3 \
           AND r.parent_run_id IS NULL \
           AND (t.recipient_audience_revision IS NULL OR t.recipient_audience_revision <= $4)",
    );
    if floored {
        sql.push_str(
            " AND (r.recipient_audience_revision IS NULL OR r.recipient_audience_revision >= $5::BIGINT) AND ",
        );
        sql.push_str(&floor_gate_thread_sql("t", "$4", "$5"));
    }
    sql
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

    // R-sws-01: under a `since_join` floor (the decision signs one and the switch is the literal true) the run
    // must be stamped from the floor on, and its thread must hold a message the caller may see from it on.
    let floor = decision.history_floor;
    let sql = stop_row_sql(floor.is_some());
    let mut stop = sqlx::query_as::<_, StopRow>(&sql)
        .bind(run_id)
        .bind(caller.org_id())
        .bind(space_id)
        .bind(decision.recipient_audience_revision);
    if let Some(floor) = floor {
        stop = stop.bind(floor);
    }
    let row: StopRow = stop
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space::test_support::{
        clean_space, connect, seed_space_message_at_revision, seed_space_run, seed_space_thread,
    };
    use mp_ids::new_ulid;
    use tonic::Code;

    const MEMBER: &str = "member-1";

    /// R-sws-01: with no floor the stop read keeps the text it always had, and a floored read adds the run-stamp
    /// floor and the thread gate, on $5, and nothing else. Checked on the statement text, so no database is needed.
    #[test]
    fn the_stop_read_keeps_its_text_without_a_floor_and_adds_the_run_and_thread_floor_under_one() {
        let legacy = concat!(
            "SELECT r.org_id AS owner_org_id, r.user_id AS owner_user_id, ",
            "r.retention_class AS retention_class ",
            "FROM runs r JOIN threads t ON t.id = r.thread_id ",
            "WHERE r.id = $1 AND r.org_id = $2 AND t.org_id = $2 AND t.space_id = $3 ",
            "AND r.parent_run_id IS NULL ",
            "AND (t.recipient_audience_revision IS NULL OR t.recipient_audience_revision <= $4)",
        );
        assert_eq!(
            stop_row_sql(false),
            legacy,
            "no floor: the text is unchanged"
        );
        assert_eq!(
            stop_row_sql(true),
            format!(
                "{legacy} AND (r.recipient_audience_revision IS NULL OR r.recipient_audience_revision >= $5::BIGINT) AND {}",
                floor_gate_thread_sql("t", "$4", "$5")
            ),
            "a floor adds the run-stamp floor and the thread gate and nothing else"
        );
    }

    /// One room-manager stop of `run` by the member, under a run-control decision that signs the floor 3, with
    /// the switch on or off.
    async fn stop_as(
        pool: &PgPool,
        org: &str,
        space: &str,
        run: &str,
        key: &str,
        switch_on: bool,
    ) -> Result<pb::CancelRunAsRoomManagerResponse, Status> {
        use crate::grpc::space_w1_tests::{live_run_control_token, HistoryFloorEnv, DECISION_REF};
        let _switch = HistoryFloorEnv::set(switch_on.then_some("true"));
        let req = pb::CancelRunAsRoomManagerRequest {
            run_id: run.to_owned(),
            space_id: space.to_owned(),
            idempotency_key: key.to_owned(),
            reason: String::new(),
            space_control_decision_token: live_run_control_token(
                org,
                MEMBER,
                space,
                run,
                key,
                5,
                Some(3),
            ),
            space_control_decision_ref: DECISION_REF.to_owned(),
        };
        cancel_run_as_room_manager(pool, &VerifiedIdentity::user_for_test(org, MEMBER), &req).await
    }

    /// R-sws-01 against Postgres: under a `since_join` floor a late joiner cannot stop a run stamped before the join,
    /// nor one on a thread with nothing from the floor on, and can stop the run stamped from the join on, on a
    /// listed thread. With the switch off the same signed floor is ignored and the earlier run stops as before.
    #[tokio::test]
    #[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
    async fn a_late_joiner_cannot_stop_a_run_from_before_the_join_nor_one_on_an_unlisted_thread() {
        use crate::grpc::space_w1_tests::StreamKeysEnv;
        let Some(pool) = connect().await else {
            return;
        };
        let _keys = StreamKeysEnv::publish();
        let suffix = new_ulid();
        let org = format!("stop-floor-{suffix}");
        let space = format!("space-stop-floor-{suffix}");
        let listed = format!("{suffix}-listed");
        let unlisted = format!("{suffix}-unlisted");
        seed_space_thread(&pool, &org, &space, &listed, "owner-1").await;
        seed_space_thread(&pool, &org, &space, &unlisted, "owner-1").await;
        // The listed thread holds a turn before the join (revision 1) and one from revision 4 on; the unlisted
        // thread holds only the earlier turn.
        seed_space_message_at_revision(
            &pool,
            &space,
            &listed,
            &format!("{suffix}-l1"),
            "before the join",
            1,
        )
        .await;
        seed_space_message_at_revision(
            &pool,
            &space,
            &listed,
            &format!("{suffix}-l2"),
            "after the join",
            4,
        )
        .await;
        seed_space_message_at_revision(
            &pool,
            &space,
            &unlisted,
            &format!("{suffix}-u1"),
            "before the join",
            1,
        )
        .await;
        let early = format!("{suffix}-run-early");
        let late = format!("{suffix}-run-late");
        let quiet = format!("{suffix}-run-quiet");
        let off = format!("{suffix}-run-off");
        // `early` is stamped before the join on the listed thread; `late` from the join on, on it; `quiet` from the
        // join on, on the unlisted thread; `off` before the join on the listed thread, for the switch-off case.
        for (run, thread, stamp) in [
            (&early, &listed, 2_i64),
            (&late, &listed, 4),
            (&quiet, &unlisted, 4),
            (&off, &listed, 2),
        ] {
            seed_space_run(&pool, &org, &space, thread, run, "running", "persistent").await;
            sqlx::query("UPDATE runs SET recipient_audience_revision = $1 WHERE id = $2")
                .bind(stamp)
                .bind(run)
                .execute(&pool)
                .await
                .expect("stamp the run");
        }

        let refused = stop_as(
            &pool,
            &org,
            &space,
            &early,
            &format!("{suffix}-key-early"),
            true,
        )
        .await
        .expect_err("a run stamped before the join is not stopped");
        assert_eq!(
            refused.code(),
            Code::NotFound,
            "the run stamp floor refuses it"
        );
        let refused = stop_as(
            &pool,
            &org,
            &space,
            &quiet,
            &format!("{suffix}-key-quiet"),
            true,
        )
        .await
        .expect_err("a run on an unlisted thread is not stopped");
        assert_eq!(refused.code(), Code::NotFound, "the thread gate refuses it");
        let stopped = stop_as(
            &pool,
            &org,
            &space,
            &late,
            &format!("{suffix}-key-late"),
            true,
        )
        .await
        .expect("a run from the join on, on a listed thread, is stopped");
        assert!(stopped.cancelled, "the stop is recorded");
        let stopped = stop_as(
            &pool,
            &org,
            &space,
            &off,
            &format!("{suffix}-key-off"),
            false,
        )
        .await
        .expect("with the switch off the signed floor is ignored");
        assert!(
            stopped.cancelled,
            "the earlier run stops as before the floor"
        );
        clean_space(&pool, &org, &space).await;
    }
}
