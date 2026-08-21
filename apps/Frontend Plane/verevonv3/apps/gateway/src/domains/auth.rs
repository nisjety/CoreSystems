mod protected;
mod public;
mod shared;

/// Reused by the chat path to give the model verified org identity context.
pub(crate) use protected::resolve_org_name;

use axum::{
    routing::{get, post},
    Router,
};

use crate::{config::AppState, middleware::require_session};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    // Public routes: no session required (create/validate sessions).
    let public_routes = Router::new()
        // Better Auth sends provider round-trips to these public paths. They
        // stay on the Verevon origin and are forwarded by a bounded, opaque
        // callback proxy; session middleware must not intercept them.
        .route("/api/auth/callback/{provider}", get(public::oauth_callback))
        .route(
            "/api/auth/sso/callback/{provider}",
            get(public::sso_oidc_callback),
        )
        .route(
            "/api/auth/sso/saml2/callback/{provider}",
            get(public::sso_saml_callback_get).post(public::sso_saml_callback_post),
        )
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
            "/api/v1/auth/email-verification/otp/send",
            post(public::send_email_verification_otp),
        )
        .route(
            "/api/v1/auth/email-verification/otp/verify",
            post(public::verify_email_verification_otp),
        )
        .route(
            "/api/v1/auth/phone-verification/otp/send",
            post(public::send_phone_verification_otp),
        )
        .route(
            "/api/v1/auth/phone-verification/otp/verify",
            post(public::verify_phone_verification_otp),
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
        .route("/api/v1/auth/oauth/{provider}", get(public::oauth_initiate))
        .route("/api/v1/auth/sso/initiate", get(public::sso_initiate));

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
        // Two-factor (TOTP) enrollment — session-bound, enable + confirm + codes.
        .route(
            "/api/v1/auth/2fa/enable",
            post(protected::two_factor_enable),
        )
        .route(
            "/api/v1/auth/2fa/get-totp-uri",
            post(protected::two_factor_get_totp_uri),
        )
        .route(
            "/api/v1/auth/2fa/verify-totp",
            post(protected::two_factor_verify_totp),
        )
        .route(
            "/api/v1/auth/2fa/generate-backup-codes",
            post(protected::two_factor_generate_backup_codes),
        )
        // Platform super-admin: cross-org user directory (all users, every org).
        // Role-gated inside the handler; auth-core re-checks via its admin plugin.
        .route("/api/v1/admin/users", get(protected::admin_list_users))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session));

    Router::new().merge(public_routes).merge(protected_routes)
}
