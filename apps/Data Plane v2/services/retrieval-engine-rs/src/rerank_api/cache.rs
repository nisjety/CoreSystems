//! The replay cache of `POST /v1/rerank` (QB-T05a).
//!
//! It holds scores and input offsets only, never query or document text. Only an `ok`
//! outcome is stored, so a degraded call can be retried. The key is the cost idempotency key,
//! which already includes the org, the caller's request id and a digest of the content, so a
//! reused request id with different content is a separate entry and a separate metered call.

use std::sync::Arc;

use super::limits::{REPLAY_CAPACITY, REPLAY_TTL};

/// Ranked `(input index, score)` pairs, best first.
pub type RankedScores = Arc<Vec<(usize, f32)>>;

#[derive(Clone)]
pub struct RerankCache {
    inner: moka::future::Cache<String, RankedScores>,
}

impl RerankCache {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: moka::future::Cache::builder()
                .max_capacity(REPLAY_CAPACITY)
                .time_to_live(REPLAY_TTL)
                .build(),
        }
    }

    pub async fn get(&self, key: &str) -> Option<RankedScores> {
        self.inner.get(key).await
    }

    pub async fn put(&self, key: String, ranked: Vec<(usize, f32)>) {
        self.inner.insert(key, Arc::new(ranked)).await;
    }
}

impl Default for RerankCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_entry_round_trips_and_a_missing_key_is_none() {
        let cache = RerankCache::new();
        assert!(cache.get("rerank:o:r:d").await.is_none());
        cache
            .put("rerank:o:r:d".to_owned(), vec![(2, 0.9), (0, 0.1)])
            .await;
        let hit = cache.get("rerank:o:r:d").await.expect("hit");
        assert_eq!(hit.as_slice(), &[(2, 0.9), (0, 0.1)]);
        assert!(
            cache.get("rerank:other-org:r:d").await.is_none(),
            "keys are org-scoped"
        );
    }
}
