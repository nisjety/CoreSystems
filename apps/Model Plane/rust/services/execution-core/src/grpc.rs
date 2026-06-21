//! gRPC server implementing `ExecutionCore` on :9093.

use mp_contracts::model_plane::v1::{
    self as pb,
    execution_core_server::{ExecutionCore, ExecutionCoreServer},
    orchestration_core_service_client::OrchestrationCoreServiceClient,
    session_core_client::SessionCoreClient,
};
use mp_ids::new_ulid;
use tonic::{Request, Response, Status};
use tracing::{info, warn};

use crate::runtime_loop;
use crate::state::{RunSnapshot, RunStatus, StateStore};

pub struct ExecutionService {
    state: StateStore,
    session_channel: tonic::transport::Channel,
    inference_channel: tonic::transport::Channel,
}

impl ExecutionService {
    pub fn new(
        state: StateStore,
        session_channel: tonic::transport::Channel,
        inference_channel: tonic::transport::Channel,
    ) -> Self {
        Self {
            state,
            session_channel,
            inference_channel,
        }
    }

    fn session_client(&self) -> SessionCoreClient<tonic::transport::Channel> {
        SessionCoreClient::new(self.session_channel.clone())
    }

    /// `OrchestrationCoreService` is hosted on session-core's gRPC server, so it
    /// shares the same channel.
    fn orchestration_client(&self) -> OrchestrationCoreServiceClient<tonic::transport::Channel> {
        OrchestrationCoreServiceClient::new(self.session_channel.clone())
    }

    /// Channel to inference-core for the agent run driver's `Infer` round.
    fn inference_channel(&self) -> tonic::transport::Channel {
        self.inference_channel.clone()
    }
}

#[tonic::async_trait]
impl ExecutionCore for ExecutionService {
    async fn execute_step(
        &self,
        request: Request<pb::ExecuteStepRequest>,
    ) -> Result<Response<pb::ExecuteStepResponse>, Status> {
        let req = request.into_inner();
        let run_id = req.run_id.clone();
        let step_id = req.step_id.clone();

        let prior = self.state.get_or_create(&run_id);

        // Surface browser-agent progress (B4) on the run-event stream. The sink
        // publishes BrowserActionDispatched/BrowserObservationReceived events to
        // session-core's orchestration broadcast (shared channel). Best-effort:
        // failures never fail the step.
        let browser_sink =
            crate::browser_events::OrchestrationEventSink::new(self.session_channel.clone());

        let outcome = runtime_loop::execute_step(
            &req.tool_name,
            &req.tool_input,
            &req.permission_mode,
            &req.hook_context,
            &req.org_id,
            // ExecuteStepRequest carries no user_id (proto gap) — this primitive
            // step RPC runs org-scoped. The agentic grounding path is RunAgent
            // (runtime_loop/agent.rs), which threads the run's verified user_id.
            // Follow-up: add user_id to ExecuteStepRequest so this path is
            // viewer-scoped too (tracked for PR-4).
            "",
            Some(&browser_sink),
        )
        .await;

        // HITL enforcement: when the posture gated this step, create the
        // durable Approval and pause the run. session-core broadcasts
        // RUN_PAUSED_FOR_APPROVAL so operator surfaces show the pause. Best
        // effort — the step is already paused regardless of the record write.
        if outcome.status == "awaiting_approval" {
            let mut orchestration = self.orchestration_client();
            if let Err(error) = orchestration
                .create_approval(pb::CreateApprovalRequest {
                    run_id: run_id.clone(),
                    step_id: step_id.clone(),
                    kind: pb::ApprovalKind::Destructive as i32,
                    requested_of: req.org_id.clone(),
                    org_id: req.org_id.clone(),
                    user_id: String::new(),
                    reason: format!("tool '{}' requires approval", req.tool_name),
                    expires_in_seconds: 3600,
                    // execution-core has no upstream cache id to align — let
                    // session-core mint the durable approval id (matrix §4.1).
                    client_approval_id: String::new(),
                    // Stable per-(run, step) idempotency key (D-1): a re-paused
                    // step collapses onto the existing durable approval via the
                    // (org_id, idempotency_key) ON CONFLICT guard.
                    idempotency_key: format!("{run_id}:{step_id}"),
                })
                .await
            {
                warn!(error = %error, run_id = %run_id, "failed to create approval for paused step");
            }
        }

        let status = match outcome.status.as_str() {
            "completed" => RunStatus::Completed,
            "awaiting_approval" => RunStatus::AwaitingApproval,
            _ => RunStatus::Failed,
        };

        let scrubbed_output = crate::scrub::scrub_string(&outcome.output);
        let scrubbed_error = crate::scrub::scrub_string(&outcome.error);

        let next = RunSnapshot {
            run_id: prior.run_id,
            step_index: prior.step_index.saturating_add(1),
            status,
            last_error: if scrubbed_error.is_empty() {
                None
            } else {
                Some(scrubbed_error.clone())
            },
        };

        let mut checkpoint_value = serde_json::json!({
            "run_id": next.run_id,
            "step_index": next.step_index,
            "status": next.status.as_str(),
            "last_error": next.last_error,
            "step": {
                "id": step_id,
                "status": outcome.status.clone(),
                "output": scrubbed_output.clone(),
                "error": scrubbed_error.clone(),
                "compaction_triggered": outcome.compaction_triggered,
            }
        });
        crate::scrub::scrub_json_value(&mut checkpoint_value);
        let checkpoint_bytes = serde_json::to_vec(&checkpoint_value).map_err(|error| {
            Status::internal(format!("checkpoint serialization failed: {error}"))
        })?;

        let mut session_client = self.session_client();
        session_client
            .save_checkpoint(pb::SaveCheckpointRequest {
                run_id: run_id.clone(),
                checkpoint_id: new_ulid(),
                state: checkpoint_bytes,
            })
            .await
            .map_err(|error| {
                Status::internal(format!("session-core save_checkpoint failed: {error}"))
            })?;

        session_client
            .complete_step(pb::CompleteStepRequest {
                run_id: run_id.clone(),
                step_id: step_id.clone(),
                status: outcome.status.clone(),
                output: scrubbed_output.clone(),
                error: scrubbed_error.clone(),
            })
            .await
            .map_err(|error| {
                Status::internal(format!("session-core complete_step failed: {error}"))
            })?;

        self.state.update(next);

        Ok(Response::new(pb::ExecuteStepResponse {
            step_id,
            status: outcome.status,
            output: scrubbed_output,
            error: scrubbed_error,
            compaction_triggered: outcome.compaction_triggered,
        }))
    }

    async fn resume_run(
        &self,
        request: Request<pb::ResumeRunRequest>,
    ) -> Result<Response<pb::ResumeRunResponse>, Status> {
        let req = request.into_inner();
        let mut snapshot = self.state.get_or_create(&req.run_id);
        let prior_status = snapshot.status;
        snapshot.status = RunStatus::Running;
        self.state.update(snapshot.clone());
        info!(
            run_id = %req.run_id,
            step_index = snapshot.step_index,
            from = prior_status.as_str(),
            "resume_run: run state set to Running"
        );

        Ok(Response::new(pb::ResumeRunResponse {
            resumed: true,
            step_index: snapshot.step_index,
        }))
    }

    async fn cancel_run(
        &self,
        request: Request<pb::CancelRunRequest>,
    ) -> Result<Response<pb::CancelRunResponse>, Status> {
        let req = request.into_inner();
        let cancelled = self.state.cancel(&req.run_id, Some(req.reason));

        Ok(Response::new(pb::CancelRunResponse { cancelled }))
    }

    /// Drive a whole agent run to a terminal answer (MVP no-tool slice).
    ///
    /// Delegates to [`runtime_loop::agent::run_agent`], which transitions the
    /// run's draft plan to executing, runs a single `InferenceCore.Infer`
    /// round, persists the assistant answer, and finalizes the run with one
    /// terminal `CompleteStep`. A run is never left `'queued'`: failures take
    /// the graceful path and still produce a terminal `"failed"` outcome.
    async fn run_agent(
        &self,
        request: Request<pb::RunAgentRequest>,
    ) -> Result<Response<pb::RunAgentResponse>, Status> {
        let req = request.into_inner();
        info!(
            run_id = %req.run_id,
            thread_id = %req.thread_id,
            mode = %req.mode,
            "run_agent: driving agent run (no-tool slice)"
        );
        let response = runtime_loop::agent::run_agent(
            &self.state,
            self.session_channel.clone(),
            self.inference_channel(),
            req,
        )
        .await;
        Ok(Response::new(response))
    }
}

/// Start the gRPC server on :9093.
///
/// # Errors
///
/// Returns an error if the server fails to bind.
pub async fn serve(state: StateStore) -> anyhow::Result<()> {
    let addr = "0.0.0.0:9093".parse()?;
    // Accept either env name: deployments wire `SESSION_CORE_ADDR` (compose),
    // while `SESSION_CORE_URL` is the documented primary. Reading only the
    // former silently fell back to localhost and broke the durable
    // create_approval / save_checkpoint path (gRPC "tcp connect error").
    let session_url = std::env::var("SESSION_CORE_URL")
        .or_else(|_| std::env::var("SESSION_CORE_ADDR"))
        .unwrap_or_else(|_| "http://localhost:9091".to_owned());
    let session_channel = tonic::transport::Endpoint::from_shared(session_url)?.connect_lazy();

    // inference-core backs the agent run driver's single Infer round (RunAgent).
    // Same dual env-name convention as session-core: compose wires
    // `INFERENCE_CORE_ADDR`, `INFERENCE_CORE_URL` is the documented primary.
    let inference_url = std::env::var("INFERENCE_CORE_URL")
        .or_else(|_| std::env::var("INFERENCE_CORE_ADDR"))
        .unwrap_or_else(|_| "http://inference-core:9092".to_owned());
    let inference_channel = tonic::transport::Endpoint::from_shared(inference_url)?.connect_lazy();
    info!("gRPC listening on :9093");

    tonic::transport::Server::builder()
        .add_service(ExecutionCoreServer::new(ExecutionService::new(
            state,
            session_channel,
            inference_channel,
        )))
        .serve(addr)
        .await?;

    Ok(())
}
