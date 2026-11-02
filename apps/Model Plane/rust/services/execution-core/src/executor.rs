//! Real sandboxed tool-process executor (matrix §G1).
//!
//! The `tool_bridge` dispatch is a deterministic stub — its catch-all returns
//! canned text and spawns nothing. This module is the missing real-execution
//! primitive: build argv, wrap it in the sandbox ([`sandbox::wrap_command`] —
//! bubblewrap on Linux), reject a policy downgrade, spawn the process,
//! capture its output, and **scrub secrets** from that output before it leaves
//! the execution boundary.
//!
//! # Scope & verification (built unverified per the user's call; e2e flagged)
//!
//! [`execute_sandboxed`] is the primitive. Its spawn → capture → scrub path is
//! unit-tested cross-platform via explicit policy tests. The bubblewrap **isolation** itself
//! is Linux-gated and verified separately (`sandbox.rs` tests +
//! `scripts/verify-sandbox-isolation.sh`).
//!
//! Wiring (DONE): `runtime_loop::execute_step` routes the `shell` tool to this
//! primitive (`execute_shell` → `execute_sandboxed_in_dir_with_ro_binds`, cwd `/`,
//! cleared environment, a [`SpawnAdmission`]) under a `ReadOnly` +
//! no-network default policy, behind the same permission/hook gates as every
//! other tool; non-shell tools keep the deterministic `tool_bridge` path by
//! design. A broader per-call policy (a `policy` field on the shell input) is
//! the natural future extension.
//!
//! SECURITY: the `AllowDomains` network policy is not self-enforcing (see
//! `sandbox.rs`) — a caller using it MUST also run behind a configured egress
//! proxy or the process gets unrestricted egress. `execute_sandboxed` fails
//! closed on both of the ways a policy can go unenforced: the requested local
//! filesystem isolation being unavailable, and `AllowDomains` with no egress
//! proxy (`HTTPS_PROXY`/`ALL_PROXY`) configured to actually enforce it — see
//! `require_requested_isolation`. Neither ever spawns the process.

use crate::policy::{MpSandboxPolicy, ResourceLimits};
use crate::{sandbox, scrub};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Wall-clock ceiling for one sandboxed execution. Without it, a child that
/// never exits (an accidental `while true`, a hung network read) held the
/// gRPC `ExecuteStep` open FOREVER — there was no timeout anywhere on this
/// path. Overridable per deployment via `EXECUTION_SHELL_TIMEOUT_SECS`.
const DEFAULT_EXEC_TIMEOUT_SECS: u64 = 30;

/// Same ceiling for the code-execution path, tunable separately via
/// `EXECUTION_CODE_TIMEOUT_SECS`: a data/report job has a legitimately different
/// runtime profile from a `shell` one-liner, and one knob for both would force
/// the operator to loosen the shell ceiling to buy headroom for code.
///
/// The default is deliberately well above the shell's 30s: a COLD first call
/// pays for importing `pandas`/`matplotlib` and building the font cache before
/// any of the user's work starts, which alone can approach 30s. A ceiling that
/// low turns "generate a chart" into a timeout on the first call and a success on
/// the second — the worst possible failure mode to debug.
const DEFAULT_CODE_TIMEOUT_SECS: u64 = 120;

/// Compile-time guard on the relationship above: code must never end up with a
/// tighter default than `shell`, whatever either value is edited to.
const _: () = assert!(DEFAULT_CODE_TIMEOUT_SECS > DEFAULT_EXEC_TIMEOUT_SECS);

/// Per-stream cap on returned output. `Command::output()` collects the whole
/// stream into memory, and a child that prints gigabytes would otherwise ride
/// that straight into the step result (and the model's context).
const MAX_OUTPUT_CHARS: usize = 64 * 1024;

/// Per-stream byte ceiling while a child is still running (WS06-S7,
/// `EXECUTION_SANDBOX_MAX_OUTPUT_BYTES`). Past it the child is killed, so a
/// process printing gigabytes never grows execution-core's memory.
const DEFAULT_MAX_OUTPUT_BYTES: usize = 1024 * 1024;
/// Process-wide spawn concurrency (`EXECUTION_SANDBOX_MAX_CONCURRENT`).
const DEFAULT_MAX_CONCURRENT_SPAWNS: usize = 8;
/// How long a spawn waits for a permit (`EXECUTION_SANDBOX_QUEUE_WAIT_MS`).
const DEFAULT_QUEUE_WAIT_MS: u64 = 5000;

/// Positive integer from an optional raw value; missing, unparseable or zero
/// falls back to `default`. Pure, so it is unit-testable without env mutation.
fn parse_positive(raw: Option<&str>, default: u64) -> u64 {
    raw.and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|&value| value > 0)
        .unwrap_or(default)
}

fn positive_from_env(var: &str, default: u64) -> u64 {
    parse_positive(std::env::var(var).ok().as_deref(), default)
}

fn max_output_bytes() -> usize {
    usize::try_from(positive_from_env(
        "EXECUTION_SANDBOX_MAX_OUTPUT_BYTES",
        DEFAULT_MAX_OUTPUT_BYTES as u64,
    ))
    .unwrap_or(DEFAULT_MAX_OUTPUT_BYTES)
}

/// The process-wide spawn gate. Built once from the environment.
fn spawn_gate() -> &'static (std::sync::Arc<tokio::sync::Semaphore>, Duration) {
    static GATE: std::sync::OnceLock<(std::sync::Arc<tokio::sync::Semaphore>, Duration)> =
        std::sync::OnceLock::new();
    GATE.get_or_init(|| {
        let permits = usize::try_from(positive_from_env(
            "EXECUTION_SANDBOX_MAX_CONCURRENT",
            DEFAULT_MAX_CONCURRENT_SPAWNS as u64,
        ))
        .unwrap_or(DEFAULT_MAX_CONCURRENT_SPAWNS)
        .min(tokio::sync::Semaphore::MAX_PERMITS);
        let wait = Duration::from_millis(positive_from_env(
            "EXECUTION_SANDBOX_QUEUE_WAIT_MS",
            DEFAULT_QUEUE_WAIT_MS,
        ));
        (std::sync::Arc::new(tokio::sync::Semaphore::new(permits)), wait)
    })
}

/// Take a spawn permit before any sandboxed spawn (WS06-S7). Waits at most
/// `EXECUTION_SANDBOX_QUEUE_WAIT_MS`; on timeout the spawn is refused with a
/// transient "busy" error (`WouldBlock`) so the step can be retried later.
///
/// # Errors
/// `WouldBlock` when no permit became free in time.
pub(crate) async fn acquire_spawn_permit() -> std::io::Result<tokio::sync::OwnedSemaphorePermit> {
    let (semaphore, wait) = spawn_gate();
    match tokio::time::timeout(*wait, semaphore.clone().acquire_owned()).await {
        Ok(Ok(permit)) => Ok(permit),
        _ => {
            record_limit_hit("busy");
            Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "sandbox is busy, retry later",
            ))
        }
    }
}

/// Which stream tripped the output ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputStream {
    Stdout,
    Stderr,
}

/// A resource ceiling a child hit and was killed for (WS06-S7 defines the
/// output case; WS06-S8 adds the per-spawn rlimit classes). Serialises as
/// `{"kind": "...", ...}` into the `execution.limit_hit` field of a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LimitHit {
    /// A stream produced more than `limit_bytes` while the child ran.
    Output {
        stream: OutputStream,
        limit_bytes: usize,
    },
    /// The child exceeded its CPU-time limit and got SIGXCPU.
    Cpu { limit_secs: u64 },
    /// The child tried to write a file past the size limit and got SIGXFSZ.
    FileSize { limit_bytes: u64 },
    /// The child died of SIGKILL that execution-core did not send. OOM and an
    /// external kill are indistinguishable here.
    Killed,
}

impl LimitHit {
    /// The `kind` label of `execution_core_sandbox_limit_hits_total`.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Output { .. } => "output",
            Self::Cpu { .. } => "cpu",
            Self::FileSize { .. } => "file_size",
            Self::Killed => "killed",
        }
    }

    /// One line for a tool-failure message. Starts with the retry marker
    /// "resource limit exceeded" (never transient: the same input hits the
    /// same limit again).
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Output { limit_bytes, .. } => {
                format!("resource limit exceeded: output over {limit_bytes} bytes")
            }
            Self::Cpu { limit_secs } => {
                format!("resource limit exceeded: more than {limit_secs}s of CPU time")
            }
            Self::FileSize { limit_bytes } => {
                format!("resource limit exceeded: a file grew past {limit_bytes} bytes")
            }
            Self::Killed => {
                "resource limit exceeded: the process was killed (out of memory or killed externally)"
                    .to_owned()
            }
        }
    }
}

/// Hits per kind, in the order of [`LIMIT_KINDS`].
static LIMIT_HITS: [std::sync::atomic::AtomicU64; 5] = [
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
];
const LIMIT_KINDS: [&str; 5] = ["cpu", "file_size", "output", "killed", "busy"];

/// Space-scoped `shell` calls that ran without a sandbox lease while
/// `EXECUTION_CORE_SPACE_SHELL_LEASE=observe` (`space_shell_unleased_total`).
static SHELL_UNLEASED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Count one unleased Space `shell` call. No labels: nothing here can carry an id.
pub(crate) fn record_shell_unleased() {
    SHELL_UNLEASED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// Count one limit hit (`execution_core_sandbox_limit_hits_total{kind}`).
/// An unknown kind is ignored rather than invented as a new label.
pub(crate) fn record_limit_hit(kind: &str) {
    if let Some(index) = LIMIT_KINDS.iter().position(|known| *known == kind) {
        LIMIT_HITS[index].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// The limit-hit counter in Prometheus text format.
#[must_use]
pub fn limit_hit_metrics() -> String {
    let mut out = String::from(
        "# HELP execution_core_sandbox_limit_hits_total Sandboxed spawns that hit a resource limit.\n\
         # TYPE execution_core_sandbox_limit_hits_total counter\n",
    );
    for (kind, hits) in LIMIT_KINDS.iter().zip(LIMIT_HITS.iter()) {
        out.push_str(&format!(
            "execution_core_sandbox_limit_hits_total{{kind=\"{kind}\"}} {}\n",
            hits.load(std::sync::atomic::Ordering::Relaxed)
        ));
    }
    out.push_str(
        "# HELP space_shell_unleased_total Space-scoped shell calls run without a sandbox lease (observe mode).\n\
         # TYPE space_shell_unleased_total counter\n",
    );
    out.push_str(&format!(
        "space_shell_unleased_total {}\n",
        SHELL_UNLEASED.load(std::sync::atomic::Ordering::Relaxed)
    ));
    out
}

/// Pure exit classification (WS06-S8). `signal` is the terminating signal when
/// the direct child died of one; `code` its exit code. Under `bwrap` the
/// direct child is `bwrap`, which reports a signalled inner process as
/// `128 + signal`, so both spellings are read. SIGXCPU/SIGXFSZ only count when
/// the limits were applied (otherwise they are the program's own business);
/// SIGKILL counts for any sandboxed child.
#[must_use]
pub(crate) fn classify_exit(
    signal: Option<i32>,
    code: Option<i32>,
    sandboxed: bool,
    limits: Option<&ResourceLimits>,
) -> Option<LimitHit> {
    const SIGKILL: i32 = 9;
    const SIGXCPU: i32 = 24;
    const SIGXFSZ: i32 = 25;
    let sig = signal.or_else(|| code.and_then(|c| (129..=159).contains(&c).then(|| c - 128)));
    match (sig?, limits) {
        (SIGXCPU, Some(l)) => Some(LimitHit::Cpu {
            limit_secs: l.cpu_secs,
        }),
        (SIGXFSZ, Some(l)) => Some(LimitHit::FileSize {
            limit_bytes: l.file_size_bytes,
        }),
        (SIGKILL, _) if sandboxed => Some(LimitHit::Killed),
        _ => None,
    }
}

#[cfg(unix)]
pub(crate) fn exit_signal(status: &std::process::ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    status.signal()
}

#[cfg(not(unix))]
pub(crate) fn exit_signal(_status: &std::process::ExitStatus) -> Option<i32> {
    None
}

/// Refuse a sandboxed command that was asked to carry per-spawn limits but does
/// not (`prlimit` unusable here): fail closed, as for a missing bwrap.
fn require_limits_applied(
    requested: bool,
    sandboxed: bool,
    applied: bool,
) -> std::io::Result<()> {
    if requested && sandboxed && !applied {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "per-spawn resource limits are required but prlimit is unavailable; refusing unlimited execution",
        ));
    }
    Ok(())
}

/// [`require_limits_applied`] for a wrapped command (used by every spawn site).
pub(crate) fn require_limits_applied_for(
    limits: Option<&ResourceLimits>,
    cmd: &sandbox::SandboxedCommand,
) -> std::io::Result<()> {
    require_limits_applied(limits.is_some(), cmd.sandboxed, cmd.limits_applied)
}

/// Read `reader` into `buf` until EOF or until more than `cap` bytes arrive.
/// Returns true when the cap was exceeded (`buf` then holds exactly `cap`).
/// `buf` is caller-owned so partial data survives if this future is dropped.
async fn read_capped<R: tokio::io::AsyncRead + Unpin>(
    mut reader: R,
    buf: &mut Vec<u8>,
    cap: usize,
) -> std::io::Result<bool> {
    use tokio::io::AsyncReadExt;
    let mut chunk = [0u8; 8192];
    loop {
        let n = reader.read(&mut chunk).await?;
        if n == 0 {
            return Ok(false);
        }
        let room = cap.saturating_sub(buf.len());
        if n > room {
            buf.extend_from_slice(&chunk[..room]);
            return Ok(true);
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

/// Pure timeout resolution, split out from the environment read so the knob's
/// semantics are unit-testable (env mutation is forbidden in this crate).
/// A missing, unparseable, or zero value falls back to `default_secs`.
fn parse_timeout_secs(raw: Option<&str>, default_secs: u64) -> Duration {
    let secs = raw
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|&value| value > 0)
        .unwrap_or(default_secs);
    Duration::from_secs(secs)
}

fn timeout_from_env(var: &str, default_secs: u64) -> Duration {
    parse_timeout_secs(std::env::var(var).ok().as_deref(), default_secs)
}

pub(crate) fn exec_timeout() -> Duration {
    timeout_from_env("EXECUTION_SHELL_TIMEOUT_SECS", DEFAULT_EXEC_TIMEOUT_SECS)
}

/// Wall-clock ceiling for one code-execution call
/// (`EXECUTION_CODE_TIMEOUT_SECS`, default [`DEFAULT_CODE_TIMEOUT_SECS`]s). Read
/// at call time — callers that must not depend on the process environment
/// (tests) pass their own deadline to [`execute_sandboxed_in_dir`] instead.
pub fn code_exec_timeout() -> Duration {
    timeout_from_env("EXECUTION_CODE_TIMEOUT_SECS", DEFAULT_CODE_TIMEOUT_SECS)
}

/// Scrub first, then truncate — never the other way around: truncation can cut
/// a secret in half, and half a secret no longer matches the scrub patterns.
fn scrub_and_cap(raw: &[u8]) -> String {
    let scrubbed = scrub::scrub_string(&String::from_utf8_lossy(raw));
    if scrubbed.chars().count() <= MAX_OUTPUT_CHARS {
        return scrubbed;
    }
    let mut capped: String = scrubbed.chars().take(MAX_OUTPUT_CHARS).collect();
    capped.push_str("\n…[output truncated at 64 KiB]");
    capped
}

/// Pure check, split out from the environment read so it's unit-testable
/// without mutating real process env vars (forbidden in this crate — see
/// `parse_timeout_secs`/`timeout_from_env`). True if any candidate value is
/// present and non-blank.
fn any_proxy_var_set(candidates: &[Option<&str>]) -> bool {
    candidates
        .iter()
        .any(|value| value.is_some_and(|v| !v.trim().is_empty()))
}

/// Whether an egress proxy is configured for this process, by the same
/// convention curl/most HTTP clients honor: `HTTPS_PROXY` or `ALL_PROXY`,
/// either case.
fn egress_proxy_configured() -> bool {
    any_proxy_var_set(&[
        std::env::var("HTTPS_PROXY").ok().as_deref(),
        std::env::var("https_proxy").ok().as_deref(),
        std::env::var("ALL_PROXY").ok().as_deref(),
        std::env::var("all_proxy").ok().as_deref(),
    ])
}

/// A model-authored execution policy must never silently become a host command
/// just because the local sandbox substrate is unavailable. `DangerFullAccess`
/// is the explicit operator-authored escape hatch; `External` delegates its
/// isolation contract to a separately attested provisioner and is therefore not
/// a local Bubblewrap request. Every other policy names local filesystem and/or
/// egress restrictions and has to fail closed when they cannot be enforced.
///
/// `AllowDomains` gets its own check, not just `sandboxed`: `build_bwrap_argv`
/// deliberately keeps the network namespace up for it (the allowlist lives at
/// an external egress proxy, not in bwrap itself — see `sandbox.rs`'s own
/// SECURITY note, tracked as `capability-ownership-matrix.md` §G1's recorded
/// requirement). A process can come back `sandboxed: true` — real filesystem
/// isolation — while still having FULLY UNRESTRICTED network egress, because
/// bwrap alone cannot enforce a domain allowlist. Without this check, that gap
/// was silent: `sandboxed` reported success while the allowlist was a no-op.
/// The same fail-closed gate every one-shot spawn runs, for callers that
/// build their own `tokio::process::Command` rather than going through
/// [`execute_sandboxed_in_dir`] — S4.2's background process host needs piped
/// stdio and a child that outlives the call, but must not thereby get a
/// weaker isolation check than a 30-second `code_interpreter` call.
///
/// # Errors
/// `Unsupported` when the policy demands local isolation this host cannot
/// provide, or when `AllowDomains` has no egress proxy behind it.
///
/// WS06-S9: this is the one admission gate every spawn path calls, and it takes
/// a [`SpawnAdmission`] with no default, so a new spawn path that omits the
/// Space facts does not compile.
pub(crate) fn require_spawn_admission(
    policy: &MpSandboxPolicy,
    sandboxed: bool,
    admission: &SpawnAdmission<'_>,
) -> std::io::Result<()> {
    require_spawn_admission_with(policy, sandboxed, egress_proxy_configured(), admission)
}

/// What a spawn site knows about the Space it runs for (WS06-S9). Built by the
/// caller from facts it holds, never defaulted.
#[derive(Debug, Clone, Copy)]
pub struct SpawnAdmission<'a> {
    /// The spawn serves a Space-scoped run.
    pub space_scoped: bool,
    /// The sandbox lease the caller holds for that run; required when
    /// `space_scoped`.
    pub lease_id: Option<&'a str>,
}

impl SpawnAdmission<'_> {
    /// A spawn that serves no Space (an ordinary run, or a service-internal
    /// helper). Named so the choice is visible at the call site.
    pub const NOT_SPACE: SpawnAdmission<'static> = SpawnAdmission {
        space_scoped: false,
        lease_id: None,
    };
}

/// Pure half of [`require_spawn_admission`]. Rules, in order: the isolation
/// check; no network other than `Disabled` (sandbox egress does not exist, and
/// the proxy environment variable is no longer trusted to stand in for it);
/// no `DangerFullAccess`/`External` for a Space; a Space spawn needs a lease.
fn require_spawn_admission_with(
    policy: &MpSandboxPolicy,
    sandboxed: bool,
    egress_proxy_configured: bool,
    admission: &SpawnAdmission<'_>,
) -> std::io::Result<()> {
    require_requested_isolation(policy, sandboxed, egress_proxy_configured)?;
    if policy.network().allows_any_network() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "sandbox egress is not available; refusing a spawn with network access",
        ));
    }
    if admission.space_scoped {
        if matches!(
            policy,
            MpSandboxPolicy::DangerFullAccess | MpSandboxPolicy::External { .. }
        ) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "this sandbox policy is not permitted for a Space-scoped spawn",
            ));
        }
        if !admission.lease_id.is_some_and(|lease| !lease.trim().is_empty()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "a Space-scoped sandbox spawn requires a sandbox lease",
            ));
        }
    }
    Ok(())
}

fn require_requested_isolation(
    policy: &MpSandboxPolicy,
    sandboxed: bool,
    egress_proxy_configured: bool,
) -> std::io::Result<()> {
    let requires_local_isolation = matches!(
        policy,
        MpSandboxPolicy::ReadOnly { .. } | MpSandboxPolicy::WorkspaceWrite { .. }
    );
    if requires_local_isolation && !sandboxed {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "requested sandbox isolation is unavailable; refusing unsandboxed execution",
        ));
    }
    if matches!(
        policy.network(),
        crate::policy::MpNetworkPolicy::AllowDomains(_)
    ) && !egress_proxy_configured
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "AllowDomains egress policy requires an egress proxy (HTTPS_PROXY/ALL_PROXY); \
             none is configured, refusing to grant unrestricted network egress",
        ));
    }
    Ok(())
}

/// Outcome of a sandboxed execution. `stdout`/`stderr` are already
/// secret-scrubbed, so they are safe to persist or forward.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecOutcome {
    /// Secret-scrubbed standard output.
    pub stdout: String,
    /// Secret-scrubbed standard error.
    pub stderr: String,
    /// Process exit code (`-1` if the process was killed by a signal).
    pub exit_code: i32,
    /// True if bubblewrap actually wrapped the process (Linux + `bwrap`
    /// present); false means it ran as a transparent passthrough.
    pub sandboxed: bool,
    /// Set when the child was killed for exceeding a resource ceiling
    /// (WS06-S7: the per-stream output cap). `None` for a normal exit.
    pub limit_hit: Option<LimitHit>,
    /// `enforced` when the child ran under per-spawn `prlimit` ceilings,
    /// `unlimited` otherwise (WS06-S8).
    pub limits: &'static str,
}

/// Execute `program args` under `policy`, returning scrubbed output.
///
/// Output always passes through [`scrub::scrub_string`] before returning, so a
/// child that prints a secret cannot leak it across the boundary.
///
/// # Errors
/// Returns the underlying [`std::io::Error`] if the process cannot be spawned
/// or its output collected, or a `TimedOut` error when the child exceeds the
/// execution timeout (the child is killed, not left running).
pub async fn execute_sandboxed(
    policy: &MpSandboxPolicy,
    program: &str,
    args: &[String],
) -> std::io::Result<ExecOutcome> {
    execute_sandboxed_with_timeout(policy, program, args, exec_timeout()).await
}

/// [`execute_sandboxed`] with an explicit timeout — the testable seam (env
/// mutation is forbidden in this crate, so tests inject the deadline instead).
async fn execute_sandboxed_with_timeout(
    policy: &MpSandboxPolicy,
    program: &str,
    args: &[String],
    timeout: Duration,
) -> std::io::Result<ExecOutcome> {
    let limits = crate::policy::limits_for(timeout);
    let options = sandbox::LaunchOptions {
        limits: limits.as_ref(),
        ..sandbox::LaunchOptions::default()
    };
    let cmd = sandbox::wrap_command_with(policy, program, args, options);
    require_spawn_admission(policy, cmd.sandboxed, &SpawnAdmission::NOT_SPACE)?;
    require_limits_applied_for(limits.as_ref(), &cmd)?;
    spawn_capture(&cmd, None, None, timeout, limits.as_ref()).await
}

/// Execute `program args` under `policy` **inside** `cwd`, with `env` as the
/// process's ENTIRE environment, returning scrubbed output.
///
/// This is the code-execution seam, and both extras are load-bearing:
///
/// * the working directory is part of the tool contract (a program writing
///   `out.xlsx` must land in its own workspace), and
/// * `env` REPLACES the inherited environment rather than extending it.
///   execution-core's own environment holds the internal keys it uses to reach
///   the other planes, so model-authored code must not inherit it; the allowlist
///   also has to point any runtime that needs a writable home/cache/temp
///   directory into the workspace, because everything else is read-only.
///
/// `timeout` is passed in rather than read from the environment so a caller can
/// choose its own deadline (see [`code_exec_timeout`] for the deployed default).
///
/// # Errors
/// Returns the underlying [`std::io::Error`] if the process cannot be spawned
/// or its output collected, or a `TimedOut` error when the child exceeds
/// `timeout` (the child is killed, not left running).
pub async fn execute_sandboxed_in_dir(
    policy: &MpSandboxPolicy,
    program: &str,
    args: &[String],
    cwd: &Path,
    env: &[(String, String)],
    timeout: Duration,
) -> std::io::Result<ExecOutcome> {
    execute_sandboxed_in_dir_with_ro_binds(
        policy,
        program,
        args,
        cwd,
        env,
        &[],
        timeout,
        &SpawnAdmission::NOT_SPACE,
    )
    .await
}

/// [`execute_sandboxed_in_dir`] with read-only layers: each `(host, mount)` pair
/// is bound read-only inside the sandbox, after the writable root, so a program
/// can read the files but a write to them fails with `EROFS` from the kernel.
///
/// A read-only layer is only a promise while bubblewrap enforces it. On a host
/// without a working sandbox the command would run as a plain child process that
/// can open the staged files for writing, so staging is refused there, whatever
/// the policy says ([`require_ro_binds_enforced`]).
///
/// # Errors
/// As [`execute_sandboxed_in_dir`], plus `Unsupported` when `ro_binds` is
/// non-empty and the command would not be wrapped.
#[allow(clippy::too_many_arguments)]
pub async fn execute_sandboxed_in_dir_with_ro_binds(
    policy: &MpSandboxPolicy,
    program: &str,
    args: &[String],
    cwd: &Path,
    env: &[(String, String)],
    ro_binds: &[(PathBuf, PathBuf)],
    timeout: Duration,
    admission: &SpawnAdmission<'_>,
) -> std::io::Result<ExecOutcome> {
    // WS06-S8: per-spawn ceilings sized from this call's wall-clock budget.
    let limits = crate::policy::limits_for(timeout);
    let options = sandbox::LaunchOptions {
        cwd: Some(cwd),
        env: sandbox::SandboxEnv::Only(env),
        extra_ro_binds: ro_binds,
        limits: limits.as_ref(),
    };
    let cmd = sandbox::wrap_command_with(policy, program, args, options);
    require_spawn_admission(policy, cmd.sandboxed, admission)?;
    require_ro_binds_enforced(!ro_binds.is_empty(), cmd.sandboxed)?;
    require_limits_applied_for(limits.as_ref(), &cmd)?;
    spawn_capture(&cmd, Some(cwd), Some(env), timeout, limits.as_ref()).await
}

/// Staged inputs are read-only only inside bubblewrap. Refuses a command that
/// asks for read-only layers but would run unwrapped (no bwrap on the host, or a
/// policy that applies none), rather than hand the program writable copies and
/// still call them read-only.
fn require_ro_binds_enforced(has_ro_binds: bool, sandboxed: bool) -> std::io::Result<()> {
    if has_ro_binds && !sandboxed {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "read-only attachment staging needs the OS sandbox, which is unavailable; refusing unsandboxed execution",
        ));
    }
    Ok(())
}

/// Spawn and capture. `env: Some(pairs)` means the process starts from an EMPTY
/// environment and gets exactly `pairs`; `None` inherits (the `shell` path).
async fn spawn_capture(
    cmd: &sandbox::SandboxedCommand,
    cwd: Option<&Path>,
    env: Option<&[(String, String)]>,
    timeout: Duration,
    limits: Option<&ResourceLimits>,
) -> std::io::Result<ExecOutcome> {
    let mut command = tokio::process::Command::new(&cmd.program);
    command.args(&cmd.args);
    // Set on the spawned process itself, not just via bwrap's `--chdir`/
    // `--clearenv`: when bubblewrap is absent the wrap is a transparent
    // passthrough, so without these a dev host would run with the working
    // directory and the FULL service environment of execution-core itself.
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    if let Some(allowlist) = env {
        command.env_clear();
        command.envs(allowlist.iter().map(|(key, value)| (key, value)));
    }
    // `kill_on_drop` is what makes the timeout real: `timeout()` cancels by
    // DROPPING the output future, and without it the orphaned child keeps
    // running (and holding its sandbox) after we've already given up on it.
    // WS06-S7: admission first (bounded wait, transient refusal), held until the
    // child has been reaped.
    let _permit = acquire_spawn_permit().await?;
    let cap = max_output_bytes();
    // Same stdio as `output()` (stdin closed, both streams piped), but read
    // through a byte cap so a runaway child is killed instead of buffered.
    command
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let run = async {
        let mut child = command.spawn()?;
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let mut hit: Option<LimitHit> = None;
        if let (Some(out_pipe), Some(err_pipe)) = (child.stdout.take(), child.stderr.take()) {
            let mut out_f = Box::pin(read_capped(out_pipe, &mut stdout, cap));
            let mut err_f = Box::pin(read_capped(err_pipe, &mut stderr, cap));
            let (mut out_done, mut err_done) = (false, false);
            while !(out_done && err_done) && hit.is_none() {
                tokio::select! {
                    r = &mut out_f, if !out_done => {
                        out_done = true;
                        if r? {
                            hit = Some(LimitHit::Output { stream: OutputStream::Stdout, limit_bytes: cap });
                        }
                    }
                    r = &mut err_f, if !err_done => {
                        err_done = true;
                        if r? {
                            hit = Some(LimitHit::Output { stream: OutputStream::Stderr, limit_bytes: cap });
                        }
                    }
                }
            }
        }
        if hit.is_some() {
            let _ = child.start_kill();
        }
        let status = child.wait().await?;
        Ok::<_, std::io::Error>((stdout, stderr, status, hit))
    };
    let (stdout, stderr, status, limit_hit) = tokio::time::timeout(timeout, run)
        .await
        .map_err(|_elapsed| {
            std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!(
                    "execution exceeded {}s and was killed; long-running commands are not supported here",
                    timeout.as_secs()
                ),
            )
        })??;
    // WS06-S8: a child we did not kill ourselves may have died of a limit.
    let applied = cmd.limits_applied.then_some(limits).flatten();
    let limit_hit = limit_hit.or_else(|| {
        classify_exit(exit_signal(&status), status.code(), cmd.sandboxed, applied)
    });
    if let Some(hit) = &limit_hit {
        record_limit_hit(hit.kind());
    }
    Ok(ExecOutcome {
        stdout: scrub_and_cap(&stdout),
        stderr: scrub_and_cap(&stderr),
        exit_code: status.code().unwrap_or(-1),
        sandboxed: cmd.sandboxed,
        limit_hit,
        limits: if cmd.limits_applied {
            sandbox::LIMITS_ENFORCED
        } else {
            sandbox::LIMITS_UNLIMITED
        },
    })
}

/// Test seam (WS06-S7): the spawn gate is process-wide, so tests that exhaust it must not overlap
/// (one test's hold would starve another's recovery check). This lock serialises them.
#[cfg(test)]
static GATE_HOLD_SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Every permit of the process-wide spawn gate, held by one test at a time.
#[cfg(test)]
pub(crate) struct HeldGate {
    permits: Option<tokio::sync::OwnedSemaphorePermit>,
    _serial: tokio::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl HeldGate {
    /// Give the permits back while keeping other gate-exhausting tests out.
    pub(crate) fn release_permits(&mut self) {
        self.permits = None;
    }
}

/// Take EVERY permit of the process-wide spawn gate so a test can prove the "sandbox is busy"
/// refusal without mutating the environment.
#[cfg(test)]
pub(crate) async fn hold_every_spawn_permit_for_test() -> HeldGate {
    let serial = GATE_HOLD_SERIAL.lock().await;
    let (semaphore, _) = spawn_gate();
    let total = usize::try_from(positive_from_env(
        "EXECUTION_SANDBOX_MAX_CONCURRENT",
        DEFAULT_MAX_CONCURRENT_SPAWNS as u64,
    ))
    .unwrap_or(DEFAULT_MAX_CONCURRENT_SPAWNS)
    .min(tokio::sync::Semaphore::MAX_PERMITS);
    let permits = semaphore
        .clone()
        .acquire_many_owned(u32::try_from(total).expect("permit count fits u32"))
        .await
        .expect("the spawn gate is never closed");
    HeldGate {
        permits: Some(permits),
        _serial: serial,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn staging_is_refused_where_the_read_only_bind_is_not_enforced() {
        let refused = require_ro_binds_enforced(true, false).expect_err("must refuse");
        assert_eq!(refused.kind(), std::io::ErrorKind::Unsupported);
        assert!(require_ro_binds_enforced(true, true).is_ok());
        // No staging, no new requirement: every existing caller is unaffected.
        assert!(require_ro_binds_enforced(false, false).is_ok());
    }

    #[test]
    fn local_restricted_policies_reject_an_unsandboxed_downgrade() {
        for policy in [
            MpSandboxPolicy::ReadOnly {
                network: crate::policy::MpNetworkPolicy::Disabled,
            },
            MpSandboxPolicy::WorkspaceWrite {
                writable_roots: vec![],
                network: crate::policy::MpNetworkPolicy::Disabled,
            },
        ] {
            let err = require_requested_isolation(&policy, false, false)
                .expect_err("restricted policy must never run as a host command");
            assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
        }
        assert!(
            require_requested_isolation(&MpSandboxPolicy::DangerFullAccess, false, false).is_ok()
        );
        assert!(require_requested_isolation(
            &MpSandboxPolicy::External {
                network: crate::policy::MpNetworkPolicy::Disabled,
            },
            false,
            false,
        )
        .is_ok());
    }

    #[test]
    fn allow_domains_without_an_egress_proxy_fails_closed() {
        // `sandboxed: true` alone is not enough: bwrap keeps the network
        // namespace up for `AllowDomains` (the allowlist lives at an external
        // proxy, not in bwrap), so without a configured proxy this policy
        // would silently grant unrestricted egress despite reporting real
        // filesystem isolation. Both ReadOnly and WorkspaceWrite carry it.
        for policy in [
            MpSandboxPolicy::ReadOnly {
                network: crate::policy::MpNetworkPolicy::AllowDomains(vec![
                    "api.openai.com".to_owned()
                ]),
            },
            MpSandboxPolicy::WorkspaceWrite {
                writable_roots: vec![],
                network: crate::policy::MpNetworkPolicy::AllowDomains(vec![
                    "api.openai.com".to_owned()
                ]),
            },
        ] {
            let err = require_requested_isolation(&policy, true, false).expect_err(
                "AllowDomains without a configured proxy must refuse, even though sandboxed=true",
            );
            assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
            assert!(err.to_string().contains("egress proxy"));
        }
    }

    #[test]
    fn allow_domains_with_an_egress_proxy_configured_is_accepted() {
        let policy = MpSandboxPolicy::WorkspaceWrite {
            writable_roots: vec![],
            network: crate::policy::MpNetworkPolicy::AllowDomains(
                vec!["api.openai.com".to_owned()],
            ),
        };
        assert!(require_requested_isolation(&policy, true, true).is_ok());
    }

    #[test]
    fn a_disabled_or_allow_all_network_policy_never_needs_a_proxy() {
        // The proxy requirement is specific to AllowDomains -- Disabled is
        // enforced by bwrap itself (`--unshare-net`), and AllowAll never
        // claimed a restriction a proxy would need to back up.
        assert!(require_requested_isolation(
            &MpSandboxPolicy::ReadOnly {
                network: crate::policy::MpNetworkPolicy::Disabled,
            },
            true,
            false,
        )
        .is_ok());
        assert!(require_requested_isolation(
            &MpSandboxPolicy::ReadOnly {
                network: crate::policy::MpNetworkPolicy::AllowAll,
            },
            true,
            false,
        )
        .is_ok());
    }

    #[test]
    fn any_proxy_var_set_is_true_only_for_a_non_blank_candidate() {
        assert!(!any_proxy_var_set(&[None, None, None, None]));
        assert!(!any_proxy_var_set(&[Some(""), Some("   "), None, None]));
        assert!(any_proxy_var_set(&[
            None,
            None,
            Some("http://proxy:3128"),
            None
        ]));
    }

    // DangerFullAccess => wrap_command returns a transparent passthrough, so
    // these run the real binary directly and work cross-platform (no bwrap).

    #[tokio::test]
    async fn captures_stdout_and_zero_exit() {
        let out = execute_sandboxed(
            &MpSandboxPolicy::DangerFullAccess,
            "echo",
            &args(&["hello"]),
        )
        .await
        .expect("spawn echo");
        assert!(out.stdout.contains("hello"), "stdout: {:?}", out.stdout);
        assert_eq!(out.exit_code, 0);
        assert!(!out.sandboxed, "DangerFullAccess is passthrough");
    }

    #[tokio::test]
    async fn scrubs_secrets_from_child_output() {
        // The child prints a key-shaped token; the executor must redact it
        // before returning. Token assembled at runtime so no literal secret
        // sits in source (scanner-safe).
        let secret = format!("sk-{}", "leakme1234567890abcdefghij");
        let out = execute_sandboxed(
            &MpSandboxPolicy::DangerFullAccess,
            "echo",
            &args(&[secret.as_str()]),
        )
        .await
        .expect("spawn echo");
        assert!(
            !out.stdout.contains(secret.as_str()),
            "secret leaked through executor: {:?}",
            out.stdout
        );
    }

    #[tokio::test]
    async fn captures_nonzero_exit_code() {
        let out = execute_sandboxed(
            &MpSandboxPolicy::DangerFullAccess,
            "sh",
            &args(&["-c", "exit 3"]),
        )
        .await
        .expect("spawn sh");
        assert_eq!(out.exit_code, 3);
    }

    /// A child that never exits must not hold the step open forever — that was
    /// the actual gap: no timeout existed anywhere on this path, so one hung
    /// `shell` call pinned its gRPC request indefinitely.
    #[tokio::test]
    async fn a_hung_child_is_killed_at_the_timeout() {
        let started = std::time::Instant::now();
        let result = execute_sandboxed_with_timeout(
            &MpSandboxPolicy::DangerFullAccess,
            "sh",
            &args(&["-c", "sleep 300"]),
            Duration::from_secs(1),
        )
        .await;
        let err = result.expect_err("a 300s sleep must not outlive a 1s timeout");
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "the kill must happen at the timeout, not at child exit"
        );
    }

    /// The code-execution path spawns a program *inside* a per-call workspace and
    /// hands it a closed environment, so three things must hold on the real spawn
    /// (including the no-bubblewrap passthrough this test exercises): the cwd
    /// applies, the allowlist applies, and the inherited environment is GONE.
    #[tokio::test]
    async fn cwd_applies_and_the_inherited_environment_is_replaced() {
        // A variable the parent definitely has and no shell can invent.
        // HOME is NOT usable as the probe: MSYS/Git-Bash's `sh.exe` synthesises
        // it from the Windows user even when spawned with a fully cleared
        // environment (verified: with only PATH passed, the child reports
        // USERPROFILE absent — so the clear worked — yet HOME present). Probing
        // HOME therefore fails on a developer host while the invariant it
        // stands for is intact, which is exactly the kind of red test that
        // stops being read.
        let canary = std::env::vars()
            .map(|(key, _)| key)
            .find(|key| {
                !key.eq_ignore_ascii_case("home")
                    && !key.eq_ignore_ascii_case("path")
                    && key != "VEREVON_EXEC_TEST"
            })
            .expect("the test runner has an environment");
        let parent_home = std::env::var("HOME").unwrap_or_default();
        let dir = std::env::temp_dir().join(format!("verevon-exec-cwd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        let canonical = std::fs::canonicalize(&dir).expect("canonicalize test dir");
        let out = execute_sandboxed_in_dir(
            &MpSandboxPolicy::DangerFullAccess,
            "sh",
            &args(&[
                "-c",
                &format!(
                    "printf '%s|%s|%s' \"$(pwd -P)\" \"$VEREVON_EXEC_TEST\" \"${{{canary}:-absent}}\""
                ),
            ]),
            &dir,
            // PATH must be in the allowlist or `sh` itself could not be resolved
            // — the same reason the code path always passes it through.
            &[
                ("PATH".to_owned(), std::env::var("PATH").unwrap_or_default()),
                ("VEREVON_EXEC_TEST".to_owned(), "injected".to_owned()),
            ],
            Duration::from_secs(10),
        )
        .await
        .expect("spawn sh");
        std::fs::remove_dir_all(&dir).expect("cleanup test dir");
        // Compared field by field rather than against one formatted path.
        // `canonicalize` renders a Windows path (`C:\...`, sometimes with a
        // `\?\` prefix) while the shell reports its own (`/c/...`), so a
        // whole-string equality failed on developer machines for a reason that
        // had nothing to do with the three invariants below — and a test that
        // is red for a spurious reason stops being read. The unique directory
        // name is what actually proves the cwd applied, in any path syntax.
        let workspace = dir
            .file_name()
            .expect("the test workspace has a name")
            .to_string_lossy()
            .into_owned();
        let fields: Vec<&str> = out.stdout.split('|').collect();
        assert_eq!(fields.len(), 3, "unexpected probe output: {}", out.stdout);
        assert!(
            fields[0].ends_with(&workspace),
            "the per-call workspace must be the cwd, got {} (canonical {})",
            fields[0],
            canonical.display()
        );
        assert_eq!(fields[1], "injected", "the env allowlist must apply");
        assert_eq!(
            fields[2], "absent",
            "the inherited environment must be gone: the child still sees {canary}"
        );
        assert!(
            !out.stdout.contains(&parent_home),
            "the parent environment must not reach the child"
        );
        assert_eq!(out.exit_code, 0);
    }

    /// The code timeout knob has to be REAL: a deployment that sets
    /// `EXECUTION_CODE_TIMEOUT_SECS` must get that value, and anything
    /// nonsensical must fall back rather than produce a zero-second deadline.
    #[test]
    fn the_timeout_knob_is_parsed_with_a_safe_fallback() {
        assert_eq!(
            parse_timeout_secs(Some("45"), DEFAULT_CODE_TIMEOUT_SECS),
            Duration::from_secs(45)
        );
        assert_eq!(
            parse_timeout_secs(Some(" 90 "), DEFAULT_CODE_TIMEOUT_SECS),
            Duration::from_secs(90)
        );
        for bad in [None, Some(""), Some("0"), Some("-5"), Some("soon")] {
            assert_eq!(
                parse_timeout_secs(bad, DEFAULT_CODE_TIMEOUT_SECS),
                Duration::from_secs(DEFAULT_CODE_TIMEOUT_SECS),
                "input {bad:?} must fall back to the default"
            );
        }
    }

    /// Unbounded child output must not ride into the step result verbatim.
    #[tokio::test]
    async fn oversized_output_is_capped_with_a_marker() {
        let out = execute_sandboxed(
            &MpSandboxPolicy::DangerFullAccess,
            "sh",
            // ~200 KiB of 'y' lines, well past the 64 KiB cap.
            &args(&["-c", "yes | head -c 200000"]),
        )
        .await
        .expect("spawn sh");
        assert!(out.stdout.contains("[output truncated"));
        assert!(
            out.stdout.chars().count() < MAX_OUTPUT_CHARS + 100,
            "capped output must stay near the cap"
        );
    }

    // ---- WS06-S7: bounded output, env parsing, spawn admission --------------------------------

    #[test]
    fn parse_positive_falls_back_on_missing_zero_and_garbage() {
        assert_eq!(parse_positive(None, 7), 7);
        assert_eq!(parse_positive(Some("0"), 7), 7);
        assert_eq!(parse_positive(Some(""), 7), 7);
        assert_eq!(parse_positive(Some("abc"), 7), 7);
        assert_eq!(parse_positive(Some("-5"), 7), 7);
        assert_eq!(parse_positive(Some("1.5"), 7), 7);
        assert_eq!(parse_positive(Some("99999999999999999999999"), 7), 7);
        assert_eq!(parse_positive(Some("42"), 7), 42);
        assert_eq!(parse_positive(Some(" 42 "), 7), 42);
    }

    #[test]
    fn the_shipped_defaults_match_the_compose_files() {
        assert_eq!(DEFAULT_MAX_OUTPUT_BYTES, 1_048_576);
        assert_eq!(DEFAULT_MAX_CONCURRENT_SPAWNS, 8);
        assert_eq!(DEFAULT_QUEUE_WAIT_MS, 5000);
    }

    #[tokio::test]
    async fn read_capped_stops_at_the_cap_and_reports_it() {
        let data = vec![b'a'; 20_000];
        let mut buf = Vec::new();
        let hit = read_capped(&data[..], &mut buf, 10_000)
            .await
            .expect("read");
        assert!(hit, "more than the cap must be reported");
        assert_eq!(buf.len(), 10_000, "the buffer holds exactly the cap");
    }

    #[tokio::test]
    async fn read_capped_does_not_report_output_that_exactly_fits() {
        let data = vec![b'a'; 10_000];
        let mut buf = Vec::new();
        let hit = read_capped(&data[..], &mut buf, 10_000)
            .await
            .expect("read");
        assert!(!hit, "exactly the cap is not past the cap");
        assert_eq!(buf.len(), 10_000);

        let mut small = Vec::new();
        assert!(!read_capped(&b"hi"[..], &mut small, 10_000)
            .await
            .expect("read"));
        assert_eq!(small, b"hi");
    }

    /// A runaway child must be killed at the cap, not buffered: execution-core's memory stays
    /// bounded by the cap (the returned text is itself capped far below it), and the outcome says
    /// which ceiling tripped.
    #[tokio::test]
    async fn a_child_printing_100_mb_is_killed_at_the_output_cap() {
        let started = std::time::Instant::now();
        let out = execute_sandboxed_with_timeout(
            &MpSandboxPolicy::DangerFullAccess,
            "sh",
            &args(&["-c", "yes | head -c 104857600"]),
            Duration::from_secs(120),
        )
        .await
        .expect("spawn sh");
        assert_eq!(
            out.limit_hit,
            Some(LimitHit::Output {
                stream: OutputStream::Stdout,
                limit_bytes: max_output_bytes(),
            }),
            "stdout past the cap must report Output"
        );
        assert!(
            out.stdout.chars().count() < MAX_OUTPUT_CHARS + 100,
            "the returned text stays near the 64 KiB step cap, not the 100 MB the child printed"
        );
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "the kill must happen at the cap, not after the child finished printing"
        );
    }

    #[tokio::test]
    async fn stderr_past_the_cap_is_killed_and_attributed_to_stderr() {
        let out = execute_sandboxed_with_timeout(
            &MpSandboxPolicy::DangerFullAccess,
            "sh",
            &args(&["-c", "yes | head -c 104857600 >&2"]),
            Duration::from_secs(120),
        )
        .await
        .expect("spawn sh");
        assert_eq!(
            out.limit_hit,
            Some(LimitHit::Output {
                stream: OutputStream::Stderr,
                limit_bytes: max_output_bytes(),
            })
        );
    }

    #[tokio::test]
    async fn a_normal_exit_reports_no_limit_hit() {
        let out = execute_sandboxed(
            &MpSandboxPolicy::DangerFullAccess,
            "echo",
            &args(&["hello"]),
        )
        .await
        .expect("spawn echo");
        assert_eq!(out.limit_hit, None);
        assert_eq!(out.exit_code, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn acquire_spawn_permit_times_out_with_would_block_when_the_gate_is_exhausted() {
        let mut held = hold_every_spawn_permit_for_test().await;
        let err = acquire_spawn_permit().await.expect_err("the gate is full");
        assert_eq!(err.kind(), std::io::ErrorKind::WouldBlock);
        assert_eq!(err.to_string(), "sandbox is busy, retry later");

        // Releasing the permits makes the gate usable again.
        held.release_permits();
        assert!(acquire_spawn_permit().await.is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn a_sandboxed_spawn_is_refused_with_would_block_when_the_gate_is_full() {
        let _held = hold_every_spawn_permit_for_test().await;
        let err = execute_sandboxed(
            &MpSandboxPolicy::DangerFullAccess,
            "echo",
            &args(&["never runs"]),
        )
        .await
        .expect_err("admission must refuse before any spawn");
        assert_eq!(err.kind(), std::io::ErrorKind::WouldBlock);
        assert_eq!(err.to_string(), "sandbox is busy, retry later");
    }
}
