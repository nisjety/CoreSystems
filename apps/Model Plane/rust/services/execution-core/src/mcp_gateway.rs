//! Bridge from the governed agent loop to model-gateway's MCP registry.
//!
//! execution-core has no MCP registry of its own — per the capability-ownership
//! matrix §G2 the **gateway** owns MCP server registration, discovery, and the
//! tool proxy. To let the governed loop OFFER an org's registered MCP tools to
//! the model and ROUTE the calls it makes, exec-core dials the gateway's
//! `ModelGateway` gRPC: `ListMcpTools` for the agent-facing tool defs (namespaced
//! `mcp__<server_id>__<tool>`), and the existing `ProxyMcpTool` to execute one.
//!
//! Both directions are **best-effort**: a missing or unreachable gateway yields
//! no MCP tools, and the agent still runs with its built-in toolset. The gateway
//! shares `model-plane-network` with exec-core, so the default address resolves
//! in-cluster without extra wiring.
//!
//! # Two paths
//!
//! With `MCP_DO_TOOLS=off` (the default) the calls below the plain client are
//! exactly what they were: unauthenticated, staged, and unbound, so a gateway that
//! demands a credential offers nothing and a call is refused at the gate.
//!
//! With `MCP_DO_TOOLS=read` or `all` the run goes through [`GovernedMcp`]: the
//! gateway is called as the execution service (its own credential, never the
//! person's), the listing is unstaged and carries each tool's digest, and a tool is
//! offered only when the registry approved it ([`plan_offer`]). The registry is
//! asked once per run segment, for every tool the run is offered, so the same
//! answer feeds the offer, the gate's class lookups and the digest the gateway
//! re-checks before it runs a call.

use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use mp_contracts::model_plane::v1 as pb;
use mp_contracts::model_plane::v1::capability_core_client::CapabilityCoreClient;
use mp_contracts::model_plane::v1::mcp_elicitation_service_client::McpElicitationServiceClient;
use mp_contracts::model_plane::v1::model_gateway_client::ModelGatewayClient;
use mp_contracts::tool_class::SideEffectClass;
use mp_contracts::user_question;
use tonic::transport::Channel;
use tonic::{Request, Status};

use crate::capability_policy::ServiceTokenProvider;
use crate::permission::ToolClassView;

/// In-network model-gateway gRPC address (shared `model-plane-network`).
const DEFAULT_ADDR: &str = "http://model-gateway:9090";

/// In-network capability-core gRPC address, as `execution-core`'s policy client uses.
const DEFAULT_CAPABILITY_ADDR: &str = "http://capability-core:9097";

/// Lazily-connected client to the gateway's MCP surface.
#[derive(Clone)]
pub struct McpGatewayClient {
    channel: Channel,
}

impl McpGatewayClient {
    /// Build a lazily-connected client. Reads `MODEL_GATEWAY_ADDR` (or
    /// `MODEL_GATEWAY_GRPC_URL`), defaulting to the in-network gateway.
    /// `connect_lazy` never dials here; a dead gateway surfaces at call time and
    /// is handled best-effort. Returns `None` only if the address is unparseable.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let addr = std::env::var("MODEL_GATEWAY_ADDR")
            .or_else(|_| std::env::var("MODEL_GATEWAY_GRPC_URL"))
            .unwrap_or_else(|_| DEFAULT_ADDR.to_owned());
        let channel = tonic::transport::Endpoint::from_shared(addr)
            .ok()?
            .connect_timeout(std::time::Duration::from_secs(2))
            .timeout(std::time::Duration::from_secs(8))
            .connect_lazy();
        Some(Self { channel })
    }

    /// Agent-facing MCP tool defs for `(org_id, user_id)`, namespaced
    /// `mcp__<server>__<tool>` and filtered to what this user may USE (org-wide,
    /// owned, or shared-to-them — never another user's private tools).
    /// Best-effort: any transport/RPC error yields an empty list (offer nothing).
    pub async fn list_tools(&self, org_id: &str, user_id: &str) -> Vec<pb::ToolDefinition> {
        let mut client = ModelGatewayClient::new(self.channel.clone());
        match client
            .list_mcp_tools(pb::ListMcpToolsRequest {
                request_id: String::new(),
                org_id: org_id.to_owned(),
                user_id: user_id.to_owned(),
                unstaged: false,
            })
            .await
        {
            Ok(resp) => resp.into_inner().tools,
            Err(e) => {
                tracing::debug!(error = %e, "mcp list_tools failed; offering no MCP tools");
                Vec::new()
            }
        }
    }

    /// Proxy a single MCP tool call through the gateway. Returns the tool's
    /// output JSON, or an error string the model can see and react to.
    ///
    /// # Errors
    /// Returns `Err` if the gateway gRPC call fails (transport) or the remote
    /// MCP server reports an error (`error_message` non-empty).
    pub async fn proxy_tool(
        &self,
        org_id: &str,
        user_id: &str,
        server_id: &str,
        tool_name: &str,
        input_json: &str,
    ) -> Result<String, String> {
        // The plain path never allows a server's form, so a form here is a gateway fault.
        match self
            .proxy(Request::new(proxy_request(
                org_id, user_id, server_id, tool_name, input_json, "", false,
            )))
            .await?
        {
            McpProxyOutcome::Output(output) => Ok(output),
            McpProxyOutcome::Elicitation(_) => Err(UNASKED_FORM.to_owned()),
        }
    }

    async fn proxy(
        &self,
        request: Request<pb::ProxyMcpToolRequest>,
    ) -> Result<McpProxyOutcome, String> {
        let mut client = ModelGatewayClient::new(self.channel.clone());
        let resp = client
            .proxy_mcp_tool(request)
            .await
            .map_err(|e| format!("mcp proxy transport: {e}"))?
            .into_inner();
        proxy_outcome(resp)
    }
}

/// What a proxied MCP call came back with.
#[derive(Debug, Clone, PartialEq)]
pub enum McpProxyOutcome {
    /// The call finished; the tool's output JSON.
    Output(String),
    /// The server asked the run's owner a question and the gateway holds the call until it is
    /// answered, declined or cancelled (EP-4 S4). Only a call that allowed elicitation gets one.
    Elicitation(pb::McpElicitationPending),
}

/// The refusal of a form the call did not allow. The gateway sets one only when the request
/// allowed it, so this reads a gateway fault, and the held call is cancelled.
pub const UNASKED_FORM: &str =
    "the MCP server asked a question this call did not allow; the call was cancelled";

/// Read a proxy answer: a pending form first (its output and error are empty by contract), then
/// the server's error, then the output.
fn proxy_outcome(resp: pb::ProxyMcpToolResponse) -> Result<McpProxyOutcome, String> {
    if let Some(pending) = resp.elicitation {
        return Ok(McpProxyOutcome::Elicitation(pending));
    }
    if resp.error_message.is_empty() {
        Ok(McpProxyOutcome::Output(resp.output_json))
    } else {
        Err(resp.error_message)
    }
}

fn proxy_request(
    org_id: &str,
    user_id: &str,
    server_id: &str,
    tool_name: &str,
    input_json: &str,
    expected_digest: &str,
    allow_elicitation: bool,
) -> pb::ProxyMcpToolRequest {
    pb::ProxyMcpToolRequest {
        request_id: String::new(),
        org_id: org_id.to_owned(),
        server_id: server_id.to_owned(),
        tool_name: tool_name.to_owned(),
        input_json: input_json.to_owned(),
        // Re-authorized per-user gateway-side (defense in depth).
        user_id: user_id.to_owned(),
        expected_digest: expected_digest.to_owned(),
        // EP-4 S7: true only for a run [`allow_elicitation`] admits; false keeps today's wire.
        allow_elicitation,
    }
}

// --- MCP elicitation in Do runs (EP-4 S7) ------------------------------------

/// The switch that lets a governed run's MCP calls carry a server's form to the run's owner
/// (`MCP_DO_ELICITATION`). Read strictly: only the literal `true` enables it, and the default is
/// off. Off, every `ProxyMcpToolRequest.allow_elicitation` is false and the wire is what it was.
/// It needs the gateway's `MCP_ELICITATION_ENABLED` too; either one off means no form is asked.
pub const ELICITATION_SWITCH_ENV: &str = "MCP_DO_ELICITATION";

/// Only the literal `true` enables the switch, as every switch of this service reads it.
#[must_use]
pub fn elicitation_switch_on(raw: Option<&str>) -> bool {
    raw == Some("true")
}

/// Whether the deployment lets a run carry a server's form, read once per process so a run
/// cannot see it change.
#[must_use]
pub fn elicitation_enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        elicitation_switch_on(std::env::var(ELICITATION_SWITCH_ENV).ok().as_deref())
    })
}

/// The facts of a run that decide whether its MCP calls may carry a server's form (D9).
#[derive(Debug, Clone, Copy, Default)]
pub struct ElicitationRun {
    /// 0 for the root loop; a delegated subagent has no owner of its own to ask.
    pub depth: u32,
    pub zdr: bool,
    /// The run's route is waived (DEC-32), which keeps nothing, exactly like ZDR.
    pub waived: bool,
    /// The run can pause and resume (WS03 S8-A): a form holds the run until its owner answers.
    pub parkable: bool,
    /// A Space run has more than one person who could answer; W3 asks only a personal run's owner.
    pub space_scoped: bool,
    /// Questions the run has put to its owner so far, its model's and servers' alike.
    pub questions_asked: u32,
}

/// Whether this run's MCP calls may allow the server to ask its owner a question.
#[must_use]
pub fn allow_elicitation(run: &ElicitationRun) -> bool {
    allow_elicitation_when(elicitation_enabled(), run)
}

/// [`allow_elicitation`] with the switch given, so the rule is testable without the environment.
/// A form becomes a question, so it shares the run's question cap with `ask_user`; session-core
/// enforces the same cap when the question is stored.
#[must_use]
pub fn allow_elicitation_when(on: bool, run: &ElicitationRun) -> bool {
    on && run.depth == 0
        && !run.zdr
        && !run.waived
        && run.parkable
        && !run.space_scoped
        && usize::try_from(run.questions_asked)
            .is_ok_and(|asked| asked < user_question::MAX_QUESTIONS_PER_RUN)
}

/// `AnswerMcpElicitationRequest.action` for an answered form.
pub const ELICITATION_ACCEPT: &str = "accept";
/// The owner dismissed the form without an answer.
pub const ELICITATION_DECLINE: &str = "decline";
/// No answer will come: the question expired, or the run ended or was cancelled.
pub const ELICITATION_CANCEL: &str = "cancel";

/// The gateway's typed error for an answer that reached a replica without the held call (D16).
pub const ELICITATION_EXPIRED: &str = "mcp_elicitation_expired";

/// What goes back to the server for one form, read from its question.
#[derive(Debug, Clone, PartialEq)]
pub struct ElicitationReply {
    pub action: &'static str,
    /// The owner's answers; empty unless `action` is accept.
    pub answers: Vec<pb::QuestionAnswerItem>,
}

/// The reply a question's state stands for: an answer is accepted with its items, a skip is a
/// decline, and anything else (expired, cancelled, or answered with nothing) cancels the call, so
/// the server never reads silence as consent.
#[must_use]
pub fn elicitation_reply(question: &pb::RunQuestion) -> ElicitationReply {
    match question.state() {
        pb::QuestionState::Answered if !question.answers.is_empty() => ElicitationReply {
            action: ELICITATION_ACCEPT,
            answers: question.answers.clone(),
        },
        pb::QuestionState::Skipped => ElicitationReply {
            action: ELICITATION_DECLINE,
            answers: Vec::new(),
        },
        _ => ElicitationReply {
            action: ELICITATION_CANCEL,
            answers: Vec::new(),
        },
    }
}

/// The question a pending form is put to the owner as: a `resume` question that names the
/// server, carries the gateway's mapped items, lives no longer than the gateway holds the call,
/// and is keyed `{run_id}:{step_id}:elicit:<ordinal>` so a retry of the same step returns the
/// same question. `None` when the hold is already over or the form carries nothing to ask: the
/// call is then cancelled instead of asked.
#[must_use]
pub fn elicitation_question(
    run_id: &str,
    step_id: &str,
    org_id: &str,
    pending: &pb::McpElicitationPending,
    ordinal: u32,
    now_unix: i64,
) -> Option<pb::CreateRunQuestionRequest> {
    let hold_left = pending.expires_at_unix.saturating_sub(now_unix);
    if hold_left <= 0 || pending.items.is_empty() {
        return None;
    }
    let expires_in_seconds = u32::try_from(hold_left)
        .unwrap_or(u32::MAX)
        .min(user_question::RESUME_TTL_SECS);
    Some(pb::CreateRunQuestionRequest {
        run_id: run_id.to_owned(),
        step_id: step_id.to_owned(),
        org_id: org_id.to_owned(),
        delivery: user_question::DELIVERY_RESUME.to_owned(),
        items: pending.items.clone(),
        expires_in_seconds,
        idempotency_key: format!("{run_id}:{step_id}:elicit:{ordinal}"),
        origin: user_question::ORIGIN_MCP_ELICITATION.to_owned(),
        origin_label: elicitation_origin_label(pending),
    })
}

/// The server's display name as a question's origin label: control characters dropped, cut at
/// [`user_question::MAX_ORIGIN_LABEL`] characters, and the server id when the name is empty, so
/// the label session-core requires is never blank.
#[must_use]
pub fn elicitation_origin_label(pending: &pb::McpElicitationPending) -> String {
    let clean = |text: &str| -> String {
        text.chars()
            .filter(|ch| !ch.is_control())
            .take(user_question::MAX_ORIGIN_LABEL)
            .collect::<String>()
            .trim()
            .to_owned()
    };
    let label = clean(&pending.server_label);
    if label.is_empty() {
        clean(&pending.server_id)
    } else {
        label
    }
}

/// What the model reads while the run waits for the owner's answer to a server's form.
#[must_use]
pub fn elicitation_waiting_text(origin_label: &str) -> String {
    format!("Venter på svar fra eieren på et skjema fra {origin_label}.")
}

fn answer_request(
    org_id: &str,
    user_id: &str,
    elicitation_id: &str,
    reply: ElicitationReply,
) -> pb::AnswerMcpElicitationRequest {
    pb::AnswerMcpElicitationRequest {
        elicitation_id: elicitation_id.to_owned(),
        org_id: org_id.to_owned(),
        // The gateway answers only for the user the call was held for: the run's owner.
        user_id: user_id.to_owned(),
        action: reply.action.to_owned(),
        answers: reply.answers,
    }
}

/// Read an answer's outcome. A call the gateway no longer holds becomes the typed
/// [`ELICITATION_EXPIRED`] error the model reads: the call did not complete, and nothing says it
/// ran.
fn answer_outcome(resp: pb::AnswerMcpElicitationResponse) -> Result<McpProxyOutcome, String> {
    if resp.error_message.starts_with(ELICITATION_EXPIRED) {
        return Err(format!(
            "{ELICITATION_EXPIRED}: the MCP server's form is no longer waiting for an answer, so \
             the call did not complete"
        ));
    }
    proxy_outcome(pb::ProxyMcpToolResponse {
        output_json: resp.output_json,
        error_message: resp.error_message,
        elicitation: resp.elicitation,
        // An answer carries no request id, and the outcome does not read one.
        ..Default::default()
    })
}

/// Split `mcp__<server_id>__<tool>` into `(server_id, tool)`. Mirrors the
/// gateway's parse: split on the FIRST `__` after the prefix (a tool name may
/// itself contain `__`; a `server_id` ULID does not).
#[must_use]
pub fn parse_mcp_tool_name(name: &str) -> Option<(&str, &str)> {
    name.strip_prefix("mcp__")
        .and_then(|rest| rest.split_once("__"))
}

// --- MCP in Do runs (WS05-S4) ------------------------------------------------

/// How far MCP tools reach a Do run (`MCP_DO_TOOLS`).
///
/// * `Off`: nothing governed. The plain listing above is what it always was.
/// * `Read`: only tools the registry approved AND classed `read` are offered and
///   executed. Approved MCP writes dead-end today (an approval for one cannot be
///   resumed cold), so they are withheld rather than offered and abandoned.
/// * `All`: any approved class is offered. Kept off until a resumed run can run an
///   approved MCP call; the filter and the gate are the same as `Read`.
///
/// Parsed strictly: only the exact literals `read` and `all` move off `Off`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum McpDoTools {
    #[default]
    Off,
    Read,
    All,
}

impl McpDoTools {
    pub const ENV: &'static str = "MCP_DO_TOOLS";

    #[must_use]
    pub fn from_env() -> Self {
        let raw = std::env::var(Self::ENV).ok();
        let mode = Self::from_raw(raw.as_deref());
        if mode == Self::Off && raw.as_deref().is_some_and(|v| !v.is_empty() && v != "off") {
            tracing::warn!(
                "{} is not one of off, read or all; MCP stays off for Do runs",
                Self::ENV
            );
        }
        mode
    }

    fn from_raw(raw: Option<&str>) -> Self {
        match raw {
            Some("read") => Self::Read,
            Some("all") => Self::All,
            _ => Self::Off,
        }
    }

    /// Whether a tool of this class may be offered and executed in this mode.
    #[must_use]
    pub fn allows(self, class: SideEffectClass) -> bool {
        match self {
            Self::Off => false,
            Self::Read => class == SideEffectClass::Read,
            Self::All => true,
        }
    }
}

/// Most MCP tools one run is offered when `MCP_DO_TOOL_CAP` does not say.
pub const DEFAULT_TOOL_CAP: usize = 40;
const MAX_TOOL_CAP: usize = 256;

/// `MCP_DO_TOOL_CAP`: a positive count up to what one registry lookup carries. A
/// value that is not one is the default, never zero (which would be "off" by a
/// different name).
#[must_use]
pub fn tool_cap_from_env() -> usize {
    cap_from_raw(std::env::var("MCP_DO_TOOL_CAP").ok().as_deref())
}

fn cap_from_raw(raw: Option<&str>) -> usize {
    raw.and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|cap| (1..=MAX_TOOL_CAP).contains(cap))
        .unwrap_or(DEFAULT_TOOL_CAP)
}

/// The unstaged listing the gateway returns for a governed run: every tool, and the
/// digest and declared side effect of each.
#[derive(Debug, Default, Clone)]
pub struct McpListing {
    pub tools: Vec<pb::ToolDefinition>,
    pub meta: Vec<pb::McpToolMeta>,
}

/// Why a listed tool was not offered. Each is counted in
/// `mcp_tools_withheld_total{reason}` so a tool that quietly never appears has a
/// number to explain it.
pub mod withheld {
    pub const GATEWAY_UNAVAILABLE: &str = "gateway_unavailable";
    pub const REGISTRY_UNAVAILABLE: &str = "registry_unavailable";
    pub const NO_META: &str = "no_meta";
    pub const UNBOUND: &str = "unbound";
    pub const NOT_MCP_BINDING: &str = "not_mcp_binding";
    pub const NOT_APPROVED: &str = "not_approved";
    pub const DIGEST_MISMATCH: &str = "digest_mismatch";
    pub const POLICY_DENY: &str = "policy_deny";
    pub const CLASS_NOT_ALLOWED: &str = "class_not_allowed";
    pub const OVER_CAP: &str = "over_cap";
    pub const ZDR: &str = "zdr";
    pub const GATE_NOT_ENFORCING: &str = "gate_not_enforcing";
}

/// What a governed run is offered.
#[derive(Debug, Default)]
pub struct McpOffer {
    /// The tool definitions the model sees, sorted by qualified name (the tool list
    /// is part of the cached prefix).
    pub tools: Vec<pb::ToolDefinition>,
    /// The registry's declared class of each offered tool, for the gate.
    pub views: Vec<(String, ToolClassView)>,
    /// How many listed tools were withheld, by reason.
    pub withheld: BTreeMap<&'static str, usize>,
}

/// Capability ids the registry derives for MCP tool rows; a binding under any other
/// id is not an MCP tool's.
const MCP_CAPABILITY_PREFIX: &str = "cap.mcp_tool.";

/// Decide which listed tools a governed run is offered. Pure.
///
/// A tool is offered only when every one of these holds, so a cold or stale
/// gateway cache fails to offer instead of failing to authorise:
///
/// * the gateway reported its digest, and the registry holds a binding for it;
/// * the binding is an MCP tool's, its review state is `approved`, and the digest
///   the registry pinned equals the digest the gateway just observed (a changed
///   definition is not offered, and would be refused if it were called);
/// * the registry's effective policy is not `deny`;
/// * the class the registry declared is one the mode allows.
///
/// What passes is sorted by qualified name and cut at `cap`; the rest is counted.
#[must_use]
pub fn plan_offer(
    mode: McpDoTools,
    cap: usize,
    listing: McpListing,
    bindings: &[pb::ToolBinding],
) -> McpOffer {
    let meta: HashMap<&str, &pb::McpToolMeta> = listing
        .meta
        .iter()
        .map(|meta| (meta.qualified_name.as_str(), meta))
        .collect();
    let bound: HashMap<&str, &pb::ToolBinding> = bindings
        .iter()
        .map(|binding| (binding.tool_name.as_str(), binding))
        .collect();
    let mut offer = McpOffer::default();
    let mut withhold = |reason: &'static str| {
        *offer.withheld.entry(reason).or_default() += 1;
    };
    let mut admitted: Vec<(pb::ToolDefinition, ToolClassView)> = Vec::new();
    for tool in listing.tools {
        let Some(meta) = meta.get(tool.name.as_str()) else {
            withhold(withheld::NO_META);
            continue;
        };
        let Some(binding) = bound.get(tool.name.as_str()) else {
            withhold(withheld::UNBOUND);
            continue;
        };
        if !binding.capability_id.starts_with(MCP_CAPABILITY_PREFIX) {
            withhold(withheld::NOT_MCP_BINDING);
            continue;
        }
        if binding.review_state != "approved" {
            withhold(withheld::NOT_APPROVED);
            continue;
        }
        if binding.tool_digest.is_empty() || binding.tool_digest != meta.digest {
            withhold(withheld::DIGEST_MISMATCH);
            continue;
        }
        if binding.effective_policy == "deny" {
            withhold(withheld::POLICY_DENY);
            continue;
        }
        let view = ToolClassView::from_registry(
            &tool.name,
            &binding.side_effect_class,
            &binding.approval_floor,
            &binding.tool_digest,
        );
        if !mode.allows(view.class()) {
            withhold(withheld::CLASS_NOT_ALLOWED);
            continue;
        }
        admitted.push((tool, view));
    }
    admitted.sort_by(|left, right| left.0.name.cmp(&right.0.name));
    admitted.dedup_by(|later, earlier| later.0.name == earlier.0.name);
    if admitted.len() > cap {
        *offer.withheld.entry(withheld::OVER_CAP).or_default() += admitted.len() - cap;
        admitted.truncate(cap);
    }
    for (tool, view) in admitted {
        offer.views.push((tool.name.clone(), view));
        offer.tools.push(tool);
    }
    offer
}

/// The class and approval floor the signed per-call policy decision carried for one
/// call, as capability-core worded them. Empty when the decision does not say.
///
/// The offer's [`ToolClassView`] is a snapshot from when the segment's tools were
/// listed. A registry row can be tightened after that without its digest changing, so
/// the view the gate and the mode check read can be looser than what capability-core
/// just decided on. The two are compared at dispatch and a difference refuses the call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SignedToolClass {
    pub class: String,
    pub floor: String,
}

/// The typed refusal prefix of a call whose class changed under the run, so a log
/// search or a client can key on it.
pub const CLASS_CHANGED: &str = "class_changed";

impl SignedToolClass {
    #[must_use]
    pub fn new(class: &str, floor: &str) -> Self {
        Self {
            class: class.to_owned(),
            floor: floor.to_owned(),
        }
    }

    /// Whether the decision's class or floor is anything other than what `view`
    /// holds. A decision that names no class cannot confirm the view, so it counts as
    /// a difference: the call fails closed rather than running on a stale snapshot.
    #[must_use]
    pub fn differs_from(&self, tool_name: &str, view: &ToolClassView) -> bool {
        if self.class.trim().is_empty() {
            return true;
        }
        let signed = ToolClassView::from_registry(tool_name, &self.class, &self.floor, "");
        signed.class() != view.class() || signed.floor() != view.floor()
    }
}

/// Count what a governed offer withheld.
pub fn count_withheld(withheld: &BTreeMap<&'static str, usize>) {
    for (reason, count) in withheld {
        metrics::counter!("mcp_tools_withheld_total", "reason" => *reason)
            .increment(u64::try_from(*count).unwrap_or(u64::MAX));
    }
}

/// Where a governed run reads the gateway's listing and the registry's bindings
/// from: [`GovernedMcp`] in production, a fake in a test.
#[tonic::async_trait]
pub trait GovernedSource: Send + Sync {
    async fn listing(&self, org_id: &str, user_id: &str) -> Result<McpListing, Status>;

    async fn bindings(
        &self,
        run_id: &str,
        org_id: &str,
        user_id: &str,
        names: &[String],
    ) -> Result<Vec<pb::ToolBinding>, Status>;
}

/// What the registry and the gateway said about one run's offer.
#[derive(Debug, Default)]
pub struct RunOffer {
    /// The MCP tools the model is offered, and why the others were not.
    pub offer: McpOffer,
    /// The declared class of each built-in tool the run is offered, for the gate.
    pub builtin_views: Vec<(String, ToolClassView)>,
    /// The registry answered. When it did not, nothing here may replace what the
    /// run already knew: an outage is not "nothing is classified".
    pub answered: bool,
}

/// Most names one registry lookup carries (the registry refuses more).
const MAX_BINDING_NAMES: usize = 256;

/// The most MCP tools one run can be offered: what the operator asked for, but never
/// more than the run's view store keeps once the built-in tools' views are in it. A
/// tool offered without a view is a tool every call of which is refused ("not bound for
/// this run"), so it is not offered. Also bounded by one registry lookup, which carries
/// the built-in names as well.
#[must_use]
pub fn effective_cap(cap: usize, builtin_count: usize) -> usize {
    cap.min(crate::permission::MAX_VIEWS_PER_RUN.saturating_sub(builtin_count))
        .min(MAX_BINDING_NAMES.saturating_sub(builtin_count))
}

/// Resolve one run's offer: list the org's MCP tools when `mode` is on, ask the
/// registry once for them and for the built-in tools in `builtin_names`, and decide
/// what is offered ([`plan_offer`]). Never errors: a gateway or registry that cannot
/// answer means no MCP tool is offered and no class is learned, each counted.
///
/// The built-in names are always in the lookup and always keep a view; it is the MCP
/// tools that are bounded ([`effective_cap`]), and what does not fit is counted as
/// `over_cap`, never offered.
pub async fn resolve_run_offer(
    source: &dyn GovernedSource,
    mode: McpDoTools,
    cap: usize,
    run_id: &str,
    org_id: &str,
    user_id: &str,
    builtin_names: &[String],
) -> RunOffer {
    let mut resolved = RunOffer::default();
    let mut listing = if mode == McpDoTools::Off {
        McpListing::default()
    } else {
        match source.listing(org_id, user_id).await {
            Ok(listing) => listing,
            Err(error) => {
                tracing::warn!(code = ?error.code(), "governed MCP listing unavailable; offering no MCP tools");
                resolved
                    .offer
                    .withheld
                    .insert(withheld::GATEWAY_UNAVAILABLE, 1);
                McpListing::default()
            }
        }
    };
    // The built-in names come first and are never the ones cut: they sort after `mcp__`
    // (`save_memory`, `shell`, `web_fetch`), so cutting the sorted union would drop exactly
    // them once an org lists a few hundred MCP tools. It is the MCP names that are bounded,
    // and a listed tool the lookup has no room for is withheld, not offered unbound.
    let mut builtin: Vec<String> = builtin_names.to_vec();
    builtin.sort();
    builtin.dedup();
    builtin.truncate(MAX_BINDING_NAMES);
    let cap = effective_cap(cap, builtin.len());
    let mut mcp_names: Vec<String> = listing
        .tools
        .iter()
        .map(|tool| tool.name.clone())
        .filter(|name| name.starts_with("mcp__"))
        .collect();
    mcp_names.sort();
    mcp_names.dedup();
    let mut over_lookup = 0_usize;
    let room = MAX_BINDING_NAMES - builtin.len();
    if mcp_names.len() > room {
        let unlooked: std::collections::HashSet<String> =
            mcp_names.split_off(room).into_iter().collect();
        let listed = listing.tools.len();
        listing.tools.retain(|tool| !unlooked.contains(&tool.name));
        over_lookup = listed - listing.tools.len();
    }
    let mut names = builtin;
    names.extend(mcp_names);
    if names.is_empty() {
        resolved.answered = true;
        return resolved;
    }
    let bindings = match source.bindings(run_id, org_id, user_id, &names).await {
        Ok(bindings) => bindings,
        Err(error) => {
            tracing::warn!(code = ?error.code(), "tool bindings unavailable; offering no MCP tools and learning no classes");
            resolved
                .offer
                .withheld
                .insert(withheld::REGISTRY_UNAVAILABLE, listing.tools.len().max(1));
            return resolved;
        }
    };
    resolved.answered = true;
    resolved.builtin_views = bindings
        .iter()
        .filter(|binding| builtin_names.contains(&binding.tool_name))
        .map(|binding| {
            (
                binding.tool_name.clone(),
                ToolClassView::from_registry(
                    &binding.tool_name,
                    &binding.side_effect_class,
                    &binding.approval_floor,
                    &binding.tool_digest,
                ),
            )
        })
        .collect();
    let gateway_failed = resolved
        .offer
        .withheld
        .remove(withheld::GATEWAY_UNAVAILABLE);
    resolved.offer = plan_offer(mode, cap, listing, &bindings);
    if over_lookup > 0 {
        *resolved
            .offer
            .withheld
            .entry(withheld::OVER_CAP)
            .or_default() += over_lookup;
    }
    if let Some(count) = gateway_failed {
        resolved
            .offer
            .withheld
            .insert(withheld::GATEWAY_UNAVAILABLE, count);
    }
    resolved
}

// --- The execution service's own credentials ---------------------------------

const TOKEN_REFRESH_SKEW: Duration = Duration::from_secs(30);
const MAX_TOKEN_TTL_SECONDS: u64 = 3_600;
const GATEWAY_AUDIENCE: &str = "model-gateway";
/// What the gateway's RPC gate asks of a service for the two calls below.
const GATEWAY_SCOPES: [&str; 2] = ["gateway:read", "tools:execute"];
const GATEWAY_TOKEN_REASON: &str = "execution-core governed MCP tools";
const CAPABILITY_TOKEN_REASON: &str = "execution-core MCP tool bindings";

/// Auth Core's service-token route for the gateway's audience, as the execution
/// service. Deployment-owned: the credential is exchanged for a token bound to the
/// run's tenant and the two scopes above, never held by a model, a request or a
/// log. If Control's principal registry does not grant the execution service this
/// audience, Auth Core refuses and a governed run offers no MCP tool.
struct GatewayTokens {
    auth_core_url: String,
    service_id: String,
    credential: String,
    http: reqwest::Client,
    cache: tokio::sync::Mutex<HashMap<String, (String, Instant)>>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenBundle {
    token: String,
    expires_in_seconds: u64,
    audience: String,
}

impl GatewayTokens {
    fn from_env() -> Option<Self> {
        let value = |name: &str| {
            std::env::var(name)
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        };
        Some(Self {
            auth_core_url: value("AUTH_CORE_URL")?.trim_end_matches('/').to_owned(),
            service_id: value("EXECUTION_CORE_SERVICE_ID")
                .unwrap_or_else(|| "execution-core".to_owned()),
            credential: value("EXECUTION_CORE_SERVICE_API_KEY")?,
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(5))
                .build()
                .ok()?,
            cache: tokio::sync::Mutex::new(HashMap::new()),
        })
    }

    async fn token(&self, org_id: &str) -> Result<String, Status> {
        let org_id = org_id.trim();
        if org_id.is_empty() {
            return Err(Status::invalid_argument("gateway token tenant is required"));
        }
        {
            let mut cache = self.cache.lock().await;
            cache.retain(|_, (_, expires_at)| *expires_at > Instant::now() + TOKEN_REFRESH_SKEW);
            if let Some((token, _)) = cache.get(org_id) {
                return Ok(token.clone());
            }
        }
        let unavailable = |what: &str| {
            tracing::warn!(what, "gateway service credential unavailable");
            Status::unavailable("gateway credential unavailable")
        };
        let mut credential = reqwest::header::HeaderValue::from_str(&self.credential)
            .map_err(|_| unavailable("malformed"))?;
        credential.set_sensitive(true);
        let response = self
            .http
            .post(format!(
                "{}/api/model-plane/internal-token",
                self.auth_core_url
            ))
            .header("x-service-id", &self.service_id)
            .header("x-service-api-key", credential)
            .json(&serde_json::json!({
                "orgId": org_id,
                "scopes": GATEWAY_SCOPES,
                "reason": GATEWAY_TOKEN_REASON,
            }))
            .send()
            .await
            .map_err(|_| unavailable("transport"))?;
        if !response.status().is_success() {
            return Err(unavailable("refused"));
        }
        let bundle = response
            .json::<TokenBundle>()
            .await
            .map_err(|_| unavailable("unreadable"))?;
        if bundle.token.trim().is_empty()
            || bundle.audience != GATEWAY_AUDIENCE
            || !(1..=MAX_TOKEN_TTL_SECONDS).contains(&bundle.expires_in_seconds)
        {
            return Err(unavailable("invalid"));
        }
        self.cache.lock().await.insert(
            org_id.to_owned(),
            (
                bundle.token.clone(),
                Instant::now() + Duration::from_secs(bundle.expires_in_seconds),
            ),
        );
        Ok(bundle.token)
    }
}

enum Credentials {
    Service {
        gateway: Box<GatewayTokens>,
        capability: Box<ServiceTokenProvider>,
    },
    #[cfg(test)]
    Static,
}

/// The governed MCP surface: the gateway's listing and proxy, and the registry's
/// tool bindings, each called as the execution service.
pub struct GovernedMcp {
    gateway: McpGatewayClient,
    registry: Channel,
    credentials: Credentials,
}

impl GovernedMcp {
    /// The process's governed client, built once. `None` when the deployment gave
    /// this service no credential for it (a governed run then offers no MCP tool).
    #[must_use]
    pub fn shared() -> Option<&'static Self> {
        static SHARED: OnceLock<Option<GovernedMcp>> = OnceLock::new();
        SHARED
            .get_or_init(|| {
                let built = Self::from_env();
                if built.is_none() {
                    tracing::warn!(
                        "governed MCP is unavailable: the gateway address or the execution \
                         service credential is not configured"
                    );
                }
                built
            })
            .as_ref()
    }

    fn from_env() -> Option<Self> {
        let gateway = McpGatewayClient::from_env()?;
        let addr = std::env::var("CAPABILITY_CORE_ADDR")
            .unwrap_or_else(|_| DEFAULT_CAPABILITY_ADDR.to_owned());
        let registry = tonic::transport::Endpoint::from_shared(addr)
            .ok()?
            .connect_timeout(Duration::from_secs(3))
            .connect_lazy();
        Some(Self {
            gateway,
            registry,
            credentials: Credentials::Service {
                gateway: Box::new(GatewayTokens::from_env()?),
                capability: Box::new(ServiceTokenProvider::from_env().ok()?),
            },
        })
    }

    async fn gateway_bearer(&self, org_id: &str) -> Result<String, Status> {
        match &self.credentials {
            Credentials::Service { gateway, .. } => gateway.token(org_id).await,
            #[cfg(test)]
            Credentials::Static => Ok("gateway-token".to_owned()),
        }
    }

    async fn capability_bearer(&self, org_id: &str) -> Result<String, Status> {
        match &self.credentials {
            Credentials::Service { capability, .. } => capability
                .token_with_scopes(org_id, &["capability:read"], CAPABILITY_TOKEN_REASON)
                .await
                .map_err(|error| {
                    tracing::warn!(%error, "capability service credential unavailable");
                    Status::unavailable("capability credential unavailable")
                }),
            #[cfg(test)]
            Credentials::Static => Ok("capability-token".to_owned()),
        }
    }

    /// The org's MCP tools as the gateway lists them for this user: every tool, not
    /// the staged pair, with the digest of each.
    ///
    /// The capability bearer rides along so a gateway replica that has not seen the
    /// org's servers can hydrate its cache from the registry. The gateway takes it only
    /// from a service identity such as this one and only forwards it to capability-core,
    /// which verifies it; a bearer capability-core refuses leaves the gateway's cache as
    /// it was.
    ///
    /// # Errors
    ///
    /// A status when no credential could be obtained or the gateway refused.
    pub async fn listing(&self, org_id: &str, user_id: &str) -> Result<McpListing, Status> {
        let mut request = Request::new(pb::ListMcpToolsRequest {
            request_id: String::new(),
            org_id: org_id.to_owned(),
            user_id: user_id.to_owned(),
            unstaged: true,
        });
        insert_bearer(
            &mut request,
            "authorization",
            &self.gateway_bearer(org_id).await?,
        )?;
        insert_bearer(
            &mut request,
            "x-capability-authorization",
            &self.capability_bearer(org_id).await?,
        )?;
        let response = ModelGatewayClient::new(self.gateway.channel.clone())
            .list_mcp_tools(request)
            .await?
            .into_inner();
        Ok(McpListing {
            tools: response.tools,
            meta: response.meta,
        })
    }

    /// The registry's bindings for `names`: class, floor, digest, review state and
    /// effective policy. A name the registry does not hold is absent.
    ///
    /// # Errors
    ///
    /// A status when no credential could be obtained or the registry is unreachable.
    pub async fn bindings(
        &self,
        run_id: &str,
        org_id: &str,
        user_id: &str,
        names: &[String],
    ) -> Result<Vec<pb::ToolBinding>, Status> {
        let mut request = Request::new(pb::ResolveToolBindingsRequest {
            org_id: org_id.to_owned(),
            run_id: run_id.to_owned(),
            user_id: user_id.to_owned(),
            tool_names: names.to_vec(),
        });
        insert_bearer(
            &mut request,
            "authorization",
            &self.capability_bearer(org_id).await?,
        )?;
        let response = tokio::time::timeout(
            Duration::from_secs(3),
            CapabilityCoreClient::new(self.registry.clone()).resolve_tool_bindings(request),
        )
        .await
        .map_err(|_| Status::unavailable("tool binding lookup timed out"))??;
        Ok(response.into_inner().bindings)
    }

    /// Run one MCP tool through the gateway as the execution service. The gateway
    /// refuses the call when the tool's definition no longer has `expected_digest`.
    /// With `allow_elicitation` (EP-4 S7) the server may ask the run's owner a question,
    /// and the gateway answers with the held form instead of an output.
    ///
    /// # Errors
    ///
    /// An error string the model can read: no credential, a transport fault, or the
    /// remote server's own error.
    #[allow(clippy::too_many_arguments)]
    pub async fn proxy(
        &self,
        org_id: &str,
        user_id: &str,
        server_id: &str,
        tool_name: &str,
        input_json: &str,
        expected_digest: &str,
        allow_elicitation: bool,
    ) -> Result<McpProxyOutcome, String> {
        let mut request = Request::new(proxy_request(
            org_id,
            user_id,
            server_id,
            tool_name,
            input_json,
            expected_digest,
            allow_elicitation,
        ));
        let bearer = self
            .gateway_bearer(org_id)
            .await
            .map_err(|status| format!("mcp gateway credential: {}", status.message()))?;
        insert_bearer(&mut request, "authorization", &bearer)
            .map_err(|status| status.message().to_owned())?;
        self.gateway.proxy(request).await
    }

    /// Send one reply to a form the gateway holds (EP-4 S7), as the execution service and for
    /// the run's owner, and read the held call on: its output, the server's error, or a further
    /// form.
    ///
    /// # Errors
    ///
    /// An error string the model can read: no credential, a transport fault, the server's own
    /// error, or [`ELICITATION_EXPIRED`] when the gateway no longer holds the call.
    pub async fn answer_elicitation(
        &self,
        org_id: &str,
        user_id: &str,
        elicitation_id: &str,
        reply: ElicitationReply,
    ) -> Result<McpProxyOutcome, String> {
        let mut request = Request::new(answer_request(org_id, user_id, elicitation_id, reply));
        let bearer = self
            .gateway_bearer(org_id)
            .await
            .map_err(|status| format!("mcp gateway credential: {}", status.message()))?;
        insert_bearer(&mut request, "authorization", &bearer)
            .map_err(|status| status.message().to_owned())?;
        let response = McpElicitationServiceClient::new(self.gateway.channel.clone())
            .answer_mcp_elicitation(request)
            .await
            .map_err(|e| format!("mcp elicitation transport: {e}"))?
            .into_inner();
        answer_outcome(response)
    }

    /// One screening verdict from model-gateway, asked as the execution service. The capability
    /// bearer rides along when this service can mint one, so the gateway reads the org's injection
    /// policy. Without it the gateway keeps its own default mode and screening stays on.
    ///
    /// # Errors
    ///
    /// A status when no gateway credential could be obtained or the gateway refused the call.
    pub async fn screen_content(
        &self,
        request: pb::ScreenContentRequest,
    ) -> Result<pb::ScreenContentResponse, Status> {
        let org_id = request.org_id.clone();
        let mut request = Request::new(request);
        insert_bearer(
            &mut request,
            "authorization",
            &self.gateway_bearer(&org_id).await?,
        )?;
        if let Ok(bearer) = self.capability_bearer(&org_id).await {
            insert_bearer(&mut request, "x-capability-authorization", &bearer)?;
        }
        ModelGatewayClient::new(self.gateway.channel.clone())
            .screen_content(request)
            .await
            .map(tonic::Response::into_inner)
    }
}

#[tonic::async_trait]
impl GovernedSource for GovernedMcp {
    async fn listing(&self, org_id: &str, user_id: &str) -> Result<McpListing, Status> {
        GovernedMcp::listing(self, org_id, user_id).await
    }

    async fn bindings(
        &self,
        run_id: &str,
        org_id: &str,
        user_id: &str,
        names: &[String],
    ) -> Result<Vec<pb::ToolBinding>, Status> {
        GovernedMcp::bindings(self, run_id, org_id, user_id, names).await
    }
}

fn insert_bearer<T>(
    request: &mut Request<T>,
    header: &'static str,
    token: &str,
) -> Result<(), Status> {
    request.metadata_mut().insert(
        header,
        format!("Bearer {token}")
            .parse()
            .map_err(|_| Status::internal("credential is not forwardable"))?,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_server_and_tool() {
        assert_eq!(
            parse_mcp_tool_name("mcp__01ABC__read_file"),
            Some(("01ABC", "read_file"))
        );
    }

    #[test]
    fn splits_on_first_separator_so_tool_may_contain_underscores() {
        assert_eq!(
            parse_mcp_tool_name("mcp__srv__do__a__thing"),
            Some(("srv", "do__a__thing"))
        );
    }

    #[test]
    fn rejects_non_mcp_and_malformed() {
        assert_eq!(parse_mcp_tool_name("web_search"), None);
        assert_eq!(parse_mcp_tool_name("mcp__noseparator"), None);
    }

    #[test]
    fn only_the_exact_literals_move_the_switch_off_off() {
        assert_eq!(McpDoTools::from_raw(None), McpDoTools::Off);
        assert_eq!(McpDoTools::from_raw(Some("off")), McpDoTools::Off);
        assert_eq!(McpDoTools::from_raw(Some("read")), McpDoTools::Read);
        assert_eq!(McpDoTools::from_raw(Some("all")), McpDoTools::All);
        for raw in [
            "", "READ", "Read", " read", "read ", "true", "1", "on", "write",
        ] {
            assert_eq!(McpDoTools::from_raw(Some(raw)), McpDoTools::Off, "{raw:?}");
        }
        assert_eq!(McpDoTools::default(), McpDoTools::Off);
    }

    #[test]
    fn a_mode_allows_exactly_the_classes_it_names() {
        for class in SideEffectClass::ALL {
            assert!(!McpDoTools::Off.allows(class), "{class:?}");
            assert_eq!(
                McpDoTools::Read.allows(class),
                class == SideEffectClass::Read,
                "{class:?}"
            );
            assert!(McpDoTools::All.allows(class), "{class:?}");
        }
    }

    #[test]
    fn a_signed_class_confirms_the_view_only_when_class_and_floor_both_match() {
        let view = ToolClassView::from_registry("mcp__srv__x", "read", "none", DIGEST);
        assert!(!SignedToolClass::new("read", "none").differs_from("mcp__srv__x", &view));
        // Tightened since the offer: the class, or the floor alone.
        assert!(SignedToolClass::new("external_write", "none").differs_from("mcp__srv__x", &view));
        assert!(SignedToolClass::new("read", "ask").differs_from("mcp__srv__x", &view));
        // A decision that does not say cannot confirm anything.
        assert!(SignedToolClass::default().differs_from("mcp__srv__x", &view));
        assert!(SignedToolClass::new(" ", "none").differs_from("mcp__srv__x", &view));
        // A class outside the vocabulary is the strictest value, never a read.
        assert!(SignedToolClass::new("READ", "none").differs_from("mcp__srv__x", &view));
    }

    #[test]
    fn the_cap_is_a_positive_count_or_the_default() {
        assert_eq!(cap_from_raw(None), DEFAULT_TOOL_CAP);
        assert_eq!(cap_from_raw(Some("12")), 12);
        assert_eq!(cap_from_raw(Some(" 7 ")), 7);
        assert_eq!(cap_from_raw(Some("256")), 256);
        for raw in ["0", "-1", "257", "many", "", "4.5"] {
            assert_eq!(cap_from_raw(Some(raw)), DEFAULT_TOOL_CAP, "{raw:?}");
        }
    }

    const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OTHER_DIGEST: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn definition(name: &str) -> pb::ToolDefinition {
        pb::ToolDefinition {
            name: name.to_owned(),
            description: format!("{name} description"),
            parameters_json: "{}".to_owned(),
        }
    }

    fn meta(name: &str, digest: &str) -> pb::McpToolMeta {
        pb::McpToolMeta {
            qualified_name: name.to_owned(),
            server_id: "srv".to_owned(),
            tool_name: name.to_owned(),
            digest: digest.to_owned(),
            declared_side_effect: "read_only".to_owned(),
        }
    }

    fn binding(name: &str, class: &str) -> pb::ToolBinding {
        pb::ToolBinding {
            tool_name: name.to_owned(),
            capability_id: "cap.mcp_tool.abc".to_owned(),
            capability_version: "1.0.0".to_owned(),
            side_effect_class: class.to_owned(),
            approval_floor: "none".to_owned(),
            tool_digest: DIGEST.to_owned(),
            review_state: "approved".to_owned(),
            effective_policy: "allow".to_owned(),
        }
    }

    fn listing(names: &[&str]) -> McpListing {
        McpListing {
            tools: names.iter().map(|name| definition(name)).collect(),
            meta: names.iter().map(|name| meta(name, DIGEST)).collect(),
        }
    }

    fn offered(offer: &McpOffer) -> Vec<&str> {
        offer.tools.iter().map(|tool| tool.name.as_str()).collect()
    }

    #[test]
    fn an_approved_digest_matched_read_tool_is_offered_in_read_mode() {
        let offer = plan_offer(
            McpDoTools::Read,
            40,
            listing(&["mcp__srv__echo_read"]),
            &[binding("mcp__srv__echo_read", "read")],
        );
        assert_eq!(offered(&offer), ["mcp__srv__echo_read"]);
        let (name, view) = &offer.views[0];
        assert_eq!(name, "mcp__srv__echo_read");
        assert_eq!(view.class(), SideEffectClass::Read);
        assert_eq!(view.digest(), DIGEST);
        assert!(offer.withheld.is_empty());
    }

    #[test]
    fn read_mode_withholds_every_class_but_read_and_all_mode_offers_them() {
        let names = [
            "mcp__srv__a_read",
            "mcp__srv__b_write",
            "mcp__srv__c_destroy",
            "mcp__srv__d_unknown",
        ];
        let bindings = [
            binding(names[0], "read"),
            binding(names[1], "external_write"),
            binding(names[2], "destructive"),
            binding(names[3], "unclassified"),
        ];
        let read = plan_offer(McpDoTools::Read, 40, listing(&names), &bindings);
        assert_eq!(offered(&read), ["mcp__srv__a_read"]);
        assert_eq!(read.withheld[withheld::CLASS_NOT_ALLOWED], 3);

        let all = plan_offer(McpDoTools::All, 40, listing(&names), &bindings);
        assert_eq!(offered(&all), names);
        assert!(all.withheld.is_empty());

        let off = plan_offer(McpDoTools::Off, 40, listing(&names), &bindings);
        assert!(off.tools.is_empty());
    }

    #[test]
    fn a_class_the_registry_does_not_recognise_is_never_a_read() {
        let offer = plan_offer(
            McpDoTools::Read,
            40,
            listing(&["mcp__srv__x"]),
            &[binding("mcp__srv__x", "READ")],
        );
        assert!(offer.tools.is_empty());
        assert_eq!(offer.withheld[withheld::CLASS_NOT_ALLOWED], 1);
    }

    #[test]
    fn nothing_is_offered_without_an_approved_digest_matched_registry_binding() {
        let mut drifted = binding("mcp__srv__drifted", "read");
        drifted.tool_digest = OTHER_DIGEST.to_owned();
        let mut pending = binding("mcp__srv__pending", "read");
        pending.review_state = "pending".to_owned();
        let mut quarantined = binding("mcp__srv__quarantined", "read");
        quarantined.review_state = "drifted".to_owned();
        let mut denied = binding("mcp__srv__denied", "read");
        denied.effective_policy = "deny".to_owned();
        let mut foreign = binding("mcp__srv__foreign", "read");
        foreign.capability_id = "cap.web.fetch".to_owned();
        let mut undigested = binding("mcp__srv__undigested", "read");
        undigested.tool_digest.clear();
        let mut no_meta = listing(&["mcp__srv__nometa"]);
        no_meta.meta.clear();

        let mut subject = listing(&[
            "mcp__srv__drifted",
            "mcp__srv__pending",
            "mcp__srv__quarantined",
            "mcp__srv__denied",
            "mcp__srv__foreign",
            "mcp__srv__undigested",
            "mcp__srv__unbound",
        ]);
        // The gateway observed the undigested tool with the digest the registry
        // lacks: an empty pin never matches.
        let offer = plan_offer(
            McpDoTools::All,
            40,
            subject.clone(),
            &[drifted, pending, quarantined, denied, foreign, undigested],
        );
        assert!(offer.tools.is_empty(), "{:?}", offered(&offer));
        assert_eq!(offer.withheld[withheld::DIGEST_MISMATCH], 2);
        assert_eq!(offer.withheld[withheld::NOT_APPROVED], 2);
        assert_eq!(offer.withheld[withheld::POLICY_DENY], 1);
        assert_eq!(offer.withheld[withheld::NOT_MCP_BINDING], 1);
        assert_eq!(offer.withheld[withheld::UNBOUND], 1);

        subject.tools.push(definition("mcp__srv__nometa"));
        let offer = plan_offer(
            McpDoTools::All,
            40,
            McpListing {
                tools: no_meta.tools,
                meta: Vec::new(),
            },
            &[binding("mcp__srv__nometa", "read")],
        );
        assert!(offer.tools.is_empty());
        assert_eq!(offer.withheld[withheld::NO_META], 1);
    }

    #[test]
    fn a_policy_that_asks_still_offers_the_tool() {
        let mut asking = binding("mcp__srv__ask", "read");
        asking.effective_policy = "ask".to_owned();
        let offer = plan_offer(McpDoTools::Read, 40, listing(&["mcp__srv__ask"]), &[asking]);
        assert_eq!(offered(&offer), ["mcp__srv__ask"]);
    }

    #[test]
    fn the_offer_is_sorted_by_qualified_name_and_cut_at_the_cap() {
        let names = [
            "mcp__srv__zeta",
            "mcp__srv__alpha",
            "mcp__srv__mid",
            "mcp__srv__beta",
        ];
        let bindings: Vec<pb::ToolBinding> =
            names.iter().map(|name| binding(name, "read")).collect();
        let offer = plan_offer(McpDoTools::Read, 3, listing(&names), &bindings);
        assert_eq!(
            offered(&offer),
            ["mcp__srv__alpha", "mcp__srv__beta", "mcp__srv__mid"]
        );
        assert_eq!(offer.withheld[withheld::OVER_CAP], 1);
        assert_eq!(
            offer
                .views
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            offered(&offer),
            "the gate's views and the model's tools are the same set"
        );

        // The same set in any listing order is the same prefix.
        let reversed: Vec<&str> = names.iter().rev().copied().collect();
        let again = plan_offer(McpDoTools::Read, 3, listing(&reversed), &bindings);
        assert_eq!(offered(&again), offered(&offer));
    }

    #[test]
    fn a_name_listed_twice_is_offered_once() {
        let mut twice = listing(&["mcp__srv__echo"]);
        twice.tools.push(definition("mcp__srv__echo"));
        let offer = plan_offer(
            McpDoTools::Read,
            40,
            twice,
            &[binding("mcp__srv__echo", "read")],
        );
        assert_eq!(offered(&offer), ["mcp__srv__echo"]);
    }

    #[test]
    fn an_owner_effect_view_keeps_its_floor_whatever_the_mode() {
        let mut owner = binding("mcp__srv__owner", "owner_effect");
        owner.approval_floor = "none".to_owned();
        let offer = plan_offer(McpDoTools::All, 40, listing(&["mcp__srv__owner"]), &[owner]);
        assert_eq!(
            offer.views[0].1.floor(),
            mp_contracts::tool_class::ApprovalFloor::DurableOwner
        );
    }

    // ---------------------------------------------------------------------
    // The registry call, through a real gRPC channel to a stand-in
    // ---------------------------------------------------------------------

    use mp_contracts::model_plane::v1::{
        capability_core_server::{CapabilityCore, CapabilityCoreServer},
        CapabilityDetail, CheckSkillPromotionRequest, CheckSkillPromotionResponse,
        EvaluatePolicyRequest, EvaluatePolicyResponse, GetCapabilityRequest,
        ListCapabilitiesRequest, ListCapabilitiesResponse, PromoteSkillRequest,
        PromoteSkillResponse, ResolveToolBindingsRequest, ResolveToolBindingsResponse,
        ValidateSkillBundleRequest, ValidateSkillBundleResponse,
    };
    use std::sync::{Arc, Mutex};
    use tokio::net::TcpListener;
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::transport::{Endpoint, Server};
    use tonic::Response;

    #[derive(Default)]
    struct Seen {
        requests: Mutex<Vec<ResolveToolBindingsRequest>>,
        authorization: Mutex<Vec<String>>,
        bindings: Mutex<Vec<pb::ToolBinding>>,
        fail: Mutex<bool>,
    }

    struct StandIn(Arc<Seen>);

    #[tonic::async_trait]
    impl CapabilityCore for StandIn {
        async fn list_capabilities(
            &self,
            _: Request<ListCapabilitiesRequest>,
        ) -> Result<Response<ListCapabilitiesResponse>, Status> {
            Err(Status::unimplemented("not used"))
        }
        async fn get_capability(
            &self,
            _: Request<GetCapabilityRequest>,
        ) -> Result<Response<CapabilityDetail>, Status> {
            Err(Status::unimplemented("not used"))
        }
        async fn evaluate_policy(
            &self,
            _: Request<EvaluatePolicyRequest>,
        ) -> Result<Response<EvaluatePolicyResponse>, Status> {
            Err(Status::unimplemented("not used"))
        }
        async fn resolve_tool_bindings(
            &self,
            request: Request<ResolveToolBindingsRequest>,
        ) -> Result<Response<ResolveToolBindingsResponse>, Status> {
            if *self.0.fail.lock().unwrap() {
                return Err(Status::unavailable("down"));
            }
            self.0.authorization.lock().unwrap().push(
                request
                    .metadata()
                    .get("authorization")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or_default()
                    .to_owned(),
            );
            let request = request.into_inner();
            let bindings = self
                .0
                .bindings
                .lock()
                .unwrap()
                .iter()
                .filter(|binding| request.tool_names.contains(&binding.tool_name))
                .cloned()
                .collect();
            self.0.requests.lock().unwrap().push(request);
            Ok(Response::new(ResolveToolBindingsResponse { bindings }))
        }
        async fn validate_skill_bundle(
            &self,
            _: Request<ValidateSkillBundleRequest>,
        ) -> Result<Response<ValidateSkillBundleResponse>, Status> {
            Err(Status::unimplemented("not used"))
        }
        async fn check_skill_promotion(
            &self,
            _: Request<CheckSkillPromotionRequest>,
        ) -> Result<Response<CheckSkillPromotionResponse>, Status> {
            Err(Status::unimplemented("not used"))
        }
        async fn promote_skill(
            &self,
            _: Request<PromoteSkillRequest>,
        ) -> Result<Response<PromoteSkillResponse>, Status> {
            Err(Status::unimplemented("not used"))
        }

        async fn list_skill_bundles(
            &self,
            _: Request<mp_contracts::model_plane::v1::ListSkillBundlesRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::ListSkillBundlesResponse>, Status> {
            Err(Status::unimplemented("not used"))
        }

        async fn get_skill_bundle_manifest(
            &self,
            _: Request<mp_contracts::model_plane::v1::GetSkillBundleManifestRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::SkillBundleManifest>, Status> {
            Err(Status::unimplemented("not used"))
        }

        async fn get_skill_blob(
            &self,
            _: Request<mp_contracts::model_plane::v1::GetSkillBlobRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::GetSkillBlobResponse>, Status> {
            Err(Status::unimplemented("not used"))
        }
    }

    async fn governed_against_a_stand_in() -> (GovernedMcp, Arc<Seen>) {
        let seen = Arc::new(Seen::default());
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = listener.local_addr().expect("address");
        let service = StandIn(seen.clone());
        tokio::spawn(async move {
            Server::builder()
                .add_service(CapabilityCoreServer::new(service))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .ok();
        });
        let registry = Endpoint::from_shared(format!("http://{address}"))
            .expect("endpoint")
            .connect()
            .await
            .expect("connect");
        let gateway = McpGatewayClient::from_env().expect("lazy gateway channel");
        (
            GovernedMcp {
                gateway,
                registry,
                credentials: Credentials::Static,
            },
            seen,
        )
    }

    #[tokio::test]
    async fn the_registry_is_asked_as_the_execution_service_for_the_runs_own_tenant() {
        let (governed, seen) = governed_against_a_stand_in().await;
        seen.bindings
            .lock()
            .unwrap()
            .push(binding("mcp__srv__echo_read", "read"));
        let names = vec![
            "mcp__srv__echo_read".to_owned(),
            "mcp__srv__gone".to_owned(),
        ];
        let bindings = governed
            .bindings("run-1", "org-1", "user-1", &names)
            .await
            .expect("the stand-in answers");
        assert_eq!(bindings.len(), 1, "a name the registry lacks is absent");
        assert_eq!(bindings[0].tool_name, "mcp__srv__echo_read");

        let requests = seen.requests.lock().unwrap();
        assert_eq!(requests.len(), 1, "one lookup for the whole offer");
        assert_eq!(requests[0].org_id, "org-1");
        assert_eq!(requests[0].run_id, "run-1");
        assert_eq!(requests[0].user_id, "user-1");
        assert_eq!(requests[0].tool_names, names);
        assert_eq!(
            seen.authorization.lock().unwrap().as_slice(),
            ["Bearer capability-token"]
        );
    }

    #[tokio::test]
    async fn an_unreachable_registry_is_an_error_not_an_empty_answer() {
        let (governed, seen) = governed_against_a_stand_in().await;
        *seen.fail.lock().unwrap() = true;
        let error = governed
            .bindings("run-1", "org-1", "user-1", &["mcp__srv__x".to_owned()])
            .await
            .expect_err("an outage must not read as 'nothing is bound'");
        assert_eq!(error.code(), tonic::Code::Unavailable);
    }

    #[test]
    fn a_credential_is_sent_as_a_bearer_and_only_if_it_can_be() {
        let mut request = Request::new(());
        insert_bearer(&mut request, "authorization", "abc").expect("a plain token");
        assert_eq!(
            request.metadata().get("authorization").unwrap(),
            "Bearer abc"
        );
        let refused = insert_bearer(&mut request, "authorization", "line\nbreak")
            .expect_err("a token that cannot be a header value is refused");
        assert_eq!(refused.code(), tonic::Code::Internal);
    }

    // ---------------------------------------------------------------------
    // How many MCP tools one run can be offered, and which names are looked up
    // ---------------------------------------------------------------------

    /// A gateway and registry that list `mcp_count` approved read tools and know every
    /// built-in the run names. It records the names the registry was asked for.
    struct Bulk {
        mcp_count: usize,
        asked: Mutex<Vec<String>>,
    }

    fn bulk_name(index: usize) -> String {
        format!("mcp__srv__t{index:03}")
    }

    #[tonic::async_trait]
    impl GovernedSource for Bulk {
        async fn listing(&self, _: &str, _: &str) -> Result<McpListing, Status> {
            let names: Vec<String> = (0..self.mcp_count).map(bulk_name).collect();
            Ok(McpListing {
                tools: names.iter().map(|name| definition(name)).collect(),
                meta: names.iter().map(|name| meta(name, DIGEST)).collect(),
            })
        }

        async fn bindings(
            &self,
            _: &str,
            _: &str,
            _: &str,
            names: &[String],
        ) -> Result<Vec<pb::ToolBinding>, Status> {
            *self.asked.lock().unwrap() = names.to_vec();
            Ok(names
                .iter()
                .map(|name| {
                    if name.starts_with("mcp__") {
                        binding(name, "read")
                    } else {
                        pb::ToolBinding {
                            tool_name: name.clone(),
                            capability_id: "cap.builtin.fixture".to_owned(),
                            side_effect_class: "read".to_owned(),
                            approval_floor: "none".to_owned(),
                            ..Default::default()
                        }
                    }
                })
                .collect())
        }
    }

    /// The built-in tools a run offers: the late-sorting names are the ones a cut of the
    /// sorted union would take first.
    fn builtin_names(count: usize) -> Vec<String> {
        let mut names: Vec<String> = ["save_memory", "shell", "web_fetch"]
            .iter()
            .map(|name| (*name).to_owned())
            .collect();
        names.extend((0..count.saturating_sub(3)).map(|index| format!("builtin_{index:03}")));
        names
    }

    #[test]
    fn the_cap_never_exceeds_the_views_a_run_keeps_after_its_built_in_tools() {
        let views = crate::permission::MAX_VIEWS_PER_RUN;
        assert_eq!(effective_cap(40, 40), 40, "a cap that fits is the cap");
        assert_eq!(effective_cap(MAX_TOOL_CAP, 40), views - 40);
        assert_eq!(effective_cap(MAX_TOOL_CAP, 0), views.min(MAX_BINDING_NAMES));
        assert_eq!(effective_cap(40, views), 0, "built-ins fill the store: no room");
        assert_eq!(effective_cap(40, views + 10), 0);
    }

    #[tokio::test]
    async fn a_high_cap_never_offers_a_tool_the_run_has_no_view_for() {
        let source = Bulk {
            mcp_count: 300,
            asked: Mutex::new(Vec::new()),
        };
        let builtin = builtin_names(40);
        let resolved = resolve_run_offer(
            &source,
            McpDoTools::Read,
            MAX_TOOL_CAP,
            "run-bulk",
            "org-bulk",
            "user",
            &builtin,
        )
        .await;
        let views = crate::permission::MAX_VIEWS_PER_RUN;
        let offered: Vec<String> = resolved
            .offer
            .tools
            .iter()
            .map(|tool| tool.name.clone())
            .collect();
        assert_eq!(offered.len(), views - builtin.len());
        let over_cap = resolved.offer.withheld[withheld::OVER_CAP];
        // What the run records is every built-in view and every offered tool's view.
        crate::permission::record_run_views(
            "org-bulk",
            "run-bulk",
            resolved
                .builtin_views
                .into_iter()
                .chain(resolved.offer.views),
        );
        for name in builtin.iter().chain(offered.iter()) {
            assert!(
                crate::permission::run_view("org-bulk", "run-bulk", name).is_some(),
                "{name} has no view: a call to it would be refused as 'not bound'"
            );
        }
        // The tools that did not fit are counted, never offered.
        assert_eq!(over_cap, 300 - offered.len());
    }

    #[tokio::test]
    async fn a_built_in_name_is_never_the_one_the_registry_lookup_cuts() {
        // Sorted with the MCP names, `save_memory`, `shell` and `web_fetch` come after
        // `mcp__`, so a cut of the union takes them first.
        let source = Bulk {
            mcp_count: 400,
            asked: Mutex::new(Vec::new()),
        };
        let builtin = builtin_names(40);
        let resolved = resolve_run_offer(
            &source,
            McpDoTools::Read,
            40,
            "run-names",
            "org-names",
            "user",
            &builtin,
        )
        .await;
        let asked = source.asked.lock().unwrap().clone();
        assert!(asked.len() <= MAX_BINDING_NAMES);
        for name in &builtin {
            assert!(asked.contains(name), "{name} was cut from the lookup");
        }
        let learned: Vec<&str> = resolved
            .builtin_views
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        for name in ["save_memory", "shell", "web_fetch"] {
            assert!(learned.contains(&name), "{name} lost its view");
        }
        assert_eq!(resolved.offer.tools.len(), 40, "the configured cap still holds");
    }

    #[test]
    fn the_proxy_request_carries_the_digest_the_gateway_must_recheck() {
        let governed = proxy_request("org", "user", "srv", "tool", "{}", DIGEST, false);
        assert_eq!(governed.expected_digest, DIGEST);
        assert_eq!(governed.server_id, "srv");
        assert_eq!(governed.tool_name, "tool");
        let plain = proxy_request("org", "user", "srv", "tool", "{}", "", false);
        assert!(plain.expected_digest.is_empty());
    }

    // --- EP-4 S7: elicitation -----------------------------------------------

    fn root_run() -> ElicitationRun {
        ElicitationRun {
            depth: 0,
            zdr: false,
            waived: false,
            parkable: true,
            space_scoped: false,
            questions_asked: 0,
        }
    }

    fn pending(label: &str, expires_at_unix: i64) -> pb::McpElicitationPending {
        pb::McpElicitationPending {
            elicitation_id: "el_01".to_owned(),
            server_id: "srv_01".to_owned(),
            server_label: label.to_owned(),
            message: "Which project?".to_owned(),
            requested_schema_json: "{}".to_owned(),
            items: vec![pb::QuestionItem {
                id: "project".to_owned(),
                kind: "text".to_owned(),
                prompt: "Project".to_owned(),
                ..Default::default()
            }],
            expires_at_unix,
        }
    }

    fn question(state: pb::QuestionState, answers: Vec<pb::QuestionAnswerItem>) -> pb::RunQuestion {
        pb::RunQuestion {
            state: state as i32,
            answers,
            ..Default::default()
        }
    }

    #[test]
    fn the_elicitation_switch_reads_only_the_literal_true() {
        assert!(elicitation_switch_on(Some("true")));
        for raw in [None, Some(""), Some("TRUE"), Some("1"), Some(" true"), Some("yes")] {
            assert!(!elicitation_switch_on(raw), "{raw:?}");
        }
    }

    #[test]
    fn allow_elicitation_false_when_switch_off() {
        assert!(allow_elicitation_when(true, &root_run()));
        assert!(!allow_elicitation_when(false, &root_run()));
        let request = proxy_request("org", "user", "srv", "tool", "{}", DIGEST, false);
        assert!(!request.allow_elicitation, "off keeps today's wire");
        let request = proxy_request("org", "user", "srv", "tool", "{}", DIGEST, true);
        assert!(request.allow_elicitation);
    }

    #[test]
    fn allow_elicitation_false_for_subagent_and_space_runs() {
        let refused = [
            ElicitationRun { depth: 1, ..root_run() },
            ElicitationRun { space_scoped: true, ..root_run() },
            ElicitationRun { zdr: true, ..root_run() },
            ElicitationRun { waived: true, ..root_run() },
            ElicitationRun { parkable: false, ..root_run() },
        ];
        for run in refused {
            assert!(!allow_elicitation_when(true, &run), "{run:?}");
        }
    }

    #[test]
    fn question_cap_counts_elicitations() {
        let cap = u32::try_from(user_question::MAX_QUESTIONS_PER_RUN).expect("small");
        let below = ElicitationRun { questions_asked: cap - 1, ..root_run() };
        assert!(allow_elicitation_when(true, &below));
        let at = ElicitationRun { questions_asked: cap, ..root_run() };
        assert!(!allow_elicitation_when(true, &at), "a run at its question cap asks no more");
    }

    #[test]
    fn a_proxy_answer_with_a_form_is_an_elicitation() {
        let form = pending("Fixture", 1_000);
        let outcome = proxy_outcome(pb::ProxyMcpToolResponse {
            output_json: String::new(),
            error_message: String::new(),
            elicitation: Some(form.clone()),
            ..Default::default()
        });
        assert_eq!(outcome, Ok(McpProxyOutcome::Elicitation(form)));
        let done = proxy_outcome(pb::ProxyMcpToolResponse {
            output_json: "{\"ok\":true}".to_owned(),
            error_message: String::new(),
            elicitation: None,
            ..Default::default()
        });
        assert_eq!(done, Ok(McpProxyOutcome::Output("{\"ok\":true}".to_owned())));
        let failed = proxy_outcome(pb::ProxyMcpToolResponse {
            output_json: String::new(),
            error_message: "boom".to_owned(),
            elicitation: None,
            ..Default::default()
        });
        assert_eq!(failed, Err("boom".to_owned()));
    }

    #[test]
    fn pending_form_creates_a_resume_question_with_server_origin() {
        let form = pending("Fixture", 1_600);
        let request = elicitation_question("run_1", "step_1", "org_1", &form, 1, 1_000)
            .expect("a live form is asked");
        assert_eq!(request.delivery, user_question::DELIVERY_RESUME);
        assert_eq!(request.origin, user_question::ORIGIN_MCP_ELICITATION);
        assert_eq!(request.origin_label, "Fixture");
        assert_eq!(request.items, form.items);
        assert_eq!(request.expires_in_seconds, 600, "the question lives as long as the hold");
        assert_eq!(request.idempotency_key, "run_1:step_1:elicit:1");
        assert_eq!(request.run_id, "run_1");
        assert_eq!(request.org_id, "org_1");
    }

    #[test]
    fn retry_returns_the_same_question() {
        let form = pending("Fixture", 2_000);
        let first = elicitation_question("run_1", "step_1", "org_1", &form, 1, 1_000).expect("asked");
        let retry = elicitation_question("run_1", "step_1", "org_1", &form, 1, 1_500).expect("asked");
        assert_eq!(first.idempotency_key, retry.idempotency_key);
        let second_form =
            elicitation_question("run_1", "step_1", "org_1", &form, 2, 1_500).expect("asked");
        assert_ne!(first.idempotency_key, second_form.idempotency_key);
    }

    #[test]
    fn an_expired_or_empty_form_is_not_asked() {
        assert!(elicitation_question("r", "s", "o", &pending("F", 1_000), 1, 1_000).is_none());
        let mut empty = pending("F", 5_000);
        empty.items.clear();
        assert!(elicitation_question("r", "s", "o", &empty, 1, 1_000).is_none());
        let far = pending("F", i64::MAX);
        let request = elicitation_question("r", "s", "o", &far, 1, 0).expect("asked");
        assert_eq!(request.expires_in_seconds, user_question::RESUME_TTL_SECS);
    }

    #[test]
    fn the_origin_label_is_clean_bounded_and_never_blank() {
        assert_eq!(elicitation_origin_label(&pending("Fix\u{7}ture\n", 0)), "Fixture");
        assert_eq!(elicitation_origin_label(&pending("  ", 0)), "srv_01");
        let long = "x".repeat(200);
        assert_eq!(
            elicitation_origin_label(&pending(&long, 0)).chars().count(),
            user_question::MAX_ORIGIN_LABEL
        );
        assert_eq!(
            elicitation_waiting_text("Fixture"),
            "Venter på svar fra eieren på et skjema fra Fixture."
        );
    }

    #[test]
    fn answer_is_mapped_and_sent_as_accept() {
        let answers = vec![pb::QuestionAnswerItem {
            item_id: "project".to_owned(),
            option_ids: Vec::new(),
            text: "Apollo".to_owned(),
        }];
        let reply = elicitation_reply(&question(pb::QuestionState::Answered, answers.clone()));
        assert_eq!(reply.action, ELICITATION_ACCEPT);
        assert_eq!(reply.answers, answers);
        let request = answer_request("org", "owner", "el_01", reply);
        assert_eq!(request.action, "accept");
        assert_eq!(request.user_id, "owner");
        assert_eq!(request.elicitation_id, "el_01");
        assert_eq!(request.answers, answers);
    }

    #[test]
    fn skip_sends_decline() {
        let reply = elicitation_reply(&question(pb::QuestionState::Skipped, Vec::new()));
        assert_eq!(reply.action, ELICITATION_DECLINE);
        assert!(reply.answers.is_empty());
    }

    #[test]
    fn expiry_sends_cancel() {
        for state in [
            pb::QuestionState::Expired,
            pb::QuestionState::Cancelled,
            pb::QuestionState::Requested,
            pb::QuestionState::Unspecified,
        ] {
            let reply = elicitation_reply(&question(state, Vec::new()));
            assert_eq!(reply.action, ELICITATION_CANCEL, "{state:?}");
        }
        let empty_answer = elicitation_reply(&question(pb::QuestionState::Answered, Vec::new()));
        assert_eq!(empty_answer.action, ELICITATION_CANCEL, "an empty answer is no consent");
    }

    #[test]
    fn lost_held_call_is_a_tool_error() {
        let lost = answer_outcome(pb::AnswerMcpElicitationResponse {
            output_json: String::new(),
            error_message: format!("{ELICITATION_EXPIRED}: the form is no longer waiting"),
            elicitation: None,
        })
        .expect_err("a call the gateway lost is an error");
        assert!(lost.starts_with(ELICITATION_EXPIRED), "{lost}");
        let done = answer_outcome(pb::AnswerMcpElicitationResponse {
            output_json: "{}".to_owned(),
            error_message: String::new(),
            elicitation: None,
        });
        assert_eq!(done, Ok(McpProxyOutcome::Output("{}".to_owned())));
        let again = pending("Fixture", 1_000);
        let asked_again = answer_outcome(pb::AnswerMcpElicitationResponse {
            output_json: String::new(),
            error_message: String::new(),
            elicitation: Some(again.clone()),
        });
        assert_eq!(asked_again, Ok(McpProxyOutcome::Elicitation(again)));
    }
}
