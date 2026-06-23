//! `/api/v1/mcp/servers` — the MCP-servers management surface.
//!
//! Thin authenticated proxy from the SPA to model-gateway's MCP-server registry
//! (`GET/POST /v1/mcp/servers`, `DELETE /v1/mcp/servers/{server_id}`, and
//! `POST /v1/mcp/servers/{server_id}/share`). Listing enumerates the servers the
//! caller may see (own + shared-to-them + org-wide), the backend omits each
//! server's secret `token` from responses, POST registers a new server, DELETE
//! removes one, and `/share` replaces a user-owned server's share set.
//!
//! Ownership/sharing: servers are private (`scope == "user"`) by default;
//! org-wide servers (`scope == "org"`) require an org admin — gated here before
//! proxying — and model-gateway re-derives admin/owner authoritatively from the
//! verified token (it also honors the forwarded `x-user-role`).
//!
//! Org scope is resolved server-side from the validated session — the browser
//! never picks which org's servers it reads or writes. model-gateway re-derives
//! org from the model-plane token's verified claims, so the JSON body must NOT
//! carry an org id; the only client inputs are the registration fields and the
//! path `server_id`.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{delete, get, post},
    Extension, Json, Router,
};
use reqwest::Method;
use serde_json::Value;

use crate::{
    config::AppState,
    domains::chat::shared::{model_token, proxy_model_json},
    envelope::error,
    middleware::{require_session, AuthenticatedUser},
    rate_limit::rate_limit_middleware,
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/mcp/servers",
            get(list_servers).post(register_server),
        )
        .route("/api/v1/mcp/servers/:server_id", delete(delete_server))
        .route("/api/v1/mcp/servers/:server_id/share", post(share_server))
        // Per-org/user rate limiting, ordered like `agents_runs.rs`:
        // `require_session` (written last → outer) runs first and inserts
        // `AuthenticatedUser`, so `rate_limit_middleware` (written first → inner)
        // keys by the validated org/user rather than client IP.
        .route_layer(axum::middleware::from_fn(rate_limit_middleware))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

/// Whether the validated session user is an org administrator.
///
/// Mirrors `domains::orgs::shared::require_org_admin`: a top-level `admin` /
/// `superadmin` on the Better Auth role short-circuits, otherwise the active
/// org's session-context role must be `owner` / `admin`. Used as the
/// authoritative gate before forwarding an org-scoped registration to
/// model-gateway (which independently re-derives admin from token scopes or
/// the forwarded `x-user-role`).
async fn is_org_admin(state: &AppState, user: &AuthenticatedUser) -> bool {
    if has_any_role(user.auth_role.as_deref(), &["admin", "superadmin"]) {
        return true;
    }

    let org_role = crate::upstream::resolve_session_context(state, user)
        .await
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    has_any_role(Some(org_role.as_str()), &["owner", "admin"])
}

fn has_any_role(value: Option<&str>, allowed: &[&str]) -> bool {
    value
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .any(|role| {
            allowed
                .iter()
                .any(|allowed_role| role.eq_ignore_ascii_case(allowed_role))
        })
}

async fn list_servers(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let token = model_token(&state, &user, &headers).await;
    let url = format!("{}/v1/mcp/servers", state.model_gateway_url);
    let (status, body) =
        proxy_model_json(&state, Method::GET, &url, None, token.as_deref(), &user).await;
    (status, body).into_response()
}

/// Register a new MCP server. The JSON body (`name`, `url`, `transport`,
/// `token`, `tool_allowlist`, `enabled`, `server_id`, `scope`) is forwarded
/// verbatim — org id is NEVER injected, since model-gateway derives it from the
/// verified model-plane token's claims.
///
/// `scope` defaults to `user` (private). Creating an org-wide server
/// (`scope == "org"`) requires an org admin: this gate rejects non-admins with
/// 403 before proxying. model-gateway re-checks admin authoritatively, so this
/// is a defense-in-depth gate that also lets the SPA surface a clear error.
async fn register_server(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let wants_org_scope = body
        .get("scope")
        .and_then(Value::as_str)
        .map(|scope| scope.eq_ignore_ascii_case("org"))
        .unwrap_or(false);
    if wants_org_scope && !is_org_admin(&state, &user).await {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "forbidden",
                "Only organization admins can create org-wide MCP servers.",
            )),
        )
            .into_response();
    }

    let token = model_token(&state, &user, &headers).await;
    let url = format!("{}/v1/mcp/servers", state.model_gateway_url);
    let (status, body) = proxy_model_json(
        &state,
        Method::POST,
        &url,
        Some(body),
        token.as_deref(),
        &user,
    )
    .await;
    (status, body).into_response()
}

/// Replace a user-owned server's share set. Body `{ "user_ids": [...] }` is
/// forwarded verbatim to model-gateway, which enforces OWNER-ONLY authorization
/// (403 otherwise) — the gateway re-derives owner/org from the verified token,
/// so no ownership identifiers are injected here.
async fn share_server(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(server_id): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let token = model_token(&state, &user, &headers).await;
    let url = format!(
        "{}/v1/mcp/servers/{}/share",
        state.model_gateway_url,
        urlencoding::encode(&server_id)
    );
    let (status, body) = proxy_model_json(
        &state,
        Method::POST,
        &url,
        Some(body),
        token.as_deref(),
        &user,
    )
    .await;
    (status, body).into_response()
}

async fn delete_server(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(server_id): Path<String>,
) -> impl IntoResponse {
    let token = model_token(&state, &user, &headers).await;
    let url = format!(
        "{}/v1/mcp/servers/{}",
        state.model_gateway_url,
        urlencoding::encode(&server_id)
    );
    let (status, body) =
        proxy_model_json(&state, Method::DELETE, &url, None, token.as_deref(), &user).await;
    (status, body).into_response()
}
