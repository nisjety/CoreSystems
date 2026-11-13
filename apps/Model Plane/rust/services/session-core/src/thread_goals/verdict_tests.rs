//! Goal verdict and judge-gate tests (EP-7 S3 [W3-EP-7-S3-a], OD-58 [R2-OD-58-b]), a test-only child
//! module of `thread_goals`.
//!
//! * Pure tier: only the persistent goal-judge principal passes `refuse_non_judge`; the judge read
//!   scope is not implied by the verdict write scope.
//! * DB tier (every test `#[ignore]`d, needs `DATABASE_URL` to a disposable Postgres with the
//!   session-core migrations): a `met` verdict closes the goal, the owner's reopen bumps the
//!   revision, a verdict is idempotent per turn, a verdict row keeps no reasons or evidence ids, and a
//!   verdict on a thread that holds a waived run is refused and changes no row.

use super::test_support::insert_completed_run;
use super::*;
use crate::retention_receipt::RunRetention;

const ORG: &str = "org-goal-verdicts";
const OWNER: &str = "user-goal-verdict-owner";

fn judge() -> VerifiedIdentity {
    VerifiedIdentity::service_for_test_as(
        ORG,
        JUDGE_CALLER,
        &[GOAL_VERDICT_WRITE_SCOPE, GOAL_JUDGE_READ_SCOPE],
        false,
    )
}

fn owner() -> VerifiedIdentity {
    VerifiedIdentity::user_for_test(ORG, OWNER)
}

// ---- pure ----------------------------------------------------------------------------------

#[test]
fn refuse_non_judge_admits_only_the_goal_judge_principal() {
    let judge_writer = VerifiedIdentity::service_for_test_as(
        "org-1",
        "service:model-gateway-goal-judge",
        &[GOAL_VERDICT_WRITE_SCOPE],
        false,
    );
    assert!(refuse_non_judge(&judge_writer).is_ok());

    // The ZDR-capable gateway principal, even holding the verdict scope, is not the judge.
    let gateway = VerifiedIdentity::service_for_test_as(
        "org-1",
        "service:model-gateway",
        &[GOAL_VERDICT_WRITE_SCOPE, GOAL_JUDGE_READ_SCOPE],
        false,
    );
    assert_eq!(
        refuse_non_judge(&gateway).unwrap_err().code(),
        tonic::Code::PermissionDenied
    );

    // A person whose id happens to be the judge's principal string is still not a service.
    let user = VerifiedIdentity::user_for_test("org-1", "user-1");
    assert_eq!(
        refuse_non_judge(&user).unwrap_err().code(),
        tonic::Code::PermissionDenied
    );
    let spoof = VerifiedIdentity::user_for_test("org-1", JUDGE_CALLER);
    assert_eq!(
        refuse_non_judge(&spoof).unwrap_err().code(),
        tonic::Code::PermissionDenied
    );
}

#[test]
fn the_judge_read_scope_is_not_implied_by_the_write_scope() {
    let write_only = VerifiedIdentity::service_for_test_as(
        "org-1",
        JUDGE_CALLER,
        &[GOAL_VERDICT_WRITE_SCOPE],
        false,
    );
    assert_eq!(
        authorize_operation(&write_only, GOAL_JUDGE_READ_SCOPE)
            .unwrap_err()
            .code(),
        tonic::Code::PermissionDenied
    );
    assert!(authorize_operation(&write_only, GOAL_VERDICT_WRITE_SCOPE).is_ok());
    let read_only = VerifiedIdentity::service_for_test_as(
        "org-1",
        JUDGE_CALLER,
        &[GOAL_JUDGE_READ_SCOPE],
        false,
    );
    assert_eq!(
        authorize_operation(&read_only, GOAL_VERDICT_WRITE_SCOPE)
            .unwrap_err()
            .code(),
        tonic::Code::PermissionDenied
    );
}

#[test]
fn a_zdr_judge_credential_cannot_write_a_verdict() {
    let zdr_judge = VerifiedIdentity::service_for_test_as(
        "org-1",
        JUDGE_CALLER,
        &[GOAL_VERDICT_WRITE_SCOPE],
        true,
    );
    let refused = authorize_operation(&zdr_judge, GOAL_VERDICT_WRITE_SCOPE).unwrap_err();
    assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
    assert!(refuse_retention_exempt(&zdr_judge).is_err());
}

// ---- DB --------------------------------------------------------------------------------------

async fn connect() -> PgPool {
    let pool =
        PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required"))
            .await
            .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}

async fn seed_thread(pool: &PgPool) -> String {
    let thread = new_ulid();
    sqlx::query("INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, $3)")
        .bind(&thread)
        .bind(ORG)
        .bind(OWNER)
        .execute(pool)
        .await
        .unwrap();
    thread
}

/// A goal with one claimed, bound and settled turn (turn 1), ready for a verdict.
async fn settled_goal(pool: &PgPool) -> (pb::ThreadGoal, String) {
    let thread = seed_thread(pool).await;
    let goal = set_thread_goal(
        pool,
        &owner(),
        pb::SetThreadGoalRequest {
            thread_id: thread.clone(),
            org_id: ORG.to_owned(),
            objective: "Ship the quarterly report".to_owned(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .goal
    .unwrap();
    claim_goal_turn(
        pool,
        &owner(),
        pb::ClaimGoalTurnRequest {
            goal_id: goal.id.clone(),
            org_id: ORG.to_owned(),
            expected_revision: 0,
        },
    )
    .await
    .unwrap();
    let run_id = format!("run-{}", new_ulid());
    bind_goal_turn_run(
        pool,
        &owner(),
        pb::BindGoalTurnRunRequest {
            goal_id: goal.id.clone(),
            org_id: ORG.to_owned(),
            turn_no: 1,
            run_id: run_id.clone(),
        },
        false,
    )
    .await
    .unwrap();
    let goal = settle_goal_turn(
        pool,
        &owner(),
        pb::SettleGoalTurnRequest {
            goal_id: goal.id.clone(),
            org_id: ORG.to_owned(),
            turn_no: 1,
            run_id,
            outcome: "completed".to_owned(),
            metered_input_tokens: 100,
            metered_output_tokens: 0,
            cost_usd_micros: -1,
            cost_basis: "metered".to_owned(),
            judge_tokens: 0,
            usage_source: "run_response".to_owned(),
        },
    )
    .await
    .unwrap()
    .goal
    .unwrap();
    (goal, thread)
}

fn verdict(
    goal: &pb::ThreadGoal,
    verdict: &str,
    judge_tokens: u64,
) -> pb::RecordGoalVerdictRequest {
    pb::RecordGoalVerdictRequest {
        org_id: ORG.to_owned(),
        goal_id: goal.id.clone(),
        turn_no: 1,
        verdict: verdict.to_owned(),
        reasons: vec!["The report was shipped".to_owned()],
        evidence_ids: vec!["tool-call-1".to_owned()],
        judge_model: "judge-model".to_owned(),
        judge_tokens,
    }
}

type TurnVerdict = (
    Option<String>,
    Option<sqlx::types::Json<Vec<String>>>,
    Option<sqlx::types::Json<Vec<String>>>,
    i64,
);

async fn turn_verdict(pool: &PgPool, goal_id: &str) -> TurnVerdict {
    sqlx::query_as(
        "SELECT verdict, verdict_reasons, evidence_ids, judge_tokens::bigint
         FROM thread_goal_turns WHERE goal_id = $1 AND turn_no = 1",
    )
    .bind(goal_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
async fn verdict_met_closes_goal() {
    let pool = connect().await;
    let (goal, _) = settled_goal(&pool).await;
    assert_eq!(goal.status, "active");
    let judged = record_goal_verdict(&pool, &judge(), verdict(&goal, "met", 40))
        .await
        .unwrap()
        .goal
        .unwrap();
    assert_eq!(judged.status, "met");
    assert_eq!(
        judged.tokens_used,
        goal.tokens_used + 40,
        "the judge's tokens count"
    );
    let (stored, reasons, evidence, judge_tokens) = turn_verdict(&pool, &goal.id).await;
    assert_eq!(stored.as_deref(), Some("met"));
    // Content-free: the reasons and evidence ids are validated, never stored.
    assert!(reasons.is_none(), "verdict_reasons stays NULL");
    assert!(evidence.is_none(), "evidence_ids stays NULL");
    assert_eq!(judge_tokens, 40);
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
async fn verdict_not_met_leaves_the_goal_open_and_blocked_stops_it() {
    let pool = connect().await;
    let (goal, _) = settled_goal(&pool).await;
    let open = record_goal_verdict(&pool, &judge(), verdict(&goal, "not_met", 0))
        .await
        .unwrap()
        .goal
        .unwrap();
    assert_eq!(open.status, "active");

    let (goal, _) = settled_goal(&pool).await;
    let blocked = record_goal_verdict(&pool, &judge(), verdict(&goal, "blocked", 0))
        .await
        .unwrap()
        .goal
        .unwrap();
    assert_eq!(blocked.status, "blocked");
    assert_eq!(blocked.status_reason, JUDGE_BLOCKED);
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
async fn user_reopen_bumps_revision() {
    let pool = connect().await;
    let (goal, _) = settled_goal(&pool).await;
    let met = record_goal_verdict(&pool, &judge(), verdict(&goal, "met", 0))
        .await
        .unwrap()
        .goal
        .unwrap();
    assert_eq!(met.status, "met");
    let reopened = update_thread_goal(
        &pool,
        &owner(),
        pb::UpdateThreadGoalRequest {
            goal_id: met.id.clone(),
            org_id: ORG.to_owned(),
            expected_revision: met.revision,
            update_paths: vec!["status".to_owned()],
            status: "active".to_owned(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .goal
    .unwrap();
    assert_eq!(reopened.status, "active");
    assert_eq!(reopened.revision, met.revision + 1);
    // The stale revision the reopen used is now a conflict.
    let stale = update_thread_goal(
        &pool,
        &owner(),
        pb::UpdateThreadGoalRequest {
            goal_id: met.id.clone(),
            org_id: ORG.to_owned(),
            expected_revision: met.revision,
            update_paths: vec!["status".to_owned()],
            status: "paused".to_owned(),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert_eq!(stale.code(), tonic::Code::Aborted);
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
async fn a_repeated_verdict_on_a_judged_turn_changes_nothing() {
    let pool = connect().await;
    let (goal, _) = settled_goal(&pool).await;
    let first = record_goal_verdict(&pool, &judge(), verdict(&goal, "not_met", 25))
        .await
        .unwrap()
        .goal
        .unwrap();
    let again = record_goal_verdict(&pool, &judge(), verdict(&goal, "met", 999))
        .await
        .unwrap()
        .goal
        .unwrap();
    assert_eq!(again.status, "active", "the retry does not close the goal");
    assert_eq!(
        again.tokens_used, first.tokens_used,
        "the retry spends nothing"
    );
    assert_eq!(again.revision, first.revision);
    let (stored, _, _, judge_tokens) = turn_verdict(&pool, &goal.id).await;
    assert_eq!(stored.as_deref(), Some("not_met"));
    assert_eq!(judge_tokens, 25);
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
async fn a_verdict_on_a_thread_holding_a_waived_run_is_refused_and_changes_no_row() {
    let pool = connect().await;
    let (goal, thread) = settled_goal(&pool).await;
    insert_completed_run(
        &pool,
        ORG,
        OWNER,
        &thread,
        &RunRetention::waived("ack-1", "conn-1"),
    )
    .await;
    let before = turn_verdict(&pool, &goal.id).await;
    let goal_before: (String, i64, i64) = sqlx::query_as(
        "SELECT status, tokens_used, revision::bigint FROM thread_goals WHERE id = $1",
    )
    .bind(&goal.id)
    .fetch_one(&pool)
    .await
    .unwrap();

    let refused = record_goal_verdict(&pool, &judge(), verdict(&goal, "met", 50))
        .await
        .unwrap_err();
    assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
    let expected = mp_contracts::zdr_refusal::Refusal::DURABLE_MUTATION_FORBIDDEN
        .grpc_status(tonic::Code::FailedPrecondition);
    assert_eq!(refused.message(), expected.message());

    assert_eq!(
        turn_verdict(&pool, &goal.id).await,
        before,
        "the turn is unchanged"
    );
    let goal_after: (String, i64, i64) = sqlx::query_as(
        "SELECT status, tokens_used, revision::bigint FROM thread_goals WHERE id = $1",
    )
    .bind(&goal.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(goal_after, goal_before, "the goal is unchanged");
}
