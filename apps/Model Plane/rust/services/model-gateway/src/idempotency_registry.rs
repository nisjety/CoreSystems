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
//! Cheap (`DashMap` + `Instant`), no new deps. In-memory + single-replica today
//! — promote the cache to session-core/Postgres for durable, cross-replica
//! dedup later (same matrix-style note as `cancel_registry`).
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

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dashmap::DashMap;

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
}
