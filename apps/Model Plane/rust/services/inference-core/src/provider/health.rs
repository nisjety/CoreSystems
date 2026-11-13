//! Failure classes for provider calls, and a bench board that keeps a failing
//! provider and model from being tried again on every request.
//!
//! Pure: no I/O and no environment reads. The clock is passed in as `now`, a
//! `tokio::time::Instant` (the clock the provider chain already reads, so a paused
//! test clock can drive it). Nothing here sees prompt text, content or tool
//! arguments, and nothing is logged. A provider's error text is read only to pick
//! a [`FailureClass`]; the text is never stored, logged or labelled.
//!
//! Inert until a caller consults it. A later unit wires the board into the
//! fallback chain behind a default-off switch, so no routing decision changes on
//! its own.
//!
//! # Classes
//!
//! [`classify`] maps a [`ProviderError`] to a [`FailureClass`], or to `None` when
//! the failure says nothing about the provider's health: a gate refusal (residency,
//! privacy tier, entitlement, vision), a configuration fault, an exhausted chain, or
//! a chain whose every provider is benched. Its match has no wildcard arm, so a new
//! variant does not compile until it is classified.
//!
//! An HTTP failure is read from its text only in the form the providers build it:
//! a leading three-digit status (`"503: body"`, or reqwest's `"503 Service
//! Unavailable"`), or, with no status, a transport error whose text says it timed
//! out. A bare 429 with no quota word is `None`, because the chat providers read a
//! 429 as a typed `RateLimited` before they build any text.
//!
//! # Benching
//!
//! [`benches`] says which classes can bench a provider. Auth and Quota bench at
//! once, and so does a rate limit that declares a retry-after above zero. Server,
//! Timeout, Empty, and a rate limit with no declared retry-after bench only after
//! [`CONSECUTIVE_FAILURES_TO_BENCH`] failures in a row from at least
//! [`MIN_DISTINCT_ORGS`] distinct orgs. The guard exists because a Server, Timeout
//! or Empty failure can be provoked by one caller's own input, such as a huge or
//! slow prompt, and the board is shared by every org on a replica: one tenant must
//! not bench a platform provider for everyone.
//!
//! A single-org deployment therefore never benches on Server, Timeout or Empty; it
//! benches only on Auth, Quota and declared rate limits. That is the price of the
//! guard.
//!
//! A run is the benching failures counted since the last success or the last bench
//! started. A bench lasts [`BENCH_BASE`], doubled for each repeat since the last
//! success and capped at [`BENCH_MAX`]. A declared retry-after sets the duration
//! instead, clamped to [`BENCH_MAX`]. Failures that arrive while a bench runs change
//! nothing: they were in flight when it began. When a bench ends, the entry is
//! half-open: the next call is tried and a new run starts, so a repeat bench needs
//! the same run as the first one (a single failure, for Auth and Quota). A success
//! clears the entry.
//!
//! The map holds at most [`MAX_ENTRIES`] entries. When it is full, expired entries
//! go first, then the least recently seen.
//!
//! # Storage
//!
//! The board lives in process, one per replica. A bench is a latency optimisation
//! whose worst case is one extra failed call per replica, so a shared store would
//! add a hot-path dependency for little gain. Org ids are kept only as
//! process-local hashes, for the current run.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tokio::time::Instant;

use super::{ProviderError, SubscriptionRefusal};

/// Consecutive benching failures a guarded class needs before the board benches.
pub const CONSECUTIVE_FAILURES_TO_BENCH: u32 = 3;

/// Distinct org ids those failures must come from. Also the most org fingerprints
/// an entry keeps for its run.
pub const MIN_DISTINCT_ORGS: usize = 2;

/// The first bench of a run. Each repeat since the last success doubles it.
pub const BENCH_BASE: Duration = Duration::from_secs(30);

/// The longest any bench lasts, a declared retry-after included.
pub const BENCH_MAX: Duration = Duration::from_secs(300);

/// The most doublings of [`BENCH_BASE`]. Four doublings already exceed [`BENCH_MAX`].
const MAX_BENCH_EXPONENT: u32 = 4;

/// The most entries the board holds at once.
pub const MAX_ENTRIES: usize = 1_024;

/// Words a 429 body uses when the limit is the account's quota or billing rather
/// than a request rate. `insufficient_quota` contains `quota`.
const QUOTA_WORDS: [&str; 2] = ["quota", "billing"];

/// Why a provider call failed, in a closed vocabulary.
///
/// Each class has one fixed label ([`FailureClass::as_str`]), so a provider's free
/// text never reaches a log, a metric or an operator through this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FailureClass {
    /// The provider rejected the credential behind the call (401, 403), or a
    /// subscription sign-in must be renewed.
    Auth,
    /// The quota or billing behind the call is exhausted: a 402, a 429 with a quota or
    /// billing word, or a subscription usage limit.
    Quota,
    /// The provider throttled the call: a typed rate limit.
    RateLimit,
    /// The prompt exceeded the model's input window. A property of the request.
    ContextTooLong,
    /// The provider refused the request as malformed. A second send gets the same
    /// answer.
    MalformedRequest,
    /// The subscription refused the request for a reason that is neither a usage
    /// limit nor a sign-in renewal. It says nothing about the provider's health.
    Refusal,
    /// The provider failed on its side: a 5xx, a broker fault, or a transport error
    /// that is not a timeout.
    Server,
    /// The call timed out: a 408 or 504, or a transport timeout.
    Timeout,
    /// The provider answered with no usable text.
    Empty,
}

impl FailureClass {
    /// Every class, in declaration order.
    pub const ALL: [Self; 9] = [
        Self::Auth,
        Self::Quota,
        Self::RateLimit,
        Self::ContextTooLong,
        Self::MalformedRequest,
        Self::Refusal,
        Self::Server,
        Self::Timeout,
        Self::Empty,
    ];

    /// The stable label for this class: `auth`, `quota`, `rate_limit`,
    /// `context_too_long`, `malformed_request`, `refusal`, `server`, `timeout` or
    /// `empty`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auth => "auth",
            Self::Quota => "quota",
            Self::RateLimit => "rate_limit",
            Self::ContextTooLong => "context_too_long",
            Self::MalformedRequest => "malformed_request",
            Self::Refusal => "refusal",
            Self::Server => "server",
            Self::Timeout => "timeout",
            Self::Empty => "empty",
        }
    }
}

/// The class of a provider failure, or `None` when the failure says nothing about the
/// provider's health.
///
/// A `SubscriptionRefused` comes from the caller's own subscription connection, not from
/// the platform's key. Whether such a failure reaches the board is for the unit that wires
/// the board in, not for this mapping.
#[must_use]
pub fn classify(error: &ProviderError) -> Option<FailureClass> {
    match error {
        ProviderError::Http(detail) => classify_http(detail),
        ProviderError::Unavailable(detail) => {
            leading_status(detail).and_then(|status| classify_status(status, detail))
        }
        ProviderError::RateLimited { .. } => Some(FailureClass::RateLimit),
        ProviderError::SubscriptionRefused { reason, .. } => match reason {
            SubscriptionRefusal::UsageLimit => Some(FailureClass::Quota),
            SubscriptionRefusal::ReauthRequired => Some(FailureClass::Auth),
            SubscriptionRefusal::UpstreamUnavailable | SubscriptionRefusal::BadRequest => {
                Some(FailureClass::Refusal)
            }
        },
        ProviderError::InvalidResponse(text) if text.trim().is_empty() => Some(FailureClass::Empty),
        ProviderError::InvalidResponse(_) | ProviderError::SubscriptionFailed(_) => {
            Some(FailureClass::Server)
        }
        ProviderError::TooLong { .. } => Some(FailureClass::ContextTooLong),
        ProviderError::RequestRejected { .. } => Some(FailureClass::MalformedRequest),
        ProviderError::AllExhausted { .. }
        | ProviderError::UnsupportedModel(_)
        | ProviderError::ResidencyViolation(_)
        | ProviderError::ZdrUnavailable(_)
        | ProviderError::TierUnavailable(_)
        | ProviderError::VisionUnsupported(_)
        | ProviderError::ModelNotEntitled { .. }
        | ProviderError::EntitlementUnavailable
        | ProviderError::ModelHidden { .. }
        | ProviderError::ImageModelUnavailable { .. }
        | ProviderError::ProvidersBenched { .. } => None,
    }
}

/// Whether a failure of `class` can bench a provider at all.
///
/// Auth, Quota, `RateLimit`, Server, Timeout and Empty are facts about the provider or
/// the platform's account. `ContextTooLong`, `MalformedRequest` and Refusal are about
/// this request, so they never touch the board.
#[must_use]
pub fn benches(class: FailureClass) -> bool {
    match class {
        FailureClass::Auth
        | FailureClass::Quota
        | FailureClass::RateLimit
        | FailureClass::Server
        | FailureClass::Timeout
        | FailureClass::Empty => true,
        FailureClass::ContextTooLong | FailureClass::MalformedRequest | FailureClass::Refusal => {
            false
        }
    }
}

/// Classifies an `Http` failure: a leading status decides it, and with no status the
/// text decides between a timeout and a server fault.
fn classify_http(detail: &str) -> Option<FailureClass> {
    match leading_status(detail) {
        Some(status) => classify_status(status, detail),
        None if mentions_timeout(detail) => Some(FailureClass::Timeout),
        None => Some(FailureClass::Server),
    }
}

/// Classifies an HTTP status, with the body text read only for a 429.
fn classify_status(status: u16, detail: &str) -> Option<FailureClass> {
    match status {
        401 | 403 => Some(FailureClass::Auth),
        402 => Some(FailureClass::Quota),
        429 if mentions_quota(detail) => Some(FailureClass::Quota),
        408 | 504 => Some(FailureClass::Timeout),
        500..=599 => Some(FailureClass::Server),
        _ => None,
    }
}

/// The three-digit HTTP status that opens `detail`, when it does.
///
/// Providers build an HTTP failure as `"<status>: <body>"`, and reqwest's status
/// display starts with the same three digits (`"503 Service Unavailable"`). Any other
/// leading digit run, such as a four-digit number, is not a status.
fn leading_status(detail: &str) -> Option<u16> {
    let trimmed = detail.trim_start();
    if trimmed.bytes().take_while(u8::is_ascii_digit).count() != 3 {
        return None;
    }
    trimmed.get(..3)?.parse().ok()
}

/// Whether a transport error's text says it timed out, in either spelling.
fn mentions_timeout(detail: &str) -> bool {
    let lower = detail.to_ascii_lowercase();
    lower.contains("timed out") || lower.contains("timeout")
}

/// Whether a 429 body names a quota or billing limit.
fn mentions_quota(detail: &str) -> bool {
    let lower = detail.to_ascii_lowercase();
    QUOTA_WORDS.iter().any(|word| lower.contains(word))
}

/// Whether one failure benches on its own: an Auth or Quota fault, or a rate limit that
/// declares when to retry. The other benching classes need the run guard.
fn benches_at_once(class: FailureClass, declared: Option<Duration>) -> bool {
    match class {
        FailureClass::Auth | FailureClass::Quota => true,
        FailureClass::RateLimit => declared.is_some(),
        FailureClass::ContextTooLong
        | FailureClass::MalformedRequest
        | FailureClass::Refusal
        | FailureClass::Server
        | FailureClass::Timeout
        | FailureClass::Empty => false,
    }
}

/// The bench for the `repeats`-th bench of a run with no declared retry-after:
/// [`BENCH_BASE`] doubled for each repeat after the first, capped at [`BENCH_MAX`].
fn bench_duration(repeats: u32) -> Duration {
    let exponent = repeats.saturating_sub(1).min(MAX_BENCH_EXPONENT);
    BENCH_BASE.saturating_mul(1_u32 << exponent).min(BENCH_MAX)
}

/// A board key: the provider and model ids, lowercased.
type BoardKey = (String, String);

fn board_key(provider: &str, model: &str) -> BoardKey {
    (provider.to_ascii_lowercase(), model.to_ascii_lowercase())
}

/// A process-local hash of an org id, so the board never holds the id text.
fn org_fingerprint(org_id: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    org_id.hash(&mut hasher);
    hasher.finish()
}

/// Makes room for one new entry once the map is full: expired entries go first, then
/// the least recently seen.
fn make_room(entries: &mut HashMap<BoardKey, BenchState>, now: Instant) {
    if entries.len() < MAX_ENTRIES {
        return;
    }
    entries.retain(|_, state| !state.ended(now));
    while entries.len() >= MAX_ENTRIES {
        let Some(oldest) = entries
            .iter()
            .min_by_key(|(_, state)| state.last_seen)
            .map(|(key, _)| key.clone())
        else {
            break;
        };
        entries.remove(&oldest);
    }
}

/// One provider and model's run of failures, and its bench if it has one.
struct BenchState {
    /// Benching failures in the current run.
    failures: u32,
    /// Fingerprints of the orgs in the current run, at most [`MIN_DISTINCT_ORGS`].
    org_fingerprints: Vec<u64>,
    /// Benches started since the last success. Drives the doubling.
    benches: u32,
    /// When the current or most recent bench ends. `None` before the first bench.
    benched_until: Option<Instant>,
    /// The last time a benching failure was recorded here. Orders eviction.
    last_seen: Instant,
}

impl BenchState {
    fn new(now: Instant) -> Self {
        Self {
            failures: 0,
            org_fingerprints: Vec::new(),
            benches: 0,
            benched_until: None,
            last_seen: now,
        }
    }

    /// The end of the bench, while it runs at `now`.
    fn running_until(&self, now: Instant) -> Option<Instant> {
        self.benched_until.filter(|until| now < *until)
    }

    /// Whether a bench has run out by `now`. This is the half-open state, and eviction
    /// drops these entries first.
    fn ended(&self, now: Instant) -> bool {
        self.benched_until.is_some_and(|until| until <= now)
    }

    /// Starts a bench at `now` and ends the current run, so the next run is judged
    /// afresh. `declared` is the provider's retry-after when it gave one above zero.
    fn start_bench(&mut self, now: Instant, declared: Option<Duration>) -> Instant {
        self.benches = self.benches.saturating_add(1);
        let duration = match declared {
            Some(after) => after.min(BENCH_MAX),
            None => bench_duration(self.benches),
        };
        let until = now + duration;
        self.benched_until = Some(until);
        self.failures = 0;
        self.org_fingerprints.clear();
        until
    }
}

/// The bench board: each provider and model's run of failures and any bench, shared by
/// every org on a replica.
///
/// Keys are the provider and model ids, lowercased. Entries hold timestamps and
/// process-local org hashes, never content.
#[derive(Default)]
pub struct BenchBoard {
    entries: Mutex<HashMap<BoardKey, BenchState>>,
}

impl std::fmt::Debug for BenchBoard {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The entries are left out on purpose: they hold org hashes and run state.
        formatter.debug_struct("BenchBoard").finish_non_exhaustive()
    }
}

impl BenchBoard {
    /// An empty board.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<BoardKey, BenchState>> {
        // A poisoned lock only means a panic held it elsewhere. The map is still a
        // valid map, so the board keeps working rather than panicking on every call.
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The end of `(provider, model)`'s bench, while it runs at `now`.
    ///
    /// `None` when the pair is not benched, or its bench has ended: an ended bench is
    /// half-open, so the next call is tried.
    #[must_use]
    pub fn benched_until(&self, provider: &str, model: &str, now: Instant) -> Option<Instant> {
        self.lock()
            .get(&board_key(provider, model))
            .and_then(|state| state.running_until(now))
    }

    /// Records one failed call to `(provider, model)` made for `org_id`, and returns the
    /// new bench end when this failure starts a bench.
    ///
    /// A failure whose class does not bench (see [`benches`]) changes nothing. Neither
    /// does a failure that arrives while a bench runs: it was in flight when the bench
    /// began, and counting it would double the bench for one outage. A zero
    /// `retry_after` declares nothing.
    ///
    /// Auth, Quota and a declared rate limit bench at once. Every other benching class
    /// benches when the run holds [`CONSECUTIVE_FAILURES_TO_BENCH`] failures from at
    /// least [`MIN_DISTINCT_ORGS`] distinct orgs.
    pub fn record_failure(
        &self,
        provider: &str,
        model: &str,
        org_id: &str,
        class: FailureClass,
        retry_after: Option<Duration>,
        now: Instant,
    ) -> Option<Instant> {
        if !benches(class) {
            return None;
        }
        let declared = retry_after.filter(|after| !after.is_zero());
        let key = board_key(provider, model);
        let mut entries = self.lock();
        if !entries.contains_key(&key) {
            make_room(&mut entries, now);
        }
        let state = entries.entry(key).or_insert_with(|| BenchState::new(now));
        if state.running_until(now).is_some() {
            return None;
        }
        state.last_seen = now;
        state.failures = state.failures.saturating_add(1);
        let fingerprint = org_fingerprint(org_id);
        if state.org_fingerprints.len() < MIN_DISTINCT_ORGS
            && !state.org_fingerprints.contains(&fingerprint)
        {
            state.org_fingerprints.push(fingerprint);
        }
        let guard_met = state.failures >= CONSECUTIVE_FAILURES_TO_BENCH
            && state.org_fingerprints.len() >= MIN_DISTINCT_ORGS;
        if benches_at_once(class, declared) || guard_met {
            Some(state.start_bench(now, declared))
        } else {
            None
        }
    }

    /// Clears `(provider, model)`'s run and bench. Called after a successful call: a
    /// success ends the run, and the next failure starts a new one at the base duration.
    pub fn record_success(&self, provider: &str, model: &str) {
        self.lock().remove(&board_key(provider, model));
    }
}

/// Failure classes and the bench board, under an injected clock (R2-OD-40-a).
#[cfg(test)]
#[path = "health_tests.rs"]
mod tests;
