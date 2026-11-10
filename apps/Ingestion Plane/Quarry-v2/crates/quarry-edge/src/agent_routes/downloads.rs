//! Q3-E3.4-7a — the run's governed downloads, and the hand-off of one to Data Plane.
//!
//!   GET  /v1/agent/runs/{run_id}/downloads
//!   POST /v1/agent/runs/{run_id}/downloads/{artifact_id}/knowledge
//!
//! A `download_ref` step stores an admitted file as a tenant-owned artifact (E3.4-4/6). Until
//! now nothing let its owner see those files or turn one into knowledge:
//! `ingest_client::DownloadHandoff` had no caller. These two routes are that caller. They add
//! no store and no new write path:
//!
//! * the list is read from the run's own receipts (the artifact id the download step pushed last
//!   in its proof bundle), then each candidate is read back through the artifact store under the
//!   caller's verified org, checked to be a browser download by its object key and to pass the
//!   download type allow-list by its bytes. It names the artifact, its type, size and digest. It
//!   carries no URL and no file name;
//! * the hand-off loads the same bytes, derives the text (the PDF parser, or UTF-8 for text,
//!   Markdown and CSV) and calls `DownloadHandoff::hand_off`, which is the one Data Plane ingest
//!   contract. The org comes from the verified token, never from the request.
//!
//! Dark: both routes answer 403 `downloads_disabled` until `QUARRY_EDGE__DOWNLOADS` is on, and
//! the hand-off answers the typed `download_handoff_disabled` until
//! `QUARRY_EDGE__DOWNLOAD_HANDOFF` is on. Both default off.
//!
//! ZDR: a ZDR run keeps no receipt and no download, so the list is empty, and the hand-off is
//! refused (`DownloadHandoffRejection::ZdrRefused`, 403) before any receipt or byte is read.

use super::*;

use quarry_core::ids::kinds::ArtifactKind;
use quarry_runtime::action_runtime::downloads as dl;
use quarry_runtime::ingest_client::{
    download_handoff_enabled_from_env, DownloadHandoff, DownloadHandoffClaim,
    DownloadHandoffRejection, DownloadedArtifact,
};
use sha2::{Digest, Sha256};

/// The most downloads one list answer carries; a run's later downloads are not read.
const MAX_LISTED_DOWNLOADS: usize = 64;

/// The longest `idempotency_key` accepted, in characters.
const MAX_IDEMPOTENCY_KEY_CHARS: usize = 128;

/// The most hand-off answers the process remembers. Past it an arbitrary older entry is dropped:
/// a repeat of a dropped hand-off reaches Data Plane again, whose own fingerprint dedupe answers.
const MAX_REMEMBERED_HANDOFFS: usize = 4096;

/// Where a listed download stands with the hand-off to Data Plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadHandoffState {
    /// This edge has not handed the file to Data Plane (since it started).
    NotHandedOff,
    /// This edge handed the file to Data Plane and holds the document id it answered.
    HandedOff,
}

/// One admitted download of a run, as the list names it.
#[derive(Debug, Clone, Serialize)]
pub struct RunDownload {
    /// The tenant-owned artifact that holds the bytes.
    pub artifact_id: String,
    /// The MIME type of the admitted type (`application/pdf`, `text/plain`, `text/markdown`,
    /// `text/csv`).
    pub mime_type: String,
    /// The size of the stored bytes.
    pub byte_size: u64,
    /// SHA-256 of the stored bytes, lower-case hex.
    pub sha256: String,
    /// The admitted type, as a sanitised extension (`pdf`, `txt`, `md`, `csv`).
    pub admitted_type: String,
    /// Whether this edge has handed the file to Data Plane.
    pub handoff_state: DownloadHandoffState,
}

/// The body of the hand-off route.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DownloadKnowledgeBody {
    /// A caller's retry label. Accepted and length-checked; the dedupe is by artifact id and
    /// digest, so a retry with or without it gets the first answer.
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

/// The answer of the hand-off route.
#[derive(Debug, Clone, Serialize)]
pub struct DownloadKnowledgeData {
    /// The Data Plane document the file became.
    pub document_id: String,
    /// `true` when this call created the document; `false` when an earlier call for the same
    /// artifact and digest had already done it (the same `document_id` is returned).
    pub created: bool,
}

/// A run the caller owns, resolved without reading any content.
struct OwnedRun {
    run_id: RunKind,
    /// The run is ZDR (its own posture, or the caller's floor). No receipts are read for it.
    zdr: bool,
    receipts: Vec<StepReceipt>,
}

type HandoffSlot = Arc<TokioMutex<Option<String>>>;

/// The hand-off answers this process has given, keyed by org, artifact id and digest. Holds a
/// document id (an opaque Data Plane id) and nothing of the file.
fn handoff_memory() -> &'static StdMutex<HashMap<String, HandoffSlot>> {
    static MEMORY: std::sync::OnceLock<StdMutex<HashMap<String, HandoffSlot>>> =
        std::sync::OnceLock::new();
    MEMORY.get_or_init(|| StdMutex::new(HashMap::new()))
}

fn handoff_key(org_id: &str, artifact_id: &str, sha256: &str) -> String {
    format!("{org_id}|{artifact_id}|{sha256}")
}

/// The slot for one hand-off, created on first use. A concurrent duplicate waits on the same
/// slot, so two racing calls hand the file over once.
fn handoff_slot(key: &str) -> HandoffSlot {
    let mut memory = handoff_memory().lock().expect("handoff memory poisoned");
    if let Some(slot) = memory.get(key) {
        return slot.clone();
    }
    if memory.len() >= MAX_REMEMBERED_HANDOFFS {
        if let Some(evict) = memory.keys().next().cloned() {
            memory.remove(&evict);
        }
    }
    let slot: HandoffSlot = Arc::new(TokioMutex::new(None));
    memory.insert(key.to_owned(), slot.clone());
    slot
}

/// The document id already answered for `key`, if any (and not mid-flight).
fn handed_off_document(key: &str) -> Option<String> {
    let memory = handoff_memory().lock().expect("handoff memory poisoned");
    let slot = memory.get(key)?;
    let guard = slot.try_lock().ok()?;
    guard.clone()
}

fn downloads_disabled(request_id: &str) -> ApiErr {
    driver_err(
        request_id,
        QuarryError::new(ErrorCode::Forbidden, "downloads_disabled"),
    )
}

fn run_not_found(request_id: &str) -> ApiErr {
    status_err(StatusCode::NOT_FOUND, request_id, "agent run not found")
}

/// The run behind `run_id` when the caller owns it, with its receipts unless it is ZDR. A live
/// run answers with its own posture; a run read from the durable store answers with the
/// caller's floor. A run another caller owns, or that has no receipts, is a plain 404.
async fn owned_run(
    state: &AppState,
    claims: &crate::auth::Claims,
    run_id: &str,
    request_id: &str,
) -> Result<OwnedRun, ApiErr> {
    let entry_arc = {
        let map = state.agent_runs.lock().expect("agent_runs mutex poisoned");
        map.get(run_id).map(|handle| {
            if handle.live.owned_by(claims) {
                handle.live.touch();
            }
            handle.entry.clone()
        })
    };
    let floor = claims.zdr_floor();
    if let Some(entry_arc) = entry_arc {
        let entry = entry_arc.lock().await;
        if !caller_owns_live_run(&entry, claims) {
            return Err(run_not_found(request_id));
        }
        let zdr = floor || entry.zdr.is_active();
        let receipts = if zdr {
            Vec::new()
        } else {
            state
                .receipts
                .list_for_actor(&entry.org_id, &entry.actor_id, &entry.run_id)
                .await
                .map_err(|e| driver_err(request_id, e))?
        };
        return Ok(OwnedRun {
            run_id: entry.run_id.clone(),
            zdr,
            receipts,
        });
    }
    let parsed = run_id
        .parse::<RunKind>()
        .map_err(|_| run_not_found(request_id))?;
    if floor {
        // A ZDR caller's runs keep no receipts: nothing to read, and nothing to prove ownership
        // with. The caller learns only that the posture is ZDR.
        return Ok(OwnedRun {
            run_id: parsed,
            zdr: true,
            receipts: Vec::new(),
        });
    }
    let receipts = state
        .receipts
        .list_for_actor(&claims.org_id, claims.actor_id(), &parsed)
        .await
        .map_err(|e| driver_err(request_id, e))?;
    if receipts.is_empty() {
        return Err(run_not_found(request_id));
    }
    Ok(OwnedRun {
        run_id: parsed,
        zdr: false,
        receipts,
    })
}

fn is_download_receipt(receipt: &StepReceipt) -> bool {
    matches!(
        receipt.action,
        AgentAction::DownloadRef { .. } | AgentAction::FrameDownloadRef { .. }
    ) && matches!(receipt.outcome, StepOutcome::Completed { .. })
}

/// The artifact each completed download step stored: the id the observation runner pushes last
/// in the step's proof bundle (the same one `settle_download` admitted). Oldest first, each id
/// once, at most [`MAX_LISTED_DOWNLOADS`].
fn download_candidates(receipts: &[StepReceipt]) -> Vec<(ArtifactKind, &StepReceipt)> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for receipt in receipts.iter().filter(|r| is_download_receipt(r)) {
        let Some(id) = receipt
            .observation
            .as_ref()
            .and_then(|obs| obs.proof_bundle.as_ref())
            .and_then(|proof| proof.artifact_ids.last())
        else {
            continue;
        };
        if seen.insert(id.to_string()) {
            out.push((id.clone(), receipt));
            if out.len() >= MAX_LISTED_DOWNLOADS {
                break;
            }
        }
    }
    out
}

/// A stored download read back for the caller's org and checked.
struct LoadedDownload {
    bytes: Vec<u8>,
    kind: dl::DownloadType,
    sha256: String,
}

fn admitted_extension(kind: dl::DownloadType) -> &'static str {
    match kind {
        dl::DownloadType::Pdf => "pdf",
        dl::DownloadType::PlainText => "txt",
        dl::DownloadType::Markdown => "md",
        dl::DownloadType::Csv => "csv",
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Reads the artifact under `org_id` (another org's artifact is a `NotFound`), requires its
/// object key to name a browser download and its bytes to pass the download type allow-list.
/// A type outside the allow-list is the typed `UnsupportedType` refusal.
async fn load_download(
    state: &AppState,
    org_id: &str,
    id: &ArtifactKind,
) -> QuarryResult<LoadedDownload> {
    let (bytes, key) = state.artifacts.get_with_key(org_id, id).await?;
    if !dl::is_download_artifact_key(&key) {
        return Err(QuarryError::new(ErrorCode::NotFound, "download not found"));
    }
    let Ok(kind) = dl::admit_type(None, &bytes) else {
        return Err(DownloadHandoffRejection::UnsupportedType.into_error());
    };
    let sha256 = sha256_hex(&bytes);
    Ok(LoadedDownload {
        bytes,
        kind,
        sha256,
    })
}

/// The page a download was taken from, as the receipt records it: masked as every receipt read
/// is, then reduced to scheme, host and path (no userinfo, query or fragment). Anything that is
/// not an http(s) URL is passed on as it is and refused by the hand-off.
fn download_source_url(receipt: &StepReceipt) -> String {
    let readable = receipt_for_read(receipt.clone(), false);
    let raw = readable
        .observation
        .as_ref()
        .map(|obs| obs.url.clone())
        .unwrap_or_default();
    match Url::parse(&raw) {
        Ok(mut parsed) => {
            let _ = parsed.set_username("");
            let _ = parsed.set_password(None);
            parsed.set_query(None);
            parsed.set_fragment(None);
            parsed.to_string()
        }
        Err(_) => raw,
    }
}

/// The text of a download: the bounded PDF parser for a PDF, strict UTF-8 for the text types
/// (the type check already required the bytes to be UTF-8 without a NUL).
async fn extract_text(loaded: &LoadedDownload) -> QuarryResult<String> {
    match loaded.kind {
        dl::DownloadType::Pdf => {
            let parsed = quarry_runtime::content_route::parse_document(
                quarry_transform::sniff::ContentKind::Pdf,
                loaded.bytes.clone(),
            )
            .await?;
            Ok(parsed.text)
        }
        dl::DownloadType::PlainText | dl::DownloadType::Markdown | dl::DownloadType::Csv => {
            String::from_utf8(loaded.bytes.clone())
                .map_err(|_| DownloadHandoffRejection::UnsupportedType.into_error())
        }
    }
}

/// `GET /v1/agent/runs/{run_id}/downloads` — the admitted downloads of a run the caller owns.
/// `403 downloads_disabled` until `QUARRY_EDGE__DOWNLOADS` is on; `404` for a run the caller
/// does not own. A ZDR run lists nothing. A stored file that no longer reads back, or is no
/// longer an admitted type, is left out of the list.
pub async fn list_run_downloads(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(run_id): Path<String>,
) -> Result<Json<Envelope<Vec<RunDownload>>>, ApiErr> {
    let request_id = crate::request_id::current();
    if !downloads_enabled() {
        return Err(downloads_disabled(&request_id));
    }
    let run = owned_run(&state, &claims, &run_id, &request_id).await?;
    if run.zdr {
        return Ok(Json(Envelope::ok(request_id, Vec::new())));
    }
    let mut listed = Vec::new();
    for (artifact_id, _receipt) in download_candidates(&run.receipts) {
        let Ok(loaded) = load_download(&state, &claims.org_id, &artifact_id).await else {
            continue;
        };
        let key = handoff_key(&claims.org_id, &artifact_id.to_string(), &loaded.sha256);
        let handoff_state = if handed_off_document(&key).is_some() {
            DownloadHandoffState::HandedOff
        } else {
            DownloadHandoffState::NotHandedOff
        };
        listed.push(RunDownload {
            artifact_id: artifact_id.to_string(),
            mime_type: loaded.kind.mime().to_owned(),
            byte_size: loaded.bytes.len() as u64,
            sha256: loaded.sha256,
            admitted_type: admitted_extension(loaded.kind).to_owned(),
            handoff_state,
        });
    }
    Ok(Json(Envelope::ok(request_id, listed)))
}

/// `POST /v1/agent/runs/{run_id}/downloads/{artifact_id}/knowledge` — hand one admitted download
/// of the caller's own run to Data Plane as a document.
///
/// Order: `403 downloads_disabled`, then the typed `download_handoff_disabled`, then
/// `download_handoff_zdr_refused` (403) for a ZDR caller or run, all before any receipt or byte
/// is read; then `404` for a run or download the caller does not own; then the type allow-list
/// and the parse; then `DownloadHandoff::hand_off`, whose typed refusals pass through. The org
/// and the user come from the verified claim. A repeat for the same artifact and digest gets the
/// first document id with `created: false`.
pub async fn send_download_to_knowledge(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path((run_id, artifact_id)): Path<(String, String)>,
    Json(body): Json<DownloadKnowledgeBody>,
) -> Result<Json<Envelope<DownloadKnowledgeData>>, ApiErr> {
    let request_id = crate::request_id::current();
    if !downloads_enabled() {
        return Err(downloads_disabled(&request_id));
    }
    if !download_handoff_enabled_from_env() {
        return Err(driver_err(
            &request_id,
            DownloadHandoffRejection::Disabled.into_error(),
        ));
    }
    if claims.zdr_floor() {
        return Err(driver_err(
            &request_id,
            DownloadHandoffRejection::ZdrRefused.into_error(),
        ));
    }
    if let Some(key) = body.idempotency_key.as_deref() {
        if key.chars().count() > MAX_IDEMPOTENCY_KEY_CHARS || key.chars().any(char::is_control) {
            return Err(status_err(
                StatusCode::BAD_REQUEST,
                &request_id,
                "idempotency_key is too long or has control characters",
            ));
        }
    }
    let artifact_id = artifact_id
        .parse::<ArtifactKind>()
        .map_err(|_| status_err(StatusCode::NOT_FOUND, &request_id, "download not found"))?;
    let run = owned_run(&state, &claims, &run_id, &request_id).await?;
    if run.zdr {
        return Err(driver_err(
            &request_id,
            DownloadHandoffRejection::ZdrRefused.into_error(),
        ));
    }
    let Some((_, receipt)) = download_candidates(&run.receipts)
        .into_iter()
        .find(|(id, _)| id.to_string() == artifact_id.to_string())
    else {
        return Err(status_err(
            StatusCode::NOT_FOUND,
            &request_id,
            "download not found",
        ));
    };
    let Some(ingest) = state.ingest.clone() else {
        return Err(driver_err(
            &request_id,
            QuarryError::new(
                ErrorCode::Unsupported,
                "data plane ingest is not configured",
            ),
        ));
    };
    let loaded = load_download(&state, &claims.org_id, &artifact_id)
        .await
        .map_err(|e| driver_err(&request_id, e))?;

    // One hand-off per org, artifact and digest: a repeat (or a racing duplicate that waited on
    // the slot) gets the first answer.
    let slot = handoff_slot(&handoff_key(
        &claims.org_id,
        &artifact_id.to_string(),
        &loaded.sha256,
    ));
    let mut slot = slot.lock().await;
    if let Some(document_id) = slot.clone() {
        return Ok(Json(Envelope::ok(
            request_id,
            DownloadKnowledgeData {
                document_id,
                created: false,
            },
        )));
    }

    let extracted_text = extract_text(&loaded)
        .await
        .map_err(|e| driver_err(&request_id, e))?;
    let artifact = DownloadedArtifact {
        artifact_id: artifact_id.clone(),
        org_id: claims.org_id.clone(),
        run_id: run.run_id.clone(),
        source_url: download_source_url(receipt),
        mime_type: loaded.kind.mime().to_owned(),
        byte_size: loaded.bytes.len() as u64,
        sha256: loaded.sha256.clone(),
        filename: Some(format!("download.{}", admitted_extension(loaded.kind))),
        extracted_text,
        zdr: ZdrMode::Off,
        privacy_policy: None,
    };
    let claim = DownloadHandoffClaim {
        org_id: claims.org_id.clone(),
        user_id: Some(claims.actor_id().to_owned()),
    };
    let response = DownloadHandoff::from_env(ingest)
        .hand_off(&claim, &artifact)
        .await
        .map_err(|e| driver_err(&request_id, e))?;
    *slot = Some(response.document_id.clone());
    Ok(Json(Envelope::ok(
        request_id,
        DownloadKnowledgeData {
            document_id: response.document_id,
            created: true,
        },
    )))
}
