//! W3-EP-6-S9-pin, BFF half: `PUT /api/v1/spaces/{space_ref}/agents/{binding_ref}/pin` through the
//! real router with mock Auth, Control (user-core membership) and Application (Convex).
//! Off: the path is not in the route table (plain 404, no Convex call) and the shared function
//! answers `feature_disabled`. On: only an owner or manager reaches Convex, the version travels as
//! `pinnedVersion` (absent for `null`, which clears), and Convex's refusals map to typed errors.

use super::{parse_pin, set_space_agent_pin, PinRequest};
use crate::config::AppState;
use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;
use wiremock::matchers::{method as wm_method, path as wm_path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PIN: &str = "/api/v1/spaces/space-room/agents/sab_1/pin";

struct Fx {
    state: AppState,
    application: MockServer,
    _auth: MockServer,
    _user_core: MockServer,
}

async fn fx(role: &str, pinning: bool, mutation: ResponseTemplate) -> Fx {
    let auth = MockServer::start().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/api/auth/get-session"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
            "session": {"activeOrganizationId": "org-1"}
        })))
        .mount(&auth)
        .await;
    let user_core = MockServer::start().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/api/v1/me/session-context"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "userId":"user-1", "orgId":"org-1", "role":"member", "onboardingStatus":"COMPLETED"
        })))
        .mount(&user_core)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/api/v1/internal/spaces/space-room/membership"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {"space_ref": "space-room", "kind": "room", "role": role}
        })))
        .mount(&user_core)
        .await;
    let application = MockServer::start().await;
    Mock::given(wm_method("POST"))
        .and(wm_path("/api/mutation"))
        .respond_with(mutation)
        .mount(&application)
        .await;
    let mut state = crate::tests::test_state(false);
    state.application_convex_url = application.uri();
    state.application_convex_service_key = "application-test-key".into();
    state.auth_core_url = auth.uri();
    state.user_core_url = user_core.uri();
    state.space_features.agent_version_pinning = pinning;
    Fx {
        state,
        application,
        _auth: auth,
        _user_core: user_core,
    }
}

fn pinned(version: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({"value": {
        "bindingRef": "sab_1", "subjectId": "agent-agent-1", "pinnedVersion": version,
        "projectionVersion": 5, "changed": true
    }}))
}

fn convex_error(message: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({"status": "error", "errorMessage": message}))
}

async fn put_pin(fx: &Fx, body: Value) -> (u16, Value) {
    let response = crate::build_router(fx.state.clone())
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(PIN)
                .header("cookie", "better-auth.session_token=session-1")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status().as_u16();
    let payload: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
            .unwrap_or(Value::Null);
    (status, payload)
}

async fn convex_calls(fx: &Fx) -> Vec<Value> {
    fx.application
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|request| serde_json::from_slice(&request.body).unwrap_or(Value::Null))
        .collect()
}

fn user() -> crate::middleware::AuthenticatedUser {
    crate::middleware::AuthenticatedUser {
        user_id: "user-1".to_owned(),
        user_email: "user@example.invalid".to_owned(),
        user_name: "User".to_owned(),
        user_image: None,
        email_verified: true,
        auth_role: Some("member".to_owned()),
        active_org_id: Some("org-1".to_owned()),
        authorized_membership: Some(crate::middleware::AuthorizedMembership {
            organization_id: "org-1".to_owned(),
            role: "member".to_owned(),
        }),
    }
}

#[test]
fn parse_pin_accepts_a_positive_whole_version_or_null_and_nothing_else() {
    assert_eq!(
        parse_pin(&json!({"version": 3})).unwrap(),
        PinRequest { version: Some(3) }
    );
    assert_eq!(
        parse_pin(&json!({"version": 3.0})).unwrap(),
        PinRequest { version: Some(3) }
    );
    assert_eq!(
        parse_pin(&json!({"version": null})).unwrap(),
        PinRequest { version: None }
    );
    for body in [
        json!({}),
        json!({"version": 0}),
        json!({"version": -1}),
        json!({"version": 1.5}),
        json!({"version": "2"}),
        json!([1]),
        json!({"version": 1, "agent_id": "forged"}),
    ] {
        let Err((status, axum::Json(payload))) = parse_pin(&body) else {
            panic!("expected a refusal for {body}");
        };
        assert_eq!(status.as_u16(), 400, "{body}");
        let code = payload["error"]["code"].as_str().unwrap_or_default();
        assert!(
            code == "invalid_policy" || code == "unsupported_policy_field",
            "{body}: {code}"
        );
    }
}

#[tokio::test]
async fn switch_off_the_pin_path_is_absent_and_convex_is_never_asked() {
    let _env = crate::config::TEST_ENV_LOCK.lock().await;
    let fx = fx("owner", false, pinned(json!(1))).await;
    let (status, _) = put_pin(&fx, json!({"version": 1})).await;
    assert_eq!(status, 404);
    assert!(convex_calls(&fx).await.is_empty());
}

#[tokio::test]
async fn switch_off_the_shared_function_answers_feature_disabled_without_convex() {
    let _env = crate::config::TEST_ENV_LOCK.lock().await;
    let fx = fx("owner", false, pinned(json!(1))).await;
    let (status, axum::Json(payload)) = set_space_agent_pin(
        &fx.state,
        &user(),
        "space-room",
        "sab_1",
        &json!({"version": 1}),
    )
    .await;
    assert_eq!(status.as_u16(), 404);
    assert_eq!(payload["error"]["code"], "feature_disabled");
    assert!(convex_calls(&fx).await.is_empty());
}

#[tokio::test]
async fn an_editor_or_viewer_is_forbidden_before_convex() {
    let _env = crate::config::TEST_ENV_LOCK.lock().await;
    for role in ["editor", "viewer"] {
        let fx = fx(role, true, pinned(json!(1))).await;
        let (status, payload) = put_pin(&fx, json!({"version": 1})).await;
        assert_eq!(status, 403, "{role}");
        assert_eq!(payload["error"]["code"], "forbidden", "{role}");
        assert!(convex_calls(&fx).await.is_empty(), "{role}");
    }
}

#[tokio::test]
async fn an_owner_or_manager_pins_with_the_session_identity() {
    let _env = crate::config::TEST_ENV_LOCK.lock().await;
    for role in ["owner", "manager"] {
        let fx = fx(role, true, pinned(json!(2))).await;
        let (status, payload) = put_pin(&fx, json!({"version": 2})).await;
        assert_eq!(status, 200, "{role}: {payload}");
        assert_eq!(payload["data"]["binding_ref"], "sab_1");
        assert_eq!(payload["data"]["pinned_version"], 2);
        assert_eq!(payload["data"]["projection_version"], 5);
        assert_eq!(payload["data"]["changed"], true);
        let calls = convex_calls(&fx).await;
        assert_eq!(calls.len(), 1, "{role}");
        assert_eq!(
            calls[0]["path"],
            "spaceAgents:setSpaceAgentBindingPinForGateway"
        );
        let args = &calls[0]["args"];
        assert_eq!(args["externalAuthId"], "user-1");
        assert_eq!(args["externalOrgId"], "org-1");
        assert_eq!(args["spaceRef"], "space-room");
        assert_eq!(args["bindingRef"], "sab_1");
        assert_eq!(args["pinnedVersion"], 2);
    }
}

#[tokio::test]
async fn a_null_version_clears_by_sending_no_pinned_version() {
    let _env = crate::config::TEST_ENV_LOCK.lock().await;
    let fx = fx("owner", true, pinned(Value::Null)).await;
    let (status, payload) = put_pin(&fx, json!({"version": null})).await;
    assert_eq!(status, 200, "{payload}");
    assert_eq!(payload["data"]["pinned_version"], Value::Null);
    let calls = convex_calls(&fx).await;
    assert_eq!(calls.len(), 1);
    assert!(
        calls[0]["args"].get("pinnedVersion").is_none(),
        "null must be absent, not null"
    );
}

#[tokio::test]
async fn convex_refusals_map_to_typed_errors_and_never_leak_the_raw_text() {
    let _env = crate::config::TEST_ENV_LOCK.lock().await;
    let cases: Vec<(ResponseTemplate, u16, &str)> = vec![
        (
            convex_error("Uncaught Error: agent_version_not_found at spaceAgents.ts:952"),
            404,
            "agent_version_not_found",
        ),
        (
            convex_error("Uncaught Error: agent_binding_not_found"),
            404,
            "agent_binding_not_found",
        ),
        (
            convex_error("Uncaught Error: invalid_policy"),
            400,
            "invalid_policy",
        ),
        (
            convex_error("Server Error at secret-internal-host:3210 stack trace"),
            503,
            "agent_binding_unavailable",
        ),
        (
            ResponseTemplate::new(500).set_body_string("secret-internal-host exploded"),
            503,
            "agent_binding_unavailable",
        ),
    ];
    for (template, expected_status, expected_code) in cases {
        let fx = fx("owner", true, template).await;
        let (status, payload) = put_pin(&fx, json!({"version": 7})).await;
        assert_eq!(status, expected_status, "{expected_code}");
        assert_eq!(payload["error"]["code"], expected_code);
        let text = payload.to_string();
        assert!(!text.contains("secret-internal-host"));
        assert!(!text.contains("spaceAgents.ts"));
        assert!(!text.contains("stack trace"));
    }
}
