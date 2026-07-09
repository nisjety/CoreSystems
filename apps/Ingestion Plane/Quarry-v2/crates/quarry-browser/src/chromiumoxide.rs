//! Chromiumoxide CDP driver.
//!
//! Implements [`BrowserDriver`] using the [`chromiumoxide`] crate, which
//! drives a local Chromium/Chrome process over the Chrome DevTools Protocol.
//!
//! Feature-gated behind `chromiumoxide`. The driver launches a single
//! browser process per `ChromiumoxideDriver` and reuses it across leases;
//! each lease owns a small tab set keyed by its session affinity key.

use ::chromiumoxide::cdp::browser_protocol::browser::BrowserContextId;
use ::chromiumoxide::cdp::browser_protocol::input::{
    DispatchMouseEventParams, DispatchMouseEventPointerType, DispatchMouseEventType, MouseButton,
};
use ::chromiumoxide::cdp::browser_protocol::log::EventEntryAdded;
use ::chromiumoxide::cdp::browser_protocol::network::{
    CookieParam, EventRequestWillBeSent, EventResponseReceived,
};
use ::chromiumoxide::cdp::browser_protocol::target::{
    CreateBrowserContextParams, CreateTargetParamsBuilder,
};
use async_trait::async_trait;
use bytes::Bytes;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use ::chromiumoxide::cdp::browser_protocol::page::{
    CaptureScreenshotFormat, CaptureScreenshotParamsBuilder, EventLifecycleEvent,
    EventScreencastFrame, PrintToPdfParams, ScreencastFrameAckParams, StartScreencastFormat,
    StartScreencastParams, StopScreencastParams,
};
use ::chromiumoxide::cdp::js_protocol::runtime::EventConsoleApiCalled;
use ::chromiumoxide::handler::viewport::Viewport as ChromiumViewport;
use ::chromiumoxide::{Browser, BrowserConfig, Page};
use futures::StreamExt;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::lease::{BrowserLease, BrowserViewport};

use crate::actions::ScrollTarget;
use crate::session::{Cookie, ProfileStore, SessionSnapshot, Viewport};
use crate::{
    BrowserDevtoolsEvent, BrowserDriver, BrowserSession, BrowserTab, LiveFrame, LiveFrameFormat,
    LiveFrameOptions, SessionInner,
};

const LIVE_FRAME_CACHE_TTL_MS: u64 = 250;
const DEVTOOLS_EVENT_BUFFER_LIMIT: usize = 512;

#[derive(Debug, Clone)]
struct CachedLiveFrame {
    session_key: String,
    tab_id: String,
    page_epoch: u64,
    options: LiveFrameOptions,
    captured_at: Instant,
    frame: LiveFrame,
}

#[derive(Debug, Clone)]
struct TabPage {
    tab_id: String,
    page: Page,
    title_hint: Option<String>,
    url_hint: Option<String>,
}

#[derive(Debug, Default)]
struct SessionPages {
    active_tab_id: Option<String>,
    tabs: Vec<TabPage>,
    next_tab_index: u64,
    /// The isolated CDP browser context this session's tabs live in. Created
    /// lazily on the session's first tab and reused for every subsequent tab
    /// so multiple tabs in the same session share one cookie/storage jar,
    /// while distinct sessions (distinct `session_affinity_key`s) never do.
    browser_context_id: Option<BrowserContextId>,
}

/// Local Chromium driver backed by chromiumoxide.
///
/// Holds a lazy `Browser` handle and per-session `Page` tabs. The CDP event
/// handler is spawned onto the tokio runtime and aborted on `release`.
pub struct ChromiumoxideDriver {
    browser: Arc<Mutex<Option<Browser>>>,
    pages: Arc<Mutex<HashMap<String, SessionPages>>>,
    handler: Arc<Mutex<Option<JoinHandle<()>>>>,
    active_viewport: Arc<Mutex<Option<BrowserViewport>>>,
    page_epoch: Arc<Mutex<u64>>,
    live_frame_cache: Arc<Mutex<Option<CachedLiveFrame>>>,
    devtools_events: Arc<Mutex<HashMap<String, Vec<BrowserDevtoolsEvent>>>>,
    devtools_sequence: Arc<Mutex<u64>>,
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
            pages: Arc::new(Mutex::new(HashMap::new())),
            handler: Arc::new(Mutex::new(None)),
            active_viewport: Arc::new(Mutex::new(None)),
            page_epoch: Arc::new(Mutex::new(0)),
            live_frame_cache: Arc::new(Mutex::new(None)),
            devtools_events: Arc::new(Mutex::new(HashMap::new())),
            devtools_sequence: Arc::new(Mutex::new(0)),
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
        self.pages.lock().await.clear();
        self.devtools_events.lock().await.clear();
        self.invalidate_live_frame_cache().await;

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

    fn session_key(session: &BrowserSession) -> String {
        session.lease.session_affinity_key.clone()
    }

    async fn active_tab_page(&self, session: &BrowserSession) -> Option<(String, Page)> {
        let session_key = Self::session_key(session);
        let pages = self.pages.lock().await;
        let session_pages = pages.get(&session_key)?;
        let active_tab_id = session_pages.active_tab_id.as_ref()?;
        session_pages
            .tabs
            .iter()
            .find(|tab| &tab.tab_id == active_tab_id)
            .map(|tab| (tab.tab_id.clone(), tab.page.clone()))
    }

    async fn current_tab_page(&self, session: &BrowserSession) -> QuarryResult<(String, Page)> {
        self.active_tab_page(session).await.ok_or_else(|| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                "chromiumoxide: no current page; call goto() first",
            )
        })
    }

    async fn current_page(&self, session: &BrowserSession) -> QuarryResult<Page> {
        self.current_tab_page(session).await.map(|(_, page)| page)
    }

    async fn open_tab_page(
        &self,
        session: &BrowserSession,
        url: Option<&str>,
    ) -> QuarryResult<(String, Page)> {
        self.ensure_browser(session.lease.viewport).await?;
        let browser_guard = self.browser.lock().await;
        let browser = browser_guard.as_ref().ok_or_else(|| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                "chromiumoxide: browser not initialized",
            )
        })?;

        let session_key = Self::session_key(session);
        let (tab_id, existing_context_id) = {
            let mut pages = self.pages.lock().await;
            let session_pages = pages.entry(session_key.clone()).or_default();
            session_pages.next_tab_index = session_pages.next_tab_index.saturating_add(1);
            let tab_id = format!("tab-{}", session_pages.next_tab_index);
            (tab_id, session_pages.browser_context_id.clone())
        };

        // Every session gets its own isolated CDP browser context on its first
        // tab, created once and reused for every later tab on that same
        // session. Without this, `browser.new_page` puts every session's tabs
        // — across every org, user, profile, and ZDR declaration — into the
        // browser's single default context, sharing one global cookie jar.
        let context_id = match existing_context_id {
            Some(id) => id,
            None => {
                let id = browser
                    .create_browser_context(CreateBrowserContextParams::default())
                    .await
                    .map_err(|e| {
                        QuarryError::new(
                            ErrorCode::DriverFailed,
                            "chromiumoxide create_browser_context failed",
                        )
                        .with_details(json!({ "error": e.to_string() }))
                    })?;
                let mut pages = self.pages.lock().await;
                let session_pages = pages.entry(session_key.clone()).or_default();
                session_pages.browser_context_id = Some(id.clone());
                id
            }
        };

        let snapshot = self.load_snapshot(&session.lease).await;
        let initial_url = url.unwrap_or("about:blank");
        let new_page_params = CreateTargetParamsBuilder::default()
            .url("about:blank")
            .browser_context_id(context_id)
            .build()
            .map_err(|e| {
                QuarryError::new(
                    ErrorCode::DriverFailed,
                    "chromiumoxide new_page params build failed",
                )
                .with_details(json!({ "error": e }))
            })?;
        let page = browser.new_page(new_page_params).await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "chromiumoxide new_page failed")
                .with_details(json!({ "error": e.to_string(), "url": "about:blank" }))
        })?;
        self.install_devtools_collectors(session_key.clone(), tab_id.clone(), &page)
            .await;
        if initial_url != "about:blank" {
            page.goto(initial_url).await.map_err(|e| {
                QuarryError::new(ErrorCode::DriverFailed, "chromiumoxide goto failed")
                    .with_details(json!({ "error": e.to_string(), "url": initial_url }))
            })?;
            if let Some(snapshot) = snapshot.as_ref() {
                // chromiumoxide's `Page::set_cookies` hard-requires the page to
                // already be on a real http(s) URL — it validates `page.url()`
                // itself before ever looking at any per-cookie `url` field, and
                // errors "Blank page can not have cookie" otherwise. Cookies must
                // therefore be hydrated AFTER the initial navigation, not before
                // it (as this previously did): restoring a profile with saved
                // cookies into a fresh tab/session used to fail outright — every
                // attach of a persistent, cookie-bearing profile 502'd — because
                // hydration ran while the page was still `about:blank`.
                let cookies_restored = !snapshot.cookies.is_empty();
                if cookies_restored {
                    hydrate_cookies(&page, snapshot).await?;
                }
                let storage_restored = hydrate_storage(&page, snapshot).await?;
                if cookies_restored || storage_restored {
                    // Reload so the already-completed initial `goto` request is
                    // replayed with the freshly restored cookies/storage attached
                    // (matches the pre-existing storage-only reload behavior).
                    page.reload().await.map_err(|e| {
                        QuarryError::new(
                            ErrorCode::DriverFailed,
                            "chromiumoxide profile reload failed",
                        )
                        .with_details(json!({ "error": e.to_string(), "url": initial_url }))
                    })?;
                }
            }
        }
        // A genuinely blank tab (no initial URL — e.g. `new_tab` with no `url`)
        // has no origin to scope cookies to, and chromiumoxide cannot set cookies
        // on `about:blank` regardless; cookie/storage hydration is skipped for
        // that case instead of hard-failing tab creation.
        install_popup_bridge(&page).await?;

        let mut pages = self.pages.lock().await;
        let session_pages = pages.entry(session_key).or_default();
        session_pages.tabs.push(TabPage {
            tab_id: tab_id.clone(),
            page: page.clone(),
            title_hint: None,
            url_hint: Some(initial_url.to_owned()),
        });
        session_pages.active_tab_id = Some(tab_id.clone());
        Ok((tab_id, page))
    }

    async fn update_tab_metadata(&self, session_key: &str, tab_id: &str, page: &Page) {
        let url = page.url().await.ok().flatten();
        let title = page_title(page).await;
        let mut pages = self.pages.lock().await;
        if let Some(session_pages) = pages.get_mut(session_key) {
            if let Some(tab) = session_pages
                .tabs
                .iter_mut()
                .find(|tab| tab.tab_id == tab_id)
            {
                tab.url_hint = url;
                tab.title_hint = title;
            }
        }
    }

    async fn install_devtools_collectors(&self, session_key: String, tab_id: String, page: &Page) {
        match page.event_listener::<EventConsoleApiCalled>().await {
            Ok(mut events) => {
                let event_buffer = self.devtools_events.clone();
                let sequence = self.devtools_sequence.clone();
                let session_key = session_key.clone();
                let tab_id = tab_id.clone();
                tokio::spawn(async move {
                    while let Some(event) = events.next().await {
                        let event = normalize_console_event(&tab_id, &event);
                        push_devtools_event(&event_buffer, &sequence, &session_key, event).await;
                    }
                });
            }
            Err(err) => tracing::debug!(error = %err, "console devtools listener unavailable"),
        }

        match page.event_listener::<EventEntryAdded>().await {
            Ok(mut events) => {
                let event_buffer = self.devtools_events.clone();
                let sequence = self.devtools_sequence.clone();
                let session_key = session_key.clone();
                let tab_id = tab_id.clone();
                tokio::spawn(async move {
                    while let Some(event) = events.next().await {
                        let event = normalize_log_event(&tab_id, &event);
                        push_devtools_event(&event_buffer, &sequence, &session_key, event).await;
                    }
                });
            }
            Err(err) => tracing::debug!(error = %err, "log devtools listener unavailable"),
        }

        match page.event_listener::<EventRequestWillBeSent>().await {
            Ok(mut events) => {
                let event_buffer = self.devtools_events.clone();
                let sequence = self.devtools_sequence.clone();
                let session_key = session_key.clone();
                let tab_id = tab_id.clone();
                tokio::spawn(async move {
                    while let Some(event) = events.next().await {
                        let event = normalize_request_event(&tab_id, &event);
                        push_devtools_event(&event_buffer, &sequence, &session_key, event).await;
                    }
                });
            }
            Err(err) => tracing::debug!(error = %err, "request devtools listener unavailable"),
        }

        match page.event_listener::<EventResponseReceived>().await {
            Ok(mut events) => {
                let event_buffer = self.devtools_events.clone();
                let sequence = self.devtools_sequence.clone();
                let session_key = session_key.clone();
                let tab_id = tab_id.clone();
                tokio::spawn(async move {
                    while let Some(event) = events.next().await {
                        let event = normalize_response_event(&tab_id, &event);
                        push_devtools_event(&event_buffer, &sequence, &session_key, event).await;
                    }
                });
            }
            Err(err) => tracing::debug!(error = %err, "response devtools listener unavailable"),
        }

        match page.event_listener::<EventLifecycleEvent>().await {
            Ok(mut events) => {
                let event_buffer = self.devtools_events.clone();
                let sequence = self.devtools_sequence.clone();
                tokio::spawn(async move {
                    while let Some(event) = events.next().await {
                        let event = normalize_lifecycle_event(&tab_id, &event);
                        push_devtools_event(&event_buffer, &sequence, &session_key, event).await;
                    }
                });
            }
            Err(err) => tracing::debug!(error = %err, "lifecycle devtools listener unavailable"),
        }
    }

    async fn invalidate_live_frame_cache(&self) {
        let mut epoch = self.page_epoch.lock().await;
        *epoch = epoch.saturating_add(1);
        *self.live_frame_cache.lock().await = None;
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
        let Some((_tab_id, page)) = self.active_tab_page(session).await else {
            return Ok(());
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

async fn push_devtools_event(
    buffers: &Arc<Mutex<HashMap<String, Vec<BrowserDevtoolsEvent>>>>,
    sequence: &Arc<Mutex<u64>>,
    session_key: &str,
    mut event: BrowserDevtoolsEvent,
) {
    let mut seq = sequence.lock().await;
    *seq = seq.saturating_add(1);
    event.sequence = *seq;
    drop(seq);

    let mut buffers = buffers.lock().await;
    let events = buffers.entry(session_key.to_owned()).or_default();
    events.push(event);
    if events.len() > DEVTOOLS_EVENT_BUFFER_LIMIT {
        let excess = events.len() - DEVTOOLS_EVENT_BUFFER_LIMIT;
        events.drain(0..excess);
    }
}

fn normalize_console_event(tab_id: &str, event: &EventConsoleApiCalled) -> BrowserDevtoolsEvent {
    let payload = serde_json::to_value(event).unwrap_or(serde_json::Value::Null);
    let method = payload
        .get("type")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let text = payload
        .get("args")
        .and_then(serde_json::Value::as_array)
        .map(|args| {
            args.iter()
                .filter_map(remote_object_text)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|text| !text.is_empty());
    let level = method.as_deref().map(console_method_level);
    BrowserDevtoolsEvent {
        sequence: 0,
        tab_id: Some(tab_id.to_owned()),
        category: "console".to_owned(),
        name: "Runtime.consoleAPICalled".to_owned(),
        level,
        method,
        url: None,
        status: None,
        text: text.map(truncate_devtools_text),
        timestamp_ms: now_ms(),
        payload,
    }
}

fn normalize_log_event(tab_id: &str, event: &EventEntryAdded) -> BrowserDevtoolsEvent {
    let payload = serde_json::to_value(event).unwrap_or(serde_json::Value::Null);
    let entry = payload.get("entry").unwrap_or(&serde_json::Value::Null);
    let level = entry
        .get("level")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let text = entry
        .get("text")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let url = entry
        .get("url")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    BrowserDevtoolsEvent {
        sequence: 0,
        tab_id: Some(tab_id.to_owned()),
        category: "console".to_owned(),
        name: "Log.entryAdded".to_owned(),
        level,
        method: None,
        url,
        status: None,
        text: text.map(truncate_devtools_text),
        timestamp_ms: now_ms(),
        payload,
    }
}

fn normalize_request_event(tab_id: &str, event: &EventRequestWillBeSent) -> BrowserDevtoolsEvent {
    let payload = serde_json::to_value(event).unwrap_or(serde_json::Value::Null);
    let request = payload.get("request").unwrap_or(&serde_json::Value::Null);
    let method = request
        .get("method")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let url = request
        .get("url")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let text = match (method.as_deref(), url.as_deref()) {
        (Some(method), Some(url)) => Some(format!("{method} {url}")),
        (Some(method), None) => Some(method.to_owned()),
        (None, Some(url)) => Some(url.to_owned()),
        (None, None) => None,
    };
    BrowserDevtoolsEvent {
        sequence: 0,
        tab_id: Some(tab_id.to_owned()),
        category: "network".to_owned(),
        name: "Network.requestWillBeSent".to_owned(),
        level: None,
        method,
        url,
        status: None,
        text: text.map(truncate_devtools_text),
        timestamp_ms: now_ms(),
        payload,
    }
}

fn normalize_response_event(tab_id: &str, event: &EventResponseReceived) -> BrowserDevtoolsEvent {
    let payload = serde_json::to_value(event).unwrap_or(serde_json::Value::Null);
    let response = payload.get("response").unwrap_or(&serde_json::Value::Null);
    let url = response
        .get("url")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let status = response
        .get("status")
        .and_then(serde_json::Value::as_i64)
        .and_then(|status| u16::try_from(status).ok());
    let text = match (status, url.as_deref()) {
        (Some(status), Some(url)) => Some(format!("{status} {url}")),
        (Some(status), None) => Some(status.to_string()),
        (None, Some(url)) => Some(url.to_owned()),
        (None, None) => None,
    };
    BrowserDevtoolsEvent {
        sequence: 0,
        tab_id: Some(tab_id.to_owned()),
        category: "network".to_owned(),
        name: "Network.responseReceived".to_owned(),
        level: None,
        method: None,
        url,
        status,
        text: text.map(truncate_devtools_text),
        timestamp_ms: now_ms(),
        payload,
    }
}

fn normalize_lifecycle_event(tab_id: &str, event: &EventLifecycleEvent) -> BrowserDevtoolsEvent {
    let payload = serde_json::to_value(event).unwrap_or(serde_json::Value::Null);
    let name = payload
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("lifecycle")
        .to_owned();
    BrowserDevtoolsEvent {
        sequence: 0,
        tab_id: Some(tab_id.to_owned()),
        category: "lifecycle".to_owned(),
        name: "Page.lifecycleEvent".to_owned(),
        level: None,
        method: Some(name.clone()),
        url: None,
        status: None,
        text: Some(name),
        timestamp_ms: now_ms(),
        payload,
    }
}

fn remote_object_text(value: &serde_json::Value) -> Option<String> {
    value
        .get("value")
        .map(json_scalar_text)
        .or_else(|| {
            value
                .get("description")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .or_else(|| {
            value
                .get("unserializableValue")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .map(truncate_devtools_text)
}

fn json_scalar_text(value: &serde_json::Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}

fn console_method_level(method: &str) -> String {
    match method {
        "error" | "assert" => "error",
        "warning" => "warning",
        "debug" => "debug",
        _ => "info",
    }
    .to_owned()
}

fn truncate_devtools_text(text: String) -> String {
    const MAX: usize = 1_000;
    if text.len() <= MAX {
        return text;
    }
    let mut end = MAX;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &text[..end])
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

async fn page_title(page: &Page) -> Option<String> {
    page.evaluate("document.title")
        .await
        .ok()
        .and_then(|value| value.into_value::<String>().ok())
        .map(|title| title.trim().to_owned())
        .filter(|title| !title.is_empty())
}

fn normalize_live_frame_options(options: LiveFrameOptions) -> LiveFrameOptions {
    LiveFrameOptions {
        format: options.format,
        quality: options.quality.clamp(1, 100),
        max_width: options.max_width.clamp(320, 3840),
        max_height: options.max_height.clamp(240, 2160),
        every_nth_frame: options.every_nth_frame.clamp(1, 10),
        timeout_ms: options.timeout_ms.clamp(100, 5_000),
    }
}

async fn capture_live_frame(page: &Page, options: LiveFrameOptions) -> QuarryResult<LiveFrame> {
    let mut events = page
        .event_listener::<EventScreencastFrame>()
        .await
        .map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                "chromiumoxide screencast listener failed",
            )
            .with_details(json!({ "error": e.to_string() }))
        })?;
    let format = match options.format {
        LiveFrameFormat::Jpeg => StartScreencastFormat::Jpeg,
        LiveFrameFormat::Png => StartScreencastFormat::Png,
    };
    let params = StartScreencastParams::builder()
        .format(format)
        .quality(options.quality as i64)
        .max_width(options.max_width as i64)
        .max_height(options.max_height as i64)
        .every_nth_frame(options.every_nth_frame as i64)
        .build();

    page.execute(params).await.map_err(|e| {
        QuarryError::new(
            ErrorCode::DriverFailed,
            "chromiumoxide screencast start failed",
        )
        .with_details(json!({ "error": e.to_string() }))
    })?;

    let timeout = Duration::from_millis(options.timeout_ms);
    let frame = match tokio::time::timeout(timeout, events.next()).await {
        Ok(Some(frame)) => frame,
        Ok(None) => {
            let _ = page.execute(StopScreencastParams::default()).await;
            return Err(QuarryError::new(
                ErrorCode::DriverFailed,
                "chromiumoxide screencast ended before a frame arrived",
            ));
        }
        Err(_) => {
            let _ = page.execute(StopScreencastParams::default()).await;
            return Err(QuarryError::new(
                ErrorCode::DriverFailed,
                "chromiumoxide screencast frame timed out",
            ));
        }
    };

    page.execute(ScreencastFrameAckParams::new(frame.session_id))
        .await
        .map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                "chromiumoxide screencast ack failed",
            )
            .with_details(json!({ "error": e.to_string() }))
        })?;
    if let Err(err) = page.execute(StopScreencastParams::default()).await {
        tracing::warn!(error = %err, "chromiumoxide screencast stop failed");
    }

    Ok(LiveFrame {
        mime_type: options.format.mime_type().to_owned(),
        data_base64: String::from(frame.data.clone()),
    })
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

    async fn release(&self, session: BrowserSession) -> QuarryResult<()> {
        if let Err(err) = self.persist_current_page(&session).await {
            tracing::warn!(error = %err, "chromiumoxide profile capture skipped");
        }
        let session_key = Self::session_key(&session);
        let removed = self.pages.lock().await.remove(&session_key);
        if let Some(session_pages) = removed {
            for tab in session_pages.tabs {
                if let Err(err) = tab.page.close().await {
                    tracing::debug!(
                        tab_id = %tab.tab_id,
                        error = %err,
                        "chromiumoxide tab close during release failed"
                    );
                }
            }
            if let Some(context_id) = session_pages.browser_context_id {
                let browser_guard = self.browser.lock().await;
                if let Some(browser) = browser_guard.as_ref() {
                    if let Err(err) = browser.dispose_browser_context(context_id).await {
                        tracing::warn!(
                            session_key = %session_key,
                            error = %err,
                            "chromiumoxide dispose_browser_context failed"
                        );
                    }
                }
            }
        }
        self.devtools_events.lock().await.remove(&session_key);
        self.invalidate_live_frame_cache().await;
        Ok(())
    }

    async fn goto(&self, session: &BrowserSession, url: &str) -> QuarryResult<()> {
        self.ensure_browser(session.lease.viewport).await?;
        let (tab_id, page, created) = match self.active_tab_page(session).await {
            Some((tab_id, page)) => (tab_id, page, false),
            None => {
                let (tab_id, page) = self.open_tab_page(session, Some(url)).await?;
                (tab_id, page, true)
            }
        };

        if !created {
            page.goto(url).await.map_err(|e| {
                QuarryError::new(ErrorCode::DriverFailed, "chromiumoxide goto failed")
                    .with_details(json!({ "error": e.to_string(), "url": url }))
            })?;
            install_popup_bridge(&page).await?;
        }

        let session_key = Self::session_key(session);
        self.update_tab_metadata(&session_key, &tab_id, &page).await;
        self.invalidate_live_frame_cache().await;

        let mut inner = session.inner.lock().await;
        inner.pages_served = inner.pages_served.saturating_add(1);
        Ok(())
    }

    async fn content(&self, _session: &BrowserSession) -> QuarryResult<Bytes> {
        let page = self.current_page(_session).await?;
        let html = page.content().await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "chromiumoxide content failed")
                .with_details(json!({ "error": e.to_string() }))
        })?;
        Ok(Bytes::from(html.into_bytes()))
    }

    async fn screenshot(&self, _session: &BrowserSession, full_page: bool) -> QuarryResult<Bytes> {
        let page = self.current_page(_session).await?;
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

    async fn live_frame(
        &self,
        session: &BrowserSession,
        options: LiveFrameOptions,
    ) -> QuarryResult<LiveFrame> {
        let options = normalize_live_frame_options(options);
        let session_key = Self::session_key(session);
        let (tab_id, page) = self.current_tab_page(session).await?;
        let page_epoch = *self.page_epoch.lock().await;
        let mut cache = self.live_frame_cache.lock().await;
        if let Some(cached) = cache.as_ref() {
            if cached.session_key == session_key
                && cached.tab_id == tab_id
                && cached.page_epoch == page_epoch
                && cached.options == options
                && cached.captured_at.elapsed() <= Duration::from_millis(LIVE_FRAME_CACHE_TTL_MS)
            {
                return Ok(cached.frame.clone());
            }
        }

        let frame = capture_live_frame(&page, options).await?;
        *cache = Some(CachedLiveFrame {
            session_key,
            tab_id,
            page_epoch,
            options,
            captured_at: Instant::now(),
            frame: frame.clone(),
        });
        Ok(frame)
    }

    async fn pdf(&self, _session: &BrowserSession) -> QuarryResult<Bytes> {
        let page = self.current_page(_session).await?;
        let pdf = page.pdf(PrintToPdfParams::default()).await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "chromiumoxide pdf failed")
                .with_details(json!({ "error": e.to_string() }))
        })?;
        Ok(Bytes::from(pdf))
    }

    async fn list_tabs(&self, session: &BrowserSession) -> QuarryResult<Vec<BrowserTab>> {
        let session_key = Self::session_key(session);
        let (active_tab_id, tabs) = {
            let pages = self.pages.lock().await;
            let Some(session_pages) = pages.get(&session_key) else {
                return Ok(Vec::new());
            };
            (
                session_pages.active_tab_id.clone(),
                session_pages.tabs.clone(),
            )
        };

        let mut result = Vec::with_capacity(tabs.len());
        let mut fresh = Vec::with_capacity(tabs.len());
        for tab in tabs {
            let url = tab.page.url().await.ok().flatten().or(tab.url_hint);
            let title = page_title(&tab.page).await.or(tab.title_hint);
            let active = active_tab_id.as_deref() == Some(tab.tab_id.as_str());
            fresh.push((tab.tab_id.clone(), title.clone(), url.clone()));
            result.push(BrowserTab {
                tab_id: tab.tab_id,
                title,
                url,
                active,
            });
        }

        let mut pages = self.pages.lock().await;
        if let Some(session_pages) = pages.get_mut(&session_key) {
            for (tab_id, title, url) in fresh {
                if let Some(tab) = session_pages
                    .tabs
                    .iter_mut()
                    .find(|tab| tab.tab_id == tab_id)
                {
                    tab.title_hint = title;
                    tab.url_hint = url;
                }
            }
        }

        Ok(result)
    }

    async fn new_tab(
        &self,
        session: &BrowserSession,
        url: Option<&str>,
    ) -> QuarryResult<BrowserTab> {
        let (tab_id, page) = self.open_tab_page(session, url).await?;
        let session_key = Self::session_key(session);
        self.update_tab_metadata(&session_key, &tab_id, &page).await;
        self.invalidate_live_frame_cache().await;
        let tabs = self.list_tabs(session).await?;
        tabs.into_iter()
            .find(|tab| tab.tab_id == tab_id)
            .ok_or_else(|| {
                QuarryError::new(
                    ErrorCode::DriverFailed,
                    "chromiumoxide new_tab disappeared after creation",
                )
            })
    }

    async fn select_tab(&self, session: &BrowserSession, tab_id: &str) -> QuarryResult<()> {
        let session_key = Self::session_key(session);
        let mut pages = self.pages.lock().await;
        let session_pages = pages.get_mut(&session_key).ok_or_else(|| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                "chromiumoxide: no tabs for session",
            )
        })?;
        let exists = session_pages.tabs.iter().any(|tab| tab.tab_id == tab_id);
        if !exists {
            return Err(
                QuarryError::new(ErrorCode::DriverFailed, "chromiumoxide: tab not found")
                    .with_details(json!({ "tab_id": tab_id })),
            );
        }
        session_pages.active_tab_id = Some(tab_id.to_owned());
        drop(pages);
        self.invalidate_live_frame_cache().await;
        Ok(())
    }

    async fn close_tab(&self, session: &BrowserSession, tab_id: &str) -> QuarryResult<()> {
        let session_key = Self::session_key(session);
        let removed = {
            let mut pages = self.pages.lock().await;
            let session_pages = pages.get_mut(&session_key).ok_or_else(|| {
                QuarryError::new(
                    ErrorCode::DriverFailed,
                    "chromiumoxide: no tabs for session",
                )
            })?;
            let index = session_pages
                .tabs
                .iter()
                .position(|tab| tab.tab_id == tab_id)
                .ok_or_else(|| {
                    QuarryError::new(ErrorCode::DriverFailed, "chromiumoxide: tab not found")
                        .with_details(json!({ "tab_id": tab_id }))
                })?;
            let removed = session_pages.tabs.remove(index);
            if session_pages.active_tab_id.as_deref() == Some(tab_id) {
                session_pages.active_tab_id =
                    session_pages.tabs.first().map(|tab| tab.tab_id.clone());
            }
            if session_pages.tabs.is_empty() {
                pages.remove(&session_key);
            }
            removed
        };

        if let Err(err) = removed.page.close().await {
            tracing::debug!(
                tab_id = %removed.tab_id,
                error = %err,
                "chromiumoxide close_tab failed"
            );
        }
        self.invalidate_live_frame_cache().await;
        Ok(())
    }

    async fn devtools_events(
        &self,
        session: &BrowserSession,
        after_sequence: u64,
        limit: usize,
    ) -> QuarryResult<Vec<BrowserDevtoolsEvent>> {
        let session_key = Self::session_key(session);
        let limit = limit.clamp(1, DEVTOOLS_EVENT_BUFFER_LIMIT);
        let buffers = self.devtools_events.lock().await;
        let events = buffers
            .get(&session_key)
            .map(|events| {
                events
                    .iter()
                    .filter(|event| event.sequence > after_sequence)
                    .take(limit)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        Ok(events)
    }

    async fn wait_for(
        &self,
        _session: &BrowserSession,
        selector: &str,
        timeout_ms: u32,
    ) -> QuarryResult<()> {
        let page = self.current_page(_session).await?;
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
        let page = self.current_page(_session).await?;
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

    async fn click_point(&self, _session: &BrowserSession, x: f64, y: f64) -> QuarryResult<()> {
        let page = self.current_page(_session).await?;
        let base = DispatchMouseEventParams::builder()
            .x(x)
            .y(y)
            .button(MouseButton::Left)
            .click_count(1)
            .pointer_type(DispatchMouseEventPointerType::Mouse);

        let moved = base
            .clone()
            .r#type(DispatchMouseEventType::MouseMoved)
            .buttons(0)
            .build()
            .map_err(|e| {
                QuarryError::new(
                    ErrorCode::DriverFailed,
                    format!("click_point build move failed: {e}"),
                )
            })?;
        let pressed = base
            .clone()
            .r#type(DispatchMouseEventType::MousePressed)
            .buttons(1)
            .build()
            .map_err(|e| {
                QuarryError::new(
                    ErrorCode::DriverFailed,
                    format!("click_point build press failed: {e}"),
                )
            })?;
        let released = base
            .r#type(DispatchMouseEventType::MouseReleased)
            .buttons(0)
            .build()
            .map_err(|e| {
                QuarryError::new(
                    ErrorCode::DriverFailed,
                    format!("click_point build release failed: {e}"),
                )
            })?;

        page.execute(moved).await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "click_point move failed")
                .with_details(json!({ "x": x, "y": y, "error": e.to_string() }))
        })?;
        page.execute(pressed).await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "click_point press failed")
                .with_details(json!({ "x": x, "y": y, "error": e.to_string() }))
        })?;
        page.execute(released).await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "click_point release failed")
                .with_details(json!({ "x": x, "y": y, "error": e.to_string() }))
        })?;
        Ok(())
    }

    async fn type_text(
        &self,
        _session: &BrowserSession,
        selector: &str,
        text: &str,
    ) -> QuarryResult<()> {
        let page = self.current_page(_session).await?;
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
        let page = self.current_page(_session).await?;
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

    async fn mouse_wheel(
        &self,
        _session: &BrowserSession,
        x: f64,
        y: f64,
        delta_x: f64,
        delta_y: f64,
    ) -> QuarryResult<()> {
        let page = self.current_page(_session).await?;
        let event = DispatchMouseEventParams::builder()
            .r#type(DispatchMouseEventType::MouseWheel)
            .x(x)
            .y(y)
            .button(MouseButton::None)
            .buttons(0)
            .delta_x(delta_x)
            .delta_y(delta_y)
            .pointer_type(DispatchMouseEventPointerType::Mouse)
            .build()
            .map_err(|e| {
                QuarryError::new(
                    ErrorCode::DriverFailed,
                    format!("mouse_wheel build failed: {e}"),
                )
            })?;

        page.execute(event).await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "mouse_wheel failed").with_details(json!({
                "x": x,
                "y": y,
                "delta_x": delta_x,
                "delta_y": delta_y,
                "error": e.to_string(),
            }))
        })?;
        Ok(())
    }

    async fn press(&self, _session: &BrowserSession, key: &str) -> QuarryResult<()> {
        let page = self.current_page(_session).await?;
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
        let page = self.current_page(_session).await?;
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
        let page = self.current_page(_session).await?;
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
        let page = self.current_page(_session).await?;
        page.evaluate("window.history.back()").await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "back failed")
                .with_details(json!({ "error": e.to_string() }))
        })?;
        tokio::time::sleep(Duration::from_millis(100)).await;
        Ok(())
    }

    async fn forward(&self, _session: &BrowserSession) -> QuarryResult<()> {
        let page = self.current_page(_session).await?;
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

    fn make_lease_with_key(key: &str) -> BrowserLease {
        let mut lease = make_lease();
        lease.session_affinity_key = key.into();
        lease
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

    #[test]
    fn live_frame_options_are_normalized() {
        let options = normalize_live_frame_options(LiveFrameOptions {
            format: LiveFrameFormat::Png,
            quality: 0,
            max_width: 10,
            max_height: 50_000,
            every_nth_frame: 0,
            timeout_ms: 10,
        });

        assert_eq!(options.format, LiveFrameFormat::Png);
        assert_eq!(options.quality, 1);
        assert_eq!(options.max_width, 320);
        assert_eq!(options.max_height, 2160);
        assert_eq!(options.every_nth_frame, 1);
        assert_eq!(options.timeout_ms, 100);
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

    /// Integration test that requires a local Chromium/Chrome binary.
    /// Skipped unless `CHROMIUMOXIDE_TEST=1` is set.
    #[tokio::test]
    async fn captures_live_frame_after_navigation() {
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
                "data:text/html,<html><body style='background:%230056cc'><h1>frame</h1></body></html>",
            )
            .await
            .expect("goto");

        let frame = driver
            .live_frame(
                &session,
                LiveFrameOptions {
                    format: LiveFrameFormat::Jpeg,
                    quality: 60,
                    max_width: 640,
                    max_height: 480,
                    every_nth_frame: 1,
                    timeout_ms: 2_000,
                },
            )
            .await
            .expect("live frame");

        assert_eq!(frame.mime_type, "image/jpeg");
        assert!(frame.data_base64.len() > 100);

        driver.release(session).await.expect("release");
    }

    /// Regression test for the "Blank page can not have cookie" bug: opening a
    /// session/tab against a profile that already has saved cookies used to
    /// hard-fail because cookie hydration ran on the freshly-created
    /// `about:blank` page, before the first real navigation — chromiumoxide's
    /// `Page::set_cookies` requires the page to already be on a real http(s)
    /// URL. Integration test that requires a local Chromium/Chrome binary.
    /// Skipped unless `CHROMIUMOXIDE_TEST=1` is set.
    #[tokio::test]
    async fn reopening_a_session_with_a_saved_profile_restores_cookies_without_erroring() {
        if std::env::var("CHROMIUMOXIDE_TEST").ok().as_deref() != Some("1") {
            eprintln!("skipping: set CHROMIUMOXIDE_TEST=1 to run");
            return;
        }

        let profile_store: Arc<dyn ProfileStore> =
            Arc::new(crate::session::InMemoryProfileStore::new());
        let mut lease = make_lease();
        lease.persist_profile = true;
        let profile_id = lease.profile_id.clone();

        // Pre-seed the profile store exactly as a prior session's close would
        // have: a snapshot with at least one real cookie for a real origin.
        let snapshot = SessionSnapshot {
            cookies: vec![Cookie {
                name: "velion_test".into(),
                value: "livecheck123".into(),
                domain: "example.com".into(),
                path: "/".into(),
                secure: false,
                http_only: false,
                expires: None,
            }],
            local_storage: vec![],
            session_storage: vec![],
            indexed_db: vec![],
            user_agent: None,
            viewport: None,
            locale: None,
            timezone: None,
        };
        profile_store
            .save(&lease.org_id, &profile_id, &snapshot)
            .await
            .expect("seed profile snapshot");

        let driver = ChromiumoxideDriver::new().with_profile_store(profile_store);
        let session = driver.acquire(&lease).await.expect("acquire");
        driver
            .goto(&session, "https://example.com/")
            .await
            .expect("goto should succeed with a persisted profile's cookies restored");

        driver.release(session).await.expect("release");
    }

    /// Regression test for the cross-session cookie leak: every session used
    /// to share one global Chromium browser context, so a cookie set by one
    /// session's very first tab was already visible to a brand-new, unrelated
    /// session's very first tab. Integration test that requires a local
    /// Chromium/Chrome binary. Skipped unless `CHROMIUMOXIDE_TEST=1` is set.
    #[tokio::test]
    async fn distinct_sessions_do_not_share_cookies() {
        if std::env::var("CHROMIUMOXIDE_TEST").ok().as_deref() != Some("1") {
            eprintln!("skipping: set CHROMIUMOXIDE_TEST=1 to run");
            return;
        }

        let driver = ChromiumoxideDriver::new();

        let lease_a = make_lease_with_key("isolation-session-a");
        let session_a = driver.acquire(&lease_a).await.expect("acquire a");
        driver
            .goto(&session_a, "https://example.com/")
            .await
            .expect("goto a");
        driver
            .evaluate(&session_a, "document.cookie = 'leak_test=from_a; path=/'")
            .await
            .expect("set cookie on a");

        let lease_b = make_lease_with_key("isolation-session-b");
        let session_b = driver.acquire(&lease_b).await.expect("acquire b");
        driver
            .goto(&session_b, "https://example.com/")
            .await
            .expect("goto b");
        let cookie_b = driver
            .evaluate(&session_b, "document.cookie")
            .await
            .expect("read cookie on b");
        let cookie_b = cookie_b.as_str().unwrap_or_default();
        assert!(
            !cookie_b.contains("leak_test"),
            "session b must not see session a's cookie, saw: {cookie_b:?}"
        );

        // Releasing session a's context must not disturb session b, which is
        // still concurrently live.
        driver.release(session_a).await.expect("release a");
        let cookie_b_after = driver
            .evaluate(&session_b, "document.cookie")
            .await
            .expect("read cookie on b after a released");
        let cookie_b_after = cookie_b_after.as_str().unwrap_or_default();
        assert!(
            !cookie_b_after.contains("leak_test"),
            "session b must remain unaffected after session a's context is disposed"
        );

        driver.release(session_b).await.expect("release b");
    }

    /// A second tab opened on an EXISTING session must land in that same
    /// session's own isolated context, not a fresh one — otherwise per-tab
    /// state within one logical session would incorrectly fragment.
    /// Integration test that requires a local Chromium/Chrome binary. Skipped
    /// unless `CHROMIUMOXIDE_TEST=1` is set.
    #[tokio::test]
    async fn second_tab_on_same_session_shares_that_sessions_cookies() {
        if std::env::var("CHROMIUMOXIDE_TEST").ok().as_deref() != Some("1") {
            eprintln!("skipping: set CHROMIUMOXIDE_TEST=1 to run");
            return;
        }

        let driver = ChromiumoxideDriver::new();
        let lease = make_lease_with_key("same-session-multi-tab");
        let session = driver.acquire(&lease).await.expect("acquire");

        driver
            .goto(&session, "https://example.com/")
            .await
            .expect("goto tab 1");
        driver
            .evaluate(&session, "document.cookie = 'shared_tab_test=abc; path=/'")
            .await
            .expect("set cookie on tab 1");

        // Opens a second tab on the SAME session/lease; new_tab makes it active.
        driver
            .new_tab(&session, Some("https://example.com/"))
            .await
            .expect("new_tab");
        let cookie_on_tab2 = driver
            .evaluate(&session, "document.cookie")
            .await
            .expect("read cookie on tab 2");
        let cookie_on_tab2 = cookie_on_tab2.as_str().unwrap_or_default();
        assert!(
            cookie_on_tab2.contains("shared_tab_test"),
            "second tab on the same session must see that session's own cookie, saw: {cookie_on_tab2:?}"
        );

        driver.release(session).await.expect("release");
    }
}
