//! Guards the cross-service invariants that execution-core's own loop constants
//! claim to hold against model-gateway's.
//!
//! Three constants in `runtime_loop/agent.rs` document a relationship to a
//! sibling service rather than a value chosen on its own:
//!
//! * `DEFAULT_MAX_ROUNDS` — "deliberately at least the inline-chat budget: a
//!   deployed agent is the LONG-horizon surface, so it must never get less room
//!   to work than plain chat."
//! * `MAX_ROUNDS_CEILING` — the same relationship at the hard ceiling.
//! * `MAX_TOOL_CONTEXT_CHARS` — "matches model-gateway's
//!   `MAX_TOOL_OUTPUT_CHARS` so the same tool result is not silently richer on
//!   one surface than the other."
//!
//! Until now all three were enforced by nothing but those comments. Both sides
//! agree today, so this is a regression guard rather than a fix — it exists
//! because the numbers already drifted once. `DEFAULT_MAX_ROUNDS` was 4 with a
//! ceiling of 8, under what a self-describing MCP server needs just to discover
//! a schema before acting (Visma spends two rounds on `list_skills` +
//! `get_skill`), so a deployed agent starved on work plain chat completed.
//! Nothing errored: the agent ran out of rounds and reported a graceful
//! non-answer, which is why it went unnoticed.
//!
//! # Why this reads source text instead of importing the constants
//!
//! The two services are separate crates with separate deployment cadence and
//! neither depends on the other. Adding a build dependency purely so a test
//! could compare integers would couple their build graphs — heavier coupling
//! than the invariant being protected, and an invitation to reach further across
//! the boundary later.
//!
//! Both sets of constants are also private to their modules, which is correct.
//! Widening either to `pub` just for test visibility would export loop internals
//! as API and make the next accidental cross-service call compile. So neither
//! side is widened and both are read the same way: this asserts on the source of
//! record, symmetrically.
//!
//! The cost is that the test knows two paths. If either file moves this fails —
//! the right outcome, because half the invariant moved and somebody must
//! re-point it. Fix the path; do not delete the assertion.

use std::path::{Path, PathBuf};

/// Both relative to this crate root, which sits alongside model-gateway under
/// `services/`.
const AGENT_LOOP: &str = "src/runtime_loop/agent.rs";
const INLINE_LOOP: &str = "../model-gateway/src/tool_loop.rs";

fn read(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read {} ({error}).\n\
             This file holds one half of a documented cross-service invariant. \
             If it moved, re-point the path in this test — do not delete the \
             assertions, or the relationship goes back to being enforced by a \
             comment alone.",
            path.display()
        )
    })
}

/// Reads `const NAME: TYPE = VALUE;` out of Rust source, tolerating the digit
/// separators the codebase mixes freely (`8_000` on one side, `8000` on the
/// other) and any visibility modifier (`pub(crate) const` on one side, bare
/// `const` on the other).
///
/// Comment lines are skipped rather than matched: every one of these constants
/// is referenced by name in the other's doc comment, so a `contains` match that
/// did not skip them would happily read a value out of prose.
fn const_value(source: &str, file: &str, name: &str) -> usize {
    let needle = format!("const {name}:");
    let line = source
        .lines()
        .find(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with("//") && trimmed.contains(&needle)
        })
        .unwrap_or_else(|| {
            panic!(
                "{file} no longer defines `{name}`. It is one half of an \
                 invariant the loop constants claim to hold; find what replaced \
                 it and re-point this test rather than dropping the check."
            )
        });
    let raw = line
        .split('=')
        .nth(1)
        .and_then(|rhs| rhs.split(';').next())
        .unwrap_or_else(|| panic!("cannot parse a value out of `{line}`"));
    raw.trim()
        .replace('_', "")
        .parse()
        .unwrap_or_else(|error| panic!("`{name}` is not a plain integer ({error}): {line}"))
}

fn agent(name: &str) -> usize {
    const_value(
        &read(AGENT_LOOP),
        Path::new(AGENT_LOOP).to_str().unwrap(),
        name,
    )
}

fn inline(name: &str) -> usize {
    const_value(
        &read(INLINE_LOOP),
        Path::new(INLINE_LOOP).to_str().unwrap(),
        name,
    )
}

/// A deployed agent must never get a smaller default round budget than inline
/// chat. It is the long-horizon surface; if chat can afford N rounds to answer,
/// an agent working a durable goal cannot be given fewer.
#[test]
fn agent_default_round_budget_is_at_least_inline_chat() {
    let chat = inline("DEFAULT_MAX_TOOL_ROUNDS");
    let deployed = agent("DEFAULT_MAX_ROUNDS");
    assert!(
        deployed >= chat,
        "execution-core DEFAULT_MAX_ROUNDS ({deployed}) is below model-gateway \
         DEFAULT_MAX_TOOL_ROUNDS ({chat}). A deployed agent would get less room \
         to work than plain chat, and that fails by running out of rounds and \
         reporting a graceful non-answer rather than by erroring — so it does \
         not surface as a bug report."
    );
}

/// The same relationship at the hard ceiling, so a caller that explicitly asks
/// for more rounds is not capped lower on the agent surface than on chat.
#[test]
fn agent_round_ceiling_is_at_least_inline_chat() {
    let chat = inline("MAX_TOOL_ROUNDS_CEILING");
    let deployed = agent("MAX_ROUNDS_CEILING");
    assert!(
        deployed >= chat,
        "execution-core MAX_ROUNDS_CEILING ({deployed}) is below model-gateway \
         MAX_TOOL_ROUNDS_CEILING ({chat}), so an explicit high round request is \
         clamped harder on the long-horizon surface than on chat."
    );
}

/// One tool result must render into context identically on both surfaces.
/// Equality, not an inequality: whichever side is larger, the same tool call
/// yields more usable text on one surface, and a model that succeeded in chat
/// then silently sees a shorter result as an agent.
#[test]
fn rendered_tool_output_cap_matches_inline_chat() {
    let chat = inline("MAX_TOOL_OUTPUT_CHARS");
    let deployed = agent("MAX_TOOL_CONTEXT_CHARS");
    assert_eq!(
        deployed, chat,
        "execution-core MAX_TOOL_CONTEXT_CHARS ({deployed}) and model-gateway \
         MAX_TOOL_OUTPUT_CHARS ({chat}) disagree, so the same tool result is \
         richer on one surface than the other."
    );
}

// ---- WS03 S8-A S8-6: the park resume against session-core's park store ----------------------
//
// The executor's park resume and session-core's park store are two independently deployed
// services that must agree on two things no build dependency checks: the closed token the store
// answers a park claim with when the park is not yet due (`park_not_due`, which the executor
// turns into its own `park_not_due` refusal, so the run stays parked), and the format of a
// park's reference (`park:{run_id}:{n}`, which the store validates on a park save and the
// executor writes on the park fold and checks on the claimed one). Both read the reference
// from the one helper in mp-contracts. These pins read the sources of record, like the rest.

const SESSION_CHECKPOINT: &str = "../session-core/src/loop_checkpoint.rs";
const EXECUTOR_PARK: &str = "src/runtime_loop/park.rs";
const LOOP_FOLD: &str = "../../crates/mp-contracts/src/loop_fold.rs";

/// The lines of `source` that are production code: not line comments, and nothing from the
/// file's first `#[cfg(test)] mod tests {` on (each pinned file keeps its tests there, and a test
/// may spell a reference out to check it).
fn code_lines(source: &str) -> Vec<&str> {
    let lines: Vec<&str> = source.lines().collect();
    let end = (1..lines.len())
        .find(|&at| lines[at].trim() == "mod tests {" && lines[at - 1].trim() == "#[cfg(test)]")
        .map_or(lines.len(), |at| at - 1);
    lines[..end]
        .iter()
        .copied()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect()
}

/// session-core answers a not-yet-due park claim `failed_precondition("park_not_due")`, and the
/// executor maps exactly that code and message to its `park_not_due` refusal.
fn park_not_due_violations(session: &str, park: &str) -> Vec<String> {
    let mut violations = Vec::new();
    if !code_lines(session)
        .iter()
        .any(|line| line.contains("Status::failed_precondition(\"park_not_due\")"))
    {
        violations.push(
            "session-core no longer answers a not-yet-due park claim \
             `failed_precondition(\"park_not_due\")`: the executor would refuse it as unavailable"
                .to_owned(),
        );
    }
    if !code_lines(park).iter().any(|line| {
        line.contains("(Code::FailedPrecondition, \"park_not_due\") => ResumeRefusal::ParkNotDue")
    }) {
        violations.push(
            "the executor's classify_park_claim no longer maps session-core's \
             (FailedPrecondition, \"park_not_due\") to ResumeRefusal::ParkNotDue"
                .to_owned(),
        );
    }
    violations
}

/// session-core opens and checks a claimed fold BEFORE it commits the claim, so a fold that does
/// not authenticate (`failed_precondition("loop checkpoint did not authenticate")`) or is
/// malformed (`invalid_argument`) rolls the claim back: the row STAYS parked and the executor has
/// no seq to close at. The executor must therefore answer those two with
/// `ResumeRefusal::CheckpointUnavailable` (`resume_checkpoint_unavailable`, which the SPA reads as
/// "the run stays parked"), never `ParkNotRestorable` (`park_not_restorable`, which it reads as
/// "no longer parked").
fn park_torn_claim_violations(session: &str, park: &str) -> Vec<String> {
    let mut violations = Vec::new();
    let flat = |source: &str| {
        code_lines(source)
            .join(" ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    if !flat(session)
        .contains("Status::failed_precondition(\"loop checkpoint did not authenticate\")")
    {
        violations.push(
            "session-core no longer answers a claimed fold that does not authenticate \
             `failed_precondition(\"loop checkpoint did not authenticate\")`"
                .to_owned(),
        );
    }
    if !flat(park).contains(
        "(Code::FailedPrecondition, \"loop checkpoint did not authenticate\") \
         | (Code::InvalidArgument, _) => ResumeRefusal::CheckpointUnavailable,",
    ) {
        violations.push(
            "the executor's classify_park_claim no longer maps a torn fold (did not authenticate, \
             invalid argument) to ResumeRefusal::CheckpointUnavailable: the row stays parked, so \
             it must not be park_not_restorable"
                .to_owned(),
        );
    }
    violations
}

/// A park's reference is `park:{run_id}:{n}`, built by the one mp-contracts helper; session-core
/// validates a park save against it, and the executor writes and checks it through it. Neither
/// service builds the string itself.
fn park_ref_violations(session: &str, park: &str, loop_fold: &str) -> Vec<String> {
    let mut violations = Vec::new();
    let helper = code_lines(loop_fold);
    let at = helper
        .iter()
        .position(|line| line.contains("pub fn park_ref(run_id: &str, n: u32) -> String {"));
    match at {
        Some(at)
            if helper
                .get(at + 1)
                .is_some_and(|line| line.trim() == "format!(\"park:{run_id}:{n}\")") => {}
        _ => violations.push(
            "mp-contracts park_ref is no longer `format!(\"park:{run_id}:{n}\")`".to_owned(),
        ),
    }
    let session_code = code_lines(session);
    if !session_code
        .iter()
        .any(|line| line.contains("input.pause_ref != park_ref(run_id, parks)"))
    {
        violations.push(
            "session-core no longer validates a park save's reference against park_ref(run_id, parks)"
                .to_owned(),
        );
    }
    for (name, code) in [("session-core", &session_code), ("the executor", &code_lines(park))] {
        if code
            .iter()
            .any(|line| line.contains("fn park_ref(") || line.contains("\"park:"))
        {
            violations.push(format!(
                "{name} builds a park reference itself instead of through mp-contracts park_ref"
            ));
        }
    }
    let park_code = code_lines(park);
    for need in ["lf::park_ref(site.run_id, n)", "lf::park_ref(run_id, parks)"] {
        if !park_code.iter().any(|line| line.contains(need)) {
            violations.push(format!(
                "the executor no longer writes or checks a park's reference with `{need}`"
            ));
        }
    }
    violations
}

#[test]
fn the_park_resume_and_the_park_store_agree_on_park_not_due() {
    let violations = park_not_due_violations(&read(SESSION_CHECKPOINT), &read(EXECUTOR_PARK));
    assert!(violations.is_empty(), "{violations:#?}");
}

#[test]
fn a_torn_park_claim_stays_parked_and_is_refused_as_unavailable() {
    let violations = park_torn_claim_violations(&read(SESSION_CHECKPOINT), &read(EXECUTOR_PARK));
    assert!(violations.is_empty(), "{violations:#?}");
}

#[test]
fn the_park_resume_and_the_park_store_agree_on_the_park_reference() {
    let violations = park_ref_violations(
        &read(SESSION_CHECKPOINT),
        &read(EXECUTOR_PARK),
        &read(LOOP_FOLD),
    );
    assert!(violations.is_empty(), "{violations:#?}");
}

/// `text` with the first `from` replaced by `to`; the target must exist.
fn mutate(text: &str, from: &str, to: &str) -> String {
    assert!(text.contains(from), "the mutation target `{from}` is gone: re-point this test");
    text.replacen(from, to, 1)
}

/// Each pin goes red when either side drifts.
#[test]
fn the_park_pins_go_red_when_either_side_drifts() {
    let session = read(SESSION_CHECKPOINT);
    let park = read(EXECUTOR_PARK);
    let loop_fold = read(LOOP_FOLD);
    let flagged = |violations: Vec<String>, containing: &str| {
        assert!(
            violations.iter().any(|violation| violation.contains(containing)),
            "expected a violation containing {containing:?}, got {violations:?}"
        );
    };
    flagged(
        park_not_due_violations(
            &mutate(
                &session,
                "Status::failed_precondition(\"park_not_due\")",
                "Status::failed_precondition(\"park_not_yet_due\")",
            ),
            &park,
        ),
        "session-core no longer answers a not-yet-due park claim",
    );
    flagged(
        park_not_due_violations(
            &session,
            &mutate(
                &park,
                "(Code::FailedPrecondition, \"park_not_due\") => ResumeRefusal::ParkNotDue",
                "(Code::Aborted, \"park_not_due\") => ResumeRefusal::ParkNotDue",
            ),
        ),
        "classify_park_claim no longer maps",
    );
    flagged(
        park_torn_claim_violations(
            &mutate(
                &session,
                "Status::failed_precondition(\"loop checkpoint did not authenticate\")",
                "Status::failed_precondition(\"loop checkpoint is unreadable\")",
            ),
            &park,
        ),
        "session-core no longer answers a claimed fold that does not authenticate",
    );
    flagged(
        park_torn_claim_violations(
            &session,
            &mutate(
                &park,
                "| (Code::InvalidArgument, _) => ResumeRefusal::CheckpointUnavailable,",
                "| (Code::InvalidArgument, _) => ResumeRefusal::ParkNotRestorable,",
            ),
        ),
        "classify_park_claim no longer maps a torn fold",
    );
    flagged(
        park_ref_violations(
            &session,
            &park,
            &mutate(&loop_fold, "format!(\"park:{run_id}:{n}\")", "format!(\"park/{run_id}/{n}\")"),
        ),
        "mp-contracts park_ref is no longer",
    );
    flagged(
        park_ref_violations(
            &mutate(
                &session,
                "input.pause_ref != park_ref(run_id, parks)",
                "input.pause_ref != format!(\"park:{run_id}:{parks}\")",
            ),
            &park,
            &loop_fold,
        ),
        "session-core no longer validates",
    );
    flagged(
        park_ref_violations(
            &session,
            &mutate(&park, "lf::park_ref(run_id, parks)", "format!(\"park:{run_id}:{parks}\")"),
            &loop_fold,
        ),
        "the executor builds a park reference itself",
    );
}
