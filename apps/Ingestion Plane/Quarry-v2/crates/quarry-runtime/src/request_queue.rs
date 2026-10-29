//! Durable request queue for long-running batch jobs (Apify-style).
//!
//! Closes the "Apify request queues" gap. Quarry's BatchJobWF currently
//! materializes all URLs in memory before fanout; this works for
//! thousands of URLs but breaks down at millions because the workflow
//! history grows linearly. The `RequestQueue` trait provides a durable,
//! pop-when-needed interface that the orchestrator can drain in chunks
//! across multiple Temporal activity invocations.
//!
//! ## Design
//!
//! - Single-tenant per queue (org_id baked into the queue name).
//! - FIFO with priority lanes: `enqueue(req, priority=Default)`. Higher
//!   priority items pop first.
//! - At-least-once delivery: a popped item is "in-flight" until
//!   `ack(request_id)` confirms the activity succeeded. If the activity
//!   crashes, the item is automatically requeued after `visibility_timeout`.
//! - Idempotency: enqueue is keyed on a stable `request_id`; re-enqueueing
//!   the same id is a no-op.
//!
//! Three implementations ship:
//! - `InMemoryRequestQueue`: dev/test, single-process
//! - `RedisRequestQueue`: prod, durable, multi-worker (TODO)
//! - `PostgresRequestQueue`: prod, durable, transactional (TODO)
//!
//! For now the in-memory backend covers tests; backends are pluggable via
//! the `RequestQueue` trait so production deployments slot in their
//! storage of choice without touching the workflow code.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Low,
    #[default]
    Default,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedRequest {
    pub request_id: String,
    pub url: String,
    pub priority: Priority,
    pub payload: Value,
    pub enqueued_at: DateTime<Utc>,
    /// Number of times this request has been delivered to a worker.
    /// Bumped each time it's `pop()`'d. Workers that see >1 may want to
    /// behave defensively (e.g. assume retries in flight).
    pub attempt: u32,
}

#[derive(Debug, Clone, Default)]
pub struct QueueStats {
    pub queued: usize,
    pub in_flight: usize,
    pub total_enqueued: u64,
    pub total_acked: u64,
    pub total_requeued: u64,
}

#[async_trait]
pub trait RequestQueue: Send + Sync {
    /// Add a request to the queue. Idempotent on `request_id`. Returns
    /// `false` if the request was already known.
    async fn enqueue(
        &self,
        request_id: String,
        url: String,
        priority: Priority,
        payload: Value,
    ) -> QuarryResult<bool>;

    /// Pop the highest-priority request. Returns `None` if the queue is
    /// empty (no waiting). The popped request enters "in-flight" state
    /// and must be `ack`'d or it will be auto-requeued after the
    /// visibility timeout.
    async fn pop(&self) -> QuarryResult<Option<QueuedRequest>>;

    /// Acknowledge successful processing. Removes the request permanently.
    async fn ack(&self, request_id: &str) -> QuarryResult<()>;

    /// Move expired in-flight requests back to the queue. Returns the
    /// number of items requeued. Workers/orchestrators should call this
    /// periodically.
    async fn reap_expired(&self) -> QuarryResult<u64>;

    /// Drop a request without acking it (use sparingly — typically when
    /// the request hits a permanent failure that retry won't fix).
    async fn fail_permanently(&self, request_id: &str, reason: &str) -> QuarryResult<()>;

    async fn stats(&self) -> QuarryResult<QueueStats>;
}

#[derive(Debug, Clone)]
pub struct InMemoryRequestQueue {
    inner: Arc<RwLock<QueueState>>,
    visibility_timeout: Duration,
    /// Hard cap on queued + in-flight items. `enqueue` returns
    /// `Err(ResourceExhausted)` when adding would exceed this. None = unbounded.
    /// Defaults to 100,000 to protect against runaway producers.
    max_size: Option<usize>,
}

#[derive(Debug)]
struct QueueState {
    /// Priority-keyed FIFOs.
    by_priority: BTreeMap<Priority, VecDeque<QueuedRequest>>,
    /// Active request_ids, queued or in-flight (used for dedup).
    known: HashSet<String>,
    /// Currently-popped requests pending ack, with their checkout time.
    in_flight: HashMap<String, (QueuedRequest, Instant)>,
    /// Counters for observability.
    total_enqueued: u64,
    total_acked: u64,
    total_requeued: u64,
}

impl InMemoryRequestQueue {
    pub fn new(visibility_timeout: Duration) -> Self {
        Self {
            inner: Arc::new(RwLock::new(QueueState {
                by_priority: BTreeMap::new(),
                known: HashSet::new(),
                in_flight: HashMap::new(),
                total_enqueued: 0,
                total_acked: 0,
                total_requeued: 0,
            })),
            visibility_timeout,
            max_size: Some(100_000),
        }
    }

    /// Configure a hard cap. Use `None` to disable the cap (NOT recommended
    /// for production — buggy producers can OOM).
    pub fn with_max_size(mut self, max_size: Option<usize>) -> Self {
        self.max_size = max_size;
        self
    }
}

impl Default for InMemoryRequestQueue {
    fn default() -> Self {
        Self::new(Duration::from_secs(60))
    }
}

#[async_trait]
impl RequestQueue for InMemoryRequestQueue {
    async fn enqueue(
        &self,
        request_id: String,
        url: String,
        priority: Priority,
        payload: Value,
    ) -> QuarryResult<bool> {
        if request_id.is_empty() {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "request_id must not be empty",
            ));
        }
        let mut s = self.inner.write().await;

        // Check size cap BEFORE the dedup check so a flood of duplicate
        // request_ids can't bypass the cap.
        if let Some(cap) = self.max_size {
            let total: usize =
                s.by_priority.values().map(|q| q.len()).sum::<usize>() + s.in_flight.len();
            if total >= cap && !s.known.contains(&request_id) {
                return Err(QuarryError::new(
                    ErrorCode::RateLimited,
                    format!("request queue full (cap={cap}, current={total}); enqueue rejected"),
                ));
            }
        }

        if !s.known.insert(request_id.clone()) {
            return Ok(false);
        }
        let req = QueuedRequest {
            request_id,
            url,
            priority,
            payload,
            enqueued_at: Utc::now(),
            attempt: 0,
        };
        s.by_priority.entry(priority).or_default().push_back(req);
        s.total_enqueued += 1;
        Ok(true)
    }

    async fn pop(&self) -> QuarryResult<Option<QueuedRequest>> {
        let mut s = self.inner.write().await;
        // Iterate priorities high → low.
        let priorities: Vec<Priority> = s.by_priority.keys().rev().copied().collect();
        for p in priorities {
            if let Some(queue) = s.by_priority.get_mut(&p) {
                if let Some(mut req) = queue.pop_front() {
                    req.attempt += 1;
                    let id = req.request_id.clone();
                    s.in_flight.insert(id, (req.clone(), Instant::now()));
                    return Ok(Some(req));
                }
            }
        }
        Ok(None)
    }

    async fn ack(&self, request_id: &str) -> QuarryResult<()> {
        let mut s = self.inner.write().await;
        if s.in_flight.remove(request_id).is_some() {
            s.known.remove(request_id);
            s.total_acked += 1;
            Ok(())
        } else {
            Err(QuarryError::new(
                ErrorCode::NotFound,
                format!("request {request_id} not in flight"),
            ))
        }
    }

    async fn reap_expired(&self) -> QuarryResult<u64> {
        let now = Instant::now();
        let mut s = self.inner.write().await;
        let timeout = self.visibility_timeout;
        let expired: Vec<String> = s
            .in_flight
            .iter()
            .filter(|(_, (_, checkout))| now.duration_since(*checkout) > timeout)
            .map(|(id, _)| id.clone())
            .collect();

        let mut requeued = 0u64;
        for id in expired {
            if let Some((req, _)) = s.in_flight.remove(&id) {
                let priority = req.priority;
                s.by_priority.entry(priority).or_default().push_back(req);
                requeued += 1;
            }
        }
        s.total_requeued += requeued;
        Ok(requeued)
    }

    async fn fail_permanently(&self, request_id: &str, reason: &str) -> QuarryResult<()> {
        let mut s = self.inner.write().await;
        s.in_flight.remove(request_id);
        s.known.remove(request_id);
        tracing::warn!(request_id, reason, "request failed permanently");
        Ok(())
    }

    async fn stats(&self) -> QuarryResult<QueueStats> {
        let s = self.inner.read().await;
        let queued: usize = s.by_priority.values().map(|q| q.len()).sum();
        Ok(QueueStats {
            queued,
            in_flight: s.in_flight.len(),
            total_enqueued: s.total_enqueued,
            total_acked: s.total_acked,
            total_requeued: s.total_requeued,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn enqueue_pop_ack_roundtrip() {
        let q = InMemoryRequestQueue::default();
        q.enqueue(
            "r1".into(),
            "https://x".into(),
            Priority::Default,
            json!({}),
        )
        .await
        .unwrap();

        let popped = q.pop().await.unwrap().unwrap();
        assert_eq!(popped.request_id, "r1");
        assert_eq!(popped.attempt, 1);

        q.ack("r1").await.unwrap();
        let again = q.pop().await.unwrap();
        assert!(again.is_none());

        let s = q.stats().await.unwrap();
        assert_eq!(s.total_acked, 1);
        assert_eq!(s.total_enqueued, 1);
    }

    #[tokio::test]
    async fn enqueue_is_idempotent_on_request_id() {
        let q = InMemoryRequestQueue::default();
        let first = q
            .enqueue(
                "dup".into(),
                "https://x".into(),
                Priority::Default,
                json!({}),
            )
            .await
            .unwrap();
        let second = q
            .enqueue(
                "dup".into(),
                "https://x".into(),
                Priority::Default,
                json!({}),
            )
            .await
            .unwrap();
        assert!(first);
        assert!(!second);
        let stats = q.stats().await.unwrap();
        assert_eq!(stats.queued, 1);
    }

    #[tokio::test]
    async fn high_priority_pops_before_default() {
        let q = InMemoryRequestQueue::default();
        q.enqueue("low".into(), "u".into(), Priority::Low, json!({}))
            .await
            .unwrap();
        q.enqueue("def".into(), "u".into(), Priority::Default, json!({}))
            .await
            .unwrap();
        q.enqueue("hi".into(), "u".into(), Priority::High, json!({}))
            .await
            .unwrap();

        let p1 = q.pop().await.unwrap().unwrap();
        let p2 = q.pop().await.unwrap().unwrap();
        let p3 = q.pop().await.unwrap().unwrap();
        assert_eq!(p1.request_id, "hi");
        assert_eq!(p2.request_id, "def");
        assert_eq!(p3.request_id, "low");
    }

    #[tokio::test]
    async fn ack_unknown_returns_not_found() {
        let q = InMemoryRequestQueue::default();
        let err = q.ack("nope").await.unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
    }

    #[tokio::test]
    async fn reap_expired_requeues_in_flight_after_timeout() {
        let q = InMemoryRequestQueue::new(Duration::from_millis(10));
        q.enqueue("r".into(), "u".into(), Priority::Default, json!({}))
            .await
            .unwrap();
        let _ = q.pop().await.unwrap().unwrap(); // checkout

        // Before timeout — no reap.
        let reaped_immediate = q.reap_expired().await.unwrap();
        assert_eq!(reaped_immediate, 0);

        tokio::time::sleep(Duration::from_millis(30)).await;
        let reaped = q.reap_expired().await.unwrap();
        assert_eq!(reaped, 1);

        // Now poppable again.
        let again = q.pop().await.unwrap().unwrap();
        assert_eq!(again.request_id, "r");
        assert_eq!(again.attempt, 2, "second delivery bumps attempt counter");
    }

    #[tokio::test]
    async fn fail_permanently_drops_request_silently() {
        let q = InMemoryRequestQueue::default();
        q.enqueue("doomed".into(), "u".into(), Priority::Default, json!({}))
            .await
            .unwrap();
        let _ = q.pop().await.unwrap().unwrap();
        q.fail_permanently("doomed", "blocklisted").await.unwrap();

        // Reap shouldn't bring it back.
        tokio::time::sleep(Duration::from_millis(10)).await;
        let reaped = q.reap_expired().await.unwrap();
        assert_eq!(reaped, 0);
        let s = q.stats().await.unwrap();
        assert_eq!(s.queued, 0);
        assert_eq!(s.in_flight, 0);
    }

    #[tokio::test]
    async fn pop_empty_queue_returns_none() {
        let q = InMemoryRequestQueue::default();
        let got = q.pop().await.unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn empty_request_id_rejected() {
        let q = InMemoryRequestQueue::default();
        let err = q
            .enqueue("".into(), "u".into(), Priority::Default, json!({}))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
    }

    #[tokio::test]
    async fn priority_default_is_default_variant() {
        assert_eq!(Priority::default(), Priority::Default);
    }

    #[tokio::test]
    async fn enqueue_rejects_when_max_size_exceeded() {
        let q = InMemoryRequestQueue::default().with_max_size(Some(2));
        q.enqueue("a".into(), "u".into(), Priority::Default, json!({}))
            .await
            .unwrap();
        q.enqueue("b".into(), "u".into(), Priority::Default, json!({}))
            .await
            .unwrap();

        // Re-enqueue of existing id is still idempotent (returns false, doesn't reject).
        let ret = q
            .enqueue("a".into(), "u".into(), Priority::Default, json!({}))
            .await
            .unwrap();
        assert!(!ret);

        // New id past the cap is rejected.
        let err = q
            .enqueue("c".into(), "u".into(), Priority::Default, json!({}))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::RateLimited);
        assert!(err.message.contains("cap=2"));
    }

    #[tokio::test]
    async fn enqueue_unbounded_when_max_size_none() {
        let q = InMemoryRequestQueue::default().with_max_size(None);
        for i in 0..1_000 {
            q.enqueue(format!("r{i}"), "u".into(), Priority::Default, json!({}))
                .await
                .unwrap();
        }
        let stats = q.stats().await.unwrap();
        assert_eq!(stats.queued, 1_000);
    }

    #[tokio::test]
    async fn cap_counts_in_flight_too() {
        let q = InMemoryRequestQueue::default().with_max_size(Some(2));
        q.enqueue("a".into(), "u".into(), Priority::Default, json!({}))
            .await
            .unwrap();
        q.enqueue("b".into(), "u".into(), Priority::Default, json!({}))
            .await
            .unwrap();
        let _ = q.pop().await.unwrap().unwrap(); // a is now in-flight
                                                 // Queued is now 1, in-flight is 1, total = 2, still at cap.
        let err = q
            .enqueue("c".into(), "u".into(), Priority::Default, json!({}))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::RateLimited);
    }

    #[tokio::test]
    async fn stats_reflects_lifecycle() {
        let q = InMemoryRequestQueue::default();
        q.enqueue("a".into(), "u".into(), Priority::Default, json!({}))
            .await
            .unwrap();
        q.enqueue("b".into(), "u".into(), Priority::Default, json!({}))
            .await
            .unwrap();

        let s1 = q.stats().await.unwrap();
        assert_eq!(s1.queued, 2);
        assert_eq!(s1.in_flight, 0);

        let _ = q.pop().await.unwrap();
        let s2 = q.stats().await.unwrap();
        assert_eq!(s2.queued, 1);
        assert_eq!(s2.in_flight, 1);

        q.ack("a").await.unwrap();
        let s3 = q.stats().await.unwrap();
        assert_eq!(s3.in_flight, 0);
        assert_eq!(s3.total_acked, 1);
    }
}
