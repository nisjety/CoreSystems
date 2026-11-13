//! Input-pause fold tests [W2-WS14-S5-b3], a test-only child module of `loop_checkpoint`.
//!
//! * Pure tier: only a `pause` save under an input-pause reference is stamped; the claim's three
//!   new branches (claimed elsewhere, thread busy, thread moved) are reached only through
//!   `input_pause_claim`, which an approval reference and a park claim never set.
//! * DB tier (every test `#[ignore]`d, needs `DATABASE_URL` to a scratch Postgres whose name
//!   contains `test`): an input fold save is stamped and an approval fold save is not; a claim
//!   after a newer message closes the fold with `loop_checkpoint_thread_moved`; a second token
//!   during a live lease gets `loop_checkpoint_claimed_elsewhere` and the same token
//!   `loop_checkpoint_claim_held`; a claim after the lease lapsed succeeds; an approval fold claim
//!   is unchanged by a newer message.

use super::test_support::{cipher, fold_for, fold_state, scratch_pool, seed_run};
use super::*;

#[test]
fn only_a_pause_under_an_input_reference_is_an_input_pause_save() {
    assert!(is_input_pause_save("pause", "run-1:step-1:input"));
    assert!(is_input_pause_save("pause", "run-1:step-1:elicit:1"));
    assert!(
        !is_input_pause_save("pause", "run-1:step-1"),
        "an approval reference"
    );
    assert!(!is_input_pause_save("pause", "appr-1"));
    assert!(!is_input_pause_save("pause", ""));
    assert!(!is_input_pause_save("park", "park:run-1:1"));
    assert!(
        !is_input_pause_save("round", "run-1:step-1:input"),
        "only a pause"
    );
    assert!(!is_input_pause_save("pre_effect", "run-1:step-1:input"));
}

fn claim_body() -> String {
    let source = include_str!("loop_checkpoint.rs").replace("\r\n", "\n");
    let start = source
        .find("pub(crate) async fn claim(")
        .expect("the claim function");
    let body = &source[start..];
    body[..body.find("\n}\n").expect("end of claim")].to_owned()
}

#[test]
fn the_new_claim_branches_run_only_for_an_input_pause_reference() {
    let body = claim_body();
    assert!(body.contains(
        "let input_pause_claim =\n        !park_claim && mp_contracts::user_question::is_input_pause_ref(pause_ref);"
    ));
    for code in [
        "loop_checkpoint_claimed_elsewhere",
        "loop_checkpoint_thread_busy",
        "loop_checkpoint_thread_moved",
    ] {
        assert_eq!(body.matches(code).count(), 1, "{code} is written once");
        let at = body.find(code).unwrap();
        let gate = body[..at]
            .rfind("if input_pause_claim {")
            .unwrap_or_else(|| panic!("{code} sits under the input-pause gate"));
        // No other `if` block closes between the gate and the code at the gate's own depth: the
        // gate is the nearest enclosing input-pause block, not one that ended earlier.
        let between = &body[gate..at];
        let opens = between.matches('{').count();
        let closes = between.matches('}').count();
        assert!(
            opens > closes,
            "{code} is inside `if input_pause_claim {{ ... }}`"
        );
    }
    // The park branch keeps its committed answers.
    let park = body
        .find("if park_claim {\n        if let Some(stamp)")
        .expect("the park branch");
    let park_branch = &body[park..park + 700];
    assert!(park_branch.contains("Status::aborted(\"loop_checkpoint_claim_held\")"));
    assert!(park_branch.contains("Status::failed_precondition(\"loop_checkpoint_not_claimable\")"));
}

// ---- DB --------------------------------------------------------------------------------------

async fn save_pause(pool: &PgPool, run_id: &str, pause_ref: &str) -> i64 {
    let fold = fold_for(run_id);
    save(
        pool,
        &cipher(),
        SaveInput {
            run_id,
            expected_seq: 0,
            kind: "pause",
            fold: &fold,
            authority_digest: "auth-1",
            pause_ref,
            park_reason: "",
            park_until: None,
        },
    )
    .await
    .expect("the pause fold is stored")
    .seq
}

async fn post_message(pool: &PgPool, run_id: &str) {
    sqlx::query(
        "INSERT INTO messages (id, thread_id, role, content)
         SELECT $1, thread_id, 'user', 'a newer turn' FROM runs WHERE id = $2",
    )
    .bind(mp_ids::new_ulid())
    .bind(run_id)
    .execute(pool)
    .await
    .expect("a newer message");
}

fn stamp_of(claimed: &Claimed) -> Option<i64> {
    let value: serde_json::Value = serde_json::from_slice(&claimed.fold).expect("fold json");
    value["extensions"][THREAD_WATERMARK_KEY].as_i64()
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a scratch Postgres"]
async fn an_input_fold_save_is_stamped_and_an_approval_fold_save_is_not() {
    let Some(pool) = scratch_pool().await else {
        return;
    };
    let (run_id, _, _) = seed_run(&pool, "persistent").await;
    post_message(&pool, &run_id).await;
    let input_ref = format!("{run_id}:step-1:input");
    save_pause(&pool, &run_id, &input_ref).await;
    let claimed = claim(&pool, &cipher(), &run_id, &input_ref, "token-a")
        .await
        .expect("an unmoved thread is claimable");
    let position: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(sequence), 0)::bigint FROM messages
          WHERE thread_id = (SELECT thread_id FROM runs WHERE id = $1)",
    )
    .bind(&run_id)
    .fetch_one(&pool)
    .await
    .expect("position");
    assert_eq!(
        stamp_of(&claimed),
        Some(position),
        "stamped at the thread's position"
    );

    let (approval_run, _, _) = seed_run(&pool, "persistent").await;
    let approval_ref = format!("{approval_run}:step-1");
    save_pause(&pool, &approval_run, &approval_ref).await;
    let claimed = claim(&pool, &cipher(), &approval_run, &approval_ref, "token-a")
        .await
        .expect("claim");
    assert_eq!(
        stamp_of(&claimed),
        None,
        "an approval fold is stored as it came"
    );
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a scratch Postgres"]
async fn a_claim_after_a_newer_message_closes_the_input_fold_as_thread_moved() {
    let Some(pool) = scratch_pool().await else {
        return;
    };
    let (run_id, _, _) = seed_run(&pool, "persistent").await;
    let input_ref = format!("{run_id}:step-1:input");
    save_pause(&pool, &run_id, &input_ref).await;
    post_message(&pool, &run_id).await;
    let refused = claim(&pool, &cipher(), &run_id, &input_ref, "token-a")
        .await
        .err()
        .expect("the thread moved");
    assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
    assert_eq!(refused.message(), "loop_checkpoint_thread_moved");
    assert_eq!(fold_state(&pool, &run_id).await.as_deref(), Some("closed"));
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a scratch Postgres"]
async fn a_second_token_during_a_live_lease_is_told_claimed_elsewhere_and_wins_after_it_lapses() {
    let Some(pool) = scratch_pool().await else {
        return;
    };
    let (run_id, _, _) = seed_run(&pool, "persistent").await;
    let input_ref = format!("{run_id}:step-1:input");
    save_pause(&pool, &run_id, &input_ref).await;
    claim(&pool, &cipher(), &run_id, &input_ref, "token-a")
        .await
        .expect("the first claim");
    let elsewhere = claim(&pool, &cipher(), &run_id, &input_ref, "token-b")
        .await
        .err()
        .expect("held by another continuation");
    assert_eq!(elsewhere.code(), tonic::Code::Aborted);
    assert_eq!(elsewhere.message(), "loop_checkpoint_claimed_elsewhere");
    let held = claim(&pool, &cipher(), &run_id, &input_ref, "token-a")
        .await
        .err()
        .expect("the same token");
    assert_eq!(held.message(), "loop_checkpoint_claim_held");

    sqlx::query(
        "UPDATE run_loop_checkpoints SET claim_expires_at = now() - interval '1 second' WHERE run_id = $1",
    )
    .bind(&run_id)
    .execute(&pool)
    .await
    .expect("lapse the lease");
    claim(&pool, &cipher(), &run_id, &input_ref, "token-b")
        .await
        .expect("a lapsed lease is claimable");
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a scratch Postgres"]
async fn an_approval_fold_claim_is_unchanged_by_a_newer_message_or_a_second_token() {
    let Some(pool) = scratch_pool().await else {
        return;
    };
    let (run_id, _, _) = seed_run(&pool, "persistent").await;
    let approval_ref = format!("{run_id}:step-1");
    save_pause(&pool, &run_id, &approval_ref).await;
    post_message(&pool, &run_id).await;
    claim(&pool, &cipher(), &run_id, &approval_ref, "token-a")
        .await
        .expect("an approval claim reads no thread position");
    let other = claim(&pool, &cipher(), &run_id, &approval_ref, "token-b")
        .await
        .err()
        .expect("held");
    assert_eq!(other.code(), tonic::Code::FailedPrecondition);
    assert_eq!(
        other.message(),
        "loop_checkpoint_not_claimable",
        "a second token on an approval pause keeps the committed answer"
    );
}
