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
const ACTION_SCHEMA: &str = r#"{
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

        let request = InferRequest {
            request_id: mp_ids::new_ulid(),
            org_id: config.org_id.clone(),
            model: self.model.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".to_owned(),
                    content: system,
                    ..Default::default()
                },
                ChatMessage {
                    role: "user".to_owned(),
                    content: observation_text,
                    ..Default::default()
                },
            ],
            temperature: 0.2,
            max_tokens: 512,
            structured_output_schema: ACTION_SCHEMA.to_owned(),
            ..Default::default()
        };

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

#[derive(Debug, serde::Deserialize)]
struct NextAction {
    action: String,
    #[serde(default)]
    selector: String,
    #[serde(default)]
    snapshot_id: String,
    #[serde(default)]
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
}
