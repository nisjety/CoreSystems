//! The approval edit golden, run against `apply_edit` (EP-10-S3).
//!
//! `fixtures/approval-edit-cases.json` is owned here. The SPA holds a byte-identical copy at
//! `src/shared/approvals/contracts/approval-edit-cases.json` and its vitest pins the same digest,
//! so a change to a case or a limit moves both pins in one reviewed change.

use mp_contracts::approval_edit::{
    apply_edit, EditError, MailSendPatch, FIELD_BODY_TEXT, FIELD_SUBJECT, FIELD_TO,
    MAX_BODY_TEXT_BYTES, MAX_RECIPIENTS, MAX_RECIPIENT_BYTES, MAX_SUBJECT_BYTES, MIN_RECIPIENTS,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

const GOLDEN: &str = include_str!("fixtures/approval-edit-cases.json");
/// The digest the SPA's vitest pins for its copy. Re-pin both when the golden changes.
const GOLDEN_SHA256: &str = "e54250ebe399854c1464390f0ebfc49d2dde578f401edda61beadf116690c792";

fn golden() -> Value {
    serde_json::from_str(GOLDEN).expect("golden parses as JSON")
}

/// Replace every `{"$repeat": s, "count": n}` with `s` repeated `n` times, so the golden can hold
/// an oversized subject or body without carrying its bytes.
fn expand(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            if let (Some(Value::String(text)), Some(count), 2) =
                (map.get("$repeat"), map.get("count").and_then(Value::as_u64), map.len())
            {
                return Value::String(text.repeat(usize::try_from(count).expect("count fits")));
            }
            Value::Object(map.iter().map(|(k, v)| (k.clone(), expand(v))).collect())
        }
        Value::Array(items) => Value::Array(items.iter().map(expand).collect()),
        other => other.clone(),
    }
}

#[test]
fn approval_edit_golden_bytes_match_the_pinned_sha256() {
    let digest = format!("{:x}", Sha256::digest(GOLDEN.as_bytes()));
    assert_eq!(
        digest, GOLDEN_SHA256,
        "the golden changed: re-pin GOLDEN_SHA256 here and in the SPA together"
    );
}

#[test]
fn approval_edit_golden_limits_codes_and_fields_are_the_contract() {
    let golden = golden();
    assert_eq!(golden["contract"], "model-plane.approval-edit");
    assert_eq!(golden["version"], 1);
    let limits = &golden["limits"];
    assert_eq!(limits["min_recipients"], MIN_RECIPIENTS);
    assert_eq!(limits["max_recipients"], MAX_RECIPIENTS);
    assert_eq!(limits["max_subject_bytes"], MAX_SUBJECT_BYTES);
    assert_eq!(limits["max_body_text_bytes"], MAX_BODY_TEXT_BYTES);
    assert_eq!(limits["max_recipient_bytes"], MAX_RECIPIENT_BYTES);
    let codes: Vec<&str> = EditError::ALL.iter().map(|error| error.code()).collect();
    assert_eq!(golden["codes"], serde_json::json!(codes));
    assert_eq!(
        golden["edited_fields"],
        serde_json::json!([FIELD_TO, FIELD_SUBJECT, FIELD_BODY_TEXT])
    );
}

#[test]
fn approval_edit_golden_cases_hold() {
    let golden = golden();
    let cases = golden["cases"].as_array().expect("cases is an array");
    assert!(!cases.is_empty());
    let mut seen_codes = std::collections::BTreeSet::new();
    for case in cases {
        let name = case["name"].as_str().expect("case name");
        let patch: MailSendPatch =
            serde_json::from_value(expand(&case["patch"])).unwrap_or_else(|error| {
                panic!("{name}: patch does not parse: {error}");
            });
        let result = apply_edit(&expand(&case["input"]), &patch);
        match case.get("error").and_then(Value::as_str) {
            Some(code) => {
                let error = result.expect_err(name);
                assert_eq!(error.code(), code, "{name}");
                seen_codes.insert(code.to_owned());
            }
            None => {
                let edited = result.unwrap_or_else(|error| panic!("{name}: refused with {error}"));
                assert_eq!(edited.input, expand(&case["expect"]["input"]), "{name}");
                assert_eq!(
                    serde_json::json!(edited.edited_fields),
                    case["expect"]["edited_fields"],
                    "{name}"
                );
            }
        }
    }
    let every: std::collections::BTreeSet<String> = EditError::ALL
        .iter()
        .map(|error| error.code().to_owned())
        .collect();
    assert_eq!(seen_codes, every, "the golden covers every refusal code");
}
