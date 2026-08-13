//! Chromiumoxide CDP driver.
//!
//! Implements [`BrowserDriver`] using the [`chromiumoxide`] crate, which
//! drives a local Chromium/Chrome process over the Chrome DevTools Protocol.
//!
//! Feature-gated behind `chromiumoxide`. The driver launches a single
//! browser process per `ChromiumoxideDriver` and reuses it across leases;
//! each lease owns a small tab set keyed by its session affinity key.

use ::chromiumoxide::cdp::browser_protocol::accessibility::{
    EnableParams as AccessibilityEnableParams, GetFullAxTreeParams,
};
use ::chromiumoxide::cdp::browser_protocol::browser::{
    BrowserContextId, CancelDownloadParams, DownloadProgressState, EventDownloadProgress,
    EventDownloadWillBegin, SetDownloadBehaviorBehavior, SetDownloadBehaviorParams,
};
use ::chromiumoxide::cdp::browser_protocol::dom::{
    BackendNodeId, ResolveNodeParams, SetFileInputFilesParams,
};
use ::chromiumoxide::cdp::browser_protocol::fetch::{
    ContinueRequestParams, EnableParams as FetchEnableParams, EventRequestPaused, FailRequestParams,
};
use ::chromiumoxide::cdp::browser_protocol::input::{
    DispatchMouseEventParams, DispatchMouseEventPointerType, DispatchMouseEventType, MouseButton,
};
use ::chromiumoxide::cdp::browser_protocol::log::EventEntryAdded;
use ::chromiumoxide::cdp::browser_protocol::network::{
    CookieParam, ErrorReason, EventRequestWillBeSent, EventResponseReceived,
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
    CaptureScreenshotFormat, CaptureScreenshotParamsBuilder, EventJavascriptDialogOpening,
    EventLifecycleEvent, EventScreencastFrame, FrameId, GetFrameTreeParams,
    HandleJavaScriptDialogParams, PrintToPdfParams, ScreencastFrameAckParams,
    StartScreencastFormat, StartScreencastParams, StopScreencastParams,
};
use ::chromiumoxide::cdp::js_protocol::runtime::{
    CallArgument, CallFunctionOnParams, EventConsoleApiCalled, ReleaseObjectParams,
};
use ::chromiumoxide::handler::viewport::Viewport as ChromiumViewport;
use ::chromiumoxide::{Browser, BrowserConfig, Page};
use futures::StreamExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use quarry_core::contracts::{
    AccessibilityNode, AccessibilityProjection, BrowserDialog, BrowserEgressDecision,
    BrowserEgressReceipt, BrowserFrame, BrowserStartupMode, BrowserTelemetry,
};
use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::lease::{BrowserLease, BrowserViewport};

use crate::actions::ScrollTarget;
use crate::navigation::{guard_navigation_target, guard_page_request_target};
use crate::session::{Cookie, ProfileStore, SessionSnapshot, Viewport};
use crate::{
    BrowserDevtoolsEvent, BrowserDownloadedFile, BrowserDriver, BrowserDriverCapabilities,
    BrowserEgressPolicy, BrowserEgressProxyProvider, BrowserNativeProjection, BrowserNativeTarget,
    BrowserSession, BrowserTab, LiveFrame, LiveFrameFormat, LiveFrameOptions, SessionInner,
    VerifiedTargetAction, VerifiedTargetOperation,
};

const LIVE_FRAME_CACHE_TTL_MS: u64 = 250;
const DEVTOOLS_EVENT_BUFFER_LIMIT: usize = 512;
const MAX_GOVERNED_DOWNLOAD_BYTES: u64 = 25 * 1024 * 1024;
const DOWNLOAD_BEGIN_TIMEOUT: Duration = Duration::from_secs(10);
const DOWNLOAD_COMPLETE_TIMEOUT: Duration = Duration::from_secs(30);

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
    /// Private download directory configured only for this isolated browser
    /// context. Dropping the session removes it after all tabs are closed.
    download_dir: Option<Arc<tempfile::TempDir>>,
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
    egress_receipts: Arc<Mutex<HashMap<String, Vec<BrowserEgressReceipt>>>>,
    /// Egress receipts are paged per browser session, so their sequence must
    /// be per-session too. A process-global sequence would make a fresh run
    /// appear to have skipped receipts created by an unrelated tenant.
    egress_sequence: Arc<Mutex<HashMap<String, u64>>>,
    egress_policies: Arc<Mutex<HashMap<String, BrowserEgressPolicy>>>,
    pinned_egress_proxy: Option<Arc<dyn BrowserEgressProxyProvider>>,
    proxy_receipt_cursors: Arc<Mutex<HashMap<String, u64>>>,
    /// Startup data is recorded per lease. Renderer metrics are sampled only
    /// when telemetry is requested, so no global process metric can bleed
    /// across tenant-owned browser sessions.
    session_telemetry: Arc<Mutex<HashMap<String, BrowserTelemetry>>>,
    dialogs: Arc<Mutex<HashMap<String, Vec<BrowserDialog>>>>,
    /// The public dialog id is an opaque Quarry receipt handle, while CDP
    /// responds to the target/page that owns the active dialog. Keep that
    /// binding private so responding after a tab switch cannot affect a
    /// dialog on the newly active tab.
    dialog_pages: Arc<Mutex<HashMap<String, HashMap<String, Page>>>>,
    dialog_sequence: Arc<Mutex<u64>>,
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
            egress_receipts: Arc::new(Mutex::new(HashMap::new())),
            egress_sequence: Arc::new(Mutex::new(HashMap::new())),
            egress_policies: Arc::new(Mutex::new(HashMap::new())),
            pinned_egress_proxy: None,
            proxy_receipt_cursors: Arc::new(Mutex::new(HashMap::new())),
            session_telemetry: Arc::new(Mutex::new(HashMap::new())),
            dialogs: Arc::new(Mutex::new(HashMap::new())),
            dialog_pages: Arc::new(Mutex::new(HashMap::new())),
            dialog_sequence: Arc::new(Mutex::new(0)),
            profiles: None,
        }
    }

    pub fn with_profile_store(mut self, store: Arc<dyn ProfileStore>) -> Self {
        self.profiles = Some(store);
        self
    }

    /// Route every Chromium HTTP(S) connection through Quarry's loopback
    /// DNS-pinning egress authority. Capability advertising remains
    /// fail-closed until the complete path is verified; this config merely
    /// prevents the browser from bypassing the transport boundary.
    pub fn with_pinned_egress_proxy(
        mut self,
        provider: Arc<dyn BrowserEgressProxyProvider>,
    ) -> Self {
        self.pinned_egress_proxy = Some(provider);
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
        self.egress_receipts.lock().await.clear();
        self.egress_sequence.lock().await.clear();
        self.egress_policies.lock().await.clear();
        self.proxy_receipt_cursors.lock().await.clear();
        self.session_telemetry.lock().await.clear();
        self.dialogs.lock().await.clear();
        self.dialog_pages.lock().await.clear();
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
        if self.pinned_egress_proxy.is_some() {
            // QUIC/HTTP3 can open sockets outside the explicit HTTP proxy
            // path. Per-session CDP contexts install the actual proxy below;
            // Chromium must use that CONNECT/HTTP boundary until a separately
            // pinned QUIC egress implementation exists.
            builder = builder.arg("--disable-quic");
        }
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

    /// Browser navigation is permitted only after both halves of Quarry's
    /// egress boundary are present: the DNS-pinning transport provider and a
    /// session-scoped host policy. Keeping this invariant in the driver
    /// prevents a new caller from accidentally treating CDP's URL guard as a
    /// sufficient network boundary.
    async fn require_configured_egress(&self, session: &BrowserSession) -> QuarryResult<()> {
        if self.pinned_egress_proxy.is_none() {
            return Err(QuarryError::unsupported_action(
                "chromium browser navigation requires a pinned egress proxy",
            ));
        }
        let session_key = Self::session_key(session);
        if self.egress_policies.lock().await.contains_key(&session_key) {
            Ok(())
        } else {
            Err(QuarryError::new(
                ErrorCode::SecurityBlocked,
                "chromium browser navigation requires an installed egress policy",
            ))
        }
    }

    /// Copy transport-authority receipts into the driver's one monotonic,
    /// session-scoped receipt stream. The provider's native sequence stays
    /// private; callers only see the driver's unified sequence together with
    /// the CDP Fetch decisions that caused Chromium to continue or abort.
    async fn synchronize_proxy_egress_receipts(&self, session_key: &str) -> QuarryResult<()> {
        let Some(provider) = &self.pinned_egress_proxy else {
            return Ok(());
        };
        // Serialize a session's cursor advancement. Holding this small mutex
        // through the provider read prevents two concurrent observers from
        // promoting the same transport receipt twice.
        let mut cursors = self.proxy_receipt_cursors.lock().await;
        let after_sequence = cursors.get(session_key).copied().unwrap_or(0);
        let transport_receipts = provider
            .receipts_after(session_key, after_sequence, DEVTOOLS_EVENT_BUFFER_LIMIT)
            .await?;
        if let Some(last_sequence) = transport_receipts
            .iter()
            .map(|receipt| receipt.sequence)
            .max()
        {
            cursors.insert(session_key.to_owned(), last_sequence);
        }
        drop(cursors);

        for receipt in transport_receipts {
            push_egress_receipt(
                &self.egress_receipts,
                &self.egress_sequence,
                session_key,
                BrowserEgressReceipt {
                    sequence: 0,
                    // The HTTP proxy observes a connection, not a CDP target;
                    // do not fabricate a tab attribution.
                    tab_id: None,
                    method: receipt.method,
                    url: receipt.url,
                    decision: receipt.decision,
                    policy: receipt.policy,
                    timestamp_ms: receipt.timestamp_ms,
                },
            )
            .await;
        }
        Ok(())
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
        let initial_url = url.unwrap_or("about:blank");
        guard_navigation_target(initial_url).await?;
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
                let context_params = if let Some(provider) = &self.pinned_egress_proxy {
                    let endpoint = provider.endpoint_for_session(&session_key).await?;
                    CreateBrowserContextParams::builder()
                        .proxy_server(endpoint.as_str())
                        // Override Chromium's implicit loopback bypass. The
                        // Fetch policy still denies loopback destinations;
                        // this only stops a transport-level proxy bypass.
                        .proxy_bypass_list("<-loopback>")
                        .build()
                } else {
                    CreateBrowserContextParams::default()
                };
                let id = browser
                    .create_browser_context(context_params)
                    .await
                    .map_err(|e| {
                        QuarryError::new(
                            ErrorCode::DriverFailed,
                            "chromiumoxide create_browser_context failed",
                        )
                        .with_details(json!({ "error": e.to_string() }))
                    })?;
                let download_dir = Arc::new(
                    tempfile::Builder::new()
                        .prefix("quarry-download-")
                        .tempdir()
                        .map_err(|error| {
                            QuarryError::new(
                                ErrorCode::DriverFailed,
                                "chromiumoxide download quarantine directory creation failed",
                            )
                            .with_details(json!({ "error": error.to_string() }))
                        })?,
                );
                let download_path = download_dir.path().to_str().ok_or_else(|| {
                    QuarryError::new(
                        ErrorCode::Internal,
                        "chromiumoxide download quarantine path is not valid UTF-8",
                    )
                })?;
                let download_behavior = SetDownloadBehaviorParams::builder()
                    .behavior(SetDownloadBehaviorBehavior::AllowAndName)
                    .browser_context_id(id.clone())
                    .download_path(download_path)
                    .events_enabled(true)
                    .build()
                    .map_err(|error| {
                        QuarryError::new(
                            ErrorCode::Internal,
                            "build chromiumoxide download quarantine configuration failed",
                        )
                        .with_details(json!({ "error": error }))
                    })?;
                browser.execute(download_behavior).await.map_err(|error| {
                    QuarryError::new(
                        ErrorCode::DriverFailed,
                        "chromiumoxide download quarantine configuration failed",
                    )
                    .with_details(json!({ "error": error.to_string() }))
                })?;
                let mut pages = self.pages.lock().await;
                let session_pages = pages.entry(session_key.clone()).or_default();
                session_pages.browser_context_id = Some(id.clone());
                session_pages.download_dir = Some(download_dir);
                id
            }
        };

        let snapshot = self.load_snapshot(&session.lease).await;
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
        install_dialog_collector(
            &page,
            session_key.clone(),
            self.dialogs.clone(),
            self.dialog_pages.clone(),
            self.dialog_sequence.clone(),
        )
        .await;
        install_network_guard(
            &page,
            session_key.clone(),
            tab_id.clone(),
            self.egress_receipts.clone(),
            self.egress_sequence.clone(),
            self.egress_policies.clone(),
        )
        .await?;
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

/// Install a fail-closed CDP Fetch listener before a page can navigate.
///
/// The direct `goto`/`new_tab` guard protects only caller-provided URLs. This
/// listener covers redirects and requests initiated by the page itself (such
/// as fetch/XHR, iframes, images, and stylesheet resources) before Chromium
/// is permitted to open the connection.
async fn install_network_guard(
    page: &Page,
    session_key: String,
    tab_id: String,
    receipts: Arc<Mutex<HashMap<String, Vec<BrowserEgressReceipt>>>>,
    sequence: Arc<Mutex<HashMap<String, u64>>>,
    policies: Arc<Mutex<HashMap<String, BrowserEgressPolicy>>>,
) -> QuarryResult<()> {
    let mut events = page
        .event_listener::<EventRequestPaused>()
        .await
        .map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                "chromiumoxide request guard listener failed",
            )
            .with_details(json!({ "error": e.to_string() }))
        })?;
    // Register the listener before turning interception on. Fetch.enable
    // pauses every request by default; without this command the listener
    // below is inert and redirects, frames, XHR, and subresources can leave
    // the browser without the policy having a chance to decide them.
    page.execute(
        FetchEnableParams::builder()
            .handle_auth_requests(true)
            .build(),
    )
    .await
    .map_err(|error| {
        QuarryError::new(
            ErrorCode::DriverFailed,
            "chromiumoxide enable browser network guard failed",
        )
        .with_details(json!({ "error": error.to_string() }))
    })?;
    let guarded_page = page.clone();
    tokio::spawn(async move {
        while let Some(event) = events.next().await {
            let request_id = event.request_id.clone();
            let request_url = event.request.url.clone();
            let method = event.request.method.clone();
            let policy_allows_url = policies
                .lock()
                .await
                .get(&session_key)
                .cloned()
                // A page can start emitting Fetch events as soon as its CDP
                // target is attached. Missing session policy must be deny-all
                // rather than the legacy unconstrained default.
                .unwrap_or_else(BrowserEgressPolicy::deny_all)
                .allows_url(&request_url);
            let (decision, policy, command_result) = if !policy_allows_url {
                (
                    BrowserEgressDecision::Block,
                    "domain_grant_blocked".to_owned(),
                    guarded_page
                        .execute(FailRequestParams::new(request_id, ErrorReason::Aborted))
                        .await
                        .map(|_| ())
                        .map_err(|failure| failure.to_string()),
                )
            } else {
                match guard_page_request_target(&request_url).await {
                    Ok(()) => (
                        BrowserEgressDecision::Allow,
                        "url_dns_public".to_owned(),
                        guarded_page
                            .execute(ContinueRequestParams::new(request_id))
                            .await
                            .map(|_| ())
                            .map_err(|error| error.to_string()),
                    ),
                    Err(error) => {
                        tracing::warn!(
                            code = ?error.code,
                            "chromiumoxide blocked unsafe page request"
                        );
                        (
                            BrowserEgressDecision::Block,
                            "url_or_dns_blocked".to_owned(),
                            guarded_page
                                .execute(FailRequestParams::new(request_id, ErrorReason::Aborted))
                                .await
                                .map(|_| ())
                                .map_err(|failure| failure.to_string()),
                        )
                    }
                }
            };
            push_egress_receipt(
                &receipts,
                &sequence,
                &session_key,
                BrowserEgressReceipt {
                    sequence: 0,
                    tab_id: Some(tab_id.clone()),
                    method,
                    url: redact_egress_url(&request_url),
                    decision,
                    policy,
                    timestamp_ms: now_ms(),
                },
            )
            .await;
            if let Err(error) = command_result {
                tracing::warn!(error = %error, "chromiumoxide request guard resolution failed");
            }
        }
    });
    Ok(())
}

/// Register dialog openings as observable state. The listener deliberately
/// does not respond to the dialog: accepting an alert/confirm/prompt can
/// change page state, so that authority remains with a later grant-bound
/// `respond_dialog` action.
async fn install_dialog_collector(
    page: &Page,
    session_key: String,
    dialogs: Arc<Mutex<HashMap<String, Vec<BrowserDialog>>>>,
    dialog_pages: Arc<Mutex<HashMap<String, HashMap<String, Page>>>>,
    sequence: Arc<Mutex<u64>>,
) {
    let Ok(mut events) = page.event_listener::<EventJavascriptDialogOpening>().await else {
        tracing::debug!("javascript dialog listener unavailable");
        return;
    };
    let dialog_page = page.clone();
    tokio::spawn(async move {
        while let Some(event) = events.next().await {
            let dialog_id = {
                let mut sequence = sequence.lock().await;
                *sequence = sequence.saturating_add(1);
                format!("dlg_{}", *sequence)
            };
            let removed = {
                let mut dialogs = dialogs.lock().await;
                let active = dialogs.entry(session_key.clone()).or_default();
                active.push(BrowserDialog {
                    dialog_id: dialog_id.clone(),
                    frame_id: Some(event.frame_id.inner().clone()),
                    kind: event.r#type.as_ref().to_ascii_lowercase(),
                    message: truncate_devtools_text(&event.message),
                    default_prompt: event.default_prompt.as_ref().map(truncate_devtools_text),
                    origin: url::Url::parse(&event.url)
                        .ok()
                        .map(|url| url.origin().ascii_serialization()),
                    opened_at_ms: now_ms(),
                });
                // Chromium has at most one active JavaScript dialog per target;
                // retaining a tiny bound is defence-in-depth if a provider emits
                // repeated events while a page is being torn down.
                if active.len() > 4 {
                    active
                        .drain(0..active.len() - 4)
                        .map(|dialog| dialog.dialog_id)
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                }
            };
            let mut pages = dialog_pages.lock().await;
            let session_pages = pages.entry(session_key.clone()).or_default();
            for removed_id in removed {
                session_pages.remove(&removed_id);
            }
            session_pages.insert(dialog_id, dialog_page.clone());
        }
    });
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

async fn push_egress_receipt(
    buffers: &Arc<Mutex<HashMap<String, Vec<BrowserEgressReceipt>>>>,
    sequence: &Arc<Mutex<HashMap<String, u64>>>,
    session_key: &str,
    mut receipt: BrowserEgressReceipt,
) {
    let mut sequences = sequence.lock().await;
    let sequence = sequences.entry(session_key.to_owned()).or_default();
    *sequence = sequence.saturating_add(1);
    receipt.sequence = *sequence;
    drop(sequences);

    let mut buffers = buffers.lock().await;
    let receipts = buffers.entry(session_key.to_owned()).or_default();
    receipts.push(receipt);
    if receipts.len() > DEVTOOLS_EVENT_BUFFER_LIMIT {
        let excess = receipts.len() - DEVTOOLS_EVENT_BUFFER_LIMIT;
        receipts.drain(0..excess);
    }
}

/// Retain only an origin and path for the auditable policy receipt. A request
/// URL can carry credentials or other page-controlled secrets in its query;
/// neither belongs in a browser timeline, observation, or durable receipt.
fn redact_egress_url(raw: &str) -> String {
    url::Url::parse(raw)
        .ok()
        .and_then(|url| {
            let host = url.host_str()?;
            let port = url
                .port()
                .map(|port| format!(":{port}"))
                .unwrap_or_default();
            Some(format!("{}://{}{}{}", url.scheme(), host, port, url.path()))
        })
        .unwrap_or_else(|| "unparseable_url".to_owned())
}

/// Keep browser-controlled filename metadata harmless and bounded before it
/// becomes part of an artifact receipt. The saved file itself is GUID-named by
/// CDP; this is display metadata only and never a filesystem target.
fn sanitize_download_filename(raw: &str) -> String {
    let filename = raw
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|character| !character.is_control())
        .take(128)
        .collect::<String>();
    if filename.is_empty() || filename == "." || filename == ".." {
        "download.bin".to_owned()
    } else {
        filename
    }
}

const AX_PROJECTION_NODE_LIMIT: usize = 300;
const FRAME_PROJECTION_LIMIT: usize = 64;

fn native_projection_from_cdp(
    ax_tree: serde_json::Value,
    frame_tree: serde_json::Value,
) -> BrowserNativeProjection {
    let raw_nodes = ax_tree
        .get("nodes")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let truncated = raw_nodes.len() > AX_PROJECTION_NODE_LIMIT;
    let nodes = raw_nodes
        .into_iter()
        .take(AX_PROJECTION_NODE_LIMIT)
        .filter_map(|node| {
            let node_id = node.get("nodeId")?.as_str()?.to_owned();
            Some(AccessibilityNode {
                node_id,
                role: ax_value_text(node.get("role")),
                name: ax_value_text(node.get("name")),
                value: ax_value_text(node.get("value")),
                ignored: node
                    .get("ignored")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                frame_id: node
                    .get("frameId")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned),
                child_ids: node
                    .get("childIds")
                    .and_then(serde_json::Value::as_array)
                    .map(|children| {
                        children
                            .iter()
                            .filter_map(serde_json::Value::as_str)
                            .take(100)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default(),
            })
        })
        .collect::<Vec<_>>();
    let mut frames = Vec::new();
    collect_frame_projection(
        frame_tree
            .get("frameTree")
            .unwrap_or(&serde_json::Value::Null),
        &mut frames,
    );

    let targets = raw_nodes_to_native_targets(&ax_tree);

    BrowserNativeProjection {
        accessibility: Some(AccessibilityProjection {
            source: "chromium_cdp_ax".to_owned(),
            nodes,
            truncated,
        }),
        frames,
        targets,
    }
}

/// Produce executable bindings for unignored AX nodes in every observed frame.
/// CDP backend node ids are never exposed on the public observation; they stay
/// inside Quarry until the matching snapshot action executes. Runtime requires
/// the dedicated `frame_*_ref` contract for a child-frame binding, so observing
/// a cross-origin iframe never grants a bare top-level ref authority.
fn raw_nodes_to_native_targets(ax_tree: &serde_json::Value) -> Vec<BrowserNativeTarget> {
    ax_tree
        .get("nodes")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|node| {
            !node
                .get("ignored")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        })
        .filter_map(|node| {
            let ax_node_id = node.get("nodeId")?.as_str()?.to_owned();
            let backend_node_id = node.get("backendDOMNodeId")?.as_i64()?;
            let frame_id = node
                .get("frameId")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            let role = ax_value_text(node.get("role"));
            let name = ax_value_text(node.get("name"));
            let value = ax_value_text(node.get("value"));
            // The full AX tree is retained for observation, but executable
            // refs are deliberately restricted to native interactive roles.
            // Static text, document containers, and generic groups must not
            // become click targets merely because they have a name.
            if !is_actionable_ax_role(role.as_deref()) {
                return None;
            }
            Some(BrowserNativeTarget {
                ax_node_id,
                backend_node_id,
                frame_id,
                role,
                name,
                value,
            })
        })
        .take(AX_PROJECTION_NODE_LIMIT)
        .collect()
}

fn is_actionable_ax_role(role: Option<&str>) -> bool {
    matches!(
        role.map(|role| role.to_ascii_lowercase()),
        Some(role)
            if matches!(
                role.as_str(),
                "button"
                    | "link"
                    | "textbox"
                    | "searchbox"
                    | "combobox"
                    | "listbox"
                    | "option"
                    | "checkbox"
                    | "radio"
                    | "switch"
                    | "slider"
                    | "spinbutton"
                    | "tab"
                    | "menuitem"
                    | "menuitemcheckbox"
                    | "menuitemradio"
                    | "treeitem"
            )
    )
}

/// Resolve the exact CDP backend node produced by an AX observation and run
/// the effect against that object. Unlike a selector, this cannot drift to a
/// later matching element between resolution and the effect. The remote
/// object is always released before returning to avoid retaining page nodes
/// across an agent session.
async fn execute_native_target_action(
    page: &Page,
    target: BrowserNativeTarget,
    operation: VerifiedTargetOperation,
) -> QuarryResult<()> {
    // `DOM.resolveNode` must execute in the target frame's current realm. In
    // particular, an out-of-process child iframe cannot be safely assumed to
    // share the top-level page's execution context. The frame id itself came
    // from the just-validated native AX snapshot; this is not model input.
    let execution_context_id = match target.frame_id.as_deref() {
        Some(frame_id) => page
            .frame_execution_context(FrameId::new(frame_id))
            .await
            .map_err(|error| {
                QuarryError::new(
                    ErrorCode::TargetRepairRequired,
                    "observed frame is no longer live; observe again",
                )
                .with_details(json!({ "frame_id": frame_id, "error": error.to_string() }))
            })?
            .ok_or_else(|| {
                QuarryError::new(
                    ErrorCode::TargetRepairRequired,
                    "observed frame has no active execution context; observe again",
                )
                .with_details(json!({ "frame_id": frame_id }))
            })
            .map(Some)?,
        None => None,
    };
    let mut resolve =
        ResolveNodeParams::builder().backend_node_id(BackendNodeId::new(target.backend_node_id));
    if let Some(execution_context_id) = execution_context_id {
        resolve = resolve.execution_context_id(execution_context_id);
    }
    let resolved = page.execute(resolve.build()).await.map_err(|error| {
        QuarryError::new(
            ErrorCode::TargetRepairRequired,
            "native accessibility target is no longer live; observe again",
        )
        .with_details(json!({ "ax_node_id": target.ax_node_id, "error": error.to_string() }))
    })?;
    let object_id = resolved.object.object_id.clone().ok_or_else(|| {
        QuarryError::new(
            ErrorCode::TargetRepairRequired,
            "native accessibility target could not be resolved; observe again",
        )
        .with_details(json!({ "ax_node_id": target.ax_node_id }))
    })?;
    let operation_value = serde_json::to_value(&operation).map_err(|error| {
        QuarryError::new(
            ErrorCode::Internal,
            "serialize native target operation failed",
        )
        .with_details(json!({ "error": error.to_string() }))
    })?;
    let call = CallFunctionOnParams::builder()
        .function_declaration(
            r#"function(request) {
                const element = this;
                if (!(element instanceof Element) || !element.isConnected) {
                    return { ok: false, reason: 'native_target_not_connected' };
                }
                switch (request.type) {
                    case 'click':
                        element.click();
                        return { ok: true };
                    case 'type': {
                        if (!('value' in element)) return { ok: false, reason: 'target_not_typeable' };
                        element.focus();
                        const proto = element instanceof HTMLTextAreaElement
                            ? HTMLTextAreaElement.prototype
                            : HTMLInputElement.prototype;
                        const setter = Object.getOwnPropertyDescriptor(proto, 'value')?.set;
                        if (!setter) return { ok: false, reason: 'value_setter_unavailable' };
                        setter.call(element, request.text);
                        element.dispatchEvent(new InputEvent('input', {
                            bubbles: true, inputType: 'insertText', data: request.text
                        }));
                        element.dispatchEvent(new Event('change', { bubbles: true }));
                        return { ok: true };
                    }
                    case 'select': {
                        if (!(element instanceof HTMLSelectElement)) {
                            return { ok: false, reason: 'target_not_select' };
                        }
                        element.value = request.value;
                        if (element.value !== request.value) {
                            return { ok: false, reason: 'select_value_unavailable' };
                        }
                        element.dispatchEvent(new Event('input', { bubbles: true }));
                        element.dispatchEvent(new Event('change', { bubbles: true }));
                        return { ok: true };
                    }
                    case 'wait_for':
                        // This ref was observed before the action began. Its
                        // exact backend node is still connected, so a native
                        // wait succeeds without re-querying a selector.
                        return { ok: true };
                    default:
                        return { ok: false, reason: 'unsupported_native_operation' };
                }
            }"#,
        )
        .object_id(object_id.clone())
        .argument(CallArgument::builder().value(operation_value).build())
        .return_by_value(true)
        .user_gesture(true)
        .build()
        .map_err(|error| {
            QuarryError::new(ErrorCode::Internal, "build native target action failed")
                .with_details(json!({ "error": error }))
        })?;
    let result = page.execute(call).await;
    // A failed release is not a reason to hide the action result, but the
    // short-lived handle must never be retained for a later action.
    let _ = page.execute(ReleaseObjectParams::new(object_id)).await;
    let result = result.map_err(|error| {
        QuarryError::new(ErrorCode::DriverFailed, "native target action failed")
            .with_details(json!({ "ax_node_id": target.ax_node_id, "error": error.to_string() }))
    })?;
    if result.exception_details.is_some() {
        return Err(QuarryError::new(
            ErrorCode::TargetRepairRequired,
            "native target action raised a page exception; observe again",
        )
        .with_details(json!({ "ax_node_id": target.ax_node_id })));
    }
    let value = result
        .result
        .result
        .value
        .unwrap_or(serde_json::Value::Null);
    if value.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
        Ok(())
    } else {
        Err(QuarryError::new(
            ErrorCode::TargetRepairRequired,
            "native accessibility target changed or rejected the action",
        )
        .with_details(value))
    }
}

fn ax_value_text(value: Option<&serde_json::Value>) -> Option<String> {
    let value = value?.get("value")?;
    match value {
        serde_json::Value::String(value) if !value.is_empty() => {
            Some(truncate_devtools_text(value))
        }
        serde_json::Value::Number(value) => Some(value.to_string()),
        serde_json::Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn collect_frame_projection(value: &serde_json::Value, output: &mut Vec<BrowserFrame>) {
    if output.len() >= FRAME_PROJECTION_LIMIT {
        return;
    }
    let Some(frame) = value.get("frame") else {
        return;
    };
    let Some(frame_id) = frame.get("id").and_then(serde_json::Value::as_str) else {
        return;
    };
    let child_frame_ids = value
        .get("childFrames")
        .and_then(serde_json::Value::as_array)
        .map(|children| {
            children
                .iter()
                .filter_map(|child| {
                    child
                        .get("frame")
                        .and_then(|frame| frame.get("id"))
                        .and_then(serde_json::Value::as_str)
                })
                .take(100)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    output.push(BrowserFrame {
        frame_id: frame_id.to_owned(),
        parent_frame_id: frame
            .get("parentId")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        origin: frame
            .get("securityOrigin")
            .and_then(serde_json::Value::as_str)
            .filter(|origin| !origin.is_empty())
            .map(str::to_owned),
        name: frame
            .get("name")
            .and_then(serde_json::Value::as_str)
            .filter(|name| !name.is_empty())
            .map(truncate_devtools_text),
        child_frame_ids,
    });
    if let Some(children) = value
        .get("childFrames")
        .and_then(serde_json::Value::as_array)
    {
        for child in children {
            collect_frame_projection(child, output);
            if output.len() >= FRAME_PROJECTION_LIMIT {
                break;
            }
        }
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
    let raw_payload = serde_json::to_value(event).unwrap_or(serde_json::Value::Null);
    let request = raw_payload
        .get("request")
        .unwrap_or(&serde_json::Value::Null);
    let method = request
        .get("method")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let url = request
        .get("url")
        .and_then(serde_json::Value::as_str)
        .map(redact_egress_url);
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
        method: method.clone(),
        url: url.clone(),
        status: None,
        text: text.map(truncate_devtools_text),
        timestamp_ms: now_ms(),
        // Do not forward DevTools' raw request payload: it can contain query
        // secrets, headers, cookies, and POST bodies. Observability needs only
        // the redacted request identity and method.
        payload: json!({ "request": { "method": method.clone(), "url": url.clone() } }),
    }
}

fn normalize_response_event(tab_id: &str, event: &EventResponseReceived) -> BrowserDevtoolsEvent {
    let raw_payload = serde_json::to_value(event).unwrap_or(serde_json::Value::Null);
    let response = raw_payload
        .get("response")
        .unwrap_or(&serde_json::Value::Null);
    let url = response
        .get("url")
        .and_then(serde_json::Value::as_str)
        .map(redact_egress_url);
    let status = response
        .get("status")
        .and_then(serde_json::Value::as_i64)
        .and_then(|status| u16::try_from(status).ok());
    let mime_type = response
        .get("mimeType")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
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
        url: url.clone(),
        status,
        text: text.map(truncate_devtools_text),
        timestamp_ms: now_ms(),
        // The full response object includes request/response headers and
        // timing internals. Keep only the fields required for a compact
        // network summary and redact the URL first.
        payload: json!({ "response": {
            "url": url.clone(),
            "status": status,
            "mimeType": mime_type.clone(),
        } }),
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

fn truncate_devtools_text(text: impl AsRef<str>) -> String {
    const MAX: usize = 1_000;
    let text = text.as_ref();
    if text.len() <= MAX {
        return text.to_owned();
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

fn elapsed_ms(started_at: Instant) -> u64 {
    started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

/// CDP performance values are floating-point counters. Reject non-finite or
/// negative values rather than wrapping/saturating a malformed provider value
/// into an implausible resource claim.
fn finite_metric_u64(value: f64, scale: f64) -> Option<u64> {
    let scaled = value * scale;
    (scaled.is_finite() && scaled >= 0.0 && scaled <= u64::MAX as f64)
        .then_some(scaled.round() as u64)
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
    fn capabilities(&self) -> BrowserDriverCapabilities {
        BrowserDriverCapabilities {
            persistent_profile: true,
            devtools_trace: true,
            downloads_to_artifacts: false,
            // Native upload code exists, but it stays unavailable to agent
            // runs until the artifact quarantine and OOPIF path have runtime
            // proof equal to the download path.
            uploads_from_artifacts: false,
            full_visual_fidelity: true,
            // The Chromium-enabled runtime proof exercises the mandatory
            // per-session proxy plus CDP Fetch boundary for redirects,
            // frames, XHR/fetch, subresources, and script navigation. Remote
            // providers keep their independent fail-closed defaults.
            isolated_egress: self.pinned_egress_proxy.is_some(),
            security_evidence: self.pinned_egress_proxy.is_some(),
            atomic_target_actions: true,
        }
    }

    async fn configure_egress_policy(
        &self,
        session: &BrowserSession,
        policy: BrowserEgressPolicy,
    ) -> QuarryResult<()> {
        let session_key = Self::session_key(session);
        let provider = self.pinned_egress_proxy.as_ref().ok_or_else(|| {
            QuarryError::unsupported_action(
                "configure_egress_policy requires a pinned egress proxy",
            )
        })?;
        provider
            .configure_policy(&session_key, policy.clone())
            .await?;
        self.egress_policies
            .lock()
            .await
            .insert(session_key, policy);
        Ok(())
    }

    async fn acquire(&self, lease: &BrowserLease) -> QuarryResult<BrowserSession> {
        let acquired_at = Instant::now();
        let startup_mode = {
            let browser = self.browser.lock().await;
            let active_viewport = self.active_viewport.lock().await;
            if browser.is_some() && *active_viewport == lease.viewport {
                BrowserStartupMode::Warm
            } else {
                BrowserStartupMode::Cold
            }
        };
        self.ensure_browser(lease.viewport).await?;
        let session = BrowserSession {
            lease: lease.clone(),
            inner: Arc::new(Mutex::new(SessionInner {
                connected: true,
                pages_served: 0,
            })),
        };
        self.session_telemetry.lock().await.insert(
            Self::session_key(&session),
            BrowserTelemetry {
                startup_mode,
                startup_latency_ms: Some(elapsed_ms(acquired_at)),
                ..BrowserTelemetry::default()
            },
        );
        Ok(session)
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
        self.egress_receipts.lock().await.remove(&session_key);
        self.egress_sequence.lock().await.remove(&session_key);
        self.egress_policies.lock().await.remove(&session_key);
        self.proxy_receipt_cursors.lock().await.remove(&session_key);
        self.session_telemetry.lock().await.remove(&session_key);
        if let Some(provider) = &self.pinned_egress_proxy {
            provider.release_session(&session_key).await;
        }
        self.dialogs.lock().await.remove(&session_key);
        self.dialog_pages.lock().await.remove(&session_key);
        self.invalidate_live_frame_cache().await;
        Ok(())
    }

    async fn goto(&self, session: &BrowserSession, url: &str) -> QuarryResult<()> {
        guard_navigation_target(url).await?;
        self.require_configured_egress(session).await?;
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

    async fn act_on_verified_target(
        &self,
        session: &BrowserSession,
        action: VerifiedTargetAction,
    ) -> QuarryResult<()> {
        let page = self.current_page(session).await?;
        if let Some(native_target) = action.native_target {
            return execute_native_target_action(&page, native_target, action.operation).await;
        }
        let action_json = serde_json::to_string(&action).map_err(|error| {
            QuarryError::new(
                ErrorCode::Internal,
                format!("serialize verified target action: {error}"),
            )
        })?;
        let script = format!(
            r#"(() => {{
                const request = {action_json};
                const normalized = (value) => (value || '').replace(/\s+/g, ' ').trim().toLowerCase();
                const matches = Array.from(document.querySelectorAll(request.selector)).filter((element) =>
                    element.tagName.toLowerCase() === request.tag &&
                    request.attributes.every(([name, value]) => element.getAttribute(name) === value) &&
                    normalized(element.textContent) === request.normalized_text
                );
                if (matches.length !== 1) return {{ ok: false, reason: 'target_not_unique_or_changed', match_count: matches.length }};
                const element = matches[0];
                switch (request.operation.type) {{
                    case 'click': element.click(); break;
                    case 'type': {{
                        if (!('value' in element)) return {{ ok: false, reason: 'target_not_typeable' }};
                        element.focus();
                        const proto = element instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
                        const setter = Object.getOwnPropertyDescriptor(proto, 'value')?.set;
                        if (!setter) return {{ ok: false, reason: 'value_setter_unavailable' }};
                        setter.call(element, request.operation.text);
                        element.dispatchEvent(new InputEvent('input', {{ bubbles: true, inputType: 'insertText', data: request.operation.text }}));
                        element.dispatchEvent(new Event('change', {{ bubbles: true }}));
                        break;
                    }}
                    case 'select': {{
                        if (!(element instanceof HTMLSelectElement)) return {{ ok: false, reason: 'target_not_select' }};
                        element.value = request.operation.value;
                        if (element.value !== request.operation.value) return {{ ok: false, reason: 'select_value_unavailable' }};
                        element.dispatchEvent(new Event('input', {{ bubbles: true }}));
                        element.dispatchEvent(new Event('change', {{ bubbles: true }}));
                        break;
                    }}
                    case 'wait_for':
                        // A snapshot ref names an already-observed element.
                        // Reaching this point means it remains uniquely bound
                        // and connected, which is the only safe interpretation
                        // of wait-for without degrading to a fresh selector.
                        break;
                    default: return {{ ok: false, reason: 'unsupported_verified_operation' }};
                }}
                return {{ ok: true }};
            }})()"#,
        );
        let result = page.evaluate(script).await.map_err(|error| {
            QuarryError::new(ErrorCode::DriverFailed, "verified target action failed")
                .with_details(json!({ "error": error.to_string() }))
        })?;
        let value = result
            .into_value::<serde_json::Value>()
            .unwrap_or(serde_json::Value::Null);
        if value.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
            Ok(())
        } else {
            Err(QuarryError::new(
                ErrorCode::TargetRepairRequired,
                "verified target changed or is no longer unique",
            )
            .with_details(value))
        }
    }

    async fn upload_staged_file_to_target(
        &self,
        session: &BrowserSession,
        target: BrowserNativeTarget,
        staged_file: &Path,
    ) -> QuarryResult<()> {
        let path = staged_file.to_str().ok_or_else(|| {
            QuarryError::new(
                ErrorCode::Internal,
                "Quarry staging path is not valid UTF-8 for CDP upload",
            )
        })?;
        let page = self.current_page(session).await?;
        // Resolve and inspect the exact backend node first. This is not a
        // selector lookup: a stale node fails rather than being rebound to a
        // later matching input. Child frames require their own current realm;
        // using the top-level realm here would make an iframe upload depend on
        // CDP implementation details instead of the explicit frame contract.
        let execution_context_id = match target.frame_id.as_deref() {
            Some(frame_id) => page
                .frame_execution_context(FrameId::new(frame_id))
                .await
                .map_err(|error| {
                    QuarryError::new(
                        ErrorCode::TargetRepairRequired,
                        "upload target frame is no longer live; observe again",
                    )
                    .with_details(json!({ "frame_id": frame_id, "error": error.to_string() }))
                })?
                .ok_or_else(|| {
                    QuarryError::new(
                        ErrorCode::TargetRepairRequired,
                        "upload target frame has no active execution context; observe again",
                    )
                    .with_details(json!({ "frame_id": frame_id }))
                })
                .map(Some)?,
            None => None,
        };
        let mut resolve = ResolveNodeParams::builder()
            .backend_node_id(BackendNodeId::new(target.backend_node_id));
        if let Some(execution_context_id) = execution_context_id {
            resolve = resolve.execution_context_id(execution_context_id);
        }
        let resolved = page.execute(resolve.build()).await.map_err(|error| {
            QuarryError::new(
                ErrorCode::TargetRepairRequired,
                "upload target is no longer live; observe again",
            )
            .with_details(json!({ "ax_node_id": target.ax_node_id, "error": error.to_string() }))
        })?;
        let object_id = resolved.object.object_id.clone().ok_or_else(|| {
            QuarryError::new(
                ErrorCode::TargetRepairRequired,
                "upload target could not be resolved; observe again",
            )
            .with_details(json!({ "ax_node_id": target.ax_node_id }))
        })?;
        let check = CallFunctionOnParams::builder()
            .function_declaration(
                "function() { return this instanceof HTMLInputElement && this.type === 'file' && this.isConnected; }",
            )
            .object_id(object_id.clone())
            .return_by_value(true)
            .build()
            .map_err(|error| {
                QuarryError::new(ErrorCode::Internal, "build upload target check failed")
                    .with_details(json!({ "error": error }))
            })?;
        let check_result = page.execute(check).await;
        let _ = page.execute(ReleaseObjectParams::new(object_id)).await;
        let check_result = check_result.map_err(|error| {
            QuarryError::new(ErrorCode::DriverFailed, "upload target check failed").with_details(
                json!({ "ax_node_id": target.ax_node_id, "error": error.to_string() }),
            )
        })?;
        if check_result.exception_details.is_some()
            || check_result
                .result
                .result
                .value
                .as_ref()
                .and_then(serde_json::Value::as_bool)
                != Some(true)
        {
            return Err(QuarryError::new(
                ErrorCode::TargetRepairRequired,
                "snapshot target is not a live file input; observe again",
            )
            .with_details(json!({ "ax_node_id": target.ax_node_id })));
        }
        let set_files = SetFileInputFilesParams::builder()
            .file(path)
            .backend_node_id(BackendNodeId::new(target.backend_node_id))
            .build()
            .map_err(|error| {
                QuarryError::new(ErrorCode::Internal, "build governed upload command failed")
                    .with_details(json!({ "error": error }))
            })?;
        page.execute(set_files).await.map_err(|error| {
            QuarryError::new(
                ErrorCode::TargetRepairRequired,
                "upload target changed before file attachment; observe again",
            )
            .with_details(json!({ "ax_node_id": target.ax_node_id, "error": error.to_string() }))
        })?;
        Ok(())
    }

    async fn download_from_verified_target(
        &self,
        session: &BrowserSession,
        target: BrowserNativeTarget,
    ) -> QuarryResult<BrowserDownloadedFile> {
        let session_key = Self::session_key(session);
        let page = self.current_page(session).await?;
        // AX nodes normally carry their owner frame, but Chromium can omit it
        // for a top-level node. Never turn that omission into an "accept any
        // download in this browser context" rule: derive the active page's
        // root frame before installing the global Browser.download listeners.
        // This keeps a concurrent tab in the same lease from satisfying the
        // wrong action's download receipt.
        let expected_frame_id = match target.frame_id.clone() {
            Some(frame_id) => frame_id,
            None => root_frame_id(&page).await?,
        };
        let (context_id, download_dir) = {
            let pages = self.pages.lock().await;
            let session_pages = pages.get(&session_key).ok_or_else(|| {
                QuarryError::new(
                    ErrorCode::DriverFailed,
                    "download session context is unavailable",
                )
            })?;
            let context_id = session_pages.browser_context_id.clone().ok_or_else(|| {
                QuarryError::new(
                    ErrorCode::DriverFailed,
                    "download browser context is unavailable",
                )
            })?;
            let download_dir = session_pages.download_dir.clone().ok_or_else(|| {
                QuarryError::new(
                    ErrorCode::DriverFailed,
                    "download quarantine directory is unavailable",
                )
            })?;
            (context_id, download_dir)
        };
        // Subscribe before clicking. Browser.download* events are global to the
        // Chromium process, so the event is additionally tied to this exact
        // target's observed frame before its GUID is accepted.
        let (mut begins, mut progress) = {
            let browser_guard = self.browser.lock().await;
            let browser = browser_guard.as_ref().ok_or_else(|| {
                QuarryError::new(
                    ErrorCode::DriverFailed,
                    "chromiumoxide browser is unavailable",
                )
            })?;
            let begins = browser
                .event_listener::<EventDownloadWillBegin>()
                .await
                .map_err(|error| {
                    QuarryError::new(
                        ErrorCode::DriverFailed,
                        "chromiumoxide download-begin listener failed",
                    )
                    .with_details(json!({ "error": error.to_string() }))
                })?;
            let progress = browser
                .event_listener::<EventDownloadProgress>()
                .await
                .map_err(|error| {
                    QuarryError::new(
                        ErrorCode::DriverFailed,
                        "chromiumoxide download-progress listener failed",
                    )
                    .with_details(json!({ "error": error.to_string() }))
                })?;
            (begins, progress)
        };

        execute_native_target_action(&page, target, VerifiedTargetOperation::Click).await?;

        let began = tokio::time::timeout(DOWNLOAD_BEGIN_TIMEOUT, async {
            while let Some(event) = begins.next().await {
                let event_frame_id = event.frame_id.inner();
                if expected_frame_id == event_frame_id.as_str() {
                    return Some(event);
                }
            }
            None
        })
        .await
        .map_err(|_| QuarryError::new(ErrorCode::Timeout, "browser download did not begin"))?
        .ok_or_else(|| {
            QuarryError::new(ErrorCode::DriverFailed, "browser download listener ended")
        })?;

        let guid = began.guid.clone();
        let completed_path = tokio::time::timeout(DOWNLOAD_COMPLETE_TIMEOUT, async {
            while let Some(event) = progress.next().await {
                if event.guid != guid {
                    continue;
                }
                if event.received_bytes.is_finite()
                    && event.received_bytes > MAX_GOVERNED_DOWNLOAD_BYTES as f64
                {
                    if let Some(browser) = self.browser.lock().await.as_ref() {
                        if let Ok(cancel) = CancelDownloadParams::builder()
                            .guid(guid.clone())
                            .browser_context_id(context_id.clone())
                            .build()
                        {
                            let _ = browser.execute(cancel).await;
                        }
                    }
                    return Err(QuarryError::new(
                        ErrorCode::BadRequest,
                        "browser download exceeds the governed size limit",
                    ));
                }
                match event.state {
                    DownloadProgressState::Completed => return Ok(event.file_path.clone()),
                    DownloadProgressState::Canceled => {
                        return Err(QuarryError::new(
                            ErrorCode::DriverFailed,
                            "browser download was canceled",
                        ));
                    }
                    DownloadProgressState::InProgress => {}
                }
            }
            Err(QuarryError::new(
                ErrorCode::DriverFailed,
                "browser download progress listener ended",
            ))
        })
        .await
        .map_err(|_| QuarryError::new(ErrorCode::Timeout, "browser download did not complete"))??;

        let root = tokio::fs::canonicalize(download_dir.path())
            .await
            .map_err(|error| {
                QuarryError::new(
                    ErrorCode::DriverFailed,
                    "download quarantine directory disappeared",
                )
                .with_details(json!({ "error": error.to_string() }))
            })?;
        let candidate = completed_path
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join(&guid));
        let file = tokio::fs::canonicalize(&candidate).await.map_err(|error| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                "browser download file is unavailable",
            )
            .with_details(json!({ "error": error.to_string() }))
        })?;
        if !file.starts_with(&root) {
            return Err(QuarryError::new(
                ErrorCode::SecurityBlocked,
                "browser download escaped Quarry's private quarantine directory",
            ));
        }
        let metadata = tokio::fs::metadata(&file).await.map_err(|error| {
            QuarryError::new(ErrorCode::DriverFailed, "inspect browser download failed")
                .with_details(json!({ "error": error.to_string() }))
        })?;
        if !metadata.is_file() || metadata.len() > MAX_GOVERNED_DOWNLOAD_BYTES {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "browser download failed governed type or size admission",
            ));
        }
        let bytes = tokio::fs::read(file).await.map_err(|error| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                "read quarantined browser download failed",
            )
            .with_details(json!({ "error": error.to_string() }))
        })?;
        Ok(BrowserDownloadedFile {
            bytes: Bytes::from(bytes),
            suggested_filename: sanitize_download_filename(&began.suggested_filename),
            source_url: redact_egress_url(&began.url),
        })
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
        self.require_configured_egress(session).await?;
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
        let (removed, empty_context) = {
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
            let empty_context = if session_pages.tabs.is_empty() {
                pages
                    .remove(&session_key)
                    .and_then(|session_pages| session_pages.browser_context_id)
            } else {
                None
            };
            (removed, empty_context)
        };

        if let Err(err) = removed.page.close().await {
            tracing::debug!(
                tab_id = %removed.tab_id,
                error = %err,
                "chromiumoxide close_tab failed"
            );
        }
        if let Some(context_id) = empty_context {
            let browser_guard = self.browser.lock().await;
            if let Some(browser) = browser_guard.as_ref() {
                if let Err(error) = browser.dispose_browser_context(context_id).await {
                    tracing::warn!(
                        session_key = %session_key,
                        error = %error,
                        "chromiumoxide dispose_browser_context after final tab close failed"
                    );
                }
            }
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

    async fn egress_receipts(
        &self,
        session: &BrowserSession,
        after_sequence: u64,
        limit: usize,
    ) -> QuarryResult<Vec<BrowserEgressReceipt>> {
        let session_key = Self::session_key(session);
        let limit = limit.clamp(1, DEVTOOLS_EVENT_BUFFER_LIMIT);
        self.synchronize_proxy_egress_receipts(&session_key).await?;
        let buffers = self.egress_receipts.lock().await;
        let Some(receipts) = buffers.get(&session_key) else {
            return Ok(Vec::new());
        };
        if let Some(first_pending) = receipts
            .iter()
            .find(|receipt| receipt.sequence > after_sequence)
        {
            let expected = after_sequence.saturating_add(1);
            if first_pending.sequence != expected {
                return Err(QuarryError::new(
                    ErrorCode::Conflict,
                    "browser egress receipt continuity was lost; stop and re-observe",
                )
                .with_details(json!({
                    "after_sequence": after_sequence,
                    "first_available_sequence": first_pending.sequence,
                })));
            }
        }
        Ok(receipts
            .iter()
            .filter(|receipt| receipt.sequence > after_sequence)
            .take(limit)
            .cloned()
            .collect())
    }

    async fn telemetry(&self, session: &BrowserSession) -> QuarryResult<BrowserTelemetry> {
        let session_key = Self::session_key(session);
        let base = self
            .session_telemetry
            .lock()
            .await
            .get(&session_key)
            .cloned()
            .unwrap_or_default();
        let page = self.current_page(session).await?;
        let metrics = page.metrics().await.map_err(|error| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                "chromiumoxide performance metrics query failed",
            )
            .with_details(json!({ "error": error.to_string() }))
        })?;
        let scaled_metric = |name: &str, scale: f64| {
            metrics
                .iter()
                .find(|metric| metric.name == name)
                .and_then(|metric| finite_metric_u64(metric.value, scale))
        };
        Ok(BrowserTelemetry {
            renderer_task_cpu_ms: scaled_metric("TaskDuration", 1_000.0),
            renderer_js_heap_used_bytes: scaled_metric("JSHeapUsedSize", 1.0),
            ..base
        })
    }

    async fn native_page_projection(
        &self,
        session: &BrowserSession,
    ) -> QuarryResult<BrowserNativeProjection> {
        let page = self.current_page(session).await?;
        page.execute(AccessibilityEnableParams::default())
            .await
            .map_err(|error| {
                QuarryError::new(
                    ErrorCode::DriverFailed,
                    "chromiumoxide accessibility domain enable failed",
                )
                .with_details(json!({ "error": error.to_string() }))
            })?;
        let ax_tree = page
            .execute(GetFullAxTreeParams::builder().depth(8).build())
            .await
            .map_err(|error| {
                QuarryError::new(
                    ErrorCode::DriverFailed,
                    "chromiumoxide accessibility tree capture failed",
                )
                .with_details(json!({ "error": error.to_string() }))
            })?;
        let frame_tree = page
            .execute(GetFrameTreeParams::default())
            .await
            .map_err(|error| {
                QuarryError::new(
                    ErrorCode::DriverFailed,
                    "chromiumoxide frame tree capture failed",
                )
                .with_details(json!({ "error": error.to_string() }))
            })?;
        let ax_tree = serde_json::to_value(&ax_tree.result).map_err(|error| {
            QuarryError::new(
                ErrorCode::Internal,
                "chromiumoxide accessibility tree serialization failed",
            )
            .with_details(json!({ "error": error.to_string() }))
        })?;
        let frame_tree = serde_json::to_value(&frame_tree.result).map_err(|error| {
            QuarryError::new(
                ErrorCode::Internal,
                "chromiumoxide frame tree serialization failed",
            )
            .with_details(json!({ "error": error.to_string() }))
        })?;
        Ok(native_projection_from_cdp(ax_tree, frame_tree))
    }

    async fn dialogs(&self, session: &BrowserSession) -> QuarryResult<Vec<BrowserDialog>> {
        let session_key = Self::session_key(session);
        Ok(self
            .dialogs
            .lock()
            .await
            .get(&session_key)
            .cloned()
            .unwrap_or_default())
    }

    async fn respond_dialog(
        &self,
        session: &BrowserSession,
        dialog_id: &str,
        accept: bool,
        prompt_text: Option<&str>,
    ) -> QuarryResult<()> {
        let session_key = Self::session_key(session);
        {
            let dialogs = self.dialogs.lock().await;
            if !dialogs
                .get(&session_key)
                .is_some_and(|active| active.iter().any(|dialog| dialog.dialog_id == dialog_id))
            {
                return Err(QuarryError::new(
                    ErrorCode::TargetRepairRequired,
                    "dialog is no longer active; observe the browser again",
                ));
            }
        }
        let page = self
            .dialog_pages
            .lock()
            .await
            .get(&session_key)
            .and_then(|pages| pages.get(dialog_id))
            .cloned()
            .ok_or_else(|| {
                QuarryError::new(
                    ErrorCode::TargetRepairRequired,
                    "dialog no longer has an owning browser target; observe again",
                )
            })?;
        let mut params = HandleJavaScriptDialogParams::new(accept);
        params.prompt_text = prompt_text.map(str::to_owned);
        page.execute(params).await.map_err(|error| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                "chromiumoxide dialog response failed",
            )
            .with_details(json!({ "error": error.to_string() }))
        })?;
        self.dialogs
            .lock()
            .await
            .entry(session_key.clone())
            .and_modify(|active| active.retain(|dialog| dialog.dialog_id != dialog_id));
        self.dialog_pages
            .lock()
            .await
            .entry(Self::session_key(session))
            .and_modify(|pages| {
                pages.remove(dialog_id);
            });
        Ok(())
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

/// Return the opaque top-level frame id for the active page. This stays inside
/// the driver and is used only to correlate an otherwise frame-less native AX
/// target to the download event that its click causes.
async fn root_frame_id(page: &Page) -> QuarryResult<String> {
    let frame_tree = page
        .execute(GetFrameTreeParams::default())
        .await
        .map_err(|error| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                "chromiumoxide root frame lookup failed for governed download",
            )
            .with_details(json!({ "error": error.to_string() }))
        })?;
    serde_json::to_value(&frame_tree.result)
        .ok()
        .and_then(|tree| {
            tree.get("frameTree")
                .and_then(|frame_tree| frame_tree.get("frame"))
                .and_then(|frame| frame.get("id"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .filter(|frame_id| !frame_id.is_empty())
        .ok_or_else(|| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                "chromiumoxide root frame id is unavailable for governed download",
            )
        })
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
    use wiremock::MockServer;

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

    #[tokio::test]
    async fn blocks_loopback_subresources_before_they_reach_the_server() {
        let target = MockServer::start().await;
        let driver = ChromiumoxideDriver::new();
        let session = driver.acquire(&make_lease()).await.expect("acquire");
        driver
            .new_tab(&session, None)
            .await
            .expect("open blank tab");
        let page = driver.current_page(&session).await.expect("current page");

        page.set_content(format!("<img src=\"{}/private.png\">", target.uri()))
            .await
            .expect("inject page-controlled subresource");
        tokio::time::sleep(Duration::from_millis(250)).await;

        assert!(
            target
                .received_requests()
                .await
                .expect("request log")
                .is_empty(),
            "the browser must never contact a loopback subresource"
        );
        driver.release(session).await.expect("release");
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
                name: "verevon_test".into(),
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
