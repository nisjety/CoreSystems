use axum::{
    extract::{Extension, Path, State},
    http::HeaderMap,
};
use reqwest::Method;

use crate::{config::AppState, middleware::AuthenticatedUser};

use super::shared::proxy_for_user;

pub(super) async fn list_sync_jobs(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> impl axum::response::IntoResponse {
    let url = format!("{}/api/v1/sync-jobs", state.integration_core_url);
    proxy_for_user(&state, &user, &headers, Method::GET, &url, None).await
}

pub(super) async fn get_sync_job(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl axum::response::IntoResponse {
    let url = format!(
        "{}/api/v1/sync-jobs/{}",
        state.integration_core_url,
        urlencoding::encode(&id)
    );
    proxy_for_user(&state, &user, &headers, Method::GET, &url, None).await
}
