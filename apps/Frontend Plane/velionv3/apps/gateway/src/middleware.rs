use axum::{
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::{config::AppState, envelope::error};

/// TTL for a cached *positive* session validation, keyed on a hash of the raw
/// session cookie. Short by design: a cached entry can outlive a server-side
/// revocation/expiry by at most this window, so we keep it to a few seconds.
/// Overridable via `GATEWAY_SESSION_CACHE_TTL_SECS` and clamped to [5, 15] so a
/// misconfiguration can never extend a revoked session for an unsafe duration.
const SESSION_VALIDATION_TTL_DEFAULT_SECS: u64 = 10;
const SESSION_VALIDATION_TTL_MIN_SECS: u64 = 5;
const SESSION_VALIDATION_TTL_MAX_SECS: u64 = 15;

/// Resolve the configured session-validation cache TTL, clamped to the safe band.
fn session_validation_ttl_secs() -> u64 {
    std::env::var("GATEWAY_SESSION_CACHE_TTL_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(SESSION_VALIDATION_TTL_DEFAULT_SECS)
        .clamp(SESSION_VALIDATION_TTL_MIN_SECS, SESSION_VALIDATION_TTL_MAX_SECS)
}

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
///
/// `Serialize`/`Deserialize` so a *positive* validation can round-trip through
/// the short-TTL session-validation cache (see [`validate_session_cookie`]).
#[derive(Clone, Debug, Serialize, Deserialize)]
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
    // A real, validated Better Auth session is ALWAYS authoritative and is
    // resolved first. The dev-auth bypass must never override a logged-in user:
    // if it did, a `Bearer dev-bypass` header (which the SPA injects whenever
    // VITE_ALLOW_DEV_AUTH_BYPASS is on) would collapse every caller — including
    // real, distinct-tenant logins — onto the single shared dev identity, so
    // different organizations would read each other's data (cross-tenant leak).
    let cookie_header = request
        .headers()
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();

    if let Some(user) = validate_session_cookie(&state, &cookie_header).await {
        record_tenant_observability(&user);
        stamp_trusted_identity(&mut request, &user);
        request.extensions_mut().insert(user);
        return next.run(request).await;
    }

    // Dev-only fallback: applies ONLY when there is no valid session and
    // ALLOW_DEV_AUTH_BYPASS is explicitly enabled (see `dev_bypass_user`). It
    // can never downgrade or impersonate an authenticated user.
    if let Some(user) = dev_bypass_user(&state, request.headers()) {
        record_tenant_observability(&user);
        stamp_trusted_identity(&mut request, &user);
        request.extensions_mut().insert(user);
        return next.run(request).await;
    }

    (
        StatusCode::UNAUTHORIZED,
        Json(error("unauthorized", "Authentication required")),
    )
        .into_response()
}

/// Emit per-tenant observability for a validated request (Phase 6 B13): an
/// org/tenant-labeled Prometheus counter plus a tracing event carrying the
/// org/tenant + user, so a request can be traced through the cores by tenant.
/// The org is the authoritative id from the validated session ("none" if unset).
fn record_tenant_observability(user: &AuthenticatedUser) {
    let org = user.active_org_id.as_deref().unwrap_or("none");
    crate::observability::record_authenticated_request(org);
    tracing::info!(
        org_id = %org,
        tenant = %org,
        user_id = %user.user_id,
        "gateway authenticated request"
    );
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

    // B1: collapse the per-request auth-core `/get-session` fan-out. Every
    // authenticated gateway request runs through `require_session`, which used to
    // call auth-core once per request — a single SPA page load fans out ~10-15
    // authed calls, so a busy tab hit auth-core's Better Auth get-session rate
    // limit (the 429→401 cascade the rate-limit raise only masked).
    //
    // We cache the *positive* validation result keyed on a hash of the exact
    // session cookie, for a short TTL (5-15s). Safety properties:
    //   * Key is the cookie hash → a different cookie never reads another's entry,
    //     and a rotated/cleared cookie misses the cache and re-validates live.
    //   * Only positive validations are cached, and only briefly — a revoked or
    //     expired session is honored within at most one TTL window, after which
    //     the entry ages out and the next request re-validates against auth-core.
    //   * Negative results are never cached, so a freshly-signed-in cookie is
    //     never pinned to a stale "unauthenticated" answer.
    //   * Degrade-safe: a disabled/unreachable cache simply means every request
    //     validates live, exactly as before.
    let ttl = session_validation_ttl_secs();
    let cache_key = crate::cache::cache_key("session-validation", &[cookie_header]);
    if let Some(cached) = state.cache.lookup_within(&cache_key, ttl).await {
        if let Ok(user) = serde_json::from_value::<AuthenticatedUser>(cached) {
            return Some(user);
        }
        // A malformed/legacy cache entry: fall through to a live validation.
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

    let authenticated = AuthenticatedUser {
        user_id: user.id,
        user_email: user.email,
        user_name: user.name.unwrap_or_default(),
        user_image: user.image,
        email_verified: user.email_verified,
        auth_role: user.role,
        active_org_id,
    };

    // Cache only this positive validation, for the short TTL. Failures are
    // ignored inside `store_for_secs` (degrade-safe).
    if let Ok(value) = serde_json::to_value(&authenticated) {
        state.cache.store_for_secs(&cache_key, &value, ttl).await;
    }

    Some(authenticated)
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

    /// B1: the session-validation cache TTL must stay inside the safe [5, 15]s
    /// band regardless of env input, so a misconfiguration can never extend a
    /// revoked session for an unsafe duration. Serial because it mutates a
    /// process-global env var.
    #[test]
    fn session_validation_ttl_is_clamped_to_safe_band() {
        let key = "GATEWAY_SESSION_CACHE_TTL_SECS";
        let prev = std::env::var(key).ok();

        std::env::remove_var(key);
        assert_eq!(
            super::session_validation_ttl_secs(),
            super::SESSION_VALIDATION_TTL_DEFAULT_SECS,
            "unset → default"
        );

        std::env::set_var(key, "3");
        assert_eq!(super::session_validation_ttl_secs(), 5, "below floor → floor");

        std::env::set_var(key, "120");
        assert_eq!(super::session_validation_ttl_secs(), 15, "above ceiling → ceiling");

        std::env::set_var(key, "8");
        assert_eq!(super::session_validation_ttl_secs(), 8, "in-band → as-is");

        std::env::set_var(key, "not-a-number");
        assert_eq!(
            super::session_validation_ttl_secs(),
            super::SESSION_VALIDATION_TTL_DEFAULT_SECS,
            "unparseable → default"
        );

        match prev {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }

    /// B1: a positive `AuthenticatedUser` must round-trip through serde so the
    /// cache store→lookup path returns an identical identity.
    #[test]
    fn authenticated_user_round_trips_through_serde() {
        let user = AuthenticatedUser {
            user_id: "u_1".into(),
            user_email: "a@b.no".into(),
            user_name: "Alice".into(),
            user_image: Some("https://img".into()),
            email_verified: true,
            auth_role: Some("admin".into()),
            active_org_id: Some("org_1".into()),
        };
        let value = serde_json::to_value(&user).expect("serialize");
        let back: AuthenticatedUser = serde_json::from_value(value).expect("deserialize");
        assert_eq!(back.user_id, user.user_id);
        assert_eq!(back.user_email, user.user_email);
        assert_eq!(back.active_org_id, user.active_org_id);
        assert_eq!(back.email_verified, user.email_verified);
    }
}
