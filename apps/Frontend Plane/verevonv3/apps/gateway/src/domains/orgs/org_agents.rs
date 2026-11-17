//! W3-DEC-ORGAGENT (owner decision D19): the org-admin management routes of the org-visible
//! agents. Control (user-core) owns the rows, the canonical owner-or-admin check (auth-core,
//! re-resolved on every call), the lifecycle rules, the room cascade and the audit row. This
//! module is only the Frontend Plane's door to it: session role (defence in depth), body shape
//! check, then a proxy that signs the v1 delegation. It evaluates no policy and holds no state.
//!
//! Order on every route: require_org_admin, id and body shape check, proxy_json. The routes have
//! no flag: they are org-admin gated here and in Control, and carry no content.

use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    envelope::{error, error_with_details},
    middleware::AuthenticatedUser,
    upstream::proxy_json,
};

use super::shared::{actor_for, require_org_admin, GatewayJsonResponse};

const CODE_INVALID: &str = "invalid_request";
const AGENT_ID_SUFFIX_MIN: usize = 6;
const AGENT_ID_SUFFIX_MAX: usize = 64;

fn invalid(field: &str, message: &str) -> GatewayJsonResponse {
    (
        StatusCode::BAD_REQUEST,
        Json(error_with_details(
            CODE_INVALID,
            message,
            json!({ "field": field }),
        )),
    )
}

fn agent_not_found() -> GatewayJsonResponse {
    (
        StatusCode::NOT_FOUND,
        Json(error("agent_not_found", "agent not found")),
    )
}

fn control_url(state: &AppState, suffix: &str) -> String {
    format!(
        "{}/api/v1/internal/agents/org-subjects{suffix}",
        state.user_core_url
    )
}

/// `agent-` followed by 6 to 64 ASCII letters, digits, `_` or `-`: the shape migration 045
/// enforces. Checked here so a malformed id never reaches the signed URL.
fn is_agent_subject_id(id: &str) -> bool {
    let Some(suffix) = id.strip_prefix("agent-") else {
        return false;
    };
    (AGENT_ID_SUFFIX_MIN..=AGENT_ID_SUFFIX_MAX).contains(&suffix.len())
        && suffix
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// The body of a lifecycle change: `{ "expected_revision": <non-negative integer> }` and nothing
/// else. The organization, the actor and the role come from the delegation, never from the body.
/// Returns the body rebuilt from the allowed field, or the refusal.
fn validate_lifecycle_body(body: &Value) -> Result<Value, GatewayJsonResponse> {
    let Some(object) = body.as_object() else {
        return Err(invalid("body", "The request body must be a JSON object."));
    };
    if object.keys().any(|key| key != "expected_revision") {
        return Err(invalid("body", "The request body has an unsupported field."));
    }
    let Some(revision) = object
        .get("expected_revision")
        .and_then(Value::as_i64)
        .filter(|revision| *revision >= 0)
    else {
        return Err(invalid(
            "expected_revision",
            "expected_revision must be a non-negative integer.",
        ));
    };
    Ok(json!({ "expected_revision": revision }))
}

/// Shared by the routes below and the `spaces.*_org_agent` action dispatchers, so both ride the
/// same org-admin check and the same shape check. `suffix` selects the Control route: "" is the
/// definition itself (retire), "/suspend" and "/reactivate" are the two state changes.
pub(crate) async fn change_org_agent(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    agent_subject_id: &str,
    method: Method,
    suffix: &str,
    body: &Value,
) -> GatewayJsonResponse {
    if let Err(response) = require_org_admin(state, user, org_id).await {
        return response;
    }
    if !is_agent_subject_id(agent_subject_id) {
        return agent_not_found();
    }
    let forwarded = match validate_lifecycle_body(body) {
        Ok(forwarded) => forwarded,
        Err(response) => return response,
    };
    proxy_json(
        state,
        method,
        &control_url(state, &format!("/{agent_subject_id}{suffix}")),
        Some(forwarded),
        Some(org_id),
        Some(&actor_for(user)),
        None,
    )
    .await
}

/// GET /api/v1/orgs/:id/agents: the org-visible agents with their current lifecycle.
pub(crate) async fn list_org_agents(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> GatewayJsonResponse {
    if let Err(response) = require_org_admin(&state, &user, &id).await {
        return response;
    }
    proxy_json(
        &state,
        Method::GET,
        &control_url(&state, ""),
        None,
        Some(&id),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

/// PUT /api/v1/orgs/:id/agents/:agent_subject_id/suspend
pub(crate) async fn suspend_org_agent(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((id, agent_subject_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> GatewayJsonResponse {
    change_org_agent(
        &state,
        &user,
        &id,
        &agent_subject_id,
        Method::PUT,
        "/suspend",
        &body,
    )
    .await
}

/// PUT /api/v1/orgs/:id/agents/:agent_subject_id/reactivate
pub(crate) async fn reactivate_org_agent(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((id, agent_subject_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> GatewayJsonResponse {
    change_org_agent(
        &state,
        &user,
        &id,
        &agent_subject_id,
        Method::PUT,
        "/reactivate",
        &body,
    )
    .await
}

/// DELETE /api/v1/orgs/:id/agents/:agent_subject_id: retire the agent (soft delete). The
/// revision travels in the JSON body, as for the other two changes.
pub(crate) async fn retire_org_agent(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((id, agent_subject_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> GatewayJsonResponse {
    change_org_agent(
        &state,
        &user,
        &id,
        &agent_subject_id,
        Method::DELETE,
        "",
        &body,
    )
    .await
}


#[cfg(test)]
mod tests {
    //! W3-DEC-ORGAGENT (D19) through the real router: the org-admin gate and the body shape run before
    //! any Control call, and the revision reaches Control on each lifecycle change (retire carries its
    //! body on DELETE).
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;
    use wiremock::matchers::{body_json, method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SUBJECT: &str = "agent-abc123";

    struct Fixture {
        state: crate::config::AppState,
        user_core: MockServer,
        _auth: MockServer,
    }

    /// A session in org-1 whose organisation role is `role` (the gate reads it from session-context).
    async fn fixture(role: &str) -> Fixture {
        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                "session": {"activeOrganizationId": "org-1"}
            })))
            .mount(&auth)
            .await;
        for slug in ["session-core", "capability-core"] {
            Mock::given(wm_method("GET"))
                .and(wm_path(format!("/api/{slug}/token")))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({"token": "plane-token"})),
                )
                .mount(&auth)
                .await;
        }
        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId": "user-1", "orgId": "org-1", "role": role, "onboardingStatus": "COMPLETED"
            })))
            .mount(&user_core)
            .await;
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        Fixture {
            state,
            user_core,
            _auth: auth,
        }
    }

    async fn call(
        state: crate::config::AppState,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (u16, Vec<u8>) {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("cookie", "better-auth.session_token=session-1");
        if body.is_some() {
            builder = builder.header("content-type", "application/json");
        }
        let request = builder
            .body(Body::from(body.map(|value| value.to_string()).unwrap_or_default()))
            .unwrap();
        let response = crate::build_router(state)
            .oneshot(request)
            .await
            .expect("the router answers");
        let status = response.status().as_u16();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("a body")
            .to_bytes()
            .to_vec();
        (status, bytes)
    }

    fn json_of(bytes: &[u8]) -> Value {
        serde_json::from_slice(bytes).unwrap_or(Value::Null)
    }

    async fn control_calls(user_core: &MockServer) -> usize {
        user_core
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .filter(|request| request.url.path().contains("/internal/agents/org-subjects"))
            .count()
    }

    fn suspend_uri(subject: &str) -> String {
        format!("/api/v1/orgs/org-1/agents/{subject}/suspend")
    }

    #[tokio::test]
    async fn a_non_admin_is_refused_before_any_control_call() {
        let fixture = fixture("member").await;
        let (status, body) = call(
            fixture.state.clone(),
            "PUT",
            &suspend_uri(SUBJECT),
            Some(json!({"expected_revision": 3})),
        )
        .await;
        assert_eq!(status, 403);
        assert_eq!(json_of(&body)["error"]["code"], "forbidden");
        assert_eq!(control_calls(&fixture.user_core).await, 0);
    }

    #[tokio::test]
    async fn a_malformed_agent_id_is_404_before_any_control_call() {
        let fixture = fixture("admin").await;
        let (status, body) = call(
            fixture.state.clone(),
            "PUT",
            &suspend_uri("agent-x"),
            Some(json!({"expected_revision": 3})),
        )
        .await;
        assert_eq!(status, 404);
        assert_eq!(json_of(&body)["error"]["code"], "agent_not_found");
        assert_eq!(control_calls(&fixture.user_core).await, 0);
    }

    #[tokio::test]
    async fn an_extra_field_or_a_bad_revision_is_400_before_any_control_call() {
        let fixture = fixture("owner").await;
        for body in [
            json!({"expected_revision": 3, "role": "owner"}),
            json!({}),
            json!({"expected_revision": -1}),
            json!([1]),
        ] {
            let (status, reply) = call(
                fixture.state.clone(),
                "PUT",
                &suspend_uri(SUBJECT),
                Some(body.clone()),
            )
            .await;
            assert_eq!(status, 400, "body {body}");
            assert_eq!(json_of(&reply)["error"]["code"], "invalid_request", "body {body}");
        }
        assert_eq!(control_calls(&fixture.user_core).await, 0);
    }

    #[tokio::test]
    async fn the_revision_reaches_control_on_suspend_reactivate_and_retire() {
        let fixture = fixture("admin").await;
        for tail in ["suspend", "reactivate"] {
            Mock::given(wm_method("PUT"))
                .and(wm_path(format!(
                    "/api/v1/internal/agents/org-subjects/{SUBJECT}/{tail}"
                )))
                .and(body_json(json!({"expected_revision": 3})))
                .respond_with(ResponseTemplate::new(200).set_body_json(
                    json!({"data": {"agent_subject_id": SUBJECT, "revision": 4}}),
                ))
                .expect(1)
                .mount(&fixture.user_core)
                .await;
        }
        Mock::given(wm_method("DELETE"))
            .and(wm_path(format!("/api/v1/internal/agents/org-subjects/{SUBJECT}")))
            .and(body_json(json!({"expected_revision": 3})))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"data": {"agent_subject_id": SUBJECT, "retired": true}}),
            ))
            .expect(1)
            .mount(&fixture.user_core)
            .await;

        for (method, uri) in [
            ("PUT", suspend_uri(SUBJECT)),
            ("PUT", format!("/api/v1/orgs/org-1/agents/{SUBJECT}/reactivate")),
            ("DELETE", format!("/api/v1/orgs/org-1/agents/{SUBJECT}")),
        ] {
            let (status, body) = call(
                fixture.state.clone(),
                method,
                &uri,
                Some(json!({"expected_revision": 3})),
            )
            .await;
            assert_eq!(status, 200, "{method} {uri}: {}", json_of(&body));
        }
        fixture.user_core.verify().await;
    }
}
