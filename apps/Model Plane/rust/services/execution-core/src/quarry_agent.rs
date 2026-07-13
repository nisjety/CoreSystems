//! Quarry agent-browser client (P7 · Phase A).
//!
//! Model Plane *drives* the agentic browser loop; Quarry *executes* each action
//! against a leased browser session and returns an observation. Model Plane and
//! Quarry live in separate cargo workspaces, so this module MIRRORS Quarry's
//! JSON wire contract via local DTOs (the same approach `model-gateway`'s Fetch
//! client takes) rather than importing `quarry-core`.
//!
//! Transport — HTTP per-step against `quarry-edge` (`/v1/agent/*`):
//!   POST   /v1/agent/runs             → start a run (acquire a browser lease)
//!   POST   /v1/agent/runs/{id}/step   → execute one `AgentAction`, return observation
//!   DELETE /v1/agent/runs/{id}        → release the run / lease
//!
//! Quarry wraps every response in `Envelope { data, meta, error }`, so the
//! client unwraps `.data`. `AgentAction` is internally tagged
//! (`{"type":"navigate","url":...}`) to match quarry-core exactly; `zdr` is a
//! bool because the edge converts it via `ZdrMode::from(bool)`.

// Error enums are self-evident (`AgentClientError`); `# Errors` prose would be
// noise. `doc_markdown` over-flags wire tokens like `snake_case`. Both are
// low-signal pedantic lints — scoped-allowed here (cf. workspace-allowed
// `must_use_candidate`).
#![allow(clippy::missing_errors_doc, clippy::doc_markdown)]

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::browser_agent::{ActionType, BrowserAction, BrowserObservation, ObservationStatus};
use crate::quarry_auth::TokenSource;

const AGENT_SCOPES: &[&str] = &["browser:execute"];

// ---------------------------------------------------------------------------
// Wire DTOs — mirror of quarry-core::contracts (JSON-compatible).
// ---------------------------------------------------------------------------

/// A single browser action. Internally tagged on `type`, snake_case — matches
/// quarry-core `AgentAction` exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentAction {
    Navigate { url: String },
    Click { selector: String },
    Type { selector: String, text: String },
    Press { key: String },
    Scroll { target: String },
    Select { selector: String, value: String },
    Wait { ms: u32 },
    WaitFor { selector: String, timeout_ms: u32 },
    Screenshot { full_page: bool },
    Pdf,
    Evaluate { script: String },
    Back,
    GetContent,
}

/// Per-run safety budget. Mirrors quarry-core `AgentConstraints`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentConstraints {
    pub max_steps: u32,
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_runtime_s: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cost_usd: Option<f64>,
}

/// `POST /v1/agent/runs` request body.
#[derive(Debug, Clone, Serialize)]
pub struct StartRunRequest {
    pub org_id: String,
    pub constraints: AgentConstraints,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    pub zdr: bool,
}

/// `POST /v1/agent/runs` response payload (inside the envelope `data`).
#[derive(Debug, Clone, Deserialize)]
pub struct StartRunResponse {
    pub run_id: String,
    pub lease_id: String,
}

/// `POST /v1/agent/runs/{id}/step` request body. Mirrors quarry-core
/// `AgentActionRequest` (the edge uses the run's stored constraints/zdr; the
/// extra fields are accepted and ignored server-side).
#[derive(Debug, Clone, Serialize)]
pub struct StepRequest {
    pub run_id: String,
    pub lease_id: String,
    pub action: AgentAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instruction: Option<String>,
    pub constraints: AgentConstraints,
    pub zdr: bool,
}

/// Observation returned by a step. Mirrors quarry-core `BrowserObservation`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireObservation {
    #[serde(default)]
    pub run_id: String,
    #[serde(default)]
    pub step: u32,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub dom_summary: Option<DomSummary>,
    #[serde(default)]
    pub screenshot_artifact_id: Option<String>,
    #[serde(default)]
    pub console_summary: Vec<ConsoleLine>,
    #[serde(default)]
    pub network_summary: Vec<NetworkEntry>,
    #[serde(default)]
    pub policy_denials: Vec<String>,
    #[serde(default)]
    pub observed_at: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct DomSummary {
    #[serde(default)]
    pub node_count: u32,
    #[serde(default)]
    pub interactive_elements: Vec<InteractiveElement>,
    #[serde(default)]
    pub text_snippet: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InteractiveElement {
    pub tag: String,
    pub selector: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConsoleLine {
    pub level: String,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NetworkEntry {
    pub method: String,
    pub url: String,
    pub status: u16,
    #[serde(default)]
    pub content_type: Option<String>,
}

/// Quarry's REST envelope (`{ data, meta, error }`). We only need `data` +
/// `error`; `meta` is ignored.
#[derive(Debug, Clone, Deserialize)]
struct WireEnvelope<T> {
    #[serde(default = "Option::default")]
    data: Option<T>,
    #[serde(default)]
    error: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Internal ⇄ wire mapping.
// ---------------------------------------------------------------------------

/// Map the loop's internal `BrowserAction` onto a Quarry wire `AgentAction`.
/// `Extract`/`Observe` have no direct Quarry equivalent — both resolve to
/// `GetContent`, which returns the current DOM/observation.
#[must_use]
pub fn action_to_wire(action: &BrowserAction) -> AgentAction {
    match action.action_type {
        ActionType::Goto => AgentAction::Navigate {
            url: action.url.clone(),
        },
        ActionType::Click => AgentAction::Click {
            selector: action.selector.clone(),
        },
        ActionType::Type => AgentAction::Type {
            selector: action.selector.clone(),
            text: action.value.clone(),
        },
        ActionType::Scroll => AgentAction::Scroll {
            target: if action.value.is_empty() {
                action.selector.clone()
            } else {
                action.value.clone()
            },
        },
        ActionType::Wait => AgentAction::Wait {
            ms: u32::try_from(action.max_wait_ms).unwrap_or(0),
        },
        ActionType::Extract | ActionType::Observe => AgentAction::GetContent,
    }
}

/// Build the loop's internal `BrowserObservation` from a Quarry wire
/// observation. A non-empty `policy_denials` list marks the action `Blocked`.
#[must_use]
pub fn observation_from_wire(
    wire: &WireObservation,
    action_id: &str,
    grant_id: &str,
) -> BrowserObservation {
    let status = if wire.policy_denials.is_empty() {
        ObservationStatus::Success
    } else {
        ObservationStatus::Blocked
    };
    let extracted_text = wire
        .dom_summary
        .as_ref()
        .and_then(|d| d.text_snippet.clone())
        .unwrap_or_default();
    let error_message = if wire.policy_denials.is_empty() {
        String::new()
    } else {
        wire.policy_denials.join("; ")
    };

    BrowserObservation {
        observation_id: format!("obs_{}_{:04}", wire.run_id, wire.step),
        action_id: action_id.to_owned(),
        grant_id: grant_id.to_owned(),
        status,
        page_url: wire.url.clone(),
        page_title: wire.title.clone().unwrap_or_default(),
        extracted_text,
        screenshot_ref: wire.screenshot_artifact_id.clone().unwrap_or_default(),
        dom_snapshot_ref: String::new(),
        error_message,
    }
}

// ---------------------------------------------------------------------------
// HTTP client.
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum AgentClientError {
    #[error("quarry agent transport error: {0}")]
    Transport(String),
    #[error("quarry agent returned {status}: {body}")]
    Status { status: u16, body: String },
    #[error("quarry agent decode error: {0}")]
    Decode(String),
    #[error("quarry agent authentication failed: {0}")]
    Authentication(String),
}

/// HTTP client for Quarry's agent-browser endpoint. Mirrors the auth scheme of
/// `model-gateway`'s Fetch client: bearer token + `X-Quarry-Org` header.
#[derive(Debug, Clone)]
pub struct QuarryAgentClient {
    http: reqwest::Client,
    base_url: String,
    auth: TokenSource,
}

impl QuarryAgentClient {
    /// Construct a client against an explicit base URL + bearer token.
    pub fn new(
        base_url: impl Into<String>,
        token: impl Into<String>,
    ) -> Result<Self, AgentClientError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| AgentClientError::Transport(e.to_string()))?;
        Ok(Self {
            http,
            base_url: base_url.into(),
            auth: TokenSource::Static(token.into()),
        })
    }

    /// Build a client from the environment, gated by `QUARRY_BROWSER_AGENT_ENABLED`
    /// (truthy) and `QUARRY_EDGE_URL` (non-empty) — analogous to how the Fetch
    /// tool gates on `QUARRY_EDGE_URL`. Returns `None` when the browser agent is
    /// disabled or unconfigured, so the loop can degrade gracefully.
    pub fn from_env() -> Result<Option<Self>, AgentClientError> {
        let enabled = std::env::var("QUARRY_BROWSER_AGENT_ENABLED")
            .map(|v| matches!(v.trim(), "1" | "true" | "TRUE" | "yes"))
            .unwrap_or(false);
        if !enabled {
            return Ok(None);
        }
        let base_url = std::env::var("QUARRY_EDGE_URL")
            .ok()
            .filter(|s| !s.is_empty());
        let Some(base_url) = base_url else {
            return Ok(None);
        };
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| AgentClientError::Transport(error.to_string()))?;
        let auth = TokenSource::from_env()
            .map_err(|error| AgentClientError::Authentication(error.to_string()))?;
        Ok(Some(Self {
            http,
            base_url,
            auth,
        }))
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url.trim_end_matches('/'), path)
    }

    async fn token(&self, org_id: &str) -> Result<String, AgentClientError> {
        self.auth
            .token(org_id, AGENT_SCOPES)
            .await
            .map_err(|error| AgentClientError::Authentication(error.to_string()))
    }

    async fn invalidate_if_matches(
        &self,
        org_id: &str,
        rejected_token: &str,
    ) -> Result<(), AgentClientError> {
        self.auth
            .invalidate_if_matches(org_id, AGENT_SCOPES, rejected_token)
            .await
            .map_err(|error| AgentClientError::Authentication(error.to_string()))
    }

    /// Start an agent run: Quarry acquires a browser lease/session and returns
    /// the `run_id` + `lease_id` used by subsequent steps.
    pub async fn start_run(
        &self,
        org_id: &str,
        constraints: &AgentConstraints,
        zdr: bool,
        profile_id: Option<String>,
    ) -> Result<StartRunResponse, AgentClientError> {
        let body = StartRunRequest {
            org_id: org_id.to_owned(),
            constraints: constraints.clone(),
            profile_id,
            zdr,
        };
        let mut retried_unauthorized = false;
        let resp = loop {
            let token = self.token(org_id).await?;
            let resp = self
                .http
                .post(self.url("/v1/agent/runs"))
                .bearer_auth(&token)
                .header("x-quarry-org", org_id)
                .json(&body)
                .send()
                .await
                .map_err(|e| AgentClientError::Transport(e.to_string()))?;
            if resp.status() == reqwest::StatusCode::UNAUTHORIZED && !retried_unauthorized {
                self.invalidate_if_matches(org_id, &token).await?;
                retried_unauthorized = true;
                continue;
            }
            break resp;
        };
        Self::decode(resp).await
    }

    /// Execute one action against an existing run; returns the observation.
    pub async fn step(
        &self,
        run_id: &str,
        lease_id: &str,
        org_id: &str,
        action: AgentAction,
        constraints: &AgentConstraints,
        zdr: bool,
    ) -> Result<WireObservation, AgentClientError> {
        let body = StepRequest {
            run_id: run_id.to_owned(),
            lease_id: lease_id.to_owned(),
            action,
            instruction: None,
            constraints: constraints.clone(),
            zdr,
        };
        let endpoint = self.url(&format!("/v1/agent/runs/{run_id}/step"));
        let mut retried_unauthorized = false;
        let resp = loop {
            let token = self.token(org_id).await?;
            let resp = self
                .http
                .post(&endpoint)
                .bearer_auth(&token)
                .header("x-quarry-org", org_id)
                .json(&body)
                .send()
                .await
                .map_err(|e| AgentClientError::Transport(e.to_string()))?;
            if resp.status() == reqwest::StatusCode::UNAUTHORIZED && !retried_unauthorized {
                self.invalidate_if_matches(org_id, &token).await?;
                retried_unauthorized = true;
                continue;
            }
            break resp;
        };
        Self::decode(resp).await
    }

    /// Release a run and its browser lease. Best-effort; non-2xx is surfaced.
    pub async fn close_run(&self, run_id: &str, org_id: &str) -> Result<(), AgentClientError> {
        let endpoint = self.url(&format!("/v1/agent/runs/{run_id}"));
        let mut retried_unauthorized = false;
        let resp = loop {
            let token = self.token(org_id).await?;
            let resp = self
                .http
                .delete(&endpoint)
                .bearer_auth(&token)
                .header("x-quarry-org", org_id)
                .send()
                .await
                .map_err(|e| AgentClientError::Transport(e.to_string()))?;
            if resp.status() == reqwest::StatusCode::UNAUTHORIZED && !retried_unauthorized {
                self.invalidate_if_matches(org_id, &token).await?;
                retried_unauthorized = true;
                continue;
            }
            break resp;
        };
        let status = resp.status();
        if status.is_success() {
            Ok(())
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(AgentClientError::Status {
                status: status.as_u16(),
                body,
            })
        }
    }

    /// Decode a Quarry `Envelope<T>` response, returning the unwrapped `data`.
    async fn decode<T: for<'de> Deserialize<'de>>(
        resp: reqwest::Response,
    ) -> Result<T, AgentClientError> {
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| AgentClientError::Transport(e.to_string()))?;
        if !status.is_success() {
            return Err(AgentClientError::Status {
                status: status.as_u16(),
                body: text,
            });
        }
        let env: WireEnvelope<T> =
            serde_json::from_str(&text).map_err(|e| AgentClientError::Decode(e.to_string()))?;
        if let Some(err) = env.error {
            return Err(AgentClientError::Status {
                status: status.as_u16(),
                body: err.to_string(),
            });
        }
        env.data
            .ok_or_else(|| AgentClientError::Decode("envelope contained no data".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn act(action_type: ActionType, selector: &str, value: &str, url: &str) -> BrowserAction {
        BrowserAction {
            action_id: "act_1".to_owned(),
            grant_id: "grant_1".to_owned(),
            action_type,
            selector: selector.to_owned(),
            value: value.to_owned(),
            url: url.to_owned(),
            max_wait_ms: 3000,
            reason: String::new(),
            risk_category: None,
        }
    }

    #[test]
    fn agent_action_serializes_internally_tagged_snake_case() {
        let v = serde_json::to_value(AgentAction::Navigate {
            url: "https://example.com".into(),
        })
        .unwrap();
        assert_eq!(v["type"], "navigate");
        assert_eq!(v["url"], "https://example.com");

        let t = serde_json::to_value(AgentAction::Type {
            selector: "#q".into(),
            text: "hi".into(),
        })
        .unwrap();
        assert_eq!(t["type"], "type");
        assert_eq!(t["selector"], "#q");
        assert_eq!(t["text"], "hi");

        assert_eq!(
            serde_json::to_value(AgentAction::GetContent).unwrap()["type"],
            "get_content"
        );
    }

    #[test]
    fn action_to_wire_maps_each_internal_variant() {
        assert_eq!(
            action_to_wire(&act(ActionType::Goto, "", "", "https://x.io")),
            AgentAction::Navigate {
                url: "https://x.io".into()
            }
        );
        assert_eq!(
            action_to_wire(&act(ActionType::Click, "#btn", "", "")),
            AgentAction::Click {
                selector: "#btn".into()
            }
        );
        assert_eq!(
            action_to_wire(&act(ActionType::Type, "#q", "hello", "")),
            AgentAction::Type {
                selector: "#q".into(),
                text: "hello".into()
            }
        );
        assert_eq!(
            action_to_wire(&act(ActionType::Extract, "", "", "")),
            AgentAction::GetContent
        );
        assert_eq!(
            action_to_wire(&act(ActionType::Observe, "", "", "")),
            AgentAction::GetContent
        );
        assert_eq!(
            action_to_wire(&act(ActionType::Wait, "", "", "")),
            AgentAction::Wait { ms: 3000 }
        );
    }

    #[test]
    fn observation_from_wire_maps_fields_and_blocked() {
        let mut wire = WireObservation {
            run_id: "run_1".into(),
            step: 2,
            url: "https://example.com/p".into(),
            title: Some("Title".into()),
            dom_summary: Some(DomSummary {
                node_count: 10,
                interactive_elements: vec![],
                text_snippet: Some("page text".into()),
            }),
            ..Default::default()
        };
        let obs = observation_from_wire(&wire, "act_9", "grant_9");
        assert_eq!(obs.status, ObservationStatus::Success);
        assert_eq!(obs.page_url, "https://example.com/p");
        assert_eq!(obs.page_title, "Title");
        assert_eq!(obs.extracted_text, "page text");
        assert_eq!(obs.action_id, "act_9");

        wire.policy_denials = vec!["domain not allowed".into()];
        let blocked = observation_from_wire(&wire, "act_9", "grant_9");
        assert_eq!(blocked.status, ObservationStatus::Blocked);
        assert!(blocked.error_message.contains("domain not allowed"));
    }

    #[tokio::test]
    async fn start_run_posts_and_unwraps_envelope() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/agent/runs"))
            .and(header("authorization", "Bearer tok"))
            .and(header("x-quarry-org", "org_1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": { "run_id": "run_abc", "lease_id": "lease_xyz" },
                "meta": { "request_id": "req_1" },
                "error": null,
            })))
            .mount(&server)
            .await;

        let client = QuarryAgentClient::new(server.uri(), "tok").unwrap();
        let resp = client
            .start_run("org_1", &AgentConstraints::default(), false, None)
            .await
            .unwrap();
        assert_eq!(resp.run_id, "run_abc");
        assert_eq!(resp.lease_id, "lease_xyz");
    }

    #[tokio::test]
    async fn step_posts_action_and_unwraps_observation() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/agent/runs/run_abc/step"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {
                    "run_id": "run_abc",
                    "step": 1,
                    "url": "https://example.com",
                    "title": "Example",
                    "dom_summary": {"node_count": 3, "interactive_elements": [], "text_snippet": "hi"},
                    "observed_at": "2026-05-30T00:00:00Z",
                },
                "meta": { "request_id": "req_2" },
                "error": null,
            })))
            .mount(&server)
            .await;

        let client = QuarryAgentClient::new(server.uri(), "tok").unwrap();
        let obs = client
            .step(
                "run_abc",
                "lease_xyz",
                "org_1",
                AgentAction::GetContent,
                &AgentConstraints::default(),
                false,
            )
            .await
            .unwrap();
        assert_eq!(obs.url, "https://example.com");
        assert_eq!(obs.title.as_deref(), Some("Example"));
        let mapped = observation_from_wire(&obs, "act_1", "grant_1");
        assert_eq!(mapped.extracted_text, "hi");
    }

    #[tokio::test]
    async fn step_surfaces_non_2xx_as_status_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/agent/runs/run_abc/step"))
            .respond_with(ResponseTemplate::new(403).set_body_string("domain violation"))
            .mount(&server)
            .await;

        let client = QuarryAgentClient::new(server.uri(), "tok").unwrap();
        let err = client
            .step(
                "run_abc",
                "lease_xyz",
                "org_1",
                AgentAction::Navigate {
                    url: "https://evil.io".into(),
                },
                &AgentConstraints::default(),
                false,
            )
            .await
            .unwrap_err();
        match err {
            AgentClientError::Status { status, .. } => assert_eq!(status, 403),
            other => panic!("expected Status error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn step_retries_exactly_once_after_unauthorized() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/agent/runs/run_abc/step"))
            .respond_with(ResponseTemplate::new(401))
            .expect(2)
            .mount(&server)
            .await;

        let client = QuarryAgentClient::new(server.uri(), "expired").unwrap();
        let error = client
            .step(
                "run_abc",
                "lease_xyz",
                "org_1",
                AgentAction::GetContent,
                &AgentConstraints::default(),
                false,
            )
            .await
            .expect_err("second 401 must be surfaced");
        assert!(matches!(
            error,
            AgentClientError::Status { status: 401, .. }
        ));
    }
}
