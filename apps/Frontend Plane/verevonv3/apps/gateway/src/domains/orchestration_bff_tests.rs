//! Through the real router (session middleware included), with wiremock Auth Core, user-core,
//! org-core and model-gateway:
//!
//! * W2-WS13-S6/S7: the standing-authority routes (thread approval grants, rule proposals) pass
//!   model-gateway's status and typed body through, refuse bad ids and any confirm/dismiss body
//!   before an upstream call, always send `{}` upstream, and never forward forged scoping headers.
//! * ZDR-Z-19e: a decision naming the connection of a waived run asks Auth Core once and forwards
//!   only waived bearers; one naming no connection asks nothing.

use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;
use wiremock::{
    matchers::{header as wm_header, method, path, path_regex},
    Mock, MockServer, ResponseTemplate,
};

use crate::config::AppState;

const COOKIE: &str = "better-auth.session_token=session-1";
const DECISION: &str = "signed-decision-1";
const DECISION_PATH: &str = "/api/retention/zdr-decision";

struct Upstreams {
    auth: MockServer,
    user_core: MockServer,
    org_core: MockServer,
    model_gateway: MockServer,
}

impl Upstreams {
    async fn start() -> Self {
        let auth = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                "session": {"activeOrganizationId": "org-1"}
            })))
            .mount(&auth)
            .await;
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
            .mount(&auth)
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
            .mount(&auth)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/control-policy/internal-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "token": "policy-token", "expiresInSeconds": 300,
            })))
            .mount(&auth)
            .await;
        let user_core = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId": "user-1", "orgId": "org-1", "role": "member",
                "onboardingStatus": "COMPLETED"
            })))
            .mount(&user_core)
            .await;
        let org_core = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/organizations/org-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": {
                "id": "org-1",
                "metadata": { "interactiveRetention": { "zdr": true } },
                "retention_posture": { "effective_zdr": true },
            }})))
            .mount(&org_core)
            .await;
        Self {
            auth,
            user_core,
            org_core,
            model_gateway: MockServer::start().await,
        }
    }

    fn state(&self, waiver_switch: bool) -> AppState {
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = self.auth.uri();
        state.user_core_url = self.user_core.uri();
        state.org_core_url = self.org_core.uri();
        state.model_gateway_url = self.model_gateway.uri();
        state.zdr_waiver_ui_enabled = waiver_switch;
        state.zdr_effective_posture_read = true;
        state.retention_decision_service_api_key = Some("decision-key".to_owned());
        state
    }

    async fn upstream(&self, verb: &str, upstream_path: &str, response: ResponseTemplate) {
        Mock::given(method(verb))
            .and(path(upstream_path))
            .respond_with(response)
            .mount(&self.model_gateway)
            .await;
    }

    async fn model_gateway_requests(&self) -> Vec<wiremock::Request> {
        self.model_gateway
            .received_requests()
            .await
            .expect("recording on")
    }

    async fn decision_calls(&self) -> usize {
        self.auth
            .received_requests()
            .await
            .expect("recording on")
            .iter()
            .filter(|request| request.url.path() == DECISION_PATH)
            .count()
    }
}

async fn send(
    state: AppState,
    verb: &str,
    uri: &str,
    body: Option<&str>,
    extra_headers: &[(&str, &str)],
) -> (u16, Value) {
    let mut request = Request::builder()
        .method(verb)
        .uri(uri)
        .header("cookie", COOKIE);
    for (name, value) in extra_headers {
        request = request.header(*name, *value);
    }
    let body = match body {
        Some(body) => {
            request = request.header("content-type", "application/json");
            Body::from(body.to_owned())
        }
        None => Body::empty(),
    };
    let response = crate::build_router(state)
        .oneshot(request.body(body).expect("request"))
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

#[tokio::test]
async fn standing_authority_routes_pass_typed_answers_through_unchanged() {
    for (verb, uri, upstream_path, status, code) in [
        (
            "GET",
            "/api/v1/orchestration/threads/thread-1/approval-grants",
            "/v1/orchestration/threads/thread-1/approval-grants",
            404,
            "approval_grants_disabled",
        ),
        (
            "DELETE",
            "/api/v1/orchestration/approval-grants/apg_1",
            "/v1/orchestration/approval-grants/apg_1",
            404,
            "approval_grant_not_found",
        ),
        (
            "GET",
            "/api/v1/orchestration/rule-proposals",
            "/v1/orchestration/rule-proposals",
            412,
            "zdr_durable_mutation_forbidden",
        ),
        (
            "POST",
            "/api/v1/orchestration/rule-proposals/arp_1/confirm",
            "/v1/orchestration/rule-proposals/arp_1/confirm",
            503,
            "rule_confirm_incomplete",
        ),
        (
            "POST",
            "/api/v1/orchestration/rule-proposals/arp_1/dismiss",
            "/v1/orchestration/rule-proposals/arp_1/dismiss",
            409,
            "rule_proposal_busy",
        ),
    ] {
        let upstreams = Upstreams::start().await;
        let refusal = json!({ "error": { "code": code, "message": "refused", "details": {} } });
        upstreams
            .upstream(
                verb,
                upstream_path,
                ResponseTemplate::new(status).set_body_json(refusal.clone()),
            )
            .await;
        let (answered, body) = send(upstreams.state(false), verb, uri, None, &[]).await;
        assert_eq!(answered, status, "{verb} {uri}: {body}");
        assert_eq!(body["error"]["code"], code, "{verb} {uri}: {body}");
    }
}

#[tokio::test]
async fn confirm_and_dismiss_send_an_empty_object_and_refuse_any_client_body() {
    for action in ["confirm", "dismiss"] {
        let upstreams = Upstreams::start().await;
        let upstream_path = format!("/v1/orchestration/rule-proposals/arp_1/{action}");
        upstreams
            .upstream(
                "POST",
                &upstream_path,
                ResponseTemplate::new(200).set_body_json(json!({ "data": { "ok": true } })),
            )
            .await;
        let uri = format!("/api/v1/orchestration/rule-proposals/arp_1/{action}");
        for forged in [
            r#"{"capability":"cap.everything"}"#,
            r#"{"scope":"org","expires_at":"2099-01-01"}"#,
            r#"[]"#,
            r#""x""#,
        ] {
            let (status, _) = send(upstreams.state(false), "POST", &uri, Some(forged), &[]).await;
            assert_eq!(status, 400, "{action} {forged}");
        }
        assert!(
            upstreams.model_gateway_requests().await.is_empty(),
            "{action}"
        );

        for empty in [None, Some("{}"), Some("  ")] {
            let (status, body) = send(upstreams.state(false), "POST", &uri, empty, &[]).await;
            assert_eq!(status, 200, "{action} {empty:?}: {body}");
        }
        for request in upstreams.model_gateway_requests().await {
            let sent: Value = serde_json::from_slice(&request.body).expect("json");
            assert_eq!(sent, json!({}), "{action}");
        }
        if action == "confirm" {
            let requests = upstreams.model_gateway_requests().await;
            assert!(requests
                .iter()
                .all(|request| request.headers.get("x-capability-authorization").is_some()));
        }
    }
}

#[tokio::test]
async fn bad_standing_ids_are_refused_before_any_upstream_call() {
    let upstreams = Upstreams::start().await;
    let long_thread = "t".repeat(129);
    let long_grant = format!("apg_{}", "g".repeat(125));
    for (verb, uri) in [
        (
            "DELETE",
            "/api/v1/orchestration/approval-grants/grant-1".to_owned(),
        ),
        (
            "DELETE",
            format!("/api/v1/orchestration/approval-grants/{long_grant}"),
        ),
        (
            "POST",
            "/api/v1/orchestration/rule-proposals/prop-1/confirm".to_owned(),
        ),
        (
            "POST",
            "/api/v1/orchestration/rule-proposals/prop-1/dismiss".to_owned(),
        ),
        (
            "GET",
            format!("/api/v1/orchestration/threads/{long_thread}/approval-grants"),
        ),
    ] {
        let (status, _) = send(upstreams.state(false), verb, &uri, None, &[]).await;
        assert_eq!(status, 400, "{verb} {uri}");
    }
    assert!(upstreams.model_gateway_requests().await.is_empty());
}

#[tokio::test]
async fn forged_scoping_headers_never_reach_model_gateway() {
    let upstreams = Upstreams::start().await;
    upstreams
        .upstream(
            "GET",
            "/v1/orchestration/threads/thread-1/approval-grants",
            ResponseTemplate::new(200).set_body_json(json!({ "data": [] })),
        )
        .await;
    let (status, _) = send(
        upstreams.state(false),
        "GET",
        "/api/v1/orchestration/threads/thread-1/approval-grants",
        None,
        &[
            ("x-org-id", "org-evil"),
            ("x-user-id", "user-evil"),
            ("x-user-role", "owner"),
            ("x-session-authorization", "Bearer forged"),
        ],
    )
    .await;
    assert_eq!(status, 200);
    let requests = upstreams.model_gateway_requests().await;
    assert_eq!(requests.len(), 1);
    let sent = &requests[0].headers;
    let header = |name: &str| {
        sent.get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    assert_ne!(header("x-org-id").as_deref(), Some("org-evil"));
    assert_eq!(header("x-user-id").as_deref(), Some("user-1"));
    assert_ne!(header("x-user-role").as_deref(), Some("owner"));
    assert_ne!(
        header("x-session-authorization").as_deref(),
        Some("Bearer forged")
    );
}

// ZDR-Z-19e

#[tokio::test]
async fn a_waived_decision_asks_auth_core_once_and_forwards_only_waived_bearers() {
    let upstreams = Upstreams::start().await;
    Mock::given(method("POST"))
        .and(path(DECISION_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": {
            "decision": "waived", "decision_token": DECISION,
        }})))
        .mount(&upstreams.auth)
        .await;
    upstreams
        .upstream(
            "POST",
            "/v1/orchestration/approvals/appr-1/decide",
            ResponseTemplate::new(200).set_body_json(json!({ "data": { "status": "approved" } })),
        )
        .await;
    let (status, body) = send(
        upstreams.state(true),
        "POST",
        "/api/v1/orchestration/approvals/appr-1/decide",
        Some(
            r#"{"decision":"approve","subscription_connection_id":"conn-1","provider":"openai-codex-subscription"}"#,
        ),
        &[],
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(upstreams.decision_calls().await, 1);
    let requests = upstreams.model_gateway_requests().await;
    assert_eq!(requests.len(), 1);
    for (name, value) in requests[0].headers.iter() {
        let name = name.as_str();
        if name == "authorization" || name.ends_with("-authorization") {
            let value = value.to_str().unwrap_or_default();
            assert!(value.starts_with("Bearer waived-"), "{name}: {value}");
        }
    }
    let sent: Value = serde_json::from_slice(&requests[0].body).expect("json");
    assert!(sent.get("subscription_connection_id").is_none());
    assert!(sent.get("provider").is_none());
}

#[tokio::test]
async fn a_decision_naming_no_connection_asks_nothing() {
    let upstreams = Upstreams::start().await;
    upstreams
        .upstream(
            "POST",
            "/v1/orchestration/approvals/appr-1/decide",
            ResponseTemplate::new(200).set_body_json(json!({ "data": { "status": "approved" } })),
        )
        .await;
    for switch in [false, true] {
        let (status, body) = send(
            upstreams.state(switch),
            "POST",
            "/api/v1/orchestration/approvals/appr-1/decide",
            Some(r#"{"decision":"approve"}"#),
            &[],
        )
        .await;
        assert_eq!(status, 200, "switch={switch}: {body}");
    }
    assert_eq!(upstreams.decision_calls().await, 0);
    let auth = upstreams
        .auth
        .received_requests()
        .await
        .expect("recording on");
    assert!(auth
        .iter()
        .all(|request| request.headers.get("x-retention-decision").is_none()));
}

#[tokio::test]
async fn an_auth_core_refusal_of_a_continuation_is_relayed_and_nothing_goes_upstream() {
    let upstreams = Upstreams::start().await;
    let refusal = json!({ "error": {
        "code": "zdr_connection_not_acknowledged", "message": "m", "retryable": false, "action": "none",
    }});
    Mock::given(method("POST"))
        .and(path(DECISION_PATH))
        .respond_with(ResponseTemplate::new(412).set_body_json(refusal.clone()))
        .mount(&upstreams.auth)
        .await;
    let (status, body) = send(
        upstreams.state(true),
        "POST",
        "/api/v1/orchestration/approvals/appr-1/decide",
        Some(
            r#"{"decision":"approve","subscription_connection_id":"conn-1","provider":"openai-codex-subscription"}"#,
        ),
        &[],
    )
    .await;
    assert_eq!(status, 412);
    assert_eq!(body, refusal);
    assert!(upstreams.model_gateway_requests().await.is_empty());
}

// ZDR-Z-19e, every continuation route (round 2): plan approve and reject, todo status, run resume,
// continue and cancel, the approval preview, the agents plan approval, the chat invoke cancel and
// the chat stream resume. Each one naming the connection of a waived run asks Auth Core once and
// sends model-gateway only bearers minted with the decision; an Auth Core refusal is relayed with
// nothing upstream; with the switch on and no connection named it sends model-gateway exactly what
// it sends with the switch off.

const CONNECTION_QUERY: &str =
    "subscription_connection_id=conn-1&provider=openai-codex-subscription";

/// One continuation route: (BFF verb, BFF uri without query, body without the connection keys or
/// `None`, where the connection is named (`true`: query string, `false`: JSON body), model-gateway
/// verb, model-gateway path, upstream answer is SSE).
struct ContinuationRoute {
    verb: &'static str,
    uri: &'static str,
    body: Option<Value>,
    in_query: bool,
    upstream_verb: &'static str,
    upstream_path: &'static str,
    sse: bool,
}

fn continuation_routes() -> Vec<ContinuationRoute> {
    let route = |verb, uri, body, in_query, upstream_verb, upstream_path, sse| ContinuationRoute {
        verb,
        uri,
        body,
        in_query,
        upstream_verb,
        upstream_path,
        sse,
    };
    vec![
        route(
            "POST",
            "/api/v1/orchestration/plans/plan-1/approve",
            Some(json!({ "note": "ok" })),
            false,
            "POST",
            "/v1/orchestration/plans/plan-1/approve",
            false,
        ),
        route(
            "POST",
            "/api/v1/orchestration/plans/plan-1/reject",
            Some(json!({ "reason": "no" })),
            false,
            "POST",
            "/v1/orchestration/plans/plan-1/reject",
            false,
        ),
        route(
            "POST",
            "/api/v1/orchestration/todos/todo-1/status",
            Some(json!({ "status": "done" })),
            false,
            "POST",
            "/v1/orchestration/todos/todo-1/status",
            false,
        ),
        route(
            "POST",
            "/api/v1/orchestration/runs/run-1/resume",
            Some(json!({})),
            false,
            "POST",
            "/v1/orchestration/runs/run-1/resume",
            false,
        ),
        route(
            "POST",
            "/api/v1/orchestration/runs/run-1/cancel",
            Some(json!({})),
            false,
            "POST",
            "/v1/orchestration/runs/run-1/cancel",
            false,
        ),
        route(
            "GET",
            "/api/v1/orchestration/approvals/apr-1/preview",
            None,
            true,
            "GET",
            "/v1/orchestration/approvals/apr-1/preview",
            false,
        ),
        route(
            "POST",
            "/api/v1/orchestration/runs/run-1/continue",
            Some(json!({ "approval_id": "apr-1", "claim_token": "claim-1" })),
            true,
            "POST",
            "/v1/orchestration/runs/run-1/continue",
            true,
        ),
        route(
            "POST",
            "/api/v1/agents/runs/run-1/plan-approval",
            Some(json!({ "rung": "act", "justification": "go" })),
            false,
            "POST",
            "/v1/runs/run-1/plan-approval",
            false,
        ),
        route(
            "POST",
            "/api/v1/chat/invocations/req-1/cancel",
            Some(json!({})),
            false,
            "POST",
            "/v1/invoke/req-1/cancel",
            false,
        ),
        route(
            "GET",
            "/api/v1/chat/stream/resume/req-1",
            None,
            true,
            "GET",
            "/v1/invoke/resume/req-1",
            true,
        ),
    ]
}

impl ContinuationRoute {
    /// The request the SPA sends, naming the waived connection or not.
    fn request(&self, name_connection: bool) -> (String, Option<String>) {
        let uri = if name_connection && self.in_query {
            format!("{}?{CONNECTION_QUERY}", self.uri)
        } else {
            self.uri.to_owned()
        };
        let body = self.body.clone().map(|mut body| {
            if name_connection && !self.in_query {
                body["subscription_connection_id"] = json!("conn-1");
                body["provider"] = json!("openai-codex-subscription");
            }
            body.to_string()
        });
        (uri, body)
    }

    fn upstream_answer(&self) -> ResponseTemplate {
        if self.sse {
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string("data: {\"type\":\"done\"}\n\n")
        } else {
            ResponseTemplate::new(200).set_body_json(json!({ "data": { "ok": true } }))
        }
    }
}

async fn send_raw(state: AppState, verb: &str, uri: &str, body: Option<&str>) -> (u16, Vec<u8>) {
    let mut request = Request::builder()
        .method(verb)
        .uri(uri)
        .header("cookie", COOKIE);
    let body = match body {
        Some(body) => {
            request = request.header("content-type", "application/json");
            Body::from(body.to_owned())
        }
        None => Body::empty(),
    };
    let response = crate::build_router(state)
        .oneshot(request.body(body).expect("request"))
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

async fn waived_decision(upstreams: &Upstreams) {
    Mock::given(method("POST"))
        .and(path(DECISION_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": {
            "decision": "waived", "decision_token": DECISION,
        }})))
        .mount(&upstreams.auth)
        .await;
}

#[tokio::test]
async fn every_continuation_route_naming_a_waived_connection_asks_once_and_forwards_only_waived_bearers(
) {
    for route in continuation_routes() {
        let upstreams = Upstreams::start().await;
        waived_decision(&upstreams).await;
        upstreams
            .upstream(
                route.upstream_verb,
                route.upstream_path,
                route.upstream_answer(),
            )
            .await;
        let (uri, body) = route.request(true);
        let (status, answered) =
            send_raw(upstreams.state(true), route.verb, &uri, body.as_deref()).await;
        let answered = String::from_utf8_lossy(&answered).into_owned();
        assert_eq!(status, 200, "{} {uri}: {answered}", route.verb);
        assert_eq!(
            upstreams.decision_calls().await,
            1,
            "{} {uri}: one fresh decision per continuation",
            route.verb
        );
        let requests: Vec<_> = upstreams
            .model_gateway_requests()
            .await
            .into_iter()
            .filter(|request| request.url.path() == route.upstream_path)
            .collect();
        assert_eq!(requests.len(), 1, "{} {uri}", route.verb);
        let sent = &requests[0];
        let mut bearers = 0;
        for (name, value) in sent.headers.iter() {
            let name = name.as_str();
            if name == "authorization" || name.ends_with("-authorization") {
                bearers += 1;
                let value = value.to_str().unwrap_or_default();
                assert!(
                    value.starts_with("Bearer waived-"),
                    "{} {uri}: {name} carried a bearer minted without the waiver: {value}",
                    route.verb
                );
            }
        }
        assert!(
            bearers >= 2,
            "{} {uri}: model and session bearers",
            route.verb
        );
        for name in ["x-browser-authorization", "x-sandbox-authorization"] {
            assert!(
                sent.headers.get(name).is_none(),
                "{} {uri}: {name} sent on a waived continuation",
                route.verb
            );
        }
        assert_ne!(
            sent.headers.get("x-zdr").and_then(|v| v.to_str().ok()),
            Some("true"),
            "{} {uri}: a waived continuation is not a ZDR run",
            route.verb
        );
        let query = sent.url.query().unwrap_or_default();
        assert!(
            !query.contains("subscription_connection_id") && !query.contains("provider="),
            "{} {uri}: the connection is never forwarded: {query}",
            route.verb
        );
        if !sent.body.is_empty() {
            let forwarded: Value = serde_json::from_slice(&sent.body).expect("json");
            assert!(
                forwarded.get("subscription_connection_id").is_none()
                    && forwarded.get("provider").is_none(),
                "{} {uri}: the connection is never forwarded: {forwarded}",
                route.verb
            );
        }
    }
}

#[tokio::test]
async fn every_continuation_route_relays_an_auth_core_refusal_and_sends_nothing_upstream() {
    let refusal = json!({ "error": {
        "code": "zdr_connection_not_acknowledged", "message": "m", "retryable": false, "action": "none",
    }});
    for route in continuation_routes() {
        let upstreams = Upstreams::start().await;
        Mock::given(method("POST"))
            .and(path(DECISION_PATH))
            .respond_with(ResponseTemplate::new(412).set_body_json(refusal.clone()))
            .mount(&upstreams.auth)
            .await;
        upstreams
            .upstream(
                route.upstream_verb,
                route.upstream_path,
                route.upstream_answer(),
            )
            .await;
        let (uri, body) = route.request(true);
        let (status, answered) =
            send_raw(upstreams.state(true), route.verb, &uri, body.as_deref()).await;
        let answered: Value = serde_json::from_slice(&answered).unwrap_or(Value::Null);
        assert_eq!(status, 412, "{} {uri}: {answered}", route.verb);
        assert_eq!(answered, refusal, "{} {uri}", route.verb);
        assert!(
            upstreams.model_gateway_requests().await.is_empty(),
            "{} {uri}: nothing goes upstream after a refusal",
            route.verb
        );
    }
}

#[tokio::test]
async fn every_continuation_route_refuses_a_waived_mint_refusal_with_nothing_upstream() {
    let refusal = json!({ "error": {
        "code": "zdr_waiver_route_mismatch", "message": "m", "retryable": false, "action": "none",
    }});
    for route in continuation_routes() {
        let mut upstreams = Upstreams::start().await;
        // A fresh Auth Core double whose session-core mint refuses the decision; wiremock prefers
        // the mock mounted first among equal priorities, so the refusal is mounted before the rest.
        let auth = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/session-core/token"))
            .and(wm_header("x-retention-decision", DECISION))
            .respond_with(ResponseTemplate::new(412).set_body_json(refusal.clone()))
            .with_priority(1)
            .mount(&auth)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                "session": {"activeOrganizationId": "org-1"}
            })))
            .mount(&auth)
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
                let kind = if request.headers.get("x-retention-decision").is_some() {
                    "waived"
                } else {
                    "plain"
                };
                ResponseTemplate::new(200).set_body_json(json!({
                    "token": format!("{kind}-{audience}"), "expiresInSeconds": 300,
                }))
            })
            .with_priority(5)
            .mount(&auth)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/control-policy/internal-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "token": "policy-token", "expiresInSeconds": 300,
            })))
            .mount(&auth)
            .await;
        upstreams.auth = auth;
        waived_decision(&upstreams).await;
        upstreams
            .upstream(
                route.upstream_verb,
                route.upstream_path,
                route.upstream_answer(),
            )
            .await;
        let (uri, body) = route.request(true);
        let (status, answered) =
            send_raw(upstreams.state(true), route.verb, &uri, body.as_deref()).await;
        let answered: Value = serde_json::from_slice(&answered).unwrap_or(Value::Null);
        assert_eq!(status, 412, "{} {uri}: {answered}", route.verb);
        assert_eq!(answered, refusal, "{} {uri}", route.verb);
        assert!(
            upstreams.model_gateway_requests().await.is_empty(),
            "{} {uri}: nothing goes upstream after a mint refusal",
            route.verb
        );
    }
}

/// The parts of a model-gateway request that must not move when the switch is on and the
/// continuation names no connection: method, path and query, every header but the per-request ids,
/// and the body bytes.
fn upstream_shape(request: &wiremock::Request) -> (String, String, Vec<(String, String)>, Vec<u8>) {
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
    (
        request.method.to_string(),
        format!(
            "{}?{}",
            request.url.path(),
            request.url.query().unwrap_or_default()
        ),
        headers,
        request.body.clone(),
    )
}

#[tokio::test]
async fn a_continuation_naming_no_connection_is_byte_identical_with_the_switch_on_and_off() {
    for route in continuation_routes() {
        let mut shapes = Vec::new();
        for switch in [false, true] {
            let upstreams = Upstreams::start().await;
            upstreams
                .upstream(
                    route.upstream_verb,
                    route.upstream_path,
                    route.upstream_answer(),
                )
                .await;
            let (uri, body) = route.request(false);
            let (status, answered) =
                send_raw(upstreams.state(switch), route.verb, &uri, body.as_deref()).await;
            assert_eq!(
                status,
                200,
                "{} {uri} switch={switch}: {}",
                route.verb,
                String::from_utf8_lossy(&answered)
            );
            assert_eq!(upstreams.decision_calls().await, 0, "{} {uri}", route.verb);
            let requests = upstreams.model_gateway_requests().await;
            assert_eq!(requests.len(), 1, "{} {uri} switch={switch}", route.verb);
            shapes.push(upstream_shape(&requests[0]));
        }
        assert_eq!(
            shapes[0], shapes[1],
            "{} {}: the switch alone changed the upstream request",
            route.verb, route.uri
        );
    }
}

// The /continue relay of the org agent-policy refusals, end to end through the router.

#[tokio::test]
async fn continue_relays_org_policy_unavailable_and_org_agents_paused_with_the_bffs_own_words() {
    for (status, code) in [(503, "org_policy_unavailable"), (403, "org_agents_paused")] {
        let upstreams = Upstreams::start().await;
        upstreams
            .upstream(
                "POST",
                "/v1/orchestration/runs/run-1/continue",
                ResponseTemplate::new(status).set_body_json(json!({ "error": {
                    "code": code, "message": "upstream words <secret>",
                }})),
            )
            .await;
        let (answered, body) = send(
            upstreams.state(false),
            "POST",
            "/api/v1/orchestration/runs/run-1/continue",
            Some(r#"{"approval_id":"apr-1","claim_token":"claim-1"}"#),
            &[],
        )
        .await;
        assert_eq!(answered, status, "{code}: {body}");
        assert_eq!(body["error"]["code"], code, "{code}: {body}");
        let message = body["error"]["message"].as_str().unwrap_or_default();
        assert!(
            !message.is_empty() && !message.contains("upstream words"),
            "{code}: the BFF answers its own words, never the upstream message: {body}"
        );
    }
}

#[tokio::test]
async fn continue_relays_org_policy_unavailable_only_under_its_own_status() {
    let upstreams = Upstreams::start().await;
    upstreams
        .upstream(
            "POST",
            "/v1/orchestration/runs/run-1/continue",
            ResponseTemplate::new(500).set_body_json(json!({ "error": {
                "code": "org_policy_unavailable", "message": "m",
            }})),
        )
        .await;
    let (answered, body) = send(
        upstreams.state(false),
        "POST",
        "/api/v1/orchestration/runs/run-1/continue",
        Some(r#"{"approval_id":"apr-1","claim_token":"claim-1"}"#),
        &[],
    )
    .await;
    assert_ne!(
        body["error"]["code"], "org_policy_unavailable",
        "a code relays only under its own status: {answered} {body}"
    );
}

/// ZDR-Z-19e through the action surface: `orchestration.decide_approval`, `resume_run` and
/// `cancel_run` call the orchestration handlers directly. An agent action names no connection, so
/// even with the waiver switch on, a waived decision available, and connection keys planted in the
/// action input, nothing is asked of Auth Core and every bearer is minted without a waiver.
#[tokio::test]
async fn the_action_surface_continuations_never_ask_and_never_waive() {
    for (action_id, input, upstream_path) in [
        (
            "orchestration.decide_approval",
            json!({ "approvalId": "apr-1", "decision": "approve",
                    "subscription_connection_id": "conn-1", "provider": "openai-codex-subscription" }),
            "/v1/orchestration/approvals/apr-1/decide",
        ),
        (
            "orchestration.resume_run",
            json!({ "runId": "run-1",
                    "subscription_connection_id": "conn-1", "provider": "openai-codex-subscription" }),
            "/v1/orchestration/runs/run-1/resume",
        ),
        (
            "orchestration.cancel_run",
            json!({ "runId": "run-1",
                    "subscription_connection_id": "conn-1", "provider": "openai-codex-subscription" }),
            "/v1/orchestration/runs/run-1/cancel",
        ),
    ] {
        let upstreams = Upstreams::start().await;
        waived_decision(&upstreams).await;
        upstreams
            .upstream(
                "POST",
                upstream_path,
                ResponseTemplate::new(200).set_body_json(json!({ "data": { "ok": true } })),
            )
            .await;
        let body = json!({ "actionId": action_id, "input": input }).to_string();
        let (status, answered) = send(
            upstreams.state(true),
            "POST",
            "/api/v1/actions/execute",
            Some(&body),
            &[],
        )
        .await;
        assert_eq!(status, 200, "{action_id}: {answered}");
        assert_eq!(
            upstreams.decision_calls().await,
            0,
            "{action_id}: an action never asks for a waiver"
        );
        let requests: Vec<_> = upstreams
            .model_gateway_requests()
            .await
            .into_iter()
            .filter(|request| request.url.path() == upstream_path)
            .collect();
        assert_eq!(requests.len(), 1, "{action_id}");
        let sent = &requests[0];
        let mut bearers = 0;
        for (name, value) in sent.headers.iter() {
            let name = name.as_str();
            if name == "authorization" || name.ends_with("-authorization") {
                bearers += 1;
                let value = value.to_str().unwrap_or_default();
                assert!(
                    !value.starts_with("Bearer waived-"),
                    "{action_id}: {name} carried a waived bearer: {value}"
                );
            }
        }
        assert!(bearers >= 1, "{action_id}: a bearer was sent");
        if !sent.body.is_empty() {
            let forwarded: Value = serde_json::from_slice(&sent.body).expect("json");
            assert!(
                forwarded.get("subscription_connection_id").is_none()
                    && forwarded.get("provider").is_none(),
                "{action_id}: the connection is never forwarded: {forwarded}"
            );
        }
    }
}
