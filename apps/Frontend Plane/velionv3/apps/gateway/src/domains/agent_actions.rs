//! Agent "specialized actions" surface for the chat composer's `/` menu:
//! skills and capabilities. Both are Model-Plane catalogs fronted by
//! model-gateway (`/v1/skills`, `/v1/capabilities`, which it in turn proxies to
//! capability-core). The SPA only ever talks to this gateway, so these proxies
//! are what let the `/` menu surface real, org-scoped skills/capabilities.
//!
//! Reads (skills/capabilities catalogs) are open to any authenticated member so
//! the `/` menu can surface them; activation happens client-side by threading
//! the chosen action into the chat invoke `tools` array. Skill AUTHORING
//! (create/update/delete) is admin-gated here (defense in depth) and mutates the
//! durable org skill registry (model-gateway `/v1/skills` → capability-core),
//! which the chat path injects into the live prompt via `MatchSkills`.

use std::collections::HashMap;

use axum::{
    extract::{Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use reqwest::Method;
use serde_json::Value;

use crate::{
    config::AppState,
    domains::chat::shared,
    envelope::error,
    middleware::{require_session, AuthenticatedUser},
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/skills", get(list_skills).post(create_skill))
        .route(
            "/api/v1/skills/:skill_id",
            post(update_skill).delete(delete_skill),
        )
        .route("/api/v1/plugins", get(list_plugins).post(create_plugin))
        .route(
            "/api/v1/plugins/:plugin_id",
            post(update_plugin).delete(delete_plugin),
        )
        .route("/api/v1/cron", get(list_cron).post(create_cron))
        .route(
            "/api/v1/cron/:cron_id",
            post(update_cron).delete(delete_cron),
        )
        .route("/api/v1/capabilities", get(list_capabilities))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

/// Whether the validated session user may author org skills. Mirrors
/// `mcp.rs::is_org_admin`: a platform `admin`/`superadmin` short-circuits,
/// else the active org's session-context role must be `owner`/`admin`.
/// model-gateway/capability-core re-derive write authority from the verified
/// capability token; this is a defense-in-depth gate that also lets the SPA
/// surface a clear error.
async fn can_author_skills(state: &AppState, user: &AuthenticatedUser) -> bool {
    let role_has = |value: Option<&str>, allowed: &[&str]| {
        value.unwrap_or_default().split(',').map(str::trim).any(|role| {
            allowed
                .iter()
                .any(|allowed_role| role.eq_ignore_ascii_case(allowed_role))
        })
    };
    if role_has(user.auth_role.as_deref(), &["admin", "superadmin"]) {
        return true;
    }
    let org_role = crate::upstream::resolve_session_context(state, user)
        .await
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    role_has(Some(org_role.as_str()), &["owner", "admin"])
}

/// Rebuild a forwardable query string from the incoming params, dropping empty
/// values. Upstream handlers read `q`, `kind`, `limit`, `after_id`.
fn build_query(params: &HashMap<String, String>) -> String {
    let pairs: Vec<String> = params
        .iter()
        .filter(|(_, value)| !value.trim().is_empty())
        .map(|(key, value)| {
            format!(
                "{}={}",
                urlencoding::encode(key),
                urlencoding::encode(value)
            )
        })
        .collect();
    if pairs.is_empty() {
        String::new()
    } else {
        format!("?{}", pairs.join("&"))
    }
}

async fn list_skills(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let token = shared::model_token(&state, &user, &headers).await;
    let capability = match shared::required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return shared::delegated_auth_unavailable(error),
    };
    let url = format!(
        "{}/v1/skills{}",
        state.model_gateway_url,
        build_query(&params)
    );
    shared::proxy_model_json_with_capability(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
}

/// Create a durable org skill. Body (`name`, `description`, `content`,
/// `trigger_keywords`, ...) is forwarded to model-gateway `/v1/skills`
/// (→ capability-core `agent_skills`). Admin-gated; org id is never injected —
/// it is derived from the verified capability token's claims.
async fn create_skill(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    if !can_author_skills(&state, &user).await {
        return (
            StatusCode::FORBIDDEN,
            Json(error("forbidden", "Only organization admins can author skills.")),
        )
            .into_response();
    }
    let token = shared::model_token(&state, &user, &headers).await;
    let capability = match shared::required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(err) => return shared::delegated_auth_unavailable(err).into_response(),
    };
    let url = format!("{}/v1/skills", state.model_gateway_url);
    shared::proxy_model_json_with_capability(
        &state,
        Method::POST,
        &url,
        Some(body),
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
    .into_response()
}

/// Update a durable org skill (enable/disable, edit content/description).
/// Admin-gated; proxied to model-gateway `/v1/skills/{id}` (PATCH).
async fn update_skill(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(skill_id): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    if !can_author_skills(&state, &user).await {
        return (
            StatusCode::FORBIDDEN,
            Json(error("forbidden", "Only organization admins can edit skills.")),
        )
            .into_response();
    }
    let token = shared::model_token(&state, &user, &headers).await;
    let capability = match shared::required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(err) => return shared::delegated_auth_unavailable(err).into_response(),
    };
    let url = format!(
        "{}/v1/skills/{}",
        state.model_gateway_url,
        urlencoding::encode(&skill_id)
    );
    shared::proxy_model_json_with_capability(
        &state,
        Method::PATCH,
        &url,
        Some(body),
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
    .into_response()
}

/// Delete a durable org skill. Admin-gated; proxied to model-gateway
/// `/v1/skills/{id}` (DELETE).
async fn delete_skill(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(skill_id): Path<String>,
) -> impl IntoResponse {
    if !can_author_skills(&state, &user).await {
        return (
            StatusCode::FORBIDDEN,
            Json(error("forbidden", "Only organization admins can delete skills.")),
        )
            .into_response();
    }
    let token = shared::model_token(&state, &user, &headers).await;
    let capability = match shared::required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(err) => return shared::delegated_auth_unavailable(err).into_response(),
    };
    let url = format!(
        "{}/v1/skills/{}",
        state.model_gateway_url,
        urlencoding::encode(&skill_id)
    );
    shared::proxy_model_json_with_capability(
        &state,
        Method::DELETE,
        &url,
        None,
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
    .into_response()
}

/// List the org's plugin packages (open to members so the UI can show them).
async fn list_plugins(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let token = shared::model_token(&state, &user, &headers).await;
    let capability = match shared::required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return shared::delegated_auth_unavailable(error),
    };
    let url = format!("{}/v1/plugins{}", state.model_gateway_url, build_query(&params));
    shared::proxy_model_json_with_capability(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
}

/// Register a plugin package (`name`, `version`, `description`, `manifest_json`).
/// Admin-gated; proxied to model-gateway `/v1/plugins` → capability-core. New
/// plugins default disabled until an admin enables them.
async fn create_plugin(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    if !can_author_skills(&state, &user).await {
        return (
            StatusCode::FORBIDDEN,
            Json(error("forbidden", "Only organization admins can register plugins.")),
        )
            .into_response();
    }
    let token = shared::model_token(&state, &user, &headers).await;
    let capability = match shared::required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(err) => return shared::delegated_auth_unavailable(err).into_response(),
    };
    let url = format!("{}/v1/plugins", state.model_gateway_url);
    shared::proxy_model_json_with_capability(
        &state,
        Method::POST,
        &url,
        Some(body),
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
    .into_response()
}

/// Update a plugin (enable/disable, pin, rollout state). Admin-gated.
async fn update_plugin(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(plugin_id): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    if !can_author_skills(&state, &user).await {
        return (
            StatusCode::FORBIDDEN,
            Json(error("forbidden", "Only organization admins can change plugins.")),
        )
            .into_response();
    }
    let token = shared::model_token(&state, &user, &headers).await;
    let capability = match shared::required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(err) => return shared::delegated_auth_unavailable(err).into_response(),
    };
    let url = format!(
        "{}/v1/plugins/{}",
        state.model_gateway_url,
        urlencoding::encode(&plugin_id)
    );
    shared::proxy_model_json_with_capability(
        &state,
        Method::PATCH,
        &url,
        Some(body),
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
    .into_response()
}

/// Delete (soft) a plugin package. Admin-gated.
async fn delete_plugin(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(plugin_id): Path<String>,
) -> impl IntoResponse {
    if !can_author_skills(&state, &user).await {
        return (
            StatusCode::FORBIDDEN,
            Json(error("forbidden", "Only organization admins can delete plugins.")),
        )
            .into_response();
    }
    let token = shared::model_token(&state, &user, &headers).await;
    let capability = match shared::required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(err) => return shared::delegated_auth_unavailable(err).into_response(),
    };
    let url = format!(
        "{}/v1/plugins/{}",
        state.model_gateway_url,
        urlencoding::encode(&plugin_id)
    );
    shared::proxy_model_json_with_capability(
        &state,
        Method::DELETE,
        &url,
        None,
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
    .into_response()
}

/// List the org's cron schedules (open to members).
async fn list_cron(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let token = shared::model_token(&state, &user, &headers).await;
    let capability = match shared::required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return shared::delegated_auth_unavailable(error),
    };
    let url = format!("{}/v1/cron{}", state.model_gateway_url, build_query(&params));
    shared::proxy_model_json_with_capability(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
}

/// Create a cron schedule (`name`, `schedule_expr`, `timezone`, `task_template`,
/// `enabled`). Admin-gated; proxied to model-gateway `/v1/cron` → capability-core.
/// capability-core validates the cron expression and seeds next_fire_at.
async fn create_cron(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    if !can_author_skills(&state, &user).await {
        return (
            StatusCode::FORBIDDEN,
            Json(error("forbidden", "Only organization admins can create cron schedules.")),
        )
            .into_response();
    }
    let token = shared::model_token(&state, &user, &headers).await;
    let capability = match shared::required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(err) => return shared::delegated_auth_unavailable(err).into_response(),
    };
    let url = format!("{}/v1/cron", state.model_gateway_url);
    shared::proxy_model_json_with_capability(
        &state,
        Method::POST,
        &url,
        Some(body),
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
    .into_response()
}

/// Update a cron schedule (enable/disable, edit expression). Admin-gated.
async fn update_cron(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(cron_id): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    if !can_author_skills(&state, &user).await {
        return (
            StatusCode::FORBIDDEN,
            Json(error("forbidden", "Only organization admins can change cron schedules.")),
        )
            .into_response();
    }
    let token = shared::model_token(&state, &user, &headers).await;
    let capability = match shared::required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(err) => return shared::delegated_auth_unavailable(err).into_response(),
    };
    let url = format!(
        "{}/v1/cron/{}",
        state.model_gateway_url,
        urlencoding::encode(&cron_id)
    );
    shared::proxy_model_json_with_capability(
        &state,
        Method::PATCH,
        &url,
        Some(body),
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
    .into_response()
}

/// Delete a cron schedule. Admin-gated.
async fn delete_cron(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(cron_id): Path<String>,
) -> impl IntoResponse {
    if !can_author_skills(&state, &user).await {
        return (
            StatusCode::FORBIDDEN,
            Json(error("forbidden", "Only organization admins can delete cron schedules.")),
        )
            .into_response();
    }
    let token = shared::model_token(&state, &user, &headers).await;
    let capability = match shared::required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(err) => return shared::delegated_auth_unavailable(err).into_response(),
    };
    let url = format!(
        "{}/v1/cron/{}",
        state.model_gateway_url,
        urlencoding::encode(&cron_id)
    );
    shared::proxy_model_json_with_capability(
        &state,
        Method::DELETE,
        &url,
        None,
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
    .into_response()
}

async fn list_capabilities(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let token = shared::model_token(&state, &user, &headers).await;
    let capability = match shared::required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return shared::delegated_auth_unavailable(error),
    };
    let url = format!(
        "{}/v1/capabilities{}",
        state.model_gateway_url,
        build_query(&params)
    );
    shared::proxy_model_json_with_capability(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await
}
