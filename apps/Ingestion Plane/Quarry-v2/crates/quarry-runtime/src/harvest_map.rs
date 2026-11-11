//! Harvest map: what a page's own markup already states, put to two uses by the
//! AI format engine.
//!
//! - [`render_harvest_block`] renders the harvested facts as a bounded block for
//!   the untrusted page section of a model prompt: labelled figures with their
//!   unit and period, the JSON-LD scalar fields, and the first table's headers.
//!   The model then sees facts that exist only in hydration state, which
//!   readability strips with the rest of the script markup.
//! - [`prefill`] takes a value for a top-level primitive schema property when a
//!   depth-1 key in the markup carries that property's name, so the model is
//!   never asked for something the page already states. [`merge`] puts the
//!   prefilled values back over the model's output.
//!
//! The match is exact after folding case and underscores. A property stays with
//! the model when no key matches, when the matching value is not type-compatible
//! or outside the schema's `enum`, or when two matching keys disagree.
//!
//! Everything here is pure: no I/O, no logging, nothing stored or forwarded.
//! Every value comes from the page and is untrusted data. The switch that decides
//! whether a caller uses this module lives with the caller
//! (`QUARRY_EDGE__AI_FORMATS_HARVEST`).

use quarry_transform::json_ld::JsonLdSummary;
use quarry_transform::structured::{BlobSource, StructuredData};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};

/// Opens the block. The same heading the answer path puts over its figure block,
/// so both channels read the same to the model.
const BLOCK_HEADER: &str = "\n\n[Structured data extracted from this page]";

/// Longest string a prefill takes from the page. A longer value is prose rather
/// than an exact fact, so the model writes it instead.
const MAX_PREFILL_STRING_CHARS: usize = 2_000;

/// 2^53, the largest magnitude at which every integer is exact in an `f64`.
const MAX_EXACT_INTEGER: f64 = 9_007_199_254_740_992.0;

/// Which harvested channel supplied a prefilled value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrefillSource {
    /// A depth-1 key of one schema.org JSON-LD node.
    JsonLd,
    /// A microdata or RDFa property name and its value.
    Microdata,
    /// A depth-1 key of a framework hydration or state payload, either a JSON
    /// script or a `window` state assignment.
    Hydration,
}

/// The rule that admitted a prefilled value. Exact name is the only rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrefillRule {
    /// The page key equals the property name after case and underscore folding.
    ExactName,
}

/// One prefilled field and where its value came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrefillProvenance {
    /// The top-level schema property that was filled.
    pub field: String,
    /// The harvested channel that held the value.
    pub source: PrefillSource,
    /// The rule that admitted the value.
    pub rule: PrefillRule,
}

/// The values a page's markup states for top-level schema properties, and the
/// schema the model still has to fill.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Prefill {
    /// Prefilled `field -> value` pairs, each one type-checked against the schema.
    pub values: Map<String, Value>,
    /// One entry per key of `values`, in schema property order.
    pub provenance: Vec<PrefillProvenance>,
    /// The schema without the prefilled names in `properties` and `required`.
    /// When nothing is left to fill, the caller makes no model call.
    pub remainder_schema: Value,
}

/// Render the harvested facts as a block for the untrusted page section of a
/// prompt.
///
/// The block holds the labelled figures (each with its unit and period), the
/// JSON-LD scalar fields, and the headers of the first table that has any.
/// Returns an empty string when there is nothing to show. Otherwise the result is
/// at most `cap_chars` characters. A line that would overflow the cap is left out
/// whole, so a figure is never shown cut in half.
pub fn render_harvest_block(
    data: &StructuredData,
    ld: Option<&JsonLdSummary>,
    cap_chars: usize,
) -> String {
    let mut lines: Vec<String> = Vec::new();

    for fig in &data.figures {
        let label = single_line(&fig.label);
        let value = single_line(&fig.value);
        if label.is_empty() || value.is_empty() {
            continue;
        }
        let unit = fig
            .unit
            .as_deref()
            .map(single_line)
            .filter(|u| !u.is_empty());
        let period = fig
            .period
            .as_deref()
            .map(single_line)
            .filter(|p| !p.is_empty());
        let unit = unit.map(|u| format!(" {u}")).unwrap_or_default();
        let period = period.map(|p| format!(" ({p})")).unwrap_or_default();
        lines.push(format!("- {label}: {value}{unit}{period}"));
    }

    if let Some(ld) = ld {
        // `image` is left out: a URL is not a fact the model can quote.
        let scalars = [
            ("headline", ld.headline.as_deref()),
            ("description", ld.description.as_deref()),
            ("author", ld.author.as_deref()),
            ("datePublished", ld.date_published.as_deref()),
            ("dateModified", ld.date_modified.as_deref()),
            ("publisher", ld.publisher.as_deref()),
            ("sku", ld.sku.as_deref()),
            ("price", ld.price.as_deref()),
            ("availability", ld.availability.as_deref()),
        ];
        for (name, value) in scalars {
            let value = value.map(single_line).unwrap_or_default();
            if !value.is_empty() {
                lines.push(format!("- JSON-LD {name}: {value}"));
            }
        }
    }

    if let Some(table) = data.tables.iter().find(|t| !t.headers.is_empty()) {
        let headers: Vec<String> = table
            .headers
            .iter()
            .map(|h| single_line(h))
            .filter(|h| !h.is_empty())
            .collect();
        if !headers.is_empty() {
            lines.push(format!("- table headers: {}", headers.join(" | ")));
        }
    }

    // Whole lines only. The header counts against the cap as well, so the block
    // is bounded as a whole.
    let header_chars = BLOCK_HEADER.chars().count();
    let mut body = String::new();
    for line in &lines {
        let line = format!("\n{line}");
        let used = header_chars + body.chars().count();
        if used + line.chars().count() <= cap_chars {
            body.push_str(&line);
        }
    }
    if body.is_empty() {
        return String::new();
    }
    format!("{BLOCK_HEADER}{body}")
}

/// Take the values the page's markup states for top-level primitive properties of
/// `schema`, and report the schema the model still has to fill.
///
/// Only `string`, `number`, `integer` and `boolean` properties are considered.
/// A property is prefilled when a depth-1 key in the markup folds to its name, the
/// value is type-compatible with it, and the value is one of its `enum` values
/// when it has an enum. The candidates come from JSON-LD nodes, then microdata
/// items, then hydration payloads, and that order decides the provenance. A
/// property whose candidates disagree on the value is not prefilled.
pub fn prefill(schema: &Value, data: &StructuredData, ld: Option<&JsonLdSummary>) -> Prefill {
    let mut values = Map::new();
    let mut provenance = Vec::new();

    if let Some(props) = schema.get("properties").and_then(Value::as_object) {
        for (name, sub) in props {
            let Some(ty) = primitive_type(sub) else {
                continue;
            };
            let options: Option<&[Value]> = match sub.get("enum") {
                None => None,
                Some(Value::Array(opts)) => Some(opts.as_slice()),
                // A malformed `enum` admits nothing, so the field stays with the model.
                Some(_) => continue,
            };
            let found = candidates(&fold(name), ty, options, data, ld);
            let Some((source, value)) = found.first().cloned() else {
                continue;
            };
            // Two different page values for one name is not an exact match.
            if found.iter().any(|(_, other)| !same_value(other, &value)) {
                continue;
            }
            values.insert(name.clone(), value);
            provenance.push(PrefillProvenance {
                field: name.clone(),
                source,
                rule: PrefillRule::ExactName,
            });
        }
    }

    let remainder_schema = remainder(schema, &values);
    Prefill {
        values,
        provenance,
        remainder_schema,
    }
}

/// Merge the prefilled values over the model's output.
///
/// A prefilled value replaces the same key from the model, since the page's own
/// markup is the stronger source. A model output that is not an object is replaced
/// by the prefilled values. With nothing prefilled, the model output is returned
/// unchanged. The caller still validates the result against the full original
/// schema.
pub fn merge(prefill: &Prefill, model_output: Value) -> Value {
    if prefill.values.is_empty() {
        return model_output;
    }
    let mut merged = match model_output {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    for (field, value) in &prefill.values {
        merged.insert(field.clone(), value.clone());
    }
    Value::Object(merged)
}

/// The type of a schema property, when it is a primitive a prefill may take.
fn primitive_type(schema: &Value) -> Option<&str> {
    let ty = schema.get("type")?.as_str()?;
    matches!(ty, "string" | "number" | "integer" | "boolean").then_some(ty)
}

/// Folds a schema property name or a page key into the form the two are compared
/// in: lowercase, with underscores dropped. `datePublished`, `date_published` and
/// `DATE_PUBLISHED` all fold to `datepublished`.
fn fold(name: &str) -> String {
    name.chars()
        .filter(|c| *c != '_')
        .flat_map(char::to_lowercase)
        .collect()
}

/// The depth-1 candidates for one folded property name, in source order. Each one
/// is already type-checked and inside the property's `enum`.
fn candidates(
    folded: &str,
    ty: &str,
    options: Option<&[Value]>,
    data: &StructuredData,
    ld: Option<&JsonLdSummary>,
) -> Vec<(PrefillSource, Value)> {
    let mut found = Vec::new();

    if let Some(ld) = ld {
        for node in &ld.raw {
            if let Some(map) = node.as_object() {
                push_matches(&mut found, PrefillSource::JsonLd, map, folded, ty, options);
            }
        }
    }

    // The harvester flattens a nested itemscope into its enclosing item, so a
    // microdata pair here may sit deeper than the top level. A disagreeing
    // duplicate blocks the prefill; a single agreeing value is accepted.
    for item in &data.microdata {
        for (name, text) in &item.properties {
            if fold(name) != folded {
                continue;
            }
            if let Some(value) = accept(&Value::String(text.clone()), ty, options) {
                found.push((PrefillSource::Microdata, value));
            }
        }
    }

    for blob in &data.blobs {
        if !matches!(blob.source, BlobSource::Hydration | BlobSource::Assignment) {
            continue;
        }
        if let Some(map) = blob.json.as_object() {
            push_matches(
                &mut found,
                PrefillSource::Hydration,
                map,
                folded,
                ty,
                options,
            );
        }
    }

    found
}

/// Pushes the values under the keys of one object that fold to `folded`. A key
/// that starts with `@` is a JSON-LD keyword and never a property name.
fn push_matches(
    found: &mut Vec<(PrefillSource, Value)>,
    source: PrefillSource,
    map: &Map<String, Value>,
    folded: &str,
    ty: &str,
    options: Option<&[Value]>,
) {
    for (key, value) in map {
        if key.starts_with('@') || fold(key) != folded {
            continue;
        }
        if let Some(value) = accept(value, ty, options) {
            found.push((source, value));
        }
    }
}

/// The coerced value when `value` is type-compatible with `ty` and inside the
/// property's `enum`, if it has one.
fn accept(value: &Value, ty: &str, options: Option<&[Value]>) -> Option<Value> {
    let coerced = coerce(value, ty)?;
    admitted(options, &coerced).then_some(coerced)
}

/// Type check and coercion for one page value.
///
/// A string is trimmed and must be non-empty. Only numeric strings coerce, and only
/// for `number` and `integer`. Objects and arrays are never compatible.
fn coerce(value: &Value, ty: &str) -> Option<Value> {
    match (ty, value) {
        ("string", Value::String(s)) => {
            let t = s.trim();
            (!t.is_empty() && t.chars().count() <= MAX_PREFILL_STRING_CHARS)
                .then(|| Value::String(t.to_string()))
        }
        ("boolean", Value::Bool(b)) => Some(Value::Bool(*b)),
        ("number", Value::Number(n)) => Some(Value::Number(n.clone())),
        ("number", Value::String(s)) => {
            Number::from_f64(s.trim().parse::<f64>().ok()?).map(Value::Number)
        }
        ("integer", Value::Number(n)) => integer_number(n).map(Value::Number),
        ("integer", Value::String(s)) => {
            let t = s.trim();
            match t.parse::<i64>() {
                Ok(i) => Some(Value::Number(Number::from(i))),
                Err(_) => {
                    let f = t.parse::<f64>().ok()?;
                    integer_from_f64(f).map(|i| Value::Number(Number::from(i)))
                }
            }
        }
        _ => None,
    }
}

/// An integer-valued JSON number, kept as it is when it already is an integer.
fn integer_number(n: &Number) -> Option<Number> {
    if n.is_i64() || n.is_u64() {
        return Some(n.clone());
    }
    integer_from_f64(n.as_f64()?).map(Number::from)
}

/// The integer an `f64` holds exactly, when it holds one within the exact range.
fn integer_from_f64(f: f64) -> Option<i64> {
    (f.fract() == 0.0 && f.abs() <= MAX_EXACT_INTEGER).then_some(f as i64)
}

/// JSON equality, with numbers compared as numbers, so `1` equals `1.0`.
fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        _ => a == b,
    }
}

/// Whether `candidate` is one of the `enum` values, or the property has no enum.
fn admitted(options: Option<&[Value]>, candidate: &Value) -> bool {
    match options {
        None => true,
        Some(opts) => opts.iter().any(|o| same_value(o, candidate)),
    }
}

/// The schema with the prefilled names removed from `properties` and `required`.
fn remainder(schema: &Value, prefilled: &Map<String, Value>) -> Value {
    let mut out = schema.clone();
    if let Some(obj) = out.as_object_mut() {
        if let Some(Value::Object(props)) = obj.get_mut("properties") {
            for name in prefilled.keys() {
                props.remove(name);
            }
        }
        if let Some(Value::Array(required)) = obj.get_mut("required") {
            required.retain(|r| r.as_str().is_none_or(|s| !prefilled.contains_key(s)));
        }
    }
    out
}

/// The text with control characters turned into spaces, so a page value cannot
/// start a new line inside the block.
fn single_line(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}
