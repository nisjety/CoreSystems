//! The web-taint marker (WS11.S7, session-core half).
//!
//! A run that read web, browser or third-party-MCP content has `runs.external_content_at` set
//! (migration 0087). The assistant text written inside such a run may paraphrase a page that
//! carried instructions, so Dreaming (`dreaming.rs`) does not turn those assistant messages into
//! durable memory. User messages stay eligible, and so does a later run of the same thread that
//! read nothing external. The marker is a timestamp derived from a tool NAME: no URL, query or
//! result is stored, so a zero-retention run stores nothing but a time.
//!
//! Which tools count is not decided here. Both services ask
//! `mp_content_safety::TrustClass::classify(name).is_external_content()`; the pin is
//! `external_content_tool_list_is_pinned` (here, and in `mp-content-safety`).
//!
//! The marker is set in the transaction that records the step:
//!
//! * `CompleteStep` (execution-core's loop), only for a step that COMPLETED: a failed fetch
//!   produced no outside content.
//! * `ReserveToolAction` (the inline chat loop), at reservation, which is before the tool runs and
//!   so also marks a call that later fails. That is the conservative side for a guard that only
//!   tightens.
//!
//! A delegated child's marker reaches every ancestor run (`subagent_edges`), because the parent
//! reads what its child brought back.

use mp_content_safety::TrustClass;
use sqlx::{Postgres, Transaction};

/// Kill switch for the Dreaming filter. Default ON: the guard only tightens, so unlike the other
/// new behaviours it ships enabled. Only an explicit `0`, `false`, `off` or `no` turns it off, for
/// a research-heavy tenant that must not wait for a rebuild.
pub const SKIP_ENV: &str = "SESSION_CORE_DREAMING_SKIP_EXTERNAL_CONTENT";

/// Seconds after a tainted run ends during which an assistant message still counts as written
/// inside it (the message row can land just after the run's terminal step). A placeholder, pinned
/// by the DB tests.
pub const WINDOW_GRACE_SECS: i64 = 60;

/// Whether a tool's result is external content (web, browser, third-party MCP).
#[must_use]
pub fn is_external_content_tool(name: &str) -> bool {
    TrustClass::classify(name).is_external_content()
}

/// Whether the kill-switch value leaves the Dreaming filter on.
#[must_use]
pub fn skip_enabled_from(value: Option<&str>) -> bool {
    !matches!(
        value.map(|v| v.trim().to_ascii_lowercase()).as_deref(),
        Some("0" | "false" | "off" | "no")
    )
}

/// Whether Dreaming skips assistant messages written inside a tainted run.
#[must_use]
pub fn skip_enabled() -> bool {
    skip_enabled_from(std::env::var(SKIP_ENV).ok().as_deref())
}

/// Mark `run_id` and every ancestor run (through `subagent_edges`) as having read external
/// content. Idempotent: a run already marked keeps its first timestamp. Returns the number of
/// runs newly marked.
///
/// # Errors
///
/// A database error; the caller's transaction decides.
pub async fn mark_external_content(
    tx: &mut Transaction<'_, Postgres>,
    run_id: &str,
) -> Result<u64, sqlx::Error> {
    sqlx::query(
        "WITH RECURSIVE lineage(id) AS (
             SELECT $1::text
             UNION
             SELECT e.parent_run_id FROM subagent_edges e JOIN lineage l ON e.child_run_id = l.id
         )
         UPDATE runs SET external_content_at = now()
         WHERE id IN (SELECT id FROM lineage) AND external_content_at IS NULL",
    )
    .bind(run_id)
    .execute(&mut **tx)
    .await
    .map(|done| done.rows_affected())
}

/// SQL predicate (over `messages m`) true for an assistant message written inside a tainted run of
/// its thread: `$flag` is the 1-based parameter number of the kill switch (a bool bound by the
/// caller), so a switched-off filter makes the predicate false everywhere.
#[must_use]
pub fn tainted_assistant_sql(flag: usize) -> String {
    format!(
        "(m.role = 'assistant' AND ${flag} AND EXISTS ( \
           SELECT 1 FROM runs er \
           WHERE er.thread_id = m.thread_id \
             AND er.external_content_at IS NOT NULL \
             AND m.created_at >= er.created_at \
             AND m.created_at <= COALESCE(er.ended_at, now()) + interval '{WINDOW_GRACE_SECS} seconds'))"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loop_checkpoint::test_support::{scratch_pool, seed_run};

    #[test]
    fn external_content_tool_list_is_pinned() {
        for name in [
            "web_search",
            "web_fetch",
            "fetch_url",
            "web.search",
            "web.read",
            "browser_agent",
            "browser.observe",
            "browser.act",
            "browser.console",
            "extract_structured",
            "knowledge.map",
            "knowledge.crawl_site",
            "knowledge.batch",
            "mcp__srv__create_order",
        ] {
            assert!(is_external_content_tool(name), "{name}");
        }
        for name in [
            "read_provider_action",
            "execute_provider_action",
            "knowledge_search",
            "save_memory",
            "recall_memory",
            "browser.open",
            "",
        ] {
            assert!(!is_external_content_tool(name), "{name}");
        }
    }

    #[test]
    fn mcp_prefixed_tools_are_external() {
        for name in ["mcp__a__b", "mcp__x", "mcp__"] {
            assert!(is_external_content_tool(name), "{name}");
        }
        // A look-alike without the exact prefix is not.
        for name in ["mcp_a_b", "MCP__a", "xmcp__a"] {
            assert!(!is_external_content_tool(name), "{name}");
        }
    }

    #[test]
    fn org_internal_tools_are_not_external() {
        for name in [
            "knowledge_search",
            "list_provider_actions",
            "save_memory",
            "recall_memory",
            "create_plan",
            "some_future_tool",
        ] {
            assert!(!is_external_content_tool(name), "{name}");
        }
    }

    #[test]
    fn the_dreaming_filter_is_on_unless_explicitly_switched_off() {
        assert!(skip_enabled_from(None));
        for on in ["", "1", "true", "on", "yes", "garbage"] {
            assert!(skip_enabled_from(Some(on)), "{on:?}");
        }
        for off in ["0", "false", "off", "no", " OFF ", "False"] {
            assert!(!skip_enabled_from(Some(off)), "{off:?}");
        }
    }

    #[test]
    fn the_tainted_predicate_binds_the_switch_it_is_given() {
        let sql = tainted_assistant_sql(7);
        assert!(sql.contains("AND $7 AND"), "{sql}");
        assert!(
            sql.contains("m.role = 'assistant'"),
            "user rows are never matched"
        );
        assert!(sql.contains("interval '60 seconds'"));
    }

    // ---- Postgres (ignored): DATABASE_URL to a scratch database whose name says "test" ----

    use mp_contracts::model_plane::v1 as pb;
    use sqlx::PgPool;

    async fn marker(pool: &PgPool, run_id: &str) -> Option<chrono::DateTime<chrono::Utc>> {
        sqlx::query_scalar("SELECT external_content_at FROM runs WHERE id = $1")
            .bind(run_id)
            .fetch_one(pool)
            .await
            .expect("read marker")
    }

    /// A tool step as execution-core writes it: the audit prefix names the tool.
    fn tool_step(run_id: &str, step_id: &str, tool: &str, status: &str) -> pb::CompleteStepRequest {
        pb::CompleteStepRequest {
            run_id: run_id.to_owned(),
            step_id: step_id.to_owned(),
            status: status.to_owned(),
            output: format!(
                "[data_category=public_non_personal zdr=false tool={tool}] result text"
            ),
            error: String::new(),
            terminal: false,
        }
    }

    async fn complete(pool: &PgPool, request: pb::CompleteStepRequest) {
        crate::grpc::complete_step_inner(pool, None, request)
            .await
            .expect("complete step");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn external_content_marker_is_set_once_by_complete_step() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (run, _, _) = seed_run(&pool, "persistent").await;
        assert_eq!(marker(&pool, &run).await, None, "a new run is untainted");

        // Org-internal tools, and a web fetch that FAILED, taint nothing.
        complete(
            &pool,
            tool_step(&run, "s1", "knowledge_search", "completed"),
        )
        .await;
        complete(&pool, tool_step(&run, "s2", "save_memory", "completed")).await;
        complete(&pool, tool_step(&run, "s3", "web_fetch", "failed")).await;
        assert_eq!(marker(&pool, &run).await, None);

        // A completed web fetch does.
        complete(&pool, tool_step(&run, "s4", "web_fetch", "completed")).await;
        let first = marker(&pool, &run).await.expect("tainted");

        // A second external step keeps the first timestamp.
        complete(
            &pool,
            tool_step(&run, "s5", "mcp__srv__lookup", "completed"),
        )
        .await;
        complete(&pool, tool_step(&run, "s6", "browser.observe", "completed")).await;
        assert_eq!(
            marker(&pool, &run).await,
            Some(first),
            "set once, never moved"
        );
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn external_content_marker_is_set_by_inline_reserve_tool_action() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (run, _, _) = seed_run(&pool, "persistent").await;
        let reserve = |action: &str, tool: &str| pb::ReserveToolActionRequest {
            run_id: run.clone(),
            action_id: action.to_owned(),
            request_id: format!("req-{action}"),
            tool: tool.to_owned(),
            data_category: "public_non_personal".to_owned(),
            zdr: false,
            ..Default::default()
        };
        crate::grpc::reserve_tool_action_inner(&pool, None, reserve("a1", "knowledge_search"))
            .await
            .expect("internal");
        assert_eq!(marker(&pool, &run).await, None);
        crate::grpc::reserve_tool_action_inner(&pool, None, reserve("a2", "web_search"))
            .await
            .expect("web search");
        let first = marker(&pool, &run)
            .await
            .expect("tainted by the reservation");
        // An exact replay of the reservation moves nothing.
        crate::grpc::reserve_tool_action_inner(&pool, None, reserve("a2", "web_search"))
            .await
            .expect("replay");
        assert_eq!(marker(&pool, &run).await, Some(first));
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn external_content_marker_reaches_the_parent_run() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (grandparent, org, user) = seed_run(&pool, "persistent").await;
        let thread: String = sqlx::query_scalar("SELECT thread_id FROM runs WHERE id = $1")
            .bind(&grandparent)
            .fetch_one(&pool)
            .await
            .expect("thread");
        let mut ids = vec![grandparent.clone()];
        for depth in ["parent", "child"] {
            let id = format!("{depth}-{}", mp_ids::new_ulid());
            sqlx::query(
                "INSERT INTO runs (id, thread_id, goal, org_id, user_id, parent_run_id) \
                 VALUES ($1, $2, 'g', $3, $4, $5)",
            )
            .bind(&id)
            .bind(&thread)
            .bind(&org)
            .bind(&user)
            .bind(ids.last().expect("previous"))
            .execute(&pool)
            .await
            .expect("seed run");
            sqlx::query("INSERT INTO subagent_edges (parent_run_id, child_run_id) VALUES ($1, $2)")
                .bind(ids.last().expect("previous"))
                .bind(&id)
                .execute(&pool)
                .await
                .expect("seed edge");
            ids.push(id);
        }
        // A sibling that read nothing stays clean.
        let sibling = format!("sibling-{}", mp_ids::new_ulid());
        sqlx::query(
            "INSERT INTO runs (id, thread_id, goal, org_id, user_id) VALUES ($1, $2, 'g', $3, $4)",
        )
        .bind(&sibling)
        .bind(&thread)
        .bind(&org)
        .bind(&user)
        .execute(&pool)
        .await
        .expect("seed sibling");
        sqlx::query("INSERT INTO subagent_edges (parent_run_id, child_run_id) VALUES ($1, $2)")
            .bind(&grandparent)
            .bind(&sibling)
            .execute(&pool)
            .await
            .expect("seed sibling edge");

        let child = ids.last().expect("child").clone();
        complete(&pool, tool_step(&child, "c1", "web_fetch", "completed")).await;

        for id in &ids {
            assert!(
                marker(&pool, id).await.is_some(),
                "{id} is tainted by its descendant"
            );
        }
        assert_eq!(
            marker(&pool, &sibling).await,
            None,
            "a sibling branch is not"
        );
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn external_content_zdr_run_stores_only_a_timestamp() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (run, _, _) = seed_run(&pool, "zdr").await;
        let row = |pool: PgPool, run: String| async move {
            sqlx::query_scalar::<_, serde_json::Value>(
                "SELECT to_jsonb(r) - 'external_content_at' FROM runs r WHERE id = $1",
            )
            .bind(run)
            .fetch_one(&pool)
            .await
            .expect("row")
        };
        let before = row(pool.clone(), run.clone()).await;
        let mut tx = pool.begin().await.expect("begin");
        let marked = mark_external_content(&mut tx, &run).await.expect("mark");
        tx.commit().await.expect("commit");
        assert_eq!(marked, 1);
        assert!(marker(&pool, &run).await.is_some());
        assert_eq!(
            row(pool.clone(), run.clone()).await,
            before,
            "nothing but the timestamp column changed"
        );
        // The marker added no event, message or other row for the run.
        let events: i64 = sqlx::query_scalar("SELECT count(*) FROM events WHERE run_id = $1")
            .bind(&run)
            .fetch_one(&pool)
            .await
            .expect("events");
        assert_eq!(events, 0);
        // Marking again changes nothing.
        let mut tx = pool.begin().await.expect("begin");
        assert_eq!(
            mark_external_content(&mut tx, &run).await.expect("again"),
            0
        );
        tx.commit().await.expect("commit");
    }
}
