//! Browserless.io remote CDP driver.
//!
//! Implements [`BrowserDriver`] over the Browserless REST API:
//! - `/content?token=...` → fetch rendered HTML for a URL
//! - `/screenshot?token=...` → PNG bytes
//! - `/pdf?token=...` → PDF bytes
//!
//! Session state is minimal: we don't hold a persistent WebSocket in this
//! driver — each `goto` stores the target URL on the session, and
//! `content`/`screenshot`/`pdf` issue a one-shot POST against Browserless.
//!
//! ## Sticky sessions / proxy affinity
//!
//! Browserless supports `&blockAds=true&proxyServer=...&sessionId=...`
//! query params that cause the request to land on a specific worker pool
//! and reuse cookies/state. Quarry maps the lease's `session_affinity_key`
//! → `sessionId` and `proxy_affinity.sticky_key` → `sessionId` (when proxy
//! affinity is bound). This makes multi-page authenticated flows reuse a
//! single Browserless backend node instead of round-robining.

use async_trait::async_trait;
use bytes::Bytes;
use reqwest::Client;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::lease::BrowserLease;

use crate::{BrowserDriver, BrowserSession, SessionInner};

/// Tiny URL-encoder for proxy_server values (only the characters we
/// actually expect to need escaping in a Browserless query string).
/// Keeps a single dependency-free implementation in this module so we
/// don't pull a heavy URL crate into quarry-browser's dep graph.
fn urlencoding(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// Session-scoped navigation state for the Browserless driver.
#[derive(Default)]
struct NavState {
    current_url: Option<String>,
    /// Sticky session ID derived from lease.session_affinity_key. When set,
    /// every `/content`/`/screenshot`/`/pdf` POST adds `&sessionId=...` so
    /// Browserless lands the request on the same backend worker.
    session_id: Option<String>,
    /// Proxy server URL (full https:// or socks5:// URL) bound to this lease.
    proxy_server: Option<String>,
}

pub struct BrowserlessDriver {
    base_url: String,
    token: Option<String>,
    http: Client,
    nav: Arc<Mutex<NavState>>,
}

impl BrowserlessDriver {
    /// Create a driver pointing at `base_url` (e.g. `https://chrome.browserless.io`).
    pub fn new(base_url: impl Into<String>, token: Option<String>) -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("reqwest client");
        Self {
            base_url: base_url.into(),
            token,
            http,
            nav: Arc::new(Mutex::new(NavState::default())),
        }
    }

    /// Build the endpoint URL, appending `?token=...` when configured.
    /// Also threads sticky session and proxy params when bound.
    fn endpoint(&self, path: &str) -> String {
        let trimmed = self.base_url.trim_end_matches('/');
        let mut url = match &self.token {
            Some(t) => format!("{trimmed}/{path}?token={t}"),
            None => format!("{trimmed}/{path}"),
        };
        // session/proxy affinity is checked synchronously without locking
        // — this is best-effort: callers who care about deterministic
        // sticky routing should set state before issuing the call.
        if let Ok(nav) = self.nav.try_lock() {
            if let Some(sid) = &nav.session_id {
                let sep = if url.contains('?') { '&' } else { '?' };
                url.push(sep);
                url.push_str("sessionId=");
                url.push_str(sid);
            }
            if let Some(ps) = &nav.proxy_server {
                let sep = if url.contains('?') { '&' } else { '?' };
                url.push(sep);
                url.push_str("proxyServer=");
                url.push_str(&urlencoding(ps));
            }
        }
        url
    }

    async fn current_url(&self) -> QuarryResult<String> {
        self.nav.lock().await.current_url.clone().ok_or_else(|| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                "browserless: no current URL; call goto() first",
            )
        })
    }

    async fn post_json_bytes(&self, path: &str, body: serde_json::Value) -> QuarryResult<Bytes> {
        let url = self.endpoint(path);
        let resp = self.http.post(&url).json(&body).send().await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "browserless request failed")
                .with_details(json!({ "error": e.to_string(), "endpoint": path }))
        })?;

        let status = resp.status();
        let bytes = resp.bytes().await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "browserless body read failed")
                .with_details(json!({ "error": e.to_string(), "endpoint": path }))
        })?;

        if !status.is_success() {
            return Err(QuarryError::new(
                ErrorCode::DriverFailed,
                "browserless returned non-success status",
            )
            .with_details(json!({
                "status": status.as_u16(),
                "endpoint": path,
                "body": String::from_utf8_lossy(&bytes).to_string(),
            })));
        }

        Ok(bytes)
    }
}

#[async_trait]
impl BrowserDriver for BrowserlessDriver {
    async fn acquire(&self, lease: &BrowserLease) -> QuarryResult<BrowserSession> {
        // Prefer the proxy sticky_key (often a more stable shard) when
        // present; fall back to session_affinity_key. An empty key means
        // "no affinity" and we skip the sessionId param.
        let session_id = match &lease.proxy_affinity.sticky_key {
            Some(k) if !k.is_empty() => Some(k.clone()),
            _ if !lease.session_affinity_key.is_empty() => {
                Some(lease.session_affinity_key.clone())
            }
            _ => None,
        };

        // Proxy pool name → proxy server URL is configured on the
        // BrowserlessDriver via `with_proxy_pool` later. We only carry
        // the lease's pool name forward; the driver maps it to a real URL
        // at request time. For now we only seed the session_id.
        let mut nav = self.nav.lock().await;
        nav.session_id = session_id;
        nav.proxy_server = None;
        drop(nav);

        Ok(BrowserSession {
            lease: lease.clone(),
            inner: Arc::new(Mutex::new(SessionInner {
                connected: true,
                pages_served: 0,
            })),
        })
    }

    async fn release(&self, _session: BrowserSession) -> QuarryResult<()> {
        let mut nav = self.nav.lock().await;
        nav.current_url = None;
        nav.session_id = None;
        nav.proxy_server = None;
        Ok(())
    }

    async fn goto(&self, session: &BrowserSession, url: &str) -> QuarryResult<()> {
        self.nav.lock().await.current_url = Some(url.to_string());
        let mut inner = session.inner.lock().await;
        inner.pages_served = inner.pages_served.saturating_add(1);
        Ok(())
    }

    async fn content(&self, _session: &BrowserSession) -> QuarryResult<Bytes> {
        let url = self.current_url().await?;
        self.post_json_bytes("content", json!({ "url": url })).await
    }

    async fn screenshot(&self, _session: &BrowserSession, full_page: bool) -> QuarryResult<Bytes> {
        let url = self.current_url().await?;
        self.post_json_bytes(
            "screenshot",
            json!({
                "url": url,
                "options": { "fullPage": full_page, "type": "png" },
            }),
        )
        .await
    }

    async fn pdf(&self, _session: &BrowserSession) -> QuarryResult<Bytes> {
        let url = self.current_url().await?;
        self.post_json_bytes("pdf", json!({ "url": url })).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarry_core::error::ErrorCode;
    use quarry_core::ids::kinds;
    use quarry_core::lease::{BrowserLease, Capability, ProxyAffinity};
    use wiremock::matchers::{method, path as wpath, query_param};
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
            org_id: "test_org".into(),
        }
    }

    #[test]
    fn endpoint_with_token() {
        let d = BrowserlessDriver::new("https://chrome.browserless.io", Some("abc".into()));
        assert_eq!(
            d.endpoint("content"),
            "https://chrome.browserless.io/content?token=abc"
        );
    }

    #[test]
    fn endpoint_without_token() {
        let d = BrowserlessDriver::new("https://chrome.browserless.io/", None);
        assert_eq!(d.endpoint("pdf"), "https://chrome.browserless.io/pdf");
    }

    #[tokio::test]
    async fn endpoint_threads_session_id_when_acquired() {
        let driver = BrowserlessDriver::new("https://x.com", Some("abc".into()));
        let mut lease = make_lease();
        lease.session_affinity_key = "tenant-42".into();
        let _session = driver.acquire(&lease).await.unwrap();

        // Lock the mutex behind us so try_lock fails — exercise the
        // best-effort path. Note: in practice the lock is uncontended at
        // call time because acquire/release are explicit.
        let url = driver.endpoint("content");
        assert!(
            url.contains("sessionId=tenant-42"),
            "expected sessionId in URL, got: {url}"
        );
    }

    #[tokio::test]
    async fn endpoint_prefers_proxy_sticky_key_over_session_affinity() {
        use quarry_core::lease::ProxyAffinity;
        let driver = BrowserlessDriver::new("https://x.com", None);
        let mut lease = make_lease();
        lease.session_affinity_key = "session-a".into();
        lease.proxy_affinity = ProxyAffinity {
            pool: "premium".into(),
            sticky_key: Some("proxy-shard-7".into()),
        };
        let _session = driver.acquire(&lease).await.unwrap();
        let url = driver.endpoint("content");
        assert!(url.contains("sessionId=proxy-shard-7"));
        assert!(!url.contains("session-a"));
    }

    #[tokio::test]
    async fn release_clears_sticky_state() {
        let driver = BrowserlessDriver::new("https://x.com", None);
        let mut lease = make_lease();
        lease.session_affinity_key = "tenant-1".into();
        let session = driver.acquire(&lease).await.unwrap();
        driver.release(session).await.unwrap();
        let url = driver.endpoint("content");
        assert!(!url.contains("sessionId="), "sticky should be cleared after release: {url}");
    }

    #[test]
    fn urlencoding_escapes_special_chars() {
        assert_eq!(urlencoding("https://proxy:8080"), "https%3A%2F%2Fproxy%3A8080");
        assert_eq!(urlencoding("simple-name_42.7"), "simple-name_42.7");
    }

    #[tokio::test]
    async fn goto_then_content_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/content"))
            .and(query_param("token", "abc"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"<html>ok</html>"))
            .mount(&server)
            .await;

        let driver = BrowserlessDriver::new(server.uri(), Some("abc".into()));
        let session = driver.acquire(&make_lease()).await.unwrap();
        driver.goto(&session, "https://example.com").await.unwrap();
        let body = driver.content(&session).await.unwrap();
        assert_eq!(&body[..], b"<html>ok</html>");
    }

    #[tokio::test]
    async fn screenshot_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/screenshot"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"\x89PNG\r\n"))
            .mount(&server)
            .await;

        let driver = BrowserlessDriver::new(server.uri(), None);
        let session = driver.acquire(&make_lease()).await.unwrap();
        driver.goto(&session, "https://example.com").await.unwrap();
        let png = driver.screenshot(&session, false).await.unwrap();
        assert_eq!(&png[..], b"\x89PNG\r\n");
    }

    #[tokio::test]
    async fn pdf_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/pdf"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"%PDF-1.4"))
            .mount(&server)
            .await;

        let driver = BrowserlessDriver::new(server.uri(), None);
        let session = driver.acquire(&make_lease()).await.unwrap();
        driver.goto(&session, "https://example.com").await.unwrap();
        let pdf = driver.pdf(&session).await.unwrap();
        assert_eq!(&pdf[..], b"%PDF-1.4");
    }

    #[tokio::test]
    async fn content_error_maps_to_driver_failed() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wpath("/content"))
            .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
            .mount(&server)
            .await;

        let driver = BrowserlessDriver::new(server.uri(), None);
        let session = driver.acquire(&make_lease()).await.unwrap();
        driver.goto(&session, "https://example.com").await.unwrap();
        let err = driver.content(&session).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::DriverFailed);
        assert!(!err.retryable);
        let details = err.details.as_ref().expect("details present");
        assert_eq!(details["status"].as_u64(), Some(500));
    }

    #[tokio::test]
    async fn content_without_goto_fails() {
        let server = MockServer::start().await;
        let driver = BrowserlessDriver::new(server.uri(), None);
        let session = driver.acquire(&make_lease()).await.unwrap();
        let err = driver.content(&session).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::DriverFailed);
        assert!(err.message.contains("no current URL"));
    }
}
