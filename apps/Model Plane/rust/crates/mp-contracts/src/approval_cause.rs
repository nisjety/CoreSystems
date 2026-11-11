//! Why an approval was asked for: the closed cause vocabulary (build plan WS13 S1).
//!
//! Every pause for a human decision used to say the same thing, "tool 'X' requires
//! approval", whichever rule raised it. A person deciding, a later audit row and a
//! standing grant all need the rule, so each creator now states one of a closed set
//! of codes, and session-core stores it beside the approval.
//!
//! The set is closed on purpose. [`CauseCode::from_wire`] sends every value outside
//! it to [`CauseCode::UnclassifiedGate`], never to a nearer-looking code, so a creator
//! that is newer than the reader degrades to the least specific honest answer. The
//! file `api/contracts/approval-causes.json` is the same set written out as data, and
//! `tests/approval_cause_golden.rs` pins the two to each other.
//!
//! A cause carries no argument text. `action_key` names the tool and, for the one tool
//! whose risk depends on it, the operation. `call_digest` identifies the exact call
//! under a key derived from the run, so the same arguments in another run have another
//! digest and nothing can be recovered from one.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::loop_fold::switch_from;
use crate::model_plane::v1::{Approval, ApprovalCause, ApprovalState};
use crate::tool_class::SideEffectClass;

/// The `execute_provider_action` tool, the one whose key carries its operation.
const PROVIDER_ACTION_TOOL: &str = "execute_provider_action";
/// The only scope any cause offers until grants exist (WS13 S6).
pub const SCOPE_ONCE: &str = "once";
/// Longest string any one cause field keeps.
pub const MAX_FIELD_CHARS: usize = 200;
/// Longest operation name an `action_key` will carry.
const MAX_OPERATION_CHARS: usize = 64;
/// Domain separation for the run-salted digest key.
const DIGEST_KEY_CONTEXT: &str = "model-plane approval call digest v1";

/// Why a pause was raised. Closed: see the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CauseCode {
    /// An owner-plane effect, always a durable human approval.
    OwnerEffectReserved,
    /// A read whose approval is about disclosure, not danger.
    ConsentToDisclose,
    /// A risky tool call held by the `ask` posture.
    RiskyEffectUnderAsk,
    /// A provider write held by the `ask` posture.
    ProviderWrite,
    /// The capability registry asked: the capability is high-risk.
    CapabilityHighRisk,
    /// A tool policy rule asked. Reserved for WS05; nothing raises it yet.
    CapabilityPolicyAsk,
    /// A pre-tool hook escalated the call to a person.
    HookAsk,
    /// A browser action the Model proposed, held by its risk category.
    BrowserActionRisk,
    /// A provider write the chat posture approved on the person's behalf.
    AutoPostureProviderWrite,
    /// The person holds the run's browser, or the agent asked them for help in it (EP-1). The
    /// approval is the hand-back: consuming it lets the held browser call run again.
    BrowserHumanAssist,
    /// An egress call (a web fetch or a provider action) held for a person after a connected-app
    /// read came back flagged by screening (WS23-S3). Only `once` is offered.
    TaintedEgress,
    /// A gate that did not say why, or a code this reader does not know.
    UnclassifiedGate,
}

impl CauseCode {
    /// Every code, in the order the golden lists them.
    pub const ALL: [Self; 12] = [
        Self::OwnerEffectReserved,
        Self::ConsentToDisclose,
        Self::RiskyEffectUnderAsk,
        Self::ProviderWrite,
        Self::CapabilityHighRisk,
        Self::CapabilityPolicyAsk,
        Self::HookAsk,
        Self::BrowserActionRisk,
        Self::AutoPostureProviderWrite,
        Self::BrowserHumanAssist,
        Self::TaintedEgress,
        Self::UnclassifiedGate,
    ];

    /// The wire and storage token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OwnerEffectReserved => "owner_effect_reserved",
            Self::ConsentToDisclose => "consent_to_disclose",
            Self::RiskyEffectUnderAsk => "risky_effect_under_ask",
            Self::ProviderWrite => "provider_write",
            Self::CapabilityHighRisk => "capability_high_risk",
            Self::CapabilityPolicyAsk => "capability_policy_ask",
            Self::HookAsk => "hook_ask",
            Self::BrowserActionRisk => "browser_action_risk",
            Self::AutoPostureProviderWrite => "auto_posture_provider_write",
            Self::BrowserHumanAssist => "browser_human_assist",
            Self::TaintedEgress => "tainted_egress",
            Self::UnclassifiedGate => "unclassified_gate",
        }
    }

    /// Parse a wire or stored token. Exact match only; an empty, padded, differently
    /// cased or unknown value is [`Self::UnclassifiedGate`].
    #[must_use]
    pub fn from_wire(token: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|code| code.as_str() == token)
            .unwrap_or(Self::UnclassifiedGate)
    }

    /// Whether `token` is exactly one of the codes. Distinguishes a creator that
    /// said `unclassified_gate` from one that said something this reader lacks.
    #[must_use]
    pub fn is_known(token: &str) -> bool {
        Self::ALL.into_iter().any(|code| code.as_str() == token)
    }
}

/// The scopes a code offers on its own: `once`. A call's own facts can widen them, see
/// [`eligible_scopes`]; this is the answer for a cause that carries no facts.
#[must_use]
pub fn scope_options_for(_code: CauseCode) -> &'static [&'static str] {
    &[SCOPE_ONCE]
}

/// A standing grant covers its call for the rest of the thread (build plan WS13 S6).
pub const SCOPE_THREAD: &str = "thread";
/// A standing grant covers its call for the rest of the run.
pub const SCOPE_RUN: &str = "run";
/// The prefix every MCP tool name carries.
const MCP_TOOL_PREFIX: &str = "mcp__";
/// Tools whose writes are bound to their own single-use receipt, never to a grant.
const PROVIDER_WRITE_TOOLS: [&str; 3] =
    [PROVIDER_ACTION_TOOL, "book_shipment", "publish_social_post"];

/// The scopes a decider may grant for a pause, from its code and the call's facts (build plan
/// WS13 S6, D4 and K3-3). Closed: `thread` and `run` are offered only where the pause is a
/// consent, risky-call or capability gate and the call's declared class is a read, a sandbox
/// computation or a context write. An external write is offered the same only for a pinned MCP
/// tool, the one kind whose definition a person reviewed, and never under the high-risk capability
/// gate. Every other pause is `once`: provider writes, owner effects, browser actions, destructive
/// and unclassified effects, unpinned MCP tools, and the shell and process tools. A cause that
/// names no class, tool or key is `once`.
#[must_use]
pub fn eligible_scopes(cause: &ApprovalCause) -> Vec<String> {
    let code = CauseCode::from_wire(&cause.code);
    let widening_code = matches!(
        code,
        CauseCode::RiskyEffectUnderAsk
            | CauseCode::ConsentToDisclose
            | CauseCode::CapabilityHighRisk
    );
    let class = SideEffectClass::from_wire(&cause.effect_class);
    let pinned_external_write = class == SideEffectClass::ExternalWrite
        && code != CauseCode::CapabilityHighRisk
        && is_pinned_mcp(cause);
    let standing_class = matches!(
        class,
        SideEffectClass::Read | SideEffectClass::SandboxCompute | SideEffectClass::ContextWrite
    ) || pinned_external_write;
    let keyed = !cause.tool.trim().is_empty()
        && !cause.action_key.is_empty()
        && !is_standing_excluded(cause);
    if widening_code && standing_class && keyed {
        vec![
            SCOPE_ONCE.to_owned(),
            SCOPE_THREAD.to_owned(),
            SCOPE_RUN.to_owned(),
        ]
    } else {
        vec![SCOPE_ONCE.to_owned()]
    }
}

/// Whether a cause's tool is an MCP tool whose definition is pinned: `tool_digest` is the sha256
/// of the definition a person reviewed (build plan WS05 S5, WS13 S7). An unpinned MCP tool has no
/// digest to bind a rule or a grant to, so it is never offered a standing scope or a rule.
#[must_use]
pub fn is_pinned_mcp(cause: &ApprovalCause) -> bool {
    cause.tool.trim().starts_with(MCP_TOOL_PREFIX) && is_sha256_hex(&cause.tool_digest)
}

/// Whether a cause may become a standing rule (build plan WS13 S7): it must offer a thread grant,
/// name a pinned MCP tool, and declare a class a person may relax, which is at most an external
/// write (capability-core's `CanRelax` for a person). A built-in tool is never offered one.
#[must_use]
pub fn rule_offered(cause: &ApprovalCause) -> bool {
    cause
        .scope_options
        .iter()
        .any(|scope| scope == SCOPE_THREAD)
        && is_pinned_mcp(cause)
        && SideEffectClass::from_wire(&cause.effect_class) <= SideEffectClass::ExternalWrite
}

/// A sha256 digest as the registry writes it: 64 lowercase hex characters.
fn is_sha256_hex(candidate: &str) -> bool {
    candidate.len() == 64
        && candidate
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// Tools no standing grant may cover, whatever their class. An MCP tool is covered only while its
/// definition is pinned.
fn is_standing_excluded(cause: &ApprovalCause) -> bool {
    let tool = cause.tool.trim();
    if tool.starts_with(MCP_TOOL_PREFIX) {
        return !is_pinned_mcp(cause);
    }
    tool == "shell" || tool.starts_with("process_") || PROVIDER_WRITE_TOOLS.contains(&tool)
}

/// The key a standing grant would match: the tool, plus the operation for the one tool
/// whose risk depends on it. Never an argument value, so a name like `message.send` can
/// appear and a recipient or a body cannot.
#[must_use]
pub fn action_key(tool_name: &str, tool_input: &str) -> String {
    let tool = tool_name.trim();
    if tool != PROVIDER_ACTION_TOOL {
        return tool.to_owned();
    }
    let operation = serde_json::from_str::<Value>(tool_input)
        .ok()
        .and_then(|input| {
            input
                .get("operation")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .filter(|op| is_operation_name(op));
    match operation {
        Some(op) => format!("{tool}:{op}"),
        None => tool.to_owned(),
    }
}

/// A catalogued operation name looks like `message.send`; anything else is argument
/// text and is not echoed into a key.
fn is_operation_name(candidate: &str) -> bool {
    !candidate.is_empty()
        && candidate.chars().count() <= MAX_OPERATION_CHARS
        && candidate
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
}

/// A blake3 digest of the exact call under a key derived from `run_id`.
///
/// Arguments are canonicalised first (object keys sorted, whitespace dropped), so the
/// same call written two ways has one digest. The tool and the arguments are length
/// prefixed, so no pair of different calls shares the bytes that are hashed. The key
/// is the run's, which is why a digest from one run says nothing in another.
#[must_use]
pub fn call_digest(run_id: &str, tool_name: &str, tool_input: &str) -> String {
    let key = blake3::derive_key(DIGEST_KEY_CONTEXT, run_id.as_bytes());
    let arguments = canonical_arguments(tool_input);
    let tool = tool_name.trim();
    let mut hasher = blake3::Hasher::new_keyed(&key);
    hasher.update(&(tool.len() as u64).to_le_bytes());
    hasher.update(tool.as_bytes());
    hasher.update(&(arguments.len() as u64).to_le_bytes());
    hasher.update(arguments.as_bytes());
    hasher.finalize().to_hex().to_string()
}

/// The arguments with object keys sorted at every depth. Text that is not JSON is
/// hashed as written (trimmed), so a malformed call still has a stable digest.
fn canonical_arguments(tool_input: &str) -> String {
    let trimmed = tool_input.trim();
    match serde_json::from_str::<Value>(trimmed) {
        Ok(value) => sorted(&value).to_string(),
        Err(_) => trimmed.to_owned(),
    }
}

fn sorted(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), sorted(v)))
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
        other => other.clone(),
    }
}

/// The cause of a pause on a tool call: the code, the tool, its action key and the
/// run-salted digest of the call. The scope options are the code's.
#[must_use]
pub fn for_call(code: CauseCode, run_id: &str, tool_name: &str, tool_input: &str) -> ApprovalCause {
    for_call_with_class(code, run_id, tool_name, tool_input, "")
}

/// [`for_call`] with the call's declared effect class, which the scope eligibility reads. An
/// empty class, or one outside the vocabulary, makes the cause `once` only.
#[must_use]
pub fn for_call_with_class(
    code: CauseCode,
    run_id: &str,
    tool_name: &str,
    tool_input: &str,
    effect_class: &str,
) -> ApprovalCause {
    for_call_with_tool(code, run_id, tool_name, tool_input, effect_class, "")
}

/// [`for_call_with_class`] for a call whose tool definition is known: `tool_digest` is the sha256
/// of the MCP definition the person reviewed, and empty for any other call (build plan WS13 S7).
#[must_use]
pub fn for_call_with_tool(
    code: CauseCode,
    run_id: &str,
    tool_name: &str,
    tool_input: &str,
    effect_class: &str,
    tool_digest: &str,
) -> ApprovalCause {
    let mut cause = ApprovalCause {
        code: code.as_str().to_owned(),
        tool: tool_name.trim().to_owned(),
        effect_class: effect_class.to_owned(),
        action_key: action_key(tool_name, tool_input),
        call_digest: call_digest(run_id, tool_name, tool_input),
        tool_digest: tool_digest.to_owned(),
        ..ApprovalCause::default()
    };
    cause.scope_options = eligible_scopes(&cause);
    cause
}

/// A cause that names only its code, for a gate with no tool call to describe.
#[must_use]
pub fn for_code(code: CauseCode) -> ApprovalCause {
    let mut cause = ApprovalCause {
        code: code.as_str().to_owned(),
        ..ApprovalCause::default()
    };
    cause.scope_options = eligible_scopes(&cause);
    cause
}

/// What a store may keep of a cause a caller sent: an unknown code becomes
/// `unclassified_gate`, the scope options are the code's own (a caller cannot widen
/// them), and every string is bounded. Returns the normalised cause and whether the
/// code was one this reader knows.
#[must_use]
pub fn normalized(mut cause: ApprovalCause) -> (ApprovalCause, bool) {
    let known = CauseCode::is_known(&cause.code);
    let code = CauseCode::from_wire(&cause.code);
    cause.code = code.as_str().to_owned();
    for field in [
        &mut cause.tool,
        &mut cause.effect_class,
        &mut cause.rule_id,
        &mut cause.risk_category,
        &mut cause.action_key,
        &mut cause.call_digest,
    ] {
        if field.chars().count() > MAX_FIELD_CHARS {
            *field = field.chars().take(MAX_FIELD_CHARS).collect();
        }
    }
    // A digest that is not sha256 hex pins nothing: the tool reads as unpinned.
    if !is_sha256_hex(&cause.tool_digest) {
        cause.tool_digest.clear();
    }
    keep_browser_assist_facts(&mut cause, code);
    cause.scope_options = eligible_scopes(&cause);
    (cause, known)
}

/// `assist_kind` of a pause raised because the person took the browser (EP-1).
pub const ASSIST_KIND_USER_TAKEOVER: &str = "user_takeover";
/// Every `assist_kind` a `browser_human_assist` pause may state: the takeover, and the challenges
/// the agent may ask help with (EP-1 D4).
pub const ASSIST_KINDS: [&str; 5] =
    [ASSIST_KIND_USER_TAKEOVER, "login", "captcha", "consent", "waf"];
/// `assist_origin` of a pause the person started by taking the browser.
pub const ASSIST_ORIGIN_PERSON: &str = "person";
/// `assist_origin` of a pause the agent raised to ask for help.
pub const ASSIST_ORIGIN_AGENT: &str = "agent";

/// The cause of a pause on a browser call the person holds or was asked to help with (EP-1): the
/// call's cause under `browser_human_assist`, with the closed kind and origin and the session's
/// host. A word outside the closed sets is dropped, never stored.
#[must_use]
pub fn for_browser_assist(
    run_id: &str,
    tool_name: &str,
    tool_input: &str,
    assist_kind: &str,
    assist_origin: &str,
    host: &str,
) -> ApprovalCause {
    let cause = ApprovalCause {
        assist_kind: assist_kind.to_owned(),
        assist_origin: assist_origin.to_owned(),
        host: host.to_owned(),
        ..for_call(CauseCode::BrowserHumanAssist, run_id, tool_name, tool_input)
    };
    normalized(cause).0
}

/// The browser-assist facts a cause may keep: only a `browser_human_assist` cause states them, only
/// closed words for the kind and origin, and a bounded host. Any other cause keeps none, so a
/// creator cannot attach a host to a pause whose card does not show one.
fn keep_browser_assist_facts(cause: &mut ApprovalCause, code: CauseCode) {
    if code != CauseCode::BrowserHumanAssist {
        cause.assist_kind.clear();
        cause.assist_origin.clear();
        cause.host.clear();
        return;
    }
    if !ASSIST_KINDS.contains(&cause.assist_kind.as_str()) {
        cause.assist_kind.clear();
    }
    if ![ASSIST_ORIGIN_PERSON, ASSIST_ORIGIN_AGENT].contains(&cause.assist_origin.as_str()) {
        cause.assist_origin.clear();
    }
    if cause.host.chars().count() > MAX_FIELD_CHARS {
        cause.host = cause.host.chars().take(MAX_FIELD_CHARS).collect();
    }
}

/// The stored shape, `approvals.metadata.cause`. Empty strings and an unset
/// `step_up_required` are left out, so a row carries only what the creator said.
#[must_use]
pub fn to_metadata(cause: &ApprovalCause) -> Value {
    let mut map = serde_json::Map::new();
    map.insert("code".to_owned(), Value::String(cause.code.clone()));
    for (key, value) in [
        ("tool", &cause.tool),
        ("effect_class", &cause.effect_class),
        ("rule_id", &cause.rule_id),
        ("risk_category", &cause.risk_category),
        ("action_key", &cause.action_key),
        ("call_digest", &cause.call_digest),
        ("tool_digest", &cause.tool_digest),
        ("assist_kind", &cause.assist_kind),
        ("assist_origin", &cause.assist_origin),
        ("host", &cause.host),
    ] {
        if !value.is_empty() {
            map.insert(key.to_owned(), Value::String(value.clone()));
        }
    }
    map.insert("scope_options".to_owned(), json!(cause.scope_options));
    if cause.step_up_required {
        map.insert("step_up_required".to_owned(), Value::Bool(true));
    }
    Value::Object(map)
}

/// Read a cause back out of `approvals.metadata`. `None` when the row has none; a
/// stored code this reader does not know reads as `unclassified_gate`.
#[must_use]
pub fn from_metadata(metadata: &Value) -> Option<ApprovalCause> {
    let stored = metadata.get("cause")?.as_object()?;
    let text = |key: &str| {
        stored
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let cause = ApprovalCause {
        code: text("code"),
        tool: text("tool"),
        effect_class: text("effect_class"),
        rule_id: text("rule_id"),
        risk_category: text("risk_category"),
        scope_options: stored
            .get("scope_options")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        step_up_required: stored
            .get("step_up_required")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        action_key: text("action_key"),
        call_digest: text("call_digest"),
        tool_digest: text("tool_digest"),
        // Write-only (see the proto): a stored cause never carries it.
        host_class: String::new(),
        assist_kind: text("assist_kind"),
        assist_origin: text("assist_origin"),
        host: text("host"),
    };
    Some(normalized(cause).0)
}

/// The environment switch for denial memory within a run (build plan WS13 S4). Default off.
/// Read through the one strict parser, so only a literal `true` enables it.
pub const DENIAL_MEMORY_ENV: &str = "APPROVAL_DENIAL_MEMORY_ENABLED";

/// Parse the denial-memory switch: `true` in any case and with surrounding whitespace, and
/// nothing else. An unset or malformed value leaves denial memory off.
#[must_use]
pub fn denial_memory_from(value: Option<&str>) -> bool {
    switch_from(value)
}

/// Whether denial memory is on in this process.
#[must_use]
pub fn denial_memory_enabled() -> bool {
    denial_memory_from(std::env::var(DENIAL_MEMORY_ENV).ok().as_deref())
}

/// The environment switch for approval scope grants (build plan WS13 S6). Default off; only the
/// literal `true` enables it, read through the one strict parser.
pub const SCOPE_GRANTS_ENV: &str = "APPROVAL_SCOPE_GRANTS_ENABLED";
/// The environment setting for a standing grant's lifetime, in seconds (build plan K3-3).
pub const THREAD_GRANT_TTL_ENV: &str = "APPROVAL_THREAD_GRANT_TTL_SECS";
/// The lifetime a standing grant gets when the setting is unset, malformed or not positive.
pub const THREAD_GRANT_TTL_DEFAULT_SECS: i64 = 3600;
/// The longest lifetime a standing grant may be given.
pub const THREAD_GRANT_TTL_MAX_SECS: i64 = 28800;

/// Parse the scope-grants switch: `true` in any case and with surrounding whitespace, and nothing
/// else. An unset or malformed value leaves grants off.
#[must_use]
pub fn scope_grants_from(value: Option<&str>) -> bool {
    switch_from(value)
}

/// Whether approval scope grants are on in this process.
#[must_use]
pub fn scope_grants_enabled() -> bool {
    scope_grants_from(std::env::var(SCOPE_GRANTS_ENV).ok().as_deref())
}

/// The lifetime in seconds of a standing grant: the setting, capped at the maximum. Unset,
/// malformed and non-positive values get the default.
#[must_use]
pub fn thread_grant_ttl_from(value: Option<&str>) -> i64 {
    match value.and_then(|text| text.trim().parse::<i64>().ok()) {
        Some(secs) if secs > 0 => secs.min(THREAD_GRANT_TTL_MAX_SECS),
        _ => THREAD_GRANT_TTL_DEFAULT_SECS,
    }
}

/// The lifetime of a standing grant in this process.
#[must_use]
pub fn thread_grant_ttl_secs() -> i64 {
    thread_grant_ttl_from(std::env::var(THREAD_GRANT_TTL_ENV).ok().as_deref())
}

/// Whether a listed approval is a decision that declined this exact call in this run. The
/// digest is run-salted, so the run check is a second guard, not the only one. An approval
/// that states no cause never matches.
#[must_use]
pub fn declines_call(approval: &Approval, run_id: &str, call_digest: &str) -> bool {
    approval.run_id == run_id
        && approval.state == ApprovalState::Denied as i32
        && approval
            .cause
            .as_ref()
            .is_some_and(|cause| cause.call_digest == call_digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_round_trips_through_its_token() {
        for code in CauseCode::ALL {
            assert_eq!(CauseCode::from_wire(code.as_str()), code);
            assert!(CauseCode::is_known(code.as_str()));
        }
    }

    #[test]
    fn a_token_outside_the_vocabulary_is_unclassified() {
        for token in [
            "",
            " provider_write",
            "PROVIDER_WRITE",
            "tainted_egress_retired",
            "nope",
        ] {
            assert_eq!(CauseCode::from_wire(token), CauseCode::UnclassifiedGate);
        }
        assert!(!CauseCode::is_known("tainted_egress_retired"));
    }

    #[test]
    fn the_key_names_the_operation_and_never_an_argument() {
        assert_eq!(
            action_key("delete_records", r#"{"id":"r-1"}"#),
            "delete_records"
        );
        assert_eq!(
            action_key(
                "execute_provider_action",
                r#"{"connection_id":"c1","operation":"message.send","body":{"text":"hi"}}"#
            ),
            "execute_provider_action:message.send"
        );
        assert_eq!(
            action_key(
                "execute_provider_action",
                r#"{"operation":"send to alice@example.com"}"#
            ),
            "execute_provider_action",
            "an operation that is not a catalogued name is argument text"
        );
        assert_eq!(
            action_key("execute_provider_action", "not json"),
            "execute_provider_action"
        );
    }

    #[test]
    fn the_digest_is_stable_per_run_and_differs_across_runs_and_arguments() {
        let a = call_digest("run_1", "delete_records", r#"{"id":"r-1","force":true}"#);
        let reordered = call_digest(
            "run_1",
            "delete_records",
            r#"{ "force": true, "id": "r-1" }"#,
        );
        assert_eq!(
            a, reordered,
            "the same call written two ways has one digest"
        );
        assert_ne!(
            a,
            call_digest("run_2", "delete_records", r#"{"id":"r-1","force":true}"#)
        );
        assert_ne!(
            a,
            call_digest("run_1", "delete_records", r#"{"id":"r-2","force":true}"#)
        );
        assert_ne!(
            a,
            call_digest("run_1", "delete_record", r#"{"id":"r-1","force":true}"#)
        );
        assert_eq!(a.len(), 64);
        assert!(!a.contains("r-1"));
    }

    #[test]
    fn the_digest_does_not_share_bytes_across_the_tool_and_argument_boundary() {
        assert_ne!(
            call_digest("run_1", "ab", "c"),
            call_digest("run_1", "a", "bc"),
        );
    }

    #[test]
    fn normalising_degrades_an_unknown_code_and_fixes_the_scope_options() {
        let (cause, known) = normalized(ApprovalCause {
            code: "tainted_egress_retired".to_owned(),
            scope_options: vec!["run".to_owned(), "thread".to_owned()],
            tool: "x".repeat(MAX_FIELD_CHARS + 50),
            ..ApprovalCause::default()
        });
        assert!(!known);
        assert_eq!(cause.code, "unclassified_gate");
        assert_eq!(cause.scope_options, vec!["once".to_owned()]);
        assert_eq!(cause.tool.chars().count(), MAX_FIELD_CHARS);
    }

    #[test]
    fn the_stored_shape_reads_back_and_a_row_without_a_cause_reads_none() {
        let cause = for_call(
            CauseCode::RiskyEffectUnderAsk,
            "run_1",
            "delete_records",
            r#"{"id":"r-1"}"#,
        );
        let metadata = json!({ "reason": "r", "cause": to_metadata(&cause) });
        assert_eq!(from_metadata(&metadata), Some(cause));
        assert_eq!(from_metadata(&json!({ "reason": "r" })), None);
        assert_eq!(from_metadata(&json!({ "cause": "not an object" })), None);
    }

    #[test]
    fn denial_memory_is_off_unless_the_switch_reads_true() {
        assert!(!denial_memory_from(None), "an unset switch is off");
        for value in ["", "1", "yes", "on", "false", "truee"] {
            assert!(
                !denial_memory_from(Some(value)),
                "{value:?} does not enable denial memory"
            );
        }
        for value in ["true", "TRUE", " True "] {
            assert!(
                denial_memory_from(Some(value)),
                "{value:?} enables denial memory"
            );
        }
    }

    #[test]
    fn a_denial_matches_only_the_call_it_declined_in_its_run() {
        let arguments = r#"{"id":"r-1"}"#;
        let denied = Approval {
            run_id: "run_1".to_owned(),
            state: ApprovalState::Denied as i32,
            cause: Some(for_call(
                CauseCode::RiskyEffectUnderAsk,
                "run_1",
                "delete_records",
                arguments,
            )),
            ..Approval::default()
        };
        let digest = call_digest("run_1", "delete_records", arguments);
        assert!(declines_call(&denied, "run_1", &digest));
        assert!(
            !declines_call(
                &denied,
                "run_1",
                &call_digest("run_1", "delete_records", r#"{"id":"r-2"}"#)
            ),
            "a different argument set is a different call"
        );
        assert!(
            !declines_call(
                &denied,
                "run_2",
                &call_digest("run_2", "delete_records", arguments)
            ),
            "the same call in another run is not declined"
        );
        let granted = Approval {
            state: ApprovalState::Granted as i32,
            ..denied.clone()
        };
        assert!(
            !declines_call(&granted, "run_1", &digest),
            "only a denial declines"
        );
        let uncaused = Approval {
            cause: None,
            ..denied
        };
        assert!(
            !declines_call(&uncaused, "run_1", &digest),
            "an approval that states no cause never matches"
        );
    }

    fn scopes(cause: &ApprovalCause) -> Vec<&str> {
        cause.scope_options.iter().map(String::as_str).collect()
    }

    #[test]
    fn capability_high_risk_destructive_is_once_only() {
        for class in [
            "destructive",
            "external_write",
            "owner_effect",
            "unclassified",
            "",
        ] {
            let cause = for_call_with_class(
                CauseCode::CapabilityHighRisk,
                "run_1",
                "delete_records",
                r#"{"id":"r-1"}"#,
                class,
            );
            assert_eq!(scopes(&cause), ["once"], "{class}");
        }
        let read = for_call_with_class(
            CauseCode::CapabilityHighRisk,
            "run_1",
            "knowledge_search",
            r#"{"q":"x"}"#,
            "read",
        );
        assert_eq!(scopes(&read), ["once", "thread", "run"]);
    }

    #[test]
    fn a_read_sandbox_or_context_write_under_a_widening_code_offers_thread_and_run() {
        for code in [
            CauseCode::RiskyEffectUnderAsk,
            CauseCode::ConsentToDisclose,
            CauseCode::CapabilityHighRisk,
        ] {
            for class in ["read", "sandbox_compute", "context_write"] {
                let cause = for_call_with_class(code, "run_1", "knowledge_search", "{}", class);
                assert_eq!(
                    scopes(&cause),
                    ["once", "thread", "run"],
                    "{code:?} {class}"
                );
            }
        }
    }

    #[test]
    fn owner_effects_and_unwidening_codes_are_once_whatever_the_class() {
        for code in [
            CauseCode::OwnerEffectReserved,
            CauseCode::ProviderWrite,
            CauseCode::AutoPostureProviderWrite,
            CauseCode::BrowserActionRisk,
            CauseCode::HookAsk,
            CauseCode::CapabilityPolicyAsk,
            CauseCode::UnclassifiedGate,
        ] {
            let cause = for_call_with_class(code, "run_1", "knowledge_search", "{}", "read");
            assert_eq!(scopes(&cause), ["once"], "{code:?}");
        }
    }

    #[test]
    fn provider_writes_mcp_shell_and_process_tools_are_once_whatever_the_class() {
        for tool in [
            "execute_provider_action",
            "book_shipment",
            "publish_social_post",
            "mcp__srv__lookup",
            "shell",
            "process_start",
        ] {
            let cause =
                for_call_with_class(CauseCode::ConsentToDisclose, "run_1", tool, "{}", "read");
            assert_eq!(scopes(&cause), ["once"], "{tool}");
        }
    }

    #[test]
    fn a_cause_without_a_class_tool_or_key_is_once() {
        let no_class = for_call(
            CauseCode::RiskyEffectUnderAsk,
            "run_1",
            "knowledge_search",
            "{}",
        );
        assert_eq!(scopes(&no_class), ["once"], "a call that states no class");
        let differently_cased = for_call_with_class(
            CauseCode::RiskyEffectUnderAsk,
            "run_1",
            "knowledge_search",
            "{}",
            "Read",
        );
        assert_eq!(
            scopes(&differently_cased),
            ["once"],
            "the class is matched exactly"
        );
        let no_tool = ApprovalCause {
            code: "risky_effect_under_ask".to_owned(),
            effect_class: "read".to_owned(),
            action_key: "knowledge_search".to_owned(),
            ..ApprovalCause::default()
        };
        assert_eq!(eligible_scopes(&no_tool), ["once"]);
        let no_key = ApprovalCause {
            code: "risky_effect_under_ask".to_owned(),
            tool: "knowledge_search".to_owned(),
            effect_class: "read".to_owned(),
            ..ApprovalCause::default()
        };
        assert_eq!(eligible_scopes(&no_key), ["once"]);
    }

    #[test]
    fn normalising_recomputes_the_scopes_from_the_facts() {
        let sent_wide = ApprovalCause {
            code: "provider_write".to_owned(),
            tool: "knowledge_search".to_owned(),
            effect_class: "read".to_owned(),
            action_key: "knowledge_search".to_owned(),
            scope_options: vec!["once".to_owned(), "thread".to_owned(), "run".to_owned()],
            ..ApprovalCause::default()
        };
        let (cause, known) = normalized(sent_wide);
        assert!(known);
        assert_eq!(
            scopes(&cause),
            ["once"],
            "a provider write is never offered a standing scope, whatever the sender states"
        );
        let read = ApprovalCause {
            code: "consent_to_disclose".to_owned(),
            tool: "knowledge_search".to_owned(),
            effect_class: "read".to_owned(),
            action_key: "knowledge_search".to_owned(),
            scope_options: vec!["once".to_owned()],
            ..ApprovalCause::default()
        };
        let (cause, _) = normalized(read);
        assert_eq!(scopes(&cause), ["once", "thread", "run"]);
    }

    #[test]
    fn the_scope_grants_switch_is_strict() {
        assert!(scope_grants_from(Some("true")));
        assert!(scope_grants_from(Some(" TRUE ")));
        for value in [
            None,
            Some(""),
            Some("1"),
            Some("yes"),
            Some("on"),
            Some("false"),
        ] {
            assert!(!scope_grants_from(value), "{value:?}");
        }
    }

    #[test]
    fn the_thread_grant_lifetime_defaults_and_caps_at_the_maximum() {
        assert_eq!(thread_grant_ttl_from(None), 3600);
        assert_eq!(thread_grant_ttl_from(Some("nonsense")), 3600);
        assert_eq!(thread_grant_ttl_from(Some("0")), 3600);
        assert_eq!(thread_grant_ttl_from(Some("-5")), 3600);
        assert_eq!(thread_grant_ttl_from(Some(" 900 ")), 900);
        assert_eq!(thread_grant_ttl_from(Some("28800")), 28800);
        assert_eq!(thread_grant_ttl_from(Some("604800")), 28800);
    }

    const PINNED: &str = "2b6a1f0c9d8e7f6a5b4c3d2e1f0a9b8c7d6e5f4a3b2c1d0e9f8a7b6c5d4e3f2a";

    fn mcp_call(code: CauseCode, class: &str, digest: &str) -> ApprovalCause {
        for_call_with_tool(
            code,
            "run_1",
            "mcp__fixture__create_note",
            r#"{"title":"x"}"#,
            class,
            digest,
        )
    }

    #[test]
    fn a_pinned_mcp_external_write_offers_thread_and_run_under_a_widening_code() {
        for code in [CauseCode::RiskyEffectUnderAsk, CauseCode::ConsentToDisclose] {
            let cause = mcp_call(code, "external_write", PINNED);
            assert_eq!(scopes(&cause), ["once", "thread", "run"], "{code:?}");
        }
        let high_risk_read = mcp_call(CauseCode::CapabilityHighRisk, "read", PINNED);
        assert_eq!(scopes(&high_risk_read), ["once", "thread", "run"]);
    }

    #[test]
    fn an_unpinned_or_malformed_mcp_digest_leaves_the_tool_once() {
        let uppercase = PINNED.to_uppercase();
        for digest in ["", "not-a-digest", uppercase.as_str()] {
            for class in ["read", "external_write"] {
                let cause = mcp_call(CauseCode::RiskyEffectUnderAsk, class, digest);
                assert_eq!(scopes(&cause), ["once"], "{digest:?} {class}");
            }
        }
    }

    #[test]
    fn a_pinned_mcp_destructive_owner_or_high_risk_external_write_stays_once() {
        for (code, class) in [
            (CauseCode::RiskyEffectUnderAsk, "destructive"),
            (CauseCode::RiskyEffectUnderAsk, "owner_effect"),
            (CauseCode::CapabilityHighRisk, "external_write"),
        ] {
            let cause = mcp_call(code, class, PINNED);
            assert_eq!(scopes(&cause), ["once"], "{code:?} {class}");
        }
    }

    #[test]
    fn a_digest_on_a_built_in_tool_pins_nothing() {
        let cause = for_call_with_tool(
            CauseCode::RiskyEffectUnderAsk,
            "run_1",
            "execute_provider_action",
            r#"{"operation":"message.send"}"#,
            "external_write",
            PINNED,
        );
        assert_eq!(scopes(&cause), ["once"]);
        assert!(!is_pinned_mcp(&cause));
    }

    #[test]
    fn the_pin_survives_the_stored_shape_and_a_malformed_digest_is_dropped() {
        let cause = mcp_call(CauseCode::RiskyEffectUnderAsk, "external_write", PINNED);
        let metadata = json!({ "cause": to_metadata(&cause) });
        assert_eq!(from_metadata(&metadata), Some(cause.clone()));
        let (malformed, _) = normalized(ApprovalCause {
            tool_digest: "not-a-digest".to_owned(),
            ..cause
        });
        assert!(malformed.tool_digest.is_empty());
        assert_eq!(scopes(&malformed), ["once"]);
    }

    #[test]
    fn a_rule_is_offered_only_for_a_pinned_mcp_tool_the_person_may_relax() {
        let offered = [
            mcp_call(CauseCode::RiskyEffectUnderAsk, "external_write", PINNED),
            mcp_call(CauseCode::ConsentToDisclose, "read", PINNED),
            mcp_call(CauseCode::CapabilityHighRisk, "read", PINNED),
        ];
        for cause in &offered {
            assert!(rule_offered(cause), "{cause:?}");
        }
        let refused = [
            mcp_call(CauseCode::RiskyEffectUnderAsk, "destructive", PINNED),
            mcp_call(CauseCode::RiskyEffectUnderAsk, "owner_effect", PINNED),
            mcp_call(CauseCode::CapabilityHighRisk, "external_write", PINNED),
            mcp_call(CauseCode::HookAsk, "external_write", PINNED),
            mcp_call(CauseCode::RiskyEffectUnderAsk, "external_write", ""),
            for_call_with_tool(
                CauseCode::RiskyEffectUnderAsk,
                "run_1",
                "write_file",
                "{}",
                "external_write",
                PINNED,
            ),
        ];
        for cause in &refused {
            assert!(!rule_offered(cause), "{cause:?}");
        }
    }

    #[test]
    fn a_browser_assist_cause_is_once_and_keeps_its_closed_facts_through_the_stored_shape() {
        let cause = for_browser_assist(
            "run_1",
            "browser.observe",
            "{}",
            ASSIST_KIND_USER_TAKEOVER,
            ASSIST_ORIGIN_PERSON,
            "example.com",
        );
        assert_eq!(cause.code, "browser_human_assist");
        assert_eq!(scopes(&cause), ["once"]);
        assert_eq!(
            (cause.assist_kind.as_str(), cause.assist_origin.as_str(), cause.host.as_str()),
            ("user_takeover", "person", "example.com")
        );
        let metadata = json!({ "cause": to_metadata(&cause) });
        assert_eq!(from_metadata(&metadata), Some(cause));
    }

    #[test]
    fn browser_assist_words_outside_the_closed_sets_are_dropped() {
        let cause = for_browser_assist(
            "run_1",
            "browser.observe",
            "{}",
            "type your password here",
            "page",
            &"h".repeat(MAX_FIELD_CHARS + 10),
        );
        assert_eq!(cause.assist_kind, "");
        assert_eq!(cause.assist_origin, "");
        assert_eq!(cause.host.chars().count(), MAX_FIELD_CHARS);
        for kind in ASSIST_KINDS {
            let kept =
                for_browser_assist("run_1", "browser.observe", "{}", kind, ASSIST_ORIGIN_AGENT, "");
            assert_eq!(kept.assist_kind, kind);
            assert_eq!(kept.assist_origin, "agent");
        }
    }

    #[test]
    fn only_a_browser_assist_cause_may_state_a_host_or_assist_facts() {
        let (cause, known) = normalized(ApprovalCause {
            code: "risky_effect_under_ask".to_owned(),
            assist_kind: ASSIST_KIND_USER_TAKEOVER.to_owned(),
            assist_origin: ASSIST_ORIGIN_PERSON.to_owned(),
            host: "example.com".to_owned(),
            ..ApprovalCause::default()
        });
        assert!(known);
        assert_eq!(
            (cause.assist_kind.as_str(), cause.assist_origin.as_str(), cause.host.as_str()),
            ("", "", "")
        );
    }
}
