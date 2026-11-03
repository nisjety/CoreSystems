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
//! 5. The run's route. A persistent run names none. A zero-retention-waived run is admitted only to a
//!    caller whose own waiver names the run's connection and acknowledgement, and the verdict then names
//!    that route (provider and connection) so the gateway decides the posture on it. A zero-retention run,
//!    and any other waived run, is refused with the typed refusal.
//!
//! No state is written.

#![allow(clippy::result_large_err)]

use chrono::Utc;
use mp_contracts::model_plane::v1 as pb;
use mp_contracts::zdr_refusal::Refusal;
use sqlx::PgPool;
use tonic::Status;

use crate::auth::{authorize_operation, VerifiedIdentity, ZdrWaiver};

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
    waiver_connection_id: Option<String>,
    waiver_ack_id: Option<String>,
    started_at_unix: i64,
}

/// The run joined to its thread, when it is a root run of a thread of this Space and org inside the
/// decision's audience ceiling. Anything else is `NotFound`: there is no oracle for a run id the viewer
/// may not see.
async fn fetch_attach_row(
    pool: &PgPool,
    run_id: &str,
    org_id: &str,
    space_id: &str,
    audience_revision: i64,
) -> Result<AttachRow, Status> {
    let query = concat!(
        "SELECT r.thread_id AS thread_id, r.user_id AS owner_subject_id, ",
        crate::run_phase::run_phase_sql!("r"),
        " AS run_phase, r.retention_class AS retention_class, \
         r.waiver_connection_id AS waiver_connection_id, r.waiver_ack_id AS waiver_ack_id, \
         extract(epoch from r.created_at)::bigint AS started_at_unix \
         FROM runs r JOIN threads t ON t.id = r.thread_id \
         WHERE r.id = $1 AND r.org_id = $2 AND t.org_id = $2 AND t.space_id = $3 \
           AND r.parent_run_id IS NULL \
           AND (t.recipient_audience_revision IS NULL OR t.recipient_audience_revision <= $4)"
    );
    sqlx::query_as(query)
        .bind(run_id)
        .bind(org_id)
        .bind(space_id)
        .bind(audience_revision)
        .fetch_optional(pool)
        .await
        .map_err(|error| Status::internal(error.to_string()))?
        .ok_or_else(|| Status::not_found("run not found"))
}

/// The route a live view of this run is decided on. A persistent run names no connection, so its route
/// is empty. A zero-retention-waived run is served only to a caller whose own waiver names the connection
/// and acknowledgement the run rests on; the route is then that waiver's provider and connection. Any
/// other waived run, a run without its receipt, and a zero-retention run are refused.
fn live_route(row: &AttachRow, waiver: Option<&ZdrWaiver>) -> Result<(String, String), Status> {
    match row.retention_class.as_str() {
        "persistent" => Ok((String::new(), String::new())),
        "zdr_waived" => {
            let (Some(connection), Some(ack)) = (row.waiver_connection_id.as_deref(), row.waiver_ack_id.as_deref())
            else {
                return Err(zdr_refusal());
            };
            match waiver {
                Some(held) if held.connection_id() == connection && held.ack_id() == ack => {
                    Ok((held.provider().to_owned(), connection.to_owned()))
                }
                _ => Err(zdr_refusal()),
            }
        }
        _ => Err(zdr_refusal()),
    }
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

    let row = fetch_attach_row(
        pool,
        run_id,
        caller.org_id(),
        space_id,
        decision.recipient_audience_revision,
    )
    .await?;
    let (provider, connection) = live_route(&row, caller.zdr_waiver())?;

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
        provider,
        connection,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space::test_support;
    use mp_contracts::agentic_subscription::PROVIDER_ID;

    fn row(retention: &str, connection: Option<&str>, ack: Option<&str>) -> AttachRow {
        AttachRow {
            thread_id: "thread-1".to_owned(),
            owner_subject_id: "owner-1".to_owned(),
            run_phase: "running".to_owned(),
            retention_class: retention.to_owned(),
            waiver_connection_id: connection.map(str::to_owned),
            waiver_ack_id: ack.map(str::to_owned),
            started_at_unix: 0,
        }
    }

    fn holder(connection: &str, ack: &str) -> VerifiedIdentity {
        VerifiedIdentity::user_for_test_with_waiver("org-1", "viewer-1", connection, ack)
    }

    #[test]
    fn a_persistent_run_names_no_route() {
        let got = live_route(&row("persistent", None, None), None).expect("persistent runs are served");
        assert_eq!(got, (String::new(), String::new()));
    }

    #[test]
    fn a_waived_run_is_served_to_the_caller_whose_waiver_binds_its_connection_and_ack() {
        let viewer = holder("conn-1", "ack-1");
        let got = live_route(&row("zdr_waived", Some("conn-1"), Some("ack-1")), viewer.zdr_waiver())
            .expect("a bound waiver is honoured");
        assert_eq!(got, (PROVIDER_ID.to_owned(), "conn-1".to_owned()));
    }

    #[test]
    fn a_waived_run_is_refused_for_another_connection_another_ack_or_no_waiver() {
        let run = row("zdr_waived", Some("conn-1"), Some("ack-1"));
        let other_connection = holder("conn-2", "ack-1");
        assert!(live_route(&run, other_connection.zdr_waiver()).is_err());
        let other_ack = holder("conn-1", "ack-9");
        assert!(live_route(&run, other_ack.zdr_waiver()).is_err());
        assert!(live_route(&run, None).is_err());
    }

    #[test]
    fn a_waived_run_without_its_receipt_is_refused() {
        let viewer = holder("conn-1", "ack-1");
        assert!(live_route(&row("zdr_waived", None, Some("ack-1")), viewer.zdr_waiver()).is_err());
        assert!(live_route(&row("zdr_waived", Some("conn-1"), None), viewer.zdr_waiver()).is_err());
    }

    #[test]
    fn a_zero_retention_run_is_refused_whoever_asks() {
        let viewer = holder("conn-1", "ack-1");
        assert!(live_route(&row("zdr", None, None), viewer.zdr_waiver()).is_err());
        assert!(live_route(&row("zdr", None, None), None).is_err());
    }

    /// DB tier: a waived run is read with its receipt, and the verdict's route is the caller's waiver's
    /// provider and connection only when the connection and acknowledgement match. Needs the disposable
    /// Postgres harness (`space::test_support::connect`); not run in this pass.
    #[tokio::test]
    #[ignore = "needs DB harness"]
    async fn a_bound_waived_run_reads_its_receipt_and_is_admitted_on_its_connection() {
        let Some(pool) = test_support::connect().await else { return };
        let (org, space, thread, run) = ("org-rv-1", "space-rv-1", "thread-rv-1", "run-rv-1");
        test_support::seed_space_thread(&pool, org, space, thread, "owner-1").await;
        sqlx::query(
            "INSERT INTO runs (id,thread_id,goal,org_id,user_id,retention_class,waiver_ack_id,waiver_connection_id)
             VALUES ($1,$2,'g',$3,'owner-1','zdr_waived','ack-rv-1','conn-rv-1')",
        )
        .bind(run)
        .bind(thread)
        .bind(org)
        .execute(&pool)
        .await
        .expect("the waived run");

        let fetched = fetch_attach_row(&pool, run, org, space, i64::MAX)
            .await
            .expect("the run is inside the audience");
        assert_eq!(fetched.retention_class, "zdr_waived");
        assert_eq!(fetched.waiver_connection_id.as_deref(), Some("conn-rv-1"));
        assert_eq!(fetched.waiver_ack_id.as_deref(), Some("ack-rv-1"));

        let bound = holder("conn-rv-1", "ack-rv-1");
        let got = live_route(&fetched, bound.zdr_waiver()).expect("bound");
        assert_eq!(got, (PROVIDER_ID.to_owned(), "conn-rv-1".to_owned()));
        let other = holder("conn-rv-2", "ack-rv-1");
        assert!(live_route(&fetched, other.zdr_waiver()).is_err());
    }

    /// DB tier: a persistent run is read with no route, so it is served to a caller with no waiver.
    #[tokio::test]
    #[ignore = "needs DB harness"]
    async fn a_persistent_run_reads_with_no_route() {
        let Some(pool) = test_support::connect().await else { return };
        let (org, space, thread, run) = ("org-rv-2", "space-rv-2", "thread-rv-2", "run-rv-2");
        test_support::seed_space_thread(&pool, org, space, thread, "owner-1").await;
        sqlx::query(
            "INSERT INTO runs (id,thread_id,goal,org_id,user_id,retention_class)
             VALUES ($1,$2,'g',$3,'owner-1','persistent')",
        )
        .bind(run)
        .bind(thread)
        .bind(org)
        .execute(&pool)
        .await
        .expect("the persistent run");

        let fetched = fetch_attach_row(&pool, run, org, space, i64::MAX)
            .await
            .expect("the run is inside the audience");
        let got = live_route(&fetched, None).expect("persistent");
        assert_eq!(got, (String::new(), String::new()));
    }
}
