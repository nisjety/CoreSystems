//! Idempotency guard for `/v1/invoke` (chat-parity §1 — "idempotent regenerate").
//!
//! A client that supplies an `idempotency_key` (e.g. a regenerate retry, a
//! double-clicked send, or a network-level replay) must not trigger a second
//! inference run or a second budget charge. This registry claims a key for the
//! duration of a request and caches the completed [`CachedInvoke`] so a
//! duplicate returns the *same* `request_id` + content instead of re-running.
//!
//! Three outcomes when a request claims a key (see [`Claim`]):
//!   - `Proceed` — first claimant; run inference, then `commit` the result.
//!   - `Cached`  — a fresh completed result exists; return it verbatim.
//!   - `InFlight`— another request holds the key right now; reject as duplicate.
//!
//! Cheap (`DashMap` + `Instant`), no new deps. In-memory and per replica by default; with
//! `MODEL_GATEWAY_SHARED_IDEMPOTENCY` on, the streaming path also claims the key in Redis (see
//! the shared claim section below) so a duplicate that lands on another replica attaches to the
//! original run. The unary `/v1/invoke` path stays per replica.
//!
//! # SECURITY: the claim key is namespaced by verified identity
//!
//! `idempotency_key` is a *client-supplied* string and this registry is a
//! process-global map shared by every tenant. Keying it on that string alone
//! made a completed answer replayable by anyone who sent the same value: two
//! users who both send `"retry-1"` (or any guessable key) collide, and the
//! second receives the first user's `Claim::Cached` response — a cross-tenant
//! disclosure of generated content, reachable without guessing a request id.
//!
//! [`claim`](IdempotencyRegistry::claim) therefore takes the verified org and
//! user and derives the real key with [`scoped_idempotency_key`]. There is no
//! unscoped entry point, so a caller cannot forget to namespace — the same
//! "secure by construction" shape as `stream_buffer::scoped_stream_key`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use futures::future::BoxFuture;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};

use crate::stream_buffer::StreamBufferStore;

/// Separator between the identity components of a claim key. ASCII unit
/// separator, chosen to match `stream_buffer::KEY_SEPARATOR`.
///
/// The client-controlled component is deliberately **last**: the org and user
/// are server-derived from verified claims, so a caller who smuggles a
/// separator into their own `idempotency_key` only extends their own suffix
/// and can never synthesize a different tenant's prefix.
const KEY_SEPARATOR: char = '\u{1f}';

/// Namespace a client idempotency key to the verified caller.
///
/// Deriving the key from identity means a caller in another org (or another
/// user in the same org) computes a different key, misses, and proceeds with
/// their own generation instead of replaying someone else's answer.
#[must_use]
pub fn scoped_idempotency_key(org_id: &str, user_id: &str, client_key: &str) -> String {
    format!("{org_id}{KEY_SEPARATOR}{user_id}{KEY_SEPARATOR}{client_key}")
}

/// How long a completed result stays replayable for its key.
const DONE_TTL: Duration = Duration::from_secs(600);
/// How long a `Pending` claim is honored before it is treated as abandoned
/// (e.g. the original request's process died mid-flight). Keeps a crashed
/// request from wedging its key forever.
const PENDING_TTL: Duration = Duration::from_secs(120);
/// Public client idempotency values are retained in-memory while in flight and
/// as cache keys. Keep the accepted input bounded before it can allocate.
const MAX_KEY_BYTES: usize = 256;
/// A bounded number of fresh entries prevents unique public keys from growing
/// the registry without limit. Together with the per-entry payload cap this
/// holds completed response payloads to at most 64 MiB (plus small metadata)
/// before admission fails closed. Expired entries are reclaimed first.
const MAX_ENTRIES: usize = 1_024;
/// A completed response is caller-derived/provider-derived data. Refuse to
/// retain a large response for replay; the original request still succeeds.
const MAX_CACHED_VALUE_BYTES: usize = 64 * 1024;

/// The cacheable shape of a completed `/v1/invoke` response.
#[derive(Debug, Clone, PartialEq)]
pub struct CachedInvoke {
    pub request_id: String,
    pub content: String,
    pub model_used: String,
    pub usage: Option<CachedInvokeUsage>,
}

/// The exact non-content usage metadata returned with a completed invoke.
/// It is retained alongside the cached response so an idempotent replay does
/// not turn a real measured run into a fabricated zero-cost/zero-confidence
/// run.
#[derive(Debug, Clone, PartialEq)]
pub struct CachedInvokeUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cost_usd: Option<f64>,
    pub latency_ms: u64,
    pub confidence: Option<f64>,
}

#[derive(Clone)]
enum Entry {
    Pending { since: Instant },
    Done { value: CachedInvoke, at: Instant },
}

/// Outcome of claiming an idempotency key.
pub enum Claim {
    /// First claimant — proceed with the work, then `commit` on the guard.
    Proceed(CommitGuard),
    /// A fresh completed result already exists; return it without re-running.
    Cached(CachedInvoke),
    /// Another request currently holds this key (duplicate in-flight).
    InFlight,
    /// The caller's public key is malformed/too large or the bounded registry
    /// cannot safely admit another live entry. The request must not run
    /// unprotected as a fallback.
    Rejected(ClaimRejection),
}

/// Why an idempotency claim was deliberately rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaimRejection {
    InvalidKey,
    CapacityExceeded,
}

#[derive(Clone, Copy)]
struct RegistryLimits {
    max_key_bytes: usize,
    max_entries: usize,
    max_cached_value_bytes: usize,
}

struct RegistryState {
    inner: DashMap<String, Entry>,
    /// Serializes admission, completion, and release so capacity remains a
    /// strict bound rather than a best-effort race under concurrent requests.
    admission: Mutex<()>,
    limits: RegistryLimits,
}

/// Tracks in-flight + recently-completed invokes keyed by client idempotency key.
#[derive(Clone)]
pub struct IdempotencyRegistry {
    state: Arc<RegistryState>,
}

impl IdempotencyRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::with_limits(RegistryLimits {
            max_key_bytes: MAX_KEY_BYTES,
            max_entries: MAX_ENTRIES,
            max_cached_value_bytes: MAX_CACHED_VALUE_BYTES,
        })
    }

    /// Claim `client_key` for a new request, namespaced to the verified caller.
    /// See [`Claim`] for the three outcomes. Expired `Done` entries and
    /// abandoned `Pending` claims are reclaimed.
    ///
    /// `org_id`/`user_id` MUST come from verified claims, never from the
    /// request body — they are what keeps one tenant's cached answer
    /// unreachable to another. See the module-level security note.
    pub fn claim(&self, org_id: &str, user_id: &str, client_key: &str) -> Claim {
        // Bound the *client* portion: the public 256-byte contract must not
        // shrink because a long org/user prefix was prepended.
        if client_key.trim().is_empty() || client_key.len() > self.state.limits.max_key_bytes {
            return Claim::Rejected(ClaimRejection::InvalidKey);
        }
        let key = &scoped_idempotency_key(org_id, user_id, client_key);
        let now = Instant::now();
        let _admission = self
            .state
            .admission
            .lock()
            .expect("idempotency registry admission lock poisoned");
        self.purge_expired(now);
        if let Some(entry) = self.state.inner.get(key) {
            match entry.value() {
                Entry::Done { value, at } if now.duration_since(*at) < DONE_TTL => {
                    return Claim::Cached(value.clone());
                }
                Entry::Pending { since } if now.duration_since(*since) < PENDING_TTL => {
                    return Claim::InFlight;
                }
                // Stale (expired Done or abandoned Pending) — fall through and reclaim.
                _ => {}
            }
        }
        if self.state.inner.len() >= self.state.limits.max_entries {
            return Claim::Rejected(ClaimRejection::CapacityExceeded);
        }
        self.state
            .inner
            .insert(key.to_owned(), Entry::Pending { since: now });
        Claim::Proceed(CommitGuard {
            state: self.state.clone(),
            key: key.to_owned(),
            committed: false,
            shared: None,
        })
    }

    /// Number of currently-tracked keys (Pending + cached Done).
    #[must_use]
    pub fn tracked(&self) -> usize {
        self.state.inner.len()
    }

    fn purge_expired(&self, now: Instant) {
        self.state.inner.retain(|_, entry| match entry {
            Entry::Done { at, .. } => now.duration_since(*at) < DONE_TTL,
            Entry::Pending { since } => now.duration_since(*since) < PENDING_TTL,
        });
    }

    fn with_limits(limits: RegistryLimits) -> Self {
        Self {
            state: Arc::new(RegistryState {
                inner: DashMap::new(),
                admission: Mutex::new(()),
                limits,
            }),
        }
    }
}

impl Default for IdempotencyRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// RAII guard for a `Pending` claim. On `commit` the result is cached; if the
/// guard is dropped without committing (any early `?` return / error), the
/// claim is released so an immediate retry can proceed instead of getting
/// `InFlight` until the pending TTL elapses. Holds an `Arc` clone of the map
/// (not a borrow), so it is `'static` and can be held across `.await`.
pub struct CommitGuard {
    state: Arc<RegistryState>,
    key: String,
    committed: bool,
    /// Set only when this request won the shared claim; settled when the guard drops.
    shared: Option<SharedRelease>,
}

impl CommitGuard {
    /// Cache the completed result under this key and release the claim.
    pub fn commit(mut self, value: CachedInvoke) {
        let _admission = self
            .state
            .admission
            .lock()
            .expect("idempotency registry admission lock poisoned");
        if cached_value_bytes(&value) <= self.state.limits.max_cached_value_bytes {
            self.state.inner.insert(
                self.key.clone(),
                Entry::Done {
                    value,
                    at: Instant::now(),
                },
            );
        } else {
            // Preserve the normal response but do not retain an oversized
            // provider completion in the process heap for a later replay.
            self.state
                .inner
                .remove_if(&self.key, |_, entry| matches!(entry, Entry::Pending { .. }));
        }
        self.committed = true;
    }
}

impl Drop for CommitGuard {
    fn drop(&mut self) {
        if let Some(release) = self.shared.take() {
            // Stop the renewal before the settle is spawned, so that no renewal re-extends the
            // claim the settle is about to write as `Done` or delete.
            release.stop_renewal();
            // The stream has ended: the settle reads the buffer and writes Redis, so it cannot
            // run in `drop` itself. Outside a runtime (a unit test) the claim simply expires.
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                runtime.spawn(release.settle());
            }
        }
        if !self.committed {
            // Only drop our own still-Pending claim; never clobber a sibling's Done.
            let _admission = self
                .state
                .admission
                .lock()
                .expect("idempotency registry admission lock poisoned");
            self.state
                .inner
                .remove_if(&self.key, |_, e| matches!(e, Entry::Pending { .. }));
        }
    }
}

// ---------------------------------------------------------------------------
// Shared (cross-replica) claim
// ---------------------------------------------------------------------------

/// The switch for the shared claim. Default off; `true` and nothing else turns it on.
pub const SHARED_IDEMPOTENCY_ENV: &str = "MODEL_GATEWAY_SHARED_IDEMPOTENCY";

/// The strict parser of [`SHARED_IDEMPOTENCY_ENV`].
#[must_use]
pub fn shared_idempotency_enabled_from(value: Option<&str>) -> bool {
    mp_contracts::loop_fold::switch_from(value)
}

/// Whether the shared claim is on, read from the environment at each call.
#[must_use]
pub fn shared_idempotency_enabled() -> bool {
    shared_idempotency_enabled_from(std::env::var(SHARED_IDEMPOTENCY_ENV).ok().as_deref())
}

const SHARED_PREFIX: &str = "mp:gw:idem:";

fn shared_key_of(scoped_key: &str) -> String {
    format!("{SHARED_PREFIX}{scoped_key}")
}

/// The Redis key of a caller's idempotency key. Built from the same identity-scoped key as the
/// local map, so another tenant derives a different key and never sees this one.
#[must_use]
pub fn shared_claim_key(org_id: &str, user_id: &str, client_key: &str) -> String {
    shared_key_of(&scoped_idempotency_key(org_id, user_id, client_key))
}

/// Where a shared claim is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SharedClaimState {
    Pending,
    Done,
}

/// What Redis holds for a claimed key: ids and a state, never an answer. The original stream's
/// frames stay in the stream buffer, which is identity-scoped and has its own ZDR rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedClaimValue {
    pub request_id: String,
    pub run_id: Option<String>,
    pub thread_id: Option<String>,
    pub state: SharedClaimState,
}

/// The result of trying to take a shared claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SharedClaimOutcome {
    /// This request owns the key.
    Won,
    /// Another request (usually on another replica) owns it.
    Held(SharedClaimValue),
    /// Redis could not answer; the caller falls back to the local registry alone.
    Unavailable,
}

/// The shared half of the claim, behind a seam so the two-replica behaviour runs against a fake.
pub trait SharedClaimStore: Send + Sync {
    /// `SET NX` of `value` for [`PENDING_TTL`].
    fn claim<'a>(
        &'a self,
        key: &'a str,
        value: &'a SharedClaimValue,
    ) -> BoxFuture<'a, SharedClaimOutcome>;
    /// The current value, or `None` for a missing key, an unreadable value and an outage alike.
    fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Option<SharedClaimValue>>;
    /// Writes `done` for [`DONE_TTL`], or deletes the key when `None`, only while the key still
    /// belongs to `request_id`: a claim that expired and was taken by another request is theirs.
    fn settle<'a>(
        &'a self,
        key: &'a str,
        request_id: &'a str,
        done: Option<&'a SharedClaimValue>,
    ) -> BoxFuture<'a, ()>;
    /// Extends a Pending claim to [`PENDING_TTL`] while it is still `request_id`'s, so a stream
    /// that outlives the TTL keeps its claim. `false` when the key is gone, belongs to another
    /// request or is already `Done`, and the renewal then stops. `true` when it was extended, and
    /// also when the store cannot answer: an outage must not end the renewal for good, so the next
    /// tick tries again.
    fn renew<'a>(&'a self, key: &'a str, request_id: &'a str) -> BoxFuture<'a, bool>;
}

/// What a request that holds the local claim learns from the shared one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SharedAttempt {
    /// This request owns the shared key; it settles when the guard drops.
    Owner,
    /// Another request owns the key: attach to its run instead of starting a second one.
    Attach(SharedClaimValue),
    /// The shared store is unavailable: behave exactly as the local registry says.
    Local,
}

/// Everything the guard needs, when it drops, to turn its claim into `Done` or remove it.
struct SharedRelease {
    store: Arc<dyn SharedClaimStore>,
    key: String,
    request_id: String,
    run_id: Option<String>,
    thread_id: Option<String>,
    stream_buffers: StreamBufferStore,
    buffer_key: String,
    /// The renewal tick that keeps the Pending claim alive while the stream runs. `None` when
    /// there was no runtime to run it on.
    renewal: Option<tokio::task::AbortHandle>,
}

impl SharedRelease {
    /// Stops the renewal. The guard calls this before it spawns the settle, so no renewal can
    /// re-extend the claim the settle writes as `Done` or deletes. Dropping an unsettled release
    /// does it too: a tick left running would renew a dead request's claim for as long as the
    /// runtime lives.
    fn stop_renewal(&self) {
        if let Some(renewal) = &self.renewal {
            renewal.abort();
        }
    }

    /// A stream that buffered its `done` frame is a completed answer a duplicate can still
    /// attach to for [`DONE_TTL`]. Any other end (an error, a stop, a refusal before the run
    /// started) removes the claim, as the local guard releases its own, so a retry is free to run.
    async fn settle(self) {
        let completed = self
            .stream_buffers
            .replay_after(&self.buffer_key, None)
            .await
            .done
            .is_some();
        let done = completed.then(|| SharedClaimValue {
            request_id: self.request_id.clone(),
            run_id: self.run_id.clone(),
            thread_id: self.thread_id.clone(),
            state: SharedClaimState::Done,
        });
        self.store
            .settle(&self.key, &self.request_id, done.as_ref())
            .await;
    }
}

impl Drop for SharedRelease {
    fn drop(&mut self) {
        self.stop_renewal();
    }
}

impl CommitGuard {
    /// Take the shared claim for this request, after the local one. `buffer_key` is the
    /// request's identity-scoped stream-buffer key, read when the guard drops to tell a
    /// completed answer from an abandoned one.
    pub async fn claim_shared(
        &mut self,
        store: &Arc<dyn SharedClaimStore>,
        request_id: &str,
        stream_buffers: &StreamBufferStore,
        buffer_key: &str,
    ) -> SharedAttempt {
        let key = shared_key_of(&self.key);
        let pending = SharedClaimValue {
            request_id: request_id.to_owned(),
            run_id: None,
            thread_id: None,
            state: SharedClaimState::Pending,
        };
        match store.claim(&key, &pending).await {
            SharedClaimOutcome::Won => {
                // A Pending claim lapses after PENDING_TTL unless it is renewed, and a stream that
                // outlives it would let a duplicate on another replica start a second run. With no
                // runtime to tick on (a unit test) the claim lapses as it always did.
                let renewal = tokio::runtime::Handle::try_current().ok().map(|runtime| {
                    runtime
                        .spawn(keep_claim_alive(
                            store.clone(),
                            key.clone(),
                            request_id.to_owned(),
                        ))
                        .abort_handle()
                });
                self.shared = Some(SharedRelease {
                    store: store.clone(),
                    key,
                    request_id: request_id.to_owned(),
                    run_id: None,
                    thread_id: None,
                    stream_buffers: stream_buffers.clone(),
                    buffer_key: buffer_key.to_owned(),
                    renewal,
                });
                SharedAttempt::Owner
            }
            SharedClaimOutcome::Held(original) => SharedAttempt::Attach(original),
            SharedClaimOutcome::Unavailable => SharedAttempt::Local,
        }
    }

    /// Record the run this request started, so the `Done` value names it. A no-op without a
    /// shared claim.
    pub fn record_run(&mut self, run_id: &str, thread_id: &str) {
        if let Some(release) = self.shared.as_mut() {
            release.run_id = Some(run_id.to_owned());
            release.thread_id = Some(thread_id.to_owned());
        }
    }
}

/// Renews a Pending claim every third of [`PENDING_TTL`] while its stream runs, so a live stream
/// never lets its claim lapse. Stops when `renew` reports the claim is no longer this request's;
/// otherwise the guard aborts the task when the stream ends.
async fn keep_claim_alive(store: Arc<dyn SharedClaimStore>, key: String, request_id: String) {
    loop {
        tokio::time::sleep(PENDING_TTL / 3).await;
        if !store.renew(&key, &request_id).await {
            // Logged by operation only: the key holds the org and user ids.
            tracing::warn!(
                op = "renew",
                "shared idempotency claim was lost while its stream ran; a duplicate may start"
            );
            return;
        }
    }
}

/// The shared claim of a key this replica already holds locally as in flight: the duplicate
/// landed on the original's own replica, and the local map does not know the original's request
/// id. `None` when nothing is held, or the store cannot answer.
pub async fn lookup_shared(
    store: &Arc<dyn SharedClaimStore>,
    org_id: &str,
    user_id: &str,
    client_key: &str,
) -> Option<SharedClaimValue> {
    store
        .get(&shared_claim_key(org_id, user_id, client_key))
        .await
}

/// Longest one Redis command may take before it is given up on; the same bound as the stream
/// buffer's, so an outage costs a send no more than a frame write.
const SHARED_OP_TIMEOUT: Duration = Duration::from_millis(250);
/// After a failed or timed-out command Redis is left alone for this long.
const SHARED_COOLDOWN: Duration = Duration::from_secs(5);
const SHARED_CONNECT_TIMEOUT: Duration = Duration::from_secs(1);

/// The Redis implementation: one `ConnectionManager` and a cooldown breaker of its own.
struct RedisClaimStore {
    conn: redis::aio::ConnectionManager,
    origin: tokio::time::Instant,
    open_until_ms: Arc<AtomicU64>,
}

impl RedisClaimStore {
    fn now_ms(&self) -> u64 {
        u64::try_from(self.origin.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// One command under the timeout and the breaker; `None` for a failure, a timeout and an
    /// open breaker alike. Logs carry the operation, never the key (it holds org and user ids).
    async fn run<T>(
        &self,
        what: &'static str,
        op: impl std::future::Future<Output = redis::RedisResult<T>>,
    ) -> Option<T> {
        if self.now_ms() < self.open_until_ms.load(Ordering::Relaxed) {
            return None;
        }
        let failure = match tokio::time::timeout(SHARED_OP_TIMEOUT, op).await {
            Ok(Ok(value)) => return Some(value),
            Ok(Err(error)) => error.to_string(),
            Err(_) => "timed out".to_owned(),
        };
        tracing::warn!(
            error = %failure,
            op = what,
            cooldown_secs = SHARED_COOLDOWN.as_secs(),
            "redis idempotency op failed; not using redis for the cooldown"
        );
        let cooldown = u64::try_from(SHARED_COOLDOWN.as_millis()).unwrap_or(u64::MAX);
        self.open_until_ms
            .store(self.now_ms().saturating_add(cooldown), Ordering::Relaxed);
        None
    }

    async fn read(&self, key: &str) -> Option<SharedClaimValue> {
        let mut conn = self.conn.clone();
        let raw = self
            .run("get", conn.get::<_, Option<String>>(key))
            .await
            .flatten()?;
        serde_json::from_str(&raw).ok()
    }
}

impl SharedClaimStore for RedisClaimStore {
    fn claim<'a>(
        &'a self,
        key: &'a str,
        value: &'a SharedClaimValue,
    ) -> BoxFuture<'a, SharedClaimOutcome> {
        Box::pin(async move {
            let Ok(raw) = serde_json::to_string(value) else {
                return SharedClaimOutcome::Unavailable;
            };
            let mut conn = self.conn.clone();
            let ttl_ms = u64::try_from(PENDING_TTL.as_millis()).unwrap_or(u64::MAX);
            let mut set = redis::cmd("SET");
            set.arg(key).arg(raw).arg("NX").arg("PX").arg(ttl_ms);
            match self
                .run("claim", set.query_async::<Option<String>>(&mut conn))
                .await
            {
                None => SharedClaimOutcome::Unavailable,
                Some(Some(_)) => SharedClaimOutcome::Won,
                // Held. A holder that expired between the two commands, or an unreadable value,
                // leaves the send to the local registry rather than guessing.
                Some(None) => self
                    .read(key)
                    .await
                    .map_or(SharedClaimOutcome::Unavailable, SharedClaimOutcome::Held),
            }
        })
    }

    fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Option<SharedClaimValue>> {
        Box::pin(self.read(key))
    }

    fn settle<'a>(
        &'a self,
        key: &'a str,
        request_id: &'a str,
        done: Option<&'a SharedClaimValue>,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            // A read then a write, not one atomic step: the window is the 250 ms command bound,
            // and the worst outcome is overwriting a claim that expired in it.
            if self
                .read(key)
                .await
                .is_none_or(|held| held.request_id != request_id)
            {
                return;
            }
            let mut conn = self.conn.clone();
            match done.and_then(|value| serde_json::to_string(value).ok()) {
                Some(raw) => {
                    let ttl_ms = u64::try_from(DONE_TTL.as_millis()).unwrap_or(u64::MAX);
                    let mut set = redis::cmd("SET");
                    set.arg(key).arg(raw).arg("PX").arg(ttl_ms);
                    self.run("settle_done", set.query_async::<()>(&mut conn))
                        .await;
                }
                None => {
                    self.run("settle_delete", conn.del::<_, ()>(key)).await;
                }
            }
        })
    }

    fn renew<'a>(&'a self, key: &'a str, request_id: &'a str) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            let mut conn = self.conn.clone();
            // `run` has logged the failure and opened the breaker. The claim may still be ours
            // once Redis answers, so the renewal carries on rather than ending for good.
            let Some(raw) = self
                .run("renew_get", conn.get::<_, Option<String>>(key))
                .await
            else {
                return true;
            };
            let held = raw.and_then(|raw| serde_json::from_str::<SharedClaimValue>(&raw).ok());
            if held.is_none_or(|held| {
                held.request_id != request_id || held.state != SharedClaimState::Pending
            }) {
                return false;
            }
            // The read-then-write of `settle`, with the same 250 ms window. Inside it a claim can
            // lapse and pass to another request, which this PEXPIRE then extends; and a renewal
            // can land after the settle's write, leaving a `Done` value on the pending TTL and so
            // shortening its replay window. A key that is gone stays gone: PEXPIRE is a no-op.
            let ttl_ms = u64::try_from(PENDING_TTL.as_millis()).unwrap_or(u64::MAX);
            let mut expire = redis::cmd("PEXPIRE");
            expire.arg(key).arg(ttl_ms);
            // 0 when the key vanished between the two commands: the claim is gone, so stop.
            match self
                .run("renew_expire", expire.query_async::<i64>(&mut conn))
                .await
            {
                Some(extended) => extended == 1,
                None => true,
            }
        })
    }
}

static SHARED_STORE: tokio::sync::OnceCell<Option<Arc<dyn SharedClaimStore>>> =
    tokio::sync::OnceCell::const_new();

async fn connect_shared_store() -> Option<Arc<dyn SharedClaimStore>> {
    let Ok(url) = std::env::var("REDIS_URL") else {
        tracing::warn!("shared idempotency is on but REDIS_URL is unset; claims stay per replica");
        return None;
    };
    let client = match redis::Client::open(url) {
        Ok(client) => client,
        Err(error) => {
            tracing::warn!(%error, "invalid REDIS_URL; shared idempotency claims stay per replica");
            return None;
        }
    };
    let config = redis::aio::ConnectionManagerConfig::new()
        .set_connection_timeout(SHARED_CONNECT_TIMEOUT)
        .set_response_timeout(SHARED_OP_TIMEOUT)
        .set_number_of_retries(1);
    match redis::aio::ConnectionManager::new_with_config(client, config).await {
        Ok(conn) => {
            tracing::info!("shared idempotency: Redis claim store active");
            Some(Arc::new(RedisClaimStore {
                conn,
                origin: tokio::time::Instant::now(),
                open_until_ms: Arc::new(AtomicU64::new(0)),
            }))
        }
        Err(error) => {
            tracing::warn!(%error, "shared idempotency: Redis connect failed; claims stay per replica");
            None
        }
    }
}

/// The shared claim store, or `None` when the switch is off, `REDIS_URL` is unset or the first
/// connect failed (the outcome is kept until restart, like the stream buffer's backend choice).
/// With the switch off nothing connects and nothing is read.
pub async fn shared_store() -> Option<Arc<dyn SharedClaimStore>> {
    if !shared_idempotency_enabled() {
        return None;
    }
    SHARED_STORE.get_or_init(connect_shared_store).await.clone()
}

fn cached_value_bytes(value: &CachedInvoke) -> usize {
    value
        .request_id
        .len()
        .saturating_add(value.content.len())
        .saturating_add(value.model_used.len())
        .saturating_add(std::mem::size_of::<CachedInvokeUsage>())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORG: &str = "org_A";
    const USER: &str = "user_1";

    fn sample(id: &str) -> CachedInvoke {
        CachedInvoke {
            request_id: id.to_owned(),
            content: "hello".to_owned(),
            model_used: "test-model".to_owned(),
            usage: None,
        }
    }

    #[test]
    fn first_claim_proceeds_then_caches_on_commit() {
        let reg = IdempotencyRegistry::new();
        match reg.claim(ORG, USER, "k1") {
            Claim::Proceed(guard) => guard.commit(sample("req-1")),
            _ => panic!("first claim must Proceed"),
        }
        // Second claim with the same key returns the cached result verbatim.
        match reg.claim(ORG, USER, "k1") {
            Claim::Cached(v) => {
                assert_eq!(v.request_id, "req-1");
                assert_eq!(v.content, "hello");
            }
            _ => panic!("duplicate must return Cached"),
        }
    }

    #[test]
    fn concurrent_duplicate_while_pending_is_in_flight() {
        let reg = IdempotencyRegistry::new();
        // held = still pending
        let Claim::Proceed(_guard) = reg.claim(ORG, USER, "k2") else {
            panic!("first claim must Proceed");
        };
        assert!(
            matches!(reg.claim(ORG, USER, "k2"), Claim::InFlight),
            "a second claim while the first is in-flight is rejected"
        );
    }

    #[test]
    fn dropped_guard_without_commit_releases_the_claim() {
        let reg = IdempotencyRegistry::new();
        {
            let Claim::Proceed(_guard) = reg.claim(ORG, USER, "k3") else {
                panic!("first claim must Proceed");
            };
            // guard dropped here without commit (simulates an errored request)
        }
        assert_eq!(reg.tracked(), 0, "abandoned claim is released on drop");
        // A retry after the error proceeds rather than seeing InFlight.
        assert!(matches!(reg.claim(ORG, USER, "k3"), Claim::Proceed(_)));
    }

    #[test]
    fn distinct_keys_do_not_interfere() {
        let reg = IdempotencyRegistry::new();
        match reg.claim(ORG, USER, "a") {
            Claim::Proceed(g) => g.commit(sample("req-a")),
            _ => panic!(),
        }
        assert!(
            matches!(reg.claim(ORG, USER, "b"), Claim::Proceed(_)),
            "a different key is unaffected by another's cached result"
        );
    }

    #[test]
    fn oversized_public_key_is_rejected_before_it_is_retained() {
        let reg = IdempotencyRegistry::with_limits(RegistryLimits {
            max_key_bytes: 4,
            max_entries: 2,
            max_cached_value_bytes: 64,
        });
        assert!(matches!(
            reg.claim(ORG, USER, "abcde"),
            Claim::Rejected(ClaimRejection::InvalidKey)
        ));
        assert_eq!(reg.tracked(), 0);
    }

    #[test]
    fn capacity_is_bounded_and_fails_closed_without_evicting_live_claims() {
        let reg = IdempotencyRegistry::with_limits(RegistryLimits {
            max_key_bytes: 64,
            max_entries: 2,
            max_cached_value_bytes: 64,
        });
        let Claim::Proceed(first) = reg.claim(ORG, USER, "first") else {
            panic!("first claim must proceed");
        };
        let Claim::Proceed(_second) = reg.claim(ORG, USER, "second") else {
            panic!("second claim must proceed");
        };
        assert!(
            matches!(
                reg.claim(ORG, USER, "third"),
                Claim::Rejected(ClaimRejection::CapacityExceeded)
            ),
            "a full registry must not silently run an unprotected duplicate"
        );
        drop(first);
        assert!(matches!(reg.claim(ORG, USER, "third"), Claim::Proceed(_)));
    }

    #[test]
    fn oversized_completion_is_not_retained_for_replay() {
        let reg = IdempotencyRegistry::with_limits(RegistryLimits {
            max_key_bytes: 64,
            max_entries: 2,
            max_cached_value_bytes: 8,
        });
        let Claim::Proceed(guard) = reg.claim(ORG, USER, "key") else {
            panic!("first claim must proceed");
        };
        guard.commit(sample("request-too-large"));
        assert_eq!(reg.tracked(), 0);
        assert!(matches!(reg.claim(ORG, USER, "key"), Claim::Proceed(_)));
    }
    #[test]
    fn another_orgs_identical_key_cannot_replay_a_cached_answer() {
        let reg = IdempotencyRegistry::new();
        // Tenant A completes a generation under a guessable key.
        match reg.claim("org_A", "user_1", "retry-1") {
            Claim::Proceed(g) => g.commit(sample("req-secret")),
            _ => panic!("first claim must Proceed"),
        }
        // Tenant B sends the SAME client key. It must run its own generation,
        // never receive A's cached content.
        assert!(
            matches!(reg.claim("org_B", "user_1", "retry-1"), Claim::Proceed(_)),
            "a different org must not replay another tenant's cached answer"
        );
        // Same org, different user: also isolated.
        assert!(
            matches!(reg.claim("org_A", "user_2", "retry-1"), Claim::Proceed(_)),
            "a different user must not replay another user's cached answer"
        );
        // The original owner still gets their own replay.
        assert!(matches!(
            reg.claim("org_A", "user_1", "retry-1"),
            Claim::Cached(_)
        ));
    }

    #[test]
    fn another_orgs_identical_key_is_not_reported_in_flight() {
        let reg = IdempotencyRegistry::new();
        let Claim::Proceed(_held) = reg.claim("org_A", "user_1", "k") else {
            panic!("first claim must Proceed");
        };
        // Without scoping this leaked existence: B learned A had a live request.
        assert!(
            matches!(reg.claim("org_B", "user_1", "k"), Claim::Proceed(_)),
            "one tenant's in-flight key must not block or be observable by another"
        );
    }

    #[test]
    fn a_separator_in_the_client_key_cannot_forge_another_tenants_namespace() {
        let reg = IdempotencyRegistry::new();
        match reg.claim("org_A", "user_1", "k") {
            Claim::Proceed(g) => g.commit(sample("req-secret")),
            _ => panic!("first claim must Proceed"),
        }
        // Attacker in org_B smuggles separators trying to rebuild A's prefix.
        let forged = format!("org_A{KEY_SEPARATOR}user_1{KEY_SEPARATOR}k");
        assert!(
            matches!(reg.claim("org_B", "user_2", &forged), Claim::Proceed(_)),
            "the server-derived prefix leads, so a crafted key cannot impersonate"
        );
    }

    #[test]
    fn the_public_key_limit_is_measured_on_the_client_portion_only() {
        let reg = IdempotencyRegistry::with_limits(RegistryLimits {
            max_key_bytes: 8,
            max_entries: 4,
            max_cached_value_bytes: 64,
        });
        // A long org/user prefix must not consume the caller's key budget.
        assert!(
            matches!(
                reg.claim("a-very-long-org-identifier", "a-very-long-user-id", "12345678"),
                Claim::Proceed(_)
            ),
            "the prefix is server-side overhead, not part of the public limit"
        );
    }

    // -- The shared claim ----------------------------------------------------------------

    /// A Redis stand-in two registries (two replicas) can share. `down` makes it answer
    /// nothing, like an open breaker. Keys expire on the tokio clock, as Redis's `PX` would, so
    /// a paused test moves their TTLs.
    #[derive(Default)]
    struct FakeShared {
        /// Each claim with the instant its TTL runs out.
        map: Mutex<std::collections::HashMap<String, (SharedClaimValue, tokio::time::Instant)>>,
        down: std::sync::atomic::AtomicBool,
        /// Every `renew` call, answered or not.
        renewals: std::sync::atomic::AtomicUsize,
    }

    impl FakeShared {
        fn is_down(&self) -> bool {
            self.down.load(Ordering::SeqCst)
        }
        /// The value of a key whose TTL has not run out.
        fn held(&self, key: &str) -> Option<SharedClaimValue> {
            let now = tokio::time::Instant::now();
            self.map
                .lock()
                .unwrap()
                .get(key)
                .filter(|(_, expires)| *expires > now)
                .map(|(value, _)| value.clone())
        }
        /// The TTL a live key has left.
        fn ttl(&self, key: &str) -> Option<Duration> {
            let now = tokio::time::Instant::now();
            self.map
                .lock()
                .unwrap()
                .get(key)
                .filter(|(_, expires)| *expires > now)
                .map(|(_, expires)| expires.duration_since(now))
        }
        fn renewals(&self) -> usize {
            self.renewals.load(Ordering::SeqCst)
        }
    }

    impl SharedClaimStore for FakeShared {
        fn claim<'a>(
            &'a self,
            key: &'a str,
            value: &'a SharedClaimValue,
        ) -> BoxFuture<'a, SharedClaimOutcome> {
            Box::pin(async move {
                if self.is_down() {
                    return SharedClaimOutcome::Unavailable;
                }
                let now = tokio::time::Instant::now();
                let mut map = self.map.lock().unwrap();
                map.retain(|_, (_, expires)| *expires > now);
                match map.get(key) {
                    Some((held, _)) => SharedClaimOutcome::Held(held.clone()),
                    None => {
                        map.insert(key.to_owned(), (value.clone(), now + PENDING_TTL));
                        SharedClaimOutcome::Won
                    }
                }
            })
        }
        fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Option<SharedClaimValue>> {
            Box::pin(async move { (!self.is_down()).then(|| self.held(key)).flatten() })
        }
        fn settle<'a>(
            &'a self,
            key: &'a str,
            request_id: &'a str,
            done: Option<&'a SharedClaimValue>,
        ) -> BoxFuture<'a, ()> {
            Box::pin(async move {
                if self.is_down() {
                    return;
                }
                let now = tokio::time::Instant::now();
                let mut map = self.map.lock().unwrap();
                map.retain(|_, (_, expires)| *expires > now);
                if map
                    .get(key)
                    .is_none_or(|(held, _)| held.request_id != request_id)
                {
                    return;
                }
                match done {
                    Some(value) => {
                        map.insert(key.to_owned(), (value.clone(), now + DONE_TTL));
                    }
                    None => {
                        map.remove(key);
                    }
                }
            })
        }
        fn renew<'a>(&'a self, key: &'a str, request_id: &'a str) -> BoxFuture<'a, bool> {
            Box::pin(async move {
                self.renewals.fetch_add(1, Ordering::SeqCst);
                // As in the Redis store, an outage answers true and the renewal goes on.
                if self.is_down() {
                    return true;
                }
                let now = tokio::time::Instant::now();
                let mut map = self.map.lock().unwrap();
                map.retain(|_, (_, expires)| *expires > now);
                let Some((held, expires)) = map.get_mut(key) else {
                    return false;
                };
                if held.request_id != request_id || held.state != SharedClaimState::Pending {
                    return false;
                }
                *expires = now + PENDING_TTL;
                true
            })
        }
    }

    fn fake() -> (Arc<FakeShared>, Arc<dyn SharedClaimStore>) {
        let fake = Arc::new(FakeShared::default());
        let store: Arc<dyn SharedClaimStore> = fake.clone();
        (fake, store)
    }

    /// A claim taken the way `invoke_sse` takes it: local first, then shared.
    async fn send(
        reg: &IdempotencyRegistry,
        store: &Arc<dyn SharedClaimStore>,
        org: &str,
        request_id: &str,
        buffers: &StreamBufferStore,
    ) -> (CommitGuard, SharedAttempt) {
        let Claim::Proceed(mut guard) = reg.claim(org, USER, "send-1") else {
            panic!("a fresh replica's local claim must proceed");
        };
        let buffer_key = crate::stream_buffer::scoped_stream_key(org, USER, request_id);
        let attempt = guard
            .claim_shared(store, request_id, buffers, &buffer_key)
            .await;
        (guard, attempt)
    }

    async fn until(mut ready: impl FnMut() -> bool) {
        for _ in 0..200 {
            if ready() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("the guard's settle never landed");
    }

    #[test]
    fn the_shared_switch_is_on_for_true_and_nothing_else() {
        assert!(shared_idempotency_enabled_from(Some("true")));
        assert!(shared_idempotency_enabled_from(Some(" TRUE ")));
        for off in [None, Some(""), Some("1"), Some("yes"), Some("on"), Some("truee")] {
            assert!(!shared_idempotency_enabled_from(off), "{off:?}");
        }
    }

    #[test]
    fn the_shared_key_is_identity_scoped() {
        assert_eq!(
            shared_claim_key("org_A", "user_1", "k"),
            format!("mp:gw:idem:org_A{KEY_SEPARATOR}user_1{KEY_SEPARATOR}k")
        );
        assert_ne!(
            shared_claim_key("org_A", "user_1", "k"),
            shared_claim_key("org_B", "user_1", "k")
        );
        assert_ne!(
            shared_claim_key("org_A", "user_1", "k"),
            shared_claim_key("org_A", "user_2", "k")
        );
    }

    #[test]
    fn a_shared_value_holds_ids_and_a_state_only() {
        let value = SharedClaimValue {
            request_id: "req-1".into(),
            run_id: Some("run-1".into()),
            thread_id: Some("thr-1".into()),
            state: SharedClaimState::Done,
        };
        let json: serde_json::Value = serde_json::to_value(&value).unwrap();
        let mut keys: Vec<_> = json.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["request_id", "run_id", "state", "thread_id"]);
        assert_eq!(json["state"], "done");
    }

    #[tokio::test]
    async fn a_duplicate_on_another_replica_attaches_to_the_original() {
        let (_, store) = fake();
        let buffers = StreamBufferStore::new();
        let (replica_a, replica_b) = (IdempotencyRegistry::new(), IdempotencyRegistry::new());
        let (_owner, first) = send(&replica_a, &store, ORG, "req-A", &buffers).await;
        assert_eq!(first, SharedAttempt::Owner);
        let (_dup, second) = send(&replica_b, &store, ORG, "req-B", &buffers).await;
        match second {
            SharedAttempt::Attach(original) => {
                assert_eq!(original.request_id, "req-A");
                assert_eq!(original.state, SharedClaimState::Pending);
            }
            other => panic!("the duplicate must attach, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn another_orgs_identical_key_does_not_attach() {
        let (_, store) = fake();
        let buffers = StreamBufferStore::new();
        let (replica_a, replica_b) = (IdempotencyRegistry::new(), IdempotencyRegistry::new());
        let (_owner, first) = send(&replica_a, &store, "org_A", "req-A", &buffers).await;
        assert_eq!(first, SharedAttempt::Owner);
        let (_other, second) = send(&replica_b, &store, "org_B", "req-B", &buffers).await;
        assert_eq!(
            second,
            SharedAttempt::Owner,
            "a foreign tenant derives another key, so it neither attaches nor learns of the first"
        );
    }

    #[tokio::test]
    async fn an_unavailable_store_leaves_the_local_behaviour() {
        let (fake, store) = fake();
        fake.down.store(true, Ordering::SeqCst);
        let buffers = StreamBufferStore::new();
        let reg = IdempotencyRegistry::new();
        let (guard, attempt) = send(&reg, &store, ORG, "req-A", &buffers).await;
        assert_eq!(attempt, SharedAttempt::Local);
        assert!(guard.shared.is_none(), "nothing to settle without a shared claim");
        // The local registry still protects the key on this replica.
        assert!(matches!(reg.claim(ORG, USER, "send-1"), Claim::InFlight));
        drop(guard);
        assert!(matches!(reg.claim(ORG, USER, "send-1"), Claim::Proceed(_)));
        assert!(lookup_shared(&store, ORG, USER, "send-1").await.is_none());
    }

    #[tokio::test]
    async fn a_completed_stream_leaves_a_done_claim_naming_its_run() {
        let (fake, store) = fake();
        let buffers = StreamBufferStore::new();
        let reg = IdempotencyRegistry::new();
        let (mut guard, attempt) = send(&reg, &store, ORG, "req-A", &buffers).await;
        assert_eq!(attempt, SharedAttempt::Owner);
        guard.record_run("run-1", "thr-1");
        let buffer_key = crate::stream_buffer::scoped_stream_key(ORG, USER, "req-A");
        buffers
            .finish(
                &buffer_key,
                crate::stream_buffer::StreamDone {
                    seq: 3,
                    model_used: "m".into(),
                    input_tokens: 1,
                    output_tokens: 1,
                },
            )
            .await;
        drop(guard);
        let key = shared_claim_key(ORG, USER, "send-1");
        until(|| fake.held(&key).is_some_and(|v| v.state == SharedClaimState::Done)).await;
        let done = lookup_shared(&store, ORG, USER, "send-1").await.unwrap();
        assert_eq!(done.request_id, "req-A");
        assert_eq!(done.run_id.as_deref(), Some("run-1"));
        assert_eq!(done.thread_id.as_deref(), Some("thr-1"));
        // A later duplicate, on any replica, still finds the original.
        let (_dup, attempt) = send(&IdempotencyRegistry::new(), &store, ORG, "req-B", &buffers).await;
        assert!(matches!(attempt, SharedAttempt::Attach(v) if v.request_id == "req-A"));
    }

    #[tokio::test]
    async fn an_abandoned_stream_removes_its_claim_so_a_retry_can_run() {
        let (fake, store) = fake();
        let buffers = StreamBufferStore::new();
        let reg = IdempotencyRegistry::new();
        let (guard, attempt) = send(&reg, &store, ORG, "req-A", &buffers).await;
        assert_eq!(attempt, SharedAttempt::Owner);
        // No `done` frame was buffered: the request errored or was refused.
        drop(guard);
        let key = shared_claim_key(ORG, USER, "send-1");
        until(|| fake.held(&key).is_none()).await;
        let (_retry, attempt) = send(&reg, &store, ORG, "req-B", &buffers).await;
        assert_eq!(attempt, SharedAttempt::Owner);
    }

    #[tokio::test]
    async fn a_settle_never_touches_a_claim_another_request_took() {
        let (fake, store) = fake();
        let key = shared_claim_key(ORG, USER, "send-1");
        let theirs = SharedClaimValue {
            request_id: "req-theirs".into(),
            run_id: None,
            thread_id: None,
            state: SharedClaimState::Pending,
        };
        assert_eq!(store.claim(&key, &theirs).await, SharedClaimOutcome::Won);
        store.settle(&key, "req-mine", None).await;
        assert_eq!(fake.held(&key), Some(theirs));
    }

    /// Lets every ready task run (a renewal tick, a settle) without moving the paused clock. A
    /// yield keeps the test ready, so the runtime never advances time on its own meanwhile.
    async fn run_ready_tasks() {
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_pending_claim_outlives_its_ttl_while_its_stream_runs() {
        let (fake, store) = fake();
        let buffers = StreamBufferStore::new();
        let reg = IdempotencyRegistry::new();
        let (guard, attempt) = send(&reg, &store, ORG, "req-A", &buffers).await;
        assert_eq!(attempt, SharedAttempt::Owner);
        let key = shared_claim_key(ORG, USER, "send-1");
        run_ready_tasks().await;
        // Four renewals, the last at 160 s: the claim's own TTL would have run out at 120 s.
        for _ in 0..4 {
            tokio::time::advance(PENDING_TTL / 3).await;
            run_ready_tasks().await;
        }
        assert_eq!(fake.renewals(), 4);
        assert!(
            fake.held(&key).is_some(),
            "a live stream keeps its claim past PENDING_TTL"
        );
        assert_eq!(
            fake.ttl(&key),
            Some(PENDING_TTL),
            "the last renewal restored the full TTL"
        );
        drop(guard);
    }

    #[tokio::test(start_paused = true)]
    async fn the_renewal_stops_once_the_stream_ends_and_settle_takes_over() {
        let (fake, store) = fake();
        let buffers = StreamBufferStore::new();
        let reg = IdempotencyRegistry::new();
        let (mut guard, attempt) = send(&reg, &store, ORG, "req-A", &buffers).await;
        assert_eq!(attempt, SharedAttempt::Owner);
        let key = shared_claim_key(ORG, USER, "send-1");
        run_ready_tasks().await;
        tokio::time::advance(PENDING_TTL / 3).await;
        assert_eq!(fake.renewals(), 1);
        guard.record_run("run-1", "thr-1");
        let buffer_key = crate::stream_buffer::scoped_stream_key(ORG, USER, "req-A");
        buffers
            .finish(
                &buffer_key,
                crate::stream_buffer::StreamDone {
                    seq: 3,
                    model_used: "m".into(),
                    input_tokens: 1,
                    output_tokens: 1,
                },
            )
            .await;
        drop(guard);
        run_ready_tasks().await;
        assert_eq!(
            fake.held(&key).map(|value| value.state),
            Some(SharedClaimState::Done)
        );
        assert_eq!(
            fake.ttl(&key),
            Some(DONE_TTL),
            "settle wrote the Done value for DONE_TTL"
        );
        // Three renewal periods later: no renewal ran, and the Done value was not extended.
        tokio::time::advance(PENDING_TTL * 3).await;
        run_ready_tasks().await;
        assert_eq!(fake.renewals(), 1, "no renewal after the guard dropped");
        assert_eq!(fake.ttl(&key), Some(DONE_TTL - PENDING_TTL * 3));
    }

    #[tokio::test(start_paused = true)]
    async fn renew_never_extends_a_claim_another_request_owns() {
        let (fake, store) = fake();
        let key = shared_claim_key(ORG, USER, "send-1");
        let theirs = SharedClaimValue {
            request_id: "req-theirs".into(),
            run_id: None,
            thread_id: None,
            state: SharedClaimState::Pending,
        };
        assert_eq!(store.claim(&key, &theirs).await, SharedClaimOutcome::Won);
        tokio::time::advance(Duration::from_secs(100)).await;
        assert!(
            !store.renew(&key, "req-mine").await,
            "another request's claim is not ours"
        );
        tokio::time::advance(Duration::from_secs(30)).await;
        assert_eq!(
            fake.held(&key),
            None,
            "nothing extended it, so it ran out at 120 s"
        );
        // Once this request has settled its own claim as Done, a renewal leaves it alone too.
        let done_key = shared_claim_key(ORG, USER, "send-2");
        let mine = SharedClaimValue {
            request_id: "req-mine".into(),
            ..theirs.clone()
        };
        assert_eq!(store.claim(&done_key, &mine).await, SharedClaimOutcome::Won);
        let done = SharedClaimValue {
            state: SharedClaimState::Done,
            ..mine.clone()
        };
        store.settle(&done_key, "req-mine", Some(&done)).await;
        assert!(
            !store.renew(&done_key, "req-mine").await,
            "a Done claim is never re-extended"
        );
        assert_eq!(fake.ttl(&done_key), Some(DONE_TTL));
    }

    #[tokio::test(start_paused = true)]
    async fn an_outage_does_not_end_the_renewal() {
        let (fake, store) = fake();
        let buffers = StreamBufferStore::new();
        let reg = IdempotencyRegistry::new();
        let (guard, attempt) = send(&reg, &store, ORG, "req-A", &buffers).await;
        assert_eq!(attempt, SharedAttempt::Owner);
        let key = shared_claim_key(ORG, USER, "send-1");
        run_ready_tasks().await;
        // The renewal at 40 s lands in an outage; the one at 80 s, after recovery, extends it.
        fake.down.store(true, Ordering::SeqCst);
        tokio::time::advance(PENDING_TTL / 3).await;
        fake.down.store(false, Ordering::SeqCst);
        tokio::time::advance(PENDING_TTL / 3).await;
        run_ready_tasks().await;
        assert_eq!(
            fake.renewals(),
            2,
            "the renewal in the outage did not end the loop"
        );
        // Past the claim's original 120 s: only the 80 s renewal's extension holds it now.
        tokio::time::advance(Duration::from_secs(50)).await;
        assert!(
            fake.held(&key).is_some(),
            "the renewal after the outage extended the claim"
        );
        drop(guard);
    }

    #[tokio::test(start_paused = true)]
    async fn dropping_the_release_stops_its_renewal() {
        let (fake, store) = fake();
        let buffers = StreamBufferStore::new();
        let reg = IdempotencyRegistry::new();
        let (mut guard, attempt) = send(&reg, &store, ORG, "req-A", &buffers).await;
        assert_eq!(attempt, SharedAttempt::Owner);
        let key = shared_claim_key(ORG, USER, "send-1");
        run_ready_tasks().await;
        // A release dropped without a settle (a replaced claim, a drop with no runtime) must not
        // leave its renewal running for a claim nobody will settle.
        drop(guard.shared.take());
        tokio::time::advance(PENDING_TTL * 3).await;
        run_ready_tasks().await;
        assert_eq!(fake.renewals(), 0);
        assert_eq!(
            fake.held(&key),
            None,
            "with no renewal the claim ran out at 120 s"
        );
    }
}
