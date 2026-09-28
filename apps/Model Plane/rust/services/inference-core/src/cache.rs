//! In-memory prompt cache using `DashMap` with TTL expiry and
//! least-recently-used eviction at capacity.
//!
//! Keyed by blake3 hash of (tenant scope + model + serialized messages +
//! response-shaping fields). Only caches non-streaming responses. TTL defaults
//! to 5 minutes. ZDR requests neither read nor write this cache.

use std::time::{Duration, Instant};

use dashmap::DashMap;
use tracing::debug;

use crate::provider::{InferRequest, InferResponse};

/// Maximum number of cached entries.
const MAX_ENTRIES: usize = 10_000;

/// Low-water mark after a capacity eviction: drop the least-recently-used
/// entries down to this. The ~10% headroom lets the O(n) eviction scan amortize
/// over many subsequent inserts instead of running on every at-capacity `put`.
const EVICT_TO: usize = 9_000;

/// A cached response with expiry and last-access timestamps (for LRU eviction).
struct CacheEntry {
    response: InferResponse,
    expires_at: Instant,
    last_access: Instant,
}

/// Thread-safe prompt cache with TTL-based eviction.
pub struct PromptCache {
    entries: DashMap<String, CacheEntry>,
    ttl: Duration,
}

impl PromptCache {
    /// Create a new prompt cache with the given TTL in seconds.
    #[must_use]
    pub fn new(ttl_secs: u64) -> Self {
        Self {
            entries: DashMap::new(),
            ttl: Duration::from_secs(ttl_secs),
        }
    }

    /// Compute the cache key for a request. Covers every field that changes the
    /// completion: model + messages + sampling **and** the response-shaping
    /// fields (`tools`, `tool_choice`, structured-output schema). Omitting the latter
    /// let an identical-message request *with* tools/schema collide with one
    /// *without* and be served the wrong (tool-less / unstructured) answer.
    ///
    /// The tenant scope (`org_id`, `user_id`) is part of the key, and is the FIRST
    /// thing hashed. Without it this cache spans tenants: two orgs asking a
    /// byte-identical question share one entry. The served text would be
    /// legitimate — a prompt-identical completion — but the shared entry is a
    /// cross-tenant **existence oracle**: a caller can probe a guessed prompt and
    /// learn from the latency whether someone else already asked it, which leaks
    /// another org's activity without leaking any content. `user_id` is included
    /// for the same reason inside an org, where private-until-shared ownership
    /// means one user's questions are not another's to discover. The hit rate
    /// barely moves: the key already pins the entire message history under a
    /// 5-minute TTL, so realistic hits are the same user regenerating or
    /// double-submitting — which stays cached.
    ///
    /// Lengths are hashed with each identity field so `("ab", "c")` cannot
    /// collide with `("a", "bc")`.
    fn cache_key(req: &InferRequest) -> String {
        let mut hasher = blake3::Hasher::new();
        for scope in [
            req.org_id.as_str(),
            req.user_id.as_str(),
            req.subscription_connection_id.as_str(),
        ] {
            hasher.update(&(scope.len() as u64).to_le_bytes());
            hasher.update(scope.as_bytes());
        }
        hasher.update(req.provider_hint.as_bytes());
        hasher.update(req.model.as_bytes());
        for msg in &req.messages {
            hasher.update(msg.role.as_bytes());
            hasher.update(msg.content.as_bytes());
            hasher.update(&(msg.compaction_summary.len() as u64).to_le_bytes());
            hasher.update(msg.compaction_summary.as_bytes());
        }
        hasher.update(&req.temperature.to_le_bytes());
        hasher.update(&req.max_tokens.to_le_bytes());
        // Response-shaping fields — a tool/schema change must miss the cache.
        for tool in &req.tools {
            hasher.update(tool.name.as_bytes());
            hasher.update(tool.parameters_json.as_bytes());
        }
        hasher.update(req.tool_choice.as_bytes());
        if let Some(schema) = &req.structured_output_schema {
            hasher.update(schema.as_bytes());
        }
        hasher.finalize().to_hex().to_string()
    }

    /// Look up a cached response. Returns `None` on miss, expired entry, or ZDR request.
    pub fn get(&self, req: &InferRequest) -> Option<InferResponse> {
        if req.zdr {
            return None;
        }
        let key = Self::cache_key(req);

        let mut entry = self.entries.get_mut(&key)?;
        if entry.expires_at < Instant::now() {
            drop(entry);
            self.entries.remove(&key);
            debug!(cache_key = %key, "cache entry expired");
            return None;
        }
        // Record the read so capacity eviction is genuinely least-recently-used.
        entry.last_access = Instant::now();
        Some(entry.response.clone())
    }

    /// Insert a response into the cache. Skips insertion for ZDR requests.
    pub fn put(&self, req: &InferRequest, response: &InferResponse) {
        if req.zdr {
            return;
        }
        // At capacity: reclaim expired entries first, then, if still full, evict
        // the least-recently-used entries down to EVICT_TO. Previously this path
        // skipped insertion entirely once full, so a busy tenant's cache silently
        // froze — every new prompt was dropped and the cache degraded to a no-op,
        // forfeiting all further inference-cost savings.
        if self.entries.len() >= MAX_ENTRIES {
            self.evict_expired();
            if self.entries.len() >= MAX_ENTRIES {
                self.evict_lru();
            }
        }

        let now = Instant::now();
        let key = Self::cache_key(req);
        self.entries.insert(
            key,
            CacheEntry {
                response: response.clone(),
                expires_at: now + self.ttl,
                last_access: now,
            },
        );
    }

    /// Remove all expired entries.
    fn evict_expired(&self) {
        let now = Instant::now();
        self.entries.retain(|_, entry| entry.expires_at > now);
    }

    /// Evict the least-recently-used entries down to `EVICT_TO`. Runs only when
    /// the cache is still at capacity after expired-eviction. Takes one O(n)
    /// snapshot of (key, `last_access`) and removes the oldest `len - EVICT_TO`;
    /// batching to the low-water mark amortizes the scan over the freed headroom.
    fn evict_lru(&self) {
        let mut stamps: Vec<(String, Instant)> = self
            .entries
            .iter()
            .map(|entry| (entry.key().clone(), entry.last_access))
            .collect();
        if stamps.len() <= EVICT_TO {
            return;
        }
        // Most-recently-used first; evict everything past the low-water mark.
        stamps.sort_unstable_by(|a, b| b.1.cmp(&a.1));
        for (key, _) in stamps.into_iter().skip(EVICT_TO) {
            self.entries.remove(&key);
        }
    }

    /// Number of entries currently in the cache.
    #[allow(dead_code)]
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    #[allow(dead_code)]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ChatMessage, InferRequest, InferResponse};

    fn sample_request() -> InferRequest {
        InferRequest {
            request_id: "req-1".to_owned(),
            provider_hint: String::new(),
            model: "claude-sonnet-4-20250514".to_owned(),
            messages: vec![ChatMessage {
                compaction_summary: String::new(),
                role: "user".to_owned(),
                content: "Hello".to_owned(),
                name: String::new(),
            }],
            temperature: 0.7,
            max_tokens: 1024,
            structured_output_schema: None,
            zdr: false,
            ..Default::default()
        }
    }

    fn sample_response() -> InferResponse {
        InferResponse {
            compaction_summary: String::new(),
            request_id: "req-1".to_owned(),
            content: "Hi there!".to_owned(),
            model_used: "claude-sonnet-4-20250514".to_owned(),
            stop_reason: "end_turn".to_owned(),
            input_tokens: 10,
            output_tokens: 5,
            ..Default::default()
        }
    }

    #[test]
    fn cache_miss_returns_none() {
        let cache = PromptCache::new(300);
        assert!(cache.get(&sample_request()).is_none());
    }

    #[test]
    fn cache_hit_returns_response() {
        let cache = PromptCache::new(300);
        let req = sample_request();
        let resp = sample_response();

        cache.put(&req, &resp);
        let cached = cache.get(&req);

        assert!(cached.is_some());
        assert_eq!(cached.unwrap().content, "Hi there!");
    }

    #[test]
    fn zdr_request_never_persists_or_reads_durable_state() {
        // ZDR contract for inference-core's only durable response sink: under
        // `req.zdr == true` the prompt cache must neither write durable state
        // (`put` is a no-op) nor read it back (`get` returns None). This pins
        // the cache.rs short-circuits so a future change cannot silently
        // reintroduce a Zero-Data-Retention leak through the cache.
        let cache = PromptCache::new(300);
        let mut req = sample_request();
        req.zdr = true;
        let resp = sample_response();

        // Insert under ZDR — must write nothing durable.
        cache.put(&req, &resp);
        assert!(cache.is_empty(), "ZDR put must not persist any cache entry");

        // Read under ZDR — must short-circuit to a miss even if state existed.
        assert!(
            cache.get(&req).is_none(),
            "ZDR get must not read cached state"
        );

        // Even when a non-ZDR entry for the identical prompt is already present
        // (same cache key — `zdr` is intentionally excluded from `cache_key`),
        // a ZDR request must still refuse to read it.
        let mut non_zdr = req.clone();
        non_zdr.zdr = false;
        cache.put(&non_zdr, &resp);
        assert!(
            cache.get(&non_zdr).is_some(),
            "non-ZDR request should still hit the cache"
        );
        assert!(
            cache.get(&req).is_none(),
            "ZDR get must refuse to read even a pre-existing non-ZDR entry"
        );
    }

    #[test]
    fn expired_entry_evicted() {
        let cache = PromptCache::new(0); // 0 second TTL
        let req = sample_request();
        let resp = sample_response();

        cache.put(&req, &resp);
        // Entry expires immediately
        std::thread::sleep(std::time::Duration::from_millis(10));

        assert!(cache.get(&req).is_none());
    }

    #[test]
    fn full_cache_evicts_lru_instead_of_dropping_new_entries() {
        // Regression: at capacity the cache used to skip insertion entirely,
        // freezing to a no-op. It must instead evict old entries and still admit
        // new ones, staying bounded at MAX_ENTRIES.
        let cache = PromptCache::new(300);
        for i in 0..(MAX_ENTRIES + 200) {
            let mut req = sample_request();
            req.messages[0].content = format!("prompt-{i}");
            cache.put(&req, &sample_response());
        }
        assert!(
            cache.len() <= MAX_ENTRIES,
            "cache exceeded MAX_ENTRIES: {}",
            cache.len()
        );
        // The most recently inserted prompt must be retrievable — the old
        // skip-when-full behavior would have dropped it.
        let mut newest = sample_request();
        newest.messages[0].content = format!("prompt-{}", MAX_ENTRIES + 199);
        assert!(
            cache.get(&newest).is_some(),
            "newest entry was dropped — cache froze at capacity"
        );
    }

    #[test]
    fn different_models_different_keys() {
        let cache = PromptCache::new(300);
        let mut req1 = sample_request();
        req1.model = "model-a".to_owned();

        let mut req2 = sample_request();
        req2.model = "model-b".to_owned();

        let resp = sample_response();
        cache.put(&req1, &resp);

        assert!(cache.get(&req1).is_some());
        assert!(cache.get(&req2).is_none());
    }

    /// A byte-identical prompt from a different org must MISS.
    ///
    /// Not because the completion would be wrong — it would be a legitimate
    /// prompt-identical answer — but because a shared entry turns the cache into
    /// a cross-tenant existence oracle: org B probes a guessed prompt and learns
    /// from the hit whether org A already asked it.
    #[test]
    fn an_identical_prompt_from_another_org_does_not_hit() {
        let cache = PromptCache::new(300);
        let mut org_a = sample_request();
        org_a.org_id = "org-a".to_owned();
        let mut org_b = sample_request();
        org_b.org_id = "org-b".to_owned();

        cache.put(&org_a, &sample_response());

        assert!(cache.get(&org_a).is_some(), "the owning org still hits");
        assert!(
            cache.get(&org_b).is_none(),
            "cross-tenant cache hit: org B was served org A's cached entry"
        );
    }

    /// Same reasoning one level down: private-until-shared ownership means a
    /// colleague's questions are not mine to discover by probing.
    #[test]
    fn an_identical_prompt_from_another_user_in_the_same_org_does_not_hit() {
        let cache = PromptCache::new(300);
        let mut mine = sample_request();
        mine.org_id = "org-a".to_owned();
        mine.user_id = "user-1".to_owned();
        let mut theirs = sample_request();
        theirs.org_id = "org-a".to_owned();
        theirs.user_id = "user-2".to_owned();

        cache.put(&mine, &sample_response());

        assert!(cache.get(&mine).is_some());
        assert!(cache.get(&theirs).is_none());
    }

    /// A user may have multiple subscription connections. A response generated
    /// under one plan/account must not be served as a cache hit for another.
    #[test]
    fn an_identical_prompt_from_another_subscription_connection_does_not_hit() {
        let cache = PromptCache::new(300);
        let mut first = sample_request();
        first.org_id = "org-a".to_owned();
        first.user_id = "user-1".to_owned();
        first.subscription_connection_id = "conn-a".to_owned();
        let mut second = first.clone();
        second.subscription_connection_id = "conn-b".to_owned();

        cache.put(&first, &sample_response());

        assert!(cache.get(&first).is_some());
        assert!(
            cache.get(&second).is_none(),
            "a second subscription connection must not share cached output"
        );
    }

    /// The same caller re-asking must still hit — this is the cache's real
    /// workload (regenerate / double-submit), so scoping must not disable it.
    #[test]
    fn the_same_caller_still_hits_so_scoping_keeps_the_cache_useful() {
        let cache = PromptCache::new(300);
        let mut req = sample_request();
        req.org_id = "org-a".to_owned();
        req.user_id = "user-1".to_owned();

        cache.put(&req, &sample_response());

        let mut retry = req.clone();
        retry.request_id = "req-2".to_owned();
        assert!(
            cache.get(&retry).is_some(),
            "a retry of the same prompt by the same caller must still be cached"
        );
    }

    /// Identity fields are length-prefixed, so a boundary shift cannot collide.
    #[test]
    fn adjacent_identity_fields_cannot_collide_across_the_boundary() {
        let cache = PromptCache::new(300);
        let mut split_one = sample_request();
        split_one.org_id = "ab".to_owned();
        split_one.user_id = "c".to_owned();
        let mut split_two = sample_request();
        split_two.org_id = "a".to_owned();
        split_two.user_id = "bc".to_owned();

        cache.put(&split_one, &sample_response());

        assert!(cache.get(&split_two).is_none());
    }
}
