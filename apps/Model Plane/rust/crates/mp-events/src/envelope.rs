//! The canonical event envelope mirroring the proto `Event` message.
//!
//! This is the Rust-native representation used for JSON encoding on NATS
//! and for the append-only events table in Postgres.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Canonical event envelope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Envelope {
    /// Globally unique event identifier (ULID).
    pub event_id: String,

    /// Discriminator string (e.g. `SESSION_START`, `RUN_COMPLETED`).
    pub event_type: String,

    /// Schema version for this event type.
    pub schema_version: u32,

    /// Wall-clock time when the event was produced.
    pub ts: DateTime<Utc>,

    /// Service that produced this event.
    pub producer: String,

    /// Trace correlation ID.
    pub correlation_id: String,

    /// ID of the event that directly caused this event.
    pub causation_id: String,

    /// Client-supplied deduplication key.
    pub idempotency_key: String,

    /// Tenant identifier.
    pub org_id: String,

    /// Acting user.
    pub user_id: String,

    /// Canonical resource reference (e.g. "thread/{id}").
    pub resource_ref: String,

    /// Type-specific payload as arbitrary JSON.
    pub payload: serde_json::Value,

    /// Zero Data Retention mode. When true, this event must not be persisted durably.
    #[serde(default)]
    pub zdr: bool,
}

impl Envelope {
    /// Validate that all required envelope fields are present and non-empty.
    ///
    /// # Errors
    ///
    /// Returns an error describing the first missing required field.
    pub fn validate(&self) -> Result<(), EnvelopeError> {
        if self.event_id.is_empty() {
            return Err(EnvelopeError::MissingField("event_id"));
        }
        if self.event_type.is_empty() {
            return Err(EnvelopeError::MissingField("event_type"));
        }
        if self.schema_version == 0 {
            return Err(EnvelopeError::MissingField("schema_version"));
        }
        if self.producer.is_empty() {
            return Err(EnvelopeError::MissingField("producer"));
        }
        if self.org_id.is_empty() {
            return Err(EnvelopeError::MissingField("org_id"));
        }
        if self.ts.timestamp_millis() <= 0 {
            return Err(EnvelopeError::MissingField("ts"));
        }
        if self.correlation_id.is_empty() {
            return Err(EnvelopeError::MissingField("correlation_id"));
        }
        if self.idempotency_key.is_empty() {
            return Err(EnvelopeError::MissingField("idempotency_key"));
        }
        if self.user_id.is_empty() {
            return Err(EnvelopeError::MissingField("user_id"));
        }
        if self.resource_ref.is_empty() {
            return Err(EnvelopeError::MissingField("resource_ref"));
        }
        Ok(())
    }

    /// Encode this envelope to JSON bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails.
    pub fn to_json_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    /// Decode an envelope from JSON bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if deserialization fails.
    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}

/// Errors from envelope operations.
#[derive(Debug, thiserror::Error)]
pub enum EnvelopeError {
    #[error("missing required field: {0}")]
    MissingField(&'static str),
}
