//! Sandbox & network isolation policy — the canonical vocabulary that
//! execution-core's runtime loop branches on when launching tool processes,
//! and that sandbox-manager consumes when provisioning isolation.
//!
//! Added per `docs/capability-ownership-matrix.md` §G3. The enum shape is
//! adapted from `OpenAI` Codex's `SandboxPolicy` / `PermissionProfile`
//! (Apache-2.0), independently reimplemented; the [`MpNetworkPolicy::AllowDomains`]
//! egress-allowlist variant is our extension.
//!
//! This is the **only** sandbox-policy type in the Model Plane — per matrix
//! §3 the "real OS isolation" owner is execution-core. Do not introduce a
//! parallel policy enum elsewhere; promote this to a proto message only when
//! it must cross the execution-core ↔ sandbox-manager boundary (Tier 2 / G1).
//!
//! Enforcement (bubblewrap / Landlock / seccomp on Linux, container/egress
//! provisioning in sandbox-manager) lands in G1 and reads this policy; this
//! module defines the policy and its safe defaults.

use std::path::PathBuf;

/// Network egress policy for a sandboxed process.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum MpNetworkPolicy {
    /// No network access at all. Default — safest.
    #[default]
    Disabled,
    /// Unrestricted egress.
    AllowAll,
    /// Egress permitted only to the listed domains. Enforced by the
    /// network-proxy layer (G1); until that lands, treat as "restricted —
    /// deny by default if the proxy is unavailable".
    AllowDomains(Vec<String>),
}

impl MpNetworkPolicy {
    /// True if any outbound network is permitted.
    #[must_use]
    pub fn allows_any_network(&self) -> bool {
        !matches!(self, Self::Disabled)
    }

    /// True if egress to `domain` is permitted under this policy.
    #[must_use]
    pub fn allows_domain(&self, domain: &str) -> bool {
        match self {
            Self::Disabled => false,
            Self::AllowAll => true,
            Self::AllowDomains(list) => list.iter().any(|d| d == domain),
        }
    }
}

/// Filesystem + network isolation policy applied to a single tool execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MpSandboxPolicy {
    /// No sandbox — full host access. Reserved for explicitly trusted runs
    /// (operator opt-in). Implies unrestricted network.
    DangerFullAccess,
    /// Read-only filesystem; network governed by the embedded policy.
    ReadOnly { network: MpNetworkPolicy },
    /// Writes confined to `writable_roots`; the rest of the filesystem is
    /// read-only; network governed by the embedded policy.
    WorkspaceWrite {
        writable_roots: Vec<PathBuf>,
        network: MpNetworkPolicy,
    },
    /// Isolation delegated to an external provisioner (sandbox-manager
    /// container / Daytona). execution-core forwards only the network policy.
    External { network: MpNetworkPolicy },
}

impl Default for MpSandboxPolicy {
    /// Safest default: read-only filesystem, no network.
    fn default() -> Self {
        Self::ReadOnly {
            network: MpNetworkPolicy::Disabled,
        }
    }
}

impl MpSandboxPolicy {
    /// True if the policy forbids all filesystem writes.
    #[must_use]
    pub fn is_read_only(&self) -> bool {
        matches!(self, Self::ReadOnly { .. })
    }

    /// True if the policy disables sandboxing entirely (host access).
    #[must_use]
    pub fn is_full_access(&self) -> bool {
        matches!(self, Self::DangerFullAccess)
    }

    /// The writable roots granted by this policy (empty unless `WorkspaceWrite`).
    #[must_use]
    pub fn writable_roots(&self) -> &[PathBuf] {
        match self {
            Self::WorkspaceWrite { writable_roots, .. } => writable_roots,
            _ => &[],
        }
    }

    /// The effective network policy. `DangerFullAccess` implies `AllowAll`.
    #[must_use]
    pub fn network(&self) -> MpNetworkPolicy {
        match self {
            Self::DangerFullAccess => MpNetworkPolicy::AllowAll,
            Self::ReadOnly { network }
            | Self::WorkspaceWrite { network, .. }
            | Self::External { network } => network.clone(),
        }
    }

    /// Build a policy from a filesystem-mode keyword plus a network policy.
    /// Unknown keywords fall back to the safe `ReadOnly` default. Accepts
    /// `_` or `-` separators and is case-insensitive
    /// (e.g. `"workspace-write"`, `"DANGER_FULL_ACCESS"`).
    #[must_use]
    pub fn from_keyword(keyword: &str, network: MpNetworkPolicy) -> Self {
        match keyword
            .trim()
            .to_ascii_lowercase()
            .replace('_', "-")
            .as_str()
        {
            "danger-full-access" | "full" => Self::DangerFullAccess,
            "workspace-write" | "write" => Self::WorkspaceWrite {
                writable_roots: Vec::new(),
                network,
            },
            "external" => Self::External { network },
            _ => Self::ReadOnly { network },
        }
    }
}

// ---------------------------------------------------------------------------
// Per-spawn resource limits (WS06-S8)
// ---------------------------------------------------------------------------

/// Whether per-spawn `prlimit` limits are applied (`EXECUTION_SANDBOX_LIMITS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitsMode {
    /// Apply limits to every sandboxed spawn; refuse the spawn if `prlimit`
    /// is unusable. The default, and what any unrecognised value means.
    Enforce,
    /// Spawn without per-spawn limits. Warned about at every start.
    Off,
}

impl LimitsMode {
    /// `off` (case-insensitive) is the only way out; anything else, including
    /// garbage or a missing value, enforces (fail closed).
    #[must_use]
    pub fn from_raw(raw: Option<&str>) -> Self {
        match raw.map(str::trim) {
            Some(value) if value.eq_ignore_ascii_case("off") => Self::Off,
            _ => Self::Enforce,
        }
    }
}

/// Limits for ONE spawn, rendered as `prlimit` flags. Inherited by `bwrap` and
/// everything under it. `MpSandboxPolicy` stays the one canonical isolation
/// vocabulary; this carries only numeric ceilings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceLimits {
    /// `RLIMIT_CPU`, seconds of CPU time (soft). A child over it gets SIGXCPU.
    pub cpu_secs: u64,
    /// `RLIMIT_AS`, bytes of virtual address space (not RSS; the container
    /// memory limit is the RSS backstop).
    pub address_space_bytes: u64,
    /// `RLIMIT_FSIZE`, bytes of the largest file the child may write.
    pub file_size_bytes: u64,
    /// `RLIMIT_NOFILE`, open file descriptors.
    pub open_files: u64,
}

/// The hard `RLIMIT_CPU` sits this many seconds above the soft one. With soft
/// equal to hard the kernel sends SIGKILL, never SIGXCPU, and a CPU overrun
/// could not be told apart from an OOM kill.
pub const CPU_HARD_GAP_SECS: u64 = 2;

impl ResourceLimits {
    /// The `prlimit` arguments for these limits, up to but excluding the `--`
    /// that separates them from the wrapped command. Pure, so it is unit
    /// testable on any OS.
    #[must_use]
    pub fn prlimit_args(&self) -> Vec<String> {
        vec![
            format!(
                "--cpu={}:{}",
                self.cpu_secs,
                self.cpu_secs.saturating_add(CPU_HARD_GAP_SECS)
            ),
            format!("--as={}", self.address_space_bytes),
            format!("--fsize={}", self.file_size_bytes),
            format!("--nofile={}", self.open_files),
            "--core=0".to_owned(),
        ]
    }
}

const DEFAULT_CPU_FACTOR: u64 = 2;
const DEFAULT_MAX_ADDRESS_SPACE_MB: u64 = 4096;
const DEFAULT_MAX_FILE_MB: u64 = 64;
const DEFAULT_MAX_OPEN_FILES: u64 = 1024;

/// The tunables behind [`ResourceLimits`], read once from the environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LimitsConfig {
    pub mode: LimitsMode,
    pub cpu_factor: u64,
    pub max_address_space_mb: u64,
    pub max_file_mb: u64,
    pub max_open_files: u64,
}

fn positive(raw: Option<&str>, default: u64) -> u64 {
    raw.and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|&value| value > 0)
        .unwrap_or(default)
}

impl LimitsConfig {
    /// Pure parse of the raw env values (env mutation is forbidden in this
    /// crate). Missing, zero or unparseable numbers fall back to the default.
    #[must_use]
    pub fn from_raw(
        mode: Option<&str>,
        cpu_factor: Option<&str>,
        address_space_mb: Option<&str>,
        file_mb: Option<&str>,
        open_files: Option<&str>,
    ) -> Self {
        Self {
            mode: LimitsMode::from_raw(mode),
            cpu_factor: positive(cpu_factor, DEFAULT_CPU_FACTOR),
            max_address_space_mb: positive(address_space_mb, DEFAULT_MAX_ADDRESS_SPACE_MB),
            max_file_mb: positive(file_mb, DEFAULT_MAX_FILE_MB),
            max_open_files: positive(open_files, DEFAULT_MAX_OPEN_FILES),
        }
    }

    /// The process-wide configuration, decided once.
    #[must_use]
    pub fn current() -> &'static Self {
        static CONFIG: std::sync::OnceLock<LimitsConfig> = std::sync::OnceLock::new();
        CONFIG.get_or_init(|| {
            let var = |name: &str| std::env::var(name).ok();
            Self::from_raw(
                var("EXECUTION_SANDBOX_LIMITS").as_deref(),
                var("EXECUTION_SANDBOX_CPU_FACTOR").as_deref(),
                var("EXECUTION_SANDBOX_MAX_ADDRESS_SPACE_MB").as_deref(),
                var("EXECUTION_SANDBOX_MAX_FILE_MB").as_deref(),
                var("EXECUTION_SANDBOX_MAX_OPEN_FILES").as_deref(),
            )
        })
    }

    /// The limits for one spawn whose wall-clock budget is `wall` (a one-shot
    /// call's timeout, or a background process's TTL), or `None` when limits
    /// are switched off. CPU seconds are `wall x cpu_factor`, at least 1.
    #[must_use]
    pub fn limits_for(&self, wall: std::time::Duration) -> Option<ResourceLimits> {
        if self.mode == LimitsMode::Off {
            return None;
        }
        Some(ResourceLimits {
            cpu_secs: wall.as_secs().saturating_mul(self.cpu_factor).max(1),
            address_space_bytes: self.max_address_space_mb.saturating_mul(1024 * 1024),
            file_size_bytes: self.max_file_mb.saturating_mul(1024 * 1024),
            open_files: self.max_open_files,
        })
    }
}

/// [`LimitsConfig::limits_for`] on the process-wide configuration.
#[must_use]
pub fn limits_for(wall: std::time::Duration) -> Option<ResourceLimits> {
    LimitsConfig::current().limits_for(wall)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_read_only_no_network() {
        let p = MpSandboxPolicy::default();
        assert!(p.is_read_only());
        assert!(!p.is_full_access());
        assert!(!p.network().allows_any_network());
    }

    #[test]
    fn danger_full_access_implies_allow_all_network() {
        let p = MpSandboxPolicy::DangerFullAccess;
        assert!(p.is_full_access());
        assert!(p.network().allows_any_network());
        assert!(p.network().allows_domain("anything.example.com"));
        assert!(p.writable_roots().is_empty());
    }

    #[test]
    fn workspace_write_exposes_writable_roots() {
        let p = MpSandboxPolicy::WorkspaceWrite {
            writable_roots: vec![PathBuf::from("/work"), PathBuf::from("/tmp/agent")],
            network: MpNetworkPolicy::Disabled,
        };
        assert!(!p.is_read_only());
        assert_eq!(p.writable_roots().len(), 2);
        assert!(!p.network().allows_any_network());
    }

    #[test]
    fn allow_domains_is_an_allowlist() {
        let net = MpNetworkPolicy::AllowDomains(vec!["api.openai.com".to_owned()]);
        assert!(net.allows_any_network());
        assert!(net.allows_domain("api.openai.com"));
        assert!(!net.allows_domain("evil.example.com"));
    }

    #[test]
    fn from_keyword_parses_and_defaults_safely() {
        let net = MpNetworkPolicy::Disabled;
        assert!(matches!(
            MpSandboxPolicy::from_keyword("DANGER_FULL_ACCESS", net.clone()),
            MpSandboxPolicy::DangerFullAccess
        ));
        assert!(matches!(
            MpSandboxPolicy::from_keyword("workspace-write", net.clone()),
            MpSandboxPolicy::WorkspaceWrite { .. }
        ));
        assert!(matches!(
            MpSandboxPolicy::from_keyword("external", net.clone()),
            MpSandboxPolicy::External { .. }
        ));
        // Unknown → safe read-only default.
        assert!(
            MpSandboxPolicy::from_keyword("nonsense", net).is_read_only(),
            "unknown keyword must fall back to read-only"
        );
    }
}
