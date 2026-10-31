//! `/api/v1/mcp/servers` — the MCP-servers management surface.
//!
//! Thin authenticated proxy from the SPA to model-gateway's MCP-server registry
//! (`GET/POST /v1/mcp/servers`, `DELETE /v1/mcp/servers/{server_id}`, and
//! `POST /v1/mcp/servers/{server_id}/share`). Listing enumerates the servers the
//! caller may see (own + shared-to-them + org-wide), the backend omits each
//! server's secret `token` from responses, POST registers a new server, DELETE
//! removes one, and `/share` replaces a user-owned server's share set.
//!
//! The same router also carries the per-tool surface of those servers
//! (`GET /v1/mcp/servers/{server_id}/tools`, `POST .../tools/{tool}/approve`) and
//! the per-tool allow/ask/never policies (`/api/v1/tool-policies`). Both are thin
//! proxies: capability-core decides who may approve a tool (the server's owner, or
//! an admin for a shared server) and who may set a policy, from the signed
//! principal. The only gate here is defense in depth: an org-scope policy write
//! by a non-admin is refused before it is proxied, as an org-wide server is.
//!
//! Ownership/sharing: servers are private (`scope == "user"`) by default;
//! org-wide servers (`scope == "org"`) require an org admin — gated here before
//! proxying — and model-gateway re-derives admin/owner authoritatively from the
//! verified token. Forwarded role headers are presentation hints only and grant
//! no Model Plane authority.
//!
//! Org scope is resolved server-side from the validated session — the browser
//! never picks which org's servers it reads or writes. model-gateway re-derives
//! org from the model-plane token's verified claims, so the JSON body must NOT
//! carry an org id; the only client inputs are the registration fields and the
//! path `server_id`.

use axum::{
    extract::{Path, Query, RawQuery, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
    routing::{delete, get, post, put},
    Extension, Json, Router,
};
use reqwest::{header::LOCATION, Method};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    config::AppState,
    domains::chat::shared::{
        data_plane_authorization_value, delegated_auth_unavailable, model_token,
        proxy_model_json_with_capability, required_capability_token,
    },
    envelope::error,
    middleware::{has_authorized_org_role, require_session, AuthenticatedUser},
    rate_limit::rate_limit_middleware,
    upstream::same_upstream_origin,
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/mcp/servers",
            get(list_servers).post(register_server),
        )
        .route("/api/v1/mcp/servers/{server_id}", delete(delete_server))
        .route("/api/v1/mcp/servers/{server_id}/share", post(share_server))
        .route("/api/v1/mcp/servers/oauth/start", post(oauth_start))
        .route("/api/v1/mcp/servers/oauth/callback", get(oauth_callback))
        .route("/api/v1/mcp/servers/connect", post(connect_server))
        .route("/api/v1/mcp/servers/{server_id}/tools", get(list_server_tools))
        .route(
            "/api/v1/mcp/servers/{server_id}/tools/{tool}/approve",
            post(approve_server_tool),
        )
        .route("/api/v1/tool-policies", get(list_tool_policies))
        .route(
            "/api/v1/tool-policies/{capability_id}",
            put(set_tool_policy).delete(revoke_tool_policy),
        )
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
/// model-gateway (which independently derives authorization from verified token
/// scopes).
async fn is_org_admin(state: &AppState, user: &AuthenticatedUser) -> bool {
    let _ = state;
    has_authorized_org_role(user, &["owner", "admin"])
}

/// `true` if the request wants an org-wide server (`scope == "org"`) but the
/// caller isn't an org admin — the shared gate for both plain and
/// OAuth-based server registration.
async fn org_scope_forbidden(state: &AppState, user: &AuthenticatedUser, body: &Value) -> bool {
    let wants_org_scope = body
        .get("scope")
        .and_then(Value::as_str)
        .map(|scope| scope.eq_ignore_ascii_case("org"))
        .unwrap_or(false);
    wants_org_scope && !is_org_admin(state, user).await
}

async fn list_servers(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let token = model_token(&state, &user, &headers).await;
    let capability = match required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return delegated_auth_unavailable(error).into_response(),
    };
    let url = format!("{}/v1/mcp/servers", state.model_gateway_url);
    let (status, body) = proxy_model_json_with_capability(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await;
    (status, body).into_response()
}

/// Register a new MCP server. The JSON body (`name`, `url`, `transport`,
/// `tool_allowlist`, `enabled`, `scope`) is forwarded for strict
/// validation — org id is NEVER injected, since model-gateway derives it from
/// the verified model-plane token's claims. Raw tokens and caller-selected
/// server IDs are rejected by model-gateway; secret-reference onboarding is not
/// implemented yet.
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
    if org_scope_forbidden(&state, &user, &body).await {
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
    let capability = match required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return delegated_auth_unavailable(error).into_response(),
    };
    let url = format!("{}/v1/mcp/servers", state.model_gateway_url);
    let (status, body) = proxy_model_json_with_capability(
        &state,
        Method::POST,
        &url,
        Some(body),
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await;
    (status, body).into_response()
}

/// Replace a user-owned server's share set. Body `{ "user_ids": [...] }` is
/// forwarded verbatim to model-gateway, which enforces OWNER-ONLY authorization
/// (403 otherwise) — the gateway re-derives owner/org from the verified token,
/// so no ownership identifiers are injected here.
pub(crate) async fn share_server(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(server_id): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let token = model_token(&state, &user, &headers).await;
    let capability = match required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return delegated_auth_unavailable(error).into_response(),
    };
    let url = format!(
        "{}/v1/mcp/servers/{}/share",
        state.model_gateway_url,
        urlencoding::encode(&server_id)
    );
    let (status, body) = proxy_model_json_with_capability(
        &state,
        Method::POST,
        &url,
        Some(body),
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await;
    (status, body).into_response()
}

pub(crate) async fn delete_server(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(server_id): Path<String>,
) -> impl IntoResponse {
    let token = model_token(&state, &user, &headers).await;
    let capability = match required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return delegated_auth_unavailable(error).into_response(),
    };
    let url = format!(
        "{}/v1/mcp/servers/{}",
        state.model_gateway_url,
        urlencoding::encode(&server_id)
    );
    let (status, body) = proxy_model_json_with_capability(
        &state,
        Method::DELETE,
        &url,
        None,
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await;
    (status, body).into_response()
}

/// Begin connecting an MCP server that requires real OAuth 2.1 login (e.g.
/// Visma Net) instead of a static token. Body (`name`, `url`,
/// `tool_allowlist`, `scope`) is forwarded verbatim to model-gateway, which
/// discovers the server's protected-resource + authorization-server
/// metadata and dynamically registers a client (RFC 7591 — no pre-existing
/// app needed). Response carries the `authorization_url` the browser
/// should navigate to next. Same org-admin gate as `register_server` for
/// `scope == "org"`.
async fn oauth_start(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    if org_scope_forbidden(&state, &user, &body).await {
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
    let capability = match required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return delegated_auth_unavailable(error).into_response(),
    };
    let url = format!("{}/v1/mcp/servers/oauth/start", state.model_gateway_url);
    let (status, body) = proxy_model_json_with_capability(
        &state,
        Method::POST,
        &url,
        Some(body),
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await;
    (status, body).into_response()
}

/// The single auto-detecting entry point: body is `{name, url, token?,
/// tool_allowlist?, scope?}` — no transport or auth-kind choice. model-gateway
/// discovers whether `url` is an OAuth 2.1 protected resource and either
/// returns `{needs_oauth: true, authorization_url}` (the caller then
/// navigates the browser there, same as `oauth_start`) or registers the
/// server directly and returns it, same shape as `register_server`. Same
/// org-admin gate for `scope == "org"`.
async fn connect_server(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    if org_scope_forbidden(&state, &user, &body).await {
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
    let capability = match required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return delegated_auth_unavailable(error).into_response(),
    };
    let url = format!("{}/v1/mcp/servers/connect", state.model_gateway_url);
    let (status, body) = proxy_model_json_with_capability(
        &state,
        Method::POST,
        &url,
        Some(body),
        token.as_deref(),
        Some(&capability),
        &user,
    )
    .await;
    (status, body).into_response()
}

/// The registry rows of one server's tools: class, approval floor, review state
/// and digests, never a description or a schema.
async fn list_server_tools(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(server_id): Path<String>,
) -> Response {
    let url = format!(
        "{}/v1/mcp/servers/{}/tools",
        state.model_gateway_url,
        urlencoding::encode(&server_id)
    );
    proxy_tool_surface(&state, &user, &headers, Method::GET, &url, None).await
}

/// A person accepts the definition of a tool they reviewed. Body
/// `{ "approved_digest": "<observed digest shown to them>" }` is forwarded
/// verbatim. No role gate here: the owner of a user-scoped server may approve its
/// tools and an admin those of a shared one, which capability-core derives from
/// the signed principal and the server's own record, so an admin-only gate in
/// this layer would lock owners out.
async fn approve_server_tool(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((server_id, tool)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Response {
    let url = format!(
        "{}/v1/mcp/servers/{}/tools/{}/approve",
        state.model_gateway_url,
        urlencoding::encode(&server_id),
        urlencoding::encode(&tool)
    );
    proxy_tool_surface(&state, &user, &headers, Method::POST, &url, Some(body)).await
}

#[derive(Debug, Default, Deserialize)]
struct ToolPolicyQuery {
    capability_id: Option<String>,
    scope: Option<String>,
}

/// The org's tool policies and the caller's own, optionally for one capability.
async fn list_tool_policies(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Query(query): Query<ToolPolicyQuery>,
) -> Response {
    let mut url = format!("{}/v1/tool-policies", state.model_gateway_url);
    if let Some(capability_id) = query.capability_id.as_deref().filter(|id| !id.is_empty()) {
        url.push_str("?capability_id=");
        url.push_str(&urlencoding::encode(capability_id));
    }
    proxy_tool_surface(&state, &user, &headers, Method::GET, &url, None).await
}

/// Set the caller's own allow/ask/never policy for a capability's tools, or, when
/// the body asks for `"scope": "org"`, the org's. An org policy needs an org
/// admin: refused here before it is proxied, and re-derived from the signed token
/// scopes by capability-core, which also checks the tool's class and the reviewed
/// digest of an `allow`. The org id is never taken from the body.
async fn set_tool_policy(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(capability_id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let scope = body.get("scope").and_then(Value::as_str);
    if let Some(refusal) = org_policy_forbidden(&state, &user, scope).await {
        return refusal.into_response();
    }
    let url = format!(
        "{}/v1/tool-policies/{}",
        state.model_gateway_url,
        urlencoding::encode(&capability_id)
    );
    proxy_tool_surface(&state, &user, &headers, Method::PUT, &url, Some(body)).await
}

/// Revoke the caller's own policy for a capability, or, with `?scope=org`, the
/// org's (admin only, as for `set_tool_policy`).
async fn revoke_tool_policy(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(capability_id): Path<String>,
    Query(query): Query<ToolPolicyQuery>,
) -> Response {
    if let Some(refusal) = org_policy_forbidden(&state, &user, query.scope.as_deref()).await {
        return refusal.into_response();
    }
    let mut url = format!(
        "{}/v1/tool-policies/{}",
        state.model_gateway_url,
        urlencoding::encode(&capability_id)
    );
    if let Some(scope) = query.scope.as_deref().filter(|scope| !scope.is_empty()) {
        url.push_str("?scope=");
        url.push_str(&urlencoding::encode(scope));
    }
    proxy_tool_surface(&state, &user, &headers, Method::DELETE, &url, None).await
}

/// The refusal for an org-scope tool-policy request from someone who is not an
/// org admin, or `None` when the request may be proxied.
async fn org_policy_forbidden(
    state: &AppState,
    user: &AuthenticatedUser,
    scope: Option<&str>,
) -> Option<(StatusCode, Json<Value>)> {
    let wants_org = scope.is_some_and(|scope| scope.eq_ignore_ascii_case("org"));
    if wants_org && !is_org_admin(state, user).await {
        return Some((
            StatusCode::FORBIDDEN,
            Json(error(
                "forbidden",
                "Only organization admins can set org-wide tool policies.",
            )),
        ));
    }
    None
}

/// Forward one request of the per-tool surface with the caller's model-gateway
/// and capability-core credentials, exactly as the server routes above do.
async fn proxy_tool_surface(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    method: Method,
    url: &str,
    body: Option<Value>,
) -> Response {
    let token = model_token(state, user, headers).await;
    let capability = match required_capability_token(state, user, headers).await {
        Ok(token) => token,
        Err(error) => return delegated_auth_unavailable(error).into_response(),
    };
    let (status, body) = proxy_model_json_with_capability(
        state,
        method,
        url,
        body,
        token.as_deref(),
        Some(&capability),
        user,
    )
    .await;
    (status, body).into_response()
}

/// Where the external authorization server redirects the browser back to
/// after consent. A plain top-level navigation — no fetch, no bearer
/// header of its own — so the caller's identity comes from the same
/// session cookie every other route here relies on (`require_session`),
/// never from the query string. Relays `code`/`state`/`error` to
/// model-gateway verbatim and forwards only its `Location` redirect back to
/// the browser: model-gateway never returns a body worth relaying here,
/// always a redirect to the SPA's settings page, success or failure alike.
/// Any local failure (missing capability token, network error, no
/// `Location` in the response) redirects to the same settings page with
/// `mcp_oauth=error` rather than surfacing raw JSON mid-navigation.
async fn oauth_callback(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Response {
    let fallback = format!(
        "{}/settings/mcp?mcp_oauth=error",
        state.verevon_public_origin.trim_end_matches('/')
    );
    let org_id = user.active_org_id.clone().unwrap_or_default();

    let Some(token) = model_token(&state, &user, &headers).await else {
        tracing::warn!(%org_id, "mcp oauth callback: could not mint a model-gateway token");
        return Redirect::to(&fallback).into_response();
    };
    let capability = match required_capability_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => {
            tracing::warn!(
                %org_id,
                audience = error.audience.claim(),
                "mcp oauth callback: could not mint a capability-core token"
            );
            return Redirect::to(&fallback).into_response();
        }
    };

    let mut url = format!("{}/v1/mcp/servers/oauth/callback", state.model_gateway_url);
    if let Some(query) = query.filter(|value| !value.is_empty()) {
        url.push('?');
        url.push_str(&query);
    }
    if !same_upstream_origin(&url, &state.model_gateway_url) {
        tracing::warn!(%org_id, "mcp oauth callback: target failed the upstream-origin check");
        return Redirect::to(&fallback).into_response();
    }

    // `x-capability-authorization` must carry the same "Bearer <jwt>" shape
    // model-gateway's decode_delegated_bearer expects (mirroring
    // proxy_model_json_with_delegations) — a bare token here fails
    // strip_prefix("Bearer ") and model-gateway 401s the whole request with
    // nothing to log on its side, since a malformed delegated header looks
    // identical to one that was never sent.
    let Some(capability_header) = data_plane_authorization_value(&capability) else {
        tracing::warn!(%org_id, "mcp oauth callback: capability token could not be formatted as a header value");
        return Redirect::to(&fallback).into_response();
    };
    let request = state
        .client
        .get(&url)
        .bearer_auth(&token)
        .header("x-capability-authorization", capability_header);

    let upstream = match request.send().await {
        Ok(upstream) => upstream,
        Err(error) => {
            tracing::warn!(%org_id, error_class = crate::zdr_log::class_reqwest(&error), "mcp oauth callback: model-gateway request failed");
            return Redirect::to(&fallback).into_response();
        }
    };
    let upstream_status = upstream.status();
    let Some(location) = upstream.headers().get(LOCATION).cloned() else {
        tracing::warn!(
            %org_id,
            status = upstream_status.as_u16(),
            "mcp oauth callback: model-gateway response had no Location header"
        );
        return Redirect::to(&fallback).into_response();
    };
    let status = StatusCode::from_u16(upstream_status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut response = match Response::builder()
        .status(status)
        .body(axum::body::Body::empty())
    {
        Ok(response) => response,
        Err(error) => {
            // zdr-log-ok: an http::Error from building a redirect response out of a header value the model-gateway returned; it names the header problem, not a page or query
            tracing::warn!(%org_id, error = %error, "mcp oauth callback: could not build the redirect response");
            return Redirect::to(&fallback).into_response();
        }
    };
    response.headers_mut().insert(LOCATION, location);
    response
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderMap;

    use super::*;
    use crate::middleware::AuthorizedMembership;

    fn member(role: &str) -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.test".to_owned(),
            user_name: "User".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("user".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: Some(AuthorizedMembership {
                organization_id: "org-1".to_owned(),
                role: role.to_owned(),
            }),
        }
    }

    #[tokio::test]
    async fn an_org_scope_policy_needs_an_org_admin() {
        let state = crate::tests::test_state(false);
        for scope in ["org", "ORG", "Org"] {
            let refusal = org_policy_forbidden(&state, &member("member"), Some(scope))
                .await
                .expect("a member may not set an org policy");
            assert_eq!(refusal.0, StatusCode::FORBIDDEN, "{scope}");
            assert!(
                org_policy_forbidden(&state, &member("admin"), Some(scope))
                    .await
                    .is_none(),
                "{scope}"
            );
            assert!(
                org_policy_forbidden(&state, &member("owner"), Some(scope))
                    .await
                    .is_none(),
                "{scope}"
            );
        }
    }

    #[tokio::test]
    async fn a_personal_policy_needs_no_role() {
        let state = crate::tests::test_state(false);
        for scope in [None, Some(""), Some("user")] {
            assert!(
                org_policy_forbidden(&state, &member("member"), scope)
                    .await
                    .is_none(),
                "{scope:?}"
            );
        }
    }

    // The refusal is answered before a credential is minted or anything is
    // forwarded: every upstream URL of the test state is a closed port, so a
    // forwarded request would surface as a gateway error, not a 403.
    #[tokio::test]
    async fn a_member_cannot_set_or_revoke_an_org_policy_through_the_routes() {
        let state = crate::tests::test_state(false);
        let set = set_tool_policy(
            State(state.clone()),
            Extension(member("member")),
            HeaderMap::new(),
            Path("cap.mcp_tool.x".to_owned()),
            Json(serde_json::json!({"scope": "org", "decision": "never"})),
        )
        .await;
        assert_eq!(set.status(), StatusCode::FORBIDDEN);
        let revoke = revoke_tool_policy(
            State(state),
            Extension(member("member")),
            HeaderMap::new(),
            Path("cap.mcp_tool.x".to_owned()),
            Query(ToolPolicyQuery {
                capability_id: None,
                scope: Some("org".to_owned()),
            }),
        )
        .await;
        assert_eq!(revoke.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn the_per_tool_routes_are_registered_behind_the_session_gate() {
        // Source-text pin: the routes and their session gate live in one router,
        // so a tool route can never be mounted outside `require_session`.
        let source = include_str!("mcp.rs");
        let router = source
            .split("pub(crate) fn router")
            .nth(1)
            .and_then(|rest| rest.split("\n}").next())
            .expect("the router function");
        for route in [
            "/api/v1/mcp/servers/{server_id}/tools\"",
            "/api/v1/mcp/servers/{server_id}/tools/{tool}/approve",
            "/api/v1/tool-policies\"",
            "/api/v1/tool-policies/{capability_id}",
            "from_fn_with_state(state, require_session)",
        ] {
            assert!(router.contains(route), "{route}");
        }
    }
}
