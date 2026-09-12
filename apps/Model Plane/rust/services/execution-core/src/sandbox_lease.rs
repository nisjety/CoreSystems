//! Acquires, hydrates, and releases a sandbox-manager lease + workspace for a
//! run's Space-scoped `code_interpreter` calls (S3.3 §3.5 phases B.2 and C).
//!
//! [`ensure_sandbox_lease`] acquires (or reuses) the lease itself, credential-
//! free (`SCRATCH`) by default — `code_interpreter` is governed as
//! `cap.command.sandbox` precisely because it is hermetic (read-only rootfs,
//! no network, `egress: "disabled_by_default"`), so nothing here needs more
//! than that allowlist for the lease grant alone. [`ensure_hydrated_workspace`]
//! is the first real caller of `ActivateLease` (SCRATCH → ACTIVE) and
//! `GetWorkspaceManifest`: a Space-scoped call needs its DURABLE workspace
//! content, not just a hermetic execution grant, so the lease is promoted and
//! its manifest hydrated onto local disk the first time a run actually uses
//! one. [`release_sandbox_lease_if_any`] is the first real caller of
//! `SnapshotSandbox`, uploading whatever the run's own hydrated workspace
//! accumulated before releasing the lease.
//!
//! Release (and, with it, the workspace upload) is only reachable from the
//! two points execution-core can observe a run's actual end: `CancelRun`,
//! and a governed `RunAgent`'s own `finalize()`. A run driven purely by
//! repeated direct `ExecuteStep` calls (the inline chat `code_interpreter`
//! path) has no such signal at all — confirmed by tracing the call graph,
//! not assumed — so that lease's only backstop is [`LEASE_TTL`] expiring
//! server-side, and any hydrated workspace such a run creates is never
//! uploaded OR deleted by this module at all — its edits are lost, and its
//! local directory persists until the container itself restarts. This is a
//! known gap, sharing the same root cause and same existing "inline
//! ExecuteStep chat path has no run-ended signal" limitation the B.2 release
//! design already documented — not a new one this phase introduced, and not
//! an oversight.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mp_contracts::model_plane::v1::WorkspaceChangedFile;
use tonic::Status;
use tracing::warn;

use crate::capability_client::{CapabilityClient, CapabilityDecisionRequest};
use crate::sandbox_manager_client::{LeaseRequest, SandboxManagerClient};
use crate::state::{HydratedWorkspace, SandboxLease, StateStore};
use crate::workspace_cas::CasClient;
use crate::workspace_hydrate::WorkspaceFileEntry;

/// One hour: generous for a single agent run's `code_interpreter` calls,
/// bounded so a lease neither `CancelRun` nor `RunAgent`'s own `finalize()`
/// ever reaches (the inline `ExecuteStep` chat path, see the module doc)
/// cannot accumulate forever server-side. sandbox-manager enforces this TTL
/// itself; this constant only chooses it.
const LEASE_TTL: std::time::Duration = std::time::Duration::from_hours(1);

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
    /// Absent exactly when `CasClient::from_env` found no MinIO
    /// configuration — a valid disabled state. A Space-scoped
    /// `code_interpreter` step fails closed without it (see
    /// `ensure_hydrated_workspace`), since there is nowhere to fetch or
    /// upload workspace content.
    pub cas_client: Option<&'a CasClient>,
    /// Mints execution-core's own `sandbox:write`/`sandbox:read` service
    /// credential for `ActivateLease`/`GetWorkspaceManifest` here and
    /// `SnapshotSandbox`/`ReleaseLease` at run end — never `AcquireLease`,
    /// which needs the delegated user-bound bearer instead.
    pub sandbox_tokens: &'a SandboxManagerTokenProvider,
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

/// Ensures this run's Space-scoped `code_interpreter` workspace is hydrated
/// onto local disk, reusing it across every later call in the same run — the
/// one deliberate exception to `code_interpreter::Workspace::create`'s own
/// "never adopt an existing directory" invariant, which stays true for every
/// non-Space call (this function is never on that path at all).
///
/// The FIRST Space-scoped call in a run: activates `lease` (SCRATCH →
/// ACTIVE — this module's own first real caller of `ActivateLease`, since
/// nothing durable existed for this lease to read before now), fetches its
/// current workspace manifest, and hydrates it. Every later call reuses the
/// cached directory without any further sandbox-manager round trip.
///
/// # Errors
/// Fails closed — never runs `code_interpreter` against an unhydrated or
/// partially-hydrated directory — when CAS is not configured, the service
/// credential is unavailable, or `ActivateLease`/`GetWorkspaceManifest`/
/// `hydrate` itself fails.
pub async fn ensure_hydrated_workspace(
    ctx: &SandboxLeaseContext<'_>,
    state: &StateStore,
    run_id: &str,
    org_id: &str,
    lease: &SandboxLease,
) -> Result<PathBuf, Status> {
    if let Some(existing) = state.hydrated_workspace(run_id) {
        return Ok(existing.path);
    }

    let cas = ctx.cas_client.ok_or_else(|| {
        Status::failed_precondition("CAS is not configured; Space workspace unavailable")
    })?;
    let token = ctx.sandbox_tokens.token(org_id).await.map_err(|error| {
        warn!(%error, "sandbox-manager service credential unavailable for workspace hydrate");
        Status::unavailable("sandbox-manager service credential unavailable")
    })?;

    ctx.sandbox_manager_client
        .activate_lease(&token, &lease.lease_id, &lease.backend_id)
        .await
        .inspect_err(|status| {
            warn!(code = ?status.code(), "sandbox-manager ActivateLease failed");
        })?;
    let manifest = ctx
        .sandbox_manager_client
        .get_workspace_manifest(&token, &lease.lease_id, &lease.backend_id)
        .await
        .inspect_err(|status| {
            warn!(code = ?status.code(), "sandbox-manager GetWorkspaceManifest failed");
        })?;
    let entries: Vec<WorkspaceFileEntry> = manifest
        .entries
        .into_iter()
        .map(|entry| WorkspaceFileEntry {
            path: entry.path,
            content_hash: entry.content_hash,
        })
        .collect();

    let path = workspace_dir(ctx.space_id, &lease.lease_id);
    std::fs::create_dir_all(&path).map_err(|error| {
        Status::internal(format!(
            "could not create Space workspace directory: {error}"
        ))
    })?;
    let baseline = crate::workspace_hydrate::hydrate(cas, &entries, &path)
        .await
        .map_err(|error| Status::internal(format!("Space workspace hydrate failed: {error}")))?;

    let workspace = HydratedWorkspace {
        path: path.clone(),
        baseline,
    };
    state.cache_hydrated_workspace(run_id, workspace);
    Ok(path)
}

/// The persistent directory a Space-scoped lease's workspace hydrates into —
/// distinct per Space AND per lease, so two concurrent runs against the same
/// Space (each with its own lease) never share a directory.
fn workspace_dir(space_id: &str, lease_id: &str) -> PathBuf {
    std::env::temp_dir()
        .join("verevon-space-workspaces")
        .join(crate::code_interpreter::path_slug(space_id))
        .join(crate::code_interpreter::path_slug(lease_id))
}

const MAX_TOKEN_TTL_SECONDS: u64 = 3_600;
const TOKEN_REFRESH_SKEW: Duration = Duration::from_secs(30);
const SANDBOX_MANAGER_AUDIENCE: &str = "sandbox-manager";
/// Both scopes, not just `sandbox:write`: sandbox-manager's `authz.go`
/// gates `GetWorkspaceManifest` on `sandbox:read` with an exact
/// `slices.Contains` check (no write-implies-read hierarchy), and Auth Core
/// issues exactly the scopes requested (`plane-service-principal.ts`
/// returns `scopes: requestedScopes`, narrowed from the registry allowlist).
/// A write-only request therefore produced a token the hydrate path's
/// manifest read was refused with — found 2026-09-13 while designing S4.2.
const SERVICE_SCOPES: [&str; 2] = ["sandbox:read", "sandbox:write"];

struct CachedToken {
    value: String,
    expires_at: Instant,
}

/// Mints Auth-Core-issued, `aud=sandbox-manager` credentials from
/// execution-core's own deployment service principal — the SAME
/// `EXECUTION_CORE_SERVICE_ID`/`EXECUTION_CORE_SERVICE_API_KEY` already used
/// for `capability_policy.rs`'s `ServiceTokenProvider` (one service
/// credential, authorized for multiple audiences via Auth Core's
/// `plane-service-principals.json` registry — see that file's own
/// `execution-core` entry). A deliberate, independent copy of that struct's
/// shape rather than a generalized/shared one: this codebase already keeps
/// one such provider per audience (model-gateway alone has six — see
/// `docs/decisions/ledger.md`'s 2026-09-11 entry), so duplicating here
/// matches established convention rather than introducing a premature
/// abstraction two crates would have to share.
///
/// Used ONLY for `ActivateLease`/`GetWorkspaceManifest`/`SnapshotSandbox`/
/// `ReleaseLease` — never `AcquireLease`, which requires the delegated
/// user-bound bearer instead (see `SandboxLeaseContext`'s own doc for why).
/// Minted with [`SERVICE_SCOPES`] (read AND write) because that set of RPCs
/// spans both of sandbox-manager's scopes.
pub struct SandboxManagerTokenProvider {
    auth_core_url: String,
    service_id: String,
    credential: String,
    http: reqwest::Client,
    cache: tokio::sync::Mutex<HashMap<String, CachedToken>>,
}

impl std::fmt::Debug for SandboxManagerTokenProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SandboxManagerTokenProvider")
            .field("auth_core_url", &self.auth_core_url)
            .field("service_id", &self.service_id)
            .field("credential", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenResponse {
    token: String,
    expires_in_seconds: u64,
    audience: String,
}

impl SandboxManagerTokenProvider {
    /// # Errors
    /// Returns an error when required deployment configuration
    /// (`AUTH_CORE_URL`, `EXECUTION_CORE_SERVICE_API_KEY`) is missing or
    /// blank, or the HTTP client cannot be built.
    pub fn from_env() -> anyhow::Result<Self> {
        let required = |name: &'static str| {
            std::env::var(name)
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow::anyhow!("{name} is required"))
        };
        Ok(Self {
            auth_core_url: required("AUTH_CORE_URL")?.trim_end_matches('/').to_owned(),
            service_id: std::env::var("EXECUTION_CORE_SERVICE_ID")
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "execution-core".to_owned()),
            credential: required("EXECUTION_CORE_SERVICE_API_KEY")?,
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(5))
                .build()?,
            cache: tokio::sync::Mutex::new(HashMap::new()),
        })
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(auth_core_url: &str, service_id: &str, credential: &str) -> Self {
        Self {
            auth_core_url: auth_core_url.trim_end_matches('/').to_owned(),
            service_id: service_id.to_owned(),
            credential: credential.to_owned(),
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(5))
                .build()
                .expect("test HTTP client"),
            cache: tokio::sync::Mutex::new(HashMap::new()),
        }
    }

    /// # Errors
    /// Fails when the tenant is empty, Auth Core refuses the principal (a
    /// scope outside the deployment allowlist is refused outright, never
    /// silently narrowed), or the issued credential is not a valid
    /// `sandbox-manager` token.
    pub async fn token(&self, org_id: &str) -> anyhow::Result<String> {
        let org_id = org_id.trim();
        if org_id.is_empty() {
            anyhow::bail!("sandbox-manager token tenant is required");
        }
        let cache_key = org_id.to_owned();
        let now = Instant::now();
        {
            let mut cache = self.cache.lock().await;
            cache.retain(|_, token| token.expires_at > now + TOKEN_REFRESH_SKEW);
            if let Some(token) = cache.get(&cache_key) {
                return Ok(token.value.clone());
            }
        }

        let mut credential = reqwest::header::HeaderValue::from_str(&self.credential)
            .map_err(|_| anyhow::anyhow!("execution service credential is malformed"))?;
        credential.set_sensitive(true);
        let response = self
            .http
            .post(format!(
                "{}/api/{SANDBOX_MANAGER_AUDIENCE}/internal-token",
                self.auth_core_url
            ))
            .header("x-service-id", &self.service_id)
            .header("x-service-api-key", credential)
            .json(&serde_json::json!({
                "orgId": org_id,
                "scopes": SERVICE_SCOPES,
                "reason": "operate a Space-scoped sandbox lease lifecycle after acquire",
            }))
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!(
                "Auth Core refused a sandbox-manager credential (status {})",
                response.status()
            );
        }
        let bundle = response.json::<TokenResponse>().await?;
        if bundle.token.trim().is_empty()
            || bundle.audience != SANDBOX_MANAGER_AUDIENCE
            || !(1..=MAX_TOKEN_TTL_SECONDS).contains(&bundle.expires_in_seconds)
        {
            anyhow::bail!("Auth Core returned an invalid sandbox-manager credential");
        }
        self.cache.lock().await.insert(
            cache_key,
            CachedToken {
                value: bundle.token.clone(),
                expires_at: Instant::now() + Duration::from_secs(bundle.expires_in_seconds),
            },
        );
        Ok(bundle.token)
    }
}

/// Releases this run's sandbox lease, if it ever acquired one — the
/// counterpart to [`ensure_sandbox_lease`]/[`ensure_hydrated_workspace`].
/// Best-effort throughout: never returns an error, because a release or
/// upload failure must never fail the `CancelRun`/`RunAgent` response it
/// rides along with — sandbox-manager's own TTL is the backstop either way,
/// so a lost step here delays cleanup or loses this run's own uncommitted
/// workspace edits, it does not leak a lease unboundedly.
///
/// If this run ever hydrated a workspace, its changes are diffed and
/// uploaded (`workspace_hydrate::diff_and_upload` + `SnapshotSandbox`)
/// *before* the lease itself is released, and the local directory is removed
/// either way — this is the only place a Space-scoped workspace directory is
/// ever deleted, mirroring `code_interpreter::Workspace`'s own `Drop`, just
/// keyed to lease release instead of per-call.
pub async fn release_sandbox_lease_if_any(
    state: &StateStore,
    sandbox_manager_client: &SandboxManagerClient,
    tokens: &SandboxManagerTokenProvider,
    cas_client: Option<&CasClient>,
    run_id: &str,
    org_id: &str,
) {
    let Some(lease) = state.take_sandbox_lease(run_id) else {
        return;
    };
    let token = match tokens.token(org_id).await {
        Ok(token) => token,
        Err(error) => {
            warn!(run_id = %run_id, %error, "sandbox-manager service credential unavailable; lease left for TTL expiry");
            return;
        }
    };

    if let Some(workspace) = state.take_hydrated_workspace(run_id) {
        upload_and_discard_workspace(
            sandbox_manager_client,
            cas_client,
            &token,
            &lease,
            &workspace,
        )
        .await;
    }

    if let Err(status) = sandbox_manager_client
        .release_lease(&token, &lease.lease_id, &lease.backend_id)
        .await
    {
        warn!(
            run_id = %run_id,
            lease_id = %lease.lease_id,
            code = ?status.code(),
            "sandbox-manager ReleaseLease failed; lease left for TTL expiry"
        );
    }
}

/// Diffs and uploads `workspace`'s changes (if CAS is configured), records
/// them via `SnapshotSandbox`, and removes the local directory regardless of
/// whether either step succeeded — a lost upload means this run's edits
/// never reach the Space, but must never leave a directory behind forever or
/// block the lease release that follows.
async fn upload_and_discard_workspace(
    sandbox_manager_client: &SandboxManagerClient,
    cas_client: Option<&CasClient>,
    token: &str,
    lease: &SandboxLease,
    workspace: &HydratedWorkspace,
) {
    if let Some(cas) = cas_client {
        match crate::workspace_hydrate::diff_and_upload(cas, &workspace.path, &workspace.baseline)
            .await
        {
            Ok(changed) if !changed.is_empty() => {
                let changed_files: Vec<WorkspaceChangedFile> = changed
                    .into_iter()
                    .map(|file| WorkspaceChangedFile {
                        path: file.path,
                        content_hash: file.content_hash,
                        size_bytes: i64::try_from(file.size_bytes).unwrap_or(i64::MAX),
                        base_hash: file.base_hash.unwrap_or_default(),
                    })
                    .collect();
                if let Err(status) = sandbox_manager_client
                    .snapshot_sandbox(
                        token,
                        &lease.lease_id,
                        "code_interpreter run end",
                        &lease.backend_id,
                        changed_files,
                    )
                    .await
                {
                    warn!(
                        lease_id = %lease.lease_id,
                        code = ?status.code(),
                        "sandbox-manager SnapshotSandbox failed; this run's workspace changes are lost"
                    );
                }
            }
            Ok(_) => {}
            Err(error) => {
                warn!(
                    lease_id = %lease.lease_id,
                    %error,
                    "Space workspace diff/upload failed; this run's workspace changes are lost"
                );
            }
        }
    } else {
        warn!(
            lease_id = %lease.lease_id,
            "CAS is not configured; this run's workspace changes cannot be uploaded and are lost"
        );
    }

    cleanup_workspace_dir(&workspace.path);
}

fn cleanup_workspace_dir(path: &Path) {
    if let Err(error) = std::fs::remove_dir_all(path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            warn!(%error, workspace = %path.display(), "Space workspace cleanup failed");
        }
    }
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

    fn unroutable_token_provider() -> SandboxManagerTokenProvider {
        SandboxManagerTokenProvider::new_for_test(
            "http://127.0.0.1:1",
            "execution-core",
            "test-service-secret-at-least-32-bytes",
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
        let tokens = unroutable_token_provider();
        let ctx = SandboxLeaseContext {
            space_id: "space-1",
            sandbox_bearer: "bearer",
            capability_client: None,
            sandbox_manager_client: &sandbox_manager_client,
            backend_id: "backend-1",
            cas_client: None,
            sandbox_tokens: &tokens,
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
        let tokens = unroutable_token_provider();
        let ctx = SandboxLeaseContext {
            space_id: "space-1",
            sandbox_bearer: "bearer",
            capability_client: None,
            sandbox_manager_client: &sandbox_manager_client,
            backend_id: "backend-1",
            cas_client: None,
            sandbox_tokens: &tokens,
        };

        let error = ensure_sandbox_lease(&ctx, &state, "run-1", "org-a", "user-a")
            .await
            .expect_err("no capability client configured must fail closed");

        assert_eq!(error.code(), tonic::Code::FailedPrecondition);
        assert_eq!(state.sandbox_lease("run-1"), None);
    }

    #[tokio::test]
    async fn releasing_a_run_with_no_lease_never_touches_the_network() {
        let state = StateStore::new();
        let sandbox_manager_client = unroutable_sandbox_manager_client();
        let tokens = SandboxManagerTokenProvider::new_for_test(
            "http://127.0.0.1:1",
            "execution-core",
            "service-secret-at-least-32-bytes",
        );

        // A run that never acquired a lease releases silently: no token
        // request, no ReleaseLease call, and above all, no panic.
        release_sandbox_lease_if_any(
            &state,
            &sandbox_manager_client,
            &tokens,
            None,
            "run-1",
            "org-a",
        )
        .await;
    }

    #[tokio::test]
    async fn a_release_credential_failure_is_swallowed_and_the_lease_is_still_gone() {
        let state = StateStore::new();
        state.cache_sandbox_lease(
            "run-1",
            SandboxLease {
                lease_id: "lease-1".to_owned(),
                backend_id: "backend-1".to_owned(),
            },
        );
        let sandbox_manager_client = unroutable_sandbox_manager_client();
        // No Auth Core listening on this address: `tokens.token()` fails.
        let tokens = SandboxManagerTokenProvider::new_for_test(
            "http://127.0.0.1:1",
            "execution-core",
            "service-secret-at-least-32-bytes",
        );

        release_sandbox_lease_if_any(
            &state,
            &sandbox_manager_client,
            &tokens,
            None,
            "run-1",
            "org-a",
        )
        .await;

        // `take_sandbox_lease` already removed it before the credential
        // request was even attempted — a retried release call finds nothing
        // to release rather than trying (and failing) again forever.
        assert_eq!(state.sandbox_lease("run-1"), None);
    }

    #[tokio::test]
    async fn a_hydrated_workspace_is_reused_by_a_second_call_without_touching_the_network() {
        let state = StateStore::new();
        let workspace = HydratedWorkspace {
            path: PathBuf::from("/tmp/space-workspace-1"),
            baseline: HashMap::from([("a.txt".to_owned(), "sha256:aaa".to_owned())]),
        };
        state.cache_hydrated_workspace("run-1", workspace.clone());
        let sandbox_manager_client = unroutable_sandbox_manager_client();
        let tokens = unroutable_token_provider();
        let ctx = SandboxLeaseContext {
            space_id: "space-1",
            sandbox_bearer: "bearer",
            capability_client: None,
            sandbox_manager_client: &sandbox_manager_client,
            backend_id: "backend-1",
            cas_client: None,
            sandbox_tokens: &tokens,
        };
        let lease = SandboxLease {
            lease_id: "lease-1".to_owned(),
            backend_id: "backend-1".to_owned(),
        };

        let path = ensure_hydrated_workspace(&ctx, &state, "run-1", "org-a", &lease)
            .await
            .expect("cached workspace is returned without any client call");

        assert_eq!(path, workspace.path);
    }

    #[tokio::test]
    async fn hydrating_a_new_workspace_without_cas_configured_fails_closed() {
        let state = StateStore::new();
        let sandbox_manager_client = unroutable_sandbox_manager_client();
        let tokens = unroutable_token_provider();
        let ctx = SandboxLeaseContext {
            space_id: "space-1",
            sandbox_bearer: "bearer",
            capability_client: None,
            sandbox_manager_client: &sandbox_manager_client,
            backend_id: "backend-1",
            cas_client: None,
            sandbox_tokens: &tokens,
        };
        let lease = SandboxLease {
            lease_id: "lease-1".to_owned(),
            backend_id: "backend-1".to_owned(),
        };

        let error = ensure_hydrated_workspace(&ctx, &state, "run-1", "org-a", &lease)
            .await
            .expect_err("no CAS client configured must fail closed");

        assert_eq!(error.code(), tonic::Code::FailedPrecondition);
        assert!(state.hydrated_workspace("run-1").is_none());
    }

    #[tokio::test]
    async fn releasing_a_lease_with_no_cas_configured_still_releases_and_cleans_up() {
        let state = StateStore::new();
        state.cache_sandbox_lease(
            "run-1",
            SandboxLease {
                lease_id: "lease-1".to_owned(),
                backend_id: "backend-1".to_owned(),
            },
        );
        let dir = std::env::temp_dir().join("verevon-sandbox-lease-test-no-cas");
        std::fs::create_dir_all(&dir).expect("create test workspace dir");
        std::fs::write(dir.join("a.txt"), b"data").expect("write into test workspace");
        state.cache_hydrated_workspace(
            "run-1",
            HydratedWorkspace {
                path: dir.clone(),
                baseline: HashMap::new(),
            },
        );
        let sandbox_manager_client = unroutable_sandbox_manager_client();
        // No Auth Core listening, but the token check happens before the
        // workspace step here — use a real mock so release_lease itself is
        // reached and this test proves the DIRECTORY cleanup specifically.
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let auth = MockServer::start().await;
        Mock::given(wiremock::matchers::method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "sandbox-manager-token",
                "expiresInSeconds": 300,
                "audience": "sandbox-manager"
            })))
            .mount(&auth)
            .await;
        let tokens =
            SandboxManagerTokenProvider::new_for_test(&auth.uri(), "execution-core", "secret");

        release_sandbox_lease_if_any(
            &state,
            &sandbox_manager_client,
            &tokens,
            None,
            "run-1",
            "org-a",
        )
        .await;

        assert!(
            !dir.exists(),
            "the workspace directory must be removed even when CAS is not configured"
        );
        assert!(state.hydrated_workspace("run-1").is_none());
    }

    #[tokio::test]
    async fn service_token_requests_both_sandbox_scopes_and_is_cached() {
        use wiremock::matchers::{body_json, header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let auth = MockServer::start().await;
        // `sandbox:read` is load-bearing, not decorative: `GetWorkspaceManifest`
        // is gated on it in sandbox-manager's `authz.go`, and Auth Core issues
        // exactly the scopes requested here — a write-only request would leave
        // the hydrate path's manifest read refused (`authz_test.go`'s own
        // "GetWorkspaceManifest missing read scope" case).
        Mock::given(method("POST"))
            .and(path("/api/sandbox-manager/internal-token"))
            .and(header("x-service-id", "execution-core"))
            .and(header("x-service-api-key", "service-secret"))
            .and(body_json(serde_json::json!({
                "orgId": "org-a",
                "scopes": ["sandbox:read", "sandbox:write"],
                "reason": "operate a Space-scoped sandbox lease lifecycle after acquire"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "sandbox-manager-token",
                "expiresInSeconds": 300,
                "audience": "sandbox-manager"
            })))
            .expect(1)
            .mount(&auth)
            .await;
        let provider = SandboxManagerTokenProvider::new_for_test(
            &auth.uri(),
            "execution-core",
            "service-secret",
        );

        assert_eq!(
            provider.token("org-a").await.unwrap(),
            "sandbox-manager-token"
        );
        // Second call hits the cache — `.expect(1)` above would fail the test
        // if a second HTTP request went out.
        assert_eq!(
            provider.token("org-a").await.unwrap(),
            "sandbox-manager-token"
        );
        assert!(!format!("{provider:?}").contains("service-secret"));
    }

    #[tokio::test]
    async fn service_token_rejects_wrong_audience() {
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let auth = MockServer::start().await;
        Mock::given(wiremock::matchers::method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "wrong-audience-token",
                "expiresInSeconds": 300,
                "audience": "model-gateway"
            })))
            .mount(&auth)
            .await;
        let provider =
            SandboxManagerTokenProvider::new_for_test(&auth.uri(), "execution-core", "secret");

        let error = provider
            .token("org-a")
            .await
            .expect_err("wrong token audience must fail closed");
        assert!(error
            .to_string()
            .contains("invalid sandbox-manager credential"));
    }
}
