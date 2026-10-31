//! Pure query half of result handles (WS25-S5).
//!
//! The shape a parked result is described by, and the slices `result_query`
//! reads it through. Both the gateway's handle store (`model-gateway`'s
//! `tool_result_handles`) and execution-core's run-scoped store read through
//! these functions, so a result parked by either is queried by one set of
//! rules. Nothing here holds state or reads the clock.

use std::collections::BTreeSet;
use std::hash::{Hash as _, Hasher as _};

use serde_json::{json, Map, Value};

/// Rows returned by one `result_query` page when the caller names no limit.
pub const DEFAULT_PAGE_LIMIT: usize = 25;

/// Ceiling on one page regardless of the requested limit — the point of a
/// handle is to keep large results out of model context, so an unbounded
/// `limit` would defeat it.
pub const MAX_PAGE_LIMIT: usize = 200;

/// Longest projection-hint list carried on a handle. A wide result (hundreds
/// of columns) must not reintroduce the bloat the handle exists to avoid.
const MAX_PROJECTION_HINTS: usize = 40;

/// Characters one `result_query` read returns from a parked text result. The
/// handle keeps a long page out of model context, so each read is bounded and
/// the rest is reached by `range`.
pub const MAX_TEXT_READ_CHARS: usize = 4_000;

/// Characters of a parked text result quoted in its handle summary.
const TEXT_HEAD_CHARS: usize = 200;

/// What [`describe_payload`] derives from a payload, before it becomes a
/// handle. Split out so the derivation is unit-testable with no store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultShape {
    pub row_count: Option<usize>,
    pub projection_hints: Vec<String>,
    pub schema_id: Option<String>,
    pub summary: String,
}

/// MCP's `tools/call` result wraps the real payload in
/// `{"content":[{"type":"text","text":"..."}]}`, and servers routinely put a
/// JSON document inside that `text` string. Projecting or filtering the
/// envelope instead of the document would be useless, so unwrap one level
/// when the shape matches exactly, and re-parse the text when it is itself
/// JSON.
///
/// Returns the payload unchanged when it is not that shape — this never
/// guesses.
#[must_use]
pub fn unwrap_mcp_content(payload: &Value) -> Value {
    let Some(content) = payload.get("content").and_then(Value::as_array) else {
        return payload.clone();
    };
    // Only unwrap a single text block. Multiple blocks (or an image/resource
    // block) carry structure that discarding would lose.
    let [only] = content.as_slice() else {
        return payload.clone();
    };
    let Some(text) = only.get("text").and_then(Value::as_str) else {
        return payload.clone();
    };
    serde_json::from_str::<Value>(text.trim()).unwrap_or_else(|_| Value::String(text.to_owned()))
}

/// The row array inside a payload, if it is row-shaped: either a bare array,
/// or an object whose single array-valued field holds the rows (the common
/// `{"items": [...]}` / `{"rows": [...]}` / `{"results": [...]}` shape).
#[must_use]
pub fn rows_of(payload: &Value) -> Option<&Vec<Value>> {
    if let Some(array) = payload.as_array() {
        return Some(array);
    }
    let object = payload.as_object()?;
    let mut arrays = object.values().filter_map(Value::as_array);
    let first = arrays.next()?;
    // Two array fields is ambiguous — picking one would silently query the
    // wrong collection, so treat the payload as non-row-shaped instead.
    if arrays.next().is_some() {
        return None;
    }
    Some(first)
}

/// Derive a handle's descriptive fields from the payload.
#[must_use]
pub fn describe_payload(payload: &Value, size_bytes: usize) -> ResultShape {
    let Some(rows) = rows_of(payload) else {
        return ResultShape {
            row_count: None,
            projection_hints: Vec::new(),
            schema_id: None,
            summary: format!(
                "A {} result of {size_bytes} bytes, held complete under this handle.",
                value_kind(payload)
            ),
        };
    };
    let field_names: BTreeSet<String> = rows
        .iter()
        .filter_map(Value::as_object)
        .flat_map(|row| row.keys().cloned())
        .collect();
    let schema_id = (!field_names.is_empty()).then(|| shape_fingerprint(&field_names));
    let projection_hints: Vec<String> = field_names
        .iter()
        .take(MAX_PROJECTION_HINTS)
        .cloned()
        .collect();
    let summary = if projection_hints.is_empty() {
        format!(
            "{} rows ({size_bytes} bytes), held complete under this handle.",
            rows.len()
        )
    } else {
        format!(
            "{} rows ({size_bytes} bytes) with {} field(s), held complete under this handle.",
            rows.len(),
            field_names.len()
        )
    };
    ResultShape {
        row_count: Some(rows.len()),
        projection_hints,
        schema_id,
        summary,
    }
}

/// Derive a text handle's descriptive fields: the size in characters and lines,
/// and the start of the text, so the model can tell what the page is before it
/// reads anything.
#[must_use]
pub fn describe_text(text: &str, size_bytes: usize) -> ResultShape {
    let head: String = text.chars().take(TEXT_HEAD_CHARS).collect();
    ResultShape {
        row_count: None,
        projection_hints: Vec::new(),
        schema_id: None,
        summary: format!(
            "A text result of {} characters on {} lines ({size_bytes} bytes), held complete under this handle. It begins: {head}",
            text.chars().count(),
            text.lines().count()
        ),
    }
}

fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "empty",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "numeric",
        Value::String(_) => "text",
        Value::Array(_) => "list",
        Value::Object(_) => "structured",
    }
}

/// Stable hash of a row shape's field names. Same columns ⇒ same id, across
/// calls and across servers.
fn shape_fingerprint(field_names: &BTreeSet<String>) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for name in field_names {
        name.hash(&mut hasher);
        // Separator, so {"ab","c"} and {"a","bc"} cannot collide.
        0xffu8.hash(&mut hasher);
    }
    format!("shape_{:016x}", hasher.finish())
}

/// One `result_query` request, already parsed and validated.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HandleQuery {
    /// Field names to keep. Empty keeps every field.
    pub select: Vec<String>,
    /// Optional row filter.
    pub filter: Option<RowFilter>,
    /// Optional aggregate. When set, rows are NOT returned — only the
    /// aggregate value ("aggregate-only model visibility").
    pub aggregate: Option<Aggregate>,
    pub offset: usize,
    pub limit: usize,
    /// Character window for a text result. A row result refuses it.
    pub range: Option<TextRange>,
}

/// `length` characters starting at character `offset`. A zero length means
/// the default read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextRange {
    pub offset: usize,
    pub length: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowFilter {
    pub field: String,
    pub op: FilterOp,
    pub value: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterOp {
    Eq,
    Ne,
    Contains,
    Gt,
    Gte,
    Lt,
    Lte,
}

impl FilterOp {
    /// # Errors
    /// Returns `Err` naming every accepted operator when `raw` is not one.
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "eq" | "=" | "==" => Ok(Self::Eq),
            "ne" | "!=" => Ok(Self::Ne),
            "contains" => Ok(Self::Contains),
            "gt" | ">" => Ok(Self::Gt),
            "gte" | ">=" => Ok(Self::Gte),
            "lt" | "<" => Ok(Self::Lt),
            "lte" | "<=" => Ok(Self::Lte),
            other => Err(format!(
                "unknown filter op '{other}' — use one of: eq, ne, contains, gt, gte, lt, lte"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggregateOp {
    Count,
    Sum,
    Min,
    Max,
    Avg,
}

impl AggregateOp {
    /// # Errors
    /// Returns `Err` naming every accepted aggregate when `raw` is not one.
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "count" => Ok(Self::Count),
            "sum" => Ok(Self::Sum),
            "min" => Ok(Self::Min),
            "max" => Ok(Self::Max),
            "avg" | "mean" => Ok(Self::Avg),
            other => Err(format!(
                "unknown aggregate op '{other}' — use one of: count, sum, min, max, avg"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Aggregate {
    pub op: AggregateOp,
    /// Required for every op except `count`, which counts rows.
    pub field: Option<String>,
}

/// Run a query against a stored payload.
///
/// Order is SQL-like and fixed: **filter → aggregate → project → page**. An
/// aggregate short-circuits: it reports over the whole filtered set, never
/// over one page, so a paged aggregate cannot be mistaken for a total.
///
/// # Errors
/// Returns `Err` when the payload is not row-shaped but the query asks for a
/// row operation, or when an aggregate names no field where one is required.
pub fn apply_query(payload: &Value, query: &HandleQuery) -> Result<Value, String> {
    if query.range.is_some() {
        return Err(
            "range applies to a text result only — this result is a list of records, so page it \
             with offset and limit"
                .to_owned(),
        );
    }
    let Some(rows) = rows_of(payload) else {
        if query.aggregate.is_some() || !query.select.is_empty() || query.filter.is_some() {
            return Err(
                "this result is not a list of records, so select/where/aggregate do not apply — \
                 query it with no arguments to read the whole value"
                    .to_owned(),
            );
        }
        return Ok(payload.clone());
    };

    let filtered: Vec<&Value> = match &query.filter {
        Some(filter) => rows.iter().filter(|row| row_matches(row, filter)).collect(),
        None => rows.iter().collect(),
    };

    if let Some(aggregate) = &query.aggregate {
        return compute_aggregate(&filtered, aggregate);
    }

    let limit = if query.limit == 0 {
        DEFAULT_PAGE_LIMIT
    } else {
        query.limit.min(MAX_PAGE_LIMIT)
    };
    let page: Vec<Value> = filtered
        .iter()
        .skip(query.offset)
        .take(limit)
        .map(|row| project_row(row, &query.select))
        .collect();
    let returned = page.len();
    let matched = filtered.len();

    Ok(json!({
        "rows": page,
        "matched_rows": matched,
        "returned_rows": returned,
        "offset": query.offset,
        // Stated explicitly so a page is never mistaken for the whole set —
        // the same reason bounded_tool_output spells out "INCOMPLETE".
        "has_more": query.offset.saturating_add(returned) < matched,
    }))
}

/// Read a text result by character window. Every read is capped at
/// [`MAX_TEXT_READ_CHARS`], so a page cannot be pulled back into context whole;
/// `has_more` says whether a further range remains.
///
/// # Errors
/// Returns `Err` when the query names a row operation or a row page, which a
/// text result does not have.
pub fn apply_text_query(text: &str, query: &HandleQuery) -> Result<Value, String> {
    if !query.select.is_empty()
        || query.filter.is_some()
        || query.aggregate.is_some()
        || query.offset != 0
        || query.limit != 0
    {
        return Err(
            "this result is text, so select/where/aggregate/offset/limit do not apply — read it \
             with a range {offset, length} in characters"
                .to_owned(),
        );
    }
    let window = query.range.unwrap_or(TextRange {
        offset: 0,
        length: MAX_TEXT_READ_CHARS,
    });
    let length = match window.length {
        0 => MAX_TEXT_READ_CHARS,
        requested => requested.min(MAX_TEXT_READ_CHARS),
    };
    let total = text.chars().count();
    let slice: String = text.chars().skip(window.offset).take(length).collect();
    let returned = slice.chars().count();
    Ok(json!({
        "text": slice,
        "offset": window.offset,
        "returned_chars": returned,
        "total_chars": total,
        // Stated explicitly, as for a page of rows: a window is never mistaken
        // for the whole text.
        "has_more": window.offset.saturating_add(returned) < total,
    }))
}

/// Keep only `select`ed fields. An empty `select` keeps the row whole; a
/// field the row does not carry is simply absent, never `null` — a fabricated
/// null would read as "this record has no value there", which is a different
/// claim from "this field was not present".
fn project_row(row: &Value, select: &[String]) -> Value {
    if select.is_empty() {
        return row.clone();
    }
    let Some(object) = row.as_object() else {
        return row.clone();
    };
    let projected: Map<String, Value> = select
        .iter()
        .filter_map(|field| {
            object
                .get(field)
                .map(|value| (field.clone(), value.clone()))
        })
        .collect();
    Value::Object(projected)
}

fn row_matches(row: &Value, filter: &RowFilter) -> bool {
    let Some(actual) = row.get(&filter.field) else {
        return false;
    };
    match filter.op {
        FilterOp::Eq => scalar_eq(actual, &filter.value),
        FilterOp::Ne => !scalar_eq(actual, &filter.value),
        FilterOp::Contains => scalar_text(actual)
            .zip(scalar_text(&filter.value))
            .is_some_and(|(haystack, needle)| {
                haystack.to_lowercase().contains(&needle.to_lowercase())
            }),
        FilterOp::Gt | FilterOp::Gte | FilterOp::Lt | FilterOp::Lte => {
            compare_ordered(actual, &filter.value, filter.op)
        }
    }
}

/// Equality over scalars, case- and whitespace-insensitive for text. Objects
/// and arrays never match — a structural comparison the model did not ask for
/// would produce confident-looking nonsense.
fn scalar_eq(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::String(a), Value::String(b)) => a.trim().eq_ignore_ascii_case(b.trim()),
        // A number quoted in the filter should still match a real number —
        // models routinely quote numeric literals.
        (Value::Number(a), Value::String(b)) | (Value::String(b), Value::Number(a)) => {
            b.trim().parse::<f64>().is_ok_and(|parsed| {
                a.as_f64()
                    .is_some_and(|actual| (actual - parsed).abs() < f64::EPSILON)
            })
        }
        (Value::Number(a), Value::Number(b)) => a == b,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Null, Value::Null) => true,
        _ => false,
    }
}

fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

/// Ordered comparison, numeric when both sides are numeric, lexicographic
/// when both are text. A mixed or non-scalar pair does not match rather than
/// coercing — an unordered comparison silently deciding an order is how a
/// filter quietly returns the wrong rows.
fn compare_ordered(actual: &Value, expected: &Value, op: FilterOp) -> bool {
    let ordering = match (as_number(actual), as_number(expected)) {
        (Some(a), Some(b)) => a.partial_cmp(&b),
        _ => match (actual.as_str(), expected.as_str()) {
            (Some(a), Some(b)) => Some(a.cmp(b)),
            _ => None,
        },
    };
    let Some(ordering) = ordering else {
        return false;
    };
    match op {
        FilterOp::Gt => ordering.is_gt(),
        FilterOp::Gte => ordering.is_ge(),
        FilterOp::Lt => ordering.is_lt(),
        FilterOp::Lte => ordering.is_le(),
        FilterOp::Eq | FilterOp::Ne | FilterOp::Contains => false,
    }
}

fn as_number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        // A numeric string on either side is still a number for ordering.
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// Aggregate over the filtered set. Non-numeric values are **skipped and
/// counted**, not coerced to zero — a column with three numbers and seven
/// nulls must not report a sum as if it covered ten rows.
fn compute_aggregate(rows: &[&Value], aggregate: &Aggregate) -> Result<Value, String> {
    if aggregate.op == AggregateOp::Count {
        return Ok(json!({ "aggregate": "count", "value": rows.len() }));
    }
    let Some(field) = aggregate.field.as_deref() else {
        return Err(format!(
            "aggregate '{}' requires a 'field' — only 'count' works without one",
            aggregate_name(aggregate.op)
        ));
    };
    let numbers: Vec<f64> = rows
        .iter()
        .filter_map(|row| row.get(field))
        .filter_map(as_number)
        .collect();
    let considered = numbers.len();
    let skipped = rows.len().saturating_sub(considered);
    if numbers.is_empty() {
        return Ok(json!({
            "aggregate": aggregate_name(aggregate.op),
            "field": field,
            "value": Value::Null,
            "rows_considered": 0,
            "rows_skipped_non_numeric": skipped,
            "note": "No numeric values in this field, so no aggregate could be computed.",
        }));
    }
    let value = match aggregate.op {
        AggregateOp::Sum => numbers.iter().sum::<f64>(),
        AggregateOp::Min => numbers.iter().copied().fold(f64::INFINITY, f64::min),
        AggregateOp::Max => numbers.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        // Lossless: a row count is converted through u32 rather than cast
        // straight from usize, whose upper range f64 cannot represent exactly.
        AggregateOp::Avg => {
            let divisor = u32::try_from(considered).unwrap_or(u32::MAX);
            numbers.iter().sum::<f64>() / f64::from(divisor)
        }
        AggregateOp::Count => unreachable!("count returns above"),
    };
    Ok(json!({
        "aggregate": aggregate_name(aggregate.op),
        "field": field,
        "value": value,
        "rows_considered": considered,
        "rows_skipped_non_numeric": skipped,
    }))
}

const fn aggregate_name(op: AggregateOp) -> &'static str {
    match op {
        AggregateOp::Count => "count",
        AggregateOp::Sum => "sum",
        AggregateOp::Min => "min",
        AggregateOp::Max => "max",
        AggregateOp::Avg => "avg",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_window(offset: usize, length: usize) -> HandleQuery {
        HandleQuery {
            range: Some(TextRange { offset, length }),
            ..HandleQuery::default()
        }
    }

    fn rows() -> Value {
        json!({"items": [
            {"name": "alpha", "qty": 2},
            {"name": "beta", "qty": 5},
            {"name": "gamma", "qty": 9}
        ]})
    }

    #[test]
    fn result_query_describes_a_text_result_by_size_lines_and_head() {
        let shape = describe_text("first line\nsecond line", 23);
        assert!(shape.summary.contains("22 characters on 2 lines"), "{}", shape.summary);
        assert!(shape.summary.contains("It begins: first line"), "{}", shape.summary);
        assert_eq!(shape.row_count, None);
    }

    #[test]
    fn result_query_caps_a_text_read_and_says_whether_more_remains() {
        let text = "x".repeat(MAX_TEXT_READ_CHARS + 500);
        let value = apply_text_query(&text, &text_window(0, 10_000)).expect("a text window");
        assert_eq!(value["returned_chars"], json!(MAX_TEXT_READ_CHARS));
        assert_eq!(value["total_chars"], json!(MAX_TEXT_READ_CHARS + 500));
        assert_eq!(value["has_more"], json!(true));
    }

    #[test]
    fn result_query_counts_a_text_window_in_characters_not_bytes() {
        let text = "ééééé"; // five characters, ten bytes
        let value = apply_text_query(text, &text_window(1, 3)).expect("a text window");
        assert_eq!(value["text"], json!("ééé"));
        assert_eq!(value["returned_chars"], json!(3));
        assert_eq!(value["has_more"], json!(true));
    }

    #[test]
    fn result_query_refuses_row_operations_on_a_text_result() {
        let query = HandleQuery {
            aggregate: Some(Aggregate {
                op: AggregateOp::Count,
                field: None,
            }),
            ..text_window(0, 10)
        };
        assert!(apply_text_query("abc", &query).is_err());
    }

    #[test]
    fn result_query_refuses_a_range_on_a_list_of_records() {
        assert!(apply_query(&rows(), &text_window(0, 10)).is_err());
    }

    #[test]
    fn result_query_pages_and_filters_a_record_list_with_a_stable_shape() {
        let query = HandleQuery {
            filter: Some(RowFilter {
                field: "qty".to_owned(),
                op: FilterOp::Gt,
                value: json!(3),
            }),
            limit: 1,
            ..HandleQuery::default()
        };
        let page = apply_query(&rows(), &query).expect("a row page");
        assert_eq!(page["matched_rows"], json!(2));
        assert_eq!(page["returned_rows"], json!(1));
        assert_eq!(page["has_more"], json!(true));
        let first = describe_payload(&rows(), 80);
        let second = describe_payload(&rows(), 80);
        assert_eq!(first.schema_id, second.schema_id);
        assert_eq!(first.row_count, Some(3));
    }

    #[test]
    fn result_query_unwraps_an_mcp_text_block_carrying_json() {
        let wrapped = json!({"content": [{"type": "text", "text": "{\"ok\": true}"}]});
        assert_eq!(unwrap_mcp_content(&wrapped), json!({"ok": true}));
    }

    #[test]
    fn result_query_refuses_an_unknown_filter_op_and_lists_the_valid_ones() {
        let error = FilterOp::parse("like").expect_err("not an op");
        assert!(error.contains("eq, ne, contains"), "{error}");
    }
}
