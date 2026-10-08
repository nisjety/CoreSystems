//! P7 · Phase D — LLM action planner.
//!
//! The browser-agent loop's `plan_next_action` is deterministic (always
//! `Observe`). This module gives it a real brain: given the goal + the latest
//! `BrowserObservation`, it asks inference-core (gRPC `InferenceCore.Infer`,
//! already generated in `mp-contracts`) for the next browser action as JSON
//! constrained by a JSON Schema, and maps the result onto a `BrowserAction`.
//!
//! Real integration: this is a live gRPC call to inference-core (no mock). It
//! is gated by `QUARRY_BROWSER_AGENT_LLM=1`; when disabled the loop falls back
//! to the deterministic planner. inference-core requires an independently
//! verified, user-bound `aud=inference-core` bearer delegated from ingress;
//! without it the LLM planner is disabled before any downstream RPC. The address comes from
//! `INFERENCE_CORE_URL` / `INFERENCE_CORE_ADDR` (default `http://localhost:9092`).
//!
//! Cost governance (Phase 5): the planner does NOT pin a concrete model. It
//! defaults to a **Verevon intent mode** (`verevon-balance`) so inference-core's
//! Budget/Balance/Genius intent layer picks the model (complexity × the org's
//! budget posture), and it forwards the run's org as `x-org-id` gRPC metadata so
//! inference-core's cost-core budget guard counts and caps these calls — instead
//! of a hardcoded model that bypassed both. `QUARRY_BROWSER_AGENT_MODEL` still
//! overrides the model (e.g. to pin one when the intent layer is disabled).

// See quarry_agent.rs — scoped-allow the two low-signal doc pedantic lints.
#![allow(clippy::missing_errors_doc, clippy::doc_markdown)]

use mp_contracts::model_plane::v1::inference_core_client::InferenceCoreClient;
use mp_contracts::model_plane::v1::{ChatMessage, InferRequest};
use std::sync::Arc;
use tonic::transport::Channel;

use crate::browser_agent::{
    ActionType, BrowserAction, BrowserObservation, PlanConfig, RiskCategory,
};

/// JSON Schema describing the single next action the model must return.
///
/// `risk_category` (Phase 5 — HITL gates, plan capability #8) lets the model
/// self-report when an action it is about to choose looks like a login,
/// checkout, form submission, or other destructive/elevated-risk step — the
/// same self-report pattern already used for `reason`. This is deliberately
/// advisory, not the only signal: `classify_action_risk` in `browser_agent.rs`
/// ORs it with a deterministic keyword/URL backstop so a model that omits or
/// under-reports risk doesn't silently bypass the gate.
///
/// This is the schema the planner has always sent. Its optional fields and open
/// `additionalProperties` are NOT what OpenAI strict structured outputs accept
/// (the subscription broker marks its `json_schema` text format `strict`), so
/// whether the subscription route takes it is the E3 spike's question; the
/// ignored test `planner_on_terra_returns_valid_action_json` in
/// `tests/browser_agent_e2e.rs` answers it live. Until then this stays the
/// schema on every route, and [`ACTION_SCHEMA_STRICT`] is unused by `next_action`.
pub const ACTION_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "action": {"type": "string", "enum": ["navigate","click","type","scroll","wait","extract","done"]},
    "selector": {"type": "string", "description": "Legacy compatibility selector; prefer ref_id from the current Quarry snapshot."},
    "snapshot_id": {"type": "string"},
    "generation": {"type": "integer", "minimum": 0},
    "ref_id": {"type": "string", "description": "Opaque Quarry target ref such as @e1 from the current observation."},
    "frame_id": {"type": "string", "description": "Required only when the selected snapshot target reports a child-frame id; echo it exactly and never invent one."},
    "value": {"type": "string"},
    "url": {"type": "string"},
    "reason": {"type": "string"},
    "risk_category": {"type": "string", "enum": ["login","checkout","posting_form","destructive","cross_domain_navigation","persistent_cookie_use","none"]}
  },
  "required": ["action"]
}"#;

/// Strict-mode variant of [`ACTION_SCHEMA`] for the ChatGPT subscription route
/// (E3 spike; not wired into `next_action` unless the live probe shows the plain
/// schema is rejected or answered with prose).
///
/// OpenAI strict structured outputs need every property listed in `required`
/// and `additionalProperties: false`, and have no optional fields. An unused
/// field is therefore sent as a sentinel instead of being left out: `""` for
/// every string, `-1` for `generation`, and `"none"` for `risk_category`.
/// `NextAction` already reads all of those as "not supplied" (an empty string is
/// absent, `generation: -1` is `None`, `"none"` is no self-reported risk), so a
/// strict reply maps onto the same `BrowserAction` as a plain one. `generation`
/// carries no `minimum`, because the `-1` sentinel is below it.
///
/// Keep the keys, the `action` enum and the `risk_category` enum in step with
/// [`ACTION_SCHEMA`]: a unit test fails when the two vocabularies drift.
pub const ACTION_SCHEMA_STRICT: &str = r#"{
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "action": {"type": "string", "enum": ["navigate","click","type","scroll","wait","extract","done"]},
    "selector": {"type": "string", "description": "Legacy CSS selector. Use an empty string unless no snapshot ref can express the target; prefer ref_id."},
    "snapshot_id": {"type": "string", "description": "snapshot_id of the latest observation when targeting a ref, echoed exactly. Otherwise an empty string."},
    "generation": {"type": "integer", "description": "generation of the latest observation when targeting a ref, echoed exactly. Otherwise -1."},
    "ref_id": {"type": "string", "description": "Opaque Quarry target ref such as @e1 from the latest observation. Otherwise an empty string."},
    "frame_id": {"type": "string", "description": "The target's child-frame id when the observation reports one, echoed exactly and never invented. Otherwise an empty string."},
    "value": {"type": "string", "description": "Text to type for action=type. Otherwise an empty string."},
    "url": {"type": "string", "description": "Destination for action=navigate. Otherwise an empty string."},
    "reason": {"type": "string", "description": "One short sentence on why this action. May be an empty string."},
    "risk_category": {"type": "string", "enum": ["login","checkout","posting_form","destructive","cross_domain_navigation","persistent_cookie_use","none"]}
  },
  "required": ["action","selector","snapshot_id","generation","ref_id","frame_id","value","url","reason","risk_category"]
}"#;

/// Default planner model — a **Verevon intent mode**, NOT a pinned id, so the
/// browser-agent planner routes through inference-core's Budget/Balance/Genius
/// intent layer + cost-core budget guard instead of bypassing them.
const DEFAULT_MODEL: &str = "verevon-balance";
const DEFAULT_ADDR: &str = "http://localhost:9092";

/// Resolve the planner model: an explicit non-empty `QUARRY_BROWSER_AGENT_MODEL`
/// wins (lets ops pin a concrete model when the intent layer is disabled);
/// otherwise the Verevon intent-mode default so the call participates in
/// budget-aware model selection.
fn resolve_planner_model() -> String {
    std::env::var("QUARRY_BROWSER_AGENT_MODEL")
        .ok()
        .map(|m| m.trim().to_owned())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| DEFAULT_MODEL.to_owned())
}

/// gRPC planner backed by inference-core.
#[derive(Clone)]
pub struct LlmPlanner {
    client: InferenceCoreClient<Channel>,
    model: String,
    inference_bearer: Arc<str>,
}

impl LlmPlanner {
    /// Build from the environment. Gated by `QUARRY_BROWSER_AGENT_LLM`; returns
    /// `None` when disabled or the endpoint URL is unparseable, so the loop can
    /// fall back to the deterministic planner.
    #[must_use]
    pub fn from_env(inference_bearer: Option<&str>) -> Option<Self> {
        let enabled = std::env::var("QUARRY_BROWSER_AGENT_LLM")
            .map(|v| matches!(v.trim(), "1" | "true" | "TRUE" | "yes"))
            .unwrap_or(false);
        if !enabled {
            return None;
        }
        let inference_bearer = inference_bearer
            .map(str::trim)
            .filter(|value| !value.is_empty() && !value.chars().any(char::is_whitespace))?;
        let url = std::env::var("INFERENCE_CORE_URL")
            .or_else(|_| std::env::var("INFERENCE_CORE_ADDR"))
            .unwrap_or_else(|_| DEFAULT_ADDR.to_owned());
        let channel = Channel::from_shared(url).ok()?.connect_lazy();
        let model = resolve_planner_model();
        Some(Self {
            client: InferenceCoreClient::new(channel),
            model,
            inference_bearer: Arc::from(inference_bearer),
        })
    }

    /// Ask the model for the next action. `Ok(None)` means the model reported
    /// the task complete (`action == "done"`).
    pub async fn next_action(
        &self,
        config: &PlanConfig,
        observation: Option<&BrowserObservation>,
    ) -> Result<Option<BrowserAction>, String> {
        let request = planner_infer_request(config, observation, &self.model, ACTION_SCHEMA);

        let mut grpc_request = tonic::Request::new(request);
        grpc_request.metadata_mut().insert(
            "authorization",
            format!("Bearer {}", self.inference_bearer)
                .parse()
                .map_err(|_| "verified inference credential is not forwardable".to_owned())?,
        );
        let response = self
            .client
            .clone()
            .infer(grpc_request)
            .await
            .map_err(|e| format!("inference infer rpc failed: {e}"))?
            .into_inner();

        let parsed: NextAction = serde_json::from_str(response.content.trim()).map_err(|e| {
            format!(
                "could not parse action JSON ({e}); content={}",
                response.content
            )
        })?;
        Ok(parsed.into_browser_action())
    }
}

/// The `InferRequest` the planner sends for one step: the goal and the latest
/// observation as the prompt, the given `model`, and `structured_output_schema`
/// (pass [`ACTION_SCHEMA`], or [`ACTION_SCHEMA_STRICT`] for the subscription
/// route once the E3 spike calls for it).
///
/// The route fields (`provider_hint`, `subscription_connection_id`) are left
/// empty, which is the default route; a caller that has an explicit route sets
/// them on the returned request. `next_action` builds its request here, and so
/// does the live spike test, so the probe sends exactly what the planner sends.
#[must_use]
pub fn planner_infer_request(
    config: &PlanConfig,
    observation: Option<&BrowserObservation>,
    model: &str,
    structured_output_schema: &str,
) -> InferRequest {
    let observation_text = observation.map_or_else(
        || "No observation yet — this is the first step.".to_owned(),
        |o| {
            let targets = o
                .snapshot_targets
                .iter()
                .take(40)
                .map(|target| {
                    format!(
                        "  {} role={} name={} text={} frame_id={}",
                        target.ref_id,
                        target.role.as_deref().unwrap_or(""),
                        target.name.as_deref().unwrap_or(""),
                        target.text.as_deref().unwrap_or(""),
                        target.frame_id.as_deref().unwrap_or(""),
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "Current page:\n  url: {}\n  title: {}\n  extracted_text: {}\n  snapshot_id: {}\n  generation: {}\n  targets:\n{}",
                o.page_url,
                o.page_title,
                o.extracted_text,
                o.dom_snapshot_ref,
                o.snapshot_generation.map_or_else(String::new, |generation| generation.to_string()),
                targets,
            )
        },
    );
    let system = format!(
        "You are an autonomous web-browsing agent. Goal: {}\n\
         Choose the SINGLE next browser action and return ONLY JSON matching the schema. \
         Use action=\"done\" when the goal is satisfied. \
         For navigate set url. For click/type, prefer the exact snapshot_id, generation, and ref_id from the latest observation; these opaque refs are the only safe way to target a dynamic page. When a target reports a child frame_id, echo that exact frame_id too; Quarry will reject a cross-frame action without it. Use CSS selector only as legacy compatibility when no current snapshot target can express the intent. For type also set value.",
        config.system_prompt
    );

    InferRequest {
        request_id: mp_ids::new_ulid(),
        org_id: config.org_id.clone(),
        model: model.to_owned(),
        messages: vec![
            ChatMessage {
                compaction_summary: String::new(),
                role: "system".to_owned(),
                content: system,
                ..Default::default()
            },
            ChatMessage {
                compaction_summary: String::new(),
                role: "user".to_owned(),
                content: observation_text,
                ..Default::default()
            },
        ],
        temperature: 0.2,
        max_tokens: 512,
        structured_output_schema: structured_output_schema.to_owned(),
        ..Default::default()
    }
}

#[derive(Debug, serde::Deserialize)]
struct NextAction {
    action: String,
    #[serde(default)]
    selector: String,
    #[serde(default)]
    snapshot_id: String,
    /// `ACTION_SCHEMA_STRICT` sends `-1` for "no generation"; that reads as
    /// `None`, exactly like an absent field.
    #[serde(default, deserialize_with = "generation_or_sentinel")]
    generation: Option<u32>,
    #[serde(default)]
    ref_id: String,
    #[serde(default)]
    frame_id: String,
    #[serde(default)]
    value: String,
    #[serde(default)]
    url: String,
    /// The model's rationale for choosing this action. Requested by
    /// `ACTION_SCHEMA` but previously discarded — now carried onto
    /// `BrowserAction.reason` so it reaches the run-event stream (Phase 2).
    #[serde(default)]
    reason: String,
    /// The model's self-reported risk classification (Phase 5). Free-form on
    /// the wire (`#[serde(default)]`, no enum) so an unrecognized or absent
    /// value degrades to "no self-reported risk" rather than a parse error —
    /// `classify_action_risk`'s deterministic backstop still runs regardless.
    #[serde(default)]
    risk_category: String,
}

impl NextAction {
    /// Map the model's choice onto a `BrowserAction`. `action_id`/`grant_id` are
    /// filled in by the caller from the plan's gate decision. `None` ⇒ done.
    fn into_browser_action(self) -> Option<BrowserAction> {
        let action_type = match self.action.as_str() {
            "navigate" => ActionType::Goto,
            "click" => ActionType::Click,
            "type" => ActionType::Type,
            "scroll" => ActionType::Scroll,
            "wait" => ActionType::Wait,
            "extract" => ActionType::Extract,
            "done" => return None,
            // Unknown / "observe" → look at the page again.
            _ => ActionType::Observe,
        };
        let target_components_present = !self.snapshot_id.is_empty()
            || self.generation.is_some()
            || !self.ref_id.is_empty()
            || !self.frame_id.is_empty();
        let target = match (
            self.generation,
            self.snapshot_id.is_empty(),
            self.ref_id.is_empty(),
        ) {
            (Some(generation), false, false) => Some(crate::browser_agent::BrowserTargetRef {
                snapshot_id: self.snapshot_id,
                generation,
                ref_id: self.ref_id,
                frame_id: (!self.frame_id.trim().is_empty()).then(|| self.frame_id),
            }),
            _ => None,
        };
        // A planner that supplied a partial opaque binding must not silently
        // fall back to a CSS action that might affect a different element.
        // An empty selector makes the legacy driver reject/re-observe instead.
        let selector = if target_components_present && target.is_none() {
            String::new()
        } else {
            self.selector
        };
        Some(BrowserAction {
            action_id: String::new(),
            grant_id: String::new(),
            action_type,
            selector,
            value: self.value,
            url: self.url,
            max_wait_ms: 5000,
            target,
            reason: self.reason,
            risk_category: RiskCategory::from_wire(&self.risk_category),
        })
    }
}

/// Reads `generation`, treating exactly the strict schema's `-1` sentinel as
/// "not supplied". Any other negative number, or a value above `u32::MAX`, is
/// still a parse error, as before.
fn generation_or_sentinel<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error as _;
    use serde::Deserialize as _;

    match Option::<i64>::deserialize(deserializer)? {
        None | Some(-1) => Ok(None),
        Some(generation) => u32::try_from(generation)
            .map(Some)
            .map_err(D::Error::custom),
    }
}

/// How [`parse_action_content`] had to read a planner reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionParse {
    /// The whole reply was one JSON object: how `next_action` reads a reply today.
    Exact,
    /// The reply needed the tolerant extractor: code fences, prose around the
    /// object, or trailing commas.
    Tolerant,
}

/// Reads a planner reply into the next action: `Ok(None)` is `done`.
///
/// A reply that is exactly one JSON object is read as-is ([`ActionParse::Exact`]).
/// Anything else goes through [`extract_action_json`] ([`ActionParse::Tolerant`]).
/// `next_action` does not use this yet: it still reads the reply as-is. It is
/// here for the E3 spike, whose live probe reports which of the two a Terra
/// reply needed, and which `next_action` should adopt only if that says so.
///
/// The error text never carries the reply.
pub fn parse_action_content(
    content: &str,
) -> Result<(Option<BrowserAction>, ActionParse), String> {
    let trimmed = content.trim();
    if let Ok(action) = serde_json::from_str::<NextAction>(trimmed) {
        return Ok((action.into_browser_action(), ActionParse::Exact));
    }
    let json = extract_action_json(trimmed)
        .ok_or_else(|| "no complete JSON object in the planner reply".to_owned())?;
    let action = serde_json::from_str::<NextAction>(&json).map_err(|e| {
        format!(
            "could not parse action JSON ({:?} error at column {})",
            e.classify(),
            e.column()
        )
    })?;
    Ok((action.into_browser_action(), ActionParse::Tolerant))
}

/// The first complete JSON object in `raw`, with trailing commas removed.
///
/// This is the tolerant half of reading a model reply that should have been
/// bare JSON: it skips a Markdown code fence (with or without a language tag)
/// and any prose before or after the object, because it only looks at the
/// braces. Braces, commas and quotes inside string values do not confuse it.
/// A balanced `{...}`
/// that is not JSON (such as `{braces}` in a sentence) is skipped; a `{` that
/// never closes (a reply cut off by the token limit) gives `None`.
pub fn extract_action_json(raw: &str) -> Option<String> {
    let mut search_from = 0;
    while let Some(relative) = raw[search_from..].find('{') {
        let start = search_from + relative;
        let end = balanced_object_end(raw, start)?;
        let candidate = strip_trailing_commas(&raw[start..end]);
        if serde_json::from_str::<serde_json::Value>(&candidate).is_ok_and(|v| v.is_object()) {
            return Some(candidate);
        }
        search_from = end;
    }
    None
}

/// The byte offset just past the `}` that closes the `{` at `start`, ignoring
/// braces inside JSON strings. `None` when the object never closes.
fn balanced_object_end(text: &str, start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, ch) in text[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(start + offset + ch.len_utf8());
                }
            }
            _ => {}
        }
    }
    None
}

/// `json` without any `,` that is followed (past whitespace) by `}` or `]`.
/// Commas inside JSON strings are left alone.
fn strip_trailing_commas(json: &str) -> String {
    let chars: Vec<char> = json.chars().collect();
    let mut out = String::with_capacity(json.len());
    let mut in_string = false;
    let mut escaped = false;
    for (index, &ch) in chars.iter().enumerate() {
        if in_string {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
        } else if ch == ',' {
            let next = chars[index + 1..].iter().find(|c| !c.is_whitespace());
            if matches!(next, Some('}' | ']')) {
                continue;
            }
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> NextAction {
        serde_json::from_str(json).expect("valid action json")
    }

    #[test]
    fn navigate_maps_to_goto_with_url() {
        let action = parse(r#"{"action":"navigate","url":"https://example.com"}"#)
            .into_browser_action()
            .expect("navigate is not terminal");
        assert_eq!(action.action_type, ActionType::Goto);
        assert_eq!(action.url, "https://example.com");
    }

    #[test]
    fn click_maps_to_click_with_selector() {
        let action = parse(r#"{"action":"click","selector":"button.submit"}"#)
            .into_browser_action()
            .expect("click is not terminal");
        assert_eq!(action.action_type, ActionType::Click);
        assert_eq!(action.selector, "button.submit");
    }

    #[test]
    fn type_maps_to_type_with_value() {
        // Double-hash raw delimiter so the `#` in the CSS id selector doesn't
        // terminate the string.
        let action = parse(r##"{"action":"type","selector":"#q","value":"rust lang"}"##)
            .into_browser_action()
            .expect("type is not terminal");
        assert_eq!(action.action_type, ActionType::Type);
        assert_eq!(action.value, "rust lang");
        assert_eq!(action.selector, "#q");
    }

    #[test]
    fn done_is_terminal() {
        assert!(parse(r#"{"action":"done"}"#)
            .into_browser_action()
            .is_none());
    }

    #[test]
    fn unknown_action_falls_back_to_observe() {
        // A hallucinated/unsupported action degrades to a safe re-observe
        // rather than failing the loop.
        let action = parse(r#"{"action":"frobnicate"}"#)
            .into_browser_action()
            .expect("unknown maps to observe, not terminal");
        assert_eq!(action.action_type, ActionType::Observe);
    }

    #[test]
    fn missing_required_action_is_a_parse_error() {
        // `action` is required by the schema; absent it, parsing must fail so
        // the caller degrades to the deterministic planner.
        assert!(serde_json::from_str::<NextAction>(r#"{"selector":"x"}"#).is_err());
    }

    #[test]
    fn reason_is_captured_onto_the_browser_action() {
        // Phase 2: `reason` (present in ACTION_SCHEMA) is no longer discarded —
        // it flows onto `BrowserAction.reason` for the run-event stream.
        let action = parse(r#"{"action":"scroll","reason":"need to see more"}"#)
            .into_browser_action()
            .expect("scroll is not terminal");
        assert_eq!(action.action_type, ActionType::Scroll);
        assert_eq!(action.reason, "need to see more");
    }

    #[test]
    fn reason_defaults_to_empty_when_absent() {
        let action = parse(r#"{"action":"scroll"}"#)
            .into_browser_action()
            .expect("scroll is not terminal");
        assert_eq!(action.reason, "");
    }

    #[test]
    fn risk_category_self_report_is_captured_onto_the_browser_action() {
        // Phase 5: the model can self-report a risk classification alongside
        // its rationale — carried onto `BrowserAction.risk_category` so the
        // in-loop HITL gate can use it (ORed with the deterministic backstop).
        let action = parse(r##"{"action":"click","selector":"#pay","risk_category":"checkout"}"##)
            .into_browser_action()
            .expect("click is not terminal");
        assert_eq!(action.risk_category, Some(RiskCategory::Checkout));
    }

    #[test]
    fn risk_category_defaults_to_none_when_absent_or_unrecognized() {
        let absent = parse(r#"{"action":"scroll"}"#)
            .into_browser_action()
            .expect("scroll is not terminal");
        assert_eq!(absent.risk_category, None);

        let none_value = parse(r#"{"action":"scroll","risk_category":"none"}"#)
            .into_browser_action()
            .expect("scroll is not terminal");
        assert_eq!(none_value.risk_category, None);

        let hallucinated = parse(r#"{"action":"scroll","risk_category":"frobnicate"}"#)
            .into_browser_action()
            .expect("scroll is not terminal");
        assert_eq!(hallucinated.risk_category, None);
    }

    #[test]
    fn truly_unknown_fields_are_still_ignored() {
        // Any field NextAction doesn't declare must be ignored, not rejected.
        let action = parse(r#"{"action":"scroll","confidence":0.9,"unused":"x"}"#)
            .into_browser_action()
            .expect("scroll is not terminal");
        assert_eq!(action.action_type, ActionType::Scroll);
    }

    #[test]
    fn planner_model_resolution_defaults_to_verevon_intent_mode() {
        // Phase 5: the default must be a Verevon intent mode (routes through the
        // Budget/Balance/Genius selection + budget guard), NOT a pinned model.
        // Sequential (not two tests) to avoid racing on the shared env var.
        std::env::remove_var("QUARRY_BROWSER_AGENT_MODEL");
        assert_eq!(resolve_planner_model(), "verevon-balance");
        assert_eq!(DEFAULT_MODEL, "verevon-balance");

        // An explicit override is honored (ops pin a model when intent is off).
        std::env::set_var("QUARRY_BROWSER_AGENT_MODEL", "gpt-4o-mini");
        assert_eq!(resolve_planner_model(), "gpt-4o-mini");

        // A blank override falls back to the intent-mode default.
        std::env::set_var("QUARRY_BROWSER_AGENT_MODEL", "   ");
        assert_eq!(resolve_planner_model(), "verevon-balance");

        std::env::remove_var("QUARRY_BROWSER_AGENT_MODEL");
    }

    // ---- E3 spike: strict schema variant, tolerant extractor, request builder.

    use crate::browser_agent::{BrowserSnapshotTarget, BrowserTargetRef, ObservationStatus};
    use serde_json::{json, Value};
    use std::collections::BTreeSet;

    fn schema_value(raw: &str) -> Value {
        serde_json::from_str(raw).expect("schema is valid JSON")
    }

    fn property_names(schema: &Value) -> BTreeSet<String> {
        schema["properties"]
            .as_object()
            .expect("properties object")
            .keys()
            .cloned()
            .collect()
    }

    fn enum_values(schema: &Value, property: &str) -> Vec<String> {
        schema["properties"][property]["enum"]
            .as_array()
            .expect("enum array")
            .iter()
            .map(|member| member.as_str().expect("string enum member").to_owned())
            .collect()
    }

    /// A reply as a strict-mode model has to produce it: every property of
    /// `ACTION_SCHEMA_STRICT` present, the unused ones as the schema's
    /// sentinels, then `overrides` laid over them.
    fn strict_reply(action: &str, overrides: &Value) -> String {
        let schema = schema_value(ACTION_SCHEMA_STRICT);
        let mut reply = serde_json::Map::new();
        for (key, spec) in schema["properties"].as_object().expect("properties object") {
            let kind = spec["type"].as_str().expect("typed property");
            let value = match (key.as_str(), kind) {
                ("action", _) => json!(action),
                ("risk_category", _) => json!("none"),
                (_, "string") => json!(""),
                (_, "integer") => json!(-1),
                (key, kind) => panic!("no sentinel for {key}: {kind}"),
            };
            reply.insert(key.clone(), value);
        }
        for (key, value) in overrides.as_object().expect("overrides object") {
            assert!(
                reply.contains_key(key),
                "override for a key the strict schema lacks: {key}"
            );
            reply.insert(key.clone(), value.clone());
        }
        Value::Object(reply).to_string()
    }

    fn strict_action(action: &str, overrides: &Value) -> BrowserAction {
        parse(&strict_reply(action, overrides))
            .into_browser_action()
            .expect("not a terminal action")
    }

    #[test]
    fn strict_schema_is_a_strict_mode_schema() {
        let schema = schema_value(ACTION_SCHEMA_STRICT);
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["additionalProperties"], json!(false));
        let required: Vec<String> = schema["required"]
            .as_array()
            .expect("required array")
            .iter()
            .map(|key| key.as_str().expect("string key").to_owned())
            .collect();
        let required_set: BTreeSet<String> = required.iter().cloned().collect();
        assert_eq!(required.len(), required_set.len(), "a key is required twice");
        assert_eq!(
            required_set,
            property_names(&schema),
            "strict mode needs every property listed in `required`"
        );
        for (key, spec) in schema["properties"].as_object().expect("properties object") {
            let kind = spec["type"]
                .as_str()
                .unwrap_or_else(|| panic!("{key} has no plain type"));
            assert!(
                matches!(kind, "string" | "integer"),
                "{key}: {kind} would need an optional or nested form"
            );
            assert!(
                spec.get("minimum").is_none(),
                "{key}: a minimum conflicts with the -1 sentinel"
            );
        }
    }

    #[test]
    fn strict_schema_keeps_the_plain_schema_vocabulary() {
        let plain = schema_value(ACTION_SCHEMA);
        let strict = schema_value(ACTION_SCHEMA_STRICT);
        assert_eq!(
            property_names(&strict),
            property_names(&plain),
            "a field added to one schema must be added to the other"
        );
        assert_eq!(enum_values(&strict, "action"), enum_values(&plain, "action"));
        assert_eq!(
            enum_values(&strict, "risk_category"),
            enum_values(&plain, "risk_category")
        );
        for (key, spec) in plain["properties"].as_object().expect("properties object") {
            assert_eq!(strict["properties"][key]["type"], spec["type"], "{key}");
        }
    }

    #[test]
    fn strict_replies_map_to_the_right_action_for_every_action_kind() {
        let cases: [(&str, Value, Option<ActionType>); 7] = [
            (
                "navigate",
                json!({"url": "https://example.com", "reason": "open the site"}),
                Some(ActionType::Goto),
            ),
            (
                "click",
                json!({"snapshot_id": "snap-1", "generation": 3, "ref_id": "@e2"}),
                Some(ActionType::Click),
            ),
            (
                "type",
                json!({"snapshot_id": "snap-1", "generation": 0, "ref_id": "@e1", "value": "rust lang"}),
                Some(ActionType::Type),
            ),
            ("scroll", json!({}), Some(ActionType::Scroll)),
            ("wait", json!({}), Some(ActionType::Wait)),
            ("extract", json!({}), Some(ActionType::Extract)),
            ("done", json!({"reason": "goal met"}), None),
        ];
        // A new action in the schema must come with a case here.
        let covered: Vec<String> = cases.iter().map(|(action, _, _)| (*action).to_owned()).collect();
        assert_eq!(
            covered,
            enum_values(&schema_value(ACTION_SCHEMA_STRICT), "action")
        );
        for (action, overrides, expected) in cases {
            let mapped = parse(&strict_reply(action, &overrides)).into_browser_action();
            match expected {
                None => assert!(mapped.is_none(), "{action} is terminal"),
                Some(kind) => {
                    let mapped = mapped.unwrap_or_else(|| panic!("{action} must not be terminal"));
                    assert_eq!(mapped.action_type, kind, "{action}");
                }
            }
        }
    }

    #[test]
    fn strict_navigate_carries_its_url_and_reason_and_no_target() {
        let action = strict_action(
            "navigate",
            &json!({"url": "https://example.com", "reason": "open the site"}),
        );
        assert_eq!(action.url, "https://example.com");
        assert_eq!(action.reason, "open the site");
        assert_eq!(action.target, None);
        assert_eq!(action.selector, "");
        assert_eq!(action.risk_category, None);
    }

    #[test]
    fn strict_click_by_ref_keeps_the_opaque_binding() {
        let action = strict_action(
            "click",
            &json!({"snapshot_id": "snap-1", "generation": 3, "ref_id": "@e2"}),
        );
        assert_eq!(
            action.target,
            Some(BrowserTargetRef {
                snapshot_id: "snap-1".to_owned(),
                generation: 3,
                ref_id: "@e2".to_owned(),
                frame_id: None,
            })
        );
        assert_eq!(action.selector, "");
    }

    #[test]
    fn strict_type_in_a_child_frame_keeps_the_frame_and_generation_zero() {
        // Generation 0 is a real generation, not the -1 sentinel.
        let action = strict_action(
            "type",
            &json!({
                "snapshot_id": "snap-1", "generation": 0, "ref_id": "@e1",
                "frame_id": "frame-9", "value": "rust lang"
            }),
        );
        assert_eq!(action.value, "rust lang");
        assert_eq!(
            action.target,
            Some(BrowserTargetRef {
                snapshot_id: "snap-1".to_owned(),
                generation: 0,
                ref_id: "@e1".to_owned(),
                frame_id: Some("frame-9".to_owned()),
            })
        );
    }

    #[test]
    fn strict_sentinels_alone_leave_no_target_and_no_selector() {
        let action = strict_action("click", &json!({}));
        assert_eq!(action.action_type, ActionType::Click);
        assert_eq!(action.target, None);
        assert_eq!(action.selector, "");
    }

    #[test]
    fn strict_sentinel_generation_still_blanks_a_partial_binding() {
        // The model named a snapshot and a ref but sent the -1 sentinel for the
        // generation: that is a partial opaque binding, which must not fall
        // back to the CSS selector it also sent.
        let action = strict_action(
            "click",
            &json!({"snapshot_id": "snap-1", "ref_id": "@e2", "selector": "button.pay"}),
        );
        assert_eq!(action.target, None);
        assert_eq!(action.selector, "");
    }

    #[test]
    fn strict_risk_category_self_report_survives_and_none_is_no_report() {
        let checkout = strict_action("click", &json!({"risk_category": "checkout"}));
        assert_eq!(checkout.risk_category, Some(RiskCategory::Checkout));
        let none = strict_action("click", &json!({}));
        assert_eq!(none.risk_category, None);
    }

    #[test]
    fn the_minus_one_sentinel_reads_as_absent_and_zero_is_a_generation() {
        let generation = |raw: &str| parse(raw).generation;
        assert_eq!(generation(r#"{"action":"click","generation":-1}"#), None);
        // Only the documented sentinel is tolerated; any other negative stays
        // a parse error, as it was before the strict variant existed.
        assert!(
            serde_json::from_str::<NextAction>(r#"{"action":"click","generation":-5}"#).is_err()
        );
        assert_eq!(generation(r#"{"action":"click","generation":0}"#), Some(0));
        assert_eq!(generation(r#"{"action":"click","generation":7}"#), Some(7));
        assert_eq!(generation(r#"{"action":"click","generation":null}"#), None);
        assert_eq!(generation(r#"{"action":"click"}"#), None);
        assert!(
            serde_json::from_str::<NextAction>(r#"{"action":"click","generation":4294967296}"#)
                .is_err()
        );
    }

    #[test]
    fn a_bare_json_reply_is_read_exactly() {
        let (action, mode) = parse_action_content(
            "  \n{\"action\":\"navigate\",\"url\":\"https://example.com\"}\n",
        )
        .expect("parses");
        assert_eq!(mode, ActionParse::Exact);
        assert_eq!(action.expect("navigate").action_type, ActionType::Goto);
    }

    #[test]
    fn fenced_replies_are_read_tolerantly() {
        let object = "{\"action\":\"navigate\",\"url\":\"https://example.com\"}";
        for reply in [
            format!("```json\n{object}\n```"),
            format!("```\n{object}\n```"),
            format!("Here is the next action:\n```json\n{object}\n```\nLet me know."),
            format!("The next step is {object} and then we are done."),
        ] {
            let (action, mode) = parse_action_content(&reply)
                .unwrap_or_else(|e| panic!("{reply}: {e}"));
            assert_eq!(mode, ActionParse::Tolerant, "{reply}");
            let action = action.expect("navigate is not terminal");
            assert_eq!(action.action_type, ActionType::Goto, "{reply}");
            assert_eq!(action.url, "https://example.com", "{reply}");
        }
    }

    #[test]
    fn trailing_commas_are_removed_outside_strings_only() {
        assert_eq!(
            extract_action_json("{\"a\":[1,2,],\"b\":2,}").as_deref(),
            Some("{\"a\":[1,2],\"b\":2}")
        );
        // Fenced, with a trailing comma, and a value that itself holds `,}`,
        // `{` and a comma before a bracket: only the real trailing comma goes.
        let reply = "```json\n{\"action\":\"type\",\"value\":\"keep,} {this ,]\",\"reason\":\"r\",\n}\n```";
        let (action, mode) = parse_action_content(reply).expect("parses");
        assert_eq!(mode, ActionParse::Tolerant);
        assert_eq!(action.expect("type").value, "keep,} {this ,]");
    }

    #[test]
    fn braces_and_escaped_quotes_inside_strings_do_not_end_the_object() {
        let reply = "Sure: {\"action\":\"type\",\"value\":\"say \\\"hi\\\" }\",\"reason\":\"x\"} done";
        let (action, mode) = parse_action_content(reply).expect("parses");
        assert_eq!(mode, ActionParse::Tolerant);
        assert_eq!(action.expect("type").value, "say \"hi\" }");
    }

    #[test]
    fn a_balanced_non_json_brace_pair_is_skipped_and_the_first_real_object_wins() {
        let reply = "Use {braces} like so: {\"action\":\"done\"} then {\"action\":\"navigate\",\"url\":\"https://a.example\"}";
        let (action, mode) = parse_action_content(reply).expect("parses");
        assert_eq!(mode, ActionParse::Tolerant);
        assert!(action.is_none(), "the first object is `done`");
    }

    #[test]
    fn replies_without_a_complete_action_object_are_errors() {
        for reply in [
            "",
            "I cannot help with that.",
            "[]",
            // Cut off by the token limit.
            "{\"action\":\"navigate\",\"url\":\"https://exa",
            // Balanced but not JSON.
            "{\"action\": navigate}",
            // JSON, but not an action.
            "{\"note\":\"no action key\"}",
        ] {
            assert!(
                parse_action_content(reply).is_err(),
                "{reply:?} must not read as an action"
            );
        }
    }

    #[test]
    fn a_parse_error_does_not_carry_the_reply() {
        let secret = "https://private.example/token-abc123";
        for reply in [
            format!("{{\"action\":\"navigate\",\"url\":\"{secret}\" oops}}"),
            format!("{{\"url\":\"{secret}\"}}"),
            format!("{{\"action\":\"click\",\"generation\":\"{secret}\"}}"),
        ] {
            let error = parse_action_content(&reply).expect_err("not an action");
            assert!(!error.contains("private.example"), "{error}");
            assert!(!error.contains("token-abc123"), "{error}");
        }
    }

    fn test_config() -> PlanConfig {
        PlanConfig {
            plan_id: "plan_1".to_owned(),
            grant_id: "grant_1".to_owned(),
            run_id: "run_1".to_owned(),
            org_id: "org_x".to_owned(),
            system_prompt: "Find the opening hours.".to_owned(),
            max_steps: 8,
            max_runtime_s: 60,
            allowed_domains: vec!["example.com".to_owned()],
            stop_criteria: String::new(),
            require_approval: false,
            max_cost_usd: None,
            zdr: false,
            profile_id: None,
            start_url: None,
            postcondition: String::new(),
        }
    }

    fn test_observation() -> BrowserObservation {
        BrowserObservation {
            observation_id: "obs_1".to_owned(),
            action_id: "act_1".to_owned(),
            grant_id: "grant_1".to_owned(),
            status: ObservationStatus::Success,
            page_url: "https://example.com/".to_owned(),
            page_title: "Example Domain".to_owned(),
            extracted_text: "Example Domain".to_owned(),
            screenshot_ref: String::new(),
            dom_snapshot_ref: "snap-9".to_owned(),
            snapshot_generation: Some(2),
            snapshot_targets: vec![BrowserSnapshotTarget {
                ref_id: "@e1".to_owned(),
                role: Some("link".to_owned()),
                name: Some("More information".to_owned()),
                text: None,
                frame_id: None,
            }],
            error_message: String::new(),
        }
    }

    #[test]
    fn planner_request_is_on_the_default_route_with_the_given_model_and_schema() {
        let request = planner_infer_request(&test_config(), None, DEFAULT_MODEL, ACTION_SCHEMA);
        assert_eq!(request.org_id, "org_x");
        assert_eq!(request.model, "verevon-balance");
        // No route fields: a caller with an explicit route sets them itself.
        assert_eq!(request.provider_hint, "");
        assert_eq!(request.subscription_connection_id, "");
        assert_eq!(request.structured_output_schema, ACTION_SCHEMA);
        assert!((request.temperature - 0.2).abs() < f32::EPSILON);
        assert_eq!(request.max_tokens, 512);
        assert!(!request.request_id.is_empty());
        assert_eq!(request.messages.len(), 2);
        assert_eq!(request.messages[0].role, "system");
        assert!(request.messages[0]
            .content
            .contains("Goal: Find the opening hours."));
        assert_eq!(request.messages[1].role, "user");
        assert!(request.messages[1].content.contains("first step"));
    }

    #[test]
    fn planner_request_carries_the_strict_schema_and_the_observation_refs() {
        let request = planner_infer_request(
            &test_config(),
            Some(&test_observation()),
            "gpt-5.6-terra",
            ACTION_SCHEMA_STRICT,
        );
        assert_eq!(request.model, "gpt-5.6-terra");
        assert_eq!(request.structured_output_schema, ACTION_SCHEMA_STRICT);
        let user = &request.messages[1].content;
        assert!(user.contains("snapshot_id: snap-9"), "{user}");
        assert!(user.contains("generation: 2"), "{user}");
        assert!(user.contains("@e1 role=link name=More information"), "{user}");
    }
}
