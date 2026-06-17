//! In-memory LRU prompt cache using `DashMap` with TTL-based eviction.
//!
//! Keyed by blake3 hash of (model + serialized messages). Only caches
//! non-streaming responses. TTL defaults to 5 minutes.

use std::time::{Duration, Instant};

use dashmap::DashMap;
use tracing::debug;

use crate::provider::{InferRequest, InferResponse};

/// Maximum number of cached entries.
const MAX_ENTRIES: usize = 10_000;

/// A cached response with expiry timestamp.
struct CacheEntry {
    response: InferResponse,
    expires_at: Instant,
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
    fn cache_key(req: &InferRequest) -> String {
        let mut hasher = blake3::Hasher::new();
        hasher.update(req.provider_hint.as_bytes());
        hasher.update(req.model.as_bytes());
        for msg in &req.messages {
            hasher.update(msg.role.as_bytes());
            hasher.update(msg.content.as_bytes());
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

        let entry = self.entries.get(&key)?;
        if entry.expires_at < Instant::now() {
            drop(entry);
            self.entries.remove(&key);
            debug!(cache_key = %key, "cache entry expired");
            return None;
        }

        Some(entry.response.clone())
    }

    /// Insert a response into the cache. Skips insertion for ZDR requests.
    pub fn put(&self, req: &InferRequest, response: &InferResponse) {
        if req.zdr {
            return;
        }
        // Simple eviction: if at capacity, skip insertion.
        // A more sophisticated LRU could be added later.
        if self.entries.len() >= MAX_ENTRIES {
            self.evict_expired();
            if self.entries.len() >= MAX_ENTRIES {
                debug!("cache full, skipping insertion");
                return;
            }
        }

        let key = Self::cache_key(req);
        self.entries.insert(
            key,
            CacheEntry {
                response: response.clone(),
                expires_at: Instant::now() + self.ttl,
            },
        );
    }

    /// Remove all expired entries.
    fn evict_expired(&self) {
        let now = Instant::now();
        self.entries.retain(|_, entry| entry.expires_at > now);
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
}
