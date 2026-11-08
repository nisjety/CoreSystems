//! Per-origin robots.txt cache (WS14.S4).
//!
//! `RobotsTxt::parse` is fast but fetching robots.txt is not, so every origin
//! pays one fetch per TTL, not one per page. The cache keeps what the fetch
//! learned about an origin as a [`RobotsState`]:
//!
//! - `Rules`: the parsed file (TTL 24 h);
//! - `Absent`: the origin has no robots.txt, so everything is allowed (TTL 1 h);
//! - `Unreachable`: the fetch failed, so the policy decides (TTL 5 min).
//!
//! The key is the origin, `scheme://host:port`: `http` and `https`, and two ports
//! of one host, have separate rules (RFC 9309 2.3). The map is bounded and drops
//! the oldest entry when it is full. Concurrent first callers for one origin
//! share one fetch, and the per-origin lock is dropped again when the fetch is
//! over, so the in-flight map does not grow with the number of origins seen.
//!
//! The fetch is pluggable ([`RobotsFetcher`]). The production fetcher is
//! `robots_gate::DriverRobotsFetcher`, which goes through the guarded driver
//! chain; this module owns no HTTP client (the former `ReqwestRobotsFetcher`, a
//! second egress path around the SSRF guard, is gone).
//!
//! Time is `tokio::time::Instant`, so the TTL tests run under `tokio::time::pause`.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio::time::Instant;
use url::Url;

use quarry_transform::robots::RobotsTxt;

/// How long parsed rules are kept.
pub const RULES_TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// How long "no robots.txt here" is kept.
pub const ABSENT_TTL: Duration = Duration::from_secs(60 * 60);
/// How long "robots.txt could not be fetched" is kept.
pub const UNREACHABLE_TTL: Duration = Duration::from_secs(5 * 60);
/// Origins kept before the oldest is dropped.
pub const DEFAULT_MAX_ENTRIES: usize = 10_000;

/// What one fetch of an origin's robots.txt came back with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RobotsFetch {
    /// A 2xx answer: the body (possibly cut at the fetch cap).
    Found(String),
    /// The origin says it has no robots.txt (a 4xx answer): allow everything.
    Absent,
    /// No usable answer (5xx, a timeout, a refused target, a transport error).
    /// The text is a short class label, never a URL or a message from the origin.
    Unreachable(String),
}

/// What the cache holds for an origin.
#[derive(Debug, Clone)]
pub enum RobotsState {
    Rules(Arc<RobotsTxt>),
    Absent,
    Unreachable,
}

impl RobotsState {
    /// The state a fetch result stands for. A found body is parsed here, once.
    pub fn from_fetch(fetch: RobotsFetch) -> Self {
        match fetch {
            RobotsFetch::Found(body) => Self::Rules(Arc::new(RobotsTxt::parse(&body))),
            RobotsFetch::Absent => Self::Absent,
            RobotsFetch::Unreachable(_) => Self::Unreachable,
        }
    }
}

/// The cache key for a URL: `scheme://host:port` in lower case, the default port
/// written out. `None` for a URL that is not http(s) or has no host.
pub fn origin_key(url: &Url) -> Option<String> {
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?;
    let port = url.port_or_known_default()?;
    Some(format!("{}://{}:{}", url.scheme(), host.to_ascii_lowercase(), port))
}

#[async_trait]
pub trait RobotsFetcher: Send + Sync {
    /// Fetch the robots.txt of `origin` (a key from [`origin_key`]). `org_id` is
    /// the tenant the fetch is made for; it picks the egress identity and is
    /// never part of the cache key. The answer is a [`RobotsFetch`]; an
    /// implementation does not return an error.
    async fn fetch(&self, origin: &str, org_id: &str) -> RobotsFetch;
}

#[derive(Debug, Clone)]
struct Entry {
    state: RobotsState,
    fetched_at: Instant,
}

#[derive(Clone)]
pub struct RobotsCache {
    fetcher: Arc<dyn RobotsFetcher>,
    entries: Arc<RwLock<HashMap<String, Entry>>>,
    /// Per-origin locks: one fetch in flight per origin. An entry is removed when
    /// the fetch that held it is over and nobody else is waiting on it.
    in_flight: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    rules_ttl: Duration,
    absent_ttl: Duration,
    unreachable_ttl: Duration,
    max_entries: usize,
}

impl RobotsCache {
    pub fn new(fetcher: Arc<dyn RobotsFetcher>) -> Self {
        Self {
            fetcher,
            entries: Arc::new(RwLock::new(HashMap::new())),
            in_flight: Arc::new(Mutex::new(HashMap::new())),
            rules_ttl: RULES_TTL,
            absent_ttl: ABSENT_TTL,
            unreachable_ttl: UNREACHABLE_TTL,
            max_entries: DEFAULT_MAX_ENTRIES,
        }
    }

    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.rules_ttl = ttl;
        self
    }

    /// The TTL of "no robots.txt here".
    pub fn with_negative_ttl(mut self, ttl: Duration) -> Self {
        self.absent_ttl = ttl;
        self
    }

    pub fn with_unreachable_ttl(mut self, ttl: Duration) -> Self {
        self.unreachable_ttl = ttl;
        self
    }

    /// The most origins kept. At least 1.
    pub fn with_max_entries(mut self, max: usize) -> Self {
        self.max_entries = max.max(1);
        self
    }

    fn ttl_for(&self, state: &RobotsState) -> Duration {
        match state {
            RobotsState::Rules(_) => self.rules_ttl,
            RobotsState::Absent => self.absent_ttl,
            RobotsState::Unreachable => self.unreachable_ttl,
        }
    }

    fn fresh(&self, origin: &str) -> Option<RobotsState> {
        let entries = self.entries.read().unwrap_or_else(|e| e.into_inner());
        let entry = entries.get(origin)?;
        (entry.fetched_at.elapsed() < self.ttl_for(&entry.state)).then(|| entry.state.clone())
    }

    fn insert(&self, origin: &str, state: RobotsState) {
        let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
        if !entries.contains_key(origin) && entries.len() >= self.max_entries {
            if let Some(oldest) = entries
                .iter()
                .min_by_key(|(_, e)| e.fetched_at)
                .map(|(k, _)| k.clone())
            {
                entries.remove(&oldest);
            }
        }
        entries.insert(
            origin.to_string(),
            Entry {
                state,
                fetched_at: Instant::now(),
            },
        );
    }

    /// What the cache already knows about `origin`, without any network call.
    /// `None` when the origin is not cached or its entry has expired.
    pub fn peek(&self, origin: &str) -> Option<RobotsState> {
        self.fresh(origin)
    }

    /// The state of `origin`: from the cache while it is fresh, otherwise fetched
    /// and stored. Concurrent callers for one origin share one fetch.
    pub async fn get(&self, origin: &str, org_id: &str) -> RobotsState {
        if let Some(state) = self.fresh(origin) {
            return state;
        }
        let lock = {
            let mut map = self.in_flight.lock().await;
            map.entry(origin.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let guard = lock.lock().await;
        // Another caller may have filled the entry while this one waited.
        let state = match self.fresh(origin) {
            Some(state) => state,
            None => {
                let state = RobotsState::from_fetch(self.fetcher.fetch(origin, org_id).await);
                self.insert(origin, state.clone());
                state
            }
        };
        drop(guard);
        // Forget the lock when only the map and this caller still hold it. The
        // map is locked for the check, so nobody can pick the lock up meanwhile.
        let mut map = self.in_flight.lock().await;
        if Arc::strong_count(&lock) <= 2 {
            map.remove(origin);
        }
        state
    }

    /// Fetch `origin` and return what came back, storing nothing: the entry map
    /// is neither read nor written. For a caller that must leave no trace (a
    /// request under ZDR).
    pub async fn get_uncached(&self, origin: &str, org_id: &str) -> RobotsState {
        RobotsState::from_fetch(self.fetcher.fetch(origin, org_id).await)
    }

    /// Force-evict an entry. Useful when an operator rotates a site's
    /// robots.txt and wants Quarry to pick up the new rules immediately.
    pub fn invalidate(&self, origin: &str) {
        self.entries
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(origin);
    }

    /// Origins held, expired entries included until they are replaced.
    pub fn len(&self) -> usize {
        self.entries.read().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Origins with a fetch in flight or a lock not yet dropped.
    pub async fn in_flight_len(&self) -> usize {
        self.in_flight.lock().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A fetcher that counts its calls and answers with a fixed result,
    /// after an optional (virtual) delay.
    struct CountingFetcher {
        calls: Arc<AtomicUsize>,
        answer: RobotsFetch,
        delay: Duration,
    }

    impl CountingFetcher {
        fn new(answer: RobotsFetch) -> (Arc<Self>, Arc<AtomicUsize>) {
            let calls = Arc::new(AtomicUsize::new(0));
            (
                Arc::new(Self {
                    calls: calls.clone(),
                    answer,
                    delay: Duration::ZERO,
                }),
                calls,
            )
        }
    }

    #[async_trait]
    impl RobotsFetcher for CountingFetcher {
        async fn fetch(&self, _origin: &str, _org_id: &str) -> RobotsFetch {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            self.answer.clone()
        }
    }

    fn found() -> RobotsFetch {
        RobotsFetch::Found("User-agent: *\nDisallow: /admin".into())
    }

    #[tokio::test]
    async fn found_is_fetched_once_then_cached() {
        let (fetcher, calls) = CountingFetcher::new(found());
        let cache = RobotsCache::new(fetcher);
        for _ in 0..3 {
            assert!(matches!(
                cache.get("https://example.com:443", "org").await,
                RobotsState::Rules(_)
            ));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn absent_is_cached() {
        let (fetcher, calls) = CountingFetcher::new(RobotsFetch::Absent);
        let cache = RobotsCache::new(fetcher);
        for _ in 0..3 {
            assert!(matches!(
                cache.get("https://example.com:443", "org").await,
                RobotsState::Absent
            ));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn unreachable_is_cached_and_never_reads_as_rules() {
        let (fetcher, calls) = CountingFetcher::new(RobotsFetch::Unreachable("status_503".into()));
        let cache = RobotsCache::new(fetcher);
        for _ in 0..3 {
            assert!(matches!(
                cache.get("https://example.com:443", "org").await,
                RobotsState::Unreachable
            ));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn ttls_are_24h_for_rules_1h_for_absent_5min_for_unreachable() {
        for (answer, ttl) in [
            (found(), RULES_TTL),
            (RobotsFetch::Absent, ABSENT_TTL),
            (RobotsFetch::Unreachable("timeout".into()), UNREACHABLE_TTL),
        ] {
            let (fetcher, calls) = CountingFetcher::new(answer);
            let cache = RobotsCache::new(fetcher);
            let origin = "https://example.com:443";
            cache.get(origin, "org").await;
            tokio::time::advance(ttl - Duration::from_secs(1)).await;
            cache.get(origin, "org").await;
            assert_eq!(calls.load(Ordering::SeqCst), 1, "still fresh a second before {ttl:?}");
            assert!(cache.peek(origin).is_some());
            tokio::time::advance(Duration::from_secs(2)).await;
            assert!(cache.peek(origin).is_none(), "peek never returns an expired entry");
            cache.get(origin, "org").await;
            assert_eq!(calls.load(Ordering::SeqCst), 2, "refetched after {ttl:?}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn fifty_concurrent_callers_share_one_fetch() {
        let calls = Arc::new(AtomicUsize::new(0));
        let fetcher = Arc::new(CountingFetcher {
            calls: calls.clone(),
            answer: found(),
            delay: Duration::from_millis(200),
        });
        let cache = RobotsCache::new(fetcher);
        let mut handles = Vec::new();
        for _ in 0..50 {
            let cache = cache.clone();
            handles.push(tokio::spawn(async move {
                cache.get("https://example.com:443", "org").await
            }));
        }
        for handle in handles {
            assert!(matches!(handle.await.unwrap(), RobotsState::Rules(_)));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(cache.in_flight_len().await, 0, "the lock map is pruned after the fetch");
    }

    #[tokio::test]
    async fn in_flight_is_pruned_after_every_fetch() {
        let (fetcher, _calls) = CountingFetcher::new(found());
        let cache = RobotsCache::new(fetcher);
        for i in 0..20 {
            cache.get(&format!("https://h{i}.test:443"), "org").await;
        }
        assert_eq!(cache.in_flight_len().await, 0);
        assert_eq!(cache.len(), 20);
    }

    #[tokio::test(start_paused = true)]
    async fn the_oldest_entry_goes_first_at_the_cap() {
        let (fetcher, _calls) = CountingFetcher::new(found());
        let cache = RobotsCache::new(fetcher).with_max_entries(3);
        for name in ["a", "b", "c"] {
            cache.get(&format!("https://{name}.test:443"), "org").await;
            tokio::time::advance(Duration::from_secs(1)).await;
        }
        assert_eq!(cache.len(), 3);
        cache.get("https://d.test:443", "org").await;
        assert_eq!(cache.len(), 3, "the map never grows past the cap");
        assert!(cache.peek("https://a.test:443").is_none(), "the oldest origin was dropped");
        assert!(cache.peek("https://b.test:443").is_some());
        assert!(cache.peek("https://d.test:443").is_some());
    }

    #[tokio::test]
    async fn origin_keys_differ_by_scheme_and_port() {
        let a = Url::parse("https://example.com/x").unwrap();
        let b = Url::parse("http://example.com/x").unwrap();
        let c = Url::parse("https://example.com:8443/x").unwrap();
        let d = Url::parse("https://EXAMPLE.com:443/y?q=1").unwrap();
        assert_eq!(origin_key(&a).unwrap(), "https://example.com:443");
        assert_eq!(origin_key(&b).unwrap(), "http://example.com:80");
        assert_eq!(origin_key(&c).unwrap(), "https://example.com:8443");
        assert_eq!(origin_key(&a), origin_key(&d));
        assert_ne!(origin_key(&a), origin_key(&b));
        assert_ne!(origin_key(&a), origin_key(&c));
        assert!(origin_key(&Url::parse("ftp://example.com/").unwrap()).is_none());

        let (fetcher, calls) = CountingFetcher::new(found());
        let cache = RobotsCache::new(fetcher);
        cache.get(&origin_key(&a).unwrap(), "org").await;
        cache.get(&origin_key(&b).unwrap(), "org").await;
        cache.get(&origin_key(&c).unwrap(), "org").await;
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn get_uncached_inserts_and_reads_nothing() {
        let (fetcher, calls) = CountingFetcher::new(found());
        let cache = RobotsCache::new(fetcher);
        cache.get_uncached("https://example.com:443", "org").await;
        cache.get_uncached("https://example.com:443", "org").await;
        assert_eq!(calls.load(Ordering::SeqCst), 2, "no read from a cache either");
        assert!(cache.is_empty());
        assert!(cache.peek("https://example.com:443").is_none());
    }

    #[tokio::test]
    async fn invalidate_forces_a_refetch() {
        let (fetcher, calls) = CountingFetcher::new(found());
        let cache = RobotsCache::new(fetcher);
        cache.get("https://example.com:443", "org").await;
        cache.invalidate("https://example.com:443");
        cache.get("https://example.com:443", "org").await;
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn distinct_origins_are_cached_independently() {
        let (fetcher, calls) = CountingFetcher::new(found());
        let cache = RobotsCache::new(fetcher);
        cache.get("https://a.com:443", "org").await;
        cache.get("https://b.com:443", "org").await;
        cache.get("https://a.com:443", "org").await;
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(cache.len(), 2);
    }
}
