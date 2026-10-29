//! Agent harness profile + its approval posture (`HARNESS_PHASE1` §1).
//!
//! The profile is decided by the operator on the agent definition (Convex) and
//! sent to the gateway on each invoke. It is the single source of truth for
//! whether the harness gates risky actions behind a human approval:
//!
//!   - `chat`           → `auto` posture: no approval gates, clean single-user
//!     experience. The harness stays invisible.
//!   - `deployed_agent` → `ask` posture: risky/destructive tool calls produce
//!     an Approval + `RUN_PAUSED_FOR_APPROVAL`, surfaced to
//!     the operator (run-event feed) until decided.
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
    /// `Wire/permission_mode` string consumed by the run loop's permission gate
    /// (`ExecuteStep.permission_mode`: "auto" | "ask" | "deny").
    #[must_use]
    pub fn as_permission_mode(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Ask => "ask",
        }
    }

    /// The stricter of two postures — `Ask` (gated) dominates `Auto` (un-gated).
    /// Used to clamp a client request so it can only ever ADD gating.
    #[must_use]
    pub fn stricter(self, other: Self) -> Self {
        match (self, other) {
            (Self::Auto, Self::Auto) => Self::Auto,
            _ => Self::Ask,
        }
    }
}

/// Server-side approval-posture floor derived from the authenticated
/// principal's (server-signed) JWT scopes.
///
/// A principal carrying any of `autonomous_scopes` is an autonomous /
/// deployed-agent identity and can NEVER run un-gated, so its floor is `Ask`
/// regardless of the client-supplied profile. Every other principal gets
/// `base_floor` (typically `Auto` so interactive chat is unaffected; ops can
/// raise it to `Ask` to fail safe fleet-wide).
#[must_use]
pub fn floor_from_scopes(
    scopes: &[String],
    autonomous_scopes: &[&str],
    base_floor: ApprovalPosture,
) -> ApprovalPosture {
    let is_autonomous = autonomous_scopes
        .iter()
        .any(|needle| scopes.iter().any(|s| s == needle));
    if is_autonomous {
        ApprovalPosture::Ask
    } else {
        base_floor
    }
}

/// Resolve the SERVER-AUTHORITATIVE approval posture for a run.
///
/// The client-supplied `profile` is treated as a REQUEST only: it may ratchet
/// the posture STRICTER (`Auto` → `Ask`) but can never weaken it below the
/// server `floor`. This keeps the approval-relevant decision off the client
/// trust boundary — a malicious or careless client cannot send
/// `profile:"chat"` to disable approval gating on a run whose server floor is
/// `Ask` — while leaving `profile`'s benign role (SSE stream shape) intact.
#[must_use]
pub fn resolve_posture(floor: ApprovalPosture, requested: AgentProfile) -> ApprovalPosture {
    floor.stricter(requested.posture())
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

    #[test]
    fn stricter_lets_ask_dominate_auto() {
        use ApprovalPosture::{Ask, Auto};
        assert_eq!(Auto.stricter(Auto), Auto);
        assert_eq!(Auto.stricter(Ask), Ask);
        assert_eq!(Ask.stricter(Auto), Ask);
        assert_eq!(Ask.stricter(Ask), Ask);
    }

    #[test]
    fn client_profile_can_ratchet_stricter_but_never_weaker() {
        use ApprovalPosture::{Ask, Auto};
        // Floor Auto: the client decides (interactive user unaffected).
        assert_eq!(resolve_posture(Auto, AgentProfile::Chat), Auto);
        // Floor Auto: client may ADD gating by asking for the deployed profile.
        assert_eq!(resolve_posture(Auto, AgentProfile::DeployedAgent), Ask);
        // Floor Ask: a client claiming `chat` CANNOT downgrade to un-gated.
        // This is the security property — the exploit in the audit is closed.
        assert_eq!(resolve_posture(Ask, AgentProfile::Chat), Ask);
        assert_eq!(resolve_posture(Ask, AgentProfile::DeployedAgent), Ask);
    }

    #[test]
    fn autonomous_scope_pins_floor_to_ask() {
        use ApprovalPosture::{Ask, Auto};
        let autonomous = &["agent:autonomous", "agent:deployed"];
        // A principal with an autonomous scope is pinned to Ask even if the
        // base floor is Auto — an autonomous identity never runs un-gated.
        let scopes = vec!["chat".to_owned(), "agent:autonomous".to_owned()];
        assert_eq!(floor_from_scopes(&scopes, autonomous, Auto), Ask);
        // A principal without an autonomous scope gets the base floor.
        let user_scopes = vec!["chat".to_owned()];
        assert_eq!(floor_from_scopes(&user_scopes, autonomous, Auto), Auto);
        assert_eq!(floor_from_scopes(&user_scopes, autonomous, Ask), Ask);
    }

    #[test]
    fn autonomous_principal_cannot_be_downgraded_end_to_end() {
        // Full chain: an autonomous principal sending `profile:"chat"` to try to
        // bypass gating still resolves to Ask.
        let autonomous = &["agent:autonomous"];
        let scopes = vec!["agent:autonomous".to_owned()];
        let floor = floor_from_scopes(&scopes, autonomous, ApprovalPosture::Auto);
        assert_eq!(
            resolve_posture(floor, AgentProfile::from_wire(Some("chat"))),
            ApprovalPosture::Ask
        );
    }
}
