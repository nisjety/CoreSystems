use axum::{
    extract::{Extension, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    envelope::{error, ok, unwrap_data},
    middleware::AuthenticatedUser,
    upstream::proxy_json,
};

use super::shared::actor_for;

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
/// on any failure so session bootstrap never breaks on a name lookup.
async fn resolve_org_name(state: &AppState, user: &AuthenticatedUser, org_id: &str) -> String {
    let (status, Json(body)) = proxy_json(
        state,
        Method::GET,
        &format!(
            "{}/api/v1/organizations/{}",
            state.org_core_url,
            urlencoding::encode(org_id)
        ),
        None,
        None,
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
    let mut request = state
        .client
        .get(format!("{}/api/v1/users/me", state.user_core_url))
        .header("x-internal-api-key", &state.internal_api_key)
        .header("x-user-id", user.user_id.as_str());

    if !user.user_email.trim().is_empty() {
        request = request.header("x-user-email", user.user_email.as_str());
    }
    if !user.user_name.trim().is_empty() {
        request = request.header("x-user-name", user.user_name.as_str());
    }
    if let Some(role) = user
        .auth_role
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        request = request.header("x-user-role", role);
    }
    if let Some(image) = user
        .user_image
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        request = request.header("x-user-avatar", image);
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
        Err(request_error) => (
            StatusCode::BAD_GATEWAY,
            Json(error("upstream_unavailable", request_error.to_string())),
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
