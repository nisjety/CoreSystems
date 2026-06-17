use axum::{
    extract::{Extension, State},
    http::HeaderMap,
};
use reqwest::Method;

use crate::{config::AppState, middleware::AuthenticatedUser, upstream::proxy_integration_json};

use super::shared::{actor_for, org_id_from_headers};

pub(super) async fn integration_profile(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> impl axum::response::IntoResponse {
    let org_id = org_id_from_headers(&headers);
    let url = format!(
        "{}/api/v1/projections/integration-profile",
        state.integration_core_url
    );
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
