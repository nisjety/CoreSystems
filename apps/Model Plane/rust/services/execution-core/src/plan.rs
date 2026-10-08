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

/// Refusal for a delegated subagent (`depth > 0`) that calls a `plan.*` tool.
///
/// A proposed plan is the person's confirm-before-execute gate on what THIS
/// run will do, and the newest proposal of a run is the one the person is
/// shown (last-proposed-wins). A delegated subagent inherits the parent's
/// identity, session credential and `run_id` verbatim, and it is exactly the
/// context that reads the untrusted pages and documents a delegation exists
/// for; if it could call `plan.propose` it could replace the plan the main
/// agent proposed with one of its own, on the person's approval surface. So it
/// is a leaf that investigates, like every other tool a leaf may not use to
/// change what the person is asked to approve.
#[must_use]
pub fn main_agent_only_refusal(tool: &str) -> String {
    format!(
        "{tool} is only available to the main agent: a plan is what the person confirms before \
         this run acts, and you are a delegated subagent that only investigates. Report your \
         findings to the agent that delegated to you instead; it decides what to propose."
    )
}

/// Ceilings on what a `plan.propose` call may carry. session-core enforces
/// the same numbers as the authority (`orchestration_grpc.rs`'s
/// `MAX_PLAN_*`); they are repeated here so the model gets an actionable
/// tool error ("at most 100 steps") instead of an opaque RPC failure. The
/// model is not a trusted author: it reads web pages and documents, and
/// whatever it writes here is shown to the person as "the plan" and later
/// copied into an executing run's context.
pub const MAX_STEPS: usize = 100;
/// See [`MAX_STEPS`].
pub const MAX_TITLE_CHARS: usize = 500;
/// See [`MAX_STEPS`].
pub const MAX_OPERATION_CHARS: usize = 120;
/// See [`MAX_STEPS`].
pub const MAX_SUMMARY_CHARS: usize = 4_000;

/// The refusal for a proposal outside the ceilings, or `None` when it is
/// acceptable. `steps` is `(title, operation)` per step.
#[must_use]
pub fn proposal_problem(summary: &str, steps: &[(&str, &str)]) -> Option<String> {
    if summary.trim().is_empty() {
        return Some("summary must not be empty".to_owned());
    }
    if summary.chars().count() > MAX_SUMMARY_CHARS {
        return Some(format!(
            "summary is longer than {MAX_SUMMARY_CHARS} characters; keep it to one or two sentences"
        ));
    }
    if steps.is_empty() {
        return Some("at least one step is required".to_owned());
    }
    if steps.len() > MAX_STEPS {
        return Some(format!(
            "a plan may have at most {MAX_STEPS} steps; group related work into fewer steps"
        ));
    }
    for (index, (title, operation)) in steps.iter().enumerate() {
        let number = index + 1;
        if title.trim().is_empty() {
            return Some(format!("step {number} has no title"));
        }
        if title.chars().count() > MAX_TITLE_CHARS {
            return Some(format!(
                "step {number}'s title is longer than {MAX_TITLE_CHARS} characters"
            ));
        }
        if operation.chars().count() > MAX_OPERATION_CHARS {
            return Some(format!(
                "step {number}'s operation is longer than {MAX_OPERATION_CHARS} characters; \
                 use a short slug, not a description"
            ));
        }
    }
    None
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

    #[test]
    fn the_subagent_refusal_names_the_tool_and_tells_the_model_what_to_do_instead() {
        let message = main_agent_only_refusal(PROPOSE_TOOL);
        assert!(message.starts_with("plan.propose is only available to the main agent"));
        assert!(message.contains("Report your findings"));
    }

    #[test]
    fn a_reasonable_proposal_has_no_problem() {
        assert_eq!(
            proposal_problem(
                "Send the overdue invoice",
                &[("Draft the email", ""), ("Send it", "email_send")]
            ),
            None
        );
    }

    #[test]
    fn a_proposal_outside_the_ceilings_is_refused_with_a_reason_the_model_can_act_on() {
        assert!(proposal_problem("  ", &[("Draft", "")]).is_some());
        assert!(proposal_problem("Plan", &[]).is_some());
        assert!(proposal_problem("Plan", &[("  ", "")])
            .is_some_and(|reason| reason.contains("step 1")));
        let long_title = "t".repeat(MAX_TITLE_CHARS + 1);
        assert!(proposal_problem("Plan", &[("Ok", ""), (long_title.as_str(), "")])
            .is_some_and(|reason| reason.contains("step 2")));
        let long_op = "o".repeat(MAX_OPERATION_CHARS + 1);
        assert!(proposal_problem("Plan", &[("Ok", long_op.as_str())]).is_some());
        let long_summary = "s".repeat(MAX_SUMMARY_CHARS + 1);
        assert!(proposal_problem(&long_summary, &[("Ok", "")]).is_some());
        let many: Vec<(&str, &str)> = (0..=MAX_STEPS).map(|_| ("step", "")).collect();
        assert!(proposal_problem("Plan", &many).is_some());
        let at_limit: Vec<(&str, &str)> = (0..MAX_STEPS).map(|_| ("step", "")).collect();
        assert_eq!(proposal_problem("Plan", &at_limit), None);
    }
}
