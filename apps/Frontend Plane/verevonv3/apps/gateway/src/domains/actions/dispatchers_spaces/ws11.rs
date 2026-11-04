//! WS11-S3 and WS11-S7: action dispatchers for "Mark all read" (`spaces.mark_all_read`), "Mark
//! unread" (`spaces.mark_unread`), the room level (`spaces.set_notify_level`), the attention inbox
//! (`spaces.set_attention_item_state`) and the thread follow control (`spaces.set_thread_follow`).
//! Each rides the same function as its route (`POST /api/v1/spaces/attention/read-all`, `POST
//! /api/v1/spaces/{space_ref}/unread`, `PUT /api/v1/spaces/{space_ref}/notify`, `POST
//! /api/v1/spaces/attention/items/{item_id}/state` and `PUT
//! /api/v1/spaces/{space_ref}/threads/{thread_id}/follow` in `domains/spaces/attention.rs`, so the
//! flag, the Zero Data Retention refusal and the typed errors exist once. Neither id is
//! model-executable: an inbox and a follow are a person's own attention.
//!
//! WS11-S11 adds `spaces.create_reminder` and `spaces.cancel_reminder` (`POST
//! /api/v1/spaces/{space_ref}/reminders`, `DELETE /api/v1/spaces/reminders/{id}`) through
//! `create_reminder_core` / `cancel_reminder_core` in `domains/spaces/reminders.rs`: creating one is
//! the typed 412 under Zero Data Retention, cancelling is always allowed.
//!
//! WS11-S10 adds `spaces.set_status` (the org-scoped status, `PUT /api/v1/spaces/status`) through
//! `put_status_core` in `domains/spaces/status.rs`: a text under Zero Data Retention is the typed 412.

use axum::{http::HeaderMap, response::Response};
use serde_json::{Map, Value};

use crate::config::AppState;
use crate::domains::spaces::attention::{
    mark_unread_core, put_follow_core, put_notify_core, read_all_core, set_item_state_core,
};
use crate::domains::spaces::reminders::{cancel_reminder_core, create_reminder_core};
use crate::domains::spaces::status::put_status_core;
use crate::middleware::AuthenticatedUser;

use super::super::dispatchers::{
    owner_json_response_to_envelope, ticket_bad_request, ticket_string,
};

pub(in crate::domains::actions) async fn dispatch_spaces_set_attention_item_state(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let item_id = ticket_string(input, "itemId");
    if item_id.is_empty() {
        return ticket_bad_request("spaces.set_attention_item_state requires a non-empty 'itemId'");
    }
    let mut body = Map::new();
    if let Some(value) = input.get("state") {
        body.insert("state".to_owned(), value.clone());
    }
    if let Some(value) = input.get("snoozeUntil") {
        body.insert("snooze_until".to_owned(), value.clone());
    }
    let (status, axum::Json(resp)) =
        set_item_state_core(state, user, item_id, &Value::Object(body)).await;
    owner_json_response_to_envelope("spaces.set_attention_item_state", user, status, resp)
}

pub(in crate::domains::actions) async fn dispatch_spaces_set_thread_follow(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    let thread_id = ticket_string(input, "threadId");
    if space_ref.is_empty() || thread_id.is_empty() {
        return ticket_bad_request(
            "spaces.set_thread_follow requires a non-empty 'spaceRef' and 'threadId'",
        );
    }
    let mut body = Map::new();
    if let Some(value) = input.get("state") {
        body.insert("state".to_owned(), value.clone());
    }
    let (status, axum::Json(resp)) = put_follow_core(
        state,
        user,
        Some(headers),
        space_ref,
        thread_id,
        &Value::Object(body),
    )
    .await;
    owner_json_response_to_envelope("spaces.set_thread_follow", user, status, resp)
}

pub(in crate::domains::actions) async fn dispatch_spaces_mark_all_read(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let mut body = Map::new();
    if let Some(value) = input.get("spaceRefs") {
        body.insert("space_refs".to_owned(), value.clone());
    }
    if let Some(value) = input.get("kinds") {
        body.insert("kinds".to_owned(), value.clone());
    }
    let (status, axum::Json(resp)) = read_all_core(state, user, &Value::Object(body)).await;
    owner_json_response_to_envelope("spaces.mark_all_read", user, status, resp)
}

pub(in crate::domains::actions) async fn dispatch_spaces_mark_unread(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    if space_ref.is_empty() {
        return ticket_bad_request("spaces.mark_unread requires a non-empty 'spaceRef'");
    }
    let mut body = Map::new();
    if let Some(value) = input.get("beforeMs") {
        body.insert("before_ms".to_owned(), value.clone());
    }
    let (status, axum::Json(resp)) =
        mark_unread_core(state, user, space_ref, &Value::Object(body)).await;
    owner_json_response_to_envelope("spaces.mark_unread", user, status, resp)
}

pub(in crate::domains::actions) async fn dispatch_spaces_set_notify_level(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    if space_ref.is_empty() {
        return ticket_bad_request("spaces.set_notify_level requires a non-empty 'spaceRef'");
    }
    let mut body = Map::new();
    if let Some(value) = input.get("level") {
        body.insert("level".to_owned(), value.clone());
    }
    if let Some(value) = input.get("hidePreview") {
        body.insert("hide_preview".to_owned(), value.clone());
    }
    let (status, axum::Json(resp)) =
        put_notify_core(state, user, space_ref, &Value::Object(body)).await;
    owner_json_response_to_envelope("spaces.set_notify_level", user, status, resp)
}

pub(in crate::domains::actions) async fn dispatch_spaces_set_status(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let mut body = Map::new();
    if let Some(value) = input.get("state") {
        body.insert("state".to_owned(), value.clone());
    }
    if let Some(value) = input.get("text") {
        body.insert("text".to_owned(), value.clone());
    }
    if let Some(value) = input.get("expiresAt") {
        body.insert("expires_at".to_owned(), value.clone());
    }
    let (status, axum::Json(resp)) =
        put_status_core(state, user, Some(headers), &Value::Object(body)).await;
    owner_json_response_to_envelope("spaces.set_status", user, status, resp)
}

pub(in crate::domains::actions) async fn dispatch_spaces_create_reminder(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    let thread_id = ticket_string(input, "threadId");
    if space_ref.is_empty() || thread_id.is_empty() {
        return ticket_bad_request(
            "spaces.create_reminder requires a non-empty 'spaceRef' and 'threadId'",
        );
    }
    let mut body = Map::new();
    body.insert("thread_id".to_owned(), Value::String(thread_id.to_owned()));
    if let Some(value) = input.get("messageId") {
        body.insert("message_id".to_owned(), value.clone());
    }
    if let Some(value) = input.get("remindAt") {
        body.insert("remind_at".to_owned(), value.clone());
    }
    let (status, axum::Json(resp)) = create_reminder_core(
        state,
        user,
        Some(headers),
        space_ref,
        &Value::Object(body),
    )
    .await;
    owner_json_response_to_envelope("spaces.create_reminder", user, status, resp)
}

pub(in crate::domains::actions) async fn dispatch_spaces_cancel_reminder(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let reminder_id = ticket_string(input, "reminderId");
    if reminder_id.is_empty() {
        return ticket_bad_request("spaces.cancel_reminder requires a non-empty 'reminderId'");
    }
    let (status, axum::Json(resp)) = cancel_reminder_core(state, user, reminder_id).await;
    owner_json_response_to_envelope("spaces.cancel_reminder", user, status, resp)
}

#[cfg(test)]
mod tests {
    //! WS11-S3.gw: the three attention action dispatchers map their input onto the route bodies.
    use super::*;
    use axum::http::StatusCode;
    use http_body_util::BodyExt;
    use serde_json::json;
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
            authorized_membership: Some(crate::middleware::AuthorizedMembership {
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

    /// Control lists `room-a` and `room-b` and says the caller is a viewer of `room-a`;
    /// Application answers every mutation with `{}`-shaped values. The attention flag is `flag`.
    async fn stack(flag: bool) -> (AppState, MockServer, MockServer) {
        let control = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/internal/spaces"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"spaces": [
                {"space_ref": "room-a", "kind": "room"}, {"space_ref": "room-b", "kind": "room"}
            ]}})))
            .mount(&control)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/internal/spaces/room-a/membership"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"space_ref": "room-a", "kind": "room", "role": "viewer"}
            })))
            .mount(&control)
            .await;
        let application = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/mutation"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": {
                "roomsMarked": 2, "itemsResolved": 1, "markedAt": 9,
                "lastReadAt": 5, "manualUnread": true, "level": "off", "hidePreview": true
            }})))
            .mount(&application)
            .await;
        let mut state = crate::tests::test_state(false);
        state.user_core_url = control.uri();
        state.application_convex_url = application.uri();
        state.application_convex_service_key = "application-test-key".into();
        state.space_features.attention_api = flag;
        (state, control, application)
    }

    async fn mutations(application: &MockServer) -> Vec<Value> {
        application
            .received_requests()
            .await
            .unwrap()
            .iter()
            .map(|request| serde_json::from_slice(&request.body).unwrap())
            .collect()
    }

    #[tokio::test]
    async fn dispatch_spaces_mark_all_read_maps_space_refs_and_kinds_to_the_route_body() {
        let (state, _control, application) = stack(true).await;
        let input = json!({"spaceRefs": ["room-a", "zz"], "kinds": ["mention", "reply"], "orgId": "forged", "externalAuthId": "forged"});
        let (status, body) = read(dispatch_spaces_mark_all_read(&state, &user(), &input).await).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["data"]["actionId"], "spaces.mark_all_read");
        assert_eq!(body["data"]["status"], "completed");
        assert_eq!(body["data"]["result"], json!({"rooms_marked": 2, "items_resolved": 1, "marked_at": 9}));
        let sent = mutations(&application).await;
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["path"], "spaceAttention:markAllReadForGateway");
        assert_eq!(sent[0]["args"]["spaceRefs"], json!(["room-a"]), "intersected with Control's index");
        assert_eq!(sent[0]["args"]["kinds"], json!(["mention", "reply"]));
        assert_eq!(sent[0]["args"]["externalAuthId"], "user-1", "the input never names the user");

        // no input at all is every room, default kinds
        let (status, _) = read(dispatch_spaces_mark_all_read(&state, &user(), &json!({})).await).await;
        assert_eq!(status, StatusCode::OK);
        let sent = mutations(&application).await;
        assert_eq!(sent[1]["args"]["spaceRefs"], json!(["room-a", "room-b"]));
        assert!(sent[1]["args"].get("kinds").is_none());
        // a kind outside mention|reply is the route's 400
        let (status, body) = read(dispatch_spaces_mark_all_read(&state, &user(), &json!({"kinds": ["invite"]})).await).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_kind");
    }

    #[tokio::test]
    async fn dispatch_spaces_mark_unread_requires_space_ref_and_maps_before_ms() {
        let (state, control, application) = stack(true).await;
        for input in [json!({"beforeMs": 5}), json!({"spaceRef": "  ", "beforeMs": 5})] {
            let (status, body) = read(dispatch_spaces_mark_unread(&state, &user(), &input).await).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{input}");
            assert_eq!(body["error"]["code"], "invalid_input");
        }
        assert!(control.received_requests().await.unwrap().is_empty());
        let input = json!({"spaceRef": "room-a", "beforeMs": 1_700_000_000_000_i64});
        let (status, body) = read(dispatch_spaces_mark_unread(&state, &user(), &input).await).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["data"]["actionId"], "spaces.mark_unread");
        assert_eq!(body["data"]["result"], json!({"last_read_at": 5, "manual_unread": true}));
        let sent = mutations(&application).await;
        assert_eq!(sent[0]["path"], "spaceReadMarkers:markUnreadForGateway");
        assert_eq!(sent[0]["args"]["spaceRef"], "room-a");
        assert_eq!(sent[0]["args"]["beforeMs"], 1_700_000_000_000_i64);
        // a missing or non-positive time is the route's 400
        let (status, body) = read(dispatch_spaces_mark_unread(&state, &user(), &json!({"spaceRef": "room-a", "beforeMs": 0})).await).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_before_ms");
        let (status, body) = read(dispatch_spaces_mark_unread(&state, &user(), &json!({"spaceRef": "room-a"})).await).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_request");
    }

    #[tokio::test]
    async fn dispatch_spaces_set_notify_level_requires_space_ref_and_maps_level_and_hide_preview() {
        let (state, control, application) = stack(true).await;
        let (status, body) = read(dispatch_spaces_set_notify_level(&state, &user(), &json!({"level": "off"})).await).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_input");
        assert!(control.received_requests().await.unwrap().is_empty());
        let input = json!({"spaceRef": "room-a", "level": "off", "hidePreview": true});
        let (status, body) = read(dispatch_spaces_set_notify_level(&state, &user(), &input).await).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["data"]["actionId"], "spaces.set_notify_level");
        assert_eq!(body["data"]["result"], json!({"level": "off", "hide_preview": true}));
        let sent = mutations(&application).await;
        assert_eq!(sent[0]["path"], "spaceAttention:setRoomLevelForGateway");
        assert_eq!(sent[0]["args"]["spaceRef"], "room-a");
        assert_eq!(sent[0]["args"]["level"], "off");
        assert_eq!(sent[0]["args"]["hidePreview"], true);
        // a level outside the closed set, and an input with neither field, are the route's 400s
        let (status, body) = read(dispatch_spaces_set_notify_level(&state, &user(), &json!({"spaceRef": "room-a", "level": "loud"})).await).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_level");
        let (status, body) = read(dispatch_spaces_set_notify_level(&state, &user(), &json!({"spaceRef": "room-a"})).await).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_request");
    }

    #[tokio::test]
    async fn the_attention_dispatchers_answer_404_with_the_flag_off_and_call_nothing() {
        let (state, control, application) = stack(false).await;
        let input = json!({"spaceRef": "room-a", "beforeMs": 5, "level": "all"});
        for response in [
            dispatch_spaces_mark_all_read(&state, &user(), &json!({})).await,
            dispatch_spaces_mark_unread(&state, &user(), &input).await,
            dispatch_spaces_set_notify_level(&state, &user(), &input).await,
        ] {
            let (status, body) = read(response).await;
            assert_eq!(status, StatusCode::NOT_FOUND);
            assert_eq!(body["error"]["code"], "space_attention_disabled");
        }
        assert!(control.received_requests().await.unwrap().is_empty());
        assert!(application.received_requests().await.unwrap().is_empty());
    }
}
