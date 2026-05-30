//! Agent harness profile + its approval posture (HARNESS_PHASE1 §1).
//!
//! The profile is decided by the operator on the agent definition (Convex) and
//! sent to the gateway on each invoke. It is the single source of truth for
//! whether the harness gates risky actions behind a human approval:
//!
//!   - `chat`           → `auto` posture: no approval gates, clean single-user
//!                        experience. The harness stays invisible.
//!   - `deployed_agent` → `ask` posture: risky/destructive tool calls produce
//!                        an Approval + `RUN_PAUSED_FOR_APPROVAL`, surfaced to
//!                        the operator (run-event feed) until decided.
//!
//! Enforcement of the `ask` posture lives in the run loop (orchestrator-core +
//! the Approval primitives in mp-orchestration / session-core). This module is
//! the policy mapping the gateway stamps onto the invoke so that loop and the
//! operator surfaces agree on the posture.

/// Resolved harness profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentProfile {
    Chat,
    DeployedAgent,
}

/// Approval posture derived from the profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalPosture {
    /// No gating — proceed automatically.
    Auto,
    /// Gate risky/destructive actions behind a human approval.
    Ask,
}

impl AgentProfile {
    /// Parse the wire value. Unknown/empty defaults to `Chat` — the safe,
    /// non-gating, clean-surface default so a missing field never blocks a run.
    #[must_use]
    pub fn from_wire(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some("deployed_agent") => Self::DeployedAgent,
            _ => Self::Chat,
        }
    }

    /// Stable wire string.
    #[must_use]
    pub fn as_wire(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::DeployedAgent => "deployed_agent",
        }
    }

    /// The approval posture this profile implies.
    #[must_use]
    pub fn posture(self) -> ApprovalPosture {
        match self {
            Self::Chat => ApprovalPosture::Auto,
            Self::DeployedAgent => ApprovalPosture::Ask,
        }
    }
}

impl ApprovalPosture {
    /// Wire/permission_mode string consumed by the run loop's permission gate
    /// (`ExecuteStep.permission_mode`: "auto" | "ask" | "deny").
    #[must_use]
    pub fn as_permission_mode(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Ask => "ask",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_is_auto_posture() {
        let p = AgentProfile::from_wire(Some("chat"));
        assert_eq!(p, AgentProfile::Chat);
        assert_eq!(p.posture(), ApprovalPosture::Auto);
        assert_eq!(p.posture().as_permission_mode(), "auto");
    }

    #[test]
    fn deployed_agent_is_ask_posture() {
        let p = AgentProfile::from_wire(Some("deployed_agent"));
        assert_eq!(p, AgentProfile::DeployedAgent);
        assert_eq!(p.posture(), ApprovalPosture::Ask);
        assert_eq!(p.posture().as_permission_mode(), "ask");
    }

    #[test]
    fn unknown_and_missing_default_to_chat() {
        assert_eq!(AgentProfile::from_wire(None), AgentProfile::Chat);
        assert_eq!(AgentProfile::from_wire(Some("")), AgentProfile::Chat);
        assert_eq!(
            AgentProfile::from_wire(Some("nonsense")),
            AgentProfile::Chat
        );
    }

    #[test]
    fn wire_round_trips() {
        for p in [AgentProfile::Chat, AgentProfile::DeployedAgent] {
            assert_eq!(AgentProfile::from_wire(Some(p.as_wire())), p);
        }
    }
}
