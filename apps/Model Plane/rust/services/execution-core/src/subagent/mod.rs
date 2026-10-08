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

pub mod research_fan_out;

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

/// Maximum delegated child runs ONE top-level run may start, across all of its
/// rounds.
///
/// Deliberately a **second, independent** ceiling rather than a consequence of
/// the round budget — the same separation DeepSeek keeps between its fixed
/// loop's round budget and its `maxTotalAgents` backstop, "so the fixed loop's
/// round budget and the generic runaway-child backstop cannot disagree".
///
/// # Why the round budget does not already cover this
///
/// It bounds total *work*, not *fan-out*. One round dispatches every tool call
/// the model emitted concurrently, so a single round can open twenty
/// delegations: twenty `StartManagedRun` writes, twenty lineage rows, twenty
/// concurrent inference calls — all inside a round budget that still adds up.
/// The first delegation's `spawn` claims the whole remaining pool, so the rest
/// are refused for *lack of budget*, which is a true statement about rounds and
/// a misleading one about what just happened: the fan-out already happened at
/// the point of refusal, and the reason the model reads should be the real one.
///
/// # Why eight (as a DEFAULT, not a ceiling any more)
///
/// `MAX_DEPTH == 1`, so this is the total breadth of one run's delegation tree.
/// Eight was more than any real task had needed (the largest observed use was
/// two or three parallel lookups) and few enough that a human reading the Agent
/// Run Console could still follow the causal chain — the same reason the depth
/// limit is one. Wide-research fan-out (chat-wave2-specs, 2026-09-28) widened
/// this from a hard compile-time constant to a per-run, per-org configurable
/// value — see [`resolve_child_ceiling`] — because a genuine wide-research call
/// (dozens of parallel sub-questions) legitimately wants more than eight, and a
/// single hardcoded number cannot serve both that and the "few enough to
/// audit" default at once. Eight is kept as the unchanged DEFAULT so a run that
/// never opts in sees no behavior change.
pub const DEFAULT_MAX_CHILDREN: u32 = 8;

/// Hard server-side ceiling on [`resolve_child_ceiling`]'s output, regardless
/// of what the request or an org policy asks for. No request or org
/// configuration can ever widen a run past this — it exists so a
/// misconfigured org policy (or a future bug in it) cannot turn "configurable"
/// into "unbounded". Matches the scale a wide-research fan-out call
/// (`subagent::research_fan_out`) is actually meant to reach.
pub const MAX_CHILDREN_CEILING: u32 = 100;

/// Resolve the per-run child ceiling from what the request asked for and what
/// the run's org allows.
///
/// Mirrors [`resolve_round_budget`]'s shape: `requested` follows the same
/// "`None` or `Some(0)` means unspecified, use the default" wire convention as
/// `RunAgentRequest.max_subagents`. The org ceiling, when configured, can only
/// ever TIGHTEN the result — a request cannot widen past what the org allows,
/// the same non-negotiable-widening rule `org_quota.rs` already establishes
/// for cost ceilings in model-gateway. Both are additionally clamped by the
/// hard [`MAX_CHILDREN_CEILING`], which neither side can widen past either.
#[must_use]
pub fn resolve_child_ceiling(requested: Option<u32>, org_ceiling: Option<u32>) -> u32 {
    let org = org_ceiling.unwrap_or(MAX_CHILDREN_CEILING).min(MAX_CHILDREN_CEILING);
    let wanted = match requested {
        None | Some(0) => DEFAULT_MAX_CHILDREN,
        Some(n) => n,
    };
    wanted.min(org).min(MAX_CHILDREN_CEILING)
}

/// Refuse a delegation that would exceed `ceiling` (the run's own resolved
/// value from [`resolve_child_ceiling`] — no longer a single global constant).
///
/// `started` is how many this run has already opened. The message names the real
/// cause and what to do instead, because the model's alternative to delegating
/// is doing the work itself — and a refusal it cannot interpret becomes a retry
/// loop against the same ceiling.
///
/// # Errors
///
/// Returns the refusal text when the ceiling is already reached.
pub fn guard_child_ceiling(started: u32, ceiling: u32) -> Result<(), String> {
    if started >= ceiling {
        return Err(format!(
            "delegation refused: this run has already started {started} subagents, which is the \
             per-run limit of {ceiling}. Do the remaining work yourself, or narrow it \
             into one delegated task instead of several."
        ));
    }
    Ok(())
}

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

    /// The ceiling is about breadth, and the round budget is about work. Two
    /// separate limits on purpose: a run can exhaust either one without the
    /// other, and collapsing them would make one of the two refusals lie about
    /// its cause.
    #[test]
    fn the_child_ceiling_is_independent_of_the_round_budget() {
        guard_child_ceiling(0, DEFAULT_MAX_CHILDREN).expect("a run with no children may delegate");
        guard_child_ceiling(DEFAULT_MAX_CHILDREN - 1, DEFAULT_MAX_CHILDREN)
            .expect("the last slot is usable");
        let refusal = guard_child_ceiling(DEFAULT_MAX_CHILDREN, DEFAULT_MAX_CHILDREN)
            .expect_err("the ceiling must actually stop the next one");
        assert!(
            refusal.contains("per-run limit"),
            "the refusal must name the real cause, not borrow the budget's: {refusal}"
        );
        assert!(
            !refusal.contains("round budget"),
            "a breadth refusal that blames the round budget sends the model to \
             re-plan the wrong thing: {refusal}"
        );
        // And a run at the ceiling with plenty of budget is still refused —
        // which is exactly the case the round budget cannot express.
        assert!(resolve_round_budget(100, None).is_ok());
        assert!(guard_child_ceiling(DEFAULT_MAX_CHILDREN, DEFAULT_MAX_CHILDREN).is_err());
    }

    /// Past the ceiling as well as at it. An `==` check would let a counter that
    /// overshot (a concurrent batch incrementing past the limit) pass forever.
    #[test]
    fn the_ceiling_holds_past_its_own_value() {
        assert!(guard_child_ceiling(DEFAULT_MAX_CHILDREN + 5, DEFAULT_MAX_CHILDREN).is_err());
    }

    /// A run whose org policy or request raised the ceiling is bound by the
    /// CONFIGURED value, not by the old hardcoded eight — the whole point of
    /// making this configurable.
    #[test]
    fn guard_child_ceiling_reports_the_configured_ceiling_not_a_constant() {
        guard_child_ceiling(39, 40).expect("the 40th configured slot is usable");
        let refusal = guard_child_ceiling(40, 40).expect_err("the configured ceiling must stop it");
        assert!(
            refusal.contains("limit of 40"),
            "the refusal must interpolate the passed ceiling, not a hardcoded 8: {refusal}"
        );
    }

    #[test]
    fn resolve_child_ceiling_prefers_the_tighter_of_request_and_org_ceiling() {
        // Neither set → the unchanged default.
        assert_eq!(resolve_child_ceiling(None, None), DEFAULT_MAX_CHILDREN);
        assert_eq!(resolve_child_ceiling(Some(0), None), DEFAULT_MAX_CHILDREN);

        // Request tighter than the org ceiling → request wins.
        assert_eq!(resolve_child_ceiling(Some(5), Some(20)), 5);

        // Request wider than the org ceiling → the org wins; a request can
        // only tighten, never widen past what the org allows.
        assert_eq!(resolve_child_ceiling(Some(50), Some(20)), 20);

        // No request, but the org tightens below the default.
        assert_eq!(resolve_child_ceiling(None, Some(3)), 3);
    }

    #[test]
    fn resolve_child_ceiling_never_exceeds_the_hard_ceiling() {
        assert_eq!(
            resolve_child_ceiling(Some(u32::MAX), Some(u32::MAX)),
            MAX_CHILDREN_CEILING
        );
        assert_eq!(resolve_child_ceiling(Some(u32::MAX), None), MAX_CHILDREN_CEILING);
        assert_eq!(resolve_child_ceiling(None, Some(u32::MAX)), DEFAULT_MAX_CHILDREN);
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
