//! Validating a tool call's arguments against the tool's own declared schema,
//! before the call is dispatched.
//!
//! ## Why this exists
//!
//! A tool call whose arguments do not match its schema fails somewhere
//! downstream — in an executor, or at a remote MCP server that answers with
//! whatever prose it chooses, often "Invalid request" with no indication of which
//! field was at fault. The model then guesses. Checking here turns that into
//! exact field problems plus the schema itself, so the model can repair and retry
//! in one round: *validation failure → exact field errors → expose the schema →
//! repair → revalidate*.
//!
//! ## It had no callers
//!
//! This began life in `model-gateway/argument_repair.rs` with a doc comment
//! stating it was wired to `tool_loop`'s `mcp_call` arm. **That arm does not
//! exist and the wiring was never done** — 393 lines and 12 tests, reachable from
//! nothing, describing a tool that was never built. Moved here and wired to the
//! builtin dispatch paths of *both* loops, because a validator only one loop runs
//! is how a deployed agent starts accepting arguments chat would have refused.
//!
//! ## Fail open, always
//!
//! A validator that rejects a call the executor would have accepted is worse than
//! no validator at all: it breaks working integrations for the sake of tidiness.
//! So every rule here is one-directional — it reports a problem only when the
//! schema is unambiguous about it, and stays silent on everything else:
//!
//! - An unparseable schema, a non-object schema, a missing `properties`, or any
//!   construct beyond `required`/`type`/`enum` (`oneOf`, `$ref`, nested object
//!   schemas, `pattern`, `minimum`, …) ⇒ **no opinion**.
//! - A field the schema does not mention ⇒ **no opinion** (`additionalProperties`
//!   is commonly open, and rejecting an extra field is a pure false positive).
//! - A `null` value ⇒ **no type opinion**, since servers differ on whether null
//!   means "absent" or "explicitly empty".
//! - Type coercions models make routinely and executors routinely accept (a
//!   quoted number for a numeric field, a number for a string field) ⇒
//!   **allowed**. Only a genuinely irreconcilable mismatch — a non-numeric word
//!   where a number is required, a scalar where an object or array is required —
//!   is reported.
//!
//! The result catches the two failure modes that actually dominate (a missing
//! required field, and a value outside a declared `enum`) and otherwise gets out
//! of the way.

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
/// Only the schema of the tool the model already chose. Deliberately not the
/// whole catalogue: it has made its choice, and re-listing everything would
/// spend prompt on a decision that is already made.
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

/// Required argument paths whose values can come **only** from the user or from
/// a prior tool result — never from the model's own knowledge.
///
/// # Why a table and not a schema property
///
/// JSON Schema has no way to say "this value must be grounded in the request".
/// It can say a field is required, which is what
/// [`validate_arguments`] already checks — and that check passes an invented
/// postal code, because an invented value is perfectly well-formed. Measured
/// 2026-08-25 on the live catalogue: required values here were fabricated in
/// 55-70 % of samples, every one schema-valid, and adding "never guess them" to
/// the tool description moved it to 60 % correct and no further. Prose persuades;
/// this refuses.
///
/// Deliberately narrow, and it must stay that way. A path belongs here only when
/// no legitimate source other than the conversation exists. `web_search.query`
/// restates the request, `yr_weather.lat/lon` is public fact, and
/// `execute_provider_action.connection_id` is *discoverable* by another offered
/// tool — the model resolves that one unprompted, 10/10 measured. Adding any of
/// those would refuse calls that were about to succeed.
///
/// Kept in step with `USER_SUPPLIED_ARG_TOOLS` (the prompt-side list in
/// execution-core) by a contract test: a tool told to ask for a value it is then
/// never checked on, or checked on a value it was never told about, is a
/// half-wired rule.
const GROUNDED_ARGUMENT_PATHS: &[(&str, &[&str])] = &[
    (
        "get_shipping_quotes",
        &[
            "from.postal_code",
            "to.postal_code",
            "from.name",
            "to.name",
            "length_cm",
            "width_cm",
            "height_cm",
        ],
    ),
    (
        "book_shipment",
        &[
            "from.postal_code",
            "to.postal_code",
            "from.name",
            "to.name",
            "length_cm",
            "width_cm",
            "height_cm",
            // This one places a real order that costs money. Its own description
            // says to use "the exact carrier_code/service_name/price from the
            // quote the user chose", so all three come from a prior tool result
            // and are in the conversation when the call is legitimate. A price
            // that is not there was invented, and an invented price books a real
            // shipment at a number nobody agreed to.
            "price_amount_cents",
            "carrier_code",
            "service_name",
        ],
    ),
    // The chat loop's shipping tool. Same exposure as `get_shipping_quotes`
    // under a different NAME and a different SHAPE — dimensions nest under
    // `package` here. Both facts matter: keying the table on the agent loop's
    // spelling alone left the chat loop's gate reachable but never firing, which
    // is the "wired but inert" failure this repo keeps producing. Its
    // description does not even carry the "never guess" line the agent's does.
    (
        "shipping_get_quotes",
        &[
            "from.postal_code",
            "to.postal_code",
            "from.name",
            "to.name",
            "package.length_cm",
            "package.width_cm",
            "package.height_cm",
        ],
    ),
    // THIRD spelling of the same capability. The chat dispatch arm accepts
    // `"shipping_get_quotes" | "shipping.get_quotes"` because the Console's
    // explicit tool selection uses the dotted id as a client-DECLARED tool —
    // which `inline_tool_allowed` admits (it is a denylist) and which no
    // builtin schema covers, so before this entry the dotted name reached the
    // real executor having skipped BOTH the schema check and this gate. A
    // grounding table keyed on exact names must key on every dispatchable
    // spelling, or the alias is the bypass.
    (
        "shipping.get_quotes",
        &[
            "from.postal_code",
            "to.postal_code",
            "from.name",
            "to.name",
            "package.length_cm",
            "package.width_cm",
            "package.height_cm",
        ],
    ),
];

/// Tools this module will ground-check. Exposed so the prompt-side list can be
/// pinned against it rather than trusted to stay in step by hand.
#[must_use]
pub fn ground_checked_tools() -> Vec<&'static str> {
    GROUNDED_ARGUMENT_PATHS
        .iter()
        .map(|(name, _)| *name)
        .collect()
}

/// Every run of digits in `text`, as strings.
///
/// Digit runs rather than substring search: `30` must match "30x20x15 cm" and
/// must **not** match "130 kg". A substring test gets the second one wrong, and
/// getting it wrong means refusing a call whose value was stated.
fn digit_runs(text: &str) -> Vec<String> {
    let mut runs = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            current.push(ch);
        } else if !current.is_empty() {
            runs.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        runs.push(current);
    }
    runs
}

/// Does `value` appear in the conversation?
///
/// Numbers match a digit run, additionally allowing the value scaled by 100 in
/// either direction — a price stated as "199" legitimately becomes `19900` minor
/// units, and refusing that conversion would break a call the user explicitly
/// authorised. Strings match case-insensitively as a substring, which is the
/// lenient direction on purpose: this check exists to catch values with no
/// source at all, not to police phrasing.
fn value_is_grounded(value: &Value, text: &str, runs: &[String]) -> bool {
    match value {
        Value::Number(number) => {
            let candidates = if let Some(int) = number.as_i64() {
                let mut out = vec![int.to_string()];
                if int % 100 == 0 {
                    out.push((int / 100).to_string());
                }
                out.push((int.saturating_mul(100)).to_string());
                out
            } else {
                vec![number.to_string()]
            };
            candidates.iter().any(|candidate| {
                runs.iter().any(|run| {
                    run == candidate
                        || run.trim_start_matches('0') == candidate.trim_start_matches('0')
                })
            })
        }
        Value::String(raw) => {
            let needle = raw.trim();
            if needle.chars().count() < 2 {
                // A one-character value matches almost anything; claiming it is
                // grounded and claiming it is not are both noise.
                return true;
            }
            if needle.chars().all(|ch| ch.is_ascii_digit()) {
                return runs.iter().any(|run| {
                    run == needle || run.trim_start_matches('0') == needle.trim_start_matches('0')
                });
            }
            text.to_lowercase().contains(&needle.to_lowercase())
        }
        // Booleans, objects and arrays are not values a user "states"; nothing
        // here can judge them, so nothing here refuses them.
        _ => true,
    }
}

/// Read a dotted path out of an arguments object.
fn value_at_path<'a>(args: &'a Value, path: &str) -> Option<&'a Value> {
    let mut node = args;
    for segment in path.split('.') {
        node = node.get(segment)?;
    }
    Some(node)
}

/// Required values this call supplied that appear nowhere in `conversation`.
///
/// Fails **open** in the same places [`validate_arguments`] does: an unparseable
/// call, an unknown tool, or an empty conversation yields no errors. A check that
/// cannot see the request must not refuse on that basis — the whole point is to
/// catch a value with no source, and "no visible source" and "nothing visible"
/// are different facts.
#[must_use]
pub fn ungrounded_arguments(
    tool_name: &str,
    arguments_json: &str,
    conversation: &str,
) -> Vec<ArgumentError> {
    if conversation.trim().is_empty() {
        return Vec::new();
    }
    let Some((_, paths)) = GROUNDED_ARGUMENT_PATHS
        .iter()
        .find(|(name, _)| *name == tool_name)
    else {
        return Vec::new();
    };
    let Ok(args) = serde_json::from_str::<Value>(arguments_json) else {
        return Vec::new();
    };
    let runs = digit_runs(conversation);

    let mut errors = Vec::new();
    for path in *paths {
        let Some(value) = value_at_path(&args, path) else {
            continue;
        };
        if value.is_null() {
            continue;
        }
        if !value_is_grounded(value, conversation, &runs) {
            errors.push(ArgumentError {
                field: (*path).to_owned(),
                problem: format!(
                    "you supplied {value}, which appears nowhere in this conversation. \
                     Only the user can provide it."
                ),
            });
        }
    }
    errors
}

/// The message returned in place of a call built on values the user never gave.
///
/// Deliberately **not** [`repair_message`]. That one says "fix this field", which
/// against a fabrication invites a second guess — the model already produced a
/// schema-valid value and has no reason to think the next one is worse. This says
/// the value has no source and names asking as the only way forward, and it says
/// not to substitute, because the measured failure mode is substitution.
#[must_use]
pub fn grounding_message(tool_name: &str, errors: &[ArgumentError]) -> String {
    let mut out = format!(
        "The call to `{tool_name}` was not made. It supplied values that appear nowhere in this \
         conversation:\n"
    );
    for error in errors {
        out.push_str(&format!("  - {}: {}\n", error.field, error.problem));
    }
    out.push_str(
        "Do not retry with different values — a guessed address or dimension produces a real, \
         plausible, wrong result that nobody can tell from a correct one. Ask the user for exactly \
         these values in one short question, then call the tool again with what they give you.",
    );
    out
}

#[cfg(test)]
mod grounding_tests {
    use super::*;

    const QUOTE: &str = "get_shipping_quotes";

    fn args(json: &str) -> String {
        json.to_owned()
    }

    /// The measured defect, verbatim: the model invented postal codes and
    /// dimensions for a query that stated only weight and two city names.
    #[test]
    fn an_invented_postal_code_and_dimensions_are_refused() {
        let errors = ungrounded_arguments(
            QUOTE,
            &args(
                r#"{"from":{"name":"Bergen","postal_code":"5000","city":"Bergen"},
                    "to":{"name":"Stavanger","postal_code":"4000","city":"Stavanger"},
                    "weight_kg":3,"length_cm":30,"width_cm":20,"height_cm":10}"#,
            ),
            "hva vil det koste å sende 3 kg fra Bergen til Stavanger?",
        );
        let fields: Vec<&str> = errors.iter().map(|e| e.field.as_str()).collect();
        assert!(fields.contains(&"from.postal_code"), "got {fields:?}");
        assert!(fields.contains(&"to.postal_code"), "got {fields:?}");
        assert!(fields.contains(&"length_cm"), "got {fields:?}");
        // The city names WERE stated, so they are grounded and must not be flagged.
        assert!(!fields.contains(&"from.name"), "Bergen is in the request");
        assert!(!fields.contains(&"to.name"), "Stavanger is in the request");
    }

    /// The false-refusal case that matters most: everything stated, so nothing
    /// may be refused. A grounding check that fires here is worse than none.
    #[test]
    fn a_fully_specified_request_is_never_refused() {
        assert_eq!(
            ungrounded_arguments(
                QUOTE,
                &args(
                    r#"{"from":{"name":"Storgata 1","postal_code":"0155","city":"Oslo"},
                        "to":{"name":"Kongens gate 2","postal_code":"7011","city":"Trondheim"},
                        "weight_kg":5,"length_cm":30,"width_cm":20,"height_cm":15}"#,
                ),
                "compare shipping prices for a 5 kg parcel, 30x20x15 cm, from Storgata 1, \
                 0155 Oslo to Kongens gate 2, 7011 Trondheim",
            ),
            Vec::new(),
            "every value appears in the request"
        );
    }

    /// `30` must match "30x20x15" and must NOT match "130". A substring test
    /// gets the second wrong, and getting it wrong refuses a stated value.
    #[test]
    fn a_number_matches_a_digit_run_and_not_a_longer_number() {
        let runs = digit_runs("30x20x15 cm");
        assert!(value_is_grounded(
            &serde_json::json!(30),
            "30x20x15 cm",
            &runs
        ));
        assert!(value_is_grounded(
            &serde_json::json!(15),
            "30x20x15 cm",
            &runs
        ));

        let runs = digit_runs("the parcel weighs 130 kg");
        assert!(
            !value_is_grounded(&serde_json::json!(30), "the parcel weighs 130 kg", &runs),
            "30 is a substring of 130 but was never stated"
        );
    }

    /// A price stated in kroner legitimately becomes minor units. Refusing that
    /// conversion would block a booking the user explicitly authorised.
    #[test]
    fn a_minor_unit_conversion_counts_as_grounded() {
        let text = "book the Bring option at 199 NOK";
        let runs = digit_runs(text);
        assert!(
            value_is_grounded(&serde_json::json!(19900), text, &runs),
            "199 kroner is 19900 minor units"
        );
    }

    /// A leading zero is formatting, not a different postal code.
    #[test]
    fn a_leading_zero_does_not_break_the_match() {
        let text = "send it to 0155 Oslo";
        let runs = digit_runs(text);
        assert!(value_is_grounded(&serde_json::json!("0155"), text, &runs));
        assert!(value_is_grounded(&serde_json::json!(155), text, &runs));
    }

    /// Values from an earlier tool result are grounded — the conversation is the
    /// unit of grounding, not the latest user turn. Otherwise every second-turn
    /// booking would be refused.
    #[test]
    fn a_value_from_a_prior_tool_result_is_grounded() {
        let conversation = "compare prices for a 5 kg parcel from Oslo to Trondheim\n\
                            Tool results for your previous request:\n\
                            get_shipping_quotes -> carrier_code=BRING postal codes 0155 -> 7011, \
                            30x20x15 cm";
        assert_eq!(
            ungrounded_arguments(
                QUOTE,
                &args(
                    r#"{"from":{"postal_code":"0155"},"to":{"postal_code":"7011"},
                        "length_cm":30,"width_cm":20,"height_cm":15}"#,
                ),
                conversation,
            ),
            Vec::new()
        );
    }

    /// Fails open everywhere it cannot see enough to judge. "No visible source"
    /// and "nothing visible" are different facts and only the first may refuse.
    #[test]
    fn it_fails_open_on_anything_it_cannot_judge() {
        let bad = r#"{"from":{"postal_code":"9999"}}"#;
        assert_eq!(
            ungrounded_arguments(QUOTE, bad, "   "),
            Vec::new(),
            "an empty conversation is not evidence of fabrication"
        );
        assert_eq!(
            ungrounded_arguments("web_search", bad, "anything"),
            Vec::new(),
            "an unlisted tool is left alone"
        );
        assert_eq!(
            ungrounded_arguments(QUOTE, "not json", "anything"),
            Vec::new(),
            "an unparseable call is the schema validator's business, not this one's"
        );
        assert_eq!(
            ungrounded_arguments(QUOTE, r#"{"weight_kg":3}"#, "send 3 kg"),
            Vec::new(),
            "an absent path is not an ungrounded one"
        );
    }

    /// The message must not invite a second guess — the model already produced a
    /// schema-valid value and has no reason to think another would be worse.
    #[test]
    fn the_message_says_ask_rather_than_retry() {
        let errors = vec![ArgumentError {
            field: "from.postal_code".to_owned(),
            problem: "invented".to_owned(),
        }];
        let message = grounding_message(QUOTE, &errors).to_lowercase();
        assert!(message.contains("do not retry with different values"));
        assert!(message.contains("ask the user"));
        assert!(message.contains("from.postal_code"));
    }

    /// The list stays narrow, or it refuses calls that were about to succeed.
    #[test]
    fn only_values_with_no_other_source_are_ground_checked() {
        let checked = ground_checked_tools();
        for open_ended in [
            "web_search",
            "knowledge_search",
            "yr_weather",
            "code_interpreter",
            "execute_provider_action",
            "read_subagent_result",
        ] {
            assert!(
                !checked.contains(&open_ended),
                "{open_ended}'s required values restate the request, are public fact, \
                 or are discoverable by another offered tool"
            );
        }
    }
}
