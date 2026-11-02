//! Permission policy evaluation for tool execution.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

use mp_contracts::model_plane::v1::AutonomyRung;
use mp_contracts::tool_class::{effective_floor, ApprovalFloor, SideEffectClass};

use crate::capability_policy::ToolClassGate;

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
    crate::owner_actions::requires_durable_approval(tool_name)
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
    check_autonomy_rung_with(ToolClassGate::Off, None, granted, tool_name, tool_input)
}

/// [`check_autonomy_rung`] under the tool class gate: the rung the call needs is
/// the declared class's where the gate says so, and an unstated rung is still no
/// constraint at all.
///
/// # Errors
///
/// The same refusal sentence as [`check_autonomy_rung`].
pub fn check_autonomy_rung_with(
    gate: ToolClassGate,
    view: Option<&ToolClassView>,
    granted: AutonomyRung,
    tool_name: &str,
    tool_input: &str,
) -> Result<(), String> {
    if granted == AutonomyRung::Unspecified {
        return Ok(());
    }
    let needed = rung_required_for_with(gate, view, tool_name, tool_input);
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
///
/// The two Space workspace writes (WS24-S4) are the same kind of write: an
/// overlay change that the run's own later reads see, so a restricted context
/// must not make it either. This list is transitional; WS05 S4/S6 answer it
/// from the declared class instead.
#[must_use]
pub fn is_restricted_context_write(tool_name: &str) -> bool {
    let name = tool_name.trim();
    // E3.7.10: a crawl or a batch can ingest pages into the org's knowledge base, a durable
    // write the seed declares `context_write`; plan mode and a delegated leaf must not run it.
    name == "save_memory"
        || crate::web_tools::is_durable_crawl_tool(name)
        || crate::workspace_tools::is_workspace_write(name)
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
///
/// This list and the `mcp__` blanket are the legacy classifier (WS05-S6). Under
/// `TOOL_CLASS_GATE=enforce` they answer only for a tool the registry gave the run
/// no class for, which is the strict answer. They are deleted, and the `*_with`
/// functions below lose their gate, once the `shadow` soak has shown zero built-in
/// divergence; until then they are also what `off` and `shadow` decide by.
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

// --- Declared tool classes (WS05-S4) ----------------------------------------
//
// Everything above guesses a tool's risk from its name. The registry declares it
// (`mp_contracts::tool_class`), and the functions below read the declaration
// instead. They are reached only through `TOOL_CLASS_GATE`:
//
// * `off`: the legacy answer, byte for byte, and nothing is resolved.
// * `shadow`: both are computed, a disagreement is counted
//   (`tool_class_divergence_total{point,tool}`), and the legacy answer decides.
// * `enforce`: the declared class decides wherever the run has a view of the tool;
//   a tool the registry did not answer for keeps the legacy answer, which is the
//   strict one for every name this module does not know.
//
// Nothing here loosens by itself: a class can only be as permissive as the
// registry declared it, a floor only ever adds an approval, and the one place a
// call may lower its own class is the provider-operation refinement below.

/// What the registry declared about one tool, as one run saw it.
///
/// The floor is already the effective one (raised to what the class implies). An
/// MCP tool also carries the digest of the definition the registry approved, which
/// is what the gateway re-checks before it runs the call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolClassView {
    class: SideEffectClass,
    floor: ApprovalFloor,
    /// The call may lower itself to a read when its operation is catalogued as one.
    /// The registry row says so (`call_refinement = provider_operation`), but the
    /// wire does not carry the column: the one seeded row that has it is the
    /// external write `execute_provider_action`, so that is what grants it.
    provider_operation: bool,
    digest: String,
}

impl ToolClassView {
    /// A view from a registry binding's wire values. Anything outside the
    /// vocabulary is the strictest value, never a guess at the nearest one.
    #[must_use]
    pub fn from_registry(tool_name: &str, class: &str, floor: &str, digest: &str) -> Self {
        let class = SideEffectClass::from_wire(class);
        Self {
            class,
            floor: effective_floor(class, ApprovalFloor::from_wire(floor)),
            provider_operation: tool_name == EXECUTE_PROVIDER_ACTION_TOOL
                && class == SideEffectClass::ExternalWrite,
            digest: digest.to_owned(),
        }
    }

    #[must_use]
    pub fn class(&self) -> SideEffectClass {
        self.class
    }

    #[must_use]
    pub fn floor(&self) -> ApprovalFloor {
        self.floor
    }

    /// The pinned digest of an MCP tool's approved definition; empty for a built-in.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// The class of this call. A provider action whose operation is catalogued as
    /// a read is a read; an unknown operation, malformed input and every write stay
    /// the declared write class, and no other tool may lower.
    fn class_for_call(&self, tool_input: &str) -> SideEffectClass {
        if self.provider_operation && !provider_action_is_risky(tool_input) {
            SideEffectClass::Read
        } else {
            self.class
        }
    }
}

/// `plan.propose` is the one durable write plan mode exists to make, so it is
/// declared `context_write` and must still be callable in plan mode and at the
/// `read_only` rung. This is the explicit rule that keeps it so; without it the
/// declared class alone would refuse the tool plan mode is for.
fn is_plan_proposal(tool_name: &str) -> bool {
    tool_name.trim() == crate::plan::PROPOSE_TOOL
}

/// Whether a call needs the human gate under `ask`, by declared class.
#[must_use]
pub fn is_risky_class(view: &ToolClassView, tool_input: &str) -> bool {
    view.class_for_call(tool_input).is_risky()
}

/// Whether plan mode and delegated leaves must refuse a call that is not risky,
/// by declared class: a durable write confined to the run's own context, other
/// than the plan proposal itself.
#[must_use]
pub fn is_restricted_context_write_class(view: &ToolClassView, tool_name: &str) -> bool {
    view.class.is_restricted_context_write() && !is_plan_proposal(tool_name)
}

/// The autonomy rung a call needs, by declared class and with the call's own
/// arguments (a schema is registry-global, the refinement is per call).
#[must_use]
pub fn rung_required_for_class(
    view: &ToolClassView,
    tool_name: &str,
    tool_input: &str,
) -> AutonomyRung {
    let class = view.class_for_call(tool_input);
    if class == SideEffectClass::ContextWrite && is_plan_proposal(tool_name) {
        return AutonomyRung::ReadOnly;
    }
    class.required_rung()
}

/// [`evaluate_call`] from the declared class and floor: a floor above `none` asks
/// on every posture except the stricter `deny`, and under `ask` a risky class asks.
#[must_use]
pub fn evaluate_call_with_class(
    mode: PermissionMode,
    view: &ToolClassView,
    tool_input: &str,
) -> PermissionDecision {
    if view.floor != ApprovalFloor::None {
        return if mode == PermissionMode::Deny {
            PermissionDecision::Deny
        } else {
            PermissionDecision::AwaitApproval
        };
    }
    match mode {
        PermissionMode::Deny => PermissionDecision::Deny,
        PermissionMode::Ask if is_risky_class(view, tool_input) => {
            PermissionDecision::AwaitApproval
        }
        PermissionMode::Auto | PermissionMode::Ask => PermissionDecision::Allow,
    }
}

/// One answer under the gate: the legacy one for `off`, for a tool the run has no
/// view of, and for `shadow` (which counts a disagreement); the declared one for
/// `enforce`.
fn by_gate<T: PartialEq>(
    gate: ToolClassGate,
    view: Option<&ToolClassView>,
    tool_name: &str,
    point: &'static str,
    legacy: T,
    declared: impl FnOnce(&ToolClassView) -> T,
) -> T {
    match (gate, view) {
        (ToolClassGate::Off, _) | (_, None) => legacy,
        (ToolClassGate::Shadow, Some(view)) => {
            if declared(view) != legacy {
                record_divergence(point, tool_name);
            }
            legacy
        }
        (ToolClassGate::Enforce, Some(view)) => declared(view),
    }
}

/// Counted, never silent. An MCP name is bucketed so the label set stays bounded
/// by the number of call sites and built-in tools, not by the servers an org
/// connects.
fn record_divergence(point: &'static str, tool_name: &str) {
    let tool = if tool_name.starts_with("mcp__") {
        "mcp__".to_owned()
    } else {
        tool_name.trim().to_owned()
    };
    tracing::warn!(point, tool = %tool, "tool class divergence (shadow: legacy decides)");
    metrics::counter!("tool_class_divergence_total", "point" => point, "tool" => tool).increment(1);
}

/// [`evaluate_call`] under the gate.
#[must_use]
pub fn evaluate_call_with(
    gate: ToolClassGate,
    view: Option<&ToolClassView>,
    mode: PermissionMode,
    tool_name: &str,
    tool_input: &str,
) -> PermissionDecision {
    by_gate(
        gate,
        view,
        tool_name,
        "posture",
        evaluate_call(mode, tool_name, tool_input),
        |view| evaluate_call_with_class(mode, view, tool_input),
    )
}

/// [`is_risky_call`] under the gate.
#[must_use]
pub fn is_risky_call_with(
    gate: ToolClassGate,
    view: Option<&ToolClassView>,
    tool_name: &str,
    tool_input: &str,
) -> bool {
    by_gate(
        gate,
        view,
        tool_name,
        "risky",
        is_risky_call(tool_name, tool_input),
        |view| is_risky_class(view, tool_input),
    )
}

/// [`is_restricted_context_write`] under the gate.
#[must_use]
pub fn is_restricted_context_write_with(
    gate: ToolClassGate,
    view: Option<&ToolClassView>,
    tool_name: &str,
) -> bool {
    by_gate(
        gate,
        view,
        tool_name,
        "restricted_write",
        is_restricted_context_write(tool_name),
        |view| is_restricted_context_write_class(view, tool_name),
    )
}

/// [`rung_required_for`] under the gate. The arguments still decide: the legacy
/// answer reads them through `is_risky_call`, the declared one through the
/// provider-operation refinement.
#[must_use]
pub fn rung_required_for_with(
    gate: ToolClassGate,
    view: Option<&ToolClassView>,
    tool_name: &str,
    tool_input: &str,
) -> AutonomyRung {
    by_gate(
        gate,
        view,
        tool_name,
        "rung",
        rung_required_for(tool_name, tool_input),
        |view| rung_required_for_class(view, tool_name, tool_input),
    )
}

/// [`requires_durable_owner_approval`] under the gate: the registry's own
/// never-auto marker is the `durable_owner` floor.
#[must_use]
pub fn requires_durable_owner_approval_with(
    gate: ToolClassGate,
    view: Option<&ToolClassView>,
    tool_name: &str,
) -> bool {
    by_gate(
        gate,
        view,
        tool_name,
        "owner_approval",
        requires_durable_owner_approval(tool_name),
        |view| view.floor == ApprovalFloor::DurableOwner,
    )
}

/// [`requires_consent_to_disclose`] under the gate. A declared disclosure is a read
/// class carrying the `ask` floor. An `ask` floor on any other class is a side-effect
/// approval, and an unclassified tool has that floor by implication without being a
/// read. The reserved name stays out of this closure, so shadow still counts a reserved
/// name whose row disagrees; call sites OR it in, and a view can add the marker but
/// never remove it.
#[must_use]
pub fn requires_consent_to_disclose_with(
    gate: ToolClassGate,
    view: Option<&ToolClassView>,
    tool_name: &str,
) -> bool {
    by_gate(
        gate,
        view,
        tool_name,
        "disclosure_consent",
        requires_consent_to_disclose(tool_name),
        |view| view.class == SideEffectClass::Read && view.floor == ApprovalFloor::Ask,
    )
}

/// The process's tool class gate, read once. A unit test never sees the
/// environment: the gate is off there unless the test names it.
#[must_use]
pub fn tool_class_gate() -> ToolClassGate {
    #[cfg(test)]
    {
        ToolClassGate::Off
    }
    #[cfg(not(test))]
    {
        static GATE: OnceLock<ToolClassGate> = OnceLock::new();
        *GATE.get_or_init(ToolClassGate::from_env)
    }
}

/// A run's classifier: the gate and the run whose snapshot of declared classes it
/// reads. Every call site that used a name heuristic asks this instead.
#[derive(Debug, Clone, Copy)]
pub struct RunGate<'a> {
    gate: ToolClassGate,
    org_id: &'a str,
    run_id: &'a str,
}

impl<'a> RunGate<'a> {
    #[must_use]
    pub fn for_run(org_id: &'a str, run_id: &'a str) -> Self {
        Self::with_gate(tool_class_gate(), org_id, run_id)
    }

    #[must_use]
    pub fn with_gate(gate: ToolClassGate, org_id: &'a str, run_id: &'a str) -> Self {
        Self {
            gate,
            org_id,
            run_id,
        }
    }

    fn view(&self, tool_name: &str) -> Option<ToolClassView> {
        if self.gate == ToolClassGate::Off {
            return None;
        }
        run_view(self.org_id, self.run_id, tool_name)
    }

    #[must_use]
    pub fn evaluate_call(
        &self,
        mode: PermissionMode,
        tool_name: &str,
        tool_input: &str,
    ) -> PermissionDecision {
        evaluate_call_with(
            self.gate,
            self.view(tool_name).as_ref(),
            mode,
            tool_name,
            tool_input,
        )
    }

    #[must_use]
    pub fn is_risky_call(&self, tool_name: &str, tool_input: &str) -> bool {
        is_risky_call_with(
            self.gate,
            self.view(tool_name).as_ref(),
            tool_name,
            tool_input,
        )
    }

    #[must_use]
    pub fn is_restricted_context_write(&self, tool_name: &str) -> bool {
        is_restricted_context_write_with(self.gate, self.view(tool_name).as_ref(), tool_name)
    }

    #[must_use]
    pub fn requires_durable_owner_approval(&self, tool_name: &str) -> bool {
        requires_durable_owner_approval_with(self.gate, self.view(tool_name).as_ref(), tool_name)
    }

    #[must_use]
    pub fn requires_consent_to_disclose(&self, tool_name: &str) -> bool {
        requires_consent_to_disclose_with(self.gate, self.view(tool_name).as_ref(), tool_name)
    }

    /// The autonomy rung this call needs, by the run's declared class where the gate
    /// says so. The effect ledger classifies a call through this, so it cannot call a
    /// call an external effect while the retry policy calls it a read.
    #[must_use]
    pub fn rung_required_for(&self, tool_name: &str, tool_input: &str) -> AutonomyRung {
        rung_required_for_with(
            self.gate,
            self.view(tool_name).as_ref(),
            tool_name,
            tool_input,
        )
    }

    /// # Errors
    ///
    /// The refusal the model reads, as [`check_autonomy_rung`].
    pub fn check_autonomy_rung(
        &self,
        granted: AutonomyRung,
        tool_name: &str,
        tool_input: &str,
    ) -> Result<(), String> {
        check_autonomy_rung_with(
            self.gate,
            self.view(tool_name).as_ref(),
            granted,
            tool_name,
            tool_input,
        )
    }
}

/// Most runs the snapshot tracks, and most tools it keeps per run. Both are memory
/// bounds: a run offers a few dozen tools, and the least recently used run is
/// forgotten first. A forgotten run is classified by the legacy names, which is
/// the strict answer.
const MAX_TRACKED_RUNS: usize = 1024;
/// Also what bounds an offer: the built-in views and the offered MCP views of one run
/// share it, so an offer that did not leave room for its built-in tools would lose the
/// views of its last MCP tools (`mcp_gateway::resolve_run_offer` clamps to it).
pub(crate) const MAX_VIEWS_PER_RUN: usize = 256;

#[derive(Default)]
struct RunViewStore {
    clock: u64,
    runs: HashMap<(String, String), RunViews>,
}

struct RunViews {
    touched: u64,
    views: HashMap<String, ToolClassView>,
}

impl RunViewStore {
    /// Replace one run's views, forgetting the least recently used run when the
    /// store is full.
    fn record(&mut self, key: (String, String), views: HashMap<String, ToolClassView>) {
        self.clock += 1;
        let touched = self.clock;
        if !self.runs.contains_key(&key) && self.runs.len() >= MAX_TRACKED_RUNS {
            let oldest = self
                .runs
                .iter()
                .min_by_key(|(_, run)| run.touched)
                .map(|(key, _)| key.clone());
            if let Some(oldest) = oldest {
                self.runs.remove(&oldest);
            }
        }
        self.runs.insert(key, RunViews { touched, views });
    }

    fn view(&mut self, key: &(String, String), tool_name: &str) -> Option<ToolClassView> {
        self.clock += 1;
        let touched = self.clock;
        let run = self.runs.get_mut(key)?;
        run.touched = touched;
        run.views.get(tool_name).cloned()
    }
}

fn run_view_store() -> MutexGuard<'static, RunViewStore> {
    static STORE: OnceLock<Mutex<RunViewStore>> = OnceLock::new();
    STORE
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// Replace the declared classes this run knows about. Called once per offer, after
/// the registry answered; an offer the registry could not answer leaves the
/// previous snapshot standing instead of clearing it.
pub fn record_run_views(
    org_id: &str,
    run_id: &str,
    views: impl IntoIterator<Item = (String, ToolClassView)>,
) {
    let key = (org_id.to_owned(), run_id.to_owned());
    let views: HashMap<String, ToolClassView> = views.into_iter().take(MAX_VIEWS_PER_RUN).collect();
    run_view_store().record(key, views);
}

/// The declared class of `tool_name` in this run, if the registry answered for it.
#[must_use]
pub fn run_view(org_id: &str, run_id: &str, tool_name: &str) -> Option<ToolClassView> {
    run_view_store().view(&(org_id.to_owned(), run_id.to_owned()), tool_name)
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
    fn restricted_context_writes_are_exactly_the_memory_and_workspace_writes() {
        // save_memory and the two Space workspace writes must NOT be approval-gated on a normal run
        // (that would kill the feature, and an overlay is the run's own) but MUST be refused in plan
        // mode and delegated subagents — the two-classifier split this function exists for.
        for name in [
            "save_memory",
            " save_memory ",
            "workspace_edit",
            " workspace_put ",
        ] {
            assert!(
                is_restricted_context_write(name),
                "{name:?} is a restricted write"
            );
            assert!(
                !is_risky_tool(name.trim()),
                "{name:?} must not need human approval"
            );
        }
        for name in [
            "recall_memory",
            "knowledge_search",
            "workspace_read",
            "workspace_grep",
            "workspace_glob",
            "workspace_list",
            "plan.propose",
        ] {
            assert!(
                !is_restricted_context_write(name),
                "{name} is fine in a restricted context"
            );
        }
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
    fn workspace_reads_are_allowed_under_ask_and_read_only_rung() {
        for name in [
            "workspace_glob",
            "workspace_grep",
            "workspace_list",
            "workspace_read",
        ] {
            assert_eq!(
                evaluate_call(PermissionMode::Ask, name, "{}"),
                PermissionDecision::Allow,
                "{name}"
            );
            assert_eq!(rung_required_for(name, "{}"), AutonomyRung::ReadOnly, "{name}");
            assert!(!is_risky_tool(name), "{name} trips the risky-name gate");
        }
    }

    /// The Space workspace writes need the `workspace_write` rung, as `save_memory` does: a run
    /// granted only `read_only` (plan mode) is refused, and a run granted `workspace_write` is not.
    #[test]
    fn workspace_writes_require_the_workspace_write_rung() {
        for name in ["workspace_edit", "workspace_put"] {
            assert_eq!(
                rung_required_for(name, "{}"),
                AutonomyRung::WorkspaceWrite,
                "{name}"
            );
            assert!(
                check_autonomy_rung(AutonomyRung::ReadOnly, name, "{}").is_err(),
                "{name} must be refused at read_only"
            );
            check_autonomy_rung(AutonomyRung::WorkspaceWrite, name, "{}")
                .expect("the granted rung covers a workspace write");
        }
        check_autonomy_rung(AutonomyRung::ReadOnly, "workspace_grep", "{}")
            .expect("a read runs at read_only");
    }

    /// The writes are an overlay of one run and ask no approval under either posture, as
    /// `save_memory` asks none: the rung and the plan and subagent refusals are their gates.
    #[test]
    fn workspace_edit_pauses_nothing_under_auto_and_ask() {
        for name in ["workspace_edit", "workspace_put"] {
            for mode in [PermissionMode::Auto, PermissionMode::Ask] {
                assert_eq!(
                    evaluate_call(mode, name, "{}"),
                    PermissionDecision::Allow,
                    "{name} under {mode:?}"
                );
            }
        }
    }

    #[test]
    fn read_only_refuses_everything_plan_mode_refuses() {
        for tool in [
            "book_shipment",
            "save_memory",
            "workspace_edit",
            "workspace_put",
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

    // --- The legacy name-based classification, as a golden (WS05-S0) ---------
    //
    // Each line is what the name heuristics answer today for one tool name: the
    // posture gate under `ask` and `auto`, the rung, the never-auto predicates
    // and the restricted-context check, over a read and a write argument form.
    // Only `execute_provider_action` answers differently by its arguments. This
    // is the oracle that declared side-effect classes must reproduce for every
    // built-in before the name list is deleted, so a change to a row must be a
    // deliberate change to a classifier, never a side effect of another edit.

    const READ_CALL: &str = r#"{"connection_id":"c1","operation":"pages.list"}"#;
    const WRITE_CALL: &str = r#"{"connection_id":"c1","operation":"pages.post"}"#;

    /// Today's answers, one line per name, sorted by name. The leading and
    /// trailing spaces and the upper-case `MCP__` prefix are legacy gaps: the
    /// classifiers match the raw name, so those spellings are not risky today.
    /// The class lookup has to answer them deliberately when it lands.
    const LEGACY_CLASSIFICATION_GOLDEN: &str = r" mcp__s__t                 risky=no/no restricted=no rung=read_only/read_only owner=no consent=no ask=Allow/Allow auto=Allow/Allow
 read_subagent_result      risky=no/no restricted=no rung=read_only/read_only owner=no consent=yes ask=AwaitApproval/AwaitApproval auto=AwaitApproval/AwaitApproval
 save_memory               risky=no/no restricted=yes rung=workspace_write/workspace_write owner=no consent=no ask=Allow/Allow auto=Allow/Allow
 tickets.create            risky=yes/yes restricted=no rung=danger_full_access/danger_full_access owner=yes consent=no ask=AwaitApproval/AwaitApproval auto=AwaitApproval/AwaitApproval
MCP__s__t                  risky=no/no restricted=no rung=read_only/read_only owner=no consent=no ask=Allow/Allow auto=Allow/Allow
book_shipment              risky=yes/yes restricted=no rung=danger_full_access/danger_full_access owner=no consent=no ask=AwaitApproval/AwaitApproval auto=Allow/Allow
browser.act                risky=yes/yes restricted=no rung=danger_full_access/danger_full_access owner=no consent=no ask=AwaitApproval/AwaitApproval auto=Allow/Allow
browser.observe            risky=no/no restricted=no rung=read_only/read_only owner=no consent=no ask=Allow/Allow auto=Allow/Allow
code_interpreter           risky=no/no restricted=no rung=read_only/read_only owner=no consent=no ask=Allow/Allow auto=Allow/Allow
company_lookup             risky=no/no restricted=no rung=read_only/read_only owner=no consent=no ask=Allow/Allow auto=Allow/Allow
execute_provider_action    risky=no/yes restricted=no rung=read_only/danger_full_access owner=no consent=no ask=Allow/AwaitApproval auto=Allow/Allow
get_shipping_quotes        risky=no/no restricted=no rung=read_only/read_only owner=no consent=no ask=Allow/Allow auto=Allow/Allow
knowledge_search           risky=no/no restricted=no rung=read_only/read_only owner=no consent=no ask=Allow/Allow auto=Allow/Allow
list_provider_actions      risky=no/no restricted=no rung=read_only/read_only owner=no consent=no ask=Allow/Allow auto=Allow/Allow
list_social_accounts       risky=no/no restricted=no rung=read_only/read_only owner=no consent=no ask=Allow/Allow auto=Allow/Allow
list_subagent_results      risky=no/no restricted=no rung=read_only/read_only owner=no consent=no ask=Allow/Allow auto=Allow/Allow
mcp__s__t                  risky=yes/yes restricted=no rung=danger_full_access/danger_full_access owner=no consent=no ask=AwaitApproval/AwaitApproval auto=Allow/Allow
news                       risky=no/no restricted=no rung=read_only/read_only owner=no consent=no ask=Allow/Allow auto=Allow/Allow
plan.propose               risky=no/no restricted=no rung=read_only/read_only owner=no consent=no ask=Allow/Allow auto=Allow/Allow
process_list               risky=no/no restricted=no rung=read_only/read_only owner=no consent=no ask=Allow/Allow auto=Allow/Allow
process_read               risky=no/no restricted=no rung=read_only/read_only owner=no consent=no ask=Allow/Allow auto=Allow/Allow
process_signal             risky=no/no restricted=no rung=read_only/read_only owner=no consent=no ask=Allow/Allow auto=Allow/Allow
process_start              risky=no/no restricted=no rung=read_only/read_only owner=no consent=no ask=Allow/Allow auto=Allow/Allow
process_stdin              risky=no/no restricted=no rung=read_only/read_only owner=no consent=no ask=Allow/Allow auto=Allow/Allow
publish_social_post        risky=yes/yes restricted=no rung=danger_full_access/danger_full_access owner=no consent=no ask=AwaitApproval/AwaitApproval auto=Allow/Allow
read_provider_action       risky=no/no restricted=no rung=read_only/read_only owner=no consent=no ask=Allow/Allow auto=Allow/Allow
read_subagent_result       risky=no/no restricted=no rung=read_only/read_only owner=no consent=yes ask=AwaitApproval/AwaitApproval auto=AwaitApproval/AwaitApproval
reattach_skill             risky=no/no restricted=no rung=read_only/read_only owner=no consent=no ask=Allow/Allow auto=Allow/Allow
recall_memory              risky=no/no restricted=no rung=read_only/read_only owner=no consent=no ask=Allow/Allow auto=Allow/Allow
save_memory                risky=no/no restricted=yes rung=workspace_write/workspace_write owner=no consent=no ask=Allow/Allow auto=Allow/Allow
shipping_carriers          risky=no/no restricted=no rung=read_only/read_only owner=no consent=no ask=Allow/Allow auto=Allow/Allow
subagent.research_fan_out  risky=no/no restricted=no rung=read_only/read_only owner=no consent=no ask=Allow/Allow auto=Allow/Allow
subagent.task              risky=no/no restricted=no rung=read_only/read_only owner=no consent=no ask=Allow/Allow auto=Allow/Allow
tickets.create             risky=yes/yes restricted=no rung=danger_full_access/danger_full_access owner=yes consent=no ask=AwaitApproval/AwaitApproval auto=AwaitApproval/AwaitApproval
track_shipment             risky=no/no restricted=no rung=read_only/read_only owner=no consent=no ask=Allow/Allow auto=Allow/Allow
traffic                    risky=no/no restricted=no rung=read_only/read_only owner=no consent=no ask=Allow/Allow auto=Allow/Allow
web.read                   risky=no/no restricted=no rung=read_only/read_only owner=no consent=no ask=Allow/Allow auto=Allow/Allow
web.search                 risky=no/no restricted=no rung=read_only/read_only owner=no consent=no ask=Allow/Allow auto=Allow/Allow
web_fetch                  risky=no/no restricted=no rung=read_only/read_only owner=no consent=no ask=Allow/Allow auto=Allow/Allow
web_search                 risky=no/no restricted=no rung=read_only/read_only owner=no consent=no ask=Allow/Allow auto=Allow/Allow
yr_weather                 risky=no/no restricted=no rung=read_only/read_only owner=no consent=no ask=Allow/Allow auto=Allow/Allow
";

    fn legacy_golden_line(name: &str) -> String {
        let yes_no = |answer: bool| if answer { "yes" } else { "no" };
        let rung = |args: &str| mp_contracts::autonomy::label(rung_required_for(name, args));
        let posture =
            |mode: PermissionMode, args: &str| format!("{:?}", evaluate_call(mode, name, args));
        format!(
            "{name:<26} risky={}/{} restricted={} rung={}/{} owner={} consent={} ask={}/{} auto={}/{}",
            yes_no(is_risky_call(name, READ_CALL)),
            yes_no(is_risky_call(name, WRITE_CALL)),
            yes_no(is_restricted_context_write(name)),
            rung(READ_CALL),
            rung(WRITE_CALL),
            yes_no(requires_durable_owner_approval(name)),
            yes_no(requires_consent_to_disclose(name)),
            posture(PermissionMode::Ask, READ_CALL),
            posture(PermissionMode::Ask, WRITE_CALL),
            posture(PermissionMode::Auto, READ_CALL),
            posture(PermissionMode::Auto, WRITE_CALL),
        )
    }

    /// Complete and exact: a changed classifier answer fails here, and so does a
    /// name that is offered but missing from the golden, so every new tool is
    /// classified on purpose rather than by whatever substring it contains.
    #[test]
    fn legacy_classification_golden() {
        let mut names: std::collections::BTreeSet<String> =
            crate::runtime_loop::agent::offerable_tool_names()
                .into_iter()
                .collect();
        // Named rather than offered: `tickets.create` is never offered (its
        // owner-plane gate is the point), `mcp__s__t` stands for any MCP tool,
        // and the spellings are the ingress variants the gate can be handed.
        names.extend(
            [
                "tickets.create",
                "mcp__s__t",
                "MCP__s__t",
                " mcp__s__t ",
                " tickets.create ",
                " read_subagent_result ",
                " save_memory ",
            ]
            .map(str::to_owned),
        );
        let actual: Vec<String> = names.iter().map(|name| legacy_golden_line(name)).collect();
        let golden: Vec<&str> = LEGACY_CLASSIFICATION_GOLDEN.lines().collect();
        assert_eq!(
            actual,
            golden,
            "the name-based classification changed: update the golden only in the change that moved a classifier, and name the rows that moved"
        );
        for name in &names {
            for args in [READ_CALL, WRITE_CALL] {
                assert_eq!(
                    evaluate_call(PermissionMode::Deny, name, args),
                    PermissionDecision::Deny,
                    "`{name}` must be refused under deny whatever its class"
                );
            }
        }
    }
}

/// The declared-class half of the engine (WS05-S4): what a registry view decides,
/// and how `TOOL_CLASS_GATE` lets it.
#[cfg(test)]
mod class_tests {
    use super::*;
    use metrics::{
        Counter, CounterFn, Gauge, Histogram, Key, KeyName, Metadata, Recorder, SharedString, Unit,
    };
    use std::sync::{Arc, Mutex};

    const READ_CALL: &str = r#"{"connection_id":"c1","operation":"pages.list"}"#;
    const WRITE_CALL: &str = r#"{"connection_id":"c1","operation":"pages.post"}"#;

    /// Counter increments, keyed `name{label=value,...}`.
    #[derive(Default)]
    struct Counted(Arc<Mutex<Vec<String>>>);

    struct Tap(String, Arc<Mutex<Vec<String>>>);

    impl CounterFn for Tap {
        fn increment(&self, _value: u64) {
            self.1.lock().unwrap().push(self.0.clone());
        }
        fn absolute(&self, _value: u64) {}
    }

    impl Recorder for Counted {
        fn describe_counter(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}
        fn describe_gauge(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}
        fn describe_histogram(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}
        fn register_counter(&self, key: &Key, _: &Metadata<'_>) -> Counter {
            let mut labels: Vec<String> = key
                .labels()
                .map(|label| format!("{}={}", label.key(), label.value()))
                .collect();
            labels.sort();
            Counter::from_arc(Arc::new(Tap(
                format!("{}{{{}}}", key.name(), labels.join(",")),
                Arc::clone(&self.0),
            )))
        }
        fn register_gauge(&self, _: &Key, _: &Metadata<'_>) -> Gauge {
            Gauge::noop()
        }
        fn register_histogram(&self, _: &Key, _: &Metadata<'_>) -> Histogram {
            Histogram::noop()
        }
    }

    fn view(name: &str, class: &str) -> ToolClassView {
        ToolClassView::from_registry(name, class, "none", "digest")
    }

    /// The seed rows migration 0016 gives the built-in tools, as
    /// `(tool, class, floor, refinement)`.
    fn seeded_built_ins() -> Vec<(String, String, String)> {
        let sql = include_str!(
            "../../../../../go/services/capability-core/migrations/0016_capability_tool_classes.up.sql"
        );
        let row = regex::Regex::new(
            r"\('global', '[^']+', '([^']+)', '([a-z_]+)', '([a-z_]+)', (?:NULL|'[a-z_]+'), 'seed'\)",
        )
        .expect("seed row pattern");
        let rows: Vec<(String, String, String)> = row
            .captures_iter(sql)
            .map(|c| (c[1].to_owned(), c[2].to_owned(), c[3].to_owned()))
            .collect();
        assert!(
            rows.len() >= 30,
            "the seed row format changed: {} rows",
            rows.len()
        );
        rows
    }

    /// The class-driven engine says what the name heuristics say, for every built-in
    /// tool the registry seeds and for a read and a write form of its arguments.
    /// `plan.propose` is the one declared stricter than its name, and the explicit
    /// rule that keeps it callable in plan mode makes it agree here too.
    #[test]
    fn the_declared_class_reproduces_the_legacy_answers_for_every_seeded_built_in() {
        for (tool, class, floor) in seeded_built_ins() {
            let view = ToolClassView::from_registry(&tool, &class, &floor, "");
            for args in [READ_CALL, WRITE_CALL] {
                let what = format!("`{tool}` ({class}/{floor}) with {args}");
                assert_eq!(
                    is_risky_class(&view, args),
                    is_risky_call(&tool, args),
                    "risky: {what}"
                );
                assert_eq!(
                    is_restricted_context_write_class(&view, &tool),
                    is_restricted_context_write(&tool),
                    "restricted: {what}"
                );
                assert_eq!(
                    mp_contracts::autonomy::label(rung_required_for_class(&view, &tool, args)),
                    mp_contracts::autonomy::label(rung_required_for(&tool, args)),
                    "rung: {what}"
                );
                for mode in [
                    PermissionMode::Ask,
                    PermissionMode::Auto,
                    PermissionMode::Deny,
                ] {
                    assert_eq!(
                        evaluate_call_with_class(mode, &view, args),
                        evaluate_call(mode, &tool, args),
                        "{mode:?}: {what}"
                    );
                }
            }
        }
    }

    #[test]
    fn off_and_shadow_decide_by_name_and_enforce_by_class_only_where_a_view_exists() {
        // A view that disagrees with the name on every point: `browser.act` declared a read.
        let read = view("browser.act", "read");
        for gate in [ToolClassGate::Off, ToolClassGate::Shadow] {
            assert!(
                is_risky_call_with(gate, Some(&read), "browser.act", "{}"),
                "{gate:?}"
            );
            assert_eq!(
                evaluate_call_with(gate, Some(&read), PermissionMode::Ask, "browser.act", "{}"),
                PermissionDecision::AwaitApproval,
                "{gate:?}"
            );
            assert_eq!(
                rung_required_for_with(gate, Some(&read), "browser.act", "{}"),
                AutonomyRung::DangerFullAccess,
                "{gate:?}"
            );
        }
        assert!(!is_risky_call_with(
            ToolClassGate::Enforce,
            Some(&read),
            "browser.act",
            "{}"
        ));
        assert_eq!(
            evaluate_call_with(
                ToolClassGate::Enforce,
                Some(&read),
                PermissionMode::Ask,
                "browser.act",
                "{}"
            ),
            PermissionDecision::Allow
        );
        // No view of the tool: the legacy answer, whatever the gate.
        for gate in [
            ToolClassGate::Off,
            ToolClassGate::Shadow,
            ToolClassGate::Enforce,
        ] {
            assert!(
                is_risky_call_with(gate, None, "browser.act", "{}"),
                "{gate:?}"
            );
            assert!(
                is_risky_call_with(gate, None, "mcp__s__t", "{}"),
                "{gate:?}"
            );
        }
    }

    #[test]
    fn shadow_counts_a_divergence_by_point_and_tool_and_stays_silent_when_they_agree() {
        let counted = Counted::default();
        let guard = metrics::set_default_local_recorder(&counted);
        let declared_read = view("browser.act", "read");
        let agreeing = view("browser.act", "external_write");
        assert!(is_risky_call_with(
            ToolClassGate::Shadow,
            Some(&declared_read),
            "browser.act",
            "{}"
        ));
        assert!(is_risky_call_with(
            ToolClassGate::Shadow,
            Some(&agreeing),
            "browser.act",
            "{}"
        ));
        // An MCP name is bucketed so a connected server cannot grow the label set.
        let mcp_read = view("mcp__srv__echo", "read");
        assert!(is_risky_call_with(
            ToolClassGate::Shadow,
            Some(&mcp_read),
            "mcp__srv__echo",
            "{}"
        ));
        // Enforce and off do not count: enforce has no legacy to compare, off no view.
        assert!(!is_risky_call_with(
            ToolClassGate::Enforce,
            Some(&declared_read),
            "browser.act",
            "{}"
        ));
        assert!(is_risky_call_with(
            ToolClassGate::Off,
            Some(&declared_read),
            "browser.act",
            "{}"
        ));
        drop(guard);
        let mut seen = counted.0.lock().unwrap().clone();
        seen.sort();
        assert_eq!(
            seen,
            [
                "tool_class_divergence_total{point=risky,tool=browser.act}",
                "tool_class_divergence_total{point=risky,tool=mcp__}",
            ]
        );
    }

    #[test]
    fn an_approval_floor_asks_on_every_posture_except_the_stricter_deny() {
        for floor in ["ask", "durable_owner"] {
            let view = ToolClassView::from_registry("mcp__srv__x", "read", floor, "d");
            assert_eq!(
                evaluate_call_with_class(PermissionMode::Auto, &view, "{}"),
                PermissionDecision::AwaitApproval,
                "{floor}"
            );
            assert_eq!(
                evaluate_call_with_class(PermissionMode::Ask, &view, "{}"),
                PermissionDecision::AwaitApproval,
                "{floor}"
            );
            assert_eq!(
                evaluate_call_with_class(PermissionMode::Deny, &view, "{}"),
                PermissionDecision::Deny,
                "{floor}"
            );
        }
        // An owner effect carries its floor whatever the registry row said.
        let owner = ToolClassView::from_registry("mcp__srv__x", "owner_effect", "none", "d");
        assert_eq!(owner.floor(), ApprovalFloor::DurableOwner);
        assert_eq!(
            evaluate_call_with_class(PermissionMode::Auto, &owner, "{}"),
            PermissionDecision::AwaitApproval
        );
    }

    #[test]
    fn an_mcp_read_runs_under_ask_and_every_other_class_pauses() {
        for (class, expected) in [
            ("read", PermissionDecision::Allow),
            ("sandbox_compute", PermissionDecision::Allow),
            ("context_write", PermissionDecision::Allow),
            ("external_write", PermissionDecision::AwaitApproval),
            ("destructive", PermissionDecision::AwaitApproval),
            ("owner_effect", PermissionDecision::AwaitApproval),
            ("unclassified", PermissionDecision::AwaitApproval),
            ("a_class_from_the_future", PermissionDecision::AwaitApproval),
        ] {
            assert_eq!(
                evaluate_call_with_class(PermissionMode::Ask, &view("mcp__srv__t", class), "{}"),
                expected,
                "{class}"
            );
        }
        // Under auto only the floor pauses a call, as for a built-in.
        assert_eq!(
            evaluate_call_with_class(
                PermissionMode::Auto,
                &view("mcp__srv__t", "external_write"),
                "{}"
            ),
            PermissionDecision::Allow
        );
    }

    #[test]
    fn plan_mode_and_leaf_roles_allow_an_mcp_read_and_refuse_the_rest() {
        // The predicate plan mode and the leaf role both use.
        let refused = |class: &str| {
            let view = view("mcp__srv__t", class);
            is_risky_class(&view, "{}") || is_restricted_context_write_class(&view, "mcp__srv__t")
        };
        assert!(!refused("read"));
        assert!(!refused("sandbox_compute"));
        for class in [
            "context_write",
            "external_write",
            "destructive",
            "owner_effect",
            "unclassified",
            "bogus",
        ] {
            assert!(refused(class), "{class}");
        }
        // And the one durable write plan mode exists for stays callable.
        let proposal = view("plan.propose", "context_write");
        assert!(!is_restricted_context_write_class(
            &proposal,
            "plan.propose"
        ));
        assert!(is_restricted_context_write_class(&proposal, "save_memory"));
        assert_eq!(
            rung_required_for_class(&proposal, "plan.propose", "{}"),
            AutonomyRung::ReadOnly
        );
        assert_eq!(
            rung_required_for_class(&proposal, "save_memory", "{}"),
            AutonomyRung::WorkspaceWrite
        );
    }

    #[test]
    fn only_the_provider_operation_refinement_may_lower_a_call_and_only_to_a_read() {
        let action = view(EXECUTE_PROVIDER_ACTION_TOOL, "external_write");
        assert!(
            !is_risky_class(&action, READ_CALL),
            "a catalogued read operation"
        );
        assert_eq!(
            rung_required_for_class(&action, EXECUTE_PROVIDER_ACTION_TOOL, READ_CALL),
            AutonomyRung::ReadOnly
        );
        for args in [
            WRITE_CALL,
            "{}",
            "not json",
            r#"{"operation":"nobody.knows"}"#,
        ] {
            assert!(is_risky_class(&action, args), "{args}");
            assert_eq!(
                rung_required_for_class(&action, EXECUTE_PROVIDER_ACTION_TOOL, args),
                AutonomyRung::DangerFullAccess,
                "{args}"
            );
        }
        // No other tool lowers itself, whatever its arguments say.
        let other = view("mcp__srv__create", "external_write");
        assert!(is_risky_class(&other, READ_CALL));
        // And the refinement does not follow a class the registry did not declare for it.
        let lowered_by_name = view(EXECUTE_PROVIDER_ACTION_TOOL, "destructive");
        assert!(is_risky_class(&lowered_by_name, READ_CALL));
    }

    #[test]
    fn a_run_gate_reads_the_runs_own_snapshot_and_no_other() {
        let (org, run) = ("org-run-gate", "run-run-gate");
        record_run_views(
            org,
            run,
            [("mcp__s__echo".to_owned(), view("mcp__s__echo", "read"))],
        );
        let enforce = RunGate::with_gate(ToolClassGate::Enforce, org, run);
        assert!(!enforce.is_risky_call("mcp__s__echo", "{}"));
        assert_eq!(
            enforce.evaluate_call(PermissionMode::Ask, "mcp__s__echo", "{}"),
            PermissionDecision::Allow
        );
        assert!(enforce
            .check_autonomy_rung(AutonomyRung::ReadOnly, "mcp__s__echo", "{}")
            .is_ok());
        // Another run, another tenant, and a name the snapshot lacks: the strict legacy answer.
        for other in [
            RunGate::with_gate(ToolClassGate::Enforce, org, "another-run"),
            RunGate::with_gate(ToolClassGate::Enforce, "another-org", run),
        ] {
            assert!(other.is_risky_call("mcp__s__echo", "{}"));
            assert!(other
                .check_autonomy_rung(AutonomyRung::ReadOnly, "mcp__s__echo", "{}")
                .is_err());
        }
        assert!(enforce.is_risky_call("mcp__s__elsewhere", "{}"));
        // The gate off never reads the snapshot.
        let off = RunGate::with_gate(ToolClassGate::Off, org, run);
        assert!(off.is_risky_call("mcp__s__echo", "{}"));
        // An unstated rung stays no constraint, class or not.
        assert!(enforce
            .check_autonomy_rung(AutonomyRung::Unspecified, "browser.act", "{}")
            .is_ok());
    }

    #[test]
    fn a_run_gate_names_the_rung_a_call_needs_from_the_runs_declared_class() {
        let (org, run) = ("org-run-gate-rung", "run-run-gate-rung");
        record_run_views(
            org,
            run,
            [
                ("mcp__s__echo".to_owned(), view("mcp__s__echo", "read")),
                ("mcp__s__post".to_owned(), view("mcp__s__post", "external_write")),
            ],
        );
        let enforce = RunGate::with_gate(ToolClassGate::Enforce, org, run);
        assert_eq!(
            enforce.rung_required_for("mcp__s__echo", "{}"),
            AutonomyRung::ReadOnly
        );
        assert_eq!(
            enforce.rung_required_for("mcp__s__post", "{}"),
            AutonomyRung::DangerFullAccess
        );
        // No view for the name, and the gate off: the legacy name-based rung.
        assert_eq!(
            enforce.rung_required_for("mcp__s__elsewhere", "{}"),
            AutonomyRung::DangerFullAccess
        );
        let off = RunGate::with_gate(ToolClassGate::Off, org, run);
        assert_eq!(
            off.rung_required_for("mcp__s__echo", "{}"),
            rung_required_for("mcp__s__echo", "{}")
        );
    }

    #[test]
    fn a_recorded_snapshot_replaces_the_runs_views_and_the_oldest_run_is_forgotten_first() {
        let mut store = RunViewStore::default();
        let key = |n: usize| ("org".to_owned(), format!("run-{n}"));
        let one = |name: &str, class: &str| HashMap::from([(name.to_owned(), view(name, class))]);
        store.record(key(0), one("a", "read"));
        store.record(key(0), one("b", "read"));
        assert!(
            store.view(&key(0), "a").is_none(),
            "a snapshot replaces, it does not merge"
        );
        assert!(store.view(&key(0), "b").is_some());

        for n in 1..MAX_TRACKED_RUNS {
            store.record(key(n), one("a", "read"));
        }
        assert_eq!(store.runs.len(), MAX_TRACKED_RUNS);
        // Reading run 0 makes it the most recently used, so run 1 goes next.
        assert!(store.view(&key(0), "b").is_some());
        store.record(key(MAX_TRACKED_RUNS), one("a", "read"));
        assert_eq!(store.runs.len(), MAX_TRACKED_RUNS);
        assert!(
            store.view(&key(1), "a").is_none(),
            "the least recently used run is forgotten"
        );
        assert!(store.view(&key(0), "b").is_some());
        assert!(store.view(&key(MAX_TRACKED_RUNS), "a").is_some());
    }

    #[test]
    fn the_legacy_mcp_blanket_is_what_off_and_a_missing_view_still_answer() {
        // The test the class-driven engine replaced: an `mcp__` name is gated under
        // `ask` and not under `auto` when nothing declared otherwise.
        assert!(is_risky_call_with(
            ToolClassGate::Enforce,
            None,
            "mcp__fakemcp__echo",
            "{}"
        ));
        assert_eq!(
            evaluate_call_with(
                ToolClassGate::Enforce,
                None,
                PermissionMode::Ask,
                "mcp__fakemcp__echo",
                "{}"
            ),
            PermissionDecision::AwaitApproval
        );
        assert_eq!(
            evaluate_call_with(
                ToolClassGate::Enforce,
                None,
                PermissionMode::Auto,
                "mcp__fakemcp__echo",
                "{}"
            ),
            PermissionDecision::Allow
        );
    }

    /// The two never-auto predicates are floor lookups under `enforce`, and say what
    /// the names say for every built-in the registry seeds, so replacing them moves
    /// no built-in.
    #[test]
    fn the_never_auto_predicates_follow_the_declared_floor_and_reproduce_the_names() {
        for (tool, class, floor) in seeded_built_ins() {
            let view = ToolClassView::from_registry(&tool, &class, &floor, "");
            assert_eq!(
                requires_durable_owner_approval_with(ToolClassGate::Enforce, Some(&view), &tool),
                requires_durable_owner_approval(&tool),
                "owner approval: {tool} ({class}/{floor})"
            );
            assert_eq!(
                requires_consent_to_disclose_with(ToolClassGate::Enforce, Some(&view), &tool),
                requires_consent_to_disclose(&tool),
                "disclosure consent: {tool} ({class}/{floor})"
            );
        }
    }

    #[test]
    fn the_never_auto_predicates_are_decided_by_name_unless_enforced_and_counted_in_shadow() {
        let counted = Counted::default();
        let guard = metrics::set_default_local_recorder(&counted);
        // A view that disagrees with the names: the owner effect declared with no floor
        // cannot exist (the floor is raised to the class), so declare the floor itself.
        let owner = ToolClassView::from_registry("mcp__srv__x", "read", "durable_owner", "d");
        let consent = ToolClassView::from_registry("mcp__srv__y", "read", "ask", "d");
        let open = view("tickets.create", "read");
        for gate in [ToolClassGate::Off, ToolClassGate::Shadow] {
            assert!(
                requires_durable_owner_approval_with(gate, Some(&open), "tickets.create"),
                "{gate:?}"
            );
            assert!(
                !requires_durable_owner_approval_with(gate, Some(&owner), "mcp__srv__x"),
                "{gate:?}"
            );
            assert!(
                !requires_consent_to_disclose_with(gate, Some(&consent), "mcp__srv__y"),
                "{gate:?}"
            );
        }
        assert!(requires_durable_owner_approval_with(
            ToolClassGate::Enforce,
            Some(&owner),
            "mcp__srv__x"
        ));
        assert!(requires_consent_to_disclose_with(
            ToolClassGate::Enforce,
            Some(&consent),
            "mcp__srv__y"
        ));
        // Nothing declared for the tool: the legacy name answer, the strict one.
        for gate in [
            ToolClassGate::Off,
            ToolClassGate::Shadow,
            ToolClassGate::Enforce,
        ] {
            assert!(
                requires_durable_owner_approval_with(gate, None, "tickets.create"),
                "{gate:?}"
            );
            assert!(
                requires_consent_to_disclose_with(
                    gate,
                    None,
                    crate::runtime_loop::subagent_results::READ_TOOL
                ),
                "{gate:?}"
            );
        }
        drop(guard);
        let mut seen = counted.0.lock().unwrap().clone();
        seen.sort();
        assert_eq!(
            seen,
            [
                "tool_class_divergence_total{point=disclosure_consent,tool=mcp__}",
                "tool_class_divergence_total{point=owner_approval,tool=mcp__}",
                "tool_class_divergence_total{point=owner_approval,tool=tickets.create}",
            ]
        );
    }

    #[test]
    fn a_run_gate_answers_the_never_auto_predicates_from_the_runs_snapshot() {
        let (org, run) = ("org-run-gate-floor", "run-run-gate-floor");
        record_run_views(
            org,
            run,
            [(
                "mcp__s__pay".to_owned(),
                ToolClassView::from_registry("mcp__s__pay", "external_write", "durable_owner", "d"),
            )],
        );
        let enforce = RunGate::with_gate(ToolClassGate::Enforce, org, run);
        assert!(enforce.requires_durable_owner_approval("mcp__s__pay"));
        assert!(!enforce.requires_consent_to_disclose("mcp__s__pay"));
        // Another run has no snapshot, the gate off never reads one.
        assert!(
            !RunGate::with_gate(ToolClassGate::Enforce, org, "another-run")
                .requires_durable_owner_approval("mcp__s__pay")
        );
        assert!(!RunGate::with_gate(ToolClassGate::Off, org, run)
            .requires_durable_owner_approval("mcp__s__pay"));
        assert!(enforce.requires_durable_owner_approval("tickets.create"));
    }

    /// A disclosure is a read class carrying the `ask` floor, not any `ask` floor. An
    /// approval on a side effect is not one, and an unclassified tool has the floor by
    /// implication without being a read. Off and shadow answer by name.
    #[test]
    fn an_ask_floor_is_a_disclosure_only_on_a_read_class() {
        let read_tool = crate::runtime_loop::subagent_results::READ_TOOL;
        let lookup = ToolClassView::from_registry("mcp__s__lookup", "read", "ask", "d");
        assert!(requires_consent_to_disclose_with(
            ToolClassGate::Enforce,
            Some(&lookup),
            "mcp__s__lookup"
        ));
        for (tool, class, floor) in [
            ("mcp__s__w", "external_write", "ask"),
            ("mcp__s__del", "destructive", "ask"),
            ("mcp__s__mystery", "unclassified", "none"),
            ("mcp__s__mem", "context_write", "ask"),
            ("mcp__s__box", "sandbox_compute", "ask"),
            ("mcp__s__own", "read", "durable_owner"),
        ] {
            let view = ToolClassView::from_registry(tool, class, floor, "d");
            assert!(
                !requires_consent_to_disclose_with(ToolClassGate::Enforce, Some(&view), tool),
                "{tool} ({class}/{floor}) is a side effect, not a disclosure"
            );
        }
        assert_eq!(
            ToolClassView::from_registry("mcp__s__mystery", "unclassified", "none", "d").floor(),
            ApprovalFloor::Ask,
            "an unclassified tool is asked for by implication, and is still not a read"
        );
        // Off and shadow answer by name, whatever the view says.
        let drift = ToolClassView::from_registry(read_tool, "external_write", "none", "d");
        for gate in [ToolClassGate::Off, ToolClassGate::Shadow] {
            assert!(
                !requires_consent_to_disclose_with(gate, Some(&lookup), "mcp__s__lookup"),
                "{gate:?}"
            );
            assert!(
                requires_consent_to_disclose_with(gate, Some(&drift), read_tool),
                "{gate:?}"
            );
        }
        // Shadow counts a disagreement and the legacy answer decides. A side effect with
        // the ask floor agrees with the name, so it is silent. The reserved name whose
        // row says external write disagrees, so it is counted, once.
        let counted = Counted::default();
        let guard = metrics::set_default_local_recorder(&counted);
        let pay = ToolClassView::from_registry("mcp__s__pay", "external_write", "ask", "d");
        assert!(!requires_consent_to_disclose_with(
            ToolClassGate::Shadow,
            Some(&pay),
            "mcp__s__pay"
        ));
        assert!(requires_consent_to_disclose_with(
            ToolClassGate::Shadow,
            Some(&drift),
            read_tool
        ));
        drop(guard);
        let seen = counted.0.lock().unwrap().clone();
        assert_eq!(
            seen,
            ["tool_class_divergence_total{point=disclosure_consent,tool=read_subagent_result}"]
        );
    }
}
