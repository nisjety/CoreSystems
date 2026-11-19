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
use crate::space::audience_window::floor_admits_at;

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
    /// The run's start instant, checked against the member's timestamp floor (D18).
    created_at: chrono::DateTime<Utc>,
}

/// The run joined to its thread, when it is a root run of a thread of this Space and org inside the
/// decision's audience ceiling and the member's history floors. Anything else is `NotFound`: there is no
/// oracle for a run id the viewer may not see. A run that started before the member's timestamp floor
/// (D18) is refused the same way: its live output may be built from the history the window hides.
async fn fetch_attach_row(
    pool: &PgPool,
    run_id: &str,
    org_id: &str,
    space_id: &str,
    audience_revision: i64,
    floor_revision: Option<i64>,
    floor_at: Option<chrono::DateTime<Utc>>,
) -> Result<AttachRow, Status> {
    let query = concat!(
        "SELECT r.thread_id AS thread_id, r.user_id AS owner_subject_id, ",
        crate::run_phase::run_phase_sql!("r"),
        " AS run_phase, r.retention_class AS retention_class, \
         r.waiver_connection_id AS waiver_connection_id, r.waiver_ack_id AS waiver_ack_id, \
         extract(epoch from r.created_at)::bigint AS started_at_unix, \
         r.created_at AS created_at \
         FROM runs r JOIN threads t ON t.id = r.thread_id \
         WHERE r.id = $1 AND r.org_id = $2 AND t.org_id = $2 AND t.space_id = $3 \
           AND r.parent_run_id IS NULL \
           AND (t.recipient_audience_revision IS NULL OR t.recipient_audience_revision <= $4) \
           AND ($5::BIGINT IS NULL OR r.recipient_audience_revision IS NULL \
                OR r.recipient_audience_revision >= $5::BIGINT) \
           AND ($6::TIMESTAMPTZ IS NULL OR r.created_at >= $6::TIMESTAMPTZ)"
    );
    let row = sqlx::query_as::<_, AttachRow>(query)
        .bind(run_id)
        .bind(org_id)
        .bind(space_id)
        .bind(audience_revision)
        .bind(floor_revision)
        .bind(floor_at)
        .fetch_optional(pool)
        .await
        .map_err(|error| Status::internal(error.to_string()))?
        .ok_or_else(|| Status::not_found("run not found"))?;
    admit_started_at(row, floor_at)
}

/// D18: a run that started before the member's timestamp floor is `NotFound`, the same answer as a run
/// the viewer may not see. The SQL applies the same rule; this check holds on the row that was read.
fn admit_started_at(row: AttachRow, floor_at: Option<chrono::DateTime<Utc>>) -> Result<AttachRow, Status> {
    if floor_admits_at(row.created_at, floor_at) {
        Ok(row)
    } else {
        Err(Status::not_found("run not found"))
    }
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

/// Until when a verdict holds, in Unix seconds: the lesser of the decision's own expiry and
/// [`MAX_VALID_SECONDS`] from `now`.
fn valid_until_unix(decision_expires_at: chrono::DateTime<Utc>, now: chrono::DateTime<Utc>) -> i64 {
    decision_expires_at
        .timestamp()
        .min(now.timestamp().saturating_add(MAX_VALID_SECONDS))
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
        decision.history_floor,
        decision.history_floor_at,
    )
    .await?;
    let (provider, connection) = live_route(&row, caller.zdr_waiver())?;

    let valid_until = valid_until_unix(decision.expires_at, now);
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
            created_at: chrono::DateTime::from_timestamp(0, 0).expect("the epoch"),
        }
    }

    fn holder(connection: &str, ack: &str) -> VerifiedIdentity {
        VerifiedIdentity::user_for_test_with_waiver("org-1", "viewer-1", connection, ack)
    }

    #[test]
    fn a_run_that_started_before_the_members_floor_is_not_found_and_one_after_it_is_served() {
        let at = |secs: i64| chrono::DateTime::from_timestamp(1_800_000_000 + secs, 0).expect("valid test time");
        let started = |secs: i64| AttachRow { created_at: at(secs), ..row("persistent", None, None) };
        let floor = Some(at(100));
        let refusal = admit_started_at(started(99), floor).err().map(|status| status.code());
        assert_eq!(refusal, Some(tonic::Code::NotFound), "a run started before the floor is refused");
        assert!(admit_started_at(started(100), floor).is_ok(), "a run started at the floor is served");
        assert!(admit_started_at(started(0), None).is_ok(), "no floor admits every run the ceiling admits");
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

        let fetched = fetch_attach_row(&pool, run, org, space, i64::MAX, None, None)
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

        let fetched = fetch_attach_row(&pool, run, org, space, i64::MAX, None, None)
            .await
            .expect("the run is inside the audience");
        let got = live_route(&fetched, None).expect("persistent");
        assert_eq!(got, (String::new(), String::new()));
    }

    // ---- the verdict (`room_attach_*`) ------------------------------------------------------
    //
    // The refusals that come before the first query run against a lazy pool nothing ever reaches.
    // The tests that read rows are `#[ignore]`d and need a disposable Postgres
    // (`space::test_support::connect`).

    use crate::grpc::space_w1_tests::{mint, StreamKeysEnv, DECISION_REF};

    fn lazy_pool() -> PgPool {
        sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgresql://unreachable.invalid/session")
            .expect("lazy pool")
    }

    /// A Control `model.thread.read` decision (audience `model-plane`) for `viewer` in `space`, with
    /// the given permissions, zero-retention claim and `(issued_at, expires_at)` window.
    fn read_token(
        org: &str,
        viewer: &str,
        space: &str,
        permissions: &[&str],
        zdr: bool,
        window: (chrono::DateTime<Utc>, chrono::DateTime<Utc>),
    ) -> String {
        mint(
            crate::grpc::CONTROL_SPACE_DECISION_AUDIENCE,
            permissions,
            zdr,
            |claims| {
                claims["org_id"] = serde_json::json!(org);
                claims["subject_id"] = serde_json::json!(viewer);
                claims["space_ref"] = serde_json::json!(space);
                claims["issued_at"] = serde_json::json!(window.0.to_rfc3339());
                claims["expires_at"] = serde_json::json!(window.1.to_rfc3339());
            },
            false,
        )
    }

    /// A read decision that is valid now, for two minutes.
    fn live_token(org: &str, viewer: &str, space: &str) -> String {
        let now = Utc::now();
        read_token(
            org,
            viewer,
            space,
            &["thread:read"],
            false,
            (now, now + chrono::Duration::minutes(2)),
        )
    }

    fn attach_request(run: &str, space: &str, token: String) -> pb::AuthorizeRoomRunAttachRequest {
        pb::AuthorizeRoomRunAttachRequest {
            run_id: run.to_owned(),
            space_id: space.to_owned(),
            space_read_decision_ref: DECISION_REF.to_owned(),
            space_read_decision_token: token,
        }
    }

    fn assert_agent_run_refusal(status: &Status) {
        assert_eq!(status.code(), tonic::Code::FailedPrecondition, "{status:?}");
        assert_eq!(
            Refusal::from_status(status),
            Some(Refusal::feature_not_supported("agent_run")),
            "{status:?}"
        );
    }

    #[tokio::test]
    async fn room_attach_a_zero_retention_credential_is_refused_with_the_agent_run_feature() {
        let zdr = VerifiedIdentity::user_for_test_with_zdr("org-1", "viewer-1", true);
        let refused = authorize_room_run_attach(
            &lazy_pool(),
            &zdr,
            &attach_request("run-1", "space-1", String::new()),
        )
        .await
        .expect_err("the room live view has no zero-retention form");
        assert_agent_run_refusal(&refused);
    }

    #[tokio::test]
    async fn room_attach_a_decision_with_the_zero_retention_claim_is_refused_with_the_agent_run_feature() {
        let _keys = StreamKeysEnv::publish();
        let now = Utc::now();
        // Control: the same decision without the claim verifies, so the refusal below is the claim's.
        crate::grpc::verify_space_read_claims(
            &live_token("org-1", "viewer-1", "space-1"),
            DECISION_REF,
            "org-1",
            "viewer-1",
            "space-1",
            now,
        )
        .expect("a live decision verifies");

        let token = read_token(
            "org-1",
            "viewer-1",
            "space-1",
            &["thread:read"],
            true,
            (now, now + chrono::Duration::minutes(2)),
        );
        let viewer = VerifiedIdentity::user_for_test("org-1", "viewer-1");
        let refused =
            authorize_room_run_attach(&lazy_pool(), &viewer, &attach_request("run-1", "space-1", token))
                .await
                .expect_err("a decision that names zero retention opens no live view");
        assert_agent_run_refusal(&refused);
    }

    #[tokio::test]
    async fn room_attach_expired_foreign_subject_and_write_permission_decisions_are_refused() {
        let _keys = StreamKeysEnv::publish();
        let now = Utc::now();
        let minute = chrono::Duration::minutes(1);
        let live = (now, now + minute * 2);
        let viewer = VerifiedIdentity::user_for_test("org-1", "viewer-1");
        // Control: the live decision verifies, so each refusal below is its own fault.
        crate::grpc::verify_space_read_claims(
            &read_token("org-1", "viewer-1", "space-1", &["thread:read"], false, live),
            DECISION_REF,
            "org-1",
            "viewer-1",
            "space-1",
            now,
        )
        .expect("a live decision verifies");

        let cases = [
            (
                "an expired decision",
                read_token("org-1", "viewer-1", "space-1", &["thread:read"], false, (now - minute * 10, now - minute * 5)),
                "Control Space read decision is expired or incomplete",
            ),
            (
                "another member's decision",
                read_token("org-1", "someone-else", "space-1", &["thread:read"], false, live),
                "Control Space decision does not authorize this conversation read",
            ),
            (
                "a decision that carries a write permission",
                read_token("org-1", "viewer-1", "space-1", &["thread:read", "thread:append"], false, live),
                "a Space read decision must not carry write permissions",
            ),
            (
                "a decision for another Space",
                read_token("org-1", "viewer-1", "space-2", &["thread:read"], false, live),
                "Control Space decision does not authorize this conversation read",
            ),
        ];
        for (what, token, message) in cases {
            let refused = authorize_room_run_attach(
                &lazy_pool(),
                &viewer,
                &attach_request("run-1", "space-1", token),
            )
            .await
            .expect_err(what);
            assert_eq!(refused.code(), tonic::Code::PermissionDenied, "{what}");
            assert_eq!(refused.message(), message, "{what}");
        }
    }

    #[test]
    fn room_attach_valid_until_is_the_sooner_of_the_decision_expiry_and_ten_minutes() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-17T12:00:00Z")
            .expect("test time")
            .with_timezone(&Utc);
        let second = chrono::Duration::seconds(1);
        assert_eq!(MAX_VALID_SECONDS, 600);
        // A decision that ends in two minutes bounds the verdict at its own expiry.
        assert_eq!(valid_until_unix(now + second * 120, now), now.timestamp() + 120);
        // One that lasts an hour is cut to ten minutes from now.
        assert_eq!(valid_until_unix(now + second * 3600, now), now.timestamp() + 600);
        // At the cap itself, both readings agree.
        assert_eq!(valid_until_unix(now + second * 600, now), now.timestamp() + 600);
        // An expiry already past is never stretched.
        assert_eq!(valid_until_unix(now - second * 5, now), now.timestamp() - 5);
    }

    /// One Space thread with one persistent root run, in fresh org and Space ids.
    struct Room {
        pool: PgPool,
        org: String,
        space: String,
        thread: String,
        run: String,
    }

    async fn room(tag: &str) -> Option<Room> {
        let pool = test_support::connect().await?;
        let sfx = mp_ids::new_ulid();
        let (org, space) = (format!("ra-{tag}-org-{sfx}"), format!("ra-{tag}-space-{sfx}"));
        let (thread, run) = (format!("ra-{tag}-thread-{sfx}"), format!("ra-{tag}-run-{sfx}"));
        test_support::seed_space_thread(&pool, &org, &space, &thread, "owner-1").await;
        test_support::seed_space_run(&pool, &org, &space, &thread, &run, "queued", "persistent").await;
        Some(Room {
            pool,
            org,
            space,
            thread,
            run,
        })
    }

    async fn watch(
        room: &Room,
        viewer: &VerifiedIdentity,
        space: &str,
        run: &str,
        token: String,
    ) -> Result<pb::AuthorizeRoomRunAttachResponse, Status> {
        authorize_room_run_attach(&room.pool, viewer, &attach_request(run, space, token)).await
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn room_attach_wrong_space_foreign_org_beyond_ceiling_and_child_run_are_not_found() {
        let Some(room) = room("nf").await else {
            return;
        };
        let _keys = StreamKeysEnv::publish();
        let viewer = VerifiedIdentity::user_for_test(&room.org, "viewer-1");
        // Control: a member may watch a root run of the Space.
        let ok = watch(
            &room,
            &viewer,
            &room.space,
            &room.run,
            live_token(&room.org, "viewer-1", &room.space),
        )
        .await
        .expect("a member may watch a root run of the Space");
        assert_eq!(ok.run_id, room.run);
        assert_eq!(ok.thread_id, room.thread);
        assert_eq!(ok.org_id, room.org);
        assert_eq!(ok.owner_subject_id, "owner-1");

        // A run that is not in the decision's Space.
        let other_space = format!("{}-other", room.space);
        let wrong_space = watch(
            &room,
            &viewer,
            &other_space,
            &room.run,
            live_token(&room.org, "viewer-1", &other_space),
        )
        .await
        .expect_err("another Space's decision reaches no run of this one");

        // A member of another org, holding that org's own decision.
        let stranger = VerifiedIdentity::user_for_test("org-elsewhere", "viewer-1");
        let foreign_org = watch(
            &room,
            &stranger,
            &room.space,
            &room.run,
            live_token("org-elsewhere", "viewer-1", &room.space),
        )
        .await
        .expect_err("another org reaches no run of this one");

        // A thread whose audience is newer than the decision's ceiling (the decision is at revision 4).
        let (late_thread, late_run) = (format!("{}-late", room.thread), format!("{}-late", room.run));
        test_support::seed_space_thread(&room.pool, &room.org, &room.space, &late_thread, "owner-1").await;
        test_support::seed_space_run(&room.pool, &room.org, &room.space, &late_thread, &late_run, "queued", "persistent")
            .await;
        sqlx::query("UPDATE threads SET recipient_audience_revision = 9 WHERE id = $1")
            .bind(&late_thread)
            .execute(&room.pool)
            .await
            .expect("a newer audience");
        let beyond_ceiling = watch(
            &room,
            &viewer,
            &room.space,
            &late_run,
            live_token(&room.org, "viewer-1", &room.space),
        )
        .await
        .expect_err("a thread beyond the audience ceiling is not visible");

        // A child run: only a root run is watched.
        let child_run = format!("{}-child", room.run);
        test_support::seed_space_run(&room.pool, &room.org, &room.space, &room.thread, &child_run, "queued", "persistent")
            .await;
        sqlx::query("UPDATE runs SET parent_run_id = $1 WHERE id = $2")
            .bind(&room.run)
            .bind(&child_run)
            .execute(&room.pool)
            .await
            .expect("make it a child run");
        let child = watch(
            &room,
            &viewer,
            &room.space,
            &child_run,
            live_token(&room.org, "viewer-1", &room.space),
        )
        .await
        .expect_err("a child run is not watched");

        for (what, refused) in [
            ("wrong space", wrong_space),
            ("foreign org", foreign_org),
            ("beyond the ceiling", beyond_ceiling),
            ("child run", child),
        ] {
            assert_eq!(refused.code(), tonic::Code::NotFound, "{what}");
            assert_eq!(refused.message(), "run not found", "{what}: no oracle for the run id");
        }
        test_support::clean_space(&room.pool, &room.org, &room.space).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn room_attach_a_revoked_viewer_is_refused_on_reauthorize() {
        let Some(room) = room("rv").await else {
            return;
        };
        let _keys = StreamKeysEnv::publish();
        let viewer = VerifiedIdentity::user_for_test(&room.org, "viewer-1");
        watch(&room, &viewer, &room.space, &room.run, live_token(&room.org, "viewer-1", &room.space))
            .await
            .expect("a member watches");

        sqlx::query(
            "INSERT INTO space_membership_revocations (space_ref, org_id, subject_id) VALUES ($1, $2, $3) \
             ON CONFLICT (space_ref, subject_id) DO UPDATE SET revoked_at = NOW()",
        )
        .bind(&room.space)
        .bind(&room.org)
        .bind("viewer-1")
        .execute(&room.pool)
        .await
        .expect("remove the member");
        // The 15-second re-authorization meets the removal although the decision still holds.
        let refused = watch(&room, &viewer, &room.space, &room.run, live_token(&room.org, "viewer-1", &room.space))
            .await
            .expect_err("a removed member watches nothing");
        assert_eq!(refused.code(), tonic::Code::PermissionDenied);
        assert_eq!(refused.message(), "Space membership required for this run");

        sqlx::query("DELETE FROM space_membership_revocations WHERE space_ref = $1")
            .bind(&room.space)
            .execute(&room.pool)
            .await
            .ok();
        test_support::clean_space(&room.pool, &room.org, &room.space).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn room_attach_zero_retention_and_unbound_waived_runs_are_refused_with_the_agent_run_feature() {
        let Some(room) = room("zr").await else {
            return;
        };
        let _keys = StreamKeysEnv::publish();
        let (zdr_run, waived_run) = (format!("{}-zdr", room.run), format!("{}-waived", room.run));
        test_support::seed_space_run(&room.pool, &room.org, &room.space, &room.thread, &zdr_run, "queued", "zdr").await;
        test_support::seed_space_run(&room.pool, &room.org, &room.space, &room.thread, &waived_run, "queued", "zdr_waived")
            .await;

        let viewer = VerifiedIdentity::user_for_test(&room.org, "viewer-1");
        for run in [&zdr_run, &waived_run] {
            let refused = watch(&room, &viewer, &room.space, run, live_token(&room.org, "viewer-1", &room.space))
                .await
                .expect_err("a run that keeps nothing, or whose waiver is not the viewer's, is not watched");
            assert_agent_run_refusal(&refused);
        }
        // A waiver for another connection does not bind the waived run (it rests on conn-1 / ack-1).
        let other_connection =
            VerifiedIdentity::user_for_test_with_waiver(&room.org, "viewer-1", "conn-2", "ack-1");
        let refused = watch(
            &room,
            &other_connection,
            &room.space,
            &waived_run,
            live_token(&room.org, "viewer-1", &room.space),
        )
        .await
        .expect_err("another connection's waiver does not bind the run");
        assert_agent_run_refusal(&refused);
        // The persistent run beside them is unaffected.
        watch(&room, &viewer, &room.space, &room.run, live_token(&room.org, "viewer-1", &room.space))
            .await
            .expect("the persistent run still attaches");
        test_support::clean_space(&room.pool, &room.org, &room.space).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn room_attach_reports_the_run_phase_and_a_window_bounded_by_the_decision_and_ten_minutes() {
        let Some(room) = room("ph").await else {
            return;
        };
        let _keys = StreamKeysEnv::publish();
        let viewer = VerifiedIdentity::user_for_test(&room.org, "viewer-1");
        // A queued run with no open obligation reads as queued.
        let queued = watch(&room, &viewer, &room.space, &room.run, live_token(&room.org, "viewer-1", &room.space))
            .await
            .expect("a queued run attaches");
        assert_eq!(queued.run_phase, "queued");

        // A run a producer started (an open obligation inside its deadline) reads as running.
        let started = crate::terminalization::start_managed_run_inner(
            &room.pool,
            pb::StartManagedRunRequest {
                thread_id: room.thread.clone(),
                agent_id: "general-v1".to_owned(),
                goal: "a goal".to_owned(),
                mode: "execute".to_owned(),
                org_id: room.org.clone(),
                user_id: "owner-1".to_owned(),
                start_key: format!("k-{}", room.run),
                terminal_source: pb::ManagedRunSource::ExecutionAgent as i32,
                ..Default::default()
            },
            false,
        )
        .await
        .expect("a managed run starts");
        let running = watch(
            &room,
            &viewer,
            &room.space,
            &started.run_id,
            live_token(&room.org, "viewer-1", &room.space),
        )
        .await
        .expect("a running run attaches");
        assert_eq!(running.run_phase, "running");
        assert_eq!(running.owner_subject_id, "owner-1");
        assert!(running.started_at_unix > 0);

        // The window: a short decision bounds it at its own expiry, a long one is cut to ten minutes.
        let now = Utc::now();
        let expires = now + chrono::Duration::seconds(120);
        let short = watch(
            &room,
            &viewer,
            &room.space,
            &room.run,
            read_token(&room.org, "viewer-1", &room.space, &["thread:read"], false, (now, expires)),
        )
        .await
        .expect("a short decision attaches");
        assert_eq!(short.valid_until_unix, expires.timestamp());
        let before = Utc::now().timestamp();
        let long = watch(
            &room,
            &viewer,
            &room.space,
            &room.run,
            read_token(
                &room.org,
                "viewer-1",
                &room.space,
                &["thread:read"],
                false,
                (now, now + chrono::Duration::hours(1)),
            ),
        )
        .await
        .expect("a long decision attaches");
        let after = Utc::now().timestamp();
        assert!(
            (before + 600..=after + 600).contains(&long.valid_until_unix),
            "{} is not ten minutes from now ({before}..{after})",
            long.valid_until_unix
        );
        test_support::clean_space(&room.pool, &room.org, &room.space).await;
    }
}
