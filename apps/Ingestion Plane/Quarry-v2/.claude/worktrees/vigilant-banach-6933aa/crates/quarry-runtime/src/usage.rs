//! Usage metering — emits billable-event records that Control Plane's
//! `billing-core` consumes off NATS.
//!
//! P3 / cluster #billing.
//!
//! ## Contract
//!
//! billing-core subscribes to JetStream subject filter `usage.>` (stream
//! `CONTROL_PLANE_EVENTS`). The payload shape is fixed at
//! `apps/Control Plane/billing-core/internal/billing/types.go:43`:
//!
//! ```jsonc
//! {
//!   "event_id":   "quarry:<request_id>:<metric>", // primary idempotency key
//!   "org_id":     "org_...",
//!   "metric":     "quarry.scrape.page",           // free-form; downstream Lago code
//!   "quantity":   1.0,
//!   "source":     "quarry-edge",
//!   "occurred_at":"2026-05-19T12:34:56Z",          // RFC3339
//!   "metadata":   { "user_id": "...", ... }
//! }
//! ```
//!
//! billing-core dedupes on `event_id` (`billing_usage_dedup` table). We
//! shape the ID as `quarry:<request_id>:<metric>` so a request that
//! emits multiple metric kinds doesn't collide with itself.
//!
//! ## Best-effort
//!
//! Usage metering is **not** on the critical path — every meter call
//! `tokio::spawn`s the publish so the handler returns immediately. A
//! billing outage degrades cost accounting but never user-facing
//! latency. The HTTP path that already exists at billing-core
//! (`POST /api/v1/billing/orgs/:orgId/usage`) is the operator's
//! recovery surface if NATS replay isn't enough.

use std::sync::Arc;

use async_nats::jetstream;
use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::nats_event_bus::NatsEventBus;

/// Wire-shape canonical billing event. Field names + JSON casing MUST
/// match billing-core's `UsageEvent` Go struct exactly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageEvent {
    pub event_id: String,
    pub org_id: String,
    pub metric: String,
    pub quantity: f64,
    pub source: String,
    pub occurred_at: DateTime<Utc>,
    pub metadata: serde_json::Value,
}

impl UsageEvent {
    /// Build an event with the standard `quarry:<request_id>:<metric>`
    /// idempotency shape and `occurred_at = now`. Callers supply the
    /// org_id (verified JWT claim), metric, quantity, and metadata.
    pub fn new(
        request_id: impl Into<String>,
        org_id: impl Into<String>,
        metric: impl Into<String>,
        quantity: f64,
        metadata: serde_json::Value,
    ) -> Self {
        let request_id = request_id.into();
        let metric = metric.into();
        Self {
            event_id: format!("quarry:{request_id}:{metric}"),
            org_id: org_id.into(),
            metric,
            quantity,
            source: "quarry-edge".to_string(),
            occurred_at: Utc::now(),
            metadata,
        }
    }
}

/// Canonical metric strings. Free-form per billing-core's contract but
/// centralised here so producers don't drift. Update Lago billable
/// metric codes to match before shipping a new metric to production.
pub mod metrics {
    pub const SCRAPE_PAGE: &str = "quarry.scrape.page";
    pub const CRAWL_SEED: &str = "quarry.crawl.seed";
    pub const SEARCH_QUERY: &str = "quarry.search.query";
    pub const ANSWER_SYNTH: &str = "quarry.answer.synth";
    pub const BATCH_URL: &str = "quarry.batch.url";
}

/// Pluggable usage emitter. Production wires `NatsUsageMeter`. Tests +
/// dev wire `NoopUsageMeter` so no broker is required.
#[async_trait]
pub trait UsageMeter: Send + Sync {
    async fn meter(&self, event: UsageEvent);
}

/// No-op meter — silently discards every event. Used when NATS is not
/// configured or in test harnesses that don't want to assert metering.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopUsageMeter;

#[async_trait]
impl UsageMeter for NoopUsageMeter {
    async fn meter(&self, _event: UsageEvent) {}
}

/// JetStream-backed meter. Publishes to `usage.<metric>` so each metric
/// gets its own subject and consumers can subscribe granularly. billing-core
/// listens on `usage.>` wildcard so every event reaches it.
///
/// Constructed from an already-connected `NatsEventBus` so we reuse the
/// client (one TCP connection per process) — cheap to clone.
pub struct NatsUsageMeter {
    js: jetstream::Context,
}

impl NatsUsageMeter {
    pub fn from_bus(bus: &NatsEventBus) -> Self {
        // `NatsEventBus` exposes a `client()` accessor; build a fresh
        // JetStream context off the same connection (Context is cheap —
        // just a wrapper around a Client reference).
        let js = jetstream::new(bus.client().clone());
        Self { js }
    }

    /// Subject derivation. Each metric gets its own subject under the
    /// `usage.>` root so subscribers (Lago bridge, dashboards) can
    /// filter precisely. Examples:
    /// - `quarry.scrape.page` → `usage.quarry.scrape.page`
    /// - `quarry.search.query` → `usage.quarry.search.query`
    fn subject_for(metric: &str) -> String {
        format!("usage.{metric}")
    }
}

#[async_trait]
impl UsageMeter for NatsUsageMeter {
    async fn meter(&self, event: UsageEvent) {
        // Spawn so the handler that calls `.meter(...).await` returns
        // immediately. Billing outage / broker latency never bottlenecks
        // user-facing request paths.
        let js = self.js.clone();
        tokio::spawn(async move {
            let subject = Self::subject_for(&event.metric);
            let bytes = match serde_json::to_vec(&event) {
                Ok(b) => Bytes::from(b),
                Err(e) => {
                    tracing::warn!(error = %e, "usage event serialize failed");
                    return;
                }
            };
            match js.publish(subject.clone(), bytes).await {
                Ok(_ack) => {
                    // Drop the ack future intentionally — at-most-once is
                    // fine for usage. billing-core dedupes on event_id
                    // so a duplicate from JetStream redelivery is safe.
                    tracing::debug!(
                        %subject,
                        metric = %event.metric,
                        quantity = event.quantity,
                        "usage metered"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        %subject,
                        "usage event publish failed"
                    );
                }
            }
        });
    }
}

/// Convenience erasure for `main.rs` wiring.
pub fn into_dyn(m: impl UsageMeter + 'static) -> Arc<dyn UsageMeter> {
    Arc::new(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex;

    #[test]
    fn event_id_format_is_quarry_request_metric() {
        let evt = UsageEvent::new(
            "req_abc",
            "org_xyz",
            metrics::SCRAPE_PAGE,
            1.0,
            json!({}),
        );
        assert_eq!(evt.event_id, "quarry:req_abc:quarry.scrape.page");
        assert_eq!(evt.org_id, "org_xyz");
        assert_eq!(evt.metric, "quarry.scrape.page");
        assert_eq!(evt.quantity, 1.0);
        assert_eq!(evt.source, "quarry-edge");
    }

    #[test]
    fn serializes_to_billing_core_wire_shape() {
        // Pin the field names exactly — Go struct uses snake_case JSON
        // tags. A drift here breaks billing without a compiler warning.
        let evt = UsageEvent::new(
            "req_1",
            "org_a",
            metrics::SEARCH_QUERY,
            1.0,
            json!({"user_id": "u1", "result_count": 7}),
        );
        let s = serde_json::to_string(&evt).unwrap();
        assert!(s.contains("\"event_id\":"));
        assert!(s.contains("\"org_id\":"));
        assert!(s.contains("\"metric\":"));
        assert!(s.contains("\"quantity\":"));
        assert!(s.contains("\"source\":"));
        assert!(s.contains("\"occurred_at\":"));
        assert!(s.contains("\"metadata\":"));
        // RFC3339 timestamp shape
        assert!(s.contains("T") && s.contains("Z"), "occurred_at must be RFC3339 UTC");
    }

    #[test]
    fn subject_derivation_matches_usage_wildcard() {
        // billing-core subscribes to `usage.>` — every metric must
        // land under that root.
        assert_eq!(
            NatsUsageMeter::subject_for(metrics::SCRAPE_PAGE),
            "usage.quarry.scrape.page"
        );
        assert_eq!(
            NatsUsageMeter::subject_for(metrics::CRAWL_SEED),
            "usage.quarry.crawl.seed"
        );
        // Custom metrics still land under `usage.>`.
        assert_eq!(
            NatsUsageMeter::subject_for("internal.benchmark"),
            "usage.internal.benchmark"
        );
    }

    /// Recording meter used by handler-level integration tests downstream.
    pub(crate) struct RecordingMeter {
        pub events: Mutex<Vec<UsageEvent>>,
    }

    impl RecordingMeter {
        pub fn new() -> Self {
            Self {
                events: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl UsageMeter for RecordingMeter {
        async fn meter(&self, event: UsageEvent) {
            self.events.lock().unwrap().push(event);
        }
    }

    #[tokio::test]
    async fn noop_meter_silently_discards() {
        let m = NoopUsageMeter;
        m.meter(UsageEvent::new(
            "r", "o", metrics::SCRAPE_PAGE, 1.0, json!({}),
        ))
        .await;
        // Just proving the call doesn't panic / hang. No assertion.
    }

    #[tokio::test]
    async fn recording_meter_captures_payload() {
        let m = RecordingMeter::new();
        m.meter(UsageEvent::new(
            "r1",
            "org_alpha",
            metrics::ANSWER_SYNTH,
            1.0,
            json!({"sources": 3}),
        ))
        .await;
        let events = m.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].metric, "quarry.answer.synth");
        assert_eq!(events[0].org_id, "org_alpha");
    }
}

#[cfg(test)]
pub(crate) use tests::RecordingMeter;
