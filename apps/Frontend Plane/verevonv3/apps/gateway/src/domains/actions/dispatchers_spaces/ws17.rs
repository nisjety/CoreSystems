//! WS17-S5: the four human-only personal-agent actions. Each rides the same core function as its REST
//! route in `domains/spaces/personal_agents.rs`, so the rollout flag, the membership check, Control's
//! decisions and the typed errors exist once. None is model-executable (`model-access.ts` closes them):
//! a model creating, re-scoping or moving a person's agent would be an authority inversion.
//!
//! WS17-S6: `spaces.open_private_panel` rides the open core of `domains/spaces/private_panel.rs` the same way.
//!
//! WS17-S7: `memory.declassify_to_space` rides the core of `domains/spaces/memory_declassify.rs` the same way. Its
//! input carries the route's three body fields (`memoryId`, `reviewedContent`, `clientRequestId`) and `spaceRef`.
//!
//! WS17-S7b: `memory.retract_from_space` rides `retract_memory_core` the same way (`memoryId`, `clientRequestId`, `spaceRef`).

use axum::{http::HeaderMap, response::Response, Json};
use serde_json::{json, Value};

use crate::config::AppState;
use crate::domains::spaces::personal_agents::{
    create_personal_agent_core, put_agent_access_core, set_primary_agent_core, withdraw_agent_core,
};
use crate::domains::spaces::memory_declassify::{declassify_memory_core, retract_memory_core};
use crate::domains::spaces::private_panel::open_private_panel_core;
use crate::middleware::AuthenticatedUser;

use super::super::dispatchers::{
    owner_json_response_to_envelope, ticket_bad_request, ticket_string,
};

pub(in crate::domains::actions) async fn dispatch_agents_create_personal(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let mut body = json!({
        "name": ticket_string(input, "name"),
        "client_request_id": ticket_string(input, "clientRequestId"),
    });
    if !ticket_string(input, "instructions").is_empty() {
        body["instructions"] = json!(ticket_string(input, "instructions"));
    }
    if !ticket_string(input, "avatarColor").is_empty() {
        body["avatar_color"] = json!(ticket_string(input, "avatarColor"));
    }
    let (status, Json(payload)) = create_personal_agent_core(state, user, &body).await;
    owner_json_response_to_envelope("agents.create_personal", user, status, payload)
}

pub(in crate::domains::actions) async fn dispatch_agents_update_access(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let agent_ref = ticket_string(input, "agentRef");
    if agent_ref.is_empty() {
        return ticket_bad_request("agents.update_access requires a non-empty 'agentRef'");
    }
    let entries: Vec<Value> = input
        .get("entries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|entry| {
            json!({
                "principal_id": ticket_string(entry, "principalId"),
                "role": ticket_string(entry, "role"),
            })
        })
        .collect();
    let body = json!({
        "visibility": ticket_string(input, "visibility"),
        "add_policy": ticket_string(input, "addPolicy"),
        "entries": entries,
        "expected_revision": input.get("expectedRevision").cloned().unwrap_or(Value::Null),
    });
    let (status, Json(payload)) = put_agent_access_core(state, user, agent_ref, &body).await;
    owner_json_response_to_envelope("agents.update_access", user, status, payload)
}

pub(in crate::domains::actions) async fn dispatch_agents_set_primary(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let Some(agent_ref) = input.get("agentRef") else {
        return ticket_bad_request("agents.set_primary requires 'agentRef' (a string or null)");
    };
    let body = json!({ "agent_ref": agent_ref });
    let (status, Json(payload)) = set_primary_agent_core(state, user, &body).await;
    owner_json_response_to_envelope("agents.set_primary", user, status, payload)
}

pub(in crate::domains::actions) async fn dispatch_spaces_withdraw_agent(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    let agent_ref = ticket_string(input, "agentRef");
    if space_ref.is_empty() || agent_ref.is_empty() {
        return ticket_bad_request("spaces.withdraw_agent requires non-empty 'spaceRef' and 'agentRef'");
    }
    let (status, Json(payload)) = withdraw_agent_core(state, user, agent_ref, space_ref).await;
    owner_json_response_to_envelope("spaces.withdraw_agent", user, status, payload)
}

/// `spaces.open_private_panel` (WS17-S6): get or create the caller's private panel pointer for one room. Human-only;
/// the ZDR refusal and the flag are checked by the core, which reads the request headers for the ZDR check.
pub(in crate::domains::actions) async fn dispatch_spaces_open_private_panel(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    if space_ref.is_empty() {
        return ticket_bad_request("spaces.open_private_panel requires a non-empty 'spaceRef'");
    }
    let (status, Json(payload)) = open_private_panel_core(state, user, headers, space_ref).await;
    owner_json_response_to_envelope("spaces.open_private_panel", user, status, payload)
}

/// `memory.declassify_to_space` (WS17-S7): share one of the caller's personal memories into a shared Space room
/// after the caller has reviewed the text. Human-only. Action input keys are camelCase like every sibling arm
/// (`spaceRef`, `memoryId`, `reviewedContent`, `clientRequestId`); the core body keeps the route's snake_case
/// names. The text is read exactly as sent (`ticket_string` trims, so it is not used here); the core applies
/// the route's refusals in the route's order.
pub(in crate::domains::actions) async fn dispatch_memory_declassify_to_space(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let space_ref = input.get("spaceRef").and_then(Value::as_str).unwrap_or_default();
    let body = json!({
        "memory_id": input.get("memoryId").cloned().unwrap_or(Value::Null),
        "reviewed_content": input.get("reviewedContent").cloned().unwrap_or(Value::Null),
        "client_request_id": input.get("clientRequestId").cloned().unwrap_or(Value::Null),
    });
    let (status, Json(payload)) = declassify_memory_core(state, user, headers, space_ref, body).await;
    owner_json_response_to_envelope("memory.declassify_to_space", user, status, payload)
}

/// `memory.retract_from_space` (WS17-S7b): take back a room copy the caller shared (SD-22). Human-only. Input keys are
/// camelCase (`spaceRef`, `memoryId`, `clientRequestId`); the core keeps the route's snake_case body. Zero Data Retention
/// does not refuse a removal, so the core does not check it.
pub(in crate::domains::actions) async fn dispatch_memory_retract_from_space(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let space_ref = input.get("spaceRef").and_then(Value::as_str).unwrap_or_default();
    let body = json!({
        "memory_id": input.get("memoryId").cloned().unwrap_or(Value::Null),
        "client_request_id": input.get("clientRequestId").cloned().unwrap_or(Value::Null),
    });
    let (status, Json(payload)) = retract_memory_core(state, user, headers, space_ref, body).await;
    owner_json_response_to_envelope("memory.retract_from_space", user, status, payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::AuthorizedMembership;
    use axum::http::StatusCode;
    use http_body_util::BodyExt;
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

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

    async fn read(response: Response) -> (StatusCode, Value) {
        let status = response.status();
        let bytes = response.into_body().collect().await.expect("a body").to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    /// `memory.declassify_to_space` reads camelCase inputs and hands the core the route's snake_case body: Control is
    /// asked for this memory, this Space and the exact bytes of the reviewed text (never trimmed).
    #[tokio::test]
    async fn the_declassify_arm_reads_camel_case_inputs_and_asks_control_for_the_exact_text() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/organizations/org-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"metadata": {"interactiveRetention": {"zdr": false}}}
            })))
            .mount(&server)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": [
                {"spaceRef": "space-room", "name": "Room", "kind": "room", "lifecycle": "active"}
            ]})))
            .mount(&server)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/internal/spaces/memory-declassify-decision"))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({"error": "refused"})))
            .mount(&server)
            .await;
        let mut state = crate::tests::test_state(false);
        state.org_core_url = server.uri();
        state.user_core_url = server.uri();
        state.application_convex_url = server.uri();
        state.application_convex_service_key = "application-test-key".into();
        state.space_features.space_memory_declassify_api = true;

        let input = json!({
            "spaceRef": "space-room",
            "memoryId": "mem-1",
            "reviewedContent": "  hello  ",
            "clientRequestId": "req-1"
        });
        let (status, body) =
            read(dispatch_memory_declassify_to_space(&state, &user(), &HeaderMap::new(), &input).await).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(body["error"]["code"], "memory_declassify_not_authorized");

        let received = server.received_requests().await.unwrap_or_default();
        let asked = received
            .into_iter()
            .find(|request| request.url.path().ends_with("memory-declassify-decision"))
            .expect("Control was asked");
        let asked: Value = serde_json::from_slice(&asked.body).expect("a JSON decision request");
        assert_eq!(asked["operation"], "declassify");
        assert_eq!(asked["space_ref"], "space-room");
        assert_eq!(asked["memory_id"], "mem-1");
        assert_eq!(asked["idempotency_key"], "req-1");
        let digest = format!(
            "sha256:{}",
            hex::encode(<sha2::Sha256 as sha2::Digest>::digest(b"  hello  "))
        );
        assert_eq!(asked["content_digest"], digest);

        // The arm reads only the camelCase names: the snake_case spelling reaches the core as null and is refused as a
        // request shape before any Control call.
        let snake = json!({
            "spaceRef": "space-room",
            "memory_id": "mem-1",
            "reviewed_content": "hello",
            "client_request_id": "req-1"
        });
        let (status, body) =
            read(dispatch_memory_declassify_to_space(&state, &user(), &HeaderMap::new(), &snake).await).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["error"]["code"], "invalid_request");
    }
}
