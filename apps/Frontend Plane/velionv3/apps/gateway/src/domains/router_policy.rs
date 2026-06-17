//! Router-policy domain — admin read/write of the Velion intent layer's routing
//! policy (the "model router": mode→model table, complexity thresholds, budget
//! cap). Both verbs proxy to inference-core's internal HTTP endpoint:
//!
//! * `GET` returns the **effective** policy inference-core is currently using.
//! * `PUT` is a write-through: inference-core persists to session-core and
//!   refreshes its own live policy, then returns the stored policy.
//!
//! Admin enforcement is the SPA's `RequireWorkspaceAdmin` gate plus the
//! forwarded `x-user-role`; this proxy requires an authenticated session.

use axum::{
    extract::{Extension, State},
    http::HeaderMap,
    response::IntoResponse,
    routing::get,
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
        .route("/api/v1/router-policy", get(get_policy).put(put_policy))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

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

async fn get_policy(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let url = format!("{}/internal/v1/router-policy", state.inference_core_url);
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

async fn put_policy(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let url = format!("{}/internal/v1/router-policy", state.inference_core_url);
    proxy_json(
        &state,
        Method::PUT,
        &url,
        Some(body),
        org_id_from_headers(&headers).as_deref(),
        Some(&actor_for(&user)),
        None,
    )
    .await
}
