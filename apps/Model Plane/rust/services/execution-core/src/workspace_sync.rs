//! Thread-scope workspace sync for `code_interpreter` (WS22-S2, DEC-23).
//!
//! A thread's files outlive the run that wrote them. [`open`] hydrates the
//! thread's files from sandbox-manager into a fresh directory for one call, and
//! [`ThreadWorkspace::commit`] saves what the call created, changed or removed.
//! A path another writer changed first is reported as a conflict and never
//! overwritten. The directory is removed when the workspace drops, on every path.
//!
//! Off unless `EXECUTION_CORE_THREAD_WORKSPACE` is the literal `true`. With it
//! off, and for every Space step, zero-retention step and waived step, nothing
//! here runs and `code_interpreter` keeps its per-call directory.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use mp_contracts::model_plane::v1::{ThreadDeletedPath, WorkspaceChangedFile};
use serde_json::{json, Value};
use tracing::warn;

use crate::sandbox_manager_client::SandboxManagerClient;
use crate::state::SandboxLease;
use crate::tool_bridge::ToolExecution;
use crate::workspace_cas::{self, CasClient};
use crate::workspace_hydrate::{self, WorkspaceFileEntry};

/// The most files one thread keeps after a call (DEC-23 D6). sandbox-manager
/// applies its own ceiling as well.
pub const MAX_THREAD_FILES: usize = 256;

/// The most bytes one thread keeps after a call (DEC-23 D6).
pub const MAX_THREAD_BYTES: u64 = 64 * 1024 * 1024;

/// The directory each call's hydrated copy is created under, inside the process
/// temp directory.
const ROOT_DIR: &str = "verevon-thread-workspaces";

/// Whether thread-scope workspaces are switched on. Read once per process, and
/// only the literal `true` enables them: an unset, empty or misspelled value keeps
/// the per-call path.
#[must_use]
pub fn switched_on() -> bool {
    static SWITCH: OnceLock<bool> = OnceLock::new();
    *SWITCH.get_or_init(|| {
        std::env::var("EXECUTION_CORE_THREAD_WORKSPACE").is_ok_and(|value| value == "true")
    })
}

/// The thread a step's `code_interpreter` workspace belongs to, or `None` when the
/// step stays on the per-call path. A Space step never takes the thread path, and
/// neither does a zero-retention or waived one: nothing such a turn writes is kept.
#[must_use]
pub fn thread_for_step<'a>(
    switched_on: bool,
    persistence_free: bool,
    space_id: &str,
    thread_id: &'a str,
) -> Option<&'a str> {
    (switched_on && !persistence_free && space_id.is_empty() && !thread_id.trim().is_empty())
        .then_some(thread_id)
}

/// What one thread-scope call reads and writes through: the step's thread, the
/// user-bound sandbox-manager bearer, and the clients that reach sandbox-manager
/// and CAS. Built only for a step [`thread_for_step`] admitted, so its presence is
/// never a Space decision.
#[derive(Clone, Copy)]
pub struct ThreadScope<'a> {
    pub thread_id: &'a str,
    pub bearer: &'a str,
    pub backend_id: &'a str,
    pub sandbox_manager_client: &'a SandboxManagerClient,
    pub cas_client: Option<&'a CasClient>,
}

/// Why a thread workspace was not opened or not saved. The token is what a caller
/// reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    /// The files the thread would keep exceed the per-thread ceiling. Nothing was
    /// uploaded or recorded.
    QuotaExceeded,
    /// The thread's files could not be read from sandbox-manager or written into
    /// the call's directory.
    HydrateFailed,
    /// The changes could not be uploaded or recorded.
    CommitFailed,
    /// Not kept here: zero retention, a waiver, or no object store configured.
    Unavailable,
}

impl Cause {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::QuotaExceeded => "quota_exceeded",
            Self::HydrateFailed => "hydrate_failed",
            Self::CommitFailed => "commit_failed",
            Self::Unavailable => "unavailable",
        }
    }
}

/// A failed open or commit: its typed cause and a message for the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub cause: Cause,
    pub message: String,
}

impl Failure {
    fn new(cause: Cause, message: impl Into<String>) -> Self {
        Self {
            cause,
            message: message.into(),
        }
    }

    /// The workspace part of a tool result for a save that did not happen.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({ "scope": "thread", "state": "refused", "cause": self.cause.as_str() })
    }
}

/// What one commit did, for the model's tool result.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// Paths another writer changed first. This call's content for them was not
    /// saved, and the thread keeps the other writer's.
    pub conflicts: Vec<String>,
    /// Paths with a known secret pattern, removed before they were saved.
    pub redacted_paths: Vec<String>,
    /// Files this call created or changed, as offered to the thread.
    pub changed: usize,
    /// Files this call removed, as offered to the thread.
    pub removed: usize,
}

impl Report {
    #[must_use]
    pub fn to_json(&self) -> Value {
        let state = if self.conflicts.is_empty() {
            "synced"
        } else {
            "conflict"
        };
        json!({
            "scope": "thread",
            "state": state,
            "conflicts": self.conflicts,
            "redacted_paths": self.redacted_paths,
        })
    }
}

/// One call's hydrated copy of a thread's files. Dropping it removes the directory.
pub struct ThreadWorkspace<'a> {
    scope: ThreadScope<'a>,
    lease_id: String,
    backend_id: String,
    prefix: String,
    path: PathBuf,
    /// The `path -> content_hash` the thread held when this call opened it: what
    /// a save compares against and what a save of a removal sends as its base.
    baseline: HashMap<String, String>,
}

/// Opens the thread's workspace for one call: fetches its files from sandbox-manager
/// and hydrates them into a fresh directory.
///
/// # Errors
/// [`Cause::Unavailable`] when the call is persistence-free or no object store is
/// configured; [`Cause::HydrateFailed`] when the files cannot be read or written.
/// A failed open leaves nothing behind.
pub async fn open<'a>(
    scope: ThreadScope<'a>,
    lease: &SandboxLease,
    org_id: &str,
    persistence_free: bool,
) -> Result<ThreadWorkspace<'a>, Failure> {
    open_in(
        &std::env::temp_dir().join(ROOT_DIR),
        scope,
        lease,
        org_id,
        persistence_free,
    )
    .await
}

/// [`open`] with the directory's parent named, so a test can see what a call left
/// under it.
async fn open_in<'a>(
    root: &Path,
    scope: ThreadScope<'a>,
    lease: &SandboxLease,
    org_id: &str,
    persistence_free: bool,
) -> Result<ThreadWorkspace<'a>, Failure> {
    if persistence_free {
        return Err(Failure::new(
            Cause::Unavailable,
            "the thread's files are not kept under zero data retention or a waiver",
        ));
    }
    let Some(cas) = scope.cas_client else {
        return Err(Failure::new(
            Cause::Unavailable,
            "no object store is configured for the thread's files",
        ));
    };
    let prefix = workspace_cas::thread_scope(org_id, scope.thread_id)
        .map_err(|error| Failure::new(Cause::Unavailable, error))?;
    let response = scope
        .sandbox_manager_client
        .get_thread_workspace(scope.bearer, &lease.lease_id, &lease.backend_id)
        .await
        .inspect_err(|status| {
            warn!(code = ?status.code(), "sandbox-manager GetThreadWorkspace failed");
        })
        .map_err(|_| Failure::new(Cause::HydrateFailed, "the thread's files could not be read"))?;

    std::fs::create_dir_all(root).map_err(|error| {
        Failure::new(
            Cause::HydrateFailed,
            format!("could not create the thread's directory: {error}"),
        )
    })?;
    let path = root.join(mp_ids::new_ulid());
    std::fs::create_dir(&path).map_err(|error| {
        Failure::new(
            Cause::HydrateFailed,
            format!("could not create the thread's directory: {error}"),
        )
    })?;
    // The directory exists from here on, so every early return below drops the
    // workspace and removes it.
    let mut workspace = ThreadWorkspace {
        scope,
        lease_id: lease.lease_id.clone(),
        backend_id: lease.backend_id.clone(),
        prefix,
        path,
        baseline: HashMap::new(),
    };
    let entries: Vec<WorkspaceFileEntry> = response
        .entries
        .into_iter()
        .map(|entry| WorkspaceFileEntry {
            path: entry.path,
            content_hash: entry.content_hash,
        })
        .collect();
    workspace.hydrate(cas, &entries).await?;
    Ok(workspace)
}

impl ThreadWorkspace<'_> {
    /// The directory the call runs in.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    async fn hydrate(
        &mut self,
        cas: &CasClient,
        entries: &[WorkspaceFileEntry],
    ) -> Result<(), Failure> {
        for entry in entries {
            let relative = workspace_hydrate::safe_relative_path(&entry.path)
                .map_err(|error| Failure::new(Cause::HydrateFailed, error))?;
            let content = cas
                .get_scoped(&self.prefix, &entry.content_hash)
                .await
                .map_err(|error| Failure::new(Cause::HydrateFailed, error))?;
            let destination = self.path.join(&relative);
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent).map_err(|error| {
                    Failure::new(
                        Cause::HydrateFailed,
                        format!("could not create the thread's directory: {error}"),
                    )
                })?;
            }
            std::fs::write(&destination, &content).map_err(|error| {
                Failure::new(
                    Cause::HydrateFailed,
                    format!("could not write the thread's file: {error}"),
                )
            })?;
            self.baseline
                .insert(entry.path.clone(), entry.content_hash.clone());
        }
        Ok(())
    }

    /// Saves what this call created, changed or removed.
    ///
    /// Nothing is uploaded when the thread would exceed its ceiling, so a refused
    /// save leaves the thread as it was. A path another writer changed first comes
    /// back as a conflict and keeps that writer's content.
    ///
    /// # Errors
    /// [`Cause::QuotaExceeded`] when the thread would keep too much;
    /// [`Cause::CommitFailed`] when a file cannot be read, uploaded or recorded.
    pub async fn commit(&self) -> Result<Report, Failure> {
        let Some(cas) = self.scope.cas_client else {
            return Err(Failure::new(
                Cause::Unavailable,
                "no object store is configured for the thread's files",
            ));
        };
        let kept = self.kept_files()?;
        check_quota(kept.len(), kept.iter().map(|(_, size)| *size).sum())?;

        let mut changes = Vec::new();
        let mut redacted_paths = Vec::new();
        for (path, _) in &kept {
            let raw = std::fs::read(self.path.join(path)).map_err(|error| {
                Failure::new(
                    Cause::CommitFailed,
                    format!("could not read '{path}': {error}"),
                )
            })?;
            let base_hash = self.baseline.get(path);
            if base_hash == Some(&workspace_hydrate::content_version(&raw)) {
                continue;
            }
            if workspace_hydrate::would_redact(&raw) {
                redacted_paths.push(path.clone());
            }
            let content = workspace_hydrate::redact(&raw);
            let content_hash = cas
                .put_scoped(&self.prefix, &content)
                .await
                .map_err(|error| Failure::new(Cause::CommitFailed, error))?;
            changes.push(WorkspaceChangedFile {
                path: path.clone(),
                content_hash,
                size_bytes: i64::try_from(content.len()).unwrap_or(i64::MAX),
                base_hash: base_hash.cloned().unwrap_or_default(),
            });
        }

        let deleted: Vec<ThreadDeletedPath> =
            workspace_hydrate::diff_deletions(&self.path, &self.baseline)
                .into_iter()
                .map(|path| {
                    let base_hash = self.baseline.get(&path).cloned().unwrap_or_default();
                    ThreadDeletedPath { path, base_hash }
                })
                .collect();
        let changed = changes.len();
        let removed = deleted.len();
        if changed == 0 && removed == 0 {
            return Ok(Report {
                redacted_paths,
                ..Report::default()
            });
        }

        let response = self
            .scope
            .sandbox_manager_client
            .apply_thread_workspace_changes(
                self.scope.bearer,
                &self.lease_id,
                &self.backend_id,
                changes,
                deleted,
            )
            .await
            .inspect_err(|status| {
                warn!(code = ?status.code(), "sandbox-manager ApplyThreadWorkspaceChanges failed");
            })
            .map_err(|_| {
                Failure::new(
                    Cause::CommitFailed,
                    "the thread's changes could not be recorded",
                )
            })?;
        Ok(Report {
            conflicts: response.conflicting_paths,
            redacted_paths,
            changed,
            removed,
        })
    }

    /// The files this call's directory holds that the thread keeps, with their
    /// sizes, in path order.
    fn kept_files(&self) -> Result<Vec<(String, u64)>, Failure> {
        let mut relatives = Vec::new();
        workspace_hydrate::collect_relative_paths(&self.path, &self.path, &mut relatives)
            .map_err(|error| Failure::new(Cause::CommitFailed, error))?;
        let mut kept = Vec::new();
        for relative in relatives {
            let path = relative.to_string_lossy().replace('\\', "/");
            let size = std::fs::metadata(self.path.join(&relative))
                .map_err(|error| {
                    Failure::new(
                        Cause::CommitFailed,
                        format!("could not stat '{path}': {error}"),
                    )
                })?
                .len();
            if workspace_hydrate::is_syncable(&path, size) {
                kept.push((path, size));
            }
        }
        kept.sort();
        Ok(kept)
    }
}

impl Drop for ThreadWorkspace<'_> {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                warn!(%error, workspace = %self.path.display(), "thread workspace cleanup failed");
            }
        }
    }
}

/// Refuses a thread whose files would exceed either ceiling after this call. It
/// runs before any upload, so a refused call leaves the thread exactly as it was.
fn check_quota(files: usize, bytes: u64) -> Result<(), Failure> {
    if files > MAX_THREAD_FILES || bytes > MAX_THREAD_BYTES {
        return Err(Failure::new(
            Cause::QuotaExceeded,
            format!(
                "this thread's files would exceed the workspace limit of {MAX_THREAD_FILES} files and {MAX_THREAD_BYTES} bytes; nothing from this call was saved"
            ),
        ));
    }
    Ok(())
}

/// Adds the workspace part to a successful `code_interpreter` result. A call that
/// failed carries an error message rather than a result object and is returned as
/// it was.
#[must_use]
pub fn attach_workspace(mut execution: ToolExecution, workspace: Value) -> ToolExecution {
    if execution.error.is_some() {
        return execution;
    }
    if let Ok(Value::Object(mut result)) = serde_json::from_str::<Value>(&execution.output) {
        result.insert("workspace".to_owned(), workspace);
        if let Ok(output) = serde_json::to_string(&Value::Object(result)) {
            execution.output = output;
        }
    }
    execution
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};
    use std::net::SocketAddr;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use crate::code_interpreter::{Language, MAX_FILE_BYTES};

    use mp_contracts::model_plane::v1::sandbox_manager_server::{
        SandboxManager, SandboxManagerServer,
    };
    use mp_contracts::model_plane::v1::{
        AcquireLeaseRequest, AcquireLeaseResponse, ActivateLeaseRequest, ActivateLeaseResponse,
        AppendProcessOutputRequest, AppendProcessOutputResponse,
        ApplyThreadWorkspaceChangesRequest, ApplyThreadWorkspaceChangesResponse,
        DeleteThreadWorkspaceRequest, DeleteThreadWorkspaceResponse, GetProcessRequest,
        GetProcessResponse, GetThreadWorkspaceRequest, GetThreadWorkspaceResponse,
        GetWorkspaceManifestRequest, GetWorkspaceManifestResponse, ListProcessesRequest,
        ListProcessesResponse, PromoteWorkspaceRequest, PromoteWorkspaceResponse,
        ReadProcessOutputRequest, ReadProcessOutputResponse, ReconcileProcessesRequest,
        ReconcileProcessesResponse, RegisterProcessRequest, RegisterProcessResponse,
        ReleaseLeaseRequest, ReleaseLeaseResponse, SandboxHealthRequest, SandboxHealthResponse,
        SnapshotRequest, SnapshotResponse, ThreadWorkspaceEntry, UpdateProcessStateRequest,
        UpdateProcessStateResponse,
    };
    use tonic::{Request, Response, Status};
    use wiremock::matchers::any;
    use wiremock::{Mock, MockServer, Request as WireRequest, Respond, ResponseTemplate};

    use super::*;

    const ORG: &str = "org-1";
    const LEASE_ID: &str = "lease-thread-1";
    const BACKEND_ID: &str = "backend-1";

    /// The rows the fake sandbox-manager holds for thread workspaces: path to
    /// (content hash, size), and how many applies it has served.
    #[derive(Default)]
    struct ThreadRows {
        files: BTreeMap<String, (String, i64)>,
        applies: usize,
    }

    /// A sandbox-manager whose two thread RPCs follow the contract the real one
    /// implements: compare-and-swap per path on `base_hash`, a conflict is reported
    /// and never applied. Every other RPC is unused by these tests.
    #[derive(Clone, Default)]
    struct FakeSandboxManager {
        rows: Arc<Mutex<ThreadRows>>,
    }

    impl FakeSandboxManager {
        fn insert(&self, path: &str, hash: &str, size: i64) {
            self.rows
                .lock()
                .expect("fake rows")
                .files
                .insert(path.to_owned(), (hash.to_owned(), size));
        }

        fn applies(&self) -> usize {
            self.rows.lock().expect("fake rows").applies
        }

        fn paths(&self) -> Vec<String> {
            self.rows
                .lock()
                .expect("fake rows")
                .files
                .keys()
                .cloned()
                .collect()
        }
    }

    fn unused<T>() -> Result<Response<T>, Status> {
        Err(Status::unimplemented(
            "not used by the thread workspace tests",
        ))
    }

    #[tonic::async_trait]
    impl SandboxManager for FakeSandboxManager {
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

        async fn health(
            &self,
            _: Request<SandboxHealthRequest>,
        ) -> Result<Response<SandboxHealthResponse>, Status> {
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

        async fn get_thread_workspace(
            &self,
            request: Request<GetThreadWorkspaceRequest>,
        ) -> Result<Response<GetThreadWorkspaceResponse>, Status> {
            let request = request.into_inner();
            if request.lease_id != LEASE_ID {
                return Err(Status::not_found("unknown thread lease"));
            }
            let rows = self.rows.lock().expect("fake rows");
            let entries = rows
                .files
                .iter()
                .map(|(path, (hash, size))| ThreadWorkspaceEntry {
                    path: path.clone(),
                    content_hash: hash.clone(),
                    size_bytes: *size,
                })
                .collect();
            Ok(Response::new(GetThreadWorkspaceResponse {
                entries,
                revision: i64::try_from(rows.applies).unwrap_or(i64::MAX),
            }))
        }

        async fn apply_thread_workspace_changes(
            &self,
            request: Request<ApplyThreadWorkspaceChangesRequest>,
        ) -> Result<Response<ApplyThreadWorkspaceChangesResponse>, Status> {
            let request = request.into_inner();
            let mut rows = self.rows.lock().expect("fake rows");
            let mut conflicting_paths = Vec::new();
            for change in request.changes {
                let current = rows
                    .files
                    .get(&change.path)
                    .map(|(hash, _)| hash.clone())
                    .unwrap_or_default();
                if current != change.base_hash {
                    conflicting_paths.push(change.path);
                } else {
                    rows.files
                        .insert(change.path, (change.content_hash, change.size_bytes));
                }
            }
            for deleted in request.deleted {
                let current = rows
                    .files
                    .get(&deleted.path)
                    .map(|(hash, _)| hash.clone())
                    .unwrap_or_default();
                if current != deleted.base_hash {
                    conflicting_paths.push(deleted.path);
                } else {
                    rows.files.remove(&deleted.path);
                }
            }
            rows.applies += 1;
            Ok(Response::new(ApplyThreadWorkspaceChangesResponse {
                conflicting_paths,
                revision: i64::try_from(rows.applies).unwrap_or(i64::MAX),
                file_count: i64::try_from(rows.files.len()).unwrap_or(i64::MAX),
                total_bytes: rows.files.values().map(|(_, size)| *size).sum(),
            }))
        }

        async fn delete_thread_workspace(
            &self,
            _: Request<DeleteThreadWorkspaceRequest>,
        ) -> Result<Response<DeleteThreadWorkspaceResponse>, Status> {
            unused()
        }
    }

    /// Serves the fake on a local port and returns a client connected to it.
    async fn start_fake(fake: FakeSandboxManager) -> SandboxManagerClient {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind a local port");
        let address: SocketAddr = listener.local_addr().expect("local address");
        tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(SandboxManagerServer::new(fake))
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await
                .expect("the fake sandbox-manager serves");
        });
        let channel = tonic::transport::Endpoint::from_shared(format!("http://{address}"))
            .expect("a valid endpoint")
            .connect()
            .await
            .expect("connect to the fake");
        SandboxManagerClient::new(channel)
    }

    /// A content-addressed object store in memory, behind a local HTTP server.
    #[derive(Clone, Default)]
    struct CasFake {
        objects: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    }

    impl Respond for CasFake {
        fn respond(&self, request: &WireRequest) -> ResponseTemplate {
            let key = request.url.path().to_owned();
            let mut objects = self.objects.lock().expect("fake object store");
            match request.method.as_str() {
                "PUT" => {
                    objects.insert(key, request.body.clone());
                    ResponseTemplate::new(200)
                }
                "HEAD" if objects.contains_key(&key) => ResponseTemplate::new(200),
                "GET" => match objects.get(&key) {
                    Some(body) => ResponseTemplate::new(200).set_body_bytes(body.clone()),
                    None => ResponseTemplate::new(404),
                },
                _ => ResponseTemplate::new(404),
            }
        }
    }

    struct Rig {
        fake: FakeSandboxManager,
        client: SandboxManagerClient,
        cas_client: CasClient,
        _cas: MockServer,
    }

    async fn rig() -> Rig {
        let fake = FakeSandboxManager::default();
        let client = start_fake(fake.clone()).await;
        let cas = MockServer::start().await;
        Mock::given(any())
            .respond_with(CasFake::default())
            .mount(&cas)
            .await;
        let cas_client = CasClient::new_with_transport(
            &cas.uri(),
            "test-bucket",
            "access-key",
            &"a".repeat(32),
            "us-east-1",
        )
        .expect("valid transport config");
        Rig {
            fake,
            client,
            cas_client,
            _cas: cas,
        }
    }

    fn scope<'a>(rig: &'a Rig, thread_id: &'a str) -> ThreadScope<'a> {
        ThreadScope {
            thread_id,
            bearer: "user-bearer",
            backend_id: BACKEND_ID,
            sandbox_manager_client: &rig.client,
            cas_client: Some(&rig.cas_client),
        }
    }

    fn lease() -> SandboxLease {
        SandboxLease {
            lease_id: LEASE_ID.to_owned(),
            backend_id: BACKEND_ID.to_owned(),
            processes_permitted: false,
        }
    }

    fn fresh_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "execution-core-thread-workspace-{name}-{}",
            std::process::id()
        ));
        std::fs::remove_dir_all(&root).ok();
        root
    }

    /// Opens the thread, writes each file at the top of its directory, saves, and
    /// returns what the save reported.
    async fn save(rig: &Rig, root: &Path, thread_id: &str, files: &[(&str, &[u8])]) -> Report {
        let workspace = open_in(root, scope(rig, thread_id), &lease(), ORG, false)
            .await
            .expect("the thread opens");
        for (name, bytes) in files {
            std::fs::write(workspace.path().join(name), bytes).expect("the call writes its file");
        }
        workspace.commit().await.expect("the call saves")
    }

    #[tokio::test]
    async fn second_call_sees_first_calls_file() {
        let rig = rig().await;
        let root = fresh_root("second-call");
        save(
            &rig,
            &root,
            "thread-1",
            &[("notes.txt", b"written by the first call")],
        )
        .await;

        let second = open_in(&root, scope(&rig, "thread-1"), &lease(), ORG, false)
            .await
            .expect("the second call opens");
        assert_eq!(
            std::fs::read(second.path().join("notes.txt")).expect("the file is hydrated"),
            b"written by the first call"
        );
        drop(second);
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn deleted_file_is_recorded() {
        let rig = rig().await;
        let root = fresh_root("deleted");
        save(&rig, &root, "thread-1", &[("a.txt", b"a"), ("b.txt", b"b")]).await;

        let second = open_in(&root, scope(&rig, "thread-1"), &lease(), ORG, false)
            .await
            .expect("the second call opens");
        std::fs::remove_file(second.path().join("a.txt")).expect("the call removes its file");
        let report = second.commit().await.expect("the removal saves");

        assert_eq!(report.removed, 1);
        assert_eq!(rig.fake.paths(), vec!["b.txt".to_owned()]);
        drop(second);
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn scratch_and_program_files_are_not_synced() {
        let rig = rig().await;
        let root = fresh_root("scratch");
        let workspace = open_in(&root, scope(&rig, "thread-1"), &lease(), ORG, false)
            .await
            .expect("the thread opens");
        let dir = workspace.path().to_path_buf();
        for sub in [".tmp", ".mplconfig", "attachments", "pkg/__pycache__"] {
            std::fs::create_dir_all(dir.join(sub)).expect("the scratch directory");
        }
        std::fs::write(dir.join(Language::Python.program_file()), b"print(1)")
            .expect("the program");
        std::fs::write(dir.join(".tmp/scratch.txt"), b"tmp").expect("scratch");
        std::fs::write(dir.join(".mplconfig/fontlist.json"), b"{}").expect("matplotlib cache");
        std::fs::write(dir.join("pkg/__pycache__/m.pyc"), b"\0").expect("bytecode");
        std::fs::write(dir.join("attachments/input.csv"), b"a,b").expect("attachment");
        std::fs::write(
            dir.join("big.bin"),
            vec![0u8; usize::try_from(MAX_FILE_BYTES + 1).expect("fits in memory")],
        )
        .expect("an oversized file");
        std::fs::write(dir.join("keep.txt"), b"keep").expect("a kept file");

        workspace.commit().await.expect("the call saves");

        assert_eq!(rig.fake.paths(), vec!["keep.txt".to_owned()]);
        drop(workspace);
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn conflict_is_reported_not_overwritten() {
        let rig = rig().await;
        let root = fresh_root("conflict");
        save(&rig, &root, "thread-1", &[("notes.txt", b"old")]).await;

        // Both calls open on the same version; the first to save wins.
        let mine = open_in(&root, scope(&rig, "thread-1"), &lease(), ORG, false)
            .await
            .expect("the second writer opens");
        let theirs = open_in(&root, scope(&rig, "thread-1"), &lease(), ORG, false)
            .await
            .expect("the first writer opens");
        std::fs::write(theirs.path().join("notes.txt"), b"theirs").expect("their edit");
        theirs.commit().await.expect("the first writer saves");
        drop(theirs);

        std::fs::write(mine.path().join("notes.txt"), b"mine").expect("my edit");
        let report = mine.commit().await.expect("the second save completes");
        assert_eq!(report.conflicts, vec!["notes.txt".to_owned()]);
        assert_eq!(report.to_json()["state"], "conflict");

        let reopened = open_in(&root, scope(&rig, "thread-1"), &lease(), ORG, false)
            .await
            .expect("the thread reopens");
        assert_eq!(
            std::fs::read(reopened.path().join("notes.txt")).expect("the file is hydrated"),
            b"theirs",
            "the thread keeps the other writer's content"
        );
        drop(reopened);
        drop(mine);
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn drop_removes_the_directory_on_every_path() {
        let rig = rig().await;
        let root = fresh_root("drop");
        let workspace = open_in(&root, scope(&rig, "thread-1"), &lease(), ORG, false)
            .await
            .expect("the thread opens");
        let path = workspace.path().to_path_buf();
        assert!(path.is_dir());
        drop(workspace);
        assert!(
            !path.exists(),
            "a dropped workspace leaves no directory behind"
        );

        // A stored file the object store does not hold: hydration fails part-way.
        rig.fake
            .insert("missing.txt", &format!("sha256:{}", "0".repeat(64)), 1);
        let failure = open_in(&root, scope(&rig, "thread-1"), &lease(), ORG, false)
            .await
            .err()
            .expect("the open fails");
        assert_eq!(failure.cause, Cause::HydrateFailed);
        assert_eq!(
            std::fs::read_dir(&root).expect("the root exists").count(),
            0,
            "a failed open leaves nothing behind"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn files_over_quota_are_refused_with_a_typed_cause() {
        let rig = rig().await;
        let root = fresh_root("quota");
        let workspace = open_in(&root, scope(&rig, "thread-1"), &lease(), ORG, false)
            .await
            .expect("the thread opens");
        for index in 0..=MAX_THREAD_FILES {
            std::fs::write(workspace.path().join(format!("f{index}.txt")), b"x")
                .expect("the call writes its file");
        }

        let failure = workspace.commit().await.expect_err("the save is refused");
        assert_eq!(failure.cause, Cause::QuotaExceeded);
        assert_eq!(failure.to_json()["cause"], "quota_exceeded");
        assert!(
            rig.fake.paths().is_empty() && rig.fake.applies() == 0,
            "a refused save uploads and records nothing"
        );
        drop(workspace);
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn a_redacted_file_is_reported_not_silently_altered() {
        let rig = rig().await;
        let root = fresh_root("redacted");
        let report = save(
            &rig,
            &root,
            "thread-1",
            &[("config.txt", b"api_key: \"xyz123456\"")],
        )
        .await;
        assert_eq!(report.redacted_paths, vec!["config.txt".to_owned()]);
        assert_eq!(report.to_json()["redacted_paths"][0], "config.txt");

        let reopened = open_in(&root, scope(&rig, "thread-1"), &lease(), ORG, false)
            .await
            .expect("the thread reopens");
        let stored = std::fs::read_to_string(reopened.path().join("config.txt"))
            .expect("the file is hydrated");
        assert!(
            !stored.contains("xyz123456"),
            "the thread stores the scrubbed text"
        );
        assert_eq!(stored, crate::scrub::scrub_string("api_key: \"xyz123456\""));
        drop(reopened);
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn an_unchanged_thread_sends_nothing_to_sandbox_manager() {
        let rig = rig().await;
        let root = fresh_root("unchanged");
        save(&rig, &root, "thread-1", &[("notes.txt", b"same")]).await;
        let applies = rig.fake.applies();

        let report = save(&rig, &root, "thread-1", &[]).await;

        assert_eq!(report, Report::default());
        assert_eq!(rig.fake.applies(), applies);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_quota_is_a_file_count_and_a_byte_ceiling() {
        assert!(check_quota(MAX_THREAD_FILES, MAX_THREAD_BYTES).is_ok());
        assert_eq!(
            check_quota(MAX_THREAD_FILES + 1, 0)
                .err()
                .map(|failure| failure.cause),
            Some(Cause::QuotaExceeded)
        );
        assert_eq!(
            check_quota(1, MAX_THREAD_BYTES + 1)
                .err()
                .map(|failure| failure.cause),
            Some(Cause::QuotaExceeded)
        );
    }

    #[test]
    fn the_workspace_part_joins_a_result_and_never_a_failure() {
        let ok = attach_workspace(
            ToolExecution {
                output: "{\"stdout\":\"hi\"}".to_owned(),
                error: None,
            },
            json!({ "scope": "thread", "state": "synced" }),
        );
        let result: Value = serde_json::from_str(&ok.output).expect("the output stays JSON");
        assert_eq!(result["workspace"]["state"], "synced");
        assert_eq!(result["stdout"], "hi");

        let failed = attach_workspace(
            ToolExecution {
                output: String::new(),
                error: Some("the program could not start".to_owned()),
            },
            json!({ "scope": "thread", "state": "synced" }),
        );
        assert!(failed.output.is_empty());
        assert_eq!(failed.error.as_deref(), Some("the program could not start"));
    }
}
