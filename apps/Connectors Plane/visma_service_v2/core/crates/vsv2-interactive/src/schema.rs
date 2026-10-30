//! A small checker for the JSON Schema subset that Visma's OpenAPI request bodies use: `type`,
//! `properties`, `required`, `items` and `enum`. An object that declares `properties` is closed by
//! default: a misspelt field name is refused here, because the API would otherwise drop it without a
//! word. Anything outside this subset is ignored, which is the lenient direction.

use serde_json::Value;

/// How many problems one check reports; the rest are not listed.
const MAX_ERRORS: usize = 20;

/// Check `value` against `schema`. Returns every problem found (up to a bound), each prefixed with the
/// path of the offending value (`$.lines[0].quantity.value`).
pub fn check(schema: &Value, value: &Value) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    walk(schema, value, "$", &mut errors);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn walk(schema: &Value, value: &Value, at: &str, errors: &mut Vec<String>) {
    if errors.len() >= MAX_ERRORS {
        return;
    }
    let Some(node) = schema.as_object() else {
        return;
    };
    if let Some(expected) = node.get("type").and_then(Value::as_str) {
        if !type_matches(expected, value) {
            errors.push(format!("{at}: expected {expected}, got {}", kind(value)));
            return;
        }
    }
    if let Some(allowed) = node.get("enum").and_then(Value::as_array) {
        if !allowed.contains(value) {
            errors.push(format!("{at}: must be one of {}", render(allowed)));
        }
    }
    if let (Some(items), Some(array)) = (node.get("items"), value.as_array()) {
        for (index, item) in array.iter().enumerate() {
            walk(items, item, &format!("{at}[{index}]"), errors);
        }
    }
    let Some(map) = value.as_object() else {
        return;
    };
    if let Some(required) = node.get("required").and_then(Value::as_array) {
        for name in required.iter().filter_map(Value::as_str) {
            if !map.contains_key(name) {
                errors.push(format!("{at}.{name}: required"));
            }
        }
    }
    let Some(properties) = node.get("properties").and_then(Value::as_object) else {
        return;
    };
    for key in map.keys() {
        if !properties.contains_key(key) {
            errors.push(format!("{at}.{key}: unknown field"));
        }
    }
    let required: Vec<&str> = node
        .get("required")
        .and_then(Value::as_array)
        .map(|names| names.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    for (key, child) in map {
        // `null` for an optional property means "not given": Visma's own descriptions say "null or omitted".
        if child.is_null() && !required.contains(&key.as_str()) {
            continue;
        }
        if let Some(child_schema) = properties.get(key) {
            walk(child_schema, child, &format!("{at}.{key}"), errors);
        }
    }
}

fn type_matches(expected: &str, value: &Value) -> bool {
    match expected {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        "null" => value.is_null(),
        _ => true,
    }
}

fn kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn render(allowed: &[Value]) -> String {
    allowed
        .iter()
        .take(10)
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "orderType": {"type": "string"},
                "hold": {"type": "object", "properties": {"value": {"type": "boolean"}}},
                "lines": {"type": "array", "items": {
                    "type": "object",
                    "required": ["inventoryNumber"],
                    "properties": {"inventoryNumber": {"properties": {"value": {"type": "string"}}}}
                }},
                "kind": {"enum": ["A", "B"]}
            },
            "required": ["orderType"]
        })
    }

    fn problems(value: Value) -> Vec<String> {
        check(&schema(), &value).err().unwrap_or_default()
    }

    #[test]
    fn a_conforming_body_passes() {
        assert!(check(
            &schema(),
            &json!({"orderType": "SO", "lines": [{"inventoryNumber": {"value": "X"}}]})
        )
        .is_ok());
    }

    #[test]
    fn missing_required_fields_are_named_with_their_path() {
        let p = problems(json!({"lines": [{}]}));
        assert!(p.contains(&"$.orderType: required".to_string()), "{p:?}");
        assert!(
            p.contains(&"$.lines[0].inventoryNumber: required".to_string()),
            "{p:?}"
        );
    }

    #[test]
    fn wrong_types_are_reported_at_the_nested_value() {
        let p = problems(json!({"orderType": "SO", "lines": [{"inventoryNumber": {"value": 5}}]}));
        assert_eq!(
            p,
            vec!["$.lines[0].inventoryNumber.value: expected string, got number".to_string()]
        );
    }

    #[test]
    fn unknown_field_names_are_refused_at_every_closed_level() {
        assert!(problems(json!({"orderType": "SO", "orderTyp": "SO"}))
            .contains(&"$.orderTyp: unknown field".to_string()));
        let p = problems(json!({"orderType": "SO", "hold": {"value": true, "valu": 1}}));
        assert!(
            p.contains(&"$.hold.valu: unknown field".to_string()),
            "{p:?}"
        );
    }

    #[test]
    fn enums_and_the_root_type_are_enforced() {
        assert!(problems(json!({"orderType": "SO", "kind": "C"}))
            .iter()
            .any(|e| e.starts_with("$.kind: must be one of")));
        assert_eq!(
            problems(json!(["SO"])),
            vec!["$: expected object, got array".to_string()]
        );
    }

    #[test]
    fn null_is_accepted_for_optional_properties_and_refused_for_required_ones() {
        assert!(check(&schema(), &json!({"orderType": "SO", "kind": null})).is_ok());
        let p = problems(json!({"orderType": null}));
        assert_eq!(
            p,
            vec!["$.orderType: expected string, got null".to_string()]
        );
    }

    #[test]
    fn schemas_without_properties_accept_any_keys() {
        assert!(check(&json!({"type": "object"}), &json!({"anything": [1, 2]})).is_ok());
        assert!(check(&json!({}), &json!("no type declared, so any value passes")).is_ok());
    }
}
