//! D18: the action dispatcher for per-member room history visibility (`spaces.set_member_history_window`), and the
//! tests that pin the invite's `sharePastConversation` choice on `spaces.add_member`.
//!
//! The dispatcher rides the same gateway function as `PUT /api/v1/spaces/{space_ref}/members/{subject_id}/history-window`
//! (`room_admin::set_member_history_window`), so the gateway's own switch, the closed body fields, the typed errors and
//! Control's owner-or-manager rule, audit row and floor exist once. This module forwards three fields (mode, instant,
//! expected revision) under their snake_case names and nothing else: an input can never carry an organization, an
//! actor or a floor. The id is human-only (never model-executable). A window can only hide, it never widens what a
//! member reads, and it holds a mode and an instant, no content, so every Zero Data Retention posture behaves the same.

use axum::response::Response;
use serde_json::{Map, Value};

use crate::config::AppState;
use crate::domains::spaces::room_admin::set_member_history_window;
use crate::middleware::AuthenticatedUser;

use super::super::dispatchers::{
    owner_json_response_to_envelope, ticket_bad_request, ticket_string,
};

/// `spaces.set_member_history_window`. The window's pair rule (a `since_date` needs an instant that is not in the
/// future, the other two modes carry none) is Control's; the gateway checks the shape and the vocabulary, and a body
/// without a valid `mode` is refused before any Control call, so nothing is ever set by default.
pub(in crate::domains::actions) async fn dispatch_spaces_set_member_history_window(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    let member_id = ticket_string(input, "memberId");
    if space_ref.is_empty() || member_id.is_empty() {
        return ticket_bad_request(
            "spaces.set_member_history_window requires non-empty 'spaceRef' and 'memberId'",
        );
    }
    let mut body = Map::new();
    for (field, route_field) in [
        ("mode", "mode"),
        ("sinceAt", "since_at"),
        ("expectedRevision", "expected_revision"),
    ] {
        if let Some(value) = input.get(field) {
            body.insert(route_field.to_owned(), value.clone());
        }
    }
    let (status, axum::Json(resp)) =
        set_member_history_window(state, user, space_ref, member_id, &Value::Object(body)).await;
    owner_json_response_to_envelope("spaces.set_member_history_window", user, status, resp)
}

#[cfg(test)]
mod tests {
    use super::super::super::dispatchers::dispatch_space_add_member;
    use super::*;
    use axum::http::StatusCode;
    use http_body_util::BodyExt;
    use serde_json::json;
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const CONTROL_WINDOW: &str = "/api/v1/internal/spaces/space-room/members/user-2/history-window";
    const CONTROL_MEMBERS: &str = "/api/v1/internal/spaces/space-room/members";

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
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// A gateway with Control (and an unreachable Application, so an audience reconcile answers `unconfirmed`) mocked.
    /// `windows` is the gateway's own history-window switch; the member writer is on.
    async fn stack(windows: bool) -> (AppState, MockServer) {
        let control = MockServer::start().await;
        let mut state = crate::tests::test_state(false);
        state.user_core_url = control.uri();
        state.space_features.member_history_windows = windows;
        state.space_features.membership_writer_api = true;
        (state, control)
    }

    async fn mount(control: &MockServer, verb: &str, route: &str, status: u16, body: Value) {
        Mock::given(wm_method(verb))
            .and(wm_path(route))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .mount(control)
            .await;
    }

    /// The JSON bodies Control received on one route.
    async fn bodies(control: &MockServer, verb: &str, route: &str) -> Vec<Value> {
        control
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|request| request.method.to_string() == verb && request.url.path() == route)
            .map(|request| serde_json::from_slice(&request.body).unwrap_or(Value::Null))
            .collect()
    }

    #[tokio::test]
    async fn the_window_action_needs_a_room_and_a_member_and_calls_nothing() {
        let (state, control) = stack(true).await;
        for input in [
            json!({"memberId": "user-2", "mode": "full"}),
            json!({"spaceRef": "space-room", "mode": "full"}),
            json!({"spaceRef": "  ", "memberId": "user-2", "mode": "full"}),
            json!({"spaceRef": "space-room", "memberId": 7, "mode": "full"}),
        ] {
            let (status, body) =
                read(dispatch_spaces_set_member_history_window(&state, &user(), &input).await)
                    .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{input}");
            assert_eq!(body["error"]["code"], "invalid_input", "{input}");
        }
        assert!(control.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_window_action_forwards_the_closed_fields_and_wraps_the_owner_envelope() {
        let (state, control) = stack(true).await;
        mount(
            &control,
            "PUT",
            CONTROL_WINDOW,
            200,
            json!({"data": {
                "space_ref": "space-room", "subject_id": "user-2", "changed": true,
                "history_window": {
                    "source": "member", "mode": "since_date", "since_at": "2026-11-01T08:00:00Z", "revision": 3,
                    "set_by": "internal-user", "floor_token": "internal-secret"
                },
                "debug": "internal"
            }}),
        )
        .await;
        let input = json!({
            "spaceRef": "space-room", "memberId": "user-2", "mode": "since_date",
            "sinceAt": "2026-11-01T09:00:00+01:00", "expectedRevision": 2,
            "orgId": "forged", "role": "owner", "historyFloorAt": "1970-01-01T00:00:00Z"
        });
        let (status, body) =
            read(dispatch_spaces_set_member_history_window(&state, &user(), &input).await).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["data"]["actionId"], "spaces.set_member_history_window");
        assert_eq!(body["data"]["status"], "completed");
        assert_eq!(
            body["data"]["result"],
            json!({
                "space_ref": "space-room", "subject_id": "user-2", "changed": true,
                "history_window": {"source": "member", "mode": "since_date", "since_at": "2026-11-01T08:00:00Z", "revision": 3}
            }),
            "the window Control confirmed, rebuilt as four fields: nothing else Control said"
        );
        assert!(!body.to_string().contains("internal"), "{body}");
        assert_eq!(
            bodies(&control, "PUT", CONTROL_WINDOW).await,
            vec![
                json!({"mode": "since_date", "since_at": "2026-11-01T08:00:00Z", "expected_revision": 2})
            ],
            "only the three fields travel, the instant in its canonical UTC form"
        );
    }

    /// A body with no usable mode is refused at the gateway: no window is ever set by default.
    #[tokio::test]
    async fn the_window_action_refuses_a_missing_or_unknown_mode_before_any_control_call() {
        let (state, control) = stack(true).await;
        for input in [
            json!({"spaceRef": "space-room", "memberId": "user-2"}),
            json!({"spaceRef": "space-room", "memberId": "user-2", "mode": "room"}),
            json!({"spaceRef": "space-room", "memberId": "user-2", "mode": "everything"}),
            json!({"spaceRef": "space-room", "memberId": "user-2", "mode": ["full"]}),
            json!({"spaceRef": "space-room", "memberId": "user-2", "mode": "since_date", "sinceAt": "yesterday"}),
        ] {
            let (status, body) =
                read(dispatch_spaces_set_member_history_window(&state, &user(), &input).await)
                    .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{input}");
            assert_eq!(body["error"]["code"], "invalid_request", "{input}");
        }
        assert!(control.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_window_action_answers_404_feature_disabled_with_the_gateway_switch_off() {
        let (state, control) = stack(false).await;
        let input = json!({"spaceRef": "space-room", "memberId": "user-2", "mode": "full"});
        let (status, body) =
            read(dispatch_spaces_set_member_history_window(&state, &user(), &input).await).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "feature_disabled");
        assert!(control.received_requests().await.unwrap().is_empty());
    }

    /// Control owns who may: its typed refusal passes through with its status, and nothing is wrapped as a success.
    #[tokio::test]
    async fn the_window_action_passes_control_s_typed_refusal_through_with_its_status() {
        let (state, control) = stack(true).await;
        mount(
            &control,
            "PUT",
            CONTROL_WINDOW,
            403,
            json!({"error": {"code": "room_role_forbidden", "message": "Only a room owner or manager may change a window."}}),
        )
        .await;
        let input = json!({"spaceRef": "space-room", "memberId": "user-2", "mode": "full"});
        let (status, body) =
            read(dispatch_spaces_set_member_history_window(&state, &user(), &input).await).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "room_role_forbidden");
        assert!(body.get("data").is_none(), "{body}");
    }

    // ---- spaces.add_member carries the invite's choice ------------------------------------------------------------

    async fn add_member_stack() -> (AppState, MockServer) {
        let (state, control) = stack(false).await;
        mount(
            &control,
            "POST",
            CONTROL_MEMBERS,
            200,
            json!({"data": {
                "member_count": 3, "changed": false, "role": "editor",
                "history_window": {"source": "member", "mode": "since_join", "since_at": null, "revision": 1}
            }}),
        )
        .await;
        (state, control)
    }

    /// A "don't share the past" invite must reach Control as `false`. A dispatcher that dropped it would add the person
    /// with the past shared: it fails open against "hide means hide".
    #[tokio::test]
    async fn add_member_carries_a_false_share_past_conversation_to_control() {
        let (state, control) = add_member_stack().await;
        let input = json!({
            "spaceRef": "space-room", "memberId": "user-2", "role": "viewer", "sharePastConversation": false
        });
        let (status, body) = read(dispatch_space_add_member(&state, &user(), &input).await).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["data"]["actionId"], "spaces.add_member");
        assert_eq!(
            body["data"]["result"]["history_window"],
            json!({"source": "member", "mode": "since_join", "since_at": null, "revision": 1}),
            "the window the new member reads under comes back on the action result too"
        );
        let sent = bodies(&control, "POST", CONTROL_MEMBERS).await;
        assert_eq!(sent.len(), 1);
        assert_eq!(
            sent[0]["share_past_conversation"],
            json!(false),
            "{}",
            sent[0]
        );
        assert_eq!(sent[0]["subject_id"], "user-2");
        assert_eq!(sent[0]["role"], "viewer");
    }

    #[tokio::test]
    async fn add_member_with_no_choice_or_the_default_leaves_the_room_in_charge() {
        let (state, control) = add_member_stack().await;
        let absent = json!({"spaceRef": "space-room", "memberId": "user-2"});
        let shared =
            json!({"spaceRef": "space-room", "memberId": "user-3", "sharePastConversation": true});
        for input in [&absent, &shared] {
            let (status, body) =
                read(dispatch_space_add_member(&state, &user(), input).await).await;
            assert_eq!(status, StatusCode::OK, "{body}");
        }
        let sent = bodies(&control, "POST", CONTROL_MEMBERS).await;
        assert_eq!(sent.len(), 2);
        assert!(
            sent[0].get("share_past_conversation").is_none(),
            "no choice is not sent: {}",
            sent[0]
        );
        assert!(
            sent[0].get("role").is_none(),
            "no role is not sent: {}",
            sent[0]
        );
        assert_eq!(
            sent[1]["share_past_conversation"],
            json!(true),
            "{}",
            sent[1]
        );
    }

    /// A value that is not a boolean must never degrade into the default and share the past.
    #[tokio::test]
    async fn add_member_refuses_a_malformed_share_past_conversation_and_adds_nobody() {
        let (state, control) = add_member_stack().await;
        for choice in [
            json!("false"),
            json!(0),
            json!("no"),
            json!({"share": false}),
            json!(["false"]),
        ] {
            let input = json!({"spaceRef": "space-room", "memberId": "user-2", "sharePastConversation": choice});
            let (status, body) =
                read(dispatch_space_add_member(&state, &user(), &input).await).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{input}");
            assert_eq!(body["error"]["code"], "invalid_request", "{input}");
            assert_eq!(
                body["error"]["details"]["field"], "share_past_conversation",
                "{input}"
            );
        }
        assert!(
            bodies(&control, "POST", CONTROL_MEMBERS).await.is_empty(),
            "nobody was added: Control was never called"
        );
    }
}
