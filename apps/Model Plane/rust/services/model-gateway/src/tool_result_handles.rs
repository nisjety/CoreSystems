//! Tool result handles — `MODEL_PLANE_IMPROVEMENTS_2026.md` §23.6.
//!
//! ## Why this exists
//!
//! Before this module, a tool result larger than `MAX_TOOL_OUTPUT_CHARS` was
//! **truncated** into the model's context with a note saying so
//! (`tool_loop::bounded_tool_output`). That note is honest, but it leaves the
//! model with only two options: re-run the call with a narrower request (a
//! whole extra round-trip, and often it cannot narrow it — the upstream tool
//! may have no filter parameter), or answer from a partial result set.
//!
//! A handle replaces the truncated blob with a small, complete *description*
//! of the result — how many rows, what fields, how big, what it is — plus an
//! id the model can query. The full payload stays in the gateway, out of
//! model context, and the model pulls exactly the slice it needs via
//! `result_query` (projection, filter, pagination, or aggregate-only).
//!
//! ## Scope, honestly
//!
//! §23.6 lists six capabilities. Four are implemented here: field projection,
//! filtering, runtime pagination, and aggregate-only model visibility. Two are
//! **not**: direct tool-to-tool transfer (passing a handle as another tool's
//! argument without materializing it) and artifact-to-tool transfer. What
//! *is* wired is handle-to-artifact materialization — `result_query`'s
//! `as_artifact` reuses the existing authored-artifact envelope so the user
//! can download the full or filtered result — which is what populates
//! [`ToolResultHandle::artifact_ref`].
//!
//! ## Isolation and retention
//!
//! Handles are keyed by `(org_id, user_id, handle_id)`, so a handle id
//! observed in one tenant — or by a different user in the same tenant —
//! simply does not resolve. This is the same discipline `ownership.rs`
//! applies to MCP servers, applied to result data.
//!
//! Under **ZDR the caller must not create a handle at all** (see
//! `tool_loop`'s dispatch path): a handle would hold result content in
//! gateway memory past the turn that produced it, which is exactly the
//! retention a ZDR turn promises not to do. ZDR keeps the pre-existing
//! inline-truncation behavior instead.

use std::collections::BTreeSet;
use std::hash::{Hash as _, Hasher as _};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use serde_json::{json, Map, Value};

/// How long a handle stays resolvable. Long enough for a multi-round tool
/// loop to page through a result, short enough that gateway memory is not a
/// long-lived store of tenant data.
pub const HANDLE_TTL: Duration = Duration::from_secs(900);

/// Hard cap on live handles per process. A runaway loop must not turn the
/// gateway into an unbounded cache; the oldest entries are evicted first.
const MAX_LIVE_HANDLES: usize = 512;

/// Rows returned by one `result_query` page when the caller names no limit.
pub const DEFAULT_PAGE_LIMIT: usize = 25;

/// Ceiling on one page regardless of the requested limit — the point of a
/// handle is to keep large results out of model context, so an unbounded
/// `limit` would defeat it.
pub const MAX_PAGE_LIMIT: usize = 200;

/// Longest projection-hint list carried on a handle. A wide result (hundreds
/// of columns) must not reintroduce the bloat the handle exists to avoid.
const MAX_PROJECTION_HINTS: usize = 40;

/// §23.6's `ToolResultHandle`, field-for-field.
///
/// Every field is derived from the payload itself — nothing here is a claim
/// the gateway cannot substantiate from the bytes it received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResultHandle {
    /// Opaque id the model passes back to `result_query`.
    pub handle_id: String,
    /// The qualified tool name that produced this result (e.g.
    /// `mcp__srv__execute_query`), so a later query is traceable to its
    /// source without the model having to restate it.
    pub capability_id: String,
    /// Set once the result has been materialized as a downloadable artifact
    /// (`result_query` with `as_artifact`), `None` until then.
    pub artifact_ref: Option<String>,
    /// A **content-derived shape fingerprint** — the hash of the sorted union
    /// of row field names. Two results with the same columns share a
    /// `schema_id`. Deliberately NOT a §23.3 normalized-schema registry id;
    /// that registry does not exist yet, and reusing the name for a hash
    /// would overclaim.
    pub schema_id: Option<String>,
    /// Number of rows, when the payload is row-shaped. `None` for a scalar or
    /// a non-row object — absent, never a fabricated `1`.
    pub row_count: Option<usize>,
    /// Size of the payload as received, in bytes.
    pub size_bytes: usize,
    /// One line describing what this is, for the model to reason about
    /// without fetching anything.
    pub summary: String,
    /// Field names available for `select` — the union of keys across the
    /// sampled rows, capped.
    pub projection_hints: Vec<String>,
    /// RFC3339 instant after which this handle no longer resolves.
    pub expires_at: Option<String>,
}

impl ToolResultHandle {
    /// The wire form handed to the model in place of the oversized result.
    /// Names the next action explicitly — a handle the model does not know
    /// how to use is worse than a truncated blob, because a blob at least
    /// carries data.
    #[must_use]
    pub fn to_model_json(&self) -> String {
        let mut envelope = Map::new();
        envelope.insert("handle_id".to_owned(), json!(self.handle_id));
        envelope.insert("capability_id".to_owned(), json!(self.capability_id));
        envelope.insert("size_bytes".to_owned(), json!(self.size_bytes));
        envelope.insert("summary".to_owned(), json!(self.summary));
        if let Some(row_count) = self.row_count {
            envelope.insert("row_count".to_owned(), json!(row_count));
        }
        if let Some(schema_id) = &self.schema_id {
            envelope.insert("schema_id".to_owned(), json!(schema_id));
        }
        if let Some(artifact_ref) = &self.artifact_ref {
            envelope.insert("artifact_ref".to_owned(), json!(artifact_ref));
        }
        if let Some(expires_at) = &self.expires_at {
            envelope.insert("expires_at".to_owned(), json!(expires_at));
        }
        if !self.projection_hints.is_empty() {
            envelope.insert("projection_hints".to_owned(), json!(self.projection_hints));
        }
        envelope.insert(
            "note".to_owned(),
            json!(
                "The COMPLETE result is held under this handle — it was not truncated. Use the \
                 result_query tool with this handle_id to read it: `select` to pick fields, \
                 `where` to filter, `offset`/`limit` to page, or `aggregate` for a count/sum/\
                 min/max/avg without reading rows."
            ),
        );
        Value::Object(envelope).to_string()
    }
}

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

/// A stored result, plus the metadata a later query needs.
#[derive(Clone)]
struct StoredResult {
    stored_at: Instant,
    capability_id: String,
    payload: Value,
    size_bytes: usize,
    artifact_ref: Option<String>,
}

/// Live handles, keyed by `(org_id, user_id, handle_id)`.
///
/// The user component is not decoration: two users in one org must not be
/// able to read each other's tool results by guessing or replaying a handle
/// id, exactly as `ownership.rs` prevents for the servers themselves.
#[derive(Clone, Default)]
pub struct ToolResultStore {
    inner: Arc<DashMap<(String, String, String), StoredResult>>,
}

impl ToolResultStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Store `payload` and return the handle describing it.
    ///
    /// `now_rfc3339` is passed in rather than read here so the expiry stamp
    /// is testable without a clock.
    pub fn insert(
        &self,
        org_id: &str,
        user_id: &str,
        capability_id: &str,
        payload: Value,
        size_bytes: usize,
        expires_at: Option<String>,
    ) -> ToolResultHandle {
        self.prune();
        let handle_id = format!("res_{}", mp_ids::new_ulid());
        let shape = describe_payload(&payload, size_bytes);
        self.inner.insert(
            (org_id.to_owned(), user_id.to_owned(), handle_id.clone()),
            StoredResult {
                stored_at: Instant::now(),
                capability_id: capability_id.to_owned(),
                payload,
                size_bytes,
                artifact_ref: None,
            },
        );
        ToolResultHandle {
            handle_id,
            capability_id: capability_id.to_owned(),
            artifact_ref: None,
            schema_id: shape.schema_id,
            row_count: shape.row_count,
            size_bytes,
            summary: shape.summary,
            projection_hints: shape.projection_hints,
            expires_at,
        }
    }

    /// Resolve a handle for this exact `(org, user)`. Returns `None` for an
    /// unknown id, another tenant's or user's id, or an expired one — all
    /// indistinguishable to the caller by design.
    #[must_use]
    pub fn resolve(&self, org_id: &str, user_id: &str, handle_id: &str) -> Option<ResolvedResult> {
        let key = (org_id.to_owned(), user_id.to_owned(), handle_id.to_owned());
        let entry = self.inner.get(&key)?;
        if entry.stored_at.elapsed() > HANDLE_TTL {
            return None;
        }
        Some(ResolvedResult {
            capability_id: entry.capability_id.clone(),
            payload: entry.payload.clone(),
            size_bytes: entry.size_bytes,
            artifact_ref: entry.artifact_ref.clone(),
        })
    }

    /// Record that this handle's result has been materialized as `artifact_ref`.
    /// Returns false when the handle does not resolve for this `(org, user)`.
    pub fn attach_artifact(
        &self,
        org_id: &str,
        user_id: &str,
        handle_id: &str,
        artifact_ref: &str,
    ) -> bool {
        let key = (org_id.to_owned(), user_id.to_owned(), handle_id.to_owned());
        let Some(mut entry) = self.inner.get_mut(&key) else {
            return false;
        };
        if entry.stored_at.elapsed() > HANDLE_TTL {
            return false;
        }
        entry.artifact_ref = Some(artifact_ref.to_owned());
        true
    }

    /// Number of live (non-expired) handles. Test/observability helper.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner
            .iter()
            .filter(|entry| entry.stored_at.elapsed() <= HANDLE_TTL)
            .count()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Drop expired entries, then the oldest survivors if still over cap.
    fn prune(&self) {
        self.inner
            .retain(|_, stored| stored.stored_at.elapsed() <= HANDLE_TTL);
        let over = self.inner.len().saturating_sub(MAX_LIVE_HANDLES);
        if over == 0 {
            return;
        }
        let mut ages: Vec<((String, String, String), Instant)> = self
            .inner
            .iter()
            .map(|entry| (entry.key().clone(), entry.stored_at))
            .collect();
        ages.sort_by_key(|(_, stored_at)| *stored_at);
        for (key, _) in ages.into_iter().take(over) {
            self.inner.remove(&key);
        }
    }
}

/// A resolved handle's contents.
pub struct ResolvedResult {
    pub capability_id: String,
    pub payload: Value,
    pub size_bytes: usize,
    pub artifact_ref: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows_payload() -> Value {
        json!([
            {"id": 1, "name": "Alpha", "amount": 100, "status": "open"},
            {"id": 2, "name": "Beta", "amount": 250, "status": "closed"},
            {"id": 3, "name": "Gamma", "amount": 50, "status": "open"},
        ])
    }

    #[test]
    fn unwraps_a_single_text_content_block_carrying_json() {
        let envelope = json!({"content": [{"type": "text", "text": "[{\"a\":1}]"}]});
        assert_eq!(unwrap_mcp_content(&envelope), json!([{"a": 1}]));
    }

    #[test]
    fn unwraps_plain_text_content_to_a_string_not_a_parse_failure() {
        let envelope = json!({"content": [{"type": "text", "text": "not json"}]});
        assert_eq!(unwrap_mcp_content(&envelope), json!("not json"));
    }

    #[test]
    fn leaves_multi_block_and_non_mcp_payloads_untouched() {
        // Two blocks carry structure that unwrapping one would discard.
        let multi = json!({"content": [{"text": "a"}, {"text": "b"}]});
        assert_eq!(unwrap_mcp_content(&multi), multi);
        let plain = json!({"rows": [1, 2]});
        assert_eq!(unwrap_mcp_content(&plain), plain);
    }

    #[test]
    fn rows_of_finds_bare_arrays_and_single_wrapped_arrays() {
        assert_eq!(rows_of(&json!([1, 2, 3])).map(Vec::len), Some(3));
        assert_eq!(rows_of(&json!({"items": [1, 2]})).map(Vec::len), Some(2));
        // Ambiguous: two array fields, so querying either would be a guess.
        assert_eq!(rows_of(&json!({"a": [1], "b": [2]})), None);
        assert_eq!(rows_of(&json!({"total": 4})), None);
    }

    #[test]
    fn describe_reports_row_count_fields_and_a_stable_shape_id() {
        let shape = describe_payload(&rows_payload(), 512);
        assert_eq!(shape.row_count, Some(3));
        assert_eq!(
            shape.projection_hints,
            vec!["amount", "id", "name", "status"]
        );
        assert!(shape.summary.contains("3 rows"));

        // Same columns, different data and size ⇒ same fingerprint.
        let same_shape = describe_payload(
            &json!([{"id": 9, "name": "Z", "amount": 1, "status": "open"}]),
            64,
        );
        assert_eq!(shape.schema_id, same_shape.schema_id);

        // Different columns ⇒ different fingerprint.
        let other = describe_payload(&json!([{"id": 1, "other": 2}]), 64);
        assert_ne!(shape.schema_id, other.schema_id);
    }

    #[test]
    fn describe_never_invents_a_row_count_for_a_scalar() {
        let shape = describe_payload(&json!({"total": 42}), 12);
        assert_eq!(shape.row_count, None);
        assert_eq!(shape.schema_id, None);
        assert!(shape.projection_hints.is_empty());
    }

    #[test]
    fn query_pages_and_reports_whether_more_remains() {
        let query = HandleQuery {
            limit: 2,
            ..HandleQuery::default()
        };
        let result = apply_query(&rows_payload(), &query).expect("row-shaped");
        assert_eq!(result["returned_rows"], 2);
        assert_eq!(result["matched_rows"], 3);
        assert_eq!(result["has_more"], true);

        let rest = apply_query(
            &rows_payload(),
            &HandleQuery {
                offset: 2,
                limit: 2,
                ..HandleQuery::default()
            },
        )
        .expect("row-shaped");
        assert_eq!(rest["returned_rows"], 1);
        assert_eq!(rest["has_more"], false);
    }

    #[test]
    fn query_projects_only_selected_fields_and_omits_absent_ones() {
        let query = HandleQuery {
            select: vec!["name".to_owned(), "nonexistent".to_owned()],
            ..HandleQuery::default()
        };
        let result = apply_query(&rows_payload(), &query).expect("row-shaped");
        let first = &result["rows"][0];
        assert_eq!(first["name"], "Alpha");
        // Absent, not null — "not present" and "present but empty" are
        // different claims about the record.
        assert!(first.get("nonexistent").is_none());
        assert!(first.get("amount").is_none());
    }

    #[test]
    fn query_filters_by_equality_case_insensitively() {
        let query = HandleQuery {
            filter: Some(RowFilter {
                field: "status".to_owned(),
                op: FilterOp::Eq,
                value: json!("OPEN"),
            }),
            ..HandleQuery::default()
        };
        let result = apply_query(&rows_payload(), &query).expect("row-shaped");
        assert_eq!(result["matched_rows"], 2);
    }

    #[test]
    fn query_filters_numerically_including_quoted_numbers() {
        let query = HandleQuery {
            filter: Some(RowFilter {
                field: "amount".to_owned(),
                op: FilterOp::Gt,
                // A model quoting its numeric literal must still work.
                value: json!("99"),
            }),
            ..HandleQuery::default()
        };
        let result = apply_query(&rows_payload(), &query).expect("row-shaped");
        assert_eq!(result["matched_rows"], 2);
    }

    #[test]
    fn a_row_missing_the_filtered_field_never_matches() {
        let payload = json!([{"id": 1}, {"id": 2, "status": "open"}]);
        let query = HandleQuery {
            filter: Some(RowFilter {
                field: "status".to_owned(),
                op: FilterOp::Ne,
                value: json!("closed"),
            }),
            ..HandleQuery::default()
        };
        let result = apply_query(&payload, &query).expect("row-shaped");
        // The row with no `status` is absent, not "not closed" — treating a
        // missing field as satisfying `ne` would invent matches.
        assert_eq!(result["matched_rows"], 1);
    }

    #[test]
    fn aggregate_reports_over_the_whole_filtered_set_not_one_page() {
        let query = HandleQuery {
            aggregate: Some(Aggregate {
                op: AggregateOp::Sum,
                field: Some("amount".to_owned()),
            }),
            limit: 1,
            ..HandleQuery::default()
        };
        let result = apply_query(&rows_payload(), &query).expect("row-shaped");
        assert_eq!(result["value"], 400.0);
        assert_eq!(result["rows_considered"], 3);
        // Aggregate-only: no rows travel back into model context.
        assert!(result.get("rows").is_none());
    }

    #[test]
    fn aggregate_skips_non_numeric_values_and_says_how_many() {
        let payload = json!([{"n": 10}, {"n": "abc"}, {"n": null}, {"n": 5}]);
        let query = HandleQuery {
            aggregate: Some(Aggregate {
                op: AggregateOp::Avg,
                field: Some("n".to_owned()),
            }),
            ..HandleQuery::default()
        };
        let result = apply_query(&payload, &query).expect("row-shaped");
        assert_eq!(result["value"], 7.5);
        assert_eq!(result["rows_considered"], 2);
        assert_eq!(result["rows_skipped_non_numeric"], 2);
    }

    #[test]
    fn aggregate_over_no_numeric_values_is_null_not_zero() {
        let payload = json!([{"n": "abc"}]);
        let query = HandleQuery {
            aggregate: Some(Aggregate {
                op: AggregateOp::Sum,
                field: Some("n".to_owned()),
            }),
            ..HandleQuery::default()
        };
        let result = apply_query(&payload, &query).expect("row-shaped");
        // Zero would be a claim that the values summed to nothing.
        assert_eq!(result["value"], Value::Null);
    }

    #[test]
    fn non_count_aggregate_without_a_field_is_refused() {
        let query = HandleQuery {
            aggregate: Some(Aggregate {
                op: AggregateOp::Sum,
                field: None,
            }),
            ..HandleQuery::default()
        };
        assert!(apply_query(&rows_payload(), &query)
            .unwrap_err()
            .contains("requires a 'field'"));
    }

    #[test]
    fn row_operations_on_a_non_row_payload_are_refused_not_faked() {
        let payload = json!({"total": 42});
        let query = HandleQuery {
            select: vec!["total".to_owned()],
            ..HandleQuery::default()
        };
        assert!(apply_query(&payload, &query)
            .unwrap_err()
            .contains("not a list of records"));
        // With no row operation requested, the value comes back whole.
        assert_eq!(
            apply_query(&payload, &HandleQuery::default()).expect("passthrough"),
            payload
        );
    }

    #[test]
    fn limit_is_capped_so_a_handle_cannot_dump_everything_back() {
        let big: Value = (0..500).map(|i| json!({"i": i})).collect();
        let query = HandleQuery {
            limit: 100_000,
            ..HandleQuery::default()
        };
        let result = apply_query(&big, &query).expect("row-shaped");
        assert_eq!(result["returned_rows"], MAX_PAGE_LIMIT);
        assert_eq!(result["has_more"], true);
    }

    #[test]
    fn a_handle_never_resolves_for_another_org_or_another_user() {
        let store = ToolResultStore::new();
        let handle = store.insert("org_a", "user_a", "mcp__s__t", rows_payload(), 512, None);

        assert!(store
            .resolve("org_a", "user_a", &handle.handle_id)
            .is_some());
        // Same user id, different tenant.
        assert!(store
            .resolve("org_b", "user_a", &handle.handle_id)
            .is_none());
        // Same tenant, different user — a colleague must not read this result.
        assert!(store
            .resolve("org_a", "user_b", &handle.handle_id)
            .is_none());
        assert!(store.resolve("org_a", "user_a", "res_made_up").is_none());
    }

    #[test]
    fn attaching_an_artifact_is_also_org_and_user_scoped() {
        let store = ToolResultStore::new();
        let handle = store.insert("org_a", "user_a", "mcp__s__t", rows_payload(), 512, None);

        assert!(!store.attach_artifact("org_b", "user_a", &handle.handle_id, "art_1"));
        assert!(!store.attach_artifact("org_a", "user_b", &handle.handle_id, "art_1"));
        assert!(store.attach_artifact("org_a", "user_a", &handle.handle_id, "art_1"));

        let resolved = store
            .resolve("org_a", "user_a", &handle.handle_id)
            .expect("resolves");
        assert_eq!(resolved.artifact_ref.as_deref(), Some("art_1"));
        assert_eq!(resolved.capability_id, "mcp__s__t");
    }

    #[test]
    fn the_model_envelope_carries_the_handle_and_says_it_is_complete() {
        let store = ToolResultStore::new();
        let handle = store.insert(
            "org_a",
            "user_a",
            "mcp__s__query",
            rows_payload(),
            512,
            Some("2026-08-09T12:00:00Z".to_owned()),
        );
        let envelope: Value = serde_json::from_str(&handle.to_model_json()).expect("valid json");

        assert_eq!(envelope["handle_id"], handle.handle_id);
        assert_eq!(envelope["capability_id"], "mcp__s__query");
        assert_eq!(envelope["row_count"], 3);
        assert_eq!(envelope["expires_at"], "2026-08-09T12:00:00Z");
        assert!(envelope["projection_hints"].as_array().is_some());
        // The note must tell the model the data is intact — the whole point
        // of a handle over a truncated blob.
        let note = envelope["note"].as_str().expect("note present");
        assert!(note.contains("COMPLETE"));
        assert!(note.contains("result_query"));
        // An unmaterialized handle claims no artifact.
        assert!(envelope.get("artifact_ref").is_none());
    }

    #[test]
    fn filter_and_aggregate_ops_reject_unknown_names_by_listing_the_valid_ones() {
        assert_eq!(FilterOp::parse("EQ").unwrap(), FilterOp::Eq);
        assert_eq!(FilterOp::parse(">=").unwrap(), FilterOp::Gte);
        let error = FilterOp::parse("regex").unwrap_err();
        assert!(error.contains("regex") && error.contains("contains"));

        assert_eq!(AggregateOp::parse("mean").unwrap(), AggregateOp::Avg);
        assert!(AggregateOp::parse("median").unwrap_err().contains("avg"));
    }
}
