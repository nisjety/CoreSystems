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
//!
//! ## Transport (U37)
//!
//! With `QUARRY_EDGE__USAGE_BUS_URL` set, the meter connects to the Control Plane NATS that
//! billing-core subscribes (stream `CONTROL_PLANE_EVENTS` covers `usage.>`) and each publish
//! carries `Nats-Msg-Id = event_id`, waits for the JetStream ack and is retried
//! (`USAGE_PUBLISH_ATTEMPTS`): at-least-once, deduplicated by billing-core on `event_id`.
//! Unset keeps today's behaviour (the Quarry bus, ack not awaited).

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
    /// WS16.S6: one AI format (`json`, `summary`, `question`, `highlights`) that finished `ok`,
    /// on `/v1/scrape` or per source of `/v1/extract`. Governed by its own switch,
    /// `QUARRY_EDGE__USAGE_LLM_METRICS`, not by the browser metrics switch below.
    pub const EXTRACT_LLM: &str = "quarry.extract.llm";
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

    /// WS16.S6: the mode of `quarry.extract.llm`. The default reports `Off`, so a meter that
    /// does not gate the metric is never handed one.
    fn llm_metrics_mode(&self) -> LlmMetricsMode {
        LlmMetricsMode::Off
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

/// How `quarry.extract.llm` is emitted (`QUARRY_EDGE__USAGE_LLM_METRICS`, WS16.S6). A switch of
/// its own, separate from [`BrowserMetricsMode`]: `Off` emits nothing; `Shadow` counts the
/// events without publishing; `On` publishes them through the usage meter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LlmMetricsMode {
    #[default]
    Off,
    Shadow,
    On,
}

impl LlmMetricsMode {
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
    /// U37: when set, each publish carries `Nats-Msg-Id = event_id`, waits for the JetStream
    /// ack and retries a failed publish (at-least-once). Only the usage bus meter sets it: the
    /// Quarry bus has no stream over `usage.>`, so an ack there never comes (today's behaviour
    /// drops the ack future).
    await_ack: bool,
}

/// U37: `QUARRY_EDGE__USAGE_BUS_URL`, the Control Plane NATS that billing-core subscribes
/// (`usage.>`, stream `CONTROL_PLANE_EVENTS`). Unset or blank keeps today's behaviour: the meter
/// rides the Quarry bus (`QUARRY_EDGE__NATS_URL`).
pub const ENV_USAGE_BUS_URL: &str = "QUARRY_EDGE__USAGE_BUS_URL";
/// The runtime identity on the Control NATS (Control uses one user and password per service).
pub const ENV_USAGE_BUS_USER: &str = "QUARRY_EDGE__USAGE_BUS_USER";
pub const ENV_USAGE_BUS_PASSWORD: &str = "QUARRY_EDGE__USAGE_BUS_PASSWORD";
/// Inbox prefix on the Control NATS. Control grants each service its own `_INBOX.<NAME>.>`
/// subscribe permission (billing-core uses `_INBOX.BILLING_CONTROL`), and the JetStream ack
/// arrives on the inbox, so the prefix must match the grant.
pub const USAGE_BUS_INBOX_PREFIX: &str = "_INBOX.QUARRY_CONTROL";
/// The JetStream de-duplication header. Set to the event id, so a retried publish inside the
/// stream's duplicate window is stored once; billing-core also dedupes on the event id.
pub const MSG_ID_HEADER: &str = "Nats-Msg-Id";
/// Publish attempts on the usage bus before the event is given up (and logged).
pub const USAGE_PUBLISH_ATTEMPTS: u32 = 3;
/// Back-off between attempts, multiplied by the attempt number.
pub const USAGE_PUBLISH_BACKOFF: std::time::Duration = std::time::Duration::from_millis(250);

/// Why the usage bus settings were refused. Carries no value, so it is safe to log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageBusConfigError {
    /// Only one of the user and the password is set.
    PartialCredentials,
}

impl std::fmt::Display for UsageBusConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PartialCredentials => write!(
                f,
                "{ENV_USAGE_BUS_USER} and {ENV_USAGE_BUS_PASSWORD} must be set together"
            ),
        }
    }
}

impl std::error::Error for UsageBusConfigError {}

/// U37: where the usage meter publishes when the Control bus is configured.
#[derive(Clone, PartialEq, Eq)]
pub struct UsageBusConfig {
    pub url: String,
    pub user: Option<String>,
    pub password: Option<String>,
}

impl std::fmt::Debug for UsageBusConfig {
    // The password never reaches a log line; the URL is shown without any userinfo.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UsageBusConfig")
            .field("url", &redact_userinfo(&self.url))
            .field("user", &self.user)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

/// Drops `user:pass@` from a URL so it can be logged.
pub fn redact_userinfo(url: &str) -> String {
    match (url.find("://"), url.rfind('@')) {
        (Some(scheme_end), Some(at)) if at > scheme_end => {
            format!("{}://{}", &url[..scheme_end], &url[at + 1..])
        }
        _ => url.to_string(),
    }
}

impl UsageBusConfig {
    /// Reads the settings through `lookup` (the process environment in production). `Ok(None)`
    /// when the URL is unset or blank: today's behaviour.
    pub fn from_lookup(
        lookup: impl Fn(&str) -> Option<String>,
    ) -> Result<Option<Self>, UsageBusConfigError> {
        let non_empty = |key: &str| {
            lookup(key)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let Some(url) = non_empty(ENV_USAGE_BUS_URL) else {
            return Ok(None);
        };
        let user = non_empty(ENV_USAGE_BUS_USER);
        let password = lookup(ENV_USAGE_BUS_PASSWORD).filter(|v| !v.is_empty());
        if user.is_some() != password.is_some() {
            return Err(UsageBusConfigError::PartialCredentials);
        }
        Ok(Some(Self {
            url,
            user,
            password,
        }))
    }

    pub fn from_env() -> Result<Option<Self>, UsageBusConfigError> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }
}

/// The headers of one usage publish on the usage bus: the JetStream de-duplication id.
pub fn usage_publish_headers(event_id: &str) -> async_nats::HeaderMap {
    let mut headers = async_nats::HeaderMap::new();
    headers.insert(MSG_ID_HEADER, event_id);
    headers
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
            await_ack: false,
        }
    }

    /// U37: connect to the Control NATS that billing-core subscribes. No stream is created:
    /// `CONTROL_PLANE_EVENTS` (which covers `usage.>`) is provisioned on the Control side.
    pub async fn connect_usage_bus(
        config: &UsageBusConfig,
    ) -> Result<Self, async_nats::ConnectError> {
        let mut opts = async_nats::ConnectOptions::new()
            .name("quarry-edge-usage")
            .custom_inbox_prefix(USAGE_BUS_INBOX_PREFIX)
            .connection_timeout(std::time::Duration::from_secs(5));
        if let (Some(user), Some(password)) = (&config.user, &config.password) {
            opts = opts.user_and_password(user.clone(), password.clone());
        }
        let client = opts.connect(config.url.as_str()).await?;
        Ok(Self {
            js: jetstream::new(client),
            inflight: Arc::new(PublishTracker::default()),
            await_ack: true,
        })
    }

    /// One acknowledged publish: the message is sent with its de-duplication id and the call
    /// returns once the stream has stored it.
    async fn publish_acked(
        js: &jetstream::Context,
        subject: &str,
        event_id: &str,
        payload: Bytes,
    ) -> Result<(), jetstream::context::PublishError> {
        let ack = js
            .publish_with_headers(subject.to_string(), usage_publish_headers(event_id), payload)
            .await?;
        ack.await?;
        Ok(())
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
        let await_ack = self.await_ack;
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
            if await_ack {
                // U37: at-least-once on the usage bus. The guard above keeps `flush` waiting
                // through the retries.
                for attempt in 1..=USAGE_PUBLISH_ATTEMPTS {
                    match Self::publish_acked(&js, &subject, &event.event_id, bytes.clone())
                        .await
                    {
                        Ok(()) => {
                            tracing::debug!(
                                %subject,
                                metric = %event.metric,
                                quantity = event.quantity,
                                attempt,
                                "usage metered (acked)"
                            );
                            return;
                        }
                        Err(e) if attempt < USAGE_PUBLISH_ATTEMPTS => {
                            // zdr-log-ok: a NATS publish error plus the usage subject; the usage event is a count (metric, quantity), never page content; spawned task, no request span
                            tracing::debug!(
                                error = %e,
                                %subject,
                                attempt,
                                "usage publish not acked; retrying"
                            );
                            tokio::time::sleep(USAGE_PUBLISH_BACKOFF * attempt).await;
                        }
                        Err(e) => {
                            // zdr-log-ok: a NATS publish error plus the usage subject; the usage event is a count (metric, quantity), never page content; spawned task, no request span
                            tracing::warn!(
                                error = %e,
                                %subject,
                                attempts = USAGE_PUBLISH_ATTEMPTS,
                                "usage event publish not acked; given up"
                            );
                        }
                    }
                }
                return;
            }
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

#[cfg(test)]
mod usage_bus_tests {
    use super::*;
    use std::collections::HashMap;

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |key| map.get(key).cloned()
    }

    #[test]
    fn usage_bus_unset_or_blank_keeps_todays_behaviour() {
        assert_eq!(UsageBusConfig::from_lookup(lookup(&[])), Ok(None));
        assert_eq!(
            UsageBusConfig::from_lookup(lookup(&[(ENV_USAGE_BUS_URL, "  ")])),
            Ok(None)
        );
    }

    #[test]
    fn usage_bus_reads_url_and_credentials_together() {
        let config = UsageBusConfig::from_lookup(lookup(&[
            (ENV_USAGE_BUS_URL, "nats://controlplane-nats:4222"),
            (ENV_USAGE_BUS_USER, "quarry-edge-control"),
            (ENV_USAGE_BUS_PASSWORD, "pw-value"),
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(config.url, "nats://controlplane-nats:4222");
        assert_eq!(config.user.as_deref(), Some("quarry-edge-control"));
        assert_eq!(config.password.as_deref(), Some("pw-value"));

        let anonymous = UsageBusConfig::from_lookup(lookup(&[(
            ENV_USAGE_BUS_URL,
            "nats://controlplane-nats:4222",
        )]))
        .unwrap()
        .unwrap();
        assert!(anonymous.user.is_none() && anonymous.password.is_none());
    }

    #[test]
    fn usage_bus_refuses_half_credentials() {
        for pairs in [
            [
                (ENV_USAGE_BUS_URL, "nats://controlplane-nats:4222"),
                (ENV_USAGE_BUS_USER, "quarry-edge-control"),
            ],
            [
                (ENV_USAGE_BUS_URL, "nats://controlplane-nats:4222"),
                (ENV_USAGE_BUS_PASSWORD, "pw-value"),
            ],
        ] {
            assert_eq!(
                UsageBusConfig::from_lookup(lookup(&pairs)),
                Err(UsageBusConfigError::PartialCredentials)
            );
        }
    }

    #[test]
    fn usage_bus_debug_never_shows_the_password_or_userinfo() {
        let config = UsageBusConfig {
            url: "nats://someone:pw-in-url@controlplane-nats:4222".to_string(),
            user: Some("quarry-edge-control".to_string()),
            password: Some("pw-value".to_string()),
        };
        let shown = format!("{config:?}");
        assert!(!shown.contains("pw-value"), "{shown}");
        assert!(!shown.contains("pw-in-url"), "{shown}");
        assert!(shown.contains("controlplane-nats:4222"), "{shown}");
        assert_eq!(
            redact_userinfo("nats://controlplane-nats:4222"),
            "nats://controlplane-nats:4222"
        );
    }

    #[test]
    fn usage_publish_carries_the_event_id_as_the_dedup_header() {
        let event = UsageEvent::keyed(
            &["run_abc", "s0"],
            "org_a",
            metrics::AGENT_RUN,
            1.0,
            serde_json::json!({}),
        )
        .unwrap();
        let headers = usage_publish_headers(&event.event_id);
        assert_eq!(
            headers.get(MSG_ID_HEADER).map(|v| v.as_str()),
            Some("quarry:run_abc:s0:quarry.agent.run")
        );
    }

    #[test]
    fn every_quarry_metric_lands_under_billing_core_subscription() {
        // Pinned against billing-core usage_test.go (TestUsageSubjectAndQuarryMetricsArePinned):
        // the filter is `usage.>` and the seven browser metrics are the list billing-core maps.
        for metric in metrics::BROWSER_METRICS {
            let subject = NatsUsageMeter::subject_for(metric);
            assert!(subject.starts_with("usage.quarry."), "{subject}");
        }
        assert_eq!(
            metrics::BROWSER_METRICS,
            [
                "quarry.browser.second.agent",
                "quarry.browser.second.interactive",
                "quarry.agent.run",
                "quarry.agent.step",
                "quarry.crawl.page",
                "quarry.extract.url",
                "quarry.map.call",
            ]
        );
    }

    #[test]
    fn retries_are_bounded_and_back_off() {
        assert!(USAGE_PUBLISH_ATTEMPTS >= 2);
        let total: std::time::Duration = (1..USAGE_PUBLISH_ATTEMPTS)
            .map(|attempt| USAGE_PUBLISH_BACKOFF * attempt)
            .sum();
        // The shutdown flush waits 5 s; the back-off alone must leave room for the acks.
        assert!(total < std::time::Duration::from_secs(2), "{total:?}");
    }
}
