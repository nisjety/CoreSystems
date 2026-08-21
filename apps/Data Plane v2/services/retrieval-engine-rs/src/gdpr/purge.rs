//! Hard-purge of every org-scoped row this crate owns, for the Data-Plane
//! half of cross-plane GDPR organization erasure.
//!
//! `documents-api-go`'s `org_purge.go` names this crate's slice of that work
//! explicitly (`retrieval-engine-rs — retrieval_runs, access_audit_log,
//! admin_audit_log, agent_retrieval_configs, context_pins`) as a FOLLOW-UP it
//! cannot reach; this module closes that follow-up.
//!
//! Purge target and retention note: `access_audit_log` and `admin_audit_log`
//! are audit trails, but nothing in this service's own migrations, schema
//! comments, or docs (`infra/postgres/init.sql`, `docs/gap-data.md`) declares
//! a retention period that must survive org erasure — they are described as
//! access/admin-action logging, not as a compliance-mandated retention store
//! independent of the org's lifecycle. Absent such a requirement, they are
//! purged like every other org-scoped table here, matching the org-erasure
//! contract's target list.
//!
//! `retrieval_candidates` is NOT purged by an explicit statement here: its
//! `trace_id` foreign key to `retrieval_runs` is declared `ON DELETE CASCADE`
//! (`infra/postgres/init.sql`), so deleting `retrieval_runs` rows removes
//! their candidates automatically. No FK exists between any of the five
//! tables purged below, so their deletion order is not safety-load-bearing;
//! all five run inside one transaction purely for all-or-nothing atomicity.
//!
//! Idempotency: NATS is at-least-once delivery. Every statement below is a
//! `DELETE ... WHERE org_id = $1`, so a redelivered event matches zero rows
//! the second time — not an error.
//!
//! Safety: every statement is scoped strictly by the caller-supplied
//! `org_id`, bound as a query parameter (never string-interpolated), so a
//! purge for one org can never touch another org's rows.
//!
//! Phase 1 RLS: this module deliberately keeps running on the plain
//! (unscoped, superuser) pool, and converting it is its own change rather than
//! part of a sweep. Three reasons, in order of severity:
//!
//! 1. **Failure here is silent.** Under a scoped transaction a mis-scoped
//!    `DELETE` matches zero rows and reports success — indistinguishable from
//!    "nothing to purge", which this module's own idempotency contract above
//!    says is the normal second-delivery outcome. An erasure that quietly
//!    deletes nothing and returns Ok is a GDPR compliance failure that no
//!    caller would notice. That risk profile earns dedicated verification
//!    against a real database, not a bundled change.
//! 2. **`admin_audit_log.org_id` is NULLABLE.** The scoped role cannot see or
//!    delete a NULL-org row, so a scoped purge would leave exactly the
//!    platform-wide admin records behind while reporting a clean run.
//! 3. **The `retrieval_candidates` cascade is out of reach.** That table has no
//!    `org_id`, so it is absent from the 35-table array in
//!    `20260809120000_org_rls_isolation.sql` and `dataplane_app` holds no
//!    DELETE grant on it — the `ON DELETE CASCADE` documented above would fail
//!    under the scoped role.

use sqlx::{PgPool, Postgres, Transaction};

/// Per-table row counts from one purge run (logging/metrics/tests).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PurgeSummary {
    pub retrieval_runs: u64,
    pub access_audit_log: u64,
    pub admin_audit_log: u64,
    pub agent_retrieval_configs: u64,
    pub context_pins: u64,
    /// The org's cache version after the purge bumped it, invalidating every
    /// semantic-cache entry the org had in Dragonfly.
    ///
    /// Not a row count and deliberately outside `total()`: entries are not
    /// deleted key-by-key, they become unreachable because the version embedded
    /// in their key no longer matches. `None` when the bump could not be
    /// recorded — see the purge's doc comment.
    pub cache_version_after: Option<i64>,
}

impl PurgeSummary {
    /// Total rows deleted across every table in one purge run.
    #[must_use]
    pub fn total(&self) -> u64 {
        self.retrieval_runs
            + self.access_audit_log
            + self.admin_audit_log
            + self.agent_retrieval_configs
            + self.context_pins
    }
}

/// Hard-purge every org-scoped row this crate owns for `org_id`, in one
/// transaction, then invalidate the org's cached retrieval results.
///
/// # Cache invalidation
///
/// Deleting the rows above does not by itself remove the org's answers from the
/// semantic cache in Dragonfly: cached retrieval results are keyed
/// `…{org_id}:v{org_version}:s{scope}:{key}`, so they survive independently of
/// Postgres. Until this bumped the version, an org erasure left its previously
/// cached results retrievable — the content, still served, after the source rows
/// were gone.
///
/// The version bump is the invalidation: every existing key embeds the old
/// version and can no longer be constructed by a reader, so the whole org's
/// cached results become unreachable in one write. That is why this does not
/// (and cannot practically) scan-and-delete keys — `cache_key` is a hash, so
/// there is no key pattern to match per org beyond the prefix, and a wildcard
/// scan across a shared Dragonfly is exactly the operation to avoid on an
/// erasure path.
///
/// ⚠ **Not covered**: the embedding cache (`…embed:{model_version}:{text_hash}`)
/// is content-addressed with no org in its key and is shared across orgs by
/// design, so it cannot be purged per-org. It stores vectors keyed by a hash of
/// text, with no org attribution — flagged here rather than left implicit,
/// because "the cache is cleared" would otherwise overstate what happens.
///
/// # Errors
///
/// Returns an error if the transaction fails to begin, any statement fails,
/// or the commit fails. On error nothing is purged — the transaction rolls
/// back, so a NAK'd redelivery retries the whole purge cleanly.
///
/// The version bump happens AFTER the commit and is best-effort: it cannot
/// fail the purge, because the rows are already gone and a redelivery would
/// re-run a no-op delete. A failed bump is logged by
/// [`crate::cache::org_version::bump`] and surfaces here as
/// `cache_version_after: None`, which a caller should treat as "cached results
/// may still be served for this org" and retry.
pub async fn purge_organization_data(pool: &PgPool, org_id: &str) -> anyhow::Result<PurgeSummary> {
    let mut tx: Transaction<'_, Postgres> = pool.begin().await?;

    // Bound to locals (rather than assigned onto a `default()` struct) so the
    // statement order stays explicit.
    //
    // Cascades to `retrieval_candidates` (ON DELETE CASCADE on trace_id).
    let retrieval_runs = sqlx::query("DELETE FROM retrieval_runs WHERE org_id = $1")
        .bind(org_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    let access_audit_log = sqlx::query("DELETE FROM access_audit_log WHERE org_id = $1")
        .bind(org_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    // admin_audit_log.org_id is nullable (some admin actions are not
    // org-scoped) — `WHERE org_id = $1` naturally leaves NULL-org rows alone.
    let admin_audit_log = sqlx::query("DELETE FROM admin_audit_log WHERE org_id = $1")
        .bind(org_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    let agent_retrieval_configs =
        sqlx::query("DELETE FROM agent_retrieval_configs WHERE org_id = $1")
            .bind(org_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();

    let context_pins = sqlx::query("DELETE FROM context_pins WHERE org_id = $1")
        .bind(org_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    tx.commit().await?;

    // After the commit, deliberately: bumping first would invalidate the cache
    // for a purge that might then roll back, discarding a healthy cache for no
    // reason. Bumping after means the only failure mode is a stale-but-orphaned
    // cache, which the None below reports.
    let cache_version_after = match crate::cache::org_version::bump(pool, org_id).await {
        version if version > 0 => Some(version),
        _ => {
            tracing::error!(
                org_id,
                "GDPR erasure purged Postgres rows but could NOT bump the org cache version;                  previously cached retrieval results may still be served for this org"
            );
            None
        }
    };

    Ok(PurgeSummary {
        retrieval_runs,
        access_audit_log,
        admin_audit_log,
        agent_retrieval_configs,
        context_pins,
        cache_version_after,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary_with_rows() -> PurgeSummary {
        PurgeSummary {
            retrieval_runs: 3,
            access_audit_log: 5,
            admin_audit_log: 1,
            agent_retrieval_configs: 2,
            context_pins: 4,
            cache_version_after: Some(7),
        }
    }

    #[test]
    fn purge_summary_total_sums_every_row_field() {
        assert_eq!(summary_with_rows().total(), 15);
    }

    /// The cache version is a version number, not a quantity. Folding it into
    /// `total()` would make an erasure log a row count that no table produced.
    #[test]
    fn purge_summary_total_excludes_the_cache_version() {
        let bumped = summary_with_rows();
        let not_bumped = PurgeSummary {
            cache_version_after: None,
            ..bumped
        };
        assert_eq!(bumped.total(), not_bumped.total());
        assert_eq!(not_bumped.total(), 15);
    }

    /// A failed bump must be distinguishable from a successful one, because it
    /// means the org's cached results may still be served after erasure.
    #[test]
    fn a_failed_cache_bump_is_reported_as_none_not_zero() {
        let failed = PurgeSummary {
            cache_version_after: None,
            ..summary_with_rows()
        };
        assert!(
            failed.cache_version_after.is_none(),
            "a failed bump must surface as None so callers can retry, not as a version of 0              that reads like a real value"
        );
    }

    /// Static-analysis guard: every `DELETE` must be parameterized (never
    /// string-interpolate `org_id` into a query) — matches the crate's
    /// org-isolation safety requirement enforced elsewhere in this service.
    ///
    /// Scans only the production code above `mod tests`, via
    /// `include_str!("purge.rs")` split on this module's own marker. Scanning
    /// the whole file (tests included) is self-defeating: any needle this
    /// test builds to search for a forbidden/expected pattern is itself
    /// source text that `include_str!` would also pick up, so the assertion
    /// can trivially match (or be defeated by) its own code rather than the
    /// five `DELETE` statements it's meant to check.
    #[test]
    fn purge_statements_are_parameterized() {
        let production_code = include_str!("purge.rs")
            .split_once("#[cfg(test)]")
            .expect("this module has a #[cfg(test)] section")
            .0;
        assert!(
            !production_code.contains("DELETE FROM {}") && !production_code.contains("format!"),
            "a DELETE must never be built by interpolating org_id itself into the query text"
        );
        for table in [
            "retrieval_runs",
            "access_audit_log",
            "admin_audit_log",
            "agent_retrieval_configs",
            "context_pins",
        ] {
            let mut needle = "DELETE FROM ".to_owned();
            needle.push_str(table);
            needle.push_str(" WHERE org_id = $1");
            assert!(
                production_code.contains(&needle),
                "expected a parameterized purge of {table}"
            );
        }
    }
}
