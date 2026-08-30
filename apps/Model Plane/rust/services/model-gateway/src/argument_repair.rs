//! Argument repair — `MODEL_PLANE_IMPROVEMENTS_2026.md` §23.8.
//!
//! ## Why this exists
//!
//! §23.12's staged disclosure lets the model invoke a tool through `mcp_call`
//! by name. That is the point — but it also means the model can call a tool
//! whose full input schema it never inspected (`mcp_catalog` returns Level-1
//! summaries by default; the full schema is a second, optional call). When the
//! arguments are wrong, the cost today is a full round-trip to a remote MCP
//! server, which answers with whatever prose it chooses — often "Invalid
//! request" with no indication of which field was at fault.
//!
//! This module checks the arguments against the tool's own declared schema
//! **before** the call leaves the gateway, and on failure returns the exact
//! field problems plus the schema itself, so the model can repair and retry in
//! one round rather than guessing. §23.8's flow, precisely:
//! `validation failure → exact field errors → expose the selected capability
//! schema → repair → revalidate`.
//!
//! ## Fail open, always
//!
//! A validator that rejects a call the upstream would have accepted is worse
//! than no validator at all: it breaks working integrations for the sake of
//! tidiness. So every rule here is one-directional — it reports a problem only
//! when the schema is unambiguous about it, and stays silent on everything
//! else:
//!
//! - An unparseable schema, a non-object schema, a missing `properties`, or
//!   any construct beyond `required`/`type`/`enum` (`oneOf`, `$ref`, nested
//!   object schemas, `pattern`, `minimum`, …) ⇒ **no opinion**.
//! - A field the schema does not mention ⇒ **no opinion** (`additionalProperties`
//!   is commonly open, and rejecting an extra field is a pure false positive).
//! - A `null` value ⇒ **no type opinion**, since servers differ on whether
//!   null means "absent" or "explicitly empty".
//! - Type coercions models make routinely and servers routinely accept
//!   (a quoted number for a numeric field, a number for a string field) ⇒
//!   **allowed**. Only a genuinely irreconcilable mismatch — a non-numeric
//!   word where a number is required, a scalar where an object or array is
//!   required — is reported.
//!
//! The result is a validator that catches the two failure modes that actually
//! dominate (a missing required field, and a value outside a declared `enum`)
//! and otherwise gets out of the way.
//!
//! ## Where it is wired
//!
//! `tool_loop`'s `mcp_call` arm only — the staged path, which already fetches
//! the tool definitions to validate the tool name, so the schema is free.
//! The direct `mcp__<server>__<tool>` arm is **deliberately left untouched**:
//! it is the path proven live against Visma, and it holds no schema at hand,
//! so validating there would mean both new risk and a new lookup for no new
//! information.

use serde_json::Value;

/// One thing wrong with the supplied arguments, named precisely enough that
/// the model can fix that field without re-reading the whole schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArgumentError {
    /// The offending field name.
    pub field: String,
    /// What is wrong with it, phrased as an instruction.
    pub problem: String,
}

/// Check `arguments_json` against `schema_json`.
///
/// Returns an empty vector when the arguments are acceptable **or** when the
/// schema does not say enough to judge — see the module docs on failing open.
#[must_use]
pub fn validate_arguments(schema_json: &str, arguments_json: &str) -> Vec<ArgumentError> {
    let Ok(schema) = serde_json::from_str::<Value>(schema_json) else {
        return Vec::new();
    };
    // Only object-shaped inputs are checkable here; anything else is outside
    // what this validator claims to understand.
    if schema.get("type").and_then(Value::as_str) != Some("object") {
        return Vec::new();
    }

    let trimmed = arguments_json.trim();
    let arguments = if trimmed.is_empty() {
        Value::Object(serde_json::Map::new())
    } else {
        match serde_json::from_str::<Value>(trimmed) {
            Ok(value) => value,
            // Unparseable arguments are unambiguously wrong, and saying so
            // locally beats letting a remote server reject the bytes.
            Err(error) => {
                return vec![ArgumentError {
                    field: "(arguments)".to_owned(),
                    problem: format!(
                        "arguments must be a JSON object, but this did not parse: {error}"
                    ),
                }]
            }
        }
    };
    let Some(supplied) = arguments.as_object() else {
        return vec![ArgumentError {
            field: "(arguments)".to_owned(),
            problem: "arguments must be a JSON object".to_owned(),
        }];
    };

    let mut errors = Vec::new();

    // 1. Required fields. `required` names keys that must be PRESENT — a key
    //    present with a null value satisfies it, exactly as JSON Schema says.
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for name in required.iter().filter_map(Value::as_str) {
            if !supplied.contains_key(name) {
                errors.push(ArgumentError {
                    field: name.to_owned(),
                    problem: "required, but missing".to_owned(),
                });
            }
        }
    }

    // 2. Per-field checks, only for fields the schema actually describes.
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return errors;
    };
    for (name, spec) in properties {
        let Some(actual) = supplied.get(name) else {
            continue;
        };
        // Null carries no reliable type signal — see module docs.
        if actual.is_null() {
            continue;
        }
        if let Some(problem) = enum_problem(spec, actual) {
            errors.push(ArgumentError {
                field: name.clone(),
                problem,
            });
            // A value outside the enum is already the whole story for this
            // field; a type complaint on top would just be noise.
            continue;
        }
        if let Some(problem) = type_problem(spec, actual) {
            errors.push(ArgumentError {
                field: name.clone(),
                problem,
            });
        }
    }
    errors
}

/// Report a value that is not among a declared `enum`. The schema lists the
/// legal values explicitly, so this is never a guess.
fn enum_problem(spec: &Value, actual: &Value) -> Option<String> {
    let allowed = spec.get("enum")?.as_array()?;
    if allowed.is_empty() || allowed.contains(actual) {
        return None;
    }
    // Compare strings case-insensitively before complaining — a model that
    // sent "B2B" for an enum of ["b2b"] made a trivially repairable mistake,
    // and many servers fold case anyway.
    if let Some(text) = actual.as_str() {
        if allowed
            .iter()
            .filter_map(Value::as_str)
            .any(|candidate| candidate.eq_ignore_ascii_case(text))
        {
            return None;
        }
    }
    let rendered: Vec<String> = allowed.iter().map(ToString::to_string).collect();
    Some(format!(
        "must be one of {}, but got {actual}",
        rendered.join(", ")
    ))
}

/// Report only irreconcilable type mismatches. Every coercion a server might
/// plausibly accept is allowed through — see the module docs.
fn type_problem(spec: &Value, actual: &Value) -> Option<String> {
    let expected = spec.get("type")?.as_str()?;
    match expected {
        // A number or bool where a string is wanted is the single most common
        // model slip and is coerced by essentially every server.
        "string" => (actual.is_object() || actual.is_array())
            .then(|| format!("must be a string, but got {}", describe(actual))),
        "number" | "integer" => {
            if actual.is_number() {
                return None;
            }
            // A quoted number is fine; a word is not.
            match actual.as_str() {
                Some(text) if text.trim().parse::<f64>().is_ok() => None,
                _ => Some(format!("must be a number, but got {}", describe(actual))),
            }
        }
        "boolean" => {
            if actual.is_boolean() {
                return None;
            }
            match actual.as_str() {
                Some(text)
                    if matches!(text.trim().to_ascii_lowercase().as_str(), "true" | "false") =>
                {
                    None
                }
                _ => Some(format!(
                    "must be true or false, but got {}",
                    describe(actual)
                )),
            }
        }
        "object" => (!actual.is_object())
            .then(|| format!("must be an object, but got {}", describe(actual))),
        "array" => {
            (!actual.is_array()).then(|| format!("must be an array, but got {}", describe(actual)))
        }
        // "null", unions, and anything else: no opinion.
        _ => None,
    }
}

fn describe(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(_) => "a boolean".to_owned(),
        Value::Number(_) => "a number".to_owned(),
        Value::String(text) => format!("the text \"{}\"", truncate(text, 40)),
        Value::Array(_) => "an array".to_owned(),
        Value::Object(_) => "an object".to_owned(),
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    text.chars().take(max).collect::<String>() + "…"
}

/// Build the message returned in place of the rejected call.
///
/// Carries §23.8's third step — "expose only selected capability schema" —
/// by including *this one tool's* schema inline, so the repair needs no extra
/// `mcp_catalog` round-trip. Deliberately not the whole catalog: the model has
/// already chosen its capability, and re-listing the rest would undo the
/// context saving staged disclosure exists for.
#[must_use]
pub fn repair_message(tool_name: &str, errors: &[ArgumentError], schema_json: &str) -> String {
    let problems: Vec<String> = errors
        .iter()
        .map(|error| format!("  - {}: {}", error.field, error.problem))
        .collect();
    format!(
        "'{tool_name}' was NOT called — its arguments did not match the tool's own schema, so \
         nothing was sent and nothing changed. Fix these and call it again:\n{}\n\nThe schema for \
         {tool_name} is:\n{schema_json}",
        problems.join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORDER_SCHEMA: &str = r#"{
        "type": "object",
        "properties": {
            "order_id": {"type": "string"},
            "quantity": {"type": "integer"},
            "confirmed": {"type": "boolean"},
            "segment": {"type": "string", "enum": ["b2b", "b2c"]},
            "address": {"type": "object"},
            "tags": {"type": "array"}
        },
        "required": ["order_id", "segment"]
    }"#;

    #[test]
    fn accepts_arguments_that_match_the_schema() {
        let args = r#"{"order_id":"SO-1","segment":"b2b","quantity":3,"confirmed":true}"#;
        assert!(validate_arguments(ORDER_SCHEMA, args).is_empty());
    }

    #[test]
    fn reports_each_missing_required_field_by_name() {
        let errors = validate_arguments(ORDER_SCHEMA, r#"{"quantity":1}"#);
        let fields: Vec<&str> = errors.iter().map(|e| e.field.as_str()).collect();
        assert_eq!(fields, vec!["order_id", "segment"]);
        assert!(errors[0].problem.contains("required"));
    }

    #[test]
    fn a_present_but_null_field_still_satisfies_required() {
        // JSON Schema's `required` is about key presence, and servers differ
        // on what null means — so this must not be reported.
        let errors = validate_arguments(ORDER_SCHEMA, r#"{"order_id":null,"segment":"b2b"}"#);
        assert!(errors.is_empty(), "unexpected: {errors:?}");
    }

    #[test]
    fn reports_a_value_outside_a_declared_enum_and_lists_the_legal_ones() {
        let errors =
            validate_arguments(ORDER_SCHEMA, r#"{"order_id":"SO-1","segment":"wholesale"}"#);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].field, "segment");
        assert!(errors[0].problem.contains("b2b") && errors[0].problem.contains("b2c"));
    }

    #[test]
    fn enum_matching_folds_case_rather_than_failing_a_trivially_right_call() {
        let errors = validate_arguments(ORDER_SCHEMA, r#"{"order_id":"SO-1","segment":"B2B"}"#);
        assert!(errors.is_empty(), "unexpected: {errors:?}");
    }

    #[test]
    fn coercions_servers_accept_are_allowed_through() {
        // Quoted number for an integer field, quoted bool for a boolean field,
        // and a number for a string field — all routine, all coerced widely.
        let args = r#"{"order_id":12345,"segment":"b2b","quantity":"3","confirmed":"true"}"#;
        let errors = validate_arguments(ORDER_SCHEMA, args);
        assert!(errors.is_empty(), "unexpected: {errors:?}");
    }

    #[test]
    fn reports_only_irreconcilable_type_mismatches() {
        let args =
            r#"{"order_id":"SO-1","segment":"b2b","quantity":"many","address":"Oslo","tags":"a"}"#;
        let errors = validate_arguments(ORDER_SCHEMA, args);
        let fields: Vec<&str> = errors.iter().map(|e| e.field.as_str()).collect();
        assert_eq!(fields, vec!["address", "quantity", "tags"]);
        assert!(errors
            .iter()
            .any(|e| e.problem.contains("must be a number")));
        assert!(errors
            .iter()
            .any(|e| e.problem.contains("must be an object")));
        assert!(errors
            .iter()
            .any(|e| e.problem.contains("must be an array")));
    }

    #[test]
    fn a_field_the_schema_does_not_mention_is_never_an_error() {
        // additionalProperties is commonly open; complaining is a pure false
        // positive that would break a working call.
        let args = r#"{"order_id":"SO-1","segment":"b2b","undocumented_extra":{"x":1}}"#;
        assert!(validate_arguments(ORDER_SCHEMA, args).is_empty());
    }

    #[test]
    fn says_nothing_when_the_schema_says_nothing() {
        // Unparseable, non-object, open, and constructs beyond this
        // validator's vocabulary all mean "no opinion".
        assert!(validate_arguments("not json", r#"{"a":1}"#).is_empty());
        assert!(validate_arguments(r#"{"type":"string"}"#, r#""x""#).is_empty());
        assert!(validate_arguments(r#"{"type":"object"}"#, r#"{"a":1}"#).is_empty());
        let composed = r#"{"type":"object","properties":{"a":{"oneOf":[{"type":"string"}]}}}"#;
        assert!(validate_arguments(composed, r#"{"a":123}"#).is_empty());
        let constrained = r#"{"type":"object","properties":{"a":{"type":"integer","minimum":10}}}"#;
        assert!(validate_arguments(constrained, r#"{"a":1}"#).is_empty());
    }

    #[test]
    fn empty_arguments_are_treated_as_an_empty_object_not_a_parse_failure() {
        let errors = validate_arguments(ORDER_SCHEMA, "");
        // Reports the missing required fields, not a parse error.
        assert_eq!(errors.len(), 2);
        assert!(errors.iter().all(|e| e.problem.contains("required")));
    }

    #[test]
    fn unparseable_arguments_are_reported_as_such() {
        let errors = validate_arguments(ORDER_SCHEMA, "{not json");
        assert_eq!(errors.len(), 1);
        assert!(errors[0].problem.contains("did not parse"));
    }

    #[test]
    fn the_repair_message_names_the_fields_and_carries_the_schema() {
        let errors = validate_arguments(ORDER_SCHEMA, r#"{"quantity":1}"#);
        let message = repair_message("mcp__srv__create_order", &errors, ORDER_SCHEMA);

        // States plainly that nothing happened — a model that thinks the call
        // half-succeeded may take a compensating action it should not.
        assert!(message.contains("NOT called"));
        assert!(message.contains("nothing was sent"));
        assert!(message.contains("order_id") && message.contains("segment"));
        // The schema travels with the error, so the repair needs no extra
        // catalog round-trip.
        assert!(message.contains("\"required\""));
    }
}
