//! Golden fixture contract tests for JetStream stream/consumer specs.
//!
//! Validates:
//! - decode -> encode round-trip preserves all fields
//! - `validate()` accepts well-formed specs
//! - missing-field fixtures are rejected with descriptive errors

use mp_events::jetstream::{
    AckPolicy, ConsumerSpec, DiscardPolicy, RetentionPolicy, StorageType, StreamSpec,
};

fn load_valid_stream() -> StreamSpec {
    StreamSpec::from_json_bytes(include_bytes!("fixtures/jetstream_stream_spec.json"))
        .expect("decode valid stream fixture")
}

fn load_valid_consumer() -> ConsumerSpec {
    ConsumerSpec::from_json_bytes(include_bytes!("fixtures/jetstream_consumer_spec.json"))
        .expect("decode valid consumer fixture")
}

#[test]
fn stream_golden_valid_roundtrip() {
    let spec = load_valid_stream();
    assert!(spec.validate().is_ok());
    assert_eq!(spec.name, "MODEL_PLANE_EVENTS");
    assert_eq!(spec.subjects, vec!["mp.events.>".to_string()]);
    assert_eq!(spec.retention, RetentionPolicy::Limits);
    assert_eq!(spec.storage, StorageType::File);
    assert_eq!(spec.max_age_secs, 604_800);
    assert_eq!(spec.replicas, 3);
    assert_eq!(spec.discard, DiscardPolicy::Old);

    let bytes = spec.to_json_bytes().expect("encode");
    let decoded = StreamSpec::from_json_bytes(&bytes).expect("decode round-trip");
    assert_eq!(decoded, spec);
}

#[test]
fn consumer_golden_valid_roundtrip() {
    let spec = load_valid_consumer();
    assert!(spec.validate().is_ok());
    assert_eq!(spec.durable, "model-gateway-worker");
    assert_eq!(spec.ack_policy, AckPolicy::Explicit);
    assert_eq!(spec.ack_wait_secs, 30);
    assert_eq!(spec.max_deliver, 5);
    assert_eq!(spec.filter_subject, "mp.events.run.>");

    let bytes = spec.to_json_bytes().expect("encode");
    let decoded = ConsumerSpec::from_json_bytes(&bytes).expect("decode round-trip");
    assert_eq!(decoded, spec);
}

#[test]
fn stream_missing_name_fails_validation() {
    let spec = StreamSpec::from_json_bytes(include_bytes!(
        "fixtures/jetstream_stream_missing_fields.json"
    ))
    .expect("decode");
    let err = spec.validate().unwrap_err().to_string();
    assert!(err.contains("name"), "expected name error, got: {err}");
}

#[test]
fn stream_missing_subjects_fails_validation() {
    let mut spec = load_valid_stream();
    spec.subjects.clear();
    let err = spec.validate().unwrap_err().to_string();
    assert!(
        err.contains("subjects"),
        "expected subjects error, got: {err}"
    );
}

#[test]
fn stream_empty_subject_fails_validation() {
    let mut spec = load_valid_stream();
    spec.subjects = vec!["".to_string()];
    let err = spec.validate().unwrap_err().to_string();
    assert!(
        err.contains("subjects"),
        "expected subjects error, got: {err}"
    );
}

#[test]
fn stream_zero_max_age_fails_validation() {
    let mut spec = load_valid_stream();
    spec.max_age_secs = 0;
    let err = spec.validate().unwrap_err().to_string();
    assert!(
        err.contains("max_age_secs"),
        "expected max_age_secs error, got: {err}"
    );
}

#[test]
fn stream_zero_replicas_fails_validation() {
    let mut spec = load_valid_stream();
    spec.replicas = 0;
    let err = spec.validate().unwrap_err().to_string();
    assert!(
        err.contains("replicas"),
        "expected replicas error, got: {err}"
    );
}

#[test]
fn consumer_missing_durable_fails_validation() {
    let spec = ConsumerSpec::from_json_bytes(include_bytes!(
        "fixtures/jetstream_consumer_missing_fields.json"
    ))
    .expect("decode");
    let err = spec.validate().unwrap_err().to_string();
    assert!(
        err.contains("durable"),
        "expected durable error, got: {err}"
    );
}

#[test]
fn consumer_zero_ack_wait_fails_validation() {
    let mut spec = load_valid_consumer();
    spec.ack_wait_secs = 0;
    let err = spec.validate().unwrap_err().to_string();
    assert!(
        err.contains("ack_wait_secs"),
        "expected ack_wait_secs error, got: {err}"
    );
}

#[test]
fn consumer_zero_max_deliver_fails_validation() {
    let mut spec = load_valid_consumer();
    spec.max_deliver = 0;
    let err = spec.validate().unwrap_err().to_string();
    assert!(
        err.contains("max_deliver"),
        "expected max_deliver error, got: {err}"
    );
}

#[test]
fn consumer_missing_filter_subject_fails_validation() {
    let mut spec = load_valid_consumer();
    spec.filter_subject.clear();
    let err = spec.validate().unwrap_err().to_string();
    assert!(
        err.contains("filter_subject"),
        "expected filter_subject error, got: {err}"
    );
}
