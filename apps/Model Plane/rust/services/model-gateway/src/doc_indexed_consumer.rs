//! Phase 4 — Data Plane v2 durable-retrieval readiness signal.
//!
//! The READ-path dual of the Quarry→Data Plane write path (see
//! `docs/plans/quarry-dataplane-integration-fix-plan.md` §Phase 4). Quarry
//! scrapes a URL and (when `ingest:true`) hands it to Data Plane v2, which
//! stores the document, chunks it, then embeds every knowledge unit
//! **asynchronously**. The agent gets Quarry's inline scrape result
//! immediately, but the document only becomes *durably retrievable* once all
//! its units reach `embedding_status='done'`.
//!
//! At that moment Data Plane v2's embedding-engine flips `documents.status`
//! to `indexed` and publishes a plain-JSON event on the core-NATS subject
//! [`DOC_INDEXED_SUBJECT`]:
//!
//! ```json
//! { "document_id": "...", "org_id": "...", "title": "...",
//!   "embedding_model": "...", "embedding_provider": "...",
//!   "idempotency_key": "doc.indexed-…" }
//! ```
//!
//! (Producer: `Data Plane v2/services/embedding-engine-rs/src/batch/mod.rs`.)
//!
//! This module is the consumer. [`run`] subscribes to that subject and records
//! each `(org_id, document_id)` in a [`DocReadyRegistry`], which the retrieval
//! relay ([`crate::dataplane::retrieve`]) can [`await_ready`] on so a
//! just-ingested document's embeddings can be *awaited* ("retrieve durable on
//! next turn") rather than guessed.
//!
//! ## Optional / best-effort, with a timeout fallback — by design
//!
//! Everything here is non-blocking-to-the-gateway and self-guarding:
//!
//! - [`run`] disables itself when `NATS_URL` is unset or the bus is
//!   unreachable, exactly like [`crate::capability_consumer`]; the gateway is
//!   never blocked on its presence.
//! - The event is published over **core** NATS (at-most-once), so this is a
//!   core subscription. A missed signal is not fatal: [`await_ready`] simply
//!   times out and the caller falls back to the inline scrape context it
//!   already holds. The registry is an *optimization* (turn a guess into a
//!   bounded wait), never a correctness dependency — the authoritative
//!   per-document status remains `DocumentService.GetDocumentIndexStatus`
//!   (surfaced as `pending_documents` in the retrieval response).
//! - The registry is process-local and bounded ([`DEFAULT_CAPACITY`], FIFO
//!   eviction) so it can never leak memory across a long-lived gateway.
//!
//! ## Verification
//!
//! [`parse_indexed`] (pure) and the [`DocReadyRegistry`] behavior (mark / fast
//! path / concurrent wake / timeout / org isolation / eviction) are
//! unit-tested below. The live [`run`] loop needs a running NATS bus to
//! exercise end-to-end; it is best-effort and self-guarding, so it never
//! breaks the gateway when NATS is absent (mirrors `capability_consumer`).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dashmap::DashMap;
use serde::Deserialize;
use tokio::sync::Notify;
use tracing::{debug, info, warn};

/// Core-NATS subject the Data Plane v2 embedding-engine publishes on once a
/// document is fully embedded and flipped to `status='indexed'`. Must match
/// `SUBJECT_DOC_INDEXED` in the producer (`embedding-engine-rs/.../batch/mod.rs`).
pub const DOC_INDEXED_SUBJECT: &str = "dataplane.documents.indexed";

/// Max `(org, document)` readiness entries retained process-wide before FIFO
/// eviction. ~50k keys ≈ a few MB; far above the in-flight ingest working set,
/// so eviction of an *actively awaited* key is not a practical concern.
pub const DEFAULT_CAPACITY: usize = 50_000;

/// Decoded `dataplane.documents.indexed` event. Unknown fields
/// (`embedding_model`, `idempotency_key`, …) are tolerated and ignored — only
/// the identity (`org_id`, `document_id`) drives readiness; `title` is kept for
/// observability.
#[derive(Debug, Clone, Deserialize)]
pub struct DocIndexedEvent {
    pub document_id: String,
    pub org_id: String,
    #[serde(default)]
    pub title: String,
}

/// Pure: decode + validate a raw event payload.
///
/// Returns `Some` only for a well-formed event carrying both a non-empty
/// `org_id` and `document_id`; malformed JSON or a missing identity yields
/// `None` (the [`run`] loop logs and skips it). Kept pure and separate from the
/// IO loop so it can be unit-tested without a bus.
#[must_use]
pub fn parse_indexed(payload: &[u8]) -> Option<DocIndexedEvent> {
    let ev: DocIndexedEvent = serde_json::from_slice(payload).ok()?;
    if ev.org_id.is_empty() || ev.document_id.is_empty() {
        return None;
    }
    Some(ev)
}

#[derive(Debug)]
struct Inner {
    /// `(org_id, document_id)` → indexed. Presence == durably retrievable.
    ready: DashMap<(String, String), ()>,
    /// Insertion order for O(1) FIFO eviction at capacity.
    order: Mutex<VecDeque<(String, String)>>,
    /// Wakes every in-flight [`DocReadyRegistry::await_ready`] on each new mark.
    notify: Notify,
    capacity: usize,
}

/// Process-local registry of documents that Data Plane v2 has reported as
/// fully indexed. Cheap to clone (`Arc`-backed) and shared via [`crate::state::AppState`].
#[derive(Clone, Debug)]
pub struct DocReadyRegistry {
    inner: Arc<Inner>,
}

impl Default for DocReadyRegistry {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }
}

impl DocReadyRegistry {
    /// New registry with the default capacity.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// New registry with an explicit eviction capacity (≥ 1). Primarily for
    /// tests that want to exercise eviction without inserting 50k entries.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Inner {
                ready: DashMap::new(),
                order: Mutex::new(VecDeque::new()),
                notify: Notify::new(),
                capacity: capacity.max(1),
            }),
        }
    }

    /// Record that `(org_id, document_id)` is durably retrievable and wake any
    /// waiters. Idempotent: a repeat signal does not re-queue the key for
    /// eviction. Evicts the oldest entry when over [`Inner::capacity`].
    pub fn mark_indexed(&self, org_id: &str, document_id: &str) {
        let key = (org_id.to_owned(), document_id.to_owned());
        // Only touch the order ring + evict when this is a *new* key.
        if self.inner.ready.insert(key.clone(), ()).is_none() {
            // Recover from a poisoned lock rather than panic — the critical
            // section below cannot itself panic, so poisoning would only be
            // inherited, and dropping a few order entries is harmless.
            let mut order = self
                .inner
                .order
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            order.push_back(key);
            while order.len() > self.inner.capacity {
                if let Some(evicted) = order.pop_front() {
                    self.inner.ready.remove(&evicted);
                }
            }
        }
        // Wake awaiters even on a duplicate mark — cheap, and avoids any
        // chance of a waiter that armed between insert and now being missed.
        self.inner.notify.notify_waiters();
    }

    /// Non-blocking check: is `(org_id, document_id)` already known indexed?
    #[must_use]
    pub fn is_ready(&self, org_id: &str, document_id: &str) -> bool {
        self.inner
            .ready
            .contains_key(&(org_id.to_owned(), document_id.to_owned()))
    }

    /// Await durable-retrieval readiness for `(org_id, document_id)`, up to
    /// `within`. Returns `true` if the document is (or becomes) indexed before
    /// the deadline, `false` on timeout — the caller then falls back to inline
    /// scrape context.
    ///
    /// Lost-wakeup-free: each iteration arms `Notify::notified()` *before*
    /// re-checking the set, so a [`mark_indexed`] racing between the check and
    /// the arm is still observed.
    pub async fn await_ready(&self, org_id: &str, document_id: &str, within: Duration) -> bool {
        let key = (org_id.to_owned(), document_id.to_owned());
        if self.inner.ready.contains_key(&key) {
            return true;
        }
        let wait = async {
            loop {
                // Arm the wakeup first, then re-check: closes the race where a
                // mark lands between the check and the await.
                let notified = self.inner.notify.notified();
                if self.inner.ready.contains_key(&key) {
                    return;
                }
                notified.await;
            }
        };
        tokio::time::timeout(within, wait).await.is_ok()
    }
}

/// Run the consumer: connect a NATS subscriber from `NATS_URL`, subscribe to
/// [`DOC_INDEXED_SUBJECT`], and record each event into `registry`.
///
/// Best-effort and self-contained: if `NATS_URL` is unset or the bus is
/// unreachable it logs and returns, never breaking the gateway. Spawn once at
/// startup (see `main.rs`). The producer publishes via core NATS (not
/// JetStream), so a core subscription with at-most-once delivery is the right
/// fit — a missed `indexed` signal only costs an [`await_ready`] timeout, after
/// which the caller proceeds with the context it already has.
pub async fn run(registry: DocReadyRegistry) {
    let Ok(url) = std::env::var("NATS_URL") else {
        info!("NATS_URL unset; doc-indexed readiness consumer disabled");
        return;
    };
    let client = match async_nats::connect(&url).await {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "doc-indexed consumer: NATS connect failed; disabled");
            return;
        }
    };
    let mut sub = match client.subscribe(DOC_INDEXED_SUBJECT.to_owned()).await {
        Ok(s) => s,
        Err(e) => {
            warn!(error = %e, "doc-indexed consumer: subscribe failed; disabled");
            return;
        }
    };
    info!(
        subject = DOC_INDEXED_SUBJECT,
        "doc-indexed readiness consumer started"
    );
    use futures::StreamExt as _;
    while let Some(msg) = sub.next().await {
        match parse_indexed(&msg.payload) {
            Some(ev) => {
                registry.mark_indexed(&ev.org_id, &ev.document_id);
                debug!(
                    org_id = %ev.org_id,
                    document_id = %ev.document_id,
                    title = %ev.title,
                    "doc indexed; durable-retrieval readiness recorded"
                );
            }
            None => {
                warn!(subject = %msg.subject, "doc-indexed consumer: undecodable/empty event")
            }
        }
    }
    info!("doc-indexed readiness consumer stopped (subscription closed)");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(json: serde_json::Value) -> Vec<u8> {
        serde_json::to_vec(&json).expect("valid json")
    }

    #[test]
    fn parse_accepts_producer_shape_and_ignores_extra_fields() {
        let ev = parse_indexed(&payload(serde_json::json!({
            "document_id": "doc-1",
            "org_id": "org-9",
            "title": "Hello",
            "embedding_model": "text-embedding-3-large",
            "embedding_provider": "azure",
            "idempotency_key": "doc.indexed-abc",
        })))
        .expect("well-formed event must parse");
        assert_eq!(ev.document_id, "doc-1");
        assert_eq!(ev.org_id, "org-9");
        assert_eq!(ev.title, "Hello");
    }

    #[test]
    fn parse_rejects_missing_identity_and_garbage() {
        // Missing document_id.
        assert!(parse_indexed(&payload(serde_json::json!({ "org_id": "o" }))).is_none());
        // Missing org_id.
        assert!(parse_indexed(&payload(serde_json::json!({ "document_id": "d" }))).is_none());
        // Empty strings are not an identity.
        assert!(parse_indexed(&payload(
            serde_json::json!({ "document_id": "", "org_id": "o" })
        ))
        .is_none());
        // Not JSON at all.
        assert!(parse_indexed(b"not json").is_none());
    }

    #[test]
    fn mark_then_is_ready_is_true_and_org_scoped() {
        let reg = DocReadyRegistry::new();
        assert!(!reg.is_ready("org-a", "doc-1"));
        reg.mark_indexed("org-a", "doc-1");
        assert!(reg.is_ready("org-a", "doc-1"));
        // Tenant isolation: same document id, different org, must NOT be ready.
        assert!(!reg.is_ready("org-b", "doc-1"));
    }

    #[tokio::test]
    async fn await_ready_fast_path_when_already_indexed() {
        let reg = DocReadyRegistry::new();
        reg.mark_indexed("o", "d");
        assert!(
            reg.await_ready("o", "d", Duration::from_secs(5)).await,
            "already-indexed doc must resolve immediately"
        );
    }

    #[tokio::test]
    async fn await_ready_wakes_on_concurrent_mark() {
        let reg = DocReadyRegistry::new();
        let marker = reg.clone();
        // Scheduled on the same current-thread runtime; runs cooperatively once
        // await_ready arms its notification and yields.
        let handle = tokio::spawn(async move {
            marker.mark_indexed("o", "d");
        });
        assert!(
            reg.await_ready("o", "d", Duration::from_secs(5)).await,
            "a mark arriving while awaiting must wake the waiter"
        );
        handle.await.expect("marker task");
    }

    #[tokio::test]
    async fn await_ready_times_out_when_never_indexed() {
        let reg = DocReadyRegistry::new();
        assert!(
            !reg.await_ready("o", "missing", Duration::from_millis(50))
                .await,
            "a doc that never signals must time out to false"
        );
    }

    #[test]
    fn eviction_drops_oldest_at_capacity() {
        let reg = DocReadyRegistry::with_capacity(2);
        reg.mark_indexed("o", "d1");
        reg.mark_indexed("o", "d2");
        reg.mark_indexed("o", "d3"); // evicts d1
        assert!(!reg.is_ready("o", "d1"), "oldest entry must be evicted");
        assert!(reg.is_ready("o", "d2"));
        assert!(reg.is_ready("o", "d3"));
    }

    #[test]
    fn duplicate_mark_does_not_consume_extra_capacity() {
        let reg = DocReadyRegistry::with_capacity(2);
        reg.mark_indexed("o", "d1");
        reg.mark_indexed("o", "d1"); // duplicate — must not push a 2nd ring slot
        reg.mark_indexed("o", "d2");
        // If the duplicate had consumed a slot, d1 would have been evicted here.
        reg.mark_indexed("o", "d2"); // duplicate of d2
        assert!(reg.is_ready("o", "d1"), "duplicate marks must not evict d1");
        assert!(reg.is_ready("o", "d2"));
    }
}
