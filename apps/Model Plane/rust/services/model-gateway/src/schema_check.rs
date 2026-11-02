//! A small JSON Schema subset validator for model-written structured output (WS16.S8).
//!
//! This is a port of Quarry's `quarry-core::json_schema` (the validator behind its
//! `json` format), so the gateway's interim Model-proposed extract route and
//! Quarry's own route accept and refuse the same documents. The shared behaviour
//! is pinned by `tests/fixtures/ai-extract/schema-corpus.json`.
//!
//! Supported keywords:
//!
//! - `type` (a name or a list of names; `integer` accepts only whole JSON
//!   integers, `number` accepts both)
//! - `enum`, `required`, `properties`, `items`
//! - `minItems` / `maxItems` / `minLength` / `maxLength` (string length in chars)
//! - `minimum` / `maximum` (inclusive)
//! - `additionalProperties: false`
//! - extensions Quarry's validator does not have: `const`, `anyOf`, `oneOf`
//!
//! Every other keyword is ignored, exactly as in Quarry. A non-object schema
//! (`true`, `false`, a string) accepts everything.
//!
//! Issues carry a path and a code only. A value, and a property name the model
//! invented, never appear in an issue: the issues are fed back to the model in
//! the repair prompt and into logs.

use serde_json::Value;

/// The most issues a caller keeps and shows.
pub const MAX_ISSUES: usize = 20;
/// Longest issue path kept, in chars.
const MAX_PATH_CHARS: usize = 200;

/// One reason a value does not fit a schema.
#[derive(Debug, Clone, PartialEq)]
pub enum ValidationIssue {
    MissingRequired { path: String, key: String },
    TypeMismatch { path: String, expected: String, got: String },
    EnumMismatch { path: String },
    ConstMismatch { path: String },
    LengthOutOfRange { path: String },
    NumberOutOfRange { path: String },
    AdditionalProperty { path: String },
    AnyOfNoMatch { path: String },
    OneOfMismatch { path: String, matched: usize },
}

/// An issue as it is shown: a path and a stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaIssue {
    pub path: String,
    pub code: &'static str,
}

impl ValidationIssue {
    /// The shown form. A missing key is named in the path (`$.a.b` for key `b`
    /// missing from `$.a`) because the schema, not the model, named it; an
    /// unexpected property is reported at its parent.
    #[must_use]
    pub fn issue(&self) -> SchemaIssue {
        let (path, code) = match self {
            Self::MissingRequired { path, key } => (format!("{path}.{key}"), "missing_required"),
            Self::TypeMismatch { path, .. } => (path.clone(), "type_mismatch"),
            Self::EnumMismatch { path } => (path.clone(), "enum_mismatch"),
            Self::ConstMismatch { path } => (path.clone(), "const_mismatch"),
            Self::LengthOutOfRange { path } => (path.clone(), "length_out_of_range"),
            Self::NumberOutOfRange { path } => (path.clone(), "number_out_of_range"),
            Self::AdditionalProperty { path } => (path.clone(), "additional_property"),
            Self::AnyOfNoMatch { path } => (path.clone(), "any_of_no_match"),
            Self::OneOfMismatch { path, .. } => (path.clone(), "one_of_mismatch"),
        };
        SchemaIssue {
            path: path.chars().take(MAX_PATH_CHARS).collect(),
            code,
        }
    }
}

/// Validate `value` against `schema`. Empty when valid.
#[must_use]
pub fn validate(value: &Value, schema: &Value) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    validate_at("$", value, schema, &mut issues);
    issues
}

/// The shown issues for `value`, at most [`MAX_ISSUES`].
#[must_use]
pub fn issues(value: &Value, schema: &Value) -> Vec<SchemaIssue> {
    validate(value, schema)
        .iter()
        .map(ValidationIssue::issue)
        .take(MAX_ISSUES)
        .collect()
}

#[must_use]
pub fn is_valid(value: &Value, schema: &Value) -> bool {
    validate(value, schema).is_empty()
}

fn validate_at(path: &str, value: &Value, schema: &Value, out: &mut Vec<ValidationIssue>) {
    let Some(schema_obj) = schema.as_object() else {
        return;
    };

    if let Some(declared) = schema_obj.get("type") {
        if !matches_type(value, declared) {
            out.push(ValidationIssue::TypeMismatch {
                path: path.to_owned(),
                expected: type_name(declared),
                got: type_of_value(value).to_owned(),
            });
            // A wrong type makes the other checks noise.
            return;
        }
    }

    if let Some(allowed) = schema_obj.get("enum").and_then(Value::as_array) {
        if !allowed.iter().any(|candidate| candidate == value) {
            out.push(ValidationIssue::EnumMismatch { path: path.to_owned() });
        }
    }

    if let Some(constant) = schema_obj.get("const") {
        if constant != value {
            out.push(ValidationIssue::ConstMismatch { path: path.to_owned() });
        }
    }

    if let Some(branches) = schema_obj.get("anyOf").and_then(Value::as_array) {
        if !branches.is_empty() && !branches.iter().any(|branch| is_valid(value, branch)) {
            out.push(ValidationIssue::AnyOfNoMatch { path: path.to_owned() });
        }
    }

    if let Some(branches) = schema_obj.get("oneOf").and_then(Value::as_array) {
        if !branches.is_empty() {
            let matched = branches.iter().filter(|branch| is_valid(value, branch)).count();
            if matched != 1 {
                out.push(ValidationIssue::OneOfMismatch {
                    path: path.to_owned(),
                    matched,
                });
            }
        }
    }

    match value {
        Value::Object(map) => {
            if let Some(required) = schema_obj.get("required").and_then(Value::as_array) {
                for key in required.iter().filter_map(Value::as_str) {
                    if !map.contains_key(key) {
                        out.push(ValidationIssue::MissingRequired {
                            path: path.to_owned(),
                            key: key.to_owned(),
                        });
                    }
                }
            }
            let properties = schema_obj.get("properties").and_then(Value::as_object);
            if let Some(properties) = properties {
                for (key, sub_schema) in properties {
                    if let Some(sub_value) = map.get(key) {
                        validate_at(&format!("{path}.{key}"), sub_value, sub_schema, out);
                    }
                }
            }
            if schema_obj.get("additionalProperties") == Some(&Value::Bool(false)) {
                for key in map.keys() {
                    if !properties.is_some_and(|known| known.contains_key(key)) {
                        out.push(ValidationIssue::AdditionalProperty { path: path.to_owned() });
                    }
                }
            }
        }
        Value::Array(items) => {
            check_length(
                path,
                items.len(),
                bound(schema_obj, "minItems"),
                bound(schema_obj, "maxItems"),
                out,
            );
            if let Some(item_schema) = schema_obj.get("items") {
                for (index, item) in items.iter().enumerate() {
                    validate_at(&format!("{path}[{index}]"), item, item_schema, out);
                }
            }
        }
        Value::String(text) => {
            check_length(
                path,
                text.chars().count(),
                bound(schema_obj, "minLength"),
                bound(schema_obj, "maxLength"),
                out,
            );
        }
        Value::Number(number) => {
            let n = number.as_f64().unwrap_or(0.0);
            let below = schema_obj
                .get("minimum")
                .and_then(Value::as_f64)
                .is_some_and(|min| n < min);
            let above = schema_obj
                .get("maximum")
                .and_then(Value::as_f64)
                .is_some_and(|max| n > max);
            // Quarry reports one issue per violated bound.
            if below {
                out.push(ValidationIssue::NumberOutOfRange { path: path.to_owned() });
            }
            if above {
                out.push(ValidationIssue::NumberOutOfRange { path: path.to_owned() });
            }
        }
        _ => {}
    }
}

fn bound(schema: &serde_json::Map<String, Value>, key: &str) -> Option<usize> {
    schema
        .get(key)
        .and_then(Value::as_u64)
        .map(|n| usize::try_from(n).unwrap_or(usize::MAX))
}

fn check_length(
    path: &str,
    actual: usize,
    min: Option<usize>,
    max: Option<usize>,
    out: &mut Vec<ValidationIssue>,
) {
    if min.is_some_and(|m| actual < m) || max.is_some_and(|m| actual > m) {
        out.push(ValidationIssue::LengthOutOfRange { path: path.to_owned() });
    }
}

fn matches_type(value: &Value, declared: &Value) -> bool {
    let actual = type_of_value(value);
    match declared {
        Value::String(name) => type_matches_one(actual, name),
        Value::Array(names) => names
            .iter()
            .any(|name| name.as_str().is_some_and(|name| type_matches_one(actual, name))),
        // A declaration of an unknown shape does not reject.
        _ => true,
    }
}

fn type_matches_one(actual: &str, declared: &str) -> bool {
    match declared {
        "number" => matches!(actual, "number" | "integer"),
        other => actual == other,
    }
}

fn type_of_value(value: &Value) -> &'static str {
    match value {
        Value::Object(_) => "object",
        Value::Array(_) => "array",
        Value::String(_) => "string",
        Value::Bool(_) => "boolean",
        Value::Null => "null",
        Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
        Value::Number(_) => "number",
    }
}

fn type_name(declared: &Value) -> String {
    match declared {
        Value::String(name) => name.clone(),
        Value::Array(names) => names
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("|"),
        _ => "unknown".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use sha2::{Digest, Sha256};

    /// The bytes of the shared corpus. Pinned: a change to the file is a change to the contract and
    /// goes with the lead's `zdr-contract-sync.test.sh` digest row.
    const CORPUS: &str = include_str!("../tests/fixtures/ai-extract/schema-corpus.json");
    const CORPUS_SHA256: &str = "f3ce2e27a78010d28df3e14bc688908e681426db6bfc2b968a59409810a7c6f5";

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
            hex, CORPUS_SHA256,
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
