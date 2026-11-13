//! F-f06-19 (HTTP route side): under `ORG_AGENT_POLICY_MODE=enforce` a stored run is neither continued nor resumed
//! for an org whose agent runs are paused, whose mode cap is lockdown, or whose policy cannot be read. Each of the
//! four routes (`/continue` with an approval, its question form, its park form, and `POST .../resume`) answers the
//! org-policy refusal through the real router, after the owner check and before the run is read or the execution is
//! asked to resume. An org with an `auto` cap passes the policy and reaches the run read, so the refusal is the
//! policy's and not a missing double.
//!
//! This binary sets its own switches (policy mode enforce, ask-user resume, loop resume, loop parking) before the
//! first request, so no other test binary sees them.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use axum::{
    body::{to_bytes, Body},
    http::{header::AUTHORIZATION, Request, StatusCode},
    Router,
};
use model_gateway::http_routes::build_router;
use mp_contracts::model_plane::v1::{
    run_service_client::RunServiceClient,
    run_service_server::{RunService, RunServiceServer},
    CancelRunRequest, CancelRunResponse, GetRunRequest, GetScheduledStepContextRequest,
    ListRunsRequest, ListRunsResponse, ListSystemRunsRequest, ResolveRunActionAuthorityRequest,
    ResolveRunActionAuthorityResponse, ResolveRunOwnerRequest, ResolveRunOwnerResponse,
    ResolveScheduledStepAuthorityRequest, ResolveScheduledStepAuthorityResponse,
    ResolveThreadOwnerRequest, ResolveThreadOwnerResponse, RunDetail, ScheduledStepContext,
};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{
    transport::{Endpoint, Server},
    Request as TonicRequest, Response, Status,
};
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[path = "support/offline_downstreams.rs"]
mod offline_downstreams;

const TEST_AUTH_KID: &str = "continuation-org-policy-test";
const TEST_AUTH_ISSUER: &str = "https://auth.test/model";
const POLICY_PATH: &str = "/api/v1/org-agent-policy";
const PAUSED: &str = model_gateway::org_agent_policy::AGENTS_PAUSED_CODE;
const UNAVAILABLE: &str = model_gateway::org_agent_policy::POLICY_UNAVAILABLE_CODE;

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

/// The JWKS, the audiences and this binary's switches. Every test calls it before its first request.
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
    std::env::set_var("SESSION_CORE_AUTH_AUDIENCE", "session-core");
    std::env::set_var("EXECUTION_CORE_AUTH_AUDIENCE", "execution-core");
    std::env::set_var("INFERENCE_CORE_AUTH_AUDIENCE", "inference-core");
    std::env::set_var("DATA_PLANE_AUTH_AUDIENCE", "data-plane");
    std::env::set_var("CAPABILITY_CORE_AUTH_AUDIENCE", "capability-core");
    std::env::set_var("ORG_AGENT_POLICY_MODE", "enforce");
    std::env::set_var("AGENT_ASK_USER_RESUME_ENABLED", "true");
    std::env::set_var("EXECUTION_LOOP_RESUME_ENABLED", "true");
    std::env::set_var("EXECUTION_LOOP_PARKING_ENABLED", "true");
    jwks
}

/// One user's bearers for every audience the continuation routes read, in their headers.
fn with_user_bearers(
    builder: axum::http::request::Builder,
    org_id: &str,
) -> axum::http::request::Builder {
    let now = token_now();
    let token = |audience: &str| {
        encode_test_token(&json!({
            "sub": "owner-user",
            "iss": TEST_AUTH_ISSUER,
            "aud": audience,
            "exp": now + 300,
            "nbf": now - 5,
            "org_id": org_id,
            "user_id": "owner-user",
            "principal_type": "user",
            "zdr": false
        }))
    };
    builder
        .header(AUTHORIZATION, format!("Bearer {}", token("model-gateway")))
        .header(
            "x-session-authorization",
            format!("Bearer {}", token("session-core")),
        )
        .header(
            "x-inference-authorization",
            format!("Bearer {}", token("inference-core")),
        )
        .header(
            "x-execution-authorization",
            format!("Bearer {}", token("execution-core")),
        )
        .header(
            "x-data-plane-authorization",
            format!("Bearer {}", token("data-plane")),
        )
        .header(
            "x-capability-authorization",
            format!("Bearer {}", token("capability-core")),
        )
}

/// A run-service double: every run is the caller's own, and each `GetRun` (the first read after the policy) is
/// counted and answered as a running run.
#[derive(Clone, Default)]
struct OwnedRuns {
    get_run_calls: Arc<AtomicUsize>,
}

#[tonic::async_trait]
impl RunService for OwnedRuns {
    async fn resolve_approval_authority(
        &self,
        _: TonicRequest<mp_contracts::model_plane::v1::ResolveApprovalAuthorityRequest>,
    ) -> Result<Response<mp_contracts::model_plane::v1::ResolveApprovalAuthorityResponse>, Status>
    {
        Err(Status::unimplemented("not served by this double"))
    }

    async fn get_run(&self, _: TonicRequest<GetRunRequest>) -> Result<Response<RunDetail>, Status> {
        self.get_run_calls.fetch_add(1, Ordering::SeqCst);
        Err(Status::unimplemented("the run read is only counted here"))
    }

    async fn list_runs(
        &self,
        _: TonicRequest<ListRunsRequest>,
    ) -> Result<Response<ListRunsResponse>, Status> {
        Err(Status::unimplemented("not served by this double"))
    }

    async fn list_system_runs(
        &self,
        _: TonicRequest<ListSystemRunsRequest>,
    ) -> Result<Response<ListRunsResponse>, Status> {
        Err(Status::unimplemented("not served by this double"))
    }

    async fn cancel_run(
        &self,
        _: TonicRequest<CancelRunRequest>,
    ) -> Result<Response<CancelRunResponse>, Status> {
        Err(Status::unimplemented("not served by this double"))
    }

    async fn resolve_run_owner(
        &self,
        _: TonicRequest<ResolveRunOwnerRequest>,
    ) -> Result<Response<ResolveRunOwnerResponse>, Status> {
        Ok(Response::new(ResolveRunOwnerResponse { authorized: true }))
    }

    async fn resolve_thread_owner(
        &self,
        _: TonicRequest<ResolveThreadOwnerRequest>,
    ) -> Result<Response<ResolveThreadOwnerResponse>, Status> {
        Err(Status::unimplemented("not served by this double"))
    }

    async fn resolve_run_action_authority(
        &self,
        _: TonicRequest<ResolveRunActionAuthorityRequest>,
    ) -> Result<Response<ResolveRunActionAuthorityResponse>, Status> {
        Err(Status::unimplemented("not served by this double"))
    }

    async fn resolve_scheduled_step_authority(
        &self,
        _: TonicRequest<ResolveScheduledStepAuthorityRequest>,
    ) -> Result<Response<ResolveScheduledStepAuthorityResponse>, Status> {
        Err(Status::unimplemented("not served by this double"))
    }

    async fn authorize_room_run_attach(
        &self,
        _: TonicRequest<mp_contracts::model_plane::v1::AuthorizeRoomRunAttachRequest>,
    ) -> Result<Response<mp_contracts::model_plane::v1::AuthorizeRoomRunAttachResponse>, Status>
    {
        Err(Status::unimplemented("not served by this double"))
    }

    async fn cancel_run_as_room_manager(
        &self,
        _: TonicRequest<mp_contracts::model_plane::v1::CancelRunAsRoomManagerRequest>,
    ) -> Result<Response<mp_contracts::model_plane::v1::CancelRunAsRoomManagerResponse>, Status>
    {
        Err(Status::unimplemented("not served by this double"))
    }

    async fn get_scheduled_step_context(
        &self,
        _: TonicRequest<GetScheduledStepContextRequest>,
    ) -> Result<Response<ScheduledStepContext>, Status> {
        Err(Status::unimplemented("not served by this double"))
    }
}

async fn spawn_runs(runs: OwnedRuns) -> RunServiceClient<tonic::transport::Channel> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind run double");
    let addr = listener.local_addr().expect("run double addr");
    tokio::spawn(async move {
        Server::builder()
            .add_service(RunServiceServer::new(runs))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .ok();
    });
    let channel = Endpoint::from_shared(format!("http://{addr}"))
        .expect("run double endpoint")
        .connect()
        .await
        .expect("connect run double");
    RunServiceClient::new(channel)
}

/// capability-core's org-agent-policy read: `settings` as the document, or `status` with no body.
async fn policy_registry(org: &str, answer: Result<Value, u16>) -> MockServer {
    let server = MockServer::start().await;
    let response = match answer {
        Ok(settings) => ResponseTemplate::new(200).set_body_json(json!({
            "org_id": org, "revision": 1, "digest": "sha256:1", "settings": settings
        })),
        Err(status) => ResponseTemplate::new(status),
    };
    Mock::given(method("GET"))
        .and(path(POLICY_PATH))
        .respond_with(response)
        .mount(&server)
        .await;
    server
}

/// The router over the run double and the policy registry; every other downstream is offline.
async fn router(policy: &MockServer) -> (Router, OwnedRuns) {
    let runs = OwnedRuns::default();
    let mut state = offline_downstreams::offline_state();
    state.run_client = spawn_runs(runs.clone()).await;
    state.capability_core_base_url = policy.uri();
    (build_router(state, None), runs)
}

/// The four continuation routes of one run, as `(name, method, uri, body)`.
fn routes(run_id: &str) -> Vec<(&'static str, &'static str, String, Option<Value>)> {
    let continue_uri = format!("/v1/orchestration/runs/{run_id}/continue");
    vec![
        (
            "continue (approval)",
            "POST",
            continue_uri.clone(),
            Some(json!({ "approval_id": "appr-1", "claim_token": "claim-1" })),
        ),
        (
            "continue (question form)",
            "POST",
            continue_uri.clone(),
            Some(json!({
                "approval_id": "",
                "claim_token": "claim-1",
                "question_id": "q_1",
                "answers": [{ "item_id": "item-1", "option_ids": ["opt-1"] }]
            })),
        ),
        (
            "continue (park form)",
            "POST",
            continue_uri,
            Some(json!({ "approval_id": "", "claim_token": "claim-1" })),
        ),
        (
            "resume",
            "POST",
            format!("/v1/orchestration/runs/{run_id}/resume"),
            None,
        ),
    ]
}

async fn call(
    app: &Router,
    org: &str,
    http_method: &str,
    uri: &str,
    body: Option<&Value>,
) -> (StatusCode, Value) {
    let builder = with_user_bearers(Request::builder().method(http_method).uri(uri), org)
        .header("content-type", "application/json");
    let request = builder
        .body(body.map_or_else(Body::empty, |body| Body::from(body.to_string())))
        .expect("request");
    let response = app.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// Each of the four routes, for an org whose policy answers `answer`, is refused with `code` at `status` and reads
/// no run.
async fn assert_refused(
    org_prefix: &str,
    answer: Result<Value, u16>,
    status: StatusCode,
    code: &str,
) {
    for (index, (name, http_method, uri, body)) in routes("run-1").into_iter().enumerate() {
        // One org per route: the policy cache is per org and per process.
        let org = format!("{org_prefix}-{index}");
        let registry = policy_registry(&org, answer.clone()).await;
        let (app, runs) = router(&registry).await;
        let (got_status, got) = call(&app, &org, http_method, &uri, body.as_ref()).await;
        assert_eq!(got_status, status, "{name} for {org}: {got}");
        assert_eq!(got["error"]["code"], code, "{name} for {org}: {got}");
        assert_eq!(
            runs.get_run_calls.load(Ordering::SeqCst),
            0,
            "{name} for {org} read the run after the policy refused it"
        );
        assert!(
            !registry
                .received_requests()
                .await
                .unwrap_or_default()
                .is_empty(),
            "{name} for {org} was refused without reading the policy"
        );
    }
}

#[tokio::test]
#[serial_test::serial]
async fn a_paused_org_continues_and_resumes_no_run() {
    let _jwks = start_auth().await;
    assert_refused(
        "org-paused",
        Ok(json!({ "agents.paused": true, "mode_cap": "auto" })),
        StatusCode::FORBIDDEN,
        PAUSED,
    )
    .await;
}

#[tokio::test]
#[serial_test::serial]
async fn a_lockdown_cap_continues_and_resumes_no_run() {
    let _jwks = start_auth().await;
    assert_refused(
        "org-lockdown",
        Ok(json!({ "mode_cap": "lockdown" })),
        StatusCode::FORBIDDEN,
        PAUSED,
    )
    .await;
}

#[tokio::test]
#[serial_test::serial]
async fn an_unreadable_policy_continues_and_resumes_no_run() {
    let _jwks = start_auth().await;
    assert_refused(
        "org-unreadable",
        Err(503),
        StatusCode::SERVICE_UNAVAILABLE,
        UNAVAILABLE,
    )
    .await;
}

/// The contrast: an `auto` org passes the policy, and each route goes on to read the run (the double answers that
/// read with an error, so nothing is launched).
#[tokio::test]
#[serial_test::serial]
async fn an_auto_org_passes_the_policy_and_reaches_the_run() {
    let _jwks = start_auth().await;
    for (index, (name, http_method, uri, body)) in routes("run-1").into_iter().enumerate() {
        let org = format!("org-auto-{index}");
        let registry = policy_registry(&org, Ok(json!({ "mode_cap": "auto" }))).await;
        let (app, runs) = router(&registry).await;
        let (status, got) = call(&app, &org, http_method, &uri, body.as_ref()).await;
        assert_ne!(got["error"]["code"], PAUSED, "{name}: {status} {got}");
        assert_ne!(got["error"]["code"], UNAVAILABLE, "{name}: {status} {got}");
        if name == "resume" {
            // The resume reads no run: it asks the (offline) execution to resume, so the policy let it through.
            assert_ne!(status, StatusCode::FORBIDDEN, "{name}: {got}");
        } else {
            assert_eq!(
                runs.get_run_calls.load(Ordering::SeqCst),
                1,
                "{name} did not reach the run read: {status} {got}"
            );
        }
    }
}
