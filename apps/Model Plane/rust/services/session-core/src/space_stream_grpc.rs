//! `SpaceEvents.StreamSpaceEvents`: the per-Space event tail (build plan WS02-S3).
//!
//! Serves rows of the Space event log (migration 0072, written by triggers) to one
//! subscriber at a time, as ids and closed words only. The log is the source of truth: the
//! in-process wake ([`SpaceWakes`], fed by the Postgres `LISTEN` task) only removes latency, so
//! a lost wake costs a 30 s floor poll and never a row.
//!
//! Authority, in order, all fail closed:
//!
//! 1. the caller is a user principal whose id equals `user_id` (a service token is refused);
//! 2. the Control decision verifies for the audience `model-plane-space-stream`
//!    ([`verify_space_stream_decision`]); a read decision (`model-plane`) never opens a stream;
//! 3. a Zero Data Retention caller or decision gets `connected{mode: "poll_only"}`, then
//!    `closing{zdr_poll_only}`, and no log row is ever read;
//! 4. every row and every heartbeat goes through the pure [`gate`]: lease expiry, membership
//!    revocation (the `space_membership_revocations` table, behind a 2 s cache), the decision's
//!    audience-revision ceiling, an unknown scope or kind (stubbed `hidden`).
//!
//! Nothing in this module is an authorization input by itself; the verified decision is, and the
//! lease bounds how long one decision keeps a stream open.
//!
//! Stream reads use their own small pool (`SPACE_STREAM_DB_POOL`, default 8) so streams cannot
//! starve the main pool, and the number of open streams is capped (`SPACE_STREAM_MAX_STREAMS`,
//! default 5000 per replica).

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, SecondsFormat, Utc};
use mp_contracts::model_plane::v1 as pb;
use mp_contracts::model_plane::v1::space_event_frame::Frame;
use mp_contracts::model_plane::v1::space_events_server::SpaceEvents;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use tokio::sync::{broadcast, mpsc};
use tokio::time::{interval_at, Instant as TokioInstant};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tracing::warn;

use crate::auth::{authorize_operation, identity};
use crate::grpc::verify_space_stream_decision;
use crate::space::events::{
    gate, run_state, CloseCode, GateContext, GateOutcome, GateRow, SpaceWakes, Wake,
};

const STREAM_POOL_ENV: &str = "SPACE_STREAM_DB_POOL";
const MAX_STREAMS_ENV: &str = "SPACE_STREAM_MAX_STREAMS";
const DEFAULT_STREAM_POOL: usize = 8;
const DEFAULT_MAX_STREAMS: usize = 5000;
const HEARTBEAT: Duration = Duration::from_secs(10);
const FLOOR_POLL: Duration = Duration::from_secs(30);
const SEND_TIMEOUT: Duration = Duration::from_secs(15);
const REVOCATION_CACHE_TTL: Duration = Duration::from_secs(2);
const REVOCATION_CACHE_MAX: usize = 20_000;
const BATCH: i64 = 200;
const CHANNEL_DEPTH: usize = 64;
/// Lease: the shorter of the decision's expiry minus this margin and 85 to 100 s (jittered).
const EXPIRY_MARGIN_SECONDS: i64 = 10;
const LEASE_BASE_SECONDS: u64 = 85;
const LEASE_JITTER_SECONDS: u64 = 15;
const MAX_SPACE_ID_BYTES: usize = 128;

/// Kinds the v1 registry knows. An unknown kind is forwarded only as a `hidden` stub.
const KNOWN_KINDS: [&str; 7] = [
    "thread.created",
    "message.appended",
    "thread.updated",
    "thread.archived",
    "thread.deleted",
    "run.state_changed",
    "redacted",
];
const FIELD_WORDS: [&str; 4] = ["title", "preview", "pinned", "persona"];

type RevocationKey = (String, String, String);

struct Shared {
    pool: PgPool,
    wakes: SpaceWakes,
    active: AtomicUsize,
    max_streams: usize,
    revocations: Mutex<HashMap<RevocationKey, (Instant, Option<DateTime<Utc>>)>>,
}

/// Handler state for `SpaceEvents`. Cheap to clone-share: everything lives behind one `Arc`.
pub(crate) struct SpaceEventsGrpc {
    shared: Arc<Shared>,
}

impl SpaceEventsGrpc {
    /// `fallback` is used for stream reads only when the dedicated pool cannot be configured.
    pub(crate) fn new(fallback: PgPool, wakes: SpaceWakes) -> Self {
        let pool = stream_pool(fallback);
        let max_streams = env_usize(MAX_STREAMS_ENV, DEFAULT_MAX_STREAMS).max(1);
        Self {
            shared: Arc::new(Shared {
                pool,
                wakes,
                active: AtomicUsize::new(0),
                max_streams,
                revocations: Mutex::new(HashMap::new()),
            }),
        }
    }
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(default)
}

/// The dedicated, lazily connected pool for stream reads.
fn stream_pool(fallback: PgPool) -> PgPool {
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgresql://postgres:postgres@localhost:5432/session_core".to_owned()
    });
    let size = u32::try_from(env_usize(STREAM_POOL_ENV, DEFAULT_STREAM_POOL).clamp(1, 64))
        .unwrap_or(8);
    match PgPoolOptions::new()
        .max_connections(size)
        .acquire_timeout(Duration::from_secs(5))
        .connect_lazy(&url)
    {
        Ok(pool) => pool,
        Err(error) => {
            warn!(%error, "space stream pool could not be configured; using the main pool");
            fallback
        }
    }
}

/// Holds one stream slot; releases it (and updates the gauge) on drop.
struct ActiveGuard {
    shared: Arc<Shared>,
}

impl ActiveGuard {
    fn acquire(shared: &Arc<Shared>) -> Option<Self> {
        let previous = shared.active.fetch_add(1, Ordering::SeqCst);
        if previous >= shared.max_streams {
            shared.active.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        metrics::gauge!("mp_session_space_stream_active").set((previous + 1) as f64);
        Some(Self {
            shared: shared.clone(),
        })
    }
}

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        let previous = self.shared.active.fetch_sub(1, Ordering::SeqCst);
        metrics::gauge!("mp_session_space_stream_active").set(previous.saturating_sub(1) as f64);
    }
}

#[tonic::async_trait]
impl SpaceEvents for SpaceEventsGrpc {
    type StreamSpaceEventsStream = ReceiverStream<Result<pb::SpaceEventFrame, Status>>;

    async fn stream_space_events(
        &self,
        request: Request<pb::StreamSpaceEventsRequest>,
    ) -> Result<Response<Self::StreamSpaceEventsStream>, Status> {
        let caller = identity(&request)?;
        // A read: a ZDR caller passes here and is answered `poll_only` below.
        authorize_operation(&caller, "session:read")?;
        let req = request.into_inner();
        caller.authorize_org(&req.org_id)?;
        // A user principal equal to `user_id`; a service identity has no user id and is refused.
        caller.authorize_user(&req.user_id)?;
        let space_id = req.space_id.trim().to_owned();
        if space_id.is_empty() || space_id.len() > MAX_SPACE_ID_BYTES {
            return Err(Status::invalid_argument("space_id is required"));
        }
        let decision = verify_space_stream_decision(
            &req.decision_token,
            &req.decision_ref,
            &req.org_id,
            &req.user_id,
            &space_id,
            Utc::now(),
        )?;

        let (tx, rx) = mpsc::channel(CHANNEL_DEPTH);

        if caller.zdr() || decision.zero_data_retention {
            // No log row is read for a ZDR caller or decision.
            let _ = tx.try_send(Ok(frame(Frame::Connected(pb::SpaceStreamConnected {
                generation: 0,
                head: 0,
                resumed: false,
                lease_seconds: 0,
                heartbeat_seconds: 0,
                mode: "poll_only".to_owned(),
                cursor: 0,
            }))));
            let _ = tx.try_send(Ok(frame(closing_frame(CloseCode::ZdrPollOnly, 0, 0))));
            count_frame("connected");
            count_frame("closing");
            count_closed("zdr_poll_only");
            return Ok(Response::new(ReceiverStream::new(rx)));
        }

        let guard = ActiveGuard::acquire(&self.shared).ok_or_else(|| {
            Status::resource_exhausted("space event stream capacity reached; retry shortly")
        })?;

        // Subscribe to wakes BEFORE reading the head, so a commit between the head read and
        // the first drain cannot be missed.
        let wake_rx = self.shared.wakes.subscribe();
        let head = read_head(&self.shared.pool, &req.org_id, &space_id)
            .await
            .map_err(|_| Status::unavailable("space event log is unavailable"))?;
        let (generation, head_seq, min_seq) = head.unwrap_or((1, 0, 1));

        let after = if req.resume {
            if req.generation != generation {
                return too_old(tx, rx, "generation_changed", head_seq, generation);
            }
            if req.after_seq > head_seq {
                return too_old(tx, rx, "ahead_of_head", head_seq, generation);
            }
            if req.after_seq < 0 || req.after_seq + 1 < min_seq {
                return too_old(tx, rx, "retention", head_seq, generation);
            }
            req.after_seq
        } else {
            head_seq
        };

        let now = Utc::now();
        let lease = lease_duration(now, decision.expires_at, jitter_seconds());
        let state = StreamState {
            shared: self.shared.clone(),
            _guard: guard,
            tx,
            wake_rx,
            org_id: req.org_id,
            space_id,
            user_id: req.user_id,
            generation,
            after,
            head_hint: head_seq,
            resumed: req.resume,
            ceiling: decision.recipient_audience_revision,
            issued_at: decision.issued_at,
            lease,
            lease_until: now
                + chrono::Duration::from_std(lease).unwrap_or_else(|_| chrono::Duration::seconds(1)),
        };
        metrics::counter!("mp_session_space_stream_opened_total").increment(1);
        tokio::spawn(async move {
            let reason = state.drive().await;
            count_closed(reason);
        });
        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

/// Answers `cursor_too_old` as the only frame, after authority was verified.
#[allow(clippy::result_large_err)]
fn too_old(
    tx: mpsc::Sender<Result<pb::SpaceEventFrame, Status>>,
    rx: mpsc::Receiver<Result<pb::SpaceEventFrame, Status>>,
    reason: &str,
    head: i64,
    generation: i64,
) -> Result<Response<ReceiverStream<Result<pb::SpaceEventFrame, Status>>>, Status> {
    let _ = tx.try_send(Ok(frame(Frame::CursorTooOld(pb::SpaceStreamCursorTooOld {
        reason: reason.to_owned(),
        head,
        generation,
    }))));
    count_frame("cursor_too_old");
    count_closed("cursor_too_old");
    Ok(Response::new(ReceiverStream::new(rx)))
}

fn frame(inner: Frame) -> pb::SpaceEventFrame {
    pb::SpaceEventFrame { frame: Some(inner) }
}

fn count_frame(kind: &'static str) {
    metrics::counter!("mp_session_space_stream_frames_total", "kind" => kind).increment(1);
}

fn count_closed(reason: &'static str) {
    metrics::counter!("mp_session_space_stream_closed_total", "reason" => reason).increment(1);
}

/// The closing word of a gate close, and whether the client should reconnect.
pub(crate) fn close_word(code: CloseCode) -> (&'static str, bool) {
    match code {
        CloseCode::LeaseExpired => ("lease_expired", true),
        CloseCode::Reauthorize => ("reauthorize", true),
        CloseCode::MembershipRevoked => ("membership_revoked", false),
        CloseCode::ZdrPollOnly => ("zdr_poll_only", false),
    }
}

fn closing_frame(code: CloseCode, generation: i64, cursor: i64) -> Frame {
    let (word, reconnect) = close_word(code);
    Frame::Closing(pb::SpaceStreamClosing {
        code: word.to_owned(),
        reconnect,
        retry_after_ms: 0,
        generation,
        cursor,
    })
}

/// Lease length: the shorter of the decision's expiry minus 10 s and `85 + jitter` seconds,
/// never below one second (a decision that is about to expire gets a lease that closes at once
/// and a fresh mint, not a refusal).
pub(crate) fn lease_duration(
    now: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    jitter_seconds: u64,
) -> Duration {
    let cap = Duration::from_secs(LEASE_BASE_SECONDS + jitter_seconds.min(LEASE_JITTER_SECONDS));
    let remaining = (expires_at - chrono::Duration::seconds(EXPIRY_MARGIN_SECONDS) - now)
        .to_std()
        .unwrap_or(Duration::ZERO);
    cap.min(remaining).max(Duration::from_secs(1))
}

fn jitter_seconds() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| u64::from(elapsed.subsec_nanos()));
    nanos % (LEASE_JITTER_SECONDS + 1)
}

/// `(generation, head_seq, min_seq)` of the Space, or `None` before its first row.
async fn read_head(
    pool: &PgPool,
    org_id: &str,
    space_id: &str,
) -> Result<Option<(i64, i64, i64)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT generation, head_seq, min_seq FROM space_event_heads
          WHERE org_id = $1 AND space_ref = $2",
    )
    .bind(org_id)
    .bind(space_id)
    .fetch_optional(pool)
    .await
}

/// One log row, as read.
struct LogRow {
    seq: i64,
    generation: i64,
    kind: String,
    scope: String,
    actor_type: String,
    actor_subject_id: Option<String>,
    thread_id: Option<String>,
    run_id: Option<String>,
    message_id: Option<String>,
    audience_revision: Option<i64>,
    retention_class: String,
    refs: serde_json::Value,
    created_at: DateTime<Utc>,
}

async fn fetch_rows(
    pool: &PgPool,
    org_id: &str,
    space_id: &str,
    after: i64,
) -> Result<Vec<LogRow>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT seq, generation, kind, scope, actor_type, actor_subject_id, thread_id, run_id,
                message_id, audience_revision, retention_class, refs, created_at
           FROM space_event_log
          WHERE org_id = $1 AND space_ref = $2 AND seq > $3
          ORDER BY seq
          LIMIT $4",
    )
    .bind(org_id)
    .bind(space_id)
    .bind(after)
    .bind(BATCH)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|row| {
            Ok(LogRow {
                seq: row.try_get("seq")?,
                generation: row.try_get("generation")?,
                kind: row.try_get("kind")?,
                scope: row.try_get("scope")?,
                actor_type: row.try_get("actor_type")?,
                actor_subject_id: row.try_get("actor_subject_id")?,
                thread_id: row.try_get("thread_id")?,
                run_id: row.try_get("run_id")?,
                message_id: row.try_get("message_id")?,
                audience_revision: row.try_get("audience_revision")?,
                retention_class: row.try_get("retention_class")?,
                refs: row.try_get("refs")?,
                created_at: row.try_get("created_at")?,
            })
        })
        .collect()
}

/// A short closed word: lower-case letters, digits and underscore.
fn closed_word(value: &str) -> Option<String> {
    (!value.is_empty()
        && value.len() <= 40
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'))
    .then(|| value.to_owned())
}

/// An id carried in `refs`: bounded and free of whitespace and control characters.
fn safe_id(value: &str) -> Option<String> {
    (!value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':' | b'.')))
    .then(|| value.to_owned())
}

fn ref_str(refs: &serde_json::Value, key: &str) -> Option<String> {
    refs.get(key).and_then(serde_json::Value::as_str).map(str::to_owned)
}

/// The wire record of a row that passed the gate. Only allowlisted `refs` keys are copied,
/// each through its own closed-word or id filter, so a stray key in the log can never reach
/// the wire.
fn record_from_row(row: &LogRow) -> pb::SpaceEventRecord {
    let actor_type = match row.actor_type.as_str() {
        "user" | "agent" | "system" => row.actor_type.clone(),
        _ => "system".to_owned(),
    };
    let fields = row
        .refs
        .get("fields")
        .and_then(serde_json::Value::as_array)
        .map(|words| {
            words
                .iter()
                .filter_map(serde_json::Value::as_str)
                .filter(|word| FIELD_WORDS.contains(word))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    pb::SpaceEventRecord {
        seq: row.seq,
        generation: row.generation,
        kind: row.kind.clone(),
        at: row.created_at.to_rfc3339_opts(SecondsFormat::Millis, true),
        actor_type,
        actor_id: row
            .actor_subject_id
            .as_deref()
            .and_then(safe_id)
            .unwrap_or_default(),
        thread_id: row.thread_id.as_deref().and_then(safe_id).unwrap_or_default(),
        run_id: row.run_id.as_deref().and_then(safe_id).unwrap_or_default(),
        message_id: row.message_id.as_deref().and_then(safe_id).unwrap_or_default(),
        message_sequence: row
            .refs
            .get("message_sequence")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or_default(),
        role: ref_str(&row.refs, "role")
            .as_deref()
            .and_then(closed_word)
            .unwrap_or_default(),
        agent_id: ref_str(&row.refs, "agent_id")
            .as_deref()
            .and_then(safe_id)
            .unwrap_or_default(),
        state: ref_str(&row.refs, "state")
            .map(|state| run_state(&state).to_owned())
            .unwrap_or_default(),
        prev_state: ref_str(&row.refs, "prev")
            .map(|state| run_state(&state).to_owned())
            .unwrap_or_default(),
        origin: ref_str(&row.refs, "origin")
            .as_deref()
            .and_then(closed_word)
            .unwrap_or_default(),
        fields,
        parent_run_id: ref_str(&row.refs, "parent_run_id")
            .as_deref()
            .and_then(safe_id)
            .unwrap_or_default(),
    }
}

/// A `hidden` stub: only seq, generation, kind and time, so the client's seq stays contiguous.
fn hidden_stub(row: &LogRow) -> pb::SpaceEventRecord {
    pb::SpaceEventRecord {
        seq: row.seq,
        generation: row.generation,
        kind: "hidden".to_owned(),
        at: row.created_at.to_rfc3339_opts(SecondsFormat::Millis, true),
        actor_type: "system".to_owned(),
        ..Default::default()
    }
}

struct StreamState {
    shared: Arc<Shared>,
    _guard: ActiveGuard,
    tx: mpsc::Sender<Result<pb::SpaceEventFrame, Status>>,
    wake_rx: broadcast::Receiver<Wake>,
    org_id: String,
    space_id: String,
    user_id: String,
    generation: i64,
    /// Highest seq fully processed. The stream sends rows with `seq > after`.
    after: i64,
    /// Highest seq known to be committed (from the head read, wakes and rows). Heartbeats
    /// report it without a database read.
    head_hint: i64,
    resumed: bool,
    /// Highest `recipient_audience_revision` the decision authorizes.
    ceiling: i64,
    issued_at: DateTime<Utc>,
    lease: Duration,
    lease_until: DateTime<Utc>,
}

impl StreamState {
    /// Runs the stream to its end and returns the closed reason word for the metrics.
    async fn drive(mut self) -> &'static str {
        let connected = Frame::Connected(pb::SpaceStreamConnected {
            generation: self.generation,
            head: self.head_hint,
            resumed: self.resumed,
            lease_seconds: i32::try_from(self.lease.as_secs()).unwrap_or(i32::MAX),
            heartbeat_seconds: i32::try_from(HEARTBEAT.as_secs()).unwrap_or(10),
            mode: "live".to_owned(),
            cursor: self.after,
        });
        if let Some(reason) = self.emit(connected, "connected").await {
            return reason;
        }
        // Drain once at once: a row committed after the head read but before the first wake
        // must not wait for the floor poll.
        if let Some(reason) = self.drain().await {
            return reason;
        }

        let start = TokioInstant::now();
        let mut heartbeat = interval_at(start + HEARTBEAT, HEARTBEAT);
        let mut floor = interval_at(start + FLOOR_POLL, FLOOR_POLL);
        loop {
            let lease_left = (self.lease_until - Utc::now())
                .to_std()
                .unwrap_or(Duration::ZERO);
            let mut beat = false;
            tokio::select! {
                () = self.tx.closed() => return "client_gone",
                () = tokio::time::sleep(lease_left) => {
                    return self.close(CloseCode::LeaseExpired).await;
                }
                _ = heartbeat.tick() => { beat = true; }
                _ = floor.tick() => {}
                wake = self.wake_rx.recv() => match wake {
                    Ok(Wake::Row { org_id, space_ref, seq, generation }) => {
                        if org_id != self.org_id || space_ref != self.space_id {
                            continue;
                        }
                        if generation == self.generation {
                            self.head_hint = self.head_hint.max(seq);
                        }
                    }
                    Ok(Wake::All) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return "wake_closed",
                },
            }
            if beat {
                // The per-heartbeat gate: stream-wide closes only (lease, revocation, ZDR).
                let revoked_at = match self.revoked_at().await {
                    Ok(value) => value,
                    Err(_) => return self.fail_unavailable().await,
                };
                let probe = GateRow {
                    org_id: &self.org_id,
                    scope: "space",
                    audience_revision: Some(0),
                    retention_class: "persistent",
                };
                if let GateOutcome::Close(code) = gate(&probe, &self.gate_context(revoked_at)) {
                    return self.close(code).await;
                }
            }
            if let Some(reason) = self.drain().await {
                return reason;
            }
            if beat {
                let heartbeat_frame = Frame::Heartbeat(pb::SpaceStreamHeartbeat {
                    head: self.head_hint.max(self.after),
                    generation: self.generation,
                    cursor: self.after,
                });
                if let Some(reason) = self.emit(heartbeat_frame, "heartbeat").await {
                    return reason;
                }
            }
        }
    }

    fn gate_context(&self, revoked_at: Option<DateTime<Utc>>) -> GateContext<'_> {
        GateContext {
            org_id: &self.org_id,
            now: Utc::now(),
            lease_until: self.lease_until,
            decision_ceiling: self.ceiling,
            decision_issued_at: self.issued_at,
            revoked_at,
            // ZDR callers and decisions never reach a stream task.
            caller_zdr: false,
            decision_zdr: false,
        }
    }

    /// Reads and delivers every row after the cursor, a batch at a time. Returns the closed
    /// reason when the stream must end.
    async fn drain(&mut self) -> Option<&'static str> {
        loop {
            let rows =
                match fetch_rows(&self.shared.pool, &self.org_id, &self.space_id, self.after).await
                {
                    Ok(rows) => rows,
                    Err(_) => return Some(self.fail_unavailable().await),
                };
            if rows.is_empty() {
                return None;
            }
            let revoked_at = match self.revoked_at().await {
                Ok(value) => value,
                Err(_) => return Some(self.fail_unavailable().await),
            };
            let full = rows.len() as i64 >= BATCH;
            for row in &rows {
                // A hole or a generation change means this cursor can no longer be trusted:
                // the client resnapshots instead of guessing.
                if row.generation != self.generation {
                    return Some(self.cursor_too_old("generation_changed").await);
                }
                if row.seq != self.after + 1 {
                    return Some(self.cursor_too_old("retention").await);
                }
                let verdict = gate(
                    &GateRow {
                        org_id: &self.org_id,
                        scope: &row.scope,
                        audience_revision: row.audience_revision,
                        retention_class: &row.retention_class,
                    },
                    &self.gate_context(revoked_at),
                );
                let record = match verdict {
                    GateOutcome::Close(code) => return Some(self.close(code).await),
                    GateOutcome::Stub => hidden_stub(row),
                    GateOutcome::Deliver if KNOWN_KINDS.contains(&row.kind.as_str()) => {
                        record_from_row(row)
                    }
                    GateOutcome::Deliver => hidden_stub(row),
                };
                if let Some(reason) = self.emit(Frame::Event(record), "event").await {
                    return Some(reason);
                }
                self.after = row.seq;
                self.head_hint = self.head_hint.max(row.seq);
            }
            if !full {
                return None;
            }
        }
    }

    /// Sends one frame with the slow-consumer timeout. `Some(reason)` ends the stream.
    async fn emit(&self, inner: Frame, kind: &'static str) -> Option<&'static str> {
        match self
            .tx
            .send_timeout(Ok(frame(inner)), SEND_TIMEOUT)
            .await
        {
            Ok(()) => {
                count_frame(kind);
                None
            }
            Err(mpsc::error::SendTimeoutError::Timeout(_)) => {
                let _ = self.tx.try_send(Ok(frame(Frame::Closing(pb::SpaceStreamClosing {
                    code: "slow_consumer".to_owned(),
                    reconnect: true,
                    retry_after_ms: 0,
                    generation: self.generation,
                    cursor: self.after,
                }))));
                Some("slow_consumer")
            }
            Err(mpsc::error::SendTimeoutError::Closed(_)) => Some("client_gone"),
        }
    }

    async fn close(&self, code: CloseCode) -> &'static str {
        let frame_kind = closing_frame(code, self.generation, self.after);
        let _ = self.emit(frame_kind, "closing").await;
        close_word(code).0
    }

    async fn cursor_too_old(&self, reason: &'static str) -> &'static str {
        let too_old = Frame::CursorTooOld(pb::SpaceStreamCursorTooOld {
            reason: reason.to_owned(),
            head: self.head_hint.max(self.after),
            generation: self.generation,
        });
        let _ = self.emit(too_old, "cursor_too_old").await;
        "cursor_too_old"
    }

    /// A database failure ends the stream with `UNAVAILABLE`; the model-gateway maps it to
    /// an upstream error and the client reconnects with its cursor.
    async fn fail_unavailable(&self) -> &'static str {
        let _ = self
            .tx
            .send(Err(Status::unavailable("space event log is unavailable")))
            .await;
        "db_unavailable"
    }

    /// `revoked_at` of this subject's revocation row for the Space, behind a 2 s cache shared
    /// by all streams, so idle cost is bounded by distinct Space and member pairs.
    async fn revoked_at(&self) -> Result<Option<DateTime<Utc>>, sqlx::Error> {
        let key: RevocationKey = (
            self.org_id.clone(),
            self.space_id.clone(),
            self.user_id.clone(),
        );
        if let Ok(cache) = self.shared.revocations.lock() {
            if let Some((at, value)) = cache.get(&key) {
                if at.elapsed() < REVOCATION_CACHE_TTL {
                    return Ok(*value);
                }
            }
        }
        let value: Option<DateTime<Utc>> = sqlx::query_scalar(
            "SELECT revoked_at FROM space_membership_revocations
              WHERE space_ref = $1 AND org_id = $2 AND subject_id = $3",
        )
        .bind(&self.space_id)
        .bind(&self.org_id)
        .bind(&self.user_id)
        .fetch_optional(&self.shared.pool)
        .await?;
        if let Ok(mut cache) = self.shared.revocations.lock() {
            if cache.len() >= REVOCATION_CACHE_MAX {
                cache.retain(|_, (at, _)| at.elapsed() < REVOCATION_CACHE_TTL);
            }
            cache.insert(key, (Instant::now(), value));
        }
        Ok(value)
    }
}
