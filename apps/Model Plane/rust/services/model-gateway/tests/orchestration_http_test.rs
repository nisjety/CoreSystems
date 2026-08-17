use axum::{
    body::to_bytes,
    body::Body,
    http::{header::AUTHORIZATION, Request, StatusCode},
};
use model_gateway::{
    http_routes::build_router,
    state::{AppState, DynPublisher},
};
use mp_contracts::model_plane::v1::{
    orchestration_core_service_client::OrchestrationCoreServiceClient,
    orchestration_core_service_server::{OrchestrationCoreService, OrchestrationCoreServiceServer},
    orchestration_event,
    run_service_client::RunServiceClient,
    run_service_server::{RunService, RunServiceServer},
    AcknowledgeApprovalDeliveryRequest, AcknowledgeApprovalDeliveryResponse, Approval,
    ApprovalState, AttachSubagentRequest, AttachSubagentResponse, CancelRunRequest,
    CancelRunResponse, ClaimApprovalDeliveriesRequest, ClaimApprovalDeliveriesResponse,
    CreateApprovalRequest, CreateApprovalResponse, DecideApprovalRequest, DecideApprovalResponse,
    GetApprovalContinuationRequest, GetApprovalContinuationResponse, GetApprovalRequest,
    GetApprovalResponse, GetPlanRequest, GetPlanResponse, GetRunRequest,
    GetScheduledStepContextRequest, GetSubagentLineageRequest, GetSubagentLineageResponse,
    GetTodoRequest, GetTodoResponse, LineageEdge, ListApprovalsRequest, ListApprovalsResponse,
    ListPlansRequest, ListPlansResponse, ListRunsRequest, ListRunsResponse, ListSystemRunsRequest,
    ListTodosRequest, ListTodosResponse, OrchestrationEvent, OrgPendingApprovalsRequest,
    OrgPendingApprovalsResponse, Plan, PlanState, RecordApprovalContinuationOutcomeRequest,
    RecordApprovalContinuationOutcomeResponse, RecordApprovalContinuationStartedRequest,
    RecordApprovalContinuationStartedResponse, RecordOrchestrationEventRequest,
    RecordOrchestrationEventResponse, ResolveRunActionAuthorityRequest,
    ResolveRunActionAuthorityResponse, ResolveRunOwnerRequest, ResolveRunOwnerResponse,
    ResolveScheduledStepAuthorityRequest, ResolveScheduledStepAuthorityResponse, RunDetail,
    ScheduledStepContext, StreamRunEventsRequest, SubagentLineage, Todo, TodoState,
    TransitionPlanRequest, TransitionPlanResponse, TransitionTodoRequest, TransitionTodoResponse,
};
use mp_events::publisher::InMemoryPublisher;
use std::{
    pin::Pin,
    sync::{Arc, Mutex},
};
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{
    transport::{Endpoint, Server},
    Request as TonicRequest, Response, Status,
};
use tower::ServiceExt;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

type MockEventStream =
    Pin<Box<dyn futures::Stream<Item = Result<OrchestrationEvent, Status>> + Send>>;

/// Captured `(id, state_int, actor, comment)` tuple recorded by a transition/decide RPC.
type CapturedTransition = Arc<Mutex<Option<(String, i32, String, String)>>>;

const TEST_AUTH_KID: &str = "orchestration-http-test";
const TEST_AUTH_ISSUER: &str = "https://auth.test/model";

struct UserTokens {
    model: String,
    session: String,
    execution: String,
}

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
        std::env::set_var("EXECUTION_CORE_AUTH_AUDIENCE", "execution-core");
        Self { _jwks: jwks }
    }

    fn user_tokens(org_id: &str, user_id: &str) -> UserTokens {
        let now = token_now();
        let claims = |audience: &str| {
            serde_json::json!({
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
        UserTokens {
            model: encode_test_token(&claims("model-gateway")),
            session: encode_test_token(&claims("session-core")),
            execution: encode_test_token(&claims("execution-core")),
        }
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

#[derive(Clone, Default)]
struct CaptureState {
    list_plans_run_id: Arc<Mutex<Option<String>>>,
    get_plan_id: Arc<Mutex<Option<String>>>,
    transition_plan: CapturedTransition,
    list_todos: Arc<Mutex<Option<(String, String)>>>,
    get_todo_id: Arc<Mutex<Option<String>>>,
    transition_todo: CapturedTransition,
    list_approvals: Arc<Mutex<Option<(String, String)>>>,
    get_approval_id: Arc<Mutex<Option<String>>>,
    decide_approval: CapturedTransition,
    lineage_thread_id: Arc<Mutex<Option<String>>>,
    stream_run_id: Arc<Mutex<Option<String>>>,
}

struct MockOrchestration {
    state: CaptureState,
}

impl MockOrchestration {
    fn new() -> (Self, CaptureState) {
        let state = CaptureState::default();
        (
            Self {
                state: state.clone(),
            },
            state,
        )
    }
}

#[tonic::async_trait]
impl OrchestrationCoreService for MockOrchestration {
    type StreamRunEventsStream = MockEventStream;

    async fn list_plans(
        &self,
        request: TonicRequest<ListPlansRequest>,
    ) -> Result<Response<ListPlansResponse>, Status> {
        let request = request.into_inner();
        *self.state.list_plans_run_id.lock().unwrap() = Some(request.run_id.clone());
        Ok(Response::new(ListPlansResponse {
            plans: vec![Plan {
                id: "plan-1".into(),
                run_id: request.run_id,
                thread_id: "thread-1".into(),
                author: "user-1".into(),
                state: PlanState::Proposed as i32,
                summary: "summary".into(),
                steps: Vec::new(),
                supersedes: String::new(),
                metadata: None,
                created_at: None,
                updated_at: None,
            }],
        }))
    }

    async fn get_plan(
        &self,
        request: TonicRequest<GetPlanRequest>,
    ) -> Result<Response<GetPlanResponse>, Status> {
        let request = request.into_inner();
        *self.state.get_plan_id.lock().unwrap() = Some(request.plan_id.clone());
        Ok(Response::new(GetPlanResponse {
            plan: Some(Plan {
                id: request.plan_id,
                run_id: "run-1".into(),
                thread_id: "thread-1".into(),
                author: "user-1".into(),
                state: PlanState::Approved as i32,
                summary: "loaded".into(),
                steps: Vec::new(),
                supersedes: String::new(),
                metadata: None,
                created_at: None,
                updated_at: None,
            }),
        }))
    }

    async fn transition_plan(
        &self,
        request: TonicRequest<TransitionPlanRequest>,
    ) -> Result<Response<TransitionPlanResponse>, Status> {
        let request = request.into_inner();
        *self.state.transition_plan.lock().unwrap() = Some((
            request.plan_id.clone(),
            request.target_state,
            request.actor.clone(),
            request.reason.clone(),
        ));
        Ok(Response::new(TransitionPlanResponse {
            plan: Some(Plan {
                id: request.plan_id,
                run_id: "run-1".into(),
                thread_id: "thread-1".into(),
                author: request.actor,
                state: request.target_state,
                summary: request.reason,
                steps: Vec::new(),
                supersedes: String::new(),
                metadata: None,
                created_at: None,
                updated_at: None,
            }),
        }))
    }

    async fn list_todos(
        &self,
        request: TonicRequest<ListTodosRequest>,
    ) -> Result<Response<ListTodosResponse>, Status> {
        let request = request.into_inner();
        *self.state.list_todos.lock().unwrap() =
            Some((request.thread_id.clone(), request.run_id.clone()));
        Ok(Response::new(ListTodosResponse {
            todos: vec![Todo {
                id: "todo-1".into(),
                thread_id: request.thread_id,
                run_id: request.run_id,
                assignee: "user-1".into(),
                title: "todo".into(),
                description: "desc".into(),
                state: TodoState::Pending as i32,
                priority: 2,
                blocked_by: Vec::new(),
                metadata: None,
                created_at: None,
                updated_at: None,
                completed_at: None,
            }],
        }))
    }

    async fn get_todo(
        &self,
        request: TonicRequest<GetTodoRequest>,
    ) -> Result<Response<GetTodoResponse>, Status> {
        let request = request.into_inner();
        *self.state.get_todo_id.lock().unwrap() = Some(request.todo_id.clone());
        Ok(Response::new(GetTodoResponse {
            todo: Some(Todo {
                id: request.todo_id,
                thread_id: "thread-1".into(),
                run_id: "run-1".into(),
                assignee: "user-1".into(),
                title: "loaded".into(),
                description: String::new(),
                state: TodoState::InProgress as i32,
                priority: 2,
                blocked_by: Vec::new(),
                metadata: None,
                created_at: None,
                updated_at: None,
                completed_at: None,
            }),
        }))
    }

    async fn transition_todo(
        &self,
        request: TonicRequest<TransitionTodoRequest>,
    ) -> Result<Response<TransitionTodoResponse>, Status> {
        let request = request.into_inner();
        *self.state.transition_todo.lock().unwrap() = Some((
            request.todo_id.clone(),
            request.target_state,
            request.actor.clone(),
            request.reason.clone(),
        ));
        Ok(Response::new(TransitionTodoResponse {
            todo: Some(Todo {
                id: request.todo_id,
                thread_id: "thread-1".into(),
                run_id: "run-1".into(),
                assignee: request.actor,
                title: "updated".into(),
                description: request.reason,
                state: request.target_state,
                priority: 2,
                blocked_by: Vec::new(),
                metadata: None,
                created_at: None,
                updated_at: None,
                completed_at: None,
            }),
        }))
    }

    async fn list_approvals(
        &self,
        request: TonicRequest<ListApprovalsRequest>,
    ) -> Result<Response<ListApprovalsResponse>, Status> {
        let request = request.into_inner();
        *self.state.list_approvals.lock().unwrap() =
            Some((request.run_id.clone(), request.step_id.clone()));
        Ok(Response::new(ListApprovalsResponse {
            approvals: vec![Approval {
                id: "appr-1".into(),
                run_id: request.run_id,
                step_id: request.step_id,
                kind: 1,
                state: ApprovalState::Requested as i32,
                requested_of: "reviewer-1".into(),
                decided_by: String::new(),
                decision_reason: String::new(),
                context: None,
                requested_at: None,
                decided_at: None,
                expires_at: None,
                org_id: "org-1".into(),
            }],
        }))
    }

    async fn list_pending_approvals(
        &self,
        request: TonicRequest<OrgPendingApprovalsRequest>,
    ) -> Result<Response<OrgPendingApprovalsResponse>, Status> {
        let request = request.into_inner();
        Ok(Response::new(OrgPendingApprovalsResponse {
            approvals: vec![Approval {
                id: "appr-pending-1".into(),
                run_id: "run-1".into(),
                step_id: String::new(),
                kind: 1,
                state: ApprovalState::Requested as i32,
                requested_of: "reviewer-1".into(),
                decided_by: String::new(),
                decision_reason: String::new(),
                context: None,
                requested_at: None,
                decided_at: None,
                expires_at: None,
                org_id: if request.org_id.is_empty() {
                    "org-1".into()
                } else {
                    request.org_id
                },
            }],
        }))
    }

    async fn get_approval(
        &self,
        request: TonicRequest<GetApprovalRequest>,
    ) -> Result<Response<GetApprovalResponse>, Status> {
        let request = request.into_inner();
        *self.state.get_approval_id.lock().unwrap() = Some(request.approval_id.clone());
        Ok(Response::new(GetApprovalResponse {
            approval: Some(Approval {
                id: request.approval_id,
                run_id: "run-1".into(),
                step_id: "step-1".into(),
                kind: 1,
                state: ApprovalState::Requested as i32,
                requested_of: "reviewer-1".into(),
                decided_by: String::new(),
                decision_reason: String::new(),
                context: None,
                requested_at: None,
                decided_at: None,
                expires_at: None,
                org_id: "org-1".into(),
            }),
        }))
    }

    async fn create_approval(
        &self,
        request: TonicRequest<CreateApprovalRequest>,
    ) -> Result<Response<CreateApprovalResponse>, Status> {
        let request = request.into_inner();
        Ok(Response::new(CreateApprovalResponse {
            approval: Some(Approval {
                id: "appr-mock".into(),
                run_id: request.run_id,
                step_id: request.step_id,
                kind: request.kind,
                state: 1,
                requested_of: request.requested_of,
                decided_by: String::new(),
                decision_reason: String::new(),
                context: None,
                requested_at: None,
                decided_at: None,
                expires_at: None,
                org_id: request.org_id,
            }),
        }))
    }

    async fn decide_approval(
        &self,
        request: TonicRequest<DecideApprovalRequest>,
    ) -> Result<Response<DecideApprovalResponse>, Status> {
        let request = request.into_inner();
        *self.state.decide_approval.lock().unwrap() = Some((
            request.approval_id.clone(),
            request.decision,
            request.decided_by.clone(),
            request.decision_reason.clone(),
        ));
        Ok(Response::new(DecideApprovalResponse {
            approval: Some(Approval {
                id: request.approval_id,
                run_id: "run-1".into(),
                step_id: "step-1".into(),
                kind: 1,
                state: request.decision,
                requested_of: "reviewer-1".into(),
                decided_by: request.decided_by,
                decision_reason: request.decision_reason,
                context: None,
                requested_at: None,
                decided_at: None,
                expires_at: None,
                org_id: "org-1".into(),
            }),
        }))
    }

    async fn claim_approval_deliveries(
        &self,
        _: TonicRequest<ClaimApprovalDeliveriesRequest>,
    ) -> Result<Response<ClaimApprovalDeliveriesResponse>, Status> {
        Err(Status::unimplemented("not needed in this test"))
    }

    async fn get_approval_continuation(
        &self,
        _: TonicRequest<GetApprovalContinuationRequest>,
    ) -> Result<Response<GetApprovalContinuationResponse>, Status> {
        Err(Status::unimplemented("not needed in this test"))
    }

    async fn record_approval_continuation_started(
        &self,
        _: TonicRequest<RecordApprovalContinuationStartedRequest>,
    ) -> Result<Response<RecordApprovalContinuationStartedResponse>, Status> {
        Err(Status::unimplemented("not needed in this test"))
    }

    async fn record_approval_continuation_outcome(
        &self,
        _: TonicRequest<RecordApprovalContinuationOutcomeRequest>,
    ) -> Result<Response<RecordApprovalContinuationOutcomeResponse>, Status> {
        Err(Status::unimplemented("not needed in this test"))
    }

    async fn acknowledge_approval_delivery(
        &self,
        _: TonicRequest<AcknowledgeApprovalDeliveryRequest>,
    ) -> Result<Response<AcknowledgeApprovalDeliveryResponse>, Status> {
        Err(Status::unimplemented("not needed in this test"))
    }

    async fn get_run_proof_bundle(
        &self,
        request: TonicRequest<mp_contracts::model_plane::v1::GetRunProofBundleRequest>,
    ) -> Result<Response<mp_contracts::model_plane::v1::GetRunProofBundleResponse>, Status> {
        let request = request.into_inner();
        Ok(Response::new(
            mp_contracts::model_plane::v1::GetRunProofBundleResponse {
                bundle: Some(mp_contracts::model_plane::v1::RunProofBundle {
                    bundle_version: 1,
                    run_id: request.run_id,
                    org_id: request.org_id,
                    ..Default::default()
                }),
            },
        ))
    }

    async fn get_verification_metrics(
        &self,
        _: TonicRequest<mp_contracts::model_plane::v1::GetVerificationMetricsRequest>,
    ) -> Result<Response<mp_contracts::model_plane::v1::GetVerificationMetricsResponse>, Status>
    {
        Err(Status::unimplemented(
            "get_verification_metrics not needed in this test",
        ))
    }

    async fn get_subagent_lineage(
        &self,
        request: TonicRequest<GetSubagentLineageRequest>,
    ) -> Result<Response<GetSubagentLineageResponse>, Status> {
        let request = request.into_inner();
        *self.state.lineage_thread_id.lock().unwrap() = Some(request.thread_id.clone());
        Ok(Response::new(GetSubagentLineageResponse {
            lineage: Some(SubagentLineage {
                thread_id: request.thread_id,
                max_depth: 1,
                edges: vec![LineageEdge {
                    parent_run_id: "run-parent".into(),
                    child_run_id: "run-child".into(),
                    role: 1,
                    spawned_at: None,
                }],
            }),
        }))
    }

    async fn attach_subagent(
        &self,
        _: TonicRequest<mp_contracts::model_plane::v1::AttachSubagentRequest>,
    ) -> Result<Response<mp_contracts::model_plane::v1::AttachSubagentResponse>, Status> {
        Err(Status::unimplemented(
            "attach_subagent not needed in this test",
        ))
    }

    async fn record_orchestration_event(
        &self,
        _: TonicRequest<mp_contracts::model_plane::v1::RecordOrchestrationEventRequest>,
    ) -> Result<Response<mp_contracts::model_plane::v1::RecordOrchestrationEventResponse>, Status>
    {
        Ok(Response::new(
            mp_contracts::model_plane::v1::RecordOrchestrationEventResponse {
                event_id: "evt-mock".into(),
            },
        ))
    }

    async fn stream_run_events(
        &self,
        request: TonicRequest<StreamRunEventsRequest>,
    ) -> Result<Response<Self::StreamRunEventsStream>, Status> {
        let request = request.into_inner();
        *self.state.stream_run_id.lock().unwrap() = Some(request.run_id);
        Ok(Response::new(Box::pin(futures::stream::iter(vec![Ok(
            OrchestrationEvent {
                event_id: "evt-1".into(),
                at: None,
                event: Some(orchestration_event::Event::PlanTransitioned(
                    orchestration_event::PlanTransitioned {
                        plan_id: "plan-1".into(),
                        run_id: "run-1".into(),
                        from: PlanState::Draft as i32,
                        to: PlanState::Approved as i32,
                    },
                )),
            },
        )]))))
    }
}

async fn spawn_orchestration_mock(
    svc: MockOrchestration,
) -> OrchestrationCoreServiceClient<tonic::transport::Channel> {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        Server::builder()
            .add_service(OrchestrationCoreServiceServer::new(svc))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .ok();
    });
    let channel = Endpoint::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .unwrap();
    OrchestrationCoreServiceClient::new(channel)
}

fn make_state(
    orchestration_client: OrchestrationCoreServiceClient<tonic::transport::Channel>,
) -> AppState {
    let mut state = AppState::new();
    state.orchestration_client = orchestration_client;
    state
}

struct MockRunOwner;

#[tonic::async_trait]
impl RunService for MockRunOwner {
    async fn get_run(&self, _: TonicRequest<GetRunRequest>) -> Result<Response<RunDetail>, Status> {
        Err(Status::unimplemented("get_run not needed in test"))
    }

    async fn list_runs(
        &self,
        _: TonicRequest<ListRunsRequest>,
    ) -> Result<Response<ListRunsResponse>, Status> {
        Err(Status::unimplemented("list_runs not needed in test"))
    }

    async fn list_system_runs(
        &self,
        _: TonicRequest<ListSystemRunsRequest>,
    ) -> Result<Response<ListRunsResponse>, Status> {
        Err(Status::unimplemented("list_system_runs not needed in test"))
    }

    async fn cancel_run(
        &self,
        _: TonicRequest<CancelRunRequest>,
    ) -> Result<Response<CancelRunResponse>, Status> {
        Err(Status::unimplemented("cancel_run not needed in test"))
    }

    async fn resolve_run_owner(
        &self,
        request: TonicRequest<ResolveRunOwnerRequest>,
    ) -> Result<Response<ResolveRunOwnerResponse>, Status> {
        let bearer = request
            .metadata()
            .get("authorization")
            .and_then(|value| value.to_str().ok());
        if !bearer.is_some_and(|value| value.starts_with("Bearer ")) {
            return Err(Status::unauthenticated(
                "verified session credential required",
            ));
        }
        let request = request.into_inner();
        Ok(Response::new(ResolveRunOwnerResponse {
            authorized: request.run_id == "run-owned"
                && request.org_id == "org-owner"
                && request.user_id == "owner-user",
        }))
    }

    async fn resolve_run_action_authority(
        &self,
        _: TonicRequest<ResolveRunActionAuthorityRequest>,
    ) -> Result<Response<ResolveRunActionAuthorityResponse>, Status> {
        Err(Status::unimplemented(
            "run action authority not needed in orchestration route test",
        ))
    }

    async fn resolve_scheduled_step_authority(
        &self,
        _: TonicRequest<ResolveScheduledStepAuthorityRequest>,
    ) -> Result<Response<ResolveScheduledStepAuthorityResponse>, Status> {
        Err(Status::unimplemented(
            "scheduled step authority not needed in orchestration route test",
        ))
    }

    async fn get_scheduled_step_context(
        &self,
        _: TonicRequest<GetScheduledStepContextRequest>,
    ) -> Result<Response<ScheduledStepContext>, Status> {
        Err(Status::unimplemented(
            "scheduled step context not needed in orchestration route test",
        ))
    }
}

async fn spawn_run_owner_mock() -> RunServiceClient<tonic::transport::Channel> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind run ownership mock");
    let addr = listener.local_addr().expect("run ownership mock addr");
    tokio::spawn(async move {
        Server::builder()
            .add_service(RunServiceServer::new(MockRunOwner))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .ok();
    });
    let channel = Endpoint::from_shared(format!("http://{addr}"))
        .expect("run ownership mock endpoint")
        .connect()
        .await
        .expect("connect run ownership mock");
    RunServiceClient::new(channel)
}

async fn run_owner_state() -> (AppState, Arc<DynPublisher>) {
    let publisher = Arc::new(DynPublisher::InMemory(InMemoryPublisher::new()));
    let mut state = AppState::new();
    state.publisher = publisher.clone();
    state.run_client = spawn_run_owner_mock().await;
    (state, publisher)
}

#[tokio::test]
#[serial_test::serial]
async fn orchestration_http_routes_proxy_requests() {
    let _auth = AuthFixture::start().await;
    let tokens = AuthFixture::user_tokens("org-1", "user-1");
    let (mock, capture) = MockOrchestration::new();
    let app = build_router(make_state(spawn_orchestration_mock(mock).await), None);

    let cases = vec![
        ("GET", "/v1/orchestration/runs/run-1/plans"),
        ("GET", "/v1/orchestration/plans/plan-9"),
        (
            "GET",
            "/v1/orchestration/threads/thread-7/todos?run_id=run-7",
        ),
        ("GET", "/v1/orchestration/todos/todo-9"),
        (
            "GET",
            "/v1/orchestration/runs/run-2/approvals?step_id=step-2",
        ),
        ("GET", "/v1/orchestration/approvals/appr-9"),
        ("GET", "/v1/orchestration/threads/thread-3/lineage"),
    ];

    for (method, uri) in cases {
        let req = Request::builder()
            .method(method)
            .uri(uri)
            .header(AUTHORIZATION, format!("Bearer {}", tokens.model))
            .header(
                "x-session-authorization",
                format!("Bearer {}", tokens.session),
            )
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        assert_eq!(
            status,
            StatusCode::OK,
            "uri={uri}, body={}",
            String::from_utf8_lossy(&body)
        );
    }

    assert_eq!(
        *capture.list_plans_run_id.lock().unwrap(),
        Some("run-1".into())
    );
    assert_eq!(*capture.get_plan_id.lock().unwrap(), Some("plan-9".into()));
    assert_eq!(
        *capture.list_todos.lock().unwrap(),
        Some(("thread-7".into(), "run-7".into()))
    );
    assert_eq!(*capture.get_todo_id.lock().unwrap(), Some("todo-9".into()));
    assert_eq!(
        *capture.list_approvals.lock().unwrap(),
        Some(("run-2".into(), "step-2".into()))
    );
    assert_eq!(
        *capture.get_approval_id.lock().unwrap(),
        Some("appr-9".into())
    );
    assert_eq!(
        *capture.lineage_thread_id.lock().unwrap(),
        Some("thread-3".into())
    );
}

#[tokio::test]
#[serial_test::serial]
async fn orchestration_run_events_route_relays_sse() {
    let _auth = AuthFixture::start().await;
    let tokens = AuthFixture::user_tokens("org-1", "user-1");
    let (mock, capture) = MockOrchestration::new();
    let app = build_router(make_state(spawn_orchestration_mock(mock).await), None);

    let req = Request::builder()
        .method("GET")
        .uri("/v1/runs/run-33/events")
        .header(AUTHORIZATION, format!("Bearer {}", tokens.model))
        .header(
            "x-session-authorization",
            format!("Bearer {}", tokens.session),
        )
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("event: plan_transitioned"));
    assert!(body.contains("\"plan_id\":\"plan-1\""));
    assert_eq!(
        *capture.stream_run_id.lock().unwrap(),
        Some("run-33".into())
    );
}

// ---------------------------------------------------------------------------
// A first-time "approve" must not 502. `quarantine_granted_approval_continuation`
// is a deliberate placeholder that always reports the continuation as
// unavailable for a granted approval (there is no descriptor-backed dispatcher
// yet) — but the decision itself was already durably recorded by the
// `DecideApproval` call that precedes it, so that placeholder result must
// surface as an informational field on a 200, never as a gateway failure.
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial_test::serial]
async fn decide_approval_grant_succeeds_without_a_continuation_dispatcher() {
    let _auth = AuthFixture::start().await;
    let tokens = AuthFixture::user_tokens("org-1", "user-1");
    let (mock, capture) = MockOrchestration::new();
    let app = build_router(make_state(spawn_orchestration_mock(mock).await), None);

    let req = Request::builder()
        .method("POST")
        .uri("/v1/orchestration/approvals/appr-42/decide")
        .header(AUTHORIZATION, format!("Bearer {}", tokens.model))
        .header(
            "x-session-authorization",
            format!("Bearer {}", tokens.session),
        )
        .header(
            "x-execution-authorization",
            format!("Bearer {}", tokens.execution),
        )
        .header("content-type", "application/json")
        .body(Body::from(r#"{"decision":"approve"}"#))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap_or_else(|error| {
        panic!(
            "decide response must be valid JSON: {error}, body={}",
            String::from_utf8_lossy(&body)
        )
    });

    assert_eq!(
        status,
        StatusCode::OK,
        "a granted decision must not surface as a gateway failure, body={json}"
    );
    assert_eq!(json["approval"]["state"], "APPROVAL_STATE_GRANTED");
    assert_eq!(
        json["continuation_delivery"], "pending",
        "no descriptor-backed dispatcher exists yet, so delivery is reported pending, not resumed, body={json}"
    );
    assert_eq!(
        *capture.decide_approval.lock().unwrap(),
        Some((
            "appr-42".to_owned(),
            ApprovalState::Granted as i32,
            "user-1".to_owned(),
            String::new()
        )),
        "the grant must still reach the durable store despite the placeholder"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn decide_approval_reject_has_no_continuation_delivery_field() {
    let _auth = AuthFixture::start().await;
    let tokens = AuthFixture::user_tokens("org-1", "user-1");
    let (mock, _capture) = MockOrchestration::new();
    let app = build_router(make_state(spawn_orchestration_mock(mock).await), None);

    let req = Request::builder()
        .method("POST")
        .uri("/v1/orchestration/approvals/appr-43/decide")
        .header(AUTHORIZATION, format!("Bearer {}", tokens.model))
        .header(
            "x-session-authorization",
            format!("Bearer {}", tokens.session),
        )
        .header(
            "x-execution-authorization",
            format!("Bearer {}", tokens.execution),
        )
        .header("content-type", "application/json")
        .body(Body::from(r#"{"decision":"reject"}"#))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap_or_else(|error| {
        panic!(
            "decide response must be valid JSON: {error}, body={}",
            String::from_utf8_lossy(&body)
        )
    });

    assert_eq!(status, StatusCode::OK, "body={json}");
    assert_eq!(json["approval"]["state"], "APPROVAL_STATE_DENIED");
    assert!(
        json.get("continuation_delivery").is_none(),
        "a denial never queues a continuation, body={json}"
    );
}

// ---------------------------------------------------------------------------
// Phase 6 — cross-org approval IDOR fix.
//
// A dedicated, org-aware mock (distinct from `MockOrchestration` above, whose
// approvals always report a fixed `org_id: "org-1"` regardless of the
// request) that behaves like the REAL session-core fix: `get_approval`/
// `decide_approval`/`list_approvals` only honor a request whose `org_id`
// matches the approval's home org, otherwise responding exactly as if the
// approval didn't exist. This proves model-gateway's HTTP handlers actually
// thread `Extension<Claims>.org_id` into the outgoing gRPC request — not just
// that they compile.
// ---------------------------------------------------------------------------

/// The one org that owns the single seeded approval (`appr-owned`).
const OWNER_ORG: &str = "org-owner";
const OTHER_ORG: &str = "org-intruder";

#[derive(Clone, Default)]
struct OrgScopedMock {
    /// `(org_id, decision)` from the last accepted (same-org) decide call.
    last_decision: Arc<Mutex<Option<(String, i32)>>>,
}

#[tonic::async_trait]
impl OrchestrationCoreService for OrgScopedMock {
    async fn list_plans(
        &self,
        _: TonicRequest<ListPlansRequest>,
    ) -> Result<Response<ListPlansResponse>, Status> {
        Err(Status::unimplemented("not needed in this test"))
    }
    async fn get_plan(
        &self,
        _: TonicRequest<GetPlanRequest>,
    ) -> Result<Response<GetPlanResponse>, Status> {
        Err(Status::unimplemented("not needed in this test"))
    }
    async fn transition_plan(
        &self,
        _: TonicRequest<TransitionPlanRequest>,
    ) -> Result<Response<TransitionPlanResponse>, Status> {
        Err(Status::unimplemented("not needed in this test"))
    }
    async fn list_todos(
        &self,
        _: TonicRequest<ListTodosRequest>,
    ) -> Result<Response<ListTodosResponse>, Status> {
        Err(Status::unimplemented("not needed in this test"))
    }
    async fn get_todo(
        &self,
        _: TonicRequest<GetTodoRequest>,
    ) -> Result<Response<GetTodoResponse>, Status> {
        Err(Status::unimplemented("not needed in this test"))
    }
    async fn transition_todo(
        &self,
        _: TonicRequest<TransitionTodoRequest>,
    ) -> Result<Response<TransitionTodoResponse>, Status> {
        Err(Status::unimplemented("not needed in this test"))
    }
    async fn create_approval(
        &self,
        _: TonicRequest<CreateApprovalRequest>,
    ) -> Result<Response<CreateApprovalResponse>, Status> {
        Err(Status::unimplemented("not needed in this test"))
    }
    async fn list_approvals(
        &self,
        request: TonicRequest<ListApprovalsRequest>,
    ) -> Result<Response<ListApprovalsResponse>, Status> {
        let request = request.into_inner();
        let approvals = if request.org_id.is_empty() || request.org_id == OWNER_ORG {
            vec![owned_approval(ApprovalState::Requested)]
        } else {
            Vec::new()
        };
        Ok(Response::new(ListApprovalsResponse { approvals }))
    }
    async fn list_pending_approvals(
        &self,
        _: TonicRequest<OrgPendingApprovalsRequest>,
    ) -> Result<Response<OrgPendingApprovalsResponse>, Status> {
        Err(Status::unimplemented("not needed in this test"))
    }
    async fn get_approval(
        &self,
        request: TonicRequest<GetApprovalRequest>,
    ) -> Result<Response<GetApprovalResponse>, Status> {
        let request = request.into_inner();
        let approval = if request.approval_id == "appr-owned"
            && (request.org_id.is_empty() || request.org_id == OWNER_ORG)
        {
            Some(owned_approval(ApprovalState::Requested))
        } else {
            None
        };
        Ok(Response::new(GetApprovalResponse { approval }))
    }
    async fn decide_approval(
        &self,
        request: TonicRequest<DecideApprovalRequest>,
    ) -> Result<Response<DecideApprovalResponse>, Status> {
        let request = request.into_inner();
        if request.approval_id != "appr-owned"
            || (!request.org_id.is_empty() && request.org_id != OWNER_ORG)
        {
            // Mirrors the real session-core fix: a cross-org decide is
            // rejected as not-found, never applied.
            return Err(Status::not_found("approval not found"));
        }
        *self.last_decision.lock().unwrap() = Some((request.org_id.clone(), request.decision));
        Ok(Response::new(DecideApprovalResponse {
            approval: Some(Approval {
                state: request.decision,
                decided_by: request.decided_by,
                decision_reason: request.decision_reason,
                ..owned_approval(ApprovalState::Requested)
            }),
        }))
    }
    async fn claim_approval_deliveries(
        &self,
        _: TonicRequest<ClaimApprovalDeliveriesRequest>,
    ) -> Result<Response<ClaimApprovalDeliveriesResponse>, Status> {
        Err(Status::unimplemented("not needed in this test"))
    }
    async fn get_approval_continuation(
        &self,
        _: TonicRequest<GetApprovalContinuationRequest>,
    ) -> Result<Response<GetApprovalContinuationResponse>, Status> {
        Err(Status::unimplemented("not needed in this test"))
    }
    async fn record_approval_continuation_started(
        &self,
        _: TonicRequest<RecordApprovalContinuationStartedRequest>,
    ) -> Result<Response<RecordApprovalContinuationStartedResponse>, Status> {
        Err(Status::unimplemented("not needed in this test"))
    }
    async fn record_approval_continuation_outcome(
        &self,
        _: TonicRequest<RecordApprovalContinuationOutcomeRequest>,
    ) -> Result<Response<RecordApprovalContinuationOutcomeResponse>, Status> {
        Err(Status::unimplemented("not needed in this test"))
    }
    async fn acknowledge_approval_delivery(
        &self,
        _: TonicRequest<AcknowledgeApprovalDeliveryRequest>,
    ) -> Result<Response<AcknowledgeApprovalDeliveryResponse>, Status> {
        Err(Status::unimplemented("not needed in this test"))
    }
    async fn get_run_proof_bundle(
        &self,
        _: TonicRequest<mp_contracts::model_plane::v1::GetRunProofBundleRequest>,
    ) -> Result<Response<mp_contracts::model_plane::v1::GetRunProofBundleResponse>, Status> {
        Err(Status::unimplemented("not needed in this test"))
    }
    async fn get_verification_metrics(
        &self,
        _: TonicRequest<mp_contracts::model_plane::v1::GetVerificationMetricsRequest>,
    ) -> Result<Response<mp_contracts::model_plane::v1::GetVerificationMetricsResponse>, Status>
    {
        Err(Status::unimplemented("not needed in this test"))
    }
    async fn get_subagent_lineage(
        &self,
        _: TonicRequest<GetSubagentLineageRequest>,
    ) -> Result<Response<GetSubagentLineageResponse>, Status> {
        Err(Status::unimplemented("not needed in this test"))
    }
    async fn attach_subagent(
        &self,
        _: TonicRequest<AttachSubagentRequest>,
    ) -> Result<Response<AttachSubagentResponse>, Status> {
        Err(Status::unimplemented("not needed in this test"))
    }
    type StreamRunEventsStream = MockEventStream;
    async fn stream_run_events(
        &self,
        _: TonicRequest<StreamRunEventsRequest>,
    ) -> Result<Response<Self::StreamRunEventsStream>, Status> {
        Err(Status::unimplemented("not needed in this test"))
    }
    async fn record_orchestration_event(
        &self,
        _: TonicRequest<RecordOrchestrationEventRequest>,
    ) -> Result<Response<RecordOrchestrationEventResponse>, Status> {
        Ok(Response::new(RecordOrchestrationEventResponse {
            event_id: "evt-mock".into(),
        }))
    }
}

fn owned_approval(state: ApprovalState) -> Approval {
    Approval {
        id: "appr-owned".into(),
        run_id: "run-owned".into(),
        step_id: String::new(),
        kind: 1,
        state: state as i32,
        requested_of: "reviewer-1".into(),
        decided_by: String::new(),
        decision_reason: String::new(),
        context: None,
        requested_at: None,
        decided_at: None,
        expires_at: None,
        org_id: OWNER_ORG.into(),
    }
}

async fn spawn_org_scoped_mock(
    svc: OrgScopedMock,
) -> OrchestrationCoreServiceClient<tonic::transport::Channel> {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        Server::builder()
            .add_service(OrchestrationCoreServiceServer::new(svc))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .ok();
    });
    let channel = Endpoint::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .unwrap();
    OrchestrationCoreServiceClient::new(channel)
}

#[tokio::test]
#[serial_test::serial]
async fn get_approval_is_scoped_to_the_callers_org() {
    let _auth = AuthFixture::start().await;
    let owner_tokens = AuthFixture::user_tokens(OWNER_ORG, "owner-user");
    let intruder_tokens = AuthFixture::user_tokens(OTHER_ORG, "intruder-user");
    let mock = OrgScopedMock::default();
    let app = build_router(make_state(spawn_org_scoped_mock(mock).await), None);

    // The owning org can read its own approval.
    let owner_req = Request::builder()
        .method("GET")
        .uri("/v1/orchestration/approvals/appr-owned")
        .header(AUTHORIZATION, format!("Bearer {}", owner_tokens.model))
        .header(
            "x-session-authorization",
            format!("Bearer {}", owner_tokens.session),
        )
        .body(Body::empty())
        .unwrap();
    let owner_resp = app.clone().oneshot(owner_req).await.unwrap();
    assert_eq!(
        owner_resp.status(),
        StatusCode::OK,
        "owner org must succeed"
    );

    // A different, authenticated org gets 404 — not a distinguishable
    // "forbidden" that would confirm the approval's existence.
    let intruder_req = Request::builder()
        .method("GET")
        .uri("/v1/orchestration/approvals/appr-owned")
        .header(AUTHORIZATION, format!("Bearer {}", intruder_tokens.model))
        .header(
            "x-session-authorization",
            format!("Bearer {}", intruder_tokens.session),
        )
        .body(Body::empty())
        .unwrap();
    let intruder_resp = app.oneshot(intruder_req).await.unwrap();
    assert_eq!(
        intruder_resp.status(),
        StatusCode::NOT_FOUND,
        "cross-org read must be rejected as not-found"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn decide_approval_is_scoped_to_the_callers_org() {
    let _auth = AuthFixture::start().await;
    let owner_tokens = AuthFixture::user_tokens(OWNER_ORG, "owner-user");
    let intruder_tokens = AuthFixture::user_tokens(OTHER_ORG, "intruder-user");
    let mock = OrgScopedMock::default();
    let (mock, last_decision) = (mock.clone(), mock.last_decision.clone());
    let app = build_router(make_state(spawn_org_scoped_mock(mock).await), None);

    // A different org cannot decide (approve) another org's approval.
    let intruder_req = Request::builder()
        .method("POST")
        .uri("/v1/orchestration/approvals/appr-owned/decide")
        .header(AUTHORIZATION, format!("Bearer {}", intruder_tokens.model))
        .header(
            "x-session-authorization",
            format!("Bearer {}", intruder_tokens.session),
        )
        .header(
            "x-execution-authorization",
            format!("Bearer {}", intruder_tokens.execution),
        )
        .header("content-type", "application/json")
        .body(Body::from(r#"{"decision":"approve"}"#))
        .unwrap();
    let intruder_resp = app.clone().oneshot(intruder_req).await.unwrap();
    assert_eq!(
        intruder_resp.status(),
        StatusCode::NOT_FOUND,
        "cross-org decide must be rejected as not-found"
    );
    assert!(
        last_decision.lock().unwrap().is_none(),
        "the cross-org decide must never have reached the durable store"
    );

    // The owning org can still decide its own approval. Use denial here so
    // this tenant-boundary test does not also depend on an execution-core
    // resume mock; granted/resume behavior has its own contract tests.
    let owner_req = Request::builder()
        .method("POST")
        .uri("/v1/orchestration/approvals/appr-owned/decide")
        .header(AUTHORIZATION, format!("Bearer {}", owner_tokens.model))
        .header(
            "x-session-authorization",
            format!("Bearer {}", owner_tokens.session),
        )
        .header(
            "x-execution-authorization",
            format!("Bearer {}", owner_tokens.execution),
        )
        .header("content-type", "application/json")
        .body(Body::from(r#"{"decision":"reject"}"#))
        .unwrap();
    let owner_resp = app.oneshot(owner_req).await.unwrap();
    assert_eq!(
        owner_resp.status(),
        StatusCode::OK,
        "owner org must succeed"
    );
    let (decided_org, decision) = last_decision.lock().unwrap().clone().unwrap();
    assert_eq!(decided_org, OWNER_ORG);
    assert_eq!(decision, ApprovalState::Denied as i32);
}

#[tokio::test]
#[serial_test::serial]
async fn list_approvals_is_scoped_to_the_callers_org() {
    let _auth = AuthFixture::start().await;
    let owner_tokens = AuthFixture::user_tokens(OWNER_ORG, "owner-user");
    let intruder_tokens = AuthFixture::user_tokens(OTHER_ORG, "intruder-user");
    let mock = OrgScopedMock::default();
    let app = build_router(make_state(spawn_org_scoped_mock(mock).await), None);

    let owner_req = Request::builder()
        .method("GET")
        .uri("/v1/orchestration/runs/run-owned/approvals")
        .header(AUTHORIZATION, format!("Bearer {}", owner_tokens.model))
        .header(
            "x-session-authorization",
            format!("Bearer {}", owner_tokens.session),
        )
        .body(Body::empty())
        .unwrap();
    let owner_resp = app.clone().oneshot(owner_req).await.unwrap();
    assert_eq!(owner_resp.status(), StatusCode::OK);
    let owner_body = to_bytes(owner_resp.into_body(), usize::MAX).await.unwrap();
    let owner_json: serde_json::Value = serde_json::from_slice(&owner_body).unwrap();
    assert_eq!(owner_json["approvals"].as_array().unwrap().len(), 1);

    let intruder_req = Request::builder()
        .method("GET")
        .uri("/v1/orchestration/runs/run-owned/approvals")
        .header(AUTHORIZATION, format!("Bearer {}", intruder_tokens.model))
        .header(
            "x-session-authorization",
            format!("Bearer {}", intruder_tokens.session),
        )
        .body(Body::empty())
        .unwrap();
    let intruder_resp = app.oneshot(intruder_req).await.unwrap();
    assert_eq!(intruder_resp.status(), StatusCode::OK);
    let intruder_body = to_bytes(intruder_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let intruder_json: serde_json::Value = serde_json::from_slice(&intruder_body).unwrap();
    assert_eq!(
        intruder_json["approvals"].as_array().unwrap().len(),
        0,
        "a different org must see no approvals for someone else's run"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn http_run_event_routes_require_a_durable_owner_before_publish() {
    let _auth = AuthFixture::start().await;
    let owner = AuthFixture::user_tokens("org-owner", "owner-user");
    let same_org_other_user = AuthFixture::user_tokens("org-owner", "user-other");
    let other_org = AuthFixture::user_tokens("org-other", "user-other");
    let (state, publisher) = run_owner_state().await;
    let app = build_router(state, None);

    let cancel = Request::builder()
        .method("POST")
        .uri("/v1/orchestration/runs/run-owned/cancel")
        .header(
            AUTHORIZATION,
            format!("Bearer {}", same_org_other_user.model),
        )
        .header(
            "x-session-authorization",
            format!("Bearer {}", same_org_other_user.session),
        )
        .body(Body::empty())
        .unwrap();
    let cancel = app.clone().oneshot(cancel).await.unwrap();
    assert_eq!(cancel.status(), StatusCode::FORBIDDEN);

    let feedback = Request::builder()
        .method("POST")
        .uri("/v1/feedback")
        .header(AUTHORIZATION, format!("Bearer {}", other_org.model))
        .header(
            "x-session-authorization",
            format!("Bearer {}", other_org.session),
        )
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"run_id":"run-owned","skill_id":"skill-test","rating":"good"}"#,
        ))
        .unwrap();
    let feedback = app.clone().oneshot(feedback).await.unwrap();
    assert_eq!(feedback.status(), StatusCode::FORBIDDEN);

    let resume = Request::builder()
        .method("POST")
        .uri("/v1/orchestration/runs/run-owned/resume")
        .header(
            AUTHORIZATION,
            format!("Bearer {}", same_org_other_user.model),
        )
        .header(
            "x-session-authorization",
            format!("Bearer {}", same_org_other_user.session),
        )
        .header(
            "x-execution-authorization",
            format!("Bearer {}", same_org_other_user.execution),
        )
        .body(Body::empty())
        .unwrap();
    let resume = app.clone().oneshot(resume).await.unwrap();
    assert_eq!(resume.status(), StatusCode::FORBIDDEN);
    assert!(
        publisher.drain().is_empty(),
        "a foreign run must not publish cancellation, feedback, or resume events"
    );

    let owner_cancel = Request::builder()
        .method("POST")
        .uri("/v1/orchestration/runs/run-owned/cancel")
        .header(AUTHORIZATION, format!("Bearer {}", owner.model))
        .header(
            "x-session-authorization",
            format!("Bearer {}", owner.session),
        )
        .body(Body::empty())
        .unwrap();
    let owner_cancel = app.clone().oneshot(owner_cancel).await.unwrap();
    assert_eq!(owner_cancel.status(), StatusCode::OK);

    let owner_feedback = Request::builder()
        .method("POST")
        .uri("/v1/feedback")
        .header(AUTHORIZATION, format!("Bearer {}", owner.model))
        .header(
            "x-session-authorization",
            format!("Bearer {}", owner.session),
        )
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"run_id":"run-owned","skill_id":"skill-test","rating":"good"}"#,
        ))
        .unwrap();
    let owner_feedback = app.oneshot(owner_feedback).await.unwrap();
    assert_eq!(owner_feedback.status(), StatusCode::OK);

    let events = publisher.drain();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].1.event_type, "RUN_CANCEL_REQUESTED");
    assert_eq!(events[1].1.event_type, "FEEDBACK_RATED");
}
