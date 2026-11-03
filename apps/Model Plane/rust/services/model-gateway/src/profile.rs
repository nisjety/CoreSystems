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
//!
//! The agent mode a person picks (WS25-S6) resolves onto this posture, never beside it: the
//! mode's floor is the mode that states the posture already in force, so a choice can only tighten.

use std::sync::OnceLock;

use mp_contracts::agent_mode::{self, AgentMode, Resolved, UnknownAgentMode};

/// The environment switch of the agent modes (WS25-S6). Off by default: with it off no choice is
/// read, and a turn is byte-identical to one sent before the field existed. Parsed by the shared
/// strict parser, so only the literal `true` enables it.
pub const AGENT_MODES_ENV: &str = "AGENT_MODES";

/// The one strict parser of the agent modes switch.
#[must_use]
pub fn agent_modes_from(value: Option<&str>) -> bool {
    mp_contracts::loop_fold::switch_from(value)
}

/// Whether the agent modes switch is on in this process. Read once, so it cannot change under a
/// request.
#[must_use]
pub fn agent_modes_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| agent_modes_from(std::env::var(AGENT_MODES_ENV).ok().as_deref()))
}

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

/// The agent mode that states a posture already in force: `auto` for `Auto`, and `ask_first` for
/// `Ask`, whose permission gate is the posture itself. A mode resolved from this floor is never
/// looser than the posture the turn would have run under without one.
#[must_use]
pub fn mode_floor_from(posture: ApprovalPosture) -> AgentMode {
    match posture {
        ApprovalPosture::Auto => AgentMode::Auto,
        ApprovalPosture::Ask => AgentMode::AskFirst,
    }
}

/// The agent mode the person chose for this turn (WS25-S6), or `None` when none applies.
///
/// Absent with the switch off, so an unchanged request is never read. A blank id is absent, as every
/// optional id on the invoke request is. An id this build does not know is an error, which the caller
/// refuses before any run exists; it never falls back to a mode.
///
/// # Errors
///
/// [`UnknownAgentMode`] for a non-blank id that is not one of the four modes.
pub fn chosen_agent_mode(
    enabled: bool,
    chosen: Option<&str>,
) -> Result<Option<AgentMode>, UnknownAgentMode> {
    match chosen.map(str::trim).filter(|id| !id.is_empty()) {
        Some(id) if enabled => AgentMode::parse(id).map(Some),
        _ => Ok(None),
    }
}

/// What the chosen mode resolves to for a turn that would otherwise run under `requested`: the
/// strictest of that posture's mode and the choice. The posture is the floor, so a choice looser
/// than it changes nothing, and the source names the floor.
#[must_use]
pub fn resolve_turn_mode(
    chosen: Option<AgentMode>,
    requested: ApprovalPosture,
) -> Option<Resolved> {
    chosen.map(|choice| agent_mode::resolve(mode_floor_from(requested), None, Some(choice), &[]))
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

    #[test]
    fn the_mode_floor_states_the_posture_already_in_force() {
        use ApprovalPosture::{Ask, Auto};
        assert_eq!(mode_floor_from(Auto), AgentMode::Auto);
        assert_eq!(mode_floor_from(Ask), AgentMode::AskFirst);
        for posture in [Auto, Ask] {
            assert_eq!(
                mode_floor_from(posture).preset().permission_mode,
                posture.as_permission_mode(),
                "{posture:?}"
            );
        }
    }

    /// The byte-identical guarantee on the posture: with no choice, the turn resolves to nothing, and
    /// the posture it runs under is the one the platform and the client's profile already set.
    #[test]
    fn a_turn_with_no_choice_resolves_to_no_mode_and_keeps_its_posture() {
        use ApprovalPosture::{Ask, Auto};
        for requested in [Auto, Ask] {
            for absent in [None, Some(""), Some("   ")] {
                let chosen =
                    chosen_agent_mode(true, absent).expect("an absent choice is not an error");
                assert_eq!(chosen, None, "{absent:?}");
                assert_eq!(resolve_turn_mode(chosen, requested), None, "{absent:?}");
            }
        }
    }

    #[test]
    fn the_agent_mode_switch_is_strict_and_off_reads_no_choice() {
        assert!(agent_modes_from(Some("true")));
        assert!(agent_modes_from(Some("  TRUE ")));
        for off in [
            None,
            Some(""),
            Some("false"),
            Some("1"),
            Some("on"),
            Some("yes"),
            Some("truee"),
        ] {
            assert!(!agent_modes_from(off), "{off:?} must stay off");
        }
        // Off: a choice is not read at all, not even to refuse it, so the request is the one it was.
        assert_eq!(chosen_agent_mode(false, Some("read_only")), Ok(None));
        assert_eq!(chosen_agent_mode(false, Some("malformed")), Ok(None));
    }

    #[test]
    fn an_unknown_mode_is_refused_and_never_becomes_a_mode() {
        for unknown in ["malformed", "full_access", "Read_Only", "read-only"] {
            assert_eq!(
                chosen_agent_mode(true, Some(unknown)),
                Err(UnknownAgentMode),
                "{unknown:?}"
            );
        }
    }

    #[test]
    fn a_read_only_choice_holds_the_turn_to_its_plan_gate_and_only_tightens() {
        let chosen = chosen_agent_mode(true, Some("read_only")).expect("a known mode");
        let resolved = resolve_turn_mode(chosen, ApprovalPosture::Auto).expect("a choice resolves");
        assert_eq!(resolved.mode, AgentMode::ReadOnly);
        assert!(resolved.preset.plan_gate);
        assert_eq!(resolved.preset.permission_mode, "ask");
        assert_eq!(resolved.source, agent_mode::Source::User);

        // A choice looser than the posture already in force changes nothing: the floor decides.
        let looser = chosen_agent_mode(true, Some("auto")).expect("a known mode");
        let kept = resolve_turn_mode(looser, ApprovalPosture::Ask).expect("a choice resolves");
        assert_eq!(kept.mode, AgentMode::AskFirst);
        assert_eq!(kept.source, agent_mode::Source::Floor);
    }

    #[test]
    fn a_choice_never_loosens_the_permission_the_posture_set() {
        use ApprovalPosture::{Ask, Auto};
        for requested in [Auto, Ask] {
            for id in ["lockdown", "read_only", "ask_first", "auto"] {
                let chosen = chosen_agent_mode(true, Some(id)).expect("a known mode");
                let resolved = resolve_turn_mode(chosen, requested).expect("a choice resolves");
                if requested == Ask {
                    assert_eq!(
                        resolved.preset.permission_mode, "ask",
                        "{id} over an ask posture"
                    );
                }
            }
        }
    }

    #[test]
    fn a_lockdown_choice_allows_no_do_run() {
        let chosen = chosen_agent_mode(true, Some("lockdown")).expect("a known mode");
        let resolved = resolve_turn_mode(chosen, ApprovalPosture::Auto).expect("a choice resolves");
        assert_eq!(resolved.mode, AgentMode::Lockdown);
        assert!(!resolved.preset.do_runs);
    }
}
