//! gRPC server implementing `ExecutionCore` on :9093.

use std::sync::Arc;
use std::time::Duration;

use mp_contracts::approval_cause::{self, CauseCode};
use mp_contracts::model_plane::v1::{
    self as pb,
    browser_broker_client::BrowserBrokerClient,
    execution_core_server::{ExecutionCore, ExecutionCoreServer},
    inference_core_client::InferenceCoreClient,
    orchestration_core_service_client::OrchestrationCoreServiceClient,
    run_service_client::RunServiceClient,
    session_core_client::SessionCoreClient,
};
use mp_ids::new_ulid;
use sha2::{Digest, Sha256};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{Request, Response, Status};
use tracing::{info, warn};

use crate::auth::{
    delegated_session_bearer, AuthenticatedService, AuthenticatedUser, DelegatedBrowserBearer,
    DelegatedSessionBearer, JwtVerifier,
};
use crate::capability_client::CapabilityClient;
use crate::http_health::Readiness;
use crate::runtime_loop;
use crate::sandbox_manager_client::SandboxManagerClient;
use crate::scheduled_inference_auth::ScheduledInferenceTokenProvider;
use crate::scheduled_step_decision::ScheduledStepDecisionVerifier;
use crate::session_terminal_auth::{ManagedRunTokenProvider, SessionTerminalTokenProvider};
use crate::state::{RunSnapshot, RunStatus, StateStore};
use crate::workspace_cas::CasClient;

/// Floor of the largest request tonic decodes: the 4 MiB default would refuse
/// a run request carrying attachment bytes (WS08-S5/S6).
const MAX_REQUEST_DECODING_BYTES: usize = 16 * 1024 * 1024;
/// Room for the rest of a request around its attachment bytes.
const REQUEST_DECODING_HEADROOM_BYTES: usize = 8 * 1024 * 1024;

pub(crate) struct ExecutionService {
    state: StateStore,
    auth: JwtVerifier,
    session_channel: tonic::transport::Channel,
    inference_channel: tonic::transport::Channel,
    browser_channel: tonic::transport::Channel,
    ownership: Arc<dyn RunOwnershipResolver>,
    capability_policy: Arc<dyn crate::capability_policy::CapabilityPolicy>,
    terminal_tokens: Arc<dyn ManagedRunTokenProvider>,
    scheduled_step_decision_verifier: Option<ScheduledStepDecisionVerifier>,
    scheduled_inference_tokens: Option<Arc<ScheduledInferenceTokenProvider>>,
    /// Absent exactly when `CapabilityClient::from_env` found no Control
    /// Plane configuration — a valid disabled state. A Space-scoped
    /// `code_interpreter` step fails closed without it (see
    /// `sandbox_lease::ensure_sandbox_lease`); every other step is unaffected.
    capability_client: Option<CapabilityClient>,
    sandbox_manager_client: SandboxManagerClient,
    /// Absent exactly when `CasClient::from_env` found no MinIO
    /// configuration — a valid disabled state, same convention as
    /// `capability_client` above. 3.5.C's own `code_interpreter.rs` rewrite
    /// (not yet landed) is this field's first real reader; nothing consumes
    /// it yet.
    cas_client: Option<CasClient>,
    /// This instance's own stable identifier — see
    /// `http_health::resolve_backend_id`'s doc for why the SAME value must be
    /// presented on every request from this process.
    backend_id: String,
    /// Mints execution-core's own `sandbox:write` service credential for
    /// `ReleaseLease` at a run's actual end (`cancel_run`, `run_agent`'s own
    /// `finalize()`) — never for `AcquireLease`, which needs the delegated
    /// user-bound bearer instead. See `sandbox_lease`'s module doc.
    sandbox_tokens: Arc<crate::sandbox_lease::SandboxManagerTokenProvider>,
    /// S4.2's background process host, or `None` when this instance does not
    /// run background processes (the default). Its only reader today is the
    /// lease-release path, which stops a lease's children before the
    /// workspace is uploaded; the tools that start one arrive in step 5.
    process_host: Option<Arc<crate::process_host::ProcessHost>>,
    /// Bundle reads for `code_interpreter` skill mounts (WS19-S8). Absent when their
    /// configuration is, and a step that names mounts then fails closed.
    skill_bundles: Option<Arc<crate::skill_mounts::GrpcSkillBundleSource>>,
}

#[tonic::async_trait]
trait RunOwnershipResolver: Send + Sync {
    async fn resolve(
        &self,
        run_id: &str,
        org_id: &str,
        user_id: &str,
        bearer: &str,
    ) -> Result<bool, Status>;
}

struct SessionCoreRunOwnershipResolver {
    channel: tonic::transport::Channel,
}

#[tonic::async_trait]
impl RunOwnershipResolver for SessionCoreRunOwnershipResolver {
    async fn resolve(
        &self,
        run_id: &str,
        org_id: &str,
        user_id: &str,
        bearer: &str,
    ) -> Result<bool, Status> {
        let mut client = RunServiceClient::new(self.channel.clone());
        client
            .resolve_run_owner(authenticated_session_request(
                pb::ResolveRunOwnerRequest {
                    run_id: run_id.to_owned(),
                    org_id: org_id.to_owned(),
                    user_id: user_id.to_owned(),
                },
                bearer,
            )?)
            .await
            .map(|response| response.into_inner().authorized)
            .map_err(|error| {
                // A typed refusal from Session Core (owner decision 17) is the answer and passes through,
                // so the caller sees the reason rather than a retryable outage.
                if mp_contracts::zdr_refusal::Refusal::from_status(&error).is_some() {
                    return error;
                }
                warn!(code = ?error.code(), "Session Core run ownership lookup unavailable");
                Status::unavailable("run ownership unavailable")
            })
    }
}

impl ExecutionService {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_with_scheduled_step_runtime(
        state: StateStore,
        auth: JwtVerifier,
        session_channel: tonic::transport::Channel,
        inference_channel: tonic::transport::Channel,
        browser_channel: tonic::transport::Channel,
        capability_policy: Arc<dyn crate::capability_policy::CapabilityPolicy>,
        terminal_tokens: Arc<dyn ManagedRunTokenProvider>,
        scheduled_step_decision_verifier: Option<ScheduledStepDecisionVerifier>,
        scheduled_inference_tokens: Option<Arc<ScheduledInferenceTokenProvider>>,
        capability_client: Option<CapabilityClient>,
        sandbox_manager_client: SandboxManagerClient,
        cas_client: Option<CasClient>,
        backend_id: String,
        sandbox_tokens: Arc<crate::sandbox_lease::SandboxManagerTokenProvider>,
        skill_bundles: Option<Arc<crate::skill_mounts::GrpcSkillBundleSource>>,
    ) -> Self {
        let ownership = Arc::new(SessionCoreRunOwnershipResolver {
            channel: session_channel.clone(),
        });
        // Built here rather than passed in: everything it needs is already an
        // argument, so no caller grows a parameter for a capability that is
        // off by default. `None` is the ordinary case — see
        // `ProcessHost::from_env`, which returns it unless an operator asked
        // for background processes AND bwrap can actually isolate them.
        let process_host = crate::process_host::ProcessHost::from_env(
            sandbox_manager_client.clone(),
            sandbox_tokens.clone(),
            &backend_id,
        )
        .map(Arc::new);
        Self {
            state,
            auth,
            session_channel,
            inference_channel,
            browser_channel,
            ownership,
            capability_policy,
            terminal_tokens,
            scheduled_step_decision_verifier,
            scheduled_inference_tokens,
            capability_client,
            sandbox_manager_client,
            cas_client,
            backend_id,
            sandbox_tokens,
            process_host,
            skill_bundles,
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

    async fn validate_browser_grant(
        &self,
        tool_input: &str,
        browser_bearer: &DelegatedBrowserBearer,
    ) -> Result<crate::tool_bridge::ValidatedBrowserGrant, Status> {
        let grant_id = crate::tool_bridge::requested_browser_grant_id(tool_input)
            .map_err(|_| Status::invalid_argument("browser grant_id is required"))?;
        let request = authenticated_browser_request(
            pb::ValidateGrantRequest {
                grant_id: grant_id.clone(),
            },
            browser_bearer.as_str(),
        )?;
        let mut client = BrowserBrokerClient::new(self.browser_channel.clone());
        let response = tokio::time::timeout(Duration::from_secs(3), client.validate_grant(request))
            .await
            .map_err(|_| Status::unavailable("browser grant validation unavailable"))?
            .map_err(|error| {
                warn!(code = ?error.code(), "browser grant validation failed");
                if matches!(
                    error.code(),
                    tonic::Code::Unavailable | tonic::Code::DeadlineExceeded
                ) {
                    Status::unavailable("browser grant validation unavailable")
                } else {
                    // Do not disclose whether a forged or cross-tenant grant exists.
                    Status::permission_denied("browser grant is not authorized")
                }
            })?
            .into_inner();
        crate::tool_bridge::ValidatedBrowserGrant::from_broker_response(&grant_id, &response)
            .map_err(|_| Status::permission_denied("browser grant is not authorized"))
    }

    async fn authorize_run(
        &self,
        caller: &AuthenticatedUser,
        run_id: &str,
        session_bearer: &DelegatedSessionBearer,
    ) -> Result<(), Status> {
        // Every user bearer of a run carries the caller's own retention posture
        // (Z-18, matrix rule 1). Nothing else on this path judges the session
        // bearer, so it is judged here, before the run's authority is used.
        self.auth
            .bind_session_bearer(session_bearer, caller)
            .await?;
        authorize_durable_run_owner(
            &self.state,
            self.ownership.as_ref(),
            caller,
            run_id,
            session_bearer,
        )
        .await
    }

    /// Run the bounded, service-owned scheduled inference lane. Session Core
    /// is the idempotency authority: a step is claimed before inference and a
    /// metadata-only receipt is recorded after it. A transport-ambiguous
    /// provider response is explicitly `unknown_outcome`, never a blind retry.
    async fn execute_scheduled_step_runtime(
        &self,
        req: pb::ExecuteScheduledStepRequest,
    ) -> Result<Response<pb::ExecuteScheduledStepResponse>, Status> {
        let inference_tokens = self.scheduled_inference_tokens.as_ref().ok_or_else(|| {
            Status::failed_precondition(
                "scheduled-step inference service credential is not configured",
            )
        })?;
        let session_bearer = self
            .terminal_tokens
            .scheduled_step_token(&req.org_id)
            .await
            .map_err(|error| {
                warn!(%error, "scheduled-step Session credential unavailable");
                Status::unavailable("scheduled-step Session credential unavailable")
            })?;
        // Mint the effect credential before claiming the step.  A missing
        // service credential must not leave a durable `claimed` receipt that
        // no worker can ever complete.
        let inference_bearer = inference_tokens.token(&req.org_id).await.map_err(|error| {
            warn!(%error, "scheduled-step inference credential unavailable");
            Status::unavailable("scheduled-step inference credential unavailable")
        })?;

        let claim = self
            .session_client()
            .claim_scheduled_step(authenticated_session_request(
                pb::ClaimScheduledStepRequest {
                    run_id: req.run_id.clone(),
                    thread_id: req.thread_id.clone(),
                    org_id: req.org_id.clone(),
                    space_id: req.space_id.clone(),
                    schedule_id: req.schedule_id.clone(),
                    fire_key: req.fire_key.clone(),
                    template_digest: req.template_digest.clone(),
                    step_id: req.step_id.clone(),
                    step_index: req.step_index,
                    policy_digest: req.policy_digest.clone(),
                    idempotency_key: req.idempotency_key.clone(),
                },
                &session_bearer,
            )?)
            .await
            .map_err(|error| {
                warn!(code = ?error.code(), "scheduled-step Session claim failed");
                if matches!(
                    error.code(),
                    tonic::Code::Unavailable | tonic::Code::DeadlineExceeded
                ) {
                    Status::unavailable("scheduled-step Session claim unavailable")
                } else {
                    error
                }
            })?
            .into_inner();

        if !claim.claimed {
            if claim.status == "claimed" {
                return Err(Status::aborted(
                    "scheduled step is already claimed; reconcile its receipt before retrying",
                ));
            }
            return Ok(Response::new(pb::ExecuteScheduledStepResponse {
                step_id: req.step_id,
                status: claim.status.clone(),
                receipt_id: claim.receipt_id,
                output: String::new(),
                error: String::new(),
                unknown_outcome: claim.status == "unknown_outcome",
                ..Default::default()
            }));
        }

        let run = match RunServiceClient::new(self.session_channel.clone())
            .get_scheduled_step_context(authenticated_session_request(
                pb::GetScheduledStepContextRequest {
                    run_id: req.run_id.clone(),
                    thread_id: req.thread_id.clone(),
                    org_id: req.org_id.clone(),
                },
                &session_bearer,
            )?)
            .await
        {
            Ok(response) => response.into_inner(),
            Err(error) => {
                warn!(code = ?error.code(), "scheduled-step run lookup failed");
                // The claim was durably accepted but the owner metadata could
                // not be read. Preserve that uncertainty for reconciliation;
                // never leave a silent `claimed` row that a retry could repeat.
                let _ = self
                    .record_scheduled_step_receipt(
                        &req,
                        &session_bearer,
                        &claim.receipt_id,
                        "unknown_outcome",
                        "",
                        "run_lookup_unknown",
                        true,
                    )
                    .await;
                return Err(Status::unavailable("scheduled-step run lookup unavailable"));
            }
        };
        if run.thread_id != req.thread_id || matches!(run.status.as_str(), "cancelled" | "failed") {
            let error_code = if run.status == "cancelled" {
                "run_cancelled"
            } else {
                "run_not_executable"
            };
            let receipt = self
                .record_scheduled_step_receipt(
                    &req,
                    &session_bearer,
                    &claim.receipt_id,
                    "failed",
                    "",
                    error_code,
                    false,
                )
                .await?;
            return Ok(Response::new(pb::ExecuteScheduledStepResponse {
                step_id: req.step_id,
                status: receipt.status,
                receipt_id: receipt.receipt_id,
                output: String::new(),
                error: error_code.to_owned(),
                unknown_outcome: false,
                ..Default::default()
            }));
        }
        if run.goal.trim().is_empty() {
            let receipt = self
                .record_scheduled_step_receipt(
                    &req,
                    &session_bearer,
                    &claim.receipt_id,
                    "failed",
                    "",
                    "missing_scheduled_goal",
                    false,
                )
                .await?;
            return Ok(Response::new(pb::ExecuteScheduledStepResponse {
                step_id: req.step_id,
                status: receipt.status,
                receipt_id: receipt.receipt_id,
                output: String::new(),
                error: "missing_scheduled_goal".to_owned(),
                unknown_outcome: false,
                ..Default::default()
            }));
        }

        // A step whose schedule stored a consent contract takes the bounded agent lane when
        // its switch is on (WS20 S6). Without a contract, or with the switch off, the
        // single-inference lane below runs byte-for-byte as before.
        if !run.contract_json.is_empty() && runtime_loop::scheduled_agent::enabled() {
            return self
                .execute_scheduled_agent_step(
                    &req,
                    &run,
                    &session_bearer,
                    &inference_bearer,
                    &claim.receipt_id,
                )
                .await;
        }

        let inference_request = scheduled_inference_request(&req, &run.goal);
        let inference_result = match tokio::time::timeout(
            Duration::from_secs(60),
            InferenceCoreClient::new(self.inference_channel.clone()).infer(
                authenticated_inference_request(inference_request, &inference_bearer)?,
            ),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Err(Status::deadline_exceeded(
                "scheduled-step inference outcome is unknown",
            )),
        };
        match inference_result {
            Ok(response) => {
                let output = response.into_inner().content;
                // With the result switch on, the answer is persisted into the thread
                // and the fire records the run (build plan WS20 S1). Off keeps the
                // receipt-only path byte-for-byte.
                let (receipt_id, status) = if scheduled_result_persist_enabled() {
                    let receipt = self
                        .record_scheduled_run_result(&req, &session_bearer, &claim.receipt_id, &output)
                        .await?;
                    (receipt.receipt_id, receipt.status)
                } else {
                    let output_digest = format!("sha256:{:x}", Sha256::digest(output.as_bytes()));
                    let receipt = self
                        .record_scheduled_step_receipt(
                            &req,
                            &session_bearer,
                            &claim.receipt_id,
                            "completed",
                            &output_digest,
                            "",
                            false,
                        )
                        .await?;
                    (receipt.receipt_id, receipt.status)
                };
                Ok(Response::new(pb::ExecuteScheduledStepResponse {
                    step_id: req.step_id,
                    status,
                    receipt_id,
                    output,
                    error: String::new(),
                    unknown_outcome: false,
                    ..Default::default()
                }))
            }
            Err(error) => {
                let (status, error_code, unknown) = scheduled_step_failure_status(error.code());
                let receipt = self
                    .record_scheduled_step_receipt(
                        &req,
                        &session_bearer,
                        &claim.receipt_id,
                        status,
                        "",
                        error_code,
                        unknown,
                    )
                    .await?;
                Ok(Response::new(pb::ExecuteScheduledStepResponse {
                    step_id: req.step_id,
                    status: receipt.status,
                    receipt_id: receipt.receipt_id,
                    output: String::new(),
                    error: error_code.to_owned(),
                    unknown_outcome: unknown,
                    ..Default::default()
                }))
            }
        }
    }

    /// Complete a claimed step with its answer through Session Core's result RPC
    /// (build plan WS20 S1). The answer is content, and it travels only on this
    /// direct hop.
    async fn record_scheduled_run_result(
        &self,
        req: &pb::ExecuteScheduledStepRequest,
        bearer: &str,
        receipt_id: &str,
        output: &str,
    ) -> Result<pb::RecordScheduledRunResultResponse, Status> {
        self.session_client()
            .record_scheduled_run_result(authenticated_session_request(
                scheduled_run_result_request(req, receipt_id, output),
                bearer,
            )?)
            .await
            .map(|response| response.into_inner())
            .map_err(|error| {
                warn!(code = ?error.code(), "scheduled-step result persistence failed");
                Status::unavailable("scheduled-step result persistence unavailable")
            })
    }

    /// Run one scheduled Do step in the bounded agent loop (WS20 S6). The stored contract is
    /// verified again here, then the loop runs under the service identity, unattended: a gated
    /// effect is withheld and counted, never paused for. A transport loss or the wall clock is
    /// an unknown outcome, reconciled from the receipt and never retried.
    async fn execute_scheduled_agent_step(
        &self,
        req: &pb::ExecuteScheduledStepRequest,
        run: &pb::ScheduledStepContext,
        session_bearer: &str,
        inference_bearer: &str,
        receipt_id: &str,
    ) -> Result<Response<pb::ExecuteScheduledStepResponse>, Status> {
        // Zero retention and waivers are refused before any work: the lane keeps no durable
        // state for an unattended run (WS20 S6, ZDR clause). The caller is the lane's own
        // identity, which carries no retention until the step context does.
        let caller = AuthenticatedUser {
            org_id: req.org_id.clone(),
            user_id: req.subject_id.clone(),
            zdr: false,
            org_zdr: None,
            zdr_waiver: None,
        };
        if let Some(code) = runtime_loop::scheduled_agent::refuses_retention(
            caller.zdr,
            caller.zdr_waiver.is_some(),
        ) {
            return self
                .finish_scheduled_agent_step(
                    req,
                    session_bearer,
                    receipt_id,
                    &AgentStepOutcome::failed(code),
                )
                .await;
        }
        let now = chrono::Utc::now();
        let contract = match runtime_loop::scheduled_agent::admit(
            &run.contract_json,
            &run.contract_digest,
            &run.contract_expires_at,
            now,
        ) {
            Ok(contract) => contract,
            Err(code) => {
                return self
                    .finish_scheduled_agent_step(
                        req,
                        session_bearer,
                        receipt_id,
                        &AgentStepOutcome::failed(code),
                    )
                    .await;
            }
        };
        let tools = runtime_loop::agent::OfferedTools {
            defs: runtime_loop::scheduled_agent::offered_defs(
                &contract,
                runtime_loop::agent::base_tool_defs(!req.space_id.is_empty()),
            ),
            derivable: false,
        };
        let run_req = runtime_loop::scheduled_agent::run_request(req, &run.goal, &contract);
        self.state.begin_unattended(&req.run_id);
        let driven = tokio::time::timeout(
            runtime_loop::scheduled_agent::wall_clock(&contract, now),
            runtime_loop::agent::run_agent_with_offered(
                &self.state,
                self.session_channel.clone(),
                self.inference_channel(),
                &caller,
                run_req,
                tools,
                None,
                Some(session_bearer.to_owned()),
                inference_bearer.to_owned(),
                self.capability_policy.as_ref(),
                self.terminal_tokens.as_ref(),
                None,
                // A scheduled run never carries a browser bearer (WS20).
                None,
                self.capability_client.as_ref(),
                &self.sandbox_manager_client,
                &self.backend_id,
                self.cas_client.as_ref(),
                &self.sandbox_tokens,
                self.process_host.as_deref(),
                // A scheduled run is always Space-scoped (`validate_scheduled_step_bindings` requires
                // its Space) and carries no retrieval decision, so its knowledge search is withheld
                // rather than run organisation-wide (WS18-S5; WS20 owns a decision for these runs).
                crate::knowledge_tools::SpaceRetrieval::Withheld,
                None,
                Some(runtime_loop::scheduled_agent::loop_scope(&contract)),
            ),
        )
        .await;
        let withheld = self.state.take_withheld(&req.run_id);
        let outcome = match driven {
            Ok(Ok(response)) => {
                let usage = response.usage.unwrap_or_default();
                AgentStepOutcome {
                    status: if response.status == "completed" {
                        "completed"
                    } else {
                        "failed"
                    },
                    output: runtime_loop::scheduled_agent::transcript_text(
                        &response.final_output,
                        &withheld,
                    ),
                    error_code: if response.status == "completed" {
                        ""
                    } else {
                        "scheduled_run_incomplete"
                    },
                    unknown: false,
                    rounds_executed: response.rounds_executed,
                    withheld_effects: u32::try_from(withheld.len()).unwrap_or(u32::MAX),
                    input_tokens: u32::try_from(usage.input_tokens).unwrap_or(u32::MAX),
                    output_tokens: u32::try_from(usage.output_tokens).unwrap_or(u32::MAX),
                }
            }
            Ok(Err(error)) => {
                warn!(code = ?error.code(), "scheduled agent run failed");
                let (status, error_code, unknown) = scheduled_step_failure_status(error.code());
                AgentStepOutcome {
                    status,
                    error_code,
                    unknown,
                    ..AgentStepOutcome::failed("")
                }
            }
            Err(_elapsed) => AgentStepOutcome {
                status: "unknown_outcome",
                error_code: "scheduled_run_deadline",
                unknown: true,
                ..AgentStepOutcome::failed("")
            },
        };
        self.finish_scheduled_agent_step(req, session_bearer, receipt_id, &outcome)
            .await
    }

    /// Complete a claimed agent-lane step through Session Core, which appends the transcript and
    /// records the counts. The answer is content and travels only on this direct hop; the
    /// response to the orchestrator carries counts and no output.
    async fn finish_scheduled_agent_step(
        &self,
        req: &pb::ExecuteScheduledStepRequest,
        bearer: &str,
        receipt_id: &str,
        outcome: &AgentStepOutcome,
    ) -> Result<Response<pb::ExecuteScheduledStepResponse>, Status> {
        let receipt = self
            .session_client()
            .record_scheduled_run_result(authenticated_session_request(
                scheduled_agent_result_request(req, receipt_id, outcome),
                bearer,
            )?)
            .await
            .map(|response| response.into_inner())
            .map_err(|error| {
                warn!(code = ?error.code(), "scheduled agent result persistence failed");
                Status::unavailable("scheduled-step result persistence unavailable")
            })?;
        Ok(Response::new(pb::ExecuteScheduledStepResponse {
            step_id: req.step_id.clone(),
            status: receipt.status,
            receipt_id: receipt.receipt_id,
            output: String::new(),
            error: outcome.error_code.to_owned(),
            unknown_outcome: outcome.unknown,
            rounds_executed: outcome.rounds_executed,
            withheld_effects: outcome.withheld_effects,
            tool_calls: 0,
            input_tokens: outcome.input_tokens,
            output_tokens: outcome.output_tokens,
        }))
    }

    async fn record_scheduled_step_receipt(
        &self,
        req: &pb::ExecuteScheduledStepRequest,
        bearer: &str,
        receipt_id: &str,
        status: &str,
        output_digest: &str,
        error_code: &str,
        unknown_outcome: bool,
    ) -> Result<pb::RecordScheduledStepReceiptResponse, Status> {
        self.session_client()
            .record_scheduled_step_receipt(authenticated_session_request(
                pb::RecordScheduledStepReceiptRequest {
                    run_id: req.run_id.clone(),
                    step_id: req.step_id.clone(),
                    org_id: req.org_id.clone(),
                    idempotency_key: req.idempotency_key.clone(),
                    receipt_id: receipt_id.to_owned(),
                    status: status.to_owned(),
                    output_digest: output_digest.to_owned(),
                    error_code: error_code.to_owned(),
                    unknown_outcome,
                },
                bearer,
            )?)
            .await
            .map(|response| response.into_inner())
            .map_err(|error| {
                warn!(code = ?error.code(), "scheduled-step receipt persistence failed");
                Status::unavailable("scheduled-step receipt persistence unavailable")
            })
    }
}

/// Whether a completed scheduled step's answer is persisted through
/// `RecordScheduledRunResult` (build plan WS20 S1). It reads the name Session Core
/// reads, so one deploy setting drives both sides. Only the literal `true` enables.
fn scheduled_result_persist_enabled() -> bool {
    mp_contracts::loop_fold::switch_from(
        std::env::var(SCHEDULED_RESULT_PERSIST_ENV).ok().as_deref(),
    )
}

const SCHEDULED_RESULT_PERSIST_ENV: &str = "SESSION_CORE_SCHEDULED_RESULT_PERSIST";

/// The one completed-step result this lane sends. It carries the run's identity,
/// the receipt it completes and the answer, and no counts: the scheduled Do lane
/// that reports them is a later slice.
fn scheduled_run_result_request(
    req: &pb::ExecuteScheduledStepRequest,
    receipt_id: &str,
    output: &str,
) -> pb::RecordScheduledRunResultRequest {
    pb::RecordScheduledRunResultRequest {
        run_id: req.run_id.clone(),
        step_id: req.step_id.clone(),
        org_id: req.org_id.clone(),
        idempotency_key: req.idempotency_key.clone(),
        receipt_id: receipt_id.to_owned(),
        status: "completed".to_owned(),
        output: output.to_owned(),
        ..Default::default()
    }
}

/// What one agent-lane step reports to Session Core (WS20 S6). The counts carry no content;
/// `output` is the transcript text and travels only on the direct hop.
struct AgentStepOutcome {
    status: &'static str,
    output: String,
    error_code: &'static str,
    unknown: bool,
    rounds_executed: u32,
    withheld_effects: u32,
    input_tokens: u32,
    output_tokens: u32,
}

impl AgentStepOutcome {
    fn failed(error_code: &'static str) -> Self {
        Self {
            status: "failed",
            output: String::new(),
            error_code,
            unknown: false,
            rounds_executed: 0,
            withheld_effects: 0,
            input_tokens: 0,
            output_tokens: 0,
        }
    }
}

/// The Session Core result for an agent-lane step. `tool_calls` is not reported by the
/// driver yet, so it stays zero.
fn scheduled_agent_result_request(
    req: &pb::ExecuteScheduledStepRequest,
    receipt_id: &str,
    outcome: &AgentStepOutcome,
) -> pb::RecordScheduledRunResultRequest {
    pb::RecordScheduledRunResultRequest {
        run_id: req.run_id.clone(),
        step_id: req.step_id.clone(),
        org_id: req.org_id.clone(),
        idempotency_key: req.idempotency_key.clone(),
        receipt_id: receipt_id.to_owned(),
        status: outcome.status.to_owned(),
        output: outcome.output.clone(),
        error_code: outcome.error_code.to_owned(),
        unknown_outcome: outcome.unknown,
        rounds_executed: outcome.rounds_executed,
        withheld_effects: outcome.withheld_effects,
        tool_calls: 0,
        input_tokens: outcome.input_tokens,
        output_tokens: outcome.output_tokens,
    }
}

fn scheduled_inference_request(
    req: &pb::ExecuteScheduledStepRequest,
    goal: &str,
) -> pb::InferRequest {
    pb::InferRequest {
        purpose: pb::InferPurpose::ScheduledRound as i32,
        // Keep provider correlation in the same tenant/run/step namespace as
        // the Session receipt. A bare step id can collide across runs or
        // tenants, which makes an ambiguous provider response impossible to
        // reconcile safely.
        request_id: format!("scheduled:{}:{}", req.org_id, req.idempotency_key),
        org_id: req.org_id.clone(),
        model: "verevon-balance".to_owned(),
        messages: vec![
            pb::ChatMessage {
                compaction_summary: String::new(),
                role: "system".to_owned(),
                content:
                    "Execute one bounded scheduled step. Do not call tools or external effects."
                        .to_owned(),
                name: String::new(),
            },
            pb::ChatMessage {
                compaction_summary: String::new(),
                role: "user".to_owned(),
                content: goal.to_owned(),
                name: String::new(),
            },
        ],
        temperature: 0.2,
        max_tokens: 1024,
        tool_choice: "none".to_owned(),
        ..Default::default()
    }
}

fn scheduled_step_failure_status(code: tonic::Code) -> (&'static str, &'static str, bool) {
    match code {
        tonic::Code::Unavailable | tonic::Code::DeadlineExceeded | tonic::Code::Cancelled => {
            ("unknown_outcome", "inference_outcome_unknown", true)
        }
        _ => ("failed", "inference_failed", false),
    }
}

#[allow(clippy::result_large_err)]
async fn authorize_durable_run_owner(
    state: &StateStore,
    ownership: &dyn RunOwnershipResolver,
    caller: &AuthenticatedUser,
    run_id: &str,
    session_bearer: &DelegatedSessionBearer,
) -> Result<(), Status> {
    if run_id.trim().is_empty() {
        return Err(Status::invalid_argument("run_id is required"));
    }
    if state.authorizes(run_id, &caller.org_id, &caller.user_id) {
        return Ok(());
    }
    if !ownership
        .resolve(
            run_id,
            &caller.org_id,
            &caller.user_id,
            session_bearer.as_str(),
        )
        .await?
    {
        return Err(Status::permission_denied("run access denied"));
    }
    if !state.cache_verified_owner(run_id, &caller.org_id, &caller.user_id) {
        return Err(Status::permission_denied("run access denied"));
    }
    Ok(())
}

#[allow(clippy::result_large_err)]
fn authenticated_session_request<T>(value: T, bearer: &str) -> Result<Request<T>, Status> {
    let mut request = Request::new(value);
    let authorization = format!("Bearer {bearer}")
        .parse()
        .map_err(|_| Status::internal("verified session credential is not forwardable"))?;
    request
        .metadata_mut()
        .insert("authorization", authorization);
    Ok(request)
}

#[allow(clippy::result_large_err)]
fn authenticated_inference_request<T>(value: T, bearer: &str) -> Result<Request<T>, Status> {
    let mut request = Request::new(value);
    let authorization = format!("Bearer {bearer}")
        .parse()
        .map_err(|_| Status::internal("verified inference credential is not forwardable"))?;
    request
        .metadata_mut()
        .insert("authorization", authorization);
    Ok(request)
}

#[allow(clippy::result_large_err)]
fn authenticated_browser_request<T>(value: T, bearer: &str) -> Result<Request<T>, Status> {
    let mut request = Request::new(value);
    let authorization = format!("Bearer {bearer}")
        .parse()
        .map_err(|_| Status::internal("verified browser credential is not forwardable"))?;
    request
        .metadata_mut()
        .insert("authorization", authorization);
    Ok(request)
}

/// Persist an approval before any caller may report a HITL pause as durable.
///
/// # Errors
///
/// Returns the local request-construction error, or `unavailable` when
/// session-core cannot durably create the approval record. Callers must return
/// the error before they checkpoint or expose `AwaitingApproval` state.
#[allow(clippy::result_large_err)]
/// A lazily-connected channel that cannot hang on an unreachable peer.
///
/// `connect_lazy` on a bare `Endpoint` applies NO bound to establishing the
/// connection, so a call against a peer that is down waits on the OS TCP stack
/// — which refuses fast on Linux and retries SYN for seconds on Windows. That
/// difference is why this looked like a flaky test rather than the missing
/// bound it is: `direct_hitl_rejects_an_undurable_approval_pause` asserts a
/// direct human-in-the-loop approval fails FAST when `orchestration-core` is
/// unreachable, because the alternative is a run that neither pauses nor
/// proceeds while a human waits on it. Every other channel in this crate
/// already bounds itself this way (`auth.rs`, `capability_client.rs`,
/// `capability_policy.rs`); these four were the exception.
///
/// Connection setup is bounded for every peer. A per-REQUEST timeout is the
/// caller's business and deliberately not set here: session and capability
/// calls are short, but an inference or browser call can legitimately run for
/// minutes, and a blanket request timeout would abort exactly the work this
/// service exists to do.
fn lazy_bounded_channel(url: String) -> Result<tonic::transport::Channel, tonic::transport::Error> {
    Ok(tonic::transport::Endpoint::from_shared(url)?
        .connect_timeout(GRPC_CONNECT_TIMEOUT)
        .connect_lazy())
}

/// Matches the 3 s used by every other outbound channel in this crate.
const GRPC_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

async fn create_durable_approval(
    session_channel: &tonic::transport::Channel,
    approval: pb::CreateApprovalRequest,
    bearer: &str,
) -> Result<(), Status> {
    let request = authenticated_session_request(approval, bearer)?;
    OrchestrationCoreServiceClient::new(session_channel.clone())
        .create_approval(request)
        .await
        .map(|_| ())
        .map_err(|error| {
            warn!(code = ?error.code(), "durable approval persistence unavailable");
            Status::unavailable("approval persistence unavailable")
        })
}

/// Refuses a durable execution path (a tool, an agent run, a run control) for a
/// caller whose token says ZDR, or whose request does. `feature` is the catalogue
/// key of what is refused, so the status names it.
#[allow(clippy::result_large_err)]
/// The run context is judged at the run_agent boundary: a context that breaks
/// its bounds, or that carries conversation content on a zero-retention run, is
/// refused with the closed token only (build plan WS02 S4).
fn enforce_run_context(req: &pb::RunAgentRequest) -> Result<(), Status> {
    mp_contracts::run_context::validate(req.run_context.as_ref(), req.zdr)
        .map_err(|refusal| Status::invalid_argument(refusal.token()))
}

/// A continuation (`RunAgentRequest.resume`, build plan WS03 S3) is judged at the
/// boundary: refused while the durable-loop switch is off, and refused when it
/// does not name the exact approved call. The resumed segment authenticates with
/// the request's own bearers; nothing here widens authority.
///
/// A park resume (WS03 S8-6, mode `park`) names no approval: it carries a claim token and
/// nothing an approval stage reads (no approval, step, tool, fingerprint or brief), and a
/// park that names one is incomplete. Neither the client's segment number nor the parking
/// switch is read here: with parking off an existing park stays resumable.
fn enforce_resume(req: &pb::RunAgentRequest, enabled: bool) -> Result<(), Status> {
    let Some(resume) = req.resume.as_ref() else {
        return Ok(());
    };
    if !enabled {
        return Err(Status::failed_precondition("continue_disabled"));
    }
    if resume_names_question(resume) {
        return enforce_question_resume(resume, runtime_loop::resume::ask_resume_enabled());
    }
    if !matches!(resume.mode.as_str(), "segment" | "loop" | "park") {
        return Err(Status::invalid_argument("resume_incomplete"));
    }
    if resume.mode == "park" {
        let names_an_approval = !resume.approval_id.is_empty()
            || !resume.step_id.is_empty()
            || !resume.tool_name.is_empty()
            || !resume.action_fingerprint.is_empty()
            || !resume.brief.is_empty();
        if names_an_approval || resume.claim_token.is_empty() {
            return Err(Status::invalid_argument("resume_incomplete"));
        }
        return Ok(());
    }
    if resume.approval_id.is_empty() || resume.claim_token.is_empty() {
        return Err(Status::invalid_argument("resume_incomplete"));
    }
    Ok(())
}

/// Whether a continuation names a run question rather than an approval (build plan WS14 S5). Such a
/// continuation carries a question id or an answer, and nothing an approval stage reads.
fn resume_names_question(resume: &pb::AgentResume) -> bool {
    !resume.question_id.is_empty() || !resume.answer_json.is_empty()
}

/// A question continuation (build plan WS14 S5), judged after the continue switch: refused while
/// `AGENT_ASK_USER_RESUME_ENABLED` and its prerequisites are not all on, and refused unless it names
/// one question with its answer and a claim. It never names an approval, a step, a tool or a brief.
fn enforce_question_resume(resume: &pb::AgentResume, question_resume_on: bool) -> Result<(), Status> {
    if !question_resume_on {
        return Err(Status::failed_precondition("continue_disabled"));
    }
    let names_an_approval = !resume.approval_id.is_empty()
        || !resume.step_id.is_empty()
        || !resume.tool_name.is_empty()
        || !resume.action_fingerprint.is_empty()
        || !resume.brief.is_empty();
    if names_an_approval
        || !matches!(resume.mode.as_str(), "segment" | "loop")
        || resume.question_id.is_empty()
        || resume.answer_json.is_empty()
        || resume.claim_token.is_empty()
    {
        return Err(Status::invalid_argument("resume_incomplete"));
    }
    Ok(())
}

/// Judges the attachments of an `ExecuteStep` request and holds them for the
/// call. `None` when the request carries none, which is every request today.
///
/// Only `code_interpreter` reads them, so a list on any other tool is refused
/// instead of being held for nothing. A zero-retention or waived caller never
/// carries one.
#[allow(clippy::result_large_err)]
fn register_step_attachments(
    req: &pb::ExecuteStepRequest,
    zdr_or_waived: bool,
) -> Result<Option<crate::run_inputs::RunInputsGuard>, Status> {
    if req.attachments.is_empty() {
        return Ok(None);
    }
    if req.tool_name != "code_interpreter" {
        return Err(Status::invalid_argument("attachments_unsupported_tool"));
    }
    mp_contracts::attachments::validate(&req.attachments, zdr_or_waived)
        .map_err(attachment_refusal_status)?;
    crate::run_inputs::global()
        .register(&req.org_id, &req.run_id, &req.attachments)
        .map_err(|refusal| {
            Status::resource_exhausted(match refusal {
                crate::run_inputs::RegisterError::EntryCap => "run_inputs_entry_cap",
                crate::run_inputs::RegisterError::ByteCap => "run_inputs_byte_cap",
            })
        })
}

/// The status a refused attachment list answers with: the zero-retention catalogue
/// refusal for a ZDR or waived caller, the closed token for everything else.
fn attachment_refusal_status(refusal: mp_contracts::attachments::AttachmentRefusal) -> Status {
    if refusal == mp_contracts::attachments::AttachmentRefusal::Zdr {
        mp_contracts::zdr_refusal::Refusal::feature_not_supported("attachments")
            .grpc_status(tonic::Code::FailedPrecondition)
    } else {
        Status::invalid_argument(refusal.token())
    }
}

/// Most images one run may carry: what inference-core accepts on a request.
const MAX_RUN_IMAGES: usize = 4;
/// The image types inference-core allows as parts of a message.
const RUN_IMAGE_MIMES: [&str; 4] = ["image/png", "image/jpeg", "image/webp", "image/gif"];

/// Judges the attachments of a `RunAgent` request at the boundary (WS08-S6). Files
/// meet the contract the `ExecuteStep` path applies; images, which the sandbox
/// never receives, meet the bounds inference-core will apply to them. A zero-retention
/// or waived caller never carries either. The list is partitioned by moving the
/// entries, so no attachment byte is copied, and put back with files first.
#[allow(clippy::result_large_err)]
fn enforce_run_attachments(
    req: &mut pb::RunAgentRequest,
    zdr_or_waived: bool,
) -> Result<(), Status> {
    if req.attachments.is_empty() {
        return Ok(());
    }
    let (images, files): (Vec<pb::RunAttachment>, Vec<pb::RunAttachment>) =
        std::mem::take(&mut req.attachments)
            .into_iter()
            .partition(|attachment| attachment.kind == pb::RunAttachmentKind::Image as i32);
    let judged = judge_run_attachments(&files, &images, zdr_or_waived);
    req.attachments = files;
    req.attachments.extend(images);
    judged
}

#[allow(clippy::result_large_err)]
fn judge_run_attachments(
    files: &[pb::RunAttachment],
    images: &[pb::RunAttachment],
    zdr_or_waived: bool,
) -> Result<(), Status> {
    use mp_contracts::attachments::{
        sha256_hex, validate, AttachmentRefusal as Refusal, MAX_ATTACHMENT_BYTES, MAX_TOTAL_BYTES,
    };
    validate(files, zdr_or_waived).map_err(attachment_refusal_status)?;
    if images.is_empty() {
        return Ok(());
    }
    if zdr_or_waived {
        return Err(attachment_refusal_status(Refusal::Zdr));
    }
    if images.len() > MAX_RUN_IMAGES {
        return Err(attachment_refusal_status(Refusal::TooMany));
    }
    let mut total: usize = files.iter().map(|file| file.content.len()).sum();
    for image in images {
        if !RUN_IMAGE_MIMES.contains(&image.mime.as_str()) {
            return Err(attachment_refusal_status(Refusal::KindUnsupported));
        }
        let size = image.content.len();
        if size == 0 {
            return Err(attachment_refusal_status(Refusal::Empty));
        }
        if size > MAX_ATTACHMENT_BYTES {
            return Err(attachment_refusal_status(Refusal::TooLarge));
        }
        if image.size_bytes != size as u64 {
            return Err(attachment_refusal_status(Refusal::SizeMismatch));
        }
        if image.sha256 != sha256_hex(&image.content) {
            return Err(attachment_refusal_status(Refusal::HashMismatch));
        }
        total = total.saturating_add(size);
    }
    if total > MAX_TOTAL_BYTES {
        return Err(attachment_refusal_status(Refusal::TotalTooLarge));
    }
    Ok(())
}

/// E3.3-f: queue the steer text a pause or resume carries, before the run's state moves. Empty
/// text is nothing to do. While `QUARRY_BROWSER_AGENT_STEER` is off the text is refused, not
/// dropped: the person must not watch their words vanish. The ZDR refusal is the caller's
/// (`enforce_persistence_free_execution` already ran for every run control).
#[allow(clippy::result_large_err)]
fn queue_steer_text(state: &StateStore, run_id: &str, text: &str) -> Result<(), Status> {
    if text.trim().is_empty() {
        return Ok(());
    }
    if !crate::browser_agent::steer_enabled() {
        return Err(Status::failed_precondition("browser_steer_disabled"));
    }
    match state.enqueue_steer(run_id, text) {
        crate::state::SteerOutcome::Queued { .. } => Ok(()),
        crate::state::SteerOutcome::Empty => Ok(()),
        crate::state::SteerOutcome::TooLong { .. } => {
            Err(Status::invalid_argument("steer_text_too_long"))
        }
        crate::state::SteerOutcome::Full { .. } => {
            Err(Status::resource_exhausted("steer_queue_full"))
        }
    }
}

fn enforce_persistence_free_execution(
    caller: &AuthenticatedUser,
    request_zdr: bool,
    feature: &'static str,
) -> Result<(), Status> {
    if caller.zdr || request_zdr {
        return Err(
            mp_contracts::zdr_refusal::Refusal::feature_not_supported(feature)
                .grpc_status(tonic::Code::FailedPrecondition),
        );
    }
    Ok(())
}

/// The thread a step's `code_interpreter` workspace is kept in, when the step takes the thread path
/// (WS22-S2). Only such a step demands the user-bound sandbox bearer, so with the switch off no step
/// does, and each request is handled as it was before the switch existed.
fn thread_workspace_thread<'a>(
    switched_on: bool,
    tool_name: &str,
    space_id: &str,
    thread_id: &'a str,
    persistence_free: bool,
) -> Option<&'a str> {
    if tool_name != "code_interpreter" {
        return None;
    }
    crate::sandbox_lease::workspace_sync::thread_for_step(
        switched_on,
        persistence_free,
        space_id,
        thread_id,
    )
}

#[allow(clippy::result_large_err)]
fn validate_granted_approval(
    approval: Option<&pb::Approval>,
    approval_id: &str,
    run_id: &str,
    org_id: &str,
) -> Result<(), Status> {
    let Some(approval) = approval else {
        return Err(Status::failed_precondition(
            "durable granted approval is required",
        ));
    };
    if approval.id != approval_id || approval.run_id != run_id || approval.org_id != org_id {
        return Err(Status::permission_denied("approval scope mismatch"));
    }
    if approval.state != pb::ApprovalState::Granted as i32 {
        return Err(Status::failed_precondition(
            "approval is not durably granted",
        ));
    }
    Ok(())
}

/// Approval delivery must restart the suspended work, not merely change the
/// in-memory status of a task that has already exited. The durable outbox is
/// the source of truth until execution-core has a restartable continuation
/// contract and a worker that can acknowledge that delivery.
#[allow(clippy::result_large_err)]
fn resume_durable_approval_continuation(
    _state: &StateStore,
    _run_id: &str,
) -> Result<Option<u32>, Status> {
    Err(Status::unavailable(
        "durable approval is queued; execution continuation delivery is not available",
    ))
}

/// `ExecuteStep` is an authenticated integration primitive, not an alternate
/// owner-effect entrypoint. Reserved actions enter only through the agent loop
/// after a run-bound server-resolved catalog decision, and a future delivery
/// worker will need a distinct, durable continuation capability to execute an
/// approved descriptor. Keeping them out of this RPC prevents a caller from
/// naming a tool directly to bypass either control. A tool the run declares
/// `durable_owner` is held the same way under `TOOL_CLASS_GATE`.
fn reject_direct_owner_action(
    gate: crate::permission::RunGate<'_>,
    tool_name: &str,
) -> Result<(), Status> {
    // OR-ed with the name, never substituted for it: this site only refuses, so a
    // declared view may add the marker but cannot lift one the name reserves. The view
    // is read by the trimmed name, as the name predicate trims at every ingress.
    if crate::permission::requires_durable_owner_approval(tool_name)
        || gate.requires_durable_owner_approval(tool_name.trim())
    {
        return Err(Status::permission_denied(
            "reserved owner actions cannot be invoked through ExecuteStep",
        ));
    }
    Ok(())
}

/// The internal one-step RPC has no safe default permission posture. Agentic
/// requests normalize their own mode to `ask`, but a direct caller must be
/// explicit so an absent or malformed field cannot silently become `auto`.
fn validate_direct_permission_mode(permission_mode: &str) -> Result<(), Status> {
    match permission_mode {
        "auto" | "ask" | "deny" => Ok(()),
        _ => Err(Status::invalid_argument(
            "permission_mode must be one of auto, ask, or deny",
        )),
    }
}

/// Judge the inference route a `RunAgent` request names before any of the run's
/// work starts.
///
/// A request that names none (the empty `provider_hint` and
/// `subscription_connection_id` of every run that predates the fields) passes
/// untouched. One that names the ChatGPT subscription route passes only when
/// `crate::agent_route::validate_run_route` says so (kill switch on, canonical
/// provider, a connection, a concrete model, read-only, and a retention the
/// caller's claims allow: a waiver for this connection, or an ordinary turn).
///
/// The refusals are `PermissionDenied` or `InvalidArgument`, which model-gateway
/// terminalizes as a confirmed dispatch rejection: nothing ran, so the prepared
/// run is closed rather than left to be retried into the same refusal. They are
/// refusals, never a downgrade to the default route: ignoring the route would
/// run the loop on a paid provider the user did not choose.
///
/// `switches` is a parameter so the decision is testable without touching the
/// process environment; `run_agent` passes the real ones.
#[allow(clippy::result_large_err)]
fn enforce_agent_route(
    req: &pb::RunAgentRequest,
    switches: mp_contracts::agentic_subscription::Switches,
    caller: &AuthenticatedUser,
) -> Result<(), Status> {
    crate::agent_route::validate_run_route(req, switches, caller).inspect_err(|status| {
        warn!(
            run_id = %req.run_id,
            code = ?status.code(),
            "run_agent: refused the inference route the request names"
        );
    })
}

/// Validate the public shape of the scheduled-step lane before it is wired to
/// any authority or execution adapter. This keeps malformed Temporal activity
/// input from becoming an implicit wildcard when the lane is enabled later.
#[allow(clippy::result_large_err)]
fn validate_scheduled_step_bindings(req: &pb::ExecuteScheduledStepRequest) -> Result<(), Status> {
    fn identifier(value: &str) -> bool {
        !value.is_empty()
            && value.len() <= 256
            && value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b'.')
            })
    }
    fn digest(value: &str) -> bool {
        value.len() == 71
            && value.starts_with("sha256:")
            && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
    }

    if !identifier(&req.run_id)
        || !identifier(&req.thread_id)
        || !identifier(&req.org_id)
        || !identifier(&req.space_id)
        || !identifier(&req.subject_id)
        || !identifier(&req.schedule_id)
        || !identifier(&req.fire_key)
        || !identifier(&req.step_id)
        || !identifier(&req.idempotency_key)
        || !digest(&req.template_digest)
        || !digest(&req.policy_digest)
        || req.control_decision_token.trim().is_empty()
        || req.control_decision_token.len() > 16_384
    {
        return Err(Status::invalid_argument(
            "scheduled-step bindings are invalid",
        ));
    }
    Ok(())
}

const SCHEDULED_STEP_SERVICE_ID: &str = "service:orchestrator-core";
const SCHEDULED_STEP_SCOPE: &str = "model:schedule:step";

#[allow(clippy::result_large_err)]
fn authorize_scheduled_step_service(
    caller: &AuthenticatedService,
    org_id: &str,
) -> Result<(), Status> {
    if caller.service_id != SCHEDULED_STEP_SERVICE_ID {
        return Err(Status::permission_denied(
            "scheduled-step service principal is not authorized",
        ));
    }
    caller.authorize_org(org_id)?;
    if !caller.has_scope(SCHEDULED_STEP_SCOPE) {
        return Err(Status::permission_denied(
            "scheduled-step service scope is required",
        ));
    }
    if caller.zdr {
        return Err(
            mp_contracts::zdr_refusal::Refusal::feature_not_supported("scheduled_step")
                .grpc_status(tonic::Code::FailedPrecondition),
        );
    }
    Ok(())
}

fn validate_restricted_browser_step(req: &pb::ExecuteStepRequest) -> Result<(), Status> {
    if req.tool_name != "browser_agent" {
        return Err(Status::invalid_argument("restricted browser dispatch requires browser_agent"));
    }
    let input: serde_json::Value = serde_json::from_str(&req.tool_input)
        .map_err(|_| Status::invalid_argument("invalid restricted browser input"))?;
    let has_attach = input.get("attach_session_id").and_then(serde_json::Value::as_str)
        .is_some_and(|id| !id.trim().is_empty());
    let ask: crate::quarry_agent::WireRequestAsk = input.get("policy").cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .ok_or_else(|| Status::invalid_argument("restricted attachment requires a policy"))?;
    if !has_attach || ask.is_empty() {
        return Err(Status::invalid_argument("restricted attachment requires a non-empty policy"));
    }
    if !["browser.act.upload", "browser.act.download"].iter().all(|required| ask.disable.iter().any(|capability| capability == required)) {
        return Err(Status::invalid_argument("restricted attachment must disable file transfers"));
    }
    Ok(())
}

#[cfg(test)]
mod restricted_browser_dispatch_tests {
    use super::*;
    #[test]
    fn restricted_rpc_refuses_missing_policy_or_attachment() {
        let mut req = pb::ExecuteStepRequest { tool_name: "browser_agent".into(), ..Default::default() };
        for input in ["{}", r#"{"attach_session_id":"s","policy":{}}"#, r#"{"policy":{"disable":["browser.act.input"]}}"#] {
            req.tool_input = input.into();
            assert!(validate_restricted_browser_step(&req).is_err());
        }
        req.tool_input = r#"{"attach_session_id":"s","policy":{"disable":["browser.act.input"]}}"#.into();
        assert!(validate_restricted_browser_step(&req).is_err());
        req.tool_input = r#"{"attach_session_id":"s","policy":{"disable":["browser.act.upload","browser.act.download"]}}"#.into();
        assert!(validate_restricted_browser_step(&req).is_ok());
        req.tool_name = "echo".into();
        assert!(validate_restricted_browser_step(&req).is_err());
    }
}

#[tonic::async_trait]
impl ExecutionCore for ExecutionService {
    async fn browser_agent_capabilities(
        &self,
        request: Request<pb::BrowserAgentCapabilitiesRequest>,
    ) -> Result<Response<pb::BrowserAgentCapabilitiesResponse>, Status> {
        let caller = self.auth.authenticate(&request).await?;
        caller.authorize(&request.get_ref().org_id, None)?;
        Ok(Response::new(pb::BrowserAgentCapabilitiesResponse { attached_policy_protocol: 1 }))
    }

    async fn execute_restricted_browser_step(
        &self,
        request: Request<pb::ExecuteStepRequest>,
    ) -> Result<Response<pb::ExecuteStepResponse>, Status> {
        validate_restricted_browser_step(request.get_ref())?;
        // The existing method authenticates all delegated credentials and
        // run ownership before validating the broker grant or starting Quarry.
        self.execute_step(request).await
    }
    // single-step RPC: gate → execute → HITL → persist is one linear flow
    #[allow(clippy::too_many_lines)]
    async fn execute_step(
        &self,
        request: Request<pb::ExecuteStepRequest>,
    ) -> Result<Response<pb::ExecuteStepResponse>, Status> {
        let session_bearer = delegated_session_bearer(&request)?;
        let caller = self.auth.authenticate(&request).await?;
        let data_plane_bearer = self
            .auth
            .authenticate_delegated_data_plane(&request, &caller)
            .await?;
        let inference_bearer = self
            .auth
            .authenticate_delegated_inference(&request, &caller)
            .await?;
        // Every route that can reach Quarry's stateful browser surface needs a
        // separately verified BrowserBroker audience credential. The MCP
        // aliases are not read-only HTTP tools: `browser.observe` exposes a
        // live leased context and `browser.act` can cause external effects.
        let browser_bearer = if matches!(
            request.get_ref().tool_name.as_str(),
            "browser_agent" | "browser.observe" | "browser.act" | "browser.console"
        ) {
            Some(
                self.auth
                    .authenticate_delegated_browser(&request, &caller)
                    .await?,
            )
        } else {
            None
        };
        // Demanded only for a Space-scoped code_interpreter, shell (WS06-S9) or workspace read (WS24-S3) step —
        // every other step, and a non-Space one, is unaffected.
        // sandbox-manager's own AcquireLease binds a Space capability
        // decision to the CALLING principal, so a service-level credential
        // could never pass it; this is the one delegated bearer that must be
        // user-bound (S3_3_DURABLE_WORKSPACE_DESIGN_2026-09-11.md §3.5 B.2).
        // A thread-scope code_interpreter (WS22-S2) demands the same user-bound bearer, for its
        // thread's own lease. Only with the switch on, and only for a step that is neither
        // Space-scoped nor persistence-free, does that path apply.
        let thread_scope_id = thread_workspace_thread(
            crate::sandbox_lease::workspace_sync::switched_on(),
            &request.get_ref().tool_name,
            &request.get_ref().space_id,
            &request.get_ref().thread_id,
            request.get_ref().zdr || caller.zdr || caller.zdr_waiver.is_some(),
        )
        .map(str::to_owned);
        let sandbox_bearer = if ((request.get_ref().tool_name == "code_interpreter"
            || request.get_ref().tool_name == "shell"
            || crate::workspace_tools::is_workspace_tool(&request.get_ref().tool_name))
            && !request.get_ref().space_id.is_empty())
            || thread_scope_id.is_some()
        {
            Some(
                self.auth
                    .authenticate_delegated_sandbox_manager(&request, &caller)
                    .await?,
            )
        } else {
            None
        };
        let req = request.into_inner();
        caller.authorize(&req.org_id, Some(&req.user_id))?;
        self.authorize_run(&caller, &req.run_id, &session_bearer)
            .await?;
        reject_direct_owner_action(
            crate::permission::RunGate::for_run(&req.org_id, &req.run_id),
            &req.tool_name,
        )?;
        validate_direct_permission_mode(&req.permission_mode)?;
        enforce_persistence_free_execution(&caller, req.zdr, "tool_execution")?;
        // The attached files are judged after the caller and the run owner are
        // authorized and held only for this call: the guard drops, and zeroes the
        // bytes, when the handler returns on any path.
        let _run_inputs = register_step_attachments(
            &req,
            req.zdr || caller.zdr || caller.zdr_waiver.is_some(),
        )?;
        // Holds the run's skill tree for the length of this call, so the code_interpreter arm
        // can bind it. A step that names no mounts, as every step does while
        // SKILL_BUNDLE_MOUNT_ENABLED is off, reads nothing and holds nothing.
        let _skill_mounts = crate::skill_mounts::stage_for_step(
            &req.org_id,
            &req.run_id,
            &req.tool_name,
            &req.skill_mounts,
            req.zdr || caller.zdr || caller.zdr_waiver.is_some(),
            self.skill_bundles
                .as_deref()
                .map(|source| source as &dyn crate::skill_mounts::SkillBundleSource),
        )
        .await?;
        // Resolve the opaque JSON grant id with BrowserBroker only after the
        // signed caller and durable run owner are authorized. The resulting
        // immutable policy is passed to the runtime; raw tool JSON cannot
        // select a different grant or expand its allowed domains.
        let browser_grant = match browser_bearer.as_ref() {
            Some(browser_bearer) => Some(
                self.validate_browser_grant(&req.tool_input, browser_bearer)
                    .await?,
            ),
            None => None,
        };
        let run_id = req.run_id.clone();
        let step_id = req.step_id.clone();

        let prior = self.state.get_or_create(&run_id);

        // Surface browser-agent progress (B4) on the run-event stream. The sink
        // publishes BrowserActionDispatched/BrowserObservationReceived events to
        // session-core's orchestration broadcast (shared channel). Best-effort:
        // failures never fail the step.
        let browser_sink = crate::browser_events::OrchestrationEventSink::new(
            self.session_channel.clone(),
            Some(self.state.clone()),
        )
        .with_verified_bearer(session_bearer.as_str())
        .with_receipt_persistence_confirmed(
            !req.zdr && !caller.zdr && caller.org_zdr == Some(false) && caller.zdr_waiver.is_none(),
        );

        let sandbox = sandbox_bearer
            .as_ref()
            .filter(|_| !req.space_id.is_empty())
            .map(|sandbox_bearer| {
            crate::sandbox_lease::SandboxLeaseContext {
                space_id: &req.space_id,
                sandbox_bearer: sandbox_bearer.as_str(),
                capability_client: self.capability_client.as_ref(),
                sandbox_manager_client: &self.sandbox_manager_client,
                backend_id: &self.backend_id,
                cas_client: self.cas_client.as_ref(),
                sandbox_tokens: &self.sandbox_tokens,
                process_host: self.process_host.as_deref(),
            }
        });

        // The thread a code_interpreter call keeps its files in (WS22-S2). Present only for a step that
        // took the thread path: a Space step never does, so `sandbox` stays its only context.
        let thread = match (thread_scope_id.as_deref(), sandbox_bearer.as_ref()) {
            (Some(thread_id), Some(bearer)) if req.space_id.is_empty() => {
                Some(crate::sandbox_lease::workspace_sync::ThreadScope {
                    thread_id,
                    bearer: bearer.as_str(),
                    backend_id: &self.backend_id,
                    sandbox_manager_client: &self.sandbox_manager_client,
                    cas_client: self.cas_client.as_ref(),
                })
            }
            _ => None,
        };
        let outcome = runtime_loop::execute_step_with_browser_grant(
            &req.tool_name,
            &req.tool_input,
            &req.permission_mode,
            &req.hook_context,
            &req.org_id,
            // Viewer scope: ExecuteStepRequest now carries user_id (proto field
            // 8), so a knowledge-search (or any viewer-scoped tool) run through
            // this primitive filters to the caller's visible set, not the whole
            // org. Empty = org-scoped (legacy/unauthenticated caller).
            &req.user_id,
            // run_id + step_id let a provider write bind its durable approval
            // record; the shared session channel reaches the approval store.
            &req.run_id,
            &req.step_id,
            // The direct single-step RPC carries no thread context — memory
            // tools fail closed here, same authority class as subagents.
            "",
            Some(self.session_channel.clone()),
            Some(&browser_sink),
            Some(&self.state),
            req.zdr,
            // The caller's own waiver (Z-18): a waived caller's retrieval is
            // filtered, whichever step it runs. A step names no model route, so
            // nothing else is judged here.
            caller.zdr_waiver.is_some(),
            req.min_privacy_tier,
            Some(data_plane_bearer.as_str()),
            Some(session_bearer.as_str()),
            Some(inference_bearer.as_str()),
            self.capability_policy.as_ref(),
            browser_grant.as_ref(),
            sandbox.as_ref(),
            thread.as_ref(),
        )
        .await;

        // HITL enforcement: a pause is durable only after session-core accepts
        // the Approval. On persistence failure, fail the RPC before checkpoint
        // or StateStore writes can expose a fictional AwaitingApproval state.
        if outcome.status == "awaiting_approval" {
            create_durable_approval(
                &self.session_channel,
                pb::CreateApprovalRequest {
                    continuation_mode: String::new(),
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
                    continuation_descriptor_json: String::new(),
                    tool_key: String::new(),
                    action_digest: String::new(),
                    turn_origin: String::new(),
                    grant_use_token: String::new(),
                    context_json: crate::space_gate::card_context_json(
                        &req.tool_name,
                        &req.tool_input,
                        crate::space_gate::RESUME_NONE,
                    ),
                    cause: Some(approval_cause::for_call(
                        outcome.cause.unwrap_or(CauseCode::UnclassifiedGate),
                        &run_id,
                        &req.tool_name,
                        &req.tool_input,
                    )),
                    ..Default::default()
                },
                session_bearer.as_str(),
            )
            .await?;
        }

        let status = match outcome.status.as_str() {
            "completed" => RunStatus::Completed,
            "awaiting_approval" => RunStatus::AwaitingApproval,
            // Browser-agent cancellations/aborts are terminal non-successful
            // states, not generic failures to be reinterpreted as completed.
            // Keep their exact string in the response/checkpoint step while
            // projecting the in-memory run lifecycle as cancelled.
            "cancelled" | "aborted" => RunStatus::Cancelled,
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
            .save_checkpoint(authenticated_session_request(
                pb::SaveCheckpointRequest {
                    run_id: run_id.clone(),
                    checkpoint_id: new_ulid(),
                    state: checkpoint_bytes,
                },
                session_bearer.as_str(),
            )?)
            .await
            .map_err(|error| {
                Status::internal(format!("session-core save_checkpoint failed: {error}"))
            })?;

        session_client
            .complete_step(authenticated_session_request(
                pb::CompleteStepRequest {
                    run_id: run_id.clone(),
                    step_id: step_id.clone(),
                    status: outcome.status.clone(),
                    output: scrubbed_output.clone(),
                    error: scrubbed_error.clone(),
                    terminal: false,
                },
                session_bearer.as_str(),
            )?)
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
        let session_bearer = delegated_session_bearer(&request)?;
        let caller = self.auth.authenticate(&request).await?;
        // A resumed retained run can emit session events and perform external
        // browser actions. Do not rely on Model Gateway alone: Execution Core
        // is the authoritative execution-dispatch boundary.
        enforce_persistence_free_execution(&caller, false, "run_control")?;
        let req = request.into_inner();
        caller.authorize(&req.org_id, None)?;
        self.authorize_run(&caller, &req.run_id, &session_bearer)
            .await?;
        queue_steer_text(&self.state, &req.run_id, &req.steer_text)?;
        let resumed_step = if req.approval_id.trim().is_empty() {
            self.state.resume_paused(&req.run_id)
        } else {
            let approval = self
                .orchestration_client()
                .get_approval(authenticated_session_request(
                    pb::GetApprovalRequest {
                        approval_id: req.approval_id.clone(),
                        org_id: req.org_id.clone(),
                        ..Default::default()
                    },
                    session_bearer.as_str(),
                )?)
                .await
                .map_err(|error| {
                    warn!(code = ?error.code(), "durable approval verification unavailable");
                    Status::unavailable("durable approval verification unavailable")
                })?
                .into_inner()
                .approval;
            validate_granted_approval(
                approval.as_ref(),
                &req.approval_id,
                &req.run_id,
                &req.org_id,
            )?;
            resume_durable_approval_continuation(&self.state, &req.run_id)?
        };
        let resumed = resumed_step.is_some();
        let snapshot = self.state.snapshot(&req.run_id);
        let step_index = resumed_step
            .or_else(|| snapshot.as_ref().map(|current| current.step_index))
            .unwrap_or_default();
        if resumed {
            info!(
                run_id = %req.run_id,
                step_index,
                "resume_run: gated run atomically set to Running"
            );
        } else {
            warn!(
                run_id = %req.run_id,
                status = snapshot.as_ref().map_or("unknown", |current| current.status.as_str()),
                "resume_run rejected: transition does not match durable authority"
            );
        }

        Ok(Response::new(pb::ResumeRunResponse {
            resumed,
            step_index,
        }))
    }

    async fn cancel_run(
        &self,
        request: Request<pb::CancelRunRequest>,
    ) -> Result<Response<pb::CancelRunResponse>, Status> {
        let session_bearer = delegated_session_bearer(&request)?;
        let caller = self.auth.authenticate(&request).await?;
        enforce_persistence_free_execution(&caller, false, "run_control")?;
        let req = request.into_inner();
        self.authorize_run(&caller, &req.run_id, &session_bearer)
            .await?;
        let cancelled = self.state.cancel(&req.run_id, Some(req.reason));

        // Best-effort: a cancelled run whose lease fails to release is not a
        // failed cancellation — sandbox-manager's own TTL is the backstop.
        // No-op when this run never acquired a lease at all.
        crate::sandbox_lease::release_sandbox_lease_if_any(
            &self.state,
            &self.sandbox_manager_client,
            &self.sandbox_tokens,
            self.cas_client.as_ref(),
            self.process_host.as_deref(),
            &req.run_id,
            &caller.org_id,
        )
        .await;

        // Execution Core owns only the in-memory cancellation latch. Session
        // Core is the receipt authority, so this service deliberately returns
        // an empty receipt id; model-gateway calls the durable RunService after
        // authorization rather than treating this latch as audit evidence.
        Ok(Response::new(pb::CancelRunResponse {
            cancelled,
            receipt_id: String::new(),
        }))
    }

    /// Pause an active run (Phase 2 B5). Mirrors `resume_run`/`cancel_run`:
    /// flips the in-memory `StateStore` entry so an in-flight loop's own
    /// gate (e.g. `browser_agent::decide_next_action`) observes it between
    /// steps and blocks there — never mid-step.
    async fn pause_run(
        &self,
        request: Request<pb::PauseRunRequest>,
    ) -> Result<Response<pb::PauseRunResponse>, Status> {
        let session_bearer = delegated_session_bearer(&request)?;
        let caller = self.auth.authenticate(&request).await?;
        enforce_persistence_free_execution(&caller, false, "run_control")?;
        let req = request.into_inner();
        caller.authorize(&req.org_id, None)?;
        self.authorize_run(&caller, &req.run_id, &session_bearer)
            .await?;
        queue_steer_text(&self.state, &req.run_id, &req.steer_text)?;
        let paused = self.state.pause(&req.run_id);
        info!(run_id = %req.run_id, "pause_run: run state set to Paused");

        Ok(Response::new(pb::PauseRunResponse { paused }))
    }

    /// Drive a whole agent run to a terminal answer through a governed
    /// multi-tool loop.
    ///
    /// Delegates to [`runtime_loop::agent::run_agent`], which transitions the
    /// run's draft plan to executing, loops `Infer` → dispatch tool calls →
    /// feed outcomes back (the SAME `execute_step_inner` core `ExecuteStep`
    /// itself dispatches through) up to its round budget, persists the
    /// assistant answer when retention permits, and records one immutable
    /// managed terminal receipt. A run is never left `'queued'`: failures take
    /// the graceful path and produce a durable `"failed"` outcome, while
    /// receipt failures remain explicitly unavailable rather than claiming
    /// terminal completion.
    async fn run_agent(
        &self,
        request: Request<pb::RunAgentRequest>,
    ) -> Result<Response<pb::RunAgentResponse>, Status> {
        let session_bearer = delegated_session_bearer(&request)?;
        let caller = self.auth.authenticate(&request).await?;
        let data_plane_bearer = self
            .auth
            .authenticate_delegated_data_plane(&request, &caller)
            .await?;
        let inference_bearer = self
            .auth
            .authenticate_delegated_inference(&request, &caller)
            .await?;
        // Demanded whenever the run is Space-scoped, regardless of which
        // tools it ends up calling — unlike ExecuteStep's per-call gate, a
        // governed run decides tool-by-tool only once the loop is already
        // running, so there is no earlier point to conditionally demand this
        // on a per-tool basis. Mirrors the mandatory data-plane/inference
        // bearers above, which every ExecuteStep call already requires
        // unconditionally for the identical reason.
        // A run whose thread keeps a workspace (WS22-S2) demands the same user-bound bearer, which its
        // code_interpreter calls use for the thread's own lease. With the switch off, or for a run that
        // is persistence-free, nothing changes.
        let thread_workspace = crate::sandbox_lease::workspace_sync::thread_for_step(
            crate::sandbox_lease::workspace_sync::switched_on(),
            request.get_ref().zdr || caller.zdr || caller.zdr_waiver.is_some(),
            &request.get_ref().space_id,
            &request.get_ref().thread_id,
        )
        .is_some();
        let sandbox_bearer = if request.get_ref().space_id.is_empty() && !thread_workspace {
            None
        } else {
            Some(
                self.auth
                    .authenticate_delegated_sandbox_manager(&request, &caller)
                    .await?,
            )
        };
        // The browser bearer names the user's broker grants, so it is read only while the Do
        // loop's browser tools are switched on (WS21-S2). With the switch off the header is never
        // verified and the run is the one it was before.
        let browser_bearer = if runtime_loop::agent::do_browser_enabled() {
            self.auth.authenticate_run_browser(&request, &caller).await?
        } else {
            None
        };
        // The Space retrieval the gateway authorised for this run's knowledge search (WS18-S5). It is
        // read before the request is consumed, and a header that is not valid text is withheld, not
        // ignored: a decision nobody can read must not turn into an organisation-wide search.
        let space_retrieval_header = request
            .metadata()
            .get(crate::knowledge_tools::SPACE_RETRIEVAL_HEADER)
            .map(|value| value.to_str().unwrap_or_default().to_owned());
        let mut req = request.into_inner();
        caller.authorize(&req.org_id, Some(&req.user_id))?;
        self.authorize_run(&caller, &req.run_id, &session_bearer)
            .await?;
        enforce_persistence_free_execution(&caller, req.zdr, "agent_run")?;
        // The attachments are judged right after the zero-retention gate and before
        // any run work: a waived caller passes that gate yet never carries one.
        let persistence_free = req.zdr || caller.zdr || caller.zdr_waiver.is_some();
        enforce_run_attachments(&mut req, persistence_free)?;
        // The run context is judged at the boundary like the route below: a
        // context that breaks its bounds, or that carries conversation content
        // on a zero-retention run, is refused with the closed token only.
        enforce_run_context(&req)?;
        enforce_resume(&req, mp_contracts::loop_fold::continue_after_approval_enabled())?;
        // A question continuation is judged above, but its apply on resume is not built yet (the
        // executor's input pause and its answer step). It is refused here, before anything is
        // consumed, rather than read as an approval.
        if req.resume.as_ref().is_some_and(resume_names_question) {
            return Err(Status::failed_precondition("question_resume_unavailable"));
        }
        // The inference route the request names (`provider_hint` and
        // `subscription_connection_id`) is judged here, at the boundary and
        // before any of the run's work: a route this process may not honour is
        // refused, never replaced by the default one.
        enforce_agent_route(
            &req,
            mp_contracts::agentic_subscription::Switches::from_env(),
            &caller,
        )?;
        info!(
            run_id = %req.run_id,
            thread_id = %req.thread_id,
            mode = %req.mode,
            "run_agent: driving agent run through the governed multi-tool loop"
        );
        let response = runtime_loop::agent::run_agent(
            &self.state,
            self.session_channel.clone(),
            self.inference_channel(),
            &caller,
            req,
            Some(data_plane_bearer.as_str().to_owned()),
            Some(session_bearer.as_str().to_owned()),
            inference_bearer.as_str().to_owned(),
            self.capability_policy.as_ref(),
            self.terminal_tokens.as_ref(),
            sandbox_bearer
                .as_ref()
                .map(|bearer| bearer.as_str().to_owned()),
            browser_bearer
                .as_ref()
                .map(|bearer| bearer.as_str().to_owned()),
            self.capability_client.as_ref(),
            &self.sandbox_manager_client,
            &self.backend_id,
            self.cas_client.as_ref(),
            &self.sandbox_tokens,
            self.process_host.as_deref(),
            crate::knowledge_tools::SpaceRetrieval::from_header(space_retrieval_header.as_deref()),
        )
        .await?;
        Ok(Response::new(response))
    }

    async fn execute_scheduled_step(
        &self,
        request: Request<pb::ExecuteScheduledStepRequest>,
    ) -> Result<Response<pb::ExecuteScheduledStepResponse>, Status> {
        let caller = self.auth.authenticate_scheduled_step(&request).await?;
        // Deliberately fail closed until the complete service-owned lane is
        // deployed: Control step decisions, Session current-run claim/receipt,
        // and a service-principal verifier are all required before any model
        // or provider work can be reached. In particular, do not fall back to
        // ExecuteStep or manufacture a user identity from this request.
        let req = request.into_inner();
        validate_scheduled_step_bindings(&req)?;
        authorize_scheduled_step_service(&caller, &req.org_id)?;
        let verifier = self
            .scheduled_step_decision_verifier
            .as_ref()
            .ok_or_else(|| {
                Status::failed_precondition(
                    "scheduled-step Control decision verifier is not configured",
                )
            })?;
        verifier.verify(&req, chrono::Utc::now())?;
        self.execute_scheduled_step_runtime(req).await
    }
}

struct ReadinessGuard(Readiness);

impl Drop for ReadinessGuard {
    fn drop(&mut self) {
        self.0.set_grpc_ready(false);
    }
}

/// Start the authenticated gRPC server on :9093.
///
/// The listener is deliberately not host-published by Compose. Execution and
/// agent-run RPCs require a user-bound `aud=execution-core` bearer plus
/// separate `aud=data-plane`, `aud=session-core`, and `aud=inference-core`
/// credentials for tool-capable calls. Delegated credentials are used only
/// for their matching downstream and are independently verified there. Every run
/// mutation has the same gate, so Orchestrator's unsigned legacy `ExecuteStep`
/// path remains fail-closed.
///
/// # Errors
///
/// Returns an error if the server cannot bind or one of its endpoints is
/// malformed.
pub async fn serve(
    state: StateStore,
    readiness: Readiness,
    auth: JwtVerifier,
) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind("0.0.0.0:9093").await?;
    info!(
        thread_workspace = crate::sandbox_lease::workspace_sync::switched_on(),
        "execution-core thread workspace switch read at startup"
    );
    let session_url = std::env::var("SESSION_CORE_URL")
        .or_else(|_| std::env::var("SESSION_CORE_ADDR"))
        .unwrap_or_else(|_| "http://localhost:9091".to_owned());
    let inference_url = std::env::var("INFERENCE_CORE_URL")
        .or_else(|_| std::env::var("INFERENCE_CORE_ADDR"))
        .unwrap_or_else(|_| "http://inference-core:9092".to_owned());
    let browser_url = std::env::var("BROWSER_BROKER_URL")
        .or_else(|_| std::env::var("BROWSER_BROKER_ADDR"))
        .unwrap_or_else(|_| "http://browser-broker:9095".to_owned());
    let capability_url = std::env::var("CAPABILITY_CORE_ADDR")
        .unwrap_or_else(|_| "http://capability-core:9097".to_owned());
    let capability_channel = lazy_bounded_channel(capability_url)?;
    let skill_bundles =
        match crate::skill_mounts::GrpcSkillBundleSource::from_env(capability_channel.clone()) {
            Ok(source) => Some(Arc::new(source)),
            Err(error) => {
                warn!(%error, "skill bundle source unavailable; skill mounts fail closed");
                None
            }
        };
    let capability_policy = Arc::new(crate::capability_policy::GrpcCapabilityPolicy::from_env(
        capability_channel,
    )?);
    let terminal_tokens: Arc<dyn ManagedRunTokenProvider> =
        Arc::new(SessionTerminalTokenProvider::from_env()?);
    let scheduled_step_decision_verifier = match ScheduledStepDecisionVerifier::from_env() {
        Ok(verifier) => Some(verifier),
        Err(error) => {
            warn!(%error, "scheduled-step Control decision verifier unavailable; lane remains disabled");
            None
        }
    };
    let scheduled_inference_tokens = match ScheduledInferenceTokenProvider::from_env() {
        Ok(provider) => Some(Arc::new(provider)),
        Err(error) => {
            warn!(%error, "scheduled-step inference credential unavailable; lane remains disabled");
            None
        }
    };
    // Same Control Plane client `http_health.rs`'s `/capability-profile`
    // endpoint already uses — absent configuration is a valid disabled state
    // there, and identically so here: a Space-scoped `code_interpreter` step
    // fails closed (see `sandbox_lease::ensure_sandbox_lease`) rather than the
    // whole process refusing to start.
    let capability_client = CapabilityClient::from_env()
        .map_err(|error| anyhow::anyhow!("sandbox capability client configuration: {error}"))?;
    let sandbox_manager_client = SandboxManagerClient::from_env()
        .map_err(|error| anyhow::anyhow!("sandbox-manager client configuration: {error}"))?;
    // Absent configuration is a valid disabled state (§1's own convention) —
    // a Space-scoped code_interpreter step fails closed without it, same as
    // an absent capability_client above; nothing reads this yet (3.5.C.5).
    let cas_client = CasClient::from_env()
        .map_err(|error| anyhow::anyhow!("CAS client configuration: {error}"))?;
    let backend_id = crate::http_health::resolve_backend_id();
    // Same deployment service principal capability_policy's own
    // ServiceTokenProvider already requires (EXECUTION_CORE_SERVICE_ID/
    // EXECUTION_CORE_SERVICE_API_KEY/AUTH_CORE_URL) — construction above
    // already fails startup closed without them, so requiring them again
    // here for a different audience changes nothing about what a deployment
    // must configure.
    let sandbox_tokens = Arc::new(
        crate::sandbox_lease::SandboxManagerTokenProvider::from_env().map_err(|error| {
            anyhow::anyhow!("sandbox-manager service token provider configuration: {error}")
        })?,
    );

    serve_with_listener(
        state,
        readiness,
        auth,
        listener,
        session_url,
        inference_url,
        browser_url,
        capability_policy,
        terminal_tokens,
        scheduled_step_decision_verifier,
        scheduled_inference_tokens,
        capability_client,
        sandbox_manager_client,
        cas_client,
        backend_id,
        sandbox_tokens,
        skill_bundles,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn serve_with_listener(
    state: StateStore,
    readiness: Readiness,
    auth: JwtVerifier,
    listener: tokio::net::TcpListener,
    session_url: String,
    inference_url: String,
    browser_url: String,
    capability_policy: Arc<dyn crate::capability_policy::CapabilityPolicy>,
    terminal_tokens: Arc<dyn ManagedRunTokenProvider>,
    scheduled_step_decision_verifier: Option<ScheduledStepDecisionVerifier>,
    scheduled_inference_tokens: Option<Arc<ScheduledInferenceTokenProvider>>,
    capability_client: Option<CapabilityClient>,
    sandbox_manager_client: SandboxManagerClient,
    cas_client: Option<CasClient>,
    backend_id: String,
    sandbox_tokens: Arc<crate::sandbox_lease::SandboxManagerTokenProvider>,
    skill_bundles: Option<Arc<crate::skill_mounts::GrpcSkillBundleSource>>,
) -> anyhow::Result<()> {
    let session_channel = lazy_bounded_channel(session_url)?;
    let inference_channel = lazy_bounded_channel(inference_url)?;
    let browser_channel = lazy_bounded_channel(browser_url)?;

    readiness.set_grpc_ready(true);
    let _readiness_guard = ReadinessGuard(readiness);
    info!("gRPC listening on :9093");

    tonic::transport::Server::builder()
        .add_service(
            ExecutionCoreServer::new(ExecutionService::new_with_scheduled_step_runtime(
                state,
                auth,
                session_channel,
                inference_channel,
                browser_channel,
                capability_policy,
                terminal_tokens,
                scheduled_step_decision_verifier,
                scheduled_inference_tokens,
                capability_client,
                sandbox_manager_client,
                cas_client,
                backend_id,
                sandbox_tokens,
                skill_bundles,
            ))
            .max_decoding_message_size(request_decoding_limit_bytes()),
        )
        .serve_with_incoming(TcpListenerStream::new(listener))
        .await?;

    Ok(())
}

/// Largest request tonic decodes. A request carrying attachments holds their
/// bytes raw (gRPC has no base64), so the floor is raised to the declared
/// attachment total (`CHAT_ATTACHMENT_MAX_TOTAL_BYTES`, the number model-gateway
/// and the BFF derive their body limits from) plus room for the rest of the
/// request. Unset, it is the floor: nothing changes until the cap is raised.
fn request_decoding_limit_bytes() -> usize {
    decoding_limit_with(&|key| std::env::var(key).ok())
}

fn decoding_limit_with(get: &dyn Fn(&str) -> Option<String>) -> usize {
    get("CHAT_ATTACHMENT_MAX_TOTAL_BYTES")
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|total| *total > 0)
        .map_or(MAX_REQUEST_DECODING_BYTES, |total| {
            MAX_REQUEST_DECODING_BYTES.max(total.saturating_add(REQUEST_DECODING_HEADROOM_BYTES))
        })
}

#[cfg(test)]
mod decoding_limit_tests {
    use super::{decoding_limit_with, MAX_REQUEST_DECODING_BYTES, REQUEST_DECODING_HEADROOM_BYTES};

    fn limit(total: Option<&str>) -> usize {
        decoding_limit_with(&|_| total.map(str::to_owned))
    }

    #[test]
    fn unset_or_unreadable_totals_keep_the_floor() {
        assert_eq!(limit(None), MAX_REQUEST_DECODING_BYTES);
        assert_eq!(limit(Some("0")), MAX_REQUEST_DECODING_BYTES);
        assert_eq!(limit(Some("lots")), MAX_REQUEST_DECODING_BYTES);
        // Today's declared total (3 MiB) fits under the floor.
        assert_eq!(limit(Some("3145728")), MAX_REQUEST_DECODING_BYTES);
    }

    #[test]
    fn a_raised_total_raises_the_limit_to_hold_it() {
        let total = 64 * 1024 * 1024;
        assert_eq!(
            limit(Some(&total.to_string())),
            total + REQUEST_DECODING_HEADROOM_BYTES
        );
    }
}

#[cfg(test)]
mod auth_tests {
    use super::*;
    use crate::session_terminal_auth::SessionTerminalTokenError;
    use mp_contracts::model_plane::v1::execution_core_client::ExecutionCoreClient;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn session_bearer() -> crate::auth::DelegatedSessionBearer {
        crate::auth::DelegatedSessionBearer::for_test()
    }

    struct AllowCapabilityPolicy;

    #[tonic::async_trait]
    impl crate::capability_policy::CapabilityPolicy for AllowCapabilityPolicy {
        async fn evaluate(
            &self,
            _tool_name: &str,
            _run_id: &str,
            _org_id: &str,
        ) -> Result<crate::capability_policy::CapabilityDecision, Status> {
            Ok(crate::capability_policy::CapabilityDecision::Allow)
        }
    }

    struct StaticTerminalTokens;

    #[tonic::async_trait]
    impl ManagedRunTokenProvider for StaticTerminalTokens {
        async fn terminalize_token(
            &self,
            _org_id: &str,
        ) -> Result<String, SessionTerminalTokenError> {
            Ok("test-terminalize-service-token".to_owned())
        }

        async fn heartbeat_token(
            &self,
            _org_id: &str,
        ) -> Result<String, SessionTerminalTokenError> {
            Ok("test-heartbeat-service-token".to_owned())
        }

        async fn scheduled_step_token(
            &self,
            _org_id: &str,
        ) -> Result<String, SessionTerminalTokenError> {
            Ok("test-scheduled-step-service-token".to_owned())
        }
    }

    struct FakeOwnershipResolver {
        owner_org: &'static str,
        owner_user: &'static str,
        unavailable: bool,
        calls: AtomicUsize,
    }

    #[tonic::async_trait]
    impl RunOwnershipResolver for FakeOwnershipResolver {
        async fn resolve(
            &self,
            _run_id: &str,
            org_id: &str,
            user_id: &str,
            _bearer: &str,
        ) -> Result<bool, Status> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.unavailable {
                return Err(Status::unavailable("session unavailable"));
            }
            Ok(org_id == self.owner_org && user_id == self.owner_user)
        }
    }

    #[tokio::test]
    async fn durable_owner_is_resolved_before_cache_and_attacker_cannot_win_after_restart() {
        let state = StateStore::new();
        let resolver = FakeOwnershipResolver {
            owner_org: "org-owner",
            owner_user: "user-owner",
            unavailable: false,
            calls: AtomicUsize::new(0),
        };
        let attacker = AuthenticatedUser::for_test("org-attacker", "user-attacker");
        assert_eq!(
            authorize_durable_run_owner(
                &state,
                &resolver,
                &attacker,
                "run-existing",
                &session_bearer()
            )
            .await
            .unwrap_err()
            .code(),
            tonic::Code::PermissionDenied
        );
        assert!(!state.authorizes("run-existing", "org-attacker", "user-attacker"));

        let owner = AuthenticatedUser::for_test("org-owner", "user-owner");
        authorize_durable_run_owner(&state, &resolver, &owner, "run-existing", &session_bearer())
            .await
            .expect("durable owner");
        authorize_durable_run_owner(&state, &resolver, &owner, "run-existing", &session_bearer())
            .await
            .expect("verified cache");
        assert_eq!(resolver.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn durable_owner_unavailable_or_unknown_fails_closed_without_cache() {
        let state = StateStore::new();
        let caller = AuthenticatedUser::for_test("org-owner", "user-owner");
        let unavailable = FakeOwnershipResolver {
            owner_org: "org-owner",
            owner_user: "user-owner",
            unavailable: true,
            calls: AtomicUsize::new(0),
        };
        assert_eq!(
            authorize_durable_run_owner(
                &state,
                &unavailable,
                &caller,
                "run-existing",
                &session_bearer()
            )
            .await
            .unwrap_err()
            .code(),
            tonic::Code::Unavailable
        );
        assert!(!state.authorizes("run-existing", "org-owner", "user-owner"));

        let unknown = FakeOwnershipResolver {
            owner_org: "org-other",
            owner_user: "user-other",
            unavailable: false,
            calls: AtomicUsize::new(0),
        };
        assert_eq!(
            authorize_durable_run_owner(
                &state,
                &unknown,
                &caller,
                "run-existing",
                &session_bearer()
            )
            .await
            .unwrap_err()
            .code(),
            tonic::Code::PermissionDenied
        );
        assert!(!state.authorizes("run-existing", "org-owner", "user-owner"));
    }

    #[test]
    fn approval_resume_requires_a_granted_durable_record_bound_to_run_and_tenant() {
        let granted = pb::Approval {
            id: "appr_1".to_owned(),
            run_id: "run_1".to_owned(),
            state: pb::ApprovalState::Granted as i32,
            org_id: "org_1".to_owned(),
            ..Default::default()
        };

        validate_granted_approval(Some(&granted), "appr_1", "run_1", "org_1")
            .expect("exact durable grant");
        assert_eq!(
            validate_granted_approval(Some(&granted), "appr_1", "run_other", "org_1")
                .unwrap_err()
                .code(),
            tonic::Code::PermissionDenied
        );
        assert_eq!(
            validate_granted_approval(Some(&granted), "appr_1", "run_1", "org_other")
                .unwrap_err()
                .code(),
            tonic::Code::PermissionDenied
        );

        let pending = pb::Approval {
            state: pb::ApprovalState::Requested as i32,
            ..granted
        };
        assert_eq!(
            validate_granted_approval(Some(&pending), "appr_1", "run_1", "org_1")
                .unwrap_err()
                .code(),
            tonic::Code::FailedPrecondition
        );
        assert_eq!(
            validate_granted_approval(None, "appr_1", "run_1", "org_1")
                .unwrap_err()
                .code(),
            tonic::Code::FailedPrecondition
        );
    }

    #[test]
    fn durable_grant_stays_gated_until_a_real_continuation_delivery_exists() {
        let state = StateStore::new();
        state.update(RunSnapshot {
            run_id: "run_approval".to_owned(),
            step_index: 7,
            status: RunStatus::AwaitingApproval,
            last_error: None,
        });

        let error = resume_durable_approval_continuation(&state, "run_approval")
            .expect_err("a state flip must not impersonate continuation delivery");
        assert_eq!(error.code(), tonic::Code::Unavailable);
        assert_eq!(
            state.snapshot("run_approval").map(|run| run.status),
            Some(RunStatus::AwaitingApproval)
        );
    }

    #[test]
    fn direct_execute_step_refuses_reserved_owner_actions_and_ambiguous_modes() {
        let gate = crate::permission::RunGate::for_run("org-direct", "run-direct");
        let error = reject_direct_owner_action(gate, crate::ticket_tools::TOOL_NAME)
            .expect_err("a public direct-step RPC cannot execute an owner action");
        assert_eq!(error.code(), tonic::Code::PermissionDenied);
        assert_eq!(
            reject_direct_owner_action(gate, "  tickets.create  ")
                .expect_err("whitespace does not turn a reserved action into a direct tool")
                .code(),
            tonic::Code::PermissionDenied
        );
        reject_direct_owner_action(gate, "shell").expect("ordinary tools keep the direct API");

        for mode in ["", "execute", "unknown"] {
            assert_eq!(
                validate_direct_permission_mode(mode)
                    .expect_err("direct calls need an explicit supported mode")
                    .code(),
                tonic::Code::InvalidArgument
            );
        }
        for mode in ["auto", "ask", "deny"] {
            validate_direct_permission_mode(mode).expect("recognized permission mode");
        }
    }

    /// Under `enforce` the run's declared `durable_owner` floor makes a tool an owner
    /// action here though its name is not reserved. The gate off reads no view, and a
    /// reserved name stays refused whatever the view says.
    #[test]
    fn direct_execute_step_refuses_a_tool_the_run_declares_durable_owner_under_enforce() {
        use crate::capability_policy::ToolClassGate;
        use crate::permission::{record_run_views, RunGate, ToolClassView};
        let (org, run) = ("org-direct-owner", "run-direct-owner");
        record_run_views(
            org,
            run,
            [
                (
                    "mcp__srv__pay".to_owned(),
                    ToolClassView::from_registry(
                        "mcp__srv__pay",
                        "external_write",
                        "durable_owner",
                        "d",
                    ),
                ),
                (
                    "tickets.create".to_owned(),
                    ToolClassView::from_registry("tickets.create", "read", "none", "d"),
                ),
            ],
        );
        let enforce = RunGate::with_gate(ToolClassGate::Enforce, org, run);
        let off = RunGate::with_gate(ToolClassGate::Off, org, run);
        assert_eq!(
            reject_direct_owner_action(enforce, "mcp__srv__pay")
                .expect_err("a declared durable_owner tool is not a direct tool")
                .code(),
            tonic::Code::PermissionDenied
        );
        reject_direct_owner_action(off, "mcp__srv__pay")
            .expect("with the gate off the declared floor is never read");
        assert_eq!(
            reject_direct_owner_action(enforce, "tickets.create")
                .expect_err("a reserved name is refused whatever the view says")
                .code(),
            tonic::Code::PermissionDenied
        );
    }

    #[test]
    fn scheduled_result_request_carries_identity_and_answer_only() {
        let req = pb::ExecuteScheduledStepRequest {
            run_id: "run_01".to_owned(),
            org_id: "org_01".to_owned(),
            step_id: "step_01".to_owned(),
            idempotency_key: "fire_01:step:0".to_owned(),
            ..Default::default()
        };
        let result = scheduled_run_result_request(&req, "scheduled_step_01", "the answer");
        assert_eq!(result.run_id, "run_01");
        assert_eq!(result.org_id, "org_01");
        assert_eq!(result.step_id, "step_01");
        assert_eq!(result.idempotency_key, "fire_01:step:0");
        assert_eq!(result.receipt_id, "scheduled_step_01");
        assert_eq!(result.status, "completed");
        assert_eq!(result.output, "the answer");
        assert!(!result.unknown_outcome);
        assert!(result.error_code.is_empty());
        assert_eq!(result.withheld_effects, 0);
    }

    #[test]
    fn scheduled_result_persist_is_off_unless_the_literal_switch_is_true() {
        if std::env::var_os(SCHEDULED_RESULT_PERSIST_ENV).is_none() {
            assert!(!scheduled_result_persist_enabled(), "off by default");
        }
        assert!(mp_contracts::loop_fold::switch_from(Some("true")));
        assert!(!mp_contracts::loop_fold::switch_from(Some("on")));
    }

    #[test]
    fn scheduled_step_contract_rejects_wildcards_and_requires_digests() {
        let valid_digest = format!("sha256:{}", "a".repeat(64));
        let valid = pb::ExecuteScheduledStepRequest {
            run_id: "run_01".to_owned(),
            thread_id: "thread_01".to_owned(),
            org_id: "org_01".to_owned(),
            space_id: "space_01".to_owned(),
            subject_id: "user_01".to_owned(),
            schedule_id: "schedule_01".to_owned(),
            fire_key: "fire_01".to_owned(),
            template_digest: valid_digest.clone(),
            step_id: "step_01".to_owned(),
            step_index: 0,
            policy_digest: valid_digest,
            idempotency_key: "idem_01".to_owned(),
            control_decision_token: "scheduled-step-decision".to_owned(),
        };
        validate_scheduled_step_bindings(&valid).expect("canonical bindings are accepted");

        let mut forged = valid.clone();
        forged.org_id = "*".to_owned();
        assert_eq!(
            validate_scheduled_step_bindings(&forged)
                .expect_err("wildcard tenant must not become an authority selector")
                .code(),
            tonic::Code::InvalidArgument
        );

        let mut missing_digest = valid;
        missing_digest.template_digest.clear();
        assert_eq!(
            validate_scheduled_step_bindings(&missing_digest)
                .expect_err("scheduled work must bind an immutable template")
                .code(),
            tonic::Code::InvalidArgument
        );
    }

    #[test]
    fn scheduled_step_service_gate_is_exact_and_non_zdr() {
        let valid = AuthenticatedService::for_test(
            "org_01",
            SCHEDULED_STEP_SERVICE_ID,
            &[SCHEDULED_STEP_SCOPE],
            false,
        );
        authorize_scheduled_step_service(&valid, "org_01")
            .expect("exact scheduled-step service is accepted");

        for (service_id, scopes, org_id, zdr, expected) in [
            (
                "service:other",
                vec![SCHEDULED_STEP_SCOPE],
                "org_01",
                false,
                tonic::Code::PermissionDenied,
            ),
            (
                SCHEDULED_STEP_SERVICE_ID,
                vec!["session:write"],
                "org_01",
                false,
                tonic::Code::PermissionDenied,
            ),
            (
                SCHEDULED_STEP_SERVICE_ID,
                vec![SCHEDULED_STEP_SCOPE],
                "org_02",
                false,
                tonic::Code::PermissionDenied,
            ),
            (
                SCHEDULED_STEP_SERVICE_ID,
                vec![SCHEDULED_STEP_SCOPE],
                "org_01",
                true,
                tonic::Code::FailedPrecondition,
            ),
        ] {
            let caller = AuthenticatedService::for_test("org_01", service_id, &scopes, zdr);
            assert_eq!(
                authorize_scheduled_step_service(&caller, org_id)
                    .expect_err("scheduled-step service gate must fail closed")
                    .code(),
                expected
            );
        }
    }

    #[test]
    fn signed_zdr_blocks_execution_dispatch_and_durable_run_control() {
        let caller = AuthenticatedUser::for_test_with_zdr("org-owner", "user-owner", true);
        for operation in ["tool_execution", "run_control"] {
            let error = enforce_persistence_free_execution(&caller, false, operation)
                .expect_err("signed ZDR must dominate a caller-controlled request flag");
            assert_eq!(error.code(), tonic::Code::FailedPrecondition);
        }
    }

    #[test]
    fn scheduled_step_runtime_is_tool_free_and_bounded_to_unknown_on_transport_loss() {
        let request = pb::ExecuteScheduledStepRequest {
            org_id: "org-1".to_owned(),
            step_id: "run-1:step:0".to_owned(),
            idempotency_key: "run-1:step:0".to_owned(),
            ..Default::default()
        };
        let inference = scheduled_inference_request(&request, "send the daily summary");
        assert!(inference.tools.is_empty());
        assert_eq!(inference.tool_choice, "none");
        assert_eq!(inference.zdr, false);
        assert_eq!(inference.request_id, "scheduled:org-1:run-1:step:0");
        assert_eq!(inference.messages[1].content, "send the daily summary");

        assert_eq!(
            scheduled_step_failure_status(tonic::Code::DeadlineExceeded),
            ("unknown_outcome", "inference_outcome_unknown", true)
        );
        assert_eq!(
            scheduled_step_failure_status(tonic::Code::Unavailable),
            ("unknown_outcome", "inference_outcome_unknown", true)
        );
        assert_eq!(
            scheduled_step_failure_status(tonic::Code::InvalidArgument),
            ("failed", "inference_failed", false)
        );
    }

    #[tokio::test]
    async fn direct_hitl_rejects_an_undurable_approval_pause() {
        // Bind then release an ephemeral address so the real CreateApproval
        // client receives a deterministic connection failure rather than this
        // test relying on a shared well-known port.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral listener");
        let address = listener.local_addr().expect("listener address");
        drop(listener);
        // Built through the SAME helper production uses: a channel the test
        // bounded by itself would prove nothing about the service.
        let channel = lazy_bounded_channel(format!("http://{address}")).expect("endpoint");
        let state = StateStore::new();

        // Comfortably above GRPC_CONNECT_TIMEOUT and far below "hangs": the
        // point is that SOME application-level bound exists, not its exact value.
        let error = tokio::time::timeout(
            GRPC_CONNECT_TIMEOUT + Duration::from_secs(5),
            create_durable_approval(
                &channel,
                pb::CreateApprovalRequest {
                    run_id: "run-undurable".to_owned(),
                    step_id: "step-undurable".to_owned(),
                    ..Default::default()
                },
                "session-token",
            ),
        )
        .await
        .expect("CreateApproval connection failure should be bounded")
        .expect_err("an unpersisted approval must fail the direct HITL path");

        assert_eq!(error.code(), tonic::Code::Unavailable);
        assert!(
            state.snapshot("run-undurable").is_none(),
            "the failed persistence path must not claim AwaitingApproval"
        );
    }

    fn subscription_run() -> pb::RunAgentRequest {
        pb::RunAgentRequest {
            run_id: "run-1".to_owned(),
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
            model: "gpt-5.6-terra".to_owned(),
            provider_hint: "openai-codex-subscription".to_owned(),
            subscription_connection_id: "conn_1".to_owned(),
            plan_mode: true,
            ..Default::default()
        }
    }

    const ROUTE_ON: mp_contracts::agentic_subscription::Switches =
        mp_contracts::agentic_subscription::Switches {
            enabled: true,
            allow_effects: false,
        };

    /// The verified caller of a run that is not ZDR and holds no waiver.
    fn route_caller() -> AuthenticatedUser {
        AuthenticatedUser::for_test("org-1", "user-1")
    }

    #[test]
    fn a_subscription_run_is_refused_when_the_switch_is_off() {
        let error = enforce_agent_route(
            &subscription_run(),
            mp_contracts::agentic_subscription::Switches::OFF,
            &route_caller(),
        )
        .expect_err("a disabled route must not start");
        assert_eq!(error.code(), tonic::Code::PermissionDenied);
        assert!(
            error
                .message()
                .contains(mp_contracts::agentic_subscription::DISABLED_CODE),
            "{}",
            error.message()
        );
        // The only difference from a run that is accepted is the switch.
        enforce_agent_route(&subscription_run(), ROUTE_ON, &route_caller())
            .expect("switch on admits the run");
    }

    #[test]
    fn a_subscription_run_is_refused_with_zdr_or_a_pseudo_model() {
        let mut zdr = subscription_run();
        zdr.zdr = true;
        let error = enforce_agent_route(&zdr, ROUTE_ON, &route_caller())
            .expect_err("the seat has no ZDR commitment");
        assert_eq!(error.code(), tonic::Code::PermissionDenied);

        for pseudo in ["verevon-balance", "verevon-auto", "Verevon-Genius", ""] {
            let mut request = subscription_run();
            request.model = pseudo.to_owned();
            let error = enforce_agent_route(&request, ROUTE_ON, &route_caller())
                .expect_err("a pseudo model names no model");
            assert_eq!(error.code(), tonic::Code::InvalidArgument, "{pseudo:?}");
        }
    }

    /// The gateway boundary admits a waived run on its own connection only (Z-18):
    /// the same request, with the caller's waiver for another connection, is refused.
    #[test]
    fn a_waived_subscription_run_is_admitted_only_on_its_own_connection() {
        let waived = AuthenticatedUser::for_test_with_waiver("org-1", "user-1", "conn_1", "ack-1");
        enforce_agent_route(&subscription_run(), ROUTE_ON, &waived)
            .expect("the waiver binds this connection");

        let other = AuthenticatedUser::for_test_with_waiver("org-1", "user-1", "conn_2", "ack-1");
        let error = enforce_agent_route(&subscription_run(), ROUTE_ON, &other)
            .expect_err("a waiver for another connection is refused");
        assert_eq!(error.code(), tonic::Code::PermissionDenied);
        assert_eq!(
            mp_contracts::zdr_refusal::Refusal::from_status(&error).map(|refusal| refusal.code()),
            Some(mp_contracts::zdr_refusal::code::WAIVER_ROUTE_MISMATCH)
        );
    }

    #[test]
    fn every_route_refusal_is_one_the_gateway_terminalizes_as_a_rejection() {
        // model-gateway closes the prepared run only for these three codes
        // (`is_confirmed_agent_dispatch_rejection`); any other would leave the
        // run open to be retried into the same refusal.
        let mut half_set = subscription_run();
        half_set.subscription_connection_id.clear();
        let mut alias = subscription_run();
        alias.provider_hint = "chatgpt-codex".to_owned();
        let mut acting = subscription_run();
        acting.plan_mode = false;
        for (label, request, switches) in [
            ("switch off", subscription_run(), mp_contracts::agentic_subscription::Switches::OFF),
            ("half-set route", half_set, ROUTE_ON),
            ("alias provider", alias, ROUTE_ON),
            ("not read-only", acting, ROUTE_ON),
        ] {
            let error = enforce_agent_route(&request, switches, &route_caller()).expect_err(label);
            assert!(
                matches!(
                    error.code(),
                    tonic::Code::InvalidArgument
                        | tonic::Code::PermissionDenied
                        | tonic::Code::Unauthenticated
                ),
                "{label}: {:?}",
                error.code()
            );
        }
    }

    #[test]
    fn a_run_without_route_fields_is_never_affected_by_the_switch() {
        let plain = pb::RunAgentRequest {
            run_id: "run-1".to_owned(),
            model: "gpt-4o-mini".to_owned(),
            ..Default::default()
        };
        for switches in [mp_contracts::agentic_subscription::Switches::OFF, ROUTE_ON] {
            enforce_agent_route(&plain, switches, &route_caller())
                .expect("a route-less run is unchanged");
        }
    }

    #[tokio::test]
    async fn live_grpc_listener_is_ready_and_rejects_unsigned_execution() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral listener");
        let address = listener.local_addr().expect("listener address");
        let readiness = Readiness::new();
        let server_readiness = readiness.clone();
        let auth = test_verifier().await;
        let handle = tokio::spawn(serve_with_listener(
            StateStore::new(),
            server_readiness,
            auth,
            listener,
            "http://127.0.0.1:1".to_owned(),
            "http://127.0.0.1:1".to_owned(),
            "http://127.0.0.1:1".to_owned(),
            Arc::new(AllowCapabilityPolicy),
            Arc::new(StaticTerminalTokens),
            None,
            None,
            None,
            SandboxManagerClient::from_env().expect("valid default sandbox-manager endpoint"),
            None,
            "test-backend".to_owned(),
            std::sync::Arc::new(
                crate::sandbox_lease::SandboxManagerTokenProvider::new_for_test(
                    "http://127.0.0.1:1",
                    "execution-core",
                    "test-service-secret-at-least-32-bytes",
                ),
            ),
            None,
        ));

        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while !readiness.is_ready() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("gRPC listener should become ready");

        let mut client = ExecutionCoreClient::connect(format!("http://{address}"))
            .await
            .expect("connect to execution gRPC");
        let error = client
            .execute_step(pb::ExecuteStepRequest::default())
            .await
            .expect_err("unsigned execution must fail closed");
        assert_eq!(error.code(), tonic::Code::Unauthenticated);

        let error = client
            .resume_run(pb::ResumeRunRequest::default())
            .await
            .expect_err("unsigned run mutation must fail closed");
        assert_eq!(error.code(), tonic::Code::Unauthenticated);

        handle.abort();
        let _ = handle.await;
        assert!(!readiness.is_ready());
    }

    async fn test_verifier() -> JwtVerifier {
        use base64::Engine;
        use rsa::pkcs8::{EncodePrivateKey, LineEnding};
        use rsa::traits::PublicKeyParts;
        use wiremock::{
            matchers::{method, path},
            Mock, MockServer, ResponseTemplate,
        };

        let mut rng = rand::thread_rng();
        let private = rsa::RsaPrivateKey::new(&mut rng, 2048).expect("RSA key generation");
        let _private_pem = private.to_pkcs8_pem(LineEnding::LF).expect("private PEM");
        let public = rsa::RsaPublicKey::from(&private);
        let n = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public.n().to_bytes_be());
        let e = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public.e().to_bytes_be());
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "keys": [{
                    "kty": "RSA", "use": "sig", "alg": "RS256",
                    "kid": "listener-key", "n": n, "e": e
                }]
            })))
            .mount(&server)
            .await;
        JwtVerifier::from_config(crate::auth::AuthConfig {
            jwks_url: format!("{}/jwks", server.uri()),
            issuer: "auth-core".to_owned(),
            audience: "data-plane".to_owned(),
            jwks_ttl: std::time::Duration::from_secs(300),
            leeway_secs: 0,
        })
        .await
        .expect("test verifier")
    }
}

#[cfg(test)]
mod run_context_boundary_tests {
    use super::*;

    fn request_with(context: Option<pb::RunContext>, zdr: bool) -> pb::RunAgentRequest {
        pb::RunAgentRequest {
            run_id: "run".to_owned(),
            zdr,
            run_context: context,
            ..Default::default()
        }
    }

    fn history() -> pb::RunContext {
        pb::RunContext {
            history: vec![pb::ChatMessage {
                role: "user".to_owned(),
                content: "an earlier question".to_owned(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn a_zdr_run_with_history_is_refused_at_the_boundary() {
        let refused = enforce_run_context(&request_with(Some(history()), true))
            .expect_err("history on a ZDR run must be refused");
        assert_eq!(refused.code(), tonic::Code::InvalidArgument);
        // Only the closed token leaves the service, never the content.
        assert_eq!(refused.message(), "run_context_zdr_history");
    }

    fn resume(mode: &str) -> pb::AgentResume {
        pb::AgentResume {
            mode: mode.to_owned(),
            approval_id: "appr_1".to_owned(),
            claim_token: "claim".to_owned(),
            ..Default::default()
        }
    }

    fn with_resume(r: Option<pb::AgentResume>) -> pb::RunAgentRequest {
        pb::RunAgentRequest {
            run_id: "run".to_owned(),
            resume: r,
            ..Default::default()
        }
    }

    #[test]
    fn a_continuation_is_refused_while_the_switch_is_off_and_when_incomplete() {
        let refused = enforce_resume(&with_resume(Some(resume("segment"))), false)
            .expect_err("switch off");
        assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
        assert_eq!(refused.message(), "continue_disabled");

        assert!(enforce_resume(&with_resume(Some(resume("segment"))), true).is_ok());
        assert!(enforce_resume(&with_resume(Some(resume("loop"))), true).is_ok());
        assert!(enforce_resume(&with_resume(None), false).is_ok(), "no resume = today");

        let mut bad_mode = resume("segment");
        bad_mode.mode = "other".to_owned();
        let mut no_claim = resume("segment");
        no_claim.claim_token.clear();
        let mut no_approval = resume("segment");
        no_approval.approval_id.clear();
        for incomplete in [bad_mode, no_claim, no_approval] {
            let e = enforce_resume(&with_resume(Some(incomplete)), true).expect_err("incomplete");
            assert_eq!(e.code(), tonic::Code::InvalidArgument);
            assert_eq!(e.message(), "resume_incomplete");
        }
    }

    fn question_resume(mode: &str) -> pb::AgentResume {
        pb::AgentResume {
            mode: mode.to_owned(),
            question_id: "q_1".to_owned(),
            answer_json: "{}".to_owned(),
            claim_token: "claim".to_owned(),
            ..Default::default()
        }
    }

    /// A question continuation is refused while its switch is off, and it names exactly one question,
    /// its answer and its claim. It never names an approval, a step, a tool or a brief.
    #[test]
    fn enforce_question_resume_needs_the_switch_and_exactly_the_question_shape() {
        for mode in ["segment", "loop"] {
            assert!(enforce_question_resume(&question_resume(mode), true).is_ok(), "{mode}");
            let off = enforce_question_resume(&question_resume(mode), false).expect_err("switch off");
            assert_eq!(off.message(), "continue_disabled");
        }
        let incomplete = [
            pb::AgentResume { mode: "park".to_owned(), ..question_resume("segment") },
            pb::AgentResume { approval_id: "appr_1".to_owned(), ..question_resume("segment") },
            pb::AgentResume { tool_name: "delete_records".to_owned(), ..question_resume("segment") },
            pb::AgentResume { brief: "recorded results".to_owned(), ..question_resume("segment") },
            pb::AgentResume { question_id: String::new(), ..question_resume("segment") },
            pb::AgentResume { answer_json: String::new(), ..question_resume("segment") },
            pb::AgentResume { claim_token: String::new(), ..question_resume("segment") },
            pb::AgentResume { mode: "PARK".to_owned(), ..question_resume("segment") },
        ];
        for shape in incomplete {
            let e = enforce_question_resume(&shape, true).expect_err("an incomplete question shape");
            assert_eq!(e.message(), "resume_incomplete");
        }
    }

    /// Only a continuation that carries a question or an answer takes the question check, so an
    /// approval continuation keeps its own.
    #[test]
    fn resume_names_a_question_only_when_it_carries_one() {
        assert!(resume_names_question(&question_resume("segment")));
        let answer_only = pb::AgentResume { answer_json: "{}".to_owned(), ..Default::default() };
        assert!(resume_names_question(&answer_only));
        assert!(!resume_names_question(&resume("segment")));
        let park = pb::AgentResume { mode: "park".to_owned(), claim_token: "c".to_owned(), ..Default::default() };
        assert!(!resume_names_question(&park));
    }

    /// With the continue switch off a question continuation is refused for that reason first.
    #[test]
    fn enforce_resume_refuses_a_question_continuation_while_the_continue_switch_is_off() {
        let refused = enforce_resume(&with_resume(Some(question_resume("segment"))), false)
            .expect_err("switch off");
        assert_eq!(refused.message(), "continue_disabled");
    }

    /// A park resume (WS03 S8-6): the `park` mode with a claim token and nothing an approval
    /// names. The switch is judged first, as for every resume; a park that names an approval, a
    /// step, a tool, a fingerprint or a brief, or carries no token, is incomplete. The committed
    /// rows of the other two modes are unchanged, and the validator never reads `segment_no`
    /// or the parking switch (a park stays resumable with parking off).
    #[test]
    fn enforce_resume_park_shape() {
        let park = || pb::AgentResume {
            mode: "park".to_owned(),
            claim_token: "claim".to_owned(),
            ..Default::default()
        };
        // The switch first, whatever the shape.
        for shape in [park(), pb::AgentResume { approval_id: "appr_1".to_owned(), ..park() }] {
            let refused = enforce_resume(&with_resume(Some(shape)), false).expect_err("switch off");
            assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
            assert_eq!(refused.message(), "continue_disabled");
        }
        assert!(enforce_resume(&with_resume(Some(park())), true).is_ok());
        // A client-sent segment number is not read (the executor derives it from the fold).
        assert!(enforce_resume(
            &with_resume(Some(pb::AgentResume { segment_no: 7, ..park() })),
            true
        )
        .is_ok());
        let incomplete = [
            pb::AgentResume { approval_id: "appr_1".to_owned(), ..park() },
            pb::AgentResume { step_id: "tool_1_x".to_owned(), ..park() },
            pb::AgentResume { tool_name: "delete_records".to_owned(), ..park() },
            pb::AgentResume { action_fingerprint: "fp".to_owned(), ..park() },
            pb::AgentResume { brief: "recorded results".to_owned(), ..park() },
            pb::AgentResume { claim_token: String::new(), ..park() },
            pb::AgentResume { mode: "PARK".to_owned(), ..park() },
        ];
        for shape in incomplete {
            let e = enforce_resume(&with_resume(Some(shape)), true).expect_err("incomplete park");
            assert_eq!(e.code(), tonic::Code::InvalidArgument);
            assert_eq!(e.message(), "resume_incomplete");
        }
        // The committed truth table of the approval stages: both ids are still required.
        for mode in ["segment", "loop"] {
            assert!(enforce_resume(&with_resume(Some(resume(mode))), true).is_ok(), "{mode}");
            let mut no_approval = resume(mode);
            no_approval.approval_id.clear();
            let mut no_claim = resume(mode);
            no_claim.claim_token.clear();
            for shape in [no_approval, no_claim] {
                let e = enforce_resume(&with_resume(Some(shape)), true).expect_err(mode);
                assert_eq!(e.message(), "resume_incomplete");
            }
        }
    }

    #[test]
    fn the_same_context_on_a_persistent_run_and_an_absent_context_pass() {
        assert!(enforce_run_context(&request_with(Some(history()), false)).is_ok());
        assert!(enforce_run_context(&request_with(None, true)).is_ok());
        assert!(enforce_run_context(&request_with(None, false)).is_ok());
    }
}

#[cfg(test)]
mod step_attachment_tests {
    use super::*;
    use mp_contracts::attachments::sha256_hex;

    // The registry behind register_step_attachments is process-global and the
    // tests run in parallel, so each test owns a run id: sharing one key makes
    // the "nothing held" assertions race the test that holds a list.
    const ORG: &str = "org-attachment-test";

    fn step(
        tool: &str,
        run: &str,
        attachments: Vec<pb::RunAttachment>,
    ) -> pb::ExecuteStepRequest {
        pb::ExecuteStepRequest {
            run_id: run.to_owned(),
            org_id: ORG.to_owned(),
            tool_name: tool.to_owned(),
            attachments,
            ..Default::default()
        }
    }

    fn file(name: &str, content: &[u8]) -> pb::RunAttachment {
        pb::RunAttachment {
            id: name.to_owned(),
            name: name.to_owned(),
            mime: "text/csv".to_owned(),
            size_bytes: content.len() as u64,
            sha256: sha256_hex(content),
            content: content.to_vec(),
            kind: pb::RunAttachmentKind::File as i32,
        }
    }

    #[test]
    fn a_step_without_attachments_registers_nothing() {
        for zdr in [false, true] {
            let held = register_step_attachments(&step("code_interpreter", "run-none", Vec::new()), zdr)
                .expect("no list, no refusal");
            assert!(held.is_none());
        }
    }

    #[test]
    fn a_zero_retention_or_waived_step_refuses_the_files_at_the_boundary() {
        let request = step("code_interpreter", "run-refused", vec![file("a.csv", b"1,2")]);
        let refused = register_step_attachments(&request, true).expect_err("ZDR refuses");
        assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
        assert!(
            crate::run_inputs::global()
                .get(ORG, "run-refused")
                .is_none(),
            "nothing may be held after a refusal"
        );
    }

    #[test]
    fn only_code_interpreter_takes_files() {
        let request = step("web_search", "run-wrong-tool", vec![file("a.csv", b"1,2")]);
        let refused = register_step_attachments(&request, false).expect_err("wrong tool");
        assert_eq!(refused.code(), tonic::Code::InvalidArgument);
        assert_eq!(refused.message(), "attachments_unsupported_tool");
    }

    #[test]
    fn a_list_the_contract_refuses_is_never_held() {
        let mut tampered = file("a.csv", b"1,2");
        tampered.content = b"9,9".to_vec();
        let refused = register_step_attachments(
            &step("code_interpreter", "run-tampered", vec![tampered]),
            false,
        )
        .expect_err("a hash mismatch");
        assert_eq!(refused.code(), tonic::Code::InvalidArgument);
        assert_eq!(refused.message(), "attachments_hash_mismatch");
        let unsafe_name = file("../a.csv", b"1,2");
        let refused =
            register_step_attachments(
                &step("code_interpreter", "run-unsafe-name", vec![unsafe_name]),
                false,
            )
            .expect_err("an unsafe name");
        assert_eq!(refused.message(), "attachments_name_invalid");
    }

    #[test]
    fn a_valid_list_is_held_for_the_call_and_released_with_the_guard() {
        let request = step("code_interpreter", "run-held", vec![file("a.csv", b"1,2")]);
        let held = register_step_attachments(&request, false)
            .expect("registers")
            .expect("a guard");
        assert!(crate::run_inputs::global().get(ORG, "run-held").is_some());
        drop(held);
        assert!(crate::run_inputs::global().get(ORG, "run-held").is_none());
    }
}

#[cfg(test)]
mod run_attachment_tests {
    use super::*;
    use mp_contracts::attachments::sha256_hex;

    fn attachment(name: &str, content: &[u8], kind: pb::RunAttachmentKind, mime: &str) -> pb::RunAttachment {
        pb::RunAttachment {
            id: name.to_owned(),
            name: name.to_owned(),
            mime: mime.to_owned(),
            size_bytes: content.len() as u64,
            sha256: sha256_hex(content),
            content: content.to_vec(),
            kind: kind as i32,
        }
    }

    fn file(name: &str, content: &[u8]) -> pb::RunAttachment {
        attachment(name, content, pb::RunAttachmentKind::File, "text/csv")
    }

    fn image(name: &str, content: &[u8]) -> pb::RunAttachment {
        attachment(name, content, pb::RunAttachmentKind::Image, "image/png")
    }

    fn run_with(attachments: Vec<pb::RunAttachment>) -> pb::RunAgentRequest {
        pb::RunAgentRequest {
            run_id: "run-attachments".to_owned(),
            attachments,
            ..Default::default()
        }
    }

    #[test]
    fn a_run_without_attachments_passes_even_when_persistence_free() {
        for zdr in [false, true] {
            let mut req = run_with(Vec::new());
            assert!(enforce_run_attachments(&mut req, zdr).is_ok());
            assert!(req.attachments.is_empty());
        }
    }

    #[test]
    fn a_do_run_with_attachments_under_zdr_is_refused_at_the_boundary() {
        for list in [
            vec![file("sales.csv", b"a,b")],
            vec![image("shot.png", b"png")],
            vec![image("shot.png", b"png"), file("sales.csv", b"a,b")],
        ] {
            let mut req = run_with(list);
            let refused = enforce_run_attachments(&mut req, true).expect_err("zero retention");
            assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
            assert!(
                !refused.message().contains("sales") && !refused.message().contains("shot"),
                "the refusal carries no client text: {}",
                refused.message()
            );
        }
    }

    #[test]
    fn files_and_images_pass_and_files_come_first() {
        let mut req = run_with(vec![
            image("one.png", b"1"),
            file("sales.csv", b"a,b"),
            image("two.png", b"2"),
        ]);
        enforce_run_attachments(&mut req, false).expect("a good list");
        let order: Vec<&str> = req.attachments.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(order, ["sales.csv", "one.png", "two.png"]);
    }

    #[test]
    fn a_bad_file_or_image_refuses_the_whole_list_with_its_closed_token() {
        let mut tampered = image("shot.png", b"png");
        tampered.content = b"gnp".to_vec();
        let mut wrong_size = image("shot.png", b"png");
        wrong_size.size_bytes = 1;
        let mut svg = image("shot.svg", b"<svg/>");
        svg.mime = "image/svg+xml".to_owned();
        let cases = [
            (vec![tampered], "attachments_hash_mismatch"),
            (vec![wrong_size], "attachments_size_mismatch"),
            (vec![svg], "attachments_kind_unsupported"),
            (vec![image("empty.png", b"")], "attachments_empty"),
            (
                (0..=MAX_RUN_IMAGES)
                    .map(|index| image(&format!("{index}.png"), b"x"))
                    .collect(),
                "attachments_too_many",
            ),
            (vec![file("../a.csv", b"1,2")], "attachments_name_invalid"),
            (
                vec![attachment("a", b"x", pb::RunAttachmentKind::Unspecified, "text/csv")],
                "attachments_kind_unsupported",
            ),
        ];
        for (list, token) in cases {
            let mut req = run_with(list);
            let refused = enforce_run_attachments(&mut req, false).expect_err(token);
            assert_eq!(refused.code(), tonic::Code::InvalidArgument, "{token}");
            assert_eq!(refused.message(), token);
        }
    }

    #[test]
    fn thread_scope_demands_the_sandbox_bearer_only_when_enabled() {
        // With the switch off no step takes the thread path, so none demands the bearer, and each
        // request is handled as it was before the switch existed.
        assert_eq!(
            super::thread_workspace_thread(false, "code_interpreter", "", "thread_01", false),
            None
        );
        assert_eq!(
            super::thread_workspace_thread(true, "code_interpreter", "", "thread_01", false),
            Some("thread_01")
        );
        // Only the code interpreter takes the thread path, and only a step that names a thread.
        assert_eq!(
            super::thread_workspace_thread(true, "web_search", "", "thread_01", false),
            None
        );
        assert_eq!(
            super::thread_workspace_thread(true, "code_interpreter", "", "", false),
            None
        );
    }

    #[test]
    fn space_scope_never_takes_the_thread_path() {
        assert_eq!(
            super::thread_workspace_thread(true, "code_interpreter", "space_01", "thread_01", false),
            None
        );
    }

    #[tokio::test]
    async fn thread_workspace_refused_under_zdr() {
        // A zero-retention or waived step never takes the thread path, whatever the switch says.
        assert_eq!(
            super::thread_workspace_thread(true, "code_interpreter", "", "thread_01", true),
            None
        );
        // The lease guard refuses it with the typed token before any RPC. The endpoint is
        // unroutable, so reaching the wire would fail with a different status.
        let state = crate::state::StateStore::new();
        let client = crate::sandbox_manager_client::SandboxManagerClient::new(
            tonic::transport::Endpoint::from_shared("http://127.0.0.1:1")
                .expect("valid endpoint")
                .connect_lazy(),
        );
        let status = crate::sandbox_lease::ensure_thread_lease(
            &client,
            "bearer",
            &state,
            "run_01",
            "org_01",
            "thread_01",
            true,
        )
        .await
        .expect_err("a zero-retention thread lease is refused");
        let refusal = mp_contracts::zdr_refusal::Refusal::from_status(&status)
            .expect("the refusal carries its catalogue kind");
        assert_eq!(
            refusal.code(),
            mp_contracts::zdr_refusal::code::FEATURE_NOT_SUPPORTED
        );
        assert_eq!(refusal.feature(), Some("sandbox_workspace"));
    }
}
