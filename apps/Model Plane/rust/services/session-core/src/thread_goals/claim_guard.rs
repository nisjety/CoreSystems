//! The pump guard on a goal turn claim (Wave 3 EP-7 S4, owner decisions D4 and D5).
//!
//! The goal bar's "Keep going" pump sends the next continuation turn from the browser (D4), and a
//! goal never auto-continues across a decision that waits on its owner (D5). The browser stops on
//! its own, but the rule must hold for a stale tab or a hostile client too, so the claim that
//! admits every goal turn (`ClaimGoalTurn`, EP-7 S2) runs [`refuse_unsettled_claim`] inside its
//! transaction, after it has locked the goal row and before it numbers a turn. A claim is refused:
//!
//! - `failed_precondition` `thread_goal_pending_decision` while the thread holds an undecided
//!   approval (a `browser_human_assist` takeover included), an unanswered question, a plan in
//!   `proposed`, or a parked run. session-core reads its own tables, so no other plane is asked.
//! - `resource_exhausted` `thread_goal_rate_limited` once the goal has made
//!   [`MAX_CLAIMS_PER_HOUR`] claims in the last hour, whatever sent them.
//!
//! The guard holds for every claim, a person's Continue as well: the claim carries no field that
//! says who sent it, and one that did could be forged. Nothing here grants anything; the pump never
//! approves, answers or resumes (D5).
//!
//! A turn the pump sent is marked for audit: the gateway sends the claim's run binding with the
//! [`GOAL_AUTO_HEADER`] request header, and the binding stamps `runs.metadata.goal_auto` with
//! [`stamp_goal_auto`]. The mark grants nothing and is read by nothing that decides.

use sqlx::{Postgres, Transaction};
use tonic::Status;

use super::internal;

/// The refusal while a decision waits on the owner (D5). Strings, as the other goal refusals are.
pub(crate) const PENDING_DECISION: &str = "thread_goal_pending_decision";
/// The refusal past the claim rate.
pub(crate) const RATE_LIMITED: &str = "thread_goal_rate_limited";
/// D5 default: at most this many claims per goal in any hour. Ten turns a goal take well under it;
/// a loop that settles turns as fast as it can is stopped by it.
pub(crate) const MAX_CLAIMS_PER_HOUR: i64 = 30;

/// The gRPC request header that marks a goal turn as sent by the pump. Only the literal `true`
/// counts, as with the switches.
pub(crate) const GOAL_AUTO_HEADER: &str = "x-goal-auto";

/// What waits on the owner, in the order the guard names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PendingDecision {
    Approval,
    Question,
    ProposedPlan,
    ParkedRun,
}

impl PendingDecision {
    fn from_token(token: &str) -> Option<Self> {
        match token {
            "approval" => Some(Self::Approval),
            "question" => Some(Self::Question),
            "plan" => Some(Self::ProposedPlan),
            "parked" => Some(Self::ParkedRun),
            _ => None,
        }
    }
}

/// The first decision that waits on the owner of a thread, as one token, or NULL when none does.
/// An approval or a question that has expired waits on nobody. A park counts while it is parked or
/// being resumed: a resumed run is still the run in flight.
const PENDING_DECISION_SQL: &str = "SELECT CASE
        WHEN EXISTS (
            SELECT 1 FROM approvals a JOIN runs r ON r.id = a.run_id
             WHERE r.thread_id = $1
               AND a.status = 'requested'
               AND (a.expires_at IS NULL OR a.expires_at > now())) THEN 'approval'
        WHEN EXISTS (
            SELECT 1 FROM run_questions q
             WHERE q.thread_id = $1
               AND q.state = 'requested'
               AND q.expires_at > now()) THEN 'question'
        WHEN EXISTS (
            SELECT 1 FROM plans p
             WHERE p.thread_id = $1 AND p.status = 'proposed') THEN 'plan'
        WHEN EXISTS (
            SELECT 1 FROM run_loop_checkpoints c JOIN runs r ON r.id = c.run_id
             WHERE r.thread_id = $1
               AND c.kind = 'park'
               AND c.state IN ('parked', 'resuming')) THEN 'parked'
    END";

/// The claims a goal made in the last hour. The claim ledger is `thread_goal_turns` (EP-7 S2): a
/// refused claim writes no row, so only admitted turns count.
const CLAIMS_LAST_HOUR_SQL: &str = "SELECT COUNT(*) FROM thread_goal_turns
     WHERE goal_id = $1 AND claimed_at > now() - interval '1 hour'";

const STAMP_GOAL_AUTO_SQL: &str = "UPDATE runs
        SET metadata = COALESCE(metadata, '{}'::jsonb) || '{\"goal_auto\": true}'::jsonb,
            updated_at = now()
      WHERE id = $1 AND org_id = $2";

fn pending_refusal(pending: Option<PendingDecision>) -> Result<(), Status> {
    match pending {
        Some(_) => Err(Status::failed_precondition(PENDING_DECISION)),
        None => Ok(()),
    }
}

fn rate_refusal(claims_last_hour: i64) -> Result<(), Status> {
    if claims_last_hour >= MAX_CLAIMS_PER_HOUR {
        return Err(Status::resource_exhausted(RATE_LIMITED));
    }
    Ok(())
}

/// The first decision that waits on the owner of `thread_id`, if any.
pub(crate) async fn pending_decision(
    tx: &mut Transaction<'_, Postgres>,
    thread_id: &str,
) -> Result<Option<PendingDecision>, Status> {
    let token: Option<String> = sqlx::query_scalar(PENDING_DECISION_SQL)
        .bind(thread_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(internal)?;
    Ok(token.as_deref().and_then(PendingDecision::from_token))
}

/// The D5 guard and the claim rate, in that order. Call it in the claim's transaction once the goal
/// row is locked (`FOR UPDATE`), so two claims on one goal cannot both read the same count.
pub(crate) async fn refuse_unsettled_claim(
    tx: &mut Transaction<'_, Postgres>,
    thread_id: &str,
    goal_id: &str,
) -> Result<(), Status> {
    pending_refusal(pending_decision(tx, thread_id).await?)?;
    let claims: i64 = sqlx::query_scalar(CLAIMS_LAST_HOUR_SQL)
        .bind(goal_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(internal)?;
    rate_refusal(claims)
}

/// Whether the caller marked the request as sent by the pump.
pub(crate) fn goal_auto_requested(metadata: &tonic::metadata::MetadataMap) -> bool {
    metadata
        .get(GOAL_AUTO_HEADER)
        .and_then(|value| value.to_str().ok())
        == Some("true")
}

/// Marks the bound run as a pump-sent goal turn (`runs.metadata.goal_auto = true`) for audit.
pub(crate) async fn stamp_goal_auto(
    tx: &mut Transaction<'_, Postgres>,
    run_id: &str,
    org_id: &str,
) -> Result<(), Status> {
    sqlx::query(STAMP_GOAL_AUTO_SQL)
        .bind(run_id)
        .bind(org_id)
        .execute(&mut **tx)
        .await
        .map_err(internal)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::VerifiedIdentity;
    use mp_contracts::model_plane::v1 as pb;
    use mp_ids::new_ulid;
    use sqlx::PgPool;

    #[test]
    fn claim_guard_pending_decision_refuses_typed() {
        assert!(pending_refusal(None).is_ok());
        for pending in [
            PendingDecision::Approval,
            PendingDecision::Question,
            PendingDecision::ProposedPlan,
            PendingDecision::ParkedRun,
        ] {
            let refused = pending_refusal(Some(pending)).unwrap_err();
            assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
            assert_eq!(refused.message(), PENDING_DECISION);
        }
    }

    #[test]
    fn claim_guard_tokens_are_closed() {
        assert_eq!(PendingDecision::from_token("approval"), Some(PendingDecision::Approval));
        assert_eq!(PendingDecision::from_token("question"), Some(PendingDecision::Question));
        assert_eq!(PendingDecision::from_token("plan"), Some(PendingDecision::ProposedPlan));
        assert_eq!(PendingDecision::from_token("parked"), Some(PendingDecision::ParkedRun));
        assert_eq!(PendingDecision::from_token("granted"), None);
        // Every token the query can answer is one the guard reads.
        for token in ["'approval'", "'question'", "'plan'", "'parked'"] {
            assert!(PENDING_DECISION_SQL.contains(token), "{token}");
        }
    }

    #[test]
    fn claim_guard_reads_only_live_decisions() {
        // An expired approval or question waits on nobody, and a plan counts only while proposed.
        assert!(PENDING_DECISION_SQL.contains("a.status = 'requested'"));
        assert!(PENDING_DECISION_SQL.contains("a.expires_at IS NULL OR a.expires_at > now()"));
        assert!(PENDING_DECISION_SQL.contains("q.state = 'requested'"));
        assert!(PENDING_DECISION_SQL.contains("q.expires_at > now()"));
        assert!(PENDING_DECISION_SQL.contains("p.status = 'proposed'"));
        assert!(PENDING_DECISION_SQL.contains("c.state IN ('parked', 'resuming')"));
    }

    #[test]
    fn claim_guard_rate_allows_thirty_an_hour() {
        assert!(rate_refusal(0).is_ok());
        assert!(rate_refusal(MAX_CLAIMS_PER_HOUR - 1).is_ok());
        let refused = rate_refusal(MAX_CLAIMS_PER_HOUR).unwrap_err();
        assert_eq!(refused.code(), tonic::Code::ResourceExhausted);
        assert_eq!(refused.message(), RATE_LIMITED);
        assert!(CLAIMS_LAST_HOUR_SQL.contains("interval '1 hour'"));
    }

    #[test]
    fn goal_auto_header_reads_literal_true_only() {
        let mut metadata = tonic::metadata::MetadataMap::new();
        assert!(!goal_auto_requested(&metadata));
        for value in ["1", "TRUE", "yes", ""] {
            metadata.insert(GOAL_AUTO_HEADER, value.parse().unwrap());
            assert!(!goal_auto_requested(&metadata), "{value}");
        }
        metadata.insert(GOAL_AUTO_HEADER, "true".parse().unwrap());
        assert!(goal_auto_requested(&metadata));
        // The mark is an audit field only: it merges into the run's metadata and nothing else.
        assert!(STAMP_GOAL_AUTO_SQL.contains("\"goal_auto\": true"));
        assert!(STAMP_GOAL_AUTO_SQL.contains("WHERE id = $1 AND org_id = $2"));
    }

    // The database tests below run with `--ignored` against a disposable Postgres, as the other
    // session-core database tests do. They call the guard the claim runs, in a transaction of
    // their own; the claim's own tests (EP-7 S2) cover the turn it then numbers.

    const ORG: &str = "org-goal-pump";
    const OWNER: &str = "user-goal-pump";

    struct Seeded {
        pool: PgPool,
        thread: String,
        run: String,
        goal: String,
    }

    async fn seeded() -> Seeded {
        let pool =
            PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required"))
                .await
                .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let thread = new_ulid();
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, $3)",
        )
        .bind(&thread)
        .bind(ORG)
        .bind(OWNER)
        .execute(&pool)
        .await
        .unwrap();
        let run = new_ulid();
        sqlx::query(
            "INSERT INTO runs (id, thread_id, goal, org_id, user_id) VALUES ($1, $2, 'g', $3, $4)",
        )
        .bind(&run)
        .bind(&thread)
        .bind(ORG)
        .bind(OWNER)
        .execute(&pool)
        .await
        .unwrap();
        let goal = super::super::set_thread_goal(
            &pool,
            &VerifiedIdentity::user_for_test(ORG, OWNER),
            pb::SetThreadGoalRequest {
                thread_id: thread.clone(),
                org_id: ORG.to_owned(),
                objective: "Find three suppliers".to_owned(),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .goal
        .unwrap()
        .id;
        Seeded {
            pool,
            thread,
            run,
            goal,
        }
    }

    async fn guard(seed: &Seeded) -> Result<(), Status> {
        let mut tx = seed.pool.begin().await.unwrap();
        let result = refuse_unsettled_claim(&mut tx, &seed.thread, &seed.goal).await;
        tx.rollback().await.unwrap();
        result
    }

    fn assert_pending(result: Result<(), Status>) {
        let refused = result.unwrap_err();
        assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
        assert_eq!(refused.message(), PENDING_DECISION);
    }

    async fn seed_approval(seed: &Seeded, kind: &str, expires_in_secs: f64) {
        sqlx::query(
            "INSERT INTO approvals (id, run_id, kind, status, org_id, user_id, expires_at)
             VALUES ($1, $2, $3, 'requested', $4, $5, now() + make_interval(secs => $6))",
        )
        .bind(new_ulid())
        .bind(&seed.run)
        .bind(kind)
        .bind(ORG)
        .bind(OWNER)
        .bind(expires_in_secs)
        .execute(&seed.pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn claim_refused_with_pending_approval() {
        let seed = seeded().await;
        assert!(guard(&seed).await.is_ok(), "nothing waits on the owner yet");
        // An expired approval waits on nobody.
        seed_approval(&seed, "tool", -60.0).await;
        assert!(guard(&seed).await.is_ok());
        seed_approval(&seed, "tool", 600.0).await;
        assert_pending(guard(&seed).await);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn claim_refused_while_a_takeover_waits() {
        // Round B C-B5: a goal turn waiting on a browser takeover is a pending approval, so the
        // pump does not send past it; its expiry reaches the goal as a failed turn (EP-7 S2).
        let seed = seeded().await;
        seed_approval(&seed, "browser_human_assist", 1_800.0).await;
        assert_pending(guard(&seed).await);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn claim_refused_with_unanswered_question() {
        let seed = seeded().await;
        sqlx::query(
            "INSERT INTO run_questions (id, run_id, thread_id, org_id, user_id, spec, expires_at)
             VALUES ($1, $2, $3, $4, $5, '[]'::jsonb, now() + interval '1 hour')",
        )
        .bind(format!("q_{}", new_ulid()))
        .bind(&seed.run)
        .bind(&seed.thread)
        .bind(ORG)
        .bind(OWNER)
        .execute(&seed.pool)
        .await
        .unwrap();
        assert_pending(guard(&seed).await);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn claim_refused_with_proposed_plan() {
        let seed = seeded().await;
        let plan = new_ulid();
        sqlx::query(
            "INSERT INTO plans (id, thread_id, run_id, status, org_id, user_id)
             VALUES ($1, $2, $3, 'proposed', $4, $5)",
        )
        .bind(&plan)
        .bind(&seed.thread)
        .bind(&seed.run)
        .bind(ORG)
        .bind(OWNER)
        .execute(&seed.pool)
        .await
        .unwrap();
        assert_pending(guard(&seed).await);
        // A plan the owner decided no longer waits.
        sqlx::query("UPDATE plans SET status = 'approved' WHERE id = $1")
            .bind(&plan)
            .execute(&seed.pool)
            .await
            .unwrap();
        assert!(guard(&seed).await.is_ok());
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn claim_refused_with_parked_run() {
        let seed = seeded().await;
        sqlx::query(
            "INSERT INTO run_loop_checkpoints
                (run_id, org_id, user_id, seq, kind, state, pause_ref, park_reason,
                 park_until, authority_digest, segment_no, ciphertext, fold_digest)
             VALUES ($1, $2, $3, 1, 'park', 'parked', $4, 'upstream_unavailable',
                     now() + interval '1 minute', 'auth-1', 1, 'x', $5)",
        )
        .bind(&seed.run)
        .bind(ORG)
        .bind(OWNER)
        .bind(format!("park:{}:1", seed.run))
        .bind("0".repeat(64))
        .execute(&seed.pool)
        .await
        .unwrap();
        assert_pending(guard(&seed).await);
    }

    /// A claim row of the S2 ledger, made `minutes_ago` minutes ago.
    async fn claim_at(seed: &Seeded, turn_no: i64, minutes_ago: i32) {
        sqlx::query(
            "INSERT INTO thread_goal_turns (goal_id, turn_no, org_id, claimed_at, claim_expires_at)
             VALUES ($1, $2, $3, now() - make_interval(mins => $4), now())",
        )
        .bind(&seed.goal)
        .bind(i32::try_from(turn_no).unwrap())
        .bind(ORG)
        .bind(minutes_ago)
        .execute(&seed.pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn claim_rate_limited() {
        let seed = seeded().await;
        // One claim older than the hour, which no longer counts, then 29 inside it.
        claim_at(&seed, 1, 61).await;
        for turn_no in 2..=MAX_CLAIMS_PER_HOUR {
            claim_at(&seed, turn_no, 1).await;
        }
        assert!(guard(&seed).await.is_ok(), "29 claims in the hour");
        claim_at(&seed, MAX_CLAIMS_PER_HOUR + 1, 0).await;
        let refused = guard(&seed).await.unwrap_err();
        assert_eq!(refused.code(), tonic::Code::ResourceExhausted);
        assert_eq!(refused.message(), RATE_LIMITED);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn goal_auto_stamp_marks_the_run_only() {
        let seed = seeded().await;
        let mut tx = seed.pool.begin().await.unwrap();
        stamp_goal_auto(&mut tx, &seed.run, "another-org").await.unwrap();
        stamp_goal_auto(&mut tx, &seed.run, ORG).await.unwrap();
        tx.commit().await.unwrap();
        let marked: Option<bool> =
            sqlx::query_scalar("SELECT (metadata->>'goal_auto')::boolean FROM runs WHERE id = $1")
                .bind(&seed.run)
                .fetch_one(&seed.pool)
                .await
                .unwrap();
        assert_eq!(marked, Some(true));
    }
}
