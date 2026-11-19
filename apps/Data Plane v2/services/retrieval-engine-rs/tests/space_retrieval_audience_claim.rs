//! WS17-S9: the Space columns on a retrieval trace, and migration 20261116114344 that adds them.
//!
//! Both tests need TEST_DATABASE_URL pointing at a disposable PostgreSQL and are `#[ignore]`d
//! otherwise (the convention of the other database tests in this crate). The migration test runs
//! its up and down inside one transaction that is rolled back, so the shared `retrieval_runs`
//! table is left as it was found. The trace test leaves the migration applied, which is the state
//! the trace writer needs.

use retrieval_engine::pipeline::types::{PipelineTimings, RetrievalRequest};
use retrieval_engine::space_scope::{ResolvedSpaceRetrievalScope, VerifiedSpaceAuthority};
use retrieval_engine::trace::{get_trace, persist_trace};
use sqlx::PgPool;

mod common;

const MIGRATION_UP: &str =
    include_str!("../../../infra/postgres/migrations/20261116114344_space_retrieval_audience_claim.sql");
const MIGRATION_DOWN: &str = include_str!(
    "../../../infra/postgres/migrations/20261116114344_space_retrieval_audience_claim.down.sql"
);

const ORG: &str = "org-space-trace";
const SPACE: &str = "space:org-space-trace:room-1";
const AUDIENCE_HASH: &str = "audience-hash-7";
const AUDIENCE_REVISION: i64 = 3;

/// Counts the three Space columns that exist on this session's `retrieval_runs`.
const SPACE_COLUMNS_SQL: &str = "SELECT COUNT(*) FROM information_schema.columns
    WHERE table_schema = current_schema() AND table_name = 'retrieval_runs'
      AND column_name IN ('space_ref', 'space_audience_hash', 'space_audience_revision')";

async fn connect() -> PgPool {
    let url = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL is required for this ignored test");
    PgPool::connect(&url).await.expect("connect to TEST_DATABASE_URL")
}

/// The base shape the trace writer and reader use, before the migration adds the Space columns.
async fn create_base_tables(pool: &PgPool) {
    sqlx::raw_sql(
        "CREATE TABLE IF NOT EXISTS retrieval_runs (
            trace_id TEXT PRIMARY KEY,
            org_id TEXT NOT NULL,
            query TEXT,
            query_embedding_model TEXT,
            index_version TEXT,
            filters_json JSONB,
            reranker_name TEXT,
            zdr_mode TEXT,
            top_k INTEGER,
            dense_retrieval_ms INTEGER,
            sparse_retrieval_ms INTEGER,
            rerank_ms INTEGER,
            total_ms INTEGER,
            candidate_count_dense INTEGER,
            candidate_count_sparse INTEGER,
            candidate_count_fused INTEGER,
            candidate_count_reranked INTEGER,
            mode_mix JSONB,
            zdr_actions_applied JSONB,
            mode_mix_applied JSONB,
            actor_user_id TEXT,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        CREATE TABLE IF NOT EXISTS retrieval_candidates (
            trace_id TEXT NOT NULL,
            rank INTEGER NOT NULL,
            knowledge_id TEXT,
            document_id TEXT,
            dense_score DOUBLE PRECISION,
            sparse_score DOUBLE PRECISION,
            rerank_score DOUBLE PRECISION,
            final_score DOUBLE PRECISION,
            source_chunk_ref TEXT
        );",
    )
    .execute(pool)
    .await
    .expect("create the base retrieval trace tables");
    // Grants apply to the tables that exist when they run, so this follows the CREATE TABLEs.
    common::grant_rls_runtime_role(pool).await;
}

fn space_scope() -> ResolvedSpaceRetrievalScope {
    ResolvedSpaceRetrievalScope {
        authority: VerifiedSpaceAuthority {
            authority_revision: 1,
            decision_ref: "decision-space-trace".into(),
            history_floor: Default::default(),
            org_id: ORG.into(),
            privacy_policy_ref: "privacy-space-trace".into(),
            recipient_audience_hash: AUDIENCE_HASH.into(),
            recipient_audience_ref: "audience-space-trace".into(),
            recipient_audience_revision: AUDIENCE_REVISION,
            resource_authorization_ref: "resource-space-trace".into(),
            space_ref: SPACE.into(),
        },
        collection_id: None,
        document_ids: Vec::new(),
        document_ids_truncated: false,
        owner_resource_ref: "owner-space-trace".into(),
        workspace_id: Some("workspace-space-trace".into()),
    }
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to disposable PostgreSQL"]
async fn the_audience_claim_migration_adds_nullable_columns_and_its_down_removes_them() {
    let pool = connect().await;
    create_base_tables(&pool).await;

    // One transaction, rolled back at the end: the shared table is unchanged afterwards.
    let mut tx = pool.begin().await.expect("begin");
    let existing = format!("space-migration-existing-{}", std::process::id());
    sqlx::query(
        "INSERT INTO retrieval_runs (trace_id, org_id, query) VALUES ($1, $2, 'before the migration')",
    )
    .bind(&existing)
    .bind(ORG)
    .execute(&mut *tx)
    .await
    .expect("an existing run");

    sqlx::raw_sql(MIGRATION_UP)
        .execute(&mut *tx)
        .await
        .expect("the up migration applies");
    let added: i64 = sqlx::query_scalar(SPACE_COLUMNS_SQL)
        .fetch_one(&mut *tx)
        .await
        .expect("column lookup after up");
    assert_eq!(added, 3, "up adds the three Space columns");
    let all_null: bool = sqlx::query_scalar(
        "SELECT space_ref IS NULL AND space_audience_hash IS NULL AND space_audience_revision IS NULL
         FROM retrieval_runs WHERE trace_id = $1",
    )
    .bind(&existing)
    .fetch_one(&mut *tx)
    .await
    .expect("the existing run after up");
    assert!(all_null, "an existing run keeps NULL in all three columns");

    sqlx::raw_sql(MIGRATION_DOWN)
        .execute(&mut *tx)
        .await
        .expect("the down migration applies");
    let left: i64 = sqlx::query_scalar(SPACE_COLUMNS_SQL)
        .fetch_one(&mut *tx)
        .await
        .expect("column lookup after down");
    assert_eq!(left, 0, "down removes the three Space columns");

    tx.rollback().await.expect("rollback keeps the shared table unchanged");
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to disposable PostgreSQL"]
async fn a_space_scoped_run_persists_its_space_and_audience_and_an_unscoped_run_persists_nulls() {
    let pool = connect().await;
    create_base_tables(&pool).await;
    // The migration is idempotent (ADD COLUMN IF NOT EXISTS), and the trace writer needs it applied.
    sqlx::raw_sql(MIGRATION_UP)
        .execute(&pool)
        .await
        .expect("the up migration applies");

    let timings = PipelineTimings::default();

    let mut scoped: RetrievalRequest = serde_json::from_value(
        serde_json::json!({ "org_id": ORG, "query": "what is in the room?" }),
    )
    .expect("a minimal request");
    scoped.space_scope = Some(space_scope());
    let scoped_trace = persist_trace(&pool, &scoped, &[], &timings, None, "disabled", None, &[])
        .await
        .expect("the scoped trace persists");
    let scoped_run = get_trace(&pool, &scoped_trace, ORG, None)
        .await
        .expect("the scoped trace reads back")
        .expect("the scoped trace exists")
        .run;
    assert_eq!(scoped_run.space_ref.as_deref(), Some(SPACE));
    assert_eq!(scoped_run.space_audience_hash.as_deref(), Some(AUDIENCE_HASH));
    assert_eq!(scoped_run.space_audience_revision, Some(AUDIENCE_REVISION));

    let unscoped: RetrievalRequest = serde_json::from_value(
        serde_json::json!({ "org_id": ORG, "query": "what is open?" }),
    )
    .expect("a minimal request");
    let unscoped_trace = persist_trace(&pool, &unscoped, &[], &timings, None, "disabled", None, &[])
        .await
        .expect("the unscoped trace persists");
    let plain = get_trace(&pool, &unscoped_trace, ORG, None)
        .await
        .expect("the unscoped trace reads back")
        .expect("the unscoped trace exists")
        .run;
    assert!(plain.space_ref.is_none(), "an unscoped run has no Space");
    assert!(plain.space_audience_hash.is_none(), "an unscoped run has no audience hash");
    assert!(plain.space_audience_revision.is_none(), "an unscoped run has no audience revision");
}
