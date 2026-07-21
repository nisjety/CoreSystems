//! Inbox domain — unified support conversations.
//!
//! Proxies the SPA's `/api/v1/inbox/*` surface to conversation-core-go's
//! `/api/v1/conversations`, `/api/v1/inboxes`, and `/api/v1/ai-actions` (the HITL
//! review queue) API. conversation-core-go
//! authenticates with the internal API key + `x-org-id` + `x-user-*` actor
//! headers — exactly what `proxy_json` forwards — so these are thin proxies and
//! the SPA never talks to the Application Plane directly.

use axum::{
    extract::{Extension, Path, State},
    http::Uri,
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post},
    Json, Router,
};
use reqwest::Method;
use serde_json::Value;

use crate::{
    config::AppState,
    middleware::{require_session, AuthenticatedUser},
    upstream::proxy_conversation_json,
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
        .route("/api/v1/inbox/feedback", post(submit_feedback))
        .route("/api/v1/inbox/ai-actions", get(list_ai_actions))
        .route(
            "/api/v1/inbox/ai-actions/:id/approve",
            post(approve_ai_action),
        )
        .route(
            "/api/v1/inbox/ai-actions/:id/reject",
            post(reject_ai_action),
        )
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

// ── helpers ─────────────────────────────────────────────────────────────────

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
) -> Response {
    let url = format!("{}/api/v1/inboxes", state.conversation_core_url);
    proxy_conversation_json(&state, Method::GET, &url, None, &user, None)
        .await
        .into_response()
}

async fn list_conversations(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    uri: Uri,
) -> Response {
    let url = format!(
        "{}/api/v1/conversations{}",
        state.conversation_core_url,
        qs(&uri)
    );
    proxy_conversation_json(&state, Method::GET, &url, None, &user, None)
        .await
        .into_response()
}

async fn get_conversation(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Response {
    let url = format!(
        "{}/api/v1/conversations/{}",
        state.conversation_core_url,
        urlencoding::encode(&id)
    );
    proxy_conversation_json(&state, Method::GET, &url, None, &user, None)
        .await
        .into_response()
}

async fn add_message(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    forward_conversation_write(&state, &user, Method::POST, &id, "messages", Some(body)).await
}

async fn add_note(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    forward_conversation_write(&state, &user, Method::POST, &id, "notes", Some(body)).await
}

/// A signed-in org member's one-line friction report from the shell's
/// persistent "Send feedback" control. Proxies straight to
/// conversation-core-go's `/api/v1/feedback`, which lands it as a new,
/// `demo-feedback`-tagged conversation in the org's own Inbox — the SPA never
/// picks the conversation id or org scope itself.
async fn submit_feedback(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    let url = format!("{}/api/v1/feedback", state.conversation_core_url);
    proxy_conversation_json(&state, Method::POST, &url, Some(body), &user, None)
        .await
        .into_response()
}

async fn patch_status(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    forward_conversation_write(&state, &user, Method::PATCH, &id, "status", Some(body)).await
}

async fn patch_assignment(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    forward_conversation_write(&state, &user, Method::PATCH, &id, "assignment", Some(body)).await
}

async fn add_tag(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    forward_conversation_write(&state, &user, Method::POST, &id, "tags", Some(body)).await
}

async fn remove_tag(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((id, tag)): Path<(String, String)>,
) -> Response {
    let url = format!(
        "{}/api/v1/conversations/{}/tags/{}",
        state.conversation_core_url,
        urlencoding::encode(&id),
        urlencoding::encode(&tag)
    );
    proxy_conversation_json(&state, Method::DELETE, &url, None, &user, None)
        .await
        .into_response()
}

async fn forward_conversation_write(
    state: &AppState,
    user: &AuthenticatedUser,
    method: Method,
    id: &str,
    sub: &str,
    body: Option<Value>,
) -> Response {
    let url = format!(
        "{}/api/v1/conversations/{}/{}",
        state.conversation_core_url,
        urlencoding::encode(id),
        sub
    );
    proxy_conversation_json(state, method, &url, body, user, None)
        .await
        .into_response()
}

// ── AI-action HITL review queue ───────────────────────────────────────────────
//
// Model-proposed actions awaiting a human decision. The org scope is always the
// authenticated session's org (never a client header/query/body), so a foreign
// action id simply does not match `org_id = $authenticated AND id = $id` upstream
// and resolves to 404 — never a cross-tenant read or a phantom review row.

async fn list_ai_actions(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    uri: Uri,
) -> Response {
    let url = format!(
        "{}/api/v1/ai-actions{}",
        state.conversation_core_url,
        qs(&uri)
    );
    proxy_conversation_json(&state, Method::GET, &url, None, &user, None)
        .await
        .into_response()
}

async fn approve_ai_action(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    body: Option<Json<Value>>,
) -> Response {
    forward_ai_action_review(&state, &user, &id, "approve", body.map(|Json(value)| value)).await
}

async fn reject_ai_action(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    body: Option<Json<Value>>,
) -> Response {
    forward_ai_action_review(&state, &user, &id, "reject", body.map(|Json(value)| value)).await
}

async fn forward_ai_action_review(
    state: &AppState,
    user: &AuthenticatedUser,
    id: &str,
    decision: &str,
    body: Option<Value>,
) -> Response {
    let url = format!(
        "{}/api/v1/ai-actions/{}/{}",
        state.conversation_core_url,
        urlencoding::encode(id),
        decision
    );
    proxy_conversation_json(state, Method::POST, &url, body, user, None)
        .await
        .into_response()
}
