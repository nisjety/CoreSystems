use axum::{
    extract::{Extension, State},
    Json,
};
use reqwest::Method;
use serde_json::Value;

use crate::{config::AppState, middleware::AuthenticatedUser, upstream::proxy_json};

use super::shared::actor_for;

pub(super) async fn get_preferences(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> impl axum::response::IntoResponse {
    // user-core serves preferences at /api/v1/preferences (not /users/me/preferences);
    // the user is resolved from the forwarded actor identity headers.
    let url = format!("{}/api/v1/preferences", state.user_core_url);
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

pub(super) async fn update_preferences(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> impl axum::response::IntoResponse {
    // user-core serves preferences at /api/v1/preferences (not /users/me/preferences);
    // the user is resolved from the forwarded actor identity headers.
    let url = format!("{}/api/v1/preferences", state.user_core_url);
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
