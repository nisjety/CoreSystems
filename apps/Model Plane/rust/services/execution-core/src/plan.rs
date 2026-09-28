//! `plan.*` control tools — currently just `plan.propose`, the model's way of
//! emitting its plan-mode plan as DATA instead of unstructured markdown
//! (chat-wave2-specs plan-gate §2.1).
//!
//! Mirrors [`crate::subagent`]'s `TOOL_PREFIX`/`is_subagent_tool` shape so a
//! future second `plan.*` tool has a namespace to land in, and so every place
//! that already special-cases "is this a control tool, not an action" by
//! prefix (dispatch, classification, tests enumerating the offered catalog)
//! can extend to this family the same way it already does to `subagent.*`.
//!
//! Unlike delegation, `plan.propose` needs no dispatch trait: it does not run
//! a nested loop, it makes one gRPC call to session-core's `ProposePlan`
//! (`crate::runtime_loop::execute_propose_plan`). It is read-only from the
//! run's own permission-ladder point of view — it does not act, so it is
//! never classified by `permission::is_risky_call`/`is_restricted_context_write`
//! (matching `list_subagent_results`'s treatment). It still needs a REAL
//! binding in `capability_policy::trusted_capability_id` — `None` there means
//! "no governed capability exists for this tool", which
//! `GrpcCapabilityPolicy` (the only production implementation) treats as an
//! outright refusal, not a bypass. `plan.propose` reuses `save_memory`'s
//! `cap.memory.index` (same shape: a low-risk, org/thread-scoped, ZDR-gated
//! durable write through session-core) rather than mint an unseeded id that
//! would evaluate against nothing and fail closed the same way.

/// Namespace plan-mode control tools live under (`plan.<verb>`).
pub const TOOL_PREFIX: &str = "plan.";

/// The one tool in this family today. A model calls this, never a bare
/// prefix, so [`is_plan_tool`] additionally requires something after the dot
/// — matching `subagent::is_subagent_tool`'s exact shape.
pub const PROPOSE_TOOL: &str = "plan.propose";

/// Whether `tool_name` addresses a plan-mode control tool.
#[must_use]
pub fn is_plan_tool(tool_name: &str) -> bool {
    tool_name.starts_with(TOOL_PREFIX) && tool_name.len() > TOOL_PREFIX.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_plan_tool_matches_the_propose_tool_and_rejects_the_bare_prefix() {
        assert!(is_plan_tool("plan.propose"));
        assert!(is_plan_tool("plan.x"));
        assert!(!is_plan_tool("plan."));
        assert!(!is_plan_tool("plan"));
        assert!(!is_plan_tool("knowledge_search"));
    }

    #[test]
    fn propose_tool_constant_is_itself_a_plan_tool() {
        assert!(is_plan_tool(PROPOSE_TOOL));
    }
}
