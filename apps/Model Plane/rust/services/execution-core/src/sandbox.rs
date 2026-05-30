//! Sandbox launch translation — turns an [`MpSandboxPolicy`] into a concrete
//! command invocation for running tool processes under OS isolation.
//!
//! Per `docs/capability-ownership-matrix.md` §G1. The bubblewrap (`bwrap`)
//! argv construction is **pure and unit-tested on any OS**; the `bwrap`
//! binary and Landlock/seccomp application are Linux-only, so the runtime
//! availability check is `#[cfg(target_os = "linux")]`-gated and this module
//! compiles cleanly on macOS/dev hosts (the Linux-only paths just aren't
//! built).
//!
//! Consumes the canonical [`crate::policy::MpSandboxPolicy`] (G3) — there is
//! deliberately no second sandbox vocabulary.
//!
//! **Safety-by-default for rollout:** [`wrap_command`] is a transparent
//! passthrough unless the host is Linux *and* `bwrap` is on `PATH`. Wiring it
//! into the tool-exec path therefore changes nothing on hosts without
//! bubblewrap, and transparently adds isolation where it is available — a
//! progressive enhancement, not a breaking change.

use std::path::PathBuf;

use crate::policy::{MpNetworkPolicy, MpSandboxPolicy};

/// The bubblewrap binary used for filesystem/namespace isolation on Linux.
pub const BWRAP_BIN: &str = "bwrap";

/// A concrete, ready-to-spawn command: program + args, plus whether it was
/// actually wrapped in a sandbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxedCommand {
    pub program: String,
    pub args: Vec<String>,
    /// `true` if wrapped in bubblewrap; `false` for a direct passthrough
    /// (`DangerFullAccess`, `External` delegation, or sandbox unavailable).
    pub sandboxed: bool,
}

/// Build the bubblewrap argument vector for `policy`, wrapping `program args`.
///
/// Returns `None` when the policy implies no local bwrap wrapping:
/// `DangerFullAccess` (explicitly trusted) or `External` (isolation delegated
/// to sandbox-manager). Pure — no IO — and fully unit-testable on any OS.
#[must_use]
pub fn build_bwrap_argv(
    policy: &MpSandboxPolicy,
    program: &str,
    args: &[String],
) -> Option<Vec<String>> {
    match policy {
        MpSandboxPolicy::DangerFullAccess | MpSandboxPolicy::External { .. } => None,
        MpSandboxPolicy::ReadOnly { network } => Some(assemble_argv(&[], network, program, args)),
        MpSandboxPolicy::WorkspaceWrite {
            writable_roots,
            network,
        } => Some(assemble_argv(writable_roots, network, program, args)),
    }
}

fn assemble_argv(
    writable_roots: &[PathBuf],
    network: &MpNetworkPolicy,
    program: &str,
    args: &[String],
) -> Vec<String> {
    let mut argv: Vec<String> = vec![
        "--die-with-parent".into(),
        "--unshare-pid".into(),
        "--unshare-uts".into(),
        "--unshare-ipc".into(),
        "--new-session".into(),
        "--proc".into(),
        "/proc".into(),
        "--dev".into(),
        "/dev".into(),
        "--tmpfs".into(),
        "/tmp".into(),
        // Read-only root baseline; writable roots are bound over it below.
        "--ro-bind".into(),
        "/".into(),
        "/".into(),
    ];

    // Writable roots bound over the read-only root (later binds win in bwrap).
    for root in writable_roots {
        let p = root.to_string_lossy().into_owned();
        argv.push("--bind".into());
        argv.push(p.clone());
        argv.push(p);
    }

    // Only an unshared net namespace fully blocks egress. `AllowDomains` keeps
    // the namespace (so the egress proxy is reachable) and relies on the
    // network-proxy layer to enforce the allowlist; `AllowAll` shares the host
    // network; `Disabled` cuts the network entirely.
    if matches!(network, MpNetworkPolicy::Disabled) {
        argv.push("--unshare-net".into());
    }

    argv.push("--".into());
    argv.push(program.to_owned());
    argv.extend(args.iter().cloned());
    argv
}

/// Whether OS sandboxing is available on this host (Linux + `bwrap` on `PATH`).
#[must_use]
pub fn is_supported() -> bool {
    #[cfg(target_os = "linux")]
    {
        which_bwrap().is_some()
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

#[cfg(target_os = "linux")]
fn which_bwrap() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(BWRAP_BIN))
        .find(|p| p.is_file())
}

/// Translate a policy into a concrete command to spawn.
///
/// On Linux with `bwrap` available, wraps the command in bubblewrap; otherwise
/// (non-Linux, no `bwrap`, `DangerFullAccess`, or `External`) returns a direct
/// passthrough. Call this immediately before spawning a tool process. Because
/// it degrades to passthrough when bubblewrap is absent, wiring it is a
/// no-behaviour-change enhancement on hosts without `bwrap`.
#[must_use]
pub fn wrap_command(policy: &MpSandboxPolicy, program: &str, args: &[String]) -> SandboxedCommand {
    if is_supported() {
        if let Some(argv) = build_bwrap_argv(policy, program, args) {
            return SandboxedCommand {
                program: BWRAP_BIN.to_owned(),
                args: argv,
                sandboxed: true,
            };
        }
    }
    SandboxedCommand {
        program: program.to_owned(),
        args: args.to_vec(),
        sandboxed: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn read_only_no_network_blocks_writes_and_egress() {
        let p = MpSandboxPolicy::ReadOnly {
            network: MpNetworkPolicy::Disabled,
        };
        let argv = build_bwrap_argv(&p, "ls", &args(&["-la"])).expect("wrapped");
        // read-only root, no writable binds, network cut.
        assert!(windowed(&argv, &["--ro-bind", "/", "/"]));
        assert!(argv.iter().any(|a| a == "--unshare-net"));
        assert!(!argv.iter().any(|a| a == "--bind"));
        // program + args after the `--` terminator.
        assert!(windowed(&argv, &["--", "ls", "-la"]));
    }

    #[test]
    fn workspace_write_binds_each_writable_root() {
        let p = MpSandboxPolicy::WorkspaceWrite {
            writable_roots: vec![PathBuf::from("/work"), PathBuf::from("/tmp/agent")],
            network: MpNetworkPolicy::Disabled,
        };
        let argv = build_bwrap_argv(&p, "bash", &args(&["-c", "echo hi"])).expect("wrapped");
        assert!(windowed(&argv, &["--ro-bind", "/", "/"]));
        assert!(windowed(&argv, &["--bind", "/work", "/work"]));
        assert!(windowed(&argv, &["--bind", "/tmp/agent", "/tmp/agent"]));
    }

    #[test]
    fn allow_all_network_keeps_host_net() {
        let p = MpSandboxPolicy::ReadOnly {
            network: MpNetworkPolicy::AllowAll,
        };
        let argv = build_bwrap_argv(&p, "curl", &args(&["x"])).expect("wrapped");
        assert!(!argv.iter().any(|a| a == "--unshare-net"));
    }

    #[test]
    fn allow_domains_keeps_namespace_for_proxy() {
        // AllowDomains must NOT unshare net — the egress proxy needs to be
        // reachable; the allowlist is enforced at the proxy, not by bwrap.
        let p = MpSandboxPolicy::ReadOnly {
            network: MpNetworkPolicy::AllowDomains(vec!["api.openai.com".to_owned()]),
        };
        let argv = build_bwrap_argv(&p, "curl", &args(&["x"])).expect("wrapped");
        assert!(!argv.iter().any(|a| a == "--unshare-net"));
    }

    #[test]
    fn danger_full_access_and_external_are_passthrough() {
        assert!(build_bwrap_argv(&MpSandboxPolicy::DangerFullAccess, "sh", &[]).is_none());
        assert!(build_bwrap_argv(
            &MpSandboxPolicy::External {
                network: MpNetworkPolicy::AllowAll
            },
            "sh",
            &[]
        )
        .is_none());
    }

    #[test]
    fn wrap_command_passthrough_when_unsupported() {
        // On the CI/dev host (non-Linux or no bwrap), wrap_command must be a
        // transparent passthrough — proving the no-behaviour-change property.
        if !is_supported() {
            let cmd = wrap_command(&MpSandboxPolicy::default(), "echo", &args(&["hi"]));
            assert!(!cmd.sandboxed);
            assert_eq!(cmd.program, "echo");
            assert_eq!(cmd.args, args(&["hi"]));
        }
    }

    /// True if `needle` appears as a contiguous subsequence of `haystack`.
    fn windowed(haystack: &[String], needle: &[&str]) -> bool {
        if needle.is_empty() || needle.len() > haystack.len() {
            return false;
        }
        haystack
            .windows(needle.len())
            .any(|w| w.iter().zip(needle).all(|(a, b)| a == b))
    }
}
