//! Two gateway replicas over one run-control bus (build plan WS04-S2, WS04-S3).
//!
//! Each "replica" is its own `AppState` (its own cancel and queued-input registries) built over
//! one `LoopbackHub`, so a Stop, a queued message or a liveness probe on the replica that does not
//! own a stream has to cross the bus to reach it, exactly as it does across two containers on
//! NATS. The loopback hub carries the messages in their wire encoding.

use axum::{
    body::{to_bytes, Body},
    http::{header::AUTHORIZATION, Request, StatusCode},
    Router,
};
use model_gateway::{
    cancel_registry::CancelRegistry,
    http_routes::build_router,
    queued_input::EnqueueOutcome,
    run_control::{
        CancelRoute, LoopbackHub, QueueRoute, RunControl, RunControlMsg, RunControlOp,
    },
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
        Self::mint("model-gateway", org_id, user_id, zdr, false)
    }

    /// The ingress token and the delegated session-core bearer of one caller. Both carry the
    /// same retention posture, as Auth Core mints them: `waived` adds the connection-bound
    /// waiver of a turn on a user's own subscription connection.
    fn caller(org_id: &str, user_id: &str, zdr: bool, waived: bool) -> (String, String) {
        (
            Self::mint("model-gateway", org_id, user_id, zdr, waived),
            Self::mint("session-core", org_id, user_id, zdr, waived),
        )
    }

    fn mint(aud: &str, org_id: &str, user_id: &str, zdr: bool, waived: bool) -> String {
        let now = token_now();
        let mut claims = serde_json::json!({
            "sub": user_id,
            "iss": TEST_AUTH_ISSUER,
            "aud": aud,
            "exp": now + 300,
            "nbf": now - 5,
            "org_id": org_id,
            "user_id": user_id,
            "principal_type": "user",
            "zdr": zdr
        });
        if waived {
            claims["org_zdr"] = serde_json::json!(true);
            claims["zdr_waiver"] = serde_json::json!({
                "version": 1,
                "provider": "openai-codex-subscription",
                "connection": "conn-1"
            });
        }
        encode_test_token(&claims)
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

    /// What `invoke_stream_sse` does when it starts a stream for `request_id` producing `run_id`
    /// on `thread-1`: register it for Stop and for queued input, then make it reachable over the
    /// bus. Returns its cancel flag.
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
        self.state.queued_inputs.register(
            request_id,
            org_id,
            user_id,
            "thread-1",
            false,
            model_gateway::normalize::RetrievalScope::Auto,
        );
        self.state
            .run_control
            .attach(&self.state.cancels, &self.state.queued_inputs, request_id, run_id)
            .await;
        flag
    }

    /// Queue `body` for `request_id` through this replica's route, as the given caller.
    async fn queue_with(
        &self,
        request_id: &str,
        (token, session): (String, String),
        body: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        let request = Request::builder()
            .method("POST")
            .uri(format!("/v1/invoke/{request_id}/queue"))
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .header("x-session-authorization", format!("Bearer {session}"))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .expect("request");
        let response = self.app.clone().oneshot(request).await.expect("response");
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        (status, serde_json::from_slice(&body).expect("json body"))
    }

    async fn queue(
        &self,
        request_id: &str,
        org_id: &str,
        user_id: &str,
        content: &str,
    ) -> (StatusCode, serde_json::Value) {
        self.queue_with(
            request_id,
            AuthFixture::caller(org_id, user_id, false, false),
            serde_json::json!({ "content": content }),
        )
        .await
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

fn drained(replica: &Replica, request_id: &str) -> Vec<String> {
    replica.state.queued_inputs.drain(request_id)
}

#[tokio::test]
#[serial_test::serial]
async fn a_message_queued_on_the_other_replica_is_delivered_through_the_owners_drain() {
    let _auth = AuthFixture::start().await;
    let hub = LoopbackHub::new();
    let (owner, other) = (Replica::on(&hub), Replica::on(&hub));
    owner.start_stream("req-1", "run-1", "org-1", "user-1").await;

    let (status, body) = other.queue("req-1", "org-1", "user-1", "  use EUR  ").await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(body["queued"], true);
    assert_eq!(body["pending"], 1);
    assert_eq!(body["request_id"], "req-1");

    // The owner's loop takes it at its next round boundary, exactly as for a local message; the
    // replica that was asked holds nothing.
    assert!(drained(&other, "req-1").is_empty());
    assert_eq!(drained(&owner, "req-1"), ["use EUR"]);

    // The owner itself answers from its own registry.
    let (status, body) = owner.queue("req-1", "org-1", "user-1", "and NOK").await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(drained(&owner, "req-1"), ["and NOK"]);
}

#[tokio::test]
#[serial_test::serial]
async fn the_bounds_are_the_owners_whichever_replica_is_asked() {
    let _auth = AuthFixture::start().await;
    let hub = LoopbackHub::new();
    let (owner, other) = (Replica::on(&hub), Replica::on(&hub));
    owner.start_stream("req-1", "run-1", "org-1", "user-1").await;

    // Empty and too long are refused with the answers the owner gives, and queue nothing.
    let (status, body) = other.queue("req-1", "org-1", "user-1", "   ").await;
    assert_eq!((status, body["error"].as_str()), (StatusCode::BAD_REQUEST, Some("content is required")));
    let long = "x".repeat(model_gateway::queued_input::MAX_QUEUED_CHARS + 1);
    let (status, body) = other.queue("req-1", "org-1", "user-1", &long).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["chars"], model_gateway::queued_input::MAX_QUEUED_CHARS + 1);
    assert!(drained(&owner, "req-1").is_empty());

    for n in 0..model_gateway::queued_input::MAX_QUEUED_MESSAGES {
        let (status, body) = other.queue("req-1", "org-1", "user-1", &format!("m{n}")).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        assert_eq!(body["pending"], n + 1);
    }
    let (status, body) = other.queue("req-1", "org-1", "user-1", "one too many").await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["pending"], model_gateway::queued_input::MAX_QUEUED_MESSAGES);
    assert_eq!(drained(&owner, "req-1"), ["m0", "m1", "m2"]);
}

#[tokio::test]
#[serial_test::serial]
async fn a_foreign_caller_or_a_stream_nobody_owns_is_the_same_404_and_queues_nothing() {
    let _auth = AuthFixture::start().await;
    let hub = LoopbackHub::new();
    let (owner, other) = (Replica::on(&hub), Replica::on(&hub));
    owner.start_stream("req-1", "run-1", "org-1", "user-1").await;

    let nothing = other.queue("req-nobody", "org-1", "user-1", "hi").await;
    assert_eq!(nothing.0, StatusCode::NOT_FOUND);
    assert_eq!(nothing.1["resend_as_new_turn"], true);
    for (org, user) in [("org-1", "user-2"), ("org-2", "user-1")] {
        let (status, body) = other.queue("req-1", org, user, "hi").await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{org}/{user}");
        assert_eq!(
            (body["error"].clone(), body["resend_as_new_turn"].clone(), body["queued"].clone()),
            (nothing.1["error"].clone(), nothing.1["resend_as_new_turn"].clone(), nothing.1["queued"].clone()),
            "the body must not tell the stream exists"
        );
    }
    assert!(drained(&owner, "req-1").is_empty());

    // An id that is not a subject token is a 404 here, never a subject injection.
    for id in ["req-1.queue", "%2A", "req-1%20x"] {
        assert_eq!(other.queue(id, "org-1", "user-1", "hi").await.0, StatusCode::NOT_FOUND, "{id}");
    }
    assert!(drained(&owner, "req-1").is_empty());
}

#[tokio::test]
#[serial_test::serial]
async fn a_claimed_thread_that_is_not_the_streams_is_a_409_and_queues_nothing() {
    let _auth = AuthFixture::start().await;
    let hub = LoopbackHub::new();
    let (owner, other) = (Replica::on(&hub), Replica::on(&hub));
    owner.start_stream("req-1", "run-1", "org-1", "user-1").await;
    let token = || AuthFixture::caller("org-1", "user-1", false, false);

    for replica in [&other, &owner] {
        let (status, body) = replica
            .queue_with("req-1", token(), serde_json::json!({ "content": "hi", "thread_id": "thread-9" }))
            .await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(body["queued"], false);
    }
    assert!(drained(&owner, "req-1").is_empty());

    let (status, body) = other
        .queue_with("req-1", token(), serde_json::json!({ "content": "hi", "thread_id": "thread-1" }))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(drained(&owner, "req-1"), ["hi"]);
}

/// A repeat of an attempt (its reply was lost, so the caller asked again) queues one message and
/// gets the first answer.
#[tokio::test]
#[serial_test::serial]
async fn a_repeated_op_id_is_one_item_across_the_bus() {
    let hub = LoopbackHub::new();
    let (owner, other) = (Replica::on(&hub), Replica::on(&hub));
    owner.start_stream("req-1", "run-1", "org-1", "user-1").await;

    let mut msg = RunControlMsg::new(RunControlOp::Queue, "org-1", "user-1");
    msg.request_id = "req-1".into();
    msg.op_id = "op-1".into();
    msg.text = Some("use EUR".into());
    let first = other.state.run_control.queue_input(&other.state.queued_inputs, &msg).await;
    let again = other.state.run_control.queue_input(&other.state.queued_inputs, &msg).await;
    assert!(matches!(first, QueueRoute::Peer(EnqueueOutcome::Queued { pending: 1, .. })), "{first:?}");
    assert_eq!(first, again);
    assert_eq!(drained(&owner, "req-1"), ["use EUR"]);

    // A different attempt with the same words is a second message.
    msg.op_id = "op-2".into();
    other.state.run_control.queue_input(&other.state.queued_inputs, &msg).await;
    assert_eq!(drained(&owner, "req-1"), ["use EUR"]);
}

#[tokio::test]
#[serial_test::serial]
async fn an_owner_that_stops_answering_a_queue_is_a_503_not_a_fake_202_or_404() {
    let _auth = AuthFixture::start().await;
    let hub = LoopbackHub::new();
    let (owner, other) = (Replica::on(&hub), Replica::on(&hub));
    owner.start_stream("req-1", "run-1", "org-1", "user-1").await;

    hub.mute(true);
    let (status, body) = other.queue("req-1", "org-1", "user-1", "hi").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        body,
        serde_json::json!({
            "request_id": "req-1",
            "queued": false,
            "error": "run_control_unavailable",
            "retryable": true,
        })
    );
    assert!(drained(&owner, "req-1").is_empty());

    hub.mute(false);
    assert_eq!(other.queue("req-1", "org-1", "user-1", "hi").await.0, StatusCode::ACCEPTED);
    assert_eq!(drained(&owner, "req-1"), ["hi"]);
}

#[tokio::test]
#[serial_test::serial]
async fn with_the_bus_off_a_message_for_another_replicas_stream_is_the_old_404() {
    let _auth = AuthFixture::start().await;
    let hub = LoopbackHub::new();
    let owner = Replica::on(&hub);
    owner.start_stream("req-1", "run-1", "org-1", "user-1").await;
    let other = Replica::without_bus();

    let (status, body) = other.queue("req-1", "org-1", "user-1", "hi").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["resend_as_new_turn"], true);
    assert!(drained(&owner, "req-1").is_empty());
}

/// Queued text changes a run's durable state, so a ZDR or a waived caller is refused before the
/// message is built, let alone sent: the owner's queue stays empty and nothing is asked of it.
#[tokio::test]
#[serial_test::serial]
async fn a_zdr_or_waived_caller_puts_nothing_on_the_bus() {
    let _auth = AuthFixture::start().await;
    let hub = LoopbackHub::new();
    let (owner, other) = (Replica::on(&hub), Replica::on(&hub));
    owner.start_stream("req-1", "run-1", "org-1", "user-1").await;
    // A bus that answers nothing turns any message that did reach it into a visible timeout.
    hub.mute(true);

    for token in [
        AuthFixture::caller("org-1", "user-1", true, false),
        AuthFixture::caller("org-1", "user-1", false, true),
    ] {
        let (status, body) = other
            .queue_with("req-1", token, serde_json::json!({ "content": "secret words" }))
            .await;
        assert!(
            status.is_client_error() && status != StatusCode::NOT_FOUND,
            "a typed refusal, not a 404 and not a 503: {status} {body}"
        );
        assert_ne!(status, StatusCode::SERVICE_UNAVAILABLE);
    }
    hub.mute(false);
    assert!(drained(&owner, "req-1").is_empty());
}

/// The resume tail's liveness is the owner's, over the bus: a stream that is producing quietly
/// on another replica keeps the tail open past the 120 s inactivity window, and the tail closes
/// once the producer is gone. Run on a paused clock.
#[tokio::test]
#[serial_test::serial]
async fn a_probe_keeps_the_resume_tail_alive_past_the_inactivity_window() {
    let _auth = AuthFixture::start().await;
    let hub = LoopbackHub::new();
    let (owner, other) = (Replica::on(&hub), Replica::on(&hub));
    owner.start_stream("req-1", "run-1", "org-1", "user-1").await;

    let resume = |replica: &Replica, user: &str| {
        let request = Request::builder()
            .method("GET")
            .uri("/v1/invoke/resume/req-1")
            .header(
                AUTHORIZATION,
                format!("Bearer {}", AuthFixture::model_token("org-1", user, false)),
            )
            .body(Body::empty())
            .expect("request");
        replica.app.clone().oneshot(request)
    };

    // Nothing is buffered and the producer is on the other replica. Without the bus the resume
    // is the old 404; another caller's resume is the same 404 even with it.
    let off = Replica::without_bus();
    assert_eq!(resume(&off, "user-1").await.expect("response").status(), StatusCode::NOT_FOUND);
    assert_eq!(resume(&other, "user-2").await.expect("response").status(), StatusCode::NOT_FOUND);

    // Warmed up with real time (token verification does real I/O); the clock is paused after.
    let response = resume(&other, "user-1").await.expect("response");
    assert_eq!(response.status(), StatusCode::OK, "the probe finds the producer");
    let tail = tokio::spawn(async move { to_bytes(response.into_body(), usize::MAX).await });
    tokio::time::pause();

    tokio::time::sleep(Duration::from_secs(130)).await;
    assert!(!tail.is_finished(), "a quiet producer on another replica is still followed after 120 s");

    owner.state.cancels.finish("req-1");
    tokio::time::sleep(Duration::from_secs(10)).await;
    assert!(tail.is_finished(), "the tail closes once the owner no longer produces");
}
