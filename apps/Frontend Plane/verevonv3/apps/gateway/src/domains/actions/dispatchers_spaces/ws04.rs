//! WS04-S6: action dispatchers for the room member writer (`spaces.change_member_role`,
//! `spaces.leave_room`). Both ride the same gateway functions as the member routes in
//! `domains/spaces/room_admin.rs`, so the flag, the Control delegation and the typed error
//! pass-through exist once. Neither id is model-executable. The caller-supplied idempotency key
//! is forwarded unchanged so a retried action replays instead of re-applying.

use axum::response::Response;
use serde_json::{json, Map, Value};

use crate::domains::spaces::room_admin::{
    change_member_role, feature_disabled, join_room, remove_member, update_room_details,
    update_room_settings,
};
use crate::middleware::AuthenticatedUser;
use crate::config::AppState;

use super::super::dispatchers::{
    owner_json_response_to_envelope, ticket_bad_request, ticket_string,
};

pub(in crate::domains::actions) async fn dispatch_spaces_change_member_role(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
    idempotency_key: &str,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    let member_id = ticket_string(input, "memberId");
    let role = ticket_string(input, "role");
    if space_ref.is_empty() || member_id.is_empty() || role.is_empty() {
        return ticket_bad_request(
            "spaces.change_member_role requires non-empty 'spaceRef', 'memberId' and 'role'",
        );
    }
    let mut body = Map::new();
    body.insert("role".to_owned(), json!(role));
    if let Some(revision) = input.get("expectedRevision").and_then(Value::as_i64) {
        body.insert("expected_revision".to_owned(), json!(revision));
    }
    if !idempotency_key.trim().is_empty() {
        body.insert("idempotency_key".to_owned(), json!(idempotency_key.trim()));
    }
    let (status, axum::Json(resp)) =
        change_member_role(state, user, space_ref, member_id, &Value::Object(body)).await;
    owner_json_response_to_envelope("spaces.change_member_role", user, status, resp)
}

pub(in crate::domains::actions) async fn dispatch_spaces_leave_room(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    if space_ref.is_empty() {
        return ticket_bad_request("spaces.leave_room requires a non-empty 'spaceRef'");
    }
    // Leaving has no legacy path: with the writer off the action is refused rather than
    // mapped onto the legacy remove, which would look for a member literally named "me".
    if !state.space_features.membership_writer_api {
        let (status, axum::Json(resp)) = feature_disabled();
        return owner_json_response_to_envelope("spaces.leave_room", user, status, resp);
    }
    let (status, axum::Json(resp)) = remove_member(state, user, space_ref, "me").await;
    owner_json_response_to_envelope("spaces.leave_room", user, status, resp)
}

/// WS04-S2: `spaces.update_room_details`. Rides the same gateway function as
/// `PATCH /api/v1/spaces/{space_ref}`, so the flag, the owner-or-manager gate and the typed
/// errors exist once. The id is human-only (never model-executable).
pub(in crate::domains::actions) async fn dispatch_spaces_update_room_details(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    if space_ref.is_empty() {
        return ticket_bad_request("spaces.update_room_details requires a non-empty 'spaceRef'");
    }
    let mut body = Map::new();
    for field in ["name", "topic"] {
        if let Some(value) = input.get(field) {
            body.insert(field.to_owned(), value.clone());
        }
    }
    let (status, axum::Json(resp)) =
        update_room_details(state, user, space_ref, &Value::Object(body)).await;
    owner_json_response_to_envelope("spaces.update_room_details", user, status, resp)
}

/// WS04-S8: `spaces.join_room`. Rides the same gateway function as
/// `POST /api/v1/spaces/{space_ref}/join`, so the flag `SPACE_ROOM_DIRECTORY_API`, Control's
/// open-room rule (a private room answers the same 404 as an unknown one) and the audience
/// reconcile exist once. Human-only (never model-executable). The caller's idempotency key is
/// forwarded unchanged so a retried join replays instead of re-applying.
pub(in crate::domains::actions) async fn dispatch_spaces_join_room(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
    idempotency_key: &str,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    if space_ref.is_empty() {
        return ticket_bad_request("spaces.join_room requires a non-empty 'spaceRef'");
    }
    let mut body = Map::new();
    if !idempotency_key.trim().is_empty() {
        body.insert("idempotency_key".to_owned(), json!(idempotency_key.trim()));
    }
    let (status, axum::Json(resp)) = join_room(state, user, space_ref, &Value::Object(body)).await;
    owner_json_response_to_envelope("spaces.join_room", user, status, resp)
}

/// WS04-S8: `spaces.update_room_settings`. Rides the same gateway function as
/// `PATCH /api/v1/spaces/{space_ref}/settings`; Control decides who may change visibility or the
/// self-join role and whether the organization's directory policy allows `org_public`.
/// Human-only (never model-executable).
pub(in crate::domains::actions) async fn dispatch_spaces_update_room_settings(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
    idempotency_key: &str,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    if space_ref.is_empty() {
        return ticket_bad_request("spaces.update_room_settings requires a non-empty 'spaceRef'");
    }
    let mut body = Map::new();
    if let Some(value) = input.get("visibility") {
        body.insert("visibility".to_owned(), value.clone());
    }
    if let Some(value) = input.get("joinRole") {
        body.insert("join_role".to_owned(), value.clone());
    }
    if !idempotency_key.trim().is_empty() {
        body.insert("idempotency_key".to_owned(), json!(idempotency_key.trim()));
    }
    let (status, axum::Json(resp)) =
        update_room_settings(state, user, space_ref, &Value::Object(body)).await;
    owner_json_response_to_envelope("spaces.update_room_settings", user, status, resp)
}

#[cfg(test)]
mod tests {
    use super::*;
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

    /// Control reports `role` for `space-room`; Application answers the details mutation.
    async fn stack(role: &str) -> (AppState, MockServer, MockServer) {
        let control = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/internal/spaces/space-room/membership"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"space_ref": "space-room", "kind": "room", "role": role}
            })))
            .mount(&control)
            .await;
        let application = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/mutation"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "value": {"spaceRef": "space-room", "name": "Leveranse", "topic": "Ukens leveranser"}
            })))
            .mount(&application)
            .await;
        let mut state = crate::tests::test_state(false);
        state.user_core_url = control.uri();
        state.application_convex_url = application.uri();
        state.application_convex_service_key = "application-test-key".into();
        (state, control, application)
    }

    #[tokio::test]
    async fn update_room_details_is_a_400_without_a_space_ref_and_calls_nothing() {
        let (mut state, control, application) = stack("owner").await;
        state.space_features.room_details_ui = true;
        for input in [json!({"name": "Ny"}), json!({"spaceRef": "  ", "name": "Ny"}), json!({"spaceRef": 5})] {
            let (status, body) = read(dispatch_spaces_update_room_details(&state, &user(), &input).await).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{input}");
            assert_eq!(body["error"]["code"], "invalid_input");
        }
        assert!(control.received_requests().await.unwrap().is_empty());
        assert!(application.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn update_room_details_forwards_only_name_and_topic_and_wraps_the_owner_envelope() {
        let (mut state, _control, application) = stack("manager").await;
        state.space_features.room_details_ui = true;
        let input = json!({
            "spaceRef": "space-room", "name": "Leveranse", "topic": "Ukens leveranser",
            "orgId": "forged", "role": "owner", "externalOrgId": "forged"
        });
        let (status, body) = read(dispatch_spaces_update_room_details(&state, &user(), &input).await).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["data"]["actionId"], "spaces.update_room_details");
        assert_eq!(body["data"]["status"], "completed");
        assert_eq!(
            body["data"]["result"],
            json!({"space_ref": "space-room", "name": "Leveranse", "topic": "Ukens leveranser"})
        );
        let requests = application.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let sent: Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(sent["path"], "spaces:updateRoomDetailsForGateway");
        let args = sent["args"].as_object().unwrap();
        let mut keys: Vec<&str> = args.keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(
            keys,
            ["externalAuthId", "externalOrgId", "name", "serviceKey", "spaceRef", "topic"],
            "only name and topic come from the input"
        );
        assert_eq!(args["externalOrgId"], "org-1", "the org is the session's");
    }

    #[tokio::test]
    async fn update_room_details_flag_off_is_404_and_a_viewer_is_403() {
        let (state, control, application) = stack("owner").await;
        let input = json!({"spaceRef": "space-room", "name": "Ny"});
        let (status, body) = read(dispatch_spaces_update_room_details(&state, &user(), &input).await).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "feature_disabled");
        assert!(control.received_requests().await.unwrap().is_empty());
        assert!(application.received_requests().await.unwrap().is_empty());

        let (mut state, _control, application) = stack("viewer").await;
        state.space_features.room_details_ui = true;
        let (status, body) = read(dispatch_spaces_update_room_details(&state, &user(), &input).await).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "room_role_forbidden");
        assert!(application.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn change_member_role_and_leave_room_validate_their_input_first() {
        let (state, control, _application) = stack("owner").await;
        for input in [
            json!({"memberId": "u", "role": "viewer"}),
            json!({"spaceRef": "space-room", "role": "viewer"}),
            json!({"spaceRef": "space-room", "memberId": "u"}),
        ] {
            let (status, body) =
                read(dispatch_spaces_change_member_role(&state, &user(), &input, "idem-key-1").await).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{input}");
            assert_eq!(body["error"]["code"], "invalid_input");
        }
        let (status, _) = read(dispatch_spaces_leave_room(&state, &user(), &json!({})).await).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(control.received_requests().await.unwrap().is_empty());
    }

    /// Leaving has no legacy path: with the writer off the action is refused, never mapped onto
    /// the legacy remove.
    #[tokio::test]
    async fn leave_room_is_refused_with_the_writer_off() {
        let (state, control, application) = stack("editor").await;
        let (status, body) =
            read(dispatch_spaces_leave_room(&state, &user(), &json!({"spaceRef": "space-room"})).await).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "feature_disabled");
        assert!(control.received_requests().await.unwrap().is_empty());
        assert!(application.received_requests().await.unwrap().is_empty());
    }
}
