//! The durable stream the ledger rides, and the sinks that publish to it.
//!
//! # Why a stream, and why this one
//!
//! The v1 cost event was a plain core-NATS publish read by a plain core subscription: a
//! consumer that was restarting, or a broker that was busy, lost the event, and the
//! producer never learned. Spend that is not recorded cannot be billed or budgeted, so the
//! v2 event is published to a JetStream stream and acknowledged by the broker
//! (`PubAck`), and the consumer reads it through a durable consumer that acknowledges after
//! the row is written.
//!
//! The stream binds the subject the cost events always used, `dataplane.cost.ledger`.
//! Binding it (rather than inventing a second subject) is what makes the migration safe:
//!
//! * a stream captures a core publish on its subject, so a producer that has not been
//!   upgraded yet keeps working, and its events are retained too;
//! * a JetStream publish is also delivered to core subscribers, so a consumer that has
//!   not been upgraded still sees the events. That is the "dual publish for one release"
//!   without a second publish call, which would have booked every event twice;
//! * a subject belongs to exactly one stream (`infra/nats/SUBJECTS.md`).
//!
//! # Retention
//!
//! `Limits`, never `Interest` or `WorkQueue`. Interest drops a message that has no consumer
//! bound when it arrives, and a producer that starts before the consumer (the order of a
//! deploy) would lose every event in between while the stream looked healthy. The
//! definition is deliberately the same shape as the dead-letter stream's
//! (`nats-connection-rs/src/dlq.rs`) for the same reason. `ensure_stream` refuses to
//! run on a stream that exists with another retention, because retention cannot be
//! changed in place and the alternative is a ledger that silently loses events.
//!
//! The data-orchestrator creates and checks the same stream with the same values
//! (`internal/cost/stream.go`); `scripts/check-subjects.sh` fails when the two names
//! drift.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use async_nats::jetstream::{self, message::PublishMessage, Context};
use event_envelope_rs::EventSigner;

use crate::event::UsageEvent;
use crate::{LedgerError, SUBJECT_COST_LEDGER};

/// The stream's name. Mirrored in `data-orchestrator-go/internal/cost/stream.go`.
pub const STREAM_NAME: &str = "DATAPLANE_COST";

/// 7 days, like the other fan-out streams: long enough that a consumer down over a weekend
/// loses nothing, and the same horizon the consumer accepts an expired envelope within.
pub const STREAM_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

/// 512 MiB, bounded on purpose: `Limits` never discards on acknowledgement, so a ceiling
/// is what stops a consumer that is down for a long time from filling the broker's store.
/// An event is about 1.5 KiB, so this holds roughly 350,000.
pub const STREAM_MAX_BYTES: i64 = 512 * 1024 * 1024;

/// The window inside which the broker drops a second publish of the same `Nats-Msg-Id`,
/// which is what makes a publish retry harmless.
pub const STREAM_DUPLICATE_WINDOW: Duration = Duration::from_secs(120);

const PUBLISH_ATTEMPTS: usize = 3;
const PUBLISH_BACKOFF: [Duration; 2] = [Duration::from_millis(200), Duration::from_secs(1)];

/// The stream as it is defined. Split out of [`ensure_stream`] so the values, which are
/// correctness decisions and not tuning, are asserted without a broker.
pub fn stream_config() -> jetstream::stream::Config {
    jetstream::stream::Config {
        name: STREAM_NAME.to_owned(),
        subjects: vec![SUBJECT_COST_LEDGER.to_owned()],
        // Written out, not left to the default: see the module documentation.
        retention: jetstream::stream::RetentionPolicy::Limits,
        // A ledger that lived in memory would not survive the broker restart that often
        // comes with the incident that made it matter.
        storage: jetstream::stream::StorageType::File,
        max_age: STREAM_MAX_AGE,
        max_bytes: STREAM_MAX_BYTES,
        discard: jetstream::stream::DiscardPolicy::Old,
        duplicate_window: STREAM_DUPLICATE_WINDOW,
        ..Default::default()
    }
}

/// What an existing stream needs to be the ledger's: `Err` when it cannot be (a setting
/// that cannot be changed in place is wrong), `Ok(Some(config))` with the converged
/// configuration when something that can be changed is behind, `Ok(None)` when it is right.
///
/// Only ever raises a limit: an operator who widened one on a live broker keeps it.
pub fn converge(
    current: &jetstream::stream::Config,
) -> Result<Option<jetstream::stream::Config>, String> {
    if current.retention != jetstream::stream::RetentionPolicy::Limits {
        return Err(format!(
            "{STREAM_NAME} exists with {:?} retention: the ledger needs Limits, because under \
             any other policy an event published while no consumer is bound is discarded. \
             Retention cannot be changed in place: delete and recreate the stream",
            current.retention
        ));
    }
    if current.storage != jetstream::stream::StorageType::File {
        return Err(format!(
            "{STREAM_NAME} exists with {:?} storage: the ledger must survive a broker restart",
            current.storage
        ));
    }
    let mut upgraded = current.clone();
    let mut changed = false;
    if !upgraded
        .subjects
        .iter()
        .any(|subject| subject == SUBJECT_COST_LEDGER)
    {
        upgraded.subjects.push(SUBJECT_COST_LEDGER.to_owned());
        changed = true;
    }
    if upgraded.max_age != Duration::ZERO && upgraded.max_age < STREAM_MAX_AGE {
        upgraded.max_age = STREAM_MAX_AGE;
        changed = true;
    }
    if upgraded.max_bytes > 0 && upgraded.max_bytes < STREAM_MAX_BYTES {
        upgraded.max_bytes = STREAM_MAX_BYTES;
        changed = true;
    }
    if upgraded.duplicate_window < STREAM_DUPLICATE_WINDOW {
        upgraded.duplicate_window = STREAM_DUPLICATE_WINDOW;
        changed = true;
    }
    Ok(changed.then_some(upgraded))
}

/// Create the stream, or check and converge the one that exists. Idempotent: every
/// producer and the consumer call it on boot.
///
/// Fails, rather than warns, when the stream cannot be the ledger's. A producer that has
/// metering switched on and cannot reach a durable ledger must not start: every event it
/// published would be lost without a sound.
pub async fn ensure_stream(js: &Context) -> Result<(), LedgerError> {
    let stream_error = |error: &dyn std::fmt::Display| LedgerError::Stream(error.to_string());
    let mut stream = js
        .get_or_create_stream(stream_config())
        .await
        .map_err(|error| stream_error(&error))?;
    let current = stream
        .info()
        .await
        .map_err(|error| stream_error(&error))?
        .config
        .clone();
    if let Some(upgraded) = converge(&current).map_err(LedgerError::Stream)? {
        js.update_stream(upgraded)
            .await
            .map_err(|error| stream_error(&error))?;
        tracing::info!(stream = STREAM_NAME, "usage ledger stream limits converged");
    }
    tracing::info!(
        stream = STREAM_NAME,
        subject = SUBJECT_COST_LEDGER,
        "usage ledger stream ready"
    );
    Ok(())
}

pub type LedgerFuture<'a> = Pin<Box<dyn Future<Output = Result<(), LedgerError>> + Send + 'a>>;

/// Where a producer sends the events it has built.
pub trait LedgerSink: Send + Sync {
    /// Sign and publish one event. `Ok` means the broker acknowledged it (or already had it).
    fn publish<'a>(&'a self, event: &'a UsageEvent) -> LedgerFuture<'a>;
}

/// The production sink: sign with the producer's event key, publish to the stream with the
/// event's id as the broker's de-duplication id, and wait for the `PubAck`.
pub struct JetStreamLedger {
    js: Context,
    signer: Arc<EventSigner>,
}

impl JetStreamLedger {
    pub fn new(client: async_nats::Client, signer: Arc<EventSigner>) -> Self {
        Self {
            js: jetstream::new(client),
            signer,
        }
    }

    /// The JetStream context, so a producer can [`ensure_stream`] with the connection it
    /// already holds.
    pub fn context(&self) -> &Context {
        &self.js
    }

    async fn publish_once(&self, envelope: &[u8], message_id: &str) -> Result<(), LedgerError> {
        let ack = self
            .js
            .send_publish(
                SUBJECT_COST_LEDGER,
                PublishMessage::build()
                    .payload(envelope.to_vec().into())
                    .message_id(message_id),
            )
            .await
            .map_err(|error| LedgerError::Publish(error.to_string()))?
            .await
            .map_err(|error| LedgerError::Publish(error.to_string()))?;
        // The subject is captured by one stream, ours. Another name here means the subject
        // was bound somewhere else and the consumer will never see the event.
        if ack.stream != STREAM_NAME {
            return Err(LedgerError::Publish(format!(
                "acknowledged by stream {} instead of {STREAM_NAME}",
                ack.stream
            )));
        }
        Ok(())
    }
}

impl LedgerSink for JetStreamLedger {
    fn publish<'a>(&'a self, event: &'a UsageEvent) -> LedgerFuture<'a> {
        Box::pin(async move {
            // Signed once: a retry sends the same bytes under the same message id, which is
            // what the broker de-duplicates. The consumer accepts an authentic envelope
            // whose short lifetime has passed, so a retry that outlives it is not lost.
            let envelope = event.sign(&self.signer)?;
            let message_id = event.message_id();
            let mut last = None;
            for attempt in 0..PUBLISH_ATTEMPTS {
                match self.publish_once(&envelope, &message_id).await {
                    Ok(()) => return Ok(()),
                    Err(error) => {
                        tracing::warn!(
                            attempt = attempt + 1,
                            attempts = PUBLISH_ATTEMPTS,
                            feature = event.feature,
                            %error,
                            "usage event was not acknowledged"
                        );
                        last = Some(error);
                        if let Some(pause) = PUBLISH_BACKOFF.get(attempt) {
                            tokio::time::sleep(*pause).await;
                        }
                    }
                }
            }
            Err(last.unwrap_or_else(|| LedgerError::Publish("no attempt was made".to_owned())))
        })
    }
}

/// An in-memory sink for tests and local development: records what it is given, and can be
/// told to refuse.
#[derive(Default)]
pub struct MemoryLedger {
    state: Mutex<MemoryState>,
}

#[derive(Default)]
struct MemoryState {
    events: Vec<UsageEvent>,
    refusals: usize,
}

impl MemoryLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// Refuse the next `count` publishes.
    pub fn refuse_next(&self, count: usize) {
        self.lock().refusals = count;
    }

    pub fn events(&self) -> Vec<UsageEvent> {
        self.lock().events.clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, MemoryState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl LedgerSink for MemoryLedger {
    fn publish<'a>(&'a self, event: &'a UsageEvent) -> LedgerFuture<'a> {
        Box::pin(async move {
            let mut state = self.lock();
            if state.refusals > 0 {
                state.refusals -= 1;
                return Err(LedgerError::Publish(
                    "refused by the test ledger".to_owned(),
                ));
            }
            state.events.push(event.clone());
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CallUsage, Feature, UsageBuilder};

    #[test]
    fn the_stream_is_a_durable_limits_stream_on_the_cost_subject_alone() {
        let config = stream_config();
        assert_eq!(config.name, "DATAPLANE_COST");
        assert_eq!(config.subjects, vec!["dataplane.cost.ledger".to_owned()]);
        // Interest and WorkQueue both discard an event that has no consumer bound when it
        // arrives: the producer-before-consumer order of a deploy would lose them.
        assert_eq!(config.retention, jetstream::stream::RetentionPolicy::Limits);
        assert_eq!(config.storage, jetstream::stream::StorageType::File);
        assert_eq!(config.max_age, Duration::from_secs(7 * 24 * 3600));
        assert_eq!(config.max_bytes, 512 * 1024 * 1024);
        assert_eq!(config.discard, jetstream::stream::DiscardPolicy::Old);
        assert_eq!(config.duplicate_window, Duration::from_secs(120));
    }

    #[test]
    fn a_stream_with_the_wrong_retention_or_storage_is_refused_not_repaired() {
        for retention in [
            jetstream::stream::RetentionPolicy::Interest,
            jetstream::stream::RetentionPolicy::WorkQueue,
        ] {
            let mut existing = stream_config();
            existing.retention = retention;
            let error = converge(&existing).expect_err("a ledger on a discarding policy");
            assert!(error.contains("Limits"), "{error}");
        }
        let mut memory = stream_config();
        memory.storage = jetstream::stream::StorageType::Memory;
        assert!(converge(&memory).is_err());
    }

    #[test]
    fn a_stream_that_is_right_is_left_alone() {
        assert_eq!(converge(&stream_config()), Ok(None));
    }

    #[test]
    fn what_can_be_changed_is_raised_and_never_lowered() {
        let mut behind = stream_config();
        behind.max_age = Duration::from_secs(24 * 3600);
        behind.max_bytes = 1024;
        behind.duplicate_window = Duration::from_secs(1);
        behind.subjects = vec!["operator.bound.subject".to_owned()];
        let upgraded = converge(&behind).unwrap().expect("behind");
        assert_eq!(upgraded.max_age, STREAM_MAX_AGE);
        assert_eq!(upgraded.max_bytes, STREAM_MAX_BYTES);
        assert_eq!(upgraded.duplicate_window, STREAM_DUPLICATE_WINDOW);
        assert!(upgraded
            .subjects
            .contains(&"dataplane.cost.ledger".to_owned()));
        assert!(
            upgraded
                .subjects
                .contains(&"operator.bound.subject".to_owned()),
            "another subject an operator bound is not removed"
        );

        let mut widened = stream_config();
        widened.max_age = Duration::from_secs(30 * 24 * 3600);
        widened.max_bytes = 4 * STREAM_MAX_BYTES;
        assert_eq!(
            converge(&widened),
            Ok(None),
            "an operator's wider limit is kept"
        );

        let mut unlimited = stream_config();
        unlimited.max_age = Duration::ZERO;
        unlimited.max_bytes = -1;
        assert_eq!(
            converge(&unlimited),
            Ok(None),
            "no limit is wider than any limit"
        );
    }

    fn event() -> UsageEvent {
        let mut builder = UsageBuilder::new(Feature::GraphExtraction, "org-a", false).unwrap();
        builder.push(CallUsage::from_counts(
            "r1",
            "gpt-4o-mini",
            "azure-openai",
            100,
            10,
            0,
            true,
            false,
        ));
        builder.build().remove(0)
    }

    #[tokio::test]
    async fn the_memory_ledger_records_and_refuses_on_request() {
        let ledger = MemoryLedger::new();
        ledger.refuse_next(1);
        let event = event();
        assert!(ledger.publish(&event).await.is_err());
        assert!(ledger.events().is_empty());
        ledger.publish(&event).await.expect("second publish");
        assert_eq!(ledger.events(), vec![event]);
    }
}
