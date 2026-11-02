//! `JetStream` consumer for the run trace (WS10.S6).
//!
//! Quarry publishes browser-run trace events on `mp.v1.trace.run.<run_id>` with a publish-only
//! principal (WS10.S5). This durable consumer, `session-core-trace`, validates each message and
//! inserts it into `run_trace_events` (migration 0086), where `EventLog.QueryEvents`
//! (`eventlog_grpc.rs`) reads it back for the run's owner.
//!
//! What the consumer will not do:
//!
//! * Trust the message's identity. `org_id` must match the run's, and the stored `user_id` is the
//!   run's, never the message's. A mismatch is terminated, with ids only in the log.
//! * Store a zero-retention run. The message must carry an explicit `"zdr": false` (an absent key
//!   is refused, because the JSON twin decodes an absent key to false), AND the run must not be
//!   `retention_class = 'zdr'`.
//! * Store a producer that is not `quarry-edge`, an event type that is not one of the seven trace
//!   types, a payload over 8 KiB, or a run id that is not a single subject token.
//!
//! Dispositions: a stored or duplicate message is acked; a message for a run session-core has not
//! seen yet is NAKed after 2 s (the run row can lag the first event) and dropped, counted, once the
//! consumer's `MaxDeliver` is spent; everything the message itself got wrong is terminated; a
//! database error is NAKed.
//!
//! The consumer is off unless `SESSION_CORE_TRACE_CONSUMER` is `1`, `true` or `on`. The retention
//! sweeper at the bottom runs regardless: it only deletes rows older than
//! `SESSION_CORE_TRACE_RETENTION_DAYS` (default 30), and there are none unless the consumer ran.

use crate::store::Pool;
use async_nats::jetstream::{self, consumer::PullConsumer, AckKind};
use chrono::{DateTime, Utc};
use futures::StreamExt;
use metrics::{counter, histogram};
use mp_events::envelope::{Envelope, RUN_ORIGINS};
use serde_json::Value;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

pub const STREAM_NAME: &str = "MODEL_PLANE_RUN_TRACE";
pub const SUBJECT_FILTER: &str = "mp.v1.trace.run.*";
pub const DURABLE_NAME: &str = "session-core-trace";
/// The only producer a trace row may name.
pub const TRACE_PRODUCER: &str = "quarry-edge";
/// The seven trace event types (`EventType` 150-156 without the `EVENT_TYPE_` prefix).
pub const TRACE_EVENT_TYPES: [&str; 7] = [
    "BROWSER_SESSION_OPENED",
    "BROWSER_SESSION_CLOSED",
    "BROWSER_CONTROL_CHANGED",
    "BROWSER_TAB_CHANGED",
    "BROWSER_STEP_RECEIPT_RECORDED",
    "BROWSER_EGRESS_DECIDED",
    "BROWSER_ARTIFACT_RECORDED",
];

/// The consumer switch. Anything but `1`, `true` or `on` leaves it off.
pub const CONSUMER_ENV: &str = "SESSION_CORE_TRACE_CONSUMER";
/// Days a trace row is kept. Unset, zero or unparseable means [`DEFAULT_RETENTION_DAYS`].
pub const RETENTION_DAYS_ENV: &str = "SESSION_CORE_TRACE_RETENTION_DAYS";
pub const DEFAULT_RETENTION_DAYS: u32 = 30;
/// The sweeper deletes at most this many rows per statement.
pub const SWEEP_BATCH: i64 = 5_000;

/// A message larger than this is refused before it is parsed.
const MAX_MESSAGE_BYTES: usize = 64 * 1024;
/// The payload's cap, measured the way Postgres measures `payload::text` (migration 0086's CHECK).
const MAX_PAYLOAD_BYTES: usize = 8192;
const MAX_ID_LEN: usize = 64;
const MAX_LABEL_LEN: usize = 128;
const MAX_RESOURCE_REF_LEN: usize = 256;
/// How long a NAKed message waits before it is redelivered.
const RETRY_DELAY: Duration = Duration::from_secs(2);
const SWEEP_INTERVAL: Duration = Duration::from_secs(3600);

/// Whether the consumer switch value enables it.
#[must_use]
pub fn flag_enabled(value: Option<&str>) -> bool {
    matches!(value.map(str::trim), Some("1" | "true" | "on"))
}

/// Whether `SESSION_CORE_TRACE_CONSUMER` enables the consumer.
#[must_use]
pub fn enabled() -> bool {
    flag_enabled(std::env::var(CONSUMER_ENV).ok().as_deref())
}

/// Retention in days for a configured value.
#[must_use]
pub fn retention_days_from(value: Option<&str>) -> u32 {
    value
        .and_then(|v| v.trim().parse::<u32>().ok())
        .filter(|days| *days > 0)
        .unwrap_or(DEFAULT_RETENTION_DAYS)
}

/// Why a message was refused. The code is the metric label and the log line; no variant carries
/// message content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invalid {
    MessageTooLarge,
    NotJson,
    NotAnObject,
    /// The `zdr` key is absent, or is not a boolean.
    ZdrUndeclared,
    /// `"zdr": true`: a zero-retention event is never stored.
    ZdrTrue,
    /// The JSON does not decode as an envelope.
    Shape,
    EventType,
    Producer,
    ProducerSeq,
    RunId,
    OrgId,
    EventId,
    FieldTooLong,
    Nul,
    Origin,
    Agent,
    PayloadNotAnObject,
    PayloadTooLarge,
}

impl Invalid {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::MessageTooLarge => "message_too_large",
            Self::NotJson => "not_json",
            Self::NotAnObject => "not_an_object",
            Self::ZdrUndeclared => "zdr_undeclared",
            Self::ZdrTrue => "zdr_true",
            Self::Shape => "shape",
            Self::EventType => "event_type",
            Self::Producer => "producer",
            Self::ProducerSeq => "producer_seq",
            Self::RunId => "run_id",
            Self::OrgId => "org_id",
            Self::EventId => "event_id",
            Self::FieldTooLong => "field_too_long",
            Self::Nul => "nul",
            Self::Origin => "origin",
            Self::Agent => "agent",
            Self::PayloadNotAnObject => "payload_not_an_object",
            Self::PayloadTooLarge => "payload_too_large",
        }
    }
}

/// A message that passed [`validate_trace_envelope`]: everything `persist` binds. `org_id` is the
/// message's CLAIM, checked against the run's; the stored org and user are the run's.
#[derive(Debug, Clone, PartialEq)]
pub struct TraceEvent {
    pub event_id: String,
    pub event_type: String,
    pub schema_version: i32,
    pub occurred_at: DateTime<Utc>,
    pub producer: String,
    pub producer_seq: i64,
    pub org_id: String,
    pub run_id: String,
    pub resource_ref: String,
    pub correlation_id: String,
    pub causation_id: String,
    pub request_id: String,
    pub agent_id: String,
    pub agent_role: String,
    pub agent_parent_id: String,
    pub origin: String,
    pub payload: Value,
}

fn id_token_ok(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ID_LEN
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// The length Postgres gives `payload::text` for a jsonb holding `value`: objects and arrays are
/// rendered with `": "` and `", "` separators, which is longer than the compact form the producer
/// sent. Measured here so a payload the CHECK would refuse is terminated, not NAKed five times.
fn jsonb_text_len(value: &Value) -> usize {
    match value {
        Value::Null | Value::Bool(true) => 4,
        Value::Bool(false) => 5,
        Value::Number(n) => n.to_string().len(),
        Value::String(s) => serde_json::to_string(s).map_or(usize::MAX / 4, |t| t.len()),
        Value::Array(items) => {
            let inner: usize = items.iter().map(jsonb_text_len).sum();
            2 + inner + 2 * items.len().saturating_sub(1)
        }
        Value::Object(map) => {
            let inner: usize = map
                .iter()
                .map(|(k, v)| {
                    serde_json::to_string(k).map_or(usize::MAX / 4, |t| t.len())
                        + 2
                        + jsonb_text_len(v)
                })
                .sum();
            2 + inner + 2 * map.len().saturating_sub(1)
        }
    }
}

/// Postgres rejects a NUL in `text` and in a jsonb string, permanently.
fn json_has_nul(value: &Value) -> bool {
    match value {
        Value::String(s) => s.contains('\0'),
        Value::Array(items) => items.iter().any(json_has_nul),
        Value::Object(map) => map.iter().any(|(k, v)| k.contains('\0') || json_has_nul(v)),
        _ => false,
    }
}

/// Validate one message. Pure: no database, no clock.
///
/// # Errors
///
/// The first rule the message breaks (see [`Invalid`]).
pub fn validate_trace_envelope(raw: &[u8]) -> Result<TraceEvent, Invalid> {
    if raw.len() > MAX_MESSAGE_BYTES {
        return Err(Invalid::MessageTooLarge);
    }
    let value: Value = serde_json::from_slice(raw).map_err(|_| Invalid::NotJson)?;
    let Value::Object(object) = &value else {
        return Err(Invalid::NotAnObject);
    };
    // The posture is probed on the raw JSON: `Envelope::zdr` is `#[serde(default)]`, so an absent
    // key would read as a non-ZDR run. A trace producer must say so itself.
    match object.get("zdr") {
        Some(Value::Bool(false)) => {}
        Some(Value::Bool(true)) => return Err(Invalid::ZdrTrue),
        _ => return Err(Invalid::ZdrUndeclared),
    }
    let envelope: Envelope = serde_json::from_value(value).map_err(|_| Invalid::Shape)?;

    if !TRACE_EVENT_TYPES.contains(&envelope.event_type.as_str()) {
        return Err(Invalid::EventType);
    }
    if envelope.producer != TRACE_PRODUCER {
        return Err(Invalid::Producer);
    }
    let producer_seq = match envelope.producer_seq {
        Some(seq) if seq >= 1 => i64::try_from(seq).map_err(|_| Invalid::ProducerSeq)?,
        _ => return Err(Invalid::ProducerSeq),
    };
    let run_id = envelope.run_id.clone().unwrap_or_default();
    if !id_token_ok(&run_id) {
        return Err(Invalid::RunId);
    }
    if envelope.org_id.is_empty() || envelope.org_id.len() > 128 {
        return Err(Invalid::OrgId);
    }
    if envelope.event_id.is_empty() || envelope.event_id.len() > MAX_LABEL_LEN {
        return Err(Invalid::EventId);
    }
    if envelope.schema_version == 0 {
        return Err(Invalid::Shape);
    }
    if envelope.resource_ref.is_empty() || envelope.resource_ref.len() > MAX_RESOURCE_REF_LEN {
        return Err(Invalid::FieldTooLong);
    }
    let request_id = envelope.request_id.clone().unwrap_or_default();
    for label in [
        &envelope.correlation_id,
        &envelope.causation_id,
        &request_id,
    ] {
        if label.len() > MAX_LABEL_LEN {
            return Err(Invalid::FieldTooLong);
        }
    }
    let origin = envelope.origin.clone().unwrap_or_default();
    if !origin.is_empty() && !RUN_ORIGINS.contains(&origin.as_str()) {
        return Err(Invalid::Origin);
    }
    let agent = envelope.agent.clone().unwrap_or_default();
    for part in [&agent.id, &agent.role, &agent.parent_id] {
        if !part.is_empty() && !id_token_ok(part) {
            return Err(Invalid::Agent);
        }
    }
    for text in [
        &envelope.event_id,
        &envelope.org_id,
        &envelope.resource_ref,
        &envelope.correlation_id,
        &envelope.causation_id,
        &request_id,
    ] {
        if text.contains('\0') {
            return Err(Invalid::Nul);
        }
    }
    if !envelope.payload.is_object() {
        return Err(Invalid::PayloadNotAnObject);
    }
    if json_has_nul(&envelope.payload) {
        return Err(Invalid::Nul);
    }
    if jsonb_text_len(&envelope.payload) > MAX_PAYLOAD_BYTES {
        return Err(Invalid::PayloadTooLarge);
    }

    Ok(TraceEvent {
        event_id: envelope.event_id,
        event_type: envelope.event_type,
        schema_version: i32::try_from(envelope.schema_version).unwrap_or(i32::MAX),
        occurred_at: envelope.ts,
        producer: envelope.producer,
        producer_seq,
        org_id: envelope.org_id,
        run_id,
        resource_ref: envelope.resource_ref,
        correlation_id: envelope.correlation_id,
        causation_id: envelope.causation_id,
        request_id,
        agent_id: agent.id,
        agent_role: agent.role,
        agent_parent_id: agent.parent_id,
        origin,
        payload: envelope.payload,
    })
}

/// What `persist` did with one validated event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistOutcome {
    /// A new row. `gap_before` is true when `producer_seq - 1` was not in the table yet: a dropped
    /// event, or one still in flight (a redelivery can land out of order), so it is a suspicion,
    /// counted and never an error.
    Inserted { gap_before: bool },
    /// The `(run_id, producer, producer_seq)` or the `event_id` is already stored.
    Duplicate,
    /// No such run, yet. The run row can lag the first event.
    RunUnknown,
    /// The run belongs to another org than the message claims.
    OrgMismatch,
    /// The run is a zero-retention run.
    ZdrRun,
}

/// What the loop does with the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    Ack,
    Nak(Duration),
    Term,
}

impl PersistOutcome {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Inserted { .. } => "stored",
            Self::Duplicate => "duplicate",
            Self::RunUnknown => "run_unknown",
            Self::OrgMismatch => "org_mismatch",
            Self::ZdrRun => "zdr_run",
        }
    }

    /// `RunUnknown` is NAKed (the loop turns it into a drop once `MaxDeliver` is spent).
    #[must_use]
    pub const fn disposition(self) -> Disposition {
        match self {
            Self::Inserted { .. } | Self::Duplicate => Disposition::Ack,
            Self::RunUnknown => Disposition::Nak(RETRY_DELAY),
            Self::OrgMismatch | Self::ZdrRun => Disposition::Term,
        }
    }
}

/// Insert one validated event.
///
/// The row's tenant and user are the RUN'S, read in the insert itself: the statement selects from
/// `runs` where the id, the claimed org and a non-ZDR class all hold, so a wrong claim or a ZDR run
/// inserts nothing. When it inserts nothing, one more read says which of the four reasons it was.
///
/// # Errors
///
/// A database error; the caller NAKs.
pub async fn persist(pool: &Pool, event: &TraceEvent) -> Result<PersistOutcome, sqlx::Error> {
    let inserted = sqlx::query(
        "INSERT INTO run_trace_events (
             event_id, run_id, org_id, user_id, producer, producer_seq, event_type,
             schema_version, occurred_at, resource_ref, correlation_id, causation_id,
             request_id, agent_id, agent_role, agent_parent_id, origin, payload
         )
         SELECT $1, r.id, r.org_id, r.user_id, $3, $4, $5,
                $6, $7, $8, $9, $10,
                $11, $12, $13, $14, $15, $16
         FROM runs r
         WHERE r.id = $2 AND r.org_id = $17 AND r.retention_class <> 'zdr'
         ON CONFLICT DO NOTHING",
    )
    .bind(&event.event_id)
    .bind(&event.run_id)
    .bind(&event.producer)
    .bind(event.producer_seq)
    .bind(&event.event_type)
    .bind(event.schema_version)
    .bind(event.occurred_at)
    .bind(&event.resource_ref)
    .bind(&event.correlation_id)
    .bind(&event.causation_id)
    .bind(&event.request_id)
    .bind(&event.agent_id)
    .bind(&event.agent_role)
    .bind(&event.agent_parent_id)
    .bind(&event.origin)
    .bind(&event.payload)
    .bind(&event.org_id)
    .execute(pool)
    .await?
    .rows_affected();

    if inserted == 1 {
        let gap_before = if event.producer_seq > 1 {
            let previous: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM run_trace_events
                                WHERE run_id = $1 AND producer = $2 AND producer_seq = $3)",
            )
            .bind(&event.run_id)
            .bind(&event.producer)
            .bind(event.producer_seq - 1)
            .fetch_one(pool)
            .await?;
            !previous
        } else {
            false
        };
        return Ok(PersistOutcome::Inserted { gap_before });
    }

    let run: Option<(String, String)> =
        sqlx::query_as("SELECT org_id, retention_class FROM runs WHERE id = $1")
            .bind(&event.run_id)
            .fetch_optional(pool)
            .await?;
    Ok(match run {
        None => PersistOutcome::RunUnknown,
        Some((org_id, _)) if org_id != event.org_id => PersistOutcome::OrgMismatch,
        Some((_, class)) if class == "zdr" => PersistOutcome::ZdrRun,
        Some(_) => PersistOutcome::Duplicate,
    })
}

/// The run id a trace subject names: its last token, when the subject is
/// `mp.v1.trace.run.<token>` and nothing else.
fn subject_run_id(subject: &str) -> Option<&str> {
    let token = subject.strip_prefix("mp.v1.trace.run.")?;
    (!token.is_empty() && !token.contains('.')).then_some(token)
}

/// Run the consumer until its message stream ends.
///
/// # Errors
///
/// A connection or binding failure; the supervisor restarts it.
pub async fn run(pool: Pool, nats_url: String) -> anyhow::Result<()> {
    info!(%nats_url, "session-core trace consumer connecting");
    let client = crate::nats_connection::connect(&nats_url).await?;
    let js = jetstream::new(client);

    // Deployment provisioning owns the stream and the durable consumer; the runtime credential
    // can bind, pull and ack, not change the topology.
    let stream = js.get_stream(STREAM_NAME).await?;
    let consumer: PullConsumer = stream
        .get_consumer(DURABLE_NAME)
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    if consumer.cached_info().config.filter_subject != SUBJECT_FILTER {
        return Err(anyhow::anyhow!(
            "pre-provisioned trace consumer filter mismatch"
        ));
    }
    let max_deliver = consumer.cached_info().config.max_deliver;
    let mut messages = consumer.messages().await?;
    info!(max_deliver, "session-core trace consumer ready");

    while let Some(msg) = messages.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                warn!(error = %e, "error receiving trace message");
                counter!("mp_session_trace_events_total", "outcome" => "receive_error")
                    .increment(1);
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            }
        };
        let started = Instant::now();

        let event = match validate_trace_envelope(&msg.payload) {
            Ok(event) if subject_run_id(&msg.subject) == Some(event.run_id.as_str()) => event,
            Ok(_) => {
                // The subject's token is the publisher's; a body naming another run is refused.
                refuse(&msg, "subject_run_mismatch").await;
                continue;
            }
            Err(invalid) => {
                refuse(&msg, invalid.code()).await;
                continue;
            }
        };

        match persist(&pool, &event).await {
            Ok(outcome) => {
                histogram!("mp_session_trace_persist_duration_seconds")
                    .record(started.elapsed().as_secs_f64());
                if let PersistOutcome::Inserted { gap_before: true } = outcome {
                    counter!("mp_session_trace_events_total", "outcome" => "gap_suspected")
                        .increment(1);
                }
                let spent = max_deliver > 0
                    && msg
                        .info()
                        .map_or(false, |info| info.delivered >= max_deliver);
                match outcome.disposition() {
                    Disposition::Ack => {
                        counter!("mp_session_trace_events_total", "outcome" => outcome.label())
                            .increment(1);
                        if let Err(e) = msg.ack().await {
                            warn!(error = %e, "trace ack failed");
                        }
                    }
                    Disposition::Nak(_) if spent => {
                        // The run never appeared in MaxDeliver tries: dropped, counted.
                        warn!(run_id = %event.run_id, "trace event dropped: run unknown after the last delivery");
                        counter!("mp_session_trace_events_total", "outcome" => "dropped_run_unknown")
                            .increment(1);
                        if let Err(e) = msg.ack_with(AckKind::Term).await {
                            warn!(error = %e, "trace term failed");
                        }
                    }
                    Disposition::Nak(delay) => {
                        counter!("mp_session_trace_events_total", "outcome" => outcome.label())
                            .increment(1);
                        if let Err(e) = msg.ack_with(AckKind::Nak(Some(delay))).await {
                            warn!(error = %e, "trace nak failed");
                        }
                    }
                    Disposition::Term => {
                        // Ids only: the run and the org the message claimed, never its content.
                        warn!(
                            run_id = %event.run_id,
                            outcome = outcome.label(),
                            "trace event refused for its run"
                        );
                        counter!("mp_session_trace_events_total", "outcome" => outcome.label())
                            .increment(1);
                        if let Err(e) = msg.ack_with(AckKind::Term).await {
                            warn!(error = %e, "trace term failed");
                        }
                    }
                }
            }
            Err(e) => {
                error!(error = %e, "failed to persist trace event");
                counter!("mp_session_trace_events_total", "outcome" => "db_error").increment(1);
                if let Err(ack_err) = msg.ack_with(AckKind::Nak(Some(RETRY_DELAY))).await {
                    warn!(error = %ack_err, "trace nak failed");
                }
            }
        }
    }

    warn!("session-core trace message stream ended");
    Ok(())
}

/// Terminate a message the validator refused. Only the reason code is logged.
async fn refuse(msg: &jetstream::Message, code: &'static str) {
    warn!(reason = code, "trace event refused");
    counter!("mp_session_trace_events_total", "outcome" => "invalid", "reason" => code)
        .increment(1);
    if let Err(e) = msg.ack_with(AckKind::Term).await {
        warn!(error = %e, "trace term failed");
    }
}

/// Delete trace rows received more than `days` days ago, `batch` rows per statement, until a
/// statement deletes fewer than a full batch. Returns the number deleted.
///
/// # Errors
///
/// A database error; rows already deleted stay deleted.
pub async fn sweep_once(pool: &Pool, days: u32, batch: i64) -> Result<u64, sqlx::Error> {
    let mut total = 0_u64;
    loop {
        let deleted = sqlx::query(
            "DELETE FROM run_trace_events
             WHERE ingest_seq IN (
                 SELECT ingest_seq FROM run_trace_events
                 WHERE received_at < now() - make_interval(days => $1)
                 ORDER BY received_at
                 LIMIT $2
             )",
        )
        .bind(i32::try_from(days).unwrap_or(i32::MAX))
        .bind(batch)
        .execute(pool)
        .await?
        .rows_affected();
        total += deleted;
        if i64::try_from(deleted).unwrap_or(i64::MAX) < batch {
            return Ok(total);
        }
    }
}

/// The retention sweeper: once an hour, until the process stops.
///
/// # Errors
///
/// Never returns `Ok`; a database error ends the pass and the supervisor restarts it.
pub async fn run_retention_sweeper(pool: Pool) -> anyhow::Result<()> {
    let days = retention_days_from(std::env::var(RETENTION_DAYS_ENV).ok().as_deref());
    info!(days, "session-core trace retention sweeper started");
    loop {
        let deleted = sweep_once(&pool, days, SWEEP_BATCH).await?;
        if deleted > 0 {
            info!(deleted, days, "trace retention sweep");
            counter!("mp_session_trace_retention_deleted_total").increment(deleted);
        }
        tokio::time::sleep(SWEEP_INTERVAL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loop_checkpoint::test_support::{scratch_pool, seed_run};
    use serde_json::json;

    const GOLDEN: &str = include_str!("../tests/fixtures/run-trace-envelope.golden.json");

    fn golden() -> Value {
        serde_json::from_str(GOLDEN).expect("golden json")
    }

    fn bytes(value: &Value) -> Vec<u8> {
        serde_json::to_vec(value).expect("json")
    }

    fn refused(mutate: impl FnOnce(&mut Value)) -> Invalid {
        let mut value = golden();
        mutate(&mut value);
        match validate_trace_envelope(&bytes(&value)) {
            Ok(_) => panic!("the message must be refused"),
            Err(invalid) => invalid,
        }
    }

    #[test]
    fn validate_accepts_the_run_trace_golden() {
        let event = validate_trace_envelope(GOLDEN.as_bytes()).expect("golden is valid");
        assert_eq!(event.event_type, "BROWSER_STEP_RECEIPT_RECORDED");
        assert_eq!(event.producer, TRACE_PRODUCER);
        assert_eq!(event.producer_seq, 7);
        assert_eq!(event.run_id, "01JC7K3M9Q4R8T2V5W6X7Y8Z9C");
        assert_eq!(event.org_id, "01JC7K0000ORG0000000000001");
        assert_eq!(event.origin, "AGENT");
        assert_eq!(event.request_id, "req_01JC7K3M9Q4R8T2V5W6X7Y8Z9D");
        assert_eq!(
            (
                event.agent_id.as_str(),
                event.agent_role.as_str(),
                event.agent_parent_id.as_str()
            ),
            ("general-v1", "subagent", "primary-v1")
        );
        assert_eq!(event.payload["outcome"], "ok");
    }

    #[test]
    fn validate_accepts_every_trace_event_type() {
        for name in TRACE_EVENT_TYPES {
            let mut value = golden();
            value["event_type"] = json!(name);
            assert!(validate_trace_envelope(&bytes(&value)).is_ok(), "{name}");
        }
    }

    #[test]
    fn validate_rejects_an_absent_zdr_key() {
        assert_eq!(
            refused(|v| {
                v.as_object_mut().unwrap().remove("zdr");
            }),
            Invalid::ZdrUndeclared
        );
        // A non-boolean posture is undeclared too, not "false".
        assert_eq!(
            refused(|v| v["zdr"] = json!("false")),
            Invalid::ZdrUndeclared
        );
        assert_eq!(refused(|v| v["zdr"] = json!(0)), Invalid::ZdrUndeclared);
        assert_eq!(refused(|v| v["zdr"] = Value::Null), Invalid::ZdrUndeclared);
    }

    #[test]
    fn validate_rejects_zdr_true() {
        assert_eq!(refused(|v| v["zdr"] = json!(true)), Invalid::ZdrTrue);
    }

    #[test]
    fn validate_rejects_a_foreign_event_type() {
        for other in [
            "STEP_COMPLETED",
            "RUN_STARTED",
            "BROWSER_ATTACH_SHARED_JAR",
            "browser_session_opened",
            "",
        ] {
            assert_eq!(
                refused(|v| v["event_type"] = json!(other)),
                Invalid::EventType,
                "{other:?}"
            );
        }
    }

    #[test]
    fn validate_rejects_a_foreign_producer() {
        for other in [
            "session-core",
            "execution-core",
            "Quarry-Edge",
            "",
            "quarry-edge2",
        ] {
            assert_eq!(
                refused(|v| v["producer"] = json!(other)),
                Invalid::Producer,
                "{other:?}"
            );
        }
    }

    #[test]
    fn validate_rejects_producer_seq_zero() {
        assert_eq!(
            refused(|v| v["producer_seq"] = json!(0)),
            Invalid::ProducerSeq
        );
        assert_eq!(
            refused(|v| v["producer_seq"] = Value::Null),
            Invalid::ProducerSeq
        );
        assert_eq!(
            refused(|v| {
                v.as_object_mut().unwrap().remove("producer_seq");
            }),
            Invalid::ProducerSeq
        );
        // Past i64: the column cannot hold it.
        assert_eq!(
            refused(|v| v["producer_seq"] = json!(u64::MAX)),
            Invalid::ProducerSeq
        );
    }

    #[test]
    fn validate_rejects_an_oversized_payload() {
        // Compact JSON under the cap but over it the way jsonb renders it (", " and ": ").
        let items: Vec<Value> = (0..3000).map(|n| json!(n % 10)).collect();
        assert_eq!(
            refused(|v| v["payload"] = json!({ "a": items })),
            Invalid::PayloadTooLarge
        );
        let big = "x".repeat(MAX_PAYLOAD_BYTES);
        assert_eq!(
            refused(|v| v["payload"] = json!({ "k": big })),
            Invalid::PayloadTooLarge
        );
        // Just inside the cap is stored: `{"k": "<n x>"}` is n + 9 bytes.
        let fits = "x".repeat(MAX_PAYLOAD_BYTES - 9);
        let mut value = golden();
        value["payload"] = json!({ "k": fits });
        assert!(validate_trace_envelope(&bytes(&value)).is_ok());
        // A message over the transport cap is refused before it is parsed.
        assert_eq!(
            validate_trace_envelope(&vec![b' '; MAX_MESSAGE_BYTES + 1]),
            Err(Invalid::MessageTooLarge)
        );
    }

    #[test]
    fn validate_rejects_a_dotted_run_id() {
        for bad in [
            "01JC7K.3M9Q",
            "run.id",
            "",
            "a b",
            "run*",
            "run>",
            "run/../x",
            &"r".repeat(MAX_ID_LEN + 1),
        ] {
            assert_eq!(
                refused(|v| v["run_id"] = json!(bad)),
                Invalid::RunId,
                "{bad:?}"
            );
        }
        assert_eq!(
            refused(|v| {
                v.as_object_mut().unwrap().remove("run_id");
            }),
            Invalid::RunId
        );
    }

    #[test]
    fn validate_rejects_what_postgres_would_refuse_forever() {
        // A NUL in text or in a jsonb string is a permanent insert error: terminate, never NAK.
        assert_eq!(
            refused(|v| v["resource_ref"] = json!("run/a\u{0}b")),
            Invalid::Nul
        );
        assert_eq!(
            refused(|v| v["payload"] = json!({ "k": ["fine", { "deep": "a\u{0}" }] })),
            Invalid::Nul
        );
        assert_eq!(refused(|v| v["origin"] = json!("ROBOT")), Invalid::Origin);
        assert_eq!(refused(|v| v["agent"]["id"] = json!("a.b")), Invalid::Agent);
        assert_eq!(
            refused(|v| v["payload"] = json!([1, 2])),
            Invalid::PayloadNotAnObject
        );
        assert_eq!(refused(|v| v["org_id"] = json!("")), Invalid::OrgId);
        assert_eq!(refused(|v| v["schema_version"] = json!(0)), Invalid::Shape);
        assert_eq!(validate_trace_envelope(b"not json"), Err(Invalid::NotJson));
        assert_eq!(validate_trace_envelope(b"[1]"), Err(Invalid::NotAnObject));
    }

    #[test]
    fn jsonb_text_len_matches_the_way_postgres_renders_jsonb() {
        // select '{"a":[1,2],"b":"x","c":null,"d":true,"e":false}'::jsonb::text
        //   = {"a": [1, 2], "b": "x", "c": null, "d": true, "e": false}
        let value = json!({"a": [1, 2], "b": "x", "c": null, "d": true, "e": false});
        assert_eq!(
            jsonb_text_len(&value),
            r#"{"a": [1, 2], "b": "x", "c": null, "d": true, "e": false}"#.len()
        );
        assert_eq!(jsonb_text_len(&json!({})), 2);
        assert_eq!(jsonb_text_len(&json!([])), 2);
    }

    #[test]
    fn the_trace_consumer_is_off_unless_the_switch_is_one_true_or_on() {
        assert!(!flag_enabled(None));
        for off in ["", "0", "false", "off", "yes", "TRUE", "2", "enabled"] {
            assert!(!flag_enabled(Some(off)), "{off:?}");
        }
        for on in ["1", "true", "on", " 1 "] {
            assert!(flag_enabled(Some(on)), "{on:?}");
        }
    }

    #[test]
    fn retention_defaults_to_thirty_days() {
        assert_eq!(retention_days_from(None), 30);
        assert_eq!(retention_days_from(Some("")), 30);
        assert_eq!(retention_days_from(Some("0")), 30);
        assert_eq!(retention_days_from(Some("-3")), 30);
        assert_eq!(retention_days_from(Some("soon")), 30);
        assert_eq!(retention_days_from(Some("7")), 7);
        assert_eq!(retention_days_from(Some(" 90 ")), 90);
    }

    #[test]
    fn outcomes_map_to_the_documented_dispositions() {
        assert_eq!(
            PersistOutcome::Inserted { gap_before: false }.disposition(),
            Disposition::Ack
        );
        assert_eq!(PersistOutcome::Duplicate.disposition(), Disposition::Ack);
        assert_eq!(
            PersistOutcome::RunUnknown.disposition(),
            Disposition::Nak(Duration::from_secs(2))
        );
        assert_eq!(PersistOutcome::OrgMismatch.disposition(), Disposition::Term);
        assert_eq!(PersistOutcome::ZdrRun.disposition(), Disposition::Term);
    }

    #[test]
    fn the_subject_names_exactly_one_run_token() {
        assert_eq!(subject_run_id("mp.v1.trace.run.abc-1"), Some("abc-1"));
        for bad in [
            "mp.v1.trace.run.",
            "mp.v1.trace.run.a.b",
            "mp.v1.trace.runs.a",
            "mp.v1.run.a",
            "",
        ] {
            assert_eq!(subject_run_id(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_stream_and_consumer_names_are_the_provisioned_contract() {
        // go/services/nats-provisioner/run_trace.go provisions exactly these.
        assert_eq!(STREAM_NAME, "MODEL_PLANE_RUN_TRACE");
        assert_eq!(DURABLE_NAME, "session-core-trace");
        assert_eq!(SUBJECT_FILTER, "mp.v1.trace.run.*");
    }

    // ---- Postgres (ignored): DATABASE_URL to a scratch database whose name says "test" ----

    /// The golden with this run's identity, as a validated event.
    fn event_for(run_id: &str, org: &str, seq: u64) -> TraceEvent {
        let mut value = golden();
        value["run_id"] = json!(run_id);
        value["org_id"] = json!(org);
        value["producer_seq"] = json!(seq);
        value["event_id"] = json!(format!("ev-{run_id}-{seq}"));
        value["resource_ref"] = json!(format!("run/{run_id}"));
        validate_trace_envelope(&bytes(&value)).expect("valid event")
    }

    async fn count(pool: &Pool, run_id: &str) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM run_trace_events WHERE run_id = $1")
            .bind(run_id)
            .fetch_one(pool)
            .await
            .expect("count")
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn trace_persist_inserts_one_row_per_event() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (run, org, user) = seed_run(&pool, "persistent").await;
        for seq in 1..=3 {
            let outcome = persist(&pool, &event_for(&run, &org, seq))
                .await
                .expect("persist");
            assert_eq!(
                outcome,
                PersistOutcome::Inserted { gap_before: false },
                "{seq}"
            );
        }
        assert_eq!(count(&pool, &run).await, 3);
        let (stored_org, stored_user, producer, origin, agent, seq_order): (
            String,
            String,
            String,
            String,
            String,
            Vec<i64>,
        ) = sqlx::query_as(
            "SELECT min(org_id), min(user_id), min(producer), min(origin), min(agent_id),
                    array_agg(producer_seq ORDER BY ingest_seq)
             FROM run_trace_events WHERE run_id = $1",
        )
        .bind(&run)
        .fetch_one(&pool)
        .await
        .expect("read back");
        // The tenant and the user are the run's.
        assert_eq!((stored_org, stored_user), (org, user));
        assert_eq!(
            (producer.as_str(), origin.as_str(), agent.as_str()),
            ("quarry-edge", "AGENT", "general-v1")
        );
        assert_eq!(seq_order, vec![1, 2, 3]);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn trace_persist_is_idempotent_on_redelivery() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (run, org, _) = seed_run(&pool, "persistent").await;
        let event = event_for(&run, &org, 1);
        assert_eq!(
            persist(&pool, &event).await.expect("first"),
            PersistOutcome::Inserted { gap_before: false }
        );
        for _ in 0..3 {
            assert_eq!(
                persist(&pool, &event).await.expect("again"),
                PersistOutcome::Duplicate
            );
        }
        // Same (run, producer, seq) under a different event id is still the same event.
        let mut renamed = event.clone();
        renamed.event_id = "ev-other".into();
        assert_eq!(
            persist(&pool, &renamed).await.expect("renamed"),
            PersistOutcome::Duplicate
        );
        assert_eq!(count(&pool, &run).await, 1);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn trace_persist_detects_a_producer_seq_gap() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (run, org, _) = seed_run(&pool, "persistent").await;
        let outcomes = [
            persist(&pool, &event_for(&run, &org, 1)).await.expect("1"),
            persist(&pool, &event_for(&run, &org, 2)).await.expect("2"),
            // 3 is missing.
            persist(&pool, &event_for(&run, &org, 4)).await.expect("4"),
            // 3 arrives late: stored, and 2 is there, so no gap before it.
            persist(&pool, &event_for(&run, &org, 3)).await.expect("3"),
        ];
        assert_eq!(
            outcomes,
            [
                PersistOutcome::Inserted { gap_before: false },
                PersistOutcome::Inserted { gap_before: false },
                PersistOutcome::Inserted { gap_before: true },
                PersistOutcome::Inserted { gap_before: false },
            ]
        );
        // Another run's seq 1 does not make this run's 5 look dense.
        let (other, other_org, _) = seed_run(&pool, "persistent").await;
        persist(&pool, &event_for(&other, &other_org, 1))
            .await
            .expect("other 1");
        assert_eq!(
            persist(&pool, &event_for(&run, &org, 6)).await.expect("6"),
            PersistOutcome::Inserted { gap_before: true }
        );
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn trace_persist_refuses_an_org_mismatch() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (run, _, _) = seed_run(&pool, "persistent").await;
        let outcome = persist(&pool, &event_for(&run, "some-other-org", 1))
            .await
            .expect("persist");
        assert_eq!(outcome, PersistOutcome::OrgMismatch);
        assert_eq!(count(&pool, &run).await, 0);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn trace_persist_refuses_an_unknown_run() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let outcome = persist(&pool, &event_for("no-such-run", "org-x", 1))
            .await
            .expect("persist");
        assert_eq!(outcome, PersistOutcome::RunUnknown);
        assert_eq!(
            outcome.disposition(),
            Disposition::Nak(Duration::from_secs(2))
        );
        let rows: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM run_trace_events WHERE run_id = 'no-such-run'",
        )
        .fetch_one(&pool)
        .await
        .expect("count");
        assert_eq!(rows, 0);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn trace_persist_refuses_a_zdr_run() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (zdr_run, zdr_org, _) = seed_run(&pool, "zdr").await;
        let outcome = persist(&pool, &event_for(&zdr_run, &zdr_org, 1))
            .await
            .expect("persist");
        assert_eq!(outcome, PersistOutcome::ZdrRun);
        assert_eq!(
            count(&pool, &zdr_run).await,
            0,
            "a zero-retention run never gets a row"
        );
        // A waived run is a persistent run: stored.
        let (waived_run, waived_org, _) = seed_run(&pool, "zdr_waived").await;
        assert_eq!(
            persist(&pool, &event_for(&waived_run, &waived_org, 1))
                .await
                .expect("waived"),
            PersistOutcome::Inserted { gap_before: false }
        );
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn retention_sweeper_deletes_only_old_rows_in_batches() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (run, org, _) = seed_run(&pool, "persistent").await;
        for seq in 1..=7 {
            persist(&pool, &event_for(&run, &org, seq))
                .await
                .expect("persist");
        }
        // Rows 1-5 are 40 days old, 6 is 29 days old, 7 is fresh.
        sqlx::query(
            "UPDATE run_trace_events SET received_at = now() - make_interval(days => \
             CASE WHEN producer_seq <= 5 THEN 40 WHEN producer_seq = 6 THEN 29 ELSE 0 END) \
             WHERE run_id = $1",
        )
        .bind(&run)
        .execute(&pool)
        .await
        .expect("age rows");
        // A batch of 2 needs three statements for five old rows (2 + 2 + 1).
        let deleted = sweep_once(&pool, 30, 2).await.expect("sweep");
        assert!(
            deleted >= 5,
            "at least this run's five old rows, got {deleted}"
        );
        let left: Vec<i64> = sqlx::query_scalar(
            "SELECT producer_seq FROM run_trace_events WHERE run_id = $1 ORDER BY producer_seq",
        )
        .bind(&run)
        .fetch_all(&pool)
        .await
        .expect("left");
        assert_eq!(left, vec![6, 7], "only the rows older than 30 days go");
        assert_eq!(
            sweep_once(&pool, 30, 2).await.expect("again"),
            0,
            "idempotent"
        );
    }
}
