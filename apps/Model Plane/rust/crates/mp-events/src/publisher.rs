//! Publisher trait for event envelopes.
//!
//! Concrete implementations:
//! - `InMemoryPublisher` — for tests and local development.
//! - NATS publisher — behind the `nats` feature flag.

use crate::envelope::Envelope;
use std::future::Future;

/// Trait for publishing event envelopes to a message bus.
pub trait EventPublisher: Send + Sync {
    /// Publish an envelope to the given subject.
    fn publish(
        &self,
        subject: &str,
        envelope: &Envelope,
    ) -> impl Future<Output = Result<(), PublishError>> + Send;
}

/// Errors from publishing.
#[derive(Debug, thiserror::Error)]
pub enum PublishError {
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("transport error: {0}")]
    Transport(String),
}

/// In-memory publisher that records published envelopes for testing.
#[derive(Default)]
pub struct InMemoryPublisher {
    published: std::sync::Mutex<Vec<(String, Envelope)>>,
}

impl InMemoryPublisher {
    /// Create a new in-memory publisher.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Return all published (subject, envelope) pairs.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    #[must_use]
    pub fn drain(&self) -> Vec<(String, Envelope)> {
        let mut lock = self.published.lock().expect("mutex not poisoned");
        std::mem::take(&mut *lock)
    }
}

impl EventPublisher for InMemoryPublisher {
    async fn publish(&self, subject: &str, envelope: &Envelope) -> Result<(), PublishError> {
        let mut lock = self.published.lock().expect("mutex not poisoned");
        lock.push((subject.to_owned(), envelope.clone()));
        Ok(())
    }
}
