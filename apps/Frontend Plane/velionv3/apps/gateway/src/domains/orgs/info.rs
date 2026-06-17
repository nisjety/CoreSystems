use axum::{
    extract::{Extension, Path, State},
    response::IntoResponse,
};
use reqwest::Method;

use crate::{config::AppState, middleware::AuthenticatedUser, upstream::proxy_json};

use super::shared::actor_for;

pub(super) async fn get_org(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let url = format!(
        "{}/api/v1/organizations/{}",
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

pub(super) async fn get_org_entitlements(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let url = format!(
        "{}/api/v1/organizations/{}/entitlements",
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
