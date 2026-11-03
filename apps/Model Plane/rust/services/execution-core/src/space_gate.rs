//! What an approval card says about a gated call (build plan WS10-S1).
//!
//! The card context is built here from an allowlist: a stable `tool_key`, a short `action_label`
//! and the `resume_class` the pause expects. Raw tool arguments never enter it. Session Core
//! validates the object again and decides the resume class from the rows it holds (a continuation
//! descriptor makes it an action continuation), so this module states intent, not authority.
//!
//! The three resumable tool names live here once; the approval delivery worker reads them from
//! this module so the list the card promises and the list the worker can run cannot drift
//! (WS10-S9: `RESUMABLE_KINDS`, `resume_class_for`, the proof checklist and the parity test).

use serde_json::json;

/// Provider write run by the approval delivery worker from its descriptor.
pub(crate) const PROVIDER_ACTION_TOOL: &str = "execute_provider_action";
/// Shipment booking run by the approval delivery worker from its descriptor.
pub(crate) const SHIPMENT_BOOKING_TOOL: &str = "book_shipment";
/// Ticket creation run by the approval delivery worker from its descriptor.
pub(crate) const TICKETS_CREATE_TOOL: &str = "tickets.create";

/// Resume class names, the same words Session Core stores.
pub(crate) const RESUME_ACTION_CONTINUATION: &str = "action_continuation";
pub(crate) const RESUME_LIVE_POLL: &str = "live_poll";
pub(crate) const RESUME_NONE: &str = "none";

// ---------------------------------------------------------------------------------------------
// WS10-S9: the resumable-kind registry.
//
// A "resumable kind" is a tool whose granted approval the approval delivery worker can run cold,
// as the requester, from the encrypted continuation descriptor (`action_continuation`). This is
// the single list: the card's `resume_class`, the once-token exclusion and the worker's
// `parse_resumable_action` all read it, and the parity test (owed to the test phase) fails when
// the worker's match arms and this list drift. Everything not listed here is `none`: its grant
// lets the agent retry the exact action once, and the card says so.
//
// Space adds no kind to this list (plan SD-21): a Space run is never loop-owned, so there is no
// in-place resume to promise. Adding a kind is its own later change and must pass
// `NEW_KIND_CHECKLIST`.
// ---------------------------------------------------------------------------------------------

/// One tool the delivery worker can run from a descriptor. The proof fields are read by the
/// parity test and by reviewers.
#[allow(dead_code)]
pub(crate) struct ResumableKind {
    /// The tool name the descriptor carries (`ContinuationDescriptor.tool_name`).
    pub tool_name: &'static str,
    /// How a replay of the same approval cannot run the effect twice.
    pub idempotency: &'static str,
    /// What the worker checks about the descriptor before it runs (beyond org and user).
    pub binding: &'static str,
}

/// Every kind the worker can resume. Order is not significant.
pub(crate) const RESUMABLE_KINDS: &[ResumableKind] = &[
    ResumableKind {
        tool_name: PROVIDER_ACTION_TOOL,
        idempotency: "integration-corev2 replays the cached outcome for the same idempotency key (run and step)",
        binding: "input deserializes into the provider action; postcondition read-back settles an unreceipted write",
    },
    ResumableKind {
        tool_name: SHIPMENT_BOOKING_TOOL,
        idempotency: "the booking idempotency key is the run and step, the same as the live path",
        binding: "input deserializes into the booking; booked_by is injected from the run user at descriptor time",
    },
    ResumableKind {
        tool_name: TICKETS_CREATE_TOOL,
        idempotency: "ticket_idempotency_key bound at descriptor time",
        binding: "schema_sha256, payload_sha256 and ticket_idempotency_key match; owner_user_id equals user_id",
    },
];

/// What a new resumable kind must prove before it is added to `RESUMABLE_KINDS`. Each item is a
/// reviewer question; "no" to any of them means the kind stays `none`.
#[allow(dead_code)]
pub(crate) const NEW_KIND_CHECKLIST: &[&str] = &[
    "The create call carries an idempotency key that survives a worker retry, or the effect has a provider read-back that settles an unreceipted write.",
    "The descriptor captures every argument the effect needs, and none is a model-supplied identity (the actor is the requester from the descriptor).",
    "The worker re-checks the requester's authority with Control before it runs (Space runs: the standing-rule and membership check of WS10-S3).",
    "A zero-retention run is refused before a descriptor is written (continuation_descriptor already fails closed); the kind adds no content-persisting path.",
    "The kind has an arm in parse_resumable_action, a fixture in the parity test, and an action_label in space_gate::action_label.",
    "A failed or cancelled effect maps to an allowlisted failure code the card can show, never raw provider text.",
    "The once-token does not apply to it (the grant is its continuation), and a retry by the agent cannot run it a second time.",
];

/// True when the delivery worker can run this tool from a descriptor.
pub(crate) fn is_resumable_tool(tool_name: &str) -> bool {
    RESUMABLE_KINDS.iter().any(|kind| kind.tool_name == tool_name)
}

/// The resume class a pause for this tool can honestly state. Only a resumable kind that has a
/// descriptor is an action continuation; a browser live approval states `live_poll` itself.
pub(crate) fn resume_class_for(tool_name: &str, descriptor_present: bool) -> &'static str {
    if descriptor_present && is_resumable_tool(tool_name) {
        RESUME_ACTION_CONTINUATION
    } else {
        RESUME_NONE
    }
}

/// Longest value Session Core accepts in the card context.
const MAX_VALUE_CHARS: usize = 128;

/// A name that is safe to show: ASCII letters, digits and `_ . : - /`, at most 64 characters.
fn safe_identifier(value: &str) -> Option<&str> {
    let ok = !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b':' | b'-' | b'/'));
    ok.then_some(value)
}

/// The `tool_key` of a call: the tool's own name when it is a plain identifier, else empty.
pub(crate) fn tool_key(tool_name: &str) -> &str {
    safe_identifier(tool_name).unwrap_or_default()
}

/// A short label for the gated action, from an allowlist. Arguments are read only for the
/// provider operation name, and only when it is a plain identifier.
pub(crate) fn action_label(tool_name: &str, tool_input: &str) -> String {
    let label = match tool_name {
        PROVIDER_ACTION_TOOL => {
            let operation = serde_json::from_str::<serde_json::Value>(tool_input)
                .ok()
                .and_then(|input| input.get("operation")?.as_str().map(str::to_owned));
            match operation.as_deref().and_then(safe_identifier) {
                Some(operation) => format!("Run provider action {operation}"),
                None => "Run a provider action".to_owned(),
            }
        }
        SHIPMENT_BOOKING_TOOL => "Book a shipment".to_owned(),
        TICKETS_CREATE_TOOL => "Create a ticket".to_owned(),
        "shell" => "Run a command in the sandbox".to_owned(),
        "browser_agent" => "Run a browser action".to_owned(),
        other => match safe_identifier(other) {
            Some(name) => format!("Use the tool {name}"),
            None => "Use a tool".to_owned(),
        },
    };
    label.chars().take(MAX_VALUE_CHARS).collect()
}

/// The `CreateApprovalRequest.context_json` of a paused tool call. `resume_class` states what the
/// pause expects; Session Core overrides it from its own rows.
pub(crate) fn card_context_json(tool_name: &str, tool_input: &str, resume_class: &str) -> String {
    json!({
        "tool_key": tool_key(tool_name),
        "action_label": action_label(tool_name, tool_input),
        "resume_class": resume_class,
    })
    .to_string()
}

// ---------------------------------------------------------------------------------------------
// WS10-S3: the Space approval gate.
//
// A Space run's gated call is folded with Control's per-tool levels: `allow < ask < block`, the
// most restrictive wins. A source can only tighten, so no layer loosens another. An `ask` is then
// resolved in a fixed order: the never-auto set always pauses; the once-token (a person already
// granted this exact action and the agent is retrying it) passes once; a Control standing rule
// (MCP tools only) passes with a signed proof Session Core verifies; otherwise the call pauses
// as it always did. Non-Space runs never reach this code, and `EXECUTION_CORE_SPACE_APPROVAL_GATE=off`
// restores today's behaviour exactly.
// ---------------------------------------------------------------------------------------------

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use mp_contracts::approval_cause::CauseCode;
use mp_contracts::model_plane::v1 as pb;
use reqwest::{Client, StatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use tonic::transport::Channel;

use crate::control_http_client::{bounded_secret, env_flag, env_value, service_base_url, service_endpoint};

const GATE_ENV: &str = "EXECUTION_CORE_SPACE_APPROVAL_GATE";
const CONTROL_URL_ENV: &str = "CONTROL_PLANE_USER_CORE_URL";
const CONTROL_TOKEN_ENV: &str = "EXECUTION_CORE_CONTROL_APPROVAL_GATE_TOKEN";
const CONTROL_LOOPBACK_ENV: &str = "EXECUTION_CORE_ALLOW_INSECURE_APPROVAL_GATE_LOOPBACK";
const GATE_VIEW_PATH: &str = "/api/v1/internal/spaces/tool-gate-view";
const GRANT_USE_PATH: &str = "/api/v1/internal/spaces/approval-grants/use";
const CONTINUATION_CHECK_PATH: &str = "/api/v1/internal/spaces/approval-continuation-check";
const HTTP_TIMEOUT: Duration = Duration::from_secs(5);
/// Longest a gate view is reused (Control's own bound; the signed decisions live 120 s).
const VIEW_CACHE_MAX: Duration = Duration::from_secs(100);
const VIEW_CACHE_CAPACITY: usize = 1024;
/// Standing rules cover MCP tools only.
const STANDING_RULE_TOOL_PREFIX: &str = "mcp__";

/// How the gate runs (`EXECUTION_CORE_SPACE_APPROVAL_GATE`). Unset or unknown is `off`: a
/// deployment that does not name the mode keeps today's behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GateMode {
    Off,
    /// Reads the Control view and logs/counts where it would differ; changes nothing.
    Shadow,
    Enforce,
}

impl GateMode {
    pub(crate) fn parse(value: Option<&str>) -> Self {
        match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            Some("enforce") => Self::Enforce,
            Some("shadow") => Self::Shadow,
            _ => Self::Off,
        }
    }

    pub(crate) fn from_env() -> Self {
        Self::parse(std::env::var(GATE_ENV).ok().as_deref())
    }
}

/// A tool's level. Ordered so the most restrictive is the maximum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum GateLevel {
    Allow,
    Ask,
    Block,
}

impl GateLevel {
    fn from_wire(value: &str) -> Option<Self> {
        match value {
            "allow" => Some(Self::Allow),
            "ask" => Some(Self::Ask),
            "block" => Some(Self::Block),
            _ => None,
        }
    }
}

/// Fold the levels of every source: the most restrictive wins; no sources is `allow`.
pub(crate) fn fold(levels: &[GateLevel]) -> GateLevel {
    levels.iter().copied().max().unwrap_or(GateLevel::Allow)
}

/// The key a policy row and a standing rule name a call by: the tool, or for a provider action
/// `execute_provider_action:<operation>`. Never arguments.
pub(crate) fn action_key(tool_name: &str, tool_input: &str) -> String {
    if tool_name == PROVIDER_ACTION_TOOL {
        let operation = serde_json::from_str::<JsonValue>(tool_input)
            .ok()
            .and_then(|input| input.get("operation")?.as_str().map(str::to_owned));
        if let Some(operation) = operation.as_deref().and_then(safe_identifier) {
            return format!("{PROVIDER_ACTION_TOOL}:{operation}");
        }
    }
    tool_name.to_owned()
}

/// Canonical JSON: object keys sorted at every depth, no whitespace. Written out here so the
/// digest does not depend on a serde feature another crate might switch on.
fn canonical_json(value: &JsonValue, out: &mut String) {
    match value {
        JsonValue::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&JsonValue::String(key.clone()).to_string());
                out.push(':');
                canonical_json(&map[key], out);
            }
            out.push('}');
        }
        JsonValue::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                canonical_json(item, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// Hex sha256 of the canonical `{"input":<input>,"tool":<tool>}`. Input that is not JSON is
/// digested as the string it is. The digest is all Session Core keeps of the call.
pub(crate) fn action_digest(tool_name: &str, tool_input: &str) -> String {
    let input = serde_json::from_str::<JsonValue>(tool_input)
        .unwrap_or_else(|_| JsonValue::String(tool_input.to_owned()));
    let mut canonical = String::new();
    canonical_json(
        &serde_json::json!({ "tool": tool_name, "input": input }),
        &mut canonical,
    );
    Sha256::digest(canonical.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A tool's rule registry digest as the `sha256:<hex>` commitment Control binds a rule to.
fn schema_commitment(digest: &str) -> Option<String> {
    let digest = digest.trim();
    let hex = digest.strip_prefix("sha256:").unwrap_or(digest);
    (hex.len() == 64 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
        .then(|| format!("sha256:{hex}"))
}

#[derive(Debug)]
#[allow(dead_code)] // the fields are read through `Debug` in the gate's log lines
pub(crate) enum ControlGateError {
    /// Control answered a final refusal (4xx); `code` is its typed code.
    Refused { status: u16, code: Option<String> },
    /// Transport failure, 5xx or a malformed body.
    Unavailable(String),
}

#[derive(Debug, Clone, Deserialize)]
struct ToolLevel {
    #[serde(default)]
    level: String,
    #[serde(default)]
    source: String,
}

/// What Control says the gate may fold for one run (`tool-gate-view`). Every field is optional:
/// an absent `tools` entry means "no Control-side level", never `allow`-by-policy.
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct GateView {
    #[serde(default)]
    tools: HashMap<String, ToolLevel>,
    #[serde(default)]
    standing_rules_enabled: bool,
    #[serde(default)]
    agent_subject_id: Option<String>,
    /// The Space's quarantine setting (`off`, `shadow`, `enforce`); absent reads as `off`
    /// (build plan WS10-S8).
    #[serde(default)]
    quarantine_mode: Option<String>,
}

impl GateView {
    /// The level and source layer for a call: the specific action key first, then the tool, then
    /// the `*` row. Absent rows are `allow`.
    fn level_for(&self, action_key: &str, tool_key: &str) -> (GateLevel, String) {
        for key in [action_key, tool_key, "*"] {
            if let Some(row) = self.tools.get(key) {
                if let Some(level) = GateLevel::from_wire(&row.level) {
                    return (level, row.source.clone());
                }
            }
        }
        (GateLevel::Allow, String::new())
    }
}

#[derive(Debug, Deserialize)]
struct DataEnvelope<T> {
    data: T,
}

#[derive(Debug, Deserialize)]
struct GrantUseResponse {
    token: String,
}

/// `skip_serializing_if` helper: an empty field is left out of the request.
fn blank(value: &&str) -> bool {
    value.is_empty()
}

#[derive(Debug, Serialize)]
struct RunRequest<'a> {
    run_id: &'a str,
    org_id: &'a str,
    #[serde(skip_serializing_if = "blank")]
    agent_subject_id: &'a str,
    #[serde(skip_serializing_if = "blank")]
    tool_key: &'a str,
    #[serde(skip_serializing_if = "blank")]
    action_key: &'a str,
    #[serde(skip_serializing_if = "blank")]
    schema_hash: &'a str,
    #[serde(skip_serializing_if = "blank")]
    approval_id: &'a str,
}

/// Control's approval-gate surface for execution-core: the per-run view (cached at most 100 s), the
/// standing-rule use, and the continuation authority check. Authenticated with this service's
/// own `X-Service-Token`; the request names the run and Control resolves the requester, Space
/// and organisation itself. Absent configuration is a valid disabled state, not an error.
pub(crate) struct ControlGateClient {
    http: Client,
    base_url: Url,
    service_token: String,
    views: Mutex<HashMap<(String, String), (Instant, GateView)>>,
}

impl ControlGateClient {
    fn from_env() -> Result<Option<Self>, String> {
        let (Some(base_url), Some(token)) =
            (env_value(CONTROL_URL_ENV), env_value(CONTROL_TOKEN_ENV))
        else {
            return Ok(None);
        };
        Self::new(&base_url, &token, env_flag(CONTROL_LOOPBACK_ENV)).map(Some)
    }

    fn new(base_url: &str, token: &str, allow_insecure_loopback: bool) -> Result<Self, String> {
        let base_url = service_base_url(base_url, "Control Plane approval gate", allow_insecure_loopback)?;
        let service_token = bounded_secret(token, "Control approval gate service token")?;
        let http = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(HTTP_TIMEOUT)
            .build()
            .map_err(|error| format!("approval gate HTTP client: {error}"))?;
        Ok(Self {
            http,
            base_url,
            service_token,
            views: Mutex::new(HashMap::new()),
        })
    }

    /// The process-wide client. `None` when unconfigured (the gate then behaves as `off` for the
    /// Control inputs) or when the configuration is malformed (logged once, never panics).
    pub(crate) fn shared() -> Option<&'static Self> {
        // Test-only: a test installs a client for its own thread (env mutation is forbidden).
        #[cfg(test)]
        if let Some(client) = tests_support::installed_client() {
            return Some(client);
        }
        static CLIENT: OnceLock<Option<ControlGateClient>> = OnceLock::new();
        CLIENT
            .get_or_init(|| match Self::from_env() {
                Ok(client) => client,
                Err(error) => {
                    tracing::error!(%error, "Space approval gate: Control client configuration is invalid; the gate runs without Control");
                    None
                }
            })
            .as_ref()
    }

    async fn post<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        body: &RunRequest<'_>,
    ) -> Result<T, ControlGateError> {
        let endpoint = service_endpoint(&self.base_url, path).map_err(ControlGateError::Unavailable)?;
        let response = self
            .http
            .post(endpoint)
            .header("X-Service-Id", "execution-core")
            .header("X-Service-Token", &self.service_token)
            .json(body)
            .send()
            .await
            .map_err(|error| ControlGateError::Unavailable(format!("request failed: {error}")))?;
        let status = response.status();
        if status.is_success() {
            return response
                .json::<DataEnvelope<T>>()
                .await
                .map(|envelope| envelope.data)
                .map_err(|error| ControlGateError::Unavailable(format!("response malformed: {error}")));
        }
        if status.is_client_error() && status != StatusCode::REQUEST_TIMEOUT && status != StatusCode::TOO_MANY_REQUESTS {
            // Only the typed code is read from a refusal; nothing else of the body is kept.
            let code = response.json::<JsonValue>().await.ok().and_then(|body| {
                body.pointer("/error/code")
                    .or_else(|| body.get("code"))?
                    .as_str()
                    .map(str::to_owned)
            });
            return Err(ControlGateError::Refused { status: status.as_u16(), code });
        }
        Err(ControlGateError::Unavailable(format!("returned {status}")))
    }

    /// The run's gate view, from the cache while it is younger than 100 s.
    pub(crate) async fn gate_view(&self, org_id: &str, run_id: &str) -> Result<GateView, ControlGateError> {
        let key = (org_id.to_owned(), run_id.to_owned());
        if let Ok(views) = self.views.lock() {
            if let Some((at, view)) = views.get(&key) {
                if at.elapsed() < VIEW_CACHE_MAX {
                    return Ok(view.clone());
                }
            }
        }
        let view: GateView = self
            .post(
                GATE_VIEW_PATH,
                &RunRequest { run_id, org_id, agent_subject_id: "", tool_key: "", action_key: "", schema_hash: "", approval_id: "" },
            )
            .await?;
        if let Ok(mut views) = self.views.lock() {
            if views.len() >= VIEW_CACHE_CAPACITY {
                views.retain(|_, (at, _)| at.elapsed() < VIEW_CACHE_MAX);
                if views.len() >= VIEW_CACHE_CAPACITY {
                    views.clear();
                }
            }
            views.insert(key, (Instant::now(), view.clone()));
        }
        Ok(view)
    }

    /// Ask Control whether a standing rule covers this call. `Ok(None)` is "no applicable rule".
    async fn use_grant(
        &self,
        org_id: &str,
        run_id: &str,
        agent_subject_id: &str,
        tool_key: &str,
        action_key: &str,
        schema_hash: &str,
    ) -> Result<Option<String>, ControlGateError> {
        match self
            .post::<GrantUseResponse>(
                GRANT_USE_PATH,
                &RunRequest { run_id, org_id, agent_subject_id, tool_key, action_key, schema_hash, approval_id: "" },
            )
            .await
        {
            Ok(grant) => Ok(Some(grant.token)),
            Err(ControlGateError::Refused { status: 404, .. }) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// The freshness fence before a worker runs an approved action as the requester. `Ok(false)`
    /// is Control's `authority_changed`.
    pub(crate) async fn continuation_authorized(
        &self,
        org_id: &str,
        run_id: &str,
        approval_id: &str,
    ) -> Result<bool, ControlGateError> {
        #[derive(Deserialize)]
        struct Authorized {
            #[serde(default)]
            authorized: bool,
        }
        match self
            .post::<Authorized>(
                CONTINUATION_CHECK_PATH,
                &RunRequest { run_id, org_id, agent_subject_id: "", tool_key: "", action_key: "", schema_hash: "", approval_id },
            )
            .await
        {
            Ok(answer) => Ok(answer.authorized),
            Err(ControlGateError::Refused { status: 403, code }) if code.as_deref() == Some("authority_changed") => Ok(false),
            Err(error) => Err(error),
        }
    }
}

/// One gated call, as the gate needs to see it.
pub(crate) struct GateCall<'a> {
    pub tool_name: &'a str,
    pub tool_input: &'a str,
    pub org_id: &'a str,
    pub user_id: &'a str,
    pub run_id: &'a str,
    pub step_id: &'a str,
    pub zdr: bool,
    pub waived: bool,
    pub session_channel: Option<&'a Channel>,
    pub session_bearer: Option<&'a str>,
    /// The cause the existing gates would pause under, used when this fold adds none of its own.
    pub fallback_cause: CauseCode,
}

/// What the gate decided for a call that some source asked about.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum GateOutcome {
    /// Run the call: nothing asked, or a spent once-approval / a standing rule passed the prompt.
    Pass,
    /// Pause for a person, under this cause.
    Pause(CauseCode),
    /// Refuse the call: a Control policy row says `block`. The string is a source slug.
    Block(String),
}

/// A plain slug is shown; anything else is not.
fn source_slug(source: &str) -> String {
    let ok = !source.is_empty()
        && source.len() <= 64
        && source.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    if ok { source.to_owned() } else { "policy".to_owned() }
}

/// Whether the call is one a grant or the once-token may never pass: owner-plane effects and
/// consent-to-disclose reads.
fn is_never_auto(tool_name: &str) -> bool {
    crate::permission::requires_durable_owner_approval(tool_name)
        || crate::permission::requires_consent_to_disclose(tool_name)
}

/// Fold the existing gates' ask (`prior_ask`, the cause they would pause under) with Control's
/// level for this call, then resolve an `ask`. Only called for a Space-scoped step in `enforce`.
pub(crate) async fn resolve(call: &GateCall<'_>, prior_ask: Option<CauseCode>) -> GateOutcome {
    let action_key = action_key(call.tool_name, call.tool_input);
    let tool_key = tool_key(call.tool_name);
    let client = ControlGateClient::shared();
    let (view, view_failed) = match client {
        Some(client) => match client.gate_view(call.org_id, call.run_id).await {
            Ok(view) => (Some(view), false),
            Err(error) => {
                tracing::warn!(?error, run_id = call.run_id, "Space approval gate: Control view unavailable");
                metrics::counter!("space_approval_gate_view_unavailable_total").increment(1);
                (None, true)
            }
        },
        None => (None, false),
    };

    let mut levels = Vec::with_capacity(3);
    let mut policy_source = String::new();
    if prior_ask.is_some() {
        levels.push(GateLevel::Ask);
    }
    if let Some(view) = &view {
        let (level, source) = view.level_for(&action_key, tool_key);
        policy_source = source;
        levels.push(level);
    }
    // View unavailable (configured but failing): a risky call asks, with grants disabled; a
    // non-risky call stays as the existing gates decided.
    let grants_available = view.as_ref().is_some_and(|view| view.standing_rules_enabled);
    if view_failed && crate::permission::is_risky_call(call.tool_name, call.tool_input) {
        levels.push(GateLevel::Ask);
    }

    match fold(&levels) {
        GateLevel::Allow => GateOutcome::Pass,
        GateLevel::Block => GateOutcome::Block(source_slug(&policy_source)),
        GateLevel::Ask => {
            let cause = prior_ask.unwrap_or(if view.as_ref().is_some_and(|v| v.level_for(&action_key, tool_key).0 == GateLevel::Ask) {
                CauseCode::CapabilityPolicyAsk
            } else {
                call.fallback_cause
            });
            if is_never_auto(call.tool_name) || call.zdr || call.waived {
                return GateOutcome::Pause(cause);
            }
            if try_once_token(call, tool_key).await {
                metrics::counter!("space_approval_gate_pass_total", "via" => "once").increment(1);
                return GateOutcome::Pass;
            }
            if grants_available && try_standing_rule(call, view.as_ref(), tool_key, &action_key).await {
                metrics::counter!("space_approval_gate_pass_total", "via" => "standing_rule").increment(1);
                return GateOutcome::Pass;
            }
            GateOutcome::Pause(cause)
        }
    }
}

/// Spend the once-approval for this exact action. Only a non-resumable call has one (a resumable
/// call's grant is its continuation, which the worker runs; a retry would run it twice).
async fn try_once_token(call: &GateCall<'_>, tool_key: &str) -> bool {
    if is_resumable_tool(call.tool_name) || tool_key.is_empty() {
        return false;
    }
    let (Some(channel), Some(bearer)) = (call.session_channel, call.session_bearer) else {
        return false;
    };
    let Ok(request) = crate::runtime_loop::authenticated_session_request(
        pb::ClaimOnceApprovalRequest {
            org_id: call.org_id.to_owned(),
            run_id: call.run_id.to_owned(),
            tool_key: tool_key.to_owned(),
            action_digest: action_digest(call.tool_name, call.tool_input),
        },
        bearer,
    ) else {
        return false;
    };
    match pb::orchestration_core_service_client::OrchestrationCoreServiceClient::new(channel.clone())
        .claim_once_approval(request)
        .await
    {
        Ok(response) => response.into_inner().claimed,
        Err(status) => {
            tracing::warn!(code = ?status.code(), run_id = call.run_id, "Space approval gate: once-token claim failed; the call pauses");
            false
        }
    }
}

/// Ask Control for a standing-rule proof and have Session Core record the approval already
/// granted. The call passes only when the stored approval is granted.
async fn try_standing_rule(
    call: &GateCall<'_>,
    view: Option<&GateView>,
    tool_key: &str,
    action_key: &str,
) -> bool {
    if !call.tool_name.starts_with(STANDING_RULE_TOOL_PREFIX) || tool_key.is_empty() {
        return false;
    }
    // The agent whose rule applies comes from Control's view; without it no rule can be named.
    let Some(agent_subject_id) = view.and_then(|view| view.agent_subject_id.as_deref()).filter(|id| !id.is_empty()) else {
        return false;
    };
    let Some(schema_hash) = crate::permission::run_view(call.org_id, call.run_id, call.tool_name)
        .and_then(|view| schema_commitment(view.digest()))
    else {
        return false;
    };
    let (Some(client), Some(channel), Some(bearer)) = (ControlGateClient::shared(), call.session_channel, call.session_bearer) else {
        return false;
    };
    let token = match client
        .use_grant(call.org_id, call.run_id, agent_subject_id, tool_key, action_key, &schema_hash)
        .await
    {
        Ok(Some(token)) => token,
        Ok(None) => return false,
        Err(error) => {
            tracing::warn!(?error, run_id = call.run_id, "Space approval gate: standing-rule use failed; the call pauses");
            return false;
        }
    };
    let Ok(request) = crate::runtime_loop::authenticated_session_request(
        pb::CreateApprovalRequest {
            run_id: call.run_id.to_owned(),
            step_id: call.step_id.to_owned(),
            kind: pb::ApprovalKind::ToolCall as i32,
            requested_of: call.org_id.to_owned(),
            org_id: call.org_id.to_owned(),
            user_id: call.user_id.to_owned(),
            reason: "a standing rule passed this call".to_owned(),
            expires_in_seconds: 3600,
            // The same key the pause path mints under, so a step has one approval.
            idempotency_key: format!("{}:{}", call.run_id, call.step_id),
            context_json: card_context_json(call.tool_name, call.tool_input, RESUME_NONE),
            tool_key: tool_key.to_owned(),
            action_digest: action_digest(call.tool_name, call.tool_input),
            grant_use_token: token,
            ..Default::default()
        },
        bearer,
    ) else {
        return false;
    };
    match pb::orchestration_core_service_client::OrchestrationCoreServiceClient::new(channel.clone())
        .create_approval(request)
        .await
    {
        Ok(response) => response
            .into_inner()
            .approval
            .is_some_and(|approval| approval.state == pb::ApprovalState::Granted as i32),
        Err(status) => {
            tracing::warn!(code = ?status.code(), run_id = call.run_id, "Space approval gate: standing-rule approval refused; the call pauses");
            false
        }
    }
}

/// The Space's quarantine setting from Control's gate view (build plan WS10-S8). `None` when
/// Control is unconfigured or unreachable, which quarantine reads as `off`.
pub(crate) async fn control_quarantine_mode(org_id: &str, run_id: &str) -> Option<String> {
    let client = ControlGateClient::shared()?;
    match client.gate_view(org_id, run_id).await {
        Ok(view) => view.quarantine_mode,
        Err(error) => {
            tracing::warn!(?error, run_id, "Space quarantine: Control view unavailable; quarantine is off for this call");
            None
        }
    }
}

/// Shadow mode: read Control's level for a call every existing gate allowed, and count where the
/// enforced gate would have asked or blocked. Changes nothing.
pub(crate) async fn observe_shadow(call: &GateCall<'_>) {
    let Some(client) = ControlGateClient::shared() else {
        return;
    };
    let Ok(view) = client.gate_view(call.org_id, call.run_id).await else {
        metrics::counter!("space_approval_gate_view_unavailable_total").increment(1);
        return;
    };
    let (level, source) = view.level_for(&action_key(call.tool_name, call.tool_input), tool_key(call.tool_name));
    if level != GateLevel::Allow {
        metrics::counter!("space_approval_gate_shadow_would_differ_total", "level" => if level == GateLevel::Block { "block" } else { "ask" }).increment(1);
        tracing::info!(
            run_id = call.run_id,
            tool = call.tool_name,
            would_be = if level == GateLevel::Block { "block" } else { "ask" },
            source = %source_slug(&source),
            "Space approval gate (shadow): Control would not allow this call"
        );
    }
}

/// Test seam for [`ControlGateClient::shared`]: a per-thread client, so a test can point the gate
/// at a mock Control without touching the process environment.
#[cfg(test)]
pub(crate) mod tests_support {
    use super::ControlGateClient;
    use std::cell::Cell;

    thread_local! {
        static CLIENT: Cell<Option<&'static ControlGateClient>> = const { Cell::new(None) };
    }

    pub(crate) fn installed_client() -> Option<&'static ControlGateClient> {
        CLIENT.with(Cell::get)
    }

    pub(crate) fn install(client: ControlGateClient) {
        CLIENT.with(|slot| slot.set(Some(Box::leak(Box::new(client)))));
    }

    pub(crate) fn uninstall() {
        CLIENT.with(|slot| slot.set(None));
    }

    /// A client for a mock Control at `base_url` (loopback allowed, a throwaway token).
    pub(crate) fn client_for(base_url: &str) -> ControlGateClient {
        ControlGateClient::new(base_url, &"c".repeat(36), true).expect("loopback client")
    }
}

#[cfg(test)]
mod gate_tests {
    use super::*;

    #[test]
    fn fold_takes_the_most_restrictive_level() {
        assert_eq!(fold(&[]), GateLevel::Allow);
        assert_eq!(fold(&[GateLevel::Allow, GateLevel::Ask]), GateLevel::Ask);
        assert_eq!(fold(&[GateLevel::Ask, GateLevel::Block, GateLevel::Allow]), GateLevel::Block);
    }

    #[test]
    fn mode_defaults_to_off() {
        assert_eq!(GateMode::parse(None), GateMode::Off);
        assert_eq!(GateMode::parse(Some("bogus")), GateMode::Off);
        assert_eq!(GateMode::parse(Some(" Enforce ")), GateMode::Enforce);
        assert_eq!(GateMode::parse(Some("shadow")), GateMode::Shadow);
    }

    #[test]
    fn the_digest_ignores_key_order_and_whitespace() {
        let a = action_digest("shell", r#"{"b":1,"a":{"y":2,"x":[1,2]}}"#);
        let b = action_digest("shell", r#"{ "a": {"x":[1,2], "y":2}, "b": 1 }"#);
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert_ne!(a, action_digest("other", r#"{"b":1,"a":{"y":2,"x":[1,2]}}"#));
    }

    #[test]
    fn provider_action_keys_name_the_operation_only() {
        assert_eq!(
            action_key(PROVIDER_ACTION_TOOL, r#"{"operation":"mail.send","body":"secret"}"#),
            "execute_provider_action:mail.send"
        );
        assert_eq!(action_key("shell", "{}"), "shell");
    }

    // -- WS10-S3 postponed tests -------------------------------------------

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tonic::codegen::{http, BoxFuture, Service};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn the_digest_matches_a_golden_vector() {
        // sha256 of the canonical `{"input":{"command":"ls"},"tool":"shell"}`; Session Core
        // and Control hold the same vector, so a change here is a contract change.
        assert_eq!(
            action_digest("shell", r#"{"command":"ls"}"#),
            "8bd087dd9ade255c1c46c5bb3525ad0ebe9d36ea95db30c67c27d593925906b7"
        );
        // Input that is not JSON is digested as the string it is.
        assert_eq!(
            action_digest("shell", "not json"),
            "167aee8ca898d6b2749bf5c0b778b80ab2abe767335b651939d365d3a80966ca"
        );
    }

    #[test]
    fn the_action_key_never_carries_arguments_and_rejects_unsafe_operations() {
        // The key is the operation only, whatever else the input holds.
        let key = action_key(
            PROVIDER_ACTION_TOOL,
            r#"{"operation":"mail.send","args":{"to":"a@example.com"}}"#,
        );
        assert_eq!(key, "execute_provider_action:mail.send");
        assert!(!key.contains("example.com"));
        // An operation that is not a safe identifier, or missing, degrades to the tool name.
        for input in [
            r#"{"operation":"mail send; drop"}"#,
            r#"{"operation":""}"#,
            r#"{"op":"mail.send"}"#,
            "not json",
        ] {
            assert_eq!(action_key(PROVIDER_ACTION_TOOL, input), PROVIDER_ACTION_TOOL, "{input}");
        }
    }

    #[test]
    fn the_lattice_is_allow_below_ask_below_block() {
        assert!(GateLevel::Allow < GateLevel::Ask && GateLevel::Ask < GateLevel::Block);
        assert_eq!(fold(&[GateLevel::Block, GateLevel::Allow]), GateLevel::Block);
        assert_eq!(fold(&[GateLevel::Allow, GateLevel::Allow]), GateLevel::Allow);
        assert_eq!(GateLevel::from_wire("ask"), Some(GateLevel::Ask));
        assert_eq!(GateLevel::from_wire("ALLOW"), None);
        assert_eq!(GateLevel::from_wire(""), None);
    }

    #[test]
    fn a_view_answers_specific_key_then_tool_then_star_and_defaults_to_allow() {
        let view: GateView = serde_json::from_value(serde_json::json!({
            "tools": {
                "execute_provider_action:mail.send": {"level": "block", "source": "org_policy"},
                "execute_provider_action": {"level": "ask", "source": "space_policy"},
                "*": {"level": "ask", "source": "default"},
                "bogus": {"level": "maybe", "source": "x"}
            }
        }))
        .expect("view parses");
        assert_eq!(
            view.level_for("execute_provider_action:mail.send", "execute_provider_action"),
            (GateLevel::Block, "org_policy".to_owned())
        );
        assert_eq!(
            view.level_for("execute_provider_action:cal.read", "execute_provider_action"),
            (GateLevel::Ask, "space_policy".to_owned())
        );
        assert_eq!(view.level_for("shell", "shell"), (GateLevel::Ask, "default".to_owned()));
        // An unrecognised level is skipped, not read as allow-by-policy for that row only.
        assert_eq!(view.level_for("bogus", "bogus").0, GateLevel::Ask);
        let empty: GateView = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(empty.level_for("shell", "shell"), (GateLevel::Allow, String::new()));
        assert!(!empty.standing_rules_enabled);
        assert!(empty.agent_subject_id.is_none() && empty.quarantine_mode.is_none());
    }

    #[test]
    fn the_source_slug_shows_plain_slugs_only() {
        assert_eq!(source_slug("org_policy"), "org_policy");
        for hidden in ["", "Org Policy", "has-dash", &"a".repeat(65), "x\ny"] {
            assert_eq!(source_slug(hidden), "policy", "{hidden:?}");
        }
    }

    #[test]
    fn a_schema_commitment_needs_a_full_lowercase_sha256() {
        let hex = "a".repeat(64);
        assert_eq!(schema_commitment(&hex), Some(format!("sha256:{hex}")));
        assert_eq!(schema_commitment(&format!(" sha256:{hex} ")), Some(format!("sha256:{hex}")));
        assert_eq!(schema_commitment(&"A".repeat(64)), None);
        assert_eq!(schema_commitment(&"a".repeat(63)), None);
        assert_eq!(schema_commitment(""), None);
    }

    // ---- A mock Control and a mock Session Core -------------------------------------------

    const TEST_TOKEN: &str = "cccccccccccccccccccccccccccccccccccc";

    fn control_for(server: &MockServer) -> ControlGateClient {
        ControlGateClient::new(&server.uri(), TEST_TOKEN, true).expect("loopback client")
    }

    async fn serve_view(server: &MockServer, view: serde_json::Value) {
        Mock::given(method("POST"))
            .and(path(GATE_VIEW_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "data": view })))
            .mount(server)
            .await;
    }

    async fn requests_to(server: &MockServer, request_path: &str) -> usize {
        server
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .filter(|request| request.url.path() == request_path)
            .count()
    }

    /// Answers `ClaimOnceApproval` (counting calls) and refuses every other method.
    #[derive(Clone)]
    struct MockSessionCore {
        claims: Arc<AtomicUsize>,
        claimed: bool,
    }

    impl tonic::server::NamedService for MockSessionCore {
        const NAME: &'static str = "model_plane.v1.OrchestrationCoreService";
    }

    impl Service<http::Request<tonic::body::BoxBody>> for MockSessionCore {
        type Response = http::Response<tonic::body::BoxBody>;
        type Error = std::convert::Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;

        fn poll_ready(
            &mut self,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }

        fn call(&mut self, request: http::Request<tonic::body::BoxBody>) -> Self::Future {
            let this = self.clone();
            if request.uri().path() != "/model_plane.v1.OrchestrationCoreService/ClaimOnceApproval" {
                return Box::pin(async move {
                    let mut response = http::Response::new(tonic::body::empty_body());
                    let headers = response.headers_mut();
                    headers.insert("grpc-status", (tonic::Code::Unimplemented as i32).to_string().parse().unwrap());
                    headers.insert("content-type", "application/grpc".parse().unwrap());
                    Ok(response)
                });
            }
            struct ClaimSvc(MockSessionCore);
            impl tonic::server::UnaryService<pb::ClaimOnceApprovalRequest> for ClaimSvc {
                type Response = pb::ClaimOnceApprovalResponse;
                type Future = BoxFuture<tonic::Response<Self::Response>, tonic::Status>;
                fn call(
                    &mut self,
                    request: tonic::Request<pb::ClaimOnceApprovalRequest>,
                ) -> Self::Future {
                    let this = self.0.clone();
                    Box::pin(async move {
                        let body = request.into_inner();
                        // The claim binds the run, the tool and the exact action digest.
                        assert!(!body.run_id.is_empty() && !body.tool_key.is_empty());
                        assert_eq!(body.action_digest.len(), 64);
                        this.claims.fetch_add(1, Ordering::SeqCst);
                        Ok(tonic::Response::new(pb::ClaimOnceApprovalResponse {
                            claimed: this.claimed,
                            ..Default::default()
                        }))
                    })
                }
            }
            Box::pin(async move {
                let mut grpc = tonic::server::Grpc::new(tonic::codec::ProstCodec::default());
                Ok(grpc.unary(ClaimSvc(this), request).await)
            })
        }
    }

    async fn session_core(claimed: bool) -> (Channel, Arc<AtomicUsize>) {
        let claims = Arc::new(AtomicUsize::new(0));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let service = MockSessionCore {
            claims: Arc::clone(&claims),
            claimed,
        };
        tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(service)
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await
                .ok();
        });
        let channel = tonic::transport::Endpoint::from_shared(format!("http://{addr}"))
            .unwrap()
            .connect()
            .await
            .expect("connect to the mock Session Core");
        (channel, claims)
    }

    fn call<'a>(tool: &'a str, input: &'a str, channel: Option<&'a Channel>) -> GateCall<'a> {
        GateCall {
            tool_name: tool,
            tool_input: input,
            org_id: "org-1",
            user_id: "user-1",
            run_id: "run-1",
            step_id: "step-1",
            zdr: false,
            waived: false,
            session_channel: channel,
            session_bearer: channel.map(|_| "bearer-bbbbbbbb"),
            fallback_cause: CauseCode::RiskyEffectUnderAsk,
        }
    }

    // ---- Control client over HTTP ----------------------------------------------------------

    #[tokio::test]
    async fn the_client_reads_a_view_once_and_serves_the_second_read_from_cache() {
        let server = MockServer::start().await;
        serve_view(&server, serde_json::json!({"tools": {"shell": {"level": "ask", "source": "org_policy"}}})).await;
        let client = control_for(&server);
        let first = client.gate_view("org-1", "run-1").await.expect("view");
        assert_eq!(first.level_for("shell", "shell").0, GateLevel::Ask);
        client.gate_view("org-1", "run-1").await.expect("cached view");
        assert_eq!(requests_to(&server, GATE_VIEW_PATH).await, 1, "second read must hit the cache");
        client.gate_view("org-1", "run-2").await.expect("another run");
        assert_eq!(requests_to(&server, GATE_VIEW_PATH).await, 2);
    }

    #[tokio::test]
    async fn the_client_sends_the_service_headers_and_no_arguments() {
        let server = MockServer::start().await;
        serve_view(&server, serde_json::json!({})).await;
        control_for(&server).gate_view("org-1", "run-1").await.expect("view");
        let requests = server.received_requests().await.unwrap();
        let request = &requests[0];
        assert_eq!(request.headers.get("x-service-id").unwrap(), "execution-core");
        assert_eq!(request.headers.get("x-service-token").unwrap(), TEST_TOKEN);
        let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(body["run_id"], "run-1");
        assert_eq!(body["org_id"], "org-1");
    }

    #[tokio::test]
    async fn a_5xx_or_malformed_view_is_unavailable_and_a_4xx_is_a_typed_refusal() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path(GATE_VIEW_PATH))
            .respond_with(ResponseTemplate::new(503)).mount(&server).await;
        assert!(matches!(
            control_for(&server).gate_view("o", "r").await,
            Err(ControlGateError::Unavailable(_))
        ));

        let refused = MockServer::start().await;
        Mock::given(method("POST")).and(path(GATE_VIEW_PATH))
            .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({"error": {"code": "forbidden", "message": "x"}})))
            .mount(&refused).await;
        assert!(matches!(
            control_for(&refused).gate_view("o", "r").await,
            Err(ControlGateError::Refused { status: 403, code: Some(ref code) }) if code == "forbidden"
        ));

        let malformed = MockServer::start().await;
        Mock::given(method("POST")).and(path(GATE_VIEW_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json")).mount(&malformed).await;
        assert!(matches!(
            control_for(&malformed).gate_view("o", "r").await,
            Err(ControlGateError::Unavailable(_))
        ));
    }

    #[tokio::test]
    async fn a_grant_use_is_none_on_404_and_the_token_on_200() {
        let none = MockServer::start().await;
        Mock::given(method("POST")).and(path(GRANT_USE_PATH))
            .respond_with(ResponseTemplate::new(404)).mount(&none).await;
        assert!(matches!(
            control_for(&none).use_grant("o", "r", "agent-1", "mcp__s__t", "mcp__s__t", "sha256:x").await,
            Ok(None)
        ));
        let some = MockServer::start().await;
        Mock::given(method("POST")).and(path(GRANT_USE_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"data": {"token": "proof-token"}})))
            .mount(&some).await;
        assert!(matches!(
            control_for(&some).use_grant("o", "r", "agent-1", "mcp__s__t", "mcp__s__t", "sha256:x").await,
            Ok(Some(ref token)) if token == "proof-token"
        ));
    }

    #[tokio::test]
    async fn the_continuation_check_reads_authority_changed_as_false() {
        let changed = MockServer::start().await;
        Mock::given(method("POST")).and(path(CONTINUATION_CHECK_PATH))
            .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({"error": {"code": "authority_changed"}})))
            .mount(&changed).await;
        assert!(matches!(control_for(&changed).continuation_authorized("o", "r", "a").await, Ok(false)));

        let other = MockServer::start().await;
        Mock::given(method("POST")).and(path(CONTINUATION_CHECK_PATH))
            .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({"error": {"code": "forbidden"}})))
            .mount(&other).await;
        assert!(matches!(
            control_for(&other).continuation_authorized("o", "r", "a").await,
            Err(ControlGateError::Refused { status: 403, .. })
        ));

        let ok = MockServer::start().await;
        Mock::given(method("POST")).and(path(CONTINUATION_CHECK_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"data": {"authorized": true}})))
            .mount(&ok).await;
        assert!(matches!(control_for(&ok).continuation_authorized("o", "r", "a").await, Ok(true)));
    }

    // ---- resolve ---------------------------------------------------------------------------

    const MCP_TOOL: &str = "mcp__srv__do_thing";

    #[tokio::test]
    async fn a_never_auto_tool_pauses_even_with_a_once_approval_and_a_rule_present() {
        let server = MockServer::start().await;
        serve_view(&server, serde_json::json!({
            "tools": {}, "standing_rules_enabled": true, "agent_subject_id": "agent-1"
        })).await;
        let (channel, claims) = session_core(true).await;
        tests_support::install(control_for(&server));
        let disclose = crate::runtime_loop::subagent_results::READ_TOOL;
        assert!(is_never_auto(disclose));
        let outcome = resolve(&call(disclose, "{}", Some(&channel)), Some(CauseCode::ConsentToDisclose)).await;
        tests_support::uninstall();
        assert_eq!(outcome, GateOutcome::Pause(CauseCode::ConsentToDisclose));
        assert_eq!(claims.load(Ordering::SeqCst), 0, "a never-auto call must not claim the once-token");
        assert_eq!(requests_to(&server, GRANT_USE_PATH).await, 0, "nor ask for a rule");
    }

    #[tokio::test]
    async fn block_beats_a_once_token_and_a_rule() {
        let server = MockServer::start().await;
        serve_view(&server, serde_json::json!({
            "tools": {MCP_TOOL: {"level": "block", "source": "org_policy"}},
            "standing_rules_enabled": true, "agent_subject_id": "agent-1"
        })).await;
        let (channel, claims) = session_core(true).await;
        tests_support::install(control_for(&server));
        let outcome = resolve(&call(MCP_TOOL, "{}", Some(&channel)), Some(CauseCode::RiskyEffectUnderAsk)).await;
        tests_support::uninstall();
        assert_eq!(outcome, GateOutcome::Block("org_policy".to_owned()));
        assert_eq!(claims.load(Ordering::SeqCst), 0);
        assert_eq!(requests_to(&server, GRANT_USE_PATH).await, 0);
    }

    #[tokio::test]
    async fn an_unavailable_view_makes_a_risky_call_ask_with_grants_disabled() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path(GATE_VIEW_PATH))
            .respond_with(ResponseTemplate::new(500)).mount(&server).await;
        tests_support::install(control_for(&server));
        // `mcp__` tools are risky by name; no prior ask from the existing gates.
        assert!(crate::permission::is_risky_call(MCP_TOOL, "{}"));
        // Never-auto forced off for the grant half: no channel either, so only the pause is left.
        let outcome = resolve(&call(MCP_TOOL, "{}", None), None).await;
        tests_support::uninstall();
        assert_eq!(outcome, GateOutcome::Pause(CauseCode::RiskyEffectUnderAsk));
        assert_eq!(requests_to(&server, GRANT_USE_PATH).await, 0, "grants are disabled while the view is down");
    }

    #[tokio::test]
    async fn an_unavailable_view_leaves_a_non_risky_call_as_the_existing_gates_decided() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path(GATE_VIEW_PATH))
            .respond_with(ResponseTemplate::new(500)).mount(&server).await;
        tests_support::install(control_for(&server));
        assert!(!crate::permission::is_risky_call("web_search", "{}"));
        let outcome = resolve(&call("web_search", "{}", None), None).await;
        tests_support::uninstall();
        assert_eq!(outcome, GateOutcome::Pass);
    }

    #[tokio::test]
    async fn zdr_and_waived_calls_never_claim_or_use_a_grant() {
        for (zdr, waived) in [(true, false), (false, true), (true, true)] {
            let server = MockServer::start().await;
            serve_view(&server, serde_json::json!({
                "tools": {MCP_TOOL: {"level": "ask", "source": "space_policy"}},
                "standing_rules_enabled": true, "agent_subject_id": "agent-1"
            })).await;
            let (channel, claims) = session_core(true).await;
            tests_support::install(control_for(&server));
            let mut gated = call(MCP_TOOL, "{}", Some(&channel));
            gated.zdr = zdr;
            gated.waived = waived;
            let outcome = resolve(&gated, None).await;
            tests_support::uninstall();
            assert_eq!(outcome, GateOutcome::Pause(CauseCode::CapabilityPolicyAsk), "zdr={zdr} waived={waived}");
            assert_eq!(claims.load(Ordering::SeqCst), 0, "zdr={zdr} waived={waived}");
            assert_eq!(requests_to(&server, GRANT_USE_PATH).await, 0, "zdr={zdr} waived={waived}");
        }
    }

    #[tokio::test]
    async fn a_non_mcp_tool_never_reaches_the_standing_rule_use() {
        let server = MockServer::start().await;
        serve_view(&server, serde_json::json!({
            "tools": {"shell": {"level": "ask", "source": "space_policy"}},
            "standing_rules_enabled": true, "agent_subject_id": "agent-1"
        })).await;
        // The once-approval is not there to claim, so the call falls through to the rule step.
        let (channel, claims) = session_core(false).await;
        tests_support::install(control_for(&server));
        let outcome = resolve(&call("shell", "{}", Some(&channel)), None).await;
        tests_support::uninstall();
        assert_eq!(outcome, GateOutcome::Pause(CauseCode::CapabilityPolicyAsk));
        assert_eq!(claims.load(Ordering::SeqCst), 1, "the once-token is tried");
        assert_eq!(requests_to(&server, GRANT_USE_PATH).await, 0, "a non-mcp tool never asks Control for a rule");
    }

    #[tokio::test]
    async fn a_claimed_once_approval_passes_a_non_resumable_ask() {
        let server = MockServer::start().await;
        serve_view(&server, serde_json::json!({
            "tools": {"shell": {"level": "ask", "source": "space_policy"}}
        })).await;
        let (channel, claims) = session_core(true).await;
        tests_support::install(control_for(&server));
        let outcome = resolve(&call("shell", r#"{"program":"ls"}"#, Some(&channel)), None).await;
        tests_support::uninstall();
        assert_eq!(outcome, GateOutcome::Pass);
        assert_eq!(claims.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn without_a_control_client_only_the_existing_gates_ask() {
        // No client installed and no environment: the gate behaves as the existing gates decided.
        assert!(tests_support::installed_client().is_none());
        if ControlGateClient::shared().is_some() {
            return;
        }
        assert_eq!(resolve(&call("shell", "{}", None), None).await, GateOutcome::Pass);
        assert_eq!(
            resolve(&call("shell", "{}", None), Some(CauseCode::ProviderWrite)).await,
            GateOutcome::Pause(CauseCode::ProviderWrite)
        );
    }
}
