use axum::{
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

use crate::{config::AppState, envelope::error};

const STRIPPED_HEADERS: &[&str] = &[
    "x-user-id",
    "x-user-email",
    "x-user-name",
    "x-user-avatar",
    "x-user-role",
    "x-user-roles",
    "x-session-user-id",
    "x-session-user-email",
    "x-session-user-name",
    "x-session-user-avatar",
    "x-session-user-role",
    "x-auth-user-id",
    "x-auth-user-email",
    "x-auth-user-name",
    "x-auth-user-avatar",
    "x-auth-role",
    "x-internal-api-key",
    "x-internal-request",
    "x-org-id",
    // Client-controlled tenant scoping must never be trusted: the org id is
    // derived server-side from the validated session (`authorized_org_id`).
    "x-velion-org-id",
];

/// User identity extracted from a validated Better Auth session.
#[derive(Clone, Debug)]
pub(crate) struct AuthenticatedUser {
    pub(crate) user_id: String,
    pub(crate) user_email: String,
    pub(crate) user_name: String,
    pub(crate) user_image: Option<String>,
    pub(crate) email_verified: bool,
    pub(crate) auth_role: Option<String>,
    /// The session's active organization (Better Auth `activeOrganizationId`) — the
    /// org the user is currently acting as (set via `/organization/set-active`).
    /// `None` until they switch (or before the org plugin sets a default); callers
    /// fall back to the primary-org membership from user-core's session-context.
    pub(crate) active_org_id: Option<String>,
}

#[derive(Deserialize)]
struct SessionValidationResponse {
    user: Option<UserFields>,
    session: Option<SessionFields>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionFields {
    active_organization_id: Option<String>,
}

#[derive(Deserialize)]
struct UserFields {
    id: String,
    email: String,
    name: Option<String>,
    image: Option<String>,
    role: Option<String>,
    #[serde(default, rename = "emailVerified")]
    email_verified: bool,
}

/// Remove every [`STRIPPED_HEADERS`] entry from a header map. Header-name
/// matching is case-insensitive (HTTP normalizes names), so a forged
/// `X-Velion-Org-Id` is dropped just like `x-velion-org-id`.
fn strip_identity_headers(headers: &mut HeaderMap) {
    for name in STRIPPED_HEADERS {
        headers.remove(*name);
    }
}

/// Strip identity/internal headers that must never arrive from the browser.
/// Applied globally before any routing so no handler ever sees spoofed identity.
pub(crate) async fn strip_inbound_identity_headers(mut request: Request, next: Next) -> Response {
    strip_identity_headers(request.headers_mut());
    next.run(request).await
}

/// Validate the session cookie via auth-core and inject AuthenticatedUser into request extensions.
/// Returns 401 if no cookie is present or auth-core rejects the session.
pub(crate) async fn require_session(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    if let Some(user) = dev_bypass_user(&state, request.headers()) {
        stamp_trusted_identity(&mut request, &user);
        request.extensions_mut().insert(user);
        return next.run(request).await;
    }

    let cookie_header = request
        .headers()
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();

    match validate_session_cookie(&state, &cookie_header).await {
        Some(user) => {
            stamp_trusted_identity(&mut request, &user);
            request.extensions_mut().insert(user);
            next.run(request).await
        }
        None => (
            StatusCode::UNAUTHORIZED,
            Json(error("unauthorized", "Authentication required")),
        )
            .into_response(),
    }
}

fn dev_bypass_user(state: &AppState, headers: &axum::http::HeaderMap) -> Option<AuthenticatedUser> {
    if !state.allow_dev_auth_bypass {
        return None;
    }

    let authorized = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim() == "Bearer dev-bypass")
        .unwrap_or(false);
    if !authorized {
        return None;
    }

    Some(AuthenticatedUser {
        user_id: "velion-v3-local-user".to_owned(),
        user_email: "local@velion.dev".to_owned(),
        user_name: "Dev".to_owned(),
        user_image: None,
        email_verified: true,
        auth_role: Some("admin".to_owned()),
        active_org_id: None,
    })
}

/// Re-stamp trusted identity headers from the *validated* session.
///
/// `strip_inbound_identity_headers` removes any browser-supplied `x-session-user-*`
/// on ingress. Re-adding them here is safe because the values now originate from
/// auth-core, not the client — this lets header-based handlers (the onboarding flow)
/// resolve the real user via `actor_from_headers` without each handler having to
/// extract the `AuthenticatedUser` extension. These headers never leave the gateway:
/// `proxy_json` builds fresh upstream requests and does not forward inbound headers.
///
/// Non-ASCII names fail `HeaderValue::from_str` and are simply skipped — the
/// user id (always ASCII) is the only field the trusted-actor path requires.
fn stamp_trusted_identity(request: &mut Request, user: &AuthenticatedUser) {
    let headers = request.headers_mut();
    if let Ok(value) = HeaderValue::from_str(&user.user_id) {
        headers.insert("x-session-user-id", value);
    }
    if let Ok(value) = HeaderValue::from_str(&user.user_email) {
        headers.insert("x-session-user-email", value);
    }
    if let Ok(value) = HeaderValue::from_str(&user.user_name) {
        headers.insert("x-session-user-name", value);
    }
    if let Some(image) = user
        .user_image
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if let Ok(value) = HeaderValue::from_str(image) {
            headers.insert("x-session-user-avatar", value);
        }
    }
    if let Some(role) = user
        .auth_role
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        if let Ok(value) = HeaderValue::from_str(role) {
            headers.insert("x-session-user-role", value);
        }
    }
}

pub(crate) async fn validate_session_cookie(
    state: &AppState,
    cookie_header: &str,
) -> Option<AuthenticatedUser> {
    if cookie_header.is_empty() {
        return None;
    }

    let resp = state
        .client
        .get(format!("{}/api/auth/get-session", state.auth_core_url))
        .header("cookie", cookie_header)
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let data = resp.json::<SessionValidationResponse>().await.ok()?;
    let active_org_id = data
        .session
        .and_then(|s| s.active_organization_id)
        .map(|id| id.trim().to_owned())
        .filter(|id| !id.is_empty());
    let user = data.user?;

    Some(AuthenticatedUser {
        user_id: user.id,
        user_email: user.email,
        user_name: user.name.unwrap_or_default(),
        user_image: user.image,
        email_verified: user.email_verified,
        auth_role: user.role,
        active_org_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    /// Tier-1: the ingress strip set must drop a client-forged tenant-scoping
    /// header (`x-velion-org-id`) — the root of the cross-tenant IDOR — along
    /// with `x-org-id`, while leaving unrelated headers intact. Case-insensitive.
    #[test]
    fn strips_forged_org_scoping_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("x-velion-org-id", HeaderValue::from_static("org-victim"));
        headers.insert("X-Velion-Org-Id", HeaderValue::from_static("org-victim"));
        headers.insert("x-org-id", HeaderValue::from_static("org-victim"));
        headers.insert("cookie", HeaderValue::from_static("session=abc"));

        strip_identity_headers(&mut headers);

        assert!(
            headers.get("x-velion-org-id").is_none(),
            "x-velion-org-id must be stripped at ingress"
        );
        assert!(
            headers.get("x-org-id").is_none(),
            "x-org-id must be stripped at ingress"
        );
        assert_eq!(
            headers.get("cookie").map(|v| v.to_str().unwrap()),
            Some("session=abc"),
            "non-identity headers must survive"
        );
    }

    #[test]
    fn stripped_headers_includes_velion_org_id() {
        assert!(STRIPPED_HEADERS.contains(&"x-velion-org-id"));
        assert!(STRIPPED_HEADERS.contains(&"x-org-id"));
    }
}
