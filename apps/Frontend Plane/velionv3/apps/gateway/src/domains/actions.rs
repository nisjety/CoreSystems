mod dispatchers;
mod handlers;
mod shared;

use axum::{routing::post, Router};

use crate::{config::AppState, middleware::require_session};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/actions/execute", post(handlers::execute_action))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}
