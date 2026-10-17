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
    }
}
