//! The gateway's shadow writer into session-core's artifact revision store (Wave 3 EP-3 S2).
//!
//! The gateway counts artifact versions in process memory (`artifacts::ArtifactVersionStore`)
//! and publishes each turn's last version of every artifact into the assistant message's
//! `metadata.artifacts`. With `MODEL_GATEWAY_ARTIFACT_REVISIONS` at `shadow` (or `on`), the same
//! revisions are also appended to `ArtifactRevisionService` once the assistant message is
//! durable, each as a compare-and-set against the version this replica last published. Nothing
//! the user sees changes: the metadata is written as before, the write runs on its own task after
//! the turn's save, and a refusal is a counter and a log line, never a turn error. A
//! `revision_mismatch` is the number shadow mode exists to measure: how often the process-local
//! commit lock let two replicas move one artifact.
//!
//! Owner decisions taken by their recommendation (EP-3 section 3): the store is thread-owned in
//! session-core (OD-1, OD-2) and the metadata stays unchanged; the head is thread-wide and each
//! revision names its run, from which session-core resolves the assistant message (OD-8); the
//! caps are session-core's (OD-10). Nothing is written for a ZDR turn (it has no artifacts, and
//! the RPC refuses the credential) or for a waived turn (OD-11, DEC-32): metadata keeps only what
//! Ask already persists. The write carries the user's session bearer, so session-core checks the
//! thread's owner and attributes the revision to the user's turn. Content is never logged.
//!
//! `on` is S3's third state. The read routes answer (`artifact_revision_routes`), a turn's start
//! raises the process-local counters to the table's heads ([`seed_from_table_heads`]), and the
//! append moves from after the save to the commit lock ([`commit_turn_revisions`]): the table's
//! compare-and-set then decides a race between replicas, and a moved head ends the turn with the
//! same `artifact_revision_conflict` a race inside one replica gets. Each published frame names
//! its editor (`model`).

use mp_contracts::model_plane::v1::{
    AppendArtifactRevisionRequest, AppendOutcome, ListArtifactRevisionsRequest,
};
use sha2::{Digest as _, Sha256};

use crate::artifacts::ArtifactVersionStore;
use crate::auth::VerifiedSessionBearer;
use crate::sse_events::RecordedArtifact;
use crate::state::AppState;

/// The writer switch: `off`, `shadow` or `on`. Unset (or empty, as compose writes an unset
/// variable) is `off`; any other value fails startup, so a typo cannot silently write nothing.
pub const MODE_ENV: &str = "MODEL_GATEWAY_ARTIFACT_REVISIONS";

/// The refusal session-core answers when the head moved (`aborted`).
const REVISION_MISMATCH: &str = "revision_mismatch";

/// How the gateway uses the revision store.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ArtifactRevisionsMode {
    /// No call to the store. The default, and the behaviour before EP-3.
    #[default]
    Off,
    /// Every published model revision is also appended; a refusal is only counted.
    Shadow,
    /// As `Shadow` for this writer; S3 adds the read routes under this state.
    On,
}

impl ArtifactRevisionsMode {
    /// The strict parse: only the three literals, lowercase and untrimmed.
    ///
    /// # Errors
    ///
    /// Any value other than unset, empty, `off`, `shadow` or `on`.
    pub fn parse(raw: Option<&str>) -> Result<Self, String> {
        match raw {
            None | Some("" | "off") => Ok(Self::Off),
            Some("shadow") => Ok(Self::Shadow),
            Some("on") => Ok(Self::On),
            Some(_) => Err(format!("{MODE_ENV} must be off, shadow or on")),
        }
    }

    /// Read [`MODE_ENV`] once at startup.
    ///
    /// # Errors
    ///
    /// The value is not one of the three literals.
    pub fn from_env() -> anyhow::Result<Self> {
        Self::parse(std::env::var(MODE_ENV).ok().as_deref()).map_err(anyhow::Error::msg)
    }

    /// Whether model revisions are written.
    #[must_use]
    pub const fn writes(self) -> bool {
        !matches!(self, Self::Off)
    }

    /// Whether the turn's revisions are written best effort after the assistant message is
    /// durable (`shadow`). In `on` they are written earlier, under the commit lock.
    #[must_use]
    pub const fn writes_after_save(self) -> bool {
        matches!(self, Self::Shadow)
    }

    /// Whether the table is the compare-and-set at the commit lock, the turn starts from its
    /// heads and the published frames name their editor (`on`).
    #[must_use]
    pub const fn table_is_authority(self) -> bool {
        matches!(self, Self::On)
    }
}

/// The editor every frame a model turn publishes names in `on` mode.
pub const MODEL_EDITOR: &str = "model";

/// Most heads one turn start reads. Session Core reads more than 50 as 50; a thread with more
/// artifacts keeps the rest from its message metadata, as before.
const MAX_SEEDED_HEADS: u32 = 50;

/// Whether a turn of this posture writes revisions: the mode writes, and the turn is neither
/// ZDR nor waived.
#[must_use]
pub const fn permits(mode: ArtifactRevisionsMode, zdr: bool, waived: bool) -> bool {
    mode.writes() && !zdr && !waived
}

/// The editor a published frame names: `model` in `on` for a turn whose revisions are stored,
/// otherwise none, so the frame stays byte-identical to before.
#[must_use]
pub const fn frame_editor(
    mode: ArtifactRevisionsMode,
    zdr: bool,
    waived: bool,
) -> Option<&'static str> {
    if mode.table_is_authority() && permits(mode, zdr, waived) {
        Some(MODEL_EDITOR)
    } else {
        None
    }
}

/// One revision to append: what the turn published, and the head it moved from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedRevision {
    pub artifact_id: String,
    pub kind: String,
    pub title: String,
    pub content: String,
    /// The version the turn published (and wrote into the message metadata).
    pub version: u32,
    /// The version this replica had committed before the turn: the compare-and-set.
    pub expected_version: u32,
}

/// The run a turn's revisions belong to.
#[derive(Debug, Clone)]
pub struct RevisionTarget {
    pub org_id: String,
    pub thread_id: String,
    pub run_id: String,
}

/// Pair each published artifact with the head this replica committed before the turn. Must be
/// called before the turn's versions are seeded into `committed`, or every head reads as moved.
#[must_use]
pub fn plan_turn_revisions(
    committed: &ArtifactVersionStore,
    thread_id: &str,
    artifacts: &[RecordedArtifact],
) -> Vec<PlannedRevision> {
    artifacts
        .iter()
        .map(|artifact| PlannedRevision {
            artifact_id: artifact.id.clone(),
            kind: artifact.kind.clone(),
            title: artifact.title.clone(),
            content: artifact.content.clone(),
            version: artifact.version,
            expected_version: committed
                .current_version(thread_id, &artifact.id)
                .unwrap_or(0),
        })
        .collect()
}

/// Write one turn's model revisions, best effort, after the assistant message is durable. Reads
/// the heads from `committed` first, so the caller must call it before seeding the turn's
/// versions. Returns whether a write was started: nothing is planned or sent with the mode off,
/// for a ZDR or waived turn, or when the turn published no artifact.
pub fn record_turn_revisions(
    state: &AppState,
    bearer: &VerifiedSessionBearer,
    target: RevisionTarget,
    committed: &ArtifactVersionStore,
    artifacts: &[RecordedArtifact],
    zdr: bool,
    waived: bool,
) -> bool {
    if !state.artifact_revisions_mode.writes_after_save()
        || !permits(state.artifact_revisions_mode, zdr, waived)
        || artifacts.is_empty()
    {
        return false;
    }
    let planned = plan_turn_revisions(committed, &target.thread_id, artifacts);
    let state = state.clone();
    let bearer = bearer.clone();
    tokio::spawn(async move {
        write_turn(&state, &bearer, &target, planned).await;
    });
    true
}

/// What one turn's writes came to. Counted, never returned to the turn.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct WriteTally {
    pub appended: usize,
    pub duplicate: usize,
    pub conflict: usize,
    pub failed: usize,
}

/// Append the planned revisions in order, one call each. A failure of one does not stop the
/// next: each artifact has its own head.
pub async fn write_turn(
    state: &AppState,
    bearer: &VerifiedSessionBearer,
    target: &RevisionTarget,
    planned: Vec<PlannedRevision>,
) -> WriteTally {
    write_planned(state, bearer, target, planned, false).await
}

/// The table refused a moved head under the commit lock: another replica (or another run this
/// replica no longer knows of) published a revision of the same artifact first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevisionConflict;

/// `on` mode: append the turn's model revisions under the commit lock, before the assistant
/// message is saved, so the table's compare-and-set decides between replicas. The caller ends
/// the turn with `artifact_revision_conflict` on `Err`, as it does for a race inside one replica.
/// Stops at the first conflict. Any other refusal is counted and the turn goes on: the message
/// metadata stays the record the panel and the Space aggregate read, so a store outage must not
/// fail a turn. Nothing is sent outside `on`, for a ZDR or waived turn, or without artifacts.
///
/// # Errors
///
/// [`RevisionConflict`] when session-core answers `revision_mismatch`.
pub async fn commit_turn_revisions(
    state: &AppState,
    bearer: &VerifiedSessionBearer,
    target: &RevisionTarget,
    committed: &ArtifactVersionStore,
    artifacts: &[RecordedArtifact],
    zdr: bool,
    waived: bool,
) -> Result<(), RevisionConflict> {
    if !state.artifact_revisions_mode.table_is_authority()
        || !permits(state.artifact_revisions_mode, zdr, waived)
        || artifacts.is_empty()
    {
        return Ok(());
    }
    let planned = plan_turn_revisions(committed, &target.thread_id, artifacts);
    let tally = write_planned(state, bearer, target, planned, true).await;
    if tally.conflict > 0 {
        return Err(RevisionConflict);
    }
    Ok(())
}

/// `on` mode, at a turn's start: raise this replica's counters (and text) to the table's heads,
/// so a revision another replica committed is the base this turn revises. Seeding never lowers
/// a version, so the message metadata the turn also rehydrates from cannot undo it. Best effort:
/// on any refusal the turn starts from the metadata alone, as before. Returns how many heads
/// were seeded.
pub async fn seed_from_table_heads(
    state: &AppState,
    bearer: &VerifiedSessionBearer,
    org_id: &str,
    thread_id: &str,
    zdr: bool,
    waived: bool,
) -> usize {
    if !state.artifact_revisions_mode.table_is_authority() || zdr || waived {
        return 0;
    }
    let request = ListArtifactRevisionsRequest {
        org_id: org_id.to_owned(),
        thread_id: thread_id.to_owned(),
        artifact_id: String::new(),
        // The text is the base an update_artifact revises; a head without it would let the
        // model rewrite an older copy under a newer number.
        include_content: true,
        limit: MAX_SEEDED_HEADS,
        before_version: 0,
    };
    let Ok(request) = crate::http_routes::authenticated_session_request(request, bearer) else {
        return 0;
    };
    let heads = match state
        .artifact_revision_client
        .clone()
        .list_artifact_revisions(request)
        .await
    {
        Ok(response) => response.into_inner().revisions,
        Err(status) => {
            tracing::debug!(code = ?status.code(), "artifact revision heads not read");
            metrics::counter!("mp_gateway_artifact_revision_seed_total", "outcome" => "failed")
                .increment(1);
            return 0;
        }
    };
    for head in &heads {
        state.artifact_versions.seed_persisted(
            thread_id,
            &head.artifact_id,
            &head.title,
            &head.content,
            head.version,
        );
        if let Some(kind) = crate::artifacts::ArtifactKind::parse(&head.kind) {
            state
                .artifact_versions
                .remember_kind(thread_id, &head.artifact_id, kind);
        }
    }
    metrics::counter!("mp_gateway_artifact_revision_seed_total", "outcome" => "seeded")
        .increment(1);
    heads.len()
}

async fn write_planned(
    state: &AppState,
    bearer: &VerifiedSessionBearer,
    target: &RevisionTarget,
    planned: Vec<PlannedRevision>,
    stop_on_conflict: bool,
) -> WriteTally {
    let mut tally = WriteTally::default();
    for revision in planned {
        let outcome = match append(state, bearer, target, revision).await {
            Ok(AppendOutcome::Duplicate) => {
                tally.duplicate += 1;
                "duplicate"
            }
            Ok(_) => {
                tally.appended += 1;
                "appended"
            }
            Err(status) if is_conflict(&status) => {
                tally.conflict += 1;
                // Ids only: the cross-replica comparison is what this line is for.
                tracing::info!(
                    thread_id = %target.thread_id,
                    run_id = %target.run_id,
                    authoritative = stop_on_conflict,
                    "artifact revision conflict"
                );
                "conflict"
            }
            Err(status) => {
                tally.failed += 1;
                // The code only: a status message is not repeated into the log.
                tracing::debug!(code = ?status.code(), "artifact revision not stored");
                "failed"
            }
        };
        metrics::counter!("mp_gateway_artifact_revision_writes_total", "outcome" => outcome)
            .increment(1);
        if stop_on_conflict && tally.conflict > 0 {
            break;
        }
    }
    tally
}

/// Whether session-core refused the append because the head moved.
fn is_conflict(status: &tonic::Status) -> bool {
    status.code() == tonic::Code::Aborted && status.message() == REVISION_MISMATCH
}

/// A retry of the same published revision returns the first row: the key names the run, the
/// artifact and the version, never the content.
fn idempotency_key(run_id: &str, artifact_id: &str, version: u32) -> String {
    let digest = Sha256::digest(format!("{run_id}\n{artifact_id}\n{version}").as_bytes());
    format!("model-{digest:x}")
}

/// The append for one published model revision. The source message is left to session-core,
/// which resolves it from the run's completed answer.
fn append_request(
    target: &RevisionTarget,
    revision: PlannedRevision,
) -> AppendArtifactRevisionRequest {
    AppendArtifactRevisionRequest {
        org_id: target.org_id.clone(),
        thread_id: target.thread_id.clone(),
        idempotency_key: idempotency_key(&target.run_id, &revision.artifact_id, revision.version),
        artifact_id: revision.artifact_id,
        expected_version: revision.expected_version,
        kind: revision.kind,
        title: revision.title,
        content: revision.content,
        editor_kind: "model".to_owned(),
        run_id: target.run_id.clone(),
        source_message_id: String::new(),
        restored_from_version: 0,
        version: revision.version,
    }
}

async fn append(
    state: &AppState,
    bearer: &VerifiedSessionBearer,
    target: &RevisionTarget,
    revision: PlannedRevision,
) -> Result<AppendOutcome, tonic::Status> {
    let request =
        crate::http_routes::authenticated_session_request(append_request(target, revision), bearer)
            .map_err(|_| tonic::Status::internal("the session credential is not forwardable"))?;
    let response = state
        .artifact_revision_client
        .clone()
        .append_artifact_revision(request)
        .await?
        .into_inner();
    Ok(AppendOutcome::try_from(response.outcome).unwrap_or(AppendOutcome::Unspecified))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mp_contracts::model_plane::v1::{
        artifact_revision_service_client::ArtifactRevisionServiceClient,
        artifact_revision_service_server::{
            ArtifactRevisionService, ArtifactRevisionServiceServer,
        },
        AppendArtifactRevisionResponse, ArtifactRevision, GetArtifactRevisionRequest,
        GetArtifactRevisionResponse, ListArtifactRevisionsResponse,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    /// A revision store double: counts appends, keeps the requests, and answers each with a
    /// conflict or an append. With `cas` it keeps one head per artifact and refuses a stale
    /// expected version as session-core does; `heads` is what a heads listing returns.
    #[derive(Clone, Default)]
    struct FakeStore {
        calls: Arc<AtomicUsize>,
        requests: Arc<Mutex<Vec<AppendArtifactRevisionRequest>>>,
        conflict: bool,
        cas: bool,
        table: Arc<Mutex<std::collections::HashMap<String, u32>>>,
        heads: Vec<ArtifactRevision>,
        lists: Arc<Mutex<Vec<ListArtifactRevisionsRequest>>>,
    }

    #[tonic::async_trait]
    impl ArtifactRevisionService for FakeStore {
        async fn append_artifact_revision(
            &self,
            request: tonic::Request<AppendArtifactRevisionRequest>,
        ) -> Result<tonic::Response<AppendArtifactRevisionResponse>, tonic::Status> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let request = request.into_inner();
            self.requests.lock().unwrap().push(request.clone());
            if self.conflict {
                return Err(tonic::Status::aborted(REVISION_MISMATCH));
            }
            if self.cas {
                let mut table = self.table.lock().unwrap();
                let head = table.entry(request.artifact_id.clone()).or_insert(0);
                if *head != request.expected_version {
                    return Err(tonic::Status::aborted(REVISION_MISMATCH));
                }
                *head = request.version;
            }
            Ok(tonic::Response::new(AppendArtifactRevisionResponse {
                revision: None,
                outcome: AppendOutcome::Appended as i32,
            }))
        }

        async fn list_artifact_revisions(
            &self,
            request: tonic::Request<ListArtifactRevisionsRequest>,
        ) -> Result<tonic::Response<ListArtifactRevisionsResponse>, tonic::Status> {
            self.lists.lock().unwrap().push(request.into_inner());
            Ok(tonic::Response::new(ListArtifactRevisionsResponse {
                revisions: self.heads.clone(),
                has_more: false,
            }))
        }

        async fn get_artifact_revision(
            &self,
            _request: tonic::Request<GetArtifactRevisionRequest>,
        ) -> Result<tonic::Response<GetArtifactRevisionResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("not used"))
        }
    }

    async fn state_with(fake: FakeStore, mode: ArtifactRevisionsMode) -> AppState {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = tonic::transport::Server::builder()
                .add_service(ArtifactRevisionServiceServer::new(fake))
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await;
        });
        let mut state = AppState::new();
        state.artifact_revision_client =
            ArtifactRevisionServiceClient::connect(format!("http://{address}"))
                .await
                .unwrap();
        state.artifact_revisions_mode = mode;
        state
    }

    fn bearer() -> VerifiedSessionBearer {
        VerifiedSessionBearer::for_test("session-token")
    }

    fn target() -> RevisionTarget {
        RevisionTarget {
            org_id: "org-a".to_owned(),
            thread_id: "thread-a".to_owned(),
            run_id: "run-a".to_owned(),
        }
    }

    fn published(id: &str, version: u32) -> RecordedArtifact {
        RecordedArtifact {
            id: id.to_owned(),
            kind: "document".to_owned(),
            title: "Report".to_owned(),
            content: format!("text {version}"),
            version,
            provenance: None,
        }
    }

    /// Let a spawned write (if any) reach the double before the count is read.
    async fn settle() {
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    #[test]
    fn flag_parses_off_shadow_on_and_rejects_typos() {
        assert_eq!(
            ArtifactRevisionsMode::parse(None),
            Ok(ArtifactRevisionsMode::Off)
        );
        assert_eq!(
            ArtifactRevisionsMode::parse(Some("")),
            Ok(ArtifactRevisionsMode::Off)
        );
        assert_eq!(
            ArtifactRevisionsMode::parse(Some("off")),
            Ok(ArtifactRevisionsMode::Off)
        );
        assert_eq!(
            ArtifactRevisionsMode::parse(Some("shadow")),
            Ok(ArtifactRevisionsMode::Shadow)
        );
        assert_eq!(
            ArtifactRevisionsMode::parse(Some("on")),
            Ok(ArtifactRevisionsMode::On)
        );
        for typo in ["true", "1", "Shadow", "ON", " on", "shadow ", "of", "yes"] {
            assert!(
                ArtifactRevisionsMode::parse(Some(typo)).is_err(),
                "{typo:?}"
            );
        }
        assert_eq!(ArtifactRevisionsMode::default(), ArtifactRevisionsMode::Off);
        assert!(!ArtifactRevisionsMode::Off.writes());
        assert!(ArtifactRevisionsMode::Shadow.writes());
        assert!(ArtifactRevisionsMode::On.writes());
    }

    #[test]
    fn the_default_state_writes_nothing() {
        assert_eq!(
            AppState::new().artifact_revisions_mode,
            ArtifactRevisionsMode::Off
        );
    }

    #[test]
    fn only_a_persistent_turn_in_a_writing_mode_is_permitted() {
        assert!(permits(ArtifactRevisionsMode::Shadow, false, false));
        assert!(permits(ArtifactRevisionsMode::On, false, false));
        assert!(!permits(ArtifactRevisionsMode::Off, false, false));
        assert!(!permits(ArtifactRevisionsMode::Shadow, true, false));
        assert!(!permits(ArtifactRevisionsMode::Shadow, false, true));
    }

    #[test]
    fn plan_reads_the_committed_head_before_the_turn_is_seeded() {
        let committed = ArtifactVersionStore::new();
        committed.seed_persisted("thread-a", "report", "Report", "text 2", 2);
        let planned = plan_turn_revisions(
            &committed,
            "thread-a",
            &[published("report", 3), published("notes", 1)],
        );
        assert_eq!(planned[0].expected_version, 2);
        assert_eq!(planned[0].version, 3);
        assert_eq!(
            planned[1].expected_version, 0,
            "a new artifact must not exist yet"
        );
        assert_eq!(planned[1].version, 1);
    }

    #[test]
    fn the_append_is_a_model_revision_keyed_on_ids_only() {
        let planned = PlannedRevision {
            artifact_id: "report".to_owned(),
            kind: "deck".to_owned(),
            title: "Report".to_owned(),
            content: "text".to_owned(),
            version: 4,
            expected_version: 3,
        };
        let request = append_request(&target(), planned);
        assert_eq!(request.editor_kind, "model");
        assert_eq!(request.kind, "deck", "kind is an open string (C-B8)");
        assert_eq!(request.run_id, "run-a");
        assert_eq!((request.expected_version, request.version), (3, 4));
        assert_eq!(request.restored_from_version, 0);
        assert!(request.source_message_id.is_empty());
        assert_eq!(
            request.idempotency_key,
            idempotency_key("run-a", "report", 4)
        );
        assert!(request.idempotency_key.len() <= 128);
        assert_ne!(
            request.idempotency_key,
            idempotency_key("run-a", "report", 5)
        );
        assert!(!request.idempotency_key.contains("text"));
    }

    #[tokio::test]
    async fn off_mode_makes_no_call() {
        let fake = FakeStore::default();
        let state = state_with(fake.clone(), ArtifactRevisionsMode::Off).await;
        let committed = ArtifactVersionStore::new();
        assert!(!record_turn_revisions(
            &state,
            &bearer(),
            target(),
            &committed,
            &[published("report", 1)],
            false,
            false,
        ));
        settle().await;
        assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn zdr_turn_records_no_revision() {
        let fake = FakeStore::default();
        let state = state_with(fake.clone(), ArtifactRevisionsMode::Shadow).await;
        let committed = ArtifactVersionStore::new();
        assert!(!record_turn_revisions(
            &state,
            &bearer(),
            target(),
            &committed,
            &[published("report", 1)],
            true,
            false,
        ));
        settle().await;
        assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn waived_turn_records_no_revision() {
        let fake = FakeStore::default();
        let state = state_with(fake.clone(), ArtifactRevisionsMode::Shadow).await;
        let committed = ArtifactVersionStore::new();
        assert!(!record_turn_revisions(
            &state,
            &bearer(),
            target(),
            &committed,
            &[published("report", 1)],
            false,
            true,
        ));
        settle().await;
        assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_turn_without_artifacts_makes_no_call() {
        let fake = FakeStore::default();
        let state = state_with(fake.clone(), ArtifactRevisionsMode::Shadow).await;
        let committed = ArtifactVersionStore::new();
        assert!(!record_turn_revisions(
            &state,
            &bearer(),
            target(),
            &committed,
            &[],
            false,
            false,
        ));
        settle().await;
        assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn shadow_mode_writes_each_published_revision_once() {
        let fake = FakeStore::default();
        let state = state_with(fake.clone(), ArtifactRevisionsMode::Shadow).await;
        let committed = ArtifactVersionStore::new();
        committed.seed_persisted("thread-a", "report", "Report", "text 1", 1);
        assert!(record_turn_revisions(
            &state,
            &bearer(),
            target(),
            &committed,
            &[published("report", 2), published("notes", 1)],
            false,
            false,
        ));
        settle().await;
        assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
        let requests = fake.requests.lock().unwrap().clone();
        let heads: Vec<(&str, u32, u32)> = requests
            .iter()
            .map(|r| (r.artifact_id.as_str(), r.expected_version, r.version))
            .collect();
        assert_eq!(heads, [("report", 1, 2), ("notes", 0, 1)]);
    }

    #[tokio::test]
    async fn shadow_mode_never_fails_the_turn_on_conflict() {
        let fake = FakeStore {
            conflict: true,
            ..FakeStore::default()
        };
        let state = state_with(fake.clone(), ArtifactRevisionsMode::Shadow).await;
        let planned = plan_turn_revisions(
            &ArtifactVersionStore::new(),
            "thread-a",
            &[published("report", 1), published("notes", 1)],
        );
        let tally = write_turn(&state, &bearer(), &target(), planned).await;
        assert_eq!(
            tally,
            WriteTally {
                conflict: 2,
                ..WriteTally::default()
            },
            "every conflict is counted and the next artifact is still written"
        );
        assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn an_unreachable_store_is_only_counted() {
        // A port nothing listens on: the connect is refused, whatever runs on the dev ports.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let mut state = AppState::new();
        state.artifact_revision_client = ArtifactRevisionServiceClient::new(
            tonic::transport::Endpoint::from_shared(format!("http://{address}"))
                .unwrap()
                .connect_lazy(),
        );
        state.artifact_revisions_mode = ArtifactRevisionsMode::Shadow;
        let planned = plan_turn_revisions(
            &ArtifactVersionStore::new(),
            "thread-a",
            &[published("report", 1)],
        );
        let tally = write_turn(&state, &bearer(), &target(), planned).await;
        assert_eq!(tally.failed, 1);
        assert_eq!(tally.conflict, 0);
    }

    #[test]
    fn only_an_aborted_revision_mismatch_is_a_conflict() {
        assert!(is_conflict(&tonic::Status::aborted(REVISION_MISMATCH)));
        assert!(!is_conflict(&tonic::Status::aborted("other")));
        assert!(!is_conflict(&tonic::Status::failed_precondition(
            REVISION_MISMATCH
        )));
        assert!(!is_conflict(&tonic::Status::resource_exhausted(
            "artifact_revision_limit"
        )));
    }

    #[test]
    fn only_a_stored_on_mode_frame_names_its_editor() {
        assert_eq!(
            frame_editor(ArtifactRevisionsMode::On, false, false),
            Some("model")
        );
        assert_eq!(frame_editor(ArtifactRevisionsMode::Off, false, false), None);
        assert_eq!(frame_editor(ArtifactRevisionsMode::Shadow, false, false), None);
        assert_eq!(frame_editor(ArtifactRevisionsMode::On, true, false), None);
        assert_eq!(frame_editor(ArtifactRevisionsMode::On, false, true), None);
    }

    #[test]
    fn only_on_makes_the_table_the_authority() {
        assert!(!ArtifactRevisionsMode::Off.table_is_authority());
        assert!(!ArtifactRevisionsMode::Shadow.table_is_authority());
        assert!(ArtifactRevisionsMode::On.table_is_authority());
        assert!(ArtifactRevisionsMode::Shadow.writes_after_save());
        assert!(!ArtifactRevisionsMode::On.writes_after_save());
        assert!(!ArtifactRevisionsMode::Off.writes_after_save());
    }

    #[tokio::test]
    async fn on_mode_writes_at_the_commit_lock_not_after_save() {
        let fake = FakeStore::default();
        let state = state_with(fake.clone(), ArtifactRevisionsMode::On).await;
        let committed = ArtifactVersionStore::new();
        assert!(
            !record_turn_revisions(
                &state,
                &bearer(),
                target(),
                &committed,
                &[published("report", 1)],
                false,
                false,
            ),
            "the after-save write is shadow's"
        );
        settle().await;
        assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            commit_turn_revisions(
                &state,
                &bearer(),
                &target(),
                &committed,
                &[published("report", 1)],
                false,
                false,
            )
            .await,
            Ok(())
        );
        assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn the_commit_append_is_off_outside_on_and_for_zdr_or_waived_turns() {
        for (mode, zdr, waived) in [
            (ArtifactRevisionsMode::Off, false, false),
            (ArtifactRevisionsMode::Shadow, false, false),
            (ArtifactRevisionsMode::On, true, false),
            (ArtifactRevisionsMode::On, false, true),
        ] {
            let fake = FakeStore {
                conflict: true,
                ..FakeStore::default()
            };
            let state = state_with(fake.clone(), mode).await;
            let outcome = commit_turn_revisions(
                &state,
                &bearer(),
                &target(),
                &ArtifactVersionStore::new(),
                &[published("report", 1)],
                zdr,
                waived,
            )
            .await;
            assert_eq!(outcome, Ok(()), "{mode:?} zdr={zdr} waived={waived}");
            assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
        }
    }

    #[tokio::test]
    async fn cross_replica_conflict_is_artifact_revision_conflict() {
        // One session-core, two gateway replicas: each has its own process-local store and
        // commit lock, and both revise version 1 of the same report.
        let fake = FakeStore {
            cas: true,
            ..FakeStore::default()
        };
        fake.table.lock().unwrap().insert("report".to_owned(), 1);
        let replica_a = state_with(fake.clone(), ArtifactRevisionsMode::On).await;
        let replica_b = state_with(fake.clone(), ArtifactRevisionsMode::On).await;
        for replica in [&replica_a, &replica_b] {
            replica
                .artifact_versions
                .seed_persisted("thread-a", "report", "Report", "text 1", 1);
        }
        let first = commit_turn_revisions(
            &replica_a,
            &bearer(),
            &target(),
            &replica_a.artifact_versions,
            &[published("report", 2)],
            false,
            false,
        )
        .await;
        let second = commit_turn_revisions(
            &replica_b,
            &bearer(),
            &target(),
            &replica_b.artifact_versions,
            &[published("report", 2)],
            false,
            false,
        )
        .await;
        assert_eq!(first, Ok(()));
        assert_eq!(
            second,
            Err(RevisionConflict),
            "the second replica's turn ends as a revision conflict, not a lost write"
        );
        assert_eq!(fake.table.lock().unwrap()["report"], 2);
    }

    #[tokio::test]
    async fn the_commit_stops_at_the_first_conflict() {
        let fake = FakeStore {
            conflict: true,
            ..FakeStore::default()
        };
        let state = state_with(fake.clone(), ArtifactRevisionsMode::On).await;
        let outcome = commit_turn_revisions(
            &state,
            &bearer(),
            &target(),
            &ArtifactVersionStore::new(),
            &[published("report", 1), published("notes", 1)],
            false,
            false,
        )
        .await;
        assert_eq!(outcome, Err(RevisionConflict));
        assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn an_unreachable_store_does_not_fail_the_commit() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let mut state = AppState::new();
        state.artifact_revision_client = ArtifactRevisionServiceClient::new(
            tonic::transport::Endpoint::from_shared(format!("http://{address}"))
                .unwrap()
                .connect_lazy(),
        );
        state.artifact_revisions_mode = ArtifactRevisionsMode::On;
        let outcome = commit_turn_revisions(
            &state,
            &bearer(),
            &target(),
            &ArtifactVersionStore::new(),
            &[published("report", 1)],
            false,
            false,
        )
        .await;
        assert_eq!(outcome, Ok(()));
    }

    fn head(id: &str, version: u32) -> ArtifactRevision {
        ArtifactRevision {
            artifact_id: id.to_owned(),
            version,
            kind: "document".to_owned(),
            title: "Report".to_owned(),
            content: format!("table text {version}"),
            editor_kind: "model".to_owned(),
            ..ArtifactRevision::default()
        }
    }

    #[tokio::test]
    async fn turn_start_seeds_from_table_heads() {
        let fake = FakeStore {
            heads: vec![head("report", 4)],
            ..FakeStore::default()
        };
        let state = state_with(fake.clone(), ArtifactRevisionsMode::On).await;
        // This replica last saw version 2 from the message metadata.
        state
            .artifact_versions
            .seed_persisted("thread-a", "report", "Report", "metadata text 2", 2);
        let seeded =
            seed_from_table_heads(&state, &bearer(), "org-a", "thread-a", false, false).await;
        assert_eq!(seeded, 1);
        assert_eq!(
            state.artifact_versions.current_version("thread-a", "report"),
            Some(4)
        );
        assert_eq!(
            state
                .artifact_versions
                .content_of("thread-a", "report")
                .as_deref(),
            Some("table text 4"),
            "the base the next update revises is the table's text"
        );
        // The metadata rehydration that follows cannot lower it.
        state
            .artifact_versions
            .seed_persisted("thread-a", "report", "Report", "metadata text 2", 2);
        assert_eq!(
            state.artifact_versions.current_version("thread-a", "report"),
            Some(4)
        );
        let lists = fake.lists.lock().unwrap().clone();
        assert_eq!(lists.len(), 1);
        assert!(lists[0].artifact_id.is_empty(), "one heads read");
        assert!(lists[0].include_content);
        assert_eq!(lists[0].org_id, "org-a");
    }

    #[tokio::test]
    async fn seeding_reads_nothing_outside_on_or_for_zdr_or_waived_turns() {
        for (mode, zdr, waived) in [
            (ArtifactRevisionsMode::Off, false, false),
            (ArtifactRevisionsMode::Shadow, false, false),
            (ArtifactRevisionsMode::On, true, false),
            (ArtifactRevisionsMode::On, false, true),
        ] {
            let fake = FakeStore {
                heads: vec![head("report", 4)],
                ..FakeStore::default()
            };
            let state = state_with(fake.clone(), mode).await;
            assert_eq!(
                seed_from_table_heads(&state, &bearer(), "org-a", "thread-a", zdr, waived).await,
                0
            );
            assert!(fake.lists.lock().unwrap().is_empty());
            assert_eq!(
                state.artifact_versions.current_version("thread-a", "report"),
                None
            );
        }
    }
}
