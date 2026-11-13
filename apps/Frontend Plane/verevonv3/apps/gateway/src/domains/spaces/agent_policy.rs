//! WS08-S5: the binding wake policy writer.
//!
//! `PATCH /api/v1/spaces/{space_ref}/agents/{binding_ref}/policy` body
//! `{respond_to, respond_to_subjects?, expected_updated_at}` sets who may wake one bound agent
//! in this room (`members` = any editor, manager or owner, `managers`, or an `allowlist` of at
//! most 50 Control subject ids). It is a human decision: only the room's owner or a manager,
//! decided from Control's live membership BEFORE any Application request, may write it. The
//! Convex mutation re-checks that the binding belongs to this Space and organization, compares
//! `expected_updated_at` (a second manager's concurrent change answers 409 `policy_conflict`)
//! and appends one `spaceAgentPolicyEvents` row. The policy narrows or widens who the mention
//! path lets wake the agent; it grants nothing, and Control still decides who may post or wake.
//! No message content moves here, so Zero Data Retention organizations behave the same and no
//! model is called. Behind `SpaceFeatureFlags::agent_wake_policy_api` (off answers 404
//! `feature_disabled`), a rollout control and never an authorization input.
//!
//! EP-6 S9: the agent version pin, `PUT /api/v1/spaces/{space_ref}/agents/{binding_ref}/pin` body
//! `{version: number | null}`, sits beside the wake policy writer: the same owner-or-manager gate
//! (refused as `forbidden`), the same typed envelope, and one Convex mutation
//! (`spaceAgents:setSpaceAgentBindingPinForGateway`). It selects one published version of the bound
//! agent (`null` clears the pin). It is registered only behind `SpaceFeatureFlags::agent_version_pinning`
//! (see [`routes`]).

use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    routing::put,
    Json, Router,
};
use serde_json::{json, Value};

use super::room_admin::convex_mutation_message;
use crate::{config::AppState, envelope::error, middleware::AuthenticatedUser};

type Reply = (StatusCode, Json<Value>);

/// Most subject ids an allowlist may carry (the Convex mutation enforces the same bound).
const MAX_RESPOND_TO_SUBJECTS: usize = 50;
const MAX_SUBJECT_ID_CHARS: usize = 128;
const ALLOWED_FIELDS: [&str; 3] = ["respond_to", "respond_to_subjects", "expected_updated_at"];

fn invalid_policy(reason: &str) -> Reply {
    (
        StatusCode::BAD_REQUEST,
        Json(crate::envelope::error_with_details(
            "invalid_policy",
            "The wake policy is not valid.",
            json!({"reason": reason}),
        )),
    )
}

fn unavailable() -> Reply {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(error(
            "agent_binding_unavailable",
            "The agent's wake policy could not be changed. Nothing was altered.",
        )),
    )
}

/// The validated request: nothing here has been trusted yet except its shape.
#[derive(Debug, PartialEq)]
struct PolicyRequest {
    respond_to: String,
    respond_to_subjects: Vec<String>,
    expected_updated_at: u64,
}

fn parse_policy(body: &Value) -> Result<PolicyRequest, Reply> {
    let Some(object) = body.as_object() else {
        return Err(invalid_policy("body_not_an_object"));
    };
    if let Some(field) = object.keys().find(|key| !ALLOWED_FIELDS.contains(&key.as_str())) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(crate::envelope::error_with_details(
                "unsupported_policy_field",
                "Only respond_to, respond_to_subjects and expected_updated_at can be set here.",
                json!({"field": field.chars().take(64).collect::<String>()}),
            )),
        ));
    }
    // `group` is retired and is not a value here: only the three modes below exist.
    let respond_to = match object.get("respond_to").and_then(Value::as_str).map(str::trim) {
        Some(mode @ ("members" | "managers" | "allowlist")) => mode.to_owned(),
        Some(_) => return Err(invalid_policy("unknown_respond_to")),
        None => return Err(invalid_policy("respond_to_required")),
    };
    let mut subjects: Vec<String> = Vec::new();
    match object.get("respond_to_subjects") {
        None | Some(Value::Null) => {}
        Some(Value::Array(items)) => {
            for item in items {
                let Some(id) = item.as_str().map(str::trim) else {
                    return Err(invalid_policy("bad_subject_id"));
                };
                if id.is_empty() || id.chars().count() > MAX_SUBJECT_ID_CHARS {
                    return Err(invalid_policy("bad_subject_id"));
                }
                if !subjects.iter().any(|existing| existing == id) {
                    subjects.push(id.to_owned());
                }
            }
        }
        Some(_) => return Err(invalid_policy("bad_subject_list")),
    }
    if subjects.len() > MAX_RESPOND_TO_SUBJECTS {
        return Err(invalid_policy("too_many_subjects"));
    }
    if respond_to == "allowlist" && subjects.is_empty() {
        return Err(invalid_policy("allowlist_empty"));
    }
    if respond_to != "allowlist" && !subjects.is_empty() {
        return Err(invalid_policy("subjects_need_allowlist"));
    }
    let expected_updated_at = object
        .get("expected_updated_at")
        .and_then(|value| value.as_u64().or_else(|| value.as_f64().filter(|f| *f >= 0.0 && f.fract() == 0.0).map(|f| f as u64)))
        .ok_or_else(|| invalid_policy("expected_updated_at_required"))?;
    Ok(PolicyRequest {
        respond_to,
        respond_to_subjects: subjects,
        expected_updated_at,
    })
}

/// Maps Application's stable mutation messages to the typed browser errors.
fn map_policy_failure(message: Option<&str>) -> Reply {
    let Some(message) = message else {
        return unavailable();
    };
    if message.contains("policy_conflict") {
        return (
            StatusCode::CONFLICT,
            Json(error(
                "policy_conflict",
                "The agent's wake policy changed since you loaded it. Reload and try again.",
            )),
        );
    }
    if message.contains("agent_binding_not_found") {
        return (
            StatusCode::NOT_FOUND,
            Json(error(
                "agent_binding_not_found",
                "That agent is not bound to this room.",
            )),
        );
    }
    if message.contains("invalid_policy") {
        return invalid_policy("rejected");
    }
    unavailable()
}

/// Axum handler for `PATCH /api/v1/spaces/{space_ref}/agents/{binding_ref}/policy`.
pub(super) async fn set_space_agent_wake_policy_route(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((space_ref, binding_ref)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Reply {
    set_space_agent_wake_policy(&state, &user, &space_ref, &binding_ref, &body).await
}

/// The route's body, shared with the `spaces.set_agent_wake_policy` action so the flag, the
/// owner-or-manager gate and the typed errors exist once.
pub(crate) async fn set_space_agent_wake_policy(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
    binding_ref: &str,
    body: &Value,
) -> Reply {
    if !state.space_features.agent_wake_policy_api {
        return (
            StatusCode::NOT_FOUND,
            Json(error(
                "feature_disabled",
                "Agent wake policy is not enabled.",
            )),
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
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "binding_ref_required",
                "A binding reference is required.",
            )),
        );
    }
    let request = match parse_policy(body) {
        Ok(request) => request,
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
                    "Only a Space owner or manager can change who may wake an agent.",
                )),
            )
        }
        Err(refusal) => return refusal,
    }

    let mut args = json!({
        "externalAuthId": user.user_id,
        "externalOrgId": org_id,
        "spaceRef": space_ref,
        "bindingRef": binding_ref,
        "respondTo": request.respond_to,
        "expectedUpdatedAt": request.expected_updated_at,
    });
    if request.respond_to == "allowlist" {
        args["respondToSubjects"] = json!(request.respond_to_subjects);
    }
    match convex_mutation_message(
        state,
        "spaceAgents:setSpaceAgentWakePolicyForGateway",
        args,
    )
    .await
    {
        Ok(result) => (
            StatusCode::OK,
            Json(json!({"data": {
                "binding_ref": result.get("bindingRef").and_then(Value::as_str).unwrap_or(binding_ref),
                "respond_to": result.get("respondTo").and_then(Value::as_str).unwrap_or(&request.respond_to),
                "respond_to_subjects": result.get("respondToSubjects").cloned().unwrap_or_else(|| json!([])),
                "projection_version": result.get("projectionVersion").cloned().unwrap_or(Value::Null),
                "updated_at": result.get("updatedAt").cloned().unwrap_or(Value::Null),
                "changed": result.get("changed").and_then(Value::as_bool).unwrap_or(false),
            }})),
        ),
        Err(message) => map_policy_failure(message.as_deref()),
    }
}

/// Only `version` may be set on the pin route.
const PIN_ALLOWED_FIELDS: [&str; 1] = ["version"];

/// The validated pin: `None` clears it, `Some` pins one published version.
#[derive(Debug, PartialEq)]
struct PinRequest {
    version: Option<u64>,
}

fn parse_pin(body: &Value) -> Result<PinRequest, Reply> {
    let Some(object) = body.as_object() else {
        return Err(invalid_policy("body_not_an_object"));
    };
    if let Some(field) = object
        .keys()
        .find(|key| !PIN_ALLOWED_FIELDS.contains(&key.as_str()))
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(crate::envelope::error_with_details(
                "unsupported_policy_field",
                "Only version can be set here.",
                json!({"field": field.chars().take(64).collect::<String>()}),
            )),
        ));
    }
    let version = match object.get("version") {
        None => return Err(invalid_policy("version_required")),
        Some(Value::Null) => None,
        Some(value) => {
            let whole = value
                .as_u64()
                .or_else(|| {
                    value
                        .as_f64()
                        .filter(|f| f.fract() == 0.0 && *f >= 1.0)
                        .map(|f| f as u64)
                })
                .filter(|version| *version >= 1);
            Some(whole.ok_or_else(|| invalid_policy("bad_version"))?)
        }
    };
    Ok(PinRequest { version })
}

/// Maps Application's stable pin messages to the typed browser errors.
fn map_pin_failure(message: Option<&str>) -> Reply {
    let Some(message) = message else {
        return unavailable();
    };
    if message.contains("agent_version_not_found") {
        return (
            StatusCode::NOT_FOUND,
            Json(error(
                "agent_version_not_found",
                "That version of the agent is not published.",
            )),
        );
    }
    if message.contains("agent_binding_not_found") {
        return (
            StatusCode::NOT_FOUND,
            Json(error(
                "agent_binding_not_found",
                "That agent is not bound to this room.",
            )),
        );
    }
    if message.contains("invalid_policy") {
        return invalid_policy("rejected");
    }
    unavailable()
}

/// Axum handler for `PUT /api/v1/spaces/{space_ref}/agents/{binding_ref}/pin`.
async fn set_space_agent_pin_route(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((space_ref, binding_ref)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Reply {
    set_space_agent_pin(&state, &user, &space_ref, &binding_ref, &body).await
}

/// The pin route's body, shared with the `spaces.agent_binding.pin` action so the flag, the
/// owner-or-manager gate and the typed errors exist once.
pub(crate) async fn set_space_agent_pin(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
    binding_ref: &str,
    body: &Value,
) -> Reply {
    // The action reaches this function without the route, so the switch is checked here too.
    if !state.space_features.agent_version_pinning {
        return (
            StatusCode::NOT_FOUND,
            Json(error(
                "feature_disabled",
                "Agent version pinning is not enabled.",
            )),
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
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "binding_ref_required",
                "A binding reference is required.",
            )),
        );
    }
    let request = match parse_pin(body) {
        Ok(request) => request,
        Err(reply) => return reply,
    };

    // Role first, and against THIS room, from Control's live membership: owner or manager only.
    match super::resolve_space_role(state, user, &org_id, space_ref).await {
        Ok(role) if super::CREATE_AGENT_ROLES.contains(&role.as_str()) => {}
        Ok(_) => {
            return (
                StatusCode::FORBIDDEN,
                Json(error(
                    "forbidden",
                    "Only a Space owner or manager can pin an agent to a version.",
                )),
            )
        }
        Err(refusal) => return refusal,
    }

    let mut args = json!({
        "externalAuthId": user.user_id,
        "externalOrgId": org_id,
        "spaceRef": space_ref,
        "bindingRef": binding_ref,
    });
    // Absent, never null: Convex's optional number refuses null, and an absent version clears the pin.
    if let Some(version) = request.version {
        args["pinnedVersion"] = json!(version);
    }
    match convex_mutation_message(state, "spaceAgents:setSpaceAgentBindingPinForGateway", args)
        .await
    {
        Ok(result) => (
            StatusCode::OK,
            Json(json!({"data": {
                "binding_ref": result.get("bindingRef").and_then(Value::as_str).unwrap_or(binding_ref),
                "pinned_version": result.get("pinnedVersion").cloned().unwrap_or(Value::Null),
                "projection_version": result.get("projectionVersion").cloned().unwrap_or(Value::Null),
                "changed": result.get("changed").and_then(Value::as_bool).unwrap_or(false),
            }})),
        ),
        Err(message) => map_pin_failure(message.as_deref()),
    }
}

/// The pin route, registered only when `AGENT_VERSION_PINNING` is on. With the switch off the path
/// is not in the route table at all, so it answers the same 404 as any unknown path.
pub(super) fn routes(agent_version_pinning: bool) -> Router<AppState> {
    let router = Router::new();
    if agent_version_pinning {
        router.route(
            "/api/v1/spaces/{space_ref}/agents/{binding_ref}/pin",
            put(set_space_agent_pin_route),
        )
    } else {
        router
    }
}

#[cfg(test)]
mod tests {
    //! WS08-S5: the request parser as a table, then the PATCH route through the real router with
    //! mock Auth, Control and Application (Convex).
    use super::{map_policy_failure, parse_policy};
    use crate::config::AppState;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const POLICY: &str = "/api/v1/spaces/space-room/agents/sab_1/policy";

    fn refusal(body: Value) -> (u16, String, Value) {
        let Err((status, axum::Json(payload))) = parse_policy(&body) else {
            panic!("expected a refusal for {body}");
        };
        (
            status.as_u16(),
            payload["error"]["code"].as_str().unwrap_or_default().to_owned(),
            payload["error"]["details"].clone(),
        )
    }

    #[test]
    fn parse_policy_accepts_the_three_modes_and_normalises_the_allowlist() {
        let members = parse_policy(&json!({"respond_to": "members", "expected_updated_at": 5})).unwrap();
        assert_eq!(members.respond_to, "members");
        assert!(members.respond_to_subjects.is_empty());
        assert_eq!(members.expected_updated_at, 5);

        let managers = parse_policy(&json!({"respond_to": " managers ", "respond_to_subjects": null, "expected_updated_at": 5.0})).unwrap();
        assert_eq!(managers.respond_to, "managers");
        assert_eq!(managers.expected_updated_at, 5);

        let allowlist = parse_policy(&json!({
            "respond_to": "allowlist",
            "respond_to_subjects": [" user-1 ", "user-2", "user-1"],
            "expected_updated_at": 1_760_000_000_000_u64
        }))
        .unwrap();
        assert_eq!(allowlist.respond_to_subjects, vec!["user-1".to_owned(), "user-2".to_owned()]);
    }

    #[test]
    fn parse_policy_refuses_what_it_should_with_typed_codes() {
        // An unsupported field is its own code and echoes at most 64 characters of the name.
        let (status, code, details) = refusal(json!({"respond_to": "members", "expected_updated_at": 1, "approval_mode": "auto"}));
        assert_eq!((status, code.as_str()), (400, "unsupported_policy_field"));
        assert_eq!(details["field"], "approval_mode");
        let long = "f".repeat(200);
        let (_, _, details) = refusal(json!({"respond_to": "members", "expected_updated_at": 1, long.clone(): 1}));
        assert_eq!(details["field"].as_str().unwrap().len(), 64);

        let cases: Vec<(Value, &str)> = vec![
            (json!([1]), "body_not_an_object"),
            (json!("members"), "body_not_an_object"),
            (json!({"expected_updated_at": 1}), "respond_to_required"),
            (json!({"respond_to": 5, "expected_updated_at": 1}), "respond_to_required"),
            (json!({"respond_to": "everyone", "expected_updated_at": 1}), "unknown_respond_to"),
            // `group` is retired: it is not a respond_to value.
            (json!({"respond_to": "group", "expected_updated_at": 1}), "unknown_respond_to"),
            (json!({"respond_to": "", "expected_updated_at": 1}), "unknown_respond_to"),
            (json!({"respond_to": "allowlist", "expected_updated_at": 1}), "allowlist_empty"),
            (json!({"respond_to": "allowlist", "respond_to_subjects": [], "expected_updated_at": 1}), "allowlist_empty"),
            (json!({"respond_to": "allowlist", "respond_to_subjects": ["  "], "expected_updated_at": 1}), "bad_subject_id"),
            (json!({"respond_to": "allowlist", "respond_to_subjects": [7], "expected_updated_at": 1}), "bad_subject_id"),
            (json!({"respond_to": "allowlist", "respond_to_subjects": "user-1", "expected_updated_at": 1}), "bad_subject_list"),
            (json!({"respond_to": "managers", "respond_to_subjects": ["user-1"], "expected_updated_at": 1}), "subjects_need_allowlist"),
            (json!({"respond_to": "members", "respond_to_subjects": ["user-1"], "expected_updated_at": 1}), "subjects_need_allowlist"),
            (json!({"respond_to": "members"}), "expected_updated_at_required"),
            (json!({"respond_to": "members", "expected_updated_at": "5"}), "expected_updated_at_required"),
            (json!({"respond_to": "members", "expected_updated_at": -1}), "expected_updated_at_required"),
            (json!({"respond_to": "members", "expected_updated_at": 1.5}), "expected_updated_at_required"),
        ];
        for (body, reason) in cases {
            let (status, code, details) = refusal(body.clone());
            assert_eq!((status, code.as_str()), (400, "invalid_policy"), "{body}");
            assert_eq!(details["reason"], reason, "{body}");
        }
    }

    #[test]
    fn parse_policy_caps_the_allowlist_at_50_distinct_ids_of_128_characters() {
        let ids = |count: usize| (0..count).map(|i| format!("user-{i}")).collect::<Vec<_>>();
        let fifty = parse_policy(&json!({"respond_to": "allowlist", "respond_to_subjects": ids(50), "expected_updated_at": 1})).unwrap();
        assert_eq!(fifty.respond_to_subjects.len(), 50);
        let (_, _, details) = refusal(json!({"respond_to": "allowlist", "respond_to_subjects": ids(51), "expected_updated_at": 1}));
        assert_eq!(details["reason"], "too_many_subjects");
        // duplicates collapse before the count
        let mut with_dupes = ids(50);
        with_dupes.push("user-0".to_owned());
        assert!(parse_policy(&json!({"respond_to": "allowlist", "respond_to_subjects": with_dupes, "expected_updated_at": 1})).is_ok());
        let long = "x".repeat(129);
        let (_, _, details) = refusal(json!({"respond_to": "allowlist", "respond_to_subjects": [long], "expected_updated_at": 1}));
        assert_eq!(details["reason"], "bad_subject_id");
        let ok = "x".repeat(128);
        assert!(parse_policy(&json!({"respond_to": "allowlist", "respond_to_subjects": [ok], "expected_updated_at": 1})).is_ok());
    }

    #[test]
    fn map_policy_failure_maps_stable_messages_and_hides_everything_else() {
        let reply = |message: Option<&str>| {
            let (status, axum::Json(body)) = map_policy_failure(message);
            (status.as_u16(), body)
        };
        let (status, body) = reply(Some("Uncaught Error: policy_conflict at spaceAgents.ts:812"));
        assert_eq!((status, body["error"]["code"].as_str().unwrap()), (409, "policy_conflict"));
        assert!(!body.to_string().contains("spaceAgents.ts"));
        let (status, body) = reply(Some("agent_binding_not_found"));
        assert_eq!((status, body["error"]["code"].as_str().unwrap()), (404, "agent_binding_not_found"));
        let (status, body) = reply(Some("invalid_policy"));
        assert_eq!((status, body["error"]["code"].as_str().unwrap()), (400, "invalid_policy"));
        for message in [None, Some("Server Error: secret-internal-host:3210"), Some("Unauthorized")] {
            let (status, body) = reply(message);
            assert_eq!((status, body["error"]["code"].as_str().unwrap()), (503, "agent_binding_unavailable"));
            assert!(!body.to_string().contains("secret-internal-host"));
        }
    }

    struct Fx {
        state: AppState,
        application: MockServer,
        _auth: MockServer,
        _user_core: MockServer,
    }

    async fn fx(role: &str, flag: bool, mutation: ResponseTemplate) -> Fx {
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
        state.space_features.agent_wake_policy_api = flag;
        Fx { state, application, _auth: auth, _user_core: user_core }
    }

    fn ok_result() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"value": {
            "bindingRef": "sab_1", "subjectId": "agent-1", "respondTo": "allowlist",
            "respondToSubjects": ["user-2"], "projectionVersion": 5, "updatedAt": 1_760_000_000_001_u64,
            "changed": true
        }}))
    }

    fn convex_error(message: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"status": "error", "errorMessage": message}))
    }

    async fn patch(fx: &Fx, body: Value) -> (u16, Value) {
        let response = crate::build_router(fx.state.clone())
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(POLICY)
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

    fn body() -> Value {
        json!({"respond_to": "allowlist", "respond_to_subjects": ["user-2"], "expected_updated_at": 1_760_000_000_000_u64})
    }

    #[tokio::test]
    async fn flag_off_is_404_feature_disabled_and_makes_no_convex_call() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx("owner", false, ok_result()).await;
        let (status, payload) = patch(&fx, body()).await;
        assert_eq!(status, 404);
        assert_eq!(payload["error"]["code"], "feature_disabled");
        assert!(convex_calls(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn an_editor_or_viewer_gets_403_space_role_cannot_manage_agent_with_no_convex_call() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for role in ["editor", "viewer"] {
            let fx = fx(role, true, ok_result()).await;
            let (status, payload) = patch(&fx, body()).await;
            assert_eq!(status, 403, "{role}");
            assert_eq!(payload["error"]["code"], "space_role_cannot_manage_agent");
            assert!(convex_calls(&fx).await.is_empty(), "{role}");
        }
    }

    #[tokio::test]
    async fn an_invalid_body_is_refused_before_convex_is_asked() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx("owner", true, ok_result()).await;
        let (status, payload) = patch(&fx, json!({"respond_to": "group", "expected_updated_at": 1})).await;
        assert_eq!(status, 400);
        assert_eq!(payload["error"]["code"], "invalid_policy");
        let (status, payload) = patch(&fx, json!({"respond_to": "members", "expected_updated_at": 1, "org_id": "forged"})).await;
        assert_eq!(status, 400);
        assert_eq!(payload["error"]["code"], "unsupported_policy_field");
        assert!(convex_calls(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn an_owner_or_manager_sets_the_policy_with_the_session_identity() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for role in ["owner", "manager"] {
            let fx = fx(role, true, ok_result()).await;
            let (status, payload) = patch(&fx, body()).await;
            assert_eq!(status, 200, "{role}: {payload}");
            let data = &payload["data"];
            assert_eq!(data["binding_ref"], "sab_1");
            assert_eq!(data["respond_to"], "allowlist");
            assert_eq!(data["respond_to_subjects"], json!(["user-2"]));
            assert_eq!(data["projection_version"], 5);
            assert_eq!(data["changed"], true);
            let calls = convex_calls(&fx).await;
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0]["path"], "spaceAgents:setSpaceAgentWakePolicyForGateway");
            let args = &calls[0]["args"];
            assert_eq!(args["externalAuthId"], "user-1");
            assert_eq!(args["externalOrgId"], "org-1");
            assert_eq!(args["spaceRef"], "space-room");
            assert_eq!(args["bindingRef"], "sab_1");
            assert_eq!(args["respondTo"], "allowlist");
            assert_eq!(args["respondToSubjects"], json!(["user-2"]));
            assert_eq!(args["expectedUpdatedAt"], 1_760_000_000_000_u64);
        }
    }

    #[tokio::test]
    async fn a_non_allowlist_policy_sends_no_subject_list() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx("owner", true, ok_result()).await;
        let (status, _) = patch(&fx, json!({"respond_to": "managers", "expected_updated_at": 7})).await;
        assert_eq!(status, 200);
        let calls = convex_calls(&fx).await;
        assert!(calls[0]["args"].get("respondToSubjects").is_none());
    }

    #[tokio::test]
    async fn convex_failures_map_to_typed_errors_and_never_leak_the_raw_text() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let cases: Vec<(ResponseTemplate, u16, &str)> = vec![
            (convex_error("Uncaught Error: policy_conflict"), 409, "policy_conflict"),
            (convex_error("Uncaught Error: agent_binding_not_found"), 404, "agent_binding_not_found"),
            (convex_error("Server Error at secret-internal-host:3210 stack trace"), 503, "agent_binding_unavailable"),
            (ResponseTemplate::new(500).set_body_string("secret-internal-host exploded"), 503, "agent_binding_unavailable"),
        ];
        for (template, expected_status, expected_code) in cases {
            let fx = fx("owner", true, template).await;
            let (status, payload) = patch(&fx, body()).await;
            assert_eq!(status, expected_status, "{expected_code}");
            assert_eq!(payload["error"]["code"], expected_code);
            assert!(!payload.to_string().contains("secret-internal-host"));
            assert!(!payload.to_string().contains("stack trace"));
        }
    }
}

#[cfg(test)]
#[path = "agent_policy_pin_tests.rs"]
mod pin_tests;
