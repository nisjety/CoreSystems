//! R2-OD-58-c: the goal judge presents its own persistent principal, never the gateway's zero-retention one.
//! `from_lookup` needs `MODEL_GATEWAY_GOAL_JUDGE_SERVICE_API_KEY` and never falls back to
//! `MODEL_GATEWAY_SERVICE_API_KEY`; a minted token whose `zdr` claim is true or unreadable is refused
//! (`ZdrScoped`); and the attended judge's evidence read fails closed (`evidence_or_skip`).

use base64::Engine as _;
use mp_contracts::model_plane::v1::JudgeToolCall;
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::thread_goal::{
    evidence_or_skip, GoalVerdictTokenError, GoalVerdictTokens, JudgeSkipped,
};

fn lookup(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
    move |name| {
        pairs
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| (*value).to_owned())
    }
}

#[test]
fn from_lookup_with_only_the_gateway_service_key_is_a_configuration_error() {
    let only_gateway = lookup(&[
        ("AUTH_CORE_URL", "http://auth-core:8080"),
        ("MODEL_GATEWAY_SERVICE_ID", "model-gateway"),
        ("MODEL_GATEWAY_SERVICE_API_KEY", "zdr-principal-secret"),
    ]);
    match GoalVerdictTokens::from_lookup(&only_gateway) {
        Err(GoalVerdictTokenError::Configuration(name)) => {
            assert_eq!(name, "MODEL_GATEWAY_GOAL_JUDGE_SERVICE_API_KEY");
        }
        other => panic!("the judge must not borrow the gateway's zdr principal: {other:?}"),
    }
    // A blank judge key is unset.
    let blank = lookup(&[
        ("AUTH_CORE_URL", "http://auth-core:8080"),
        ("MODEL_GATEWAY_SERVICE_API_KEY", "zdr-principal-secret"),
        ("MODEL_GATEWAY_GOAL_JUDGE_SERVICE_API_KEY", "   "),
    ]);
    assert!(matches!(
        GoalVerdictTokens::from_lookup(&blank),
        Err(GoalVerdictTokenError::Configuration(
            "MODEL_GATEWAY_GOAL_JUDGE_SERVICE_API_KEY"
        ))
    ));
    // With the judge's own key it builds, and the credential never shows in Debug.
    let judge = lookup(&[
        ("AUTH_CORE_URL", "http://auth-core:8080"),
        ("MODEL_GATEWAY_GOAL_JUDGE_SERVICE_API_KEY", "judge-secret"),
    ]);
    let tokens = GoalVerdictTokens::from_lookup(&judge).expect("the judge's own credential");
    let debug = format!("{tokens:?}");
    assert!(debug.contains("model-gateway-goal-judge"), "{debug}");
    assert!(!debug.contains("judge-secret"), "{debug}");
}

fn token_with(payload: serde_json::Value) -> String {
    let body = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload.to_string());
    format!("eyJhbGciOiJub25lIn0.{body}.sig")
}

async fn auth_core_minting(token: String) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/session-core/internal-token"))
        .and(header("x-service-id", "model-gateway-goal-judge"))
        .and(header("x-service-api-key", "judge-secret"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "token": token,
            "expiresInSeconds": 300,
            "audience": "session-core",
        })))
        .mount(&server)
        .await;
    server
}

fn judge_tokens(server: &MockServer) -> GoalVerdictTokens {
    GoalVerdictTokens::from_service_credential(
        &server.uri(),
        "model-gateway-goal-judge".to_owned(),
        "judge-secret".to_owned(),
    )
    .expect("client")
}

#[tokio::test]
async fn a_minted_token_with_a_true_or_unreadable_zdr_claim_is_refused() {
    for payload in [
        json!({ "sub": "service:model-gateway-goal-judge", "zdr": true }),
        json!({ "sub": "service:model-gateway-goal-judge" }),
        json!({ "sub": "service:model-gateway-goal-judge", "zdr": "false" }),
    ] {
        let server = auth_core_minting(token_with(payload.clone())).await;
        let tokens = judge_tokens(&server);
        let verdict = tokens.verdict_token("org-1").await;
        assert!(
            matches!(verdict, Err(GoalVerdictTokenError::ZdrScoped)),
            "{payload}: {verdict:?}"
        );
        let read = tokens.read_token("org-1").await;
        assert!(
            matches!(read, Err(GoalVerdictTokenError::ZdrScoped)),
            "{payload}: {read:?}"
        );
        // The refusal names the registry row, never the token.
        let message = GoalVerdictTokenError::ZdrScoped.to_string();
        assert!(
            message.contains("service:model-gateway-goal-judge"),
            "{message}"
        );
        assert!(!message.contains("eyJ"), "{message}");
    }
}

#[tokio::test]
async fn a_persistent_token_is_minted_with_the_judge_scopes() {
    let token = token_with(json!({ "sub": "service:model-gateway-goal-judge", "zdr": false }));
    let server = auth_core_minting(token.clone()).await;
    let tokens = judge_tokens(&server);
    assert_eq!(
        tokens.verdict_token("org-1").await.expect("verdict token"),
        token
    );
    assert_eq!(tokens.read_token("org-1").await.expect("read token"), token);
    let scopes: Vec<serde_json::Value> = server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .map(|request| {
            serde_json::from_slice::<serde_json::Value>(&request.body).expect("json")["scopes"]
                .clone()
        })
        .collect();
    assert_eq!(
        scopes,
        vec![
            json!(["session:goal-verdict:write"]),
            json!(["session:goal-judge:read"])
        ]
    );
}

fn call(tool: &str, status: &str) -> JudgeToolCall {
    JudgeToolCall {
        tool: tool.to_owned(),
        status: status.to_owned(),
        ..Default::default()
    }
}

#[test]
fn evidence_or_skip_fails_closed_on_any_status_and_offers_a_successful_read() {
    let refusal = mp_contracts::zdr_refusal::Refusal::DURABLE_MUTATION_FORBIDDEN
        .grpc_status(tonic::Code::FailedPrecondition);
    assert_eq!(evidence_or_skip(Err(refusal)), Err(JudgeSkipped));
    assert_eq!(
        evidence_or_skip(Err(tonic::Status::unavailable("session-core down"))),
        Err(JudgeSkipped)
    );
    assert_eq!(
        evidence_or_skip(Err(tonic::Status::permission_denied("not the judge"))),
        Err(JudgeSkipped)
    );
    let empty = evidence_or_skip(Ok(Vec::new())).expect("a read with no calls is answered");
    assert!(empty.is_empty());
    let two = evidence_or_skip(Ok(vec![
        call("web_search", "completed"),
        call("fetch_url", "failed"),
    ]))
    .expect("answered");
    assert_eq!(two.len(), 2, "{two:?}");
}
