use axum::{
    extract::{Extension, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use reqwest::{Method, RequestBuilder};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    config::AppState,
    envelope::{error, ok, ok_with_cursor},
    middleware::AuthenticatedUser,
    upstream::authorized_org_id,
};

use super::shared;

/// One page of the chat list. Session Core pages on a keyset cursor, so the
/// list has no cap that also decides which threads can be opened: a thread past
/// the first page is read by id.
const THREAD_PAGE_SIZE: usize = 50;

/// Persona spec: absence of `persona_enabled` on the wire (a legacy thread
/// row from before the column existed, or a durable-thread response a caller
/// built by hand) must read as "on", the same "unset != off" convention
/// `pinned`'s tri-state uses at the request layer.
fn default_true() -> bool {
    true
}

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
    /// Persona spec: the per-thread override of the global persona switch.
    /// Session Core owns this column the same way it owns `pinned` above;
    /// absent on the wire (a pre-persona thread row) reads as enabled.
    #[serde(default = "default_true")]
    persona_enabled: bool,
    /// Session Core's newest run projection. The Chat surface uses this only
    /// for a quiet history-rail status hint; the run endpoint remains the
    /// authority for detailed state and actions.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    latest_run_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    latest_run_status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    latest_run_updated_at: Option<String>,
    /// Set when the owner archived the thread. The chat list hides archived
    /// threads by default; the archive surface reads this to label them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    archived_at: Option<String>,
    /// Session Core owns star state and returns it with the thread summary, as it
    /// owns `pinned`. The Starred filter and the star control read it.
    #[serde(default)]
    starred: bool,
    /// Whether the thread holds a reply the owner has not read (build plan DEC-11).
    /// Session Core owns it; the sidebar dot reads it.
    #[serde(default)]
    unread: bool,
    /// The derived attention state (WS16 S1), as its wire name. Absent while the attention
    /// switch is off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    attention_state: Option<String>,
    /// When the attention state began. Absent when the state has no start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    attention_since: Option<String>,
    /// Whether the thread has an unread reply. Present only with an attention state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    attention_unseen: Option<bool>,
    /// The thread this one was forked from, when it is a fork (build plan WS17 S6).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    forked_from_thread_id: Option<String>,
    /// The status of the thread's latest goal (EP-7 S5), relayed from Model Gateway: one closed
    /// token. Absent while the thread has no goal or the goal or attention switch is off, so the
    /// sidebar chip needs no second call.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    goal_status: String,
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
    /// Same "omit = leave unchanged" contract as `pinned` above — the
    /// per-thread persona override (persona spec).
    #[serde(default)]
    pub(crate) persona_enabled: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThreadsResponse {
    sessions: Vec<ChatThreadSummary>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeleteThreadResponse {
    deleted: String,
    sessions: Vec<ChatThreadSummary>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThreadPageResponse {
    sessions: Vec<ChatThreadSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ThreadListQuery {
    cursor: Option<String>,
    limit: Option<usize>,
    archived: Option<String>,
    starred_only: Option<bool>,
    /// One named agent's threads (EP-6 P3). Forwarded verbatim as a query value to Model Gateway,
    /// which validates it and reads it only while its own switch is on. Never an org or user scope.
    agent: Option<String>,
}

/// What one ListThreads call asks Model Gateway for. `limit` is the page size.
struct ThreadListRequest<'a> {
    cursor: Option<&'a str>,
    limit: usize,
    archived: Option<&'a str>,
    thread_id: Option<&'a str>,
    starred_only: bool,
    agent: Option<&'a str>,
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
    /// Set only when the caller asked for a page: whether older turns exist
    /// before it, so a partial transcript cannot pass for a complete one.
    #[serde(skip_serializing_if = "Option::is_none")]
    has_more: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct DurableThreadsResponse {
    #[serde(default)]
    threads: Vec<DurableThreadSummary>,
    #[serde(default)]
    next_cursor: String,
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
    #[serde(default = "default_true", alias = "personaEnabled")]
    persona_enabled: bool,
    #[serde(default, alias = "latestRunId")]
    latest_run_id: String,
    #[serde(default, alias = "latestRunStatus")]
    latest_run_status: String,
    #[serde(default, alias = "latestRunUpdatedAt")]
    latest_run_updated_at: Option<String>,
    #[serde(default, alias = "archivedAt")]
    archived_at: String,
    #[serde(default)]
    starred: bool,
    #[serde(default)]
    unread: bool,
    #[serde(default, alias = "attentionState")]
    attention_state: String,
    #[serde(default, alias = "attentionSince")]
    attention_since: String,
    #[serde(default, alias = "attentionUnseen")]
    attention_unseen: bool,
    #[serde(default, alias = "forkedFromThreadId")]
    forked_from_thread_id: String,
    #[serde(default, alias = "goalStatus")]
    goal_status: String,
}

pub(super) async fn list_threads(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Query(query): Query<ThreadListQuery>,
) -> Response {
    if let Err(response) = scope_for(&state, &user).await {
        return response;
    }
    let cursor = query
        .cursor
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let archived = query
        .archived
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let agent = query
        .agent
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let request = ThreadListRequest {
        cursor,
        limit: query
            .limit
            .unwrap_or(THREAD_PAGE_SIZE)
            .clamp(1, THREAD_PAGE_SIZE),
        archived,
        thread_id: None,
        starred_only: query.starred_only.unwrap_or(false),
        agent,
    };
    let (sessions, next_cursor) =
        match fetch_durable_threads(&state, &user, &headers, request).await {
            Ok(page) => page,
            Err(response) => return response,
        };
    let page = ThreadPageResponse {
        sessions: chat_history_sessions(sessions),
        next_cursor: (!next_cursor.is_empty()).then(|| next_cursor.clone()),
    };
    Json(ok_with_cursor(page, &next_cursor)).into_response()
}

/// One chat thread's session without its transcript. The list is paged, so
/// the SPA resolves a thread it was not shown through this route.
pub(super) async fn get_thread(
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
    match read_durable_thread(&state, &user, &headers, &thread_id).await {
        Ok(Some(session)) => Json(ok(ThreadResponse {
            session,
            transcript: None,
        }))
        .into_response(),
        Ok(None) => thread_not_found(),
        Err(response) => response,
    }
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
    Query(query): Query<ThreadMessagesPageQuery>,
) -> Response {
    if let Err(response) = scope_for(&state, &user).await {
        return response;
    }
    let thread_id = normalize_id(&thread_id);
    if thread_id.is_empty() {
        return bad_request("thread_id is required.");
    }
    let page = match query.page() {
        Ok(page) => page,
        Err(message) => return bad_request(message),
    };
    let session = match read_durable_thread(&state, &user, &headers, &thread_id).await {
        Ok(Some(session)) => session,
        Ok(None) => return thread_not_found(),
        Err(response) => return response,
    };
    let (transcript, has_more) =
        match read_canonical_transcript(&state, &user, &headers, &thread_id, page.as_ref()).await {
            Ok((transcript, has_more)) => (
                transcript.map(|mut transcript| {
                    transcript.updated_at = session.updated_at.clone();
                    transcript
                }),
                has_more,
            ),
            Err(response) => return response,
        };
    Json(ok(TranscriptResponse {
        transcript,
        has_more: page.is_some().then_some(has_more),
    }))
    .into_response()
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

    if body.title.is_some()
        || body.preview.is_some()
        || body.pinned.is_some()
        || body.persona_enabled.is_some()
    {
        if let Err(response) = update_durable_presentation(
            &state,
            &user,
            &headers,
            &thread_id,
            body.title.as_deref(),
            body.preview.as_deref(),
            body.pinned,
            body.persona_enabled,
        )
        .await
        {
            return response;
        }
    }
    let session = match read_durable_thread(&state, &user, &headers, &thread_id).await {
        Ok(Some(session)) => session,
        Ok(None) => return thread_not_found(),
        Err(response) => return response,
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

/// Star and unread state from the sidebar. The body carries the two flags only,
/// so this route cannot reach the title, preview or pin. An omitted flag is left
/// as it is; a body with neither is refused.
#[derive(Debug, Deserialize)]
pub(crate) struct ThreadFlagsRequest {
    #[serde(default)]
    starred: Option<bool>,
    #[serde(default)]
    unread: Option<bool>,
}

pub(crate) async fn update_thread_flags(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
    Json(body): Json<ThreadFlagsRequest>,
) -> Response {
    if let Err(response) = scope_for(&state, &user).await {
        return response;
    }
    let thread_id = normalize_id(&thread_id);
    if thread_id.is_empty() {
        return bad_request("thread_id is required.");
    }
    if body.starred.is_none() && body.unread.is_none() {
        return bad_request("starred or unread is required.");
    }
    if let Err(response) = update_durable_thread_flags(
        &state,
        &user,
        &headers,
        &thread_id,
        body.starred,
        body.unread,
    )
    .await
    {
        return response;
    }
    let session = match read_durable_thread(&state, &user, &headers, &thread_id).await {
        Ok(Some(session)) => session,
        Ok(None) => return thread_not_found(),
        Err(response) => return response,
    };
    Json(ok(ThreadResponse {
        session,
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
    // The first page comes back so the sidebar can redraw without a second call.
    let request = ThreadListRequest {
        cursor: None,
        limit: THREAD_PAGE_SIZE,
        archived: None,
        thread_id: None,
        starred_only: false,
        agent: None,
    };
    let (sessions, _) = match fetch_durable_threads(&state, &user, &headers, request).await {
        Ok(page) => page,
        Err(response) => return response,
    };
    Json(ok(DeleteThreadResponse {
        deleted: thread_id,
        sessions: chat_history_sessions(sessions),
    }))
    .into_response()
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

/// The body of `POST /api/v1/chat/threads/{id}/fork`: the assistant turn to copy up to, and a title.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ForkThreadBody {
    from_message_id: String,
    #[serde(default)]
    title: String,
}

/// Start a new chat thread from a copy of this one, up to an assistant turn (build plan WS17 S6).
/// Only a chat thread the caller owns can be forked here: the lookup keeps a Space or Support
/// thread out of this route. Session Core makes the copy and decides whether it may be made; the
/// answer is the new thread's session.
pub(super) async fn fork_thread(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
    Json(body): Json<ForkThreadBody>,
) -> Response {
    if let Err(response) = scope_for(&state, &user).await {
        return response;
    }
    let thread_id = normalize_id(&thread_id);
    if thread_id.is_empty() {
        return bad_request("thread_id is required.");
    }
    let from_message_id = body.from_message_id.trim().to_owned();
    if from_message_id.is_empty() {
        return bad_request("from_message_id is required.");
    }
    match read_durable_thread(&state, &user, &headers, &thread_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return thread_not_found(),
        Err(response) => return response,
    }
    let url = format!(
        "{}/v1/threads/{}/fork",
        state.model_gateway_url,
        urlencoding::encode(&thread_id),
    );
    let token = shared::model_token(&state, &user, &headers).await;
    let session_token = match shared::required_session_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return shared::delegated_auth_unavailable(error).into_response(),
    };
    let (status, Json(payload)) = shared::proxy_model_json_with_session(
        &state,
        Method::POST,
        &url,
        Some(json!({ "fromMessageId": from_message_id, "title": body.title.trim() })),
        token.as_deref(),
        Some(&session_token),
        &user,
    )
    .await;
    if !status.is_success() {
        return (status, Json(payload)).into_response();
    }
    let new_thread_id = payload["thread_id"].as_str().unwrap_or_default().trim().to_owned();
    if new_thread_id.is_empty() {
        return (
            StatusCode::BAD_GATEWAY,
            Json(error("fork_unavailable", "The fork did not name a thread.")),
        )
            .into_response();
    }
    match read_durable_thread(&state, &user, &headers, &new_thread_id).await {
        Ok(Some(session)) => Json(ok(ThreadResponse {
            session,
            transcript: None,
        }))
        .into_response(),
        Ok(None) => thread_not_found(),
        Err(response) => response,
    }
}

/// The query of `GET /api/v1/chat/threads/{thread_id}/export`: the two choices the owner made. Both
/// are optional here on purpose: Model Gateway owns the closed sets and answers a missing or
/// unknown choice with its own typed `invalid_request`, so the browser hears one authority.
#[derive(Debug, Deserialize)]
pub(super) struct ThreadExportQuery {
    format: Option<String>,
    profile: Option<String>,
}

/// Export one of the caller's own conversations (Wave 3 EP-12 S11). The BFF adds only the org scope
/// and the owner's own bearers. Model Gateway and Session Core decide whether the thread is the
/// caller's and may leave the platform: a zero-retention or waived caller, a Space thread and a
/// thread with a run kept outside retention are refused there, and that refusal reaches the browser
/// as the gateway wrote it. The file comes back unchanged.
pub(super) async fn export_thread(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
    Query(query): Query<ThreadExportQuery>,
) -> Response {
    if let Err(response) = scope_for(&state, &user).await {
        return response;
    }
    let thread_id = normalize_id(&thread_id);
    if thread_id.is_empty() {
        return bad_request("thread_id is required.");
    }
    let url = thread_export_url(&state.model_gateway_url, &thread_id, &query);
    let model_token = shared::model_token(&state, &user, &headers).await;
    let session_token = match shared::required_session_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return shared::delegated_auth_unavailable(error).into_response(),
    };
    relay_thread_export(&state, &user, &url, model_token.as_deref(), &session_token).await
}

/// The upstream export URL. Each choice the owner made is passed on encoded, and a choice the owner
/// left out is left out, so Model Gateway names the missing parameter itself.
fn thread_export_url(base: &str, thread_id: &str, query: &ThreadExportQuery) -> String {
    let mut params = Vec::new();
    if let Some(format) = &query.format {
        params.push(format!("format={}", urlencoding::encode(format)));
    }
    if let Some(profile) = &query.profile {
        params.push(format!("profile={}", urlencoding::encode(profile)));
    }
    let mut url = format!(
        "{base}/v1/threads/{}/export",
        urlencoding::encode(thread_id)
    );
    if !params.is_empty() {
        url.push('?');
        url.push_str(&params.join("&"));
    }
    url
}

/// Attach the owner's own credentials to one export call: the model bearer, the session bearer and
/// the verified user's identity. No other audience's credential rides on it, and none of the
/// client's headers are copied.
fn owner_export_request(
    request: RequestBuilder,
    user_id: &str,
    org_id: &str,
    model_token: Option<&str>,
    session_token: &str,
) -> RequestBuilder {
    let mut request = request.header("x-user-id", user_id);
    if !org_id.is_empty() {
        request = request.header("x-org-id", org_id);
    }
    if let Some(token) = model_token {
        request = request.bearer_auth(token);
    }
    if let Some(value) = shared::data_plane_authorization_value(session_token) {
        request = request.header("x-session-authorization", value);
    }
    request
}

/// One call to Model Gateway's export route, relayed as the file it is. The body is never parsed:
/// an export is not a JSON answer, so a JSON proxy would lose it. A refusal keeps its typed JSON
/// body; any other upstream failure becomes the gateway's own unavailable envelope.
async fn relay_thread_export(
    state: &AppState,
    user: &AuthenticatedUser,
    url: &str,
    model_token: Option<&str>,
    session_token: &str,
) -> Response {
    let org_id = authorized_org_id(state, user).await;
    let request = owner_export_request(
        state.client.get(url),
        &user.user_id,
        org_id.trim(),
        model_token,
        session_token,
    );
    let response = match crate::request_id::apply(request).send().await {
        Ok(response) => response,
        Err(_) => return export_unavailable(),
    };
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let disposition = response
        .headers()
        .get(header::CONTENT_DISPOSITION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = match response.bytes().await {
        Ok(body) => body,
        Err(_) => return export_unavailable(),
    };
    if !status.is_success() {
        let is_json = content_type
            .as_deref()
            .is_some_and(|value| value.starts_with("application/json"));
        return if is_json {
            (status, [(header::CONTENT_TYPE, "application/json")], body).into_response()
        } else {
            (status, Json(crate::envelope::upstream_unavailable())).into_response()
        };
    }
    (
        status,
        export_answer_headers(content_type.as_deref(), disposition.as_deref()),
        body,
    )
        .into_response()
}

fn export_unavailable() -> Response {
    (
        StatusCode::BAD_GATEWAY,
        Json(crate::envelope::upstream_unavailable()),
    )
        .into_response()
}

/// The owner's copy's headers: the media type and download name Model Gateway gave the file,
/// `nosniff` so the browser never guesses another type, and no-store, because the body is the
/// owner's conversation. Nothing else from the upstream answer is passed on.
fn export_answer_headers(content_type: Option<&str>, disposition: Option<&str>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    let media_type = content_type
        .and_then(|value| HeaderValue::from_str(value).ok())
        .unwrap_or_else(|| HeaderValue::from_static("application/octet-stream"));
    headers.insert(header::CONTENT_TYPE, media_type);
    let download = disposition
        .and_then(|value| HeaderValue::from_str(value).ok())
        .unwrap_or_else(|| HeaderValue::from_static("attachment"));
    headers.insert(header::CONTENT_DISPOSITION, download);
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers
}

/// The body of `POST /api/v1/chat/threads/import` (EP-12 S15): the export file's text and an optional title.
/// Nothing else is read, so an owner or organisation named in the body is refused rather than used.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ImportThreadBody {
    export_json: String,
    title: Option<String>,
}

/// The largest import body the gateway takes: Model Gateway's own file cap (`IMPORT_BODY_LIMIT_BYTES`). The file
/// travels inside this JSON body, so a file near the cap can meet the limit here first.
pub(super) const THREAD_IMPORT_BODY_LIMIT_BYTES: usize = 2 * 1024 * 1024;

/// The longest title an import takes. The title rides on the upstream query string, so it is kept short, as a
/// fork's title is.
const THREAD_IMPORT_TITLE_MAX_CHARS: usize = 96;

/// Make one of the caller's own export files into a new conversation (EP-12 S15). The BFF adds only the org
/// scope and the owner's own bearers, as the export does. Model Gateway refuses a zero-retention or waived caller
/// before it reads the file, and Session Core parses the file strictly and stores the turns it keeps; each refusal
/// reaches the browser as the gateway wrote it. Nothing is stored here.
pub(super) async fn import_thread(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<ImportThreadBody>,
) -> Response {
    if let Err(response) = scope_for(&state, &user).await {
        return response;
    }
    let title = body.title.as_deref().unwrap_or_default().trim();
    if title.chars().count() > THREAD_IMPORT_TITLE_MAX_CHARS {
        return bad_request("title is too long.");
    }
    let url = thread_import_url(&state.model_gateway_url, title);
    let model_token = shared::model_token(&state, &user, &headers).await;
    let session_token = match shared::required_session_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return shared::delegated_auth_unavailable(error).into_response(),
    };
    relay_thread_import(
        &state,
        &user,
        &url,
        body.export_json,
        model_token.as_deref(),
        &session_token,
    )
    .await
}

/// The upstream import URL. Model Gateway reads the file from the body and the title from the query, so the
/// title, when the owner gave one, is the only thing that rides on the query string.
fn thread_import_url(base: &str, title: &str) -> String {
    if title.is_empty() {
        format!("{base}/v1/threads/import")
    } else {
        format!(
            "{base}/v1/threads/import?title={}",
            urlencoding::encode(title)
        )
    }
}

/// One call to Model Gateway's import route under the owner's own model and session bearers, as the export
/// sends them. The file is the body, as Model Gateway reads it. Its answer is relayed with its status, a typed
/// refusal included; an answer that is not JSON becomes the gateway's own unavailable envelope.
async fn relay_thread_import(
    state: &AppState,
    user: &AuthenticatedUser,
    url: &str,
    export_json: String,
    model_token: Option<&str>,
    session_token: &str,
) -> Response {
    let org_id = authorized_org_id(state, user).await;
    let request = owner_export_request(
        state
            .client
            .post(url)
            .header(header::CONTENT_TYPE, "application/json")
            .body(export_json),
        &user.user_id,
        org_id.trim(),
        model_token,
        session_token,
    );
    let response = match crate::request_id::apply(request).send().await {
        Ok(response) => response,
        Err(_) => return export_unavailable(),
    };
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let body = match response.bytes().await {
        Ok(body) => body,
        Err(_) => return export_unavailable(),
    };
    match serde_json::from_slice::<Value>(&body) {
        Ok(answer) => (status, [(header::CACHE_CONTROL, "no-store")], Json(answer)).into_response(),
        Err(_) => export_unavailable(),
    }
}

/// The body of `PUT /api/v1/chat/threads/{id}/goal` (Wave 3 EP-7 S1). Only these fields are
/// relayed; the owner, org and thread come from the session and the path, never from the body.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SetThreadGoalBody {
    objective: String,
    #[serde(default)]
    success_criteria: Vec<String>,
    #[serde(default)]
    max_turns: Option<u32>,
    #[serde(default)]
    token_budget: Option<u64>,
    #[serde(default)]
    replace: bool,
}

/// The body of `PATCH /api/v1/chat/threads/{id}/goal`: the goal and the revision it was read at,
/// then only the fields to change (a field left out keeps its value).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct UpdateThreadGoalBody {
    goal_id: String,
    expected_revision: u64,
    #[serde(default)]
    objective: Option<String>,
    #[serde(default)]
    success_criteria: Option<Vec<String>>,
    #[serde(default)]
    max_turns: Option<u32>,
    #[serde(default)]
    token_budget: Option<u64>,
    #[serde(default)]
    status: Option<String>,
}

impl SetThreadGoalBody {
    fn upstream(self) -> Value {
        let mut body = json!({
            "objective": self.objective,
            "successCriteria": self.success_criteria,
            "replace": self.replace,
        });
        if let Some(turns) = self.max_turns {
            body["maxTurns"] = json!(turns);
        }
        if let Some(budget) = self.token_budget {
            body["tokenBudget"] = json!(budget);
        }
        body
    }
}

impl UpdateThreadGoalBody {
    /// The gateway writes exactly the fields this body carries, so absent stays absent.
    fn upstream(self) -> Value {
        let mut body = json!({
            "goalId": self.goal_id.trim(),
            "expectedRevision": self.expected_revision,
        });
        if let Some(objective) = self.objective {
            body["objective"] = json!(objective);
        }
        if let Some(criteria) = self.success_criteria {
            body["successCriteria"] = json!(criteria);
        }
        if let Some(turns) = self.max_turns {
            body["maxTurns"] = json!(turns);
        }
        if let Some(budget) = self.token_budget {
            body["tokenBudget"] = json!(budget);
        }
        if let Some(status) = self.status {
            body["status"] = json!(status);
        }
        body
    }
}

fn thread_goal_url(base: &str, thread_id: &str) -> String {
    format!("{base}/v1/threads/{}/goal", urlencoding::encode(thread_id))
}

/// Read the thread's goal (Wave 3 EP-7 S1). Model Gateway and Session Core decide who may read
/// it; the answer `{ found, goal }` and every typed refusal are relayed unchanged.
pub(super) async fn get_thread_goal(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
) -> Response {
    relay_thread_goal(&state, &user, &headers, &thread_id, Method::GET, None, false).await
}

/// Set a goal on one of the caller's own chat threads (Wave 3 EP-7 S1).
pub(super) async fn set_thread_goal(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
    Json(body): Json<SetThreadGoalBody>,
) -> Response {
    let body = body.upstream();
    relay_thread_goal(&state, &user, &headers, &thread_id, Method::PUT, Some(body), true).await
}

/// Edit, pause, resume or clear the goal of one of the caller's own chat threads (EP-7 S1).
pub(super) async fn update_thread_goal(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
    Json(body): Json<UpdateThreadGoalBody>,
) -> Response {
    if body.goal_id.trim().is_empty() {
        return bad_request("goalId is required.");
    }
    let body = body.upstream();
    relay_thread_goal(&state, &user, &headers, &thread_id, Method::PATCH, Some(body), true).await
}

/// One call to Model Gateway's goal route under the user's own model and session tokens. A write
/// first checks the thread is the caller's own chat thread, as the fork does, so a Space or Support
/// thread never reaches the store through this route. No scoping header is forwarded.
async fn relay_thread_goal(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    thread_id: &str,
    method: Method,
    body: Option<Value>,
    write: bool,
) -> Response {
    if let Err(response) = scope_for(state, user).await {
        return response;
    }
    let thread_id = normalize_id(thread_id);
    if thread_id.is_empty() {
        return bad_request("thread_id is required.");
    }
    if write {
        match read_durable_thread(state, user, headers, &thread_id).await {
            Ok(Some(_)) => {}
            Ok(None) => return thread_not_found(),
            Err(response) => return response,
        }
    }
    let url = thread_goal_url(&state.model_gateway_url, &thread_id);
    let token = shared::model_token(state, user, headers).await;
    let session_token = match shared::required_session_token(state, user, headers).await {
        Ok(token) => token,
        Err(error) => return shared::delegated_auth_unavailable(error).into_response(),
    };
    let (status, Json(payload)) = shared::proxy_model_json_with_session(
        state,
        method,
        &url,
        body,
        token.as_deref(),
        Some(&session_token),
        user,
    )
    .await;
    (status, Json(payload)).into_response()
}

/// Hide one chat thread from the ordinary list. Its transcript and audit
/// evidence stay; `unarchive_thread` brings it back.
pub(super) async fn archive_thread(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
) -> Response {
    change_archive_state(&state, &user, &headers, &thread_id, true).await
}

pub(super) async fn unarchive_thread(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
) -> Response {
    change_archive_state(&state, &user, &headers, &thread_id, false).await
}

async fn change_archive_state(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    thread_id: &str,
    archived: bool,
) -> Response {
    if let Err(response) = scope_for(state, user).await {
        return response;
    }
    let thread_id = normalize_id(thread_id);
    if thread_id.is_empty() {
        return bad_request("thread_id is required.");
    }
    // Only a chat thread the caller owns can change archive state here; the
    // lookup also keeps a Space or Support thread out of this route.
    match read_durable_thread(state, user, headers, &thread_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return thread_not_found(),
        Err(response) => return response,
    }
    let (url, body) = archive_state_call(&state.model_gateway_url, &thread_id, archived);
    let token = shared::model_token(state, user, headers).await;
    let session_token = match shared::required_session_token(state, user, headers).await {
        Ok(token) => token,
        Err(error) => return shared::delegated_auth_unavailable(error).into_response(),
    };
    let (status, Json(payload)) = shared::proxy_model_json_with_session(
        state,
        Method::POST,
        &url,
        body,
        token.as_deref(),
        Some(&session_token),
        user,
    )
    .await;
    if !status.is_success() {
        return (status, Json(payload)).into_response();
    }
    match read_durable_thread(state, user, headers, &thread_id).await {
        Ok(Some(session)) => Json(ok(ThreadResponse {
            session,
            transcript: None,
        }))
        .into_response(),
        Ok(None) => thread_not_found(),
        Err(response) => response,
    }
}

/// The Model Gateway call that changes an archive state. Archiving uses the
/// archive route, which writes Session Core's THREAD_ARCHIVED evidence event.
/// Restoring uses the presentation route, which Session Core accepts only as
/// `archived: false`.
fn archive_state_call(base: &str, thread_id: &str, archived: bool) -> (String, Option<Value>) {
    let thread_id = urlencoding::encode(thread_id);
    if archived {
        (format!("{base}/v1/threads/{thread_id}/archive"), None)
    } else {
        (
            format!("{base}/v1/threads/{thread_id}/presentation"),
            Some(json!({ "archived": false })),
        )
    }
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

/// One page of the caller's threads as Session Core orders them, plus the
/// cursor for the next page. Session Core owns the sort and the cursor; the BFF
/// relays both without re-sorting, so the order is the one the cursor expects.
async fn fetch_durable_threads(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    request: ThreadListRequest<'_>,
) -> Result<(Vec<ChatThreadSummary>, String), Response> {
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
    let url = durable_thread_list_url(&state.model_gateway_url, &request);
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
    let sessions = payload
        .threads
        .into_iter()
        .filter_map(|item| durable_to_summary(item, &now))
        .collect();
    Ok((sessions, payload.next_cursor))
}

/// The chat thread with this id, archived or not. Read by id rather than
/// searched for in a page, so a thread past the first page opens like any
/// other. `None` when the thread is not a chat thread the caller owns.
async fn read_durable_thread(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    thread_id: &str,
) -> Result<Option<ChatThreadSummary>, Response> {
    let request = ThreadListRequest {
        cursor: None,
        limit: 1,
        archived: Some("all"),
        thread_id: Some(thread_id),
        starred_only: false,
        agent: None,
    };
    let (sessions, _) = fetch_durable_threads(state, user, headers, request).await?;
    Ok(chat_history_sessions(sessions)
        .into_iter()
        .find(|session| session.thread_id == thread_id))
}

fn durable_thread_list_url(base: &str, request: &ThreadListRequest<'_>) -> String {
    let mut params = vec![format!("limit={}", request.limit), "origin=chat".to_owned()];
    if let Some(thread_id) = request.thread_id {
        params.push(format!("thread_id={}", urlencoding::encode(thread_id)));
    }
    if let Some(cursor) = request.cursor {
        params.push(format!("cursor={}", urlencoding::encode(cursor)));
    }
    if let Some(archived) = request.archived {
        params.push(format!("archived={}", urlencoding::encode(archived)));
    }
    if request.starred_only {
        params.push("starred_only=true".to_owned());
    }
    if let Some(agent) = request.agent {
        params.push(format!("agent={}", urlencoding::encode(agent)));
    }
    format!("{base}/v1/threads?{}", params.join("&"))
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
        persona_enabled: item.persona_enabled,
        latest_run_id: item.latest_run_id,
        latest_run_status: item.latest_run_status,
        latest_run_updated_at: item.latest_run_updated_at,
        archived_at: Some(item.archived_at.trim().to_owned()).filter(|value| !value.is_empty()),
        starred: item.starred,
        unread: item.unread,
        // The attention fields are present only with a state: a flag-off replica sends none.
        attention_unseen: (!item.attention_state.is_empty()).then_some(item.attention_unseen),
        forked_from_thread_id: Some(item.forked_from_thread_id.trim().to_owned())
            .filter(|value| !value.is_empty()),
        attention_state: Some(item.attention_state.trim().to_owned())
            .filter(|value| !value.is_empty()),
        attention_since: Some(item.attention_since.trim().to_owned())
            .filter(|value| !value.is_empty()),
        // A closed status token from Model Gateway, relayed as it arrived; empty while goals are off.
        goal_status: item.goal_status.trim().to_owned(),
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
    persona_enabled: Option<bool>,
) -> Result<(), Response> {
    post_durable_presentation(
        state,
        user,
        headers,
        thread_id,
        json!({
            "title": title,
            "preview": preview,
            "pinned": pinned,
            // model-gateway's `UpdateThreadPresentationBody` is
            // `#[serde(rename_all = "camelCase", deny_unknown_fields)]` —
            // this key must be camelCase, unlike the single-word fields
            // above where snake_case and camelCase happen to coincide.
            "personaEnabled": persona_enabled,
        }),
    )
    .await
}

/// Star and unread state, written through the same Session Core presentation
/// call. An omitted flag is left as it is, so this never touches the title,
/// preview or pin.
pub(crate) async fn update_durable_thread_flags(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    thread_id: &str,
    starred: Option<bool>,
    unread: Option<bool>,
) -> Result<(), Response> {
    post_durable_presentation(
        state,
        user,
        headers,
        thread_id,
        json!({ "starred": starred, "unread": unread }),
    )
    .await
}

async fn post_durable_presentation(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    thread_id: &str,
    body: Value,
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
        Some(body),
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
    // The owner's own delegated Data Plane credential, as the snapshot share routes send it. Model
    // Gateway forwards it to Session Core only, which uses it to revoke the deleted thread's
    // snapshot view grants at Control. Best-effort: without it the delete runs exactly as before.
    let data_plane_token = shared::data_plane_token(state, user, headers).await;
    let (status, Json(payload)) = shared::proxy_model_json_with_delegations(
        state,
        Method::DELETE,
        &url,
        None,
        token.as_deref(),
        data_plane_token.as_deref(),
        None,
        None,
        None,
        None,
        Some(&session_token),
        None,
        None,
        None,
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
    // Same delegation as the single delete: the owner's own credential, for Session Core's grant revoke.
    let data_plane_token = shared::data_plane_token(state, user, headers).await;
    let (status, Json(payload)) = shared::proxy_model_json_with_delegations(
        state,
        Method::DELETE,
        &url,
        None,
        token.as_deref(),
        data_plane_token.as_deref(),
        None,
        None,
        None,
        None,
        Some(&session_token),
        None,
        None,
        None,
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
    /// Set by Model Gateway on a paged read: whether older turns exist before it.
    #[serde(default)]
    has_more: bool,
}

#[derive(Debug, Deserialize)]
struct CanonicalMessage {
    #[serde(default)]
    source_scope: Option<String>,
    /// Per-turn retrieval-source filter this message was recorded under
    /// (chat-wave2 scope spec §2.5), relayed the same one-line way
    /// `source_scope` is above: Model Gateway's `ThreadMessage` flattens
    /// session-core's `messages.metadata` onto the top level, so this key
    /// arrives already flattened, never nested. Absent for every turn
    /// recorded before this field existed, or one that used `auto`.
    #[serde(default)]
    retrieval_scope: Option<String>,
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
    /// The turn's authored work product (documents, code, HTML), latest
    /// version per id, as Model Gateway persists it. Relayed for the same
    /// reason as the two above: the Resultat panel was empty on a reopened
    /// thread because the only copy lived in the browser that streamed it.
    #[serde(default)]
    artifacts: Option<Value>,
    /// The context chips a user turn was sent with (Wave 3 EP-3 S5), flattened
    /// from `messages.metadata.context_refs` like the keys above. Held as a raw
    /// value and re-emitted through `relayed_context_refs`, which keeps only
    /// `kind`, `id` and `label`: the metadata map carries gateway bookkeeping
    /// that must never reach the browser.
    #[serde(default)]
    context_refs: Option<Value>,
    /// The deep-research plan an assistant plan turn ended with (Wave 3 EP-2 S5),
    /// flattened from `messages.metadata.research_plan`. Re-emitted through
    /// `relayed_research_plan`, which keeps only the sub-queries and questions.
    #[serde(default)]
    research_plan: Option<Value>,
}

/// Most chips a turn relays: the gateway's own cap on `context_refs`.
const RELAYED_CONTEXT_REFS_MAX: usize = 20;
/// Longest chip label relayed, in characters: the gateway cuts labels at the same length.
const RELAYED_CONTEXT_REF_LABEL_MAX: usize = 120;
/// Longest chip id relayed: the gateway's own id bound.
const RELAYED_CONTEXT_REF_ID_MAX: usize = 128;
/// The chip kinds the composer can attach.
const RELAYED_CONTEXT_REF_KINDS: [&str; 5] = ["document", "thread", "space", "skill", "artifact_selection"];
/// Most sub-queries a relayed plan keeps.
const RELAYED_PLAN_SUB_QUERIES_MAX: usize = 20;
/// Longest sub-query or question relayed, in characters.
const RELAYED_PLAN_TEXT_MAX: usize = 200;
/// Most clarifying questions a relayed plan keeps: the plan card's own cap.
const RELAYED_PLAN_QUESTIONS_MAX: usize = 3;

/// The chips of a stored `context_refs` value, as `[{kind, id, label}]`, or `None` when nothing
/// valid is left. Only an array is read, only an object entry with a known kind and an id is kept,
/// and every other key on an entry (a selection range, anything the gateway added) is dropped.
fn relayed_context_refs(value: Option<&Value>) -> Option<Value> {
    let entries = value?.as_array()?;
    let chips: Vec<Value> = entries
        .iter()
        .filter_map(|entry| {
            let entry = entry.as_object()?;
            let kind = entry.get("kind")?.as_str()?;
            if !RELAYED_CONTEXT_REF_KINDS.contains(&kind) {
                return None;
            }
            let id = entry.get("id")?.as_str()?;
            if id.is_empty() || id.chars().count() > RELAYED_CONTEXT_REF_ID_MAX {
                return None;
            }
            let label: String = entry
                .get("label")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .chars()
                .take(RELAYED_CONTEXT_REF_LABEL_MAX)
                .collect();
            Some(json!({ "kind": kind, "id": id, "label": label }))
        })
        .take(RELAYED_CONTEXT_REFS_MAX)
        .collect();
    (!chips.is_empty()).then(|| Value::Array(chips))
}

/// The non-blank strings of one plan list, cut to length and to `max` entries.
fn relayed_plan_strings(value: Option<&Value>, max: usize) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .filter(|text| !text.trim().is_empty())
                .map(|text| text.chars().take(RELAYED_PLAN_TEXT_MAX).collect::<String>())
                .take(max)
                .collect()
        })
        .unwrap_or_default()
}

/// The plan of a stored `research_plan` value, as `{sub_queries, questions}`, or `None` when it is
/// not an object or holds no sub-query. Everything else on the record (its version, its caps) is
/// dropped; the card re-derives its cap itself.
fn relayed_research_plan(value: Option<&Value>) -> Option<Value> {
    let record = value?.as_object()?;
    let sub_queries = relayed_plan_strings(record.get("sub_queries"), RELAYED_PLAN_SUB_QUERIES_MAX);
    if sub_queries.is_empty() {
        return None;
    }
    let questions = relayed_plan_strings(record.get("questions"), RELAYED_PLAN_QUESTIONS_MAX);
    Some(json!({ "sub_queries": sub_queries, "questions": questions }))
}

async fn read_canonical_transcript(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    thread_id: &str,
    page: Option<&ThreadMessagesPage>,
) -> Result<(Option<ChatThreadTranscript>, bool), Response> {
    let token = shared::model_token(state, user, headers).await;
    let session_token = shared::required_session_token(state, user, headers)
        .await
        .map_err(|error| shared::delegated_auth_unavailable(error).into_response())?;
    let url = thread_messages_url(&state.model_gateway_url, thread_id, page);
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
        return Ok((None, false));
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
    let has_more = payload.has_more;
    Ok((
        canonical_messages_to_transcript(thread_id, payload.messages),
        has_more,
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
            if message.source_scope.as_deref() == Some("conversation") { turn["sourceScope"] = json!("conversation"); }
            if let Some(scope) = message.retrieval_scope.as_deref() {
                if scope == "knowledge_only" || scope == "web_only" {
                    turn["retrievalScope"] = json!(scope);
                }
            }
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
            if let Some(artifacts) = message
                .artifacts
                .filter(|value| value.as_array().is_some_and(|list| !list.is_empty()))
            {
                turn["artifacts"] = artifacts;
            }
            // Allowlisted re-emits (Wave 3 EP-3 S5 / EP-2 S5): absent unless the stored
            // value is the expected shape, and never the metadata map itself.
            if let Some(refs) = relayed_context_refs(message.context_refs.as_ref()) {
                turn["contextRefs"] = refs;
            }
            if let Some(plan) = relayed_research_plan(message.research_plan.as_ref()) {
                turn["researchPlan"] = plan;
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
    // An execute turn runs under a plan grant, and a support thread can never
    // hold one. The model gateway reads both spellings, so both are removed.
    object.remove("execute_granted");
    object.remove("executeGranted");
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

pub(super) fn bad_request(message: &'static str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(error("invalid_request", message)),
    )
        .into_response()
}

/// The most turns one page of a thread's messages may carry. Model Gateway caps
/// its own page at the same size, so a larger request is refused here rather
/// than quietly shortened.
const MAX_THREAD_MESSAGES_PAGE: u32 = 200;

/// The tail-page parameters a thread's messages read may carry (WS09 S8). Both
/// are optional: with neither, the read is the unpaged one it has always been.
#[derive(Debug, Deserialize)]
pub(super) struct ThreadMessagesPageQuery {
    limit: Option<String>,
    before: Option<String>,
}

/// A page request that has passed validation and may be forwarded.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct ThreadMessagesPage {
    limit: u32,
    before: Option<String>,
}

impl ThreadMessagesPageQuery {
    /// The page asked for, or `None` for the unpaged read. Nothing that fails
    /// here is forwarded: a limit outside 1 to 200, a `before` without a limit,
    /// and a `before` that is not a message id are all refused.
    pub(super) fn page(&self) -> Result<Option<ThreadMessagesPage>, &'static str> {
        let before = self
            .before
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let Some(raw_limit) = self.limit.as_deref() else {
            return match before {
                Some(_) => Err("before needs a limit."),
                None => Ok(None),
            };
        };
        let limit = raw_limit
            .trim()
            .parse::<u32>()
            .ok()
            .filter(|value| (1..=MAX_THREAD_MESSAGES_PAGE).contains(value))
            .ok_or("limit must be a whole number from 1 to 200.")?;
        if before.is_some_and(|value| !is_message_id(value)) {
            return Err("before must be a message id.");
        }
        Ok(Some(ThreadMessagesPage {
            limit,
            before: before.map(str::to_owned),
        }))
    }
}

/// The shape of a message id as Model Gateway accepts it in a cursor: ASCII
/// letters, digits, `-` and `_`, at most 128 bytes.
fn is_message_id(value: &str) -> bool {
    (1..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// Model Gateway's messages route for one thread. Without a page it is the URL
/// every caller has always used.
pub(super) fn thread_messages_url(
    base: &str,
    thread_id: &str,
    page: Option<&ThreadMessagesPage>,
) -> String {
    let url = format!(
        "{base}/v1/threads/{}/messages",
        urlencoding::encode(thread_id)
    );
    match page {
        None => url,
        Some(ThreadMessagesPage {
            limit,
            before: None,
        }) => format!("{url}?limit={limit}"),
        Some(ThreadMessagesPage {
            limit,
            before: Some(before),
        }) => format!("{url}?limit={limit}&before={}", urlencoding::encode(before)),
    }
}

#[cfg(test)]
mod thread_messages_url_tests {
    use super::{thread_messages_url, ThreadMessagesPage, ThreadMessagesPageQuery};

    fn query(limit: Option<&str>, before: Option<&str>) -> ThreadMessagesPageQuery {
        ThreadMessagesPageQuery {
            limit: limit.map(str::to_owned),
            before: before.map(str::to_owned),
        }
    }

    #[test]
    fn thread_messages_url_is_unchanged_without_a_page() {
        assert_eq!(
            thread_messages_url("http://mg", "thread-1", None),
            "http://mg/v1/threads/thread-1/messages"
        );
        assert_eq!(query(None, None).page(), Ok(None));
    }

    #[test]
    fn thread_messages_url_forwards_only_a_validated_page() {
        let page = query(Some(" 100 "), Some("01JABC-x_9"))
            .page()
            .expect("a valid page");
        assert_eq!(
            page,
            Some(ThreadMessagesPage {
                limit: 100,
                before: Some("01JABC-x_9".to_owned()),
            })
        );
        assert_eq!(
            thread_messages_url("http://mg", "thread-1", page.as_ref()),
            "http://mg/v1/threads/thread-1/messages?limit=100&before=01JABC-x_9"
        );
        let tail = query(Some("4"), None).page().expect("a tail page");
        assert_eq!(
            thread_messages_url("http://mg", "thread-1", tail.as_ref()),
            "http://mg/v1/threads/thread-1/messages?limit=4"
        );
    }

    #[test]
    fn thread_messages_page_refuses_what_model_gateway_would_not_serve() {
        assert!(query(Some("0"), None).page().is_err());
        assert!(query(Some("201"), None).page().is_err());
        assert!(query(Some("fifty"), None).page().is_err());
        assert!(query(None, Some("01JABC")).page().is_err());
        assert!(query(Some("10"), Some("a&limit=1")).page().is_err());
        assert!(query(Some("10"), Some("../etc")).page().is_err());
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn attention_fields_reach_the_spa_only_with_a_state() {
        let now = "2026-08-16T12:00:00Z";
        let waiting: DurableThreadSummary = serde_json::from_value(json!({
            "thread_id": "thread-1",
            "title": "Waiting",
            "updated_at": "2026-08-16T11:59:00Z",
            "attention_state": "needs_approval",
            "attention_since": "2026-08-16T11:58:00Z",
            "attention_unseen": true,
        }))
        .expect("a summary with an attention state");
        let summary = durable_to_summary(waiting, now).expect("a summary");
        let wire = serde_json::to_value(&summary).expect("json");
        assert_eq!(wire["attentionState"], "needs_approval");
        assert_eq!(wire["attentionSince"], "2026-08-16T11:58:00Z");
        assert_eq!(wire["attentionUnseen"], true);

        // A replica with the switch off sends none of the three, and the SPA sees none.
        let off: DurableThreadSummary = serde_json::from_value(json!({
            "thread_id": "thread-2",
            "title": "Switch off",
            "updated_at": "2026-08-16T11:59:00Z",
        }))
        .expect("a summary without attention");
        let summary = durable_to_summary(off, now).expect("a summary");
        let wire = serde_json::to_value(&summary).expect("json");
        assert!(wire.get("attentionState").is_none());
        assert!(wire.get("attentionSince").is_none());
        assert!(wire.get("attentionUnseen").is_none());
    }

    use super::{
        canonical_messages_to_transcript, chat_history_sessions, durable_to_summary,
        enforce_support_thread_policy, strip_support_thread_capabilities, CanonicalMessage,
        DurableThreadSummary, SaveThreadRequest,
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
                persona_enabled: true,
                space_id: "space_room_1".to_owned(),
                origin: "space".to_owned(),
                latest_run_id: String::new(),
                latest_run_status: String::new(),
                latest_run_updated_at: None,
                archived_at: String::new(),
                starred: false,
                unread: false,
                attention_state: String::new(),
                attention_since: String::new(),
                attention_unseen: false,
                forked_from_thread_id: String::new(),
                goal_status: String::new(),
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
                persona_enabled: true,
                space_id: String::new(),
                origin: "chat".to_owned(),
                latest_run_id: String::new(),
                latest_run_status: String::new(),
                latest_run_updated_at: None,
                archived_at: String::new(),
                starred: false,
                unread: false,
                attention_state: String::new(),
                attention_since: String::new(),
                attention_unseen: false,
                forked_from_thread_id: String::new(),
                goal_status: String::new(),
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
                    persona_enabled: true,
                    space_id: String::new(),
                    origin: origin.to_owned(),
                    latest_run_id: String::new(),
                    latest_run_status: String::new(),
                    latest_run_updated_at: None,
                    archived_at: String::new(),
                    starred: false,
                    unread: false,
                    attention_state: String::new(),
                    attention_since: String::new(),
                    attention_unseen: false,
                    forked_from_thread_id: String::new(),
                    goal_status: String::new(),
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
                persona_enabled: true,
                space_id: String::new(),
                origin: String::new(),
                latest_run_id: String::new(),
                latest_run_status: String::new(),
                latest_run_updated_at: None,
                archived_at: String::new(),
                starred: false,
                unread: false,
                attention_state: String::new(),
                attention_since: String::new(),
                attention_unseen: false,
                forked_from_thread_id: String::new(),
                goal_status: String::new(),
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
                persona_enabled: true,
                space_id: "space_1".to_owned(),
                origin: "space".to_owned(),
                latest_run_id: "run_1".to_owned(),
                latest_run_status: "awaiting_approval".to_owned(),
                latest_run_updated_at: Some("2026-08-11T11:00:00Z".to_owned()),
                archived_at: String::new(),
                starred: false,
                unread: false,
                attention_state: String::new(),
                attention_since: String::new(),
                attention_unseen: false,
                forked_from_thread_id: String::new(),
                goal_status: String::new(),
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

    /// Persona spec: a `PATCH .../threads/{id}` body that only sets
    /// `personaEnabled` must deserialize with `title`/`preview`/`pinned`
    /// still `None` — the same "omit = leave unchanged" contract `pinned`
    /// already has, so `save_thread`'s dirty-check picks up the write
    /// without touching the other durable fields.
    #[test]
    fn save_thread_updates_persona_enabled_without_touching_title_or_pin() {
        let body: SaveThreadRequest =
            serde_json::from_value(json!({"personaEnabled": false})).expect("valid request");

        assert_eq!(body.persona_enabled, Some(false));
        assert!(body.title.is_none());
        assert!(body.preview.is_none());
        assert!(body.pinned.is_none());
        assert!(
            body.title.is_some() || body.preview.is_some() || body.pinned.is_some() || body.persona_enabled.is_some(),
            "the dirty-check must still fire on a persona-only write"
        );
    }

    /// A thread row from before the `persona_enabled` column existed (or a
    /// Session Core response an old binary produced) carries no
    /// `personaEnabled` field at all. That must read as enabled, not
    /// disabled — the same direction `pinned`'s absence already defaults to
    /// "leave as is", and the proto-level rationale documented in the
    /// persona spec's §5 ("proto3 bool zero-value trap").
    #[test]
    fn thread_listing_reports_persona_enabled_default_true_for_legacy_threads() {
        let now = "2026-08-16T12:00:00Z";
        let legacy: DurableThreadSummary = serde_json::from_value(json!({
            "threadId": "thread_legacy",
            "title": "Before persona shipped",
            "preview": "",
            "updatedAt": now,
            "createdAt": now,
            "pinned": false,
            "spaceId": "",
            "origin": "chat",
            // no personaEnabled key at all
        }))
        .expect("valid legacy record");

        let summary = durable_to_summary(legacy, now).expect("valid record");
        assert!(summary.persona_enabled);
    }

    #[test]
    fn support_thread_policy_removes_every_execution_capability() {
        let mut body = json!({
            "features": ["usage", "tools", "agentic", "citations"],
            "tools": [{"name": "knowledge_search"}],
            "browse_web": true,
            "generate_image": true,
            "plan_mode": true,
            "execute_granted": true,
            "executeGranted": true,
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
        // An execute turn runs under a plan grant a support thread can never
        // hold; the model gateway accepts the camelCase alias, so both go.
        assert!(
            body.get("execute_granted").is_none(),
            "a support turn must not forward execute_granted"
        );
        assert!(
            body.get("executeGranted").is_none(),
            "a support turn must not forward the executeGranted alias"
        );
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
                    source_scope: None,
                    retrieval_scope: None,
                    message_id: "01JABCDEF".into(),
                    role: "user".into(),
                    content: "Question".into(),
                    agent_name: String::new(),
                    grounding: None,
                    citations: None,
                    artifacts: None,
                    context_refs: None,
                    research_plan: None,
                },
                CanonicalMessage {
                    source_scope: None,
                    retrieval_scope: None,
                    // A row written before the conversation read returned ids.
                    message_id: "   ".into(),
                    role: "assistant".into(),
                    content: "Answer".into(),
                    agent_name: String::new(),
                    grounding: None,
                    citations: None,
                    artifacts: None,
                    context_refs: None,
                    research_plan: None,
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
                    source_scope: None,
                    retrieval_scope: None,
                    message_id: String::new(),
                    role: "user".into(),
                    content: "Question".into(),
                    agent_name: String::new(),
                    grounding: None,
                    citations: None,
                    artifacts: None,
                    context_refs: None,
                    research_plan: None,
                },
                CanonicalMessage {
                    source_scope: None,
                    retrieval_scope: None,
                    message_id: String::new(),
                    role: "assistant".into(),
                    content: "Answer".into(),
                    agent_name: "Statusagent".into(),
                    grounding: None,
                    citations: None,
                    artifacts: None,
                    context_refs: None,
                    research_plan: None,
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
                    source_scope: None,
                    retrieval_scope: None,
                    message_id: String::new(),
                    role: "user".into(),
                    content: "Which pram is best?".into(),
                    agent_name: String::new(),
                    grounding: None,
                    citations: None,
                    artifacts: None,
                    context_refs: None,
                    research_plan: None,
                },
                CanonicalMessage {
                    source_scope: None,
                    retrieval_scope: None,
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
                    artifacts: None,
                    context_refs: None,
                    research_plan: None,
                },
                CanonicalMessage {
                    source_scope: None,
                    retrieval_scope: None,
                    message_id: String::new(),
                    role: "assistant".into(),
                    content: "Ungrounded reply".into(),
                    agent_name: String::new(),
                    grounding: Some(Value::Null),
                    citations: None,
                    artifacts: None,
                    context_refs: None,
                    research_plan: None,
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

    /// chat-wave2 scope spec §2.5: a turn's per-turn retrieval-source filter
    /// relays the same one-line way `source_scope` does above — present only
    /// for a known, non-`auto` value, absent (not `null`) otherwise.
    #[test]
    fn canonical_messages_carry_persisted_retrieval_scope() {
        let transcript = canonical_messages_to_transcript(
            "thread-1",
            vec![
                CanonicalMessage {
                    source_scope: None,
                    retrieval_scope: Some("knowledge_only".into()),
                    message_id: String::new(),
                    role: "user".into(),
                    content: "Hva sier retningslinjene om dette?".into(),
                    agent_name: String::new(),
                    grounding: None,
                    citations: None,
                    artifacts: None,
                    context_refs: None,
                    research_plan: None,
                },
                CanonicalMessage {
                    source_scope: None,
                    retrieval_scope: Some("web_only".into()),
                    message_id: String::new(),
                    role: "user".into(),
                    content: "Hva skjer i nyhetene i dag?".into(),
                    agent_name: String::new(),
                    grounding: None,
                    citations: None,
                    artifacts: None,
                    context_refs: None,
                    research_plan: None,
                },
                CanonicalMessage {
                    source_scope: None,
                    retrieval_scope: None,
                    message_id: String::new(),
                    role: "user".into(),
                    content: "Ordinary turn.".into(),
                    agent_name: String::new(),
                    grounding: None,
                    citations: None,
                    artifacts: None,
                    context_refs: None,
                    research_plan: None,
                },
                CanonicalMessage {
                    // An unrecognized value must never be relayed verbatim —
                    // the SPA's own union type only knows the two real ones.
                    source_scope: None,
                    retrieval_scope: Some("bogus".into()),
                    message_id: String::new(),
                    role: "user".into(),
                    content: "Stale/unknown scope value.".into(),
                    agent_name: String::new(),
                    grounding: None,
                    citations: None,
                    artifacts: None,
                    context_refs: None,
                    research_plan: None,
                },
            ],
        )
        .expect("canonical conversation should render");

        assert_eq!(transcript.turns[0]["retrievalScope"], "knowledge_only");
        assert_eq!(transcript.turns[1]["retrievalScope"], "web_only");
        assert!(transcript.turns[2].get("retrievalScope").is_none());
        assert!(transcript.turns[3].get("retrievalScope").is_none());
    }

    /// The Resultat panel was empty on every reopened thread because the
    /// canonical read carried no artifacts (RUN-LOG finding 3). A persisted
    /// list is relayed as-is; an empty one must not create the key.
    #[test]
    fn canonical_messages_carry_persisted_artifacts() {
        let transcript = canonical_messages_to_transcript(
            "thread-1",
            vec![
                CanonicalMessage {
                    source_scope: None,
                    retrieval_scope: None,
                    message_id: String::new(),
                    role: "assistant".into(),
                    content: "Rapporten ligger i sidepanelet.".into(),
                    agent_name: String::new(),
                    grounding: None,
                    citations: None,
                    artifacts: Some(json!([
                        { "id": "rapport", "kind": "document", "title": "Salgsrapport", "content": "# Uke 37", "version": 2 },
                    ])),
                    context_refs: None,
                    research_plan: None,
                },
                CanonicalMessage {
                    source_scope: None,
                    retrieval_scope: None,
                    message_id: String::new(),
                    role: "assistant".into(),
                    content: "Ingen artefakter.".into(),
                    agent_name: String::new(),
                    grounding: None,
                    citations: None,
                    artifacts: Some(json!([])),
                    context_refs: None,
                    research_plan: None,
                },
            ],
        )
        .expect("canonical conversation should render");

        assert_eq!(transcript.turns[0]["artifacts"][0]["id"], "rapport");
        assert_eq!(transcript.turns[0]["artifacts"][0]["version"], 2);
        assert!(transcript.turns[1].get("artifacts").is_none());
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
                    source_scope: None,
                    retrieval_scope: None,
                    message_id: String::new(),
                    role: "assistant".into(),
                    content: "Oslo.".into(),
                    agent_name: String::new(),
                    grounding: None,
                    citations: Some(json!([
                        { "id": "w1", "title": "Oslo", "url": "https://example.test/oslo", "snippet": "capital" },
                    ])),
                    artifacts: None,
                    context_refs: None,
                    research_plan: None,
                },
                CanonicalMessage {
                    source_scope: None,
                    retrieval_scope: None,
                    message_id: String::new(),
                    role: "assistant".into(),
                    content: "No sources used.".into(),
                    agent_name: String::new(),
                    grounding: None,
                    // An empty list is not evidence; it must not create the key.
                    citations: Some(json!([])),
                    artifacts: None,
                    context_refs: None,
                    research_plan: None,
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

#[cfg(test)]
mod thread_paging_tests {
    use super::*;

    #[test]
    fn durable_summary_carries_star_and_unread_and_the_starred_filter_is_forwarded() {
        let summary = durable_to_summary(
            DurableThreadSummary {
                thread_id: "thread-star".to_owned(),
                title: "Kept".to_owned(),
                preview: String::new(),
                updated_at: "2026-10-28T10:00:00Z".to_owned(),
                created_at: "2026-10-28T09:00:00Z".to_owned(),
                pinned: false,
                persona_enabled: true,
                space_id: String::new(),
                origin: "chat".to_owned(),
                latest_run_id: String::new(),
                latest_run_status: String::new(),
                latest_run_updated_at: None,
                archived_at: String::new(),
                starred: true,
                unread: true,
                attention_state: String::new(),
                attention_since: String::new(),
                attention_unseen: false,
                forked_from_thread_id: String::new(),
                goal_status: String::new(),
            },
            "2026-10-28T11:00:00Z",
        )
        .expect("a thread with an id is listed");
        assert!(summary.starred);
        assert!(summary.unread);
        let url = durable_thread_list_url(
            "http://model-gateway",
            &ThreadListRequest {
                cursor: None,
                limit: 50,
                archived: None,
                thread_id: None,
                starred_only: true,
                agent: None,
            },
        );
        assert!(url.ends_with("&starred_only=true"));
    }

    #[test]
    fn read_durable_thread_url_addresses_one_thread_by_id_beyond_the_page() {
        let url = durable_thread_list_url(
            "http://model-gateway",
            &ThreadListRequest {
                cursor: None,
                limit: 1,
                archived: Some("all"),
                thread_id: Some("thread-81"),
                starred_only: false,
                agent: None,
            },
        );
        assert_eq!(
            url,
            "http://model-gateway/v1/threads?limit=1&origin=chat&thread_id=thread-81&archived=all"
        );
    }

    #[test]
    fn thread_81_transcript_is_reachable() {
        // The transcript route resolves its thread by id, so the lookup never
        // depends on the thread being inside the first page.
        let url = durable_thread_list_url(
            "http://model-gateway",
            &ThreadListRequest {
                cursor: None,
                limit: 1,
                archived: Some("all"),
                thread_id: Some("thread-81"),
                starred_only: false,
                agent: None,
            },
        );
        assert!(url.contains("thread_id=thread-81"));
        assert!(!url.contains("limit=50"));
    }

    #[test]
    fn thread_list_url_forwards_the_cursor_and_page_size() {
        let url = durable_thread_list_url(
            "http://model-gateway",
            &ThreadListRequest {
                cursor: Some("abc-_123"),
                limit: 50,
                archived: None,
                thread_id: None,
                starred_only: false,
                agent: None,
            },
        );
        assert_eq!(
            url,
            "http://model-gateway/v1/threads?limit=50&origin=chat&cursor=abc-_123"
        );
    }

    #[test]
    fn archive_goes_to_the_archive_route_without_a_body() {
        let (url, body) = archive_state_call("http://model-gateway", "thread-9", true);
        assert_eq!(url, "http://model-gateway/v1/threads/thread-9/archive");
        assert_eq!(body, None);
    }

    #[test]
    fn restore_goes_to_the_presentation_route_as_archived_false() {
        let (url, body) = archive_state_call("http://model-gateway", "thread-9", false);
        assert_eq!(url, "http://model-gateway/v1/threads/thread-9/presentation");
        assert_eq!(body, Some(json!({ "archived": false })));
    }
}

#[cfg(test)]
mod thread_goal_handler_tests {
    use super::*;

    #[test]
    fn set_goal_body_relays_only_its_own_fields() {
        let body: SetThreadGoalBody = serde_json::from_value(json!({
            "objective": "Ship the report",
            "successCriteria": ["Numbers reconciled"],
        }))
        .unwrap();
        assert_eq!(
            body.upstream(),
            json!({
                "objective": "Ship the report",
                "successCriteria": ["Numbers reconciled"],
                "replace": false,
            })
        );
        // An owner, org or Space field from the browser is refused, never relayed.
        for smuggled in ["userId", "orgId", "spaceId", "threadId"] {
            let mut raw = json!({"objective": "x"});
            raw[smuggled] = json!("forged");
            assert!(serde_json::from_value::<SetThreadGoalBody>(raw).is_err(), "{smuggled}");
        }
    }

    #[test]
    fn update_goal_body_keeps_absent_fields_absent() {
        let body: UpdateThreadGoalBody = serde_json::from_value(json!({
            "goalId": " goal_1 ",
            "expectedRevision": 2,
            "status": "paused",
        }))
        .unwrap();
        assert_eq!(
            body.upstream(),
            json!({"goalId": "goal_1", "expectedRevision": 2, "status": "paused"})
        );
        let without_goal = json!({"status": "paused"});
        assert!(serde_json::from_value::<UpdateThreadGoalBody>(without_goal).is_err());
    }

    #[test]
    fn goal_url_names_the_thread_once_encoded() {
        assert_eq!(
            thread_goal_url("http://model-gateway", "thread/9"),
            "http://model-gateway/v1/threads/thread%2F9/goal"
        );
    }
}

#[cfg(test)]
mod thread_goal_route_tests {
    use axum::{body::Body, http::Request, http::StatusCode};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn answer(server: &MockServer, verb: &str, route: &str, body: Value) {
        Mock::given(wm_method(verb))
            .and(wm_path(route))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn goal_routes_relay_each_verb_and_forward_no_forged_scope() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let auth = MockServer::start().await;
        answer(
            &auth,
            "GET",
            "/api/auth/get-session",
            json!({
                "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                "session": {"activeOrganizationId": "org-1"}
            }),
        )
        .await;
        for path in ["/api/model-plane/token", "/api/session-core/token"] {
            answer(&auth, "GET", path, json!({"token": "delegated-token"})).await;
        }
        let user_core = MockServer::start().await;
        answer(
            &user_core,
            "GET",
            "/api/v1/me/session-context",
            json!({"userId": "user-1", "orgId": "org-1", "role": "member", "onboardingStatus": "COMPLETED"}),
        )
        .await;
        let model = MockServer::start().await;
        answer(
            &model,
            "GET",
            "/v1/threads",
            json!({"threads": [{"thread_id": "thread-1", "origin": "chat"}], "next_cursor": ""}),
        )
        .await;
        let reply = json!({"found": true, "goal": {"id": "goal_1", "threadId": "thread-1", "status": "active"}});
        for verb in ["GET", "PUT", "PATCH"] {
            answer(&model, verb, "/v1/threads/thread-1/goal", reply.clone()).await;
        }

        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.model_gateway_url = model.uri();
        let calls = [
            ("GET", None),
            (
                "PUT",
                Some(json!({"objective": "Ship it", "successCriteria": ["Numbers reconciled"]})),
            ),
            (
                "PATCH",
                Some(json!({"goalId": "goal_1", "expectedRevision": 1, "status": "paused"})),
            ),
        ];
        for (verb, body) in calls {
            let mut builder = Request::builder()
                .method(verb)
                .uri("/api/v1/chat/threads/thread-1/goal?org_id=org-forged")
                .header("cookie", "better-auth.session_token=session-1")
                .header("x-verevon-org-id", "org-forged")
                .header("x-verevon-user-id", "user-forged");
            let request_body = match body {
                Some(value) => {
                    builder = builder.header("content-type", "application/json");
                    Body::from(value.to_string())
                }
                None => Body::empty(),
            };
            let response = crate::build_router(state.clone())
                .oneshot(builder.body(request_body).unwrap())
                .await
                .unwrap();
            let status = response.status();
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            let answered: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
            assert_eq!(status, StatusCode::OK, "{verb}: {answered}");
            assert_eq!(answered, reply, "{verb} relays the upstream answer unchanged");
        }

        let received = model.received_requests().await.unwrap();
        let forwarded = |verb: &str| {
            received
                .iter()
                .find(|request| {
                    request.method.as_str() == verb && request.url.path() == "/v1/threads/thread-1/goal"
                })
                .unwrap_or_else(|| panic!("{verb} reaches model-gateway"))
        };
        for verb in ["GET", "PUT", "PATCH"] {
            let request = forwarded(verb);
            assert_eq!(request.url.query(), None, "{verb}: the browser's query stays behind");
            assert!(request.headers.get("x-verevon-org-id").is_none(), "{verb}");
            assert!(request.headers.get("x-verevon-user-id").is_none(), "{verb}");
        }
        let put: Value = serde_json::from_slice(&forwarded("PUT").body).unwrap();
        assert_eq!(
            put,
            json!({"objective": "Ship it", "successCriteria": ["Numbers reconciled"], "replace": false})
        );
        let patch: Value = serde_json::from_slice(&forwarded("PATCH").body).unwrap();
        assert_eq!(
            patch,
            json!({"goalId": "goal_1", "expectedRevision": 1, "status": "paused"})
        );
    }
}

#[cfg(test)]
mod thread_export_handler_tests {
    use super::*;

    #[test]
    fn export_url_names_the_thread_and_passes_each_choice_once_encoded() {
        let query = ThreadExportQuery {
            format: Some("json".to_owned()),
            profile: Some("redacted".to_owned()),
        };
        assert_eq!(
            thread_export_url("http://model-gateway", "thread/9", &query),
            "http://model-gateway/v1/threads/thread%2F9/export?format=json&profile=redacted"
        );
        // A choice the owner left out is left out, and a value cannot add a second parameter.
        let partial = ThreadExportQuery {
            format: None,
            profile: Some("full&format=json".to_owned()),
        };
        assert_eq!(
            thread_export_url("http://model-gateway", "thread-9", &partial),
            "http://model-gateway/v1/threads/thread-9/export?profile=full%26format%3Djson"
        );
    }

    #[test]
    fn export_proxies_owner_bearer_only() {
        let request = owner_export_request(
            reqwest::Client::new().get("http://model-gateway/v1/threads/thread-9/export"),
            "user-1",
            "org-1",
            Some("model-token"),
            "session-token",
        )
        .build()
        .expect("the export request builds");
        let headers = request.headers();
        assert_eq!(headers[header::AUTHORIZATION], "Bearer model-token");
        assert_eq!(headers["x-session-authorization"], "Bearer session-token");
        assert_eq!(headers["x-user-id"], "user-1");
        assert_eq!(headers["x-org-id"], "org-1");
        // No other audience's credential rides on this call.
        for name in [
            "x-data-plane-authorization",
            "x-capability-authorization",
            "x-inference-authorization",
            "x-execution-authorization",
            "x-cost-authorization",
            "x-sandbox-authorization",
        ] {
            assert!(headers.get(name).is_none(), "{name}");
        }
    }

    #[test]
    fn export_preserves_content_disposition() {
        let headers = export_answer_headers(
            Some("text/markdown; charset=utf-8"),
            Some("attachment; filename=\"verevon-thread-thread-9.md\""),
        );
        assert_eq!(
            headers[header::CONTENT_TYPE],
            "text/markdown; charset=utf-8"
        );
        assert_eq!(
            headers[header::CONTENT_DISPOSITION],
            "attachment; filename=\"verevon-thread-thread-9.md\""
        );
        assert_eq!(headers[header::CACHE_CONTROL], "no-store");
        assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        // Without an upstream name the copy is still a download, never a page.
        let unnamed = export_answer_headers(None, None);
        assert_eq!(unnamed[header::CONTENT_DISPOSITION], "attachment");
        assert_eq!(unnamed[header::CONTENT_TYPE], "application/octet-stream");
    }
}

#[cfg(test)]
mod fork_handler_tests {
    use super::*;

    #[test]
    fn fork_body_reads_camel_case_and_refuses_unknown_fields() {
        let body: ForkThreadBody =
            serde_json::from_value(json!({"fromMessageId": "turn-4"})).unwrap();
        assert_eq!(body.from_message_id, "turn-4");
        assert!(body.title.is_empty());
        assert!(serde_json::from_value::<ForkThreadBody>(
            json!({"fromMessageId": "turn-4", "copyAll": true})
        )
        .is_err());
    }

    #[test]
    fn a_fork_carries_its_source_and_a_thread_that_is_not_forked_carries_none() {
        let forked: DurableThreadSummary = serde_json::from_value(
            json!({"threadId": "thread-fork", "forkedFromThreadId": "thread-source"}),
        )
        .unwrap();
        let summary = durable_to_summary(forked, "2026-10-31T00:00:00Z").unwrap();
        assert_eq!(summary.forked_from_thread_id.as_deref(), Some("thread-source"));

        let plain: DurableThreadSummary =
            serde_json::from_value(json!({"threadId": "thread-plain"})).unwrap();
        let summary = durable_to_summary(plain, "2026-10-31T00:00:00Z").unwrap();
        assert!(summary.forked_from_thread_id.is_none());
    }
}

#[cfg(test)]
#[path = "history_relay_tests.rs"]
mod relay_tests;
