//! Golden cases of the approval cause vocabulary (build plan WS13 S1).
//!
//! `api/contracts/approval-causes.json` is the closed set written out as data. The
//! code that raises a cause, the store that keeps it and the card that renders it all
//! read the same enum, so a code added in one place and not the other fails here
//! instead of reaching a person as a blank reason.

use mp_contracts::approval_cause::{
    call_digest, for_call, from_metadata, normalized, scope_options_for, to_metadata, CauseCode,
};
use mp_contracts::model_plane::v1::ApprovalCause;
use serde_json::Value;

const GOLDEN: &str = include_str!("../../../../api/contracts/approval-causes.json");

fn golden() -> Value {
    serde_json::from_str(GOLDEN).expect("the golden is JSON")
}

fn golden_codes() -> Vec<String> {
    golden()["codes"]
        .as_array()
        .expect("codes is a list")
        .iter()
        .map(|row| row["code"].as_str().expect("a code is text").to_owned())
        .collect()
}

#[test]
fn approval_cause_vocabulary_matches_the_golden_in_order() {
    let enum_codes: Vec<&str> = CauseCode::ALL.iter().map(|code| code.as_str()).collect();
    assert_eq!(
        golden_codes(),
        enum_codes,
        "a code was added to the enum or the golden without the other"
    );
    assert_eq!(
        golden()["unknown_degrades_to"],
        CauseCode::UnclassifiedGate.as_str()
    );
}

#[test]
fn approval_cause_scope_options_match_the_golden_for_every_code() {
    for row in golden()["codes"].as_array().expect("codes is a list") {
        let code = CauseCode::from_wire(row["code"].as_str().expect("code"));
        let expected: Vec<&str> = row["scope_options"]
            .as_array()
            .expect("scope_options is a list")
            .iter()
            .map(|scope| scope.as_str().expect("a scope is text"))
            .collect();
        assert_eq!(scope_options_for(code), expected.as_slice(), "{row}");
    }
}

#[test]
fn approval_cause_digest_known_answer_pins_the_algorithm() {
    let answer = &golden()["digest"]["known_answer"];
    assert_eq!(
        call_digest(
            answer["run_id"].as_str().expect("run_id"),
            answer["tool"].as_str().expect("tool"),
            answer["arguments"].as_str().expect("arguments"),
        ),
        answer["digest"].as_str().expect("digest"),
        "the digest changed: every stored call_digest would stop matching"
    );
}

#[test]
fn approval_cause_round_trips_and_unknown_codes_degrade() {
    // Every known code survives the stored shape unchanged.
    for code in CauseCode::ALL {
        let cause = for_call(code, "run_1", "delete_records", r#"{"id":"r-1"}"#);
        let stored = serde_json::json!({ "cause": to_metadata(&cause) });
        assert_eq!(from_metadata(&stored), Some(cause), "{code:?}");
    }

    // A creator newer than this reader degrades, and cannot widen the scopes it offers.
    let (degraded, known) = normalized(ApprovalCause {
        code: "tainted_egress_retired".to_owned(),
        scope_options: vec!["thread".to_owned(), "run".to_owned()],
        ..ApprovalCause::default()
    });
    assert!(!known);
    assert_eq!(degraded.code, "unclassified_gate");
    assert_eq!(degraded.scope_options, vec!["once".to_owned()]);

    // A stored row with a code this reader does not know reads as unclassified too.
    let stored =
        serde_json::json!({ "cause": { "code": "from_the_future", "scope_options": ["run"] } });
    let read = from_metadata(&stored).expect("a stored cause");
    assert_eq!(read.code, "unclassified_gate");
    assert_eq!(read.scope_options, vec!["once".to_owned()]);
}

#[test]
fn approval_cause_never_carries_argument_text() {
    let secret = "alice@example.com-body-text";
    let arguments = format!(
        r#"{{"connection_id":"conn_private_77","operation":"message.send","body":{{"text":"{secret}"}}}}"#
    );
    let cause = for_call(
        CauseCode::ProviderWrite,
        "run_1",
        "execute_provider_action",
        &arguments,
    );
    let stored = to_metadata(&cause).to_string();
    assert!(!stored.contains(secret), "{stored}");
    assert!(!stored.contains("conn_private_77"), "{stored}");
    assert_eq!(cause.action_key, "execute_provider_action:message.send");
}
