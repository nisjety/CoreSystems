use axum::{
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    config::AppState,
    envelope::{error, ok},
    middleware::AuthenticatedUser,
    upstream::authorized_org_id,
};

use super::shared;

const MAX_THREADS: usize = 80;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChatThreadSummary {
    thread_id: String,
    title: String,
    preview: String,
    updated_at: String,
    /// Session Core owns pin state and returns this field with the durable
    /// thread summary. The gateway only relays that authority to the SPA.
    #[serde(default)]
    pinned: bool,
    /// A non-secret durable routing reference. It lets the client request a
    /// fresh append decision after reload; it is never an audience list or
    /// bearer and Control still resolves current authority.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    space_ref: String,
    /// Which surface created this thread -- see session-core's
    /// `ThreadSummary.origin`. Exposed so a future client affordance (a
    /// "this came from Support" badge, say) does not need a new field; today
    /// nothing on the client reads it, only `chat_history_sessions` below does.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    origin: String,
    /// Session Core's newest run projection. The Chat surface uses this only
    /// for a quiet history-rail status hint; the run endpoint remains the
    /// authority for detailed state and actions.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    latest_run_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    latest_run_status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    latest_run_updated_at: Option<String>,
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
pub(crate) struct SaveThreadRequest {
    #[serde(default)]
    pub(crate) title: Option<String>,
    /// Absent means "leave the pin as it is", which is why this is an Option
    /// rather than a bool: the SPA saves a thread on every turn to refresh the
    /// title and preview, and a bare `false` default would silently unpin on
    /// the next message.
    #[serde(default)]
    pub(crate) pinned: Option<bool>,
    #[serde(default)]
    pub(crate) preview: Option<String>,
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
    #[serde(default)]
    pinned: bool,
    #[serde(default, alias = "spaceId")]
    space_id: String,
    #[serde(default)]
    origin: String,
    #[serde(default, alias = "latestRunId")]
    latest_run_id: String,
    #[serde(default, alias = "latestRunStatus")]
    latest_run_status: String,
    #[serde(default, alias = "latestRunUpdatedAt")]
    latest_run_updated_at: Option<String>,
}

pub(super) async fn list_threads(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = scope_for(&state, &user).await {
        return response;
    }
    let sessions = match read_durable_threads(&state, &user, &headers).await {
        Ok(sessions) => sessions,
        Err(response) => return response,
    };
    Json(ok(ThreadsResponse {
        sessions: chat_history_sessions(sessions),
    }))
    .into_response()
}

/// Space-scoped conversations are the room's shared record and render in the
/// room's own timeline; Support-assist and the Agent Run Console mint their own
/// unscoped threads that must not appear here either. Chat's history lists
/// only threads Chat itself created. Filtered at the LISTING and not in
/// `read_durable_threads`: that read also authorizes transcript fetches, which
/// is exactly how the room timeline reads its posts' content.
///
/// This used to filter on `space_ref.trim().is_empty()` -- which distinguishes
/// Space-scoped threads from everything else, but says nothing about *why* an
/// unscoped thread exists. That let Agent Run Console and Support-assist
/// threads (both unscoped, since they invoke with no thread/session key) leak
/// into this list. `origin` is a declared fact set once at creation, not an
/// inference from an unrelated column, and closes both leaks at once.
///
/// A blank `origin` is treated as non-chat (excluded) rather than as "unknown,
/// assume chat": the upstream request already asked for `origin=chat`, so a
/// blank value here means the field did not survive the round trip, and
/// failing closed is safer than including a thread this filter cannot
/// classify.
fn chat_history_sessions(sessions: Vec<ChatThreadSummary>) -> Vec<ChatThreadSummary> {
    sessions
        .into_iter()
        .filter(|session| session.origin == "chat")
        .collect()
}

pub(super) async fn get_thread_transcript(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
) -> Response {
    if let Err(response) = scope_for(&state, &user).await {
        return response;
    }
    let thread_id = normalize_id(&thread_id);
    if thread_id.is_empty() {
        return bad_request("thread_id is required.");
    }
    let threads = match read_durable_threads(&state, &user, &headers).await {
        Ok(threads) => threads,
        Err(response) => return response,
    };
    let Some(session) = threads.iter().find(|thread| thread.thread_id == thread_id) else {
        return thread_not_found();
    };
    let transcript = match read_canonical_transcript(&state, &user, &headers, &thread_id).await {
        Ok(transcript) => transcript.map(|mut transcript| {
            transcript.updated_at = session.updated_at.clone();
            transcript
        }),
        Err(response) => return response,
    };
    Json(ok(TranscriptResponse { transcript })).into_response()
}

pub(crate) async fn save_thread(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
    Json(body): Json<SaveThreadRequest>,
) -> Response {
    if let Err(response) = scope_for(&state, &user).await {
        return response;
    }
    let thread_id = normalize_id(&thread_id);
    if thread_id.is_empty() {
        return bad_request("thread_id is required.");
    }

    if body.title.is_some() || body.preview.is_some() || body.pinned.is_some() {
        if let Err(response) = update_durable_presentation(
            &state,
            &user,
            &headers,
            &thread_id,
            body.title.as_deref(),
            body.preview.as_deref(),
            body.pinned,
        )
        .await
        {
            return response;
        }
    }
    let sessions = match read_durable_threads(&state, &user, &headers).await {
        Ok(sessions) => sessions,
        Err(response) => return response,
    };
    let Some(session) = sessions
        .iter()
        .find(|item| item.thread_id == thread_id)
        .cloned()
    else {
        return thread_not_found();
    };

    Json(ok(ThreadResponse {
        session,
        // Turns and task steps are intentionally not accepted as durable BFF
        // state. Session Core / Model Gateway is the sole conversation owner;
        // the browser may keep its own rendering cache for offline UX.
        transcript: None,
    }))
    .into_response()
}

pub(crate) async fn delete_thread(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
) -> Response {
    if let Err(response) = scope_for(&state, &user).await {
        return response;
    }
    let thread_id = normalize_id(&thread_id);
    if thread_id.is_empty() {
        return bad_request("thread_id is required.");
    }
    if let Err(response) = delete_durable_thread(&state, &user, &headers, &thread_id).await {
        return response;
    }
    let sessions = match read_durable_threads(&state, &user, &headers).await {
        Ok(sessions) => sessions,
        Err(response) => return response,
    };
    Json(ok(ThreadsResponse { sessions })).into_response()
}

pub(crate) async fn clear_threads(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = scope_for(&state, &user).await {
        return response;
    }
    if let Err(response) = delete_durable_threads(&state, &user, &headers).await {
        return response;
    }
    Json(ok(ThreadsResponse { sessions: vec![] })).into_response()
}

async fn scope_for(state: &AppState, user: &AuthenticatedUser) -> Result<(), Response> {
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
    Ok(())
}

async fn read_durable_threads(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
) -> Result<Vec<ChatThreadSummary>, Response> {
    let token = shared::model_token(state, user, headers).await;
    let session_token = match shared::required_session_token(state, user, headers).await {
        Ok(token) => token,
        Err(error) => return Err(shared::delegated_auth_unavailable(error).into_response()),
    };
    // origin=chat is requested here AND re-checked in chat_history_sessions
    // below: the request-side filter is the efficient path, the response-side
    // check is what actually enforces the invariant if session-core or
    // model-gateway ever ignores the query parameter -- the same
    // belt-and-braces shape this codebase already uses for ZDR.
    let url = format!(
        "{}/v1/threads?limit={MAX_THREADS}&origin=chat",
        state.model_gateway_url
    );
    let (status, Json(payload)) = shared::proxy_model_json_with_session(
        state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        Some(&session_token),
        user,
    )
    .await;
    if !status.is_success() {
        return Err((status, Json(payload)).into_response());
    }
    let payload = serde_json::from_value::<DurableThreadsResponse>(payload).map_err(|_| {
        (
            StatusCode::BAD_GATEWAY,
            Json(error(
                "invalid_model_gateway_response",
                "Model Gateway returned an invalid thread listing.",
            )),
        )
            .into_response()
    })?;
    let now = now_iso();
    Ok(payload
        .threads
        .into_iter()
        .filter_map(|item| durable_to_summary(item, &now))
        .collect())
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
        // Presentation text is validated and normalized by Session Core. The
        // BFF intentionally does not trim, truncate, or replace it here.
        title: item.title,
        preview: item.preview,
        updated_at,
        pinned: item.pinned,
        space_ref: item.space_id.trim().to_owned(),
        origin: item.origin.trim().to_owned(),
        latest_run_id: item.latest_run_id,
        latest_run_status: item.latest_run_status,
        latest_run_updated_at: item.latest_run_updated_at,
    })
}

/// `pub(crate)`: the Space room's own presentation route reuses this rather
/// than duplicating the Model Gateway call. Session Core keeps it owner-bound,
/// so a room member can only retitle or pin a post they started.
pub(crate) async fn update_durable_presentation(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    thread_id: &str,
    title: Option<&str>,
    preview: Option<&str>,
    pinned: Option<bool>,
) -> Result<(), Response> {
    let token = shared::model_token(state, user, headers).await;
    let session_token = shared::required_session_token(state, user, headers)
        .await
        .map_err(|error| shared::delegated_auth_unavailable(error).into_response())?;
    let url = format!(
        "{}/v1/threads/{}/presentation",
        state.model_gateway_url,
        urlencoding::encode(thread_id),
    );
    let (status, Json(payload)) = shared::proxy_model_json_with_session(
        state,
        Method::POST,
        &url,
        Some(json!({
            "title": title,
            "preview": preview,
            "pinned": pinned,
        })),
        token.as_deref(),
        Some(&session_token),
        user,
    )
    .await;
    if status.is_success() {
        Ok(())
    } else {
        Err((status, Json(payload)).into_response())
    }
}

/// Permanently erase one thread via Model Gateway `DELETE /v1/threads/{id}`,
/// which is owner-bound and backed by Session Core's `DeleteThread`.
///
/// NOT the archive route. `POST /v1/threads/{id}/archive` also exists and is
/// non-destructive; this one removes the thread, its messages, run
/// descendants, events/audit evidence, plans, tasks, approvals and
/// continuation records, and does not come back.
async fn delete_durable_thread(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    thread_id: &str,
) -> Result<(), Response> {
    let token = shared::model_token(state, user, headers).await;
    let session_token = shared::required_session_token(state, user, headers)
        .await
        .map_err(|error| shared::delegated_auth_unavailable(error).into_response())?;
    let url = format!(
        "{}/v1/threads/{}",
        state.model_gateway_url,
        urlencoding::encode(thread_id),
    );
    let (status, Json(payload)) = shared::proxy_model_json_with_session(
        state,
        Method::DELETE,
        &url,
        None,
        token.as_deref(),
        Some(&session_token),
        user,
    )
    .await;
    if status.is_success() {
        Ok(())
    } else {
        Err((status, Json(payload)).into_response())
    }
}

/// Permanently erase every thread the caller owns via Model Gateway
/// `DELETE /v1/threads`. See [`delete_durable_thread`] — this is the
/// destructive operation, not `POST /v1/threads/archive`.
async fn delete_durable_threads(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
) -> Result<(), Response> {
    let token = shared::model_token(state, user, headers).await;
    let session_token = shared::required_session_token(state, user, headers)
        .await
        .map_err(|error| shared::delegated_auth_unavailable(error).into_response())?;
    let url = format!("{}/v1/threads", state.model_gateway_url);
    let (status, Json(payload)) = shared::proxy_model_json_with_session(
        state,
        Method::DELETE,
        &url,
        None,
        token.as_deref(),
        Some(&session_token),
        user,
    )
    .await;
    if status.is_success() {
        Ok(())
    } else {
        Err((status, Json(payload)).into_response())
    }
}

fn thread_not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(error(
            "thread_not_found",
            "The chat thread no longer exists.",
        )),
    )
        .into_response()
}

#[derive(Debug, Deserialize)]
struct CanonicalMessagesResponse {
    #[serde(default)]
    messages: Vec<CanonicalMessage>,
}

#[derive(Debug, Deserialize)]
struct CanonicalMessage {
    /// Durable message id from session-core, relayed by Model Gateway.
    ///
    /// Empty for rows written before the conversation read returned ids; the
    /// positional fallback below covers those. Message pinning sends this id
    /// back, so a synthesised index would name a different turn as soon as the
    /// thread grew.
    #[serde(default)]
    message_id: String,
    #[serde(default)]
    role: String,
    #[serde(default)]
    content: String,
    /// The persona this turn answered as, when it had one — session-core's
    /// at-the-time record, surfaced for per-turn attribution in the room.
    #[serde(default)]
    agent_name: String,
    /// The turn's persisted evidence, as Model Gateway flattens it onto the
    /// message. Relayed verbatim so a reopened thread shows the same sources
    /// the live stream did; before this existed the read path returned
    /// role/content only and every evidence surface was empty on any device
    /// but the one that streamed the turn.
    ///
    /// Two keys because the product has two evidence sources: `grounding` is
    /// Data Plane retrieval, `citations` is what the tool loop cited (web
    /// search, deep research). A turn can carry either, both, or neither.
    #[serde(default)]
    grounding: Option<Value>,
    #[serde(default)]
    citations: Option<Value>,
}

async fn read_canonical_transcript(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    thread_id: &str,
) -> Result<Option<ChatThreadTranscript>, Response> {
    let token = shared::model_token(state, user, headers).await;
    let session_token = shared::required_session_token(state, user, headers)
        .await
        .map_err(|error| shared::delegated_auth_unavailable(error).into_response())?;
    let url = format!(
        "{}/v1/threads/{}/messages",
        state.model_gateway_url,
        urlencoding::encode(thread_id),
    );
    let (status, Json(payload)) = shared::proxy_model_json_with_session(
        state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        Some(&session_token),
        user,
    )
    .await;
    if status == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !status.is_success() {
        return Err((status, Json(payload)).into_response());
    }
    let payload = serde_json::from_value::<CanonicalMessagesResponse>(payload).map_err(|_| {
        (
            StatusCode::BAD_GATEWAY,
            Json(error(
                "invalid_model_gateway_response",
                "Model Gateway returned an invalid conversation.",
            )),
        )
            .into_response()
    })?;
    Ok(canonical_messages_to_transcript(
        thread_id,
        payload.messages,
    ))
}

fn canonical_messages_to_transcript(
    thread_id: &str,
    messages: Vec<CanonicalMessage>,
) -> Option<ChatThreadTranscript> {
    if messages.is_empty() {
        return None;
    }
    let turns = messages
        .into_iter()
        .enumerate()
        .map(|(index, message)| {
            // The durable id when there is one, a positional id otherwise.
            // The fallback is deliberately still positional rather than random:
            // it keeps this response stable across reads for the same thread,
            // and an id a client cannot resolve upstream is better than one
            // that changes on every fetch.
            let id = if message.message_id.trim().is_empty() {
                format!("canonical-{}", index + 1)
            } else {
                message.message_id.clone()
            };
            let mut turn = json!({
                "id": id,
                "role": message.role,
                "content": message.content,
            });
            if !message.agent_name.trim().is_empty() {
                turn["agentName"] = json!(message.agent_name);
            }
            // Attached only when the turn actually recorded evidence, matching
            // the conditional shape used for `agentName` above: the SPA treats
            // an absent key as "nothing grounded" and a present one as real
            // evidence, so writing `null` here would be a different claim.
            if let Some(grounding) = message.grounding.filter(|value| !value.is_null()) {
                turn["grounding"] = grounding;
            }
            if let Some(citations) = message
                .citations
                .filter(|value| value.as_array().is_some_and(|list| !list.is_empty()))
            {
                turn["citations"] = citations;
            }
            turn
        })
        .collect();
    Some(ChatThreadTranscript {
        thread_id: thread_id.to_owned(),
        turns,
        task_steps: None,
        updated_at: String::new(),
    })
}

/// Makes Support-derived threads durably read-only at the same-origin trust
/// boundary. Classification is encoded in an immutable thread namespace, so
/// enforcement cannot fail open across replicas, restarts, or cache outages.
pub(crate) fn enforce_support_thread_policy(body: &mut Value) -> Result<(), &'static str> {
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
    // Pinning changes the prompt sent upstream, and a support thread's
    // history is customer-authored transcript text this surface is only ever
    // allowed to READ. Same posture as every capability above: stripped, not
    // trusted, so a support turn cannot re-weight that content upstream.
    object.insert("pinned_message_ids".to_owned(), Value::Array(vec![]));
    object.insert(
        "session_key".to_owned(),
        Value::String(thread_id.to_owned()),
    );
}

fn normalize_id(value: &str) -> String {
    value.trim().to_owned()
}

fn normalize_timestamp(value: Option<&str>, fallback: &str) -> String {
    value
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc).to_rfc3339())
        .unwrap_or_else(|| fallback.to_owned())
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
        canonical_messages_to_transcript, chat_history_sessions, durable_to_summary,
        enforce_support_thread_policy, strip_support_thread_capabilities, CanonicalMessage,
        DurableThreadSummary,
    };
    use serde_json::{json, Value};

    #[test]
    fn chat_history_excludes_space_scoped_conversations() {
        let now = "2026-08-16T12:00:00Z";
        let space_thread = durable_to_summary(
            DurableThreadSummary {
                thread_id: "thread_room".to_owned(),
                title: "@Statusagent hva er din rolle?".to_owned(),
                preview: String::new(),
                updated_at: now.to_owned(),
                created_at: now.to_owned(),
                pinned: false,
                space_id: "space_room_1".to_owned(),
                origin: "space".to_owned(),
                latest_run_id: String::new(),
                latest_run_status: String::new(),
                latest_run_updated_at: None,
            },
            now,
        )
        .expect("valid record");
        let chat_thread = durable_to_summary(
            DurableThreadSummary {
                thread_id: "thread_chat".to_owned(),
                title: "hvem er aquatiq".to_owned(),
                preview: String::new(),
                updated_at: now.to_owned(),
                created_at: now.to_owned(),
                pinned: false,
                space_id: String::new(),
                origin: "chat".to_owned(),
                latest_run_id: String::new(),
                latest_run_status: String::new(),
                latest_run_updated_at: None,
            },
            now,
        )
        .expect("valid record");

        let sessions = chat_history_sessions(vec![space_thread, chat_thread]);

        // The room's shared record belongs to the room's timeline; Chat's own
        // history must list only threads Chat itself created.
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].thread_id, "thread_chat");
    }

    /// The regression this whole change targets: an unscoped thread (empty
    /// `space_id`) minted by the Agent Run Console or Support-assist used to be
    /// indistinguishable from an ordinary chat thread by this filter, because it
    /// only ever looked at `space_id`. `origin` now carries the declared fact
    /// directly, so both leak into the sidebar until it is checked and neither
    /// does once it is.
    #[test]
    fn chat_history_excludes_unscoped_non_chat_origins() {
        let now = "2026-08-16T12:00:00Z";
        let fixture = |thread_id: &str, origin: &str| {
            durable_to_summary(
                DurableThreadSummary {
                    thread_id: thread_id.to_owned(),
                    title: "untitled".to_owned(),
                    preview: String::new(),
                    updated_at: now.to_owned(),
                    created_at: now.to_owned(),
                    pinned: false,
                    space_id: String::new(),
                    origin: origin.to_owned(),
                    latest_run_id: String::new(),
                    latest_run_status: String::new(),
                    latest_run_updated_at: None,
                },
                now,
            )
            .expect("valid record")
        };

        let agent_run_thread = fixture("thread_agent_run", "agent_run");
        let support_thread = fixture("thread_support", "support");
        let system_thread = fixture("thread_system", "system");
        let chat_thread = fixture("thread_chat", "chat");

        let sessions = chat_history_sessions(vec![
            agent_run_thread,
            support_thread,
            system_thread,
            chat_thread,
        ]);

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].thread_id, "thread_chat");
    }

    /// A blank origin must fail closed (excluded), not fail open as an assumed
    /// chat thread -- the request-side `origin=chat` filter is defense in
    /// depth, not the only check, and this is what makes that meaningful.
    #[test]
    fn chat_history_excludes_a_blank_origin_rather_than_assuming_chat() {
        let now = "2026-08-16T12:00:00Z";
        let unclassified = durable_to_summary(
            DurableThreadSummary {
                thread_id: "thread_unclassified".to_owned(),
                title: "untitled".to_owned(),
                preview: String::new(),
                updated_at: now.to_owned(),
                created_at: now.to_owned(),
                pinned: false,
                space_id: String::new(),
                origin: String::new(),
                latest_run_id: String::new(),
                latest_run_status: String::new(),
                latest_run_updated_at: None,
            },
            now,
        )
        .expect("valid record");

        assert_eq!(chat_history_sessions(vec![unclassified]).len(), 0);
    }

    #[test]
    fn durable_thread_presentation_is_returned_without_a_gateway_override() {
        let summary = durable_to_summary(
            DurableThreadSummary {
                thread_id: "thread_1".to_owned(),
                title: "Customer shipping follow-up".to_owned(),
                preview: "Waiting on the carrier receipt".to_owned(),
                updated_at: "2026-08-11T10:00:00Z".to_owned(),
                created_at: "2026-08-10T10:00:00Z".to_owned(),
                pinned: true,
                space_id: "space_1".to_owned(),
                origin: "space".to_owned(),
                latest_run_id: "run_1".to_owned(),
                latest_run_status: "awaiting_approval".to_owned(),
                latest_run_updated_at: Some("2026-08-11T11:00:00Z".to_owned()),
            },
            "2026-08-11T12:00:00Z",
        )
        .expect("valid Session Core record");

        assert_eq!(summary.title, "Customer shipping follow-up");
        assert_eq!(summary.preview, "Waiting on the carrier receipt");
        assert!(summary.pinned);
        assert_eq!(summary.space_ref, "space_1");
        assert_eq!(summary.origin, "space");
        assert_eq!(summary.latest_run_id, "run_1");
        assert_eq!(summary.latest_run_status, "awaiting_approval");
        assert_eq!(
            summary.latest_run_updated_at.as_deref(),
            Some("2026-08-11T11:00:00Z")
        );
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
            "pinned_message_ids": ["m1", "m2"],
            "session_key": "spoofed-session"
        });
        strip_support_thread_capabilities(body.as_object_mut().expect("object"), "support_thread");

        assert_eq!(body["features"], json!(["usage", "citations"]));
        assert_eq!(body["tools"], Value::Array(vec![]));
        assert_eq!(body["browse_web"], false);
        assert_eq!(body["generate_image"], false);
        assert_eq!(body["plan_mode"], false);
        assert_eq!(body["deep_research"], false);
        assert_eq!(body["pinned_message_ids"], serde_json::json!([]));
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

    /// Message pinning sends an id back, so the id a client receives has to be
    /// the durable one whenever session-core has it. A positional index names a
    /// different turn as soon as the thread grows, which would silently pin the
    /// wrong message.
    #[test]
    fn durable_message_ids_are_relayed_and_fall_back_positionally() {
        let transcript = canonical_messages_to_transcript(
            "thread-1",
            vec![
                CanonicalMessage {
                    message_id: "01JABCDEF".into(),
                    role: "user".into(),
                    content: "Question".into(),
                    agent_name: String::new(),
                    grounding: None,
                    citations: None,
                },
                CanonicalMessage {
                    // A row written before the conversation read returned ids.
                    message_id: "   ".into(),
                    role: "assistant".into(),
                    content: "Answer".into(),
                    agent_name: String::new(),
                    grounding: None,
                    citations: None,
                },
            ],
        )
        .expect("two messages make a transcript");

        assert_eq!(transcript.turns[0]["id"], "01JABCDEF");
        assert_eq!(transcript.turns[1]["id"], "canonical-2");
    }

    #[test]
    fn canonical_messages_are_adapted_without_task_steps_or_bff_metadata() {
        let transcript = canonical_messages_to_transcript(
            "thread-1",
            vec![
                CanonicalMessage {
                    message_id: String::new(),
                    role: "user".into(),
                    content: "Question".into(),
                    agent_name: String::new(),
                    grounding: None,
                    citations: None,
                },
                CanonicalMessage {
                    message_id: String::new(),
                    role: "assistant".into(),
                    content: "Answer".into(),
                    agent_name: "Statusagent".into(),
                    grounding: None,
                    citations: None,
                },
            ],
        )
        .expect("canonical conversation should render");

        assert_eq!(transcript.thread_id, "thread-1");
        assert_eq!(transcript.turns[0]["role"], "user");
        assert_eq!(transcript.turns[1]["content"], "Answer");
        // Per-turn persona attribution passes through; a turn without one
        // carries no agentName key at all rather than an empty string.
        assert_eq!(transcript.turns[1]["agentName"], "Statusagent");
        assert!(transcript.turns[0].get("agentName").is_none());
        assert!(transcript.task_steps.is_none());
        assert!(canonical_messages_to_transcript("thread-1", Vec::new()).is_none());
    }

    /// The regression this closes: history used to return role/content only, so
    /// a reopened thread had no sources on any device but the one that streamed
    /// it. Grounding must now survive the canonical read — and stay absent, not
    /// null, for turns that grounded nothing.
    #[test]
    fn canonical_messages_carry_persisted_grounding() {
        let transcript = canonical_messages_to_transcript(
            "thread-1",
            vec![
                CanonicalMessage {
                    message_id: String::new(),
                    role: "user".into(),
                    content: "Which pram is best?".into(),
                    agent_name: String::new(),
                    grounding: None,
                    citations: None,
                },
                CanonicalMessage {
                    message_id: String::new(),
                    role: "assistant".into(),
                    content: "The Nuna TRIV LX.".into(),
                    agent_name: String::new(),
                    grounding: Some(json!({
                        "mode": "hybrid",
                        "source_count": 1,
                        "citations": [{ "id": "c1", "title": "Nuna TRIV LX", "url": "https://example.test" }],
                    })),
                    citations: None,
                },
                CanonicalMessage {
                    message_id: String::new(),
                    role: "assistant".into(),
                    content: "Ungrounded reply".into(),
                    agent_name: String::new(),
                    grounding: Some(Value::Null),
                    citations: None,
                },
            ],
        )
        .expect("canonical conversation should render");

        assert_eq!(transcript.turns[1]["grounding"]["mode"], "hybrid");
        assert_eq!(transcript.turns[1]["grounding"]["citations"][0]["id"], "c1");
        // A turn that grounded nothing must not gain the key at all: the SPA
        // reads its presence as "this turn has evidence".
        assert!(transcript.turns[0].get("grounding").is_none());
        assert!(transcript.turns[2].get("grounding").is_none());
    }

    /// Tool-loop evidence (web search, deep research) never appears in
    /// `grounding` — it arrives as a separate `citations` list. This is the
    /// common case in practice, so it gets its own coverage.
    #[test]
    fn canonical_messages_carry_tool_loop_citations() {
        let transcript = canonical_messages_to_transcript(
            "thread-1",
            vec![
                CanonicalMessage {
                    message_id: String::new(),
                    role: "assistant".into(),
                    content: "Oslo.".into(),
                    agent_name: String::new(),
                    grounding: None,
                    citations: Some(json!([
                        { "id": "w1", "title": "Oslo", "url": "https://example.test/oslo", "snippet": "capital" },
                    ])),
                },
                CanonicalMessage {
                    message_id: String::new(),
                    role: "assistant".into(),
                    content: "No sources used.".into(),
                    agent_name: String::new(),
                    grounding: None,
                    // An empty list is not evidence; it must not create the key.
                    citations: Some(json!([])),
                },
            ],
        )
        .expect("canonical conversation should render");

        assert_eq!(
            transcript.turns[0]["citations"][0]["url"],
            "https://example.test/oslo"
        );
        assert!(transcript.turns[0].get("grounding").is_none());
        assert!(transcript.turns[1].get("citations").is_none());
    }
}
