use axum::{
    extract::{Extension, Path, State},
    Json,
};
use reqwest::Method;
use serde_json::Value;

use crate::{config::AppState, middleware::AuthenticatedUser, upstream::proxy_json};

use super::shared::{actor_for, require_org_admin, GatewayJsonResponse};

pub(super) async fn list_members(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> GatewayJsonResponse {
    if let Err(response) = require_org_admin(&state, &user, &id).await {
        return response;
    }

    let url = format!(
        "{}/orgs/{}/members",
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

pub(super) async fn invite_member(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> GatewayJsonResponse {
    if let Err(response) = require_org_admin(&state, &user, &id).await {
        return response;
    }

    let url = format!(
        "{}/orgs/{}/members/invite",
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

pub(super) async fn remove_member(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((id, user_id)): Path<(String, String)>,
) -> GatewayJsonResponse {
    if let Err(response) = require_org_admin(&state, &user, &id).await {
        return response;
    }

    let url = format!(
        "{}/orgs/{}/members/{}",
        state.org_core_url,
        urlencoding::encode(&id),
        urlencoding::encode(&user_id)
    );
    proxy_json(
        &state,
        Method::DELETE,
        &url,
        None,
        None,
        Some(&actor_for(&user)),
        None,
    )
    .await
}

pub(super) async fn update_member_role(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((id, user_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> GatewayJsonResponse {
    if let Err(response) = require_org_admin(&state, &user, &id).await {
        return response;
    }

    let url = format!(
        "{}/orgs/{}/members/{}/role",
        state.org_core_url,
        urlencoding::encode(&id),
        urlencoding::encode(&user_id)
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
