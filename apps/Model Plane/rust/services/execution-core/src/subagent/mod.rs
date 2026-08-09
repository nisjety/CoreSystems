//! Delegated subagents — a `subagent.*` tool call runs a REAL nested agent loop.
//!
//! This module owns the parts of delegation that must hold regardless of who
//! drives the nested loop: what a `subagent.*` tool name means, how its task
//! input is parsed, the recursion bound, and the round budget it may inherit.
//! The loop itself is [`crate::runtime_loop::agent`] re-entered with a fresh
//! message history — a subagent is the SAME driver, not a second implementation.
//!
//! This previously exposed `maybe_spawn`, which returned a `"spawned <tool>"`
//! summary that `runtime_loop` appended to the tool output. Nothing was ever
//! spawned, so a model that asked for delegated work was handed a fabricated
//! success it then reasoned from. Every path here either runs a real loop or
//! returns an explicit error.

use crate::runtime_loop::agent::{DEFAULT_MAX_ROUNDS, MAX_ROUNDS_CEILING};

/// Namespace all delegated-agent tools live under (`subagent.<task>`).
pub const TOOL_PREFIX: &str = "subagent.";

/// Maximum nesting depth: depth 0 is the user-facing run, depth 1 a delegated
/// subagent — which may NOT delegate further.
///
/// One level keeps a long sub-task out of the parent's context (the reason to
/// delegate at all) while bounding fan-out to something a human reviewing the
/// run in the Agent Run Console can still follow. Deeper nesting multiplies
/// spend and makes the causal chain of a write essentially unauditable.
pub const MAX_DEPTH: u32 = 1;

/// Dispatch capability for a delegated (nested) agent loop.
///
/// Only a caller that already holds the agent driver's machinery — inference
/// channel, session channel, tool allowlist, round budget — can implement this,
/// so `runtime_loop::execute_step` cannot fabricate a subagent result the way
/// the old stub did. The single-step `ExecuteStep` RPC has no loop to delegate
/// into and therefore supplies no dispatcher, which fails closed.
#[tonic::async_trait]
pub trait SubagentDispatch: Send + Sync {
    /// Run `tool_name`'s delegated task to completion and return its final
    /// answer as the tool output, or an explicit error the parent can act on.
    async fn spawn(&self, tool_name: &str, tool_input: &str) -> Result<String, String>;
}

/// Whether `tool_name` addresses a delegated subagent. The bare prefix is NOT a
/// subagent tool, matching `capability_policy::trusted_capability_id` exactly so
/// the governed capability binding and this dispatch can never disagree.
#[must_use]
pub fn is_subagent_tool(tool_name: &str) -> bool {
    tool_name.starts_with(TOOL_PREFIX) && tool_name.len() > TOOL_PREFIX.len()
}

/// The task label of a subagent tool (`subagent.research` → `research`), used in
/// log lines and in the error text the parent model reads.
#[must_use]
pub fn label(tool_name: &str) -> &str {
    tool_name.strip_prefix(TOOL_PREFIX).unwrap_or(tool_name)
}

/// Whether a loop at `depth` may delegate at all.
#[must_use]
pub fn may_spawn_at(depth: u32) -> bool {
    depth < MAX_DEPTH
}

/// Recursion guard. Fails closed with text aimed at the model that asked, so it
/// can finish the work itself instead of retrying a refused delegation.
///
/// # Errors
///
/// Returns the refusal when `parent_depth` is already at [`MAX_DEPTH`].
pub fn guard_depth(parent_depth: u32) -> Result<(), String> {
    if may_spawn_at(parent_depth) {
        return Ok(());
    }
    Err(format!(
        "delegation refused: a subagent may not spawn another subagent (nesting depth limit is \
         {MAX_DEPTH}). Complete this part of the task yourself with the tools you already have."
    ))
}

/// A parsed delegated task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentTask {
    /// The delegated goal. This is the ONLY thing seeded into the nested loop's
    /// fresh message history — the parent transcript is deliberately not shared.
    pub goal: String,
    /// Round budget the caller asked for, still clamped by
    /// [`resolve_round_budget`].
    pub max_rounds: Option<u32>,
}

/// Parse a `subagent.*` tool input.
///
/// `task` is accepted as an alias for `goal` because models reach for both; the
/// contract stays strict on what is produced (a non-empty goal).
///
/// # Errors
///
/// Returns an explicit, model-readable error when the input is not JSON or
/// carries no goal.
pub fn parse_task(tool_name: &str, tool_input: &str) -> Result<SubagentTask, String> {
    #[derive(serde::Deserialize)]
    struct RawTask {
        #[serde(default)]
        goal: Option<String>,
        #[serde(default)]
        task: Option<String>,
        #[serde(default)]
        max_rounds: Option<u32>,
    }

    let raw: RawTask = serde_json::from_str(tool_input).map_err(|error| {
        format!(
            "invalid {tool_name} input: {error}; expected {{\"goal\": \"<the delegated task>\"}}"
        )
    })?;
    let goal = raw.goal.or(raw.task).unwrap_or_default();
    let goal = goal.trim();
    if goal.is_empty() {
        return Err(format!(
            "{tool_name} requires a non-empty \"goal\" describing the task to delegate"
        ));
    }
    Ok(SubagentTask {
        goal: goal.to_owned(),
        max_rounds: raw.max_rounds,
    })
}

/// Round budget for a delegated loop: the requested budget (or the loop default)
/// clamped by the hard ceiling AND by whatever the parent has left.
///
/// The parent's remaining budget is the binding cap because nested rounds are
/// charged back to the same run — otherwise a parent with 12 rounds that
/// delegates on every round would drive 12 × 12 inference calls.
///
/// # Errors
///
/// Returns a refusal when the parent has no budget left to lend.
pub fn resolve_round_budget(
    parent_rounds_remaining: u32,
    requested: Option<u32>,
) -> Result<u32, String> {
    if parent_rounds_remaining == 0 {
        return Err(
            "delegation refused: this run has no round budget left to lend a subagent".to_owned(),
        );
    }
    let wanted = match requested {
        // An explicit 0 means "unspecified" on the wire (proto3 default), same
        // convention as `RunAgentRequest.max_rounds`.
        None | Some(0) => DEFAULT_MAX_ROUNDS,
        Some(rounds) => rounds,
    };
    Ok(wanted.min(MAX_ROUNDS_CEILING).min(parent_rounds_remaining))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subagent_tool_names_require_a_task_suffix() {
        assert!(is_subagent_tool("subagent.research"));
        assert!(is_subagent_tool("subagent.x"));
        assert!(!is_subagent_tool("subagent."));
        assert!(!is_subagent_tool("subagent"));
        assert!(!is_subagent_tool("knowledge_search"));
        assert_eq!(label("subagent.research"), "research");
    }

    #[test]
    fn recursion_guard_allows_one_level_and_then_fails_closed() {
        assert!(may_spawn_at(0));
        assert!(!may_spawn_at(MAX_DEPTH));
        guard_depth(0).expect("the user-facing run may delegate");

        let refusal = guard_depth(MAX_DEPTH).expect_err("a subagent may not delegate further");
        assert!(
            refusal.contains("may not spawn another subagent"),
            "the refusal must say why: {refusal}"
        );
        assert!(
            refusal.contains("yourself"),
            "the refusal must tell the model what to do instead: {refusal}"
        );
        // A guard that panicked (or allowed) at an impossible depth would turn a
        // runaway into an outage rather than a refusal.
        assert!(guard_depth(99).is_err());
    }

    #[test]
    fn task_parsing_accepts_goal_or_task_and_rejects_an_empty_delegation() {
        assert_eq!(
            parse_task("subagent.research", r#"{"goal":"  summarise Q3  "}"#)
                .expect("goal form parses"),
            SubagentTask {
                goal: "summarise Q3".to_owned(),
                max_rounds: None
            }
        );
        assert_eq!(
            parse_task(
                "subagent.research",
                r#"{"task":"summarise Q3","max_rounds":3}"#
            )
            .expect("task alias parses"),
            SubagentTask {
                goal: "summarise Q3".to_owned(),
                max_rounds: Some(3)
            }
        );

        let no_goal = parse_task("subagent.research", r#"{"max_rounds":3}"#)
            .expect_err("a delegation with no goal is not runnable");
        assert!(no_goal.contains("goal"), "{no_goal}");
        assert!(parse_task("subagent.research", "not-json").is_err());
        assert!(parse_task("subagent.research", r#"{"goal":"   "}"#).is_err());
    }

    #[test]
    fn round_budget_is_bounded_by_the_parents_remaining_rounds() {
        // Unspecified → the loop's own default, still capped by the parent.
        assert_eq!(
            resolve_round_budget(100, None).expect("budget"),
            DEFAULT_MAX_ROUNDS
        );
        assert_eq!(resolve_round_budget(2, None).expect("budget"), 2);
        assert_eq!(resolve_round_budget(2, Some(0)).expect("budget"), 2);

        // A request is honored only up to the ceiling and the parent's remainder.
        assert_eq!(resolve_round_budget(100, Some(3)).expect("budget"), 3);
        assert_eq!(
            resolve_round_budget(u32::MAX, Some(u32::MAX)).expect("budget"),
            MAX_ROUNDS_CEILING
        );
        assert_eq!(resolve_round_budget(5, Some(u32::MAX)).expect("budget"), 5);

        let exhausted =
            resolve_round_budget(0, Some(4)).expect_err("a parent with no budget cannot lend any");
        assert!(exhausted.contains("no round budget left"), "{exhausted}");
    }
}
