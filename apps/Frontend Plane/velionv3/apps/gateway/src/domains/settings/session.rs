use axum::{
    extract::{Extension, State},
    Json,
};
use reqwest::Method;
use serde_json::Value;

use crate::{config::AppState, middleware::AuthenticatedUser, upstream::proxy_json};

use super::shared::actor_for;

pub(super) async fn session_refresh(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> impl axum::response::IntoResponse {
    let url = format!("{}/api/v1/sessions/refresh", state.session_core_url);
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
