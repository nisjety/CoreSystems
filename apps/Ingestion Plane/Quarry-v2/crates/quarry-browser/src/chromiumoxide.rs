//! Chromiumoxide CDP driver.
//!
//! Implements [`BrowserDriver`] using the [`chromiumoxide`] crate, which
//! drives a local Chromium/Chrome process over the Chrome DevTools Protocol.
//!
//! Feature-gated behind `chromiumoxide`. The driver launches a single
//! browser process per `ChromiumoxideDriver` and reuses it across leases;
//! each `goto` opens a fresh page tab and replaces the previous one.

use ::chromiumoxide::cdp::browser_protocol::network::CookieParam;
use async_trait::async_trait;
use bytes::Bytes;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use ::chromiumoxide::cdp::browser_protocol::page::{
    CaptureScreenshotFormat, CaptureScreenshotParamsBuilder, PrintToPdfParams,
};
use ::chromiumoxide::handler::viewport::Viewport as ChromiumViewport;
use ::chromiumoxide::{Browser, BrowserConfig, Page};
use futures::StreamExt;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::lease::{BrowserLease, BrowserViewport};

use crate::actions::ScrollTarget;
use crate::session::{Cookie, ProfileStore, SessionSnapshot, Viewport};
use crate::{BrowserDriver, BrowserSession, SessionInner};

/// Local Chromium driver backed by chromiumoxide.
///
/// Holds a lazy `Browser` handle and the most recent `Page`. The CDP event
/// handler is spawned onto the tokio runtime and aborted on `release`.
pub struct ChromiumoxideDriver {
    browser: Arc<Mutex<Option<Browser>>>,
    page: Arc<Mutex<Option<Page>>>,
    handler: Arc<Mutex<Option<JoinHandle<()>>>>,
    active_viewport: Arc<Mutex<Option<BrowserViewport>>>,
    profiles: Option<Arc<dyn ProfileStore>>,
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
            active_viewport: Arc::new(Mutex::new(None)),
            profiles: None,
        }
    }

    pub fn with_profile_store(mut self, store: Arc<dyn ProfileStore>) -> Self {
        self.profiles = Some(store);
        self
    }

    async fn ensure_browser(&self, viewport: Option<BrowserViewport>) -> QuarryResult<()> {
        let mut guard = self.browser.lock().await;
        let mut active_viewport = self.active_viewport.lock().await;
        if guard.is_some() && *active_viewport == viewport {
            return Ok(());
        }
        if let Some(mut browser) = guard.take() {
            if let Err(err) = browser.close().await {
                tracing::warn!(error = %err, "chromiumoxide browser close before relaunch failed");
            }
        }
        if let Some(handle) = self.handler.lock().await.take() {
            handle.abort();
        }
        *self.page.lock().await = None;

        let launch_dirs = chromium_launch_dirs().map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                "chromiumoxide launch directory setup failed",
            )
            .with_details(json!({ "error": e.to_string() }))
        })?;
        let mut builder = BrowserConfig::builder()
            .user_data_dir(&launch_dirs.user_data)
            .arg("--disable-dev-shm-usage")
            .arg("--disable-breakpad")
            .arg("--disable-crash-reporter")
            .arg("--disable-crashpad")
            .arg("--noerrdialogs")
            .arg(format!(
                "--crash-dumps-dir={}",
                launch_dirs.crash_dumps.display()
            ));
        if let Some(viewport) = viewport {
            builder = builder.viewport(chromium_viewport(viewport));
        }
        if env_truthy("QUARRY_BROWSER_NO_SANDBOX") {
            builder = builder.no_sandbox();
        }

        let config = builder.build().map_err(|e| {
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
        *active_viewport = viewport;
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

    async fn load_snapshot(&self, lease: &BrowserLease) -> Option<SessionSnapshot> {
        if !lease.persist_profile {
            return None;
        }
        self.profiles
            .as_ref()?
            .load(&lease.org_id, &lease.profile_id)
            .await
            .unwrap_or_else(|err| {
                tracing::warn!(error = %err, "chromiumoxide profile load failed; starting fresh");
                None
            })
    }

    async fn persist_current_page(&self, session: &BrowserSession) -> QuarryResult<()> {
        if !session.lease.persist_profile {
            return Ok(());
        }
        let Some(store) = self.profiles.as_ref() else {
            return Ok(());
        };
        let page = match self.page.lock().await.clone() {
            Some(page) => page,
            None => return Ok(()),
        };
        let mut snapshot = tokio::time::timeout(Duration::from_secs(5), capture_snapshot(&page))
            .await
            .map_err(|_| {
                QuarryError::new(
                    ErrorCode::DriverFailed,
                    "chromiumoxide profile capture timed out",
                )
            })??;
        snapshot.viewport = session.lease.viewport.map(session_viewport);
        store
            .save(&session.lease.org_id, &session.lease.profile_id, &snapshot)
            .await
    }
}

fn env_truthy(name: &str) -> bool {
    std::env::var(name)
        .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

struct ChromiumLaunchDirs {
    user_data: PathBuf,
    crash_dumps: PathBuf,
}

fn chromium_launch_dirs() -> std::io::Result<ChromiumLaunchDirs> {
    let root = std::env::var_os("QUARRY_BROWSER_PROFILE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("quarry-chromium"));
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let namespace = format!("edge-{}-{stamp}", std::process::id());
    let user_data = root.join("profiles").join(&namespace);
    let crash_dumps = root.join("crashpad").join(&namespace);
    std::fs::create_dir_all(&user_data)?;
    std::fs::create_dir_all(&crash_dumps)?;
    Ok(ChromiumLaunchDirs {
        user_data,
        crash_dumps,
    })
}

fn cookie_param(cookie: &Cookie) -> QuarryResult<CookieParam> {
    CookieParam::builder()
        .name(cookie.name.clone())
        .value(cookie.value.clone())
        .domain(cookie.domain.clone())
        .path(cookie.path.clone())
        .secure(cookie.secure)
        .http_only(cookie.http_only)
        .build()
        .map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                "chromiumoxide cookie restore failed",
            )
            .with_details(json!({ "error": e.to_string(), "name": cookie.name }))
        })
}

async fn hydrate_cookies(page: &Page, snapshot: &SessionSnapshot) -> QuarryResult<()> {
    if snapshot.cookies.is_empty() {
        return Ok(());
    }
    let cookies = snapshot
        .cookies
        .iter()
        .map(cookie_param)
        .collect::<QuarryResult<Vec<_>>>()?;
    page.set_cookies(cookies).await.map_err(|e| {
        QuarryError::new(
            ErrorCode::DriverFailed,
            "chromiumoxide cookie hydrate failed",
        )
        .with_details(json!({ "error": e.to_string() }))
    })?;
    Ok(())
}

async fn hydrate_storage(page: &Page, snapshot: &SessionSnapshot) -> QuarryResult<bool> {
    if snapshot.local_storage.is_empty() && snapshot.session_storage.is_empty() {
        return Ok(false);
    }
    let script = format!(
        r#"(() => {{
            const localEntries = {local};
            const sessionEntries = {session};
            for (const [key, value] of localEntries) window.localStorage.setItem(key, value);
            for (const [key, value] of sessionEntries) window.sessionStorage.setItem(key, value);
            return true;
        }})()"#,
        local = serde_json::to_string(&snapshot.local_storage).unwrap_or_else(|_| "[]".into()),
        session = serde_json::to_string(&snapshot.session_storage).unwrap_or_else(|_| "[]".into()),
    );
    page.evaluate(script).await.map_err(|e| {
        QuarryError::new(
            ErrorCode::DriverFailed,
            "chromiumoxide storage hydrate failed",
        )
        .with_details(json!({ "error": e.to_string() }))
    })?;
    Ok(true)
}

async fn install_popup_bridge(page: &Page) -> QuarryResult<()> {
    page.evaluate(
        r#"(() => {
            if (window.__quarryPopupBridgeInstalled) return true;
            Object.defineProperty(window, '__quarryPopupBridgeInstalled', {
                value: true,
                configurable: false,
                enumerable: false,
                writable: false
            });
            const originalOpen = window.open ? window.open.bind(window) : null;
            window.open = function quarryPopupBridge(url, target, features) {
                if (typeof url === 'string' && url.trim()) {
                    window.location.assign(url);
                    return window;
                }
                return originalOpen ? originalOpen(url, target, features) : null;
            };
            document.addEventListener('click', (event) => {
                const target = event.target && event.target.closest
                    ? event.target.closest('a[target="_blank"], a[rel~="external"]')
                    : null;
                if (!target || !target.href) return;
                event.preventDefault();
                window.location.assign(target.href);
            }, true);
            return true;
        })()"#,
    )
    .await
    .map_err(|e| {
        QuarryError::new(
            ErrorCode::DriverFailed,
            "chromiumoxide popup bridge install failed",
        )
        .with_details(json!({ "error": e.to_string() }))
    })?;
    Ok(())
}

async fn capture_snapshot(page: &Page) -> QuarryResult<SessionSnapshot> {
    let raw = page.get_cookies().await.map_err(|e| {
        QuarryError::new(
            ErrorCode::DriverFailed,
            "chromiumoxide cookie capture failed",
        )
        .with_details(json!({ "error": e.to_string() }))
    })?;
    let cookies = raw
        .into_iter()
        .map(|cookie| Cookie {
            name: cookie.name,
            value: cookie.value,
            domain: cookie.domain,
            path: cookie.path,
            secure: cookie.secure,
            http_only: cookie.http_only,
            expires: None,
        })
        .collect();

    let value = page
        .evaluate(
            r#"(() => ({
                localStorage: Object.entries(window.localStorage || {}),
                sessionStorage: Object.entries(window.sessionStorage || {}),
                userAgent: navigator.userAgent || null,
                locale: navigator.language || null,
                timezone: Intl.DateTimeFormat().resolvedOptions().timeZone || null
            }))()"#,
        )
        .await
        .map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                "chromiumoxide storage capture failed",
            )
            .with_details(json!({ "error": e.to_string() }))
        })?
        .into_value::<serde_json::Value>()
        .unwrap_or(serde_json::Value::Null);

    Ok(SessionSnapshot {
        cookies,
        local_storage: storage_entries(value.get("localStorage")),
        session_storage: storage_entries(value.get("sessionStorage")),
        indexed_db: vec![],
        user_agent: value
            .get("userAgent")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        viewport: None,
        locale: value
            .get("locale")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        timezone: value
            .get("timezone")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
    })
}

fn storage_entries(value: Option<&serde_json::Value>) -> Vec<(String, String)> {
    value
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    let pair = entry.as_array()?;
                    let key = pair.first()?.as_str()?.to_owned();
                    let value = pair.get(1)?.as_str()?.to_owned();
                    Some((key, value))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn chromium_viewport(viewport: BrowserViewport) -> ChromiumViewport {
    ChromiumViewport {
        width: viewport.width,
        height: viewport.height,
        device_scale_factor: Some(viewport.device_scale_factor),
        emulating_mobile: viewport.is_mobile,
        is_landscape: viewport.width >= viewport.height,
        has_touch: viewport.is_mobile,
    }
}

fn session_viewport(viewport: BrowserViewport) -> Viewport {
    Viewport {
        width: viewport.width,
        height: viewport.height,
        device_scale_factor: viewport.device_scale_factor,
        is_mobile: viewport.is_mobile,
    }
}

#[async_trait]
impl BrowserDriver for ChromiumoxideDriver {
    async fn acquire(&self, lease: &BrowserLease) -> QuarryResult<BrowserSession> {
        self.ensure_browser(lease.viewport).await?;
        Ok(BrowserSession {
            lease: lease.clone(),
            inner: Arc::new(Mutex::new(SessionInner {
                connected: true,
                pages_served: 0,
            })),
        })
    }

    async fn release(&self, _session: BrowserSession) -> QuarryResult<()> {
        if let Err(err) = self.persist_current_page(&_session).await {
            tracing::warn!(error = %err, "chromiumoxide profile capture skipped");
        }
        // Drop the current page; keep the browser alive for reuse across leases.
        *self.page.lock().await = None;
        Ok(())
    }

    async fn goto(&self, session: &BrowserSession, url: &str) -> QuarryResult<()> {
        self.ensure_browser(session.lease.viewport).await?;
        let browser_guard = self.browser.lock().await;
        let browser = browser_guard.as_ref().ok_or_else(|| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                "chromiumoxide: browser not initialized",
            )
        })?;

        let snapshot = self.load_snapshot(&session.lease).await;
        let page = browser.new_page("about:blank").await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "chromiumoxide new_page failed")
                .with_details(json!({ "error": e.to_string(), "url": "about:blank" }))
        })?;
        if let Some(snapshot) = snapshot.as_ref() {
            hydrate_cookies(&page, snapshot).await?;
        }
        page.goto(url).await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "chromiumoxide goto failed")
                .with_details(json!({ "error": e.to_string(), "url": url }))
        })?;
        if let Some(snapshot) = snapshot.as_ref() {
            if hydrate_storage(&page, snapshot).await? {
                page.reload().await.map_err(|e| {
                    QuarryError::new(
                        ErrorCode::DriverFailed,
                        "chromiumoxide profile reload failed",
                    )
                    .with_details(json!({ "error": e.to_string(), "url": url }))
                })?;
            }
        }
        install_popup_bridge(&page).await?;

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
            QuarryError::new(ErrorCode::DriverFailed, "select failed").with_details(
                json!({ "selector": selector, "value": value, "error": e.to_string() }),
            )
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

    async fn forward(&self, _session: &BrowserSession) -> QuarryResult<()> {
        let page = self.current_page().await?;
        page.evaluate("window.history.forward()")
            .await
            .map_err(|e| {
                QuarryError::new(ErrorCode::DriverFailed, "forward failed")
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
            persist_profile: false,
            viewport: None,
            org_id: "test_org".into(),
        }
    }

    #[test]
    fn chromium_viewport_maps_desktop_dimensions() {
        let viewport = chromium_viewport(BrowserViewport {
            width: 1280,
            height: 800,
            device_scale_factor: 1.0,
            is_mobile: false,
        });

        assert_eq!(viewport.width, 1280);
        assert_eq!(viewport.height, 800);
        assert_eq!(viewport.device_scale_factor, Some(1.0));
        assert!(!viewport.emulating_mobile);
        assert!(viewport.is_landscape);
        assert!(!viewport.has_touch);
    }

    #[test]
    fn session_viewport_preserves_persisted_profile_dimensions() {
        let viewport = session_viewport(BrowserViewport {
            width: 390,
            height: 844,
            device_scale_factor: 3.0,
            is_mobile: true,
        });

        assert_eq!(viewport.width, 390);
        assert_eq!(viewport.height, 844);
        assert_eq!(viewport.device_scale_factor, 3.0);
        assert!(viewport.is_mobile);
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
