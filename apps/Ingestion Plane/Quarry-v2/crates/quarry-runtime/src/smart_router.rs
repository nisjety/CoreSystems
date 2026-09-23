//! Smart search router — picks the right engine for the job.
//!
//! Wraps the four-tier search stack (Tantivy → Stract → SearXNG → Brave)
//! with:
//!
//! 1. **Query intent classification** — detects URL/domain lookups
//!    (navigational), freshness keywords ("today", "latest", "2026") and
//!    quoted exact-match phrases. Each intent routes differently:
//!
//!    | Intent | Strategy |
//!    |---|---|
//!    | `Fresh` | Skip Tantivy (corpus is stale); parallel-fan-out to live SERPs and merge |
//!    | `Navigational` | Tantivy URL field first (microseconds); fall through to Stract |
//!    | `Phrase` | Tantivy first (BM25 owns exact-phrase quality); widen only on miss |
//!    | `Default` | Tantivy first; if results < threshold OR top score < quality_floor, fan out in parallel |
//!
//! 2. **Parallel widening** — instead of sequential fallback (Tantivy waits
//!    for full timeout before trying Stract), the default path queries
//!    Tantivy synchronously then concurrently kicks Stract + SearXNG when
//!    the local corpus didn't have enough. Merge + dedupe by canonical URL.
//!
//!    The merge *fuses* rather than concatenates: a URL two providers both
//!    returned has its reciprocal-rank contributions summed, so independent
//!    agreement between engines becomes ranking evidence instead of being
//!    thrown away with the duplicate. Tiering still decides **which
//!    providers run**; fusion only decides **the order within whatever pool
//!    they produced**.
//!
//! 3. **Circuit breaker per provider** — track failure count + last_failure
//!    per provider. After N failures in window_s seconds, skip that provider
//!    until cooldown_s elapses. Prevents one flapping upstream from dragging
//!    every request's p99.
//!
//! 4. **TTL cache** — blake3 hash of `(query, country, language, limit)` →
//!    cached results. In-memory; Redis can layer on top via the existing
//!    `quarry-edge/src/cache.rs` if cross-instance sharing is needed.
//!
//! 5. **Paid providers are gated on TWO per-request terms**, both of which
//!    must hold before Brave or Serper is dispatched anywhere in this module
//!    (`SearchOptions::paid_allowed()`):
//!
//!    * **not ZDR** — see point 6; and
//!    * **`allow_paid_providers`** — the Model Plane's per-request statement
//!      that this tenant's plan (Balance / Genius) includes paid fan-out.
//!      It defaults to `false`, so a caller that says nothing gets the free
//!      chain only.
//!
//!    The terms are independent conjuncts, never alternatives: permission
//!    cannot buy its way past ZDR, and not being ZDR does not by itself buy
//!    paid providers. Beyond that gate, Brave/Serper remain *backup* — even a
//!    permitted request only reaches them when the free chain (Tantivy +
//!    Stract + SearXNG) returns < min_results, or on the narrow eager path
//!    for terse head queries against an empty local corpus.
//!
//! 6. **ZDR overrides everything above.** When `SearchOptions.zdr` is set,
//!    Brave and Serper are never invoked — not as an eager paid backup, not
//!    as a last-resort fallback — regardless of `zero_saas_search`, which is
//!    a separate, operator-wide, opt-in posture toggle, and regardless of
//!    `allow_paid_providers`. `zdr` is a per-request signal that cannot be
//!    relaxed by config or by caller permission: the free chain (Tantivy /
//!    Stract / SearXNG / Data Plane) is all a zero-retention query ever gets.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::RwLock;
use tokio::task::JoinSet;
use url::Url;

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};

use crate::autoprompt::QueryRewriter;
use crate::fusion::RRF_K;
use crate::intent_classifier::IntentClassifier;
use crate::serp::{SearchOptions, SearchProvider, SearchResult};

/// Classified intent that drives routing.
///
/// The first four variants are detectable by pure rules (URL shape, quoted
/// phrase, freshness keywords). The remaining four are surfaced only when
/// an optional LLM classifier is wired into the router and the rule
/// fast-path returns [`QueryIntent::Default`]. Keeping them on the same
/// enum lets the router select a single path per query without a second
/// dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryIntent {
    /// Looks like a URL or bare domain — wants exact-target retrieval.
    Navigational,
    /// Contains time-sensitive language. Corpus likely stale.
    Fresh,
    /// Quoted exact-phrase query. BM25 territory.
    Phrase,
    /// Deep research question — fan out aggressively across every
    /// configured engine. Surfaced by the LLM classifier.
    Research,
    /// Comparison between two or more entities — same fan-out as Research
    /// but a higher result-limit hint.
    Comparative,
    /// Has a geographic component — biases SearXNG/Brave country params.
    Local,
    /// About source code, libraries, or APIs — Tantivy first (likely indexed
    /// docs), then Stract (good code repo coverage).
    Code,
    /// Everything else.
    Default,
}

/// Stable one-byte discriminant for the cache key.
///
/// Written out by hand rather than derived from the enum's memory layout or
/// its `Debug` string: cache keys must not shift because a variant was
/// inserted in the middle of the enum or a doc-rename touched a name. Every
/// value here is permanent — add new variants at the end.
fn intent_cache_tag(intent: Option<QueryIntent>) -> u8 {
    match intent {
        None => 0,
        Some(QueryIntent::Navigational) => 1,
        Some(QueryIntent::Fresh) => 2,
        Some(QueryIntent::Phrase) => 3,
        Some(QueryIntent::Research) => 4,
        Some(QueryIntent::Comparative) => 5,
        Some(QueryIntent::Local) => 6,
        Some(QueryIntent::Code) => 7,
        Some(QueryIntent::Default) => 8,
    }
}

/// Tunable knobs on the routing decisions.
#[derive(Debug, Clone)]
pub struct RouterConfig {
    /// Minimum local-corpus results before we declare success without
    /// widening. Default 3.
    pub min_local_results: usize,
    /// Minimum live-search results expected after widening. Below this,
    /// the router will try the paid Brave tier as last resort. Default 1.
    pub min_total_results: usize,
    /// Per-provider failure count over `circuit_window` before the
    /// provider is short-circuited. Default 3.
    pub circuit_threshold: u32,
    /// Rolling window for failure counting (seconds). Default 60.
    pub circuit_window_s: u64,
    /// How long a circuit stays open before retry (seconds). Default 30.
    pub circuit_cooldown_s: u64,
    /// In-memory result cache TTL (seconds). Default 300 (5 min). Set to
    /// 0 to disable.
    pub cache_ttl_s: u64,
    /// Hard timeout on any single provider call (seconds). Default 10.
    pub provider_timeout_s: u64,
    /// Multiplier applied to each provider's reciprocal-rank contribution
    /// during the merge, keyed by the router's internal provider name
    /// (`"tantivy_local"`, `"stract"`, `"searxng"`, `"brave"`, `"serper"`).
    ///
    /// Empty by default, and a missing key means 1.0 — so a config-free
    /// deployment weights every engine identically and fusion changes only
    /// the *order* of the pool the tiering rules already chose, never its
    /// membership. Raise an entry above 1.0 to say "a hit from this engine
    /// is worth more than a hit from the others at the same rank"; that is a
    /// quality judgement about the engine, which is why it lives in operator
    /// config rather than being hard-coded here.
    pub provider_weights: HashMap<String, f32>,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            min_local_results: 3,
            min_total_results: 1,
            circuit_threshold: 3,
            circuit_window_s: 60,
            circuit_cooldown_s: 30,
            cache_ttl_s: 300,
            provider_timeout_s: 10,
            provider_weights: HashMap::new(),
        }
    }
}

/// Per-provider health metadata.
#[derive(Debug, Default)]
struct HealthState {
    /// Failure count within the rolling window.
    failures: AtomicU32,
    /// Unix-second of first failure in current window. 0 = no failures.
    window_start: AtomicU64,
    /// Unix-second of last failure (drives cooldown). 0 = no failures.
    last_failure: AtomicU64,
}

impl HealthState {
    /// Returns true when the provider should be skipped right now.
    fn is_open(&self, cfg: &RouterConfig) -> bool {
        let count = self.failures.load(Ordering::Relaxed);
        if count < cfg.circuit_threshold {
            return false;
        }
        let last = self.last_failure.load(Ordering::Relaxed);
        let now = now_secs();
        now.saturating_sub(last) < cfg.circuit_cooldown_s
    }

    fn record_failure(&self, cfg: &RouterConfig) {
        let now = now_secs();
        let start = self.window_start.load(Ordering::Relaxed);
        if start == 0 || now.saturating_sub(start) > cfg.circuit_window_s {
            // New window — reset counter.
            self.window_start.store(now, Ordering::Relaxed);
            self.failures.store(1, Ordering::Relaxed);
        } else {
            self.failures.fetch_add(1, Ordering::Relaxed);
        }
        self.last_failure.store(now, Ordering::Relaxed);
    }

    fn record_success(&self) {
        // Drain the failure window. Provider is back to healthy.
        self.failures.store(0, Ordering::Relaxed);
        self.window_start.store(0, Ordering::Relaxed);
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Clone)]
struct CacheEntry {
    results: Vec<SearchResult>,
    cached_at: Instant,
}

/// Smart router wrapping the four-tier search stack.
///
/// Implements [`SearchProvider`] so it slots into `AppState` as a drop-in
/// replacement for [`crate::serp::FallbackSearchProvider`].
pub struct SmartSearchRouter {
    tantivy: Option<Arc<dyn SearchProvider>>,
    stract: Option<Arc<dyn SearchProvider>>,
    searxng: Option<Arc<dyn SearchProvider>>,
    /// Paid backup tier — invoked only when free chain returns < min_total_results.
    brave: Option<Arc<dyn SearchProvider>>,
    /// Secondary paid tier (Serper). Tried alongside Brave when both are configured.
    serper: Option<Arc<dyn SearchProvider>>,
    /// Optional async intent classifier. When `None`, the router uses the
    /// pure-rule fast-path (`classify_intent`) exclusively. When `Some`,
    /// every query is run through the classifier — typically a
    /// `CachedClassifier(HybridClassifier(MpIntentClassifier))` stack
    /// that consults the LLM only on rule-default queries and caches
    /// answers across requests.
    intent_classifier: Option<Arc<dyn IntentClassifier>>,
    /// Optional LLM query rewriter (autoprompt). When set, `Research`/
    /// `Comparative` queries are rewritten into a tighter web-search query
    /// before fan-out. `None` → queries dispatch verbatim.
    query_rewriter: Option<Arc<dyn QueryRewriter>>,
    config: RouterConfig,
    health: Arc<HashMap<&'static str, HealthState>>,
    cache: Arc<RwLock<HashMap<String, CacheEntry>>>,
}

/// Builder so callers can mix-and-match which providers exist.
pub struct SmartSearchRouterBuilder {
    tantivy: Option<Arc<dyn SearchProvider>>,
    stract: Option<Arc<dyn SearchProvider>>,
    searxng: Option<Arc<dyn SearchProvider>>,
    brave: Option<Arc<dyn SearchProvider>>,
    serper: Option<Arc<dyn SearchProvider>>,
    intent_classifier: Option<Arc<dyn IntentClassifier>>,
    query_rewriter: Option<Arc<dyn QueryRewriter>>,
    config: RouterConfig,
}

impl SmartSearchRouterBuilder {
    pub fn new() -> Self {
        Self {
            tantivy: None,
            stract: None,
            searxng: None,
            brave: None,
            serper: None,
            intent_classifier: None,
            query_rewriter: None,
            config: RouterConfig::default(),
        }
    }

    pub fn with_tantivy(mut self, p: Arc<dyn SearchProvider>) -> Self {
        self.tantivy = Some(p);
        self
    }
    pub fn with_stract(mut self, p: Arc<dyn SearchProvider>) -> Self {
        self.stract = Some(p);
        self
    }
    pub fn with_searxng(mut self, p: Arc<dyn SearchProvider>) -> Self {
        self.searxng = Some(p);
        self
    }
    pub fn with_brave(mut self, p: Arc<dyn SearchProvider>) -> Self {
        self.brave = Some(p);
        self
    }
    pub fn with_serper(mut self, p: Arc<dyn SearchProvider>) -> Self {
        self.serper = Some(p);
        self
    }
    pub fn with_config(mut self, config: RouterConfig) -> Self {
        self.config = config;
        self
    }

    /// Plug in an async intent classifier. Without one, the router uses
    /// the pure rule fast-path (URL/phrase/freshness keyword detection).
    /// With one, every query goes through it; the recommended composition
    /// is `CachedClassifier(HybridClassifier(MpIntentClassifier))` so
    /// the LLM only fires on rule-default queries and answers are cached.
    pub fn with_intent_classifier(mut self, c: Arc<dyn IntentClassifier>) -> Self {
        self.intent_classifier = Some(c);
        self
    }

    /// Plug in an LLM query rewriter (autoprompt). When set, `Research` and
    /// `Comparative` queries are rewritten into a tighter web-search query
    /// before fan-out. The rewriter is degrade-safe — any failure leaves the
    /// original query untouched.
    pub fn with_query_rewriter(mut self, r: Arc<dyn QueryRewriter>) -> Self {
        self.query_rewriter = Some(r);
        self
    }

    pub fn build(self) -> QuarryResult<SmartSearchRouter> {
        // At least one provider must be configured, else /v1/search would
        // always return empty.
        if self.tantivy.is_none()
            && self.stract.is_none()
            && self.searxng.is_none()
            && self.brave.is_none()
            && self.serper.is_none()
        {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "SmartSearchRouter requires at least one configured provider",
            ));
        }
        let mut health = HashMap::new();
        for name in ["tantivy_local", "stract", "searxng", "brave", "serper"] {
            health.insert(name, HealthState::default());
        }
        Ok(SmartSearchRouter {
            tantivy: self.tantivy,
            stract: self.stract,
            searxng: self.searxng,
            brave: self.brave,
            serper: self.serper,
            intent_classifier: self.intent_classifier,
            query_rewriter: self.query_rewriter,
            config: self.config,
            health: Arc::new(health),
            cache: Arc::new(RwLock::new(HashMap::new())),
        })
    }
}

impl Default for SmartSearchRouterBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl SmartSearchRouter {
    pub fn builder() -> SmartSearchRouterBuilder {
        SmartSearchRouterBuilder::new()
    }

    /// Public for testability — pure rule-only function over the query
    /// string. Always available, even when an async classifier is wired.
    /// The router itself goes through [`Self::classify_async`].
    pub fn classify(&self, query: &str) -> QueryIntent {
        classify_intent(query)
    }

    /// Run the configured async classifier if any, otherwise fall back to
    /// the pure rule classifier. Production wiring layers the rule
    /// fast-path inside [`crate::intent_classifier::HybridClassifier`], so
    /// this method's behavior matches `classify()` for obvious queries
    /// even when an LLM classifier is plugged in.
    pub async fn classify_async(&self, query: &str) -> QueryIntent {
        self.classify_async_for_org(query, None).await
    }

    pub async fn classify_async_for_org(&self, query: &str, org_id: Option<&str>) -> QueryIntent {
        match &self.intent_classifier {
            Some(c) => c.classify_for_org(query, org_id).await,
            None => classify_intent(query),
        }
    }

    /// Resolve the intent that actually drives routing for one request.
    ///
    /// A caller-supplied [`SearchOptions::intent`] **overrides** the router's
    /// own classification rather than merely seeding it. Two reasons:
    ///
    /// * A hint is strictly better evidence than a keyword rule. The rules
    ///   see one bare query string; the caller (the edge, relaying the Model
    ///   Plane) has the conversation, the tool call that produced the query
    ///   and the user's plan. Seeding — "use the hint only where the rules
    ///   say `Default`" — would make the hint powerless in exactly the cases
    ///   it exists for, since a caller sends one precisely when it disagrees
    ///   with what the string looks like ("2026 pricing" reads Fresh to the
    ///   year rule but is a Research fan-out).
    /// * It cannot be abused into a policy bypass. Intent selects only which
    ///   of the router's own paths runs; every path applies the same ZDR,
    ///   paid-provider and circuit-breaker gates, so the worst a wrong or
    ///   hostile hint achieves is a suboptimal route for that one request.
    ///   It also joins the cache key, so it cannot poison anyone else's.
    ///
    /// The hint additionally short-circuits the classifier call entirely,
    /// which on the LLM-backed wiring is the request's only Model Plane hop.
    async fn resolve_intent(&self, query: &str, opts: &SearchOptions) -> QueryIntent {
        match opts.intent {
            Some(hint) => {
                tracing::debug!(intent = ?hint, "router: honoring caller intent hint");
                hint
            }
            None => {
                self.classify_async_for_org(query, opts.org_id.as_deref())
                    .await
            }
        }
    }

    /// Returns the configured cache TTL in seconds. 0 means disabled.
    pub fn cache_ttl(&self) -> u64 {
        self.config.cache_ttl_s
    }

    fn cache_key(query: &str, opts: &SearchOptions) -> String {
        let mut hasher = blake3::Hasher::new();
        hasher.update(query.as_bytes());
        hasher.update(b"|");
        hasher.update(opts.country.as_deref().unwrap_or("").as_bytes());
        hasher.update(b"|");
        hasher.update(opts.language.as_deref().unwrap_or("").as_bytes());
        hasher.update(b"|");
        hasher.update(&opts.limit.to_le_bytes());
        hasher.update(b"|");
        hasher.update(&[opts.safe_search as u8]);
        hasher.update(b"|");
        // Result-affecting filters MUST participate or two requests differing
        // only by topic/recency/phrase/domain would collide on one cache entry.
        hasher.update(opts.topic.as_deref().unwrap_or("").as_bytes());
        hasher.update(b"|");
        hasher.update(opts.time_range.as_deref().unwrap_or("").as_bytes());
        hasher.update(b"|");
        hasher.update(&[opts.exact_match as u8]);
        hasher.update(b"|");
        hasher.update(opts.include_domains.join(",").as_bytes());
        hasher.update(b"|");
        hasher.update(opts.exclude_domains.join(",").as_bytes());
        hasher.update(b"|");
        // Tenant partitioning: org_id MUST participate in the cache key
        // or two tenants searching the same terms would share results
        // (org-A's Tantivy hits would leak to org-B's response).
        hasher.update(opts.org_id.as_deref().unwrap_or("").as_bytes());
        hasher.update(b"|");
        // The intent hint selects the routing topology (which providers run,
        // in which shape), so two requests differing only by intent produce
        // genuinely different result sets. Leaving it out of the key would
        // let the first caller's routing be served silently to the second —
        // e.g. a `Fresh` hint's live-SERP results answering a later
        // `Navigational` request for the same string, or vice versa.
        hasher.update(&[intent_cache_tag(opts.intent)]);
        hasher.finalize().to_hex().to_string()
    }

    async fn cache_get(&self, key: &str) -> Option<Vec<SearchResult>> {
        if self.config.cache_ttl_s == 0 {
            return None;
        }
        let map = self.cache.read().await;
        let entry = map.get(key)?;
        if entry.cached_at.elapsed() > Duration::from_secs(self.config.cache_ttl_s) {
            return None;
        }
        Some(entry.results.clone())
    }

    async fn cache_put(&self, key: String, results: &[SearchResult]) {
        if self.config.cache_ttl_s == 0 {
            return;
        }
        let mut map = self.cache.write().await;
        // Cap cache size to prevent unbounded growth — drop oldest on
        // overflow. Threshold is high enough that hot keys stay warm.
        const MAX_ENTRIES: usize = 10_000;
        if map.len() >= MAX_ENTRIES {
            // Cheap eviction: clear half. Better than complex LRU for
            // this scale.
            let to_drop: Vec<String> = map.keys().take(MAX_ENTRIES / 2).cloned().collect();
            for k in to_drop {
                map.remove(&k);
            }
        }
        map.insert(
            key,
            CacheEntry {
                results: results.to_vec(),
                cached_at: Instant::now(),
            },
        );
    }

    /// Run a provider with timeout + health tracking.
    async fn run_provider(
        &self,
        name: &'static str,
        provider: &Arc<dyn SearchProvider>,
        query: &str,
        opts: &SearchOptions,
    ) -> Option<Vec<SearchResult>> {
        let Some(state) = self.health.get(name) else {
            // Unknown provider name — call but don't track health.
            return run_with_timeout(provider, query, opts, self.config.provider_timeout_s)
                .await
                .ok()
                .map(annotate(name));
        };
        if state.is_open(&self.config) {
            tracing::debug!(provider = name, "circuit open; skipping");
            return None;
        }
        match run_with_timeout(provider, query, opts, self.config.provider_timeout_s).await {
            Ok(results) => {
                state.record_success();
                Some(annotate(name)(results))
            }
            Err(e) => {
                state.record_failure(&self.config);
                tracing::warn!(provider = name, error = %e, "provider failed");
                None
            }
        }
    }

    /// Default path: Tantivy → maybe widen to Stract+SearXNG → Brave/Serper as last resort.
    async fn default_path(
        &self,
        query: &str,
        opts: &SearchOptions,
    ) -> QuarryResult<Vec<SearchResult>> {
        let mut combined: Vec<SearchResult> = Vec::new();
        let target_results = self.target_results(opts);
        let mut eager_paid_backups = false;

        // 1. Hit Tantivy first (synchronous, microseconds).
        if let Some(t) = &self.tantivy {
            if let Some(local) = self.run_provider("tantivy_local", t, query, opts).await {
                // Through `merge_dedupe` rather than `extend` even though
                // `combined` is empty here: that is where a result acquires
                // its fusion score, and a local hit that skipped it would be
                // scoreless when the widened providers are fused in below.
                combined = merge_dedupe(&self.config, combined, local);
            }
        }

        // 2. Widen only if local corpus didn't meet the threshold.
        if combined.len() < self.config.min_local_results {
            // Both gate terms, via `paid_allowed()`: never race Brave/Serper
            // in eagerly for a ZDR request no matter how terse the query
            // looks, nor for a tenant whose plan did not grant paid fan-out
            // — see module doc points 5 and 6.
            eager_paid_backups = opts.paid_allowed()
                && combined.is_empty()
                && should_eagerly_run_paid_backups(query, opts);
            let mut providers: Vec<(&'static str, Arc<dyn SearchProvider>)> = Vec::new();
            if let Some(s) = self.stract.clone() {
                providers.push(("stract", s));
            }
            if let Some(s) = self.searxng.clone() {
                providers.push(("searxng", s));
            }
            // Broad "head" queries from humans and LLMs are often terse
            // entity lookups ("OpenAI", "Stripe API"). If the local corpus
            // is empty, start the paid backup tier immediately instead of
            // waiting for a slow free provider to time out first.
            if eager_paid_backups {
                if let Some(s) = self.serper.clone() {
                    providers.insert(0, ("serper", s));
                }
                if let Some(b) = self.brave.clone() {
                    providers.insert(0, ("brave", b));
                }
            }
            combined = self
                .collect_parallel(query, opts, combined, target_results, providers)
                .await;
        }

        // 3. Paid backup only when free chain returned <min_total_results.
        // `paid_allowed()` here is load-bearing, not defense in depth — this
        // is the last point before Brave/Serper would be dispatched.
        if opts.paid_allowed()
            && !eager_paid_backups
            && combined.len() < self.config.min_total_results
        {
            if let Some(b) = &self.brave {
                if let Some(rs) = self.run_provider("brave", b, query, opts).await {
                    combined = merge_dedupe(&self.config, combined, rs);
                }
            }
            if combined.len() < self.config.min_total_results {
                if let Some(s) = &self.serper {
                    if let Some(rs) = self.run_provider("serper", s, query, opts).await {
                        combined = merge_dedupe(&self.config, combined, rs);
                    }
                }
            }
        }

        Ok(rank_and_truncate(combined, opts.limit))
    }

    /// Fresh path: skip Tantivy. Parallel-fan-out to live SERPs.
    async fn fresh_path(
        &self,
        query: &str,
        opts: &SearchOptions,
    ) -> QuarryResult<Vec<SearchResult>> {
        let mut combined: Vec<SearchResult> = Vec::new();
        let mut futures = Vec::new();
        if let Some(s) = self.stract.clone() {
            let h = self.clone_health();
            let cfg = self.config.clone();
            let q = query.to_string();
            let o = opts.clone();
            futures.push(tokio::spawn(async move {
                run_one(&h, "stract", &s, &q, &o, &cfg).await
            }));
        }
        if let Some(s) = self.searxng.clone() {
            let h = self.clone_health();
            let cfg = self.config.clone();
            let q = query.to_string();
            let o = opts.clone();
            futures.push(tokio::spawn(async move {
                run_one(&h, "searxng", &s, &q, &o, &cfg).await
            }));
        }
        let outputs = futures::future::join_all(futures).await;
        for out in outputs {
            if let Ok(Some(rs)) = out {
                combined = merge_dedupe(&self.config, combined, rs);
            }
        }
        // Paid backup if both free providers failed/empty. Skip Brave
        // entirely unless BOTH gate terms hold — see module doc points 5, 6.
        if opts.paid_allowed() && combined.is_empty() {
            if let Some(b) = &self.brave {
                if let Some(rs) = self.run_provider("brave", b, query, opts).await {
                    combined = merge_dedupe(&self.config, combined, rs);
                }
            }
        }
        Ok(rank_and_truncate(combined, opts.limit))
    }

    fn clone_health(&self) -> Arc<HashMap<&'static str, HealthState>> {
        self.health.clone()
    }

    fn target_results(&self, opts: &SearchOptions) -> usize {
        opts.limit
            .max(self.config.min_local_results as u32)
            .max(self.config.min_total_results as u32)
            .max(1) as usize
    }

    async fn collect_parallel(
        &self,
        query: &str,
        opts: &SearchOptions,
        mut combined: Vec<SearchResult>,
        target_results: usize,
        providers: Vec<(&'static str, Arc<dyn SearchProvider>)>,
    ) -> Vec<SearchResult> {
        if providers.is_empty() || combined.len() >= target_results {
            return combined;
        }

        let mut joins = JoinSet::new();
        let provider_count = providers.len();
        let mut resolved: Vec<Option<Option<Vec<SearchResult>>>> = vec![None; provider_count];
        for (idx, (name, provider)) in providers.into_iter().enumerate() {
            let health = self.clone_health();
            let config = self.config.clone();
            let q = query.to_string();
            let o = opts.clone();
            joins.spawn(async move {
                (
                    idx,
                    run_one(&health, name, &provider, &q, &o, &config).await,
                )
            });
        }

        let mut next_to_merge = 0usize;
        while let Some(out) = joins.join_next().await {
            if let Ok((idx, results)) = out {
                resolved[idx] = Some(results);
                while next_to_merge < provider_count {
                    let Some(slot) = resolved[next_to_merge].take() else {
                        break;
                    };
                    if let Some(results) = slot {
                        combined = merge_dedupe(&self.config, combined, results);
                    }
                    next_to_merge += 1;
                    if combined.len() >= target_results {
                        joins.abort_all();
                        break;
                    }
                }
            }
        }

        while joins.join_next().await.is_some() {}

        combined
    }

    /// Research / Comparative path: unconditionally fan out to **every**
    /// configured free provider in parallel, then merge + dedupe. Skips
    /// the "is the local corpus enough" gate that `default_path` uses —
    /// research queries explicitly want breadth, so we always pay the
    /// cost of all free engines. Paid backup still gated on empty result.
    async fn widen_path(
        &self,
        query: &str,
        opts: &SearchOptions,
    ) -> QuarryResult<Vec<SearchResult>> {
        let mut combined: Vec<SearchResult> = Vec::new();
        let mut futures = Vec::new();

        if let Some(t) = self.tantivy.clone() {
            let h = self.clone_health();
            let cfg = self.config.clone();
            let q = query.to_string();
            let o = opts.clone();
            futures.push(tokio::spawn(async move {
                run_one(&h, "tantivy_local", &t, &q, &o, &cfg).await
            }));
        }
        if let Some(s) = self.stract.clone() {
            let h = self.clone_health();
            let cfg = self.config.clone();
            let q = query.to_string();
            let o = opts.clone();
            futures.push(tokio::spawn(async move {
                run_one(&h, "stract", &s, &q, &o, &cfg).await
            }));
        }
        if let Some(s) = self.searxng.clone() {
            let h = self.clone_health();
            let cfg = self.config.clone();
            let q = query.to_string();
            let o = opts.clone();
            futures.push(tokio::spawn(async move {
                run_one(&h, "searxng", &s, &q, &o, &cfg).await
            }));
        }
        let outputs = futures::future::join_all(futures).await;
        for out in outputs {
            if let Ok(Some(rs)) = out {
                combined = merge_dedupe(&self.config, combined, rs);
            }
        }

        // Paid backup only when the free fan-out came back empty —
        // research queries shouldn't fall to paid silently when we got
        // good free results. Skip Brave/Serper entirely unless BOTH gate
        // terms hold — see module doc points 5 and 6.
        if opts.paid_allowed() && combined.len() < self.config.min_total_results {
            if let Some(b) = &self.brave {
                if let Some(rs) = self.run_provider("brave", b, query, opts).await {
                    combined = merge_dedupe(&self.config, combined, rs);
                }
            }
            if combined.len() < self.config.min_total_results {
                if let Some(s) = &self.serper {
                    if let Some(rs) = self.run_provider("serper", s, query, opts).await {
                        combined = merge_dedupe(&self.config, combined, rs);
                    }
                }
            }
        }

        Ok(rank_and_truncate(combined, opts.limit))
    }
}

/// Helper for spawned futures — runs a provider with timeout + health.
async fn run_one(
    health: &Arc<HashMap<&'static str, HealthState>>,
    name: &'static str,
    provider: &Arc<dyn SearchProvider>,
    query: &str,
    opts: &SearchOptions,
    config: &RouterConfig,
) -> Option<Vec<SearchResult>> {
    let Some(state) = health.get(name) else {
        return run_with_timeout(provider, query, opts, config.provider_timeout_s)
            .await
            .ok()
            .map(annotate(name));
    };
    if state.is_open(config) {
        return None;
    }
    match run_with_timeout(provider, query, opts, config.provider_timeout_s).await {
        Ok(results) => {
            state.record_success();
            Some(annotate(name)(results))
        }
        Err(_) => {
            state.record_failure(config);
            None
        }
    }
}

async fn run_with_timeout(
    provider: &Arc<dyn SearchProvider>,
    query: &str,
    opts: &SearchOptions,
    timeout_s: u64,
) -> QuarryResult<Vec<SearchResult>> {
    tokio::time::timeout(Duration::from_secs(timeout_s), provider.search(query, opts))
        .await
        .map_err(|_| QuarryError::new(ErrorCode::Timeout, "provider call timed out"))?
}

/// Annotate every result with the provider name so the merged response
/// shows which engine returned what. Defensive — provider impls already
/// set this, but we re-stamp to guarantee.
fn annotate(name: &'static str) -> impl Fn(Vec<SearchResult>) -> Vec<SearchResult> {
    move |mut results| {
        for r in &mut results {
            if r.provider.is_empty() {
                r.provider = name.to_string();
            }
        }
        results
    }
}

/// Query parameters that carry campaign/attribution state and never change
/// which document a URL points at.
const TRACKING_PARAMS: &[&str] = &[
    "fbclid", "gclid", "msclkid", "yclid", "igshid", "mc_cid", "mc_eid", "ref", "ref_src",
    "_hsenc", "_hsmi",
];

fn is_tracking_param(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    k.starts_with("utm_") || TRACKING_PARAMS.contains(&k.as_str())
}

/// Canonical form of a URL, for use **as a dedupe key only**.
///
/// This must never replace the URL handed back to the caller: the user still
/// has to get the real, clickable link the provider returned, tracking
/// parameters and all. Its only job is to stop one document from occupying
/// several of the caller's five result slots — an exact-string dedupe let
/// http vs https, `www.` vs not, a trailing slash and a campaign tag through
/// as four separate "sources", which both starved the budget and read to the
/// answer layer as independent corroboration.
///
/// Unparsable input falls back to the lowercased raw string, so a malformed
/// URL still dedupes against an identical malformed URL rather than being
/// silently dropped.
fn dedupe_key(raw: &str) -> String {
    let trimmed = raw.trim();
    let Ok(parsed) = Url::parse(trimmed) else {
        return trimmed.to_lowercase();
    };
    let scheme = match parsed.scheme() {
        // http and https address the same document; a provider returning the
        // http variant of a page another provider returned over https is not
        // a second source.
        "http" | "https" => "http",
        other => other,
    };
    let host_owned = parsed.host_str().unwrap_or_default().to_lowercase();
    let host = host_owned.strip_prefix("www.").unwrap_or(&host_owned);
    let port = parsed.port().map(|p| format!(":{p}")).unwrap_or_default();
    let path = parsed.path();
    let path = path.strip_suffix('/').unwrap_or(path);
    let kept: Vec<String> = parsed
        .query_pairs()
        .filter(|(k, _)| !is_tracking_param(k))
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    let query = if kept.is_empty() {
        String::new()
    } else {
        format!("?{}", kept.join("&"))
    };
    // The fragment is dropped: `…/page#intro` and `…/page` are one document,
    // and the caller keeps whichever form arrived first.
    format!("{scheme}://{host}{port}{path}{query}")
}

/// Weight multiplier for one provider's contributions. Missing = 1.0, so an
/// operator who configures nothing gets an unweighted fusion.
fn provider_weight(cfg: &RouterConfig, provider: &str) -> f32 {
    cfg.provider_weights
        .get(provider)
        .copied()
        .unwrap_or(1.0_f32)
}

/// This result's evidence value before the per-provider weight.
///
/// A provider that already fused across its own upstream engines (SearXNG,
/// via `serp::rank_by_fusion`) hands us a `fusion_score` that is a sum over
/// those engines, and that sum is the honest measure of how many independent
/// engines backed the URL — so it is used as-is. Everything else is a single
/// opinion at one rank, worth the canonical `1/(k + rank)`.
///
/// Reusing `fusion::RRF_K` matters: the router's scale has to be the SearXNG
/// adapter's scale or a single-engine provider's hit and a metasearch hit
/// would not be comparable, and whichever constant was larger would win the
/// merge on units alone.
fn rrf_contrib(result: &SearchResult) -> f32 {
    result
        .fusion_score
        .unwrap_or_else(|| 1.0 / (RRF_K + result.rank as f32))
}

/// [`rrf_contrib`] scaled by the contributing provider's configured weight.
fn weighted_contrib(cfg: &RouterConfig, result: &SearchResult) -> f32 {
    provider_weight(cfg, &result.provider) * rrf_contrib(result)
}

/// Merge two result lists, deduping on the canonicalised URL ([`dedupe_key`])
/// while the stored `url` stays exactly as the provider returned it. Keeps the
/// first (higher-priority) occurrence — the router supplies its providers in
/// priority order — and *fuses* the loser into it instead of discarding it.
///
/// Dropping the duplicate wasted the single strongest quality signal the
/// router has. Provider A and provider B independently returning the same URL
/// is corroboration; a URL only one of them found is not. Under the old
/// "first wins, rest dropped" rule those two cases produced identical output,
/// so the final order was decided by which provider happened to be earlier in
/// the tier list rather than by what the engines actually agreed on. Summing
/// reciprocal-rank contributions turns each extra sighting into score.
///
/// Every result leaves this function carrying a `fusion_score`, including
/// ones that were never duplicated — their score is simply their own single
/// contribution. That is deliberate: the ranking step downstream has to
/// compare merged and unmerged results on one scale, and a `None` that means
/// "never collided" is indistinguishable at that point from "scored zero".
///
/// The `HashMap` is a key→index lookup only and is never iterated to build
/// the output; the output order is `base` then first-seen `extra`, which is
/// what makes a run reproducible.
fn merge_dedupe(
    cfg: &RouterConfig,
    mut base: Vec<SearchResult>,
    extra: Vec<SearchResult>,
) -> Vec<SearchResult> {
    let mut index: HashMap<String, usize> = HashMap::with_capacity(base.len() + extra.len());
    for (i, r) in base.iter().enumerate() {
        index.entry(dedupe_key(&r.url)).or_insert(i);
    }

    for mut r in extra {
        let key = dedupe_key(&r.url);
        match index.get(&key).copied() {
            Some(i) => {
                let incoming = weighted_contrib(cfg, &r);
                let kept = &mut base[i];
                // `kept.fusion_score` is already an accumulated, weighted
                // total whenever this result came through the push branch, so
                // it is added as-is and never re-weighted. The fallback only
                // fires for a result some caller placed in `base` directly.
                let running = match kept.fusion_score {
                    Some(s) => s,
                    None => weighted_contrib(cfg, kept),
                };
                kept.fusion_score = Some(running + incoming);
                // Union the engine provenance: the kept result's URL was, in
                // fact, returned by the loser's engines too, and the length of
                // this vector is what the answer layer reads as "how many
                // independent sources".
                for engine in r.engines.drain(..) {
                    if !kept.engines.contains(&engine) {
                        kept.engines.push(engine);
                    }
                }
                // Only fill a gap. The kept result is the higher-priority
                // provider's, and overwriting its snippet with a lower tier's
                // would silently change what the caller sees for a URL the
                // preferred provider already described.
                if kept.snippet.as_deref().is_none_or(|s| s.trim().is_empty())
                    && r.snippet.is_some()
                {
                    kept.snippet = r.snippet.take();
                }
            }
            None => {
                r.fusion_score = Some(weighted_contrib(cfg, &r));
                index.insert(key, base.len());
                base.push(r);
            }
        }
    }
    base
}

/// Order a merged pool by fused evidence, cut it to the caller's limit, then
/// renumber ranks 1..n.
///
/// Membership of the pool is settled before this runs — tiering, circuit
/// breakers, the ZDR/paid gate and `min_total_results` all decide *which
/// providers ran*, and this function never adds or removes a provider's
/// results, only reorders what they returned. That separation is the reason
/// enabling fusion cannot change which upstreams a request touches.
///
/// The sort is STABLE, so equal scores keep insertion order — which is the
/// provider priority order the tier list defines. That is what keeps tiering
/// meaningful under fusion: with no weights configured, a lower tier's hit
/// only climbs above a higher tier's by being corroborated or by being ranked
/// better within its own list, never by arriving from a different provider.
fn rank_and_truncate(mut results: Vec<SearchResult>, limit: u32) -> Vec<SearchResult> {
    results.sort_by(|a, b| rrf_contrib(b).total_cmp(&rrf_contrib(a)));
    results.truncate(limit.max(1) as usize);
    for (i, r) in results.iter_mut().enumerate() {
        r.rank = (i as u32) + 1;
    }
    results
}

fn should_eagerly_run_paid_backups(query: &str, opts: &SearchOptions) -> bool {
    let q = query.trim();
    if q.is_empty()
        || opts.exact_match
        || opts.topic.is_some()
        || opts.time_range.is_some()
        || q.contains("://")
        || q.contains('/')
        || q.contains('.')
    {
        return false;
    }

    let terms: Vec<&str> = q.split_whitespace().collect();
    if terms.is_empty() || terms.len() > 2 {
        return false;
    }

    terms.iter().all(|term| {
        term.len() >= 2
            && term
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '-' | '&' | '+'))
    })
}

#[async_trait]
impl SearchProvider for SmartSearchRouter {
    async fn search(&self, query: &str, opts: &SearchOptions) -> QuarryResult<Vec<SearchResult>> {
        let q = query.trim();
        if q.is_empty() {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "query must not be empty",
            ));
        }

        // Cache check first — same query+opts within TTL returns cached.
        let cache_key = Self::cache_key(q, opts);
        if let Some(hit) = self.cache_get(&cache_key).await {
            return Ok(hit);
        }

        let intent = self.resolve_intent(q, opts).await;
        let results = match intent {
            QueryIntent::Fresh => self.fresh_path(q, opts).await?,
            // Research / Comparative always fan out to every free engine
            // — depth over efficiency. Autoprompt (when wired) rewrites the
            // verbose question into a tighter query first; the cache key above
            // stays on the original query so repeats still hit cache without
            // re-invoking the rewriter.
            QueryIntent::Research | QueryIntent::Comparative => {
                let dispatch: Cow<'_, str> = match &self.query_rewriter {
                    Some(rw) => {
                        let rewritten = rw.rewrite_for_org(q, opts.org_id.as_deref()).await;
                        if rewritten != q {
                            tracing::debug!(original = q, rewritten = %rewritten, "autoprompt: query rewritten");
                        }
                        Cow::Owned(rewritten)
                    }
                    None => Cow::Borrowed(q),
                };
                self.widen_path(dispatch.as_ref(), opts).await?
            }
            // Local biases the per-call options towards the user's
            // country before falling into the standard default topology.
            // If the caller didn't supply a country, leave None — the
            // downstream provider will fall back to its own defaults
            // rather than pick a wrong one. The intent label flows
            // through to result.provider for telemetry.
            QueryIntent::Local => self.default_path(q, opts).await?,
            // Code: Tantivy first (we likely indexed dev docs), then
            // widen to Stract which has good code-repo coverage. The
            // existing default_path topology delivers this.
            QueryIntent::Code => self.default_path(q, opts).await?,
            // Navigational and Phrase use the same default-path topology.
            // The bias (URL field weighting, exact-phrase tokenization)
            // lives inside the Tantivy provider via its query parser.
            QueryIntent::Navigational | QueryIntent::Phrase | QueryIntent::Default => {
                self.default_path(q, opts).await?
            }
        };

        // Store in cache (best-effort — drop on full).
        self.cache_put(cache_key, &results).await;

        Ok(results)
    }

    fn name(&self) -> &str {
        "smart_router"
    }
}

/// Pure intent classifier. Public so it can be unit-tested without
/// constructing a router.
pub fn classify_intent(query: &str) -> QueryIntent {
    let q = query.trim();
    if q.is_empty() {
        return QueryIntent::Default;
    }

    // 1. Phrase: surrounded by double quotes (entire query).
    if q.starts_with('"') && q.ends_with('"') && q.len() >= 2 {
        return QueryIntent::Phrase;
    }

    // 2. Navigational: looks like a URL or bare host.
    if q.starts_with("http://") || q.starts_with("https://") {
        return QueryIntent::Navigational;
    }
    // Bare host heuristic: contains a dot, no spaces, alnum/.-/_/: chars only.
    if !q.contains(' ')
        && q.contains('.')
        && q.chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '/'))
    {
        return QueryIntent::Navigational;
    }

    // 3. Fresh: contains time-sensitive keywords.
    let lower = q.to_lowercase();
    for kw in FRESH_KEYWORDS {
        if contains_word(&lower, kw) {
            return QueryIntent::Fresh;
        }
    }
    // Year-like 4-digit token in the current decade is also a freshness signal.
    for token in lower.split_whitespace() {
        if token.len() == 4 {
            if let Ok(year) = token.parse::<u32>() {
                if (2024..=2030).contains(&year) {
                    return QueryIntent::Fresh;
                }
            }
        }
    }

    QueryIntent::Default
}

/// True when `keyword` occurs in `haystack` as a whole word or whole phrase.
///
/// A plain `contains` is not safe for this list: English "now" is a substring
/// of "knowledge" and "live" of "delivery", and the Norwegian entries are far
/// worse — "nå" sits inside "når"/"nåværende" and "i år" inside "i årets".
/// Substring matching therefore routes ordinary questions down the Fresh path,
/// skipping the local corpus for no reason. Both ends of a match must be a
/// non-word character (or the string edge).
///
/// `haystack` and `keyword` are both expected already lowercased.
fn contains_word(haystack: &str, keyword: &str) -> bool {
    let mut from = 0usize;
    while let Some(rel) = haystack[from..].find(keyword) {
        let start = from + rel;
        let end = start + keyword.len();
        let opens = haystack[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !is_word_char(c));
        let closes = haystack[end..]
            .chars()
            .next()
            .is_none_or(|c| !is_word_char(c));
        if opens && closes {
            return true;
        }
        // Step past this occurrence's first character (not its whole length)
        // so an overlapping later match is still considered.
        from = start + haystack[start..].chars().next().map_or(1, char::len_utf8);
    }
    false
}

/// Word constituent for [`contains_word`]. `char::is_alphanumeric` is the
/// Unicode-aware form, which matters here: 'å'/'ø'/'æ' must count as letters
/// or every Norwegian keyword would match mid-word.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Keywords that flip a query into the "fresh" bucket and skip the
/// stale local corpus.
///
/// Norwegian sits alongside English because Verevon's users write their
/// recency queries in Norwegian ("siste nytt", "i går", "denne uken"); an
/// English-only list left every one of them on the stale local corpus.
/// Bokmål is the primary form, with the nynorsk spellings that actually
/// differ ("nyhende", "veka", "nyleg", "i morgon") listed next to it.
/// Deliberately absent: nynorsk "no" for "now" (unusable — it collides with
/// English "no" and the ISO country code) and "fersk"/"ferske", which in a
/// food-sector tenant far more often means fresh produce than recent news.
const FRESH_KEYWORDS: &[&str] = &[
    // English.
    "today",
    "yesterday",
    "tomorrow",
    "now",
    "latest",
    "current",
    "breaking",
    "live",
    "this week",
    "this month",
    "this year",
    "recent",
    "news",
    // Norwegian — bokmål, plus differing nynorsk forms.
    "i dag",
    "idag",
    "i går",
    "igår",
    "i morgen",
    "imorgen",
    "i morgon",
    "imorgon",
    "nå",
    "siste",
    "siste nytt",
    "nyeste",
    "nyheter",
    "nyhende",
    "aktuelt",
    "oppdatert",
    "oppdatering",
    "nylig",
    "nyleg",
    "denne uken",
    "denne uka",
    "denne veka",
    "denne måneden",
    "denne månaden",
    "i år",
    "iår",
    "årets",
    "i fjor",
    "direkte",
];

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Intent classifier ------------------------------------------------

    #[test]
    fn classifies_url_as_navigational() {
        assert_eq!(
            classify_intent("https://example.com/page"),
            QueryIntent::Navigational
        );
        assert_eq!(classify_intent("http://docs.rs"), QueryIntent::Navigational);
    }

    #[test]
    fn classifies_bare_domain_as_navigational() {
        assert_eq!(classify_intent("github.com"), QueryIntent::Navigational);
        assert_eq!(
            classify_intent("docs.example.com/rust"),
            QueryIntent::Navigational
        );
    }

    #[test]
    fn classifies_quoted_phrase_as_phrase() {
        assert_eq!(classify_intent("\"exact match\""), QueryIntent::Phrase);
    }

    #[test]
    fn classifies_today_as_fresh() {
        assert_eq!(classify_intent("stock prices today"), QueryIntent::Fresh);
        assert_eq!(classify_intent("latest rust release"), QueryIntent::Fresh);
        assert_eq!(classify_intent("breaking news"), QueryIntent::Fresh);
    }

    #[test]
    fn classifies_year_2026_as_fresh() {
        assert_eq!(classify_intent("rust roadmap 2026"), QueryIntent::Fresh);
    }

    #[test]
    fn classifies_neutral_as_default() {
        assert_eq!(classify_intent("how does tokio work"), QueryIntent::Default);
        assert_eq!(
            classify_intent("rust ownership rules"),
            QueryIntent::Default
        );
    }

    #[test]
    fn classifies_norwegian_recency_as_fresh() {
        for q in [
            "siste nytt om laks",
            "strømpris i dag",
            "hva skjedde i går",
            "hva skjer nå",
            "leveranser denne uken",
            "nyheter om mattilsynet",
            "oppdatert prisliste",
            "nylig publiserte rapporter",
            "årets resultat",
            "omsetning i fjor",
            "i morgen åpner messen",
            "nyhende frå kysten",
            "denne veka",
            "direkte frå messa",
        ] {
            assert_eq!(classify_intent(q), QueryIntent::Fresh, "query: {q}");
        }
    }

    #[test]
    fn norwegian_keywords_do_not_match_inside_longer_words() {
        // "nå" inside "når"/"nåværende", "i år" inside "i årsrapporten",
        // "siste" inside "sistemann" — all Default, not Fresh.
        for q in [
            "når kommer neste versjon",
            "nåværende leverandør av emballasje",
            "hva står i årsrapporten",
            "hvem er sistemann i køen",
            "idagsverk per ansatt",
        ] {
            assert_eq!(classify_intent(q), QueryIntent::Default, "query: {q}");
        }
    }

    #[test]
    fn english_keywords_are_word_bounded_too() {
        // "now" inside "knowledge" and "live" inside "delivery" used to flip
        // these to Fresh under substring matching.
        assert_eq!(
            classify_intent("knowledge base articles"),
            QueryIntent::Default
        );
        assert_eq!(classify_intent("delivery terms"), QueryIntent::Default);
        assert_eq!(classify_intent("what is happening now"), QueryIntent::Fresh);
    }

    #[test]
    fn contains_word_respects_boundaries() {
        assert!(contains_word("hva skjer nå", "nå"));
        assert!(contains_word("nå er det slutt", "nå"));
        assert!(!contains_word("når kommer den", "nå"));
        assert!(!contains_word("nåværende", "nå"));
        // Overlapping candidate: the first "i dag" is mid-word, the second
        // is a real hit — the scan must not stop at the first rejection.
        assert!(contains_word("i dagligvare og i dag", "i dag"));
        assert!(!contains_word("i dagligvare", "i dag"));
    }

    #[test]
    fn empty_query_is_default() {
        assert_eq!(classify_intent("  "), QueryIntent::Default);
    }

    // ---- HealthState ------------------------------------------------------

    #[test]
    fn health_starts_closed() {
        let cfg = RouterConfig::default();
        let h = HealthState::default();
        assert!(!h.is_open(&cfg));
    }

    #[test]
    fn health_opens_after_threshold_failures() {
        let cfg = RouterConfig {
            circuit_threshold: 2,
            ..Default::default()
        };
        let h = HealthState::default();
        h.record_failure(&cfg);
        assert!(!h.is_open(&cfg));
        h.record_failure(&cfg);
        assert!(h.is_open(&cfg));
    }

    #[test]
    fn health_resets_on_success() {
        let cfg = RouterConfig {
            circuit_threshold: 1,
            ..Default::default()
        };
        let h = HealthState::default();
        h.record_failure(&cfg);
        h.record_failure(&cfg);
        assert!(h.is_open(&cfg));
        h.record_success();
        assert!(!h.is_open(&cfg));
    }

    // ---- Cache key --------------------------------------------------------

    #[test]
    fn cache_key_is_stable_for_same_inputs() {
        let opts = SearchOptions::default();
        let k1 = SmartSearchRouter::cache_key("rust", &opts);
        let k2 = SmartSearchRouter::cache_key("rust", &opts);
        assert_eq!(k1, k2);
    }

    #[test]
    fn cache_key_differs_on_query_change() {
        let opts = SearchOptions::default();
        let k1 = SmartSearchRouter::cache_key("rust", &opts);
        let k2 = SmartSearchRouter::cache_key("go", &opts);
        assert_ne!(k1, k2);
    }

    #[test]
    fn cache_key_differs_on_limit_change() {
        let mut a = SearchOptions::default();
        let mut b = SearchOptions::default();
        a.limit = 10;
        b.limit = 20;
        let k1 = SmartSearchRouter::cache_key("rust", &a);
        let k2 = SmartSearchRouter::cache_key("rust", &b);
        assert_ne!(k1, k2);
    }

    // ---- Merge / dedupe ---------------------------------------------------

    fn r(url: &str, provider: &str) -> SearchResult {
        SearchResult {
            url: url.into(),
            title: None,
            snippet: None,
            rank: 1,
            provider: provider.into(),
            ..Default::default()
        }
    }

    #[test]
    fn merge_dedupe_keeps_first_occurrence() {
        let base = vec![
            r("https://a/", "tantivy_local"),
            r("https://b/", "tantivy_local"),
        ];
        let extra = vec![r("https://b/", "stract"), r("https://c/", "stract")];
        let out = merge_dedupe(&RouterConfig::default(), base, extra);
        assert_eq!(out.len(), 3);
        // b should retain its tantivy_local provenance.
        assert_eq!(
            out.iter().find(|r| r.url == "https://b/").unwrap().provider,
            "tantivy_local"
        );
        assert!(out
            .iter()
            .any(|r| r.url == "https://c/" && r.provider == "stract"));
    }

    #[test]
    fn dedupe_key_collapses_cosmetic_url_variants() {
        let canonical = dedupe_key("https://example.com/artikkel");
        for variant in [
            "http://example.com/artikkel",
            "https://www.example.com/artikkel",
            "https://WWW.Example.COM/artikkel",
            "https://example.com/artikkel/",
            "https://example.com/artikkel#toppen",
            "https://example.com/artikkel?utm_source=nyhetsbrev&utm_medium=email",
            "https://example.com/artikkel?fbclid=abc123",
            "https://example.com/artikkel?gclid=xyz&ref=partner",
            "  https://example.com/artikkel  ",
        ] {
            assert_eq!(dedupe_key(variant), canonical, "variant: {variant}");
        }
    }

    #[test]
    fn dedupe_key_keeps_meaningful_differences() {
        let base = dedupe_key("https://example.com/artikkel");
        // Distinct documents must stay distinct.
        assert_ne!(dedupe_key("https://example.com/annen"), base);
        assert_ne!(dedupe_key("https://blog.example.com/artikkel"), base);
        assert_ne!(dedupe_key("https://example.com:8443/artikkel"), base);
        // A non-tracking query parameter selects a different document.
        assert_ne!(dedupe_key("https://example.com/artikkel?side=2"), base);
        // ...and survives alongside a stripped one.
        assert_eq!(
            dedupe_key("https://example.com/artikkel?side=2&utm_campaign=q1"),
            dedupe_key("https://example.com/artikkel?side=2")
        );
    }

    #[test]
    fn dedupe_key_falls_back_to_raw_string_when_unparsable() {
        assert_eq!(dedupe_key("not a url"), "not a url");
        assert_eq!(dedupe_key(" NOT A URL "), "not a url");
    }

    #[test]
    fn merge_dedupe_collapses_canonical_duplicates() {
        let base = vec![r("https://example.com/artikkel", "tantivy_local")];
        let extra = vec![
            r("http://www.example.com/artikkel/?utm_source=x", "stract"),
            r("https://example.com/annen", "stract"),
        ];
        let out = merge_dedupe(&RouterConfig::default(), base, extra);
        assert_eq!(out.len(), 2);
        // The duplicate is dropped; the survivor keeps its first-seen
        // provenance and its original, untouched URL.
        assert_eq!(out[0].url, "https://example.com/artikkel");
        assert_eq!(out[0].provider, "tantivy_local");
        assert_eq!(out[1].url, "https://example.com/annen");
    }

    #[test]
    fn merge_dedupe_returns_untouched_urls_to_the_caller() {
        // Canonicalisation is a key, not a rewrite: the tracking params the
        // user needs to actually reach the page must survive into the output.
        let tracked = "https://example.com/artikkel?utm_source=nyhetsbrev";
        let out = merge_dedupe(
            &RouterConfig::default(),
            Vec::new(),
            vec![r(tracked, "searxng")],
        );
        assert_eq!(out[0].url, tracked);
    }

    #[test]
    fn eager_paid_backup_heuristic_is_narrow() {
        let opts = SearchOptions::default();
        assert!(should_eagerly_run_paid_backups("OpenAI", &opts));
        assert!(should_eagerly_run_paid_backups("stripe api", &opts));
        assert!(!should_eagerly_run_paid_backups(
            "how does tokio work",
            &opts
        ));
        assert!(!should_eagerly_run_paid_backups("github.com", &opts));
        assert!(!should_eagerly_run_paid_backups("\"exact match\"", &opts));
    }

    // ---- Router behavior --------------------------------------------------

    /// Test SearchProvider that returns a canned list.
    struct Canned(Vec<SearchResult>);

    #[async_trait]
    impl SearchProvider for Canned {
        async fn search(
            &self,
            _query: &str,
            _opts: &SearchOptions,
        ) -> QuarryResult<Vec<SearchResult>> {
            Ok(self.0.clone())
        }
        fn name(&self) -> &str {
            "canned"
        }
    }

    struct Failing;

    #[async_trait]
    impl SearchProvider for Failing {
        async fn search(
            &self,
            _query: &str,
            _opts: &SearchOptions,
        ) -> QuarryResult<Vec<SearchResult>> {
            Err(QuarryError::new(ErrorCode::DriverFailed, "boom"))
        }
        fn name(&self) -> &str {
            "failing"
        }
    }

    struct Pending;

    #[async_trait]
    impl SearchProvider for Pending {
        async fn search(
            &self,
            _query: &str,
            _opts: &SearchOptions,
        ) -> QuarryResult<Vec<SearchResult>> {
            futures::future::pending::<QuarryResult<Vec<SearchResult>>>().await
        }
        fn name(&self) -> &str {
            "pending"
        }
    }

    #[test]
    fn builder_rejects_empty_chain() {
        match SmartSearchRouter::builder().build() {
            Ok(_) => panic!("expected build to reject empty chain"),
            Err(e) => assert_eq!(e.code, ErrorCode::BadRequest),
        }
    }

    #[tokio::test]
    async fn empty_query_rejected() {
        let router = SmartSearchRouter::builder()
            .with_tantivy(Arc::new(Canned(vec![r("https://a/", "tantivy_local")])))
            .build()
            .unwrap();
        let err = router
            .search("   ", &SearchOptions::default())
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
    }

    #[tokio::test]
    async fn default_path_uses_tantivy_when_enough_results() {
        let local = vec![
            r("https://a/", "tantivy_local"),
            r("https://b/", "tantivy_local"),
            r("https://c/", "tantivy_local"),
        ];
        let router = SmartSearchRouter::builder()
            .with_tantivy(Arc::new(Canned(local)))
            .with_stract(Arc::new(Canned(vec![r("https://web/", "stract")])))
            .build()
            .unwrap();
        let results = router
            .search("rust", &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 3);
        // No widening — none of the URLs come from stract.
        assert!(results.iter().all(|r| r.provider == "tantivy_local"));
    }

    #[tokio::test]
    async fn default_path_widens_when_local_results_thin() {
        let cfg = RouterConfig {
            min_local_results: 3,
            cache_ttl_s: 0, // disable cache for this test
            ..Default::default()
        };
        let local = vec![r("https://a/", "tantivy_local")];
        let web = vec![r("https://b/", "stract"), r("https://c/", "stract")];
        let router = SmartSearchRouter::builder()
            .with_tantivy(Arc::new(Canned(local)))
            .with_stract(Arc::new(Canned(web)))
            .with_config(cfg)
            .build()
            .unwrap();
        let results = router
            .search("rust", &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 3);
        assert!(results.iter().any(|r| r.url == "https://a/"));
        assert!(results.iter().any(|r| r.url == "https://b/"));
    }

    #[tokio::test]
    async fn default_path_returns_as_soon_as_parallel_remote_target_is_met() {
        let router = SmartSearchRouter::builder()
            .with_stract(Arc::new(Canned(vec![r("https://fast/", "stract")])))
            .with_searxng(Arc::new(Pending))
            .with_config(RouterConfig {
                min_local_results: 1,
                min_total_results: 1,
                cache_ttl_s: 0,
                provider_timeout_s: 30,
                ..Default::default()
            })
            .build()
            .unwrap();
        let opts = SearchOptions {
            limit: 1,
            ..Default::default()
        };
        let results =
            tokio::time::timeout(Duration::from_millis(100), router.search("OpenAI", &opts))
                .await
                .expect("search should not wait for the slow sibling provider")
                .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].provider, "stract");
    }

    #[tokio::test]
    async fn short_head_queries_can_return_from_brave_without_waiting_for_free_timeout() {
        let router = SmartSearchRouter::builder()
            .with_searxng(Arc::new(Pending))
            .with_brave(Arc::new(Canned(vec![r("https://paid/", "brave")])))
            .with_config(RouterConfig {
                min_local_results: 1,
                min_total_results: 1,
                cache_ttl_s: 0,
                provider_timeout_s: 30,
                ..Default::default()
            })
            .build()
            .unwrap();
        let opts = SearchOptions {
            limit: 1,
            // Paid fan-out is opt-in per request; without this the eager
            // path is gated off and the test would be proving the gate.
            allow_paid_providers: true,
            ..Default::default()
        };
        let results =
            tokio::time::timeout(Duration::from_millis(100), router.search("OpenAI", &opts))
                .await
                .expect(
                    "head query should not serialize a paid fallback behind a slow free provider",
                )
                .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].provider, "brave");
    }

    #[tokio::test]
    async fn fresh_path_skips_tantivy() {
        // If Tantivy is configured but the query is "fresh", we should
        // NOT hit it. Use a Tantivy that would panic if called to prove.
        struct PanickingLocal;
        #[async_trait]
        impl SearchProvider for PanickingLocal {
            async fn search(
                &self,
                _query: &str,
                _opts: &SearchOptions,
            ) -> QuarryResult<Vec<SearchResult>> {
                panic!("Tantivy should be skipped on fresh queries");
            }
            fn name(&self) -> &str {
                "panicking"
            }
        }
        let router = SmartSearchRouter::builder()
            .with_tantivy(Arc::new(PanickingLocal))
            .with_stract(Arc::new(Canned(vec![r("https://news/", "stract")])))
            .with_config(RouterConfig {
                cache_ttl_s: 0,
                ..Default::default()
            })
            .build()
            .unwrap();
        let results = router
            .search("breaking news today", &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].provider, "stract");
    }

    #[tokio::test]
    async fn cache_returns_same_results_within_ttl() {
        let local = vec![r("https://a/", "tantivy_local")];
        let router = SmartSearchRouter::builder()
            .with_tantivy(Arc::new(Canned(local)))
            .with_config(RouterConfig {
                cache_ttl_s: 60,
                ..Default::default()
            })
            .build()
            .unwrap();
        let r1 = router
            .search("rust", &SearchOptions::default())
            .await
            .unwrap();
        let r2 = router
            .search("rust", &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(r1.len(), r2.len());
        assert_eq!(r1[0].url, r2[0].url);
    }

    #[tokio::test]
    async fn brave_invoked_only_when_free_chain_empty() {
        // Tantivy empty, Stract failing → should fall through to Brave.
        let router = SmartSearchRouter::builder()
            .with_tantivy(Arc::new(Canned(vec![])))
            .with_stract(Arc::new(Failing))
            .with_brave(Arc::new(Canned(vec![r("https://paid/", "brave")])))
            .with_config(RouterConfig {
                cache_ttl_s: 0,
                ..Default::default()
            })
            .build()
            .unwrap();
        let opts = SearchOptions {
            allow_paid_providers: true,
            ..Default::default()
        };
        let results = router.search("obscure query", &opts).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].provider, "brave");
    }

    #[tokio::test]
    async fn brave_not_invoked_when_free_chain_succeeds() {
        // Failing Brave that would error if called — proves it isn't called
        // when Stract returns enough results.
        let router = SmartSearchRouter::builder()
            .with_tantivy(Arc::new(Canned(vec![])))
            .with_stract(Arc::new(Canned(vec![r("https://free/", "stract")])))
            .with_brave(Arc::new(Failing))
            .with_config(RouterConfig {
                cache_ttl_s: 0,
                ..Default::default()
            })
            .build()
            .unwrap();
        // Permission granted, so what keeps Brave out of this run is the
        // "free chain was enough" rule and nothing else.
        let opts = SearchOptions {
            allow_paid_providers: true,
            ..Default::default()
        };
        let results = router.search("rust async", &opts).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].provider, "stract");
    }

    // ---- ZDR: Brave/Serper must never be invoked --------------------------
    //
    // Each test below configures a provider that panics if called — a
    // silent regression that let a ZDR query reach Brave/Serper would fail
    // loudly here instead of shipping unnoticed.

    struct PanickingBrave;
    #[async_trait]
    impl SearchProvider for PanickingBrave {
        async fn search(
            &self,
            _query: &str,
            _opts: &SearchOptions,
        ) -> QuarryResult<Vec<SearchResult>> {
            panic!("ZDR request must never reach Brave");
        }
        fn name(&self) -> &str {
            "brave"
        }
    }

    struct PanickingSerper;
    #[async_trait]
    impl SearchProvider for PanickingSerper {
        async fn search(
            &self,
            _query: &str,
            _opts: &SearchOptions,
        ) -> QuarryResult<Vec<SearchResult>> {
            panic!("ZDR request must never reach Serper");
        }
        fn name(&self) -> &str {
            "serper"
        }
    }

    #[tokio::test]
    async fn zdr_skips_eager_paid_backup_for_short_queries() {
        // Same shape as `short_head_queries_can_return_from_brave_without_
        // waiting_for_free_timeout`, but zdr=true. A short bare-entity query
        // with an empty local corpus would normally race Brave in
        // immediately; ZDR must suppress that regardless of query shape.
        let router = SmartSearchRouter::builder()
            .with_searxng(Arc::new(Canned(vec![r("https://free/", "searxng")])))
            .with_brave(Arc::new(PanickingBrave))
            .with_config(RouterConfig {
                min_local_results: 1,
                min_total_results: 1,
                cache_ttl_s: 0,
                ..Default::default()
            })
            .build()
            .unwrap();
        let opts = SearchOptions {
            limit: 1,
            zdr: true,
            // Permission deliberately granted: ZDR is an independent
            // conjunct, so the request must still be refused paid providers.
            allow_paid_providers: true,
            ..Default::default()
        };
        let results = router.search("OpenAI", &opts).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].provider, "searxng");
    }

    #[tokio::test]
    async fn zdr_never_falls_back_to_brave_or_serper_in_default_path() {
        // No free tier configured at all, so a non-ZDR run would fall
        // through to Brave then Serper. ZDR must return empty instead.
        let router = SmartSearchRouter::builder()
            .with_brave(Arc::new(PanickingBrave))
            .with_serper(Arc::new(PanickingSerper))
            .with_config(RouterConfig {
                cache_ttl_s: 0,
                ..Default::default()
            })
            .build()
            .unwrap();
        let opts = SearchOptions {
            zdr: true,
            // Permission deliberately granted: ZDR is an independent
            // conjunct, so the request must still be refused paid providers.
            allow_paid_providers: true,
            ..Default::default()
        };
        let results = router
            .search("an obscure research question", &opts)
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn zdr_never_falls_back_to_brave_in_fresh_path() {
        // "breaking news today" classifies as Fresh (skips Tantivy, goes
        // straight to the free live-SERP fan-out then, normally, Brave on
        // empty). No free provider is configured, so ZDR is the only thing
        // standing between this query and Brave.
        let router = SmartSearchRouter::builder()
            .with_brave(Arc::new(PanickingBrave))
            .with_config(RouterConfig {
                cache_ttl_s: 0,
                ..Default::default()
            })
            .build()
            .unwrap();
        let opts = SearchOptions {
            zdr: true,
            // Permission deliberately granted: ZDR is an independent
            // conjunct, so the request must still be refused paid providers.
            allow_paid_providers: true,
            ..Default::default()
        };
        let results = router.search("breaking news today", &opts).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn zdr_never_falls_back_to_brave_or_serper_in_widen_path() {
        let router = SmartSearchRouter::builder()
            .with_brave(Arc::new(PanickingBrave))
            .with_serper(Arc::new(PanickingSerper))
            .with_intent_classifier(Arc::new(FixedIntent(QueryIntent::Research)))
            .with_config(RouterConfig {
                cache_ttl_s: 0,
                ..Default::default()
            })
            .build()
            .unwrap();
        let opts = SearchOptions {
            zdr: true,
            // Permission deliberately granted: ZDR is an independent
            // conjunct, so the request must still be refused paid providers.
            allow_paid_providers: true,
            ..Default::default()
        };
        let results = router
            .search("explain consensus protocols", &opts)
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    // ---- Intent classifier integration -----------------------------------

    /// Async classifier stub that returns a preset intent without touching
    /// the network. Lets us drive the router through Research/Comparative
    /// paths deterministically.
    struct FixedIntent(QueryIntent);

    #[async_trait]
    impl crate::intent_classifier::IntentClassifier for FixedIntent {
        async fn classify(&self, _query: &str) -> QueryIntent {
            self.0
        }
    }

    #[tokio::test]
    async fn research_intent_forces_widen_even_when_local_is_enough() {
        // Tantivy returns 5 results (way above min_local_results=3). The
        // default path would stop there. Research intent must still fan
        // out to Stract.
        let local = vec![
            r("https://a/", "tantivy_local"),
            r("https://b/", "tantivy_local"),
            r("https://c/", "tantivy_local"),
            r("https://d/", "tantivy_local"),
            r("https://e/", "tantivy_local"),
        ];
        let web = vec![r("https://web/", "stract")];
        let router = SmartSearchRouter::builder()
            .with_tantivy(Arc::new(Canned(local)))
            .with_stract(Arc::new(Canned(web)))
            .with_intent_classifier(Arc::new(FixedIntent(QueryIntent::Research)))
            .with_config(RouterConfig {
                cache_ttl_s: 0,
                ..Default::default()
            })
            .build()
            .unwrap();
        let results = router
            .search("explain consensus protocols", &SearchOptions::default())
            .await
            .unwrap();
        // Must include the stract URL → proving widen_path ran.
        assert!(results.iter().any(|r| r.url == "https://web/"));
    }

    #[tokio::test]
    async fn comparative_intent_also_forces_widen() {
        let local = vec![r("https://a/", "tantivy_local")];
        let web = vec![r("https://b/", "stract")];
        let router = SmartSearchRouter::builder()
            .with_tantivy(Arc::new(Canned(local)))
            .with_stract(Arc::new(Canned(web)))
            .with_intent_classifier(Arc::new(FixedIntent(QueryIntent::Comparative)))
            .with_config(RouterConfig {
                cache_ttl_s: 0,
                ..Default::default()
            })
            .build()
            .unwrap();
        let results = router
            .search("rust vs go", &SearchOptions::default())
            .await
            .unwrap();
        assert!(results.iter().any(|r| r.url == "https://b/"));
    }

    #[tokio::test]
    async fn code_intent_routes_through_default_path() {
        // Code uses default_path topology — Tantivy first, widen only if
        // thin. Five tantivy results means no widen, even with stract.
        let local = vec![
            r("https://a/", "tantivy_local"),
            r("https://b/", "tantivy_local"),
            r("https://c/", "tantivy_local"),
        ];
        let router = SmartSearchRouter::builder()
            .with_tantivy(Arc::new(Canned(local)))
            .with_stract(Arc::new(Canned(vec![r("https://web/", "stract")])))
            .with_intent_classifier(Arc::new(FixedIntent(QueryIntent::Code)))
            .with_config(RouterConfig {
                cache_ttl_s: 0,
                ..Default::default()
            })
            .build()
            .unwrap();
        let results = router
            .search("tokio runtime internals", &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 3);
        assert!(results.iter().all(|r| r.provider == "tantivy_local"));
    }

    #[tokio::test]
    async fn classifier_absent_uses_rule_fast_path() {
        // No classifier configured — query "breaking news" should still
        // be Fresh-classified by the rule fast-path and skip Tantivy.
        struct ShouldNotCall;
        #[async_trait]
        impl SearchProvider for ShouldNotCall {
            async fn search(
                &self,
                _query: &str,
                _opts: &SearchOptions,
            ) -> QuarryResult<Vec<SearchResult>> {
                panic!("rule fast-path should still classify this as Fresh");
            }
            fn name(&self) -> &str {
                "should_not_call"
            }
        }
        let router = SmartSearchRouter::builder()
            .with_tantivy(Arc::new(ShouldNotCall))
            .with_stract(Arc::new(Canned(vec![r("https://news/", "stract")])))
            .with_config(RouterConfig {
                cache_ttl_s: 0,
                ..Default::default()
            })
            .build()
            .unwrap();
        let results = router
            .search("breaking news today", &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results[0].provider, "stract");
    }

    #[tokio::test]
    async fn ranks_renumbered_after_merge() {
        let cfg = RouterConfig {
            min_local_results: 5,
            cache_ttl_s: 0,
            ..Default::default()
        };
        let local = vec![r("https://a/", "tantivy_local")];
        let web = vec![r("https://b/", "stract")];
        let router = SmartSearchRouter::builder()
            .with_tantivy(Arc::new(Canned(local)))
            .with_stract(Arc::new(Canned(web)))
            .with_config(cfg)
            .build()
            .unwrap();
        let results = router
            .search("rust", &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results[0].rank, 1);
        assert_eq!(results[1].rank, 2);
    }

    // ---- Rank fusion ------------------------------------------------------

    /// Result with an explicit provider-supplied rank, which is what the
    /// fusion fallback scores off.
    fn r_ranked(url: &str, provider: &str, rank: u32) -> SearchResult {
        SearchResult {
            url: url.into(),
            title: None,
            snippet: None,
            rank,
            provider: provider.into(),
            ..Default::default()
        }
    }

    #[test]
    fn three_providers_agreeing_outrank_a_single_engine_rank_one() {
        // The whole point of fusing instead of concatenating: corroboration
        // beats one engine's confident top hit.
        let cfg = RouterConfig::default();
        let mut pool = merge_dedupe(
            &cfg,
            Vec::new(),
            vec![r_ranked("https://solo.example/a", "tantivy_local", 1)],
        );
        for provider in ["stract", "searxng", "brave"] {
            pool = merge_dedupe(
                &cfg,
                pool,
                vec![r_ranked("https://agreed.example/b", provider, 5)],
            );
        }
        let out = rank_and_truncate(pool, 10);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].url, "https://agreed.example/b");
        assert_eq!(out[0].rank, 1);
        assert_eq!(out[1].url, "https://solo.example/a");
        // 3 × 1/(k+5) really is larger than 1 × 1/(k+1).
        assert!(out[0].fusion_score.unwrap() > out[1].fusion_score.unwrap());
    }

    #[test]
    fn merge_dedupe_unions_engine_provenance_across_providers() {
        let cfg = RouterConfig::default();
        let mut kept = r("https://example.com/doc", "searxng");
        kept.engines = vec!["google".into(), "brave".into()];
        let mut dupe = r("http://www.example.com/doc/?utm_source=x", "stract");
        dupe.engines = vec!["brave".into(), "marginalia".into()];

        let out = merge_dedupe(&cfg, vec![kept], vec![dupe]);
        assert_eq!(out.len(), 1);
        // Union, not concatenation — "brave" appears once.
        assert_eq!(out[0].engines, vec!["google", "brave", "marginalia"]);
        assert_eq!(out[0].url, "https://example.com/doc");
    }

    #[test]
    fn canonical_duplicates_merge_their_evidence_instead_of_being_dropped() {
        let cfg = RouterConfig::default();
        let base = vec![r_ranked("https://example.com/artikkel", "tantivy_local", 1)];
        let extra = vec![r_ranked(
            "http://www.example.com/artikkel/?utm_source=x",
            "stract",
            1,
        )];
        let out = merge_dedupe(&cfg, base, extra);
        assert_eq!(out.len(), 1);
        // Survivor keeps its untouched URL and higher-priority provenance…
        assert_eq!(out[0].url, "https://example.com/artikkel");
        assert_eq!(out[0].provider, "tantivy_local");
        // …but now carries both contributions, which is the difference from
        // the old "drop the duplicate" behavior.
        let expected = 2.0 / (RRF_K + 1.0);
        assert!((out[0].fusion_score.unwrap() - expected).abs() < 1e-6);
    }

    #[test]
    fn merge_dedupe_fills_only_an_empty_snippet() {
        let cfg = RouterConfig::default();

        let gap = r("https://example.com/doc", "tantivy_local");
        let mut donor = r("https://example.com/doc", "stract");
        donor.snippet = Some("from stract".into());
        let out = merge_dedupe(&cfg, vec![gap], vec![donor.clone()]);
        assert_eq!(out[0].snippet.as_deref(), Some("from stract"));

        let mut described = r("https://example.com/doc", "tantivy_local");
        described.snippet = Some("from tantivy".into());
        let out = merge_dedupe(&cfg, vec![described], vec![donor]);
        assert_eq!(out[0].snippet.as_deref(), Some("from tantivy"));
    }

    /// One fixed three-provider fan-out with overlaps, fused and ranked.
    fn fuse_sample(cfg: &RouterConfig) -> Vec<String> {
        let mut pool = merge_dedupe(
            cfg,
            Vec::new(),
            vec![
                r_ranked("https://a.example/1", "tantivy_local", 1),
                r_ranked("https://b.example/2", "tantivy_local", 2),
            ],
        );
        pool = merge_dedupe(
            cfg,
            pool,
            vec![
                r_ranked("https://b.example/2", "stract", 1),
                r_ranked("https://c.example/3", "stract", 2),
            ],
        );
        pool = merge_dedupe(
            cfg,
            pool,
            vec![
                r_ranked("https://c.example/3", "searxng", 1),
                r_ranked("https://d.example/4", "searxng", 3),
            ],
        );
        rank_and_truncate(pool, 10)
            .into_iter()
            .map(|r| r.url)
            .collect()
    }

    #[test]
    fn fusion_output_is_identical_across_a_hundred_runs() {
        // The lookup is a HashMap, whose iteration order varies per process
        // AND per insertion history. This fails loudly if output order ever
        // starts depending on it instead of on insertion order.
        let cfg = RouterConfig::default();
        let expected = fuse_sample(&cfg);
        for run in 0..100 {
            assert_eq!(fuse_sample(&cfg), expected, "run {run} diverged");
        }
    }

    #[test]
    fn fused_order_promotes_corroborated_urls_and_breaks_ties_by_insertion() {
        // b and c are each backed by two providers and score identically, so
        // the tie falls back to insertion order — which is provider priority
        // order. a and d were seen once each.
        let out = fuse_sample(&RouterConfig::default());
        assert_eq!(
            out,
            vec![
                "https://b.example/2",
                "https://c.example/3",
                "https://a.example/1",
                "https://d.example/4",
            ]
        );
    }

    #[test]
    fn equal_scores_keep_insertion_order_so_provider_priority_survives() {
        let cfg = RouterConfig::default();
        let base = vec![r_ranked("https://local.example/x", "tantivy_local", 1)];
        let extra = vec![r_ranked("https://web.example/y", "stract", 1)];
        let out = rank_and_truncate(merge_dedupe(&cfg, base, extra), 10);
        assert_eq!(out[0].url, "https://local.example/x");
        assert_eq!(out[1].url, "https://web.example/y");
    }

    #[test]
    fn provider_weight_defaults_to_one_and_scales_contributions() {
        let mut cfg = RouterConfig::default();
        assert_eq!(provider_weight(&cfg, "stract"), 1.0);
        assert_eq!(provider_weight(&cfg, "never-configured"), 1.0);

        // Unweighted, the rank-1 local hit wins over a rank-9 web hit.
        let unweighted = rank_and_truncate(
            merge_dedupe(
                &cfg,
                Vec::new(),
                vec![
                    r_ranked("https://local.example/x", "tantivy_local", 1),
                    r_ranked("https://web.example/y", "stract", 9),
                ],
            ),
            10,
        );
        assert_eq!(unweighted[0].url, "https://local.example/x");

        cfg.provider_weights.insert("stract".into(), 4.0);
        let weighted = rank_and_truncate(
            merge_dedupe(
                &cfg,
                Vec::new(),
                vec![
                    r_ranked("https://local.example/x", "tantivy_local", 1),
                    r_ranked("https://web.example/y", "stract", 9),
                ],
            ),
            10,
        );
        assert_eq!(weighted[0].url, "https://web.example/y");
    }

    #[test]
    fn provider_supplied_fusion_score_is_used_instead_of_the_rank_fallback() {
        // SearXNG already fused across its own upstream engines; that sum is
        // stronger evidence than its position in the response.
        let cfg = RouterConfig::default();
        let mut metasearch = r_ranked("https://meta.example/x", "searxng", 40);
        metasearch.fusion_score = Some(0.5);
        let out = rank_and_truncate(
            merge_dedupe(
                &cfg,
                Vec::new(),
                vec![r_ranked("https://solo.example/y", "stract", 1), metasearch],
            ),
            10,
        );
        assert_eq!(out[0].url, "https://meta.example/x");
    }

    #[test]
    fn rank_and_truncate_cuts_to_the_limit_and_renumbers() {
        let cfg = RouterConfig::default();
        let pool = merge_dedupe(
            &cfg,
            Vec::new(),
            vec![
                r_ranked("https://a.example/1", "stract", 1),
                r_ranked("https://b.example/2", "stract", 2),
                r_ranked("https://c.example/3", "stract", 3),
            ],
        );
        let out = rank_and_truncate(pool, 2);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].rank, 1);
        assert_eq!(out[1].rank, 2);
        // limit 0 would otherwise erase the whole response.
        assert_eq!(rank_and_truncate(out, 0).len(), 1);
    }

    // ---- Intent hint ------------------------------------------------------

    #[test]
    fn cache_key_differs_on_intent_hint() {
        // The collision this guards against: same query, same filters, two
        // different routings. Without intent in the key the second caller is
        // served the first caller's topology.
        let none = SearchOptions::default();
        let fresh = SearchOptions {
            intent: Some(QueryIntent::Fresh),
            ..Default::default()
        };
        let research = SearchOptions {
            intent: Some(QueryIntent::Research),
            ..Default::default()
        };
        let k_none = SmartSearchRouter::cache_key("laksepriser", &none);
        let k_fresh = SmartSearchRouter::cache_key("laksepriser", &fresh);
        let k_research = SmartSearchRouter::cache_key("laksepriser", &research);
        assert_ne!(k_none, k_fresh);
        assert_ne!(k_none, k_research);
        assert_ne!(k_fresh, k_research);
        // Still stable for identical inputs.
        assert_eq!(k_fresh, SmartSearchRouter::cache_key("laksepriser", &fresh));
    }

    #[test]
    fn intent_cache_tags_are_distinct() {
        let all = [
            None,
            Some(QueryIntent::Navigational),
            Some(QueryIntent::Fresh),
            Some(QueryIntent::Phrase),
            Some(QueryIntent::Research),
            Some(QueryIntent::Comparative),
            Some(QueryIntent::Local),
            Some(QueryIntent::Code),
            Some(QueryIntent::Default),
        ];
        let mut tags: Vec<u8> = all.iter().map(|i| intent_cache_tag(*i)).collect();
        tags.sort_unstable();
        tags.dedup();
        assert_eq!(tags.len(), all.len());
    }

    #[tokio::test]
    async fn two_intents_on_one_query_do_not_share_a_cache_entry() {
        // Cache ON. `Fresh` skips Tantivy and answers from the live SERP;
        // `Navigational` takes the default path and answers from Tantivy. If
        // intent were missing from the cache key the second search would be
        // handed the first one's results.
        let router = SmartSearchRouter::builder()
            .with_tantivy(Arc::new(Canned(vec![r("https://local/", "tantivy_local")])))
            .with_stract(Arc::new(Canned(vec![r("https://live/", "stract")])))
            .with_config(RouterConfig {
                min_local_results: 1,
                cache_ttl_s: 60,
                ..Default::default()
            })
            .build()
            .unwrap();

        let fresh = SearchOptions {
            intent: Some(QueryIntent::Fresh),
            ..Default::default()
        };
        let nav = SearchOptions {
            intent: Some(QueryIntent::Navigational),
            ..Default::default()
        };

        let first = router.search("kvartalstall", &fresh).await.unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].provider, "stract");

        let second = router.search("kvartalstall", &nav).await.unwrap();
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].provider, "tantivy_local");

        // …and each hint still caches against itself.
        let repeat = router.search("kvartalstall", &fresh).await.unwrap();
        assert_eq!(repeat[0].provider, "stract");
    }

    #[tokio::test]
    async fn caller_intent_hint_overrides_the_rule_classifier() {
        // "breaking news today" is Fresh by the keyword rule, which skips
        // Tantivy entirely. A `Code` hint must win and take the default path.
        let router = SmartSearchRouter::builder()
            .with_tantivy(Arc::new(Canned(vec![r("https://local/", "tantivy_local")])))
            .with_stract(Arc::new(Canned(vec![r("https://live/", "stract")])))
            .with_config(RouterConfig {
                min_local_results: 1,
                cache_ttl_s: 0,
                ..Default::default()
            })
            .build()
            .unwrap();
        let opts = SearchOptions {
            intent: Some(QueryIntent::Code),
            ..Default::default()
        };
        let results = router.search("breaking news today", &opts).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].provider, "tantivy_local");
    }

    #[tokio::test]
    async fn caller_intent_hint_overrides_a_wired_async_classifier() {
        // The classifier insists on Research, which fans out to every free
        // engine. The hint says Default, and a satisfied local corpus must
        // then end the request without touching Stract.
        let router = SmartSearchRouter::builder()
            .with_tantivy(Arc::new(Canned(vec![r("https://local/", "tantivy_local")])))
            .with_stract(Arc::new(Canned(vec![r("https://live/", "stract")])))
            .with_intent_classifier(Arc::new(FixedIntent(QueryIntent::Research)))
            .with_config(RouterConfig {
                min_local_results: 1,
                cache_ttl_s: 0,
                ..Default::default()
            })
            .build()
            .unwrap();
        let opts = SearchOptions {
            intent: Some(QueryIntent::Default),
            ..Default::default()
        };
        let results = router
            .search("explain consensus protocols", &opts)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].provider, "tantivy_local");
    }

    #[tokio::test]
    async fn absent_intent_hint_leaves_classification_to_the_router() {
        // No hint → the rule fast-path still calls this Fresh and skips the
        // local corpus, exactly as before the hint existed.
        let router = SmartSearchRouter::builder()
            .with_tantivy(Arc::new(Canned(vec![r("https://local/", "tantivy_local")])))
            .with_stract(Arc::new(Canned(vec![r("https://live/", "stract")])))
            .with_config(RouterConfig {
                min_local_results: 1,
                cache_ttl_s: 0,
                ..Default::default()
            })
            .build()
            .unwrap();
        let results = router
            .search("breaking news today", &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results[0].provider, "stract");
    }

    // ---- Paid-provider permission gate ------------------------------------
    //
    // `allow_paid_providers` defaults to false, so each of these reuses the
    // panicking Brave/Serper stubs above: reaching a paid provider without
    // permission fails the test loudly rather than quietly costing money.

    #[tokio::test]
    async fn no_permission_blocks_eager_paid_backup_for_short_queries() {
        let router = SmartSearchRouter::builder()
            .with_searxng(Arc::new(Canned(vec![r("https://free/", "searxng")])))
            .with_brave(Arc::new(PanickingBrave))
            .with_config(RouterConfig {
                min_local_results: 1,
                min_total_results: 1,
                cache_ttl_s: 0,
                ..Default::default()
            })
            .build()
            .unwrap();
        let opts = SearchOptions {
            limit: 1,
            ..Default::default()
        };
        let results = router.search("OpenAI", &opts).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].provider, "searxng");
    }

    #[tokio::test]
    async fn no_permission_blocks_paid_fallback_in_default_path() {
        let router = SmartSearchRouter::builder()
            .with_brave(Arc::new(PanickingBrave))
            .with_serper(Arc::new(PanickingSerper))
            .with_config(RouterConfig {
                cache_ttl_s: 0,
                ..Default::default()
            })
            .build()
            .unwrap();
        let results = router
            .search("an obscure research question", &SearchOptions::default())
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn no_permission_blocks_paid_fallback_in_fresh_path() {
        let router = SmartSearchRouter::builder()
            .with_brave(Arc::new(PanickingBrave))
            .with_config(RouterConfig {
                cache_ttl_s: 0,
                ..Default::default()
            })
            .build()
            .unwrap();
        let results = router
            .search("breaking news today", &SearchOptions::default())
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn no_permission_blocks_paid_fallback_in_widen_path() {
        let router = SmartSearchRouter::builder()
            .with_brave(Arc::new(PanickingBrave))
            .with_serper(Arc::new(PanickingSerper))
            .with_intent_classifier(Arc::new(FixedIntent(QueryIntent::Research)))
            .with_config(RouterConfig {
                cache_ttl_s: 0,
                ..Default::default()
            })
            .build()
            .unwrap();
        let results = router
            .search("explain consensus protocols", &SearchOptions::default())
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn paid_providers_need_permission_and_absence_of_zdr() {
        // The full two-term truth table on one shape. Only the bottom row
        // may dispatch; the ZDR rows prove permission cannot buy past ZDR.
        for (allow_paid, zdr, expect_dispatch) in [
            (false, false, false),
            (false, true, false),
            (true, true, false),
            (true, false, true),
        ] {
            let router = SmartSearchRouter::builder()
                .with_brave(Arc::new(Canned(vec![r("https://paid/", "brave")])))
                .with_config(RouterConfig {
                    cache_ttl_s: 0,
                    ..Default::default()
                })
                .build()
                .unwrap();
            let opts = SearchOptions {
                allow_paid_providers: allow_paid,
                zdr,
                ..Default::default()
            };
            let results = router
                .search("an obscure research question", &opts)
                .await
                .unwrap();
            assert_eq!(
                !results.is_empty(),
                expect_dispatch,
                "allow_paid_providers={allow_paid} zdr={zdr}"
            );
        }
    }
}
