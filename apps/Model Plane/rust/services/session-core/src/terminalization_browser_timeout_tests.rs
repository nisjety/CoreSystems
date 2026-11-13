//! Build plan EP-1 S5 [W3-EP-1-S5-timeout]: a takeover pause (a `browser_human_assist` approval)
//! that timed out ends its run as `browser_help_timed_out` when recovery picks it up, behind
//! SESSION_CORE_RECOVERY_APPROVAL_EXPIRY_ENABLED; with the switch off the run ends as it always did.
//! A test-only child module of `terminalization`, mirroring
//! `resume_question_db_tests::a_resume_question_that_lapsed_ends_its_run_as_question_expired`.
//!
//! The pure test needs nothing. The DB tests are `#[ignore]`d and need `DATABASE_URL` to a scratch
//! Postgres whose database name contains `test` (the lead's harness), run with
//! `cargo test -p session-core --bins terminalization::browser_timeout -- --ignored --test-threads=1`.

use super::*;
use crate::orchestration_store as approvals;

#[test]
fn browser_help_timed_out_is_written_by_recovery_only_and_the_column_accepts_it() {
    assert!(
        !ALLOWED_FAILURE_CODES.contains(&BROWSER_HELP_TIMED_OUT_FAILURE_CODE),
        "a caller may not submit the recovery-only code"
    );
    let migration = include_str!("../migrations/0115_browser_help_timed_out_failure_code.sql");
    assert!(
        migration.contains("'browser_help_timed_out'"),
        "the failure-code CHECK admits the new code"
    );
    assert!(
        migration.contains("'question_expired'"),
        "the widened CHECK keeps the codes it had"
    );
}

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

async fn run_status(pool: &PgPool, run_id: &str) -> String {
    sqlx::query_scalar("SELECT status FROM runs WHERE id = $1")
        .bind(run_id)
        .fetch_one(pool)
        .await
        .expect("run status")
}

async fn obligation(pool: &PgPool, run_id: &str) -> (String, Option<String>) {
    sqlx::query_as(
        "SELECT state, failure_code FROM managed_run_terminalization_outbox WHERE run_id = $1",
    )
    .bind(run_id)
    .fetch_one(pool)
    .await
    .expect("obligation")
}

/// A persistent managed run holding ONE approval with `cause_code`, already past its expiry, either
/// left `requested` or decided `timed_out`; its obligation fell due an hour ago.
async fn run_with_expired_approval(
    pool: &PgPool,
    tag: &str,
    cause_code: &str,
    timed_out: bool,
) -> String {
    let sfx = new_ulid();
    let (org, user, thread) = (
        format!("bt-org-{tag}-{sfx}"),
        "u1".to_owned(),
        format!("bt-t-{tag}-{sfx}"),
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
            wake: None,
            thread_id: thread,
            parent_run_id: String::new(),
            agent_id: "general-v1".to_owned(),
            goal: "browser help goal".to_owned(),
            mode: "execute".to_owned(),
            org_id: org.clone(),
            user_id: user.clone(),
            start_key: format!("bt-{tag}-{sfx}"),
            terminal_source: pb::ManagedRunSource::ExecutionAgent as i32,
            agent_version_pin: None,
        },
        RunRetention::persistent(),
    )
    .await
    .expect("the run starts")
    .run_id;
    let approval_id = format!("appr_{}", new_ulid());
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
        &serde_json::json!({"cause": {"code": cause_code}}),
        None,
        Some(Utc::now() + chrono::Duration::minutes(10)),
        "",
    )
    .await
    .expect("request approval")
    .expect("inserted");
    if timed_out {
        sqlx::query(
            "UPDATE approvals SET status = 'timed_out', decided_by = 'system', decided_at = now()
             WHERE id = $1",
        )
        .bind(&approval_id)
        .execute(pool)
        .await
        .expect("timed out");
    }
    sqlx::query("UPDATE approvals SET expires_at = now() - interval '1 hour' WHERE id = $1")
        .bind(&approval_id)
        .execute(pool)
        .await
        .expect("expired");
    sqlx::query(
        "UPDATE managed_run_terminalization_outbox
         SET deadline_at = now() - interval '1 hour', next_attempt_at = now() - interval '1 hour'
         WHERE run_id = $1",
    )
    .bind(&run_id)
    .execute(pool)
    .await
    .expect("age the obligation");
    run_id
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\""]
async fn a_timed_out_browser_assist_approval_ends_its_run_as_browser_help_timed_out() {
    let Some(pool) = scratch_pool().await else {
        return;
    };
    let _on = expiry_test_switch::Forced::set(true);
    for (tag, timed_out) in [("decided", true), ("lapsed", false)] {
        let run_id = run_with_expired_approval(&pool, tag, "browser_human_assist", timed_out).await;
        isolate(&pool, &run_id).await;
        assert_eq!(
            recover_due_terminalizations(&pool).await.expect("pass"),
            1,
            "{tag}: exactly this run is recovered"
        );
        assert_eq!(run_status(&pool, &run_id).await, "failed", "{tag}");
        let (state, failure) = obligation(&pool, &run_id).await;
        assert_eq!(
            state, "applied",
            "{tag}: a known cause is applied, not left for reconciliation"
        );
        assert_eq!(
            failure.as_deref(),
            Some(BROWSER_HELP_TIMED_OUT_FAILURE_CODE),
            "{tag}"
        );
    }
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\""]
async fn with_the_switch_off_a_timed_out_browser_assist_approval_ends_as_outcome_unknown() {
    let Some(pool) = scratch_pool().await else {
        return;
    };
    let _off = expiry_test_switch::Forced::set(false);
    let run_id = run_with_expired_approval(&pool, "off", "browser_human_assist", true).await;
    isolate(&pool, &run_id).await;
    assert_eq!(recover_due_terminalizations(&pool).await.expect("pass"), 1);
    assert_eq!(run_status(&pool, &run_id).await, "failed");
    let (state, failure) = obligation(&pool, &run_id).await;
    assert_eq!(
        state, "reconciliation_required",
        "off: unchanged, an unknown outcome"
    );
    assert_eq!(failure.as_deref(), Some(RECOVERY_FAILURE_CODE));
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\""]
async fn another_cause_that_timed_out_is_still_an_unknown_outcome() {
    let Some(pool) = scratch_pool().await else {
        return;
    };
    let _on = expiry_test_switch::Forced::set(true);
    let run_id = run_with_expired_approval(&pool, "other", "consent_to_disclose", true).await;
    isolate(&pool, &run_id).await;
    assert_eq!(recover_due_terminalizations(&pool).await.expect("pass"), 1);
    let (state, failure) = obligation(&pool, &run_id).await;
    assert_eq!(state, "reconciliation_required");
    assert_eq!(failure.as_deref(), Some(RECOVERY_FAILURE_CODE));
}
