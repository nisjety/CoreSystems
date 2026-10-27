//! WS01-S5: action dispatchers for the human post contracts (`spaces.create_post`,
//! `spaces.reply_in_thread`). Both ride the same function as the two REST routes in
//! `domains/spaces/human_posts.rs`, so the flag, the Zero Data Retention refusal, the Control
//! decision and the typed errors exist once. Neither id is model-executable: a post is a person
//! speaking in a room, never an agent proposal. The caller's `clientSendId` is forwarded
//! unchanged so a retried action lands on the same row instead of posting twice.

use axum::{http::HeaderMap, response::Response};
use serde_json::{json, Value};

use crate::config::AppState;
use crate::domains::spaces::human_posts::submit_post;
use crate::middleware::AuthenticatedUser;

use super::super::dispatchers::{
    owner_json_response_to_envelope, ticket_bad_request, ticket_string,
};

/// The untrusted `{content, client_send_id}` body. The post flow validates every field and
/// rejects anything else, so a missing value is passed on as absent rather than checked here.
fn post_body(input: &Value) -> Value {
    json!({
        "content": input.get("content").cloned().unwrap_or(Value::Null),
        "client_send_id": input.get("clientSendId").cloned().unwrap_or(Value::Null),
    })
}

pub(in crate::domains::actions) async fn dispatch_spaces_create_post(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    if space_ref.is_empty() {
        return ticket_bad_request("spaces.create_post requires a non-empty 'spaceRef'");
    }
    let (status, axum::Json(resp)) =
        submit_post(state, user, headers, space_ref, None, &post_body(input)).await;
    owner_json_response_to_envelope("spaces.create_post", user, status, resp)
}

pub(in crate::domains::actions) async fn dispatch_spaces_reply_in_thread(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    let thread_id = ticket_string(input, "threadId");
    if space_ref.is_empty() || thread_id.is_empty() {
        return ticket_bad_request(
            "spaces.reply_in_thread requires non-empty 'spaceRef' and 'threadId'",
        );
    }
    let (status, axum::Json(resp)) = submit_post(
        state,
        user,
        headers,
        space_ref,
        Some(thread_id),
        &post_body(input),
    )
    .await;
    owner_json_response_to_envelope("spaces.reply_in_thread", user, status, resp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        http::{HeaderValue, StatusCode},
        response::Response,
    };
    use http_body_util::BodyExt;
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SEND_ID: &str = "send_id_0123456789ab";

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

    fn headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "cookie",
            HeaderValue::from_static("better-auth.session_token=session-1"),
        );
        headers
    }

    async fn read(response: Response) -> (StatusCode, Value) {
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    struct Servers {
        state: AppState,
        user_core: MockServer,
        _others: Vec<MockServer>,
    }

    /// Every upstream a successful post touches, mocked. Flag on.
    async fn servers() -> Servers {
        let auth = MockServer::start().await;
        for slug in ["session-core", "model-plane"] {
            Mock::given(wm_method("GET"))
                .and(wm_path(format!("/api/{slug}/token")))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({"token": "plane-token"})),
                )
                .mount(&auth)
                .await;
        }
        let org = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/organizations/org-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"data": {"metadata": {"interactiveRetention": {"zdr": false}}}}),
            ))
            .mount(&org)
            .await;
        let app = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": [
                {"spaceRef": "space-room", "kind": "room", "lifecycle": "active"}
            ]})))
            .mount(&app)
            .await;
        let user_core = MockServer::start().await;
        for name in ["thread-decision", "thread-append-decision"] {
            Mock::given(wm_method("POST"))
                .and(wm_path(format!("/api/v1/internal/spaces/{name}")))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {
                    "decision": {
                        "space_ref": "space-room", "decision_ref": "d-1",
                        "recipient_audience_ref": "a-1", "recipient_audience_revision": 1,
                        "recipient_audience_hash": "h-1", "privacy_policy_ref": "p-1",
                        "authority_revision": 1, "resource_authorization_ref": "r-1",
                        "action_schema_hash": "s-1", "payload_digest": "sha256:x",
                        "idempotency_key": format!("space-post-{SEND_ID}"),
                    },
                    "token": "tokentokentoken",
                }})))
                .mount(&user_core)
                .await;
        }
        let model = MockServer::start().await;
        for route in ["/v1/threads", "/v1/threads/thread-1/messages"] {
            Mock::given(wm_method("POST"))
                .and(wm_path(route))
                .respond_with(ResponseTemplate::new(201).set_body_json(json!({
                    "thread_id": "thread-1", "message_id": format!("space-post-{SEND_ID}"),
                    "sequence": 4, "deduplicated": false,
                })))
                .mount(&model)
                .await;
        }
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.org_core_url = org.uri();
        state.application_convex_url = app.uri();
        state.application_convex_service_key = "application-test-key".into();
        state.user_core_url = user_core.uri();
        state.model_gateway_url = model.uri();
        state.space_features.human_posts_api = true;
        Servers {
            state,
            user_core,
            _others: vec![auth, org, app, model],
        }
    }

    async fn decision_bodies(servers: &Servers) -> Vec<Value> {
        servers
            .user_core
            .received_requests()
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.url.path().starts_with("/api/v1/internal/spaces/"))
            .map(|r| serde_json::from_slice(&r.body).unwrap_or(Value::Null))
            .collect()
    }

    #[tokio::test]
    async fn both_dispatchers_400_without_their_required_ids() {
        let state = crate::tests::test_state(false);
        let user = user();
        let headers = headers();
        for input in [
            json!({"content": "hi", "clientSendId": SEND_ID}),
            json!({"spaceRef": "  ", "content": "hi", "clientSendId": SEND_ID}),
        ] {
            let (status, body) =
                read(dispatch_spaces_create_post(&state, &user, &headers, &input).await).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{input}");
            assert_eq!(body["error"]["code"], "invalid_input");
        }
        for input in [
            json!({"threadId": "thread-1", "content": "hi", "clientSendId": SEND_ID}),
            json!({"spaceRef": "space-room", "content": "hi", "clientSendId": SEND_ID}),
            json!({"spaceRef": "space-room", "threadId": " ", "content": "hi"}),
        ] {
            let (status, body) =
                read(dispatch_spaces_reply_in_thread(&state, &user, &headers, &input).await).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{input}");
            assert_eq!(body["error"]["code"], "invalid_input");
        }
    }

    #[tokio::test]
    async fn flag_off_gives_404_from_both_dispatchers() {
        let state = crate::tests::test_state(false);
        assert!(!state.space_features.human_posts_api);
        let user = user();
        let headers = headers();
        let create = json!({"spaceRef": "space-room", "content": "hi", "clientSendId": SEND_ID});
        let (status, body) =
            read(dispatch_spaces_create_post(&state, &user, &headers, &create).await).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "space_human_posts_disabled");
        let reply = json!({
            "spaceRef": "space-room", "threadId": "thread-1", "content": "hi",
            "clientSendId": SEND_ID,
        });
        let (status, body) =
            read(dispatch_spaces_reply_in_thread(&state, &user, &headers, &reply).await).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "space_human_posts_disabled");
    }

    #[tokio::test]
    async fn create_post_forwards_the_send_id_and_wraps_the_receipt() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let servers = servers().await;
        let input = json!({
            "spaceRef": "space-room", "content": " hello ", "clientSendId": SEND_ID,
        });
        let (status, body) = read(
            dispatch_spaces_create_post(&servers.state, &user(), &headers(), &input).await,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["data"]["actionId"], "spaces.create_post");
        assert_eq!(body["data"]["status"], "completed");
        assert_eq!(body["data"]["result"]["thread_id"], "thread-1");
        assert_eq!(body["data"]["result"]["space_ref"], "space-room");
        assert_eq!(body["data"]["result"]["sequence"], 4);
        let calls = decision_bodies(&servers).await;
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["idempotency_key"], format!("space-post-{SEND_ID}"));
        assert!(!body.to_string().contains("tokentokentoken"));
    }

    #[tokio::test]
    async fn reply_in_thread_forwards_the_send_id_and_wraps_the_receipt() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let servers = servers().await;
        let input = json!({
            "spaceRef": "space-room", "threadId": "thread-1", "content": "hello",
            "clientSendId": SEND_ID,
        });
        let (status, body) = read(
            dispatch_spaces_reply_in_thread(&servers.state, &user(), &headers(), &input).await,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["data"]["actionId"], "spaces.reply_in_thread");
        assert_eq!(body["data"]["result"]["message_id"], format!("space-post-{SEND_ID}"));
        let calls = decision_bodies(&servers).await;
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["post_kind"], "post");
        assert_eq!(calls[0]["idempotency_key"], format!("space-post-{SEND_ID}"));
    }

    /// A missing or malformed `clientSendId` reaches the shared validator and is refused there.
    #[tokio::test]
    async fn a_missing_client_send_id_is_refused_by_the_shared_validator() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let servers = servers().await;
        let input = json!({"spaceRef": "space-room", "content": "hello"});
        let (status, body) = read(
            dispatch_spaces_create_post(&servers.state, &user(), &headers(), &input).await,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(decision_bodies(&servers).await.is_empty());
        assert!(body["error"]["code"].is_string());
    }
}
