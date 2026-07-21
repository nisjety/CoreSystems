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

use sqlx::{PgPool, Postgres, Transaction};

/// Per-table row counts from one purge run (logging/metrics/tests).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PurgeSummary {
    pub retrieval_runs: u64,
    pub access_audit_log: u64,
    pub admin_audit_log: u64,
    pub agent_retrieval_configs: u64,
    pub context_pins: u64,
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
/// transaction.
///
/// # Errors
///
/// Returns an error if the transaction fails to begin, any statement fails,
/// or the commit fails. On error nothing is purged — the transaction rolls
/// back, so a NAK'd redelivery retries the whole purge cleanly.
pub async fn purge_organization_data(pool: &PgPool, org_id: &str) -> anyhow::Result<PurgeSummary> {
    let mut tx: Transaction<'_, Postgres> = pool.begin().await?;
    let mut summary = PurgeSummary::default();

    // Cascades to `retrieval_candidates` (ON DELETE CASCADE on trace_id).
    summary.retrieval_runs = sqlx::query("DELETE FROM retrieval_runs WHERE org_id = $1")
        .bind(org_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    summary.access_audit_log = sqlx::query("DELETE FROM access_audit_log WHERE org_id = $1")
        .bind(org_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    // admin_audit_log.org_id is nullable (some admin actions are not
    // org-scoped) — `WHERE org_id = $1` naturally leaves NULL-org rows alone.
    summary.admin_audit_log = sqlx::query("DELETE FROM admin_audit_log WHERE org_id = $1")
        .bind(org_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    summary.agent_retrieval_configs =
        sqlx::query("DELETE FROM agent_retrieval_configs WHERE org_id = $1")
            .bind(org_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();

    summary.context_pins = sqlx::query("DELETE FROM context_pins WHERE org_id = $1")
        .bind(org_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    tx.commit().await?;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn purge_summary_total_sums_every_field() {
        let summary = PurgeSummary {
            retrieval_runs: 3,
            access_audit_log: 5,
            admin_audit_log: 1,
            agent_retrieval_configs: 2,
            context_pins: 4,
        };
        assert_eq!(summary.total(), 15);
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
