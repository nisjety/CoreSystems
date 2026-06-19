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
    let updated_at = normalize_timestamp(body.updated_at.as_deref(), &now);
    let session = ChatThreadSummary {
        thread_id: thread_id.clone(),
        title: normalize_display_text(
            body.title
                .as_deref()
                .or(existing.as_ref().map(|item| item.title.as_str())),
            "Velion Chat",
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
    };

    let mut sessions = read_index(&state, &scope).await;
    sessions.retain(|item| item.thread_id != thread_id);
    sessions.push(session.clone());
    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
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
    let url = format!("{}/v1/threads?limit={MAX_THREADS}", state.model_gateway_url);
    let response = state
        .client
        .get(url)
        .bearer_auth(token)
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
        title: normalize_display_text(Some(&item.title), "Velion Chat", MAX_TITLE_LEN),
        preview: normalize_display_text(Some(&item.preview), "", MAX_PREVIEW_LEN),
        updated_at,
    })
}

fn merge_thread_indexes(
    cached: Vec<ChatThreadSummary>,
    durable: Vec<ChatThreadSummary>,
) -> Vec<ChatThreadSummary> {
    let mut sessions: Vec<ChatThreadSummary> = Vec::new();
    for item in durable.into_iter().chain(cached) {
        if item.thread_id.trim().is_empty() {
            continue;
        }
        if let Some(existing) = sessions
            .iter_mut()
            .find(|existing| existing.thread_id == item.thread_id)
        {
            if existing.title.trim().is_empty() || existing.title == "Velion Chat" {
                existing.title = item.title;
            }
            if existing.preview.trim().is_empty() {
                existing.preview = item.preview;
            }
            if item.updated_at > existing.updated_at {
                existing.updated_at = item.updated_at;
            }
        } else {
            sessions.push(item);
        }
    }
    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
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
