//! A run's durable, content-free timing record (EP-14 S5, decision D11).
//!
//! An Ask turn keeps its `turn_timing` report and a Do run keeps its per-round
//! inference intervals, each as one `RUN_TIMING_RECORDED` event of the run. The
//! record holds ids, milliseconds, counts and offered tool names: never prompt
//! text, message content, tool arguments or tool results. A ZDR or waived run
//! keeps none (decision D13).
//!
//! model-gateway writes the record and session-core stores and serves it; both
//! read the schema names, the cap and the check from here so they cannot drift.

use serde_json::{json, Value};

use crate::model_plane::v1::{RoundTiming, RunTimingRecorded};

/// An Ask turn's `turn_timing` report, exactly as the SSE frame carries it.
/// It includes the optional `failed_attempts_ms` and `retry_wait_ms` of a
/// provider summary (WS15-S4); a later additive field is a new minor schema.
pub const SCHEMA_TURN: &str = "turn-timing-v1";

/// A Do run's `RunAgentResponse.round_timing` as `{"rounds":[...]}`.
pub const SCHEMA_ROUND: &str = "round-timing-v1";

/// The largest report one record may hold. A turn clock keeps at most 64 spans
/// of a few hundred bytes each, so a real report is far below it.
pub const MAX_REPORT_BYTES: usize = 64 * 1024;

/// The longest run id or request id a record may name.
const MAX_ID_LEN: usize = 64;

/// Why a record was refused. The text is safe to return: it never repeats the
/// record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rejected(pub &'static str);

/// An id as the run store keeps it: one `[A-Za-z0-9_-]` token of at most 64.
fn valid_id(id: &str) -> bool {
    (1..=MAX_ID_LEN).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

/// Check a record before it is stored, and return its report as JSON.
///
/// # Errors
///
/// [`Rejected`] for a missing or malformed id, an unknown schema, a report over
/// [`MAX_REPORT_BYTES`] or a report that is not a JSON object.
pub fn validate(timing: &RunTimingRecorded) -> Result<Value, Rejected> {
    if !valid_id(&timing.run_id) {
        return Err(Rejected("run_id must be 1 to 64 characters of [A-Za-z0-9_-]"));
    }
    if !timing.request_id.is_empty() && !valid_id(&timing.request_id) {
        return Err(Rejected("request_id must be at most 64 characters of [A-Za-z0-9_-]"));
    }
    if timing.schema != SCHEMA_TURN && timing.schema != SCHEMA_ROUND {
        return Err(Rejected("schema must be turn-timing-v1 or round-timing-v1"));
    }
    if timing.report_json.len() > MAX_REPORT_BYTES {
        return Err(Rejected("report_json exceeds its cap"));
    }
    let report: Value = serde_json::from_str(&timing.report_json)
        .map_err(|_| Rejected("report_json must be valid JSON"))?;
    if !report.is_object() {
        return Err(Rejected("report_json must be a JSON object"));
    }
    Ok(report)
}

/// The `round-timing-v1` report of a Do run's rounds.
#[must_use]
pub fn round_report(rounds: &[RoundTiming]) -> Value {
    let rounds: Vec<Value> = rounds
        .iter()
        .map(|round| {
            json!({
                "round": round.round,
                "inference_start_ms": round.inference_start_ms,
                "inference_end_ms": round.inference_end_ms,
                "dispatch_end_ms": round.dispatch_end_ms,
                "tool_count": round.tool_count,
            })
        })
        .collect();
    json!({ "rounds": rounds })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_plane::v1::{GetRunTimingResponse, RunActionTiming};
    use prost::Message;

    fn record(schema: &str, report_json: &str) -> RunTimingRecorded {
        RunTimingRecorded {
            run_id: "01RUN".to_owned(),
            request_id: "01REQ".to_owned(),
            schema: schema.to_owned(),
            report_json: report_json.to_owned(),
            phases_dropped: 0,
            recorded_at: None,
        }
    }

    #[test]
    fn run_ledger_proto_roundtrip() {
        let response = GetRunTimingResponse {
            timings: vec![RunTimingRecorded {
                phases_dropped: 3,
                recorded_at: Some(prost_types::Timestamp {
                    seconds: 1_800_000_000,
                    nanos: 5,
                }),
                ..record(SCHEMA_TURN, r#"{"server_ms":12}"#)
            }],
            actions: vec![RunActionTiming {
                action_id: "act_1".to_owned(),
                tool: "web_search".to_owned(),
                status: "completed".to_owned(),
                reserved_at: Some(prost_types::Timestamp {
                    seconds: 1_800_000_001,
                    nanos: 0,
                }),
                finalized_at: None,
                effect_class: "read".to_owned(),
            }],
            truncated: true,
        };
        let decoded = GetRunTimingResponse::decode(response.encode_to_vec().as_slice())
            .expect("the response decodes");
        assert_eq!(decoded, response);
        // An empty response is zero bytes: a reader that predates the fields sees nothing new.
        assert!(GetRunTimingResponse::default().encode_to_vec().is_empty());
    }

    #[test]
    fn both_schemas_with_an_object_report_are_accepted() {
        for schema in [SCHEMA_TURN, SCHEMA_ROUND] {
            let report = validate(&record(schema, r#"{"a":1}"#)).expect("valid");
            assert_eq!(report, json!({"a": 1}));
        }
    }

    #[test]
    fn a_malformed_record_is_rejected_without_repeating_it() {
        let cases = [
            record("turn-timing-v2", "{}"),
            record(SCHEMA_TURN, "[1,2]"),
            record(SCHEMA_TURN, "not json"),
            RunTimingRecorded {
                run_id: String::new(),
                ..record(SCHEMA_TURN, "{}")
            },
            RunTimingRecorded {
                run_id: "run/../x".to_owned(),
                ..record(SCHEMA_TURN, "{}")
            },
            RunTimingRecorded {
                request_id: "a b".to_owned(),
                ..record(SCHEMA_TURN, "{}")
            },
            record(SCHEMA_TURN, &format!(r#"{{"pad":"{}"}}"#, "x".repeat(MAX_REPORT_BYTES))),
        ];
        for case in cases {
            let Rejected(reason) = validate(&case).unwrap_err();
            assert!(!reason.contains("x".repeat(16).as_str()));
        }
    }

    #[test]
    fn the_round_report_lists_every_round_in_order() {
        let rounds = [
            RoundTiming {
                round: 1,
                inference_start_ms: 5,
                inference_end_ms: 900,
                dispatch_end_ms: 1_400,
                tool_count: 2,
            },
            RoundTiming {
                round: 2,
                inference_start_ms: 1_400,
                inference_end_ms: 2_000,
                dispatch_end_ms: 2_000,
                tool_count: 0,
            },
        ];
        let report = round_report(&rounds);
        assert_eq!(report["rounds"].as_array().map(Vec::len), Some(2));
        assert_eq!(report["rounds"][0]["tool_count"], 2);
        assert_eq!(report["rounds"][1]["dispatch_end_ms"], 2_000);
        assert_eq!(round_report(&[]), json!({ "rounds": [] }));
    }
}
