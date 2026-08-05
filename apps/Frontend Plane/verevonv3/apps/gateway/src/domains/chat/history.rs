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
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThreadsResponse {
    sessions: Vec<ChatThreadSummary>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThreadResponse {
    session: ChatThreadSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    transcript: Option<ChatThreadTranscript>,
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
    let cached = read_index(&state, &scope).await;
    let durable = read_durable_threads(&state, &user, &scope, &headers).await;
    let sessions = merge_thread_indexes(cached, durable);
    Json(ok(ThreadsResponse { sessions })).into_response()
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
    let transcript = read_transcript(&state, &scope, &thread_id).await;
    Json(ok(TranscriptResponse { transcript })).into_response()
}

pub(super) async fn save_thread(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(thread_id): Path<String>,
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
    }))
    .into_response()
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
    Json(ok(ThreadsResponse { sessions })).into_response()
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
    Json(ok(ThreadsResponse { sessions: vec![] })).into_response()
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

fn transcript_key(scope: &ChatHistoryScope, thread_id: &str) -> String {
    format!(
        "chat-history:transcript:{}:{}:{}",
        scope.org_id, scope.user_id, thread_id
    )
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
    use super::{
        enforce_support_thread_policy, merge_thread_indexes, sort_pinned_first,
        strip_support_thread_capabilities, ChatThreadSummary, MAX_THREADS,
    };
    use serde_json::{json, Value};

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
}
