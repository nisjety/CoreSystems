mod actions;
mod lookup;
mod session;

use axum::Router;

use crate::{config::AppState, middleware::require_session};

/// All onboarding routes require a valid Better Auth session. `require_session`
/// validates the cookie, injects `AuthenticatedUser`, and re-stamps trusted
/// identity headers so handlers resolve the real signed-in user instead of the
/// dev-actor fallback.
pub fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .merge(session::router())
        .merge(lookup::router())
        .merge(actions::router())
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}
