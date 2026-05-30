//! Sandbox & network isolation policy — the canonical vocabulary that
//! execution-core's runtime loop branches on when launching tool processes,
//! and that sandbox-manager consumes when provisioning isolation.
//!
//! Added per `docs/capability-ownership-matrix.md` §G3. The enum shape is
//! adapted from OpenAI Codex's `SandboxPolicy` / `PermissionProfile`
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
