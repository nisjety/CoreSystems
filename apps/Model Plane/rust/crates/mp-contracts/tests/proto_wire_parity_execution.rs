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

// ---- RunAgentResponse.park_reason (field 11, build plan WS03 S8) ----

/// A run that parked after two rounds: status "parked", its earliest resume time in
/// `resets_at` (field 8) and the closed reason in field 11. No failure code, no final
/// output. Pinned so the field number cannot drift unseen between the Rust writer and the
/// Go readers; the same bytes are pinned in the Go test of the same name.
const PARKED_WIRE_HEX: &str = "0a067061726b656418024214323032362d30392d32365430313a32313a30305a5a1270726f76696465725f657868617573746564";

fn parked(park_reason: &str) -> RunAgentResponse {
    RunAgentResponse {
        status: "parked".to_owned(),
        rounds_executed: 2,
        resets_at: "2026-09-26T01:21:00Z".to_owned(),
        park_reason: park_reason.to_owned(),
        ..Default::default()
    }
}

#[test]
fn a_parked_response_encodes_park_reason_as_field_11() {
    assert_eq!(
        hex(&parked("provider_exhausted").encode_to_vec()),
        PARKED_WIRE_HEX
    );
    // Field 11, wire type 2: key byte 0x5a. The reason is the last field written.
    let wire = unhex(PARKED_WIRE_HEX);
    assert!(
        wire.windows(2).any(|pair| pair == [0x5a, 0x12]),
        "park_reason (field 11) is a length-delimited string"
    );
}

#[test]
fn the_parked_golden_bytes_decode_and_a_response_without_the_field_is_not_parked() {
    let decoded = RunAgentResponse::decode(unhex(PARKED_WIRE_HEX).as_slice()).expect("decode");
    assert_eq!(decoded.status, "parked");
    assert_eq!(decoded.park_reason, "provider_exhausted");
    assert_eq!(decoded.resets_at, "2026-09-26T01:21:00Z");
    assert_eq!(decoded.failure_code, "");
    assert_eq!(decoded.final_output, "");
    // A response written before the field existed has no park reason, so an older
    // execution-core is never read as having parked.
    let before = RunAgentResponse::decode(
        failed("subscription_usage_limit", None, "").encode_to_vec().as_slice(),
    )
    .expect("decode");
    assert_eq!(before.park_reason, "");
    // An unset reason is not on the wire at all: the key byte and the length byte are
    // two bytes, and the reason's eighteen.
    assert_eq!(
        parked("").encode_to_vec().len() + 2 + "provider_exhausted".len(),
        unhex(PARKED_WIRE_HEX).len()
    );
}

// ---- SaveLoopCheckpointResponse.park_accepted (field 2, build plan WS03 S8) ----

/// The store's answer to a park save: seq 7 (field 1) and park_accepted true (field 2).
/// execution-core reads field 2 to know whether the park was durably accepted; a store
/// that predates the field sends only field 1, and the executor must read that as false.
#[test]
fn a_save_response_carries_park_accepted_as_field_2() {
    use mp_contracts::model_plane::v1::SaveLoopCheckpointResponse;
    let accepted = SaveLoopCheckpointResponse {
        seq: 7,
        park_accepted: true,
    };
    assert_eq!(hex(&accepted.encode_to_vec()), "08071001");
    let old_store = SaveLoopCheckpointResponse::decode(unhex("0807").as_slice()).expect("decode");
    assert_eq!(old_store.seq, 7);
    assert!(!old_store.park_accepted, "an absent field reads as not accepted");
    // A save that was not a park accepted nothing: false is not on the wire.
    let plain = SaveLoopCheckpointResponse {
        seq: 7,
        park_accepted: false,
    };
    assert_eq!(hex(&plain.encode_to_vec()), "0807");
}

// ---- RunAgentRequest.run_context (field 18, build plan WS02 S4) ----

/// A run that carries instructions, a memory block and a status. Pinned so the
/// field number and the nested numbers cannot drift unseen between the Rust
/// writer and the Go readers.
const RUN_CONTEXT_WIRE_HEX: &str = "0a0272319201160a0942652062726965662e1a016d22060a0408011001";

fn run_with_context() -> mp_contracts::model_plane::v1::RunAgentRequest {
    use mp_contracts::model_plane::v1::{
        RunAgentRequest, RunContext, RunContextPart, RunContextState, RunContextStatus,
    };
    RunAgentRequest {
        run_id: "r1".to_owned(),
        run_context: Some(RunContext {
            authored_instructions: "Be brief.".to_owned(),
            memory_block: "m".to_owned(),
            status: Some(RunContextStatus {
                instructions: Some(RunContextPart {
                    state: RunContextState::Included as i32,
                    included: 1,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn a_run_context_encodes_to_the_golden_bytes() {
    assert_eq!(hex(&run_with_context().encode_to_vec()), RUN_CONTEXT_WIRE_HEX);
}

#[test]
fn the_run_context_golden_bytes_decode_and_an_absent_context_stays_absent() {
    use mp_contracts::model_plane::v1::RunAgentRequest;
    let decoded = RunAgentRequest::decode(unhex(RUN_CONTEXT_WIRE_HEX).as_slice()).expect("decode");
    let context = decoded.run_context.expect("field 18 present");
    assert_eq!(context.authored_instructions, "Be brief.");
    assert_eq!(context.memory_block, "m");
    assert_eq!(context.status.expect("status").instructions.expect("part").included, 1);
    // A request written before the field existed has no context (today's behaviour).
    let before = RunAgentRequest { run_id: "r1".to_owned(), ..Default::default() };
    let decoded = RunAgentRequest::decode(before.encode_to_vec().as_slice()).expect("decode");
    assert!(decoded.run_context.is_none());
}
