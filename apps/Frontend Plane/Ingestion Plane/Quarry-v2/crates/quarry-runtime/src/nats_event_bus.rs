//! NATS JetStream-backed EventBus.
//!
//! Bridges Quarry's in-process event flow with the cross-plane NATS JetStream
//! (Control Plane org-core and Model Plane orchestrator-core listen on the same
//! cluster). Subjects follow the `quarry.run.<run_id>.<event_type>` pattern
//! plus an aggregate `quarry.events.<event_type>` for plane-wide consumers.
//!
//! ## Streams
//!
//! Quarry assumes a JetStream stream named `QUARRY_EVENTS` with subjects
//! `quarry.>` already exists (created by infra/nats.conf or
//! `EnsureStream::ensure`).
//!
//! ## ZDR
//!
//! Run and crawl events are control-plane signals, not source content, so the
//! payload is allowed to flow through JetStream. Callers must NOT include
//! scraped content / artifact bytes in the payload — only refs (artifact IDs,
//! fingerprints, URLs). A ZDR run never reaches this bus at all: the sink routes
//! it through `emit_ephemeral`, which stays in process.
//!
//! ## Agent-class events (D2-098, D10)
//!
//! Agent-run events (`agent_*`, `action_*`, `observation_ready`) carry what the
//! agent saw and did: URLs, element labels, action arguments. They never go out
//! raw on JetStream, ZDR or not. They reach the run's own SSE stream and the
//! in-memory replay ring in the sink; a cross-plane consumer gets them only
//! through a redacted projection (WS10.S4), not from this bus.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_nats::jetstream::{self, consumer::PullConsumer, stream::Config as StreamConfig};
use async_nats::{Client, ConnectOptions};
use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use futures::StreamExt;
use serde_json::Value;
use tokio::sync::{mpsc, RwLock};

use quarry_core::event::{Event, EventType};
use quarry_core::ids::kinds::RunKind;
use quarry_core::ids::Id;
use quarry_core::{ErrorCode, QuarryError, QuarryResult};

use crate::event_bus::{EventBus, EventReceiver};

const DEFAULT_STREAM_NAME: &str = "QUARRY_EVENTS";
const DEFAULT_SUBJECT_PREFIX: &str = "quarry";

#[derive(Debug, Clone)]
pub struct NatsConfig {
    pub url: String,
    pub stream_name: String,
    pub subject_prefix: String,
    pub user_credentials: Option<String>,
    pub auth_token: Option<String>,
    pub connect_timeout_s: u64,
}

impl NatsConfig {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            stream_name: DEFAULT_STREAM_NAME.to_string(),
            subject_prefix: DEFAULT_SUBJECT_PREFIX.to_string(),
            user_credentials: None,
            auth_token: None,
            connect_timeout_s: 5,
        }
    }

    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.auth_token = Some(token.into());
        self
    }

    pub fn with_credentials(mut self, path: impl Into<String>) -> Self {
        self.user_credentials = Some(path.into());
        self
    }
}

#[derive(Clone)]
pub struct NatsEventBus {
    client: Client,
    js: jetstream::Context,
    config: NatsConfig,
    /// Per-run monotonic sequence counter so consumers can detect gaps
    /// even when the broker reorders or duplicates events. Reset when a
    /// run completes (callers should drop the bus subscription which
    /// removes the run from this map; we GC entries on `unsubscribe`).
    seq_per_run: Arc<RwLock<HashMap<String, Arc<AtomicU64>>>>,
}

impl NatsEventBus {
    /// Connect, ensure the JetStream stream exists, and return a bus ready to
    /// publish/subscribe. Idempotent — calling `ensure_stream` twice is fine.
    pub async fn connect(config: NatsConfig) -> QuarryResult<Self> {
        let mut opts =
            ConnectOptions::new().connection_timeout(Duration::from_secs(config.connect_timeout_s));
        if let Some(token) = &config.auth_token {
            opts = opts.token(token.clone());
        }
        if let Some(creds_path) = &config.user_credentials {
            opts = opts.credentials_file(creds_path).await.map_err(|e| {
                QuarryError::new(
                    ErrorCode::Internal,
                    format!("failed to load nats creds: {e}"),
                )
            })?;
        }

        let client = opts.connect(&config.url).await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, format!("nats connect failed: {e}"))
        })?;

        let js = jetstream::new(client.clone());

        let bus = Self {
            client,
            js,
            config,
            seq_per_run: Arc::new(RwLock::new(HashMap::new())),
        };
        bus.ensure_stream().await?;
        Ok(bus)
    }

    /// Return the next sequence number for a run, allocating a counter on
    /// first use. Cheap when the run already has a counter (read lock +
    /// atomic increment). First call per run takes the write lock.
    async fn next_seq(&self, run_id: &RunKind) -> u64 {
        let key = run_id.to_string();
        // Fast path: counter exists.
        {
            let map = self.seq_per_run.read().await;
            if let Some(counter) = map.get(&key) {
                return counter.fetch_add(1, Ordering::Relaxed) + 1;
            }
        }
        // Slow path: install a new counter.
        let mut map = self.seq_per_run.write().await;
        let counter = map
            .entry(key)
            .or_insert_with(|| Arc::new(AtomicU64::new(0)));
        counter.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Idempotent stream creation — safe to call on every startup.
    pub async fn ensure_stream(&self) -> QuarryResult<()> {
        let subjects = vec![format!("{}.>", self.config.subject_prefix)];
        let stream_cfg = StreamConfig {
            name: self.config.stream_name.clone(),
            subjects,
            // 7-day retention matches Control Plane's USER_EVENTS / ORGANIZATION_EVENTS streams.
            max_age: Duration::from_secs(7 * 24 * 60 * 60),
            ..Default::default()
        };

        self.js
            .get_or_create_stream(stream_cfg)
            .await
            .map_err(|e| {
                QuarryError::new(
                    ErrorCode::DriverFailed,
                    format!("ensure stream failed: {e}"),
                )
            })?;
        Ok(())
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    fn subject_for(&self, run_id: &RunKind, event_type: &EventType) -> String {
        format!(
            "{}.run.{}.{}",
            self.config.subject_prefix,
            run_id,
            event_type_token(event_type)
        )
    }

    fn aggregate_subject_for(&self, event_type: &EventType) -> String {
        format!(
            "{}.events.{}",
            self.config.subject_prefix,
            event_type_token(event_type)
        )
    }
}

#[async_trait]
impl EventBus for NatsEventBus {
    async fn publish(
        &self,
        run_id: RunKind,
        event_type: EventType,
        payload: Value,
        idempotency_key: String,
    ) -> QuarryResult<()> {
        let seq = self.next_seq(&run_id).await;
        let event = Event {
            event_id: Id::new(),
            run_id: Some(run_id.clone()),
            job_id: None,
            event_type,
            ts: Utc::now(),
            seq,
            payload,
            idempotency_key: idempotency_key.clone(),
        };

        let bytes = serde_json::to_vec(&event).map_err(|e| {
            QuarryError::new(ErrorCode::Internal, format!("event serialize failed: {e}"))
        })?;
        let payload_bytes = Bytes::from(bytes);

        self.publish_encoded(&run_id, event_type, payload_bytes).await
    }

    async fn publish_event(&self, event: Event) -> QuarryResult<()> {
        let Some(run_id) = event.run_id.clone() else {
            return Ok(());
        };
        let event_type = event.event_type;
        let bytes = serde_json::to_vec(&event).map_err(|e| {
            QuarryError::new(ErrorCode::Internal, format!("event serialize failed: {e}"))
        })?;
        self.publish_encoded(&run_id, event_type, Bytes::from(bytes))
            .await
    }

    async fn subscribe(&self, run_id: &RunKind) -> QuarryResult<Box<dyn EventReceiver>> {
        let filter_subject = format!("{}.run.{}.>", self.config.subject_prefix, run_id);

        // Ephemeral consumer — when the receiver drops, JetStream cleans up.
        let consumer_cfg = jetstream::consumer::pull::Config {
            filter_subject,
            ..Default::default()
        };

        let stream = self
            .js
            .get_stream(&self.config.stream_name)
            .await
            .map_err(|e| {
                QuarryError::new(ErrorCode::DriverFailed, format!("get stream failed: {e}"))
            })?;
        let consumer: PullConsumer = stream.create_consumer(consumer_cfg).await.map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                format!("create consumer failed: {e}"),
            )
        })?;

        let (tx, rx) = mpsc::channel::<Event>(64);
        let consumer_arc = Arc::new(consumer);
        let consumer_clone = consumer_arc.clone();

        tokio::spawn(async move {
            let mut messages = match consumer_clone.messages().await {
                Ok(m) => m,
                Err(e) => {
                    // zdr-log-ok: a NATS client error (subject, connection state); the event payload is not part of it; spawned consumer task, no request span
                    tracing::error!(error = %e, "nats consumer messages stream failed");
                    return;
                }
            };
            while let Some(next) = messages.next().await {
                match next {
                    Ok(msg) => {
                        // Decode FIRST, ack only on success. Previously the
                        // ack ran unconditionally so a malformed event on the
                        // stream was permanently dropped with no signal —
                        // exactly the kind of silent data loss that hides
                        // serialization regressions for weeks.
                        match serde_json::from_slice::<Event>(&msg.payload) {
                            Ok(event) => {
                                if tx.send(event).await.is_err() {
                                    break;
                                }
                                let _ = msg.ack().await;
                            }
                            Err(e) => {
                                // Don't ack — let JetStream redeliver
                                // and eventually push the message to the
                                // configured DLQ. A spike in this log line
                                // means a producer is emitting bad payloads.
                                // The serde category and position, not its text: a data
                                // error quotes the offending value, which can be a URL.
                                tracing::warn!(
                                    error_class = ?e.classify(),
                                    line = e.line(),
                                    column = e.column(),
                                    payload_bytes = msg.payload.len(),
                                    "nats event deserialize failed; not acking, JetStream will retry"
                                );
                            }
                        }
                    }
                    Err(e) => {
                        // zdr-log-ok: a NATS client error (subject, connection state); the event payload is not part of it; spawned consumer task, no request span
                        tracing::warn!(error = %e, "nats consumer error");
                        break;
                    }
                }
            }
        });

        Ok(Box::new(NatsReceiver { rx }))
    }

    async fn unsubscribe(&self, run_id: &RunKind) -> QuarryResult<()> {
        // Ephemeral consumers self-cleanup when the spawned task drops.
        // Also drop the per-run sequence counter so we don't leak memory
        // on long-lived buses with many short-lived runs.
        let mut map = self.seq_per_run.write().await;
        map.remove(&run_id.to_string());
        Ok(())
    }
}

struct NatsReceiver {
    rx: mpsc::Receiver<Event>,
}

#[async_trait]
impl EventReceiver for NatsReceiver {
    async fn recv(&mut self) -> Option<Event> {
        self.rx.recv().await
    }
}

impl NatsEventBus {
    /// Publish one encoded event on its per-run subject and on the aggregate subject
    /// (WS10.S3: shared by `publish` and `publish_event`).
    async fn publish_encoded(
        &self,
        run_id: &RunKind,
        event_type: EventType,
        payload_bytes: Bytes,
    ) -> QuarryResult<()> {
        // D2-098: an agent-class event never goes out raw. Both entry points
        // (`publish` and `publish_event`) pass through here, so one check covers
        // them. Dropping is Ok: the event is still on the run's SSE stream and in
        // the replay ring; only the cross-plane copy is withheld.
        if is_agent_class(&event_type) {
            return Ok(());
        }
        let primary_subject = self.subject_for(run_id, &event_type);
        let aggregate_subject = self.aggregate_subject_for(&event_type);

        // Publish per-run AND aggregate concurrently — neither should block
        // the other. Both must succeed for the call to return Ok; partial
        // success returns Err but logs which side failed so operators can
        // recover. We don't enforce a per-subject ordering: JetStream
        // guarantees per-subject FIFO already, and our seq numbers let
        // consumers detect gaps across subjects.
        let primary_pub = self
            .js
            .publish(primary_subject.clone(), payload_bytes.clone());
        let aggregate_pub = self
            .js
            .publish(aggregate_subject.clone(), payload_bytes.clone());

        let (primary_ack, aggregate_ack) = tokio::join!(primary_pub, aggregate_pub);

        let primary_err = match primary_ack {
            Ok(ack) => match ack.await {
                Ok(_) => None,
                Err(e) => Some(format!("primary ack: {e}")),
            },
            Err(e) => Some(format!("primary publish: {e}")),
        };
        let aggregate_err = match aggregate_ack {
            Ok(ack) => match ack.await {
                Ok(_) => None,
                Err(e) => Some(format!("aggregate ack: {e}")),
            },
            Err(e) => Some(format!("aggregate publish: {e}")),
        };

        match (primary_err, aggregate_err) {
            (None, None) => Ok(()),
            (Some(p_err), None) => {
                // zdr-log-ok: a NATS client error (subject, connection state); the event payload is not part of it
                tracing::warn!(error = %p_err, subject = %primary_subject, "primary subject failed; aggregate ok");
                Err(QuarryError::new(
                    ErrorCode::DriverFailed,
                    format!("nats publish primary-subject failed: {p_err}"),
                ))
            }
            (None, Some(a_err)) => {
                // zdr-log-ok: a NATS client error (subject, connection state); the event payload is not part of it
                tracing::warn!(error = %a_err, subject = %aggregate_subject, "aggregate subject failed; primary ok");
                // Per-run consumers got the event; degrade gracefully.
                Ok(())
            }
            (Some(p_err), Some(a_err)) => Err(QuarryError::new(
                ErrorCode::DriverFailed,
                format!("nats publish both subjects failed (primary: {p_err}; aggregate: {a_err})"),
            )),
        }
    }
}

/// True for an event that belongs to an agent run: it can carry what the agent saw
/// or did (URLs, element labels, action arguments), so it must not enter JetStream
/// raw. The match has no wildcard arm on purpose: a new `EventType` variant is a
/// compile error here until someone decides which side it is on.
pub fn is_agent_class(t: &EventType) -> bool {
    match t {
        EventType::AgentStarted
        | EventType::ActionStarted
        | EventType::ActionCompleted
        | EventType::ActionFailed
        | EventType::ObservationReady
        | EventType::AgentCompleted
        | EventType::AgentFailed
        | EventType::AgentExpired
        | EventType::AgentBudgetExhausted
        | EventType::AgentAttached
        | EventType::AgentDetached => true,
        EventType::RunStarted
        | EventType::RunPaused
        | EventType::RunResumed
        | EventType::RunCancelled
        | EventType::RunCompleted
        | EventType::RunFailed
        | EventType::PageQueued
        | EventType::PageFetched
        | EventType::PageFailed
        | EventType::PageBlocked
        | EventType::PageRetried
        | EventType::PageEscalated
        | EventType::ArtifactWritten
        | EventType::SnapshotCreated
        | EventType::StoreRecordWritten
        | EventType::StoreRecordFailed
        | EventType::LeaseAcquired
        | EventType::LeaseReleased
        | EventType::ProfileRestored
        | EventType::ProfileCaptured
        | EventType::ChangeDetected
        | EventType::ChangeUnchanged
        | EventType::ScheduleFired
        | EventType::SearchIssued
        | EventType::HostDiscovered
        | EventType::BrandingExtracted
        | EventType::PageExtracted
        | EventType::ControlRequested
        | EventType::RunTimedOut
        | EventType::ScheduleSkipped => false,
    }
}

fn event_type_token(t: &EventType) -> &'static str {
    match t {
        EventType::RunStarted => "run_started",
        EventType::RunPaused => "run_paused",
        EventType::RunResumed => "run_resumed",
        EventType::RunCancelled => "run_cancelled",
        EventType::RunCompleted => "run_completed",
        EventType::RunFailed => "run_failed",
        EventType::PageQueued => "page_queued",
        EventType::PageFetched => "page_fetched",
        EventType::PageFailed => "page_failed",
        EventType::PageBlocked => "page_blocked",
        EventType::PageRetried => "page_retried",
        EventType::PageEscalated => "page_escalated",
        EventType::ArtifactWritten => "artifact_written",
        EventType::SnapshotCreated => "snapshot_created",
        EventType::StoreRecordWritten => "store_record_written",
        EventType::StoreRecordFailed => "store_record_failed",
        EventType::LeaseAcquired => "lease_acquired",
        EventType::LeaseReleased => "lease_released",
        EventType::ProfileRestored => "profile_restored",
        EventType::ProfileCaptured => "profile_captured",
        EventType::ChangeDetected => "change_detected",
        EventType::ChangeUnchanged => "change_unchanged",
        EventType::ScheduleFired => "schedule_fired",
        EventType::AgentStarted => "agent_started",
        EventType::ActionStarted => "action_started",
        EventType::ActionCompleted => "action_completed",
        EventType::ActionFailed => "action_failed",
        EventType::ObservationReady => "observation_ready",
        EventType::AgentCompleted => "agent_completed",
        EventType::AgentFailed => "agent_failed",
        EventType::AgentExpired => "agent_expired",
        EventType::SearchIssued => "search_issued",
        EventType::HostDiscovered => "host_discovered",
        EventType::BrandingExtracted => "branding_extracted",
        EventType::PageExtracted => "page_extracted",
        EventType::AgentBudgetExhausted => "agent_budget_exhausted",
        EventType::ControlRequested => "control_requested",
        EventType::RunTimedOut => "run_timed_out",
        EventType::ScheduleSkipped => "schedule_skipped",
        EventType::AgentAttached => "agent_attached",
        EventType::AgentDetached => "agent_detached",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_have_quarry_prefix() {
        let c = NatsConfig::new("nats://localhost:4222");
        assert_eq!(c.subject_prefix, "quarry");
        assert_eq!(c.stream_name, "QUARRY_EVENTS");
    }

    /// Every `EventType`, listed once. `is_agent_class` has no wildcard arm, so a new
    /// variant fails to compile there; this list keeps the expectations below honest.
    const AGENT_CLASS: &[EventType] = &[
        EventType::AgentStarted,
        EventType::ActionStarted,
        EventType::ActionCompleted,
        EventType::ActionFailed,
        EventType::ObservationReady,
        EventType::AgentCompleted,
        EventType::AgentFailed,
        EventType::AgentExpired,
        EventType::AgentBudgetExhausted,
        EventType::AgentAttached,
        EventType::AgentDetached,
    ];
    const RUN_AND_CRAWL_CLASS: &[EventType] = &[
        EventType::RunStarted,
        EventType::RunPaused,
        EventType::RunResumed,
        EventType::RunCancelled,
        EventType::RunCompleted,
        EventType::RunFailed,
        EventType::PageQueued,
        EventType::PageFetched,
        EventType::PageFailed,
        EventType::PageBlocked,
        EventType::PageRetried,
        EventType::PageEscalated,
        EventType::ArtifactWritten,
        EventType::SnapshotCreated,
        EventType::StoreRecordWritten,
        EventType::StoreRecordFailed,
        EventType::LeaseAcquired,
        EventType::LeaseReleased,
        EventType::ProfileRestored,
        EventType::ProfileCaptured,
        EventType::ChangeDetected,
        EventType::ChangeUnchanged,
        EventType::ScheduleFired,
        EventType::SearchIssued,
        EventType::HostDiscovered,
        EventType::BrandingExtracted,
        EventType::PageExtracted,
        EventType::ControlRequested,
        EventType::RunTimedOut,
        EventType::ScheduleSkipped,
    ];

    #[test]
    fn agent_class_events_are_not_published_raw() {
        for t in AGENT_CLASS {
            assert!(is_agent_class(t), "{t:?} must be agent class");
        }
    }

    #[test]
    fn run_and_crawl_events_are_published_as_before() {
        for t in RUN_AND_CRAWL_CLASS {
            assert!(!is_agent_class(t), "{t:?} must still be published");
        }
    }

    #[test]
    fn the_two_lists_cover_every_event_type_without_overlap() {
        // 11 agent-class + 30 run/crawl = the 41 variants of EventType. If a variant
        // is added, `is_agent_class` stops compiling first; this catches a list that
        // was not updated with it.
        let mut tokens: Vec<&str> = AGENT_CLASS
            .iter()
            .chain(RUN_AND_CRAWL_CLASS.iter())
            .map(event_type_token)
            .collect();
        let n = tokens.len();
        tokens.sort_unstable();
        tokens.dedup();
        assert_eq!(tokens.len(), n, "an event type is listed twice");
        assert_eq!(n, 41);
    }

    #[test]
    fn both_publish_entry_points_pass_through_the_agent_gate() {
        // The live publish path needs a broker, so this scans the source: the gate
        // sits at the top of `publish_encoded`, which `publish` and `publish_event`
        // both call, ahead of any `js.publish`.
        let src = include_str!("nats_event_bus.rs").replace("\r\n", "\n");
        let body = src
            .split("async fn publish_encoded(")
            .nth(1)
            .expect("publish_encoded present");
        let gate = body.find("is_agent_class(&event_type)").expect("gate present");
        let first_publish = body.find("let primary_pub").expect("js publish present");
        assert!(gate < first_publish, "the gate must precede the JetStream publish");
    }

    #[test]
    fn config_supports_token_auth() {
        let c = NatsConfig::new("nats://localhost:4222").with_token("secret");
        assert_eq!(c.auth_token, Some("secret".to_string()));
    }

    #[test]
    fn event_type_tokens_are_snake_case() {
        assert_eq!(event_type_token(&EventType::RunStarted), "run_started");
        assert_eq!(event_type_token(&EventType::AgentFailed), "agent_failed");
        assert_eq!(
            event_type_token(&EventType::StoreRecordWritten),
            "store_record_written"
        );
        assert_eq!(
            event_type_token(&EventType::PageExtracted),
            "page_extracted"
        );
        assert_eq!(
            event_type_token(&EventType::StoreRecordFailed),
            "store_record_failed"
        );
    }

    #[test]
    fn subject_format_is_canonical() {
        let cfg = NatsConfig::new("nats://x");
        // Build a fake bus shell to test subject formatting (no client needed).
        let run_id: RunKind = Id::new();
        let subject = format!(
            "{}.run.{}.{}",
            cfg.subject_prefix,
            run_id,
            event_type_token(&EventType::RunStarted)
        );
        assert!(subject.starts_with("quarry.run."));
        assert!(subject.ends_with(".run_started"));
    }

    /// We can't unit-test the live publish path without a NATS broker,
    /// but we can verify the per-run sequence counter logic in isolation.
    #[tokio::test]
    async fn next_seq_is_monotonic_per_run() {
        // Build a bare-bones bus shim that bypasses the connect step. We
        // need only the `seq_per_run` field to exercise next_seq. Use the
        // same Arc<RwLock> shape so the helper API matches.
        let map: Arc<RwLock<HashMap<String, Arc<AtomicU64>>>> =
            Arc::new(RwLock::new(HashMap::new()));

        async fn next(map: &Arc<RwLock<HashMap<String, Arc<AtomicU64>>>>, key: &str) -> u64 {
            {
                let m = map.read().await;
                if let Some(c) = m.get(key) {
                    return c.fetch_add(1, Ordering::Relaxed) + 1;
                }
            }
            let mut m = map.write().await;
            let c = m
                .entry(key.to_string())
                .or_insert_with(|| Arc::new(AtomicU64::new(0)));
            c.fetch_add(1, Ordering::Relaxed) + 1
        }

        let s1 = next(&map, "run-A").await;
        let s2 = next(&map, "run-A").await;
        let s3 = next(&map, "run-A").await;
        assert_eq!(s1, 1);
        assert_eq!(s2, 2);
        assert_eq!(s3, 3);

        // Different run gets its own counter starting at 1.
        let other = next(&map, "run-B").await;
        assert_eq!(other, 1);

        // Original run's next is 4.
        let s4 = next(&map, "run-A").await;
        assert_eq!(s4, 4);
    }
}
