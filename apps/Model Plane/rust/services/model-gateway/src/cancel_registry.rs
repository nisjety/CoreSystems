//! Active in-flight stream registry for cooperative cancellation (chat-parity §4).
//!
//! `/v1/invoke/stream` registers a flag per `request_id`; `POST
//! /v1/invoke/{request_id}/cancel` flips it; the SSE loop polls it each
//! iteration and emits a terminal `stopped` event. Each flag is bound to the
//! authenticated tenant/user so a request id cannot be used as a cross-tenant
//! cancel oracle. Cheap (a `DashMap` of `AtomicBool`), no new deps. In-memory
//! + single-replica today — promote to a NATS cancel subject for multi-replica
//! later (matrix-style note).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use dashmap::DashMap;

/// Tracks cancellable in-flight streams by `request_id`.
#[derive(Clone, Default)]
pub struct CancelRegistry {
    inner: Arc<DashMap<String, CancelEntry>>,
}

struct CancelEntry {
    flag: Arc<AtomicBool>,
    org_id: String,
    user_id: String,
}

impl CancelRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `request_id` as an active, cancellable stream. Returns the flag
    /// the stream loop polls; call [`finish`](Self::finish) when the stream ends.
    #[must_use]
    pub fn register(&self, request_id: &str, org_id: &str, user_id: &str) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.inner.insert(
            request_id.to_owned(),
            CancelEntry {
                flag: flag.clone(),
                org_id: org_id.to_owned(),
                user_id: user_id.to_owned(),
            },
        );
        flag
    }

    /// Request cancellation of an in-flight stream. Returns `true` if a matching
    /// active stream was found (so the caller can return 404 otherwise).
    pub fn cancel_for(&self, request_id: &str, org_id: &str, user_id: &str) -> bool {
        let Some(entry) = self.inner.get(request_id) else {
            return false;
        };
        if entry.org_id != org_id || entry.user_id != user_id {
            return false;
        }
        entry.flag.store(true, Ordering::Relaxed);
        true
    }

    #[cfg(test)]
    fn cancel(&self, request_id: &str) -> bool {
        self.inner.get(request_id).is_some_and(|entry| {
            entry.flag.store(true, Ordering::Relaxed);
            true
        })
    }

    /// Stop tracking a finished stream (idempotent).
    pub fn finish(&self, request_id: &str) {
        self.inner.remove(request_id);
    }

    /// Internal cooperative polling; callers use the already-authorized run id.
    pub(crate) fn is_cancelled(&self, request_id: &str) -> bool {
        self.inner.get(request_id).is_some_and(|entry| entry.flag.load(Ordering::Relaxed))
    }

    /// Number of currently-tracked active streams.
    #[must_use]
    pub fn active(&self) -> usize {
        self.inner.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_then_cancel_sets_the_flag() {
        let reg = CancelRegistry::new();
        let flag = reg.register("req-1", "org-a", "user-a");
        assert!(!flag.load(Ordering::Relaxed), "starts un-cancelled");
        assert!(
            reg.cancel_for("req-1", "org-a", "user-a"),
            "found the active stream"
        );
        assert!(
            flag.load(Ordering::Relaxed),
            "flag flipped — the loop will stop"
        );
    }

    #[test]
    fn cancel_unknown_request_returns_false() {
        let reg = CancelRegistry::new();
        assert!(!reg.cancel("nope"));
    }

    #[test]
    fn cancel_rejects_a_different_tenant_or_user() {
        let reg = CancelRegistry::new();
        let flag = reg.register("req-tenant", "org-a", "user-a");
        assert!(!reg.cancel_for("req-tenant", "org-b", "user-a"));
        assert!(!reg.cancel_for("req-tenant", "org-a", "user-b"));
        assert!(!flag.load(Ordering::Relaxed));
        assert!(reg.cancel_for("req-tenant", "org-a", "user-a"));
    }

    #[test]
    fn finish_stops_tracking_so_later_cancel_is_a_noop() {
        let reg = CancelRegistry::new();
        let flag = reg.register("req-2", "org-a", "user-a");
        assert_eq!(reg.active(), 1);
        reg.finish("req-2");
        assert_eq!(reg.active(), 0);
        // A late cancel (client raced the stream finishing) finds nothing.
        assert!(!reg.cancel("req-2"));
        assert!(!flag.load(Ordering::Relaxed));
    }
}
