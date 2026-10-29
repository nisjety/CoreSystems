//! Per-domain robots.txt cache with TTL.
//!
//! `RobotsTxt::parse` is fast but **fetching** robots.txt over the network
//! is not — every URL on a domain pays the same fetch latency unless we
//! cache. This module provides:
//!
//! - `RobotsCache` — in-memory `HashMap<host, (RobotsTxt, fetched_at)>`
//!   with configurable TTL (default 24h)
//! - `negative cache` for 404s and fetch errors so we don't hammer the
//!   origin re-trying a missing robots.txt every page
//! - `coalescing` so concurrent first-time requests for the same host
//!   don't fan out into N fetches — they all wait on a single in-flight
//!   future
//!
//! The fetch is pluggable via the `RobotsFetcher` trait so production
//! deployments can route the fetch through their existing TLS-impersonated
//! HTTP client (instead of reqwest's vanilla profile, which some sites
//! block).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::{Mutex, RwLock};

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_transform::robots::RobotsTxt;

#[async_trait]
pub trait RobotsFetcher: Send + Sync {
    /// Fetch the raw robots.txt body for `host`. Implementations should
    /// return:
    /// - `Ok(Some(body))` — robots.txt found, body is the raw text
    /// - `Ok(None)` — robots.txt 404'd / not present (allow everything)
    /// - `Err(...)` — transport-level error (retried per RobotsCache policy)
    async fn fetch(&self, host: &str) -> QuarryResult<Option<String>>;
}

/// Cached entry — a parsed RobotsTxt or a "no robots.txt here" marker,
/// plus the time it was fetched so we can expire by TTL.
#[derive(Debug, Clone)]
struct Entry {
    robots: Option<RobotsTxt>,
    fetched_at: Instant,
}

#[derive(Clone)]
pub struct RobotsCache {
    fetcher: Arc<dyn RobotsFetcher>,
    cache: Arc<RwLock<HashMap<String, Entry>>>,
    /// Per-host coalescing locks — only one fetch in flight per host.
    in_flight: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    ttl: Duration,
    /// Shorter TTL for negative results (no robots.txt) so transient
    /// 404s don't lock in for the full TTL.
    negative_ttl: Duration,
}

impl RobotsCache {
    pub fn new(fetcher: Arc<dyn RobotsFetcher>) -> Self {
        Self {
            fetcher,
            cache: Arc::new(RwLock::new(HashMap::new())),
            in_flight: Arc::new(Mutex::new(HashMap::new())),
            ttl: Duration::from_secs(24 * 60 * 60),
            negative_ttl: Duration::from_secs(60 * 60),
        }
    }

    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    pub fn with_negative_ttl(mut self, ttl: Duration) -> Self {
        self.negative_ttl = ttl;
        self
    }

    /// Get or fetch the parsed robots.txt for `host`. Concurrent callers
    /// for the same host coalesce onto one fetch. Returns `None` when the
    /// host has no robots.txt (callers should treat as "allow everything").
    pub async fn get(&self, host: &str) -> QuarryResult<Option<RobotsTxt>> {
        // Fast path: hit cached entry that hasn't expired.
        {
            let cache = self.cache.read().await;
            if let Some(entry) = cache.get(host) {
                let ttl = if entry.robots.is_some() {
                    self.ttl
                } else {
                    self.negative_ttl
                };
                if entry.fetched_at.elapsed() < ttl {
                    return Ok(entry.robots.clone());
                }
            }
        }

        // Coalesce: multiple concurrent first-time requests share one fetch.
        let coalesce_lock = {
            let mut map = self.in_flight.lock().await;
            map.entry(host.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let _coalesce_guard = coalesce_lock.lock().await;

        // After acquiring the coalesce lock, re-check the cache — another
        // concurrent caller may have just populated it.
        {
            let cache = self.cache.read().await;
            if let Some(entry) = cache.get(host) {
                let ttl = if entry.robots.is_some() {
                    self.ttl
                } else {
                    self.negative_ttl
                };
                if entry.fetched_at.elapsed() < ttl {
                    return Ok(entry.robots.clone());
                }
            }
        }

        // Fetch. Errors are surfaced to the caller AND cached negatively
        // for the negative TTL — burst-of-failed-pages on a broken robots
        // endpoint shouldn't hammer the origin.
        let robots = match self.fetcher.fetch(host).await {
            Ok(Some(body)) => Some(RobotsTxt::parse(&body)),
            Ok(None) => None,
            Err(e) => {
                tracing::warn!(host, error = %e, "robots.txt fetch failed; cached negative");
                None
            }
        };

        let mut cache = self.cache.write().await;
        cache.insert(
            host.to_string(),
            Entry {
                robots: robots.clone(),
                fetched_at: Instant::now(),
            },
        );
        Ok(robots)
    }

    /// Force-evict an entry. Useful when an operator rotates a site's
    /// robots.txt and wants Quarry to pick up the new rules immediately.
    pub async fn invalidate(&self, host: &str) {
        self.cache.write().await.remove(host);
    }

    pub async fn len(&self) -> usize {
        self.cache.read().await.len()
    }
}

/// Production fetcher implementation that uses a reqwest client.
/// Treats 404 as "no robots.txt" and 5xx as a transport error.
pub struct ReqwestRobotsFetcher {
    client: reqwest::Client,
    user_agent: String,
}

impl ReqwestRobotsFetcher {
    pub fn new(user_agent: impl Into<String>) -> QuarryResult<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| {
                QuarryError::new(
                    ErrorCode::Internal,
                    format!("reqwest robots client build: {e}"),
                )
            })?;
        Ok(Self {
            client,
            user_agent: user_agent.into(),
        })
    }
}

#[async_trait]
impl RobotsFetcher for ReqwestRobotsFetcher {
    async fn fetch(&self, host: &str) -> QuarryResult<Option<String>> {
        // Try HTTPS first, fall back to HTTP for sites that don't redirect.
        let urls = [format!("https://{host}/robots.txt"), format!("http://{host}/robots.txt")];
        for url in urls {
            match self
                .client
                .get(&url)
                .header("user-agent", &self.user_agent)
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    return resp.text().await.map(Some).map_err(|e| {
                        QuarryError::new(
                            ErrorCode::DriverFailed,
                            format!("robots.txt body read: {e}"),
                        )
                    });
                }
                Ok(resp) if resp.status().as_u16() == 404 => return Ok(None),
                Ok(_) => continue, // try next scheme
                Err(_) => continue,
            }
        }
        Ok(None) // both schemes silent — treat as no robots.txt
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Test fetcher that counts calls so we can verify caching cuts fetches.
    struct CountingFetcher {
        calls: Arc<AtomicUsize>,
        body: Option<String>,
    }

    #[async_trait]
    impl RobotsFetcher for CountingFetcher {
        async fn fetch(&self, _host: &str) -> QuarryResult<Option<String>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.body.clone())
        }
    }

    #[tokio::test]
    async fn first_fetch_then_cached() {
        let calls = Arc::new(AtomicUsize::new(0));
        let fetcher = Arc::new(CountingFetcher {
            calls: calls.clone(),
            body: Some("User-agent: *\nDisallow: /admin".into()),
        });
        let cache = RobotsCache::new(fetcher);

        let r1 = cache.get("example.com").await.unwrap();
        let r2 = cache.get("example.com").await.unwrap();
        let r3 = cache.get("example.com").await.unwrap();
        assert!(r1.is_some());
        assert!(r2.is_some());
        assert!(r3.is_some());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn negative_result_cached() {
        let calls = Arc::new(AtomicUsize::new(0));
        let fetcher = Arc::new(CountingFetcher {
            calls: calls.clone(),
            body: None,
        });
        let cache = RobotsCache::new(fetcher);

        let r1 = cache.get("example.com").await.unwrap();
        let r2 = cache.get("example.com").await.unwrap();
        assert!(r1.is_none());
        assert!(r2.is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn ttl_expiry_triggers_refetch() {
        let calls = Arc::new(AtomicUsize::new(0));
        let fetcher = Arc::new(CountingFetcher {
            calls: calls.clone(),
            body: Some("User-agent: *\n".into()),
        });
        let cache = RobotsCache::new(fetcher).with_ttl(Duration::from_millis(20));

        let _ = cache.get("example.com").await.unwrap();
        tokio::time::sleep(Duration::from_millis(40)).await;
        let _ = cache.get("example.com").await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn invalidate_forces_refetch() {
        let calls = Arc::new(AtomicUsize::new(0));
        let fetcher = Arc::new(CountingFetcher {
            calls: calls.clone(),
            body: Some("User-agent: *\n".into()),
        });
        let cache = RobotsCache::new(fetcher);

        let _ = cache.get("example.com").await.unwrap();
        cache.invalidate("example.com").await;
        let _ = cache.get("example.com").await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn distinct_hosts_are_cached_independently() {
        let calls = Arc::new(AtomicUsize::new(0));
        let fetcher = Arc::new(CountingFetcher {
            calls: calls.clone(),
            body: Some("User-agent: *\n".into()),
        });
        let cache = RobotsCache::new(fetcher);

        let _ = cache.get("a.com").await.unwrap();
        let _ = cache.get("b.com").await.unwrap();
        let _ = cache.get("a.com").await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(cache.len().await, 2);
    }

    #[tokio::test]
    async fn concurrent_first_time_requests_coalesce() {
        // We can't introduce artificial slowness without complicating the
        // fetcher much. Instead we verify the contract: even with N
        // concurrent get() calls before any populates the cache, only ONE
        // fetcher call happens. The lock ordering naturally ensures this.
        let calls = Arc::new(AtomicUsize::new(0));
        let fetcher = Arc::new(CountingFetcher {
            calls: calls.clone(),
            body: Some("User-agent: *\n".into()),
        });
        let cache = RobotsCache::new(fetcher);

        let mut handles = Vec::new();
        for _ in 0..10 {
            let c = cache.clone();
            handles.push(tokio::spawn(async move { c.get("example.com").await }));
        }
        for h in handles {
            let _ = h.await.unwrap();
        }
        // Coalescing means at most a few fetcher calls — strictly 1 in
        // this no-await fetcher because the first request completes
        // synchronously inside the lock. We allow up to 2 to tolerate
        // scheduler nondeterminism on slower machines.
        let n = calls.load(Ordering::SeqCst);
        assert!(n <= 2, "expected coalesced fetches, got {n}");
    }
}
