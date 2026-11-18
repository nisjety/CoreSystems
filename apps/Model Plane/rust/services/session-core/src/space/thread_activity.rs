//! Thread activity summary: backfill and pending probe (build plan WS15-S5).
//!
//! Migration 0078 adds the summary columns to `threads`, a trigger on `messages` that keeps
//! them current for every new message, and `thread_activity_recompute(thread_id)`. It does not
//! fill the threads that already existed: a full-table pass would block startup. This task
//! fills them in bounded batches, outside any migration, and [`backfill_pending`] tells a
//! reader (`ListSpaceTimeline`, WS15-S6; the attention read, WS11-S2) whether any thread that
//! has messages is still unfilled, so it can refuse with `summary_backfill_pending` instead of
//! reading a half-filled table.
//!
//! The summary holds ids, counts and a role word only, so the backfill reads and writes no
//! message content and needs no retention decision. It is idempotent and safe to run on every
//! replica at once: each rebuild takes the thread row lock first.

use std::time::Duration;

use anyhow::Result;
use sqlx::PgPool;
use tracing::{info, warn};

/// Threads rebuilt per transaction.
pub(crate) const BACKFILL_BATCH: i64 = 500;
/// Pause between batches, so the backfill never starves appenders.
const BATCH_PAUSE: Duration = Duration::from_millis(50);
/// Pause before retrying after a failed batch.
const RETRY_PAUSE: Duration = Duration::from_secs(5);

/// Rebuilds one batch of unfilled threads that have messages. Returns how many were rebuilt;
/// zero means the backfill is complete.
pub(crate) async fn backfill_batch(pool: &PgPool, batch: i64) -> Result<u64> {
    let rebuilt = sqlx::query(
        "SELECT thread_activity_recompute(b.id)
           FROM (SELECT t.id FROM threads t
                  WHERE t.last_message_seq IS NULL
                    AND EXISTS (SELECT 1 FROM messages m WHERE m.thread_id = t.id)
                  ORDER BY t.id
                  LIMIT $1) b",
    )
    .bind(batch)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(rebuilt)
}

/// True while any thread that has messages still has no summary. Readers of the summary refuse
/// while this is true.
#[allow(dead_code)] // read by ListSpaceTimeline (WS15-S6) and the attention read (WS11-S2)
pub(crate) async fn backfill_pending(pool: &PgPool) -> Result<bool> {
    let pending: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM threads t
                         WHERE t.last_message_seq IS NULL
                           AND EXISTS (SELECT 1 FROM messages m WHERE m.thread_id = t.id))",
    )
    .fetch_one(pool)
    .await?;
    Ok(pending)
}

/// The backfill loop: batches until none is left, then returns. A failed batch is retried after
/// a pause and never ends the task early; progress is kept because every batch commits alone.
pub(crate) async fn run_backfill(pool: PgPool) {
    let mut total: u64 = 0;
    loop {
        match backfill_batch(&pool, BACKFILL_BATCH).await {
            Ok(0) => {
                info!(threads = total, "thread activity backfill complete");
                return;
            }
            Ok(rebuilt) => {
                total += rebuilt;
                tokio::time::sleep(BATCH_PAUSE).await;
            }
            Err(error) => {
                warn!(%error, "thread activity backfill batch failed; retrying");
                tokio::time::sleep(RETRY_PAUSE).await;
            }
        }
    }
}

/// Tests of the summary trigger, the recompute function and the backfill (build plan WS15-S5). Every
/// test is `#[ignore]`d and needs `DATABASE_URL` to a disposable Postgres whose database name
/// contains `test`: `cargo test -p session-core --bin session-core space::thread_activity --
/// --ignored --test-threads=1`.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::VerifiedIdentity;
    use crate::grpc::space_w1_tests::{mint, StreamKeysEnv, DECISION_REF};
    use crate::space::test_support::{clean_space, connect, seed_space_thread};
    use mp_contracts::model_plane::v1 as pb;
    use serde_json::json;
    use tonic::{Request, Status};

    const NEEDS_DB: &str = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)";

    /// `(last_message_seq, turn_count, last_role, last_author_subject_id, recent_author_subject_ids,
    /// activity_max_audience_revision)` of one thread.
    type Summary = (Option<i64>, i32, Option<String>, Option<String>, Vec<String>, Option<i64>);

    async fn summary(pool: &PgPool, thread: &str) -> Summary {
        sqlx::query_as(
            "SELECT last_message_seq, turn_count, last_role, last_author_subject_id, recent_author_subject_ids, \
             activity_max_audience_revision FROM threads WHERE id = $1",
        )
        .bind(thread)
        .fetch_one(pool)
        .await
        .expect("read the summary")
    }

    fn expect(
        last_message_seq: i64,
        turn_count: i32,
        last_role: &str,
        last_author: Option<&str>,
        recent: &[&str],
        max_revision: i64,
    ) -> Summary {
        (
            Some(last_message_seq),
            turn_count,
            Some(last_role.to_owned()),
            last_author.map(str::to_owned),
            recent.iter().map(|author| (*author).to_owned()).collect(),
            Some(max_revision),
        )
    }

    /// Appends one Space message at audience `revision` and returns its sequence.
    async fn add(pool: &PgPool, space: &str, thread: &str, role: &str, author: Option<&str>, revision: i64) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO messages (id, thread_id, role, content, space_id, recipient_audience_ref, \
             recipient_audience_revision, recipient_audience_hash, authority_revision, \
             resource_authorization_ref, author_subject_id) \
             VALUES ($1, $2, $3, 'canary body', $4, 'audience-1', $5, 'sha256:audience-1', 1, 'resource-1', $6) \
             RETURNING sequence",
        )
        .bind(mp_ids::new_ulid())
        .bind(thread)
        .bind(role)
        .bind(space)
        .bind(revision)
        .bind(author)
        .fetch_one(pool)
        .await
        .expect("append a message")
    }

    /// Puts a thread back in the state of one that predates migration 0078: messages, no summary.
    async fn forget_summary(pool: &PgPool, thread: &str) {
        sqlx::query(
            "UPDATE threads SET last_message_seq = NULL, turn_count = 0, last_role = NULL, \
             last_author_subject_id = NULL, recent_author_subject_ids = '{}', activity_max_audience_revision = NULL \
             WHERE id = $1",
        )
        .bind(thread)
        .execute(pool)
        .await
        .expect("forget the summary");
    }

    async fn recompute(pool: &PgPool, thread: &str) {
        sqlx::query("SELECT thread_activity_recompute($1)")
            .bind(thread)
            .execute(pool)
            .await
            .expect("recompute");
    }

    fn ids(prefix: &str) -> (String, String, String) {
        let suffix = mp_ids::new_ulid();
        (format!("org-{prefix}-{suffix}"), format!("space-{prefix}-{suffix}"), suffix)
    }

    #[test]
    fn the_backfill_batch_is_bounded() {
        assert_eq!(BACKFILL_BATCH, 500);
        assert!(BATCH_PAUSE >= Duration::from_millis(1), "a pause between batches keeps appenders unstarved");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn trigger_maintains_summary_across_roles_and_authors() {
        let Some(pool) = connect().await else { return };
        let (org, space, suffix) = ids("act");
        let thread = format!("thread-{suffix}");
        seed_space_thread(&pool, &org, &space, &thread, "owner-1").await;
        assert_eq!(
            summary(&pool, &thread).await,
            (None, 0, None, None, Vec::new(), None),
            "a thread with no message has no summary, {NEEDS_DB}"
        );

        // The first message of a thread rebuilds its summary from the table.
        let s1 = add(&pool, &space, &thread, "user", Some("alice"), 1).await;
        assert_eq!(summary(&pool, &thread).await, expect(s1, 1, "user", Some("alice"), &["alice"], 1));
        // An assistant row is a turn; it has no author and leaves the last author alone.
        let s2 = add(&pool, &space, &thread, "assistant", None, 2).await;
        assert_eq!(summary(&pool, &thread).await, expect(s2, 2, "assistant", Some("alice"), &["alice"], 2));
        // A second author moves to the front; a lower revision never lowers the maximum.
        let s3 = add(&pool, &space, &thread, "user", Some("bob"), 1).await;
        assert_eq!(summary(&pool, &thread).await, expect(s3, 3, "user", Some("bob"), &["bob", "alice"], 2));
        // A tool row is not a turn, but it is the newest row and carries the highest revision.
        let s4 = add(&pool, &space, &thread, "tool", None, 3).await;
        assert_eq!(summary(&pool, &thread).await, expect(s4, 3, "tool", Some("bob"), &["bob", "alice"], 3));
        // An author who speaks again moves back to the front, and is not listed twice.
        let s5 = add(&pool, &space, &thread, "user", Some("alice"), 1).await;
        assert_eq!(summary(&pool, &thread).await, expect(s5, 4, "user", Some("alice"), &["alice", "bob"], 3));
        // A user row with no author is a turn and changes no author.
        let s6 = add(&pool, &space, &thread, "user", None, 1).await;
        assert_eq!(summary(&pool, &thread).await, expect(s6, 5, "user", Some("alice"), &["alice", "bob"], 3));
        clean_space(&pool, &org, &space).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn recent_authors_are_capped_at_five_newest_first() {
        let Some(pool) = connect().await else { return };
        let (org, space, suffix) = ids("recent");
        let thread = format!("thread-{suffix}");
        seed_space_thread(&pool, &org, &space, &thread, "owner-1").await;
        for n in 1..=7 {
            add(&pool, &space, &thread, "user", Some(&format!("author-{n}")), 1).await;
        }
        let (_, _, _, last, recent, _) = summary(&pool, &thread).await;
        assert_eq!(last.as_deref(), Some("author-7"), "{NEEDS_DB}");
        assert_eq!(recent, ["author-7", "author-6", "author-5", "author-4", "author-3"], "five, newest first");
        // An author already listed moves to the front instead of growing the list.
        add(&pool, &space, &thread, "user", Some("author-3"), 1).await;
        let (_, _, _, last, recent, _) = summary(&pool, &thread).await;
        assert_eq!(last.as_deref(), Some("author-3"));
        assert_eq!(recent, ["author-3", "author-7", "author-6", "author-5", "author-4"]);
        // The author who fell off the end is listed again at the front when they speak.
        add(&pool, &space, &thread, "user", Some("author-1"), 1).await;
        let (_, _, _, _, recent, _) = summary(&pool, &thread).await;
        assert_eq!(recent, ["author-1", "author-3", "author-7", "author-6", "author-5"]);
        clean_space(&pool, &org, &space).await;
    }

    fn read_token(org: &str, user: &str, space: &str) -> String {
        mint(
            crate::grpc::CONTROL_SPACE_DECISION_AUDIENCE,
            &["thread:read"],
            false,
            |claims| {
                claims["org_id"] = json!(org);
                claims["subject_id"] = json!(user);
                claims["space_ref"] = json!(space);
                claims["recipient_audience_revision"] = json!(4);
                claims["issued_at"] = json!(chrono::Utc::now().to_rfc3339());
                claims["expires_at"] = json!((chrono::Utc::now() + chrono::Duration::minutes(2)).to_rfc3339());
            },
            false,
        )
    }

    async fn window(pool: &PgPool, org: &str, space: &str) -> Result<pb::ListSpaceTimelineResponse, Status> {
        let mut request = Request::new(pb::ListSpaceTimelineRequest {
            org_id: org.to_owned(),
            space_id: space.to_owned(),
            space_read_decision_ref: DECISION_REF.to_owned(),
            space_read_decision_token: read_token(org, "viewer-1", space),
            ..Default::default()
        });
        request.extensions_mut().insert(VerifiedIdentity::user_for_test(org, "viewer-1"));
        crate::space::timeline::list_space_timeline(pool, true, request)
            .await
            .map(tonic::Response::into_inner)
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn backfill_is_idempotent_and_blocks_window_until_done() {
        let Some(pool) = connect().await else { return };
        let _keys = StreamKeysEnv::publish();
        let (org, space, suffix) = ids("backfill");
        let thread = format!("thread-{suffix}");
        seed_space_thread(&pool, &org, &space, &thread, "owner-1").await;
        add(&pool, &space, &thread, "user", Some("alice"), 1).await;
        add(&pool, &space, &thread, "assistant", None, 1).await;
        let last = add(&pool, &space, &thread, "user", Some("bob"), 2).await;
        let filled = summary(&pool, &thread).await;
        assert_eq!(filled, expect(last, 3, "user", Some("bob"), &["bob", "alice"], 2), "{NEEDS_DB}");

        // A thread that predates the migration: messages, no summary. The backfill is pending, and the
        // room window refuses instead of reading a half-filled table.
        forget_summary(&pool, &thread).await;
        assert!(backfill_pending(&pool).await.expect("probe"), "an unfilled thread with messages is pending");
        let refusal = window(&pool, &org, &space).await.expect_err("the window waits for the backfill");
        assert_eq!(refusal.code(), tonic::Code::FailedPrecondition, "{refusal:?}");
        assert_eq!(refusal.message(), crate::space::timeline::BACKFILL_PENDING);

        // The backfill fills it from the table, completes, and reports nothing pending.
        run_backfill(pool.clone()).await;
        assert!(!backfill_pending(&pool).await.expect("probe"), "run_backfill returns only when nothing is pending");
        assert_eq!(summary(&pool, &thread).await, filled, "rebuilt exactly as the trigger had kept it");
        assert_eq!(window(&pool, &org, &space).await.expect("the window answers").total_posts, 1);

        // Idempotent: a further batch finds nothing, and a recompute changes nothing.
        assert_eq!(backfill_batch(&pool, BACKFILL_BATCH).await.expect("batch"), 0);
        recompute(&pool, &thread).await;
        recompute(&pool, &thread).await;
        assert_eq!(summary(&pool, &thread).await, filled);
        clean_space(&pool, &org, &space).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn trigger_on_unfilled_thread_rebuilds_whole_summary() {
        let Some(pool) = connect().await else { return };
        let (org, space, suffix) = ids("unfilled");
        let thread = format!("thread-{suffix}");
        seed_space_thread(&pool, &org, &space, &thread, "owner-1").await;
        add(&pool, &space, &thread, "user", Some("alice"), 1).await;
        add(&pool, &space, &thread, "user", Some("bob"), 1).await;
        add(&pool, &space, &thread, "user", Some("alice"), 3).await;
        forget_summary(&pool, &thread).await;

        // The thread has three old rows and no summary; one new message must not leave a count of one.
        let newest = add(&pool, &space, &thread, "assistant", None, 2).await;
        assert_eq!(
            summary(&pool, &thread).await,
            expect(newest, 4, "assistant", Some("alice"), &["alice", "bob"], 3),
            "the trigger rebuilt the whole summary, {NEEDS_DB}"
        );
        // From here it is incremental again.
        let next = add(&pool, &space, &thread, "user", Some("carol"), 1).await;
        assert_eq!(
            summary(&pool, &thread).await,
            expect(next, 5, "user", Some("carol"), &["carol", "alice", "bob"], 3)
        );
        clean_space(&pool, &org, &space).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn concurrent_appends_on_one_thread_keep_turn_count_exact() {
        let Some(pool) = connect().await else { return };
        let (org, space, suffix) = ids("concurrent");
        let thread = format!("thread-{suffix}");
        seed_space_thread(&pool, &org, &space, &thread, "owner-1").await;
        // The first message makes the summary incremental; the rest race.
        add(&pool, &space, &thread, "user", Some("author-0"), 1).await;
        const APPENDS: usize = 24;
        let mut tasks = Vec::new();
        for n in 1..=APPENDS {
            let (pool, space, thread) = (pool.clone(), space.clone(), thread.clone());
            tasks.push(tokio::spawn(async move {
                let role = if n % 2 == 0 { "assistant" } else { "user" };
                let author = format!("author-{n}");
                add(&pool, &space, &thread, role, (role == "user").then_some(author.as_str()), 1).await
            }));
        }
        let mut newest = 0;
        for task in tasks {
            newest = newest.max(task.await.expect("an append task"));
        }
        let (last_seq, turns, _, _, recent, _) = summary(&pool, &thread).await;
        assert_eq!(usize::try_from(turns).expect("count"), APPENDS + 1, "every append counted once, {NEEDS_DB}");
        assert_eq!(last_seq, Some(newest), "the newest sequence wins");
        assert_eq!(recent.len(), 5, "{recent:?}");
        // The incremental result is what a rebuild from the table gives.
        let incremental = summary(&pool, &thread).await;
        recompute(&pool, &thread).await;
        let rebuilt = summary(&pool, &thread).await;
        assert_eq!((incremental.0, incremental.1, &incremental.2, incremental.5), (rebuilt.0, rebuilt.1, &rebuilt.2, rebuilt.5));
        clean_space(&pool, &org, &space).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn thread_activity_recompute_equals_incremental_trigger_result() {
        let Some(pool) = connect().await else { return };
        let (org, space, suffix) = ids("recompute");
        let mixed = format!("thread-mixed-{suffix}");
        let empty = format!("thread-empty-{suffix}");
        seed_space_thread(&pool, &org, &space, &mixed, "owner-1").await;
        seed_space_thread(&pool, &org, &space, &empty, "owner-1").await;
        for (role, author, revision) in [
            ("user", Some("alice"), 1),
            ("assistant", None, 2),
            ("user", Some("bob"), 1),
            ("tool", None, 3),
            ("user", Some("alice"), 1),
            ("user", None, 1),
            ("system", None, 1),
        ] {
            add(&pool, &space, &mixed, role, author, revision).await;
        }
        let incremental = summary(&pool, &mixed).await;
        assert_eq!(incremental.1, 5, "four user rows and one assistant row are turns, {NEEDS_DB}");
        recompute(&pool, &mixed).await;
        assert_eq!(summary(&pool, &mixed).await, incremental, "a rebuild equals what the trigger kept");

        // A thread with no message stays NULL (and 0, and empty): nothing to rebuild from.
        recompute(&pool, &empty).await;
        assert_eq!(summary(&pool, &empty).await, (None, 0, None, None, Vec::new(), None));
        // The backfill never counts or touches it.
        backfill_batch(&pool, BACKFILL_BATCH).await.expect("batch");
        assert_eq!(summary(&pool, &empty).await, (None, 0, None, None, Vec::new(), None));
        // A thread that does not exist is not an error.
        recompute(&pool, &format!("thread-missing-{suffix}")).await;
        clean_space(&pool, &org, &space).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn window_plan_uses_idx_threads_space_activity() {
        let Some(pool) = connect().await else { return };
        let (org, space, suffix) = ids("plan");
        // 20,000 posts in one Space, each with a summary, written straight to the table.
        let mut tx = pool.begin().await.expect("begin");
        sqlx::query("SELECT set_config('space_event.suppress', 'on', true)")
            .execute(&mut *tx)
            .await
            .ok();
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id, origin, space_id, space_decision_ref, \
             recipient_audience_ref, recipient_audience_revision, recipient_audience_hash, privacy_policy_ref, \
             resource_authorization_ref, authority_revision, last_message_seq) \
             SELECT 'bulk-' || g || '-' || $3, 'bulk-' || g || '-' || $3, $1, 'owner-1', 'space', $2, \
                    'decision-1', 'audience-1', 1, 'sha256:audience-1', 'privacy-1', 'resource-1', 1, g \
             FROM generate_series(1, 20000) AS g",
        )
        .bind(&org)
        .bind(&space)
        .bind(&suffix)
        .execute(&mut *tx)
        .await
        .expect("seed 20,000 threads");
        tx.commit().await.expect("commit");
        sqlx::query("ANALYZE threads").execute(&pool).await.expect("analyze");

        // The room window's inner query: the newest page of one Space under the audience ceiling.
        let plan: Vec<String> = sqlx::query_scalar(
            "EXPLAIN SELECT t.id FROM threads t \
             WHERE t.org_id = $1 AND t.space_id = $2 AND t.archived_at IS NULL AND t.last_message_seq IS NOT NULL \
               AND (t.recipient_audience_revision IS NULL OR t.recipient_audience_revision <= $3) \
               AND COALESCE(t.activity_max_audience_revision, t.recipient_audience_revision, 0) <= $3 \
             ORDER BY t.last_message_seq DESC LIMIT 51",
        )
        .bind(&org)
        .bind(&space)
        .bind(4_i64)
        .fetch_all(&pool)
        .await
        .expect("explain");
        let plan = plan.join("\n");
        assert!(plan.contains("idx_threads_space_activity"), "the window must walk the activity index, {NEEDS_DB}:\n{plan}");
        clean_space(&pool, &org, &space).await;
    }
}
