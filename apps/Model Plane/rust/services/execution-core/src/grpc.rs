//! gRPC server implementing `ExecutionCore` on :9093.

use std::sync::Arc;
use std::time::Duration;

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
    /// This instance's own stable identifier — see
    /// `http_health::resolve_backend_id`'s doc for why the SAME value must be
    /// presented on every request from this process.
    backend_id: String,
    /// Mints execution-core's own `sandbox:write` service credential for
    /// `ReleaseLease` at a run's actual end (`cancel_run`, `run_agent`'s own
    /// `finalize()`) — never for `AcquireLease`, which needs the delegated
    /// user-bound bearer instead. See `sandbox_lease`'s module doc.
    sandbox_tokens: crate::sandbox_lease::SandboxManagerTokenProvider,
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
        backend_id: String,
        sandbox_tokens: crate::sandbox_lease::SandboxManagerTokenProvider,
    ) -> Self {
        let ownership = Arc::new(SessionCoreRunOwnershipResolver {
            channel: session_channel.clone(),
        });
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
            backend_id,
            sandbox_tokens,
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
            }));
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
                Ok(Response::new(pb::ExecuteScheduledStepResponse {
                    step_id: req.step_id,
                    status: receipt.status,
                    receipt_id: receipt.receipt_id,
                    output,
                    error: String::new(),
                    unknown_outcome: false,
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
                }))
            }
        }
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

fn scheduled_inference_request(
    req: &pb::ExecuteScheduledStepRequest,
    goal: &str,
) -> pb::InferRequest {
    pb::InferRequest {
        // Keep provider correlation in the same tenant/run/step namespace as
        // the Session receipt. A bare step id can collide across runs or
        // tenants, which makes an ambiguous provider response impossible to
        // reconcile safely.
        request_id: format!("scheduled:{}:{}", req.org_id, req.idempotency_key),
        org_id: req.org_id.clone(),
        model: "verevon-balance".to_owned(),
        messages: vec![
            pb::ChatMessage {
                role: "system".to_owned(),
                content:
                    "Execute one bounded scheduled step. Do not call tools or external effects."
                        .to_owned(),
                name: String::new(),
            },
            pb::ChatMessage {
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

#[allow(clippy::result_large_err)]
fn enforce_persistence_free_execution(
    caller: &AuthenticatedUser,
    request_zdr: bool,
    operation: &'static str,
) -> Result<(), Status> {
    if caller.zdr || request_zdr {
        return Err(Status::failed_precondition(format!(
            "ZDR {operation} is disabled until its lifecycle is persistence-free"
        )));
    }
    Ok(())
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
/// naming a tool directly to bypass either control.
fn reject_direct_owner_action(tool_name: &str) -> Result<(), Status> {
    if crate::permission::requires_durable_owner_approval(tool_name) {
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
        return Err(Status::failed_precondition(
            "scheduled-step execution requires persistent service retention",
        ));
    }
    Ok(())
}

#[tonic::async_trait]
impl ExecutionCore for ExecutionService {
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
            "browser_agent" | "browser.observe" | "browser.act"
        ) {
            Some(
                self.auth
                    .authenticate_delegated_browser(&request, &caller)
                    .await?,
            )
        } else {
            None
        };
        // Demanded only for a Space-scoped code_interpreter step — every
        // other step, and a non-Space code_interpreter step, is unaffected.
        // sandbox-manager's own AcquireLease binds a Space capability
        // decision to the CALLING principal, so a service-level credential
        // could never pass it; this is the one delegated bearer that must be
        // user-bound (S3_3_DURABLE_WORKSPACE_DESIGN_2026-09-11.md §3.5 B.2).
        let sandbox_bearer = if request.get_ref().tool_name == "code_interpreter"
            && !request.get_ref().space_id.is_empty()
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
        reject_direct_owner_action(&req.tool_name)?;
        validate_direct_permission_mode(&req.permission_mode)?;
        enforce_persistence_free_execution(&caller, req.zdr, "tool execution")?;
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
        .with_verified_bearer(session_bearer.as_str());

        let sandbox = sandbox_bearer.as_ref().map(|sandbox_bearer| {
            crate::sandbox_lease::SandboxLeaseContext {
                space_id: &req.space_id,
                sandbox_bearer: sandbox_bearer.as_str(),
                capability_client: self.capability_client.as_ref(),
                sandbox_manager_client: &self.sandbox_manager_client,
                backend_id: &self.backend_id,
            }
        });

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
            req.min_privacy_tier,
            Some(data_plane_bearer.as_str()),
            Some(session_bearer.as_str()),
            Some(inference_bearer.as_str()),
            self.capability_policy.as_ref(),
            browser_grant.as_ref(),
            sandbox.as_ref(),
        )
        .await;

        // HITL enforcement: a pause is durable only after session-core accepts
        // the Approval. On persistence failure, fail the RPC before checkpoint
        // or StateStore writes can expose a fictional AwaitingApproval state.
        if outcome.status == "awaiting_approval" {
            create_durable_approval(
                &self.session_channel,
                pb::CreateApprovalRequest {
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
        enforce_persistence_free_execution(&caller, false, "run control")?;
        let req = request.into_inner();
        caller.authorize(&req.org_id, None)?;
        self.authorize_run(&caller, &req.run_id, &session_bearer)
            .await?;
        let resumed_step = if req.approval_id.trim().is_empty() {
            self.state.resume_paused(&req.run_id)
        } else {
            let approval = self
                .orchestration_client()
                .get_approval(authenticated_session_request(
                    pb::GetApprovalRequest {
                        approval_id: req.approval_id.clone(),
                        org_id: req.org_id.clone(),
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
        enforce_persistence_free_execution(&caller, false, "run control")?;
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
        enforce_persistence_free_execution(&caller, false, "run control")?;
        let req = request.into_inner();
        caller.authorize(&req.org_id, None)?;
        self.authorize_run(&caller, &req.run_id, &session_bearer)
            .await?;
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
        let sandbox_bearer = if request.get_ref().space_id.is_empty() {
            None
        } else {
            Some(
                self.auth
                    .authenticate_delegated_sandbox_manager(&request, &caller)
                    .await?,
            )
        };
        let req = request.into_inner();
        caller.authorize(&req.org_id, Some(&req.user_id))?;
        self.authorize_run(&caller, &req.run_id, &session_bearer)
            .await?;
        enforce_persistence_free_execution(&caller, req.zdr, "agent run")?;
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
            req,
            Some(data_plane_bearer.as_str().to_owned()),
            Some(session_bearer.as_str().to_owned()),
            inference_bearer.as_str().to_owned(),
            self.capability_policy.as_ref(),
            self.terminal_tokens.as_ref(),
            sandbox_bearer
                .as_ref()
                .map(|bearer| bearer.as_str().to_owned()),
            self.capability_client.as_ref(),
            &self.sandbox_manager_client,
            &self.backend_id,
            &self.sandbox_tokens,
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
    let capability_channel =
        tonic::transport::Endpoint::from_shared(capability_url)?.connect_lazy();
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
    let backend_id = crate::http_health::resolve_backend_id();
    // Same deployment service principal capability_policy's own
    // ServiceTokenProvider already requires (EXECUTION_CORE_SERVICE_ID/
    // EXECUTION_CORE_SERVICE_API_KEY/AUTH_CORE_URL) — construction above
    // already fails startup closed without them, so requiring them again
    // here for a different audience changes nothing about what a deployment
    // must configure.
    let sandbox_tokens =
        crate::sandbox_lease::SandboxManagerTokenProvider::from_env().map_err(|error| {
            anyhow::anyhow!("sandbox-manager service token provider configuration: {error}")
        })?;

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
        backend_id,
        sandbox_tokens,
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
    backend_id: String,
    sandbox_tokens: crate::sandbox_lease::SandboxManagerTokenProvider,
) -> anyhow::Result<()> {
    let session_channel = tonic::transport::Endpoint::from_shared(session_url)?.connect_lazy();
    let inference_channel = tonic::transport::Endpoint::from_shared(inference_url)?.connect_lazy();
    let browser_channel = tonic::transport::Endpoint::from_shared(browser_url)?.connect_lazy();

    readiness.set_grpc_ready(true);
    let _readiness_guard = ReadinessGuard(readiness);
    info!("gRPC listening on :9093");

    tonic::transport::Server::builder()
        .add_service(ExecutionCoreServer::new(
            ExecutionService::new_with_scheduled_step_runtime(
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
                backend_id,
                sandbox_tokens,
            ),
        ))
        .serve_with_incoming(TcpListenerStream::new(listener))
        .await?;

    Ok(())
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
        let error = reject_direct_owner_action(crate::ticket_tools::TOOL_NAME)
            .expect_err("a public direct-step RPC cannot execute an owner action");
        assert_eq!(error.code(), tonic::Code::PermissionDenied);
        assert_eq!(
            reject_direct_owner_action("  tickets.create  ")
                .expect_err("whitespace does not turn a reserved action into a direct tool")
                .code(),
            tonic::Code::PermissionDenied
        );
        reject_direct_owner_action("shell").expect("ordinary tools keep the direct API");

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
        for operation in ["tool execution", "run control"] {
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
        let channel = tonic::transport::Endpoint::from_shared(format!("http://{address}"))
            .expect("endpoint")
            .connect_lazy();
        let state = StateStore::new();

        let error = tokio::time::timeout(
            std::time::Duration::from_secs(2),
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
            "test-backend".to_owned(),
            crate::sandbox_lease::SandboxManagerTokenProvider::new_for_test(
                "http://127.0.0.1:1",
                "execution-core",
                "test-service-secret-at-least-32-bytes",
            ),
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
