//! Cross-plane GDPR organization-erasure purge for quickwit-adapter-rs.
//!
//! `org-core`'s `PublishGDPRErasureFanout` (both the explicit immediate
//! hard-delete path and the 30-day retention cron) and `user-core`'s
//! per-user erasure saga both publish onto the shared
//! `velion.gdpr.erasure.requested` fan-out. This module decides which of
//! those events this crate must act on, and performs the actual hard-purge
//! of the org-scoped rows this service owns: the Quickwit admin
//! rebuild/clear job queue (`quickwit_admin_jobs`) and its append-only audit
//! trail (`quickwit_admin_job_audit`).
//!
//! Scope note: this purge does NOT touch the Quickwit search index itself —
//! per-document/per-source-object removal from the index is already handled
//! by the live `dataplane.documents.deleted` / `dataplane.source_objects.deleted`
//! event handlers in `stream.rs`, which fire as documents-api-go and friends
//! erase their own org-scoped rows. This module only purges the admin
//! job-queue bookkeeping rows this crate itself owns in Postgres.
//!
//! Mirrors Model Plane session-core's `gdpr.rs` module (same fan-out
//! contract, same safety and idempotency rules) — see that module for the
//! sibling implementation this one was ported from.
//!
//! Safety: the fan-out subject also carries **per-user** erasure events
//! (`subject_type: "user"` / `"user_anonymize"`, see `user-core`'s
//! `erasureFanoutPayload`) whose `org_id` is simply the user's org — it is
//! NOT a request to erase that org. Only `subject_type == "organization"`
//! (org-core's `PublishGDPRErasureFanout` shape) triggers the purge in this
//! module; every other subject type is a deliberate no-op. Treating a
//! per-user erasure's `org_id` as "purge this org" would delete every other
//! org's admin job history over one member's personal-data request.
//!
//! Idempotency: NATS is at-least-once delivery. Every statement below is a
//! `DELETE ... WHERE org_id = $1` (or an FK-safe superset of it), so a
//! redelivered event is a no-op the second time — matching zero rows is not
//! an error.

use serde::Deserialize;
use sqlx::{PgPool, Postgres, Transaction};

const MAX_ID_LEN: usize = 255;

/// Wire shape of one `velion.gdpr.erasure.requested` message. Producers
/// (`org-core`, `user-core`) do not emit the same optional field set, so
/// only the fields this module reads are required to be present at all —
/// everything else is optional-and-ignored rather than rejected.
#[derive(Debug, Clone, Deserialize)]
struct ErasureEventWire {
    subject_type: Option<String>,
    subject_id: Option<String>,
    org_id: Option<String>,
    requested_by: Option<String>,
}

/// A validated organization-scoped erasure request. The only way to obtain
/// one is through [`parse_erasure_event`], which rejects every other
/// `subject_type` before a value of this type can exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrganizationErasure {
    pub org_id: String,
    pub requested_by: String,
}

/// Failure decoding or validating a `velion.gdpr.erasure.requested`
/// message. Every variant is a poison condition — the caller should route
/// the message to a dead-letter path (NAK, not ack) rather than retry it
/// forever.
#[derive(Debug, PartialEq, Eq)]
pub enum ErasureEventError {
    Decode(String),
    MissingOrgId,
    OrgIdTooLong,
    MissingSubjectId,
    SubjectOrgMismatch,
}

impl std::fmt::Display for ErasureEventError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Decode(msg) => write!(f, "invalid JSON payload: {msg}"),
            Self::MissingOrgId => write!(f, "missing or empty org_id"),
            Self::OrgIdTooLong => write!(f, "org_id exceeds the maximum bound"),
            Self::MissingSubjectId => write!(f, "missing or empty subject_id"),
            Self::SubjectOrgMismatch => write!(
                f,
                "subject_id does not match org_id for an organization-scoped erasure"
            ),
        }
    }
}

impl std::error::Error for ErasureEventError {}

/// Decode the fan-out payload and decide whether it is an
/// organization-scoped erasure this crate must act on.
///
/// Returns:
/// - `Ok(Some(erasure))` for a well-formed `subject_type: "organization"`
///   event — the caller should purge `erasure.org_id`.
/// - `Ok(None)` for any other well-formed `subject_type` (`"user"`,
///   `"user_anonymize"`, or anything else). See the module-level safety
///   note: those events are a deliberate no-op here.
/// - `Err(_)` for a malformed or self-contradictory payload.
///
/// # Errors
///
/// Returns [`ErasureEventError`] if the payload is not valid JSON, or if an
/// `organization`-typed event is missing `org_id`/`subject_id` or has a
/// `subject_id` that disagrees with `org_id` (the fixed contract is
/// `subject_id == org_id` for this subject type — a mismatch means the
/// event is malformed and must not be trusted for a destructive purge).
pub fn parse_erasure_event(
    payload: &[u8],
) -> Result<Option<OrganizationErasure>, ErasureEventError> {
    let wire: ErasureEventWire =
        serde_json::from_slice(payload).map_err(|e| ErasureEventError::Decode(e.to_string()))?;

    if wire.subject_type.as_deref().unwrap_or_default() != "organization" {
        return Ok(None);
    }

    let org_id = wire.org_id.unwrap_or_default().trim().to_owned();
    if org_id.is_empty() {
        return Err(ErasureEventError::MissingOrgId);
    }
    if org_id.len() > MAX_ID_LEN {
        return Err(ErasureEventError::OrgIdTooLong);
    }

    let subject_id = wire.subject_id.unwrap_or_default().trim().to_owned();
    if subject_id.is_empty() {
        return Err(ErasureEventError::MissingSubjectId);
    }
    if subject_id != org_id {
        return Err(ErasureEventError::SubjectOrgMismatch);
    }

    Ok(Some(OrganizationErasure {
        org_id,
        requested_by: wire.requested_by.unwrap_or_default(),
    }))
}

/// Per-table row counts from one purge run (logging/metrics/tests).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PurgeSummary {
    pub quickwit_admin_job_audit: u64,
    pub quickwit_admin_jobs: u64,
}

impl PurgeSummary {
    /// Total rows deleted across every table in one purge run.
    #[must_use]
    pub fn total(&self) -> u64 {
        self.quickwit_admin_job_audit + self.quickwit_admin_jobs
    }
}

/// Hard-purge every org-scoped row this crate owns for `org_id`, in one
/// transaction.
///
/// `quickwit_admin_job_audit.job_id` references `quickwit_admin_jobs(job_id)`
/// with no `ON DELETE CASCADE` (see the
/// `20260711150000_quickwit_admin_jobs.sql` migration), so audit rows must be
/// deleted before their parent job rows. The audit delete matches both the
/// row's own `org_id` column *and* membership in this org's job set, so the
/// purge stays FK-safe even in the hypothetical case where an audit row's
/// `org_id` ever drifted from its parent job's (it never should, per
/// `append_pg_audit` in `jobs.rs`, but the extra `OR` costs nothing and
/// closes that gap).
///
/// Global (`org_id IS NULL`) admin jobs are never matched by `org_id = $1`
/// and are intentionally left untouched — they are not this org's data.
///
/// # Errors
///
/// Returns an error if the transaction fails to begin, either statement
/// fails, or the commit fails. On error nothing is purged — the transaction
/// rolls back, so a NAK'd redelivery retries the whole purge cleanly.
pub async fn purge_organization_data(pool: &PgPool, org_id: &str) -> anyhow::Result<PurgeSummary> {
    let mut tx: Transaction<'_, Postgres> = pool.begin().await?;
    let mut summary = PurgeSummary::default();

    summary.quickwit_admin_job_audit = sqlx::query(
        "DELETE FROM quickwit_admin_job_audit
         WHERE org_id = $1
            OR job_id IN (SELECT job_id FROM quickwit_admin_jobs WHERE org_id = $1)",
    )
    .bind(org_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    summary.quickwit_admin_jobs = sqlx::query("DELETE FROM quickwit_admin_jobs WHERE org_id = $1")
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

    fn organization_payload(org_id: &str, subject_id: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "subject_type": "organization",
            "subject_id": subject_id,
            "org_id": org_id,
            "requested_by": "user_admin_1",
            "ts": "2026-07-21T00:00:00.000000000Z",
        }))
        .unwrap()
    }

    #[test]
    fn organization_erasure_is_parsed() {
        let payload = organization_payload("org_1", "org_1");
        let erasure = parse_erasure_event(&payload)
            .expect("decodes")
            .expect("organization events purge");
        assert_eq!(erasure.org_id, "org_1");
        assert_eq!(erasure.requested_by, "user_admin_1");
    }

    /// Safety-critical: a per-user erasure fan-out carries `org_id` for
    /// routing, not as a purge target. Treating it as one would destroy
    /// every other org's admin job history over a single user's request.
    #[test]
    fn user_subject_type_is_skipped_not_purged() {
        let payload = serde_json::to_vec(&serde_json::json!({
            "subject_type": "user",
            "subject_id": "user_1",
            "org_id": "org_1",
            "requested_by": "user_1",
            "mode": "delete",
        }))
        .unwrap();
        assert_eq!(parse_erasure_event(&payload).unwrap(), None);
    }

    #[test]
    fn user_anonymize_subject_type_is_skipped_not_purged() {
        let payload = serde_json::to_vec(&serde_json::json!({
            "subject_type": "user_anonymize",
            "subject_id": "user_1",
            "org_id": "org_1",
        }))
        .unwrap();
        assert_eq!(parse_erasure_event(&payload).unwrap(), None);
    }

    #[test]
    fn unknown_subject_type_is_skipped_not_purged() {
        let payload = serde_json::to_vec(&serde_json::json!({
            "subject_type": "something_new",
            "subject_id": "x",
            "org_id": "org_1",
        }))
        .unwrap();
        assert_eq!(parse_erasure_event(&payload).unwrap(), None);
    }

    #[test]
    fn missing_subject_type_is_skipped_not_purged() {
        let payload = serde_json::to_vec(&serde_json::json!({
            "org_id": "org_1",
        }))
        .unwrap();
        assert_eq!(parse_erasure_event(&payload).unwrap(), None);
    }

    #[test]
    fn missing_org_id_is_rejected() {
        let payload = serde_json::to_vec(&serde_json::json!({
            "subject_type": "organization",
            "subject_id": "org_1",
        }))
        .unwrap();
        assert_eq!(
            parse_erasure_event(&payload).unwrap_err(),
            ErasureEventError::MissingOrgId
        );
    }

    #[test]
    fn empty_org_id_is_rejected() {
        let payload = organization_payload("   ", "   ");
        assert_eq!(
            parse_erasure_event(&payload).unwrap_err(),
            ErasureEventError::MissingOrgId
        );
    }

    #[test]
    fn oversized_org_id_is_rejected() {
        let huge = "o".repeat(MAX_ID_LEN + 1);
        let payload = organization_payload(&huge, &huge);
        assert_eq!(
            parse_erasure_event(&payload).unwrap_err(),
            ErasureEventError::OrgIdTooLong
        );
    }

    #[test]
    fn missing_subject_id_is_rejected() {
        let payload = serde_json::to_vec(&serde_json::json!({
            "subject_type": "organization",
            "org_id": "org_1",
        }))
        .unwrap();
        assert_eq!(
            parse_erasure_event(&payload).unwrap_err(),
            ErasureEventError::MissingSubjectId
        );
    }

    /// Defense in depth: the fixed contract is `subject_id == org_id` for
    /// organization-typed events. A mismatch means the event is malformed
    /// (or forged) and must never be trusted to select a purge target.
    #[test]
    fn subject_id_org_id_mismatch_is_rejected() {
        let payload = organization_payload("org_1", "org_2");
        assert_eq!(
            parse_erasure_event(&payload).unwrap_err(),
            ErasureEventError::SubjectOrgMismatch
        );
    }

    #[test]
    fn malformed_json_is_rejected() {
        assert!(matches!(
            parse_erasure_event(b"not-json"),
            Err(ErasureEventError::Decode(_))
        ));
    }

    #[test]
    fn purge_summary_total_sums_every_field() {
        let summary = PurgeSummary {
            quickwit_admin_job_audit: 3,
            quickwit_admin_jobs: 2,
        };
        assert_eq!(summary.total(), 5);
    }

    /// Static-analysis guard on the purge SQL itself: every `DELETE`
    /// statement must be parameterized (never string-interpolate `org_id`
    /// into a query), and the audit table (child, via the FK on `job_id`)
    /// must be purged before its parent `quickwit_admin_jobs` row, matching
    /// the module docs above.
    #[test]
    fn purge_statements_are_parameterized_and_correctly_ordered() {
        let source = include_str!("gdpr.rs");

        assert!(
            !source.contains("format!(\"DELETE"),
            "a DELETE must never be built with format!/string interpolation"
        );

        let audit_delete = source
            .find("DELETE FROM quickwit_admin_job_audit")
            .expect("audit delete present");
        let jobs_delete = source
            .find("DELETE FROM quickwit_admin_jobs WHERE org_id = $1")
            .expect("jobs delete present");
        assert!(
            audit_delete < jobs_delete,
            "quickwit_admin_job_audit (references quickwit_admin_jobs) must be purged first"
        );
    }
}
