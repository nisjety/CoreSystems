//! BrowserDriverAdapter — wraps a `quarry_browser::BrowserDriver` and
//! presents it as a `crate::driver::Driver` so the pipeline can use the
//! same interface regardless of whether the page is fetched via a plain
//! HTTP stack or a full-browser session.

use async_trait::async_trait;
use std::sync::Arc;
use std::time::{Duration, Instant};
use url::Url;

use quarry_browser::{BrowserDriver as BrowserDriverTrait, BrowserEgressPolicy};
use quarry_core::output::DriverKind;
use quarry_core::{
    ids::kinds,
    lease::{BrowserLease, ProxyAffinity},
    QuarryResult,
};

use crate::{
    driver::{Driver, FetchHints},
    fetch::FetchResponse,
    lease_pool::RuntimeLeasePool,
};

const LEASE_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(30);

/// Default `wait_for_selector` timeout when the caller doesn't specify
/// one. 5 seconds is a reasonable upper bound for SPA hydration on a
/// well-provisioned page — anything longer should be an explicit caller
/// choice, not a default.
const DEFAULT_WAIT_FOR_TIMEOUT_MS: u32 = 5_000;

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
}

impl BrowserDriverAdapter {
    pub fn new(
        inner: Arc<dyn BrowserDriverTrait + Send + Sync>,
        pool: Arc<RuntimeLeasePool>,
    ) -> Self {
        Self {
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
            inner,
            pool,
            managed_processor_id: Some(processor_id.into()),
        }
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
            .acquire_timeout(&key, LEASE_ACQUIRE_TIMEOUT)
            .await?;

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
                // Internal lease minted on the fetch path — the org_id
                // travels with the request via the PageRunner that
                // invoked us. This adapter doesn't yet have access to
                // it, so we record empty and rely on the surrounding
                // PageRunner code (which knows the verified org_id from
                // the edge handler) to attach it before any persistence.
                org_id: String::new(),
            };
            self.pool
                .seed(lease.session_affinity_key.clone(), lease.clone());
            guard.lease = Some(lease);
        }

        let lease_ref = guard.lease.as_ref().expect("lease set above");
        let start = Instant::now();

        let session = match self.inner.acquire(lease_ref).await {
            Ok(s) => s,
            Err(e) => {
                guard.poison();
                return Err(e);
            }
        };

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
                &session,
                BrowserEgressPolicy::from_allowed_domains(&allowed_domains),
            )
            .await
        {
            guard.poison();
            let _ = self.inner.release(session).await;
            return Err(e);
        }

        if let Err(e) = self.inner.goto(&session, url.as_str()).await {
            guard.poison();
            let _ = self.inner.release(session).await;
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
            if let Err(e) = self.inner.wait_for(&session, selector, timeout).await {
                guard.poison();
                let _ = self.inner.release(session).await;
                return Err(e);
            }
        }

        let body_bytes = match self.inner.content(&session).await {
            Ok(b) => b,
            Err(e) => {
                guard.poison();
                let _ = self.inner.release(session).await;
                return Err(e);
            }
        };

        let response = FetchResponse {
            status: 200,
            final_url: url.clone(),
            headers: vec![],
            body: body_bytes.to_vec(),
            duration_ms: start.elapsed().as_millis() as u64,
            served_by: DriverKind::Browser,
        };

        if let Err(e) = self.inner.release(session).await {
            guard.poison();
            return Err(e);
        }

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
            Ok(BrowserSession {
                lease: lease.clone(),
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
}
