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

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_nats::jetstream;
use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

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

/// Longest event id billing-core accepts (`MaxUsageEventIDLength`).
pub const MAX_EVENT_ID_BYTES: usize = 128;

/// Why a keyed event id was refused. Callers log the variant, never the id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageIdError {
    TooLong,
    BadCharacters,
}

/// The billing-core event id rule: `^[A-Za-z0-9][A-Za-z0-9._:-]*$`, at most 128 bytes.
pub fn validate_event_id(event_id: &str) -> Result<(), UsageIdError> {
    if event_id.len() > MAX_EVENT_ID_BYTES {
        return Err(UsageIdError::TooLong);
    }
    let mut chars = event_id.chars();
    let first_ok = chars.next().is_some_and(|c| c.is_ascii_alphanumeric());
    if !first_ok || !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-'))
    {
        return Err(UsageIdError::BadCharacters);
    }
    Ok(())
}

impl UsageEvent {
    /// WS6.C-e: an event whose id is `quarry:<part>:<part>:<metric>`, for usage that has no
    /// single request (a run segment, a crawl page). Each part is a closed identifier (a run
    /// id, a segment label, a short hash). The id is checked against billing-core's rule, so a
    /// malformed part is refused rather than sent.
    pub fn keyed(
        parts: &[&str],
        org_id: impl Into<String>,
        metric: impl Into<String>,
        quantity: f64,
        metadata: serde_json::Value,
    ) -> Result<Self, UsageIdError> {
        let metric = metric.into();
        let mut event_id = String::from("quarry");
        for part in parts {
            event_id.push(':');
            event_id.push_str(part);
        }
        event_id.push(':');
        event_id.push_str(&metric);
        validate_event_id(&event_id)?;
        Ok(Self {
            event_id,
            org_id: org_id.into(),
            metric,
            quantity,
            source: "quarry-edge".to_string(),
            occurred_at: Utc::now(),
            metadata,
        })
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
    /// WS6.C-e: browser seconds of an agent run segment (whole seconds).
    pub const BROWSER_SECOND_AGENT: &str = "quarry.browser.second.agent";
    /// WS6.C-e: browser seconds of a person's manual session segment (whole seconds).
    pub const BROWSER_SECOND_INTERACTIVE: &str = "quarry.browser.second.interactive";
    /// WS6.C-e: one agent run segment.
    pub const AGENT_RUN: &str = "quarry.agent.run";
    /// WS6.C-e: completed agent steps in a run segment.
    pub const AGENT_STEP: &str = "quarry.agent.step";
    /// WS6.C-e: one crawl page the orchestrator fetched (keyed by run and URL hash).
    pub const CRAWL_PAGE: &str = "quarry.crawl.page";
    /// WS6.C-e: URLs fetched or extracted by `/v1/extract`.
    pub const EXTRACT_URL: &str = "quarry.extract.url";
    /// WS6.C-e: one `/v1/map` call.
    pub const MAP_CALL: &str = "quarry.map.call";
    /// The seven metrics that `QUARRY_EDGE__USAGE_BROWSER_METRICS` governs. The five
    /// metrics above are not in this list: the switch never drops them.
    pub const BROWSER_METRICS: [&str; 7] = [
        BROWSER_SECOND_AGENT,
        BROWSER_SECOND_INTERACTIVE,
        AGENT_RUN,
        AGENT_STEP,
        CRAWL_PAGE,
        EXTRACT_URL,
        MAP_CALL,
    ];
}

/// Pluggable usage emitter. Production wires `NatsUsageMeter`. Tests +
/// dev wire `NoopUsageMeter` so no broker is required.
#[async_trait]
pub trait UsageMeter: Send + Sync {
    async fn meter(&self, event: UsageEvent);

    /// WS6.C-e: wait, at most `deadline`, for the publishes this meter has started. The
    /// default meter has none to wait for.
    async fn flush(&self, _deadline: std::time::Duration) {}

    /// WS6.C-e: the mode of the browser metrics. The default does not gate them and reports
    /// `Off`; the call sites then keep the legacy rules for the existing metrics.
    fn browser_metrics_mode(&self) -> BrowserMetricsMode {
        BrowserMetricsMode::Off
    }
}

/// How the seven browser and crawl metrics are emitted (`QUARRY_EDGE__USAGE_BROWSER_METRICS`).
/// `Off` drops them; `Shadow` counts them without publishing; `On` publishes them and applies
/// the corrected rules to the existing metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BrowserMetricsMode {
    #[default]
    Off,
    Shadow,
    On,
}

impl BrowserMetricsMode {
    /// `off` (also the empty value), `shadow` or `on`. Anything else is `None`.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "" | "off" => Some(Self::Off),
            "shadow" => Some(Self::Shadow),
            "on" => Some(Self::On),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Shadow => "shadow",
            Self::On => "on",
        }
    }
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
    /// Publishes started and not yet acknowledged, so `flush` can wait for them.
    inflight: Arc<PublishTracker>,
}

impl NatsUsageMeter {
    pub fn from_bus(bus: &NatsEventBus) -> Self {
        // `NatsEventBus` exposes a `client()` accessor; build a fresh
        // JetStream context off the same connection (Context is cheap —
        // just a wrapper around a Client reference).
        let js = jetstream::new(bus.client().clone());
        Self {
            js,
            inflight: Arc::new(PublishTracker::default()),
        }
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
        let guard = self.inflight.begin();
        tokio::spawn(async move {
            // Held until this publish ends, on every path below.
            let _guard = guard;
            let subject = Self::subject_for(&event.metric);
            let bytes = match serde_json::to_vec(&event) {
                Ok(b) => Bytes::from(b),
                Err(e) => {
                    // zdr-log-ok: a serde serialisation error for the closed UsageEvent struct (metric, quantity, ids), which carries no caller content; spawned task, no request span
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
                    // zdr-log-ok: a NATS client error plus the usage subject; the usage event is a count (metric, quantity), never page content; spawned task, no request span
                    tracing::warn!(
                        error = %e,
                        %subject,
                        "usage event publish failed"
                    );
                }
            }
        });
    }

    async fn flush(&self, deadline: std::time::Duration) {
        let _drained = self.inflight.wait_idle(deadline).await;
    }
}

/// Counts the publishes a meter has started and not yet finished, so `flush` can wait for them.
#[derive(Debug, Default)]
pub(crate) struct PublishTracker {
    in_flight: AtomicUsize,
    idle: Notify,
}

impl PublishTracker {
    /// Marks one publish as started. It ends when the returned guard is dropped.
    pub(crate) fn begin(self: &Arc<Self>) -> PublishGuard {
        self.in_flight.fetch_add(1, Ordering::SeqCst);
        PublishGuard {
            tracker: Arc::clone(self),
        }
    }

    /// Waits until no publish is in flight or `deadline` passes. Returns whether it drained.
    pub(crate) async fn wait_idle(&self, deadline: std::time::Duration) -> bool {
        let until = tokio::time::Instant::now() + deadline;
        loop {
            // Register for the wake-up before reading the count, so a publish that ends
            // in between cannot be missed.
            let notified = self.idle.notified();
            if self.in_flight.load(Ordering::SeqCst) == 0 {
                return true;
            }
            if tokio::time::timeout_at(until, notified).await.is_err() {
                return self.in_flight.load(Ordering::SeqCst) == 0;
            }
        }
    }
}

/// One started publish. Dropping it marks the publish as finished.
pub(crate) struct PublishGuard {
    tracker: Arc<PublishTracker>,
}

impl Drop for PublishGuard {
    fn drop(&mut self) {
        if self.tracker.in_flight.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.tracker.idle.notify_waiters();
        }
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
        let evt = UsageEvent::new("req_abc", "org_xyz", metrics::SCRAPE_PAGE, 1.0, json!({}));
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
        assert!(
            s.contains("T") && s.contains("Z"),
            "occurred_at must be RFC3339 UTC"
        );
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
            "r",
            "o",
            metrics::SCRAPE_PAGE,
            1.0,
            json!({}),
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
mod metering_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keyed_id_joins_parts_and_metric() {
        let evt = UsageEvent::keyed(
            &["run_abc", "s0"],
            "org_a",
            metrics::AGENT_RUN,
            1.0,
            json!({}),
        )
        .unwrap();
        assert_eq!(evt.event_id, "quarry:run_abc:s0:quarry.agent.run");
        assert_eq!(evt.source, "quarry-edge");
    }

    #[test]
    fn keyed_id_refuses_bad_characters_and_long_ids() {
        assert_eq!(
            validate_event_id("quarry:a b:x"),
            Err(UsageIdError::BadCharacters)
        );
        assert_eq!(
            validate_event_id(":leading"),
            Err(UsageIdError::BadCharacters)
        );
        let long = "a".repeat(MAX_EVENT_ID_BYTES + 1);
        assert_eq!(validate_event_id(&long), Err(UsageIdError::TooLong));
        assert!(validate_event_id(&"a".repeat(MAX_EVENT_ID_BYTES)).is_ok());
        assert!(
            UsageEvent::keyed(&["run_x", "a b"], "o", metrics::AGENT_RUN, 1.0, json!({})).is_err()
        );
    }

    #[test]
    fn browser_metric_list_is_the_seven_new_names() {
        assert_eq!(metrics::BROWSER_METRICS.len(), 7);
        assert!(!metrics::BROWSER_METRICS.contains(&metrics::SCRAPE_PAGE));
        assert!(metrics::BROWSER_METRICS.contains(&metrics::CRAWL_PAGE));
    }

    #[test]
    fn mode_parses_the_closed_set_only() {
        assert_eq!(BrowserMetricsMode::parse(""), Some(BrowserMetricsMode::Off));
        assert_eq!(
            BrowserMetricsMode::parse(" shadow "),
            Some(BrowserMetricsMode::Shadow)
        );
        assert_eq!(
            BrowserMetricsMode::parse("on"),
            Some(BrowserMetricsMode::On)
        );
        assert_eq!(BrowserMetricsMode::parse("ON"), None);
        assert_eq!(BrowserMetricsMode::parse("yes"), None);
    }

    #[tokio::test]
    async fn flush_waits_for_inflight_publish() {
        let tracker = Arc::new(PublishTracker::default());
        let guard = tracker.begin();
        let started = tokio::time::Instant::now();
        let release = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(60)).await;
            drop(guard);
        });
        assert!(tracker.wait_idle(std::time::Duration::from_secs(5)).await);
        assert!(started.elapsed() >= std::time::Duration::from_millis(50));
        release.await.unwrap();
    }

    #[tokio::test]
    async fn flush_gives_up_at_its_deadline() {
        let tracker = Arc::new(PublishTracker::default());
        let _held = tracker.begin();
        assert!(
            !tracker
                .wait_idle(std::time::Duration::from_millis(30))
                .await
        );
    }

    #[tokio::test]
    async fn flush_returns_at_once_when_idle() {
        let tracker = PublishTracker::default();
        assert!(tracker.wait_idle(std::time::Duration::from_millis(1)).await);
    }
}
