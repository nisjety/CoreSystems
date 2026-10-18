//! The sweep that closes abandoned tool reservations (build plan WS03 S5).
//!
//! A tool call is reserved before it starts and finalized after it ends. A
//! reservation still `reserved` long after its run ended, or after its run parked,
//! belongs to a loop that died between starting the call and recording its result.
//! The effect may or may not have happened, and nothing may run the call again, so
//! the sweep closes the row as `unknown_outcome` and writes the audit event for it.
//!
//! The sweep never touches a live run's reservation, and it leaves a young one
//! alone: a loop that is still running finalizes within the run's budget. It is off
//! unless `SESSION_CORE_TOOL_LEDGER_SWEEP_ENABLED` is `true`.

use std::time::Duration;

use anyhow::Result;
use sqlx::PgPool;
use tonic::Status;

const ENABLED_ENV: &str = "SESSION_CORE_TOOL_LEDGER_SWEEP_ENABLED";

/// How often the sweep looks for abandoned reservations.
const INTERVAL: Duration = Duration::from_secs(60);

/// A reservation younger than this is left alone, in seconds.
const MIN_AGE_SECONDS: f64 = 900.0;

/// The most reservations one pass closes.
const BATCH: i64 = 100;

/// Whether the sweep is switched on. Off by default.
pub(crate) fn enabled() -> bool {
    std::env::var(ENABLED_ENV).is_ok_and(|value| value.trim().eq_ignore_ascii_case("true"))
}

fn internal(error: impl std::fmt::Display) -> Status {
    Status::internal(error.to_string())
}

/// One reservation the sweep found, with the owner its audit event names.
type Abandoned = (String, String, String, String, String, bool, String, String);

/// Close one batch of abandoned reservations. A reservation is abandoned when it is
/// older than `min_age_seconds` and its run has ended or is parked. Returns how many
/// it closed.
pub(crate) async fn sweep_once(pool: &PgPool, min_age_seconds: f64) -> Result<usize, Status> {
    let mut tx = pool.begin().await.map_err(internal)?;
    let rows: Vec<Abandoned> = sqlx::query_as(
        "SELECT i.run_id, i.action_id, i.request_id, i.tool, i.data_category, i.zdr,
                r.org_id, r.user_id
           FROM session_tool_audit_intents i
           JOIN runs r ON r.id = i.run_id
          WHERE i.status = 'reserved'
            AND i.reserved_at < now() - make_interval(secs => $1)
            AND (r.status IN ('completed', 'failed', 'cancelled')
                 OR EXISTS (SELECT 1 FROM run_loop_checkpoints c
                             WHERE c.run_id = i.run_id AND c.state = 'parked'))
          ORDER BY i.reserved_at
          LIMIT $2
            FOR UPDATE OF i SKIP LOCKED",
    )
    .bind(min_age_seconds)
    .bind(BATCH)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal)?;

    for (run_id, action_id, request_id, tool, data_category, zdr, org_id, user_id) in &rows {
        sqlx::query(
            "UPDATE session_tool_audit_intents
                SET status = 'unknown_outcome', finalized_at = now(), closed_by = 'sweep'
              WHERE run_id = $1 AND action_id = $2 AND status = 'reserved'",
        )
        .bind(run_id)
        .bind(action_id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
        let detail = crate::grpc::tool_action_detail(tool, data_category, *zdr);
        crate::grpc::enqueue_inline_tool_audit(
            &mut tx,
            org_id,
            user_id,
            run_id,
            action_id,
            request_id,
            "unknown_outcome",
            &detail,
            "final",
        )
        .await?;
    }
    tx.commit().await.map_err(internal)?;
    Ok(rows.len())
}

/// The most action ids one read names.
pub(crate) const MAX_LIST_ACTION_IDS: usize = 64;

/// One ledger row as the read returns it. Status, class, tool and digests only: the
/// table stores no arguments or outputs, and the provider receipt is not selected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolActionRow {
    pub action_id: String,
    pub tool: String,
    pub status: String,
    pub effect_class: Option<String>,
    pub args_digest: Option<String>,
    pub closed_by: Option<String>,
}

impl From<ToolActionRow> for mp_contracts::model_plane::v1::ToolActionState {
    fn from(row: ToolActionRow) -> Self {
        Self {
            action_id: row.action_id,
            tool: row.tool,
            status: row.status,
            effect_class: row.effect_class.unwrap_or_default(),
            args_digest: row.args_digest.unwrap_or_default(),
            closed_by: row.closed_by.unwrap_or_default(),
        }
    }
}

/// Check a read's shape before anything is looked up: a run id and at most
/// [`MAX_LIST_ACTION_IDS`] action ids, each one the ledger could hold.
pub(crate) fn validate_list_request(run_id: &str, action_ids: &[String]) -> Result<(), Status> {
    crate::grpc::validate_tool_action_token(run_id, "run_id", 128)?;
    if action_ids.len() > MAX_LIST_ACTION_IDS {
        return Err(Status::invalid_argument("too many action_ids"));
    }
    for action_id in action_ids {
        crate::grpc::validate_tool_action_token(action_id, "action_id", 128)?;
    }
    Ok(())
}

/// The ledger rows of `run_id` for the named actions, in reservation order.
///
/// Scoped to `org_id` and to the run: an action id of another run or another org
/// is never answered. A row a zero-retention call wrote is left out, and an id with
/// no row is absent, so a run that keeps no ledger rows answers an empty list. The
/// caller has already been authorised to read the run.
pub(crate) async fn list_actions(
    pool: &PgPool,
    org_id: &str,
    run_id: &str,
    action_ids: &[String],
) -> Result<Vec<ToolActionRow>, Status> {
    validate_list_request(run_id, action_ids)?;
    if action_ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows: Vec<(String, String, String, Option<String>, Option<String>, Option<String>)> =
        sqlx::query_as(
            "SELECT i.action_id, i.tool, i.status, i.effect_class, i.args_digest, i.closed_by
               FROM session_tool_audit_intents i
               JOIN runs r ON r.id = i.run_id
              WHERE i.run_id = $1
                AND r.org_id = $2
                AND i.action_id = ANY($3)
                AND NOT i.zdr
              ORDER BY i.reserved_at, i.action_id",
        )
        .bind(run_id)
        .bind(org_id)
        .bind(action_ids)
        .fetch_all(pool)
        .await
        .map_err(internal)?;
    Ok(rows
        .into_iter()
        .map(
            |(action_id, tool, status, effect_class, args_digest, closed_by)| ToolActionRow {
                action_id,
                tool,
                status,
                effect_class,
                args_digest,
                closed_by,
            },
        )
        .collect())
}

/// The sweep loop. It runs under `supervise_background`, so an error restarts it.
pub(crate) async fn run(pool: PgPool) -> Result<()> {
    loop {
        tokio::time::sleep(INTERVAL).await;
        if !enabled() {
            continue;
        }
        match sweep_once(&pool, MIN_AGE_SECONDS).await {
            Ok(0) => {}
            Ok(closed) => {
                tracing::info!(closed, "closed abandoned tool reservations as unknown_outcome");
            }
            Err(error) => tracing::warn!(%error, "tool ledger sweep failed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mp_contracts::model_plane::v1 as pb;

    #[test]
    fn the_sweep_is_off_unless_the_switch_is_true() {
        std::env::remove_var(ENABLED_ENV);
        assert!(!enabled());
        std::env::set_var(ENABLED_ENV, "false");
        assert!(!enabled());
        std::env::set_var(ENABLED_ENV, " True ");
        assert!(enabled());
        std::env::remove_var(ENABLED_ENV);
    }

    fn ids(count: usize) -> Vec<String> {
        (0..count).map(|index| format!("act-{index}")).collect()
    }

    #[test]
    fn a_list_request_takes_at_most_sixty_four_well_formed_ids() {
        assert!(validate_list_request("run-1", &ids(MAX_LIST_ACTION_IDS)).is_ok());
        assert!(validate_list_request("run-1", &[]).is_ok(), "no ids is an empty answer, not an error");
        assert_eq!(
            validate_list_request("run-1", &ids(MAX_LIST_ACTION_IDS + 1))
                .expect_err("one id too many")
                .code(),
            tonic::Code::InvalidArgument
        );
        let bad_ids = [
            String::new(),
            " padded ".to_owned(),
            "tab\tinside".to_owned(),
            "x".repeat(129),
        ];
        for bad in bad_ids {
            assert_eq!(
                validate_list_request("run-1", &[bad.clone()])
                    .expect_err("an id the ledger would never hold")
                    .code(),
                tonic::Code::InvalidArgument,
                "{bad:?}"
            );
        }
        assert_eq!(
            validate_list_request("", &ids(1)).expect_err("a run id is required").code(),
            tonic::Code::InvalidArgument
        );
    }

    #[test]
    fn a_row_maps_to_digest_only_wire_fields_and_a_missing_value_is_empty() {
        let row = ToolActionRow {
            action_id: "act-1".to_owned(),
            tool: "send_email".to_owned(),
            status: "completed".to_owned(),
            effect_class: Some("external".to_owned()),
            args_digest: Some("a".repeat(64)),
            closed_by: Some("executor".to_owned()),
        };
        let state = pb::ToolActionState::from(row);
        assert_eq!(
            (
                state.action_id.as_str(),
                state.tool.as_str(),
                state.status.as_str(),
                state.effect_class.as_str(),
                state.args_digest.as_str(),
                state.closed_by.as_str()
            ),
            ("act-1", "send_email", "completed", "external", "a".repeat(64).as_str(), "executor")
        );

        // A row the gateway's read-only loop wrote carries no class or digest, and a
        // still-open row has not been closed by anyone: each is an empty string.
        let open = pb::ToolActionState::from(ToolActionRow {
            action_id: "act-2".to_owned(),
            tool: "knowledge_search".to_owned(),
            status: "reserved".to_owned(),
            effect_class: None,
            args_digest: None,
            closed_by: None,
        });
        assert_eq!(
            (open.effect_class.as_str(), open.args_digest.as_str(), open.closed_by.as_str()),
            ("", "", "")
        );
    }

    /// A pool whose database never answers: a call that reaches it fails with a
    /// pool timeout, so a refusal that comes back as anything else was decided first.
    fn silent_pool() -> (std::net::TcpListener, PgPool) {
        let silent_database = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .acquire_timeout(Duration::from_millis(250))
            .connect_lazy(&format!(
                "postgresql://session-core@{}/session_core",
                silent_database.local_addr().expect("address")
            ))
            .expect("lazy pool");
        (silent_database, pool)
    }

    fn list_request(run_id: &str, count: usize) -> mp_contracts::model_plane::v1::ListToolActionsRequest {
        mp_contracts::model_plane::v1::ListToolActionsRequest {
            run_id: run_id.to_owned(),
            action_ids: ids(count),
        }
    }

    #[tokio::test]
    async fn the_read_is_refused_before_any_query_for_a_zdr_caller_a_missing_scope_and_a_bad_request() {
        use crate::auth::VerifiedIdentity;
        use crate::grpc::list_tool_actions_inner;
        let (_silent_database, pool) = silent_pool();

        // A zero-retention credential reads no ledger: the ledger belongs to a run that persists.
        let zdr = VerifiedIdentity::user_for_test_with_zdr("org-1", "u1", true);
        assert_eq!(
            list_tool_actions_inner(&pool, &zdr, list_request("run-1", 1))
                .await
                .expect_err("a zdr caller")
                .code(),
            tonic::Code::FailedPrecondition
        );
        // A service's credential is zdr by registry policy and reads on behalf of a run, so it
        // is not refused here; the rows a zero-retention call wrote are left out of the answer.
        let zdr_service = VerifiedIdentity::service_for_test("org-1", &["session:read"], true);
        assert_eq!(
            list_tool_actions_inner(&pool, &zdr_service, list_request("run-1", 1))
                .await
                .expect_err("there is no database behind this pool")
                .code(),
            tonic::Code::Internal
        );

        // A service needs the read scope; the write scope is not a read allowance.
        let writer = VerifiedIdentity::service_for_test("org-1", &["session:write"], false);
        assert_eq!(
            list_tool_actions_inner(&pool, &writer, list_request("run-1", 1))
                .await
                .expect_err("no session:read")
                .code(),
            tonic::Code::PermissionDenied
        );

        // An oversized or malformed request is refused before the run is even looked up.
        let owner = VerifiedIdentity::user_for_test("org-1", "u1");
        assert_eq!(
            list_tool_actions_inner(&pool, &owner, list_request("run-1", MAX_LIST_ACTION_IDS + 1))
                .await
                .expect_err("65 ids")
                .code(),
            tonic::Code::InvalidArgument
        );
        assert_eq!(
            list_tool_actions_inner(&pool, &owner, list_request(" ", 1))
                .await
                .expect_err("bad run id")
                .code(),
            tonic::Code::InvalidArgument
        );

        // A valid request from a permitted caller does reach the run-owner lookup.
        let reached = list_tool_actions_inner(&pool, &owner, list_request("run-1", 1))
            .await
            .expect_err("there is no database behind this pool");
        assert_eq!(reached.code(), tonic::Code::Internal, "{reached:?}");
    }

    /// The database-backed tests need a scratch database. Run them with
    /// `DATABASE_URL=... cargo test -p session-core --bin session-core tool_ledger -- --ignored --test-threads=1`
    /// against a scratch Postgres (the database name must say it is a test database).
    mod db {
        use super::*;
        use crate::grpc::{finalize_tool_action_inner, reserve_tool_action_inner};
        use mp_contracts::model_plane::v1 as pb;
        use sqlx::postgres::PgPoolOptions;

        async fn scratch_pool() -> Option<PgPool> {
            let Ok(url) = std::env::var("DATABASE_URL") else {
                eprintln!("skipping: DATABASE_URL unset");
                return None;
            };
            assert!(url.contains("test"), "the scratch database name must say test");
            let pool = PgPoolOptions::new()
                .max_connections(8)
                .connect(&url)
                .await
                .expect("connect pg");
            sqlx::migrate!("./migrations").run(&pool).await.expect("migrate");
            Some(pool)
        }

        /// Seed a thread and a run in the given status; returns the run id.
        async fn seed_run(pool: &PgPool, run_status: &str) -> String {
            let sfx = mp_ids::new_ulid();
            let (thread_id, run_id, org) = (
                format!("tl-t-{sfx}"),
                format!("tl-r-{sfx}"),
                format!("tl-org-{sfx}"),
            );
            sqlx::query("INSERT INTO threads (id,session_key,org_id,user_id) VALUES ($1,$1,$2,'u1')")
                .bind(&thread_id)
                .bind(&org)
                .execute(pool)
                .await
                .expect("seed thread");
            sqlx::query(
                "INSERT INTO runs (id,thread_id,goal,org_id,user_id,status) VALUES ($1,$2,'g',$3,'u1',$4)",
            )
            .bind(&run_id)
            .bind(&thread_id)
            .bind(&org)
            .bind(run_status)
            .execute(pool)
            .await
            .expect("seed run");
            run_id
        }

        /// Seed a run with one reservation; returns the run and action ids.
        async fn seed_reservation(pool: &PgPool, run_status: &str) -> (String, String) {
            let run_id = seed_run(pool, run_status).await;
            let action_id = format!("tl-a-{}", mp_ids::new_ulid());
            sqlx::query(
                "INSERT INTO session_tool_audit_intents
                    (run_id, action_id, request_id, tool, data_category, zdr, effect_class)
                 VALUES ($1, $2, 'req-1', 'send_email', 'customer_private', false, 'external')",
            )
            .bind(&run_id)
            .bind(&action_id)
            .execute(pool)
            .await
            .expect("seed reservation");
            (run_id, action_id)
        }

        async fn status_of(pool: &PgPool, run_id: &str, action_id: &str) -> (String, Option<String>) {
            sqlx::query_as(
                "SELECT status, closed_by FROM session_tool_audit_intents WHERE run_id = $1 AND action_id = $2",
            )
            .bind(run_id)
            .bind(action_id)
            .fetch_one(pool)
            .await
            .expect("row")
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn a_reservation_of_a_finished_run_is_closed_as_unknown_outcome() {
            let Some(pool) = scratch_pool().await else { return };
            let (run_id, action_id) = seed_reservation(&pool, "failed").await;

            assert!(sweep_once(&pool, 0.0).await.expect("sweep") >= 1);

            let (status, closed_by) = status_of(&pool, &run_id, &action_id).await;
            assert_eq!(status, "unknown_outcome");
            assert_eq!(closed_by.as_deref(), Some("sweep"));
            // The audit event for the closure is queued with the new outcome.
            let events: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM session_audit_outbox
                  WHERE payload->>'resource_id' = $1 AND payload->>'outcome' = 'unknown_outcome'",
            )
            .bind(&action_id)
            .fetch_one(&pool)
            .await
            .expect("count");
            assert_eq!(events, 1);
            // A second pass finds nothing more to close for it.
            sweep_once(&pool, 0.0).await.expect("second sweep");
            assert_eq!(status_of(&pool, &run_id, &action_id).await.0, "unknown_outcome");
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn a_reservation_of_a_live_run_is_left_alone() {
            let Some(pool) = scratch_pool().await else { return };
            let (run_id, action_id) = seed_reservation(&pool, "queued").await;

            sweep_once(&pool, 0.0).await.expect("sweep");

            assert_eq!(status_of(&pool, &run_id, &action_id).await.0, "reserved");
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn a_young_reservation_is_left_alone_even_when_its_run_has_ended() {
            let Some(pool) = scratch_pool().await else { return };
            let (run_id, action_id) = seed_reservation(&pool, "completed").await;

            sweep_once(&pool, 3600.0).await.expect("sweep");

            assert_eq!(status_of(&pool, &run_id, &action_id).await.0, "reserved");
        }

        fn reservation(run_id: &str, action_id: &str, digest: &str, class: &str) -> pb::ReserveToolActionRequest {
            pb::ReserveToolActionRequest {
                run_id: run_id.to_owned(),
                action_id: action_id.to_owned(),
                request_id: "req-1".to_owned(),
                tool: "send_email".to_owned(),
                data_category: "customer_private".to_owned(),
                zdr: false,
                args_digest: digest.to_owned(),
                effect_class: class.to_owned(),
            }
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn a_reservation_binds_its_class_and_digest_on_replay() {
            let Some(pool) = scratch_pool().await else { return };
            let run_id = seed_run(&pool, "queued").await;
            let action_id = format!("tl-a-{}", mp_ids::new_ulid());
            let digest = mp_contracts::effect_ledger::args_digest(r#"{"to":"a@example.test"}"#)
                .expect("digest");

            let first = reserve_tool_action_inner(
                &pool,
                None,
                reservation(&run_id, &action_id, &digest, "external"),
            )
            .await
            .expect("reserve")
            .into_inner();
            assert!(first.created);
            let replay = reserve_tool_action_inner(
                &pool,
                None,
                reservation(&run_id, &action_id, &digest, "external"),
            )
            .await
            .expect("exact replay")
            .into_inner();
            assert!(!replay.created);

            // The same action id cannot be rebound to other arguments or another class.
            let other_digest = mp_contracts::effect_ledger::args_digest(r#"{"to":"b@example.test"}"#)
                .expect("digest");
            for (changed, label) in [
                (reservation(&run_id, &action_id, &other_digest, "external"), "digest"),
                (reservation(&run_id, &action_id, &digest, "write"), "class"),
            ] {
                assert_eq!(
                    reserve_tool_action_inner(&pool, None, changed)
                        .await
                        .expect_err(label)
                        .code(),
                    tonic::Code::AlreadyExists,
                    "{label}"
                );
            }

            // A value outside the shared vocabulary never reaches the table.
            for bad in [
                reservation(&run_id, "tl-a-bad-class", &digest, "destructive"),
                reservation(&run_id, "tl-a-bad-digest", "not-a-digest", "write"),
            ] {
                assert_eq!(
                    reserve_tool_action_inner(&pool, None, bad)
                        .await
                        .expect_err("invalid field")
                        .code(),
                    tonic::Code::InvalidArgument
                );
            }
            let stored: (Option<String>, Option<String>) = sqlx::query_as(
                "SELECT args_digest, effect_class FROM session_tool_audit_intents WHERE run_id = $1 AND action_id = $2",
            )
            .bind(&run_id)
            .bind(&action_id)
            .fetch_one(&pool)
            .await
            .expect("stored");
            assert_eq!(stored, (Some(digest), Some("external".to_owned())));
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn a_finalize_records_the_closing_outcomes_and_a_receipt_only_with_completed() {
            let Some(pool) = scratch_pool().await else { return };
            let run_id = seed_run(&pool, "queued").await;
            let finalize = |action_id: &str, outcome: &str, receipt: &str| pb::FinalizeToolActionRequest {
                run_id: run_id.clone(),
                action_id: action_id.to_owned(),
                outcome: outcome.to_owned(),
                receipt_id: receipt.to_owned(),
            };

            // completed, with the provider's receipt.
            let completed = "tl-a-completed";
            reserve_tool_action_inner(&pool, None, reservation(&run_id, completed, "", "write"))
                .await
                .expect("reserve");
            assert_eq!(
                finalize_tool_action_inner(&pool, None, finalize(completed, "not_run", "rcpt-1"))
                    .await
                    .expect_err("a receipt only belongs to a completed call")
                    .code(),
                tonic::Code::InvalidArgument
            );
            assert!(
                finalize_tool_action_inner(&pool, None, finalize(completed, "completed", "rcpt-1"))
                    .await
                    .expect("finalize")
                    .into_inner()
                    .updated
            );
            let row: (String, Option<String>, Option<String>) = sqlx::query_as(
                "SELECT status, closed_by, provider_receipt_id FROM session_tool_audit_intents WHERE run_id = $1 AND action_id = $2",
            )
            .bind(&run_id)
            .bind(completed)
            .fetch_one(&pool)
            .await
            .expect("row");
            assert_eq!(
                row,
                (
                    "completed".to_owned(),
                    Some("executor".to_owned()),
                    Some("rcpt-1".to_owned())
                )
            );
            // A closed call takes no different outcome afterwards.
            assert_eq!(
                finalize_tool_action_inner(&pool, None, finalize(completed, "unknown_outcome", ""))
                    .await
                    .expect_err("already closed")
                    .code(),
                tonic::Code::AlreadyExists
            );

            // The two new outcomes close an open reservation.
            for outcome in ["not_run", "unknown_outcome"] {
                let action_id = format!("tl-a-{outcome}");
                reserve_tool_action_inner(&pool, None, reservation(&run_id, &action_id, "", "external"))
                    .await
                    .expect("reserve");
                assert!(
                    finalize_tool_action_inner(&pool, None, finalize(&action_id, outcome, ""))
                        .await
                        .expect("finalize")
                        .into_inner()
                        .updated,
                    "{outcome}"
                );
                assert_eq!(status_of(&pool, &run_id, &action_id).await.0, outcome);
            }

            // `reserved` is the open state, not an outcome.
            assert_eq!(
                finalize_tool_action_inner(&pool, None, finalize(completed, "reserved", ""))
                    .await
                    .expect_err("not an outcome")
                    .code(),
                tonic::Code::InvalidArgument
            );
        }

        /// The org a `seed_run` run belongs to.
        async fn org_of(pool: &PgPool, run_id: &str) -> String {
            sqlx::query_scalar("SELECT org_id FROM runs WHERE id = $1")
                .bind(run_id)
                .fetch_one(pool)
                .await
                .expect("org")
        }

        async fn list(
            pool: &PgPool,
            caller: &crate::auth::VerifiedIdentity,
            run_id: &str,
            action_ids: &[&str],
        ) -> Result<Vec<pb::ToolActionState>, tonic::Status> {
            crate::grpc::list_tool_actions_inner(
                pool,
                caller,
                pb::ListToolActionsRequest {
                    run_id: run_id.to_owned(),
                    action_ids: action_ids.iter().map(|id| (*id).to_owned()).collect(),
                },
            )
            .await
            .map(|response| {
                let mut actions = response.into_inner().actions;
                actions.sort_by(|a, b| a.action_id.cmp(&b.action_id));
                actions
            })
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn list_tool_actions_returns_status_and_digest_only_and_hides_zdr_rows() {
            let Some(pool) = scratch_pool().await else { return };
            let run_id = seed_run(&pool, "queued").await;
            let org = org_of(&pool, &run_id).await;
            let owner = crate::auth::VerifiedIdentity::user_for_test(&org, "u1");
            let digest = mp_contracts::effect_ledger::args_digest(r#"{"to":"a@example.test"}"#)
                .expect("digest");

            // One completed external call with a receipt, one still-open write, one row
            // the read-only loop wrote with no class and no digest, and one zero-retention row.
            reserve_tool_action_inner(&pool, None, reservation(&run_id, "tl-a-done", &digest, "external"))
                .await
                .expect("reserve");
            finalize_tool_action_inner(
                &pool,
                None,
                pb::FinalizeToolActionRequest {
                    run_id: run_id.clone(),
                    action_id: "tl-a-done".to_owned(),
                    outcome: "completed".to_owned(),
                    receipt_id: "rcpt-secret-looking-1".to_owned(),
                },
            )
            .await
            .expect("finalize");
            reserve_tool_action_inner(&pool, None, reservation(&run_id, "tl-a-open", &digest, "write"))
                .await
                .expect("reserve");
            reserve_tool_action_inner(&pool, None, reservation(&run_id, "tl-a-bare", "", ""))
                .await
                .expect("reserve");
            let mut zdr = reservation(&run_id, "tl-a-zdr", &digest, "write");
            zdr.zdr = true;
            reserve_tool_action_inner(&pool, None, zdr).await.expect("reserve");

            let asked = ["tl-a-done", "tl-a-open", "tl-a-bare", "tl-a-zdr", "tl-a-never-reserved"];
            let rows = list(&pool, &owner, &run_id, &asked).await.expect("list");
            let summary: Vec<(&str, &str, &str, &str, &str, &str)> = rows
                .iter()
                .map(|row| {
                    (
                        row.action_id.as_str(),
                        row.tool.as_str(),
                        row.status.as_str(),
                        row.effect_class.as_str(),
                        row.args_digest.as_str(),
                        row.closed_by.as_str(),
                    )
                })
                .collect();
            assert_eq!(
                summary,
                vec![
                    ("tl-a-bare", "send_email", "reserved", "", "", ""),
                    ("tl-a-done", "send_email", "completed", "external", digest.as_str(), "executor"),
                    ("tl-a-open", "send_email", "reserved", "write", digest.as_str(), ""),
                ],
                "the zero-retention row and the id that was never reserved are absent"
            );
            // Nothing the wire row carries is the provider's receipt or any argument text.
            let wire = format!("{rows:?}");
            assert!(!wire.contains("rcpt-secret-looking-1"), "{wire}");
            assert!(!wire.contains("example.test"), "{wire}");

            // The ids asked for bound the answer: nothing else of the run is listed.
            let only = list(&pool, &owner, &run_id, &["tl-a-open"]).await.expect("list");
            assert_eq!(only.len(), 1);
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn a_run_with_no_ledger_rows_lists_an_empty_answer_not_an_error() {
            let Some(pool) = scratch_pool().await else { return };
            let run_id = seed_run(&pool, "queued").await;
            let org = org_of(&pool, &run_id).await;
            let owner = crate::auth::VerifiedIdentity::user_for_test(&org, "u1");

            assert!(list(&pool, &owner, &run_id, &["tl-a-1", "tl-a-2"])
                .await
                .expect("a run that keeps no rows")
                .is_empty());
            assert!(list(&pool, &owner, &run_id, &[]).await.expect("no ids").is_empty());
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn list_tool_actions_is_scoped_to_the_callers_org_and_run() {
            let Some(pool) = scratch_pool().await else { return };
            let run_id = seed_run(&pool, "queued").await;
            let other_run = seed_run(&pool, "queued").await;
            let org = org_of(&pool, &run_id).await;
            let owner = crate::auth::VerifiedIdentity::user_for_test(&org, "u1");
            reserve_tool_action_inner(&pool, None, reservation(&run_id, "tl-a-mine", "", "write"))
                .await
                .expect("reserve");
            reserve_tool_action_inner(&pool, None, reservation(&other_run, "tl-a-theirs", "", "write"))
                .await
                .expect("reserve");

            // Another run's action id is not answered under this run.
            let rows = list(&pool, &owner, &run_id, &["tl-a-mine", "tl-a-theirs"])
                .await
                .expect("list");
            assert_eq!(
                rows.iter().map(|row| row.action_id.as_str()).collect::<Vec<_>>(),
                vec!["tl-a-mine"]
            );

            // Another org, another user of the same org, and a run that does not exist
            // are all refused; none of them learns whether an action id exists.
            let other_org = crate::auth::VerifiedIdentity::user_for_test("another-org", "u1");
            assert_eq!(
                list(&pool, &other_org, &run_id, &["tl-a-mine"]).await.expect_err("another org").code(),
                tonic::Code::PermissionDenied
            );
            let other_user = crate::auth::VerifiedIdentity::user_for_test(&org, "u2");
            assert_eq!(
                list(&pool, &other_user, &run_id, &["tl-a-mine"]).await.expect_err("another user").code(),
                tonic::Code::PermissionDenied
            );
            assert_eq!(
                list(&pool, &owner, "tl-r-no-such-run", &["tl-a-mine"])
                    .await
                    .expect_err("no such run")
                    .code(),
                tonic::Code::NotFound
            );

            // A service in the org with the read scope may read it, as it may read the run.
            let service = crate::auth::VerifiedIdentity::service_for_test(&org, &["session:read"], false);
            assert_eq!(list(&pool, &service, &run_id, &["tl-a-mine"]).await.expect("service").len(), 1);
        }
    }
}
