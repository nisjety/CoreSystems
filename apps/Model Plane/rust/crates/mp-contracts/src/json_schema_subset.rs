//! A small JSON Schema subset validator for model-written structured output (WS16.S8).
//!
//! This is a port of Quarry's `quarry-core::json_schema` (the validator behind its
//! `json` format), so the gateway's interim Model-proposed extract route and
//! Quarry's own route accept and refuse the same documents. The shared behaviour
//! is pinned by `tests/fixtures/ai-extract/schema-corpus.json` in model-gateway,
//! whose `schema_check` module re-exports this one.
//!
//! The validator lives in this crate so that execution-core can check a
//! wide-research item's typed result (`SUBAGENT_TYPED_RESULTS_ENABLED`) with the
//! same rules as the gateway, rather than with a second copy of them.
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
