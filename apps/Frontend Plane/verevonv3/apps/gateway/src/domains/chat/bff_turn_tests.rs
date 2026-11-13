//! ZDR-Z-19b (waived chat turn), the side chat relay, and the AG-UI turn and its resume (the
//! capability bearer of each), driven through the routes with a wiremock Auth Core, org-core and
//! model-gateway.
//!
//! Auth Core's double mints a token named `waived-<audience>` only for a mint that carries
//! `x-retention-decision: signed-decision-1`, and `plain-<audience>` for any other mint, so a bearer
//! that reached model-gateway without the waiver is visible by name.

use axum::{body::Body, extract::Extension, http::Request, routing::post, Router};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;
use wiremock::{
    matchers::{header as wm_header, method, path, path_regex},
    Mock, MockServer, ResponseTemplate,
};

use super::{
    json_handlers,
    shared::tests::{assert_rule_answer, credential_rule_cases},
};
use crate::{
    cache::ResultCache,
    config::AppState,
    middleware::{AuthenticatedUser, AuthorizedMembership},
};

const COOKIE: &str = "better-auth.session_token=session-1";
const DECISION: &str = "signed-decision-1";
const DECISION_PATH: &str = "/api/retention/zdr-decision";

fn member() -> AuthenticatedUser {
    AuthenticatedUser {
        user_id: "user-1".to_owned(),
        user_email: "user@example.test".to_owned(),
        user_name: "Chat User".to_owned(),
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

struct Upstreams {
    auth: MockServer,
    org_core: MockServer,
    model_gateway: MockServer,
}

impl Upstreams {
    /// A ZDR org (switch-on effective field and stored intent agree).
    async fn start() -> Self {
        let auth = MockServer::start().await;
        let org_core = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/organizations/org-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": {
                "id": "org-1",
                "name": "Org One",
                "metadata": { "interactiveRetention": { "zdr": true } },
                "retention_posture": { "effective_zdr": true },
            }})))
            .mount(&org_core)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/control-policy/internal-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "token": "policy-token", "expiresInSeconds": 300,
            })))
            .mount(&auth)
            .await;
        Self {
            auth,
            org_core,
            model_gateway: MockServer::start().await,
        }
    }

    async fn decision(&self, response: ResponseTemplate) {
        Mock::given(method("POST"))
            .and(path(DECISION_PATH))
            .respond_with(response)
            .mount(&self.auth)
            .await;
    }

    /// Every audience mint: `waived-<aud>` with the decision header, `plain-<aud>` without.
    async fn mints(&self) {
        Mock::given(method("GET"))
            .and(path_regex(r"^/api/[a-z-]+/token$"))
            .and(wm_header("x-retention-decision", DECISION))
            .respond_with(|request: &wiremock::Request| {
                let audience = request
                    .url
                    .path()
                    .split('/')
                    .nth(2)
                    .unwrap_or("")
                    .to_owned();
                ResponseTemplate::new(200).set_body_json(json!({
                    "token": format!("waived-{audience}"), "expiresInSeconds": 300,
                }))
            })
            .with_priority(1)
            .mount(&self.auth)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/api/[a-z-]+/token$"))
            .respond_with(|request: &wiremock::Request| {
                let audience = request
                    .url
                    .path()
                    .split('/')
                    .nth(2)
                    .unwrap_or("")
                    .to_owned();
                ResponseTemplate::new(200).set_body_json(json!({
                    "token": format!("plain-{audience}"), "expiresInSeconds": 300,
                }))
            })
            .with_priority(5)
            .mount(&self.auth)
            .await;
    }

    fn state(&self, waiver_switch: bool) -> AppState {
        AppState {
            auth_core_url: self.auth.uri(),
            org_core_url: self.org_core.uri(),
            model_gateway_url: self.model_gateway.uri(),
            cache: ResultCache::disabled(),
            zdr_waiver_ui_enabled: waiver_switch,
            zdr_effective_posture_read: true,
            retention_decision_service_api_key: Some("decision-key".to_owned()),
            ..crate::tests::test_state(false)
        }
    }

    async fn auth_requests(&self) -> Vec<wiremock::Request> {
        self.auth.received_requests().await.expect("recording on")
    }

    async fn model_gateway_requests(&self, wanted: &str) -> Vec<wiremock::Request> {
        self.model_gateway
            .received_requests()
            .await
            .expect("recording on")
            .into_iter()
            .filter(|request| request.url.path() == wanted)
            .collect()
    }
}

fn app(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/chat/invoke", post(json_handlers::invoke_chat))
        .route(
            "/api/v1/chat/threads/{thread_id}/side-chat",
            post(json_handlers::ask_side_chat),
        )
        .layer(Extension(member()))
        .with_state(state)
}

async fn post_json(state: AppState, uri: &str, body: Value) -> (u16, Value) {
    let response = app(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .header("cookie", COOKIE)
                .body(Body::from(body.to_string()))
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status().as_u16();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn subscription_turn() -> Value {
    json!({
        "content": "hello",
        "provider": "openai-codex-subscription",
        "subscription_connection_id": "conn-1",
        "model": "gpt-5-codex",
    })
}

/// The delegated bearer headers model-gateway receives on a JSON chat turn.
const BEARER_HEADERS: [&str; 6] = [
    "authorization",
    "x-data-plane-authorization",
    "x-inference-authorization",
    "x-execution-authorization",
    "x-cost-authorization",
    "x-session-authorization",
];

#[tokio::test]
async fn a_waived_turn_mints_every_bearer_with_the_decision_and_forwards_only_waived_bearers() {
    let upstreams = Upstreams::start().await;
    upstreams
        .decision(ResponseTemplate::new(200).set_body_json(json!({ "data": {
            "decision": "waived", "decision_token": DECISION,
        }})))
        .await;
    upstreams.mints().await;
    Mock::given(method("POST"))
        .and(path("/v1/invoke"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "content": "hi" })))
        .mount(&upstreams.model_gateway)
        .await;

    let (status, body) = post_json(
        upstreams.state(true),
        "/api/v1/chat/invoke",
        subscription_turn(),
    )
    .await;
    assert_eq!(status, 200, "{body}");

    // The decision is asked before the first mint.
    let auth = upstreams.auth_requests().await;
    let decision_at = auth
        .iter()
        .position(|request| request.url.path() == DECISION_PATH)
        .expect("the decision was asked");
    let first_mint = auth
        .iter()
        .position(|request| request.url.path().ends_with("/token"))
        .expect("bearers were minted");
    assert!(decision_at < first_mint, "decision before the first mint");

    // Every turn mint carries the decision; no browser or sandbox bearer is minted.
    for request in auth.iter().filter(|r| r.url.path().ends_with("/token")) {
        let audience = request.url.path();
        assert!(
            !audience.contains("browser-broker") && !audience.contains("sandbox-manager"),
            "{audience} minted on a waived turn"
        );
    }

    let invokes = upstreams.model_gateway_requests("/v1/invoke").await;
    assert_eq!(invokes.len(), 1);
    let invoke = &invokes[0];
    for name in BEARER_HEADERS {
        let value = invoke
            .headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_else(|| panic!("{name} missing on a waived turn"));
        assert!(
            value.starts_with("Bearer waived-"),
            "{name} carried a bearer minted without the waiver: {value}"
        );
    }
    for name in ["x-sandbox-authorization", "x-browser-authorization"] {
        assert!(
            invoke.headers.get(name).is_none(),
            "{name} sent on a waived turn"
        );
    }
    let sent: Value = serde_json::from_slice(&invoke.body).expect("json");
    assert_ne!(
        sent.get("zdr"),
        Some(&json!(true)),
        "no org raise on a waived turn"
    );
}

#[tokio::test]
async fn an_auth_core_refusal_of_one_waived_mint_is_relayed_and_nothing_goes_upstream() {
    let upstreams = Upstreams::start().await;
    upstreams
        .decision(ResponseTemplate::new(200).set_body_json(json!({ "data": {
            "decision": "waived", "decision_token": DECISION,
        }})))
        .await;
    let refusal = json!({ "error": {
        "code": "zdr_waiver_route_mismatch", "message": "m", "retryable": false, "action": "none",
    }});
    Mock::given(method("GET"))
        .and(path("/api/cost-core/token"))
        .respond_with(ResponseTemplate::new(412).set_body_json(refusal.clone()))
        .with_priority(1)
        .mount(&upstreams.auth)
        .await;
    upstreams.mints().await;
    Mock::given(method("POST"))
        .and(path("/v1/invoke"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&upstreams.model_gateway)
        .await;

    let (status, body) = post_json(
        upstreams.state(true),
        "/api/v1/chat/invoke",
        subscription_turn(),
    )
    .await;
    assert_eq!(status, 412);
    assert_eq!(body, refusal);
    assert!(upstreams
        .model_gateway_requests("/v1/invoke")
        .await
        .is_empty());
}

#[tokio::test]
async fn a_decision_refusal_answers_before_any_mint() {
    let upstreams = Upstreams::start().await;
    let refusal = json!({ "error": {
        "code": "zdr_connection_not_acknowledged", "message": "m", "retryable": false, "action": "none",
    }});
    upstreams
        .decision(ResponseTemplate::new(412).set_body_json(refusal.clone()))
        .await;
    upstreams.mints().await;
    let (status, body) = post_json(
        upstreams.state(true),
        "/api/v1/chat/invoke",
        subscription_turn(),
    )
    .await;
    assert_eq!(status, 412);
    assert_eq!(body, refusal);
    assert!(upstreams
        .auth_requests()
        .await
        .iter()
        .all(|request| !request.url.path().ends_with("/token")));
    assert!(upstreams
        .model_gateway_requests("/v1/invoke")
        .await
        .is_empty());
}

#[tokio::test]
async fn with_the_switch_off_no_decision_is_asked_and_the_org_raise_stays() {
    let upstreams = Upstreams::start().await;
    upstreams.decision(ResponseTemplate::new(200)).await;
    upstreams.mints().await;
    Mock::given(method("POST"))
        .and(path("/v1/invoke"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&upstreams.model_gateway)
        .await;
    let (status, _) = post_json(
        upstreams.state(false),
        "/api/v1/chat/invoke",
        subscription_turn(),
    )
    .await;
    assert_eq!(status, 200);
    let auth = upstreams.auth_requests().await;
    assert!(auth.iter().all(|r| r.url.path() != DECISION_PATH));
    assert!(auth
        .iter()
        .all(|r| r.headers.get("x-retention-decision").is_none()));
    let invokes = upstreams.model_gateway_requests("/v1/invoke").await;
    let sent: Value = serde_json::from_slice(&invokes[0].body).expect("json");
    assert_eq!(sent["zdr"], json!(true), "the ZDR org raise is unchanged");
}

// The main JSON turn (`POST /api/v1/chat/invoke`) applies the one 401/403 rule too (V3-bff-3-rel2).

/// A plain turn with the waiver switch off (the default), whose model-gateway answer is `status` and `reply`.
async fn invoke_answering(status: u16, reply: Value) -> (u16, Value) {
    let upstreams = Upstreams::start().await;
    upstreams.mints().await;
    Mock::given(method("POST"))
        .and(path("/v1/invoke"))
        .respond_with(ResponseTemplate::new(status).set_body_json(reply))
        .mount(&upstreams.model_gateway)
        .await;
    post_json(
        upstreams.state(false),
        "/api/v1/chat/invoke",
        json!({ "content": "hello" }),
    )
    .await
}

/// A 401 would sign the person out, so the main JSON turn never relays one, and an untyped 403 (model-gateway's generic
/// `forbidden`, or any 403 without a typed code) is answered as unavailable too. A typed 403 is relayed as sent.
#[tokio::test]
async fn the_main_json_turn_applies_the_one_credential_rule() {
    for (label, status, reply, relayed) in credential_rule_cases() {
        let (answered, body) = invoke_answering(status, reply.clone()).await;
        assert_rule_answer(
            label,
            relayed,
            &reply,
            axum::http::StatusCode::from_u16(answered).expect("a status"),
            &body,
            "delegated_auth_unavailable",
        );
    }
}

/// Switch-off behaviour is unchanged: a success and every other refusal of the relay keep their status and body.
#[tokio::test]
async fn the_main_json_turn_relays_every_other_answer_unchanged() {
    for (status, reply) in [
        (200, json!({ "content": "hi" })),
        (
            409,
            json!({ "error": { "code": "connection_ambiguous", "message": "m", "details": {} } }),
        ),
        (
            412,
            json!({ "error": { "code": "zdr_connection_not_acknowledged", "message": "m", "details": {} } }),
        ),
    ] {
        let (answered, body) = invoke_answering(status, reply.clone()).await;
        assert_eq!(answered, status, "{reply}");
        assert_eq!(body, reply, "{reply}");
    }
}

// Side chat relay.

async fn side_chat_answering(status: u16, body: Value) -> (u16, Value) {
    let upstreams = Upstreams::start().await;
    upstreams.mints().await;
    Mock::given(method("POST"))
        .and(path("/v1/threads/thread-1/side-chat"))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .mount(&upstreams.model_gateway)
        .await;
    post_json(
        upstreams.state(false),
        "/api/v1/chat/threads/thread-1/side-chat",
        json!({ "question": "What did we decide?" }),
    )
    .await
}

/// A credential model-gateway did not accept is not the browser's session: it stays 503.
#[tokio::test]
async fn an_untyped_credential_rejection_on_side_chat_is_503() {
    for status in [401, 403] {
        let (answered, body) =
            side_chat_answering(status, json!({ "error": "unauthorized" })).await;
        assert_eq!(answered, 503, "{status}");
        assert_eq!(body["error"]["code"], "delegated_auth_unavailable");
    }
}

/// model-gateway's typed 403 refusals of a side chat (an allowlisted-out model on a connection
/// thread, and the ZDR credential refusals of the connection route) must reach the browser with
/// their code, not be rewritten into "authentication unavailable".
#[tokio::test]
async fn typed_403_side_chat_refusals_are_relayed_with_their_code() {
    for code in [
        "model_not_entitled",
        "zdr_waiver_route_mismatch",
        "zdr_posture_mismatch",
    ] {
        let refusal = json!({ "error": { "code": code, "message": "refused", "details": {} } });
        let (answered, body) = side_chat_answering(403, refusal.clone()).await;
        assert_eq!(answered, 403, "{code}: {body}");
        assert_eq!(body["error"]["code"], code);
    }
}

/// Only model-gateway's typed 403 refusals are relayed. A 401 is never relayed, typed or not, since it would sign the
/// person out. Model Gateway's generic `forbidden` (a Session Core permission refusal) stays the unavailable answer:
/// relaying every typed 403 is a decision to take on purpose, and this test is where it would show.
#[tokio::test]
async fn a_typed_401_and_a_generic_403_on_side_chat_stay_503() {
    let typed = json!({ "error": { "code": "zdr_posture_mismatch", "message": "m" } });
    let (answered, body) = side_chat_answering(401, typed).await;
    assert_eq!(answered, 503, "{body}");
    assert_eq!(body["error"]["code"], "delegated_auth_unavailable");
    let generic = json!({ "error": { "code": "forbidden", "message": "m" } });
    let (answered, body) = side_chat_answering(403, generic).await;
    assert_eq!(answered, 503, "{body}");
    assert_eq!(body["error"]["code"], "delegated_auth_unavailable");
}

/// Other typed side-chat refusals keep their status and code.
#[tokio::test]
async fn non_auth_side_chat_refusals_pass_through() {
    for (status, code) in [
        (409, "connection_ambiguous"),
        (409, "side_chat_thread_retention_refused"),
        (400, "side_chat_model_required"),
        (412, "zdr_connection_not_acknowledged"),
        (404, "side_chat_disabled"),
    ] {
        let refusal = json!({ "error": { "code": code, "message": "refused", "details": {} } });
        let (answered, body) = side_chat_answering(status, refusal.clone()).await;
        assert_eq!(answered, status, "{code}");
        assert_eq!(body, refusal, "{code}");
    }
}

/// R8: on a customer-gateway or subscription thread model-gateway serves the side chat on the
/// thread's connection with the model the request names, and refuses a request that names none
/// (`side_chat_model_required`). The BFF must let the browser name that model.
#[tokio::test]
async fn a_side_chat_may_name_the_model_its_connection_thread_needs() {
    let upstreams = Upstreams::start().await;
    upstreams.mints().await;
    Mock::given(method("POST"))
        .and(path("/v1/threads/thread-1/side-chat"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "data": { "answer": "a" } })),
        )
        .mount(&upstreams.model_gateway)
        .await;
    let (status, body) = post_json(
        upstreams.state(false),
        "/api/v1/chat/threads/thread-1/side-chat",
        json!({ "question": "What did we decide?", "model": "gpt-5-codex" }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let sent = upstreams
        .model_gateway_requests("/v1/threads/thread-1/side-chat")
        .await;
    assert_eq!(sent.len(), 1);
    let forwarded: Value = serde_json::from_slice(&sent[0].body).expect("json");
    assert_eq!(forwarded["model"], "gpt-5-codex");
}

/// A named side chat model is one model id: a model with a space or a control character, or one longer than 200
/// characters, is refused as a bad request before any upstream call. A 200-character id is forwarded as it is.
#[tokio::test]
async fn a_side_chat_names_one_bounded_model_id() {
    let too_long = "m".repeat(201);
    for model in ["gpt 5", "gpt-5\ncodex", too_long.as_str()] {
        let upstreams = Upstreams::start().await;
        upstreams.mints().await;
        Mock::given(method("POST"))
            .and(path("/v1/threads/thread-1/side-chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": {} })))
            .mount(&upstreams.model_gateway)
            .await;
        let (status, body) = post_json(
            upstreams.state(false),
            "/api/v1/chat/threads/thread-1/side-chat",
            json!({ "question": "What did we decide?", "model": model }),
        )
        .await;
        assert_eq!(status, 400, "{model:?}: {body}");
        assert_eq!(body["error"]["code"], "invalid_request", "{model:?}");
        assert!(upstreams
            .model_gateway_requests("/v1/threads/thread-1/side-chat")
            .await
            .is_empty());
    }

    let longest = "m".repeat(200);
    let upstreams = Upstreams::start().await;
    upstreams.mints().await;
    Mock::given(method("POST"))
        .and(path("/v1/threads/thread-1/side-chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": {} })))
        .mount(&upstreams.model_gateway)
        .await;
    let (status, body) = post_json(
        upstreams.state(false),
        "/api/v1/chat/threads/thread-1/side-chat",
        json!({ "question": "What did we decide?", "model": longest }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let sent = upstreams
        .model_gateway_requests("/v1/threads/thread-1/side-chat")
        .await;
    assert_eq!(sent.len(), 1);
    let forwarded: Value = serde_json::from_slice(&sent[0].body).expect("json");
    assert_eq!(forwarded["model"], json!(longest));
}

/// A side chat body still cannot pick a posture or another thread.
#[tokio::test]
async fn a_side_chat_body_cannot_lower_the_posture() {
    let upstreams = Upstreams::start().await;
    upstreams.mints().await;
    Mock::given(method("POST"))
        .and(path("/v1/threads/thread-1/side-chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": {} })))
        .mount(&upstreams.model_gateway)
        .await;
    let (status, _) = post_json(
        upstreams.state(false),
        "/api/v1/chat/threads/thread-1/side-chat",
        json!({ "question": "q", "zdr": false }),
    )
    .await;
    if status == 200 {
        let sent = upstreams
            .model_gateway_requests("/v1/threads/thread-1/side-chat")
            .await;
        let forwarded: Value = serde_json::from_slice(&sent[0].body).expect("json");
        assert_eq!(forwarded["zdr"], json!(true), "the ZDR org raise holds");
    } else {
        assert_eq!(status, 422);
        assert!(upstreams
            .model_gateway_requests("/v1/threads/thread-1/side-chat")
            .await
            .is_empty());
    }
}

/// Without a model the side chat is forwarded as before: the body carries no `model` key, and a blank or null model is
/// the same as none. Model Gateway uses the field only on a connection thread, so the platform answer does not change.
#[tokio::test]
async fn a_side_chat_without_a_model_forwards_the_body_as_before() {
    let bodies = [
        json!({ "question": "What did we decide?" }),
        json!({ "question": "What did we decide?", "model": "   " }),
        json!({ "question": "What did we decide?", "model": null }),
    ];
    for body in bodies {
        let upstreams = Upstreams::start().await;
        upstreams.mints().await;
        Mock::given(method("POST"))
            .and(path("/v1/threads/thread-1/side-chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": {} })))
            .mount(&upstreams.model_gateway)
            .await;
        let (status, reply) = post_json(
            upstreams.state(false),
            "/api/v1/chat/threads/thread-1/side-chat",
            body.clone(),
        )
        .await;
        assert_eq!(status, 200, "{body}: {reply}");
        let sent = upstreams
            .model_gateway_requests("/v1/threads/thread-1/side-chat")
            .await;
        assert_eq!(sent.len(), 1, "{body}");
        let forwarded: Value = serde_json::from_slice(&sent[0].body).expect("json");
        assert!(forwarded.get("model").is_none(), "{body}: {forwarded}");
        assert_eq!(forwarded["question"], "What did we decide?", "{body}");
        let keys = forwarded.as_object().map_or(0, |object| object.len());
        assert_eq!(keys, 2, "{body}: {forwarded}");
    }
}

/// A side chat body may name a model, but not a connection: the connection of a thread is read by Model Gateway from
/// the stored thread, so `model_connection_id` and `subscription_connection_id` are refused as before.
#[tokio::test]
async fn a_side_chat_body_still_cannot_name_a_connection() {
    let upstreams = Upstreams::start().await;
    upstreams.mints().await;
    Mock::given(method("POST"))
        .and(path("/v1/threads/thread-1/side-chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": {} })))
        .mount(&upstreams.model_gateway)
        .await;
    for field in ["model_connection_id", "subscription_connection_id"] {
        let mut body = json!({ "question": "q", "model": "gpt-5-codex" });
        body[field] = json!("conn-1");
        let (status, _) = post_json(
            upstreams.state(false),
            "/api/v1/chat/threads/thread-1/side-chat",
            body,
        )
        .await;
        assert_eq!(status, 422, "{field}");
    }
    assert!(upstreams
        .model_gateway_requests("/v1/threads/thread-1/side-chat")
        .await
        .is_empty());
}

// EP-6 S10: a forged `agent_roles` never reaches model-gateway, with the pinning switch off or on.

async fn forwarded_body(
    state: AppState,
    upstreams: &Upstreams,
    uri: &str,
    upstream_path: &str,
) -> Value {
    let app = Router::new()
        .route("/api/v1/chat/invoke", post(json_handlers::invoke_chat))
        .route("/api/v1/chat/stream", super::streams::stream_route())
        .layer(Extension(member()))
        .with_state(state);
    let body = json!({
        "content": "hello",
        "agentic": true,
        "agent_roles": [{ "agent_ref": "forged", "capability_ids": ["cap.everything"] }],
        "agentRoles": [{ "agent_ref": "forged-too" }],
    });
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .header("cookie", COOKIE)
                .body(Body::from(body.to_string()))
                .expect("request"),
        )
        .await
        .expect("response");
    let _ = response.into_body().collect().await;
    let sent = upstreams.model_gateway_requests(upstream_path).await;
    assert_eq!(sent.len(), 1, "{uri} reached model-gateway once");
    serde_json::from_slice(&sent[0].body).expect("json")
}

#[tokio::test]
async fn a_forged_agent_roles_is_stripped_on_both_chat_paths_with_the_switch_off_and_on() {
    for pinning in [false, true] {
        for (uri, upstream_path) in [
            ("/api/v1/chat/invoke", "/v1/invoke"),
            ("/api/v1/chat/stream", "/v1/invoke/stream"),
        ] {
            let upstreams = Upstreams::start().await;
            upstreams.mints().await;
            Mock::given(method("POST"))
                .and(path(upstream_path))
                .respond_with(
                    ResponseTemplate::new(200)
                        .insert_header("content-type", "text/event-stream")
                        .set_body_string("data: {}\n\n"),
                )
                .mount(&upstreams.model_gateway)
                .await;
            let mut state = upstreams.state(false);
            state.space_features.agent_version_pinning = pinning;
            let sent = forwarded_body(state, &upstreams, uri, upstream_path).await;
            assert!(
                sent.get("agent_roles").is_none(),
                "{uri} pinning={pinning}: {sent}"
            );
            assert!(
                sent.get("agentRoles").is_none(),
                "{uri} pinning={pinning}: {sent}"
            );
        }
    }
}

// EP-6 S8: the preview injector is routed by the AppState flag alone.

#[tokio::test]
async fn the_preview_injector_reads_only_the_appstate_flag() {
    let user = member();
    let preview = json!({
        "content": "Hei",
        "thread_id": "thread-1",
        "agent_preview": { "agent_ref": "agent_1" },
    });
    let off = crate::tests::test_state(false);
    assert!(!off.agent_definition_versions);
    let mut body = preview.clone();
    crate::domains::agent_actions::inject_agent_draft_preview(&off, &user, "org-1", &mut body)
        .await
        .expect("off: no refusal");
    assert_eq!(body, preview, "off: the body is exactly as it came");

    let on = AppState {
        agent_definition_versions: true,
        ..crate::tests::test_state(false)
    };
    let mut body = preview.clone();
    let (status, axum::Json(answer)) =
        crate::domains::agent_actions::inject_agent_draft_preview(&on, &user, "org-1", &mut body)
            .await
            .expect_err("on: a preview in a thread is refused");
    assert_eq!(status.as_u16(), 400);
    assert_eq!(answer["error"]["code"], "agent_preview_not_in_thread");
}

// ZDR-Z-19b on the stream path (round 2): `/api/v1/chat/stream` is the route the SPA sends every
// interactive turn through, so the waiver has to hold there as it does on the JSON invoke.

fn stream_app(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/chat/stream", super::streams::stream_route())
        .layer(Extension(member()))
        .with_state(state)
}

async fn post_stream(state: AppState, body: Value) -> (u16, Vec<u8>) {
    let response = stream_app(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/chat/stream")
                .header("content-type", "application/json")
                .header("cookie", COOKIE)
                .body(Body::from(body.to_string()))
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status().as_u16();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    (status, bytes.to_vec())
}

async fn mount_stream(upstreams: &Upstreams) {
    Mock::given(method("POST"))
        .and(path("/v1/invoke/stream"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string("data: {\"type\":\"done\"}\n\n"),
        )
        .mount(&upstreams.model_gateway)
        .await;
}

/// The bearer headers model-gateway verifies on a streamed chat turn.
const STREAM_BEARER_HEADERS: [&str; 6] = [
    "authorization",
    "x-data-plane-authorization",
    "x-inference-authorization",
    "x-execution-authorization",
    "x-cost-authorization",
    "x-session-authorization",
];

#[tokio::test]
async fn a_waived_stream_turn_mints_every_bearer_with_the_decision_and_forwards_only_waived_bearers(
) {
    let upstreams = Upstreams::start().await;
    upstreams
        .decision(ResponseTemplate::new(200).set_body_json(json!({ "data": {
            "decision": "waived", "decision_token": DECISION,
        }})))
        .await;
    upstreams.mints().await;
    mount_stream(&upstreams).await;

    let (status, body) = post_stream(upstreams.state(true), subscription_turn()).await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));

    let auth = upstreams.auth_requests().await;
    let decision_at = auth
        .iter()
        .position(|request| request.url.path() == DECISION_PATH)
        .expect("the decision was asked");
    let mints: Vec<_> = auth
        .iter()
        .enumerate()
        .filter(|(_, request)| request.url.path().ends_with("/token"))
        .collect();
    assert!(!mints.is_empty(), "bearers were minted");
    for (at, request) in &mints {
        let audience = request.url.path();
        assert!(*at > decision_at, "{audience} minted before the decision");
        assert_eq!(
            request
                .headers
                .get("x-retention-decision")
                .and_then(|v| v.to_str().ok()),
            Some(DECISION),
            "{audience} minted without the decision on a waived turn"
        );
        assert!(
            !audience.contains("browser-broker") && !audience.contains("sandbox-manager"),
            "{audience} minted on a waived turn"
        );
    }

    let streams = upstreams.model_gateway_requests("/v1/invoke/stream").await;
    assert_eq!(streams.len(), 1);
    let stream = &streams[0];
    for name in STREAM_BEARER_HEADERS {
        let value = stream
            .headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_else(|| panic!("{name} missing on a waived stream turn"));
        assert!(
            value.starts_with("Bearer waived-"),
            "{name} carried a bearer minted without the waiver: {value}"
        );
    }
    for (name, value) in stream.headers.iter() {
        let name = name.as_str();
        if name == "authorization" || name.ends_with("-authorization") {
            let value = value.to_str().unwrap_or_default();
            assert!(
                value.starts_with("Bearer waived-"),
                "{name} carried a bearer minted without the waiver: {value}"
            );
        }
    }
    for name in ["x-sandbox-authorization", "x-browser-authorization"] {
        assert!(
            stream.headers.get(name).is_none(),
            "{name} sent on a waived stream turn"
        );
    }
    assert_ne!(
        stream.headers.get("x-zdr").and_then(|v| v.to_str().ok()),
        Some("true"),
        "a waived stream turn is not raised to ZDR"
    );
    let sent: Value = serde_json::from_slice(&stream.body).expect("json");
    assert_ne!(
        sent.get("zdr"),
        Some(&json!(true)),
        "no org raise on a waived stream turn"
    );
}

#[tokio::test]
async fn an_auth_core_refusal_on_the_stream_path_is_relayed_and_nothing_goes_upstream() {
    for refused_at_decision in [true, false] {
        let upstreams = Upstreams::start().await;
        let refusal = json!({ "error": {
            "code": "zdr_connection_not_acknowledged", "message": "m", "retryable": false, "action": "none",
        }});
        if refused_at_decision {
            upstreams
                .decision(ResponseTemplate::new(412).set_body_json(refusal.clone()))
                .await;
        } else {
            upstreams
                .decision(ResponseTemplate::new(200).set_body_json(json!({ "data": {
                    "decision": "waived", "decision_token": DECISION,
                }})))
                .await;
            Mock::given(method("GET"))
                .and(path("/api/inference-core/token"))
                .respond_with(ResponseTemplate::new(412).set_body_json(refusal.clone()))
                .with_priority(1)
                .mount(&upstreams.auth)
                .await;
        }
        upstreams.mints().await;
        mount_stream(&upstreams).await;
        let (status, body) = post_stream(upstreams.state(true), subscription_turn()).await;
        let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        assert_eq!(status, 412, "decision={refused_at_decision}: {body}");
        assert_eq!(body, refusal, "decision={refused_at_decision}");
        assert!(
            upstreams
                .model_gateway_requests("/v1/invoke/stream")
                .await
                .is_empty(),
            "decision={refused_at_decision}: nothing goes upstream"
        );
    }
}

/// The parts of a model-gateway request that must not move with the switch: path, every header but
/// the host and per-request ids, and the body.
fn turn_shape(request: &wiremock::Request) -> (String, Vec<(String, String)>, Value) {
    let mut headers: Vec<(String, String)> = request
        .headers
        .iter()
        .filter(|(name, _)| {
            !matches!(
                name.as_str(),
                "host" | "x-request-id" | "traceparent" | "tracestate" | "x-correlation-id"
            )
        })
        .map(|(name, value)| {
            (
                name.as_str().to_owned(),
                value.to_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    headers.sort();
    let mut body: Value = serde_json::from_slice(&request.body).expect("json");
    // The viewer's clock is stamped per request (EP-8-S1); it is not the switch's doing.
    if let Some(object) = body.as_object_mut() {
        object.remove("viewer_now");
        object.remove("client_now");
    }
    (request.url.path().to_owned(), headers, body)
}

/// ZDR-Z-19b: "the Unchanged path is byte-identical to today". A turn the switch does not waive (a
/// platform turn, and a subscription turn in an org that is not ZDR) sends model-gateway the same
/// request with the switch on as with it off, on both the stream and the JSON path.
#[tokio::test]
async fn an_unchanged_turn_is_the_same_request_with_the_switch_on_and_off() {
    let platform_turn = json!({ "content": "hello", "model": "gpt-5" });
    for (uri, upstream_path) in [
        ("/api/v1/chat/stream", "/v1/invoke/stream"),
        ("/api/v1/chat/invoke", "/v1/invoke"),
    ] {
        for (label, turn, zdr_org) in [
            ("platform turn, ZDR org", platform_turn.clone(), true),
            ("subscription turn, non-ZDR org", subscription_turn(), false),
        ] {
            let mut shapes = Vec::new();
            for switch in [false, true] {
                let upstreams = Upstreams::start().await;
                if !zdr_org {
                    // A fresh org-core double: the org is not ZDR, by stored intent and effective field.
                    let org_core = MockServer::start().await;
                    Mock::given(method("GET"))
                        .and(path("/api/v1/organizations/org-1"))
                        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": {
                            "id": "org-1",
                            "name": "Org One",
                            "metadata": { "interactiveRetention": { "zdr": false } },
                            "retention_posture": { "effective_zdr": false },
                        }})))
                        .mount(&org_core)
                        .await;
                    let mut state = upstreams.state(switch);
                    state.org_core_url = org_core.uri();
                    upstreams.decision(ResponseTemplate::new(500)).await;
                    upstreams.mints().await;
                    mount_stream(&upstreams).await;
                    Mock::given(method("POST"))
                        .and(path("/v1/invoke"))
                        .respond_with(
                            ResponseTemplate::new(200).set_body_json(json!({ "content": "hi" })),
                        )
                        .mount(&upstreams.model_gateway)
                        .await;
                    let status = if uri.ends_with("stream") {
                        post_stream(state, turn.clone()).await.0
                    } else {
                        post_json(state, uri, turn.clone()).await.0
                    };
                    assert_eq!(status, 200, "{uri} {label} switch={switch}");
                } else {
                    upstreams.decision(ResponseTemplate::new(500)).await;
                    upstreams.mints().await;
                    mount_stream(&upstreams).await;
                    Mock::given(method("POST"))
                        .and(path("/v1/invoke"))
                        .respond_with(
                            ResponseTemplate::new(200).set_body_json(json!({ "content": "hi" })),
                        )
                        .mount(&upstreams.model_gateway)
                        .await;
                    let state = upstreams.state(switch);
                    let status = if uri.ends_with("stream") {
                        post_stream(state, turn.clone()).await.0
                    } else {
                        post_json(state, uri, turn.clone()).await.0
                    };
                    assert_eq!(status, 200, "{uri} {label} switch={switch}");
                }
                let auth = upstreams.auth_requests().await;
                assert!(
                    auth.iter().all(|r| r.url.path() != DECISION_PATH),
                    "{uri} {label} switch={switch}: no decision is asked"
                );
                let sent = upstreams.model_gateway_requests(upstream_path).await;
                assert_eq!(sent.len(), 1, "{uri} {label} switch={switch}");
                shapes.push(turn_shape(&sent[0]));
            }
            assert_eq!(
                shapes[0], shapes[1],
                "{uri} {label}: the switch alone changed the upstream request"
            );
        }
    }
}

/// ZDR-Z-19b (4): the invariant check after the thread's Space is resolved. A waived turn whose
/// outbound body names a Space (the browser's selection, a resolved `space_ref`, or Control-injected
/// Space authority) is refused 412 with the catalogue's `zdr_waiver_route_mismatch` before anything
/// goes upstream; a blank reference is no Space; every other retention passes untouched.
#[test]
fn a_waived_turn_that_names_a_space_after_resolution_is_refused_as_a_route_mismatch() {
    use crate::retention_decision::TurnRetention;
    let waived = TurnRetention::Waived {
        decision_token: DECISION.to_owned(),
    };
    for body in [
        json!({ "space_ref": "space-1" }),
        json!({ "spaceRef": "space-1" }),
        json!({ "space_context": { "space_id": "space-1" } }),
        json!({ "space_append_context": { "space_id": "space-1" } }),
    ] {
        let (status, refusal) = super::shared::waived_space_refusal(&waived, &body)
            .unwrap_or_else(|| panic!("{body}: a waived Space turn must be refused"));
        assert_eq!(status.as_u16(), 412, "{body}");
        assert_eq!(
            refusal.0["error"]["code"], "zdr_waiver_route_mismatch",
            "{body}"
        );
    }
    for body in [
        json!({}),
        json!({ "space_ref": "" }),
        json!({ "space_ref": "   " }),
        json!({ "thread_id": "thread-1" }),
    ] {
        assert!(
            super::shared::waived_space_refusal(&waived, &body).is_none(),
            "{body}: no Space named"
        );
    }
    for retention in [
        TurnRetention::Unchanged,
        TurnRetention::Zdr,
        TurnRetention::Persistent,
    ] {
        assert!(
            super::shared::waived_space_refusal(&retention, &json!({ "space_ref": "space-1" }))
                .is_none(),
            "{retention:?}: only a waived turn is checked"
        );
    }
}

// The AG-UI turn and its resume (`domains::ag_ui`). Both carry the capability bearer a native chat
// turn and its resume carry: best effort, minted without a retention decision (an AG-UI turn asks
// for none), so the bearer is plain. model-gateway reads the org's agent and PII policy with it.

fn ag_ui_app(state: AppState) -> Router {
    Router::new()
        .route(
            "/api/v1/ag-ui/stream",
            post(crate::domains::ag_ui::stream_agent_ui),
        )
        .route(
            "/api/v1/ag-ui/stream/{request_id}",
            axum::routing::get(crate::domains::ag_ui::resume_agent_ui),
        )
        .layer(Extension(member()))
        .with_state(state)
}

async fn send_app(app: Router, request: Request<Body>) -> (u16, Vec<u8>) {
    let response = app.oneshot(request).await.expect("response");
    let status = response.status().as_u16();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    (status, bytes.to_vec())
}

async fn post_ag_ui(state: AppState, body: Value) -> (u16, Vec<u8>) {
    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/ag-ui/stream")
        .header("content-type", "application/json")
        .header("cookie", COOKIE)
        .body(Body::from(body.to_string()))
        .expect("request");
    send_app(ag_ui_app(state), request).await
}

async fn get_ag_ui_resume(state: AppState, request_id: &str) -> (u16, Vec<u8>) {
    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/v1/ag-ui/stream/{request_id}"))
        .header("cookie", COOKIE)
        .body(Body::empty())
        .expect("request");
    send_app(ag_ui_app(state), request).await
}

/// The `x-capability-authorization` header of one request model-gateway received, if it carried one.
fn capability_header(request: &wiremock::Request) -> Option<&str> {
    request
        .headers
        .get("x-capability-authorization")
        .and_then(|value| value.to_str().ok())
}

#[tokio::test]
async fn an_ag_ui_turn_forwards_the_capability_bearer_as_a_native_turn_does() {
    let mut shapes = Vec::new();
    for switch in [false, true] {
        let upstreams = Upstreams::start().await;
        upstreams.mints().await;
        mount_stream(&upstreams).await;
        let (status, body) = post_ag_ui(
            upstreams.state(switch),
            json!({ "content": "hello", "model": "gpt-5" }),
        )
        .await;
        assert_eq!(
            status,
            200,
            "switch={switch}: {}",
            String::from_utf8_lossy(&body)
        );
        let streams = upstreams.model_gateway_requests("/v1/invoke/stream").await;
        assert_eq!(streams.len(), 1, "switch={switch}");
        assert_eq!(
            capability_header(&streams[0]),
            Some("Bearer plain-capability-core"),
            "switch={switch}: the AG-UI turn carries the capability bearer"
        );
        shapes.push(turn_shape(&streams[0]));
    }
    assert_eq!(
        shapes[0], shapes[1],
        "the waiver switch alone changed the AG-UI turn"
    );
}

#[tokio::test]
async fn an_ag_ui_resume_forwards_the_capability_bearer_as_the_native_resume_does() {
    let upstreams = Upstreams::start().await;
    upstreams.mints().await;
    Mock::given(method("GET"))
        .and(path("/v1/invoke/resume/req-1"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string("data: {\"type\":\"done\"}\n\n"),
        )
        .mount(&upstreams.model_gateway)
        .await;
    let (status, body) = get_ag_ui_resume(upstreams.state(false), "req-1").await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    let resumes = upstreams
        .model_gateway_requests("/v1/invoke/resume/req-1")
        .await;
    assert_eq!(resumes.len(), 1);
    assert_eq!(
        capability_header(&resumes[0]),
        Some("Bearer plain-capability-core"),
        "the AG-UI resume carries the capability bearer"
    );
}

#[tokio::test]
async fn a_failed_capability_mint_leaves_the_ag_ui_turn_running_without_the_bearer() {
    let upstreams = Upstreams::start().await;
    upstreams.mints().await;
    Mock::given(method("GET"))
        .and(path("/api/capability-core/token"))
        .respond_with(ResponseTemplate::new(503))
        .with_priority(1)
        .mount(&upstreams.auth)
        .await;
    mount_stream(&upstreams).await;
    let (status, body) = post_ag_ui(
        upstreams.state(false),
        json!({ "content": "hello", "model": "gpt-5" }),
    )
    .await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    let streams = upstreams.model_gateway_requests("/v1/invoke/stream").await;
    assert_eq!(streams.len(), 1);
    assert_eq!(
        capability_header(&streams[0]),
        None,
        "a failed mint sends no capability bearer and the turn still goes"
    );
    let inference = streams[0].headers.get("x-inference-authorization");
    assert!(inference.is_some(), "the other delegated bearers still go");
}
