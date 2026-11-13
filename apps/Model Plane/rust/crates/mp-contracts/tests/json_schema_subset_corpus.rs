//! The JSON Schema subset validator in its own home (W5-071-a): the shared corpus that
//! pins it in model-gateway (`schema_check`, which re-exports this module) passes here
//! too, so a change in mp-contracts cannot drift from the corpus unseen.

use mp_contracts::json_schema_subset::{is_valid, issues, validate, MAX_ISSUES};
use serde_json::{json, Value};

const CORPUS: &str =
    include_str!("../../../services/model-gateway/tests/fixtures/ai-extract/schema-corpus.json");

#[test]
fn every_corpus_case_gives_the_same_issues_here() {
    let corpus: Value = serde_json::from_str(CORPUS).expect("the corpus is JSON");
    let cases = corpus["cases"].as_array().expect("cases");
    assert!(cases.len() >= 30, "the corpus lost cases");
    for case in cases {
        let name = case["name"].as_str().expect("case name");
        let mut expected: Vec<(String, String)> = case["issues"]
            .as_array()
            .expect("issues")
            .iter()
            .map(|issue| {
                (
                    issue["path"].as_str().expect("path").to_owned(),
                    issue["code"].as_str().expect("code").to_owned(),
                )
            })
            .collect();
        expected.sort();
        let mut got: Vec<(String, String)> = issues(&case["value"], &case["schema"])
            .into_iter()
            .map(|issue| (issue.path, issue.code.to_owned()))
            .collect();
        got.sort();
        assert_eq!(got, expected, "corpus case {name}");
        assert_eq!(
            is_valid(&case["value"], &case["schema"]),
            expected.is_empty(),
            "{name}"
        );
    }
}

#[test]
fn validate_is_uncapped_and_issues_are_capped() {
    let schema = json!({"type": "array", "items": {"type": "string"}});
    let value = Value::Array((0..50).map(|n| json!(n)).collect());
    assert_eq!(validate(&value, &schema).len(), 50);
    assert_eq!(issues(&value, &schema).len(), MAX_ISSUES);
}

#[test]
fn an_issue_never_carries_a_value_or_an_invented_key() {
    let schema = json!({
        "type": "object",
        "properties": {"a": {"enum": ["x"]}},
        "additionalProperties": false
    });
    let value = json!({"a": "SECRET-VALUE", "SECRET-KEY": 1});
    let shown = format!(
        "{:?} {:?}",
        issues(&value, &schema),
        validate(&value, &schema)
    );
    assert!(!shown.contains("SECRET"), "{shown}");
}
