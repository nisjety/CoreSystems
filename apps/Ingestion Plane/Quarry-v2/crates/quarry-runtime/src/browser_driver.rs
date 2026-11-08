//! BrowserDriverAdapter — wraps a `quarry_browser::BrowserDriver` and
//! presents it as a `crate::driver::Driver` so the pipeline can use the
//! same interface regardless of whether the page is fetched via a plain
//! HTTP stack or a full-browser session.

use async_trait::async_trait;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use url::Url;

use quarry_browser::{
    BrowserDriver as BrowserDriverTrait, BrowserEgressPolicy, BrowserSession, StitchPermit,
};
use quarry_core::capture::{CaptureLimits, CapturePlan};
use quarry_core::scrape_actions::{ActionLimits, ActionStatus};
use tokio_util::sync::CancellationToken;
use quarry_core::output::DriverKind;
use quarry_core::{
    ids::kinds,
    lease::{BrowserLease, ProxyAffinity},
    ErrorCode, QuarryError, QuarryResult,
};

use crate::{
    action_runtime::{ActionBudget, ActionRuntime, ScrapeActionRun},
    capture::{CaptureWarning, PageCaptures},
    driver::{Driver, FetchHints, ScrapeCapabilities},
    fetch::FetchResponse,
    lease_pool::{LeaseGuard, RuntimeLeasePool},
};

const LEASE_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(30);

/// Default `wait_for_selector` timeout when the caller doesn't specify
/// one. 5 seconds is a reasonable upper bound for SPA hydration on a
/// well-provisioned page — anything longer should be an explicit caller
/// choice, not a default.
const DEFAULT_WAIT_FOR_TIMEOUT_MS: u32 = 5_000;

/// Hydration settle budget when the caller gave no `wait_for_selector`.
/// `goto` resolves on the document load event, which for a JS shell
/// (Next.js/Nuxt/SPA) fires BEFORE the framework has rendered any text —
/// snapshotting right there returns the same empty shell the static driver
/// already had, silently defeating the fallback. When the first snapshot
/// still looks like a shell we re-snapshot until the visible text stops
/// growing or this budget elapses. Override with `QUARRY_BROWSER_SETTLE_MS`.
const DEFAULT_SETTLE_MS: u64 = 3_500;
const SETTLE_POLL: Duration = Duration::from_millis(300);
/// Below this many visible text chars a snapshot is treated as "not hydrated
/// yet" (a real page has far more; a shell has a title and little else).
const SETTLE_MIN_TEXT_CHARS: usize = 600;

fn settle_budget() -> Duration {
    std::env::var("QUARRY_BROWSER_SETTLE_MS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(Duration::from_millis(DEFAULT_SETTLE_MS))
}

/// E3.3-j: the post-action settle is on only when this flag reads `1`, `true`, `on` or `yes`.
/// It is off by default, so an observation then carries no `settle_ms` and its wire is unchanged.
/// The name is its own: `QUARRY_BROWSER_SETTLE_MS` above is the hydration budget, live in the stack.
pub const ENV_ACTION_SETTLE: &str = "QUARRY_BROWSER_ACTION_SETTLE";
/// E3.3-j: the bound on the post-action wait in ms, read only when the settle is on. A malformed
/// value takes the default, and any value is capped at `ACTION_SETTLE_MAX_MS`.
pub const ENV_ACTION_SETTLE_MS: &str = "QUARRY_BROWSER_ACTION_SETTLE_MS";
const ACTION_SETTLE_DEFAULT_MS: u64 = 300;
const ACTION_SETTLE_MAX_MS: u64 = 2_000;
/// E3.3-j: the gap between two DOM samples. The DOM is quiet when two samples one gap apart match.
const ACTION_SETTLE_SAMPLE: Duration = Duration::from_millis(100);

/// E3.3-j: the post-action settle bound, or `None` when the settle is off.
pub(crate) fn action_settle_bound() -> Option<Duration> {
    action_settle_bound_from(
        std::env::var(ENV_ACTION_SETTLE).ok().as_deref(),
        std::env::var(ENV_ACTION_SETTLE_MS).ok().as_deref(),
    )
}

/// The pure form of [`action_settle_bound`]. An unrecognised flag is off, as an unset one is: a
/// typo must not switch a wait on. A bound of zero is off too.
fn action_settle_bound_from(flag: Option<&str>, bound_ms: Option<&str>) -> Option<Duration> {
    let on = matches!(
        flag.map(|value| value.trim().to_ascii_lowercase()).as_deref(),
        Some("1" | "true" | "on" | "yes")
    );
    if !on {
        return None;
    }
    let ms = bound_ms
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(ACTION_SETTLE_DEFAULT_MS)
        .min(ACTION_SETTLE_MAX_MS);
    (ms > 0).then(|| Duration::from_millis(ms))
}

/// E3.3-j: how long the page's DOM took to go quiet after an action, in ms, never more than
/// `bound`. The DOM is sampled through `content()`, not watched by a mutation observer: it is
/// quiet when two samples one `ACTION_SETTLE_SAMPLE` apart are byte-identical. A value equal to
/// `bound` means it was still changing when the bound ran out. A failed or slow read ends the
/// wait at once, and nothing is retried. The samples are compared in memory and then dropped.
pub(crate) async fn settle_after_action(
    browser: &dyn BrowserDriverTrait,
    session: &BrowserSession,
    bound: Duration,
) -> u64 {
    settle_quiescence(bound, move || async move { browser.content(session).await.ok() }).await
}

/// The quiescence loop behind [`settle_after_action`], over any sampler, so a test can script the
/// DOM. A `None` from the sampler is a failed read, or one that ran past the bound.
async fn settle_quiescence<F, Fut>(bound: Duration, mut sample: F) -> u64
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<bytes::Bytes>>,
{
    let started = Instant::now();
    let deadline = started + bound;
    let elapsed_ms = || started.elapsed().min(bound).as_millis() as u64;
    let Some(mut previous) = sample_before(deadline, sample()).await else {
        return elapsed_ms();
    };
    while Instant::now() < deadline {
        let gap = ACTION_SETTLE_SAMPLE.min(deadline.saturating_duration_since(Instant::now()));
        tokio::time::sleep(gap).await;
        if Instant::now() >= deadline {
            break;
        }
        match sample_before(deadline, sample()).await {
            Some(next) if next == previous => return elapsed_ms(),
            Some(next) => previous = next,
            None => return elapsed_ms(),
        }
    }
    elapsed_ms()
}

/// One DOM sample, cut off at `deadline`: a read that runs past the bound is no sample.
async fn sample_before<Fut>(deadline: Instant, read: Fut) -> Option<bytes::Bytes>
where
    Fut: std::future::Future<Output = Option<bytes::Bytes>>,
{
    let left = deadline.saturating_duration_since(Instant::now());
    tokio::time::timeout(left, read).await.ok().flatten()
}

/// A snapshot that still needs hydration time: carries scripts (so it *can*
/// hydrate) but exposes little or no readable text yet. Script-less bodies
/// (plain HTML, test stubs) never wait.
fn looks_unhydrated(body: &[u8]) -> bool {
    let has_script = body
        .windows(b"<script".len())
        .any(|w| w.eq_ignore_ascii_case(b"<script"));
    if !has_script {
        return false;
    }
    crate::fallback_driver::is_js_shell_needing_browser(body, 200)
        || crate::fallback_driver::visible_text_len(body) < SETTLE_MIN_TEXT_CHARS
}

/// The page URL as it may appear in a log line (WS12.S6): userinfo and secret
/// query or fragment values are masked by the WS12 Durable URL rules before the
/// URL is formatted into a `tracing` field.
fn log_url(url: &str) -> String {
    let mut tally = quarry_core::redaction::Tally::new();
    quarry_core::redaction::redact_url(url, quarry_core::redaction::Profile::Durable, &mut tally)
        .into_owned()
}

/// Environment variable for per-fetch session isolation (WS13.1). On by
/// default; `0` or `false` restores the legacy host-keyed affinity key.
pub const ENV_SESSION_ISOLATION: &str = "QUARRY_SCRAPE_SESSION_ISOLATION";

/// WS13.4b: the refusal for a capture on a browser that cannot take one.
fn capture_unavailable() -> QuarryError {
    QuarryError::new(
        ErrorCode::Unsupported,
        "this browser cannot take screenshots or PDFs",
    )
    .with_details(serde_json::json!({ "reason": "browser_capability_unavailable" }))
}

/// The typed reason a capture failed: the driver's `details.reason` when it
/// set one, else the stable error code. Never the raw message, which can carry
/// page-controlled text.
fn capture_failure_reason(error: &QuarryError) -> String {
    error
        .details
        .as_ref()
        .and_then(|details| details.get("reason"))
        .and_then(serde_json::Value::as_str)
        .map_or_else(|| error.code.wire_name().to_owned(), str::to_owned)
}

/// Egress decisions scanned for the blocked-request count. A bound: a page
/// that makes more requests than this under-counts.
const CAPTURE_EGRESS_SCAN: usize = 1024;

/// WS13.4b, WS13.5: how many requests the egress policy blocked on this session. A
/// count only, never a URL. A bound: a page that makes more requests than
/// `CAPTURE_EGRESS_SCAN` under-counts. A failed read counts as none, since the page is
/// still returned.
async fn count_blocked_requests(
    inner: &(dyn BrowserDriverTrait + Send + Sync),
    session: &BrowserSession,
) -> u32 {
    match inner.egress_receipts(session, 0, CAPTURE_EGRESS_SCAN).await {
        Ok(receipts) => receipts
            .iter()
            .filter(|receipt| {
                matches!(
                    receipt.decision,
                    quarry_core::contracts::BrowserEgressDecision::Block
                )
            })
            .count()
            .try_into()
            .unwrap_or(u32::MAX),
        Err(_) => 0,
    }
}

/// WS13.4b: take what the request asked for from the settled page. Each
/// capture is independent. A failure is a warning, never an error, so the
/// extracted page is still returned.
async fn take_captures(
    inner: &(dyn BrowserDriverTrait + Send + Sync),
    session: &BrowserSession,
    plan: &CapturePlan,
    limits: &CaptureLimits,
    url: &Url,
    stitch: Option<&StitchPermit>,
) -> PageCaptures {
    let mut out = PageCaptures::default();
    if let Some(options) = plan.screenshot.as_ref() {
        match inner
            .capture_screenshot_stitched(session, options, limits, stitch)
            .await
        {
            Ok(image) => {
                if image.truncated {
                    out.warnings.push(CaptureWarning {
                        code: "screenshot_truncated".into(),
                        message: "the page is taller than one capture; the image stops at the limit"
                            .into(),
                    });
                }
                out.screenshot = Some(image);
            }
            Err(error) => out.warnings.push(CaptureWarning {
                code: "screenshot_failed".into(),
                message: format!(
                    "the screenshot was not taken: {}",
                    capture_failure_reason(&error)
                ),
            }),
        }
    }
    if let Some(options) = plan.pdf.as_ref() {
        match inner.capture_pdf(session, options, limits).await {
            Ok(pdf) => out.pdf = Some(pdf),
            Err(error) => out.warnings.push(CaptureWarning {
                code: "pdf_failed".into(),
                message: format!("the PDF was not made: {}", capture_failure_reason(&error)),
            }),
        }
    }
    // Counts only: the blocked URLs never leave the driver.
    out.blocked_requests = count_blocked_requests(inner, session).await;
    out.final_url = match inner.current_url(session).await {
        Ok(Some(raw)) => raw.parse::<Url>().ok().filter(|reached| reached != url),
        _ => None,
    };
    out
}

/// Environment variable for the heavy lane (WS13.1): how many requests may hold
/// the heavy permit at once. Default 1, clamped to the pool size.
pub const ENV_HEAVY_PERMITS: &str = "QUARRY_SCRAPE_HEAVY_PERMITS";

/// Whether a fetch gets its own session key (WS13.1), from the raw value of
/// `QUARRY_SCRAPE_SESSION_ISOLATION`. Unset or empty is on; only `0` and
/// `false` (ignoring case and surrounding space) turn it off.
pub fn session_isolation_from(value: Option<&str>) -> bool {
    match value.map(|raw| raw.trim().to_ascii_lowercase()) {
        None => true,
        Some(raw) => !(raw == "0" || raw == "false"),
    }
}

/// Whether session isolation is on in this process (reads the environment).
pub fn session_isolation_enabled() -> bool {
    session_isolation_from(std::env::var(ENV_SESSION_ISOLATION).ok().as_deref())
}

/// Heavy-lane permits from the raw value of `QUARRY_SCRAPE_HEAVY_PERMITS`: a
/// missing or unparsable value is 1, and the result is clamped to
/// `1..=pool_size` (a pool of zero still leaves one permit).
pub fn heavy_permits_from(value: Option<&str>, pool_size: usize) -> usize {
    let requested = value
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .unwrap_or(1);
    requested.clamp(1, pool_size.max(1))
}

/// Heavy-lane permits in this process (reads the environment).
pub fn heavy_permits_from_env(pool_size: usize) -> usize {
    heavy_permits_from(std::env::var(ENV_HEAVY_PERMITS).ok().as_deref(), pool_size)
}

/// WS13.6: whether a full-page screenshot may stitch tiles, from the raw value of
/// `QUARRY_EDGE__SCRAPE_STITCH`. Only `on`, `1` and `true` (ignoring case and
/// surrounding space) turn it on; unset is off, and the image is then cropped at the
/// single-capture limit with a warning.
pub fn scrape_stitch_from(value: Option<&str>) -> bool {
    matches!(
        value.map(|raw| raw.trim().to_ascii_lowercase()).as_deref(),
        Some("on" | "1" | "true")
    )
}

/// Whether stitching is on in this process (reads the environment).
pub fn scrape_stitch_enabled() -> bool {
    scrape_stitch_from(std::env::var(ENV_SCRAPE_STITCH).ok().as_deref())
}

/// Environment variable for tile-and-stitch of full-page screenshots (WS13.6).
pub const ENV_SCRAPE_STITCH: &str = "QUARRY_EDGE__SCRAPE_STITCH";

fn heavy_lane(pool: &RuntimeLeasePool) -> Arc<Semaphore> {
    Arc::new(Semaphore::new(heavy_permits_from_env(pool.max_concurrent())))
}

/// The session key of one isolated fetch (WS13.1): the host, a tilde, then the
/// fresh lease id. The host keeps the key readable in logs; the lease id makes
/// it unique, so no two fetches share a browser context.
fn isolated_session_key(host: &str, lease_id: impl std::fmt::Display) -> String {
    format!("{host}~{lease_id}")
}

/// A lease for one isolated fetch (WS13.1). Nothing is pooled under the host
/// key and the lease is never returned to the pool, so the next fetch starts
/// from a fresh browser context with no cookies or storage from this one.
fn isolated_lease(host: &str, org_id: &str) -> BrowserLease {
    let lease_id = kinds::LeaseKind::new();
    let session_affinity_key = isolated_session_key(host, &lease_id);
    BrowserLease {
        lease_id,
        profile_id: kinds::ProfileKind::new(),
        session_affinity_key,
        proxy_affinity: ProxyAffinity {
            pool: "default".into(),
            sticky_key: None,
        },
        ttl_s: 30,
        capabilities: vec![],
        artifact_bucket: "default".into(),
        persist_profile: false,
        viewport: None,
        org_id: org_id.to_owned(),
    }
}

/// Adapts a [`quarry_browser::BrowserDriver`] into the [`Driver`] trait.
///
/// The adapter acquires a browser session, executes the fetch, then
/// releases the session back to the pool.  The actual page navigation
/// and HTML extraction happen inside the concrete `BrowserDriver`
/// implementation (e.g. the `browserless` remote driver); this struct
/// only performs the protocol translation.
pub struct BrowserDriverAdapter {
    inner: Arc<dyn BrowserDriverTrait + Send + Sync>,
    pool: Arc<RuntimeLeasePool>,
    managed_processor_id: Option<String>,
    /// The heavy lane (WS13.1): permits for requests that hold the browser
    /// longer than a plain fetch. Sized from `QUARRY_SCRAPE_HEAVY_PERMITS`.
    heavy: Arc<Semaphore>,
    /// Whether every fetch gets its own session key (WS13.1), read from
    /// `QUARRY_SCRAPE_SESSION_ISOLATION` when the adapter is built.
    session_isolation: bool,
    /// WS13.6: whether a full-page screenshot may stitch tiles, read from
    /// `QUARRY_EDGE__SCRAPE_STITCH` when the adapter is built. Off by default.
    scrape_stitch: bool,
}

/// The browser session and pool slot of one fetch, held until the fetch
/// gives them back. A fetch that is cancelled (the HTTP request was dropped,
/// or a caller's timeout dropped the future) never reaches its own release,
/// so the drop releases the session on the runtime and keeps the pool slot
/// until that release has run: the page, the context and the slot never
/// outlive the fetch.
struct HeldSession {
    inner: Arc<dyn BrowserDriverTrait + Send + Sync>,
    session: Option<BrowserSession>,
    lease: Option<LeaseGuard>,
}

impl HeldSession {
    fn session(&self) -> &BrowserSession {
        self.session.as_ref().expect("held until finish")
    }

    /// Release the session and give the slot back. `failed` (or a failed
    /// release) poisons the lease so the pool does not hand it out again.
    ///
    /// The release runs on its own task and this future only awaits the
    /// join handle: a fetch cancelled WHILE the release is running (the
    /// release can take seconds in Chromium) cannot abort it half way, and
    /// the slot comes back only once the release is over. The guard has
    /// already handed session and lease to that task, so its own drop has
    /// nothing left to do.
    async fn finish(mut self, failed: bool) -> QuarryResult<()> {
        let session = self.session.take().expect("held until finish");
        let lease = self.lease.take();
        let inner = self.inner.clone();
        // The driver's release runs on a task of its own inside the outer
        // task, so that a driver that panics is seen here as a join error
        // (the release failed) and not as an unwind through the lease guard,
        // which would give the slot back unpoisoned.
        let release = tokio::spawn(async move {
            let mut lease = lease;
            let released = match tokio::spawn(async move { inner.release(session).await }).await {
                Ok(released) => released,
                Err(_) => Err(QuarryError::new(
                    ErrorCode::Internal,
                    "browser session release panicked",
                )),
            };
            if failed || released.is_err() {
                if let Some(lease) = lease.as_mut() {
                    lease.poison();
                }
            }
            drop(lease);
            released
        });
        match release.await {
            Ok(released) => released,
            Err(_) => Err(QuarryError::new(
                ErrorCode::Internal,
                "browser session release task failed",
            )),
        }
    }
}

impl Drop for HeldSession {
    fn drop(&mut self) {
        let Some(session) = self.session.take() else {
            return;
        };
        // Cancelled mid-fetch: the session's state is unknown, so the lease
        // is not reused.
        let mut lease = self.lease.take();
        if let Some(lease) = lease.as_mut() {
            lease.poison();
        }
        let inner = self.inner.clone();
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                runtime.spawn(async move {
                    if let Err(error) = inner.release(session).await {
                        tracing::debug!(
                            error_class = error.code.wire_name(),
                            "browser fetch cancelled: session release failed"
                        );
                    }
                    drop(lease);
                });
            }
            Err(_) => {
                tracing::warn!("browser fetch dropped outside a runtime: session not released");
            }
        }
    }
}

impl BrowserDriverAdapter {
    pub fn new(
        inner: Arc<dyn BrowserDriverTrait + Send + Sync>,
        pool: Arc<RuntimeLeasePool>,
    ) -> Self {
        Self {
            heavy: heavy_lane(&pool),
            session_isolation: session_isolation_enabled(),
            scrape_stitch: scrape_stitch_enabled(),
            inner,
            pool,
            managed_processor_id: None,
        }
    }

    pub fn managed_provider(
        inner: Arc<dyn BrowserDriverTrait + Send + Sync>,
        pool: Arc<RuntimeLeasePool>,
        processor_id: impl Into<String>,
    ) -> Self {
        Self {
            heavy: heavy_lane(&pool),
            session_isolation: session_isolation_enabled(),
            scrape_stitch: scrape_stitch_enabled(),
            inner,
            pool,
            managed_processor_id: Some(processor_id.into()),
        }
    }

    /// Whether each fetch gets its own session key (WS13.1).
    pub fn session_isolation(&self) -> bool {
        self.session_isolation
    }

    /// Set session isolation explicitly, overriding the environment read when
    /// the adapter was built.
    pub fn with_session_isolation(mut self, enabled: bool) -> Self {
        self.session_isolation = enabled;
        self
    }

    /// Whether a full-page screenshot may stitch tiles (WS13.6).
    pub fn scrape_stitch(&self) -> bool {
        self.scrape_stitch
    }

    /// Set tile-and-stitch explicitly, overriding the environment read when the
    /// adapter was built (WS13.6).
    pub fn with_scrape_stitch(mut self, enabled: bool) -> Self {
        self.scrape_stitch = enabled;
        self
    }

    /// Rebuild the heavy lane with `permits` (WS13.1), clamped to the pool size.
    pub fn with_heavy_permits(mut self, permits: usize) -> Self {
        let permits = permits.clamp(1, self.pool.max_concurrent().max(1));
        self.heavy = Arc::new(Semaphore::new(permits));
        self
    }

    /// Heavy-lane permits free right now (WS13.1). The actions and capture
    /// slices (WS13.4, WS13.4a) take them; nothing takes them in this slice.
    pub fn heavy_permits_available(&self) -> usize {
        self.heavy.available_permits()
    }

    /// WS13.6: a heavy-lane permit for one stitched screenshot. Taken only when
    /// stitching is on, the screenshot is full-page, and the page is taller than
    /// one capture. A busy lane gives none, and the capture is then cropped at the
    /// limit (the opposite of an action, which waits).
    async fn stitch_permit_for(
        &self,
        session: &BrowserSession,
        plan: &CapturePlan,
    ) -> Option<StitchPermit> {
        if !self.scrape_stitch {
            return None;
        }
        let options = plan.screenshot.as_ref()?;
        if !self
            .inner
            .screenshot_needs_stitch(session, options)
            .await
            .unwrap_or(false)
        {
            return None;
        }
        self.heavy
            .clone()
            .try_acquire_owned()
            .ok()
            .map(StitchPermit::new)
    }
}

#[async_trait]
impl Driver for BrowserDriverAdapter {
    fn kind(&self) -> DriverKind {
        DriverKind::Browser
    }

    fn scrape_capabilities(&self) -> ScrapeCapabilities {
        let caps = self.inner.capabilities();
        ScrapeCapabilities {
            actions: caps.scrape_actions,
            capture: caps.capture_options,
        }
    }

    async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
        self.do_fetch(url, &FetchHints::default()).await
    }

    async fn fetch_conditional(
        &self,
        url: &Url,
        hints: &FetchHints,
    ) -> QuarryResult<FetchResponse> {
        self.do_fetch(url, hints).await
    }

    async fn browser_health(&self) -> Option<quarry_browser::BrowserHealth> {
        Some(self.inner.health().await)
    }
}

impl BrowserDriverAdapter {
    /// Shared fetch path. Honours `render.wait_for_selector` between
    /// goto and content; cache-validator hints (`if_none_match` /
    /// `if_modified_since`) are intentionally NOT propagated — browser
    /// navigation has no clean way to send conditional headers and the
    /// caller falls back to a full render.
    async fn do_fetch(&self, url: &Url, hints: &FetchHints) -> QuarryResult<FetchResponse> {
        if let Some(processor_id) = self.managed_processor_id.as_deref() {
            hints
                .privacy
                .guard_third_party_processing("browser", processor_id)?;
        }
        // WS13.4b: a capture needs a browser that can take one. Refused before
        // the pool is touched, so the refusal costs no lease and no session.
        if hints.render.capture.is_some() && !self.inner.capabilities().capture_options {
            return Err(capture_unavailable());
        }
        // WS13.5: actions need a browser that can run them. Refused before the pool is
        // touched, like a capture.
        if hints.render.actions.is_some() && !self.inner.capabilities().scrape_actions {
            return Err(actions_unavailable());
        }
        // WS13.5: a request with actions holds one heavy-lane permit for the whole page
        // run. A busy lane is a 429 at once, never a queue (a capture crops instead).
        let _heavy_permit = if hints.render.actions.is_some() {
            Some(self.heavy.clone().try_acquire_owned().map_err(|_| heavy_browser_busy())?)
        } else {
            None
        };
        let key = url.host_str().unwrap_or("default").to_string();
        let mut guard = self
            .pool
            .acquire_timeout_for_org(&hints.org_id, &key, LEASE_ACQUIRE_TIMEOUT)
            .await?;

        // The pooled lease is shared by host across callers, so the tenant is
        // stamped on this call's copy rather than on the pooled one.
        let mut call_lease = if self.session_isolation {
            // WS13.1: a fresh lease id per fetch, so every fetch gets its own
            // browser context and cookie jar, even for the same host and across
            // organizations. Nothing is seeded under the host key, and the guard
            // gives back no lease when it drops.
            let _ = guard.take_lease();
            isolated_lease(&key, &hints.org_id)
        } else {
            if guard.lease.is_none() {
                let lease = BrowserLease {
                    lease_id: kinds::LeaseKind::new(),
                    profile_id: kinds::ProfileKind::new(),
                    session_affinity_key: key.clone(),
                    proxy_affinity: ProxyAffinity {
                        pool: "default".into(),
                        sticky_key: None,
                    },
                    ttl_s: 30,
                    capabilities: vec![],
                    artifact_bucket: "default".into(),
                    persist_profile: false,
                    viewport: None,
                    // Minted on the fetch path. The pooled copy is shared by
                    // host, so the tenant is stamped on each call's copy below;
                    // this value is the first caller's and is never read for
                    // another call.
                    org_id: hints.org_id.clone(),
                };
                self.pool
                    .seed(lease.session_affinity_key.clone(), lease.clone());
                guard.lease = Some(lease);
            }
            guard.lease.as_ref().expect("lease set above").clone()
        };
        call_lease.org_id = hints.org_id.clone();
        let lease_ref = &call_lease;
        let start = Instant::now();

        let session = match self.inner.acquire(lease_ref).await {
            Ok(s) => s,
            Err(e) => {
                guard.poison();
                return Err(e);
            }
        };
        // From here on the session and the slot are released on every path,
        // a cancelled fetch included.
        let held = HeldSession {
            inner: self.inner.clone(),
            session: Some(session),
            lease: Some(guard),
        };
        let session = held.session();

        // The ordinary scrape pipeline is not an exemption from Quarry's
        // browser egress authority. A fresh Chromium context begins deny-all;
        // derive the narrow public-fetch grant from the already-parsed request
        // URL before the first navigation so redirects and subresources remain
        // behind the same DNS-pinning/proof boundary as agent runs.
        let allowed_domains = url
            .host_str()
            .map(|host| vec![host.to_owned()])
            .unwrap_or_default();
        if let Err(e) = self
            .inner
            .configure_egress_policy(
                session,
                BrowserEgressPolicy::from_allowed_domains(&allowed_domains),
            )
            .await
        {
            let _ = held.finish(true).await;
            return Err(e);
        }

        if let Err(e) = self.inner.goto(session, url.as_str()).await {
            let _ = held.finish(true).await;
            return Err(e);
        }

        // Optional wait_for between goto and content. A wait_for
        // failure is fatal — the caller asked us to block on the
        // selector before snapshotting, so returning partial DOM would
        // silently violate that contract. The session is poisoned so
        // the pool doesn't hand it to the next caller.
        if let Some(selector) = hints.render.wait_for_selector.as_deref() {
            let timeout = hints
                .render
                .wait_for_timeout_ms
                .unwrap_or(DEFAULT_WAIT_FOR_TIMEOUT_MS);
            if let Err(e) = self.inner.wait_for(session, selector, timeout).await {
                let _ = held.finish(true).await;
                return Err(e);
            }
        }

        // WS13.5: scrape-time actions run on the page the fetch opened, after the
        // selector wait and before the page is read. Under `abort` a failure releases
        // the session and fails the fetch with the ordered outcomes; under `continue`
        // the outcomes ride the captures and the run goes on. The page-level cancel
        // token is never fired here: dropping the fetch future releases the session.
        let mut actions_ran: Option<ScrapeActionRun> = None;
        if let Some(plan) = hints.render.actions.as_deref() {
            let limits = ActionLimits::default();
            let budget = ActionBudget {
                deadline: Instant::now() + hints.render.actions_deadline.unwrap_or(ACTIONS_DEADLINE),
                max_total_wait_ms: limits.max_total_wait_ms,
                max_single_wait_ms: limits.max_single_wait_ms,
                default_wait_for_ms: limits.default_wait_for_ms,
                capture_limits: hints.render.capture_limits.clone(),
            };
            let attached = ActionRuntime::attached(self.inner.clone(), session.clone());
            let cancel = CancellationToken::new();
            let run = match attached.run_scrape(plan, &budget, &cancel).await {
                Ok(run) => run,
                Err(e) => {
                    let _ = held.finish(true).await;
                    return Err(e);
                }
            };
            if let Some(index) = run.aborted_at {
                let _ = held.finish(true).await;
                return Err(action_failed(&run, index));
            }
            actions_ran = Some(run);
        }

        let mut body_bytes = match self.inner.content(session).await {
            Ok(b) => b,
            Err(e) => {
                let _ = held.finish(true).await;
                return Err(e);
            }
        };

        // Hydration settle (only without an explicit wait_for, which already
        // defines "ready"). Re-snapshot while the DOM still looks like an
        // unhydrated shell; stop once the visible text stabilises or the
        // budget is spent. Snapshot errors here are non-fatal — we keep the
        // last good body.
        if hints.render.wait_for_selector.is_none() && looks_unhydrated(&body_bytes) {
            let budget = settle_budget();
            let deadline = Instant::now() + budget;
            let mut last_text = crate::fallback_driver::visible_text_len(&body_bytes);
            let mut polls = 0u32;
            while Instant::now() < deadline {
                tokio::time::sleep(SETTLE_POLL).await;
                polls += 1;
                let Ok(next) = self.inner.content(session).await else {
                    break;
                };
                let text = crate::fallback_driver::visible_text_len(&next);
                let grew = text > last_text;
                if text >= last_text {
                    body_bytes = next;
                }
                if !grew && text >= SETTLE_MIN_TEXT_CHARS {
                    break;
                }
                last_text = last_text.max(text);
            }
            tracing::info!(
                url = %log_url(url.as_str()),
                polls,
                budget_ms = budget.as_millis() as u64,
                text_chars = last_text,
                body_bytes = body_bytes.len(),
                hydrated = last_text >= SETTLE_MIN_TEXT_CHARS,
                "browser hydration settle"
            );
        }

        // WS13.4b: capture what the request asked for while the page is still
        // open, before the session is released.
        let mut captures = match hints.render.capture.as_ref() {
            Some(plan) => {
                // WS13.6: a stitch permit only when the page needs one and the heavy lane is free.
                let stitch = self.stitch_permit_for(session, plan).await;
                Some(Box::new(
                    take_captures(
                        self.inner.as_ref(),
                        session,
                        plan,
                        &hints.render.capture_limits,
                        url,
                        stitch.as_ref(),
                    )
                    .await,
                ))
            }
            None => None,
        };

        // WS13.5: the ordered action outcomes and their output bytes ride the captures,
        // and the final URL is the page the actions left the browser on.
        let final_url = match actions_ran.take() {
            Some(run) => {
                let caps = captures.get_or_insert_with(|| Box::new(PageCaptures::default()));
                if run.outcomes.iter().any(|o| o.status == ActionStatus::Failed) {
                    caps.warnings.push(CaptureWarning {
                        code: "action_failed".into(),
                        message: "an action failed and the run went on (continue)".into(),
                    });
                }
                caps.actions = run.outcomes;
                caps.action_outputs = run.outputs;
                // WS13.5: a run without a capture plan never reaches take_captures, so its
                // blocked requests are counted here, as a capture would count them.
                if hints.render.capture.is_none() {
                    caps.blocked_requests =
                        count_blocked_requests(self.inner.as_ref(), session).await;
                }
                match self.inner.current_url(session).await {
                    Ok(Some(raw)) => raw.parse::<Url>().unwrap_or_else(|_| url.clone()),
                    _ => url.clone(),
                }
            }
            None => url.clone(),
        };

        let response = FetchResponse {
            status: 200,
            final_url,
            // A DOM snapshot is UTF-8 by construction (CDP hands it over
            // as a JSON string), but this response used to carry no
            // headers at all — leaving the pipeline's charset decode with
            // nothing but a sniff, which can mis-guess a legacy encoding
            // and mojibake the whole page. Declare what we know. If a
            // backend ever returned non-UTF-8 bytes anyway, decode()'s
            // sanity check falls through to sniffing regardless.
            headers: vec![(
                "content-type".to_string(),
                "text/html; charset=utf-8".to_string(),
            )],
            body: body_bytes.to_vec(),
            duration_ms: start.elapsed().as_millis() as u64,
            served_by: DriverKind::Browser,
            captures,
        };

        held.finish(false).await?;

        Ok(response)
    }
}

/// WS13.5: the longest a scrape-time action run may take when the caller set no
/// deadline (the router allows 125 s with actions on; the handler bounds at 115 s).
const ACTIONS_DEADLINE: Duration = Duration::from_secs(115);

/// WS13.5: the typed refusal for actions on a browser that cannot run them.
fn actions_unavailable() -> QuarryError {
    QuarryError::new(
        ErrorCode::Unsupported,
        "this browser cannot run scrape actions",
    )
    .with_details(serde_json::json!({ "reason": "browser_capability_unavailable" }))
}

/// WS13.5: the heavy lane is held by another action run. Same 429 body as a rate limit.
fn heavy_browser_busy() -> QuarryError {
    QuarryError::new(
        ErrorCode::RateLimited,
        "the browser is busy with another action run; try again shortly",
    )
    .with_details(serde_json::json!({ "reason": "heavy_browser_busy" }))
}

/// WS13.5: an action failed under `abort`. The details carry every outcome in order,
/// so the caller sees which action stopped the run and what was skipped. Never a
/// driver message or a typed text.
fn action_failed(run: &ScrapeActionRun, index: usize) -> QuarryError {
    let action_type = run.outcomes.get(index).map_or("unknown", |o| o.kind.as_str());
    QuarryError::new(ErrorCode::DriverFailed, "a scrape action failed").with_details(
        serde_json::json!({
            "reason": "action_failed",
            "action_index": index,
            "action_type": action_type,
            "actions": run.outcomes,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use quarry_browser::{BrowserSession, SessionInner};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Mutex as TokioMutex;

    struct MockBrowserDriver {
        acquire_count: AtomicUsize,
        goto_count: AtomicUsize,
        content_count: AtomicUsize,
        release_count: AtomicUsize,
        wait_for_count: AtomicUsize,
        call_order: TokioMutex<Vec<&'static str>>,
        lease_keys: TokioMutex<Vec<String>>,
        lease_orgs: TokioMutex<Vec<String>>,
        last_url: TokioMutex<Option<String>>,
        last_wait_for: TokioMutex<Option<(String, u32)>>,
        body: Bytes,
    }

    impl MockBrowserDriver {
        fn new(body: &'static [u8]) -> Self {
            Self {
                acquire_count: AtomicUsize::new(0),
                goto_count: AtomicUsize::new(0),
                content_count: AtomicUsize::new(0),
                release_count: AtomicUsize::new(0),
                wait_for_count: AtomicUsize::new(0),
                call_order: TokioMutex::new(Vec::new()),
                lease_keys: TokioMutex::new(Vec::new()),
                lease_orgs: TokioMutex::new(Vec::new()),
                last_url: TokioMutex::new(None),
                last_wait_for: TokioMutex::new(None),
                body: Bytes::from_static(body),
            }
        }
    }

    #[async_trait]
    impl BrowserDriverTrait for MockBrowserDriver {
        async fn configure_egress_policy(
            &self,
            _session: &BrowserSession,
            _policy: BrowserEgressPolicy,
        ) -> QuarryResult<()> {
            Ok(())
        }
        async fn acquire(&self, lease: &BrowserLease) -> QuarryResult<BrowserSession> {
            self.acquire_count.fetch_add(1, Ordering::SeqCst);
            self.call_order.lock().await.push("acquire");
            self.lease_keys
                .lock()
                .await
                .push(lease.session_affinity_key.clone());
            self.lease_orgs.lock().await.push(lease.org_id.clone());
            Ok(BrowserSession {
                lease: lease.clone(),
                tab_pin: None,
                inner: Arc::new(TokioMutex::new(SessionInner::default())),
            })
        }
        async fn release(&self, _session: BrowserSession) -> QuarryResult<()> {
            self.release_count.fetch_add(1, Ordering::SeqCst);
            self.call_order.lock().await.push("release");
            Ok(())
        }
        async fn goto(&self, _session: &BrowserSession, url: &str) -> QuarryResult<()> {
            self.goto_count.fetch_add(1, Ordering::SeqCst);
            *self.last_url.lock().await = Some(url.to_string());
            self.call_order.lock().await.push("goto");
            Ok(())
        }
        async fn content(&self, _session: &BrowserSession) -> QuarryResult<Bytes> {
            self.content_count.fetch_add(1, Ordering::SeqCst);
            self.call_order.lock().await.push("content");
            Ok(self.body.clone())
        }
        async fn screenshot(
            &self,
            _session: &BrowserSession,
            _full_page: bool,
        ) -> QuarryResult<Bytes> {
            Ok(Bytes::new())
        }
        async fn pdf(&self, _session: &BrowserSession) -> QuarryResult<Bytes> {
            Ok(Bytes::new())
        }
        async fn wait_for(
            &self,
            _session: &BrowserSession,
            selector: &str,
            timeout_ms: u32,
        ) -> QuarryResult<()> {
            self.wait_for_count.fetch_add(1, Ordering::SeqCst);
            *self.last_wait_for.lock().await = Some((selector.to_string(), timeout_ms));
            self.call_order.lock().await.push("wait_for");
            Ok(())
        }
    }

    #[tokio::test]
    async fn kind_is_browser() {
        let mock = Arc::new(MockBrowserDriver::new(b""));
        let pool = Arc::new(RuntimeLeasePool::new(4));
        let adapter = BrowserDriverAdapter::new(mock, pool);
        assert_eq!(adapter.kind(), DriverKind::Browser);
    }

    #[tokio::test]
    async fn fetch_returns_200_with_body() {
        let mock = Arc::new(MockBrowserDriver::new(b"<html>hello</html>"));
        let pool = Arc::new(RuntimeLeasePool::new(4));
        let adapter = BrowserDriverAdapter::new(mock, pool);
        let url = Url::parse("https://example.com/page").unwrap();
        let resp = adapter.fetch(&url).await.unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"<html>hello</html>".to_vec());
    }

    #[tokio::test]
    async fn fetch_declares_the_snapshot_charset() {
        // The DOM snapshot is UTF-8 by construction. Without this header
        // the pipeline can only charset-sniff the body, and a sniff that
        // mis-guesses a legacy encoding mojibakes every non-ASCII char
        // on the page (observed live: titles with "på" → "pÃ¥").
        let mock = Arc::new(MockBrowserDriver::new(
            "<html><title>p\u{e5} norsk</title></html>".as_bytes(),
        ));
        let pool = Arc::new(RuntimeLeasePool::new(4));
        let adapter = BrowserDriverAdapter::new(mock, pool);
        let url = Url::parse("https://example.com/page").unwrap();
        let resp = adapter.fetch(&url).await.unwrap();
        let ct = resp
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            .map(|(_, v)| v.as_str());
        assert_eq!(ct, Some("text/html; charset=utf-8"));
    }

    #[test]
    fn script_less_bodies_never_wait_for_hydration() {
        assert!(!looks_unhydrated(b"x"));
        assert!(!looks_unhydrated(b"<html><body><p>hei</p></body></html>"));
    }

    #[test]
    fn sparse_script_bearing_bodies_wait_for_hydration() {
        let shell = b"<html><head><script src=\"/_next/static/a.js\"></script></head><body><div id=\"__next\"></div></body></html>";
        assert!(looks_unhydrated(shell));
        let mut hydrated = b"<html><head><script>1</script></head><body>".to_vec();
        hydrated.extend_from_slice("<p>ord </p>".repeat(200).as_bytes());
        hydrated.extend_from_slice(b"</body></html>");
        assert!(!looks_unhydrated(&hydrated));
    }

    #[tokio::test]
    async fn fetch_preserves_final_url() {
        let mock = Arc::new(MockBrowserDriver::new(b"x"));
        let pool = Arc::new(RuntimeLeasePool::new(4));
        let adapter = BrowserDriverAdapter::new(mock, pool);
        let url = Url::parse("https://example.com/path?q=1").unwrap();
        let resp = adapter.fetch(&url).await.unwrap();
        assert_eq!(resp.final_url, url);
    }

    #[tokio::test]
    async fn fetch_calls_in_order() {
        let mock = Arc::new(MockBrowserDriver::new(b"x"));
        let pool = Arc::new(RuntimeLeasePool::new(4));
        let adapter = BrowserDriverAdapter::new(mock.clone(), pool);
        let url = Url::parse("https://example.com/").unwrap();
        adapter.fetch(&url).await.unwrap();

        assert_eq!(mock.acquire_count.load(Ordering::SeqCst), 1);
        assert_eq!(mock.goto_count.load(Ordering::SeqCst), 1);
        assert_eq!(mock.content_count.load(Ordering::SeqCst), 1);
        assert_eq!(mock.release_count.load(Ordering::SeqCst), 1);

        let order = mock.call_order.lock().await.clone();
        assert_eq!(order, vec!["acquire", "goto", "content", "release"]);

        let last_url = mock.last_url.lock().await.clone();
        assert_eq!(last_url, Some("https://example.com/".to_string()));
    }

    #[tokio::test]
    async fn fetch_conditional_without_render_hints_skips_wait_for() {
        let mock = Arc::new(MockBrowserDriver::new(b"x"));
        let pool = Arc::new(RuntimeLeasePool::new(4));
        let adapter = BrowserDriverAdapter::new(mock.clone(), pool);
        let url = Url::parse("https://example.com/").unwrap();
        adapter
            .fetch_conditional(&url, &FetchHints::default())
            .await
            .unwrap();

        assert_eq!(mock.wait_for_count.load(Ordering::SeqCst), 0);
        let order = mock.call_order.lock().await.clone();
        assert_eq!(order, vec!["acquire", "goto", "content", "release"]);
    }

    #[tokio::test]
    async fn fetch_conditional_invokes_wait_for_when_selector_set() {
        use crate::driver::RenderHints;
        let mock = Arc::new(MockBrowserDriver::new(b"x"));
        let pool = Arc::new(RuntimeLeasePool::new(4));
        let adapter = BrowserDriverAdapter::new(mock.clone(), pool);
        let url = Url::parse("https://example.com/").unwrap();
        let hints = FetchHints {
            render: RenderHints {
                wait_for_selector: Some("#hydrated".into()),
                wait_for_timeout_ms: Some(2_500),
                ..RenderHints::default()
            },
            ..FetchHints::default()
        };
        adapter.fetch_conditional(&url, &hints).await.unwrap();

        assert_eq!(mock.wait_for_count.load(Ordering::SeqCst), 1);
        let last = mock.last_wait_for.lock().await.clone();
        assert_eq!(last, Some(("#hydrated".to_string(), 2_500)));

        // Ordering: wait_for must run after goto and before content so
        // the DOM is settled when we snapshot.
        let order = mock.call_order.lock().await.clone();
        assert_eq!(
            order,
            vec!["acquire", "goto", "wait_for", "content", "release"]
        );
    }

    #[tokio::test]
    async fn fetch_conditional_uses_default_timeout_when_unset() {
        use crate::driver::RenderHints;
        let mock = Arc::new(MockBrowserDriver::new(b"x"));
        let pool = Arc::new(RuntimeLeasePool::new(4));
        let adapter = BrowserDriverAdapter::new(mock.clone(), pool);
        let url = Url::parse("https://example.com/").unwrap();
        let hints = FetchHints {
            render: RenderHints {
                wait_for_selector: Some(".ready".into()),
                wait_for_timeout_ms: None,
                ..RenderHints::default()
            },
            ..FetchHints::default()
        };
        adapter.fetch_conditional(&url, &hints).await.unwrap();

        let last = mock.last_wait_for.lock().await.clone();
        assert_eq!(
            last,
            Some((".ready".to_string(), DEFAULT_WAIT_FOR_TIMEOUT_MS))
        );
    }

    /// A driver whose `goto` never returns: the fetch can only end by being
    /// dropped. It records each release and how many pool slots were free
    /// when the release ran.
    struct StallingDriver {
        entered: tokio::sync::Notify,
        releases: AtomicUsize,
        free_slots_at_release: std::sync::Mutex<Vec<usize>>,
        pool: std::sync::OnceLock<Arc<RuntimeLeasePool>>,
    }

    #[async_trait]
    impl BrowserDriverTrait for StallingDriver {
        async fn configure_egress_policy(
            &self,
            _session: &BrowserSession,
            _policy: BrowserEgressPolicy,
        ) -> QuarryResult<()> {
            Ok(())
        }
        async fn acquire(&self, lease: &BrowserLease) -> QuarryResult<BrowserSession> {
            Ok(BrowserSession {
                lease: lease.clone(),
                tab_pin: None,
                inner: Arc::new(TokioMutex::new(SessionInner::default())),
            })
        }
        async fn release(&self, _session: BrowserSession) -> QuarryResult<()> {
            let free = self.pool.get().map(|pool| pool.available()).unwrap_or(usize::MAX);
            self.free_slots_at_release.lock().unwrap().push(free);
            self.releases.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        async fn goto(&self, _session: &BrowserSession, _url: &str) -> QuarryResult<()> {
            self.entered.notify_one();
            std::future::pending::<()>().await;
            Ok(())
        }
        async fn content(&self, _session: &BrowserSession) -> QuarryResult<Bytes> {
            Ok(Bytes::new())
        }
        async fn screenshot(
            &self,
            _session: &BrowserSession,
            _full_page: bool,
        ) -> QuarryResult<Bytes> {
            Ok(Bytes::new())
        }
        async fn pdf(&self, _session: &BrowserSession) -> QuarryResult<Bytes> {
            Ok(Bytes::new())
        }
        async fn wait_for(
            &self,
            _session: &BrowserSession,
            _selector: &str,
            _timeout_ms: u32,
        ) -> QuarryResult<()> {
            Ok(())
        }
    }

    /// A fetch cancelled mid-navigation (the HTTP request was dropped, or a
    /// caller's timeout dropped the future) releases its browser session,
    /// and its pool slot comes back only after that release ran.
    #[tokio::test]
    async fn cancelled_fetch_releases_the_session_and_the_slot() {
        let driver = Arc::new(StallingDriver {
            entered: tokio::sync::Notify::new(),
            releases: AtomicUsize::new(0),
            free_slots_at_release: std::sync::Mutex::new(Vec::new()),
            pool: std::sync::OnceLock::new(),
        });
        let pool = Arc::new(RuntimeLeasePool::new(1));
        driver.pool.set(pool.clone()).ok().expect("set once");
        let adapter = Arc::new(BrowserDriverAdapter::new(driver.clone(), pool.clone()));
        let fetching = {
            let adapter = adapter.clone();
            tokio::spawn(async move {
                let url = Url::parse("https://example.com/").unwrap();
                adapter.fetch(&url).await
            })
        };
        tokio::time::timeout(Duration::from_secs(5), driver.entered.notified())
            .await
            .expect("the fetch reached goto");
        assert_eq!(pool.available(), 0, "the fetch holds the only slot");

        fetching.abort();
        assert!(fetching.await.expect_err("aborted").is_cancelled());

        let released = tokio::time::timeout(Duration::from_secs(5), async {
            while driver.releases.load(Ordering::SeqCst) == 0 || pool.available() == 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
        assert!(released.is_ok(), "the cancelled fetch released its session and slot");
        assert_eq!(driver.releases.load(Ordering::SeqCst), 1, "released exactly once");
        assert_eq!(
            *driver.free_slots_at_release.lock().unwrap(),
            vec![0],
            "the slot was still held while the session was released"
        );
        assert_eq!(pool.available(), 1);
    }

    /// A driver whose `release` takes a while (closing a Chromium context
    /// can): the fetch itself is quick, so the only place to cancel it is
    /// inside the release.
    struct SlowReleaseDriver {
        release_started: tokio::sync::Notify,
        release_gate: tokio::sync::Notify,
        completed: AtomicUsize,
        free_slots_at_completion: std::sync::Mutex<Vec<usize>>,
        pool: std::sync::OnceLock<Arc<RuntimeLeasePool>>,
    }

    #[async_trait]
    impl BrowserDriverTrait for SlowReleaseDriver {
        async fn configure_egress_policy(
            &self,
            _session: &BrowserSession,
            _policy: BrowserEgressPolicy,
        ) -> QuarryResult<()> {
            Ok(())
        }
        async fn acquire(&self, lease: &BrowserLease) -> QuarryResult<BrowserSession> {
            Ok(BrowserSession {
                lease: lease.clone(),
                tab_pin: None,
                inner: Arc::new(TokioMutex::new(SessionInner::default())),
            })
        }
        async fn release(&self, _session: BrowserSession) -> QuarryResult<()> {
            self.release_started.notify_one();
            self.release_gate.notified().await;
            let free = self.pool.get().map(|pool| pool.available()).unwrap_or(usize::MAX);
            self.free_slots_at_completion.lock().unwrap().push(free);
            self.completed.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        async fn goto(&self, _session: &BrowserSession, _url: &str) -> QuarryResult<()> {
            Ok(())
        }
        async fn content(&self, _session: &BrowserSession) -> QuarryResult<Bytes> {
            Ok(Bytes::from_static(b"<html>ok</html>"))
        }
        async fn screenshot(
            &self,
            _session: &BrowserSession,
            _full_page: bool,
        ) -> QuarryResult<Bytes> {
            Ok(Bytes::new())
        }
        async fn pdf(&self, _session: &BrowserSession) -> QuarryResult<Bytes> {
            Ok(Bytes::new())
        }
        async fn wait_for(
            &self,
            _session: &BrowserSession,
            _selector: &str,
            _timeout_ms: u32,
        ) -> QuarryResult<()> {
            Ok(())
        }
    }

    /// A fetch cancelled while its final release is running does not abort
    /// that release: it runs to completion, and the pool slot comes back
    /// only after it.
    #[tokio::test]
    async fn a_fetch_cancelled_during_release_still_completes_the_release() {
        let driver = Arc::new(SlowReleaseDriver {
            release_started: tokio::sync::Notify::new(),
            release_gate: tokio::sync::Notify::new(),
            completed: AtomicUsize::new(0),
            free_slots_at_completion: std::sync::Mutex::new(Vec::new()),
            pool: std::sync::OnceLock::new(),
        });
        let pool = Arc::new(RuntimeLeasePool::new(1));
        driver.pool.set(pool.clone()).ok().expect("set once");
        let adapter = Arc::new(BrowserDriverAdapter::new(driver.clone(), pool.clone()));
        let fetching = {
            let adapter = adapter.clone();
            tokio::spawn(async move {
                let url = Url::parse("https://example.com/").unwrap();
                adapter.fetch(&url).await
            })
        };
        tokio::time::timeout(Duration::from_secs(5), driver.release_started.notified())
            .await
            .expect("the fetch reached its release");

        fetching.abort();
        assert!(fetching.await.expect_err("aborted").is_cancelled());
        // Cancelled in the middle of the release: the release is unfinished
        // and the slot is still held.
        assert_eq!(driver.completed.load(Ordering::SeqCst), 0);
        assert_eq!(pool.available(), 0, "the slot is held until the release is over");

        driver.release_gate.notify_one();
        let finished = tokio::time::timeout(Duration::from_secs(5), async {
            while driver.completed.load(Ordering::SeqCst) == 0 || pool.available() == 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
        assert!(finished.is_ok(), "the release ran to completion after the cancel");
        assert_eq!(driver.completed.load(Ordering::SeqCst), 1, "released exactly once");
        assert_eq!(
            *driver.free_slots_at_completion.lock().unwrap(),
            vec![0],
            "the slot was held until the release finished"
        );
        assert_eq!(pool.available(), 1);
    }

    /// A driver whose release panics.
    struct PanicReleaseDriver;

    #[async_trait]
    impl BrowserDriverTrait for PanicReleaseDriver {
        async fn configure_egress_policy(
            &self,
            _session: &BrowserSession,
            _policy: BrowserEgressPolicy,
        ) -> QuarryResult<()> {
            Ok(())
        }
        async fn acquire(&self, lease: &BrowserLease) -> QuarryResult<BrowserSession> {
            Ok(BrowserSession {
                lease: lease.clone(),
                tab_pin: None,
                inner: Arc::new(TokioMutex::new(SessionInner::default())),
            })
        }
        async fn release(&self, _session: BrowserSession) -> QuarryResult<()> {
            panic!("release exploded");
        }
        async fn goto(&self, _session: &BrowserSession, _url: &str) -> QuarryResult<()> {
            Ok(())
        }
        async fn content(&self, _session: &BrowserSession) -> QuarryResult<Bytes> {
            Ok(Bytes::from_static(b"<html>ok</html>"))
        }
        async fn screenshot(
            &self,
            _session: &BrowserSession,
            _full_page: bool,
        ) -> QuarryResult<Bytes> {
            Ok(Bytes::new())
        }
        async fn pdf(&self, _session: &BrowserSession) -> QuarryResult<Bytes> {
            Ok(Bytes::new())
        }
        async fn wait_for(
            &self,
            _session: &BrowserSession,
            _selector: &str,
            _timeout_ms: u32,
        ) -> QuarryResult<()> {
            Ok(())
        }
    }

    /// A driver whose release panics fails the fetch with an internal error,
    /// frees the slot, and evicts the lease: the slot does not go back to
    /// the pool as if the release had worked.
    #[tokio::test]
    async fn a_panicking_release_poisons_the_lease_and_frees_the_slot() {
        let pool = Arc::new(RuntimeLeasePool::new(1));
        let adapter = BrowserDriverAdapter::new(Arc::new(PanicReleaseDriver), pool.clone());
        let url = Url::parse("https://example.com/").unwrap();
        let err = adapter.fetch(&url).await.expect_err("the release panicked");
        assert_eq!(err.code, ErrorCode::Internal);
        let freed = tokio::time::timeout(Duration::from_secs(5), async {
            while pool.available() == 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
        assert!(freed.is_ok(), "the slot came back");
        let next = pool.try_acquire("example.com").expect("slot free");
        assert!(!next.has_lease(), "the lease was evicted, not returned to the pool");
    }

    /// A fetch that ends normally releases once and is not released again by
    /// the guard.
    #[tokio::test]
    async fn finished_fetch_releases_exactly_once() {
        let mock = Arc::new(MockBrowserDriver::new(b"x"));
        let pool = Arc::new(RuntimeLeasePool::new(1));
        let adapter = BrowserDriverAdapter::new(mock.clone(), pool.clone());
        let url = Url::parse("https://example.com/").unwrap();
        adapter.fetch(&url).await.unwrap();
        tokio::task::yield_now().await;
        assert_eq!(mock.release_count.load(Ordering::SeqCst), 1);
        assert_eq!(pool.available(), 1);
    }

    #[tokio::test]
    async fn managed_browser_provider_requires_processor_approval() {
        let mock = Arc::new(MockBrowserDriver::new(b"x"));
        let pool = Arc::new(RuntimeLeasePool::new(4));
        let adapter = BrowserDriverAdapter::managed_provider(mock.clone(), pool, "browserbase");
        let url = Url::parse("https://example.com/").unwrap();

        let err = adapter
            .fetch_conditional(&url, &FetchHints::default())
            .await
            .unwrap_err();

        assert_eq!(err.code, quarry_core::error::ErrorCode::Forbidden);
        assert_eq!(mock.acquire_count.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn managed_browser_provider_allows_matching_processor_policy() {
        let mock = Arc::new(MockBrowserDriver::new(b"x"));
        let pool = Arc::new(RuntimeLeasePool::new(4));
        let adapter = BrowserDriverAdapter::managed_provider(mock.clone(), pool, "browserbase");
        let url = Url::parse("https://example.com/").unwrap();
        let hints = FetchHints {
            privacy: quarry_core::privacy::PrivacyPolicy {
                allow_third_party_processing: true,
                processor_id: Some("browserbase".into()),
                ..quarry_core::privacy::PrivacyPolicy::default()
            },
            ..FetchHints::default()
        };

        adapter.fetch_conditional(&url, &hints).await.unwrap();

        assert_eq!(mock.acquire_count.load(Ordering::SeqCst), 1);
    }

    /// Serialises the tests that set `QUARRY_SCRAPE_SESSION_ISOLATION` (WS13.1).
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn host_url(path: &str) -> Url {
        Url::parse(&format!("https://example.com{path}")).unwrap()
    }

    #[tokio::test]
    async fn concurrent_same_host_fetches_use_distinct_session_keys() {
        let mock = Arc::new(MockBrowserDriver::new(b"x"));
        let pool = Arc::new(RuntimeLeasePool::new(4));
        let adapter = BrowserDriverAdapter::new(mock.clone(), pool).with_session_isolation(true);
        let url_a = host_url("/a");
        let url_b = host_url("/b");
        let (first, second) = tokio::join!(adapter.fetch(&url_a), adapter.fetch(&url_b));
        first.unwrap();
        second.unwrap();

        let keys = mock.lease_keys.lock().await.clone();
        assert_eq!(keys.len(), 2);
        assert_ne!(keys[0], keys[1], "same-host fetches shared a session key");
        assert!(keys.iter().all(|key| key.starts_with("example.com~")));
    }

    #[test]
    fn isolated_session_key_starts_with_the_host_and_ends_with_the_lease_id() {
        let lease_id = kinds::LeaseKind::new();
        let key = isolated_session_key("example.com", &lease_id);
        assert!(key.starts_with("example.com~"));
        assert!(key.ends_with(&lease_id.to_string()));
    }

    #[tokio::test]
    async fn isolated_lease_carries_the_request_org() {
        let mock = Arc::new(MockBrowserDriver::new(b"x"));
        let pool = Arc::new(RuntimeLeasePool::new(4));
        let adapter = BrowserDriverAdapter::new(mock.clone(), pool).with_session_isolation(true);
        let hints = FetchHints {
            org_id: "org_a".into(),
            ..FetchHints::default()
        };
        adapter
            .fetch_conditional(&host_url("/"), &hints)
            .await
            .unwrap();

        let orgs = mock.lease_orgs.lock().await.clone();
        assert_eq!(orgs, vec!["org_a".to_string()]);
    }

    #[tokio::test]
    async fn an_isolated_fetch_does_not_return_its_lease_to_the_pool() {
        let mock = Arc::new(MockBrowserDriver::new(b"x"));
        let pool = Arc::new(RuntimeLeasePool::new(4));
        let adapter =
            BrowserDriverAdapter::new(mock.clone(), pool.clone()).with_session_isolation(true);
        adapter.fetch(&host_url("/")).await.unwrap();

        let keys = mock.lease_keys.lock().await.clone();
        assert_eq!(keys.len(), 1);
        let by_session = pool.try_acquire(&keys[0]).unwrap();
        assert!(
            !by_session.has_lease(),
            "the isolated lease was returned to the pool"
        );
        let by_host = pool.try_acquire("example.com").unwrap();
        assert!(!by_host.has_lease(), "an isolated fetch seeded the host key");
    }

    #[tokio::test]
    async fn kill_switch_restores_the_host_affinity_key() {
        let mock = Arc::new(MockBrowserDriver::new(b"x"));
        let pool = Arc::new(RuntimeLeasePool::new(4));
        let adapter = {
            let _env = ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let previous = std::env::var(ENV_SESSION_ISOLATION).ok();
            std::env::set_var(ENV_SESSION_ISOLATION, "0");
            let adapter = BrowserDriverAdapter::new(mock.clone(), pool);
            match previous {
                Some(value) => std::env::set_var(ENV_SESSION_ISOLATION, value),
                None => std::env::remove_var(ENV_SESSION_ISOLATION),
            }
            adapter
        };
        assert!(!adapter.session_isolation());

        adapter.fetch(&host_url("/")).await.unwrap();
        adapter.fetch(&host_url("/")).await.unwrap();
        let keys = mock.lease_keys.lock().await.clone();
        assert_eq!(
            keys,
            vec!["example.com".to_string(), "example.com".to_string()]
        );
    }

    #[test]
    fn isolation_flag_is_on_unless_zero_or_false() {
        assert!(session_isolation_from(None));
        assert!(session_isolation_from(Some("")));
        assert!(session_isolation_from(Some("on")));
        assert!(!session_isolation_from(Some("0")));
        assert!(!session_isolation_from(Some(" False ")));
    }

    #[test]
    fn heavy_lane_defaults_to_one_and_clamps_to_the_pool() {
        assert_eq!(heavy_permits_from(None, 4), 1);
        assert_eq!(heavy_permits_from(Some("nope"), 4), 1);
        assert_eq!(heavy_permits_from(Some("3"), 4), 3);
        assert_eq!(heavy_permits_from(Some("9"), 4), 4);
        assert_eq!(heavy_permits_from(Some("0"), 4), 1);
    }
}

#[cfg(test)]
mod capture_tests {
    use super::*;
    use crate::driver::RenderHints;
    use bytes::Bytes;
    use quarry_browser::{BrowserSession, SessionInner};
    use quarry_core::capture::{
        CaptureLimits, CapturePlan, CapturedImage, CapturedPdf, ImageFormat, PdfOptions,
        ScreenshotOptions,
    };
    use quarry_core::contracts::BrowserEgressReceipt;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Mutex as TokioMutex;

    /// A browser that records the order of calls. Captures can be made to
    /// fail, the browser can be told it cannot capture, and it reports a
    /// number of blocked requests.
    struct CaptureMock {
        capture_options: bool,
        fail_screenshot: bool,
        blocked: usize,
        acquire_count: AtomicUsize,
        call_order: TokioMutex<Vec<&'static str>>,
    }

    impl CaptureMock {
        fn new(capture_options: bool, fail_screenshot: bool, blocked: usize) -> Self {
            Self {
                capture_options,
                fail_screenshot,
                blocked,
                acquire_count: AtomicUsize::new(0),
                call_order: TokioMutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl BrowserDriverTrait for CaptureMock {
        fn capabilities(&self) -> quarry_browser::BrowserDriverCapabilities {
            quarry_browser::BrowserDriverCapabilities {
                capture_options: self.capture_options,
                ..Default::default()
            }
        }
        async fn configure_egress_policy(
            &self,
            _session: &BrowserSession,
            _policy: BrowserEgressPolicy,
        ) -> QuarryResult<()> {
            self.call_order.lock().await.push("configure_egress_policy");
            Ok(())
        }
        async fn acquire(&self, lease: &BrowserLease) -> QuarryResult<BrowserSession> {
            self.acquire_count.fetch_add(1, Ordering::SeqCst);
            self.call_order.lock().await.push("acquire");
            Ok(BrowserSession {
                lease: lease.clone(),
                tab_pin: None,
                inner: Arc::new(TokioMutex::new(SessionInner::default())),
            })
        }
        async fn release(&self, _session: BrowserSession) -> QuarryResult<()> {
            self.call_order.lock().await.push("release");
            Ok(())
        }
        async fn goto(&self, _session: &BrowserSession, _url: &str) -> QuarryResult<()> {
            self.call_order.lock().await.push("goto");
            Ok(())
        }
        async fn content(&self, _session: &BrowserSession) -> QuarryResult<Bytes> {
            self.call_order.lock().await.push("content");
            Ok(Bytes::from_static(b"<html><body>ok</body></html>"))
        }
        async fn screenshot(&self, _session: &BrowserSession, _full_page: bool) -> QuarryResult<Bytes> {
            Ok(Bytes::new())
        }
        async fn pdf(&self, _session: &BrowserSession) -> QuarryResult<Bytes> {
            Ok(Bytes::new())
        }
        async fn wait_for(
            &self,
            _session: &BrowserSession,
            _selector: &str,
            _timeout_ms: u32,
        ) -> QuarryResult<()> {
            Ok(())
        }
        async fn capture_screenshot(
            &self,
            _session: &BrowserSession,
            _options: &ScreenshotOptions,
            _limits: &CaptureLimits,
        ) -> QuarryResult<CapturedImage> {
            self.call_order.lock().await.push("capture_screenshot");
            if self.fail_screenshot {
                return Err(QuarryError::new(ErrorCode::DriverFailed, "raw page text")
                    .with_details(serde_json::json!({ "reason": "capture_too_large" })));
            }
            Ok(CapturedImage {
                bytes: vec![0x89, b'P', b'N', b'G'],
                format: ImageFormat::Png,
                width_px: 800,
                height_px: 600,
                tiles: 1,
                stitched: false,
                truncated: false,
            })
        }
        async fn capture_pdf(
            &self,
            _session: &BrowserSession,
            _options: &PdfOptions,
            _limits: &CaptureLimits,
        ) -> QuarryResult<CapturedPdf> {
            self.call_order.lock().await.push("capture_pdf");
            Ok(CapturedPdf {
                bytes: b"%PDF-1.7".to_vec(),
            })
        }
        async fn egress_receipts(
            &self,
            _session: &BrowserSession,
            _after_sequence: u64,
            _limit: usize,
        ) -> QuarryResult<Vec<BrowserEgressReceipt>> {
            Ok((0..self.blocked)
                .map(|index| BrowserEgressReceipt {
                    sequence: index as u64,
                    tab_id: None,
                    method: "GET".into(),
                    url: "https://blocked.example/".into(),
                    decision: quarry_core::contracts::BrowserEgressDecision::Block,
                    policy: "deny_all".into(),
                    timestamp_ms: 0,
                })
                .collect())
        }
    }

    fn capture_hints(plan: CapturePlan) -> FetchHints {
        FetchHints {
            render: RenderHints {
                capture: Some(plan),
                ..RenderHints::default()
            },
            ..FetchHints::default()
        }
    }

    fn both() -> CapturePlan {
        CapturePlan {
            screenshot: Some(ScreenshotOptions::default()),
            pdf: Some(PdfOptions::default()),
        }
    }

    fn page_url() -> Url {
        Url::parse("https://example.com/").unwrap()
    }

    #[tokio::test]
    async fn capture_runs_after_content_and_before_release() {
        let mock = Arc::new(CaptureMock::new(true, false, 0));
        let adapter = BrowserDriverAdapter::new(mock.clone(), Arc::new(RuntimeLeasePool::new(4)));
        let resp = adapter
            .fetch_conditional(&page_url(), &capture_hints(both()))
            .await
            .expect("fetch ok");

        let order = mock.call_order.lock().await.clone();
        assert_eq!(
            order,
            vec![
                "acquire",
                "configure_egress_policy",
                "goto",
                "content",
                "capture_screenshot",
                "capture_pdf",
                "release"
            ]
        );
        let captures = resp.captures.expect("captures carried on the response");
        assert!(captures.screenshot.is_some());
        assert!(captures.pdf.is_some());
        assert!(captures.warnings.is_empty());
    }

    #[tokio::test]
    async fn capture_failure_is_a_warning_and_the_page_still_returns() {
        let mock = Arc::new(CaptureMock::new(true, true, 0));
        let adapter = BrowserDriverAdapter::new(mock.clone(), Arc::new(RuntimeLeasePool::new(4)));
        let resp = adapter
            .fetch_conditional(&page_url(), &capture_hints(both()))
            .await
            .expect("the page still returns when a capture fails");

        assert_eq!(resp.status, 200);
        assert!(!resp.body.is_empty());
        let captures = resp.captures.expect("captures carried on the response");
        assert!(captures.screenshot.is_none());
        assert!(captures.pdf.is_some(), "the other capture is unaffected");
        let warning = captures
            .warnings
            .iter()
            .find(|w| w.code == "screenshot_failed")
            .expect("screenshot_failed warning");
        // The message names the typed reason, never the raw error text.
        assert!(warning.message.contains("capture_too_large"));
        assert!(!warning.message.contains("raw page text"));
    }

    #[tokio::test]
    async fn driver_without_capture_capability_is_refused_before_acquire() {
        let mock = Arc::new(CaptureMock::new(false, false, 0));
        let adapter = BrowserDriverAdapter::new(mock.clone(), Arc::new(RuntimeLeasePool::new(4)));
        match adapter
            .fetch_conditional(&page_url(), &capture_hints(both()))
            .await
        {
            Err(error) => {
                assert_eq!(error.code, ErrorCode::Unsupported);
                assert_eq!(
                    error.details.as_ref().and_then(|d| d.get("reason")).and_then(|r| r.as_str()),
                    Some("browser_capability_unavailable")
                );
            }
            Ok(_) => panic!("a browser without the capability must refuse the capture"),
        }
        assert_eq!(mock.acquire_count.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn blocked_requests_are_counted_without_urls() {
        let mock = Arc::new(CaptureMock::new(true, false, 2));
        let adapter = BrowserDriverAdapter::new(mock.clone(), Arc::new(RuntimeLeasePool::new(4)));
        let resp = adapter
            .fetch_conditional(&page_url(), &capture_hints(CapturePlan {
                screenshot: Some(ScreenshotOptions::default()),
                pdf: None,
            }))
            .await
            .expect("fetch ok");
        let captures = resp.captures.expect("captures carried on the response");
        assert_eq!(captures.blocked_requests, 2);
        assert!(captures.final_url.is_none(), "the mock reports no current url");
    }
}

#[cfg(test)]
mod action_settle_tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A DOM that answers `script` in order and then repeats its last answer. `None` is a failed
    /// read. The counter returned with it is the number of reads taken.
    fn scripted_dom(
        script: &[Option<&'static str>],
    ) -> (impl FnMut() -> std::future::Ready<Option<bytes::Bytes>>, Arc<AtomicUsize>) {
        let reads = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&reads);
        let mut answers: VecDeque<Option<&'static str>> = script.iter().copied().collect();
        let sampler = move || {
            counter.fetch_add(1, Ordering::SeqCst);
            let answer = if answers.len() > 1 {
                answers.pop_front().flatten()
            } else {
                answers.front().copied().flatten()
            };
            std::future::ready(answer.map(|html| bytes::Bytes::from_static(html.as_bytes())))
        };
        (sampler, reads)
    }

    #[tokio::test]
    async fn a_quiet_dom_settles_on_the_second_matching_sample() {
        let (dom, reads) = scripted_dom(&[Some("<p>a</p>"), Some("<p>a</p>")]);
        let settled = settle_quiescence(Duration::from_millis(2_000), dom).await;
        assert!(settled < 2_000, "a quiet DOM ends the wait before the bound: {settled} ms");
        assert_eq!(reads.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn a_dom_that_changes_once_settles_at_the_first_matching_pair() {
        let (dom, reads) = scripted_dom(&[Some("<p>a</p>"), Some("<p>b</p>"), Some("<p>b</p>")]);
        let settled = settle_quiescence(Duration::from_millis(2_000), dom).await;
        assert!(settled < 2_000, "the wait ends at the quiet pair: {settled} ms");
        assert_eq!(reads.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn a_dom_that_keeps_changing_is_bounded_and_reports_the_bound() {
        let (dom, reads) = scripted_dom(&[
            Some("<p>1</p>"),
            Some("<p>2</p>"),
            Some("<p>3</p>"),
            Some("<p>4</p>"),
            Some("<p>5</p>"),
        ]);
        let settled = settle_quiescence(Duration::from_millis(300), dom).await;
        assert_eq!(settled, 300, "still changing at the bound: the bound is reported");
        assert!(reads.load(Ordering::SeqCst) <= 4, "the samples stop at the bound");
    }

    #[tokio::test]
    async fn a_failed_first_read_ends_the_wait_without_a_retry() {
        let (dom, reads) = scripted_dom(&[None]);
        let settled = settle_quiescence(Duration::from_millis(300), dom).await;
        assert!(settled < 300, "a failed first read ends the wait at once: {settled} ms");
        assert_eq!(reads.load(Ordering::SeqCst), 1, "a failed read is not retried");
    }

    #[tokio::test]
    async fn a_read_that_fails_mid_wait_stops_the_wait_without_a_retry() {
        let (dom, reads) = scripted_dom(&[Some("<p>a</p>"), None]);
        let settled = settle_quiescence(Duration::from_millis(2_000), dom).await;
        assert!(settled < 2_000, "the wait stops at the failed read: {settled} ms");
        assert_eq!(reads.load(Ordering::SeqCst), 2, "the failed read is not retried");
    }

    #[test]
    fn the_settle_is_off_unless_its_flag_is_on() {
        assert_eq!(action_settle_bound_from(None, None), None);
        assert_eq!(action_settle_bound_from(None, Some("750")), None);
        assert_eq!(action_settle_bound_from(Some("0"), Some("750")), None);
        assert_eq!(action_settle_bound_from(Some("off"), None), None);
        assert_eq!(action_settle_bound_from(Some("maybe"), Some("750")), None, "a typo stays off");
    }

    #[test]
    fn an_on_flag_takes_the_bound_with_its_default_and_cap() {
        assert_eq!(action_settle_bound_from(Some("1"), None), Some(Duration::from_millis(300)));
        assert_eq!(
            action_settle_bound_from(Some(" ON "), Some("nope")),
            Some(Duration::from_millis(300)),
            "a malformed bound takes the default"
        );
        assert_eq!(action_settle_bound_from(Some("true"), Some("750")), Some(Duration::from_millis(750)));
        assert_eq!(action_settle_bound_from(Some("yes"), Some("5000")), Some(Duration::from_millis(2_000)));
        assert_eq!(action_settle_bound_from(Some("on"), Some("0")), None, "a zero bound is off");
    }
}
