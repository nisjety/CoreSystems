//! The canonical event envelope mirroring the proto `Event` message.
//!
//! This is the Rust-native representation used for JSON encoding on NATS
//! and for the append-only events table in Postgres.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The agent that produced a trace event (`model_plane.v1.AgentRef`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentRef {
    /// Agent identifier, for example `general-v1`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,

    /// Role of the agent within the run, for example `primary`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub role: String,

    /// Parent agent identifier; empty for the primary agent.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub parent_id: String,
}

/// JSON names of `model_plane.v1.RunOrigin`: the enum name without the
/// `RUN_ORIGIN_` prefix. A label for display and filtering only, never an
/// authorization input.
pub const RUN_ORIGINS: [&str; 5] = ["USER", "AGENT", "SCHEDULE", "WEBHOOK", "API"];

/// Canonical event envelope.
///
/// The run-trace fields (`producer_seq`, `agent`, `origin`, `run_id`,
/// `request_id`; proto fields 21-25) are optional and omitted from the JSON
/// when absent, so every pre-trace envelope still decodes and re-encodes
/// byte-for-byte.
///
/// `zdr` is NOT optional on the wire: an absent key decodes to `false`, so a
/// producer of trace events must always write `"zdr": false` itself. A consumer
/// that must fail closed on an undeclared posture has to probe the raw JSON
/// (see session-core `trace_nats`), not this struct.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
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

    /// Per-`(run_id, producer)` sequence, dense from 1. `None` = not numbered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer_seq: Option<u64>,

    /// Agent that produced the event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentRef>,

    /// What started the run: one of [`RUN_ORIGINS`]. Never authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,

    /// Model Plane run id (bare ULID, no dots) the event belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,

    /// Request id of the call that caused the event. Not an auth, tenancy or
    /// idempotency key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::*;

    const PRE_TRACE_JSON: &str = r#"{"event_id":"01HXYZ01234567890ABCDEFGHI","event_type":"RUN_STARTED","schema_version":1,"ts":"2026-01-15T12:00:00Z","producer":"model-gateway","correlation_id":"corr-001","causation_id":"cause-001","idempotency_key":"req-001","org_id":"01HORG01234567890ABCDEFGHI","user_id":"01HUSR01234567890ABCDEFGHI","resource_ref":"run/01HRUN01234567890ABCDEFGHI","payload":{"goal":"test run"}}"#;

    fn trace_envelope() -> Envelope {
        let mut env = Envelope::from_json_bytes(PRE_TRACE_JSON.as_bytes()).expect("decode");
        env.event_type = "BROWSER_STEP_RECEIPT_RECORDED".to_string();
        env.producer = "quarry-edge".to_string();
        env.producer_seq = Some(7);
        env.agent = Some(AgentRef {
            id: "general-v1".to_string(),
            role: "primary".to_string(),
            parent_id: String::new(),
        });
        env.origin = Some("AGENT".to_string());
        env.run_id = Some("01HRUN01234567890ABCDEFGHI".to_string());
        env.request_id = Some("req_abc123".to_string());
        env
    }

    #[test]
    fn envelope_round_trips_trace_fields() {
        let env = trace_envelope();
        let bytes = env.to_json_bytes().expect("encode");
        let back = Envelope::from_json_bytes(&bytes).expect("decode");
        assert_eq!(back, env);
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(v["producer_seq"], 7);
        assert_eq!(v["origin"], "AGENT");
        assert_eq!(v["agent"]["id"], "general-v1");
        assert_eq!(v["agent"]["role"], "primary");
        assert!(v["agent"].get("parent_id").is_none(), "empty parent omitted");
        assert_eq!(v["run_id"], "01HRUN01234567890ABCDEFGHI");
        assert_eq!(v["request_id"], "req_abc123");
    }

    #[test]
    fn envelope_decodes_pre_trace_json() {
        let env = Envelope::from_json_bytes(PRE_TRACE_JSON.as_bytes()).expect("decode");
        assert!(env.validate().is_ok());
        assert!(!env.zdr, "an absent zdr key decodes to false");
        assert_eq!(env.producer_seq, None);
        assert_eq!(env.agent, None);
        assert_eq!(env.origin, None);
        assert_eq!(env.run_id, None);
        assert_eq!(env.request_id, None);
    }

    #[test]
    fn envelope_omits_none_trace_fields() {
        let env = Envelope::from_json_bytes(PRE_TRACE_JSON.as_bytes()).expect("decode");
        let v: serde_json::Value =
            serde_json::from_slice(&env.to_json_bytes().expect("encode")).expect("json");
        let obj = v.as_object().expect("object");
        for key in ["producer_seq", "agent", "origin", "run_id", "request_id"] {
            assert!(!obj.contains_key(key), "{key} must be omitted when None");
        }
        // zdr is always written, so a pre-trace envelope carries an explicit false.
        assert_eq!(obj.get("zdr"), Some(&serde_json::Value::Bool(false)));
    }

    #[test]
    fn run_origins_are_the_proto_enum_names_without_prefix() {
        assert_eq!(RUN_ORIGINS, ["USER", "AGENT", "SCHEDULE", "WEBHOOK", "API"]);
    }
}
