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
use ::chromiumoxide::cdp::browser_protocol::emulation::{
    SetDeviceMetricsOverrideParams, SetFocusEmulationEnabledParams, SetTouchEmulationEnabledParams,
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
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;

use ::chromiumoxide::cdp::browser_protocol::page::{
    CaptureScreenshotFormat, CaptureScreenshotParamsBuilder, EventJavascriptDialogOpening,
    EventLifecycleEvent, EventScreencastFrame, FrameId, GetFrameTreeParams,
    HandleJavaScriptDialogParams, PrintToPdfParams, ScreencastFrameAckParams,
    SetWebLifecycleStateParams, SetWebLifecycleStateState, StartScreencastFormat,
    StartScreencastParams, StopScreencastParams,
};
use ::chromiumoxide::cdp::js_protocol::runtime::{
    CallArgument, CallFunctionOnParams, EventConsoleApiCalled, ReleaseObjectParams,
};
use ::chromiumoxide::error::CdpError;
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

use crate::actionability::{self, DialogWatch};
use crate::actions::ScrollTarget;
use crate::deadlines::{best_effort, deadline, read_with_retry, timeout_error, with_deadline, Op};
use crate::navigation::{guard_navigation_target, guard_page_request_target};
use crate::session::{Cookie, ProfileStore, SessionSnapshot, Viewport};
use crate::slots::{
    Admission, BrowserPoolConfig, LossReason, PageRefusal, ReaperMode, SlotBook, SlotId,
    SlotStatus,
};
use crate::{
    BrowserDevtoolsEvent, BrowserDownloadedFile, BrowserDriver, BrowserDriverCapabilities,
    BrowserEgressPolicy, BrowserEgressProxyProvider, BrowserHealth, BrowserNativeProjection,
    BrowserNativeTarget, BrowserSession, BrowserTab, LiveFrame, LiveFrameFormat, LiveFrameOptions,
    SessionInner, SlotHealth, VerifiedTargetAction, VerifiedTargetOperation,
};

const LIVE_FRAME_CACHE_TTL_MS: u64 = 250;
const DEVTOOLS_EVENT_BUFFER_LIMIT: usize = 512;
const MAX_GOVERNED_DOWNLOAD_BYTES: u64 = 25 * 1024 * 1024;
/// The driver runs one browser process today (K = 1, ADR-10).
const SLOT: SlotId = 0;
/// How long a retired browser gets to exit before its directories go.
const BROWSER_EXIT_WAIT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
struct CachedLiveFrame {
    tab_id: String,
    page_epoch: u64,
    options: LiveFrameOptions,
    captured_at: Instant,
    frame: LiveFrame,
}

/// One session's live-frame cache. The epoch changes whenever the session's
/// page changes, so a capture that started before the change is not stored.
/// `owner` is the book entry the slot belongs to: a key that is released and
/// admitted again is a different session, and nothing of the old one
/// reaches the new one's slot.
#[derive(Debug)]
struct LiveFrameSlot {
    owner: u64,
    epoch: u64,
    cached: Option<CachedLiveFrame>,
}

/// What [`LiveFrameCache::begin`] found.
#[derive(Debug)]
enum FrameLookup {
    /// A fresh cached frame of this entry, tab and options.
    Hit(LiveFrame),
    /// Capture, then [`LiveFrameCache::store`] under this owner and epoch.
    Capture { owner: u64, epoch: u64 },
    /// The call's entry is not the key's current one (released, or released
    /// and admitted again): no slot is read or made.
    NotOwner,
}

/// Every session's live-frame cache, by session key. Pure bookkeeping; the
/// driver keeps it under one async mutex.
#[derive(Debug, Default)]
struct LiveFrameCache {
    slots: HashMap<String, LiveFrameSlot>,
}

impl LiveFrameCache {
    /// A capture for entry `owner` starts; `current` is the entry the key is
    /// admitted under now. A slot left by an earlier entry is dropped.
    fn begin(
        &mut self,
        key: &str,
        owner: Option<u64>,
        current: Option<u64>,
        tab_id: &str,
        options: LiveFrameOptions,
        fresh_epoch: impl FnOnce() -> u64,
    ) -> FrameLookup {
        let (Some(owner), Some(current)) = (owner, current) else {
            return FrameLookup::NotOwner;
        };
        if owner != current {
            return FrameLookup::NotOwner;
        }
        if self.slots.get(key).is_some_and(|slot| slot.owner != owner) {
            self.slots.remove(key);
        }
        let slot = self
            .slots
            .entry(key.to_owned())
            .or_insert_with(|| LiveFrameSlot {
                owner,
                epoch: fresh_epoch(),
                cached: None,
            });
        if let Some(cached) = slot.cached.as_ref() {
            if cached.tab_id == tab_id
                && cached.page_epoch == slot.epoch
                && cached.options == options
                && cached.captured_at.elapsed() <= Duration::from_millis(LIVE_FRAME_CACHE_TTL_MS)
            {
                return FrameLookup::Hit(cached.frame.clone());
            }
        }
        FrameLookup::Capture {
            owner,
            epoch: slot.epoch,
        }
    }

    /// Keep a frame captured under `owner` for the page at `frame.page_epoch`,
    /// only while that slot is still the entry's and its page did not change.
    fn store(&mut self, key: &str, owner: u64, frame: CachedLiveFrame) -> bool {
        match self.slots.get_mut(key) {
            Some(slot) if slot.owner == owner && slot.epoch == frame.page_epoch => {
                slot.cached = Some(frame);
                true
            }
            _ => false,
        }
    }

    /// An operation that started under entry `owner` changed the page.
    fn invalidate(&mut self, key: &str, owner: Option<u64>, current: Option<u64>, epoch: u64) {
        match current {
            // A released session gets no slot back (its teardown removed it).
            None => {
                self.slots.remove(key);
            }
            Some(current) if Some(current) == owner => {
                self.slots.insert(
                    key.to_owned(),
                    LiveFrameSlot {
                        owner: current,
                        epoch,
                        cached: None,
                    },
                );
            }
            // Released and admitted again while the operation ran: the slot
            // is the newer session's, and this late call leaves it alone.
            Some(_) => {}
        }
    }

    fn remove(&mut self, key: &str) -> Option<LiveFrameSlot> {
        self.slots.remove(key)
    }

    fn clear(&mut self) {
        self.slots.clear();
    }

    #[cfg(test)]
    fn contains_key(&self, key: &str) -> bool {
        self.slots.contains_key(key)
    }
}

#[derive(Debug, Clone)]
struct TabPage {
    tab_id: String,
    page: Page,
    /// The book entry that reserved this page; closing it gives the page
    /// back to that entry only.
    admission: u64,
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
    /// The running browser. Every CDP call takes the read side, so sessions
    /// attach, probe and tear down in parallel and a slow `new_page` never
    /// blocks another session; only a launch, a relaunch and the exit check
    /// take the write side.
    browser: Arc<RwLock<Option<Browser>>>,
    pages: Arc<Mutex<HashMap<String, SessionPages>>>,
    handler: Arc<Mutex<Option<JoinHandle<()>>>>,
    /// How many times this driver has launched a browser. A launch is rare
    /// and clears per-browser state, so tests assert it stays at one while
    /// leases with different viewports come and go.
    launches: Arc<AtomicUsize>,
    /// Who holds a session, on which slot, with how many pages (slots.rs).
    /// Innermost lock: a `std` mutex, never held across an `.await`.
    book: Arc<std::sync::Mutex<SlotBook>>,
    pool: BrowserPoolConfig,
    /// Where the running browser keeps its profile and crash dumps; removed
    /// when that browser is retired.
    launch_dirs: Arc<Mutex<Option<ChromiumLaunchDirs>>>,
    /// One tab attach at a time per session, so two attaches cannot each
    /// create a browser context; attaches of different sessions run in
    /// parallel (the browser lock is held only while creating).
    attach_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    /// The reaper and the prober, started at the first `acquire`.
    background: Arc<std::sync::Mutex<Vec<JoinHandle<()>>>>,
    background_started: Arc<AtomicBool>,
    /// False for the views the background tasks hold: only the driver itself
    /// tears down on drop.
    owner: bool,
    /// A teardown in progress, per session: an `acquire` of the same key
    /// waits for it, so a teardown never frees the next session's state.
    teardowns: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    /// Test hooks: stalls and a stale content read.
    #[cfg(test)]
    hooks: Arc<TestHooks>,
    frame_epochs: Arc<AtomicU64>,
    live_frames: Arc<Mutex<LiveFrameCache>>,
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
    /// `QUARRY_BROWSER_TRUSTED_INPUT=on`: ref-targeted click, type and select
    /// on a top-level target run the actionability pipeline and send trusted
    /// input (WS3.K). Off by default.
    trusted_input: bool,
}

/// How a native target action reaches its element.
#[derive(Clone)]
enum InputMode {
    /// The in-page script that always was: `element.click()`, a value setter,
    /// synthetic events. Governed downloads, uploads, child-frame targets and
    /// `wait_for` always use it.
    Synthetic,
    /// The actionability pipeline and real CDP input (see `actionability.rs`),
    /// with the session's dialog list so a click that opens an `alert` returns.
    Trusted(DialogWatch),
}

impl Default for ChromiumoxideDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl ChromiumoxideDriver {
    pub fn new() -> Self {
        let pool = BrowserPoolConfig::from_env();
        Self {
            browser: Arc::new(RwLock::new(None)),
            pages: Arc::new(Mutex::new(HashMap::new())),
            handler: Arc::new(Mutex::new(None)),
            launches: Arc::new(AtomicUsize::new(0)),
            book: Arc::new(std::sync::Mutex::new(SlotBook::new(pool.slot_config()))),
            pool,
            launch_dirs: Arc::new(Mutex::new(None)),
            attach_locks: Arc::new(Mutex::new(HashMap::new())),
            background: Arc::new(std::sync::Mutex::new(Vec::new())),
            background_started: Arc::new(AtomicBool::new(false)),
            owner: true,
            teardowns: Arc::new(Mutex::new(HashMap::new())),
            #[cfg(test)]
            hooks: Arc::new(TestHooks::default()),
            frame_epochs: Arc::new(AtomicU64::new(0)),
            live_frames: Arc::new(Mutex::new(LiveFrameCache::default())),
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
            trusted_input: actionability::parse_trusted_input(
                std::env::var("QUARRY_BROWSER_TRUSTED_INPUT")
                    .ok()
                    .as_deref(),
            ),
        }
    }

    /// Turn trusted input (the actionability pipeline) on or off, overriding
    /// `QUARRY_BROWSER_TRUSTED_INPUT`. Call before the first `acquire`.
    pub fn with_trusted_input(mut self, enabled: bool) -> Self {
        self.trusted_input = enabled;
        self
    }

    /// The input mode for one native target of `session`. Trusted only with
    /// the flag on and only for the page's own frame: a child frame has its
    /// own coordinate space and may live in another process, so its targets
    /// stay on the synthetic path (WS3.L).
    async fn input_mode_for(
        &self,
        session: &BrowserSession,
        page: &Page,
        target: &BrowserNativeTarget,
    ) -> InputMode {
        if !self.trusted_input {
            return InputMode::Synthetic;
        }
        if let Some(frame_id) = target.frame_id.as_deref() {
            match root_frame_id(page).await {
                Ok(root) if root == frame_id => {}
                _ => return InputMode::Synthetic,
            }
        }
        InputMode::Trusted(DialogWatch {
            dialogs: self.dialogs.clone(),
            session_key: Self::session_key(session),
        })
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

    /// Replace the pool settings read from the environment (session cap, page
    /// cap, reaper and prober). Call before the first `acquire`.
    pub fn with_pool_config(mut self, pool: BrowserPoolConfig) -> Self {
        self.pool = pool;
        self.book = Arc::new(std::sync::Mutex::new(SlotBook::new(pool.slot_config())));
        self
    }

    /// A non-owning view over the same state, for the background tasks. Its
    /// drop tears nothing down.
    fn share(&self) -> Self {
        Self {
            browser: self.browser.clone(),
            pages: self.pages.clone(),
            handler: self.handler.clone(),
            launches: self.launches.clone(),
            book: self.book.clone(),
            pool: self.pool,
            launch_dirs: self.launch_dirs.clone(),
            attach_locks: self.attach_locks.clone(),
            background: self.background.clone(),
            background_started: self.background_started.clone(),
            owner: false,
            teardowns: self.teardowns.clone(),
            #[cfg(test)]
            hooks: self.hooks.clone(),
            frame_epochs: self.frame_epochs.clone(),
            live_frames: self.live_frames.clone(),
            devtools_events: self.devtools_events.clone(),
            devtools_sequence: self.devtools_sequence.clone(),
            egress_receipts: self.egress_receipts.clone(),
            egress_sequence: self.egress_sequence.clone(),
            egress_policies: self.egress_policies.clone(),
            pinned_egress_proxy: self.pinned_egress_proxy.clone(),
            proxy_receipt_cursors: self.proxy_receipt_cursors.clone(),
            session_telemetry: self.session_telemetry.clone(),
            dialogs: self.dialogs.clone(),
            dialog_pages: self.dialog_pages.clone(),
            dialog_sequence: self.dialog_sequence.clone(),
            profiles: self.profiles.clone(),
            trusted_input: self.trusted_input,
        }
    }

    /// The slot book. Never hold the guard across an `.await`.
    fn book(&self) -> std::sync::MutexGuard<'_, SlotBook> {
        self.book
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// How many browsers this driver has launched (tests only).
    #[cfg(test)]
    fn launch_count(&self) -> usize {
        self.launches.load(Ordering::SeqCst)
    }

    /// Start the reaper and the prober once, on the first `acquire` (a driver
    /// that is only constructed, as in most edge tests, starts nothing).
    /// `QUARRY_BROWSER_REAPER=0` leaves the reaper out. The prober always
    /// runs: it is what relaunches a crashed browser on an edge that `/ready`
    /// has taken out of traffic, where no `acquire` comes to do it.
    fn start_background(&self) {
        if self.background_started.swap(true, Ordering::SeqCst) {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            self.background_started.store(false, Ordering::SeqCst);
            return;
        };
        let reaper = (self.pool.reaper != ReaperMode::Off).then(|| {
            let view = self.share();
            let every = self.pool.reaper_interval;
            runtime.spawn(async move {
                let mut ticks = tokio::time::interval(every);
                ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                ticks.tick().await;
                loop {
                    ticks.tick().await;
                    view.reaper_tick().await;
                }
            })
        });
        let prober = {
            let view = self.share();
            let every = self.pool.probe_interval;
            runtime.spawn(async move {
                let mut ticks = tokio::time::interval(every);
                ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                ticks.tick().await;
                loop {
                    ticks.tick().await;
                    view.probe_tick().await;
                }
            })
        };
        let mut background = self
            .background
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        background.extend(reaper);
        background.push(prober);
    }

    /// One pass of the idle reaper. In `log` mode it only says what it would
    /// do; with `1` it tears the sessions down (without saving their profile:
    /// an abandoned session's state is not a checkpoint anyone asked for).
    async fn reaper_tick(&self) {
        if self.pool.reaper != ReaperMode::On {
            for reaped in self.book().reap_idle(Instant::now(), false) {
                tracing::info!(
                    reason = reaped.reason.as_str(),
                    slot = reaped.slot,
                    pages = reaped.pages,
                    mode = "log",
                    "browser session would be reaped"
                );
            }
            return;
        }
        // One session at a time, decided again under its teardown gate: a key
        // released and admitted again between the book's first look and the
        // teardown is not due any more, and its new state is left alone.
        let candidates = self.book().reap_candidates(Instant::now());
        for key in candidates {
            let gate = self.teardown_gate(&key).await;
            let _tearing_down = gate.lock().await;
            let reaped = self.book().reap_if_due(&key, Instant::now());
            if let Some(reaped) = reaped {
                self.teardown_locked(&key).await;
                tracing::info!(
                    reason = reaped.reason.as_str(),
                    slot = reaped.slot,
                    pages = reaped.pages,
                    "browser session reaped"
                );
            }
            self.drop_teardown_gate(&key, &gate).await;
        }
    }

    /// One pass of the health prober over the slot. A browser whose process
    /// has exited is marked dead and relaunched here, so an edge that answers
    /// `/ready` 503 (and so gets no traffic) can still recover. An unhealthy
    /// slot that holds sessions is never killed; an empty one is recycled.
    async fn probe_tick(&self) {
        let Some((status, generation)) = self
            .book()
            .slot(SLOT)
            .map(|slot| (slot.status, slot.generation))
        else {
            return;
        };
        if generation == 0 {
            // Never launched: nothing to probe, and that is ready.
            return;
        }
        // `try_wait` needs the write side. Busy (sessions are attaching) means
        // the browser is answering; the handler task still reports an exit,
        // and the next tick checks again.
        let exited = match self.browser.try_write() {
            Ok(mut guard) => match guard.as_mut() {
                Some(browser) => matches!(browser.try_wait(), Ok(Some(_))),
                None => false,
            },
            Err(_) => false,
        };
        if exited {
            self.on_browser_lost(SLOT, generation).await;
        }
        if exited || status == SlotStatus::Dead {
            if let Err(error) = self.ensure_browser().await {
                tracing::warn!(
                    slot = SLOT,
                    code = error.code.wire_name(),
                    "browser slot relaunch failed"
                );
            }
            return;
        }
        let ok = match self.run_probe(SLOT).await {
            Ok(()) => true,
            Err(error) => {
                tracing::debug!(
                    slot = SLOT,
                    code = error.code.wire_name(),
                    "browser health probe failed"
                );
                #[cfg(test)]
                self.hooks
                    .probe_errors
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push((error.code, error.message.clone()));
                false
            }
        };
        let (became_unhealthy, unhealthy) = {
            let mut book = self.book();
            let became = book.record_probe(SLOT, generation, ok);
            let unhealthy = book
                .slot(SLOT)
                .is_some_and(|slot| slot.healthy == Some(false));
            (became, unhealthy)
        };
        if became_unhealthy {
            tracing::warn!(slot = SLOT, generation, "browser slot unhealthy");
        }
        // Nobody is using it: recycle. With sessions on it, never. The count is
        // read again under the book lock that marks it dead, so a session
        // admitted while the probe ran is never thrown away.
        let recycled = unhealthy && {
            let mut book = self.book();
            let empty = book.slot(SLOT).is_some_and(|slot| slot.sessions == 0);
            if empty {
                book.mark_dead(SLOT, generation, Instant::now());
            }
            empty
        };
        if recycled {
            tracing::warn!(slot = SLOT, generation, "browser slot recycled");
            if let Err(error) = self.ensure_browser().await {
                tracing::warn!(
                    slot = SLOT,
                    code = error.code.wire_name(),
                    "browser slot relaunch failed"
                );
            }
        }
    }

    /// Open and close a page in a throwaway context, under `Op::HealthProbe`.
    /// The context is made like a session's (`create_session_context`), so it
    /// goes through the pinned egress proxy when there is one, with a deny-all
    /// policy installed first. It never touches a session's pages, policies or
    /// receipts.
    async fn run_probe(&self, slot: SlotId) -> QuarryResult<()> {
        let probe_key = format!("probe:{slot}");
        let mut guard = AttachGuard::new(self, probe_key.clone(), None);
        let outcome = with_deadline(Op::HealthProbe, async {
            if let Some(provider) = &self.pinned_egress_proxy {
                provider
                    .configure_policy(&probe_key, BrowserEgressPolicy::deny_all())
                    .await?;
            }
            let context_id = self.create_session_context(&probe_key).await?;
            guard.context = Some(context_id.clone());
            let params = CreateTargetParamsBuilder::default()
                .url("about:blank")
                .browser_context_id(context_id)
                .build()
                .map_err(|error| {
                    QuarryError::new(ErrorCode::Internal, "probe page params failed")
                        .with_details(json!({ "error": error }))
                })?;
            let page = {
                let browser_guard = self.browser.read().await;
                let browser = browser_guard.as_ref().ok_or_else(browser_missing)?;
                browser.new_page(params).await.map_err(|_| {
                    QuarryError::new(ErrorCode::DriverFailed, "probe page did not open")
                })?
            };
            guard.page = Some(page.clone());
            Ok(())
        })
        .await;
        // Tear down inline; a probe that timed out leaves it to the guard.
        if outcome.is_ok() {
            guard.close_now(self).await;
        }
        drop(guard);
        if let Some(provider) = &self.pinned_egress_proxy {
            provider.release_session(&probe_key).await;
        }
        outcome
    }

    /// The browser of `slot` at `generation` went away: mark it dead, drop the
    /// page handles of every session that was on it (their next call says
    /// `browser_process_lost`) and log it once.
    async fn on_browser_lost(&self, slot: SlotId, generation: u64) {
        let (changed, lost) = {
            let mut book = self.book();
            let live = book
                .slot(slot)
                .is_some_and(|state| state.generation == generation && state.status != SlotStatus::Dead);
            (live, book.mark_dead(slot, generation, Instant::now()))
        };
        if !changed {
            return;
        }
        tracing::warn!(
            slot,
            generation,
            sessions = lost.len(),
            "browser process lost"
        );
        for key in &lost {
            self.pages.lock().await.remove(key);
            self.dialog_pages.lock().await.remove(key);
            self.live_frames.lock().await.remove(key);
        }
    }

    /// Take the browser out of service: kill it if it still runs, wait for it
    /// (bounded), then delete its profile and crash-dump directories.
    async fn retire_browser(mut browser: Browser, dirs: Option<ChromiumLaunchDirs>) {
        let _ = tokio::time::timeout(BROWSER_EXIT_WAIT, browser.kill()).await;
        let _ = tokio::time::timeout(BROWSER_EXIT_WAIT, browser.wait()).await;
        drop(browser);
        if let Some(dirs) = dirs {
            dirs.remove();
        }
    }

    /// Launch the browser if there is none, or if the one there is dead. A
    /// viewport is never a reason to launch: it is applied per page (see
    /// `apply_page_hygiene`), so a lease that asks for a different size cannot
    /// close the browser under the other sessions that are using it.
    async fn ensure_browser(&self) -> QuarryResult<()> {
        // The common case, a running browser, needs only the read side, so it
        // never queues behind (or in front of) other sessions' CDP calls.
        {
            let guard = self.browser.read().await;
            let dead = self
                .book()
                .slot(SLOT)
                .is_some_and(|slot| slot.status == SlotStatus::Dead);
            if guard.is_some() && !dead {
                return Ok(());
            }
        }
        let mut guard = self.browser.write().await;
        let dead = self
            .book()
            .slot(SLOT)
            .is_some_and(|slot| slot.status == SlotStatus::Dead);
        if guard.is_some() && !dead {
            return Ok(());
        }
        // A fresh launch: whatever was kept for a previous browser is stale.
        if let Some(handle) = self.handler.lock().await.take() {
            handle.abort();
        }
        if let Some(old) = guard.take() {
            let dirs = self.launch_dirs.lock().await.take();
            Self::retire_browser(old, dirs).await;
        } else if self.launches.load(Ordering::SeqCst) == 0 {
            sweep_stale_launch_dirs();
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
        self.live_frames.lock().await.clear();

        let launch_dirs = chromium_launch_dirs().map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                "chromiumoxide launch directory setup failed",
            )
            .with_details(json!({ "error": e.to_string() }))
        })?;
        let mut builder = BrowserConfig::builder()
            // Above the largest deadline in the table, so a typed Timeout from
            // `with_deadline` always beats the library's untyped one; this only
            // bounds the calls that are not wrapped (event collectors, guards).
            .request_timeout(Duration::from_secs(40))
            .launch_timeout(Duration::from_secs(20))
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
        // No browser-wide emulation: chromiumoxide's default would force one
        // 800x600 viewport on every page. Each page is sized when it opens.
        builder = builder.viewport(None::<ChromiumViewport>);
        match sandbox_policy(is_production_env(), env_truthy("QUARRY_BROWSER_NO_SANDBOX")) {
            Ok(SandboxMode::Enabled) => {}
            Ok(SandboxMode::DisabledByOperator) => {
                tracing::warn!(
                    "chromium sandbox disabled by QUARRY_BROWSER_NO_SANDBOX; \
                     only acceptable on an isolated development stack"
                );
                builder = builder.no_sandbox();
            }
            Err(reason) => {
                return Err(QuarryError::new(ErrorCode::DriverFailed, reason));
            }
        }

        let config = builder.build().map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "chromiumoxide config build failed")
                .with_details(json!({ "error": e.to_string() }))
        })?;

        let launched = with_deadline(Op::Launch, async {
            Browser::launch(config).await.map_err(|e| {
                QuarryError::new(ErrorCode::DriverFailed, "chromiumoxide launch failed")
                    .with_details(json!({ "error": e.to_string() }))
            })
        })
        .await;
        let (browser, mut events) = match launched {
            Ok(launched) => launched,
            Err(error) => {
                launch_dirs.remove();
                return Err(error);
            }
        };

        self.launches.fetch_add(1, Ordering::SeqCst);
        let generation = self.book().launched(SLOT);
        // Drain CDP events. The websocket failing means the browser process is
        // gone (killed, crashed, out of memory): mark the slot dead so the next
        // acquire relaunches it and its sessions get a typed loss.
        let watcher = self.share();
        let handle = tokio::spawn(async move {
            while let Some(event) = events.next().await {
                if let Err(CdpError::Ws(_)) = event {
                    break;
                }
            }
            watcher.on_browser_lost(SLOT, generation).await;
        });

        *guard = Some(browser);
        *self.launch_dirs.lock().await = Some(launch_dirs);
        *self.handler.lock().await = Some(handle);
        tracing::info!(slot = SLOT, generation, "browser slot launched");
        Ok(())
    }

    /// Create the isolated browser context a session's tabs live in (or a
    /// probe's throwaway page). With a pinned egress proxy the context is
    /// bound to that proxy's per-session endpoint, always: there is no other
    /// way to make a context in this driver. The browser lock is held only for
    /// the creation itself.
    async fn create_session_context(&self, session_key: &str) -> QuarryResult<BrowserContextId> {
        let context_params = if let Some(provider) = &self.pinned_egress_proxy {
            let endpoint = provider.endpoint_for_session(session_key).await?;
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
        let browser_guard = self.browser.read().await;
        let browser = browser_guard.as_ref().ok_or_else(browser_missing)?;
        with_deadline(Op::TabAttach, async {
            browser
                .create_browser_context(context_params)
                .await
                .map_err(|e| {
                    QuarryError::new(
                        ErrorCode::DriverFailed,
                        "chromiumoxide create_browser_context failed",
                    )
                    .with_details(json!({ "error": e.to_string() }))
                })
        })
        .await
    }

    /// Whether `session` was lost (reaped or its browser died): the typed
    /// error for it, so a later call never quietly opens a fresh tab.
    fn refuse_lost(&self, session: &BrowserSession) -> QuarryResult<()> {
        let key = Self::session_key(session);
        match self.book().loss(&key, Instant::now()) {
            Some(reason) => Err(session_lost_error(reason)),
            None => Ok(()),
        }
    }

    async fn attach_lock(&self, session_key: &str) -> Arc<Mutex<()>> {
        self.attach_locks
            .lock()
            .await
            .entry(session_key.to_owned())
            .or_default()
            .clone()
    }

    /// Everything `release` frees except the profile capture: the book entry,
    /// pages, context, collectors, policies, receipts and the proxy session.
    /// Safe to repeat.
    ///
    /// The book entry goes first: an attach still in flight then fails its
    /// final admission check and its guard closes the page and context it
    /// made, instead of adding them after the session's pages are gone. A new
    /// `acquire` of the same key waits for the teardown gate, so the teardown
    /// never frees the state of the session admitted after it.
    async fn teardown_session(&self, session_key: &str) {
        let gate = self.teardown_gate(session_key).await;
        let _tearing_down = gate.lock().await;
        self.book().release(session_key);
        self.teardown_locked(session_key).await;
        self.drop_teardown_gate(session_key, &gate).await;
    }

    /// The per-session gate `teardown_session` and the reaper hold.
    async fn teardown_gate(&self, session_key: &str) -> Arc<Mutex<()>> {
        self.teardowns
            .lock()
            .await
            .entry(session_key.to_owned())
            .or_default()
            .clone()
    }

    /// Forget the gate, unless a later teardown has replaced it.
    async fn drop_teardown_gate(&self, session_key: &str, gate: &Arc<Mutex<()>>) {
        let mut teardowns = self.teardowns.lock().await;
        if teardowns
            .get(session_key)
            .is_some_and(|current| Arc::ptr_eq(current, gate))
        {
            teardowns.remove(session_key);
        }
    }

    /// Wait for a teardown of `session_key` that is still running.
    async fn wait_for_teardown(&self, session_key: &str) {
        let gate = self.teardowns.lock().await.get(session_key).cloned();
        if let Some(gate) = gate {
            drop(gate.lock().await);
        }
    }

    /// The teardown itself. The caller holds the session's teardown gate and
    /// has already taken the session out of the book.
    async fn teardown_locked(&self, session_key: &str) {
        let session_key = session_key.to_owned();
        let removed = self.pages.lock().await.remove(&session_key);
        #[cfg(test)]
        {
            let stall = *self
                .hooks
                .teardown_stall
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(stall) = stall {
                tokio::time::sleep(stall).await;
            }
        }
        if let Some(session_pages) = removed {
            for tab in session_pages.tabs {
                if let Err(err) = best_effort(Op::PageClose, tab.page.close()).await {
                    tracing::debug!(
                        tab_id = %tab.tab_id,
                        error_class = crate::deadlines::cdp_class(&err),
                        "chromiumoxide tab close during release failed"
                    );
                }
            }
            if let Some(context_id) = session_pages.browser_context_id {
                let browser_guard = self.browser.read().await;
                if let Some(browser) = browser_guard.as_ref() {
                    if let Err(err) = best_effort(
                        Op::ContextClose,
                        browser.dispose_browser_context(context_id),
                    )
                    .await
                    {
                        tracing::warn!(
                            session_key = %session_key,
                            error_class = crate::deadlines::cdp_class(&err),
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
        self.live_frames.lock().await.remove(&session_key);
        self.attach_locks.lock().await.remove(&session_key);
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
        let found = {
            let pages = self.pages.lock().await;
            let session_pages = pages.get(&session_key)?;
            let active_tab_id = session_pages.active_tab_id.as_ref()?;
            session_pages
                .tabs
                .iter()
                .find(|tab| &tab.tab_id == active_tab_id)
                .map(|tab| (tab.tab_id.clone(), tab.page.clone()))
        };
        if found.is_some() {
            // Any use of the session's page counts as activity for the reaper.
            self.book().touch(&session_key, Instant::now());
        }
        found
    }

    /// The session's active page, or why there is none: `session_reaped`,
    /// `browser_process_lost` or `no_current_page` (all not retryable).
    async fn current_tab_page(&self, session: &BrowserSession) -> QuarryResult<(String, Page)> {
        if let Some(found) = self.active_tab_page(session).await {
            return Ok(found);
        }
        let key = Self::session_key(session);
        let loss = self.book().loss(&key, Instant::now());
        Err(match loss {
            Some(reason) => session_lost_error(reason),
            None => {
                let mut error = QuarryError::new(
                    ErrorCode::DriverFailed,
                    "chromiumoxide: no current page; call goto() first",
                )
                .with_details(json!({ "reason": "no_current_page" }));
                error.retryable = false;
                error
            }
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
        let session_key = Self::session_key(session);
        // One attach at a time for this session (so it cannot get two
        // contexts); other sessions attach in parallel.
        let attach_lock = self.attach_lock(&session_key).await;
        let _attaching = attach_lock.lock().await;

        // The page counts against the slot from here; the guard gives it back,
        // and closes whatever this call created, on every early return, on a
        // timeout and when the caller drops this future.
        let admission = self.reserve_page(&session_key)?;
        let mut attach = AttachGuard::new(self, session_key.clone(), Some(admission));
        self.ensure_browser().await?;

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
                let id = self.create_session_context(&session_key).await?;
                attach.context = Some(id.clone());
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
                {
                    let browser_guard = self.browser.read().await;
                    let browser = browser_guard.as_ref().ok_or_else(browser_missing)?;
                    with_deadline(Op::TabAttach, async {
                        browser.execute(download_behavior).await.map_err(|error| {
                            QuarryError::new(
                                ErrorCode::DriverFailed,
                                "chromiumoxide download quarantine configuration failed",
                            )
                            .with_details(json!({ "error": error.to_string() }))
                        })
                    })
                    .await?;
                }
                attach.download_dir = Some(download_dir);
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
        let page = {
            let browser_guard = self.browser.read().await;
            let browser = browser_guard.as_ref().ok_or_else(browser_missing)?;
            with_deadline(Op::TabAttach, async {
                browser.new_page(new_page_params).await.map_err(|e| {
                    QuarryError::new(ErrorCode::DriverFailed, "chromiumoxide new_page failed")
                        .with_details(json!({ "error": e.to_string(), "url": "about:blank" }))
                })
            })
            .await?
        };
        attach.page = Some(page.clone());
        #[cfg(test)]
        {
            let stall = *self
                .hooks
                .attach_stall
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(stall) = stall {
                self.hooks.attach_stalled.store(true, Ordering::SeqCst);
                tokio::time::sleep(stall).await;
            }
        }
        // Size and focus the page before anything observes or navigates it.
        with_deadline(
            Op::TabAttach,
            apply_page_hygiene(
                &page,
                resolve_viewport(session.lease.viewport),
                page_hygiene_enabled(),
            ),
        )
        .await?;
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
            with_deadline(Op::Navigate, async {
                page.goto(initial_url).await.map_err(|e| {
                    QuarryError::new(ErrorCode::DriverFailed, "chromiumoxide goto failed")
                        .with_details(json!({ "error": e.to_string(), "url": initial_url }))
                })
            })
            .await?;
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
                    with_deadline(Op::Navigate, async {
                        page.reload().await.map_err(|e| {
                            QuarryError::new(
                                ErrorCode::DriverFailed,
                                "chromiumoxide profile reload failed",
                            )
                            .with_details(json!({ "error": e.to_string(), "url": initial_url }))
                        })
                    })
                    .await?;
                }
            }
        }
        // A genuinely blank tab (no initial URL — e.g. `new_tab` with no `url`)
        // has no origin to scope cookies to, and chromiumoxide cannot set cookies
        // on `about:blank` regardless; cookie/storage hydration is skipped for
        // that case instead of hard-failing tab creation.
        install_popup_bridge(&page).await?;

        let mut pages = self.pages.lock().await;
        // Released, reaped or lost while attaching: the guard closes the page.
        if let Some(error) = self.not_admitted_error(&session_key) {
            return Err(error);
        }
        let session_pages = pages.entry(session_key).or_default();
        if let Some(context_id) = attach.context.take() {
            session_pages.browser_context_id = Some(context_id);
            session_pages.download_dir = attach.download_dir.take();
        }
        session_pages.tabs.push(TabPage {
            tab_id: tab_id.clone(),
            page: page.clone(),
            admission,
            title_hint: None,
            url_hint: Some(initial_url.to_owned()),
        });
        session_pages.active_tab_id = Some(tab_id.clone());
        attach.disarm();
        Ok((tab_id, page))
    }

    /// Count a page against the session's slot, or say why it cannot open:
    /// `browser_page_cap` (429), or the session is gone. Returns the book
    /// entry the page counts against, which the close has to name.
    fn reserve_page(&self, session_key: &str) -> QuarryResult<u64> {
        let (refusal, admission) = {
            let mut book = self.book();
            let refusal = book.page_opened(session_key, Instant::now());
            (refusal, book.admission_id(session_key))
        };
        match refusal {
            Ok(_) => Ok(admission.expect("a page was just reserved for this entry")),
            Err(PageRefusal::Cap { limit, active }) => Err(QuarryError::new(
                ErrorCode::RateLimited,
                "browser page capacity exhausted",
            )
            .with_details(json!({
                "reason": "browser_page_cap",
                "scope": "driver",
                "limit": limit,
                "active": active,
                "retry_after_seconds": crate::slots::BUSY_RETRY_AFTER_S,
            }))),
            Err(PageRefusal::NotAdmitted(Some(reason))) => Err(session_lost_error(reason)),
            Err(PageRefusal::NotAdmitted(None)) => Err(not_open_error()),
        }
    }

    /// `Some` when the session is no longer admitted.
    fn not_admitted_error(&self, session_key: &str) -> Option<QuarryError> {
        let book = self.book();
        if book.is_admitted(session_key) {
            return None;
        }
        Some(match book.loss(session_key, Instant::now()) {
            Some(reason) => session_lost_error(reason),
            None => not_open_error(),
        })
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
            Err(err) => tracing::debug!(error_class = crate::deadlines::cdp_class(&err), "console devtools listener unavailable"),
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
            Err(err) => tracing::debug!(error_class = crate::deadlines::cdp_class(&err), "log devtools listener unavailable"),
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
            Err(err) => tracing::debug!(error_class = crate::deadlines::cdp_class(&err), "request devtools listener unavailable"),
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
            Err(err) => tracing::debug!(error_class = crate::deadlines::cdp_class(&err), "response devtools listener unavailable"),
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
            Err(err) => tracing::debug!(error_class = crate::deadlines::cdp_class(&err), "lifecycle devtools listener unavailable"),
        }
    }

    /// The book entry `session`'s key is admitted under now. A call reads it
    /// first and passes it to `invalidate_live_frame_cache`, so a call that
    /// outlives its session never touches the session admitted after it.
    fn admission(&self, session: &BrowserSession) -> Option<u64> {
        self.book().admission_id(&Self::session_key(session))
    }

    /// The session's page changed: drop its cached frame, and make a capture
    /// that is still running for the old page unable to store itself.
    /// `owner` is the entry the calling operation started under.
    async fn invalidate_live_frame_cache(&self, session: &BrowserSession, owner: Option<u64>) {
        let session_key = Self::session_key(session);
        let epoch = self.frame_epochs.fetch_add(1, Ordering::SeqCst) + 1;
        let mut frames = self.live_frames.lock().await;
        // Read under the frames lock: a teardown releases the entry before it
        // removes the slot, so a slot made here is removed after.
        let current = self.book().admission_id(&session_key);
        frames.invalidate(&session_key, owner, current, epoch);
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
                tracing::warn!(error_class = err.code.wire_name(), "chromiumoxide profile load failed; starting fresh");
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
        let mut snapshot = with_deadline(Op::ProfileCapture, capture_snapshot(&page)).await?;
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
                tracing::warn!(error_class = crate::deadlines::cdp_class(&error), "chromiumoxide request guard resolution failed");
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

/// Whether Chromium runs with its own sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SandboxMode {
    Enabled,
    /// `QUARRY_BROWSER_NO_SANDBOX` was set outside production.
    DisabledByOperator,
}

/// The Chromium sandbox is the page-to-host boundary for every browser run, so
/// production never launches without it: a production environment with
/// `QUARRY_BROWSER_NO_SANDBOX` set is a launch error, not a warning. Elsewhere
/// the operator may opt out explicitly (old kernel, WSL2 without user
/// namespaces) and the launch logs it.
fn sandbox_policy(env_prod: bool, no_sandbox: bool) -> Result<SandboxMode, &'static str> {
    match (env_prod, no_sandbox) {
        (_, false) => Ok(SandboxMode::Enabled),
        (false, true) => Ok(SandboxMode::DisabledByOperator),
        (true, true) => Err("chromium sandbox cannot be disabled in production"),
    }
}

/// `ENVIRONMENT` (or `APP_ENV`) names a production environment.
fn is_production_env() -> bool {
    let value = std::env::var("ENVIRONMENT")
        .or_else(|_| std::env::var("APP_ENV"))
        .unwrap_or_default();
    let value = value.trim();
    value.eq_ignore_ascii_case("prod") || value.eq_ignore_ascii_case("production")
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

impl ChromiumLaunchDirs {
    /// Delete both directories (best effort: a retired browser has exited, or
    /// had five seconds to).
    fn remove(&self) {
        let _ = std::fs::remove_dir_all(&self.user_data);
        let _ = std::fs::remove_dir_all(&self.crash_dumps);
    }
}

fn chromium_profile_root() -> PathBuf {
    std::env::var_os("QUARRY_BROWSER_PROFILE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("quarry-chromium"))
}

fn chromium_launch_dirs() -> std::io::Result<ChromiumLaunchDirs> {
    let root = chromium_profile_root();
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
    mode: &InputMode,
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
    if let InputMode::Trusted(watch) = mode {
        if actionability::handles(&operation) {
            let outcome = actionability::run(page, &object_id, &operation, watch).await;
            // Same rule as the synthetic path: the handle never outlives the
            // action. Bounded, because a click that opened an `alert` has left
            // the renderer blocked inside the dialog: the release is queued
            // behind it and runs when the dialog is answered.
            let _ = tokio::time::timeout(
                Duration::from_millis(500),
                page.execute(ReleaseObjectParams::new(object_id)),
            )
            .await;
            return outcome;
        }
    }
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
    tokio::time::timeout(deadline(Op::Content), page.evaluate("document.title"))
        .await
        .ok()?
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
        tracing::warn!(error_class = crate::deadlines::cdp_class(&err), "chromiumoxide screencast stop failed");
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

/// Process default for a lease that asks for no viewport: `WxH`, for example
/// `1280x800`. Unset or unparsable means 1280x800.
const DEFAULT_VIEWPORT_ENV: &str = "QUARRY_BROWSER_DEFAULT_VIEWPORT";
/// `off` skips the two non-fatal page-hygiene calls (focus emulation and the
/// active lifecycle state). The viewport itself is always applied.
const PAGE_HYGIENE_ENV: &str = "QUARRY_BROWSER_PAGE_HYGIENE";
const FALLBACK_VIEWPORT: (u32, u32) = (1280, 800);

/// The viewport a page gets: the lease's own when it asked for one (an unset
/// device scale factor becomes 1), otherwise the process default.
fn resolve_viewport(requested: Option<BrowserViewport>) -> BrowserViewport {
    let configured = std::env::var(DEFAULT_VIEWPORT_ENV).ok();
    if requested.is_none() {
        if let Some(raw) = configured.as_deref() {
            if parse_dimensions(raw).is_none() {
                tracing::warn!("QUARRY_BROWSER_DEFAULT_VIEWPORT is not WxH; using 1280x800");
            }
        }
    }
    resolve_viewport_with(requested, configured.as_deref())
}

fn resolve_viewport_with(
    requested: Option<BrowserViewport>,
    configured: Option<&str>,
) -> BrowserViewport {
    if let Some(viewport) = requested {
        return BrowserViewport {
            device_scale_factor: if viewport.device_scale_factor > 0.0 {
                viewport.device_scale_factor
            } else {
                1.0
            },
            ..viewport
        };
    }
    let (width, height) = configured
        .and_then(parse_dimensions)
        .unwrap_or(FALLBACK_VIEWPORT);
    BrowserViewport {
        width,
        height,
        device_scale_factor: 1.0,
        is_mobile: false,
    }
}

fn parse_dimensions(raw: &str) -> Option<(u32, u32)> {
    let (width, height) = raw.trim().split_once(['x', 'X'])?;
    let width: u32 = width.trim().parse().ok()?;
    let height: u32 = height.trim().parse().ok()?;
    ((1..=7680).contains(&width) && (1..=4320).contains(&height)).then_some((width, height))
}

fn page_hygiene_enabled() -> bool {
    !matches!(
        std::env::var(PAGE_HYGIENE_ENV)
            .ok()
            .as_deref()
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref(),
        Some("off" | "0" | "false")
    )
}

fn viewport_emulation_error(error: impl std::fmt::Display) -> QuarryError {
    QuarryError::new(
        ErrorCode::DriverFailed,
        "chromiumoxide viewport emulation failed",
    )
    .with_details(json!({ "reason": "viewport_emulation_failed", "error": error.to_string() }))
}

/// Make a fresh page behave like a visible, focused window of `viewport`'s size.
///
/// Emulation is per page, and a headless Chromium with several contexts open
/// does not treat every page as foreground: without focus emulation and an
/// active lifecycle state a background page can stall timers, rendering and
/// screenshots. The size and touch calls are fatal (a page of the wrong size
/// gives wrong observations); the two hygiene calls only warn.
async fn apply_page_hygiene(
    page: &Page,
    viewport: BrowserViewport,
    hygiene: bool,
) -> QuarryResult<()> {
    page.execute(SetDeviceMetricsOverrideParams::new(
        i64::from(viewport.width),
        i64::from(viewport.height),
        viewport.device_scale_factor,
        viewport.is_mobile,
    ))
    .await
    .map_err(viewport_emulation_error)?;
    if viewport.is_mobile {
        page.execute(SetTouchEmulationEnabledParams::new(true))
            .await
            .map_err(viewport_emulation_error)?;
    }
    if hygiene {
        // Warn-only: the page works without these, a background page may stall.
        if page
            .execute(SetFocusEmulationEnabledParams::new(true))
            .await
            .is_err()
        {
            tracing::warn!("page focus emulation failed");
        }
        if page
            .execute(SetWebLifecycleStateParams::new(
                SetWebLifecycleStateState::Active,
            ))
            .await
            .is_err()
        {
            tracing::warn!("page lifecycle state could not be set to active");
        }
    }
    Ok(())
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
            trusted_input: self.trusted_input,
        }
    }

    async fn health(&self) -> BrowserHealth {
        let book = self.book();
        let slots = book
            .slots()
            .iter()
            .map(|slot| SlotHealth {
                slot: slot.id,
                status: slot.status.as_str().to_owned(),
                generation: slot.generation,
                sessions: slot.sessions,
                pages: slot.pages,
                healthy: slot.healthy,
                probe_failures: slot.probe_failures,
            })
            .collect();
        BrowserHealth::from_slots(book.config().max_sessions, book.sessions_open(), slots)
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
        // The driver's own cap comes after the edge's run caps (R-26).
        let session_key = lease.session_affinity_key.clone();
        self.wait_for_teardown(&session_key).await;
        let (admission, admission_id) = {
            let mut book = self.book();
            let admission = book.admit(
                &session_key,
                Duration::from_secs(u64::from(lease.ttl_s)),
                acquired_at,
            );
            (admission, book.admission_id(&session_key))
        };
        let needs_launch = match admission {
            Admission::Busy {
                limit,
                active,
                retry_after_s,
            } => {
                return Err(QuarryError::new(
                    ErrorCode::RateLimited,
                    "browser capacity exhausted",
                )
                .with_details(json!({
                    "reason": "browser_session_cap",
                    "scope": "driver",
                    "limit": limit,
                    "active": active,
                    "retry_after_seconds": retry_after_s,
                })));
            }
            Admission::Admitted { needs_launch, .. } => needs_launch,
        };
        // Given back if this call does not finish (an error, or the caller
        // dropping it during a cold launch) and no other acquire holds it.
        let mut rollback = AdmissionRollback::new(self, session_key.clone(), admission_id);
        self.start_background();
        let startup_mode = if self.browser.read().await.is_some() && !needs_launch {
            BrowserStartupMode::Warm
        } else {
            BrowserStartupMode::Cold
        };
        self.ensure_browser().await?;
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
        rollback.finish();
        Ok(session)
    }

    async fn release(&self, session: BrowserSession) -> QuarryResult<()> {
        if let Err(err) = self.persist_current_page(&session).await {
            tracing::warn!(error_class = err.code.wire_name(), "chromiumoxide profile capture skipped");
        }
        // Idempotent: a second release (WS2.D's finish_run releases a clone)
        // finds nothing left to free.
        self.teardown_session(&Self::session_key(&session)).await;
        Ok(())
    }

    async fn goto(&self, session: &BrowserSession, url: &str) -> QuarryResult<()> {
        let owner = self.admission(session);
        self.refuse_lost(session)?;
        guard_navigation_target(url).await?;
        self.require_configured_egress(session).await?;
        let (tab_id, page, created) = match self.active_tab_page(session).await {
            Some((tab_id, page)) => (tab_id, page, false),
            None => {
                let (tab_id, page) = self.open_tab_page(session, Some(url)).await?;
                (tab_id, page, true)
            }
        };

        if !created {
            with_deadline(Op::Navigate, async {
                page.goto(url).await.map_err(|e| {
                    QuarryError::new(ErrorCode::DriverFailed, "chromiumoxide goto failed")
                        .with_details(json!({ "error": e.to_string(), "url": url }))
                })
            })
            .await?;
            install_popup_bridge(&page).await?;
        }

        let session_key = Self::session_key(session);
        self.update_tab_metadata(&session_key, &tab_id, &page).await;
        #[cfg(test)]
        {
            if self.hooks.goto_hold.load(Ordering::SeqCst) {
                self.hooks.goto_reached.notify_one();
                self.hooks.goto_resume.notified().await;
            }
        }
        self.invalidate_live_frame_cache(session, owner).await;

        let mut inner = session.inner.lock().await;
        inner.pages_served = inner.pages_served.saturating_add(1);
        Ok(())
    }

    async fn content(&self, session: &BrowserSession) -> QuarryResult<Bytes> {
        let page = self.current_page(session).await?;
        let html = read_with_retry(Op::Content, || async {
            #[cfg(test)]
            {
                self.hooks.content_attempts.fetch_add(1, Ordering::SeqCst);
                let stale = self
                    .hooks
                    .content_stale_context
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .take();
                if let Some(context_id) = stale {
                    let error = content_in_stale_context(&page, context_id).await;
                    *self
                        .hooks
                        .content_first_error
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()) =
                        error.details.as_ref().map(|details| details.to_string());
                    return Err(error);
                }
            }
            page.content().await.map_err(|e| {
                QuarryError::new(ErrorCode::DriverFailed, "chromiumoxide content failed")
                    .with_details(json!({ "error": e.to_string() }))
            })
        })
        .await?;
        Ok(Bytes::from(html.into_bytes()))
    }

    async fn act_on_verified_target(
        &self,
        session: &BrowserSession,
        action: VerifiedTargetAction,
    ) -> QuarryResult<()> {
        let page = self.current_page(session).await?;
        if let Some(native_target) = action.native_target {
            // The frame lookup behind the mode is a CDP call too: with a
            // JavaScript dialog pending it never answers, so it runs under the
            // same bound as the action itself, not before it.
            return with_deadline(Op::NativeAction, async {
                let mode = self.input_mode_for(session, &page, &native_target).await;
                execute_native_target_action(&page, native_target, action.operation, &mode).await
            })
            .await;
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
        let result = with_deadline(Op::NativeAction, async {
            page.evaluate(script).await.map_err(|error| {
                QuarryError::new(ErrorCode::DriverFailed, "verified target action failed")
                    .with_details(json!({ "error": error.to_string() }))
            })
        })
        .await?;
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
        with_deadline(Op::NativeAction, async move {
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
    })
.await
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
            let browser_guard = self.browser.read().await;
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

        with_deadline(
            Op::NativeAction,
            execute_native_target_action(
                &page,
                target,
                VerifiedTargetOperation::Click,
                // The download is bound to this click's synthetic activation and
                // frame; real input is not part of the governed download path.
                &InputMode::Synthetic,
            ),
        )
        .await?;

        let began = tokio::time::timeout(deadline(Op::DownloadBegin), async {
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
        let completed_path = tokio::time::timeout(deadline(Op::DownloadComplete), async {
            while let Some(event) = progress.next().await {
                if event.guid != guid {
                    continue;
                }
                if event.received_bytes.is_finite()
                    && event.received_bytes > MAX_GOVERNED_DOWNLOAD_BYTES as f64
                {
                    if let Some(browser) = self.browser.read().await.as_ref() {
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
        let png = with_deadline(Op::Screenshot, async {
            page.screenshot(params).await.map_err(|e| {
                QuarryError::new(ErrorCode::DriverFailed, "chromiumoxide screenshot failed")
                    .with_details(json!({ "error": e.to_string(), "full_page": full_page }))
            })
        })
        .await?;
        Ok(Bytes::from(png))
    }

    async fn live_frame(
        &self,
        session: &BrowserSession,
        options: LiveFrameOptions,
    ) -> QuarryResult<LiveFrame> {
        let options = normalize_live_frame_options(options);
        let session_key = Self::session_key(session);
        // The entry this capture serves, read before its page is: a frame of
        // this page may only ever be cached for this entry.
        let owner = self.admission(session);
        let (tab_id, page) = self.current_tab_page(session).await?;
        #[cfg(test)]
        {
            let stall = *self
                .hooks
                .live_frame_stall
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(stall) = stall {
                self.hooks.live_frame_stalled.store(true, Ordering::SeqCst);
                tokio::time::sleep(stall).await;
            }
        }
        // Per session: one viewer's capture never waits for another session's.
        let (owner, page_epoch) = {
            let mut frames = self.live_frames.lock().await;
            // Released while this call looked for its page: its teardown has
            // removed the slot (or will, after this lock), so none is made.
            if let Some(error) = self.not_admitted_error(&session_key) {
                return Err(error);
            }
            // Released and admitted again: the key is a newer session's now.
            let current = self.book().admission_id(&session_key);
            let fresh_epoch = || self.frame_epochs.fetch_add(1, Ordering::SeqCst) + 1;
            match frames.begin(&session_key, owner, current, &tab_id, options, fresh_epoch) {
                FrameLookup::Hit(frame) => return Ok(frame),
                FrameLookup::Capture { owner, epoch } => (owner, epoch),
                FrameLookup::NotOwner => return Err(not_open_error()),
            }
        };

        let frame = capture_live_frame(&page, options).await?;
        // Stored only if the page did not change and the entry is the one
        // that started: never into a released session's slot or a newer one.
        self.live_frames.lock().await.store(
            &session_key,
            owner,
            CachedLiveFrame {
                tab_id,
                page_epoch,
                options,
                captured_at: Instant::now(),
                frame: frame.clone(),
            },
        );
        Ok(frame)
    }

    async fn pdf(&self, _session: &BrowserSession) -> QuarryResult<Bytes> {
        let page = self.current_page(_session).await?;
        let pdf = with_deadline(Op::Pdf, async {
            page.pdf(PrintToPdfParams::default()).await.map_err(|e| {
                QuarryError::new(ErrorCode::DriverFailed, "chromiumoxide pdf failed")
                    .with_details(json!({ "error": e.to_string() }))
            })
        })
        .await?;
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
        let owner = self.admission(session);
        self.refuse_lost(session)?;
        self.require_configured_egress(session).await?;
        let (tab_id, page) = self.open_tab_page(session, url).await?;
        let session_key = Self::session_key(session);
        self.update_tab_metadata(&session_key, &tab_id, &page).await;
        self.invalidate_live_frame_cache(session, owner).await;
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
        let owner = self.admission(session);
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
        self.invalidate_live_frame_cache(session, owner).await;
        Ok(())
    }

    async fn close_tab(&self, session: &BrowserSession, tab_id: &str) -> QuarryResult<()> {
        let owner = self.admission(session);
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
        #[cfg(test)]
        {
            if self.hooks.close_tab_hold.load(Ordering::SeqCst) {
                self.hooks.close_tab_reached.notify_one();
                self.hooks.close_tab_resume.notified().await;
            }
        }
        self.book().page_closed(&session_key, removed.admission);

        if let Err(err) = best_effort(Op::PageClose, removed.page.close()).await {
            tracing::debug!(
                tab_id = %removed.tab_id,
                error_class = crate::deadlines::cdp_class(&err),
                "chromiumoxide close_tab failed"
            );
        }
        if let Some(context_id) = empty_context {
            let browser_guard = self.browser.read().await;
            if let Some(browser) = browser_guard.as_ref() {
                if let Err(error) = best_effort(
                    Op::ContextClose,
                    browser.dispose_browser_context(context_id),
                )
                .await
                {
                    tracing::warn!(
                        session_key = %session_key,
                        error_class = crate::deadlines::cdp_class(&error),
                        "chromiumoxide dispose_browser_context after final tab close failed"
                    );
                }
            }
        }
        self.invalidate_live_frame_cache(session, owner).await;
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
        let (ax_tree, frame_tree) = read_with_retry(Op::AxSnapshot, || async {
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
            let frame_tree =
                page.execute(GetFrameTreeParams::default())
                    .await
                    .map_err(|error| {
                        QuarryError::new(
                            ErrorCode::DriverFailed,
                            "chromiumoxide frame tree capture failed",
                        )
                        .with_details(json!({ "error": error.to_string() }))
                    })?;
            Ok((ax_tree, frame_tree))
        })
        .await?;
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
        with_deadline(Op::Input, async {
            page.execute(params).await.map_err(|error| {
                QuarryError::new(
                    ErrorCode::DriverFailed,
                    "chromiumoxide dialog response failed",
                )
                .with_details(json!({ "error": error.to_string() }))
            })
        })
        .await?;
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
        // The caller's timeout, never longer than the table allows.
        let give_up_at = Instant::now()
            + Duration::from_millis(u64::from(timeout_ms)).min(deadline(Op::WaitFor));
        let poll = Duration::from_millis(50);
        loop {
            let found =
                tokio::time::timeout(deadline(Op::WaitForPoll), page.find_element(selector)).await;
            match found {
                Ok(Ok(_)) => return Ok(()),
                // Not there yet, or this poll stalled: look again until the time is up.
                Ok(Err(_)) | Err(_) if Instant::now() < give_up_at => {
                    tokio::time::sleep(poll).await
                }
                Ok(Err(e)) => {
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
                Err(_) => return Err(timeout_error(Op::WaitForPoll, deadline(Op::WaitForPoll))),
            }
        }
    }

    async fn click(&self, _session: &BrowserSession, selector: &str) -> QuarryResult<()> {
        let page = self.current_page(_session).await?;
        let element = with_deadline(Op::Input, async {
            page.find_element(selector).await.map_err(|e| {
                QuarryError::new(ErrorCode::DriverFailed, "click: element not found")
                    .with_details(json!({ "selector": selector, "error": e.to_string() }))
            })
        })
        .await?;
        with_deadline(Op::Input, async {
            element.click().await.map_err(|e| {
                QuarryError::new(ErrorCode::DriverFailed, "click failed")
                    .with_details(json!({ "selector": selector, "error": e.to_string() }))
            })
        })
        .await?;
        Ok(())
    }

    async fn click_point(&self, _session: &BrowserSession, x: f64, y: f64) -> QuarryResult<()> {
        let page = self.current_page(_session).await?;
        actionability::dispatch_click(&page, x, y).await
    }

    async fn type_text(
        &self,
        _session: &BrowserSession,
        selector: &str,
        text: &str,
    ) -> QuarryResult<()> {
        let page = self.current_page(_session).await?;
        let element = with_deadline(Op::Input, async {
            page.find_element(selector).await.map_err(|e| {
                QuarryError::new(ErrorCode::DriverFailed, "type: element not found")
                    .with_details(json!({ "selector": selector, "error": e.to_string() }))
            })
        })
        .await?;
        with_deadline(Op::Input, async {
            element.focus().await.map_err(|e| {
                QuarryError::new(ErrorCode::DriverFailed, "type: focus failed")
                    .with_details(json!({ "selector": selector, "error": e.to_string() }))
            })
        })
        .await?;
        with_deadline(Op::Input, async {
            element.type_str(text).await.map_err(|e| {
                QuarryError::new(ErrorCode::DriverFailed, "type failed")
                    .with_details(json!({ "selector": selector, "error": e.to_string() }))
            })
        })
        .await?;
        Ok(())
    }

    async fn scroll(&self, _session: &BrowserSession, target: &ScrollTarget) -> QuarryResult<()> {
        let page = self.current_page(_session).await?;
        match target {
            ScrollTarget::Top => {
                with_deadline(Op::Input, async {
                    page.evaluate("window.scrollTo(0, 0)").await.map_err(|e| {
                        QuarryError::new(ErrorCode::DriverFailed, "scroll-top failed")
                            .with_details(json!({ "error": e.to_string() }))
                    })
                })
                .await?;
            }
            ScrollTarget::Bottom => {
                with_deadline(Op::Input, async {
                    page.evaluate("window.scrollTo(0, document.documentElement.scrollHeight)")
                        .await
                        .map_err(|e| {
                            QuarryError::new(ErrorCode::DriverFailed, "scroll-bottom failed")
                                .with_details(json!({ "error": e.to_string() }))
                        })
                })
                .await?;
            }
            ScrollTarget::Selector(sel) => {
                let element = with_deadline(Op::Input, async {
                    page.find_element(sel).await.map_err(|e| {
                        QuarryError::new(ErrorCode::DriverFailed, "scroll: element not found")
                            .with_details(json!({ "selector": sel, "error": e.to_string() }))
                    })
                })
                .await?;
                with_deadline(Op::Input, async {
                    element.scroll_into_view().await.map_err(|e| {
                        QuarryError::new(ErrorCode::DriverFailed, "scroll_into_view failed")
                            .with_details(json!({ "selector": sel, "error": e.to_string() }))
                    })
                })
                .await?;
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

        with_deadline(Op::Input, async {
            page.execute(event).await.map_err(|e| {
                QuarryError::new(ErrorCode::DriverFailed, "mouse_wheel failed").with_details(
                    json!({
                        "x": x,
                        "y": y,
                        "delta_x": delta_x,
                        "delta_y": delta_y,
                        "error": e.to_string(),
                    }),
                )
            })
        })
        .await?;
        Ok(())
    }

    async fn press(&self, _session: &BrowserSession, key: &str) -> QuarryResult<()> {
        let page = self.current_page(_session).await?;
        // press_key is page-global; we use the body element only as a handle.
        let element = with_deadline(Op::Input, async {
            page.find_element("body").await.map_err(|e| {
                QuarryError::new(ErrorCode::DriverFailed, "press: body not found")
                    .with_details(json!({ "error": e.to_string() }))
            })
        })
        .await?;
        with_deadline(Op::Input, async {
            element.press_key(key).await.map_err(|e| {
                QuarryError::new(ErrorCode::DriverFailed, "press_key failed")
                    .with_details(json!({ "key": key, "error": e.to_string() }))
            })
        })
        .await?;
        Ok(())
    }

    async fn evaluate(
        &self,
        _session: &BrowserSession,
        script: &str,
    ) -> QuarryResult<serde_json::Value> {
        let page = self.current_page(_session).await?;
        let result = with_deadline(Op::Evaluate, async {
            page.evaluate(script).await.map_err(|e| {
                QuarryError::new(ErrorCode::DriverFailed, "evaluate failed")
                    .with_details(json!({ "error": e.to_string() }))
            })
        })
        .await?;
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
        with_deadline(Op::Input, async {
            page.evaluate(script).await.map_err(|e| {
                QuarryError::new(ErrorCode::DriverFailed, "select failed").with_details(
                    json!({ "selector": selector, "value": value, "error": e.to_string() }),
                )
            })
        })
        .await?;
        Ok(())
    }

    async fn back(&self, _session: &BrowserSession) -> QuarryResult<()> {
        let page = self.current_page(_session).await?;
        with_deadline(Op::Navigate, async {
            page.evaluate("window.history.back()").await.map_err(|e| {
                QuarryError::new(ErrorCode::DriverFailed, "back failed")
                    .with_details(json!({ "error": e.to_string() }))
            })
        })
        .await?;
        tokio::time::sleep(Duration::from_millis(100)).await;
        Ok(())
    }

    async fn forward(&self, _session: &BrowserSession) -> QuarryResult<()> {
        let page = self.current_page(_session).await?;
        with_deadline(Op::Navigate, async {
            page.evaluate("window.history.forward()")
                .await
                .map_err(|e| {
                    QuarryError::new(ErrorCode::DriverFailed, "forward failed")
                        .with_details(json!({ "error": e.to_string() }))
                })
        })
        .await?;
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
        // The background tasks hold views; only the driver itself tears down.
        if !self.owner {
            return;
        }
        // Best-effort: abort the event-loop task so it doesn't outlive the driver.
        if let Ok(mut guard) = self.handler.try_lock() {
            if let Some(h) = guard.take() {
                h.abort();
            }
        }
        for task in self
            .background
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .drain(..)
        {
            task.abort();
        }
        // Kill the browser now rather than whenever its handle is dropped, and
        // take its directories with it.
        if let Ok(mut guard) = self.browser.try_write() {
            if let Some(mut browser) = guard.take() {
                if let Some(child) = browser.get_mut_child() {
                    let _ = child.as_mut_inner().start_kill();
                }
            }
        }
        if let Ok(mut dirs) = self.launch_dirs.try_lock() {
            if let Some(dirs) = dirs.take() {
                dirs.remove();
            }
        }
    }
}

/// Test hooks: where a live test holds the driver still or bends one call.
#[cfg(test)]
#[derive(Default)]
struct TestHooks {
    /// Hold every tab attach this long after its page exists.
    attach_stall: std::sync::Mutex<Option<Duration>>,
    /// An attach reached its stall: `new_page` returned. (The page target
    /// shows up in `Target.getTargets` before that.)
    attach_stalled: AtomicBool,
    /// Hold every teardown this long after it took the session's pages.
    teardown_stall: std::sync::Mutex<Option<Duration>>,
    /// Hold `live_frame` this long after it found the page.
    live_frame_stall: std::sync::Mutex<Option<Duration>>,
    live_frame_stalled: AtomicBool,
    /// Hold `goto` after the page navigated and before it reports the page
    /// change: set `goto_hold`, wait for `goto_reached`, then let it go with
    /// `goto_resume`.
    goto_hold: AtomicBool,
    goto_reached: tokio::sync::Notify,
    goto_resume: tokio::sync::Notify,
    /// The same for `close_tab`, between taking the tab out of the session's
    /// pages and giving its page back to the slot book.
    close_tab_hold: AtomicBool,
    close_tab_reached: tokio::sync::Notify,
    close_tab_resume: tokio::sync::Notify,
    /// The next `content` read's first attempt evaluates in this execution
    /// context (one from before a navigation), as the race does.
    content_stale_context:
        std::sync::Mutex<Option<::chromiumoxide::cdp::js_protocol::runtime::ExecutionContextId>>,
    /// Every attempt `content` made, retries included.
    content_attempts: AtomicUsize,
    /// The error details of the stale attempt.
    content_first_error: std::sync::Mutex<Option<String>>,
    /// Every failed probe's code and message, oldest first.
    probe_errors: std::sync::Mutex<Vec<(ErrorCode, String)>>,
}

/// The read `content` makes, in a given (stale) execution context: Chromium's
/// own answer to a context that a navigation destroyed.
#[cfg(test)]
async fn content_in_stale_context(
    page: &Page,
    context_id: ::chromiumoxide::cdp::js_protocol::runtime::ExecutionContextId,
) -> QuarryError {
    use ::chromiumoxide::cdp::js_protocol::runtime::EvaluateParams;
    let params = match EvaluateParams::builder()
        .expression("document.documentElement.outerHTML")
        .context_id(context_id)
        .return_by_value(true)
        .build()
    {
        Ok(params) => params,
        Err(error) => {
            return QuarryError::new(ErrorCode::Internal, "content params")
                .with_details(json!({ "error": error }))
        }
    };
    match page.execute(params).await {
        Ok(_) => QuarryError::new(ErrorCode::Internal, "the stale context answered"),
        Err(error) => QuarryError::new(ErrorCode::DriverFailed, "chromiumoxide content failed")
            .with_details(json!({ "error": error.to_string() })),
    }
}

/// Tells the book how an `acquire` of one admitted entry ended. Finished:
/// the entry is someone's session. Not finished (an error, or the caller
/// dropping the future during a cold launch, which a scrape's own timeout
/// does): the book gives the entry back only when no acquire of it ever
/// finished and no other one is still running, so a second acquire that
/// joined a launching first one keeps it when the first is dropped.
struct AdmissionRollback {
    book: Arc<std::sync::Mutex<SlotBook>>,
    session_key: String,
    /// The entry this acquire admitted or joined; `None` once settled.
    admission_id: Option<u64>,
}

impl AdmissionRollback {
    fn new(driver: &ChromiumoxideDriver, session_key: String, admission_id: Option<u64>) -> Self {
        Self {
            book: driver.book.clone(),
            session_key,
            admission_id,
        }
    }

    fn finish(&mut self) {
        if let Some(id) = self.admission_id.take() {
            self.book
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .acquire_finished(&self.session_key, id);
        }
    }
}

impl Drop for AdmissionRollback {
    fn drop(&mut self) {
        if let Some(id) = self.admission_id.take() {
            self.book
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .acquire_abandoned(&self.session_key, id);
        }
    }
}

/// Closes what a tab attach (or a probe) created when it does not finish:
/// an error, a deadline, or the caller dropping the future. Only what this
/// call created: a context that existed before is the session's and stays.
struct AttachGuard {
    book: Arc<std::sync::Mutex<SlotBook>>,
    browser: Arc<RwLock<Option<Browser>>>,
    session_key: String,
    /// The book entry whose page reservation this guard gives back.
    reservation: Option<u64>,
    page: Option<Page>,
    context: Option<BrowserContextId>,
    download_dir: Option<Arc<tempfile::TempDir>>,
    armed: bool,
}

impl AttachGuard {
    fn new(driver: &ChromiumoxideDriver, session_key: String, reservation: Option<u64>) -> Self {
        Self {
            book: driver.book.clone(),
            browser: driver.browser.clone(),
            session_key,
            reservation,
            page: None,
            context: None,
            download_dir: None,
            armed: true,
        }
    }

    /// The attach finished: everything it made now belongs to the session.
    fn disarm(&mut self) {
        self.armed = false;
    }

    /// Close the page and dispose the context now, and disarm.
    async fn close_now(&mut self, driver: &ChromiumoxideDriver) {
        if let Some(page) = self.page.take() {
            let _ = best_effort(Op::PageClose, page.close()).await;
        }
        if let Some(context_id) = self.context.take() {
            let browser_guard = driver.browser.read().await;
            if let Some(browser) = browser_guard.as_ref() {
                let _ = best_effort(
                    Op::ContextClose,
                    browser.dispose_browser_context(context_id),
                )
                .await;
            }
        }
        if let Some(id) = self.reservation {
            self.book
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .page_closed(&self.session_key, id);
        }
        self.armed = false;
    }
}

impl Drop for AttachGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        if let Some(id) = self.reservation {
            self.book
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .page_closed(&self.session_key, id);
        }
        let page = self.page.take();
        let context = self.context.take();
        let download_dir = self.download_dir.take();
        if page.is_none() && context.is_none() {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let browser = self.browser.clone();
        runtime.spawn(async move {
            if let Some(page) = page {
                if best_effort(Op::PageClose, page.close()).await.is_err() {
                    tracing::debug!("abandoned tab attach: page close failed");
                }
            }
            if let Some(context_id) = context {
                let browser_guard = browser.read().await;
                if let Some(browser) = browser_guard.as_ref() {
                    if best_effort(
                        Op::ContextClose,
                        browser.dispose_browser_context(context_id),
                    )
                    .await
                    .is_err()
                    {
                        tracing::debug!("abandoned tab attach: context dispose failed");
                    }
                }
            }
            // The quarantine directory goes after the context that used it.
            drop(download_dir);
        });
    }
}

fn browser_missing() -> QuarryError {
    QuarryError::new(
        ErrorCode::DriverFailed,
        "chromiumoxide: browser not initialized",
    )
}

/// A session that was reaped or whose browser died. Not retryable: the run has
/// to start a new session (or resume), repeating the call cannot help.
fn session_lost_error(reason: LossReason) -> QuarryError {
    let message = match reason {
        LossReason::Reaped(_) => "browser session was closed after being idle too long",
        LossReason::ProcessLost => "browser process was lost; start a new session",
    };
    let mut error = QuarryError::new(ErrorCode::DriverFailed, message)
        .with_details(json!({ "reason": reason.as_str() }));
    error.retryable = false;
    error
}

/// A session this driver does not hold (released, or never acquired).
fn not_open_error() -> QuarryError {
    let mut error = QuarryError::new(ErrorCode::DriverFailed, "browser session is not open")
        .with_details(json!({ "reason": "session_not_open" }));
    error.retryable = false;
    error
}

/// Remove `<root>/{profiles,crashpad}/edge-<pid>-*` left by a process that is
/// gone (Linux: no `/proc/<pid>`). Our own pid's directories may belong to
/// another driver in this process, and elsewhere liveness is unknown: both are
/// left alone.
fn sweep_stale_launch_dirs() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let root = chromium_profile_root();
    let ours = std::process::id();
    let mut removed = 0usize;
    for sub in ["profiles", "crashpad"] {
        let Ok(entries) = std::fs::read_dir(root.join(sub)) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(pid) = name
                .to_str()
                .and_then(|name| name.strip_prefix("edge-"))
                .and_then(|rest| rest.split('-').next())
                .and_then(|pid| pid.parse::<u32>().ok())
            else {
                continue;
            };
            if pid == ours || Path::new(&format!("/proc/{pid}")).exists() {
                continue;
            }
            if std::fs::remove_dir_all(entry.path()).is_ok() {
                removed += 1;
            }
        }
    }
    if removed > 0 {
        tracing::info!(removed, "removed stale browser launch directories");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PinnedEgressProxyEndpoint;

    #[test]
    fn sandbox_is_on_by_default_everywhere() {
        assert_eq!(sandbox_policy(false, false), Ok(SandboxMode::Enabled));
        assert_eq!(sandbox_policy(true, false), Ok(SandboxMode::Enabled));
    }

    #[test]
    fn no_sandbox_is_refused_in_production_env() {
        let error = sandbox_policy(true, true).expect_err("production must keep the sandbox");
        assert!(error.contains("production"), "{error}");
    }

    #[test]
    fn no_sandbox_allowed_in_dev_env_with_warning() {
        assert_eq!(
            sandbox_policy(false, true),
            Ok(SandboxMode::DisabledByOperator)
        );
    }

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
    fn resolve_viewport_defaults_to_1280x800() {
        let viewport = resolve_viewport_with(None, None);
        assert_eq!((viewport.width, viewport.height), (1280, 800));
        assert_eq!(viewport.device_scale_factor, 1.0);
        assert!(!viewport.is_mobile);
        // Garbage in the variable is not a viewport either.
        for bad in ["", "wide", "0x800", "1280x0", "99999x800", "1280", "-1x5"] {
            let viewport = resolve_viewport_with(None, Some(bad));
            assert_eq!((viewport.width, viewport.height), (1280, 800), "{bad:?}");
        }
    }

    #[test]
    fn resolve_viewport_honours_env_override() {
        let viewport = resolve_viewport_with(None, Some("800x600"));
        assert_eq!((viewport.width, viewport.height), (800, 600));
        let viewport = resolve_viewport_with(None, Some(" 1920X1080 "));
        assert_eq!((viewport.width, viewport.height), (1920, 1080));
    }

    #[test]
    fn resolve_viewport_passes_through_lease_viewport() {
        let phone = BrowserViewport {
            width: 390,
            height: 844,
            device_scale_factor: 2.0,
            is_mobile: true,
        };
        // The env default never overrides what a lease asked for.
        assert_eq!(resolve_viewport_with(Some(phone), Some("800x600")), phone);
        // An unset scale factor (the serde default is 0) means 1.
        let unset = BrowserViewport {
            device_scale_factor: 0.0,
            ..phone
        };
        assert_eq!(
            resolve_viewport_with(Some(unset), None).device_scale_factor,
            1.0
        );
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

    /// Minimal stand-in for Quarry's real DNS-pinning egress authority: it
    /// hands out a fixed endpoint and records nothing. The deny-all egress
    /// policy installed below is what keeps the page from ever reaching the
    /// network under test.
    ///
    /// It exists so this crate can exercise `require_configured_egress`
    /// without importing `quarry_runtime::PinnedBrowserEgressProxy`: that
    /// would be a circular dependency, because quarry-runtime depends on this
    /// crate.
    struct StubEgressProvider;

    #[async_trait::async_trait]
    impl BrowserEgressProxyProvider for StubEgressProvider {
        async fn endpoint_for_session(
            &self,
            _session_key: &str,
        ) -> QuarryResult<PinnedEgressProxyEndpoint> {
            // Loopback shape required by the endpoint parser; never actually
            // contacted because the CDP Fetch guard blocks every request.
            PinnedEgressProxyEndpoint::parse("http://127.0.0.1:9")
        }

        async fn receipts_after(
            &self,
            _session_key: &str,
            _after_sequence: u64,
            _limit: usize,
        ) -> QuarryResult<Vec<BrowserEgressReceipt>> {
            Ok(Vec::new())
        }

        async fn configure_policy(
            &self,
            _session_key: &str,
            _policy: BrowserEgressPolicy,
        ) -> QuarryResult<()> {
            Ok(())
        }

        async fn release_session(&self, _session_key: &str) {}
    }

    /// A driver wired the way production wires it, with the stub provider in
    /// place of the pinned proxy (see `StubEgressProvider`).
    fn live_driver() -> ChromiumoxideDriver {
        ChromiumoxideDriver::new().with_pinned_egress_proxy(Arc::new(StubEgressProvider))
    }

    /// The shared gate: skips without `CHROMIUMOXIDE_TEST=1`, and FAILS (panics)
    /// with it when no Chromium can be found. See `crate::live_gate`.
    fn live_enabled() -> bool {
        crate::live_gate::live_enabled()
    }

    /// A session for a lease that asks for `viewport`, with a blank tab holding
    /// `marker` in its document. Returns the session.
    async fn live_session_sized(
        driver: &ChromiumoxideDriver,
        key: &str,
        viewport: Option<BrowserViewport>,
        marker: &str,
    ) -> BrowserSession {
        let mut lease = make_lease_with_key(key);
        lease.viewport = viewport;
        let session = driver.acquire(&lease).await.expect("acquire");
        driver
            .configure_egress_policy(&session, BrowserEgressPolicy::deny_all())
            .await
            .expect("install deny-all egress policy");
        driver.new_tab(&session, None).await.expect("blank tab");
        let page = driver.current_page(&session).await.expect("current page");
        // A mobile page only lays out at the device width when it asks to, so the
        // fixture says `width=device-width` like any mobile-ready page does.
        page.set_content(format!(
            "<html><head><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"></head><body><p id=\"m\">{marker}</p></body></html>"
        ))
            .await
            .expect("inject page");
        session
    }

    async fn eval_number(driver: &ChromiumoxideDriver, session: &BrowserSession, js: &str) -> i64 {
        let page = driver.current_page(session).await.expect("current page");
        page.evaluate(js)
            .await
            .expect("evaluate")
            .into_value::<i64>()
            .expect("number")
    }

    async fn eval_string(
        driver: &ChromiumoxideDriver,
        session: &BrowserSession,
        js: &str,
    ) -> String {
        let page = driver.current_page(session).await.expect("current page");
        page.evaluate(js)
            .await
            .expect("evaluate")
            .into_value::<String>()
            .expect("string")
    }

    async fn live_marker(driver: &ChromiumoxideDriver, session: &BrowserSession) -> String {
        eval_string(driver, session, "document.getElementById('m').textContent").await
    }

    fn desktop() -> BrowserViewport {
        BrowserViewport {
            width: 1280,
            height: 800,
            device_scale_factor: 1.0,
            is_mobile: false,
        }
    }

    fn phone() -> BrowserViewport {
        BrowserViewport {
            width: 390,
            height: 844,
            device_scale_factor: 2.0,
            is_mobile: true,
        }
    }

    /// Leases with different viewports share one browser: nobody's pages are
    /// lost, each page has its own size, and each is focused and visible.
    #[tokio::test]
    async fn concurrent_leases_with_different_viewports_do_not_lose_pages() {
        if !live_enabled() {
            return;
        }
        let driver = live_driver();
        let a = live_session_sized(&driver, "vp-a", Some(desktop()), "marker-a").await;
        let b = live_session_sized(&driver, "vp-b", None, "marker-b").await;
        let c = live_session_sized(&driver, "vp-c", Some(phone()), "marker-c").await;

        assert_eq!(driver.launch_count(), 1, "one browser for all three leases");
        assert_eq!(live_marker(&driver, &a).await, "marker-a");
        assert_eq!(live_marker(&driver, &b).await, "marker-b");
        assert_eq!(live_marker(&driver, &c).await, "marker-c");
        assert_eq!(eval_number(&driver, &a, "window.innerWidth").await, 1280);
        assert_eq!(eval_number(&driver, &b, "window.innerWidth").await, 1280);
        assert_eq!(eval_number(&driver, &c, "window.innerWidth").await, 390);
        for session in [&a, &b, &c] {
            assert_eq!(
                eval_string(&driver, session, "document.visibilityState").await,
                "visible"
            );
            assert_eq!(
                eval_string(&driver, session, "String(document.hasFocus())").await,
                "true"
            );
        }

        // Releasing one session leaves the others alone.
        driver.release(b).await.expect("release b");
        assert_eq!(live_marker(&driver, &a).await, "marker-a");
        assert_eq!(live_marker(&driver, &c).await, "marker-c");
        assert_eq!(driver.launch_count(), 1);
        driver.release(a).await.expect("release a");
        driver.release(c).await.expect("release c");
    }

    /// A script that never finishes gets the typed timeout for `evaluate`, says
    /// the effect is unknown, is not retryable, and leaves the session usable.
    #[tokio::test]
    async fn a_hung_evaluate_times_out_with_a_typed_error_and_the_session_survives() {
        if !live_enabled() {
            return;
        }
        let driver = live_driver();
        let session = live_session_sized(&driver, "hung-eval", None, "still-here").await;
        let started = Instant::now();
        let error = driver
            .evaluate(&session, "new Promise(() => {})")
            .await
            .expect_err("a promise that never settles must time out");
        assert_eq!(error.code, ErrorCode::Timeout, "{error:?}");
        assert!(!error.retryable);
        let details = error.details.expect("timeout details");
        assert_eq!(details["op"], "evaluate");
        assert_eq!(details["effect"], "unknown");
        assert!(
            !details.to_string().contains("Promise"),
            "no script text in the details"
        );
        assert!(
            started.elapsed() >= Duration::from_secs(14),
            "waited for the deadline"
        );
        // The page still answers.
        assert_eq!(live_marker(&driver, &session).await, "still-here");
        driver.release(session).await.expect("release");
    }

    /// The order leases arrive in does not matter: the phone first, then the
    /// desktop, then the default.
    #[tokio::test]
    async fn acquire_order_does_not_matter() {
        if !live_enabled() {
            return;
        }
        let driver = live_driver();
        let c = live_session_sized(&driver, "ord-c", Some(phone()), "marker-c").await;
        let a = live_session_sized(&driver, "ord-a", Some(desktop()), "marker-a").await;
        let b = live_session_sized(&driver, "ord-b", None, "marker-b").await;

        assert_eq!(driver.launch_count(), 1);
        assert_eq!(live_marker(&driver, &c).await, "marker-c");
        assert_eq!(live_marker(&driver, &a).await, "marker-a");
        assert_eq!(live_marker(&driver, &b).await, "marker-b");
        assert_eq!(eval_number(&driver, &c, "window.innerWidth").await, 390);
        assert_eq!(eval_number(&driver, &a, "window.innerWidth").await, 1280);
        assert_eq!(eval_number(&driver, &b, "window.innerWidth").await, 1280);
        for session in [a, b, c] {
            driver.release(session).await.expect("release");
        }
    }

    // ---- live-frame cache ownership (no browser needed) ----

    fn cached_frame(page_epoch: u64, data: &str) -> CachedLiveFrame {
        CachedLiveFrame {
            tab_id: "tab-1".into(),
            page_epoch,
            options: LiveFrameOptions::default(),
            captured_at: Instant::now(),
            frame: LiveFrame {
                mime_type: "image/jpeg".into(),
                data_base64: data.into(),
            },
        }
    }

    fn capture(lookup: FrameLookup) -> (u64, u64) {
        match lookup {
            FrameLookup::Capture { owner, epoch } => (owner, epoch),
            other => panic!("expected a capture, got {other:?}"),
        }
    }

    /// A key released and admitted again is a different session. A capture
    /// of the old session's page (its call read its entry before the
    /// release) can neither read nor fill the new session's slot, so the old
    /// session's frame never outlives it under the reused key.
    #[test]
    fn live_frame_cache_keeps_a_released_sessions_frame_out_of_the_next_session() {
        let options = LiveFrameOptions::default();
        let mut cache = LiveFrameCache::default();
        // Entry 1 caches a frame; its release removes the slot.
        let (owner, epoch) = capture(cache.begin("k", Some(1), Some(1), "tab-1", options, || 10));
        assert!(cache.store("k", owner, cached_frame(epoch, "old-session")));
        cache.remove("k");
        // Entry 2 is admitted under the same key and starts a capture.
        let (_, new_epoch) = capture(cache.begin("k", Some(2), Some(2), "tab-1", options, || 20));
        // A late call of entry 1 is refused...
        assert!(matches!(
            cache.begin("k", Some(1), Some(2), "tab-1", options, || 21),
            FrameLookup::NotOwner
        ));
        // ...and its frame is not stored under entry 2, even at the new epoch.
        assert!(!cache.store("k", 1, cached_frame(new_epoch, "old-session")));
        assert!(matches!(
            cache.begin("k", Some(2), Some(2), "tab-1", options, || 22),
            FrameLookup::Capture { owner: 2, .. }
        ));
        // A slot an earlier entry left behind is never served to a newer one.
        let mut cache = LiveFrameCache::default();
        let (owner, epoch) = capture(cache.begin("k", Some(1), Some(1), "tab-1", options, || 30));
        assert!(cache.store("k", owner, cached_frame(epoch, "old-session")));
        assert!(matches!(
            cache.begin("k", Some(2), Some(2), "tab-1", options, || 31),
            FrameLookup::Capture { owner: 2, epoch: 31 }
        ));
    }

    /// A page change that an operation of the old session reports late (its
    /// goto finished after the release and the next admission) leaves the
    /// newer session's slot alone; the session's own change drops its frame;
    /// a released key gets no slot back.
    #[test]
    fn live_frame_cache_invalidation_is_owned_by_the_entry_that_changed_the_page() {
        let options = LiveFrameOptions::default();
        let mut cache = LiveFrameCache::default();
        let (owner, epoch) = capture(cache.begin("k", Some(2), Some(2), "tab-1", options, || 40));
        assert!(cache.store("k", owner, cached_frame(epoch, "new-session")));

        cache.invalidate("k", Some(1), Some(2), 41);
        match cache.begin("k", Some(2), Some(2), "tab-1", options, || 42) {
            FrameLookup::Hit(frame) => assert_eq!(frame.data_base64, "new-session"),
            other => panic!("the newer session's frame was dropped: {other:?}"),
        }

        cache.invalidate("k", Some(2), Some(2), 43);
        assert!(matches!(
            cache.begin("k", Some(2), Some(2), "tab-1", options, || 44),
            FrameLookup::Capture { owner: 2, epoch: 43 }
        ));

        cache.invalidate("k", Some(2), None, 45);
        assert!(!cache.contains_key("k"), "a released key gets no slot back");
        assert!(matches!(
            cache.begin("k", Some(2), None, "tab-1", options, || 46),
            FrameLookup::NotOwner
        ));
        assert!(!cache.contains_key("k"));
    }


    // ---- WS3.F: slot book, crash, reaper, probe; WS3.D: attach cleanup ----

    /// A pool config without background tasks unless a test asks for them.
    fn quiet_pool() -> BrowserPoolConfig {
        BrowserPoolConfig {
            reaper: ReaperMode::Off,
            // The prober runs even with the reaper off; an hour keeps it out
            // of the way, tests call `probe_tick` themselves.
            probe_interval: Duration::from_secs(3_600),
            ..BrowserPoolConfig::default()
        }
    }

    /// Browser contexts the running browser holds.
    async fn context_count(driver: &ChromiumoxideDriver) -> usize {
        use ::chromiumoxide::cdp::browser_protocol::target::GetBrowserContextsParams;
        // Bounded: only a launch or a kill holds the write side. (Before the
        // browser handle became an RwLock, an attach's `new_page` held the
        // whole lock for up to 4.2 s under the parallel live suite, and these
        // reads timed out.)
        let guard = tokio::time::timeout(Duration::from_secs(5), driver.browser.read())
            .await
            .expect("the browser lock is free within 5 s");
        let browser = guard.as_ref().expect("a running browser");
        browser
            .execute(GetBrowserContextsParams::default())
            .await
            .expect("Target.getBrowserContexts")
            .result
            .browser_context_ids
            .len()
    }

    /// Page targets the running browser holds.
    async fn page_target_count(driver: &ChromiumoxideDriver) -> usize {
        use ::chromiumoxide::cdp::browser_protocol::target::GetTargetsParams;
        // Bounded: only a launch or a kill holds the write side.
        let guard = tokio::time::timeout(Duration::from_secs(5), driver.browser.read())
            .await
            .expect("the browser lock is free within 5 s");
        let browser = guard.as_ref().expect("a running browser");
        browser
            .execute(GetTargetsParams::default())
            .await
            .expect("Target.getTargets")
            .result
            .target_infos
            .iter()
            .filter(|target| target.r#type == "page")
            .count()
    }

    /// Poll `check` every 100 ms for up to `limit`.
    async fn eventually<F, Fut>(limit: Duration, mut check: F) -> bool
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = bool>,
    {
        let started = Instant::now();
        while started.elapsed() < limit {
            if check().await {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        check().await
    }

    fn loss_reason(error: &QuarryError) -> String {
        error
            .details
            .as_ref()
            .and_then(|details| details["reason"].as_str())
            .unwrap_or_default()
            .to_owned()
    }

    /// Killing Chromium is noticed by the CDP handler: the slot is dead, the
    /// old session's next call says `browser_process_lost` (not retryable),
    /// and the next acquire launches a new browser that works.
    #[tokio::test]
    async fn crash_is_detected_and_next_acquire_relaunches() {
        if !live_enabled() {
            return;
        }
        let driver = live_driver().with_pool_config(quiet_pool());
        let old = live_session_sized(&driver, "crash-old", None, "before").await;
        assert_eq!(driver.launch_count(), 1);
        assert_eq!(driver.book().slot(SLOT).unwrap().generation, 1);
        {
            let mut guard = driver.browser.write().await;
            let browser = guard.as_mut().expect("a running browser");
            let _ = browser.kill().await;
        }
        assert!(
            eventually(Duration::from_secs(20), || async {
                driver
                    .book()
                    .slot(SLOT)
                    .is_some_and(|slot| slot.status == SlotStatus::Dead)
            })
            .await,
            "the handler task marks the slot dead"
        );
        assert_eq!(driver.health().await.state, crate::BrowserHealthState::Unhealthy);
        assert_eq!(driver.health().await.sessions_open, 0);

        let error = driver.content(&old).await.expect_err("the old page is gone");
        assert_eq!(loss_reason(&error), "browser_process_lost", "{error:?}");
        assert!(!error.retryable);
        let error = driver
            .goto(&old, "about:blank")
            .await
            .expect_err("a lost session never opens a fresh tab");
        assert_eq!(loss_reason(&error), "browser_process_lost", "{error:?}");
        // Releasing the lost session is fine, twice.
        driver.release(old.clone()).await.expect("release lost");
        driver.release(old).await.expect("release lost again");

        let fresh = live_session_sized(&driver, "crash-new", None, "after").await;
        assert_eq!(driver.launch_count(), 2, "relaunched on the next acquire");
        assert_eq!(driver.book().slot(SLOT).unwrap().generation, 2);
        assert_eq!(live_marker(&driver, &fresh).await, "after");
        assert_eq!(driver.health().await.state, crate::BrowserHealthState::Healthy);
        driver.release(fresh).await.expect("release fresh");
    }

    /// With the reaper on, a session idle past `max(2 x ttl, idle)` is torn
    /// down (its context is disposed), tombstoned as `session_reaped`, and a
    /// busy session next to it is left alone.
    #[tokio::test]
    async fn reaper_closes_abandoned_session_and_tombstones_it() {
        if !live_enabled() {
            return;
        }
        let driver = live_driver().with_pool_config(BrowserPoolConfig {
            session_idle: Duration::from_secs(1),
            reaper_interval: Duration::from_millis(200),
            probe_interval: Duration::from_secs(3_600),
            reaper: ReaperMode::On,
            ..BrowserPoolConfig::default()
        });
        let keeper = live_session_sized(&driver, "reap-keeper", None, "kept").await;
        let mut lease = make_lease_with_key("reap-abandoned");
        // 5 s, so a 10 s idle floor: long enough to set the session up on a
        // loaded machine, short enough for a test.
        lease.ttl_s = 5;
        let abandoned = driver.acquire(&lease).await.expect("acquire");
        driver
            .configure_egress_policy(&abandoned, BrowserEgressPolicy::deny_all())
            .await
            .expect("policy");
        driver.new_tab(&abandoned, None).await.expect("blank tab");
        let with_both = context_count(&driver).await;
        assert_eq!(driver.health().await.sessions_open, 2);

        assert!(
            eventually(Duration::from_secs(30), || async {
                !driver.book().is_admitted("reap-abandoned")
            })
            .await,
            "the abandoned session is reaped within max(2 x 5 s, 1 s) plus a tick"
        );
        assert_eq!(
            driver.book().loss("reap-abandoned", Instant::now()),
            Some(LossReason::Reaped(crate::slots::ReapReason::Idle))
        );
        assert!(
            eventually(Duration::from_secs(5), || async {
                context_count(&driver).await == with_both - 1
            })
            .await,
            "its browser context is disposed"
        );
        assert!(!driver.pages.lock().await.contains_key("reap-abandoned"));
        let error = driver
            .content(&abandoned)
            .await
            .expect_err("a reaped session has no page");
        assert_eq!(loss_reason(&error), "session_reaped", "{error:?}");
        assert!(!error.retryable);
        let error = driver
            .new_tab(&abandoned, None)
            .await
            .expect_err("a reaped session never opens a fresh tab");
        assert_eq!(loss_reason(&error), "session_reaped", "{error:?}");

        // The keeper (ttl 30 s, so a 60 s floor) is untouched.
        assert!(driver.book().is_admitted("reap-keeper"));
        assert_eq!(live_marker(&driver, &keeper).await, "kept");
        assert_eq!(driver.health().await.sessions_open, 1);
        driver.release(abandoned).await.expect("release reaped");
        driver.release(keeper).await.expect("release keeper");
    }

    /// Records what the driver asks of the egress authority, in order.
    #[derive(Default)]
    struct RecordingEgressProvider {
        calls: std::sync::Mutex<Vec<String>>,
    }

    impl RecordingEgressProvider {
        fn take(&self) -> Vec<String> {
            std::mem::take(&mut *self.calls.lock().unwrap())
        }
    }

    #[async_trait::async_trait]
    impl BrowserEgressProxyProvider for RecordingEgressProvider {
        async fn endpoint_for_session(
            &self,
            session_key: &str,
        ) -> QuarryResult<PinnedEgressProxyEndpoint> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("endpoint {session_key}"));
            PinnedEgressProxyEndpoint::parse("http://127.0.0.1:9")
        }

        async fn receipts_after(
            &self,
            _session_key: &str,
            _after_sequence: u64,
            _limit: usize,
        ) -> QuarryResult<Vec<BrowserEgressReceipt>> {
            Ok(Vec::new())
        }

        async fn configure_policy(
            &self,
            session_key: &str,
            policy: BrowserEgressPolicy,
        ) -> QuarryResult<()> {
            let kind = if policy == BrowserEgressPolicy::deny_all() {
                "deny_all"
            } else {
                "other"
            };
            self.calls
                .lock()
                .unwrap()
                .push(format!("policy {session_key} {kind}"));
            Ok(())
        }

        async fn release_session(&self, session_key: &str) {
            self.calls
                .lock()
                .unwrap()
                .push(format!("release {session_key}"));
        }
    }

    /// The probe opens and closes a page in a throwaway context bound to the
    /// pinned proxy under a deny-all policy (installed before the endpoint is
    /// asked for), and leaves every session's pages, policies, receipts and
    /// events exactly as they were.
    #[tokio::test]
    async fn probe_does_not_touch_session_state_or_egress() {
        if !live_enabled() {
            return;
        }
        let provider = Arc::new(RecordingEgressProvider::default());
        let driver = ChromiumoxideDriver::new()
            .with_pinned_egress_proxy(provider.clone())
            .with_pool_config(quiet_pool());
        let session = live_session_sized(&driver, "probe-session", None, "untouched").await;
        let contexts_before = context_count(&driver).await;
        let tabs_before: Vec<String> = driver
            .pages
            .lock()
            .await
            .get("probe-session")
            .map(|pages| pages.tabs.iter().map(|tab| tab.tab_id.clone()).collect())
            .unwrap_or_default();
        let mut policy_keys_before: Vec<String> =
            driver.egress_policies.lock().await.keys().cloned().collect();
        policy_keys_before.sort();
        let receipts_before = driver.egress_receipts.lock().await.len();
        let events_before = driver.devtools_events.lock().await.len();
        let pages_book_before = driver.book().slot(SLOT).unwrap().pages;
        provider.take();

        // The probe's own 3 s deadline (`Op::HealthProbe`) can run out when
        // twenty live tests share the machine; that failure is the prober
        // working, not the property under test. Up to three probes, each with
        // the same egress sequence; any failure other than a timeout fails.
        let mut probes = 0;
        while probes < 3 {
            probes += 1;
            driver.probe_tick().await;
            assert_eq!(
                provider.take(),
                vec![
                    "policy probe:0 deny_all".to_owned(),
                    "endpoint probe:0".to_owned(),
                    "release probe:0".to_owned(),
                ],
                "deny-all first, then the pinned endpoint, then released (probe {probes})"
            );
            if driver.book().slot(SLOT).unwrap().healthy == Some(true) {
                break;
            }
        }
        let probe_errors = driver.hooks.probe_errors.lock().unwrap().clone();
        if !probe_errors.is_empty() {
            eprintln!("probe_does_not_touch_session_state_or_egress: failed probes {probe_errors:?}");
        }
        assert!(
            probe_errors.iter().all(|(code, _)| *code == ErrorCode::Timeout),
            "only a probe deadline may fail here: {probe_errors:?}"
        );
        let slot = driver.book().slot(SLOT).unwrap().clone();
        assert_eq!(slot.healthy, Some(true), "{probe_errors:?}");
        assert_eq!(slot.probe_failures, 0);
        assert_eq!(slot.pages, pages_book_before, "the probe page is not counted");
        assert_eq!(context_count(&driver).await, contexts_before, "probe context disposed");
        let tabs_after: Vec<String> = driver
            .pages
            .lock()
            .await
            .get("probe-session")
            .map(|pages| pages.tabs.iter().map(|tab| tab.tab_id.clone()).collect())
            .unwrap_or_default();
        assert_eq!(tabs_after, tabs_before);
        assert!(!driver.pages.lock().await.contains_key("probe:0"));
        let mut policy_keys_after: Vec<String> =
            driver.egress_policies.lock().await.keys().cloned().collect();
        policy_keys_after.sort();
        assert_eq!(policy_keys_after, policy_keys_before);
        assert_eq!(driver.egress_receipts.lock().await.len(), receipts_before);
        assert_eq!(driver.devtools_events.lock().await.len(), events_before);
        assert_eq!(live_marker(&driver, &session).await, "untouched");
        driver.release(session).await.expect("release");
    }

    /// A navigation that lands between a content read resolving its execution
    /// context and using it gets Chromium's real destroyed-context error. The
    /// production `content()` retries once and returns the new page. The
    /// first attempt is made in the context from before the navigation, which
    /// is exactly what the race does, deterministically.
    #[tokio::test]
    async fn navigation_during_content_read_is_retried_once() {
        if !live_enabled() {
            return;
        }
        let driver = live_driver().with_pool_config(quiet_pool());
        let session = live_session_sized(&driver, "nav-race", None, "old-page").await;
        let page = driver.current_page(&session).await.expect("page");
        let stale_context = page
            .execution_context()
            .await
            .expect("execution context")
            .expect("a main-frame context");
        page.goto("about:blank").await.expect("navigate");
        page.set_content("<html><body><p id=\"m\">new-page</p></body></html>")
            .await
            .expect("new content");

        driver.hooks.content_attempts.store(0, Ordering::SeqCst);
        *driver.hooks.content_stale_context.lock().unwrap() = Some(stale_context);
        let html = driver
            .content(&session)
            .await
            .expect("the retried read succeeds");
        assert_eq!(
            driver.hooks.content_attempts.load(Ordering::SeqCst),
            2,
            "content() retried exactly once"
        );
        let first = driver
            .hooks
            .content_first_error
            .lock()
            .unwrap()
            .clone()
            .expect("first error");
        assert!(
            crate::deadlines::is_context_destroyed(&first),
            "Chromium's error is one the retry recognises: {first}"
        );
        let html = String::from_utf8_lossy(&html).into_owned();
        assert!(html.contains("new-page"), "the new page: {html}");
        assert!(!html.contains("old-page"));
        driver.release(session).await.expect("release");
    }

    /// A tab attach that is cancelled after it created its context and page
    /// (the caller's deadline, a dropped request) leaves nothing behind: the
    /// context is disposed, the page reservation is returned, and the next
    /// attach of the same session works.
    #[tokio::test]
    async fn attach_timeout_cleans_up_partial_context() {
        if !live_enabled() {
            return;
        }
        let driver = Arc::new(live_driver().with_pool_config(quiet_pool()));
        let session = live_session(&driver, "attach-cancel").await;
        let baseline = context_count(&driver).await;
        let pages_baseline = page_target_count(&driver).await;
        *driver.hooks.attach_stall.lock().unwrap() = Some(Duration::from_secs(30));

        let attaching = {
            let driver = driver.clone();
            let session = session.clone();
            tokio::spawn(async move { driver.new_tab(&session, None).await })
        };
        assert!(
            eventually(Duration::from_secs(15), || async {
                context_count(&driver).await == baseline + 1
            })
            .await,
            "the attach created its context and stalls"
        );
        assert_eq!(driver.book().slot(SLOT).unwrap().pages, 1, "page reserved");
        // Once its page exists the attach stalls, and not on the browser lock:
        // nothing else uses this driver (no prober, no reaper), so the write
        // side is free only if the stalled attach holds no guard at all.
        assert!(
            eventually(Duration::from_secs(15), || async {
                driver.hooks.attach_stalled.load(Ordering::SeqCst)
            })
            .await,
            "the attach reached its stall"
        );
        assert_eq!(page_target_count(&driver).await, pages_baseline + 1, "its page is open");
        assert!(
            driver.browser.try_write().is_ok(),
            "the stalled attach holds no browser lock"
        );
        assert!(!attaching.is_finished(), "the attach is still stalled");
        attaching.abort();
        let _ = attaching.await;

        assert_eq!(driver.book().slot(SLOT).unwrap().pages, 0, "reservation returned");
        assert!(
            eventually(Duration::from_secs(10), || async {
                context_count(&driver).await == baseline
            })
            .await,
            "the abandoned context is disposed"
        );
        assert!(
            eventually(Duration::from_secs(10), || async {
                page_target_count(&driver).await == pages_baseline
            })
            .await,
            "the abandoned page is closed"
        );
        {
            let pages = driver.pages.lock().await;
            let entry = pages.get("attach-cancel");
            assert!(entry.is_none_or(|pages| pages.tabs.is_empty()
                && pages.browser_context_id.is_none()));
        }

        *driver.hooks.attach_stall.lock().unwrap() = None;
        driver.new_tab(&session, None).await.expect("next attach works");
        assert_eq!(context_count(&driver).await, baseline + 1);
        assert_eq!(driver.book().slot(SLOT).unwrap().pages, 1);
        driver.release(session).await.expect("release");
        assert_eq!(driver.book().slot(SLOT).unwrap().pages, 0);
        assert!(
            eventually(Duration::from_secs(5), || async {
                context_count(&driver).await == baseline
            })
            .await
        );
    }

    /// A session object for `key` without going through `acquire` (no browser).
    fn bare_session(key: &str) -> BrowserSession {
        BrowserSession {
            lease: make_lease_with_key(key),
            inner: Arc::new(Mutex::new(SessionInner {
                connected: true,
                pages_served: 0,
            })),
        }
    }

    fn assert_cap_details(error: &QuarryError, reason: &str, limit: usize, active: usize) {
        assert_eq!(error.code, ErrorCode::RateLimited, "{error:?}");
        let details = error.details.as_ref().expect("cap details");
        assert_eq!(details["reason"], reason);
        assert_eq!(details["scope"], "driver");
        assert_eq!(details["limit"], limit);
        assert_eq!(details["active"], active);
        assert_eq!(
            details["retry_after_seconds"],
            crate::slots::BUSY_RETRY_AFTER_S
        );
    }

    /// The session cap is the driver's 429: `browser_session_cap` with its
    /// numbers, and the refused key is not admitted. Refused before any
    /// launch, so no browser is needed.
    #[tokio::test]
    async fn acquire_past_the_session_cap_is_rate_limited_with_details() {
        let driver = ChromiumoxideDriver::new().with_pool_config(BrowserPoolConfig {
            max_sessions: 1,
            ..quiet_pool()
        });
        assert!(matches!(
            driver
                .book()
                .admit("cap-holder", Duration::from_secs(30), Instant::now()),
            Admission::Admitted { .. }
        ));
        let error = driver
            .acquire(&make_lease_with_key("cap-second"))
            .await
            .expect_err("over the cap");
        assert_cap_details(&error, "browser_session_cap", 1, 1);
        assert!(!driver.book().is_admitted("cap-second"));
        assert_eq!(driver.launch_count(), 0, "refused before any launch");
    }

    /// The page cap is the driver's 429 too: `browser_page_cap`, and the
    /// refused attach takes no page from the book. Refused before any launch.
    #[tokio::test]
    async fn new_tab_past_the_page_cap_is_rate_limited_with_details() {
        let driver = ChromiumoxideDriver::new()
            .with_pinned_egress_proxy(Arc::new(StubEgressProvider))
            .with_pool_config(BrowserPoolConfig {
                max_pages_per_slot: 1,
                ..quiet_pool()
            });
        let now = Instant::now();
        driver
            .book()
            .admit("page-cap", Duration::from_secs(30), now);
        driver
            .book()
            .page_opened("page-cap", now)
            .expect("the one page");
        let session = bare_session("page-cap");
        driver
            .configure_egress_policy(&session, BrowserEgressPolicy::deny_all())
            .await
            .expect("policy");
        let error = driver
            .new_tab(&session, None)
            .await
            .expect_err("over the page cap");
        assert_cap_details(&error, "browser_page_cap", 1, 1);
        assert_eq!(driver.book().slot(SLOT).unwrap().pages, 1);
        assert_eq!(driver.launch_count(), 0, "refused before any launch");
    }

    /// The two losses that are not a reap or a crash: an admitted session with
    /// no page says `no_current_page`, a released one `session_not_open`. Both
    /// are not retryable and neither opens a tab.
    #[tokio::test]
    async fn no_current_page_and_session_not_open_are_typed() {
        let driver = ChromiumoxideDriver::new()
            .with_pinned_egress_proxy(Arc::new(StubEgressProvider))
            .with_pool_config(quiet_pool());
        driver
            .book()
            .admit("typed-loss", Duration::from_secs(30), Instant::now());
        let session = bare_session("typed-loss");
        let error = driver.content(&session).await.expect_err("no page yet");
        assert_eq!(loss_reason(&error), "no_current_page", "{error:?}");
        assert!(!error.retryable);

        driver.release(session.clone()).await.expect("release");
        driver
            .configure_egress_policy(&session, BrowserEgressPolicy::deny_all())
            .await
            .expect("policy");
        let error = driver
            .new_tab(&session, None)
            .await
            .expect_err("a released session opens nothing");
        assert_eq!(loss_reason(&error), "session_not_open", "{error:?}");
        assert!(!error.retryable);
        let error = driver
            .goto(&session, "about:blank")
            .await
            .expect_err("a released session navigates nowhere");
        assert_eq!(loss_reason(&error), "session_not_open", "{error:?}");
        assert_eq!(driver.book().slot(SLOT).unwrap().pages, 0);
        assert_eq!(driver.launch_count(), 0);
    }

    /// An egress authority whose endpoint fails for the probe's key only, so
    /// sessions work and every probe fails.
    struct FailingProbeProvider;

    #[async_trait::async_trait]
    impl BrowserEgressProxyProvider for FailingProbeProvider {
        async fn endpoint_for_session(
            &self,
            session_key: &str,
        ) -> QuarryResult<PinnedEgressProxyEndpoint> {
            if session_key.starts_with("probe:") {
                return Err(QuarryError::new(
                    ErrorCode::DriverFailed,
                    "probe endpoint refused (test)",
                ));
            }
            PinnedEgressProxyEndpoint::parse("http://127.0.0.1:9")
        }

        async fn receipts_after(
            &self,
            _session_key: &str,
            _after_sequence: u64,
            _limit: usize,
        ) -> QuarryResult<Vec<BrowserEgressReceipt>> {
            Ok(Vec::new())
        }

        async fn configure_policy(
            &self,
            _session_key: &str,
            _policy: BrowserEgressPolicy,
        ) -> QuarryResult<()> {
            Ok(())
        }

        async fn release_session(&self, _session_key: &str) {}
    }

    /// Two failed probes make the slot unhealthy. With a session on it the
    /// browser is never killed and the session keeps working; once the slot
    /// is empty the next failed probe recycles it.
    #[tokio::test]
    async fn unhealthy_slot_with_sessions_is_kept_and_an_empty_one_recycled() {
        if !live_enabled() {
            return;
        }
        let driver = ChromiumoxideDriver::new()
            .with_pinned_egress_proxy(Arc::new(FailingProbeProvider))
            .with_pool_config(quiet_pool());
        let session = live_session_sized(&driver, "unhealthy-keep", None, "still-mine").await;
        assert_eq!(driver.launch_count(), 1);

        driver.probe_tick().await;
        assert_eq!(driver.book().slot(SLOT).unwrap().probe_failures, 1);
        assert_eq!(driver.book().slot(SLOT).unwrap().healthy, None, "one failure is not enough");
        driver.probe_tick().await;
        let slot = driver.book().slot(SLOT).unwrap().clone();
        assert_eq!(slot.healthy, Some(false), "two failures: unhealthy");
        assert_eq!(driver.health().await.state, crate::BrowserHealthState::Unhealthy);
        driver.probe_tick().await;
        assert_eq!(driver.launch_count(), 1, "a slot with a session is never recycled");
        assert_eq!(driver.book().slot(SLOT).unwrap().generation, 1);
        assert!(driver.book().is_admitted("unhealthy-keep"));
        assert_eq!(live_marker(&driver, &session).await, "still-mine");

        driver.release(session).await.expect("release");
        driver.probe_tick().await;
        assert_eq!(driver.launch_count(), 2, "the empty unhealthy slot is recycled");
        let slot = driver.book().slot(SLOT).unwrap().clone();
        assert_eq!(slot.generation, 2);
        assert_eq!(slot.status, SlotStatus::Ready);
        assert_eq!(slot.healthy, None, "a new browser is unprobed");
    }

    /// `QUARRY_BROWSER_REAPER=0` turns the reaper off, not the prober: after a
    /// crash the prober relaunches the browser with no acquire, so an edge
    /// that `/ready` took out of traffic recovers by itself.
    #[tokio::test]
    async fn crash_with_reaper_off_is_relaunched_by_the_prober() {
        if !live_enabled() {
            return;
        }
        let driver = live_driver().with_pool_config(BrowserPoolConfig {
            reaper: ReaperMode::Off,
            probe_interval: Duration::from_millis(300),
            ..BrowserPoolConfig::default()
        });
        let old = live_session_sized(&driver, "off-crash", None, "before").await;
        assert_eq!(driver.launch_count(), 1);
        {
            let mut guard = driver.browser.write().await;
            let browser = guard.as_mut().expect("a running browser");
            let _ = browser.kill().await;
        }
        assert!(
            eventually(Duration::from_secs(40), || async {
                driver.launch_count() == 2
                    && driver.health().await.state == crate::BrowserHealthState::Healthy
            })
            .await,
            "relaunched and probed healthy without an acquire (launches {}, state {:?})",
            driver.launch_count(),
            driver.health().await.state
        );
        let error = driver.content(&old).await.expect_err("the old page is gone");
        assert_eq!(loss_reason(&error), "browser_process_lost", "{error:?}");
        driver.release(old).await.expect("release lost");
    }

    /// A release that runs while a tab attach of the same session is in
    /// flight (the attach wakes up while the teardown is still running)
    /// leaves no page and no context behind: the attach fails its admission
    /// check and its guard closes what it made.
    #[tokio::test]
    async fn release_during_attach_leaves_no_page_or_context() {
        if !live_enabled() {
            return;
        }
        let driver = Arc::new(live_driver().with_pool_config(quiet_pool()));
        let session = live_session(&driver, "release-attach").await;
        let baseline = context_count(&driver).await;
        let pages_baseline = page_target_count(&driver).await;
        *driver.hooks.attach_stall.lock().unwrap() = Some(Duration::from_secs(2));
        // Eight seconds of margin after the attach wakes: the rest of the attach
        // (page hygiene, collectors) is slow under the parallel live suite.
        *driver.hooks.teardown_stall.lock().unwrap() = Some(Duration::from_secs(10));

        let attaching = {
            let driver = driver.clone();
            let session = session.clone();
            tokio::spawn(async move { driver.new_tab(&session, None).await })
        };
        // Wait for the stall itself: the page target is listed before
        // `new_page` returns, and releasing then would start the teardown's
        // clock while the attach is still opening its page.
        assert!(
            eventually(Duration::from_secs(15), || async {
                driver.hooks.attach_stalled.load(Ordering::SeqCst)
            })
            .await,
            "the attach opened its page and stalls"
        );
        assert_eq!(page_target_count(&driver).await, pages_baseline + 1, "its page is open");
        // The attach wakes up two seconds in, while the teardown stalls.
        driver.release(session.clone()).await.expect("release");
        assert!(attaching.is_finished(), "the attach ended during the teardown");
        let attached = attaching.await.expect("join");
        assert!(attached.is_err(), "the released session got no tab: {attached:?}");
        *driver.hooks.attach_stall.lock().unwrap() = None;
        *driver.hooks.teardown_stall.lock().unwrap() = None;

        assert!(!driver.pages.lock().await.contains_key("release-attach"));
        assert!(!driver.book().is_admitted("release-attach"));
        assert_eq!(driver.book().slot(SLOT).unwrap().pages, 0);
        assert!(
            eventually(Duration::from_secs(10), || async {
                context_count(&driver).await == baseline
            })
            .await,
            "the attach's context is disposed"
        );
        assert!(
            eventually(Duration::from_secs(10), || async {
                page_target_count(&driver).await == pages_baseline
            })
            .await,
            "the attach's page is closed"
        );
    }

    /// A live frame captured for a session released meanwhile leaves no cache
    /// slot behind.
    #[tokio::test]
    async fn live_frame_for_a_released_session_leaves_no_cache() {
        if !live_enabled() {
            return;
        }
        let driver = Arc::new(live_driver().with_pool_config(quiet_pool()));
        let session = live_session_sized(&driver, "frame-release", None, "frame").await;
        *driver.hooks.live_frame_stall.lock().unwrap() = Some(Duration::from_secs(2));
        let capturing = {
            let driver = driver.clone();
            let session = session.clone();
            tokio::spawn(async move {
                driver
                    .live_frame(&session, LiveFrameOptions::default())
                    .await
            })
        };
        assert!(
            eventually(Duration::from_secs(10), || async {
                driver.hooks.live_frame_stalled.load(Ordering::SeqCst)
            })
            .await,
            "the capture found its page and stalls"
        );
        driver.release(session).await.expect("release");
        let captured = capturing.await.expect("join");
        *driver.hooks.live_frame_stall.lock().unwrap() = None;
        let error = captured.expect_err("no frame for a released session");
        assert_eq!(loss_reason(&error), "session_not_open", "{error:?}");
        assert!(!driver.live_frames.lock().await.contains_key("frame-release"));
    }

    /// A `goto` of a session that was released and replaced under the same
    /// key while it navigated reports its page change to the entry it started
    /// under: the newer session's cached frame stays.
    #[tokio::test]
    async fn a_late_goto_of_a_released_session_leaves_the_next_sessions_frame_alone() {
        if !live_enabled() {
            return;
        }
        let driver = Arc::new(live_driver().with_pool_config(quiet_pool()));
        let old = live_session_sized(&driver, "goto-reuse", None, "old").await;
        driver.hooks.goto_hold.store(true, Ordering::SeqCst);
        let navigating = {
            let driver = driver.clone();
            let old = old.clone();
            tokio::spawn(async move { driver.goto(&old, "about:blank").await })
        };
        tokio::time::timeout(Duration::from_secs(30), driver.hooks.goto_reached.notified())
            .await
            .expect("the goto navigated and holds");
        driver.hooks.goto_hold.store(false, Ordering::SeqCst);

        driver.release(old).await.expect("release");
        let new = live_session_sized(&driver, "goto-reuse", None, "new").await;
        driver
            .live_frame(&new, LiveFrameOptions::default())
            .await
            .expect("the new session's frame");
        let cached = |frames: &LiveFrameCache| {
            frames
                .slots
                .get("goto-reuse")
                .is_some_and(|slot| slot.cached.is_some())
        };
        assert!(cached(&*driver.live_frames.lock().await), "the new frame is cached");

        driver.hooks.goto_resume.notify_one();
        navigating.await.expect("join").expect("the old goto ends");
        assert!(
            cached(&*driver.live_frames.lock().await),
            "the old session's late page change dropped the new session's frame"
        );
        driver.release(new).await.expect("release");
    }

    /// `close_tab` gives the page back to the book entry that reserved it:
    /// an ordinary close frees one page, and a close that runs after the
    /// session was released and the key admitted again frees nothing of the
    /// newer session's.
    #[tokio::test]
    async fn close_tab_gives_the_page_back_to_the_entry_that_opened_it() {
        if !live_enabled() {
            return;
        }
        let driver = Arc::new(live_driver().with_pool_config(quiet_pool()));
        let pages = |driver: &ChromiumoxideDriver| driver.book().slot(SLOT).unwrap().pages;
        let old = live_session_sized(&driver, "close-reuse", None, "old").await;
        assert_eq!(pages(&driver), 1);
        let second = driver.new_tab(&old, None).await.expect("second tab");
        assert_eq!(pages(&driver), 2);
        driver
            .close_tab(&old, &second.tab_id)
            .await
            .expect("an ordinary close");
        assert_eq!(pages(&driver), 1, "the close freed its own page");

        let first = driver
            .list_tabs(&old)
            .await
            .expect("tabs")
            .into_iter()
            .next()
            .expect("the first tab");
        driver.hooks.close_tab_hold.store(true, Ordering::SeqCst);
        let closing = {
            let driver = driver.clone();
            let old = old.clone();
            tokio::spawn(async move { driver.close_tab(&old, &first.tab_id).await })
        };
        tokio::time::timeout(
            Duration::from_secs(30),
            driver.hooks.close_tab_reached.notified(),
        )
        .await
        .expect("the close took the tab and holds");
        driver.hooks.close_tab_hold.store(false, Ordering::SeqCst);

        driver.release(old).await.expect("release");
        let new = live_session_sized(&driver, "close-reuse", None, "new").await;
        assert_eq!(pages(&driver), 1, "the new session's page");
        driver.hooks.close_tab_resume.notify_one();
        closing.await.expect("join").expect("the old close ends");
        assert_eq!(
            pages(&driver),
            1,
            "the old session's late close freed the new session's page"
        );
        driver.release(new).await.expect("release");
    }

    /// A page is reserved for the entry the key is admitted under now, and
    /// the attach guard that gives it back names that entry: a guard of an
    /// attach that outlived its session frees nothing of the next one's.
    #[tokio::test]
    async fn a_page_reservation_and_its_attach_guard_belong_to_one_entry_of_the_key() {
        let driver = ChromiumoxideDriver::new().with_pool_config(quiet_pool());
        let ttl = Duration::from_secs(30);
        let pages = |driver: &ChromiumoxideDriver| driver.book().slot(SLOT).unwrap().pages;
        driver.book().admit("res-reuse", ttl, Instant::now());
        let old = driver.reserve_page("res-reuse").expect("a page");
        assert_eq!(Some(old), driver.book().admission_id("res-reuse"));
        driver.book().release("res-reuse");
        driver.book().admit("res-reuse", ttl, Instant::now());
        let new = driver.reserve_page("res-reuse").expect("a page");
        assert_ne!(old, new, "a key admitted again is a new entry");
        assert_eq!(pages(&driver), 1);

        drop(AttachGuard::new(&driver, "res-reuse".into(), Some(old)));
        assert_eq!(pages(&driver), 1, "the old attach freed the new page");
        drop(AttachGuard::new(&driver, "res-reuse".into(), Some(new)));
        assert_eq!(pages(&driver), 0, "the guard gives back its own entry's page");
    }

    /// An acquire dropped during the cold launch (a scrape's own timeout does
    /// that) gives its admission back, and the next acquire works.
    #[tokio::test]
    async fn acquire_dropped_during_launch_gives_its_admission_back() {
        if !live_enabled() {
            return;
        }
        let driver = live_driver().with_pool_config(quiet_pool());
        let lease = make_lease_with_key("dropped-acquire");
        let outcome = tokio::time::timeout(Duration::from_millis(1), driver.acquire(&lease)).await;
        assert!(outcome.is_err(), "the launch takes longer than 1 ms");
        assert!(!driver.book().is_admitted("dropped-acquire"));
        assert_eq!(driver.book().sessions_open(), 0);

        let session = live_session_sized(&driver, "dropped-acquire", None, "back").await;
        assert_eq!(live_marker(&driver, &session).await, "back");
        driver.release(session).await.expect("release");
        assert_eq!(driver.book().sessions_open(), 0);
    }

    /// Two acquires of one key, the second joining while the first launches.
    /// The first is dropped (its scrape timed out): the second still returns
    /// a session the book holds, and it opens pages.
    #[tokio::test]
    async fn acquire_joining_a_dropped_launch_keeps_the_admission() {
        if !live_enabled() {
            return;
        }
        let driver = live_driver().with_pool_config(quiet_pool());
        let lease = make_lease_with_key("joined-acquire");
        let mut first = Box::pin(driver.acquire(&lease));
        let second = driver.acquire(&lease);
        tokio::pin!(second);
        assert!(
            tokio::time::timeout(Duration::from_millis(1), &mut first)
                .await
                .is_err(),
            "the launch takes longer than 1 ms"
        );
        assert!(driver.book().is_admitted("joined-acquire"));
        assert!(
            tokio::time::timeout(Duration::from_millis(1), &mut second)
                .await
                .is_err(),
            "the joiner waits for the launch"
        );
        // Boxed, so this drops the future itself and not a pinned reference.
        drop(first);
        assert!(
            driver.book().is_admitted("joined-acquire"),
            "the joiner still holds the entry"
        );
        let session = second.await.expect("the joiner's acquire");
        assert!(driver.book().is_admitted("joined-acquire"));
        assert_eq!(driver.book().sessions_open(), 1);
        driver
            .configure_egress_policy(&session, BrowserEgressPolicy::deny_all())
            .await
            .expect("install deny-all egress policy");
        driver.new_tab(&session, None).await.expect("a page opens");
        driver.release(session).await.expect("release");
        assert_eq!(driver.book().sessions_open(), 0);
    }

    /// A session under an allow-nothing policy: the page can render what the test
    /// injects and reach nothing else. `new_tab` refuses to open without a policy.
    async fn live_session(driver: &ChromiumoxideDriver, key: &str) -> BrowserSession {
        let session = driver
            .acquire(&make_lease_with_key(key))
            .await
            .expect("acquire");
        driver
            .configure_egress_policy(&session, BrowserEgressPolicy::deny_all())
            .await
            .expect("install deny-all egress policy");
        session
    }

    /// Page-initiated subresources must be refused by the CDP Fetch boundary
    /// before Chromium opens any connection. `new_tab` now requires the full
    /// egress half (pinned proxy + installed policy), so this test provisions
    /// both — with a deliberately empty allow-list, matching a run whose
    /// grant permits no domains.
    ///
    /// This covers the CDP half only, and an "assert the request log is empty"
    /// assertion also passes when the witness is broken — so treat it as
    /// defence-in-depth evidence, not as the proof. The positive-controlled,
    /// real-pinned-proxy proof lives in quarry-runtime; see
    /// `loopback_egress_is_proven_in_quarry_runtime` below.
    ///
    /// Requires a local Chromium/Chrome binary, so it carries the same
    /// `CHROMIUMOXIDE_TEST=1` guard as its sibling browser tests. Without that
    /// guard it launches a browser unconditionally and fails on every machine
    /// that has none.
    #[tokio::test]
    async fn blocks_loopback_subresources_before_they_reach_the_server() {
        if !live_enabled() {
            return;
        }

        let target = MockServer::start().await;
        let driver =
            ChromiumoxideDriver::new().with_pinned_egress_proxy(Arc::new(StubEgressProvider));
        let session = driver.acquire(&make_lease()).await.expect("acquire");
        // Deny-all policy: an absent or permissive policy would invalidate
        // what this test proves.
        driver
            .configure_egress_policy(&session, BrowserEgressPolicy::deny_all())
            .await
            .expect("install deny-all egress policy");
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

    /// The end-to-end loopback-egress proof lives in
    /// `quarry-runtime/tests/browser_egress_boundary.rs`, not here.
    ///
    /// An earlier in-module version of it **failed on every machine** with
    /// `"chromium browser navigation requires a pinned egress proxy"`:
    /// `require_configured_egress` needs a real `BrowserEgressProxyProvider`,
    /// the only production one is `quarry_runtime::PinnedBrowserEgressProxy`,
    /// and quarry-runtime depends on this crate — so importing it would be
    /// circular. It also lacked the `CHROMIUMOXIDE_TEST=1` guard its sibling
    /// browser tests carry, so it ran unconditionally and failed without a
    /// browser too. The CDP-boundary test above clears both objections:
    /// `StubEgressProvider` satisfies the provider requirement without the
    /// circular dependency, and the guard is now in place.
    ///
    /// What lives in quarry-runtime is still strictly more than any in-module
    /// test can be: a live Chromium refusing a loopback navigation behind the
    /// real pinned proxy, AND a real TCP request through that proxy proving the
    /// transport half — the half CDP interception is only defence in depth for.
    /// Both carry positive controls, because an "assert the request log is
    /// empty" test also passes when the witness is broken.
    ///
    /// The decision functions themselves stay unit-tested next to where they
    /// live: see `navigation::tests` for `guard_page_request_target` and
    /// `guard_navigation_target` against loopback and RFC1918 targets.
    #[test]
    fn loopback_egress_is_proven_in_quarry_runtime() {
        // A marker, deliberately: the moved coverage is easy to lose track of,
        // and a comment alone would not survive a file-wide search for the old
        // test name.
        assert!(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../quarry-runtime/tests/browser_egress_boundary.rs")
                .exists(),
            "the live browser-egress proof has moved or been deleted; it is the \
             only end-to-end evidence that Chromium cannot reach loopback"
        );
    }

    /// Integration test that requires a local Chromium/Chrome binary.
    /// Skipped unless `CHROMIUMOXIDE_TEST=1` is set.
    #[tokio::test]
    async fn launches_and_fetches_content() {
        if !live_enabled() {
            return;
        }

        // `goto("data:...")` cannot work: the navigation guard requires a host. The
        // page is injected into a blank tab instead, which is also all this test
        // needs (launch, render, read the DOM back).
        let driver = live_driver();
        let session = live_session(&driver, "live-content").await;
        driver.new_tab(&session, None).await.expect("blank tab");
        let page = driver.current_page(&session).await.expect("current page");
        page.set_content("<html><body><h1>hi</h1></body></html>")
            .await
            .expect("inject page");

        let html = driver.content(&session).await.expect("content");
        let html = String::from_utf8_lossy(&html);
        assert!(html.contains("<h1>hi</h1>"), "content was: {html}");

        driver.release(session).await.expect("release");
    }

    /// Integration test that requires a local Chromium/Chrome binary.
    /// Skipped unless `CHROMIUMOXIDE_TEST=1` is set.
    #[tokio::test]
    async fn captures_live_frame_after_navigation() {
        if !live_enabled() {
            return;
        }

        let driver = live_driver();
        let session = live_session(&driver, "live-frame").await;
        driver.new_tab(&session, None).await.expect("blank tab");
        let page = driver.current_page(&session).await.expect("current page");
        page.set_content("<html><body style='background:#0056cc'><h1>frame</h1></body></html>")
            .await
            .expect("inject page");

        let frame = driver
            .live_frame(
                &session,
                LiveFrameOptions {
                    format: LiveFrameFormat::Jpeg,
                    quality: 60,
                    max_width: 640,
                    max_height: 480,
                    every_nth_frame: 1,
                    // The clamp's maximum: the first screencast frame took
                    // over 2 s once under the parallel live suite.
                    timeout_ms: 5_000,
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
    #[ignore = "needs a public origin: cookie hydration needs a real http(s) page, and the navigation guard refuses loopback"]
    async fn reopening_a_session_with_a_saved_profile_restores_cookies_without_erroring() {
        if !live_enabled() {
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
    #[ignore = "needs a public origin: document.cookie needs a real http(s) origin, and the navigation guard refuses loopback"]
    async fn distinct_sessions_do_not_share_cookies() {
        if !live_enabled() {
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
    #[ignore = "needs a public origin: document.cookie needs a real http(s) origin, and the navigation guard refuses loopback"]
    async fn second_tab_on_same_session_shares_that_sessions_cookies() {
        if !live_enabled() {
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

    /// Chromium's memory by process, from `/proc` (Linux only; zeros elsewhere):
    /// `(processes, pss_kib, rss_kib)` summed over every process named
    /// `chrom*`. PSS counts a page shared between renderers once, which is the
    /// number to size a container by.
    fn chromium_memory_kib() -> (usize, u64, u64) {
        let (mut processes, mut pss, mut rss) = (0usize, 0u64, 0u64);
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return (0, 0, 0);
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_pid = entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.bytes().all(|b| b.is_ascii_digit()));
            if !is_pid {
                continue;
            }
            let comm = std::fs::read_to_string(path.join("comm")).unwrap_or_default();
            if !comm.trim().starts_with("chrom") {
                continue;
            }
            let Ok(rollup) = std::fs::read_to_string(path.join("smaps_rollup")) else {
                continue;
            };
            let field = |name: &str| -> u64 {
                rollup
                    .lines()
                    .find_map(|line| line.strip_prefix(name))
                    .and_then(|rest| rest.split_whitespace().next())
                    .and_then(|number| number.parse().ok())
                    .unwrap_or(0)
            };
            processes += 1;
            pss += field("Pss:");
            rss += field("Rss:");
        }
        (processes, pss, rss)
    }

    /// A page with a few thousand DOM nodes and a script-held array: heavier than
    /// a blank tab, lighter than most real sites. Memory per session on real
    /// pages will be higher; this is the floor the run caps are sized from.
    fn synthetic_page() -> String {
        let rows: String = (0..3000)
            .map(|i| format!("<div class=\"row\" style=\"padding:2px\"><span>row {i}</span> some text to lay out</div>"))
            .collect();
        format!(
            "<html><body>{rows}<script>window.items = Array.from({{length: 50000}}, (_, i) => ({{i, s: 'item ' + i}}));</script></body></html>"
        )
    }

    /// Measurement, not a regression test: prints Chromium memory with 1, 8, 16
    /// and 32 sessions open, one tab each, on `synthetic_page`. The edge's
    /// default run caps (`QUARRY_EDGE__MAX_CONCURRENT_RUNS` and friends) are
    /// placeholders until this has been run on the target machine:
    ///
    /// `CHROMIUMOXIDE_TEST=1 CHROMIUMOXIDE_MEASURE=1 cargo test -p quarry-browser
    /// --features chromiumoxide measure_memory_per_open_session -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "measurement: needs Chromium and CHROMIUMOXIDE_MEASURE=1; prints MEASURE lines"]
    async fn measure_memory_per_open_session() {
        if std::env::var("CHROMIUMOXIDE_TEST").ok().as_deref() != Some("1")
            || std::env::var("CHROMIUMOXIDE_MEASURE").ok().as_deref() != Some("1")
        {
            eprintln!("skipping: set CHROMIUMOXIDE_TEST=1 and CHROMIUMOXIDE_MEASURE=1");
            return;
        }
        let driver = live_driver();
        let page_html = synthetic_page();
        let mut sessions = Vec::new();
        let (processes, pss, rss) = chromium_memory_kib();
        eprintln!(
            "MEASURE sessions=0 processes={processes} pss_mib={} rss_mib={}",
            pss / 1024,
            rss / 1024
        );
        for target in [1usize, 8, 16, 32] {
            while sessions.len() < target {
                let session = live_session(&driver, &format!("measure-{}", sessions.len())).await;
                driver.new_tab(&session, None).await.expect("blank tab");
                let page = driver.current_page(&session).await.expect("current page");
                page.set_content(&page_html).await.expect("inject page");
                sessions.push(session);
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
            let (processes, pss, rss) = chromium_memory_kib();
            eprintln!(
                "MEASURE sessions={target} processes={processes} pss_mib={} rss_mib={}",
                pss / 1024,
                rss / 1024
            );
        }
        for session in sessions {
            driver.release(session).await.expect("release");
        }
    }

    // WS3.K: live fixtures for trusted input and the actionability pipeline.
    include!("trusted_input_live.rs");
}
