//! `SessionRetentionService.GetThreadRetention` (F-f10-02): whether a thread retains anything.
//!
//! Control's legal-hold placement asks this before it places a hold on a thread. A thread that keeps
//! nothing has nothing for a hold to keep, so org-core refuses the hold with `legal_hold_nothing_retained`
//! instead of placing one. The answer is two booleans, never a run id, a count, a message or a goal.
//!
//! A thread keeps nothing exactly when it has at least one run, every run is `zdr`, and it holds no row of
//! any family a legal hold keeps ([`crate::gdpr::HELD_THREAD_CONTENT`]): no message, plan, approval, task,
//! agent memory, thread goal, artifact revision, share snapshot, or research run that is not zero-retention.
//! A `zdr_waived` run is not `zdr`. A waived run is a normal persisted run, with its messages and its plan
//! (ZDR.md section 5.5), so a thread holding one retains, and its hold is placed. A run of any other class,
//! or of no class, retains too, so an unknown class fails toward the hold.
//!
//! The RPC itself, its authorization and its switch check live in `grpc.rs` and `auth.rs`. The switch
//! [`SWITCH_ENV`] is read once, at boot, and only its exact literal `true` turns it on.

use sqlx::PgPool;

/// The switch of the thread retention read. Only the exact literal `true` turns it on; off, the RPC answers
/// `unavailable` before it reads a row.
pub(crate) const SWITCH_ENV: &str = "SESSION_THREAD_RETENTION_READ";

/// Whether the read answers, from the process environment. Read once, at boot.
#[must_use]
pub(crate) fn enabled() -> bool {
    enabled_from(std::env::var(SWITCH_ENV).ok().as_deref())
}

/// [`enabled`] over a value that is already read: only the exact literal `true`.
#[must_use]
pub(crate) fn enabled_from(value: Option<&str>) -> bool {
    mp_contracts::loop_fold::switch_from(value)
}

/// One row over the thread in its org: the number of its runs, the number of those in class `zdr`, and whether it
/// holds a row of any family a legal hold keeps. The `WHERE` names one thread in one org, so an absent thread
/// returns no row at all, which is how `None` is told apart from a thread with no runs.
///
/// The families are the content families a legal hold keeps ([`crate::gdpr::HELD_THREAD_CONTENT`]), a subset of
/// what `delete_thread_rows` removes, keyed the way that erase keys them: `messages`, `plans`, `thread_goals`, `artifact_revisions` and `thread_share_snapshots`
/// by `thread_id`; `agent_memory` by `session_id`; `approvals` by the thread's runs or plans; `tasks` by the
/// thread's runs. A `research_runs` row counts only when it is not zero-retention: a `zdr` row keeps ids, status
/// and counters and never content (migration 0129), as a `zdr` run keeps nothing. The class is a literal, so the
/// predicate reads as the policy it states, and no waived class is named: a waived run keeps its content.
const THREAD_RETENTION_SQL: &str = "SELECT
            (SELECT COUNT(*) FROM runs r WHERE r.thread_id = t.id AND r.org_id = t.org_id),
            (SELECT COUNT(*) FROM runs r WHERE r.thread_id = t.id AND r.org_id = t.org_id
                AND r.retention_class = 'zdr'),
            EXISTS (SELECT 1 FROM messages m WHERE m.thread_id = t.id)
            OR EXISTS (SELECT 1 FROM plans p WHERE p.thread_id = t.id)
            OR EXISTS (SELECT 1 FROM approvals a
                       WHERE a.run_id IN (SELECT r.id FROM runs r WHERE r.thread_id = t.id)
                          OR a.plan_id IN (SELECT p.id FROM plans p WHERE p.thread_id = t.id))
            OR EXISTS (SELECT 1 FROM tasks k WHERE k.run_id IN (SELECT r.id FROM runs r WHERE r.thread_id = t.id))
            OR EXISTS (SELECT 1 FROM agent_memory am WHERE am.session_id = t.id)
            OR EXISTS (SELECT 1 FROM thread_goals g WHERE g.thread_id = t.id)
            OR EXISTS (SELECT 1 FROM artifact_revisions v WHERE v.thread_id = t.id)
            OR EXISTS (SELECT 1 FROM thread_share_snapshots s WHERE s.thread_id = t.id)
            OR EXISTS (SELECT 1 FROM research_runs rr WHERE rr.thread_id = t.id AND NOT rr.zdr)
     FROM threads t
     WHERE t.id = $1 AND t.org_id = $2";

/// Whether a thread keeps nothing, from its counts: at least one run, every run of class `zdr`, and no row of a
/// content family. A waived run is not `zdr`, so a thread with one never keeps nothing.
#[must_use]
pub(crate) fn keeps_nothing_only(runs: i64, zdr_runs: i64, holds_content: bool) -> bool {
    runs > 0 && runs == zdr_runs && !holds_content
}

/// Whether the org holds `thread_id`, and if so whether that thread keeps nothing.
///
/// `Some(true)` when the org holds the thread and it keeps nothing, `Some(false)` when it keeps something
/// or has no runs, and `None` when the org holds no thread with that id. A thread of another org is `None`.
///
/// # Errors
///
/// The database error, unchanged. The caller turns it into a generic status and logs it.
pub(crate) async fn nothing_retained(
    pool: &PgPool,
    org_id: &str,
    thread_id: &str,
) -> Result<Option<bool>, sqlx::Error> {
    let counts: Option<(i64, i64, bool)> = sqlx::query_as(THREAD_RETENTION_SQL)
        .bind(thread_id)
        .bind(org_id)
        .fetch_optional(pool)
        .await?;
    Ok(counts
        .map(|(runs, zdr_runs, holds_content)| keeps_nothing_only(runs, zdr_runs, holds_content)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retention_receipt::RetentionClass;

    #[test]
    fn the_switch_is_the_exact_literal_true() {
        assert!(enabled_from(Some("true")));
        for value in [
            None,
            Some(""),
            Some(" true "),
            Some("TRUE"),
            Some("True"),
            Some("1"),
            Some("yes"),
            Some("on"),
            Some("false"),
        ] {
            assert!(!enabled_from(value), "{value:?} must leave the read off");
        }
    }

    #[test]
    fn a_thread_keeps_nothing_only_when_it_has_runs_all_zdr_and_no_content() {
        assert!(
            !keeps_nothing_only(0, 0, false),
            "a thread with no runs is not reported as keeping nothing"
        );
        assert!(keeps_nothing_only(1, 1, false));
        assert!(keeps_nothing_only(3, 3, false));
        assert!(
            !keeps_nothing_only(2, 1, false),
            "one run that keeps content is enough to retain"
        );
        assert!(
            !keeps_nothing_only(1, 0, false),
            "a waived-only thread keeps its content, so it retains"
        );
        assert!(
            !keeps_nothing_only(1, 1, true),
            "a zdr thread that holds a content row retains"
        );
        assert!(!keeps_nothing_only(0, 0, true));
    }

    #[test]
    fn the_read_counts_only_zdr_runs_checks_every_held_family_and_scopes_by_thread_and_org() {
        assert_eq!(RetentionClass::Zdr.as_str(), "zdr");
        assert_eq!(RetentionClass::ZdrWaived.as_str(), "zdr_waived");
        assert!(THREAD_RETENTION_SQL.contains("r.retention_class = 'zdr'"));
        assert!(
            !THREAD_RETENTION_SQL.contains("zdr_waived"),
            "a waived run keeps its content, so the read never names its class"
        );
        assert!(!THREAD_RETENTION_SQL.contains("persistent"));
        assert!(THREAD_RETENTION_SQL.contains("WHERE t.id = $1 AND t.org_id = $2"));
        assert!(
            THREAD_RETENTION_SQL.contains("NOT rr.zdr"),
            "a zdr research row keeps ids and counters only"
        );
        for table in crate::gdpr::HELD_THREAD_CONTENT
            .iter()
            .filter(|table| **table != "runs")
        {
            assert!(
                THREAD_RETENTION_SQL.contains(&format!("FROM {table} ")),
                "{table} is a held family the read must check"
            );
        }
    }

    /// The legal-hold row of ZDR.md, read at build time (P2-sc-14-rel1). It names the refused thread as one that keeps
    /// nothing, and says a thread holding a waived run is held.
    const ZDR_DOC: &str = include_str!("../../../../docs/ZDR.md");

    /// P2-sc-14-rel1: the doc and the read agree on a waived thread. A waived run keeps its content (ZDR.md section
    /// 5.5), so a thread holding one retains and its hold is placed. Only a thread whose every run is `zdr` and that
    /// holds no content row is refused. A doc that refuses a waived thread fails here.
    #[test]
    fn a_thread_holding_a_waived_run_is_held_as_the_legal_hold_row_says() {
        assert!(
            !keeps_nothing_only(1, 0, false),
            "a waived-only thread retains, so its hold is placed"
        );
        assert!(
            !keeps_nothing_only(2, 1, false),
            "a waived run beside a zdr run retains"
        );
        assert!(
            keeps_nothing_only(2, 2, false),
            "only an all-zdr thread with no content keeps nothing"
        );
        let row = ZDR_DOC
            .lines()
            .find(|line| line.starts_with("| **Legal holds (EP-9 C1)**"))
            .expect("ZDR.md has the Legal holds row");
        assert!(
            !row.contains("ZDR or waived thread refused"),
            "the Legal holds row must not refuse a waived thread: {row}"
        );
        assert!(
            row.contains("a thread holding a waived run is held"),
            "the Legal holds row must say a thread holding a waived run is held: {row}"
        );
    }

    /// The scratch database, or `None` when `DATABASE_URL` is unset. Its name must say it is a test database.
    async fn scratch() -> Option<PgPool> {
        crate::space::test_support::connect().await
    }

    /// A thread of `org` with one run per entry of `classes`. A waived run names its acknowledgement and
    /// connection, as the schema requires, and no other class may. Returns the thread id.
    async fn seed_thread(pool: &PgPool, org: &str, classes: &[&str]) -> String {
        let suffix = mp_ids::new_ulid();
        let thread = format!("ret-t-{suffix}");
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, $3)",
        )
        .bind(&thread)
        .bind(org)
        .bind("user-ret")
        .execute(pool)
        .await
        .expect("seed thread");
        for (index, class) in classes.iter().enumerate() {
            let (ack, connection) = if *class == "zdr_waived" {
                (Some("ack-ret"), Some("conn-ret"))
            } else {
                (None, None)
            };
            sqlx::query(
                "INSERT INTO runs (id,thread_id,goal,org_id,user_id,retention_class,waiver_ack_id,waiver_connection_id)
                 VALUES ($1,$2,'g',$3,$4,$5,$6,$7)",
            )
            .bind(format!("ret-r-{suffix}-{index}"))
            .bind(&thread)
            .bind(org)
            .bind("user-ret")
            .bind(*class)
            .bind(ack)
            .bind(connection)
            .execute(pool)
            .await
            .expect("seed run");
        }
        thread
    }

    /// One message of `thread`. A message is content a legal hold keeps, so a thread holding one retains.
    async fn seed_message(pool: &PgPool, thread: &str) {
        sqlx::query("INSERT INTO messages (id, thread_id, role, content) VALUES ($1, $2, 'user', 'kept transcript')")
            .bind(format!("ret-m-{thread}"))
            .bind(thread)
            .execute(pool)
            .await
            .expect("seed message");
    }

    /// One active goal of `thread`. A goal is content a legal hold keeps, so a thread holding one retains.
    async fn seed_goal(pool: &PgPool, org: &str, thread: &str) {
        sqlx::query(
            "INSERT INTO thread_goals (id, thread_id, org_id, user_id, objective, max_turns, status)
             VALUES ($1, $2, $3, $4, 'keep the release notes', 10, 'active')",
        )
        .bind(format!("goal_{thread}"))
        .bind(thread)
        .bind(org)
        .bind("user-ret")
        .execute(pool)
        .await
        .expect("seed goal");
    }

    async fn cleanup(pool: &PgPool, orgs: &[&str]) {
        for org in orgs {
            sqlx::query(
                "DELETE FROM messages WHERE thread_id IN (SELECT id FROM threads WHERE org_id = $1)",
            )
            .bind(*org)
            .execute(pool)
            .await
            .expect("cleanup messages");
            sqlx::query("DELETE FROM thread_goals WHERE org_id = $1")
                .bind(*org)
                .execute(pool)
                .await
                .expect("cleanup goals");
            sqlx::query("DELETE FROM runs WHERE org_id = $1")
                .bind(*org)
                .execute(pool)
                .await
                .expect("cleanup runs");
            sqlx::query("DELETE FROM threads WHERE org_id = $1")
                .bind(*org)
                .execute(pool)
                .await
                .expect("cleanup threads");
        }
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres whose name contains \"test\""]
    async fn nothing_retained_matrix_reads_each_thread_of_its_own_org() {
        let Some(pool) = scratch().await else {
            return;
        };
        let org = format!("ret-org-{}", mp_ids::new_ulid());
        let other_org = format!("ret-other-{}", mp_ids::new_ulid());
        let all_zdr = seed_thread(&pool, &org, &["zdr", "zdr"]).await;
        let mixed = seed_thread(&pool, &org, &["zdr", "persistent"]).await;
        let waived_only = seed_thread(&pool, &org, &["zdr_waived"]).await;
        let zdr_and_waived = seed_thread(&pool, &org, &["zdr", "zdr_waived"]).await;
        let zdr_with_message = seed_thread(&pool, &org, &["zdr"]).await;
        seed_message(&pool, &zdr_with_message).await;
        let zdr_with_goal = seed_thread(&pool, &org, &["zdr"]).await;
        seed_goal(&pool, &org, &zdr_with_goal).await;
        let persistent_only = seed_thread(&pool, &org, &["persistent"]).await;
        let no_runs = seed_thread(&pool, &org, &[]).await;
        let foreign = seed_thread(&pool, &other_org, &["zdr"]).await;
        let unknown = format!("ret-missing-{}", mp_ids::new_ulid());

        for (thread, expected) in [
            (&all_zdr, Some(true)),
            (&mixed, Some(false)),
            (&waived_only, Some(false)),
            (&zdr_and_waived, Some(false)),
            (&zdr_with_message, Some(false)),
            (&zdr_with_goal, Some(false)),
            (&persistent_only, Some(false)),
            (&no_runs, Some(false)),
        ] {
            assert_eq!(
                nothing_retained(&pool, &org, thread)
                    .await
                    .expect("the read answers"),
                expected,
                "{thread}"
            );
        }
        // Another org's thread is absent from this org's view, and present in its own.
        assert_eq!(
            nothing_retained(&pool, &org, &foreign)
                .await
                .expect("the read answers"),
            None
        );
        assert_eq!(
            nothing_retained(&pool, &other_org, &foreign)
                .await
                .expect("the read answers"),
            Some(true)
        );
        assert_eq!(
            nothing_retained(&pool, &org, &unknown)
                .await
                .expect("the read answers"),
            None
        );
        cleanup(&pool, &[org.as_str(), other_org.as_str()]).await;
    }
}
