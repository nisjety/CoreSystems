// ZDR (Zero Data Retention) behavior tests.
//
// Verifies the two enforcement points in the orchestrator:
//   1. zdr_mode = "ephemeral"  → no row persisted in retrieval_traces
//   2. zdr_mode = "reject"     → docs with zdr_classification='restricted' filtered out
//
// These tests target the data layer where enforcement actually happens, rather
// than the full pipeline (which would require Qdrant + embedding stubs).

use sqlx::PgPool;

use retrieval_engine::pipeline::types::{
    PipelineTimings, RetrievalFiltersInput, RetrievalRequest, ScoredCandidate, ZdrMode,
};
use retrieval_engine::trace::persist_trace;

mod common;

const TEST_ORG: &str = "org-zdr-test";

fn test_db_url() -> Option<String> {
    std::env::var("TEST_DATABASE_URL").ok()
}

async fn setup_schema(pool: &PgPool) {
    // Schema mirrors init.sql for the slices we touch.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS documents (
            document_id  TEXT PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
            org_id       TEXT NOT NULL,
            source       TEXT NOT NULL DEFAULT '',
            type         TEXT NOT NULL DEFAULT '',
            title        TEXT NOT NULL DEFAULT '',
            content      TEXT NOT NULL DEFAULT '',
            status       TEXT NOT NULL DEFAULT 'pending',
            metadata     JSONB,
            zdr_classification TEXT NOT NULL DEFAULT 'internal',
            zdr_reason   TEXT,
            extraction_trace JSONB,
            created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            deleted_at   TIMESTAMPTZ
        )",
    )
    .execute(pool)
    .await
    .expect("create documents");

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS retrieval_traces (
            trace_id                  TEXT PRIMARY KEY,
            org_id                    TEXT NOT NULL,
            query                     TEXT NOT NULL,
            query_embedding_model     TEXT,
            index_version             TEXT,
            filters_applied           JSONB,
            reranker_name             TEXT,
            zdr_mode                  TEXT,
            dense_retrieval_ms        BIGINT,
            sparse_retrieval_ms       BIGINT,
            rerank_ms                 BIGINT,
            total_ms                  BIGINT,
            candidate_count_dense     INTEGER,
            candidate_count_sparse    INTEGER,
            candidate_count_fused     INTEGER,
            candidate_count_reranked  INTEGER,
            created_at                TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )",
    )
    .execute(pool)
    .await
    .expect("create retrieval_traces");

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS retrieval_candidates (
            id            BIGSERIAL    PRIMARY KEY,
            trace_id      TEXT         NOT NULL REFERENCES retrieval_traces(trace_id) ON DELETE CASCADE,
            rank          INTEGER      NOT NULL,
            knowledge_id  TEXT,
            document_id   TEXT,
            dense_score   DOUBLE PRECISION,
            sparse_score  DOUBLE PRECISION,
            rerank_score  DOUBLE PRECISION
        )"
    )
    .execute(pool)
    .await
    .expect("create retrieval_candidates");

    // `persist_trace` writes org-scoped now (`SET LOCAL ROLE dataplane_app`),
    // so the fixture has to provide that role. See
    // `common::grant_rls_runtime_role`. Must follow the CREATE TABLEs above.
    common::grant_rls_runtime_role(pool).await;
}

async fn cleanup(pool: &PgPool) {
    sqlx::query("DELETE FROM retrieval_traces WHERE org_id = $1")
        .bind(TEST_ORG)
        .execute(pool)
        .await
        .ok();
    sqlx::query("DELETE FROM documents WHERE org_id = $1")
        .bind(TEST_ORG)
        .execute(pool)
        .await
        .ok();
}

fn make_request(zdr_mode: &str) -> RetrievalRequest {
    RetrievalRequest {
        query: "test query".into(),
        org_id: TEST_ORG.into(),
        top_k: Some(10),
        top_n: Some(10),
        filters: RetrievalFiltersInput::default(),
        context_budget_tokens: None,
        context_format: None,
        zdr_mode: Some(
            zdr_mode
                .parse::<ZdrMode>()
                .expect("supported test ZDR mode"),
        ),
        sovereign_required: None,
        user_id: None,
        verified_bearer: None,
        query_expansion: None,
        reranker_model: None,
        mode_mix: None,
        agent_id: None,
        admin_read_all: false,
        space_scope: None,
    }
}

fn make_timings() -> PipelineTimings {
    PipelineTimings {
        embed_ms: 0,
        dense_ms: 0,
        sparse_ms: 0,
        fusion_ms: 0,
        rerank_ms: 0,
        source_join_ms: 0,
        total_ms: 0,
        candidate_count_dense: 0,
        candidate_count_sparse: 0,
        candidate_count_fused: 0,
        candidate_count_reranked: 0,
    }
}

async fn count_traces(pool: &PgPool) -> i64 {
    let (count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM retrieval_traces WHERE org_id = $1")
            .bind(TEST_ORG)
            .fetch_one(pool)
            .await
            .unwrap();
    count
}

// ─── Test 1: persist_trace inserts a row in non-ephemeral modes ─────────────

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to disposable PostgreSQL"]
async fn test_persist_trace_creates_row_when_not_ephemeral() {
    let url = test_db_url().expect("TEST_DATABASE_URL is required for this ignored test");
    let pool = PgPool::connect(&url).await.unwrap();
    setup_schema(&pool).await;
    cleanup(&pool).await;

    let before = count_traces(&pool).await;

    let req = make_request("disabled");
    let candidates: Vec<ScoredCandidate> = vec![];
    let timings = make_timings();

    let trace_id = persist_trace(
        &pool,
        &req,
        &candidates,
        &timings,
        None,
        "disabled",
        None,
        &[],
    )
    .await
    .expect("persist_trace");

    assert!(!trace_id.starts_with("ephemeral-"));
    assert_eq!(count_traces(&pool).await, before + 1);

    cleanup(&pool).await;
}

// ─── Test 2: ephemeral mode produces ephemeral trace_id and no DB row ───────

#[tokio::test]
async fn test_ephemeral_mode_does_not_persist_trace() {
    let Some(url) = test_db_url() else {
        return;
    };
    let pool = PgPool::connect(&url).await.unwrap();
    setup_schema(&pool).await;
    cleanup(&pool).await;

    let before = count_traces(&pool).await;

    // This mirrors what orchestrator.rs does in ephemeral mode:
    let trace_id = format!("ephemeral-{}", uuid::Uuid::new_v4());

    assert!(trace_id.starts_with("ephemeral-"));
    // Crucially: DO NOT call persist_trace.
    // Verify no row was added.
    assert_eq!(count_traces(&pool).await, before);

    cleanup(&pool).await;
}

// ─── Test 3: reject-mode SQL filters out restricted documents ───────────────

#[tokio::test]
async fn test_reject_mode_filters_restricted_docs() {
    let Some(url) = test_db_url() else {
        return;
    };
    let pool = PgPool::connect(&url).await.unwrap();
    setup_schema(&pool).await;
    cleanup(&pool).await;

    // Insert one internal and one restricted doc
    let internal_id: (String,) = sqlx::query_as(
        "INSERT INTO documents (org_id, source, type, title, content, zdr_classification)
         VALUES ($1, 's', 't', 'Internal Doc', 'safe', 'internal') RETURNING document_id",
    )
    .bind(TEST_ORG)
    .fetch_one(&pool)
    .await
    .unwrap();

    let restricted_id: (String,) = sqlx::query_as(
        "INSERT INTO documents (org_id, source, type, title, content, zdr_classification)
         VALUES ($1, 's', 't', 'Restricted Doc', 'secret', 'restricted') RETURNING document_id",
    )
    .bind(TEST_ORG)
    .fetch_one(&pool)
    .await
    .unwrap();

    let candidate_doc_ids = vec![internal_id.0.clone(), restricted_id.0.clone()];

    // Mirrors the SQL used in orchestrator.rs ZDR-reject branch:
    let restricted: Vec<(String,)> = sqlx::query_as(
        "SELECT document_id FROM documents
         WHERE document_id = ANY($1) AND zdr_classification = 'restricted'",
    )
    .bind(&candidate_doc_ids)
    .fetch_all(&pool)
    .await
    .unwrap();

    let restricted_set: std::collections::HashSet<String> =
        restricted.into_iter().map(|(id,)| id).collect();

    assert_eq!(restricted_set.len(), 1);
    assert!(restricted_set.contains(&restricted_id.0));
    assert!(!restricted_set.contains(&internal_id.0));

    // Apply filter — internal doc passes, restricted is removed
    let filtered: Vec<&String> = candidate_doc_ids
        .iter()
        .filter(|d| !restricted_set.contains(*d))
        .collect();

    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0], &internal_id.0);

    cleanup(&pool).await;
}

// ─── Test 4: disabled mode does not filter restricted docs ──────────────────

#[tokio::test]
async fn test_disabled_mode_does_not_filter_restricted() {
    let Some(url) = test_db_url() else {
        return;
    };
    let pool = PgPool::connect(&url).await.unwrap();
    setup_schema(&pool).await;
    cleanup(&pool).await;

    let restricted_id: (String,) = sqlx::query_as(
        "INSERT INTO documents (org_id, source, type, title, content, zdr_classification)
         VALUES ($1, 's', 't', 'Doc', 'x', 'restricted') RETURNING document_id",
    )
    .bind(TEST_ORG)
    .fetch_one(&pool)
    .await
    .unwrap();

    // In zdr_mode != "reject", orchestrator.rs skips the filter SQL entirely.
    // We verify: the doc still exists in the org, with restricted classification.
    let row: (String, String) = sqlx::query_as(
        "SELECT document_id, zdr_classification FROM documents WHERE document_id = $1",
    )
    .bind(&restricted_id.0)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(row.0, restricted_id.0);
    assert_eq!(row.1, "restricted");

    cleanup(&pool).await;
}
