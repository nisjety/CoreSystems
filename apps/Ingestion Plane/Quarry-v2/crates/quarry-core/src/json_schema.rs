//! Minimal JSON Schema (Draft 7-ish subset) validator used across Quarry +
//! Model Plane to validate structured-extract responses without pulling in a
//! full schema crate.
//!
//! Supports the subset Quarry actually uses:
//!
//! - **type**: `"object"`, `"array"`, `"string"`, `"integer"`, `"number"`,
//!   `"boolean"`, `"null"` (single string or array of strings)
//! - **required**: `[..]` keys must be present on objects
//! - **properties**: `{key: schema}` recursive validation per-key
//! - **items**: schema applied to every array element
//! - **enum**: value must equal one of the listed JSON values
//! - **minItems** / **maxItems** / **minLength** / **maxLength**
//! - **minimum** / **maximum** (numeric) inclusive bounds
//! - **additionalProperties: false** rejects unknown keys
//!
//! Returns a list of typed [`ValidationIssue`] paths so callers can
//! surface "data.user.age expected integer, got string" errors instead of
//! a vague "schema_valid: false". Enables Quarry's structured-extract
//! cost-ceiling enforcement to refuse malformed model responses with a
//! clear actionable error.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum ValidationIssue {
    MissingRequired {
        path: String,
        key: String,
    },
    TypeMismatch {
        path: String,
        expected: String,
        got: String,
    },
    EnumMismatch {
        path: String,
        allowed: Vec<Value>,
        got: Value,
    },
    LengthOutOfRange {
        path: String,
        len: usize,
        min: Option<usize>,
        max: Option<usize>,
    },
    NumberOutOfRange {
        path: String,
        value: f64,
        min: Option<f64>,
        max: Option<f64>,
    },
    AdditionalProperty {
        path: String,
        key: String,
    },
    InvalidSchema {
        path: String,
        reason: String,
    },
}

impl ValidationIssue {
    pub fn path(&self) -> &str {
        match self {
            ValidationIssue::MissingRequired { path, .. }
            | ValidationIssue::TypeMismatch { path, .. }
            | ValidationIssue::EnumMismatch { path, .. }
            | ValidationIssue::LengthOutOfRange { path, .. }
            | ValidationIssue::NumberOutOfRange { path, .. }
            | ValidationIssue::AdditionalProperty { path, .. }
            | ValidationIssue::InvalidSchema { path, .. } => path,
        }
    }

    pub fn human(&self) -> String {
        match self {
            ValidationIssue::MissingRequired { path, key } => {
                format!("{path}: missing required key `{key}`")
            }
            ValidationIssue::TypeMismatch {
                path,
                expected,
                got,
            } => {
                format!("{path}: expected {expected}, got {got}")
            }
            ValidationIssue::EnumMismatch { path, allowed, got } => {
                format!("{path}: value {got} not in enum {allowed:?}")
            }
            ValidationIssue::LengthOutOfRange {
                path,
                len,
                min,
                max,
            } => {
                format!("{path}: length {len} outside bounds min={min:?} max={max:?}")
            }
            ValidationIssue::NumberOutOfRange {
                path,
                value,
                min,
                max,
            } => {
                format!("{path}: value {value} outside bounds min={min:?} max={max:?}")
            }
            ValidationIssue::AdditionalProperty { path, key } => {
                format!("{path}: unexpected property `{key}` (additionalProperties=false)")
            }
            ValidationIssue::InvalidSchema { path, reason } => {
                format!("{path}: invalid schema fragment — {reason}")
            }
        }
    }
}

/// Validate `value` against `schema`. Returns an empty Vec when valid.
pub fn validate(value: &Value, schema: &Value) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    validate_at("$", value, schema, &mut issues);
    issues
}

/// Convenience: returns true when validation passes.
pub fn is_valid(value: &Value, schema: &Value) -> bool {
    validate(value, schema).is_empty()
}

fn validate_at(path: &str, value: &Value, schema: &Value, issues: &mut Vec<ValidationIssue>) {
    let Some(schema_obj) = schema.as_object() else {
        // A non-object schema is `true`/`false` — true matches everything,
        // false matches nothing. We treat anything non-object as permissive.
        return;
    };

    if let Some(t) = schema_obj.get("type") {
        if !matches_type(value, t) {
            issues.push(ValidationIssue::TypeMismatch {
                path: path.to_string(),
                expected: type_name(t),
                got: type_of_value(value).into(),
            });
            return; // type mismatch — further checks won't add useful info
        }
    }

    if let Some(en) = schema_obj.get("enum").and_then(|v| v.as_array()) {
        if !en.iter().any(|allowed| allowed == value) {
            issues.push(ValidationIssue::EnumMismatch {
                path: path.to_string(),
                allowed: en.clone(),
                got: value.clone(),
            });
        }
    }

    match value {
        Value::Object(map) => {
            let required = schema_obj
                .get("required")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            for key in &required {
                if !map.contains_key(key) {
                    issues.push(ValidationIssue::MissingRequired {
                        path: path.to_string(),
                        key: key.clone(),
                    });
                }
            }

            if let Some(props) = schema_obj.get("properties").and_then(|v| v.as_object()) {
                for (k, sub_schema) in props {
                    if let Some(sub_value) = map.get(k) {
                        let sub_path = format!("{path}.{k}");
                        validate_at(&sub_path, sub_value, sub_schema, issues);
                    }
                }
            }

            if schema_obj
                .get("additionalProperties")
                .map(|v| v == &Value::Bool(false))
                .unwrap_or(false)
            {
                let known: Vec<&str> = schema_obj
                    .get("properties")
                    .and_then(|v| v.as_object())
                    .map(|p| p.keys().map(|k| k.as_str()).collect())
                    .unwrap_or_default();
                for k in map.keys() {
                    if !known.contains(&k.as_str()) {
                        issues.push(ValidationIssue::AdditionalProperty {
                            path: path.to_string(),
                            key: k.clone(),
                        });
                    }
                }
            }
        }
        Value::Array(arr) => {
            let min = schema_obj
                .get("minItems")
                .and_then(|v| v.as_u64())
                .map(|n| n as usize);
            let max = schema_obj
                .get("maxItems")
                .and_then(|v| v.as_u64())
                .map(|n| n as usize);
            check_length(path, arr.len(), min, max, issues);

            if let Some(item_schema) = schema_obj.get("items") {
                for (i, item) in arr.iter().enumerate() {
                    let sub_path = format!("{path}[{i}]");
                    validate_at(&sub_path, item, item_schema, issues);
                }
            }
        }
        Value::String(s) => {
            let min = schema_obj
                .get("minLength")
                .and_then(|v| v.as_u64())
                .map(|n| n as usize);
            let max = schema_obj
                .get("maxLength")
                .and_then(|v| v.as_u64())
                .map(|n| n as usize);
            check_length(path, s.chars().count(), min, max, issues);
        }
        Value::Number(n) => {
            let f = n.as_f64().unwrap_or(0.0);
            let min = schema_obj.get("minimum").and_then(|v| v.as_f64());
            let max = schema_obj.get("maximum").and_then(|v| v.as_f64());
            if let Some(m) = min {
                if f < m {
                    issues.push(ValidationIssue::NumberOutOfRange {
                        path: path.to_string(),
                        value: f,
                        min,
                        max,
                    });
                }
            }
            if let Some(m) = max {
                if f > m {
                    issues.push(ValidationIssue::NumberOutOfRange {
                        path: path.to_string(),
                        value: f,
                        min,
                        max,
                    });
                }
            }
        }
        _ => {}
    }
}

fn matches_type(value: &Value, type_decl: &Value) -> bool {
    let actual = type_of_value(value);
    match type_decl {
        Value::String(s) => type_matches_one(actual, s),
        Value::Array(arr) => arr.iter().any(|t| {
            t.as_str()
                .map(|s| type_matches_one(actual, s))
                .unwrap_or(false)
        }),
        _ => true, // unknown type-decl shape: don't reject
    }
}

fn type_matches_one(actual: &str, decl: &str) -> bool {
    match decl {
        "number" => matches!(actual, "number" | "integer"),
        "integer" => actual == "integer",
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
        Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                "integer"
            } else {
                "number"
            }
        }
    }
}

fn type_name(t: &Value) -> String {
    match t {
        Value::String(s) => s.clone(),
        Value::Array(arr) => {
            let parts: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            parts.join("|")
        }
        _ => "unknown".into(),
    }
}

fn check_length(
    path: &str,
    actual: usize,
    min: Option<usize>,
    max: Option<usize>,
    issues: &mut Vec<ValidationIssue>,
) {
    let too_short = min.map(|m| actual < m).unwrap_or(false);
    let too_long = max.map(|m| actual > m).unwrap_or(false);
    if too_short || too_long {
        issues.push(ValidationIssue::LengthOutOfRange {
            path: path.to_string(),
            len: actual,
            min,
            max,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn valid_simple_object_passes() {
        let schema = json!({
            "type": "object",
            "required": ["name", "age"],
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "integer"}
            }
        });
        let value = json!({"name": "Alice", "age": 30});
        assert!(is_valid(&value, &schema));
    }

    #[test]
    fn missing_required_key_fails() {
        let schema = json!({"type": "object", "required": ["name", "age"]});
        let value = json!({"name": "Alice"});
        let issues = validate(&value, &schema);
        assert_eq!(issues.len(), 1);
        match &issues[0] {
            ValidationIssue::MissingRequired { key, .. } => assert_eq!(key, "age"),
            other => panic!("expected MissingRequired, got {other:?}"),
        }
    }

    #[test]
    fn type_mismatch_at_nested_path_reports_path() {
        let schema = json!({
            "type": "object",
            "properties": {
                "user": {
                    "type": "object",
                    "properties": {
                        "age": {"type": "integer"}
                    }
                }
            }
        });
        let value = json!({"user": {"age": "thirty"}});
        let issues = validate(&value, &schema);
        assert_eq!(issues.len(), 1);
        match &issues[0] {
            ValidationIssue::TypeMismatch {
                path,
                expected,
                got,
            } => {
                assert_eq!(path, "$.user.age");
                assert_eq!(expected, "integer");
                assert_eq!(got, "string");
            }
            other => panic!("expected TypeMismatch, got {other:?}"),
        }
    }

    #[test]
    fn integer_accepted_where_number_expected() {
        let schema = json!({"type": "number"});
        assert!(is_valid(&json!(42), &schema));
        assert!(is_valid(&json!(3.14), &schema));
    }

    #[test]
    fn float_rejected_where_integer_expected() {
        let schema = json!({"type": "integer"});
        assert!(!is_valid(&json!(3.14), &schema));
    }

    #[test]
    fn nullable_via_array_type() {
        let schema = json!({"type": ["string", "null"]});
        assert!(is_valid(&json!("hi"), &schema));
        assert!(is_valid(&json!(null), &schema));
        assert!(!is_valid(&json!(42), &schema));
    }

    #[test]
    fn enum_constraint_enforced() {
        let schema = json!({"enum": ["a", "b", "c"]});
        assert!(is_valid(&json!("a"), &schema));
        assert!(!is_valid(&json!("d"), &schema));
    }

    #[test]
    fn array_items_validated_per_element() {
        let schema = json!({
            "type": "array",
            "items": {"type": "string"}
        });
        let value = json!(["a", "b", 42]);
        let issues = validate(&value, &schema);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].path(), "$[2]");
    }

    #[test]
    fn min_max_items_enforced() {
        let schema = json!({"type": "array", "minItems": 2, "maxItems": 3});
        assert!(is_valid(&json!([1, 2]), &schema));
        assert!(is_valid(&json!([1, 2, 3]), &schema));
        assert!(!is_valid(&json!([1]), &schema));
        assert!(!is_valid(&json!([1, 2, 3, 4]), &schema));
    }

    #[test]
    fn min_max_length_string_enforced() {
        let schema = json!({"type": "string", "minLength": 2, "maxLength": 5});
        assert!(is_valid(&json!("ab"), &schema));
        assert!(is_valid(&json!("abcde"), &schema));
        assert!(!is_valid(&json!("a"), &schema));
        assert!(!is_valid(&json!("abcdef"), &schema));
    }

    #[test]
    fn min_max_number_enforced() {
        let schema = json!({"type": "number", "minimum": 0.0, "maximum": 100.0});
        assert!(is_valid(&json!(50), &schema));
        assert!(is_valid(&json!(0), &schema));
        assert!(is_valid(&json!(100), &schema));
        assert!(!is_valid(&json!(-1), &schema));
        assert!(!is_valid(&json!(101), &schema));
    }

    #[test]
    fn additional_properties_false_rejects_unknown_keys() {
        let schema = json!({
            "type": "object",
            "properties": {"name": {"type": "string"}},
            "additionalProperties": false
        });
        let value = json!({"name": "Alice", "extra": 1});
        let issues = validate(&value, &schema);
        assert_eq!(issues.len(), 1);
        match &issues[0] {
            ValidationIssue::AdditionalProperty { key, .. } => assert_eq!(key, "extra"),
            other => panic!("expected AdditionalProperty, got {other:?}"),
        }
    }

    #[test]
    fn additional_properties_default_permissive() {
        let schema = json!({"type": "object", "properties": {"name": {"type": "string"}}});
        let value = json!({"name": "Alice", "extra": 1});
        assert!(is_valid(&value, &schema));
    }

    #[test]
    fn human_message_includes_path_and_keys() {
        let schema = json!({"type": "object", "required": ["name"]});
        let value = json!({});
        let issues = validate(&value, &schema);
        assert!(issues[0].human().contains("missing required key `name`"));
    }

    #[test]
    fn deep_nested_validation_reports_full_path() {
        let schema = json!({
            "type": "object",
            "properties": {
                "users": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["email"]
                    }
                }
            }
        });
        let value = json!({"users": [{"email": "a@b"}, {}]});
        let issues = validate(&value, &schema);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].path(), "$.users[1]");
    }

    #[test]
    fn empty_schema_object_accepts_anything() {
        let schema = json!({});
        assert!(is_valid(&json!(42), &schema));
        assert!(is_valid(&json!({}), &schema));
        assert!(is_valid(&json!(null), &schema));
    }

    #[test]
    fn boolean_schema_treated_permissively() {
        let schema = json!(true);
        assert!(is_valid(&json!(42), &schema));
    }

    #[test]
    fn aggregates_multiple_issues() {
        let schema = json!({
            "type": "object",
            "required": ["a", "b"],
            "properties": {
                "a": {"type": "integer"},
                "b": {"type": "string"}
            }
        });
        let value = json!({"a": "not-int"}); // missing `b`, wrong type for `a`
        let issues = validate(&value, &schema);
        assert_eq!(issues.len(), 2);
    }
}
