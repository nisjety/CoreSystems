//! The agent mode seam as a turn sees it (build plan WS25-S6).
//!
//! The mode is chosen on the invoke request, resolved onto the posture the platform already sets, and
//! refused typed when it is not one this build knows. These tests drive the public resolver the stream
//! and the unary endpoint both call. The request-level half (that a plan gate reaches the
//! `RunAgentRequest` as `plan_mode` and `READ_ONLY`) is pinned in the unit tests of `sse.rs`, because the
//! one function that builds the request is private to the crate.

use std::path::Path;

use model_gateway::profile::{
    agent_modes_from, chosen_agent_mode, mode_floor_from, resolve_turn_mode, ApprovalPosture,
};
use mp_contracts::agent_mode::{AgentMode, Source, UNKNOWN_MODE_CODE};
use mp_contracts::model_plane::v1::AutonomyRung;

const POSTURES: [ApprovalPosture; 2] = [ApprovalPosture::Auto, ApprovalPosture::Ask];
const IDS: [&str; 4] = ["lockdown", "read_only", "ask_first", "auto"];

fn chosen(id: &str) -> Option<AgentMode> {
    chosen_agent_mode(true, Some(id)).expect("a known mode")
}

/// No choice, a blank choice, or the switch off: nothing resolves and the posture is what it was.
#[test]
fn agent_mode_no_choice_resolves_to_nothing_on_either_posture() {
    for posture in POSTURES {
        for absent in [None, Some(""), Some("  ")] {
            let chosen = chosen_agent_mode(true, absent).expect("an absent choice is not an error");
            assert_eq!(resolve_turn_mode(chosen, posture), None, "{absent:?}");
        }
    }
}

#[test]
fn agent_mode_switch_off_reads_no_choice_even_one_it_would_refuse() {
    assert!(!agent_modes_from(None));
    assert!(!agent_modes_from(Some("1")));
    assert_eq!(chosen_agent_mode(false, Some("read_only")), Ok(None));
    assert_eq!(chosen_agent_mode(false, Some("malformed")), Ok(None));
}

/// An unknown id is refused, and the refusal names the code the gateway returns.
#[test]
fn agent_mode_unknown_choice_is_refused_and_has_its_typed_code() {
    for unknown in [
        "malformed",
        "full_access",
        "Auto",
        "read-only",
        "danger_full_access",
    ] {
        assert!(
            chosen_agent_mode(true, Some(unknown)).is_err(),
            "{unknown:?}"
        );
    }
    assert_eq!(UNKNOWN_MODE_CODE, "agent_mode_unknown");
}

/// The plan gate a read-only choice sets is the one the request reads as `plan_mode`, and its rung is
/// READ_ONLY, on either posture the platform could set.
#[test]
fn agent_mode_read_only_choice_is_a_plan_gate_with_a_read_only_rung() {
    for posture in POSTURES {
        let resolved = resolve_turn_mode(chosen("read_only"), posture).expect("a choice resolves");
        assert_eq!(resolved.mode, AgentMode::ReadOnly);
        assert!(resolved.preset.plan_gate, "{posture:?}");
        assert_eq!(resolved.preset.rung, AutonomyRung::ReadOnly, "{posture:?}");
        assert_eq!(resolved.preset.permission_mode, "ask", "{posture:?}");
    }
}

#[test]
fn agent_mode_lockdown_choice_allows_no_do_run_on_either_posture() {
    for posture in POSTURES {
        let resolved = resolve_turn_mode(chosen("lockdown"), posture).expect("a choice resolves");
        assert!(!resolved.preset.do_runs, "{posture:?}");
    }
}

/// A choice can only tighten. Whatever id is chosen, a turn on the `ask` posture stays on `ask`, and a
/// turn on `auto` is never made stricter than the choice it names.
#[test]
fn agent_mode_a_choice_never_loosens_the_posture_the_platform_set() {
    for posture in POSTURES {
        for id in IDS {
            let resolved = resolve_turn_mode(chosen(id), posture).expect("a choice resolves");
            assert!(
                resolved.mode.strictness() >= mode_floor_from(posture).strictness(),
                "{id} over {posture:?}"
            );
            if posture == ApprovalPosture::Ask {
                assert_eq!(
                    resolved.preset.permission_mode, "ask",
                    "{id} over an ask posture"
                );
            }
        }
    }
}

/// A choice looser than the posture changes nothing, and the source says the floor decided.
#[test]
fn agent_mode_a_looser_choice_is_ignored_and_the_floor_is_named() {
    let resolved =
        resolve_turn_mode(chosen("auto"), ApprovalPosture::Ask).expect("a choice resolves");
    assert_eq!(resolved.mode, AgentMode::AskFirst);
    assert_eq!(resolved.source, Source::Floor);
}

/// The one ordering guard that cannot be a unit test: the chosen mode resolves before the request is
/// normalised, so the admission rules judge its plan gate exactly as they judge a plan flag the client
/// sent, and the lockdown refusal comes before any run exists.
#[test]
fn agent_mode_resolves_before_admission_in_the_stream_path() {
    let source = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/sse.rs"))
        .expect("sse.rs is readable");
    let entry = source
        .find("pub async fn invoke_stream_sse(")
        .expect("the stream entry is in sse.rs");
    let stream = &source[entry..];
    // Since WS25-S9 the stream path resolves through `org_agent_policy::resolve_turn`, strictest-wins over the
    // floor, the org cap and the choice. The agent's autonomy cap is one more ceiling, and the stream calls
    // `resolve_turn_with_agent_cap` (which resolves through that function) in its place; the ordering is the same.
    let resolve = stream
        .find("resolve_turn_with_agent_cap(")
        .expect("the stream path resolves a chosen mode");
    let admission = stream
        .find("crate::normalize::normalize_stream(")
        .expect("the stream path normalises the request");
    assert!(
        resolve < admission,
        "the mode must resolve before normalisation judges the plan flag it sets"
    );
    let refusal = stream
        .find("mp_contracts::agent_mode::LOCKDOWN_CODE")
        .expect("a lockdown mode refuses an agentic turn, typed");
    let dispatch = stream
        .find("fn spawn_run_dispatch(")
        .expect("the dispatch is in sse.rs, after the stream entry");
    assert!(
        refusal < dispatch,
        "the lockdown refusal is in the stream entry, before any run exists"
    );
}
