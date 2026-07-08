use axum::{
    body::to_bytes,
    body::Body,
    http::{header::AUTHORIZATION, Request, StatusCode},
};
use model_gateway::{http_routes::build_router, state::AppState};
use mp_contracts::model_plane::v1::{
    orchestration_core_service_client::OrchestrationCoreServiceClient,
    orchestration_core_service_server::{OrchestrationCoreService, OrchestrationCoreServiceServer},
    orchestration_event, Approval, ApprovalState, AttachSubagentRequest, AttachSubagentResponse,
    CreateApprovalRequest, CreateApprovalResponse, DecideApprovalRequest, DecideApprovalResponse,
    GetApprovalRequest, GetApprovalResponse, GetPlanRequest, GetPlanResponse,
    GetSubagentLineageRequest, GetSubagentLineageResponse, GetTodoRequest, GetTodoResponse,
    LineageEdge, ListApprovalsRequest, ListApprovalsResponse, ListPlansRequest, ListPlansResponse,
    ListTodosRequest, ListTodosResponse, OrchestrationEvent, OrgPendingApprovalsRequest,
    OrgPendingApprovalsResponse, Plan, PlanState, RecordOrchestrationEventRequest,
    RecordOrchestrationEventResponse, StreamRunEventsRequest, SubagentLineage, Todo, TodoState,
    TransitionPlanRequest, TransitionPlanResponse, TransitionTodoRequest, TransitionTodoResponse,
};
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

type MockEventStream =
    Pin<Box<dyn futures::Stream<Item = Result<OrchestrationEvent, Status>> + Send>>;

/// Captured `(id, state_int, actor, comment)` tuple recorded by a transition/decide RPC.
type CapturedTransition = Arc<Mutex<Option<(String, i32, String, String)>>>;

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

#[tokio::test]
#[serial_test::serial]
async fn orchestration_http_routes_proxy_requests() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
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
            .header(AUTHORIZATION, "Bearer dev")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "uri={uri}");
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
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    let (mock, capture) = MockOrchestration::new();
    let app = build_router(make_state(spawn_orchestration_mock(mock).await), None);

    let req = Request::builder()
        .method("GET")
        .uri("/v1/runs/run-33/events")
        .header(AUTHORIZATION, "Bearer dev")
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
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    let mock = OrgScopedMock::default();
    let app = build_router(make_state(spawn_org_scoped_mock(mock).await), None);

    // The owning org can read its own approval.
    let owner_req = Request::builder()
        .method("GET")
        .uri("/v1/orchestration/approvals/appr-owned")
        .header(AUTHORIZATION, "Bearer dev")
        .header("x-org-id", OWNER_ORG)
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
        .header(AUTHORIZATION, "Bearer dev")
        .header("x-org-id", OTHER_ORG)
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
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    let mock = OrgScopedMock::default();
    let (mock, last_decision) = (mock.clone(), mock.last_decision.clone());
    let app = build_router(make_state(spawn_org_scoped_mock(mock).await), None);

    // A different org cannot decide (approve) another org's approval.
    let intruder_req = Request::builder()
        .method("POST")
        .uri("/v1/orchestration/approvals/appr-owned/decide")
        .header(AUTHORIZATION, "Bearer dev")
        .header("x-org-id", OTHER_ORG)
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

    // The owning org can still decide its own approval.
    let owner_req = Request::builder()
        .method("POST")
        .uri("/v1/orchestration/approvals/appr-owned/decide")
        .header(AUTHORIZATION, "Bearer dev")
        .header("x-org-id", OWNER_ORG)
        .header("content-type", "application/json")
        .body(Body::from(r#"{"decision":"approve"}"#))
        .unwrap();
    let owner_resp = app.oneshot(owner_req).await.unwrap();
    assert_eq!(
        owner_resp.status(),
        StatusCode::OK,
        "owner org must succeed"
    );
    let (decided_org, decision) = last_decision.lock().unwrap().clone().unwrap();
    assert_eq!(decided_org, OWNER_ORG);
    assert_eq!(decision, ApprovalState::Granted as i32);
}

#[tokio::test]
#[serial_test::serial]
async fn list_approvals_is_scoped_to_the_callers_org() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    let mock = OrgScopedMock::default();
    let app = build_router(make_state(spawn_org_scoped_mock(mock).await), None);

    let owner_req = Request::builder()
        .method("GET")
        .uri("/v1/orchestration/runs/run-owned/approvals")
        .header(AUTHORIZATION, "Bearer dev")
        .header("x-org-id", OWNER_ORG)
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
        .header(AUTHORIZATION, "Bearer dev")
        .header("x-org-id", OTHER_ORG)
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
