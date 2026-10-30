use axum::{
    middleware,
    routing::{get, post},
    Router,
};
use tower::limit::ConcurrencyLimitLayer;

use crate::{auth, handlers, state::AppState};

pub fn app(state: AppState) -> Router {
    let protected_routes = Router::new()
        .route("/v1/endpoints", get(handlers::list_endpoints))
        .route("/v1/backfills/{backfill_id}", get(handlers::get_backfill))
        .route(
            "/v1/schedules/{schedule_id}/triggers/{trigger_id}",
            get(handlers::get_schedule_trigger),
        )
        .route("/v1/runs", get(handlers::list_runs))
        .route("/v1/checkpoints", get(handlers::list_checkpoints))
        .route(
            "/v1/schedules",
            get(handlers::list_schedules).post(handlers::create_schedule),
        )
        .route(
            "/v1/sync/backfill",
            post(handlers::backfill).layer(ConcurrencyLimitLayer::new(1)),
        )
        .route(
            "/v1/schedules/{schedule_id}/trigger",
            post(handlers::trigger_schedule).layer(ConcurrencyLimitLayer::new(1)),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_bearer_token,
        ));

    Router::new()
        .route("/v1/health", get(handlers::health))
        .merge(protected_routes)
        .with_state(state)
}
