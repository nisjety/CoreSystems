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

use std::{
    collections::HashMap,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

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
    middleware::{has_authorized_org_role, require_session, AuthenticatedUser},
};

static CRON_CREATE_COUNTER: AtomicU64 = AtomicU64::new(1);

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
    let _ = state;
    has_authorized_org_role(user, &["owner", "admin"])
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
            Json(error(
                "forbidden",
                "Only organization admins can author skills.",
            )),
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
            Json(error(
                "forbidden",
                "Only organization admins can edit skills.",
            )),
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
            Json(error(
                "forbidden",
                "Only organization admins can delete skills.",
            )),
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
    let url = format!(
        "{}/v1/plugins{}",
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
            Json(error(
                "forbidden",
                "Only organization admins can register plugins.",
            )),
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
            Json(error(
                "forbidden",
                "Only organization admins can change plugins.",
            )),
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
            Json(error(
                "forbidden",
                "Only organization admins can delete plugins.",
            )),
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
    let url = format!(
        "{}/v1/cron{}",
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

/// Create a cron schedule (`name`, `schedule_expr`, `timezone`, `task_template`,
/// `enabled`). Admin-gated; proxied to model-gateway `/v1/cron` → capability-core.
/// capability-core validates the cron expression and seeds next_fire_at.
async fn create_cron(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(mut body): Json<Value>,
) -> impl IntoResponse {
    if !can_author_skills(&state, &user).await {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "forbidden",
                "Only organization admins can create cron schedules.",
            )),
        )
            .into_response();
    }
    let token = shared::model_token(&state, &user, &headers).await;
    let capability = match shared::required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(err) => return shared::delegated_auth_unavailable(err).into_response(),
    };
    let Some(org_id) = user
        .active_org_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    else {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "active_membership_required",
                "An active organization membership is required to create a Space schedule.",
            )),
        )
            .into_response();
    };
    let counter = CRON_CREATE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let schedule_id = format!("cron_{}_{}_{}", user.user_id, nanos, counter);
    let idempotency_key = format!("space-cron-create:{schedule_id}");
    if let Err(response) = crate::domains::spaces::inject_personal_schedule_create_context(
        &state,
        &user,
        org_id,
        &mut body,
        &schedule_id,
        &idempotency_key,
    )
    .await
    {
        return response.into_response();
    }
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
            Json(error(
                "forbidden",
                "Only organization admins can change cron schedules.",
            )),
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
            Json(error(
                "forbidden",
                "Only organization admins can delete cron schedules.",
            )),
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
