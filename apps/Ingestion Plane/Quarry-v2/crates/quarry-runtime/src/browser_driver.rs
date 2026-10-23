//! BrowserDriverAdapter — wraps a `quarry_browser::BrowserDriver` and
//! presents it as a `crate::driver::Driver` so the pipeline can use the
//! same interface regardless of whether the page is fetched via a plain
//! HTTP stack or a full-browser session.

use async_trait::async_trait;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use url::Url;

use quarry_browser::{BrowserDriver as BrowserDriverTrait, BrowserEgressPolicy, BrowserSession};
use quarry_core::output::DriverKind;
use quarry_core::{
    ids::kinds,
    lease::{BrowserLease, ProxyAffinity},
    ErrorCode, QuarryError, QuarryResult,
};

use crate::{
    driver::{Driver, FetchHints},
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
}

#[async_trait]
impl Driver for BrowserDriverAdapter {
    fn kind(&self) -> DriverKind {
        DriverKind::Browser
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

        let response = FetchResponse {
            status: 200,
            final_url: url.clone(),
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
            captures: None,
        };

        held.finish(false).await?;

        Ok(response)
    }
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
