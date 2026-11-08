//! Artifact store trait. Impls: in-memory (dev), filesystem (prod).

use async_trait::async_trait;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::warn;

use quarry_core::artifact as artifact_meta;
use quarry_core::ids::kinds::{ArtifactKind, RunKind};
use quarry_core::ids::Id;
use quarry_core::{error::ErrorCode, error::QuarryError, QuarryResult};

use crate::pipeline::{catch_scanner_panic, PiiRedactionSettings, PII_REDACTION_FAILED};

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
        "scrape_record" | "scrape_record.json" => Ok(K::ScrapeRecord),
        "screenshot_jpeg" | "screenshot.jpg" => Ok(K::ScreenshotJpeg),
        "change_diff" | "change_diff.json" => Ok(K::ChangeDiff),
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

// ---------------------------------------------------------------------------
// E3.6-4b: the sink-side PII gate.
//
// The pipeline's redaction stage (E3.6-4a, `pipeline.rs`) redacts every text
// form of a page before anything is kept. This gate is the second line: a sink
// that is about to persist content for an org whose content must be redacted
// checks it itself with the same scanner (`quarry_transform::pii`) and refuses
// anything that is not clean. It never redacts or repairs: content that is not
// already clean is refused with the typed `pii_redaction_failed` failure, so a
// caller that skipped the stage (or a path that has no stage, such as the
// search and answer write-backs) cannot persist unredacted text.
//
// Which orgs: the gate reads the same settings as the stage
// (`QUARRY_EDGE__PII_REDACTION`, default off, and
// `QUARRY_EDGE__PII_REDACTION_DEFAULT_ORGS`). With the flag off it does
// nothing. A sink cannot see a per-request `redact_pii`; a caller that runs a
// redacting request wraps its store in [`PiiEnforcingStore`], which checks
// every write whatever the settings say.
// ---------------------------------------------------------------------------

/// E3.6-4b: the most text one sink check scans. The same cap as the pipeline
/// stage, so a text the stage accepted is never refused for size alone.
pub const SINK_PII_MAX_BYTES: usize = 16 * 1024 * 1024;

/// E3.6-4b: why a sink refused a write. Each is a typed `details.cause`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkPiiRefusal {
    /// The text is over [`SINK_PII_MAX_BYTES`], so it was not scanned.
    TooLarge,
    /// The scanner panicked.
    ScannerPanicked,
    /// The scanner found personal data: the content was not redacted.
    ResidualPii,
    /// A text kind whose bytes are not UTF-8, so they cannot be scanned.
    NotText,
    /// An image, PDF, trace or raw body: text redaction cannot reach it.
    CaptureNotRedactable,
}

impl SinkPiiRefusal {
    /// The wire name of the cause. The stage's causes use the same names.
    pub const fn label(self) -> &'static str {
        match self {
            Self::TooLarge => "too_large",
            Self::ScannerPanicked => "scanner_panicked",
            Self::ResidualPii => "residual_pii",
            Self::NotText => "not_text",
            Self::CaptureNotRedactable => "capture_not_redactable",
        }
    }
}

/// E3.6-4b: the typed failure a sink returns. The same `details.reason` as the
/// pipeline stage, so a page whose write was refused fails as a
/// `pii_redaction_failed` page. Not retryable, and nothing of the content in
/// it: the sink, the field (an artifact kind or a document field) and the cause.
pub fn sink_pii_error(refusal: SinkPiiRefusal, sink: &'static str, field: &str) -> QuarryError {
    QuarryError::new(
        ErrorCode::Forbidden,
        "a store refused content that was not redacted for personal data, so nothing of it was kept",
    )
    .with_details(serde_json::json!({
        "reason": PII_REDACTION_FAILED,
        "cause": refusal.label(),
        "sink": sink,
        "field": field,
    }))
}

/// E3.6-4b: `Ok` only when a scan of `text` finds nothing. A scanner panic is
/// caught and refused, never passed through.
pub fn check_text_redacted(text: &str) -> Result<(), SinkPiiRefusal> {
    if text.len() > SINK_PII_MAX_BYTES {
        return Err(SinkPiiRefusal::TooLarge);
    }
    let clean = catch_scanner_panic(|| quarry_transform::pii::scan(text).is_empty())
        .map_err(|_| SinkPiiRefusal::ScannerPanicked)?;
    if clean {
        Ok(())
    } else {
        Err(SinkPiiRefusal::ResidualPii)
    }
}

/// The artifact kinds that hold text (HTML, markdown or JSON) and can be
/// scanned. Every other kind is an image, a PDF, a trace or raw bytes.
fn is_text_artifact(kind: artifact_meta::ArtifactKind) -> bool {
    use artifact_meta::ArtifactKind as K;
    matches!(
        kind,
        K::Html
            | K::Markdown
            | K::Links
            | K::Extract
            | K::VisualChange
            | K::VisualObservation
            | K::EvidenceDelta
            | K::Tiles
            | K::RenderedPalette
            | K::Meta
            | K::ScrapeRecord
            | K::ChangeDiff
    )
}

/// E3.6-4b: whether an artifact body may be kept for an org whose content must
/// be redacted. A text kind must be UTF-8 and scan clean. Any other kind, and an
/// unknown kind, is refused: it cannot be scanned (fail closed).
pub fn check_artifact_redacted(kind: &str, body: &[u8]) -> Result<(), SinkPiiRefusal> {
    match kind_from_str(kind) {
        Ok(k) if is_text_artifact(k) => {
            let text = std::str::from_utf8(body).map_err(|_| SinkPiiRefusal::NotText)?;
            check_text_redacted(text)
        }
        _ => Err(SinkPiiRefusal::CaptureNotRedactable),
    }
}

#[cfg(test)]
thread_local! {
    static TEST_SINK_PII: std::cell::RefCell<Option<PiiRedactionSettings>> =
        const { std::cell::RefCell::new(None) };
}

/// The settings the sinks read. A test installs its own for its thread
/// ([`SinkPiiTestOverride`]) instead of touching the process environment.
fn sink_pii_settings() -> PiiRedactionSettings {
    #[cfg(test)]
    if let Some(settings) = TEST_SINK_PII.with(|slot| slot.borrow().clone()) {
        return settings;
    }
    PiiRedactionSettings::cached().clone()
}

/// Test-only: the sink settings for the current thread until dropped.
#[cfg(test)]
pub(crate) struct SinkPiiTestOverride;

#[cfg(test)]
impl SinkPiiTestOverride {
    pub(crate) fn install(settings: PiiRedactionSettings) -> Self {
        TEST_SINK_PII.with(|slot| *slot.borrow_mut() = Some(settings));
        Self
    }
}

#[cfg(test)]
impl Drop for SinkPiiTestOverride {
    fn drop(&mut self) {
        TEST_SINK_PII.with(|slot| *slot.borrow_mut() = None);
    }
}

/// E3.6-4b: whether content this org persists must already be redacted: the
/// stage is on and the org's default covers it. Off by default (the flag).
pub fn sink_redaction_required(org_id: &str) -> bool {
    let org = org_id.trim();
    sink_pii_settings().applies(false, if org.is_empty() { None } else { Some(org) })
}

/// The gate every backend's `put` runs before it writes anything.
fn gate_artifact_put(org_id: &str, kind: &str, body: &[u8]) -> QuarryResult<()> {
    if sink_redaction_required(org_id) {
        check_artifact_redacted(kind, body).map_err(|refusal| {
            warn!(
                kind,
                cause = refusal.label(),
                "artifact write refused: content not redacted for personal data"
            );
            sink_pii_error(refusal, "artifact", kind)
        })?;
    }
    Ok(())
}

/// E3.6-4b: an [`ArtifactStore`] that refuses every write that is not clean,
/// whatever the settings say. A caller running a request that asked for
/// redaction (`redact_pii: on`) wraps its store in this for that run, because
/// the backends can only see the org default. Reads, lists and counts pass
/// through unchanged.
pub struct PiiEnforcingStore {
    inner: Arc<dyn ArtifactStore>,
}

impl PiiEnforcingStore {
    pub fn new(inner: Arc<dyn ArtifactStore>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl ArtifactStore for PiiEnforcingStore {
    async fn put(
        &self,
        org_id: &str,
        run_id: &RunKind,
        page_hash: &str,
        kind: &str,
        body: Vec<u8>,
    ) -> QuarryResult<ArtifactHandle> {
        check_artifact_redacted(kind, &body).map_err(|refusal| {
            warn!(
                kind,
                cause = refusal.label(),
                "artifact write refused: content not redacted for personal data"
            );
            sink_pii_error(refusal, "artifact", kind)
        })?;
        self.inner.put(org_id, run_id, page_hash, kind, body).await
    }

    async fn get(&self, org_id: &str, id: &ArtifactKind) -> QuarryResult<Vec<u8>> {
        self.inner.get(org_id, id).await
    }

    async fn get_with_key(
        &self,
        org_id: &str,
        id: &ArtifactKind,
    ) -> QuarryResult<(Vec<u8>, String)> {
        self.inner.get_with_key(org_id, id).await
    }

    async fn list(
        &self,
        org_id: &str,
        filter: &quarry_core::pagination::ListFilter,
    ) -> QuarryResult<quarry_core::pagination::Page<quarry_core::resources::ArtifactSummary>> {
        self.inner.list(org_id, filter).await
    }

    async fn count(&self, org_id: &str) -> QuarryResult<Option<u64>> {
        self.inner.count(org_id).await
    }
}

/// The listing rules every in-process backend shares, so the memory store and the
/// filesystem store cannot drift apart: the `status` filter, the `created_*`
/// bounds, the sort, the cursor and the page size.
///
/// `key` gives a row's `(created_at, id)`. Rows are totally ordered by that pair,
/// in the filter's direction, so two rows created in the same instant (ULIDs carry
/// milliseconds) still have one fixed order, and a cursor resumes strictly after
/// the row it names. Returns the page window and the cursor of the next page
/// (`Some` only when at least one more row exists).
///
/// ZDR needs nothing here: a ZDR request never calls `put`, so there is no row.
fn list_window<T>(
    rows: Vec<T>,
    filter: &quarry_core::pagination::ListFilter,
    key: impl Fn(&T) -> (chrono::DateTime<chrono::Utc>, String),
) -> (Vec<T>, Option<quarry_core::pagination::Cursor>) {
    // Artifacts carry no lifecycle status; "stored" is the implicit one, so a
    // `?status=stored` filter matches and any other status returns nothing.
    if matches!(&filter.status, Some(status) if status != "stored") {
        return (Vec::new(), None);
    }
    let descending = filter.sort.is_descending();
    let mut keyed: Vec<((chrono::DateTime<chrono::Utc>, String), T)> = rows
        .into_iter()
        .map(|row| (key(&row), row))
        .filter(|((created_at, _), _)| filter.created_before.map_or(true, |t| *created_at <= t))
        .filter(|((created_at, _), _)| filter.created_after.map_or(true, |t| *created_at >= t))
        .collect();
    if descending {
        keyed.sort_by(|a, b| b.0.cmp(&a.0));
    } else {
        keyed.sort_by(|a, b| a.0.cmp(&b.0));
    }
    if let Some(cursor) = filter.decoded_cursor() {
        let at = (cursor.created_at, cursor.id);
        keyed.retain(|(row_key, _)| if descending { *row_key < at } else { *row_key > at });
    }
    let limit = filter.effective_limit() as usize;
    let has_more = keyed.len() > limit;
    keyed.truncate(limit);
    let next = if has_more {
        keyed
            .last()
            .map(|((created_at, id), _)| quarry_core::pagination::Cursor::new(*created_at, id.clone()))
    } else {
        None
    };
    (keyed.into_iter().map(|(_, row)| row).collect(), next)
}

/// Per-artifact metadata kept alongside the body so `list()` can serve
/// without re-walking the body blob. Cycle 22 / cluster #4 part 1.
#[derive(Debug, Clone)]
struct InMemoryArtifactMeta {
    id: ArtifactKind,
    org_id: String,
    kind: String,
    run_id: String,
    page_hash: String,
    bytes: u64,
    created_at: chrono::DateTime<chrono::Utc>,
}

/// The run and page hash a durable storage key names:
/// `org=<org>/run=<run_id>/page=<page_hash>/<stem>.<ext>`.
fn run_and_page_from_key(key: &str) -> (Option<String>, Option<String>) {
    let mut run = None;
    let mut page = None;
    for segment in key.split('/') {
        if let Some(r) = segment.strip_prefix("run=") {
            run = Some(r.to_string());
        } else if let Some(p) = segment.strip_prefix("page=") {
            page = Some(p.to_string());
        }
    }
    (run, page)
}

/// `{stem}.{ext}`: the stem is the artifact kind and never contains a dot.
fn kind_from_key(key: &str) -> String {
    key.rsplit('/')
        .next()
        .and_then(|file| file.split('.').next())
        .unwrap_or_default()
        .to_string()
}

/// Whether a row passes the artifact-only `run_id` and `kind` filters (INNH-12).
fn matches_artifact_filters(
    filter: &quarry_core::pagination::ListFilter,
    run_id: Option<&str>,
    kind: &str,
) -> bool {
    filter.run_id.as_deref().map_or(true, |want| run_id == Some(want))
        && filter.kind.as_deref().map_or(true, |want| kind == want)
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
        // E3.6-4b: nothing unredacted is kept for an org that must redact.
        gate_artifact_put(org_id, kind, &body)?;
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
            run_id: run_id.to_string(),
            page_hash: page_hash.to_string(),
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
        use quarry_core::pagination::Page;
        use quarry_core::resources::ArtifactSummary;

        // An org-less caller lists nothing, as it reads nothing (`get` refuses an
        // empty org): unattributed writes are not browsable.
        if org_id.is_empty() {
            return Ok(Page::new(Vec::new(), None, None));
        }
        let rows: Vec<InMemoryArtifactMeta> = self
            .meta
            .read()
            .await
            .iter()
            // Tenant isolation: an artifact is only listable by the org that
            // wrote it (stamped from the writer's claim at `put`).
            .filter(|m| m.org_id == org_id)
            .filter(|m| matches_artifact_filters(filter, Some(m.run_id.as_str()), &m.kind))
            .cloned()
            .collect();
        let (window, next_cursor) = list_window(rows, filter, |m| (m.created_at, m.id.to_string()));
        let items: Vec<ArtifactSummary> = window
            .into_iter()
            .map(|m| ArtifactSummary {
                artifact_id: m.id,
                org_id: m.org_id,
                kind: m.kind,
                bytes: m.bytes,
                sha256: None,
                created_at: m.created_at,
                source_url: None,
                run_id: Some(m.run_id),
                page_hash: Some(m.page_hash),
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

/// One `.index` line of one organization, as `list` sees it.
struct FsRow {
    id: ArtifactKind,
    key: String,
    created_at: chrono::DateTime<chrono::Utc>,
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

    /// Every index row of `org_id`: `(id, key, created_at)`, in index order.
    ///
    /// The organization is the `org=` segment of the key, the same tenant boundary
    /// `get` checks, so a row of another organization is never returned. The time is
    /// the one inside the id (a ULID carries its creation time in milliseconds), so
    /// it survives a restart, a copy of the directory and a rewritten file.
    async fn index_rows(&self, org_id: &str) -> QuarryResult<Vec<FsRow>> {
        let content = match tokio::fs::read_to_string(self.index_path()).await {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(QuarryError::new(
                    ErrorCode::Internal,
                    format!("index read: {e}"),
                ))
            }
        };
        let mut rows = Vec::new();
        for line in content.lines() {
            let Some((id, key)) = line.split_once('\t') else {
                continue;
            };
            if org_from_key(key) != Some(org_id) {
                continue;
            }
            let Ok(id) = id.parse::<ArtifactKind>() else {
                continue;
            };
            let Some(created_at) = i64::try_from(id.ulid().timestamp_ms())
                .ok()
                .and_then(chrono::DateTime::from_timestamp_millis)
            else {
                continue;
            };
            rows.push(FsRow {
                id,
                key: key.to_string(),
                created_at,
            });
        }
        Ok(rows)
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
        // E3.6-4b: nothing unredacted is kept for an org that must redact.
        gate_artifact_put(org_id, kind, &body)?;
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

    /// Lists from the same `.index` file `get` resolves ids with, so what `put`
    /// wrote is listed after a restart, with the rules of [`InMemoryStore::list`]
    /// (see [`list_window`]). The size is read from the file for the page window
    /// only, not for every row of the organization. A row whose file has been
    /// removed from disk is skipped.
    async fn list(
        &self,
        org_id: &str,
        filter: &quarry_core::pagination::ListFilter,
    ) -> QuarryResult<quarry_core::pagination::Page<quarry_core::resources::ArtifactSummary>> {
        use quarry_core::pagination::Page;
        use quarry_core::resources::ArtifactSummary;

        if org_id.is_empty() {
            return Ok(Page::new(Vec::new(), None, None));
        }
        let mut rows = self.index_rows(org_id).await?;
        rows.retain(|row| {
            matches_artifact_filters(filter, run_and_page_from_key(&row.key).0.as_deref(), &kind_from_key(&row.key))
        });
        let (window, next_cursor) = list_window(rows, filter, |r| (r.created_at, r.id.to_string()));
        let mut items = Vec::with_capacity(window.len());
        for row in window {
            let path = self.abs_path(&row.key);
            let bytes = match tokio::fs::metadata(&path).await {
                Ok(meta) => meta.len(),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => {
                    return Err(QuarryError::new(
                        ErrorCode::Internal,
                        format!("fs stat {}: {e}", path.display()),
                    ))
                }
            };
            let kind = kind_from_key(&row.key);
            let (run_id, page_hash) = run_and_page_from_key(&row.key);
            items.push(ArtifactSummary {
                artifact_id: row.id,
                org_id: org_id.to_string(),
                kind,
                bytes,
                sha256: None,
                created_at: row.created_at,
                source_url: None,
                run_id,
                page_hash,
            });
        }
        Ok(Page::new(items, next_cursor, None))
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
        // E3.6-4b: nothing unredacted is kept for an org that must redact.
        gate_artifact_put(org_id, kind, &body)?;
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

    // ---- One list contract, run against both in-process stores (R7) --------

    use quarry_core::pagination::ListFilter;

    /// What one listed artifact looks like to a caller, minus the id.
    type Shape = (String, String, u64);

    /// The behaviour every in-process store must have. Puts five artifacts for
    /// `org_a` (alternating html and markdown, with distinct page hashes and bodies
    /// of distinct lengths), one for `org_b` and one unattributed, then checks
    /// scoping, order, limit, cursor, the status and time filters and the empty
    /// org. Returns what `org_a` sees, newest first, so two stores can be compared.
    async fn assert_list_contract(store: &dyn ArtifactStore) -> Vec<Shape> {
        let run: RunKind = Id::new();
        let mut put_ids = Vec::new();
        let mut put_shapes = Vec::new();
        for i in 0..5usize {
            let kind = if i % 2 == 0 { "html" } else { "markdown" };
            let body = vec![b'x'; i + 1];
            let handle = store
                .put("org_a", &run, &format!("blake3:{i}"), kind, body)
                .await
                .unwrap();
            put_ids.push(handle.artifact_id.to_string());
            put_shapes.push(("org_a".to_string(), kind.to_string(), (i + 1) as u64));
            // ULIDs carry milliseconds: tick so every artifact has its own time.
            tokio::time::sleep(std::time::Duration::from_millis(3)).await;
        }
        store
            .put("org_b", &run, "blake3:b", "html", b"other org".to_vec())
            .await
            .unwrap();
        store
            .put("", &run, "blake3:none", "html", b"unattributed".to_vec())
            .await
            .unwrap();

        let all = ListFilter::default();
        let page = store.list("org_a", &all).await.unwrap();
        assert!(page.next_cursor.is_none(), "five rows fit one default page");
        let newest_first: Vec<String> = page.items.iter().map(|m| m.artifact_id.to_string()).collect();
        let mut expected_ids = put_ids.clone();
        expected_ids.reverse();
        assert_eq!(newest_first, expected_ids, "newest first, by creation");
        let shapes: Vec<Shape> = page
            .items
            .iter()
            .map(|m| (m.org_id.clone(), m.kind.clone(), m.bytes))
            .collect();
        let mut expected_shapes = put_shapes.clone();
        expected_shapes.reverse();
        assert_eq!(shapes, expected_shapes, "org, kind and size of every row");
        assert!(page.items.iter().all(|m| m.org_id == "org_a"));

        // Oldest first is the same rows the other way round.
        let oldest = store
            .list("org_a", &ListFilter { sort: quarry_core::pagination::SortDirection::Oldest, ..Default::default() })
            .await
            .unwrap();
        let oldest_ids: Vec<String> = oldest.items.iter().map(|m| m.artifact_id.to_string()).collect();
        assert_eq!(oldest_ids, put_ids, "oldest first");

        // Limit, and a cursor walk that visits every row exactly once.
        for sort in [quarry_core::pagination::SortDirection::Newest, quarry_core::pagination::SortDirection::Oldest] {
            let full: Vec<String> = store
                .list("org_a", &ListFilter { sort, ..Default::default() })
                .await
                .unwrap()
                .items
                .iter()
                .map(|m| m.artifact_id.to_string())
                .collect();
            let mut walked = Vec::new();
            let mut cursor = None;
            let mut sizes = Vec::new();
            loop {
                let page = store
                    .list("org_a", &ListFilter { limit: Some(2), cursor: cursor.clone(), sort, ..Default::default() })
                    .await
                    .unwrap();
                sizes.push(page.items.len());
                walked.extend(page.items.iter().map(|m| m.artifact_id.to_string()));
                match page.next_cursor {
                    Some(next) => cursor = Some(next),
                    None => break,
                }
                assert!(sizes.len() < 10, "the cursor never ends");
            }
            assert_eq!(sizes, vec![2, 2, 1], "{sort:?}: page sizes");
            assert_eq!(walked, full, "{sort:?}: the walk equals the full list");
        }
        let one = store
            .list("org_a", &ListFilter { limit: Some(0), ..Default::default() })
            .await
            .unwrap();
        assert_eq!(one.items.len(), 1, "a limit of 0 is clamped to 1");
        assert!(one.next_cursor.is_some());

        // Status: "stored" is the implicit status, any other matches nothing.
        let stored = store
            .list("org_a", &ListFilter { status: Some("stored".into()), ..Default::default() })
            .await
            .unwrap();
        assert_eq!(stored.items.len(), 5);
        let failed = store
            .list("org_a", &ListFilter { status: Some("failed".into()), ..Default::default() })
            .await
            .unwrap();
        assert!(failed.items.is_empty());

        // Time bounds are inclusive and use the listed `created_at`.
        let third = &oldest.items[2];
        let before = store
            .list("org_a", &ListFilter { created_before: Some(third.created_at), ..Default::default() })
            .await
            .unwrap();
        assert_eq!(before.items.len(), 3, "the first three, the bound row included");
        let after = store
            .list("org_a", &ListFilter { created_after: Some(third.created_at), ..Default::default() })
            .await
            .unwrap();
        assert_eq!(after.items.len(), 3, "the last three, the bound row included");

        // Another organization sees only its own, an unknown one nothing, an
        // org-less caller nothing (not even the unattributed write).
        let b = store.list("org_b", &all).await.unwrap();
        assert_eq!(b.items.len(), 1);
        assert_eq!(b.items[0].org_id, "org_b");
        assert!(store.list("org_c", &all).await.unwrap().items.is_empty());
        assert!(store.list("", &all).await.unwrap().items.is_empty());

        shapes
    }

    #[tokio::test]
    async fn in_memory_store_meets_the_list_contract() {
        assert_list_contract(&InMemoryStore::new()).await;
    }

    #[tokio::test]
    async fn fs_store_meets_the_list_contract() {
        let tmp = tempfile::tempdir().unwrap();
        assert_list_contract(&FilesystemStore::new(tmp.path()).unwrap()).await;
    }

    #[tokio::test]
    async fn both_stores_list_the_same_shapes() {
        let tmp = tempfile::tempdir().unwrap();
        let fs = assert_list_contract(&FilesystemStore::new(tmp.path()).unwrap()).await;
        let memory = assert_list_contract(&InMemoryStore::new()).await;
        assert_eq!(fs, memory);
    }

    // ---- INNH-12: the run_id and kind filters ---------------------------------

    /// Two runs of `org_a` (each: html, markdown, a second page's markdown), one run of
    /// `org_b`. Checks the filters, the run and page hash on every row, and tenancy.
    async fn assert_run_and_kind_filters(store: &dyn ArtifactStore) {
        let run1: RunKind = Id::new();
        let run2: RunKind = Id::new();
        let run_b: RunKind = Id::new();
        for (org, run, page, kind) in [
            ("org_a", &run1, "blake3:p1", "html"),
            ("org_a", &run1, "blake3:p1", "markdown"),
            ("org_a", &run1, "blake3:p2", "markdown"),
            ("org_a", &run2, "blake3:p1", "markdown"),
            ("org_b", &run_b, "blake3:pb", "markdown"),
        ] {
            store.put(org, run, page, kind, b"x".to_vec()).await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(3)).await;
        }
        let list = |filter: ListFilter| async move { store.list("org_a", &filter).await.unwrap().items };

        let all = list(ListFilter::default()).await;
        assert_eq!(all.len(), 4);
        for row in &all {
            assert!(row.run_id.is_some() && row.page_hash.is_some(), "{row:?}");
        }

        let of_run1 = list(ListFilter { run_id: Some(run1.to_string()), ..Default::default() }).await;
        assert_eq!(of_run1.len(), 3);
        assert!(of_run1.iter().all(|r| r.run_id.as_deref() == Some(run1.to_string().as_str())));

        let md_run1 = list(ListFilter {
            run_id: Some(run1.to_string()),
            kind: Some("markdown".into()),
            ..Default::default()
        })
        .await;
        assert_eq!(md_run1.len(), 2);
        assert!(md_run1.iter().all(|r| r.kind == "markdown"));
        let mut pages: Vec<_> = md_run1.iter().filter_map(|r| r.page_hash.clone()).collect();
        pages.sort();
        assert_eq!(pages, ["blake3:p1", "blake3:p2"]);

        let md = list(ListFilter { kind: Some("markdown".into()), ..Default::default() }).await;
        assert_eq!(md.len(), 3, "markdown across both runs of the org");

        // Paging keeps the filter: two pages of one-row windows cover the run once.
        let first = store
            .list(
                "org_a",
                &ListFilter { run_id: Some(run1.to_string()), limit: Some(2), ..Default::default() },
            )
            .await
            .unwrap();
        assert_eq!(first.items.len(), 2);
        let second = store
            .list(
                "org_a",
                &ListFilter {
                    run_id: Some(run1.to_string()),
                    limit: Some(2),
                    cursor: first.next_cursor.clone(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(second.items.len(), 1);
        assert!(second.next_cursor.is_none());

        // Another organization's run id lists nothing for this org, and an unknown run too.
        let foreign = list(ListFilter { run_id: Some(run_b.to_string()), ..Default::default() }).await;
        assert!(foreign.is_empty(), "a run of org_b is invisible to org_a: {foreign:?}");
        let unknown = list(ListFilter { run_id: Some(RunKind::new().to_string()), ..Default::default() }).await;
        assert!(unknown.is_empty());
        let none_kind = list(ListFilter { kind: Some("pdf".into()), ..Default::default() }).await;
        assert!(none_kind.is_empty());
        // ...and org_b sees its own run through the same filter.
        let b = store
            .list("org_b", &ListFilter { run_id: Some(run_b.to_string()), ..Default::default() })
            .await
            .unwrap();
        assert_eq!(b.items.len(), 1);
    }

    #[tokio::test]
    async fn in_memory_store_filters_by_run_and_kind() {
        assert_run_and_kind_filters(&InMemoryStore::new()).await;
    }

    #[tokio::test]
    async fn fs_store_filters_by_run_and_kind() {
        let tmp = tempfile::tempdir().unwrap();
        assert_run_and_kind_filters(&FilesystemStore::new(tmp.path()).unwrap()).await;
    }

    #[tokio::test]
    async fn fs_store_filters_by_run_after_a_restart() {
        let tmp = tempfile::tempdir().unwrap();
        let run: RunKind = Id::new();
        let other: RunKind = Id::new();
        {
            let store = FilesystemStore::new(tmp.path()).unwrap();
            store.put("org_a", &run, "blake3:p", "markdown", b"a".to_vec()).await.unwrap();
            store.put("org_a", &other, "blake3:q", "markdown", b"b".to_vec()).await.unwrap();
        }
        let reopened = FilesystemStore::new(tmp.path()).unwrap();
        let page = reopened
            .list("org_a", &ListFilter { run_id: Some(run.to_string()), ..Default::default() })
            .await
            .unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].run_id.as_deref(), Some(run.to_string().as_str()));
    }

    #[test]
    fn the_run_and_page_come_from_the_storage_key() {
        assert_eq!(
            run_and_page_from_key("org=o/run=run_1/page=blake3:ab/markdown.md"),
            (Some("run_1".to_string()), Some("blake3:ab".to_string()))
        );
        assert_eq!(run_and_page_from_key("org=o/other"), (None, None));
        assert_eq!(kind_from_key("org=o/run=r/page=p/extract.json"), "extract");
    }

    #[tokio::test]
    async fn fs_store_lists_what_an_earlier_store_wrote() {
        let tmp = tempfile::tempdir().unwrap();
        let run: RunKind = Id::new();
        let mut ids = Vec::new();
        {
            let store = FilesystemStore::new(tmp.path()).unwrap();
            for i in 0..3 {
                let h = store
                    .put("org_a", &run, &format!("blake3:{i}"), "markdown", format!("# {i}").into_bytes())
                    .await
                    .unwrap();
                ids.push(h.artifact_id.to_string());
                tokio::time::sleep(std::time::Duration::from_millis(3)).await;
            }
            store
                .put("org_b", &run, "blake3:b", "html", b"<p>b</p>".to_vec())
                .await
                .unwrap();
        } // dropped: nothing of the first store is in memory any more.

        let reopened = FilesystemStore::new(tmp.path()).unwrap();
        let page = reopened.list("org_a", &ListFilter::default()).await.unwrap();
        let listed: Vec<String> = page.items.iter().map(|m| m.artifact_id.to_string()).collect();
        ids.reverse();
        assert_eq!(listed, ids, "the new store lists the records, newest first");
        assert!(page.items.iter().all(|m| m.kind == "markdown" && m.bytes == 3));
        // A listed id reads back through the same store.
        let first = &page.items[0];
        assert_eq!(reopened.get("org_a", &first.artifact_id).await.unwrap(), b"# 2".to_vec());
        // Another organization sees none of org_a's.
        let b = reopened.list("org_b", &ListFilter::default()).await.unwrap();
        assert_eq!(b.items.len(), 1);
        assert_eq!(b.items[0].kind, "html");
        assert!(reopened.list("org_z", &ListFilter::default()).await.unwrap().items.is_empty());
    }

    #[tokio::test]
    async fn fs_store_list_skips_a_row_whose_file_is_gone_and_a_damaged_index_line() {
        let tmp = tempfile::tempdir().unwrap();
        let store = FilesystemStore::new(tmp.path()).unwrap();
        let run: RunKind = Id::new();
        let keep = store
            .put("org_a", &run, "blake3:keep", "html", b"keep".to_vec())
            .await
            .unwrap();
        let gone = store
            .put("org_a", &run, "blake3:gone", "markdown", b"gone".to_vec())
            .await
            .unwrap();
        std::fs::remove_file(tmp.path().join(&gone.key)).unwrap();
        // A truncated last line (a crash mid-append) and a line that is not an id.
        let mut index = std::fs::read_to_string(tmp.path().join(".index")).unwrap();
        index.push_str("art_01TRUNCATED");
        index.push_str("\nnot-an-id\torg=org_a/run=r/page=p/html.html\n");
        std::fs::write(tmp.path().join(".index"), index).unwrap();

        let page = store.list("org_a", &ListFilter::default()).await.unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].artifact_id.to_string(), keep.artifact_id.to_string());
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


/// E3.6-4b: the sink-side PII gate.
#[cfg(test)]
mod pii_sink_tests {
    use super::*;
    use quarry_core::pagination::ListFilter;

    const EMAIL: &str = "ola.nordmann@example.no";

    fn org_default(orgs: &str) -> PiiRedactionSettings {
        PiiRedactionSettings::parse(Some("on"), Some(orgs))
    }

    fn assert_refused(err: &QuarryError, cause: &str, field: &str) {
        assert_eq!(err.code, ErrorCode::Forbidden);
        let details = err.details.as_ref().expect("typed details");
        assert_eq!(details["reason"], PII_REDACTION_FAILED);
        assert_eq!(details["cause"], cause);
        assert_eq!(details["sink"], "artifact");
        assert_eq!(details["field"], field);
        // Nothing of the content in the failure.
        assert!(!err.to_string().contains(EMAIL));
        assert!(!details.to_string().contains(EMAIL));
    }

    #[test]
    fn text_check_passes_clean_and_redacted_text_and_refuses_pii() {
        assert_eq!(check_text_redacted("Kontakt oss for et tilbud."), Ok(()));
        assert_eq!(
            check_text_redacted("Skriv til [redacted:pii.email] for et tilbud."),
            Ok(())
        );
        assert_eq!(
            check_text_redacted(&format!("Skriv til {EMAIL}.")),
            Err(SinkPiiRefusal::ResidualPii)
        );
    }

    #[test]
    fn text_over_the_cap_is_refused_unscanned() {
        let big = "a".repeat(SINK_PII_MAX_BYTES + 1);
        assert_eq!(check_text_redacted(&big), Err(SinkPiiRefusal::TooLarge));
    }

    #[test]
    fn artifact_check_scans_text_kinds_and_refuses_the_rest() {
        assert_eq!(check_artifact_redacted("markdown", b"# Kontakt"), Ok(()));
        assert_eq!(check_artifact_redacted("meta.json", b"{\"driver\":\"static\"}"), Ok(()));
        assert_eq!(
            check_artifact_redacted("markdown", format!("mail {EMAIL}").as_bytes()),
            Err(SinkPiiRefusal::ResidualPii)
        );
        assert_eq!(
            check_artifact_redacted("html", &[0xff, 0xfe, 0x00]),
            Err(SinkPiiRefusal::NotText)
        );
        for kind in ["screenshot", "pdf", "raw", "trace", "thumbnail.png", "not_a_kind"] {
            assert_eq!(
                check_artifact_redacted(kind, b"anything"),
                Err(SinkPiiRefusal::CaptureNotRedactable),
                "{kind}"
            );
        }
    }

    #[test]
    fn sinks_require_redaction_only_with_the_flag_on_and_the_org_covered() {
        {
            let _o = SinkPiiTestOverride::install(PiiRedactionSettings::default());
            assert!(!sink_redaction_required("org_a"));
        }
        {
            let _o = SinkPiiTestOverride::install(PiiRedactionSettings::parse(
                Some("off"),
                Some("*"),
            ));
            assert!(!sink_redaction_required("org_a"));
        }
        {
            let _o = SinkPiiTestOverride::install(org_default("org_a, org_b"));
            assert!(sink_redaction_required("org_a"));
            assert!(sink_redaction_required("org_b"));
            assert!(!sink_redaction_required("org_c"));
            assert!(!sink_redaction_required(""));
        }
        {
            let _o = SinkPiiTestOverride::install(org_default("*"));
            assert!(sink_redaction_required("org_c"));
        }
    }

    #[tokio::test]
    async fn memory_store_refuses_unredacted_text_for_a_covered_org_and_keeps_nothing() {
        let _o = SinkPiiTestOverride::install(org_default("org_a"));
        let store = InMemoryStore::new();
        let run_id: RunKind = Id::new();
        let err = store
            .put(
                "org_a",
                &run_id,
                "blake3:p",
                "markdown",
                format!("Skriv til {EMAIL}").into_bytes(),
            )
            .await
            .unwrap_err();
        assert_refused(&err, "residual_pii", "markdown");
        let page = store.list("org_a", &ListFilter::default()).await.unwrap();
        assert!(page.items.is_empty());

        let err = store
            .put("org_a", &run_id, "blake3:p", "screenshot", vec![0x89, b'P', b'N', b'G'])
            .await
            .unwrap_err();
        assert_refused(&err, "capture_not_redactable", "screenshot");

        // Clean text is kept.
        store
            .put(
                "org_a",
                &run_id,
                "blake3:p",
                "markdown",
                b"Skriv til [redacted:pii.email]".to_vec(),
            )
            .await
            .unwrap();
        // An org the default does not cover is not gated.
        store
            .put(
                "org_b",
                &run_id,
                "blake3:p",
                "markdown",
                format!("Skriv til {EMAIL}").into_bytes(),
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn memory_store_is_unchanged_with_the_flag_off() {
        let _o = SinkPiiTestOverride::install(PiiRedactionSettings::default());
        let store = InMemoryStore::new();
        let run_id: RunKind = Id::new();
        store
            .put(
                "org_a",
                &run_id,
                "blake3:p",
                "markdown",
                format!("Skriv til {EMAIL}").into_bytes(),
            )
            .await
            .unwrap();
        store
            .put("org_a", &run_id, "blake3:p", "screenshot", vec![1, 2, 3])
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn filesystem_store_refuses_before_writing_a_file() {
        let _o = SinkPiiTestOverride::install(org_default("org_a"));
        let tmp = tempfile::tempdir().unwrap();
        let store = FilesystemStore::new(tmp.path()).unwrap();
        let run_id: RunKind = Id::new();
        let err = store
            .put(
                "org_a",
                &run_id,
                "blake3:p",
                "html",
                format!("<p>{EMAIL}</p>").into_bytes(),
            )
            .await
            .unwrap_err();
        assert_refused(&err, "residual_pii", "html");
        assert!(
            !tmp.path().join("org=org_a").exists(),
            "a refused write leaves no file behind"
        );
        let page = store.list("org_a", &ListFilter::default()).await.unwrap();
        assert!(page.items.is_empty());
    }

    #[tokio::test]
    async fn enforcing_store_checks_every_write_whatever_the_settings() {
        let _o = SinkPiiTestOverride::install(PiiRedactionSettings::default());
        let inner: Arc<dyn ArtifactStore> = Arc::new(InMemoryStore::new());
        let store = PiiEnforcingStore::new(inner.clone());
        let run_id: RunKind = Id::new();
        let err = store
            .put(
                "org_a",
                &run_id,
                "blake3:p",
                "markdown",
                format!("Skriv til {EMAIL}").into_bytes(),
            )
            .await
            .unwrap_err();
        assert_refused(&err, "residual_pii", "markdown");
        let err = store
            .put("org_a", &run_id, "blake3:p", "pdf", b"%PDF-1.7".to_vec())
            .await
            .unwrap_err();
        assert_refused(&err, "capture_not_redactable", "pdf");
        assert!(inner
            .list("org_a", &ListFilter::default())
            .await
            .unwrap()
            .items
            .is_empty());

        let body = b"Skriv til [redacted:pii.email]".to_vec();
        let handle = store
            .put("org_a", &run_id, "blake3:p", "markdown", body.clone())
            .await
            .unwrap();
        assert_eq!(store.get("org_a", &handle.artifact_id).await.unwrap(), body);
    }
}
