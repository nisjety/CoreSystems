mod api_keys;
mod preferences;
mod session;
mod settings_handlers;
mod shared;

use axum::{
    routing::{delete, get, patch, post, put},
    Router,
};

use crate::{config::AppState, middleware::require_session};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        // Settings (AI/workflow policies)
        .route("/api/v1/settings/:key", get(settings_handlers::get_setting))
        .route("/api/v1/settings/:key", put(settings_handlers::put_setting))
        // Preferences
        .route("/api/v1/preferences", get(preferences::get_preferences))
        .route(
            "/api/v1/preferences",
            patch(preferences::update_preferences),
        )
        // API keys
        .route("/api/v1/api-keys", get(api_keys::list_api_keys))
        .route("/api/v1/api-keys", post(api_keys::create_api_key))
        .route("/api/v1/api-keys/:id", delete(api_keys::delete_api_key))
        // Session refresh — `/session/current` + `/api/v1/me` are owned by domains::auth
        .route("/api/v1/session/refresh", post(session::session_refresh))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}
