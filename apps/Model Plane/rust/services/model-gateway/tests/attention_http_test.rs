//! The attention routes of model-gateway (WS16 S1): `GET /v1/attention` and
//! `POST /v1/threads/{thread_id}/seen`. Session Core is a double here, so each test states what the
//! owner's threads look like and what Session Core answers, and checks what the gateway sent.

use axum::{
    body::{to_bytes, Body},
    http::{header::AUTHORIZATION, Request, StatusCode},
    Router,
};
use model_gateway::{http_routes::build_router, state::AppState};
use mp_contracts::model_plane::v1::{
    attention_service_client::AttentionServiceClient,
    attention_service_server::{AttentionService, AttentionServiceServer},
    AttentionCounts, AttentionItem, AttentionReason, AttentionState, ListAttentionRequest,
    ListAttentionResponse, MarkThreadSeenRequest, MarkThreadSeenResponse, StreamAttentionRequest,
    StreamAttentionResponse,
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{
    transport::{Channel, Endpoint, Server},
    Request as TonicRequest, Response, Status,
};
use tower::ServiceExt;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

#[path = "support/offline_downstreams.rs"]
mod offline_downstreams;

const TEST_AUTH_KID: &str = "attention-http-test";
const TEST_AUTH_ISSUER: &str = "https://auth.test/model";

/// Session Core's `AttentionService` as the gateway sees it. Each call is recorded so a test can
/// check what the gateway sent, and the answers are the ones the test sets.
#[derive(Clone, Default)]
struct AttentionDouble {
    /// What `ListAttention` answers. The default says the attention switch is off.
    list_answer: ListAttentionResponse,
    /// The refusal `MarkThreadSeen` answers with, when set.
    mark_refusal: Option<(tonic::Code, &'static str)>,
    list_calls: Arc<Mutex<Vec<ListAttentionRequest>>>,
    mark_calls: Arc<Mutex<Vec<MarkThreadSeenRequest>>>,
}

#[tonic::async_trait]
impl AttentionService for AttentionDouble {
    type StreamAttentionStream = futures::stream::Empty<Result<StreamAttentionResponse, Status>>;

    async fn list_attention(
        &self,
        request: TonicRequest<ListAttentionRequest>,
    ) -> Result<Response<ListAttentionResponse>, Status> {
        self.list_calls.lock().unwrap().push(request.into_inner());
        Ok(Response::new(self.list_answer.clone()))
    }

    async fn mark_thread_seen(
        &self,
        request: TonicRequest<MarkThreadSeenRequest>,
    ) -> Result<Response<MarkThreadSeenResponse>, Status> {
        let request = request.into_inner();
        let thread_id = request.thread_id.clone();
        self.mark_calls.lock().unwrap().push(request);
        if let Some((code, message)) = self.mark_refusal {
            return Err(Status::new(code, message));
        }
        Ok(Response::new(MarkThreadSeenResponse { thread_id }))
    }

    async fn stream_attention(
        &self,
        _request: TonicRequest<StreamAttentionRequest>,
    ) -> Result<Response<Self::StreamAttentionStream>, Status> {
        Err(Status::unimplemented(
            "the attention stream is served from WS16 S5",
        ))
    }
}

async fn spawn_attention_double(double: AttentionDouble) -> AttentionServiceClient<Channel> {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        Server::builder()
            .add_service(AttentionServiceServer::new(double))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .ok();
    });
    let channel = Endpoint::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .unwrap();
    AttentionServiceClient::new(channel)
}

/// An `AppState` whose every downstream is offline except Session Core's attention service.
fn make_state(attention: AttentionServiceClient<Channel>) -> AppState {
    let mut state = offline_downstreams::offline_state();
    state.attention_client = attention;
    state
}

/// Serves the JWKS the gateway verifies against and points the gateway at it. The fixture keeps
/// the mock alive for the length of the test.
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
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "keys": [{
                    "kty": "RSA",
                    "use": "sig",
                    "alg": "RS256",
                    "kid": TEST_AUTH_KID,
                    "n": n,
                    "e": e
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
        std::env::set_var("SESSION_CORE_AUTH_AUDIENCE", "session-core");
        Self { _jwks: jwks }
    }
}

/// The bearers one user sends: the ingress token (`aud=model-gateway`) and the Session Core token
/// the gateway forwards (`aud=session-core`).
fn user_tokens(org_id: &str, user_id: &str) -> (String, String) {
    let now = token_now();
    let claims = |audience: &str| {
        json!({
            "sub": user_id,
            "iss": TEST_AUTH_ISSUER,
            "aud": audience,
            "exp": now + 300,
            "nbf": now - 5,
            "org_id": org_id,
            "user_id": user_id,
            "principal_type": "user",
            "zdr": false
        })
    };
    (
        encode_test_token(&claims("model-gateway")),
        encode_test_token(&claims("session-core")),
    )
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

/// Sends one request to the router as the caller whose bearers are given.
async fn send(
    app: Router,
    verb: &str,
    uri: &str,
    model_bearer: &str,
    session_bearer: Option<&str>,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(verb)
        .uri(uri)
        .header(AUTHORIZATION, format!("Bearer {model_bearer}"));
    if let Some(session) = session_bearer {
        request = request.header("x-session-authorization", format!("Bearer {session}"));
    }
    let response = app
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

#[tokio::test]
#[serial_test::serial]
async fn list_relays_the_closed_state_and_scopes_the_caller() {
    let _auth = AuthFixture::start().await;
    let (model, session) = user_tokens("org-1", "user-1");
    let double = AttentionDouble {
        list_answer: ListAttentionResponse {
            enabled: true,
            items: vec![
                AttentionItem {
                    thread_id: "thread-approval".into(),
                    state: AttentionState::NeedsApproval as i32,
                    reason: AttentionReason::Approval as i32,
                    since: Some(prost_types::Timestamp {
                        seconds: 1_800_000_000,
                        nanos: 0,
                    }),
                    unseen: false,
                    run_id: "run-9".into(),
                },
                AttentionItem {
                    thread_id: "thread-working".into(),
                    state: AttentionState::Working as i32,
                    reason: AttentionReason::Unspecified as i32,
                    since: None,
                    unseen: true,
                    run_id: String::new(),
                },
                AttentionItem {
                    thread_id: "thread-unnamed".into(),
                    state: 99,
                    reason: 99,
                    since: None,
                    unseen: false,
                    run_id: String::new(),
                },
            ],
            counts: Some(AttentionCounts {
                needs_you: 1,
                working: 1,
                unseen: 1,
            }),
        },
        ..AttentionDouble::default()
    };
    let calls = double.clone();
    let app = build_router(make_state(spawn_attention_double(double).await), None);

    let (status, body) = send(
        app,
        "GET",
        "/v1/attention?limit=5&user_id=user-2",
        &model,
        Some(session.as_str()),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["enabled"], true);
    assert_eq!(
        body["counts"],
        json!({"needs_you": 1, "working": 1, "unseen": 1})
    );
    let items = body["items"].as_array().expect("items array");
    assert_eq!(items.len(), 3);
    assert_eq!(items[0]["thread_id"], "thread-approval");
    assert_eq!(items[0]["state"], "needs_approval");
    assert_eq!(items[0]["reason"], "approval");
    assert_eq!(items[0]["run_id"], "run-9");
    assert_eq!(items[0]["unseen"], false);
    assert!(items[0]["since"]
        .as_str()
        .unwrap()
        .starts_with("2027-01-15T08:00:00"));
    assert_eq!(items[1]["state"], "working");
    assert_eq!(items[1]["unseen"], true);
    assert!(
        items[1].get("reason").is_none(),
        "an unspecified reason is omitted"
    );
    assert!(items[1].get("since").is_none());
    assert!(items[1].get("run_id").is_none());
    assert!(
        items[2].get("state").is_none(),
        "a state this build does not name is omitted"
    );
    assert!(items[2].get("reason").is_none());

    let listed = calls.list_calls.lock().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].org_id, "org-1");
    assert_eq!(
        listed[0].user_id, "user-1",
        "the owner comes from the claims, not the query"
    );
    assert_eq!(listed[0].limit, 5);
}

#[tokio::test]
#[serial_test::serial]
async fn list_says_so_while_the_attention_switch_is_off() {
    let _auth = AuthFixture::start().await;
    let (model, session) = user_tokens("org-1", "user-1");
    let double = AttentionDouble::default();
    let calls = double.clone();
    let app = build_router(make_state(spawn_attention_double(double).await), None);

    let (status, body) = send(app, "GET", "/v1/attention", &model, Some(session.as_str())).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!({
            "enabled": false,
            "items": [],
            "counts": {"needs_you": 0, "working": 0, "unseen": 0}
        })
    );
    let listed = calls.list_calls.lock().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(
        listed[0].limit, 0,
        "an absent limit reaches Session Core as zero, which reads as 200"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn mark_seen_marks_the_callers_thread_read() {
    let _auth = AuthFixture::start().await;
    let (model, session) = user_tokens("org-1", "user-1");
    let double = AttentionDouble::default();
    let calls = double.clone();
    let app = build_router(make_state(spawn_attention_double(double).await), None);

    let (status, body) = send(
        app,
        "POST",
        "/v1/threads/thread-7/seen",
        &model,
        Some(session.as_str()),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"thread_id": "thread-7"}));
    let marked = calls.mark_calls.lock().unwrap();
    assert_eq!(marked.len(), 1);
    assert_eq!(marked[0].org_id, "org-1");
    assert_eq!(marked[0].user_id, "user-1");
    assert_eq!(marked[0].thread_id, "thread-7");
}

#[tokio::test]
#[serial_test::serial]
async fn mark_seen_relays_what_session_core_refuses() {
    let _auth = AuthFixture::start().await;
    let (model, session) = user_tokens("org-1", "user-1");
    let cases = [
        (
            tonic::Code::FailedPrecondition,
            "attention_disabled",
            StatusCode::PRECONDITION_FAILED,
            "thread_persistence_not_permitted",
        ),
        (
            tonic::Code::FailedPrecondition,
            "zero-retention caller keeps its read state on the client",
            StatusCode::PRECONDITION_FAILED,
            "thread_persistence_not_permitted",
        ),
        (
            tonic::Code::PermissionDenied,
            "not the thread owner",
            StatusCode::FORBIDDEN,
            "forbidden",
        ),
        (
            tonic::Code::NotFound,
            "thread not found",
            StatusCode::NOT_FOUND,
            "thread_not_found",
        ),
    ];
    for (code, message, expected, error_code) in cases {
        let double = AttentionDouble {
            mark_refusal: Some((code, message)),
            ..AttentionDouble::default()
        };
        let app = build_router(make_state(spawn_attention_double(double).await), None);

        let (status, body) = send(
            app,
            "POST",
            "/v1/threads/thread-7/seen",
            &model,
            Some(session.as_str()),
        )
        .await;

        assert_eq!(status, expected, "{message}");
        assert_eq!(body["error"]["code"], error_code, "{message}");
        assert_eq!(body["error"]["message"], message, "{message}");
    }
}

#[tokio::test]
#[serial_test::serial]
async fn blank_thread_id_is_refused_before_session_core() {
    let _auth = AuthFixture::start().await;
    let (model, session) = user_tokens("org-1", "user-1");
    let double = AttentionDouble::default();
    let calls = double.clone();
    let app = build_router(make_state(spawn_attention_double(double).await), None);

    let (status, _) = send(
        app,
        "POST",
        "/v1/threads/%20/seen",
        &model,
        Some(session.as_str()),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(calls.mark_calls.lock().unwrap().is_empty());
}

#[tokio::test]
#[serial_test::serial]
async fn attention_routes_need_the_session_credential() {
    let _auth = AuthFixture::start().await;
    let (model, _session) = user_tokens("org-1", "user-1");
    let double = AttentionDouble::default();
    let calls = double.clone();
    let app = build_router(make_state(spawn_attention_double(double).await), None);

    let (list_status, _) = send(app.clone(), "GET", "/v1/attention", &model, None).await;
    let (mark_status, _) = send(app, "POST", "/v1/threads/thread-7/seen", &model, None).await;

    assert_eq!(list_status, StatusCode::UNAUTHORIZED);
    assert_eq!(mark_status, StatusCode::UNAUTHORIZED);
    assert!(calls.list_calls.lock().unwrap().is_empty());
    assert!(calls.mark_calls.lock().unwrap().is_empty());
}
