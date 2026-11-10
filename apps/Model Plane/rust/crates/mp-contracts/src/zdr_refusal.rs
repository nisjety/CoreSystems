//! The typed refusals of zero data retention (ZDR): one catalogue for every
//! Model Plane service.
//!
//! A ZDR turn can be refused for a reason the user can act on: the org has ZDR
//! on and no model is attested for it, a feature cannot run without durable
//! state, a connection cannot give ZDR. Each such refusal used to be a sentence
//! assembled where it happened, in English only, with no stable code for a
//! client to branch on. This module is the one place those codes and their
//! English fallback text live. The Norwegian and English text a person reads is
//! the SPA's, keyed by `code` and checked against
//! `api/contracts/zdr-refusals.json` (Z-05b), never this file's.
//!
//! Each transport carries a refusal in one shape:
//!
//! * SSE: the `error` frame, `{code, message, retryable, action, feature?}`.
//! * JSON: `{"error": {code, message, retryable, action, feature?}}`.
//! * gRPC: a status whose trailer [`KIND_TRAILER`] (`x-mp-provider-error`, the
//!   kind inference-core already declares, see `infer-statuses.json`) names the
//!   code, and whose trailer [`FEATURE_TRAILER`] names the feature when the code
//!   takes one. The status code stays the one each site already returns.
//!
//! The kind `zdr_unavailable` is frozen (CR-T3): its wire value does not change.
//! `residency_unavailable` and `tier_unavailable` are inference-core's posture
//! kinds, not ZDR refusals, and are not in this catalogue.

use serde_json::{json, Value};
use tonic::metadata::{MetadataMap, MetadataValue};
use tonic::{Code, Status};

/// Trailer naming a refusal's catalogue code. The same key inference-core uses
/// for its provider-error kinds: one vocabulary, not a parallel one.
pub const KIND_TRAILER: &str = "x-mp-provider-error";

/// Trailer naming the feature of a [`code::FEATURE_NOT_SUPPORTED`] refusal. A
/// detail trailer, like `x-mp-retry-after-ms`: it is sent only when the code
/// takes a feature.
pub const FEATURE_TRAILER: &str = "x-mp-zdr-feature";

/// The stable codes. Clients match these strings; a rename breaks them.
pub mod code {
    /// Frozen kind (CR-T3). No deployment that serves the request is attested to
    /// give zero retention, so nothing was sent to a model.
    pub const UNAVAILABLE: &str = "zdr_unavailable";
    /// The request asks for a feature that cannot run under ZDR. Carries the
    /// feature key.
    pub const FEATURE_NOT_SUPPORTED: &str = "zdr_feature_not_supported";
    /// The turn names the user's own connection, and that connection cannot
    /// give ZDR.
    pub const CONNECTION_NOT_SUPPORTED: &str = "zdr_connection_not_supported";
    /// The connection can give ZDR only once the user has confirmed, for this
    /// org, that they accept it does not here.
    pub const CONNECTION_NOT_ACKNOWLEDGED: &str = "zdr_connection_not_acknowledged";
    /// Temporary chat asks for ZDR explicitly, and the connection cannot provide
    /// it. An explicit request is never waived.
    pub const EXPLICIT_REQUEST_NOT_SUPPORTED_BY_CONNECTION: &str =
        "zdr_explicit_request_not_supported_by_connection";
    /// The retention decision could not be made right now (a dependency is
    /// unreachable). Nothing was sent. Retry.
    pub const DECISION_UNAVAILABLE: &str = "zdr_decision_unavailable";
    /// A connection-bound decision was presented on a route it does not name.
    pub const WAIVER_ROUTE_MISMATCH: &str = "zdr_waiver_route_mismatch";
    /// The credentials of one request disagree about zero retention.
    pub const POSTURE_MISMATCH: &str = "zdr_posture_mismatch";
    /// The credentials cannot create, change or delete durable state.
    pub const DURABLE_MUTATION_FORBIDDEN: &str = "zdr_durable_mutation_forbidden";
    /// An ephemeral inference failed for a reason other than the refusals above.
    /// Retry.
    pub const INFERENCE_FAILED: &str = "zdr_inference_failed";

    /// Every code, in catalogue order. A new code is added here and to
    /// [`super::ENTRIES`]; the tests fail until both agree.
    pub const ALL: &[&str] = &[
        UNAVAILABLE,
        FEATURE_NOT_SUPPORTED,
        CONNECTION_NOT_SUPPORTED,
        CONNECTION_NOT_ACKNOWLEDGED,
        EXPLICIT_REQUEST_NOT_SUPPORTED_BY_CONNECTION,
        DECISION_UNAVAILABLE,
        WAIVER_ROUTE_MISMATCH,
        POSTURE_MISMATCH,
        DURABLE_MUTATION_FORBIDDEN,
        INFERENCE_FAILED,
    ];
}

/// What a client should offer the user. The SPA maps each to a control; the
/// wire carries the name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Nothing the user can do about it here.
    None,
    /// Send the same turn again later.
    Retry,
    /// Pick a model that does not need this route.
    ChooseAnotherModel,
    /// Confirm or review the connection under Integrations.
    OpenIntegrations,
    /// Turn off the named feature and send again.
    TurnOffFeature,
}

impl Action {
    /// The wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Retry => "retry",
            Self::ChooseAnotherModel => "choose_another_model",
            Self::OpenIntegrations => "open_integrations",
            Self::TurnOffFeature => "turn_off_feature",
        }
    }
}

/// One catalogue entry: the English fallback a client without a translation
/// shows, and the facts that do not vary by where the refusal came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub code: &'static str,
    /// English. A `{feature}` placeholder is replaced by the feature's label
    /// when `takes_feature` is set. Every sentence states only what happened.
    pub message: &'static str,
    /// Whether sending the same turn again could succeed.
    pub retryable: bool,
    pub action: Action,
    /// Whether the code names a feature.
    pub takes_feature: bool,
}

/// The catalogue. Exactly one entry per [`code::ALL`] member.
pub const ENTRIES: &[Entry] = &[
    Entry {
        code: code::UNAVAILABLE,
        message: "Zero data retention applies to this request, but no model deployment is attested to provide it. The request was not sent to a model.",
        retryable: false,
        action: Action::ChooseAnotherModel,
        takes_feature: false,
    },
    Entry {
        code: code::FEATURE_NOT_SUPPORTED,
        message: "This is not supported with zero data retention yet: {feature}. The request was refused before any model call.",
        retryable: false,
        action: Action::TurnOffFeature,
        takes_feature: true,
    },
    Entry {
        code: code::CONNECTION_NOT_SUPPORTED,
        message: "Zero data retention is on for this organisation, and the ChatGPT connection selected for this chat cannot provide it. The request was not sent to any model. No other model was used.",
        retryable: false,
        action: Action::ChooseAnotherModel,
        takes_feature: false,
    },
    Entry {
        code: code::CONNECTION_NOT_ACKNOWLEDGED,
        message: "Zero data retention is on for this organisation, and the connection selected for this chat has not been confirmed for it. The request was not sent to any model.",
        retryable: false,
        action: Action::OpenIntegrations,
        takes_feature: false,
    },
    Entry {
        code: code::EXPLICIT_REQUEST_NOT_SUPPORTED_BY_CONNECTION,
        message: "Temporary chat requires zero data retention, and the connection selected for this chat cannot provide it. The request was not sent to any model.",
        retryable: false,
        action: Action::ChooseAnotherModel,
        takes_feature: false,
    },
    Entry {
        code: code::DECISION_UNAVAILABLE,
        message: "The zero data retention settings cannot be confirmed right now. The request was not sent to any model. Try again shortly.",
        retryable: true,
        action: Action::Retry,
        takes_feature: false,
    },
    Entry {
        code: code::WAIVER_ROUTE_MISMATCH,
        message: "This request names a route other than the one its zero data retention decision was made for. The request was not sent to any model.",
        retryable: false,
        action: Action::None,
        takes_feature: false,
    },
    Entry {
        code: code::POSTURE_MISMATCH,
        message: "The zero data retention setting on the credentials for this request does not match. The request was not sent to any model.",
        retryable: false,
        action: Action::None,
        takes_feature: false,
    },
    Entry {
        code: code::DURABLE_MUTATION_FORBIDDEN,
        message: "Zero Data Retention credentials cannot create, change, or delete durable state.",
        retryable: false,
        action: Action::None,
        takes_feature: false,
    },
    Entry {
        code: code::INFERENCE_FAILED,
        message: "The zero data retention request could not be completed. Nothing was stored.",
        retryable: true,
        action: Action::Retry,
        takes_feature: false,
    },
];

/// A feature a ZDR turn cannot use, named by a stable key with its English
/// label. The key is what a client receives; the label fills `{feature}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Feature {
    pub key: &'static str,
    pub label: &'static str,
}

/// The closed feature vocabulary. Every key a refusal site names must be here
/// (the tests check it), and a key never changes meaning.
pub const FEATURES: &[Feature] = &[
    Feature {
        key: "attachments",
        label: "file and image attachments",
    },
    Feature {
        key: "image_generation",
        label: "image generation",
    },
    Feature {
        key: "deep_research",
        label: "deep research",
    },
    Feature {
        key: "agentic",
        label: "Do mode",
    },
    Feature {
        key: "plan_mode",
        label: "plan mode",
    },
    Feature {
        key: "web_search",
        label: "web search and browsing",
    },
    Feature {
        key: "client_tools",
        label: "tools the client declares",
    },
    Feature {
        key: "structured_output",
        label: "structured output",
    },
    Feature {
        key: "skills",
        label: "picked skills",
    },
    Feature {
        key: "pinned_messages",
        label: "pinned messages",
    },
    Feature {
        key: "spend_cap",
        label: "a per-turn spend limit",
    },
    Feature {
        key: "deployed_agent",
        label: "deployed agents",
    },
    Feature {
        key: "tool_execution",
        label: "tool execution",
    },
    Feature {
        key: "agent_run",
        label: "agent runs",
    },
    Feature {
        key: "run_control",
        label: "run controls",
    },
    Feature {
        key: "scheduled_step",
        label: "scheduled runs",
    },
    Feature {
        key: "approval_continuation",
        label: "approval continuations",
    },
    Feature {
        key: "send_message",
        label: "sending messages",
    },
    Feature {
        key: "lsp_query",
        label: "code intelligence queries",
    },
    Feature {
        key: "mcp_execution",
        label: "MCP tool execution",
    },
    Feature {
        key: "command_execution",
        label: "command execution",
    },
    Feature {
        key: "speech_synthesis",
        label: "speech synthesis",
    },
    Feature {
        key: "speech_transcription",
        label: "speech transcription",
    },
    Feature {
        key: "browser_run",
        label: "browser runs",
    },
    Feature {
        key: "browser_run_control",
        label: "browser run controls",
    },
    Feature {
        key: "browser_tool",
        label: "browser tools",
    },
    Feature {
        key: "browser_human_assist",
        label: "browser help requests",
    },
    Feature {
        key: "translation",
        label: "translation",
    },
    Feature {
        key: "document_analysis",
        label: "document analysis",
    },
    Feature {
        key: "language_analysis",
        label: "language analysis",
    },
    Feature {
        key: "realtime_voice",
        label: "realtime voice sessions",
    },
    Feature {
        key: "video_generation",
        label: "video generation",
    },
    Feature {
        key: "connector_read",
        label: "connected-app reads",
    },
    Feature {
        key: "message_versions",
        label: "message versions",
    },
    Feature {
        key: "sandbox_workspace",
        label: "files kept between calls in a thread workspace",
    },
    Feature {
        key: "user_profile",
        label: "the standing profile",
    },
    // EP-12: the Agent API and conversation export, share and import refuse a ZDR
    // caller before any read or write, so each key names the surface it stops.
    Feature {
        key: "agent_api",
        label: "the Agent API",
    },
    Feature {
        key: "conversation_export",
        label: "conversation export and replay",
    },
    Feature {
        key: "conversation_share",
        label: "conversation sharing",
    },
    Feature {
        key: "conversation_import",
        label: "conversation import",
    },
    // EP-14 S5/S7: the run timing and request-inspection reads refuse a ZDR caller.
    Feature {
        key: "run_inspection",
        label: "run timing and request inspection",
    },
    // EP-3 S5: a send's typed context references (documents, threads, selections) are refused on a
    // ZDR turn before any network call, so no reference is resolved and none is silently dropped.
    Feature {
        key: "context_references",
        label: "references to documents, threads or selections",
    },
    // EP-7 S1: a thread goal is user content in a new durable store (D7).
    Feature {
        key: "thread_goal",
        label: "persistent goals",
    },
    // EP-9 C1: org-core refuses a thread legal hold on a ZDR or waived thread with
    // legal_hold_nothing_retained: nothing is retained, so there is nothing to hold.
    Feature {
        key: "legal_hold",
        label: "a legal hold on a thread",
    },
    // EP-11 S7: /learn sends a thread's transcript to the review model and keeps what it distils, so a
    // ZDR or waived caller, and a thread holding a ZDR or waived run, is refused before any read.
    Feature {
        key: "learn_command",
        label: "learning from this conversation",
    },
    // EP-11 S1/S8: saving, editing or importing a skill is configuration a ZDR credential cannot write.
    Feature {
        key: "skill_authoring",
        label: "saving or editing skills",
    },
    // EP-6 P3: one named agent's notes in a Space are retained content, so a ZDR or waived caller is
    // refused the listing before any read.
    Feature {
        key: "named_agent",
        label: "named agent memory",
    },
    // W4-P6: process_read's spill writes a background process's output into a workspace file, which is
    // retained content, so a ZDR or waived run is refused before any read or write.
    Feature {
        key: "process_output_spill",
        label: "saving background process output to files",
    },
];

/// The catalogue entry for `code`, if it is a catalogue code.
#[must_use]
pub fn entry(code: &str) -> Option<&'static Entry> {
    ENTRIES.iter().find(|entry| entry.code == code)
}

/// The feature for `key`, if it is in the vocabulary.
#[must_use]
pub fn feature(key: &str) -> Option<&'static Feature> {
    FEATURES.iter().find(|feature| feature.key == key)
}

/// One typed refusal, ready for any transport. Only the constructors here make
/// one, so a refusal always names a catalogue code, and a code that takes a
/// feature always names one from the vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refusal {
    code: &'static str,
    feature: Option<&'static str>,
}

impl Refusal {
    const fn of(code: &'static str) -> Self {
        Self {
            code,
            feature: None,
        }
    }

    /// A turn that names the user's own connection while ZDR applies. The text
    /// promises nothing that cannot happen: nothing was sent, and no other model
    /// stood in.
    pub const CONNECTION_NOT_SUPPORTED: Self = Self::of(code::CONNECTION_NOT_SUPPORTED);
    /// The user's own connection can give ZDR only once they have acknowledged
    /// it, and this turn carries no valid acknowledgement. Nothing was sent.
    pub const CONNECTION_NOT_ACKNOWLEDGED: Self = Self::of(code::CONNECTION_NOT_ACKNOWLEDGED);
    /// A connection-bound decision was presented on a route it does not name.
    pub const WAIVER_ROUTE_MISMATCH: Self = Self::of(code::WAIVER_ROUTE_MISMATCH);
    /// The explicit-request refusal for a connection (Temporary chat).
    pub const EXPLICIT_REQUEST_NOT_SUPPORTED_BY_CONNECTION: Self =
        Self::of(code::EXPLICIT_REQUEST_NOT_SUPPORTED_BY_CONNECTION);
    /// No attested ZDR deployment serves the request.
    pub const UNAVAILABLE: Self = Self::of(code::UNAVAILABLE);
    /// The retention decision cannot be made right now.
    pub const DECISION_UNAVAILABLE: Self = Self::of(code::DECISION_UNAVAILABLE);
    /// The credentials of one request disagree about zero retention.
    pub const POSTURE_MISMATCH: Self = Self::of(code::POSTURE_MISMATCH);
    /// The credentials cannot create, change or delete durable state.
    pub const DURABLE_MUTATION_FORBIDDEN: Self = Self::of(code::DURABLE_MUTATION_FORBIDDEN);
    /// An ephemeral inference failed for another reason.
    pub const INFERENCE_FAILED: Self = Self::of(code::INFERENCE_FAILED);

    /// A feature that cannot run under ZDR. `key` must be in [`FEATURES`].
    #[must_use]
    pub fn feature_not_supported(key: &'static str) -> Self {
        debug_assert!(feature(key).is_some(), "unknown ZDR feature key {key}");
        Self {
            code: code::FEATURE_NOT_SUPPORTED,
            feature: Some(key),
        }
    }

    /// The refusal for a catalogue code, checked: a code that takes a feature
    /// needs one from the vocabulary, and a code that does not takes none.
    #[must_use]
    pub fn new(code: &'static str, feature: Option<&'static str>) -> Option<Self> {
        let entry = entry(code)?;
        match (entry.takes_feature, feature) {
            (true, Some(key)) => Some(Self {
                code: entry.code,
                feature: Some(self::feature(key)?.key),
            }),
            (false, None) => Some(Self::of(entry.code)),
            _ => None,
        }
    }

    /// The catalogue code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }

    /// The feature key, when the code takes one.
    #[must_use]
    pub const fn feature(&self) -> Option<&'static str> {
        self.feature
    }

    /// The catalogue entry this refusal is an instance of.
    #[must_use]
    pub fn entry(&self) -> &'static Entry {
        entry(self.code).expect("a Refusal always names a catalogue code")
    }

    /// Whether sending the same turn again could succeed.
    #[must_use]
    pub fn retryable(&self) -> bool {
        self.entry().retryable
    }

    /// What the client should offer the user.
    #[must_use]
    pub fn action(&self) -> Action {
        self.entry().action
    }

    /// The English fallback, with the feature's label filled in.
    #[must_use]
    pub fn message(&self) -> String {
        let entry = self.entry();
        match self.feature {
            Some(key) if entry.takes_feature => {
                let label = feature(key).map_or(key, |feature| feature.label);
                entry.message.replace("{feature}", label)
            }
            _ => entry.message.to_owned(),
        }
    }

    /// The JSON body: `{"error": {code, message, retryable, action, feature?}}`.
    /// `feature` appears only when the code takes one.
    #[must_use]
    pub fn json_body(&self) -> Value {
        let mut error = json!({
            "code": self.code,
            "message": self.message(),
            "retryable": self.retryable(),
            "action": self.action().as_str(),
        });
        if let Some(key) = self.feature {
            error["feature"] = json!(key);
        }
        json!({ "error": error })
    }

    /// The refusal as a gRPC status with `code` (each site keeps the code it
    /// already returns) and the kind and feature trailers.
    #[must_use]
    pub fn grpc_status(&self, code: Code) -> Status {
        let mut metadata = MetadataMap::new();
        metadata.insert(KIND_TRAILER, MetadataValue::from_static(self.code));
        if let Some(key) = self.feature {
            metadata.insert(FEATURE_TRAILER, MetadataValue::from_static(key));
        }
        Status::with_metadata(code, self.message(), metadata)
    }

    /// The refusal a JSON body carries in the shape [`Refusal::json_body`]
    /// writes. `None` for any other body, including a flat `{"error": "..."}`.
    #[must_use]
    pub fn from_body(body: &Value) -> Option<Self> {
        let error = body.get("error")?;
        let entry = entry(error.get("code")?.as_str()?)?;
        let feature = match error.get("feature") {
            None => None,
            Some(value) => Some(feature(value.as_str()?)?.key),
        };
        Self::new(entry.code, feature)
    }

    /// The refusal a status carries, when its kind trailer names a catalogue
    /// code. `None` for every other status, including the frozen posture kinds
    /// that are not ZDR refusals, and for a feature code whose feature is
    /// missing or unknown.
    #[must_use]
    pub fn from_status(status: &Status) -> Option<Self> {
        let metadata = status.metadata();
        let kind = metadata.get(KIND_TRAILER)?.to_str().ok()?;
        let entry = entry(kind)?;
        if !entry.takes_feature {
            return Some(Self::of(entry.code));
        }
        let key = metadata.get(FEATURE_TRAILER)?.to_str().ok()?;
        Some(Self {
            code: entry.code,
            feature: Some(feature(key)?.key),
        })
    }
}

/// The 1-based line numbers of untyped zero-data-retention refusals in `source`:
///
/// * a raw `failed_precondition(` or `permission_denied(` call whose arguments,
///   or the line above it (the `if` that guards it), name zero data retention;
/// * a `json!` body whose `error` is a bare string naming zero data retention,
///   which is the flat shape a client cannot match on.
///
/// Such a refusal carries no catalogue kind, so a client cannot tell it from
/// any other precondition. Test items (`#[cfg(test)]` modules and functions) and
/// comment lines are not scanned, and line numbers match the source. Each
/// service's inventory test runs this over its own sources, so a new refusal
/// that bypasses [`Refusal`] fails the build.
#[must_use]
pub fn untyped_zdr_refusal_lines(source: &str) -> Vec<usize> {
    const STATUS_CALLS: [&str; 2] = ["failed_precondition(", "permission_denied("];
    const ZDR_WORDS: [&str; 6] = [
        "zdr",
        "zero data retention",
        "zero-data-retention",
        "zero-retention",
        "zero retention",
        "retention posture",
    ];
    let code = strip_test_items(source);
    let mut found = Vec::new();
    for call in STATUS_CALLS {
        let mut from = 0;
        while let Some(offset) = code[from..].find(call) {
            let start = from + offset;
            from = start + call.len();
            let line_start = code[..start].rfind('\n').map_or(0, |index| index + 1);
            if code[line_start..start].trim_start().starts_with("//") {
                continue;
            }
            let arguments = call_arguments(&code[from..]);
            let guard_start = code[..line_start]
                .rmatch_indices('\n')
                .nth(1)
                .map_or(0, |(index, _)| index + 1);
            let window = format!("{}{}", &code[guard_start..start], arguments).to_ascii_lowercase();
            if ZDR_WORDS.iter().any(|word| window.contains(word)) {
                found.push(code[..start].matches('\n').count() + 1);
            }
        }
    }
    let mut from = 0;
    while let Some(offset) = code[from..].find("json!(") {
        let start = from + offset;
        from = start + "json!(".len();
        let line_start = code[..start].rfind('\n').map_or(0, |index| index + 1);
        if code[line_start..start].trim_start().starts_with("//") {
            continue;
        }
        let arguments = call_arguments(&code[from..]);
        let compact: String = arguments.chars().filter(|c| !c.is_whitespace()).collect();
        let lower = arguments.to_ascii_lowercase();
        if compact.contains("\"error\":\"") && ZDR_WORDS.iter().any(|word| lower.contains(word)) {
            found.push(code[..start].matches('\n').count() + 1);
        }
    }
    found.sort_unstable();
    found.dedup();
    found
}

/// The source with every test item blanked out: each `#[cfg(test)]` attribute
/// and the item it marks (up to the matching closing brace of its body, or to
/// its `;` for a bodiless item). Newlines are kept, so line numbers stay true.
fn strip_test_items(source: &str) -> String {
    const MARK: &str = "#[cfg(test)]";
    let mut out = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(index) = rest.find(MARK) {
        out.push_str(&rest[..index]);
        let after = &rest[index + MARK.len()..];
        let brace = after.find('{');
        let semicolon = after.find(';');
        let end = match (brace, semicolon) {
            (Some(open), Some(semi)) if semi < open => semi + 1,
            (Some(open), _) => {
                let mut depth = 0_usize;
                let mut close = after.len();
                for (position, character) in after[open..].char_indices() {
                    match character {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                close = open + position + 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                close
            }
            (None, Some(semi)) => semi + 1,
            (None, None) => after.len(),
        };
        let blanked = &rest[index..index + MARK.len() + end];
        out.extend(blanked.chars().map(|c| if c == '\n' { '\n' } else { ' ' }));
        rest = &rest[index + MARK.len() + end..];
    }
    out.push_str(rest);
    out
}

/// The text inside a call's parentheses, given the text just after its `(`.
fn call_arguments(after_open: &str) -> &str {
    let mut depth = 1_usize;
    for (index, character) in after_open.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return &after_open[..index];
                }
            }
            _ => {}
        }
    }
    after_open
}

/// The fixture `api/contracts/zdr-refusals.json` is generated from this value.
/// Clients read it; this function is the only writer.
#[must_use]
pub fn catalogue_json() -> Value {
    json!({
        "contract": "model-plane.zdr-refusals",
        "version": 1,
        "generated_by": "mp-contracts zdr_refusal tests (MP_UPDATE_GOLDEN=1)",
        "wire": {
            "sse_event": "error",
            "sse_fields": ["code", "message", "retryable", "action", "feature"],
            "json_body": {"error": ["code", "message", "retryable", "action", "feature"]},
            "grpc_kind_trailer": KIND_TRAILER,
            "grpc_feature_trailer": FEATURE_TRAILER,
            "feature_field_when": "takes_feature"
        },
        "codes": ENTRIES
            .iter()
            .map(|entry| json!({
                "code": entry.code,
                "message_en": entry.message,
                "retryable": entry.retryable,
                "action": entry.action.as_str(),
                "takes_feature": entry.takes_feature,
            }))
            .collect::<Vec<_>>(),
        "features": FEATURES
            .iter()
            .map(|feature| json!({"key": feature.key, "label_en": feature.label}))
            .collect::<Vec<_>>(),
    })
}

/// The fixture text: pretty JSON, LF, one trailing newline.
#[must_use]
pub fn catalogue_text() -> String {
    let mut text =
        serde_json::to_string_pretty(&catalogue_json()).expect("the catalogue serializes");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../../../api/contracts/zdr-refusals.json");
    const FIXTURE_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../api/contracts/zdr-refusals.json"
    );
    const INFER_STATUSES: &str = include_str!("../../../../api/contracts/infer-statuses.json");

    /// The codes that tell the client a later retry can succeed.
    const RETRYABLE: &[&str] = &[code::DECISION_UNAVAILABLE, code::INFERENCE_FAILED];

    #[test]
    fn the_catalogue_has_exactly_one_entry_per_code() {
        assert_eq!(ENTRIES.len(), code::ALL.len());
        for code in code::ALL {
            assert!(entry(code).is_some(), "{code} has no catalogue entry");
        }
        let mut seen = std::collections::BTreeSet::new();
        for entry in ENTRIES {
            assert!(seen.insert(entry.code), "{} is listed twice", entry.code);
        }
    }

    #[test]
    fn every_entry_is_plain_english_that_states_what_happened() {
        for entry in ENTRIES {
            assert!(
                !entry.message.trim().is_empty(),
                "{} has no text",
                entry.code
            );
            assert!(
                !entry.message.contains("  "),
                "{} has a double space",
                entry.code
            );
            assert!(
                entry.message.ends_with('.'),
                "{} must end with a full stop",
                entry.code
            );
            assert!(
                !entry.message.contains('\n') && !entry.message.contains('\r'),
                "{} must be one line",
                entry.code
            );
        }
    }

    #[test]
    fn only_the_retryable_codes_are_retryable() {
        for entry in ENTRIES {
            assert_eq!(
                entry.retryable,
                RETRYABLE.contains(&entry.code),
                "{} retryable flag",
                entry.code
            );
        }
    }

    #[test]
    fn exactly_the_feature_code_takes_a_feature_and_its_action_is_turn_off() {
        for entry in ENTRIES {
            assert_eq!(
                entry.takes_feature,
                entry.code == code::FEATURE_NOT_SUPPORTED,
                "{} takes_feature",
                entry.code
            );
        }
        assert_eq!(
            entry(code::FEATURE_NOT_SUPPORTED).map(|e| e.action),
            Some(Action::TurnOffFeature)
        );
    }

    #[test]
    fn feature_keys_are_unique_and_labelled_in_english() {
        let mut seen = std::collections::BTreeSet::new();
        for feature in FEATURES {
            assert!(seen.insert(feature.key), "{} is listed twice", feature.key);
            assert!(
                !feature.label.trim().is_empty(),
                "{} has no label",
                feature.key
            );
            assert!(
                feature
                    .key
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'_'),
                "{} must be a snake_case key",
                feature.key
            );
        }
    }

    #[test]
    fn the_wire_body_carries_code_message_retryable_action_and_feature() {
        let refusal = Refusal::feature_not_supported("web_search");
        let body = refusal.json_body();
        assert_eq!(body["error"]["code"], "zdr_feature_not_supported");
        assert_eq!(body["error"]["feature"], "web_search");
        assert_eq!(body["error"]["retryable"], false);
        assert_eq!(body["error"]["action"], "turn_off_feature");
        assert_eq!(
            body["error"]["message"],
            "This is not supported with zero data retention yet: web search and browsing. The request was refused before any model call."
        );

        let plain = Refusal::CONNECTION_NOT_SUPPORTED.json_body();
        assert!(
            plain["error"].get("feature").is_none(),
            "a plain code carries no feature"
        );
        assert_eq!(plain["error"]["code"], "zdr_connection_not_supported");
    }

    #[test]
    fn a_refusal_round_trips_through_the_grpc_trailers() {
        for refusal in [
            Refusal::UNAVAILABLE,
            Refusal::CONNECTION_NOT_SUPPORTED,
            Refusal::DURABLE_MUTATION_FORBIDDEN,
            Refusal::feature_not_supported("tool_execution"),
        ] {
            let status = refusal.grpc_status(Code::FailedPrecondition);
            assert_eq!(status.code(), Code::FailedPrecondition);
            assert_eq!(status.message(), refusal.message());
            assert_eq!(
                status
                    .metadata()
                    .get(KIND_TRAILER)
                    .and_then(|v| v.to_str().ok()),
                Some(refusal.code())
            );
            assert_eq!(
                Refusal::from_status(&status),
                Some(refusal),
                "{}",
                refusal.code()
            );
        }
    }

    #[test]
    fn a_feature_trailer_is_sent_only_with_a_feature() {
        let feature =
            Refusal::feature_not_supported("plan_mode").grpc_status(Code::PermissionDenied);
        assert_eq!(
            feature
                .metadata()
                .get(FEATURE_TRAILER)
                .and_then(|v| v.to_str().ok()),
            Some("plan_mode")
        );
        let plain = Refusal::POSTURE_MISMATCH.grpc_status(Code::PermissionDenied);
        assert!(plain.metadata().get(FEATURE_TRAILER).is_none());
    }

    #[test]
    fn foreign_kinds_and_incomplete_feature_refusals_are_not_zdr_refusals() {
        for kind in [
            "rate_limited",
            "residency_unavailable",
            "tier_unavailable",
            "too_long",
        ] {
            let mut metadata = MetadataMap::new();
            metadata.insert(KIND_TRAILER, kind.parse().expect("an ASCII kind"));
            let status = Status::with_metadata(Code::FailedPrecondition, "x", metadata);
            assert_eq!(Refusal::from_status(&status), None, "{kind}");
        }
        assert_eq!(
            Refusal::from_status(&Status::failed_precondition("no trailer")),
            None
        );

        // A feature code without a feature, or with a key outside the vocabulary,
        // is not typed: the caller falls back, it does not invent a feature.
        let no_feature = Status::with_metadata(Code::FailedPrecondition, "x", {
            let mut metadata = MetadataMap::new();
            metadata.insert(
                KIND_TRAILER,
                MetadataValue::from_static(code::FEATURE_NOT_SUPPORTED),
            );
            metadata
        });
        assert_eq!(Refusal::from_status(&no_feature), None);
        let unknown_feature = Status::with_metadata(Code::FailedPrecondition, "x", {
            let mut metadata = MetadataMap::new();
            metadata.insert(
                KIND_TRAILER,
                MetadataValue::from_static(code::FEATURE_NOT_SUPPORTED),
            );
            metadata.insert(FEATURE_TRAILER, MetadataValue::from_static("made_up"));
            metadata
        });
        assert_eq!(Refusal::from_status(&unknown_feature), None);
    }

    #[test]
    fn a_json_body_round_trips_and_a_flat_body_is_not_typed() {
        for refusal in [
            Refusal::CONNECTION_NOT_SUPPORTED,
            Refusal::DURABLE_MUTATION_FORBIDDEN,
            Refusal::feature_not_supported("browser_run"),
        ] {
            assert_eq!(Refusal::from_body(&refusal.json_body()), Some(refusal));
        }
        assert_eq!(
            Refusal::from_body(&json!({"error": "a sentence, no code"})),
            None
        );
        assert_eq!(
            Refusal::from_body(&json!({"error": {"code": "made_up_code", "message": "x"}})),
            None
        );
        assert_eq!(
            Refusal::from_body(
                &json!({"error": {"code": "zdr_feature_not_supported", "feature": "made_up"}})
            ),
            None
        );
    }

    #[test]
    fn the_agent_api_and_conversation_keys_are_refusals_that_round_trip() {
        for key in [
            "agent_api",
            "conversation_export",
            "conversation_share",
            "conversation_import",
        ] {
            let refusal = Refusal::feature_not_supported(key);
            assert_eq!(refusal.feature(), Some(key));
            assert_eq!(
                Refusal::from_body(&refusal.json_body()),
                Some(refusal),
                "{key}"
            );
            assert_eq!(
                Refusal::from_status(&refusal.grpc_status(Code::FailedPrecondition)),
                Some(refusal),
                "{key}"
            );
        }
        assert_eq!(
            Refusal::feature_not_supported("agent_api").message(),
            "This is not supported with zero data retention yet: the Agent API. The request was refused before any model call."
        );
    }

    #[test]
    fn a_refusal_is_built_only_from_a_catalogue_code_with_its_own_feature_shape() {
        assert_eq!(
            Refusal::new(code::FEATURE_NOT_SUPPORTED, Some("web_search")),
            Some(Refusal::feature_not_supported("web_search"))
        );
        assert_eq!(Refusal::new(code::FEATURE_NOT_SUPPORTED, None), None);
        assert_eq!(
            Refusal::new(code::FEATURE_NOT_SUPPORTED, Some("made_up")),
            None
        );
        assert_eq!(Refusal::new(code::UNAVAILABLE, Some("web_search")), None);
        assert_eq!(
            Refusal::new(code::UNAVAILABLE, None),
            Some(Refusal::UNAVAILABLE)
        );
        assert_eq!(Refusal::new("not_a_catalogue_code", None), None);
    }

    #[test]
    fn an_untyped_zdr_refusal_is_found_by_its_arguments_or_the_guard_above_it() {
        let source = r#"fn a() {
    return Err(Status::failed_precondition("ZDR tool execution is disabled"));
}
fn b(caller: &C) -> Result<(), Status> {
    if caller.zdr {
        return Err(Status::failed_precondition(
            "scheduled-step execution requires persistent service retention",
        ));
    }
    Ok(())
}
fn c() {
    return Err(Status::permission_denied("the route cannot serve a zero-data-retention run"));
}
fn d() {
    Err(Status::failed_precondition("approval is not durably granted"))
}
fn typed() {
    return Err(Refusal::UNAVAILABLE.grpc_status(Code::FailedPrecondition));
}
// Status::failed_precondition("ZDR in a comment")
#[cfg(test)]
mod tests { fn t() { let _ = Status::failed_precondition("zdr"); } }
fn after_tests() {
    return Err(Status::permission_denied("zdr_posture_mismatch"));
}
fn json_body() -> Value {
    json!({ "error": "durable mutation is unavailable under ZDR" })
}
fn typed_json() -> Value {
    json!({ "error": { "code": "zdr_durable_mutation_forbidden" } })
}
"#;
        assert_eq!(untyped_zdr_refusal_lines(source), vec![2, 6, 13, 25, 28]);
    }

    #[test]
    fn the_frozen_kinds_of_the_shared_status_contract_still_exist() {
        for kind in [
            "zdr_unavailable",
            "residency_unavailable",
            "tier_unavailable",
        ] {
            assert!(
                INFER_STATUSES.contains(&format!("\"{kind}\"")),
                "{kind} is frozen by CR-T3 and must stay in infer-statuses.json"
            );
        }
    }

    #[test]
    fn the_committed_fixture_is_what_the_catalogue_produces() {
        let generated = catalogue_text();
        if std::env::var("MP_UPDATE_GOLDEN").is_ok_and(|value| value == "1") {
            std::fs::write(FIXTURE_PATH, &generated).expect("write the ZDR refusal fixture");
            return;
        }
        assert!(
            !FIXTURE.contains('\r'),
            "the ZDR refusal fixture keeps LF line endings"
        );
        assert_eq!(
            FIXTURE, generated,
            "zdr-refusals.json differs from the catalogue. If the change is intended, regenerate \
             it with MP_UPDATE_GOLDEN=1 cargo test -p mp-contracts zdr_refusal and update the \
             pinned digest in scripts/tests/zdr-contract-sync.test.sh."
        );
    }
}
