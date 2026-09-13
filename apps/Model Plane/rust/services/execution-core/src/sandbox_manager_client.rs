//! gRPC client for sandbox-manager's `SandboxManager` service —
//! `AcquireLease`, `ActivateLease`, `SnapshotSandbox`, `ReleaseLease`,
//! `GetWorkspaceManifest`, `PromoteWorkspace`, `Health`, and the S4.2
//! background process registry (`RegisterProcess`, `UpdateProcessState`,
//! `AppendProcessOutput`, `ReconcileProcesses`, `GetProcess`,
//! `ListProcesses`, `ReadProcessOutput`).
//!
//! Real and independently callable, but **not yet wired into a production
//! caller** — see
//! `apps/Frontend Plane/verevonv3/docs/S3_3_DURABLE_WORKSPACE_DESIGN_2026-09-11.md`
//! §3.5 phase B for why, in detail. Short version: sandbox-manager's own
//! `AcquireLease` (`internal/server/server.go`) requires, for any Space-scoped
//! request, that the CALLING principal's own `ActorID` equal the Space
//! capability decision's `SubjectID`
//! (`internal/authz/capability_verifier.go`'s `Verify`). Control's
//! decision-issuance endpoint
//! (`user-core/internal/http/spaces.go`'s sandbox-capability-decision
//! handler) only signs a decision for an actual current Space member —
//! `ResolvePersonalThreadDecisionEvidence` fails closed with
//! `ErrNoCurrentMembership` for anyone else, a service account included. So
//! this client cannot mint its own service-level org token the way
//! `capability_policy.rs`'s `ServiceTokenProvider` does for capability-core:
//! it would never pass that membership check. It needs a delegated,
//! user-bound sandbox-manager bearer instead — the same shape session-core /
//! inference-core / browser-broker / data-plane calls already receive (see
//! `auth.rs`'s `authenticate_delegated_*` family) — and no such bearer exists
//! on the wire today; `ExecuteStepRequest` carries a `space_id` (phase A) but
//! no sandbox-manager credential. Minting and forwarding one is its own
//! cross-service design question (touches Auth Core's token issuance and
//! whatever resolved model-gateway's `ThreadSpaceContext` in the first
//! place), not a mechanical continuation of this file. Hence every method
//! here takes `bearer` as a plain caller-supplied argument rather than
//! resolving one itself: building the client did not require answering that
//! question, and guessing an answer risked shipping a caller that always
//! fails `ErrNoCurrentMembership` against a real, correctly signed decision.

use std::time::Duration;

use mp_contracts::model_plane::v1::{
    sandbox_manager_client::SandboxManagerClient as GeneratedClient, update_process_state_request,
    AcquireLeaseRequest, AcquireLeaseResponse, ActivateLeaseRequest, ActivateLeaseResponse,
    AppendProcessOutputRequest, AppendProcessOutputResponse, GetProcessRequest, GetProcessResponse,
    GetWorkspaceManifestRequest, GetWorkspaceManifestResponse, ListProcessesRequest,
    ListProcessesResponse, ProcessExited, ProcessOutputChunk, ProcessSignal,
    ProcessSignalRequested, ProcessStarted, ProcessState, PromoteWorkspaceRequest,
    PromoteWorkspaceResponse, ReadProcessOutputRequest, ReadProcessOutputResponse,
    ReconcileProcessesRequest, ReconcileProcessesResponse, RedactedCommand, RegisterProcessRequest,
    RegisterProcessResponse, ReleaseLeaseRequest, ReleaseLeaseResponse, SandboxHealthRequest,
    SandboxHealthResponse, SnapshotRequest, SnapshotResponse, UpdateProcessStateRequest,
    WorkspaceChangedFile,
};
use tonic::{transport::Channel, Request, Status};

const RPC_TIMEOUT: Duration = Duration::from_secs(10);

/// One lease request. `capability_decision`/`capability_claims_json` are
/// required by sandbox-manager whenever `space_id` is non-empty (see
/// `AcquireLeaseRequest`'s own field docs in `sandboxes.proto`); leave both
/// empty only for the legacy non-Space thread/agent lease path.
pub struct LeaseRequest<'a> {
    pub scope_id: &'a str,
    pub scope_type: &'a str,
    pub ttl: Duration,
    pub org_id: &'a str,
    pub space_id: &'a str,
    pub capability_decision: &'a str,
    pub capability_claims_json: &'a str,
}

/// Identifies both a process and the exact host allowed to write it.
///
/// sandbox-manager folds these three into the predicate of every statement
/// a host's report runs, so a superseded boot lands nothing rather than
/// interleaving into rows a newer one now owns.
pub struct ProcessFence<'a> {
    pub process_id: &'a str,
    pub backend_id: &'a str,
    pub host_epoch: &'a str,
}

impl ProcessFence<'_> {
    fn validate(&self) -> Result<(), Status> {
        if self.process_id.trim().is_empty() {
            return Err(Status::invalid_argument("process_id is required"));
        }
        if self.backend_id.trim().is_empty() || self.host_epoch.trim().is_empty() {
            return Err(Status::invalid_argument(
                "backend_id and host_epoch are required",
            ));
        }
        Ok(())
    }
}

/// A command's argv, ALREADY REDACTED. The registry must never receive the
/// plaintext; `command_digest` on [`ProcessRegistration`] is what lets a
/// caller recognize the same command without it.
pub struct RedactedArgv<'a> {
    pub program: &'a str,
    pub args: &'a [&'a str],
}

/// One process registration. There is deliberately no Space: sandbox-manager
/// reads it from the lease.
pub struct ProcessRegistration<'a> {
    pub process_id: &'a str,
    pub lease_id: &'a str,
    pub backend_id: &'a str,
    pub host_epoch: &'a str,
    pub run_id: &'a str,
    pub step_id: &'a str,
    pub subject_id: &'a str,
    pub command: RedactedArgv<'a>,
    pub command_digest: &'a str,
    pub ttl_seconds: i32,
}

/// How a process ended.
pub struct ProcessOutcome<'a> {
    pub state: ProcessState,
    pub exit_code: Option<i32>,
    pub end_reason: &'a str,
    /// Whether the host finished tearing down the process's pipes and
    /// scratch directory.
    pub cleanup_done: bool,
}

#[derive(Clone)]
pub struct SandboxManagerClient {
    channel: Channel,
}

impl SandboxManagerClient {
    #[must_use]
    pub fn new(channel: Channel) -> Self {
        Self { channel }
    }

    /// Builds from `SANDBOX_MANAGER_URL`/`SANDBOX_MANAGER_ADDR`, mirroring
    /// `grpc.rs::serve`'s session/inference/browser channel resolution.
    /// Connects lazily: a down or misconfigured sandbox-manager only fails
    /// the first real RPC, never startup.
    ///
    /// # Errors
    /// Returns an error only when the resolved URL is not a valid endpoint.
    pub fn from_env() -> Result<Self, String> {
        let url = std::env::var("SANDBOX_MANAGER_URL")
            .or_else(|_| std::env::var("SANDBOX_MANAGER_ADDR"))
            .unwrap_or_else(|_| "http://sandbox-manager:9094".to_owned());
        let channel = tonic::transport::Endpoint::from_shared(url)
            .map_err(|error| format!("sandbox-manager endpoint is invalid: {error}"))?
            .connect_lazy();
        Ok(Self::new(channel))
    }

    fn client(&self) -> GeneratedClient<Channel> {
        GeneratedClient::new(self.channel.clone())
    }

    fn authorize<T>(bearer: &str, message: T) -> Result<Request<T>, Status> {
        let mut request = Request::new(message);
        request.metadata_mut().insert(
            "authorization",
            format!("Bearer {bearer}")
                .parse()
                .map_err(|_| Status::internal("sandbox-manager bearer is not forwardable"))?,
        );
        Ok(request)
    }

    /// # Errors
    /// `invalid_argument` for a malformed request (including a Space-scoped
    /// request missing its capability decision); `deadline_exceeded` past
    /// [`RPC_TIMEOUT`]; otherwise sandbox-manager's own status, unchanged —
    /// callers depend on codes like `PermissionDenied` (capability
    /// verification failed) and `FailedPrecondition` (verification not
    /// configured on that instance) reaching them intact.
    pub async fn acquire_lease(
        &self,
        bearer: &str,
        request: &LeaseRequest<'_>,
    ) -> Result<AcquireLeaseResponse, Status> {
        if request.scope_id.trim().is_empty()
            || request.scope_type.trim().is_empty()
            || request.org_id.trim().is_empty()
        {
            return Err(Status::invalid_argument(
                "scope_id, scope_type, and org_id are required",
            ));
        }
        if request.ttl.is_zero() {
            return Err(Status::invalid_argument("ttl must be greater than zero"));
        }
        if !request.space_id.is_empty()
            && (request.capability_decision.is_empty() || request.capability_claims_json.is_empty())
        {
            return Err(Status::invalid_argument(
                "capability_decision and capability_claims_json are required for a Space-scoped lease",
            ));
        }
        let message = AcquireLeaseRequest {
            scope_id: request.scope_id.to_owned(),
            scope_type: request.scope_type.to_owned(),
            ttl: Some(prost_types::Duration {
                seconds: i64::try_from(request.ttl.as_secs()).unwrap_or(i64::MAX),
                nanos: i32::try_from(request.ttl.subsec_nanos()).unwrap_or_default(),
            }),
            org_id: request.org_id.to_owned(),
            space_id: request.space_id.to_owned(),
            capability_decision: request.capability_decision.to_owned(),
            capability_claims_json: request.capability_claims_json.to_owned(),
        };
        let wire = Self::authorize(bearer, message)?;
        let outcome = tokio::time::timeout(RPC_TIMEOUT, self.client().acquire_lease(wire))
            .await
            .map_err(|_| Status::deadline_exceeded("sandbox-manager AcquireLease timed out"))?;
        Ok(outcome?.into_inner())
    }

    /// # Errors
    /// `invalid_argument` when `lease_id` is blank; `deadline_exceeded` past
    /// [`RPC_TIMEOUT`]; otherwise sandbox-manager's own status.
    pub async fn activate_lease(
        &self,
        bearer: &str,
        lease_id: &str,
        backend_id: &str,
    ) -> Result<ActivateLeaseResponse, Status> {
        if lease_id.trim().is_empty() {
            return Err(Status::invalid_argument("lease_id is required"));
        }
        let message = ActivateLeaseRequest {
            lease_id: lease_id.to_owned(),
            backend_id: backend_id.to_owned(),
        };
        let wire = Self::authorize(bearer, message)?;
        let outcome = tokio::time::timeout(RPC_TIMEOUT, self.client().activate_lease(wire))
            .await
            .map_err(|_| Status::deadline_exceeded("sandbox-manager ActivateLease timed out"))?;
        Ok(outcome?.into_inner())
    }

    /// # Errors
    /// `invalid_argument` when `lease_id` or `label` is blank;
    /// `deadline_exceeded` past [`RPC_TIMEOUT`]; otherwise sandbox-manager's
    /// own status.
    pub async fn snapshot_sandbox(
        &self,
        bearer: &str,
        lease_id: &str,
        label: &str,
        backend_id: &str,
        changed_files: Vec<WorkspaceChangedFile>,
    ) -> Result<SnapshotResponse, Status> {
        if lease_id.trim().is_empty() || label.trim().is_empty() {
            return Err(Status::invalid_argument("lease_id and label are required"));
        }
        let message = SnapshotRequest {
            lease_id: lease_id.to_owned(),
            label: label.to_owned(),
            backend_id: backend_id.to_owned(),
            changed_files,
        };
        let wire = Self::authorize(bearer, message)?;
        let outcome = tokio::time::timeout(RPC_TIMEOUT, self.client().snapshot_sandbox(wire))
            .await
            .map_err(|_| Status::deadline_exceeded("sandbox-manager SnapshotSandbox timed out"))?;
        Ok(outcome?.into_inner())
    }

    /// Resolves the layered workspace manifest a Space-scoped lease sees —
    /// the Space's own durable files shadowed path-for-path by this lease's
    /// own not-yet-merged overlay. Empty `entries` for a non-Space lease.
    ///
    /// # Errors
    /// `invalid_argument` when `lease_id` is blank; `deadline_exceeded` past
    /// [`RPC_TIMEOUT`]; otherwise sandbox-manager's own status.
    pub async fn get_workspace_manifest(
        &self,
        bearer: &str,
        lease_id: &str,
        backend_id: &str,
    ) -> Result<GetWorkspaceManifestResponse, Status> {
        if lease_id.trim().is_empty() {
            return Err(Status::invalid_argument("lease_id is required"));
        }
        let message = GetWorkspaceManifestRequest {
            lease_id: lease_id.to_owned(),
            backend_id: backend_id.to_owned(),
        };
        let wire = Self::authorize(bearer, message)?;
        let outcome = tokio::time::timeout(RPC_TIMEOUT, self.client().get_workspace_manifest(wire))
            .await
            .map_err(|_| {
                Status::deadline_exceeded("sandbox-manager GetWorkspaceManifest timed out")
            })?;
        Ok(outcome?.into_inner())
    }

    /// Merges a Space-scoped lease's own workspace_files overlay into the
    /// Space's durable rows — S3.3 step 4, deliberately its own explicit
    /// step (see `sandboxes.proto`'s own doc comment on this RPC): never
    /// called automatically by `snapshot_sandbox` or `release_lease`, and
    /// no caller is wired here yet — this method exists so a future one
    /// (a Space UI action, or some other deliberate trigger) has a real
    /// primitive to call, the same "ship the primitive, wire the caller
    /// later" position every other method on this client was in before its
    /// own first real caller landed. Safe to call more than once for the
    /// same overlay.
    ///
    /// # Errors
    /// `invalid_argument` when `lease_id` is blank; `deadline_exceeded` past
    /// [`RPC_TIMEOUT`]; otherwise sandbox-manager's own status.
    pub async fn promote_workspace(
        &self,
        bearer: &str,
        lease_id: &str,
        backend_id: &str,
    ) -> Result<PromoteWorkspaceResponse, Status> {
        if lease_id.trim().is_empty() {
            return Err(Status::invalid_argument("lease_id is required"));
        }
        let message = PromoteWorkspaceRequest {
            lease_id: lease_id.to_owned(),
            backend_id: backend_id.to_owned(),
        };
        let wire = Self::authorize(bearer, message)?;
        let outcome = tokio::time::timeout(RPC_TIMEOUT, self.client().promote_workspace(wire))
            .await
            .map_err(|_| Status::deadline_exceeded("sandbox-manager PromoteWorkspace timed out"))?;
        Ok(outcome?.into_inner())
    }

    /// # Errors
    /// `invalid_argument` when `lease_id` is blank; `deadline_exceeded` past
    /// [`RPC_TIMEOUT`]; otherwise sandbox-manager's own status.
    pub async fn release_lease(
        &self,
        bearer: &str,
        lease_id: &str,
        backend_id: &str,
    ) -> Result<ReleaseLeaseResponse, Status> {
        if lease_id.trim().is_empty() {
            return Err(Status::invalid_argument("lease_id is required"));
        }
        let message = ReleaseLeaseRequest {
            lease_id: lease_id.to_owned(),
            backend_id: backend_id.to_owned(),
        };
        let wire = Self::authorize(bearer, message)?;
        let outcome = tokio::time::timeout(RPC_TIMEOUT, self.client().release_lease(wire))
            .await
            .map_err(|_| Status::deadline_exceeded("sandbox-manager ReleaseLease timed out"))?;
        Ok(outcome?.into_inner())
    }

    /// # Errors
    /// `deadline_exceeded` past [`RPC_TIMEOUT`]; otherwise sandbox-manager's
    /// own status. Note this is `SandboxManager`'s own `Health` RPC, not the
    /// standard `grpc.health.v1.Health` service — sandbox-manager's auth
    /// interceptor exempts only the latter, so this call still needs `bearer`.
    pub async fn health(&self, bearer: &str) -> Result<SandboxHealthResponse, Status> {
        let wire = Self::authorize(bearer, SandboxHealthRequest {})?;
        let outcome = tokio::time::timeout(RPC_TIMEOUT, self.client().health(wire))
            .await
            .map_err(|_| Status::deadline_exceeded("sandbox-manager Health timed out"))?;
        Ok(outcome?.into_inner())
    }

    /// Reserves a process row before the host spawns — S4.2 step 2.
    ///
    /// The Space is never named here: sandbox-manager reads it from the
    /// lease, inside the same statement that inserts, so a caller cannot
    /// name the wrong one. The organization comes from the verified bearer
    /// for the same reason.
    ///
    /// Like every other method on this client, and every other S4.2 slice,
    /// this ships as a callable primitive with no production caller: the
    /// host that will drive it is step 4.
    ///
    /// # Errors
    /// `invalid_argument` for a blank `process_id`/`lease_id`;
    /// `permission_denied` when the lease's Space was never granted
    /// `space:processes`; `failed_precondition` when the lease itself is
    /// ineligible; `resource_exhausted` at the live-process limit;
    /// `deadline_exceeded` past [`RPC_TIMEOUT`].
    pub async fn register_process(
        &self,
        bearer: &str,
        request: &ProcessRegistration<'_>,
    ) -> Result<RegisterProcessResponse, Status> {
        if request.process_id.trim().is_empty() {
            return Err(Status::invalid_argument("process_id is required"));
        }
        if request.lease_id.trim().is_empty() {
            return Err(Status::invalid_argument("lease_id is required"));
        }
        if request.ttl_seconds <= 0 {
            return Err(Status::invalid_argument("ttl_seconds must be positive"));
        }
        let message = RegisterProcessRequest {
            process_id: request.process_id.to_owned(),
            lease_id: request.lease_id.to_owned(),
            backend_id: request.backend_id.to_owned(),
            host_epoch: request.host_epoch.to_owned(),
            run_id: request.run_id.to_owned(),
            step_id: request.step_id.to_owned(),
            subject_id: request.subject_id.to_owned(),
            command: Some(RedactedCommand {
                program: request.command.program.to_owned(),
                args: request
                    .command
                    .args
                    .iter()
                    .map(|a| (*a).to_owned())
                    .collect(),
            }),
            command_digest: request.command_digest.to_owned(),
            ttl_seconds: request.ttl_seconds,
        };
        let wire = Self::authorize(bearer, message)?;
        let outcome = tokio::time::timeout(RPC_TIMEOUT, self.client().register_process(wire))
            .await
            .map_err(|_| Status::deadline_exceeded("sandbox-manager RegisterProcess timed out"))?;
        Ok(outcome?.into_inner())
    }

    /// Reports that the host spawned the process.
    ///
    /// # Errors
    /// `failed_precondition` when this host no longer owns the process (a
    /// newer boot took over, or it is already terminal) — the caller must
    /// stop writing and drop its handle rather than retry.
    pub async fn mark_process_started(
        &self,
        bearer: &str,
        fence: &ProcessFence<'_>,
    ) -> Result<(), Status> {
        self.update_process_state(
            bearer,
            fence,
            update_process_state_request::Transition::Started(ProcessStarted {}),
            "started",
        )
        .await
    }

    /// Records a request to stop the process. The host is what actually
    /// delivers the signal; an escalation to kill is never downgraded.
    ///
    /// # Errors
    /// As [`Self::mark_process_started`].
    pub async fn request_process_signal(
        &self,
        bearer: &str,
        fence: &ProcessFence<'_>,
        signal: ProcessSignal,
    ) -> Result<(), Status> {
        if signal == ProcessSignal::Unspecified {
            return Err(Status::invalid_argument("signal must be term or kill"));
        }
        self.update_process_state(
            bearer,
            fence,
            update_process_state_request::Transition::Signal(ProcessSignalRequested {
                signal: signal as i32,
            }),
            "signal",
        )
        .await
    }

    /// Reports a terminal outcome. The first terminal outcome wins: a late
    /// report for a process the registry already settled is an idempotent
    /// no-op, not an error.
    ///
    /// # Errors
    /// `invalid_argument` for a non-terminal state or a blank reason;
    /// otherwise as [`Self::mark_process_started`].
    pub async fn mark_process_ended(
        &self,
        bearer: &str,
        fence: &ProcessFence<'_>,
        outcome: &ProcessOutcome<'_>,
    ) -> Result<(), Status> {
        if !matches!(
            outcome.state,
            ProcessState::Exited | ProcessState::Killed | ProcessState::Expired
        ) {
            return Err(Status::invalid_argument(
                "an exit must report a terminal state",
            ));
        }
        if outcome.end_reason.trim().is_empty() {
            return Err(Status::invalid_argument("end_reason is required"));
        }
        self.update_process_state(
            bearer,
            fence,
            update_process_state_request::Transition::Exited(ProcessExited {
                state: outcome.state as i32,
                exit_code: outcome.exit_code,
                end_reason: outcome.end_reason.to_owned(),
                cleanup_done: outcome.cleanup_done,
            }),
            "exited",
        )
        .await
    }

    async fn update_process_state(
        &self,
        bearer: &str,
        fence: &ProcessFence<'_>,
        transition: update_process_state_request::Transition,
        label: &'static str,
    ) -> Result<(), Status> {
        fence.validate()?;
        let message = UpdateProcessStateRequest {
            process_id: fence.process_id.to_owned(),
            backend_id: fence.backend_id.to_owned(),
            host_epoch: fence.host_epoch.to_owned(),
            transition: Some(transition),
        };
        let wire = Self::authorize(bearer, message)?;
        let outcome = tokio::time::timeout(RPC_TIMEOUT, self.client().update_process_state(wire))
            .await
            .map_err(|_| {
                Status::deadline_exceeded(format!(
                    "sandbox-manager UpdateProcessState({label}) timed out"
                ))
            })?;
        outcome?;
        Ok(())
    }

    /// Appends output and refreshes the process's heartbeat. An empty
    /// `chunks` slice is a pure heartbeat, which is what keeps silence
    /// distinguishable from a dead host — so this is safe, and expected, to
    /// call with nothing to say.
    ///
    /// Re-sending a batch after an ambiguous failure is a no-op: the
    /// registry keys output on `(process_id, seq)`, and the host owns the
    /// seq counter.
    ///
    /// # Errors
    /// As [`Self::mark_process_started`].
    pub async fn append_process_output(
        &self,
        bearer: &str,
        fence: &ProcessFence<'_>,
        chunks: Vec<ProcessOutputChunk>,
        stdin_bytes_delta: i64,
    ) -> Result<AppendProcessOutputResponse, Status> {
        fence.validate()?;
        if stdin_bytes_delta < 0 {
            return Err(Status::invalid_argument(
                "stdin_bytes_delta must not be negative",
            ));
        }
        let message = AppendProcessOutputRequest {
            process_id: fence.process_id.to_owned(),
            backend_id: fence.backend_id.to_owned(),
            host_epoch: fence.host_epoch.to_owned(),
            chunks,
            stdin_bytes_delta,
        };
        let wire = Self::authorize(bearer, message)?;
        let outcome = tokio::time::timeout(RPC_TIMEOUT, self.client().append_process_output(wire))
            .await
            .map_err(|_| {
                Status::deadline_exceeded("sandbox-manager AppendProcessOutput timed out")
            })?;
        Ok(outcome?.into_inner())
    }

    /// Declares every live process this backend owns under a DIFFERENT host
    /// epoch lost. A restarted host calls this once before serving: its
    /// predecessor's children died with it, so the registry must say so
    /// rather than leave rows claiming to be running.
    ///
    /// # Errors
    /// `invalid_argument` for a blank backend id or epoch;
    /// `deadline_exceeded` past [`RPC_TIMEOUT`].
    pub async fn reconcile_processes(
        &self,
        bearer: &str,
        backend_id: &str,
        host_epoch: &str,
    ) -> Result<ReconcileProcessesResponse, Status> {
        if backend_id.trim().is_empty() || host_epoch.trim().is_empty() {
            return Err(Status::invalid_argument(
                "backend_id and host_epoch are required",
            ));
        }
        let message = ReconcileProcessesRequest {
            backend_id: backend_id.to_owned(),
            host_epoch: host_epoch.to_owned(),
        };
        let wire = Self::authorize(bearer, message)?;
        let outcome = tokio::time::timeout(RPC_TIMEOUT, self.client().reconcile_processes(wire))
            .await
            .map_err(|_| {
                Status::deadline_exceeded("sandbox-manager ReconcileProcesses timed out")
            })?;
        Ok(outcome?.into_inner())
    }

    /// Reads one process's metadata. Descriptive: a terminal process's
    /// record stays readable.
    ///
    /// # Errors
    /// `invalid_argument` for a blank id; `not_found` outside the bearer's
    /// own organization; `deadline_exceeded` past [`RPC_TIMEOUT`].
    pub async fn get_process(
        &self,
        bearer: &str,
        process_id: &str,
    ) -> Result<GetProcessResponse, Status> {
        if process_id.trim().is_empty() {
            return Err(Status::invalid_argument("process_id is required"));
        }
        let message = GetProcessRequest {
            process_id: process_id.to_owned(),
        };
        let wire = Self::authorize(bearer, message)?;
        let outcome = tokio::time::timeout(RPC_TIMEOUT, self.client().get_process(wire))
            .await
            .map_err(|_| Status::deadline_exceeded("sandbox-manager GetProcess timed out"))?;
        Ok(outcome?.into_inner())
    }

    /// Pages a Space's processes, newest first.
    ///
    /// # Errors
    /// `invalid_argument` for a blank `space_id`; `deadline_exceeded` past
    /// [`RPC_TIMEOUT`].
    pub async fn list_processes(
        &self,
        bearer: &str,
        space_id: &str,
        include_terminal: bool,
        limit: i32,
        after_process_id: &str,
    ) -> Result<ListProcessesResponse, Status> {
        if space_id.trim().is_empty() {
            return Err(Status::invalid_argument("space_id is required"));
        }
        let message = ListProcessesRequest {
            space_id: space_id.to_owned(),
            include_terminal,
            limit,
            after_process_id: after_process_id.to_owned(),
        };
        let wire = Self::authorize(bearer, message)?;
        let outcome = tokio::time::timeout(RPC_TIMEOUT, self.client().list_processes(wire))
            .await
            .map_err(|_| Status::deadline_exceeded("sandbox-manager ListProcesses timed out"))?;
        Ok(outcome?.into_inner())
    }

    /// Reads output after a cursor. A drained terminal process returns an
    /// empty page carrying its outcome rather than a not-found, so a reader
    /// can always distinguish "nothing new" from "gone".
    ///
    /// # Errors
    /// `invalid_argument` for a blank id; `not_found` outside the bearer's
    /// own organization; `deadline_exceeded` past [`RPC_TIMEOUT`].
    pub async fn read_process_output(
        &self,
        bearer: &str,
        process_id: &str,
        after_seq: i64,
        max_bytes: i64,
    ) -> Result<ReadProcessOutputResponse, Status> {
        if process_id.trim().is_empty() {
            return Err(Status::invalid_argument("process_id is required"));
        }
        let message = ReadProcessOutputRequest {
            process_id: process_id.to_owned(),
            after_seq,
            max_bytes,
        };
        let wire = Self::authorize(bearer, message)?;
        let outcome = tokio::time::timeout(RPC_TIMEOUT, self.client().read_process_output(wire))
            .await
            .map_err(|_| {
                Status::deadline_exceeded("sandbox-manager ReadProcessOutput timed out")
            })?;
        Ok(outcome?.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_client() -> SandboxManagerClient {
        SandboxManagerClient::new(
            tonic::transport::Endpoint::from_shared("http://127.0.0.1:1")
                .expect("valid endpoint")
                .connect_lazy(),
        )
    }

    #[tokio::test]
    async fn acquire_lease_rejects_blank_required_fields() {
        let client = test_client();
        let request = LeaseRequest {
            scope_id: "",
            scope_type: "thread",
            ttl: Duration::from_secs(60),
            org_id: "org-a",
            space_id: "",
            capability_decision: "",
            capability_claims_json: "",
        };
        let error = client
            .acquire_lease("bearer", &request)
            .await
            .expect_err("blank scope_id must fail closed before any RPC");
        assert_eq!(error.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn acquire_lease_rejects_zero_ttl() {
        let client = test_client();
        let request = LeaseRequest {
            scope_id: "thread-1",
            scope_type: "thread",
            ttl: Duration::ZERO,
            org_id: "org-a",
            space_id: "",
            capability_decision: "",
            capability_claims_json: "",
        };
        let error = client
            .acquire_lease("bearer", &request)
            .await
            .expect_err("zero ttl must fail closed before any RPC");
        assert_eq!(error.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn acquire_lease_requires_a_capability_decision_once_space_scoped() {
        let client = test_client();
        let request = LeaseRequest {
            scope_id: "thread-1",
            scope_type: "thread",
            ttl: Duration::from_secs(60),
            org_id: "org-a",
            space_id: "space-1",
            capability_decision: "",
            capability_claims_json: "",
        };
        let error = client
            .acquire_lease("bearer", &request)
            .await
            .expect_err("a Space-scoped request with no decision must fail closed before any RPC");
        assert_eq!(error.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn activate_snapshot_release_manifest_promote_reject_blank_lease_id() {
        let client = test_client();
        assert_eq!(
            client
                .activate_lease("bearer", "", "backend-1")
                .await
                .expect_err("blank lease_id must fail closed")
                .code(),
            tonic::Code::InvalidArgument
        );
        assert_eq!(
            client
                .snapshot_sandbox("bearer", "", "label", "backend-1", vec![])
                .await
                .expect_err("blank lease_id must fail closed")
                .code(),
            tonic::Code::InvalidArgument
        );
        assert_eq!(
            client
                .snapshot_sandbox("bearer", "lease-1", "", "backend-1", vec![])
                .await
                .expect_err("blank label must fail closed")
                .code(),
            tonic::Code::InvalidArgument
        );
        assert_eq!(
            client
                .release_lease("bearer", "", "backend-1")
                .await
                .expect_err("blank lease_id must fail closed")
                .code(),
            tonic::Code::InvalidArgument
        );
        assert_eq!(
            client
                .get_workspace_manifest("bearer", "", "backend-1")
                .await
                .expect_err("blank lease_id must fail closed")
                .code(),
            tonic::Code::InvalidArgument
        );
        assert_eq!(
            client
                .promote_workspace("bearer", "", "backend-1")
                .await
                .expect_err("blank lease_id must fail closed")
                .code(),
            tonic::Code::InvalidArgument
        );
    }

    #[tokio::test]
    async fn a_bearer_with_invalid_header_bytes_fails_closed_instead_of_panicking() {
        let client = test_client();
        let request = LeaseRequest {
            scope_id: "thread-1",
            scope_type: "thread",
            ttl: Duration::from_secs(60),
            org_id: "org-a",
            space_id: "",
            capability_decision: "",
            capability_claims_json: "",
        };
        let error = client
            .acquire_lease("not\na-valid-header-value", &request)
            .await
            .expect_err("an unforwardable bearer must fail closed, not panic");
        assert_eq!(error.code(), tonic::Code::Internal);
    }
}
