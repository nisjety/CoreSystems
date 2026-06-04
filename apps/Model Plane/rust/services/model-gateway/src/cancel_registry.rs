//! Active in-flight stream registry for cooperative cancellation (chat-parity §4).
//!
//! `/v1/invoke/stream` registers a flag per `request_id`; `POST
//! /v1/invoke/{request_id}/cancel` flips it; the SSE loop polls it each
//! iteration and emits a terminal `stopped` event. Cheap (a `DashMap` of
//! `AtomicBool`), no new deps. In-memory + single-replica today — promote to a
//! NATS cancel subject for multi-replica later (matrix-style note).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use dashmap::DashMap;

/// Tracks cancellable in-flight streams by `request_id`.
#[derive(Clone, Default)]
pub struct CancelRegistry {
    inner: Arc<DashMap<String, Arc<AtomicBool>>>,
}

impl CancelRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `request_id` as an active, cancellable stream. Returns the flag
    /// the stream loop polls; call [`finish`](Self::finish) when the stream ends.
    #[must_use]
    pub fn register(&self, request_id: &str) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.inner.insert(request_id.to_owned(), flag.clone());
        flag
    }

    /// Request cancellation of an in-flight stream. Returns `true` if a matching
    /// active stream was found (so the caller can return 404 otherwise).
    pub fn cancel(&self, request_id: &str) -> bool {
        if let Some(flag) = self.inner.get(request_id) {
            flag.store(true, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    /// Stop tracking a finished stream (idempotent).
    pub fn finish(&self, request_id: &str) {
        self.inner.remove(request_id);
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
        let flag = reg.register("req-1");
        assert!(!flag.load(Ordering::Relaxed), "starts un-cancelled");
        assert!(reg.cancel("req-1"), "found the active stream");
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
    fn finish_stops_tracking_so_later_cancel_is_a_noop() {
        let reg = CancelRegistry::new();
        let flag = reg.register("req-2");
        assert_eq!(reg.active(), 1);
        reg.finish("req-2");
        assert_eq!(reg.active(), 0);
        // A late cancel (client raced the stream finishing) finds nothing.
        assert!(!reg.cancel("req-2"));
        assert!(!flag.load(Ordering::Relaxed));
    }
}
