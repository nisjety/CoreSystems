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
    AcknowledgeApprovalDeliveryRequest, AcknowledgeApprovalDeliveryResponse,
    AnswerRunQuestionRequest, AnswerRunQuestionResponse, Approval, ApprovalState,
    AttachSubagentRequest, AttachSubagentResponse, CancelRunRequest,
    CreateRunQuestionRequest, CreateRunQuestionResponse, ListRunQuestionsRequest,
    ListRunQuestionsResponse, QuestionItem, QuestionOption, QuestionState, RunQuestion,
    CancelRunResponse, ClaimApprovalDeliveriesRequest, ClaimApprovalDeliveriesResponse,
    ConsumeApprovalRequest, ConsumeApprovalResponse,
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
    ResolveScheduledStepAuthorityRequest, ResolveScheduledStepAuthorityResponse,
    ResolveThreadOwnerRequest, ResolveThreadOwnerResponse, RunDetail, ScheduledStepContext,
    StreamRunEventsRequest, SubagentLineage, Todo, TodoState, TransitionPlanRequest,
    TransitionPlanResponse, TransitionTodoRequest, TransitionTodoResponse,
};
use mp_events::publisher::InMemoryPublisher;
use std::{
    pin::Pin,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
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

#[path = "support/offline_downstreams.rs"]
mod offline_downstreams;

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
    /// `(thread_id, summary, step_count)` from the last `ProposePlan` call
    /// (plan-gate §2.1).
    propose_plan: Arc<Mutex<Option<(String, String, usize)>>>,
    /// plan-gate §2.3: the thread `GetPlan` reports the plan as belonging to.
    /// `None` answers `thread-1`, the thread the run-owner mock reports.
    plan_thread_id: Arc<Mutex<Option<String>>>,
    /// plan-gate §2.3: make `TransitionPlan` refuse, as session-core does for
    /// a plan that was already decided.
    fail_transition: Arc<AtomicBool>,
    /// Every `GetPlan` / `TransitionPlan` call, in arrival order, so a test
    /// can prove the plan was CHECKED before it was decided.
    plan_calls: Arc<Mutex<Vec<String>>>,
    /// `(step_id, title, removed)` of the last `TransitionPlan`'s edits.
    transition_edits: Arc<Mutex<Vec<(String, String, bool)>>>,
    list_todos: Arc<Mutex<Option<(String, String)>>>,
    get_todo_id: Arc<Mutex<Option<String>>>,
    transition_todo: CapturedTransition,
    list_approvals: Arc<Mutex<Option<(String, String)>>>,
    get_approval_id: Arc<Mutex<Option<String>>>,
    decide_approval: CapturedTransition,
    /// EP-10-S4: `(patch_json, base_fingerprint)` of the last decide that carried an edit.
    decide_edit: Arc<Mutex<Option<(String, String)>>>,
    lineage_thread_id: Arc<Mutex<Option<String>>>,
    stream_run_id: Arc<Mutex<Option<String>>>,
    /// `(run_id, org_id)` of the last `ListRunQuestions` call.
    list_questions: Arc<Mutex<Option<(String, String)>>>,
    /// The last `AnswerRunQuestion` request.
    answer_question: Arc<Mutex<Option<AnswerRunQuestionRequest>>>,
    /// Every question RPC that reached the double, to prove a refused caller never did.
    question_calls: Arc<AtomicUsize>,
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
    // Main's RPCs (merged 2026-11-02); this double does not serve them.
    async fn reroute_approval(
        &self,
        _: tonic::Request<mp_contracts::model_plane::v1::RerouteApprovalRequest>,
    ) -> Result<tonic::Response<mp_contracts::model_plane::v1::RerouteApprovalResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("reroute_approval is not served by this double"))
    }

    async fn claim_once_approval(
        &self,
        _: tonic::Request<mp_contracts::model_plane::v1::ClaimOnceApprovalRequest>,
    ) -> Result<tonic::Response<mp_contracts::model_plane::v1::ClaimOnceApprovalResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("claim_once_approval is not served by this double"))
    }

    async fn hold_quarantined_output(
        &self,
        _: tonic::Request<mp_contracts::model_plane::v1::HoldQuarantinedOutputRequest>,
    ) -> Result<tonic::Response<mp_contracts::model_plane::v1::HoldQuarantinedOutputResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("hold_quarantined_output is not served by this double"))
    }

    async fn claim_quarantine_release(
        &self,
        _: tonic::Request<mp_contracts::model_plane::v1::ClaimQuarantineReleaseRequest>,
    ) -> Result<tonic::Response<mp_contracts::model_plane::v1::ClaimQuarantineReleaseResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("claim_quarantine_release is not served by this double"))
    }

    async fn get_quarantine_preview(
        &self,
        _: tonic::Request<mp_contracts::model_plane::v1::GetQuarantinePreviewRequest>,
    ) -> Result<tonic::Response<mp_contracts::model_plane::v1::GetQuarantinePreviewResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("get_quarantine_preview is not served by this double"))
    }

    async fn get_approval_preview(
        &self,
        _: tonic::Request<mp_contracts::model_plane::v1::GetApprovalPreviewRequest>,
    ) -> Result<tonic::Response<mp_contracts::model_plane::v1::GetApprovalPreviewResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("get_approval_preview is not served by this double"))
    }

    async fn get_skipped_assist_authority(
        &self,
        _: tonic::Request<mp_contracts::model_plane::v1::GetSkippedAssistAuthorityRequest>,
    ) -> Result<tonic::Response<mp_contracts::model_plane::v1::GetSkippedAssistAuthorityResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("get_skipped_assist_authority is not served by this double"))
    }

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
        self.state.plan_calls.lock().unwrap().push("get_plan".into());
        let thread_id = self
            .state
            .plan_thread_id
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| "thread-1".into());
        Ok(Response::new(GetPlanResponse {
            plan: Some(Plan {
                id: request.plan_id,
                run_id: "run-1".into(),
                thread_id,
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
        self.state
            .plan_calls
            .lock()
            .unwrap()
            .push("transition_plan".into());
        if self.state.fail_transition.load(Ordering::SeqCst) {
            return Err(Status::failed_precondition(
                "a plan can only be approved from PLAN_STATE_PROPOSED",
            ));
        }
        *self.state.transition_edits.lock().unwrap() = request
            .edited_steps
            .iter()
            .map(|e| (e.step_id.clone(), e.title.clone(), e.removed))
            .collect();
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

    async fn propose_plan(
        &self,
        request: TonicRequest<mp_contracts::model_plane::v1::ProposePlanRequest>,
    ) -> Result<
        Response<mp_contracts::model_plane::v1::ProposePlanResponse>,
        Status,
    > {
        let request = request.into_inner();
        *self.state.propose_plan.lock().unwrap() = Some((
            request.thread_id.clone(),
            request.summary.clone(),
            request.steps.len(),
        ));
        let steps: Vec<mp_contracts::model_plane::v1::PlanStep> = request
            .steps
            .iter()
            .enumerate()
            .map(|(i, s)| mp_contracts::model_plane::v1::PlanStep {
                id: format!("step-{i}"),
                title: s.title.clone(),
                operation: s.operation.clone(),
                state: mp_contracts::model_plane::v1::PlanStepState::Pending as i32,
                created_at: None,
                updated_at: None,
                detail: String::new(),
            })
            .collect();
        Ok(Response::new(
            mp_contracts::model_plane::v1::ProposePlanResponse {
                plan: Some(Plan {
                    id: "plan-proposed-1".into(),
                    run_id: request.run_id,
                    thread_id: request.thread_id,
                    author: request.user_id,
                    state: PlanState::Proposed as i32,
                    summary: request.summary,
                    steps,
                    supersedes: request.supersedes,
                    metadata: None,
                    created_at: None,
                    updated_at: None,
                }),
            },
        ))
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
                consumed_at: None,
                tool_key: String::new(),
                action_label: String::new(),
                continuation: None,
                continuation_mode: String::new(),
                cause: None,
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
                ..Default::default()
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
                consumed_at: None,
                tool_key: String::new(),
                action_label: String::new(),
                continuation: None,
                continuation_mode: String::new(),
                cause: None,
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
                ..Default::default()
            }],
        }))
    }

    async fn get_approval(
        &self,
        request: TonicRequest<GetApprovalRequest>,
    ) -> Result<Response<GetApprovalResponse>, Status> {
        let request = request.into_inner();
        *self.state.get_approval_id.lock().unwrap() = Some(request.approval_id.clone());
        // An id that starts with `appr-stepup` stands for a cause that needs a fresh step-up.
        let cause = request.approval_id.starts_with("appr-stepup").then(|| {
            mp_contracts::model_plane::v1::ApprovalCause {
                code: "provider_write".into(),
                step_up_required: true,
                ..Default::default()
            }
        });
        Ok(Response::new(GetApprovalResponse {
            approval: Some(Approval {
                consumed_at: None,
                tool_key: String::new(),
                action_label: String::new(),
                continuation: None,
                continuation_mode: String::new(),
                cause,
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
                ..Default::default()
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
                consumed_at: None,
                tool_key: String::new(),
                action_label: String::new(),
                continuation: None,
                continuation_mode: String::new(),
                cause: None,
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
                ..Default::default()
            }),
        }))
    }

    async fn consume_approval(
        &self,
        _: TonicRequest<ConsumeApprovalRequest>,
    ) -> Result<Response<ConsumeApprovalResponse>, Status> {
        Err(Status::unimplemented("consume_approval not needed in test"))
    }

    async fn decide_approval(
        &self,
        request: TonicRequest<DecideApprovalRequest>,
    ) -> Result<Response<DecideApprovalResponse>, Status> {
        let request = request.into_inner();
        // An id that starts with `appr-loop` stands for a loop-owned approval.
        let continuation_mode = if request.approval_id.starts_with("appr-loop") {
            "loop".to_owned()
        } else {
            String::new()
        };
        *self.state.decide_approval.lock().unwrap() = Some((
            request.approval_id.clone(),
            request.decision,
            request.decided_by.clone(),
            request.decision_reason.clone(),
        ));
        if let Some(edit) = request.edit.as_ref() {
            *self.state.decide_edit.lock().unwrap() =
                Some((edit.patch_json.clone(), edit.base_fingerprint.clone()));
            // Session Core answers a base it no longer holds with its closed code.
            if edit.base_fingerprint == "f".repeat(64) {
                return Err(Status::aborted("approval_changed"));
            }
        }
        Ok(Response::new(DecideApprovalResponse {
            approval: Some(Approval {
                consumed_at: None,
                tool_key: String::new(),
                action_label: String::new(),
                continuation: None,
                continuation_mode,
                cause: None,
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
                ..Default::default()
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
                    ..Default::default()
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

    async fn create_run_question(
        &self,
        _: TonicRequest<CreateRunQuestionRequest>,
    ) -> Result<Response<CreateRunQuestionResponse>, Status> {
        Err(Status::unimplemented(
            "the gateway serves no question create route",
        ))
    }

    async fn list_run_questions(
        &self,
        request: TonicRequest<ListRunQuestionsRequest>,
    ) -> Result<Response<ListRunQuestionsResponse>, Status> {
        self.state.question_calls.fetch_add(1, Ordering::SeqCst);
        let request = request.into_inner();
        *self.state.list_questions.lock().unwrap() =
            Some((request.run_id.clone(), request.org_id.clone()));
        Ok(Response::new(ListRunQuestionsResponse {
            questions: vec![scripted_question("q-1", &request.run_id, QuestionState::Requested)],
        }))
    }

    /// The question id picks the outcome: each typed refusal Session Core can give.
    async fn answer_run_question(
        &self,
        request: TonicRequest<AnswerRunQuestionRequest>,
    ) -> Result<Response<AnswerRunQuestionResponse>, Status> {
        self.state.question_calls.fetch_add(1, Ordering::SeqCst);
        let request = request.into_inner();
        *self.state.answer_question.lock().unwrap() = Some(request.clone());
        match request.question_id.as_str() {
            "q-missing" => Err(Status::not_found("question_not_found")),
            "q-expired" => Err(Status::failed_precondition("question_expired")),
            "q-done" => Err(Status::already_exists("question_already_answered")),
            "q-bad" => Err(Status::invalid_argument(
                "invalid_answer: answers[0].option_ids[0]: unknown_option",
            )),
            _ => Ok(Response::new(AnswerRunQuestionResponse {
                question: Some(scripted_question(
                    &request.question_id,
                    "run-1",
                    if request.skip {
                        QuestionState::Skipped
                    } else {
                        QuestionState::Answered
                    },
                )),
            })),
        }
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

/// A stored question as Session Core would hand it back: one single-choice item.
fn scripted_question(id: &str, run_id: &str, state: QuestionState) -> RunQuestion {
    RunQuestion {
        id: id.into(),
        run_id: run_id.into(),
        thread_id: "thread-1".into(),
        step_id: "step-1".into(),
        org_id: "org-1".into(),
        user_id: "user-1".into(),
        state: state as i32,
        delivery: "next_turn".into(),
        items: vec![QuestionItem {
            id: "scope".into(),
            kind: "single".into(),
            prompt: "Which branch?".into(),
            options: vec![
                QuestionOption {
                    id: "main".into(),
                    label: "main".into(),
                    description: String::new(),
                },
                QuestionOption {
                    id: "feature".into(),
                    label: "A feature branch".into(),
                    description: "New work".into(),
                },
            ],
            allow_other: false,
        }],
        requested_at: Some(prost_types::Timestamp {
            seconds: 1_790_000_000,
            nanos: 0,
        }),
        expires_at: Some(prost_types::Timestamp {
            seconds: 1_790_086_400,
            nanos: 0,
        }),
        answered_at: None,
        answers: Vec::new(),
        answered_message_id: String::new(),
        origin: String::new(),
        origin_label: String::new(),
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
    let mut state = offline_downstreams::offline_state();
    state.orchestration_client = orchestration_client;
    state
}

/// Session Core's `RunService` as the run routes see it. Ownership resolves for
/// `run-owned` and its owner alone, and `CancelRun` answers only the owner's
/// forwarded session credential. Every `CancelRun` that arrives is counted, so
/// a test can prove a rejected caller never reached the durable cancel.
struct MockRunOwner {
    cancel_bearer: String,
    cancel_calls: Arc<AtomicUsize>,
}

#[tonic::async_trait]
impl RunService for MockRunOwner {
    // Main's RPCs (merged 2026-11-02); this double does not serve them.
    async fn resolve_approval_authority(
        &self,
        _: tonic::Request<mp_contracts::model_plane::v1::ResolveApprovalAuthorityRequest>,
    ) -> Result<tonic::Response<mp_contracts::model_plane::v1::ResolveApprovalAuthorityResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("resolve_approval_authority is not served by this double"))
    }

    async fn get_run(
        &self,
        request: TonicRequest<GetRunRequest>,
    ) -> Result<Response<RunDetail>, Status> {
        // Only `plan_approval` resolves a run's thread through this route
        // today: the plan-gate tests need it to say which conversation the
        // run belongs to. Any other run is not this mock's to describe.
        // `run-owned` is still in plan mode; `run-executed` has already left it.
        let request = request.into_inner();
        if !matches!(request.run_id.as_str(), "run-owned" | "run-executed") {
            return Err(Status::not_found("run not found"));
        }
        let mode = if request.run_id == "run-owned" { "plan" } else { "execute" };
        Ok(Response::new(RunDetail {
            run_id: request.run_id,
            thread_id: "thread-1".into(),
            mode: mode.into(),
            ..Default::default()
        }))
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
        request: TonicRequest<CancelRunRequest>,
    ) -> Result<Response<CancelRunResponse>, Status> {
        self.cancel_calls.fetch_add(1, Ordering::SeqCst);
        let bearer = request
            .metadata()
            .get("authorization")
            .and_then(|value| value.to_str().ok());
        if bearer != Some(self.cancel_bearer.as_str()) {
            return Err(Status::unauthenticated(
                "verified session credential required",
            ));
        }
        let request = request.into_inner();
        if request.run_id != "run-owned" {
            return Err(Status::not_found("run not found"));
        }
        if request.reason != "user_requested" {
            return Err(Status::invalid_argument("unexpected cancel reason"));
        }
        Ok(Response::new(CancelRunResponse {
            cancelled: true,
            receipt_id: "receipt-cancel-1".into(),
        }))
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
            authorized: matches!(request.run_id.as_str(), "run-owned" | "run-executed")
                && request.org_id == "org-owner"
                && request.user_id == "owner-user",
        }))
    }

    async fn resolve_thread_owner(
        &self,
        _: TonicRequest<ResolveThreadOwnerRequest>,
    ) -> Result<Response<ResolveThreadOwnerResponse>, Status> {
        Err(Status::unimplemented(
            "thread ownership not needed in orchestration route test",
        ))
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

    async fn authorize_room_run_attach(
        &self,
        _: TonicRequest<mp_contracts::model_plane::v1::AuthorizeRoomRunAttachRequest>,
    ) -> Result<Response<mp_contracts::model_plane::v1::AuthorizeRoomRunAttachResponse>, Status> {
        Err(Status::unimplemented("room run attach not needed in this test"))
    }

    async fn cancel_run_as_room_manager(
        &self,
        _: TonicRequest<mp_contracts::model_plane::v1::CancelRunAsRoomManagerRequest>,
    ) -> Result<Response<mp_contracts::model_plane::v1::CancelRunAsRoomManagerResponse>, Status> {
        Err(Status::unimplemented("room run stop not needed in this test"))
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

async fn spawn_run_owner_mock(owner: MockRunOwner) -> RunServiceClient<tonic::transport::Channel> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind run ownership mock");
    let addr = listener.local_addr().expect("run ownership mock addr");
    tokio::spawn(async move {
        Server::builder()
            .add_service(RunServiceServer::new(owner))
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

/// `owner_session_token` is the only credential the mock's `CancelRun` accepts,
/// which pins that the route forwards the verified session bearer.
async fn run_owner_state(
    owner_session_token: &str,
) -> (AppState, Arc<DynPublisher>, Arc<AtomicUsize>) {
    let publisher = Arc::new(DynPublisher::InMemory(InMemoryPublisher::new()));
    let cancel_calls = Arc::new(AtomicUsize::new(0));
    let mut state = offline_downstreams::offline_state();
    state.publisher = publisher.clone();
    state.run_client = spawn_run_owner_mock(MockRunOwner {
        cancel_bearer: format!("Bearer {owner_session_token}"),
        cancel_calls: cancel_calls.clone(),
    })
    .await;
    (state, publisher, cancel_calls)
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
async fn decide_approval_grant_of_a_loop_owned_approval_reports_awaiting_continue() {
    let _auth = AuthFixture::start().await;
    let tokens = AuthFixture::user_tokens("org-1", "user-1");
    let (mock, _capture) = MockOrchestration::new();
    let app = build_router(make_state(spawn_orchestration_mock(mock).await), None);

    let req = Request::builder()
        .method("POST")
        .uri("/v1/orchestration/approvals/appr-loop-1/decide")
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
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json["continuation_delivery"], "awaiting_continue",
        "a loop-owned approval is consumed by the loop, not delivered by the worker, body={json}"
    );
}

// ---------------------------------------------------------------------------
// EP-10-S4: approve with edits. The switch is off by default; on, the edit reaches Session Core
// with the grant and its closed refusals come back typed.
// ---------------------------------------------------------------------------

const EDIT_BASE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn edited_decide_request(tokens: &UserTokens, approval_id: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(format!("/v1/orchestration/approvals/{approval_id}/decide"))
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
        .body(Body::from(body.to_owned()))
        .unwrap()
}

fn edit_body(base: &str, scope: &str) -> String {
    serde_json::json!({
        "decision": "approve",
        "scope": scope,
        "edit": { "patch": { "subject": "New subject" }, "base_fingerprint": base },
    })
    .to_string()
}

async fn json_of(resp: axum::response::Response) -> (StatusCode, serde_json::Value) {
    let status = resp.status();
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null))
}

#[tokio::test]
#[serial_test::serial]
async fn edit_off_by_default_is_forbidden() {
    let _auth = AuthFixture::start().await;
    std::env::remove_var("APPROVAL_EDIT_ENABLED");
    let tokens = AuthFixture::user_tokens("org-1", "user-1");
    let (mock, capture) = MockOrchestration::new();
    let app = build_router(make_state(spawn_orchestration_mock(mock).await), None);

    let resp = app
        .oneshot(edited_decide_request(&tokens, "appr-42", &edit_body(EDIT_BASE, "once")))
        .await
        .unwrap();
    let (status, json) = json_of(resp).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body={json}");
    assert_eq!(json["error"]["code"], "approval_edit_disabled");
    assert!(json["error"]["details"].is_object());
    assert!(
        capture.decide_approval.lock().unwrap().is_none()
            && capture.get_approval_id.lock().unwrap().is_none(),
        "nothing reaches Session Core while the switch is off"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn edit_is_forwarded_with_the_grant_when_the_switch_is_on() {
    let _auth = AuthFixture::start().await;
    std::env::set_var("APPROVAL_EDIT_ENABLED", "true");
    let tokens = AuthFixture::user_tokens("org-1", "user-1");
    let (mock, capture) = MockOrchestration::new();
    let app = build_router(make_state(spawn_orchestration_mock(mock).await), None);

    let resp = app
        .oneshot(edited_decide_request(&tokens, "appr-42", &edit_body(EDIT_BASE, "once")))
        .await
        .unwrap();
    std::env::remove_var("APPROVAL_EDIT_ENABLED");
    let (status, json) = json_of(resp).await;
    assert_eq!(status, StatusCode::OK, "body={json}");
    let (patch, base) = capture.decide_edit.lock().unwrap().clone().expect("the edit reached it");
    assert_eq!(base, EDIT_BASE);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&patch).unwrap(),
        serde_json::json!({ "subject": "New subject" })
    );
    assert_eq!(
        capture.decide_approval.lock().unwrap().as_ref().map(|d| d.1),
        Some(ApprovalState::Granted as i32)
    );
}

#[tokio::test]
#[serial_test::serial]
async fn edit_with_scope_thread_is_refused() {
    let _auth = AuthFixture::start().await;
    std::env::set_var("APPROVAL_EDIT_ENABLED", "true");
    let tokens = AuthFixture::user_tokens("org-1", "user-1");
    let (mock, capture) = MockOrchestration::new();
    let app = build_router(make_state(spawn_orchestration_mock(mock).await), None);

    let resp = app
        .oneshot(edited_decide_request(&tokens, "appr-42", &edit_body(EDIT_BASE, "thread")))
        .await
        .unwrap();
    std::env::remove_var("APPROVAL_EDIT_ENABLED");
    let (status, json) = json_of(resp).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "body={json}");
    assert_eq!(json["error"]["code"], "edit_requires_once");
    assert!(capture.decide_approval.lock().unwrap().is_none());
}

#[tokio::test]
#[serial_test::serial]
async fn approval_changed_is_409() {
    let _auth = AuthFixture::start().await;
    std::env::set_var("APPROVAL_EDIT_ENABLED", "true");
    let tokens = AuthFixture::user_tokens("org-1", "user-1");
    let (mock, _capture) = MockOrchestration::new();
    let app = build_router(make_state(spawn_orchestration_mock(mock).await), None);

    let stale = "f".repeat(64);
    let resp = app
        .oneshot(edited_decide_request(&tokens, "appr-42", &edit_body(&stale, "")))
        .await
        .unwrap();
    std::env::remove_var("APPROVAL_EDIT_ENABLED");
    let (status, json) = json_of(resp).await;
    assert_eq!(status, StatusCode::CONFLICT, "body={json}");
    assert_eq!(json["error"]["code"], "approval_changed");
    assert!(json["error"]["details"].is_object());
}

#[tokio::test]
#[serial_test::serial]
async fn edit_passes_step_up_like_a_grant() {
    let _auth = AuthFixture::start().await;
    std::env::set_var("APPROVAL_EDIT_ENABLED", "true");
    std::env::set_var("APPROVAL_STEP_UP_ENFORCED", "enforce");
    let tokens = AuthFixture::user_tokens("org-1", "user-1");
    let (mock, capture) = MockOrchestration::new();
    let app = build_router(make_state(spawn_orchestration_mock(mock).await), None);

    let resp = app
        .oneshot(edited_decide_request(&tokens, "appr-stepup-1", &edit_body(EDIT_BASE, "once")))
        .await
        .unwrap();
    std::env::remove_var("APPROVAL_EDIT_ENABLED");
    std::env::remove_var("APPROVAL_STEP_UP_ENFORCED");
    let (status, json) = json_of(resp).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body={json}");
    assert_eq!(
        json["error"]["code"], "step_up_required",
        "an edited grant needs the same fresh re-authentication as any grant"
    );
    assert!(capture.decide_edit.lock().unwrap().is_none());
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
    // Main's RPCs (merged 2026-11-02); this double does not serve them.
    async fn reroute_approval(
        &self,
        _: tonic::Request<mp_contracts::model_plane::v1::RerouteApprovalRequest>,
    ) -> Result<tonic::Response<mp_contracts::model_plane::v1::RerouteApprovalResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("reroute_approval is not served by this double"))
    }

    async fn claim_once_approval(
        &self,
        _: tonic::Request<mp_contracts::model_plane::v1::ClaimOnceApprovalRequest>,
    ) -> Result<tonic::Response<mp_contracts::model_plane::v1::ClaimOnceApprovalResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("claim_once_approval is not served by this double"))
    }

    async fn hold_quarantined_output(
        &self,
        _: tonic::Request<mp_contracts::model_plane::v1::HoldQuarantinedOutputRequest>,
    ) -> Result<tonic::Response<mp_contracts::model_plane::v1::HoldQuarantinedOutputResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("hold_quarantined_output is not served by this double"))
    }

    async fn claim_quarantine_release(
        &self,
        _: tonic::Request<mp_contracts::model_plane::v1::ClaimQuarantineReleaseRequest>,
    ) -> Result<tonic::Response<mp_contracts::model_plane::v1::ClaimQuarantineReleaseResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("claim_quarantine_release is not served by this double"))
    }

    async fn get_quarantine_preview(
        &self,
        _: tonic::Request<mp_contracts::model_plane::v1::GetQuarantinePreviewRequest>,
    ) -> Result<tonic::Response<mp_contracts::model_plane::v1::GetQuarantinePreviewResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("get_quarantine_preview is not served by this double"))
    }

    async fn get_approval_preview(
        &self,
        _: tonic::Request<mp_contracts::model_plane::v1::GetApprovalPreviewRequest>,
    ) -> Result<tonic::Response<mp_contracts::model_plane::v1::GetApprovalPreviewResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("get_approval_preview is not served by this double"))
    }

    async fn get_skipped_assist_authority(
        &self,
        _: tonic::Request<mp_contracts::model_plane::v1::GetSkippedAssistAuthorityRequest>,
    ) -> Result<tonic::Response<mp_contracts::model_plane::v1::GetSkippedAssistAuthorityResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("get_skipped_assist_authority is not served by this double"))
    }

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
    async fn propose_plan(
        &self,
        _: TonicRequest<mp_contracts::model_plane::v1::ProposePlanRequest>,
    ) -> Result<
        Response<mp_contracts::model_plane::v1::ProposePlanResponse>,
        Status,
    > {
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
    async fn consume_approval(
        &self,
        _: TonicRequest<ConsumeApprovalRequest>,
    ) -> Result<Response<ConsumeApprovalResponse>, Status> {
        Err(Status::unimplemented("consume_approval not needed in test"))
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
    async fn create_run_question(
        &self,
        _: TonicRequest<CreateRunQuestionRequest>,
    ) -> Result<Response<CreateRunQuestionResponse>, Status> {
        Err(Status::unimplemented("not needed in this test"))
    }
    async fn list_run_questions(
        &self,
        _: TonicRequest<ListRunQuestionsRequest>,
    ) -> Result<Response<ListRunQuestionsResponse>, Status> {
        Err(Status::unimplemented("not needed in this test"))
    }
    async fn answer_run_question(
        &self,
        _: TonicRequest<AnswerRunQuestionRequest>,
    ) -> Result<Response<AnswerRunQuestionResponse>, Status> {
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
        consumed_at: None,
        tool_key: String::new(),
        action_label: String::new(),
        continuation: None,
        continuation_mode: String::new(),
        cause: None,
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
        ..Default::default()
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
    let (state, publisher, cancel_calls) = run_owner_state(&owner.session).await;
    let app = build_router(state, None);

    for foreign in [&same_org_other_user, &other_org] {
        let cancel = Request::builder()
            .method("POST")
            .uri("/v1/orchestration/runs/run-owned/cancel")
            .header(AUTHORIZATION, format!("Bearer {}", foreign.model))
            .header(
                "x-session-authorization",
                format!("Bearer {}", foreign.session),
            )
            .body(Body::empty())
            .unwrap();
        let cancel = app.clone().oneshot(cancel).await.unwrap();
        assert_eq!(cancel.status(), StatusCode::FORBIDDEN);
    }
    assert_eq!(
        cancel_calls.load(Ordering::SeqCst),
        0,
        "a foreign cancel must be refused before it reaches Session Core's CancelRun"
    );

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
    let owner_cancel = to_bytes(owner_cancel.into_body(), usize::MAX)
        .await
        .unwrap();
    let owner_cancel: serde_json::Value = serde_json::from_slice(&owner_cancel).unwrap();
    assert_eq!(owner_cancel["run_id"], "run-owned");
    assert_eq!(owner_cancel["status"], "cancelled");
    assert_eq!(owner_cancel["cancelled"], true);
    assert_eq!(
        owner_cancel["receipt_id"], "receipt-cancel-1",
        "the durable RUN_CANCELLED receipt must reach the caller"
    );
    assert_eq!(
        cancel_calls.load(Ordering::SeqCst),
        1,
        "the owner's cancel reaches CancelRun exactly once"
    );

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
    assert_eq!(events[0].1.causation_id, "receipt-cancel-1");
    assert_eq!(events[1].1.event_type, "FEEDBACK_RATED");
}

// ---------------------------------------------------------------------------
// plan-gate §2.3: `POST /v1/runs/{run_id}/plan-approval`
//
// The route is the confirm gate itself: it decides a PLAN and, only if that
// worked, grants the run authority. What matters is the ORDER (check the plan,
// decide it, then grant) and that a refusal at any step leaves no grant behind.
// ---------------------------------------------------------------------------

struct PlanGateHarness {
    app: axum::Router,
    capture: CaptureState,
    plan_mode: model_gateway::coordinator::PlanModeStore,
    owner: UserTokens,
    _auth: AuthFixture,
}

async fn plan_gate_harness() -> PlanGateHarness {
    let auth = AuthFixture::start().await;
    let owner = AuthFixture::user_tokens("org-owner", "owner-user");
    let (mock, capture) = MockOrchestration::new();
    let (mut state, _publisher, _cancel_calls) = run_owner_state(&owner.session).await;
    state.orchestration_client = spawn_orchestration_mock(mock).await;
    // The grant's durable half (`set_run_mode`) goes to session-core, which this
    // harness does not serve: `offline_state` leaves it an offline double, so the
    // test never reaches a real service that happens to be running on the default
    // local port; the route reports `persisted: false` and still answers.
    let plan_mode = state.plan_mode.clone();
    PlanGateHarness {
        app: build_router(state, None),
        capture,
        plan_mode,
        owner,
        _auth: auth,
    }
}

async fn post_plan_approval(
    harness: &PlanGateHarness,
    run_id: &str,
    tokens: &UserTokens,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let request = Request::builder()
        .method("POST")
        .uri(format!("/v1/runs/{run_id}/plan-approval"))
        .header(AUTHORIZATION, format!("Bearer {}", tokens.model))
        .header(
            "x-session-authorization",
            format!("Bearer {}", tokens.session),
        )
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = harness.app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

#[tokio::test]
#[serial_test::serial]
async fn an_approval_answers_next_message_while_the_execute_switch_is_off() {
    // WS14 S2: with the approve-then-execute switch off (the default) the answer is the one it
    // always was: the grant waits for the person's own next message, and nothing is sent by itself.
    let harness = plan_gate_harness().await;
    let (status, body) = post_plan_approval(
        &harness,
        "run-owned",
        &harness.owner,
        serde_json::json!({
            "decision": "approve",
            "granted_rung": "workspace_write",
            "justification": "writes the report into the workspace",
            "plan_id": "plan-7",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["continuation"], "next_message", "{body}");
}

#[tokio::test]
#[serial_test::serial]
async fn approving_a_plan_checks_it_decides_it_with_the_edits_and_only_then_grants() {
    let harness = plan_gate_harness().await;
    let (status, body) = post_plan_approval(
        &harness,
        "run-owned",
        &harness.owner,
        serde_json::json!({
            "decision": "approve",
            "granted_rung": "workspace_write",
            "justification": "writes the report into the workspace",
            "plan_id": "plan-7",
            "edited_steps": [
                {"step_id": "step-b", "title": "Send it", "removed": false},
                {"step_id": "step-a", "title": "Draft the email", "removed": true},
            ],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["granted_rung"], "workspace_write");
    assert_eq!(body["plan"]["id"], "plan-7");
    // The grant states when it lapses (WS14 S2a), so the client knows how long the rung it
    // just granted lasts: exactly what the store holds for this org and thread, within the
    // grant's 24 hour ceiling and not in the past.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after the epoch")
        .as_secs() as i64;
    let expires = body["grant_expires_at_unix"]
        .as_i64()
        .expect("grant_expires_at_unix is an integer");
    assert!(
        expires > now && expires <= now + 86_400,
        "the expiry is within the grant's ceiling: {expires} vs now {now}"
    );
    assert_eq!(
        Some(expires),
        harness.plan_mode.grant_expires_at("org-owner", "thread-1"),
        "it is exactly what the store holds"
    );

    // The plan was looked at BEFORE it was decided.
    assert_eq!(
        *harness.capture.plan_calls.lock().unwrap(),
        vec!["get_plan".to_owned(), "transition_plan".to_owned()]
    );
    // Decided as APPROVED, by the caller, for the stated reason, with the
    // person's edits in the person's order.
    let (plan_id, target, actor, reason) = harness
        .capture
        .transition_plan
        .lock()
        .unwrap()
        .clone()
        .expect("the plan must be transitioned");
    assert_eq!(plan_id, "plan-7");
    assert_eq!(target, PlanState::Approved as i32);
    assert_eq!(actor, "owner-user");
    assert_eq!(reason, "writes the report into the workspace");
    assert_eq!(
        *harness.capture.transition_edits.lock().unwrap(),
        vec![
            ("step-b".to_owned(), "Send it".to_owned(), false),
            ("step-a".to_owned(), "Draft the email".to_owned(), true),
        ]
    );
    // And the grant landed on the RUN's conversation, carrying the plan it
    // was approved with for the next run to follow.
    assert_eq!(
        harness.plan_mode.granted_rung("org-owner", "thread-1"),
        Some(mp_contracts::model_plane::v1::AutonomyRung::WorkspaceWrite)
    );
    assert_eq!(
        harness.plan_mode.take_approved_plan("org-owner", "thread-1"),
        Some("plan-7".to_owned())
    );
}

#[tokio::test]
#[serial_test::serial]
async fn a_plan_that_is_not_this_runs_conversation_is_refused_before_anything_is_decided() {
    let harness = plan_gate_harness().await;
    *harness.capture.plan_thread_id.lock().unwrap() = Some("thread-of-another-conversation".into());

    let (status, body) = post_plan_approval(
        &harness,
        "run-owned",
        &harness.owner,
        serde_json::json!({
            "decision": "approve",
            "granted_rung": "danger_full_access",
            "justification": "the plan sends the invoice",
            "plan_id": "plan-elsewhere",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        harness.capture.transition_plan.lock().unwrap().is_none(),
        "a plan of another conversation must not be approved through this run"
    );
    assert_eq!(
        harness.plan_mode.granted_rung("org-owner", "thread-1"),
        None,
        "and no authority may be granted on the strength of it"
    );
    assert_eq!(
        harness.plan_mode.granted_rung("org-owner", "thread-of-another-conversation"),
        None
    );

    // The same guard covers a rejection.
    let (status, _) = post_plan_approval(
        &harness,
        "run-owned",
        &harness.owner,
        serde_json::json!({
            "decision": "reject",
            "justification": "no",
            "plan_id": "plan-elsewhere",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(harness.capture.transition_plan.lock().unwrap().is_none());
}

/// The three bearers of a user, minted the way `AuthFixture::user_tokens` mints them, with the given
/// retention claims. The middleware verifies them like any other token.
fn user_tokens_with_retention(
    org_id: &str,
    user_id: &str,
    zdr: bool,
    org_zdr: bool,
    waiver: Option<serde_json::Value>,
) -> UserTokens {
    let now = token_now();
    let claims = |audience: &str| {
        let mut payload = serde_json::json!({
            "sub": user_id,
            "iss": TEST_AUTH_ISSUER,
            "aud": audience,
            "exp": now + 300,
            "nbf": now - 5,
            "org_id": org_id,
            "user_id": user_id,
            "principal_type": "user",
            "zdr": zdr,
            "org_zdr": org_zdr,
        });
        if let Some(waiver) = &waiver {
            payload["zdr_waiver"] = waiver.clone();
        }
        payload
    };
    UserTokens {
        model: encode_test_token(&claims("model-gateway")),
        session: encode_test_token(&claims("session-core")),
        execution: encode_test_token(&claims("execution-core")),
    }
}

/// Owner decision 17: a ZDR credential is refused at the gateway before any plan is read or decided,
/// and the refusal is typed. The counting double sees no call.
#[tokio::test]
#[serial_test::serial]
async fn a_zdr_credential_is_refused_before_any_plan_is_read_or_decided() {
    let harness = plan_gate_harness().await;
    let tokens = user_tokens_with_retention("org-owner", "owner-user", true, true, None);
    let (status, body) = post_plan_approval(
        &harness,
        "run-owned",
        &tokens,
        serde_json::json!({
            "decision": "approve",
            "granted_rung": "workspace_write",
            "justification": "writes the report into the workspace",
            "plan_id": "plan-7",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::PRECONDITION_FAILED, "{body}");
    assert_eq!(
        body["error"]["code"], "zdr_durable_mutation_forbidden",
        "{body}"
    );
    assert!(
        harness.capture.plan_calls.lock().unwrap().is_empty(),
        "no plan was read or decided for a ZDR credential"
    );
    assert!(harness.capture.transition_plan.lock().unwrap().is_none());
    assert_eq!(
        harness.plan_mode.granted_rung("org-owner", "thread-1"),
        None
    );
}

/// Owner decision 17: a valid waiver is not refused at the gateway. Session Core checks it against the
/// receipt of the run it would advance, so the decision reaches the owner check and the plan is decided
/// here only because the stub accepts it. The receipt check itself is pinned in Session Core's tests.
#[tokio::test]
#[serial_test::serial]
async fn a_waiver_is_not_refused_at_the_gateway_so_session_core_checks_its_receipt() {
    let harness = plan_gate_harness().await;
    let waiver = serde_json::json!({
        "version": 1,
        "provider": mp_contracts::agentic_subscription::PROVIDER_ID,
        "connection": "conn-1",
    });
    let tokens = user_tokens_with_retention("org-owner", "owner-user", false, true, Some(waiver));
    let (status, body) = post_plan_approval(
        &harness,
        "run-owned",
        &tokens,
        serde_json::json!({
            "decision": "approve",
            "granted_rung": "workspace_write",
            "justification": "writes the report into the workspace",
            "plan_id": "plan-7",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        *harness.capture.plan_calls.lock().unwrap(),
        vec!["get_plan".to_owned(), "transition_plan".to_owned()]
    );
}

#[tokio::test]
#[serial_test::serial]
async fn a_plan_the_server_refuses_to_approve_grants_no_authority() {
    // session-core refuses an approval of a plan that is no longer PROPOSED
    // (already decided, or beaten by a concurrent request). An autonomy grant
    // with no approved plan behind it is exactly the rubber stamp this gate
    // exists to prevent.
    let harness = plan_gate_harness().await;
    harness.capture.fail_transition.store(true, Ordering::SeqCst);

    let (status, body) = post_plan_approval(
        &harness,
        "run-owned",
        &harness.owner,
        serde_json::json!({
            "decision": "approve",
            "granted_rung": "workspace_write",
            "justification": "writes the report into the workspace",
            "plan_id": "plan-7",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::PRECONDITION_FAILED, "{body}");
    assert_eq!(
        harness.plan_mode.granted_rung("org-owner", "thread-1"),
        None,
        "a refused plan transition must leave no grant behind"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn rejecting_a_plan_decides_it_and_grants_nothing() {
    let harness = plan_gate_harness().await;
    let (status, body) = post_plan_approval(
        &harness,
        "run-owned",
        &harness.owner,
        // No rung and a one-word reason: an approve would be refused for both,
        // a reject needs neither because it hands nothing back.
        serde_json::json!({
            "decision": "reject",
            "justification": "nei",
            "plan_id": "plan-7",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["decision"], "reject");
    let (plan_id, target, _actor, reason) = harness
        .capture
        .transition_plan
        .lock()
        .unwrap()
        .clone()
        .expect("the plan must be transitioned");
    assert_eq!(plan_id, "plan-7");
    assert_eq!(target, PlanState::Rejected as i32);
    assert_eq!(reason, "nei");
    assert_eq!(harness.plan_mode.granted_rung("org-owner", "thread-1"), None);
}

#[tokio::test]
#[serial_test::serial]
async fn rejecting_without_a_plan_is_refused_and_an_unknown_decision_is_too() {
    let harness = plan_gate_harness().await;
    let (status, _) = post_plan_approval(
        &harness,
        "run-owned",
        &harness.owner,
        serde_json::json!({"decision": "reject", "justification": "nei"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "there is no plan to reject");

    let (status, _) = post_plan_approval(
        &harness,
        "run-owned",
        &harness.owner,
        serde_json::json!({
            "decision": "maybe",
            "granted_rung": "workspace_write",
            "justification": "writes the report into the workspace",
            "plan_id": "plan-7",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(harness.capture.plan_calls.lock().unwrap().is_empty());
    assert_eq!(harness.plan_mode.granted_rung("org-owner", "thread-1"), None);
}

#[tokio::test]
#[serial_test::serial]
async fn only_the_runs_owner_can_decide_its_plan() {
    let harness = plan_gate_harness().await;
    let stranger = AuthFixture::user_tokens("org-owner", "someone-else");
    let (status, _) = post_plan_approval(
        &harness,
        "run-owned",
        &stranger,
        serde_json::json!({
            "decision": "approve",
            "granted_rung": "workspace_write",
            "justification": "writes the report into the workspace",
            "plan_id": "plan-7",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        harness.capture.plan_calls.lock().unwrap().is_empty(),
        "a stranger must be refused before the plan is even read"
    );
    assert_eq!(harness.plan_mode.granted_rung("org-owner", "thread-1"), None);
}

#[tokio::test]
#[serial_test::serial]
async fn edits_without_a_plan_are_refused_rather_than_silently_dropped() {
    // The person rewrote steps; a body that does not say WHICH plan they belong
    // to cannot apply them, and approving anyway would grant authority against
    // the plan as the model wrote it while telling them nothing.
    let harness = plan_gate_harness().await;
    for decision in ["approve", "reject"] {
        let (status, body) = post_plan_approval(
            &harness,
            "run-owned",
            &harness.owner,
            serde_json::json!({
                "decision": decision,
                "granted_rung": "workspace_write",
                "justification": "writes the report into the workspace",
                "edited_steps": [{"step_id": "step-a", "title": "Send it", "removed": false}],
            }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{decision}: {body}");
    }
    assert!(harness.capture.plan_calls.lock().unwrap().is_empty());
    assert_eq!(harness.plan_mode.granted_rung("org-owner", "thread-1"), None);
}

#[tokio::test]
#[serial_test::serial]
async fn approving_without_a_structured_plan_is_still_the_plain_rung_grant() {
    // A turn whose model never called plan.propose (or a browser that reloaded
    // and lost the plan): no plan_id, no plan RPCs, the rung grant as before.
    let harness = plan_gate_harness().await;
    let (status, body) = post_plan_approval(
        &harness,
        "run-owned",
        &harness.owner,
        serde_json::json!({
            "granted_rung": "workspace_write",
            "justification": "writes the report into the workspace",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["plan"].is_null());
    assert!(harness.capture.plan_calls.lock().unwrap().is_empty());
    assert_eq!(
        harness.plan_mode.granted_rung("org-owner", "thread-1"),
        Some(mp_contracts::model_plane::v1::AutonomyRung::WorkspaceWrite)
    );
}

// ---------------------------------------------------------------------------
// WS14 S3: run questions. Dark: no producer exists yet, so these routes read and
// answer questions that a later slice (or a direct RPC) creates. A question is its
// own store, never an approval: an answer cannot grant authority.
// ---------------------------------------------------------------------------

struct QuestionHarness {
    app: axum::Router,
    capture: CaptureState,
}

async fn question_harness() -> QuestionHarness {
    let (mock, capture) = MockOrchestration::new();
    let app = build_router(make_state(spawn_orchestration_mock(mock).await), None);
    QuestionHarness { app, capture }
}

async fn question_call(
    harness: &QuestionHarness,
    method: &str,
    uri: &str,
    tokens: &UserTokens,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(AUTHORIZATION, format!("Bearer {}", tokens.model))
        .header(
            "x-session-authorization",
            format!("Bearer {}", tokens.session),
        );
    let body = match body {
        Some(body) => {
            builder = builder.header("content-type", "application/json");
            Body::from(body.to_string())
        }
        None => Body::empty(),
    };
    let response = harness
        .app
        .clone()
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| serde_json::Value::String(String::from_utf8_lossy(&bytes).into_owned()));
    (status, json)
}

#[tokio::test]
#[serial_test::serial]
async fn run_question_the_list_route_scopes_the_org_to_the_callers_credential() {
    let _auth = AuthFixture::start().await;
    let harness = question_harness().await;
    let tokens = AuthFixture::user_tokens("org-1", "user-1");
    let (status, body) = question_call(
        &harness,
        "GET",
        "/v1/orchestration/runs/run-9/questions?org_id=org-forged",
        &tokens,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        *harness.capture.list_questions.lock().unwrap(),
        Some(("run-9".into(), "org-1".into())),
        "the org is the verified claim's, never a query value"
    );
    let question = &body["questions"][0];
    assert_eq!(question["id"], "q-1");
    assert_eq!(question["state"], "requested");
    assert_eq!(question["delivery"], "next_turn");
    assert_eq!(question["items"][0]["kind"], "single");
    assert_eq!(question["items"][0]["options"][1]["description"], "New work");
    assert_eq!(question["items"][0]["options"][0]["description"], serde_json::Value::Null);
    assert_eq!(question["answered_at"], serde_json::Value::Null);
    assert!(
        question.get("org_id").is_none() && question.get("user_id").is_none(),
        "the owner's own ids stay off the wire"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn run_question_an_answer_is_forwarded_with_the_callers_org_and_returns_the_question() {
    let _auth = AuthFixture::start().await;
    let harness = question_harness().await;
    let tokens = AuthFixture::user_tokens("org-1", "user-1");
    let (status, body) = question_call(
        &harness,
        "POST",
        "/v1/orchestration/questions/q-7/answer",
        &tokens,
        Some(serde_json::json!({
            "answers": [{"item_id": "scope", "option_ids": ["main"]}],
            "org_id": "org-forged",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["question"]["id"], "q-7");
    assert_eq!(body["question"]["state"], "answered");
    let sent = harness
        .capture
        .answer_question
        .lock()
        .unwrap()
        .clone()
        .expect("the answer reached Session Core");
    assert_eq!(sent.question_id, "q-7");
    assert_eq!(sent.org_id, "org-1", "the org is the verified claim's");
    assert!(!sent.skip);
    assert_eq!(sent.answers.len(), 1);
    assert_eq!(sent.answers[0].item_id, "scope");
    assert_eq!(sent.answers[0].option_ids, vec!["main".to_owned()]);
}

#[tokio::test]
#[serial_test::serial]
async fn run_question_a_skip_is_forwarded_as_a_skip() {
    let _auth = AuthFixture::start().await;
    let harness = question_harness().await;
    let tokens = AuthFixture::user_tokens("org-1", "user-1");
    let (status, body) = question_call(
        &harness,
        "POST",
        "/v1/orchestration/questions/q-7/answer",
        &tokens,
        Some(serde_json::json!({"skip": true, "answered_message_id": "msg-forged"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["question"]["state"], "skipped");
    let sent = harness.capture.answer_question.lock().unwrap().clone().unwrap();
    assert!(sent.skip && sent.answers.is_empty());
    assert_eq!(
        sent.answered_message_id, "",
        "the message that carries an answer is never taken from the caller"
    );
}

/// Each refusal Session Core can give has its own status and a typed body.
#[tokio::test]
#[serial_test::serial]
async fn run_question_each_refusal_maps_to_its_typed_error() {
    let _auth = AuthFixture::start().await;
    let harness = question_harness().await;
    let tokens = AuthFixture::user_tokens("org-1", "user-1");
    for (question, status, code) in [
        ("q-missing", StatusCode::NOT_FOUND, "question_not_found"),
        ("q-expired", StatusCode::GONE, "question_expired"),
        ("q-done", StatusCode::CONFLICT, "question_already_answered"),
        ("q-bad", StatusCode::UNPROCESSABLE_ENTITY, "invalid_answer"),
    ] {
        let (got, body) = question_call(
            &harness,
            "POST",
            &format!("/v1/orchestration/questions/{question}/answer"),
            &tokens,
            Some(serde_json::json!({"skip": true})),
        )
        .await;
        assert_eq!(got, status, "{question}: {body}");
        assert_eq!(body["error"]["code"], code, "{question}: {body}");
        assert!(
            body["error"]["message"].as_str().is_some_and(|m| !m.is_empty()),
            "{question}: a message rides every typed error"
        );
        assert!(body["error"]["details"].is_object(), "{question}: {body}");
    }
    let (_, body) = question_call(
        &harness,
        "POST",
        "/v1/orchestration/questions/q-bad/answer",
        &tokens,
        Some(serde_json::json!({"skip": true})),
    )
    .await;
    assert_eq!(
        body["error"]["details"],
        serde_json::json!({"field": "answers[0].option_ids[0]", "reason": "unknown_option"}),
        "an invalid answer names the field and the reason"
    );
}

/// A ZDR credential is refused with the typed durable-mutation refusal before Session Core is
/// asked anything, on both routes.
#[tokio::test]
#[serial_test::serial]
async fn run_question_a_zdr_credential_is_refused_before_anything_is_asked() {
    let _auth = AuthFixture::start().await;
    let harness = question_harness().await;
    let tokens = user_tokens_with_retention("org-1", "user-1", true, true, None);
    let (status, body) = question_call(
        &harness,
        "GET",
        "/v1/orchestration/runs/run-1/questions",
        &tokens,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::PRECONDITION_FAILED, "{body}");
    assert_eq!(body["error"]["code"], "zdr_durable_mutation_forbidden", "{body}");
    let (status, body) = question_call(
        &harness,
        "POST",
        "/v1/orchestration/questions/q-1/answer",
        &tokens,
        Some(serde_json::json!({"skip": true})),
    )
    .await;
    assert_eq!(status, StatusCode::PRECONDITION_FAILED, "{body}");
    assert_eq!(body["error"]["code"], "zdr_durable_mutation_forbidden", "{body}");
    assert_eq!(
        harness.capture.question_calls.load(Ordering::SeqCst),
        0,
        "no question RPC was made for a ZDR credential"
    );
}

/// A waived run counts as ZDR for every new durable store (DEC-32), so a waived credential is refused
/// too, as a route mismatch, and Session Core is never asked.
#[tokio::test]
#[serial_test::serial]
async fn run_question_a_waived_credential_is_refused_before_anything_is_asked() {
    let _auth = AuthFixture::start().await;
    let harness = question_harness().await;
    let waiver = serde_json::json!({
        "version": 1,
        "provider": mp_contracts::agentic_subscription::PROVIDER_ID,
        "connection": "conn-1",
    });
    let tokens = user_tokens_with_retention("org-1", "user-1", false, true, Some(waiver));
    let (status, body) = question_call(
        &harness,
        "GET",
        "/v1/orchestration/runs/run-1/questions",
        &tokens,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"]["code"], "zdr_waiver_route_mismatch", "{body}");
    let (status, body) = question_call(
        &harness,
        "POST",
        "/v1/orchestration/questions/q-1/answer",
        &tokens,
        Some(serde_json::json!({"skip": true})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(
        harness.capture.question_calls.load(Ordering::SeqCst),
        0,
        "no question RPC was made for a waived credential"
    );
}

/// An answer is information, never a grant: the answer route shares no path with an approval, and
/// answering a question decides nothing.
#[tokio::test]
#[serial_test::serial]
async fn run_question_an_answer_never_touches_an_approval() {
    let _auth = AuthFixture::start().await;
    let harness = question_harness().await;
    let tokens = AuthFixture::user_tokens("org-1", "user-1");
    let (status, _) = question_call(
        &harness,
        "POST",
        "/v1/orchestration/questions/q-7/answer",
        &tokens,
        Some(serde_json::json!({"skip": true})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        harness.capture.decide_approval.lock().unwrap().is_none(),
        "answering a question decided no approval"
    );
}

// ---------------------------------------------------------------------------
// WS04 S4: the durable plan grant. Session Core is the double below: it records
// what `SetRunMode` writes and answers `ThreadAutonomy.GetThreadGrant` from it,
// as the two RPCs of the real service share one table. Two gateway replicas are
// two `AppState`s over the one double.
// ---------------------------------------------------------------------------

use mp_contracts::model_plane::v1::{
    thread_autonomy_client::ThreadAutonomyClient,
    thread_autonomy_server::{ThreadAutonomy, ThreadAutonomyServer},
    BindGrantToRunRequest, BindGrantToRunResponse, GetThreadGrantRequest, GetThreadGrantResponse,
    SetRunModeRequest, SetRunModeResponse,
};

#[derive(Clone, Debug)]
struct RecordedGrant {
    org_id: String,
    thread_id: String,
    rung: i32,
    plan_id: String,
    expires_at: i64,
}

/// How the double's `GetThreadGrant` misbehaves, for the tests of a failed read.
#[derive(Clone, Copy, Debug)]
enum GrantReadFailure {
    /// Answers this status code (a transient `Unavailable`, `Unimplemented` from an older
    /// Session Core, a refusal).
    Status(tonic::Code),
    /// Never answers: the gateway's own 5 s bound is what ends the read.
    Hang,
}

/// How many `SetRunMode` calls still fail. `usize::MAX` fails every one.
const FAIL_EVERY_SET_RUN_MODE: usize = usize::MAX;

#[derive(Clone, Default)]
struct SessionCoreDouble {
    grant: Arc<Mutex<Option<RecordedGrant>>>,
    /// Only the calls that succeeded.
    set_run_mode_calls: Arc<Mutex<Vec<SetRunModeRequest>>>,
    /// Every call, failed or not.
    set_run_mode_attempts: Arc<AtomicUsize>,
    /// How many of the next `SetRunMode` calls fail with `set_run_mode_failure_code`.
    set_run_mode_failures: Arc<AtomicUsize>,
    set_run_mode_failure_code: Arc<Mutex<Option<tonic::Code>>>,
    get_grant_failure: Arc<Mutex<Option<GrantReadFailure>>>,
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after the epoch")
        .as_secs() as i64
}

impl SessionCoreDouble {
    fn set_run_mode(
        &self,
        request: TonicRequest<SetRunModeRequest>,
    ) -> Result<Response<SetRunModeResponse>, Status> {
        if request.metadata().get("authorization").is_none() {
            return Err(Status::unauthenticated("verified session credential required"));
        }
        self.set_run_mode_attempts.fetch_add(1, Ordering::SeqCst);
        let failures = self.set_run_mode_failures.load(Ordering::SeqCst);
        if failures > 0 {
            if failures != FAIL_EVERY_SET_RUN_MODE {
                self.set_run_mode_failures.fetch_sub(1, Ordering::SeqCst);
            }
            let code = self
                .set_run_mode_failure_code
                .lock()
                .unwrap()
                .unwrap_or(tonic::Code::Unavailable);
            return Err(Status::new(code, "session-core is restarting"));
        }
        let request = request.into_inner();
        self.set_run_mode_calls.lock().unwrap().push(request.clone());
        if request.granted_rung != 0 {
            *self.grant.lock().unwrap() = Some(RecordedGrant {
                org_id: request.org_id.clone(),
                thread_id: "thread-1".to_owned(),
                rung: request.granted_rung,
                plan_id: request.approved_plan_id.clone(),
                expires_at: unix_now() + 86_400,
            });
        }
        Ok(Response::new(SetRunModeResponse {
            run_id: request.run_id,
            mode: request.mode,
            granted_rung: request.granted_rung,
        }))
    }
}

#[tonic::async_trait]
impl ThreadAutonomy for SessionCoreDouble {
    async fn get_thread_grant(
        &self,
        request: TonicRequest<GetThreadGrantRequest>,
    ) -> Result<Response<GetThreadGrantResponse>, Status> {
        if request.metadata().get("authorization").is_none() {
            return Err(Status::unauthenticated("verified session credential required"));
        }
        let failure = *self.get_grant_failure.lock().unwrap();
        match failure {
            Some(GrantReadFailure::Status(code)) => {
                return Err(Status::new(code, "the grant read failed"));
            }
            Some(GrantReadFailure::Hang) => std::future::pending::<()>().await,
            None => {}
        }
        let request = request.into_inner();
        let answer = match self.grant.lock().unwrap().clone() {
            Some(grant)
                if grant.org_id == request.org_id
                    && grant.thread_id == request.thread_id
                    && grant.expires_at > unix_now() =>
            {
                GetThreadGrantResponse {
                    granted_rung: grant.rung,
                    justification: String::new(),
                    approved_plan_id: grant.plan_id,
                    expires_at: Some(prost_types::Timestamp {
                        seconds: grant.expires_at,
                        nanos: 0,
                    }),
                }
            }
            _ => GetThreadGrantResponse::default(),
        };
        Ok(Response::new(answer))
    }

    /// WS14 S2: binds only the grant the thread holds for exactly the plan asked for, as the
    /// real bind does; anything else is the refusal the gateway treats as no bound grant.
    async fn bind_grant_to_run(
        &self,
        request: TonicRequest<BindGrantToRunRequest>,
    ) -> Result<Response<BindGrantToRunResponse>, Status> {
        if request.metadata().get("authorization").is_none() {
            return Err(Status::unauthenticated("verified session credential required"));
        }
        let request = request.into_inner();
        match self.grant.lock().unwrap().clone() {
            Some(grant)
                if grant.org_id == request.org_id
                    && grant.thread_id == request.thread_id
                    && grant.plan_id == request.approved_plan_id
                    && grant.expires_at > unix_now() =>
            {
                Ok(Response::new(BindGrantToRunResponse {
                    granted_rung: grant.rung,
                    justification: String::new(),
                    approved_plan_id: grant.plan_id,
                }))
            }
            _ => Err(Status::failed_precondition("plan_grant_unavailable")),
        }
    }
}

/// Only `SessionCore.SetRunMode` of the real service's surface; every other method answers
/// UNIMPLEMENTED. A hand-built tonic service rather than the 32-method trait, so the double
/// is the two RPCs this slice reads and writes and nothing else.
impl tonic::server::NamedService for SessionCoreDouble {
    const NAME: &'static str = "model_plane.v1.SessionCore";
}

impl<B> tonic::codegen::Service<tonic::codegen::http::Request<B>> for SessionCoreDouble
where
    B: tonic::codegen::Body + Send + 'static,
    B::Error: Into<tonic::codegen::StdError> + Send + 'static,
{
    type Response = tonic::codegen::http::Response<tonic::body::BoxBody>;
    type Error = std::convert::Infallible;
    type Future = tonic::codegen::BoxFuture<Self::Response, Self::Error>;

    fn poll_ready(
        &mut self,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: tonic::codegen::http::Request<B>) -> Self::Future {
        match req.uri().path() {
            "/model_plane.v1.SessionCore/SetRunMode" => {
                struct SetRunModeSvc(SessionCoreDouble);
                impl tonic::server::UnaryService<SetRunModeRequest> for SetRunModeSvc {
                    type Response = SetRunModeResponse;
                    type Future = tonic::codegen::BoxFuture<Response<Self::Response>, Status>;
                    fn call(&mut self, request: TonicRequest<SetRunModeRequest>) -> Self::Future {
                        let double = self.0.clone();
                        Box::pin(async move { double.set_run_mode(request) })
                    }
                }
                let double = self.clone();
                Box::pin(async move {
                    let mut grpc =
                        tonic::server::Grpc::new(tonic::codec::ProstCodec::default());
                    Ok(grpc.unary(SetRunModeSvc(double), req).await)
                })
            }
            _ => Box::pin(async move {
                Ok(tonic::codegen::http::Response::builder()
                    .status(200)
                    .header("grpc-status", "12")
                    .header("content-type", "application/grpc")
                    .body(tonic::codegen::empty_body())
                    .expect("an UNIMPLEMENTED response"))
            }),
        }
    }
}

async fn spawn_session_core_double(
    double: &SessionCoreDouble,
) -> (
    mp_contracts::model_plane::v1::session_core_client::SessionCoreClient<tonic::transport::Channel>,
    ThreadAutonomyClient<tonic::transport::Channel>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind session-core double");
    let addr = listener.local_addr().expect("session-core double addr");
    let served = double.clone();
    tokio::spawn(async move {
        Server::builder()
            .add_service(served.clone())
            .add_service(ThreadAutonomyServer::new(served))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .ok();
    });
    let channel = Endpoint::from_shared(format!("http://{addr}"))
        .expect("session-core double endpoint")
        .connect()
        .await
        .expect("connect session-core double");
    (
        mp_contracts::model_plane::v1::session_core_client::SessionCoreClient::new(channel.clone()),
        ThreadAutonomyClient::new(channel),
    )
}

struct DurableGrantHarness {
    /// Replica A, the one the approval is posted to.
    app: axum::Router,
    publisher: Arc<DynPublisher>,
    capture: CaptureState,
    plan_mode_a: model_gateway::coordinator::PlanModeStore,
    /// Replica B: another gateway process over the same Session Core. It saw no approval.
    replica_b: AppState,
    double: SessionCoreDouble,
    owner: UserTokens,
    _auth: AuthFixture,
}

async fn durable_grant_harness(durable: bool) -> DurableGrantHarness {
    durable_grant_harness_with(durable, false).await
}

/// [`durable_grant_harness`] with the approve-then-execute switches of the plan store stated: the
/// execute switch and the approval-resume switch, which an `execute_now` answer needs (WS14 S2).
/// With `execute` false the store is the one `durable_grant_harness` has always built.
async fn durable_grant_harness_with(durable: bool, execute: bool) -> DurableGrantHarness {
    let auth = AuthFixture::start().await;
    let owner = AuthFixture::user_tokens("org-owner", "owner-user");
    let double = SessionCoreDouble::default();
    let (session_client, grant_client) = spawn_session_core_double(&double).await;
    let configured = || {
        let store = model_gateway::coordinator::PlanModeStore::default()
            .with_execute_enabled(execute)
            .with_approval_resume_enabled(execute);
        if durable {
            store
                .with_durable_grant_enabled(true)
                .with_durable_grant_client(grant_client.clone())
        } else {
            store
        }
    };

    let (mock, capture) = MockOrchestration::new();
    let (mut state_a, publisher, _cancel_calls) = run_owner_state(&owner.session).await;
    state_a.orchestration_client = spawn_orchestration_mock(mock).await;
    state_a.session_client = session_client.clone();
    state_a.plan_mode = configured();
    let plan_mode_a = state_a.plan_mode.clone();

    let mut replica_b = AppState::new();
    replica_b.session_client = session_client;
    replica_b.plan_mode = configured();

    DurableGrantHarness {
        app: build_router(state_a, None),
        publisher,
        capture,
        plan_mode_a,
        replica_b,
        double,
        owner,
        _auth: auth,
    }
}

async fn post_durable_approval(
    harness: &DurableGrantHarness,
    run_id: &str,
    tokens: &UserTokens,
) -> (StatusCode, serde_json::Value) {
    let request = Request::builder()
        .method("POST")
        .uri(format!("/v1/runs/{run_id}/plan-approval"))
        .header(AUTHORIZATION, format!("Bearer {}", tokens.model))
        .header("x-session-authorization", format!("Bearer {}", tokens.session))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "decision": "approve",
                "granted_rung": "workspace_write",
                "justification": "writes the report into the workspace",
                "plan_id": "plan-7",
            })
            .to_string(),
        ))
        .unwrap();
    let response = harness.app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null))
}

/// WS14 S2, approve then execute: a plan approval answers `execute_now` only when the approve
/// switch is on, the grant is recorded durably for the plan, and an execute turn is accepted here
/// (the execute switch and the approval-resume switch). Turning the execute switch off answers
/// `next_message`, as it did before the switch existed.
#[tokio::test]
#[serial_test::serial]
async fn an_approval_answers_execute_now_only_when_every_switch_and_the_durable_record_hold() {
    std::env::set_var("MODEL_GATEWAY_PLAN_EXECUTE_ON_APPROVE", "true");
    let on = durable_grant_harness_with(true, true).await;
    let (status, body) = post_durable_approval(&on, "run-owned", &on.owner).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["persisted"], true, "{body}");
    assert_eq!(body["continuation"], "execute_now", "{body}");

    let execute_off = durable_grant_harness_with(true, false).await;
    let (status, body) = post_durable_approval(&execute_off, "run-owned", &execute_off.owner).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["continuation"], "next_message", "{body}");
    std::env::remove_var("MODEL_GATEWAY_PLAN_EXECUTE_ON_APPROVE");
}

/// The slice's point: approve on replica A, and a run started on replica B reads the rung and
/// the approved plan from Session Core. Neither replica's memory holds the grant, the lifecycle
/// event follows the run's durable mode, and the expiry the client is told is the stored one.
#[tokio::test]
#[serial_test::serial]
async fn a_plan_approved_on_one_replica_is_read_by_a_run_started_on_another() {
    let harness = durable_grant_harness(true).await;
    let (status, body) = post_durable_approval(&harness, "run-owned", &harness.owner).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["persisted"], true, "{body}");
    assert_eq!(body["was_in_plan_mode"], true, "the run's durable mode was plan: {body}");

    // The approval named its plan to Session Core, in the same call that granted the rung.
    let calls = harness.double.set_run_mode_calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].approved_plan_id, "plan-7");
    assert_eq!(calls[0].mode, "execute");

    // The expiry is the stored one, read back after the write.
    let expires = body["grant_expires_at_unix"]
        .as_i64()
        .expect("a durable grant states when it lapses");
    let now = unix_now();
    assert!(expires > now && expires <= now + 86_401, "{expires} vs {now}");

    // Replica A kept no copy: the memory half is unused with the switch on.
    assert_eq!(harness.plan_mode_a.granted_rung("org-owner", "thread-1"), None);
    assert_eq!(harness.plan_mode_a.take_approved_plan("org-owner", "thread-1"), None);

    // Replica B, which never saw the approval, reads the rung and the plan from Session Core.
    assert_eq!(harness.replica_b.plan_mode.granted_rung("org-owner", "thread-1"), None);
    let grant = model_gateway::session_flow::thread_grant(
        &harness.replica_b,
        "org-owner",
        "thread-1",
        &harness.owner.session,
    )
    .await
    .expect("the grant reads on another replica");
    assert_eq!(
        grant.rung,
        Some(mp_contracts::model_plane::v1::AutonomyRung::WorkspaceWrite)
    );
    assert_eq!(grant.approved_plan_id.as_deref(), Some("plan-7"));
    assert_eq!(grant.expires_at_unix, Some(expires));

    // The grant is the thread's own, and the org's: another thread or org reads no grant.
    for (org, thread) in [("org-other", "thread-1"), ("org-owner", "thread-2")] {
        let other = model_gateway::session_flow::thread_grant(
            &harness.replica_b,
            org,
            thread,
            &harness.owner.session,
        )
        .await
        .expect("the read answers");
        assert_eq!(other, model_gateway::session_flow::ThreadGrant::default(), "({org}, {thread})");
    }

    // The lifecycle event was published, once, by the replica that took the approval.
    let events = harness.publisher.drain();
    let exited: Vec<_> = events
        .iter()
        .filter(|(_, envelope)| envelope.event_type == "RUN_PLAN_MODE_EXITED")
        .collect();
    assert_eq!(exited.len(), 1, "{events:?}");
}

/// `was_in_plan_mode` is the durable run row's answer, so a run that already left plan mode
/// reports false and publishes nothing, whichever replica is asked.
#[tokio::test]
#[serial_test::serial]
async fn was_in_plan_mode_is_the_durable_runs_answer_not_this_replicas_memory() {
    let harness = durable_grant_harness(true).await;
    let (status, body) = post_durable_approval(&harness, "run-executed", &harness.owner).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["was_in_plan_mode"], false, "{body}");
    assert!(
        harness
            .publisher
            .drain()
            .iter()
            .all(|(_, envelope)| envelope.event_type != "RUN_PLAN_MODE_EXITED"),
        "no exit event for a run that was not in plan mode"
    );
    // The grant itself is still recorded: approving is granting, whatever the run's mode was.
    assert_eq!(harness.double.set_run_mode_calls.lock().unwrap().len(), 1);
}

/// With the switch off the approval is the one it always was: the grant lives in this replica's
/// memory with its in-memory expiry, and Session Core is told no plan id.
#[tokio::test]
#[serial_test::serial]
async fn with_the_switch_off_the_grant_stays_in_memory_and_session_core_is_told_no_plan() {
    let harness = durable_grant_harness(false).await;
    let (status, body) = post_durable_approval(&harness, "run-owned", &harness.owner).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        harness.plan_mode_a.granted_rung("org-owner", "thread-1"),
        Some(mp_contracts::model_plane::v1::AutonomyRung::WorkspaceWrite)
    );
    assert_eq!(
        Some(body["grant_expires_at_unix"].as_i64().expect("an expiry")),
        harness.plan_mode_a.grant_expires_at("org-owner", "thread-1")
    );
    assert_eq!(
        harness.plan_mode_a.take_approved_plan("org-owner", "thread-1"),
        Some("plan-7".to_owned())
    );
    let calls = harness.double.set_run_mode_calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].approved_plan_id, "", "the request is the one it always was");
    // A replica with no reader configured cannot read a grant: the memory half is per replica.
    let unread = model_gateway::session_flow::thread_grant(
        &harness.replica_b,
        "org-owner",
        "thread-1",
        &harness.owner.session,
    )
    .await
    .expect_err("no reader is configured while the switch is off");
    assert_eq!(unread.code(), tonic::Code::Unavailable);
}

/// A zero-retention credential records no grant, durable or not: the gateway refuses it before
/// any plan is read and Session Core is never called.
#[tokio::test]
#[serial_test::serial]
async fn a_zdr_credential_records_no_durable_grant() {
    let harness = durable_grant_harness(true).await;
    let tokens = user_tokens_with_retention("org-owner", "owner-user", true, true, None);
    let (status, body) = post_durable_approval(&harness, "run-owned", &tokens).await;
    assert_eq!(status, StatusCode::PRECONDITION_FAILED, "{body}");
    assert_eq!(body["error"]["code"], "zdr_durable_mutation_forbidden", "{body}");
    assert!(harness.double.set_run_mode_calls.lock().unwrap().is_empty());
    assert!(harness.double.grant.lock().unwrap().is_none());
    assert!(harness.capture.plan_calls.lock().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// WS04 S4 review fixes: the approval's durable write is retried and, if it still fails, the
// grant stays on the approving replica; a failed read of the grant is an error, not "no grant";
// the exit event follows the run's durable mode OR this replica's flag.
// ---------------------------------------------------------------------------

/// Defect 1: SetRunMode fails twice with a transient `Unavailable`, the approval retries it, and
/// the third attempt records the grant. The answer is `persisted: true`, the grant is Session
/// Core's, and this replica keeps no copy.
#[tokio::test]
#[serial_test::serial]
async fn an_approval_whose_durable_write_fails_twice_is_retried_and_persists() {
    let harness = durable_grant_harness(true).await;
    harness.double.set_run_mode_failures.store(2, Ordering::SeqCst);
    let (status, body) = post_durable_approval(&harness, "run-owned", &harness.owner).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["persisted"], true, "{body}");
    assert_eq!(harness.double.set_run_mode_attempts.load(Ordering::SeqCst), 3);
    assert_eq!(harness.double.set_run_mode_calls.lock().unwrap().len(), 1);
    assert!(harness.double.grant.lock().unwrap().is_some(), "Session Core recorded the grant");
    assert!(body["grant_expires_at_unix"].as_i64().is_some(), "{body}");
    assert_eq!(
        harness.plan_mode_a.granted_rung("org-owner", "thread-1"),
        None,
        "a durable grant leaves no copy in this replica's memory"
    );
}

/// Defect 1: SetRunMode never succeeds. The plan is already APPROVED and cannot be approved
/// again, so the approving replica keeps the grant in its own memory and keeps honouring it,
/// the answer says `persisted: false`, and the plan is not rolled back. The retry is bounded:
/// the first attempt and three retries, then it gives up.
#[tokio::test]
#[serial_test::serial]
async fn an_approval_whose_durable_write_always_fails_keeps_the_grant_on_this_replica() {
    let harness = durable_grant_harness(true).await;
    harness
        .double
        .set_run_mode_failures
        .store(FAIL_EVERY_SET_RUN_MODE, Ordering::SeqCst);
    let started = std::time::Instant::now();
    let (status, body) = post_durable_approval(&harness, "run-owned", &harness.owner).await;
    let waited = started.elapsed();
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["persisted"], false, "the answer is honest about it: {body}");
    assert_eq!(
        harness.double.set_run_mode_attempts.load(Ordering::SeqCst),
        4,
        "the first attempt and three retries"
    );
    assert!(
        waited >= std::time::Duration::from_millis(1_300),
        "the backoff waited 100, 300 and 900 ms: {waited:?}"
    );
    assert!(waited < std::time::Duration::from_secs(10), "and it is bounded: {waited:?}");
    assert!(harness.double.grant.lock().unwrap().is_none(), "Session Core holds nothing");

    // The plan moved to APPROVED and stayed there (it is not rolled back).
    assert_eq!(
        *harness.capture.plan_calls.lock().unwrap(),
        vec!["get_plan".to_owned(), "transition_plan".to_owned()]
    );
    // This replica keeps the grant, with its plan, and states when it lapses.
    assert_eq!(
        harness.plan_mode_a.granted_rung("org-owner", "thread-1"),
        Some(mp_contracts::model_plane::v1::AutonomyRung::WorkspaceWrite)
    );
    assert_eq!(
        harness.plan_mode_a.peek_approved_plan("org-owner", "thread-1"),
        Some("plan-7".to_owned())
    );
    assert_eq!(
        body["grant_expires_at_unix"].as_i64(),
        harness.plan_mode_a.grant_expires_at("org-owner", "thread-1"),
        "{body}"
    );
    // Another replica has no such copy: only the approving replica honours it.
    assert_eq!(harness.replica_b.plan_mode.granted_rung("org-owner", "thread-1"), None);
}

/// A refusal is not retried: it would only be refused again. And it is not a reason to keep the
/// grant in memory either (WS04 S4 second review, defect 0): a grant Session Core said no to is
/// not one this replica may honour. The plan stays APPROVED and the answer says `persisted:
/// false`.
#[tokio::test]
#[serial_test::serial]
async fn a_refused_durable_write_is_not_retried_and_keeps_no_grant() {
    for code in [
        tonic::Code::PermissionDenied,
        tonic::Code::FailedPrecondition,
        tonic::Code::InvalidArgument,
        tonic::Code::NotFound,
        tonic::Code::Unauthenticated,
    ] {
        let harness = durable_grant_harness(true).await;
        harness
            .double
            .set_run_mode_failures
            .store(FAIL_EVERY_SET_RUN_MODE, Ordering::SeqCst);
        *harness.double.set_run_mode_failure_code.lock().unwrap() = Some(code);
        let (status, body) = post_durable_approval(&harness, "run-owned", &harness.owner).await;
        assert_eq!(status, StatusCode::OK, "{code:?}: {body}");
        assert_eq!(body["persisted"], false, "{code:?}: {body}");
        assert_eq!(harness.double.set_run_mode_attempts.load(Ordering::SeqCst), 1, "{code:?}");
        assert_eq!(
            harness.plan_mode_a.granted_rung("org-owner", "thread-1"),
            None,
            "{code:?}: a refused grant is not kept in memory"
        );
        assert_eq!(harness.plan_mode_a.peek_approved_plan("org-owner", "thread-1"), None, "{code:?}");
        assert_eq!(body["grant_expires_at_unix"], serde_json::Value::Null, "{code:?}: {body}");
    }
}

/// The waiver of zero retention for one connection, as the issuer mints it.
fn waived_owner_tokens() -> UserTokens {
    let waiver = serde_json::json!({
        "version": 1,
        "provider": mp_contracts::agentic_subscription::PROVIDER_ID,
        "connection": "conn-1",
    });
    user_tokens_with_retention("org-owner", "owner-user", false, true, Some(waiver))
}

/// WS04 S4 second review, defect 1. Session Core refuses a `SetRunMode` that states a grant when
/// the credential is waived, and that refusal used to take the run's mode update down with it,
/// so every waived approval left the run in plan mode (and in production, the switch is off).
/// The gateway sends the mode change alone: no rung, no justification, no plan id. The run
/// leaves plan mode, no grant is claimed durable (`persisted: false`), and the approving replica
/// keeps the grant in memory, with the switch off and on.
#[tokio::test]
#[serial_test::serial]
async fn a_waived_approval_updates_the_run_mode_and_states_no_durable_grant() {
    for durable in [false, true] {
        let harness = durable_grant_harness(durable).await;
        let tokens = waived_owner_tokens();
        let (status, body) = post_durable_approval(&harness, "run-owned", &tokens).await;
        assert_eq!(status, StatusCode::OK, "durable={durable}: {body}");
        assert_eq!(body["persisted"], false, "durable={durable}: the grant is not durable: {body}");

        let calls = harness.double.set_run_mode_calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 1, "durable={durable}: the mode update was made once");
        assert_eq!(calls[0].run_id, "run-owned");
        assert_eq!(calls[0].mode, "execute", "durable={durable}: the run leaves plan mode");
        assert_eq!(calls[0].granted_rung, 0, "durable={durable}: no grant is stated");
        assert_eq!(calls[0].justification, "", "durable={durable}");
        assert_eq!(calls[0].approved_plan_id, "", "durable={durable}");
        assert!(harness.double.grant.lock().unwrap().is_none(), "durable={durable}: nothing on the thread");

        assert_eq!(
            harness.plan_mode_a.granted_rung("org-owner", "thread-1"),
            Some(mp_contracts::model_plane::v1::AutonomyRung::WorkspaceWrite),
            "durable={durable}: the approving replica keeps the grant in memory"
        );
        assert_eq!(
            body["grant_expires_at_unix"].as_i64(),
            harness.plan_mode_a.grant_expires_at("org-owner", "thread-1"),
            "durable={durable}: {body}"
        );
        assert_eq!(harness.replica_b.plan_mode.granted_rung("org-owner", "thread-1"), None);
    }
}

/// An ordinary (not waived) approval is unchanged: the grant is stated to Session Core, which
/// records it, with the switch off (no plan id) and on (the plan id).
#[tokio::test]
#[serial_test::serial]
async fn a_non_waived_approval_states_its_grant_to_session_core() {
    for (durable, plan_id) in [(false, ""), (true, "plan-7")] {
        let harness = durable_grant_harness(durable).await;
        let (status, body) = post_durable_approval(&harness, "run-owned", &harness.owner).await;
        assert_eq!(status, StatusCode::OK, "durable={durable}: {body}");
        let calls = harness.double.set_run_mode_calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].granted_rung,
            mp_contracts::model_plane::v1::AutonomyRung::WorkspaceWrite as i32,
            "durable={durable}"
        );
        assert_eq!(calls[0].justification, "writes the report into the workspace");
        assert_eq!(calls[0].approved_plan_id, plan_id, "durable={durable}");
        assert_eq!(calls[0].mode, "execute");
        assert_eq!(body["persisted"], true, "durable={durable}: {body}");
    }
}

/// With the switch off the approval is not retried at all: one attempt, as it always was.
#[tokio::test]
#[serial_test::serial]
async fn with_the_switch_off_the_durable_write_is_attempted_once() {
    let harness = durable_grant_harness(false).await;
    harness
        .double
        .set_run_mode_failures
        .store(FAIL_EVERY_SET_RUN_MODE, Ordering::SeqCst);
    let (status, body) = post_durable_approval(&harness, "run-owned", &harness.owner).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["persisted"], false, "{body}");
    assert_eq!(harness.double.set_run_mode_attempts.load(Ordering::SeqCst), 1);
    assert_eq!(
        harness.plan_mode_a.granted_rung("org-owner", "thread-1"),
        Some(mp_contracts::model_plane::v1::AutonomyRung::WorkspaceWrite),
        "the in-memory grant, as ever"
    );
}

/// Defect 0: every way the read of the grant can fail is an error the run start must treat as
/// unreadable. The double answers `Unavailable`, `Unimplemented` (an older Session Core with no
/// ThreadAutonomy service) and a refusal; an answer of "no grant" is the only `Ok` with no rung.
#[tokio::test]
#[serial_test::serial]
async fn a_failed_grant_read_is_an_error_and_only_an_ok_answer_is_no_grant() {
    let harness = durable_grant_harness(true).await;
    for code in [
        tonic::Code::Unavailable,
        tonic::Code::Unimplemented,
        tonic::Code::PermissionDenied,
        tonic::Code::Internal,
    ] {
        *harness.double.get_grant_failure.lock().unwrap() = Some(GrantReadFailure::Status(code));
        let error = model_gateway::session_flow::thread_grant(
            &harness.replica_b,
            "org-owner",
            "thread-1",
            &harness.owner.session,
        )
        .await
        .expect_err("a failing read is an error, never a grant with no rung");
        assert_eq!(error.code(), code);
    }
    *harness.double.get_grant_failure.lock().unwrap() = None;
    let none = model_gateway::session_flow::thread_grant(
        &harness.replica_b,
        "org-owner",
        "thread-1",
        &harness.owner.session,
    )
    .await
    .expect("an answer of no grant is Ok");
    assert_eq!(none, model_gateway::session_flow::ThreadGrant::default());
    assert!(!none.read_failed);
}

/// Defect 0: a Session Core that never answers ends at the gateway's own bound as
/// `DeadlineExceeded`, which the run start treats like any other failed read. Time is paused,
/// so the 5 s bound costs nothing.
#[tokio::test(start_paused = true)]
#[serial_test::serial]
async fn a_grant_read_that_never_answers_ends_at_its_bound() {
    let double = SessionCoreDouble::default();
    *double.get_grant_failure.lock().unwrap() = Some(GrantReadFailure::Hang);
    let (_session_client, grant_client) = spawn_session_core_double(&double).await;
    let mut state = AppState::new();
    state.plan_mode = model_gateway::coordinator::PlanModeStore::default()
        .with_durable_grant_enabled(true)
        .with_durable_grant_client(grant_client);
    let error = model_gateway::session_flow::thread_grant(&state, "org-owner", "thread-1", "token")
        .await
        .expect_err("a read that never answers is an error");
    assert_eq!(error.code(), tonic::Code::DeadlineExceeded);
}

/// Defect 3a, over HTTP: the run entered plan mode on THIS replica but the durable
/// SetRunMode(plan) was best-effort and failed, so the run row still says `execute`. The
/// durable write of the grant happens anyway, so the exit event, the only one carrying the
/// rung, the justification and the plan id, must be published.
#[tokio::test]
#[serial_test::serial]
async fn the_exit_event_is_published_when_only_this_replica_knew_the_run_was_in_plan_mode() {
    let harness = durable_grant_harness(true).await;
    model_gateway::coordinator::handle_enter_plan_mode(
        &harness.plan_mode_a,
        &InMemoryPublisher::new(),
        mp_contracts::model_plane::v1::EnterPlanModeRequest {
            request_id: "req-enter".to_owned(),
            org_id: "org-owner".to_owned(),
            run_id: "run-executed".to_owned(),
            session_id: "thread-1".to_owned(),
            rationale: "entered here, the durable write failed".to_owned(),
            ttl_seconds: 600,
        },
    )
    .await
    .expect("plan mode is entered on this replica");
    let (status, body) = post_durable_approval(&harness, "run-executed", &harness.owner).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["was_in_plan_mode"], true, "this replica's flag counts: {body}");
    let events = harness.publisher.drain();
    let exited: Vec<_> = events
        .iter()
        .filter(|(_, envelope)| envelope.event_type == "RUN_PLAN_MODE_EXITED")
        .collect();
    assert_eq!(exited.len(), 1, "{events:?}");
    assert_eq!(exited[0].1.payload["granted_rung"], "workspace_write");
    assert_eq!(exited[0].1.payload["plan_id"], "plan-7");
}
