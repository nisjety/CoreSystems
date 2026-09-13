//! S4.2 step 4 — the OS side of background processes.
//!
//! sandbox-manager owns the rows; this module owns the actual child process:
//! it spawns under the same bwrap argv every other sandboxed call uses, pipes
//! its stdio, batches and redacts its output into the registry, delivers
//! signals, enforces the TTL, and reports every transition. It persists
//! nothing itself and decides no authority — `RegisterProcess` is what admits
//! a process, and this module treats its refusal as final.
//!
//! **A process does not survive this process.** Every child runs under
//! bwrap's `--die-with-parent`, so a restart kills them. That is deliberate
//! (it is what stops a crashed worker leaking sandboxes), and it is why
//! "durable" in S4.2 means the registry stays truthful rather than that the
//! work continues: [`ProcessHost::reconcile`] tells the registry its
//! predecessor's children are gone, and the registry's own staleness sweeper
//! is the backstop if this host never gets to say so.
//!
//! Disabled unless `EXECUTION_CORE_PROCESS_HOST=enabled`, and even then only
//! where bwrap actually works — [`crate::sandbox::capability_profile`] reports
//! `background_registry` under exactly those two conditions, so what this host
//! advertises to Control is what it can really do.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use mp_contracts::model_plane::v1::{ProcessOutputChunk, ProcessSignal, ProcessStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::ChildStdin;
use tokio::sync::{mpsc, Mutex, OnceCell};
use tonic::Status;
use tracing::{info, warn};

use crate::policy::{MpNetworkPolicy, MpSandboxPolicy};
use crate::sandbox_lease::SandboxManagerTokenProvider;
use crate::sandbox_manager_client::{
    ProcessFence, ProcessOutcome, ProcessRegistration, RedactedArgv, SandboxManagerClient,
};

/// How often the output pump flushes whatever complete lines it has. Also the
/// granularity of the heartbeat below. 250ms is short enough that a reader
/// tailing a process sees it as live, and long enough that a chatty process
/// costs one RPC per tick rather than one per line.
const FLUSH_INTERVAL: Duration = Duration::from_millis(250);

/// Ticks between heartbeats when a process has produced nothing. The registry
/// declares a host lost after 60s of silence, so 15s leaves room for three
/// missed beats before anything is concluded.
const HEARTBEAT_TICKS: u32 = 60;

/// A line longer than this is flushed without its terminator rather than
/// buffered forever. The residual risk is stated rather than hidden: a secret
/// spanning the cut can no longer be matched by a scrub pattern, which is
/// exactly why the cut is this large and why every other flush is at a line
/// boundary.
const MAX_LINE_BYTES: usize = 64 * 1024;

/// How long a process gets to handle SIGTERM before it is killed outright.
const TERM_GRACE: Duration = Duration::from_secs(10);

/// Longest TTL a caller may ask for. The registry clamps to the lease's own
/// expiry as well; this is the local ceiling.
const MAX_TTL: Duration = Duration::from_hours(1);

/// Default TTL when a caller names none.
const DEFAULT_TTL: Duration = Duration::from_mins(30);

/// Total output one process may produce before it is stopped. The registry's
/// head+tail retention keeps this readable, but an unbounded logger is a
/// resource bug rather than a use case.
const MAX_TOTAL_OUTPUT_BYTES: i64 = 64 * 1024 * 1024;

/// Where a process's staged program and scratch files live — outside the
/// Space workspace, so a background process's own scaffolding never shows up
/// as workspace content to be diffed and promoted.
fn scratch_dir(space_id: &str, process_id: &str) -> PathBuf {
    std::env::temp_dir()
        .join("verevon-space-processes")
        .join(crate::code_interpreter::path_slug(space_id))
        .join(crate::code_interpreter::path_slug(process_id))
}

/// A program to write into the process's scratch directory before spawning.
/// Staged as a file rather than passed with `-c` for the same reasons
/// `code_interpreter` does it: real filenames in tracebacks, and no argv size
/// ceiling.
pub struct StagedProgram<'a> {
    pub filename: &'a str,
    pub source: &'a str,
}

/// One request to start a background process.
pub struct StartRequest<'a> {
    pub org_id: &'a str,
    pub space_id: &'a str,
    pub lease_id: &'a str,
    pub run_id: &'a str,
    pub step_id: &'a str,
    pub subject_id: &'a str,
    /// The lease's hydrated Space workspace; becomes the child's cwd and its
    /// only writable root.
    pub workspace: &'a Path,
    pub program: &'a str,
    pub args: &'a [String],
    /// Written into the scratch directory and prepended to `args` as an
    /// absolute path.
    pub staged: Option<StagedProgram<'a>>,
    pub ttl: Option<Duration>,
    /// Whether the child's stdin stays open for later writes.
    pub stdin: bool,
}

/// What a caller gets back from a successful start.
pub struct StartedProcess {
    pub process_id: String,
    pub expires_at: Option<std::time::SystemTime>,
}

/// Requests the supervisor accepts while its child runs.
enum Control {
    Signal(ProcessSignal),
}

struct LiveProcess {
    lease_id: String,
    /// `None` once closed, or when the caller never asked for stdin.
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    control: mpsc::Sender<Control>,
}

/// The OS-side host for one execution-core instance.
pub struct ProcessHost {
    backend_id: Arc<str>,
    /// This boot's identity. Every write this host makes carries it, so a
    /// predecessor's writes are refused the moment a successor registers.
    host_epoch: Arc<str>,
    client: SandboxManagerClient,
    tokens: Arc<SandboxManagerTokenProvider>,
    live: Arc<DashMap<String, LiveProcess>>,
    /// Reconcile runs once, on first use — see [`ProcessHost::reconcile`].
    reconciled: Arc<OnceCell<()>>,
}

impl ProcessHost {
    /// Construct a host, or `None` when background processes are not enabled
    /// on this instance. `None` is the ordinary case: the flag defaults off,
    /// and an instance without it advertises `bounded_oneshot`.
    pub fn from_env(
        client: SandboxManagerClient,
        tokens: Arc<SandboxManagerTokenProvider>,
        backend_id: &str,
    ) -> Option<Self> {
        if !crate::sandbox::process_host_enabled() {
            return None;
        }
        if !crate::sandbox::is_supported() {
            // Advertising and then refusing would be worse than not
            // advertising: capability_profile() already reports
            // bounded_oneshot in this case, so this branch only guards
            // against constructing a host nothing could legally use.
            warn!("EXECUTION_CORE_PROCESS_HOST is enabled but bubblewrap is unavailable; background processes stay off");
            return None;
        }
        Some(Self {
            backend_id: Arc::from(backend_id),
            host_epoch: Arc::from(mp_ids::new_ulid().as_str()),
            client,
            tokens,
            live: Arc::new(DashMap::new()),
            reconciled: Arc::new(OnceCell::new()),
        })
    }

    /// This boot's epoch, which is also the registry's write fence.
    #[must_use]
    pub fn host_epoch(&self) -> &str {
        &self.host_epoch
    }

    /// Whether this instance is the one hosting `process_id` right now.
    /// Signal and stdin are local-only operations; a process on another
    /// backend can be read and listed but not controlled.
    #[must_use]
    pub fn is_local(&self, process_id: &str) -> bool {
        self.live.contains_key(process_id)
    }

    /// Tell the registry that every live row this backend owns under an older
    /// epoch belongs to a host that is gone.
    ///
    /// Runs once, lazily, on the first operation that has an organization to
    /// mint a service token for. The design called for this at boot, but a
    /// freshly started host has no organization yet and the token provider is
    /// per-org, so a boot-time call would have had to invent one. Lazy is
    /// honest and costs little: the registry's staleness sweeper already
    /// marks a silent host's rows lost within its own window, and this makes
    /// it immediate and exact (by epoch) rather than eventual (by clock) for
    /// the case that actually matters — a restarted host taking over work in
    /// a Space someone is still watching.
    async fn reconcile(&self, org_id: &str) {
        let already = self.reconciled.clone();
        let backend_id = self.backend_id.clone();
        let host_epoch = self.host_epoch.clone();
        let tokens = self.tokens.clone();
        let client = self.client.clone();
        already
            .get_or_init(|| async move {
                let Ok(token) = tokens.token(org_id).await else {
                    warn!("no sandbox-manager credential; skipping process reconcile");
                    return;
                };
                match client
                    .reconcile_processes(&token, &backend_id, &host_epoch)
                    .await
                {
                    Ok(response) if response.lost_count > 0 => {
                        info!(
                            lost = response.lost_count,
                            "marked a previous host epoch's background processes lost"
                        );
                    }
                    Ok(_) => {}
                    Err(status) => {
                        warn!(code = ?status.code(), "process reconcile failed; the registry's sweeper remains the backstop");
                    }
                }
            })
            .await;
    }

    /// Register, spawn, and begin supervising a background process.
    ///
    /// Ordering is deliberate: the registry reserves the row FIRST, so lease
    /// eligibility, the TTL clamp and the live-count limits are all decided
    /// before anything runs. A spawn failure after that is reported as a
    /// terminal `spawn_failed` rather than left as a row that never started.
    ///
    /// # Errors
    /// Whatever `RegisterProcess` refused with — `permission_denied` when the
    /// Space was never granted `space:processes`, `resource_exhausted` at the
    /// limit, `failed_precondition` for an ineligible lease — or `internal`
    /// when the scratch directory or program file cannot be written.
    pub async fn start(&self, request: &StartRequest<'_>) -> Result<StartedProcess, Status> {
        self.reconcile(request.org_id).await;

        let ttl = request.ttl.unwrap_or(DEFAULT_TTL).min(MAX_TTL);
        let ttl_seconds = i32::try_from(ttl.as_secs())
            .map_err(|_| Status::invalid_argument("ttl is out of range"))?;
        let process_id = mp_ids::new_ulid();
        let token = self.tokens.token(request.org_id).await.map_err(|error| {
            warn!(%error, "sandbox-manager service credential unavailable");
            Status::unavailable("sandbox-manager service credential unavailable")
        })?;

        // Stage before registering only in memory: nothing is written to disk
        // until the registry has admitted the process.
        let (program, args) = Self::resolve_argv(request, &process_id);
        let digest = command_digest(&program, &args);
        let (redacted_program, redacted_args) = crate::scrub::redact_command(&program, &args);

        let registered = self
            .client
            .register_process(
                &token,
                &ProcessRegistration {
                    process_id: &process_id,
                    lease_id: request.lease_id,
                    backend_id: &self.backend_id,
                    host_epoch: &self.host_epoch,
                    run_id: request.run_id,
                    step_id: request.step_id,
                    subject_id: request.subject_id,
                    command: RedactedArgv {
                        program: &redacted_program,
                        args: &redacted_args.iter().map(String::as_str).collect::<Vec<_>>(),
                    },
                    command_digest: &digest,
                    ttl_seconds,
                },
            )
            .await?;

        let fence = OwnedFence {
            process_id: process_id.clone(),
            backend_id: self.backend_id.to_string(),
            host_epoch: self.host_epoch.to_string(),
        };

        match self.spawn(request, &process_id, &program, &args).await {
            Ok(spawned) => {
                self.supervise(request, &token, fence, spawned).await;
                Ok(StartedProcess {
                    process_id,
                    expires_at: registered
                        .process
                        .and_then(|p| p.expires_at)
                        .and_then(|t| std::time::SystemTime::try_from(t).ok()),
                })
            }
            Err(error) => {
                // The row exists and would otherwise sit in STARTING forever.
                let outcome = ProcessOutcome {
                    state: mp_contracts::model_plane::v1::ProcessState::Exited,
                    exit_code: None,
                    end_reason: "spawn_failed",
                    cleanup_done: true,
                };
                if let Err(status) = self
                    .client
                    .mark_process_ended(&token, &fence.borrow(), &outcome)
                    .await
                {
                    warn!(code = ?status.code(), "could not report a spawn failure");
                }
                let _ = tokio::fs::remove_dir_all(scratch_dir(request.space_id, &process_id)).await;
                Err(Status::internal(format!(
                    "background process could not be started: {error}"
                )))
            }
        }
    }

    /// Write to a running process's stdin. Local-only: a process hosted by
    /// another backend can be read, but not fed.
    ///
    /// # Errors
    /// `failed_precondition` when the process is not on this host or its
    /// stdin is closed; `internal` on a write error.
    pub async fn write_stdin(
        &self,
        process_id: &str,
        data: &[u8],
        close: bool,
    ) -> Result<usize, Status> {
        let handle = {
            let entry = self.live.get(process_id).ok_or_else(not_local)?;
            entry.stdin.clone()
        };
        let mut guard = handle.lock().await;
        let stdin = guard
            .as_mut()
            .ok_or_else(|| Status::failed_precondition("the process's stdin is closed"))?;
        stdin
            .write_all(data)
            .await
            .map_err(|error| Status::internal(format!("write to process stdin: {error}")))?;
        stdin
            .flush()
            .await
            .map_err(|error| Status::internal(format!("flush process stdin: {error}")))?;
        if close {
            *guard = None;
        }
        Ok(data.len())
    }

    /// Ask a running process to stop. Local-only, for the same reason as
    /// [`Self::write_stdin`].
    ///
    /// # Errors
    /// `failed_precondition` when the process is not on this host.
    pub async fn signal(&self, process_id: &str, signal: ProcessSignal) -> Result<(), Status> {
        let control = {
            let entry = self.live.get(process_id).ok_or_else(not_local)?;
            entry.control.clone()
        };
        control
            .send(Control::Signal(signal))
            .await
            .map_err(|_| Status::failed_precondition("the process has already stopped"))
    }

    /// Stop every process this host is running for a lease, used when the
    /// lease is released. Best effort: the registry marks the rows killed
    /// regardless, and a child that outlives this call dies with the host.
    pub async fn kill_for_lease(&self, lease_id: &str) {
        let targets: Vec<String> = self
            .live
            .iter()
            .filter(|entry| entry.lease_id == lease_id)
            .map(|entry| entry.key().clone())
            .collect();
        for process_id in targets {
            let _ = self.signal(&process_id, ProcessSignal::Kill).await;
        }
    }

    fn resolve_argv(request: &StartRequest<'_>, process_id: &str) -> (String, Vec<String>) {
        let mut args = Vec::with_capacity(request.args.len() + 1);
        if let Some(staged) = &request.staged {
            args.push(
                scratch_dir(request.space_id, process_id)
                    .join(staged.filename)
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        args.extend(request.args.iter().cloned());
        (request.program.to_owned(), args)
    }

    async fn spawn(
        &self,
        request: &StartRequest<'_>,
        process_id: &str,
        program: &str,
        args: &[String],
    ) -> std::io::Result<SpawnedChild> {
        let scratch = scratch_dir(request.space_id, process_id);
        tokio::fs::create_dir_all(&scratch).await?;
        if let Some(staged) = &request.staged {
            tokio::fs::write(scratch.join(staged.filename), staged.source).await?;
        }

        // The same policy code_interpreter uses: the Space workspace is the
        // only writable root, and the network namespace stays down.
        let policy = MpSandboxPolicy::WorkspaceWrite {
            writable_roots: vec![request.workspace.to_path_buf(), scratch.clone()],
            network: MpNetworkPolicy::Disabled,
        };
        let env = crate::code_interpreter::child_env_for(request.workspace);
        let options = crate::sandbox::LaunchOptions {
            cwd: Some(request.workspace),
            env: crate::sandbox::SandboxEnv::Only(&env),
            ..crate::sandbox::LaunchOptions::default()
        };
        let wrapped = crate::sandbox::wrap_command_with(&policy, program, args, options);
        // Identical fail-closed gate to every other sandboxed spawn: a host
        // that cannot isolate does not run model-authored code at all.
        crate::executor::require_requested_isolation_for(&policy, wrapped.sandboxed)?;

        let mut command = tokio::process::Command::new(&wrapped.program);
        command
            .args(&wrapped.args)
            .current_dir(request.workspace)
            .env_clear()
            .envs(env.iter().map(|(k, v)| (k, v)))
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(if request.stdin {
                std::process::Stdio::piped()
            } else {
                std::process::Stdio::null()
            })
            // Belt and braces alongside --die-with-parent: if this task is
            // dropped the child does not outlive it.
            .kill_on_drop(true);
        let mut child = command.spawn()?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let stdin = child.stdin.take();
        Ok(SpawnedChild {
            child,
            stdout,
            stderr,
            stdin,
            scratch,
        })
    }

    async fn supervise(
        &self,
        request: &StartRequest<'_>,
        token: &str,
        fence: OwnedFence,
        spawned: SpawnedChild,
    ) {
        let (control_tx, control_rx) = mpsc::channel(8);
        let stdin = Arc::new(Mutex::new(spawned.stdin));
        self.live.insert(
            fence.process_id.clone(),
            LiveProcess {
                lease_id: request.lease_id.to_owned(),
                stdin: stdin.clone(),
                control: control_tx,
            },
        );

        let _ = self
            .client
            .mark_process_started(token, &fence.borrow())
            .await;

        let supervisor = Supervisor {
            client: self.client.clone(),
            tokens: self.tokens.clone(),
            live: self.live.clone(),
            org_id: request.org_id.to_owned(),
            fence,
            ttl: request.ttl.unwrap_or(DEFAULT_TTL).min(MAX_TTL),
            scratch: spawned.scratch,
        };
        tokio::spawn(supervisor.run(spawned.child, spawned.stdout, spawned.stderr, control_rx));
    }
}

fn not_local() -> Status {
    Status::failed_precondition(
        "the process is not running on this host; signal and stdin are local operations",
    )
}

struct SpawnedChild {
    child: tokio::process::Child,
    stdout: Option<tokio::process::ChildStdout>,
    stderr: Option<tokio::process::ChildStderr>,
    stdin: Option<ChildStdin>,
    scratch: PathBuf,
}

/// An owned copy of the write fence, so the supervisor task does not borrow
/// the host.
struct OwnedFence {
    process_id: String,
    backend_id: String,
    host_epoch: String,
}

impl OwnedFence {
    fn borrow(&self) -> ProcessFence<'_> {
        ProcessFence {
            process_id: &self.process_id,
            backend_id: &self.backend_id,
            host_epoch: &self.host_epoch,
        }
    }
}

struct Supervisor {
    client: SandboxManagerClient,
    tokens: Arc<SandboxManagerTokenProvider>,
    live: Arc<DashMap<String, LiveProcess>>,
    org_id: String,
    fence: OwnedFence,
    ttl: Duration,
    scratch: PathBuf,
}

impl Supervisor {
    async fn run(
        self,
        mut child: tokio::process::Child,
        stdout: Option<tokio::process::ChildStdout>,
        stderr: Option<tokio::process::ChildStderr>,
        mut control: mpsc::Receiver<Control>,
    ) {
        let (out_tx, out_rx) = mpsc::channel::<Incoming>(64);
        if let Some(stdout) = stdout {
            tokio::spawn(pump(stdout, ProcessStream::Stdout, out_tx.clone()));
        }
        if let Some(stderr) = stderr {
            tokio::spawn(pump(stderr, ProcessStream::Stderr, out_tx.clone()));
        }
        drop(out_tx);

        let pump_handle = tokio::spawn(flush_loop(
            self.client.clone(),
            self.tokens.clone(),
            self.org_id.clone(),
            OwnedFence {
                process_id: self.fence.process_id.clone(),
                backend_id: self.fence.backend_id.clone(),
                host_epoch: self.fence.host_epoch.clone(),
            },
            out_rx,
        ));

        let monitor_pid = child.id();
        let ttl_deadline = tokio::time::Instant::now() + self.ttl;
        let mut grace: Option<tokio::time::Instant> = None;
        let mut end_reason = "exited";

        let status = loop {
            tokio::select! {
                result = child.wait() => break result,
                request = control.recv() => match request {
                    Some(Control::Signal(ProcessSignal::Term)) => {
                        deliver_term(monitor_pid);
                        end_reason = "signaled";
                        grace = Some(tokio::time::Instant::now() + TERM_GRACE);
                    }
                    Some(Control::Signal(_)) => {
                        let _ = child.start_kill();
                        end_reason = "signaled";
                    }
                    None => {
                        // Every sender is gone, which cannot happen while the
                        // host holds one; wait the child out rather than spin.
                        break child.wait().await;
                    }
                },
                () = sleep_until_maybe(grace) => {
                    let _ = child.start_kill();
                    grace = None;
                }
                () = tokio::time::sleep_until(ttl_deadline), if grace.is_none() => {
                    deliver_term(monitor_pid);
                    end_reason = "ttl_expired";
                    grace = Some(tokio::time::Instant::now() + TERM_GRACE);
                }
            }
        };

        self.live.remove(&self.fence.process_id);
        let _ = pump_handle.await;
        let _ = tokio::fs::remove_dir_all(&self.scratch).await;

        let exit_code = status
            .as_ref()
            .ok()
            .and_then(std::process::ExitStatus::code);
        let state = match (end_reason, exit_code) {
            ("ttl_expired", _) => mp_contracts::model_plane::v1::ProcessState::Expired,
            // A process that handled SIGTERM and exited cleanly still ended
            // because it was asked to, so it reads as killed rather than as
            // having finished its work.
            ("signaled", _) => mp_contracts::model_plane::v1::ProcessState::Killed,
            _ => mp_contracts::model_plane::v1::ProcessState::Exited,
        };
        let Ok(token) = self.tokens.token(&self.org_id).await else {
            warn!(process_id = %self.fence.process_id, "no credential to report the process outcome; the registry's sweeper will mark it lost");
            return;
        };
        if let Err(status) = self
            .client
            .mark_process_ended(
                &token,
                &self.fence.borrow(),
                &ProcessOutcome {
                    state,
                    exit_code,
                    end_reason,
                    cleanup_done: true,
                },
            )
            .await
        {
            warn!(code = ?status.code(), "could not report the process outcome");
        }
    }
}

/// A future that never completes when there is no deadline, so it can sit in
/// a `select!` arm unconditionally.
async fn sleep_until_maybe(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

/// Deliver SIGTERM to the sandboxed command.
///
/// The command is two `/proc` hops below the process we spawned: our child is
/// bwrap's monitor, which forks the sandbox init, which forks the command.
/// Signalling the monitor does NOT reach the command — verified directly
/// against bubblewrap 0.8.0 (`scripts/verify-sandbox-isolation.sh`'s
/// process-signal probe), where a `trap TERM` in the command fired only for
/// the resolved pid and never for the monitor. So this resolves the pid and
/// signals it, and the KILL escalation behind it is what makes the contract
/// hold regardless: SIGKILL to the monitor tears down the whole namespace,
/// which the same probe confirms leaves nothing alive.
#[cfg(unix)]
fn deliver_term(monitor_pid: Option<u32>) {
    let Some(monitor) = monitor_pid else { return };
    let Some(command) = resolve_sandboxed_pid(monitor) else {
        warn!(
            monitor,
            "could not resolve the sandboxed command's pid; escalating on the grace timer instead"
        );
        return;
    };
    let Ok(pid) = i32::try_from(command) else {
        return;
    };
    if let Err(error) = nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(pid),
        nix::sys::signal::Signal::SIGTERM,
    ) {
        warn!(%error, command, "SIGTERM to the sandboxed command failed");
    }
}

#[cfg(not(unix))]
fn deliver_term(_monitor_pid: Option<u32>) {
    // No sandbox on this platform, so nothing here can be reached anyway;
    // the grace timer's SIGKILL is the whole contract.
}

/// Walk `/proc/<pid>/task/<pid>/children` twice: monitor → sandbox init →
/// command.
#[cfg(unix)]
fn resolve_sandboxed_pid(monitor: u32) -> Option<u32> {
    let init = first_child(monitor)?;
    // A command that is itself the init (no further fork) is the terminal
    // case, not a failure.
    Some(first_child(init).unwrap_or(init))
}

#[cfg(unix)]
fn first_child(pid: u32) -> Option<u32> {
    let raw = std::fs::read_to_string(format!("/proc/{pid}/task/{pid}/children")).ok()?;
    raw.split_whitespace().next()?.parse().ok()
}

enum Incoming {
    Bytes(ProcessStream, Vec<u8>),
}

/// Read one stream to EOF, forwarding raw bytes. Line splitting, redaction
/// and sequencing all happen in one place downstream, so the two streams
/// cannot disagree about the seq counter.
async fn pump<R>(mut reader: R, stream: ProcessStream, tx: mpsc::Sender<Incoming>)
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut buffer = [0_u8; 8192];
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(read) => {
                if tx
                    .send(Incoming::Bytes(stream, buffer[..read].to_vec()))
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }
    }
}

async fn flush_loop(
    client: SandboxManagerClient,
    tokens: Arc<SandboxManagerTokenProvider>,
    org_id: String,
    fence: OwnedFence,
    mut rx: mpsc::Receiver<Incoming>,
) {
    let mut batcher = OutputBatcher::default();
    let mut ticker = tokio::time::interval(FLUSH_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut idle_ticks = 0_u32;
    let mut closed = false;

    loop {
        tokio::select! {
            incoming = rx.recv(), if !closed => match incoming {
                Some(Incoming::Bytes(stream, bytes)) => batcher.push(stream, &bytes),
                None => closed = true,
            },
            _ = ticker.tick() => {
                let chunks = batcher.drain(closed);
                let heartbeat = chunks.is_empty() && idle_ticks >= HEARTBEAT_TICKS;
                if !chunks.is_empty() || heartbeat {
                    idle_ticks = 0;
                    let Ok(token) = tokens.token(&org_id).await else { continue };
                    match client
                        .append_process_output(&token, &fence.borrow(), chunks, 0)
                        .await
                    {
                        Ok(_) => {}
                        Err(status) => {
                            // The fence refused us: a newer boot owns this
                            // process now, so stop writing rather than retry.
                            warn!(code = ?status.code(), "process output append refused; stopping this pump");
                            return;
                        }
                    }
                } else {
                    idle_ticks += 1;
                }
                if closed && batcher.is_empty() {
                    return;
                }
                if batcher.total_bytes() > MAX_TOTAL_OUTPUT_BYTES {
                    warn!(process_id = %fence.process_id, "background process exceeded its output budget");
                    return;
                }
            }
        }
    }
}

/// Turns raw stream bytes into sequenced, redacted chunks.
///
/// Split out from the IO so the rules that actually matter — that a chunk
/// ends on a line boundary, that redaction happens BEFORE any split, and that
/// one counter sequences both streams — are testable without a child process.
/// The seq is shared across stdout and stderr on purpose: the registry's
/// gap detection compares a reader's cursor against the next chunk's seq, so
/// two independent counters would look like a permanent hole.
#[derive(Default)]
struct OutputBatcher {
    next_seq: i64,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    total: i64,
}

impl OutputBatcher {
    fn push(&mut self, stream: ProcessStream, bytes: &[u8]) {
        match stream {
            ProcessStream::Stderr => self.stderr.extend_from_slice(bytes),
            _ => self.stdout.extend_from_slice(bytes),
        }
    }

    fn is_empty(&self) -> bool {
        self.stdout.is_empty() && self.stderr.is_empty()
    }

    fn total_bytes(&self) -> i64 {
        self.total
    }

    /// Emit every complete line held for either stream. `force` also emits a
    /// trailing partial line, which is correct only at EOF.
    fn drain(&mut self, force: bool) -> Vec<ProcessOutputChunk> {
        let mut chunks = Vec::new();
        for stream in [ProcessStream::Stdout, ProcessStream::Stderr] {
            let buffer = match stream {
                ProcessStream::Stderr => &mut self.stderr,
                _ => &mut self.stdout,
            };
            for (bytes, ends_with_newline) in split_flushable(buffer, force) {
                self.next_seq += 1;
                self.total += i64::try_from(bytes.len()).unwrap_or(i64::MAX);
                chunks.push(ProcessOutputChunk {
                    seq: self.next_seq,
                    stream: stream as i32,
                    content: bytes,
                    ends_with_newline,
                    captured_at: Some(std::time::SystemTime::now().into()),
                });
            }
        }
        chunks
    }
}

/// Take everything flushable out of `buffer`, redacting before any split.
///
/// Redaction has to happen on the whole complete-line region rather than
/// per-chunk-after-cutting: a secret cut in half matches no scrub pattern,
/// which is exactly the failure `executor.rs` documents for its own
/// truncation. A single line longer than [`MAX_LINE_BYTES`] is emitted
/// without its terminator rather than buffered forever.
fn split_flushable(buffer: &mut Vec<u8>, force: bool) -> Vec<(Vec<u8>, bool)> {
    let complete_upto = match buffer.iter().rposition(|byte| *byte == b'\n') {
        Some(index) => index + 1,
        None if force || buffer.len() >= MAX_LINE_BYTES => buffer.len(),
        None => 0,
    };
    if complete_upto == 0 {
        return Vec::new();
    }
    let region: Vec<u8> = buffer.drain(..complete_upto).collect();
    let ends_with_newline = region.last() == Some(&b'\n');
    let redacted = redact_bytes(&region);
    if redacted.is_empty() {
        return Vec::new();
    }
    vec![(redacted, ends_with_newline)]
}

/// Redact valid UTF-8; pass other bytes through untouched, mirroring
/// `workspace_hydrate`'s own rule for binary content.
fn redact_bytes(raw: &[u8]) -> Vec<u8> {
    match std::str::from_utf8(raw) {
        Ok(text) => crate::scrub::scrub_string(text).into_bytes(),
        Err(_) => raw.to_vec(),
    }
}

fn command_digest(program: &str, args: &[String]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(program.as_bytes());
    for arg in args {
        hasher.update([0]);
        hasher.update(arg.as_bytes());
    }
    format!("sha256:{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk_text(chunk: &ProcessOutputChunk) -> String {
        String::from_utf8(chunk.content.clone()).expect("utf-8")
    }

    #[test]
    fn only_complete_lines_flush_until_eof() {
        let mut batcher = OutputBatcher::default();
        batcher.push(ProcessStream::Stdout, b"finished\npartial");

        let chunks = batcher.drain(false);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunk_text(&chunks[0]), "finished\n");
        assert!(chunks[0].ends_with_newline);

        // The partial line is held, not emitted, until EOF forces it.
        assert!(batcher.drain(false).is_empty());
        let final_chunks = batcher.drain(true);
        assert_eq!(chunk_text(&final_chunks[0]), "partial");
        assert!(!final_chunks[0].ends_with_newline);
    }

    #[test]
    fn one_counter_sequences_both_streams() {
        // Two counters would make a reader's cursor look permanently behind
        // on one stream, which the registry reports as a retention gap.
        let mut batcher = OutputBatcher::default();
        batcher.push(ProcessStream::Stdout, b"out\n");
        batcher.push(ProcessStream::Stderr, b"err\n");
        let chunks = batcher.drain(false);
        assert_eq!(chunks.len(), 2);
        let seqs: Vec<i64> = chunks.iter().map(|c| c.seq).collect();
        assert_eq!(seqs, vec![1, 2]);

        batcher.push(ProcessStream::Stderr, b"more\n");
        let next = batcher.drain(false);
        assert_eq!(next[0].seq, 3, "the counter must not restart per stream");
    }

    #[test]
    fn a_secret_is_redacted_before_any_split() {
        let mut batcher = OutputBatcher::default();
        let secret = "sk-".to_owned() + &"a".repeat(24);
        batcher.push(
            ProcessStream::Stdout,
            format!("token={secret}\nnext line\n").as_bytes(),
        );
        let chunks = batcher.drain(false);
        let text: String = chunks.iter().map(chunk_text).collect();
        assert!(!text.contains(&secret), "secret survived: {text}");
        assert!(text.contains("[REDACTED]"), "no marker: {text}");
        assert!(text.contains("next line"));
    }

    #[test]
    fn an_endless_line_is_flushed_rather_than_buffered_forever() {
        let mut batcher = OutputBatcher::default();
        batcher.push(ProcessStream::Stdout, &vec![b'x'; MAX_LINE_BYTES + 10]);
        let chunks = batcher.drain(false);
        assert_eq!(chunks.len(), 1, "an oversized line must not be held");
        assert!(!chunks[0].ends_with_newline);
    }

    #[test]
    fn nothing_flushes_from_an_empty_buffer() {
        let mut batcher = OutputBatcher::default();
        assert!(batcher.drain(false).is_empty());
        assert!(batcher.drain(true).is_empty());
        assert!(batcher.is_empty());
    }

    #[test]
    fn non_utf8_output_passes_through_rather_than_being_mangled() {
        let mut batcher = OutputBatcher::default();
        batcher.push(ProcessStream::Stdout, &[0xff, 0xfe, b'\n']);
        let chunks = batcher.drain(false);
        assert_eq!(chunks[0].content, vec![0xff, 0xfe, b'\n']);
    }

    #[test]
    fn the_digest_covers_every_argument_unambiguously() {
        // Length-free concatenation would make ["ab","c"] and ["a","bc"] the
        // same command; the NUL separator is what prevents that.
        let a = command_digest("sh", &["ab".to_owned(), "c".to_owned()]);
        let b = command_digest("sh", &["a".to_owned(), "bc".to_owned()]);
        assert_ne!(a, b);
        assert!(a.starts_with("sha256:"));
    }

    #[test]
    fn the_scratch_directory_is_outside_the_space_workspace() {
        // A background process's own scaffolding must never be diffed and
        // promoted as workspace content.
        let scratch = scratch_dir("space-1", "proc-1");
        assert!(scratch
            .to_string_lossy()
            .contains("verevon-space-processes"));
        assert!(!scratch
            .to_string_lossy()
            .contains("verevon-space-workspaces"));
    }

    #[tokio::test]
    async fn sleep_until_maybe_never_fires_without_a_deadline() {
        let pending =
            tokio::time::timeout(Duration::from_millis(20), sleep_until_maybe(None)).await;
        assert!(pending.is_err(), "a missing deadline must never fire");
    }
}
