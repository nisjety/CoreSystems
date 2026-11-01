//! The run-trace envelope golden (`api/contracts/run-trace-envelope.golden.json`).
//!
//! This crate holds a byte-identical vendored copy
//! (`tests/fixtures/run-trace-envelope.golden.json`, kept in step by
//! `scripts/tests/zdr-contract-sync.test.sh`). Quarry's mapper, session-core's
//! validator and the Go twin are all tested against the same bytes.

use mp_events::envelope::{Envelope, RUN_ORIGINS};

const GOLDEN: &[u8] = include_bytes!("fixtures/run-trace-envelope.golden.json");
const GOLDEN_PIN: &str = include_str!("fixtures/run-trace-envelope.golden.json.sha256");

#[test]
fn run_trace_golden_decodes() {
    let env = Envelope::from_json_bytes(GOLDEN).expect("decode golden");
    env.validate().expect("golden validates");

    assert_eq!(env.event_type, "BROWSER_STEP_RECEIPT_RECORDED");
    assert_eq!(env.producer, "quarry-edge");
    assert!(!env.zdr);
    assert_eq!(env.producer_seq, Some(7));
    let agent = env.agent.as_ref().expect("agent");
    assert_eq!(agent.id, "general-v1");
    assert_eq!(agent.role, "subagent");
    assert_eq!(agent.parent_id, "primary-v1");
    assert!(RUN_ORIGINS.contains(&env.origin.as_deref().expect("origin")));
    assert_eq!(env.run_id.as_deref(), Some("01JC7K3M9Q4R8T2V5W6X7Y8Z9C"));
    assert!(env.request_id.is_some());
    assert_eq!(env.resource_ref, "run/01JC7K3M9Q4R8T2V5W6X7Y8Z9C");

    // All 18 fields survive a re-encode (nothing is dropped by the twin).
    let back: serde_json::Value =
        serde_json::from_slice(&env.to_json_bytes().expect("encode")).expect("json");
    assert_eq!(back.as_object().expect("object").len(), 18);
    let golden: serde_json::Value = serde_json::from_slice(GOLDEN).expect("golden json");
    assert_eq!(back, golden, "the Rust twin re-encodes the golden losslessly");
}

#[test]
fn run_trace_golden_writes_zdr_explicitly() {
    // The key must be present in the bytes: an absent key decodes to false, so a
    // fail-closed consumer needs the producer to say it.
    let golden: serde_json::Value = serde_json::from_slice(GOLDEN).expect("golden json");
    assert_eq!(golden.get("zdr"), Some(&serde_json::Value::Bool(false)));
}

#[test]
fn run_trace_golden_matches_its_pin_prefix() {
    // The full digest is enforced by scripts/tests/zdr-contract-sync.test.sh; this
    // keeps the pin file present and well formed inside this crate's tree.
    let digest = GOLDEN_PIN.split_whitespace().next().expect("pin digest");
    assert_eq!(digest.len(), 64);
    assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn trace_event_type_names_match_the_proto_enum() {
    use mp_contracts::model_plane::v1::EventType;

    // The JSON `event_type` is the proto enum name without `EVENT_TYPE_`.
    let expected: [(&str, i32); 8] = [
        ("BROWSER_SESSION_OPENED", 150),
        ("BROWSER_SESSION_CLOSED", 151),
        ("BROWSER_CONTROL_CHANGED", 152),
        ("BROWSER_TAB_CHANGED", 153),
        ("BROWSER_STEP_RECEIPT_RECORDED", 154),
        ("BROWSER_EGRESS_DECIDED", 155),
        ("BROWSER_ARTIFACT_RECORDED", 156),
        ("BROWSER_ATTACH_SHARED_JAR", 160),
    ];
    for (name, number) in expected {
        let ty = EventType::from_str_name(&format!("EVENT_TYPE_{name}"))
            .unwrap_or_else(|| panic!("{name} is not in the proto EventType enum"));
        assert_eq!(ty as i32, number, "{name}");
    }

    // The golden's own event type is one of them.
    let env = Envelope::from_json_bytes(GOLDEN).expect("decode golden");
    assert!(EventType::from_str_name(&format!("EVENT_TYPE_{}", env.event_type)).is_some());
}
