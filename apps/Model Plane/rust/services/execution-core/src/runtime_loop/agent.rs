//! Agent run driver — governed multi-tool loop.
//!
//! Bridges the gap where `session-core.StartRun` durably records a run as
//! `'queued'` + a `RUN_STARTED` event + a draft plan, then returns with nothing
//! to dispatch it. [`run_agent`] is that dispatch: it transitions the draft plan
//! to executing, then runs a capped ReAct-style loop — offer the read-tool
//! allowlist, `Infer`, dispatch any requested tool calls back through
//! `runtime_loop::execute_step` (the same permission/hook-gated path the
//! `ExecuteStep` RPC uses), feed the outcomes back, and re-infer until the model
//! answers or the round budget is exhausted. When retention permits, the
//! assistant answer is persisted to the run's thread. The terminal state is
//! then recorded through Session Core's immutable managed-run receipt, before
//! this process exposes a terminal projection.
//!
//! Governance is the point of this layer:
//!   * **HITL** — when a tool call is gated (`ask` posture + a risky tool),
//!     `execute_step` returns `awaiting_approval`; the loop mints a durable
//!     `CreateApproval` (exactly as the `ExecuteStep` RPC does), flips the run
//!     `AwaitingApproval`, and RETURNS early. Resume re-invokes `run_agent`.
//!   * **Default posture is `ask`** — deployed agents must not silently run
//!     `shell`; only an explicit `auto` mode opens that gate.
//!   * **Purpose-lock (scope lock)** — the agent may ONLY call tools in the
//!     offered allowlist; an un-offered tool name is rejected, never dispatched.
//!     The bound goal is logged once at run start.
//!   * **GDPR** — every dispatched tool's `data_category` and the run's real
//!     ZDR flag (`RunAgentRequest.zdr`, populated by the gateway from the chat
//!     request) are carried on the per-tool step's output detail so session-core
//!     can publish an audit event, and the ZDR flag is threaded into every
//!     inference round.
//!
//! Per-tool audit steps use a NON-TERMINAL status (`"running"`). The separate
//! managed-run lifecycle owns the single terminal transition, so a per-tool
//! audit record can never end a run mid-loop.
//!
//! A run must NEVER be left `'queued'`: an Infer/persist error attempts a
//! durable failed managed receipt and then returns
//! `RunAgentResponse { status: "failed", .. }`. If the receipt cannot be
//! obtained, the call fails unavailable rather than claiming a terminal state.

use std::collections::BTreeSet;

use mp_contracts::model_plane::v1::{
    self as pb, inference_core_client::InferenceCoreClient,
    managed_run_lifecycle_client::ManagedRunLifecycleClient,
    orchestration_core_service_client::OrchestrationCoreServiceClient,
    session_core_client::SessionCoreClient,
};
use serde_json::{json, Value as JsonValue};
use tonic::transport::Channel;
use tracing::{info, warn};

use crate::permission::PermissionMode;
use crate::runtime_loop::{self, StepOutcome};
use crate::session_terminal_auth::ManagedRunTokenProvider;

/// Tool-agnostic core of the system prompt.
///
/// # Why this is composed rather than one string (pi parity, §4.4)
///
/// This was a single monolithic preamble that described `knowledge_search`'s
/// JSON status semantics, shipment booking and social publishing
/// **unconditionally** — including on runs where none of those tools were
/// offered. Two costs: tokens spent on rules that cannot apply, and, worse, the
/// model told it can take actions this run has no tool for. That is the exact
/// inverse of the regression the old comment recorded (a prompt that
/// *understated* capability and measurably suppressed real tool use, observed
/// 2026-07-08 while calibrating the eval harness's HITL case) — and it fails the
/// same way, by describing a toolset that is not the one in front of the model.
///
/// Every measured phrasing is preserved verbatim; what changed is that each
/// piece now appears only when its tool is actually offered. Adding a tool means
/// adding its snippet next to the tool, not editing an unrelated paragraph.
const PREAMBLE_CORE: &str = "You are Verevon, a concise and helpful assistant for a Norwegian \
business. Prefer calling a tool over answering from memory whenever a listed tool could give a \
more current, accurate, or actionable result — even when the user phrases the request casually, \
indirectly, or as a question rather than a command. Do not ask whether the user wants you to \
proceed before making a read-only tool call, and do not reply that you 'cannot' do something a \
listed tool covers; call the tool and let its result decide the outcome. Only use the tools you \
have been given. When you have enough information or have taken the requested action, answer the \
user's request directly and clearly.";

/// Appended when at least one READ tool is offered.
const SNIPPET_READ_TOOLS: &str = "You have READ tools to gather facts. Use them to ground your \
answer rather than recalling from memory.";

/// Appended when at least one real-effect ACTION tool is offered.
///
/// This is the wording whose absence was *measured* to suppress tool use:
/// models defaulted to "I cannot post on your behalf" and drafted copy-paste
/// text instead of calling the tool, even with a genuinely connected account.
/// It must appear whenever an action tool is present — and must NOT appear when
/// none is, which is the half that was wrong before.
const SNIPPET_ACTION_TOOLS: &str = "You also have ACTION tools that take REAL effect for this \
organization. You DO have genuine capability to take these actions through the listed tools; you \
are not limited to suggesting text for the user to act on themselves. When the user asks you to \
do something an action tool covers, CALL the tool — do not default to 'I cannot access your \
accounts' or offer copy-paste text instead, and do not ask the user to do it manually unless the \
tool call itself reports that it cannot proceed (e.g. no connected account). Risky action tools \
require human approval before they run; that pause is expected and is not a reason to avoid \
calling the tool — say what you are attempting and let the approval step do its job.";

/// Appended only when `knowledge_search` is offered. Describing this protocol
/// without the tool taught the model a retrieval vocabulary it had no way to
/// use, and invited it to claim organization-specific grounding it never had.
const SNIPPET_KNOWLEDGE_SEARCH: &str = "A knowledge_search result is JSON: when status is \
no_results or low_confidence, reformulate with materially different terms and retry within the \
round budget; never repeat the exact same retrieval, and never invent graph, SQL, structured, \
vector-only, or MCP retrieval when the tool says that route is not configured. If \
knowledge_search still finds nothing relevant after reformulating, say plainly that the \
organization's knowledge base has nothing on this — never present a guess as an \
organization-specific fact.";

/// Appended when the memory tools are offered.
///
/// These shipped with **no** prompt guidance at all, so the model was handed two
/// tools and no account of when either is worth calling — a tool the model never
/// reaches for is indistinguishable from one that does not exist.
const SNIPPET_MEMORY_TOOLS: &str = "You can remember durable facts across conversations. Call \
save_memory for something that will still be true and useful in a later conversation — a \
preference, a decision, a stable fact about the organization — not for the content of this turn \
and not for anything the user asked you to keep private. Call recall_memory when the request \
plausibly depends on something established earlier that is not in this conversation. Do not \
narrate either call; just use what you find.";

/// Appended when delegation is offered.
const SNIPPET_SUBAGENT: &str = "You can delegate a self-contained sub-task to a subagent. A \
subagent starts with no view of this conversation, so state its goal completely; delegate only \
work that is genuinely separable, and do the rest yourself.";

/// Appended when the delegation-record tools are offered.
///
/// Says the quiet part out loud: after a restart the answers are NOT in this
/// conversation, and getting one back costs the user an approval. A model that
/// does not know the first will claim a delegation produced nothing; one that
/// does not know the second will call the read reflexively and stall the run.
const SNIPPET_SUBAGENT_RESULTS: &str = "If you delegated work earlier in this run and cannot see \
what it concluded, the record still exists even though this conversation no longer holds it: list \
your delegations, then read one if you need its finding. Reading a finding asks the user for \
permission each time, so read one when the user asks about it or when you genuinely need it to \
continue — never speculatively.";

/// Appended when a tool is offered whose required arguments include values only
/// the user can supply.
///
/// # Why this is a snippet and not a per-tool description line
///
/// `get_shipping_quotes` has carried "Ask the user for sender address, recipient
/// address and package weight/dimensions before calling; never guess them" in
/// its own description all along. Measured 2026-08-25 on the full catalogue: it
/// invented the values anyway in **19 of 20** samples — postal codes it could
/// not know, dimensions nobody stated. Per-tool prose was already tried and
/// already failed, so repeating it on more tools would be cargo-culting a
/// measured non-fix.
///
/// What the same run showed working: the model resolves *discoverable* missing
/// values correctly and unprompted — it called `list_subagent_results` to find a
/// `child_run_id` and `list_social_accounts` to find a `connection_id`, 10/10
/// each. The gap is specific to values no tool can discover because only the
/// user holds them.
///
/// The last sentence exists because [`PREAMBLE_CORE`] forbids asking permission
/// before a read-only call, and that wording suppressed real tool use when it
/// was absent (see its doc comment). Asking for a missing *fact* is a different
/// act from asking permission, and the distinction has to be stated or this
/// snippet reads as a licence to stall.
const SNIPPET_USER_SUPPLIED_ARGS: &str = "Some offered tools require values only the user can \
supply — a street address, a postal code, package dimensions, a price. Fill required arguments \
freely when the request states them or when they are public fact (a Norwegian city's coordinates, \
a registered company's name), but never invent a user-only value: a call built on a guessed postal \
code or guessed dimensions still succeeds, and returns a real, plausible, wrong answer that nobody \
can tell apart from a correct one. When such a value is missing, do not call the tool at \
all — reply with one short question naming exactly the values you need. Asking for a missing fact \
is not asking permission: never ask whether to proceed with a call you can already make.";

/// Tools whose required arguments include values only the user can supply, so a
/// missing one must be asked for rather than filled.
///
/// Deliberately short. It is **not** "every tool with required parameters" —
/// that would be 19 of 25, and most of those requireds are restatable from the
/// request (`web_search.query`, `code_interpreter.code`) or public fact
/// (`yr_weather.lat/lon`). Telling the model to ask for those would manufacture
/// the under-calling regression [`PREAMBLE_CORE`] exists to prevent. Nor does it
/// include tools whose missing values are *discoverable* by another offered tool
/// (`read_subagent_result`, `execute_provider_action`); those were measured
/// correct without any prompt help.
const USER_SUPPLIED_ARG_TOOLS: &[&str] = &["get_shipping_quotes", "book_shipment"];

/// Tools whose presence means a real-effect action is available. Kept explicit
/// rather than derived from `permission::is_risky_call`: that classifier answers
/// "does this call need a gate", which is a per-call question about arguments,
/// while this answers "should the prompt describe action capability at all",
/// which is about the offered set. Conflating them would make the prompt vary
/// with tool *arguments*.
const ACTION_TOOL_NAMES: &[&str] = &[
    "book_shipment",
    "execute_provider_action",
    "publish_social_post",
    "browser_agent",
    "shell",
    "tickets.create",
];

/// Assemble the system prompt from the tools actually offered this run.
///
/// Order is fixed and snippets are independent, so the prompt is a pure function
/// of the offered set — the property the test asserts, and what makes a prompt
/// diff reviewable.
fn compose_system_prompt(offered: &BTreeSet<String>) -> String {
    let mut parts: Vec<&str> = vec![PREAMBLE_CORE];

    let has_action = offered
        .iter()
        .any(|name| ACTION_TOOL_NAMES.contains(&name.as_str()));
    // Anything that is not an action tool is, for prompt purposes, a read tool.
    if offered
        .iter()
        .any(|name| !ACTION_TOOL_NAMES.contains(&name.as_str()))
    {
        parts.push(SNIPPET_READ_TOOLS);
    }
    if has_action {
        parts.push(SNIPPET_ACTION_TOOLS);
    }
    if offered
        .iter()
        .any(|name| USER_SUPPLIED_ARG_TOOLS.contains(&name.as_str()))
    {
        parts.push(SNIPPET_USER_SUPPLIED_ARGS);
    }
    if offered.contains("knowledge_search") {
        parts.push(SNIPPET_KNOWLEDGE_SEARCH);
    }
    if offered.contains(runtime_loop::SAVE_MEMORY_TOOL)
        || offered.contains(runtime_loop::RECALL_MEMORY_TOOL)
    {
        parts.push(SNIPPET_MEMORY_TOOLS);
    }
    if offered
        .iter()
        .any(|name| crate::subagent::is_subagent_tool(name))
    {
        parts.push(SNIPPET_SUBAGENT);
    }
    if offered.contains(runtime_loop::subagent_results::LIST_TOOL) {
        parts.push(SNIPPET_SUBAGENT_RESULTS);
    }
    parts.join(" ")
}

/// Temperature for each inference round.
const TEMPERATURE: f32 = 0.7;

/// Max tokens for each inference round.
///
/// A round emits either tool-call arguments or the final answer, so it has to
/// fit the answer: at 1024 an agentic run would truncate exactly the long
/// deliverables (tables, multi-record summaries) that justify running an agent
/// instead of asking a one-shot question.
const MAX_TOKENS: i32 = 4096;

/// Default round budget when the request does not specify one.
///
/// Deliberately at least the inline-chat budget (`model-gateway`'s
/// `tool_loop::max_tool_rounds`): a deployed agent is the LONG-horizon surface,
/// so it must never get less room to work than plain chat. It was previously 4
/// with a ceiling of 8 — below what a self-describing MCP server needs just to
/// discover a schema before acting (Visma spends 2 rounds on
/// `list_skills` + `get_skill` before its first query), which left nothing for
/// acting on the result, let alone recovering from it.
pub(crate) const DEFAULT_MAX_ROUNDS: u32 = 12;

/// Hard ceiling on rounds regardless of the request, so a misbehaving caller
/// can't drive an unbounded loop.
pub(crate) const MAX_ROUNDS_CEILING: u32 = 32;

/// Cap on a single tool outcome rendered back into the conversation context.
/// Matches model-gateway's `MAX_TOOL_OUTPUT_CHARS` so the same tool result is
/// not silently richer on one surface than the other.
const MAX_TOOL_CONTEXT_CHARS: usize = 8000;

/// Graceful reply persisted to the thread when no inference round yields an
/// answer, so a user-facing answer always exists.
const GRACEFUL_FAILURE_REPLY: &str =
    "I wasn't able to complete that request right now. Please try again.";

/// Cap on the `CompleteStep.output` payload (the durable step record); the full
/// answer still lives in the appended assistant message.
const OUTPUT_TRUNCATE: usize = 2000;

/// GDPR data-classification of a tool's results, carried on the step audit
/// detail. The five Norwegian read tools + the public web touch only public,
/// non-personal data; `knowledge_search` reads the org's own private corpus.
fn data_category(tool_name: &str) -> &'static str {
    match tool_name {
        "knowledge_search" => "customer_private",
        // External MCP tools can touch arbitrary data of unknown provenance —
        // classify honestly as `unclassified` rather than over-claim that an
        // external server returns only public, non-personal data.
        name if name.starts_with("mcp__") => "unclassified",
        // A subagent inherits the parent's whole toolset, `knowledge_search`
        // included, so its synthesized answer may quote the org's private
        // corpus. Classify by the worst case it can genuinely contain.
        name if crate::subagent::is_subagent_tool(name) => "customer_private",
        _ => "public_non_personal",
    }
}

/// One requested tool call's outcome, framed for the next round's context.
struct ToolStepResult {
    name: String,
    output: String,
    error: Option<String>,
}

/// One requested tool call's pre-dispatch fate, computed by `run_rounds`'
/// sequential pre-pass before the surviving calls are dispatched concurrently.
enum CallSlot<'r> {
    Rejected(ToolStepResult),
    ToDispatch {
        call: &'r pb::ToolCall,
        step_id: String,
    },
}

/// The first (by array order) approval-needing call in a batch, captured so
/// `run_rounds` can finish recording every OTHER call's outcome before
/// actually pausing on this one.
struct PendingPause {
    step_id: String,
    name: String,
    arguments_json: String,
}

/// Everything a round loop needs that does not change between its rounds.
///
/// Extracted so [`run_rounds`] is REENTRANT: the user-facing run and every
/// delegated subagent are the same driver with a different context, rather than
/// two implementations that drift. A subagent inherits the run's identity,
/// credentials, tools, posture and ZDR flag verbatim through `req` — delegation
/// is not an opportunity to re-derive scope from model input.
struct LoopContext<'a> {
    state: &'a crate::state::StateStore,
    session_channel: &'a Channel,
    inference_channel: &'a Channel,
    /// The run this loop belongs to. A subagent runs INSIDE its parent's run, so
    /// org/user/run/thread/ZDR are shared; only the goal and budget differ.
    req: &'a pb::RunAgentRequest,
    tools: Vec<pb::ToolDefinition>,
    /// Purpose-lock scope, derived from `tools`.
    allowlist: BTreeSet<String>,
    data_plane_bearer: Option<&'a str>,
    session_bearer: Option<&'a str>,
    inference_bearer: &'a str,
    capability_policy: &'a dyn crate::capability_policy::CapabilityPolicy,
    terminal_tokens: &'a dyn ManagedRunTokenProvider,
    /// Present only when `req.space_id` is non-empty — see
    /// `auth::authenticate_delegated_sandbox_manager`'s doc for why this
    /// specific bearer must be user-bound. Read only by the `code_interpreter`
    /// dispatch arm in `run_rounds`, and only when `req.space_id` is set.
    sandbox_bearer: Option<&'a str>,
    capability_client: Option<&'a crate::capability_client::CapabilityClient>,
    sandbox_manager_client: &'a crate::sandbox_manager_client::SandboxManagerClient,
    backend_id: &'a str,
    cas_client: Option<&'a crate::workspace_cas::CasClient>,
    sandbox_tokens: &'a crate::sandbox_lease::SandboxManagerTokenProvider,
    agent_model: String,
    permission_wire: &'static str,
    /// Nesting depth: 0 is the user-facing run, 1 a delegated subagent. Bounds
    /// recursion via `subagent::guard_depth` and selects the run-level
    /// behaviours (HITL pause, terminalization) only the root loop may perform.
    depth: u32,
    /// Prefixed onto this loop's step ids so a delegated loop's steps are
    /// attributable to the tool call that spawned them instead of colliding
    /// with the parent's `tool_<n>_<id>` sequence.
    step_prefix: String,
}

/// Why a round loop stopped — without deciding what to do about it. The root run
/// finalizes; a subagent turns the same outcome into its tool result.
enum RoundsOutcome {
    Answered(String),
    Exhausted,
    InferFailed,
    /// A gated tool minted a durable approval and the run is paused. Root only:
    /// a delegated loop cannot own the run's HITL pause.
    Paused(pb::RunAgentResponse),
}

struct RoundsResult {
    /// Whether tier-1 compaction cleared anything during this loop. Carried out
    /// rather than logged only: compaction is lossy and otherwise invisible in
    /// the response.
    compaction_triggered: bool,
    outcome: RoundsOutcome,
    /// Inference rounds this loop drove itself.
    rounds_executed: u32,
    /// Inference rounds its delegated subagents drove, charged to the same
    /// budget so N delegations cannot multiply the run's total spend.
    delegated_rounds: u32,
    grounded: bool,
}

impl RoundsResult {
    /// Every inference round the run performed, this loop's and its delegates'.
    fn total_rounds(&self) -> u32 {
        self.rounds_executed.saturating_add(self.delegated_rounds)
    }
}

/// Drive an agent run to a terminal answer through a governed multi-tool loop.
///
/// Emits `PlanTransitioned DRAFT→EXECUTING`, then loops (offer tools → `Infer` →
/// dispatch tool calls via [`runtime_loop::execute_step`] → feed outcomes back)
/// up to the round budget. On a final answer it appends the assistant message
/// when retention permits, records ONE immutable managed terminal receipt, emits
/// `PlanTransitioned EXECUTING→COMPLETED`, and snapshots the `StateStore`. If a
/// tool call is gated it mints a durable approval and returns
/// `awaiting_approval` early.
/// On any failure it takes the graceful path and returns a `"failed"` response —
/// never leaving the run `'queued'`.
///
/// # Errors
///
/// Returns `unavailable` when a HITL-gated tool cannot durably persist its
/// approval. No pause state is reported in that case.
#[allow(
    clippy::too_many_arguments,
    reason = "the authenticated execution boundary keeps each audience-specific bearer explicit"
)]
pub(crate) async fn run_agent(
    state: &crate::state::StateStore,
    session_channel: Channel,
    inference_channel: Channel,
    req: pb::RunAgentRequest,
    data_plane_bearer: Option<String>,
    session_bearer: Option<String>,
    inference_bearer: String,
    capability_policy: &dyn crate::capability_policy::CapabilityPolicy,
    terminal_tokens: &dyn ManagedRunTokenProvider,
    sandbox_bearer: Option<String>,
    capability_client: Option<&crate::capability_client::CapabilityClient>,
    sandbox_manager_client: &crate::sandbox_manager_client::SandboxManagerClient,
    backend_id: &str,
    cas_client: Option<&crate::workspace_cas::CasClient>,
    sandbox_tokens: &crate::sandbox_lease::SandboxManagerTokenProvider,
) -> Result<pb::RunAgentResponse, tonic::Status> {
    let tools = merged_tool_defs(
        &req.run_id,
        &req.org_id,
        &req.user_id,
        &req.tools,
        capability_policy,
    )
    .await;
    run_agent_with_tools(
        state,
        session_channel,
        inference_channel,
        req,
        tools,
        data_plane_bearer,
        session_bearer,
        inference_bearer,
        capability_policy,
        terminal_tokens,
        sandbox_bearer,
        capability_client,
        sandbox_manager_client,
        backend_id,
        cas_client,
        sandbox_tokens,
    )
    .await
}

/// The built-in [`offered_tool_defs`] plus the org's registered MCP tools,
/// discovered best-effort via the gateway (matrix §G2). MCP tools are
/// namespaced `mcp__<server>__<tool>`; appending them here both OFFERS them to
/// the model AND admits them into the purpose-lock allowlist (derived from this
/// Vec at ~line 154), so the `runtime_loop` dispatch arm can route them. A
/// built-in name always wins a (vanishingly unlikely) collision.
async fn merged_tool_defs(
    run_id: &str,
    org_id: &str,
    user_id: &str,
    client_tools: &[pb::ToolDefinition],
    capability_policy: &dyn crate::capability_policy::CapabilityPolicy,
) -> Vec<pb::ToolDefinition> {
    let mut tools = offered_tool_defs();
    if let Some(client) = crate::mcp_gateway::McpGatewayClient::from_env() {
        for tool in client.list_tools(org_id, user_id).await {
            append_untrusted_tool_defs(&mut tools, &[tool]);
        }
    }
    // chat-parity: fold in the caller's declared tools (RunAgentRequest.tools),
    // so a client can widen the agentic run's scope with its own or MCP tools.
    // A built-in or org MCP tool of the same name wins (the governed server
    // definition is authoritative and non-overridable); any NEW client tool
    // name is admitted into both the offered set and the purpose-lock allowlist.
    // Reserved owner actions are the exception: only a future run-bound,
    // server-resolved catalog may offer them, never a caller or MCP registry.
    // Execution still flows through the gated execute_step dispatch, so this
    // never bypasses the permission/HITL gate; a name with no resolvable
    // executor returns a graceful error the loop feeds back to the model.
    append_untrusted_tool_defs(&mut tools, client_tools);
    match capability_policy
        .resolve_server_tool_definitions(run_id, org_id)
        .await
    {
        Ok(resolved_tools) => append_server_resolved_tool_defs(&mut tools, &resolved_tools),
        Err(error) => tracing::warn!(
            code = ?error.code(),
            run_id,
            "server-resolved owner-action view unavailable; reserved tool remains absent"
        ),
    }
    tools
}

fn append_untrusted_tool_defs(
    tools: &mut Vec<pb::ToolDefinition>,
    untrusted_tools: &[pb::ToolDefinition],
) {
    for tool in untrusted_tools {
        if tool.name.trim().is_empty() || is_server_resolved_tool_name(&tool.name) {
            continue;
        }
        if !tools.iter().any(|existing| existing.name == tool.name) {
            tools.push(tool.clone());
        }
    }
}

// The only source permitted to add a reserved owner action is Capability
// Core's run-bound resolver. Its client validates the signed Control view and
// fixed schema before this helper sees a definition; this final exact-name
// allowlist prevents a compromised catalog response from becoming a general
// tool injection channel.
fn append_server_resolved_tool_defs(
    tools: &mut Vec<pb::ToolDefinition>,
    resolved_tools: &[pb::ToolDefinition],
) {
    for tool in resolved_tools {
        if tool.name != crate::ticket_tools::TOOL_NAME
            || tool.description.trim().is_empty()
            || serde_json::from_str::<serde_json::Value>(&tool.parameters_json).is_err()
        {
            continue;
        }
        if !tools.iter().any(|existing| existing.name == tool.name) {
            tools.push(tool.clone());
        }
    }
}

// A Model-facing action that crosses an owner-plane boundary must not be
// advertised merely because a caller can name it. `tickets.create` remains
// absent until Capability Core, Control, and Conversation Core resolve a fresh
// run-bound view. This is intentionally an exact identifier check: aliases and
// near-matches have no executor binding and fail closed in capability policy.
fn is_server_resolved_tool_name(name: &str) -> bool {
    name.trim() == crate::ticket_tools::TOOL_NAME
}

/// Run-level driver for `req` with an explicit tool allowlist: plan transition →
/// [`run_rounds`] → finalize. The public [`run_agent`] passes
/// [`offered_tool_defs`]; tests inject a smaller (or gated) tool set to exercise
/// the purpose-lock and HITL branches without a live tool backend.
///
/// The ReAct-style round loop itself lives in [`run_rounds`] because a delegated
/// subagent re-enters it, and must NOT re-enter any of the run-level work here:
/// there is exactly one plan transition and one managed terminal receipt per
/// run, no matter how many nested loops it drove.
#[allow(clippy::too_many_arguments)]
async fn run_agent_with_tools(
    state: &crate::state::StateStore,
    session_channel: Channel,
    inference_channel: Channel,
    req: pb::RunAgentRequest,
    tools: Vec<pb::ToolDefinition>,
    data_plane_bearer: Option<String>,
    session_bearer: Option<String>,
    inference_bearer: String,
    capability_policy: &dyn crate::capability_policy::CapabilityPolicy,
    terminal_tokens: &dyn ManagedRunTokenProvider,
    sandbox_bearer: Option<String>,
    capability_client: Option<&crate::capability_client::CapabilityClient>,
    sandbox_manager_client: &crate::sandbox_manager_client::SandboxManagerClient,
    backend_id: &str,
    cas_client: Option<&crate::workspace_cas::CasClient>,
    sandbox_tokens: &crate::sandbox_lease::SandboxManagerTokenProvider,
) -> Result<pb::RunAgentResponse, tonic::Status> {
    let plan_id = format!("plan_{}", req.run_id);

    // Posture: deployed agents default to `ask` (risky tools gated behind a
    // human approval). Only an explicit `auto` opens the gate; an unknown/empty
    // mode is treated as `ask` so `shell` is never silently allowed.
    let mode = resolve_mode(&req.mode);
    let permission_wire = mode_wire(mode);

    // Purpose-lock: the offered allowlist IS the scope lock. Tool calls outside
    // it are rejected, never dispatched.
    let allowlist: BTreeSet<String> = tools.iter().map(|t| t.name.clone()).collect();
    let max_rounds = if req.max_rounds == 0 {
        DEFAULT_MAX_ROUNDS
    } else {
        req.max_rounds.min(MAX_ROUNDS_CEILING)
    };

    // Autonomous, tool-using runs need a tool-following model. A weak chat-tier
    // model (e.g. gpt-4o-mini) under-elects tools unless the prompt is forceful,
    // so `EXECUTION_AGENT_MODEL` (e.g. "verevon-balance") lets the deployment
    // route the agentic loop through inference-core's intent layer, which
    // upgrades the model when tools are offered. Empty env → honor the
    // requested model unchanged (no behaviour change).
    let agent_model = resolve_agent_model(&req.model);

    info!(
        run_id = %req.run_id,
        mode = %permission_wire,
        max_rounds,
        tools = ?allowlist,
        model = %agent_model,
        goal = %req.goal,
        "run_agent: purpose-locked governed multi-tool run starting"
    );

    heartbeat_managed_agent_run(&session_channel, &req, terminal_tokens).await?;

    // 1. Plan: DRAFT → EXECUTING.
    publish_plan_transition(
        &session_channel,
        &plan_id,
        &req.run_id,
        pb::PlanState::Draft,
        pb::PlanState::Executing,
        session_bearer.as_deref(),
    )
    .await;

    let context = LoopContext {
        state,
        session_channel: &session_channel,
        inference_channel: &inference_channel,
        req: &req,
        tools,
        allowlist,
        data_plane_bearer: data_plane_bearer.as_deref(),
        session_bearer: session_bearer.as_deref(),
        inference_bearer: &inference_bearer,
        capability_policy,
        terminal_tokens,
        sandbox_bearer: sandbox_bearer.as_deref(),
        capability_client,
        sandbox_manager_client,
        backend_id,
        cas_client,
        sandbox_tokens,
        agent_model,
        permission_wire,
        depth: 0,
        step_prefix: String::new(),
    };

    // 2. The governed ReAct loop.
    let rounds = run_rounds(&context, &req.goal, max_rounds).await?;
    let total_rounds = rounds.total_rounds();
    // Captured before `rounds.outcome` is moved out below.
    let compaction_triggered = rounds.compaction_triggered;
    let (final_answer, success) = match rounds.outcome {
        // Paused, not finished: the approval is already durable and the run
        // stays non-terminal. Resume re-invokes `run_agent`.
        RoundsOutcome::Paused(response) => return Ok(response),
        RoundsOutcome::Answered(text) => (text, true),
        RoundsOutcome::Exhausted | RoundsOutcome::InferFailed => {
            (GRACEFUL_FAILURE_REPLY.to_owned(), false)
        }
    };

    let response = finalize(
        state,
        &session_channel,
        &plan_id,
        &req,
        &final_answer,
        success,
        total_rounds,
        rounds.grounded,
        compaction_triggered,
        session_bearer.as_deref(),
        terminal_tokens,
    )
    .await?;

    // Only reached once `finalize` actually made the run terminal (it
    // returns `Err` — never mutating `StateStore` — when the durable receipt
    // itself failed, per its own doc). One release call here covers every
    // Space-scoped `code_interpreter` call this run made, including all of
    // its delegated subagents': they share this SAME `req.run_id` (a
    // subagent's `LoopContext.req` is `parent.req` verbatim, never its own),
    // so `state.sandbox_lease` was cached under the one key this run has
    // ever used.
    crate::sandbox_lease::release_sandbox_lease_if_any(
        state,
        sandbox_manager_client,
        sandbox_tokens,
        cas_client,
        &req.run_id,
        &req.org_id,
    )
    .await;

    Ok(response)
}

/// How many matched skills to inject per run — mirrors model-gateway's own
/// `MAX_INJECTED_SKILLS` (kept as an independent constant rather than a shared
/// one: the two loops live in separate crates/binaries and this number is a
/// tuning knob, not a cross-service contract).
const MAX_INJECTED_SKILLS: usize = 3;

/// Fetch this org's enabled learned skills (session-core's `agent_skills`,
/// `ListAgentSkills`) and return the ones whose name/keywords/content overlap
/// with `goal`, formatted as `format_skill_block`-shaped system-context
/// strings — execution-core's `RunAgent` equivalent of model-gateway's
/// `skills::fetch_skill_context`/`handle_match_skills` (same keyword-overlap
/// scoring, reimplemented locally rather than shared: the two live in
/// separate crates and the scoring is ~10 lines).
///
/// Deliberately uncached, unlike the gateway's per-org TTL cache: `RunAgent`'s
/// call volume is orders of magnitude below a chat SSE turn, so a fresh
/// `ListAgentSkills` per run is simpler and can never serve a skill an
/// operator just edited or deleted. Advisory — any failure (no bearer, no
/// org, RPC error) yields an empty Vec so the run proceeds unaffected, exactly
/// like the gateway path.
async fn fetch_skill_context(
    session_channel: &Channel,
    bearer: Option<&str>,
    org_id: &str,
    goal: &str,
) -> Vec<String> {
    let Some(bearer) = bearer else {
        return Vec::new();
    };
    if org_id.trim().is_empty() {
        return Vec::new();
    }
    let query_terms: Vec<String> = goal
        .to_lowercase()
        .split_whitespace()
        .filter(|t| t.len() >= 3)
        .map(str::to_owned)
        .collect();
    if query_terms.is_empty() {
        return Vec::new();
    }

    let mut request = tonic::Request::new(pb::ListAgentSkillsRequest {
        org_id: org_id.to_owned(),
        enabled_only: true,
    });
    let Ok(header) = format!("Bearer {bearer}").parse() else {
        return Vec::new();
    };
    request.metadata_mut().insert("authorization", header);
    let Ok(response) = SessionCoreClient::new(session_channel.clone())
        .list_agent_skills(request)
        .await
    else {
        return Vec::new();
    };

    let mut scored: Vec<(f32, pb::AgentSkill)> = response
        .into_inner()
        .skills
        .into_iter()
        .filter(|s| !s.content.trim().is_empty())
        .map(|s| {
            let haystack =
                format!("{} {} {}", s.name, s.trigger_keywords.join(" "), s.content).to_lowercase();
            let hits = query_terms
                .iter()
                .filter(|t| haystack.contains(t.as_str()))
                .count();
            // reason: keyword-overlap ratio; small counts lose no meaningful precision in f32.
            #[allow(clippy::cast_precision_loss)]
            let overlap = hits as f32 / query_terms.len() as f32;
            (overlap, s)
        })
        .filter(|(overlap, _)| *overlap > 0.0)
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(MAX_INJECTED_SKILLS);

    // Bounded by SIZE as well as count. Skill content is operator-authored free
    // text with no length limit, so three long skills could take more of the
    // prompt than the conversation they exist to steer — and nothing would fail,
    // the model would just have less room and answer worse.
    let fitted = runtime_loop::skill_budget::fit_skill_blocks(
        scored
            .into_iter()
            .map(|(_, skill)| format!("## Skill: {}\n{}", skill.name, skill.content))
            .collect(),
    );
    if fitted.truncated > 0 || fitted.dropped > 0 {
        warn!(
            org_id,
            truncated = fitted.truncated,
            dropped = fitted.dropped,
            budget_chars = runtime_loop::skill_budget::SKILL_CONTEXT_BUDGET_CHARS,
            "skill guidance exceeded its context budget; degraded to fit"
        );
    }
    fitted.blocks
}

/// The governed ReAct-style round loop: offer tools → `Infer` → dispatch requested
/// tool calls through the gated [`runtime_loop::execute_step_with_subagent`]
/// path → feed the outcomes back → re-infer, until the model answers or the
/// budget is spent.
///
/// REENTRANT by design: [`run_subagent`] calls it again with `depth + 1` and a
/// fresh message history, so a delegated agent is this exact machinery — same
/// purpose-lock, same permission/hook/capability gates, same durable step
/// records — instead of a parallel implementation. Everything that must happen
/// exactly once per run (plan transitions, the managed terminal receipt, the
/// assistant message) stays in the caller.
///
/// The repair message for a tool call whose arguments do not match the schema the
/// model was shown, or `None` when there is nothing to say.
///
/// `offered` is the merged set this run advertised — builtin plus client-declared
/// plus MCP — which is the same list the purpose-lock admits from, so a call can
/// never be validated against a schema other than the one it was offered. A tool
/// absent from it is left alone; it will be refused by the purpose-lock anyway,
/// and inventing a schema for it would be guessing.
fn argument_problem(
    offered: &[pb::ToolDefinition],
    tool_name: &str,
    arguments_json: &str,
    conversation: &str,
) -> Option<String> {
    let def = offered.iter().find(|def| def.name == tool_name)?;
    let errors =
        mp_contracts::tool_arguments::validate_arguments(&def.parameters_json, arguments_json);
    if !errors.is_empty() {
        return Some(mp_contracts::tool_arguments::repair_message(
            tool_name,
            &errors,
            &def.parameters_json,
        ));
    }
    // Schema-shaped is not the same as true. A required value the user never
    // gave is well-formed and passes every check above it, which is exactly how
    // an invented postal code reached a live carrier. Checked second because a
    // malformed call should hear about its shape first.
    let ungrounded =
        mp_contracts::tool_arguments::ungrounded_arguments(tool_name, arguments_json, conversation);
    if !ungrounded.is_empty() {
        return Some(mp_contracts::tool_arguments::grounding_message(
            tool_name,
            &ungrounded,
        ));
    }
    None
}

/// The text a supplied argument may be grounded in.
///
/// Every turn **except** the system prompt. That exclusion is the whole point:
/// the measured fabrications included `from.name: "Verevon"`, a string that
/// appears only in the preamble, and tool descriptions carry example values that
/// would ground themselves. Grounding means "the user or a tool said this", and
/// the system prompt is neither.
fn grounding_conversation(messages: &[pb::ChatMessage]) -> String {
    messages
        .iter()
        .filter(|message| message.role != "system")
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Mirrors model-gateway's `run_tool_rounds`.
#[allow(clippy::too_many_lines)]
async fn run_rounds(
    ctx: &LoopContext<'_>,
    goal: &str,
    max_rounds: u32,
) -> Result<RoundsResult, tonic::Status> {
    let req = ctx.req;
    // The run's Zero-Data-Retention flag, threaded through every inference round
    // (so inference-core's prompt cache skips durable read/write) and onto each
    // tool step's GDPR audit detail. Sourced from `RunAgentRequest`, which the
    // gateway populates from the chat request's `zdr` flag — a nested loop
    // inherits it rather than deciding retention for itself.
    let zdr = req.zdr;

    // ISOLATION: a fresh history seeded only with THIS loop's goal. For a
    // subagent that is the point of delegating — a long sub-task's transcript
    // never reaches the parent's context, only its conclusion does.
    let mut messages = vec![
        pb::ChatMessage {
            role: "system".to_owned(),
            content: compose_system_prompt(&ctx.allowlist),
            name: String::new(),
        },
        pb::ChatMessage {
            role: "user".to_owned(),
            content: goal.to_owned(),
            name: String::new(),
        },
    ];

    // Skills: the same "does the model already know how to use this tool"
    // steering the inline chat loop gets (model-gateway's `fetch_skill_context`)
    // — arguably more load-bearing here, since RunAgent is where the
    // approval-gated, real-world-effect tools (book_shipment,
    // execute_provider_action, browser_agent, ...) actually run. Injected once
    // per run (not per round) as a single system message right after the
    // preamble, mirroring the inline loop's insertion point exactly.
    let skill_blocks =
        fetch_skill_context(ctx.session_channel, ctx.session_bearer, &req.org_id, goal).await;
    if !skill_blocks.is_empty() {
        messages.insert(
            1,
            pb::ChatMessage {
                role: "system".to_owned(),
                content: format!(
                    "You have access to the following skills relevant to this request. Apply their guidance when it fits:\n\n{}",
                    skill_blocks.join("\n\n")
                ),
                name: String::new(),
            },
        );
    }

    let mut inference = InferenceCoreClient::new(ctx.inference_channel.clone());
    let mut answer: Option<String> = None;
    let mut rounds_executed: u32 = 0;
    let mut delegated_rounds: u32 = 0;
    let mut step_seq: u32 = 0;
    let mut attempted_retrievals = BTreeSet::new();
    // Run-scoped, NOT round-scoped: the ceiling bounds how many delegations this
    // run opens in TOTAL, so three per round for four rounds is bounded exactly
    // like twelve at once. Deliberately independent of the round budget — see
    // `subagent::MAX_TOTAL_CHILDREN` on why one cannot express the other.
    let children_started = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    // Reported on the step outcome. Was a hardcoded `false` at every
    // construction site while being surfaced over gRPC — a dead-but-visible
    // field, the pattern §13.3 rejects.
    let mut compaction_triggered = false;
    // HONESTY_CONTRACT: true once any knowledge_search call in this run
    // actually returned org knowledge (JSON status "ok"), so the final
    // RunAgentResponse can report real grounding instead of a guess.
    let mut grounded = false;

    // A delegated loop's rounds are charged against the SAME budget, so a parent
    // that delegates on every round cannot drive max_rounds² inference calls.
    while rounds_executed + delegated_rounds < max_rounds {
        heartbeat_managed_agent_run(ctx.session_channel, req, ctx.terminal_tokens).await?;
        rounds_executed += 1;
        // Tier-1 compaction, in the loop that needed it most and did not have
        // it. A 12-round agentic run accumulates far more tool-result payload
        // than a chat turn, and this loop had NO compaction of any kind — an
        // overflowing run simply failed with a graceful apology
        // (`RoundsOutcome::InferFailed`).
        //
        // The budget and the notice are the chat loop's, not a second set:
        // `tests/compaction_parity_contract.rs` pins them equal, for the same
        // reason the skill budget is pinned — a deployed agent that compacts
        // differently from chat answers differently for reasons nobody can see.
        let cleared = crate::compaction_budget::clear_stale_tool_results(&mut messages);
        if cleared > 0 {
            compaction_triggered = true;
            info!(
                run_id = %req.run_id,
                depth = ctx.depth,
                cleared,
                carried_chars = crate::compaction_budget::tool_result_payload_chars(&messages),
                "run_agent: cleared stale tool-result payloads to stay within the context window"
            );
        }

        let mut infer_request = tonic::Request::new(pb::InferRequest {
            // No extended thinking on the governed agent loop: the effort
            // dial is a chat-surface control, and a deployed agent's budget
            // is its round budget.
            thinking_budget_tokens: 0,
            request_id: req.run_id.clone(),
            org_id: req.org_id.clone(),
            model: ctx.agent_model.clone(),
            provider_hint: String::new(),
            messages: messages.clone(),
            temperature: TEMPERATURE,
            max_tokens: MAX_TOKENS,
            structured_output_schema: String::new(),
            zdr,
            // PRIVACY FLOOR: the run's `min_privacy_tier` rides on EVERY
            // inference round, mirroring the ZDR flag above. A constrained chat
            // turn routed through the governed agent loop must enforce exactly
            // the same tier as the inline invoke path — inference-core skips
            // ineligible providers before any network call and fails closed.
            min_privacy_tier: req.min_privacy_tier,
            tools: ctx.tools.clone(),
            tool_choice: "auto".to_owned(),
            // No caller here has a residency floor to express yet; left for
            // a future org-policy wiring (see inference.proto's field doc).
            min_residency: String::new(),
            // Governed agent loops cannot select a user-owned ChatGPT
            // subscription; only an explicit user chat request may do so.
            subscription_connection_id: String::new(),
        });
        infer_request.metadata_mut().insert(
            "authorization",
            format!("Bearer {}", ctx.inference_bearer)
                .parse()
                .expect("verified compact JWT is valid gRPC metadata"),
        );
        let infer_result = inference.infer(infer_request).await;

        let response = match infer_result {
            Ok(r) => r.into_inner(),
            Err(error) => {
                warn!(
                    run_id = %req.run_id,
                    depth = ctx.depth,
                    error = %error,
                    "run_agent: inference failed; finalizing run as failed (graceful reply)"
                );
                return Ok(RoundsResult {
                    compaction_triggered,
                    outcome: RoundsOutcome::InferFailed,
                    rounds_executed,
                    delegated_rounds,
                    grounded,
                });
            }
        };

        // No tool calls → the model is ready to answer.
        if response.tool_calls.is_empty() {
            answer = Some(response.content);
            break;
        }

        // Dispatch each requested tool call through the gated execute_step path.
        //
        // Three phases, replacing what used to be one strictly sequential
        // `for call in &response.tool_calls` loop (harness-adoption pass,
        // claude-hermes-deepseek.md §7.2 — "not everything needs to run in
        // parallel, but independent calls should"):
        //
        // 1. Sequential pre-pass — purpose-lock and duplicate-retrieval
        //    rejection, and step_id assignment, stay a strict walk over the
        //    model's own array order. `attempted_retrievals.insert` and
        //    `step_seq` are both mutated here; running this phase concurrently
        //    would let a duplicate knowledge_search slip through or hand out a
        //    colliding step id.
        // 2. Concurrent dispatch — every surviving call goes through the SAME
        //    gated `execute_step_with_subagent` path as before, now in-flight
        //    together. The win is real because each call is dominated by I/O
        //    wait (capability/hook evaluation, the tool's own execution), not
        //    CPU work that would contend.
        // 3. Sequential outcome processing, in ORIGINAL array order — identical
        //    per-outcome handling to before (approval gating, step recording,
        //    grounding detection), so the audit trail and HITL behavior are
        //    unchanged in shape.
        //
        // Approval ordering, addressed directly: today's tie-break is still
        // "the first call, by array order, that needs approval" — Phase 3
        // preserves that by deferring the actual pause until after the full
        // walk (`pending_pause`), rather than returning the instant it's found.
        // That deferral matters because Phase 2 may have already run OTHER
        // calls in the batch for real by the time a pause is discovered; every
        // one of those still gets recorded via `record_tool_step` regardless of
        // its position relative to the pausing call, so a call that genuinely
        // executed is never missing from the run's own audit trail just
        // because a sibling elsewhere in the batch needed approval. Nothing
        // that requires approval ever performs its side effect early: the
        // `Ask` capability decision short-circuits `execute_step_inner` before
        // any tool body runs (see `runtime_loop::mod`'s dispatch gate), so
        // concurrency cannot let a gated action slip through unapproved
        // regardless of dispatch order.
        // A round cut off at the output ceiling leaves its LAST tool call
        // half-written: the provider returns the partial block with whatever
        // argument keys it managed to emit, and nothing downstream can tell
        // that apart from a call the model finished. Dispatching it anyway
        // means acting on arguments that are an accident of where the ceiling
        // fell. Mirrors model-gateway's inline loop exactly — including its
        // insight that only the FINAL call can be partial, so earlier calls in
        // the same round are complete and still run.
        let truncated_index = runtime_loop::retry::truncated_tool_call_index(
            &response.stop_reason,
            response.tool_calls.len(),
        );
        if truncated_index.is_some() {
            warn!(
                run_id = %req.run_id,
                stop_reason = %response.stop_reason,
                tool = response.tool_calls.last().map_or("", |call| call.name.as_str()),
                "run_agent: round hit the output token ceiling; the last tool call's arguments are truncated"
            );
        }

        let mut slots = Vec::with_capacity(response.tool_calls.len());
        for (call_index, call) in response.tool_calls.iter().enumerate() {
            // Refused BEFORE the purpose-lock and duplicate checks: a
            // truncated call's name and arguments are both unreliable, so
            // classifying it on either would be reading noise. Reported as a
            // tool error rather than skipped silently — the model reads tool
            // errors and retries, and the user sees a failed step instead of
            // watching a result never arrive.
            if truncated_index == Some(call_index) {
                slots.push(CallSlot::Rejected(ToolStepResult {
                    name: call.name.clone(),
                    output: String::new(),
                    error: Some(format!(
                        "tool '{}' was not run: this round hit the output token limit, so its \
                         arguments are truncated and cannot be trusted. Re-issue the call with \
                         complete arguments — ideally as the only call in the next round.",
                        call.name
                    )),
                }));
                continue;
            }
            // Purpose-lock: reject any tool not in the offered allowlist.
            if !ctx.allowlist.contains(&call.name) {
                warn!(
                    run_id = %req.run_id,
                    tool = %call.name,
                    "run_agent: rejecting un-offered tool (purpose-lock)"
                );
                slots.push(CallSlot::Rejected(ToolStepResult {
                    name: call.name.clone(),
                    output: String::new(),
                    error: Some(format!(
                        "tool '{}' is not in this agent's allowed scope",
                        call.name
                    )),
                }));
                continue;
            }

            // Leaf/orchestrator role split (harness-adoption pass,
            // claude-hermes-deepseek.md §7.3): a delegated subagent (depth > 0)
            // may never attempt a risky/side-effecting tool, full stop —
            // regardless of the run's own permission mode. This closes a real
            // inconsistency, not a hypothetical one: a subagent already cannot
            // request durable human approval (see the `awaiting_approval`
            // refusal below), yet under `auto` mode it could otherwise run
            // ANY tool, including the destructive ones `ask` mode would gate
            // for the top-level run — no oversight AND a less-audited context
            // is the worst combination, not a safe one. Delegation itself
            // (`subagent.*`) is exempt: it is a control tool, not an action,
            // and already has its own tailored refusal (`guard_depth`, fired
            // deeper in the dispatch, with a message aimed at the model).
            // `MAX_DEPTH == 1` means there is no depth-2+ "orchestrator" role
            // to carve an exception for; the depth-0 run IS the orchestrator.
            if ctx.depth > 0
                && !crate::subagent::is_subagent_tool(&call.name)
                && (crate::permission::is_risky_call(&call.name, &call.arguments_json)
                    || crate::permission::is_restricted_context_write(&call.name))
            {
                warn!(
                    run_id = %req.run_id,
                    depth = ctx.depth,
                    tool = %call.name,
                    "run_agent: rejecting risky tool for a delegated subagent (leaf role blocklist)"
                );
                slots.push(CallSlot::Rejected(ToolStepResult {
                    name: call.name.clone(),
                    output: String::new(),
                    error: Some(format!(
                        "tool '{}' is a side-effecting/risky action, which a delegated subagent may \
                         never run regardless of the run's permission mode; report that this step \
                         needs the main agent to run it directly",
                        call.name
                    )),
                }));
                continue;
            }

            // The graded autonomy rung, checked PER CALL with the actual
            // arguments (`permission::check_autonomy_rung`).
            //
            // This sits ALONGSIDE plan mode rather than replacing it, and the
            // order matters for what the model reads: plan mode's refusal below
            // says "this run may only investigate", which is the more specific
            // and more actionable sentence when both apply. This one covers the
            // states plan mode cannot express — a run granted `workspace_write`
            // by an approved plan may write its report and still not send,
            // publish, book or pay.
            //
            // A run with no stated rung is unaffected: `check_autonomy_rung`
            // returns `Ok` for `UNSPECIFIED`, so a caller that predates the
            // ladder behaves exactly as it did.
            //
            // Checked regardless of plan mode. This used to be gated on
            // `!req.plan_mode` — and the only production producer set a rung
            // exactly when plan_mode was true, so the one rung that could
            // refuse was set precisely when the check was skipped, and the
            // gate could never refuse anything. Plan mode's own refusal below
            // is stricter where both apply; running both costs one enum
            // comparison and removes the masking.
            if !crate::subagent::is_subagent_tool(&call.name) {
                let granted = pb::AutonomyRung::try_from(req.autonomy_rung)
                    .unwrap_or(pb::AutonomyRung::Unspecified);
                if let Err(refusal) = crate::permission::check_autonomy_rung(
                    granted,
                    &call.name,
                    &call.arguments_json,
                ) {
                    warn!(
                        run_id = %req.run_id,
                        tool = %call.name,
                        granted = mp_contracts::autonomy::label(granted),
                        "run_agent: rejecting a tool the run's autonomy rung does not cover"
                    );
                    slots.push(CallSlot::Rejected(ToolStepResult {
                        name: call.name.clone(),
                        output: String::new(),
                        error: Some(refusal),
                    }));
                    continue;
                }
            }

            // Arguments checked against the tool's OWN declared schema before
            // dispatch (`mp_contracts::tool_arguments`), in the same pre-pass as
            // the purpose-lock and the gates below.
            //
            // Wired here as well as in the chat loop deliberately: a validator
            // only one loop runs is how a deployed agent starts accepting
            // arguments chat would have refused. The schema source is the merged
            // offered set — the same list the purpose-lock uses — so what is
            // validated against is exactly what the model was shown.
            //
            // Fails OPEN by construction (see the module docs): no opinion on an
            // unusual schema, an undeclared field, or a coercion executors accept.
            // It cannot refuse a call that would have worked.
            if let Some(problem) = argument_problem(
                &ctx.tools,
                &call.name,
                &call.arguments_json,
                &grounding_conversation(&messages),
            ) {
                warn!(
                    run_id = %req.run_id,
                    tool = %call.name,
                    "run_agent: rejecting a tool call whose arguments do not match its schema"
                );
                slots.push(CallSlot::Rejected(ToolStepResult {
                    name: call.name.clone(),
                    output: String::new(),
                    error: Some(problem),
                }));
                continue;
            }

            // Delegation-record reads belong to the run that did the
            // delegating. A delegated subagent cannot delegate further
            // (`MAX_DEPTH == 1`), so it has no children — an empty list would
            // read to it as "my delegations found nothing", which is a
            // fabrication about work it never did. Refused with its reason for
            // the same reason the leaf blocklist above states its own.
            if ctx.depth > 0
                && crate::runtime_loop::subagent_results::is_main_agent_only(&call.name)
            {
                slots.push(CallSlot::Rejected(ToolStepResult {
                    name: call.name.clone(),
                    output: String::new(),
                    error: Some(
                        crate::runtime_loop::subagent_results::main_agent_only_refusal(&call.name),
                    ),
                }));
                continue;
            }

            // Server-side plan-mode enforcement (harness-adoption pass,
            // claude-hermes-deepseek.md §13.5 item 3 — the leaked Claude Code
            // hole this avoids: plan mode enforced ONLY by a re-injected
            // system-prompt reminder, with a bypass-capable context able to
            // auto-allow every tool while still telling the model it is
            // read-only). `req.plan_mode` used to reach no enforcement point
            // at all: it was tracked purely as a durable status flag
            // (session-core `run.mode` / `PlanModeStore`) for the UI to query,
            // with a doc comment in model-gateway's coordinator.rs promising a
            // "tool-dispatch middleware" that consulted it — a promise no code
            // ever kept. A run a human believed was "planning" could still
            // execute a real action if the run's own `mode` was `auto`. Reuses
            // the SAME classifier and pattern as the leaf-role blocklist
            // above: read-only tools still run, `subagent.*` is exempt (a
            // delegated investigation is not an action either), and a
            // delegated loop inherits `req` verbatim, so plan mode propagates
            // to every subagent automatically with no extra wiring.
            if req.plan_mode
                && !crate::subagent::is_subagent_tool(&call.name)
                && (crate::permission::is_risky_call(&call.name, &call.arguments_json)
                    || crate::permission::is_restricted_context_write(&call.name))
            {
                warn!(
                    run_id = %req.run_id,
                    depth = ctx.depth,
                    tool = %call.name,
                    "run_agent: rejecting risky tool while the run is in plan mode"
                );
                slots.push(CallSlot::Rejected(ToolStepResult {
                    name: call.name.clone(),
                    output: String::new(),
                    error: Some(format!(
                        "tool '{}' is a side-effecting/risky action; this run is in plan mode and \
                         may only investigate, never act. Describe what you would do instead of \
                         doing it.",
                        call.name
                    )),
                }));
                continue;
            }

            // Agentic retrieval may reformulate/backtrack across rounds, but an
            // exact repeat cannot add evidence and can burn the entire budget.
            // Suppress only valid, canonical knowledge-search duplicates;
            // changed queries or top_k values remain eligible and still flow
            // through the same capability/permission policy at dispatch.
            if let Some(signature) = retrieval_signature(&call.name, &call.arguments_json) {
                if !attempted_retrievals.insert(signature) {
                    slots.push(CallSlot::Rejected(ToolStepResult {
                        name: call.name.clone(),
                        output: String::new(),
                        error: Some(
                            "duplicate knowledge_search suppressed; reformulate the query with materially different terms before retrying"
                                .to_owned(),
                        ),
                    }));
                    continue;
                }
            }

            // Stable per-(run, step) id computed BEFORE dispatch so it is shared
            // by the gate, the durable approval binding (a provider write forwards
            // this step's real approval id), and the audit step record.
            step_seq += 1;
            let step_id = tool_step_id(&ctx.step_prefix, step_seq, call);
            slots.push(CallSlot::ToDispatch { call, step_id });
        }

        // Phase 2: concurrent dispatch. One shared budget pool for every
        // delegation-capable call in this batch — see `LoopSubagentDispatch`.
        let batch_budget = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(
            max_rounds.saturating_sub(rounds_executed + delegated_rounds),
        ));
        let outcomes_by_slot: Vec<Option<runtime_loop::StepOutcome>> = {
            let dispatches: Vec<Option<LoopSubagentDispatch<'_, '_>>> = slots
                .iter()
                .map(|slot| match slot {
                    CallSlot::ToDispatch { step_id, .. } => Some(LoopSubagentDispatch::new(
                        ctx,
                        step_id,
                        std::sync::Arc::clone(&batch_budget),
                        std::sync::Arc::clone(&children_started),
                    )),
                    CallSlot::Rejected(_) => None,
                })
                .collect();

            let results = futures::future::join_all(slots.iter().zip(dispatches.iter()).map(
                |(slot, dispatch)| async move {
                    let CallSlot::ToDispatch { call, step_id } = slot else {
                        return None;
                    };
                    let dispatch = dispatch
                        .as_ref()
                        .expect("every ToDispatch slot has a matching dispatch");
                    // Bounded transient-failure retry (harness-adoption pass:
                    // the one capability every audited harness had and we had
                    // on neither loop). `retry::tool_call_is_retryable` refuses
                    // outright for any side-effecting tool — a timeout is
                    // ambiguous, so replaying a write is how one booking
                    // becomes two — and for any non-transient failure, where a
                    // second identical attempt can only fail identically.
                    // Present only when this run is Space-scoped AND the call
                    // about to dispatch is code_interpreter — the only tool
                    // that reads it. Built fresh per call rather than once per
                    // round: `ctx.req.space_id`/`ctx.sandbox_bearer` never
                    // change within a run, but constructing the borrow only
                    // where it is used keeps the "absent unless relevant"
                    // invariant visible at the one call site that cares.
                    let sandbox = if call.name == "code_interpreter" && !ctx.req.space_id.is_empty()
                    {
                        ctx.sandbox_bearer.map(|sandbox_bearer| {
                            crate::sandbox_lease::SandboxLeaseContext {
                                space_id: &ctx.req.space_id,
                                sandbox_bearer,
                                capability_client: ctx.capability_client,
                                sandbox_manager_client: ctx.sandbox_manager_client,
                                backend_id: ctx.backend_id,
                                cas_client: ctx.cas_client,
                                sandbox_tokens: ctx.sandbox_tokens,
                            }
                        })
                    } else {
                        None
                    };
                    let mut outcome = None;
                    for attempt in 1..=runtime_loop::retry::MAX_TOOL_ATTEMPTS {
                        if attempt > 1 {
                            tokio::time::sleep(runtime_loop::retry::backoff_before_attempt(
                                attempt,
                            ))
                            .await;
                        }
                        let result = runtime_loop::execute_step_with_subagent(
                            &call.name,
                            &call.arguments_json,
                            ctx.permission_wire,
                            "",
                            &req.org_id,
                            &req.user_id,
                            &req.run_id,
                            step_id,
                            &req.thread_id,
                            Some(ctx.session_channel.clone()),
                            None,
                            None,
                            zdr,
                            req.min_privacy_tier,
                            ctx.data_plane_bearer,
                            ctx.session_bearer,
                            Some(ctx.inference_bearer),
                            ctx.capability_policy,
                            Some(dispatch),
                            sandbox.as_ref(),
                        )
                        .await;
                        let retryable = attempt < runtime_loop::retry::MAX_TOOL_ATTEMPTS
                            && runtime_loop::retry::tool_call_is_retryable(
                                &call.name,
                                &call.arguments_json,
                                &result.error,
                            );
                        if !retryable {
                            outcome = Some(result);
                            break;
                        }
                        warn!(
                            run_id = %req.run_id,
                            tool = %call.name,
                            attempt,
                            error = %result.error,
                            "run_agent: retrying tool after a transient failure"
                        );
                        outcome = Some(result);
                    }
                    outcome
                },
            ))
            .await;

            for dispatch in dispatches.iter().flatten() {
                delegated_rounds = delegated_rounds.saturating_add(dispatch.rounds_consumed());
            }
            results
        };

        // Phase 3: sequential outcome processing, in original order.
        let mut outcomes = Vec::with_capacity(response.tool_calls.len());
        let mut pending_pause: Option<PendingPause> = None;

        for (slot, outcome) in slots.into_iter().zip(outcomes_by_slot) {
            let (call, step_id, outcome) = match (slot, outcome) {
                (CallSlot::Rejected(result), _) => {
                    outcomes.push(result);
                    continue;
                }
                (CallSlot::ToDispatch { call, step_id }, Some(outcome)) => (call, step_id, outcome),
                (CallSlot::ToDispatch { .. }, None) => {
                    unreachable!("every ToDispatch slot produced an outcome in phase 2")
                }
            };

            // HITL: a gated tool may be reported as paused only after the
            // durable approval write succeeds. Otherwise propagate an explicit
            // unavailable error without a false AwaitingApproval state/event.
            if outcome.status == "awaiting_approval" {
                if ctx.depth == 0 {
                    // First one (by array order) wins the durable pause, matching
                    // the pre-parallel-dispatch tie-break exactly. A second
                    // approval-needing call in the same batch never ran its side
                    // effect either (the `Ask` decision short-circuits before
                    // that), so it is safe to simply not queue it here — the
                    // model can ask for it again once the run resumes, same as
                    // it would today with any call after the first pause.
                    if pending_pause.is_none() {
                        pending_pause = Some(PendingPause {
                            step_id,
                            name: call.name.clone(),
                            arguments_json: call.arguments_json.clone(),
                        });
                    } else {
                        outcomes.push(ToolStepResult {
                            name: call.name.clone(),
                            output: String::new(),
                            error: Some(format!(
                                "tool '{}' also requires human approval; only one approval can be \
                                 queued per turn, so this one was not queued and can be requested \
                                 again once the run resumes",
                                call.name
                            )),
                        });
                    }
                    continue;
                }
                // A delegated loop cannot own the run's pause: the approval and
                // its resume belong to the run, and resume replays the parent
                // from its goal — which would discard this subagent's progress
                // and re-pause here forever. Refuse honestly instead; the tool
                // did NOT run, and the subagent can adapt or report back.
                warn!(
                    run_id = %req.run_id,
                    depth = ctx.depth,
                    tool = %call.name,
                    "run_agent: approval-gated tool refused inside a delegated subagent"
                );
                outcomes.push(ToolStepResult {
                    name: call.name.clone(),
                    output: String::new(),
                    error: Some(format!(
                        "tool '{}' requires human approval, which a delegated subagent cannot \
                         request; report that this step needs the main agent to run it directly",
                        call.name
                    )),
                });
                continue;
            }

            // Non-terminal per-tool step (status "running") carrying the GDPR
            // audit detail. The managed lifecycle, not a step payload, owns
            // terminalization. A delegated loop records through the SAME path,
            // so its work shows up in the Agent Run Console under the step id of
            // the call that delegated it.
            record_tool_step(
                ctx.session_channel,
                &req.run_id,
                &step_id,
                &call.name,
                &outcome,
                zdr,
                ctx.session_bearer,
            )
            .await;

            if call.name == "knowledge_search"
                && outcome.error.is_empty()
                && knowledge_search_found_grounding(&outcome.output)
            {
                grounded = true;
            }

            outcomes.push(ToolStepResult {
                name: call.name.clone(),
                output: outcome.output,
                error: if outcome.error.is_empty() {
                    None
                } else {
                    Some(outcome.error)
                },
            });
        }

        if let Some(pending) = pending_pause {
            let paused = pause_for_approval(
                ctx.state,
                ctx.session_channel,
                req,
                &pending.step_id,
                &pending.name,
                &pending.arguments_json,
                ctx.permission_wire,
                rounds_executed.saturating_add(delegated_rounds),
                ctx.session_bearer,
            )
            .await?;
            return Ok(RoundsResult {
                compaction_triggered,
                outcome: RoundsOutcome::Paused(paused),
                rounds_executed,
                delegated_rounds,
                grounded,
            });
        }

        // Carry the model's interim reasoning forward, then append the framed
        // tool outcomes so the next round can answer from them.
        if !response.content.trim().is_empty() {
            messages.push(pb::ChatMessage {
                role: "assistant".to_owned(),
                content: response.content,
                name: String::new(),
            });
        }
        messages.push(pb::ChatMessage {
            role: "user".to_owned(),
            content: format_tool_context(&outcomes),
            name: String::new(),
        });
    }

    let outcome = if let Some(text) = answer {
        RoundsOutcome::Answered(text)
    } else {
        warn!(
            run_id = %req.run_id,
            depth = ctx.depth,
            rounds_executed,
            delegated_rounds,
            "run_agent: round budget exhausted without a final answer"
        );
        RoundsOutcome::Exhausted
    };
    Ok(RoundsResult {
        compaction_triggered,
        outcome,
        rounds_executed,
        delegated_rounds,
        grounded,
    })
}

/// Bridges `execute_step`'s gated dispatch back into the loop for one
/// `subagent.*` call.
///
/// Constructed per tool call, but sharing ONE budget pool with every other
/// call dispatched in the same round (see `run_rounds`'s concurrent dispatch
/// phase) — never a fixed private snapshot. When a round's tool calls run
/// concurrently, two sibling calls that BOTH decide to delegate must draw
/// from the SAME remaining allowance instead of each independently seeing
/// the full pre-round remainder and, combined, overspending it. Each records
/// what it actually spent — the parent charges those rounds to the run so
/// repeated delegation shrinks the budget instead of resetting it.
struct LoopSubagentDispatch<'a, 'b> {
    parent: &'a LoopContext<'b>,
    parent_step_id: &'a str,
    shared_rounds_remaining: std::sync::Arc<std::sync::atomic::AtomicU32>,
    /// Delegations this RUN has started, across every round. Separate from
    /// `shared_rounds_remaining` on purpose — see
    /// `subagent::MAX_TOTAL_CHILDREN`: one bounds work, the other bounds
    /// fan-out, and a run can hit either without the other.
    children_started: std::sync::Arc<std::sync::atomic::AtomicU32>,
    consumed: std::sync::atomic::AtomicU32,
}

impl<'a, 'b> LoopSubagentDispatch<'a, 'b> {
    fn new(
        parent: &'a LoopContext<'b>,
        parent_step_id: &'a str,
        shared_rounds_remaining: std::sync::Arc<std::sync::atomic::AtomicU32>,
        children_started: std::sync::Arc<std::sync::atomic::AtomicU32>,
    ) -> Self {
        Self {
            parent,
            parent_step_id,
            shared_rounds_remaining,
            children_started,
            consumed: std::sync::atomic::AtomicU32::new(0),
        }
    }

    fn rounds_consumed(&self) -> u32 {
        self.consumed.load(std::sync::atomic::Ordering::Relaxed)
    }
}

#[tonic::async_trait]
impl crate::subagent::SubagentDispatch for LoopSubagentDispatch<'_, '_> {
    async fn spawn(&self, tool_name: &str, tool_input: &str) -> Result<String, String> {
        // Exclusive claim, not a load: `swap` atomically takes the ENTIRE
        // remaining pool and leaves 0 behind, in one RMW with no gap a
        // concurrent sibling could land in. A plain load-then-settle-later
        // was tried first and does NOT work here — two siblings dispatched
        // together both reach this point before either one's nested
        // `run_rounds` (a real inference round-trip) resolves, so "settle
        // after running" always settles too late. Whichever call's `spawn`
        // executes first empties the pool for every later one this round;
        // ties are broken by dispatch order, not "fairly" split, because we
        // cannot know ahead of time which offered calls will even attempt to
        // delegate. Verified by
        // `two_concurrent_delegations_in_one_round_share_the_same_budget_pool`.
        // Claim a child slot BEFORE the round pool. Order matters: the pool
        // claim is a `swap(0)` that empties it for every sibling, so refusing
        // after it would report "no budget left" for a delegation that was
        // actually refused for fan-out — and would strand the pool as well.
        //
        // `fetch_add` returns the value BEFORE the increment, so concurrent
        // siblings each get a distinct index and exactly
        // `MAX_TOTAL_CHILDREN` of them win. A refused claim is not given back:
        // handing the slot back would let an unbounded number of refused
        // attempts keep retrying into the same slot within one round.
        let claimed = self
            .children_started
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if let Err(refusal) = crate::subagent::guard_child_ceiling(claimed) {
            warn!(
                run_id = %self.parent.req.run_id,
                started = claimed,
                limit = crate::subagent::MAX_TOTAL_CHILDREN,
                "run_agent: delegation refused by the per-run child ceiling"
            );
            return Err(refusal);
        }
        let ceiling = self
            .shared_rounds_remaining
            .swap(0, std::sync::atomic::Ordering::Relaxed);
        let (result, rounds) = run_subagent(
            self.parent,
            self.parent_step_id,
            ceiling,
            tool_name,
            tool_input,
        )
        .await;
        self.consumed
            .fetch_add(rounds, std::sync::atomic::Ordering::Relaxed);
        result
    }
}

/// Delegation role recorded on the lineage edge. Every in-loop subagent is
/// `GENERIC` today: the narrower `SubagentRole` values (coder, reviewer,
/// researcher, explorer) belong to the orchestration surface, where a caller
/// declares the role. Deriving one from a free-text tool label — `subagent.foo`
/// → "researcher"? — would be guessing at semantics the model never stated, and
/// a wrong role on a lineage graph is worse than an honest unspecified one.
const SUBAGENT_LINEAGE_ROLE: pb::SubagentRole = pb::SubagentRole::Generic;

/// Mode recorded on a delegated child run.
const SUBAGENT_RUN_MODE: &str = "execute";

/// Register a delegation as a durable child run and record the lineage edge.
///
/// # Why the live path needed this
///
/// `subagent_edges` and `GetSubagentLineage` were real and served, but ONLY the
/// orchestration surface ever wrote to them. In-loop delegation ran the child
/// under the parent's own `run_id` with prefixed step ids, so for every subagent
/// a real user actually triggered, the lineage endpoint returned nothing. The
/// graph existed; the live path was not in it.
///
/// # Idempotency
///
/// `start_key` is `<parent_run_id>:<parent_step_id>` — stable across retries of
/// the same delegating tool call, and derived from identifiers only. The
/// contract requires it not be derived from prompt or tool content, which also
/// means it is safe to log. A replayed start returns `already_started`, so a
/// re-attempted parent step reuses its child run instead of minting a second
/// one and forking the lineage.
///
/// # Failure posture
///
/// Returns the child run id, or `None` on any failure — the delegation then
/// proceeds unrecorded. The
/// subagent's work is what the user asked for; losing its lineage row is a
/// bookkeeping regression, not a reason to refuse the task. Every failure is a
/// warning with the parent run and step, so an unrecorded delegation is
/// diagnosable rather than invisible.
/// A delegation's durable child run, and whether this call created it.
struct DelegatedChildRun {
    run_id: String,
    /// True when `StartManagedRun` replayed an existing run for this
    /// `start_key`. The delegation has already happened once, so its recorded
    /// outcome is the truth — see `resume_delegated_child_run`.
    already_started: bool,
}

async fn register_delegated_child_run(
    parent: &LoopContext<'_>,
    parent_step_id: &str,
    label: &str,
    goal: &str,
) -> Option<DelegatedChildRun> {
    let token = match parent
        .terminal_tokens
        .terminalize_token(&parent.req.org_id)
        .await
    {
        Ok(token) => token,
        Err(error) => {
            warn!(
                run_id = %parent.req.run_id,
                step_id = parent_step_id,
                %error,
                "run_agent: no managed credential to register the delegated child run; \
                 delegation proceeds without a lineage record"
            );
            return None;
        }
    };

    let start_key = format!("{}:{}", parent.req.run_id, parent_step_id);
    let request = match authenticated_session_request(
        pb::StartManagedRunRequest {
            thread_id: parent.req.thread_id.clone(),
            parent_run_id: parent.req.run_id.clone(),
            agent_id: label.to_owned(),
            goal: goal.to_owned(),
            mode: SUBAGENT_RUN_MODE.to_owned(),
            org_id: parent.req.org_id.clone(),
            user_id: parent.req.user_id.clone(),
            start_key: start_key.clone(),
            terminal_source: pb::ManagedRunSource::ExecutionAgent as i32,
        },
        Some(&token),
    ) {
        Ok(request) => request,
        Err(error) => {
            warn!(run_id = %parent.req.run_id, %error, "run_agent: child-run request not forwardable");
            return None;
        }
    };

    let started = match ManagedRunLifecycleClient::new(parent.session_channel.clone())
        .start_managed_run(request)
        .await
    {
        Ok(response) => response.into_inner(),
        Err(error) => {
            warn!(
                run_id = %parent.req.run_id,
                step_id = parent_step_id,
                code = ?error.code(),
                "run_agent: could not register the delegated child run; delegation \
                 proceeds without a lineage record"
            );
            return None;
        }
    };

    // The edge is recorded separately and best-effort: `runs.parent_run_id` and
    // `subagent_edges` are two representations of the same relation, and the
    // lineage endpoint reads only the latter. A child run without its edge is
    // still durable and still terminalized; it is just missing from the graph.
    match authenticated_session_request(
        pb::AttachSubagentRequest {
            thread_id: parent.req.thread_id.clone(),
            parent_run_id: parent.req.run_id.clone(),
            child_run_id: started.run_id.clone(),
            role: SUBAGENT_LINEAGE_ROLE as i32,
        },
        parent.session_bearer,
    ) {
        Ok(request) => {
            // `AttachSubagent` lives on OrchestrationCoreService, which
            // session-core also serves — same channel, different stub.
            if let Err(error) = OrchestrationCoreServiceClient::new(parent.session_channel.clone())
                .attach_subagent(request)
                .await
            {
                // An `already_started` replay re-attaches the same edge, which
                // the composite primary key rejects. Expected, not a fault.
                if !started.already_started {
                    warn!(
                        run_id = %parent.req.run_id,
                        child_run_id = %started.run_id,
                        code = ?error.code(),
                        "run_agent: delegated child run registered but its lineage edge was not"
                    );
                }
            }
        }
        Err(error) => warn!(
            run_id = %parent.req.run_id,
            %error,
            "run_agent: lineage-edge request not forwardable"
        ),
    }

    info!(
        run_id = %parent.req.run_id,
        child_run_id = %started.run_id,
        subagent = label,
        already_started = started.already_started,
        "run_agent: delegation registered as a durable child run"
    );
    Some(DelegatedChildRun {
        run_id: started.run_id,
        already_started: started.already_started,
    })
}

/// What a replayed delegation should return instead of running again.
enum ReplayedDelegation {
    /// The earlier attempt concluded and its answer is on the record.
    Answer(String),
    /// The earlier attempt is finished but there is nothing to hand back, or it
    /// is still in flight. Either way, re-running is wrong.
    Refused(String),
}

/// Honour a replayed delegation's recorded outcome instead of re-running it.
///
/// # Why a replay must not re-run
///
/// `start_key` is `<parent_run_id>:<parent_step_id>` — identifiers only — so a
/// re-driven parent step (an approval resume, a redelivered request) reuses its
/// child run rather than forking the lineage. That is the right identity
/// behaviour, and it makes re-running the loop actively wrong: the child run
/// already carries an **immutable** terminal receipt, so a second
/// `RecordTerminalOutcome` returns the FIRST outcome. A replay that re-ran and
/// succeeded would hand the parent a good answer while the ledger kept saying
/// the child failed — and the ledger is what people audit.
///
/// So the recorded outcome is the answer, including when the recorded outcome is
/// "it failed". A delegation is not idempotent at the run level, and pretending
/// otherwise is what produces two truths.
async fn resume_delegated_child_run(
    parent: &LoopContext<'_>,
    child_run_id: &str,
    label: &str,
) -> ReplayedDelegation {
    use mp_contracts::model_plane::v1::{run_service_client::RunServiceClient, GetRunRequest};

    let request = match authenticated_session_request(
        GetRunRequest {
            run_id: child_run_id.to_owned(),
        },
        parent.session_bearer,
    ) {
        Ok(request) => request,
        Err(_) => {
            return ReplayedDelegation::Refused(format!(
                "subagent '{label}' already ran once for this step and its result could not be \
                 read back (no forwardable credential). It is not re-run, because its recorded \
                 outcome is the one that counts."
            ))
        }
    };
    let detail = match RunServiceClient::new(parent.session_channel.clone())
        .get_run(request)
        .await
    {
        Ok(response) => response.into_inner(),
        Err(error) => {
            warn!(
                child_run_id,
                code = ?error.code(),
                "run_agent: replayed delegation could not be read back"
            );
            return ReplayedDelegation::Refused(format!(
                "subagent '{label}' already ran once for this step and its result could not be \
                 read back right now. It is not re-run — that would produce an answer the run \
                 ledger contradicts."
            ));
        }
    };

    let terminal = matches!(detail.status.as_str(), "completed" | "failed" | "cancelled");
    let answer = detail.final_output.trim();
    if terminal && !answer.is_empty() {
        info!(
            child_run_id,
            subagent = label,
            "run_agent: replayed delegation resumed from its recorded answer without re-running"
        );
        return ReplayedDelegation::Answer(answer.to_owned());
    }
    if terminal {
        return ReplayedDelegation::Refused(format!(
            "subagent '{label}' already ran once for this step and finished {} without a \
             recorded conclusion. Do this part yourself rather than delegating it again.",
            detail.status
        ));
    }
    ReplayedDelegation::Refused(format!(
        "subagent '{label}' for this step is still {} from an earlier attempt. Wait for it or do \
         the work yourself; starting it again would fork one delegation into two.",
        detail.status
    ))
}

/// Settle a delegated child run's managed obligation.
///
/// A managed run is born with a terminalization obligation carrying a deadline;
/// leaving one unsettled means a watchdog eventually force-fails it. So this
/// runs on EVERY delegation outcome, including refusals and failures — the
/// child run must reach a terminal state by the same path that created it.
async fn settle_delegated_child_run(parent: &LoopContext<'_>, child_run_id: &str, success: bool) {
    let token = match parent
        .terminal_tokens
        .terminalize_token(&parent.req.org_id)
        .await
    {
        Ok(token) => token,
        Err(error) => {
            warn!(child_run_id, %error, "run_agent: no credential to settle the delegated child run");
            return;
        }
    };
    let request = match authenticated_session_request(
        pb::RecordTerminalOutcomeRequest {
            run_id: child_run_id.to_owned(),
            source: pb::ManagedRunSource::ExecutionAgent as i32,
            outcome: if success {
                pb::TerminalOutcome::Completed as i32
            } else {
                pb::TerminalOutcome::Failed as i32
            },
            failure_code: if success {
                String::new()
            } else {
                "subagent_failed".to_owned()
            },
        },
        Some(&token),
    ) {
        Ok(request) => request,
        Err(error) => {
            warn!(child_run_id, %error, "run_agent: child terminal receipt not forwardable");
            return;
        }
    };
    if let Err(error) = ManagedRunLifecycleClient::new(parent.session_channel.clone())
        .record_terminal_outcome(request)
        .await
    {
        warn!(
            child_run_id,
            code = ?error.code(),
            "run_agent: delegated child run left unsettled; the managed deadline \
             watchdog will terminalize it"
        );
    }
}

/// Run ONE delegated subagent to a final answer by re-entering [`run_rounds`],
/// and return that answer as the delegating tool call's output.
///
/// Returns the rounds consumed alongside the result — including on failure —
/// because the parent must charge them either way.
///
/// Every refusal path returns an explicit error the parent model can act on. The
/// alternative (what the old stub did) is to hand back a success for work that
/// never happened, which the parent then presents to the user as done.
async fn run_subagent(
    parent: &LoopContext<'_>,
    parent_step_id: &str,
    rounds_remaining: u32,
    tool_name: &str,
    tool_input: &str,
) -> (Result<String, String>, u32) {
    let label = crate::subagent::label(tool_name);
    if let Err(refusal) = crate::subagent::guard_depth(parent.depth) {
        warn!(
            run_id = %parent.req.run_id,
            depth = parent.depth,
            subagent = %label,
            "run_agent: subagent spawn refused by the recursion guard"
        );
        return (Err(refusal), 0);
    }
    let task = match crate::subagent::parse_task(tool_name, tool_input) {
        Ok(task) => task,
        Err(error) => return (Err(error), 0),
    };
    let budget = match crate::subagent::resolve_round_budget(rounds_remaining, task.max_rounds) {
        Ok(budget) => budget,
        Err(error) => return (Err(error), 0),
    };

    // Register the delegation as a durable child run BEFORE any work happens,
    // so a lineage row exists even for a delegation that then fails. The child
    // loop still executes under the parent's `run_id` (below): moving step
    // attribution to the child run would relocate delegated steps out from
    // under the prefix the Agent Run Console follows, which is a console-visible
    // change and a separate decision. What the child run carries is the
    // delegation's own identity, goal, parent edge and terminal outcome.
    let delegation = register_delegated_child_run(parent, parent_step_id, label, &task.goal).await;

    // A replay is a delegation that already happened. Its recorded outcome is
    // the truth, and re-running it would fork the answer from the immutable
    // receipt its child run already carries. Zero rounds are charged: no work is
    // redone, which is the whole point.
    if let Some(child) = delegation.as_ref().filter(|child| child.already_started) {
        return match resume_delegated_child_run(parent, &child.run_id, label).await {
            ReplayedDelegation::Answer(answer) => (Ok(answer), 0),
            ReplayedDelegation::Refused(reason) => (Err(reason), 0),
        };
    }

    // The child inherits the run's identity, credentials, tools, posture and ZDR
    // flag verbatim (`req` and the bearers are passed through, never re-derived
    // from the tool JSON), so tenant isolation is exactly the parent's. What it
    // does NOT inherit is the parent's transcript.
    let child = LoopContext {
        state: parent.state,
        session_channel: parent.session_channel,
        inference_channel: parent.inference_channel,
        req: parent.req,
        tools: parent.tools.clone(),
        allowlist: parent.allowlist.clone(),
        data_plane_bearer: parent.data_plane_bearer,
        session_bearer: parent.session_bearer,
        inference_bearer: parent.inference_bearer,
        capability_policy: parent.capability_policy,
        terminal_tokens: parent.terminal_tokens,
        sandbox_bearer: parent.sandbox_bearer,
        capability_client: parent.capability_client,
        sandbox_manager_client: parent.sandbox_manager_client,
        backend_id: parent.backend_id,
        cas_client: parent.cas_client,
        sandbox_tokens: parent.sandbox_tokens,
        agent_model: parent.agent_model.clone(),
        permission_wire: parent.permission_wire,
        depth: parent.depth + 1,
        // Hang the child's step ids off the delegating call's step id so the
        // Agent Run Console attributes delegated work to the call that caused
        // it, and so two sibling subagents cannot collide.
        step_prefix: format!("{parent_step_id}."),
    };

    info!(
        run_id = %parent.req.run_id,
        subagent = %label,
        depth = child.depth,
        budget,
        "run_agent: delegating to subagent (isolated context)"
    );

    let rounds_result = run_rounds(&child, &task.goal, budget).await;
    let (outcome, rounds) = match rounds_result {
        Ok(result) => {
            let rounds = result.total_rounds();
            match result.outcome {
                RoundsOutcome::Answered(text) if !text.trim().is_empty() => {
                    // `on_delegation` (harness-adoption §7.9, hermes-agent's
                    // parent-side delegation hook, MIT): record that this
                    // delegation happened and what it concluded, so a later
                    // conversation can recall "we already investigated X".
                    // Fire-and-forget and non-ZDR only — the parent's answer
                    // must never wait on, or fail because of, a memory write.
                    record_delegation_memory(parent, &task.goal, &text);
                    (Ok(text), rounds)
                }
                RoundsOutcome::Answered(_) => (
                    Err(format!("subagent '{label}' returned an empty answer")),
                    rounds,
                ),
                RoundsOutcome::Exhausted => (
                    Err(format!(
                        "subagent '{label}' spent its whole {budget}-round budget without \
                         reaching an answer; narrow the delegated task or do it directly"
                    )),
                    rounds,
                ),
                RoundsOutcome::InferFailed => (
                    Err(format!(
                        "subagent '{label}' failed: inference was unavailable"
                    )),
                    rounds,
                ),
                // Unreachable: a nested loop refuses gated tools per-call rather
                // than pausing. Kept explicit so a future change cannot turn a
                // pause into a silent success.
                RoundsOutcome::Paused(_) => (
                    Err(format!(
                        "subagent '{label}' cannot pause the run for human approval"
                    )),
                    rounds,
                ),
            }
        }
        Err(status) => (
            Err(format!("subagent '{label}' failed: {}", status.message())),
            0,
        ),
    };

    // Persist the answer on the CHILD run's own record before settling.
    //
    // This is what makes a cold resume possible: the parent's transcript
    // deliberately does not keep it (a resumed parent learns *that* its child
    // finished, and learns *what* it concluded only through an explicitly
    // permitted read — `read_subagent_result`), and the terminal receipt is
    // metadata-only by design. Ordered before the receipt so a run that is
    // reported complete already has its answer readable.
    if let (Some(child), Ok(answer)) = (delegation.as_ref(), outcome.as_ref()) {
        record_delegated_child_answer(parent, &child.run_id, answer).await;
    }

    // Always settle: a managed obligation left open is force-failed by the
    // deadline watchdog, so every path that created a child run must close it.
    if let Some(child) = delegation.as_ref() {
        settle_delegated_child_run(parent, &child.run_id, outcome.is_ok()).await;
    }
    (outcome, rounds)
}

/// Write a completed delegation's answer to the child run's own record.
///
/// Best-effort by design: the delegated work is the product, and the parent must
/// still receive its answer when the durable copy cannot be written. What is
/// lost then is only the ability to read it back after a restart, which is
/// exactly what the log line says.
///
/// ZDR skips this entirely rather than relying on the server's refusal. Both
/// checks exist on purpose: the caller knows the run's posture without a round
/// trip, and session-core fails closed for a zero-retention credential anyway —
/// so neither side depends on the other being right.
async fn record_delegated_child_answer(parent: &LoopContext<'_>, child_run_id: &str, answer: &str) {
    if parent.req.zdr {
        return;
    }
    if answer.trim().is_empty() {
        return;
    }
    let token = match parent
        .terminal_tokens
        .terminalize_token(&parent.req.org_id)
        .await
    {
        Ok(token) => token,
        Err(error) => {
            warn!(child_run_id, %error, "run_agent: no credential to record the delegated answer");
            return;
        }
    };
    let request = match authenticated_session_request(
        pb::RecordRunOutputRequest {
            run_id: child_run_id.to_owned(),
            output: answer.to_owned(),
        },
        Some(&token),
    ) {
        Ok(request) => request,
        Err(error) => {
            warn!(child_run_id, %error, "run_agent: delegated answer not forwardable");
            return;
        }
    };
    match ManagedRunLifecycleClient::new(parent.session_channel.clone())
        .record_run_output(request)
        .await
    {
        Ok(response) => {
            let stored = response.into_inner().stored_chars;
            info!(
                child_run_id,
                stored_chars = stored,
                truncated = usize::try_from(stored).unwrap_or(usize::MAX) < answer.chars().count(),
                "run_agent: delegated answer recorded on the child run"
            );
        }
        Err(error) => warn!(
            child_run_id,
            code = ?error.code(),
            "run_agent: delegated answer not durably recorded; a resumed parent will \
             see that this child finished but not what it concluded"
        ),
    }
}

/// Ceilings on what one delegation memory may record. Memory is for durable
/// facts, not transcripts — an unbounded subagent answer pasted into memory
/// would dominate every later recall by sheer size.
const DELEGATION_MEMORY_GOAL_CHARS: usize = 200;
const DELEGATION_MEMORY_OUTCOME_CHARS: usize = 400;

/// Best-effort, detached memory record of a completed delegation
/// (`on_delegation`, §7.9). Spawned so the parent's turn never waits on it;
/// every failure mode is a log line, never a tool error. ZDR runs record
/// nothing — the run was promised no durable trace, and a "helpful" memory
/// entry is exactly such a trace.
fn record_delegation_memory(parent: &LoopContext<'_>, goal: &str, outcome: &str) {
    if parent.req.zdr {
        return;
    }
    let Some(bearer) = parent.session_bearer.map(str::to_owned) else {
        return;
    };
    let truncate = |text: &str, max: usize| -> String {
        if text.chars().count() <= max {
            text.trim().to_owned()
        } else {
            let cut: String = text.chars().take(max).collect();
            format!("{}…", cut.trim_end())
        }
    };
    let content = format!(
        "Delegated task: {}\nOutcome: {}",
        truncate(goal, DELEGATION_MEMORY_GOAL_CHARS),
        truncate(outcome, DELEGATION_MEMORY_OUTCOME_CHARS),
    );
    let request = pb::IndexMemoryRequest {
        thread_id: parent.req.thread_id.clone(),
        topic: "DELEGATION".to_owned(),
        content,
        org_id: parent.req.org_id.clone(),
        memory_id: String::new(),
        user_id: parent.req.user_id.clone(),
    };
    let channel = parent.session_channel.clone();
    let run_id = parent.req.run_id.clone();
    tokio::spawn(async move {
        let request = match runtime_loop::authenticated_session_request(request, &bearer) {
            Ok(r) => r,
            Err(error) => {
                warn!(%run_id, %error, "delegation memory skipped: credential not forwardable");
                return;
            }
        };
        if let Err(status) = pb::memory_service_client::MemoryServiceClient::new(channel)
            .index_memory(request)
            .await
        {
            warn!(%run_id, error = %status.message(), "delegation memory write failed (best-effort)");
        }
    });
}

/// Resolve the model the agentic loop drives. Autonomous, tool-using runs need
/// a tool-following model; a weak chat-tier model under-elects tools. When
/// `EXECUTION_AGENT_MODEL` is set (e.g. "verevon-balance"), it becomes the
/// agentic run's model so inference-core's intent layer picks a tool-capable
/// model (the classifier upgrades complexity when tools are offered). Empty /
/// unset → the requested model is honored unchanged.
fn resolve_agent_model(requested: &str) -> String {
    match std::env::var("EXECUTION_AGENT_MODEL") {
        Ok(value) if !value.trim().is_empty() => value.trim().to_owned(),
        _ => requested.to_owned(),
    }
}

/// Resolve the request's wire mode into a posture. Deployed agents default to
/// `ask`: only an explicit `auto` allows risky tools (e.g. `shell`) without a
/// gate; `deny` blocks all tools.
fn resolve_mode(wire: &str) -> PermissionMode {
    match wire {
        "auto" => PermissionMode::Auto,
        "deny" => PermissionMode::Deny,
        // "ask", "execute", "", and anything else → ask (safe default).
        _ => PermissionMode::Ask,
    }
}

/// Map a posture back to the wire string `execute_step`/`permission::evaluate`
/// expect.
fn mode_wire(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::Auto => "auto",
        PermissionMode::Ask => "ask",
        PermissionMode::Deny => "deny",
    }
}

/// Stable per-tool step id: `{prefix}tool_{seq}_{call_id_or_name}`.
///
/// `prefix` is empty for the user-facing run and the delegating call's step id
/// (plus `.`) inside a subagent, which is what keeps two nested loops' `seq`
/// counters from producing the same step id.
fn tool_step_id(prefix: &str, seq: u32, call: &pb::ToolCall) -> String {
    let suffix = if call.id.is_empty() {
        &call.name
    } else {
        &call.id
    };
    format!("{prefix}tool_{seq}_{suffix}")
}

/// HONESTY_CONTRACT: true when a `knowledge_search` tool outcome's JSON
/// envelope (see `execute_knowledge_search`/`knowledge_tools::format_candidates`)
/// reports `status: "ok"` — i.e. the org's knowledge base actually returned
/// relevant results, not `no_results`/`low_confidence`/`degraded`. Malformed
/// output (never expected from the real tool) is treated as ungrounded.
fn knowledge_search_found_grounding(output: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(output)
        .ok()
        .and_then(|value| {
            value
                .get("status")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .is_some_and(|status| status == "ok")
}

/// Canonical signature used only to suppress exact retrieval loops. Invalid or
/// unsupported inputs deliberately return `None` so the normal dispatch path
/// can produce the authoritative validation error after capability policy.
fn retrieval_signature(tool_name: &str, tool_input: &str) -> Option<String> {
    if tool_name != "knowledge_search" {
        return None;
    }
    let input = super::parse_knowledge_input(tool_input).ok()?;
    let normalized_query = input
        .query
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ");
    Some(format!(
        "{}|{}|{}",
        input.route.as_str(),
        input.top_k,
        normalized_query
    ))
}

/// The browser adapter deliberately has a narrower wire schema than Quarry's
/// public `/step` endpoint. Model Plane can act only on opaque observations
/// from an already-granted run; it cannot smuggle CSS, URL, CDP, coordinate,
/// or filesystem authority through a generic JSON payload.
fn browser_act_parameters_json() -> String {
    r#"{
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "grant_id": {"type": "string", "description": "Opaque active BrowserBroker grant id"},
        "quarry_run_id": {"type": "string", "description": "Existing Quarry browser run id"},
        "lease_id": {"type": "string", "description": "Existing Quarry browser lease id"},
        "action": {
          "oneOf": [
            {"type":"object","additionalProperties":false,"properties":{"type":{"const":"click_ref"},"snapshot_id":{"type":"string"},"generation":{"type":"integer","minimum":0},"ref_id":{"type":"string"}},"required":["type","snapshot_id","generation","ref_id"]},
            {"type":"object","additionalProperties":false,"properties":{"type":{"const":"type_ref"},"snapshot_id":{"type":"string"},"generation":{"type":"integer","minimum":0},"ref_id":{"type":"string"},"text":{"type":"string","maxLength":65536}},"required":["type","snapshot_id","generation","ref_id","text"]},
            {"type":"object","additionalProperties":false,"properties":{"type":{"const":"select_ref"},"snapshot_id":{"type":"string"},"generation":{"type":"integer","minimum":0},"ref_id":{"type":"string"},"value":{"type":"string","maxLength":65536}},"required":["type","snapshot_id","generation","ref_id","value"]},
            {"type":"object","additionalProperties":false,"properties":{"type":{"const":"wait_for_ref"},"snapshot_id":{"type":"string"},"generation":{"type":"integer","minimum":0},"ref_id":{"type":"string"},"timeout_ms":{"type":"integer","minimum":1,"maximum":30000}},"required":["type","snapshot_id","generation","ref_id","timeout_ms"]},
            {"type":"object","additionalProperties":false,"properties":{"type":{"const":"frame_click_ref"},"snapshot_id":{"type":"string"},"generation":{"type":"integer","minimum":0},"frame_id":{"type":"string"},"ref_id":{"type":"string"}},"required":["type","snapshot_id","generation","frame_id","ref_id"]},
            {"type":"object","additionalProperties":false,"properties":{"type":{"const":"frame_type_ref"},"snapshot_id":{"type":"string"},"generation":{"type":"integer","minimum":0},"frame_id":{"type":"string"},"ref_id":{"type":"string"},"text":{"type":"string","maxLength":65536}},"required":["type","snapshot_id","generation","frame_id","ref_id","text"]},
            {"type":"object","additionalProperties":false,"properties":{"type":{"const":"frame_select_ref"},"snapshot_id":{"type":"string"},"generation":{"type":"integer","minimum":0},"frame_id":{"type":"string"},"ref_id":{"type":"string"},"value":{"type":"string","maxLength":65536}},"required":["type","snapshot_id","generation","frame_id","ref_id","value"]},
            {"type":"object","additionalProperties":false,"properties":{"type":{"const":"frame_wait_for_ref"},"snapshot_id":{"type":"string"},"generation":{"type":"integer","minimum":0},"frame_id":{"type":"string"},"ref_id":{"type":"string"},"timeout_ms":{"type":"integer","minimum":1,"maximum":30000}},"required":["type","snapshot_id","generation","frame_id","ref_id","timeout_ms"]},
            {"type":"object","additionalProperties":false,"properties":{"type":{"const":"respond_dialog"},"dialog_id":{"type":"string"},"accept":{"type":"boolean"},"approval_grant_id":{"type":"string"},"prompt_text":{"type":"string","maxLength":65536}},"required":["type","dialog_id","accept","approval_grant_id"]},
            {"type":"object","additionalProperties":false,"properties":{"type":{"const":"upload_ref"},"snapshot_id":{"type":"string"},"generation":{"type":"integer","minimum":0},"ref_id":{"type":"string"},"artifact_id":{"type":"string","description":"Tenant-owned Quarry artifact id, never a host path"},"approval_grant_id":{"type":"string"}},"required":["type","snapshot_id","generation","ref_id","artifact_id","approval_grant_id"]},
            {"type":"object","additionalProperties":false,"properties":{"type":{"const":"download_ref"},"snapshot_id":{"type":"string"},"generation":{"type":"integer","minimum":0},"ref_id":{"type":"string"},"approval_grant_id":{"type":"string"}},"required":["type","snapshot_id","generation","ref_id","approval_grant_id"]},
            {"type":"object","additionalProperties":false,"properties":{"type":{"const":"frame_upload_ref"},"snapshot_id":{"type":"string"},"generation":{"type":"integer","minimum":0},"frame_id":{"type":"string"},"ref_id":{"type":"string"},"artifact_id":{"type":"string","description":"Tenant-owned Quarry artifact id, never a host path"},"approval_grant_id":{"type":"string"}},"required":["type","snapshot_id","generation","frame_id","ref_id","artifact_id","approval_grant_id"]},
            {"type":"object","additionalProperties":false,"properties":{"type":{"const":"frame_download_ref"},"snapshot_id":{"type":"string"},"generation":{"type":"integer","minimum":0},"frame_id":{"type":"string"},"ref_id":{"type":"string"},"approval_grant_id":{"type":"string"}},"required":["type","snapshot_id","generation","frame_id","ref_id","approval_grant_id"]}
          ]
        }
      },
      "required": ["grant_id", "quarry_run_id", "lease_id", "action"]
    }"#
        .to_owned()
}

/// The read-tool allowlist offered to the model. JSON-Schema literals follow the
/// `model-gateway::tool_loop::builtin_tool_defs` pattern. This set IS the
/// purpose-lock scope: only these tools may be called.
/// Contract: every tool this loop OFFERS must have a governed capability
/// binding, and every id it binds to must be one capability-core actually
/// seeds.
///
/// # Why this is not covered by the existing runtime-loop tests
///
/// `execute_step_inner` evaluates capability policy BEFORE any dispatch, and
/// `capability_policy::trusted_capability_id` returning `None` becomes a hard
/// refusal there. So a tool added to [`offered_tool_defs`] without a binding is
/// advertised to the model and then refused the moment it is called — in
/// production only. Every runtime-loop test injects an `Allow`
/// capability-policy double that never consults the mapping, so the whole
/// existing suite passes with a tool that cannot run.
///
/// That is not hypothetical: `save_memory` and `recall_memory` were added to
/// this list, unit-tested green against that double, and were dead on the
/// governed loop until this test was written.
#[cfg(test)]
mod system_prompt_composition {
    use super::{
        compose_system_prompt, offered_tool_defs, ACTION_TOOL_NAMES, PREAMBLE_CORE,
        SNIPPET_ACTION_TOOLS, SNIPPET_KNOWLEDGE_SEARCH, SNIPPET_MEMORY_TOOLS, SNIPPET_SUBAGENT,
        SNIPPET_USER_SUPPLIED_ARGS, USER_SUPPLIED_ARG_TOOLS,
    };
    use std::collections::BTreeSet;

    fn offered(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    /// The core is unconditional; nothing tool-specific is.
    #[test]
    fn an_empty_toolset_gets_only_the_core() {
        let prompt = compose_system_prompt(&offered(&[]));
        assert_eq!(prompt, PREAMBLE_CORE);
    }

    /// The dotted Quarry tools are aliases, and their descriptions now say so.
    ///
    /// Advertising two names for one code path is a distractor: Anthropic's
    /// tool-authoring guidance names differentiation as a primary practice, and
    /// `web.search` previously described itself as using "the same boundary as
    /// web_search" without saying it *was* web_search. They are not merged
    /// because the underscore names are compatibility aliases for stored plans
    /// (`runtime_loop::QUARRY_MCP_WEB_SEARCH_TOOL`) and dropping either from the
    /// offered set would drop it from the purpose-lock allowlist too.
    #[test]
    fn the_aliased_web_tools_say_they_are_aliases_and_name_the_preferred_one() {
        let defs = offered_tool_defs();
        for (alias, preferred) in [("web.search", "web_search"), ("web.read", "web_fetch")] {
            let def = defs
                .iter()
                .find(|def| def.name == alias)
                .unwrap_or_else(|| panic!("{alias} is no longer offered; re-point this test"));
            assert!(
                def.description.contains("ALIAS"),
                "{alias} must say it is an alias, or the model is choosing between two \
                 tools it cannot tell apart"
            );
            assert!(
                def.description.contains(preferred),
                "{alias} must name {preferred} as the one to prefer"
            );
            let target = defs
                .iter()
                .find(|def| def.name == preferred)
                .expect("the preferred tool must exist");
            assert_eq!(
                def.parameters_json, target.parameters_json,
                "{alias} claims to be an alias of {preferred} but takes different arguments"
            );
        }
    }

    /// The elicitation snippet is conditional like every other one: a run with
    /// no tool that needs user-only values must not carry advice about asking
    /// for them.
    #[test]
    fn a_toolset_with_no_user_supplied_args_is_not_told_to_ask_for_any() {
        let prompt = compose_system_prompt(&offered(&["yr_weather", "web_search", "news"]));
        assert!(
            !prompt.contains(SNIPPET_USER_SUPPLIED_ARGS),
            "coordinates and a search query are derivable; asking for them is the regression"
        );
    }

    #[test]
    fn a_toolset_with_user_supplied_args_is_told_not_to_invent_them() {
        for tool in USER_SUPPLIED_ARG_TOOLS {
            let prompt = compose_system_prompt(&offered(&[tool]));
            assert!(
                prompt.contains(SNIPPET_USER_SUPPLIED_ARGS),
                "{tool} requires values only the user holds"
            );
        }
    }

    /// The list must stay narrow. Every entry has to actually require a value no
    /// other offered tool can discover and the request cannot supply — the whole
    /// reason it is not simply "tools with required parameters".
    #[test]
    fn the_user_supplied_list_excludes_derivable_and_discoverable_tools() {
        for derivable in [
            "web_search",
            "knowledge_search",
            "code_interpreter",
            "yr_weather",
            "traffic",
            "recall_memory",
        ] {
            assert!(
                !USER_SUPPLIED_ARG_TOOLS.contains(&derivable),
                "{derivable}'s required args restate the request or are public fact"
            );
        }
        for discoverable in ["read_subagent_result", "execute_provider_action"] {
            assert!(
                !USER_SUPPLIED_ARG_TOOLS.contains(&discoverable),
                "{discoverable}'s missing ids are resolved by a listing tool, measured 10/10"
            );
        }
    }

    /// The snippet has to distinguish asking for a fact from asking permission,
    /// or it contradicts [`PREAMBLE_CORE`] — whose anti-permission wording was
    /// added because its absence measurably suppressed tool use.
    #[test]
    fn the_snippet_separates_asking_for_a_fact_from_asking_permission() {
        let text = SNIPPET_USER_SUPPLIED_ARGS.to_lowercase();
        assert!(text.contains("not asking permission"));
        assert!(
            text.contains("fill required arguments"),
            "it must also say which values to fill without asking, or it reads as a licence to stall"
        );
    }

    /// The bug this item fixes. A run with only read tools must not be told it
    /// can book shipments or publish posts — it cannot, and saying so invites a
    /// confident claim about an action that will never happen.
    #[test]
    fn a_read_only_toolset_is_never_told_it_can_take_actions() {
        let prompt = compose_system_prompt(&offered(&["yr_weather", "traffic", "news"]));
        assert!(
            !prompt.contains(SNIPPET_ACTION_TOOLS),
            "a read-only run must not claim real-effect capability"
        );
        assert!(
            !prompt.contains("REAL effect"),
            "no action framing may survive in a read-only prompt"
        );
        assert!(
            !prompt.contains(SNIPPET_KNOWLEDGE_SEARCH),
            "knowledge_search rules must not appear without knowledge_search"
        );
    }

    /// And the converse — the wording whose ABSENCE was measured to suppress
    /// real tool use must appear whenever an action tool is offered. Asserted
    /// per action tool so adding one to ACTION_TOOL_NAMES cannot half-work.
    #[test]
    fn every_action_tool_triggers_the_measured_capability_wording() {
        for tool in ACTION_TOOL_NAMES {
            let prompt = compose_system_prompt(&offered(&[tool]));
            assert!(
                prompt.contains(SNIPPET_ACTION_TOOLS),
                "offering {tool} must include the action-capability wording; without \
                 it models default to 'I cannot access your accounts' and draft \
                 copy-paste text instead of calling the tool"
            );
        }
    }

    #[test]
    fn tool_specific_snippets_track_their_tools() {
        let prompt = compose_system_prompt(&offered(&["knowledge_search"]));
        assert!(prompt.contains(SNIPPET_KNOWLEDGE_SEARCH));

        let prompt = compose_system_prompt(&offered(&["save_memory"]));
        assert!(prompt.contains(SNIPPET_MEMORY_TOOLS));
        let prompt = compose_system_prompt(&offered(&["recall_memory"]));
        assert!(
            prompt.contains(SNIPPET_MEMORY_TOOLS),
            "either memory tool alone must bring the guidance"
        );

        let prompt = compose_system_prompt(&offered(&["subagent.research"]));
        assert!(prompt.contains(SNIPPET_SUBAGENT));
        let prompt = compose_system_prompt(&offered(&["subagent."]));
        assert!(
            !prompt.contains(SNIPPET_SUBAGENT),
            "the bare prefix is not a subagent tool — same rule as \
             subagent::is_subagent_tool and trusted_capability_id"
        );
    }

    /// A pure function of the offered set: same input, same output, and order
    /// independent of iteration order (BTreeSet already sorts, so this pins that
    /// no snippet is emitted from inside a per-tool loop).
    #[test]
    fn composition_is_deterministic() {
        let set = offered(&[
            "knowledge_search",
            "book_shipment",
            "save_memory",
            "yr_weather",
        ]);
        let first = compose_system_prompt(&set);
        assert_eq!(first, compose_system_prompt(&set));
        for snippet in [
            SNIPPET_ACTION_TOOLS,
            SNIPPET_KNOWLEDGE_SEARCH,
            SNIPPET_MEMORY_TOOLS,
        ] {
            assert_eq!(
                first.matches(snippet).count(),
                1,
                "each snippet must appear exactly once regardless of how many \
                 tools trigger it"
            );
        }
    }

    /// Regression guard on the real default run: the production toolset must
    /// still receive the action and knowledge-search guidance it had when the
    /// prompt was one string. A decomposition that quietly dropped either would
    /// otherwise pass every test above.
    #[test]
    fn the_real_offered_toolset_keeps_the_guidance_it_had() {
        let set: BTreeSet<String> = offered_tool_defs()
            .into_iter()
            .map(|def| def.name)
            .collect();
        let prompt = compose_system_prompt(&set);
        for (snippet, why) in [
            (SNIPPET_ACTION_TOOLS, "the default run offers action tools"),
            (
                SNIPPET_KNOWLEDGE_SEARCH,
                "the default run offers knowledge_search",
            ),
            (
                SNIPPET_MEMORY_TOOLS,
                "the default run offers the memory tools",
            ),
        ] {
            assert!(
                prompt.contains(snippet),
                "the production toolset lost guidance it used to have: {why}"
            );
        }
    }
}

#[cfg(test)]
mod capability_binding_contract {
    use super::offered_tool_defs;
    use crate::capability_policy::trusted_capability_id;
    use std::path::Path;

    /// Names that legitimately resolve no static capability. Keep this list
    /// short and justified — every entry is a tool the model can see and not
    /// use through this mapping.
    const EXPECTED_UNBOUND: &[&str] = &[];

    #[test]
    fn every_offered_tool_has_a_capability_binding() {
        let unbound: Vec<String> = offered_tool_defs()
            .into_iter()
            .map(|def| def.name)
            .filter(|name| trusted_capability_id(name).is_none())
            .filter(|name| !EXPECTED_UNBOUND.contains(&name.as_str()))
            .collect();
        assert!(
            unbound.is_empty(),
            "these tools are offered to the model but have no capability \
             binding, so `execute_step_inner` will refuse them on first use: \
             {unbound:?}\n\nAdd an arm to \
             `capability_policy::trusted_capability_id` using an id \
             capability-core already seeds, or add the tool to \
             EXPECTED_UNBOUND with a reason."
        );
    }

    /// A binding to a SEEDED id still fails closed if nothing ever attests it.
    ///
    /// Migration rows are created `availability_state='unavailable',
    /// availability_reason_code='health_not_attested'` by doctrine (0008, 0013,
    /// 0014) — source presence is not runtime health. So the seeding test above
    /// passes for a capability that can never be allowed, and this is the third
    /// link the chain needs: advertised -> bound -> SEEDED -> ATTESTED.
    ///
    /// Both instances this catches were real. `save_memory`/`recall_memory`
    /// were advertised, prompted for and dispatched while
    /// `cap.memory.{index,search}` had no attestor at all; and advertising
    /// `subagent.task` briefly re-created the same state for `cap.agent.spawn`,
    /// which 0008 had seeded and nothing attested — the model would have burned
    /// a round discovering a refusal.
    #[test]
    fn every_offered_tool_capability_has_an_attestor() {
        let attestor = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/health_attest.rs"),
        )
        .expect("health_attest.rs is the runtime attestation source; re-point this test if moved");

        // Resolve which capability ids are ACTUALLY ATTESTED, not merely named.
        //
        // The first version of this check did `attestor.contains(&capability)`
        // over the whole file — which matched the `pub const X: &str = "id";`
        // DECLARATION, so removing the attestation while leaving the constant
        // still passed. Verified by mutation: it did not fail. That is the exact
        // declared-but-unused false pass this whole test exists to catch, so it
        // is resolved properly here: collect the constant->id map, then look only
        // inside the two functions that BUILD attestations.
        let const_ids: Vec<(String, String)> = attestor
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                let rest = line
                    .strip_prefix("pub const ")
                    .or_else(|| line.strip_prefix("const "))?;
                let (name, tail) = rest.split_once(": &str = ")?;
                let value = tail.trim().trim_end_matches(';').trim_matches('"');
                (!value.is_empty()).then(|| (name.trim().to_owned(), value.to_owned()))
            })
            .collect();

        let attesting_bodies: String = ["pub fn attestable(", "pub fn memory_attestations("]
            .iter()
            .map(|signature| {
                let start = attestor
                    .find(signature)
                    .unwrap_or_else(|| panic!("`{signature}` is gone; re-point this test"));
                let rest = &attestor[start..];
                // Up to the next top-level item — enough to cover the body.
                let end = rest[1..]
                    .find("\npub fn ")
                    .or_else(|| rest[1..].find("\nfn "))
                    .map_or(rest.len(), |at| at + 1);
                rest[..end].to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n");

        let attested: Vec<&str> = const_ids
            .iter()
            .filter(|(name, _)| attesting_bodies.contains(name.as_str()))
            .map(|(_, id)| id.as_str())
            .collect();
        assert!(
            attested.len() >= 2,
            "resolved only {attested:?} as attested — the parse broke, not the invariant"
        );

        // QUARANTINE, not an approval list.
        //
        // These twelve are advertised today, bound to a capability, and NOT
        // attested by execution-core. Each one PLAUSIBLY belongs to another
        // service's reporter (quarry-edge for web/information reads,
        // integration-core for provider and social, the Data Plane for
        // retrieval, browser-broker for browser grants, shipping-core for
        // shipping) — but that was NOT verified when this list was written, and
        // the three reporters known to exist as of 2026-08-26 attest only
        // cap.command.{shell,sandbox}, cap.tool.ticket.create and
        // cap.tool.shipping.{read,book}. Notably `cap.tool.shipping.track` is
        // NOT among them.
        //
        // So this list may well be twelve more instances of exactly the defect
        // this test catches. It is recorded rather than silently excluded so the
        // question is visible and answerable; the test's job meanwhile is to
        // stop the list from GROWING, which is what caught cap.memory.* and
        // cap.agent.spawn. Removing an entry requires naming the reporter that
        // attests it.
        // VERIFIED elsewhere — a named reporter was confirmed to attest these
        // (2026-08-26 verification: shipping-core and conversation-core each run
        // a Go `capabilityhealth` reporter on their own route/scope).
        const ATTESTED_BY_ANOTHER_REPORTER: &[(&str, &str)] = &[
            (
                "cap.tool.shipping.read",
                "shipping-core capabilityhealth reporter",
            ),
            (
                "cap.tool.shipping.book",
                "shipping-core capabilityhealth reporter",
            ),
            (
                "cap.tool.ticket.create",
                "conversation-core capabilityhealth reporter",
            ),
        ];

        const UNATTESTED_BASELINE: &[&str] = &[
            "cap.tool.information.read",
            "cap.tool.shipping.track",
            "cap.tool.social.read",
            "cap.tool.social.publish",
            "cap.retrieval.query",
            "cap.tool.provider.read",
            "cap.tool.provider.execute",
            "cap.browser.open",
            "cap.tool.http",
            "cap.skill.summarize",
            "cap.agent.lineage.read",
        ];

        let mut unattested = Vec::new();
        for def in offered_tool_defs() {
            let Some(capability) = trusted_capability_id(&def.name) else {
                continue; // covered by the binding test above
            };
            if ATTESTED_BY_ANOTHER_REPORTER
                .iter()
                .any(|(id, _)| *id == capability.as_str())
                || UNATTESTED_BASELINE.contains(&capability.as_str())
            {
                continue;
            }
            if !attested.contains(&capability.as_str()) {
                unattested.push(format!("{} -> {capability}", def.name));
            }
        }
        assert!(
            unattested.is_empty(),
            "these tools are advertised and bound to a capability that              execution-core's health reporter never attests, so every call              fails closed at the policy gate: {unattested:?}\n\nEither attest              it in `health_attest.rs` from a TRUTHFUL probe of what the tool              actually needs, or add it to ATTESTED_ELSEWHERE naming the service              that owns its health."
        );
    }

    /// Binding to an id nothing seeds fails closed at runtime —
    /// `EvaluatePolicy` has nothing to evaluate — which looks identical to a
    /// missing binding. Read across to capability-core the way
    /// `tests/cross_service_loop_contract.rs` does, for the same reason: the
    /// two services deploy separately and neither can depend on the other.
    ///
    /// Capabilities are seeded from TWO places, and checking only one is how
    /// the first draft of this test produced ten false positives: the Go
    /// `registry.go` table, and the `capabilities` INSERTs in
    /// `migrations/*.up.sql`. Both are scanned.
    #[test]
    fn every_bound_capability_id_is_seeded_by_capability_core() {
        let capability_core =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../go/services/capability-core");
        let registry =
            std::fs::read_to_string(capability_core.join("internal/registry/registry.go"))
                .unwrap_or_else(|error| {
                    panic!(
                        "cannot read capability-core's registry.go ({error}). If it moved, \
                 re-point this test — do not delete it; it is the only check that \
                 these ids are real."
                    )
                });
        let migrations_dir = capability_core.join("migrations");
        let mut seeded = registry;
        let entries = std::fs::read_dir(&migrations_dir).unwrap_or_else(|error| {
            panic!(
                "cannot list {} ({error}) — half the seeding source would be \
                 invisible and this test would report false positives",
                migrations_dir.display()
            )
        });
        let mut migration_count = 0usize;
        for entry in entries {
            let path = entry.expect("readable dir entry").path();
            // ONLY `*.up.sql`. A `*.down.sql` names the same id in its DELETE
            // statements, so scanning both made a capability that is *deleted*
            // and never inserted look seeded — which is how this test passed
            // for `cap.agent.lineage.read` after its up-migration was removed.
            let is_up_migration = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".up.sql"));
            if is_up_migration {
                seeded.push_str(&std::fs::read_to_string(&path).expect("readable migration"));
                migration_count += 1;
            }
        }
        assert!(
            migration_count > 0,
            "found no migrations to scan — this test broke, not the invariant"
        );

        // Every offered tool's id, plus the two reachable only through
        // special-cased paths rather than a plain match arm.
        let mut ids: Vec<String> = offered_tool_defs()
            .into_iter()
            .filter_map(|def| trusted_capability_id(&def.name))
            .collect();
        ids.extend(trusted_capability_id("subagent.researcher"));
        ids.push(crate::ticket_tools::CAPABILITY_ID.to_owned());
        ids.sort();
        ids.dedup();
        assert!(
            !ids.is_empty(),
            "parsed no capability ids at all — this test broke, not the invariant"
        );

        // Match the quoted id so a substring of a longer id cannot pass:
        // `cap.memory` must not be satisfied by `cap.memory.search`.
        let missing: Vec<&String> = ids
            .iter()
            .filter(|id| {
                !seeded.contains(&format!("ID: \"{id}\"")) && !seeded.contains(&format!("'{id}'"))
            })
            .collect();
        assert!(
            missing.is_empty(),
            "these capability ids are bound by execution-core but seeded by \
             neither capability-core's registry.go nor its migrations, so every \
             call is refused with nothing to evaluate: {missing:?}"
        );
    }
}

fn offered_tool_defs() -> Vec<pb::ToolDefinition> {
    let tools = vec![
        pb::ToolDefinition {
            name: "yr_weather".to_owned(),
            description: "Get the current Yr/met.no weather forecast for a coordinate in Norway.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"lat":{"type":"number","description":"Latitude"},"lon":{"type":"number","description":"Longitude"}},"required":["lat","lon"]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "traffic".to_owned(),
            description: "Get traffic registration stations (volume/speed) near a coordinate from Statens Vegvesen.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"lat":{"type":"number","description":"Latitude"},"lon":{"type":"number","description":"Longitude"},"radius":{"type":"integer","description":"Search radius in metres (default 5000)"}},"required":["lat","lon"]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "news".to_owned(),
            description: "Get the latest Norwegian/industry news articles, optionally filtered by category.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"category":{"type":"string","description":"Optional category filter"},"limit":{"type":"integer","description":"Max articles (default 10)"}},"required":[]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "track_shipment".to_owned(),
            description: "Track a parcel by tracking number against the Bring/Posten carrier API.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"tracking_number":{"type":"string","description":"Carrier tracking number"}},"required":["tracking_number"]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "company_lookup".to_owned(),
            description: "Look up a Norwegian company in the public Brønnøysund Enhetsregisteret by name or 9-digit organisation number. Returns name, org.nr, address, and status.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"query":{"type":"string","description":"Company name or 9-digit organisation number"}},"required":["query"]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "get_shipping_quotes".to_owned(),
            description: "Compare shipping/freight quotes from Verevon's carrier aggregator (Bring, PostNord, DHL, Helthjem, Porterbuddy, m.fl.). Returns options sorted cheapest-first with price, transit time and features, plus any carriers that failed. Read-only comparison — it does NOT book anything. Ask the user for sender address, recipient address and package weight/dimensions before calling; never guess them.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"from":{"type":"object","description":"Sender address","properties":{"name":{"type":"string"},"street":{"type":"string"},"postal_code":{"type":"string"},"city":{"type":"string"},"country":{"type":"string","description":"ISO 3166-1 alpha-2, e.g. NO"},"is_business":{"type":"boolean"}},"required":["name","postal_code","city","country"]},"to":{"type":"object","description":"Recipient address","properties":{"name":{"type":"string"},"street":{"type":"string"},"postal_code":{"type":"string"},"city":{"type":"string"},"country":{"type":"string","description":"ISO 3166-1 alpha-2"},"is_business":{"type":"boolean"}},"required":["name","postal_code","city","country"]},"weight_kg":{"type":"number","description":"Package weight in kg"},"length_cm":{"type":"number"},"width_cm":{"type":"number"},"height_cm":{"type":"number"},"dangerous_good":{"type":"boolean","description":"Default false"},"segment":{"type":"string","enum":["b2b","b2c"],"description":"b2b when the RECIPIENT is a business, else b2c"}},"required":["from","to","weight_kg","length_cm","width_cm","height_cm","segment"]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "shipping_carriers".to_owned(),
            description: "List the carriers registered in Verevon's shipping aggregator and whether each runs on demo prices or live agreement prices.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{},"required":[]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "book_shipment".to_owned(),
            description: "BOOK a shipment with a carrier from Verevon's shipping aggregator — this places a REAL freight order (costs money; a courier will collect the parcel) and always requires human approval. Only call it after get_shipping_quotes, with the exact carrier_code/service_name/price from the quote the user chose. Cross-border shipments (from.country != to.country) REQUIRE a customs object; the server rejects them otherwise. Returns the booking id, carrier reference, and tracking number.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"quote_ref":{"type":"string","description":"Reference of the chosen quote"},"carrier_code":{"type":"string","description":"carrier_code from the chosen quote"},"service_name":{"type":"string","description":"service_name from the chosen quote"},"price_amount_cents":{"type":"integer","description":"Quoted price in minor units"},"price_currency":{"type":"string","description":"ISO 4217, e.g. NOK"},"from":{"type":"object","properties":{"name":{"type":"string"},"street":{"type":"string"},"postal_code":{"type":"string"},"city":{"type":"string"},"country":{"type":"string"},"is_business":{"type":"boolean"}},"required":["name","postal_code","city","country"]},"to":{"type":"object","properties":{"name":{"type":"string"},"street":{"type":"string"},"postal_code":{"type":"string"},"city":{"type":"string"},"country":{"type":"string"},"is_business":{"type":"boolean"}},"required":["name","postal_code","city","country"]},"weight_kg":{"type":"number"},"length_cm":{"type":"number"},"width_cm":{"type":"number"},"height_cm":{"type":"number"},"dangerous_good":{"type":"boolean"},"customs":{"type":"object","description":"Required cross-border: {contents_type: merchandise|gift|documents|sample|return, items:[{description,quantity,value_cents,currency,weight_kg,hs_code,origin_country}], incoterms?}"}},"required":["carrier_code","service_name","price_amount_cents","price_currency","from","to","weight_kg","length_cm","width_cm","height_cm"]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "list_social_accounts".to_owned(),
            description: "List the organization's connected SOCIAL MEDIA accounts (Meta/Facebook, Instagram, LinkedIn, TikTok, X, Snapchat) with status and capabilities. Read-only discovery — call this FIRST when the user asks to post/publish on social media, to learn which platforms are actually connected. Verevon CAN publish social posts: draft with publish_social_post after checking here.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{},"required":[]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "publish_social_post".to_owned(),
            description: "Create a social media post in Verevon's Social workspace and request its publish to the chosen platforms. This is a REAL outbound action and always requires human approval — first in this run, and the post then waits for workspace approval under Social → Approvals before anything goes live (report that honestly; never claim content is already published). Use platform keys from list_social_accounts. Optional scheduled_at (RFC 3339) schedules instead of publishing immediately.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"title":{"type":"string","description":"Optional internal title for the workspace"},"body":{"type":"string","description":"The post text"},"platforms":{"type":"array","items":{"type":"string"},"description":"Platform keys from list_social_accounts, e.g. ["linkedin","meta"]"},"scheduled_at":{"type":"string","description":"Optional RFC 3339 publish time"},"media":{"type":"array","items":{"type":"object"},"description":"Optional media refs, e.g. [{"type":"image","url":"https://…"}]"}},"required":["body","platforms"]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "knowledge_search".to_owned(),
            description: "Search the organization's OWN internal knowledge base through Data Plane's server-managed hybrid retrieval (dense/vector + sparse when configured). Returns a typed JSON status; reformulate on no_results/low_confidence. Graph, SQL/tabular, MCP, and vector-only routes are not configured by this contract.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"query":{"type":"string","description":"What to look up in the org knowledge base"},"top_k":{"type":"integer","minimum":1,"maximum":20,"description":"Max passages 1-20"},"route":{"type":"string","enum":["hybrid"],"description":"Supported server-managed Data Plane retrieval contract"}},"required":["query"]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "list_provider_actions".to_owned(),
            description: "List the organization's connected third-party providers (Meta/Facebook/Instagram, LinkedIn, Google, Microsoft, Slack, GitHub, Notion, Shopify, …) and the operations you can run on each via execute_provider_action. Read-only discovery — always call this FIRST to get the exact connection_id and operation names before calling execute_provider_action; never guess them. Operations marked [write] place real outbound actions and require human approval.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{},"required":[]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "execute_provider_action".to_owned(),
            description: "Run ONE operation on a connected provider through Verevon's integration gateway — e.g. publish a Facebook Page post, send a WhatsApp/Messenger message, list ad campaigns, create a GitHub issue. Use connection_id and operation exactly as returned by list_provider_actions (call that first). params/body are the operation-specific arguments described there. Write/outbound operations place REAL actions and always require human approval before they run.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"connection_id":{"type":"string","description":"Connection id from list_provider_actions"},"operation":{"type":"string","description":"Operation name from list_provider_actions, e.g. pages.post, whatsapp.messages.send, ads.campaigns"},"params":{"type":"object","description":"Operation path/query arguments, e.g. {\"pageId\":\"123\"} or {\"adAccountId\":\"act_123\"}"},"body":{"type":"object","description":"Operation request body, e.g. {\"message\":\"Hei!\"} for pages.post"}},"required":["connection_id","operation"]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "web_search".to_owned(),
            description: "Search the public web for current information. Returns ranked results with title, url, and snippet.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"query":{"type":"string","description":"Search query"},"limit":{"type":"integer","description":"Max results 1-50"}},"required":["query"]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "web_fetch".to_owned(),
            description: "Fetch and read a specific web page; returns its cleaned text content.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"url":{"type":"string","description":"Absolute http(s) URL to read"}},"required":["url"]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "web.search".to_owned(),
            description: "ALIAS for web_search — same Quarry client, same results, same scope. Kept so existing plans that name it keep working. Prefer web_search; there is no case where this returns anything different.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"query":{"type":"string","description":"Search query"},"limit":{"type":"integer","description":"Max results 1-50"}},"required":["query"]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "web.read".to_owned(),
            description: "ALIAS for web_fetch — same Quarry extract boundary, same cleaned text. Kept so existing plans that name it keep working. Prefer web_fetch; there is no case where this returns anything different.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"url":{"type":"string","description":"Absolute http(s) URL to read"}},"required":["url"]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "browser.observe".to_owned(),
            description: "Observe an EXISTING Quarry browser run using a BrowserBroker-issued grant, returning its current Quarry evidence, opaque accessibility snapshot refs, dialogs, redacted network/egress receipts, and telemetry. This cannot open a browser or navigate; use only run and lease ids provided by the approved browser workflow.".to_owned(),
            parameters_json: r#"{"type":"object","additionalProperties":false,"properties":{"grant_id":{"type":"string","description":"Opaque active BrowserBroker grant id"},"quarry_run_id":{"type":"string","description":"Existing Quarry browser run id"},"lease_id":{"type":"string","description":"Existing Quarry browser lease id"}},"required":["grant_id","quarry_run_id","lease_id"]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "browser.act".to_owned(),
            description: "Execute ONE governed action against an EXISTING Quarry browser run. Use only an exact snapshot_id, generation, and @e-style ref returned by browser.observe; never invent refs. A target with child frame_id requires the matching frame_*_ref action and exact frame id. No URLs, CSS selectors, JavaScript, raw CDP, coordinates, or filesystem paths are accepted. Dialog replies and artifact transfers require their fresh scope-specific approval grant and Quarry remains the final enforcer. This can have real external effects and requires human approval under ask posture.".to_owned(),
            parameters_json: browser_act_parameters_json(),
        },
        // Sandboxed code execution. Offered for the same reason as delegation
        // below — the purpose-lock rejects any tool absent from this list, so
        // without a definition here a `code_interpreter` call could never be
        // dispatched. `shell` stays deliberately UNOFFERED: arbitrary host
        // commands are not something an agent run should reach for, while this
        // tool is hermetic (own workspace, no network, timeout, scrubbed output).
        // The description states the real constraints because a model that
        // assumes network access or a persistent filesystem writes code that
        // cannot work here.
        pb::ToolDefinition {
            name: "code_interpreter".to_owned(),
            description: "Run REAL Python (or POSIX sh) in an isolated sandbox to compute, analyse data, or PRODUCE FILES — spreadsheets (openpyxl, XlsxWriter, pandas), Word documents (python-docx), PowerPoint (python-pptx), PDFs (reportlab, pypdf), charts/images (matplotlib headless, Pillow), plus numpy/pandas for data work. The working directory starts EMPTY and is deleted after the call, so write output with plain relative paths (e.g. open('report.xlsx','wb')) and they are returned to you as files with name, mime type and base64 content; nothing persists between calls. There is NO NETWORK: you cannot download anything, call an API, or pip install — use only the libraries listed. Pass input data via files_in (bare filenames, base64 content); the program reads them from the working directory. Print anything you want to read yourself to stdout. Long-running programs are killed at the timeout (30s by default), and a non-zero exit returns its traceback so you can fix the code and retry.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"language":{"type":"string","enum":["python","sh"],"description":"Defaults to python"},"code":{"type":"string","description":"The complete program to run"},"files_in":{"type":"array","description":"Optional input files written into the working directory before the program runs","items":{"type":"object","properties":{"name":{"type":"string","description":"Bare filename, no directories or '..'"},"content_b64":{"type":"string","description":"Base64-encoded file content"}},"required":["name","content_b64"]}}},"required":["code"]}"#.to_owned(),
        },
        // Long-term memory (harness-adoption §7.9). Offered here — and ONLY
        // here — because the governed agentic loop is memory's one legitimate
        // home: the inline chat loop refuses side effects by design (its old
        // `save_memory` arm was unreachable dead code for exactly that
        // reason), and the chat path gets memory passively via per-turn
        // prefetch instead. Writes are refused on ZDR runs with an honest
        // explanation; session-core enforces the same server-side.
        pb::ToolDefinition {
            name: "save_memory".to_owned(),
            description: "Save ONE durable fact to long-term memory so future conversations can recall it: a standing preference, constraint, decision, or identifier the user will expect you to remember (e.g. 'invoices must be in NOK', 'the project reference code is ZX-88214'). Do NOT save transient task state, tool output, or anything the user asked to keep private. Unavailable on Zero-Data-Retention runs.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"content":{"type":"string","description":"The single fact to remember, self-contained and concise"},"topic":{"type":"string","description":"Optional short topic label, e.g. PREFERENCE, CONSTRAINT, IDENTIFIER"}},"required":["content"]}"#.to_owned(),
        },
        // Delegation. This definition is what makes `run_subagent` reachable
        // first-party: dispatch is prefix-matched on `subagent.`
        // (`crate::subagent::is_subagent_tool`), the capability binding
        // (`subagent.*` → cap.agent.spawn) has been seeded since migration
        // 0008, `SNIPPET_SUBAGENT` gates on this name being offered — and yet
        // no catalogue advertised any `subagent.*` name, so the entire
        // delegation arc (nested loop, lineage, cold resume, replay semantics)
        // was reachable only by an API caller declaring the tool themselves.
        // Verified 2026-08-26: every prior `subagent.` literal was a policy
        // mapping or inside #[cfg(test)].
        pb::ToolDefinition {
            name: "subagent.task".to_owned(),
            description: "Delegate ONE self-contained sub-task to a fresh subagent that runs its own bounded tool loop and returns only its conclusion. The subagent starts with NO view of this conversation, so state the goal completely and include every fact it needs. Delegate only work that is genuinely separable (a research question, a bounded computation over tools); do the rest yourself. Costs rounds from this run's own budget and requires approval like other real-effect capabilities.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"goal":{"type":"string","description":"The complete, self-contained task for the subagent"},"max_rounds":{"type":"integer","description":"Optional cap on the subagent's tool rounds"}},"required":["goal"]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "recall_memory".to_owned(),
            description: "Search long-term memory for facts saved in earlier conversations: preferences, constraints, decisions, identifiers. Use when the task references something the user established before ('the usual carrier', 'my reference code'). Returns matching memories or an honest no_memories status — treat an empty result as 'not recorded', never as proof it was never said.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"query":{"type":"string","description":"What to look for"},"limit":{"type":"integer","minimum":1,"maximum":20,"description":"Max memories to return (default 5)"}},"required":["query"]}"#.to_owned(),
        },
        // Recovering a skill the per-prompt budget cut. `skill_budget` degrades
        // before dropping and marks what it cut — honest, and until now
        // unrecoverable. Offered in BOTH loops because the budget is shared: a
        // deployed agent must not be left reading half a rule that chat could
        // have read whole.
        pb::ToolDefinition {
            name: "reattach_skill".to_owned(),
            description: "Read one of this organization's skill instructions back IN FULL. Use it when a skill block in your context ends with a truncation marker, or when a rule you are about to follow looks cut off — acting on half an instruction is worse than pausing to read the rest. Give the skill's name exactly as it appears in the block.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"name":{"type":"string","description":"The skill's name, exactly as shown in its block"}},"required":["name"]}"#.to_owned(),
        },
        // Reading back what a delegation concluded (harness-adoption 1.1, cold
        // resume). Offered here for the same reason as everything above — the
        // purpose-lock rejects any tool absent from this list — and offered as a
        // PAIR: the listing is content-free and always available, the read is
        // approval-gated on every posture. The descriptions say so, because a
        // model that does not know the read needs consent will call it
        // reflexively and stall the run on an approval nobody expected.
        pb::ToolDefinition {
            name: "list_subagent_results".to_owned(),
            description: "List the subagent tasks THIS run has delegated, with each one's goal, status, and whether it stored a conclusion. Use it when you have delegated work and cannot see the result — after a restart your own message history no longer holds it, but the delegation records survive. This returns no conclusions, only which ones exist; read one with read_subagent_result.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{}}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "read_subagent_result".to_owned(),
            description: "Read what ONE of this run's delegated subagents concluded, by child_run_id from list_subagent_results. This requires the user's approval every time — the conclusion belongs to the subagent's own run, and bringing it into this conversation is a disclosure the user allows. So call it when the user asks what a delegation found, or when you genuinely need the finding to continue; do not call it speculatively. You may only read this run's own delegations.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"child_run_id":{"type":"string","description":"A child_run_id from list_subagent_results"}},"required":["child_run_id"]}"#.to_owned(),
        },
        // Delegation. Offered so the capability is actually reachable: the
        // purpose-lock rejects any tool absent from this list, so without a
        // definition here a `subagent.*` call could never be dispatched at all.
        // The description states the real contract (isolated context, own
        // budget, no nesting, no approval-gated tools) because a model that
        // over-delegates burns the run's shared round budget.
        pb::ToolDefinition {
            name: "subagent.task".to_owned(),
            description: "Delegate a self-contained sub-task to a subagent that runs its own tool loop with the SAME tools you have, then returns only its final answer. Its work happens in an isolated context, so use it when a sub-task needs many tool calls whose intermediate output you do not need (e.g. 'find every carrier that ships dangerous goods to Svalbard and summarise the cheapest'). Give it one clear, self-contained goal — it cannot see this conversation, cannot ask you questions, and cannot delegate further (nesting is capped at one level). It may ONLY use read/analysis tools: any side-effecting or destructive action (delete, write, send, deploy, book, publish, execute a provider action, drive a browser, run a shell command, ...) is refused outright, even if this run's own permission mode is 'auto' — delegating that kind of step does not skip its gate, it just fails, so do it yourself instead. Its rounds come out of THIS run's budget, so do not delegate work you can do in a call or two yourself.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"goal":{"type":"string","description":"The complete, self-contained task for the subagent, including any context it needs"},"max_rounds":{"type":"integer","minimum":1,"description":"Optional cap on the subagent's tool rounds; capped by this run's remaining budget"}},"required":["goal"]}"#.to_owned(),
        },
    ];
    // `tickets.create` deliberately stays absent here. A compiled adapter or
    // environment configuration is not an actor grant, and the owner catalog
    // still marks Model execution unavailable until a deployment has proven
    // both capability health and the Control/Conversation decision path. When
    // that server-authoritative availability projection is added, it can
    // surface this exact schema. Until then, caller and MCP definitions cannot
    // smuggle this reserved name into the model-facing tool set.
    tools
}

/// Frame a round of tool outcomes as a context message appended to the
/// conversation, so the model can answer from them on the next inference
/// (mirrors `model-gateway::tool_loop::format_tool_context`).
fn format_tool_context(outcomes: &[ToolStepResult]) -> String {
    use std::fmt::Write as _;
    let mut s = String::from(
        "Tool results for your previous request (use these to answer; do not call the same tool \
again unless needed). Treat tool errors and empty results as inconclusive, not as proof a thing \
does not exist:\n",
    );
    for o in outcomes {
        match &o.error {
            Some(e) => {
                let _ = writeln!(s, "- {} → ERROR: {e}", o.name);
            }
            None => {
                let _ = writeln!(
                    s,
                    "- {} → {}",
                    o.name,
                    bounded_tool_context(&o.output, MAX_TOOL_CONTEXT_CHARS)
                );
            }
        }
    }
    s
}

/// Record a NON-TERMINAL per-tool step (`status="running"`) with the GDPR audit
/// detail (`data_category`, `zdr`) prefixed onto the output, so session-core's
/// `STEP_COMPLETED` event carries the classification. Best-effort: a failure to
/// record a progress step never fails the run.
async fn record_tool_step(
    session_channel: &Channel,
    run_id: &str,
    step_id: &str,
    tool_name: &str,
    outcome: &StepOutcome,
    zdr: bool,
    bearer: Option<&str>,
) {
    // The prefix is the execution-core → session-core metadata side channel for
    // the tool_action audit row. `tool=<name>` carries the human-readable tool
    // NAME so the audit details.tool is the name, not the opaque provider call id
    // that the step_id suffix uses (E5). Tool identifiers never contain spaces,
    // so the space-delimited prefix stays parseable.
    let detail = format!(
        "[data_category={} zdr={zdr} tool={tool_name}] ",
        data_category(tool_name)
    );
    let (output, error) = if zdr {
        // ZDR persistence is classification/audit metadata only. The borrowed
        // `outcome` remains untouched and is still fed ephemerally to the next
        // inference round; only the durable Session Core payload is redacted.
        let result = if outcome.error.is_empty() {
            "success"
        } else {
            "error"
        };
        let redacted = format!("{detail}[content_redacted=true result={result}]");
        if outcome.error.is_empty() {
            (redacted, String::new())
        } else {
            (redacted.clone(), redacted)
        }
    } else if outcome.error.is_empty() {
        (
            format!("{detail}{}", truncate(&outcome.output, OUTPUT_TRUNCATE)),
            String::new(),
        )
    } else {
        (detail.clone(), format!("{detail}{}", outcome.error))
    };
    let request = authenticated_session_request(
        pb::CompleteStepRequest {
            run_id: run_id.to_owned(),
            step_id: step_id.to_owned(),
            // NON-TERMINAL: must not be "completed"/"failed" or session-core
            // would flip the run terminal mid-loop.
            status: "running".to_owned(),
            output,
            error,
            terminal: false,
        },
        bearer,
    );
    let result = match request {
        Ok(request) => {
            SessionCoreClient::new(session_channel.clone())
                .complete_step(request)
                .await
        }
        Err(error) => Err(error),
    };
    if let Err(rpc_error) = result {
        warn!(
            run_id = %run_id,
            step_id = %step_id,
            error = %rpc_error,
            "run_agent: record_tool_step (non-terminal) failed (best-effort)"
        );
    }
}

/// HITL pause: mint a durable approval for the gated tool (same shape as the
/// `ExecuteStep` RPC), then flip the run `AwaitingApproval` and return an
/// `awaiting_approval` response. A persistence failure returns `unavailable`
/// before either state change or pause event. Deliberately NO terminal
/// `CompleteStep` and NO plan COMPLETED/FAILED transition — a durable run is
/// paused, not finished; resume re-invokes `run_agent`.
/// Which kind of approval a pause is actually asking for.
///
/// `Destructive` is the honest label for the gate this loop was built around — a
/// real side effect held behind the `ask` posture. It is the wrong label for
/// `permission::requires_consent_to_disclose`, where nothing is destroyed and
/// what the person is being asked is whether content may cross into this
/// conversation. `Permission` is the enum's own name for exactly that.
pub(crate) fn approval_kind_for(tool_name: &str) -> pb::ApprovalKind {
    if crate::permission::requires_consent_to_disclose(tool_name) {
        pb::ApprovalKind::Permission
    } else {
        pb::ApprovalKind::Destructive
    }
}

/// The sentence the person reads before deciding.
///
/// "tool 'X' requires approval" describes the mechanism, not the choice. For a
/// disclosure it says what would be disclosed and where it would go, because
/// that is the whole content of the decision.
pub(crate) fn approval_reason_for(tool_name: &str) -> String {
    if crate::permission::requires_consent_to_disclose(tool_name) {
        return format!(
            "'{tool_name}' would bring what a delegated subagent concluded into this \
             conversation. Approve to let the agent read that finding."
        );
    }
    format!("tool '{tool_name}' requires approval")
}

async fn pause_for_approval(
    state: &crate::state::StateStore,
    session_channel: &Channel,
    req: &pb::RunAgentRequest,
    step_id: &str,
    tool_name: &str,
    tool_input: &str,
    permission_mode: &str,
    rounds_executed: u32,
    bearer: Option<&str>,
) -> Result<pb::RunAgentResponse, tonic::Status> {
    let continuation_descriptor_json =
        continuation_descriptor(req, step_id, tool_name, tool_input, permission_mode).await?;
    let request = authenticated_session_request(
        pb::CreateApprovalRequest {
            run_id: req.run_id.clone(),
            step_id: step_id.to_owned(),
            // Every pause used to report itself as DESTRUCTIVE. That is true of
            // the gate this function was written for (a real side effect behind
            // `ask`), and false of a consent gate: telling someone that reading
            // a finding is a destructive operation is how a prompt stops meaning
            // anything. The kind and the reason both say which one this is.
            kind: approval_kind_for(tool_name) as i32,
            requested_of: req.org_id.clone(),
            org_id: req.org_id.clone(),
            user_id: req.user_id.clone(),
            reason: approval_reason_for(tool_name),
            expires_in_seconds: 3600,
            // Let session-core mint the durable approval id (matrix §4.1).
            client_approval_id: String::new(),
            // Stable per-(run, step) idempotency key (D-1): a re-paused step
            // collapses onto the existing durable approval instead of creating
            // a duplicate via the (org_id, idempotency_key) ON CONFLICT guard.
            idempotency_key: format!("{}:{step_id}", req.run_id),
            continuation_descriptor_json,
        },
        bearer,
    );
    let request = request?;
    OrchestrationCoreServiceClient::new(session_channel.clone())
        .create_approval(request)
        .await
        .map_err(|error| {
            warn!(
                run_id = %req.run_id,
                code = ?error.code(),
                "run_agent: durable approval persistence unavailable"
            );
            tonic::Status::unavailable("approval persistence unavailable")
        })?;

    let mut snapshot = state.get_or_create(&req.run_id);
    snapshot.status = crate::state::RunStatus::AwaitingApproval;
    state.update(snapshot);

    info!(
        run_id = %req.run_id,
        tool = %tool_name,
        "run_agent: run paused for approval (HITL)"
    );

    Ok(pb::RunAgentResponse {
        status: "awaiting_approval".to_owned(),
        final_output: String::new(),
        rounds_executed,
        grounded: false,
        // A pause is not a finish. `pause_for_approval` builds this response from
        // outside the loop and so cannot see its compaction state; the resumed
        // run reports its own. Reporting `true` here from nowhere would be worse
        // than reporting nothing.
        compaction_triggered: false,
    })
}

/// Build the immutable, credential-free description of the exact tool call
/// that was paused. This is action data, never reusable authority: a later
/// delivery worker must still obtain its own narrowly scoped one-time
/// continuation capability before it may execute it.
async fn continuation_descriptor(
    req: &pb::RunAgentRequest,
    step_id: &str,
    tool_name: &str,
    tool_input: &str,
    permission_mode: &str,
) -> Result<String, tonic::Status> {
    if req.zdr {
        return Err(tonic::Status::failed_precondition(
            "ZDR runs cannot persist an approval continuation descriptor",
        ));
    }
    // A server-resolved owner action is approval-only even when the outer run
    // was requested with `auto`. Preserve that stricter semantic in the
    // descriptor/fingerprint so a future continuation worker cannot mistake
    // the retained request for an auto-approved generic tool call.
    let permission_mode = if crate::permission::requires_durable_owner_approval(tool_name) {
        "ask"
    } else {
        permission_mode
    };
    let mut input: JsonValue = serde_json::from_str(tool_input).map_err(|_| {
        tonic::Status::invalid_argument(
            "approval-gated tool input must be valid JSON before it can be persisted",
        )
    })?;
    // `book_shipment`'s `booked_by` is never a model-supplied argument — it is
    // the run's own acting user, injected the same way `execute_book_shipment`
    // injects it for the live/same-session path. The cold-resume path
    // (`approval_delivery_worker`) deserializes THIS persisted descriptor
    // directly into `BookInput`, whose `booked_by` field is required, so it
    // must be captured here too or every book_shipment continuation fails
    // closed with `invalid_continuation`.
    if tool_name == "book_shipment" {
        if let Some(object) = input.as_object_mut() {
            let has_booked_by = object
                .get("booked_by")
                .and_then(JsonValue::as_str)
                .is_some_and(|value| !value.trim().is_empty());
            if !has_booked_by {
                object.insert("booked_by".to_owned(), json!(req.user_id));
            }
        }
        enrich_shipment_recipient_contact(&mut input, &req.user_id).await;
    }
    let scope = json!({
        "version": 1,
        "run_id": req.run_id,
        "org_id": req.org_id,
        "user_id": req.user_id,
        "step_id": step_id,
        "action_kind": "tool_call",
        "tool_name": tool_name,
        "input": input.clone(),
        "permission_mode": permission_mode,
    });
    let canonical = serde_json::to_string(&scope)
        .map_err(|_| tonic::Status::internal("could not encode continuation descriptor"))?;
    let action_fingerprint = blake3::hash(canonical.as_bytes()).to_hex().to_string();
    let ticket_binding = if tool_name == crate::ticket_tools::TOOL_NAME {
        Some(
            crate::ticket_tools::continuation_binding(
                &serde_json::to_string(&input).map_err(|_| {
                    tonic::Status::internal("could not encode ticket continuation input")
                })?,
                &req.run_id,
                &req.org_id,
                step_id,
            )
            .map_err(|_| {
                tonic::Status::invalid_argument("tickets.create continuation input is invalid")
            })?,
        )
    } else {
        None
    };
    let mut descriptor = json!({
        "version": 1,
        "run_id": req.run_id,
        "org_id": req.org_id,
        "user_id": req.user_id,
        "step_id": step_id,
        "action_kind": "tool_call",
        "tool_name": tool_name,
        "input": input,
        "permission_mode": permission_mode,
        "action_fingerprint": action_fingerprint,
    });
    if let Some((schema_sha256, payload_sha256, ticket_idempotency_key)) = ticket_binding {
        let object = descriptor
            .as_object_mut()
            .expect("continuation descriptor is an object");
        object.insert("schema_sha256".to_owned(), json!(schema_sha256));
        object.insert("payload_sha256".to_owned(), json!(payload_sha256));
        object.insert(
            "ticket_idempotency_key".to_owned(),
            json!(ticket_idempotency_key),
        );
        object.insert("owner_user_id".to_owned(), json!(req.user_id));
    }
    serde_json::to_string(&descriptor)
        .map_err(|_| tonic::Status::internal("could not encode continuation descriptor"))
}

/// Fills in `input.to`'s carrier-notification contact for `book_shipment`
/// when the model supplied neither phone nor email — some carriers (Bring
/// included) reject a booking outright without one on the recipient, and
/// the model has no reason to know either for a recipient it has never
/// met. Falls back to the RUN'S OWN acting user's contact from user-core:
/// correct for this system's actual use today (the acting user is the
/// real recipient for every booking this integration places) and,
/// pragmatically, the only contact this system can vouch for without
/// inventing one. Best-effort throughout — a missing client or a failed
/// lookup leaves the input exactly as it was; it never blocks the pause
/// on this alone.
async fn enrich_shipment_recipient_contact(input: &mut JsonValue, user_id: &str) {
    let Some(to) = input.get_mut("to").and_then(JsonValue::as_object_mut) else {
        return;
    };
    let has_contact = ["phone", "email"].iter().any(|field| {
        to.get(*field)
            .and_then(JsonValue::as_str)
            .is_some_and(|value| !value.trim().is_empty())
    });
    if has_contact {
        return;
    }
    let Some(client) = crate::user_core_client::UserCoreClient::from_env() else {
        return;
    };
    match client.get_contact(user_id).await {
        Ok(contact) => {
            if let Some(email) = contact.email {
                to.insert("email".to_owned(), json!(email));
            }
            if let Some(phone) = contact.phone {
                to.insert("phone".to_owned(), json!(phone));
            }
        }
        Err(error) => {
            warn!(%error, "book_shipment: could not enrich recipient contact from user-core");
        }
    }
}

/// Renew the server-owned terminalization deadline before another agent round.
/// The producer cannot choose a deadline or carry any content in this request.
async fn heartbeat_managed_agent_run(
    session_channel: &Channel,
    req: &pb::RunAgentRequest,
    terminal_tokens: &dyn ManagedRunTokenProvider,
) -> Result<(), tonic::Status> {
    let token = terminal_tokens
        .heartbeat_token(&req.org_id)
        .await
        .map_err(|error| {
            warn!(run_id = %req.run_id, %error, "run_agent: managed heartbeat token unavailable");
            tonic::Status::unavailable("managed terminalization heartbeat unavailable")
        })?;
    ManagedRunLifecycleClient::new(session_channel.clone())
        .heartbeat_managed_run(authenticated_session_request(
            pb::HeartbeatManagedRunRequest {
                run_id: req.run_id.clone(),
                source: pb::ManagedRunSource::ExecutionAgent as i32,
            },
            Some(&token),
        )?)
        .await
        .map_err(|error| {
            warn!(run_id = %req.run_id, code = ?error.code(), "run_agent: managed heartbeat rejected");
            tonic::Status::unavailable("managed terminalization heartbeat unavailable")
        })?;
    Ok(())
}

/// Record a terminal receipt before any local state or successful response is
/// exposed. Session Core derives run owner/ZDR from `run_id`; this request has
/// no output, prompt, tool payload, or arbitrary error fields.
async fn record_managed_agent_terminal_outcome(
    session_channel: &Channel,
    req: &pb::RunAgentRequest,
    success: bool,
    terminal_tokens: &dyn ManagedRunTokenProvider,
) -> Result<(), tonic::Status> {
    let token = terminal_tokens
        .terminalize_token(&req.org_id)
        .await
        .map_err(|error| {
            warn!(run_id = %req.run_id, %error, "run_agent: managed terminal token unavailable");
            tonic::Status::unavailable("managed terminalization unavailable")
        })?;
    ManagedRunLifecycleClient::new(session_channel.clone())
        .record_terminal_outcome(authenticated_session_request(
            pb::RecordTerminalOutcomeRequest {
                run_id: req.run_id.clone(),
                source: pb::ManagedRunSource::ExecutionAgent as i32,
                outcome: if success {
                    pb::TerminalOutcome::Completed as i32
                } else {
                    pb::TerminalOutcome::Failed as i32
                },
                failure_code: if success {
                    String::new()
                } else {
                    "execution_failed".to_owned()
                },
            },
            Some(&token),
        )?)
        .await
        .map_err(|error| {
            warn!(run_id = %req.run_id, code = ?error.code(), "run_agent: managed terminal receipt rejected");
            tonic::Status::unavailable("managed terminalization unavailable")
        })?;
    Ok(())
}

/// Persist the answer when retention permits, obtain the immutable managed
/// terminal receipt, then transition local projection state. In ZDR mode the
/// answer remains only in the returned response; Session Core receives
/// content-free metadata. No successful response or local terminal state is
/// exposed if the receipt cannot be obtained.
#[allow(clippy::too_many_arguments)]
async fn finalize(
    state: &crate::state::StateStore,
    session_channel: &Channel,
    plan_id: &str,
    req: &pb::RunAgentRequest,
    answer: &str,
    success: bool,
    rounds_executed: u32,
    grounded: bool,
    // Whether the loop cleared tool-result payloads to stay inside the context
    // window. Reported on the response because compaction is lossy and
    // otherwise invisible: an answer built on a compacted prompt can be worse
    // for a reason nothing else in the response explains.
    compaction_triggered: bool,
    bearer: Option<&str>,
    terminal_tokens: &dyn ManagedRunTokenProvider,
) -> Result<pb::RunAgentResponse, tonic::Status> {
    let mut session = SessionCoreClient::new(session_channel.clone());
    let mut terminal_success = success;
    let mut final_answer = answer.to_owned();

    // 3. Persist the assistant answer only when retention permits it. An
    // append failure becomes a durable failed receipt, never a success.
    if !req.zdr {
        let append_request = authenticated_session_request(
            pb::AppendMessageRequest {
                thread_id: req.thread_id.clone(),
                role: "assistant".to_owned(),
                content: answer.to_owned(),
                metadata: None,
                ..Default::default()
            },
            bearer,
        );
        let append_result = match append_request {
            Ok(request) => session.append_message(request).await,
            Err(error) => Err(error),
        };
        if let Err(error) = append_result {
            warn!(
                run_id = %req.run_id,
                error = %error,
                "run_agent: append_message(assistant) failed; terminalizing as failed"
            );
            terminal_success = false;
            final_answer = GRACEFUL_FAILURE_REPLY.to_owned();
        }
    }

    // 4. Session Core owns the terminal run + plan transaction. On failure do
    // not mutate StateStore or return a terminal RunAgent response.
    record_managed_agent_terminal_outcome(session_channel, req, terminal_success, terminal_tokens)
        .await?;

    let status = if terminal_success {
        "completed"
    } else {
        "failed"
    };

    // 5. Keep the existing orchestration event projection for observers after
    // Session Core's durable plan update has succeeded. It is no longer the
    // authority for the terminal transition.
    let to = if terminal_success {
        pb::PlanState::Completed
    } else {
        pb::PlanState::Failed
    };
    publish_plan_transition(
        session_channel,
        plan_id,
        &req.run_id,
        pb::PlanState::Executing,
        to,
        bearer,
    )
    .await;

    // 6. Only after the receipt is durable may the in-memory StateStore expose
    // a terminal projection.
    let mut snapshot = state.get_or_create(&req.run_id);
    snapshot.status = if terminal_success {
        crate::state::RunStatus::Completed
    } else {
        crate::state::RunStatus::Failed
    };
    snapshot.last_error = if terminal_success {
        None
    } else {
        Some("execution_failed".to_owned())
    };
    state.update(snapshot);

    info!(
        run_id = %req.run_id,
        status = %status,
        "run_agent: run finalized"
    );

    Ok(pb::RunAgentResponse {
        status: status.to_owned(),
        final_output: final_answer,
        rounds_executed,
        grounded,
        compaction_triggered,
    })
}

/// Publish a `PlanTransitioned` orchestration event on session-core's broadcast
/// (the same `RecordOrchestrationEvent` path `browser_events.rs` uses).
/// Best-effort: a publish failure is logged, never propagated.
async fn publish_plan_transition(
    session_channel: &Channel,
    plan_id: &str,
    run_id: &str,
    from: pb::PlanState,
    to: pb::PlanState,
    bearer: Option<&str>,
) {
    let event = pb::orchestration_event::Event::PlanTransitioned(
        pb::orchestration_event::PlanTransitioned {
            plan_id: plan_id.to_owned(),
            run_id: run_id.to_owned(),
            from: from as i32,
            to: to as i32,
        },
    );
    let request = pb::RecordOrchestrationEventRequest {
        event: Some(pb::OrchestrationEvent {
            // session-core assigns event_id + at.
            event_id: String::new(),
            at: None,
            event: Some(event),
        }),
    };
    let request = authenticated_session_request(request, bearer);
    let result = match request {
        Ok(request) => {
            OrchestrationCoreServiceClient::new(session_channel.clone())
                .record_orchestration_event(request)
                .await
        }
        Err(error) => Err(error),
    };
    if let Err(error) = result {
        warn!(
            run_id = %run_id,
            error = %error,
            "run_agent: failed to publish PlanTransitioned (best-effort)"
        );
    }
}

#[allow(clippy::result_large_err)]
fn authenticated_session_request<T>(
    value: T,
    bearer: Option<&str>,
) -> Result<tonic::Request<T>, tonic::Status> {
    let bearer = bearer
        .ok_or_else(|| tonic::Status::unauthenticated("verified session credential required"))?;
    let mut request = tonic::Request::new(value);
    request.metadata_mut().insert(
        "authorization",
        format!("Bearer {bearer}").parse().map_err(|_| {
            tonic::Status::internal("verified session credential is not forwardable")
        })?,
    );
    Ok(request)
}

/// Truncate to at most `max` chars on a char boundary.
fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_owned();
    }
    value.chars().take(max).collect()
}

/// Bound a tool result headed for the MODEL, saying in words that it is partial.
///
/// Distinct from [`truncate`], which also feeds the durable step record where a
/// prose marker would be noise. A bare clip is actively misleading here: the
/// model cannot tell a complete result from a clipped one and will summarize
/// half a page of ERP rows as though it were the whole answer.
fn bounded_tool_context(output: &str, max: usize) -> String {
    if output.chars().count() <= max {
        return output.to_owned();
    }
    let kept: String = output.chars().take(max).collect();
    format!(
        "{kept}\n[truncated: result exceeded {max} characters and is INCOMPLETE. Do not treat \
         this as the full result set. To see the rest, narrow the request — filter harder, \
         request fewer fields, or page through it.]"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_terminal_auth::SessionTerminalTokenError;
    use mp_contracts::model_plane::v1::{
        inference_core_server::{InferenceCore, InferenceCoreServer},
        managed_run_lifecycle_server::{ManagedRunLifecycle, ManagedRunLifecycleServer},
        orchestration_core_service_server::{
            OrchestrationCoreService, OrchestrationCoreServiceServer,
        },
        session_core_server::{SessionCore, SessionCoreServer},
    };
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use tokio::net::TcpListener;
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::{
        transport::{Endpoint, Server},
        Request, Response, Status,
    };

    #[tokio::test]
    async fn continuation_descriptor_binds_the_exact_action_and_refuses_zdr() {
        let request = pb::RunAgentRequest {
            run_id: "run-1".to_owned(),
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
            ..Default::default()
        };
        let raw = continuation_descriptor(
            &request,
            "step-1",
            "execute_provider_action",
            r#"{"operation":"reply","message":"Hei"}"#,
            "ask",
        )
        .await
        .expect("retained run descriptor");
        let descriptor: serde_json::Value = serde_json::from_str(&raw).expect("descriptor json");
        assert_eq!(descriptor["run_id"], "run-1");
        assert_eq!(descriptor["org_id"], "org-1");
        assert_eq!(descriptor["user_id"], "user-1");
        assert_eq!(descriptor["tool_name"], "execute_provider_action");
        assert_eq!(descriptor["input"]["operation"], "reply");
        assert_eq!(
            descriptor["action_fingerprint"].as_str().map(str::len),
            Some(64)
        );

        let zdr = pb::RunAgentRequest {
            zdr: true,
            ..request
        };
        assert_eq!(
            continuation_descriptor(&zdr, "step-1", "tool", "{}", "ask")
                .await
                .expect_err("ZDR cannot persist a continuation")
                .code(),
            tonic::Code::FailedPrecondition
        );
    }

    /// `tickets.create` is the one tool `permission::requires_durable_owner_approval`
    /// classifies as owner-approval-only, so its descriptor must come back `ask`
    /// even when the outer run asked for `auto`. The input has to be a VALID
    /// `tickets.create` payload (`conversation_id` is required, both by the
    /// frozen `TICKET_CREATE_MODEL_PARAMETERS_JSON` schema and by
    /// `ticket_tools::parse_arguments`) — with `{}` the ticket continuation
    /// binding rejects it as `invalid_argument` and the permission assertion
    /// below is never reached.
    #[tokio::test]
    async fn owner_action_descriptor_cannot_preserve_auto_permission() {
        let request = pb::RunAgentRequest {
            run_id: "run-1".to_owned(),
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
            ..Default::default()
        };
        let raw = continuation_descriptor(
            &request,
            "step-1",
            crate::ticket_tools::TOOL_NAME,
            r#"{"conversation_id":"conv_1"}"#,
            "auto",
        )
        .await
        .expect("retained owner-action descriptor");
        let descriptor: serde_json::Value = serde_json::from_str(&raw).expect("descriptor json");
        assert_eq!(
            descriptor["permission_mode"], "ask",
            "a later continuation must retain the owner-required approval posture"
        );
    }

    /// `booked_by` is never a model-supplied argument (the model has no
    /// reason to know its own run's acting user id), so the persisted
    /// descriptor must inject it from `req.user_id` itself — otherwise the
    /// cold-resume path (`approval_delivery_worker::parse_resumable_action`,
    /// which deserializes this exact JSON into `BookInput`, a struct where
    /// `booked_by` is a required field) fails closed with
    /// `invalid_continuation` even though the live/same-session resume path
    /// works fine (it injects `booked_by` separately, in-memory, in
    /// `execute_book_shipment`).
    #[tokio::test]
    async fn book_shipment_descriptor_injects_booked_by_from_the_run_user() {
        let request = pb::RunAgentRequest {
            run_id: "run-1".to_owned(),
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
            ..Default::default()
        };
        let raw = continuation_descriptor(
            &request,
            "step-1",
            "book_shipment",
            r#"{"carrier_code":"bring","service_name":"Standard","price_amount_cents":1000,"price_currency":"NOK","from":{"name":"A","postal_code":"0001","city":"Oslo","country":"NO"},"to":{"name":"B","postal_code":"7010","city":"Trondheim","country":"NO"},"weight_kg":1.0,"length_cm":10.0,"width_cm":10.0,"height_cm":10.0}"#,
            "ask",
        )
        .await
        .expect("retained run descriptor");
        let descriptor: serde_json::Value = serde_json::from_str(&raw).expect("descriptor json");
        assert_eq!(descriptor["input"]["booked_by"], "user-1");
    }

    #[tokio::test]
    async fn book_shipment_descriptor_preserves_an_explicit_booked_by() {
        let request = pb::RunAgentRequest {
            run_id: "run-1".to_owned(),
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
            ..Default::default()
        };
        let raw = continuation_descriptor(
            &request,
            "step-1",
            "book_shipment",
            r#"{"carrier_code":"bring","service_name":"Standard","price_amount_cents":1000,"price_currency":"NOK","from":{"name":"A","postal_code":"0001","city":"Oslo","country":"NO"},"to":{"name":"B","postal_code":"7010","city":"Trondheim","country":"NO"},"weight_kg":1.0,"length_cm":10.0,"width_cm":10.0,"height_cm":10.0,"booked_by":"someone-else"}"#,
            "ask",
        )
        .await
        .expect("retained run descriptor");
        let descriptor: serde_json::Value = serde_json::from_str(&raw).expect("descriptor json");
        assert_eq!(descriptor["input"]["booked_by"], "someone-else");
    }

    #[test]
    fn canonical_retrieval_signature_detects_exact_duplicate_queries() {
        let first = retrieval_signature(
            "knowledge_search",
            r#"{"query":" Quarterly   Revenue ","top_k":5}"#,
        )
        .expect("knowledge search signature");
        let same = retrieval_signature(
            "knowledge_search",
            r#"{"top_k":5,"query":"quarterly revenue","route":"hybrid"}"#,
        )
        .expect("knowledge search signature");
        let broadened = retrieval_signature(
            "knowledge_search",
            r#"{"query":"quarterly revenue by region","top_k":5}"#,
        )
        .expect("knowledge search signature");

        assert_eq!(first, same);
        assert_ne!(first, broadened);
    }

    #[test]
    fn duplicate_tracking_applies_only_to_valid_knowledge_retrieval() {
        assert!(retrieval_signature("web_search", r#"{"query":"q"}"#).is_none());
        assert!(retrieval_signature("knowledge_search", "not-json").is_none());
        assert!(
            retrieval_signature("knowledge_search", r#"{"query":"q","route":"graph"}"#).is_none()
        );
    }

    #[test]
    fn untrusted_tool_definitions_cannot_advertise_ticket_create_before_a_server_resolver_exists() {
        let mut tools = offered_tool_defs();
        assert!(
            !tools
                .iter()
                .any(|tool| tool.name.trim() == crate::ticket_tools::TOOL_NAME),
            "the static catalog must not advertise a server-resolved owner action"
        );
        append_untrusted_tool_defs(
            &mut tools,
            &[
                pb::ToolDefinition {
                    name: crate::ticket_tools::TOOL_NAME.to_owned(),
                    description: "forged ticket action".to_owned(),
                    parameters_json: "{}".to_owned(),
                },
                pb::ToolDefinition {
                    name: format!("  {}  ", crate::ticket_tools::TOOL_NAME),
                    description: "forged whitespace ticket action".to_owned(),
                    parameters_json: "{}".to_owned(),
                },
                pb::ToolDefinition {
                    name: "custom.read".to_owned(),
                    description: "caller extension".to_owned(),
                    parameters_json: "{}".to_owned(),
                },
            ],
        );

        assert!(
            !tools
                .iter()
                .any(|tool| tool.name.trim() == crate::ticket_tools::TOOL_NAME),
            "a caller must not turn a reserved owner action into a model-offered tool"
        );
        assert!(
            tools.iter().any(|tool| tool.name == "custom.read"),
            "the guard must not change the existing extension behavior for non-reserved tools"
        );
    }

    #[test]
    fn only_the_server_resolved_view_may_offer_ticket_create() {
        let mut tools = offered_tool_defs();
        append_server_resolved_tool_defs(
            &mut tools,
            &[pb::ToolDefinition {
                name: crate::ticket_tools::TOOL_NAME.to_owned(),
                description: "Control-resolved ticket action".to_owned(),
                parameters_json: r#"{"type":"object","required":["conversation_id"]}"#.to_owned(),
            }],
        );
        assert!(tools
            .iter()
            .any(|tool| tool.name == crate::ticket_tools::TOOL_NAME));

        let before = tools.len();
        append_server_resolved_tool_defs(
            &mut tools,
            &[pb::ToolDefinition {
                name: "tickets.create.alias".to_owned(),
                description: "forged alias".to_owned(),
                parameters_json: "{}".to_owned(),
            }],
        );
        assert_eq!(
            tools.len(),
            before,
            "resolver may not widen its fixed catalog"
        );
    }

    struct TicketViewPolicy;

    #[tonic::async_trait]
    impl crate::capability_policy::CapabilityPolicy for TicketViewPolicy {
        async fn evaluate(
            &self,
            _tool_name: &str,
            _run_id: &str,
            _org_id: &str,
        ) -> Result<crate::capability_policy::CapabilityDecision, Status> {
            Ok(crate::capability_policy::CapabilityDecision::Allow)
        }

        async fn resolve_server_tool_definitions(
            &self,
            run_id: &str,
            org_id: &str,
        ) -> Result<Vec<pb::ToolDefinition>, Status> {
            if run_id != "run-1" || org_id != "org-1" {
                return Err(Status::permission_denied(
                    "unexpected model action view binding",
                ));
            }
            Ok(vec![pb::ToolDefinition {
                name: crate::ticket_tools::TOOL_NAME.to_owned(),
                description: "Control-resolved ticket action".to_owned(),
                parameters_json: crate::ticket_tools::TICKET_CREATE_MODEL_PARAMETERS_JSON
                    .to_owned(),
            }])
        }
    }

    #[tokio::test]
    async fn merged_tool_defs_accepts_ticket_only_from_the_run_bound_policy_view() {
        let tools = merged_tool_defs("run-1", "org-1", "user-1", &[], &TicketViewPolicy).await;
        assert!(tools
            .iter()
            .any(|tool| tool.name == crate::ticket_tools::TOOL_NAME));
    }

    #[test]
    fn knowledge_search_found_grounding_requires_status_ok() {
        assert!(knowledge_search_found_grounding(
            r#"{"status":"ok","result_count":2}"#
        ));
        assert!(!knowledge_search_found_grounding(
            r#"{"status":"no_results","result_count":0}"#
        ));
        assert!(!knowledge_search_found_grounding(
            r#"{"status":"low_confidence","result_count":1}"#
        ));
        assert!(!knowledge_search_found_grounding(
            r#"{"status":"degraded","result_count":0}"#
        ));
        assert!(!knowledge_search_found_grounding("not-json"));
    }

    type InferStream = Pin<Box<dyn futures::Stream<Item = Result<pb::InferChunk, Status>> + Send>>;
    type VideoStream = Pin<
        Box<
            dyn futures::Stream<Item = Result<pb::StreamVideoGenerationContentResponse, Status>>
                + Send,
        >,
    >;
    type ReplayStream = Pin<Box<dyn futures::Stream<Item = Result<pb::Event, Status>> + Send>>;
    type RunEventStream =
        Pin<Box<dyn futures::Stream<Item = Result<pb::OrchestrationEvent, Status>> + Send>>;

    struct AllowCapabilityPolicy;

    #[tonic::async_trait]
    impl crate::capability_policy::CapabilityPolicy for AllowCapabilityPolicy {
        async fn evaluate(
            &self,
            _tool_name: &str,
            _run_id: &str,
            _org_id: &str,
        ) -> Result<crate::capability_policy::CapabilityDecision, Status> {
            Ok(crate::capability_policy::CapabilityDecision::Allow)
        }
    }

    static ALLOW_CAPABILITY_POLICY: AllowCapabilityPolicy = AllowCapabilityPolicy;

    /// Records the calls the driver made into Session Core / orchestration, so
    /// tests can distinguish non-terminal legacy tool audit steps from the
    /// immutable managed terminal receipt.
    #[derive(Default)]
    struct Recorder {
        appended_assistant: Vec<String>,
        completed: Vec<(String, String)>, // (step_id, status)
        persisted_step_payloads: Vec<(String, String, String)>, // (step_id, output, error)
        managed_terminal_outcomes: Vec<(String, i32, i32, String)>, // (run_id, source, outcome, failure_code)
        managed_heartbeats: Vec<(String, i32)>,                     // (run_id, source)
        /// `(run_id, output)` for each `RecordRunOutput`. Separate from
        /// `managed_terminal_outcomes` on purpose: a test must be able to assert
        /// that a receipt was written and an answer was NOT (the ZDR case).
        recorded_run_outputs: Vec<(String, String)>,
        /// Make `RecordRunOutput` fail, to prove a delegation still returns its
        /// answer when the durable copy could not be written.
        fail_run_output: bool,
        /// Status `MockRunService` reports for a child run. `completed` unless a
        /// replay test needs to prove the still-in-flight refusal.
        replayed_run_status: Option<String>,
        plan_transitions: Vec<(i32, i32)>, // (from, to)
        approvals: Vec<(String, String)>,  // (step_id, reason)
        decisions: Vec<(String, i32)>,     // (approval_id, decision) — DecideApproval
        /// Fixture for `MockSession::list_agent_skills`; empty unless a
        /// `fetch_skill_context` test populates it.
        agent_skills: Vec<pb::AgentSkill>,
        /// Delegated child runs registered via `StartManagedRun`:
        /// (parent_run_id, goal, start_key, agent_id).
        started_child_runs: Vec<(String, String, String, String)>,
        /// Lineage edges recorded via `AttachSubagent`:
        /// (parent_run_id, child_run_id, role).
        lineage_edges: Vec<(String, String, i32)>,
        /// When set, `start_managed_run` fails — used to prove a delegation
        /// still completes when its bookkeeping does not.
        fail_child_run_start: bool,
    }

    type SharedRecorder = Arc<Mutex<Recorder>>;

    /// Test-only fixed-scope service credential source. It deliberately does
    /// not accept a caller's delegated session bearer, mirroring production's
    /// distinct Auth Core service credential boundary.
    struct StaticTerminalTokens;

    #[tonic::async_trait]
    impl ManagedRunTokenProvider for StaticTerminalTokens {
        async fn terminalize_token(
            &self,
            _org_id: &str,
        ) -> Result<String, SessionTerminalTokenError> {
            Ok("test-terminalize-service-token".to_owned())
        }

        async fn heartbeat_token(
            &self,
            _org_id: &str,
        ) -> Result<String, SessionTerminalTokenError> {
            Ok("test-heartbeat-service-token".to_owned())
        }

        async fn scheduled_step_token(
            &self,
            _org_id: &str,
        ) -> Result<String, SessionTerminalTokenError> {
            Ok("test-scheduled-step-service-token".to_owned())
        }
    }

    static STATIC_TERMINAL_TOKENS: StaticTerminalTokens = StaticTerminalTokens;

    /// A lazily-connecting client to an address nothing listens on. Every
    /// `run_agent`/`run_agent_with_tools` test below is non-Space (`space_id`
    /// empty on `sample_request()` by default), so `sandbox_lease::
    /// ensure_sandbox_lease` is never reached — this exists only to satisfy
    /// the signature.
    fn test_sandbox_manager_client() -> crate::sandbox_manager_client::SandboxManagerClient {
        crate::sandbox_manager_client::SandboxManagerClient::new(
            tonic::transport::Endpoint::from_shared("http://127.0.0.1:1")
                .expect("valid endpoint")
                .connect_lazy(),
        )
    }

    /// Same reasoning as `test_sandbox_manager_client` above: every test
    /// below is non-Space, so no run ever acquires a lease for `finalize`'s
    /// own release-on-completion call to find, meaning this credential
    /// provider is never actually asked to mint anything.
    fn test_sandbox_tokens() -> crate::sandbox_lease::SandboxManagerTokenProvider {
        crate::sandbox_lease::SandboxManagerTokenProvider::new_for_test(
            "http://127.0.0.1:1",
            "execution-core",
            "test-service-secret-at-least-32-bytes",
        )
    }

    /// One scripted inference outcome the mock returns per round.
    #[derive(Clone)]
    enum Scripted {
        /// A final answer (no tool calls).
        Answer(String),
        /// A round that requests tool calls (model content may accompany them).
        ToolCalls {
            content: String,
            calls: Vec<pb::ToolCall>,
        },
        /// A round that requests tool calls AND reports hitting the output
        /// token ceiling — the truncated-last-call case.
        TruncatedToolCalls {
            content: String,
            calls: Vec<pb::ToolCall>,
        },
        /// An infer transport error.
        Error,
    }

    // --- Inference mock: replays a scripted queue, one entry per round. ---

    type ObservedMessages = Arc<Mutex<Vec<Vec<pb::ChatMessage>>>>;

    type ObservedTiers = Arc<Mutex<Vec<i32>>>;

    struct MockInference {
        script: Mutex<std::collections::VecDeque<Scripted>>,
        /// ZDR flag observed on each `InferRequest`, so a test can assert the
        /// run's `zdr` was threaded through.
        observed_zdr: Arc<Mutex<Vec<bool>>>,
        /// Wire `min_privacy_tier` observed on each `InferRequest`, so a test
        /// can assert the run's privacy floor was threaded through.
        observed_tiers: ObservedTiers,
        /// Message history observed on each `InferRequest`, in call order. A
        /// nested subagent shares this channel, so the recording is also the
        /// evidence that its context was ISOLATED from its parent's.
        observed_messages: ObservedMessages,
    }

    impl MockInference {
        fn new(steps: Vec<Scripted>) -> Self {
            Self {
                script: Mutex::new(steps.into_iter().collect()),
                observed_zdr: Arc::new(Mutex::new(Vec::new())),
                observed_tiers: Arc::new(Mutex::new(Vec::new())),
                observed_messages: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn with_zdr_recorder(steps: Vec<Scripted>, observed_zdr: Arc<Mutex<Vec<bool>>>) -> Self {
            Self {
                script: Mutex::new(steps.into_iter().collect()),
                observed_zdr,
                observed_tiers: Arc::new(Mutex::new(Vec::new())),
                observed_messages: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn with_message_recorder(
            steps: Vec<Scripted>,
            observed_messages: ObservedMessages,
        ) -> Self {
            Self {
                script: Mutex::new(steps.into_iter().collect()),
                observed_zdr: Arc::new(Mutex::new(Vec::new())),
                observed_tiers: Arc::new(Mutex::new(Vec::new())),
                observed_messages,
            }
        }

        fn with_tier_recorder(steps: Vec<Scripted>, observed_tiers: ObservedTiers) -> Self {
            Self {
                script: Mutex::new(steps.into_iter().collect()),
                observed_zdr: Arc::new(Mutex::new(Vec::new())),
                observed_tiers,
                observed_messages: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    #[tonic::async_trait]
    impl InferenceCore for MockInference {
        type InferStreamStream = InferStream;
        type StreamVideoGenerationContentStream = VideoStream;

        async fn infer(
            &self,
            request: Request<pb::InferRequest>,
        ) -> Result<Response<pb::InferResponse>, Status> {
            let observed = request.into_inner();
            self.observed_zdr.lock().unwrap().push(observed.zdr);
            self.observed_tiers
                .lock()
                .unwrap()
                .push(observed.min_privacy_tier);
            self.observed_messages
                .lock()
                .unwrap()
                .push(observed.messages);
            // Default to a plain answer once the script is exhausted, so a loop
            // bug can't hang the test waiting for more rounds.
            let step = self
                .script
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Scripted::Answer("done".to_owned()));
            match step {
                Scripted::Answer(content) => Ok(Response::new(pb::InferResponse {
                    request_id: "req".to_owned(),
                    content,
                    model_used: "mock".to_owned(),
                    stop_reason: "end_turn".to_owned(),
                    input_tokens: 1,
                    output_tokens: 1,
                    tool_calls: Vec::new(),
                    ..Default::default()
                })),
                Scripted::TruncatedToolCalls { content, calls } => {
                    Ok(Response::new(pb::InferResponse {
                        request_id: "req".to_owned(),
                        content,
                        model_used: "mock".to_owned(),
                        // The provider's own signal that generation was cut
                        // off mid-message.
                        stop_reason: "max_tokens".to_owned(),
                        input_tokens: 0,
                        output_tokens: 0,
                        tool_calls: calls,
                        // Provider provenance (`provider_used` / `residency`) is
                        // not what this arm exercises; default it like the
                        // sibling arms instead of listing every field.
                        ..Default::default()
                    }))
                }
                Scripted::ToolCalls { content, calls } => Ok(Response::new(pb::InferResponse {
                    request_id: "req".to_owned(),
                    content,
                    model_used: "mock".to_owned(),
                    stop_reason: "tool_use".to_owned(),
                    input_tokens: 1,
                    output_tokens: 1,
                    tool_calls: calls,
                    ..Default::default()
                })),
                Scripted::Error => Err(Status::unavailable("inference down")),
            }
        }

        async fn infer_stream(
            &self,
            _: Request<pb::InferRequest>,
        ) -> Result<Response<Self::InferStreamStream>, Status> {
            Err(Status::unimplemented("infer_stream not used"))
        }

        async fn create_embedding(
            &self,
            _: Request<pb::CreateEmbeddingRequest>,
        ) -> Result<Response<pb::CreateEmbeddingResponse>, Status> {
            Err(Status::unimplemented("create_embedding not used"))
        }

        async fn list_models(
            &self,
            _: Request<pb::ListModelsRequest>,
        ) -> Result<Response<pb::ListModelsResponse>, Status> {
            Err(Status::unimplemented("list_models not used"))
        }

        async fn synthesize_speech(
            &self,
            _: Request<pb::SynthesizeSpeechRequest>,
        ) -> Result<Response<pb::SynthesizeSpeechResponse>, Status> {
            Err(Status::unimplemented("synthesize_speech not used"))
        }

        async fn transcribe_speech(
            &self,
            _: Request<pb::TranscribeSpeechRequest>,
        ) -> Result<Response<pb::TranscribeSpeechResponse>, Status> {
            Err(Status::unimplemented("transcribe_speech not used"))
        }

        async fn list_speech_voices(
            &self,
            _: Request<pb::ListSpeechVoicesRequest>,
        ) -> Result<Response<pb::ListSpeechVoicesResponse>, Status> {
            Err(Status::unimplemented("list_speech_voices not used"))
        }

        async fn translate_text(
            &self,
            _: Request<pb::TranslateTextRequest>,
        ) -> Result<Response<pb::TranslateTextResponse>, Status> {
            Err(Status::unimplemented("translate_text not used"))
        }

        async fn batch_translate_text(
            &self,
            _: Request<pb::BatchTranslateTextRequest>,
        ) -> Result<Response<pb::BatchTranslateTextResponse>, Status> {
            Err(Status::unimplemented("batch_translate_text not used"))
        }

        async fn detect_text_language(
            &self,
            _: Request<pb::DetectTextLanguageRequest>,
        ) -> Result<Response<pb::DetectTextLanguageResponse>, Status> {
            Err(Status::unimplemented("detect_text_language not used"))
        }

        async fn list_translation_languages(
            &self,
            _: Request<pb::ListTranslationLanguagesRequest>,
        ) -> Result<Response<pb::ListTranslationLanguagesResponse>, Status> {
            Err(Status::unimplemented("list_translation_languages not used"))
        }

        async fn generate_image(
            &self,
            _: Request<pb::GenerateImageRequest>,
        ) -> Result<Response<pb::GenerateImageResponse>, Status> {
            Err(Status::unimplemented("generate_image not used"))
        }

        async fn analyze_image(
            &self,
            _: Request<pb::AnalyzeImageRequest>,
        ) -> Result<Response<pb::AnalyzeImageResponse>, Status> {
            Err(Status::unimplemented("analyze_image not used"))
        }

        async fn extract_image_text(
            &self,
            _: Request<pb::ExtractImageTextRequest>,
        ) -> Result<Response<pb::ExtractImageTextResponse>, Status> {
            Err(Status::unimplemented("extract_image_text not used"))
        }

        async fn analyze_document(
            &self,
            _: Request<pb::AnalyzeDocumentRequest>,
        ) -> Result<Response<pb::AnalyzeDocumentResponse>, Status> {
            Err(Status::unimplemented("analyze_document not used"))
        }

        async fn analyze_language(
            &self,
            _: Request<pb::AnalyzeLanguageRequest>,
        ) -> Result<Response<pb::AnalyzeLanguageResponse>, Status> {
            Err(Status::unimplemented("analyze_language not used"))
        }

        async fn create_realtime_session(
            &self,
            _: Request<pb::CreateRealtimeSessionRequest>,
        ) -> Result<Response<pb::CreateRealtimeSessionResponse>, Status> {
            Err(Status::unimplemented("create_realtime_session not used"))
        }

        async fn create_video_generation_job(
            &self,
            _: Request<pb::CreateVideoGenerationJobRequest>,
        ) -> Result<Response<pb::CreateVideoGenerationJobResponse>, Status> {
            Err(Status::unimplemented(
                "create_video_generation_job not used",
            ))
        }

        async fn get_video_generation_job(
            &self,
            _: Request<pb::GetVideoGenerationJobRequest>,
        ) -> Result<Response<pb::GetVideoGenerationJobResponse>, Status> {
            Err(Status::unimplemented("get_video_generation_job not used"))
        }

        async fn stream_video_generation_content(
            &self,
            _: Request<pb::StreamVideoGenerationContentRequest>,
        ) -> Result<Response<Self::StreamVideoGenerationContentStream>, Status> {
            Err(Status::unimplemented(
                "stream_video_generation_content not used",
            ))
        }
    }

    // --- Session mock: records append_message + complete_step. ---

    struct MockSession {
        rec: SharedRecorder,
    }

    #[tonic::async_trait]
    impl SessionCore for MockSession {
        type ReplayThreadStream = ReplayStream;

        async fn create_thread(
            &self,
            _: Request<pb::CreateThreadRequest>,
        ) -> Result<Response<pb::CreateThreadResponse>, Status> {
            Err(Status::unimplemented("create_thread not used"))
        }

        async fn delete_thread(
            &self,
            _: Request<pb::DeleteThreadRequest>,
        ) -> Result<Response<pb::DeleteThreadResponse>, Status> {
            Err(Status::unimplemented("delete_thread not used"))
        }

        async fn delete_threads(
            &self,
            _: Request<pb::DeleteThreadsRequest>,
        ) -> Result<Response<pb::DeleteThreadsResponse>, Status> {
            Err(Status::unimplemented("delete_threads not used"))
        }

        async fn delete_space_threads(
            &self,
            _: Request<pb::DeleteSpaceThreadsRequest>,
        ) -> Result<Response<pb::DeleteSpaceThreadsResponse>, Status> {
            Err(Status::unimplemented("delete_space_threads not used"))
        }

        async fn prepare_scheduled_run_thread(
            &self,
            _: Request<pb::PrepareScheduledRunThreadRequest>,
        ) -> Result<Response<pb::PrepareScheduledRunThreadResponse>, Status> {
            Err(Status::unimplemented(
                "prepare_scheduled_run_thread not used",
            ))
        }

        async fn start_scheduled_run(
            &self,
            _: Request<pb::StartScheduledRunRequest>,
        ) -> Result<Response<pb::StartRunResponse>, Status> {
            Err(Status::unimplemented("start_scheduled_run not used"))
        }

        async fn claim_scheduled_step(
            &self,
            _: Request<pb::ClaimScheduledStepRequest>,
        ) -> Result<Response<pb::ClaimScheduledStepResponse>, Status> {
            Err(Status::unimplemented("claim_scheduled_step not used"))
        }

        async fn record_scheduled_step_receipt(
            &self,
            _: Request<pb::RecordScheduledStepReceiptRequest>,
        ) -> Result<Response<pb::RecordScheduledStepReceiptResponse>, Status> {
            Err(Status::unimplemented(
                "record_scheduled_step_receipt not used",
            ))
        }

        async fn append_message(
            &self,
            request: Request<pb::AppendMessageRequest>,
        ) -> Result<Response<pb::AppendMessageResponse>, Status> {
            let req = request.into_inner();
            if req.role == "assistant" {
                self.rec
                    .lock()
                    .unwrap()
                    .appended_assistant
                    .push(req.content);
            }
            Ok(Response::new(pb::AppendMessageResponse { sequence: 1 }))
        }

        async fn start_run(
            &self,
            _: Request<pb::StartRunRequest>,
        ) -> Result<Response<pb::StartRunResponse>, Status> {
            Err(Status::unimplemented("start_run not used"))
        }

        async fn complete_step(
            &self,
            request: Request<pb::CompleteStepRequest>,
        ) -> Result<Response<pb::CompleteStepResponse>, Status> {
            let req = request.into_inner();
            let mut recorder = self.rec.lock().unwrap();
            recorder.completed.push((req.step_id.clone(), req.status));
            recorder
                .persisted_step_payloads
                .push((req.step_id, req.output, req.error));
            Ok(Response::new(pb::CompleteStepResponse { step_index: 1 }))
        }

        async fn reserve_tool_action(
            &self,
            _: Request<pb::ReserveToolActionRequest>,
        ) -> Result<Response<pb::ReserveToolActionResponse>, Status> {
            Err(Status::unimplemented("reserve_tool_action not used"))
        }

        async fn finalize_tool_action(
            &self,
            _: Request<pb::FinalizeToolActionRequest>,
        ) -> Result<Response<pb::FinalizeToolActionResponse>, Status> {
            Err(Status::unimplemented("finalize_tool_action not used"))
        }

        async fn save_checkpoint(
            &self,
            _: Request<pb::SaveCheckpointRequest>,
        ) -> Result<Response<pb::SaveCheckpointResponse>, Status> {
            Err(Status::unimplemented("save_checkpoint not used"))
        }

        async fn replay_thread(
            &self,
            _: Request<pb::ReplayThreadRequest>,
        ) -> Result<Response<Self::ReplayThreadStream>, Status> {
            Err(Status::unimplemented("replay_thread not used"))
        }

        async fn get_context_assembly(
            &self,
            _: Request<pb::GetContextAssemblyRequest>,
        ) -> Result<Response<pb::GetContextAssemblyResponse>, Status> {
            Err(Status::unimplemented("get_context_assembly not used"))
        }

        async fn compact_now(
            &self,
            _: Request<pb::CompactNowRequest>,
        ) -> Result<Response<pb::CompactNowResponse>, Status> {
            Err(Status::unimplemented("compact_now not used"))
        }

        async fn set_agent_skill_enabled(
            &self,
            _: Request<pb::SetAgentSkillEnabledRequest>,
        ) -> Result<Response<pb::SetAgentSkillEnabledResponse>, Status> {
            Err(Status::unimplemented("set_agent_skill_enabled not used"))
        }

        async fn upsert_agent_skill(
            &self,
            _: Request<pb::UpsertAgentSkillRequest>,
        ) -> Result<Response<pb::UpsertAgentSkillResponse>, Status> {
            Err(Status::unimplemented("upsert_agent_skill not used"))
        }

        async fn list_agent_skills(
            &self,
            _: Request<pb::ListAgentSkillsRequest>,
        ) -> Result<Response<pb::ListAgentSkillsResponse>, Status> {
            Ok(Response::new(pb::ListAgentSkillsResponse {
                skills: self.rec.lock().unwrap().agent_skills.clone(),
            }))
        }

        async fn list_conversation(
            &self,
            _: Request<pb::ListConversationRequest>,
        ) -> Result<Response<pb::ListConversationResponse>, Status> {
            Err(Status::unimplemented("list_conversation not used"))
        }

        async fn list_threads(
            &self,
            _: Request<pb::ListThreadsRequest>,
        ) -> Result<Response<pb::ListThreadsResponse>, Status> {
            Ok(Response::new(pb::ListThreadsResponse { threads: vec![] }))
        }

        async fn update_thread_presentation(
            &self,
            _: Request<pb::UpdateThreadPresentationRequest>,
        ) -> Result<Response<pb::UpdateThreadPresentationResponse>, Status> {
            Err(Status::unimplemented("update_thread_presentation not used"))
        }

        async fn archive_thread(
            &self,
            _: Request<pb::ArchiveThreadRequest>,
        ) -> Result<Response<pb::ArchiveThreadResponse>, Status> {
            Err(Status::unimplemented("archive_thread not used"))
        }

        async fn archive_threads(
            &self,
            _: Request<pb::ArchiveThreadsRequest>,
        ) -> Result<Response<pb::ArchiveThreadsResponse>, Status> {
            Err(Status::unimplemented("archive_threads not used"))
        }

        async fn set_run_mode(
            &self,
            _: Request<pb::SetRunModeRequest>,
        ) -> Result<Response<pb::SetRunModeResponse>, Status> {
            Err(Status::unimplemented("set_run_mode not used"))
        }
    }

    // --- RunService mock: serves the run rows a REPLAYED delegation reads back.
    //
    // Added because the replay path is the only thing standing between a
    // re-driven parent step and two contradicting truths (a good answer plus a
    // ledger that says the child failed), and it cannot be tested without a run
    // to read.

    struct MockRunService {
        rec: SharedRecorder,
    }

    impl MockRunService {
        fn detail(&self, run_id: &str) -> pb::RunDetail {
            let rec = self.rec.lock().unwrap();
            let final_output = rec
                .recorded_run_outputs
                .iter()
                .find(|(recorded, _)| recorded == run_id)
                .map(|(_, answer)| answer.clone())
                .unwrap_or_default();
            pb::RunDetail {
                run_id: run_id.to_owned(),
                thread_id: sample_request().thread_id,
                parent_run_id: sample_request().run_id,
                agent_id: "task".to_owned(),
                status: rec
                    .replayed_run_status
                    .clone()
                    .unwrap_or_else(|| "completed".to_owned()),
                mode: "execute".to_owned(),
                goal: "the delegated goal".to_owned(),
                final_output,
                ..Default::default()
            }
        }
    }

    #[tonic::async_trait]
    impl pb::run_service_server::RunService for MockRunService {
        async fn get_run(
            &self,
            request: Request<pb::GetRunRequest>,
        ) -> Result<Response<pb::RunDetail>, Status> {
            Ok(Response::new(self.detail(&request.into_inner().run_id)))
        }

        async fn list_runs(
            &self,
            _: Request<pb::ListRunsRequest>,
        ) -> Result<Response<pb::ListRunsResponse>, Status> {
            let ids: Vec<String> = self
                .rec
                .lock()
                .unwrap()
                .recorded_run_outputs
                .iter()
                .map(|(run_id, _)| run_id.clone())
                .collect();
            Ok(Response::new(pb::ListRunsResponse {
                runs: ids.iter().map(|run_id| self.detail(run_id)).collect(),
                has_more: false,
            }))
        }

        async fn get_scheduled_step_context(
            &self,
            _: Request<pb::GetScheduledStepContextRequest>,
        ) -> Result<Response<pb::ScheduledStepContext>, Status> {
            Err(Status::unimplemented("not used"))
        }

        async fn cancel_run(
            &self,
            _: Request<pb::CancelRunRequest>,
        ) -> Result<Response<pb::CancelRunResponse>, Status> {
            Err(Status::unimplemented("not used"))
        }

        async fn list_system_runs(
            &self,
            _: Request<pb::ListSystemRunsRequest>,
        ) -> Result<Response<pb::ListRunsResponse>, Status> {
            Err(Status::unimplemented("not used"))
        }

        async fn resolve_run_owner(
            &self,
            _: Request<pb::ResolveRunOwnerRequest>,
        ) -> Result<Response<pb::ResolveRunOwnerResponse>, Status> {
            Err(Status::unimplemented("not used"))
        }

        async fn resolve_thread_owner(
            &self,
            _: Request<pb::ResolveThreadOwnerRequest>,
        ) -> Result<Response<pb::ResolveThreadOwnerResponse>, Status> {
            Err(Status::unimplemented("not used"))
        }

        async fn resolve_run_action_authority(
            &self,
            _: Request<pb::ResolveRunActionAuthorityRequest>,
        ) -> Result<Response<pb::ResolveRunActionAuthorityResponse>, Status> {
            Err(Status::unimplemented("not used"))
        }

        async fn resolve_scheduled_step_authority(
            &self,
            _: Request<pb::ResolveScheduledStepAuthorityRequest>,
        ) -> Result<Response<pb::ResolveScheduledStepAuthorityResponse>, Status> {
            Err(Status::unimplemented("not used"))
        }
    }

    // --- Managed terminalization mock: records immutable receipt operations. ---

    struct MockManagedRunLifecycle {
        rec: SharedRecorder,
        fail_terminal_receipt: bool,
    }

    #[tonic::async_trait]
    impl ManagedRunLifecycle for MockManagedRunLifecycle {
        async fn start_managed_run(
            &self,
            request: Request<pb::StartManagedRunRequest>,
        ) -> Result<Response<pb::StartManagedRunResponse>, Status> {
            if request
                .metadata()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                != Some("Bearer test-terminalize-service-token")
            {
                return Err(Status::unauthenticated(
                    "managed run start requires its service credential",
                ));
            }
            if self.rec.lock().unwrap().fail_child_run_start {
                return Err(Status::unavailable("managed run start unavailable"));
            }
            let req = request.into_inner();
            // Mirror the real idempotency contract: the same start_key replays
            // the same run id and reports already_started.
            let mut rec = self.rec.lock().unwrap();
            let existing = rec
                .started_child_runs
                .iter()
                .find(|(_, _, key, _)| key == &req.start_key)
                .map(|(_, _, key, _)| format!("child-{key}"));
            let already_started = existing.is_some();
            let run_id = existing.unwrap_or_else(|| format!("child-{}", req.start_key));
            if !already_started {
                rec.started_child_runs.push((
                    req.parent_run_id.clone(),
                    req.goal.clone(),
                    req.start_key.clone(),
                    req.agent_id.clone(),
                ));
            }
            Ok(Response::new(pb::StartManagedRunResponse {
                run_id,
                created_at: None,
                terminal_step_id: "execution-core-agent-final".to_owned(),
                already_started,
                thread_id: req.thread_id,
            }))
        }

        async fn record_terminal_outcome(
            &self,
            request: Request<pb::RecordTerminalOutcomeRequest>,
        ) -> Result<Response<pb::RecordTerminalOutcomeResponse>, Status> {
            if request
                .metadata()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                != Some("Bearer test-terminalize-service-token")
            {
                return Err(Status::unauthenticated(
                    "managed terminal receipt requires its service credential",
                ));
            }
            if self.fail_terminal_receipt {
                return Err(Status::unavailable("managed terminal receipt unavailable"));
            }
            let req = request.into_inner();
            self.rec.lock().unwrap().managed_terminal_outcomes.push((
                req.run_id.clone(),
                req.source,
                req.outcome,
                req.failure_code,
            ));
            Ok(Response::new(pb::RecordTerminalOutcomeResponse {
                run_id: req.run_id,
                source: req.source,
                terminal_step_id: "execution-core-agent-final".to_owned(),
                step_index: 1,
                receipt_id: "receipt-test".to_owned(),
                applied_at: None,
                already_applied: false,
                reconciliation_required: false,
            }))
        }

        async fn record_run_output(
            &self,
            request: Request<pb::RecordRunOutputRequest>,
        ) -> Result<Response<pb::RecordRunOutputResponse>, Status> {
            if request
                .metadata()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                != Some("Bearer test-terminalize-service-token")
            {
                return Err(Status::unauthenticated(
                    "recording a run answer requires its service credential",
                ));
            }
            if self.rec.lock().unwrap().fail_run_output {
                return Err(Status::unavailable("run output store unavailable"));
            }
            let req = request.into_inner();
            let stored_chars = u32::try_from(req.output.chars().count()).unwrap_or(u32::MAX);
            self.rec
                .lock()
                .unwrap()
                .recorded_run_outputs
                .push((req.run_id.clone(), req.output));
            Ok(Response::new(pb::RecordRunOutputResponse {
                run_id: req.run_id,
                stored_chars,
            }))
        }

        async fn heartbeat_managed_run(
            &self,
            request: Request<pb::HeartbeatManagedRunRequest>,
        ) -> Result<Response<pb::HeartbeatManagedRunResponse>, Status> {
            if request
                .metadata()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                != Some("Bearer test-heartbeat-service-token")
            {
                return Err(Status::unauthenticated(
                    "managed heartbeat requires its service credential",
                ));
            }
            let req = request.into_inner();
            self.rec
                .lock()
                .unwrap()
                .managed_heartbeats
                .push((req.run_id, req.source));
            Ok(Response::new(pb::HeartbeatManagedRunResponse {
                renewed_until: None,
                already_terminal: false,
            }))
        }
    }

    // --- Orchestration mock: records PlanTransitioned events. ---

    struct MockOrchestration {
        rec: SharedRecorder,
        fail_create_approval: bool,
    }

    #[tonic::async_trait]
    impl OrchestrationCoreService for MockOrchestration {
        type StreamRunEventsStream = RunEventStream;

        async fn get_run_proof_bundle(
            &self,
            _: Request<pb::GetRunProofBundleRequest>,
        ) -> Result<Response<pb::GetRunProofBundleResponse>, Status> {
            Err(Status::unimplemented("not needed in this test"))
        }

        async fn get_verification_metrics(
            &self,
            _: Request<pb::GetVerificationMetricsRequest>,
        ) -> Result<Response<pb::GetVerificationMetricsResponse>, Status> {
            Err(Status::unimplemented("not needed in this test"))
        }

        async fn record_orchestration_event(
            &self,
            request: Request<pb::RecordOrchestrationEventRequest>,
        ) -> Result<Response<pb::RecordOrchestrationEventResponse>, Status> {
            if let Some(pb::OrchestrationEvent {
                event: Some(pb::orchestration_event::Event::PlanTransitioned(pt)),
                ..
            }) = request.into_inner().event
            {
                self.rec
                    .lock()
                    .unwrap()
                    .plan_transitions
                    .push((pt.from, pt.to));
            }
            Ok(Response::new(pb::RecordOrchestrationEventResponse {
                event_id: "evt".to_owned(),
            }))
        }

        async fn list_plans(
            &self,
            _: Request<pb::ListPlansRequest>,
        ) -> Result<Response<pb::ListPlansResponse>, Status> {
            Err(Status::unimplemented("list_plans not used"))
        }

        async fn get_plan(
            &self,
            _: Request<pb::GetPlanRequest>,
        ) -> Result<Response<pb::GetPlanResponse>, Status> {
            Err(Status::unimplemented("get_plan not used"))
        }

        async fn transition_plan(
            &self,
            _: Request<pb::TransitionPlanRequest>,
        ) -> Result<Response<pb::TransitionPlanResponse>, Status> {
            Err(Status::unimplemented("transition_plan not used"))
        }

        async fn list_todos(
            &self,
            _: Request<pb::ListTodosRequest>,
        ) -> Result<Response<pb::ListTodosResponse>, Status> {
            Err(Status::unimplemented("list_todos not used"))
        }

        async fn get_todo(
            &self,
            _: Request<pb::GetTodoRequest>,
        ) -> Result<Response<pb::GetTodoResponse>, Status> {
            Err(Status::unimplemented("get_todo not used"))
        }

        async fn transition_todo(
            &self,
            _: Request<pb::TransitionTodoRequest>,
        ) -> Result<Response<pb::TransitionTodoResponse>, Status> {
            Err(Status::unimplemented("transition_todo not used"))
        }

        async fn create_approval(
            &self,
            request: Request<pb::CreateApprovalRequest>,
        ) -> Result<Response<pb::CreateApprovalResponse>, Status> {
            if self.fail_create_approval {
                return Err(Status::unavailable("approval persistence unavailable"));
            }
            let req = request.into_inner();
            self.rec
                .lock()
                .unwrap()
                .approvals
                .push((req.step_id.clone(), req.reason));
            // Return a real durable-style record (state REQUESTED) so callers that
            // resolve an approval id (e.g. execute_provider_action) get a genuine
            // per-decision id instead of nothing.
            let approval = pb::Approval {
                id: format!("appr_{}", req.idempotency_key.replace(':', "_")),
                run_id: req.run_id,
                step_id: req.step_id,
                state: pb::ApprovalState::Requested as i32,
                ..Default::default()
            };
            Ok(Response::new(pb::CreateApprovalResponse {
                approval: Some(approval),
            }))
        }

        async fn list_approvals(
            &self,
            _: Request<pb::ListApprovalsRequest>,
        ) -> Result<Response<pb::ListApprovalsResponse>, Status> {
            Err(Status::unimplemented("list_approvals not used"))
        }

        async fn list_pending_approvals(
            &self,
            _: Request<pb::OrgPendingApprovalsRequest>,
        ) -> Result<Response<pb::OrgPendingApprovalsResponse>, Status> {
            Err(Status::unimplemented("list_pending_approvals not used"))
        }

        async fn get_approval(
            &self,
            _: Request<pb::GetApprovalRequest>,
        ) -> Result<Response<pb::GetApprovalResponse>, Status> {
            Err(Status::unimplemented("get_approval not used"))
        }

        async fn decide_approval(
            &self,
            request: Request<pb::DecideApprovalRequest>,
        ) -> Result<Response<pb::DecideApprovalResponse>, Status> {
            let req = request.into_inner();
            self.rec
                .lock()
                .unwrap()
                .decisions
                .push((req.approval_id.clone(), req.decision));
            let approval = pb::Approval {
                id: req.approval_id,
                state: req.decision,
                decided_by: req.decided_by,
                decision_reason: req.decision_reason,
                ..Default::default()
            };
            Ok(Response::new(pb::DecideApprovalResponse {
                approval: Some(approval),
            }))
        }

        async fn claim_approval_deliveries(
            &self,
            _: Request<pb::ClaimApprovalDeliveriesRequest>,
        ) -> Result<Response<pb::ClaimApprovalDeliveriesResponse>, Status> {
            Err(Status::unimplemented("claim_approval_deliveries not used"))
        }

        async fn get_approval_continuation(
            &self,
            _: Request<pb::GetApprovalContinuationRequest>,
        ) -> Result<Response<pb::GetApprovalContinuationResponse>, Status> {
            Err(Status::unimplemented("get_approval_continuation not used"))
        }

        async fn record_approval_continuation_started(
            &self,
            _: Request<pb::RecordApprovalContinuationStartedRequest>,
        ) -> Result<Response<pb::RecordApprovalContinuationStartedResponse>, Status> {
            Err(Status::unimplemented(
                "record_approval_continuation_started not used",
            ))
        }

        async fn record_approval_continuation_outcome(
            &self,
            _: Request<pb::RecordApprovalContinuationOutcomeRequest>,
        ) -> Result<Response<pb::RecordApprovalContinuationOutcomeResponse>, Status> {
            Err(Status::unimplemented(
                "record_approval_continuation_outcome not used",
            ))
        }

        async fn acknowledge_approval_delivery(
            &self,
            _: Request<pb::AcknowledgeApprovalDeliveryRequest>,
        ) -> Result<Response<pb::AcknowledgeApprovalDeliveryResponse>, Status> {
            Err(Status::unimplemented(
                "acknowledge_approval_delivery not used",
            ))
        }

        async fn get_subagent_lineage(
            &self,
            _: Request<pb::GetSubagentLineageRequest>,
        ) -> Result<Response<pb::GetSubagentLineageResponse>, Status> {
            Err(Status::unimplemented("get_subagent_lineage not used"))
        }

        async fn attach_subagent(
            &self,
            request: Request<pb::AttachSubagentRequest>,
        ) -> Result<Response<pb::AttachSubagentResponse>, Status> {
            let req = request.into_inner();
            let mut rec = self.rec.lock().unwrap();
            // The real table's composite PK rejects a duplicate edge; mirror it
            // so the already_started replay path is exercised, not smoothed over.
            if rec.lineage_edges.iter().any(|(parent, child, _)| {
                parent == &req.parent_run_id && child == &req.child_run_id
            }) {
                return Err(Status::already_exists("subagent edge already recorded"));
            }
            rec.lineage_edges.push((
                req.parent_run_id.clone(),
                req.child_run_id.clone(),
                req.role,
            ));
            Ok(Response::new(pb::AttachSubagentResponse::default()))
        }

        async fn stream_run_events(
            &self,
            _: Request<pb::StreamRunEventsRequest>,
        ) -> Result<Response<Self::StreamRunEventsStream>, Status> {
            Err(Status::unimplemented("stream_run_events not used"))
        }
    }

    /// Bind an ephemeral in-process tonic server hosting Session Core,
    /// orchestration, and managed terminalization (they share one channel in
    /// production).
    async fn spawn_session_channel(rec: SharedRecorder) -> Channel {
        spawn_session_channel_with_failures(rec, false, false).await
    }

    async fn spawn_session_channel_with_approval_failure(
        rec: SharedRecorder,
        fail_create_approval: bool,
    ) -> Channel {
        spawn_session_channel_with_failures(rec, fail_create_approval, false).await
    }

    async fn spawn_session_channel_with_terminal_receipt_failure(rec: SharedRecorder) -> Channel {
        spawn_session_channel_with_failures(rec, false, true).await
    }

    async fn spawn_session_channel_with_failures(
        rec: SharedRecorder,
        fail_create_approval: bool,
        fail_terminal_receipt: bool,
    ) -> Channel {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind session");
        let addr = listener.local_addr().expect("session addr");
        tokio::spawn(async move {
            Server::builder()
                .add_service(SessionCoreServer::new(MockSession { rec: rec.clone() }))
                .add_service(OrchestrationCoreServiceServer::new(MockOrchestration {
                    rec: rec.clone(),
                    fail_create_approval,
                }))
                .add_service(pb::run_service_server::RunServiceServer::new(
                    MockRunService { rec: rec.clone() },
                ))
                .add_service(ManagedRunLifecycleServer::new(MockManagedRunLifecycle {
                    rec,
                    fail_terminal_receipt,
                }))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .ok();
        });
        Endpoint::from_shared(format!("http://{addr}"))
            .expect("session endpoint")
            .connect()
            .await
            .expect("connect session")
    }

    async fn spawn_inference_channel(script: Vec<Scripted>) -> Channel {
        spawn_inference_channel_inner(MockInference::new(script)).await
    }

    /// Like [`spawn_inference_channel`] but records every observed `InferRequest.zdr`
    /// into `observed_zdr`, so a test can assert the run's ZDR flag was threaded
    /// through to inference.
    async fn spawn_inference_channel_with_zdr(
        script: Vec<Scripted>,
        observed_zdr: Arc<Mutex<Vec<bool>>>,
    ) -> Channel {
        spawn_inference_channel_inner(MockInference::with_zdr_recorder(script, observed_zdr)).await
    }

    /// Like [`spawn_inference_channel`] but records every observed
    /// `InferRequest.min_privacy_tier`, so a test can assert the run's privacy
    /// floor was threaded through to inference.
    async fn spawn_inference_channel_with_tier(
        script: Vec<Scripted>,
        observed_tiers: ObservedTiers,
    ) -> Channel {
        spawn_inference_channel_inner(MockInference::with_tier_recorder(script, observed_tiers))
            .await
    }

    /// Like [`spawn_inference_channel`] but records the message history of every
    /// `InferRequest`, in call order, into `observed_messages`.
    async fn spawn_inference_channel_with_messages(
        script: Vec<Scripted>,
        observed_messages: ObservedMessages,
    ) -> Channel {
        spawn_inference_channel_inner(MockInference::with_message_recorder(
            script,
            observed_messages,
        ))
        .await
    }

    async fn spawn_inference_channel_inner(mock: MockInference) -> Channel {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind inference");
        let addr = listener.local_addr().expect("inference addr");
        tokio::spawn(async move {
            Server::builder()
                .add_service(InferenceCoreServer::new(mock))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .ok();
        });
        Endpoint::from_shared(format!("http://{addr}"))
            .expect("inference endpoint")
            .connect()
            .await
            .expect("connect inference")
    }

    fn sample_request() -> pb::RunAgentRequest {
        pb::RunAgentRequest {
            run_id: "run_test".to_owned(),
            thread_id: "thread_test".to_owned(),
            goal: "What is 2 + 2?".to_owned(),
            org_id: "org_test".to_owned(),
            user_id: "user_test".to_owned(),
            model: String::new(),
            mode: "execute".to_owned(),
            max_rounds: 4,
            zdr: false,
            tools: Vec::new(),
            // PRIVACY FLOOR: no tier stated, which is what an unconstrained run
            // sends — a floor is imposed only when the caller asked for one, and
            // the tests that DO care set it explicitly on the returned request.
            min_privacy_tier: pb::PrivacyTier::Unspecified as i32,
            plan_mode: false,
            // No graded constraint stated, which is what an ordinary run sends —
            // the ladder narrows a run only when a grant put it there.
            autonomy_rung: 0,
            // Non-Space by default, matching every other unconstrained field
            // above — the tests that DO care set it explicitly.
            space_id: String::new(),
        }
    }

    fn sample_agent_skill(name: &str, keywords: &[&str], content: &str) -> pb::AgentSkill {
        pb::AgentSkill {
            id: format!("sk-{name}"),
            name: name.to_owned(),
            description: String::new(),
            content: content.to_owned(),
            trigger_keywords: keywords
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
            trigger_file_patterns: Vec::new(),
            tool_restrictions: Vec::new(),
            enabled: true,
            origin: "background_review".to_owned(),
            scope: "org".to_owned(),
            owner_user_id: String::new(),
            shared_with: Vec::new(),
        }
    }

    #[tokio::test]
    async fn fetch_skill_context_injects_a_keyword_matched_skill() {
        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        rec.lock().unwrap().agent_skills = vec![sample_agent_skill(
            "Shipment booking",
            &["shipment", "shipping"],
            "Always call get_shipping_quotes before book_shipment.",
        )];
        let session_channel = spawn_session_channel(rec).await;

        let blocks = fetch_skill_context(
            &session_channel,
            Some("test-bearer"),
            "org_test",
            "please book a shipment to Oslo",
        )
        .await;

        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].contains("## Skill: Shipment booking"));
        assert!(blocks[0].contains("get_shipping_quotes before book_shipment"));
    }

    #[tokio::test]
    async fn fetch_skill_context_is_empty_without_a_bearer() {
        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        rec.lock().unwrap().agent_skills = vec![sample_agent_skill(
            "Shipment booking",
            &["shipment"],
            "body",
        )];
        let session_channel = spawn_session_channel(rec).await;

        let blocks =
            fetch_skill_context(&session_channel, None, "org_test", "book a shipment").await;

        assert!(blocks.is_empty(), "no bearer must never call session-core");
    }

    #[tokio::test]
    async fn fetch_skill_context_is_empty_when_nothing_matches() {
        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        rec.lock().unwrap().agent_skills = vec![sample_agent_skill(
            "Weather",
            &["weather"],
            "Use yr_weather for Norwegian coordinates.",
        )];
        let session_channel = spawn_session_channel(rec).await;

        let blocks =
            fetch_skill_context(&session_channel, Some("b"), "org_test", "what is 2+2").await;

        assert!(blocks.is_empty());
    }

    #[tokio::test]
    async fn zdr_tool_step_persistence_is_metadata_only_for_success_and_error() {
        const PRIVATE_OUTPUT: &str = "customer revenue is 12,345 NOK";
        const PRIVATE_ERROR: &str = "retrieval failed for secret acquisition query";
        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        let session_channel = spawn_session_channel(rec.clone()).await;
        let success = StepOutcome {
            status: "completed".to_owned(),
            output: PRIVATE_OUTPUT.to_owned(),
            error: String::new(),
            compaction_triggered: false,
        };
        let degraded = StepOutcome {
            status: "failed".to_owned(),
            output: String::new(),
            error: PRIVATE_ERROR.to_owned(),
            compaction_triggered: false,
        };

        record_tool_step(
            &session_channel,
            "run-zdr",
            "success-zdr",
            "knowledge_search",
            &success,
            true,
            Some("session-token"),
        )
        .await;
        record_tool_step(
            &session_channel,
            "run-zdr",
            "error-zdr",
            "knowledge_search",
            &degraded,
            true,
            Some("session-token"),
        )
        .await;

        // Persistence redaction must not mutate the ephemeral result that the
        // current agent round receives and may use for reasoning.
        assert_eq!(success.output, PRIVATE_OUTPUT);
        assert_eq!(degraded.error, PRIVATE_ERROR);

        let r = rec.lock().unwrap();
        for (step_id, output, error) in &r.persisted_step_payloads {
            assert!(!output.contains(PRIVATE_OUTPUT), "{step_id} leaked output");
            assert!(!output.contains(PRIVATE_ERROR), "{step_id} leaked error");
            assert!(!error.contains(PRIVATE_OUTPUT), "{step_id} leaked output");
            assert!(!error.contains(PRIVATE_ERROR), "{step_id} leaked error");
            assert!(output.contains("zdr=true"));
            assert!(output.contains("content_redacted=true"));
        }
        assert!(r.persisted_step_payloads[0].2.is_empty());
        assert!(r.persisted_step_payloads[1].2.contains("result=error"));
    }

    #[tokio::test]
    async fn non_zdr_tool_step_persistence_keeps_existing_content_behavior() {
        const TOOL_OUTPUT: &str = "ordinary persisted tool output";
        const TOOL_ERROR: &str = "ordinary persisted tool error";
        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        let session_channel = spawn_session_channel(rec.clone()).await;
        let success = StepOutcome {
            status: "completed".to_owned(),
            output: TOOL_OUTPUT.to_owned(),
            error: String::new(),
            compaction_triggered: false,
        };
        let failure = StepOutcome {
            status: "failed".to_owned(),
            output: String::new(),
            error: TOOL_ERROR.to_owned(),
            compaction_triggered: false,
        };

        record_tool_step(
            &session_channel,
            "run-persistent",
            "success-persistent",
            "knowledge_search",
            &success,
            false,
            Some("session-token"),
        )
        .await;
        record_tool_step(
            &session_channel,
            "run-persistent",
            "error-persistent",
            "knowledge_search",
            &failure,
            false,
            Some("session-token"),
        )
        .await;

        let r = rec.lock().unwrap();
        assert!(r.persisted_step_payloads[0].1.contains(TOOL_OUTPUT));
        assert!(r.persisted_step_payloads[0].2.is_empty());
        assert!(r.persisted_step_payloads[1].2.contains(TOOL_ERROR));
        assert!(!r.persisted_step_payloads[0]
            .1
            .contains("content_redacted=true"));
    }

    #[tokio::test]
    async fn no_tool_run_persists_answer_completes_and_transitions() {
        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        let session_channel = spawn_session_channel(rec.clone()).await;
        let inference_channel =
            spawn_inference_channel(vec![Scripted::Answer("The answer is 4.".to_owned())]).await;
        let state = crate::state::StateStore::new();

        let resp = run_agent(
            &state,
            session_channel,
            inference_channel,
            sample_request(),
            None,
            Some("session-token".to_owned()),
            "inference-token".to_owned(),
            &ALLOW_CAPABILITY_POLICY,
            &STATIC_TERMINAL_TOKENS,
            None,
            None,
            &test_sandbox_manager_client(),
            "test-backend",
            None,
            &test_sandbox_tokens(),
        )
        .await
        .expect("ordinary agent run should succeed");

        assert_eq!(resp.status, "completed");
        assert_eq!(resp.final_output, "The answer is 4.");
        assert_eq!(resp.rounds_executed, 1);

        let r = rec.lock().unwrap();
        assert_eq!(
            r.appended_assistant.len(),
            1,
            "exactly one assistant message"
        );
        assert_eq!(r.appended_assistant[0], "The answer is 4.");
        assert!(
            r.completed.is_empty(),
            "a no-tool run must not use legacy CompleteStep for terminalization"
        );
        assert_eq!(
            r.managed_terminal_outcomes,
            vec![(
                "run_test".to_owned(),
                pb::ManagedRunSource::ExecutionAgent as i32,
                pb::TerminalOutcome::Completed as i32,
                String::new(),
            )],
            "the managed receipt is the sole terminal authority"
        );
        assert!(
            r.managed_heartbeats
                .iter()
                .all(|(_, source)| *source == pb::ManagedRunSource::ExecutionAgent as i32),
            "only Execution Core may renew an agent run"
        );
        assert_eq!(
            r.plan_transitions,
            vec![
                (pb::PlanState::Draft as i32, pb::PlanState::Executing as i32),
                (
                    pb::PlanState::Executing as i32,
                    pb::PlanState::Completed as i32
                ),
            ],
            "DRAFT→EXECUTING then EXECUTING→COMPLETED"
        );

        assert_eq!(
            state.get_or_create("run_test").status,
            crate::state::RunStatus::Completed
        );
    }

    #[tokio::test]
    async fn run_threads_request_zdr_into_inference() {
        // GDPR: a `RunAgentRequest.zdr = true` run must thread that flag into
        // every `InferRequest` so inference-core's prompt cache skips durable
        // read/write. Drives a single non-tool round and asserts the observed
        // ZDR on the InferRequest matches the request.
        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        let session_channel = spawn_session_channel(rec.clone()).await;
        let observed_zdr: Arc<Mutex<Vec<bool>>> = Arc::new(Mutex::new(Vec::new()));
        let inference_channel = spawn_inference_channel_with_zdr(
            vec![Scripted::Answer("ok".to_owned())],
            observed_zdr.clone(),
        )
        .await;
        let state = crate::state::StateStore::new();

        let mut req = sample_request();
        req.zdr = true;
        let resp = run_agent(
            &state,
            session_channel,
            inference_channel,
            req,
            None,
            Some("session-token".to_owned()),
            "inference-token".to_owned(),
            &ALLOW_CAPABILITY_POLICY,
            &STATIC_TERMINAL_TOKENS,
            None,
            None,
            &test_sandbox_manager_client(),
            "test-backend",
            None,
            &test_sandbox_tokens(),
        )
        .await
        .expect("ZDR agent run should succeed");

        assert_eq!(resp.status, "completed");
        assert_eq!(
            resp.final_output, "ok",
            "the caller still receives the ephemeral answer"
        );
        let observed = observed_zdr.lock().unwrap();
        assert_eq!(observed.len(), 1, "exactly one inference round");
        assert!(
            observed[0],
            "the run's zdr=true must be threaded into the InferRequest"
        );
        drop(observed);

        let r = rec.lock().unwrap();
        assert!(
            r.appended_assistant.is_empty(),
            "ZDR must not append the assistant answer to durable conversation storage"
        );
        assert!(
            r.completed.is_empty() && r.persisted_step_payloads.is_empty(),
            "ZDR terminalization must not write a legacy final content payload"
        );
        assert_eq!(
            r.managed_terminal_outcomes,
            vec![(
                "run_test".to_owned(),
                pb::ManagedRunSource::ExecutionAgent as i32,
                pb::TerminalOutcome::Completed as i32,
                String::new(),
            )],
            "the metadata-only managed receipt carries no response content"
        );
    }

    #[tokio::test]
    async fn run_threads_request_min_privacy_tier_into_inference() {
        // PRIVACY FLOOR: a `RunAgentRequest.min_privacy_tier` must ride on EVERY
        // round's `InferRequest`, mirroring the ZDR threading above — otherwise a
        // constrained chat turn routed through the governed agent loop would be
        // silently downgraded to unconstrained providers.
        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        let session_channel = spawn_session_channel(rec.clone()).await;
        let observed_tiers: ObservedTiers = Arc::new(Mutex::new(Vec::new()));
        let inference_channel = spawn_inference_channel_with_tier(
            vec![Scripted::Answer("ok".to_owned())],
            observed_tiers.clone(),
        )
        .await;
        let state = crate::state::StateStore::new();

        let mut req = sample_request();
        req.min_privacy_tier = pb::PrivacyTier::Sovereign as i32;
        let resp = run_agent(
            &state,
            session_channel,
            inference_channel,
            req,
            None,
            Some("session-token".to_owned()),
            "inference-token".to_owned(),
            &ALLOW_CAPABILITY_POLICY,
            &STATIC_TERMINAL_TOKENS,
            None,
            None,
            &test_sandbox_manager_client(),
            "test-backend",
            None,
            &test_sandbox_tokens(),
        )
        .await
        .expect("tier-constrained agent run should succeed");

        assert_eq!(resp.status, "completed");
        let observed = observed_tiers.lock().unwrap();
        assert_eq!(observed.len(), 1, "exactly one inference round");
        assert_eq!(
            observed[0],
            pb::PrivacyTier::Sovereign as i32,
            "the run's min_privacy_tier must be threaded into the InferRequest"
        );
    }

    #[tokio::test]
    async fn infer_error_still_persists_reply_and_fails_run() {
        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        let session_channel = spawn_session_channel(rec.clone()).await;
        let inference_channel = spawn_inference_channel(vec![Scripted::Error]).await; // infer errors
        let state = crate::state::StateStore::new();

        let resp = run_agent(
            &state,
            session_channel,
            inference_channel,
            sample_request(),
            None,
            Some("session-token".to_owned()),
            "inference-token".to_owned(),
            &ALLOW_CAPABILITY_POLICY,
            &STATIC_TERMINAL_TOKENS,
            None,
            None,
            &test_sandbox_manager_client(),
            "test-backend",
            None,
            &test_sandbox_tokens(),
        )
        .await
        .expect("inference failure should be finalized as a response");

        assert_eq!(resp.status, "failed");
        assert_eq!(resp.final_output, GRACEFUL_FAILURE_REPLY);

        let r = rec.lock().unwrap();
        assert_eq!(
            r.appended_assistant.len(),
            1,
            "a graceful assistant reply is still appended"
        );
        assert_eq!(r.appended_assistant[0], GRACEFUL_FAILURE_REPLY);
        assert!(
            r.completed.is_empty(),
            "inference failure must not fall back to a legacy terminal CompleteStep"
        );
        assert_eq!(
            r.managed_terminal_outcomes,
            vec![(
                "run_test".to_owned(),
                pb::ManagedRunSource::ExecutionAgent as i32,
                pb::TerminalOutcome::Failed as i32,
                "execution_failed".to_owned(),
            )]
        );
        assert_eq!(
            r.plan_transitions,
            vec![
                (pb::PlanState::Draft as i32, pb::PlanState::Executing as i32),
                (
                    pb::PlanState::Executing as i32,
                    pb::PlanState::Failed as i32
                ),
            ],
            "DRAFT→EXECUTING then EXECUTING→FAILED"
        );

        assert_eq!(
            state.get_or_create("run_test").status,
            crate::state::RunStatus::Failed
        );
    }

    #[tokio::test]
    async fn terminal_receipt_failure_does_not_expose_terminal_projection() {
        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        let session_channel =
            spawn_session_channel_with_terminal_receipt_failure(rec.clone()).await;
        let inference_channel =
            spawn_inference_channel(vec![Scripted::Answer("The answer is 4.".to_owned())]).await;
        let state = crate::state::StateStore::new();

        let error = run_agent(
            &state,
            session_channel,
            inference_channel,
            sample_request(),
            None,
            Some("session-token".to_owned()),
            "inference-token".to_owned(),
            &ALLOW_CAPABILITY_POLICY,
            &STATIC_TERMINAL_TOKENS,
            None,
            None,
            &test_sandbox_manager_client(),
            "test-backend",
            None,
            &test_sandbox_tokens(),
        )
        .await
        .expect_err("a missing durable terminal receipt must fail the agent run");

        assert_eq!(error.code(), tonic::Code::Unavailable);
        assert!(
            state.snapshot("run_test").is_none(),
            "the local state projection cannot become terminal without Session Core's receipt"
        );

        let r = rec.lock().unwrap();
        assert!(
            r.managed_terminal_outcomes.is_empty(),
            "the failed RPC produced no durable terminal receipt"
        );
        assert!(
            r.completed.is_empty(),
            "the failure must not fall back to legacy CompleteStep terminalization"
        );
        assert_eq!(
            r.plan_transitions,
            vec![(pb::PlanState::Draft as i32, pb::PlanState::Executing as i32)],
            "the terminal projection event is held until the receipt succeeds"
        );
    }

    #[tokio::test]
    async fn multi_tool_run_dispatches_records_nonterminal_then_one_terminal() {
        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        let session_channel = spawn_session_channel(rec.clone()).await;

        // Round 1: model requests an offered (non-risky) tool; `execute_step`
        // dispatches it through the real arm — its serde parse fails fast (no
        // network), proving dispatch happened. Round 2: model answers.
        let call = pb::ToolCall {
            id: "call-1".to_owned(),
            name: "yr_weather".to_owned(),
            // Schema-valid, so this fails where this test needs it to — at
            // dispatch, with no configured upstream — rather than being refused
            // by pre-dispatch argument validation, which would record no audit
            // step at all and make the assertion below vacuous.
            arguments_json: r#"{"lat":60.39,"lon":5.32}"#.to_owned(),
        };
        let inference_channel = spawn_inference_channel(vec![
            Scripted::ToolCalls {
                content: String::new(),
                calls: vec![call],
            },
            Scripted::Answer("Here is the weather summary.".to_owned()),
        ])
        .await;
        let state = crate::state::StateStore::new();

        let resp = run_agent(
            &state,
            session_channel,
            inference_channel,
            sample_request(),
            None,
            Some("session-token".to_owned()),
            "inference-token".to_owned(),
            &ALLOW_CAPABILITY_POLICY,
            &STATIC_TERMINAL_TOKENS,
            None,
            None,
            &test_sandbox_manager_client(),
            "test-backend",
            None,
            &test_sandbox_tokens(),
        )
        .await
        .expect("multi-tool agent run should succeed");

        assert_eq!(resp.status, "completed");
        assert_eq!(resp.final_output, "Here is the weather summary.");
        assert_eq!(resp.rounds_executed, 2, "one tool round + one answer round");

        let r = rec.lock().unwrap();
        // The final answer is appended exactly once.
        assert_eq!(r.appended_assistant.len(), 1);
        assert_eq!(r.appended_assistant[0], "Here is the weather summary.");

        // The per-tool step is recorded through the legacy non-terminal audit
        // API. The terminal authority is the separate immutable receipt.
        assert_eq!(
            r.completed,
            vec![("tool_1_call-1".to_owned(), "running".to_owned())],
            "the tool audit is non-terminal"
        );
        assert_eq!(
            r.managed_terminal_outcomes,
            vec![(
                "run_test".to_owned(),
                pb::ManagedRunSource::ExecutionAgent as i32,
                pb::TerminalOutcome::Completed as i32,
                String::new(),
            )],
            "TERMINAL-ONCE: exactly one immutable managed receipt"
        );

        assert_eq!(
            r.plan_transitions,
            vec![
                (pb::PlanState::Draft as i32, pb::PlanState::Executing as i32),
                (
                    pb::PlanState::Executing as i32,
                    pb::PlanState::Completed as i32
                ),
            ]
        );
        assert!(r.approvals.is_empty(), "non-risky tools need no approval");
    }

    #[tokio::test]
    async fn exact_duplicate_retrieval_is_suppressed_but_run_can_backtrack() {
        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        let session_channel = spawn_session_channel(rec.clone()).await;
        let first = pb::ToolCall {
            id: "search-1".to_owned(),
            name: "knowledge_search".to_owned(),
            arguments_json: r#"{"query":"Quarterly revenue","top_k":5}"#.to_owned(),
        };
        let duplicate = pb::ToolCall {
            id: "search-2".to_owned(),
            name: "knowledge_search".to_owned(),
            arguments_json: r#"{"route":"hybrid","top_k":5,"query":" quarterly   REVENUE "}"#
                .to_owned(),
        };
        let inference_channel = spawn_inference_channel(vec![
            Scripted::ToolCalls {
                content: "I will retrieve evidence.".to_owned(),
                calls: vec![first],
            },
            Scripted::ToolCalls {
                content: "I will try again.".to_owned(),
                calls: vec![duplicate],
            },
            Scripted::Answer("I could not verify the revenue from available evidence.".to_owned()),
        ])
        .await;
        let state = crate::state::StateStore::new();

        let resp = run_agent_with_tools(
            &state,
            session_channel,
            inference_channel,
            sample_request(),
            vec![pb::ToolDefinition {
                name: "knowledge_search".to_owned(),
                description: "Search knowledge".to_owned(),
                parameters_json: "{}".to_owned(),
            }],
            None,
            Some("session-token".to_owned()),
            "inference-token".to_owned(),
            &ALLOW_CAPABILITY_POLICY,
            &STATIC_TERMINAL_TOKENS,
            None,
            None,
            &test_sandbox_manager_client(),
            "test-backend",
            None,
            &test_sandbox_tokens(),
        )
        .await
        .expect("duplicate retrieval run should succeed");

        assert_eq!(resp.status, "completed");
        assert_eq!(resp.rounds_executed, 3);
        let r = rec.lock().unwrap();
        assert_eq!(
            r.completed,
            vec![("tool_1_search-1".to_owned(), "running".to_owned())],
            "the duplicate is not dispatched or recorded as a second retrieval"
        );
        assert_eq!(r.managed_terminal_outcomes.len(), 1);
        assert_eq!(
            r.managed_terminal_outcomes[0].2,
            pb::TerminalOutcome::Completed as i32
        );
    }

    #[tokio::test]
    async fn gated_tool_creates_approval_and_returns_early() {
        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        let session_channel = spawn_session_channel(rec.clone()).await;

        // The model requests a risky tool that IS offered. Under `ask` posture
        // `execute_step` returns `awaiting_approval`, so the loop must mint an
        // approval and return early — never reaching a terminal step.
        let call = pb::ToolCall {
            id: "del-1".to_owned(),
            name: "delete_records".to_owned(),
            arguments_json: "{}".to_owned(),
        };
        let inference_channel = spawn_inference_channel(vec![Scripted::ToolCalls {
            content: String::new(),
            calls: vec![call],
        }])
        .await;
        let state = crate::state::StateStore::new();

        // Offer a single risky tool so the HITL gate fires (the production
        // allowlist is all non-risky read tools).
        let gated_tools = vec![pb::ToolDefinition {
            name: "delete_records".to_owned(),
            description: "Delete records (destructive).".to_owned(),
            parameters_json: r#"{"type":"object","properties":{}}"#.to_owned(),
        }];

        let mut req = sample_request();
        req.mode = "ask".to_owned();
        let resp = run_agent_with_tools(
            &state,
            session_channel,
            inference_channel,
            req,
            gated_tools,
            None,
            Some("session-token".to_owned()),
            "inference-token".to_owned(),
            &ALLOW_CAPABILITY_POLICY,
            &STATIC_TERMINAL_TOKENS,
            None,
            None,
            &test_sandbox_manager_client(),
            "test-backend",
            None,
            &test_sandbox_tokens(),
        )
        .await
        .expect("durable approval should pause the run");

        assert_eq!(resp.status, "awaiting_approval");
        assert_eq!(resp.final_output, "");

        let r = rec.lock().unwrap();
        assert_eq!(r.approvals.len(), 1, "exactly one approval created");
        assert_eq!(r.approvals[0].0, "tool_1_del-1");
        assert!(r.approvals[0].1.contains("delete_records"));
        // Paused, not finished: no terminal step, no assistant answer, and the
        // plan stops at EXECUTING (no COMPLETED/FAILED transition).
        assert!(r.completed.is_empty(), "no CompleteStep on a paused run");
        assert!(
            r.managed_terminal_outcomes.is_empty(),
            "a pause must not emit a managed terminal receipt"
        );
        assert!(r.appended_assistant.is_empty());
        assert_eq!(
            r.plan_transitions,
            vec![(pb::PlanState::Draft as i32, pb::PlanState::Executing as i32)],
            "only DRAFT→EXECUTING; the run is paused, not finalized"
        );

        assert_eq!(
            state.get_or_create("run_test").status,
            crate::state::RunStatus::AwaitingApproval
        );
    }

    #[tokio::test]
    async fn gated_tool_rejects_an_undurable_approval_pause() {
        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        let session_channel = spawn_session_channel_with_approval_failure(rec.clone(), true).await;
        let call = pb::ToolCall {
            id: "del-undurable".to_owned(),
            name: "delete_records".to_owned(),
            arguments_json: "{}".to_owned(),
        };
        let inference_channel = spawn_inference_channel(vec![Scripted::ToolCalls {
            content: String::new(),
            calls: vec![call],
        }])
        .await;
        let state = crate::state::StateStore::new();
        let mut req = sample_request();
        req.mode = "ask".to_owned();

        let error = run_agent_with_tools(
            &state,
            session_channel,
            inference_channel,
            req,
            vec![pb::ToolDefinition {
                name: "delete_records".to_owned(),
                description: "Delete records (destructive).".to_owned(),
                parameters_json: r#"{"type":"object","properties":{}}"#.to_owned(),
            }],
            None,
            Some("session-token".to_owned()),
            "inference-token".to_owned(),
            &ALLOW_CAPABILITY_POLICY,
            &STATIC_TERMINAL_TOKENS,
            None,
            None,
            &test_sandbox_manager_client(),
            "test-backend",
            None,
            &test_sandbox_tokens(),
        )
        .await
        .expect_err("an unpersisted approval must fail the agent HITL path");

        assert_eq!(error.code(), tonic::Code::Unavailable);
        let recorder = rec.lock().unwrap();
        assert!(
            recorder.approvals.is_empty(),
            "no durable approval was created"
        );
        assert!(
            recorder.completed.is_empty(),
            "an undurable pause must not emit a terminal or paused step"
        );
        assert!(
            recorder.managed_terminal_outcomes.is_empty(),
            "an undurable pause must not emit a managed terminal receipt"
        );
        drop(recorder);
        assert_ne!(
            state.snapshot("run_test").map(|snapshot| snapshot.status),
            Some(crate::state::RunStatus::AwaitingApproval),
            "the in-memory state must not claim a durable pause"
        );
    }

    #[tokio::test]
    async fn provider_write_auto_records_and_forwards_real_approval() {
        // Phase 2: under `auto` (chat) posture a provider WRITE is allowed to run,
        // and instead of forwarding a shared constant marker it binds a REAL
        // per-decision durable approval — created AND recorded as granted
        // (interactive user is the live approver). The integration client is
        // absent in tests, so the action itself errors (non-fatal); the point is
        // the durable approval binding.
        std::env::set_var("INTEGRATION_COREV2_URL", "http://127.0.0.1:1"); // fast connection refuse

        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        let session_channel = spawn_session_channel(rec.clone()).await;

        let call = pb::ToolCall {
            id: "pa-1".to_owned(),
            name: "execute_provider_action".to_owned(),
            arguments_json:
                r#"{"connection_id":"c1","operation":"message.send","body":{"text":"hi"}}"#
                    .to_owned(),
        };
        let inference_channel = spawn_inference_channel(vec![
            Scripted::ToolCalls {
                content: String::new(),
                calls: vec![call],
            },
            Scripted::Answer("done".to_owned()),
        ])
        .await;
        let state = crate::state::StateStore::new();

        let tools = vec![pb::ToolDefinition {
            name: "execute_provider_action".to_owned(),
            description: "Run one provider operation.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{}}"#.to_owned(),
        }];

        let mut req = sample_request();
        req.mode = "auto".to_owned();
        let resp = run_agent_with_tools(
            &state,
            session_channel,
            inference_channel,
            req,
            tools,
            None,
            Some("session-token".to_owned()),
            "inference-token".to_owned(),
            &ALLOW_CAPABILITY_POLICY,
            &STATIC_TERMINAL_TOKENS,
            None,
            None,
            &test_sandbox_manager_client(),
            "test-backend",
            None,
            &test_sandbox_tokens(),
        )
        .await
        .expect("provider write run should succeed");
        assert_eq!(resp.status, "completed");

        let r = rec.lock().unwrap();
        assert_eq!(
            r.approvals.len(),
            1,
            "one durable approval created for the provider write"
        );
        assert_eq!(
            r.decisions.len(),
            1,
            "the write's approval was recorded as decided (not a shared constant)"
        );
        assert_eq!(
            r.decisions[0].1,
            pb::ApprovalState::Granted as i32,
            "auto posture records the interactive user's grant on the real approval id"
        );
        assert!(
            r.decisions[0].0.starts_with("appr_"),
            "a real durable approval id is threaded, got {}",
            r.decisions[0].0
        );
    }

    // --- Delegated subagents -------------------------------------------------

    /// The (output, error) Session Core persisted for `step_id`.
    fn persisted_step(rec: &Recorder, step_id: &str) -> (String, String) {
        if let Some((_, output, error)) = rec
            .persisted_step_payloads
            .iter()
            .find(|(id, _, _)| id == step_id)
        {
            return (output.clone(), error.clone());
        }
        let step_ids: Vec<&String> = rec
            .persisted_step_payloads
            .iter()
            .map(|(id, _, _)| id)
            .collect();
        panic!("no persisted step '{step_id}'; recorded steps: {step_ids:?}")
    }

    /// The first user message of a recorded `InferRequest` — the goal that loop
    /// was seeded with.
    fn seeded_goal(messages: &[pb::ChatMessage]) -> &str {
        messages
            .iter()
            .find(|message| message.role == "user")
            .map_or("", |message| message.content.as_str())
    }

    fn subagent_call(id: &str, input: &str) -> pb::ToolCall {
        pb::ToolCall {
            id: id.to_owned(),
            name: "subagent.task".to_owned(),
            arguments_json: input.to_owned(),
        }
    }

    /// A tool call that dispatches for real and fails fast without a network:
    /// `yr_weather` with no coordinates loses its serde parse inside
    /// `execute_step`, which proves the nested loop reached real dispatch.
    /// A call that fails at DISPATCH — `information-core` is not configured in
    /// tests, so the tool errors when it runs.
    ///
    /// Arguments are deliberately schema-valid. This used to pass `{}`, which was
    /// a shortcut to the same failure; once pre-dispatch argument validation
    /// landed, `{}` was refused in the pre-pass instead and these tests silently
    /// stopped exercising the dispatch-failure path they were written for (three
    /// of them failed, which is how this was caught). A fixture must fail for the
    /// reason its users are testing, not for a newer one.
    fn failing_tool_call(id: &str) -> pb::ToolCall {
        pb::ToolCall {
            id: id.to_owned(),
            name: "yr_weather".to_owned(),
            arguments_json: r#"{"lat":60.39,"lon":5.32}"#.to_owned(),
        }
    }

    /// Lineage is bookkeeping; the delegated work is the product.
    ///
    /// If `StartManagedRun` is unavailable the delegation must still run and
    /// still return its answer — refusing a task the user asked for because a
    /// lineage row could not be written would trade a real failure for a
    /// cosmetic one. The inverse (recording nothing and saying nothing) was the
    /// prior behaviour and is what the warning in
    /// `register_delegated_child_run` exists to prevent.
    #[tokio::test]
    async fn a_delegation_still_completes_when_its_child_run_cannot_be_registered() {
        const DELEGATED_GOAL: &str = "Find the current Bergen weather";
        const SUBAGENT_ANSWER: &str = "Bergen: 8 degrees and raining.";

        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        rec.lock().unwrap().fail_child_run_start = true;
        let session_channel = spawn_session_channel(rec.clone()).await;
        let inference_channel = spawn_inference_channel(vec![
            Scripted::ToolCalls {
                content: "Delegating the lookup.".to_owned(),
                calls: vec![subagent_call(
                    "sub-1",
                    &format!(r#"{{"goal":"{DELEGATED_GOAL}"}}"#),
                )],
            },
            Scripted::Answer(SUBAGENT_ANSWER.to_owned()),
            Scripted::Answer(format!("Delegated result: {SUBAGENT_ANSWER}")),
        ])
        .await;
        let state = crate::state::StateStore::new();

        let resp = run_agent(
            &state,
            session_channel,
            inference_channel,
            sample_request(),
            None,
            Some("session-token".to_owned()),
            "inference-token".to_owned(),
            &ALLOW_CAPABILITY_POLICY,
            &STATIC_TERMINAL_TOKENS,
            None,
            None,
            &test_sandbox_manager_client(),
            "test-backend",
            None,
            &test_sandbox_tokens(),
        )
        .await
        .expect("a delegating run must not fail because bookkeeping did");

        assert_eq!(resp.status, "completed");
        assert_eq!(
            resp.final_output,
            format!("Delegated result: {SUBAGENT_ANSWER}"),
            "the subagent's answer must still reach the parent"
        );

        let r = rec.lock().unwrap();
        assert!(
            r.started_child_runs.is_empty(),
            "the child run genuinely failed to register in this scenario"
        );
        assert!(
            r.lineage_edges.is_empty(),
            "no edge may be recorded for a child run that does not exist — it \
             would dangle against the runs FK"
        );
        // And crucially, no orphan obligation was settled for a run that was
        // never created.
        assert_eq!(
            r.managed_terminal_outcomes
                .iter()
                .filter(|(run_id, ..)| run_id != &sample_request().run_id)
                .count(),
            0,
            "settling a child run that was never started would be a receipt for \
             nothing"
        );
    }

    /// A zero-retention run stores NOTHING durable, and a delegation's answer
    /// is content — the most obviously retained thing in the whole flow.
    ///
    /// This is checked here rather than trusted to session-core's refusal
    /// because both sides exist on purpose: the caller knows the posture with no
    /// round trip, the server fails closed for a ZDR credential anyway, and
    /// neither is allowed to depend on the other being right. Note what still
    /// happens on a ZDR run — the terminal receipt, which is metadata-only and
    /// safe. That difference IS the design.
    #[tokio::test]
    async fn a_zero_retention_delegation_records_a_receipt_but_never_its_answer() {
        const DELEGATED_GOAL: &str = "Find the current Bergen weather";
        const SUBAGENT_ANSWER: &str = "Bergen: 8 degrees and raining.";

        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        let session_channel = spawn_session_channel(rec.clone()).await;
        let inference_channel = spawn_inference_channel(vec![
            Scripted::ToolCalls {
                content: "Delegating the lookup.".to_owned(),
                calls: vec![subagent_call(
                    "sub-1",
                    &format!(r#"{{"goal":"{DELEGATED_GOAL}"}}"#),
                )],
            },
            Scripted::Answer(SUBAGENT_ANSWER.to_owned()),
            Scripted::Answer(format!("Delegated result: {SUBAGENT_ANSWER}")),
        ])
        .await;
        let state = crate::state::StateStore::new();

        let mut request = sample_request();
        request.zdr = true;
        let resp = run_agent(
            &state,
            session_channel,
            inference_channel,
            request,
            None,
            Some("session-token".to_owned()),
            "inference-token".to_owned(),
            &ALLOW_CAPABILITY_POLICY,
            &STATIC_TERMINAL_TOKENS,
            None,
            None,
            &test_sandbox_manager_client(),
            "test-backend",
            None,
            &test_sandbox_tokens(),
        )
        .await
        .expect("a ZDR delegation still runs");

        assert_eq!(
            resp.final_output,
            format!("Delegated result: {SUBAGENT_ANSWER}"),
            "the answer still reaches the parent in-flight; only the durable copy is withheld"
        );
        let r = rec.lock().unwrap();
        assert!(
            r.recorded_run_outputs.is_empty(),
            "a ZDR run promised no durable trace, and a stored conclusion is one: {:?}",
            r.recorded_run_outputs
        );
        assert!(
            !r.managed_terminal_outcomes.is_empty(),
            "the metadata-only receipt is still written — that is what makes it \
             safe on a ZDR run, and losing it would leave an obligation the \
             watchdog force-fails"
        );
    }

    /// The durable copy is bookkeeping; the delegated work is the product. A
    /// failed write must cost the ability to read the answer back later, and
    /// nothing else — the parent still gets its answer this turn.
    #[tokio::test]
    async fn a_delegation_still_answers_when_its_conclusion_cannot_be_stored() {
        const DELEGATED_GOAL: &str = "Find the current Bergen weather";
        const SUBAGENT_ANSWER: &str = "Bergen: 8 degrees and raining.";

        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        rec.lock().unwrap().fail_run_output = true;
        let session_channel = spawn_session_channel(rec.clone()).await;
        let inference_channel = spawn_inference_channel(vec![
            Scripted::ToolCalls {
                content: "Delegating the lookup.".to_owned(),
                calls: vec![subagent_call(
                    "sub-1",
                    &format!(r#"{{"goal":"{DELEGATED_GOAL}"}}"#),
                )],
            },
            Scripted::Answer(SUBAGENT_ANSWER.to_owned()),
            Scripted::Answer(format!("Delegated result: {SUBAGENT_ANSWER}")),
        ])
        .await;
        let state = crate::state::StateStore::new();

        let resp = run_agent(
            &state,
            session_channel,
            inference_channel,
            sample_request(),
            None,
            Some("session-token".to_owned()),
            "inference-token".to_owned(),
            &ALLOW_CAPABILITY_POLICY,
            &STATIC_TERMINAL_TOKENS,
            None,
            None,
            &test_sandbox_manager_client(),
            "test-backend",
            None,
            &test_sandbox_tokens(),
        )
        .await
        .expect("a delegating run must not fail because bookkeeping did");

        assert_eq!(resp.status, "completed");
        assert_eq!(
            resp.final_output,
            format!("Delegated result: {SUBAGENT_ANSWER}")
        );
        let r = rec.lock().unwrap();
        assert!(
            r.recorded_run_outputs.is_empty(),
            "the write genuinely failed in this scenario"
        );
        // And the child still reaches a terminal state, so the watchdog has
        // nothing to force-fail.
        assert!(
            r.managed_terminal_outcomes
                .iter()
                .any(|(run_id, _, outcome, _)| {
                    run_id == &format!("child-{}:tool_1_sub-1", sample_request().run_id)
                        && *outcome == pb::TerminalOutcome::Completed as i32
                }),
            "a failed answer write must not leave the child obligation open"
        );
    }

    // Asserts the whole delegation contract in one run: nesting, isolation,
    // result propagation, observability, and terminal-once.
    #[allow(clippy::too_many_lines)]
    #[tokio::test]
    async fn subagent_runs_a_real_nested_loop_in_an_isolated_context_and_returns_its_answer() {
        const DELEGATED_GOAL: &str = "Find the current Bergen weather";
        const SUBAGENT_ANSWER: &str = "Bergen: 8 degrees and raining.";

        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        let session_channel = spawn_session_channel(rec.clone()).await;

        // One shared script, consumed in call order: parent delegates, the
        // nested loop runs a tool round then answers, the parent answers from it.
        let observed_messages: ObservedMessages = Arc::new(Mutex::new(Vec::new()));
        let inference_channel = spawn_inference_channel_with_messages(
            vec![
                Scripted::ToolCalls {
                    content: "Delegating the lookup.".to_owned(),
                    calls: vec![subagent_call(
                        "sub-1",
                        &format!(r#"{{"goal":"{DELEGATED_GOAL}"}}"#),
                    )],
                },
                Scripted::ToolCalls {
                    content: String::new(),
                    calls: vec![failing_tool_call("wx-1")],
                },
                Scripted::Answer(SUBAGENT_ANSWER.to_owned()),
                Scripted::Answer(format!("Delegated result: {SUBAGENT_ANSWER}")),
            ],
            observed_messages.clone(),
        )
        .await;
        let state = crate::state::StateStore::new();

        let resp = run_agent(
            &state,
            session_channel,
            inference_channel,
            sample_request(),
            None,
            Some("session-token".to_owned()),
            "inference-token".to_owned(),
            &ALLOW_CAPABILITY_POLICY,
            &STATIC_TERMINAL_TOKENS,
            None,
            None,
            &test_sandbox_manager_client(),
            "test-backend",
            None,
            &test_sandbox_tokens(),
        )
        .await
        .expect("a delegating agent run should succeed");

        // RESULT PROPAGATION: the parent answered from the subagent's real text.
        assert_eq!(resp.status, "completed");
        assert_eq!(
            resp.final_output,
            format!("Delegated result: {SUBAGENT_ANSWER}")
        );
        assert_eq!(
            resp.rounds_executed, 4,
            "2 parent rounds + the 2 rounds its delegate spent, all charged to the run"
        );

        // ISOLATION: the nested loop was seeded with the delegated goal only —
        // never the parent's transcript or goal.
        let observed = observed_messages.lock().unwrap();
        assert_eq!(observed.len(), 4, "parent, child, child, parent");
        assert_eq!(seeded_goal(&observed[0]), sample_request().goal);
        assert_eq!(seeded_goal(&observed[1]), DELEGATED_GOAL);
        assert_eq!(
            observed[1].len(),
            2,
            "a fresh history: system preamble + the delegated goal, nothing inherited"
        );
        assert!(
            !observed[1]
                .iter()
                .any(|message| message.content.contains(&sample_request().goal)),
            "the parent's goal must not leak into the delegated context"
        );
        // Conversely the parent's own context never grows the child's transcript,
        // only its conclusion.
        assert!(
            !observed[3]
                .iter()
                .any(|message| message.content.contains("Tool results")
                    && message.content.contains("yr_weather")),
            "the child's intermediate tool traffic must stay out of the parent's context"
        );
        drop(observed);

        let r = rec.lock().unwrap();
        // DURABLE OBSERVABILITY: the nested tool step is recorded through the
        // same non-terminal audit path, keyed under the delegating call's step
        // id, and it lands BEFORE the delegating step completes.
        assert_eq!(
            r.completed,
            vec![
                ("tool_1_sub-1.tool_1_wx-1".to_owned(), "running".to_owned()),
                ("tool_1_sub-1".to_owned(), "running".to_owned()),
            ],
            "the delegated run is visible, attributed, and non-terminal"
        );
        // TERMINAL-ONCE, now asserted PER RUN rather than by total count.
        //
        // The old assertion was `managed_terminal_outcomes.len() == 1`, which
        // was a proxy: it read as "one receipt per run" but actually encoded
        // "nested loops terminalize nothing at all". Delegated child runs are
        // now durable managed runs, so each legitimately settles its own
        // obligation — and the total is no longer the invariant. The invariant
        // is that the PARENT run has exactly one, which is what a
        // double-terminalization bug would break, and the total-count form
        // could not distinguish from a child settling correctly.
        let parent_receipts = r
            .managed_terminal_outcomes
            .iter()
            .filter(|(run_id, ..)| run_id == &sample_request().run_id)
            .count();
        assert_eq!(
            parent_receipts, 1,
            "TERMINAL-ONCE: the parent run must have exactly one managed receipt"
        );
        let mut settled: Vec<&String> = r
            .managed_terminal_outcomes
            .iter()
            .map(|(run_id, ..)| run_id)
            .collect();
        let before_dedup = settled.len();
        settled.sort();
        settled.dedup();
        assert_eq!(
            settled.len(),
            before_dedup,
            "TERMINAL-ONCE: no run may be terminalized twice, parent or child"
        );

        // The delegation is now durable and in the lineage graph — the gap that
        // made GetSubagentLineage return nothing for every real subagent.
        assert_eq!(
            r.started_child_runs.len(),
            1,
            "the delegation must register exactly one durable child run"
        );
        let (parent_run_id, goal, start_key, agent_id) = &r.started_child_runs[0];
        assert_eq!(parent_run_id, &sample_request().run_id);
        assert_eq!(
            agent_id, "task",
            "the child run is labelled by the subagent's task label \
             (subagent.task -> task), not by the tool name or the step id"
        );
        assert!(
            !goal.is_empty(),
            "the child run must carry the delegated goal, or the lineage row says nothing"
        );
        assert_eq!(
            start_key,
            &format!("{}:tool_1_sub-1", sample_request().run_id),
            "start_key must be identifiers only — stable across retries and \
             carrying no prompt or tool content"
        );
        assert_eq!(
            r.lineage_edges,
            vec![(
                sample_request().run_id.clone(),
                format!("child-{}:tool_1_sub-1", sample_request().run_id),
                pb::SubagentRole::Generic as i32,
            )],
            "the parent -> child edge must be recorded, since the lineage \
             endpoint reads subagent_edges and not runs.parent_run_id"
        );
        // The child settles its own obligation; an unsettled managed run is
        // force-failed by the deadline watchdog.
        assert!(
            r.managed_terminal_outcomes
                .iter()
                .any(|(run_id, _, outcome, _)| {
                    run_id == &format!("child-{}:tool_1_sub-1", sample_request().run_id)
                        && *outcome == pb::TerminalOutcome::Completed as i32
                }),
            "the delegated child run must be settled Completed"
        );
        assert_eq!(
            r.plan_transitions,
            vec![
                (pb::PlanState::Draft as i32, pb::PlanState::Executing as i32),
                (
                    pb::PlanState::Executing as i32,
                    pb::PlanState::Completed as i32
                ),
            ],
            "a nested loop must not add plan transitions of its own"
        );

        let (output, error) = persisted_step(&r, "tool_1_sub-1");
        assert!(
            output.contains(SUBAGENT_ANSWER),
            "the subagent's answer IS the tool output: {output}"
        );
        assert!(error.is_empty(), "a successful delegation has no error");
        assert!(
            output.contains("data_category=customer_private"),
            "a subagent inherits knowledge_search, so its result is classified private: {output}"
        );
        assert!(
            !output.contains("spawned"),
            "the fabricated 'spawned <tool>' summary is gone: {output}"
        );

        // COLD RESUME (harness-adoption 1.1). The answer above lives in the
        // parent's live message history and dies with the process. What survives
        // is this: the answer written to the CHILD run's own record, readable
        // afterwards through `read_subagent_result` once the user allows it.
        //
        // The parent's own run must NOT be the one carrying it — that is the
        // whole authority split. Asserting the run id, not just the presence,
        // is what makes this a boundary test rather than a "something was
        // written" test.
        assert_eq!(
            r.recorded_run_outputs.len(),
            1,
            "the delegation's answer must be recorded exactly once"
        );
        let (answer_run_id, answer) = &r.recorded_run_outputs[0];
        assert_eq!(
            answer_run_id,
            &format!("child-{}:tool_1_sub-1", sample_request().run_id),
            "the answer belongs to the CHILD run's record, never the parent's"
        );
        assert_ne!(
            answer_run_id,
            &sample_request().run_id,
            "writing it onto the parent run would make the conclusion readable \
             with no permission asked, which is exactly what the split prevents"
        );
        assert!(
            answer.contains(SUBAGENT_ANSWER),
            "the recorded answer must be the subagent's actual conclusion: {answer}"
        );
    }

    #[tokio::test]
    async fn subagent_may_not_spawn_another_subagent() {
        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        let session_channel = spawn_session_channel(rec.clone()).await;

        // The nested loop asks to delegate again. The recursion guard must refuse
        // it with an explanation instead of opening a third level.
        let observed_messages: ObservedMessages = Arc::new(Mutex::new(Vec::new()));
        let inference_channel = spawn_inference_channel_with_messages(
            vec![
                Scripted::ToolCalls {
                    content: String::new(),
                    calls: vec![subagent_call("sub-outer", r#"{"goal":"outer task"}"#)],
                },
                Scripted::ToolCalls {
                    content: String::new(),
                    calls: vec![subagent_call("sub-inner", r#"{"goal":"inner task"}"#)],
                },
                Scripted::Answer("I completed it myself.".to_owned()),
                Scripted::Answer("Done.".to_owned()),
            ],
            observed_messages.clone(),
        )
        .await;
        let state = crate::state::StateStore::new();

        let resp = run_agent(
            &state,
            session_channel,
            inference_channel,
            sample_request(),
            None,
            Some("session-token".to_owned()),
            "inference-token".to_owned(),
            &ALLOW_CAPABILITY_POLICY,
            &STATIC_TERMINAL_TOKENS,
            None,
            None,
            &test_sandbox_manager_client(),
            "test-backend",
            None,
            &test_sandbox_tokens(),
        )
        .await
        .expect("a refused nested delegation must not fail the run");

        assert_eq!(resp.status, "completed");
        assert_eq!(resp.final_output, "Done.");
        assert_eq!(
            observed_messages.lock().unwrap().len(),
            4,
            "exactly four inference rounds: no third-level loop ever started"
        );

        let r = rec.lock().unwrap();
        let (output, error) = persisted_step(&r, "tool_1_sub-outer.tool_1_sub-inner");
        assert!(
            error.contains("may not spawn another subagent"),
            "the refusal must be an explicit error the model can act on: {error}"
        );
        assert!(
            output.is_empty() || !output.contains("spawned"),
            "a refused spawn must never look like a success: {output}"
        );
        // The outer delegation still returned its own honest answer.
        let (outer_output, outer_error) = persisted_step(&r, "tool_1_sub-outer");
        assert!(outer_error.is_empty(), "{outer_error}");
        assert!(
            outer_output.contains("I completed it myself."),
            "{outer_output}"
        );
    }

    /// THE safety property for pre-dispatch validation, in this loop too: it must
    /// have no opinion on any legitimate call against our own catalogue. A
    /// validator that refuses a call the executor would have accepted breaks a
    /// working tool, which is strictly worse than not validating.
    #[test]
    fn validation_never_objects_to_a_legitimate_call_on_the_offered_catalogue() {
        let offered = offered_tool_defs();
        for def in &offered {
            let args = minimal_valid_arguments(&def.parameters_json);
            assert!(
                argument_problem(&offered, &def.name, &args, "").is_none(),
                "'{}' rejects its own minimally valid arguments {args}",
                def.name
            );
        }
    }

    /// And it must not form an opinion about a tool whose schema this run does not
    /// hold. A client-declared or MCP tool absent from the offered set is the
    /// purpose-lock's business, not the validator's — guessing a schema would be
    /// worse than not checking.
    #[test]
    fn validation_has_no_opinion_on_a_tool_it_was_not_offered() {
        let offered = offered_tool_defs();
        assert!(argument_problem(&offered, "mcp__acme__do_thing", "{}", "").is_none());
        assert!(argument_problem(&[], "yr_weather", "{}", "").is_none());
    }

    /// Smallest object satisfying a schema's `required` fields — enough to clear
    /// validation, nothing invented beyond that.
    fn minimal_valid_arguments(parameters_json: &str) -> String {
        let Ok(schema) = serde_json::from_str::<serde_json::Value>(parameters_json) else {
            return "{}".to_owned();
        };
        let props = schema.get("properties").and_then(|p| p.as_object());
        let required = schema
            .get("required")
            .and_then(|r| r.as_array())
            .map(|r| r.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
            .unwrap_or_default();
        let mut out = serde_json::Map::new();
        for field in required {
            let spec = props.and_then(|p| p.get(field));
            let value = match spec.and_then(|s| s.get("enum")).and_then(|e| e.as_array()) {
                Some(values) if !values.is_empty() => values[0].clone(),
                _ => match spec.and_then(|s| s.get("type")).and_then(|t| t.as_str()) {
                    Some("number" | "integer") => serde_json::json!(1),
                    Some("boolean") => serde_json::json!(true),
                    Some("array") => serde_json::json!([]),
                    Some("object") => serde_json::json!({}),
                    _ => serde_json::json!("x"),
                },
            };
            out.insert(field.to_owned(), value);
        }
        serde_json::Value::Object(out).to_string()
    }

    /// A delegated subagent has no children (`MAX_DEPTH == 1`), so both
    /// delegation-record tools would only ever return nothing for it.
    ///
    /// It must be TOLD that, not handed an empty list. An empty list reads as
    /// "the delegations I made produced nothing" — a claim about work it never
    /// did, and the exact class of fabrication the fake `spawned <tool>` summary
    /// used to produce.
    #[tokio::test]
    async fn a_delegated_subagent_is_told_why_it_has_no_delegation_records() {
        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        let session_channel = spawn_session_channel(rec.clone()).await;
        let observed_messages: ObservedMessages = Arc::new(Mutex::new(Vec::new()));
        let inference_channel = spawn_inference_channel_with_messages(
            vec![
                Scripted::ToolCalls {
                    content: String::new(),
                    calls: vec![subagent_call(
                        "sub-1",
                        r#"{"goal":"summarise earlier findings"}"#,
                    )],
                },
                Scripted::ToolCalls {
                    content: String::new(),
                    calls: vec![pb::ToolCall {
                        id: "list-1".to_owned(),
                        name: crate::runtime_loop::subagent_results::LIST_TOOL.to_owned(),
                        arguments_json: "{}".to_owned(),
                    }],
                },
                Scripted::Answer("I have no delegations of my own.".to_owned()),
                Scripted::Answer("Reported back.".to_owned()),
            ],
            observed_messages.clone(),
        )
        .await;
        let state = crate::state::StateStore::new();

        // `auto`, deliberately: this refusal must not depend on a posture. If it
        // only fired under `ask` it would be the pre-existing approval refusal
        // wearing this test's name.
        let mut req = sample_request();
        req.mode = "auto".to_owned();
        let resp = run_agent(
            &state,
            session_channel,
            inference_channel,
            req,
            None,
            Some("session-token".to_owned()),
            "inference-token".to_owned(),
            &ALLOW_CAPABILITY_POLICY,
            &STATIC_TERMINAL_TOKENS,
            None,
            None,
            &test_sandbox_manager_client(),
            "test-backend",
            None,
            &test_sandbox_tokens(),
        )
        .await
        .expect("a refused record read inside a subagent must not fail the run");

        assert_eq!(resp.status, "completed");
        drop(rec.lock().unwrap());
        let observed = observed_messages.lock().unwrap();
        assert_eq!(observed.len(), 4, "parent, child, child, parent");
        let subagent_next_round = observed[2]
            .iter()
            .rfind(|message| message.role == "user")
            .map_or("", |message| message.content.as_str());
        assert!(
            subagent_next_round.contains("only available to the main agent")
                && subagent_next_round.contains("no subagent results"),
            "the refusal must reach the subagent's next round with its reason stated: \
             {subagent_next_round}"
        );
    }

    /// The graded rung, enforced per call in the real loop.
    ///
    /// `auto` posture deliberately: under `auto` nothing is gated on risk, so a
    /// refusal here can only be the rung. That is the whole point — the ladder
    /// expresses a state neither `plan_mode` (all or nothing) nor the posture
    /// (`auto` gates nothing) can.
    #[tokio::test]
    async fn a_granted_rung_refuses_what_it_does_not_cover_and_permits_what_it_does() {
        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        let session_channel = spawn_session_channel(rec.clone()).await;
        let observed_messages: ObservedMessages = Arc::new(Mutex::new(Vec::new()));
        let inference_channel = spawn_inference_channel_with_messages(
            vec![
                Scripted::ToolCalls {
                    content: String::new(),
                    calls: vec![
                        // Above the granted rung: an outbound action.
                        pb::ToolCall {
                            id: "send-1".to_owned(),
                            name: "send_invoice".to_owned(),
                            arguments_json: "{}".to_owned(),
                        },
                        // At the granted rung: a read, which must still run.
                        pb::ToolCall {
                            id: "look-1".to_owned(),
                            name: "company_lookup".to_owned(),
                            arguments_json: r#"{"query":"Aquatiq"}"#.to_owned(),
                        },
                    ],
                },
                Scripted::Answer("I could not send it; here is what I found.".to_owned()),
            ],
            observed_messages.clone(),
        )
        .await;
        let state = crate::state::StateStore::new();

        let mut req = sample_request();
        req.mode = "auto".to_owned();
        req.plan_mode = false;
        req.autonomy_rung = pb::AutonomyRung::WorkspaceWrite as i32;
        req.tools = vec![pb::ToolDefinition {
            name: "send_invoice".to_owned(),
            description: "Send an invoice to a customer.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{}}"#.to_owned(),
        }];
        let resp = run_agent(
            &state,
            session_channel,
            inference_channel,
            req,
            None,
            Some("session-token".to_owned()),
            "inference-token".to_owned(),
            &ALLOW_CAPABILITY_POLICY,
            &STATIC_TERMINAL_TOKENS,
            None,
            None,
            &test_sandbox_manager_client(),
            "test-backend",
            None,
            &test_sandbox_tokens(),
        )
        .await
        .expect("a refused rung must not fail the run");
        assert_eq!(resp.status, "completed");

        drop(rec.lock().unwrap());
        let observed = observed_messages.lock().unwrap();
        let next_round = observed[1]
            .iter()
            .rfind(|message| message.role == "user")
            .map_or("", |message| message.content.as_str());
        assert!(
            next_round.contains("danger_full_access") && next_round.contains("workspace_write"),
            "the refusal must name both rungs so the model knows how far short it is: {next_round}"
        );
        assert!(
            next_round.contains("requires a person"),
            "the model must be told it cannot widen its own authority: {next_round}"
        );
        // The read at the granted rung was NOT collateral damage.
        assert!(
            next_round.contains("company_lookup"),
            "a refusal above the rung must not suppress a call at it: {next_round}"
        );
    }

    #[tokio::test]
    async fn a_delegated_subagent_may_never_run_a_risky_tool_even_under_auto_mode() {
        // Leaf/orchestrator role split (§7.3): even under `auto` -- where the
        // TOP-LEVEL run would execute `delete_records` with no gating at all --
        // a delegated subagent must refuse it outright. `auto` mode is chosen
        // deliberately, not `ask`, so this proves the NEW blocklist fired, not
        // the pre-existing "a subagent cannot request approval" refusal (which
        // only applies under `ask` and would mask this from ever being tested).
        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        let session_channel = spawn_session_channel(rec.clone()).await;
        let observed_messages: ObservedMessages = Arc::new(Mutex::new(Vec::new()));
        let inference_channel = spawn_inference_channel_with_messages(
            vec![
                Scripted::ToolCalls {
                    content: String::new(),
                    calls: vec![subagent_call("sub-1", r#"{"goal":"purge stale records"}"#)],
                },
                Scripted::ToolCalls {
                    content: String::new(),
                    calls: vec![pb::ToolCall {
                        id: "del-1".to_owned(),
                        name: "delete_records".to_owned(),
                        arguments_json: "{}".to_owned(),
                    }],
                },
                Scripted::Answer("I could not complete this myself.".to_owned()),
                Scripted::Answer("Reported back: could not complete it.".to_owned()),
            ],
            observed_messages.clone(),
        )
        .await;
        let state = crate::state::StateStore::new();

        let mut req = sample_request();
        req.mode = "auto".to_owned();
        req.tools = vec![pb::ToolDefinition {
            name: "delete_records".to_owned(),
            description: "Delete records (destructive).".to_owned(),
            parameters_json: r#"{"type":"object","properties":{}}"#.to_owned(),
        }];
        let resp = run_agent(
            &state,
            session_channel,
            inference_channel,
            req,
            None,
            Some("session-token".to_owned()),
            "inference-token".to_owned(),
            &ALLOW_CAPABILITY_POLICY,
            &STATIC_TERMINAL_TOKENS,
            None,
            None,
            &test_sandbox_manager_client(),
            "test-backend",
            None,
            &test_sandbox_tokens(),
        )
        .await
        .expect("a refused risky tool inside a subagent must not fail the run");

        assert_eq!(resp.status, "completed");
        assert_eq!(resp.final_output, "Reported back: could not complete it.");

        // The rejection happens in the sequential pre-pass (same phase as
        // purpose-lock), before any `record_tool_step` audit write, so it is
        // never durably persisted on its own -- it is fed back to the
        // subagent as its next round's tool context, exactly like a
        // purpose-lock rejection. Verify it landed there.
        drop(rec.lock().unwrap());
        let observed = observed_messages.lock().unwrap();
        assert_eq!(observed.len(), 4, "parent, child, child, parent");
        // The tool-context message `format_tool_context` appends is the LAST
        // "user" message in the subagent's second-round request, not the
        // first (which is still the original delegated goal).
        let subagent_next_round = observed[2]
            .iter()
            .rfind(|message| message.role == "user")
            .map_or("", |message| message.content.as_str());
        assert!(
            subagent_next_round.contains("may never run")
                && subagent_next_round.contains("regardless of the run's permission mode"),
            "the refusal must reach the subagent's own next round, naming itself as the \
             leaf-role blocklist rather than a generic denial: {subagent_next_round}"
        );
        assert!(
            !subagent_next_round.contains("deleted"),
            "a refused risky call must never look like it actually ran: {subagent_next_round}"
        );
    }

    #[tokio::test]
    async fn a_truncated_round_refuses_only_its_last_tool_call_and_runs_the_rest() {
        // pi's version of this guard fails EVERY call in a length-stopped
        // message. Providers emit content blocks in order, so only the final
        // one can be half-written — failing the earlier, complete calls would
        // discard valid work. This pins the more precise behaviour (and
        // matches model-gateway's inline loop, per the contract test).
        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        let session_channel = spawn_session_channel(rec.clone()).await;
        let observed_messages: ObservedMessages = Arc::new(Mutex::new(Vec::new()));
        let inference_channel = spawn_inference_channel_with_messages(
            vec![
                Scripted::TruncatedToolCalls {
                    content: String::new(),
                    calls: vec![
                        // Complete: dispatches for real and fails fast on its
                        // own missing coordinates, proving it RAN.
                        failing_tool_call("wx-complete"),
                        // Truncated: must be refused before dispatch.
                        failing_tool_call("wx-cut-off"),
                    ],
                },
                Scripted::Answer("Retried the cut-off call properly.".to_owned()),
            ],
            observed_messages.clone(),
        )
        .await;
        let state = crate::state::StateStore::new();

        let resp = run_agent(
            &state,
            session_channel,
            inference_channel,
            sample_request(),
            None,
            Some("session-token".to_owned()),
            "inference-token".to_owned(),
            &ALLOW_CAPABILITY_POLICY,
            &STATIC_TERMINAL_TOKENS,
            None,
            None,
            &test_sandbox_manager_client(),
            "test-backend",
            None,
            &test_sandbox_tokens(),
        )
        .await
        .expect("a truncated round must not fail the run");
        assert_eq!(resp.status, "completed");

        // The complete call was audited (it really dispatched); the truncated
        // one never reached `record_tool_step`, exactly like a purpose-lock
        // rejection.
        let r = rec.lock().unwrap();
        let recorded: Vec<&str> = r.completed.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(
            recorded,
            vec!["tool_1_wx-complete"],
            "only the complete call should have been dispatched/audited: {recorded:?}"
        );
        drop(r);

        // And the model was told WHY, in its next round's tool context.
        let observed = observed_messages.lock().unwrap();
        let next_round = observed[1]
            .iter()
            .rfind(|message| message.role == "user")
            .map_or("", |message| message.content.as_str());
        assert!(
            next_round.contains("output token limit") && next_round.contains("truncated"),
            "the refusal must name truncation as the cause: {next_round}"
        );
    }

    #[tokio::test]
    async fn plan_mode_refuses_a_risky_tool_at_the_top_level_even_under_auto_mode() {
        // Server-side plan-mode enforcement (§13.5 item 3). `mode = "auto"`
        // is chosen deliberately: under `ask` this would ALSO be blocked by
        // the pre-existing approval gate, which would mask whether the NEW
        // plan-mode check fired at all. This is the top-level (depth 0) run
        // -- the leaf-blocklist test above covers the delegated-subagent
        // case; this one proves plan mode restricts the ORCHESTRATOR itself,
        // which depth alone never would.
        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        let session_channel = spawn_session_channel(rec.clone()).await;
        let inference_channel = spawn_inference_channel(vec![
            Scripted::ToolCalls {
                content: String::new(),
                calls: vec![pb::ToolCall {
                    id: "del-1".to_owned(),
                    name: "delete_records".to_owned(),
                    arguments_json: "{}".to_owned(),
                }],
            },
            Scripted::Answer("I described the plan instead of deleting anything.".to_owned()),
        ])
        .await;
        let state = crate::state::StateStore::new();

        let gated_tools = vec![pb::ToolDefinition {
            name: "delete_records".to_owned(),
            description: "Delete records (destructive).".to_owned(),
            parameters_json: r#"{"type":"object","properties":{}}"#.to_owned(),
        }];
        let mut req = sample_request();
        req.mode = "auto".to_owned();
        req.plan_mode = true;
        let resp = run_agent_with_tools(
            &state,
            session_channel,
            inference_channel,
            req,
            gated_tools,
            None,
            Some("session-token".to_owned()),
            "inference-token".to_owned(),
            &ALLOW_CAPABILITY_POLICY,
            &STATIC_TERMINAL_TOKENS,
            None,
            None,
            &test_sandbox_manager_client(),
            "test-backend",
            None,
            &test_sandbox_tokens(),
        )
        .await
        .expect("a refused plan-mode tool call must not fail the run");

        assert_eq!(resp.status, "completed");
        assert_eq!(
            resp.final_output,
            "I described the plan instead of deleting anything."
        );
        // Never durably recorded as having run (same reasoning as the
        // leaf-blocklist rejection: it is fed back as tool context, not
        // audited as an executed step) -- confirm no step was persisted.
        let r = rec.lock().unwrap();
        assert!(
            r.completed.is_empty(),
            "plan mode must refuse before any step is dispatched/recorded: {:?}",
            r.completed
        );
    }

    #[tokio::test]
    async fn subagent_honors_a_requested_budget_and_reports_exhaustion_honestly() {
        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        let session_channel = spawn_session_channel(rec.clone()).await;

        // A one-round subagent that spends that round on a tool call cannot
        // answer. The parent must be told so, not handed a fake result.
        let observed_messages: ObservedMessages = Arc::new(Mutex::new(Vec::new()));
        let inference_channel = spawn_inference_channel_with_messages(
            vec![
                Scripted::ToolCalls {
                    content: String::new(),
                    calls: vec![subagent_call(
                        "sub-b",
                        r#"{"goal":"deep research","max_rounds":1}"#,
                    )],
                },
                Scripted::ToolCalls {
                    content: String::new(),
                    calls: vec![failing_tool_call("wx-1")],
                },
                Scripted::Answer("The delegated lookup did not finish.".to_owned()),
            ],
            observed_messages.clone(),
        )
        .await;
        let state = crate::state::StateStore::new();

        let resp = run_agent(
            &state,
            session_channel,
            inference_channel,
            sample_request(),
            None,
            Some("session-token".to_owned()),
            "inference-token".to_owned(),
            &ALLOW_CAPABILITY_POLICY,
            &STATIC_TERMINAL_TOKENS,
            None,
            None,
            &test_sandbox_manager_client(),
            "test-backend",
            None,
            &test_sandbox_tokens(),
        )
        .await
        .expect("a failed delegation must not fail the parent run");

        assert_eq!(resp.status, "completed");
        assert_eq!(resp.final_output, "The delegated lookup did not finish.");
        assert_eq!(
            observed_messages.lock().unwrap().len(),
            3,
            "the subagent got exactly the 1 round it asked for, not the default 12"
        );

        let r = rec.lock().unwrap();
        let (output, error) = persisted_step(&r, "tool_1_sub-b");
        assert!(
            error.contains("1-round budget"),
            "the parent is told the real budget that ran out: {error}"
        );
        assert_eq!(
            output.trim_end(),
            "[data_category=customer_private zdr=false tool=subagent.task]",
            "an exhausted subagent contributes audit metadata only — never a result"
        );
    }

    #[tokio::test]
    async fn subagent_budget_cannot_exceed_the_parents_remaining_rounds() {
        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        let session_channel = spawn_session_channel(rec.clone()).await;

        // The parent has a 2-round budget and delegates on round 1, so exactly 1
        // round is left to lend — however many the call asks for. The delegated
        // round is then charged to the run, which is what stops the parent from
        // taking a second round of its own.
        let observed_messages: ObservedMessages = Arc::new(Mutex::new(Vec::new()));
        let inference_channel = spawn_inference_channel_with_messages(
            vec![
                Scripted::ToolCalls {
                    content: String::new(),
                    calls: vec![subagent_call(
                        "sub-greedy",
                        r#"{"goal":"boil the ocean","max_rounds":99}"#,
                    )],
                },
                Scripted::ToolCalls {
                    content: String::new(),
                    calls: vec![failing_tool_call("wx-1")],
                },
            ],
            observed_messages.clone(),
        )
        .await;
        let state = crate::state::StateStore::new();

        let mut req = sample_request();
        req.max_rounds = 2;
        let resp = run_agent(
            &state,
            session_channel,
            inference_channel,
            req,
            None,
            Some("session-token".to_owned()),
            "inference-token".to_owned(),
            &ALLOW_CAPABILITY_POLICY,
            &STATIC_TERMINAL_TOKENS,
            None,
            None,
            &test_sandbox_manager_client(),
            "test-backend",
            None,
            &test_sandbox_tokens(),
        )
        .await
        .expect("an exhausted run is still finalized");

        assert_eq!(
            observed_messages.lock().unwrap().len(),
            2,
            "1 parent round + the 1 round it had left to lend; a 99-round request \
             cannot buy more than the run owns"
        );
        assert_eq!(
            resp.status, "failed",
            "the run really did run out of budget"
        );
        assert_eq!(resp.final_output, GRACEFUL_FAILURE_REPLY);
        assert_eq!(
            resp.rounds_executed, 2,
            "the delegated round is charged to the run's budget, not free"
        );

        let r = rec.lock().unwrap();
        let (_, error) = persisted_step(&r, "tool_1_sub-greedy");
        assert!(
            error.contains("1-round budget"),
            "the clamp to the parent's remainder is what the subagent actually got: {error}"
        );
    }

    /// THE continuation property, and the bug it fixes.
    ///
    /// `start_key` is `<parent_run_id>:<parent_step_id>` — identifiers only — so
    /// a re-driven parent step reuses its child run instead of forking the
    /// lineage. That makes re-running the nested loop actively wrong: the child
    /// already carries an IMMUTABLE terminal receipt, so a second
    /// `RecordTerminalOutcome` returns the FIRST outcome. A replay that re-ran
    /// and succeeded would hand the parent a good answer while the ledger kept
    /// saying the child failed.
    ///
    /// So a replay resumes from the recorded answer and charges zero rounds —
    /// asserted by the round count, which is what proves no work was redone.
    #[tokio::test]
    async fn a_replayed_delegation_resumes_from_its_recorded_answer_without_re_running() {
        const RECORDED: &str = "Bring, 412 NOK (from the first attempt)";
        let start_key = format!("{}:tool_1_sub-1", sample_request().run_id);
        let child_run_id = format!("child-{start_key}");

        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        {
            // The delegation already happened: a child run exists for this
            // start_key and its answer is on the record.
            let mut r = rec.lock().unwrap();
            r.started_child_runs.push((
                sample_request().run_id.clone(),
                "lookup the carrier".to_owned(),
                start_key,
                "task".to_owned(),
            ));
            r.recorded_run_outputs
                .push((child_run_id.clone(), RECORDED.to_owned()));
        }
        let session_channel = spawn_session_channel(rec.clone()).await;
        let observed_messages: ObservedMessages = Arc::new(Mutex::new(Vec::new()));
        // Exactly TWO scripted rounds: the parent's delegating round and its
        // answer. A third would mean the nested loop ran.
        let inference_channel = spawn_inference_channel_with_messages(
            vec![
                Scripted::ToolCalls {
                    content: String::new(),
                    calls: vec![subagent_call("sub-1", r#"{"goal":"lookup the carrier"}"#)],
                },
                Scripted::Answer(format!("The carrier is {RECORDED}")),
            ],
            observed_messages.clone(),
        )
        .await;
        let state = crate::state::StateStore::new();

        let resp = run_agent(
            &state,
            session_channel,
            inference_channel,
            sample_request(),
            None,
            Some("session-token".to_owned()),
            "inference-token".to_owned(),
            &ALLOW_CAPABILITY_POLICY,
            &STATIC_TERMINAL_TOKENS,
            None,
            None,
            &test_sandbox_manager_client(),
            "test-backend",
            None,
            &test_sandbox_tokens(),
        )
        .await
        .expect("a resumed delegation completes the run");

        assert_eq!(resp.status, "completed");
        // `rounds_executed` is parent + delegated. Two here means the parent's
        // delegating and answering rounds and ZERO delegated rounds — the
        // non-replay version of this same script charges 4
        // (`subagent_runs_a_real_nested_loop_...`), so the difference is exactly
        // the work a resume did not redo.
        assert_eq!(
            resp.rounds_executed, 2,
            "2 parent rounds and NO delegated round: a resume redoes no work"
        );
        assert_eq!(
            observed_messages.lock().unwrap().len(),
            2,
            "two inference calls — the nested loop never ran"
        );

        let r = rec.lock().unwrap();
        let (output, error) = persisted_step(&r, "tool_1_sub-1");
        assert!(
            output.contains(RECORDED),
            "the recorded answer IS the tool output: {output}"
        );
        assert!(error.is_empty(), "a resumed delegation is not a failure");
        // And no second answer was written: the record already had one, and
        // overwriting it would replace the answer the receipt was settled for.
        assert_eq!(
            r.recorded_run_outputs.len(),
            1,
            "a resume must not rewrite the recorded answer"
        );
    }

    /// A replay whose earlier attempt left nothing to hand back must SAY so, not
    /// re-run. Re-running is what produces the good-answer/bad-ledger pair, and
    /// it is exactly the case where re-running looks most tempting.
    #[tokio::test]
    async fn a_replayed_delegation_with_nothing_recorded_is_refused_not_re_run() {
        let start_key = format!("{}:tool_1_sub-1", sample_request().run_id);
        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        rec.lock().unwrap().started_child_runs.push((
            sample_request().run_id.clone(),
            "lookup the carrier".to_owned(),
            start_key,
            "task".to_owned(),
        ));
        let session_channel = spawn_session_channel(rec.clone()).await;
        let observed_messages: ObservedMessages = Arc::new(Mutex::new(Vec::new()));
        let inference_channel = spawn_inference_channel_with_messages(
            vec![
                Scripted::ToolCalls {
                    content: String::new(),
                    calls: vec![subagent_call("sub-1", r#"{"goal":"lookup the carrier"}"#)],
                },
                Scripted::Answer("I could not get that finding.".to_owned()),
            ],
            observed_messages.clone(),
        )
        .await;
        let state = crate::state::StateStore::new();

        let resp = run_agent(
            &state,
            session_channel,
            inference_channel,
            sample_request(),
            None,
            Some("session-token".to_owned()),
            "inference-token".to_owned(),
            &ALLOW_CAPABILITY_POLICY,
            &STATIC_TERMINAL_TOKENS,
            None,
            None,
            &test_sandbox_manager_client(),
            "test-backend",
            None,
            &test_sandbox_tokens(),
        )
        .await
        .expect("a refused replay must not fail the run");
        assert_eq!(resp.status, "completed");
        assert_eq!(
            observed_messages.lock().unwrap().len(),
            2,
            "the nested loop must not run for a replay"
        );

        let r = rec.lock().unwrap();
        let (_, error) = persisted_step(&r, "tool_1_sub-1");
        assert!(
            error.contains("already ran once for this step"),
            "the refusal must name the replay as the reason: {error}"
        );
        assert!(
            error.contains("yourself"),
            "and tell the model what to do instead: {error}"
        );
    }

    /// A delegation still in flight from an earlier attempt is a different
    /// refusal: starting it again would fork one delegation into two.
    #[tokio::test]
    async fn a_replayed_delegation_still_running_is_refused_as_in_flight() {
        let start_key = format!("{}:tool_1_sub-1", sample_request().run_id);
        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        {
            let mut r = rec.lock().unwrap();
            r.started_child_runs.push((
                sample_request().run_id.clone(),
                "lookup the carrier".to_owned(),
                start_key,
                "task".to_owned(),
            ));
            r.replayed_run_status = Some("running".to_owned());
        }
        let session_channel = spawn_session_channel(rec.clone()).await;
        let inference_channel = spawn_inference_channel(vec![
            Scripted::ToolCalls {
                content: String::new(),
                calls: vec![subagent_call("sub-1", r#"{"goal":"lookup the carrier"}"#)],
            },
            Scripted::Answer("Still waiting on that.".to_owned()),
        ])
        .await;
        let state = crate::state::StateStore::new();

        run_agent(
            &state,
            session_channel,
            inference_channel,
            sample_request(),
            None,
            Some("session-token".to_owned()),
            "inference-token".to_owned(),
            &ALLOW_CAPABILITY_POLICY,
            &STATIC_TERMINAL_TOKENS,
            None,
            None,
            &test_sandbox_manager_client(),
            "test-backend",
            None,
            &test_sandbox_tokens(),
        )
        .await
        .expect("an in-flight replay must not fail the run");

        let r = rec.lock().unwrap();
        let (_, error) = persisted_step(&r, "tool_1_sub-1");
        assert!(
            error.contains("still running") && error.contains("fork one delegation into two"),
            "an in-flight replay is its own refusal, not 'nothing recorded': {error}"
        );
    }

    /// The per-run child ceiling (`subagent::MAX_TOTAL_CHILDREN`), which the
    /// round budget genuinely cannot express.
    ///
    /// # Why the round budget is not already this
    ///
    /// The premise this test started from was wrong and the failure said so: a
    /// single round cannot fan out at all, because the first `spawn` claims the
    /// whole remaining pool with `swap(0)` and every sibling is then refused for
    /// lack of budget. So fan-out within one round is already bounded to one.
    ///
    /// The real runaway is ACROSS rounds — one delegation per round, for as many
    /// rounds as the budget allows. A generous `max_rounds` therefore permits
    /// dozens of durable child runs, lineage rows and managed obligations from a
    /// single conversation. This test gives the run far more budget than the
    /// ceiling and proves the ceiling, not the budget, is what stops it.
    #[tokio::test]
    async fn the_per_run_child_ceiling_bounds_delegations_across_rounds() {
        let over = crate::subagent::MAX_TOTAL_CHILDREN + 1;
        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        let session_channel = spawn_session_channel(rec.clone()).await;

        // One delegation per round, each answered in a single child round, for
        // one round more than the ceiling allows. The trailing answer is the
        // parent's own, after the refusal it reads as a tool error.
        let mut script = Vec::new();
        for index in 0..over {
            script.push(Scripted::ToolCalls {
                content: String::new(),
                calls: vec![subagent_call(
                    &format!("sub-{index}"),
                    &format!(r#"{{"goal":"lookup {index}","max_rounds":1}}"#),
                )],
            });
            script.push(Scripted::Answer(format!("finding {index}")));
        }
        script.push(Scripted::Answer("Collected the findings.".to_owned()));
        let inference_channel = spawn_inference_channel(script).await;
        let state = crate::state::StateStore::new();

        let mut req = sample_request();
        // Far above what the delegations together can spend, so a refusal here
        // cannot be the pool talking.
        req.max_rounds = 64;
        let resp = run_agent(
            &state,
            session_channel,
            inference_channel,
            req,
            None,
            Some("session-token".to_owned()),
            "inference-token".to_owned(),
            &ALLOW_CAPABILITY_POLICY,
            &STATIC_TERMINAL_TOKENS,
            None,
            None,
            &test_sandbox_manager_client(),
            "test-backend",
            None,
            &test_sandbox_tokens(),
        )
        .await
        .expect("a refused delegation must not fail the run");
        assert_eq!(resp.status, "completed");

        let r = rec.lock().unwrap();
        assert_eq!(
            u32::try_from(r.started_child_runs.len()).unwrap(),
            crate::subagent::MAX_TOTAL_CHILDREN,
            "the ceiling must cap the DURABLE child runs, not merely the answers"
        );

        // The refusal the model reads must name the ceiling. Blaming the round
        // budget would be false — it had 60+ rounds left — and would send the
        // model to re-plan the wrong constraint.
        let refusals: Vec<String> = (0..over)
            .map(|index| persisted_step(&r, &format!("tool_{}_sub-{index}", index + 1)).1)
            .filter(|error| !error.is_empty())
            .collect();
        assert_eq!(
            refusals.len(),
            1,
            "exactly one delegation is refused, and only the last: {refusals:?}"
        );
        assert!(
            refusals[0].contains("per-run limit"),
            "the refusal must name the ceiling: {}",
            refusals[0]
        );
        assert!(
            !refusals[0].contains("no round budget left"),
            "blaming the round budget would be false — it had plenty: {}",
            refusals[0]
        );
    }

    #[tokio::test]
    async fn two_concurrent_delegations_in_one_round_share_the_same_budget_pool() {
        // Same setup as `subagent_budget_cannot_exceed_the_parents_remaining_rounds`
        // (parent has exactly 1 round left to lend) but with TWO greedy
        // delegations requested in the SAME round, dispatched concurrently.
        // Before the shared-pool fix, each `LoopSubagentDispatch` would have
        // captured its OWN full 1-round snapshot and could independently spend
        // it, letting the pair together consume up to 2 delegated rounds from
        // a run that only had 1 left. With the shared pool, the second claim
        // observes what the first already spent.
        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        let session_channel = spawn_session_channel(rec.clone()).await;
        let observed_messages: ObservedMessages = Arc::new(Mutex::new(Vec::new()));
        let inference_channel = spawn_inference_channel_with_messages(
            vec![Scripted::ToolCalls {
                content: String::new(),
                calls: vec![
                    subagent_call("sub-a", r#"{"goal":"boil the ocean","max_rounds":99}"#),
                    subagent_call("sub-b", r#"{"goal":"count the stars","max_rounds":99}"#),
                ],
            }],
            observed_messages.clone(),
        )
        .await;
        let state = crate::state::StateStore::new();

        let mut req = sample_request();
        req.max_rounds = 2;
        let resp = run_agent(
            &state,
            session_channel,
            inference_channel,
            req,
            None,
            Some("session-token".to_owned()),
            "inference-token".to_owned(),
            &ALLOW_CAPABILITY_POLICY,
            &STATIC_TERMINAL_TOKENS,
            None,
            None,
            &test_sandbox_manager_client(),
            "test-backend",
            None,
            &test_sandbox_tokens(),
        )
        .await
        .expect("an exhausted run is still finalized");

        assert_eq!(
            resp.rounds_executed, 2,
            "1 parent round + at most 1 delegated round shared by the pair -- \
             not 3, which is what a per-call snapshot (each sibling seeing the \
             full 1-round remainder independently) would have produced"
        );

        let r = rec.lock().unwrap();
        let (_, error_a) = persisted_step(&r, "tool_1_sub-a");
        let (_, error_b) = persisted_step(&r, "tool_2_sub-b");
        // Exactly one of the pair could have actually claimed the run's single
        // remaining round; the other must see the pool already spent (refused
        // outright, never getting to run at all) regardless of what the first
        // one then did with its claim. Which one wins is a dispatch-order
        // detail, not a contract this test pins.
        let refused = [&error_a, &error_b]
            .into_iter()
            .filter(|error| error.contains("no round budget left to lend"))
            .count();
        assert_eq!(
            refused, 1,
            "exactly one sibling must be refused outright for lack of shared \
             budget -- 0 would mean the pool did not cap them at all, and 2 \
             would mean neither could claim it: sub-a={error_a:?} sub-b={error_b:?}"
        );
    }

    #[tokio::test]
    async fn a_tool_call_after_the_first_pause_is_still_recorded_not_silently_dropped() {
        // `delete_records` (index 0) pauses for approval; `yr_weather` (index 1,
        // offered alongside it) comes AFTER the pausing call in the model's own
        // array order. Concurrent dispatch means it still genuinely runs (it
        // never needed approval), and Phase 3 must still write its audit step
        // even though the round as a whole returns `awaiting_approval` --
        // a call that really executed must never go missing from the run's own
        // record just because a sibling elsewhere in the batch needed a human.
        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        let session_channel = spawn_session_channel(rec.clone()).await;
        let inference_channel = spawn_inference_channel(vec![Scripted::ToolCalls {
            content: String::new(),
            calls: vec![
                pb::ToolCall {
                    id: "del-1".to_owned(),
                    name: "delete_records".to_owned(),
                    arguments_json: "{}".to_owned(),
                },
                failing_tool_call("wx-1"),
            ],
        }])
        .await;
        let state = crate::state::StateStore::new();

        let gated_tools = vec![
            pb::ToolDefinition {
                name: "delete_records".to_owned(),
                description: "Delete records (destructive).".to_owned(),
                parameters_json: r#"{"type":"object","properties":{}}"#.to_owned(),
            },
            pb::ToolDefinition {
                name: "yr_weather".to_owned(),
                description: "Weather lookup.".to_owned(),
                parameters_json: r#"{"type":"object","properties":{}}"#.to_owned(),
            },
        ];

        let mut req = sample_request();
        req.mode = "ask".to_owned();
        let resp = run_agent_with_tools(
            &state,
            session_channel,
            inference_channel,
            req,
            gated_tools,
            None,
            Some("session-token".to_owned()),
            "inference-token".to_owned(),
            &ALLOW_CAPABILITY_POLICY,
            &STATIC_TERMINAL_TOKENS,
            None,
            None,
            &test_sandbox_manager_client(),
            "test-backend",
            None,
            &test_sandbox_tokens(),
        )
        .await
        .expect("durable approval should pause the run");

        assert_eq!(resp.status, "awaiting_approval");

        let r = rec.lock().unwrap();
        assert_eq!(
            r.approvals.len(),
            1,
            "exactly one durable approval, for del-1"
        );
        assert_eq!(r.approvals[0].0, "tool_1_del-1");
        // The genuinely-executed sibling still has its own audit step, despite
        // sitting after the pausing call in the model's array order.
        assert_eq!(
            r.completed,
            vec![("tool_2_wx-1".to_owned(), "running".to_owned())],
            "wx-1 actually ran (concurrently) and must not vanish from the audit trail"
        );
    }

    #[tokio::test]
    async fn a_second_approval_needing_call_in_the_same_batch_does_not_get_a_second_durable_approval(
    ) {
        // Two independently gated calls in one round. Only the first (by array
        // order) may win the durable, idempotency-keyed approval; a second
        // approval write in the same round would be a duplicate the model
        // never asked to reconcile, and the run can only be `AwaitingApproval`
        // for one thing at a time.
        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        let session_channel = spawn_session_channel(rec.clone()).await;
        let inference_channel = spawn_inference_channel(vec![Scripted::ToolCalls {
            content: String::new(),
            calls: vec![
                pb::ToolCall {
                    id: "del-1".to_owned(),
                    name: "delete_records".to_owned(),
                    arguments_json: "{}".to_owned(),
                },
                pb::ToolCall {
                    id: "del-2".to_owned(),
                    name: "delete_records".to_owned(),
                    arguments_json: "{}".to_owned(),
                },
            ],
        }])
        .await;
        let state = crate::state::StateStore::new();

        let gated_tools = vec![pb::ToolDefinition {
            name: "delete_records".to_owned(),
            description: "Delete records (destructive).".to_owned(),
            parameters_json: r#"{"type":"object","properties":{}}"#.to_owned(),
        }];

        let mut req = sample_request();
        req.mode = "ask".to_owned();
        let resp = run_agent_with_tools(
            &state,
            session_channel,
            inference_channel,
            req,
            gated_tools,
            None,
            Some("session-token".to_owned()),
            "inference-token".to_owned(),
            &ALLOW_CAPABILITY_POLICY,
            &STATIC_TERMINAL_TOKENS,
            None,
            None,
            &test_sandbox_manager_client(),
            "test-backend",
            None,
            &test_sandbox_tokens(),
        )
        .await
        .expect("durable approval should pause the run");

        assert_eq!(resp.status, "awaiting_approval");

        let r = rec.lock().unwrap();
        assert_eq!(
            r.approvals.len(),
            1,
            "del-2 also needed approval, but only del-1 (first by array order) gets \
             the durable write; del-2 can be requested again once the run resumes"
        );
        assert_eq!(r.approvals[0].0, "tool_1_del-1");
    }
}
