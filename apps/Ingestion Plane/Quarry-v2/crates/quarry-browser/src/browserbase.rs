//! Browserbase cloud browser driver.
//!
//! Implements [`BrowserDriver`] against the Browserbase REST + Connect API:
//! - Sessions are created via `POST /v1/sessions`
//! - Pages are driven via CDP over the session's WebSocket debugger URL
//! - Screenshots, PDFs, and content come from CDP commands forwarded through
//!   the Browserbase proxy
//!
//! This driver uses a REST-based approach (similar to Browserless) for the
//! initial implementation. A future upgrade can open a persistent CDP
//! WebSocket for lower latency.

use async_trait::async_trait;
use bytes::Bytes;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::lease::BrowserLease;

use crate::navigation::guard_navigation_target;
use crate::{BrowserDriver, BrowserSession, SessionInner};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserbaseConfig {
    pub api_key: String,
    pub project_id: String,
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default)]
    pub context_id: Option<String>,
    #[serde(default)]
    pub recording: bool,
}

fn default_base_url() -> String {
    "https://www.browserbase.com".to_string()
}

#[derive(Default)]
struct SessionState {
    session_id: Option<String>,
    current_url: Option<String>,
    live_view_url: Option<String>,
    connect_url: Option<String>,
    recording_id: Option<String>,
}

pub struct BrowserbaseDriver {
    config: BrowserbaseConfig,
    http: Client,
    state: Arc<Mutex<SessionState>>,
}

impl BrowserbaseDriver {
    pub fn new(config: BrowserbaseConfig) -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("reqwest client");
        Self {
            config,
            http,
            state: Arc::new(Mutex::new(SessionState::default())),
        }
    }

    fn api_url(&self, path: &str) -> String {
        format!("{}/v1{}", self.config.base_url.trim_end_matches('/'), path)
    }

    async fn create_session(&self) -> QuarryResult<CreateSessionResponse> {
        let url = self.api_url("/sessions");
        let mut body = json!({
            "projectId": self.config.project_id,
        });
        if let Some(ctx) = &self.config.context_id {
            body["browserSettings"] = json!({ "context": { "id": ctx, "persist": true } });
        }
        if self.config.recording {
            body["keepAlive"] = json!(true);
        }
        let resp = self
            .http
            .post(&url)
            .header("x-bb-api-key", &self.config.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                QuarryError::new(ErrorCode::DriverFailed, "browserbase session create failed")
                    .with_details(json!({ "error": e.to_string() }))
            })?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(QuarryError::new(
                ErrorCode::DriverFailed,
                "browserbase session create returned error",
            )
            .with_details(json!({ "status": status.as_u16(), "body": text })));
        }

        resp.json::<CreateSessionResponse>().await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "browserbase response parse failed")
                .with_details(json!({ "error": e.to_string() }))
        })
    }

    async fn ensure_session(&self) -> QuarryResult<()> {
        let mut state = self.state.lock().await;
        if state.session_id.is_some() {
            return Ok(());
        }

        let session = self.create_session().await?;
        state.connect_url = Some(session.connect_url.clone());
        state.live_view_url = session.live_view_url.clone();
        state.recording_id = session.recording_id.clone();
        state.session_id = Some(session.id);
        Ok(())
    }

    async fn current_url(&self) -> QuarryResult<String> {
        self.state.lock().await.current_url.clone().ok_or_else(|| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                "browserbase: no current URL; call goto() first",
            )
        })
    }

    async fn post_content(&self, path: &str, body: serde_json::Value) -> QuarryResult<Bytes> {
        let state = self.state.lock().await;
        let session_id = state.session_id.as_deref().ok_or_else(|| {
            QuarryError::new(ErrorCode::DriverFailed, "browserbase: no active session")
        })?;
        let url = self.api_url(&format!("/sessions/{session_id}{path}"));
        drop(state);

        let resp = self
            .http
            .post(&url)
            .header("x-bb-api-key", &self.config.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                QuarryError::new(ErrorCode::DriverFailed, "browserbase request failed")
                    .with_details(json!({ "error": e.to_string(), "path": path }))
            })?;

        let status = resp.status();
        let bytes = resp.bytes().await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "browserbase body read failed")
                .with_details(json!({ "error": e.to_string() }))
        })?;

        if !status.is_success() {
            return Err(QuarryError::new(
                ErrorCode::DriverFailed,
                "browserbase returned non-success",
            )
            .with_details(json!({
                "status": status.as_u16(),
                "path": path,
                "body": String::from_utf8_lossy(&bytes).to_string(),
            })));
        }

        Ok(bytes)
    }

    pub async fn live_view_url(&self) -> Option<String> {
        self.state.lock().await.live_view_url.clone()
    }

    pub async fn session_id(&self) -> Option<String> {
        self.state.lock().await.session_id.clone()
    }

    pub async fn recording_id(&self) -> Option<String> {
        self.state.lock().await.recording_id.clone()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateSessionResponse {
    id: String,
    connect_url: String,
    #[serde(default)]
    live_view_url: Option<String>,
    #[serde(default)]
    recording_id: Option<String>,
}

#[async_trait]
impl BrowserDriver for BrowserbaseDriver {
    async fn acquire(&self, lease: &BrowserLease) -> QuarryResult<BrowserSession> {
        self.ensure_session().await?;
        Ok(BrowserSession {
            lease: lease.clone(),
            inner: Arc::new(Mutex::new(SessionInner {
                connected: true,
                pages_served: 0,
            })),
        })
    }

    async fn release(&self, _session: BrowserSession) -> QuarryResult<()> {
        let mut state = self.state.lock().await;
        if let Some(session_id) = state.session_id.take() {
            let url = self.api_url(&format!("/sessions/{session_id}"));
            let _ = self
                .http
                .delete(&url)
                .header("x-bb-api-key", &self.config.api_key)
                .send()
                .await;
        }
        state.current_url = None;
        state.connect_url = None;
        state.live_view_url = None;
        state.recording_id = None;
        Ok(())
    }

    async fn goto(&self, session: &BrowserSession, url: &str) -> QuarryResult<()> {
        guard_navigation_target(url).await?;
        self.ensure_session().await?;
        self.state.lock().await.current_url = Some(url.to_string());
        let mut inner = session.inner.lock().await;
        inner.pages_served = inner.pages_served.saturating_add(1);
        Ok(())
    }

    async fn content(&self, _session: &BrowserSession) -> QuarryResult<Bytes> {
        let url = self.current_url().await?;
        self.post_content("/content", json!({ "url": url })).await
    }

    async fn screenshot(&self, _session: &BrowserSession, full_page: bool) -> QuarryResult<Bytes> {
        let url = self.current_url().await?;
        self.post_content(
            "/screenshot",
            json!({
                "url": url,
                "options": { "fullPage": full_page, "type": "png" },
            }),
        )
        .await
    }

    async fn pdf(&self, _session: &BrowserSession) -> QuarryResult<Bytes> {
        let url = self.current_url().await?;
        self.post_content("/pdf", json!({ "url": url })).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarry_core::error::ErrorCode;
    use quarry_core::ids::kinds;
    use quarry_core::lease::{BrowserLease, Capability, ProxyAffinity};
    use wiremock::matchers::{header, method, path as wpath};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn make_lease() -> BrowserLease {
        BrowserLease {
            lease_id: kinds::LeaseKind::new(),
            profile_id: kinds::ProfileKind::new(),
            session_affinity_key: "k".into(),
            proxy_affinity: ProxyAffinity {
                pool: "p".into(),
                sticky_key: None,
            },
            ttl_s: 30,
            capabilities: vec![Capability::Js],
            artifact_bucket: "b".into(),
            persist_profile: false,
            viewport: None,
            org_id: "test_org".into(),
        }
    }

    fn make_config(base_url: &str) -> BrowserbaseConfig {
        BrowserbaseConfig {
            api_key: "test-key".into(),
            project_id: "proj-123".into(),
            base_url: base_url.to_string(),
            context_id: None,
            recording: false,
        }
    }

    fn unstarted_session() -> BrowserSession {
        BrowserSession {
            lease: make_lease(),
            inner: Arc::new(Mutex::new(SessionInner {
                connected: true,
                pages_served: 0,
            })),
        }
    }

    #[tokio::test]
    async fn blocked_target_does_not_create_a_browserbase_session() {
        let server = MockServer::start().await;
        let driver = BrowserbaseDriver::new(make_config(&server.uri()));

        let err = driver
            .goto(&unstarted_session(), "http://127.0.0.1:8080/private")
            .await
            .expect_err("private navigation must be blocked locally");

        assert_eq!(err.code, ErrorCode::SecurityBlocked);
        assert!(server
            .received_requests()
            .await
            .expect("request log")
            .is_empty());
    }

    #[tokio::test]
    async fn creates_session_and_fetches_content() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(wpath("/v1/sessions"))
            .and(header("x-bb-api-key", "test-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "sess-abc",
                "connectUrl": "wss://connect.browserbase.com/sess-abc",
                "liveViewUrl": "https://live.browserbase.com/sess-abc",
            })))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(wpath("/v1/sessions/sess-abc/content"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"<html>browserbase</html>"))
            .mount(&server)
            .await;

        Mock::given(method("DELETE"))
            .and(wpath("/v1/sessions/sess-abc"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let driver = BrowserbaseDriver::new(make_config(&server.uri()));
        let session = driver.acquire(&make_lease()).await.unwrap();

        assert!(driver.session_id().await.is_some());
        assert!(driver.live_view_url().await.is_some());

        driver.goto(&session, "https://example.com").await.unwrap();
        let body = driver.content(&session).await.unwrap();
        assert_eq!(&body[..], b"<html>browserbase</html>");

        driver.release(session).await.unwrap();
        assert!(driver.session_id().await.is_none());
    }

    #[tokio::test]
    async fn session_create_failure_maps_to_driver_failed() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/v1/sessions"))
            .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
            .mount(&server)
            .await;

        let driver = BrowserbaseDriver::new(make_config(&server.uri()));
        let err = driver.acquire(&make_lease()).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::DriverFailed);
    }

    #[tokio::test]
    async fn content_without_goto_fails() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/v1/sessions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "sess-xyz",
                "connectUrl": "wss://connect.browserbase.com/sess-xyz",
            })))
            .mount(&server)
            .await;

        let driver = BrowserbaseDriver::new(make_config(&server.uri()));
        let session = driver.acquire(&make_lease()).await.unwrap();
        let err = driver.content(&session).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::DriverFailed);
        assert!(err.message.contains("no current URL"));
    }

    #[tokio::test]
    async fn screenshot_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/v1/sessions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "sess-ss",
                "connectUrl": "wss://connect.browserbase.com/sess-ss",
            })))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(wpath("/v1/sessions/sess-ss/screenshot"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"\x89PNG\r\n"))
            .mount(&server)
            .await;

        let driver = BrowserbaseDriver::new(make_config(&server.uri()));
        let session = driver.acquire(&make_lease()).await.unwrap();
        driver.goto(&session, "https://example.com").await.unwrap();
        let png = driver.screenshot(&session, true).await.unwrap();
        assert_eq!(&png[..], b"\x89PNG\r\n");
    }
}
