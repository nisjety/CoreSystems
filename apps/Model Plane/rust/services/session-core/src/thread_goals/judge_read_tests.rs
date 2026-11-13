//! The judge's unattended read of a goal turn (`GetGoalTurnForJudge`, EP-7 S6, OD-58
//! [R2-OD-58-b]), a test-only child module of `thread_goals`.
//!
//! DB tier only (every test `#[ignore]`d, needs `DATABASE_URL` to a disposable Postgres with the
//! session-core migrations): a persistent goal thread answers the turn and its answer tail; a goal
//! thread or a fire thread that holds a zero-retention or waived run is refused with the closed
//! durable-mutation refusal, so neither the objective nor the answer leaves session-core.

use super::test_support::{held_receipts, insert_completed_run};
use super::*;
use crate::retention_receipt::RunRetention;

const ORG: &str = "org-goal-judge-read";
const OWNER: &str = "user-goal-judge-read";
const ANSWER: &str = "the release notes are collected";

fn judge() -> VerifiedIdentity {
    VerifiedIdentity::service_for_test_as(ORG, JUDGE_CALLER, &[GOAL_JUDGE_READ_SCOPE], false)
}

async fn connect() -> PgPool {
    let pool =
        PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required"))
            .await
            .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}

struct Fire {
    goal_thread: String,
    fire_thread: String,
    request: pb::GetGoalTurnForJudgeRequest,
}

async fn exec(pool: &PgPool, sql: &str, binds: &[&str]) {
    let mut query = sqlx::query(sql);
    for bind in binds {
        query = query.bind(*bind);
    }
    query.execute(pool).await.unwrap_or_else(|e| panic!("{sql}: {e}"));
}

/// A personal goal thread with an active goal, a schedule bound to it, one fire of that schedule
/// whose persistent run answered on its own scheduled thread, and the settled unattended turn the
/// fire ledgered.
async fn settled_fire(pool: &PgPool) -> Fire {
    let id = new_ulid().to_lowercase();
    let goal_thread = format!("judge-read-goal-thread-{id}");
    let fire_thread = format!("judge-read-fire-thread-{id}");
    let goal_id = format!("goal_{id}");
    let schedule_id = format!("judge-read-schedule-{id}");
    let run_id = format!("judge-read-run-{id}");
    let fire_key = format!("fire-{id}");
    exec(
        pool,
        "INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, $3)",
        &[&goal_thread, ORG, OWNER],
    )
    .await;
    exec(
        pool,
        "INSERT INTO thread_goals (id, thread_id, org_id, user_id, objective, max_turns, status)
         VALUES ($1, $2, $3, $4, 'collect the release notes', 10, 'active')",
        &[&goal_id, &goal_thread, ORG, OWNER],
    )
    .await;
    sqlx::query(
        "INSERT INTO cron_schedules
             (id, org_id, name, schedule_expr, created_by, creator_subject_id, task_template)
         VALUES ($1, $2, $1, '0 * * * *', $3, $3, $4)",
    )
    .bind(&schedule_id)
    .bind(ORG)
    .bind(OWNER)
    .bind(serde_json::json!({"kind": "cron", "purpose": "goal", "goal_id": goal_id}))
    .execute(pool)
    .await
    .unwrap();
    exec(
        pool,
        "INSERT INTO threads (id, session_key, org_id, user_id)
         VALUES ($1, $1, $2, 'service:orchestrator-core')",
        &[&fire_thread, ORG],
    )
    .await;
    exec(
        pool,
        "INSERT INTO runs (id, thread_id, goal, org_id, user_id, status, retention_class)
         VALUES ($1, $2, 'g', $3, 'service:orchestrator-core', 'completed', 'persistent')",
        &[&run_id, &fire_thread, ORG],
    )
    .await;
    exec(
        pool,
        "INSERT INTO cron_fires (id, schedule_id, status, fire_key, run_id, thread_id)
         VALUES ($1, $2, 'completed', $3, $4, $5)",
        &[&format!("fire-row-{id}"), &schedule_id, &fire_key, &run_id, &fire_thread],
    )
    .await;
    exec(
        pool,
        "INSERT INTO thread_goal_turns
             (goal_id, turn_no, run_id, org_id, claim_expires_at, settled_at, outcome, counted,
              unattended, scheduled_thread_id)
         VALUES ($1, 1, $2, $3, now() + interval '30 minutes', now(), 'completed', true, true, $4)",
        &[&goal_id, &run_id, ORG, &fire_thread],
    )
    .await;
    sqlx::query(
        "INSERT INTO messages (id, thread_id, role, content, metadata)
         VALUES ($1, $2, 'assistant', $3, $4)",
    )
    .bind(format!("judge-read-answer-{id}"))
    .bind(&fire_thread)
    .bind(ANSWER)
    .bind(serde_json::json!({"run_id": run_id, "source_scope": "scheduled_run"}))
    .execute(pool)
    .await
    .unwrap();
    Fire {
        goal_thread,
        fire_thread,
        request: pb::GetGoalTurnForJudgeRequest {
            org_id: ORG.to_owned(),
            goal_id,
            run_id,
            schedule_id,
            fire_key,
        },
    }
}

/// Hold `thread` with a completed run that carries `retention` as its receipt.
async fn hold(pool: &PgPool, thread: &str, retention: &RunRetention) {
    insert_completed_run(pool, ORG, OWNER, thread, retention).await;
}

fn assert_refused(refused: Status) {
    let expected = mp_contracts::zdr_refusal::Refusal::DURABLE_MUTATION_FORBIDDEN
        .grpc_status(tonic::Code::FailedPrecondition);
    assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
    assert_eq!(refused.message(), expected.message());
    assert!(
        !refused.message().contains(ANSWER) && !refused.message().contains("release notes"),
        "a refusal carries no objective and no answer"
    );
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
async fn a_persistent_fire_answers_the_turn_and_its_answer_tail() {
    let pool = connect().await;
    let fire = settled_fire(&pool).await;
    let read = get_goal_turn_for_judge(&pool, &judge(), fire.request.clone())
        .await
        .unwrap();
    assert!(read.found);
    assert_eq!(read.turn_no, 1);
    assert_eq!(read.outcome, "completed");
    assert_eq!(read.answer, ANSWER);
    assert_eq!(
        read.goal.unwrap().objective,
        "collect the release notes",
        "the fixture reaches the objective, so a refusal below is the thread check"
    );
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
async fn a_goal_thread_holding_a_zdr_or_waived_run_is_refused_before_any_read() {
    let pool = connect().await;
    for receipt in held_receipts() {
        let fire = settled_fire(&pool).await;
        hold(&pool, &fire.goal_thread, &receipt).await;
        let refused = get_goal_turn_for_judge(&pool, &judge(), fire.request.clone())
            .await
            .expect_err(receipt.class.as_str());
        assert_refused(refused);
    }
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
async fn a_fire_thread_holding_a_zdr_or_waived_run_is_refused_before_the_answer() {
    let pool = connect().await;
    for receipt in held_receipts() {
        let fire = settled_fire(&pool).await;
        hold(&pool, &fire.fire_thread, &receipt).await;
        let refused = get_goal_turn_for_judge(&pool, &judge(), fire.request.clone())
            .await
            .expect_err(receipt.class.as_str());
        assert_refused(refused);
    }
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
async fn a_judge_read_of_another_org_or_another_fire_finds_nothing() {
    let pool = connect().await;
    let fire = settled_fire(&pool).await;
    let other_fire = pb::GetGoalTurnForJudgeRequest {
        fire_key: "fire-not-this-one".to_owned(),
        ..fire.request.clone()
    };
    let read = get_goal_turn_for_judge(&pool, &judge(), other_fire)
        .await
        .unwrap();
    assert!(!read.found);
    assert!(read.goal.is_none() && read.answer.is_empty());

    let other_org = VerifiedIdentity::service_for_test_as(
        "org-someone-else",
        JUDGE_CALLER,
        &[GOAL_JUDGE_READ_SCOPE],
        false,
    );
    let refused = get_goal_turn_for_judge(&pool, &other_org, fire.request.clone())
        .await
        .unwrap_err();
    assert_eq!(refused.code(), tonic::Code::PermissionDenied);
}
