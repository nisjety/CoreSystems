//! Handler-level scenarios of the gateway's pre-consume of a continuation (build plan
//! WS03 S7c-B1): `POST /v1/orchestration/runs/{run_id}/continue` through the real router,
//! against small in-process doubles of session-core's run and orchestration services,
//! execution-core and the managed-run lifecycle.
//!
//! The doubles of `e2e_invoke_chain_test.rs` are private to that binary, so the few this
//! file needs are rebuilt here, small: the orchestration and run services are hand-rolled
//! per method (the generated traits have two dozen methods and the route calls three), the
//! execution and lifecycle doubles implement their generated traits.
//!
//! What this file pins that the unit tests in `sse.rs` cannot:
//! - the consume is made strictly before `RunAgent`, with the spend-only request;
//! - a lost consume (another claim, or the same token again) is a 409 before any stream and
//!   dispatches nothing, whatever answer shape the server gives;
//! - with the resume switch ON a refused, unreachable or ambiguous dispatch is a typed frame
//!   with `approval_spent`, never terminalized, never `agent_dispatch_*`;
//! - with the switch OFF the route is the committed one: no consume at all, the connected
//!   frame without `approval_spent`, a confirmed rejection still terminalizes, and the
//!   executor's refusals are typed frames WITHOUT `approval_spent` and without
//!   `RUN_DISPATCH_DEGRADED`;
//! - a client that disconnects while the consume is in flight still gets exactly one
//!   `RunAgent` sent;
//! - the run subject of a spent continuation is closed: an executor refusal publishes the
//!   attempt-level `RUN_CONTINUE_REFUSED` once after `RUN_STARTED` (no terminal event, no
//!   receipt, the frame unchanged), an unreachable or ambiguous outcome keeps
//!   `RUN_DISPATCH_DEGRADED`, and with the switch off every outcome publishes what it always did;
//! - `agent_run_output_unavailable`, the last error frame after the executor's answer, is final
//!   only after a spend (the typed subscription failure frame cannot be reached from this route,
//!   which launches every continuation on an unpinned route, so `sse.rs` unit tests drive it).
//!
//! The environment switches are process-global, so every test is `serial`.

use axum::{
    body::{to_bytes, Body},
    http::{
        header::{AUTHORIZATION, CONTENT_TYPE},
        Request, StatusCode,
    },
    Router,
};
use model_gateway::{http_routes::build_router, state::DynPublisher};
use mp_contracts::model_plane::v1 as mpv1;
use mp_contracts::model_plane::v1::{
    execution_core_client::ExecutionCoreClient,
    execution_core_server::{ExecutionCore, ExecutionCoreServer},
    managed_run_lifecycle_client::ManagedRunLifecycleClient,
    managed_run_lifecycle_server::{ManagedRunLifecycle, ManagedRunLifecycleServer},
    orchestration_core_service_client::OrchestrationCoreServiceClient,
    run_service_client::RunServiceClient,
};
use mp_events::publisher::InMemoryPublisher;
use serde_json::{json, Value};
use std::{
    convert::Infallible,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{
    transport::{Endpoint, Server},
    Request as TReq, Response, Status,
};
use tower::ServiceExt;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

#[path = "support/offline_downstreams.rs"]
mod offline_downstreams;

const ORG: &str = "org-1";
const USER: &str = "user-1";
const RUN: &str = "run-1";
const APPROVAL: &str = "appr-1";
const TEST_AUTH_KID: &str = "continue-preconsume-test";
const TEST_AUTH_ISSUER: &str = "https://auth.test/model";
const TERMINALIZER_TOKEN: &str = "gateway-terminalizer-token";

type HttpRequest<B> = tonic::codegen::http::Request<B>;
type HttpResponse = tonic::codegen::http::Response<tonic::body::BoxBody>;

// ---- the environment -------------------------------------------------------------

/// Sets an environment variable for the life of the guard and restores it after.
struct EnvGuard {
    name: &'static str,
    previous: Option<String>,
}

impl EnvGuard {
    fn set(name: &'static str, value: Option<&str>) -> Self {
        let previous = std::env::var(name).ok();
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
        Self { name, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var(self.name, value),
            None => std::env::remove_var(self.name),
        }
    }
}

// ---- the credentials --------------------------------------------------------------

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

fn sign(claims: &Value) -> String {
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

/// The user's ingress token and the four delegated bearers of one persistent turn, with the
/// JWKS that verifies them and the audience switches the gateway reads.
struct Auth {
    _jwks: MockServer,
    _env: Vec<EnvGuard>,
    ingress: String,
    session: String,
    execution: String,
    data_plane: String,
    inference: String,
}

async fn start_auth() -> Auth {
    use base64::Engine as _;
    use rsa::{pkcs8::DecodePublicKey, traits::PublicKeyParts};

    let public_key = rsa::RsaPublicKey::from_public_key_pem(&test_keypair().1).expect("public key");
    let n = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public_key.n().to_bytes_be());
    let e = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public_key.e().to_bytes_be());
    let jwks = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/.well-known/jwks.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "keys": [{
                "kty": "RSA", "use": "sig", "alg": "RS256",
                "kid": TEST_AUTH_KID, "n": n, "e": e
            }]
        })))
        .mount(&jwks)
        .await;
    let env = vec![
        EnvGuard::set("MODEL_GATEWAY_AUTH_DEV_BYPASS", None),
        EnvGuard::set(
            "AUTH_CORE_JWKS_URL",
            Some(&format!("{}/.well-known/jwks.json", jwks.uri())),
        ),
        EnvGuard::set("AUTH_CORE_ISSUER", Some(TEST_AUTH_ISSUER)),
        EnvGuard::set("AUTH_CORE_AUDIENCE", Some("model-gateway")),
        EnvGuard::set("SESSION_CORE_AUTH_AUDIENCE", Some("session-core")),
        EnvGuard::set("EXECUTION_CORE_AUTH_AUDIENCE", Some("execution-core")),
        EnvGuard::set("DATA_PLANE_AUTH_AUDIENCE", Some("data-plane")),
        EnvGuard::set("INFERENCE_CORE_AUTH_AUDIENCE", Some("inference-core")),
    ];
    let now = token_now();
    let claims = |audience: &str| {
        sign(&json!({
            "sub": USER, "iss": TEST_AUTH_ISSUER, "aud": audience,
            "exp": now + 300, "nbf": now - 5,
            "org_id": ORG, "user_id": USER, "principal_type": "user", "zdr": false
        }))
    };
    Auth {
        _jwks: jwks,
        _env: env,
        ingress: claims("model-gateway"),
        session: claims("session-core"),
        execution: claims("execution-core"),
        data_plane: claims("data-plane"),
        inference: claims("inference-core"),
    }
}

// ---- a tiny per-method gRPC double -------------------------------------------------

/// Answer one unary RPC with `handler`.
async fn answer<Req, Resp, B, F, Fut>(request: HttpRequest<B>, handler: F) -> HttpResponse
where
    Req: prost::Message + Default + Send + 'static,
    Resp: prost::Message + Send + 'static,
    F: FnMut(TReq<Req>) -> Fut,
    Fut: std::future::Future<Output = Result<Response<Resp>, Status>>,
    B: tonic::codegen::Body + Send + 'static,
    B::Error: Into<tonic::codegen::StdError> + Send + 'static,
{
    let mut grpc = tonic::server::Grpc::new(tonic::codec::ProstCodec::<Resp, Req>::default());
    grpc.unary(tower::service_fn(handler), request).await
}

macro_rules! double_service {
    ($double:ident, $name:literal) => {
        impl tonic::server::NamedService for $double {
            const NAME: &'static str = $name;
        }

        impl<B> tonic::codegen::Service<HttpRequest<B>> for $double
        where
            B: tonic::codegen::Body + Send + 'static,
            B::Error: Into<tonic::codegen::StdError> + Send + 'static,
        {
            type Response = HttpResponse;
            type Error = Infallible;
            type Future = tonic::codegen::BoxFuture<Self::Response, Self::Error>;

            fn poll_ready(
                &mut self,
                _cx: &mut tonic::codegen::Context<'_>,
            ) -> tonic::codegen::Poll<Result<(), Self::Error>> {
                tonic::codegen::Poll::Ready(Ok(()))
            }

            fn call(&mut self, request: HttpRequest<B>) -> Self::Future {
                let this = self.clone();
                Box::pin(async move { Ok(this.route(request).await) })
            }
        }
    };
}

/// Serve a double on a local port and connect a channel to it. A macro, so that the body
/// type the server hands its services is inferred instead of named.
macro_rules! serve {
    ($service:expr) => {{
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind a double");
        let addr = listener.local_addr().expect("double address");
        let service = $service;
        tokio::spawn(async move {
            Server::builder()
                .add_service(service)
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .ok();
        });
        Endpoint::from_shared(format!("http://{addr}"))
            .expect("endpoint")
            .connect()
            .await
            .expect("connect to a double")
    }};
}

// ---- the doubles ------------------------------------------------------------------

/// The order in which the doubles were reached: `"consume"` and `"run_agent"`.
type Order = Arc<Mutex<Vec<&'static str>>>;

/// What the double of session-core's orchestration service answers to `ConsumeApproval`.
#[derive(Clone)]
enum ConsumeBehaviour {
    /// The consume is won.
    Won,
    /// A compare-and-set by claim token, as session-core's: the first claim wins; the same
    /// token again is the replay (`consumed` false, `already_consumed` true, `same_claim`
    /// true, with the call unless omitted); another token is `already_consumed` without the
    /// call.
    SingleUse(Arc<Mutex<Option<String>>>),
    /// The consume is parked until the gate opens, and then won.
    Parked(Arc<tokio::sync::Notify>),
    /// Refused with this status.
    Refused(Status),
}

type SeenConsume = (Option<String>, mpv1::ConsumeApprovalRequest);

#[derive(Clone)]
struct Orchestration {
    order: Order,
    behaviour: ConsumeBehaviour,
    consumes: Arc<Mutex<Vec<SeenConsume>>>,
    /// How many times `GetApproval` was asked (a park continue never asks).
    approval_reads: Arc<Mutex<usize>>,
}

double_service!(Orchestration, "model_plane.v1.OrchestrationCoreService");

impl Orchestration {
    async fn route<B>(&self, request: HttpRequest<B>) -> HttpResponse
    where
        B: tonic::codegen::Body + Send + 'static,
        B::Error: Into<tonic::codegen::StdError> + Send + 'static,
    {
        let this = self.clone();
        match request.uri().path() {
            "/model_plane.v1.OrchestrationCoreService/GetApproval" => {
                *this.approval_reads.lock().expect("lock") += 1;
                answer(request, move |_: TReq<mpv1::GetApprovalRequest>| async move {
                    Ok(Response::new(mpv1::GetApprovalResponse {
                        approval: Some(mpv1::Approval {
                            id: APPROVAL.to_owned(),
                            run_id: RUN.to_owned(),
                            state: mpv1::ApprovalState::Granted as i32,
                            org_id: ORG.to_owned(),
                            continuation_mode: "loop".to_owned(),
                            ..Default::default()
                        }),
                    }))
                })
                .await
            }
            "/model_plane.v1.OrchestrationCoreService/ConsumeApproval" => {
                answer(request, move |request: TReq<mpv1::ConsumeApprovalRequest>| {
                    let this = this.clone();
                    async move { this.consume(request).await.map(Response::new) }
                })
                .await
            }
            // The event tail is optional to the route: an unopened tail is followed past.
            _ => Status::unimplemented("not needed by the route").into_http(),
        }
    }

    async fn consume(
        &self,
        request: TReq<mpv1::ConsumeApprovalRequest>,
    ) -> Result<mpv1::ConsumeApprovalResponse, Status> {
        let authorization = request
            .metadata()
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let received = request.into_inner();
        self.order.lock().expect("lock").push("consume");
        self.consumes
            .lock()
            .expect("lock")
            .push((authorization, received.clone()));
        match &self.behaviour {
            ConsumeBehaviour::Won => Ok(mpv1::ConsumeApprovalResponse {
                consumed: true,
                ..Default::default()
            }),
            ConsumeBehaviour::SingleUse(winner) => {
                let mut winner = winner.lock().expect("lock");
                Ok(match winner.as_deref() {
                    None => {
                        *winner = Some(received.claim_token.clone());
                        mpv1::ConsumeApprovalResponse {
                            consumed: true,
                            ..Default::default()
                        }
                    }
                    Some(first) => {
                        let same_claim = first == received.claim_token;
                        let with_call = same_claim && !received.omit_call;
                        mpv1::ConsumeApprovalResponse {
                            consumed: false,
                            already_consumed: true,
                            same_claim,
                            tool_name: if with_call { "shell".to_owned() } else { String::new() },
                            ..Default::default()
                        }
                    }
                })
            }
            ConsumeBehaviour::Parked(gate) => {
                gate.notified().await;
                Ok(mpv1::ConsumeApprovalResponse {
                    consumed: true,
                    ..Default::default()
                })
            }
            ConsumeBehaviour::Refused(status) => Err(status.clone()),
        }
    }
}

#[derive(Clone)]
struct Runs {
    resumable: bool,
    status: &'static str,
    park_reason: &'static str,
}

double_service!(Runs, "model_plane.v1.RunService");

impl Runs {
    async fn route<B>(&self, request: HttpRequest<B>) -> HttpResponse
    where
        B: tonic::codegen::Body + Send + 'static,
        B::Error: Into<tonic::codegen::StdError> + Send + 'static,
    {
        let (resumable, status, park_reason) = (self.resumable, self.status, self.park_reason);
        match request.uri().path() {
            "/model_plane.v1.RunService/ResolveRunOwner" => {
                answer(request, |_: TReq<mpv1::ResolveRunOwnerRequest>| async {
                    Ok(Response::new(mpv1::ResolveRunOwnerResponse { authorized: true }))
                })
                .await
            }
            "/model_plane.v1.RunService/GetRun" => {
                answer(request, move |_: TReq<mpv1::GetRunRequest>| async move {
                    Ok(Response::new(mpv1::RunDetail {
                        run_id: RUN.to_owned(),
                        thread_id: "thread-1".to_owned(),
                        status: status.to_owned(),
                        goal: "the run's goal".to_owned(),
                        resumable,
                        park_reason: park_reason.to_owned(),
                        ..Default::default()
                    }))
                })
                .await
            }
            _ => Status::unimplemented("not needed by the route").into_http(),
        }
    }
}

/// Execution-core's `RunAgent`, answering what the case says and recording each request.
#[derive(Clone)]
struct Execution {
    order: Order,
    requests: Arc<Mutex<Vec<mpv1::RunAgentRequest>>>,
    answer: Result<mpv1::RunAgentResponse, Status>,
    /// How long `RunAgent` takes to answer (a run that outlives the follow deadline).
    delay: Option<Duration>,
}

#[tonic::async_trait]
impl ExecutionCore for Execution {
    async fn execute_restricted_browser_step(
        &self,
        request: tonic::Request<mp_contracts::model_plane::v1::ExecuteStepRequest>,
    ) -> Result<tonic::Response<mp_contracts::model_plane::v1::ExecuteStepResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("not used by this test"))
    }

    async fn browser_agent_capabilities(
        &self,
        _request: tonic::Request<mp_contracts::model_plane::v1::BrowserAgentCapabilitiesRequest>,
    ) -> Result<tonic::Response<mp_contracts::model_plane::v1::BrowserAgentCapabilitiesResponse>, tonic::Status> {
        Ok(tonic::Response::new(mp_contracts::model_plane::v1::BrowserAgentCapabilitiesResponse { attached_policy_protocol: 1 }))
    }

    async fn run_agent(
        &self,
        request: TReq<mpv1::RunAgentRequest>,
    ) -> Result<Response<mpv1::RunAgentResponse>, Status> {
        self.order.lock().expect("lock").push("run_agent");
        self.requests.lock().expect("lock").push(request.into_inner());
        if let Some(delay) = self.delay {
            tokio::time::sleep(delay).await;
        }
        self.answer.clone().map(Response::new)
    }

    async fn execute_step(
        &self,
        _: TReq<mpv1::ExecuteStepRequest>,
    ) -> Result<Response<mpv1::ExecuteStepResponse>, Status> {
        Err(Status::unimplemented("not needed"))
    }

    async fn resume_run(
        &self,
        _: TReq<mpv1::ResumeRunRequest>,
    ) -> Result<Response<mpv1::ResumeRunResponse>, Status> {
        Err(Status::unimplemented("not needed"))
    }

    async fn cancel_run(
        &self,
        _: TReq<mpv1::CancelRunRequest>,
    ) -> Result<Response<mpv1::CancelRunResponse>, Status> {
        Err(Status::unimplemented("not needed"))
    }

    async fn pause_run(
        &self,
        _: TReq<mpv1::PauseRunRequest>,
    ) -> Result<Response<mpv1::PauseRunResponse>, Status> {
        Err(Status::unimplemented("not needed"))
    }

    async fn execute_scheduled_step(
        &self,
        _: TReq<mpv1::ExecuteScheduledStepRequest>,
    ) -> Result<Response<mpv1::ExecuteScheduledStepResponse>, Status> {
        Err(Status::unimplemented("not needed"))
    }
}

type Receipts = Arc<Mutex<Vec<(String, mpv1::RecordTerminalOutcomeRequest)>>>;

/// The managed-run lifecycle, of which a rejected dispatch reaches only the terminal receipt.
#[derive(Clone)]
struct Lifecycle {
    receipts: Receipts,
}

#[tonic::async_trait]
impl ManagedRunLifecycle for Lifecycle {
    async fn start_managed_run(
        &self,
        _: TReq<mpv1::StartManagedRunRequest>,
    ) -> Result<Response<mpv1::StartManagedRunResponse>, Status> {
        Err(Status::unimplemented("not needed"))
    }

    async fn record_terminal_outcome(
        &self,
        request: TReq<mpv1::RecordTerminalOutcomeRequest>,
    ) -> Result<Response<mpv1::RecordTerminalOutcomeResponse>, Status> {
        let authorization = request
            .metadata()
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        let request = request.into_inner();
        self.receipts
            .lock()
            .expect("lock")
            .push((authorization, request.clone()));
        Ok(Response::new(mpv1::RecordTerminalOutcomeResponse {
            run_id: request.run_id,
            source: request.source,
            terminal_step_id: "model-gateway-agent-dispatch-rejected".to_owned(),
            step_index: 1,
            receipt_id: "receipt-managed-terminal".to_owned(),
            ..Default::default()
        }))
    }

    async fn heartbeat_managed_run(
        &self,
        _: TReq<mpv1::HeartbeatManagedRunRequest>,
    ) -> Result<Response<mpv1::HeartbeatManagedRunResponse>, Status> {
        Err(Status::unimplemented("not needed"))
    }

    async fn record_run_output(
        &self,
        _: TReq<mpv1::RecordRunOutputRequest>,
    ) -> Result<Response<mpv1::RecordRunOutputResponse>, Status> {
        Err(Status::unimplemented("not needed"))
    }

    async fn complete_direct_run_with_message(
        &self,
        _: TReq<mpv1::CompleteDirectRunWithMessageRequest>,
    ) -> Result<Response<mpv1::CompleteDirectRunWithMessageResponse>, Status> {
        Err(Status::unimplemented("not needed"))
    }
}

/// An execution client at a port nothing listens on: the connect is refused, so the request
/// provably never left the gateway.
async fn unreachable_execution_client() -> ExecutionCoreClient<tonic::transport::Channel> {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    drop(listener);
    ExecutionCoreClient::new(
        Endpoint::from_shared(format!("http://{addr}"))
            .expect("endpoint")
            .connect_lazy(),
    )
}

/// A listener that accepts and then drops the socket without speaking gRPC: the connection
/// is ESTABLISHED before it dies, so non-delivery cannot be proven.
async fn connection_dropping_execution_client() -> ExecutionCoreClient<tonic::transport::Channel> {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(200)).await;
                drop(socket);
            });
        }
    });
    ExecutionCoreClient::new(
        Endpoint::from_shared(format!("http://{addr}"))
            .expect("endpoint")
            .connect_lazy(),
    )
}

// ---- the rig ----------------------------------------------------------------------

/// How the executor behaves.
#[derive(Clone)]
enum ExecutionKind {
    /// `RunAgent` answers this.
    Answers(Result<mpv1::RunAgentResponse, Status>),
    /// Nothing listens: the connect is refused.
    Unreachable,
    /// The connection is established and then dropped.
    Dropping,
    /// `RunAgent` answers this after the delay.
    Slow(Result<mpv1::RunAgentResponse, Status>, Duration),
}

fn paused_for_approval() -> ExecutionKind {
    ExecutionKind::Answers(Ok(mpv1::RunAgentResponse {
        status: "awaiting_approval".to_owned(),
        ..Default::default()
    }))
}

fn refusing(status: Status) -> ExecutionKind {
    ExecutionKind::Answers(Err(status))
}

/// `RunAgent` answers, successfully, that the run ended in this status (`failed`, or one the
/// gateway does not know).
fn ending_as(status: &str) -> ExecutionKind {
    ExecutionKind::Answers(Ok(mpv1::RunAgentResponse {
        status: status.to_owned(),
        ..Default::default()
    }))
}

/// A zero-retention refusal of a durable mutation, as the executor's trailer carries it, on
/// the gRPC code the case names.
fn zero_retention_refusal(code: tonic::Code) -> ExecutionKind {
    refusing(mp_contracts::zdr_refusal::Refusal::DURABLE_MUTATION_FORBIDDEN.grpc_status(code))
}

struct Case {
    /// `EXECUTION_LOOP_RESUME_ENABLED` at the gateway.
    resume_switch: bool,
    /// `RunDetail.resumable` of the run.
    resumable: bool,
    consume: ConsumeBehaviour,
    execution: ExecutionKind,
    /// `EXECUTION_LOOP_PARKING_ENABLED` at the gateway (WS03 S8-A).
    parking_switch: bool,
    /// `RunDetail.status` of the run.
    status: &'static str,
    /// `RunDetail.park_reason` of the run (empty: not parked).
    park_reason: &'static str,
}

impl Case {
    fn new(resume_switch: bool, execution: ExecutionKind) -> Self {
        Self {
            resume_switch,
            resumable: true,
            consume: ConsumeBehaviour::Won,
            execution,
            parking_switch: false,
            status: "awaiting_approval",
            park_reason: "",
        }
    }

    /// A parked run, with the resume and parking switches on.
    fn parked(execution: ExecutionKind) -> Self {
        Self {
            parking_switch: true,
            status: "running",
            park_reason: "provider_exhausted",
            ..Self::new(true, execution)
        }
    }
}

struct Rig {
    app: Router,
    auth: Auth,
    order: Order,
    consumes: Arc<Mutex<Vec<SeenConsume>>>,
    approval_reads: Arc<Mutex<usize>>,
    run_agents: Arc<Mutex<Vec<mpv1::RunAgentRequest>>>,
    receipts: Receipts,
    publisher: Arc<DynPublisher>,
    _terminalizer: MockServer,
    _env: Vec<EnvGuard>,
}

async fn rig(case: Case) -> Rig {
    let auth = start_auth().await;
    let env = vec![
        EnvGuard::set("AGENT_CONTINUE_AFTER_APPROVAL_ENABLED", Some("true")),
        EnvGuard::set(
            "EXECUTION_LOOP_RESUME_ENABLED",
            case.resume_switch.then_some("true"),
        ),
        EnvGuard::set(
            "EXECUTION_LOOP_PARKING_ENABLED",
            case.parking_switch.then_some("true"),
        ),
    ];
    let order: Order = Arc::default();
    let consumes: Arc<Mutex<Vec<SeenConsume>>> = Arc::default();
    let approval_reads: Arc<Mutex<usize>> = Arc::default();
    let run_agents: Arc<Mutex<Vec<mpv1::RunAgentRequest>>> = Arc::default();
    let receipts: Receipts = Arc::default();

    let terminalizer = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/session-core/internal-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "token": TERMINALIZER_TOKEN,
            "expiresInSeconds": 300,
            "audience": "session-core"
        })))
        .mount(&terminalizer)
        .await;

    let publisher = Arc::new(DynPublisher::InMemory(InMemoryPublisher::new()));
    let mut state = offline_downstreams::offline_state();
    state.publisher = publisher.clone();
    state.orchestration_client = OrchestrationCoreServiceClient::new(serve!(Orchestration {
        order: order.clone(),
        behaviour: case.consume,
        consumes: consumes.clone(),
        approval_reads: approval_reads.clone(),
    }));
    // The consume goes to session-core's own endpoint, not through orchestrator-core (which does
    // not serve it): one double stands for both here.
    state.consume_client = state.orchestration_client.clone();
    state.run_client = RunServiceClient::new(serve!(Runs {
        resumable: case.resumable,
        status: case.status,
        park_reason: case.park_reason,
    }));
    state.managed_run_client = ManagedRunLifecycleClient::new({
        let lifecycle = Lifecycle {
            receipts: receipts.clone(),
        };
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            Server::builder()
                .add_service(ManagedRunLifecycleServer::new(lifecycle))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .ok();
        });
        Endpoint::from_shared(format!("http://{addr}"))
            .expect("endpoint")
            .connect()
            .await
            .expect("connect")
    });
    let (execution, delay) = match case.execution {
        ExecutionKind::Slow(answer, delay) => (ExecutionKind::Answers(answer), Some(delay)),
        other => (other, None),
    };
    state.execution_client = match execution {
        ExecutionKind::Unreachable => unreachable_execution_client().await,
        ExecutionKind::Dropping => connection_dropping_execution_client().await,
        ExecutionKind::Slow(..) => unreachable!("taken apart above"),
        ExecutionKind::Answers(answer) => {
            let double = Execution {
                order: order.clone(),
                requests: run_agents.clone(),
                answer,
                delay,
            };
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
            let addr = listener.local_addr().expect("addr");
            tokio::spawn(async move {
                Server::builder()
                    .add_service(ExecutionCoreServer::new(double))
                    .serve_with_incoming(TcpListenerStream::new(listener))
                    .await
                    .ok();
            });
            ExecutionCoreClient::new(
                Endpoint::from_shared(format!("http://{addr}"))
                    .expect("endpoint")
                    .connect()
                    .await
                    .expect("connect"),
            )
        }
    };
    state
        .configure_managed_terminalization(
            &terminalizer.uri(),
            "model-gateway",
            "test-model-gateway-service-credential",
        )
        .expect("configure the terminalization credential");

    Rig {
        app: build_router(state, None),
        auth,
        order,
        consumes,
        approval_reads,
        run_agents,
        receipts,
        publisher,
        _terminalizer: terminalizer,
        _env: env,
    }
}

impl Rig {
    fn continue_request(&self, claim_token: &str) -> Request<Body> {
        self.continue_request_with(APPROVAL, claim_token)
    }

    /// The continue with this approval id (`""` is the park form, WS03 S8-A).
    fn continue_request_with(&self, approval_id: &str, claim_token: &str) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(format!("/v1/orchestration/runs/{RUN}/continue"))
            .header(AUTHORIZATION, format!("Bearer {}", self.auth.ingress))
            .header("x-session-authorization", format!("Bearer {}", self.auth.session))
            .header("x-execution-authorization", format!("Bearer {}", self.auth.execution))
            .header("x-data-plane-authorization", format!("Bearer {}", self.auth.data_plane))
            .header("x-inference-authorization", format!("Bearer {}", self.auth.inference))
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({ "approval_id": approval_id, "claim_token": claim_token }).to_string(),
            ))
            .expect("a request")
    }

    /// POST the continue and read the whole answer.
    async fn post(&self, claim_token: &str) -> Answer {
        self.post_with(APPROVAL, claim_token).await
    }

    /// POST the continue with this approval id and read the whole answer.
    async fn post_with(&self, approval_id: &str, claim_token: &str) -> Answer {
        let response = self
            .app
            .clone()
            .oneshot(self.continue_request_with(approval_id, claim_token))
            .await
            .expect("the router answers");
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("the body reads");
        Answer {
            status,
            body: String::from_utf8(bytes.to_vec()).expect("UTF-8"),
        }
    }

    fn orders(&self) -> Vec<&'static str> {
        self.order.lock().expect("lock").clone()
    }

    fn run_agent_count(&self) -> usize {
        self.run_agents.lock().expect("lock").len()
    }

    fn consume_count(&self) -> usize {
        self.consumes.lock().expect("lock").len()
    }

    fn receipt_count(&self) -> usize {
        self.receipts.lock().expect("lock").len()
    }

    fn approval_read_count(&self) -> usize {
        *self.approval_reads.lock().expect("lock")
    }

    /// Whether `RUN_DISPATCH_DEGRADED` was published for the run.
    fn degraded(&self) -> bool {
        self.publisher
            .drain()
            .iter()
            .any(|(_, envelope)| envelope.event_type == "RUN_DISPATCH_DEGRADED")
    }

    /// Everything the stream published, in order. Draining: call it once per rig.
    fn published(&self) -> Vec<(String, mp_events::envelope::Envelope)> {
        self.publisher.drain()
    }
}

struct Answer {
    status: StatusCode,
    body: String,
}

impl Answer {
    /// The SSE frames: `(event, data)`.
    fn frames(&self) -> Vec<(String, Value)> {
        self.body
            .split("\n\n")
            .filter_map(|block| {
                let event = block.lines().find_map(|line| line.strip_prefix("event: "))?;
                let data = block.lines().find_map(|line| line.strip_prefix("data: "))?;
                Some((event.to_owned(), serde_json::from_str(data).ok()?))
            })
            .collect()
    }

    fn frame(&self, event: &str) -> Option<Value> {
        self.frames()
            .into_iter()
            .find_map(|(name, payload)| (name == event).then_some(payload))
    }

    /// The `code` of the (single) error frame, if there is one.
    fn error_frame(&self) -> Option<Value> {
        self.frame("error")
    }

    /// The `error.code` of an HTTP refusal.
    fn http_code(&self) -> String {
        serde_json::from_str::<Value>(&self.body).expect("a JSON error body")["error"]["code"]
            .as_str()
            .expect("an error code")
            .to_owned()
    }
}

/// Wait, bounded, for a condition another task makes true.
async fn wait_until(what: &str, condition: impl Fn() -> bool) {
    let waited = tokio::time::timeout(Duration::from_secs(10), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    assert!(waited.is_ok(), "{what} never happened");
}

// ---- the scenarios ----------------------------------------------------------------

/// The consume is made, spend-only, strictly BEFORE `RunAgent`, with the user's session
/// bearer; the connected frame says the click is spent; and the stage hint is `loop` only
/// for a run whose pause is recorded as resumable.
#[tokio::test]
#[serial_test::serial]
async fn the_gateway_consumes_the_approval_before_it_dispatches_the_run() {
    for resumable in [true, false] {
        let mut case = Case::new(true, paused_for_approval());
        case.resumable = resumable;
        let rig = rig(case).await;
        let answer = rig.post("claim-1").await;

        assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
        assert_eq!(rig.orders(), vec!["consume", "run_agent"], "resumable {resumable}");
        let consumes = rig.consumes.lock().expect("lock");
        assert_eq!(consumes.len(), 1);
        let (authorization, consume) = &consumes[0];
        assert_eq!(
            authorization.as_deref(),
            Some(format!("Bearer {}", rig.auth.session).as_str()),
            "the consume is made with the user's session bearer, the one the executor receives"
        );
        assert!(consume.omit_call, "the gateway spends; it never holds the call");
        assert_eq!(consume.approval_id, APPROVAL);
        assert_eq!(consume.run_id, RUN);
        assert_eq!(consume.org_id, ORG);
        assert_eq!(consume.claim_token, "claim-1");
        assert!(consume.step_id.is_empty() && consume.action_fingerprint.is_empty());

        let requests = rig.run_agents.lock().expect("lock");
        assert_eq!(requests.len(), 1);
        let resume = requests[0].resume.as_ref().expect("a continuation carries its resume");
        assert_eq!(resume.mode, if resumable { "loop" } else { "segment" });
        assert_eq!(resume.segment_no, 0);
        assert_eq!(resume.approval_id, APPROVAL);
        assert_eq!(resume.claim_token, "claim-1");
        assert!(resume.brief.is_empty() && resume.step_id.is_empty() && resume.tool_name.is_empty());
        assert_eq!(requests[0].space_id, "", "the gateway states no Space");

        let connected = answer.frame("connected").expect("a connected frame");
        assert_eq!(connected["approval_spent"], true, "{}", answer.body);
        assert!(answer.frame("awaiting_approval").is_some(), "{}", answer.body);
        assert!(answer.error_frame().is_none(), "{}", answer.body);
        assert_eq!(rig.receipt_count(), 0);
    }
}

/// With the switch OFF the route is the one it always was: no consume at all, the stage is
/// `segment` whatever the run says, and the first frame is the committed one.
#[tokio::test]
#[serial_test::serial]
async fn with_the_switch_off_nothing_is_consumed_and_the_first_frame_is_the_committed_one() {
    let rig = rig(Case::new(false, paused_for_approval())).await;
    let answer = rig.post("claim-1").await;

    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    assert_eq!(rig.consume_count(), 0, "the gateway does not consume with the switch off");
    assert_eq!(rig.orders(), vec!["run_agent"]);
    let requests = rig.run_agents.lock().expect("lock");
    assert_eq!(requests[0].resume.as_ref().expect("resume").mode, "segment");
    let connected = answer.frame("connected").expect("a connected frame");
    assert_eq!(
        connected,
        json!({
            "ok": true,
            "run_id": RUN,
            "thread_id": "thread-1",
            "request_id": connected["request_id"].clone(),
        }),
        "the connected payload carries no approval_spent: {}",
        answer.body
    );
}

/// A lost consume is a 409 before any stream opens and dispatches nothing: another claim
/// already won, or the very same token is sent a second time (the executor accepts that
/// replay; the gateway does not).
#[tokio::test]
#[serial_test::serial]
async fn a_lost_consume_is_a_409_and_dispatches_nothing() {
    let winner = Arc::new(Mutex::new(None));
    let mut case = Case::new(true, paused_for_approval());
    case.consume = ConsumeBehaviour::SingleUse(winner);
    let rig = rig(case).await;

    let first = rig.post("claim-a").await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.body);
    assert_eq!(rig.run_agent_count(), 1);

    for token in ["claim-a", "claim-b"] {
        let lost = rig.post(token).await;
        assert_eq!(lost.status, StatusCode::CONFLICT, "{token}: {}", lost.body);
        assert_eq!(lost.http_code(), "continuation_already_consumed", "{token}");
        assert!(
            !lost.body.contains("event:"),
            "{token}: no stream is opened for a lost consume: {}",
            lost.body
        );
    }
    assert_eq!(rig.consume_count(), 3, "every request consumed once");
    assert_eq!(rig.run_agent_count(), 1, "a lost consume never reaches the executor");
    assert_eq!(rig.receipt_count(), 0);
}

/// A refused consume answers with its closed HTTP code and dispatches nothing.
#[tokio::test]
#[serial_test::serial]
async fn a_refused_consume_is_its_closed_code_and_dispatches_nothing() {
    for (status, expected_status, expected_code) in [
        (Status::not_found("approval not found"), StatusCode::NOT_FOUND, "approval_not_found"),
        (
            Status::failed_precondition("approval_expired"),
            StatusCode::CONFLICT,
            "approval_expired",
        ),
        (
            Status::unavailable("session-core is down"),
            StatusCode::SERVICE_UNAVAILABLE,
            "continue_unavailable",
        ),
    ] {
        let mut case = Case::new(true, paused_for_approval());
        case.consume = ConsumeBehaviour::Refused(status);
        let rig = rig(case).await;
        let answer = rig.post("claim-1").await;
        assert_eq!(answer.status, expected_status, "{}", answer.body);
        assert_eq!(answer.http_code(), expected_code);
        assert!(!answer.body.contains("session-core is down"), "an upstream message is never echoed");
        assert_eq!(rig.run_agent_count(), 0);
        assert_eq!(rig.receipt_count(), 0);
    }
}

/// Two clicks with the SAME token at once, against the real compare-and-set shape: one
/// stream, one 409, exactly one `RunAgent`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn two_simultaneous_clicks_with_one_token_dispatch_once() {
    let mut case = Case::new(true, paused_for_approval());
    case.consume = ConsumeBehaviour::SingleUse(Arc::new(Mutex::new(None)));
    let rig = rig(case).await;
    let (a, b) = tokio::join!(rig.post("claim-a"), rig.post("claim-a"));
    let mut statuses = vec![a.status, b.status];
    statuses.sort();
    assert_eq!(statuses, vec![StatusCode::OK, StatusCode::CONFLICT]);
    let lost = if a.status == StatusCode::CONFLICT { &a } else { &b };
    assert_eq!(lost.http_code(), "continuation_already_consumed");
    assert_eq!(rig.run_agent_count(), 1, "one click, one dispatch");
    assert_eq!(rig.consume_count(), 2);
}

/// The frames of every way the dispatch can fail, per state of the resume switch. With the
/// switch ON the click is spent: every frame says `approval_spent`, is not retryable, is
/// never an `agent_dispatch_*` code and the run is never terminalized. With it OFF nothing
/// was consumed by the gateway: a confirmed rejection (`InvalidArgument`,
/// `Unauthenticated`, `PermissionDenied`) keeps the committed arm (terminalized,
/// `agent_dispatch_rejected`), the executor's other refusals are typed frames WITHOUT
/// `approval_spent` and WITHOUT `RUN_DISPATCH_DEGRADED`, and an unreachable or ambiguous
/// outcome keeps the committed frames.
#[tokio::test]
#[serial_test::serial]
async fn frame_codes_pair_with_the_spend_state_per_switch_state() {
    struct Expect {
        code: &'static str,
        /// `Some(true)`: `approval_spent` is true on the error frame; `None`: absent.
        spent: bool,
        retryable: bool,
        degraded: bool,
        receipts: usize,
    }
    let spent = |code, degraded| Expect { code, spent: true, retryable: false, degraded, receipts: 0 };
    let typed = |code| Expect { code, spent: false, retryable: false, degraded: false, receipts: 0 };

    #[allow(clippy::type_complexity)]
    let cases: Vec<(&str, bool, ExecutionKind, Expect)> = vec![
        // --- switch ON: the gateway spent the click ---
        ("on / FailedPrecondition", true, refusing(Status::failed_precondition("continuation_already_consumed")), spent("continuation_already_consumed", false)),
        ("on / Aborted", true, refusing(Status::aborted("continue_in_progress")), spent("continue_in_progress", false)),
        ("on / NotFound", true, refusing(Status::not_found("approval not found")), spent("approval_not_found", false)),
        ("on / InvalidArgument", true, refusing(Status::invalid_argument("resume_incomplete")), spent("resume_incomplete", false)),
        ("on / PermissionDenied", true, refusing(Status::permission_denied("resume_forbidden")), spent("resume_forbidden", false)),
        ("on / unlisted refusal", true, refusing(Status::failed_precondition("a sentence nobody listed")), spent("continue_refused", false)),
        ("on / DeadlineExceeded", true, refusing(Status::deadline_exceeded("slow")), spent("continue_outcome_unknown", true)),
        ("on / unreachable", true, ExecutionKind::Unreachable, spent("continue_unreachable", true)),
        ("on / dropped", true, ExecutionKind::Dropping, spent("continue_outcome_unknown", true)),
        ("on / Unauthenticated", true, refusing(Status::unauthenticated("a credential sentence")), spent("continue_refused", false)),
        ("on / zero-retention trailer", true, zero_retention_refusal(tonic::Code::FailedPrecondition), spent("durable_mutation_forbidden", false)),
        ("on / zero-retention trailer on PermissionDenied", true, zero_retention_refusal(tonic::Code::PermissionDenied), spent("durable_mutation_forbidden", false)),
        // The two frames AFTER the executor answered: the run ended failed, or in a status the
        // gateway does not know. The click is spent, so neither may say `retryable`.
        ("on / the run ended failed", true, ending_as("failed"), spent("agent_run_failed", false)),
        ("on / the run ended in an unknown status", true, ending_as("quarantined"), spent("agent_run_state_unknown", true)),
        // --- switch OFF: nothing consumed by the gateway ---
        ("off / FailedPrecondition", false, refusing(Status::failed_precondition("continue_disabled")), typed("continue_disabled")),
        ("off / Aborted", false, refusing(Status::aborted("continue_in_progress")), typed("continue_in_progress")),
        ("off / NotFound", false, refusing(Status::not_found("approval not found")), typed("approval_not_found")),
        ("off / InvalidArgument", false, refusing(Status::invalid_argument("resume_incomplete")), Expect { code: "agent_dispatch_rejected", spent: false, retryable: false, degraded: false, receipts: 1 }),
        ("off / PermissionDenied", false, refusing(Status::permission_denied("resume_forbidden")), Expect { code: "agent_dispatch_rejected", spent: false, retryable: false, degraded: false, receipts: 1 }),
        ("off / DeadlineExceeded", false, refusing(Status::deadline_exceeded("slow")), Expect { code: "agent_dispatch_unavailable", spent: false, retryable: true, degraded: true, receipts: 0 }),
        ("off / unreachable", false, ExecutionKind::Unreachable, Expect { code: "agent_dispatch_unreachable", spent: false, retryable: true, degraded: false, receipts: 1 }),
        ("off / dropped", false, ExecutionKind::Dropping, Expect { code: "agent_dispatch_unavailable", spent: false, retryable: true, degraded: true, receipts: 0 }),
        ("off / Unauthenticated", false, refusing(Status::unauthenticated("a credential sentence")), Expect { code: "agent_dispatch_rejected", spent: false, retryable: false, degraded: false, receipts: 1 }),
        ("off / zero-retention trailer", false, zero_retention_refusal(tonic::Code::FailedPrecondition), typed("durable_mutation_forbidden")),
        // The committed confirmed-rejection arm answers before the typed one: a trailer that
        // travels on PermissionDenied still terminalizes, as it always did.
        ("off / zero-retention trailer on PermissionDenied", false, zero_retention_refusal(tonic::Code::PermissionDenied), Expect { code: "agent_dispatch_rejected", spent: false, retryable: false, degraded: false, receipts: 1 }),
        // The run's own end, with nothing spent by the gateway: the committed frames, retryable.
        ("off / the run ended failed", false, ending_as("failed"), Expect { code: "agent_run_failed", spent: false, retryable: true, degraded: false, receipts: 0 }),
        ("off / the run ended in an unknown status", false, ending_as("quarantined"), Expect { code: "agent_run_state_unknown", spent: false, retryable: true, degraded: true, receipts: 0 }),
    ];
    for (name, switch, execution, expect) in cases {
        let rig = rig(Case::new(switch, execution)).await;
        let answer = rig.post("claim-1").await;
        assert_eq!(answer.status, StatusCode::OK, "{name}: {}", answer.body);
        let error = answer
            .error_frame()
            .unwrap_or_else(|| panic!("{name}: an error frame is expected: {}", answer.body));
        assert_eq!(error["code"], expect.code, "{name}: {}", answer.body);
        assert_eq!(error["retryable"], expect.retryable, "{name}");
        // The committed frames of an unspent continuation that never named the run keep not
        // naming it (the two terminalizing ones, and the run's own end); every other frame does.
        let committed_without_run = !expect.spent
            && matches!(
                expect.code,
                "agent_dispatch_rejected"
                    | "agent_dispatch_unreachable"
                    | "agent_run_failed"
                    | "agent_run_state_unknown"
            );
        if committed_without_run {
            assert!(error.get("run_id").is_none(), "{name}: the committed frame is unchanged: {error}");
        } else {
            assert_eq!(error["run_id"], RUN, "{name}");
        }
        if expect.spent {
            assert_eq!(error["approval_spent"], true, "{name}");
            assert_eq!(
                answer.frame("connected").expect("connected")["approval_spent"],
                true,
                "{name}: the first frame already says the click is spent"
            );
            assert!(
                !expect.code.starts_with("agent_dispatch"),
                "{name}: a spent continuation never answers with a committed dispatch frame"
            );
        } else {
            assert!(
                error.get("approval_spent").is_none(),
                "{name}: the gateway spent nothing, so it claims nothing: {error}"
            );
            assert!(
                answer.frame("connected").expect("connected").get("approval_spent").is_none(),
                "{name}"
            );
        }
        assert_eq!(rig.consume_count(), usize::from(switch), "{name}: the gateway consumes only with the switch on");
        assert_eq!(rig.degraded(), expect.degraded, "{name}: RUN_DISPATCH_DEGRADED");
        assert_eq!(
            rig.receipt_count(),
            expect.receipts,
            "{name}: the run is terminalized only by the committed arms"
        );
        if expect.receipts == 1 {
            let receipts = rig.receipts.lock().expect("lock");
            assert_eq!(receipts[0].0, format!("Bearer {TERMINALIZER_TOKEN}"), "{name}");
            assert_eq!(
                receipts[0].1.source,
                mpv1::ManagedRunSource::GatewayAgentDispatchRejected as i32,
                "{name}"
            );
        }
    }
}

/// A client that disconnects while the consume is in flight cannot spend an approval
/// without the run being sent: the consume and the launch are one detached task.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn a_client_that_disconnects_mid_consume_still_gets_exactly_one_dispatch() {
    let gate = Arc::new(tokio::sync::Notify::new());
    let mut case = Case::new(true, paused_for_approval());
    case.consume = ConsumeBehaviour::Parked(gate.clone());
    let rig = Arc::new(rig(case).await);

    let request = rig.continue_request("claim-1");
    let app = rig.app.clone();
    let client = tokio::spawn(async move { app.oneshot(request).await });
    wait_until("the consume reaching session-core", || rig.consume_count() == 1).await;
    // The client goes away mid-consume: the handler future is dropped, as hyper drops it.
    client.abort();
    let _ = client.await;
    assert_eq!(rig.run_agent_count(), 0, "nothing is dispatched before the consume commits");

    // Session-core commits after the client is gone.
    gate.notify_one();
    wait_until("the run being dispatched", || rig.run_agent_count() == 1).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(rig.run_agent_count(), 1, "exactly one RunAgent per won consume");
    assert_eq!(rig.consume_count(), 1);
    assert_eq!(rig.receipt_count(), 0);
    assert_eq!(rig.orders(), vec!["consume", "run_agent"]);
    let requests = rig.run_agents.lock().expect("lock");
    assert_eq!(requests[0].resume.as_ref().expect("resume").claim_token, "claim-1");
}

/// The two frames after the executor answered, byte for byte. A continuation the gateway did
/// not pre-consume gets the committed frames (retryable, no run id, no `approval_spent`): the
/// click is not the gateway's, and a retry with a new token is what the committed rules allow.
/// One the gateway spent gets the same code and text, not retryable, naming the run and
/// saying `approval_spent`: a retry of a spent click is a 409, and the client follows the run.
#[tokio::test]
#[serial_test::serial]
async fn the_frames_after_the_executors_answer_are_the_committed_ones_unless_the_gateway_spent_the_click() {
    for (status, code, message) in [
        ("failed", "agent_run_failed", "The governed agent run failed."),
        (
            "quarantined",
            "agent_run_state_unknown",
            "The agent returned an unsupported lifecycle state.",
        ),
    ] {
        let rig_off = rig(Case::new(false, ending_as(status))).await;
        let off = rig_off.post("claim-1").await;
        let frame = off.error_frame().expect("an error frame");
        assert_eq!(
            frame,
            json!({
                "code": code,
                "message": message,
                "retryable": true,
                "request_id": frame["request_id"].clone(),
            }),
            "off / {status}: the committed frame, unchanged: {}",
            off.body
        );
        drop(rig_off);

        let rig_on = rig(Case::new(true, ending_as(status))).await;
        let on = rig_on.post("claim-1").await;
        let frame = on.error_frame().expect("an error frame");
        assert_eq!(
            frame,
            json!({
                "code": code,
                "message": message,
                "retryable": false,
                "request_id": frame["request_id"].clone(),
                "run_id": RUN,
                "approval_spent": true,
            }),
            "on / {status}: the same code and text, final and spent: {}",
            on.body
        );
        assert_eq!(rig_on.receipt_count(), 0, "a spent click is never terminalized by the gateway");
    }
}

/// What the observers of the run subject (`mp.v1.run.{run_id}.event`) see of a continuation
/// attempt (build plan WS03, the spent-frames unit). `RUN_STARTED` goes out before the dispatch
/// result, so a refusal the executor gives a SPENT continuation would otherwise leave a started
/// run with no closing event. The gateway closes the ATTEMPT, never the run: a refusal can be
/// `continuation_already_consumed` while another segment of the run is live, so there is no
/// terminal event and no `RUN_DISPATCH_DEGRADED` (a run-level mark) for it, and no receipt.
///
/// - a spent refusal publishes exactly `RUN_CONTINUE_REFUSED` after `RUN_STARTED`, once, with
///   the frame unchanged;
/// - a spent unreachable or ambiguous outcome keeps `RUN_DISPATCH_DEGRADED`, nothing else;
/// - with the switch off (nothing spent) every outcome publishes what it always did, and never
///   the closing event.
#[tokio::test]
#[serial_test::serial]
async fn a_spent_refusal_closes_the_attempt_on_the_run_subject_and_nothing_else_changes() {
    const STARTED: &str = "RUN_STARTED";
    const CLOSED: &str = "RUN_CONTINUE_REFUSED";
    const DEGRADED: &str = "RUN_DISPATCH_DEGRADED";
    const FAILED: &str = "RUN_FAILED";

    // (name, switch, what the executor does, what follows RUN_STARTED, the frame's code)
    #[allow(clippy::type_complexity)]
    let cases: Vec<(&str, bool, ExecutionKind, Vec<&str>, &str)> = vec![
        // --- switch ON: spent. A deterministic refusal closes the attempt.
        ("on / FailedPrecondition", true, refusing(Status::failed_precondition("continuation_already_consumed")), vec![CLOSED], "continuation_already_consumed"),
        ("on / Aborted", true, refusing(Status::aborted("continue_in_progress")), vec![CLOSED], "continue_in_progress"),
        ("on / NotFound", true, refusing(Status::not_found("approval not found")), vec![CLOSED], "approval_not_found"),
        ("on / InvalidArgument", true, refusing(Status::invalid_argument("resume_incomplete")), vec![CLOSED], "resume_incomplete"),
        ("on / PermissionDenied", true, refusing(Status::permission_denied("resume_forbidden")), vec![CLOSED], "resume_forbidden"),
        ("on / Unauthenticated", true, refusing(Status::unauthenticated("a credential sentence")), vec![CLOSED], "continue_refused"),
        ("on / unlisted refusal", true, refusing(Status::failed_precondition("a sentence nobody listed")), vec![CLOSED], "continue_refused"),
        ("on / zero-retention trailer", true, zero_retention_refusal(tonic::Code::FailedPrecondition), vec![CLOSED], "durable_mutation_forbidden"),
        // --- switch ON: spent. Unreachable and ambiguous outcomes keep their run-level mark.
        ("on / DeadlineExceeded", true, refusing(Status::deadline_exceeded("slow")), vec![DEGRADED], "continue_outcome_unknown"),
        ("on / unreachable", true, ExecutionKind::Unreachable, vec![DEGRADED], "continue_unreachable"),
        ("on / dropped", true, ExecutionKind::Dropping, vec![DEGRADED], "continue_outcome_unknown"),
        // --- switch OFF: nothing spent, the committed behaviour, never the closing event.
        ("off / FailedPrecondition", false, refusing(Status::failed_precondition("continue_disabled")), vec![], "continue_disabled"),
        ("off / Aborted", false, refusing(Status::aborted("continue_in_progress")), vec![], "continue_in_progress"),
        ("off / NotFound", false, refusing(Status::not_found("approval not found")), vec![], "approval_not_found"),
        ("off / zero-retention trailer", false, zero_retention_refusal(tonic::Code::FailedPrecondition), vec![], "durable_mutation_forbidden"),
        ("off / InvalidArgument", false, refusing(Status::invalid_argument("resume_incomplete")), vec![FAILED], "agent_dispatch_rejected"),
        ("off / PermissionDenied", false, refusing(Status::permission_denied("resume_forbidden")), vec![FAILED], "agent_dispatch_rejected"),
        ("off / unreachable", false, ExecutionKind::Unreachable, vec![FAILED], "agent_dispatch_unreachable"),
        ("off / DeadlineExceeded", false, refusing(Status::deadline_exceeded("slow")), vec![DEGRADED], "agent_dispatch_unavailable"),
        ("off / dropped", false, ExecutionKind::Dropping, vec![DEGRADED], "agent_dispatch_unavailable"),
    ];
    for (name, switch, execution, after_started, code) in cases {
        let rig = rig(Case::new(switch, execution)).await;
        let answer = rig.post("claim-1").await;
        assert_eq!(answer.status, StatusCode::OK, "{name}: {}", answer.body);
        let error = answer.error_frame().unwrap_or_else(|| panic!("{name}: an error frame: {}", answer.body));
        assert_eq!(error["code"], code, "{name}: {}", answer.body);
        let published = rig.published();
        let types: Vec<&str> = published.iter().map(|(_, envelope)| envelope.event_type.as_str()).collect();
        let mut expected = vec![STARTED];
        expected.extend(&after_started);
        assert_eq!(types, expected, "{name}: what the run subject saw");
        for (subject, _) in &published {
            assert_eq!(subject, &format!("mp.v1.run.{RUN}.event"), "{name}: the run's own subject");
        }
        let closing: Vec<_> = published.iter().filter(|(_, e)| e.event_type == CLOSED).collect();
        if after_started == [CLOSED] {
            // Exactly one closing event, and it is an ATTEMPT-level, content-free envelope: it
            // carries this attempt's request id (the frame's), never the run's, no content, no
            // token, no approval id.
            assert_eq!(closing.len(), 1, "{name}: published at most once per attempt");
            let envelope = &closing[0].1;
            assert_eq!(envelope.correlation_id, error["request_id"], "{name}: the attempt's id");
            assert_eq!(
                envelope.resource_ref,
                format!("request/{}", error["request_id"].as_str().expect("request id")),
                "{name}: attempt-level, not run-level"
            );
            assert_eq!(envelope.org_id, ORG, "{name}");
            assert_eq!(envelope.user_id, USER, "{name}");
            assert_eq!(envelope.payload, json!({ "model": "" }), "{name}: content-free");
            assert!(!envelope.zdr, "{name}");
            // The frame the client reads is the one it always was.
            assert_eq!(
                error,
                json!({
                    "code": code,
                    "message": code.replace('_', " "),
                    "retryable": false,
                    "request_id": error["request_id"].clone(),
                    "run_id": RUN,
                    "approval_spent": true,
                }),
                "{name}: the frame is unchanged by the closing event"
            );
        } else {
            assert!(closing.is_empty(), "{name}: the closing event is for a spent refusal alone");
        }
        if switch {
            // The gateway never ends the run it spent the click of: no terminal event, no receipt.
            for (_, envelope) in &published {
                assert!(
                    !["RUN_FAILED", "RUN_COMPLETED", "RUN_CANCELLED"].contains(&envelope.event_type.as_str()),
                    "{name}: the gateway published a terminal event for a spent continuation"
                );
            }
            assert_eq!(rig.receipt_count(), 0, "{name}");
        }
    }
}

/// `agent_run_output_unavailable`, the last error frame the stream can send after the executor
/// answered: the run completed and no answer could be read. With nothing spent it is the
/// committed frame, retryable, no run id. After a pre-consume a retry is a 409, so the same
/// code and text are final, name the run and say `approval_spent`; the run subject sees
/// `RUN_OUTPUT_UNAVAILABLE` either way.
#[tokio::test]
#[serial_test::serial]
async fn the_output_unavailable_frame_is_final_only_after_a_spend() {
    let message = "The completed agent run did not provide an answer.";
    let rig_off = rig(Case::new(false, ending_as("completed"))).await;
    let off = rig_off.post("claim-1").await;
    let frame = off.error_frame().expect("an error frame");
    assert_eq!(
        frame,
        json!({
            "code": "agent_run_output_unavailable",
            "message": message,
            "retryable": true,
            "request_id": frame["request_id"].clone(),
        }),
        "off: the committed frame, unchanged: {}",
        off.body
    );
    let types: Vec<String> = rig_off.published().into_iter().map(|(_, e)| e.event_type).collect();
    assert_eq!(types, ["RUN_STARTED", "RUN_OUTPUT_UNAVAILABLE"]);
    drop(rig_off);

    let rig_on = rig(Case::new(true, ending_as("completed"))).await;
    let on = rig_on.post("claim-1").await;
    let frame = on.error_frame().expect("an error frame");
    assert_eq!(
        frame,
        json!({
            "code": "agent_run_output_unavailable",
            "message": message,
            "retryable": false,
            "request_id": frame["request_id"].clone(),
            "run_id": RUN,
            "approval_spent": true,
        }),
        "on: the same code and text, final and spent: {}",
        on.body
    );
    let types: Vec<String> = rig_on.published().into_iter().map(|(_, e)| e.event_type).collect();
    assert_eq!(types, ["RUN_STARTED", "RUN_OUTPUT_UNAVAILABLE"], "the closing event of a refusal is not published for it");
    assert_eq!(rig_on.receipt_count(), 0);
}

// ---- WS03 S8-A S8-7: the park continue, which never terminalizes ------------------------

/// The park form of `/continue` (an exactly empty approval id, with the gateway's parking
/// switch on) resumes a parked run. Whatever the executor answers, the gateway NEVER
/// terminalizes it and never consumes or reads an approval: every failure is one final
/// `park_continue` frame that never says `approval_spent` (or, for a follow deadline, the bare
/// committed `agent_dispatch_unavailable`), and the run subject sees exactly one event that
/// closes the attempt after `RUN_STARTED`.
///
/// The construction case (a credential that is not valid metadata) cannot be reached through
/// the router, whose bearers are verified JWTs; `sse.rs`'s
/// `a_park_continue_skips_the_pre_consume_check_and_its_construction_failure_is_a_park_frame`
/// drives it on the stream itself.
#[tokio::test]
#[serial_test::serial]
async fn a_park_continue_never_terminalizes() {
    const REFUSED: &str = "RUN_CONTINUE_REFUSED";
    const DEGRADED: &str = "RUN_DISPATCH_DEGRADED";
    // (name, what the executor does, the frame's code, whether it is a park frame, the event
    // that closes the attempt)
    #[allow(clippy::type_complexity)]
    let cases: Vec<(&str, ExecutionKind, &str, bool, &str)> = vec![
        ("InvalidArgument", refusing(Status::invalid_argument("resume_incomplete")), "resume_incomplete", true, REFUSED),
        ("PermissionDenied", refusing(Status::permission_denied("resume_forbidden")), "resume_forbidden", true, REFUSED),
        ("Unauthenticated", refusing(Status::unauthenticated("a credential sentence")), "continue_refused", true, REFUSED),
        ("park_not_due", refusing(Status::failed_precondition("park_not_due")), "park_not_due", true, REFUSED),
        ("not_resumable", refusing(Status::failed_precondition("not_resumable")), "not_resumable", true, REFUSED),
        ("continue_disabled", refusing(Status::failed_precondition("continue_disabled")), "continue_disabled", true, REFUSED),
        ("ECONNREFUSED", ExecutionKind::Unreachable, "continue_unreachable", true, REFUSED),
        ("dropped", ExecutionKind::Dropping, "continue_outcome_unknown", true, DEGRADED),
        (
            "follow deadline",
            ExecutionKind::Slow(
                Ok(mpv1::RunAgentResponse { status: "completed".to_owned(), ..Default::default() }),
                Duration::from_secs(5),
            ),
            "agent_dispatch_unavailable",
            false,
            DEGRADED,
        ),
    ];
    for (name, execution, code, park_frame, closing) in cases {
        // Only the deadline case caps the follow: on Windows a connect to a closed port takes
        // about two seconds to be refused, so a one-second cap on every case would turn the
        // unreachable and dropped cases into deadline cases.
        let deadline = (name == "follow deadline").then_some("1");
        let _deadline = EnvGuard::set("AGENTIC_RUN_DEADLINE_SECS", deadline);
        let unreachable = matches!(execution, ExecutionKind::Unreachable | ExecutionKind::Dropping);
        let rig = rig(Case::parked(execution)).await;
        let answer = rig.post_with("", "claim-1").await;
        assert_eq!(answer.status, StatusCode::OK, "{name}: {}", answer.body);
        let error = answer
            .error_frame()
            .unwrap_or_else(|| panic!("{name}: an error frame: {}", answer.body));
        assert_eq!(error["code"], code, "{name}: {}", answer.body);
        assert!(error.get("approval_spent").is_none(), "{name}: a park spends nothing: {error}");
        assert_eq!(error["run_id"], RUN, "{name}");
        if park_frame {
            assert_eq!(error["park_continue"], true, "{name}");
            assert_eq!(error["retryable"], false, "{name}");
        } else {
            assert!(error.get("park_continue").is_none(), "{name}: the committed bare frame: {error}");
            assert_eq!(error["retryable"], true, "{name}");
        }
        let connected = answer.frame("connected").expect("a connected frame");
        assert!(connected.get("approval_spent").is_none(), "{name}");

        assert_eq!(rig.receipt_count(), 0, "{name}: the gateway never terminalizes a park continue");
        assert_eq!(rig.consume_count(), 0, "{name}: nothing is consumed");
        assert_eq!(rig.approval_read_count(), 0, "{name}: no approval is read");
        let types: Vec<String> = rig.published().into_iter().map(|(_, e)| e.event_type).collect();
        assert_eq!(types, ["RUN_STARTED", closing], "{name}: exactly one attempt-closing event");
        if !unreachable {
            let requests = rig.run_agents.lock().expect("lock");
            assert_eq!(requests.len(), 1, "{name}");
            let resume = requests[0].resume.as_ref().expect("a resume");
            assert_eq!(resume.mode, "park", "{name}");
            assert_eq!(resume.approval_id, "", "{name}");
            assert_eq!(resume.claim_token, "claim-1", "{name}");
        }
    }
}

/// A parked answer (to a park continue, or to any run) is the `run_parked` frame on the turn
/// stream and `RUN_PARKED` on the run subject: no error, nothing terminalized, no receipt. A
/// park reason the gateway does not know is an unknown status, as before.
#[tokio::test]
#[serial_test::serial]
async fn a_parked_answer_is_a_run_parked_frame() {
    let parked = |reason: &str| {
        ExecutionKind::Answers(Ok(mpv1::RunAgentResponse {
            status: "parked".to_owned(),
            park_reason: reason.to_owned(),
            resets_at: "2026-10-20T12:00:00Z".to_owned(),
            ..Default::default()
        }))
    };
    let rig_park = rig(Case::parked(parked("provider_exhausted"))).await;
    let answer = rig_park.post_with("", "claim-1").await;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    let frame = answer.frame("run_parked").expect("a run_parked frame");
    assert_eq!(
        frame,
        json!({
            "run_id": RUN,
            "request_id": frame["request_id"].clone(),
            "status": "parked",
            "reason": "provider_exhausted",
            "resume_after": "2026-10-20T12:00:00Z",
        })
    );
    assert!(answer.error_frame().is_none(), "{}", answer.body);
    let types: Vec<String> = rig_park.published().into_iter().map(|(_, e)| e.event_type).collect();
    assert_eq!(types, ["RUN_STARTED", "RUN_PARKED"]);
    assert_eq!(rig_park.receipt_count(), 0);
    drop(rig_park);

    let rig_unknown = rig(Case::parked(parked("a_reason_nobody_knows"))).await;
    let answer = rig_unknown.post_with("", "claim-1").await;
    assert!(answer.frame("run_parked").is_none(), "{}", answer.body);
    assert_eq!(answer.error_frame().expect("an error frame")["code"], "agent_run_state_unknown");
}

/// The park form is accepted only as designed: with the parking switch off an empty id is the
/// committed 400, a blank id is the committed 400 whatever the switch says, a park continue with
/// the resume switch off is 409 `continue_disabled`, and a run that is not parked, or has ended,
/// is 409 `not_resumable`. None of them reads an approval, consumes or dispatches.
#[tokio::test]
#[serial_test::serial]
async fn the_park_form_is_refused_before_any_dispatch_unless_the_run_is_parked() {
    let paused = || paused_for_approval();
    let cases: Vec<(&str, Case, &str, &str, StatusCode, &str)> = vec![
        ("parking off, empty id", Case { parking_switch: false, ..Case::parked(paused()) }, "", "claim-1", StatusCode::BAD_REQUEST, "continue_incomplete"),
        ("parking on, blank id", Case::parked(paused()), "  ", "claim-1", StatusCode::BAD_REQUEST, "continue_incomplete"),
        ("parking on, no claim", Case::parked(paused()), "", " ", StatusCode::BAD_REQUEST, "continue_incomplete"),
        ("resume switch off", Case { resume_switch: false, ..Case::parked(paused()) }, "", "claim-1", StatusCode::CONFLICT, "continue_disabled"),
        ("not parked", Case { park_reason: "", ..Case::parked(paused()) }, "", "claim-1", StatusCode::CONFLICT, "not_resumable"),
        ("not resumable", Case { resumable: false, ..Case::parked(paused()) }, "", "claim-1", StatusCode::CONFLICT, "not_resumable"),
        ("ended", Case { status: "completed", ..Case::parked(paused()) }, "", "claim-1", StatusCode::CONFLICT, "not_resumable"),
    ];
    for (name, case, approval_id, claim, status, code) in cases {
        let rig = rig(case).await;
        let answer = rig.post_with(approval_id, claim).await;
        assert_eq!(answer.status, status, "{name}: {}", answer.body);
        assert_eq!(answer.http_code(), code, "{name}");
        assert_eq!(rig.approval_read_count(), 0, "{name}: no approval is read");
        assert_eq!(rig.consume_count(), 0, "{name}");
        assert_eq!(rig.run_agent_count(), 0, "{name}");
        assert_eq!(rig.receipt_count(), 0, "{name}");
    }
    // An approval continuation is untouched by the parking switch: it still reads its approval.
    let rig = rig(Case { parking_switch: true, ..Case::new(false, paused_for_approval()) }).await;
    let answer = rig.post("claim-1").await;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    assert_eq!(rig.approval_read_count(), 1);
    assert_eq!(
        rig.run_agents.lock().expect("lock")[0].resume.as_ref().expect("resume").mode,
        "segment"
    );
}
