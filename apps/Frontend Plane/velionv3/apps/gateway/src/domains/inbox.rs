//! Inbox domain — unified support conversations.
//!
//! Proxies the SPA's `/api/v1/inbox/*` surface to conversation-core-go's
//! `/api/v1/conversations` + `/api/v1/inboxes` API. conversation-core-go
//! authenticates with the internal API key + `x-org-id` + `x-user-*` actor
//! headers — exactly what `proxy_json` forwards — so these are thin proxies and
//! the SPA never talks to the Application Plane directly.

use axum::{
    extract::{Extension, Path, State},
    http::{HeaderMap, Uri},
    response::IntoResponse,
    routing::{delete, get, patch, post},
    Json, Router,
};
use reqwest::Method;
use serde_json::Value;

use crate::{
    config::AppState,
    contracts::ActionActor,
    middleware::{require_session, AuthenticatedUser},
    upstream::proxy_json,
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/inbox/inboxes", get(list_inboxes))
        .route("/api/v1/inbox/conversations", get(list_conversations))
        .route("/api/v1/inbox/conversations/:id", get(get_conversation))
        .route(
            "/api/v1/inbox/conversations/:id/messages",
            post(add_message),
        )
        .route("/api/v1/inbox/conversations/:id/notes", post(add_note))
        .route(
            "/api/v1/inbox/conversations/:id/status",
            patch(patch_status),
        )
        .route(
            "/api/v1/inbox/conversations/:id/assignment",
            patch(patch_assignment),
        )
        .route("/api/v1/inbox/conversations/:id/tags", post(add_tag))
        .route(
            "/api/v1/inbox/conversations/:id/tags/:tag",
            delete(remove_tag),
        )
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

// ── helpers ─────────────────────────────────────────────────────────────────

fn org_id_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-velion-org-id")
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.trim().is_empty())
        .map(str::to_owned)
}

fn actor_for(user: &AuthenticatedUser) -> ActionActor {
    ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    }
}

fn qs(uri: &Uri) -> String {
    uri.query()
        .filter(|q| !q.is_empty())
        .map(|q| format!("?{q}"))
        .unwrap_or_default()
}

// ── handlers ────────────────────────────────────────────────────────────────

async fn list_inboxes(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let url = format!("{}/api/v1/inboxes", state.conversation_core_url);
    proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        org_id_from_headers(&headers).as_deref(),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

async fn list_conversations(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    uri: Uri,
) -> impl IntoResponse {
    let url = format!(
        "{}/api/v1/conversations{}",
        state.conversation_core_url,
        qs(&uri)
    );
    proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        org_id_from_headers(&headers).as_deref(),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

async fn get_conversation(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let url = format!(
        "{}/api/v1/conversations/{}",
        state.conversation_core_url,
        urlencoding::encode(&id)
    );
    proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        org_id_from_headers(&headers).as_deref(),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

async fn add_message(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    forward_conversation_write(
        &state,
        &user,
        &headers,
        Method::POST,
        &id,
        "messages",
        Some(body),
    )
    .await
}

async fn add_note(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    forward_conversation_write(
        &state,
        &user,
        &headers,
        Method::POST,
        &id,
        "notes",
        Some(body),
    )
    .await
}

async fn patch_status(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    forward_conversation_write(
        &state,
        &user,
        &headers,
        Method::PATCH,
        &id,
        "status",
        Some(body),
    )
    .await
}

async fn patch_assignment(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    forward_conversation_write(
        &state,
        &user,
        &headers,
        Method::PATCH,
        &id,
        "assignment",
        Some(body),
    )
    .await
}

async fn add_tag(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    forward_conversation_write(
        &state,
        &user,
        &headers,
        Method::POST,
        &id,
        "tags",
        Some(body),
    )
    .await
}

async fn remove_tag(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((id, tag)): Path<(String, String)>,
) -> impl IntoResponse {
    let url = format!(
        "{}/api/v1/conversations/{}/tags/{}",
        state.conversation_core_url,
        urlencoding::encode(&id),
        urlencoding::encode(&tag)
    );
    proxy_json(
        &state,
        Method::DELETE,
        &url,
        None,
        org_id_from_headers(&headers).as_deref(),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

async fn forward_conversation_write(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    method: Method,
    id: &str,
    sub: &str,
    body: Option<Value>,
) -> (axum::http::StatusCode, Json<Value>) {
    let url = format!(
        "{}/api/v1/conversations/{}/{}",
        state.conversation_core_url,
        urlencoding::encode(id),
        sub
    );
    proxy_json(
        state,
        method,
        &url,
        body,
        org_id_from_headers(headers).as_deref(),
        Some(&actor_for(user)),
        None,
    )
    .await
}
