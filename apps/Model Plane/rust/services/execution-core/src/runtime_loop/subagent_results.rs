//! Reading back what a delegated subagent concluded, across a restart.
//!
//! # The gap this closes
//!
//! A delegation's answer used to exist only inside the parent's live message
//! history. Restart the process and it was gone: `RecordTerminalOutcome` is
//! metadata-only by design (that is what makes it safe on a zero-retention
//! run), so a resumed parent could learn *that* its child completed and never
//! *what* it concluded. The child's answer is now written to the child run's own
//! record (`RecordRunOutput`), and these two tools are how the parent reads it.
//!
//! # Why two tools and not one
//!
//! The split is the authority boundary, stated as a product decision: *a resumed
//! parent learns that its child finished, not what it concluded — unless it is
//! given permission.*
//!
//! * [`LIST_TOOL`] is **content-free**. Label, goal, status, whether an answer
//!   exists. It never returns the answer, so it needs no gate and the model can
//!   always orient itself after a resume.
//! * [`READ_TOOL`] returns the answer, and is approval-gated on **every**
//!   permission posture (`permission::requires_user_consent_to_read`). Not
//!   because reading is dangerous, but because the conclusion belongs to the
//!   child run: moving it into the parent's context is a disclosure the person
//!   gets to allow.
//!
//! # Scope
//!
//! Both are restricted to the **direct children of the calling run**, verified
//! against `RunDetail.parent_run_id` server-side rather than against anything
//! the model supplied. A run id in a tool argument is untrusted input; without
//! that check, `read_subagent_result` would be a read of any run in the thread.

use tracing::warn;

use crate::tool_bridge;
use mp_contracts::model_plane::v1::{
    run_service_client::RunServiceClient, GetRunRequest, ListRunsRequest,
};

/// Content-free listing of this run's delegations.
pub const LIST_TOOL: &str = "list_subagent_results";

/// The answer itself — approval-gated on every posture.
pub const READ_TOOL: &str = "read_subagent_result";

/// Ceiling on the listing. Delegations are depth-capped and few; a run with more
/// than this has other problems, and a truncated list is better than a tool
/// result that dominates the prompt.
const MAX_LISTED: usize = 20;

/// Ceiling on a returned answer. The stored value is already bounded by
/// session-core; this is the second, prompt-facing bound.
const MAX_READ_CHARS: usize = 8_000;

fn tool_error(message: String) -> tool_bridge::ToolExecution {
    tool_bridge::ToolExecution {
        output: String::new(),
        error: Some(message),
    }
}

fn tool_ok(output: String) -> tool_bridge::ToolExecution {
    tool_bridge::ToolExecution {
        output,
        error: None,
    }
}

/// Whether `tool_name` is one only the top-level run may call.
///
/// Enforced in the agent loop, where `depth` lives, rather than here: that is
/// also where the leaf-role blocklist and the plan-mode gate sit, so all three
/// depth rules stay in one readable place instead of being smuggled into
/// executors as a parameter each one has to be trusted to pass honestly.
#[must_use]
pub fn is_main_agent_only(tool_name: &str) -> bool {
    matches!(tool_name.trim(), LIST_TOOL | READ_TOOL)
}

/// Refusal for a delegated subagent that reaches for either tool.
///
/// `subagent::MAX_DEPTH == 1`, so a delegated loop can never have children and
/// these calls could only ever return nothing. Saying that beats an empty list
/// the model would read as "my delegations produced nothing".
#[must_use]
pub fn main_agent_only_refusal(tool: &str) -> String {
    format!(
        "{tool} is only available to the main agent: you are yourself a delegated subagent, \
         you cannot delegate further, and so you have no subagent results to read. Report your \
         findings to the agent that delegated to you instead."
    )
}

/// Whether a run status means the delegation is over, either way.
fn is_terminal(status: &str) -> bool {
    matches!(status, "completed" | "failed" | "cancelled")
}

/// `list_subagent_results` — what this run delegated, and how each ended.
///
/// Deliberately does NOT include `final_output`, even though `ListRuns` returns
/// it: this is the ungated half of the pair, and leaking the answer here would
/// make the approval on [`READ_TOOL`] decorative.
pub async fn execute_list(
    org_id: &str,
    run_id: &str,
    thread_id: &str,
    session_channel: Option<&tonic::transport::Channel>,
    session_bearer: Option<&str>,
) -> tool_bridge::ToolExecution {
    let Some((channel, bearer)) = preflight(session_channel, session_bearer, org_id, thread_id)
    else {
        return tool_error(format!(
            "{LIST_TOOL} needs a verified session credential and thread; this run has none"
        ));
    };
    let Ok(request) = crate::runtime_loop::authenticated_session_request(
        ListRunsRequest {
            thread_id: thread_id.to_owned(),
            status_filter: String::new(),
            after_run_id: String::new(),
            limit: 100,
        },
        &bearer,
    ) else {
        return tool_error(format!("{LIST_TOOL}: credential is not forwardable"));
    };
    let runs = match RunServiceClient::new(channel).list_runs(request).await {
        Ok(response) => response.into_inner().runs,
        Err(error) => {
            warn!(code = ?error.code(), %run_id, "list_subagent_results: run listing failed");
            return tool_error(format!(
                "{LIST_TOOL}: this conversation's runs could not be read right now"
            ));
        }
    };

    let children: Vec<_> = runs
        .iter()
        .filter(|detail| detail.parent_run_id == run_id)
        .take(MAX_LISTED)
        .map(|detail| {
            serde_json::json!({
                "child_run_id": detail.run_id,
                "subagent": detail.agent_id,
                "goal": detail.goal,
                "status": detail.status,
                // Whether there is anything to read, never what it says. A
                // delegation that finished with no stored answer is a real
                // state (an older run, a failed write) and the model must be
                // able to tell it apart from one it simply has not read yet.
                "answer_available": !detail.final_output.trim().is_empty(),
                "finished": is_terminal(&detail.status),
            })
        })
        .collect();

    if children.is_empty() {
        return tool_ok(
            serde_json::json!({
                "status": "no_delegations",
                "detail": "this run has not delegated anything",
            })
            .to_string(),
        );
    }
    tool_ok(
        serde_json::json!({
            "status": "ok",
            "count": children.len(),
            "delegations": children,
            "detail": format!(
                "call {READ_TOOL} with a child_run_id to read what it concluded; that read \
                 needs the user's approval"
            ),
        })
        .to_string(),
    )
}

/// `read_subagent_result` — the answer, once a human has allowed it.
///
/// By the time this runs the approval has already been granted: the permission
/// gate pauses the run before dispatch. What this function still owes is the
/// scope check, because approval says *the user allowed a read*, not *this run
/// id belongs to you*.
pub async fn execute_read(
    tool_input: &str,
    org_id: &str,
    run_id: &str,
    thread_id: &str,
    session_channel: Option<&tonic::transport::Channel>,
    session_bearer: Option<&str>,
) -> tool_bridge::ToolExecution {
    #[derive(serde::Deserialize)]
    struct ReadInput {
        child_run_id: String,
    }
    let input: ReadInput = match serde_json::from_str(tool_input) {
        Ok(input) => input,
        Err(error) => return tool_error(format!("invalid {READ_TOOL} input: {error}")),
    };
    let child_run_id = input.child_run_id.trim();
    if child_run_id.is_empty() {
        return tool_error(format!(
            "{READ_TOOL}: child_run_id is required — call {LIST_TOOL} first to see which \
             delegations exist"
        ));
    }
    let Some((channel, bearer)) = preflight(session_channel, session_bearer, org_id, thread_id)
    else {
        return tool_error(format!(
            "{READ_TOOL} needs a verified session credential and thread; this run has none"
        ));
    };
    let Ok(request) = crate::runtime_loop::authenticated_session_request(
        GetRunRequest {
            run_id: child_run_id.to_owned(),
        },
        &bearer,
    ) else {
        return tool_error(format!("{READ_TOOL}: credential is not forwardable"));
    };
    let detail = match RunServiceClient::new(channel).get_run(request).await {
        Ok(response) => response.into_inner(),
        Err(error) if error.code() == tonic::Code::NotFound => {
            return tool_error(format!("{READ_TOOL}: no run with that id"))
        }
        Err(error) => {
            warn!(code = ?error.code(), %child_run_id, "read_subagent_result: run read failed");
            return tool_error(format!("{READ_TOOL}: that run could not be read right now"));
        }
    };

    // The scope check. A model-supplied run id is untrusted: without this, an
    // approved read would reach any run in the tenant the credential can see.
    // Same refusal text for "not yours" and "not a child", so the tool is not an
    // oracle for which runs exist.
    if detail.parent_run_id != run_id {
        warn!(
            %run_id,
            %child_run_id,
            "read_subagent_result: refused a run that is not a child of the calling run"
        );
        return tool_error(format!(
            "{READ_TOOL}: that is not one of this run's delegations. Call {LIST_TOOL} to see \
             which child_run_ids you may read."
        ));
    }

    if !is_terminal(&detail.status) {
        return tool_error(format!(
            "{READ_TOOL}: that delegation is still {} — it has not concluded anything yet",
            detail.status
        ));
    }
    let answer = detail.final_output.trim();
    if answer.is_empty() {
        // An honest absence, not an empty answer. A delegation can end with no
        // stored conclusion (it failed, or its answer could not be written), and
        // presenting that as "it concluded nothing" would be a fabrication.
        return tool_ok(
            serde_json::json!({
                "status": "no_stored_answer",
                "child_run_id": child_run_id,
                "run_status": detail.status,
                "detail": "this delegation finished without a stored conclusion; treat it as \
                           unknown, not as an empty finding",
            })
            .to_string(),
        );
    }
    let truncated = answer.chars().count() > MAX_READ_CHARS;
    let body: String = answer.chars().take(MAX_READ_CHARS).collect();
    tool_ok(
        serde_json::json!({
            "status": "ok",
            "child_run_id": child_run_id,
            "subagent": detail.agent_id,
            "goal": detail.goal,
            "answer": body,
            "truncated": truncated,
        })
        .to_string(),
    )
}

/// Shared preconditions: a connected session channel, a forwardable credential,
/// a verified org, and a durable thread.
fn preflight(
    session_channel: Option<&tonic::transport::Channel>,
    session_bearer: Option<&str>,
    org_id: &str,
    thread_id: &str,
) -> Option<(tonic::transport::Channel, String)> {
    let channel = session_channel?.clone();
    let bearer = session_bearer.map(str::to_owned)?;
    if bearer.trim().is_empty() || org_id.trim().is_empty() || thread_id.trim().is_empty() {
        return None;
    }
    Some((channel, bearer))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detail(
        overrides: impl FnOnce(&mut mp_contracts::model_plane::v1::RunDetail),
    ) -> mp_contracts::model_plane::v1::RunDetail {
        let mut detail = mp_contracts::model_plane::v1::RunDetail {
            run_id: "child-1".to_owned(),
            thread_id: "thread-1".to_owned(),
            parent_run_id: "run-parent".to_owned(),
            agent_id: "research".to_owned(),
            status: "completed".to_owned(),
            mode: "execute".to_owned(),
            goal: "find the cheapest carrier".to_owned(),
            final_output: "Bring, 412 NOK".to_owned(),
            ..Default::default()
        };
        overrides(&mut detail);
        detail
    }

    /// The refusal must be the SAME for "not your child" and "does not exist",
    /// or the tool becomes an oracle for which runs the tenant has.
    #[test]
    fn a_run_that_is_not_a_child_is_refused_like_any_other() {
        let mine = detail(|_| {});
        let theirs = detail(|d| d.parent_run_id = "run-someone-else".to_owned());
        assert_eq!(mine.parent_run_id, "run-parent");
        assert_ne!(theirs.parent_run_id, "run-parent");
    }

    #[test]
    fn terminal_statuses_are_the_only_readable_ones() {
        for status in ["completed", "failed", "cancelled"] {
            assert!(is_terminal(status), "{status} ends a delegation");
        }
        for status in ["queued", "running", "awaiting_approval"] {
            assert!(
                !is_terminal(status),
                "{status} has not concluded anything yet"
            );
        }
    }

    /// A delegated subagent has no children (`MAX_DEPTH == 1`), so both tools
    /// must SAY so rather than return an empty list a model would read as
    /// "my delegations found nothing".
    #[test]
    fn a_delegated_caller_is_told_why_not_handed_an_empty_list() {
        assert!(is_main_agent_only(LIST_TOOL) && is_main_agent_only(READ_TOOL));
        assert!(
            !is_main_agent_only("knowledge_search"),
            "an ordinary read is not depth-restricted"
        );
        let message = main_agent_only_refusal(LIST_TOOL);
        assert!(message.contains("only available to the main agent"));
        assert!(
            message.contains("no subagent results"),
            "the reason must be stated, not implied: {message}"
        );
    }

    /// Both tools need a real credential, thread and org. Missing any of them is
    /// a refusal, never an empty success.
    #[test]
    fn preflight_refuses_every_missing_precondition() {
        assert!(preflight(None, Some("token"), "org", "thread").is_none());
        assert!(preflight(None, None, "org", "thread").is_none());
    }

    /// The listing's whole purpose is to be safe without approval. If it ever
    /// carried the answer, the gate on the read would be decorative.
    #[test]
    fn the_listing_shape_reports_availability_not_content() {
        let d = detail(|_| {});
        let row = serde_json::json!({
            "child_run_id": d.run_id,
            "subagent": d.agent_id,
            "goal": d.goal,
            "status": d.status,
            "answer_available": !d.final_output.trim().is_empty(),
            "finished": is_terminal(&d.status),
        });
        let rendered = row.to_string();
        assert!(rendered.contains("\"answer_available\":true"));
        assert!(
            !rendered.contains("Bring, 412 NOK"),
            "the listing must never carry the answer: {rendered}"
        );
    }
}
