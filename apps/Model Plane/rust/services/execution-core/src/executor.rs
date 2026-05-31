//! Real sandboxed tool-process executor (matrix §G1).
//!
//! The `tool_bridge` dispatch is a deterministic stub — its catch-all returns
//! canned text and spawns nothing. This module is the missing real-execution
//! primitive: build argv, wrap it in the sandbox ([`sandbox::wrap_command`] —
//! bubblewrap on Linux, transparent passthrough elsewhere), spawn the process,
//! capture its output, and **scrub secrets** from that output before it leaves
//! the execution boundary.
//!
//! # Scope & verification (built unverified per the user's call; e2e flagged)
//!
//! [`execute_sandboxed`] is the primitive. Its spawn → capture → scrub path is
//! unit-tested cross-platform via a benign command (passthrough applies when
//! bubblewrap is absent, e.g. on macOS/CI). The bubblewrap **isolation** itself
//! is Linux-gated and verified separately (`sandbox.rs` tests +
//! `scripts/verify-sandbox-isolation.sh`).
//!
//! What remains the **integration decision** (deliberately NOT wired here): how
//! this plugs into tool dispatch — an async call from `runtime_loop` for
//! shell-class tools is the natural seam (`tool_bridge::execute` is sync), but
//! that is the executor-design call flagged for the team. This primitive is
//! standalone + tested so the wiring is mechanical once that's decided.
//!
//! SECURITY: the `AllowDomains` network policy is not self-enforcing (see
//! `sandbox.rs`) — a caller using it MUST also run behind a configured egress
//! proxy or the process gets unrestricted egress. `execute_sandboxed` surfaces
//! `sandboxed` so callers can refuse to run an un-sandboxed process under a
//! policy that requires isolation.

use crate::policy::MpSandboxPolicy;
use crate::{sandbox, scrub};

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
}

/// Execute `program args` under `policy`, returning scrubbed output.
///
/// Output always passes through [`scrub::scrub_string`] before returning, so a
/// child that prints a secret cannot leak it across the boundary.
///
/// # Errors
/// Returns the underlying [`std::io::Error`] if the process cannot be spawned
/// or its output collected.
pub async fn execute_sandboxed(
    policy: &MpSandboxPolicy,
    program: &str,
    args: &[String],
) -> std::io::Result<ExecOutcome> {
    let cmd = sandbox::wrap_command(policy, program, args);
    let output = tokio::process::Command::new(&cmd.program)
        .args(&cmd.args)
        .output()
        .await?;
    Ok(ExecOutcome {
        stdout: scrub::scrub_string(&String::from_utf8_lossy(&output.stdout)),
        stderr: scrub::scrub_string(&String::from_utf8_lossy(&output.stderr)),
        exit_code: output.status.code().unwrap_or(-1),
        sandboxed: cmd.sandboxed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
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
}
