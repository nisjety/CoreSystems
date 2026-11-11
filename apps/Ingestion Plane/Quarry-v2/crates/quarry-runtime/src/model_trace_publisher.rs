//! Bounded, ordered, ZDR-refusing publisher of Quarry browser-run events onto the
//! Model Plane run trace (WS10.S5).
//!
//! [`trace_projection::to_model_envelope`] decides WHAT may leave the process for
//! one event; this module decides whether and how it is sent:
//!
//! * [`ModelTracePublisher::try_enqueue`] never waits: it projects the event,
//!   serializes it and `try_send`s it onto a bounded queue. A ZDR run, a missing
//!   or invalid Model run id and an unmapped event are refused at the door; a full
//!   queue drops the event and counts it. The event stream (SSE, replay ring, the
//!   durable HTTP publisher) never waits on the broker.
//! * ONE publisher task drains the queue, so the order on the wire is the order
//!   of enqueue, which [`crate::events::EventSink`] keeps equal to `seq` order (it
//!   enqueues under the run's trace lock).
//! * The subject is `mp.v1.trace.run.<model_run_id>`; every message carries
//!   `Nats-Msg-Id: <bare event ULID>` so a JetStream redelivery deduplicates.
//! * [`ModelTracePublisher::shutdown`] flushes for at most two seconds.
//!
//! The publisher holds a publish-only principal (`quarry-trace-producer`); it has
//! no Model Plane storage access and reads nothing back. It exists only when
//! `QUARRY_EDGE__MODEL_TRACE_NATS_URL` is set (WS10.S5b wires it at boot), so it
//! is dark by default. Nothing here logs an envelope or a payload.

use std::collections::VecDeque;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_nats::jetstream::{self, context::PublishAckFuture};
use async_nats::{ConnectOptions, HeaderMap};
use async_trait::async_trait;
use bytes::Bytes;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use quarry_core::event::Event;
use quarry_core::{ErrorCode, QuarryError, QuarryResult};

use crate::trace_projection::{to_model_envelope, PublishCtx};

/// The subject prefix every trace message is published under.
pub const DEFAULT_SUBJECT_PREFIX: &str = "mp.v1.trace.run";
/// The default number of encoded envelopes the queue holds before it drops.
pub const DEFAULT_QUEUE_CAP: usize = 2048;
/// The default number of JetStream publishes awaiting their PubAck at once.
pub const DEFAULT_ACK_WINDOW: usize = 32;
/// The longest [`ModelTracePublisher::shutdown`] waits to flush, whatever the caller asks.
pub const MAX_SHUTDOWN_FLUSH: Duration = Duration::from_secs(2);
/// The JetStream deduplication header.
pub const MSG_ID_HEADER: &str = "Nats-Msg-Id";
/// The client name the trace connection announces (the publish-only principal).
pub const TRACE_CLIENT_NAME: &str = "quarry-trace-producer";

/// Configuration of the trace publisher's NATS connection and queue.
#[derive(Clone)]
pub struct ModelTraceConfig {
    /// The NATS server URL (`QUARRY_EDGE__MODEL_TRACE_NATS_URL`).
    pub url: String,
    /// The publish-only principal's user name, when the server needs one.
    pub user: Option<String>,
    /// The publish-only principal's password, when the server needs one.
    pub password: Option<String>,
    /// The subject prefix; the subject is `<prefix>.<model_run_id>`.
    pub subject_prefix: String,
    /// The bound of the in-process queue; a full queue drops and counts.
    pub queue_cap: usize,
    /// How many publishes may await their PubAck at once.
    pub ack_window: usize,
}

impl ModelTraceConfig {
    /// A configuration for `url` with the defaults and no credentials.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            user: None,
            password: None,
            subject_prefix: DEFAULT_SUBJECT_PREFIX.to_string(),
            queue_cap: DEFAULT_QUEUE_CAP,
            ack_window: DEFAULT_ACK_WINDOW,
        }
    }

    /// Set the publish-only principal's credentials.
    pub fn with_user_password(
        mut self,
        user: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        self.user = Some(user.into());
        self.password = Some(password.into());
        self
    }
}

// The password never reaches a log line.
impl fmt::Debug for ModelTraceConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModelTraceConfig")
            .field("url", &self.url)
            .field("user", &self.user)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("subject_prefix", &self.subject_prefix)
            .field("queue_cap", &self.queue_cap)
            .field("ack_window", &self.ack_window)
            .finish()
    }
}

/// Where encoded trace envelopes go. Called from the publisher's single task only,
/// so calls arrive in queue order.
#[async_trait]
pub trait TraceTransport: Send + Sync + 'static {
    /// Publish one encoded envelope on `subject` with the deduplication id `msg_id`.
    async fn publish(&self, subject: &str, msg_id: &str, bytes: Bytes) -> QuarryResult<()>;

    /// Wait for every publish still awaiting its acknowledgement. The default has
    /// nothing in flight.
    async fn flush(&self) -> QuarryResult<()> {
        Ok(())
    }
}

/// JetStream transport: `publish_with_headers` with `Nats-Msg-Id`, keeping at most
/// `ack_window` publishes awaiting their PubAck.
pub struct NatsTraceTransport {
    js: jetstream::Context,
    ack_window: usize,
    in_flight: tokio::sync::Mutex<VecDeque<PublishAckFuture>>,
}

impl NatsTraceTransport {
    /// Connect with the configuration's URL and credentials.
    pub async fn connect(config: &ModelTraceConfig) -> QuarryResult<Self> {
        let mut opts = ConnectOptions::new()
            .name(TRACE_CLIENT_NAME)
            .connection_timeout(Duration::from_secs(5));
        if let (Some(user), Some(password)) = (&config.user, &config.password) {
            opts = opts.user_and_password(user.clone(), password.clone());
        }
        let client = opts.connect(&config.url).await.map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                format!("model trace nats connect failed: {e}"),
            )
        })?;
        Ok(Self::from_context(
            jetstream::new(client),
            config.ack_window,
        ))
    }

    /// A transport over an existing JetStream context.
    pub fn from_context(js: jetstream::Context, ack_window: usize) -> Self {
        Self {
            js,
            ack_window: ack_window.max(1),
            in_flight: tokio::sync::Mutex::new(VecDeque::new()),
        }
    }
}

async fn await_ack(ack: PublishAckFuture) -> QuarryResult<()> {
    ack.await.map(|_| ()).map_err(|e| {
        QuarryError::new(
            ErrorCode::DriverFailed,
            format!("model trace publish not acknowledged: {e}"),
        )
    })
}

#[async_trait]
impl TraceTransport for NatsTraceTransport {
    async fn publish(&self, subject: &str, msg_id: &str, bytes: Bytes) -> QuarryResult<()> {
        let mut in_flight = self.in_flight.lock().await;
        // Keep the window: settle the oldest acknowledgements first. A failed
        // earlier ack belongs to an earlier message, so it is logged, not returned.
        while in_flight.len() >= self.ack_window {
            if let Some(oldest) = in_flight.pop_front() {
                if let Err(e) = await_ack(oldest).await {
                    // zdr-log-ok: a NATS client error (stream, connection state); no envelope or payload is part of it; publisher task, no request span
                    tracing::warn!(error = %e, "model trace publish not acknowledged");
                }
            }
        }
        let mut headers = HeaderMap::new();
        headers.insert(MSG_ID_HEADER, msg_id);
        let ack = self
            .js
            .publish_with_headers(subject.to_string(), headers, bytes)
            .await
            .map_err(|e| {
                QuarryError::new(
                    ErrorCode::DriverFailed,
                    format!("model trace publish failed: {e}"),
                )
            })?;
        in_flight.push_back(ack);
        Ok(())
    }

    async fn flush(&self) -> QuarryResult<()> {
        let mut in_flight = self.in_flight.lock().await;
        let mut result = Ok(());
        while let Some(ack) = in_flight.pop_front() {
            if let Err(e) = await_ack(ack).await {
                result = Err(e);
            }
        }
        result
    }
}

/// Why [`ModelTracePublisher::try_enqueue`] did not queue an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceRefusal {
    /// The run is ZDR: nothing leaves the process.
    Zdr,
    /// The run has no valid Model run id.
    NoModelRun,
    /// The projection maps nothing for this event (default deny, `seq` 0, bad org).
    Unmapped,
    /// The queue was full; the event was dropped and counted.
    QueueFull,
    /// The publisher is shutting down.
    Closed,
}

struct QueuedTrace {
    subject: String,
    msg_id: String,
    bytes: Bytes,
}

/// Publishes projected run-trace envelopes to the Model Plane through one task.
pub struct ModelTracePublisher {
    tx: mpsc::Sender<QueuedTrace>,
    subject_prefix: String,
    dropped: Arc<AtomicU64>,
    published: Arc<AtomicU64>,
    failed: Arc<AtomicU64>,
    stop: CancellationToken,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl ModelTracePublisher {
    /// Start the publisher task over `transport`. Must be called inside a Tokio runtime.
    pub fn start(config: &ModelTraceConfig, transport: Arc<dyn TraceTransport>) -> Self {
        let (tx, rx) = mpsc::channel(config.queue_cap.max(1));
        let published = Arc::new(AtomicU64::new(0));
        let failed = Arc::new(AtomicU64::new(0));
        let stop = CancellationToken::new();
        let task = tokio::spawn(run_publisher(
            rx,
            transport,
            published.clone(),
            failed.clone(),
            stop.clone(),
        ));
        Self {
            tx,
            subject_prefix: config.subject_prefix.trim_end_matches('.').to_string(),
            dropped: Arc::new(AtomicU64::new(0)),
            published,
            failed,
            stop,
            task: Mutex::new(Some(task)),
        }
    }

    /// Connect the JetStream transport from `config` and start the publisher.
    pub async fn connect(config: &ModelTraceConfig) -> QuarryResult<Self> {
        let transport = NatsTraceTransport::connect(config).await?;
        Ok(Self::start(config, Arc::new(transport)))
    }

    /// Project `event` for the run in `ctx` and queue it. Never waits: a ZDR run, a
    /// missing or invalid Model run id and an unmapped event are refused, and a full
    /// queue drops the event (counted in [`ModelTracePublisher::dropped`]).
    pub fn try_enqueue(&self, event: &Event, ctx: &PublishCtx) -> Result<(), TraceRefusal> {
        if ctx.zdr {
            return Err(TraceRefusal::Zdr);
        }
        if !quarry_core::trace::valid_token(&ctx.model_run_id, quarry_core::trace::TRACE_TOKEN_MAX)
        {
            return Err(TraceRefusal::NoModelRun);
        }
        if self.stop.is_cancelled() {
            return Err(TraceRefusal::Closed);
        }
        let envelope = to_model_envelope(event, ctx).ok_or(TraceRefusal::Unmapped)?;
        let bytes = serde_json::to_vec(&envelope).map_err(|_| TraceRefusal::Unmapped)?;
        let queued = QueuedTrace {
            subject: format!("{}.{}", self.subject_prefix, ctx.model_run_id),
            msg_id: event.event_id.ulid().to_string(),
            bytes: Bytes::from(bytes),
        };
        match self.tx.try_send(queued) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                Err(TraceRefusal::QueueFull)
            }
            Err(mpsc::error::TrySendError::Closed(_)) => Err(TraceRefusal::Closed),
        }
    }

    /// Events dropped because the queue was full.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Events the transport accepted.
    pub fn published(&self) -> u64 {
        self.published.load(Ordering::Relaxed)
    }

    /// Events the transport refused, plus a failed final flush.
    pub fn failed(&self) -> u64 {
        self.failed.load(Ordering::Relaxed)
    }

    /// Stop taking events and flush what is queued, waiting at most `deadline`
    /// (capped at [`MAX_SHUTDOWN_FLUSH`]). Whatever is still queued after that is
    /// abandoned. Idempotent.
    pub async fn shutdown(&self, deadline: Duration) {
        self.stop.cancel();
        let task = self
            .task
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        let Some(mut task) = task else {
            return;
        };
        let deadline = deadline.min(MAX_SHUTDOWN_FLUSH);
        if tokio::time::timeout(deadline, &mut task).await.is_err() {
            task.abort();
            tracing::warn!(
                dropped = self.dropped(),
                published = self.published(),
                "model trace flush hit its deadline; queued events abandoned"
            );
        }
    }
}

impl Drop for ModelTracePublisher {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

/// The single publisher task: queue order is wire order.
async fn run_publisher(
    mut rx: mpsc::Receiver<QueuedTrace>,
    transport: Arc<dyn TraceTransport>,
    published: Arc<AtomicU64>,
    failed: Arc<AtomicU64>,
    stop: CancellationToken,
) {
    loop {
        let next = tokio::select! {
            biased;
            next = rx.recv() => next,
            () = stop.cancelled() => None,
        };
        let Some(item) = next else {
            break;
        };
        send_one(&*transport, item, &published, &failed).await;
    }
    // Drain what was queued before the stop, then settle the acknowledgements.
    rx.close();
    while let Ok(item) = rx.try_recv() {
        send_one(&*transport, item, &published, &failed).await;
    }
    if let Err(e) = transport.flush().await {
        failed.fetch_add(1, Ordering::Relaxed);
        // zdr-log-ok: a NATS client error (subject, connection state); no envelope or payload is part of it; publisher task, no request span
        tracing::warn!(error = %e, "model trace flush failed");
    }
}

async fn send_one(
    transport: &dyn TraceTransport,
    item: QueuedTrace,
    published: &AtomicU64,
    failed: &AtomicU64,
) {
    match transport
        .publish(&item.subject, &item.msg_id, item.bytes)
        .await
    {
        Ok(()) => {
            published.fetch_add(1, Ordering::Relaxed);
        }
        Err(e) => {
            failed.fetch_add(1, Ordering::Relaxed);
            // zdr-log-ok: a NATS client error (subject, connection state); no envelope or payload is part of it; publisher task, no request span
            tracing::warn!(error = %e, "model trace publish failed");
        }
    }
}
