//! INNH-4: the text diff of a monitored URL.
//!
//! A change monitor (`target_kind=change_monitor`) fetches its page on a schedule and
//! records a baseline when the page is new or changed. This module is what turns those
//! baselines into something a person can read:
//!
//! * the markdown artifact the fetch wrote is kept on the baseline (`artifact_id`);
//! * a changed check stores a JSON paragraph diff of the old and new text (artifact kind
//!   `change_diff`) and a `DiffRecord` pointing at it;
//! * `GET /v1/change/diff` returns the two versions to compare, org-scoped.
//!
//! All of it is behind `QUARRY_EDGE__CHANGE_TEXT_DIFF` (default off: the recording path is
//! exactly what it was, and the route answers 501). Nothing here exists under Zero Data
//! Retention: a ZDR run stores no page text, so there is no markdown id to keep, and the read
//! route refuses a ZDR reader. The store sits behind [`ChangeStore`] so the record and read
//! logic is tested without a database.

use async_trait::async_trait;

use quarry_core::change_history::{
    cut_utf8, BaselineSnapshot, ChangeDiffView, ChangeRecord, DiffMarkdown, DiffRecord, DiffSide,
    DiffUnavailable, ParagraphDiff, TextDiffArtifact, TextDiffOp, DIFF_SUMMARY_MAX_BYTES,
    PARAGRAPH_DIFF_FORMAT, TEXT_DIFF_FORMAT, TEXT_DIFF_MAX_MARKDOWN_BYTES, TEXT_DIFF_MAX_OPS,
    TEXT_DIFF_MAX_OP_TEXT_BYTES, TEXT_DIFF_PARAGRAPH_CHARS, TEXT_DIFF_VERSION,
};
use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::ids::kinds::{ArtifactKind as ArtifactId, RunKind};
use quarry_runtime::artifact_store::{ArtifactHandle, ArtifactStore};
use quarry_runtime::postgres_baseline_store::PostgresBaselineStore;

/// Turns the text diff on. Off (the default), baselines keep no text, a changed check stores
/// the one-line fingerprint note it always did, and `GET /v1/change/diff` answers 501.
pub const ENV_CHANGE_TEXT_DIFF: &str = "QUARRY_EDGE__CHANGE_TEXT_DIFF";

/// Whether a flag value switches the feature on.
pub fn flag_is_on(value: Option<&str>) -> bool {
    matches!(
        value.map(str::trim),
        Some("1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON")
    )
}

/// Whether the text diff is enabled. Read per request: it is one environment variable and a
/// change check is rare, so the read costs nothing and a flip needs no restart.
pub fn change_text_diff_enabled() -> bool {
    flag_is_on(std::env::var(ENV_CHANGE_TEXT_DIFF).ok().as_deref())
}

/// The checks every `GET /v1/change/diff` makes before it touches the store.
pub fn gate(enabled: bool, zdr_active: bool) -> Result<(), QuarryError> {
    if !enabled {
        return Err(QuarryError::new(
            ErrorCode::Unsupported,
            format!(
                "change text diffs are not enabled on this edge: set {ENV_CHANGE_TEXT_DIFF}=1 \
                 (see docs/RUNBOOKS.md, \"Change text diff\")"
            ),
        ));
    }
    if zdr_active {
        // A baseline's text is page content kept after the fact; a ZDR reader gets none of it,
        // as with a stored scrape record.
        return Err(QuarryError::new(
            ErrorCode::Forbidden,
            "change text diffs are not readable under Zero Data Retention",
        ));
    }
    Ok(())
}

// ---- the store ---------------------------------------------------------------

/// The baseline and diff store the record and read paths use. [`PostgresBaselineStore`] is the
/// one real implementation; every method is scoped to the org it is given.
#[async_trait]
pub trait ChangeStore: Send + Sync {
    async fn compare_snapshot(
        &self,
        org_id: &str,
        source_url: &str,
        fresh_fingerprint: &str,
    ) -> QuarryResult<ChangeRecord>;
    async fn save_baseline(&self, baseline: &BaselineSnapshot) -> QuarryResult<()>;
    async fn create_diff_record(&self, diff: &DiffRecord) -> QuarryResult<()>;
    async fn load_baseline(
        &self,
        org_id: &str,
        baseline_id: &str,
    ) -> QuarryResult<Option<BaselineSnapshot>>;
    async fn load_diff(&self, org_id: &str, diff_id: &str) -> QuarryResult<Option<DiffRecord>>;
    async fn find_diff(
        &self,
        org_id: &str,
        from_baseline_id: &str,
        to_baseline_id: &str,
    ) -> QuarryResult<Option<DiffRecord>>;
    async fn load_history(
        &self,
        org_id: &str,
        source_url: &str,
        limit: u32,
    ) -> QuarryResult<Vec<BaselineSnapshot>>;
}

#[async_trait]
impl ChangeStore for PostgresBaselineStore {
    async fn compare_snapshot(
        &self,
        org_id: &str,
        source_url: &str,
        fresh_fingerprint: &str,
    ) -> QuarryResult<ChangeRecord> {
        PostgresBaselineStore::compare_snapshot(self, org_id, source_url, fresh_fingerprint).await
    }
    async fn save_baseline(&self, baseline: &BaselineSnapshot) -> QuarryResult<()> {
        PostgresBaselineStore::save_baseline(self, baseline).await
    }
    async fn create_diff_record(&self, diff: &DiffRecord) -> QuarryResult<()> {
        PostgresBaselineStore::create_diff_record(self, diff).await
    }
    async fn load_baseline(
        &self,
        org_id: &str,
        baseline_id: &str,
    ) -> QuarryResult<Option<BaselineSnapshot>> {
        PostgresBaselineStore::load_baseline(self, org_id, baseline_id).await
    }
    async fn load_diff(&self, org_id: &str, diff_id: &str) -> QuarryResult<Option<DiffRecord>> {
        PostgresBaselineStore::load_diff(self, org_id, diff_id).await
    }
    async fn find_diff(
        &self,
        org_id: &str,
        from_baseline_id: &str,
        to_baseline_id: &str,
    ) -> QuarryResult<Option<DiffRecord>> {
        PostgresBaselineStore::find_diff(self, org_id, from_baseline_id, to_baseline_id).await
    }
    async fn load_history(
        &self,
        org_id: &str,
        source_url: &str,
        limit: u32,
    ) -> QuarryResult<Vec<BaselineSnapshot>> {
        PostgresBaselineStore::load_history(self, org_id, source_url, limit).await
    }
}

// ---- the diff ----------------------------------------------------------------

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// The paragraph diff of two pages of markdown, capped for the wire: at most
/// `TEXT_DIFF_MAX_OPS` ops (added and removed first, then moved, so a long run of moves cannot
/// hide a change), each text at most `TEXT_DIFF_MAX_OP_TEXT_BYTES`. The counts are complete
/// whatever was cut. `input_truncated` carries a cut already made on an input.
pub fn build_paragraph_diff(prev: &str, new: &str, input_truncated: bool) -> ParagraphDiff {
    let semantic = quarry_transform::diff::diff_paragraphs(
        &normalize_newlines(prev),
        &normalize_newlines(new),
        TEXT_DIFF_PARAGRAPH_CHARS,
    );
    let real_delta = semantic.is_real_delta();
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut moved = Vec::new();
    for op in semantic.ops {
        match op {
            quarry_transform::diff::DiffOp::Added { index, text, .. } => {
                added.push(TextDiffOp::Added { index, text });
            }
            quarry_transform::diff::DiffOp::Removed { index, text, .. } => {
                removed.push(TextDiffOp::Removed { index, text });
            }
            quarry_transform::diff::DiffOp::Moved { from, to, .. } => {
                moved.push(TextDiffOp::Moved { from, to });
            }
        }
    }
    let mut truncated = input_truncated;
    let mut ops: Vec<TextDiffOp> = added.into_iter().chain(removed).chain(moved).collect();
    if ops.len() > TEXT_DIFF_MAX_OPS {
        ops.truncate(TEXT_DIFF_MAX_OPS);
        truncated = true;
    }
    for op in &mut ops {
        if let TextDiffOp::Added { text, .. } | TextDiffOp::Removed { text, .. } = op {
            let (cut, was_cut) = cut_utf8(text, TEXT_DIFF_MAX_OP_TEXT_BYTES);
            if was_cut {
                *text = cut.to_string();
                truncated = true;
            }
        }
    }
    ParagraphDiff {
        format: PARAGRAPH_DIFF_FORMAT.to_string(),
        real_delta,
        added: semantic.added,
        removed: semantic.removed,
        moved: semantic.moved,
        unchanged: semantic.unchanged,
        ops,
        truncated,
    }
}

/// The compact summary a `DiffRecord` carries (at most 1 KB).
pub fn diff_summary(outcome: &DiffOutcome) -> String {
    let text = match (&outcome.diff, outcome.reason) {
        (Some(diff), _) if !diff.real_delta && diff.moved == 0 => {
            "no paragraph change (the page markup changed, its text did not)".to_string()
        }
        (Some(diff), _) if diff.moved > 0 => format!(
            "+{} -{} paragraphs, {} moved",
            diff.added, diff.removed, diff.moved
        ),
        (Some(diff), _) => format!("+{} -{} paragraphs", diff.added, diff.removed),
        (None, Some(reason)) => format!("text diff unavailable: {}", reason.as_str()),
        (None, None) => "text diff unavailable".to_string(),
    };
    cut_utf8(&text, DIFF_SUMMARY_MAX_BYTES).0.to_string()
}

/// A diff, or why there is none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffOutcome {
    pub available: bool,
    pub reason: Option<DiffUnavailable>,
    pub diff: Option<ParagraphDiff>,
}

impl DiffOutcome {
    fn unavailable(reason: DiffUnavailable) -> Self {
        Self {
            available: false,
            reason: Some(reason),
            diff: None,
        }
    }
}

/// One stored markdown artifact as text, cut at `TEXT_DIFF_MAX_MARKDOWN_BYTES`; the flag says
/// whether it was cut. `None` when the artifact cannot be read for this org (gone, evicted,
/// another org's).
pub async fn read_markdown(
    artifacts: &dyn ArtifactStore,
    org_id: &str,
    id: &ArtifactId,
) -> Option<(String, bool)> {
    let bytes = artifacts.get(org_id, id).await.ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let (cut, was_cut) = cut_utf8(&text, TEXT_DIFF_MAX_MARKDOWN_BYTES);
    Some((cut.to_string(), was_cut))
}

/// Checks a markdown artifact id handed in on a record request: it must parse, be readable for
/// `org_id` (another org's id reads as not found, so it fails here) and be a markdown artifact.
/// Fails closed with a 400; nothing is saved when it does.
pub async fn verify_markdown_artifact(
    artifacts: &dyn ArtifactStore,
    org_id: &str,
    raw: &str,
) -> QuarryResult<ArtifactId> {
    let id: ArtifactId = raw.trim().parse().map_err(|_| {
        QuarryError::new(
            ErrorCode::BadRequest,
            "markdown_artifact_id is not an artifact id",
        )
    })?;
    let (_, key) = artifacts.get_with_key(org_id, &id).await.map_err(|_| {
        QuarryError::new(
            ErrorCode::BadRequest,
            "markdown_artifact_id is not readable for this org",
        )
    })?;
    // A backend that does not report keys leaves it empty; every real one names the kind.
    if !key.is_empty() && !key.contains("markdown") {
        return Err(QuarryError::new(
            ErrorCode::BadRequest,
            "markdown_artifact_id is not a markdown artifact",
        ));
    }
    Ok(id)
}

/// Diffs two baselines' stored text, or says why it cannot.
pub async fn compute_outcome(
    artifacts: &dyn ArtifactStore,
    org_id: &str,
    from: &BaselineSnapshot,
    to: &BaselineSnapshot,
) -> DiffOutcome {
    let Some(to_id) = to.artifact_id.as_ref() else {
        return DiffOutcome::unavailable(DiffUnavailable::NoText);
    };
    let Some(from_id) = from.artifact_id.as_ref() else {
        return DiffOutcome::unavailable(DiffUnavailable::NoPreviousText);
    };
    let (Some((old, old_cut)), Some((new, new_cut))) = (
        read_markdown(artifacts, org_id, from_id).await,
        read_markdown(artifacts, org_id, to_id).await,
    ) else {
        return DiffOutcome::unavailable(DiffUnavailable::ArtifactGone);
    };
    DiffOutcome {
        available: true,
        reason: None,
        diff: Some(build_paragraph_diff(&old, &new, old_cut || new_cut)),
    }
}

/// Computes and stores the diff of a changed check: the JSON artifact (kind `change_diff`) and
/// the summary the `DiffRecord` carries. An unavailable diff is stored too, with its reason, so
/// the webhook's `diff_id` always resolves.
pub async fn store_text_diff(
    artifacts: &dyn ArtifactStore,
    org_id: &str,
    run: &RunKind,
    from: &BaselineSnapshot,
    to: &BaselineSnapshot,
) -> QuarryResult<(ArtifactHandle, String)> {
    let outcome = compute_outcome(artifacts, org_id, from, to).await;
    let summary = diff_summary(&outcome);
    let artifact = TextDiffArtifact {
        version: TEXT_DIFF_VERSION,
        source_url: to.source_url.clone(),
        from_baseline_id: from.baseline_id.clone(),
        to_baseline_id: to.baseline_id.clone(),
        from_fingerprint: from.fingerprint.clone(),
        to_fingerprint: to.fingerprint.clone(),
        available: outcome.available,
        reason: outcome.reason,
        diff: outcome.diff,
    };
    let bytes = serde_json::to_vec(&artifact)
        .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("encode text diff: {e}")))?;
    let handle = artifacts
        .put(org_id, run, &to.fingerprint, "change_diff", bytes)
        .await?;
    Ok((handle, summary))
}

// ---- GET /v1/change/diff -----------------------------------------------------

/// The query of `GET /v1/change/diff`, checked.
#[derive(Debug, Clone, Default)]
pub struct DiffParams {
    pub url: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub diff_id: Option<String>,
    pub include_markdown: bool,
}

fn clean(value: Option<&str>) -> Result<Option<String>, QuarryError> {
    let value = value.map(str::trim).filter(|v| !v.is_empty());
    if value.is_some_and(|v| v.len() > 128) {
        return Err(QuarryError::new(ErrorCode::BadRequest, "an id is too long"));
    }
    Ok(value.map(str::to_string))
}

impl DiffParams {
    pub fn parse(
        url: Option<&str>,
        from: Option<&str>,
        to: Option<&str>,
        diff_id: Option<&str>,
        include: Option<&str>,
    ) -> Result<Self, QuarryError> {
        let include_markdown = match include.map(str::trim).filter(|v| !v.is_empty()) {
            None => false,
            Some("markdown") => true,
            Some(_) => {
                return Err(QuarryError::new(
                    ErrorCode::BadRequest,
                    "include accepts only `markdown`",
                ))
            }
        };
        let url = url
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string);
        let params = Self {
            url,
            from: clean(from)?,
            to: clean(to)?,
            diff_id: clean(diff_id)?,
            include_markdown,
        };
        if params.diff_id.is_some() {
            if params.from.is_some() || params.to.is_some() {
                return Err(QuarryError::new(
                    ErrorCode::BadRequest,
                    "diff_id cannot be combined with from or to",
                ));
            }
        } else if params.url.is_none() {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "url (or diff_id) required",
            ));
        } else if params.from.is_some() != params.to.is_some() {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "from and to must be given together",
            ));
        }
        Ok(params)
    }
}

fn not_found(what: &str) -> QuarryError {
    QuarryError::new(ErrorCode::NotFound, format!("{what} not found"))
}

async fn must_load_baseline(
    store: &dyn ChangeStore,
    org_id: &str,
    id: &str,
) -> QuarryResult<BaselineSnapshot> {
    store
        .load_baseline(org_id, id)
        .await?
        .ok_or_else(|| not_found("baseline"))
}

/// The two baselines to compare and the stored diff record of the pair, if any.
async fn resolve(
    store: &dyn ChangeStore,
    org_id: &str,
    p: &DiffParams,
) -> QuarryResult<(
    Option<BaselineSnapshot>,
    BaselineSnapshot,
    Option<DiffRecord>,
)> {
    if let Some(diff_id) = p.diff_id.as_deref() {
        let record = store
            .load_diff(org_id, diff_id)
            .await?
            .ok_or_else(|| not_found("diff"))?;
        if p.url.as_deref().is_some_and(|url| url != record.source_url) {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "the diff belongs to a different url",
            ));
        }
        let to = must_load_baseline(store, org_id, &record.to_baseline_id).await?;
        let from = must_load_baseline(store, org_id, &record.from_baseline_id).await?;
        return Ok((Some(from), to, Some(record)));
    }
    let url = p.url.as_deref().unwrap_or_default();
    if let (Some(from_id), Some(to_id)) = (p.from.as_deref(), p.to.as_deref()) {
        let from = must_load_baseline(store, org_id, from_id).await?;
        let to = must_load_baseline(store, org_id, to_id).await?;
        if from.source_url != url || to.source_url != url {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "from and to must be baselines of the url",
            ));
        }
        if from.baseline_id == to.baseline_id {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "from and to are the same baseline",
            ));
        }
        let record = store
            .find_diff(org_id, &from.baseline_id, &to.baseline_id)
            .await?;
        return Ok((Some(from), to, record));
    }
    let mut history = store.load_history(org_id, url, 2).await?.into_iter();
    let to = history.next().ok_or_else(|| not_found("baseline"))?;
    let from = history.next();
    let record = match from.as_ref() {
        Some(from) => {
            store
                .find_diff(org_id, &from.baseline_id, &to.baseline_id)
                .await?
        }
        None => None,
    };
    Ok((from, to, record))
}

fn side(baseline: &BaselineSnapshot) -> DiffSide {
    DiffSide {
        baseline_id: baseline.baseline_id.clone(),
        fingerprint: baseline.fingerprint.clone(),
        captured_at: baseline.captured_at,
        artifact_id: baseline.artifact_id.as_ref().map(ToString::to_string),
    }
}

async fn markdown_of(
    artifacts: &dyn ArtifactStore,
    org_id: &str,
    baseline: Option<&BaselineSnapshot>,
) -> (Option<String>, bool) {
    let Some(id) = baseline.and_then(|b| b.artifact_id.as_ref()) else {
        return (None, false);
    };
    match read_markdown(artifacts, org_id, id).await {
        Some((text, cut)) => (Some(text), cut),
        None => (None, false),
    }
}

/// The two versions of a tracked URL and the diff between them. Org-scoped at the store and at
/// every artifact read; an id of another org answers exactly like an unknown id.
pub async fn diff_core(
    store: &dyn ChangeStore,
    artifacts: &dyn ArtifactStore,
    org_id: &str,
    p: &DiffParams,
) -> QuarryResult<ChangeDiffView> {
    let (from, to, record) = resolve(store, org_id, p).await?;

    let markdown = if p.include_markdown {
        let (old, old_cut) = markdown_of(artifacts, org_id, from.as_ref()).await;
        let (new, new_cut) = markdown_of(artifacts, org_id, Some(&to)).await;
        Some(DiffMarkdown {
            from: old,
            to: new,
            truncated: old_cut || new_cut,
        })
    } else {
        None
    };

    let Some(from) = from else {
        return Ok(ChangeDiffView {
            source_url: to.source_url.clone(),
            from: None,
            to: side(&to),
            diff_id: None,
            available: false,
            reason: Some(DiffUnavailable::NoPreviousVersion),
            diff: None,
            markdown,
        });
    };

    // The diff the check stored is the one its webhook announced; use it when it reads back.
    // A record from before the text diff (format `text`) or an artifact that is gone falls
    // back to diffing the two stored pages now.
    let mut stored: Option<(String, TextDiffArtifact)> = None;
    if let Some(record) = record.filter(|r| r.format == TEXT_DIFF_FORMAT) {
        if let Ok(bytes) = artifacts.get(org_id, &record.artifact_id).await {
            if let Ok(artifact) = serde_json::from_slice::<TextDiffArtifact>(&bytes) {
                if artifact.version == TEXT_DIFF_VERSION
                    && artifact.from_baseline_id == from.baseline_id
                    && artifact.to_baseline_id == to.baseline_id
                {
                    stored = Some((record.diff_id.clone(), artifact));
                }
            }
        }
    }
    let (diff_id, outcome) = match stored {
        Some((diff_id, artifact)) => (
            Some(diff_id),
            DiffOutcome {
                available: artifact.available,
                reason: artifact.reason,
                diff: artifact.diff,
            },
        ),
        None => (None, compute_outcome(artifacts, org_id, &from, &to).await),
    };
    Ok(ChangeDiffView {
        source_url: to.source_url.clone(),
        from: Some(side(&from)),
        to: side(&to),
        diff_id,
        available: outcome.available,
        reason: outcome.reason,
        diff: outcome.diff,
        markdown,
    })
}

#[cfg(test)]
pub(crate) mod testing {
    //! An in-memory [`ChangeStore`] with the store's org scoping, for tests that have no database.

    use std::sync::Mutex;

    use super::*;
    use quarry_core::change_history::ChangeStatus;

    #[derive(Default)]
    pub struct MemStore {
        pub baselines: Mutex<Vec<BaselineSnapshot>>,
        pub diffs: Mutex<Vec<DiffRecord>>,
    }

    #[async_trait]
    impl ChangeStore for MemStore {
        async fn compare_snapshot(
            &self,
            org_id: &str,
            source_url: &str,
            fresh_fingerprint: &str,
        ) -> QuarryResult<ChangeRecord> {
            let prev = self
                .load_history(org_id, source_url, 1)
                .await?
                .into_iter()
                .next();
            let status = match prev.as_ref() {
                None => ChangeStatus::New,
                Some(p) if p.fingerprint == fresh_fingerprint => ChangeStatus::Unchanged,
                Some(_) => ChangeStatus::Changed,
            };
            Ok(ChangeRecord {
                source_url: source_url.to_string(),
                org_id: org_id.to_string(),
                status,
                new_baseline: None,
                prev_baseline: prev,
                diff_id: None,
                checked_at: chrono::Utc::now(),
            })
        }
        async fn save_baseline(&self, baseline: &BaselineSnapshot) -> QuarryResult<()> {
            self.baselines.lock().unwrap().push(baseline.clone());
            Ok(())
        }
        async fn create_diff_record(&self, diff: &DiffRecord) -> QuarryResult<()> {
            self.diffs.lock().unwrap().push(diff.clone());
            Ok(())
        }
        async fn load_baseline(
            &self,
            org_id: &str,
            baseline_id: &str,
        ) -> QuarryResult<Option<BaselineSnapshot>> {
            Ok(self
                .baselines
                .lock()
                .unwrap()
                .iter()
                .find(|b| b.org_id == org_id && b.baseline_id == baseline_id)
                .cloned())
        }
        async fn load_diff(&self, org_id: &str, diff_id: &str) -> QuarryResult<Option<DiffRecord>> {
            Ok(self
                .diffs
                .lock()
                .unwrap()
                .iter()
                .find(|d| d.org_id == org_id && d.diff_id == diff_id)
                .cloned())
        }
        async fn find_diff(
            &self,
            org_id: &str,
            from_baseline_id: &str,
            to_baseline_id: &str,
        ) -> QuarryResult<Option<DiffRecord>> {
            Ok(self
                .diffs
                .lock()
                .unwrap()
                .iter()
                .rev()
                .find(|d| {
                    d.org_id == org_id
                        && d.from_baseline_id == from_baseline_id
                        && d.to_baseline_id == to_baseline_id
                })
                .cloned())
        }
        async fn load_history(
            &self,
            org_id: &str,
            source_url: &str,
            limit: u32,
        ) -> QuarryResult<Vec<BaselineSnapshot>> {
            Ok(self
                .baselines
                .lock()
                .unwrap()
                .iter()
                .rev()
                .filter(|b| b.org_id == org_id && b.source_url == source_url)
                .take(limit as usize)
                .cloned()
                .collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::MemStore;
    use super::*;
    use quarry_core::change_history::TEXT_DIFF_MAX_OPS;
    use quarry_runtime::artifact_store::InMemoryStore;

    const ORG: &str = "org_a";
    const URL: &str = "https://example.com/pricing";

    async fn put(artifacts: &InMemoryStore, org: &str, kind: &str, body: &str) -> ArtifactId {
        artifacts
            .put(
                org,
                &RunKind::new(),
                "page_hash",
                kind,
                body.as_bytes().to_vec(),
            )
            .await
            .expect("artifact stores")
            .artifact_id
    }

    fn baseline(
        org: &str,
        url: &str,
        fingerprint: &str,
        artifact: Option<ArtifactId>,
    ) -> BaselineSnapshot {
        BaselineSnapshot {
            baseline_id: format!("bln_{}", ArtifactId::new().ulid()),
            org_id: org.into(),
            source_url: url.into(),
            fingerprint: fingerprint.into(),
            artifact_id: artifact,
            prev_baseline_id: None,
            captured_at: chrono::Utc::now(),
            run_id: None,
        }
    }

    /// Two baselines of one url with the given markdown, saved in order, and their artifacts.
    async fn two_versions(
        store: &MemStore,
        artifacts: &InMemoryStore,
        old: &str,
        new: &str,
    ) -> (BaselineSnapshot, BaselineSnapshot) {
        let old_art = put(artifacts, ORG, "markdown", old).await;
        let new_art = put(artifacts, ORG, "markdown", new).await;
        let first = baseline(ORG, URL, "blake3:1", Some(old_art));
        let second = baseline(ORG, URL, "blake3:2", Some(new_art));
        store.save_baseline(&first).await.unwrap();
        store.save_baseline(&second).await.unwrap();
        (first, second)
    }

    fn params(
        url: Option<&str>,
        from: Option<&str>,
        to: Option<&str>,
        diff_id: Option<&str>,
        include: Option<&str>,
    ) -> DiffParams {
        DiffParams::parse(url, from, to, diff_id, include).expect("valid params")
    }

    // ---- the pure diff ----

    #[test]
    fn a_changed_paragraph_is_one_added_and_one_removed() {
        let d = build_paragraph_diff(
            "intro\n\nprice 10\n\noutro",
            "intro\n\nprice 12\n\noutro",
            false,
        );
        assert_eq!((d.added, d.removed, d.moved, d.unchanged), (1, 1, 0, 2));
        assert!(d.real_delta && !d.truncated);
        assert_eq!(d.format, "paragraph");
        assert!(d.ops.contains(&TextDiffOp::Added {
            index: 1,
            text: "price 12".into()
        }));
        assert!(d.ops.contains(&TextDiffOp::Removed {
            index: 1,
            text: "price 10".into()
        }));
    }

    #[test]
    fn markup_only_churn_is_not_a_real_delta() {
        // Same text, different line endings and a reflowed paragraph: the raw bytes changed, the text did not.
        let d = build_paragraph_diff("one\r\n\r\ntwo words", "one\n\ntwo\nwords", false);
        assert_eq!((d.added, d.removed, d.moved), (0, 0, 0));
        assert!(!d.real_delta && d.ops.is_empty());
    }

    #[test]
    fn ops_are_capped_and_the_counts_stay_complete() {
        let new: String = (0..TEXT_DIFF_MAX_OPS + 100)
            .map(|i| format!("added paragraph {i}"))
            .collect::<Vec<_>>()
            .join("\n\n");
        let d = build_paragraph_diff("kept", &format!("kept\n\n{new}"), false);
        assert_eq!(d.added, TEXT_DIFF_MAX_OPS + 100);
        assert_eq!(d.ops.len(), TEXT_DIFF_MAX_OPS);
        assert!(d.truncated);
    }

    #[test]
    fn a_long_run_of_moves_cannot_hide_a_removed_paragraph() {
        let old: Vec<String> = (0..(TEXT_DIFF_MAX_OPS + 50))
            .map(|i| format!("para {i}"))
            .collect();
        let mut new = old.clone();
        new.reverse();
        let mut old_with_extra = old.clone();
        old_with_extra.push("only in the old page".into());
        let d = build_paragraph_diff(&old_with_extra.join("\n\n"), &new.join("\n\n"), false);
        assert_eq!(d.removed, 1);
        assert!(
            d.moved > TEXT_DIFF_MAX_OPS,
            "the reversal is a long run of moves"
        );
        assert!(
            matches!(d.ops.first(), Some(TextDiffOp::Removed { text, .. }) if text == "only in the old page"),
            "added and removed ops come first: {:?}",
            d.ops.first()
        );
        assert_eq!(d.ops.len(), TEXT_DIFF_MAX_OPS);
        assert!(d.truncated);
    }

    #[test]
    fn an_op_text_is_cut_on_a_character_boundary() {
        let long = "\u{00e9}".repeat(3000); // 6000 bytes, one paragraph
        let d = build_paragraph_diff("x", &format!("x\n\n{long}"), false);
        let Some(TextDiffOp::Added { text, .. }) = d.ops.first() else {
            panic!("an added op: {:?}", d.ops);
        };
        assert!(text.len() <= TEXT_DIFF_MAX_OP_TEXT_BYTES && !text.is_empty());
        assert!(d.truncated);
    }

    #[test]
    fn the_summary_is_compact() {
        let diff = |added, removed, moved, real_delta| DiffOutcome {
            available: true,
            reason: None,
            diff: Some(ParagraphDiff {
                format: "paragraph".into(),
                real_delta,
                added,
                removed,
                moved,
                unchanged: 0,
                ops: Vec::new(),
                truncated: false,
            }),
        };
        assert_eq!(
            diff_summary(&diff(3, 1, 2, true)),
            "+3 -1 paragraphs, 2 moved"
        );
        assert_eq!(diff_summary(&diff(3, 1, 0, true)), "+3 -1 paragraphs");
        assert!(diff_summary(&diff(0, 0, 0, false)).starts_with("no paragraph change"));
        assert_eq!(
            diff_summary(&DiffOutcome::unavailable(DiffUnavailable::NoPreviousText)),
            "text diff unavailable: no_previous_text"
        );
    }

    #[test]
    fn the_flag_reads_the_usual_spellings_and_defaults_off() {
        assert!(!flag_is_on(None));
        assert!(!flag_is_on(Some("")));
        assert!(!flag_is_on(Some("0")));
        assert!(!flag_is_on(Some("off")));
        for on in ["1", "true", "TRUE", "yes", "on", " 1 "] {
            assert!(flag_is_on(Some(on)), "{on}");
        }
    }

    #[test]
    fn the_gate_names_the_variable_off_and_refuses_zdr() {
        let off = gate(false, false).unwrap_err();
        assert_eq!(off.code, ErrorCode::Unsupported);
        assert!(
            off.message.contains(ENV_CHANGE_TEXT_DIFF),
            "{}",
            off.message
        );
        // Off wins over ZDR: a flag-off edge answers the same 501 to everyone.
        assert_eq!(gate(false, true).unwrap_err().code, ErrorCode::Unsupported);
        let zdr = gate(true, true).unwrap_err();
        assert_eq!(zdr.code, ErrorCode::Forbidden);
        assert!(zdr.message.contains("Zero Data Retention"));
        assert!(gate(true, false).is_ok());
    }

    #[test]
    fn query_parameters_are_checked() {
        assert!(DiffParams::parse(Some("https://x/"), None, None, None, None).is_ok());
        assert!(
            DiffParams::parse(None, None, None, Some("diff_1"), Some("markdown"))
                .unwrap()
                .include_markdown
        );
        for (url, from, to, diff_id, include) in [
            (None, None, None, None, None), // nothing to look up
            (Some("https://x/"), Some("bln_1"), None, None, None), // from without to
            (Some("https://x/"), None, Some("bln_1"), None, None), // to without from
            (None, Some("bln_1"), Some("bln_2"), None, None), // from/to without a url
            (None, Some("bln_1"), None, Some("diff_1"), None), // diff_id with from
            (Some("https://x/"), None, None, None, Some("html")), // unknown include
        ] {
            let err = DiffParams::parse(url, from, to, diff_id, include).unwrap_err();
            assert_eq!(
                err.code,
                ErrorCode::BadRequest,
                "{url:?} {from:?} {to:?} {diff_id:?} {include:?}"
            );
        }
        let long = "x".repeat(200);
        assert_eq!(
            DiffParams::parse(None, None, None, Some(&long), None)
                .unwrap_err()
                .code,
            ErrorCode::BadRequest
        );
    }

    // ---- the read route ----

    #[tokio::test]
    async fn the_latest_two_versions_are_compared_by_default() {
        let (store, artifacts) = (MemStore::default(), InMemoryStore::new());
        let (first, second) = two_versions(
            &store,
            &artifacts,
            "intro\n\nold price",
            "intro\n\nnew price",
        )
        .await;

        let view = diff_core(
            &store,
            &artifacts,
            ORG,
            &params(Some(URL), None, None, None, None),
        )
        .await
        .unwrap();
        assert_eq!(view.source_url, URL);
        assert_eq!(view.from.as_ref().unwrap().baseline_id, first.baseline_id);
        assert_eq!(view.to.baseline_id, second.baseline_id);
        assert!(view.available && view.reason.is_none());
        assert!(
            view.diff_id.is_none(),
            "no diff was stored for the pair: it was computed now"
        );
        let diff = view.diff.expect("a diff");
        assert_eq!((diff.added, diff.removed, diff.moved), (1, 1, 0));
        assert!(diff.real_delta);
        assert!(view.markdown.is_none(), "text only with include=markdown");
        assert_eq!(
            view.to.artifact_id,
            second.artifact_id.map(|a| a.to_string())
        );
    }

    #[tokio::test]
    async fn two_named_baselines_are_compared() {
        let (store, artifacts) = (MemStore::default(), InMemoryStore::new());
        let a = put(&artifacts, ORG, "markdown", "one").await;
        let b = put(&artifacts, ORG, "markdown", "one\n\ntwo").await;
        let c = put(&artifacts, ORG, "markdown", "one\n\ntwo\n\nthree").await;
        let (ba, bb, bc) = (
            baseline(ORG, URL, "blake3:a", Some(a)),
            baseline(ORG, URL, "blake3:b", Some(b)),
            baseline(ORG, URL, "blake3:c", Some(c)),
        );
        for x in [&ba, &bb, &bc] {
            store.save_baseline(x).await.unwrap();
        }
        let view = diff_core(
            &store,
            &artifacts,
            ORG,
            &params(
                Some(URL),
                Some(&ba.baseline_id),
                Some(&bc.baseline_id),
                None,
                None,
            ),
        )
        .await
        .unwrap();
        assert_eq!(view.from.unwrap().baseline_id, ba.baseline_id);
        assert_eq!(view.to.baseline_id, bc.baseline_id);
        assert_eq!(view.diff.unwrap().added, 2);
    }

    #[tokio::test]
    async fn the_stored_diff_is_what_the_diff_id_reads_back() {
        let (store, artifacts) = (MemStore::default(), InMemoryStore::new());
        let (first, second) = two_versions(&store, &artifacts, "old", "new").await;
        // A stored artifact that differs from what the pages would diff to proves which one is read.
        let mut stored = TextDiffArtifact {
            version: TEXT_DIFF_VERSION,
            source_url: URL.into(),
            from_baseline_id: first.baseline_id.clone(),
            to_baseline_id: second.baseline_id.clone(),
            from_fingerprint: first.fingerprint.clone(),
            to_fingerprint: second.fingerprint.clone(),
            available: true,
            reason: None,
            diff: Some(build_paragraph_diff("a", "b", false)),
        };
        stored.diff.as_mut().unwrap().added = 99;
        let art = put(
            &artifacts,
            ORG,
            "change_diff",
            &serde_json::to_string(&stored).unwrap(),
        )
        .await;
        let record = DiffRecord {
            diff_id: "diff_stored".into(),
            org_id: ORG.into(),
            from_baseline_id: first.baseline_id.clone(),
            to_baseline_id: second.baseline_id.clone(),
            source_url: URL.into(),
            format: "json".into(),
            artifact_id: art,
            summary: None,
            created_at: chrono::Utc::now(),
        };
        store.create_diff_record(&record).await.unwrap();

        for p in [
            params(None, None, None, Some("diff_stored"), None),
            params(Some(URL), None, None, Some("diff_stored"), None),
            params(Some(URL), None, None, None, None),
            params(
                Some(URL),
                Some(&first.baseline_id),
                Some(&second.baseline_id),
                None,
                None,
            ),
        ] {
            let view = diff_core(&store, &artifacts, ORG, &p).await.unwrap();
            assert_eq!(view.diff_id.as_deref(), Some("diff_stored"), "{p:?}");
            assert_eq!(view.diff.unwrap().added, 99, "{p:?}");
        }
    }

    #[tokio::test]
    async fn a_record_from_before_the_text_diff_falls_back_to_the_stored_pages() {
        let (store, artifacts) = (MemStore::default(), InMemoryStore::new());
        let (first, second) = two_versions(&store, &artifacts, "old", "new").await;
        let note = put(
            &artifacts,
            ORG,
            "raw",
            "content fingerprint changed: a -> b",
        )
        .await;
        store
            .create_diff_record(&DiffRecord {
                diff_id: "diff_legacy".into(),
                org_id: ORG.into(),
                from_baseline_id: first.baseline_id.clone(),
                to_baseline_id: second.baseline_id.clone(),
                source_url: URL.into(),
                format: "text".into(),
                artifact_id: note,
                summary: None,
                created_at: chrono::Utc::now(),
            })
            .await
            .unwrap();
        let view = diff_core(
            &store,
            &artifacts,
            ORG,
            &params(None, None, None, Some("diff_legacy"), None),
        )
        .await
        .unwrap();
        assert!(
            view.diff_id.is_none(),
            "the one-line note is not a text diff"
        );
        assert_eq!(view.diff.unwrap().added, 1);
    }

    #[tokio::test]
    async fn another_orgs_ids_read_exactly_like_unknown_ones() {
        let (store, artifacts) = (MemStore::default(), InMemoryStore::new());
        let (first, second) = two_versions(&store, &artifacts, "old", "new").await;
        let record = DiffRecord {
            diff_id: "diff_a".into(),
            org_id: ORG.into(),
            from_baseline_id: first.baseline_id.clone(),
            to_baseline_id: second.baseline_id.clone(),
            source_url: URL.into(),
            format: "json".into(),
            artifact_id: ArtifactId::new(),
            summary: None,
            created_at: chrono::Utc::now(),
        };
        store.create_diff_record(&record).await.unwrap();

        let other = "org_b";
        let probes = [
            params(Some(URL), None, None, None, None),
            params(
                Some(URL),
                Some(&first.baseline_id),
                Some(&second.baseline_id),
                None,
                None,
            ),
            params(None, None, None, Some("diff_a"), None),
        ];
        for p in probes {
            let foreign = diff_core(&store, &artifacts, other, &p).await.unwrap_err();
            assert_eq!(foreign.code, ErrorCode::NotFound, "{p:?}");
        }
        let unknown = diff_core(
            &store,
            &artifacts,
            ORG,
            &params(None, None, None, Some("diff_nope"), None),
        )
        .await
        .unwrap_err();
        let foreign = diff_core(
            &store,
            &artifacts,
            other,
            &params(None, None, None, Some("diff_a"), None),
        )
        .await
        .unwrap_err();
        assert_eq!(foreign.code, unknown.code);
        assert_eq!(
            foreign.message, unknown.message,
            "no existence oracle in the message"
        );
    }

    #[tokio::test]
    async fn baselines_of_different_urls_are_a_bad_request() {
        let (store, artifacts) = (MemStore::default(), InMemoryStore::new());
        let (first, second) = two_versions(&store, &artifacts, "old", "new").await;
        let other_url = baseline(ORG, "https://example.com/other", "blake3:z", None);
        store.save_baseline(&other_url).await.unwrap();
        let err = diff_core(
            &store,
            &artifacts,
            ORG,
            &params(
                Some(URL),
                Some(&first.baseline_id),
                Some(&other_url.baseline_id),
                None,
                None,
            ),
        )
        .await
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
        let same = diff_core(
            &store,
            &artifacts,
            ORG,
            &params(
                Some(URL),
                Some(&first.baseline_id),
                Some(&first.baseline_id),
                None,
                None,
            ),
        )
        .await
        .unwrap_err();
        assert_eq!(same.code, ErrorCode::BadRequest);
        // A diff_id read with a url that is not the diff's url is a bad request too.
        store
            .create_diff_record(&DiffRecord {
                diff_id: "diff_1".into(),
                org_id: ORG.into(),
                from_baseline_id: first.baseline_id.clone(),
                to_baseline_id: second.baseline_id.clone(),
                source_url: URL.into(),
                format: "json".into(),
                artifact_id: ArtifactId::new(),
                summary: None,
                created_at: chrono::Utc::now(),
            })
            .await
            .unwrap();
        let mismatch = diff_core(
            &store,
            &artifacts,
            ORG,
            &params(
                Some("https://example.com/other"),
                None,
                None,
                Some("diff_1"),
                None,
            ),
        )
        .await
        .unwrap_err();
        assert_eq!(mismatch.code, ErrorCode::BadRequest);
    }

    #[tokio::test]
    async fn an_unknown_url_is_not_found() {
        let (store, artifacts) = (MemStore::default(), InMemoryStore::new());
        let err = diff_core(
            &store,
            &artifacts,
            ORG,
            &params(Some(URL), None, None, None, None),
        )
        .await
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
    }

    #[tokio::test]
    async fn one_baseline_has_no_previous_version() {
        let (store, artifacts) = (MemStore::default(), InMemoryStore::new());
        let art = put(&artifacts, ORG, "markdown", "only page").await;
        store
            .save_baseline(&baseline(ORG, URL, "blake3:1", Some(art)))
            .await
            .unwrap();
        let view = diff_core(
            &store,
            &artifacts,
            ORG,
            &params(Some(URL), None, None, None, Some("markdown")),
        )
        .await
        .unwrap();
        assert!(view.from.is_none() && !view.available && view.diff.is_none());
        assert_eq!(view.reason, Some(DiffUnavailable::NoPreviousVersion));
        let markdown = view.markdown.expect("include=markdown");
        assert_eq!(markdown.from, None);
        assert_eq!(markdown.to.as_deref(), Some("only page"));
    }

    #[tokio::test]
    async fn a_missing_or_gone_text_says_which_side_and_why() {
        let (store, artifacts) = (MemStore::default(), InMemoryStore::new());
        let live = put(&artifacts, ORG, "markdown", "page").await;
        let gone = ArtifactId::new(); // an id nothing was stored under
        let cases = [
            // (older artifact, newer artifact, reason)
            (None, Some(live.clone()), DiffUnavailable::NoPreviousText),
            (Some(live.clone()), None, DiffUnavailable::NoText),
            (None, None, DiffUnavailable::NoText),
            (
                Some(gone.clone()),
                Some(live.clone()),
                DiffUnavailable::ArtifactGone,
            ),
            (
                Some(live.clone()),
                Some(gone),
                DiffUnavailable::ArtifactGone,
            ),
        ];
        for (n, (old, new, reason)) in cases.into_iter().enumerate() {
            let url = format!("https://example.com/case-{n}");
            store
                .save_baseline(&baseline(ORG, &url, "blake3:1", old))
                .await
                .unwrap();
            store
                .save_baseline(&baseline(ORG, &url, "blake3:2", new))
                .await
                .unwrap();
            let view = diff_core(
                &store,
                &artifacts,
                ORG,
                &params(Some(&url), None, None, None, None),
            )
            .await
            .unwrap();
            assert!(!view.available && view.diff.is_none(), "case {n}");
            assert_eq!(view.reason, Some(reason), "case {n}");
            assert!(
                view.from.is_some(),
                "the metadata of both sides is still returned"
            );
        }
    }

    #[tokio::test]
    async fn another_orgs_artifact_on_a_baseline_reads_as_gone() {
        let (store, artifacts) = (MemStore::default(), InMemoryStore::new());
        let foreign = put(&artifacts, "org_b", "markdown", "someone else's page").await;
        let own = put(&artifacts, ORG, "markdown", "my page").await;
        store
            .save_baseline(&baseline(ORG, URL, "blake3:1", Some(foreign)))
            .await
            .unwrap();
        store
            .save_baseline(&baseline(ORG, URL, "blake3:2", Some(own)))
            .await
            .unwrap();
        let view = diff_core(
            &store,
            &artifacts,
            ORG,
            &params(Some(URL), None, None, None, Some("markdown")),
        )
        .await
        .unwrap();
        assert_eq!(view.reason, Some(DiffUnavailable::ArtifactGone));
        let markdown = view.markdown.unwrap();
        assert_eq!(markdown.from, None, "the foreign text is never returned");
        assert_eq!(markdown.to.as_deref(), Some("my page"));
    }

    #[tokio::test]
    async fn include_markdown_returns_both_pages_and_caps_each() {
        let (store, artifacts) = (MemStore::default(), InMemoryStore::new());
        let huge = "a".repeat(TEXT_DIFF_MAX_MARKDOWN_BYTES + 10);
        two_versions(&store, &artifacts, "small page", &huge).await;
        let view = diff_core(
            &store,
            &artifacts,
            ORG,
            &params(Some(URL), None, None, None, Some("markdown")),
        )
        .await
        .unwrap();
        let markdown = view.markdown.expect("markdown");
        assert_eq!(markdown.from.as_deref(), Some("small page"));
        assert_eq!(markdown.to.unwrap().len(), TEXT_DIFF_MAX_MARKDOWN_BYTES);
        assert!(markdown.truncated);
        assert!(
            view.diff.expect("still diffed").truncated,
            "a cut input marks the diff"
        );
    }

    // ---- recording ----

    #[tokio::test]
    async fn verify_markdown_artifact_is_org_bound_and_kind_checked() {
        let artifacts = InMemoryStore::new();
        let md = put(&artifacts, ORG, "markdown", "page").await;
        let html = put(&artifacts, ORG, "html", "<p>page</p>").await;
        assert_eq!(
            verify_markdown_artifact(&artifacts, ORG, &md.to_string())
                .await
                .unwrap(),
            md
        );
        for (org, raw) in [
            ("org_b", md.to_string()),            // another org's artifact
            (ORG, html.to_string()),              // not markdown
            (ORG, "not-an-id".to_string()),       // not an id
            (ORG, ArtifactId::new().to_string()), // nothing stored
        ] {
            let err = verify_markdown_artifact(&artifacts, org, &raw)
                .await
                .unwrap_err();
            assert_eq!(err.code, ErrorCode::BadRequest, "{org} {raw}");
        }
    }

    #[tokio::test]
    async fn store_text_diff_writes_the_json_artifact_and_a_summary() {
        let (store, artifacts) = (MemStore::default(), InMemoryStore::new());
        let (first, second) =
            two_versions(&store, &artifacts, "keep\n\nold", "keep\n\nnew\n\nmore").await;
        let (handle, summary) = store_text_diff(&artifacts, ORG, &RunKind::new(), &first, &second)
            .await
            .unwrap();
        assert_eq!(summary, "+2 -1 paragraphs");
        let bytes = artifacts.get(ORG, &handle.artifact_id).await.unwrap();
        let stored: TextDiffArtifact = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(stored.version, TEXT_DIFF_VERSION);
        assert_eq!(stored.from_baseline_id, first.baseline_id);
        assert_eq!(stored.to_baseline_id, second.baseline_id);
        assert!(stored.available);
        let diff = stored.diff.unwrap();
        assert_eq!((diff.added, diff.removed), (2, 1));
        // Another org cannot read it back.
        assert!(artifacts.get("org_b", &handle.artifact_id).await.is_err());
    }
}
