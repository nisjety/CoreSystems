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
    Json(ok(ThreadsResponse { sessions })).into_response()
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

pub(super) async fn save_thread(
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

pub(super) async fn delete_thread(
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

pub(super) async fn clear_threads(
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
    let url = format!("{}/v1/threads?limit={MAX_THREADS}", state.model_gateway_url);
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
    })
}

async fn update_durable_presentation(
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
    #[serde(default)]
    role: String,
    #[serde(default)]
    content: String,
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
            json!({
                "id": format!("canonical-{}", index + 1),
                "role": message.role,
                "content": message.content,
            })
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
        canonical_messages_to_transcript, durable_to_summary, enforce_support_thread_policy,
        strip_support_thread_capabilities, CanonicalMessage, DurableThreadSummary,
    };
    use serde_json::{json, Value};

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
            },
            "2026-08-11T12:00:00Z",
        )
        .expect("valid Session Core record");

        assert_eq!(summary.title, "Customer shipping follow-up");
        assert_eq!(summary.preview, "Waiting on the carrier receipt");
        assert!(summary.pinned);
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

    #[test]
    fn canonical_messages_are_adapted_without_task_steps_or_bff_metadata() {
        let transcript = canonical_messages_to_transcript(
            "thread-1",
            vec![
                CanonicalMessage {
                    role: "user".into(),
                    content: "Question".into(),
                },
                CanonicalMessage {
                    role: "assistant".into(),
                    content: "Answer".into(),
                },
            ],
        )
        .expect("canonical conversation should render");

        assert_eq!(transcript.thread_id, "thread-1");
        assert_eq!(transcript.turns[0]["role"], "user");
        assert_eq!(transcript.turns[1]["content"], "Answer");
        assert!(transcript.task_steps.is_none());
        assert!(canonical_messages_to_transcript("thread-1", Vec::new()).is_none());
    }
}
