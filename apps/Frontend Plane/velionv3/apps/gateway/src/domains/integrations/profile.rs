use axum::{
    extract::{Extension, State},
    http::HeaderMap,
};
use reqwest::Method;

use crate::{config::AppState, middleware::AuthenticatedUser};

use super::shared::proxy_for_user;

pub(super) async fn integration_profile(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> impl axum::response::IntoResponse {
    let url = format!(
        "{}/api/v1/projections/integration-profile",
        state.integration_core_url
    );
    proxy_for_user(&state, &user, &headers, Method::GET, &url, None).await
}
