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

use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;

/// How long a completed result stays replayable for its key.
const DONE_TTL: Duration = Duration::from_secs(600);
/// How long a `Pending` claim is honored before it is treated as abandoned
/// (e.g. the original request's process died mid-flight). Keeps a crashed
/// request from wedging its key forever.
const PENDING_TTL: Duration = Duration::from_secs(120);

/// The cacheable shape of a completed `/v1/invoke` response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedInvoke {
    pub request_id: String,
    pub content: String,
    pub model_used: String,
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
}

/// Tracks in-flight + recently-completed invokes keyed by client idempotency key.
#[derive(Clone, Default)]
pub struct IdempotencyRegistry {
    inner: Arc<DashMap<String, Entry>>,
}

impl IdempotencyRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Claim `key` for a new request. See [`Claim`] for the three outcomes.
    /// Expired `Done` entries and abandoned `Pending` claims are reclaimed.
    pub fn claim(&self, key: &str) -> Claim {
        let now = Instant::now();
        if let Some(entry) = self.inner.get(key) {
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
        self.inner
            .insert(key.to_owned(), Entry::Pending { since: now });
        Claim::Proceed(CommitGuard {
            inner: self.inner.clone(),
            key: key.to_owned(),
            committed: false,
        })
    }

    /// Number of currently-tracked keys (Pending + cached Done).
    #[must_use]
    pub fn tracked(&self) -> usize {
        self.inner.len()
    }
}

/// RAII guard for a `Pending` claim. On `commit` the result is cached; if the
/// guard is dropped without committing (any early `?` return / error), the
/// claim is released so an immediate retry can proceed instead of getting
/// `InFlight` until the pending TTL elapses. Holds an `Arc` clone of the map
/// (not a borrow), so it is `'static` and can be held across `.await`.
pub struct CommitGuard {
    inner: Arc<DashMap<String, Entry>>,
    key: String,
    committed: bool,
}

impl CommitGuard {
    /// Cache the completed result under this key and release the claim.
    pub fn commit(mut self, value: CachedInvoke) {
        self.inner.insert(
            self.key.clone(),
            Entry::Done {
                value,
                at: Instant::now(),
            },
        );
        self.committed = true;
    }
}

impl Drop for CommitGuard {
    fn drop(&mut self) {
        if !self.committed {
            // Only drop our own still-Pending claim; never clobber a sibling's Done.
            self.inner
                .remove_if(&self.key, |_, e| matches!(e, Entry::Pending { .. }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(id: &str) -> CachedInvoke {
        CachedInvoke {
            request_id: id.to_owned(),
            content: "hello".to_owned(),
            model_used: "test-model".to_owned(),
        }
    }

    #[test]
    fn first_claim_proceeds_then_caches_on_commit() {
        let reg = IdempotencyRegistry::new();
        match reg.claim("k1") {
            Claim::Proceed(guard) => guard.commit(sample("req-1")),
            _ => panic!("first claim must Proceed"),
        }
        // Second claim with the same key returns the cached result verbatim.
        match reg.claim("k1") {
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
        let Claim::Proceed(_guard) = reg.claim("k2") else {
            panic!("first claim must Proceed");
        };
        assert!(
            matches!(reg.claim("k2"), Claim::InFlight),
            "a second claim while the first is in-flight is rejected"
        );
    }

    #[test]
    fn dropped_guard_without_commit_releases_the_claim() {
        let reg = IdempotencyRegistry::new();
        {
            let Claim::Proceed(_guard) = reg.claim("k3") else {
                panic!("first claim must Proceed");
            };
            // guard dropped here without commit (simulates an errored request)
        }
        assert_eq!(reg.tracked(), 0, "abandoned claim is released on drop");
        // A retry after the error proceeds rather than seeing InFlight.
        assert!(matches!(reg.claim("k3"), Claim::Proceed(_)));
    }

    #[test]
    fn distinct_keys_do_not_interfere() {
        let reg = IdempotencyRegistry::new();
        match reg.claim("a") {
            Claim::Proceed(g) => g.commit(sample("req-a")),
            _ => panic!(),
        }
        assert!(
            matches!(reg.claim("b"), Claim::Proceed(_)),
            "a different key is unaffected by another's cached result"
        );
    }
}
