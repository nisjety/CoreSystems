//! Router-policy domain — admin read/write of the Verevon intent layer's routing
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
) -> impl IntoResponse {
    let url = format!("{}/internal/v1/router-policy", state.inference_core_url);
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        Some(org_id.as_str()),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

pub(crate) async fn put_policy(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let url = format!("{}/internal/v1/router-policy", state.inference_core_url);
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    proxy_json(
        &state,
        Method::PUT,
        &url,
        Some(body),
        Some(org_id.as_str()),
        Some(&actor_for(&user)),
        None,
    )
    .await
}
