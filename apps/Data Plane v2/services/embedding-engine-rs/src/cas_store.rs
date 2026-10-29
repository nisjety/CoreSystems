//! `CasStore` — GDPR/DSAR erasure for the page-image content-addressable store.
//!
//! Ingestion Plane's `quarry-runtime::cas_store::CasStore` writes raw page PNGs
//! to the shared `dataplane-cas` MinIO bucket under a content-addressable,
//! document-scoped key:
//!
//! ```text
//! org={org}/doc={doc}/pages/{page_no:04}-{content_hash}.png
//! ```
//!
//! Until this module existed, nothing in Data Plane v2 could delete from that
//! bucket: `dataplane.documents.deleted` purged the page-image *vectors* from
//! Qdrant (see `document_erasure_consumer`) but never the underlying binaries,
//! and the whole-org GDPR erasure path (`gdpr.rs`) purged Qdrant only. Deleted
//! documents' rendered pages — including scanned PII — stayed in object
//! storage forever. This is deliberately delete-only: embedding-engine never
//! writes a CAS object (Ingestion Plane's renderer does that) and never reads
//! one directly either (page images are fetched via the `image_url` the
//! producer event carries, over HTTP, not a direct S3 GET).
//!
//! Key layout and the `force_path_style(true)` MinIO requirement are copied
//! from `quarry-runtime::cas_store::CasStore` verbatim — same bucket, so the
//! two must agree on where an object lives or erasure would silently miss it.

use aws_config::BehaviorVersion;
use aws_sdk_s3::types::{Delete, ObjectIdentifier};

/// Delete-only S3/MinIO client for the page-image CAS bucket.
#[derive(Clone)]
pub struct CasStore {
    client: aws_sdk_s3::Client,
    bucket: String,
}

impl CasStore {
    /// Build the client. `endpoint` (the MinIO URL) overrides `AWS_ENDPOINT_URL`
    /// when provided; credentials/region resolve from the standard AWS env via
    /// `aws_config::load_defaults`, matching every other AWS-SDK client in this
    /// codebase. Async because credential/region resolution is async.
    pub async fn new(bucket: impl Into<String>, endpoint: Option<String>) -> Self {
        let shared = aws_config::load_defaults(BehaviorVersion::latest()).await;
        let mut builder = aws_sdk_s3::config::Builder::from(&shared).force_path_style(true);
        if let Some(ep) = endpoint.filter(|e| !e.trim().is_empty()) {
            builder = builder.endpoint_url(ep);
        }
        let client = aws_sdk_s3::Client::from_conf(builder.build());
        Self {
            client,
            bucket: bucket.into(),
        }
    }

    /// Prefix covering every page of one document — the unit of per-document
    /// erasure. MUST match `quarry-runtime::cas_store::CasStore::doc_pages_prefix`.
    pub fn doc_pages_prefix(org: &str, doc: &str) -> String {
        format!("org={org}/doc={doc}/pages/")
    }

    /// Prefix covering every object an org owns in this bucket — the unit of
    /// whole-org erasure. A superset of every `doc_pages_prefix` for that org.
    pub fn org_prefix(org: &str) -> String {
        format!("org={org}/")
    }

    /// Delete every page object for one document. Returns the number of
    /// objects deleted (0 if the document had none — not an error; a
    /// text-only document has no page images at all).
    pub async fn delete_by_doc(&self, org: &str, doc: &str) -> anyhow::Result<u64> {
        self.delete_by_prefix(&Self::doc_pages_prefix(org, doc))
            .await
    }

    /// Delete every object an org owns in this bucket (whole-org erasure).
    pub async fn delete_by_org(&self, org: &str) -> anyhow::Result<u64> {
        self.delete_by_prefix(&Self::org_prefix(org)).await
    }

    /// List then batch-delete every object under `prefix`. Paginates the list
    /// call (S3 caps one page at 1000 keys) and deletes in batches of 1000 (the
    /// `DeleteObjects` API's own limit) — unlike Ingestion Plane's single-page
    /// version, a heavily-paged document/org cannot silently leave objects
    /// behind here.
    async fn delete_by_prefix(&self, prefix: &str) -> anyhow::Result<u64> {
        let mut deleted = 0u64;
        let mut continuation_token: Option<String> = None;
        loop {
            let mut req = self
                .client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(prefix);
            if let Some(token) = continuation_token.take() {
                req = req.continuation_token(token);
            }
            let listed = req
                .send()
                .await
                .map_err(|e| anyhow::anyhow!("cas list {prefix}: {e}"))?;

            let keys: Vec<String> = listed
                .contents()
                .iter()
                .filter_map(|o| o.key().map(str::to_string))
                .collect();

            for batch in keys.chunks(1000) {
                let objects: Vec<ObjectIdentifier> = batch
                    .iter()
                    .map(|k| {
                        ObjectIdentifier::builder()
                            .key(k)
                            .build()
                            .map_err(|e| anyhow::anyhow!("object id {k}: {e}"))
                    })
                    .collect::<anyhow::Result<_>>()?;
                if objects.is_empty() {
                    continue;
                }
                let count = objects.len() as u64;
                let delete = Delete::builder()
                    .set_objects(Some(objects))
                    .build()
                    .map_err(|e| anyhow::anyhow!("cas delete batch: {e}"))?;
                self.client
                    .delete_objects()
                    .bucket(&self.bucket)
                    .delete(delete)
                    .send()
                    .await
                    .map_err(|e| anyhow::anyhow!("cas delete {prefix}: {e}"))?;
                deleted += count;
            }

            if listed.is_truncated().unwrap_or(false) {
                continuation_token = listed.next_continuation_token().map(str::to_string);
                if continuation_token.is_some() {
                    continue;
                }
            }
            break;
        }
        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doc_prefix_matches_ingestion_planes_key_layout() {
        // Locked byte-for-byte against `quarry-runtime::cas_store::CasStore`'s
        // own layout — a divergence here means erasure silently misses every
        // object the producer actually wrote.
        assert_eq!(
            CasStore::doc_pages_prefix("org-1", "doc-9"),
            "org=org-1/doc=doc-9/pages/"
        );
    }

    #[test]
    fn org_prefix_is_a_superset_of_every_doc_prefix_for_that_org() {
        let org = CasStore::org_prefix("org-1");
        assert!(CasStore::doc_pages_prefix("org-1", "doc-9").starts_with(&org));
        assert!(CasStore::doc_pages_prefix("org-1", "doc-anything-else").starts_with(&org));
    }

    #[test]
    fn org_prefix_does_not_collide_across_orgs() {
        assert_ne!(CasStore::org_prefix("org-1"), CasStore::org_prefix("org-2"));
        assert!(!CasStore::doc_pages_prefix("org-2", "doc-9")
            .starts_with(&CasStore::org_prefix("org-1")));
    }
}
