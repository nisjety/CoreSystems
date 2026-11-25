//! The retention sweeper's production workspace eraser (F-f10-09, owner D-f10-7): erases a
//! deleted thread's sandbox workspace through sandbox-manager's `EraseThreadWorkspaceForRetention`
//! (F-f10-08).
//!
//! # Authority
//!
//! The call carries session-core's own service bearer, minted by Auth Core for exactly one audience
//! (`sandbox-manager`), one scope (`sandbox:thread-workspace:erase:any`) and the organization being
//! swept. sandbox-manager refuses a user principal and a service without that scope, and it erases
//! only the rows of the named organization and thread, keyed by the owner the sweeper read from the
//! thread row before its delete. Nothing is read back from a workspace.
//!
//! # Outcomes
//!
//! Only an answer that erased is [`WorkspaceErasure::Erased`]. A refusal (sandbox-manager's own switch
//! is off, or the principal or scope is not accepted), an unreachable sandbox-manager, a credential
//! Auth Core will not mint, and a call past [`ERASE_DEADLINE`] are each
//! [`WorkspaceErasure::Unconfirmed`] with [`WORKSPACE_ERASURE_UNAVAILABLE`], the one closed word the
//! workspace tier has. The pass records it and goes on: the thread's delete committed before this
//! runs, and nothing here retries inside the pass.
//!
//! # Switch
//!
//! `SESSION_RETENTION_WORKSPACE_ERASE` is read exactly: only the literal `true` enables this eraser.
//! Off, or on without `SANDBOX_MANAGER_ADDR`, `AUTH_CORE_URL`, `SESSION_CORE_SERVICE_ID` and
//! `SESSION_CORE_SERVICE_API_KEY`, the sweeper keeps the unavailable eraser. It matters only while
//! `SESSION_RETENTION_SWEEP` is `enforce`. sandbox-manager must also run with
//! `SANDBOX_MANAGER_SERVICE_THREAD_ERASE=true`, or every erase is refused.
//!
//! # Delete paths (P2-sc-16-rel1)
//!
//! The user delete, the bulk delete and the organization purge also remove thread rows, and a workspace
//! they leave behind otherwise waits for sandbox-manager's own idle sweep. [`erase_after_delete`] erases
//! the workspaces of the threads one of those paths has just committed: after its commit, never inside
//! it, under the owner the thread's row names, and never failing the delete. Each path calls it last, after
//! the semantic copies (and the snapshot grants, where the path revokes them), so a request dropped during
//! the erase skips none of them. It is switched by `SESSION_DELETE_WORKSPACE_ERASE`, read exactly as above and independent of the
//! sweeper's switch. Off, those paths erase nothing and behave as before. An erase that is not confirmed is
//! counted in `mp_session_workspace_erase_unconfirmed_total` and logged by count, never by thread id.
//!
//! # ZDR
//!
//! Ids only cross the wire. No workspace content is read, and no content is logged.

use crate::retention_sweeper::{
    UnavailableWorkspaceEraser, WorkspaceEraser, WorkspaceErasure, WORKSPACE_ERASURE_UNAVAILABLE,
};
use crate::service_token::ServiceTokenProvider;
use mp_contracts::model_plane::v1::{
    sandbox_manager_client::SandboxManagerClient, EraseThreadWorkspaceForRetentionRequest,
};
use std::{future::Future, pin::Pin, time::Duration};
use tonic::{
    metadata::MetadataValue,
    transport::{Channel, Endpoint},
    Request,
};
use tracing::warn;

/// The switch. Only the exact literal `true` enables this eraser.
pub(crate) const SWITCH_ENV: &str = "SESSION_RETENTION_WORKSPACE_ERASE";
/// sandbox-manager's gRPC address. A value without a scheme is read as `http://`.
pub(crate) const ADDR_ENV: &str = "SANDBOX_MANAGER_ADDR";
/// The switch for the delete paths (P2-sc-16-rel1). Only the exact literal `true` enables it.
pub(crate) const DELETE_SWITCH_ENV: &str = "SESSION_DELETE_WORKSPACE_ERASE";

const AUTH_CORE_ENV: &str = "AUTH_CORE_URL";
const SERVICE_ID_ENV: &str = "SESSION_CORE_SERVICE_ID";
const CREDENTIAL_ENV: &str = "SESSION_CORE_SERVICE_API_KEY";

/// The one audience session-core mints a token for here, and the one scope it asks for.
const SANDBOX_AUDIENCE: &str = "sandbox-manager";
const ERASE_SCOPES: &[&str] = &["sandbox:thread-workspace:erase:any"];
/// Recorded in Auth Core's issuance audit, so a token in the log traces back to this erase.
const TOKEN_REASON: &str = "session-core retention workspace erase";

/// The most one erase may take. Past it the erase is unconfirmed and the pass goes on.
pub(crate) const ERASE_DEADLINE: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Erases thread workspaces over sandbox-manager's service erase. One instance serves every pass, so
/// its channel and its token cache are shared.
pub(crate) struct GrpcWorkspaceEraser {
    channel: Channel,
    tokens: ServiceTokenProvider,
    deadline: Duration,
}

impl GrpcWorkspaceEraser {
    /// `endpoint` is sandbox-manager's address and `tokens` mints its bearer. The channel connects
    /// lazily, so a sandbox-manager that is down at boot fails the first erase, never the boot.
    fn new(endpoint: &str, tokens: ServiceTokenProvider) -> anyhow::Result<Self> {
        let channel = Endpoint::from_shared(endpoint.to_owned())?
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(ERASE_DEADLINE)
            .connect_lazy();
        Ok(Self {
            channel,
            tokens,
            deadline: ERASE_DEADLINE,
        })
    }

    /// The production eraser from the environment, or `None`, which keeps the unavailable eraser.
    /// `None` unless the switch is the literal `true` and the sandbox-manager address, Auth Core's
    /// URL and session-core's service identity and credential are all set. A value that is set but
    /// does not parse is logged and is `None` too: a misconfigured eraser never stops the sweeper.
    pub(crate) fn from_lookup(get: &impl Fn(&str) -> Option<String>) -> Option<Self> {
        if !enabled_from(get(SWITCH_ENV).as_deref()) {
            return None;
        }
        Self::configured(
            get,
            SWITCH_ENV,
            "retention passes leave workspaces unconfirmed",
        )
    }

    /// The eraser the delete paths use (P2-sc-16-rel1): the configuration [`Self::from_lookup`] reads, behind
    /// [`DELETE_SWITCH_ENV`] instead. `None` unless that switch is the literal `true`.
    pub(crate) fn for_delete_paths(get: &impl Fn(&str) -> Option<String>) -> Option<Self> {
        if !enabled_from(get(DELETE_SWITCH_ENV).as_deref()) {
            return None;
        }
        Self::configured(
            get,
            DELETE_SWITCH_ENV,
            "thread deletes leave their workspaces unconfirmed",
        )
    }

    /// The configuration the two switches share, read once the switch is on. `switch` names the switch in the
    /// log, and `consequence` says what an unusable setting leaves unconfirmed.
    fn configured(
        get: &impl Fn(&str) -> Option<String>,
        switch: &'static str,
        consequence: &'static str,
    ) -> Option<Self> {
        let (Some(address), Some(auth_core_url), Some(service_id), Some(credential)) = (
            non_empty(get(ADDR_ENV)),
            non_empty(get(AUTH_CORE_ENV)),
            non_empty(get(SERVICE_ID_ENV)),
            non_empty(get(CREDENTIAL_ENV)),
        ) else {
            warn!(
                "{switch} is on but {ADDR_ENV}, {AUTH_CORE_ENV}, {SERVICE_ID_ENV} or {CREDENTIAL_ENV} is unset; {consequence}"
            );
            return None;
        };
        if reqwest::Url::parse(&auth_core_url).is_err() {
            warn!("{AUTH_CORE_ENV} is not a URL; {consequence}");
            return None;
        }
        let endpoint = if address.contains("://") {
            address
        } else {
            format!("http://{address}")
        };
        let tokens = match ServiceTokenProvider::new(
            &auth_core_url,
            SANDBOX_AUDIENCE,
            ERASE_SCOPES,
            TOKEN_REASON,
            &service_id,
            &credential,
        ) {
            Ok(tokens) => tokens,
            Err(error) => {
                warn!(%error, "the workspace erase credential could not be configured; {consequence}");
                return None;
            }
        };
        match Self::new(&endpoint, tokens) {
            Ok(eraser) => Some(eraser),
            Err(error) => {
                warn!(%error, "{ADDR_ENV} is not a valid sandbox-manager address; {consequence}");
                None
            }
        }
    }

    /// One erase. Never an error: every answer that is not an erase is unconfirmed.
    async fn erase_thread(
        &self,
        org_id: &str,
        thread_id: &str,
        owner_id: &str,
    ) -> WorkspaceErasure {
        let token = match self.tokens.token(org_id, ERASE_SCOPES).await {
            Ok(token) => token,
            Err(error) => {
                warn!(%org_id, %error, "retention workspace erase has no sandbox credential; the workspace stays unconfirmed");
                return unconfirmed();
            }
        };
        let Ok(mut authorization) = MetadataValue::try_from(format!("Bearer {token}")) else {
            warn!(%org_id, "retention workspace erase credential is not valid gRPC metadata");
            return unconfirmed();
        };
        authorization.set_sensitive(true);
        let mut request = Request::new(EraseThreadWorkspaceForRetentionRequest {
            org_id: org_id.to_owned(),
            thread_id: thread_id.to_owned(),
            owner_id: owner_id.to_owned(),
        });
        request.set_timeout(self.deadline);
        request
            .metadata_mut()
            .insert("authorization", authorization);
        let mut client = SandboxManagerClient::new(self.channel.clone());
        match tokio::time::timeout(
            self.deadline,
            client.erase_thread_workspace_for_retention(request),
        )
        .await
        {
            Ok(Ok(_)) => WorkspaceErasure::Erased,
            Ok(Err(status)) => {
                warn!(%org_id, code = ?status.code(), "sandbox-manager did not confirm the retention workspace erase");
                unconfirmed()
            }
            Err(_) => {
                warn!(%org_id, "sandbox-manager did not answer the retention workspace erase in time");
                unconfirmed()
            }
        }
    }
}

#[cfg(test)]
impl GrpcWorkspaceEraser {
    /// The same eraser with a shorter deadline, so a test can see the deadline end a call.
    pub(crate) fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }
}

impl WorkspaceEraser for GrpcWorkspaceEraser {
    fn erase<'a>(
        &'a self,
        org_id: &'a str,
        thread_id: &'a str,
        owner_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = WorkspaceErasure> + Send + 'a>> {
        Box::pin(self.erase_thread(org_id, thread_id, owner_id))
    }
}

fn unconfirmed() -> WorkspaceErasure {
    WorkspaceErasure::Unconfirmed(WORKSPACE_ERASURE_UNAVAILABLE)
}

/// Whether the switch is on: only the exact literal `true`, as the repo's strict parser reads it.
#[must_use]
pub(crate) fn enabled_from(raw: Option<&str>) -> bool {
    mp_contracts::loop_fold::switch_from(raw)
}

/// The value with its surrounding whitespace removed, or `None` when it is blank.
fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// What the delete paths' sandbox workspace erasures came to. As with the semantic tier, only an erasure
/// sandbox-manager confirmed counts as erased. Both fields are zero while [`DELETE_SWITCH_ENV`] is off.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceTally {
    /// Erasures sandbox-manager confirmed.
    pub confirmed: u64,
    /// Erasures it did not confirm: refused, unreachable, past the deadline, or with no configured eraser.
    pub unconfirmed: u64,
}

/// Whether [`DELETE_SWITCH_ENV`] is on in this process: only the exact literal `true`.
pub(crate) fn delete_switch_on() -> bool {
    enabled_from(std::env::var(DELETE_SWITCH_ENV).ok().as_deref())
}

/// Erases the sandbox workspace of each `(thread_id, owner_id)` in `deleted`, one after another, for deletes
/// the caller has already committed. Best effort: an erase that is not confirmed is counted and logged, and
/// the pass goes on, so no erase fails the delete it follows.
pub(crate) async fn erase_committed_deletes(
    workspaces: &dyn WorkspaceEraser,
    operation: &'static str,
    org_id: &str,
    deleted: &[(String, String)],
) -> WorkspaceTally {
    let mut tally = WorkspaceTally::default();
    for (thread_id, owner_id) in deleted {
        match workspaces.erase(org_id, thread_id, owner_id).await {
            WorkspaceErasure::Erased => tally.confirmed += 1,
            WorkspaceErasure::Unconfirmed(_) => tally.unconfirmed += 1,
        }
    }
    if tally.unconfirmed > 0 {
        metrics::counter!(
            "mp_session_workspace_erase_unconfirmed_total",
            "operation" => operation
        )
        .increment(tally.unconfirmed);
        warn!(
            operation,
            %org_id,
            confirmed = tally.confirmed,
            unconfirmed = tally.unconfirmed,
            "a deleted thread's sandbox workspace was not confirmed erased; sandbox-manager's own sweep must clear it"
        );
    }
    tally
}

/// The delete paths' entry point (P2-sc-16-rel1), over this process's environment. See
/// [`erase_after_delete_with`].
pub(crate) async fn erase_after_delete(
    operation: &'static str,
    org_id: &str,
    deleted: &[(String, String)],
) -> WorkspaceTally {
    let get = |name: &str| std::env::var(name).ok();
    erase_after_delete_with(&get, operation, org_id, deleted).await
}

/// Erases the workspaces of `deleted`, the threads a delete path has just committed, with the settings `get`
/// returns. Off, it reaches nothing and returns an empty tally. On, it uses the production eraser when that is
/// fully configured and [`UnavailableWorkspaceEraser`] when it is not, so a half-configured deployment counts
/// each workspace unconfirmed instead of claiming it.
pub(crate) async fn erase_after_delete_with(
    get: &impl Fn(&str) -> Option<String>,
    operation: &'static str,
    org_id: &str,
    deleted: &[(String, String)],
) -> WorkspaceTally {
    if deleted.is_empty() || !enabled_from(get(DELETE_SWITCH_ENV).as_deref()) {
        return WorkspaceTally::default();
    }
    match GrpcWorkspaceEraser::for_delete_paths(get) {
        Some(eraser) => erase_committed_deletes(&eraser, operation, org_id, deleted).await,
        None => {
            erase_committed_deletes(&UnavailableWorkspaceEraser, operation, org_id, deleted).await
        }
    }
}

/// The test double for sandbox-manager and for Auth Core's token issuance.
#[cfg(test)]
pub(crate) mod test_support {
    use super::{GrpcWorkspaceEraser, ERASE_SCOPES, SANDBOX_AUDIENCE, TOKEN_REASON};
    use crate::service_token::ServiceTokenProvider;
    use mp_contracts::model_plane::v1::{
        sandbox_manager_server::{SandboxManager, SandboxManagerServer},
        AcquireLeaseRequest, AcquireLeaseResponse, ActivateLeaseRequest, ActivateLeaseResponse,
        AppendProcessOutputRequest, AppendProcessOutputResponse,
        ApplyThreadWorkspaceChangesRequest, ApplyThreadWorkspaceChangesResponse,
        DeleteThreadWorkspaceRequest, DeleteThreadWorkspaceResponse,
        EraseThreadWorkspaceForRetentionRequest, EraseThreadWorkspaceForRetentionResponse,
        GetLeaseStatusRequest, GetLeaseStatusResponse, GetProcessRequest, GetProcessResponse,
        GetThreadWorkspaceRequest, GetThreadWorkspaceResponse, GetWorkspaceManifestRequest,
        GetWorkspaceManifestResponse, ListProcessesRequest, ListProcessesResponse,
        PromoteWorkspaceRequest, PromoteWorkspaceResponse, ReadProcessOutputRequest,
        ReadProcessOutputResponse, ReconcileProcessesRequest, ReconcileProcessesResponse,
        RegisterProcessRequest, RegisterProcessResponse, ReleaseLeaseRequest, ReleaseLeaseResponse,
        SandboxHealthRequest, SandboxHealthResponse, SnapshotRequest, SnapshotResponse,
        UpdateProcessStateRequest, UpdateProcessStateResponse,
    };
    use std::{
        net::SocketAddr,
        sync::{Arc, Mutex},
        time::Duration,
    };
    use tonic::{Code, Request, Response, Status};
    use wiremock::{
        matchers::{body_json, header, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    /// One erase the fake sandbox-manager received.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub(crate) struct SeenErase {
        pub(crate) authorization: String,
        pub(crate) org_id: String,
        pub(crate) thread_id: String,
        pub(crate) owner_id: String,
    }

    /// A sandbox-manager that records each erase, then answers it with `refusal` or erases it. With
    /// `delay` it answers late, so a test can see a deadline end the call.
    #[derive(Clone, Default)]
    pub(crate) struct FakeSandboxManager {
        pub(crate) seen: Arc<Mutex<Vec<SeenErase>>>,
        pub(crate) refusal: Option<Code>,
        pub(crate) delay: Option<Duration>,
    }

    fn unused<T>() -> Result<Response<T>, Status> {
        Err(Status::unimplemented(
            "not used by the retention erase tests",
        ))
    }

    #[tonic::async_trait]
    impl SandboxManager for FakeSandboxManager {
        async fn erase_thread_workspace_for_retention(
            &self,
            request: Request<EraseThreadWorkspaceForRetentionRequest>,
        ) -> Result<Response<EraseThreadWorkspaceForRetentionResponse>, Status> {
            let authorization = request
                .metadata()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_owned();
            let message = request.into_inner();
            self.seen.lock().expect("seen").push(SeenErase {
                authorization,
                org_id: message.org_id,
                thread_id: message.thread_id,
                owner_id: message.owner_id,
            });
            if let Some(delay) = self.delay {
                tokio::time::sleep(delay).await;
            }
            match self.refusal {
                Some(code) => Err(Status::new(code, "refused by the fake")),
                None => Ok(Response::new(EraseThreadWorkspaceForRetentionResponse {
                    files_deleted: 3,
                })),
            }
        }

        async fn acquire_lease(
            &self,
            _: Request<AcquireLeaseRequest>,
        ) -> Result<Response<AcquireLeaseResponse>, Status> {
            unused()
        }

        async fn release_lease(
            &self,
            _: Request<ReleaseLeaseRequest>,
        ) -> Result<Response<ReleaseLeaseResponse>, Status> {
            unused()
        }

        async fn snapshot_sandbox(
            &self,
            _: Request<SnapshotRequest>,
        ) -> Result<Response<SnapshotResponse>, Status> {
            unused()
        }

        async fn activate_lease(
            &self,
            _: Request<ActivateLeaseRequest>,
        ) -> Result<Response<ActivateLeaseResponse>, Status> {
            unused()
        }

        async fn get_workspace_manifest(
            &self,
            _: Request<GetWorkspaceManifestRequest>,
        ) -> Result<Response<GetWorkspaceManifestResponse>, Status> {
            unused()
        }

        async fn promote_workspace(
            &self,
            _: Request<PromoteWorkspaceRequest>,
        ) -> Result<Response<PromoteWorkspaceResponse>, Status> {
            unused()
        }

        async fn register_process(
            &self,
            _: Request<RegisterProcessRequest>,
        ) -> Result<Response<RegisterProcessResponse>, Status> {
            unused()
        }

        async fn update_process_state(
            &self,
            _: Request<UpdateProcessStateRequest>,
        ) -> Result<Response<UpdateProcessStateResponse>, Status> {
            unused()
        }

        async fn append_process_output(
            &self,
            _: Request<AppendProcessOutputRequest>,
        ) -> Result<Response<AppendProcessOutputResponse>, Status> {
            unused()
        }

        async fn reconcile_processes(
            &self,
            _: Request<ReconcileProcessesRequest>,
        ) -> Result<Response<ReconcileProcessesResponse>, Status> {
            unused()
        }

        async fn get_process(
            &self,
            _: Request<GetProcessRequest>,
        ) -> Result<Response<GetProcessResponse>, Status> {
            unused()
        }

        async fn list_processes(
            &self,
            _: Request<ListProcessesRequest>,
        ) -> Result<Response<ListProcessesResponse>, Status> {
            unused()
        }

        async fn read_process_output(
            &self,
            _: Request<ReadProcessOutputRequest>,
        ) -> Result<Response<ReadProcessOutputResponse>, Status> {
            unused()
        }

        async fn health(
            &self,
            _: Request<SandboxHealthRequest>,
        ) -> Result<Response<SandboxHealthResponse>, Status> {
            unused()
        }

        async fn get_thread_workspace(
            &self,
            _: Request<GetThreadWorkspaceRequest>,
        ) -> Result<Response<GetThreadWorkspaceResponse>, Status> {
            unused()
        }

        async fn apply_thread_workspace_changes(
            &self,
            _: Request<ApplyThreadWorkspaceChangesRequest>,
        ) -> Result<Response<ApplyThreadWorkspaceChangesResponse>, Status> {
            unused()
        }

        async fn delete_thread_workspace(
            &self,
            _: Request<DeleteThreadWorkspaceRequest>,
        ) -> Result<Response<DeleteThreadWorkspaceResponse>, Status> {
            unused()
        }

        async fn get_lease_status(
            &self,
            _: Request<GetLeaseStatusRequest>,
        ) -> Result<Response<GetLeaseStatusResponse>, Status> {
            unused()
        }
    }

    /// Serves `fake` on an ephemeral local port, and returns the address.
    pub(crate) async fn serve(fake: FakeSandboxManager) -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(SandboxManagerServer::new(fake))
                .serve_with_incoming(
                    tonic::transport::server::TcpIncoming::from_listener(listener, true, None)
                        .expect("incoming"),
                )
                .await
                .expect("serve");
        });
        address
    }

    /// Auth Core minting session-core's erase token for `org_id`, and for nothing else.
    pub(crate) async fn mint_erase_token(org_id: &str) -> MockServer {
        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/sandbox-manager/internal-token"))
            .and(header("x-service-id", "session-core"))
            .and(header("x-service-api-key", "session-core-secret"))
            .and(body_json(serde_json::json!({
                "orgId": org_id,
                "scopes": ["sandbox:thread-workspace:erase:any"],
                "reason": TOKEN_REASON,
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "erase-token-fixture",
                "expiresInSeconds": 300,
                "audience": "sandbox-manager",
            })))
            .mount(&auth)
            .await;
        auth
    }

    /// An eraser whose token comes from `auth_core` and whose erases reach `sandbox`.
    pub(crate) fn eraser_for(auth_core: &str, sandbox: SocketAddr) -> GrpcWorkspaceEraser {
        let tokens = ServiceTokenProvider::new(
            auth_core,
            SANDBOX_AUDIENCE,
            ERASE_SCOPES,
            TOKEN_REASON,
            "session-core",
            "session-core-secret",
        )
        .expect("token provider");
        GrpcWorkspaceEraser::new(&format!("http://{sandbox}"), tokens).expect("eraser")
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{eraser_for, mint_erase_token, serve, FakeSandboxManager, SeenErase};
    use super::*;
    use std::collections::HashMap;
    use std::net::SocketAddr;
    use std::time::Instant;
    use tonic::Code;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    const CONFIGURED: [(&str, &str); 5] = [
        (SWITCH_ENV, "true"),
        (ADDR_ENV, "sandbox-manager:9094"),
        (AUTH_CORE_ENV, "http://auth-core:3011"),
        (SERVICE_ID_ENV, "session-core"),
        (CREDENTIAL_ENV, "session-core-secret"),
    ];

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        move |name| map.get(name).cloned()
    }

    /// `CONFIGURED` with `name` set to `value`.
    fn with(name: &str, value: &'static str) -> Vec<(&'static str, &'static str)> {
        CONFIGURED
            .iter()
            .map(|(key, current)| (*key, if *key == name { value } else { *current }))
            .collect()
    }

    #[test]
    fn only_the_literal_true_enables_the_production_eraser() {
        assert!(enabled_from(Some("true")));
        for raw in [
            None,
            Some(""),
            Some("false"),
            Some("1"),
            Some("on"),
            Some(" true "),
            Some("TRUE"),
            Some("True"),
        ] {
            assert!(!enabled_from(raw), "{raw:?}");
        }
    }

    /// Async because a built eraser holds a lazy tonic channel, which needs a runtime (the sweeper
    /// builds it inside its async pass).
    #[tokio::test]
    async fn the_eraser_is_built_only_when_switched_on_and_fully_configured() {
        assert!(GrpcWorkspaceEraser::from_lookup(&lookup(&CONFIGURED)).is_some());
        // Each of the four settings is needed, and a blank one is unset.
        for missing in [ADDR_ENV, AUTH_CORE_ENV, SERVICE_ID_ENV, CREDENTIAL_ENV] {
            let pairs = with(missing, "   ");
            assert!(
                GrpcWorkspaceEraser::from_lookup(&lookup(&pairs)).is_none(),
                "{missing} blank"
            );
        }
        // The switch reads only the literal true: a padded or cased value builds nothing.
        for raw in [" true", "true ", "TRUE", "1", "on", "false", ""] {
            let pairs = with(SWITCH_ENV, raw);
            assert!(
                GrpcWorkspaceEraser::from_lookup(&lookup(&pairs)).is_none(),
                "{raw:?}"
            );
        }
        // An Auth Core URL that does not parse, or an address that is not a valid endpoint.
        assert!(
            GrpcWorkspaceEraser::from_lookup(&lookup(&with(AUTH_CORE_ENV, "not a url"))).is_none()
        );
        assert!(
            GrpcWorkspaceEraser::from_lookup(&lookup(&with(ADDR_ENV, "exa mple:9094"))).is_none()
        );
    }

    #[test]
    fn the_production_erase_deadline_is_ten_seconds() {
        assert_eq!(ERASE_DEADLINE, Duration::from_secs(10));
    }

    #[tokio::test]
    async fn a_confirmed_erase_is_erased_and_names_the_owner_under_the_service_bearer() {
        let fake = FakeSandboxManager::default();
        let seen = fake.seen.clone();
        let sandbox = serve(fake).await;
        let auth = mint_erase_token("org-a").await;
        let eraser = eraser_for(&auth.uri(), sandbox);
        assert_eq!(
            eraser.erase("org-a", "thread-1", "user-9").await,
            WorkspaceErasure::Erased
        );
        assert_eq!(
            seen.lock().expect("seen").clone(),
            vec![SeenErase {
                authorization: "Bearer erase-token-fixture".to_owned(),
                org_id: "org-a".to_owned(),
                thread_id: "thread-1".to_owned(),
                owner_id: "user-9".to_owned(),
            }]
        );
    }

    /// Every refusal and every outage counts as unconfirmed, and each one reached sandbox-manager
    /// once: the pass records the word and goes on.
    #[tokio::test]
    async fn every_refusal_and_outage_is_unconfirmed_and_never_fails_the_pass() {
        for code in [
            Code::FailedPrecondition,
            Code::Unimplemented,
            Code::PermissionDenied,
            Code::Unauthenticated,
            Code::Internal,
            Code::Unavailable,
        ] {
            let fake = FakeSandboxManager {
                refusal: Some(code),
                ..FakeSandboxManager::default()
            };
            let seen = fake.seen.clone();
            let sandbox = serve(fake).await;
            let auth = mint_erase_token("org-a").await;
            let outcome = eraser_for(&auth.uri(), sandbox)
                .erase("org-a", "thread-1", "user-9")
                .await;
            assert_eq!(
                outcome,
                WorkspaceErasure::Unconfirmed(WORKSPACE_ERASURE_UNAVAILABLE),
                "{code:?}"
            );
            assert_eq!(
                seen.lock().expect("seen").len(),
                1,
                "{code:?} was asked once"
            );
        }
    }

    #[tokio::test]
    async fn a_credential_auth_core_will_not_mint_never_reaches_sandbox_manager() {
        let fake = FakeSandboxManager::default();
        let seen = fake.seen.clone();
        let sandbox = serve(fake).await;
        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/sandbox-manager/internal-token"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&auth)
            .await;
        let outcome = eraser_for(&auth.uri(), sandbox)
            .erase("org-a", "thread-1", "user-9")
            .await;
        assert_eq!(
            outcome,
            WorkspaceErasure::Unconfirmed(WORKSPACE_ERASURE_UNAVAILABLE)
        );
        assert!(seen.lock().expect("seen").is_empty());
    }

    #[tokio::test]
    async fn an_unreachable_sandbox_manager_is_unconfirmed() {
        // A port that was just free: nothing listens on it.
        let address = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
            listener.local_addr().expect("address")
        };
        let auth = mint_erase_token("org-a").await;
        let outcome = eraser_for(&auth.uri(), address)
            .erase("org-a", "thread-1", "user-9")
            .await;
        assert_eq!(
            outcome,
            WorkspaceErasure::Unconfirmed(WORKSPACE_ERASURE_UNAVAILABLE)
        );
    }

    #[tokio::test]
    async fn an_erase_past_its_deadline_is_unconfirmed() {
        let fake = FakeSandboxManager {
            delay: Some(Duration::from_secs(5)),
            ..FakeSandboxManager::default()
        };
        let sandbox = serve(fake).await;
        let auth = mint_erase_token("org-a").await;
        let started = Instant::now();
        let outcome = eraser_for(&auth.uri(), sandbox)
            .with_deadline(Duration::from_millis(200))
            .erase("org-a", "thread-1", "user-9")
            .await;
        assert_eq!(
            outcome,
            WorkspaceErasure::Unconfirmed(WORKSPACE_ERASURE_UNAVAILABLE)
        );
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "the deadline, not the late answer, ended the call"
        );
    }

    /// The delete switch has its own name and reads only the literal `true`, as the sweeper's does.
    #[test]
    fn the_delete_switch_has_its_own_name_and_reads_only_the_literal_true() {
        assert_eq!(DELETE_SWITCH_ENV, "SESSION_DELETE_WORKSPACE_ERASE");
        assert_ne!(DELETE_SWITCH_ENV, SWITCH_ENV);
        assert!(enabled_from(Some("true")));
        for raw in [
            None,
            Some(""),
            Some("false"),
            Some("1"),
            Some("on"),
            Some(" true "),
            Some("TRUE"),
        ] {
            assert!(!enabled_from(raw), "{raw:?}");
        }
    }

    /// Every setting the delete paths read, with the delete switch on.
    const DELETE_CONFIGURED: [(&str, &str); 5] = [
        (DELETE_SWITCH_ENV, "true"),
        (ADDR_ENV, "sandbox-manager:9094"),
        (AUTH_CORE_ENV, "http://auth-core:3011"),
        (SERVICE_ID_ENV, "session-core"),
        (CREDENTIAL_ENV, "session-core-secret"),
    ];

    /// `DELETE_CONFIGURED` with `name` set to `value`.
    fn delete_with(name: &str, value: &'static str) -> Vec<(&'static str, &'static str)> {
        DELETE_CONFIGURED
            .iter()
            .map(|(key, current)| (*key, if *key == name { value } else { *current }))
            .collect()
    }

    /// Each switch builds its own eraser and no other: the sweeper's switch does not start the delete paths,
    /// and the delete switch does not start the sweeper. Both need the same complete configuration.
    #[tokio::test]
    async fn each_switch_builds_only_its_own_eraser() {
        assert!(GrpcWorkspaceEraser::for_delete_paths(&lookup(&DELETE_CONFIGURED)).is_some());
        assert!(GrpcWorkspaceEraser::from_lookup(&lookup(&DELETE_CONFIGURED)).is_none());
        assert!(GrpcWorkspaceEraser::from_lookup(&lookup(&CONFIGURED)).is_some());
        assert!(GrpcWorkspaceEraser::for_delete_paths(&lookup(&CONFIGURED)).is_none());
        for missing in [ADDR_ENV, AUTH_CORE_ENV, SERVICE_ID_ENV, CREDENTIAL_ENV] {
            assert!(
                GrpcWorkspaceEraser::for_delete_paths(&lookup(&delete_with(missing, "   ")))
                    .is_none(),
                "{missing} blank"
            );
        }
        for raw in [" true", "true ", "TRUE", "1", "on", "false", ""] {
            assert!(
                GrpcWorkspaceEraser::for_delete_paths(&lookup(&delete_with(
                    DELETE_SWITCH_ENV,
                    raw
                )))
                .is_none(),
                "{raw:?}"
            );
        }
    }

    /// The delete paths' settings as an owned lookup: the delete switch set to `switch`, pointing at `auth`
    /// and `sandbox`.
    fn delete_lookup(
        switch: &str,
        auth: &str,
        sandbox: SocketAddr,
    ) -> impl Fn(&str) -> Option<String> {
        let settings: HashMap<String, String> = [
            (DELETE_SWITCH_ENV, switch.to_owned()),
            (ADDR_ENV, sandbox.to_string()),
            (AUTH_CORE_ENV, auth.to_owned()),
            (SERVICE_ID_ENV, "session-core".to_owned()),
            (CREDENTIAL_ENV, "session-core-secret".to_owned()),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect();
        move |name| settings.get(name).cloned()
    }

    /// Two threads of one owner, as a bulk delete names them.
    fn two_threads() -> Vec<(String, String)> {
        vec![
            ("thread-1".to_owned(), "user-9".to_owned()),
            ("thread-2".to_owned(), "user-9".to_owned()),
        ]
    }

    /// Switched on and fully configured, each deleted thread's workspace is erased once, under the service
    /// bearer and the owner its row names, and counted as confirmed.
    #[tokio::test]
    async fn a_switched_on_delete_erases_each_workspace_under_the_owner_bearer() {
        let fake = FakeSandboxManager::default();
        let seen = fake.seen.clone();
        let sandbox = serve(fake).await;
        let auth = mint_erase_token("org-a").await;
        let get = delete_lookup("true", &auth.uri(), sandbox);
        let tally = erase_after_delete_with(&get, "delete_threads", "org-a", &two_threads()).await;
        assert_eq!(
            tally,
            WorkspaceTally {
                confirmed: 2,
                unconfirmed: 0
            }
        );
        let erase_of = |thread_id: &str| SeenErase {
            authorization: "Bearer erase-token-fixture".to_owned(),
            org_id: "org-a".to_owned(),
            thread_id: thread_id.to_owned(),
            owner_id: "user-9".to_owned(),
        };
        assert_eq!(
            seen.lock().expect("seen").clone(),
            vec![erase_of("thread-1"), erase_of("thread-2")]
        );
    }

    /// Switched off, whatever else is set, a delete erases nothing, asks sandbox-manager nothing and counts
    /// nothing. Only the literal `true` switches it on.
    #[tokio::test]
    async fn a_switched_off_delete_erases_nothing() {
        let fake = FakeSandboxManager::default();
        let seen = fake.seen.clone();
        let sandbox = serve(fake).await;
        let auth = mint_erase_token("org-a").await;
        for raw in ["false", "", "1", " true", "TRUE"] {
            let get = delete_lookup(raw, &auth.uri(), sandbox);
            assert_eq!(
                erase_after_delete_with(&get, "delete_thread", "org-a", &two_threads()).await,
                WorkspaceTally::default(),
                "{raw:?}"
            );
        }
        assert!(seen.lock().expect("seen").is_empty());
    }

    /// A refusal by sandbox-manager is unconfirmed for each thread, each thread is asked once, and nothing fails.
    #[tokio::test]
    async fn a_refused_delete_erase_is_counted_unconfirmed_for_each_thread() {
        let fake = FakeSandboxManager {
            refusal: Some(Code::FailedPrecondition),
            ..FakeSandboxManager::default()
        };
        let seen = fake.seen.clone();
        let sandbox = serve(fake).await;
        let auth = mint_erase_token("org-a").await;
        let get = delete_lookup("true", &auth.uri(), sandbox);
        let tally = erase_after_delete_with(&get, "org_purge", "org-a", &two_threads()).await;
        assert_eq!(
            tally,
            WorkspaceTally {
                confirmed: 0,
                unconfirmed: 2
            }
        );
        assert_eq!(seen.lock().expect("seen").len(), 2);
    }

    /// Switched on without its configuration, the delete paths count every workspace unconfirmed and reach
    /// nothing; an empty delete counts nothing at all.
    #[tokio::test]
    async fn a_switched_on_delete_without_its_configuration_counts_each_workspace_unconfirmed() {
        let get = |name: &str| (name == DELETE_SWITCH_ENV).then(|| "true".to_owned());
        assert_eq!(
            erase_after_delete_with(&get, "delete_thread", "org-a", &two_threads()).await,
            WorkspaceTally {
                confirmed: 0,
                unconfirmed: 2
            }
        );
        assert_eq!(
            erase_after_delete_with(&get, "org_purge", "org-a", &[]).await,
            WorkspaceTally::default()
        );
    }

    /// Confirms every erase except the one of `thread-2`.
    struct ConfirmAllButSecond;

    impl WorkspaceEraser for ConfirmAllButSecond {
        fn erase<'a>(
            &'a self,
            _org_id: &'a str,
            thread_id: &'a str,
            _owner_id: &'a str,
        ) -> Pin<Box<dyn Future<Output = WorkspaceErasure> + Send + 'a>> {
            let outcome = if thread_id == "thread-2" {
                WorkspaceErasure::Unconfirmed(WORKSPACE_ERASURE_UNAVAILABLE)
            } else {
                WorkspaceErasure::Erased
            };
            Box::pin(std::future::ready(outcome))
        }
    }

    /// An erase that is not confirmed does not stop the erases after it, and each outcome is counted once.
    #[tokio::test]
    async fn an_unconfirmed_erase_does_not_stop_the_erases_after_it() {
        let tally = erase_committed_deletes(
            &ConfirmAllButSecond,
            "delete_threads",
            "org-a",
            &two_threads(),
        )
        .await;
        assert_eq!(
            tally,
            WorkspaceTally {
                confirmed: 1,
                unconfirmed: 1
            }
        );
    }

    /// Each delete path erases its workspaces after its own commit, and the purge reads the owner of every
    /// thread it deletes only through the statement that deletes it, and only while the switch is on. Pinned
    /// against the source, as the neighbouring erasure pins are. Each path erases last: after its semantic
    /// copies (and, where it revokes them, its snapshot grants), which exist nowhere else once the rows are
    /// gone, and with nothing awaited after the erase but the erase itself, so a request dropped during it
    /// skips none of them.
    #[test]
    fn each_delete_path_erases_its_workspaces_after_its_commit() {
        let grpc = include_str!("grpc.rs");
        let production = &grpc[..grpc.find("\nmod tests {").expect("the test module")];
        for (handler, operation) in [
            ("    async fn delete_thread(", "\"delete_thread\","),
            ("    async fn delete_threads(", "\"delete_threads\","),
        ] {
            let from = &production[production
                .find(handler)
                .unwrap_or_else(|| panic!("the handler {handler:?}"))..];
            let body = &from[..from
                .find("record_metrics(")
                .unwrap_or_else(|| panic!("the end of {handler:?}"))];
            let calls: Vec<usize> = body
                .match_indices("erase_after_delete(")
                .map(|(at, _)| at)
                .collect();
            assert_eq!(calls.len(), 1, "{handler:?} erases its workspaces once");
            assert!(
                body[calls[0]..].contains(operation),
                "{handler:?} names its operation {operation}"
            );
            let commit = body.find("tx.commit()").expect("the commit");
            assert!(
                commit < calls[0],
                "{handler:?} erases its workspaces only after its commit"
            );
            for (step, name) in [
                ("propagate_semantic_erasure(", "its semantic copies"),
                ("revoke_erased_snapshot_grants(", "its snapshot grants"),
            ] {
                let at = body
                    .find(step)
                    .unwrap_or_else(|| panic!("{handler:?} settles {name}"));
                assert!(
                    at < calls[0],
                    "{handler:?} erases its workspaces only after {name}"
                );
            }
            let tail = &body[calls[0]..];
            let response = tail
                .find("Ok(Response::new(")
                .unwrap_or_else(|| panic!("the response of {handler:?}"));
            assert_eq!(
                tail[..response].matches(".await").count(),
                1,
                "{handler:?} awaits nothing after its workspace erase but the erase itself"
            );
        }

        let gdpr = include_str!("gdpr.rs");
        let production = &gdpr[..gdpr
            .find("#[cfg(test)]\nmod tests {")
            .expect("the test module")];
        let purge = &production[production
            .find("pub async fn purge_organization_data(")
            .expect("the purge")..];
        let purge = &purge[..purge.find("\n}\n").expect("the end of the purge")];
        let calls: Vec<usize> = purge
            .match_indices("erase_after_delete(")
            .map(|(at, _)| at)
            .collect();
        assert_eq!(calls.len(), 1, "the purge erases its workspaces once");
        assert!(purge[calls[0]..].contains("\"org_purge\","));
        let commit = purge.find("tx.commit().await?;").expect("the purge commit");
        assert!(
            commit < calls[0],
            "the purge erases its workspaces only after its commit"
        );
        let semantic = purge
            .find("erase_semantic_copies(")
            .expect("the purge's semantic copies");
        assert!(
            semantic < calls[0],
            "the purge erases its workspaces only after its semantic copies"
        );
        let tail = &purge[calls[0]..];
        let result = tail.find("Ok(summary)").expect("the purge's result");
        assert_eq!(
            tail[..result].matches(".await").count(),
            1,
            "the purge awaits nothing after its workspace erase but the erase itself"
        );
        assert!(
            purge.contains("crate::sandbox_workspace_eraser::delete_switch_on()"),
            "the purge reads the owners only while the delete switch is on"
        );
        assert!(
            purge.contains("DELETE FROM threads WHERE org_id = $1 RETURNING id, user_id"),
            "the purge's switched-on statement returns the owner of each thread it deletes"
        );
    }
}
