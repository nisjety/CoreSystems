use axum::extract::{Extension, Path, State};
use reqwest::Method;

use crate::{config::AppState, middleware::AuthenticatedUser, upstream::proxy_integration_json};

use super::shared::actor_for;

pub(super) async fn connect_session_status(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> impl axum::response::IntoResponse {
    let org_id = {
        let o = crate::upstream::authorized_org_id(&state, &user).await;
        (!o.is_empty()).then_some(o)
    };
    let url = format!(
        "{}/api/v1/connect-sessions/{}/status",
        state.integration_core_url,
        urlencoding::encode(&id)
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
