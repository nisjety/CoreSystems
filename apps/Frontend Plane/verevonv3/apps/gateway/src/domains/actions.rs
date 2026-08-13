pub(crate) mod dispatchers;
mod handlers;
mod shared;

use axum::{
    routing::{get, post},
    Router,
};

use crate::{config::AppState, middleware::require_session};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/actions/catalog",
            get(handlers::list_action_contracts),
        )
        .route("/api/v1/actions/execute", post(handlers::execute_action))
        .route(
            "/api/v1/actions/tickets/create/:idempotency_key",
            get(handlers::reconcile_ticket_create),
        )
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}
