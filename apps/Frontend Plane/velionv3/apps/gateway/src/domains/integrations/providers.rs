use axum::{
    extract::{Extension, Path, State},
    http::HeaderMap,
    Json,
};
use reqwest::Method;
use serde_json::Value;

use crate::{config::AppState, middleware::AuthenticatedUser, upstream::proxy_integration_json};

use super::shared::{actor_for, org_id_from_headers};

pub(super) async fn list_providers(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> impl axum::response::IntoResponse {
    let org_id = org_id_from_headers(&headers);
    let url = format!("{}/api/v1/providers", state.integration_core_url);
    proxy_integration_json(
        &state,
        Method::GET,
        &url,
        None,
        Some(&actor_for(&user)),
        org_id.as_deref(),
    )
    .await
}

pub(super) async fn start_connect_session(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(provider): Path<String>,
    Json(body): Json<Value>,
) -> impl axum::response::IntoResponse {
    let org_id = org_id_from_headers(&headers);
    let url = format!(
        "{}/api/v1/providers/{}/connect-session",
        state.integration_core_url,
        urlencoding::encode(&provider)
    );
    proxy_integration_json(
        &state,
        Method::POST,
        &url,
        Some(body),
        Some(&actor_for(&user)),
        org_id.as_deref(),
    )
    .await
}
