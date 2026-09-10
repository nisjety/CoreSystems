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
//! passthrough unless the host is Linux *and* bubblewrap actually works here
//! ([`is_supported`] probes it once). Wiring it into the tool-exec path
//! therefore changes nothing on hosts without bubblewrap, and transparently
//! adds isolation where it is available — a progressive enhancement, not a
//! breaking change.
//!
//! # Two constraints the argv is shaped by (both found by running it for real
//! inside the deployed container, not by unit tests)
//!
//! 1. **No `--proc /proc`.** Mounting a fresh procfs alongside `--unshare-pid`
//!    needs `CAP_SYS_ADMIN`, and the service container runs with `CapEff=0`, so
//!    bubblewrap died before exec with `Can't mount proc on /newroot/proc:
//!    Operation not permitted` — every sandboxed call failed. Granting
//!    `SYS_ADMIN` to the one component that runs model-authored code would be a
//!    far worse trade, so the flag is simply gone, and the consequence is written
//!    down here rather than silently accepted:
//!
//!    The child gets a fresh PID namespace but the HOST procfs stays visible
//!    (read-only), so a program can read execution-core's own process list and
//!    `/proc/<pid>/cmdline`. Measured in the deployed container: `/proc` shows 62
//!    entries, and `/proc/1/environ` is **not** readable (`PermissionError` —
//!    bubblewrap's new user namespace makes the cross-process ptrace check fail),
//!    so this is a process-list disclosure, not a second route to the secrets that
//!    2 removes. Also measured: `/tmp` is read-only inside the sandbox (the
//!    earlier `--tmpfs /tmp` is shadowed by the `--ro-bind / /` that follows it),
//!    which is why anything needing scratch space must be pointed at a writable
//!    root. `--tmpfs /proc` placed AFTER the `--ro-bind` does work under
//!    `CapEff=0` and empties `/proc` entirely; it is not adopted because an empty
//!    `/proc` is untested against the numeric stack (`OpenBLAS` reads
//!    `/proc/cpuinfo`), and breaking real work to hide a process list is the wrong
//!    trade to make blind.
//! 2. **The environment is CLEARED, not inherited.** execution-core's own
//!    environment carries every cross-plane service credential it holds; a
//!    sandbox that inherits it hands `os.environ` to model-authored code. Code
//!    execution therefore launches with [`SandboxEnv::Only`] — `--clearenv` plus
//!    an explicit `--setenv` allowlist — and the spawn site clears the
//!    passthrough environment too, so a host without bubblewrap is not a hole.
//!    Only `shell`, which runs operator-authored commands, still inherits.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

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

/// How the sandboxed process's environment is derived from execution-core's.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SandboxEnv<'a> {
    /// Inherit execution-core's environment. Correct only for
    /// operator-authored commands (`shell`).
    #[default]
    Inherit,
    /// Start from an EMPTY environment and set exactly these pairs.
    ///
    /// Required for anything model-authored: execution-core's environment holds
    /// the internal keys it uses to reach the other planes, so inheriting it
    /// makes `print(os.environ)` a credential-exfiltration path. Output
    /// scrubbing is not a substitute — it cannot recognise every opaque value.
    Only(&'a [(String, String)]),
}

/// Launch shape for one sandboxed process, beyond the isolation policy itself.
#[derive(Debug, Clone, Copy, Default)]
pub struct LaunchOptions<'a> {
    /// Working directory for the child (`bwrap --chdir`).
    ///
    /// Passing the cwd explicitly matters for tools whose contract IS the
    /// working directory (code execution writing `out.xlsx` into its workspace):
    /// without `--chdir`, bwrap only *tries* to reuse the parent's cwd and, when
    /// that path does not exist inside the sandbox, merely warns and lands the
    /// child in `/` — so relative writes would silently go to the read-only root
    /// instead of the workspace. With `--chdir` a bad cwd is a loud failure.
    pub cwd: Option<&'a Path>,
    /// Environment derivation. Defaults to [`SandboxEnv::Inherit`].
    pub env: SandboxEnv<'a>,
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
    build_bwrap_argv_with(policy, program, args, LaunchOptions::default())
}

/// [`build_bwrap_argv`] with an explicit working directory and/or environment
/// policy — see [`LaunchOptions`] and [`SandboxEnv`] for why each exists.
#[must_use]
pub fn build_bwrap_argv_with(
    policy: &MpSandboxPolicy,
    program: &str,
    args: &[String],
    options: LaunchOptions<'_>,
) -> Option<Vec<String>> {
    match policy {
        MpSandboxPolicy::DangerFullAccess | MpSandboxPolicy::External { .. } => None,
        MpSandboxPolicy::ReadOnly { network } => {
            Some(assemble_argv(&[], network, program, args, options))
        }
        MpSandboxPolicy::WorkspaceWrite {
            writable_roots,
            network,
        } => Some(assemble_argv(
            writable_roots,
            network,
            program,
            args,
            options,
        )),
    }
}

fn assemble_argv(
    writable_roots: &[PathBuf],
    network: &MpNetworkPolicy,
    program: &str,
    cmd_args: &[String],
    options: LaunchOptions<'_>,
) -> Vec<String> {
    // DELIBERATELY no `--proc /proc`: with `--unshare-pid` it requires
    // CAP_SYS_ADMIN, which the service container does not have, and bwrap then
    // fails before exec — see the module header for the trade-off this accepts.
    let mut argv: Vec<String> = vec![
        "--die-with-parent".into(),
        "--unshare-pid".into(),
        "--unshare-uts".into(),
        "--unshare-ipc".into(),
        "--new-session".into(),
        "--dev".into(),
        "/dev".into(),
        "--tmpfs".into(),
        "/tmp".into(),
        // Read-only root baseline; writable roots are bound over it below.
        "--ro-bind".into(),
        "/".into(),
        "/".into(),
    ];

    // `--clearenv` FIRST, then the allowlist: bwrap applies options in order, so
    // a `--setenv` emitted before the clear would simply be wiped.
    if let SandboxEnv::Only(allowlist) = options.env {
        argv.push("--clearenv".into());
        for (key, value) in allowlist {
            argv.push("--setenv".into());
            argv.push(key.clone());
            argv.push(value.clone());
        }
    }

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
    //
    // SECURITY (G1 executor requirement): `AllowDomains` is NOT self-enforcing
    // here — bwrap keeps the network up and the allowlist lives at the proxy.
    // The executor that spawns this argv MUST fail closed when no egress proxy
    // is configured (e.g. `HTTPS_PROXY`/`ALL_PROXY` unset): otherwise the
    // process gets unrestricted egress, silently defeating the allowlist. This
    // pure builder stays env-free by design; the check belongs at the spawn
    // site. Tracked in capability-ownership-matrix §G1.
    if matches!(network, MpNetworkPolicy::Disabled) {
        argv.push("--unshare-net".into());
    }

    // Placed after the binds so the directory it names is already mounted in the
    // sandbox by the time bwrap chdirs into it.
    if let Some(dir) = options.cwd {
        argv.push("--chdir".into());
        argv.push(dir.to_string_lossy().into_owned());
    }

    argv.push("--".into());
    argv.push(program.to_owned());
    argv.extend(cmd_args.iter().cloned());
    argv
}

/// Whether OS sandboxing actually WORKS on this host, decided once per process.
///
/// This is a real one-shot probe (`bwrap --ro-bind / / --unshare-pid -- true`),
/// not a `PATH` lookup. A `PATH` check is what let a container-level capability
/// problem — bubblewrap present but unable to set up a namespace — present itself
/// as a generic per-call tool failure instead of one loud line at startup.
///
/// Probing costs one process spawn, cached in a `OnceLock`; never probe per call.
/// Call [`log_support`] at startup so the result is on the record before the
/// first tool call.
#[must_use]
pub fn is_supported() -> bool {
    static SUPPORTED: OnceLock<bool> = OnceLock::new();
    *SUPPORTED.get_or_init(probe_bwrap)
}

/// Force the [`is_supported`] probe and record the outcome. Call once at startup.
pub fn log_support() {
    if is_supported() {
        tracing::info!(
            sandbox = "bubblewrap",
            "OS sandbox verified: tool processes run isolated"
        );
    } else {
        tracing::warn!(
            sandbox = "none",
            "bubblewrap is unavailable or unusable here: sandboxed tool processes \
             are refused rather than downgraded to host execution"
        );
    }
}

/// A small, runtime-measured profile for callers that must decide whether a
/// Space can use this execution substrate. It deliberately reports what the
/// local executor can prove today, rather than promising a durable computer:
/// work is credential-free, bounded to one child process, and has no backup or
/// durable workspace. A caller needing any stronger property must select an
/// explicit external, attested backend instead of treating this process as a
/// compatible fallback.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SandboxCapabilityProfile {
    /// `bubblewrap` when a real namespace probe passed; otherwise `unavailable`.
    pub backend: &'static str,
    /// Whether restricted Model-authored work can execute on this host.
    pub local_isolation_available: bool,
    /// This executor never promises a durable Space workspace.
    pub persistence: &'static str,
    /// Only bounded one-shot children are supported; no detached process lease.
    pub processes: &'static str,
    /// Snapshots/backups are not a property of this substrate.
    pub backup: bool,
    /// Model-authored restricted policies start with egress disabled.
    pub egress: &'static str,
    /// The executor never forwards service credentials to model-authored code.
    pub credential_mode: &'static str,
}

/// A canonical, order-stable digest of the measured profile, bound into the
/// Control-issued Space capability decision's payload digest (see
/// `apps/Frontend Plane/verevonv3/docs/S3_2_SANDBOX_LEASE_CLOSEOUT_DESIGN_2026-09-10.md`
/// §1). Serializing the struct directly (rather than hand-listing fields) is
/// safe here only because every field has a fixed, small set of possible
/// values and `#[derive(Serialize)]` emits struct fields in declaration
/// order, which `serde_json` preserves — this is not a general-purpose
/// canonicalization and must not be copied for a type with a `HashMap` field
/// or other genuinely unordered data.
#[must_use]
pub fn capability_profile_digest(profile: &SandboxCapabilityProfile) -> String {
    use sha2::{Digest, Sha256};
    let canonical =
        serde_json::to_vec(profile).expect("SandboxCapabilityProfile fields are all primitives");
    format!("sha256:{:x}", Sha256::digest(canonical))
}

/// Return the substrate contract measured on this process. This is intentionally
/// independent of a Space lease: a lease must pin this profile (or a stronger
/// external profile) before it is used for an effect.
#[must_use]
pub fn capability_profile() -> SandboxCapabilityProfile {
    let local_isolation_available = is_supported();
    SandboxCapabilityProfile {
        backend: if local_isolation_available {
            "bubblewrap"
        } else {
            "unavailable"
        },
        local_isolation_available,
        persistence: "ephemeral",
        processes: "bounded_oneshot",
        backup: false,
        egress: "disabled_by_default",
        credential_mode: "credential_free",
    }
}

#[cfg(target_os = "linux")]
fn probe_bwrap() -> bool {
    // The smallest argv that exercises what the real one needs: a read-only
    // rootfs plus a PID namespace. `true` comes from the read-only bind, so a
    // success means bwrap can genuinely build the sandbox here, not merely that
    // the binary exists.
    match std::process::Command::new(BWRAP_BIN)
        .args(["--ro-bind", "/", "/", "--unshare-pid", "--", "true"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}

#[cfg(not(target_os = "linux"))]
fn probe_bwrap() -> bool {
    false
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
    wrap_command_with(policy, program, args, LaunchOptions::default())
}

/// [`wrap_command`] with an explicit working directory and/or environment policy
/// (see [`LaunchOptions`]).
///
/// The spawn site must apply the same two things to the process it launches: on a
/// host without bubblewrap this is a passthrough, so `--chdir` and `--clearenv`
/// are not there to do it — see [`crate::executor::execute_sandboxed_in_dir`].
#[must_use]
pub fn wrap_command_with(
    policy: &MpSandboxPolicy,
    program: &str,
    args: &[String],
    options: LaunchOptions<'_>,
) -> SandboxedCommand {
    if is_supported() {
        if let Some(argv) = build_bwrap_argv_with(policy, program, args, options) {
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
    fn capability_profile_never_claims_durable_or_credentialed_workspace() {
        let profile = capability_profile();
        assert!(matches!(profile.backend, "bubblewrap" | "unavailable"));
        assert_eq!(profile.local_isolation_available, is_supported());
        assert_eq!(profile.persistence, "ephemeral");
        assert_eq!(profile.processes, "bounded_oneshot");
        assert!(!profile.backup);
        assert_eq!(profile.egress, "disabled_by_default");
        assert_eq!(profile.credential_mode, "credential_free");
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

    /// The argv must NOT mount a fresh procfs. `--proc /proc` together with
    /// `--unshare-pid` requires `CAP_SYS_ADMIN`, and the service container runs
    /// with none, so bubblewrap died before exec ("Can't mount proc on
    /// `/newroot/proc`: Operation not permitted") and EVERY sandboxed call failed.
    /// Verified by running the argv inside the real container — keep it out.
    #[test]
    fn the_argv_never_mounts_a_fresh_proc() {
        for policy in [
            MpSandboxPolicy::ReadOnly {
                network: MpNetworkPolicy::Disabled,
            },
            MpSandboxPolicy::WorkspaceWrite {
                writable_roots: vec![PathBuf::from("/work")],
                network: MpNetworkPolicy::Disabled,
            },
        ] {
            let argv = build_bwrap_argv(&policy, "true", &[]).expect("wrapped");
            assert!(
                !argv.iter().any(|a| a == "--proc"),
                "--proc needs CAP_SYS_ADMIN and breaks every call: {argv:?}"
            );
            // The rest of the in-container-verified isolation set must stay.
            for flag in [
                "--die-with-parent",
                "--unshare-pid",
                "--unshare-uts",
                "--unshare-ipc",
                "--new-session",
                "--unshare-net",
            ] {
                assert!(argv.iter().any(|a| a == flag), "missing {flag}");
            }
            assert!(windowed(&argv, &["--dev", "/dev"]));
            assert!(windowed(&argv, &["--tmpfs", "/tmp"]));
            assert!(windowed(&argv, &["--ro-bind", "/", "/"]));
        }
    }

    /// Model-authored code must not inherit execution-core's environment (it holds
    /// the internal keys for every other plane). `--clearenv` has to come FIRST —
    /// bwrap applies options in order, so an allowlist emitted before the clear
    /// would simply be wiped — and `shell`, which runs operator-authored commands,
    /// must keep inheriting.
    #[test]
    fn an_env_allowlist_clears_first_and_inheritance_stays_opt_in() {
        let ws = PathBuf::from("/tmp/verevon-code-x");
        let p = MpSandboxPolicy::WorkspaceWrite {
            writable_roots: vec![ws.clone()],
            network: MpNetworkPolicy::Disabled,
        };
        let allowlist = vec![
            ("PATH".to_owned(), "/usr/bin:/bin".to_owned()),
            ("HOME".to_owned(), "/tmp/verevon-code-x".to_owned()),
        ];
        let argv = build_bwrap_argv_with(
            &p,
            "python3",
            &args(&["main.py"]),
            LaunchOptions {
                cwd: Some(&ws),
                env: SandboxEnv::Only(&allowlist),
            },
        )
        .expect("wrapped");
        let clear_at = argv
            .iter()
            .position(|a| a == "--clearenv")
            .expect("--clearenv");
        let first_setenv = argv.iter().position(|a| a == "--setenv").expect("--setenv");
        assert!(clear_at < first_setenv, "clear must precede the allowlist");
        assert!(windowed(&argv, &["--setenv", "PATH", "/usr/bin:/bin"]));
        assert!(windowed(
            &argv,
            &["--setenv", "HOME", "/tmp/verevon-code-x"]
        ));
        assert_eq!(
            argv.iter().filter(|a| *a == "--setenv").count(),
            allowlist.len(),
            "exactly the allowlist, nothing else: {argv:?}"
        );
        let terminator = argv.iter().position(|a| a == "--").expect("terminator");
        assert!(first_setenv < terminator);

        // The shell path (default options) must be unchanged: still inherits.
        let inherited = build_bwrap_argv(&p, "sh", &args(&["-c", "echo hi"])).expect("wrapped");
        assert!(!inherited.iter().any(|a| a == "--clearenv"));
        assert!(!inherited.iter().any(|a| a == "--setenv"));
    }

    #[test]
    fn workspace_write_with_chdir_lands_the_child_in_its_workspace() {
        // The code-execution path depends on this: the program is spawned with
        // the workspace as its cwd so a relative `open("out.xlsx","wb")` writes
        // into the one writable root instead of the read-only rootfs.
        let ws = PathBuf::from("/tmp/verevon-code-run-step-0-1");
        let p = MpSandboxPolicy::WorkspaceWrite {
            writable_roots: vec![ws.clone()],
            network: MpNetworkPolicy::Disabled,
        };
        let options = LaunchOptions {
            cwd: Some(&ws),
            ..LaunchOptions::default()
        };
        let argv =
            build_bwrap_argv_with(&p, "python3", &args(&["main.py"]), options).expect("wrapped");
        assert!(windowed(
            &argv,
            &[
                "--bind",
                "/tmp/verevon-code-run-step-0-1",
                "/tmp/verevon-code-run-step-0-1"
            ]
        ));
        assert!(windowed(
            &argv,
            &["--chdir", "/tmp/verevon-code-run-step-0-1"]
        ));
        assert!(argv.iter().any(|a| a == "--unshare-net"));
        // --chdir must precede the `--` terminator, i.e. be a bwrap option and
        // not an argument handed to the child program.
        let chdir_at = argv.iter().position(|a| a == "--chdir").expect("--chdir");
        let terminator = argv.iter().position(|a| a == "--").expect("terminator");
        assert!(chdir_at < terminator);
        assert!(windowed(&argv, &["--", "python3", "main.py"]));
        // The no-cwd builder must stay byte-identical to before (shell path).
        let plain = build_bwrap_argv(&p, "python3", &args(&["main.py"])).expect("wrapped");
        assert!(!plain.iter().any(|a| a == "--chdir"));
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
