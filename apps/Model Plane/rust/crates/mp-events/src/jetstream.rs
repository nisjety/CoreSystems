//! JetStream stream-as-code and consumer-as-code specifications.
//!
//! These types provide a declarative, language-agnostic description of the
//! NATS JetStream streams and consumers that back the Model Plane event bus.
//! The same JSON fixtures are consumed by the Go parity tests to keep the
//! Rust and Go implementations aligned byte-for-byte.

use serde::{Deserialize, Serialize};

/// Retention policy for a JetStream stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RetentionPolicy {
    /// Retain messages until limits are hit.
    Limits,
    /// Retain messages while interest (consumers) exists.
    Interest,
    /// Work-queue semantics; messages removed after ack.
    WorkQueue,
}

/// Storage backend for a JetStream stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StorageType {
    /// File-backed storage.
    File,
    /// In-memory storage.
    Memory,
}

/// Discard policy when stream limits are hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiscardPolicy {
    /// Discard oldest messages first.
    Old,
    /// Reject new messages.
    New,
}

/// Acknowledgement policy for a JetStream consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AckPolicy {
    /// Each message must be explicitly acked.
    Explicit,
    /// Ack of a sequence acks all prior messages.
    All,
    /// No acks required.
    None,
}

/// Declarative specification of a JetStream stream.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StreamSpec {
    /// Stream name (must be unique within the JetStream account).
    pub name: String,
    /// Subjects captured by this stream.
    pub subjects: Vec<String>,
    /// Retention policy.
    pub retention: RetentionPolicy,
    /// Storage backend.
    pub storage: StorageType,
    /// Maximum message age in seconds.
    pub max_age_secs: u64,
    /// Number of replicas (>= 1).
    pub replicas: u8,
    /// Discard policy when limits are hit.
    pub discard: DiscardPolicy,
}

impl StreamSpec {
    /// Validate that all required fields are present and well-formed.
    ///
    /// # Errors
    ///
    /// Returns [`JetStreamSpecError::MissingField`] if a required field is empty or zero.
    pub fn validate(&self) -> Result<(), JetStreamSpecError> {
        if self.name.is_empty() {
            return Err(JetStreamSpecError::MissingField("name"));
        }
        if self.subjects.is_empty() {
            return Err(JetStreamSpecError::MissingField("subjects"));
        }
        if self.subjects.iter().any(std::string::String::is_empty) {
            return Err(JetStreamSpecError::MissingField("subjects"));
        }
        if self.max_age_secs == 0 {
            return Err(JetStreamSpecError::MissingField("max_age_secs"));
        }
        if self.replicas == 0 {
            return Err(JetStreamSpecError::MissingField("replicas"));
        }
        Ok(())
    }

    /// Encode this spec to JSON bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails.
    pub fn to_json_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    /// Decode a spec from JSON bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if deserialization fails.
    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}

/// Declarative specification of a JetStream consumer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConsumerSpec {
    /// Durable consumer name.
    pub durable: String,
    /// Ack policy.
    pub ack_policy: AckPolicy,
    /// Acknowledgement wait in seconds.
    pub ack_wait_secs: u64,
    /// Maximum redelivery attempts.
    pub max_deliver: u32,
    /// Filter subject for this consumer.
    pub filter_subject: String,
}

impl ConsumerSpec {
    /// Validate that all required fields are present and well-formed.
    ///
    /// # Errors
    ///
    /// Returns [`JetStreamSpecError::MissingField`] if a required field is empty or zero.
    pub fn validate(&self) -> Result<(), JetStreamSpecError> {
        if self.durable.is_empty() {
            return Err(JetStreamSpecError::MissingField("durable"));
        }
        if self.ack_wait_secs == 0 {
            return Err(JetStreamSpecError::MissingField("ack_wait_secs"));
        }
        if self.max_deliver == 0 {
            return Err(JetStreamSpecError::MissingField("max_deliver"));
        }
        if self.filter_subject.is_empty() {
            return Err(JetStreamSpecError::MissingField("filter_subject"));
        }
        Ok(())
    }

    /// Encode this spec to JSON bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails.
    pub fn to_json_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    /// Decode a spec from JSON bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if deserialization fails.
    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}

/// Errors from JetStream spec operations.
#[derive(Debug, thiserror::Error)]
pub enum JetStreamSpecError {
    #[error("missing required field: {0}")]
    MissingField(&'static str),
}
