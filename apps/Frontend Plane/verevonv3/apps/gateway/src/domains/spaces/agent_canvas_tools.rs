//! W3-DEC-CANVAS (D2): the room agent's canvas-tools binding switch and the gateway's part of its policy.
//!
//! `PUT /api/v1/spaces/{space_ref}/agents/{binding_ref}/canvas-tools` with body `{enabled: boolean}` turns one bound
//! agent's room canvas tools (`space.canvas.read`, `space.canvas.write`) on or off in this room. It is a switch of its
//! own, separate from the self-API switch (rename, pin and post card): turning one on never offers the other. Only the
//! room's owner or a manager, decided from Control's live membership BEFORE any Application request, may write it. The
//! Convex mutation (`spaceAgents:setSpaceAgentCanvasToolsForGateway`) re-checks that the binding belongs to this Space
//! and organization and is `active`, then records the switch on the binding.
//!
//! The switch grants nothing. A run gets canvas tools only when Control's run-action decision allows each action and
//! Data Plane's canvas checks pass. On a mention turn the feature `space_canvas_tools` is added only when the flag is on,
//! this binding's switch is on and the organization is not zero-retention ([`canvas_tools_allowed`]); every other turn has
//! the feature removed. Behind `SpaceFeatureFlags::agent_canvas_tools_api` (off answers 404 `feature_disabled`), a rollout
//! control and never an authorization input. The switch reaches the room's agent list as `canvas_tools_enabled`, so this
//! module has no read route of its own.

use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    routing::put,
    Json, Router,
};
use serde_json::{json, Map, Value};

use super::room_admin::convex_mutation_message;
use crate::{config::AppState, envelope::error, middleware::AuthenticatedUser};

type Reply = (StatusCode, Json<Value>);

/// The feature a mention turn carries when its binding allows the canvas tools. Model Gateway honours it only for a
/// Space-scoped agentic turn that is not zero-retention (`space_routes::self_api::tool_profile_for_run`).
pub(crate) const SPACE_CANVAS_TOOLS_FEATURE: &str = "space_canvas_tools";

fn invalid_request(reason: &str) -> Reply {
    (
        StatusCode::BAD_REQUEST,
        Json(crate::envelope::error_with_details(
            "invalid_canvas_tools_request",
            "The agent canvas-tools setting is not valid.",
            json!({"reason": reason}),
        )),
    )
}

/// The validated request: `enabled` is the only field, and it must be a JSON boolean.
fn parse_enabled(body: &Value) -> Result<bool, Reply> {
    let Some(object) = body.as_object() else {
        return Err(invalid_request("body_not_an_object"));
    };
    if let Some(field) = object.keys().find(|key| key.as_str() != "enabled") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(crate::envelope::error_with_details(
                "invalid_canvas_tools_request",
                "Only enabled can be set here.",
                json!({"field": field.chars().take(64).collect::<String>()}),
            )),
        ));
    }
    object
        .get("enabled")
        .and_then(Value::as_bool)
        .ok_or_else(|| invalid_request("enabled_required"))
}

fn unavailable() -> Reply {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(error(
            "agent_canvas_tools_unavailable",
            "The agent canvas-tools setting could not be changed. Nothing was altered.",
        )),
    )
}

/// Maps Application's stable mutation messages to the typed browser errors.
fn map_canvas_tools_failure(message: Option<&str>) -> Reply {
    let Some(message) = message else {
        return unavailable();
    };
    if message.contains("agent_binding_not_found") {
        return (
            StatusCode::NOT_FOUND,
            Json(error(
                "agent_binding_not_found",
                "That agent is not bound to this room.",
            )),
        );
    }
    if message.contains("agent_binding_not_active") {
        return (
            StatusCode::CONFLICT,
            Json(error(
                "agent_binding_not_active",
                "That agent is not active in this room.",
            )),
        );
    }
    if message.contains("invalid_canvas_tools_request") {
        return invalid_request("rejected");
    }
    unavailable()
}

/// Axum handler for `PUT /api/v1/spaces/{space_ref}/agents/{binding_ref}/canvas-tools`.
async fn set_space_agent_canvas_tools_route(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((space_ref, binding_ref)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Reply {
    set_space_agent_canvas_tools(&state, &user, &space_ref, &binding_ref, &body).await
}

/// The switch's writer, shared with the `spaces.set_agent_canvas_tools` action so the flag, the owner-or-manager
/// gate and the typed errors exist once.
pub(crate) async fn set_space_agent_canvas_tools(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
    binding_ref: &str,
    body: &Value,
) -> Reply {
    // The action reaches this function without the route, so the switch is checked here too.
    if !state.space_features.agent_canvas_tools_api {
        return (
            StatusCode::NOT_FOUND,
            Json(error("feature_disabled", "Agent canvas tools are not enabled.")),
        );
    }
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    let space_ref = space_ref.trim();
    let binding_ref = binding_ref.trim();
    if org_id.is_empty() || space_ref.is_empty() {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "active_membership_required",
                "An active organization membership and Space are required.",
            )),
        );
    }
    if binding_ref.is_empty() {
        return invalid_request("binding_ref_required");
    }
    let enabled = match parse_enabled(body) {
        Ok(enabled) => enabled,
        Err(reply) => return reply,
    };

    // Role first, and against THIS room, from Control's live membership: owner or manager only.
    match super::resolve_space_role(state, user, &org_id, space_ref).await {
        Ok(role) if super::CREATE_AGENT_ROLES.contains(&role.as_str()) => {}
        Ok(_) => {
            return (
                StatusCode::FORBIDDEN,
                Json(error(
                    "space_role_cannot_manage_agent",
                    "Only a Space owner or manager can turn agent canvas tools on or off.",
                )),
            )
        }
        Err(refusal) => return refusal,
    }

    let args = json!({
        "externalAuthId": user.user_id,
        "externalOrgId": org_id,
        "spaceRef": space_ref,
        "bindingRef": binding_ref,
        "enabled": enabled,
    });
    match convex_mutation_message(state, "spaceAgents:setSpaceAgentCanvasToolsForGateway", args).await {
        Ok(result) => (
            StatusCode::OK,
            Json(json!({"data": {
                "binding_ref": result.get("bindingRef").and_then(Value::as_str).unwrap_or(binding_ref),
                "canvas_tools_enabled": result.get("canvasToolsEnabled").and_then(Value::as_bool).unwrap_or(enabled),
                "projection_version": result.get("projectionVersion").cloned().unwrap_or(Value::Null),
                "changed": result.get("changed").and_then(Value::as_bool).unwrap_or(false),
            }})),
        ),
        Err(message) => map_canvas_tools_failure(message.as_deref()),
    }
}

/// The one route. It checks the flag inside the handler, as the room's other feature routes do.
pub(super) fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/v1/spaces/{space_ref}/agents/{binding_ref}/canvas-tools",
        put(set_space_agent_canvas_tools_route),
    )
}

/// Whether one bound agent's canvas tools apply to a mention turn: the feature flag is on, this binding's switch is on,
/// and the organization is not zero-retention (the feature is simply not added for a ZDR org). The binding's own switch
/// is the only per-agent decision; the flag and the posture are the rollout and the privacy floor.
pub(crate) async fn canvas_tools_allowed(
    state: &AppState,
    user: &AuthenticatedUser,
    agent: &Value,
) -> bool {
    if !state.space_features.agent_canvas_tools_api {
        return false;
    }
    if agent.get("canvas_tools_enabled").and_then(Value::as_bool) != Some(true) {
        return false;
    }
    !crate::zdr::org_zdr_lookup(state, user).await.treat_as_zdr()
}

/// Removes a client-named `space_canvas_tools` from a chat body's features. Only the binding policy adds it.
pub(crate) fn strip_canvas_tools_feature(object: &mut Map<String, Value>) {
    if let Some(features) = object.get_mut("features").and_then(Value::as_array_mut) {
        features.retain(|feature| feature.as_str() != Some(SPACE_CANVAS_TOOLS_FEATURE));
    }
}

/// The canvas-tools grant on an allowed mention turn: `agentic` (the run's tool loop; execution-core then offers the
/// canvas rows only under the `space_canvas_tools` marker), `space_canvas_tools`, and `plan_mode` off, because plan
/// mode refuses registry writes.
pub(crate) fn grant_canvas_tools_feature(object: &mut Map<String, Value>) {
    let mut features: Vec<Value> = object
        .get("features")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    features.retain(|feature| feature.as_str() != Some(SPACE_CANVAS_TOOLS_FEATURE));
    if !features.iter().any(|feature| feature.as_str() == Some("agentic")) {
        features.push(json!("agentic"));
    }
    features.push(json!(SPACE_CANVAS_TOOLS_FEATURE));
    object.insert("features".to_owned(), Value::Array(features));
    object.insert("plan_mode".to_owned(), json!(false));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::AuthorizedMembership;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SWITCH: &str = "/api/v1/spaces/space-room/agents/binding-1/canvas-tools";

    fn user() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.com".to_owned(),
            user_name: "User One".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: Some(AuthorizedMembership {
                organization_id: "org-1".to_owned(),
                role: "member".to_owned(),
            }),
        }
    }

    /// The refusal a parse gave, as the browser sees it.
    fn refused(result: Result<bool, Reply>) -> (StatusCode, Value) {
        let (status, Json(body)) = result.err().expect("the request is refused");
        (status, body)
    }

    fn mutation_ok(enabled: bool) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"value": {
            "bindingRef": "binding-1",
            "canvasToolsEnabled": enabled,
            "projectionVersion": 3,
            "changed": true
        }}))
    }

    /// The doubles the switch reaches: the session, Control's Space membership (`space_role`), org-core's posture
    /// (`zdr`) and Application's mutation answer.
    struct Doubles {
        state: AppState,
        user_core: MockServer,
        convex: MockServer,
        _others: Vec<MockServer>,
    }

    async fn doubles(space_role: &str, zdr: bool, mutation: ResponseTemplate) -> Doubles {
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
                "userId": "user-1", "orgId": "org-1", "role": "member", "onboardingStatus": "COMPLETED"
            })))
            .mount(&user_core)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/internal/spaces/space-room/membership"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"space_ref": "space-room", "kind": "room", "role": space_role}
            })))
            .mount(&user_core)
            .await;
        let org_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/organizations/org-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"metadata": {"interactiveRetention": {"zdr": zdr}}}
            })))
            .mount(&org_core)
            .await;
        let convex = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/mutation"))
            .respond_with(mutation)
            .mount(&convex)
            .await;
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.org_core_url = org_core.uri();
        state.application_convex_url = convex.uri();
        state.application_convex_service_key = "application-test-key".into();
        Doubles {
            state,
            user_core,
            convex,
            _others: vec![auth, org_core],
        }
    }

    async fn requests(server: &MockServer, path_part: &str) -> Vec<wiremock::Request> {
        server
            .received_requests()
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|request| request.url.path().contains(path_part))
            .collect()
    }

    async fn send_put(state: AppState, uri: &str, body: Value) -> (u16, Value) {
        let request = Request::builder()
            .method("PUT")
            .uri(uri)
            .header("cookie", "better-auth.session_token=session-1")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .expect("a request");
        let response = crate::build_router(state)
            .oneshot(request)
            .await
            .expect("the router answers");
        let status = response.status().as_u16();
        let bytes = response.into_body().collect().await.expect("a body").to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    #[test]
    fn parse_enabled_takes_only_a_boolean_enabled_field() {
        assert_eq!(parse_enabled(&json!({"enabled": true})).ok(), Some(true));
        assert_eq!(parse_enabled(&json!({"enabled": false})).ok(), Some(false));
        for body in [
            json!({"enabled": "true"}),
            json!({"enabled": 1}),
            json!({"enabled": null}),
            json!({}),
        ] {
            let (status, value) = refused(parse_enabled(&body));
            assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
            assert_eq!(value["error"]["code"], "invalid_canvas_tools_request", "{body}");
            assert_eq!(value["error"]["details"]["reason"], "enabled_required", "{body}");
        }
        let (status, value) = refused(parse_enabled(&json!({"enabled": true, "space_id": "room-1"})));
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(value["error"]["code"], "invalid_canvas_tools_request");
        assert_eq!(value["error"]["details"]["field"], "space_id");
        let (_, value) = refused(parse_enabled(&json!(true)));
        assert_eq!(value["error"]["details"]["reason"], "body_not_an_object");
    }

    #[test]
    fn a_failed_switch_maps_each_stable_message_to_its_typed_status() {
        for (message, status, code) in [
            (Some("agent_binding_not_found"), 404, "agent_binding_not_found"),
            (Some("Error: agent_binding_not_active in this room"), 409, "agent_binding_not_active"),
            (Some("invalid_canvas_tools_request"), 400, "invalid_canvas_tools_request"),
            (Some("a failure Application did not name"), 503, "agent_canvas_tools_unavailable"),
            (None, 503, "agent_canvas_tools_unavailable"),
        ] {
            let (got, Json(body)) = map_canvas_tools_failure(message);
            assert_eq!(got.as_u16(), status, "{message:?}");
            assert_eq!(body["error"]["code"], code, "{message:?}");
        }
    }

    #[test]
    fn strip_removes_only_the_canvas_marker() {
        let mut object = json!({"features": ["tools", "space_canvas_tools", "agentic", "space_self_api"]})
            .as_object()
            .cloned()
            .unwrap();
        strip_canvas_tools_feature(&mut object);
        assert_eq!(object["features"], json!(["tools", "agentic", "space_self_api"]));
    }

    #[test]
    fn grant_adds_agentic_and_the_canvas_marker_once_and_turns_plan_mode_off() {
        let mut object = json!({"features": ["space_canvas_tools", "tools"], "plan_mode": true})
            .as_object()
            .cloned()
            .unwrap();
        grant_canvas_tools_feature(&mut object);
        grant_canvas_tools_feature(&mut object);
        assert_eq!(object["features"], json!(["tools", "agentic", "space_canvas_tools"]));
        assert_eq!(object["plan_mode"], json!(false));
        let mut bare = json!({}).as_object().cloned().unwrap();
        grant_canvas_tools_feature(&mut bare);
        assert_eq!(bare["features"], json!(["agentic", "space_canvas_tools"]));
        assert_eq!(bare["plan_mode"], json!(false));
    }

    #[tokio::test]
    async fn the_switch_is_404_and_calls_nothing_while_its_flag_is_off() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let d = doubles("owner", false, mutation_ok(true)).await;
        let (status, body) = send_put(d.state.clone(), SWITCH, json!({"enabled": true})).await;
        assert_eq!(status, 404);
        assert_eq!(body["error"]["code"], "feature_disabled");
        assert!(requests(&d.user_core, "/membership").await.is_empty());
        assert!(requests(&d.convex, "/api/mutation").await.is_empty());
    }

    #[tokio::test]
    async fn an_editor_is_refused_before_any_convex_call() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut d = doubles("editor", false, mutation_ok(true)).await;
        d.state.space_features.agent_canvas_tools_api = true;
        let (status, body) = send_put(d.state.clone(), SWITCH, json!({"enabled": true})).await;
        assert_eq!(status, 403);
        assert_eq!(body["error"]["code"], "space_role_cannot_manage_agent");
        assert!(requests(&d.convex, "/api/mutation").await.is_empty());
    }

    #[tokio::test]
    async fn an_owner_or_manager_sets_the_switch_and_gets_the_projection() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for role in ["owner", "manager"] {
            let mut d = doubles(role, false, mutation_ok(true)).await;
            d.state.space_features.agent_canvas_tools_api = true;
            let (status, body) = send_put(d.state.clone(), SWITCH, json!({"enabled": true})).await;
            assert_eq!(status, 200, "{role}");
            assert_eq!(body["data"]["binding_ref"], "binding-1", "{role}");
            assert_eq!(body["data"]["canvas_tools_enabled"], true, "{role}");
            let sent = requests(&d.convex, "/api/mutation").await;
            assert_eq!(sent.len(), 1, "{role}");
            let sent: Value = serde_json::from_slice(&sent[0].body).expect("a JSON mutation");
            assert_eq!(sent["path"], "spaceAgents:setSpaceAgentCanvasToolsForGateway", "{role}");
            assert_eq!(sent["args"]["externalOrgId"], "org-1", "{role}");
            assert_eq!(sent["args"]["spaceRef"], "space-room", "{role}");
            assert_eq!(sent["args"]["bindingRef"], "binding-1", "{role}");
            assert_eq!(sent["args"]["enabled"], true, "{role}");
        }
    }

    #[tokio::test]
    async fn an_inactive_binding_answers_409() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut d = doubles(
            "owner",
            false,
            ResponseTemplate::new(200)
                .set_body_json(json!({"status": "error", "errorMessage": "agent_binding_not_active"})),
        )
        .await;
        d.state.space_features.agent_canvas_tools_api = true;
        let (status, body) = send_put(d.state.clone(), SWITCH, json!({"enabled": true})).await;
        assert_eq!(status, 409);
        assert_eq!(body["error"]["code"], "agent_binding_not_active");
    }

    #[tokio::test]
    async fn a_zdr_org_can_flip_the_switch_but_gets_no_canvas_tools() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut zdr = doubles("manager", true, mutation_ok(true)).await;
        zdr.state.space_features.agent_canvas_tools_api = true;
        let (status, body) = send_put(zdr.state.clone(), SWITCH, json!({"enabled": true})).await;
        assert_eq!(status, 200);
        assert_eq!(body["data"]["canvas_tools_enabled"], true);
        let enabled = json!({"canvas_tools_enabled": true});
        assert!(
            !canvas_tools_allowed(&zdr.state, &user(), &enabled).await,
            "a ZDR org gets no canvas tools"
        );
        // Positive control: the same binding switch in a non-ZDR org does apply.
        let mut open = doubles("manager", false, mutation_ok(true)).await;
        open.state.space_features.agent_canvas_tools_api = true;
        assert!(canvas_tools_allowed(&open.state, &user(), &enabled).await);
    }
}
