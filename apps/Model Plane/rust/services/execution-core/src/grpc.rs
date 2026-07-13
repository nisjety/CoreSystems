//! gRPC server implementing `ExecutionCore` on :9093.

use std::sync::Arc;

use mp_contracts::model_plane::v1::{
    self as pb,
    execution_core_server::{ExecutionCore, ExecutionCoreServer},
    orchestration_core_service_client::OrchestrationCoreServiceClient,
    run_service_client::RunServiceClient,
    session_core_client::SessionCoreClient,
};
use mp_ids::new_ulid;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{Request, Response, Status};
use tracing::{info, warn};

use crate::auth::{
    delegated_session_bearer, AuthenticatedUser, DelegatedSessionBearer, JwtVerifier,
};
use crate::http_health::Readiness;
use crate::runtime_loop;
use crate::state::{RunSnapshot, RunStatus, StateStore};

pub(crate) struct ExecutionService {
    state: StateStore,
    auth: JwtVerifier,
    session_channel: tonic::transport::Channel,
    inference_channel: tonic::transport::Channel,
    ownership: Arc<dyn RunOwnershipResolver>,
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
    pub(crate) fn new(
        state: StateStore,
        auth: JwtVerifier,
        session_channel: tonic::transport::Channel,
        inference_channel: tonic::transport::Channel,
    ) -> Self {
        let ownership = Arc::new(SessionCoreRunOwnershipResolver {
            channel: session_channel.clone(),
        });
        Self {
            state,
            auth,
            session_channel,
            inference_channel,
            ownership,
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
        let req = request.into_inner();
        caller.authorize(&req.org_id, Some(&req.user_id))?;
        self.authorize_run(&caller, &req.run_id, &session_bearer)
            .await?;
        if req.zdr {
            return Err(Status::failed_precondition(
                "ZDR tool execution is disabled until the step lifecycle is persistence-free",
            ));
        }
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

        let outcome = runtime_loop::execute_step(
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
            Some(self.session_channel.clone()),
            Some(&browser_sink),
            Some(&self.state),
            req.zdr,
            Some(data_plane_bearer.as_str()),
            Some(session_bearer.as_str()),
            Some(inference_bearer.as_str()),
        )
        .await;

        // HITL enforcement: when the posture gated this step, create the
        // durable Approval and pause the run. session-core broadcasts
        // RUN_PAUSED_FOR_APPROVAL so operator surfaces show the pause. Best
        // effort — the step is already paused regardless of the record write.
        if outcome.status == "awaiting_approval" {
            let mut orchestration = self.orchestration_client();
            if let Err(error) = orchestration
                .create_approval(authenticated_session_request(
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
                    },
                    session_bearer.as_str(),
                )?)
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
        let req = request.into_inner();
        caller.authorize(&req.org_id, None)?;
        self.authorize_run(&caller, &req.run_id, &session_bearer)
            .await?;
        let resumed_step = self.state.resume(&req.run_id);
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
                "resume_run rejected: run is not AwaitingApproval or Paused"
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
        let req = request.into_inner();
        self.authorize_run(&caller, &req.run_id, &session_bearer)
            .await?;
        let cancelled = self.state.cancel(&req.run_id, Some(req.reason));

        Ok(Response::new(pb::CancelRunResponse { cancelled }))
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
        let req = request.into_inner();
        caller.authorize(&req.org_id, None)?;
        self.authorize_run(&caller, &req.run_id, &session_bearer)
            .await?;
        let paused = self.state.pause(&req.run_id);
        info!(run_id = %req.run_id, "pause_run: run state set to Paused");

        Ok(Response::new(pb::PauseRunResponse { paused }))
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
        let req = request.into_inner();
        caller.authorize(&req.org_id, Some(&req.user_id))?;
        self.authorize_run(&caller, &req.run_id, &session_bearer)
            .await?;
        if req.zdr {
            return Err(Status::failed_precondition(
                "ZDR agent runs are disabled until the full run lifecycle is persistence-free",
            ));
        }
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
            Some(data_plane_bearer.as_str().to_owned()),
            Some(session_bearer.as_str().to_owned()),
            inference_bearer.as_str().to_owned(),
        )
        .await;
        Ok(Response::new(response))
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

    serve_with_listener(state, readiness, auth, listener, session_url, inference_url).await
}

async fn serve_with_listener(
    state: StateStore,
    readiness: Readiness,
    auth: JwtVerifier,
    listener: tokio::net::TcpListener,
    session_url: String,
    inference_url: String,
) -> anyhow::Result<()> {
    let session_channel = tonic::transport::Endpoint::from_shared(session_url)?.connect_lazy();
    let inference_channel = tonic::transport::Endpoint::from_shared(inference_url)?.connect_lazy();

    readiness.set_grpc_ready(true);
    let _readiness_guard = ReadinessGuard(readiness);
    info!("gRPC listening on :9093");

    tonic::transport::Server::builder()
        .add_service(ExecutionCoreServer::new(ExecutionService::new(
            state,
            auth,
            session_channel,
            inference_channel,
        )))
        .serve_with_incoming(TcpListenerStream::new(listener))
        .await?;

    Ok(())
}

#[cfg(test)]
mod auth_tests {
    use super::*;
    use mp_contracts::model_plane::v1::execution_core_client::ExecutionCoreClient;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn session_bearer() -> crate::auth::DelegatedSessionBearer {
        crate::auth::DelegatedSessionBearer::for_test()
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
