//! W3-EP-7-S6-a: the unattended goal judge route (`POST /internal/v1/goals/:goal_id/judge-turn`) with both of its
//! switches on, through the real router. Only `service:orchestrator-core` holding `model:goal_judge` in the body's
//! own org reaches the handler's checks; a user, another service, the right service without the scope, or the right
//! service in another org is 403. The body is ids only: an extra key (the objective, an answer) or a missing id is
//! refused, and each id is checked, all before any credential is minted or any upstream is asked. A ZDR caller is
//! refused as a durable mutation. (The 404 with the switches off is pinned in `src/judge_route_tests.rs`.)
//!
//! This binary sets the two switches itself, so no other test binary sees them.

use axum::{
    body::{to_bytes, Body},
    http::{header::AUTHORIZATION, Request, StatusCode},
    Router,
};
use model_gateway::http_routes::build_router;
use serde_json::{json, Value};
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[path = "support/offline_downstreams.rs"]
mod offline_downstreams;

const TEST_AUTH_KID: &str = "judge-route-http-test";
const TEST_AUTH_ISSUER: &str = "https://auth.test/model";
const JUDGE: &str = "service:orchestrator-core";
const GOAL: &str = "goal_01HZY";

fn test_keypair() -> &'static (String, String) {
    use rsa::pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding};

    static KEYPAIR: std::sync::OnceLock<(String, String)> = std::sync::OnceLock::new();
    KEYPAIR.get_or_init(|| {
        let mut rng = rand::thread_rng();
        let private_key = rsa::RsaPrivateKey::new(&mut rng, 2048).expect("RSA key generation");
        let public_key = rsa::RsaPublicKey::from(&private_key);
        (
            private_key
                .to_pkcs8_pem(LineEnding::LF)
                .expect("private key PEM")
                .to_string(),
            public_key
                .to_public_key_pem(LineEnding::LF)
                .expect("public key PEM"),
        )
    })
}

fn encode_test_token(claims: &Value) -> String {
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};

    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(TEST_AUTH_KID.to_owned());
    encode(
        &header,
        claims,
        &EncodingKey::from_rsa_pem(test_keypair().0.as_bytes()).expect("encoding key"),
    )
    .expect("signed JWT")
}

fn token_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time")
        .as_secs()
}

async fn start_auth() -> MockServer {
    use base64::Engine as _;
    use rsa::{pkcs8::DecodePublicKey, traits::PublicKeyParts};

    let public_key =
        rsa::RsaPublicKey::from_public_key_pem(&test_keypair().1).expect("decode test public key");
    let n = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public_key.n().to_bytes_be());
    let e = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public_key.e().to_bytes_be());
    let jwks = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/.well-known/jwks.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "keys": [{ "kty": "RSA", "use": "sig", "alg": "RS256", "kid": TEST_AUTH_KID, "n": n, "e": e }]
        })))
        .mount(&jwks)
        .await;
    std::env::remove_var("MODEL_GATEWAY_AUTH_DEV_BYPASS");
    std::env::set_var(
        "AUTH_CORE_JWKS_URL",
        format!("{}/.well-known/jwks.json", jwks.uri()),
    );
    std::env::set_var("AUTH_CORE_ISSUER", TEST_AUTH_ISSUER);
    std::env::set_var("AUTH_CORE_AUDIENCE", "model-gateway");
    std::env::set_var("INFERENCE_CORE_AUTH_AUDIENCE", "inference-core");
    std::env::set_var("MODEL_GATEWAY_GOAL_UNATTENDED_ENABLED", "true");
    std::env::set_var("MODEL_GATEWAY_GOAL_JUDGE_ENABLED", "true");
    jwks
}

/// A model-gateway token for `sub` in `org`, of `principal_type`, holding `scopes`.
fn token(sub: &str, principal_type: &str, org: &str, scopes: &[&str], zdr: bool) -> String {
    let now = token_now();
    let mut claims = json!({
        "sub": sub,
        "iss": TEST_AUTH_ISSUER,
        "aud": "model-gateway",
        "exp": now + 300,
        "nbf": now - 5,
        "org_id": org,
        "principal_type": principal_type,
        "scopes": scopes,
        "zdr": zdr
    });
    if principal_type == "service" {
        claims["service_id"] = json!(sub);
        // A service token states why it was minted (3 to 500 characters), as auth-core mints one.
        claims["reason"] = json!("judge one unattended goal fire");
    } else {
        claims["user_id"] = json!(sub);
    }
    encode_test_token(&claims)
}

fn judge_token(org: &str) -> String {
    token(JUDGE, "service", org, &["model:goal_judge"], false)
}

fn ids(org: &str) -> Value {
    json!({ "run_id": "run-1", "org_id": org, "schedule_id": "sched-1", "fire_key": "fire:2026-11-12T10:00" })
}

fn app() -> Router {
    build_router(offline_downstreams::offline_state(), None)
}

async fn post(goal_id: &str, bearer: &str, body: &str) -> (StatusCode, Value) {
    let request = Request::builder()
        .method("POST")
        .uri(format!("/internal/v1/goals/{goal_id}/judge-turn"))
        .header(AUTHORIZATION, format!("Bearer {bearer}"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_owned()))
        .expect("request");
    let response = app().oneshot(request).await.expect("response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
#[serial_test::serial]
async fn only_the_orchestrator_with_the_judge_scope_in_its_own_org_reaches_the_judge() {
    let _jwks = start_auth().await;
    let body = ids("org-1").to_string();
    for (who, bearer) in [
        (
            "a user holding the scope",
            token("user-1", "user", "org-1", &["model:goal_judge"], false),
        ),
        (
            "another service holding the scope",
            token(
                "service:model-gateway",
                "service",
                "org-1",
                &["model:goal_judge"],
                false,
            ),
        ),
        (
            "the orchestrator without the scope",
            token(JUDGE, "service", "org-1", &["models:invoke"], false),
        ),
        ("the orchestrator in another org", judge_token("org-2")),
    ] {
        let (status, got) = post(GOAL, &bearer, &body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{who}: {got}");
        if !got.is_null() {
            assert_ne!(
                got["error"]["code"], "goal_judge_bearer_required",
                "{who} got past the caller check: {got}"
            );
        }
    }
    // The right caller, with no inference bearer, passes every check above and stops at the bearer.
    let (status, got) = post(GOAL, &judge_token("org-1"), &body).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{got}");
    assert_eq!(got["error"]["code"], "goal_judge_bearer_required", "{got}");
}

#[tokio::test]
#[serial_test::serial]
async fn the_judge_body_is_ids_only() {
    let _jwks = start_auth().await;
    let bearer = judge_token("org-1");

    // A key that is not one of the four ids (content the gateway must read itself) is refused.
    for extra in ["objective", "answer", "criteria", "verdict"] {
        let mut body = ids("org-1");
        body[extra] = json!("text the caller should never send");
        let (status, got) = post(GOAL, &bearer, &body.to_string()).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "an extra `{extra}` key: {got}"
        );
    }
    // Each of the four ids is required.
    for missing in ["run_id", "org_id", "schedule_id", "fire_key"] {
        let mut body = ids("org-1");
        body.as_object_mut().expect("object").remove(missing);
        let (status, got) = post(GOAL, &bearer, &body.to_string()).await;
        assert!(
            status.is_client_error() && status != StatusCode::NOT_FOUND,
            "a missing `{missing}`: {status} {got}"
        );
    }
    // Each id holds to the id rule, named in the refusal.
    for field in ["run_id", "schedule_id", "fire_key"] {
        for bad in ["", "has space", "slash/inside", &"x".repeat(129)] {
            let mut body = ids("org-1");
            body[field] = json!(bad);
            let (status, got) = post(GOAL, &bearer, &body.to_string()).await;
            assert_eq!(
                status,
                StatusCode::UNPROCESSABLE_ENTITY,
                "{field}={bad:?}: {got}"
            );
            assert_eq!(
                got["error"]["code"], "invalid_goal_judge_turn",
                "{field}={bad:?}: {got}"
            );
            assert_eq!(
                got["error"]["details"]["field"], field,
                "{field}={bad:?}: {got}"
            );
        }
    }
    // The path's goal id too.
    for bad in ["01HZY", "goal_", "goal_has-dash"] {
        let (status, got) = post(bad, &bearer, &ids("org-1").to_string()).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "goal_id={bad:?}: {got}"
        );
        assert_eq!(
            got["error"]["details"]["field"], "goal_id",
            "goal_id={bad:?}: {got}"
        );
    }
}

#[tokio::test]
#[serial_test::serial]
async fn a_zero_retention_judge_call_is_refused_as_a_durable_mutation() {
    let _jwks = start_auth().await;
    let bearer = token(JUDGE, "service", "org-1", &["model:goal_judge"], true);
    let (status, got) = post(GOAL, &bearer, &ids("org-1").to_string()).await;
    assert_eq!(status, StatusCode::PRECONDITION_FAILED, "{got}");
    assert_ne!(got["error"]["code"], "goal_judge_bearer_required", "{got}");
}
