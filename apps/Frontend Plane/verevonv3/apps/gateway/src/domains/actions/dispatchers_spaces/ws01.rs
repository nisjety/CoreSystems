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

/// The untrusted `{content, client_send_id[, mentions]}` body. The post flow validates every field
/// and rejects anything else, so a missing value is passed on as absent rather than checked here.
/// DEPTH-WS08-S6-3: `mentions` is forwarded to the same planning code the REST routes use
/// (`plan_post_mentions`), so the flag, the caps, the roster checks and the typed
/// `invalid_mentions` refusal exist once. Absent or null, the body is byte-identical to before.
fn post_body(input: &Value) -> Value {
    let mut body = json!({
        "content": input.get("content").cloned().unwrap_or(Value::Null),
        "client_send_id": input.get("clientSendId").cloned().unwrap_or(Value::Null),
    });
    if let Some(mentions) = input.get("mentions").filter(|mentions| !mentions.is_null()) {
        body["mentions"] = mentions_wire(mentions);
    }
    body
}

/// The action's camelCase `mentions` as the REST wire: each entry's `subjectId` becomes
/// `subject_id`. Nothing else is touched, so any other key or type still reaches the closed
/// shape check and is refused there (an entry that names both keys keeps `subjectId` and fails).
fn mentions_wire(mentions: &Value) -> Value {
    let mut wire = mentions.clone();
    let Some(object) = wire.as_object_mut() else {
        return wire;
    };
    for key in ["people", "agents"] {
        let Some(entries) = object.get_mut(key).and_then(Value::as_array_mut) else {
            continue;
        };
        for entry in entries.iter_mut().filter_map(Value::as_object_mut) {
            if entry.contains_key("subject_id") {
                continue;
            }
            if let Some(id) = entry.remove("subjectId") {
                entry.insert("subject_id".to_owned(), id);
            }
        }
    }
    wire
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
        servers_with(false, Value::Null).await
    }

    /// As `servers`, with the org's ZDR posture and the receipt's `mentions` (omitted when null).
    /// The roster and the poster's membership are mocked too, for the mention planner.
    async fn servers_with(zdr: bool, receipt_mentions: Value) -> Servers {
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
                json!({"data": {"metadata": {"interactiveRetention": {"zdr": zdr}}}}),
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
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/internal/spaces/space-room/roster"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {
                "members": [{"subject_type": "user", "subject_id": "u-kari", "role": "editor", "status": "active"}],
                "count": 1,
            }})))
            .mount(&user_core)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/internal/spaces/space-room/membership"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"space_ref": "space-room", "kind": "room", "role": "editor"}
            })))
            .mount(&user_core)
            .await;
        let mut receipt = json!({
            "thread_id": "thread-1", "message_id": format!("space-post-{SEND_ID}"),
            "sequence": 4, "deduplicated": false,
        });
        if !receipt_mentions.is_null() {
            receipt["mentions"] = receipt_mentions;
        }
        let model = MockServer::start().await;
        for route in ["/v1/threads", "/v1/threads/thread-1/messages"] {
            Mock::given(wm_method("POST"))
                .and(wm_path(route))
                .respond_with(ResponseTemplate::new(201).set_body_json(receipt.clone()))
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

    #[test]
    fn post_body_forwards_mentions_in_the_rest_wire_shape() {
        let plain = post_body(&json!({"content": "hi", "clientSendId": SEND_ID}));
        assert!(plain.get("mentions").is_none());
        let null = post_body(&json!({"content": "hi", "clientSendId": SEND_ID, "mentions": null}));
        assert!(null.get("mentions").is_none());
        let body = post_body(&json!({
            "content": "hi", "clientSendId": SEND_ID,
            "mentions": {
                "people": [{"subjectId": "u-kari", "label": "Kari", "source": "typed"}],
                "agents": [{"subjectId": "agent-1", "source": "locked"}],
            },
        }));
        assert_eq!(
            body["mentions"],
            json!({
                "people": [{"subject_id": "u-kari", "label": "Kari", "source": "typed"}],
                "agents": [{"subject_id": "agent-1", "source": "locked"}],
            })
        );
        // Anything else passes through untouched, for the closed shape check to refuse.
        let both = post_body(&json!({
            "content": "hi", "clientSendId": SEND_ID,
            "mentions": {"people": [{"subjectId": "a", "subject_id": "b", "label": ""}], "groups": []},
        }));
        assert_eq!(
            both["mentions"],
            json!({"people": [{"subjectId": "a", "subject_id": "b", "label": ""}], "groups": []})
        );
        assert_eq!(mentions_wire(&json!("x")), json!("x"));
    }

    /// Malformed mentions reach the REST routes' planner and get its typed refusal, before any
    /// Control decision is minted.
    #[tokio::test]
    async fn malformed_mentions_get_the_shared_invalid_mentions_refusal() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut servers = servers().await;
        servers.state.space_features.mention_records_api = true;
        let too_many: Vec<Value> = (0..21)
            .map(|i| json!({"subjectId": format!("u-{i}"), "label": "x"}))
            .collect();
        for mentions in [json!({"people": too_many}), json!({"groups": []})] {
            let input = json!({
                "spaceRef": "space-room", "threadId": "thread-1", "content": "hello",
                "clientSendId": SEND_ID, "mentions": mentions,
            });
            let (status, body) = read(
                dispatch_spaces_reply_in_thread(&servers.state, &user(), &headers(), &input).await,
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
            assert_eq!(body["error"]["code"], "invalid_mentions", "{body}");
        }
        assert!(decision_bodies(&servers).await.is_empty());
    }

    /// With the mention switch off the mentions are ignored and the post stands, exactly as on
    /// the REST routes.
    #[tokio::test]
    async fn mentions_are_ignored_while_the_mention_switch_is_off() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let servers = servers().await;
        assert!(!servers.state.space_features.mention_records_api);
        let input = json!({
            "spaceRef": "space-room", "content": "hello @Kari", "clientSendId": SEND_ID,
            "mentions": {"people": [{"subjectId": "u-kari", "label": "Kari"}]},
        });
        let (status, body) = read(
            dispatch_spaces_create_post(&servers.state, &user(), &headers(), &input).await,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(body["data"]["result"].get("mentions").is_none(), "{body}");
    }

    /// Review gap (C-PRIO-03): with the mention switch on, the action plans the mentions through
    /// the shared planner, forwards ids and closed words only, and its result carries the
    /// receipt's `mentions:{recorded, rejected}`.
    #[tokio::test]
    async fn flag_on_mentions_are_forwarded_and_reported_in_the_action_result() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut servers = servers_with(
            false,
            json!([{"subject_id": "u-kari", "kind": "user", "ordinal": 0}]),
        )
        .await;
        servers.state.space_features.mention_records_api = true;
        let input = json!({
            "spaceRef": "space-room", "content": "hei @Kari og @Borte", "clientSendId": SEND_ID,
            "mentions": {"people": [
                {"subjectId": "u-kari", "label": "Kari"},
                {"subjectId": "u-borte", "label": "Borte"},
            ]},
        });
        let (status, body) = read(
            dispatch_spaces_create_post(&servers.state, &user(), &headers(), &input).await,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            body["data"]["result"]["mentions"],
            json!({
                "recorded": [{"subject_id": "u-kari", "kind": "user", "ordinal": 0}],
                "rejected": [{"subject_id": "u-borte", "code": "person_not_in_room"}],
            }),
            "{body}"
        );
        let model = &servers._others[3];
        let sent: Vec<Value> = model
            .received_requests()
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|r| serde_json::from_slice(&r.body).unwrap_or(Value::Null))
            .collect();
        assert_eq!(sent.len(), 1);
        assert_eq!(
            sent[0]["first_message"]["mentions"],
            json!([{"subject_id": "u-kari", "kind": "user", "source": "typed", "ordinal": 0}]),
        );
    }

    /// Review gap (C-PRIO-03, C2): a ZDR org's post with mentions is refused with the typed ZDR
    /// refusal from the catalogue before the roster is read, any decision is minted or anything
    /// reaches Session Core.
    #[tokio::test]
    async fn a_zdr_post_with_mentions_is_refused_before_roster_control_and_model() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut servers = servers_with(true, Value::Null).await;
        servers.state.space_features.mention_records_api = true;
        let mentions = json!({"people": [{"subjectId": "u-kari", "label": "Kari"}]});
        let create = json!({
            "spaceRef": "space-room", "content": "hei @Kari", "clientSendId": SEND_ID,
            "mentions": mentions.clone(),
        });
        let reply = json!({
            "spaceRef": "space-room", "threadId": "thread-1", "content": "hei @Kari",
            "clientSendId": SEND_ID, "mentions": mentions,
        });
        for response in [
            dispatch_spaces_create_post(&servers.state, &user(), &headers(), &create).await,
            dispatch_spaces_reply_in_thread(&servers.state, &user(), &headers(), &reply).await,
        ] {
            let (status, body) = read(response).await;
            assert_eq!(status, StatusCode::PRECONDITION_FAILED, "{body}");
            assert_eq!(body["error"]["code"], "zdr_durable_mutation_forbidden", "{body}");
        }
        let user_core_calls = servers.user_core.received_requests().await.unwrap_or_default();
        assert!(user_core_calls.is_empty(), "no roster read and no decision minted");
        assert!(servers._others[3].received_requests().await.unwrap_or_default().is_empty());
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
