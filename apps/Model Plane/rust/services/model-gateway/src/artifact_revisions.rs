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
//! `on` is S3's third state (the read routes); for this writer it behaves as `shadow`.

use mp_contracts::model_plane::v1::{AppendArtifactRevisionRequest, AppendOutcome};
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
}

/// Whether a turn of this posture writes revisions: the mode writes, and the turn is neither
/// ZDR nor waived.
#[must_use]
pub const fn permits(mode: ArtifactRevisionsMode, zdr: bool, waived: bool) -> bool {
    mode.writes() && !zdr && !waived
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
    if !permits(state.artifact_revisions_mode, zdr, waived) || artifacts.is_empty() {
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
                // Ids only: the shadow comparison is what this line is for.
                tracing::info!(
                    thread_id = %target.thread_id,
                    run_id = %target.run_id,
                    "artifact revision shadow conflict"
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
        AppendArtifactRevisionResponse, GetArtifactRevisionRequest, GetArtifactRevisionResponse,
        ListArtifactRevisionsRequest, ListArtifactRevisionsResponse,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    /// A revision store double: counts appends, keeps the requests, and answers each with a
    /// conflict or an append.
    #[derive(Clone, Default)]
    struct FakeStore {
        calls: Arc<AtomicUsize>,
        requests: Arc<Mutex<Vec<AppendArtifactRevisionRequest>>>,
        conflict: bool,
    }

    #[tonic::async_trait]
    impl ArtifactRevisionService for FakeStore {
        async fn append_artifact_revision(
            &self,
            request: tonic::Request<AppendArtifactRevisionRequest>,
        ) -> Result<tonic::Response<AppendArtifactRevisionResponse>, tonic::Status> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.requests.lock().unwrap().push(request.into_inner());
            if self.conflict {
                return Err(tonic::Status::aborted(REVISION_MISMATCH));
            }
            Ok(tonic::Response::new(AppendArtifactRevisionResponse {
                revision: None,
                outcome: AppendOutcome::Appended as i32,
            }))
        }

        async fn list_artifact_revisions(
            &self,
            _request: tonic::Request<ListArtifactRevisionsRequest>,
        ) -> Result<tonic::Response<ListArtifactRevisionsResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("not used"))
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
}
