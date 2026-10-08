//! Process-local readiness for the additive gRPC compatibility listener, and
//! the draining flag a graceful shutdown raises (see [`crate::drain`]).

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// Starts fail-closed and becomes ready only after the gRPC socket bind succeeds.
///
/// It also carries the process's draining flag. One handle is cloned into the
/// HTTP router (`/readyz`), the gRPC listener and `main`, so the shutdown path
/// can take the instance out of rotation without a second piece of shared state.
#[derive(Clone, Default)]
pub struct GrpcReadiness {
    bound: Arc<AtomicBool>,
    draining: Arc<AtomicBool>,
}

impl GrpcReadiness {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mark_bound(&self) {
        self.bound.store(true, Ordering::Release);
    }

    pub fn mark_unbound(&self) {
        self.bound.store(false, Ordering::Release);
    }

    #[must_use]
    pub fn is_bound(&self) -> bool {
        self.bound.load(Ordering::Acquire)
    }

    /// Mark the process as shutting down: it keeps serving the work it has, but
    /// `/readyz` answers 503 from now on so nothing new is routed to it. One
    /// way only, there is no un-drain.
    pub fn begin_drain(&self) {
        self.draining.store(true, Ordering::Release);
    }

    #[must_use]
    pub fn is_draining(&self) -> bool {
        self.draining.load(Ordering::Acquire)
    }
}
