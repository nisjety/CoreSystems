//! ShareDialog backend (PR-6, re-pointed by WS14-S4) — proxies documents-api.
//!
//! Backs the verevonv3 ShareDialog: list who a document is shared with, share it
//! with a user (view-only), un-share, and the read-only "shared with me" list.
//!
//! Authority discipline: documents-api OWNS the document, so it is the service
//! that proves the signed-in user owns it before Control writes the grant
//! (`resource_grants` stays the single grant authority). This gateway holds no
//! `authz:*` scope and never calls Control's grant surface: it relays the
//! user's own `aud=data-plane` token (`proxy_data_plane_json`), and the client
//! supplies ONLY the subject (and an optional role, which documents-api refuses
//! unless it is `view`). `org_id` and the granter come from the verified
//! session / token, never from the body.
//!
//! The `ownership.share_document` / `ownership.revoke_document_share` actions
//! call the same two upstream helpers below, so there is exactly one share path.

use axum::{
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{delete, get},
    Json, Router,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    domains::knowledge::shared::proxy_data_plane_json,
    envelope::error,
    middleware::{require_session, AuthenticatedUser},
    upstream::authorized_org_id,
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/documents/{id}/shares",
            get(list_shares).post(create_share),
        )
        .route(
            "/api/v1/documents/{id}/shares/{subject_id}",
            delete(revoke_share),
        )
        .route("/api/v1/shares/shared-with-me", get(shared_with_me))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

#[derive(Deserialize)]
struct CreateShareRequest {
    subject_id: String,
    #[serde(default)]
    role: Option<String>,
}

fn default_error_code(status: StatusCode) -> &'static str {
    match status {
        StatusCode::BAD_REQUEST => "invalid_request",
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => "forbidden",
        StatusCode::NOT_FOUND => "not_found",
        StatusCode::CONFLICT => "conflict",
        StatusCode::SERVICE_UNAVAILABLE => "sharing_unavailable",
        _ => "upstream_error",
    }
}

fn is_safe_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 64
        && code
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// documents-api answers `{ "error": "message", "code": "stable_code" }`. The
/// browser contract is `{ error: { code, message } }`, so rewrite the failure
/// here. The stable code is relayed only when it is a plain snake_case token;
/// the message is the service's own short text, never an upstream URL.
fn normalize_error(status: StatusCode, body: &Value) -> Value {
    if body.get("error").map(Value::is_object).unwrap_or(false) {
        return body.clone();
    }
    let code = body
        .get("code")
        .and_then(Value::as_str)
        .filter(|c| is_safe_code(c))
        .unwrap_or_else(|| default_error_code(status));
    let message = body
        .get("error")
        .and_then(Value::as_str)
        .filter(|m| !m.is_empty() && m.len() <= 300)
        .unwrap_or("The sharing request could not be completed.");
    json!({ "error": { "code": code, "message": message } })
}

async fn org_for(state: &AppState, user: &AuthenticatedUser) -> Option<String> {
    let org = authorized_org_id(state, user).await;
    (!org.is_empty()).then_some(org)
}

/// Share `doc_id` with `subject_id` through documents-api (owner check there).
/// Shared by the REST route and the `ownership.share_document` action.
pub(crate) async fn share_document_upstream(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    doc_id: &str,
    subject_id: &str,
    role: Option<&str>,
) -> (StatusCode, Value) {
    let org_id = org_for(state, user).await;
    let url = format!(
        "{}/v1/documents/{}/shares",
        state.documents_api_url,
        urlencoding::encode(doc_id)
    );
    let mut payload = json!({ "subject_id": subject_id });
    if let Some(role) = role.map(str::trim).filter(|r| !r.is_empty()) {
        payload["role"] = json!(role);
    }
    let (status, Json(body)) = proxy_data_plane_json(
        state,
        user,
        headers,
        Method::POST,
        &url,
        Some(payload),
        org_id.as_deref(),
        None,
    )
    .await;
    if status.is_success() {
        (status, body)
    } else {
        (status, normalize_error(status, &body))
    }
}

/// Remove the user grant for `subject_id` on `doc_id` through documents-api.
/// Shared by the REST route and the `ownership.revoke_document_share` action.
pub(crate) async fn revoke_document_share_upstream(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    doc_id: &str,
    subject_id: &str,
) -> (StatusCode, Value) {
    let org_id = org_for(state, user).await;
    let url = format!(
        "{}/v1/documents/{}/shares/{}",
        state.documents_api_url,
        urlencoding::encode(doc_id),
        urlencoding::encode(subject_id)
    );
    let (status, Json(body)) = proxy_data_plane_json(
        state,
        user,
        headers,
        Method::DELETE,
        &url,
        None,
        org_id.as_deref(),
        None,
    )
    .await;
    if status.is_success() {
        (status, body)
    } else {
        (status, normalize_error(status, &body))
    }
}

/// GET /api/v1/documents/:id/shares — the ShareDialog "shared with" list. The
/// SPA reads `grants`; documents-api names the same rows `shares`. Space rows
/// carry `subject_type: "space"` and are read-only.
async fn list_shares(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(doc_id): Path<String>,
) -> impl IntoResponse {
    let org_id = org_for(&state, &user).await;
    let url = format!(
        "{}/v1/documents/{}/shares",
        state.documents_api_url,
        urlencoding::encode(&doc_id)
    );
    let (status, Json(body)) = proxy_data_plane_json(
        &state,
        &user,
        &headers,
        Method::GET,
        &url,
        None,
        org_id.as_deref(),
        None,
    )
    .await;
    if !status.is_success() {
        return (status, Json(normalize_error(status, &body))).into_response();
    }
    let grants = body.get("shares").cloned().unwrap_or_else(|| json!([]));
    (status, Json(json!({ "grants": grants }))).into_response()
}

/// POST /api/v1/documents/:id/shares — share the document with a user (view).
/// Only `subject_id` (and an optional role documents-api validates) is
/// client-supplied.
async fn create_share(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(doc_id): Path<String>,
    Json(req): Json<CreateShareRequest>,
) -> impl IntoResponse {
    let subject_id = req.subject_id.trim().to_string();
    if subject_id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("invalid_request", "subject_id is required")),
        )
            .into_response();
    }
    let (status, body) = share_document_upstream(
        &state,
        &user,
        &headers,
        &doc_id,
        &subject_id,
        req.role.as_deref(),
    )
    .await;
    (status, Json(body)).into_response()
}

/// DELETE /api/v1/documents/:id/shares/:subject_id — un-share (user grants only).
async fn revoke_share(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((doc_id, subject_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let (status, body) =
        revoke_document_share_upstream(&state, &user, &headers, &doc_id, &subject_id).await;
    (status, Json(body)).into_response()
}

/// GET /api/v1/shares/shared-with-me — documents explicitly shared with the
/// viewer. The subject is the verified token's user, never client-supplied.
async fn shared_with_me(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let org_id = org_for(&state, &user).await;
    let url = format!("{}/v1/documents/shared-with-me", state.documents_api_url);
    let (status, Json(body)) = proxy_data_plane_json(
        &state,
        &user,
        &headers,
        Method::GET,
        &url,
        None,
        org_id.as_deref(),
        None,
    )
    .await;
    if status.is_success() {
        (status, Json(body)).into_response()
    } else {
        (status, Json(normalize_error(status, &body))).into_response()
    }
}
