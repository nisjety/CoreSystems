//! ShareDialog backend (PR-6) — proxies the user-core resource_grants facade.
//!
//! Backs the verevonv3 ShareDialog: list who a document is shared with, share it
//! with a user (view-only for MVP), un-share, and the read-only "shared with me"
//! list. `resource_grants` (user-core) is the SINGLE authority retrieval +
//! documents-api enforce against — the UI writes here, never to a display flag.
//!
//! Identity discipline: `proxy_json` injects the internal key + the validated
//! session actor's `x-user-id`, so the facade's internal-key-only guard is
//! satisfied structurally and a Bearer user token can never reach it directly.
//! `org_id` and `granted_by` are set SERVER-SIDE from the session — the client
//! supplies ONLY the subject to share with.

use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get},
    Json, Router,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::error,
    middleware::{require_session, AuthenticatedUser},
    upstream::{authorized_org_id, proxy_json},
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/documents/:id/shares",
            get(list_shares).post(create_share),
        )
        .route(
            "/api/v1/documents/:id/shares/:subject_id",
            delete(revoke_share),
        )
        .route("/api/v1/shares/shared-with-me", get(shared_with_me))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

fn actor_for(user: &AuthenticatedUser) -> ActionActor {
    ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    }
}

#[derive(Deserialize)]
struct CreateShareRequest {
    subject_id: String,
}

/// GET /api/v1/documents/:id/shares — list the explicit grants on a document
/// (the ShareDialog "shared with" list).
async fn list_shares(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(doc_id): Path<String>,
) -> impl IntoResponse {
    let org_id = authorized_org_id(&state, &user).await;
    let actor = actor_for(&user);
    let url = format!(
        "{}/api/v1/internal/authz/grants?org_id={}&resource_type=document&resource_id={}",
        state.user_core_url,
        urlencoding::encode(&org_id),
        urlencoding::encode(&doc_id),
    );
    let (status, body): (StatusCode, Json<Value>) = proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        Some(&org_id),
        Some(&actor),
        None,
    )
    .await;
    (status, body).into_response()
}

/// POST /api/v1/documents/:id/shares — share the document with a user (view).
/// Only `subject_id` is client-supplied; `org_id` + `granted_by` are server-set
/// from the validated session, so a caller can never forge the granter.
async fn create_share(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
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
    let org_id = authorized_org_id(&state, &user).await;
    let actor = actor_for(&user);
    let url = format!("{}/api/v1/internal/authz/grant", state.user_core_url);
    let payload = json!({
        "org_id": org_id,
        "resource_type": "document",
        "resource_id": doc_id,
        "subject_id": subject_id,
        "subject_type": "user",
        "role": "view",
        "granted_by": actor.user_id,
    });
    let (status, body): (StatusCode, Json<Value>) = proxy_json(
        &state,
        Method::POST,
        &url,
        Some(payload),
        Some(&org_id),
        Some(&actor),
        None,
    )
    .await;
    (status, body).into_response()
}

/// DELETE /api/v1/documents/:id/shares/:subject_id — un-share (remove the grant).
async fn revoke_share(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((doc_id, subject_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let org_id = authorized_org_id(&state, &user).await;
    let actor = actor_for(&user);
    let url = format!(
        "{}/api/v1/internal/authz/grant?org_id={}&resource_type=document&resource_id={}&subject_id={}&subject_type=user",
        state.user_core_url,
        urlencoding::encode(&org_id),
        urlencoding::encode(&doc_id),
        urlencoding::encode(&subject_id),
    );
    let (status, body): (StatusCode, Json<Value>) = proxy_json(
        &state,
        Method::DELETE,
        &url,
        None,
        Some(&org_id),
        Some(&actor),
        None,
    )
    .await;
    (status, body).into_response()
}

/// GET /api/v1/shares/shared-with-me — document ids explicitly shared with the
/// viewer (off ListVisible). Subject is the session actor, never client-supplied.
async fn shared_with_me(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let org_id = authorized_org_id(&state, &user).await;
    let actor = actor_for(&user);
    let url = format!(
        "{}/api/v1/internal/authz/visible?org_id={}&subject_id={}&resource_type=document",
        state.user_core_url,
        urlencoding::encode(&org_id),
        urlencoding::encode(&actor.user_id),
    );
    let (status, body): (StatusCode, Json<Value>) = proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        Some(&org_id),
        Some(&actor),
        None,
    )
    .await;
    (status, body).into_response()
}
