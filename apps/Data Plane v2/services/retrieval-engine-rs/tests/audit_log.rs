// Wave 3 §15-E — access_audit_log writes correctly per request.
//
// Verifies:
//   - `record_access` writes one row with the expected fields.
//   - Best-effort: write failure does NOT panic (use a closed pool to simulate).

use sqlx::PgPool;

use retrieval_engine::audit::{record_access, AccessEvent};
use retrieval_engine::authz::{AuthContext, AuthMethod, EffectiveAcl};

mod common;

const TEST_ORG: &str = "org-audit-test";

fn test_db_url() -> Option<String> {
    std::env::var("TEST_DATABASE_URL").ok()
}

async fn setup_schema(pool: &PgPool) {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS access_audit_log (
            id                BIGSERIAL    PRIMARY KEY,
            request_id        TEXT         NOT NULL,
            user_id           TEXT,
            org_id            TEXT         NOT NULL,
            endpoint          TEXT         NOT NULL,
            http_status       INTEGER      NOT NULL,
            latency_ms        INTEGER      NOT NULL DEFAULT 0,
            auth_method       TEXT         NOT NULL,
            document_ids      TEXT[]       NOT NULL DEFAULT '{}',
            cause             TEXT,
            created_at        TIMESTAMPTZ  NOT NULL DEFAULT NOW()
        )",
    )
    .execute(pool)
    .await
    .expect("create access_audit_log");

    // `record_access` writes org-scoped now (`SET LOCAL ROLE dataplane_app`),
    // so the fixture has to provide that role — and because the write is
    // best-effort, a missing role would surface here as a silently empty table
    // rather than an error. See `common::grant_rls_runtime_role`.
    common::grant_rls_runtime_role(pool).await;
}

async fn cleanup(pool: &PgPool) {
    sqlx::query("DELETE FROM access_audit_log WHERE org_id = $1")
        .bind(TEST_ORG)
        .execute(pool)
        .await
        .ok();
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to disposable PostgreSQL"]
async fn writes_one_row_per_event() {
    let url = test_db_url().expect("TEST_DATABASE_URL is required for this ignored test");
    let pool = PgPool::connect(&url).await.unwrap();
    setup_schema(&pool).await;
    cleanup(&pool).await;

    let ctx = AuthContext {
        user_id: Some("user-1".into()),
        org_id: TEST_ORG.into(),
        auth_method: AuthMethod::Jwt,
        scopes: vec!["read".into()],
        zdr: false,
        acl: EffectiveAcl::allow_all(),
        request_id: "req-test-1".into(),
        verified_bearer: None,
    };

    record_access(
        &pool,
        AccessEvent {
            ctx: &ctx,
            endpoint: "POST /v1/retrieve",
            http_status: 200,
            latency_ms: 42,
            document_ids: vec!["doc-1".into(), "doc-2".into()],
            cause: "ok",
        },
    )
    .await;

    let (count, doc_count): (i64, i32) = sqlx::query_as(
        "SELECT COUNT(*)::BIGINT, COALESCE(MAX(ARRAY_LENGTH(document_ids, 1)), 0)
         FROM access_audit_log WHERE org_id = $1 AND request_id = 'req-test-1'",
    )
    .bind(TEST_ORG)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
    assert_eq!(doc_count, 2);

    cleanup(&pool).await;
}

#[tokio::test]
async fn write_failure_does_not_panic() {
    // Closed pool simulates Postgres down. record_access must NOT panic;
    // it must log a warning and return cleanly so the request finishes.
    let Some(url) = test_db_url() else {
        return;
    };
    let pool = PgPool::connect(&url).await.unwrap();
    pool.close().await;

    let ctx = AuthContext::org_scoped(TEST_ORG, AuthMethod::ApiKey, "req-test-2".into());
    record_access(
        &pool,
        AccessEvent {
            ctx: &ctx,
            endpoint: "GET /v1/retrieval/x",
            http_status: 200,
            latency_ms: 1,
            document_ids: vec![],
            cause: "ok",
        },
    )
    .await;
    // If we reach here, the best-effort contract holds.
}
