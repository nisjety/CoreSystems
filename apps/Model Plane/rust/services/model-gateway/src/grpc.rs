//! gRPC server implementing the `ModelGateway` service on :9090.
//!
//! The gateway orchestrates:
//!  1. `MemoryService.SearchMemory` — best-effort retrieval of relevant
//!     memory entries scoped to the thread/org/query.
//!  2. `InferenceCore.Infer` / `InferStream` — provider routing for the
//!     final LLM call, with memory results injected as system context.

use chrono::Utc;
use mp_contracts::model_plane::v1::{
    model_gateway_server::{ModelGateway, ModelGatewayServer},
    AppendThreadMessageRequest, AppendThreadMessageResponse, ApproveApprovalRequest,
    ApproveApprovalResponse, ChatMessage, CheckPermissionRequest, CheckPermissionResponse,
    CreateTaskRequest, CreateTaskResponse, DenyApprovalRequest, DenyApprovalResponse,
    EnterPlanModeRequest, EnterPlanModeResponse, ExecuteCommandRequest, ExecuteCommandResponse,
    ExitPlanModeRequest, ExitPlanModeResponse, ExportTrajectoriesRequest,
    ExportTrajectoriesResponse, ExtractStructuredRequest, ExtractStructuredResponse, FetchRequest,
    FetchResponse, GetAnalyticsRequest, GetAnalyticsResponse, GetPolicyRequest, GetPolicyResponse,
    GetSkillRequest, GetSkillResponse, HealthRequest, HealthResponse, InferChunk, InferRequest,
    InvokeChunk, InvokeRequest, InvokeResponse, IsPlanModeRequest, IsPlanModeResponse,
    ListCommandsRequest, ListCommandsResponse, ListHooksRequest, ListHooksResponse,
    ListMcpServersRequest, ListMcpServersResponse, ListPendingApprovalsRequest,
    ListPendingApprovalsResponse, ListPluginsRequest, ListPluginsResponse, ListSkillsRequest,
    ListSkillsResponse, ListTasksRequest, ListTasksResponse, ListThreadMessagesRequest,
    ListThreadMessagesResponse, ListTrajectoriesRequest, ListTrajectoriesResponse, LspQueryRequest,
    LspQueryResponse, MatchSkillsRequest, MatchSkillsResponse, ProxyMcpToolRequest,
    ProxyMcpToolResponse, RecordTrajectoryRequest, RecordTrajectoryResponse, RegisterHookRequest,
    RegisterHookResponse, RegisterMcpServerRequest, RegisterMcpServerResponse,
    RegisterPluginRequest, RegisterPluginResponse, RemoteTriggerRequest, RemoteTriggerResponse,
    RenderHints as ProtoRenderHints, RequestApprovalRequest, RequestApprovalResponse,
    SearchMemoryRequest, SendMessageRequest, SendMessageResponse, SetPermissionRequest,
    SetPermissionResponse, SetPluginEnabledRequest, SetPluginEnabledResponse, SetPolicyRequest,
    SetPolicyResponse, SleepRequest, SleepResponse, SpeechToTextRequest, SpeechToTextResponse,
    SynthesizeSpeechRequest, SyntheticOutputRequest, SyntheticOutputResponse, TeamCreateRequest,
    TeamCreateResponse, TeamDeleteRequest, TeamDeleteResponse, TeamListRequest, TeamListResponse,
    TextToSpeechRequest, TextToSpeechResponse, TranscribeSpeechRequest, WebSearchRequest,
    WebSearchResponse,
};
use mp_events::{envelope::Envelope, publisher::EventPublisher, subjects};
use mp_ids::new_ulid;
use tokio_stream::StreamExt;
use tonic::{Request, Response, Status};
use tracing::{info, warn};

use crate::{
    approvals, coordinator, lsp,
    quarry::{QuarryError, RenderHints},
    runtime_registries, session_flow, skills,
    state::AppState,
    tools, trajectory,
};

/// Maximum page content (markdown chars) forwarded to inference-core
/// for ExtractStructured. Tuned to stay well below the 128k context
/// floor every in-use model shares, leaving headroom for the schema +
/// system prompt + caller instructions.
const MAX_EXTRACT_CONTENT_CHARS: usize = 80_000;

pub struct GatewayService {
    state: AppState,
}

fn request_id_or_new(request_id: &str) -> String {
    if request_id.trim().is_empty() {
        new_ulid()
    } else {
        request_id.to_owned()
    }
}

fn model_or_default(model: &str) -> String {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        "default".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn org_or_unknown(org_id: &str) -> String {
    let trimmed = org_id.trim();
    if trimmed.is_empty() {
        "org_unknown".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Best-effort memory search. Returns the matched memory contents or an
/// empty `Vec` on any error (memory is advisory context — inference still
/// proceeds when the memory backend is unavailable).
async fn fetch_memory_context(
    state: &AppState,
    thread_id: &str,
    org_id: &str,
    query: &str,
) -> Vec<String> {
    let mut client = state.memory_client.clone();
    let request = SearchMemoryRequest {
        thread_id: thread_id.to_owned(),
        query: query.to_owned(),
        topic_filter: Vec::new(),
        limit: 5,
        org_id: org_id.to_owned(),
        updated_after: None,
    };
    match client.search_memory(request).await {
        Ok(resp) => resp
            .into_inner()
            .entries
            .into_iter()
            .map(|e| e.content)
            .filter(|c| !c.trim().is_empty())
            .collect(),
        Err(error) => {
            warn!(%error, "memory search failed; continuing without context");
            Vec::new()
        }
    }
}

fn build_messages(memory_context: &[String], user_content: &str) -> Vec<ChatMessage> {
    let mut messages = Vec::with_capacity(2);
    if !memory_context.is_empty() {
        let joined = memory_context.join("\n---\n");
        messages.push(ChatMessage {
            role: "system".to_owned(),
            content: format!("Relevant memory:\n{joined}"),
            name: String::new(),
        });
    }
    messages.push(ChatMessage {
        role: "user".to_owned(),
        content: user_content.to_owned(),
        name: String::new(),
    });
    messages
}

fn build_infer_request(
    request_id: &str,
    org_id: &str,
    model: &str,
    provider_hint: &str,
    messages: Vec<ChatMessage>,
    req: &InvokeRequest,
) -> InferRequest {
    InferRequest {
        request_id: request_id.to_owned(),
        org_id: org_id.to_owned(),
        model: model.to_owned(),
        provider_hint: provider_hint.to_owned(),
        messages,
        temperature: req.temperature,
        max_tokens: req.max_tokens,
        structured_output_schema: req.structured_output_schema.clone(),
        zdr: req.zdr,
    }
}

async fn publish_ingress_accepted(
    state: &AppState,
    request_id: &str,
    org_id: &str,
    model: &str,
    content_length: usize,
) {
    let envelope = Envelope {
        event_id: new_ulid(),
        event_type: "INGRESS_ACCEPTED".to_owned(),
        schema_version: 1,
        ts: Utc::now(),
        producer: "model-gateway".to_owned(),
        correlation_id: request_id.to_owned(),
        causation_id: String::new(),
        idempotency_key: request_id.to_owned(),
        org_id: org_id.to_owned(),
        user_id: "grpc_user".to_owned(),
        resource_ref: format!("request/{request_id}"),
        payload: serde_json::json!({
            "content_length": content_length,
            "model": model,
            "transport": "grpc",
        }),
        zdr: false,
    };

    if let Err(error) = state
        .publisher
        .publish(&subjects::ingress_subject("accepted"), &envelope)
        .await
    {
        tracing::warn!(%error, "failed to publish INGRESS_ACCEPTED");
    }
}

#[tonic::async_trait]
impl ModelGateway for GatewayService {
    // Long orchestration handler (session preamble + cache + inference + usage);
    // the steps are sequential and clearer inline than split across helpers.
    #[allow(clippy::too_many_lines)]
    async fn invoke(
        &self,
        request: Request<InvokeRequest>,
    ) -> Result<Response<InvokeResponse>, Status> {
        let started = std::time::Instant::now();
        let req = request.into_inner();
        let content = req.content.trim().to_owned();
        if content.is_empty() {
            return Err(Status::invalid_argument("content must not be empty"));
        }

        let request_id = request_id_or_new(&req.request_id);
        let model = model_or_default(&req.model);
        let org_id = org_or_unknown(&req.org_id);
        let session_run = session_flow::prepare_run(
            &self.state,
            Some(&req.thread_id),
            Some(&req.session_key),
            &org_id,
            "grpc_user",
            &content,
        )
        .await
        .map_err(|error| Status::internal(format!("session-core prepare_run failed: {error}")))?;

        publish_ingress_accepted(&self.state, &request_id, &org_id, &model, content.len()).await;

        let memory_context =
            fetch_memory_context(&self.state, &session_run.thread_id, &org_id, &content).await;
        let messages = build_messages(&memory_context, &content);
        let infer_req =
            build_infer_request(&request_id, &org_id, &model, &req.provider, messages, &req);

        // Semantic-cache lookup (best-effort). On a hit we skip inference-core
        // entirely but still record the assistant turn so thread history stays
        // consistent. Cache hits report zero tokens to usage accounting.
        if let Some(cache) = crate::langcache::global() {
            if let Some(cached) = cache.lookup(&content, &org_id, &model).await {
                session_flow::append_assistant_message(
                    &self.state,
                    &session_run.thread_id,
                    &cached,
                )
                .await
                .map_err(|error| {
                    Status::internal(format!("session-core append assistant failed: {error}"))
                })?;
                info!(request_id = %request_id, "gateway invoke served from langcache");
                return Ok(Response::new(InvokeResponse {
                    request_id,
                    content: cached,
                    model_used: model,
                    stop_reason: "end_turn".to_owned(),
                    input_tokens: 0,
                    output_tokens: 0,
                    sources: Vec::new(),
                }));
            }
        }

        let mut client = self.state.inference_client.clone();
        let infer_resp = client.infer(infer_req).await.map_err(|status| {
            warn!(%status, "inference-core Infer failed");
            Status::internal(format!("inference failed: {}", status.message()))
        })?;
        let infer = infer_resp.into_inner();

        session_flow::append_assistant_message(&self.state, &session_run.thread_id, &infer.content)
            .await
            .map_err(|error| {
                Status::internal(format!("session-core append assistant failed: {error}"))
            })?;

        // Store the fresh response so future semantically-similar prompts hit
        // the cache. Best-effort: never fails the request.
        if let Some(cache) = crate::langcache::global() {
            cache.store(&content, &org_id, &model, &infer.content).await;
        }

        let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let usage_envelope = Envelope {
            event_id: new_ulid(),
            event_type: "USAGE_ENVELOPE".to_owned(),
            schema_version: 1,
            ts: Utc::now(),
            producer: "model-gateway".to_owned(),
            correlation_id: request_id.clone(),
            causation_id: String::new(),
            idempotency_key: format!("{request_id}-USAGE_ENVELOPE"),
            org_id: org_id.clone(),
            user_id: "grpc_user".to_owned(),
            resource_ref: format!("request/{request_id}"),
            payload: serde_json::json!({
                "request_id": request_id.clone(),
                "org_id": org_id.clone(),
                "user_id": "grpc_user",
                "model": infer.model_used.clone(),
                "input_tokens": infer.input_tokens,
                "output_tokens": infer.output_tokens,
                "latency_ms": latency_ms,
                "transport": "grpc",
            }),
            zdr: false,
        };

        if let Err(error) = self
            .state
            .publisher
            .publish(
                &subjects::usage_subject(&usage_envelope.org_id),
                &usage_envelope,
            )
            .await
        {
            warn!(%error, "failed to publish grpc USAGE_ENVELOPE");
        }

        info!(run_id = %session_run.run_id, thread_id = %session_run.thread_id, request_id = %request_id, "gateway invoke completed");

        Ok(Response::new(InvokeResponse {
            request_id,
            content: infer.content,
            model_used: if infer.model_used.is_empty() {
                model
            } else {
                infer.model_used
            },
            stop_reason: if infer.stop_reason.is_empty() {
                "end_turn".to_owned()
            } else {
                infer.stop_reason
            },
            input_tokens: infer.input_tokens,
            output_tokens: infer.output_tokens,
            sources: Vec::new(),
        }))
    }

    type InvokeStreamStream = tokio_stream::wrappers::ReceiverStream<Result<InvokeChunk, Status>>;

    // Long orchestration handler (session preamble + cache + streaming relay).
    #[allow(clippy::too_many_lines)]
    async fn invoke_stream(
        &self,
        request: Request<InvokeRequest>,
    ) -> Result<Response<Self::InvokeStreamStream>, Status> {
        let req = request.into_inner();
        let content = req.content.trim().to_owned();
        if content.is_empty() {
            return Err(Status::invalid_argument("content must not be empty"));
        }

        let request_id = request_id_or_new(&req.request_id);
        let model = model_or_default(&req.model);
        let org_id = org_or_unknown(&req.org_id);
        let session_run = session_flow::prepare_run(
            &self.state,
            Some(&req.thread_id),
            Some(&req.session_key),
            &org_id,
            "grpc_user",
            &content,
        )
        .await
        .map_err(|error| Status::internal(format!("session-core prepare_run failed: {error}")))?;

        publish_ingress_accepted(&self.state, &request_id, &org_id, &model, content.len()).await;

        let memory_context =
            fetch_memory_context(&self.state, &session_run.thread_id, &org_id, &content).await;
        let messages = build_messages(&memory_context, &content);
        let infer_req =
            build_infer_request(&request_id, &org_id, &model, &req.provider, messages, &req);

        // Semantic-cache lookup (best-effort). On a hit, stream the cached
        // response as a single terminal chunk, record the assistant turn, and
        // skip inference-core entirely.
        if let Some(cache) = crate::langcache::global() {
            if let Some(cached) = cache.lookup(&content, &org_id, &model).await {
                session_flow::append_assistant_message(
                    &self.state,
                    &session_run.thread_id,
                    &cached,
                )
                .await
                .map_err(|error| {
                    Status::internal(format!("session-core append assistant failed: {error}"))
                })?;
                let (tx, rx) = tokio::sync::mpsc::channel::<Result<InvokeChunk, Status>>(1);
                let _ = tx
                    .send(Ok(InvokeChunk {
                        request_id: request_id.clone(),
                        delta: cached,
                        done: true,
                        model_used: model.clone(),
                        input_tokens: 0,
                        output_tokens: 0,
                    }))
                    .await;
                info!(request_id = %request_id, "gateway invoke_stream served from langcache");
                return Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
                    rx,
                )));
            }
        }

        let mut client = self.state.inference_client.clone();
        let upstream = client.infer_stream(infer_req).await.map_err(|status| {
            warn!(%status, "inference-core InferStream failed");
            Status::internal(format!("inference stream failed: {}", status.message()))
        })?;
        let mut upstream_stream = upstream.into_inner();

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<InvokeChunk, Status>>(32);
        let fallback_model = model.clone();
        let fallback_request_id = request_id.clone();
        let state = self.state.clone();
        let thread_id = session_run.thread_id.clone();
        let run_id = session_run.run_id.clone();
        // Captured for the post-stream semantic-cache write.
        let cache_content = content.clone();
        let cache_org = org_id.clone();
        let cache_model = model.clone();

        tokio::spawn(async move {
            let mut assistant_output = String::new();
            while let Some(next) = upstream_stream.next().await {
                let send_result = match next {
                    Ok(InferChunk {
                        request_id: chunk_request_id,
                        delta,
                        done,
                        model_used,
                        input_tokens,
                        output_tokens,
                    }) => {
                        let rid = if chunk_request_id.is_empty() {
                            fallback_request_id.clone()
                        } else {
                            chunk_request_id
                        };
                        if !delta.is_empty() {
                            assistant_output.push_str(&delta);
                        }
                        let mu = if model_used.is_empty() && done {
                            fallback_model.clone()
                        } else {
                            model_used
                        };
                        tx.send(Ok(InvokeChunk {
                            request_id: rid,
                            delta,
                            done,
                            model_used: mu,
                            input_tokens,
                            output_tokens,
                        }))
                        .await
                    }
                    Err(status) => {
                        warn!(%status, "inference stream error");
                        tx.send(Err(Status::internal(format!(
                            "inference stream error: {}",
                            status.message()
                        ))))
                        .await
                    }
                };
                if send_result.is_err() {
                    return;
                }
            }

            if let Err(error) =
                session_flow::append_assistant_message(&state, &thread_id, &assistant_output).await
            {
                warn!(%error, %run_id, %thread_id, "failed to persist streamed assistant message");
            }

            // Store the fully-assembled streamed response for future cache hits.
            if let Some(cache) = crate::langcache::global() {
                cache
                    .store(&cache_content, &cache_org, &cache_model, &assistant_output)
                    .await;
            }
        });

        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
            rx,
        )))
    }

    async fn health(
        &self,
        _request: Request<HealthRequest>,
    ) -> Result<Response<HealthResponse>, Status> {
        Ok(Response::new(HealthResponse {
            status: "ok".to_owned(),
        }))
    }

    // ------------------------------------------------------------------
    // Wave 10a — trivial tool RPCs. Implementations live in
    // crate::tools to keep this file readable. Each handler is a thin
    // delegate; no business logic here.
    // ------------------------------------------------------------------

    async fn web_search(
        &self,
        request: Request<WebSearchRequest>,
    ) -> Result<Response<WebSearchResponse>, Status> {
        tools::handle_web_search(&self.state, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn sleep(
        &self,
        request: Request<SleepRequest>,
    ) -> Result<Response<SleepResponse>, Status> {
        tools::handle_sleep(request.into_inner())
            .await
            .map(Response::new)
    }

    async fn remote_trigger(
        &self,
        request: Request<RemoteTriggerRequest>,
    ) -> Result<Response<RemoteTriggerResponse>, Status> {
        tools::handle_remote_trigger(request.into_inner())
            .await
            .map(Response::new)
    }

    async fn send_message(
        &self,
        request: Request<SendMessageRequest>,
    ) -> Result<Response<SendMessageResponse>, Status> {
        tools::handle_send_message(&self.state, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn synthetic_output(
        &self,
        request: Request<SyntheticOutputRequest>,
    ) -> Result<Response<SyntheticOutputResponse>, Status> {
        tools::handle_synthetic_output(request.into_inner())
            .await
            .map(Response::new)
    }

    // ------------------------------------------------------------------
    // Wave 10b — plan mode + team workers. Stores live on AppState
    // (DashMap-backed). Tool dispatch middleware checks `is_plan_mode`
    // before allowing write-class tool calls.
    // ------------------------------------------------------------------

    async fn enter_plan_mode(
        &self,
        request: Request<EnterPlanModeRequest>,
    ) -> Result<Response<EnterPlanModeResponse>, Status> {
        let req = request.into_inner();
        let run_id = req.run_id.clone();
        let org_id = req.org_id.clone();
        let resp =
            coordinator::handle_enter_plan_mode(&self.state.plan_mode, &*self.state.publisher, req)
                .await?;
        // Durable run-mode write-through (ROADMAP P3 / matrix §4.1): persist
        // mode='plan' on the run so plan mode survives restart. Best-effort —
        // the in-memory plan_mode store is authoritative for the response.
        // org_id is forwarded so session-core can org-scope the UPDATE.
        if !run_id.is_empty() && !org_id.is_empty() {
            let mut client = self.state.session_client.clone();
            if let Err(e) = client
                .set_run_mode(mp_contracts::model_plane::v1::SetRunModeRequest {
                    run_id,
                    mode: "plan".to_owned(),
                    org_id,
                })
                .await
            {
                tracing::warn!(error = %e, "durable run-mode persist (plan) failed (best-effort)");
            }
        }
        Ok(Response::new(resp))
    }

    async fn exit_plan_mode(
        &self,
        request: Request<ExitPlanModeRequest>,
    ) -> Result<Response<ExitPlanModeResponse>, Status> {
        let req = request.into_inner();
        let run_id = req.run_id.clone();
        let org_id = req.org_id.clone();
        let resp =
            coordinator::handle_exit_plan_mode(&self.state.plan_mode, &*self.state.publisher, req)
                .await?;
        // Exiting plan mode returns the run to 'execute' durably. Best-effort.
        // org_id is forwarded so session-core can org-scope the UPDATE.
        if !run_id.is_empty() && !org_id.is_empty() {
            let mut client = self.state.session_client.clone();
            if let Err(e) = client
                .set_run_mode(mp_contracts::model_plane::v1::SetRunModeRequest {
                    run_id,
                    mode: "execute".to_owned(),
                    org_id,
                })
                .await
            {
                tracing::warn!(error = %e, "durable run-mode persist (execute) failed (best-effort)");
            }
        }
        Ok(Response::new(resp))
    }

    async fn is_plan_mode(
        &self,
        request: Request<IsPlanModeRequest>,
    ) -> Result<Response<IsPlanModeResponse>, Status> {
        coordinator::handle_is_plan_mode(&self.state.plan_mode, request.into_inner())
            .map(Response::new)
    }

    async fn team_create(
        &self,
        request: Request<TeamCreateRequest>,
    ) -> Result<Response<TeamCreateResponse>, Status> {
        coordinator::handle_team_create(&self.state.team_workers, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn team_delete(
        &self,
        request: Request<TeamDeleteRequest>,
    ) -> Result<Response<TeamDeleteResponse>, Status> {
        coordinator::handle_team_delete(&self.state.team_workers, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn team_list(
        &self,
        request: Request<TeamListRequest>,
    ) -> Result<Response<TeamListResponse>, Status> {
        coordinator::handle_team_list(&self.state.team_workers, request.into_inner())
            .await
            .map(Response::new)
    }

    // ------------------------------------------------------------------
    // Wave 10c — LSP bridge passthrough.
    // ------------------------------------------------------------------

    async fn lsp_query(
        &self,
        request: Request<LspQueryRequest>,
    ) -> Result<Response<LspQueryResponse>, Status> {
        lsp::handle_lsp_query(&self.state.lsp, request.into_inner())
            .await
            .map(Response::new)
    }

    // ------------------------------------------------------------------
    // Wave 10e — approval gate.
    // ------------------------------------------------------------------

    async fn request_approval(
        &self,
        request: Request<RequestApprovalRequest>,
    ) -> Result<Response<RequestApprovalResponse>, Status> {
        let resp = approvals::handle_request_approval(
            &self.state.approvals,
            &*self.state.publisher,
            request.into_inner(),
        )
        .await?;
        // Durable write-through (matrix §4.1): persist to session-core's
        // canonical store, best-effort. The in-memory store already holds the
        // authoritative response, so a backend failure never blocks the gate.
        if let Some(approval) = &resp.approval {
            approvals::persist_approval_request(
                &mut self.state.orchestration_client.clone(),
                approval,
            )
            .await;
        }
        Ok(Response::new(resp))
    }

    async fn approve_approval(
        &self,
        request: Request<ApproveApprovalRequest>,
    ) -> Result<Response<ApproveApprovalResponse>, Status> {
        let resp = approvals::handle_approve_approval(
            &self.state.approvals,
            &*self.state.publisher,
            request.into_inner(),
        )
        .await?;
        if let Some(approval) = &resp.approval {
            approvals::persist_approval_decision(
                &mut self.state.orchestration_client.clone(),
                approval,
            )
            .await;
        }
        Ok(Response::new(resp))
    }

    async fn deny_approval(
        &self,
        request: Request<DenyApprovalRequest>,
    ) -> Result<Response<DenyApprovalResponse>, Status> {
        let resp = approvals::handle_deny_approval(
            &self.state.approvals,
            &*self.state.publisher,
            request.into_inner(),
        )
        .await?;
        if let Some(approval) = &resp.approval {
            approvals::persist_approval_decision(
                &mut self.state.orchestration_client.clone(),
                approval,
            )
            .await;
        }
        Ok(Response::new(resp))
    }

    async fn list_pending_approvals(
        &self,
        request: Request<ListPendingApprovalsRequest>,
    ) -> Result<Response<ListPendingApprovalsResponse>, Status> {
        approvals::handle_list_pending_approvals(&self.state.approvals, request.into_inner())
            .await
            .map(Response::new)
    }

    // ------------------------------------------------------------------
    // Wave 10f — trajectory recording + JSONL export.
    // ------------------------------------------------------------------

    async fn record_trajectory(
        &self,
        request: Request<RecordTrajectoryRequest>,
    ) -> Result<Response<RecordTrajectoryResponse>, Status> {
        trajectory::handle_record_trajectory(
            &self.state.trajectories,
            &*self.state.publisher,
            request.into_inner(),
        )
        .await
        .map(Response::new)
    }

    async fn list_trajectories(
        &self,
        request: Request<ListTrajectoriesRequest>,
    ) -> Result<Response<ListTrajectoriesResponse>, Status> {
        trajectory::handle_list_trajectories(&self.state.trajectories, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn export_trajectories(
        &self,
        request: Request<ExportTrajectoriesRequest>,
    ) -> Result<Response<ExportTrajectoriesResponse>, Status> {
        trajectory::handle_export_trajectories(&self.state.trajectories, request.into_inner())
            .await
            .map(Response::new)
    }

    // ------------------------------------------------------------------
    // Wave 10d — skills loader.
    // ------------------------------------------------------------------

    async fn list_skills(
        &self,
        request: Request<ListSkillsRequest>,
    ) -> Result<Response<ListSkillsResponse>, Status> {
        skills::handle_list_skills(&self.state.skills, request.into_inner()).map(Response::new)
    }

    async fn get_skill(
        &self,
        request: Request<GetSkillRequest>,
    ) -> Result<Response<GetSkillResponse>, Status> {
        skills::handle_get_skill(&self.state.skills, request.into_inner()).map(Response::new)
    }

    async fn match_skills(
        &self,
        request: Request<MatchSkillsRequest>,
    ) -> Result<Response<MatchSkillsResponse>, Status> {
        skills::handle_match_skills(&self.state.skills, request.into_inner()).map(Response::new)
    }

    // ------------------------------------------------------------------
    // Wave 10g — MCP server hosting.
    // ------------------------------------------------------------------

    async fn register_mcp_server(
        &self,
        request: Request<RegisterMcpServerRequest>,
    ) -> Result<Response<RegisterMcpServerResponse>, Status> {
        let req = request.into_inner();
        let org_id = req.org_id.clone();
        let resp = runtime_registries::handle_register_mcp_server(&self.state.mcp, req)?;
        // Best-effort write-through to capability-core, the registry
        // system-of-record (matrix §4.1/H.1): converge the gateway's in-memory
        // store toward the SoR instead of shadowing it. In-memory stays
        // authoritative for THIS response; a catalog write failure must never
        // fail registration. Fire-and-forget so registration latency isn't
        // coupled to the catalog. Skipped when the base URL is unset (tests).
        if !self.state.capability_core_base_url.is_empty() {
            if let Some(server) = resp.server.as_ref() {
                if !org_id.is_empty() && !server.name.is_empty() {
                    let payload = runtime_registries::mcp_capability_payload(&org_id, server);
                    let url = format!("{}/api/v1/mcp", self.state.capability_core_base_url);
                    let client = self.state.http_client.clone();
                    tokio::spawn(async move {
                        match client.post(&url).json(&payload).send().await {
                            Ok(r) if !r.status().is_success() => {
                                tracing::warn!(status = %r.status(), "mcp catalog write-through non-2xx (best-effort)");
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "mcp catalog write-through failed (best-effort)");
                            }
                            _ => {}
                        }
                    });
                }
            }
        }
        Ok(Response::new(resp))
    }

    async fn list_mcp_servers(
        &self,
        request: Request<ListMcpServersRequest>,
    ) -> Result<Response<ListMcpServersResponse>, Status> {
        runtime_registries::handle_list_mcp_servers(&self.state.mcp, request.into_inner())
            .map(Response::new)
    }

    async fn proxy_mcp_tool(
        &self,
        request: Request<ProxyMcpToolRequest>,
    ) -> Result<Response<ProxyMcpToolResponse>, Status> {
        runtime_registries::handle_proxy_mcp_tool(&self.state.mcp, request.into_inner())
            .await
            .map(Response::new)
    }

    // ------------------------------------------------------------------
    // Wave 10h — plugins.
    // ------------------------------------------------------------------

    async fn register_plugin(
        &self,
        request: Request<RegisterPluginRequest>,
    ) -> Result<Response<RegisterPluginResponse>, Status> {
        runtime_registries::handle_register_plugin(&self.state.plugins, request.into_inner())
            .map(Response::new)
    }

    async fn list_plugins(
        &self,
        request: Request<ListPluginsRequest>,
    ) -> Result<Response<ListPluginsResponse>, Status> {
        runtime_registries::handle_list_plugins(&self.state.plugins, request.into_inner())
            .map(Response::new)
    }

    async fn set_plugin_enabled(
        &self,
        request: Request<SetPluginEnabledRequest>,
    ) -> Result<Response<SetPluginEnabledResponse>, Status> {
        runtime_registries::handle_set_plugin_enabled(&self.state.plugins, request.into_inner())
            .map(Response::new)
    }

    // ------------------------------------------------------------------
    // Wave 10i — commands / hooks / permissions / policy.
    // ------------------------------------------------------------------

    async fn list_commands(
        &self,
        request: Request<ListCommandsRequest>,
    ) -> Result<Response<ListCommandsResponse>, Status> {
        runtime_registries::handle_list_commands(&self.state.commands, request.into_inner())
            .map(Response::new)
    }

    async fn execute_command(
        &self,
        request: Request<ExecuteCommandRequest>,
    ) -> Result<Response<ExecuteCommandResponse>, Status> {
        runtime_registries::handle_execute_command(&self.state.commands, request.into_inner())
            .map(Response::new)
    }

    async fn register_hook(
        &self,
        request: Request<RegisterHookRequest>,
    ) -> Result<Response<RegisterHookResponse>, Status> {
        runtime_registries::handle_register_hook(&self.state.hooks, request.into_inner())
            .map(Response::new)
    }

    async fn list_hooks(
        &self,
        request: Request<ListHooksRequest>,
    ) -> Result<Response<ListHooksResponse>, Status> {
        runtime_registries::handle_list_hooks(&self.state.hooks, request.into_inner())
            .map(Response::new)
    }

    async fn check_permission(
        &self,
        request: Request<CheckPermissionRequest>,
    ) -> Result<Response<CheckPermissionResponse>, Status> {
        runtime_registries::handle_check_permission(&self.state.permissions, request.into_inner())
            .map(Response::new)
    }

    async fn set_permission(
        &self,
        request: Request<SetPermissionRequest>,
    ) -> Result<Response<SetPermissionResponse>, Status> {
        runtime_registries::handle_set_permission(&self.state.permissions, request.into_inner())
            .map(Response::new)
    }

    async fn get_policy(
        &self,
        request: Request<GetPolicyRequest>,
    ) -> Result<Response<GetPolicyResponse>, Status> {
        runtime_registries::handle_get_policy(&self.state.policy, request.into_inner())
            .map(Response::new)
    }

    async fn set_policy(
        &self,
        request: Request<SetPolicyRequest>,
    ) -> Result<Response<SetPolicyResponse>, Status> {
        runtime_registries::handle_set_policy(&self.state.policy, request.into_inner())
            .map(Response::new)
    }

    // ------------------------------------------------------------------
    // Wave 10j — messages, analytics, voice, tasks.
    // ------------------------------------------------------------------

    async fn append_thread_message(
        &self,
        request: Request<AppendThreadMessageRequest>,
    ) -> Result<Response<AppendThreadMessageResponse>, Status> {
        runtime_registries::handle_append_thread_message(&self.state.messages, request.into_inner())
            .map(Response::new)
    }

    async fn list_thread_messages(
        &self,
        request: Request<ListThreadMessagesRequest>,
    ) -> Result<Response<ListThreadMessagesResponse>, Status> {
        runtime_registries::handle_list_thread_messages(&self.state.messages, request.into_inner())
            .map(Response::new)
    }

    async fn get_analytics(
        &self,
        request: Request<GetAnalyticsRequest>,
    ) -> Result<Response<GetAnalyticsResponse>, Status> {
        runtime_registries::handle_get_analytics(&self.state.analytics, request.into_inner())
            .map(Response::new)
    }

    async fn text_to_speech(
        &self,
        request: Request<TextToSpeechRequest>,
    ) -> Result<Response<TextToSpeechResponse>, Status> {
        let req = request.into_inner();
        let request_id = request_id_or_new(&req.request_id);
        let resp = self
            .state
            .inference_client
            .clone()
            .synthesize_speech(SynthesizeSpeechRequest {
                request_id: request_id.clone(),
                org_id: req.org_id,
                text: req.text,
                voice: req.voice,
                format: req.format,
                model: String::new(),
                provider_hint: String::new(),
                language: String::new(),
            })
            .await?
            .into_inner();

        Ok(Response::new(TextToSpeechResponse {
            request_id: resp.request_id,
            audio: resp.audio,
            format: resp.format,
            error_message: String::new(),
        }))
    }

    async fn speech_to_text(
        &self,
        request: Request<SpeechToTextRequest>,
    ) -> Result<Response<SpeechToTextResponse>, Status> {
        let req = request.into_inner();
        let request_id = request_id_or_new(&req.request_id);
        let resp = self
            .state
            .inference_client
            .clone()
            .transcribe_speech(TranscribeSpeechRequest {
                request_id: request_id.clone(),
                org_id: req.org_id,
                audio: req.audio,
                format: req.format,
                model: String::new(),
                provider_hint: String::new(),
                language: req.language,
            })
            .await?
            .into_inner();

        Ok(Response::new(SpeechToTextResponse {
            request_id: resp.request_id,
            text: resp.text,
            detected_language: resp.detected_language,
            error_message: String::new(),
        }))
    }

    async fn create_task(
        &self,
        request: Request<CreateTaskRequest>,
    ) -> Result<Response<CreateTaskResponse>, Status> {
        runtime_registries::handle_create_task(&self.state.tasks, request.into_inner())
            .map(Response::new)
    }

    async fn list_tasks(
        &self,
        request: Request<ListTasksRequest>,
    ) -> Result<Response<ListTasksResponse>, Status> {
        runtime_registries::handle_list_tasks(&self.state.tasks, request.into_inner())
            .map(Response::new)
    }

    // ------------------------------------------------------------------
    // Wave 9 — Fetch + ExtractStructured.
    //
    // These two RPCs replace the dead Python web_fetch + extract_structured
    // tools (apps/Model Plane v2). Fetch is a pure pass-through to Quarry;
    // ExtractStructured fans out to Quarry for the fetch step and reuses
    // inference-core's existing `structured_output_schema` path for the
    // LLM coercion step. No new RPC on inference-core, no provider
    // changes — the v1 surface already had the field.
    // ------------------------------------------------------------------

    async fn fetch(
        &self,
        request: Request<FetchRequest>,
    ) -> Result<Response<FetchResponse>, Status> {
        let req = request.into_inner();
        if !self.state.quarry.available() {
            return Err(Status::unimplemented("quarry edge not configured"));
        }
        if req.url.trim().is_empty() {
            return Err(Status::invalid_argument("url is required"));
        }

        let render = render_hints_from_proto(req.render.as_ref());
        let result = self
            .state
            .quarry
            .scrape(&req.url, &req.org_id, render.as_ref(), req.prefer_http3)
            .await
            .map_err(quarry_err_to_status)?;

        Ok(Response::new(FetchResponse {
            request_id: req.request_id,
            url: result.url,
            final_url: result.final_url,
            status: i32::from(result.status),
            content_type: result.content_type,
            title: result.title,
            markdown: result.markdown,
            text: result.text,
            fingerprint: result.fingerprint,
            language: result.language,
        }))
    }

    async fn extract_structured(
        &self,
        request: Request<ExtractStructuredRequest>,
    ) -> Result<Response<ExtractStructuredResponse>, Status> {
        let req = request.into_inner();
        if !self.state.quarry.available() {
            return Err(Status::unimplemented("quarry edge not configured"));
        }
        if req.url.trim().is_empty() {
            return Err(Status::invalid_argument("url is required"));
        }
        if req.schema_json.trim().is_empty() {
            return Err(Status::invalid_argument("schema_json is required"));
        }

        // 1. Fetch via Quarry.
        let render = render_hints_from_proto(req.render.as_ref());
        let scrape = self
            .state
            .quarry
            .scrape(&req.url, &req.org_id, render.as_ref(), false)
            .await
            .map_err(quarry_err_to_status)?;

        // Prefer markdown; fall back to plain text.
        let mut content = if scrape.markdown.is_empty() {
            scrape.text.clone()
        } else {
            scrape.markdown.clone()
        };
        if content.is_empty() {
            // Fetch succeeded but the page had no extractable text.
            // Return OK with an error_message so the caller sees a
            // structured failure, not a gRPC error.
            return Ok(Response::new(ExtractStructuredResponse {
                request_id: req.request_id,
                url: req.url.clone(),
                final_url: scrape.final_url,
                title: scrape.title,
                source_fingerprint: scrape.fingerprint,
                error_message: "no extractable content from page".to_owned(),
                ..Default::default()
            }));
        }
        if content.len() > MAX_EXTRACT_CONTENT_CHARS {
            content.truncate(safe_char_boundary(&content, MAX_EXTRACT_CONTENT_CHARS));
        }

        // 2. Build the inference call. Single user message embeds the
        // page content + caller instructions; structured_output_schema
        // makes the provider enforce the shape end-to-end.
        let mut system_prompt = String::from(
            "You extract structured data from web pages. \
             Read the supplied page content carefully. \
             For any field where the page does not contain a clear answer, \
             omit the field — do not invent values. \
             Return only a JSON object matching the schema.",
        );
        if !req.instructions.is_empty() {
            system_prompt.push_str("\n\nAdditional caller instructions:\n");
            system_prompt.push_str(&req.instructions);
        }

        let title_header = if scrape.title.is_empty() {
            String::new()
        } else {
            format!("# {}\n\n", scrape.title)
        };
        let user_prompt = format!(
            "URL: {}\n\n--- PAGE CONTENT ---\n{}{}\n--- END PAGE CONTENT ---\n\n\
             Extract the requested fields per the JSON schema.",
            req.url, title_header, content,
        );

        let infer_req = InferRequest {
            request_id: req.request_id.clone(),
            org_id: req.org_id.clone(),
            model: req.model.clone(),
            provider_hint: req.provider.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".to_owned(),
                    content: system_prompt,
                    name: String::new(),
                },
                ChatMessage {
                    role: "user".to_owned(),
                    content: user_prompt,
                    name: String::new(),
                },
            ],
            temperature: 0.0,
            max_tokens: 2048,
            structured_output_schema: req.schema_json.clone(),
            zdr: req.zdr,
        };

        let mut client = self.state.inference_client.clone();
        match client.infer(Request::new(infer_req)).await {
            Ok(resp) => {
                let infer = resp.into_inner();
                Ok(Response::new(ExtractStructuredResponse {
                    request_id: req.request_id,
                    url: req.url.clone(),
                    final_url: scrape.final_url,
                    title: scrape.title,
                    source_fingerprint: scrape.fingerprint,
                    extracted_json: infer.content,
                    model_used: infer.model_used,
                    input_tokens: infer.input_tokens,
                    output_tokens: infer.output_tokens,
                    error_message: String::new(),
                }))
            }
            Err(err) => {
                // LLM failure. OK + structured error so the caller
                // still sees the fingerprint and can decide whether to
                // retry against the same page later.
                Ok(Response::new(ExtractStructuredResponse {
                    request_id: req.request_id,
                    url: req.url.clone(),
                    final_url: scrape.final_url,
                    title: scrape.title,
                    source_fingerprint: scrape.fingerprint,
                    error_message: format!("inference failed: {err}"),
                    ..Default::default()
                }))
            }
        }
    }
}

fn render_hints_from_proto(p: Option<&ProtoRenderHints>) -> Option<RenderHints> {
    let p = p?;
    if p.wait_for_selector.is_empty() {
        return None;
    }
    Some(RenderHints {
        wait_for_selector: Some(p.wait_for_selector.clone()),
        wait_for_timeout_ms: if p.wait_for_timeout_ms > 0 {
            #[allow(clippy::cast_sign_loss)]
            Some(p.wait_for_timeout_ms as u32)
        } else {
            None
        },
    })
}

/// Map Quarry typed errors onto gRPC status codes so callers can branch
/// on the canonical kind (PermissionDenied vs Unavailable vs
/// InvalidArgument) instead of parsing strings.
fn quarry_err_to_status(err: QuarryError) -> Status {
    match err {
        QuarryError::Unavailable => Status::unimplemented("quarry edge not configured"),
        QuarryError::Transport(e) => Status::unavailable(format!("quarry transport: {e}")),
        QuarryError::EmptyEnvelope => Status::internal("quarry: empty envelope"),
        QuarryError::Decode(e) => Status::internal(format!("quarry decode: {e}")),
        QuarryError::Typed { code, message, .. } => match code.as_str() {
            "BAD_REQUEST" | "INVALID_ARGUMENT" => Status::invalid_argument(message),
            "SECURITY_BLOCKED" | "FORBIDDEN" => {
                Status::permission_denied(format!("{code}: {message}"))
            }
            "RATE_LIMITED" => Status::resource_exhausted(message),
            "TIMEOUT" => Status::deadline_exceeded(message),
            "UPSTREAM_BLOCKED" => Status::unavailable(message),
            _ => Status::internal(format!("{code}: {message}")),
        },
    }
}

/// Truncate a UTF-8 string to at most `max` bytes without splitting a
/// codepoint. Falls back to 0 if no valid boundary exists (impossible
/// for non-empty strings).
fn safe_char_boundary(s: &str, max: usize) -> usize {
    if max >= s.len() {
        return s.len();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    end
}

/// Start the gRPC server on :9090.
///
/// # Errors
///
/// Returns an error if the server fails to bind.
pub async fn serve(state: AppState) -> anyhow::Result<()> {
    let addr = "0.0.0.0:9090".parse()?;
    info!("gRPC listening on :9090");

    tonic::transport::Server::builder()
        .add_service(ModelGatewayServer::new(GatewayService { state }))
        .serve(addr)
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::DynPublisher;
    use mp_contracts::model_plane::v1::{
        inference_core_client::InferenceCoreClient,
        inference_core_server::{InferenceCore, InferenceCoreServer},
        session_core_client::SessionCoreClient,
        session_core_server::{SessionCore, SessionCoreServer},
        AnalyzeDocumentRequest, AnalyzeDocumentResponse, AnalyzeImageRequest, AnalyzeImageResponse,
        AnalyzeLanguageRequest, AnalyzeLanguageResponse, AppendMessageRequest,
        AppendMessageResponse, BatchTranslateTextRequest, BatchTranslateTextResponse,
        CompactNowRequest, CompactNowResponse, CompleteStepRequest, CompleteStepResponse,
        CreateEmbeddingRequest, CreateEmbeddingResponse, CreateRealtimeSessionRequest,
        CreateRealtimeSessionResponse, CreateThreadRequest, CreateThreadResponse,
        CreateVideoGenerationJobRequest, CreateVideoGenerationJobResponse,
        DetectTextLanguageRequest, DetectTextLanguageResponse, Event, ExtractImageTextRequest,
        ExtractImageTextResponse, GenerateImageRequest, GenerateImageResponse, GeneratedImage,
        GetContextAssemblyRequest, GetContextAssemblyResponse, GetVideoGenerationJobRequest,
        GetVideoGenerationJobResponse, InferChunk, InferResponse, LanguageAnalysisResult,
        ListModelsRequest, ListModelsResponse, ListSpeechVoicesRequest, ListSpeechVoicesResponse,
        ListTranslationLanguagesRequest, ListTranslationLanguagesResponse, ModelInfo,
        ReplayThreadRequest, SaveCheckpointRequest, SaveCheckpointResponse, SpeechVoiceInfo,
        StartRunRequest, StartRunResponse, StreamVideoGenerationContentRequest,
        StreamVideoGenerationContentResponse, SynthesizeSpeechRequest, SynthesizeSpeechResponse,
        TranscribeSpeechRequest, TranscribeSpeechResponse, TranslateTextRequest,
        TranslateTextResponse, TranslationDetection, TranslationLanguageInfo,
    };
    use mp_events::publisher::InMemoryPublisher;
    use std::{pin::Pin, sync::Arc};
    use tokio::net::TcpListener;
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::{
        transport::{Endpoint, Server},
        Response,
    };

    type MockInferStream = Pin<Box<dyn futures::Stream<Item = Result<InferChunk, Status>> + Send>>;
    type MockVideoContentStream = Pin<
        Box<
            dyn futures::Stream<Item = Result<StreamVideoGenerationContentResponse, Status>> + Send,
        >,
    >;
    type MockReplayStream = Pin<Box<dyn futures::Stream<Item = Result<Event, Status>> + Send>>;

    struct MockInferenceOk;

    #[tonic::async_trait]
    impl InferenceCore for MockInferenceOk {
        type InferStreamStream = MockInferStream;
        type StreamVideoGenerationContentStream = MockVideoContentStream;

        async fn infer(&self, _: Request<InferRequest>) -> Result<Response<InferResponse>, Status> {
            Ok(Response::new(InferResponse {
                request_id: "req-ok".to_owned(),
                content: "hello".to_owned(),
                model_used: "mock".to_owned(),
                stop_reason: "stop".to_owned(),
                input_tokens: 1,
                output_tokens: 1,
            }))
        }

        async fn infer_stream(
            &self,
            _: Request<InferRequest>,
        ) -> Result<Response<Self::InferStreamStream>, Status> {
            Ok(Response::new(Box::pin(futures::stream::iter(vec![
                Ok(InferChunk {
                    request_id: "req-stream".to_owned(),
                    delta: "hel".to_owned(),
                    done: false,
                    model_used: "mock".to_owned(),
                    input_tokens: 0,
                    output_tokens: 0,
                }),
                Ok(InferChunk {
                    request_id: "req-stream".to_owned(),
                    delta: "lo".to_owned(),
                    done: true,
                    model_used: "mock".to_owned(),
                    input_tokens: 2,
                    output_tokens: 3,
                }),
            ]))))
        }

        async fn create_embedding(
            &self,
            _: Request<CreateEmbeddingRequest>,
        ) -> Result<Response<CreateEmbeddingResponse>, Status> {
            Ok(Response::new(CreateEmbeddingResponse {
                request_id: "embed-ok".to_owned(),
                vector: vec![0.1, 0.2, 0.3],
                model_used: "mock-embedding".to_owned(),
                provider_used: "mock".to_owned(),
            }))
        }

        async fn list_models(
            &self,
            _: Request<ListModelsRequest>,
        ) -> Result<Response<ListModelsResponse>, Status> {
            Ok(Response::new(ListModelsResponse {
                models: vec![ModelInfo {
                    id: "mock-embedding".to_owned(),
                    provider: "mock".to_owned(),
                    modality: "embedding".to_owned(),
                    streaming: false,
                }],
            }))
        }

        async fn synthesize_speech(
            &self,
            _: Request<SynthesizeSpeechRequest>,
        ) -> Result<Response<SynthesizeSpeechResponse>, Status> {
            Ok(Response::new(SynthesizeSpeechResponse {
                request_id: "speech-ok".to_owned(),
                audio: b"audio".to_vec(),
                format: "mp3".to_owned(),
                model_used: "mock-tts".to_owned(),
                provider_used: "mock".to_owned(),
                duration_ms: 0,
            }))
        }

        async fn transcribe_speech(
            &self,
            _: Request<TranscribeSpeechRequest>,
        ) -> Result<Response<TranscribeSpeechResponse>, Status> {
            Ok(Response::new(TranscribeSpeechResponse {
                request_id: "stt-ok".to_owned(),
                text: "hello".to_owned(),
                detected_language: "en".to_owned(),
                confidence: 1.0,
                model_used: "mock-stt".to_owned(),
                provider_used: "mock".to_owned(),
            }))
        }

        async fn list_speech_voices(
            &self,
            _: Request<ListSpeechVoicesRequest>,
        ) -> Result<Response<ListSpeechVoicesResponse>, Status> {
            Ok(Response::new(ListSpeechVoicesResponse {
                voices: vec![SpeechVoiceInfo {
                    id: "alloy".to_owned(),
                    name: "Alloy".to_owned(),
                    language: "*".to_owned(),
                    gender: "Neutral".to_owned(),
                    provider: "mock".to_owned(),
                }],
            }))
        }

        async fn translate_text(
            &self,
            _: Request<TranslateTextRequest>,
        ) -> Result<Response<TranslateTextResponse>, Status> {
            Ok(Response::new(TranslateTextResponse {
                request_id: "translate-ok".to_owned(),
                translated_text: "hei".to_owned(),
                detected_language: "en".to_owned(),
                confidence: 1.0,
                model_used: "mock-translation".to_owned(),
                provider_used: "mock".to_owned(),
            }))
        }

        async fn batch_translate_text(
            &self,
            _: Request<BatchTranslateTextRequest>,
        ) -> Result<Response<BatchTranslateTextResponse>, Status> {
            Ok(Response::new(BatchTranslateTextResponse {
                request_id: "batch-translate-ok".to_owned(),
                translations: Vec::new(),
                model_used: "mock-translation".to_owned(),
                provider_used: "mock".to_owned(),
            }))
        }

        async fn detect_text_language(
            &self,
            _: Request<DetectTextLanguageRequest>,
        ) -> Result<Response<DetectTextLanguageResponse>, Status> {
            Ok(Response::new(DetectTextLanguageResponse {
                request_id: "detect-ok".to_owned(),
                detections: vec![TranslationDetection {
                    language: "en".to_owned(),
                    confidence: 1.0,
                    is_translation_supported: true,
                }],
                model_used: "mock-translation".to_owned(),
                provider_used: "mock".to_owned(),
            }))
        }

        async fn list_translation_languages(
            &self,
            _: Request<ListTranslationLanguagesRequest>,
        ) -> Result<Response<ListTranslationLanguagesResponse>, Status> {
            Ok(Response::new(ListTranslationLanguagesResponse {
                languages: vec![TranslationLanguageInfo {
                    code: "en".to_owned(),
                    name: "English".to_owned(),
                    native_name: "English".to_owned(),
                    direction: "ltr".to_owned(),
                }],
            }))
        }

        async fn generate_image(
            &self,
            _: Request<GenerateImageRequest>,
        ) -> Result<Response<GenerateImageResponse>, Status> {
            Ok(Response::new(GenerateImageResponse {
                request_id: "image-ok".to_owned(),
                images: vec![GeneratedImage {
                    url: String::new(),
                    b64_json: "aW1hZ2U=".to_owned(),
                    revised_prompt: "mock image".to_owned(),
                }],
                model_used: "mock-image".to_owned(),
                provider_used: "mock".to_owned(),
            }))
        }

        async fn analyze_image(
            &self,
            _: Request<AnalyzeImageRequest>,
        ) -> Result<Response<AnalyzeImageResponse>, Status> {
            Ok(Response::new(AnalyzeImageResponse {
                request_id: "vision-ok".to_owned(),
                description: "mock description".to_owned(),
                model_used: "mock-vision".to_owned(),
                provider_used: "mock".to_owned(),
                input_tokens: 1,
                output_tokens: 1,
            }))
        }

        async fn extract_image_text(
            &self,
            _: Request<ExtractImageTextRequest>,
        ) -> Result<Response<ExtractImageTextResponse>, Status> {
            Ok(Response::new(ExtractImageTextResponse {
                request_id: "ocr-ok".to_owned(),
                text: "mock text".to_owned(),
                page_count: 0,
                model_used: "mock-ocr".to_owned(),
                provider_used: "mock".to_owned(),
            }))
        }

        async fn analyze_document(
            &self,
            _: Request<AnalyzeDocumentRequest>,
        ) -> Result<Response<AnalyzeDocumentResponse>, Status> {
            Ok(Response::new(AnalyzeDocumentResponse {
                request_id: "document-ok".to_owned(),
                status: "succeeded".to_owned(),
                content: "invoice text".to_owned(),
                fields_json: r#"{"VendorName":"ACME"}"#.to_owned(),
                tables_json: "[]".to_owned(),
                paragraphs: vec!["invoice text".to_owned()],
                raw_json: "{}".to_owned(),
                pages_processed: 1,
                confidence: 0.9,
                model_used: "prebuilt-invoice".to_owned(),
                provider_used: "mock".to_owned(),
            }))
        }

        async fn analyze_language(
            &self,
            _: Request<AnalyzeLanguageRequest>,
        ) -> Result<Response<AnalyzeLanguageResponse>, Status> {
            Ok(Response::new(AnalyzeLanguageResponse {
                request_id: "language-ok".to_owned(),
                operation: "sentiment".to_owned(),
                results: vec![LanguageAnalysisResult {
                    id: "1".to_owned(),
                    sentiment: "positive".to_owned(),
                    confidence_scores_json: r#"{"positive":0.9}"#.to_owned(),
                    sentences_json: "[]".to_owned(),
                    entities_json: "[]".to_owned(),
                    key_phrases: Vec::new(),
                    redacted_text: String::new(),
                    detected_language_name: String::new(),
                    detected_language_code: String::new(),
                    confidence: 0.0,
                    summary: String::new(),
                    raw_json: "{}".to_owned(),
                }],
                model_used: "mock-language".to_owned(),
                provider_used: "mock".to_owned(),
            }))
        }

        async fn create_realtime_session(
            &self,
            _: Request<CreateRealtimeSessionRequest>,
        ) -> Result<Response<CreateRealtimeSessionResponse>, Status> {
            Ok(Response::new(CreateRealtimeSessionResponse {
                request_id: "realtime-ok".to_owned(),
                session_id: "sess_mock".to_owned(),
                client_secret: "ek_mock".to_owned(),
                websocket_url: "wss://example.test/v1/realtime?model=mock-realtime".to_owned(),
                expires_at: 1234,
                model_used: "mock-realtime".to_owned(),
                provider_used: "mock".to_owned(),
                voice: "alloy".to_owned(),
            }))
        }

        async fn create_video_generation_job(
            &self,
            _: Request<CreateVideoGenerationJobRequest>,
        ) -> Result<Response<CreateVideoGenerationJobResponse>, Status> {
            Ok(Response::new(CreateVideoGenerationJobResponse {
                request_id: "video-ok".to_owned(),
                job_id: "job_mock".to_owned(),
                status: "queued".to_owned(),
                model_used: "mock-sora".to_owned(),
                provider_used: "mock".to_owned(),
                raw_json: r#"{"id":"job_mock","status":"queued"}"#.to_owned(),
            }))
        }

        async fn get_video_generation_job(
            &self,
            _: Request<GetVideoGenerationJobRequest>,
        ) -> Result<Response<GetVideoGenerationJobResponse>, Status> {
            Ok(Response::new(GetVideoGenerationJobResponse {
                request_id: "video-status-ok".to_owned(),
                job_id: "job_mock".to_owned(),
                status: "succeeded".to_owned(),
                generation_id: "gen_mock".to_owned(),
                video_url: "https://example.test/video.mp4".to_owned(),
                error: String::new(),
                model_used: "mock-sora".to_owned(),
                provider_used: "mock".to_owned(),
                raw_json: r#"{"id":"job_mock","status":"succeeded"}"#.to_owned(),
            }))
        }

        async fn stream_video_generation_content(
            &self,
            _: Request<StreamVideoGenerationContentRequest>,
        ) -> Result<Response<Self::StreamVideoGenerationContentStream>, Status> {
            Ok(Response::new(Box::pin(futures::stream::iter(vec![
                Ok(StreamVideoGenerationContentResponse {
                    request_id: "video-content-ok".to_owned(),
                    generation_id: "gen_mock".to_owned(),
                    data: b"mock-video".to_vec(),
                    done: false,
                    content_type: "video/mp4".to_owned(),
                    content_length: 10,
                    provider_used: "mock".to_owned(),
                }),
                Ok(StreamVideoGenerationContentResponse {
                    request_id: "video-content-ok".to_owned(),
                    generation_id: "gen_mock".to_owned(),
                    data: Vec::new(),
                    done: true,
                    content_type: "video/mp4".to_owned(),
                    content_length: 10,
                    provider_used: "mock".to_owned(),
                }),
            ]))))
        }
    }

    struct MockInferenceDown;

    #[tonic::async_trait]
    impl InferenceCore for MockInferenceDown {
        type InferStreamStream = MockInferStream;
        type StreamVideoGenerationContentStream = MockVideoContentStream;

        async fn infer(&self, _: Request<InferRequest>) -> Result<Response<InferResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn infer_stream(
            &self,
            _: Request<InferRequest>,
        ) -> Result<Response<Self::InferStreamStream>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn create_embedding(
            &self,
            _: Request<CreateEmbeddingRequest>,
        ) -> Result<Response<CreateEmbeddingResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn list_models(
            &self,
            _: Request<ListModelsRequest>,
        ) -> Result<Response<ListModelsResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn synthesize_speech(
            &self,
            _: Request<SynthesizeSpeechRequest>,
        ) -> Result<Response<SynthesizeSpeechResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn transcribe_speech(
            &self,
            _: Request<TranscribeSpeechRequest>,
        ) -> Result<Response<TranscribeSpeechResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn list_speech_voices(
            &self,
            _: Request<ListSpeechVoicesRequest>,
        ) -> Result<Response<ListSpeechVoicesResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn translate_text(
            &self,
            _: Request<TranslateTextRequest>,
        ) -> Result<Response<TranslateTextResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn batch_translate_text(
            &self,
            _: Request<BatchTranslateTextRequest>,
        ) -> Result<Response<BatchTranslateTextResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn detect_text_language(
            &self,
            _: Request<DetectTextLanguageRequest>,
        ) -> Result<Response<DetectTextLanguageResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn list_translation_languages(
            &self,
            _: Request<ListTranslationLanguagesRequest>,
        ) -> Result<Response<ListTranslationLanguagesResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn generate_image(
            &self,
            _: Request<GenerateImageRequest>,
        ) -> Result<Response<GenerateImageResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn analyze_image(
            &self,
            _: Request<AnalyzeImageRequest>,
        ) -> Result<Response<AnalyzeImageResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn extract_image_text(
            &self,
            _: Request<ExtractImageTextRequest>,
        ) -> Result<Response<ExtractImageTextResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn analyze_document(
            &self,
            _: Request<AnalyzeDocumentRequest>,
        ) -> Result<Response<AnalyzeDocumentResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn analyze_language(
            &self,
            _: Request<AnalyzeLanguageRequest>,
        ) -> Result<Response<AnalyzeLanguageResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn create_realtime_session(
            &self,
            _: Request<CreateRealtimeSessionRequest>,
        ) -> Result<Response<CreateRealtimeSessionResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn create_video_generation_job(
            &self,
            _: Request<CreateVideoGenerationJobRequest>,
        ) -> Result<Response<CreateVideoGenerationJobResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn get_video_generation_job(
            &self,
            _: Request<GetVideoGenerationJobRequest>,
        ) -> Result<Response<GetVideoGenerationJobResponse>, Status> {
            Err(Status::unavailable("down"))
        }

        async fn stream_video_generation_content(
            &self,
            _: Request<StreamVideoGenerationContentRequest>,
        ) -> Result<Response<Self::StreamVideoGenerationContentStream>, Status> {
            Err(Status::unavailable("down"))
        }
    }

    struct MockSessionCore;

    #[tonic::async_trait]
    impl SessionCore for MockSessionCore {
        type ReplayThreadStream = MockReplayStream;

        async fn create_thread(
            &self,
            request: Request<CreateThreadRequest>,
        ) -> Result<Response<CreateThreadResponse>, Status> {
            let req = request.into_inner();
            Ok(Response::new(CreateThreadResponse {
                thread_id: format!("thread-{}", req.session_key),
                created_at: None,
            }))
        }

        async fn append_message(
            &self,
            _: Request<AppendMessageRequest>,
        ) -> Result<Response<AppendMessageResponse>, Status> {
            Ok(Response::new(AppendMessageResponse { sequence: 1 }))
        }

        async fn start_run(
            &self,
            request: Request<StartRunRequest>,
        ) -> Result<Response<StartRunResponse>, Status> {
            let req = request.into_inner();
            Ok(Response::new(StartRunResponse {
                run_id: format!("run-{}", req.thread_id),
                created_at: None,
            }))
        }

        async fn complete_step(
            &self,
            _: Request<CompleteStepRequest>,
        ) -> Result<Response<CompleteStepResponse>, Status> {
            Err(Status::unimplemented("complete_step not needed in test"))
        }

        async fn save_checkpoint(
            &self,
            _: Request<SaveCheckpointRequest>,
        ) -> Result<Response<SaveCheckpointResponse>, Status> {
            Err(Status::unimplemented("save_checkpoint not needed in test"))
        }

        async fn replay_thread(
            &self,
            _: Request<ReplayThreadRequest>,
        ) -> Result<Response<Self::ReplayThreadStream>, Status> {
            Err(Status::unimplemented("replay_thread not needed in test"))
        }

        async fn get_context_assembly(
            &self,
            _: Request<GetContextAssemblyRequest>,
        ) -> Result<Response<GetContextAssemblyResponse>, Status> {
            Err(Status::unimplemented(
                "get_context_assembly not needed in test",
            ))
        }

        async fn compact_now(
            &self,
            _: Request<CompactNowRequest>,
        ) -> Result<Response<CompactNowResponse>, Status> {
            Err(Status::unimplemented("compact_now not needed in test"))
        }

        async fn upsert_agent_skill(
            &self,
            _: Request<mp_contracts::model_plane::v1::UpsertAgentSkillRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::UpsertAgentSkillResponse>, Status>
        {
            Err(Status::unimplemented(
                "upsert_agent_skill not needed in test",
            ))
        }

        async fn set_run_mode(
            &self,
            _: Request<mp_contracts::model_plane::v1::SetRunModeRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::SetRunModeResponse>, Status> {
            Err(Status::unimplemented("set_run_mode not needed in test"))
        }
    }

    async fn spawn_inference_client<S: InferenceCore>(
        service: S,
    ) -> InferenceCoreClient<tonic::transport::Channel> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind inference");
        let addr = listener.local_addr().expect("inference addr");
        tokio::spawn(async move {
            Server::builder()
                .add_service(InferenceCoreServer::new(service))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .ok();
        });
        let channel = Endpoint::from_shared(format!("http://{addr}"))
            .expect("inference endpoint")
            .connect()
            .await
            .expect("connect inference");
        InferenceCoreClient::new(channel)
    }

    async fn spawn_session_client<S: SessionCore>(
        service: S,
    ) -> SessionCoreClient<tonic::transport::Channel> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind session");
        let addr = listener.local_addr().expect("session addr");
        tokio::spawn(async move {
            Server::builder()
                .add_service(SessionCoreServer::new(service))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .ok();
        });
        let channel = Endpoint::from_shared(format!("http://{addr}"))
            .expect("session endpoint")
            .connect()
            .await
            .expect("connect session");
        SessionCoreClient::new(channel)
    }

    async fn test_service<S>(inference_service: S) -> (GatewayService, Arc<DynPublisher>)
    where
        S: InferenceCore,
    {
        let publisher = Arc::new(DynPublisher::InMemory(InMemoryPublisher::new()));
        let mut state = AppState::new();
        state.publisher = publisher.clone();
        state.inference_client = spawn_inference_client(inference_service).await;
        state.session_client = spawn_session_client(MockSessionCore).await;

        (GatewayService { state }, publisher)
    }

    fn make_request(content: &str) -> InvokeRequest {
        InvokeRequest {
            request_id: String::new(),
            org_id: "org_test".to_owned(),
            session_key: "sess-1".to_owned(),
            thread_id: String::new(),
            content: content.to_owned(),
            model: String::new(),
            provider: String::new(),
            max_tokens: 128,
            temperature: 0.3,
            stream: false,
            metadata: None,
            structured_output_schema: String::new(),
            zdr: false,
            max_cost_usd: 0.0,
            max_tokens_budget: 0,
        }
    }

    #[tokio::test]
    async fn invoke_rejects_empty_content() {
        let service = GatewayService {
            state: AppState::new(),
        };
        let response = service.invoke(Request::new(make_request("   "))).await;
        assert!(response.is_err());
        assert_eq!(
            response.expect_err("must fail").code(),
            tonic::Code::InvalidArgument
        );
    }

    #[tokio::test]
    async fn invoke_stream_rejects_empty_content() {
        let service = GatewayService {
            state: AppState::new(),
        };
        let response = service.invoke_stream(Request::new(make_request(""))).await;
        assert!(response.is_err());
        assert_eq!(
            response.expect_err("must fail").code(),
            tonic::Code::InvalidArgument
        );
    }

    #[tokio::test]
    async fn health_returns_ok() {
        let service = GatewayService {
            state: AppState::new(),
        };
        let resp = service
            .health(Request::new(HealthRequest {}))
            .await
            .expect("health ok")
            .into_inner();
        assert_eq!(resp.status, "ok");
    }

    #[tokio::test]
    async fn invoke_returns_inference_response_and_emits_events() {
        let (service, publisher) = test_service(MockInferenceOk).await;

        let response = service
            .invoke(Request::new(make_request("hello")))
            .await
            .expect("invoke ok")
            .into_inner();

        assert_eq!(response.content, "hello");
        assert_eq!(response.model_used, "mock");

        let drained = publisher.drain();
        assert!(drained
            .iter()
            .any(|(subject, _)| subject.starts_with("mp.v1.ingress.")));
        assert!(drained
            .iter()
            .any(|(subject, _)| subject.starts_with("mp.v1.usage.")));
    }

    #[tokio::test]
    async fn invoke_returns_internal_when_inference_unavailable() {
        let (service, publisher) = test_service(MockInferenceDown).await;

        let error = service
            .invoke(Request::new(make_request("hello")))
            .await
            .expect_err("invoke should fail");
        assert_eq!(error.code(), tonic::Code::Internal);

        let drained = publisher.drain();
        assert!(drained
            .iter()
            .any(|(subject, _)| subject.starts_with("mp.v1.ingress.")));
        assert!(!drained
            .iter()
            .any(|(subject, _)| subject.starts_with("mp.v1.usage.")));
    }

    #[tokio::test]
    async fn invoke_stream_forwards_chunks_and_done() {
        let (service, _) = test_service(MockInferenceOk).await;

        let response = service
            .invoke_stream(Request::new(make_request("hello")))
            .await
            .expect("invoke_stream ok")
            .into_inner();

        let chunks: Vec<_> = response.collect().await;
        assert_eq!(chunks.len(), 2);
        let first = chunks[0].as_ref().expect("first chunk ok");
        assert_eq!(first.delta, "hel");
        assert!(!first.done);
        let last = chunks[1].as_ref().expect("done chunk ok");
        assert_eq!(last.delta, "lo");
        assert!(last.done);
        assert_eq!(last.input_tokens, 2);
        assert_eq!(last.output_tokens, 3);
    }

    #[tokio::test]
    async fn invoke_stream_returns_internal_when_inference_unavailable() {
        let (service, _) = test_service(MockInferenceDown).await;

        let error = service
            .invoke_stream(Request::new(make_request("hello")))
            .await
            .expect_err("invoke_stream should fail");
        assert_eq!(error.code(), tonic::Code::Internal);
    }
}
