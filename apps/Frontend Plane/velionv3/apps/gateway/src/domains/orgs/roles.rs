use axum::{
    extract::{Extension, Path, State},
    Json,
};
use reqwest::Method;
use serde_json::Value;

use crate::{config::AppState, middleware::AuthenticatedUser, upstream::proxy_json};

use super::shared::{actor_for, require_org_admin, GatewayJsonResponse};

pub(super) async fn list_roles(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> GatewayJsonResponse {
    if let Err(response) = require_org_admin(&state, &user, &id).await {
        return response;
    }

    let url = format!(
        "{}/orgs/{}/roles",
        state.org_core_url,
        urlencoding::encode(&id)
    );
    proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        None,
        Some(&actor_for(&user)),
        None,
    )
    .await
}

pub(super) async fn create_role(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> GatewayJsonResponse {
    if let Err(response) = require_org_admin(&state, &user, &id).await {
        return response;
    }

    let url = format!(
        "{}/orgs/{}/roles",
        state.org_core_url,
        urlencoding::encode(&id)
    );
    proxy_json(
        &state,
        Method::POST,
        &url,
        Some(body),
        None,
        Some(&actor_for(&user)),
        None,
    )
    .await
}

pub(super) async fn update_role(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((id, role_name)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> GatewayJsonResponse {
    if let Err(response) = require_org_admin(&state, &user, &id).await {
        return response;
    }

    let url = format!(
        "{}/orgs/{}/roles/{}",
        state.org_core_url,
        urlencoding::encode(&id),
        urlencoding::encode(&role_name)
    );
    proxy_json(
        &state,
        Method::PATCH,
        &url,
        Some(body),
        None,
        Some(&actor_for(&user)),
        None,
    )
    .await
}
