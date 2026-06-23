//! Pluggable query-intent classifiers.
//!
//! `SmartSearchRouter` consults an `IntentClassifier` to pick a routing
//! strategy per query. The classifier is intentionally async + trait-based
//! so we can layer:
//!
//! 1. [`RuleClassifier`] — pure heuristics (URL shape, quoted phrase,
//!    fresh keywords, year tokens). Zero latency, deterministic, runs
//!    offline. This is the production default.
//! 2. [`MpIntentClassifier`] — Model Plane–backed LLM classifier. Surfaces
//!    semantic intents the rules can't see (Research, Comparative, Local,
//!    Code). Hard timeout — falls back to `Default` on any error so the
//!    Model Plane is **never** in the search critical path.
//! 3. [`HybridClassifier`] — rules first, LLM only when rules return
//!    `Default`. This is the recommended way to enable LLM classification
//!    without paying its latency cost on obvious queries.
//! 4. [`CachedClassifier`] — decorator. blake3-keyed in-memory cache with
//!    a TTL. Intent for a given query string is stable for long stretches,
//!    so the cache hit rate is high in practice.
//!
//! Wiring strategy in `main.rs`:
//!
//! ```text
//! CachedClassifier(60min)
//!   └─ HybridClassifier
//!        ├─ rule fast-path (instant)
//!        └─ MpIntentClassifier(150ms timeout) ← only on Default bucket
//! ```
//!
//! The cache layer is outermost so even the rule fast-path benefits from
//! it (skipping the rule scan on hot keys), and a single `classify()`
//! call resolves with one lock acquisition on a cache hit.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use crate::mp_client::{ModelPlaneClient, ModelPlaneInvokeRequest};
use crate::smart_router::{classify_intent, QueryIntent};

/// Async trait so the LLM-backed impl can perform network I/O without
/// blocking. Pure-rule impls just return immediately.
#[async_trait]
pub trait IntentClassifier: Send + Sync {
    async fn classify(&self, query: &str) -> QueryIntent;
}

// =============================================================================
// Rule classifier — wraps the existing pure function.
// =============================================================================

/// Adapts the rule-only `classify_intent` free function to the trait so
/// it can stand in anywhere an `IntentClassifier` is expected. Cheap to
/// construct, no I/O.
#[derive(Debug, Default, Clone, Copy)]
pub struct RuleClassifier;

#[async_trait]
impl IntentClassifier for RuleClassifier {
    async fn classify(&self, query: &str) -> QueryIntent {
        classify_intent(query)
    }
}

// =============================================================================
// Model Plane classifier — single-token classification via LLM.
// =============================================================================

/// Default per-call timeout. Tight by design: the classifier is on the
/// critical path of every search that reaches the LLM bucket, so a slow
/// model degrades the entire endpoint.
const DEFAULT_TIMEOUT_MS: u64 = 150;

const CLASSIFIER_PROMPT: &str = "You are an intent classifier for a search engine. Read the user's query and respond with EXACTLY ONE TOKEN from this set — no prose, no punctuation, no explanation:\n\nNAV       — query is a URL, domain, or asks for a specific website.\nFRESH     — query asks about today, latest, breaking, current events, prices, releases.\nPHRASE    — query is a literal phrase the user wants matched verbatim.\nRESEARCH  — query is an open-ended research question that benefits from many sources.\nCOMPARE   — query compares two or more entities, products, options, or technologies.\nLOCAL     — query has a geographic component (city, country, \"near me\", restaurants in X).\nCODE      — query is about source code, programming, libraries, APIs, or developer docs.\nDEFAULT   — none of the above.\n\nQuery: {{QUERY}}\nIntent:";

/// LLM-backed classifier. Sends a tiny prompt to Model Plane and parses
/// the first token of the response. Any failure (network, timeout,
/// malformed output) returns `QueryIntent::Default` so the router can
/// always make a decision.
pub struct MpIntentClassifier {
    client: Arc<ModelPlaneClient>,
    /// Optional model override. None means MP picks its default.
    model: Option<String>,
    timeout: Duration,
}

impl MpIntentClassifier {
    pub fn new(client: Arc<ModelPlaneClient>) -> Self {
        Self {
            client,
            model: None,
            timeout: Duration::from_millis(DEFAULT_TIMEOUT_MS),
        }
    }

    /// Pin a specific model. Prefer a small, fast model like Haiku —
    /// intent classification needs latency, not depth.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Parse the model's reply into an intent. Be defensive: take only
    /// the first whitespace-separated token, uppercase it, strip non-alpha
    /// trailing characters. Unknown tokens collapse to `Default`.
    fn parse(reply: &str) -> QueryIntent {
        let first = reply
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_matches(|c: char| !c.is_alphanumeric())
            .to_uppercase();
        match first.as_str() {
            "NAV" | "NAVIGATIONAL" => QueryIntent::Navigational,
            "FRESH" => QueryIntent::Fresh,
            "PHRASE" => QueryIntent::Phrase,
            "RESEARCH" | "RES" => QueryIntent::Research,
            "COMPARE" | "COMPARATIVE" | "CMP" => QueryIntent::Comparative,
            "LOCAL" | "LOC" => QueryIntent::Local,
            "CODE" => QueryIntent::Code,
            _ => QueryIntent::Default,
        }
    }
}

#[async_trait]
impl IntentClassifier for MpIntentClassifier {
    async fn classify(&self, query: &str) -> QueryIntent {
        // Strict input length cap. A pathological 100KB query would
        // serialize a huge prompt — bail and pass through.
        if query.len() > 4_096 {
            return QueryIntent::Default;
        }
        let prompt = CLASSIFIER_PROMPT.replace("{{QUERY}}", query);
        let req = ModelPlaneInvokeRequest {
            content: prompt,
            model: self.model.clone(),
            session_key: None,
            thread_id: None,
        };
        match tokio::time::timeout(self.timeout, self.client.invoke(&req)).await {
            Ok(Ok(resp)) => Self::parse(&resp.content),
            Ok(Err(e)) => {
                tracing::debug!(error = %e, "MP intent classify failed; defaulting");
                QueryIntent::Default
            }
            Err(_) => {
                tracing::debug!(
                    timeout_ms = self.timeout.as_millis() as u64,
                    "MP intent classify timed out; defaulting"
                );
                QueryIntent::Default
            }
        }
    }
}

// =============================================================================
// Hybrid classifier — rule fast-path, LLM fallback on Default.
// =============================================================================

/// Combines a cheap rule classifier with a more expensive LLM classifier.
/// The LLM is only consulted when rules return `Default` — i.e. the
/// query doesn't have an obvious URL/phrase/freshness signal. This keeps
/// classification effectively free for the bulk of queries (navigations,
/// year-stamped queries, quoted phrases) while still giving the router
/// semantic intent for everything else.
pub struct HybridClassifier {
    inner: Arc<dyn IntentClassifier>,
}

impl HybridClassifier {
    pub fn new(inner: Arc<dyn IntentClassifier>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl IntentClassifier for HybridClassifier {
    async fn classify(&self, query: &str) -> QueryIntent {
        let rule = classify_intent(query);
        if rule != QueryIntent::Default {
            return rule;
        }
        self.inner.classify(query).await
    }
}

// =============================================================================
// Cached classifier — decorator with TTL.
// =============================================================================

#[derive(Clone, Copy)]
struct CacheEntry {
    intent: QueryIntent,
    cached_at: Instant,
}

/// Wraps any `IntentClassifier` with an in-process TTL cache. Intent for
/// a given query string is essentially stable, so even a 60-minute TTL
/// gives a high hit rate while remaining responsive to vocabulary drift.
///
/// Eviction: when the cache hits `MAX_ENTRIES`, drop the first half. This
/// is cheaper than maintaining LRU bookkeeping at the volumes we expect
/// and keeps the cache bounded.
pub struct CachedClassifier {
    inner: Arc<dyn IntentClassifier>,
    ttl: Duration,
    cache: Arc<RwLock<HashMap<String, CacheEntry>>>,
}

impl CachedClassifier {
    pub fn new(inner: Arc<dyn IntentClassifier>, ttl: Duration) -> Self {
        Self {
            inner,
            ttl,
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn key(query: &str) -> String {
        // Normalize: trim + lowercase. Keeps "Rust" and "rust" in the
        // same bucket without disturbing how the router or providers
        // see the original query text.
        let normalized = query.trim().to_lowercase();
        blake3::hash(normalized.as_bytes()).to_hex().to_string()
    }
}

const MAX_ENTRIES: usize = 10_000;

#[async_trait]
impl IntentClassifier for CachedClassifier {
    async fn classify(&self, query: &str) -> QueryIntent {
        let key = Self::key(query);

        // Fast read path — most calls land here.
        {
            let map = self.cache.read().await;
            if let Some(entry) = map.get(&key) {
                if entry.cached_at.elapsed() < self.ttl {
                    return entry.intent;
                }
            }
        }

        // Cache miss — defer to inner classifier and remember the answer.
        let intent = self.inner.classify(query).await;

        let mut map = self.cache.write().await;
        if map.len() >= MAX_ENTRIES {
            // Cheap bulk eviction. Worst-case latency hit lives on a cold
            // write path, not in the hot read path.
            let to_drop: Vec<String> = map.keys().take(MAX_ENTRIES / 2).cloned().collect();
            for k in to_drop {
                map.remove(&k);
            }
        }
        map.insert(
            key,
            CacheEntry {
                intent,
                cached_at: Instant::now(),
            },
        );
        intent
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Stub classifier whose answer is settable and whose call count is
    /// observable — needed to exercise cache + hybrid behavior without
    /// touching the network.
    struct Stub {
        intent: QueryIntent,
        calls: AtomicUsize,
    }

    impl Stub {
        fn new(intent: QueryIntent) -> Self {
            Self {
                intent,
                calls: AtomicUsize::new(0),
            }
        }
        fn call_count(&self) -> usize {
            self.calls.load(Ordering::Relaxed)
        }
    }

    #[async_trait]
    impl IntentClassifier for Stub {
        async fn classify(&self, _query: &str) -> QueryIntent {
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.intent
        }
    }

    // ---- RuleClassifier ---------------------------------------------------

    #[tokio::test]
    async fn rule_classifier_matches_free_function() {
        let c = RuleClassifier;
        assert_eq!(c.classify("github.com").await, QueryIntent::Navigational);
        assert_eq!(c.classify("\"hello\"").await, QueryIntent::Phrase);
        assert_eq!(c.classify("breaking news").await, QueryIntent::Fresh);
        assert_eq!(
            c.classify("how does tokio work").await,
            QueryIntent::Default
        );
    }

    // ---- MpIntentClassifier.parse ----------------------------------------

    #[test]
    fn parses_clean_tokens() {
        assert_eq!(MpIntentClassifier::parse("NAV"), QueryIntent::Navigational);
        assert_eq!(MpIntentClassifier::parse("FRESH"), QueryIntent::Fresh);
        assert_eq!(MpIntentClassifier::parse("PHRASE"), QueryIntent::Phrase);
        assert_eq!(MpIntentClassifier::parse("RESEARCH"), QueryIntent::Research);
        assert_eq!(
            MpIntentClassifier::parse("COMPARE"),
            QueryIntent::Comparative
        );
        assert_eq!(MpIntentClassifier::parse("LOCAL"), QueryIntent::Local);
        assert_eq!(MpIntentClassifier::parse("CODE"), QueryIntent::Code);
        assert_eq!(MpIntentClassifier::parse("DEFAULT"), QueryIntent::Default);
    }

    #[test]
    fn parses_lowercase_and_aliases() {
        assert_eq!(MpIntentClassifier::parse("nav"), QueryIntent::Navigational);
        assert_eq!(MpIntentClassifier::parse("res"), QueryIntent::Research);
        assert_eq!(MpIntentClassifier::parse("cmp"), QueryIntent::Comparative);
        assert_eq!(MpIntentClassifier::parse("loc"), QueryIntent::Local);
    }

    #[test]
    fn parses_chatty_reply_takes_first_token() {
        // Some models prefix the answer with prose despite the prompt.
        assert_eq!(
            MpIntentClassifier::parse("FRESH — because it asks about today"),
            QueryIntent::Fresh
        );
        assert_eq!(
            MpIntentClassifier::parse("  RESEARCH\nReasoning: ..."),
            QueryIntent::Research
        );
    }

    #[test]
    fn parses_punctuation_around_token() {
        assert_eq!(MpIntentClassifier::parse("`CODE`."), QueryIntent::Code);
        assert_eq!(MpIntentClassifier::parse("[LOCAL]"), QueryIntent::Local);
    }

    #[test]
    fn parses_garbage_as_default() {
        assert_eq!(MpIntentClassifier::parse(""), QueryIntent::Default);
        assert_eq!(MpIntentClassifier::parse("???"), QueryIntent::Default);
        assert_eq!(MpIntentClassifier::parse("UNKNOWN"), QueryIntent::Default);
    }

    // ---- HybridClassifier -------------------------------------------------

    #[tokio::test]
    async fn hybrid_short_circuits_on_obvious_intents() {
        // Inner stub should NEVER be called for these queries — rules
        // already classify them.
        let inner = Arc::new(Stub::new(QueryIntent::Research));
        let hybrid = HybridClassifier::new(inner.clone());

        assert_eq!(
            hybrid.classify("github.com").await,
            QueryIntent::Navigational
        );
        assert_eq!(hybrid.classify("\"foo\"").await, QueryIntent::Phrase);
        assert_eq!(hybrid.classify("news today").await, QueryIntent::Fresh);
        assert_eq!(inner.call_count(), 0);
    }

    #[tokio::test]
    async fn hybrid_delegates_to_inner_on_default() {
        let inner = Arc::new(Stub::new(QueryIntent::Research));
        let hybrid = HybridClassifier::new(inner.clone());

        // "explain the consensus mechanisms of distributed databases"
        // doesn't trigger any rule — falls through to inner.
        let out = hybrid
            .classify("explain consensus mechanisms in distributed databases")
            .await;
        assert_eq!(out, QueryIntent::Research);
        assert_eq!(inner.call_count(), 1);
    }

    // ---- CachedClassifier -------------------------------------------------

    #[tokio::test]
    async fn cached_returns_same_intent_on_hot_key() {
        let inner = Arc::new(Stub::new(QueryIntent::Research));
        let cached = CachedClassifier::new(inner.clone(), Duration::from_secs(60));

        assert_eq!(
            cached.classify("rust borrow checker").await,
            QueryIntent::Research
        );
        assert_eq!(
            cached.classify("rust borrow checker").await,
            QueryIntent::Research
        );
        assert_eq!(
            cached.classify("rust borrow checker").await,
            QueryIntent::Research
        );
        // Inner classifier was called exactly once — the next two reads
        // came from cache.
        assert_eq!(inner.call_count(), 1);
    }

    #[tokio::test]
    async fn cached_normalizes_case_and_whitespace() {
        let inner = Arc::new(Stub::new(QueryIntent::Code));
        let cached = CachedClassifier::new(inner.clone(), Duration::from_secs(60));

        cached.classify("Rust Async").await;
        cached.classify("  rust async  ").await;
        cached.classify("rust async").await;
        // All three should hit the same cache key.
        assert_eq!(inner.call_count(), 1);
    }

    #[tokio::test]
    async fn cached_misses_on_expired_ttl() {
        let inner = Arc::new(Stub::new(QueryIntent::Default));
        // Zero TTL → every read is a miss.
        let cached = CachedClassifier::new(inner.clone(), Duration::from_millis(0));

        cached.classify("q").await;
        // Allow Instant to advance past 0.
        tokio::time::sleep(Duration::from_millis(2)).await;
        cached.classify("q").await;
        assert_eq!(inner.call_count(), 2);
    }

    #[tokio::test]
    async fn cached_persists_through_multiple_unique_queries() {
        let inner = Arc::new(Stub::new(QueryIntent::Default));
        let cached = CachedClassifier::new(inner.clone(), Duration::from_secs(60));

        // 50 unique queries — far below eviction threshold.
        for i in 0..50 {
            cached.classify(&format!("query-{i}")).await;
        }
        // Same 50 again — must all hit cache.
        for i in 0..50 {
            cached.classify(&format!("query-{i}")).await;
        }
        assert_eq!(inner.call_count(), 50);
    }

    // ---- Composition: Cached ∘ Hybrid ∘ Stub ------------------------------

    #[tokio::test]
    async fn full_stack_caches_hybrid_decisions() {
        // Mirror the production wiring: Cached(Hybrid(MockMp)).
        let mp_stub = Arc::new(Stub::new(QueryIntent::Comparative));
        let hybrid: Arc<dyn IntentClassifier> = Arc::new(HybridClassifier::new(mp_stub.clone()));
        let cached = CachedClassifier::new(hybrid, Duration::from_secs(60));

        // First read goes rule fast-path → Default → MP stub → Comparative.
        assert_eq!(
            cached.classify("rust vs go for backend services").await,
            QueryIntent::Comparative
        );
        // Second read served from cache; MP stub not consulted again.
        assert_eq!(
            cached.classify("rust vs go for backend services").await,
            QueryIntent::Comparative
        );
        assert_eq!(mp_stub.call_count(), 1);

        // Rule-detectable query never reaches the MP stub.
        assert_eq!(
            cached.classify("github.com").await,
            QueryIntent::Navigational
        );
        assert_eq!(mp_stub.call_count(), 1);
    }
}
