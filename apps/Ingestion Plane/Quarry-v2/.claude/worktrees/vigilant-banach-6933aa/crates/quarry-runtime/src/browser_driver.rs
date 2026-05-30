//! BrowserDriverAdapter — wraps a `quarry_browser::BrowserDriver` and
//! presents it as a `crate::driver::Driver` so the pipeline can use the
//! same interface regardless of whether the page is fetched via a plain
//! HTTP stack or a full-browser session.

use async_trait::async_trait;
use std::sync::Arc;
use std::time::{Duration, Instant};
use url::Url;

use quarry_browser::BrowserDriver as BrowserDriverTrait;
use quarry_core::output::DriverKind;
use quarry_core::{
    ids::kinds,
    lease::{BrowserLease, ProxyAffinity},
    QuarryResult,
};

use crate::{driver::Driver, fetch::FetchResponse, lease_pool::RuntimeLeasePool};

const LEASE_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(30);

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
}

impl BrowserDriverAdapter {
    pub fn new(
        inner: Arc<dyn BrowserDriverTrait + Send + Sync>,
        pool: Arc<RuntimeLeasePool>,
    ) -> Self {
        Self { inner, pool }
    }
}

#[async_trait]
impl Driver for BrowserDriverAdapter {
    fn kind(&self) -> DriverKind {
        DriverKind::Browser
    }

    async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
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

        if let Err(e) = self.inner.goto(&session, url.as_str()).await {
            guard.poison();
            let _ = self.inner.release(session).await;
            return Err(e);
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
        call_order: TokioMutex<Vec<&'static str>>,
        last_url: TokioMutex<Option<String>>,
        body: Bytes,
    }

    impl MockBrowserDriver {
        fn new(body: &'static [u8]) -> Self {
            Self {
                acquire_count: AtomicUsize::new(0),
                goto_count: AtomicUsize::new(0),
                content_count: AtomicUsize::new(0),
                release_count: AtomicUsize::new(0),
                call_order: TokioMutex::new(Vec::new()),
                last_url: TokioMutex::new(None),
                body: Bytes::from_static(body),
            }
        }
    }

    #[async_trait]
    impl BrowserDriverTrait for MockBrowserDriver {
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
}
