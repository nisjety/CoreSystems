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
