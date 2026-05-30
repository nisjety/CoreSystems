//! Delta buffer for resumable chat streams (HARNESS_PHASE1 §3b).
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

/// A single buffered delta plus its SSE sequence id.
#[derive(Clone, Serialize, Deserialize)]
pub struct BufferedDelta {
    pub seq: u64,
    pub delta: String,
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

/// What a resume needs: deltas after the cursor and the terminal chunk.
pub struct Replay {
    pub deltas: Vec<BufferedDelta>,
    pub done: Option<StreamDone>,
    /// False when the `request_id` is unknown (evicted or never existed).
    pub found: bool,
}

// ---------------------------------------------------------------------------
// In-memory backend
// ---------------------------------------------------------------------------

struct BufferedStream {
    deltas: Vec<BufferedDelta>,
    done: Option<StreamDone>,
    updated: Instant,
}

#[derive(Clone, Default)]
pub struct InMemoryStreamBuffer {
    inner: Arc<Mutex<HashMap<String, BufferedStream>>>,
}

impl InMemoryStreamBuffer {
    fn append(&self, request_id: &str, seq: u64, delta: &str) {
        let Ok(mut map) = self.inner.lock() else {
            return;
        };
        Self::sweep(&mut map);
        let entry = map
            .entry(request_id.to_owned())
            .or_insert_with(|| BufferedStream {
                deltas: Vec::new(),
                done: None,
                updated: Instant::now(),
            });
        entry.deltas.push(BufferedDelta {
            seq,
            delta: delta.to_owned(),
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
            .or_insert_with(|| BufferedStream {
                deltas: Vec::new(),
                done: None,
                updated: Instant::now(),
            });
        entry.done = Some(done);
        entry.updated = Instant::now();
    }

    fn replay_after(&self, request_id: &str, after_seq: Option<u64>) -> Replay {
        let Ok(map) = self.inner.lock() else {
            return Replay {
                deltas: Vec::new(),
                done: None,
                found: false,
            };
        };
        match map.get(request_id) {
            None => Replay {
                deltas: Vec::new(),
                done: None,
                found: false,
            },
            Some(entry) => Replay {
                deltas: entry
                    .deltas
                    .iter()
                    .filter(|d| after_seq.is_none_or(|a| d.seq > a))
                    .cloned()
                    .collect(),
                done: entry.done.clone(),
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

#[derive(Clone)]
pub struct RedisStreamBuffer {
    conn: redis::aio::ConnectionManager,
}

impl RedisStreamBuffer {
    fn list_key(request_id: &str) -> String {
        format!("{REDIS_PREFIX}{request_id}")
    }
    fn done_key(request_id: &str) -> String {
        format!("{REDIS_PREFIX}{request_id}:done")
    }

    async fn append(&self, request_id: &str, seq: u64, delta: &str) {
        let key = Self::list_key(request_id);
        let entry = match serde_json::to_string(&BufferedDelta {
            seq,
            delta: delta.to_owned(),
        }) {
            Ok(s) => s,
            Err(_) => return,
        };
        let mut conn = self.conn.clone();
        // RPUSH + LTRIM to cap + EXPIRE in a pipeline; ignore transient errors.
        let result: redis::RedisResult<()> = redis::pipe()
            .rpush(&key, entry)
            .ignore()
            .ltrim(&key, -(MAX_DELTAS_PER_STREAM as isize), -1)
            .ignore()
            .expire(&key, STREAM_TTL_SECS)
            .ignore()
            .query_async(&mut conn)
            .await;
        if let Err(e) = result {
            warn!(error = %e, request_id, "redis stream append failed");
        }
    }

    async fn finish(&self, request_id: &str, done: StreamDone) {
        let key = Self::done_key(request_id);
        let value = match serde_json::to_string(&done) {
            Ok(s) => s,
            Err(_) => return,
        };
        let mut conn = self.conn.clone();
        let result: redis::RedisResult<()> = conn.set_ex(&key, value, STREAM_TTL_SECS as u64).await;
        if let Err(e) = result {
            warn!(error = %e, request_id, "redis stream finish failed");
        }
    }

    async fn replay_after(&self, request_id: &str, after_seq: Option<u64>) -> Replay {
        let mut conn = self.conn.clone();
        let list_key = Self::list_key(request_id);
        let done_key = Self::done_key(request_id);

        let raw: redis::RedisResult<Vec<String>> = conn.lrange(&list_key, 0, -1).await;
        let raw_done: redis::RedisResult<Option<String>> = conn.get(&done_key).await;

        let entries = match raw {
            Ok(v) => v,
            Err(e) => {
                warn!(error = %e, request_id, "redis stream replay failed");
                return Replay {
                    deltas: Vec::new(),
                    done: None,
                    found: false,
                };
            }
        };
        let done = raw_done
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str::<StreamDone>(&s).ok());

        let found = !entries.is_empty() || done.is_some();
        let deltas = entries
            .iter()
            .filter_map(|s| serde_json::from_str::<BufferedDelta>(s).ok())
            .filter(|d| after_seq.is_none_or(|a| d.seq > a))
            .collect();

        Replay {
            deltas,
            done,
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
    Redis(RedisStreamBuffer),
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
            Ok(client) => match redis::aio::ConnectionManager::new(client).await {
                Ok(conn) => {
                    tracing::info!("stream buffer: Redis backend active");
                    Self::Redis(RedisStreamBuffer { conn })
                }
                Err(e) => {
                    warn!(error = %e, "REDIS_URL set but connect failed; using in-memory stream buffer");
                    Self::new()
                }
            },
            Err(e) => {
                warn!(error = %e, "invalid REDIS_URL; using in-memory stream buffer");
                Self::new()
            }
        }
    }

    pub async fn append(&self, request_id: &str, seq: u64, delta: &str) {
        match self {
            Self::Memory(m) => m.append(request_id, seq, delta),
            Self::Redis(r) => r.append(request_id, seq, delta).await,
        }
    }

    pub async fn finish(&self, request_id: &str, done: StreamDone) {
        match self {
            Self::Memory(m) => m.finish(request_id, done),
            Self::Redis(r) => r.finish(request_id, done).await,
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
        store.append("req_1", 0, "a").await;
        store.append("req_1", 1, "b").await;
        store.append("req_1", 2, "c").await;

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
        store.append("req_2", 0, "hi").await;
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
    async fn unknown_request_is_not_found() {
        let store = StreamBufferStore::new();
        let r = store.replay_after("nope", None).await;
        assert!(!r.found);
    }

    #[tokio::test]
    async fn delta_cap_evicts_oldest() {
        let store = StreamBufferStore::new();
        for i in 0..(MAX_DELTAS_PER_STREAM as u64 + 10) {
            store.append("req_cap", i, "x").await;
        }
        let r = store.replay_after("req_cap", None).await;
        assert_eq!(r.deltas.len(), MAX_DELTAS_PER_STREAM);
        assert_eq!(r.deltas[0].seq, 10);
    }
}
