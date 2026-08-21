// Cross-plane GDPR organization-erasure purge — org-isolation test.
//
// Seeds two orgs across every table `gdpr::purge_organization_data` owns
// (plus `retrieval_candidates`, which is not purged directly but must cascade
// from `retrieval_runs`), purges org A, and asserts:
//   - every org A row is gone (including cascaded retrieval_candidates)
//   - every org B row is untouched
//   - a second purge of the already-purged org A is a no-op (idempotency)

mod common;
use sqlx::PgPool;

use retrieval_engine::gdpr::purge_organization_data;

const ORG_A: &str = "org-gdpr-erasure-A";
const ORG_B: &str = "org-gdpr-erasure-B";

fn test_db_url() -> Option<String> {
    std::env::var("TEST_DATABASE_URL").ok()
}

async fn setup_schema(pool: &PgPool) {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS retrieval_runs (
            trace_id              TEXT         PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
            org_id                TEXT         NOT NULL,
            query                 TEXT         NOT NULL,
            created_at            TIMESTAMPTZ  NOT NULL DEFAULT NOW()
        )",
    )
    .execute(pool)
    .await
    .expect("create retrieval_runs");

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS retrieval_candidates (
            id               BIGSERIAL    PRIMARY KEY,
            trace_id         TEXT         NOT NULL REFERENCES retrieval_runs(trace_id) ON DELETE CASCADE,
            rank             INTEGER      NOT NULL,
            created_at       TIMESTAMPTZ  NOT NULL DEFAULT NOW()
        )",
    )
    .execute(pool)
    .await
    .expect("create retrieval_candidates");

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS access_audit_log (
            id                BIGSERIAL    PRIMARY KEY,
            request_id        TEXT         NOT NULL,
            org_id            TEXT         NOT NULL,
            endpoint          TEXT         NOT NULL,
            http_status       INTEGER      NOT NULL,
            auth_method       TEXT         NOT NULL,
            created_at        TIMESTAMPTZ  NOT NULL DEFAULT NOW()
        )",
    )
    .execute(pool)
    .await
    .expect("create access_audit_log");

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS admin_audit_log (
            audit_id    BIGSERIAL    PRIMARY KEY,
            org_id      TEXT,
            actor       TEXT         NOT NULL,
            action      TEXT         NOT NULL,
            outcome     TEXT         NOT NULL DEFAULT 'ok',
            created_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW()
        )",
    )
    .execute(pool)
    .await
    .expect("create admin_audit_log");

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS agent_retrieval_configs (
            config_id   TEXT         PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
            org_id      TEXT         NOT NULL,
            agent_id    TEXT         NOT NULL,
            weights     JSONB        NOT NULL,
            created_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW()
        )",
    )
    .execute(pool)
    .await
    .expect("create agent_retrieval_configs");

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS context_pins (
            pin_id      TEXT         PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
            org_id      TEXT         NOT NULL,
            title       TEXT         NOT NULL DEFAULT '',
            content     TEXT         NOT NULL,
            created_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW()
        )",
    )
    .execute(pool)
    .await
    .expect("create context_pins");

    // The erasure path bumps the org's cache version to invalidate cached
    // retrieval results. Without this table the bump degrades to a no-op and
    // reports success, so the fixture would hide exactly the regression the
    // assertions below are here to catch.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS org_versions (
            org_id     TEXT        PRIMARY KEY,
            version    BIGINT      NOT NULL DEFAULT 1,
            bumped_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )",
    )
    .execute(pool)
    .await
    .expect("create org_versions");

    // The cache-version bump runs inside an org-scoped transaction, which
    // adopts the `dataplane_app` RLS role. Without the role the bump fails and
    // degrades to a no-op that reports success — so this must follow the
    // CREATE TABLEs above. See `common::grant_rls_runtime_role`.
    common::grant_rls_runtime_role(pool).await;
}

async fn cleanup(pool: &PgPool) {
    for org in [ORG_A, ORG_B] {
        sqlx::query("DELETE FROM org_versions WHERE org_id = $1")
            .bind(org)
            .execute(pool)
            .await
            .ok();
        sqlx::query("DELETE FROM retrieval_runs WHERE org_id = $1")
            .bind(org)
            .execute(pool)
            .await
            .ok();
        sqlx::query("DELETE FROM access_audit_log WHERE org_id = $1")
            .bind(org)
            .execute(pool)
            .await
            .ok();
        sqlx::query("DELETE FROM admin_audit_log WHERE org_id = $1")
            .bind(org)
            .execute(pool)
            .await
            .ok();
        sqlx::query("DELETE FROM agent_retrieval_configs WHERE org_id = $1")
            .bind(org)
            .execute(pool)
            .await
            .ok();
        sqlx::query("DELETE FROM context_pins WHERE org_id = $1")
            .bind(org)
            .execute(pool)
            .await
            .ok();
    }
}

/// Seeds one row in every purge-target table (plus one `retrieval_candidates`
/// row hanging off the `retrieval_runs` row) for `org`. Returns the seeded
/// `retrieval_runs.trace_id` so the caller can check candidate cascade.
async fn seed(pool: &PgPool, org: &str) -> String {
    let trace_id: (String,) = sqlx::query_as(
        "INSERT INTO retrieval_runs (org_id, query) VALUES ($1, 'test query') RETURNING trace_id",
    )
    .bind(org)
    .fetch_one(pool)
    .await
    .expect("insert retrieval_runs");

    sqlx::query("INSERT INTO retrieval_candidates (trace_id, rank) VALUES ($1, 1)")
        .bind(&trace_id.0)
        .execute(pool)
        .await
        .expect("insert retrieval_candidates");

    sqlx::query(
        "INSERT INTO access_audit_log (request_id, org_id, endpoint, http_status, auth_method)
         VALUES ($1, $2, '/v1/retrieve', 200, 'jwt')",
    )
    .bind(format!("req-{org}"))
    .bind(org)
    .execute(pool)
    .await
    .expect("insert access_audit_log");

    sqlx::query("INSERT INTO admin_audit_log (org_id, actor, action) VALUES ($1, 'actor-1', 'cleanup_orphans')")
        .bind(org)
        .execute(pool)
        .await
        .expect("insert admin_audit_log");

    sqlx::query(
        "INSERT INTO agent_retrieval_configs (org_id, agent_id, weights) VALUES ($1, 'agent-1', '{}'::jsonb)",
    )
    .bind(org)
    .execute(pool)
    .await
    .expect("insert agent_retrieval_configs");

    sqlx::query("INSERT INTO context_pins (org_id, content) VALUES ($1, 'pinned fact')")
        .bind(org)
        .execute(pool)
        .await
        .expect("insert context_pins");

    trace_id.0
}

async fn count_org_rows(pool: &PgPool, org: &str) -> i64 {
    let mut total = 0i64;
    for table in [
        "retrieval_runs",
        "access_audit_log",
        "admin_audit_log",
        "agent_retrieval_configs",
        "context_pins",
    ] {
        let count: (i64,) =
            sqlx::query_as(&format!("SELECT COUNT(*) FROM {table} WHERE org_id = $1"))
                .bind(org)
                .fetch_one(pool)
                .await
                .unwrap_or((0,));
        total += count.0;
    }
    total
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to disposable PostgreSQL"]
async fn purge_removes_only_the_targeted_org() {
    let url = test_db_url().expect("TEST_DATABASE_URL is required for this ignored test");
    let pool = PgPool::connect(&url).await.unwrap();
    setup_schema(&pool).await;
    cleanup(&pool).await;

    let trace_a = seed(&pool, ORG_A).await;
    seed(&pool, ORG_B).await;

    assert_eq!(count_org_rows(&pool, ORG_A).await, 5);
    assert_eq!(count_org_rows(&pool, ORG_B).await, 5);

    let summary = purge_organization_data(&pool, ORG_A)
        .await
        .expect("purge org A");
    assert_eq!(summary.total(), 5);
    assert_eq!(summary.retrieval_runs, 1);
    assert_eq!(summary.access_audit_log, 1);
    assert_eq!(summary.admin_audit_log, 1);
    assert_eq!(summary.agent_retrieval_configs, 1);
    assert_eq!(summary.context_pins, 1);

    // The cached copy must be invalidated too: cached retrieval results are
    // keyed on the org's version, so bumping it makes every entry the org had
    // unreachable. Before this was wired, an erased org's previously cached
    // answers stayed retrievable after its rows were gone.
    let first_version = summary
        .cache_version_after
        .expect("erasure must bump the org cache version, not silently skip it");

    // Org A is fully gone, including the candidate row cascaded from its
    // purged retrieval_runs row.
    assert_eq!(count_org_rows(&pool, ORG_A).await, 0);
    let remaining_candidates: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM retrieval_candidates WHERE trace_id = $1")
            .bind(&trace_a)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        remaining_candidates.0, 0,
        "retrieval_candidates must cascade-delete with its retrieval_runs parent"
    );

    // Org B is completely untouched.
    assert_eq!(count_org_rows(&pool, ORG_B).await, 5);

    // Idempotency: redelivery of the same erasure event matches zero rows,
    // not an error.
    let second_summary = purge_organization_data(&pool, ORG_A)
        .await
        .expect("second purge of already-purged org is a no-op, not an error");
    assert_eq!(second_summary.total(), 0);

    // A redelivered erasure still bumps: it costs one write and guarantees that
    // anything cached between the two deliveries is invalidated as well.
    let second_version = second_summary
        .cache_version_after
        .expect("a redelivered erasure must still bump the cache version");
    assert!(
        second_version > first_version,
        "cache version must advance on every erasure ({second_version} must exceed {first_version})"
    );

    cleanup(&pool).await;
}
