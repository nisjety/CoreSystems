//! Two gateway replicas over one run-control bus (build plan WS04-S2).
//!
//! Each "replica" is its own `AppState` (its own cancel registry) built over one
//! `LoopbackHub`, so a Stop on the replica that does not own a stream has to cross the bus to
//! reach it, exactly as it does across two containers on NATS. The loopback hub carries the
//! messages in their wire encoding.

use axum::{
    body::{to_bytes, Body},
    http::{header::AUTHORIZATION, Request, StatusCode},
    Router,
};
use model_gateway::{
    cancel_registry::CancelRegistry,
    http_routes::build_router,
    run_control::{CancelRoute, LoopbackHub, RunControl},
    state::AppState,
};
use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tower::ServiceExt;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

const TEST_AUTH_KID: &str = "run-control-two-replica-test";
const TEST_AUTH_ISSUER: &str = "https://auth.test/model";

/// A JWKS endpoint and the process-wide auth environment the gateway verifies tokens against.
struct AuthFixture {
    _jwks: MockServer,
}

impl AuthFixture {
    async fn start() -> Self {
        use base64::Engine as _;
        use rsa::{pkcs8::DecodePublicKey, traits::PublicKeyParts};

        let (_, public_pem) = test_keypair();
        let public_key =
            rsa::RsaPublicKey::from_public_key_pem(public_pem).expect("decode test public key");
        let n =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public_key.n().to_bytes_be());
        let e =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public_key.e().to_bytes_be());
        let jwks = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/.well-known/jwks.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "keys": [{
                    "kty": "RSA", "use": "sig", "alg": "RS256",
                    "kid": TEST_AUTH_KID, "n": n, "e": e
                }]
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
        Self { _jwks: jwks }
    }

    fn model_token(org_id: &str, user_id: &str, zdr: bool) -> String {
        let now = token_now();
        encode_test_token(&serde_json::json!({
            "sub": user_id,
            "iss": TEST_AUTH_ISSUER,
            "aud": "model-gateway",
            "exp": now + 300,
            "nbf": now - 5,
            "org_id": org_id,
            "user_id": user_id,
            "principal_type": "user",
            "zdr": zdr
        }))
    }
}

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

fn encode_test_token(claims: &serde_json::Value) -> String {
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

/// A replica: its own state over the shared bus, and the router that serves it.
struct Replica {
    state: AppState,
    app: Router,
}

impl Replica {
    fn on(hub: &LoopbackHub) -> Self {
        let mut state = AppState::new();
        state.run_control = RunControl::loopback(hub.clone(), Duration::from_millis(200));
        // `AppState` clones share the registry, so a replica gets a registry of its own.
        state.cancels = CancelRegistry::new();
        let app = build_router(state.clone(), None);
        Self { state, app }
    }

    /// A replica with the bus off, as every replica is before the switch is turned on.
    fn without_bus() -> Self {
        let state = AppState::new();
        let app = build_router(state.clone(), None);
        Self { state, app }
    }

    /// What `invoke_stream_sse` does when it starts a stream for `request_id` producing `run_id`:
    /// register it, then make it reachable over the bus. Returns its cancel flag.
    async fn start_stream(
        &self,
        request_id: &str,
        run_id: &str,
        org_id: &str,
        user_id: &str,
    ) -> Arc<std::sync::atomic::AtomicBool> {
        let flag = self
            .state
            .cancels
            .register_run(request_id, org_id, user_id, run_id);
        self.state
            .run_control
            .attach(&self.state.cancels, request_id, run_id)
            .await;
        flag
    }

    async fn stop(
        &self,
        request_id: &str,
        org_id: &str,
        user_id: &str,
    ) -> (StatusCode, serde_json::Value) {
        let request = Request::builder()
            .method("POST")
            .uri(format!("/v1/invoke/{request_id}/cancel"))
            .header(
                AUTHORIZATION,
                format!("Bearer {}", AuthFixture::model_token(org_id, user_id, false)),
            )
            .body(Body::empty())
            .expect("request");
        let response = self.app.clone().oneshot(request).await.expect("response");
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        (status, serde_json::from_slice(&body).expect("json body"))
    }
}

fn stopped(flag: &std::sync::atomic::AtomicBool) -> bool {
    flag.load(Ordering::Relaxed)
}

#[tokio::test]
#[serial_test::serial]
async fn a_stop_on_the_other_replica_reaches_the_owner() {
    let _auth = AuthFixture::start().await;
    let hub = LoopbackHub::new();
    let (owner, other) = (Replica::on(&hub), Replica::on(&hub));
    let flag = owner.start_stream("req-1", "run-1", "org-1", "user-1").await;

    let (status, body) = other.stop("req-1", "org-1", "user-1").await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(
        body,
        serde_json::json!({ "request_id": "req-1", "cancelled": true, "via": "peer" })
    );
    assert!(stopped(&flag), "the owner's stream observes the Stop");

    // The owner itself answers without the bus, and its answer has no `via`.
    let again = owner.stop("req-1", "org-1", "user-1").await;
    assert_eq!(again.0, StatusCode::ACCEPTED);
    assert_eq!(
        again.1,
        serde_json::json!({ "request_id": "req-1", "cancelled": true })
    );
}

#[tokio::test]
#[serial_test::serial]
async fn a_foreign_caller_gets_the_same_404_as_no_stream() {
    let _auth = AuthFixture::start().await;
    let hub = LoopbackHub::new();
    let (owner, other) = (Replica::on(&hub), Replica::on(&hub));
    let flag = owner.start_stream("req-1", "run-1", "org-1", "user-1").await;

    let nothing = other.stop("req-nobody", "org-1", "user-1").await;
    for (org, user) in [("org-1", "user-2"), ("org-2", "user-1")] {
        let (status, body) = other.stop("req-1", org, user).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{org}/{user}");
        assert_eq!(
            body["error"], "no active stream",
            "the body must not tell the stream exists"
        );
        assert_eq!(
            (status, body["error"].clone(), body["cancelled"].clone()),
            (nothing.0, nothing.1["error"].clone(), nothing.1["cancelled"].clone())
        );
    }
    assert!(!stopped(&flag), "a stranger's Stop never reaches the stream");
}

#[tokio::test]
#[serial_test::serial]
async fn a_stop_for_a_stream_nobody_owns_is_a_404() {
    let _auth = AuthFixture::start().await;
    let hub = LoopbackHub::new();
    let replica = Replica::on(&hub);
    let (status, body) = replica.stop("req-gone", "org-1", "user-1").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        body,
        serde_json::json!({
            "request_id": "req-gone", "cancelled": false, "error": "no active stream"
        })
    );
}

#[tokio::test]
#[serial_test::serial]
async fn an_id_that_is_not_a_subject_token_is_a_404_not_a_subject_injection() {
    let _auth = AuthFixture::start().await;
    let hub = LoopbackHub::new();
    let (owner, other) = (Replica::on(&hub), Replica::on(&hub));
    let flag = owner.start_stream("req-1", "run-1", "org-1", "user-1").await;
    // `req-1.cancel` would address the owner's own subject if it were spliced in unchecked, and
    // a wildcard would address every stream.
    for id in ["req-1.cancel", "%2A", "req-1%20x"] {
        let (status, _) = other.stop(id, "org-1", "user-1").await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{id}");
    }
    assert!(!stopped(&flag));
}

#[tokio::test]
#[serial_test::serial]
async fn an_owner_that_stops_answering_is_a_503_not_a_fake_202_or_404() {
    let _auth = AuthFixture::start().await;
    let hub = LoopbackHub::new();
    let (owner, other) = (Replica::on(&hub), Replica::on(&hub));
    let flag = owner.start_stream("req-1", "run-1", "org-1", "user-1").await;

    // The owner died or hung after subscribing: its subscription is still on the bus.
    hub.mute(true);
    let (status, body) = other.stop("req-1", "org-1", "user-1").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        body,
        serde_json::json!({
            "request_id": "req-1",
            "cancelled": false,
            "error": "run_control_unavailable",
            "retryable": true,
        })
    );
    assert!(!stopped(&flag));

    // A later retry succeeds once the owner answers again.
    hub.mute(false);
    assert_eq!(other.stop("req-1", "org-1", "user-1").await.0, StatusCode::ACCEPTED);
    assert!(stopped(&flag));
}

#[tokio::test]
#[serial_test::serial]
async fn a_finished_stream_leaves_nothing_subscribed() {
    let _auth = AuthFixture::start().await;
    let hub = LoopbackHub::new();
    let (owner, other) = (Replica::on(&hub), Replica::on(&hub));
    owner.start_stream("req-1", "run-1", "org-1", "user-1").await;
    assert_eq!(hub.listener_count(), 1, "one listener covers both subjects");

    owner.state.cancels.finish("req-1");
    assert_eq!(hub.listener_count(), 0);
    assert_eq!(
        other.stop("req-1", "org-1", "user-1").await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        other
            .state
            .run_control
            .cancel_run(&other.state.cancels, "run-1", "org-1", "user-1")
            .await,
        CancelRoute::NoOwner
    );
}

#[tokio::test]
#[serial_test::serial]
async fn a_durable_cancel_of_the_run_stops_the_stream_on_the_other_replica() {
    let _auth = AuthFixture::start().await;
    let hub = LoopbackHub::new();
    let (owner, other) = (Replica::on(&hub), Replica::on(&hub));
    let flag = owner.start_stream("req-1", "run-1", "org-1", "user-1").await;
    let unrelated = owner.start_stream("req-3", "run-3", "org-1", "user-1").await;

    let control = &other.state.run_control;
    let cancels = &other.state.cancels;
    for (org, user) in [("org-1", "user-2"), ("org-2", "user-1")] {
        assert_eq!(
            control.cancel_run(cancels, "run-1", org, user).await,
            CancelRoute::NoOwner,
            "{org}/{user}: another caller's Stop is the same answer as no stream"
        );
    }
    assert!(!stopped(&flag) && !stopped(&unrelated));

    assert_eq!(
        control.cancel_run(cancels, "run-1", "org-1", "user-1").await,
        CancelRoute::Peer
    );
    assert!(stopped(&flag), "the stream of the cancelled run stops");
    assert!(!stopped(&unrelated));

    // On the owner itself the local registry answers and the bus is not used.
    assert_eq!(
        owner
            .state
            .run_control
            .cancel_run(&owner.state.cancels, "run-3", "org-1", "user-1")
            .await,
        CancelRoute::Local
    );
    assert!(stopped(&unrelated));
}

#[tokio::test]
#[serial_test::serial]
async fn with_the_bus_off_nothing_subscribes_and_a_remote_stop_is_the_old_404() {
    let _auth = AuthFixture::start().await;
    let hub = LoopbackHub::new();
    let owner = Replica::without_bus();
    let flag = owner.start_stream("req-1", "run-1", "org-1", "user-1").await;
    assert_eq!(hub.listener_count(), 0);

    // Replica B has the hub, but A never subscribed, so B finds no owner.
    let other = Replica::on(&hub);
    assert_eq!(
        other.stop("req-1", "org-1", "user-1").await.0,
        StatusCode::NOT_FOUND
    );
    assert!(!stopped(&flag));

    // A replica with the bus off answers from its own registry only.
    assert_eq!(owner.stop("req-1", "org-1", "user-1").await.0, StatusCode::ACCEPTED);
    assert!(stopped(&flag));
    assert_eq!(
        owner.stop("req-nobody", "org-1", "user-1").await.0,
        StatusCode::NOT_FOUND
    );
}

/// A ZDR turn leaves `invoke_stream_sse` for its direct stream before the stream registers, so it
/// never reaches the one `attach` call-in; a ZDR caller's Stop never goes on the bus either.
#[tokio::test]
#[serial_test::serial]
async fn zdr_turn_registers_no_run_control_listener() {
    let _auth = AuthFixture::start().await;
    let hub = LoopbackHub::new();
    let replica = Replica::on(&hub);

    let request = Request::builder()
        .method("POST")
        .uri("/v1/invoke/stream")
        .header(
            AUTHORIZATION,
            format!("Bearer {}", AuthFixture::model_token("org-1", "user-1", true)),
        )
        .header("content-type", "application/json")
        .body(Body::from(r#"{"content":"hi","model":"m"}"#))
        .expect("request");
    let response = replica.app.clone().oneshot(request).await.expect("response");
    // Whatever the offline downstreams make of it, drain the stream to its end.
    let _ = to_bytes(response.into_body(), usize::MAX).await;
    assert_eq!(hub.listener_count(), 0, "a ZDR stream subscribes to nothing");
    assert_eq!(replica.state.cancels.active(), 0, "and registers nothing");

    // A ZDR caller's Stop for a stream that IS live on another replica is a plain 404: it is
    // answered here and puts nothing on the bus.
    let owner = Replica::on(&hub);
    let flag = owner.start_stream("req-1", "run-1", "org-1", "user-1").await;
    let request = Request::builder()
        .method("POST")
        .uri("/v1/invoke/req-1/cancel")
        .header(
            AUTHORIZATION,
            format!("Bearer {}", AuthFixture::model_token("org-1", "user-1", true)),
        )
        .body(Body::empty())
        .expect("request");
    let response = replica.app.clone().oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(!stopped(&flag));
}

/// The one `attach` call-in sits after the ZDR direct stream has returned, so the source order
/// itself is a guard: a refactor that moves it above the ZDR branch fails here.
#[test]
fn the_attach_call_in_follows_the_zdr_direct_stream_return() {
    let sse = include_str!("../src/sse.rs");
    let zdr_return = sse
        .find("return zdr_direct_stream(")
        .expect("the ZDR direct stream return");
    let attach = sse
        .find("state.run_control.attach(")
        .expect("the run-control call-in");
    assert!(zdr_return < attach, "attach must come after the ZDR branch returns");
    assert_eq!(
        sse.matches("run_control.attach(").count(),
        1,
        "exactly one call-in"
    );
}
