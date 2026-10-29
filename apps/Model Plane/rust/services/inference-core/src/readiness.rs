//! Process-local readiness for the authenticated gRPC listener.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// Starts fail-closed and becomes ready only after the gRPC socket bind succeeds.
#[derive(Clone, Default)]
pub struct GrpcReadiness(Arc<AtomicBool>);

impl GrpcReadiness {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mark_bound(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn mark_unbound(&self) {
        self.0.store(false, Ordering::Release);
    }

    #[must_use]
    pub fn is_bound(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}
