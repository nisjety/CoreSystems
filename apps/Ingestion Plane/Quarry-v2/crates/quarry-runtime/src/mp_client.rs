//! Model Plane HTTP client + real Planner implementation.
//!
//! Calls Model Plane's gateway `POST /v1/invoke` to ask an LLM for the next
//! agent action(s) given a BrowserObservation. Replaces MockPlanner for
//! production use.
//!
//! Auth: Bearer JWT (Model Plane gateway requires it). Caller supplies token.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

use quarry_core::contracts::{
    AgentAction, AgentActionRequest, AgentConstraints, BrowserObservation,
};
use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::ids::kinds::LeaseKind;
use quarry_core::ids::Id;
use quarry_core::zdr::ZdrMode;

use crate::planner::{Planner, PlannerDecision};

const DEFAULT_TIMEOUT_SECS: u64 = 30;
const DEFAULT_MODEL: &str = "claude-sonnet-4-6";

#[derive(Debug, Clone, Serialize)]
pub struct ModelPlaneInvokeRequest {
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelPlaneInvokeResponse {
    pub request_id: String,
    pub content: String,
    pub model_used: String,
}

#[derive(Clone)]
pub struct ModelPlaneClient {
    http: Client,
    base_url: String,
    bearer_token: Option<String>,
    default_model: String,
}

impl ModelPlaneClient {
    pub fn new(base_url: impl Into<String>) -> QuarryResult<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .build()
            .map_err(|e| {
                QuarryError::new(
                    ErrorCode::Internal,
                    format!("http client build failed: {e}"),
                )
            })?;
        Ok(Self {
            http,
            base_url: base_url.into(),
            bearer_token: None,
            default_model: DEFAULT_MODEL.to_string(),
        })
    }

    pub fn with_bearer_token(mut self, token: impl Into<String>) -> Self {
        self.bearer_token = Some(token.into());
        self
    }

    pub fn with_default_model(mut self, model: impl Into<String>) -> Self {
        self.default_model = model.into();
        self
    }

    pub async fn invoke(
        &self,
        req: &ModelPlaneInvokeRequest,
    ) -> QuarryResult<ModelPlaneInvokeResponse> {
        let url = format!("{}/v1/invoke", self.base_url.trim_end_matches('/'));
        let mut builder = self.http.post(&url).json(req);
        if let Some(token) = &self.bearer_token {
            builder = builder.bearer_auth(token);
        }
        let resp = builder.send().await.map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                format!("model-plane invoke transport failure: {e}"),
            )
        })?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(QuarryError::new(
                ErrorCode::DriverFailed,
                format!("model-plane invoke returned {status}: {body}"),
            ));
        }

        resp.json::<ModelPlaneInvokeResponse>().await.map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                format!("model-plane invoke decode failure: {e}"),
            )
        })
    }
}

/// LLM-driven planner that calls Model Plane gateway to decide next actions.
pub struct ModelPlanePlanner {
    client: Arc<ModelPlaneClient>,
    constraints: AgentConstraints,
    zdr: ZdrMode,
    session_key: Option<String>,
    lease_id: LeaseKind,
}

impl ModelPlanePlanner {
    pub fn new(client: Arc<ModelPlaneClient>, constraints: AgentConstraints) -> Self {
        Self {
            client,
            constraints,
            zdr: ZdrMode::Off,
            session_key: None,
            lease_id: Id::new(),
        }
    }

    pub fn with_session_key(mut self, key: impl Into<String>) -> Self {
        self.session_key = Some(key.into());
        self
    }

    pub fn with_zdr(mut self, zdr: ZdrMode) -> Self {
        self.zdr = zdr;
        self
    }

    pub fn with_lease_id(mut self, lease_id: LeaseKind) -> Self {
        self.lease_id = lease_id;
        self
    }

    fn build_prompt(observation: &BrowserObservation) -> String {
        let observation_json = serde_json::to_string_pretty(&json!({
            "url": observation.url,
            "title": observation.title,
            "step": observation.step,
            "interactive_elements": observation
                .dom_summary
                .as_ref()
                .map(|d| d.interactive_elements.iter().take(20).collect::<Vec<_>>())
                .unwrap_or_default(),
            "text_snippet": observation
                .dom_summary
                .as_ref()
                .and_then(|d| d.text_snippet.clone()),
            "policy_denials": observation.policy_denials,
        }))
        .unwrap_or_default();

        format!(
            "You are a browser-automation planner. Given the current browser \
             observation, decide the next 1-3 actions for the agent.\n\n\
             OBSERVATION:\n{observation_json}\n\n\
             Respond with ONLY a JSON object of the form:\n\
             {{\"done\": false, \"actions\": [<action>, ...]}}\n\n\
             Each <action> must match one of these shapes:\n\
             {{\"type\": \"navigate\", \"url\": \"...\"}}\n\
             {{\"type\": \"click\", \"selector\": \"...\"}}\n\
             {{\"type\": \"type\", \"selector\": \"...\", \"text\": \"...\"}}\n\
             {{\"type\": \"press\", \"key\": \"Enter\"}}\n\
             {{\"type\": \"scroll\", \"target\": \"...\"}}\n\
             {{\"type\": \"select\", \"selector\": \"...\", \"value\": \"...\"}}\n\
             {{\"type\": \"wait\", \"ms\": 1000}}\n\
             {{\"type\": \"wait_for\", \"selector\": \"...\", \"timeout_ms\": 5000}}\n\
             {{\"type\": \"screenshot\", \"full_page\": false}}\n\
             {{\"type\": \"pdf\"}}\n\
             {{\"type\": \"evaluate\", \"script\": \"...\"}}\n\
             {{\"type\": \"back\"}}\n\
             {{\"type\": \"get_content\"}}\n\n\
             Set \"done\": true with empty actions when the agent goal is reached \
             or no further productive action is possible."
        )
    }

    fn parse_response(content: &str) -> QuarryResult<PlannerResponse> {
        let trimmed = strip_code_fences(content.trim());
        serde_json::from_str::<PlannerResponse>(trimmed).map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                format!("planner response was not valid JSON: {e} — content: {trimmed}"),
            )
        })
    }
}

#[derive(Debug, Deserialize)]
struct PlannerResponse {
    #[serde(default)]
    done: bool,
    #[serde(default)]
    actions: Vec<AgentAction>,
}

fn strip_code_fences(s: &str) -> &str {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("```json") {
        return rest.trim().trim_end_matches("```").trim();
    }
    if let Some(rest) = s.strip_prefix("```") {
        return rest.trim().trim_end_matches("```").trim();
    }
    s
}

#[async_trait]
impl Planner for ModelPlanePlanner {
    async fn next_actions(
        &self,
        observation: &BrowserObservation,
    ) -> QuarryResult<PlannerDecision> {
        let prompt = Self::build_prompt(observation);
        let req = ModelPlaneInvokeRequest {
            content: prompt,
            model: Some(self.client.default_model.clone()),
            session_key: self.session_key.clone(),
            thread_id: None,
        };

        let resp = self.client.invoke(&req).await?;
        let parsed = Self::parse_response(&resp.content)?;

        if parsed.done || parsed.actions.is_empty() {
            return Ok(PlannerDecision::Done);
        }

        let requests: Vec<AgentActionRequest> = parsed
            .actions
            .into_iter()
            .map(|action| AgentActionRequest {
                run_id: observation.run_id.clone(),
                lease_id: self.lease_id.clone(),
                action,
                instruction: None,
                constraints: self.constraints.clone(),
                zdr: self.zdr,
            })
            .collect();

        Ok(PlannerDecision::Continue(requests))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use quarry_core::ids::kinds::RunKind;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn mock_observation() -> BrowserObservation {
        let run_id: RunKind = Id::new();
        BrowserObservation {
            run_id,
            step: 0,
            url: "https://example.com".into(),
            title: Some("Example".into()),
            dom_summary: None,
            screenshot_artifact_id: None,
            console_summary: vec![],
            network_summary: vec![],
            policy_denials: vec![],
            observed_at: Utc::now(),
        }
    }

    fn default_constraints() -> AgentConstraints {
        AgentConstraints {
            max_steps: 10,
            allowed_domains: vec![],
            max_runtime_s: None,
            max_cost_usd: None,
        }
    }

    #[test]
    fn strip_fences_removes_json_block() {
        assert_eq!(strip_code_fences("```json\n{\"a\": 1}\n```"), "{\"a\": 1}");
        assert_eq!(strip_code_fences("```\n{\"a\": 1}\n```"), "{\"a\": 1}");
        assert_eq!(strip_code_fences("{\"a\": 1}"), "{\"a\": 1}");
    }

    #[test]
    fn parse_response_continue_with_navigate() {
        let json =
            r#"{"done": false, "actions": [{"type": "navigate", "url": "https://example.com"}]}"#;
        let parsed = ModelPlanePlanner::parse_response(json).unwrap();
        assert!(!parsed.done);
        assert_eq!(parsed.actions.len(), 1);
        match &parsed.actions[0] {
            AgentAction::Navigate { url } => assert_eq!(url, "https://example.com"),
            other => panic!("expected Navigate, got {other:?}"),
        }
    }

    #[test]
    fn parse_response_done_no_actions() {
        let json = r#"{"done": true, "actions": []}"#;
        let parsed = ModelPlanePlanner::parse_response(json).unwrap();
        assert!(parsed.done);
        assert!(parsed.actions.is_empty());
    }

    #[test]
    fn parse_response_strips_fences() {
        let json = "```json\n{\"done\": true, \"actions\": []}\n```";
        let parsed = ModelPlanePlanner::parse_response(json).unwrap();
        assert!(parsed.done);
    }

    #[test]
    fn parse_response_rejects_garbage() {
        let result = ModelPlanePlanner::parse_response("not json");
        assert!(result.is_err());
    }

    #[test]
    fn build_prompt_contains_observation_url() {
        let obs = mock_observation();
        let prompt = ModelPlanePlanner::build_prompt(&obs);
        assert!(prompt.contains("https://example.com"));
        assert!(prompt.contains("\"done\""));
        assert!(prompt.contains("navigate"));
    }

    #[tokio::test]
    async fn planner_calls_gateway_and_continues() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/invoke"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "request_id": "req_test",
                "content": "{\"done\": false, \"actions\": [{\"type\": \"navigate\", \"url\": \"https://example.com\"}]}",
                "model_used": "claude-sonnet-4-6"
            })))
            .mount(&server)
            .await;

        let client = Arc::new(
            ModelPlaneClient::new(server.uri())
                .unwrap()
                .with_bearer_token("test-token"),
        );
        let planner = ModelPlanePlanner::new(client, default_constraints());
        let obs = mock_observation();

        let decision = planner.next_actions(&obs).await.unwrap();
        match decision {
            PlannerDecision::Continue(reqs) => {
                assert_eq!(reqs.len(), 1);
                match &reqs[0].action {
                    AgentAction::Navigate { url } => assert_eq!(url, "https://example.com"),
                    other => panic!("expected Navigate, got {other:?}"),
                }
            }
            PlannerDecision::Done => panic!("expected Continue"),
        }
    }

    #[tokio::test]
    async fn planner_returns_done_when_llm_signals_done() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/invoke"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "request_id": "req_test",
                "content": "{\"done\": true, \"actions\": []}",
                "model_used": "claude-sonnet-4-6"
            })))
            .mount(&server)
            .await;

        let client = Arc::new(ModelPlaneClient::new(server.uri()).unwrap());
        let planner = ModelPlanePlanner::new(client, default_constraints());
        let obs = mock_observation();

        let decision = planner.next_actions(&obs).await.unwrap();
        assert!(matches!(decision, PlannerDecision::Done));
    }

    #[tokio::test]
    async fn planner_propagates_5xx_as_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/invoke"))
            .respond_with(ResponseTemplate::new(500).set_body_string("server down"))
            .mount(&server)
            .await;

        let client = Arc::new(ModelPlaneClient::new(server.uri()).unwrap());
        let planner = ModelPlanePlanner::new(client, default_constraints());
        let obs = mock_observation();

        let result = planner.next_actions(&obs).await;
        assert!(result.is_err());
    }
}
