//! Cross-plane GDPR organization-erasure purge for embedding-engine-rs.
//!
//! `org-core`'s `PublishGDPRErasureFanout` (both the explicit immediate
//! hard-delete path and the 30-day retention cron) and `user-core`'s
//! per-user erasure saga both publish onto the shared
//! `velion.gdpr.erasure.requested` fan-out (see [`crate::gdpr_nats`], which
//! binds the pre-provisioned pull consumer and calls into
//! [`purge_organization_data`] with the org_id this module extracts). This
//! module only decides which of those events this crate must act on and
//! performs the actual Qdrant purge — it never touches NATS itself.
//!
//! Scope: this crate independently upserts org-scoped vector points, each
//! carrying an `org_id` payload field, into three Qdrant collections:
//!
//! - the main text-chunk collection (`Config::qdrant_collection`, default
//!   `dataplane_knowledge` — `qdrant_writer::upsert_vectors` /
//!   `batch::process_batch`)
//! - `wiki_consumer::WIKI_COLLECTION` (`wiki_block_embeddings`)
//! - the visual/page-image collection (`Config::qdrant_visual_collection`,
//!   default `dataplane_page_images` — `image_consumer::handle_created`)
//!
//! `entity_summary_embeddings` is provisioned (created) by this crate at
//! boot (see `main.rs`) so the nightly entity-summary job has it ready, but
//! this crate never upserts a point into it — the only reference in this
//! crate is that boot-time `ensure_collection` call. This module
//! deliberately does NOT purge it: matching the rest of this rollout's rule
//! that a service only purges what it independently writes (see
//! `index-engine-rs`'s `gdpr.rs` module doc, which applies the same
//! reasoning to a Postgres table it doesn't write), purging that collection
//! is the responsibility of whichever service actually owns writes to it.
//!
//! Safety: the fan-out subject also carries **per-user** erasure events
//! (`subject_type: "user"` / `"user_anonymize"`, routing-only `org_id`) —
//! NOT a request to erase that org. Only `subject_type == "organization"`
//! triggers the purge in this module; every other subject type is a
//! deliberate no-op (ack, don't purge, don't error).
//!
//! Idempotency: NATS is at-least-once delivery. Every purge below is a
//! Qdrant delete-by-filter scoped to `org_id`, so redelivering the same
//! event after a successful purge matches (and deletes) zero points the
//! second time — not an error.
//!
//! Safety: every filter is built from the caller-supplied `org_id` alone
//! ([`org_filter`]), matching exactly one payload field — never a
//! collection-wide wildcard, never a filter that could also match another
//! org's points.

use qdrant_client::qdrant::{Condition, CountPointsBuilder, DeletePointsBuilder, Filter};
use qdrant_client::Qdrant;
use serde::Deserialize;

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
/// event is malformed and must never be trusted to select a purge target).
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

/// The Qdrant collection names this crate owns writes to and therefore must
/// purge. Deliberately not a `Config` reference directly so this module
/// (and its tests) stay decoupled from crate-wide config plumbing — the
/// caller (`main.rs`) fills this in from `Config`/`wiki_consumer` once at
/// startup.
#[derive(Debug, Clone)]
pub struct PurgeCollections {
    pub knowledge: String,
    pub wiki: String,
    pub visual: String,
}

/// Per-collection point counts from one purge run (logging/tests).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PurgeSummary {
    pub knowledge_points: u64,
    pub wiki_points: u64,
    pub visual_points: u64,
}

impl PurgeSummary {
    /// Total points deleted across all three collections in one purge run.
    #[must_use]
    pub fn total(&self) -> u64 {
        self.knowledge_points + self.wiki_points + self.visual_points
    }
}

/// Hard-purge every org-scoped vector point this crate owns across its
/// three Qdrant collections, for `org_id`.
///
/// # Errors
///
/// Returns an error if `org_id` is empty (defense in depth — the caller
/// already validated this via [`parse_erasure_event`], but a delete-by-filter
/// is destructive enough that this module never trusts an unchecked
/// caller), or if any Qdrant count/delete call fails. Collections are
/// purged independently (no cross-collection transaction — Qdrant has none),
/// so a failure purging one does not roll back an already-completed purge
/// of another; a NAK'd redelivery simply repeats every collection's purge,
/// which is safe because each is independently idempotent.
pub async fn purge_organization_data(
    qdrant: &Qdrant,
    collections: &PurgeCollections,
    org_id: &str,
) -> anyhow::Result<PurgeSummary> {
    anyhow::ensure!(
        !org_id.trim().is_empty(),
        "organization erasure purge requires a non-empty org_id"
    );

    Ok(PurgeSummary {
        knowledge_points: purge_collection(qdrant, &collections.knowledge, org_id).await?,
        wiki_points: purge_collection(qdrant, &collections.wiki, org_id).await?,
        visual_points: purge_collection(qdrant, &collections.visual, org_id).await?,
    })
}

/// Count then delete every point in `collection` matching `org_id`.
/// `delete_points` itself reports no affected-point count, so this counts
/// against the same filter first (exact) — mirrors `retrieval-engine-rs`'s
/// semantic-cache pruning idiom (`cache/semantic.rs`). Scoped strictly by
/// the `org_id` payload field via [`org_filter`] — never a collection-wide
/// wildcard.
async fn purge_collection(qdrant: &Qdrant, collection: &str, org_id: &str) -> anyhow::Result<u64> {
    let filter = org_filter(org_id)?;

    let matched = qdrant
        .count(
            CountPointsBuilder::new(collection)
                .filter(filter.clone())
                .exact(true),
        )
        .await
        .map_err(|e| anyhow::anyhow!("qdrant count for org purge ({collection}): {e}"))?
        .result
        .map_or(0, |r| r.count);

    qdrant
        .delete_points(
            DeletePointsBuilder::new(collection)
                .points(filter)
                .wait(true),
        )
        .await
        .map_err(|e| anyhow::anyhow!("qdrant delete for org purge ({collection}): {e}"))?;

    tracing::info!(
        collection,
        org_id,
        matched,
        "qdrant org-scoped vectors purged"
    );
    Ok(matched)
}

/// Build a filter matching only points whose `org_id` payload field equals
/// `org_id`. The sole condition in the returned filter — never combined
/// with a wildcard, never widened to match another org's points.
fn org_filter(org_id: &str) -> anyhow::Result<Filter> {
    anyhow::ensure!(
        !org_id.trim().is_empty(),
        "org-scoped vector purge requires a non-empty org_id"
    );
    Ok(Filter::must([Condition::matches(
        "org_id",
        org_id.to_owned(),
    )]))
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
    /// every other org member's vectors over a single user's request.
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
    fn purge_summary_total_sums_all_collections() {
        let summary = PurgeSummary {
            knowledge_points: 3,
            wiki_points: 2,
            visual_points: 1,
        };
        assert_eq!(summary.total(), 6);
    }

    #[test]
    fn org_filter_is_scoped_to_a_single_org_id_condition() {
        let filter = org_filter("org-a").expect("bounded filter");
        assert_eq!(filter.must.len(), 1);
        assert!(org_filter("").is_err());
        assert!(org_filter("   ").is_err());
    }

    /// Isolation test (lesson #5/#8): seed two orgs' points across all three
    /// collections in a real Qdrant instance, purge one org, and assert the
    /// other org's points are completely untouched — and that redelivery
    /// (re-purge) of the same org is a safe no-op.
    ///
    /// Gated behind `TEST_QDRANT_URL` (a disposable/scratch Qdrant instance
    /// gRPC endpoint, e.g. `http://localhost:16334`) so it never runs as
    /// part of a normal `cargo test` and never touches this crate's real
    /// collections — every collection name here carries a unique,
    /// test-run-scoped suffix, created and dropped by the test itself.
    #[tokio::test]
    #[ignore = "requires TEST_QDRANT_URL pointing to a disposable Qdrant instance"]
    async fn purge_is_scoped_strictly_to_the_requested_org() {
        use qdrant_client::qdrant::{
            value::Kind as QdrantKind, CreateCollectionBuilder, Distance, PointStruct,
            UpsertPointsBuilder, Value as QdrantValue, VectorParamsBuilder,
        };
        use std::collections::HashMap;

        let url = std::env::var("TEST_QDRANT_URL")
            .expect("TEST_QDRANT_URL must point to a disposable Qdrant instance");
        let qdrant = Qdrant::from_url(&url)
            .build()
            .expect("connect disposable qdrant");

        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let collections = PurgeCollections {
            knowledge: format!("gdpr_isolation_test_knowledge_{suffix}"),
            wiki: format!("gdpr_isolation_test_wiki_{suffix}"),
            visual: format!("gdpr_isolation_test_visual_{suffix}"),
        };
        let names = [
            collections.knowledge.as_str(),
            collections.wiki.as_str(),
            collections.visual.as_str(),
        ];

        for name in names {
            qdrant
                .create_collection(
                    CreateCollectionBuilder::new(name)
                        .vectors_config(VectorParamsBuilder::new(4, Distance::Cosine)),
                )
                .await
                .expect("create scratch test collection");
        }

        // Seed org-a (2 points) and org-b (1 point) into every collection.
        for name in names {
            let mut points = Vec::new();
            for (id, org) in [(1u64, "org-a"), (2u64, "org-a"), (3u64, "org-b")] {
                let mut payload: HashMap<String, QdrantValue> = HashMap::new();
                payload.insert(
                    "org_id".to_string(),
                    QdrantValue {
                        kind: Some(QdrantKind::StringValue(org.to_string())),
                    },
                );
                points.push(PointStruct::new(id, vec![0.1, 0.2, 0.3, 0.4], payload));
            }
            qdrant
                .upsert_points(UpsertPointsBuilder::new(name, points).wait(true))
                .await
                .expect("seed scratch test points");
        }

        let summary = purge_organization_data(&qdrant, &collections, "org-a")
            .await
            .expect("purge org-a");
        assert_eq!(summary.total(), 6, "2 org-a points x 3 collections");

        for name in names {
            let org_a_remaining = qdrant
                .count(
                    CountPointsBuilder::new(name)
                        .filter(Filter::must([Condition::matches(
                            "org_id",
                            "org-a".to_string(),
                        )]))
                        .exact(true),
                )
                .await
                .expect("count org-a")
                .result
                .map_or(0, |r| r.count);
            let org_b_remaining = qdrant
                .count(
                    CountPointsBuilder::new(name)
                        .filter(Filter::must([Condition::matches(
                            "org_id",
                            "org-b".to_string(),
                        )]))
                        .exact(true),
                )
                .await
                .expect("count org-b")
                .result
                .map_or(0, |r| r.count);
            assert_eq!(
                org_a_remaining, 0,
                "org-a points must be fully purged in {name}"
            );
            assert_eq!(
                org_b_remaining, 1,
                "org-b points must be completely untouched in {name}"
            );
        }

        // Idempotency: redelivery of the same event purges zero points the
        // second time, not an error.
        let replay = purge_organization_data(&qdrant, &collections, "org-a")
            .await
            .expect("replayed purge is a no-op, not an error");
        assert_eq!(replay.total(), 0);

        for name in [collections.knowledge, collections.wiki, collections.visual] {
            let _ = qdrant.delete_collection(name).await;
        }
    }
}
