//! Acquires a sandbox-manager lease for a Space-scoped `code_interpreter`
//! step — the first real caller of `sandbox_manager_client.rs` (S3.3 §3.5
//! phase B.2). Does not yet change how `code_interpreter.rs` executes: the
//! lease is acquired and cached so the machinery is proven end to end, but
//! wiring it into the tool's own workspace is phase 3.5.C's job, not this
//! one's.
//!
//! `ActivateLease` is deliberately never called from here: `code_interpreter`
//! is governed as `cap.command.sandbox` precisely because it is hermetic
//! (read-only rootfs, no network, `egress: "disabled_by_default"`), so its
//! lease never needs more than the credential-free `SCRATCH` allowlist
//! `AcquireLease` already grants. A future capability that genuinely needs
//! network/egress would call `ActivateLease` itself; this module does not
//! speculatively wire a transition nothing here uses.

use tonic::Status;
use tracing::warn;

use crate::capability_client::{CapabilityClient, CapabilityDecisionRequest};
use crate::sandbox_manager_client::{LeaseRequest, SandboxManagerClient};
use crate::state::{SandboxLease, StateStore};

/// One hour: generous for a single agent run's `code_interpreter` calls,
/// bounded so an unreleased lease (this run's own release lifecycle is phase
/// 3.5's slice (d), not yet built) cannot accumulate forever server-side.
/// sandbox-manager enforces this TTL itself; this constant only chooses it.
const LEASE_TTL: std::time::Duration = std::time::Duration::from_secs(3_600);

/// Everything `ensure_sandbox_lease` needs beyond the step's own
/// `run_id`/`org_id`/`user_id`. A caller constructs this ONLY after it has
/// already determined the step is Space-scoped and already verified
/// `sandbox_bearer` (`auth::authenticate_delegated_sandbox_manager`) — this
/// type carries no "maybe absent bearer" state of its own, so its mere
/// existence is the caller's proof both checks already passed.
pub struct SandboxLeaseContext<'a> {
    /// The Space this step's run belongs to (`ExecuteStepRequest`/
    /// `RunAgentRequest`'s own `space_id` field, non-empty by construction).
    pub space_id: &'a str,
    /// The delegated, user-bound `aud=sandbox-manager` credential — the ONLY
    /// bearer `AcquireLease` will accept for a Space-scoped request, per
    /// sandbox-manager's own subject-identity check.
    pub sandbox_bearer: &'a str,
    /// Absent exactly when `CapabilityClient::from_env` found no Control
    /// Plane configuration — a valid disabled state, not a misconfiguration.
    pub capability_client: Option<&'a CapabilityClient>,
    pub sandbox_manager_client: &'a SandboxManagerClient,
    /// This execution-core instance's own stable identifier — the same value
    /// `http_health.rs`'s `/capability-profile` endpoint already reports and
    /// presents to Control, so the decision this function requests and the
    /// backend a lease pins to are always the same instance's own identity.
    pub backend_id: &'a str,
}

/// Returns this run's sandbox lease, acquiring one from sandbox-manager if
/// none is cached yet. A run's SECOND and later Space-scoped
/// `code_interpreter` step reuses the first step's lease — `AcquireLease`
/// mints a fresh lease id on every call (it is not idempotent by scope), so
/// without this cache every step would open its own, orphaned lease.
///
/// # Errors
/// Fails closed — never falls back to running `code_interpreter` without a
/// lease — when: no Control Plane capability client is configured; the
/// signed capability decision request fails or is refused; or
/// `AcquireLease` itself fails (including a backend-pin mismatch, surfaced
/// unchanged from sandbox-manager's own status).
pub async fn ensure_sandbox_lease(
    ctx: &SandboxLeaseContext<'_>,
    state: &StateStore,
    run_id: &str,
    org_id: &str,
    user_id: &str,
) -> Result<SandboxLease, Status> {
    if let Some(existing) = state.sandbox_lease(run_id) {
        return Ok(existing);
    }

    let capability_client = ctx.capability_client.ok_or_else(|| {
        Status::failed_precondition("Space sandbox capability verification is not configured")
    })?;
    let profile = crate::sandbox::capability_profile();
    let (decision, claims) = capability_client
        .request_decision(&CapabilityDecisionRequest {
            org_id,
            space_ref: ctx.space_id,
            subject_id: user_id,
            backend_id: ctx.backend_id,
            profile: &profile,
            idempotency_key: run_id,
        })
        .await
        .map_err(|error| {
            warn!(%error, "Space sandbox capability decision unavailable");
            Status::unavailable("Space sandbox capability decision unavailable")
        })?;
    let claims_json = serde_json::to_string(&claims)
        .map_err(|_| Status::internal("Space sandbox capability claims are not serializable"))?;

    let response = ctx
        .sandbox_manager_client
        .acquire_lease(
            ctx.sandbox_bearer,
            &LeaseRequest {
                scope_id: run_id,
                scope_type: "agent",
                ttl: LEASE_TTL,
                org_id,
                space_id: ctx.space_id,
                capability_decision: &decision,
                capability_claims_json: &claims_json,
            },
        )
        .await
        .inspect_err(|status| {
            warn!(code = ?status.code(), "sandbox-manager AcquireLease failed");
        })?;

    let lease = SandboxLease {
        lease_id: response.lease_id,
        backend_id: response.backend_id,
    };
    state.cache_sandbox_lease(run_id, lease.clone());
    Ok(lease)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unroutable_sandbox_manager_client() -> SandboxManagerClient {
        SandboxManagerClient::new(
            tonic::transport::Endpoint::from_shared("http://127.0.0.1:1")
                .expect("valid endpoint")
                .connect_lazy(),
        )
    }

    #[tokio::test]
    async fn a_second_call_for_the_same_run_reuses_the_cached_lease_without_a_client() {
        let state = StateStore::new();
        state.cache_sandbox_lease(
            "run-1",
            SandboxLease {
                lease_id: "lease-1".to_owned(),
                backend_id: "backend-1".to_owned(),
            },
        );
        // A `SandboxManagerClient` that would fail any real RPC (unroutable
        // address) still succeeds here — proof the cache hit never reaches
        // the network.
        let sandbox_manager_client = unroutable_sandbox_manager_client();
        let ctx = SandboxLeaseContext {
            space_id: "space-1",
            sandbox_bearer: "bearer",
            capability_client: None,
            sandbox_manager_client: &sandbox_manager_client,
            backend_id: "backend-1",
        };

        let lease = ensure_sandbox_lease(&ctx, &state, "run-1", "org-a", "user-a")
            .await
            .expect("cached lease is returned without any client call");

        assert_eq!(lease.lease_id, "lease-1");
        assert_eq!(lease.backend_id, "backend-1");
    }

    #[tokio::test]
    async fn acquiring_a_new_lease_without_a_capability_client_fails_closed() {
        let state = StateStore::new();
        let sandbox_manager_client = unroutable_sandbox_manager_client();
        let ctx = SandboxLeaseContext {
            space_id: "space-1",
            sandbox_bearer: "bearer",
            capability_client: None,
            sandbox_manager_client: &sandbox_manager_client,
            backend_id: "backend-1",
        };

        let error = ensure_sandbox_lease(&ctx, &state, "run-1", "org-a", "user-a")
            .await
            .expect_err("no capability client configured must fail closed");

        assert_eq!(error.code(), tonic::Code::FailedPrecondition);
        assert_eq!(state.sandbox_lease("run-1"), None);
    }
}
