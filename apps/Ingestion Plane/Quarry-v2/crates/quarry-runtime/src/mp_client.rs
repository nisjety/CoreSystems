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
use crate::service_tokens::{ServiceTokenRequest, SharedServiceTokenProvider};

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
    auth: ModelPlaneAuth,
    default_model: String,
}

#[derive(Clone)]
enum ModelPlaneAuth {
    None,
    StaticDev(String),
    Dynamic(SharedServiceTokenProvider),
}

impl ModelPlaneClient {
    pub fn new(base_url: impl Into<String>) -> QuarryResult<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .redirect(reqwest::redirect::Policy::none())
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
            auth: ModelPlaneAuth::None,
            default_model: DEFAULT_MODEL.to_string(),
        })
    }

    pub fn with_bearer_token(mut self, token: impl Into<String>) -> Self {
        self.auth = ModelPlaneAuth::StaticDev(token.into());
        self
    }

    /// Production authentication. Tokens are minted lazily for the verified
    /// request org supplied to [`Self::invoke_for_org`].
    pub fn with_token_provider(mut self, provider: SharedServiceTokenProvider) -> Self {
        self.auth = ModelPlaneAuth::Dynamic(provider);
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
        self.invoke_with_org(None, req).await
    }

    pub async fn invoke_for_org(
        &self,
        org_id: &str,
        req: &ModelPlaneInvokeRequest,
    ) -> QuarryResult<ModelPlaneInvokeResponse> {
        self.invoke_with_org(Some(org_id), req).await
    }

    async fn invoke_with_org(
        &self,
        org_id: Option<&str>,
        req: &ModelPlaneInvokeRequest,
    ) -> QuarryResult<ModelPlaneInvokeResponse> {
        let token_request =
            ServiceTokenRequest::model_plane(["models:invoke"], "invoke bounded model primitive");
        let bearer = self.bearer_for_org(org_id, &token_request, false).await?;
        let mut resp = self.send_invoke(req, bearer.as_deref()).await?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            if matches!(self.auth, ModelPlaneAuth::Dynamic(_)) {
                let refreshed = self.bearer_for_org(org_id, &token_request, true).await?;
                resp = self.send_invoke(req, refreshed.as_deref()).await?;
            }
        }

        self.decode_invoke(resp).await
    }

    async fn bearer_for_org(
        &self,
        org_id: Option<&str>,
        request: &ServiceTokenRequest,
        force_refresh: bool,
    ) -> QuarryResult<Option<String>> {
        match &self.auth {
            ModelPlaneAuth::None => Ok(None),
            ModelPlaneAuth::StaticDev(token) => Ok(Some(token.clone())),
            ModelPlaneAuth::Dynamic(provider) => {
                let org_id = org_id.ok_or_else(|| {
                    QuarryError::new(
                        ErrorCode::Forbidden,
                        "verified org_id is required for Model Plane authentication",
                    )
                })?;
                Ok(Some(
                    provider
                        .token_for_org(org_id, request, force_refresh)
                        .await?
                        .expose()
                        .to_owned(),
                ))
            }
        }
    }

    async fn send_invoke(
        &self,
        req: &ModelPlaneInvokeRequest,
        bearer: Option<&str>,
    ) -> QuarryResult<reqwest::Response> {
        let url = format!("{}/v1/invoke", self.base_url.trim_end_matches('/'));
        let mut builder = self.http.post(&url).json(req);
        if let Some(token) = bearer {
            builder = builder.bearer_auth(token);
        }
        builder.send().await.map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                format!("model-plane invoke transport failure: {e}"),
            )
        })
    }

    async fn decode_invoke(
        &self,
        resp: reqwest::Response,
    ) -> QuarryResult<ModelPlaneInvokeResponse> {
        let status = resp.status();
        if !status.is_success() {
            return Err(QuarryError::new(
                if matches!(status.as_u16(), 401 | 403) {
                    ErrorCode::Forbidden
                } else {
                    ErrorCode::DriverFailed
                },
                format!("model-plane invoke returned {status}"),
            ));
        }

        resp.json::<ModelPlaneInvokeResponse>().await.map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                format!("model-plane invoke decode failure: {e}"),
            )
        })
    }

    /// Streaming variant of [`invoke`]. POSTs to `/v1/invoke/stream` and yields
    /// answer-text deltas as the gateway produces them. The gateway emits SSE
    /// frames (`event: …\ndata: {json}\n\n`); we surface the `delta` string from
    /// each `data:` JSON object. A transport/parse failure ends the stream with
    /// a single `Err` item. (Note: bounded by the client's 30s request timeout.)
    pub async fn invoke_stream(
        &self,
        req: &ModelPlaneInvokeRequest,
    ) -> QuarryResult<impl futures::Stream<Item = QuarryResult<String>>> {
        self.invoke_stream_scoped(None, req).await
    }

    pub async fn invoke_stream_for_org(
        &self,
        org_id: &str,
        req: &ModelPlaneInvokeRequest,
    ) -> QuarryResult<impl futures::Stream<Item = QuarryResult<String>>> {
        self.invoke_stream_scoped(Some(org_id), req).await
    }

    pub async fn invoke_stream_scoped(
        &self,
        org_id: Option<&str>,
        req: &ModelPlaneInvokeRequest,
    ) -> QuarryResult<impl futures::Stream<Item = QuarryResult<String>>> {
        let token_request =
            ServiceTokenRequest::model_plane(["models:invoke"], "stream bounded model primitive");
        let bearer = self.bearer_for_org(org_id, &token_request, false).await?;
        let mut resp = self.send_invoke_stream(req, bearer.as_deref()).await?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            if matches!(self.auth, ModelPlaneAuth::Dynamic(_)) {
                let refreshed = self.bearer_for_org(org_id, &token_request, true).await?;
                resp = self.send_invoke_stream(req, refreshed.as_deref()).await?;
            }
        }
        let status = resp.status();
        if !status.is_success() {
            return Err(QuarryError::new(
                if matches!(status.as_u16(), 401 | 403) {
                    ErrorCode::Forbidden
                } else {
                    ErrorCode::DriverFailed
                },
                format!("model-plane invoke/stream returned {status}"),
            ));
        }

        Ok(async_stream::stream! {
            use futures::StreamExt;
            let mut bytes = resp.bytes_stream();
            let mut buf = String::new();
            while let Some(chunk) = bytes.next().await {
                match chunk {
                    Ok(b) => buf.push_str(&String::from_utf8_lossy(&b)),
                    Err(e) => {
                        yield Err(QuarryError::new(
                            ErrorCode::DriverFailed,
                            format!("model-plane invoke/stream read failed: {e}"),
                        ));
                        return;
                    }
                }
                while let Some(idx) = buf.find("\n\n") {
                    let frame: String = buf.drain(..idx + 2).collect();
                    for line in frame.lines() {
                        let Some(data) = line.trim_start().strip_prefix("data:") else {
                            continue;
                        };
                        let data = data.trim();
                        if data.is_empty() || data == "[DONE]" {
                            continue;
                        }
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                            if let Some(delta) = v.get("delta").and_then(|d| d.as_str()) {
                                if !delta.is_empty() {
                                    yield Ok(delta.to_string());
                                }
                            }
                        }
                    }
                }
            }
        })
    }

    async fn send_invoke_stream(
        &self,
        req: &ModelPlaneInvokeRequest,
        bearer: Option<&str>,
    ) -> QuarryResult<reqwest::Response> {
        let url = format!("{}/v1/invoke/stream", self.base_url.trim_end_matches('/'));
        let mut builder = self
            .http
            .post(&url)
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .json(req);
        if let Some(token) = bearer {
            builder = builder.bearer_auth(token);
        }
        builder.send().await.map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                format!("model-plane invoke/stream transport failure: {e}"),
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
            "snapshot": observation.snapshot.as_ref(),
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
             {{\"type\": \"click_ref\", \"snapshot_id\": \"...\", \"generation\": 0, \"ref_id\": \"@e1\"}}\n\
             {{\"type\": \"click_semantic\", \"snapshot_id\": \"...\", \"generation\": 0, \"locator\": {{\"kind\": \"role\", \"role\": \"button\", \"name\": \"...\", \"exact\": true}}}}\n\
             {{\"type\": \"type_ref\", \"snapshot_id\": \"...\", \"generation\": 0, \"ref_id\": \"@e1\", \"text\": \"...\"}}\n\
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
             When `snapshot` is present, prefer its `@e…` refs for any effectful\n\
             action. Echo its exact `snapshot_id` and `generation`; never invent\n\
             a ref and never reuse one after a new observation. Semantic locators\n\
             are allowed only when they identify exactly one snapshot target.\n\
             CSS selectors are legacy fallback only.\n\n\
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
                extraction_profile: None,
            })
            .collect();

        Ok(PlannerDecision::Continue(requests))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service_tokens::{ServiceBearer, ServiceTokenProvider, ServiceTokenRequest};
    use async_trait::async_trait;
    use chrono::Utc;
    use quarry_core::ids::kinds::RunKind;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn mock_observation() -> BrowserObservation {
        let run_id: RunKind = Id::new();
        BrowserObservation {
            run_id,
            step: 0,
            url: "https://example.com".into(),
            title: Some("Example".into()),
            snapshot: None,
            dom_summary: None,
            screenshot_artifact_id: None,
            visual_observation_artifact_id: None,
            evidence_delta_artifact_id: None,
            console_summary: vec![],
            network_summary: vec![],
            egress_receipts: vec![],
            dialogs: vec![],
            policy_denials: vec![],
            action_outcome: quarry_core::contracts::ActionOutcome::default(),
            observation_delta: None,
            challenge: None,
            extraction_profile: None,
            extraction_result: None,
            proof_bundle: None,
            target_resolution: None,
            telemetry: quarry_core::contracts::BrowserTelemetry::default(),
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

    struct RotatingModelTokenProvider(AtomicUsize);

    #[async_trait]
    impl ServiceTokenProvider for RotatingModelTokenProvider {
        async fn token_for_org(
            &self,
            org_id: &str,
            _request: &ServiceTokenRequest,
            force_refresh: bool,
        ) -> QuarryResult<ServiceBearer> {
            assert_eq!(org_id, "org_verified");
            let call = self.0.fetch_add(1, Ordering::SeqCst);
            assert_eq!(force_refresh, call > 0);
            Ok(ServiceBearer::from_test_token(if call == 0 {
                "first.model.token"
            } else {
                "fresh.model.token"
            }))
        }

        async fn invalidate(&self, _org_id: &str, _request: &ServiceTokenRequest) {}
    }

    #[derive(Clone)]
    struct ModelRejectThenAccept(Arc<AtomicUsize>);

    impl wiremock::Respond for ModelRejectThenAccept {
        fn respond(&self, request: &wiremock::Request) -> ResponseTemplate {
            let attempt = self.0.fetch_add(1, Ordering::SeqCst);
            let authorization = request
                .headers
                .get("authorization")
                .unwrap()
                .to_str()
                .unwrap();
            if attempt == 0 {
                assert_eq!(authorization, "Bearer first.model.token");
                ResponseTemplate::new(401)
            } else {
                assert_eq!(authorization, "Bearer fresh.model.token");
                ResponseTemplate::new(200).set_body_json(json!({
                    "request_id": "req_refreshed",
                    "content": "ok",
                    "model_used": "test-model"
                }))
            }
        }
    }

    #[tokio::test]
    async fn dynamic_model_client_refreshes_once_after_401() {
        let server = MockServer::start().await;
        let requests = Arc::new(AtomicUsize::new(0));
        Mock::given(method("POST"))
            .and(path("/v1/invoke"))
            .respond_with(ModelRejectThenAccept(requests.clone()))
            .expect(2)
            .mount(&server)
            .await;
        let tokens = Arc::new(RotatingModelTokenProvider(AtomicUsize::new(0)));
        let client = ModelPlaneClient::new(server.uri())
            .unwrap()
            .with_token_provider(tokens.clone());
        let request = ModelPlaneInvokeRequest {
            content: "hello".into(),
            model: None,
            session_key: None,
            thread_id: None,
        };

        let response = client
            .invoke_for_org("org_verified", &request)
            .await
            .unwrap();

        assert_eq!(response.request_id, "req_refreshed");
        assert_eq!(tokens.0.load(Ordering::SeqCst), 2);
        assert_eq!(requests.load(Ordering::SeqCst), 2);
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
