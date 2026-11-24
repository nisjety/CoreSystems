//! Tests of the honest run phase and the per-thread run guard (WS09-S1), a child module of
//! `run_phase`.
//!
//! The pure tests cover the env switch, the typed refusal and the SQL predicates. The
//! `run_phase_pg_*` tests are `#[ignore]`d and need `DATABASE_URL` to a disposable Postgres
//! whose database name contains `test`; the CI step runs them with
//! `cargo test -p session-core --bin session-core run_phase -- --ignored --test-threads=1`
//! and requires at least seven to pass.

use super::*;
use crate::space::test_support::{clean_space, connect, seed_space_thread};
use crate::terminalization::{start_managed_run_inner, ManagedRunStart};
use mp_contracts::model_plane::v1 as pb;
use sqlx::PgPool;
use std::sync::Mutex;

/// Serialises every test that reads or writes the process-wide guard switch or its env var.
static GUARD_LOCK: Mutex<()> = Mutex::new(());

struct GuardSwitch {
    _guard: std::sync::MutexGuard<'static, ()>,
}

impl GuardSwitch {
    fn hold() -> Self {
        let guard = GUARD_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Self { _guard: guard }
    }
}

impl Drop for GuardSwitch {
    fn drop(&mut self) {
        set_thread_run_guard(true);
        std::env::remove_var(THREAD_RUN_GUARD_ENV);
    }
}

// ---- pure ----------------------------------------------------------------------------------

#[test]
fn run_phase_guard_env_is_on_unless_explicitly_switched_off() {
    let _switch = GuardSwitch::hold();
    std::env::remove_var(THREAD_RUN_GUARD_ENV);
    assert!(guard_enabled_from_env(), "unset means ON");
    for off in ["false", "0", "off", "no", "OFF", " No ", "False"] {
        std::env::set_var(THREAD_RUN_GUARD_ENV, off);
        assert!(!guard_enabled_from_env(), "{off:?} is the kill switch");
    }
    for on in ["true", "1", "on", "yes", "", "enabled"] {
        std::env::set_var(THREAD_RUN_GUARD_ENV, on);
        assert!(guard_enabled_from_env(), "{on:?} keeps the guard on");
    }
}

#[test]
fn run_phase_guard_switch_defaults_on_and_toggles() {
    let _switch = GuardSwitch::hold();
    set_thread_run_guard(true);
    assert!(thread_run_guard_enabled());
    set_thread_run_guard(false);
    assert!(!thread_run_guard_enabled());
}

#[test]
fn run_phase_refusal_is_aborted_with_the_error_code_and_the_active_run_trailers() {
    let status = thread_run_active_status("run_01ABC");
    assert_eq!(status.code(), tonic::Code::Aborted);
    let metadata = status.metadata();
    assert_eq!(
        metadata.get(ERROR_CODE_TRAILER).and_then(|value| value.to_str().ok()),
        Some("thread_run_active")
    );
    assert_eq!(
        metadata.get(ACTIVE_RUN_TRAILER).and_then(|value| value.to_str().ok()),
        Some("run_01ABC")
    );
    assert_eq!(ERROR_CODE_TRAILER, "x-mp-error-code");
    assert_eq!(ACTIVE_RUN_TRAILER, "x-mp-active-run-id");
    assert_eq!(THREAD_RUN_ACTIVE_CODE, "thread_run_active");
    // An id that cannot be a header value is dropped, never a panic; the code stays.
    let odd = thread_run_active_status("bad\nid");
    assert_eq!(odd.code(), tonic::Code::Aborted);
    assert!(odd.metadata().get(ACTIVE_RUN_TRAILER).is_none());
    assert!(odd.metadata().get(ERROR_CODE_TRAILER).is_some());
}

#[test]
fn run_phase_sql_derives_a_live_phase_only_from_an_open_unexpired_obligation() {
    let sql = RUN_PHASE_SQL;
    assert!(sql.contains("r.status = 'queued'"));
    assert!(sql.contains("rp_o.state IN ('open', 'processing')"));
    assert!(sql.contains("rp_o.deadline_at > now()"));
    // WS10-S1: a requested, unexpired approval of the run reads awaiting_approval, else running.
    assert!(sql.contains("FROM approvals AS rp_a"));
    assert!(sql.contains("rp_a.run_id = r.id"));
    assert!(sql.contains("rp_a.status = 'requested'"));
    assert!(sql.contains("(rp_a.expires_at IS NULL OR rp_a.expires_at > now())"));
    assert!(sql.contains("THEN 'awaiting_approval' ELSE 'running' END) ELSE r.status END"));
    assert_eq!(run_phase_sql!("x").replace("x.", "r."), sql, "the alias is the only difference");
}

#[test]
fn run_phase_active_run_query_excludes_children_non_space_and_browser_runs() {
    let sql = ACTIVE_ROOT_RUN_SQL;
    assert!(sql.contains("r.parent_run_id IS NULL"), "a child run never holds the thread");
    assert!(sql.contains("t.space_id IS NOT NULL"), "only Space threads are guarded");
    assert!(sql.contains("o.configured_source IN ('gateway_direct', 'execution_agent')"));
    assert!(!sql.contains("browser"), "a browser-sourced run never holds the thread");
    assert!(sql.contains("o.state IN ('open', 'processing')"));
    assert!(sql.contains("o.deadline_at > now()"), "an expired obligation releases the thread");
    assert!(sql.contains("r.status IN ('queued', 'running', 'awaiting_approval')"));
}

// ---- real Postgres -------------------------------------------------------------------------

const NEEDS_DB: &str = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)";
const OWNER: &str = "owner-1";

fn ids(prefix: &str) -> (String, String) {
    let suffix = mp_ids::new_ulid();
    (format!("org-{prefix}-{suffix}"), format!("space-{prefix}-{suffix}"))
}

fn start_request(
    org: &str,
    thread: &str,
    key: &str,
    parent: &str,
    source: pb::ManagedRunSource,
) -> pb::StartManagedRunRequest {
    pb::StartManagedRunRequest {
        wake: None,
        thread_id: thread.to_owned(),
        parent_run_id: parent.to_owned(),
        agent_id: "general-v1".to_owned(),
        goal: "a goal".to_owned(),
        mode: "execute".to_owned(),
        org_id: org.to_owned(),
        user_id: OWNER.to_owned(),
        start_key: key.to_owned(),
        terminal_source: source as i32,
        agent_version_pin: None,
    }
}

async fn start(
    pool: &PgPool,
    org: &str,
    thread: &str,
    key: &str,
) -> Result<ManagedRunStart, tonic::Status> {
    start_managed_run_inner(
        pool,
        start_request(org, thread, key, "", pb::ManagedRunSource::ExecutionAgent),
        false,
    )
    .await
}

async fn phase(pool: &PgPool, run_id: &str) -> String {
    sqlx::query_scalar(&format!("SELECT {RUN_PHASE_SQL} FROM runs r WHERE r.id = $1"))
        .bind(run_id)
        .fetch_one(pool)
        .await
        .expect("phase")
}

async fn seeded(prefix: &str) -> Option<(PgPool, String, String, String)> {
    let pool = connect().await?;
    set_thread_run_guard(true);
    let (org, space) = ids(prefix);
    let thread = format!("thread-{space}");
    seed_space_thread(&pool, &org, &space, &thread, OWNER).await;
    Some((pool, org, space, thread))
}

fn assert_thread_run_active(status: &tonic::Status, active_run_id: &str) {
    assert_eq!(status.code(), tonic::Code::Aborted);
    assert_eq!(
        status.metadata().get(ERROR_CODE_TRAILER).and_then(|v| v.to_str().ok()),
        Some("thread_run_active")
    );
    assert_eq!(
        status.metadata().get(ACTIVE_RUN_TRAILER).and_then(|v| v.to_str().ok()),
        Some(active_run_id)
    );
}

#[tokio::test]
#[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
async fn run_phase_pg_queued_run_with_an_open_obligation_lists_as_running() {
    let _switch = GuardSwitch::hold();
    let Some((pool, org, space, thread)) = seeded("phase").await else { return };
    let run = start(&pool, &org, &thread, "k-phase").await.expect("the run starts");
    assert_eq!(phase(&pool, &run.run_id).await, "running", "{NEEDS_DB}");
    // The stored status is still queued: the phase is derived, never written.
    let stored: String = sqlx::query_scalar("SELECT status FROM runs WHERE id = $1")
        .bind(&run.run_id)
        .fetch_one(&pool)
        .await
        .expect("status");
    assert_eq!(stored, "queued");
    // An expired obligation reads as queued again.
    sqlx::query("UPDATE managed_run_terminalization_outbox SET deadline_at = now() - interval '1 minute' WHERE run_id = $1")
        .bind(&run.run_id)
        .execute(&pool)
        .await
        .expect("expire");
    assert_eq!(phase(&pool, &run.run_id).await, "queued");
    // A terminal run reads as its status whatever its obligation says.
    sqlx::query("UPDATE managed_run_terminalization_outbox SET deadline_at = now() + interval '10 minutes' WHERE run_id = $1")
        .bind(&run.run_id)
        .execute(&pool)
        .await
        .expect("renew");
    sqlx::query("UPDATE runs SET status = 'completed' WHERE id = $1")
        .bind(&run.run_id)
        .execute(&pool)
        .await
        .expect("complete");
    assert_eq!(phase(&pool, &run.run_id).await, "completed");
    clean_space(&pool, &org, &space).await;
}

#[tokio::test]
#[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
async fn run_phase_pg_a_requested_unexpired_approval_reads_as_awaiting_approval() {
    let _switch = GuardSwitch::hold();
    let Some((pool, org, space, thread)) = seeded("approval").await else { return };
    let run = start(&pool, &org, &thread, "k-approval").await.expect("the run starts");
    assert_eq!(phase(&pool, &run.run_id).await, "running", "{NEEDS_DB}");
    let approval = mp_ids::new_ulid();
    sqlx::query(
        "INSERT INTO approvals (id, run_id, kind, status, org_id, user_id, expires_at)
         VALUES ($1, $2, 'plan', 'requested', $3, $4, now() + interval '10 minutes')",
    )
    .bind(&approval)
    .bind(&run.run_id)
    .bind(&org)
    .bind(OWNER)
    .execute(&pool)
    .await
    .expect("the approval is requested");
    assert_eq!(phase(&pool, &run.run_id).await, "awaiting_approval");
    // Decided, or past its expiry, the approval no longer holds the run: it reads running again.
    sqlx::query("UPDATE approvals SET status = 'granted' WHERE id = $1")
        .bind(&approval)
        .execute(&pool)
        .await
        .expect("decide");
    assert_eq!(phase(&pool, &run.run_id).await, "running");
    sqlx::query("UPDATE approvals SET status = 'requested', expires_at = now() - interval '1 minute' WHERE id = $1")
        .bind(&approval)
        .execute(&pool)
        .await
        .expect("expire");
    assert_eq!(phase(&pool, &run.run_id).await, "running");
    // The stored status is still queued: awaiting_approval is derived, never written.
    let stored: String = sqlx::query_scalar("SELECT status FROM runs WHERE id = $1")
        .bind(&run.run_id)
        .fetch_one(&pool)
        .await
        .expect("status");
    assert_eq!(stored, "queued");
    sqlx::query("DELETE FROM approvals WHERE id = $1")
        .bind(&approval)
        .execute(&pool)
        .await
        .expect("the approval row is removed before its run");
    clean_space(&pool, &org, &space).await;
}

#[tokio::test]
#[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
async fn run_phase_pg_guard_off_two_starts_both_succeed() {
    let _switch = GuardSwitch::hold();
    let Some((pool, org, space, thread)) = seeded("off").await else { return };
    set_thread_run_guard(false);
    let first = start(&pool, &org, &thread, "k-off-1").await.expect("first start");
    let second = start(&pool, &org, &thread, "k-off-2").await.expect("guard off: second start");
    assert_ne!(first.run_id, second.run_id, "{NEEDS_DB}");
    clean_space(&pool, &org, &space).await;
}

#[tokio::test]
#[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
async fn run_phase_pg_guard_on_second_start_is_aborted_with_the_active_run_id() {
    let _switch = GuardSwitch::hold();
    let Some((pool, org, space, thread)) = seeded("on").await else { return };
    let first = start(&pool, &org, &thread, "k-on-1").await.expect("first start");
    let refused = start(&pool, &org, &thread, "k-on-2").await.expect_err("second start is refused");
    assert_thread_run_active(&refused, &first.run_id);
    let runs: i64 = sqlx::query_scalar("SELECT count(*) FROM runs WHERE thread_id = $1")
        .bind(&thread)
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(runs, 1, "{NEEDS_DB}");
    clean_space(&pool, &org, &space).await;
}

#[tokio::test]
#[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
async fn run_phase_pg_terminal_and_stale_runs_do_not_block() {
    let _switch = GuardSwitch::hold();
    let Some((pool, org, space, thread)) = seeded("release").await else { return };
    let first = start(&pool, &org, &thread, "k-rel-1").await.expect("first start");
    // A stale run: its obligation expired, as when the producer crashed.
    sqlx::query("UPDATE managed_run_terminalization_outbox SET deadline_at = now() - interval '1 minute' WHERE run_id = $1")
        .bind(&first.run_id)
        .execute(&pool)
        .await
        .expect("expire");
    let second = start(&pool, &org, &thread, "k-rel-2").await.expect("a stale run releases the thread");
    // A terminal run releases it too.
    sqlx::query("UPDATE runs SET status = 'completed' WHERE id = $1")
        .bind(&second.run_id)
        .execute(&pool)
        .await
        .expect("complete");
    start(&pool, &org, &thread, "k-rel-3").await.expect("a terminal run releases the thread");
    clean_space(&pool, &org, &space).await;
}

#[tokio::test]
#[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
async fn run_phase_pg_child_run_start_is_not_guarded() {
    let _switch = GuardSwitch::hold();
    let Some((pool, org, space, thread)) = seeded("child").await else { return };
    let root = start(&pool, &org, &thread, "k-child-root").await.expect("root start");
    let child = start_managed_run_inner(
        &pool,
        start_request(&org, &thread, "k-child-1", &root.run_id, pb::ManagedRunSource::ExecutionAgent),
        false,
    )
    .await
    .expect("a child run starts under a live root");
    // The child holds nothing: only the root does.
    let refused = start(&pool, &org, &thread, "k-child-other").await.expect_err("the root still holds it");
    assert_thread_run_active(&refused, &root.run_id);
    assert_ne!(child.run_id, root.run_id, "{NEEDS_DB}");
    clean_space(&pool, &org, &space).await;
}

#[tokio::test]
#[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
async fn run_phase_pg_browser_sourced_run_does_not_hold_the_thread() {
    let _switch = GuardSwitch::hold();
    let Some((pool, org, space, thread)) = seeded("browser").await else { return };
    start_managed_run_inner(
        &pool,
        start_request(&org, &thread, "k-browser-1", "", pb::ManagedRunSource::GatewayBrowser),
        false,
    )
    .await
    .expect("a browser-sourced run starts");
    start(&pool, &org, &thread, "k-browser-2").await.expect("it never held the thread");
    clean_space(&pool, &org, &space).await;
}

#[tokio::test]
#[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
async fn run_phase_pg_non_space_thread_is_not_guarded() {
    let _switch = GuardSwitch::hold();
    let Some(pool) = connect().await else { return };
    set_thread_run_guard(true);
    let (org, _) = ids("plain");
    let thread = format!("plain-{}", mp_ids::new_ulid());
    sqlx::query("INSERT INTO threads (id, session_key, org_id, user_id, created_at) VALUES ($1, $1, $2, $3, now())")
        .bind(&thread)
        .bind(&org)
        .bind(OWNER)
        .execute(&pool)
        .await
        .expect("plain thread");
    start(&pool, &org, &thread, "k-plain-1").await.expect("first start");
    start(&pool, &org, &thread, "k-plain-2").await.expect("a personal thread is not guarded");
    sqlx::query("DELETE FROM runs WHERE org_id = $1").bind(&org).execute(&pool).await.ok();
    sqlx::query("DELETE FROM threads WHERE org_id = $1").bind(&org).execute(&pool).await.ok();
}

#[tokio::test]
#[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
async fn run_phase_pg_same_start_key_returns_the_existing_run_even_with_the_guard_on() {
    let _switch = GuardSwitch::hold();
    let Some((pool, org, space, thread)) = seeded("replay").await else { return };
    let first = start(&pool, &org, &thread, "k-replay").await.expect("first start");
    assert!(!first.already_started);
    let replay = start(&pool, &org, &thread, "k-replay").await.expect("a replay is never refused");
    assert!(replay.already_started, "{NEEDS_DB}");
    assert_eq!(replay.run_id, first.run_id);
    clean_space(&pool, &org, &space).await;
}

#[tokio::test]
#[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
async fn run_phase_pg_concurrent_starts_yield_exactly_one_run() {
    let _switch = GuardSwitch::hold();
    let Some((pool, org, space, thread)) = seeded("race").await else { return };
    let mut starts = Vec::new();
    for n in 0..8 {
        let (pool, org, thread) = (pool.clone(), org.clone(), thread.clone());
        starts.push(tokio::spawn(async move {
            start(&pool, &org, &thread, &format!("k-race-{n}")).await
        }));
    }
    let mut won = 0;
    let mut refused = 0;
    for start in starts {
        match start.await.expect("start task") {
            Ok(_) => won += 1,
            Err(status) => {
                assert_eq!(status.code(), tonic::Code::Aborted, "{status:?}");
                refused += 1;
            }
        }
    }
    assert_eq!((won, refused), (1, 7), "{NEEDS_DB}");
    let runs: i64 = sqlx::query_scalar("SELECT count(*) FROM runs WHERE thread_id = $1")
        .bind(&thread)
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(runs, 1);
    clean_space(&pool, &org, &space).await;
}

/// DEPTH-WS10-S1-1, as text: migration 0137 emits the derived approval phase through the one
/// chokepoint, with closed words only, and only for a live run.
#[test]
fn approval_phase_events_use_the_chokepoint_and_the_live_predicate() {
    let migration = include_str!("../migrations/0137_space_run_approval_phase_events.sql");
    for required in [
        "PERFORM space_event_emit(",
        "'run.state_changed'",
        "v_state := 'awaiting_approval';",
        "v_state := 'running';",
        "v_run.status <> 'queued'",
        "o.state IN ('open', 'processing')",
        "o.deadline_at > now()",
        "v_run.retention_class = 'zdr'",
        "AFTER INSERT ON approvals",
        "AFTER UPDATE OF status ON approvals",
        "VALUES ('approvals_phase', SQLSTATE)",
    ] {
        assert!(migration.contains(required), "migration 0137 must contain {required}");
    }
    // The same unexpired-requested predicate the derived phase reads.
    assert!(RUN_PHASE_SQL.contains("rp_a.status = 'requested'"));
    assert!(migration.contains("a.status = 'requested'"));
    assert!(migration.contains("(a.expires_at IS NULL OR a.expires_at > now())"));
    let revert = include_str!("../migrations/revert/0137_space_run_approval_phase_events.down.sql");
    assert!(revert.contains("DROP FUNCTION IF EXISTS space_event_from_approvals_phase()"));
}

async fn run_states(pool: &PgPool, org: &str, space: &str, run_id: &str) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT refs ->> 'state', coalesce(refs ->> 'prev', '') FROM space_event_log \
          WHERE org_id = $1 AND space_ref = $2 AND run_id = $3 AND kind = 'run.state_changed' \
          ORDER BY seq",
    )
    .bind(org)
    .bind(space)
    .bind(run_id)
    .fetch_all(pool)
    .await
    .expect("run events")
}

#[tokio::test]
#[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
async fn run_phase_pg_an_approval_opening_and_closing_emits_the_phase() {
    let _switch = GuardSwitch::hold();
    let Some((pool, org, space, thread)) = seeded("approval-event").await else { return };
    crate::space::test_support::set_event_log(&pool, true).await;
    let run = start(&pool, &org, &thread, "k-approval-event").await.expect("the run starts");
    let request = |id: String| {
        let (pool, run_id, org) = (pool.clone(), run.run_id.clone(), org.clone());
        async move {
            sqlx::query(
                "INSERT INTO approvals (id, run_id, kind, status, org_id, user_id, expires_at)
                 VALUES ($1, $2, 'plan', 'requested', $3, $4, now() + interval '10 minutes')",
            )
            .bind(&id)
            .bind(&run_id)
            .bind(&org)
            .bind(OWNER)
            .execute(&pool)
            .await
            .expect("the approval is requested");
            id
        }
    };
    let first = request(mp_ids::new_ulid()).await;
    // A second approval while the run already waits moves nothing.
    let second = request(mp_ids::new_ulid()).await;
    for (id, decided) in [(&first, "granted"), (&second, "denied")] {
        sqlx::query("UPDATE approvals SET status = $2 WHERE id = $1")
            .bind(id)
            .bind(decided)
            .execute(&pool)
            .await
            .expect("decide");
    }
    let states = run_states(&pool, &org, &space, &run.run_id).await;
    let tail: Vec<(&str, &str)> = states.iter().map(|(s, p)| (s.as_str(), p.as_str())).skip(1).collect();
    assert_eq!(
        tail,
        vec![("awaiting_approval", "running"), ("running", "awaiting_approval")],
        "{NEEDS_DB}: one event when the wait starts, one when the last approval is decided"
    );
    sqlx::query("DELETE FROM approvals WHERE run_id = $1")
        .bind(&run.run_id)
        .execute(&pool)
        .await
        .expect("the approval rows are removed before their run");
    clean_space(&pool, &org, &space).await;
}
