//! Golden fixture contract tests for the event envelope.
//!
//! Validates:
//! - decode -> encode round-trip preserves all fields
//! - `schema_version` is checked
//! - required-field failures are caught by `validate()`

use mp_events::envelope::Envelope;

#[test]
fn golden_valid_roundtrip() {
    let data = include_bytes!("fixtures/envelope_valid.json");
    let envelope = Envelope::from_json_bytes(data).expect("decode valid fixture");

    assert!(
        envelope.validate().is_ok(),
        "valid fixture should pass validation"
    );
    assert_eq!(envelope.event_type, "RUN_STARTED");
    assert_eq!(envelope.schema_version, 1);
    assert_eq!(envelope.producer, "model-gateway");

    // Round-trip
    let bytes = envelope.to_json_bytes().expect("encode");
    let decoded = Envelope::from_json_bytes(&bytes).expect("decode round-trip");
    assert_eq!(decoded.event_id, envelope.event_id);
    assert_eq!(decoded.event_type, envelope.event_type);
    assert_eq!(decoded.org_id, envelope.org_id);
    assert_eq!(decoded.payload, envelope.payload);
}

#[test]
fn golden_missing_event_id_fails_validation() {
    let data = include_bytes!("fixtures/envelope_missing_fields.json");
    let envelope = Envelope::from_json_bytes(data).expect("decode missing-fields fixture");

    let result = envelope.validate();
    assert!(result.is_err(), "missing event_id should fail validation");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("event_id"),
        "error should mention event_id"
    );
}

#[test]
fn schema_version_zero_fails() {
    let mut envelope =
        Envelope::from_json_bytes(include_bytes!("fixtures/envelope_valid.json")).expect("decode");

    envelope.schema_version = 0;
    let result = envelope.validate();
    assert!(result.is_err());
}

fn load_valid() -> Envelope {
    Envelope::from_json_bytes(include_bytes!("fixtures/envelope_valid.json"))
        .expect("decode valid fixture")
}

#[test]
fn missing_ts_fails_validation() {
    let mut env = load_valid();
    env.ts = chrono::DateTime::<chrono::Utc>::from_timestamp(0, 0).unwrap();
    let err = env.validate().unwrap_err().to_string();
    assert!(err.contains("ts"), "expected ts error, got: {err}");
}

#[test]
fn missing_correlation_id_fails_validation() {
    let mut env = load_valid();
    env.correlation_id.clear();
    let err = env.validate().unwrap_err().to_string();
    assert!(
        err.contains("correlation_id"),
        "expected correlation_id error, got: {err}"
    );
}

#[test]
fn missing_idempotency_key_fails_validation() {
    let mut env = load_valid();
    env.idempotency_key.clear();
    let err = env.validate().unwrap_err().to_string();
    assert!(
        err.contains("idempotency_key"),
        "expected idempotency_key error, got: {err}"
    );
}

#[test]
fn missing_user_id_fails_validation() {
    let mut env = load_valid();
    env.user_id.clear();
    let err = env.validate().unwrap_err().to_string();
    assert!(
        err.contains("user_id"),
        "expected user_id error, got: {err}"
    );
}

#[test]
fn missing_resource_ref_fails_validation() {
    let mut env = load_valid();
    env.resource_ref.clear();
    let err = env.validate().unwrap_err().to_string();
    assert!(
        err.contains("resource_ref"),
        "expected resource_ref error, got: {err}"
    );
}

#[test]
fn missing_event_type_fails_validation() {
    let mut env = load_valid();
    env.event_type.clear();
    let err = env.validate().unwrap_err().to_string();
    assert!(
        err.contains("event_type"),
        "expected event_type error, got: {err}"
    );
}

#[test]
fn missing_producer_fails_validation() {
    let mut env = load_valid();
    env.producer.clear();
    let err = env.validate().unwrap_err().to_string();
    assert!(
        err.contains("producer"),
        "expected producer error, got: {err}"
    );
}

#[test]
fn missing_org_id_fails_validation() {
    let mut env = load_valid();
    env.org_id.clear();
    let err = env.validate().unwrap_err().to_string();
    assert!(err.contains("org_id"), "expected org_id error, got: {err}");
}
