//! Agent "specialized actions" surface for the chat composer's `/` menu:
//! skills and capabilities. Both are Model-Plane catalogs fronted by
//! model-gateway (`/v1/skills`, `/v1/capabilities`, which it in turn proxies to
//! capability-core). The SPA only ever talks to this gateway, so these proxies
//! are what let the `/` menu surface real, org-scoped skills/capabilities.
//!
//! Read-only (GET) by design — activation happens client-side by threading the
//! chosen action into the chat invoke `tools` array; there is no server-side
//! mutation here.

use std::collections::HashMap;

use axum::{
    extract::{Extension, Query, State},
    http::HeaderMap,
    response::IntoResponse,
    routing::get,
    Router,
};
use reqwest::Method;

use crate::{
    config::AppState,
    domains::chat::shared,
    middleware::{require_session, AuthenticatedUser},
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/skills", get(list_skills))
        .route("/api/v1/capabilities", get(list_capabilities))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

/// Rebuild a forwardable query string from the incoming params, dropping empty
/// values. Upstream handlers read `q`, `kind`, `limit`, `after_id`.
fn build_query(params: &HashMap<String, String>) -> String {
    let pairs: Vec<String> = params
        .iter()
        .filter(|(_, value)| !value.trim().is_empty())
        .map(|(key, value)| {
            format!(
                "{}={}",
                urlencoding::encode(key),
                urlencoding::encode(value)
            )
        })
        .collect();
    if pairs.is_empty() {
        String::new()
    } else {
        format!("?{}", pairs.join("&"))
    }
}

async fn list_skills(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let token = shared::model_token(&state, &user, &headers).await;
    let capability = match shared::required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return shared::delegated_auth_unavailable(error),
    };
    let url = format!(
        "{}/v1/skills{}",
        state.model_gateway_url,
        build_query(&params)
    );
    shared::proxy_model_json_with_capability(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
}

async fn list_capabilities(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let token = shared::model_token(&state, &user, &headers).await;
    let capability = match shared::required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return shared::delegated_auth_unavailable(error),
    };
    let url = format!(
        "{}/v1/capabilities{}",
        state.model_gateway_url,
        build_query(&params)
    );
    shared::proxy_model_json_with_capability(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
}
