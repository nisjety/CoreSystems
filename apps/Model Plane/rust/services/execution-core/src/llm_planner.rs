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
//! to the deterministic planner. inference-core's gRPC hop is unauthenticated
//! (provider API keys live inside that service); the address comes from
//! `INFERENCE_CORE_URL` / `INFERENCE_CORE_ADDR` (default `http://localhost:9092`).

// See quarry_agent.rs — scoped-allow the two low-signal doc pedantic lints.
#![allow(clippy::missing_errors_doc, clippy::doc_markdown)]

use mp_contracts::model_plane::v1::inference_core_client::InferenceCoreClient;
use mp_contracts::model_plane::v1::{ChatMessage, InferRequest};
use tonic::transport::Channel;

use crate::browser_agent::{ActionType, BrowserAction, BrowserObservation, PlanConfig};

/// JSON Schema describing the single next action the model must return.
const ACTION_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "action": {"type": "string", "enum": ["navigate","click","type","scroll","wait","extract","done"]},
    "selector": {"type": "string"},
    "value": {"type": "string"},
    "url": {"type": "string"},
    "reason": {"type": "string"}
  },
  "required": ["action"]
}"#;

const DEFAULT_MODEL: &str = "claude-sonnet-4-20250514";
const DEFAULT_ADDR: &str = "http://localhost:9092";

/// gRPC planner backed by inference-core.
#[derive(Clone)]
pub struct LlmPlanner {
    client: InferenceCoreClient<Channel>,
    model: String,
}

impl LlmPlanner {
    /// Build from the environment. Gated by `QUARRY_BROWSER_AGENT_LLM`; returns
    /// `None` when disabled or the endpoint URL is unparseable, so the loop can
    /// fall back to the deterministic planner.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let enabled = std::env::var("QUARRY_BROWSER_AGENT_LLM")
            .map(|v| matches!(v.trim(), "1" | "true" | "TRUE" | "yes"))
            .unwrap_or(false);
        if !enabled {
            return None;
        }
        let url = std::env::var("INFERENCE_CORE_URL")
            .or_else(|_| std::env::var("INFERENCE_CORE_ADDR"))
            .unwrap_or_else(|_| DEFAULT_ADDR.to_owned());
        let channel = Channel::from_shared(url).ok()?.connect_lazy();
        let model =
            std::env::var("QUARRY_BROWSER_AGENT_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_owned());
        Some(Self {
            client: InferenceCoreClient::new(channel),
            model,
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
                format!(
                    "Current page:\n  url: {}\n  title: {}\n  extracted_text: {}",
                    o.page_url, o.page_title, o.extracted_text
                )
            },
        );
        let system = format!(
            "You are an autonomous web-browsing agent. Goal: {}\n\
             Choose the SINGLE next browser action and return ONLY JSON matching the schema. \
             Use action=\"done\" when the goal is satisfied. \
             For navigate set url; for click/type set a CSS selector; for type also set value.",
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

        let response = self
            .client
            .clone()
            .infer(request)
            .await
            .map_err(|e| format!("inference infer rpc failed: {e}"))?
            .into_inner();

        let parsed: NextAction = serde_json::from_str(response.content.trim())
            .map_err(|e| format!("could not parse action JSON ({e}); content={}", response.content))?;
        Ok(parsed.into_browser_action())
    }
}

#[derive(Debug, serde::Deserialize)]
struct NextAction {
    action: String,
    #[serde(default)]
    selector: String,
    #[serde(default)]
    value: String,
    #[serde(default)]
    url: String,
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
        Some(BrowserAction {
            action_id: String::new(),
            grant_id: String::new(),
            action_type,
            selector: self.selector,
            value: self.value,
            url: self.url,
            max_wait_ms: 5000,
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
    fn extra_unknown_fields_are_ignored() {
        // The model emits a `reason` field (present in ACTION_SCHEMA) that
        // NextAction does not capture; serde must ignore it, not reject.
        let action = parse(r#"{"action":"scroll","reason":"need to see more"}"#)
            .into_browser_action()
            .expect("scroll is not terminal");
        assert_eq!(action.action_type, ActionType::Scroll);
    }
}
