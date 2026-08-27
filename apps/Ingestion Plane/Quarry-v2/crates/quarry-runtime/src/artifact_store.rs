//! Artifact store trait. Impls: in-memory (dev), filesystem (prod).

use async_trait::async_trait;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::sync::RwLock;
use tracing::warn;

use quarry_core::artifact as artifact_meta;
use quarry_core::ids::kinds::{ArtifactKind, RunKind};
use quarry_core::ids::Id;
use quarry_core::{error::ErrorCode, error::QuarryError, QuarryResult};

/// Map kind string -> enum used to compute object_key.
fn kind_from_str(kind: &str) -> QuarryResult<artifact_meta::ArtifactKind> {
    use artifact_meta::ArtifactKind as K;
    match kind {
        "html" => Ok(K::Html),
        "markdown" | "md" => Ok(K::Markdown),
        "raw" => Ok(K::Raw),
        "links" => Ok(K::Links),
        "screenshot" => Ok(K::Screenshot),
        "screenshot_annotated" | "screenshot_annotated.png" => Ok(K::ScreenshotAnnotated),
        "pdf" => Ok(K::Pdf),
        "trace" => Ok(K::Trace),
        "extract" => Ok(K::Extract),
        "visual_change" | "visual_change.json" => Ok(K::VisualChange),
        "visual_observation" | "visual_observation.json" => Ok(K::VisualObservation),
        "evidence_delta" | "evidence_delta.json" => Ok(K::EvidenceDelta),
        "thumbnail" | "thumbnail.png" => Ok(K::Thumbnail),
        "tiles" | "tiles.json" => Ok(K::Tiles),
        "page_image_clean" | "page_image_clean.png" => Ok(K::PageImageClean),
        "ocr_preprocessed" | "ocr_preprocessed.png" => Ok(K::OcrPreprocessed),
        "logo_candidate" | "logo_candidate.png" => Ok(K::LogoCandidate),
        "rendered_palette" | "rendered_palette.json" => Ok(K::RenderedPalette),
        "meta" | "meta.json" => Ok(K::Meta),
        other => Err(QuarryError::new(
            ErrorCode::BadRequest,
            format!("unknown artifact kind: {other}"),
        )),
    }
}

#[derive(Debug, Clone)]
pub struct ArtifactHandle {
    pub artifact_id: ArtifactKind,
    pub key: String,
    pub bytes: u64,
}

/// Reads back the tenant segment of an object key produced by
/// [`quarry_core::artifact::object_key`] (`org={org}/run=.../page=.../stem.ext`).
/// Returns `None` for any key that is not tenant-stamped.
pub fn org_from_key(key: &str) -> Option<&str> {
    key.split('/').next()?.strip_prefix("org=")
}

/// The `org=` segment is the only tenant boundary the CAS has, so an org id
/// that contains a path separator would blur (or escape) it. Rejected at
/// `put` time rather than sanitized, so a malformed claim can never silently
/// write into another tenant's prefix.
fn validate_org(org_id: &str) -> QuarryResult<()> {
    if org_id.contains('/') || org_id.contains("..") {
        return Err(QuarryError::new(
            ErrorCode::BadRequest,
            "org id may not contain '/' or '..'",
        ));
    }
    Ok(())
}

/// Read-side tenant check shared by every backend: the artifact's stored
/// `org=` segment must equal the caller's verified org claim.
///
/// A mismatch is reported as `NotFound`, never `Forbidden` — artifact ids are
/// ULIDs, and a distinguishable "exists but not yours" would turn that into an
/// existence oracle. An empty requested org is refused outright so that
/// unattributed writes (`org=`) can't be read back by an unauthenticated or
/// org-less principal.
fn ensure_org_owns(requested_org: &str, key: &str, id: &ArtifactKind) -> QuarryResult<()> {
    let not_found = || QuarryError::new(ErrorCode::NotFound, format!("artifact {id} not found"));
    if requested_org.is_empty() {
        warn!(artifact = %id, "artifact read refused: caller has no org claim");
        return Err(not_found());
    }
    match org_from_key(key) {
        Some(owner) if owner == requested_org => Ok(()),
        owner => {
            warn!(
                artifact = %id,
                requested_org,
                owner = owner.unwrap_or("<untagged>"),
                "cross-tenant artifact read refused"
            );
            Err(not_found())
        }
    }
}

#[async_trait]
pub trait ArtifactStore: Send + Sync {
    /// Stores `body` and stamps it with the *caller's* org. `org_id` must come
    /// from a verified claim — it becomes the artifact's tenant of record and
    /// the only thing [`ArtifactStore::get`] will match a reader against.
    async fn put(
        &self,
        org_id: &str,
        run_id: &RunKind,
        page_hash: &str,
        kind: &str,
        body: Vec<u8>,
    ) -> QuarryResult<ArtifactHandle>;

    /// Reads an artifact by id on behalf of `org_id`. Artifacts owned by any
    /// other tenant resolve to `NotFound`, so possession of an id is not by
    /// itself authority to read the bytes.
    async fn get(&self, org_id: &str, id: &ArtifactKind) -> QuarryResult<Vec<u8>>;

    /// Bytes plus the object key they were stored under.
    ///
    /// Every backend already resolves the key on the way to the bytes and then
    /// throws it away — and the key is the only authoritative record of what the
    /// artifact IS (the producer chose the kind at `put` time and
    /// `quarry_core::artifact::object_key` encoded it). Discarding it is why the
    /// HTTP route had to serve everything as `application/octet-stream`, which
    /// made a captured screenshot an opaque download with nowhere to render.
    ///
    /// Defaults to `get` with an empty key so a third-party backend keeps
    /// compiling; an empty key resolves to the same opaque default it had
    /// before, never to a wrong type.
    async fn get_with_key(
        &self,
        org_id: &str,
        id: &ArtifactKind,
    ) -> QuarryResult<(Vec<u8>, String)> {
        Ok((self.get(org_id, id).await?, String::new()))
    }

    /// Cycle 22 / cluster #4 part 1 — paginated list of artifacts
    /// scoped to a single tenant. Returns an empty page by default so
    /// backends that don't index (e.g. S3 without a separate index) can
    /// stay listable-via-control-plane only. In-process backends
    /// (InMemoryStore, FilesystemStore) override this with real impls.
    async fn list(
        &self,
        _org_id: &str,
        _filter: &quarry_core::pagination::ListFilter,
    ) -> QuarryResult<quarry_core::pagination::Page<quarry_core::resources::ArtifactSummary>> {
        Ok(quarry_core::pagination::Page::empty())
    }

    /// Optional total count. Backends MAY return `None` when an exact
    /// count is too expensive (e.g. millions of S3 keys).
    async fn count(&self, _org_id: &str) -> QuarryResult<Option<u64>> {
        Ok(None)
    }
}

/// Per-artifact metadata kept alongside the body so `list()` can serve
/// without re-walking the body blob. Cycle 22 / cluster #4 part 1.
#[derive(Debug, Clone)]
struct InMemoryArtifactMeta {
    id: ArtifactKind,
    org_id: String,
    kind: String,
    bytes: u64,
    created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Default)]
pub struct InMemoryStore {
    inner: RwLock<HashMap<String, Vec<u8>>>,
    /// Append-only registry of metadata. Cheap because tests rarely
    /// store more than a handful of artifacts; production never uses
    /// this backend.
    meta: RwLock<Vec<InMemoryArtifactMeta>>,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ArtifactStore for InMemoryStore {
    async fn put(
        &self,
        org_id: &str,
        run_id: &RunKind,
        page_hash: &str,
        kind: &str,
        body: Vec<u8>,
    ) -> QuarryResult<ArtifactHandle> {
        validate_org(org_id)?;
        let id: ArtifactKind = Id::new();
        // Same `org=` leading segment as the durable backends so
        // `org_from_key` is the single tenant parser across all three.
        let key = format!("org={org_id}/{run_id}/{page_hash}/{kind}/{id}");
        let bytes = body.len() as u64;
        self.inner.write().await.insert(key.clone(), body);
        self.meta.write().await.push(InMemoryArtifactMeta {
            id: id.clone(),
            org_id: org_id.to_string(),
            kind: kind.to_string(),
            bytes,
            created_at: chrono::Utc::now(),
        });
        Ok(ArtifactHandle {
            artifact_id: id,
            key,
            bytes,
        })
    }

    async fn get(&self, org_id: &str, id: &ArtifactKind) -> QuarryResult<Vec<u8>> {
        Ok(self.get_with_key(org_id, id).await?.0)
    }

    async fn get_with_key(
        &self,
        org_id: &str,
        id: &ArtifactKind,
    ) -> QuarryResult<(Vec<u8>, String)> {
        let map = self.inner.read().await;
        let id_str = id.to_string();
        for (k, v) in map.iter() {
            if k.ends_with(&id_str) {
                ensure_org_owns(org_id, k, id)?;
                return Ok((v.clone(), k.clone()));
            }
        }
        Err(QuarryError::new(
            ErrorCode::NotFound,
            format!("artifact {id_str} not found"),
        ))
    }

    async fn list(
        &self,
        org_id: &str,
        filter: &quarry_core::pagination::ListFilter,
    ) -> QuarryResult<quarry_core::pagination::Page<quarry_core::resources::ArtifactSummary>> {
        use quarry_core::pagination::{Cursor, Page};
        use quarry_core::resources::ArtifactSummary;

        let meta = self.meta.read().await;
        let mut rows: Vec<&InMemoryArtifactMeta> = meta
            .iter()
            // Tenant isolation: an artifact is only listable by the org that
            // wrote it (stamped from the writer's claim at `put`).
            .filter(|m| m.org_id == org_id)
            .filter(|_m| match &filter.status {
                // We don't track lifecycle status on artifacts in this
                // backend; surface "stored" as the implicit status so
                // a `?status=stored` filter matches and any other
                // status filter returns empty.
                Some(s) if s != "stored" => false,
                _ => true,
            })
            .filter(|m| match filter.created_before {
                Some(t) => m.created_at <= t,
                None => true,
            })
            .filter(|m| match filter.created_after {
                Some(t) => m.created_at >= t,
                None => true,
            })
            .collect();

        // Sort newest-first by default (matches dashboard expectations).
        if filter.sort.is_descending() {
            rows.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        } else {
            rows.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        }

        // Apply cursor: drop rows up to and including the cursor's
        // position so the next page starts strictly after it.
        if let Some(cur) = filter.decoded_cursor() {
            rows.retain(|m| {
                if filter.sort.is_descending() {
                    m.created_at < cur.created_at
                        || (m.created_at == cur.created_at && m.id.to_string() > cur.id)
                } else {
                    m.created_at > cur.created_at
                        || (m.created_at == cur.created_at && m.id.to_string() > cur.id)
                }
            });
        }

        let limit = filter.effective_limit() as usize;
        // Take one extra row so we can detect "more available" without
        // a separate count query.
        let mut window: Vec<&InMemoryArtifactMeta> = rows.iter().take(limit + 1).copied().collect();
        let has_more = window.len() > limit;
        if has_more {
            window.pop();
        }
        let next_cursor = if has_more {
            window
                .last()
                .map(|m| Cursor::new(m.created_at, m.id.to_string()))
        } else {
            None
        };

        let items: Vec<ArtifactSummary> = window
            .iter()
            .map(|m| ArtifactSummary {
                artifact_id: m.id.clone(),
                org_id: m.org_id.clone(),
                kind: m.kind.clone(),
                bytes: m.bytes,
                sha256: None,
                created_at: m.created_at,
                source_url: None,
            })
            .collect();
        Ok(Page::new(items, next_cursor, None))
    }

    async fn count(&self, org_id: &str) -> QuarryResult<Option<u64>> {
        let n = self
            .meta
            .read()
            .await
            .iter()
            .filter(|m| m.org_id == org_id)
            .count() as u64;
        Ok(Some(n))
    }
}

/// Filesystem-backed artifact store. Writes via tempfile + atomic rename.
/// Layout mirrors `quarry_core::artifact::object_key`:
///   {root}/org={org}/run={run_id}/page={page_hash}/{stem}.{ext}
pub struct FilesystemStore {
    root: PathBuf,
}

impl FilesystemStore {
    pub fn new(root: impl Into<PathBuf>) -> QuarryResult<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root).map_err(|e| {
            QuarryError::new(
                ErrorCode::Internal,
                format!("create artifact root {}: {e}", root.display()),
            )
        })?;
        Ok(Self { root })
    }

    fn abs_path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// Index file maps artifact_id -> relative key so `get` can resolve by id.
    fn index_path(&self) -> PathBuf {
        self.root.join(".index")
    }

    async fn record_index(&self, id: &ArtifactKind, key: &str) -> QuarryResult<()> {
        let line = format!("{id}\t{key}\n");
        let path = self.index_path();
        tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?;
            f.write_all(line.as_bytes())
        })
        .await
        .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("index join: {e}")))?
        .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("index write: {e}")))
    }

    async fn lookup_index(&self, id: &ArtifactKind) -> QuarryResult<Option<String>> {
        let path = self.index_path();
        let id_str = id.to_string();
        let content = match tokio::fs::read_to_string(&path).await {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(QuarryError::new(
                    ErrorCode::Internal,
                    format!("index read: {e}"),
                ))
            }
        };
        for line in content.lines() {
            if let Some((k, v)) = line.split_once('\t') {
                if k == id_str {
                    return Ok(Some(v.to_string()));
                }
            }
        }
        Ok(None)
    }
}

#[async_trait]
impl ArtifactStore for FilesystemStore {
    async fn put(
        &self,
        org_id: &str,
        run_id: &RunKind,
        page_hash: &str,
        kind: &str,
        body: Vec<u8>,
    ) -> QuarryResult<ArtifactHandle> {
        validate_org(org_id)?;
        let kind_enum = kind_from_str(kind)?;
        let key = artifact_meta::object_key(org_id, &run_id.to_string(), page_hash, kind_enum);
        let dest = self.abs_path(&key);
        let parent = dest
            .parent()
            .ok_or_else(|| QuarryError::new(ErrorCode::Internal, "artifact path has no parent"))?
            .to_path_buf();
        let bytes_len = body.len() as u64;

        // Blocking fs ops on the pool: mkdir, write tempfile, atomic persist.
        let dest_clone = dest.clone();
        tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            use std::io::Write;
            std::fs::create_dir_all(&parent)?;
            let mut tmp = tempfile::NamedTempFile::new_in(&parent)?;
            tmp.write_all(&body)?;
            tmp.as_file().sync_all()?;
            tmp.persist(&dest_clone).map_err(|e| e.error)?;
            Ok(())
        })
        .await
        .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("fs join: {e}")))?
        .map_err(|e| {
            QuarryError::new(
                ErrorCode::Internal,
                format!("fs write {}: {e}", dest.display()),
            )
        })?;

        let id: ArtifactKind = Id::new();
        self.record_index(&id, &key).await?;
        Ok(ArtifactHandle {
            artifact_id: id,
            key,
            bytes: bytes_len,
        })
    }

    async fn get(&self, org_id: &str, id: &ArtifactKind) -> QuarryResult<Vec<u8>> {
        Ok(self.get_with_key(org_id, id).await?.0)
    }

    async fn get_with_key(
        &self,
        org_id: &str,
        id: &ArtifactKind,
    ) -> QuarryResult<(Vec<u8>, String)> {
        let key = self.lookup_index(id).await?.ok_or_else(|| {
            QuarryError::new(ErrorCode::NotFound, format!("artifact {id} not found"))
        })?;
        ensure_org_owns(org_id, &key, id)?;
        let path = self.abs_path(&key);
        let bytes = tokio::fs::read(&path).await.map_err(|e| {
            QuarryError::new(
                ErrorCode::Internal,
                format!("fs read {}: {e}", path.display()),
            )
        })?;
        Ok((bytes, key))
    }
}

// Silence unused import when tests not compiled.
#[allow(dead_code)]
fn _assert_path_trait<P: AsRef<Path>>(_p: P) {}

/// S3-backed artifact store.
/// Layout matches `artifact_meta::object_key`; an index object at
/// `.index/{artifact_id}` stores the key so `get` can resolve by id.
pub struct S3Store {
    client: aws_sdk_s3::Client,
    bucket: String,
}

impl S3Store {
    pub async fn new(bucket: String) -> QuarryResult<Self> {
        let cfg = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        let client = aws_sdk_s3::Client::new(&cfg);
        Ok(Self { client, bucket })
    }

    pub fn from_parts(client: aws_sdk_s3::Client, bucket: String) -> Self {
        Self { client, bucket }
    }

    fn index_key(id: &ArtifactKind) -> String {
        format!(".index/{id}")
    }
}

#[async_trait]
impl ArtifactStore for S3Store {
    async fn put(
        &self,
        org_id: &str,
        run_id: &RunKind,
        page_hash: &str,
        kind: &str,
        body: Vec<u8>,
    ) -> QuarryResult<ArtifactHandle> {
        validate_org(org_id)?;
        let kind_enum = kind_from_str(kind)?;
        let key = artifact_meta::object_key(org_id, &run_id.to_string(), page_hash, kind_enum);
        let bytes = body.len() as u64;

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&key)
            .body(aws_sdk_s3::primitives::ByteStream::from(body))
            .send()
            .await
            .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("s3 put {key}: {e}")))?;

        let id: ArtifactKind = Id::new();
        let idx_key = Self::index_key(&id);
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&idx_key)
            .body(aws_sdk_s3::primitives::ByteStream::from(
                key.clone().into_bytes(),
            ))
            .send()
            .await
            .map_err(|e| {
                QuarryError::new(ErrorCode::Internal, format!("s3 put index {idx_key}: {e}"))
            })?;

        Ok(ArtifactHandle {
            artifact_id: id,
            key,
            bytes,
        })
    }

    async fn get(&self, org_id: &str, id: &ArtifactKind) -> QuarryResult<Vec<u8>> {
        Ok(self.get_with_key(org_id, id).await?.0)
    }

    async fn get_with_key(
        &self,
        org_id: &str,
        id: &ArtifactKind,
    ) -> QuarryResult<(Vec<u8>, String)> {
        let idx_key = Self::index_key(id);
        let idx_resp = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&idx_key)
            .send()
            .await
            .map_err(|e| {
                QuarryError::new(ErrorCode::NotFound, format!("s3 index miss {idx_key}: {e}"))
            })?;
        let idx_bytes = idx_resp
            .body
            .collect()
            .await
            .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("s3 index read: {e}")))?
            .into_bytes();
        let key = String::from_utf8(idx_bytes.to_vec())
            .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("s3 index utf8: {e}")))?;
        ensure_org_owns(org_id, &key, id)?;

        let obj = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await
            .map_err(|e| QuarryError::new(ErrorCode::NotFound, format!("s3 get {key}: {e}")))?;
        let body = obj
            .body
            .collect()
            .await
            .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("s3 body read: {e}")))?
            .into_bytes();
        Ok((body.to_vec(), key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fs_store_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = FilesystemStore::new(tmp.path()).unwrap();
        let run_id: RunKind = Id::new();
        let body = b"<html>hi</html>".to_vec();
        let h = store
            .put(
                "org_alpha",
                &run_id,
                "blake3:deadbeef",
                "html",
                body.clone(),
            )
            .await
            .unwrap();
        assert_eq!(h.bytes, body.len() as u64);
        let got = store.get("org_alpha", &h.artifact_id).await.unwrap();
        assert_eq!(got, body);
    }

    #[tokio::test]
    async fn fs_store_unknown_kind() {
        let tmp = tempfile::tempdir().unwrap();
        let store = FilesystemStore::new(tmp.path()).unwrap();
        let run_id: RunKind = Id::new();
        let err = store
            .put("org_alpha", &run_id, "blake3:x", "bogus", vec![])
            .await
            .unwrap_err();
        assert!(format!("{err:?}").contains("bogus"));
    }

    // ---- Tenant binding on read-by-id ------------------------------------

    #[tokio::test]
    async fn fs_store_get_refuses_cross_tenant_id() {
        // The whole point of the artifact read contract: holding a valid id is
        // not authority to read the bytes.
        let tmp = tempfile::tempdir().unwrap();
        let store = FilesystemStore::new(tmp.path()).unwrap();
        let run_id: RunKind = Id::new();
        let h = store
            .put(
                "org_alpha",
                &run_id,
                "blake3:x",
                "markdown",
                b"# secret".to_vec(),
            )
            .await
            .unwrap();

        let err = store.get("org_beta", &h.artifact_id).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound, "must not leak existence");
        // The owner still reads it.
        assert_eq!(
            store.get("org_alpha", &h.artifact_id).await.unwrap(),
            b"# secret".to_vec()
        );
    }

    #[tokio::test]
    async fn fs_store_get_refuses_empty_org() {
        let tmp = tempfile::tempdir().unwrap();
        let store = FilesystemStore::new(tmp.path()).unwrap();
        let run_id: RunKind = Id::new();
        let h = store
            .put(
                "",
                &run_id,
                "blake3:x",
                "markdown",
                b"# unattributed".to_vec(),
            )
            .await
            .unwrap();
        // An unattributed write must not be readable by an org-less caller
        // either — otherwise `org=` would match `org=`.
        let err = store.get("", &h.artifact_id).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
    }

    #[tokio::test]
    async fn in_memory_get_refuses_cross_tenant_id() {
        let store = InMemoryStore::new();
        let run_id: RunKind = Id::new();
        let h = store
            .put(
                "org_alpha",
                &run_id,
                "blake3:x",
                "html",
                b"<b>a</b>".to_vec(),
            )
            .await
            .unwrap();
        let err = store.get("org_beta", &h.artifact_id).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
        assert_eq!(
            store.get("org_alpha", &h.artifact_id).await.unwrap(),
            b"<b>a</b>".to_vec()
        );
    }

    #[tokio::test]
    async fn put_rejects_org_that_would_escape_the_tenant_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let store = FilesystemStore::new(tmp.path()).unwrap();
        let run_id: RunKind = Id::new();
        for bad in ["../other", "org_a/org_b"] {
            let err = store
                .put(bad, &run_id, "blake3:x", "html", b"x".to_vec())
                .await
                .unwrap_err();
            assert_eq!(err.code, ErrorCode::BadRequest, "org {bad} must be refused");
        }
    }

    #[test]
    fn org_from_key_reads_the_tenant_segment() {
        assert_eq!(
            org_from_key("org=org_alpha/run=r/page=p/html.html"),
            Some("org_alpha")
        );
        assert_eq!(org_from_key("run=r/page=p/html.html"), None);
    }

    // ---- Cycle 22 / cluster #4 — list() + count() coverage ---------------

    #[tokio::test]
    async fn in_memory_list_returns_put_artifacts() {
        let store = InMemoryStore::new();
        let run_id: RunKind = Id::new();
        store
            .put(
                "org_alpha",
                &run_id,
                "blake3:p1",
                "html",
                b"<html>a</html>".to_vec(),
            )
            .await
            .unwrap();
        store
            .put(
                "org_alpha",
                &run_id,
                "blake3:p2",
                "markdown",
                b"# hi".to_vec(),
            )
            .await
            .unwrap();

        let filter = quarry_core::pagination::ListFilter::default();
        let page = store.list("org_alpha", &filter).await.unwrap();
        assert_eq!(page.items.len(), 2);
        // Newest-first default; markdown was inserted last so it leads.
        assert_eq!(page.items[0].kind, "markdown");
        assert_eq!(page.items[1].kind, "html");
        // total count via count() matches.
        assert_eq!(store.count("org_alpha").await.unwrap(), Some(2));
    }

    #[tokio::test]
    async fn in_memory_list_isolates_orgs() {
        // One process-wide store serves every tenant, so isolation has to come
        // from the per-`put` org stamp — not from which store instance was used.
        let store = InMemoryStore::new();
        let run: RunKind = Id::new();
        store
            .put("org_alpha", &run, "blake3:x", "html", b"a".to_vec())
            .await
            .unwrap();
        store
            .put("org_beta", &run, "blake3:y", "html", b"b".to_vec())
            .await
            .unwrap();

        let p = store
            .list("org_beta", &quarry_core::pagination::ListFilter::default())
            .await
            .unwrap();
        assert_eq!(p.items.len(), 1, "each org sees only its own artifact");
        assert_eq!(p.items[0].org_id, "org_beta");
        assert_eq!(store.count("org_alpha").await.unwrap(), Some(1));
    }

    #[tokio::test]
    async fn in_memory_list_cursor_paginates_correctly() {
        let store = InMemoryStore::new();
        let run: RunKind = Id::new();
        // Insert 5 artifacts.
        for i in 0..5 {
            store
                .put(
                    "org_a",
                    &run,
                    &format!("blake3:{i}"),
                    "html",
                    format!("{i}").into(),
                )
                .await
                .unwrap();
            // Tick to ensure distinct created_at values for ordering.
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
        let page1 = store
            .list(
                "org_a",
                &quarry_core::pagination::ListFilter {
                    limit: Some(2),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(page1.items.len(), 2);
        assert!(page1.next_cursor.is_some(), "more pages available");

        let page2 = store
            .list(
                "org_a",
                &quarry_core::pagination::ListFilter {
                    limit: Some(2),
                    cursor: page1.next_cursor.clone(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        // Pages must NOT overlap.
        let p1_ids: std::collections::HashSet<_> = page1
            .items
            .iter()
            .map(|m| m.artifact_id.to_string())
            .collect();
        let p2_ids: std::collections::HashSet<_> = page2
            .items
            .iter()
            .map(|m| m.artifact_id.to_string())
            .collect();
        assert!(
            p1_ids.is_disjoint(&p2_ids),
            "page1 and page2 leaked rows: {:?} vs {:?}",
            p1_ids,
            p2_ids
        );
    }

    #[tokio::test]
    async fn default_artifact_store_list_returns_empty() {
        // S3Store has no list override — the trait's default must
        // return an empty page so callers get a typed shape without
        // 500s.
        struct StubStore;
        #[async_trait]
        impl ArtifactStore for StubStore {
            async fn put(
                &self,
                _: &str,
                _: &RunKind,
                _: &str,
                _: &str,
                _: Vec<u8>,
            ) -> QuarryResult<ArtifactHandle> {
                unimplemented!()
            }
            async fn get(&self, _: &str, _: &ArtifactKind) -> QuarryResult<Vec<u8>> {
                unimplemented!()
            }
        }
        let p = StubStore
            .list("org_a", &quarry_core::pagination::ListFilter::default())
            .await
            .unwrap();
        assert!(p.items.is_empty());
        assert!(p.next_cursor.is_none());
    }
}
