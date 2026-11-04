//! WS12-S3: action dispatchers for direct messages (`spaces.open_dm`, `spaces.hide_space`). Both
//! ride the same functions as `POST /api/v1/spaces/dm` and `POST`/`DELETE
//! /api/v1/spaces/{space_ref}/hide` in `domains/spaces/dm.rs`, so the flag, the Zero Data
//! Retention refusal, Control's pre-check and the typed errors exist once. Neither id is
//! model-executable: opening a DM names people and hiding one is a person's own sidebar state.

use axum::{http::HeaderMap, response::Response};
use serde_json::{json, Value};

use crate::config::AppState;
use crate::domains::spaces::dm::{open_dm_core, set_hidden};
use crate::domains::spaces::message_lifecycle::{revise_message, set_reaction, Revision};
use crate::middleware::AuthenticatedUser;

use super::super::dispatchers::{
    owner_json_response_to_envelope, ticket_bad_request, ticket_string,
};

pub(in crate::domains::actions) async fn dispatch_spaces_open_dm(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let Some(participants) = input.get("participants").filter(|value| value.is_array()) else {
        return ticket_bad_request("spaces.open_dm requires a 'participants' array");
    };
    let client_request_id = ticket_string(input, "clientRequestId");
    if client_request_id.is_empty() {
        return ticket_bad_request("spaces.open_dm requires a non-empty 'clientRequestId'");
    }
    // The open flow validates the shape and refuses anything else.
    let body = json!({
        "participants": participants,
        "client_request_id": client_request_id,
    });
    let bytes = serde_json::to_vec(&body).unwrap_or_default();
    let (status, axum::Json(resp)) = open_dm_core(state, user, headers, &bytes).await;
    owner_json_response_to_envelope("spaces.open_dm", user, status, resp)
}

pub(in crate::domains::actions) async fn dispatch_spaces_hide_space(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    if space_ref.is_empty() {
        return ticket_bad_request("spaces.hide_space requires a non-empty 'spaceRef'");
    }
    // `hidden` defaults to true: the action hides; false shows the DM again.
    let hidden = input.get("hidden").and_then(Value::as_bool).unwrap_or(true);
    let (status, axum::Json(resp)) = set_hidden(state, user, space_ref, hidden).await;
    owner_json_response_to_envelope("spaces.hide_space", user, status, resp)
}

/// WS12-S7: `spaces.edit_message` rides the same flow as `PATCH
/// /api/v1/spaces/{space_ref}/threads/{thread_id}/messages/{message_id}`.
pub(in crate::domains::actions) async fn dispatch_spaces_edit_message(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let (space_ref, thread_id, message_id, key) = (
        ticket_string(input, "spaceRef"),
        ticket_string(input, "threadId"),
        ticket_string(input, "messageId"),
        ticket_string(input, "idempotencyKey"),
    );
    if space_ref.is_empty() || thread_id.is_empty() || message_id.is_empty() {
        return ticket_bad_request(
            "spaces.edit_message requires non-empty 'spaceRef', 'threadId' and 'messageId'",
        );
    }
    let content = input
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let (status, axum::Json(resp)) = revise_message(
        state,
        user,
        headers,
        space_ref,
        thread_id,
        message_id,
        key,
        Revision::Edit {
            content: content.to_owned(),
        },
        Some(input),
    )
    .await;
    owner_json_response_to_envelope("spaces.edit_message", user, status, resp)
}

/// WS12-S7: `spaces.delete_message` rides the same flow as `DELETE
/// /api/v1/spaces/{space_ref}/threads/{thread_id}/messages/{message_id}`.
pub(in crate::domains::actions) async fn dispatch_spaces_delete_message(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let (space_ref, thread_id, message_id, key) = (
        ticket_string(input, "spaceRef"),
        ticket_string(input, "threadId"),
        ticket_string(input, "messageId"),
        ticket_string(input, "idempotencyKey"),
    );
    if space_ref.is_empty() || thread_id.is_empty() || message_id.is_empty() {
        return ticket_bad_request(
            "spaces.delete_message requires non-empty 'spaceRef', 'threadId' and 'messageId'",
        );
    }
    let (status, axum::Json(resp)) = revise_message(
        state,
        user,
        headers,
        space_ref,
        thread_id,
        message_id,
        key,
        Revision::Delete {
            moderation_reason: ticket_string(input, "moderationReason").to_owned(),
        },
        Some(input),
    )
    .await;
    owner_json_response_to_envelope("spaces.delete_message", user, status, resp)
}

/// WS12-S8: `spaces.toggle_reaction` rides the same flow as `PUT`/`DELETE
/// /api/v1/spaces/{space_ref}/threads/{thread_id}/messages/{message_id}/reactions/{emoji}`.
/// `present` defaults to true (add); false removes the caller's reaction.
pub(in crate::domains::actions) async fn dispatch_spaces_toggle_reaction(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let (space_ref, thread_id, message_id, emoji) = (
        ticket_string(input, "spaceRef"),
        ticket_string(input, "threadId"),
        ticket_string(input, "messageId"),
        ticket_string(input, "emoji"),
    );
    if space_ref.is_empty() || thread_id.is_empty() || message_id.is_empty() || emoji.is_empty() {
        return ticket_bad_request(
            "spaces.toggle_reaction requires non-empty 'spaceRef', 'threadId', 'messageId' and 'emoji'",
        );
    }
    let present = input.get("present").and_then(Value::as_bool).unwrap_or(true);
    let (status, axum::Json(resp)) = set_reaction(
        state, user, headers, space_ref, thread_id, message_id, emoji, present,
    )
    .await;
    owner_json_response_to_envelope("spaces.toggle_reaction", user, status, resp)
}

#[cfg(test)]
mod dm_tests {
    use super::*;
    use crate::middleware::AuthorizedMembership;
    use axum::http::StatusCode;
    use http_body_util::BodyExt;
    use wiremock::matchers::{body_partial_json, method as wm_method, path as wm_path};
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
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    struct Fx {
        state: AppState,
        control: MockServer,
        app: MockServer,
        _org: MockServer,
    }

    async fn fixture(flag: bool, zdr: bool) -> Fx {
        let org = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/organizations/org-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"data": {"metadata": {"interactiveRetention": {"zdr": zdr}}}}),
            ))
            .mount(&org)
            .await;
        let control = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/internal/spaces/dm-eligibility"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {
                "eligible": true, "participant_hash": "hash-1", "participant_ids": ["user-1", "user-2"]
            }})))
            .mount(&control)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/internal/spaces"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"spaces": [
                {"space_ref": "space-dm-1", "kind": "dm", "role": "member"}
            ]}})))
            .mount(&control)
            .await;
        let app = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/mutation"))
            .and(body_partial_json(json!({"path": "spaces:openDmForGateway"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status": "success", "value": {
                "created": true,
                "space": {"spaceRef": "space-dm-1", "name": "Ann", "kind": "dm",
                          "lifecycle": "pending_registration",
                          "participantExternalAuthIds": ["user-1", "user-2"]}
            }})))
            .mount(&app)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/mutation"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"status": "success", "value": {"hiddenAt": 1_700_000_000_000_u64}}),
            ))
            .mount(&app)
            .await;
        let mut state = crate::tests::test_state(false);
        state.space_features.dm_api = flag;
        state.user_core_url = control.uri();
        state.org_core_url = org.uri();
        state.application_convex_url = app.uri();
        state.application_convex_service_key = "application-test-key".into();
        Fx { state, control, app, _org: org }
    }

    #[tokio::test]
    async fn open_dm_validates_input_and_reaches_the_same_flow() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(true, false).await;
        for input in [
            json!({"clientRequestId": "r"}),
            json!({"participants": "user-2", "clientRequestId": "r"}),
            json!({"participants": ["user-2"]}),
            json!({"participants": ["user-2"], "clientRequestId": ""}),
        ] {
            let (status, body) = read(dispatch_spaces_open_dm(&fx.state, &user(), &HeaderMap::new(), &input).await).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{input}");
            assert_eq!(body["error"]["code"], "invalid_input");
        }
        assert!(fx.app.received_requests().await.unwrap().is_empty());
        let input = json!({"participants": ["user-2"], "clientRequestId": "r"});
        let (status, body) = read(dispatch_spaces_open_dm(&fx.state, &user(), &HeaderMap::new(), &input).await).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["actionId"], "spaces.open_dm");
        assert_eq!(body["data"]["result"]["space"]["space_ref"], "space-dm-1");
        assert_eq!(body["data"]["result"]["created"], true);
    }

    #[tokio::test]
    async fn open_dm_flag_off_is_404_and_zdr_is_412_with_no_upstream_call() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let input = json!({"participants": ["user-2"], "clientRequestId": "r"});
        let off = fixture(false, false).await;
        let (status, body) = read(dispatch_spaces_open_dm(&off.state, &user(), &HeaderMap::new(), &input).await).await;
        assert_eq!((status, body["error"]["code"].as_str().unwrap()), (StatusCode::NOT_FOUND, "dm_disabled"));
        let zdr = fixture(true, true).await;
        let (status, body) = read(dispatch_spaces_open_dm(&zdr.state, &user(), &HeaderMap::new(), &input).await).await;
        assert_eq!(status, StatusCode::PRECONDITION_FAILED);
        assert_eq!(body["error"]["code"], "zdr_durable_mutation_forbidden");
        for fx in [&off, &zdr] {
            assert!(fx.control.received_requests().await.unwrap().is_empty());
            assert!(fx.app.received_requests().await.unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn hide_space_validates_input_defaults_to_hide_and_reaches_the_same_flow() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(true, false).await;
        let (status, body) = read(dispatch_spaces_hide_space(&fx.state, &user(), &json!({"spaceRef": " "})).await).await;
        assert_eq!((status, body["error"]["code"].as_str().unwrap()), (StatusCode::BAD_REQUEST, "invalid_input"));
        let (status, body) = read(dispatch_spaces_hide_space(&fx.state, &user(), &json!({"spaceRef": "space-dm-1"})).await).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["actionId"], "spaces.hide_space");
        assert_eq!(body["data"]["result"]["hidden"], true);
        let (_, body) = read(dispatch_spaces_hide_space(&fx.state, &user(), &json!({"spaceRef": "space-dm-1", "hidden": false})).await).await;
        assert_eq!(body["data"]["result"]["hidden"], false);
        let paths: Vec<String> = fx
            .app
            .received_requests()
            .await
            .unwrap()
            .iter()
            .map(|r| serde_json::from_slice::<Value>(&r.body).unwrap()["path"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(paths, ["spaceDmHides:hideForGateway", "spaceDmHides:unhideForGateway"]);
        let off = fixture(false, false).await;
        let (status, body) = read(dispatch_spaces_hide_space(&off.state, &user(), &json!({"spaceRef": "space-dm-1"})).await).await;
        assert_eq!((status, body["error"]["code"].as_str().unwrap()), (StatusCode::NOT_FOUND, "dm_disabled"));
    }
}
