//! `POST /v1/spaces/:space_id/runs/:run_id/live` (build plan WS09-S4): the member live view of an
//! agent run, as Server-Sent Events.
//!
//! A room turn is the direct tool loop on the OWNER's invoke SSE. Its text, tool calls and reasoning
//! sit in the owner-keyed stream buffer. Another member may watch it only through this route, and
//! only what `room_run.rs` lets through: a projection by allowlist, never a relay of raw frames.
//!
//! Authority is Session Core's. The caller's Control `model.thread.read` decision rides in the POST
//! BODY (never a URL, so it stays out of access logs) and is forwarded to
//! `RunService.AuthorizeRoomRunAttach`, the only verifier: decision, viewer membership, audience
//! ceiling, run-in-this-Space, zero-retention. It is asked once before the response head and again
//! every [`REAUTHORIZE_EVERY`] for as long as the stream lives. A refusal on a re-ask closes the
//! stream with a typed `error` frame, so a removed member loses the view within one interval. When
//! the verdict's validity window ends the stream sends `reauth_required` and closes; the client
//! reconnects with a fresh decision and gets a fresh snapshot.
//!
//! This module holds no authority of its own and reads the buffer only AFTER Session Core said yes.
//! The pointer from run id to buffer key is org-scoped, and its owner segment must equal the owner
//! Session Core reports, so a pointer can never cross users.
//!
//! Order on subscribe: rollout flag, request shape, ZDR credential, Session Core authorization,
//! pointer, attach caps (8 per user, 100 per run), response head. The first frame is always a
//! `snapshot` (the whole buffer folded); `Last-Event-ID` is accepted and a reconnect re-snapshots,
//! which is lossless. Then a 300 ms tail of the buffer through the same projector.
//!
//! Errors, as `{error:{code,message,details}}`: 400 `invalid_request`, 401 `unauthorized`, 403
//! `space_read_not_authorized`, 403 `zdr_feature_not_supported` (catalogue body, feature
//! `agent_run`), 404 `run_not_found` / `feature_unavailable`, 410 `run_stream_expired`
//! (`details.fallback: thread_transcript`), 429 `too_many_live_views`, 503 `run_live_unavailable`.
//! Messages are fixed words, never the upstream text. Telemetry labels carry no ids.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::post,
    Extension, Json, Router,
};
use mp_contracts::model_plane::v1::{AuthorizeRoomRunAttachRequest, AuthorizeRoomRunAttachResponse};
use mp_contracts::zdr_refusal::Refusal;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::auth::{Claims, VerifiedSessionBearer};
use crate::http_routes::authenticated_session_request;
use crate::room_run::{RoomFrame, RoomProjector, SnapshotHead};
use crate::state::AppState;
use crate::stream_buffer::RunPointer;

const MAX_ID_BYTES: usize = 128;
const MAX_DECISION_REF_BYTES: usize = 512;
const MAX_DECISION_TOKEN_BYTES: usize = 8 * 1024;
const KEEP_ALIVE: Duration = Duration::from_secs(15);

/// How often the verdict is asked again. This is the revocation latency of an open view.
const REAUTHORIZE_EVERY: Duration = Duration::from_secs(15);
/// The tail poll of the buffer (the same cadence the owner's resume tail uses).
const TAIL_POLL: Duration = Duration::from_millis(300);
/// A tail without proof of a live producer closes after this long with no new frame.
const TAIL_INACTIVITY_LIMIT: Duration = Duration::from_secs(120);
/// No attach lives longer than this, whatever the producer does.
const TAIL_MAX: Duration = Duration::from_secs(15 * 60);

/// Attach caps (plan: 8 per user, 100 per run).
const MAX_ATTACHES_PER_USER: usize = 8;
const MAX_ATTACHES_PER_RUN: usize = 100;

/// The room run live routes.
pub fn router() -> Router<AppState> {
    Router::new().route("/v1/spaces/:space_id/runs/:run_id/live", post(run_live))
}

#[derive(Debug, Deserialize)]
struct LiveBody {
    #[serde(default)]
    space_read_decision_ref: String,
    #[serde(default)]
    space_read_decision_token: String,
}

fn error_response(status: StatusCode, code: &str, message: &str, details: Value) -> Response {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message, "details": details } })),
    )
        .into_response()
}

fn plain(status: StatusCode, code: &str, message: &str) -> Response {
    error_response(status, code, message, json!({}))
}

fn count_attach(outcome: &'static str) {
    metrics::counter!("mp_room_run_attach_total", "outcome" => outcome).increment(1);
}

/// The HTTP form of a Session Core refusal, with the metric outcome. Fixed words only.
fn status_response(status: &tonic::Status) -> (Response, &'static str) {
    if let Some(refusal) = Refusal::from_status(status) {
        return (
            (StatusCode::FORBIDDEN, Json(refusal.json_body())).into_response(),
            "zdr",
        );
    }
    match status.code() {
        tonic::Code::Unauthenticated => (
            plain(StatusCode::UNAUTHORIZED, "unauthorized", "authentication required"),
            "unauthenticated",
        ),
        tonic::Code::PermissionDenied => (
            plain(
                StatusCode::FORBIDDEN,
                "space_read_not_authorized",
                "watching this run is not permitted",
            ),
            "forbidden",
        ),
        tonic::Code::NotFound => (
            plain(StatusCode::NOT_FOUND, "run_not_found", "the run is not available"),
            "not_found",
        ),
        tonic::Code::InvalidArgument => (
            plain(StatusCode::BAD_REQUEST, "invalid_request", "the live view request is invalid"),
            "invalid",
        ),
        _ => (
            plain(
                StatusCode::SERVICE_UNAVAILABLE,
                "run_live_unavailable",
                "the live run view is unavailable",
            ),
            "unavailable",
        ),
    }
}

// ---------------------------------------------------------------------------
// Attach caps
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Registry {
    by_user: HashMap<String, usize>,
    by_run: HashMap<String, usize>,
}

fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
}

/// One open attach. Released on drop, so a closed stream or a dropped client frees its slot.
struct AttachGuard {
    user_key: String,
    run_key: String,
}

impl AttachGuard {
    fn acquire(user_key: String, run_key: String) -> Option<Self> {
        let mut registry = registry().lock().ok()?;
        let user = registry.by_user.get(&user_key).copied().unwrap_or(0);
        let run = registry.by_run.get(&run_key).copied().unwrap_or(0);
        if user >= MAX_ATTACHES_PER_USER || run >= MAX_ATTACHES_PER_RUN {
            return None;
        }
        *registry.by_user.entry(user_key.clone()).or_insert(0) += 1;
        *registry.by_run.entry(run_key.clone()).or_insert(0) += 1;
        drop(registry);
        metrics::gauge!("mp_room_run_attach_active").increment(1.0);
        Some(Self { user_key, run_key })
    }
}

impl Drop for AttachGuard {
    fn drop(&mut self) {
        if let Ok(mut registry) = registry().lock() {
            let Registry { by_user, by_run } = &mut *registry;
            for (map, key) in [(by_user, &self.user_key), (by_run, &self.run_key)] {
                if let Some(count) = map.get_mut(key) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        map.remove(key);
                    }
                }
            }
        }
        metrics::gauge!("mp_room_run_attach_active").decrement(1.0);
    }
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

fn id_ok(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_ID_BYTES && !value.chars().any(char::is_control)
}

/// Ask Session Core whether this member may watch this run right now.
async fn authorize(
    state: &AppState,
    bearer: &VerifiedSessionBearer,
    request: &AuthorizeRoomRunAttachRequest,
) -> Result<AuthorizeRoomRunAttachResponse, tonic::Status> {
    let request = authenticated_session_request(request.clone(), bearer)
        .map_err(|_| tonic::Status::internal("session credential is not forwardable"))?;
    state
        .run_client
        .clone()
        .authorize_room_run_attach(request)
        .await
        .map(tonic::Response::into_inner)
}

async fn run_live(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    Path((space_id, run_id)): Path<(String, String)>,
    Json(body): Json<LiveBody>,
) -> Response {
    // 1. Rollout flag. A rollout control only.
    if !state.space_run_policy.live_view {
        count_attach("feature_off");
        return plain(StatusCode::NOT_FOUND, "feature_unavailable", "the live run view is not enabled");
    }
    // 2. Shape.
    let space_id = space_id.trim().to_owned();
    let run_id = run_id.trim().to_owned();
    if !id_ok(&space_id) || !id_ok(&run_id) {
        count_attach("invalid");
        return plain(StatusCode::BAD_REQUEST, "invalid_request", "the live view request is invalid");
    }
    let decision_ref = body.space_read_decision_ref.trim().to_owned();
    let decision_token = body.space_read_decision_token.trim().to_owned();
    if decision_ref.is_empty()
        || decision_ref.len() > MAX_DECISION_REF_BYTES
        || decision_token.is_empty()
        || decision_token.len() > MAX_DECISION_TOKEN_BYTES
    {
        count_attach("forbidden");
        return plain(
            StatusCode::FORBIDDEN,
            "space_read_not_authorized",
            "watching this run is not permitted",
        );
    }
    // 3. A user principal that is not zero-retention. There is no zero-retention form of this view.
    if claims.user_id.trim().is_empty() {
        count_attach("forbidden");
        return plain(
            StatusCode::FORBIDDEN,
            "space_read_not_authorized",
            "a user principal is required",
        );
    }
    if claims.zdr {
        count_attach("zdr");
        return (
            StatusCode::FORBIDDEN,
            Json(Refusal::feature_not_supported("agent_run").json_body()),
        )
            .into_response();
    }

    // 4. Session Core decides. Nothing below runs for a member it did not admit.
    let auth_request = AuthorizeRoomRunAttachRequest {
        run_id: run_id.clone(),
        space_id: space_id.clone(),
        space_read_decision_ref: decision_ref,
        space_read_decision_token: decision_token,
    };
    let verdict = match authorize(&state, &bearer, &auth_request).await {
        Ok(verdict) => verdict,
        Err(status) => {
            let (response, outcome) = status_response(&status);
            count_attach(outcome);
            return response;
        }
    };

    // 5. Where the producer's buffer lives. Org-scoped; the owner segment must be the owner
    // Session Core named, so a pointer never crosses users.
    let expired = || {
        count_attach("expired");
        error_response(
            StatusCode::GONE,
            "run_stream_expired",
            "the live view of this run has ended",
            json!({ "fallback": "thread_transcript" }),
        )
    };
    let Some(pointer) = state.stream_buffers.resolve_run(&claims.org_id, &run_id).await else {
        return expired();
    };
    let Some(request_id) = pointer_request_id(&pointer, &claims.org_id, &verdict.owner_subject_id)
    else {
        return expired();
    };

    // 6. Caps, then the response head.
    let Some(guard) = AttachGuard::acquire(
        format!("{}\u{1f}{}", claims.org_id, claims.user_id),
        format!("{}\u{1f}{}", claims.org_id, run_id),
    ) else {
        count_attach("capacity");
        return plain(
            StatusCode::TOO_MANY_REQUESTS,
            "too_many_live_views",
            "too many open live views",
        );
    };
    count_attach("ok");

    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(64);
    let live = Live {
        state,
        bearer,
        auth_request,
        org_id: claims.org_id.clone(),
        owner_subject_id: verdict.owner_subject_id.clone(),
        request_id,
        pointer,
        head: SnapshotHead {
            run_id: verdict.run_id.clone(),
            thread_id: verdict.thread_id.clone(),
            started_by: verdict.owner_subject_id.clone(),
            agent_name: String::new(),
            phase: verdict.run_phase.clone(),
            started_at: verdict.started_at_unix,
            expires_at: verdict.valid_until_unix,
        },
        valid_until: verdict.valid_until_unix,
        tx,
    };
    tokio::spawn(async move {
        let _guard = guard;
        live.drive().await;
    });
    Sse::new(ReceiverStream::new(rx))
        .keep_alive(KeepAlive::new().interval(KEEP_ALIVE).text("keepalive"))
        .into_response()
}

/// The invoke request id inside a pointer's scoped key, when the key belongs to `(org, owner)`.
fn pointer_request_id(pointer: &RunPointer, org_id: &str, owner_subject_id: &str) -> Option<String> {
    let mut parts = pointer.key.split('\u{1f}');
    let (org, owner, request_id) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || org != org_id || owner != owner_subject_id || request_id.is_empty() {
        return None;
    }
    Some(request_id.to_owned())
}

// ---------------------------------------------------------------------------
// The stream
// ---------------------------------------------------------------------------

struct Live {
    state: AppState,
    bearer: VerifiedSessionBearer,
    auth_request: AuthorizeRoomRunAttachRequest,
    org_id: String,
    owner_subject_id: String,
    request_id: String,
    pointer: RunPointer,
    head: SnapshotHead,
    /// Unix seconds until which the last verdict holds.
    valid_until: i64,
    tx: mpsc::Sender<Result<Event, Infallible>>,
}

impl Live {
    /// Send one frame under an optional source seq. False when the client is gone.
    async fn send(&self, seq: Option<u64>, frame: &RoomFrame) -> bool {
        let mut event = Event::default()
            .event(frame.event_name())
            .data(frame.to_json().to_string());
        if let Some(seq) = seq {
            event = event.id(seq.to_string());
        }
        self.tx.send(Ok(event)).await.is_ok()
    }

    fn error_frame(code: &str, message: &str, retryable: bool, feature: Option<&str>) -> RoomFrame {
        RoomFrame::Error {
            code: code.to_owned(),
            message: message.to_owned(),
            retryable,
            feature: feature.map(str::to_owned),
        }
    }

    async fn drive(mut self) {
        let policy = self.state.space_run_policy;
        let mut projector = RoomProjector::new(policy.limits, policy.live_text);
        let key = self.pointer.key.clone();
        self.head.agent_name = self.pointer.persona.clone();

        // Fold the whole buffer, then say what it came to. The frames the fold produced are not
        // sent one by one: the snapshot carries their sum, and a reconnect re-snapshots.
        let replay = self.state.stream_buffers.replay_after(&key, None).await;
        let folded = projector.apply_replay(&replay);
        let mut cursor: Option<u64> = replay.deltas.iter().map(|event| event.seq).max();
        let snapshot = projector.snapshot(&self.head);
        if !self.send(cursor, &snapshot).await {
            return;
        }
        if projector.is_finished() {
            for emitted in folded {
                if matches!(emitted.frame, RoomFrame::Finished { .. })
                    && !self.send(Some(emitted.seq), &emitted.frame).await
                {
                    return;
                }
            }
            return;
        }

        let mut producer_live = crate::run_control::ProducerLiveness::new(
            self.state.cancels.clone(),
            self.state.run_control.clone(),
            &self.request_id,
            &self.org_id,
            &self.owner_subject_id,
        );
        let started = tokio::time::Instant::now();
        let mut last_progress = started;
        let mut next_authorize = started + REAUTHORIZE_EVERY;
        loop {
            tokio::time::sleep(TAIL_POLL).await;
            if self.tx.is_closed() {
                return;
            }
            // The verdict has a window. Past it the client must come back with a fresh decision.
            if chrono::Utc::now().timestamp() >= self.valid_until {
                count_attach("reauth_required");
                let _ = self.send(None, &RoomFrame::ReauthRequired).await;
                return;
            }
            if tokio::time::Instant::now() >= next_authorize {
                next_authorize = tokio::time::Instant::now() + REAUTHORIZE_EVERY;
                match authorize(&self.state, &self.bearer, &self.auth_request).await {
                    Ok(verdict) => self.valid_until = verdict.valid_until_unix,
                    Err(status) => {
                        // A refusal ends the view. A transient failure keeps it until the window
                        // runs out, which is at most the decision's own lifetime.
                        let frame = match status.code() {
                            tonic::Code::PermissionDenied | tonic::Code::Unauthenticated => {
                                Some(Self::error_frame(
                                    "space_read_not_authorized",
                                    "watching this run is no longer permitted",
                                    false,
                                    None,
                                ))
                            }
                            tonic::Code::NotFound => Some(Self::error_frame(
                                "run_not_found",
                                "the run is not available",
                                false,
                                None,
                            )),
                            _ if Refusal::from_status(&status).is_some() => Some(Self::error_frame(
                                "zdr_feature_not_supported",
                                "live agent runs are not supported with zero data retention",
                                false,
                                Some("agent_run"),
                            )),
                            _ => None,
                        };
                        if let Some(frame) = frame {
                            count_attach("revoked");
                            let _ = self.send(None, &frame).await;
                            return;
                        }
                    }
                }
            }

            // Sampled BEFORE the read: a producer seen finished wrote its last frame first.
            let live = producer_live.is_live().await;
            let tail = self.state.stream_buffers.replay_after(&key, cursor).await;
            if let Some(max) = tail.deltas.iter().map(|event| event.seq).max() {
                cursor = Some(cursor.map_or(max, |c| c.max(max)));
                last_progress = tokio::time::Instant::now();
            }
            let emitted = projector.apply_replay(&tail);
            for item in &emitted {
                if !self.send(Some(item.seq), &item.frame).await {
                    return;
                }
            }
            if projector.is_finished() {
                return;
            }
            if started.elapsed() >= TAIL_MAX
                || (!live && last_progress.elapsed() >= TAIL_INACTIVITY_LIMIT)
            {
                let _ = self
                    .send(
                        None,
                        &Self::error_frame(
                            "run_stream_ended",
                            "the live view of this run has ended",
                            true,
                            None,
                        ),
                    )
                    .await;
                return;
            }
        }
    }
}
