//! Wire parity for `RunAgentResponse`'s typed failure (fields 6, 7 and 8).
//!
//! execution-core (Rust) writes them and model-gateway (Rust) reads them, but
//! the Go services decode the same message, and a field renumbered or retyped on
//! one side would not fail anything: the reader would see an unset field and
//! the gateway would fall back to the generic "the governed agent run failed",
//! hiding a usage limit's reset time. The same bytes are pinned in
//! `go/gen/model_plane/v1/proto_wire_parity_execution_test.go`.
//!
//! `retry_after_ms` is `optional`: a wait the provider did not give is absent
//! on the wire, and a wait of zero is present and zero. The two are different
//! facts and the second pair of cases pins it.

use mp_contracts::model_plane::v1::RunAgentResponse;
use prost::Message;

/// A usage limit with its timing, after two rounds.
const USAGE_LIMIT_WIRE_HEX: &str = "0a066661696c65641215546869732072756e207761732073746f707065642e18023218737562736372697074696f6e5f75736167655f6c696d697438a0d9f5184214323032362d30392d32365430313a32313a30305a";
/// A throttle whose reported wait is zero: the field is present.
const ZERO_WAIT_WIRE_HEX: &str = "0a066661696c65641215546869732072756e207761732073746f707065642e1802320c726174655f6c696d697465643800";
/// A refusal with no timing at all: fields 7 and 8 are absent.
const NO_TIMING_WIRE_HEX: &str = "0a066661696c65641215546869732072756e207761732073746f707065642e1802321c737562736372697074696f6e5f7265617574685f7265717569726564";

fn failed(failure_code: &str, retry_after_ms: Option<u64>, resets_at: &str) -> RunAgentResponse {
    RunAgentResponse {
        status: "failed".to_owned(),
        final_output: "This run was stopped.".to_owned(),
        rounds_executed: 2,
        failure_code: failure_code.to_owned(),
        retry_after_ms,
        resets_at: resets_at.to_owned(),
        ..Default::default()
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, byte| {
        write!(out, "{byte:02x}").expect("writing to a String cannot fail");
        out
    })
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).expect("hex"))
        .collect()
}

#[test]
fn a_typed_failure_encodes_to_the_golden_bytes() {
    assert_eq!(
        hex(&failed(
            "subscription_usage_limit",
            Some(52_260_000),
            "2026-09-26T01:21:00Z"
        )
        .encode_to_vec()),
        USAGE_LIMIT_WIRE_HEX
    );
    assert_eq!(
        hex(&failed("rate_limited", Some(0), "").encode_to_vec()),
        ZERO_WAIT_WIRE_HEX,
        "a reported wait of zero is on the wire"
    );
    assert_eq!(
        hex(&failed("subscription_reauth_required", None, "").encode_to_vec()),
        NO_TIMING_WIRE_HEX,
        "an unreported wait is not"
    );
}

#[test]
fn the_golden_bytes_decode_to_the_typed_failure() {
    let usage = RunAgentResponse::decode(unhex(USAGE_LIMIT_WIRE_HEX).as_slice()).expect("decode");
    assert_eq!(usage.status, "failed");
    assert_eq!(usage.rounds_executed, 2);
    assert_eq!(usage.failure_code, "subscription_usage_limit");
    assert_eq!(usage.retry_after_ms, Some(52_260_000));
    assert_eq!(usage.resets_at, "2026-09-26T01:21:00Z");

    let zero = RunAgentResponse::decode(unhex(ZERO_WAIT_WIRE_HEX).as_slice()).expect("decode");
    assert_eq!(zero.retry_after_ms, Some(0), "zero is a wait, not an absence");

    let bare = RunAgentResponse::decode(unhex(NO_TIMING_WIRE_HEX).as_slice()).expect("decode");
    assert_eq!(bare.failure_code, "subscription_reauth_required");
    assert_eq!(bare.retry_after_ms, None);
    assert_eq!(bare.resets_at, "");
}

/// A response written before the fields existed decodes with the failure unset,
/// so an older execution-core is never read as having failed for a reason.
#[test]
fn a_response_without_the_fields_has_no_typed_failure() {
    let before = RunAgentResponse {
        status: "failed".to_owned(),
        final_output: "I wasn't able to complete that request right now.".to_owned(),
        rounds_executed: 1,
        ..Default::default()
    };
    let decoded = RunAgentResponse::decode(before.encode_to_vec().as_slice()).expect("decode");
    assert_eq!(decoded.failure_code, "");
    assert_eq!(decoded.retry_after_ms, None);
    assert_eq!(decoded.resets_at, "");
}
