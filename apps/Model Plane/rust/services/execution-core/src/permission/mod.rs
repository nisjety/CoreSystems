//! Permission policy evaluation for tool execution.

use mp_contracts::model_plane::v1::AutonomyRung;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionMode {
    Auto,
    Ask,
    Deny,
}

impl PermissionMode {
    #[must_use]
    pub fn from_wire(mode: &str) -> Self {
        match mode {
            "ask" => Self::Ask,
            "deny" => Self::Deny,
            _ => Self::Auto,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    Allow,
    AwaitApproval,
    Deny,
}

#[must_use]
pub fn evaluate(mode: PermissionMode, tool_name: &str) -> PermissionDecision {
    // Name-only evaluation. Kept for callers without the tool arguments; it
    // delegates to the call-aware path with empty input, so operation-nuanced
    // tools (e.g. `execute_provider_action`) fail safe (treated as risky) when
    // their arguments are unavailable.
    evaluate_call(mode, tool_name, "")
}

/// Operation-aware permission evaluation for a specific tool CALL. Identical to
/// [`evaluate`] but inspects `tool_input` so a tool whose risk depends on its
/// arguments is classified on the actual operation, not the tool name alone.
#[must_use]
pub fn evaluate_call(
    mode: PermissionMode,
    tool_name: &str,
    tool_input: &str,
) -> PermissionDecision {
    // Owner-plane effects are never auto-approved. A server-resolved catalog
    // view can make this tool visible, but it cannot downgrade the durable
    // human-approval requirement selected by the owner plane. `Deny` remains
    // the stricter caller posture; otherwise the runtime must pause before an
    // adapter can make any Control/owner-plane request.
    if requires_durable_owner_approval(tool_name) || requires_consent_to_disclose(tool_name) {
        return if mode == PermissionMode::Deny {
            PermissionDecision::Deny
        } else {
            PermissionDecision::AwaitApproval
        };
    }
    match mode {
        PermissionMode::Deny => PermissionDecision::Deny,
        // `ask` posture (deployed_agent profile) gates risky/destructive tools
        // behind a human approval; benign reads proceed so the bot isn't
        // pausing on every lookup.
        PermissionMode::Ask if is_risky_call(tool_name, tool_input) => {
            PermissionDecision::AwaitApproval
        }
        _ => PermissionDecision::Allow,
    }
}

/// True only for owner-plane actions that must obtain a durable, bound human
/// approval before their adapter may execute. Trim intentionally closes the
/// whitespace variant at every ingress; aliases still have no capability or
/// executor binding and fail closed separately.
#[must_use]
pub fn requires_durable_owner_approval(tool_name: &str) -> bool {
    tool_name.trim() == crate::ticket_tools::TOOL_NAME
}

/// The autonomy rung a specific call requires.
///
/// The per-call half of the ladder. Deliberately derived from the SAME
/// classifiers the posture gates already use, rather than a second table:
///
/// * a destructive or outbound action ([`is_risky_call`]) reaches outside the
///   run's own workspace, so it needs the widest rung;
/// * a durable write confined to the run's own context
///   ([`is_restricted_context_write`]) needs the middle rung;
/// * everything else is a read.
///
/// Two tables would let a call be "risky" to one gate and "read-only" to the
/// other, which is the kind of disagreement that only shows up as a bypass.
///
/// Called at execution with the actual arguments — never consulted when building
/// a tool schema. A schema is registry-global while this answer is per call: the
/// same `execute_provider_action` is a read on one call and a write on the next,
/// and only the arguments say which.
#[must_use]
pub fn rung_required_for(tool_name: &str, tool_input: &str) -> AutonomyRung {
    if is_risky_call(tool_name, tool_input) {
        return AutonomyRung::DangerFullAccess;
    }
    if is_restricted_context_write(tool_name) {
        return AutonomyRung::WorkspaceWrite;
    }
    AutonomyRung::ReadOnly
}

/// Whether a run holding `granted` may make this call, and the refusal if not.
///
/// `AUTONOMY_RUNG_UNSPECIFIED` means **no graded constraint was stated**, not
/// "read-only": a caller that predates the ladder must keep behaving exactly as
/// it did. That is why this returns `Ok` for an unstated rung rather than
/// deferring to [`mp_contracts::autonomy::permits`], whose `Unspecified` is
/// correctly the narrowest — the two answer different questions, and conflating
/// them would either break every existing run or turn an unset field into a
/// grant.
///
/// # Errors
///
/// Returns the sentence the model reads, naming the rung it has, the rung the
/// call needs, and that widening requires a person.
pub fn check_autonomy_rung(
    granted: AutonomyRung,
    tool_name: &str,
    tool_input: &str,
) -> Result<(), String> {
    if granted == AutonomyRung::Unspecified {
        return Ok(());
    }
    let needed = rung_required_for(tool_name, tool_input);
    if mp_contracts::autonomy::permits(granted, needed) {
        return Ok(());
    }
    Err(format!(
        "tool '{tool_name}' needs the '{}' autonomy rung and this run was granted '{}'. \
         Widening it requires a person to approve a plan that says why — describe what you \
         would do and why the current rung cannot do it, rather than retrying.",
        mp_contracts::autonomy::label(needed),
        mp_contracts::autonomy::label(granted),
    ))
}

/// Reads whose approval is about **disclosure**, not danger — gated on every
/// posture, including `auto`.
///
/// `read_subagent_result` returns what a delegated subagent concluded. Nothing
/// about the read is destructive, so [`is_risky_tool`] correctly does not match
/// it — and that is exactly why it needs its own predicate: under the `auto`
/// posture that ordinary chat runs use, a risk-based gate would auto-allow it,
/// and the answer would flow into the parent's context with no one asked.
///
/// The product rule this encodes: a resumed parent learns *that* its child
/// finished; it learns *what* it concluded only when the person allows it. The
/// content-free half of the pair (`list_subagent_results`) is deliberately NOT
/// here — the whole point of splitting them is that orienting after a restart
/// needs no permission.
#[must_use]
pub fn requires_consent_to_disclose(tool_name: &str) -> bool {
    tool_name.trim() == crate::runtime_loop::subagent_results::READ_TOOL
}

/// Tool name for the provider-action bridge (`integration_tools`). Its risk is
/// operation-dependent, so it is classified by argument, not by name.
const EXECUTE_PROVIDER_ACTION_TOOL: &str = "execute_provider_action";

/// Writes that restricted contexts (plan mode, delegated subagents) must
/// refuse even though they are NOT approval-gated on a normal run.
///
/// `save_memory` is the case in point: a durable memory write is low-risk
/// enough that gating every save behind a human approval would kill the
/// feature (it is the user's own org-scoped memory), so it is deliberately
/// absent from [`is_risky_tool`]'s keyword list. But it is still a durable
/// side effect — plan mode promises "investigate, never act", and
/// hermes-agent's `DELEGATE_BLOCKED_TOOLS` (the pattern our leaf/orchestrator
/// split adopted, MIT) explicitly blocks memory-write for child agents: a
/// delegated context should report findings, not quietly rewrite what the
/// parent's user will be remembered as having said.
#[must_use]
pub fn is_restricted_context_write(tool_name: &str) -> bool {
    tool_name.trim() == "save_memory"
}

/// Operation-aware risk classification for a specific tool call. Extends
/// [`is_risky_tool`] (name-based) with argument inspection for tools whose risk
/// depends on their input.
///
/// `execute_provider_action` is classified by the `operation` embedded in the
/// input JSON, read-vs-write via the frozen actions-surface catalog
/// (`integration_tools::operation_is_write`): a READ proceeds under `ask`; a
/// WRITE — or a missing/unknown/uncatalogued operation (fail safe) — is risky.
/// Every other tool falls back to the name-based [`is_risky_tool`].
#[must_use]
pub fn is_risky_call(tool_name: &str, tool_input: &str) -> bool {
    if tool_name == EXECUTE_PROVIDER_ACTION_TOOL {
        return provider_action_is_risky(tool_input);
    }
    is_risky_tool(tool_name)
}

/// Classify an `execute_provider_action` call by its `operation`. A read is not
/// risky; a write, an unknown operation, or malformed input is risky (fail safe
/// — integration-corev2 leaves unmapped operations ungated, so exec-core must
/// gate anything it cannot positively classify as a read).
fn provider_action_is_risky(tool_input: &str) -> bool {
    let operation = serde_json::from_str::<serde_json::Value>(tool_input)
        .ok()
        .and_then(|v| {
            v.get("operation")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        });
    match operation {
        Some(op) => crate::integration_tools::operation_is_write(&op).unwrap_or(true),
        None => true,
    }
}

/// Heuristic for tools with side effects worth a human gate. Matches on
/// destructive/outbound verbs in the tool name.
///
/// External MCP tools (`mcp__<server>__<tool>`) are gated unconditionally: the
/// server is third-party and the remote tool's side effects are unverifiable
/// from the name alone, so under the `ask` (`deployed_agent`) posture a human
/// approves each external call by default. Under `auto` (chat) posture nothing
/// is gated, so this does not change the default chat experience.
#[must_use]
pub fn is_risky_tool(tool_name: &str) -> bool {
    const RISKY: &[&str] = &[
        "delete",
        "remove",
        "drop",
        "write",
        "update",
        "patch",
        "create",
        "send",
        "email",
        "deploy",
        "payment",
        "refund",
        "charge",
        "exec",
        "shell",
        "post",
        "purge",
        "revoke",
        // book_shipment places a real freight order (money + a truck arriving)
        // — explicitly gated since no generic keyword above catches it.
        "transfer",
        "book_shipment",
        // execute_provider_action runs an arbitrary provider write (publish a
        // Page/Instagram post, send a WhatsApp/Messenger message, create an ad
        // campaign …). The "exec" keyword above already matches it, but list
        // it explicitly so the gate is intent-visible and survives a rename.
        // NOTE: per-operation nuance (read vs write) is applied by
        // `is_risky_call`/`provider_action_is_risky`, which run BEFORE this
        // name match; this entry is the fail-safe when arguments are absent.
        "execute_provider_action",
        // browser_agent drives a live agentic browser loop (navigate + click +
        // type + form submit) with real side effects on external sites — at
        // least as risky as any other write-capable tool. No generic keyword
        // above matches it, so gate it explicitly; under `ask` it pauses for a
        // human, under `auto` (chat) it runs as before.
        "browser_agent",
        // The Model-Plane MCP facade is intentionally split: observation is
        // read-only and stays ungated, while an opaque snapshot-ref action can
        // still submit a form, disclose an approved artifact, or accept a
        // dialog. Keep the effectful half behind the normal `ask` posture.
        "browser.act",
        // publish_social_post creates a REAL workspace post and requests its
        // publish to connected platforms. The "post" keyword above already
        // matches it, but list it explicitly so the gate is intent-visible
        // and survives a rename (same rationale as execute_provider_action).
        // Its read-side sibling list_social_accounts matches no keyword and
        // stays ungated by design.
        "publish_social_post",
    ];
    if tool_name.starts_with("mcp__") {
        return true;
    }
    let lowered = tool_name.to_ascii_lowercase();
    RISKY.iter().any(|kw| lowered.contains(kw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deny_mode_always_denies() {
        assert_eq!(
            evaluate(PermissionMode::Deny, "read_doc"),
            PermissionDecision::Deny
        );
    }

    #[test]
    fn restricted_context_writes_are_exactly_the_memory_write() {
        // save_memory must NOT be approval-gated on a normal run (that would
        // kill the feature) but MUST be refused in plan mode and delegated
        // subagents — the two-classifier split this function exists for.
        assert!(is_restricted_context_write("save_memory"));
        assert!(is_restricted_context_write(" save_memory "));
        assert!(
            !is_risky_tool("save_memory"),
            "must not need human approval"
        );
        assert!(
            !is_restricted_context_write("recall_memory"),
            "reads are fine anywhere"
        );
        assert!(!is_restricted_context_write("knowledge_search"));
    }

    #[test]
    fn ask_gates_risky_allows_benign() {
        assert_eq!(
            evaluate(PermissionMode::Ask, "delete_account"),
            PermissionDecision::AwaitApproval
        );
        assert_eq!(
            evaluate(PermissionMode::Ask, "send_email"),
            PermissionDecision::AwaitApproval
        );
        // Benign read proceeds even under ask.
        assert_eq!(
            evaluate(PermissionMode::Ask, "search_knowledge"),
            PermissionDecision::Allow
        );
    }

    #[test]
    fn auto_allows_everything() {
        assert_eq!(
            evaluate(PermissionMode::Auto, "delete_account"),
            PermissionDecision::Allow
        );
    }

    #[test]
    fn reserved_owner_action_never_auto_executes() {
        for mode in [
            PermissionMode::Auto,
            PermissionMode::from_wire(""),
            PermissionMode::from_wire("malformed"),
        ] {
            assert_eq!(
                evaluate_call(mode, crate::ticket_tools::TOOL_NAME, r#"{}"#),
                PermissionDecision::AwaitApproval,
                "a caller-selected permission mode must not bypass owner approval"
            );
        }
        assert_eq!(
            evaluate_call(
                PermissionMode::Deny,
                crate::ticket_tools::TOOL_NAME,
                r#"{}"#
            ),
            PermissionDecision::Deny,
            "a caller's explicit deny posture remains more restrictive"
        );
    }

    #[test]
    fn provider_action_write_is_gated_discovery_is_not() {
        // execute_provider_action runs arbitrary provider writes → gated.
        assert!(is_risky_tool("execute_provider_action"));
        assert_eq!(
            evaluate(PermissionMode::Ask, "execute_provider_action"),
            PermissionDecision::AwaitApproval
        );
        // list_provider_actions is read-only discovery → not gated.
        assert!(!is_risky_tool("list_provider_actions"));
        assert_eq!(
            evaluate(PermissionMode::Ask, "list_provider_actions"),
            PermissionDecision::Allow
        );
    }

    #[test]
    fn external_mcp_tools_are_gated_under_ask() {
        // Even a read-ish remote name (no risky verb) is gated under `ask`,
        // because the external server's behavior is unverifiable.
        assert!(is_risky_tool("mcp__fakemcp__echo"));
        assert_eq!(
            evaluate(PermissionMode::Ask, "mcp__fakemcp__echo"),
            PermissionDecision::AwaitApproval
        );
        // ...but not gated under the default `auto` (chat) posture.
        assert_eq!(
            evaluate(PermissionMode::Auto, "mcp__fakemcp__echo"),
            PermissionDecision::Allow
        );
    }

    #[test]
    fn social_publish_is_gated_and_account_listing_is_not() {
        // The write side pauses for a human under `ask`…
        assert!(is_risky_tool("publish_social_post"));
        assert_eq!(
            evaluate(PermissionMode::Ask, "publish_social_post"),
            PermissionDecision::AwaitApproval
        );
        // …while read-only account discovery proceeds without a gate.
        assert!(!is_risky_tool("list_social_accounts"));
        assert_eq!(
            evaluate(PermissionMode::Ask, "list_social_accounts"),
            PermissionDecision::Allow
        );
    }

    #[test]
    fn code_interpreter_runs_ungated_while_shell_stays_gated() {
        // `code_interpreter` is hermetic (read-only rootfs outside its own
        // per-call workspace, no network, wall-clock timeout, scrubbed output,
        // workspace deleted afterwards), so it must NOT pause for a human under
        // `ask` — and its name deliberately contains no RISKY substring, which
        // this asserts so a future rename cannot silently gate or ungate it.
        assert!(!is_risky_tool("code_interpreter"));
        assert_eq!(
            evaluate(PermissionMode::Ask, "code_interpreter"),
            PermissionDecision::Allow
        );
        // Arbitrary host commands keep the human gate.
        assert!(is_risky_tool("shell"));
        assert_eq!(
            evaluate(PermissionMode::Ask, "shell"),
            PermissionDecision::AwaitApproval
        );
    }

    #[test]
    fn browser_agent_is_gated_under_ask() {
        // browser_agent drives real page side effects → treated as write-risky.
        assert!(is_risky_tool("browser_agent"));
        assert_eq!(
            evaluate(PermissionMode::Ask, "browser_agent"),
            PermissionDecision::AwaitApproval
        );
        // ...but not under the default `auto` (chat) posture.
        assert_eq!(
            evaluate(PermissionMode::Auto, "browser_agent"),
            PermissionDecision::Allow
        );
    }

    #[test]
    fn provider_action_gating_is_operation_aware() {
        let read = r#"{"connection_id":"c1","operation":"pages.list"}"#;
        let write = r#"{"connection_id":"c1","operation":"whatsapp.messages.send"}"#;
        let unknown = r#"{"connection_id":"c1","operation":"totally.unknown"}"#;

        // A READ operation proceeds under `ask` — no needless approval pause.
        assert!(!is_risky_call(EXECUTE_PROVIDER_ACTION_TOOL, read));
        assert_eq!(
            evaluate_call(PermissionMode::Ask, EXECUTE_PROVIDER_ACTION_TOOL, read),
            PermissionDecision::Allow
        );

        // A WRITE operation is gated under `ask`.
        assert!(is_risky_call(EXECUTE_PROVIDER_ACTION_TOOL, write));
        assert_eq!(
            evaluate_call(PermissionMode::Ask, EXECUTE_PROVIDER_ACTION_TOOL, write),
            PermissionDecision::AwaitApproval
        );

        // Unknown / uncatalogued operation → gated (fail safe).
        assert_eq!(
            evaluate_call(PermissionMode::Ask, EXECUTE_PROVIDER_ACTION_TOOL, unknown),
            PermissionDecision::AwaitApproval
        );
        // Malformed input → gated (fail safe).
        assert_eq!(
            evaluate_call(
                PermissionMode::Ask,
                EXECUTE_PROVIDER_ACTION_TOOL,
                "not json"
            ),
            PermissionDecision::AwaitApproval
        );

        // Under `auto` nothing gates, read or write.
        assert_eq!(
            evaluate_call(PermissionMode::Auto, EXECUTE_PROVIDER_ACTION_TOOL, write),
            PermissionDecision::Allow
        );
    }

    // --- Disclosure consent: read_subagent_result -------------------------
    //
    // The gate that is NOT about danger. These tests exist because the obvious
    // implementation — add the tool to `is_risky_tool` — silently does nothing
    // on the posture that matters most.

    /// THE property. Ordinary chat runs use `auto`, where a risk-based gate
    /// never fires. If this tool were gated by risk, reading a subagent's
    /// conclusion into the parent's context would happen with nobody asked —
    /// which is precisely the decision this encodes: the parent learns *that*
    /// its child finished, and *what* it concluded only with permission.
    #[test]
    fn reading_a_subagent_result_needs_approval_on_every_posture() {
        for mode in [PermissionMode::Auto, PermissionMode::Ask] {
            assert_eq!(
                evaluate(mode, crate::runtime_loop::subagent_results::READ_TOOL),
                PermissionDecision::AwaitApproval,
                "{mode:?} must still ask before disclosing a subagent's conclusion"
            );
        }
        // `deny` stays the stricter posture — consent cannot upgrade a refusal.
        assert_eq!(
            evaluate(
                PermissionMode::Deny,
                crate::runtime_loop::subagent_results::READ_TOOL
            ),
            PermissionDecision::Deny
        );
    }

    /// And the reason a risk-based gate would not have worked: the tool is
    /// genuinely not risky. If it ever starts matching `is_risky_tool`, the
    /// consent predicate would be doing nothing on `ask` and this test says so.
    #[test]
    fn the_consent_gate_is_not_a_risk_gate_in_disguise() {
        assert!(
            !is_risky_tool(crate::runtime_loop::subagent_results::READ_TOOL),
            "a read is not destructive; the gate is about disclosure, and a risk \
             classification here would make `auto` auto-allow it"
        );
        assert!(requires_consent_to_disclose(
            crate::runtime_loop::subagent_results::READ_TOOL
        ));
    }

    /// The content-free half must stay ungated, or a resumed run cannot even
    /// orient itself without interrupting the user — which would push the model
    /// toward guessing at what it delegated instead of looking.
    #[test]
    fn listing_delegations_never_asks_for_permission() {
        let list = crate::runtime_loop::subagent_results::LIST_TOOL;
        assert!(!requires_consent_to_disclose(list));
        assert!(!is_risky_tool(list));
        for mode in [PermissionMode::Auto, PermissionMode::Ask] {
            assert_eq!(
                evaluate(mode, list),
                PermissionDecision::Allow,
                "{mode:?} must let a run see WHICH delegations it has"
            );
        }
    }

    /// The label the person actually reads. Calling a disclosure "destructive"
    /// is how an approval prompt stops carrying information — and every pause
    /// used to be labelled that way, including this one.
    #[test]
    fn a_disclosure_is_not_reported_as_a_destructive_operation() {
        use crate::runtime_loop::agent::{approval_kind_for, approval_reason_for};
        use mp_contracts::model_plane::v1::ApprovalKind;

        let read = crate::runtime_loop::subagent_results::READ_TOOL;
        assert_eq!(approval_kind_for(read), ApprovalKind::Permission);
        assert_eq!(
            approval_kind_for("book_shipment"),
            ApprovalKind::Destructive,
            "a real side effect keeps the label it earned"
        );

        let reason = approval_reason_for(read);
        assert!(
            reason.contains("into this conversation"),
            "the reason must state what would be disclosed and where it would go, \
             not just name the mechanism: {reason}"
        );
        assert!(
            !reason.contains("requires approval"),
            "the generic mechanism sentence tells the person nothing about the \
             choice they are making: {reason}"
        );
    }

    // --- The graded autonomy ladder, checked per call -----------------------

    /// The rung a call needs comes from the SAME classifiers the posture gates
    /// use. A second table would let one call be "risky" here and "read-only"
    /// there, and that disagreement is a bypass, not an inconsistency.
    #[test]
    fn the_required_rung_follows_the_existing_risk_classification() {
        assert_eq!(
            rung_required_for("book_shipment", "{}"),
            AutonomyRung::DangerFullAccess,
            "an outbound action reaches outside the workspace"
        );
        assert_eq!(
            rung_required_for("save_memory", "{}"),
            AutonomyRung::WorkspaceWrite,
            "a durable write confined to the run's own context is the middle rung"
        );
        assert_eq!(
            rung_required_for("knowledge_search", "{}"),
            AutonomyRung::ReadOnly
        );
    }

    /// Per call, with the arguments — not per tool name. `execute_provider_action`
    /// is a read on one call and a write on the next, and only the arguments say
    /// which. This is why the check cannot live in a tool schema.
    #[test]
    fn the_same_tool_needs_different_rungs_on_different_calls() {
        // Real catalogued operations — `operation_is_write` fails safe for an
        // unknown one, so an invented name would have made this test pass for
        // the wrong reason.
        let read = r#"{"operation":"pages.list"}"#;
        let write = r#"{"operation":"pages.post"}"#;
        assert_eq!(
            rung_required_for(EXECUTE_PROVIDER_ACTION_TOOL, read),
            AutonomyRung::ReadOnly,
            "a catalogued read does not need full access"
        );
        assert_eq!(
            rung_required_for(EXECUTE_PROVIDER_ACTION_TOOL, write),
            AutonomyRung::DangerFullAccess
        );
        // Which makes the same tool permitted and refused under one rung.
        assert!(check_autonomy_rung(
            AutonomyRung::WorkspaceWrite,
            EXECUTE_PROVIDER_ACTION_TOOL,
            read
        )
        .is_ok());
        assert!(check_autonomy_rung(
            AutonomyRung::WorkspaceWrite,
            EXECUTE_PROVIDER_ACTION_TOOL,
            write
        )
        .is_err());
    }

    /// The state plan mode cannot express, and the reason the ladder exists: a
    /// run granted `workspace_write` by an approved plan may write its report
    /// and still not send, publish, book or pay.
    #[test]
    fn workspace_write_permits_a_durable_write_and_still_refuses_an_outbound_action() {
        check_autonomy_rung(AutonomyRung::WorkspaceWrite, "save_memory", "{}")
            .expect("the granted rung covers a workspace write");
        let refusal = check_autonomy_rung(AutonomyRung::WorkspaceWrite, "book_shipment", "{}")
            .expect_err("an outbound action is above this rung");
        assert!(
            refusal.contains("danger_full_access") && refusal.contains("workspace_write"),
            "the refusal must name BOTH rungs, or the model cannot tell how far short it is: {refusal}"
        );
        assert!(
            refusal.contains("requires a person"),
            "the model must be told that widening is not something it can do: {refusal}"
        );
    }

    /// Non-regression, and the reason `UNSPECIFIED` is not treated as read-only
    /// here even though `mp_contracts::autonomy::permits` correctly does: a
    /// caller that predates the ladder must behave exactly as it did, and
    /// reading its unset field as the narrowest rung would refuse every write on
    /// every existing run.
    #[test]
    fn a_run_with_no_stated_rung_is_unconstrained_by_the_ladder() {
        for tool in ["book_shipment", "save_memory", "knowledge_search"] {
            check_autonomy_rung(AutonomyRung::Unspecified, tool, "{}")
                .unwrap_or_else(|error| panic!("{tool} must be unaffected: {error}"));
        }
        // While the shared contract still treats an unset rung as no authority,
        // which is the right answer to the DIFFERENT question of what a grant
        // covers.
        assert!(!mp_contracts::autonomy::permits(
            AutonomyRung::Unspecified,
            AutonomyRung::WorkspaceWrite
        ));
    }

    /// `read_only` is what a plan-mode run is held to, so it must refuse exactly
    /// what plan mode refuses — stated in the ladder's vocabulary instead of as
    /// a boolean, so the two cannot drift.
    #[test]
    fn read_only_refuses_everything_plan_mode_refuses() {
        for tool in [
            "book_shipment",
            "save_memory",
            "publish_social_post",
            "shell",
        ] {
            assert!(
                check_autonomy_rung(AutonomyRung::ReadOnly, tool, "{}").is_err(),
                "{tool} must be refused at read_only, as plan mode refuses it"
            );
            assert!(
                is_risky_call(tool, "{}") || is_restricted_context_write(tool),
                "{tool} is refused by plan mode too — the two gates must agree on the set"
            );
        }
        check_autonomy_rung(AutonomyRung::ReadOnly, "knowledge_search", "{}")
            .expect("reads still run at read_only, exactly as under plan mode");
    }
}
