//! The model entitlement refusal catalogue (WS10-S1): the golden is what the
//! code produces, every code has one entry, a refusal is content-free, and the
//! refusal rows of the entitlement matrix are what the real builders write.

use mp_contracts::model_entitlement::{
    catalogue_json, catalogue_text, code, entry, Reason, Refusal, ENTRIES,
};
use serde_json::Value;

const FIXTURE: &str = include_str!("../../../../api/contracts/model-entitlement-refusals.json");
const FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../api/contracts/model-entitlement-refusals.json"
);
const MATRIX: &str = include_str!("../../../../api/contracts/model-entitlement-matrix.json");

fn matrix() -> Value {
    serde_json::from_str(MATRIX).expect("the entitlement matrix is JSON")
}

fn rows_of_kind(matrix: &Value, kind: &str) -> Vec<Value> {
    matrix["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .filter(|row| row["kind"] == kind)
        .cloned()
        .collect()
}

fn sorted_keys(value: &Value) -> Vec<String> {
    let mut keys: Vec<String> = value
        .as_object()
        .expect("an object")
        .keys()
        .cloned()
        .collect();
    keys.sort();
    keys
}

#[test]
fn the_catalogue_has_exactly_one_entry_per_code() {
    assert_eq!(ENTRIES.len(), code::ALL.len());
    for code in code::ALL {
        assert!(entry(code).is_some(), "{code} has no catalogue entry");
    }
    let mut seen = std::collections::BTreeSet::new();
    for entry in ENTRIES {
        assert!(seen.insert(entry.code), "{} is listed twice", entry.code);
    }
}

#[test]
fn every_sentence_is_plain_english_that_states_what_happened() {
    let mut sentences: Vec<(&str, &str)> = ENTRIES.iter().map(|e| (e.code, e.message)).collect();
    sentences.extend(Reason::ALL.iter().map(|r| (r.as_str(), r.message())));
    for (name, sentence) in sentences {
        assert!(!sentence.trim().is_empty(), "{name} has no text");
        assert!(!sentence.contains("  "), "{name} has a double space");
        assert!(sentence.ends_with('.'), "{name} ends without a full stop");
        assert!(
            sentence.contains("not sent to a model"),
            "{name} must say nothing was sent to a model"
        );
    }
}

#[test]
fn only_the_retryable_code_offers_a_retry() {
    for entry in ENTRIES {
        let retry = entry.action.as_str() == "retry";
        assert_eq!(entry.retryable, retry, "{}", entry.code);
        assert_eq!(
            entry.retryable,
            entry.code == code::ENTITLEMENT_UNAVAILABLE,
            "{}",
            entry.code
        );
    }
}

#[test]
fn a_refusal_names_a_model_and_a_reason_and_nothing_else() {
    for reason in Reason::ALL {
        let body = Refusal::not_entitled("gpt-5", reason).json_body();
        let error = &body["error"];
        assert_eq!(
            sorted_keys(error),
            ["action", "code", "details", "message", "retryable"],
            "{reason:?}"
        );
        assert_eq!(
            sorted_keys(&error["details"]),
            ["model", "reason"],
            "{reason:?}"
        );
    }
    let body = Refusal::unavailable().json_body();
    assert_eq!(
        sorted_keys(&body["error"]),
        ["action", "code", "message", "retryable"]
    );
}

#[test]
fn the_committed_fixture_is_what_the_catalogue_produces() {
    let generated = catalogue_text();
    if std::env::var("MP_UPDATE_GOLDEN").is_ok_and(|value| value == "1") {
        std::fs::write(FIXTURE_PATH, &generated).expect("write the refusal fixture");
        return;
    }
    assert!(
        !FIXTURE.contains('\r'),
        "the refusal fixture keeps LF line endings"
    );
    assert_eq!(
        FIXTURE, generated,
        "model-entitlement-refusals.json differs from the catalogue. If the change is intended, \
         regenerate it with MP_UPDATE_GOLDEN=1 cargo test -p mp-contracts --test \
         model_entitlement_catalogue, then update the .sha256 beside it and the pin in \
         scripts/tests/model-entitlement-contract-sync.test.sh."
    );
    let parsed: Value = serde_json::from_str(FIXTURE).expect("the fixture is JSON");
    assert_eq!(parsed, catalogue_json());
}

#[test]
fn the_reasons_are_the_matrix_vocabulary() {
    let matrix = matrix();
    let mut matrix_reasons: Vec<&str> = matrix["vocabulary"]["reasons"]
        .as_array()
        .expect("vocabulary.reasons")
        .iter()
        .map(|reason| reason.as_str().expect("a reason name"))
        .collect();
    let mut ours: Vec<&str> = Reason::ALL.iter().map(|reason| reason.as_str()).collect();
    matrix_reasons.sort_unstable();
    ours.sort_unstable();
    assert_eq!(ours, matrix_reasons);
}

#[test]
fn the_refusal_rows_of_the_matrix_are_what_the_builders_write() {
    let matrix = matrix();
    let rows = rows_of_kind(&matrix, "refusal");
    assert!(!rows.is_empty(), "the matrix has no refusal row to check");
    for row in rows {
        let reason = Reason::parse(row["input"]["reason"].as_str().expect("reason"))
            .expect("the row names a reason of the vocabulary");
        let refusal = Refusal::not_entitled(row["input"]["model"].as_str().expect("model"), reason);
        let expect = &row["expect"];
        let body = refusal.json_body();
        assert_eq!(
            u64::from(refusal.http_status()),
            expect["http_status"].as_u64().unwrap()
        );
        assert_eq!(body["error"]["code"], expect["code"]);
        assert_eq!(
            Value::from(sorted_keys(&body["error"])),
            expect["body_keys"],
            "{}",
            row["id"]
        );
        assert_eq!(
            Value::from(sorted_keys(&body["error"]["details"])),
            expect["details_keys"],
            "{}",
            row["id"]
        );
    }
}

#[test]
fn the_refusing_availability_rows_of_the_matrix_are_the_unavailable_entry() {
    let matrix = matrix();
    let mut refusing = 0;
    for row in rows_of_kind(&matrix, "availability") {
        let expect = &row["expect"];
        if expect["decision"] != "refuse" {
            continue;
        }
        refusing += 1;
        let refusal = Refusal::unavailable();
        assert_eq!(expect["code"], refusal.code(), "{}", row["id"]);
        assert_eq!(
            expect["http_status"].as_u64().unwrap(),
            u64::from(refusal.http_status()),
            "{}",
            row["id"]
        );
        assert_eq!(expect["retryable"], refusal.retryable(), "{}", row["id"]);
    }
    assert!(refusing > 0, "the matrix has no refusing availability row");
}
