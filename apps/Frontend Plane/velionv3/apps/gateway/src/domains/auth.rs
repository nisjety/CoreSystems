mod protected;
mod public;
mod shared;

use axum::{
    routing::{get, post},
    Router,
};

use crate::{config::AppState, middleware::require_session};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    // Public routes: no session required (create/validate sessions).
    let public_routes = Router::new()
        .route("/api/v1/auth/sign-up", post(public::sign_up))
        .route("/api/v1/auth/sign-in", post(public::sign_in))
        .route("/api/v1/auth/2fa/verify", post(public::verify_two_factor))
        .route("/api/v1/auth/sign-out", post(public::sign_out))
        .route("/api/v1/auth/session", get(public::get_auth_session))
        .route(
            "/api/v1/auth/email-verification/send",
            post(public::send_email_verification),
        )
        .route(
            "/api/v1/auth/email-verification/verify",
            post(public::verify_email),
        )
        .route(
            "/api/v1/auth/password/check-strength",
            post(public::check_password_strength),
        )
        .route(
            "/api/v1/auth/password/send-reset",
            post(public::send_password_reset),
        )
        .route("/api/v1/auth/password/reset", post(public::reset_password))
        .route("/api/v1/auth/oauth/:provider", get(public::oauth_initiate));

    // Protected routes: session cookie must be valid.
    let protected_routes = Router::new()
        .route("/api/v1/session/current", get(protected::session_current))
        .route(
            "/api/v1/me",
            get(protected::get_me).patch(protected::patch_me),
        )
        .route(
            "/api/v1/me/session-context",
            get(protected::get_session_context),
        )
        .route_layer(axum::middleware::from_fn_with_state(state, require_session));

    Router::new().merge(public_routes).merge(protected_routes)
}
