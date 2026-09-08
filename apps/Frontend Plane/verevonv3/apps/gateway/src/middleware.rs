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

/// The outcome of checking a browser session cookie against auth-core.
///
/// The split between `Rejected` and `Unavailable` is the entire point of this
/// type. Only auth-core *answering* "there is no session behind this cookie" may
/// end the browser's session. Failing to reach auth-core at all — a timeout, a
/// connection reset, a 429, a 5xx — says nothing whatsoever about the user, and
/// treating it as a rejection signs out people whose sessions are perfectly
/// valid. That conflation is exactly what made a healthy 7-day session drop a
/// user back to the login screen every few minutes: every authenticated request
/// re-validates live, so a single unlucky round trip anywhere in the app was
/// enough to clear the session.
pub(crate) enum SessionValidation {
    /// auth-core resolved the cookie to a live session.
    Valid(AuthenticatedUser),
    /// auth-core answered, and the answer was "no session". Authoritative.
    Rejected,
    /// auth-core could not be consulted, so the session's real state is unknown.
    /// Callers must preserve whatever the client already has.
    Unavailable,
}

/// Resolve the configured session-validation cache TTL, clamped to the safe band.
fn session_validation_ttl_secs() -> u64 {
    std::env::var("GATEWAY_SESSION_CACHE_TTL_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(SESSION_VALIDATION_TTL_DEFAULT_SECS)
        .clamp(
            SESSION_VALIDATION_TTL_MIN_SECS,
            SESSION_VALIDATION_TTL_MAX_SECS,
        )
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
    "x-verevon-org-id",
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
    /// `None` until they switch (or before the org plugin sets a default). This is
    /// requested scope only; sensitive callers use `authorized_membership` below.
    pub(crate) active_org_id: Option<String>,
    /// Live user-core/Better Auth membership for `active_org_id`. This is never
    /// read from the session-validation cache: sensitive routes populate it from
    /// the canonical authority on every request so removals take effect promptly.
    #[serde(skip)]
    pub(crate) authorized_membership: Option<AuthorizedMembership>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AuthorizedMembership {
    pub(crate) organization_id: String,
    pub(crate) role: String,
}

/// Check an organization role only against the live membership decision added
/// by `require_session`. Cached session context and the platform-level Auth role
/// are presentation data and must never grant tenant administration authority.
pub(crate) fn has_authorized_org_role(user: &AuthenticatedUser, allowed_roles: &[&str]) -> bool {
    let Some(active_org_id) = user
        .active_org_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return false;
    };
    let Some(membership) = user.authorized_membership.as_ref() else {
        return false;
    };
    membership.organization_id.trim() == active_org_id
        && allowed_roles
            .iter()
            .any(|role| membership.role.trim().eq_ignore_ascii_case(role))
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
/// `X-Verevon-Org-Id` is dropped just like `x-verevon-org-id`.
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
    request: Request,
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

    let allow_cached_session = allows_cached_session_validation(request.uri().path());
    let validation = validate_session_cookie(&state, &cookie_header, allow_cached_session).await;
    // Read this before the `Valid` arm moves the user out of `validation`.
    let verification_unavailable = matches!(validation, SessionValidation::Unavailable);
    if let SessionValidation::Valid(user) = validation {
        return authorize_request(state, request, next, user).await;
    }

    // Dev-only fallback: applies ONLY when there is no valid session and
    // ALLOW_DEV_AUTH_BYPASS is explicitly enabled (see `dev_bypass_user`). It
    // can never downgrade or impersonate an authenticated user.
    if let Some(user) = dev_bypass_user(&state, request.headers()) {
        return authorize_request(state, request, next, user).await;
    }

    // We could not consult auth-core, so we do not know whether this session is
    // still good. Answering 401 here would be a lie the browser acts on: the SPA
    // treats a 401 `unauthorized` as proof of expiry and clears the session, so a
    // momentary auth-core blip would sign out a user whose session is untouched
    // and valid for days. 503 says "ask again", which is the truth, and leaves
    // the client's session alone.
    if verification_unavailable {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(error(
                "session_verification_unavailable",
                "Session verification is temporarily unavailable. Your session is unchanged — retry shortly.",
            )),
        )
            .into_response();
    }

    (
        StatusCode::UNAUTHORIZED,
        Json(error("unauthorized", "Authentication required")),
    )
        .into_response()
}

async fn authorize_request(
    state: AppState,
    mut request: Request,
    next: Next,
    mut user: AuthenticatedUser,
) -> Response {
    let membership_required = requires_active_membership(request.uri().path());
    let membership_optional = optionally_resolves_active_membership(request.uri().path());
    let has_requested_org = user
        .active_org_id
        .as_deref()
        .map(str::trim)
        .is_some_and(|org_id| !org_id.is_empty());
    if membership_required || (membership_optional && has_requested_org) {
        match crate::upstream::resolve_active_membership(&state, &user).await {
            crate::upstream::ActiveMembershipResolution::Member(membership) => {
                // Tenant-facing downstreams historically read `auth_role` when
                // constructing their actor headers. Replace the platform/session
                // role with the exact organization role for this request so every
                // shared caller inherits canonical user/org/role scoping.
                user.auth_role = Some(membership.role.clone());
                user.authorized_membership = Some(membership);
            }
            crate::upstream::ActiveMembershipResolution::Missing => {
                if membership_required {
                    return (
                        StatusCode::FORBIDDEN,
                        Json(error(
                            "organization_membership_required",
                            "An explicit active organization membership is required.",
                        )),
                    )
                        .into_response();
                }
                // Bootstrap endpoints must remain usable after removal so the
                // caller can select another organization. Do not expose or proxy
                // the stale active scope as if it were still authorized.
                user.active_org_id = None;
            }
            crate::upstream::ActiveMembershipResolution::AuthorityUnavailable => {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(error(
                        "organization_membership_authority_unavailable",
                        "The organization membership authority is unavailable.",
                    )),
                )
                    .into_response();
            }
        }
    }

    record_tenant_observability(&user);
    stamp_trusted_identity(&mut request, &user);
    request.extensions_mut().insert(user);
    next.run(request).await
}

/// Session-only routes are the narrowly-scoped flows needed before an active
/// organization exists (or to select one). Everything else is tenant-sensitive
/// and requires a fresh canonical membership decision.
fn requires_active_membership(path: &str) -> bool {
    if path == "/api/v1/session/current"
        || path == "/api/v1/me"
        || path == "/api/v1/me/session-context"
        || path == "/api/v1/admin/users"
        || path == "/api/v1/orgs"
        || path == "/api/v1/orgs/switch-active"
        || path == "/api/v1/onboarding/lifecycle"
        || path.starts_with("/api/v1/auth/2fa/")
        || is_pre_org_onboarding_route(path)
    {
        return false;
    }

    let invitation = path
        .strip_prefix("/api/v1/orgs/invitations/")
        .and_then(|rest| rest.strip_suffix("/accept"));
    !matches!(invitation, Some(id) if !id.is_empty() && !id.contains('/'))
}

/// Exact onboarding routes that operate only on the authenticated user or on
/// public/transient preview data before an organization exists. Any onboarding
/// route not listed here is tenant-bearing and must pass the live membership
/// authority before its handler runs.
fn is_pre_org_onboarding_route(path: &str) -> bool {
    matches!(
        path,
        "/api/v1/session/bootstrap"
            | "/api/v1/onboarding/status"
            | "/api/v1/onboarding/state"
            | "/api/v1/onboarding/theme"
            | "/api/v1/onboarding/brreg/search"
            | "/api/v1/onboarding/crawl-preview"
            | "/api/v1/onboarding/recommend-plan"
            | "/api/v1/onboarding/translate-recommendation"
            | "/api/v1/onboarding/actions/create-organization"
    )
}

/// Session bootstrap is usable without an organization, but an active scope—if
/// present—must be checked live before it can influence returned org data.
/// The onboarding lifecycle probe is in the same class: without an org it
/// honestly reports CREATED, and with one the live membership decision (plus
/// the handler's own canonical checks) gates the org-scoped answer.
fn optionally_resolves_active_membership(path: &str) -> bool {
    matches!(
        path,
        "/api/v1/session/current" | "/api/v1/me/session-context" | "/api/v1/onboarding/lifecycle"
    )
}

/// Cache only the organization-list bootstrap request, whose response and
/// authorization do not depend on the session's active tenant. Tenant-bearing,
/// role-bearing, and session-mutation routes always validate the current
/// server-side session live, preventing a concurrent org switch from reusing or
/// re-storing stale scope.
fn allows_cached_session_validation(path: &str) -> bool {
    path == "/api/v1/orgs"
}

/// Emit per-tenant observability for a validated request (Phase 6 B13): an
/// org/tenant-labeled Prometheus counter plus a tracing event carrying the
/// org/tenant + user, so a request can be traced through the cores by tenant.
/// The org is the live canonical membership id ("none" on session-only flows).
fn record_tenant_observability(user: &AuthenticatedUser) {
    let org = user
        .authorized_membership
        .as_ref()
        .map(|membership| membership.organization_id.as_str())
        .unwrap_or("none");
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
        user_id: "verevon-v3-local-user".to_owned(),
        user_email: "local@verevon.dev".to_owned(),
        user_name: "Dev".to_owned(),
        user_image: None,
        email_verified: true,
        auth_role: Some("admin".to_owned()),
        active_org_id: None,
        authorized_membership: None,
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
    allow_cached: bool,
) -> SessionValidation {
    if cookie_header.is_empty() {
        // No cookie at all is a definitive answer about the caller, not an outage.
        return SessionValidation::Rejected;
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
    if allow_cached {
        if let Some(cached) = state.cache.lookup_within(&cache_key, ttl).await {
            if let Ok(user) = serde_json::from_value::<AuthenticatedUser>(cached) {
                return SessionValidation::Valid(user);
            }
            // A malformed/legacy cache entry: fall through to a live validation.
        }
    }

    let resp = match state
        .client
        .get(format!("{}/api/auth/get-session", state.auth_core_url))
        .header("cookie", cookie_header)
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(err) => {
            tracing::warn!(
                error = %err,
                "session validation could not reach auth-core; preserving the caller's session"
            );
            return SessionValidation::Unavailable;
        }
    };

    let status = resp.status();
    if !status.is_success() {
        // Only an explicit auth rejection is authoritative. A 429 (rate limit),
        // a 5xx, or a misrouted 404 all mean we failed to *ask* — not that the
        // caller is signed out — so they must never clear a session.
        if matches!(status.as_u16(), 401 | 403) {
            return SessionValidation::Rejected;
        }
        tracing::warn!(
            status = status.as_u16(),
            "auth-core could not answer session validation; preserving the caller's session"
        );
        return SessionValidation::Unavailable;
    }

    // Deserialize through `Option`: Better Auth answers a cookie with no live
    // session as a bare `null` body, which serde cannot fold into the struct.
    // Reading it as `Option` keeps that answer a *rejection* — decoding it as a
    // struct fails, and calling that failure an outage would leave a genuinely
    // signed-out browser unable to ever be signed out.
    let data = match resp.json::<Option<SessionValidationResponse>>().await {
        Ok(Some(data)) => data,
        Ok(None) => return SessionValidation::Rejected,
        Err(err) => {
            tracing::warn!(
                error = %err,
                "auth-core session response was unreadable; preserving the caller's session"
            );
            return SessionValidation::Unavailable;
        }
    };
    let active_org_id = data
        .session
        .and_then(|s| s.active_organization_id)
        .map(|id| id.trim().to_owned())
        .filter(|id| !id.is_empty());
    // Better Auth answers a cookie with no live session as 200 + a null user.
    // That is a real answer, so it is a rejection rather than an outage.
    let Some(user) = data.user else {
        return SessionValidation::Rejected;
    };

    let authenticated = AuthenticatedUser {
        user_id: user.id,
        user_email: user.email,
        user_name: user.name.unwrap_or_default(),
        user_image: user.image,
        email_verified: user.email_verified,
        auth_role: user.role,
        active_org_id,
        authorized_membership: None,
    };

    // Cache only this positive validation, for the short TTL. Failures are
    // ignored inside `store_for_secs` (degrade-safe).
    if allow_cached {
        if let Ok(value) = serde_json::to_value(&authenticated) {
            state.cache.store_for_secs(&cache_key, &value, ttl).await;
        }
    }

    SessionValidation::Valid(authenticated)
}

/// Remove the short-lived positive session validation after a successful
/// server-side session mutation (for example, changing the active org). Better
/// Auth can persist that mutation without changing the session cookie, so
/// leaving the cookie-keyed entry in place would retain the previous org until
/// the validation TTL expires.
pub(crate) async fn invalidate_session_validation_cache(state: &AppState, cookie_header: &str) {
    if cookie_header.trim().is_empty() {
        return;
    }
    let cache_key = crate::cache::cache_key("session-validation", &[cookie_header]);
    state.cache.delete(&cache_key).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use serde_json::json;
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Drive one authenticated request through the real router against an
    /// auth-core that answers `get-session` with `template`, and report the
    /// status plus the machine-readable error code the browser would see.
    async fn session_probe(template: ResponseTemplate) -> (u16, String) {
        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(template)
            .mount(&auth)
            .await;
        probe_against_auth_core(auth.uri()).await
    }

    async fn probe_against_auth_core(auth_core_url: String) -> (u16, String) {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth_core_url;
        let response = crate::build_router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/v1/navbar")
                    .header("cookie", "better-auth.session_token=session-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status().as_u16();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap_or(json!({}));
        let code = parsed["error"]["code"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        (status, code)
    }

    /// The logout regression: auth-core being briefly unable to answer must not
    /// be reported to the browser as "you are signed out". The SPA clears its
    /// session on a 401 `unauthorized`, so returning that here would sign out a
    /// user whose session is untouched and valid for days.
    #[tokio::test]
    async fn auth_core_outage_does_not_sign_the_user_out() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, code) = session_probe(ResponseTemplate::new(503)).await;
        assert_eq!(status, 503, "an auth-core outage must not answer 401");
        assert_eq!(code, "session_verification_unavailable");
    }

    /// A rate-limited validation is the documented 429→401 cascade. It says
    /// nothing about the session and must never end it.
    #[tokio::test]
    async fn rate_limited_session_validation_does_not_sign_the_user_out() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, code) = session_probe(ResponseTemplate::new(429)).await;
        assert_eq!(status, 503, "a 429 from auth-core must not answer 401");
        assert_eq!(code, "session_verification_unavailable");
    }

    /// An unreadable body means we never learned the answer, so it is an outage
    /// rather than a rejection.
    #[tokio::test]
    async fn unreadable_auth_core_response_does_not_sign_the_user_out() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, code) =
            session_probe(ResponseTemplate::new(200).set_body_string("not json")).await;
        assert_eq!(status, 503);
        assert_eq!(code, "session_verification_unavailable");
    }

    /// An unreachable auth-core is a transport failure, not a verdict.
    #[tokio::test]
    async fn unreachable_auth_core_does_not_sign_the_user_out() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        // Port 1 refuses immediately, so this exercises the `send()` error arm.
        let (status, code) = probe_against_auth_core("http://127.0.0.1:1".to_owned()).await;
        assert_eq!(status, 503);
        assert_eq!(code, "session_verification_unavailable");
    }

    /// The other half of the contract: a real sign-out must still sign the user
    /// out. Better Auth answers a dead cookie with 200 + a null user, and that
    /// IS an answer, so it must still produce the 401 the SPA acts on.
    #[tokio::test]
    async fn auth_core_reporting_no_session_still_signs_the_user_out() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, code) =
            session_probe(ResponseTemplate::new(200).set_body_json(json!({"user": null}))).await;
        assert_eq!(
            status, 401,
            "a genuine 'no session' answer must still end the session"
        );
        assert_eq!(code, "unauthorized");
    }

    /// Better Auth's real "no session" answer is a bare `null` body, not
    /// `{"user": null}` — verified live against auth-core. A struct-shaped mock
    /// misses it, and reading that body as a struct fails to decode, so treating
    /// a decode failure as an outage would make a signed-out browser
    /// un-sign-out-able: every request would answer 503 forever.
    #[tokio::test]
    async fn a_bare_null_body_is_a_rejection_not_an_outage() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, code) =
            session_probe(ResponseTemplate::new(200).set_body_json(json!(null))).await;
        assert_eq!(status, 401, "a bare null body means 'no session'");
        assert_eq!(code, "unauthorized");
    }

    /// auth-core explicitly rejecting the cookie is equally authoritative.
    #[tokio::test]
    async fn auth_core_rejecting_the_cookie_still_signs_the_user_out() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, code) = session_probe(ResponseTemplate::new(401)).await;
        assert_eq!(status, 401);
        assert_eq!(code, "unauthorized");
    }

    /// Tier-1: the ingress strip set must drop a client-forged tenant-scoping
    /// header (`x-verevon-org-id`) — the root of the cross-tenant IDOR — along
    /// with `x-org-id`, while leaving unrelated headers intact. Case-insensitive.
    #[test]
    fn strips_forged_org_scoping_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("x-verevon-org-id", HeaderValue::from_static("org-victim"));
        headers.insert("X-Verevon-Org-Id", HeaderValue::from_static("org-victim"));
        headers.insert("x-org-id", HeaderValue::from_static("org-victim"));
        headers.insert("cookie", HeaderValue::from_static("session=abc"));

        strip_identity_headers(&mut headers);

        assert!(
            headers.get("x-verevon-org-id").is_none(),
            "x-verevon-org-id must be stripped at ingress"
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
    fn stripped_headers_includes_verevon_org_id() {
        assert!(STRIPPED_HEADERS.contains(&"x-verevon-org-id"));
        assert!(STRIPPED_HEADERS.contains(&"x-org-id"));
    }

    #[test]
    fn organization_role_checks_use_only_live_authorized_membership() {
        let mut user = AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.test".to_owned(),
            user_name: "User".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("owner".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: None,
        };

        assert!(!has_authorized_org_role(&user, &["owner", "admin"]));
        user.authorized_membership = Some(AuthorizedMembership {
            organization_id: "org-1".to_owned(),
            role: "member".to_owned(),
        });
        assert!(!has_authorized_org_role(&user, &["owner", "admin"]));
        user.authorized_membership = Some(AuthorizedMembership {
            organization_id: "org-1".to_owned(),
            role: "admin".to_owned(),
        });
        assert!(has_authorized_org_role(&user, &["owner", "admin"]));
    }

    #[test]
    fn tenant_and_session_mutation_routes_never_use_cached_session_scope() {
        for path in [
            "/api/v1/knowledge/sources",
            "/api/v1/billing/checkout",
            "/api/v1/mcp/servers",
            "/api/v1/me/session-context",
            "/api/v1/session/current",
            "/api/v1/orgs/switch-active",
        ] {
            assert!(!allows_cached_session_validation(path), "{path}");
        }
        assert!(allows_cached_session_validation("/api/v1/orgs"));
    }

    #[test]
    fn only_explicit_pre_org_flows_are_session_only() {
        for path in [
            "/api/v1/session/current",
            "/api/v1/session/bootstrap",
            "/api/v1/me",
            "/api/v1/me/session-context",
            "/api/v1/auth/2fa/enable",
            "/api/v1/admin/users",
            "/api/v1/onboarding/status",
            "/api/v1/onboarding/state",
            "/api/v1/onboarding/theme",
            "/api/v1/onboarding/brreg/search",
            "/api/v1/onboarding/crawl-preview",
            "/api/v1/onboarding/recommend-plan",
            "/api/v1/onboarding/translate-recommendation",
            "/api/v1/onboarding/actions/create-organization",
            "/api/v1/orgs/switch-active",
            "/api/v1/orgs",
            "/api/v1/orgs/invitations/inv_123/accept",
            "/api/v1/onboarding/lifecycle",
        ] {
            assert!(!requires_active_membership(path), "{path}");
        }
        assert!(optionally_resolves_active_membership(
            "/api/v1/session/current"
        ));
        assert!(optionally_resolves_active_membership(
            "/api/v1/me/session-context"
        ));
        assert!(optionally_resolves_active_membership(
            "/api/v1/onboarding/lifecycle"
        ));
        assert!(!optionally_resolves_active_membership(
            "/api/v1/onboarding/status"
        ));

        for path in [
            "/api/v1/router-policy",
            "/api/v1/notifications",
            "/api/v1/social/catalog",
            "/api/v1/leads/search",
            "/api/v1/inbox/conversations",
            "/api/v1/orgs/org-victim/members",
            "/api/v1/onboarding/complete",
            "/api/v1/onboarding/graph-preview",
            "/api/v1/onboarding/actions/set-plan",
            "/api/v1/onboarding/actions/start-checkout",
            "/api/v1/onboarding/actions/confirm-checkout",
            "/api/v1/onboarding/actions/start-website-ingest",
            "/api/v1/onboarding/actions/start-connect-session",
            "/api/v1/onboarding/actions/discover-source",
            "/api/v1/onboarding/actions/cleanup-source",
            "/api/v1/onboarding/actions/warm-sharepoint-discovery",
            "/api/v1/onboarding/actions/start-integration-sync",
            "/api/v1/onboarding-evil/session",
            "/api/v1/orgs/invitations/inv_123/accept/extra",
        ] {
            assert!(requires_active_membership(path), "{path}");
        }
    }

    /// B1: the session-validation cache TTL must stay inside the safe [5, 15]s
    /// band regardless of env input, so a misconfiguration can never extend a
    /// revoked session for an unsafe duration. Serial because it mutates a
    /// process-global env var.
    #[test]
    fn session_validation_ttl_is_clamped_to_safe_band() {
        // The comment above already notes this var is process-global; the lock
        // is what stops a concurrent test validating a session against the
        // values this test writes.
        let _env = crate::config::TEST_ENV_LOCK.blocking_lock();
        let key = "GATEWAY_SESSION_CACHE_TTL_SECS";
        let prev = std::env::var(key).ok();

        std::env::remove_var(key);
        assert_eq!(
            super::session_validation_ttl_secs(),
            super::SESSION_VALIDATION_TTL_DEFAULT_SECS,
            "unset → default"
        );

        std::env::set_var(key, "3");
        assert_eq!(
            super::session_validation_ttl_secs(),
            5,
            "below floor → floor"
        );

        std::env::set_var(key, "120");
        assert_eq!(
            super::session_validation_ttl_secs(),
            15,
            "above ceiling → ceiling"
        );

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
            authorized_membership: Some(AuthorizedMembership {
                organization_id: "org_1".into(),
                role: "admin".into(),
            }),
        };
        let value = serde_json::to_value(&user).expect("serialize");
        let back: AuthenticatedUser = serde_json::from_value(value).expect("deserialize");
        assert_eq!(back.user_id, user.user_id);
        assert_eq!(back.user_email, user.user_email);
        assert_eq!(back.active_org_id, user.active_org_id);
        assert_eq!(back.email_verified, user.email_verified);
        assert!(
            back.authorized_membership.is_none(),
            "live membership decisions must never round-trip through session cache"
        );
    }
}
