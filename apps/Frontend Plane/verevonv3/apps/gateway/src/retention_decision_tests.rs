//! ZDR-Z-19a / ZDR-Z-19e: the BFF's retention-decision client, driven by the vendored posture matrix
//! (`tests/fixtures/zdr-posture-matrix.json`, `decision_rows`) against a wiremock Auth Core and
//! org-core.
//!
//! The mapping the BFF owes for each row whose connection is not `none`:
//!
//! | matrix row                         | BFF outcome                                        |
//! |------------------------------------|----------------------------------------------------|
//! | decision_call false, org off       | `Unchanged`, no Auth Core call (model-gateway's resolver refuses any `refuse` row downstream) |
//! | decision_call false, org outage    | `Err(503 zdr_decision_unavailable)`, no Auth Core call |
//! | decision_call true, zdr            | `Zdr`                                              |
//! | decision_call true, persistent     | `Persistent`                                       |
//! | decision_call true, waived         | `Waived` with Auth Core's token                    |
//! | decision_call true, refuse         | `Err` with Auth Core's status and body unchanged   |
//! | decision_call true, Auth Core down | `Err(503 zdr_decision_unavailable)`                |

use std::time::Duration;

use axum::http::{HeaderMap, HeaderValue, StatusCode};
use serde_json::{json, Value};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, Request, ResponseTemplate,
};

use super::*;
use crate::middleware::AuthorizedMembership;

const MATRIX: &str = include_str!("../tests/fixtures/zdr-posture-matrix.json");
const DECISION_PATH: &str = "/api/retention/zdr-decision";
const POLICY_TOKEN_PATH: &str = "/api/control-policy/internal-token";
const SERVICE_KEY: &str = "retention-decision-service-key";
const COOKIE: &str = "better-auth.session_token=session-1";

fn user_of(org_id: &str) -> AuthenticatedUser {
    AuthenticatedUser {
        user_id: "user-1".to_owned(),
        user_email: "user@example.test".to_owned(),
        user_name: "User One".to_owned(),
        user_image: None,
        email_verified: true,
        auth_role: Some("member".to_owned()),
        active_org_id: Some(org_id.to_owned()),
        authorized_membership: Some(AuthorizedMembership {
            organization_id: org_id.to_owned(),
            role: "member".to_owned(),
        }),
    }
}

fn headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("cookie", HeaderValue::from_static(COOKIE));
    headers
}

fn subscription_body(explicit_zdr: bool) -> Value {
    json!({
        "content": "hello",
        "provider": SUBSCRIPTION_PROVIDER,
        "subscription_connection_id": "conn-1",
        "zdr": explicit_zdr,
    })
}

fn org_record(zdr: bool) -> Value {
    json!({ "data": {
        "id": "org-1",
        "metadata": { "interactiveRetention": { "zdr": zdr } },
        "retention_posture": { "effective_zdr": zdr },
    }})
}

struct Upstreams {
    auth_core: MockServer,
    org_core: MockServer,
    model_gateway: MockServer,
}

impl Upstreams {
    async fn start() -> Self {
        Self {
            auth_core: MockServer::start().await,
            org_core: MockServer::start().await,
            model_gateway: MockServer::start().await,
        }
    }

    fn state(&self) -> AppState {
        AppState {
            auth_core_url: self.auth_core.uri(),
            org_core_url: self.org_core.uri(),
            model_gateway_url: self.model_gateway.uri(),
            zdr_waiver_ui_enabled: true,
            zdr_effective_posture_read: true,
            retention_decision_service_api_key: Some(SERVICE_KEY.to_owned()),
            ..AppState::for_tests()
        }
    }

    /// org-core answers the org record for `org_id`; `None` is an outage (HTTP 500).
    async fn org(&self, org_id: &str, zdr: Option<bool>) {
        let response = match zdr {
            Some(zdr) => ResponseTemplate::new(200).set_body_json(org_record(zdr)),
            None => ResponseTemplate::new(500),
        };
        Mock::given(method("GET"))
            .and(path(format!("/api/v1/organizations/{org_id}")))
            .respond_with(response)
            .mount(&self.org_core)
            .await;
    }

    /// Auth Core mints a policy token named after the org it was asked for.
    async fn policy_tokens(&self) {
        Mock::given(method("POST"))
            .and(path(POLICY_TOKEN_PATH))
            .respond_with(|request: &Request| {
                let body: Value = serde_json::from_slice(&request.body).unwrap_or_default();
                let org = body["orgId"].as_str().unwrap_or_default().to_owned();
                ResponseTemplate::new(200).set_body_json(json!({
                    "token": format!("policy-token-{org}"),
                    "expiresInSeconds": 300,
                }))
            })
            .mount(&self.auth_core)
            .await;
    }

    async fn decision(&self, response: ResponseTemplate) {
        Mock::given(method("POST"))
            .and(path(DECISION_PATH))
            .respond_with(response)
            .mount(&self.auth_core)
            .await;
    }

    async fn requests_to(&self, wanted: &str) -> Vec<Request> {
        self.auth_core
            .received_requests()
            .await
            .expect("recording on")
            .into_iter()
            .filter(|request| request.url.path() == wanted)
            .collect()
    }
}

fn refusal_body(code: &str) -> Value {
    json!({ "error": {
        "code": code,
        "message": "refused",
        "retryable": code == "zdr_decision_unavailable",
        "action": "none",
    }})
}

/// What the Auth Core double answers for a row the BFF asks.
fn auth_core_answer(row: &Value) -> ResponseTemplate {
    let expect = &row["expect"];
    let outage = row["inputs"]["outage"].as_str();
    if outage == Some("auth_core") {
        // Auth Core cannot answer at all; the BFF must not guess.
        return ResponseTemplate::new(502).set_body_string("bad gateway");
    }
    match expect["decision"].as_str().expect("decision") {
        "zdr" => ResponseTemplate::new(200)
            .set_body_json(json!({ "data": { "decision": "zdr", "reason": expect["code"] } })),
        "persistent" => ResponseTemplate::new(200).set_body_json(
            json!({ "data": { "decision": "persistent", "reason": expect["code"] } }),
        ),
        "waived" => ResponseTemplate::new(200).set_body_json(json!({ "data": {
            "decision": "waived",
            "reason": expect["code"],
            "decision_token": "signed-decision-1",
        }})),
        "refuse" => {
            let code = expect["code"].as_str().expect("code");
            let status = if code == "zdr_decision_unavailable" {
                503
            } else {
                412
            };
            ResponseTemplate::new(status).set_body_json(refusal_body(code))
        }
        other => panic!("unknown decision {other}"),
    }
}

#[tokio::test]
async fn every_decision_row_with_a_connection_maps_to_its_bff_outcome() {
    let matrix: Value = serde_json::from_str(MATRIX).expect("matrix");
    let rows: Vec<&Value> = matrix["decision_rows"]
        .as_array()
        .expect("decision_rows")
        .iter()
        .filter(|row| row["inputs"]["connection"] != "none")
        .collect();
    assert!(rows.len() > 20, "the matrix has its connection rows");

    for row in rows {
        let id = row["id"].as_str().expect("id");
        let inputs = &row["inputs"];
        let expect = &row["expect"];
        let upstreams = Upstreams::start().await;
        let org_zdr = if inputs["outage"] == "org_core" {
            None
        } else {
            Some(inputs["org_zdr_intent"].as_bool().expect("intent"))
        };
        upstreams.org("org-1", org_zdr).await;
        upstreams.policy_tokens().await;
        upstreams.decision(auth_core_answer(row)).await;

        let explicit_zdr = inputs["explicit_zdr"].as_bool().expect("explicit_zdr");
        let outcome = turn_retention(
            &upstreams.state(),
            &user_of("org-1"),
            &headers(),
            &subscription_body(explicit_zdr),
        )
        .await;

        let decision_calls = upstreams.requests_to(DECISION_PATH).await;
        let called = expect["decision_call"].as_bool().expect("decision_call");
        assert_eq!(!decision_calls.is_empty(), called, "{id}: decision_call");

        if !called {
            if org_zdr.is_none() {
                let refusal = outcome.expect_err(id);
                assert_eq!(refusal.status, StatusCode::SERVICE_UNAVAILABLE, "{id}");
                assert_eq!(
                    refusal.body["error"]["code"], "zdr_decision_unavailable",
                    "{id}"
                );
            } else {
                assert_eq!(outcome, Ok(TurnRetention::Unchanged), "{id}");
            }
            assert!(
                upstreams.requests_to(POLICY_TOKEN_PATH).await.is_empty(),
                "{id}: no policy token without a decision"
            );
            continue;
        }

        assert_eq!(decision_calls.len(), 1, "{id}: asked once");
        let sent: Value = serde_json::from_slice(&decision_calls[0].body).expect("json");
        assert_eq!(
            sent,
            json!({
                "connection_id": "conn-1",
                "provider": SUBSCRIPTION_PROVIDER,
                "explicit_zdr": explicit_zdr,
            }),
            "{id}: decision body"
        );
        let request = &decision_calls[0];
        assert_eq!(
            request
                .headers
                .get("authorization")
                .and_then(|v| v.to_str().ok()),
            Some("Bearer policy-token-org-1"),
            "{id}: the org-bound policy token"
        );
        assert_eq!(
            request.headers.get("cookie").and_then(|v| v.to_str().ok()),
            Some(COOKIE),
            "{id}: the user's own cookie"
        );

        let outage = inputs["outage"].as_str();
        match (expect["decision"].as_str().expect("decision"), outage) {
            (_, Some("auth_core")) => {
                let refusal = outcome.expect_err(id);
                assert_eq!(refusal.status, StatusCode::SERVICE_UNAVAILABLE, "{id}");
                assert_eq!(
                    refusal.body["error"]["code"], "zdr_decision_unavailable",
                    "{id}"
                );
            }
            ("zdr", _) => assert_eq!(outcome, Ok(TurnRetention::Zdr), "{id}"),
            ("persistent", _) => assert_eq!(outcome, Ok(TurnRetention::Persistent), "{id}"),
            ("waived", _) => assert_eq!(
                outcome,
                Ok(TurnRetention::Waived {
                    decision_token: "signed-decision-1".to_owned()
                }),
                "{id}"
            ),
            ("refuse", _) => {
                let code = expect["code"].as_str().expect("code");
                let refusal = outcome.expect_err(id);
                let status = if code == "zdr_decision_unavailable" {
                    503
                } else {
                    412
                };
                assert_eq!(refusal.status.as_u16(), status, "{id}");
                assert_eq!(refusal.body, refusal_body(code), "{id}: relayed unchanged");
            }
            (other, _) => panic!("{id}: unknown decision {other}"),
        }
    }
}

#[tokio::test]
async fn with_the_switch_off_nothing_is_asked() {
    let upstreams = Upstreams::start().await;
    upstreams.org("org-1", Some(true)).await;
    upstreams.policy_tokens().await;
    upstreams.decision(ResponseTemplate::new(200)).await;
    let state = AppState {
        zdr_waiver_ui_enabled: false,
        ..upstreams.state()
    };
    assert_eq!(
        turn_retention(
            &state,
            &user_of("org-1"),
            &headers(),
            &subscription_body(false)
        )
        .await,
        Ok(TurnRetention::Unchanged)
    );
    assert_eq!(
        continuation_retention(
            &state,
            &user_of("org-1"),
            &headers(),
            Some(("conn-1", SUBSCRIPTION_PROVIDER))
        )
        .await,
        Ok(TurnRetention::Unchanged)
    );
    assert!(upstreams
        .auth_core
        .received_requests()
        .await
        .unwrap()
        .is_empty());
    assert!(upstreams
        .org_core
        .received_requests()
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn an_auth_core_timeout_is_a_typed_unavailable_refusal() {
    let upstreams = Upstreams::start().await;
    upstreams.org("org-1", Some(true)).await;
    upstreams.policy_tokens().await;
    upstreams
        .decision(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "data": { "decision": "persistent" } }))
                .set_delay(CALL_TIMEOUT + Duration::from_secs(2)),
        )
        .await;
    let refusal = turn_retention(
        &upstreams.state(),
        &user_of("org-1"),
        &headers(),
        &subscription_body(false),
    )
    .await
    .expect_err("a timeout refuses");
    assert_eq!(refusal.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(refusal.body["error"]["code"], "zdr_decision_unavailable");
}

/// A 412 whose code is not one the decision endpoint can answer is not relayed as if it were.
#[tokio::test]
async fn an_unknown_refusal_code_is_not_relayed() {
    let upstreams = Upstreams::start().await;
    upstreams.org("org-1", Some(true)).await;
    upstreams.policy_tokens().await;
    upstreams
        .decision(ResponseTemplate::new(412).set_body_json(refusal_body("something_else")))
        .await;
    let refusal = turn_retention(
        &upstreams.state(),
        &user_of("org-1"),
        &headers(),
        &subscription_body(false),
    )
    .await
    .expect_err("refused");
    assert_eq!(refusal.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(refusal.body["error"]["code"], "zdr_decision_unavailable");
}

#[tokio::test]
async fn a_space_body_never_asks() {
    let upstreams = Upstreams::start().await;
    upstreams.org("org-1", Some(true)).await;
    upstreams.policy_tokens().await;
    upstreams.decision(ResponseTemplate::new(200)).await;
    for (key, value) in [
        ("space_ref", json!("space-1")),
        ("spaceRef", json!("space-1")),
        ("space_id", json!("space-1")),
        ("room_ref", json!("room-1")),
        ("space_context", json!({ "space_id": "space-1" })),
    ] {
        let mut body = subscription_body(false);
        body[key] = value;
        assert_eq!(
            turn_retention(&upstreams.state(), &user_of("org-1"), &headers(), &body).await,
            Ok(TurnRetention::Unchanged),
            "{key}"
        );
    }
    assert!(upstreams.requests_to(DECISION_PATH).await.is_empty());
    assert!(upstreams.requests_to(POLICY_TOKEN_PATH).await.is_empty());
}

async fn mount_thread_lookup(upstreams: &Upstreams, threads: Value) {
    for audience in ["model-plane", "session-core", "model-gateway"] {
        Mock::given(method("GET"))
            .and(path(format!("/api/{audience}/token")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "token": format!("{audience}-token"),
                "expiresInSeconds": 300,
            })))
            .mount(&upstreams.auth_core)
            .await;
    }
    Mock::given(method("GET"))
        .and(path("/v1/threads"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "threads": threads })))
        .mount(&upstreams.model_gateway)
        .await;
}

#[tokio::test]
async fn a_follow_up_whose_thread_is_in_a_space_never_asks() {
    let upstreams = Upstreams::start().await;
    upstreams.org("org-1", Some(true)).await;
    upstreams.policy_tokens().await;
    upstreams
        .decision(ResponseTemplate::new(200).set_body_json(json!({ "data": {
            "decision": "waived", "decision_token": "signed-decision-1",
        }})))
        .await;
    mount_thread_lookup(
        &upstreams,
        json!([{ "thread_id": "thread-1", "space_id": "space-1" }]),
    )
    .await;
    let mut state = upstreams.state();
    state.space_features.thread_scope_resolve = true;
    let mut body = subscription_body(false);
    body["thread_id"] = json!("thread-1");
    assert_eq!(
        turn_retention(&state, &user_of("org-1"), &headers(), &body).await,
        Ok(TurnRetention::Unchanged)
    );
    assert!(upstreams.requests_to(DECISION_PATH).await.is_empty());

    // The same follow-up on a thread outside any Space asks.
    let upstreams = Upstreams::start().await;
    upstreams.org("org-1", Some(true)).await;
    upstreams.policy_tokens().await;
    upstreams
        .decision(ResponseTemplate::new(200).set_body_json(json!({ "data": {
            "decision": "waived", "decision_token": "signed-decision-1",
        }})))
        .await;
    mount_thread_lookup(
        &upstreams,
        json!([{ "thread_id": "thread-1", "space_id": "" }]),
    )
    .await;
    let mut state = upstreams.state();
    state.space_features.thread_scope_resolve = true;
    assert_eq!(
        turn_retention(&state, &user_of("org-1"), &headers(), &body).await,
        Ok(TurnRetention::Waived {
            decision_token: "signed-decision-1".to_owned()
        })
    );

    // A thread the caller cannot see is refused before any decision.
    let upstreams = Upstreams::start().await;
    upstreams.org("org-1", Some(true)).await;
    upstreams.policy_tokens().await;
    upstreams.decision(ResponseTemplate::new(200)).await;
    mount_thread_lookup(&upstreams, json!([])).await;
    let mut state = upstreams.state();
    state.space_features.thread_scope_resolve = true;
    let refusal = turn_retention(&state, &user_of("org-1"), &headers(), &body)
        .await
        .expect_err("not visible");
    assert_eq!(refusal.status, StatusCode::FORBIDDEN);
    assert_eq!(refusal.body["error"]["code"], "space_thread_not_visible");
    assert!(upstreams.requests_to(DECISION_PATH).await.is_empty());
}

#[tokio::test]
async fn the_policy_token_is_cached_per_org_and_never_shared_across_orgs() {
    let upstreams = Upstreams::start().await;
    upstreams.org("org-1", Some(true)).await;
    upstreams.org("org-2", Some(true)).await;
    upstreams.policy_tokens().await;
    upstreams
        .decision(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "data": { "decision": "persistent" } })),
        )
        .await;
    let state = upstreams.state();
    for org in ["org-1", "org-2", "org-1", "org-2"] {
        assert_eq!(
            turn_retention(&state, &user_of(org), &headers(), &subscription_body(false)).await,
            Ok(TurnRetention::Persistent),
            "{org}"
        );
    }
    let mints = upstreams.requests_to(POLICY_TOKEN_PATH).await;
    assert_eq!(mints.len(), 2, "one mint per org, then the cache");
    let orgs: Vec<Value> = mints
        .iter()
        .map(|request| serde_json::from_slice::<Value>(&request.body).unwrap()["orgId"].clone())
        .collect();
    assert_eq!(orgs, vec![json!("org-1"), json!("org-2")]);
    for mint in &mints {
        assert_eq!(
            mint.headers
                .get("x-service-id")
                .and_then(|v| v.to_str().ok()),
            Some(SERVICE_ID)
        );
        assert_eq!(
            mint.headers
                .get("x-service-api-key")
                .and_then(|v| v.to_str().ok()),
            Some(SERVICE_KEY)
        );
        let body: Value = serde_json::from_slice(&mint.body).unwrap();
        assert_eq!(body["scopes"], json!([DECIDE_SCOPE]));
    }
    let bearers: Vec<String> = upstreams
        .requests_to(DECISION_PATH)
        .await
        .iter()
        .map(|request| {
            request.headers["authorization"]
                .to_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(
        bearers,
        vec![
            "Bearer policy-token-org-1",
            "Bearer policy-token-org-2",
            "Bearer policy-token-org-1",
            "Bearer policy-token-org-2",
        ]
    );
}

#[tokio::test]
async fn an_unconfigured_credential_refuses_a_zdr_org_without_any_call() {
    let upstreams = Upstreams::start().await;
    upstreams.org("org-1", Some(true)).await;
    upstreams.policy_tokens().await;
    upstreams.decision(ResponseTemplate::new(200)).await;
    let state = AppState {
        retention_decision_service_api_key: None,
        ..upstreams.state()
    };
    let refusal = turn_retention(
        &state,
        &user_of("org-1"),
        &headers(),
        &subscription_body(false),
    )
    .await
    .expect_err("refused");
    assert_eq!(refusal.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(refusal.body["error"]["code"], "zdr_decision_unavailable");
    assert!(upstreams
        .auth_core
        .received_requests()
        .await
        .unwrap()
        .is_empty());
}

// ZDR-Z-19e: continuations.

#[tokio::test]
async fn a_continuation_naming_a_connection_asks_once_without_explicit_zdr() {
    let upstreams = Upstreams::start().await;
    upstreams.org("org-1", Some(true)).await;
    upstreams.policy_tokens().await;
    upstreams
        .decision(ResponseTemplate::new(200).set_body_json(json!({ "data": {
            "decision": "waived", "decision_token": "signed-decision-1",
        }})))
        .await;
    let outcome = continuation_retention(
        &upstreams.state(),
        &user_of("org-1"),
        &headers(),
        Some(("conn-1", SUBSCRIPTION_PROVIDER)),
    )
    .await;
    assert_eq!(
        outcome,
        Ok(TurnRetention::Waived {
            decision_token: "signed-decision-1".to_owned()
        })
    );
    let calls = upstreams.requests_to(DECISION_PATH).await;
    assert_eq!(calls.len(), 1);
    let sent: Value = serde_json::from_slice(&calls[0].body).unwrap();
    assert_eq!(
        sent,
        json!({
            "connection_id": "conn-1",
            "provider": SUBSCRIPTION_PROVIDER,
            "explicit_zdr": false,
        })
    );
}

#[tokio::test]
async fn a_continuation_naming_no_connection_makes_no_call() {
    let upstreams = Upstreams::start().await;
    upstreams.org("org-1", Some(true)).await;
    upstreams.policy_tokens().await;
    upstreams.decision(ResponseTemplate::new(200)).await;
    assert_eq!(
        continuation_retention(&upstreams.state(), &user_of("org-1"), &headers(), None).await,
        Ok(TurnRetention::Unchanged)
    );
    assert!(upstreams
        .auth_core
        .received_requests()
        .await
        .unwrap()
        .is_empty());
    assert!(upstreams
        .org_core
        .received_requests()
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn a_continuation_refusal_after_a_revoke_is_relayed() {
    let upstreams = Upstreams::start().await;
    upstreams.org("org-1", Some(true)).await;
    upstreams.policy_tokens().await;
    upstreams
        .decision(
            ResponseTemplate::new(412)
                .set_body_json(refusal_body("zdr_connection_not_acknowledged")),
        )
        .await;
    let refusal = continuation_retention(
        &upstreams.state(),
        &user_of("org-1"),
        &headers(),
        Some(("conn-1", SUBSCRIPTION_PROVIDER)),
    )
    .await
    .expect_err("refused");
    assert_eq!(refusal.status, StatusCode::PRECONDITION_FAILED);
    assert_eq!(
        refusal.body,
        refusal_body("zdr_connection_not_acknowledged")
    );
}

#[test]
fn a_continuation_connection_is_read_only_when_both_values_are_valid() {
    let on = AppState {
        zdr_waiver_ui_enabled: true,
        ..AppState::for_tests()
    };
    let mut body = json!({
        "decision": "approve",
        "subscription_connection_id": "conn-1",
        "provider": SUBSCRIPTION_PROVIDER,
    });
    let connection = take_continuation_connection(&on, &mut body).expect("named");
    assert_eq!(connection.as_pair(), ("conn-1", SUBSCRIPTION_PROVIDER));
    assert_eq!(
        body,
        json!({ "decision": "approve" }),
        "neither key is forwarded"
    );

    for (connection_id, provider) in [
        (json!("conn 1"), json!(SUBSCRIPTION_PROVIDER)),
        (json!("c".repeat(129)), json!(SUBSCRIPTION_PROVIDER)),
        (json!(""), json!(SUBSCRIPTION_PROVIDER)),
        (json!("conn-1"), json!(null)),
        (json!(7), json!(SUBSCRIPTION_PROVIDER)),
    ] {
        let mut body = json!({
            "subscription_connection_id": connection_id,
            "provider": provider,
        });
        assert_eq!(take_continuation_connection(&on, &mut body), None, "{body}");
    }

    // Switch off: the body is untouched, byte for byte.
    let off = AppState::for_tests();
    let original = json!({
        "decision": "approve",
        "subscription_connection_id": "conn-1",
        "provider": SUBSCRIPTION_PROVIDER,
    });
    let mut body = original.clone();
    assert_eq!(take_continuation_connection(&off, &mut body), None);
    assert_eq!(body, original);
}
