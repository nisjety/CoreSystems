//! WS10-S5b: the per-tool policy editor routes.
//!
//! * `GET /api/v1/spaces/{space_ref}/tool-policy[?agent_subject_id=]`: Control's stored Space
//!   ceiling, every agent layer and (with `agent_subject_id`) the merged view for that agent,
//!   which names the level AND the layer that decided it per tool. Any member who can read the
//!   Space may read it; Control decides.
//! * `PUT /api/v1/spaces/{space_ref}/tool-policy` body `{entries, expected_revision}`: replaces the
//!   Space ceiling (every agent of the room).
//! * `PUT /api/v1/spaces/{space_ref}/agents/{binding_ref}/tool-policy` same body: replaces one
//!   agent's layer. The binding is resolved to its Control subject id from Application's binding
//!   projection (the Space and organization must match and the binding must be active or paused);
//!   Control then refuses an agent that is not an active member of the Space.
//!
//! Control is the store and the only authority: it resolves the caller's live role from the
//! gateway's signed delegation (a manager or owner writes, an archived room is read-only, a
//! personal Space has no policy) and compares `expected_revision` per layer (409
//! `tool_policy_revision_conflict`). The body here carries no actor, organization, role or Space;
//! unknown fields are refused. A policy only tightens (allow < ask < block), so a write needs no
//! entitlement and Zero Data Retention organizations behave the same: keys and levels, never
//! content.
//!
//! After an agent-layer write Control has already committed. The gateway then records the new
//! layer revision and the merged digest on the binding in Application
//! (`spaceAgents:setSpaceAgentBindingPolicyForGateway`), the projection the editor and the legacy
//! filter read. That sync never rolls Control back: a failure answers `projection: "pending"` and
//! the next successful write or read of the merged view heals it. The legacy `approvalMode` and
//! `allowedTools` are deliberately NOT derived from the policy (`blocked` or a stripped `agentic`
//! feature would silence the agent, not tighten one tool).
//!
//! Behind `SpaceFeatureFlags::tool_policy_editor` (off answers 404 `feature_disabled`), a rollout
//! control and never an authorization input.

use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    routing::{get, put},
    Json, Router,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use super::room_admin::convex_mutation_message;
use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::{error, error_with_details, unwrap_data},
    middleware::AuthenticatedUser,
    upstream::proxy_json,
};

type Reply = (StatusCode, Json<Value>);

/// Control's own bounds (`MaxToolPolicyEntries`, `maxApprovalKeyLen`); Control validates fully.
const MAX_ENTRIES: usize = 200;
const MAX_TOOL_KEY_CHARS: usize = 200;
const MAX_SUBJECT_ID_CHARS: usize = 200;
const MAX_RELAYED_MESSAGE_CHARS: usize = 300;
const ALLOWED_FIELDS: [&str; 2] = ["entries", "expected_revision"];

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/spaces/{space_ref}/tool-policy",
            get(get_tool_policy_route).put(put_space_tool_policy_route),
        )
        .route(
            "/api/v1/spaces/{space_ref}/agents/{binding_ref}/tool-policy",
            put(put_agent_tool_policy_route),
        )
}

fn feature_disabled() -> Reply {
    (
        StatusCode::NOT_FOUND,
        Json(error(
            "feature_disabled",
            "The tool policy editor is not enabled.",
        )),
    )
}

fn unavailable() -> Reply {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(error(
            "tool_policy_unavailable",
            "The tool policy is unavailable. Nothing was changed.",
        )),
    )
}

fn invalid(reason: &'static str) -> Reply {
    (
        StatusCode::BAD_REQUEST,
        Json(error_with_details(
            "tool_policy_invalid",
            "The tool policy is not valid.",
            json!({ "reason": reason }),
        )),
    )
}

fn actor_of(user: &AuthenticatedUser) -> ActionActor {
    ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    }
}

fn control_url(state: &AppState, space_ref: &str, tail: &str) -> String {
    format!(
        "{}/api/v1/internal/spaces/{}/{}",
        state.user_core_url,
        urlencoding::encode(space_ref),
        tail
    )
}

/// The organization and trimmed Space ref, or the typed refusal every Space route gives.
async fn scope_of(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
) -> Result<(String, String), Reply> {
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    let space_ref = space_ref.trim().to_owned();
    if org_id.is_empty() || space_ref.is_empty() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(error(
                "active_membership_required",
                "An active organization membership and Space are required.",
            )),
        ));
    }
    Ok((org_id, space_ref))
}

/// A well-formed lowercase snake_case code, as Control's typed errors use.
fn is_error_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 64
        && code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

/// Control's failure as the browser envelope: a typed body keeps its status, code and bounded
/// scalar details; anything else (and a typed 401, which the SPA would read as an expired session)
/// is the 503 that says nothing changed.
fn relay_control_error(status: StatusCode, body: &Value) -> Reply {
    let typed = body
        .get("error")
        .and_then(Value::as_object)
        .and_then(|error| {
            let code = error.get("code")?.as_str().filter(|code| is_error_code(code))?;
            let message: String = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .chars()
                .take(MAX_RELAYED_MESSAGE_CHARS)
                .collect();
            let mut out = Map::new();
            out.insert("code".to_owned(), json!(code));
            out.insert("message".to_owned(), json!(message));
            if let Some(details) = error.get("details").and_then(Value::as_object) {
                let scalars: Map<String, Value> = details
                    .iter()
                    .filter(|(_, value)| !value.is_array() && !value.is_object())
                    .take(16)
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect();
                out.insert("details".to_owned(), Value::Object(scalars));
            }
            Some(Value::Object(out))
        });
    match typed {
        Some(error) if status != StatusCode::UNAUTHORIZED => {
            (status, Json(json!({ "error": error })))
        }
        _ => unavailable(),
    }
}

/// The validated, normalized write: only `entries` and `expected_revision` ever reach Control.
fn parse_update(body: &Value) -> Result<Value, Reply> {
    let Some(object) = body.as_object() else {
        return Err(invalid("body_not_an_object"));
    };
    if let Some(field) = object.keys().find(|key| !ALLOWED_FIELDS.contains(&key.as_str())) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(error_with_details(
                "unsupported_policy_field",
                "Only entries and expected_revision can be set here.",
                json!({"field": field.chars().take(64).collect::<String>()}),
            )),
        ));
    }
    let Some(entries) = object.get("entries").and_then(Value::as_array) else {
        return Err(invalid("entries_required"));
    };
    if entries.len() > MAX_ENTRIES {
        return Err(invalid("too_many_entries"));
    }
    let mut normalized = Vec::with_capacity(entries.len());
    for entry in entries {
        let Some(entry) = entry.as_object() else {
            return Err(invalid("bad_entry"));
        };
        let key = entry.get("tool_key").and_then(Value::as_str);
        let level = entry.get("level").and_then(Value::as_str);
        match (key, level) {
            (Some(key), Some(level @ ("allow" | "ask" | "block")))
                if !key.is_empty() && key.chars().count() <= MAX_TOOL_KEY_CHARS =>
            {
                normalized.push(json!({"tool_key": key, "level": level}));
            }
            _ => return Err(invalid("bad_entry")),
        }
    }
    let expected_revision = object
        .get("expected_revision")
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_f64().filter(|f| *f >= 0.0 && f.fract() == 0.0).map(|f| f as u64))
        })
        .ok_or_else(|| invalid("expected_revision_required"))?;
    Ok(json!({"entries": normalized, "expected_revision": expected_revision}))
}

#[derive(Deserialize)]
pub(super) struct PolicyQuery {
    agent_subject_id: Option<String>,
}

pub(super) async fn get_tool_policy_route(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(space_ref): Path<String>,
    Query(query): Query<PolicyQuery>,
) -> Reply {
    get_tool_policy(&state, &user, &space_ref, query.agent_subject_id.as_deref()).await
}

pub(super) async fn put_space_tool_policy_route(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(space_ref): Path<String>,
    Json(body): Json<Value>,
) -> Reply {
    put_space_tool_policy(&state, &user, &space_ref, &body).await
}

pub(super) async fn put_agent_tool_policy_route(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((space_ref, binding_ref)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Reply {
    put_agent_tool_policy(&state, &user, &space_ref, &binding_ref, &body).await
}

/// Control's read, relayed: `{space_ref, ceiling, agents, merged}`.
pub(crate) async fn get_tool_policy(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
    agent_subject_id: Option<&str>,
) -> Reply {
    if !state.space_features.tool_policy_editor {
        return feature_disabled();
    }
    let (org_id, space_ref) = match scope_of(state, user, space_ref).await {
        Ok(scope) => scope,
        Err(reply) => return reply,
    };
    let mut url = control_url(state, &space_ref, "tool-policy");
    if let Some(agent) = agent_subject_id.map(str::trim).filter(|agent| !agent.is_empty()) {
        if agent.chars().count() > MAX_SUBJECT_ID_CHARS {
            return invalid("agent_subject_id_oversized");
        }
        url.push_str("?agent_subject_id=");
        url.push_str(&urlencoding::encode(agent));
    }
    let (status, Json(control)) = proxy_json(
        state,
        Method::GET,
        &url,
        None,
        Some(&org_id),
        Some(&actor_of(user)),
        None,
    )
    .await;
    if !status.is_success() {
        return relay_control_error(status, &control);
    }
    (StatusCode::OK, Json(json!({ "data": unwrap_data(&control) })))
}

async fn put_control_layer(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    url: &str,
    payload: Value,
) -> Result<Value, Reply> {
    let (status, Json(control)) = proxy_json(
        state,
        Method::PUT,
        url,
        Some(payload),
        Some(org_id),
        Some(&actor_of(user)),
        None,
    )
    .await;
    if !status.is_success() {
        return Err(relay_control_error(status, &control));
    }
    Ok(unwrap_data(&control).clone())
}

/// Replaces the Space ceiling on Control. Application keeps no ceiling, so nothing is projected.
pub(crate) async fn put_space_tool_policy(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
    body: &Value,
) -> Reply {
    if !state.space_features.tool_policy_editor {
        return feature_disabled();
    }
    let (org_id, space_ref) = match scope_of(state, user, space_ref).await {
        Ok(scope) => scope,
        Err(reply) => return reply,
    };
    let payload = match parse_update(body) {
        Ok(payload) => payload,
        Err(reply) => return reply,
    };
    let url = control_url(state, &space_ref, "tool-policy");
    match put_control_layer(state, user, &org_id, &url, payload).await {
        Ok(data) => (StatusCode::OK, Json(json!({ "data": data }))),
        Err(reply) => reply,
    }
}

/// The agent's Control subject id for a binding of THIS Space and organization that is still
/// bound (active or paused), from Application's binding projection.
async fn binding_subject_id(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
    binding_ref: &str,
) -> Result<String, Reply> {
    let value = super::convex_gateway_call(
        state,
        "query",
        "spaceAgents:spaceAgentBindingsForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "spaceRef": space_ref,
        }),
    )
    .await
    .map_err(|()| unavailable())?;
    value
        .as_array()
        .into_iter()
        .flatten()
        .find(|binding| {
            binding.get("bindingRef").and_then(Value::as_str) == Some(binding_ref)
                && matches!(
                    binding.get("status").and_then(Value::as_str),
                    Some("active" | "paused")
                )
        })
        .and_then(|binding| binding.get("subjectId").and_then(Value::as_str))
        .filter(|subject| !subject.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(error(
                    "agent_binding_not_found",
                    "That agent is not bound to this room.",
                )),
            )
        })
}

/// Replaces one agent's layer on Control, then projects the new revision and the merged digest
/// onto the binding. The Control write stands whatever the projection does.
pub(crate) async fn put_agent_tool_policy(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
    binding_ref: &str,
    body: &Value,
) -> Reply {
    if !state.space_features.tool_policy_editor {
        return feature_disabled();
    }
    let (org_id, space_ref) = match scope_of(state, user, space_ref).await {
        Ok(scope) => scope,
        Err(reply) => return reply,
    };
    let binding_ref = binding_ref.trim();
    if binding_ref.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "binding_ref_required",
                "A binding reference is required.",
            )),
        );
    }
    let payload = match parse_update(body) {
        Ok(payload) => payload,
        Err(reply) => return reply,
    };
    let subject_id = match binding_subject_id(state, user, &org_id, &space_ref, binding_ref).await {
        Ok(subject) => subject,
        Err(reply) => return reply,
    };
    let url = control_url(
        state,
        &space_ref,
        &format!("agents/{}/tool-policy", urlencoding::encode(&subject_id)),
    );
    let data = match put_control_layer(state, user, &org_id, &url, payload).await {
        Ok(data) => data,
        Err(reply) => return reply,
    };
    let revision = data
        .pointer("/policy/revision")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let digest = data
        .pointer("/merged/digest")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let projection = if digest.is_empty() {
        "pending"
    } else {
        match convex_mutation_message(
            state,
            "spaceAgents:setSpaceAgentBindingPolicyForGateway",
            json!({
                "externalAuthId": user.user_id,
                "externalOrgId": org_id,
                "spaceRef": space_ref,
                "bindingRef": binding_ref,
                "toolPolicyRevision": revision,
                "toolPolicyDigest": digest,
            }),
        )
        .await
        {
            Ok(_) => "synced",
            Err(_) => "pending",
        }
    };
    let mut out = data;
    if let Some(object) = out.as_object_mut() {
        object.insert("binding_ref".to_owned(), json!(binding_ref));
        object.insert("projection".to_owned(), json!(projection));
    }
    (StatusCode::OK, Json(json!({ "data": out })))
}

#[cfg(test)]
mod tests {
    //! WS10-S5b tests: the three tool policy routes through the real router, with wiremock for
    //! auth-core, Control (user-core) and Application (Convex). No model is called.
    use super::*;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    use wiremock::matchers::{body_partial_json, method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const CEILING: &str = "/api/v1/spaces/room-a/tool-policy";
    const AGENT: &str = "/api/v1/spaces/room-a/agents/binding-1/tool-policy";
    const CONTROL_CEILING: &str = "/api/v1/internal/spaces/room-a/tool-policy";
    const BINDINGS: &str = "spaceAgents:spaceAgentBindingsForGateway";
    const PROJECT: &str = "spaceAgents:setSpaceAgentBindingPolicyForGateway";

    struct Fx {
        state: AppState,
        _auth: MockServer,
        user_core: MockServer,
        application: MockServer,
    }

    /// A signed-in member of `org-1` through the real router. The tool policy flag is on; Control
    /// answers only what a test mounts.
    async fn fx() -> Fx {
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
        let application = MockServer::start().await;
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.application_convex_url = application.uri();
        state.application_convex_service_key = "application-test-key".into();
        state.space_features.tool_policy_editor = true;
        Fx {
            state,
            _auth: auth,
            user_core,
            application,
        }
    }

    async fn mount_control(fx: &Fx, verb: &str, route: &str, template: ResponseTemplate) {
        Mock::given(wm_method(verb))
            .and(wm_path(route))
            .respond_with(template)
            .mount(&fx.user_core)
            .await;
    }

    async fn mount_convex(fx: &Fx, route: &str, function: &str, template: ResponseTemplate) {
        Mock::given(wm_method("POST"))
            .and(wm_path(route))
            .and(body_partial_json(json!({"path": function})))
            .respond_with(template)
            .mount(&fx.application)
            .await;
    }

    fn bindings() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"status": "success", "value": [
            {"bindingRef": "binding-1", "status": "active", "subjectId": "agent-9"},
            {"bindingRef": "binding-2", "status": "revoked", "subjectId": "agent-8"},
            {"bindingRef": "binding-3", "status": "paused", "subjectId": "agent/7"},
            {"bindingRef": "binding-4", "status": "active", "subjectId": ""},
            {"bindingRef": "binding-5", "status": "pending", "subjectId": "agent-5"}
        ]}))
    }

    fn convex_ok() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"status": "success", "value": null}))
    }

    async fn call(fx: &Fx, method: &str, uri: &str, body: Option<Value>) -> (u16, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header("cookie", "better-auth.session_token=session-1");
        if body.is_some() {
            request = request.header("content-type", "application/json");
        }
        let response = crate::build_router(fx.state.clone())
            .oneshot(
                request
                    .body(match &body {
                        Some(value) => Body::from(value.to_string()),
                        None => Body::empty(),
                    })
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

    /// The tool policy requests Control received, as (method, path, query, headers' org and user, body).
    async fn control_requests(fx: &Fx) -> Vec<(String, String, String, Value)> {
        fx.user_core
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|request| request.url.path().contains("tool-policy"))
            .map(|request| {
                (
                    request.method.to_string(),
                    request.url.path().to_owned(),
                    request.url.query().unwrap_or_default().to_owned(),
                    serde_json::from_slice(&request.body).unwrap_or(Value::Null),
                )
            })
            .collect()
    }

    async fn convex_calls(fx: &Fx, function: &str) -> Vec<Value> {
        fx.application
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter_map(|request| serde_json::from_slice::<Value>(&request.body).ok())
            .filter(|body| body["path"] == function)
            .collect()
    }

    fn write_body() -> Value {
        json!({
            "entries": [
                {"tool_key": "mcp__crm__delete_contact", "level": "block"},
                {"tool_key": "web_search", "level": "ask"}
            ],
            "expected_revision": 3
        })
    }

    fn control_policy(revision: u64, digest: Option<&str>) -> ResponseTemplate {
        let mut data = json!({"policy": {"revision": revision, "entries": []}});
        if let Some(digest) = digest {
            data["merged"] = json!({"digest": digest});
        }
        ResponseTemplate::new(200).set_body_json(json!({"data": data}))
    }

    // ---- the flag --------------------------------------------------------------------------------

    #[tokio::test]
    async fn flag_off_gives_404_feature_disabled_on_all_three_routes_and_calls_nothing() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut fx = fx().await;
        fx.state.space_features.tool_policy_editor = false;
        for (method, uri, body) in [
            ("GET", CEILING, None),
            ("PUT", CEILING, Some(write_body())),
            ("PUT", AGENT, Some(write_body())),
        ] {
            let (status, got) = call(&fx, method, uri, body).await;
            assert_eq!(status, 404, "{method} {uri}");
            assert_eq!(got["error"]["code"], "feature_disabled", "{method} {uri}");
        }
        assert!(control_requests(&fx).await.is_empty());
        assert!(fx.application.received_requests().await.unwrap().is_empty());
    }

    // ---- the body --------------------------------------------------------------------------------

    #[tokio::test]
    async fn a_bad_write_body_is_a_400_and_no_control_call_is_made() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx().await;
        mount_control(&fx, "PUT", CONTROL_CEILING, control_policy(4, None)).await;
        mount_convex(&fx, "/api/query", BINDINGS, bindings()).await;
        let entry = |key: &str, level: &str| json!({"tool_key": key, "level": level});
        let too_many: Vec<Value> = (0..201).map(|i| entry(&format!("tool_{i}"), "ask")).collect();
        let long_key = "k".repeat(201);
        for (body, code, reason, field) in [
            (json!({"entries": [], "expected_revision": 0, "org_id": "forged"}), "unsupported_policy_field", None, Some("org_id")),
            (json!({"entries": [], "expected_revision": 0, "role": "owner"}), "unsupported_policy_field", None, Some("role")),
            (json!({"entries": [], "expected_revision": 0, "space_ref": "other"}), "unsupported_policy_field", None, Some("space_ref")),
            (json!({"entries": []}), "tool_policy_invalid", Some("expected_revision_required"), None),
            (json!({"entries": [], "expected_revision": -1}), "tool_policy_invalid", Some("expected_revision_required"), None),
            (json!({"entries": [], "expected_revision": 1.5}), "tool_policy_invalid", Some("expected_revision_required"), None),
            (json!({"entries": [], "expected_revision": "3"}), "tool_policy_invalid", Some("expected_revision_required"), None),
            (json!({"expected_revision": 0}), "tool_policy_invalid", Some("entries_required"), None),
            (json!({"entries": "all", "expected_revision": 0}), "tool_policy_invalid", Some("entries_required"), None),
            (json!({"entries": [entry("web_search", "maybe")], "expected_revision": 0}), "tool_policy_invalid", Some("bad_entry"), None),
            (json!({"entries": [entry("web_search", "")], "expected_revision": 0}), "tool_policy_invalid", Some("bad_entry"), None),
            (json!({"entries": [entry("", "allow")], "expected_revision": 0}), "tool_policy_invalid", Some("bad_entry"), None),
            (json!({"entries": [entry(&long_key, "allow")], "expected_revision": 0}), "tool_policy_invalid", Some("bad_entry"), None),
            (json!({"entries": ["web_search"], "expected_revision": 0}), "tool_policy_invalid", Some("bad_entry"), None),
            (json!({"entries": [{"tool_key": "web_search"}], "expected_revision": 0}), "tool_policy_invalid", Some("bad_entry"), None),
            (json!({"entries": too_many, "expected_revision": 0}), "tool_policy_invalid", Some("too_many_entries"), None),
            (json!(["entries"]), "tool_policy_invalid", Some("body_not_an_object"), None),
        ] {
            for uri in [CEILING, AGENT] {
                let (status, got) = call(&fx, "PUT", uri, Some(body.clone())).await;
                assert_eq!(status, 400, "{uri} {body}");
                assert_eq!(got["error"]["code"], code, "{uri} {body}");
                if let Some(reason) = reason {
                    assert_eq!(got["error"]["details"]["reason"], reason, "{uri} {body}");
                }
                if let Some(field) = field {
                    assert_eq!(got["error"]["details"]["field"], field, "{uri} {body}");
                }
            }
        }
        assert!(control_requests(&fx).await.is_empty(), "a refused body never reached Control");
        assert!(
            fx.application.received_requests().await.unwrap().is_empty(),
            "a refused body never looked a binding up"
        );
    }

    #[tokio::test]
    async fn the_ceiling_write_sends_control_only_entries_and_the_revision_under_the_session_identity() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx().await;
        mount_control(&fx, "PUT", CONTROL_CEILING, control_policy(4, None)).await;
        // The boundary: exactly 200 entries and a whole-number revision written as a float are fine.
        let two_hundred: Vec<Value> =
            (0..200).map(|i| json!({"tool_key": format!("tool_{i}"), "level": "allow"})).collect();
        let (status, got) = call(
            &fx,
            "PUT",
            CEILING,
            Some(json!({"entries": two_hundred, "expected_revision": 3.0})),
        )
        .await;
        assert_eq!(status, 200, "{got}");
        let (status, got) = call(&fx, "PUT", CEILING, Some(write_body())).await;
        assert_eq!(status, 200, "{got}");
        assert_eq!(got["data"]["policy"]["revision"], 4);
        let sent = control_requests(&fx).await;
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].1, CONTROL_CEILING);
        assert_eq!(sent[0].3["entries"].as_array().unwrap().len(), 200);
        assert_eq!(sent[0].3["expected_revision"], 3);
        assert_eq!(
            sent[1].3,
            json!({
                "entries": [
                    {"tool_key": "mcp__crm__delete_contact", "level": "block"},
                    {"tool_key": "web_search", "level": "ask"}
                ],
                "expected_revision": 3
            }),
            "only entries and expected_revision ever reach Control"
        );
        // Application keeps no ceiling, so nothing is looked up or projected.
        assert!(fx.application.received_requests().await.unwrap().is_empty());
        // The caller and organization are the session's: Control decides the role from them.
        let requests = fx.user_core.received_requests().await.unwrap();
        let put = requests
            .iter()
            .find(|request| request.method.as_str() == "PUT")
            .expect("a PUT to Control");
        assert_eq!(put.headers.get("x-user-id").unwrap(), "user-1");
        assert_eq!(put.headers.get("x-org-id").unwrap(), "org-1");
    }

    // ---- the agent layer -------------------------------------------------------------------------

    #[tokio::test]
    async fn the_agent_layer_resolves_its_binding_to_the_subject_id_and_refuses_one_that_is_not_bound() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx().await;
        mount_convex(&fx, "/api/query", BINDINGS, bindings()).await;
        mount_convex(&fx, "/api/mutation", PROJECT, convex_ok()).await;
        mount_control(&fx, "PUT", "/api/v1/internal/spaces/room-a/agents/agent-9/tool-policy", control_policy(5, Some("digest-1"))).await;
        mount_control(&fx, "PUT", "/api/v1/internal/spaces/room-a/agents/agent%2F7/tool-policy", control_policy(6, Some("digest-2"))).await;

        let (status, got) = call(&fx, "PUT", AGENT, Some(write_body())).await;
        assert_eq!(status, 200, "{got}");
        assert_eq!(got["data"]["binding_ref"], "binding-1");
        // A paused binding is still bound; its subject id is one encoded path segment.
        let (status, got) = call(
            &fx,
            "PUT",
            "/api/v1/spaces/room-a/agents/binding-3/tool-policy",
            Some(write_body()),
        )
        .await;
        assert_eq!(status, 200, "{got}");
        assert_eq!(got["data"]["binding_ref"], "binding-3");

        let lookups = convex_calls(&fx, BINDINGS).await;
        assert_eq!(lookups[0]["args"]["externalAuthId"], "user-1");
        assert_eq!(lookups[0]["args"]["externalOrgId"], "org-1");
        assert_eq!(lookups[0]["args"]["spaceRef"], "room-a");

        let writes_before = control_requests(&fx).await.len();
        // Revoked, still pending, unknown, subject-less and other-Space bindings (which the
        // Space-scoped projection does not return) are each a 404 and write nothing.
        for binding in ["binding-2", "binding-4", "binding-5", "binding-unknown", "binding-of-another-space"] {
            let (status, got) = call(
                &fx,
                "PUT",
                &format!("/api/v1/spaces/room-a/agents/{binding}/tool-policy"),
                Some(write_body()),
            )
            .await;
            assert_eq!(status, 404, "{binding}");
            assert_eq!(got["error"]["code"], "agent_binding_not_found", "{binding}");
        }
        assert_eq!(control_requests(&fx).await.len(), writes_before, "no refused binding reached Control");
    }

    #[tokio::test]
    async fn a_binding_lookup_failure_is_a_503_and_writes_nothing() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx().await;
        mount_convex(&fx, "/api/query", BINDINGS, ResponseTemplate::new(500)).await;
        mount_control(&fx, "PUT", "/api/v1/internal/spaces/room-a/agents/agent-9/tool-policy", control_policy(5, Some("d"))).await;
        let (status, got) = call(&fx, "PUT", AGENT, Some(write_body())).await;
        assert_eq!(status, 503, "{got}");
        assert_eq!(got["error"]["code"], "tool_policy_unavailable");
        assert!(control_requests(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn the_agent_write_projects_the_revision_and_digest_and_a_failed_sync_still_answers_200_pending() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let agent_route = "/api/v1/internal/spaces/room-a/agents/agent-9/tool-policy";
        // Synced: Control committed, the binding now carries the revision and the merged digest.
        let fx_ok = fx().await;
        mount_convex(&fx_ok, "/api/query", BINDINGS, bindings()).await;
        mount_convex(&fx_ok, "/api/mutation", PROJECT, convex_ok()).await;
        mount_control(&fx_ok, "PUT", agent_route, control_policy(5, Some("digest-1"))).await;
        let (status, got) = call(&fx_ok, "PUT", AGENT, Some(write_body())).await;
        assert_eq!(status, 200, "{got}");
        assert_eq!(got["data"]["projection"], "synced");
        assert_eq!(got["data"]["binding_ref"], "binding-1");
        assert_eq!(got["data"]["policy"]["revision"], 5);
        let projected = convex_calls(&fx_ok, PROJECT).await;
        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0]["args"]["toolPolicyRevision"], 5);
        assert_eq!(projected[0]["args"]["toolPolicyDigest"], "digest-1");
        assert_eq!(projected[0]["args"]["bindingRef"], "binding-1");
        assert_eq!(projected[0]["args"]["spaceRef"], "room-a");
        assert_eq!(projected[0]["args"]["externalOrgId"], "org-1");
        assert_eq!(projected[0]["args"]["externalAuthId"], "user-1");

        // The sync failing never rolls Control back: still a 200, with the projection pending.
        let fx_sync = fx().await;
        mount_convex(&fx_sync, "/api/query", BINDINGS, bindings()).await;
        mount_convex(
            &fx_sync,
            "/api/mutation",
            PROJECT,
            ResponseTemplate::new(200).set_body_json(json!({"status": "error", "errorMessage": "binding_not_found"})),
        )
        .await;
        mount_control(&fx_sync, "PUT", agent_route, control_policy(5, Some("digest-1"))).await;
        let (status, got) = call(&fx_sync, "PUT", AGENT, Some(write_body())).await;
        assert_eq!(status, 200, "{got}");
        assert_eq!(got["data"]["projection"], "pending");
        assert_eq!(got["data"]["policy"]["revision"], 5, "the Control answer stands");

        // No merged digest to project: pending, and Application is not asked to write.
        let fx_nodigest = fx().await;
        mount_convex(&fx_nodigest, "/api/query", BINDINGS, bindings()).await;
        mount_convex(&fx_nodigest, "/api/mutation", PROJECT, convex_ok()).await;
        mount_control(&fx_nodigest, "PUT", agent_route, control_policy(5, None)).await;
        let (status, got) = call(&fx_nodigest, "PUT", AGENT, Some(write_body())).await;
        assert_eq!(status, 200, "{got}");
        assert_eq!(got["data"]["projection"], "pending");
        assert!(convex_calls(&fx_nodigest, PROJECT).await.is_empty());
    }

    // ---- Control's own refusals ------------------------------------------------------------------

    #[tokio::test]
    async fn controls_typed_errors_are_relayed_with_status_and_code_and_untyped_json_is_a_503() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let typed = |status: u16, body: Value| ResponseTemplate::new(status).set_body_json(body);
        let long_message = "m".repeat(400);
        for (template, want_status, want_code) in [
            (
                typed(403, json!({"error": {"code": "tool_policy_role_required", "message": "A manager or owner writes the policy."}})),
                403,
                "tool_policy_role_required",
            ),
            (
                typed(409, json!({"error": {"code": "tool_policy_revision_conflict", "message": "Changed meanwhile.", "details": {"current_revision": 7}}})),
                409,
                "tool_policy_revision_conflict",
            ),
            (typed(404, json!({"error": {"code": "room_not_found", "message": "No such room."}})), 404, "room_not_found"),
            // A typed 401 would read as an expired session in the SPA, so it is not relayed.
            (typed(401, json!({"error": {"code": "unauthorized", "message": "no"}})), 503, "tool_policy_unavailable"),
            // A bare failure is typed `upstream_error` by proxy_json before this relay sees it, and
            // passes with its status and none of the upstream's own words.
            (ResponseTemplate::new(500).set_body_string("<html>boom</html>"), 500, "upstream_error"),
            // Untyped JSON and codes that are not snake_case say nothing of the upstream.
            (typed(500, json!({"error": "plain string"})), 503, "tool_policy_unavailable"),
            (typed(400, json!({"error": {"code": "Bad-Code", "message": "x"}})), 503, "tool_policy_unavailable"),
        ] {
            let fx = fx().await;
            mount_control(&fx, "PUT", CONTROL_CEILING, template).await;
            let (status, got) = call(&fx, "PUT", CEILING, Some(write_body())).await;
            assert_eq!(status, want_status, "{want_code}: {got}");
            assert_eq!(got["error"]["code"], want_code, "{got}");
        }

        // Details keep scalars only, and the message is bounded.
        let bounded = self::fx().await;
        mount_control(
            &bounded,
            "PUT",
            CONTROL_CEILING,
            typed(
                409,
                json!({"error": {
                    "code": "tool_policy_revision_conflict",
                    "message": long_message,
                    "details": {"current_revision": 7, "layer": "ceiling", "nested": {"a": 1}, "list": [1, 2]}
                }}),
            ),
        )
        .await;
        let (status, got) = call(&bounded, "PUT", CEILING, Some(write_body())).await;
        assert_eq!(status, 409);
        assert_eq!(got["error"]["message"].as_str().unwrap().chars().count(), 300);
        assert_eq!(got["error"]["details"], json!({"current_revision": 7, "layer": "ceiling"}));

        // The same relay serves the read and the agent write.
        let read = self::fx().await;
        mount_control(&read, "GET", CONTROL_CEILING, typed(403, json!({"error": {"code": "space_read_not_authorized", "message": "no"}}))).await;
        let (status, got) = call(&read, "GET", CEILING, None).await;
        assert_eq!(status, 403);
        assert_eq!(got["error"]["code"], "space_read_not_authorized");
        let agent = self::fx().await;
        mount_convex(&agent, "/api/query", BINDINGS, bindings()).await;
        mount_convex(&agent, "/api/mutation", PROJECT, convex_ok()).await;
        mount_control(
            &agent,
            "PUT",
            "/api/v1/internal/spaces/room-a/agents/agent-9/tool-policy",
            typed(409, json!({"error": {"code": "tool_policy_revision_conflict", "message": "Changed meanwhile.", "details": {"current_revision": 9}}})),
        )
        .await;
        let (status, got) = call(&agent, "PUT", AGENT, Some(write_body())).await;
        assert_eq!(status, 409);
        assert_eq!(got["error"]["details"]["current_revision"], 9);
        assert!(convex_calls(&agent, PROJECT).await.is_empty(), "a refused write is never projected");
    }

    // ---- the read --------------------------------------------------------------------------------

    #[tokio::test]
    async fn the_read_relays_controls_view_and_forwards_the_agent_subject_id_encoded() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx().await;
        let view = json!({"space_ref": "room-a", "ceiling": {"revision": 2, "entries": []}, "agents": [], "merged": null});
        mount_control(&fx, "GET", CONTROL_CEILING, ResponseTemplate::new(200).set_body_json(json!({"data": view.clone()}))).await;
        let (status, got) = call(&fx, "GET", CEILING, None).await;
        assert_eq!(status, 200, "{got}");
        assert_eq!(got["data"], view);
        // A blank agent_subject_id is not forwarded; a real one is, as one encoded query value.
        let (status, _) = call(&fx, "GET", &format!("{CEILING}?agent_subject_id=%20%20"), None).await;
        assert_eq!(status, 200);
        let (status, _) = call(&fx, "GET", &format!("{CEILING}?agent_subject_id=agent%209%26x"), None).await;
        assert_eq!(status, 200);
        let sent = control_requests(&fx).await;
        assert_eq!(sent[0].2, "");
        assert_eq!(sent[1].2, "", "a blank id is dropped");
        assert_eq!(sent[2].2, "agent_subject_id=agent%209%26x");
        assert!(sent.iter().all(|request| request.0 == "GET" && request.1 == CONTROL_CEILING));

        // An oversized id is refused before Control is asked.
        let long = "a".repeat(201);
        let (status, got) = call(&fx, "GET", &format!("{CEILING}?agent_subject_id={long}"), None).await;
        assert_eq!(status, 400);
        assert_eq!(got["error"]["code"], "tool_policy_invalid");
        assert_eq!(got["error"]["details"]["reason"], "agent_subject_id_oversized");
        assert_eq!(control_requests(&fx).await.len(), 3);
    }
}
