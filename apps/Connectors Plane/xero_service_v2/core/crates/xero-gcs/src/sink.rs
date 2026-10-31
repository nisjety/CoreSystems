//! Raw object sinks: the `RawSink` trait plus a GCS and a local-disk impl.
//!
//! Both impls write the object body AND a `.meta.json` sidecar. The GCS impl
//! attaches the `ObjectMeta` map as native GCS custom object metadata; the
//! local impl just mirrors the same key layout on disk for `--dry-run`.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::NaiveDate;
use uuid::Uuid;

use gcloud_storage::client::{Client, ClientConfig};
use gcloud_storage::http::buckets::get::GetBucketRequest;
use gcloud_storage::http::objects::get::GetObjectRequest;
use gcloud_storage::http::objects::upload::{UploadObjectRequest, UploadType};
use gcloud_storage::http::objects::Object;
use gcloud_storage::http::Error as GcsHttpError;
use sha2::{Digest, Sha256};
use tracing::warn;

use crate::error::{GcsError, Result};
use crate::manifest::{manifest_key, RunManifest};
use crate::meta::{sidecar_bytes, sidecar_key, ObjectMeta};

/// Content type for both the raw object body and the sidecar.
const CONTENT_TYPE_JSON: &str = "application/json";
const MAX_UPLOAD_ATTEMPTS: usize = 5;
const UPLOAD_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(90);

fn upload_hash(body: &[u8]) -> String {
    format!("{:x}", Sha256::digest(body))
}

fn upload_crc32c(body: &[u8]) -> String {
    STANDARD.encode(crc32c::crc32c(body).to_be_bytes())
}

fn existing_upload_matches(
    object: &Object,
    expected_metadata: &HashMap<String, String>,
    expected_crc32c: &str,
    expected_size: usize,
    expected_content_type: &str,
) -> bool {
    object.generation > 0
        && object.size == expected_size as i64
        && object.crc32c.as_deref() == Some(expected_crc32c)
        && object.content_type.as_deref() == Some(expected_content_type)
        && object.metadata.as_ref() == Some(expected_metadata)
}

fn is_upload_precondition_failure(error: &GcsHttpError) -> bool {
    matches!(error, GcsHttpError::Response(response) if response.code == 412)
}

fn is_retryable_upload_error(error: &GcsHttpError) -> bool {
    match error {
        GcsHttpError::Response(response) => response.is_retriable(),
        GcsHttpError::HttpClient(error) | GcsHttpError::RawResponse(error, _) => {
            error.is_timeout()
                || error.is_connect()
                || error.is_request()
                || error
                    .status()
                    .is_some_and(|status| matches!(status.as_u16(), 408 | 429 | 500..=599))
        }
        _ => false,
    }
}

async fn wait_before_upload_retry(attempt: usize) {
    let seconds = 1_u64 << attempt;
    warn!(
        attempt = attempt + 1,
        wait_seconds = seconds,
        "transient GCS upload failure; retrying"
    );
    tokio::time::sleep(Duration::from_secs(seconds)).await;
}

/// Fresh sidecars pin the uploaded body, never caller-supplied data identity.
fn data_sidecar_meta(meta: &ObjectMeta, uploaded: &Object) -> Result<ObjectMeta> {
    if !matches!(meta.get("x-source-system"), Some("xero") | Some("xero_v2")) {
        return Ok(meta.clone());
    }
    if uploaded.generation <= 0 {
        return Err(GcsError::Upload(
            "fresh raw upload returned no valid data generation".into(),
        ));
    }
    let crc32c = uploaded
        .crc32c
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| GcsError::Upload("fresh raw upload returned no crc32c".into()))?;
    Ok(ObjectMeta(
        meta.0
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .chain([
                ("x-data-generation".into(), uploaded.generation.to_string()),
                ("x-data-crc32c".into(), crc32c.to_string()),
            ])
            .collect(),
    ))
}

/// A sink that persists verbatim raw API response bytes plus a metadata sidecar.
#[async_trait::async_trait]
pub trait RawSink: Send + Sync {
    /// Persist `body` at `key` with `meta` as object metadata, then persist the
    /// `.meta.json` sidecar alongside it.
    async fn put_raw(&self, key: &str, body: &[u8], meta: &ObjectMeta) -> Result<()>;

    async fn put_raw_typed(
        &self,
        key: &str,
        body: &[u8],
        meta: &ObjectMeta,
        _content_type: &str,
    ) -> Result<()> {
        self.put_raw(key, body, meta).await
    }
}

/// Production sink: writes to a GCS bucket with full custom object metadata.
pub struct GcsRawSink {
    client: Client,
    bucket: String,
}

impl GcsRawSink {
    /// Build a GCS sink, authenticating from `GOOGLE_APPLICATION_CREDENTIALS`
    /// (or the other sources `with_auth` supports).
    pub async fn new(bucket: String) -> Result<Self> {
        if bucket.trim().is_empty() {
            return Err(GcsError::Config(
                "bucket name must not be empty".to_string(),
            ));
        }
        let config = ClientConfig::default()
            .with_auth()
            .await
            .map_err(|e| GcsError::Auth(e.to_string()))?;
        let client = Client::new(config);
        Ok(Self { client, bucket })
    }

    /// The bucket this sink writes to.
    pub fn bucket(&self) -> &str {
        &self.bucket
    }

    /// Confirm the configured bucket exists and this identity can read its metadata.
    pub async fn check_bucket(&self) -> Result<()> {
        self.client
            .get_bucket(&GetBucketRequest {
                bucket: self.bucket.clone(),
                ..Default::default()
            })
            .await
            .map(|_| ())
            .map_err(|e| GcsError::Auth(format!("bucket {} check failed: {e}", self.bucket)))
    }

    /// Convert the deterministic `BTreeMap` metadata into the `HashMap` the
    /// GCS API expects for `Object.metadata`.
    fn meta_to_hashmap(meta: &ObjectMeta) -> HashMap<String, String> {
        meta.0.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    /// Upload `body` at `key` with custom object metadata via a multipart
    /// upload (the only upload form that carries `Object.metadata`).
    async fn upload_with_meta(
        &self,
        key: &str,
        body: Vec<u8>,
        meta: &ObjectMeta,
        content_type: &str,
    ) -> Result<Object> {
        let expected_hash = upload_hash(&body);
        let expected_crc32c = upload_crc32c(&body);
        let mut expected_metadata = Self::meta_to_hashmap(meta);
        expected_metadata.insert("x-upload-sha256".into(), expected_hash);
        for attempt in 0..MAX_UPLOAD_ATTEMPTS {
            let object = Object {
                name: key.to_string(),
                bucket: self.bucket.clone(),
                content_type: Some(content_type.to_string()),
                metadata: Some(expected_metadata.clone()),
                ..Default::default()
            };
            let req = UploadObjectRequest {
                bucket: self.bucket.clone(),
                if_generation_match: Some(0),
                ..Default::default()
            };
            let upload_type = UploadType::Multipart(Box::new(object));
            match tokio::time::timeout(
                UPLOAD_ATTEMPT_TIMEOUT,
                self.client.upload_object(&req, body.clone(), &upload_type),
            )
            .await
            {
                Ok(Ok(uploaded))
                    if existing_upload_matches(
                        &uploaded,
                        &expected_metadata,
                        &expected_crc32c,
                        body.len(),
                        content_type,
                    ) =>
                {
                    return Ok(uploaded)
                }
                Ok(Ok(_)) => {
                    return Err(GcsError::Upload(format!(
                        "upload {key} returned mismatched object identity"
                    )))
                }
                Ok(Err(error)) if is_upload_precondition_failure(&error) => {
                    let get_req = GetObjectRequest {
                        bucket: self.bucket.clone(),
                        object: key.to_string(),
                        ..Default::default()
                    };
                    match tokio::time::timeout(
                        UPLOAD_ATTEMPT_TIMEOUT,
                        self.client.get_object(&get_req),
                    )
                    .await
                    {
                        Ok(Ok(existing))
                            if existing_upload_matches(
                                &existing,
                                &expected_metadata,
                                &expected_crc32c,
                                body.len(),
                                content_type,
                            ) =>
                        {
                            return Ok(existing)
                        }
                        Ok(Ok(_)) => {
                            return Err(GcsError::Upload(format!(
                                "upload {key} conflicts with an existing object"
                            )))
                        }
                        Ok(Err(read_error))
                            if attempt + 1 < MAX_UPLOAD_ATTEMPTS
                                && is_retryable_upload_error(&read_error) =>
                        {
                            wait_before_upload_retry(attempt).await;
                        }
                        Ok(Err(read_error)) => {
                            return Err(GcsError::Upload(format!(
                                "upload {key} exists but could not be verified: {read_error}"
                            )))
                        }
                        Err(_) if attempt + 1 < MAX_UPLOAD_ATTEMPTS => {
                            wait_before_upload_retry(attempt).await;
                        }
                        Err(_) => {
                            return Err(GcsError::Upload(format!(
                                "upload {key} exists but verification timed out"
                            )))
                        }
                    }
                }
                Ok(Err(error))
                    if attempt + 1 < MAX_UPLOAD_ATTEMPTS && is_retryable_upload_error(&error) =>
                {
                    wait_before_upload_retry(attempt).await;
                }
                Ok(Err(error)) => {
                    return Err(GcsError::Upload(format!("upload {key} failed: {error}")))
                }
                Err(_) if attempt + 1 < MAX_UPLOAD_ATTEMPTS => {
                    wait_before_upload_retry(attempt).await;
                }
                Err(_) => {
                    return Err(GcsError::Upload(format!(
                        "upload {key} timed out after {MAX_UPLOAD_ATTEMPTS} attempts"
                    )))
                }
            }
        }
        unreachable!("upload attempts are positive")
    }

    /// Upload the sidecar with the same create-only and recovery rules as the body.
    async fn upload_plain(&self, key: &str, body: Vec<u8>) -> Result<()> {
        self.upload_with_meta(key, body, &ObjectMeta::default(), CONTENT_TYPE_JSON)
            .await
            .map(|_| ())
    }
}

#[async_trait::async_trait]
impl RawSink for GcsRawSink {
    async fn put_raw(&self, key: &str, body: &[u8], meta: &ObjectMeta) -> Result<()> {
        self.put_raw_typed(key, body, meta, CONTENT_TYPE_JSON).await
    }

    async fn put_raw_typed(
        &self,
        key: &str,
        body: &[u8],
        meta: &ObjectMeta,
        content_type: &str,
    ) -> Result<()> {
        if content_type.is_empty()
            || content_type.len() > 128
            || content_type.chars().any(char::is_control)
        {
            return Err(GcsError::Config("invalid raw content type".into()));
        }
        // 1. Object body with native custom metadata.
        let uploaded = self
            .upload_with_meta(key, body.to_vec(), meta, content_type)
            .await?;
        // 2. Sidecar (queryable copy of the metadata, survives metadata-stripping copies).
        let side_meta = data_sidecar_meta(meta, &uploaded)?;
        let side_key = sidecar_key(key);
        let side_body = sidecar_bytes(&side_meta);
        self.upload_plain(&side_key, side_body).await?;
        Ok(())
    }
}

/// Dry-run sink: mirrors the same key layout under `root` on local disk.
pub struct LocalDirSink {
    root: PathBuf,
}

impl LocalDirSink {
    /// Create a local sink rooted at `root` (created lazily on first write).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The root directory this sink writes under.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolve `key` to a path strictly under `root`, rejecting any key that
    /// would escape the root directory.
    ///
    /// The check is lexical (no filesystem access, so it works before the file
    /// exists): the joined path is walked component by component. An absolute
    /// key, a Windows prefix/root component, or a `..` that would climb above
    /// `root` all cause an [`GcsError::Io`] rather than a silent escape.
    fn resolve_within_root(&self, key: &str) -> Result<PathBuf> {
        let joined = self.root.join(key);
        let mut resolved = PathBuf::new();
        let mut depth: usize = 0;
        let root_depth = self.root.components().count();
        for component in joined.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    if depth <= root_depth {
                        return Err(GcsError::Io(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            format!("key {key:?} escapes the sink root directory"),
                        )));
                    }
                    depth -= 1;
                    resolved.pop();
                }
                other => {
                    resolved.push(other);
                    depth += 1;
                }
            }
        }
        if !resolved.starts_with(&self.root) {
            return Err(GcsError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("key {key:?} escapes the sink root directory"),
            )));
        }
        Ok(resolved)
    }

    /// Write `body` to `root/key`, creating parent directories as needed.
    ///
    /// Rejects any `key` that would resolve outside `root` (path traversal).
    async fn write_file(&self, key: &str, body: &[u8]) -> Result<()> {
        let path = self.resolve_within_root(key)?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                tokio::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700)).await?;
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            use tokio::io::AsyncWriteExt;
            let mut file = tokio::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)
                .await?;
            tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).await?;
            file.write_all(body).await?;
        }
        #[cfg(not(unix))]
        tokio::fs::write(&path, body).await?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl RawSink for LocalDirSink {
    async fn put_raw(&self, key: &str, body: &[u8], meta: &ObjectMeta) -> Result<()> {
        self.write_file(key, body).await?;
        let side_key = sidecar_key(key);
        let side_body = sidecar_bytes(meta);
        self.write_file(&side_key, &side_body).await?;
        Ok(())
    }
}

/// Serialize and write a run manifest to its (prefix-free) manifest key.
///
/// Writes the existing pretty JSON body via `put_raw` with run identity and
/// timing metadata. A `GenerationSink` also isolates the manifest key.
pub async fn write_manifest(
    sink: &dyn RawSink,
    run_date: NaiveDate,
    run_id: Uuid,
    m: &RunManifest,
) -> Result<()> {
    let key = manifest_key(run_date, run_id);
    let body = serde_json::to_vec_pretty(m)?;
    let meta = manifest_meta(m);
    sink.put_raw(&key, &body, &meta).await
}

/// Metadata for a manifest object, without changing its serialized body.
fn manifest_meta(manifest: &RunManifest) -> ObjectMeta {
    ObjectMeta(std::collections::BTreeMap::from([
        ("x-vendor".into(), "xero".into()),
        ("x-sync-type".into(), "manifest".into()),
        ("x-run-id".into(), manifest.run_id.clone()),
        ("x-synced-at".into(), manifest.finished_at.clone()),
        ("x-run-started-at".into(), manifest.started_at.clone()),
        ("x-http-status".into(), "200".into()),
        ("x-record-count".into(), "0".into()),
        ("x-tenant-id".into(), "run".into()),
        ("x-endpoint".into(), "sync_manifest".into()),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::EntityOutcome;
    use std::collections::BTreeMap;

    #[test]
    fn only_transient_gcs_responses_are_retried() {
        for code in [408, 429, 500, 503] {
            let error =
                gcloud_storage::http::Error::Response(gcloud_storage::http::error::ErrorResponse {
                    code,
                    errors: vec![],
                    message: "transient".into(),
                });
            assert!(is_retryable_upload_error(&error));
        }
        for code in [400, 401, 403, 404] {
            let error =
                gcloud_storage::http::Error::Response(gcloud_storage::http::error::ErrorResponse {
                    code,
                    errors: vec![],
                    message: "permanent".into(),
                });
            assert!(!is_retryable_upload_error(&error));
        }
    }

    #[test]
    fn recovered_upload_requires_matching_content_identity() {
        let expected = upload_hash(b"raw response");
        let expected_crc = upload_crc32c(b"raw response");
        let metadata = HashMap::from([("x-upload-sha256".into(), expected.clone())]);
        let matching = Object {
            generation: 123,
            size: 12,
            crc32c: Some(expected_crc.clone()),
            content_type: Some("application/json".into()),
            metadata: Some(metadata.clone()),
            ..Default::default()
        };
        assert!(existing_upload_matches(
            &matching,
            &metadata,
            &expected_crc,
            12,
            "application/json"
        ));
        assert!(!existing_upload_matches(
            &matching,
            &metadata,
            &expected_crc,
            13,
            "application/json"
        ));
        assert!(!existing_upload_matches(
            &matching,
            &metadata,
            &expected_crc,
            12,
            "application/pdf"
        ));
        assert!(!existing_upload_matches(
            &matching,
            &HashMap::from([("x-upload-sha256".into(), upload_hash(b"other"))]),
            &expected_crc,
            12,
            "application/json"
        ));
        assert!(!existing_upload_matches(
            &matching,
            &HashMap::from([
                ("x-upload-sha256".into(), expected.clone()),
                ("x-run-id".into(), "different-run".into()),
            ]),
            &expected_crc,
            12,
            "application/json"
        ));
        assert!(!existing_upload_matches(
            &matching,
            &metadata,
            "AAAAAA==",
            12,
            "application/json"
        ));
    }

    #[test]
    fn crc32c_matches_standard_check_vector() {
        assert_eq!(upload_crc32c(b"123456789"), "4waSgw==");
    }

    #[test]
    fn precondition_failure_is_not_a_transient_upload_error() {
        let error = GcsHttpError::Response(gcloud_storage::http::error::ErrorResponse {
            code: 412,
            errors: vec![],
            message: "precondition failed".into(),
        });
        assert!(is_upload_precondition_failure(&error));
        assert!(!is_retryable_upload_error(&error));
    }

    fn sample_meta() -> ObjectMeta {
        let mut map = BTreeMap::new();
        map.insert("x-vendor".to_string(), "xero".to_string());
        map.insert("x-endpoint".to_string(), "invoices".to_string());
        map.insert("x-page".to_string(), "1".to_string());
        ObjectMeta(map)
    }

    fn fresh_meta() -> ObjectMeta {
        ObjectMeta(
            sample_meta()
                .0
                .into_iter()
                .chain([
                    ("x-source-system".into(), "xero_v2".into()),
                    (
                        "x-ingestion-generation".into(),
                        "550e8400-e29b-41d4-a716-446655440000".into(),
                    ),
                    ("x-data-generation".into(), "old-generation".into()),
                    ("x-data-crc32c".into(), "old-checksum".into()),
                ])
                .collect(),
        )
    }

    #[test]
    fn fresh_sidecar_meta_pins_uploaded_identity_without_mutating_original() {
        let meta = fresh_meta();
        let original = meta.clone();
        let uploaded = Object {
            generation: i64::MAX,
            crc32c: Some("ImIEBA==".into()),
            ..Default::default()
        };

        let sidecar = data_sidecar_meta(&meta, &uploaded).unwrap();

        assert_eq!(
            sidecar.get("x-data-generation"),
            Some("9223372036854775807")
        );
        assert_eq!(sidecar.get("x-data-crc32c"), Some("ImIEBA=="));
        assert_eq!(sidecar.len(), meta.len());
        for (key, value) in &original.0 {
            if !matches!(key.as_str(), "x-data-generation" | "x-data-crc32c") {
                assert_eq!(sidecar.get(key), Some(value.as_str()));
            }
        }
        assert_eq!(meta, original);
    }

    #[test]
    fn fresh_sidecar_meta_requires_positive_uploaded_generation() {
        let meta = fresh_meta();
        let original = meta.clone();
        for generation in [0, -1] {
            let uploaded = Object {
                generation,
                crc32c: Some("ImIEBA==".into()),
                ..Default::default()
            };
            let result = data_sidecar_meta(&meta, &uploaded);
            assert!(
                matches!(result, Err(GcsError::Upload(message)) if message.contains("generation"))
            );
        }
        assert_eq!(meta, original);
    }

    #[test]
    fn fresh_sidecar_meta_requires_uploaded_crc32c() {
        let meta = fresh_meta();
        let original = meta.clone();
        for crc32c in [None, Some(String::new()), Some(" \t".into())] {
            let uploaded = Object {
                generation: 123,
                crc32c,
                ..Default::default()
            };
            let result = data_sidecar_meta(&meta, &uploaded);
            assert!(matches!(result, Err(GcsError::Upload(message)) if message.contains("crc32c")));
        }
        assert_eq!(meta, original);
    }

    #[test]
    fn fresh_sidecar_meta_adds_identity_fields_to_a_new_map() {
        let meta = ObjectMeta(BTreeMap::from([
            ("x-source-system".into(), "xero_v2".into()),
            ("x-custom".into(), "keep-me".into()),
        ]));
        let original = meta.clone();
        let uploaded = Object {
            generation: 123,
            crc32c: Some("AAAAAA==".into()),
            ..Default::default()
        };
        let sidecar = data_sidecar_meta(&meta, &uploaded).unwrap();
        assert_eq!(sidecar.get("x-data-generation"), Some("123"));
        assert_eq!(sidecar.get("x-data-crc32c"), Some("AAAAAA=="));
        assert_eq!(sidecar.get("x-custom"), Some("keep-me"));
        assert_eq!(sidecar.len(), meta.len() + 2);
        assert_eq!(meta, original);
    }

    #[test]
    fn main_xero_sidecar_meta_pins_uploaded_identity() {
        let meta = ObjectMeta(BTreeMap::from([
            ("x-source-system".into(), "xero".into()),
            ("x-custom".into(), "keep-me".into()),
        ]));
        let uploaded = Object {
            generation: 123,
            crc32c: Some("AAAAAA==".into()),
            ..Default::default()
        };

        let sidecar = data_sidecar_meta(&meta, &uploaded).unwrap();

        assert_eq!(sidecar.get("x-data-generation"), Some("123"));
        assert_eq!(sidecar.get("x-data-crc32c"), Some("AAAAAA=="));
        assert_eq!(sidecar.get("x-custom"), Some("keep-me"));
    }

    #[test]
    fn legacy_sidecar_meta_preserves_old_behavior_without_requiring_identity() {
        let legacy = ObjectMeta(
            sample_meta()
                .0
                .into_iter()
                .chain([
                    ("x-source-system".into(), "xero_v1".into()),
                    ("x-data-generation".into(), "existing-value".into()),
                ])
                .collect(),
        );
        for meta in [sample_meta(), legacy] {
            let original = meta.clone();
            for uploaded in [
                Object::default(),
                Object {
                    generation: 123,
                    crc32c: Some("ImIEBA==".into()),
                    ..Default::default()
                },
            ] {
                assert_eq!(data_sidecar_meta(&meta, &uploaded).unwrap(), original);
                assert_eq!(meta, original);
            }
        }
    }

    #[tokio::test]
    async fn local_dir_sink_round_trips_object_and_sidecar() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sink = LocalDirSink::new(dir.path());

        let key = "raw/xero/t/2.0/invoices/2026-06-17/TS_rid_p001.json";
        let body = br#"{"Id":"x","Status":"OK","Invoices":[]}"#;
        let meta = sample_meta();

        sink.put_raw(key, body, &meta).await.expect("put_raw");

        // Object body matches verbatim.
        let obj_path = dir.path().join(key);
        let read_body = tokio::fs::read(&obj_path).await.expect("read object");
        assert_eq!(read_body, body);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = tokio::fs::metadata(&obj_path)
                .await
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        // Sidecar exists and parses back to the same map.
        let side_path = dir.path().join(sidecar_key(key));
        let read_side = tokio::fs::read(&side_path).await.expect("read sidecar");
        let parsed: BTreeMap<String, String> =
            serde_json::from_slice(&read_side).expect("parse sidecar");
        assert_eq!(parsed, meta.0);
    }

    #[tokio::test]
    async fn local_dir_sink_preserves_binary_response_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let sink = LocalDirSink::new(dir.path());
        let key = "raw/xero/t/2.0/accounting_get_getInvoiceAsPdf/2026-10-02/doc.pdf";
        let body = b"%PDF-1.7\0\xff";
        sink.put_raw_typed(key, body, &sample_meta(), "application/pdf")
            .await
            .unwrap();
        assert_eq!(tokio::fs::read(dir.path().join(key)).await.unwrap(), body);
        assert!(dir.path().join(format!("{key}.meta.json")).exists());
    }

    #[tokio::test]
    async fn local_dir_sink_rejects_parent_dir_traversal_key() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sink = LocalDirSink::new(dir.path());
        let err = sink
            .put_raw("../../etc/passwd", b"{}", &sample_meta())
            .await
            .expect_err("traversal key must be rejected");
        assert!(
            matches!(err, GcsError::Io(_)),
            "expected Io error, got {err:?}"
        );
        // Nothing should have been written outside the root.
        assert!(!dir.path().parent().unwrap().join("etc/passwd").exists());
    }

    #[tokio::test]
    async fn local_dir_sink_rejects_absolute_key() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sink = LocalDirSink::new(dir.path());
        let err = sink
            .put_raw("/tmp/xero_escape_attempt.json", b"{}", &sample_meta())
            .await
            .expect_err("absolute key must be rejected");
        assert!(
            matches!(err, GcsError::Io(_)),
            "expected Io error, got {err:?}"
        );
    }

    #[tokio::test]
    async fn local_dir_sink_allows_interior_parent_dir() {
        // A `..` that stays within root must still be allowed.
        let dir = tempfile::tempdir().expect("tempdir");
        let sink = LocalDirSink::new(dir.path());
        sink.put_raw("a/b/../c/file.json", b"{}", &sample_meta())
            .await
            .expect("interior .. should resolve within root");
        assert!(dir.path().join("a/c/file.json").exists());
    }

    #[tokio::test]
    async fn local_dir_sink_creates_nested_directories() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sink = LocalDirSink::new(dir.path());
        let key = "a/deeply/nested/key/file.json";
        sink.put_raw(key, b"{}", &sample_meta())
            .await
            .expect("put_raw");
        assert!(dir.path().join(key).exists());
    }

    #[tokio::test]
    async fn write_manifest_lands_outside_raw_prefix() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sink = LocalDirSink::new(dir.path());
        let run_date = NaiveDate::from_ymd_opt(2026, 6, 17).expect("date");
        let run_id = Uuid::parse_str("5f3c9a2e-0000-0000-0000-000000000000").expect("uuid");

        let m = RunManifest {
            run_id: run_id.to_string(),
            started_at: "2026-06-17T03:00:00Z".to_string(),
            finished_at: "2026-06-17T03:01:00Z".to_string(),
            mode: "incremental".to_string(),
            window_days: Some(3),
            entities: vec![EntityOutcome {
                tenant: "t".to_string(),
                endpoint: "invoices".to_string(),
                pages: 1,
                records: 1,
                termination: "empty-page".to_string(),
                request_key: None,
                error: None,
                outcome_class: None,
                http_status: None,
            }],
        };

        write_manifest(&sink, run_date, run_id, &m)
            .await
            .expect("write_manifest");

        let key = manifest_key(run_date, run_id);
        assert!(!key.starts_with("raw/"));
        let path = dir.path().join(&key);
        assert!(path.exists(), "manifest not written at {key}");
        let back: RunManifest =
            serde_json::from_slice(&tokio::fs::read(&path).await.expect("read"))
                .expect("parse manifest");
        assert_eq!(back, m);
        assert_eq!(
            tokio::fs::read(&path).await.unwrap(),
            serde_json::to_vec_pretty(&m).unwrap()
        );
        let sidecar = tokio::fs::read(dir.path().join(sidecar_key(&key)))
            .await
            .unwrap();
        let tags: BTreeMap<String, String> = serde_json::from_slice(&sidecar).unwrap();
        assert_eq!(
            tags,
            BTreeMap::from([
                ("x-vendor".into(), "xero".into()),
                ("x-sync-type".into(), "manifest".into()),
                ("x-run-id".into(), m.run_id.clone()),
                ("x-synced-at".into(), m.finished_at.clone()),
                ("x-run-started-at".into(), m.started_at.clone()),
                ("x-http-status".into(), "200".into()),
                ("x-record-count".into(), "0".into()),
                ("x-tenant-id".into(), "run".into()),
                ("x-endpoint".into(), "sync_manifest".into()),
            ])
        );
    }
}
