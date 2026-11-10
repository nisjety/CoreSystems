//! `/api/v1/org/agent-policy`, `/api/v1/org/hooks` and `/api/v1/org/permission-rules`: the organization's agent
//! policy, its durable hook rules (WS25-S10) and its permission rules (W5-074-c).
//!
//! A thin authenticated proxy from the SPA to model-gateway's `/v1/org/*` routes, which forward to
//! capability-core, the system of record for both (DEC-14). Any member of the active organization reads the
//! policy in force, its effective view and the hook rules. A write needs an org admin, and an organization
//! under Zero Data Retention is refused typed before anything is forwarded. capability-core re-derives the
//! admin claim, the write scope and the ZDR posture from the signed principal, so the gate here is defense in
//! depth that also lets the SPA say why.
//!
//! The organization comes from the validated session only, never from the browser: no body or header of the
//! request can name one. A policy write carries the revision it was made against in its body as `revision`,
//! because this proxy forwards the session's own credentials and nothing the client sent; model-gateway turns
//! it into the `If-Match` precondition capability-core requires.

use axum::{
    extract::{Extension, Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, put},
    Json, Router,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    domains::chat::shared::{
        delegated_auth_unavailable, model_token, proxy_model_json_with_capability,
        required_capability_token,
    },
    envelope::error,
    middleware::{has_authorized_org_role, require_session, AuthenticatedUser},
    rate_limit::rate_limit_middleware,
    zdr::org_zdr_enabled,
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/org/agent-policy",
            get(get_agent_policy).put(set_agent_policy),
        )
        .route(
            "/api/v1/org/agent-policy/effective",
            get(get_effective_agent_policy),
        )
        .route("/api/v1/org/hooks", get(list_hooks).post(create_hook))
        .route(
            "/api/v1/org/hooks/{hook_id}",
            put(update_hook).delete(delete_hook),
        )
        .route(
            "/api/v1/org/permission-rules",
            get(list_permission_rules).post(create_permission_rule),
        )
        .route(
            "/api/v1/org/permission-rules/{rule_id}",
            delete(delete_permission_rule),
        )
        // Per-org/user rate limiting, ordered as in `mcp.rs`: `require_session` runs first and inserts
        // `AuthenticatedUser`, so the limiter keys by the validated organization and user.
        .route_layer(axum::middleware::from_fn(rate_limit_middleware))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

/// The refusal for a write from someone who is not an org admin, or from an organization under Zero Data
/// Retention, or `None` when the write may be forwarded. Both are decided here, before anything is sent.
async fn write_refusal(state: &AppState, user: &AuthenticatedUser) -> Option<Response> {
    if !has_authorized_org_role(user, &["owner", "admin"]) {
        return Some(
            (
                StatusCode::FORBIDDEN,
                Json(error(
                    "forbidden",
                    "Only organization admins can change the agent policy, its hook rules or its permission rules.",
                )),
            )
                .into_response(),
        );
    }
    if org_zdr_enabled(state, user).await {
        return Some((StatusCode::PRECONDITION_FAILED, Json(zdr_refusal())).into_response());
    }
    None
}

/// The typed Zero Data Retention refusal for a durable write, shaped as model-gateway's refusal catalogue
/// shapes it. Nothing was stored.
fn zdr_refusal() -> Value {
    json!({
        "error": {
            "code": "zdr_durable_mutation_forbidden",
            "message": "Zero Data Retention is on for this organization, so its agent policy, hook rules and permission rules cannot be changed.",
            "retryable": false,
            "action": "none",
        }
    })
}

/// Forwards one request to model-gateway with the caller's model-plane and capability-core credentials, as
/// the other policy proxies do. The caller's session cookie is read only to mint those two credentials.
async fn forward(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    method: Method,
    route: &str,
    body: Option<Value>,
) -> Response {
    let token = model_token(state, user, headers).await;
    let capability = match required_capability_token(state, user, headers).await {
        Ok(token) => token,
        Err(error) => return delegated_auth_unavailable(error).into_response(),
    };
    let url = format!("{}{route}", state.model_gateway_url);
    let (status, body) = proxy_model_json_with_capability(
        state,
        method,
        &url,
        body,
        token.as_deref(),
        Some(&capability),
        user,
    )
    .await;
    (status, body).into_response()
}

/// `GET /api/v1/org/agent-policy` — the organization's agent policy in force, for any member.
async fn get_agent_policy(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> Response {
    forward(
        &state,
        &user,
        &headers,
        Method::GET,
        "/v1/org/agent-policy",
        None,
    )
    .await
}

/// `PUT /api/v1/org/agent-policy` — an org admin replaces the policy at the `revision` the body names.
async fn set_agent_policy(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if let Some(refusal) = write_refusal(&state, &user).await {
        return refusal;
    }
    forward(
        &state,
        &user,
        &headers,
        Method::PUT,
        "/v1/org/agent-policy",
        Some(body),
    )
    .await
}

/// `GET /api/v1/org/agent-policy/effective` — each setting with the value in force, for any member.
async fn get_effective_agent_policy(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> Response {
    forward(
        &state,
        &user,
        &headers,
        Method::GET,
        "/v1/org/agent-policy/effective",
        None,
    )
    .await
}

/// `GET /api/v1/org/hooks` — the organization's active hook rules, for any member.
async fn list_hooks(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> Response {
    forward(&state, &user, &headers, Method::GET, "/v1/org/hooks", None).await
}

/// `POST /api/v1/org/hooks` — an org admin adds a rule.
async fn create_hook(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if let Some(refusal) = write_refusal(&state, &user).await {
        return refusal;
    }
    forward(
        &state,
        &user,
        &headers,
        Method::POST,
        "/v1/org/hooks",
        Some(body),
    )
    .await
}

/// `PUT /api/v1/org/hooks/{hook_id}` — an org admin replaces the content of an active rule.
async fn update_hook(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(hook_id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    if let Some(refusal) = write_refusal(&state, &user).await {
        return refusal;
    }
    let route = format!("/v1/org/hooks/{}", urlencoding::encode(&hook_id));
    forward(&state, &user, &headers, Method::PUT, &route, Some(body)).await
}

/// `DELETE /api/v1/org/hooks/{hook_id}` — an org admin retires an active rule.
async fn delete_hook(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(hook_id): Path<String>,
) -> Response {
    if let Some(refusal) = write_refusal(&state, &user).await {
        return refusal;
    }
    let route = format!("/v1/org/hooks/{}", urlencoding::encode(&hook_id));
    forward(&state, &user, &headers, Method::DELETE, &route, None).await
}

/// `GET /api/v1/org/permission-rules` — the organization's permission rules and the policy revision they are at, for any
/// member.
async fn list_permission_rules(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> Response {
    let response = forward(
        &state,
        &user,
        &headers,
        Method::GET,
        "/v1/org/permission-rules",
        None,
    )
    .await;
    no_store(response)
}

/// `POST /api/v1/org/permission-rules` — an org admin adds a deny or ask rule.
async fn create_permission_rule(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if let Some(refusal) = write_refusal(&state, &user).await {
        return no_store(refusal);
    }
    let response = forward(
        &state,
        &user,
        &headers,
        Method::POST,
        "/v1/org/permission-rules",
        Some(body),
    )
    .await;
    no_store(response)
}

/// `DELETE /api/v1/org/permission-rules/{rule_id}` — an org admin removes a rule.
async fn delete_permission_rule(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(rule_id): Path<String>,
) -> Response {
    if let Some(refusal) = write_refusal(&state, &user).await {
        return no_store(refusal);
    }
    let route = format!("/v1/org/permission-rules/{}", urlencoding::encode(&rule_id));
    let response = forward(&state, &user, &headers, Method::DELETE, &route, None).await;
    no_store(response)
}

/// Marks a response `Cache-Control: no-store`: a rule set is the organization's policy, so no browser or shared cache
/// keeps it.
fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::AuthorizedMembership;
    use axum::http::HeaderValue;
    use http_body_util::BodyExt;
    use wiremock::{
        matchers::{method as wm_method, path as wm_path, path_regex},
        Mock, MockServer, ResponseTemplate,
    };

    /// A user of organization-1 whose Control-verified membership carries `role`. The session's own role is
    /// `member` whatever the membership says, so a test can tell which of the two reached the upstream.
    fn user(role: &str) -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.invalid".to_owned(),
            user_name: "User".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: Some(AuthorizedMembership {
                organization_id: "org-1".to_owned(),
                role: role.to_owned(),
            }),
        }
    }

    /// One mock stands in for auth-core (the plane tokens), org-core (the organization's posture) and
    /// model-gateway.
    async fn gateway(zdr: bool) -> MockServer {
        let server = MockServer::start().await;
        for audience in ["model-plane", "capability-core"] {
            Mock::given(wm_method("GET"))
                .and(wm_path(format!("/api/{audience}/token")))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(json!({"token": format!("{audience}-token")})),
                )
                .mount(&server)
                .await;
        }
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/organizations/org-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "org-1",
                "metadata": { "interactiveRetention": { "zdr": zdr } }
            })))
            .mount(&server)
            .await;
        Mock::given(path_regex("^/v1/org/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
            .mount(&server)
            .await;
        server
    }

    fn state_for(server: &MockServer) -> AppState {
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = server.uri();
        state.org_core_url = server.uri();
        state.model_gateway_url = server.uri();
        state
    }

    fn rule() -> Value {
        json!({
            "event": "pre_tool",
            "toolMatcher": "shell",
            "decision": "deny",
            "reason": "no shell"
        })
    }

    async fn answer(response: Response) -> (StatusCode, Value) {
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("response body")
            .to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// The `METHOD path` of each request model-gateway received, in order.
    async fn gateway_calls(server: &MockServer) -> Vec<String> {
        server
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .filter(|request| request.url.path().starts_with("/v1/org/"))
            .map(|request| format!("{} {}", request.method, request.url.path()))
            .collect()
    }

    #[tokio::test]
    async fn a_member_reads_the_policy_and_no_write_reaches_the_gateway() {
        let server = gateway(false).await;
        let state = state_for(&server);

        let (status, _) = answer(
            get_agent_policy(
                State(state.clone()),
                Extension(user("member")),
                HeaderMap::new(),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, body) = answer(
            set_agent_policy(
                State(state),
                Extension(user("member")),
                HeaderMap::new(),
                Json(json!({"settings": {}, "revision": 0})),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "forbidden");
        assert_eq!(gateway_calls(&server).await, ["GET /v1/org/agent-policy"]);
    }

    #[tokio::test]
    async fn an_admin_write_is_forwarded_for_the_session_org_and_no_forged_header_is_passed_on() {
        let server = gateway(false).await;
        let state = state_for(&server);
        let mut headers = HeaderMap::new();
        headers.insert("x-org-id", HeaderValue::from_static("org-evil"));
        headers.insert("x-user-role", HeaderValue::from_static("owner"));
        let body = json!({"settings": {"mode_cap": "read_only"}, "revision": 2});

        let (status, _) = answer(
            set_agent_policy(
                State(state),
                Extension(user("admin")),
                headers,
                Json(body.clone()),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let received = server.received_requests().await.unwrap_or_default();
        let write = received
            .iter()
            .find(|request| request.url.path() == "/v1/org/agent-policy")
            .expect("the admin's write was forwarded");
        assert_eq!(
            write.headers.get("x-org-id"),
            Some(&HeaderValue::from_static("org-1"))
        );
        assert_eq!(
            write.headers.get("x-user-role"),
            Some(&HeaderValue::from_static("member"))
        );
        // The revision travels in the body; model-gateway turns it into If-Match.
        assert_eq!(
            serde_json::from_slice::<Value>(&write.body).expect("a JSON body"),
            body
        );
    }

    #[tokio::test]
    async fn a_zdr_organization_refuses_every_write_before_anything_is_forwarded() {
        let server = gateway(true).await;
        let state = state_for(&server);
        let admin = user("admin");

        let (status, body) = answer(
            set_agent_policy(
                State(state.clone()),
                Extension(admin.clone()),
                HeaderMap::new(),
                Json(json!({"settings": {}, "revision": 0})),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::PRECONDITION_FAILED);
        assert_eq!(body["error"]["code"], "zdr_durable_mutation_forbidden");
        assert_eq!(body["error"]["retryable"], false);
        assert_eq!(body["error"]["action"], "none");

        let (status, _) = answer(
            create_hook(
                State(state.clone()),
                Extension(admin.clone()),
                HeaderMap::new(),
                Json(rule()),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::PRECONDITION_FAILED);
        let (status, _) = answer(
            update_hook(
                State(state.clone()),
                Extension(admin.clone()),
                HeaderMap::new(),
                Path("ohr_1".to_owned()),
                Json(rule()),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::PRECONDITION_FAILED);
        let (status, _) = answer(
            delete_hook(
                State(state.clone()),
                Extension(admin),
                HeaderMap::new(),
                Path("ohr_1".to_owned()),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::PRECONDITION_FAILED);

        // The only call made was the organization's posture lookup: no credential was minted and no write
        // was sent.
        let received = server.received_requests().await.unwrap_or_default();
        assert!(
            received
                .iter()
                .all(|request| request.url.path() == "/api/v1/organizations/org-1"),
            "a refused write reached an upstream"
        );

        // Reading stays open to the members of a ZDR organization.
        let (status, _) = answer(
            get_agent_policy(State(state), Extension(user("member")), HeaderMap::new()).await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn hook_rules_are_read_by_members_and_written_only_by_admins() {
        let server = gateway(false).await;
        let state = state_for(&server);
        let member = user("member");

        let (status, _) = answer(
            list_hooks(
                State(state.clone()),
                Extension(member.clone()),
                HeaderMap::new(),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = answer(
            create_hook(
                State(state.clone()),
                Extension(member.clone()),
                HeaderMap::new(),
                Json(rule()),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = answer(
            update_hook(
                State(state.clone()),
                Extension(member.clone()),
                HeaderMap::new(),
                Path("ohr_1".to_owned()),
                Json(rule()),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = answer(
            delete_hook(
                State(state),
                Extension(member),
                HeaderMap::new(),
                Path("ohr_1".to_owned()),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(gateway_calls(&server).await, ["GET /v1/org/hooks"]);
    }

    #[tokio::test]
    async fn an_admins_hook_writes_and_the_effective_read_reach_their_gateway_routes() {
        let server = gateway(false).await;
        let state = state_for(&server);
        let admin = user("admin");

        let (status, _) = answer(
            create_hook(
                State(state.clone()),
                Extension(admin.clone()),
                HeaderMap::new(),
                Json(rule()),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = answer(
            update_hook(
                State(state.clone()),
                Extension(admin.clone()),
                HeaderMap::new(),
                Path("ohr_1".to_owned()),
                Json(rule()),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = answer(
            delete_hook(
                State(state.clone()),
                Extension(admin.clone()),
                HeaderMap::new(),
                Path("ohr_1".to_owned()),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = answer(
            get_effective_agent_policy(State(state), Extension(admin), HeaderMap::new()).await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        assert_eq!(
            gateway_calls(&server).await,
            [
                "POST /v1/org/hooks",
                "PUT /v1/org/hooks/ohr_1",
                "DELETE /v1/org/hooks/ohr_1",
                "GET /v1/org/agent-policy/effective",
            ]
        );
    }
}
