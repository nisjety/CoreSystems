//! Org-scoped database transactions for Data Plane v2 (Phase 1 RLS).
//!
//! # What this is for
//!
//! Every DPv2 service connects to Postgres as `dataplane`, which is SUPERUSER
//! and owns the tables — and PostgreSQL superusers bypass row-level security
//! unconditionally. So the RLS policies added by
//! `20260809120000_org_rls_isolation.sql` do nothing on a normal connection;
//! they only bite inside a transaction that has explicitly dropped to the
//! restricted [`RLS_RUNTIME_ROLE`]. That is what this crate does.
//!
//! Opening a scoped transaction runs, in order:
//!
//! ```text
//! SELECT set_config('app.current_org', $1, true);  -- SET LOCAL, parameterized
//! SET LOCAL ROLE dataplane_app;                    -- drop superuser
//! ```
//!
//! For the rest of that transaction the effective role has `NOBYPASSRLS`, so
//! every statement is filtered to the given org by the database itself. Both
//! settings are transaction-local: `COMMIT`/`ROLLBACK` reverts them, so a
//! pooled connection is never left de-privileged or carrying another request's
//! org.
//!
//! This is the Rust counterpart of Control Plane org-core's
//! `DB.WithOrgScope` (`internal/database/database.go`), which uses the same two
//! statements against its own `org_core_app` role.
//!
//! # Why a transaction handle, not a closure
//!
//! org-core's Go version takes a `func(tx) error` callback. The direct Rust
//! translation needs higher-ranked trait bounds over an async closure, which is
//! painful to write and worse to read at every call site. Returning the
//! `Transaction` instead composes naturally with sqlx's normal API and is
//! fail-safe in the same way: sqlx rolls a `Transaction` back on drop, so
//! forgetting to commit loses the write rather than leaking an unscoped or
//! half-applied one.
//!
//! ```no_run
//! # async fn example(pool: &sqlx::PgPool) -> anyhow::Result<()> {
//! let mut tx = pg_org_scope::begin_org_scoped(pool, "org-123").await?;
//! let rows = sqlx::query("SELECT document_id FROM documents")
//!     .fetch_all(&mut *tx)
//!     .await?;  // <- returns only org-123's rows, enforced by Postgres
//! tx.commit().await?;
//! # Ok(())
//! # }
//! ```
//!
//! # When NOT to use this
//!
//! Some work is legitimately cross-org and must keep running on the normal
//! (unscoped, superuser) pool — the same documented exceptions org-core's own
//! audit identified:
//!
//! - **Background workers draining a queue/outbox for all orgs** — a scoped
//!   transaction would see only one org's rows, so a poller wrapped in this
//!   would silently stop draining everyone else's work.
//! - **GDPR/erasure and admin rebuild paths** that operate across orgs, and the
//!   rows they write with a NULL `org_id` (`admin_audit_log`,
//!   `quickwit_admin_jobs`, `quickwit_admin_job_audit`). A NULL `org_id` never
//!   satisfies the policy, so a scoped `INSERT` of one is rejected by
//!   `WITH CHECK` — correctly, since such a row belongs to no tenant.
//!
//! Use this for request-scoped paths that act on behalf of exactly one org.
//! Wrapping the wrong thing fails loudly (empty results or a policy violation),
//! not silently — but it still fails, so classify the call site first.

use sqlx::{PgPool, Postgres, Transaction};

/// The NOLOGIN, `NOBYPASSRLS` role the scoped transaction switches to. Created
/// by `20260809120000_org_rls_isolation.sql`. Hardcoded rather than
/// configurable: it is interpolated into `SET LOCAL ROLE`, which cannot take a
/// bind parameter, so a caller-supplied value would be an injection vector for
/// no practical benefit.
pub const RLS_RUNTIME_ROLE: &str = "dataplane_app";

/// The transaction-local GUC the RLS policies compare `org_id` against.
pub const ORG_GUC: &str = "app.current_org";

/// Begin a transaction pinned to `org_id` and enforced by row-level security.
///
/// The returned [`Transaction`] must be committed by the caller; dropping it
/// rolls back (sqlx's normal behaviour).
///
/// # Errors
///
/// Fails if `org_id` is empty or whitespace — that would set the GUC to `''`,
/// which matches no row and would surface later as a confusing "everything is
/// empty" bug rather than an obvious error. Also fails on the usual
/// begin/exec database errors.
pub async fn begin_org_scoped<'a>(
    pool: &'a PgPool,
    org_id: &str,
) -> anyhow::Result<Transaction<'a, Postgres>> {
    if org_id.trim().is_empty() {
        anyhow::bail!("org-scoped transaction requires a non-empty org_id");
    }

    let mut tx = pool.begin().await?;

    // Parameterized: `SET LOCAL app.current_org = $1` is not valid syntax
    // (SET LOCAL cannot bind), but set_config(..., is_local => true) is
    // exactly equivalent and does bind. This is what keeps a hostile org_id
    // from being SQL injection.
    sqlx::query("SELECT set_config($1, $2, true)")
        .bind(ORG_GUC)
        .bind(org_id)
        .execute(&mut *tx)
        .await?;

    // Order matters: set the GUC first, then drop privilege. Doing it the other
    // way works today but leaves a window where the role is restricted and the
    // org is unset, which is a strictly worse failure mode to debug.
    sqlx::query(&format!("SET LOCAL ROLE {RLS_RUNTIME_ROLE}"))
        .execute(&mut *tx)
        .await?;

    Ok(tx)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The role name is interpolated into SQL (SET LOCAL ROLE cannot bind), so
    /// it must never become caller-controlled. This test is a tripwire on that
    /// invariant: if someone parameterizes the role, this fails and they have
    /// to think about injection.
    #[test]
    fn runtime_role_is_a_fixed_bare_identifier() {
        assert_eq!(RLS_RUNTIME_ROLE, "dataplane_app");
        assert!(RLS_RUNTIME_ROLE
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_'));
    }

    #[test]
    fn guc_name_matches_the_migrations_policy_expression() {
        // Policies are `org_id = current_setting('app.current_org', true)`.
        // A drift here silently disables every policy (the GUC the code sets
        // and the one the policy reads would differ), so pin it.
        assert_eq!(ORG_GUC, "app.current_org");
    }

    #[tokio::test]
    async fn empty_org_id_is_rejected_before_touching_the_database() {
        // An unreachable pool: if the guard regressed we would get a
        // connection error instead of the validation error asserted here,
        // which is what makes this test meaningful rather than tautological.
        let pool =
            PgPool::connect_lazy("postgres://unused:unused@127.0.0.1:1/unused").expect("lazy pool");

        for candidate in ["", "   ", "\t"] {
            let err = begin_org_scoped(&pool, candidate)
                .await
                .expect_err("empty org must be rejected");
            assert!(
                err.to_string().contains("non-empty org_id"),
                "unexpected error for {candidate:?}: {err}"
            );
        }
    }

    /// Real end-to-end proof against a live database. Ignored by default
    /// because it needs one; run with a DPv2 database that has the RLS
    /// migration applied:
    ///
    /// ```text
    /// TEST_DATABASE_URL=postgres://dataplane:...@127.0.0.1:5442/dataplane \
    ///   cargo test -p pg-org-scope-rs -- --ignored
    /// ```
    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL pointing at a DPv2 database with RLS applied"]
    async fn scoped_transaction_isolates_orgs_for_real() {
        let url = std::env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must point at a DPv2 database");
        let pool = PgPool::connect(&url).await.expect("connect");

        // Unscoped (superuser) baseline: how many documents exist in total.
        let (total,): (i64,) = sqlx::query_as("SELECT count(*) FROM documents")
            .fetch_one(&pool)
            .await
            .expect("baseline count");

        // Scoped to an org that owns nothing: must see zero, and must not be
        // able to mutate anything.
        let mut tx = begin_org_scoped(&pool, "pg-org-scope-rs-nonexistent-org")
            .await
            .expect("begin scoped");
        let (visible,): (i64,) = sqlx::query_as("SELECT count(*) FROM documents")
            .fetch_one(&mut *tx)
            .await
            .expect("scoped count");
        assert_eq!(visible, 0, "a foreign org must see no documents");

        let affected = sqlx::query("UPDATE documents SET title = 'rls-test-should-not-apply'")
            .execute(&mut *tx)
            .await
            .expect("scoped update")
            .rows_affected();
        assert_eq!(affected, 0, "a foreign org must not update any document");
        // Roll back rather than commit — this test must never leave a trace.
        tx.rollback().await.expect("rollback");

        // The unscoped connection still sees everything, proving the filtering
        // above came from the scoped role and not from an empty table.
        let (after,): (i64,) = sqlx::query_as("SELECT count(*) FROM documents")
            .fetch_one(&pool)
            .await
            .expect("post count");
        assert_eq!(after, total, "unscoped visibility must be unchanged");
    }
}
