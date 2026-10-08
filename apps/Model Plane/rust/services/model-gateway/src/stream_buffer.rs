//! Delta buffer for resumable chat streams (`HARNESS_PHASE1` §3b).
//!
//! As `invoke_stream_sse` produces deltas it appends them here keyed by
//! `request_id`, tagged with the same monotonic seq used as the SSE `id:`
//! line. On reconnect/reload a client calls `/v1/invoke/resume/{request_id}`
//! with `Last-Event-Id`; the resume handler replays buffered deltas with
//! `seq > last_event_id` and the final `done` chunk if the stream finished.
//!
//! Two backends, selected at startup by `REDIS_URL`:
//!   - **Redis** (multi-replica): a list per request + a done key, both with a
//!     TTL. A reconnect that lands on a different gateway replica still
//!     resumes, because the buffer is shared.
//!   - **In-memory** (single-replica / dev / Redis unavailable): a bounded,
//!     TTL-swept `Mutex<HashMap>`.
//!
//! Methods are async so both backends share one signature.

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use tracing::warn;

/// Max deltas retained per stream. A 1024-token reply rarely exceeds a few
/// hundred SSE chunks; cap well above that and drop the oldest beyond it.
const MAX_DELTAS_PER_STREAM: usize = 4_096;

/// How long a finished/idle stream stays resumable before eviction.
const STREAM_TTL: Duration = Duration::from_secs(600);
const STREAM_TTL_SECS: i64 = 600;

const REDIS_PREFIX: &str = "mp:gw:stream:";

/// Separator between the identity components of a buffer key. ASCII unit
/// separator, which cannot occur in an org id, a user id, or a ULID request
/// id — so no combination of those values can be made to collide with a
/// different combination.
const KEY_SEPARATOR: char = '\u{1f}';

/// The key a stream's buffered deltas live under.
///
/// **The identity prefix is the access control.** A resume names only a
/// `request_id`, and that id is not a secret: it is emitted in the `connected`
/// SSE event, echoed in every chunk, and used as a URL path segment, so it
/// reaches browser history, proxy logs, and screenshots. Keying the buffer on
/// the request id alone therefore let any authenticated caller — including one
/// from another tenant — replay someone else's assistant output for the whole
/// TTL window.
///
/// Deriving the key from the *verified* caller identity closes that by
/// construction: a caller from another org (or another user in the same org)
/// computes a different key, misses, and gets the same 404 as an unknown id.
/// There is no comparison to forget and no owner record to look up.
#[must_use]
pub fn scoped_stream_key(org_id: &str, user_id: &str, request_id: &str) -> String {
    format!("{org_id}{KEY_SEPARATOR}{user_id}{KEY_SEPARATOR}{request_id}")
}

/// A single buffered SSE frame plus its sequence id.
///
/// # Rich events, and reading records written before them
///
/// This buffer originally held only assistant text, so a reconnect replayed the
/// answer and silently lost every rich event — tool calls, citations, usage,
/// the generated title (parity doc §4.1, "the one real stream gap"). It now
/// buffers the frame: an event NAME and its `data:` payload, which makes it
/// general over every current and future event rather than a list to keep in
/// step.
///
/// `event`/`data` are `#[serde(default)]` and `delta` is retained so records
/// written by an older build — already sitting in Redis/Dragonfly under a
/// 10-minute TTL when the new build starts — still deserialize instead of
/// failing the whole replay. A legacy record has an empty `event` and a
/// populated `delta`; [`BufferedEvent::is_legacy_text`] identifies it, and the
/// resume handler rebuilds the `chunk` frame it used to send. Without this a
/// deploy would 500 every in-flight resume for the length of one TTL.
#[derive(Clone, Serialize, Deserialize)]
pub struct BufferedEvent {
    pub seq: u64,
    /// SSE event name. Empty only in legacy text-delta records.
    #[serde(default)]
    pub event: String,
    /// The SSE `data:` payload, verbatim as it was sent.
    #[serde(default)]
    pub data: String,
    /// Legacy text delta. Written by builds before rich buffering; never
    /// written now, which is why it is skipped when empty rather than
    /// serialized as `""` on every frame.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub delta: String,
}

impl BufferedEvent {
    /// A record from before rich buffering: no event name, text in `delta`.
    #[must_use]
    pub fn is_legacy_text(&self) -> bool {
        self.event.is_empty()
    }
}

/// Terminal info captured when a stream completes, so a late resumer still
/// receives a correct `done` event (lost-final-chunk handling).
#[derive(Clone, Serialize, Deserialize)]
pub struct StreamDone {
    pub seq: u64,
    pub model_used: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// Recorded when a stream ends on an `error` or `stopped` frame instead of
/// `done`, so a resume can tell a finished failure from a producer that is
/// still working. It is not a frame: the terminal frame itself is buffered
/// like any other, and replaying this marker sends nothing.
///
/// Kept apart from [`StreamDone`] on purpose. A failed stream must never be
/// replayed as `done`, and a build that predates this record reads only the
/// `done` key, so it can never mistake an ended stream for a completed one.
#[derive(Clone, Serialize, Deserialize)]
pub struct StreamEnd {
    /// The cursor just past the stream's last buffered frame, as
    /// [`StreamDone::seq`] is for `done`.
    pub seq: u64,
}

/// What a resume needs: deltas after the cursor and the terminal chunk.
pub struct Replay {
    pub deltas: Vec<BufferedEvent>,
    pub done: Option<StreamDone>,
    /// Present once the stream ended without `done`, whatever the cursor:
    /// there is nothing left to follow.
    pub ended: Option<StreamEnd>,
    /// False when the `request_id` is unknown (evicted or never existed).
    pub found: bool,
}

// ---------------------------------------------------------------------------
// In-memory backend
// ---------------------------------------------------------------------------

struct BufferedStream {
    deltas: Vec<BufferedEvent>,
    done: Option<StreamDone>,
    ended: Option<StreamEnd>,
    updated: Instant,
}

impl BufferedStream {
    fn empty() -> Self {
        Self {
            deltas: Vec::new(),
            done: None,
            ended: None,
            updated: Instant::now(),
        }
    }
}

#[derive(Clone, Default)]
pub struct InMemoryStreamBuffer {
    inner: Arc<Mutex<HashMap<String, BufferedStream>>>,
}

impl InMemoryStreamBuffer {
    fn append(&self, request_id: &str, seq: u64, event: &str, data: &str) {
        let Ok(mut map) = self.inner.lock() else {
            return;
        };
        Self::sweep(&mut map);
        let entry = map
            .entry(request_id.to_owned())
            .or_insert_with(BufferedStream::empty);
        entry.deltas.push(BufferedEvent {
            seq,
            event: event.to_owned(),
            data: data.to_owned(),
            delta: String::new(),
        });
        if entry.deltas.len() > MAX_DELTAS_PER_STREAM {
            let overflow = entry.deltas.len() - MAX_DELTAS_PER_STREAM;
            entry.deltas.drain(0..overflow);
        }
        entry.updated = Instant::now();
    }

    fn finish(&self, request_id: &str, done: StreamDone) {
        let Ok(mut map) = self.inner.lock() else {
            return;
        };
        let entry = map
            .entry(request_id.to_owned())
            .or_insert_with(BufferedStream::empty);
        entry.done = Some(done);
        entry.updated = Instant::now();
    }

    fn end(&self, request_id: &str, end: StreamEnd) {
        let Ok(mut map) = self.inner.lock() else {
            return;
        };
        let entry = map
            .entry(request_id.to_owned())
            .or_insert_with(BufferedStream::empty);
        entry.ended = Some(end);
        entry.updated = Instant::now();
    }

    fn replay_after(&self, request_id: &str, after_seq: Option<u64>) -> Replay {
        let Ok(map) = self.inner.lock() else {
            return Replay {
                deltas: Vec::new(),
                done: None,
                ended: None,
                found: false,
            };
        };
        match map.get(request_id) {
            None => Replay {
                deltas: Vec::new(),
                done: None,
                ended: None,
                found: false,
            },
            Some(entry) => Replay {
                deltas: entry
                    .deltas
                    .iter()
                    .filter(|d| after_seq.is_none_or(|a| d.seq > a))
                    .cloned()
                    .collect(),
                // `done` is itself an SSE frame and carries the terminal
                // sequence id.  Once a client has acknowledged that cursor,
                // replaying it again creates a duplicate terminal frame (and
                // some clients treat the duplicate as a second completed
                // turn).  Keep the terminal frame in the buffer for clients
                // resuming before it, but make the cursor contract apply to
                // it just like it does to deltas.
                done: entry
                    .done
                    .as_ref()
                    .filter(|done| after_seq.is_none_or(|a| done.seq > a))
                    .cloned(),
                // Not a frame, so no cursor applies: a client that already
                // saw the terminal error still learns the stream is over.
                ended: entry.ended.clone(),
                found: true,
            },
        }
    }

    fn sweep(map: &mut HashMap<String, BufferedStream>) {
        let now = Instant::now();
        map.retain(|_, s| now.duration_since(s.updated) < STREAM_TTL);
    }
}

// ---------------------------------------------------------------------------
// Redis backend
// ---------------------------------------------------------------------------

/// Longest one Redis command may take before it is given up on.
///
/// The producer awaits every frame's append inline, so this is also the longest
/// a frame can be held up by the buffer. The connection manager's own defaults
/// have no response or connect timeout at all: with the store hung (paused
/// container, stalled host) an append never returned, which froze the answer it
/// belonged to for as long as the store stayed down (37.9 s measured against a
/// paused Dragonfly 1.37), and with the store gone every append waited out a
/// full reconnect cycle. A healthy append measured 2 to 20 ms through a
/// published port, so this is generous.
const REDIS_OP_TIMEOUT: Duration = Duration::from_millis(250);

/// After a Redis command fails or times out, Redis is left alone for this long.
/// Without it an outage costs [`REDIS_OP_TIMEOUT`] on every frame of every
/// stream. With it, an outage costs one bounded attempt per cooldown.
const REDIS_COOLDOWN: Duration = Duration::from_secs(5);

/// How long the connection manager waits to connect before it gives up, and how
/// often it retries one reconnect cycle. Short, because [`REDIS_COOLDOWN`] owns
/// the pacing of retries.
const REDIS_CONNECT_TIMEOUT: Duration = Duration::from_secs(1);

/// Bounded connection settings (see [`REDIS_OP_TIMEOUT`]).
fn redis_manager_config() -> redis::aio::ConnectionManagerConfig {
    redis::aio::ConnectionManagerConfig::new()
        .set_connection_timeout(REDIS_CONNECT_TIMEOUT)
        .set_response_timeout(REDIS_OP_TIMEOUT)
        .set_number_of_retries(1)
}

/// Keeps a slow or dead Redis from stalling the streams that write to it.
///
/// Every command runs under [`REDIS_OP_TIMEOUT`]. The first failure or timeout
/// opens the breaker for [`REDIS_COOLDOWN`]; while it is open commands are not
/// sent at all and report "no result". It closes by itself: the first command
/// after the cooldown is the probe, and a success needs no bookkeeping.
///
/// What an open breaker costs: frames written during it are not buffered, so a
/// resume that needs them (after a crash, or on another replica) falls back to
/// the run-status path instead of replaying. That is the right trade against an
/// answer that stops moving.
#[derive(Clone)]
struct Breaker {
    origin: tokio::time::Instant,
    open_until_ms: Arc<AtomicU64>,
}

impl Breaker {
    fn new() -> Self {
        Self {
            origin: tokio::time::Instant::now(),
            open_until_ms: Arc::new(AtomicU64::new(0)),
        }
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.origin.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn is_open(&self) -> bool {
        self.now_ms() < self.open_until_ms.load(Ordering::Relaxed)
    }

    fn trip(&self) {
        let cooldown = u64::try_from(REDIS_COOLDOWN.as_millis()).unwrap_or(u64::MAX);
        self.open_until_ms
            .store(self.now_ms().saturating_add(cooldown), Ordering::Relaxed);
    }

    /// Run one Redis command under the breaker. `op` is only called when the
    /// breaker is closed, so a skipped command does no work. The log lines
    /// carry the operation, not the stream key: that holds org and user ids.
    async fn run<T, Fut>(&self, what: &'static str, op: impl FnOnce() -> Fut) -> Option<T>
    where
        Fut: Future<Output = redis::RedisResult<T>>,
    {
        if self.is_open() {
            return None;
        }
        match tokio::time::timeout(REDIS_OP_TIMEOUT, op()).await {
            Ok(Ok(value)) => Some(value),
            Ok(Err(e)) => {
                warn!(
                    error = %e,
                    op = what,
                    cooldown_secs = REDIS_COOLDOWN.as_secs(),
                    "redis stream op failed; not using redis for the cooldown"
                );
                self.trip();
                None
            }
            Err(_) => {
                warn!(
                    op = what,
                    timeout_ms = u64::try_from(REDIS_OP_TIMEOUT.as_millis()).unwrap_or(u64::MAX),
                    cooldown_secs = REDIS_COOLDOWN.as_secs(),
                    "redis stream op timed out; not using redis for the cooldown"
                );
                self.trip();
                None
            }
        }
    }
}

#[derive(Clone)]
pub struct RedisStreamBuffer {
    conn: redis::aio::ConnectionManager,
    breaker: Breaker,
}

impl RedisStreamBuffer {
    fn list_key(request_id: &str) -> String {
        format!("{REDIS_PREFIX}{request_id}")
    }
    fn done_key(request_id: &str) -> String {
        format!("{REDIS_PREFIX}{request_id}:done")
    }
    fn ended_key(request_id: &str) -> String {
        format!("{REDIS_PREFIX}{request_id}:ended")
    }

    async fn append(&self, request_id: &str, seq: u64, event: &str, data: &str) {
        let key = Self::list_key(request_id);
        let Ok(entry) = serde_json::to_string(&BufferedEvent {
            seq,
            event: event.to_owned(),
            data: data.to_owned(),
            delta: String::new(),
        }) else {
            return;
        };
        let cap = isize::try_from(MAX_DELTAS_PER_STREAM).unwrap_or(isize::MAX);
        // RPUSH + LTRIM to cap + EXPIRE in a pipeline. A failure is logged by
        // the breaker and does not fail the stream.
        self.breaker
            .run("append", || {
                let mut conn = self.conn.clone();
                async move {
                    redis::pipe()
                        .rpush(&key, entry)
                        .ignore()
                        .ltrim(&key, -cap, -1)
                        .ignore()
                        .expire(&key, STREAM_TTL_SECS)
                        .ignore()
                        .query_async::<()>(&mut conn)
                        .await
                }
            })
            .await;
    }

    async fn finish(&self, request_id: &str, done: StreamDone) {
        let key = Self::done_key(request_id);
        let Ok(value) = serde_json::to_string(&done) else {
            return;
        };
        self.breaker
            .run("finish", || {
                let mut conn = self.conn.clone();
                async move {
                    conn.set_ex::<_, _, ()>(&key, value, STREAM_TTL_SECS as u64)
                        .await
                }
            })
            .await;
    }

    async fn end(&self, request_id: &str, end: StreamEnd) {
        let key = Self::ended_key(request_id);
        let Ok(value) = serde_json::to_string(&end) else {
            return;
        };
        self.breaker
            .run("end", || {
                let mut conn = self.conn.clone();
                async move {
                    conn.set_ex::<_, _, ()>(&key, value, STREAM_TTL_SECS as u64)
                        .await
                }
            })
            .await;
    }

    async fn replay_after(&self, request_id: &str, after_seq: Option<u64>) -> Replay {
        let list_key = Self::list_key(request_id);
        let done_key = Self::done_key(request_id);
        let ended_key = Self::ended_key(request_id);

        // The terminal records are read BEFORE the frames. The producer writes
        // every frame before either record, so a record seen here guarantees
        // the list read below holds the frames before it. Read the other way
        // round, a terminal landing between the two reads ended a resume
        // without its last frames.
        //
        // Both reads run under the breaker. If either fails or Redis is being
        // left alone, the stream is reported unknown, the same answer an
        // evicted stream gets, and the caller falls back to the run-status path.
        let raw_terminal: Option<Vec<Option<String>>> = self
            .breaker
            .run("replay_terminal", || {
                let mut conn = self.conn.clone();
                let keys = [done_key.clone(), ended_key.clone()];
                async move { conn.mget(&keys).await }
            })
            .await;
        let raw: Option<Vec<String>> = match raw_terminal {
            Some(_) => {
                self.breaker
                    .run("replay_frames", || {
                        let mut conn = self.conn.clone();
                        let key = list_key.clone();
                        async move { conn.lrange(&key, 0, -1).await }
                    })
                    .await
            }
            None => None,
        };

        let (Some(raw_terminal), Some(entries)) = (raw_terminal, raw) else {
            return Replay {
                deltas: Vec::new(),
                done: None,
                ended: None,
                found: false,
            };
        };
        let mut raw_terminal = raw_terminal.into_iter();
        let raw_done = raw_terminal.next().flatten();
        let raw_ended = raw_terminal.next().flatten();
        // A terminal stream remains a known stream even when the caller's
        // cursor is already at (or beyond) the terminal frame. Keep `found`
        // independent from the cursor-filtered payload so Redis and the
        // in-memory backend agree: a valid, fully-acknowledged stream is not a
        // 404 just because it has nothing left to replay.
        let parsed_done = raw_done.and_then(|s| serde_json::from_str::<StreamDone>(&s).ok());
        let done_present = parsed_done.is_some();
        let done = parsed_done.filter(|done| after_seq.is_none_or(|a| done.seq > a));
        let ended = raw_ended.and_then(|s| serde_json::from_str::<StreamEnd>(&s).ok());

        let found = !entries.is_empty() || done_present || ended.is_some();
        let deltas = entries
            .iter()
            .filter_map(|s| serde_json::from_str::<BufferedEvent>(s).ok())
            .filter(|d| after_seq.is_none_or(|a| d.seq > a))
            .collect();

        Replay {
            deltas,
            done,
            ended,
            found,
        }
    }
}

// ---------------------------------------------------------------------------
// Public store (backend-agnostic)
// ---------------------------------------------------------------------------

/// Cheaply-cloneable handle to the resume buffer.
#[derive(Clone)]
pub enum StreamBufferStore {
    Memory(InMemoryStreamBuffer),
    Redis(Box<RedisStreamBuffer>),
}

impl StreamBufferStore {
    /// In-memory backend (default, tests, single-replica).
    #[must_use]
    pub fn new() -> Self {
        Self::Memory(InMemoryStreamBuffer::default())
    }

    /// Connect to Redis when `REDIS_URL` is set, else fall back to in-memory.
    /// Never fails — a Redis outage degrades to single-replica resume.
    #[must_use]
    pub async fn from_env() -> Self {
        let Ok(url) = std::env::var("REDIS_URL") else {
            return Self::new();
        };
        match redis::Client::open(url) {
            Ok(client) => {
                match redis::aio::ConnectionManager::new_with_config(client, redis_manager_config())
                    .await
                {
                    Ok(conn) => {
                        tracing::info!("stream buffer: Redis backend active");
                        Self::Redis(Box::new(RedisStreamBuffer {
                            conn,
                            breaker: Breaker::new(),
                        }))
                    }
                    Err(e) => {
                        warn!(error = %e, "REDIS_URL set but connect failed; using in-memory stream buffer");
                        Self::new()
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "invalid REDIS_URL; using in-memory stream buffer");
                Self::new()
            }
        }
    }

    /// Buffer one SSE frame so a reconnect can replay it.
    ///
    /// `event` is the SSE event name and `data` its payload verbatim. Taking the
    /// frame rather than just text is what makes resume lossless for rich events
    /// and keeps this store general over events that do not exist yet — the same
    /// no-allowlist property the Verevon gateway's relay has.
    pub async fn append(&self, request_id: &str, seq: u64, event: &str, data: &str) {
        match self {
            Self::Memory(m) => m.append(request_id, seq, event, data),
            Self::Redis(r) => r.append(request_id, seq, event, data).await,
        }
    }

    pub async fn finish(&self, request_id: &str, done: StreamDone) {
        match self {
            Self::Memory(m) => m.finish(request_id, done),
            Self::Redis(r) => r.finish(request_id, done).await,
        }
    }

    /// Record that the stream ended on an `error` or `stopped` frame, after
    /// that frame was buffered. A resume then stops at the end of what is
    /// buffered instead of tailing a stream that will never write again, and
    /// is never shown `done`.
    pub async fn end(&self, request_id: &str, end: StreamEnd) {
        match self {
            Self::Memory(m) => m.end(request_id, end),
            Self::Redis(r) => r.end(request_id, end).await,
        }
    }

    #[must_use]
    pub async fn replay_after(&self, request_id: &str, after_seq: Option<u64>) -> Replay {
        match self {
            Self::Memory(m) => m.replay_after(request_id, after_seq),
            Self::Redis(r) => r.replay_after(request_id, after_seq).await,
        }
    }
}

impl Default for StreamBufferStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn append_then_replay_after_cursor() {
        let store = StreamBufferStore::new();
        store.append("req_1", 0, "chunk", "a").await;
        store.append("req_1", 1, "chunk", "b").await;
        store.append("req_1", 2, "chunk", "c").await;

        let all = store.replay_after("req_1", None).await;
        assert!(all.found);
        assert_eq!(all.deltas.len(), 3);

        let after0 = store.replay_after("req_1", Some(0)).await;
        assert_eq!(after0.deltas.len(), 2);
        assert_eq!(after0.deltas[0].seq, 1);
    }

    #[tokio::test]
    async fn finish_is_replayed() {
        let store = StreamBufferStore::new();
        store.append("req_2", 0, "chunk", "hi").await;
        store
            .finish(
                "req_2",
                StreamDone {
                    seq: 1,
                    model_used: "m".into(),
                    input_tokens: 3,
                    output_tokens: 4,
                },
            )
            .await;
        let r = store.replay_after("req_2", Some(0)).await;
        assert!(r.found);
        assert!(r.done.is_some());
        assert_eq!(r.done.unwrap().output_tokens, 4);
    }

    #[tokio::test]
    async fn terminal_frame_is_not_replayed_after_its_cursor() {
        let store = StreamBufferStore::new();
        store
            .append("req_terminal_cursor", 0, "chunk", "answer")
            .await;
        store
            .finish(
                "req_terminal_cursor",
                StreamDone {
                    seq: 1,
                    model_used: "m".into(),
                    input_tokens: 1,
                    output_tokens: 1,
                },
            )
            .await;

        let replay = store.replay_after("req_terminal_cursor", Some(1)).await;
        assert!(replay.found);
        assert!(replay.deltas.is_empty());
        assert!(
            replay.done.is_none(),
            "a client that acknowledged done must not receive a duplicate terminal frame"
        );

        let before_done = store.replay_after("req_terminal_cursor", Some(0)).await;
        assert!(before_done.done.is_some());
    }

    #[tokio::test]
    async fn unknown_request_is_not_found() {
        let store = StreamBufferStore::new();
        let r = store.replay_after("nope", None).await;
        assert!(!r.found);
        assert!(r.ended.is_none());
    }

    /// A stream that failed after its answer was buffered must never replay as
    /// `done`, and a client that already saw its error must still learn it is
    /// over rather than tailing a stream nothing will ever write to again.
    #[tokio::test]
    async fn an_ended_stream_is_finished_but_never_done() {
        let store = StreamBufferStore::new();
        store.append("req_failed", 0, "chunk", "answer").await;
        store
            .append("req_failed", 1, "error", "{\"code\":\"x\"}")
            .await;
        assert!(store.replay_after("req_failed", None).await.ended.is_none());
        store.end("req_failed", StreamEnd { seq: 2 }).await;

        let replay = store.replay_after("req_failed", None).await;
        assert!(replay.found);
        assert!(replay.done.is_none(), "a failed stream is never completed");
        assert_eq!(replay.ended.expect("ended").seq, 2);
        assert_eq!(replay.deltas.len(), 2);

        let acknowledged = store.replay_after("req_failed", Some(1)).await;
        assert!(acknowledged.deltas.is_empty());
        assert!(
            acknowledged.ended.is_some(),
            "the end marker is not a frame, so no cursor filters it"
        );
    }

    #[tokio::test]
    async fn a_stream_that_ended_before_buffering_anything_is_still_known() {
        let store = StreamBufferStore::new();
        store.end("req_early", StreamEnd { seq: 0 }).await;
        let replay = store.replay_after("req_early", None).await;
        assert!(replay.found);
        assert!(replay.ended.is_some());
    }

    #[tokio::test]
    async fn delta_cap_evicts_oldest() {
        let store = StreamBufferStore::new();
        for i in 0..(MAX_DELTAS_PER_STREAM as u64 + 10) {
            store.append("req_cap", i, "chunk", "x").await;
        }
        let r = store.replay_after("req_cap", None).await;
        assert_eq!(r.deltas.len(), MAX_DELTAS_PER_STREAM);
        assert_eq!(r.deltas[0].seq, 10);
    }

    /// Regression: a buffered stream was addressable by `request_id` alone, so
    /// A record written by a build that buffered text only must still
    /// deserialize and still replay, or a deploy 500s every in-flight resume for
    /// the length of one TTL (10 minutes) while those records age out.
    #[test]
    fn legacy_text_records_still_deserialize() {
        let legacy = r#"{"seq":7,"delta":"half an answer"}"#;
        let frame: BufferedEvent =
            serde_json::from_str(legacy).expect("a legacy record must still parse");
        assert_eq!(frame.seq, 7);
        assert_eq!(frame.delta, "half an answer");
        assert!(
            frame.is_legacy_text(),
            "no event name means the resume handler must rebuild the chunk frame"
        );
        assert!(frame.event.is_empty() && frame.data.is_empty());
    }

    /// Any event, including ones that do not exist yet: the buffer stores the
    /// frame, so it never needs a list of known events to keep in step with.
    #[tokio::test]
    async fn any_event_name_round_trips() {
        let store = StreamBufferStore::new();
        let key = scoped_stream_key("org", "user", "req_rich");
        for (seq, (event, data)) in [
            ("chunk", r#"{"delta":"hi"}"#),
            ("tool_call", r#"{"id":"1","name":"shell"}"#),
            ("memory_recall", r#"{"count":3}"#),
            ("an_event_invented_next_year", r#"{"x":1}"#),
        ]
        .iter()
        .enumerate()
        {
            store.append(&key, seq as u64, event, data).await;
        }
        let replay = store.replay_after(&key, None).await;
        let names: Vec<&str> = replay.deltas.iter().map(|f| f.event.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "chunk",
                "tool_call",
                "memory_recall",
                "an_event_invented_next_year"
            ],
            "the buffer must not filter or rename events"
        );
        assert_eq!(replay.deltas[2].data, r#"{"count":3}"#);
    }

    /// any authenticated caller who saw an id — and it travels in the
    /// `connected` event, every chunk, and the cancel URL — could replay
    /// another tenant's assistant output for the whole TTL window.
    #[tokio::test]
    async fn a_stream_never_replays_for_another_org_or_user() {
        let store = StreamBufferStore::new();
        let request_id = "req_shared_id";
        let owner = scoped_stream_key("org_a", "user_a", request_id);
        store.append(&owner, 0, "chunk", "tenant a's answer").await;

        // The owner still resumes.
        let mine = store.replay_after(&owner, None).await;
        assert!(mine.found);
        assert_eq!(mine.deltas[0].data, "tenant a's answer");
        assert_eq!(mine.deltas[0].event, "chunk");
        assert!(
            !mine.deltas[0].is_legacy_text(),
            "a freshly written frame is never a legacy record"
        );

        // Another tenant knowing the exact request id derives a different key.
        let other_org = scoped_stream_key("org_b", "user_a", request_id);
        assert!(!store.replay_after(&other_org, None).await.found);

        // So does a different user inside the SAME tenant.
        let colleague = scoped_stream_key("org_a", "user_b", request_id).clone();
        assert!(!store.replay_after(&colleague, None).await.found);

        // And the bare id — the pre-fix key — addresses nothing at all.
        assert!(!store.replay_after(request_id, None).await.found);
    }

    /// Release-shaped Redis proof. The shell harness runs this test once to
    /// write a completed stream, restarts the Redis container, then runs it
    /// again in read mode. Keeping the test ignored avoids requiring Redis for
    /// normal unit suites while still making the cross-replica/restart claim
    /// executable in CI and release verification.
    #[tokio::test]
    #[ignore = "requires REDIS_URL and REDIS_DURABILITY_PHASE=write|read"]
    async fn redis_resume_survives_store_restart_and_preserves_identity_scope() {
        let phase = std::env::var("REDIS_DURABILITY_PHASE").expect("REDIS_DURABILITY_PHASE");
        let request_id =
            std::env::var("REDIS_DURABILITY_REQUEST_ID").expect("REDIS_DURABILITY_REQUEST_ID");
        let owner = scoped_stream_key("org_redis_e2e", "user_redis_e2e", &request_id);
        let store = StreamBufferStore::from_env().await;
        assert!(
            matches!(store, StreamBufferStore::Redis(_)),
            "REDIS_URL must resolve"
        );

        match phase.as_str() {
            "write" => {
                store.append(&owner, 0, "chunk", "persisted answer").await;
                store
                    .finish(
                        &owner,
                        StreamDone {
                            seq: 1,
                            model_used: "redis-e2e".to_owned(),
                            input_tokens: 2,
                            output_tokens: 3,
                        },
                    )
                    .await;
                let replay = store.replay_after(&owner, Some(0)).await;
                assert!(replay.found);
                assert_eq!(replay.deltas.len(), 0, "cursor 0 filters seq 0");
                assert!(replay.done.is_some());
            }
            "read" => {
                let replay = store.replay_after(&owner, Some(0)).await;
                assert!(replay.found, "completed stream must survive Redis restart");
                assert_eq!(replay.deltas.len(), 0, "cursor 0 filters seq 0");
                assert_eq!(replay.done.expect("terminal frame").seq, 1);
                let acknowledged = store.replay_after(&owner, Some(1)).await;
                assert!(acknowledged.found);
                assert!(acknowledged.done.is_none());

                let other = scoped_stream_key("org_other", "user_redis_e2e", &request_id);
                assert!(!store.replay_after(&other, None).await.found);
            }
            invalid => panic!("unsupported REDIS_DURABILITY_PHASE={invalid}"),
        }
    }

    #[test]
    fn scoped_keys_cannot_be_made_to_collide_across_identities() {
        // Without a separator that cannot occur in the components, an org
        // ending in part of a user id could address another pair's stream.
        assert_ne!(
            scoped_stream_key("org", "a", "b"),
            scoped_stream_key("org", "a\u{1f}b", "")
        );
        assert_ne!(
            scoped_stream_key("orga", "user", "req"),
            scoped_stream_key("org", "auser", "req")
        );
    }

    // -- The breaker: a hung or dead Redis must not stall the stream ----------

    /// A command that never answers (a paused or stalled store, which the
    /// connection manager's default settings wait on forever) is cut off at
    /// `REDIS_OP_TIMEOUT`; after that Redis is not asked again until the
    /// cooldown has passed, however many frames follow; the first command after
    /// the cooldown is the probe, and its success closes the breaker.
    #[tokio::test(start_paused = true)]
    async fn a_hung_command_is_cut_off_and_the_breaker_then_stops_asking() {
        use std::sync::atomic::AtomicUsize;
        let breaker = Breaker::new();
        let asked = Arc::new(AtomicUsize::new(0));
        let before = tokio::time::Instant::now();

        let hung: Option<()> = breaker
            .run("append", || {
                asked.fetch_add(1, Ordering::SeqCst);
                std::future::pending::<redis::RedisResult<()>>()
            })
            .await;
        assert!(hung.is_none());
        assert_eq!(
            before.elapsed(),
            REDIS_OP_TIMEOUT,
            "a hung command holds one frame up for the timeout and no longer"
        );
        assert!(breaker.is_open());

        // The rest of the answer's frames: not sent, not waited for.
        for _ in 0..1_000 {
            let skipped: Option<()> = breaker
                .run("append", || {
                    asked.fetch_add(1, Ordering::SeqCst);
                    async { Ok::<(), redis::RedisError>(()) }
                })
                .await;
            assert!(skipped.is_none());
        }
        assert_eq!(
            asked.load(Ordering::SeqCst),
            1,
            "an open breaker sends nothing"
        );
        assert_eq!(
            before.elapsed(),
            REDIS_OP_TIMEOUT,
            "a thousand skipped frames cost no time at all"
        );

        // After the cooldown the next command is the probe.
        tokio::time::sleep(REDIS_COOLDOWN).await;
        let recovered = breaker
            .run("append", || {
                asked.fetch_add(1, Ordering::SeqCst);
                async { Ok::<u8, redis::RedisError>(7) }
            })
            .await;
        assert_eq!(recovered, Some(7));
        assert!(
            !breaker.is_open(),
            "a successful probe leaves the breaker closed"
        );
        assert_eq!(asked.load(Ordering::SeqCst), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn an_error_opens_the_breaker_and_a_failed_probe_opens_it_again() {
        let breaker = Breaker::new();
        let refused = || async {
            Err::<(), _>(redis::RedisError::from((
                redis::ErrorKind::IoError,
                "connection refused",
            )))
        };

        assert!(breaker.run("append", refused).await.is_none());
        assert!(breaker.is_open(), "a refused command opens the breaker");

        tokio::time::sleep(REDIS_COOLDOWN / 2).await;
        assert!(breaker.is_open(), "still cooling down halfway through");
        tokio::time::sleep(REDIS_COOLDOWN / 2 + Duration::from_millis(1)).await;
        assert!(!breaker.is_open(), "the cooldown ends by itself");

        assert!(breaker.run("append", refused).await.is_none());
        assert!(
            breaker.is_open(),
            "the probe failed, so it waits out another cooldown"
        );
    }

    // -- Measurements against a real store (ignored, need REDIS_URL) ----------

    fn unique_probe_id(tag: &str) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        scoped_stream_key("org_probe", "user_probe", &format!("{tag}_{nanos}"))
    }

    async fn time_appends(store: &StreamBufferStore, key: &str, frames: usize) -> Vec<Duration> {
        // A `chunk` frame as the gateway buffers it is a couple of hundred bytes.
        let data = format!("{{\"delta\":\"{}\"}}", "x".repeat(200));
        let mut took = Vec::with_capacity(frames);
        for seq in 0..frames {
            let started = Instant::now();
            store.append(key, seq as u64, "chunk", &data).await;
            took.push(started.elapsed());
        }
        took.sort();
        took
    }

    fn at_percentile(sorted: &[Duration], percent: usize) -> Duration {
        sorted[(sorted.len() * percent / 100).min(sorted.len() - 1)]
    }

    /// What the Redis buffer adds to every frame of every answer, next to the
    /// in-memory buffer it replaces. Run it against the stack's Dragonfly from
    /// inside the compose network to get the number that matters; through a
    /// published port the host's port forwarding dominates it. The bound is the
    /// point at which per-frame appends would need batching (50 ms batches with
    /// memory as the hot path) instead of staying inline.
    #[tokio::test]
    #[ignore = "requires REDIS_URL"]
    async fn redis_append_latency_probe() {
        let redis = StreamBufferStore::from_env().await;
        assert!(
            matches!(redis, StreamBufferStore::Redis(_)),
            "REDIS_URL must resolve"
        );
        let frames: usize = std::env::var("REDIS_PROBE_FRAMES")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(1_000);

        let in_memory = time_appends(&StreamBufferStore::new(), "probe", frames).await;
        let over_redis = time_appends(&redis, &unique_probe_id("latency"), frames).await;
        for (name, took) in [("memory", &in_memory), ("redis", &over_redis)] {
            println!(
                "append latency over {frames} frames, {name}: p50={:?} p95={:?} p99={:?} max={:?}",
                at_percentile(took, 50),
                at_percentile(took, 95),
                at_percentile(took, 99),
                took[took.len() - 1],
            );
        }
        assert!(
            at_percentile(&over_redis, 95) < Duration::from_millis(25),
            "per-frame appends are no longer cheap enough to leave inline"
        );
    }

    /// The stall proof, against a real store. Pause the Redis container, then
    /// create the file named by `STALL_PROBE_PAUSED_FLAG`: twenty appends must
    /// then finish in about one timeout, not hang. Unpause it and create
    /// `STALL_PROBE_RESUMED_FLAG`: after the cooldown the next frame is buffered
    /// and replays again.
    #[tokio::test]
    #[ignore = "requires REDIS_URL, STALL_PROBE_PAUSED_FLAG and STALL_PROBE_RESUMED_FLAG (paths)"]
    async fn redis_stalled_server_does_not_stall_appends() {
        let paused_flag =
            std::env::var("STALL_PROBE_PAUSED_FLAG").expect("STALL_PROBE_PAUSED_FLAG");
        let resumed_flag =
            std::env::var("STALL_PROBE_RESUMED_FLAG").expect("STALL_PROBE_RESUMED_FLAG");
        let store = StreamBufferStore::from_env().await;
        assert!(
            matches!(store, StreamBufferStore::Redis(_)),
            "REDIS_URL must resolve"
        );
        let key = unique_probe_id("stall");
        time_appends(&store, &key, 5).await;
        println!("STALL_PROBE_READY: pause the Redis container, then create {paused_flag}");

        let wait_for = |path: String| async move {
            let deadline = Instant::now() + Duration::from_secs(180);
            while !std::path::Path::new(&path).exists() {
                assert!(Instant::now() < deadline, "timed out waiting for {path}");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        };
        wait_for(paused_flag).await;

        let started = Instant::now();
        for seq in 5..25_u64 {
            store
                .append(&key, seq, "chunk", "{\"delta\":\"during the stall\"}")
                .await;
        }
        let stalled_for = started.elapsed();
        println!("twenty appends against a paused store took {stalled_for:?}");
        assert!(
            stalled_for < Duration::from_millis(1_500),
            "appends must not wait on a hung store: took {stalled_for:?}"
        );

        println!("STALL_PROBE_PAUSED_OK: unpause the Redis container, then create {resumed_flag}");
        wait_for(resumed_flag).await;
        tokio::time::sleep(REDIS_COOLDOWN + Duration::from_millis(500)).await;
        store
            .append(&key, 25, "chunk", "{\"delta\":\"after\"}")
            .await;
        let replay = store.replay_after(&key, Some(24)).await;
        assert!(
            replay.found,
            "the store is back and the buffer is using it again"
        );
        assert_eq!(
            replay
                .deltas
                .iter()
                .map(|frame| frame.seq)
                .collect::<Vec<_>>(),
            [25],
            "the frame sent after recovery replays"
        );
    }
}
