//! The agent modes a person or an organisation can pick for a run, and the one rule that
//! combines them (build plan WS25-S6, DEC-15).
//!
//! A mode is a named preset of what a run may do: the permission posture its gates run
//! under, whether a plan gate holds it read-only, the autonomy rung it states, and whether
//! it may start as a Do run at all. The modes are ordered by strictness, and a run takes
//! the strictest of the platform floor, the organisation's cap and the person's choice, so
//! a choice can only tighten what the platform already allows. An id this build does not
//! know is refused. It never falls back to a mode, because the fallback is the mode a typo
//! would get.
//!
//! `api/contracts/agent-modes.json` pins the ids and the presets, so the composer and the
//! gateway read one vocabulary. The words people read are the Frontend's copy, not this
//! module's.

use crate::model_plane::v1::AutonomyRung;

/// The typed refusal for a mode id this build does not know.
pub const UNKNOWN_MODE_CODE: &str = "agent_mode_unknown";

/// The typed refusal for a Do run asked for under a mode that allows none.
pub const LOCKDOWN_CODE: &str = "mode_lockdown";

/// One of the four agent modes. Stricter modes have a higher [`AgentMode::strictness`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentMode {
    /// No Do runs. Plain answers only.
    Lockdown,
    /// The run is held to plan mode, so a write tool is refused and the autonomy rung is
    /// read-only.
    ReadOnly,
    /// The permission gates ask before a risky call.
    AskFirst,
    /// No gate beyond what the platform floor already requires.
    Auto,
}

impl AgentMode {
    /// Every mode, strictest first.
    pub const ALL: [Self; 4] = [Self::Lockdown, Self::ReadOnly, Self::AskFirst, Self::Auto];

    /// The wire id.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lockdown => "lockdown",
            Self::ReadOnly => "read_only",
            Self::AskFirst => "ask_first",
            Self::Auto => "auto",
        }
    }

    /// Parses a wire id exactly.
    ///
    /// # Errors
    ///
    /// [`UnknownAgentMode`] for any other text, including a differently cased or padded id.
    pub fn parse(id: &str) -> Result<Self, UnknownAgentMode> {
        Self::ALL
            .into_iter()
            .find(|mode| mode.as_str() == id)
            .ok_or(UnknownAgentMode)
    }

    /// How strict the mode is: a higher number is stricter, and [`resolve`] takes the maximum.
    #[must_use]
    pub fn strictness(self) -> u8 {
        match self {
            Self::Lockdown => 3,
            Self::ReadOnly => 2,
            Self::AskFirst => 1,
            Self::Auto => 0,
        }
    }

    /// What the mode does to a run.
    #[must_use]
    pub fn preset(self) -> Preset {
        match self {
            Self::Lockdown => Preset {
                permission_mode: "ask",
                plan_gate: false,
                rung: AutonomyRung::Unspecified,
                do_runs: false,
            },
            Self::ReadOnly => Preset {
                permission_mode: "ask",
                plan_gate: true,
                rung: AutonomyRung::ReadOnly,
                do_runs: true,
            },
            Self::AskFirst => Preset {
                permission_mode: "ask",
                plan_gate: false,
                rung: AutonomyRung::Unspecified,
                do_runs: true,
            },
            Self::Auto => Preset {
                permission_mode: "auto",
                plan_gate: false,
                rung: AutonomyRung::Unspecified,
                do_runs: true,
            },
        }
    }
}

/// The refusal for a mode id this build does not know. It carries no text: the id is the
/// caller's, and it is never echoed back or logged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnknownAgentMode;

/// What a mode does to one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preset {
    /// The permission posture the run's gates take: `ask` or `auto`, the `ExecuteStep` wire.
    pub permission_mode: &'static str,
    /// Whether plan mode holds the run read-only. Plan mode states READ_ONLY wherever the run
    /// is dispatched, so a plan gate is the way a mode narrows the rung.
    pub plan_gate: bool,
    /// The autonomy rung the run states. `Unspecified` states no constraint of its own, and
    /// the thread's grant decides as it always has.
    pub rung: AutonomyRung,
    /// Whether the run may start as a Do run.
    pub do_runs: bool,
}

/// A limit that a named source other than the platform floor, the organisation or the person
/// puts on a run, such as a Space's. A ceiling tightens and never loosens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ceiling {
    pub source: &'static str,
    pub mode: AgentMode,
}

/// Which input a resolved mode came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Floor,
    Org,
    User,
    Ceiling(&'static str),
}

impl Source {
    /// The name a log line and an explanation use.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Floor => "floor",
            Self::Org => "org",
            Self::User => "user",
            Self::Ceiling(name) => name,
        }
    }
}

/// The mode a run takes, with its preset and the input that decided it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolved {
    pub mode: AgentMode,
    pub preset: Preset,
    pub source: Source,
}

/// The mode a run takes: the strictest of the platform floor, the organisation's cap, the
/// person's choice and every ceiling. On a tie the input that came first keeps the label, so
/// the floor is named whenever it is already as strict as anything else.
#[must_use]
pub fn resolve(
    floor: AgentMode,
    org_cap: Option<AgentMode>,
    user_choice: Option<AgentMode>,
    ceilings: &[Ceiling],
) -> Resolved {
    let candidates = org_cap
        .map(|mode| (mode, Source::Org))
        .into_iter()
        .chain(user_choice.map(|mode| (mode, Source::User)))
        .chain(
            ceilings
                .iter()
                .map(|ceiling| (ceiling.mode, Source::Ceiling(ceiling.source))),
        );
    let (mode, source) = candidates.fold((floor, Source::Floor), |strictest, candidate| {
        if candidate.0.strictness() > strictest.0.strictness() {
            candidate
        } else {
            strictest
        }
    });
    Resolved {
        mode,
        preset: mode.preset(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_mode_ids_parse_exactly_and_refuse_everything_else() {
        for mode in AgentMode::ALL {
            assert_eq!(AgentMode::parse(mode.as_str()), Ok(mode));
        }
        for unknown in [
            "",
            "full_access",
            "Auto",
            " auto",
            "read-only",
            "ask",
            "deny",
        ] {
            assert_eq!(
                AgentMode::parse(unknown),
                Err(UnknownAgentMode),
                "{unknown:?} must not fall back to a mode"
            );
        }
    }

    #[test]
    fn agent_mode_order_runs_from_lockdown_to_auto() {
        let strictness: Vec<u8> = AgentMode::ALL
            .iter()
            .map(|mode| mode.strictness())
            .collect();
        assert_eq!(strictness, [3, 2, 1, 0]);
        assert_eq!(
            AgentMode::ALL.map(AgentMode::as_str),
            ["lockdown", "read_only", "ask_first", "auto"]
        );
    }

    /// The only modes that may widen a run's permission posture are the ones that already
    /// ask nothing. A posture the platform floor set can only be tightened.
    #[test]
    fn agent_mode_permission_is_ask_except_for_auto() {
        for mode in AgentMode::ALL {
            let expected = if mode == AgentMode::Auto {
                "auto"
            } else {
                "ask"
            };
            assert_eq!(mode.preset().permission_mode, expected, "{mode:?}");
        }
    }

    #[test]
    fn agent_mode_only_the_plan_gate_states_read_only_and_only_read_only_has_one() {
        for mode in AgentMode::ALL {
            let preset = mode.preset();
            assert_eq!(
                preset.plan_gate,
                mode == AgentMode::ReadOnly,
                "{mode:?} plan gate"
            );
            if preset.plan_gate {
                assert_eq!(preset.rung, AutonomyRung::ReadOnly, "{mode:?}");
            }
        }
    }

    #[test]
    fn agent_mode_lockdown_is_the_only_mode_without_do_runs() {
        for mode in AgentMode::ALL {
            assert_eq!(
                mode.preset().do_runs,
                mode != AgentMode::Lockdown,
                "{mode:?}"
            );
        }
    }

    #[test]
    fn agent_mode_resolve_takes_the_strictest_input() {
        let read_only = resolve(AgentMode::Auto, None, Some(AgentMode::ReadOnly), &[]);
        assert_eq!(read_only.mode, AgentMode::ReadOnly);
        assert_eq!(read_only.source, Source::User);

        let org_wins = resolve(
            AgentMode::AskFirst,
            Some(AgentMode::Lockdown),
            Some(AgentMode::Auto),
            &[],
        );
        assert_eq!(org_wins.mode, AgentMode::Lockdown);
        assert_eq!(org_wins.source, Source::Org);
    }

    /// The person can only tighten. A looser choice than the floor is ignored, so it can never
    /// be used to take a gate off the platform set.
    #[test]
    fn agent_mode_user_choice_only_tightens_the_floor() {
        for floor in AgentMode::ALL {
            for choice in AgentMode::ALL {
                let resolved = resolve(floor, None, Some(choice), &[]);
                assert!(
                    resolved.mode.strictness() >= floor.strictness(),
                    "{choice:?} over {floor:?} gave {:?}",
                    resolved.mode
                );
            }
        }
        let looser = resolve(AgentMode::AskFirst, None, Some(AgentMode::Auto), &[]);
        assert_eq!(looser.mode, AgentMode::AskFirst);
        assert_eq!(
            looser.source,
            Source::Floor,
            "a choice that does not tighten is not the source"
        );
    }

    /// No choice and no cap: the mode is the floor, and so is everything it presets. This is
    /// what keeps a turn that names no mode byte-identical to one sent before the field existed.
    #[test]
    fn agent_mode_without_a_choice_resolves_to_the_floor() {
        for floor in AgentMode::ALL {
            let resolved = resolve(floor, None, None, &[]);
            assert_eq!(resolved.mode, floor);
            assert_eq!(resolved.preset, floor.preset());
            assert_eq!(resolved.source, Source::Floor);
        }
    }

    #[test]
    fn agent_mode_a_ceiling_tightens_and_names_its_source() {
        let space = Ceiling {
            source: "space",
            mode: AgentMode::ReadOnly,
        };
        let resolved = resolve(AgentMode::Auto, None, Some(AgentMode::Auto), &[space]);
        assert_eq!(resolved.mode, AgentMode::ReadOnly);
        assert_eq!(resolved.source, Source::Ceiling("space"));
        assert_eq!(resolved.source.label(), "space");
        let looser = Ceiling {
            source: "space",
            mode: AgentMode::Auto,
        };
        assert_eq!(
            resolve(AgentMode::ReadOnly, None, None, &[looser]).mode,
            AgentMode::ReadOnly
        );
    }

    #[test]
    fn agent_mode_a_tie_is_labelled_by_the_input_that_came_first() {
        let tied = resolve(AgentMode::AskFirst, Some(AgentMode::AskFirst), None, &[]);
        assert_eq!(tied.source, Source::Floor);
        let org_first = resolve(
            AgentMode::Auto,
            Some(AgentMode::ReadOnly),
            Some(AgentMode::ReadOnly),
            &[],
        );
        assert_eq!(org_first.source, Source::Org);
    }
}
