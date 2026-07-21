mod connect_sessions;
mod connections;
mod profile;
mod providers;
mod shared;
mod sync_jobs;

use axum::{
    routing::{delete, get, post},
    Router,
};

use crate::{config::AppState, middleware::require_session};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        // Providers
        .route(
            "/api/v1/integrations/providers",
            get(providers::list_providers),
        )
        .route(
            "/api/v1/integrations/providers/:provider/connect-session",
            post(providers::start_connect_session),
        )
        // Connect sessions
        .route(
            "/api/v1/integrations/connect-sessions/:id/status",
            get(connect_sessions::connect_session_status),
        )
        // Connections — static "connections" route before param ":id" routes
        .route(
            "/api/v1/integrations/connections",
            get(connections::list_connections),
        )
        .route(
            "/api/v1/integrations/connections/:id",
            get(connections::get_connection),
        )
        .route(
            "/api/v1/integrations/connections/:id",
            delete(connections::disconnect),
        )
        .route(
            "/api/v1/integrations/connections/:id/sync",
            post(connections::trigger_sync),
        )
        .route(
            "/api/v1/integrations/connections/:id/inbox-history",
            post(connections::extend_inbox_history),
        )
        // Sync jobs — static before param
        .route(
            "/api/v1/integrations/sync-jobs",
            get(sync_jobs::list_sync_jobs),
        )
        .route(
            "/api/v1/integrations/sync-jobs/:id",
            get(sync_jobs::get_sync_job),
        )
        // Profile projection
        .route(
            "/api/v1/integrations/profile",
            get(profile::integration_profile),
        )
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}
