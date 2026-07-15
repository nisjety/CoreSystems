use axum::extract::{Extension, Path, State};
use reqwest::Method;

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
        Some(&id),
        Some(&actor_for(&user)),
        None,
    )
    .await
}
