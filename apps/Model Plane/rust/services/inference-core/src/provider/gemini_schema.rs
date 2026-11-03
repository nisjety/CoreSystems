//! The JSON Schema subset Vertex AI accepts for a function declaration's
//! `parameters` and for a structured-output `responseSchema`.
//!
//! Vertex validates a schema strictly: one keyword it does not know fails the whole
//! request, not only the tool that carries it. So the translation keeps an allowlist
//! of keywords and drops every other one. Dropping a keyword only loosens what the
//! model may produce, and the arguments are still checked where they are used
//! (model-gateway and execution-core). A schema whose meaning dropping would change
//! is refused as a whole, so the caller leaves out that one tool and counts it under
//! [`DROPPED_TOOL_TOTAL`].

use metrics::counter;
use serde_json::{Map, Value};

/// Counter of tool declarations left out because their schema cannot be expressed.
/// Its `reason` label is the closed set [`Unexpressible::as_str`] names.
pub const DROPPED_TOOL_TOTAL: &str = "inference_gemini_tool_dropped_total";

/// How deeply `$ref`s may nest before the schema counts as recursive, which Vertex
/// cannot express.
const MAX_REF_DEPTH: usize = 8;

/// The most values an `enum` may carry. A longer list is refused rather than cut,
/// since a cut list would reject values the schema accepts.
const MAX_ENUM_VALUES: usize = 100;

/// Keywords whose meaning dropping would change, so a schema that uses one is refused.
const REFUSED_KEYWORDS: &[&str] = &[
    "oneOf",
    "allOf",
    "not",
    "if",
    "prefixItems",
    "contains",
    "propertyNames",
    "unevaluatedItems",
    "unevaluatedProperties",
];

/// Why a schema cannot be expressed. The `reason` label of [`DROPPED_TOOL_TOTAL`] is
/// [`Unexpressible::as_str`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unexpressible {
    /// The text is not JSON.
    Unparseable,
    /// The root is not an object, so it cannot describe a function's arguments.
    NotAnObject,
    /// A (sub)schema names no type and nothing else settles one.
    Untyped,
    /// A `type` list names more than one non-null type.
    TypeUnion,
    /// The schema admits only null.
    NullType,
    /// A `type` name outside the JSON Schema set.
    UnknownType,
    /// A keyword whose meaning would change if it were dropped.
    Combinator,
    /// A `$ref` that does not point into this same document.
    ExternalRef,
    /// A `$ref` whose pointer resolves to nothing.
    UnresolvedRef,
    /// A `$ref` chain deeper than [`MAX_REF_DEPTH`], which is a recursive schema.
    RefDepth,
    /// An `enum` or `const` value that is not a string, or sits on a non-string type.
    EnumNotString,
    /// An `enum` with no values.
    EnumEmpty,
    /// An `enum` longer than [`MAX_ENUM_VALUES`].
    EnumTooLong,
    /// An object with no properties, which Vertex refuses.
    OpenObject,
    /// An array with no `items`, which Vertex refuses.
    ArrayWithoutItems,
}

impl Unexpressible {
    /// The closed label for [`DROPPED_TOOL_TOTAL`].
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unparseable => "unparseable",
            Self::NotAnObject => "not_object",
            Self::Untyped => "untyped",
            Self::TypeUnion => "type_union",
            Self::NullType => "null_type",
            Self::UnknownType => "unknown_type",
            Self::Combinator => "combinator",
            Self::ExternalRef => "external_ref",
            Self::UnresolvedRef => "unresolved_ref",
            Self::RefDepth => "ref_depth",
            Self::EnumNotString => "enum_not_string",
            Self::EnumEmpty => "enum_empty",
            Self::EnumTooLong => "enum_too_long",
            Self::OpenObject => "open_object",
            Self::ArrayWithoutItems => "array_without_items",
        }
    }
}

/// The JSON Schema type keywords the translation keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    String,
    Number,
    Integer,
    Boolean,
    Array,
    Object,
}

impl Kind {
    fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "string" => Some(Self::String),
            "number" => Some(Self::Number),
            "integer" => Some(Self::Integer),
            "boolean" => Some(Self::Boolean),
            "array" => Some(Self::Array),
            "object" => Some(Self::Object),
            _ => None,
        }
    }

    /// The name Vertex's `Schema.type` takes.
    fn vertex_name(self) -> &'static str {
        match self {
            Self::String => "STRING",
            Self::Number => "NUMBER",
            Self::Integer => "INTEGER",
            Self::Boolean => "BOOLEAN",
            Self::Array => "ARRAY",
            Self::Object => "OBJECT",
        }
    }
}

/// The `parameters` of a function declaration for a tool whose arguments `schema_json`
/// describes. `None` when the schema declares no argument at all: Vertex refuses an
/// object with no properties, so the no-argument tool leaves the field out. An empty
/// text is no schema, which declares no argument either.
pub fn parameters(schema_json: &str) -> Result<Option<Value>, Unexpressible> {
    if schema_json.trim().is_empty() {
        return Ok(None);
    }
    let root = parse(schema_json)?;
    if declares_no_arguments(&root) {
        return Ok(None);
    }
    let schema = node(&root, &root, 0)?;
    if schema.get("type").and_then(Value::as_str) != Some("OBJECT") {
        return Err(Unexpressible::NotAnObject);
    }
    Ok(Some(schema))
}

/// The `responseSchema` for a structured-output request. Unlike [`parameters`], a
/// schema with no fields is refused: Vertex cannot ask for an object without fields,
/// so the answer would be unconstrained.
pub fn response_schema(schema_json: &str) -> Result<Value, Unexpressible> {
    let root = parse(schema_json)?;
    node(&root, &root, 0)
}

/// Count one tool declaration left out under the reason it was refused for.
pub fn count_dropped_tool(reason: Unexpressible) {
    counter!(DROPPED_TOOL_TOTAL, "reason" => reason.as_str()).increment(1);
}

fn parse(schema_json: &str) -> Result<Value, Unexpressible> {
    serde_json::from_str(schema_json.trim()).map_err(|_| Unexpressible::Unparseable)
}

/// True for an object that names no property and no reference or branch that could
/// supply one, so it asks for no argument.
fn declares_no_arguments(root: &Value) -> bool {
    let Some(map) = root.as_object() else {
        return false;
    };
    let object_typed = map.get("type").map_or(true, |declared| {
        declared
            .as_str()
            .is_some_and(|name| name.eq_ignore_ascii_case("object"))
    });
    let no_properties = map
        .get("properties")
        .map_or(true, |properties| properties.as_object().is_some_and(Map::is_empty));
    let no_indirection = ["$ref", "anyOf"]
        .iter()
        .chain(REFUSED_KEYWORDS.iter())
        .all(|key| !map.contains_key(*key));
    object_typed && no_properties && no_indirection
}

/// The Vertex form of one JSON Schema node, with the local `$ref`s it names inlined.
fn node(schema: &Value, root: &Value, depth: usize) -> Result<Value, Unexpressible> {
    let map = schema.as_object().ok_or(Unexpressible::Untyped)?;
    if let Some(reference) = map.get("$ref") {
        return follow_ref(reference, map, root, depth);
    }
    if REFUSED_KEYWORDS.iter().any(|key| map.contains_key(*key)) {
        return Err(Unexpressible::Combinator);
    }
    if let Some(branches) = map.get("anyOf") {
        return any_of(map, branches, root, depth);
    }
    let (kind, declared_nullable) = match map.get("type") {
        Some(declared) => declared_kind(declared)?,
        None => (inferred_kind(map)?, false),
    };
    let mut out = Map::new();
    out.insert(
        "type".to_owned(),
        Value::String(kind.vertex_name().to_owned()),
    );
    if declared_nullable || map.get("nullable") == Some(&Value::Bool(true)) {
        out.insert("nullable".to_owned(), Value::Bool(true));
    }
    copy_text(map, &mut out);
    if let Some(values) = enum_values(map)? {
        if kind != Kind::String {
            return Err(Unexpressible::EnumNotString);
        }
        out.insert("enum".to_owned(), Value::Array(values));
    }
    if let Some(format) = map.get("format").and_then(Value::as_str) {
        if keeps_format(kind, format) {
            out.insert("format".to_owned(), Value::String(format.to_owned()));
        }
    }
    match kind {
        Kind::String => {
            copy_keyword(map, &mut out, "pattern", Value::is_string);
            copy_keyword(map, &mut out, "minLength", Value::is_u64);
            copy_keyword(map, &mut out, "maxLength", Value::is_u64);
        }
        Kind::Integer | Kind::Number => {
            copy_keyword(map, &mut out, "minimum", Value::is_number);
            copy_keyword(map, &mut out, "maximum", Value::is_number);
        }
        Kind::Array => {
            let items = map.get("items").ok_or(Unexpressible::ArrayWithoutItems)?;
            out.insert("items".to_owned(), node(items, root, depth)?);
            copy_keyword(map, &mut out, "minItems", Value::is_u64);
            copy_keyword(map, &mut out, "maxItems", Value::is_u64);
        }
        Kind::Object => {
            let properties = map
                .get("properties")
                .and_then(Value::as_object)
                .filter(|properties| !properties.is_empty())
                .ok_or(Unexpressible::OpenObject)?;
            let mut fields = Map::new();
            for (name, field) in properties {
                fields.insert(name.clone(), node(field, root, depth)?);
            }
            out.insert("properties".to_owned(), Value::Object(fields));
            copy_keyword(map, &mut out, "required", is_string_list);
            copy_keyword(map, &mut out, "minProperties", Value::is_u64);
            copy_keyword(map, &mut out, "maxProperties", Value::is_u64);
        }
        Kind::Boolean => {}
    }
    Ok(Value::Object(out))
}

/// A local `$ref` replaced by the schema it names. The referring node's description
/// wins over the target's. Only a fragment of this same document resolves.
fn follow_ref(
    reference: &Value,
    map: &Map<String, Value>,
    root: &Value,
    depth: usize,
) -> Result<Value, Unexpressible> {
    let pointer = reference
        .as_str()
        .and_then(|text| text.strip_prefix('#'))
        .ok_or(Unexpressible::ExternalRef)?;
    if depth >= MAX_REF_DEPTH {
        return Err(Unexpressible::RefDepth);
    }
    let target = root.pointer(pointer).ok_or(Unexpressible::UnresolvedRef)?;
    let mut out = node(target, root, depth + 1)?;
    if let (Some(description), Some(object)) = (
        map.get("description").filter(|text| text.is_string()),
        out.as_object_mut(),
    ) {
        object.insert("description".to_owned(), description.clone());
    }
    Ok(out)
}

/// A node that is one of several shapes. A null branch makes the rest nullable, and a
/// single shape left is that shape, so an optional field reads as a plain typed field.
fn any_of(
    map: &Map<String, Value>,
    branches: &Value,
    root: &Value,
    depth: usize,
) -> Result<Value, Unexpressible> {
    if ["type", "properties", "items", "enum", "const"]
        .iter()
        .any(|key| map.contains_key(*key))
    {
        return Err(Unexpressible::Combinator);
    }
    let branches = branches.as_array().ok_or(Unexpressible::Untyped)?;
    let mut nullable = map.get("nullable") == Some(&Value::Bool(true));
    let mut kept = Vec::new();
    for branch in branches {
        if is_null(branch) {
            nullable = true;
        } else {
            kept.push(node(branch, root, depth)?);
        }
    }
    let mut out = match kept.len() {
        0 => return Err(Unexpressible::NullType),
        1 => match kept.pop() {
            Some(Value::Object(only)) => only,
            _ => return Err(Unexpressible::Untyped),
        },
        _ => {
            let mut wrapper = Map::new();
            wrapper.insert("anyOf".to_owned(), Value::Array(kept));
            wrapper
        }
    };
    if nullable {
        out.insert("nullable".to_owned(), Value::Bool(true));
    }
    copy_text(map, &mut out);
    Ok(Value::Object(out))
}

/// True for a branch that admits only null.
fn is_null(branch: &Value) -> bool {
    match branch.get("type") {
        Some(Value::String(name)) => name.eq_ignore_ascii_case("null"),
        Some(Value::Array(names)) => matches!(
            names.as_slice(),
            [only] if only.as_str().is_some_and(|name| name.eq_ignore_ascii_case("null"))
        ),
        _ => false,
    }
}

/// The kind a `type` keyword names, and whether it also names null. One non-null kind
/// beside null is that kind, nullable. Two or more kinds are refused.
fn declared_kind(declared: &Value) -> Result<(Kind, bool), Unexpressible> {
    match declared {
        Value::String(name) => Ok((single_kind(name)?, false)),
        Value::Array(names) => {
            let mut nullable = false;
            let mut kinds = Vec::new();
            for name in names {
                let name = name.as_str().ok_or(Unexpressible::UnknownType)?;
                if name.eq_ignore_ascii_case("null") {
                    nullable = true;
                } else {
                    kinds.push(single_kind(name)?);
                }
            }
            match kinds.as_slice() {
                [kind] => Ok((*kind, nullable)),
                [] => Err(Unexpressible::NullType),
                _ => Err(Unexpressible::TypeUnion),
            }
        }
        _ => Err(Unexpressible::UnknownType),
    }
}

fn single_kind(name: &str) -> Result<Kind, Unexpressible> {
    if name.eq_ignore_ascii_case("null") {
        return Err(Unexpressible::NullType);
    }
    Kind::parse(name).ok_or(Unexpressible::UnknownType)
}

/// The kind of a node that names no `type`, when its other keywords settle one.
/// Anything else is untyped, and Vertex needs a type.
fn inferred_kind(map: &Map<String, Value>) -> Result<Kind, Unexpressible> {
    if map.contains_key("properties") {
        return Ok(Kind::Object);
    }
    if map.contains_key("items") {
        return Ok(Kind::Array);
    }
    let text_values = match (map.get("enum"), map.get("const")) {
        (Some(Value::Array(values)), None) => {
            !values.is_empty() && values.iter().all(Value::is_string)
        }
        (None, Some(value)) => value.is_string(),
        _ => false,
    };
    if text_values {
        Ok(Kind::String)
    } else {
        Err(Unexpressible::Untyped)
    }
}

/// The values an `enum`, or a single `const`, allows. Every value must be a string,
/// and the list must be non-empty and within [`MAX_ENUM_VALUES`].
fn enum_values(map: &Map<String, Value>) -> Result<Option<Vec<Value>>, Unexpressible> {
    let values = match (map.get("enum"), map.get("const")) {
        (None, None) => return Ok(None),
        (Some(_), Some(_)) => return Err(Unexpressible::Combinator),
        (Some(Value::Array(values)), None) => values.clone(),
        (Some(_), None) => return Err(Unexpressible::EnumNotString),
        (None, Some(value)) => vec![value.clone()],
    };
    if values.is_empty() {
        return Err(Unexpressible::EnumEmpty);
    }
    if values.len() > MAX_ENUM_VALUES {
        return Err(Unexpressible::EnumTooLong);
    }
    if !values.iter().all(Value::is_string) {
        return Err(Unexpressible::EnumNotString);
    }
    Ok(Some(values))
}

/// The `format` values Vertex takes for a type. Any other format is an annotation
/// Vertex would reject, so it is left out rather than refused.
fn keeps_format(kind: Kind, format: &str) -> bool {
    match kind {
        Kind::String => format == "date-time",
        Kind::Integer => matches!(format, "int32" | "int64"),
        Kind::Number => matches!(format, "float" | "double"),
        Kind::Boolean | Kind::Array | Kind::Object => false,
    }
}

/// The keywords a person reads, kept as they are.
fn copy_text(map: &Map<String, Value>, out: &mut Map<String, Value>) {
    for key in ["title", "description"] {
        copy_keyword(map, out, key, Value::is_string);
    }
}

/// Copy one keyword whose value passes `valid`. A keyword this kind does not take is
/// never asked for, so keywords that do not apply to a type are dropped with no effect.
fn copy_keyword(
    map: &Map<String, Value>,
    out: &mut Map<String, Value>,
    key: &str,
    valid: fn(&Value) -> bool,
) {
    if let Some(value) = map.get(key).filter(|value| valid(*value)) {
        out.insert(key.to_owned(), value.clone());
    }
}

fn is_string_list(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|names| names.iter().all(Value::is_string))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// The built-in tool schemas the execution core offers, one JSON text per tool.
    const BUILTIN_SCHEMAS: &str = include_str!("../../tests/fixtures/builtin-tool-schemas.json");

    fn expressed(schema: &str) -> Value {
        parameters(schema)
            .expect("the schema is expressible")
            .expect("the schema declares arguments")
    }

    #[test]
    fn inlines_local_ref() {
        let out = expressed(
            r##"{"type":"object","properties":{"where":{"$ref":"#/$defs/Place"}},"$defs":{"Place":{"type":"object","properties":{"city":{"type":"string"}}}}}"##,
        );
        assert_eq!(out["properties"]["where"]["type"], "OBJECT");
        assert_eq!(
            out["properties"]["where"]["properties"]["city"]["type"],
            "STRING"
        );
        assert!(out.get("$defs").is_none());
    }

    #[test]
    fn drops_additional_properties() {
        let out = expressed(
            r#"{"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"a":{"type":"string"}},"additionalProperties":false}"#,
        );
        assert!(out.get("additionalProperties").is_none());
        assert!(out.get("$schema").is_none());
        assert_eq!(out["properties"]["a"], json!({"type": "STRING"}));
    }

    #[test]
    fn const_becomes_single_enum() {
        let out = expressed(r#"{"type":"object","properties":{"kind":{"const":"invoice"}}}"#);
        assert_eq!(
            out["properties"]["kind"],
            json!({"type": "STRING", "enum": ["invoice"]})
        );
    }

    #[test]
    fn nullable_from_type_union() {
        let out = expressed(r#"{"type":"object","properties":{"note":{"type":["string","null"]}}}"#);
        assert_eq!(
            out["properties"]["note"],
            json!({"type": "STRING", "nullable": true})
        );
    }

    #[test]
    fn a_combinator_is_refused_not_loosened() {
        let error = parameters(
            r#"{"type":"object","properties":{"x":{"oneOf":[{"type":"string"},{"type":"integer"}]}}}"#,
        )
        .expect_err("oneOf cannot be kept by dropping it");
        assert_eq!(error, Unexpressible::Combinator);
    }

    #[test]
    fn external_and_unresolved_refs_are_refused() {
        let external = parameters(
            r#"{"type":"object","properties":{"x":{"$ref":"https://example.com/s.json"}}}"#,
        );
        assert_eq!(external, Err(Unexpressible::ExternalRef));
        let missing = parameters(
            r##"{"type":"object","properties":{"x":{"$ref":"#/$defs/Missing"}}}"##,
        );
        assert_eq!(missing, Err(Unexpressible::UnresolvedRef));
    }

    #[test]
    fn a_recursive_ref_is_refused_at_the_depth_bound() {
        let error = parameters(r##"{"type":"object","properties":{"next":{"$ref":"#"}}}"##);
        assert_eq!(error, Err(Unexpressible::RefDepth));
    }

    #[test]
    fn a_ref_keeps_the_referring_description() {
        let out = expressed(
            r##"{"type":"object","properties":{"where":{"$ref":"#/$defs/Place","description":"Where to go"}},"$defs":{"Place":{"type":"object","description":"A place","properties":{"city":{"type":"string"}}}}}"##,
        );
        assert_eq!(out["properties"]["where"]["description"], "Where to go");
    }

    #[test]
    fn enum_limits_and_types_are_refused_not_cut() {
        let long: Vec<String> = (0..=MAX_ENUM_VALUES).map(|n| format!("v{n}")).collect();
        let too_long = format!(
            r#"{{"type":"object","properties":{{"k":{{"type":"string","enum":{}}}}}}}"#,
            json!(long)
        );
        assert_eq!(parameters(&too_long), Err(Unexpressible::EnumTooLong));
        let numeric = r#"{"type":"object","properties":{"n":{"type":"integer","enum":[1,2]}}}"#;
        assert_eq!(parameters(numeric), Err(Unexpressible::EnumNotString));
        let empty = r#"{"type":"object","properties":{"k":{"type":"string","enum":[]}}}"#;
        assert_eq!(parameters(empty), Err(Unexpressible::EnumEmpty));
    }

    #[test]
    fn a_type_union_of_two_kinds_is_refused() {
        let error = parameters(
            r#"{"type":"object","properties":{"x":{"type":["string","integer"]}}}"#,
        );
        assert_eq!(error, Err(Unexpressible::TypeUnion));
    }

    #[test]
    fn an_open_object_and_an_array_without_items_are_refused() {
        assert_eq!(
            parameters(r#"{"type":"object","properties":{"meta":{"type":"object"}}}"#),
            Err(Unexpressible::OpenObject)
        );
        assert_eq!(
            parameters(r#"{"type":"object","properties":{"tags":{"type":"array"}}}"#),
            Err(Unexpressible::ArrayWithoutItems)
        );
    }

    #[test]
    fn unparseable_and_non_object_roots_are_refused() {
        assert_eq!(parameters("{not json"), Err(Unexpressible::Unparseable));
        assert_eq!(
            parameters(r#"{"type":"array","items":{"type":"string"}}"#),
            Err(Unexpressible::NotAnObject)
        );
    }

    #[test]
    fn a_schema_with_no_arguments_leaves_parameters_out() {
        assert_eq!(parameters(""), Ok(None));
        assert_eq!(parameters("{}"), Ok(None));
        assert_eq!(parameters(r#"{"type":"object","properties":{}}"#), Ok(None));
    }

    #[test]
    fn a_response_schema_with_no_fields_is_refused() {
        assert_eq!(
            response_schema(r#"{"type":"object"}"#),
            Err(Unexpressible::OpenObject)
        );
        assert_eq!(
            response_schema(r#"{"type":"object","properties":{"ok":{"type":"boolean"}}}"#)
                .expect("expressible"),
            json!({"type": "OBJECT", "properties": {"ok": {"type": "BOOLEAN"}}})
        );
    }

    #[test]
    fn an_optional_branch_collapses_to_a_nullable_field() {
        let out = expressed(
            r#"{"type":"object","properties":{"x":{"anyOf":[{"type":"string"},{"type":"null"}]}}}"#,
        );
        assert_eq!(out["properties"]["x"], json!({"type": "STRING", "nullable": true}));
    }

    #[test]
    fn formats_outside_vertex_are_dropped_and_keywords_of_other_types_are_ignored() {
        let out = expressed(
            r#"{"type":"object","properties":{"when":{"type":"string","format":"date-time"},"mail":{"type":"string","format":"email"},"n":{"type":"string","minimum":3,"maxLength":5}}}"#,
        );
        assert_eq!(out["properties"]["when"]["format"], "date-time");
        assert!(out["properties"]["mail"].get("format").is_none());
        assert_eq!(out["properties"]["n"], json!({"type": "STRING", "maxLength": 5}));
    }

    #[test]
    fn sanitiser_is_idempotent() {
        let complex = r##"{"type":"object","properties":{"where":{"$ref":"#/$defs/Place","description":"Where"},"kind":{"const":"a"},"tags":{"type":"array","items":{"type":["string","null"]}}},"required":["kind"],"$defs":{"Place":{"type":"object","properties":{"city":{"type":"string","format":"date-time"}}}}}"##;
        let mut schemas: Vec<String> =
            serde_json::from_str(BUILTIN_SCHEMAS).expect("the fixture is a JSON array of texts");
        schemas.push(complex.to_owned());
        for schema in &schemas {
            // A built-in schema the sanitiser refuses has no output to compare; the golden test
            // below counts those.
            if let Ok(Some(first)) = parameters(schema) {
                let second = expressed(&first.to_string());
                assert_eq!(second, first, "{schema}");
            }
        }
        // The check must reach the constructed schema, so it cannot pass by skipping everything.
        assert!(expressed(complex).is_object());
    }

    #[test]
    fn only_the_free_form_builtin_tools_are_left_out() {
        // Three built-in tools take free-form arguments, an object with no properties that Vertex
        // has no closed form for, and two take a value with no type. Those five are the only
        // built-in tools a Gemini request leaves out. Any other refusal is a regression in the
        // sanitiser or in a tool schema.
        let schemas: Vec<String> =
            serde_json::from_str(BUILTIN_SCHEMAS).expect("the fixture is a JSON array of texts");
        assert!(schemas.len() > 40, "the fixture lists the built-in tools");
        let refusals: Vec<Unexpressible> = schemas
            .iter()
            .filter_map(|schema| parameters(schema).err())
            .collect();
        let open_objects = refusals
            .iter()
            .filter(|reason| **reason == Unexpressible::OpenObject)
            .count();
        let untyped = refusals
            .iter()
            .filter(|reason| **reason == Unexpressible::Untyped)
            .count();
        assert_eq!((open_objects, untyped, refusals.len()), (3, 2, 5));
    }
}
