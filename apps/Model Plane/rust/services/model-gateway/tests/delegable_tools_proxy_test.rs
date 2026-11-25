//! F-f02-14: `GET /v1/cron/delegable-tools` is forwarded to capability-core's delegable tool list as the verified caller.
//! The gateway adds no query and no body, sends the caller's delegated capability bearer, and passes the answer through
//! unchanged, a refusal included. A request without the delegated bearer is refused before any upstream call.
//!
//! One test function runs the three cases in order, so they share one JWKS stand-in and never race on the process
//! environment.

use axum::{
    body::{to_bytes, Body},
    http::{header::AUTHORIZATION, Request, StatusCode},
};
use model_gateway::http_routes::build_router;
use serde_json::{json, Value};
use tower::ServiceExt;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[path = "support/offline_downstreams.rs"]
mod offline_downstreams;

const TEST_AUTH_KID: &str = "delegable-tools-proxy-test";
const TEST_AUTH_ISSUER: &str = "https://auth.test/model";
const TEST_USER: &str = "user-a";
const TEST_ORG: &str = "org-a";

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

/// A bearer for `audience`, bound to the same user, org and retention posture as the ingress token, which is what a
/// delegated capability bearer must match.
fn bearer(audience: &str) -> String {
    let now = token_now();
    encode_test_token(&json!({
        "sub": TEST_USER,
        "iss": TEST_AUTH_ISSUER,
        "aud": audience,
        "exp": now + 300,
        "nbf": now - 5,
        "org_id": TEST_ORG,
        "user_id": TEST_USER,
        "principal_type": "user",
        "scopes": ["capability:read"],
        "zdr": false
    }))
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
    std::env::set_var("CAPABILITY_CORE_AUTH_AUDIENCE", "capability-core");
    jwks
}

/// A capability-core stand-in that answers the delegable tool list with `answer` for exactly `capability_token`.
async fn capability_core(capability_token: &str, answer: ResponseTemplate) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/cron/delegable-tools"))
        .and(header(
            "authorization",
            format!("Bearer {capability_token}").as_str(),
        ))
        .respond_with(answer)
        .mount(&server)
        .await;
    server
}

/// One `GET /v1/cron/delegable-tools` through the real router, with the given delegated capability bearer (or none).
async fn get_delegable_tools(
    capability_base_url: String,
    capability_token: Option<&str>,
) -> (StatusCode, Value) {
    let mut state = offline_downstreams::offline_state();
    state.capability_core_base_url = capability_base_url;
    let mut builder = Request::builder()
        .method("GET")
        .uri("/v1/cron/delegable-tools")
        .header(AUTHORIZATION, format!("Bearer {}", bearer("model-gateway")));
    if let Some(token) = capability_token {
        builder = builder.header("x-capability-authorization", format!("Bearer {token}"));
    }
    let request = builder.body(Body::empty()).expect("request");
    let response = build_router(state, None)
        .oneshot(request)
        .await
        .expect("response");
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
async fn the_delegable_tool_list_is_forwarded_as_the_verified_caller() {
    let _jwks = start_auth().await;

    // The list is forwarded with the caller's delegated bearer, and capability-core's answer is passed through.
    let token = bearer("capability-core");
    let answer = json!({ "data": [{ "tool": "web_search", "capability": "cap.tool.web_search" }] });
    let capability =
        capability_core(&token, ResponseTemplate::new(200).set_body_json(&answer)).await;
    let (status, body) = get_delegable_tools(capability.uri(), Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, answer);
    let forwarded = capability.received_requests().await.unwrap_or_default();
    assert_eq!(forwarded.len(), 1, "the list is forwarded once");
    assert_eq!(forwarded[0].method.as_str(), "GET");
    assert!(
        forwarded[0].url.query().is_none(),
        "the gateway forwards no query: {}",
        forwarded[0].url
    );

    // capability-core's refusal (the list is off while contracts are off) passes through with its status and body.
    let refusal = json!({ "error": { "code": "not_found", "message": "Delegable tools are not available.", "details": {} } });
    let capability =
        capability_core(&token, ResponseTemplate::new(404).set_body_json(&refusal)).await;
    let (status, body) = get_delegable_tools(capability.uri(), Some(&token)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body, refusal);

    // Without the delegated capability bearer the route refuses before any upstream call.
    let capability =
        capability_core(&token, ResponseTemplate::new(200).set_body_json(&answer)).await;
    let (status, _) = get_delegable_tools(capability.uri(), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(
        capability
            .received_requests()
            .await
            .unwrap_or_default()
            .is_empty(),
        "a request without the delegated bearer reached capability-core"
    );
}
