use std::{collections::HashMap, sync::Arc};

use axum::{
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;

use crate::{
    cache::cache_key,
    config::AppState,
    envelope::{error, ok},
    middleware::AuthenticatedUser,
    upstream::authorized_org_id,
};

use super::shared;

const CHAT_HISTORY_TTL_SECS: u64 = 90 * 24 * 60 * 60;
/// A "this thread ran a ZDR turn" marker must outlive anything it guards, so it
/// gets the same window as the transcript store itself. A marker that expired
/// first would silently re-open persistence for the very thread it was written
/// to protect.
const CHAT_ZDR_MARKER_TTL_SECS: u64 = CHAT_HISTORY_TTL_SECS;
const MAX_THREADS: usize = 80;
const MAX_TRANSCRIPT_TURNS: usize = 160;
const MAX_TRANSCRIPT_STEPS: usize = 320;
const MAX_TITLE_LEN: usize = 96;
const MAX_PREVIEW_LEN: usize = 180;

#[derive(Clone, Default)]
pub(crate) struct ChatHistoryStore {
    memory: Arc<RwLock<HashMap<String, Value>>>,
}

impl ChatHistoryStore {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    async fn get(&self, key: &str) -> Option<Value> {
        self.memory.read().await.get(key).cloned()
    }

    async fn set(&self, key: String, value: Value) {
        self.memory.write().await.insert(key, value);
    }

    async fn delete(&self, key: &str) {
        self.memory.write().await.remove(key);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChatThreadSummary {
    thread_id: String,
    title: String,
    preview: String,
    updated_at: String,
    /// Whether the user pinned this thread to the top of the sidebar.
    ///
    /// Lives in THIS index rather than in session-core. That is not a shortcut:
    /// this index is already the cross-device home for presentation state — it
    /// is Dragonfly write-through, keyed per (org, user), with a 90-day TTL, and
    /// it already owns the AI-generated title that session-core does not know
    /// about. A pin is presentation, so it belongs beside the title.
    ///
    /// `#[serde(default)]` so an index written before pins existed decodes as
    /// unpinned instead of failing the whole listing.
    #[serde(default)]
    pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChatThreadTranscript {
    thread_id: String,
    turns: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    task_steps: Option<Vec<Value>>,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SaveThreadRequest {
    #[serde(default)]
    title: Option<String>,
    /// Absent means "leave the pin as it is", which is why this is an Option
    /// rather than a bool: the SPA saves a thread on every turn to refresh the
    /// title and preview, and a bare `false` default would silently unpin on
    /// the next message.
    #[serde(default)]
    pinned: Option<bool>,
    #[serde(default)]
    preview: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default)]
    turns: Option<Vec<Value>>,
    #[serde(default)]
    task_steps: Option<Vec<Value>>,
    /// A client may RAISE the retention posture for this snapshot. It can never
    /// lower it: [`retention_posture`] ORs this with the `x-zdr` header, the
    /// thread's own ZDR marker, and the organisation's standing posture.
    #[serde(default)]
    zdr: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThreadsResponse {
    sessions: Vec<ChatThreadSummary>,
    /// The server's retention verdict for this caller.
    ///
    /// The SPA also keeps a `localStorage` copy of conversations for instant
    /// paint on reload. That copy had the same defect this endpoint did — its
    /// only gate was the browser's in-memory temporary-chat `Set` — and being
    /// on the user's own device, it is the one copy no erasure fan-out can
    /// reach. Rather than have the SPA decide, the server states the posture
    /// here and the client obeys: no local content writes while `zdr` is true,
    /// and an existing local copy is dropped the moment it turns true.
    retention: RetentionPosture,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RetentionPosture {
    /// True when conversation content must not be retained anywhere —
    /// including the client's own storage.
    zdr: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThreadResponse {
    session: ChatThreadSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    transcript: Option<ChatThreadTranscript>,
    /// Whether this snapshot was actually retained.
    ///
    /// A ZDR save is answered `200 { retained: false }` rather than an error on
    /// purpose. The SPA saves a snapshot on EVERY turn, and org-wide ZDR is a
    /// normal configuration, not an anomaly — erroring there would paint a
    /// correctly-behaving workspace red on every message. `false` is the honest,
    /// machine-readable answer: the request was understood, and nothing was kept.
    retained: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TranscriptResponse {
    transcript: Option<ChatThreadTranscript>,
}

#[derive(Debug, Deserialize)]
struct DurableThreadsResponse {
    #[serde(default)]
    threads: Vec<DurableThreadSummary>,
}

#[derive(Debug, Deserialize)]
struct DurableThreadSummary {
    #[serde(default, alias = "threadId")]
    thread_id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    preview: String,
    #[serde(default, alias = "updatedAt")]
    updated_at: String,
    #[serde(default, alias = "createdAt")]
    created_at: String,
}

#[derive(Debug, Clone)]
struct ChatHistoryScope {
    org_id: String,
    user_id: String,
}

pub(super) async fn list_threads(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> Response {
    let scope = match scope_for(&state, &user).await {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    // An org can turn ZDR ON while a previous window's history is already
    // stored here. Refusing new writes alone would leave that residue readable
    // for the rest of its 90-day TTL, so the first listing under the new posture
    // clears the Frontend Plane's copy. After that this is a no-op — the index
    // is empty, and a ZDR turn never creates a thread in session-core to index.
    //
    // The purge drops OUR copy; it does not hide the owning plane's. Threads
    // session-core still holds (created before the posture changed) keep listing
    // through `read_durable_threads` below, which is read-through, not
    // retention. Suppressing them here would be this layer overriding a decision
    // that is not its to make — and would look like data loss to the user while
    // the data still exists one plane over.
    let zdr = crate::zdr::org_zdr_enabled(&state, &user).await;
    let cached = if zdr {
        let removed = purge_user_history(
            &state,
            &scope.org_id,
            &scope.user_id,
            PurgeScope::ContentOnly,
        )
        .await;
        if removed > 0 {
            tracing::info!(
                removed,
                "purged retained chat history: organization is under Zero Data Retention"
            );
        }
        Vec::new()
    } else {
        read_index(&state, &scope).await
    };
    let durable = read_durable_threads(&state, &user, &scope, &headers).await;
    let sessions = merge_thread_indexes(cached, durable);
    Json(ok(ThreadsResponse {
        sessions,
        retention: RetentionPosture { zdr },
    }))
    .into_response()
}

pub(super) async fn get_thread_transcript(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(thread_id): Path<String>,
) -> Response {
    let scope = match scope_for(&state, &user).await {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    let thread_id = normalize_id(&thread_id);
    if thread_id.is_empty() {
        return bad_request("thread_id is required.");
    }
    // Same converge-to-empty rule as the listing: a thread that acquired the ZDR
    // posture after it was stored must not still be readable here. Serving it
    // and purging later would mean the transcript endpoint is the one place the
    // guarantee does not hold.
    if thread_is_zdr(&state, &scope, &thread_id).await
        || crate::zdr::org_zdr_enabled(&state, &user).await
    {
        forget_thread(&state, &scope, &thread_id).await;
        return Json(ok(TranscriptResponse { transcript: None })).into_response();
    }
    let transcript = read_transcript(&state, &scope, &thread_id).await;
    Json(ok(TranscriptResponse { transcript })).into_response()
}

pub(super) async fn save_thread(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(thread_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<SaveThreadRequest>,
) -> Response {
    let scope = match scope_for(&state, &user).await {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    let thread_id = normalize_id(&thread_id);
    if thread_id.is_empty() {
        return bad_request("thread_id is required.");
    }

    // THE ZDR GATE. Everything below this point writes conversation content —
    // the transcript obviously, but the index too: `preview` is the last turn's
    // text and `title` is often the model's own summary of the conversation.
    //
    // The Model Plane guarantees a ZDR turn leaves zero durable trace:
    // `zdr_direct_stream` creates no thread, no run, and appends no message,
    // and `claims.effective_zdr(req.zdr)` ORs the token posture over the request
    // body so a client cannot downgrade it. This endpoint used to undo that
    // guarantee wholesale — it accepted a fully client-supplied transcript and
    // kept it in Frontend-Plane Dragonfly for 90 days with no server-side check
    // at all, while the Model Plane's telemetry correctly reported that nothing
    // had been retained. The only gate was an in-memory `Set` in the browser,
    // and a gate that lives in the SPA is not a gate: a reloaded tab, a replayed
    // request, a non-SPA client, or one regression is enough to defeat it.
    if retention_posture(&state, &user, &scope, &thread_id, &headers, &body).await {
        // A save under ZDR is treated as a PURGE, not merely a no-op. A thread
        // can acquire the posture partway through its life (an org enables ZDR,
        // or a branch carries a temporary thread's content into a new id), and
        // leaving the earlier copy behind would retain exactly the content the
        // posture forbids.
        forget_thread(&state, &scope, &thread_id).await;
        tracing::info!(
            thread_id = %thread_id,
            "chat snapshot not retained: Zero Data Retention posture"
        );
        // The echoed summary is built from the thread id and placeholders, NOT
        // from the submitted title/preview. Echoing the caller's own text back
        // would disclose nothing new, but it would make this response look like
        // a read of something stored, and the next reader should not have to
        // work out which.
        return Json(ok(ThreadResponse {
            session: ephemeral_summary(&thread_id),
            transcript: None,
            retained: false,
        }))
        .into_response();
    }

    let now = now_iso();
    let existing = read_index(&state, &scope)
        .await
        .into_iter()
        .find(|item| item.thread_id == thread_id);
    // A pin is metadata, not activity. Without this, toggling a pin sent no
    // `updated_at` and fell through to `now`, so pinning a month-old thread
    // relabelled it as touched today — misreporting the conversation's last
    // activity in the sidebar and in every recency sort downstream. When the
    // request changes nothing a reader would call content, keep the stored
    // timestamp.
    let metadata_only = body.title.is_none()
        && body.preview.is_none()
        && body.turns.is_none()
        && body.task_steps.is_none()
        && body.updated_at.is_none();
    let updated_at = if metadata_only {
        existing
            .as_ref()
            .map_or_else(|| now.clone(), |item| item.updated_at.clone())
    } else {
        normalize_timestamp(body.updated_at.as_deref(), &now)
    };
    let session = ChatThreadSummary {
        thread_id: thread_id.clone(),
        title: normalize_display_text(
            body.title
                .as_deref()
                .or(existing.as_ref().map(|item| item.title.as_str())),
            "Verevon Chat",
            MAX_TITLE_LEN,
        ),
        preview: normalize_display_text(
            body.preview
                .as_deref()
                .or(existing.as_ref().map(|item| item.preview.as_str())),
            "Open live session",
            MAX_PREVIEW_LEN,
        ),
        updated_at: updated_at.clone(),
        pinned: body
            .pinned
            .unwrap_or_else(|| existing.as_ref().is_some_and(|item| item.pinned)),
    };

    // Enrol in the org roster on the way in. This is the ONLY moment an
    // organisation erasure can learn that this user has history to purge —
    // hashed cache keys make the set the sole enumeration — so it must happen
    // on the same path that creates the thing being enumerated.
    remember_user_in_org(&state, &scope).await;

    let mut sessions = read_index(&state, &scope).await;
    sessions.retain(|item| item.thread_id != thread_id);
    sessions.push(session.clone());
    sort_pinned_first(&mut sessions);
    sessions.truncate(MAX_THREADS);
    write_value(
        &state,
        &index_key(&scope),
        serde_json::to_value(&sessions).unwrap_or(Value::Null),
    )
    .await;

    let transcript = body.turns.map(|turns| ChatThreadTranscript {
        thread_id: thread_id.clone(),
        turns: tail_values(turns, MAX_TRANSCRIPT_TURNS),
        task_steps: body
            .task_steps
            .map(|steps| tail_values(steps, MAX_TRANSCRIPT_STEPS))
            .filter(|steps| !steps.is_empty()),
        updated_at,
    });
    if let Some(transcript) = &transcript {
        write_value(
            &state,
            &transcript_key(&scope, &thread_id),
            serde_json::to_value(transcript).unwrap_or(Value::Null),
        )
        .await;
    }

    Json(ok(ThreadResponse {
        session,
        transcript,
        retained: true,
    }))
    .into_response()
}

/// The server-derived Zero Data Retention posture for one snapshot write.
///
/// Four independent sources, OR-ed — any one of them alone means "retain
/// nothing", and nothing a client sends can clear another's vote. This mirrors
/// `claims.effective_zdr(req.zdr)` in model-gateway, which likewise ORs a
/// posture the caller does not control over the one it does.
///
/// Ordered cheapest-first so the common non-ZDR save costs at most one org-core
/// lookup (itself cached for a minute), and a request that already declares ZDR
/// costs nothing at all.
async fn retention_posture(
    state: &AppState,
    user: &AuthenticatedUser,
    scope: &ChatHistoryScope,
    thread_id: &str,
    headers: &HeaderMap,
    body: &SaveThreadRequest,
) -> bool {
    // 1 + 2. This request says so: the `x-zdr` header, or an explicit
    //        `zdr: true` in the body.
    if shared::zdr_flag(headers) || body.zdr.unwrap_or(false) {
        return true;
    }
    // 3. This THREAD has already run a ZDR turn. Written server-side by the
    //    stream/invoke path (see `mark_thread_zdr`) from the same normalized
    //    posture that is forwarded to the Model Plane, so it survives a page
    //    reload, a replayed save, and a client that never heard of ZDR — the
    //    exact failures the browser's in-memory Set cannot survive.
    if thread_is_zdr(state, scope, thread_id).await {
        return true;
    }
    // 4. The ORGANISATION is ZDR. No client input reaches this at all, and it
    //    fails closed if org-core cannot be reached.
    crate::zdr::org_zdr_enabled(state, user).await
}

/// A response-only summary for a thread that was deliberately not retained.
fn ephemeral_summary(thread_id: &str) -> ChatThreadSummary {
    ChatThreadSummary {
        thread_id: thread_id.to_owned(),
        title: "Verevon Chat".to_owned(),
        preview: String::new(),
        updated_at: now_iso(),
        pinned: false,
    }
}

/// Remove every retained trace of one thread: its transcript, and its entry in
/// the sidebar index. Used by the ZDR gate and by erasure — never by an ordinary
/// save.
async fn forget_thread(state: &AppState, scope: &ChatHistoryScope, thread_id: &str) {
    delete_value(state, &transcript_key(scope, thread_id)).await;
    let mut sessions = read_index(state, scope).await;
    let before = sessions.len();
    sessions.retain(|item| item.thread_id != thread_id);
    if sessions.len() != before {
        write_value(
            state,
            &index_key(scope),
            serde_json::to_value(&sessions).unwrap_or(Value::Null),
        )
        .await;
    }
}

pub(super) async fn delete_thread(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(thread_id): Path<String>,
) -> Response {
    let scope = match scope_for(&state, &user).await {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    let thread_id = normalize_id(&thread_id);
    if thread_id.is_empty() {
        return bad_request("thread_id is required.");
    }
    let mut sessions = read_index(&state, &scope).await;
    sessions.retain(|item| item.thread_id != thread_id);
    write_value(
        &state,
        &index_key(&scope),
        serde_json::to_value(&sessions).unwrap_or(Value::Null),
    )
    .await;
    delete_value(&state, &transcript_key(&scope, &thread_id)).await;
    // A deletion response reports the posture too, so a client that only ever
    // deletes still learns it must not be keeping a local copy.
    let zdr = crate::zdr::org_zdr_enabled(&state, &user).await;
    Json(ok(ThreadsResponse {
        sessions,
        retention: RetentionPosture { zdr },
    }))
    .into_response()
}

pub(super) async fn clear_threads(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    let scope = match scope_for(&state, &user).await {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    let sessions = read_index(&state, &scope).await;
    for item in sessions {
        delete_value(&state, &transcript_key(&scope, &item.thread_id)).await;
    }
    write_value(
        &state,
        &index_key(&scope),
        serde_json::to_value(Vec::<ChatThreadSummary>::new()).unwrap_or(Value::Null),
    )
    .await;
    let zdr = crate::zdr::org_zdr_enabled(&state, &user).await;
    Json(ok(ThreadsResponse {
        sessions: vec![],
        retention: RetentionPosture { zdr },
    }))
    .into_response()
}

async fn scope_for(
    state: &AppState,
    user: &AuthenticatedUser,
) -> Result<ChatHistoryScope, Response> {
    let org_id = authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(error(
                "org_scope_required",
                "An authorized organization scope is required.",
            )),
        )
            .into_response());
    }
    Ok(ChatHistoryScope {
        org_id,
        user_id: user.user_id.clone(),
    })
}

async fn read_index(state: &AppState, scope: &ChatHistoryScope) -> Vec<ChatThreadSummary> {
    read_value(state, &index_key(scope))
        .await
        .and_then(|value| serde_json::from_value::<Vec<ChatThreadSummary>>(value).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|item| !item.thread_id.trim().is_empty())
        .take(MAX_THREADS)
        .collect()
}

async fn read_durable_threads(
    state: &AppState,
    user: &AuthenticatedUser,
    scope: &ChatHistoryScope,
    headers: &HeaderMap,
) -> Vec<ChatThreadSummary> {
    let Some(token) = shared::model_token(state, user, headers).await else {
        return Vec::new();
    };
    let Some(session_token) = shared::session_token(state, user, headers).await else {
        return Vec::new();
    };
    let url = format!("{}/v1/threads?limit={MAX_THREADS}", state.model_gateway_url);
    let response = state
        .client
        .get(url)
        .bearer_auth(token)
        .header("x-session-authorization", format!("Bearer {session_token}"))
        .header("x-user-id", &scope.user_id)
        .header("x-org-id", &scope.org_id)
        .send()
        .await;
    let Ok(response) = response else {
        return Vec::new();
    };
    if !response.status().is_success() {
        return Vec::new();
    }
    let Ok(payload) = response.json::<DurableThreadsResponse>().await else {
        return Vec::new();
    };
    let now = now_iso();
    payload
        .threads
        .into_iter()
        .filter_map(|item| durable_to_summary(item, &now))
        .collect()
}

fn durable_to_summary(item: DurableThreadSummary, now: &str) -> Option<ChatThreadSummary> {
    let thread_id = normalize_id(&item.thread_id);
    if thread_id.is_empty() {
        return None;
    }
    let updated_at = if item.updated_at.trim().is_empty() {
        normalize_timestamp(Some(&item.created_at), now)
    } else {
        normalize_timestamp(Some(&item.updated_at), now)
    };
    Some(ChatThreadSummary {
        thread_id,
        title: normalize_display_text(Some(&item.title), "Verevon Chat", MAX_TITLE_LEN),
        preview: normalize_display_text(Some(&item.preview), "", MAX_PREVIEW_LEN),
        updated_at,
        // session-core has no pin concept, so a durable entry is always
        // unpinned. `merge_thread_indexes` must therefore never let a durable
        // entry overwrite a cached pin — see the OR there.
        pinned: false,
    })
}

/// Order the index for the sidebar: pinned threads first, then by recency.
///
/// This must run BEFORE any `truncate(MAX_THREADS)`. Sorting by `updated_at`
/// alone and then cutting at the cap destroyed pins: an old pinned thread sorted
/// to the bottom, fell outside the cap, and the truncated list was written
/// straight back to the index — losing both the pin and the thread. The same bug
/// existed client-side in `chat-thread-history.ts` and is fixed there too.
fn sort_pinned_first(sessions: &mut [ChatThreadSummary]) {
    sessions.sort_by(|left, right| {
        right
            .pinned
            .cmp(&left.pinned)
            .then_with(|| right.updated_at.cmp(&left.updated_at))
    });
}

fn merge_thread_indexes(
    cached: Vec<ChatThreadSummary>,
    durable: Vec<ChatThreadSummary>,
) -> Vec<ChatThreadSummary> {
    let mut sessions: Vec<ChatThreadSummary> = Vec::new();
    // CACHED first: the SPA's saved snapshot owns presentation (its title may
    // be the AI-generated thread summary), while session-core's durable
    // summary auto-titles threads with the raw first message. Durable entries
    // still contribute threads the SPA never snapshotted, fill empty fields,
    // and advance `updated_at`. With durable first, every listing clobbered a
    // generated title back to the echoed question.
    for item in cached.into_iter().chain(durable) {
        if item.thread_id.trim().is_empty() {
            continue;
        }
        if let Some(existing) = sessions
            .iter_mut()
            .find(|existing| existing.thread_id == item.thread_id)
        {
            if existing.title.trim().is_empty() || existing.title == "Verevon Chat" {
                existing.title = item.title;
            }
            if existing.preview.trim().is_empty() {
                existing.preview = item.preview;
            }
            // OR, never assign: `durable_to_summary` always reports `false`,
            // so assigning would unpin every thread on each listing that
            // reaches session-core.
            existing.pinned = existing.pinned || item.pinned;
            if item.updated_at > existing.updated_at {
                existing.updated_at = item.updated_at;
            }
        } else {
            sessions.push(item);
        }
    }
    sort_pinned_first(&mut sessions);
    sessions.truncate(MAX_THREADS);
    sessions
}

async fn read_transcript(
    state: &AppState,
    scope: &ChatHistoryScope,
    thread_id: &str,
) -> Option<ChatThreadTranscript> {
    read_value(state, &transcript_key(scope, thread_id))
        .await
        .and_then(|value| serde_json::from_value::<ChatThreadTranscript>(value).ok())
}

async fn read_value(state: &AppState, logical_key: &str) -> Option<Value> {
    let redis_key = cache_key("chat-history", &[logical_key]);
    if let Some(value) = state
        .cache
        .lookup_within(&redis_key, CHAT_HISTORY_TTL_SECS)
        .await
    {
        state
            .chat_history_store
            .set(logical_key.to_owned(), value.clone())
            .await;
        return Some(value);
    }
    state.chat_history_store.get(logical_key).await
}

async fn write_value(state: &AppState, logical_key: &str, value: Value) {
    state
        .chat_history_store
        .set(logical_key.to_owned(), value.clone())
        .await;
    let redis_key = cache_key("chat-history", &[logical_key]);
    state
        .cache
        .store_for_secs(&redis_key, &value, CHAT_HISTORY_TTL_SECS)
        .await;
}

async fn delete_value(state: &AppState, logical_key: &str) {
    state.chat_history_store.delete(logical_key).await;
    let redis_key = cache_key("chat-history", &[logical_key]);
    state.cache.delete(&redis_key).await;
}

fn index_key(scope: &ChatHistoryScope) -> String {
    format!("chat-history:index:{}:{}", scope.org_id, scope.user_id)
}

/// Redis key for an organisation's chat-history user ROSTER.
///
/// # Why a roster has to exist at all
///
/// `cache_key` HASHES its parts (`verevon:gw:chat-history:<hash>`), so there is
/// no `SCAN chat-history:index:{org}:*` — the literal key never reaches Redis.
/// Every other purge here walks the per-user index, but an ORGANISATION erasure
/// arrives naming only the org: without a roster there is no way to discover
/// which users under it have stored history, and the org-wide fan-out would have
/// nothing to iterate. This set is that enumeration, and nothing else reads it.
///
/// It holds user ids only — no conversation content, no titles, no previews.
/// Not hashed through `cache_key`, because the erasure consumer must be able to
/// build this key from an org id alone, with no prior read.
fn org_roster_key(org_id: &str) -> String {
    format!("verevon:gw:chat-history:roster:{org_id}")
}

/// Record that this (org, user) has stored chat history, so an organisation
/// erasure can find them. Idempotent (`SADD`), and re-arms the roster TTL.
async fn remember_user_in_org(state: &AppState, scope: &ChatHistoryScope) {
    state
        .cache
        .set_add(
            &org_roster_key(&scope.org_id),
            &scope.user_id,
            CHAT_HISTORY_TTL_SECS,
        )
        .await;
}

fn transcript_key(scope: &ChatHistoryScope, thread_id: &str) -> String {
    format!(
        "chat-history:transcript:{}:{}:{}",
        scope.org_id, scope.user_id, thread_id
    )
}

/// Key for the "this thread has run a ZDR turn" marker.
///
/// Scoped identically to the transcript it guards, so the marker and the thing
/// it protects can never be looked up under different identities.
fn zdr_marker_key(scope: &ChatHistoryScope, thread_id: &str) -> String {
    format!(
        "chat-history:zdr:{}:{}:{}",
        scope.org_id, scope.user_id, thread_id
    )
}

/// Record, server-side, that this thread has run a Zero Data Retention turn.
///
/// Called from the chat stream/invoke paths with the SAME normalized posture
/// that is forwarded to the Model Plane, so the BFF's notion of "temporary" is
/// derived from the request the model actually served rather than from the
/// browser's memory. The marking is one-way and sticky for the thread's whole
/// life, matching the SPA's own rule ("temporary chat locks in at the first
/// send") — but durably, where a reload cannot lose it.
///
/// Best-effort by construction: a marker that fails to write leaves the org
/// posture and the per-request flags still guarding the save path.
pub(crate) async fn mark_thread_zdr(
    state: &AppState,
    org_id: &str,
    user_id: &str,
    thread_id: &str,
) {
    let thread_id = normalize_id(thread_id);
    if org_id.trim().is_empty() || user_id.trim().is_empty() || thread_id.is_empty() {
        return;
    }
    let scope = ChatHistoryScope {
        org_id: org_id.to_owned(),
        user_id: user_id.to_owned(),
    };
    let key = cache_key("chat-history", &[&zdr_marker_key(&scope, &thread_id)]);
    state
        .cache
        .store_for_secs(&key, &Value::Bool(true), CHAT_ZDR_MARKER_TTL_SECS)
        .await;
    // Mirror into the process-local store so a single-instance deployment with
    // no Dragonfly configured is still protected. `read_value`'s Redis-first /
    // memory-fallback shape means either tier alone is enough to deny a save.
    state
        .chat_history_store
        .set(zdr_marker_key(&scope, &thread_id), Value::Bool(true))
        .await;
}

/// Whether this thread carries a ZDR marker.
async fn thread_is_zdr(state: &AppState, scope: &ChatHistoryScope, thread_id: &str) -> bool {
    read_value(state, &zdr_marker_key(scope, thread_id))
        .await
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

/// What a purge is allowed to remove.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PurgeScope {
    /// Drop retained content, but KEEP the ZDR markers. Used when the posture
    /// itself triggered the purge: forgetting why a thread was protected would
    /// let the very next save re-create what was just removed.
    ContentOnly,
    /// Erasure. Drop the markers too — the subject is gone, and a marker is
    /// itself a record that this person held a conversation under this id.
    Everything,
}

/// Delete this (org, user)'s retained chat history.
///
/// Returns the number of threads whose transcripts were removed.
///
/// The Redis keys are HASHED by `cache_key`, so there is no wildcard/SCAN path
/// to a user's transcripts — the index IS the enumeration, which is why it is
/// read first and cleared last. A transcript whose index entry was already lost
/// is unreachable by any code path and expires with its TTL.
pub(crate) async fn purge_user_history(
    state: &AppState,
    org_id: &str,
    user_id: &str,
    purge: PurgeScope,
) -> usize {
    if org_id.trim().is_empty() || user_id.trim().is_empty() {
        return 0;
    }
    let scope = ChatHistoryScope {
        org_id: org_id.to_owned(),
        user_id: user_id.to_owned(),
    };
    let sessions = read_index(state, &scope).await;
    let removed = sessions.len();
    for item in &sessions {
        delete_value(state, &transcript_key(&scope, &item.thread_id)).await;
        if purge == PurgeScope::Everything {
            delete_value(state, &zdr_marker_key(&scope, &item.thread_id)).await;
        }
    }
    delete_value(state, &index_key(&scope)).await;
    removed
}

/// Delete every user's retained chat history for one organisation.
///
/// Returns `(users, threads)` actually purged.
///
/// Driven by the cross-plane GDPR erasure fan-out for
/// `subject_type: "organization"`. The org roster is the enumeration (see
/// [`org_roster_key`]); the roster itself is deleted last, so a redelivery that
/// crashes midway still finds the remaining users on the next attempt. Fully
/// idempotent — a second delivery finds an empty roster and purges nothing,
/// which is what NATS at-least-once requires.
pub(crate) async fn purge_org_history(state: &AppState, org_id: &str) -> (usize, usize) {
    let org_id = org_id.trim();
    if org_id.is_empty() {
        return (0, 0);
    }
    let roster_key = org_roster_key(org_id);
    let members = state.cache.set_members(&roster_key).await;
    let mut threads = 0;
    for user_id in &members {
        threads += purge_user_history(state, org_id, user_id, PurgeScope::Everything).await;
    }
    state.cache.delete(&roster_key).await;
    (members.len(), threads)
}

/// Makes Support-derived threads durably read-only at the same-origin trust
/// boundary. Classification is encoded in an immutable thread namespace, so
/// enforcement cannot fail open across replicas, restarts, or cache outages.
pub(super) fn enforce_support_thread_policy(body: &mut Value) -> Result<(), &'static str> {
    let Some(object) = body.as_object_mut() else {
        return Ok(());
    };
    let requested = object
        .remove("support_read_only")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let thread_id = object
        .get("thread_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    if requested && thread_id.is_none() {
        return Err("support_read_only requires an explicit thread_id");
    }

    let Some(thread_id) = thread_id else {
        return Ok(());
    };
    let restricted = thread_id.starts_with("support_");
    if requested && !restricted {
        return Err("support_read_only requires a support_ thread_id");
    }
    if restricted {
        strip_support_thread_capabilities(object, &thread_id);
    }
    Ok(())
}

fn strip_support_thread_capabilities(object: &mut serde_json::Map<String, Value>, thread_id: &str) {
    let features = object
        .get("features")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|item| !matches!(item.as_str(), Some("tools" | "agentic")))
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    object.insert("features".to_owned(), Value::Array(features));
    object.insert("tools".to_owned(), Value::Array(vec![]));
    object.insert("browse_web".to_owned(), Value::Bool(false));
    object.insert("generate_image".to_owned(), Value::Bool(false));
    object.insert("plan_mode".to_owned(), Value::Bool(false));
    object.insert("deep_research".to_owned(), Value::Bool(false));
    object.insert(
        "session_key".to_owned(),
        Value::String(thread_id.to_owned()),
    );
}

fn normalize_id(value: &str) -> String {
    value.trim().to_owned()
}

fn normalize_display_text(value: Option<&str>, fallback: &str, max_len: usize) -> String {
    let text = value
        .map(|value| value.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| fallback.to_owned());
    if text.chars().count() <= max_len {
        return text;
    }
    let mut truncated = text
        .chars()
        .take(max_len.saturating_sub(3))
        .collect::<String>();
    truncated = truncated.trim_end().to_owned();
    format!("{truncated}...")
}

fn normalize_timestamp(value: Option<&str>, fallback: &str) -> String {
    value
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc).to_rfc3339())
        .unwrap_or_else(|| fallback.to_owned())
}

fn tail_values(mut values: Vec<Value>, max_len: usize) -> Vec<Value> {
    if values.len() <= max_len {
        return values;
    }
    values.drain(0..values.len() - max_len);
    values
}

fn now_iso() -> String {
    Utc::now().to_rfc3339()
}

fn bad_request(message: &'static str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(error("invalid_request", message)),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::{
        matchers::{method as wm_method, path as wm_path},
        Mock, MockServer, ResponseTemplate,
    };

    fn summary(thread_id: &str, title: &str, preview: &str, updated_at: &str) -> ChatThreadSummary {
        ChatThreadSummary {
            thread_id: thread_id.to_owned(),
            title: title.to_owned(),
            preview: preview.to_owned(),
            updated_at: updated_at.to_owned(),
            pinned: false,
        }
    }

    fn pinned(thread_id: &str, updated_at: &str) -> ChatThreadSummary {
        ChatThreadSummary {
            pinned: true,
            ..summary(thread_id, "pinned thread", "", updated_at)
        }
    }

    /// The data-loss bug this ordering exists to prevent: an OLD pinned thread
    /// sorted to the bottom by recency, cut by the cap, and the truncated list
    /// written back — losing the pin and the thread together.
    #[test]
    fn an_old_pinned_thread_survives_the_cap() {
        let mut sessions: Vec<ChatThreadSummary> = (0..MAX_THREADS)
            .map(|index| {
                summary(
                    &format!("t{index}"),
                    "recent",
                    "",
                    &format!("2026-08-01T10:{index:02}:00Z"),
                )
            })
            .collect();
        sessions.push(pinned("old-but-pinned", "2020-01-01T00:00:00Z"));

        sort_pinned_first(&mut sessions);
        sessions.truncate(MAX_THREADS);

        assert_eq!(
            sessions[0].thread_id, "old-but-pinned",
            "pinned must sort first"
        );
        assert!(
            sessions
                .iter()
                .any(|item| item.thread_id == "old-but-pinned"),
            "a pinned thread must never be truncated away"
        );
        assert_eq!(sessions.len(), MAX_THREADS);
    }

    /// Within each group, recency still decides.
    #[test]
    fn recency_orders_within_the_pinned_and_unpinned_groups() {
        let mut sessions = vec![
            summary("older", "a", "", "2026-01-01T00:00:00Z"),
            pinned("pin-older", "2020-01-01T00:00:00Z"),
            summary("newer", "b", "", "2026-08-01T00:00:00Z"),
            pinned("pin-newer", "2026-07-01T00:00:00Z"),
        ];
        sort_pinned_first(&mut sessions);
        let order: Vec<&str> = sessions
            .iter()
            .map(|item| item.thread_id.as_str())
            .collect();
        assert_eq!(order, ["pin-newer", "pin-older", "newer", "older"]);
    }

    /// session-core has no pin concept, so every durable entry reports
    /// `pinned: false`. Merging must never let that unpin a cached pin.
    #[test]
    fn a_durable_listing_cannot_unpin_a_pinned_thread() {
        let cached = vec![pinned("t1", "2026-08-01T00:00:00Z")];
        let durable = vec![summary(
            "t1",
            "raw first message",
            "preview",
            "2026-08-02T00:00:00Z",
        )];

        let merged = merge_thread_indexes(cached, durable);

        assert_eq!(merged.len(), 1);
        assert!(merged[0].pinned, "a durable refresh must not clear the pin");
        // And the durable entry still advances recency, as it did before.
        assert_eq!(merged[0].updated_at, "2026-08-02T00:00:00Z");
    }

    #[test]
    fn cached_snapshot_title_wins_over_durable_auto_title() {
        let cached = vec![summary(
            "t1",
            "Oslo: Norges kulturelle hovedstad",
            "answer preview",
            "2026-07-29T21:40:53+00:00",
        )];
        let durable = vec![summary(
            "t1",
            "Hva er hovedstaden i Norge, og hva er byen mest kjent for?",
            "answer preview",
            "2026-07-29T21:41:19+00:00",
        )];
        let merged = merge_thread_indexes(cached, durable);
        assert_eq!(merged.len(), 1);
        // The SPA snapshot's (possibly AI-generated) title survives listing…
        assert_eq!(merged[0].title, "Oslo: Norges kulturelle hovedstad");
        // …while durable activity still advances the timestamp.
        assert_eq!(merged[0].updated_at, "2026-07-29T21:41:19+00:00");
    }

    #[test]
    fn durable_threads_still_appear_and_fill_placeholder_fields() {
        let cached = vec![summary(
            "t1",
            "Verevon Chat",
            "",
            "2026-07-29T10:00:00+00:00",
        )];
        let durable = vec![
            summary(
                "t1",
                "Real question",
                "real preview",
                "2026-07-29T09:00:00+00:00",
            ),
            summary("t2", "Durable only", "p", "2026-07-29T11:00:00+00:00"),
        ];
        let merged = merge_thread_indexes(cached, durable);
        assert_eq!(merged.len(), 2);
        // Sorted by activity: t2 (11:00) ahead of t1 (10:00).
        assert_eq!(merged[0].thread_id, "t2");
        assert_eq!(merged[0].title, "Durable only");
        // Placeholder cached fields are filled from the durable summary.
        assert_eq!(merged[1].title, "Real question");
        assert_eq!(merged[1].preview, "real preview");
        assert_eq!(merged[1].updated_at, "2026-07-29T10:00:00+00:00");
    }

    #[test]
    fn support_thread_policy_removes_every_execution_capability() {
        let mut body = json!({
            "features": ["usage", "tools", "agentic", "citations"],
            "tools": [{"name": "knowledge_search"}],
            "browse_web": true,
            "generate_image": true,
            "plan_mode": true,
            "deep_research": true,
            "session_key": "spoofed-session"
        });
        strip_support_thread_capabilities(body.as_object_mut().expect("object"), "support_thread");

        assert_eq!(body["features"], json!(["usage", "citations"]));
        assert_eq!(body["tools"], Value::Array(vec![]));
        assert_eq!(body["browse_web"], false);
        assert_eq!(body["generate_image"], false);
        assert_eq!(body["plan_mode"], false);
        assert_eq!(body["deep_research"], false);
        assert_eq!(body["session_key"], "support_thread");
    }

    #[test]
    fn support_namespace_is_immutable_and_does_not_depend_on_cached_metadata() {
        let mut later_turn = json!({
            "thread_id": "support_durable-thread",
            "features": ["tools", "agentic"],
            "tools": [{"name": "dangerous_action"}],
            "browse_web": true
        });
        enforce_support_thread_policy(&mut later_turn).expect("support namespace is valid");
        assert_eq!(later_turn["features"], json!([]));
        assert_eq!(later_turn["tools"], json!([]));
        assert_eq!(later_turn["browse_web"], false);

        let mut invalid_initial = json!({
            "thread_id": "ordinary-thread",
            "support_read_only": true
        });
        assert!(enforce_support_thread_policy(&mut invalid_initial).is_err());
    }

    // ── Zero Data Retention gate ────────────────────────────────────────────

    fn test_user() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.invalid".to_owned(),
            user_name: "User".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: Some(crate::middleware::AuthorizedMembership {
                organization_id: "org-1".to_owned(),
                role: "member".to_owned(),
            }),
        }
    }

    fn test_scope() -> ChatHistoryScope {
        ChatHistoryScope {
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
        }
    }

    /// An org-core that answers with a given standing ZDR posture.
    async fn org_core_with_zdr(zdr: bool) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/organizations/org-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "org-1",
                "metadata": { "interactiveRetention": { "zdr": zdr } }
            })))
            .mount(&server)
            .await;
        server
    }

    async fn state_for(org_core: &MockServer) -> AppState {
        let mut state = crate::tests::test_state(false);
        state.org_core_url = org_core.uri();
        state
    }

    fn headers_with_zdr() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("x-zdr", axum::http::HeaderValue::from_static("true"));
        headers
    }

    fn save_body() -> SaveThreadRequest {
        SaveThreadRequest {
            title: Some("Kvartalstall".to_owned()),
            pinned: None,
            preview: Some("Omsetningen endte på 4,2 mrd".to_owned()),
            updated_at: None,
            turns: Some(vec![json!({ "role": "user", "content": "hemmelig" })]),
            task_steps: None,
            zdr: None,
        }
    }

    /// The SRS-1 defect, exactly: the browser's in-memory temporary-chat `Set`
    /// is gone (reloaded tab / replayed request / non-SPA client), so the save
    /// arrives looking completely ordinary. The thread's own server-side marker
    /// — written when the ZDR turn was dispatched — must still refuse it.
    #[tokio::test]
    async fn a_thread_that_ran_a_zdr_turn_stays_unpersistable_after_the_client_forgets() {
        let org_core = org_core_with_zdr(false).await;
        let state = state_for(&org_core).await;
        let user = test_user();

        // The ZDR turn goes out; the gateway records it, not the browser.
        mark_thread_zdr(&state, "org-1", "user-1", "thread-1").await;

        // A later save carries no ZDR signal whatsoever.
        assert!(
            retention_posture(
                &state,
                &user,
                &test_scope(),
                "thread-1",
                &HeaderMap::new(),
                &save_body(),
            )
            .await,
            "a thread marked ZDR server-side must stay unpersistable"
        );
        // A different thread in the same scope is unaffected.
        assert!(
            !retention_posture(
                &state,
                &user,
                &test_scope(),
                "thread-2",
                &HeaderMap::new(),
                &save_body(),
            )
            .await
        );
    }

    /// Each source alone is sufficient, and no client input can clear another's
    /// vote — the same one-way OR `claims.effective_zdr` applies upstream.
    #[tokio::test]
    async fn any_single_source_alone_establishes_the_posture() {
        let org_core = org_core_with_zdr(false).await;
        let state = state_for(&org_core).await;
        let user = test_user();

        // Header alone.
        assert!(
            retention_posture(
                &state,
                &user,
                &test_scope(),
                "t",
                &headers_with_zdr(),
                &save_body()
            )
            .await
        );
        // Body alone.
        let declared = SaveThreadRequest {
            zdr: Some(true),
            ..save_body()
        };
        assert!(
            retention_posture(
                &state,
                &user,
                &test_scope(),
                "t",
                &HeaderMap::new(),
                &declared
            )
            .await
        );
        // And a body that says `false` cannot cancel the header.
        let denied = SaveThreadRequest {
            zdr: Some(false),
            ..save_body()
        };
        assert!(
            retention_posture(
                &state,
                &user,
                &test_scope(),
                "t",
                &headers_with_zdr(),
                &denied
            )
            .await
        );
    }

    /// Org-wide ZDR is the case the SPA cannot express at all: it only knows
    /// per-thread "temporary chat". The posture has to come from org-core.
    #[tokio::test]
    async fn an_org_under_zdr_retains_nothing_even_when_the_request_says_nothing() {
        let org_core = org_core_with_zdr(true).await;
        let state = state_for(&org_core).await;
        assert!(
            retention_posture(
                &state,
                &test_user(),
                &test_scope(),
                "t",
                &HeaderMap::new(),
                &save_body(),
            )
            .await
        );
    }

    /// An org-core outage must not be a licence to retain. The two failure modes
    /// are not symmetric: guessing "not ZDR" durably stores content the org may
    /// have forbidden, guessing "ZDR" only skips a cache write.
    #[tokio::test]
    async fn an_unreachable_org_core_fails_closed() {
        let org_core = MockServer::start().await; // no mocks: every GET 404s
        let state = state_for(&org_core).await;
        assert!(
            retention_posture(
                &state,
                &test_user(),
                &test_scope(),
                "t",
                &HeaderMap::new(),
                &save_body(),
            )
            .await,
            "an unresolvable posture must be treated as Zero Data Retention"
        );
    }

    #[tokio::test]
    async fn an_ordinary_save_in_a_non_zdr_org_is_still_retained() {
        let org_core = org_core_with_zdr(false).await;
        let state = state_for(&org_core).await;
        assert!(
            !retention_posture(
                &state,
                &test_user(),
                &test_scope(),
                "t",
                &HeaderMap::new(),
                &save_body(),
            )
            .await
        );
    }

    /// A save under ZDR is a PURGE, not a no-op: a thread can acquire the
    /// posture after content was already stored (an org enables ZDR, or a branch
    /// carries a temporary thread's turns into a new id).
    #[tokio::test]
    async fn a_zdr_save_removes_content_that_was_already_stored() {
        let org_core = org_core_with_zdr(false).await;
        let state = state_for(&org_core).await;
        let scope = test_scope();

        // Pre-existing retained copy, from before the posture applied.
        write_value(
            &state,
            &transcript_key(&scope, "thread-1"),
            json!({ "threadId": "thread-1", "turns": [{"content": "hemmelig"}], "updatedAt": "2026-08-01T00:00:00Z" }),
        )
        .await;
        write_value(
            &state,
            &index_key(&scope),
            json!([{ "threadId": "thread-1", "title": "T", "preview": "hemmelig", "updatedAt": "2026-08-01T00:00:00Z" }]),
        )
        .await;
        assert!(read_transcript(&state, &scope, "thread-1").await.is_some());

        let response = save_thread(
            State(state.clone()),
            Extension(test_user()),
            Path("thread-1".to_owned()),
            headers_with_zdr(),
            Json(save_body()),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            read_transcript(&state, &scope, "thread-1").await.is_none(),
            "the earlier transcript must be gone, not merely left unupdated"
        );
        assert!(
            read_index(&state, &scope).await.is_empty(),
            "the index entry carries the last turn's text and must go too"
        );
    }

    /// Turning ZDR on must not leave the previous window's history readable for
    /// the rest of its 90-day TTL. The first listing under the new posture
    /// clears the Frontend Plane's copy.
    #[tokio::test]
    async fn enabling_org_zdr_clears_history_stored_before_the_posture_changed() {
        let org_core = org_core_with_zdr(true).await;
        let state = state_for(&org_core).await;
        let scope = test_scope();

        write_value(
            &state,
            &transcript_key(&scope, "t1"),
            json!({ "threadId": "t1", "turns": [{"content": "fra før"}], "updatedAt": "2026-08-01T00:00:00Z" }),
        )
        .await;
        write_value(
            &state,
            &index_key(&scope),
            json!([{ "threadId": "t1", "title": "T1", "preview": "fra før", "updatedAt": "2026-08-01T00:00:00Z" }]),
        )
        .await;

        let response = list_threads(
            State(state.clone()),
            Extension(test_user()),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        assert!(read_index(&state, &scope).await.is_empty());
        assert!(read_transcript(&state, &scope, "t1").await.is_none());
    }

    /// SRS-3: after erasure the owning plane deletes the conversation and
    /// reports success — a complete copy must not keep living here.
    #[tokio::test]
    async fn purging_a_subject_removes_the_index_and_every_transcript() {
        let org_core = org_core_with_zdr(false).await;
        let state = state_for(&org_core).await;
        let scope = test_scope();

        for thread in ["t1", "t2"] {
            write_value(
                &state,
                &transcript_key(&scope, thread),
                json!({ "threadId": thread, "turns": [{"content": "x"}], "updatedAt": "2026-08-01T00:00:00Z" }),
            )
            .await;
        }
        write_value(
            &state,
            &index_key(&scope),
            json!([
                { "threadId": "t1", "title": "T1", "preview": "p", "updatedAt": "2026-08-01T00:00:00Z" },
                { "threadId": "t2", "title": "T2", "preview": "p", "updatedAt": "2026-08-02T00:00:00Z" }
            ]),
        )
        .await;
        mark_thread_zdr(&state, "org-1", "user-1", "t1").await;

        let removed = purge_user_history(&state, "org-1", "user-1", PurgeScope::Everything).await;

        assert_eq!(removed, 2);
        assert!(read_index(&state, &scope).await.is_empty());
        assert!(read_transcript(&state, &scope, "t1").await.is_none());
        assert!(read_transcript(&state, &scope, "t2").await.is_none());
        // Erasure clears the marker too: it is itself a record that this person
        // held a conversation under that id, and there is no subject left.
        assert!(!thread_is_zdr(&state, &scope, "t1").await);
    }

    /// The roster is the ONLY enumeration an organisation erasure has: hashed
    /// cache keys mean there is no `SCAN chat-history:index:{org}:*`. It must
    /// therefore be populated by the same path that creates the history, and
    /// must hold user ids only — never conversation content.
    #[test]
    fn the_org_roster_key_is_derivable_from_an_org_id_alone() {
        // The erasure consumer receives an org id and nothing else, so this key
        // must not depend on a prior read or on `cache_key`'s hashing.
        assert_eq!(
            org_roster_key("org-1"),
            "verevon:gw:chat-history:roster:org-1"
        );
        assert_ne!(org_roster_key("org-1"), org_roster_key("org-2"));
    }

    /// A posture-driven purge must NOT forget why the thread was protected —
    /// otherwise the very next save re-creates what was just removed.
    #[tokio::test]
    async fn a_content_only_purge_keeps_the_zdr_markers() {
        let org_core = org_core_with_zdr(false).await;
        let state = state_for(&org_core).await;
        let scope = test_scope();

        write_value(
            &state,
            &transcript_key(&scope, "t1"),
            json!({ "threadId": "t1", "turns": [], "updatedAt": "2026-08-01T00:00:00Z" }),
        )
        .await;
        write_value(
            &state,
            &index_key(&scope),
            json!([{ "threadId": "t1", "title": "T1", "preview": "p", "updatedAt": "2026-08-01T00:00:00Z" }]),
        )
        .await;
        mark_thread_zdr(&state, "org-1", "user-1", "t1").await;

        purge_user_history(&state, "org-1", "user-1", PurgeScope::ContentOnly).await;

        assert!(read_transcript(&state, &scope, "t1").await.is_none());
        assert!(
            thread_is_zdr(&state, &scope, "t1").await,
            "the marker must survive so a later save is still refused"
        );
    }

    /// The marker is written from the NORMALIZED body — the posture actually
    /// forwarded to the Model Plane — and only when there is a thread to mark.
    #[tokio::test]
    async fn only_a_zdr_turn_with_a_thread_id_is_marked() {
        let org_core = org_core_with_zdr(false).await;
        let state = state_for(&org_core).await;
        let user = test_user();
        let scope = test_scope();

        shared::record_zdr_thread(&state, &user, "org-1", &json!({ "thread_id": "plain" })).await;
        assert!(!thread_is_zdr(&state, &scope, "plain").await);

        shared::record_zdr_thread(
            &state,
            &user,
            "org-1",
            &json!({ "thread_id": "temp", "zdr": true }),
        )
        .await;
        assert!(thread_is_zdr(&state, &scope, "temp").await);

        // No thread id: nothing to key a marker on, and nothing blows up.
        shared::record_zdr_thread(&state, &user, "org-1", &json!({ "zdr": true })).await;
    }
}
