//! Shared fixture helpers for the DB-gated (`#[ignore]`d) integration tests.
//!
//! Lives in `tests/common/mod.rs` rather than `tests/common.rs` so Cargo treats
//! it as a shared module rather than compiling it as its own test binary.

use sqlx::PgPool;

/// Creates the RLS runtime role and grants it the fixture schema.
///
/// # Why a test fixture needs a database role at all
///
/// Production code in this crate opens org-scoped transactions through
/// `pg_org_scope::begin_org_scoped` (see `services/pg-org-scope-rs`), which
/// issues `SET LOCAL ROLE dataplane_app` on every scoped path — retrieval
/// arms, the gRPC document/knowledge services, trace persistence, the access
/// audit writer, and more.
///
/// In production that role is created by
/// `infra/postgres/migrations/20260809120000_org_rls_isolation.sql`. These
/// tests, however, build their own minimal schema with `CREATE TABLE` on a bare
/// disposable database and never run the migrations — so without this helper
/// every scoped path fails at runtime with:
///
/// ```text
/// error returned from database: role "dataplane_app" does not exist
/// ```
///
/// That failure is invisible to a normal `cargo test` run, because every test
/// that would hit it is `#[ignore]`d behind `TEST_DATABASE_URL`. Do not delete
/// this as boilerplate: removing it silently disables the integration tests
/// that cover cross-tenant isolation.
///
/// # Grants only — deliberately
///
/// This creates the role and grants it access, but does **not** enable row-level
/// security or install any policy. That is the point: the fixtures keep
/// asserting exactly what they asserted before RLS existed — that each query's
/// own `org_id` predicate does the filtering. Having the fixtures enforce RLS
/// too would be a strictly stronger test, but it changes what these suites
/// cover, so it belongs in its own deliberate change.
///
/// # Ordering
///
/// Must be called **after** the fixture's `CREATE TABLE` statements:
/// `GRANT ... ON ALL TABLES` applies to the tables that exist when it runs, not
/// to ones created later.
pub async fn grant_rls_runtime_role(pool: &PgPool) {
    sqlx::raw_sql(
        r#"
        -- Roles are cluster-wide, so a concurrent test in the same binary may
        -- win the race to create it. A plain IF NOT EXISTS check has a TOCTOU
        -- window here, so the create is guarded by an exception handler.
        --
        -- Catching `duplicate_object` (42710) ALONE is not enough, which is
        -- easy to get wrong: when two backends race with the role absent, the
        -- loser faults on the `pg_authid_rolname_index` unique index and
        -- raises `unique_violation` (23505) before Postgres ever reaches the
        -- duplicate-object check. Observed directly against a real cluster:
        --   duplicate key value violates unique constraint
        --   "pg_authid_rolname_index" ... Key (rolname)=(dataplane_app)
        -- Both codes must be caught or the suite flakes only under
        -- parallelism, on a cluster that does not already have the role.
        DO $role$
        BEGIN
            CREATE ROLE dataplane_app NOLOGIN NOSUPERUSER NOBYPASSRLS;
        EXCEPTION WHEN duplicate_object OR unique_violation THEN
            NULL;
        END
        $role$;

        GRANT USAGE ON SCHEMA public TO dataplane_app;
        GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO dataplane_app;
        GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO dataplane_app;
        "#,
    )
    .execute(pool)
    .await
    .expect("create and grant the dataplane_app RLS runtime role");
}
