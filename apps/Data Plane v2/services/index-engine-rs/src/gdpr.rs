//! Cross-plane GDPR organization-erasure purge for index-engine-rs.
//!
//! `org-core`'s `PublishGDPRErasureFanout` (immediate hard-delete path and
//! the 30-day retention cron) publishes onto the shared
//! `verevon.gdpr.erasure.requested` fan-out on the cross-plane broker
//! (`control-shared-nats`, stream `AQENCIA_CONTROLPLANE`). This module
//! decides which of those events this crate must act on, and performs the
//! actual hard-purge of the two org-scoped tables index-engine-rs — and
//! only index-engine-rs — owns write/delete access to:
//!
//! - `knowledge_units` (chunks/embeddings): this crate is the only writer
//!   that `INSERT`s or `DELETE`s rows here (`builder::process_document`,
//!   `outbox::delete_and_enqueue`). `embedding-engine-rs` only ever
//!   `UPDATE`s the `embedding_status`/`embedded_at`/`error_message` columns
//!   of rows that already exist — it never deletes or inserts a row — so it
//!   does not independently own purge responsibility for this table.
//! - `index_deletion_outbox`: the atomic deletion-intent outbox this crate
//!   alone reads and writes (see `outbox.rs`); no other service references
//!   this table at all.
//!
//! Note: `knowledge_units.document_id` is
//! `REFERENCES documents(document_id) ON DELETE CASCADE`
//! (`infra/postgres/init.sql`), so `documents-api-go`'s own GDPR purge of
//! `documents` rows will also cascade-delete these rows. This module's
//! direct, independently-idempotent `DELETE ... WHERE org_id = $1` does not
//! rely on that cascade — NATS delivers the same fan-out event to every
//! consumer independently with no ordering guarantee across services — and
//! is simply a no-op the second time either purge already ran.
//! `index_deletion_outbox` has no such cascade and must be purged directly
//! regardless.
//!
//! Safety: the fan-out subject also carries **per-user** erasure events
//! (`subject_type: "user"` / `"user_anonymize"`, routing-only `org_id`) —
//! NOT a request to erase that org. Only `subject_type == "organization"`
//! triggers the purge in this module; every other subject type is a
//! deliberate no-op (ack, don't purge, don't error).
//!
//! Idempotency: NATS is at-least-once delivery. Every statement below is a
//! `DELETE ... WHERE org_id = $1`, so a redelivered event is a no-op the
//! second time — matching zero rows is not an error.

use serde::Deserialize;
use sqlx::{PgPool, Postgres, Transaction};

const MAX_ID_LEN: usize = 255;

/// Wire shape of one `verevon.gdpr.erasure.requested` message. Producers
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

/// Failure decoding or validating a `verevon.gdpr.erasure.requested`
/// message. Every variant is a poison condition — the caller should
/// dead-letter (NAK) the message rather than retry it forever.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ErasureEventError {
    #[error("invalid JSON payload: {0}")]
    Decode(String),
    #[error("missing or empty org_id")]
    MissingOrgId,
    #[error("org_id exceeds the maximum bound")]
    OrgIdTooLong,
    #[error("missing or empty subject_id")]
    MissingSubjectId,
    #[error("subject_id does not match org_id for an organization-scoped erasure")]
    SubjectOrgMismatch,
}

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

/// Per-table row counts from one purge run (logging/tests).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PurgeSummary {
    pub index_deletion_outbox: u64,
    pub knowledge_units: u64,
}

impl PurgeSummary {
    /// Total rows deleted across both tables in one purge run.
    #[must_use]
    pub fn total(&self) -> u64 {
        self.index_deletion_outbox + self.knowledge_units
    }
}

/// Hard-purge every org-scoped row this crate owns for `org_id`, in one
/// transaction. There is no foreign key between `index_deletion_outbox` and
/// `knowledge_units` (`index_deletion_outbox.knowledge_ids` is a plain JSONB
/// array, not a real reference), so the order between the two statements
/// below is arbitrary — both are independently `org_id`-scoped and
/// idempotent.
///
/// # Errors
///
/// Returns an error if the transaction fails to begin, either statement
/// fails, or the commit fails. On error nothing is purged — the transaction
/// rolls back, so a NAK'd redelivery retries the whole purge cleanly.
///
/// Phase 1 RLS: this purge deliberately keeps running on the plain (unscoped,
/// superuser) pool while `builder::process_document` moved onto an org-scoped
/// transaction. **Failure here would be silent.** Under a scoped transaction a
/// mis-scoped `DELETE` matches zero rows and reports success —
/// indistinguishable from "nothing left to purge", which the idempotency
/// contract above says is the *normal* outcome of a redelivered event. An
/// erasure that quietly deletes nothing and returns `Ok` is a GDPR compliance
/// failure no caller would notice, so converting it earns its own change with
/// dedicated verification against a real database rather than riding along in a
/// sweep. This mirrors the same decision, for the same reason, in
/// `graph-index-rs/src/store.rs` and `retrieval-engine-rs/src/gdpr/purge.rs`.
pub async fn purge_organization_data(pool: &PgPool, org_id: &str) -> anyhow::Result<PurgeSummary> {
    // Phase 1 RLS: unscoped on purpose — see the doc comment above.
    let mut tx: Transaction<'_, Postgres> = pool.begin().await?;

    // Bound to locals rather than assigned onto a `default()` struct (clippy's
    // `field_reassign_with_default`). Order between these two statements is
    // arbitrary — see the fn doc: there is no foreign key between them, both are
    // independently `org_id`-scoped, and both are idempotent.
    let index_deletion_outbox = sqlx::query("DELETE FROM index_deletion_outbox WHERE org_id = $1")
        .bind(org_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    let knowledge_units = sqlx::query("DELETE FROM knowledge_units WHERE org_id = $1")
        .bind(org_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    tx.commit().await?;
    Ok(PurgeSummary {
        index_deletion_outbox,
        knowledge_units,
    })
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
            "ts": "2026-07-20T00:00:00.000000000Z",
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
    /// every other org member's chunks/embeddings over a single user's
    /// request.
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
    fn purge_summary_total_sums_both_fields() {
        let summary = PurgeSummary {
            index_deletion_outbox: 3,
            knowledge_units: 5,
        };
        assert_eq!(summary.total(), 8);
    }

    /// Static-analysis guard on the purge SQL itself: every `DELETE`
    /// statement must be parameterized (never string-interpolate `org_id`
    /// into a query).
    #[test]
    fn purge_statements_are_parameterized() {
        let source = include_str!("gdpr.rs");
        assert!(
            !source.contains("format!(\"DELETE"),
            "a DELETE must never be built with format!/string interpolation"
        );
        assert!(source.contains("DELETE FROM index_deletion_outbox WHERE org_id = $1"));
        assert!(source.contains("DELETE FROM knowledge_units WHERE org_id = $1"));
    }

    /// Isolation test (lesson #8): seed two orgs across both org-scoped
    /// tables, purge one, and assert the other org's rows in both tables are
    /// completely untouched.
    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL pointing to disposable PostgreSQL"]
    async fn purge_is_scoped_strictly_to_the_requested_org() {
        let database_url = std::env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must point to disposable PostgreSQL");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(4)
            .connect(&database_url)
            .await
            .expect("connect disposable postgres");

        sqlx::raw_sql(
            "CREATE TABLE documents (document_id TEXT PRIMARY KEY);
             CREATE TABLE knowledge_units (
                 knowledge_id TEXT PRIMARY KEY,
                 document_id TEXT NOT NULL REFERENCES documents(document_id) ON DELETE CASCADE,
                 org_id TEXT NOT NULL
             );",
        )
        .execute(&pool)
        .await
        .expect("create minimal knowledge_units fixture");
        sqlx::raw_sql(include_str!(
            "../../../infra/postgres/migrations/20260711170000_index_deletion_outbox.sql"
        ))
        .execute(&pool)
        .await
        .expect("apply index_deletion_outbox migration");

        for doc_id in ["doc-a", "doc-b"] {
            sqlx::query("INSERT INTO documents VALUES ($1)")
                .bind(doc_id)
                .execute(&pool)
                .await
                .expect("insert fixture document");
        }
        for (kid, org, doc) in [
            ("kid-a1", "org-a", "doc-a"),
            ("kid-a2", "org-a", "doc-a"),
            ("kid-b1", "org-b", "doc-b"),
        ] {
            sqlx::query("INSERT INTO knowledge_units VALUES ($1, $2, $3)")
                .bind(kid)
                .bind(doc)
                .bind(org)
                .execute(&pool)
                .await
                .expect("insert fixture chunk");
        }
        for (org, doc, key) in [
            ("org-a", "doc-a", "erasure-fixture-org-a"),
            ("org-b", "doc-b", "erasure-fixture-org-b"),
        ] {
            sqlx::query(
                "INSERT INTO index_deletion_outbox
                     (org_id, document_id, knowledge_ids, idempotency_key)
                 VALUES ($1, $2, $3, $4)",
            )
            .bind(org)
            .bind(doc)
            .bind(serde_json::json!(["kid-placeholder"]))
            .bind(key)
            .execute(&pool)
            .await
            .expect("insert fixture outbox row");
        }

        let summary = purge_organization_data(&pool, "org-a")
            .await
            .expect("purge org-a");
        assert_eq!(summary.knowledge_units, 2);
        assert_eq!(summary.index_deletion_outbox, 1);

        let org_a_units: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM knowledge_units WHERE org_id = 'org-a'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let org_b_units: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM knowledge_units WHERE org_id = 'org-b'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let org_a_outbox: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM index_deletion_outbox WHERE org_id = 'org-a'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let org_b_outbox: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM index_deletion_outbox WHERE org_id = 'org-b'")
                .fetch_one(&pool)
                .await
                .unwrap();

        assert_eq!(org_a_units, 0, "org-a chunks must be fully purged");
        assert_eq!(org_b_units, 1, "org-b chunks must be completely untouched");
        assert_eq!(org_a_outbox, 0, "org-a outbox rows must be fully purged");
        assert_eq!(
            org_b_outbox, 1,
            "org-b outbox rows must be completely untouched"
        );

        // Idempotency (lesson #7): redelivery of the same event purges zero
        // rows the second time, not an error.
        let replay = purge_organization_data(&pool, "org-a")
            .await
            .expect("replayed purge is a no-op, not an error");
        assert_eq!(replay.total(), 0);
    }
}
