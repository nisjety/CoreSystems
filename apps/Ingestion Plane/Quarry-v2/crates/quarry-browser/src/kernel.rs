//! Kernel cloud-browser driver.
//!
//! Kernel (kernel-images / kernel.so) provides cloud browser sessions over
//! CDP, isolated VMs, replays, and live view. Quarry treats Kernel as a
//! browser **runtime substrate**, not an agent brain — Model Plane plans,
//! Quarry executes via this driver.
//!
//! ## API surface (paraphrased from Kernel docs)
//!
//! - `POST /v1/browsers` body `{org_id, profile_id?, ttl_s?, replay?, live_view?}`
//!   → `{browser_id, cdp_url, replay_url?, live_view_url?, expires_at}`
//! - `DELETE /v1/browsers/{browser_id}` → `{success}`
//! - `POST /v1/browsers/{browser_id}/content` → rendered HTML for current URL
//! - `POST /v1/browsers/{browser_id}/screenshot` → PNG
//! - `POST /v1/browsers/{browser_id}/pdf` → PDF
//! - `POST /v1/browsers/{browser_id}/goto` body `{url}` → ack
//!
//! Quarry stores `browser_id` in the session inner state. Production wiring
//! prefers the CDP URL directly via `chromiumoxide` when available; this
//! driver is a REST-only fallback that mirrors the BrowserlessDriver shape.

use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::lease::BrowserLease;

use crate::session::{Cookie, ProfileStore, SessionSnapshot};
use crate::{BrowserDriver, BrowserSession, SessionInner};

#[derive(Debug, Clone)]
pub struct KernelConfig {
    pub base_url: String,
    pub api_key: String,
    /// Optional org_id to scope sessions; defaults to lease.session_affinity_key.
    pub org_id: Option<String>,
    /// Lease TTL hint passed to Kernel (secs); 0 = use Kernel default.
    pub ttl_s: u64,
    pub enable_replay: bool,
    pub enable_live_view: bool,
}

impl KernelConfig {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            org_id: None,
            ttl_s: 0,
            enable_replay: false,
            enable_live_view: false,
        }
    }
}

#[derive(Debug, Default)]
struct KernelSessionState {
    browser_id: Option<String>,
    cdp_url: Option<String>,
    replay_url: Option<String>,
    live_view_url: Option<String>,
    current_url: Option<String>,
    expires_at: Option<DateTime<Utc>>,
}

pub struct KernelDriver {
    config: KernelConfig,
    http: Client,
    state: Arc<Mutex<KernelSessionState>>,
    /// Optional ProfileStore for persistent session restore. When set, the
    /// driver loads the snapshot keyed on `lease.profile_id` before
    /// requesting a Kernel browser, then re-saves on `release` so the next
    /// run picks up where this one left off.
    profiles: Option<Arc<dyn ProfileStore>>,
}

#[derive(Debug, Serialize)]
struct CreateBrowserRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    org_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    profile_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ttl_s: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    replay: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    live_view: Option<bool>,
    /// Cookies to seed the browser with — restored from a `SessionSnapshot`.
    /// Kernel accepts an array of objects with `name`, `value`, `domain`
    /// (optional), `path` (optional), `secure` (default false), `http_only`
    /// (default false), `same_site` (optional).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    cookies: Vec<RestoredCookie>,
    /// Initial localStorage entries keyed on the storage origin.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    local_storage: Vec<KvEntry>,
    /// User-agent override, if the snapshot recorded one.
    #[serde(skip_serializing_if = "Option::is_none")]
    user_agent: Option<String>,
    /// Viewport hints — `null` lets Kernel pick its default.
    #[serde(skip_serializing_if = "Option::is_none")]
    viewport: Option<ViewportSpec>,
    /// Locale string e.g. "en-US".
    #[serde(skip_serializing_if = "Option::is_none")]
    locale: Option<String>,
    /// IANA timezone e.g. "America/New_York".
    #[serde(skip_serializing_if = "Option::is_none")]
    timezone: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RestoredCookie {
    name: String,
    value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(default)]
    secure: bool,
    #[serde(default)]
    http_only: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    same_site: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KvEntry {
    key: String,
    value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ViewportSpec {
    width: u32,
    height: u32,
    device_scale_factor: f64,
    is_mobile: bool,
}

#[derive(Debug, Deserialize)]
struct CreateBrowserResponse {
    browser_id: String,
    cdp_url: String,
    #[serde(default)]
    replay_url: Option<String>,
    #[serde(default)]
    live_view_url: Option<String>,
    #[serde(default)]
    expires_at: Option<DateTime<Utc>>,
}

impl KernelDriver {
    pub fn new(config: KernelConfig) -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("reqwest client");
        Self {
            config,
            http,
            state: Arc::new(Mutex::new(KernelSessionState::default())),
            profiles: None,
        }
    }

    /// Attach a ProfileStore for session restore/capture. With this set, the
    /// driver loads `SessionSnapshot { cookies, local_storage, user_agent,
    /// viewport, locale, timezone }` keyed on `lease.profile_id` and seeds
    /// the Kernel browser at create time. On `release` the captured state
    /// (best-effort — Kernel must report it back via `/state` endpoint, see
    /// `fetch_session_state`) is saved back to the same key.
    pub fn with_profile_store(mut self, store: Arc<dyn ProfileStore>) -> Self {
        self.profiles = Some(store);
        self
    }

    fn endpoint(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.config.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    fn auth_header(&self) -> String {
        format!("Bearer {}", self.config.api_key)
    }

    /// Public accessor for live-view URL (used by edge to surface to UI).
    pub async fn live_view_url(&self) -> Option<String> {
        self.state.lock().await.live_view_url.clone()
    }

    pub async fn replay_url(&self) -> Option<String> {
        self.state.lock().await.replay_url.clone()
    }

    pub async fn browser_id(&self) -> Option<String> {
        self.state.lock().await.browser_id.clone()
    }

    pub async fn cdp_url(&self) -> Option<String> {
        self.state.lock().await.cdp_url.clone()
    }

    async fn ensure_browser(&self, lease: &BrowserLease) -> QuarryResult<String> {
        let mut state = self.state.lock().await;
        if let Some(id) = &state.browser_id {
            return Ok(id.clone());
        }

        // Attempt to restore from ProfileStore. Failures are non-fatal: a
        // missing or unreadable profile means we just create a fresh
        // browser with no seeded state.
        let snapshot = match &self.profiles {
            Some(store) => store
                .load(&lease.org_id, &lease.profile_id)
                .await
                .unwrap_or(None),
            None => None,
        };

        let cookies = snapshot
            .as_ref()
            .map(|s| {
                s.cookies
                    .iter()
                    .map(|c| RestoredCookie {
                        name: c.name.clone(),
                        value: c.value.clone(),
                        domain: opt_string(&c.domain),
                        path: opt_string(&c.path),
                        secure: c.secure,
                        http_only: c.http_only,
                        same_site: None, // Cookie struct in session.rs predates same_site
                    })
                    .collect()
            })
            .unwrap_or_default();

        let local_storage = snapshot
            .as_ref()
            .map(|s| {
                s.local_storage
                    .iter()
                    .map(|(k, v)| KvEntry {
                        key: k.clone(),
                        value: v.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();

        let viewport = snapshot.as_ref().and_then(|s| {
            s.viewport.as_ref().map(|v| ViewportSpec {
                width: v.width,
                height: v.height,
                device_scale_factor: v.device_scale_factor,
                is_mobile: v.is_mobile,
            })
        });

        let body = CreateBrowserRequest {
            org_id: self
                .config
                .org_id
                .as_deref()
                .or(Some(lease.session_affinity_key.as_str())),
            profile_id: Some(lease.profile_id.to_string()),
            ttl_s: Some(self.config.ttl_s.max(u64::from(lease.ttl_s))),
            replay: Some(self.config.enable_replay),
            live_view: Some(self.config.enable_live_view),
            cookies,
            local_storage,
            user_agent: snapshot.as_ref().and_then(|s| s.user_agent.clone()),
            viewport,
            locale: snapshot.as_ref().and_then(|s| s.locale.clone()),
            timezone: snapshot.as_ref().and_then(|s| s.timezone.clone()),
        };

        let resp = self
            .http
            .post(self.endpoint("v1/browsers"))
            .header("authorization", self.auth_header())
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                QuarryError::new(
                    ErrorCode::DriverFailed,
                    format!("kernel create failed: {e}"),
                )
            })?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let txt = resp.text().await.unwrap_or_default();
            return Err(QuarryError::new(
                if status == 429 {
                    ErrorCode::RateLimited
                } else if status >= 500 {
                    ErrorCode::DriverFailed
                } else {
                    ErrorCode::BadRequest
                },
                format!("kernel create returned {status}: {txt}"),
            ));
        }

        let parsed: CreateBrowserResponse = resp.json().await.map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                format!("kernel decode failed: {e}"),
            )
        })?;

        state.browser_id = Some(parsed.browser_id.clone());
        state.cdp_url = Some(parsed.cdp_url);
        state.replay_url = parsed.replay_url;
        state.live_view_url = parsed.live_view_url;
        state.expires_at = parsed.expires_at;
        Ok(parsed.browser_id)
    }

    /// Fetch the current session state (cookies + localStorage + viewport
    /// hints) from a running Kernel browser. Returns `None` when the
    /// endpoint isn't available (older Kernel versions) so callers can
    /// fall back gracefully.
    ///
    /// Called from `release` to capture state before the browser is torn
    /// down. Production Kernel exposes `GET /v1/browsers/{id}/state`
    /// returning a JSON body with the same shape we restore from on
    /// acquire (RestoredCookie + KvEntry).
    async fn fetch_session_state(&self, browser_id: &str) -> QuarryResult<Option<SessionSnapshot>> {
        let url = self.endpoint(&format!("v1/browsers/{browser_id}/state"));
        let resp = self
            .http
            .get(&url)
            .header("authorization", self.auth_header())
            .send()
            .await
            .map_err(|e| {
                QuarryError::new(
                    ErrorCode::DriverFailed,
                    format!("kernel state fetch failed: {e}"),
                )
            })?;

        if resp.status().as_u16() == 404 {
            return Ok(None);
        }
        if !resp.status().is_success() {
            return Ok(None); // best-effort
        }

        #[derive(Debug, Deserialize)]
        struct StateResponse {
            #[serde(default)]
            cookies: Vec<RestoredCookie>,
            #[serde(default)]
            local_storage: Vec<KvEntry>,
            #[serde(default)]
            user_agent: Option<String>,
            #[serde(default)]
            viewport: Option<ViewportSpec>,
            #[serde(default)]
            locale: Option<String>,
            #[serde(default)]
            timezone: Option<String>,
        }

        let parsed: StateResponse = resp.json().await.map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                format!("kernel state decode failed: {e}"),
            )
        })?;

        let snapshot = SessionSnapshot {
            cookies: parsed
                .cookies
                .into_iter()
                .map(|c| Cookie {
                    name: c.name,
                    value: c.value,
                    domain: c.domain.unwrap_or_default(),
                    path: c.path.unwrap_or_else(|| "/".into()),
                    secure: c.secure,
                    http_only: c.http_only,
                    expires: None,
                })
                .collect(),
            local_storage: parsed
                .local_storage
                .into_iter()
                .map(|e| (e.key, e.value))
                .collect(),
            session_storage: vec![],
            // Cycle 20 / cluster #13 — Kernel's /state endpoint
            // doesn't yet surface IDB rows. Leave empty for now;
            // a follow-up cycle will wire IDB capture into the Kernel
            // protocol once they add it (open question with the vendor).
            indexed_db: vec![],
            user_agent: parsed.user_agent,
            viewport: parsed.viewport.map(|v| crate::session::Viewport {
                width: v.width,
                height: v.height,
                device_scale_factor: v.device_scale_factor,
                is_mobile: v.is_mobile,
            }),
            locale: parsed.locale,
            timezone: parsed.timezone,
        };
        Ok(Some(snapshot))
    }

    async fn current_url(&self) -> QuarryResult<String> {
        self.state.lock().await.current_url.clone().ok_or_else(|| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                "kernel: no current URL; call goto() first",
            )
        })
    }

    async fn post_bytes(
        &self,
        browser_id: &str,
        action: &str,
        body: serde_json::Value,
    ) -> QuarryResult<Bytes> {
        let url = self.endpoint(&format!("v1/browsers/{browser_id}/{action}"));
        let resp = self
            .http
            .post(&url)
            .header("authorization", self.auth_header())
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                QuarryError::new(
                    ErrorCode::DriverFailed,
                    format!("kernel {action} failed: {e}"),
                )
            })?;

        let status = resp.status();
        let bytes = resp.bytes().await.map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                format!("kernel {action} body read failed: {e}"),
            )
        })?;

        if !status.is_success() {
            return Err(QuarryError::new(
                ErrorCode::DriverFailed,
                format!(
                    "kernel {action} returned {}: {}",
                    status.as_u16(),
                    String::from_utf8_lossy(&bytes)
                ),
            ));
        }
        Ok(bytes)
    }
}

#[async_trait]
impl BrowserDriver for KernelDriver {
    async fn acquire(&self, lease: &BrowserLease) -> QuarryResult<BrowserSession> {
        self.ensure_browser(lease).await?;
        Ok(BrowserSession {
            lease: lease.clone(),
            inner: Arc::new(Mutex::new(SessionInner {
                connected: true,
                pages_served: 0,
            })),
        })
    }

    async fn release(&self, session: BrowserSession) -> QuarryResult<()> {
        // Best-effort: capture session state from Kernel before deleting,
        // then persist back via ProfileStore so the next acquire can restore.
        // The state-fetch is bounded by a tight timeout (5s) — we'd rather
        // lose the capture than block release for the default 60s when
        // Kernel hangs. Release MUST be fast because callers hold the
        // lease until it returns.
        let captured = if self.profiles.is_some() {
            let id_opt = self.state.lock().await.browser_id.clone();
            if let Some(id) = id_opt {
                let fetch_fut = self.fetch_session_state(&id);
                match tokio::time::timeout(Duration::from_secs(5), fetch_fut).await {
                    Ok(Ok(snap)) => snap,
                    Ok(Err(e)) => {
                        tracing::warn!(error = %e, "kernel state fetch failed; skipping capture");
                        None
                    }
                    Err(_) => {
                        tracing::warn!("kernel state fetch timed out at 5s; skipping capture");
                        None
                    }
                }
            } else {
                None
            }
        } else {
            None
        };

        let mut state = self.state.lock().await;
        if let Some(id) = state.browser_id.take() {
            let url = self.endpoint(&format!("v1/browsers/{id}"));
            let _ = self
                .http
                .delete(&url)
                .header("authorization", self.auth_header())
                .send()
                .await;
        }
        state.current_url = None;
        state.cdp_url = None;
        state.replay_url = None;
        state.live_view_url = None;
        state.expires_at = None;
        drop(state);

        if let (Some(store), Some(snapshot)) = (&self.profiles, captured) {
            if let Err(e) = store
                .save(&session.lease.org_id, &session.lease.profile_id, &snapshot)
                .await
            {
                tracing::warn!(error = %e, "kernel session capture save failed");
            }
        }
        Ok(())
    }

    async fn goto(&self, session: &BrowserSession, url: &str) -> QuarryResult<()> {
        let id = self.ensure_browser(&session.lease).await?;
        self.post_bytes(&id, "goto", json!({ "url": url })).await?;
        self.state.lock().await.current_url = Some(url.to_string());
        let mut inner = session.inner.lock().await;
        inner.pages_served = inner.pages_served.saturating_add(1);
        Ok(())
    }

    async fn content(&self, session: &BrowserSession) -> QuarryResult<Bytes> {
        let id = self.ensure_browser(&session.lease).await?;
        let url = self.current_url().await?;
        self.post_bytes(&id, "content", json!({ "url": url })).await
    }

    async fn screenshot(&self, session: &BrowserSession, full_page: bool) -> QuarryResult<Bytes> {
        let id = self.ensure_browser(&session.lease).await?;
        let url = self.current_url().await?;
        self.post_bytes(
            &id,
            "screenshot",
            json!({ "url": url, "full_page": full_page, "type": "png" }),
        )
        .await
    }

    async fn pdf(&self, session: &BrowserSession) -> QuarryResult<Bytes> {
        let id = self.ensure_browser(&session.lease).await?;
        let url = self.current_url().await?;
        self.post_bytes(&id, "pdf", json!({ "url": url })).await
    }

    async fn wait_for(
        &self,
        session: &BrowserSession,
        selector: &str,
        timeout_ms: u32,
    ) -> QuarryResult<()> {
        let id = self.ensure_browser(&session.lease).await?;
        self.post_bytes(
            &id,
            "wait_for",
            json!({ "selector": selector, "timeout_ms": timeout_ms }),
        )
        .await
        .map(|_| ())
    }

    async fn click(&self, session: &BrowserSession, selector: &str) -> QuarryResult<()> {
        let id = self.ensure_browser(&session.lease).await?;
        self.post_bytes(&id, "click", json!({ "selector": selector }))
            .await
            .map(|_| ())
    }

    async fn type_text(
        &self,
        session: &BrowserSession,
        selector: &str,
        text: &str,
    ) -> QuarryResult<()> {
        let id = self.ensure_browser(&session.lease).await?;
        self.post_bytes(&id, "type", json!({ "selector": selector, "text": text }))
            .await
            .map(|_| ())
    }

    async fn evaluate(
        &self,
        session: &BrowserSession,
        script: &str,
    ) -> QuarryResult<serde_json::Value> {
        let id = self.ensure_browser(&session.lease).await?;
        let bytes = self
            .post_bytes(&id, "evaluate", json!({ "script": script }))
            .await?;
        serde_json::from_slice::<serde_json::Value>(&bytes).map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                format!("kernel evaluate decode: {e}"),
            )
        })
    }

    async fn press(&self, session: &BrowserSession, key: &str) -> QuarryResult<()> {
        let id = self.ensure_browser(&session.lease).await?;
        self.post_bytes(&id, "press", json!({ "key": key }))
            .await
            .map(|_| ())
    }

    async fn scroll(
        &self,
        session: &BrowserSession,
        target: &crate::actions::ScrollTarget,
    ) -> QuarryResult<()> {
        let id = self.ensure_browser(&session.lease).await?;
        self.post_bytes(&id, "scroll", json!({ "target": target }))
            .await
            .map(|_| ())
    }

    async fn select(
        &self,
        session: &BrowserSession,
        selector: &str,
        value: &str,
    ) -> QuarryResult<()> {
        let id = self.ensure_browser(&session.lease).await?;
        self.post_bytes(
            &id,
            "select",
            json!({ "selector": selector, "value": value }),
        )
        .await
        .map(|_| ())
    }

    async fn back(&self, session: &BrowserSession) -> QuarryResult<()> {
        let id = self.ensure_browser(&session.lease).await?;
        self.post_bytes(&id, "back", json!({})).await.map(|_| ())
    }

    async fn forward(&self, session: &BrowserSession) -> QuarryResult<()> {
        let id = self.ensure_browser(&session.lease).await?;
        self.post_bytes(&id, "forward", json!({})).await.map(|_| ())
    }
}

fn opt_string(s: &str) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarry_core::ids::kinds;
    use quarry_core::lease::{BrowserLease, Capability, ProxyAffinity};
    use wiremock::matchers::{header, method, path as wpath};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn make_lease() -> BrowserLease {
        BrowserLease {
            lease_id: kinds::LeaseKind::new(),
            profile_id: kinds::ProfileKind::new(),
            session_affinity_key: "org_demo".into(),
            proxy_affinity: ProxyAffinity {
                pool: "p".into(),
                sticky_key: None,
            },
            ttl_s: 60,
            capabilities: vec![Capability::Js],
            artifact_bucket: "b".into(),
            persist_profile: false,
            viewport: None,
            org_id: "test_org".into(),
        }
    }

    #[test]
    fn config_defaults_are_safe() {
        let c = KernelConfig::new("https://api.kernel.so", "k_test");
        assert_eq!(c.ttl_s, 0);
        assert!(!c.enable_replay);
        assert!(!c.enable_live_view);
    }

    #[tokio::test]
    async fn acquire_creates_browser_and_caches_cdp_url() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/v1/browsers"))
            .and(header("authorization", "Bearer k_test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "browser_id": "b_abc",
                "cdp_url": "wss://cdp.kernel.so/sessions/b_abc",
                "live_view_url": "https://kernel.so/live/b_abc",
                "replay_url": null,
                "expires_at": null,
            })))
            .mount(&server)
            .await;

        let driver = KernelDriver::new(KernelConfig::new(server.uri(), "k_test"));
        let session = driver.acquire(&make_lease()).await.unwrap();
        assert!(session.lease.profile_id.to_string().len() > 0);
        assert_eq!(driver.browser_id().await.as_deref(), Some("b_abc"));
        assert!(driver.cdp_url().await.unwrap().starts_with("wss://"));
        assert!(driver.live_view_url().await.is_some());
    }

    #[tokio::test]
    async fn release_calls_delete_browser() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/v1/browsers"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "browser_id": "b_xyz",
                "cdp_url": "wss://x",
            })))
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(wpath("/v1/browsers/b_xyz"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success": true})))
            .mount(&server)
            .await;

        let driver = KernelDriver::new(KernelConfig::new(server.uri(), "k"));
        let session = driver.acquire(&make_lease()).await.unwrap();
        driver.release(session).await.unwrap();
        assert!(driver.browser_id().await.is_none());
    }

    #[tokio::test]
    async fn goto_then_content_uses_browser_id() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/v1/browsers"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "browser_id": "b_1",
                "cdp_url": "wss://x",
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(wpath("/v1/browsers/b_1/goto"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(wpath("/v1/browsers/b_1/content"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"<html>kernel</html>"))
            .mount(&server)
            .await;

        let driver = KernelDriver::new(KernelConfig::new(server.uri(), "k"));
        let session = driver.acquire(&make_lease()).await.unwrap();
        driver.goto(&session, "https://example.com").await.unwrap();
        let html = driver.content(&session).await.unwrap();
        assert_eq!(&html[..], b"<html>kernel</html>");
    }

    #[tokio::test]
    async fn create_5xx_returns_driver_failed() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/v1/browsers"))
            .respond_with(ResponseTemplate::new(502).set_body_string("upstream"))
            .mount(&server)
            .await;

        let driver = KernelDriver::new(KernelConfig::new(server.uri(), "k"));
        let err = driver.acquire(&make_lease()).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::DriverFailed);
    }

    #[tokio::test]
    async fn create_429_returns_rate_limited() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/v1/browsers"))
            .respond_with(ResponseTemplate::new(429).set_body_string("slow"))
            .mount(&server)
            .await;

        let driver = KernelDriver::new(KernelConfig::new(server.uri(), "k"));
        let err = driver.acquire(&make_lease()).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::RateLimited);
    }

    #[tokio::test]
    async fn acceptance_wait_for_dispatches_selector_and_timeout() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/v1/browsers"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "browser_id": "b_w",
                "cdp_url": "wss://x",
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(wpath("/v1/browsers/b_w/wait_for"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .mount(&server)
            .await;

        let driver = KernelDriver::new(KernelConfig::new(server.uri(), "k"));
        let session = driver.acquire(&make_lease()).await.unwrap();
        driver
            .wait_for(&session, "#dynamic-content", 5_000)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn acceptance_evaluate_returns_json_value() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/v1/browsers"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "browser_id": "b_e",
                "cdp_url": "wss://x",
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(wpath("/v1/browsers/b_e/evaluate"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": 42,
                "type": "number"
            })))
            .mount(&server)
            .await;

        let driver = KernelDriver::new(KernelConfig::new(server.uri(), "k"));
        let session = driver.acquire(&make_lease()).await.unwrap();
        let v = driver.evaluate(&session, "1 + 41").await.unwrap();
        assert_eq!(v["result"].as_i64(), Some(42));
    }

    #[tokio::test]
    async fn acceptance_screenshot_returns_png_bytes() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/v1/browsers"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "browser_id": "b_s",
                "cdp_url": "wss://x",
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(wpath("/v1/browsers/b_s/goto"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(wpath("/v1/browsers/b_s/screenshot"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"\x89PNG\r\n\x1a\n"))
            .mount(&server)
            .await;

        let driver = KernelDriver::new(KernelConfig::new(server.uri(), "k"));
        let session = driver.acquire(&make_lease()).await.unwrap();
        driver.goto(&session, "https://x.com").await.unwrap();
        let png = driver.screenshot(&session, true).await.unwrap();
        assert_eq!(&png[0..4], b"\x89PNG");
    }

    #[tokio::test]
    async fn acceptance_pdf_returns_pdf_bytes() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/v1/browsers"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "browser_id": "b_p",
                "cdp_url": "wss://x",
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(wpath("/v1/browsers/b_p/goto"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(wpath("/v1/browsers/b_p/pdf"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"%PDF-1.4\n"))
            .mount(&server)
            .await;

        let driver = KernelDriver::new(KernelConfig::new(server.uri(), "k"));
        let session = driver.acquire(&make_lease()).await.unwrap();
        driver.goto(&session, "https://x.com").await.unwrap();
        let pdf = driver.pdf(&session).await.unwrap();
        assert!(pdf.starts_with(b"%PDF"));
    }

    #[tokio::test]
    async fn acceptance_action_script_click_type_press_select_back_forward() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/v1/browsers"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "browser_id": "b_a",
                "cdp_url": "wss://x",
            })))
            .mount(&server)
            .await;
        // Each action endpoint returns 200 ok.
        for action in ["click", "type", "press", "select", "back", "forward"] {
            Mock::given(method("POST"))
                .and(wpath(format!("/v1/browsers/b_a/{action}")))
                .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
                .mount(&server)
                .await;
        }

        let driver = KernelDriver::new(KernelConfig::new(server.uri(), "k"));
        let session = driver.acquire(&make_lease()).await.unwrap();

        driver.click(&session, "#submit").await.unwrap();
        driver
            .type_text(&session, "#email", "alice@example.com")
            .await
            .unwrap();
        driver.press(&session, "Enter").await.unwrap();
        driver.select(&session, "#country", "US").await.unwrap();
        driver.back(&session).await.unwrap();
        driver.forward(&session).await.unwrap();
    }

    #[tokio::test]
    async fn acceptance_profile_restore_seeds_cookies_on_create() {
        use crate::session::{Cookie, InMemoryProfileStore, SessionSnapshot};
        use wiremock::matchers::body_partial_json;

        let server = MockServer::start().await;

        // Verify the create-browser POST body includes the seeded cookie
        // and localStorage by matching its JSON shape.
        Mock::given(method("POST"))
            .and(wpath("/v1/browsers"))
            .and(body_partial_json(json!({
                "cookies": [{
                    "name": "session",
                    "value": "abc123",
                }],
                "local_storage": [{
                    "key": "ui.theme",
                    "value": "dark",
                }],
                "user_agent": "CustomUA/1.0",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "browser_id": "b_r",
                "cdp_url": "wss://x",
            })))
            .expect(1)
            .mount(&server)
            .await;

        let store = Arc::new(InMemoryProfileStore::new());
        let lease = make_lease();
        store
            .save(
                &lease.org_id,
                &lease.profile_id,
                &SessionSnapshot {
                    cookies: vec![Cookie {
                        name: "session".into(),
                        value: "abc123".into(),
                        domain: "example.com".into(),
                        path: "/".into(),
                        secure: true,
                        http_only: true,
                        expires: None,
                    }],
                    local_storage: vec![("ui.theme".into(), "dark".into())],
                    user_agent: Some("CustomUA/1.0".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let driver =
            KernelDriver::new(KernelConfig::new(server.uri(), "k")).with_profile_store(store);
        let _session = driver.acquire(&lease).await.unwrap();

        // Wiremock asserts `expect(1)` was satisfied on Drop. If the body
        // didn't match, the mock would never have responded and acquire()
        // would have failed.
    }

    #[tokio::test]
    async fn acceptance_release_captures_state_into_profile_store() {
        use crate::session::InMemoryProfileStore;

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/v1/browsers"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "browser_id": "b_cap",
                "cdp_url": "wss://x",
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(wpath("/v1/browsers/b_cap/state"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "cookies": [{
                    "name": "auth",
                    "value": "freshtoken",
                    "domain": "example.com",
                    "path": "/",
                    "secure": true,
                    "http_only": true
                }],
                "local_storage": [{"key": "ui.theme", "value": "dark"}],
                "user_agent": "CapturedUA/2.0"
            })))
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(wpath("/v1/browsers/b_cap"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let store = Arc::new(InMemoryProfileStore::new());
        let lease = make_lease();
        let driver = KernelDriver::new(KernelConfig::new(server.uri(), "k"))
            .with_profile_store(store.clone());
        let session = driver.acquire(&lease).await.unwrap();
        driver.release(session).await.unwrap();

        let restored = store
            .load(&lease.org_id, &lease.profile_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(restored.cookies.len(), 1);
        assert_eq!(restored.cookies[0].name, "auth");
        assert_eq!(restored.cookies[0].value, "freshtoken");
        assert_eq!(restored.user_agent.as_deref(), Some("CapturedUA/2.0"));
    }

    #[test]
    fn opt_string_returns_none_for_empty() {
        assert_eq!(opt_string(""), None);
        assert_eq!(opt_string("/"), Some("/".to_string()));
    }
}
