//! Chromiumoxide CDP driver.
//!
//! Implements [`BrowserDriver`] using the [`chromiumoxide`] crate, which
//! drives a local Chromium/Chrome process over the Chrome DevTools Protocol.
//!
//! Feature-gated behind `chromiumoxide`. The driver launches a single
//! browser process per `ChromiumoxideDriver` and reuses it across leases;
//! each `goto` opens a fresh page tab and replaces the previous one.

use async_trait::async_trait;
use bytes::Bytes;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use ::chromiumoxide::cdp::browser_protocol::page::{
    CaptureScreenshotFormat, CaptureScreenshotParamsBuilder, PrintToPdfParams,
};
use ::chromiumoxide::{Browser, BrowserConfig, Page};
use futures::StreamExt;
use std::time::{Duration, Instant};

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::lease::BrowserLease;

use crate::actions::ScrollTarget;
use crate::{BrowserDriver, BrowserSession, SessionInner};

/// Local Chromium driver backed by chromiumoxide.
///
/// Holds a lazy `Browser` handle and the most recent `Page`. The CDP event
/// handler is spawned onto the tokio runtime and aborted on `release`.
pub struct ChromiumoxideDriver {
    browser: Arc<Mutex<Option<Browser>>>,
    page: Arc<Mutex<Option<Page>>>,
    handler: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl Default for ChromiumoxideDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl ChromiumoxideDriver {
    pub fn new() -> Self {
        Self {
            browser: Arc::new(Mutex::new(None)),
            page: Arc::new(Mutex::new(None)),
            handler: Arc::new(Mutex::new(None)),
        }
    }

    async fn ensure_browser(&self) -> QuarryResult<()> {
        let mut guard = self.browser.lock().await;
        if guard.is_some() {
            return Ok(());
        }

        let config = BrowserConfig::builder().build().map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "chromiumoxide config build failed")
                .with_details(json!({ "error": e.to_string() }))
        })?;

        let (browser, mut events) = Browser::launch(config).await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "chromiumoxide launch failed")
                .with_details(json!({ "error": e.to_string() }))
        })?;

        let handle = tokio::spawn(async move {
            while let Some(_event) = events.next().await {
                // drain CDP events; explicit handling can be added later
            }
        });

        *guard = Some(browser);
        *self.handler.lock().await = Some(handle);
        Ok(())
    }

    async fn current_page(&self) -> QuarryResult<Page> {
        self.page.lock().await.clone().ok_or_else(|| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                "chromiumoxide: no current page; call goto() first",
            )
        })
    }
}

#[async_trait]
impl BrowserDriver for ChromiumoxideDriver {
    async fn acquire(&self, lease: &BrowserLease) -> QuarryResult<BrowserSession> {
        self.ensure_browser().await?;
        Ok(BrowserSession {
            lease: lease.clone(),
            inner: Arc::new(Mutex::new(SessionInner {
                connected: true,
                pages_served: 0,
            })),
        })
    }

    async fn release(&self, _session: BrowserSession) -> QuarryResult<()> {
        // Drop the current page; keep the browser alive for reuse across leases.
        *self.page.lock().await = None;
        Ok(())
    }

    async fn goto(&self, session: &BrowserSession, url: &str) -> QuarryResult<()> {
        self.ensure_browser().await?;
        let browser_guard = self.browser.lock().await;
        let browser = browser_guard.as_ref().ok_or_else(|| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                "chromiumoxide: browser not initialized",
            )
        })?;

        let page = browser.new_page(url).await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "chromiumoxide new_page failed")
                .with_details(json!({ "error": e.to_string(), "url": url }))
        })?;

        *self.page.lock().await = Some(page);

        let mut inner = session.inner.lock().await;
        inner.pages_served = inner.pages_served.saturating_add(1);
        Ok(())
    }

    async fn content(&self, _session: &BrowserSession) -> QuarryResult<Bytes> {
        let page = self.current_page().await?;
        let html = page.content().await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "chromiumoxide content failed")
                .with_details(json!({ "error": e.to_string() }))
        })?;
        Ok(Bytes::from(html.into_bytes()))
    }

    async fn screenshot(&self, _session: &BrowserSession, full_page: bool) -> QuarryResult<Bytes> {
        let page = self.current_page().await?;
        let params = CaptureScreenshotParamsBuilder::default()
            .format(CaptureScreenshotFormat::Png)
            .capture_beyond_viewport(full_page)
            .build();
        let png = page.screenshot(params).await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "chromiumoxide screenshot failed")
                .with_details(json!({ "error": e.to_string(), "full_page": full_page }))
        })?;
        Ok(Bytes::from(png))
    }

    async fn pdf(&self, _session: &BrowserSession) -> QuarryResult<Bytes> {
        let page = self.current_page().await?;
        let pdf = page.pdf(PrintToPdfParams::default()).await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "chromiumoxide pdf failed")
                .with_details(json!({ "error": e.to_string() }))
        })?;
        Ok(Bytes::from(pdf))
    }

    async fn wait_for(
        &self,
        _session: &BrowserSession,
        selector: &str,
        timeout_ms: u32,
    ) -> QuarryResult<()> {
        let page = self.current_page().await?;
        let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
        let poll = Duration::from_millis(50);
        loop {
            match page.find_element(selector).await {
                Ok(_) => return Ok(()),
                Err(_) if Instant::now() < deadline => tokio::time::sleep(poll).await,
                Err(e) => {
                    return Err(QuarryError::new(
                        ErrorCode::DriverFailed,
                        "wait_for selector timed out",
                    )
                    .with_details(json!({
                        "selector": selector,
                        "timeout_ms": timeout_ms,
                        "error": e.to_string(),
                    })));
                }
            }
        }
    }

    async fn click(&self, _session: &BrowserSession, selector: &str) -> QuarryResult<()> {
        let page = self.current_page().await?;
        let element = page.find_element(selector).await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "click: element not found")
                .with_details(json!({ "selector": selector, "error": e.to_string() }))
        })?;
        element.click().await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "click failed")
                .with_details(json!({ "selector": selector, "error": e.to_string() }))
        })?;
        Ok(())
    }

    async fn type_text(
        &self,
        _session: &BrowserSession,
        selector: &str,
        text: &str,
    ) -> QuarryResult<()> {
        let page = self.current_page().await?;
        let element = page.find_element(selector).await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "type: element not found")
                .with_details(json!({ "selector": selector, "error": e.to_string() }))
        })?;
        element.focus().await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "type: focus failed")
                .with_details(json!({ "selector": selector, "error": e.to_string() }))
        })?;
        element.type_str(text).await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "type failed")
                .with_details(json!({ "selector": selector, "error": e.to_string() }))
        })?;
        Ok(())
    }

    async fn scroll(&self, _session: &BrowserSession, target: &ScrollTarget) -> QuarryResult<()> {
        let page = self.current_page().await?;
        match target {
            ScrollTarget::Top => {
                page.evaluate("window.scrollTo(0, 0)").await.map_err(|e| {
                    QuarryError::new(ErrorCode::DriverFailed, "scroll-top failed")
                        .with_details(json!({ "error": e.to_string() }))
                })?;
            }
            ScrollTarget::Bottom => {
                page.evaluate("window.scrollTo(0, document.documentElement.scrollHeight)")
                    .await
                    .map_err(|e| {
                        QuarryError::new(ErrorCode::DriverFailed, "scroll-bottom failed")
                            .with_details(json!({ "error": e.to_string() }))
                    })?;
            }
            ScrollTarget::Selector(sel) => {
                let element = page.find_element(sel).await.map_err(|e| {
                    QuarryError::new(ErrorCode::DriverFailed, "scroll: element not found")
                        .with_details(json!({ "selector": sel, "error": e.to_string() }))
                })?;
                element.scroll_into_view().await.map_err(|e| {
                    QuarryError::new(ErrorCode::DriverFailed, "scroll_into_view failed")
                        .with_details(json!({ "selector": sel, "error": e.to_string() }))
                })?;
            }
        }
        Ok(())
    }

    async fn press(&self, _session: &BrowserSession, key: &str) -> QuarryResult<()> {
        let page = self.current_page().await?;
        // press_key is page-global; we use the body element only as a handle.
        let element = page.find_element("body").await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "press: body not found")
                .with_details(json!({ "error": e.to_string() }))
        })?;
        element.press_key(key).await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "press_key failed")
                .with_details(json!({ "key": key, "error": e.to_string() }))
        })?;
        Ok(())
    }

    async fn evaluate(
        &self,
        _session: &BrowserSession,
        script: &str,
    ) -> QuarryResult<serde_json::Value> {
        let page = self.current_page().await?;
        let result = page.evaluate(script).await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "evaluate failed")
                .with_details(json!({ "error": e.to_string() }))
        })?;
        let value = result
            .into_value::<serde_json::Value>()
            .unwrap_or(serde_json::Value::Null);
        Ok(value)
    }

    async fn select(
        &self,
        _session: &BrowserSession,
        selector: &str,
        value: &str,
    ) -> QuarryResult<()> {
        let page = self.current_page().await?;
        let script = format!(
            r#"(() => {{
                const el = document.querySelector({sel});
                if (!el) throw new Error('select: element not found');
                el.value = {val};
                el.dispatchEvent(new Event('change', {{ bubbles: true }}));
            }})()"#,
            sel = serde_json::to_string(selector).unwrap_or_default(),
            val = serde_json::to_string(value).unwrap_or_default(),
        );
        page.evaluate(script).await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "select failed")
                .with_details(json!({ "selector": selector, "value": value, "error": e.to_string() }))
        })?;
        Ok(())
    }

    async fn back(&self, _session: &BrowserSession) -> QuarryResult<()> {
        let page = self.current_page().await?;
        page.evaluate("window.history.back()").await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "back failed")
                .with_details(json!({ "error": e.to_string() }))
        })?;
        tokio::time::sleep(Duration::from_millis(100)).await;
        Ok(())
    }
}

impl Drop for ChromiumoxideDriver {
    fn drop(&mut self) {
        // Best-effort: abort the event-loop task so it doesn't outlive the driver.
        if let Ok(mut guard) = self.handler.try_lock() {
            if let Some(h) = guard.take() {
                h.abort();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarry_core::ids::kinds;
    use quarry_core::lease::{BrowserLease, Capability, ProxyAffinity};

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

    /// Integration test that requires a local Chromium/Chrome binary.
    /// Skipped unless `CHROMIUMOXIDE_TEST=1` is set.
    #[tokio::test]
    async fn launches_and_fetches_content() {
        if std::env::var("CHROMIUMOXIDE_TEST").ok().as_deref() != Some("1") {
            eprintln!("skipping: set CHROMIUMOXIDE_TEST=1 to run");
            return;
        }

        let driver = ChromiumoxideDriver::new();
        let lease = make_lease();
        let session = driver.acquire(&lease).await.expect("acquire");
        driver
            .goto(
                &session,
                "data:text/html,<html><body><h1>hi</h1></body></html>",
            )
            .await
            .expect("goto");

        let html = driver.content(&session).await.expect("content");
        assert!(html
            .windows(3)
            .any(|w| w == b"hi="[..3].to_ascii_lowercase().as_slice() || w == b"<h1"));

        driver.release(session).await.expect("release");
    }
}
