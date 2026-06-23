//! `CasStore` — the content-addressable object store for the visual-RAG arm.
//!
//! Writes raw page PNGs to a dedicated MinIO/S3 bucket (`dataplane-cas`) under a
//! content-addressable, document-scoped key:
//!
//! ```text
//! org={org}/doc={doc}/pages/{page_no:04}-{content_hash}.png
//! ```
//!
//! This is intentionally NOT `ArtifactStore`/`S3Store`: that store derives its
//! key internally (`org={org}/run={run}/page={hash}/{stem}.{ext}`) and cannot
//! express a `doc=`/`pages/` layout. `CasStore` also sets **`force_path_style(true)`**
//! on the S3 config — which `S3Store` omits — and default MinIO requires it
//! (without it the SDK addresses `dataplane-cas.<endpoint>` virtual-host style and
//! fails). Endpoint/region/credentials come from the standard AWS env
//! (`AWS_ENDPOINT_URL` / `AWS_REGION` / `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY`)
//! via `aws_config::load_defaults`, mirroring `S3Store`'s recipe, with an optional
//! explicit endpoint override.
//!
//! Live `put`/`get`/`delete` are exercised end-to-end against MinIO (gated on the
//! live stack); the offline tests pin the pure key/prefix layout that erasure and
//! the serve route depend on.

use aws_config::BehaviorVersion;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::{Delete, ObjectIdentifier};

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};

/// Direct S3/MinIO client for the page-image CAS bucket.
#[derive(Clone)]
pub struct CasStore {
    client: aws_sdk_s3::Client,
    bucket: String,
}

impl CasStore {
    /// Build the client, reusing `S3Store`'s `load_defaults` recipe PLUS the
    /// MinIO-critical `force_path_style(true)`. `endpoint` (e.g. the MinIO URL)
    /// overrides `AWS_ENDPOINT_URL` when provided. Async because credential/region
    /// resolution is async.
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

    /// Content-addressable, document-scoped object key for one page PNG.
    /// `page_no` is zero-padded to 4 digits for lexical ordering; `content_hash`
    /// (blake3 hex of the PNG bytes) makes a re-render of identical bytes idempotent.
    pub fn page_object_key(org: &str, doc: &str, page_no: i64, content_hash: &str) -> String {
        format!("org={org}/doc={doc}/pages/{page_no:04}-{content_hash}.png")
    }

    /// Prefix covering every page of one document — the unit of erasure.
    pub fn doc_pages_prefix(org: &str, doc: &str) -> String {
        format!("org={org}/doc={doc}/pages/")
    }

    /// Write one page PNG. Returns the CAS key. `content_type` is set to
    /// `image/png` so the serve route / consumer see a correct MIME.
    pub async fn put_page_png(
        &self,
        org: &str,
        doc: &str,
        page_no: i64,
        content_hash: &str,
        png: Vec<u8>,
    ) -> QuarryResult<String> {
        let key = Self::page_object_key(org, doc, page_no, content_hash);
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&key)
            .content_type("image/png")
            .body(ByteStream::from(png))
            .send()
            .await
            .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("cas put {key}: {e}")))?;
        Ok(key)
    }

    /// Read one page PNG by its CAS key (used by the `image/png` serve route).
    pub async fn get_object(&self, key: &str) -> QuarryResult<Vec<u8>> {
        let out = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("cas get {key}: {e}")))?;
        let body = out
            .body
            .collect()
            .await
            .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("cas read {key}: {e}")))?;
        Ok(body.into_bytes().to_vec())
    }

    /// Delete every page object for a document (GDPR/ZDR erasure). Returns the
    /// number of objects deleted. Single-page listing (≤1000 keys) is sufficient
    /// for the MVP (web docs are 1 page); paginate if multi-thousand-page docs land.
    pub async fn delete_by_doc(&self, org: &str, doc: &str) -> QuarryResult<u64> {
        let prefix = Self::doc_pages_prefix(org, doc);
        let listed = self
            .client
            .list_objects_v2()
            .bucket(&self.bucket)
            .prefix(&prefix)
            .send()
            .await
            .map_err(|e| {
                QuarryError::new(ErrorCode::Internal, format!("cas list {prefix}: {e}"))
            })?;

        let keys: Vec<String> = listed
            .contents()
            .iter()
            .filter_map(|o| o.key().map(str::to_string))
            .collect();
        if keys.is_empty() {
            return Ok(0);
        }

        let mut objects = Vec::with_capacity(keys.len());
        for k in &keys {
            let oi = ObjectIdentifier::builder()
                .key(k)
                .build()
                .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("object id {k}: {e}")))?;
            objects.push(oi);
        }
        let delete = Delete::builder()
            .set_objects(Some(objects))
            .build()
            .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("cas delete batch: {e}")))?;

        self.client
            .delete_objects()
            .bucket(&self.bucket)
            .delete(delete)
            .send()
            .await
            .map_err(|e| {
                QuarryError::new(ErrorCode::Internal, format!("cas delete {prefix}: {e}"))
            })?;

        Ok(keys.len() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_object_key_is_content_addressable_zero_padded_and_doc_scoped() {
        let k = CasStore::page_object_key("org-1", "doc-9", 0, "b3abc");
        assert_eq!(k, "org=org-1/doc=doc-9/pages/0000-b3abc.png");
        // page-scoped: different page_no → different key
        assert_ne!(k, CasStore::page_object_key("org-1", "doc-9", 1, "b3abc"));
        // content-addressable: different bytes (hash) → different key (idempotent re-render)
        assert_ne!(k, CasStore::page_object_key("org-1", "doc-9", 0, "b3xyz"));
        // every page key lives under the document's erasure prefix
        assert!(k.starts_with(&CasStore::doc_pages_prefix("org-1", "doc-9")));
    }

    #[test]
    fn page_no_padding_orders_lexically() {
        let p9 = CasStore::page_object_key("o", "d", 9, "h");
        let p10 = CasStore::page_object_key("o", "d", 10, "h");
        assert!(p9.contains("/0009-"));
        assert!(p10.contains("/0010-"));
        assert!(p9 < p10, "zero-padding keeps lexical order == page order");
    }

    #[test]
    fn doc_pages_prefix_scopes_org_and_doc() {
        assert_eq!(CasStore::doc_pages_prefix("o", "d"), "org=o/doc=d/pages/");
        // org isolation in the key space: different org → different prefix
        assert_ne!(
            CasStore::doc_pages_prefix("o1", "d"),
            CasStore::doc_pages_prefix("o2", "d")
        );
    }
}
