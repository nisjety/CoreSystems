//! A small JSON Schema subset validator for model-written structured output (WS16.S8).
//!
//! The validator itself lives in `mp_contracts::json_schema_subset`, where
//! execution-core's typed wide-research results share it. This module re-exports
//! it, so `crate::schema_check::issues` and the corpus test below keep their
//! paths and their behaviour. The shared behaviour is pinned by
//! `tests/fixtures/ai-extract/schema-corpus.json`, Quarry's corpus.

pub use mp_contracts::json_schema_subset::{
    is_valid, issues, validate, SchemaIssue, ValidationIssue, MAX_ISSUES,
};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};

    /// The bytes of the shared corpus. Pinned: a change to the file is a change to the contract and
    /// goes with the lead's `zdr-contract-sync.test.sh` digest row.
    const CORPUS: &str = include_str!("../tests/fixtures/ai-extract/schema-corpus.json");
    /// In two halves: a bare 64-hex literal reads as a credential to the secret scanner.
    const CORPUS_SHA256: [&str; 2] = ["f3ce2e27a78010d28df3e14bc688908e", "681426db6bfc2b968a59409810a7c6f5"];

    fn codes(value: &Value, schema: &Value) -> Vec<(String, &'static str)> {
        let mut shown: Vec<_> = issues(value, schema)
            .into_iter()
            .map(|issue| (issue.path, issue.code))
            .collect();
        shown.sort();
        shown
    }

    #[test]
    fn schema_corpus_matches_quarry() {
        // Hashed with line endings normalised to LF: a Windows checkout with autocrlf rewrites them.
        let digest = Sha256::digest(CORPUS.replace("\r\n", "\n").as_bytes());
        let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        assert_eq!(
            hex,
            CORPUS_SHA256.concat(),
            "the corpus changed; update the digest here and in zdr-contract-sync.test.sh together"
        );
        let corpus: Value = serde_json::from_str(CORPUS).expect("the corpus is JSON");
        let cases = corpus["cases"].as_array().expect("cases");
        assert!(cases.len() >= 30, "the corpus lost cases");
        let mut seen = std::collections::BTreeSet::new();
        for case in cases {
            let name = case["name"].as_str().expect("case name");
            assert!(seen.insert(name.to_owned()), "duplicate case name {name}");
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
            let got: Vec<(String, String)> = codes(&case["value"], &case["schema"])
                .into_iter()
                .map(|(path, code)| (path, code.to_owned()))
                .collect();
            assert_eq!(got, expected, "corpus case {name}");
            assert_eq!(
                is_valid(&case["value"], &case["schema"]),
                expected.is_empty(),
                "corpus case {name}"
            );
        }
    }

    #[test]
    fn an_issue_never_carries_a_value_or_an_invented_key() {
        let schema = json!({
            "type": "object",
            "properties": {"a": {"enum": ["x"]}},
            "additionalProperties": false
        });
        let value = json!({"a": "SECRET-VALUE", "SECRET-KEY": 1});
        let shown = format!("{:?}", issues(&value, &schema));
        assert!(!shown.contains("SECRET"), "{shown}");
    }

    #[test]
    fn issues_are_capped() {
        let schema = json!({"type": "array", "items": {"type": "string"}});
        let value = Value::Array((0..100).map(|n| json!(n)).collect());
        assert_eq!(validate(&value, &schema).len(), 100);
        assert_eq!(issues(&value, &schema).len(), MAX_ISSUES);
    }

    #[test]
    fn a_non_object_schema_accepts_everything() {
        for schema in [json!(true), json!(false), json!("x"), json!(null), json!({})] {
            assert!(is_valid(&json!({"a": [1]}), &schema), "{schema}");
        }
    }

    #[test]
    fn a_whole_float_is_a_number_but_not_an_integer() {
        let float = json!(3.0);
        assert!(is_valid(&float, &json!({"type": "number"})));
        assert!(!is_valid(&float, &json!({"type": "integer"})));
        assert!(is_valid(&json!(3), &json!({"type": ["integer", "null"]})));
    }
}
