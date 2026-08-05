use axum::{
    extract::{Extension, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    envelope::{error, ok, unwrap_data},
    middleware::AuthenticatedUser,
    upstream::{browser_origin, proxy_auth, proxy_json, user_core_delegation_headers},
};

use super::shared::{actor_for, cookie_header};

pub(super) async fn session_current(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    // user-core's session-context, scoped to the session's active org (role,
    // onboardingStatus for the org the user is acting as); cached per
    // (user, active-org) in `upstream` so this is round-trip-free when paired with
    // /me/session-context on bootstrap.
    let ctx = crate::upstream::resolve_session_context(&state, &user).await;

    // Current org = the session's active org (what the user is acting as, set via
    // org-switch) when present, else the primary-org membership. The display name is
    // resolved from org-core for whichever org that is. `role` is the membership role
    // for that same org, so the permission hints below match the active org.
    let org_id = user
        .active_org_id
        .clone()
        .or_else(|| ctx.get("orgId").and_then(Value::as_str).map(str::to_owned));
    let role = ctx
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let permissions = permission_hints(user.auth_role.as_deref(), role.as_str());
    let org = match org_id {
        Some(id) if !id.is_empty() => {
            let name = resolve_org_name(&state, &user, &id).await;
            Some(json!({ "id": id, "name": name, "role": role }))
        }
        _ => None,
    };

    Json(ok(json!({
        "user": {
            "id": user.user_id,
            "email": user.user_email,
            "name": user.user_name,
            "emailVerified": user.email_verified,
            "image": user.user_image,
            "role": user.auth_role,
        },
        "org": org,
        "permissions": permissions,
        "onboardingStatus": ctx
            .get("onboardingStatus")
            .and_then(Value::as_str)
            .unwrap_or("CREATED"),
        "status": "authenticated",
    })))
}

/// Resolve an organization's display name from org-core. Returns an empty string
/// on any failure so session bootstrap never breaks on a name lookup. Also reused
/// by the chat path (see `domains::chat::shared`) to give the model verified
/// org/user identity context — never trust a client-supplied name for that.
pub(crate) async fn resolve_org_name(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
) -> String {
    let (status, Json(body)) = proxy_json(
        state,
        Method::GET,
        &format!(
            "{}/api/v1/organizations/{}",
            state.org_core_url,
            urlencoding::encode(org_id)
        ),
        None,
        Some(org_id),
        Some(&actor_for(user)),
        None,
    )
    .await;
    if !status.is_success() {
        return String::new();
    }
    unwrap_data(&body)
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

pub(super) async fn get_me(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let url = format!("{}/api/v1/users/me", state.user_core_url);
    let avatar = user
        .user_image
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default();
    let actor = actor_for(&user);
    let headers = user_core_delegation_headers(
        &state.user_core_service_token,
        &Method::GET,
        &url,
        &[],
        &actor,
        user.active_org_id.as_deref(),
        avatar,
        chrono::Utc::now(),
    );
    let mut request = state.client.get(url);
    for (name, value) in headers {
        request = request.header(name, value);
    }

    match request.send().await {
        Ok(response) => {
            let status =
                StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let mut body = response.json::<Value>().await.unwrap_or_else(|_| json!({}));
            if status.is_success() {
                merge_session_user_profile(&mut body, &user);
            }
            (status, Json(body))
        }
        Err(_) => (
            StatusCode::BAD_GATEWAY,
            Json(crate::envelope::upstream_unavailable()),
        ),
    }
}

pub(super) async fn patch_me(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    proxy_json(
        &state,
        Method::PATCH,
        &format!("{}/api/v1/users/me", state.user_core_url),
        Some(body),
        None,
        Some(&actor_for(&user)),
        None,
    )
    .await
}

pub(super) async fn get_session_context(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    // Cached per-user in `upstream`; the SPA hits this on bootstrap alongside
    // /session/current, so the second call is served from cache.
    let ctx = crate::upstream::resolve_session_context(&state, &user).await;

    // Current org = the session's active org (if switched) else the primary-org
    // membership — matching the org the gateway scopes operations to.
    let org_id = user
        .active_org_id
        .as_deref()
        .filter(|value| !value.is_empty())
        .or_else(|| ctx.get("orgId").and_then(Value::as_str));
    let role = ctx.get("role").and_then(Value::as_str).unwrap_or_default();
    let permissions = permission_hints(user.auth_role.as_deref(), role);
    let onboarding_status = ctx
        .get("onboardingStatus")
        .and_then(Value::as_str)
        .unwrap_or("CREATED");

    // user-core returns a single primary org; the SPA reads `orgs[0]`. Emit a
    // superset: flat fields for existing readers plus an `orgs` array for the SPA.
    let orgs = match org_id {
        Some(id) => {
            let name = resolve_org_name(&state, &user, id).await;
            json!([{ "id": id, "name": name, "role": role }])
        }
        None => json!([]),
    };

    // resolve_session_context returns Null on failure → the fields below fall back
    // to safe defaults, so this endpoint always answers 200 (as it did before).
    (
        StatusCode::OK,
        Json(ok(json!({
            "userId": ctx
                .get("userId")
                .and_then(Value::as_str)
                .unwrap_or(user.user_id.as_str()),
            "email": user.user_email,
            "name": user.user_name,
            "image": user.user_image,
            "orgId": org_id,
            "role": role,
            "permissions": permissions,
            "onboardingStatus": onboarding_status,
            "orgs": orgs,
        }))),
    )
}

// --- Two-factor (TOTP) enrollment ---------------------------------------
//
// Enrollment is session-bound: the user is already authenticated (require_session
// runs first) and Better Auth's two-factor plugin endpoints operate on the live
// session. `proxy_auth` forwards the session cookie, returns any Set-Cookie, and
// strips session tokens from the response body — so the QR/secret and backup codes
// reach the SPA but the session secret never leaks into JS-readable JSON.

/// Begin TOTP enrollment: returns `{ totpURI, backupCodes }` for the user to
/// scan. Requires the account password (re-auth) per Better Auth's plugin.
pub(super) async fn two_factor_enable(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    proxy_two_factor(&state, &headers, "/api/auth/two-factor/enable", Some(body)).await
}

/// Fetch the otpauth:// TOTP URI for the (already-enabled, unverified) factor.
/// Used to (re)render the QR / manual key without re-running enable.
pub(super) async fn two_factor_get_totp_uri(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    proxy_two_factor(
        &state,
        &headers,
        "/api/auth/two-factor/get-totp-uri",
        Some(body),
    )
    .await
}

/// Confirm enrollment by verifying a TOTP code from the authenticator app.
pub(super) async fn two_factor_verify_totp(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    proxy_two_factor(
        &state,
        &headers,
        "/api/auth/two-factor/verify-totp",
        Some(body),
    )
    .await
}

/// (Re)generate single-use backup codes. Returns `{ backupCodes }`.
pub(super) async fn two_factor_generate_backup_codes(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    proxy_two_factor(
        &state,
        &headers,
        "/api/auth/two-factor/generate-backup-codes",
        Some(body),
    )
    .await
}

async fn proxy_two_factor(
    state: &AppState,
    headers: &HeaderMap,
    path: &str,
    body: Option<Value>,
) -> Response {
    let url = format!("{}{}", state.auth_core_url, path);
    let cookie = cookie_header(headers);
    let origin = browser_origin(headers);
    proxy_auth(
        state,
        Method::POST,
        &url,
        body,
        Some(&cookie),
        origin.as_deref(),
    )
    .await
}

fn merge_session_user_profile(body: &mut Value, user: &AuthenticatedUser) {
    if let Some(data) = body.get_mut("data") {
        merge_session_user_profile(data, user);
        return;
    }

    if let Some(profile) = body.get_mut("user").and_then(Value::as_object_mut) {
        merge_profile_object(profile, user);
        return;
    }

    if let Some(profile) = body.as_object_mut() {
        merge_profile_object(profile, user);
    }
}

fn merge_profile_object(profile: &mut serde_json::Map<String, Value>, user: &AuthenticatedUser) {
    set_string_if_missing(profile, "id", &user.user_id);
    set_string_if_missing(profile, "email", &user.user_email);
    set_string_if_missing(profile, "name", &user.user_name);
    set_string_if_missing(profile, "display_name", &user.user_name);
    set_bool_if_missing(profile, "email_verified", user.email_verified);
    set_bool_if_missing(profile, "emailVerified", user.email_verified);

    if let Some(image) = user
        .user_image
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        set_string_if_missing(profile, "avatarUrl", image);
        set_string_if_missing(profile, "avatar", image);
        set_string_if_missing(profile, "image", image);
    }
}

fn set_string_if_missing(profile: &mut serde_json::Map<String, Value>, key: &str, value: &str) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return;
    }

    let has_value = profile
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .map(|current| !current.is_empty())
        .unwrap_or(false);
    if !has_value {
        profile.insert(key.to_owned(), Value::String(trimmed.to_owned()));
    }
}

fn set_bool_if_missing(profile: &mut serde_json::Map<String, Value>, key: &str, value: bool) {
    if !profile.get(key).is_some_and(Value::is_boolean) {
        profile.insert(key.to_owned(), Value::Bool(value));
    }
}

fn permission_hints(auth_role: Option<&str>, org_role: &str) -> Vec<&'static str> {
    let mut permissions = vec!["profile:read", "profile:update"];
    if has_any_role(org_role, &["owner", "admin"]) {
        permissions.extend(["org:read", "org:members:read", "org:members:update"]);
    }
    if has_any_role(auth_role.unwrap_or_default(), &["admin", "superadmin"]) {
        permissions.extend(["admin:users:read", "admin:users:update"]);
    }
    permissions
}

fn has_any_role(value: &str, allowed: &[&str]) -> bool {
    value.split(',').map(str::trim).any(|role| {
        allowed
            .iter()
            .any(|allowed_role| role.eq_ignore_ascii_case(allowed_role))
    })
}

/// `GET /api/v1/admin/users` — cross-org user directory for platform super
/// admins. Unlike `/api/v1/orgs/{id}/members` (scoped to one org), this lists
/// EVERY user in the deployment by proxying Better Auth's admin `list-users`,
/// which is not org-scoped. Gated here on the top-level Better Auth role
/// (`admin`/`superadmin`); auth-core re-checks the same role via its admin
/// plugin, so this is defense-in-depth, not the sole gate.
pub(super) async fn admin_list_users(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    uri: axum::http::Uri,
    headers: HeaderMap,
) -> Response {
    if !has_any_role(
        user.auth_role.as_deref().unwrap_or_default(),
        &["admin", "superadmin"],
    ) {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "forbidden",
                "Cross-org user administration requires a platform admin role.",
            )),
        )
            .into_response();
    }

    // Forward the client's paging/search query (limit, offset, searchValue,
    // sortBy, …) verbatim to Better Auth's admin list-users.
    let query = uri.query().filter(|q| !q.is_empty());
    let url = match query {
        Some(q) => format!("{}/api/auth/admin/list-users?{q}", state.auth_core_url),
        None => format!(
            "{}/api/auth/admin/list-users?limit=200",
            state.auth_core_url
        ),
    };

    proxy_auth(
        &state,
        Method::GET,
        &url,
        None,
        Some(&cookie_header(&headers)),
        browser_origin(&headers).as_deref(),
    )
    .await
}
