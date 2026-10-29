use axum::{
    extract::{Extension, Path, State},
    Json,
};
use reqwest::Method;
use serde_json::Value;

use crate::{config::AppState, middleware::AuthenticatedUser, upstream::proxy_json};

use super::shared::actor_for;

pub(super) async fn get_setting(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(key): Path<String>,
) -> impl axum::response::IntoResponse {
    let url = format!(
        "{}/api/v1/settings/{}",
        state.user_core_url,
        urlencoding::encode(&key)
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

pub(super) async fn put_setting(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(key): Path<String>,
    Json(body): Json<Value>,
) -> impl axum::response::IntoResponse {
    let url = format!(
        "{}/api/v1/settings/{}",
        state.user_core_url,
        urlencoding::encode(&key)
    );
    proxy_json(
        &state,
        Method::PUT,
        &url,
        Some(body),
        None,
        Some(&actor_for(&user)),
        None,
    )
    .await
}
