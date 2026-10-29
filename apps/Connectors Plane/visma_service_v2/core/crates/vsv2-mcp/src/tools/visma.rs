//! Live read tools: tenants, ERP REST query, single sales order, DataMart schema/query.

use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use vsv2_interactive::{Reader, WriteTarget};

use crate::catalog::{Catalog, CatalogEntry};
use crate::meta::meta;
use crate::tool::{parse_args, Tool, ToolDef, ToolError, ToolKind};
use crate::visma::{graphql_guard, ErpGet, GraphQl, Tenants};

pub const LIST_TENANTS: &str = "visma_list_tenants";
pub const ERP_QUERY: &str = "visma_erp_query";
pub const SALESORDER_GET: &str = "visma_salesorder_get";
pub const DATAMART_SCHEMA: &str = "visma_datamart_schema";
pub const DATAMART_QUERY: &str = "visma_datamart_query";

const DEFAULT_PAGE_SIZE: u32 = 25;
const MAX_PAGE_SIZE: u32 = 100;

// ------------------------------------------------------------------ tenants

pub struct ListTenants(pub Tenants);

#[async_trait]
impl Tool for ListTenants {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: LIST_TENANTS,
            description:
                "List the Visma tenants (companies) this server can read: id, name, status, \
                DataMart company id. Other tools take a `tenant` argument (id or name); the \
                default tenant is marked."
                    .into(),
            input_schema: json!({"type": "object", "properties": {}, "additionalProperties": false}),
            kind: ToolKind::Read,
        }
    }

    async fn call(&self, args: Value) -> Result<Value, ToolError> {
        let _: Map<String, Value> = parse_args::<Map<String, Value>>(args).and_then(|m| {
            m.is_empty()
                .then_some(m)
                .ok_or_else(|| ToolError::InvalidArgs("this tool takes no arguments".into()))
        })?;
        Ok(json!({"default_tenant": self.0.default_id(), "tenants": self.0.describe()}))
    }
}

// ------------------------------------------------------------------ ERP query

pub struct ErpQuery {
    pub catalog: Catalog,
    pub tenants: Tenants,
    pub erp: Arc<dyn ErpGet>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ErpArgs {
    endpoint: String,
    #[serde(default)]
    tenant: Option<String>,
    /// Fetch one record by key instead of a page.
    #[serde(default)]
    key: Option<String>,
    /// Extra query-string filters, e.g. {"lastModifiedDateTime": "2026-01-01"}.
    #[serde(default)]
    query: Map<String, Value>,
    /// 1-based, whatever the API's own paging base is.
    #[serde(default)]
    page: Option<u32>,
    #[serde(default)]
    page_size: Option<u32>,
}

/// Paging parameters that `page` and `page_size` own; a caller cannot pass them as filters.
const RESERVED_PARAMS: &[&str] = &[
    "pagesize",
    "pagenumber",
    "pageindex",
    "numbertoread",
    "skiprecords",
];

fn valid_param_key(k: &str) -> bool {
    let mut ch = k.chars();
    ch.next().is_some_and(|c| c.is_ascii_alphabetic())
        && k.len() <= 64
        && ch.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '$'))
}

fn scalar_to_param(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if s.len() <= 200 && !s.chars().any(char::is_control) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Array(a) if !a.is_empty() && a.len() <= 20 => {
            let parts: Option<Vec<String>> = a.iter().map(scalar_to_param).collect();
            parts.map(|p| p.join(","))
        }
        _ => None,
    }
}

/// Percent-encode one path segment; only unreserved characters pass through.
fn encode_segment(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Build query-string pairs: caller filters, required params (falling back to the config
/// default), then paging. Returns an error naming any required param nobody supplied.
fn build_query(
    entry: &CatalogEntry,
    a: &ErpArgs,
    page_size: u32,
    page: u32,
) -> Result<Vec<(String, String)>, ToolError> {
    let mut q: Vec<(String, String)> = Vec::new();
    for (k, v) in &a.query {
        if !valid_param_key(k) {
            return Err(ToolError::InvalidArgs(format!(
                "invalid query parameter name {k:?}"
            )));
        }
        if RESERVED_PARAMS.contains(&k.to_ascii_lowercase().as_str()) {
            return Err(ToolError::InvalidArgs(format!(
                "`{k}` is controlled by the `page`/`page_size` arguments"
            )));
        }
        let val = scalar_to_param(v).ok_or_else(|| {
            ToolError::InvalidArgs(format!(
                "query parameter {k:?} must be a short string, number, bool or list of those"
            ))
        })?;
        q.push((k.clone(), val));
    }
    for req in &entry.required_query {
        if q.iter().any(|(k, _)| k.eq_ignore_ascii_case(req)) {
            continue;
        }
        let default = entry.default_query.get(req).and_then(scalar_to_param);
        match default {
            Some(v) => q.push((req.clone(), v)),
            None => {
                return Err(ToolError::InvalidArgs(format!(
                    "endpoint {} requires query parameter `{req}` (pass it in `query`)",
                    entry.endpoint_name
                )))
            }
        }
    }
    if a.key.is_none() {
        match entry.pagination_profile.as_deref() {
            Some("finance_page_number") => {
                q.push(("pageNumber".into(), page.to_string()));
                q.push(("pageSize".into(), page_size.to_string()));
            }
            // Finance endpoints that declare no pageNumber: the deprecated numberToRead/skipRecords
            // pair, which the spec still lists for them.
            Some("numbertoread_skip") => {
                q.push(("numberToRead".into(), page_size.to_string()));
                q.push((
                    "skipRecords".into(),
                    (page - 1).saturating_mul(page_size).to_string(),
                ));
            }
            Some("salesorder_page_index") | Some("journaltx_page_index") => {
                q.push(("pageIndex".into(), (page - 1).to_string()));
                q.push(("pageSize".into(), page_size.to_string()));
            }
            // The endpoint returns its whole list in one response, so a second page would repeat the first.
            _ => {
                if page > 1 {
                    return Err(ToolError::InvalidArgs(format!(
                        "endpoint {} returns its whole list in one response; `page` must be 1",
                        entry.endpoint_name
                    )));
                }
            }
        }
    }
    Ok(q)
}

/// Normalize the shapes Visma APIs return into rows (+ totals) or a single record.
fn shape(body: Value) -> Value {
    match body {
        Value::Array(rows) => json!({"row_count": rows.len(), "rows": rows}),
        Value::Object(mut m) => match m.get("value").and_then(Value::as_array).cloned() {
            Some(rows) => {
                let mut out = json!({"row_count": rows.len(), "rows": rows});
                for k in ["totalCount", "nextPage"] {
                    if let Some(v) = m.remove(k) {
                        out[k] = v;
                    }
                }
                out
            }
            None => json!({"record": Value::Object(m)}),
        },
        other => json!({"record": other}),
    }
}

/// Filter names the caller chose: not paging, not `expand*` switches, not the sort order, not
/// parameters the endpoint requires anyway.
fn user_filters<'a>(entry: &CatalogEntry, a: &'a ErpArgs) -> Vec<&'a str> {
    a.query
        .keys()
        .map(String::as_str)
        .filter(|k| !k.to_ascii_lowercase().starts_with("expand"))
        .filter(|k| !k.eq_ignore_ascii_case("orderby"))
        .filter(|k| {
            !entry
                .required_query
                .iter()
                .any(|r| r.eq_ignore_ascii_case(k))
        })
        .collect()
}

/// The size of the whole collection a response reports: a top-level `totalCount` (SalesOrder) or
/// the `metadata.totalCount` every Finance row carries. An empty page proves a total of zero.
fn reported_total(out: &Value) -> Option<u64> {
    out["totalCount"]
        .as_u64()
        .or_else(|| out["rows"][0]["metadata"]["totalCount"].as_u64())
        .or_else(|| (out["row_count"].as_u64() == Some(0)).then_some(0))
}

impl ErpQuery {
    /// Visma ignores a filter name it does not know and returns the whole collection. Ask the same
    /// question once more without the caller's filters (one row) and compare the reported totals,
    /// so a filter that did nothing is visible instead of silently producing a wrong answer.
    async fn filter_check(
        &self,
        tenant: &vsv2_common::TenantId,
        path: &str,
        entry: &CatalogEntry,
        a: &ErpArgs,
        filters: &[&str],
        with_total: Option<u64>,
    ) -> Value {
        let unknown = |why: String| {
            json!({"filters": filters, "verdict": "unknown", "reason": why,
                   "note": "Could not tell whether the filters changed the result; compare totals yourself."})
        };
        let baseline = ErpArgs {
            endpoint: a.endpoint.clone(),
            tenant: None,
            key: None,
            query: a
                .query
                .iter()
                .filter(|(k, _)| !filters.contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            page: Some(1),
            page_size: Some(1),
        };
        let query = match build_query(entry, &baseline, 1, 1) {
            Ok(q) => q,
            Err(e) => return unknown(e.to_string()),
        };
        let without_total = match self.erp.get(tenant, path, &query).await {
            Ok(body) => reported_total(&shape(body)),
            Err(e) => return unknown(format!("the comparison read failed: {e}")),
        };
        let (verdict, note) = match (with_total, without_total) {
            (Some(w), Some(b)) if w < b => (
                "narrowed",
                "The filters reduced the collection: the total is smaller than without them.",
            ),
            (Some(w), Some(b)) if w == b => (
                "no_effect_detected",
                "The filters did not change the total. Either every row matches, or Visma ignored a filter \
                 name it does not know (it does not reject unknown names). Do not trust this page as \
                 filtered until the names are confirmed.",
            ),
            _ => (
                "unknown",
                "This endpoint reports no total, so the effect of the filters cannot be verified.",
            ),
        };
        json!({
            "filters": filters,
            "total_with_filters": with_total,
            "total_without_filters": without_total,
            "verdict": verdict,
            "note": note,
        })
    }
}

#[async_trait]
impl Tool for ErpQuery {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: ERP_QUERY,
            description: format!(
                "Live read (GET) from the Visma ERP REST APIs for one synced endpoint (see \
                 {} for names). Without `key`: one page of rows (`page` is 1-based, page_size \
                 default {DEFAULT_PAGE_SIZE}, max {MAX_PAGE_SIZE}); with `key`: that single record. \
                 `query` holds API filters (e.g. lastModifiedDateTime). A filter name the API reference does not \
                 declare for this endpoint is refused, because Visma would ignore it silently; for an endpoint \
                 the reference does not cover, `filter_check` compares the total with and without the filters. \
                 Use this for current operational data; use warehouse_query for history and analytics. Read-only.",
                crate::tools::endpoints::NAME
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "endpoint": {"type": "string", "description": "endpoint name, e.g. customers"},
                    "tenant": {"type": "string", "description": "tenant id or name (default tenant if omitted)"},
                    "key": {"type": "string", "description": "single record key (appended to the path)"},
                    "query": {"type": "object", "description": "API query parameters"},
                    "page": {"type": "integer", "minimum": 1},
                    "page_size": {"type": "integer", "minimum": 1, "maximum": MAX_PAGE_SIZE}
                },
                "required": ["endpoint"],
                "additionalProperties": false
            }),
            kind: ToolKind::Read,
        }
    }

    async fn call(&self, args: Value) -> Result<Value, ToolError> {
        let a: ErpArgs = parse_args(args)?;
        let entry = self.catalog.get(&a.endpoint).ok_or_else(|| {
            ToolError::InvalidArgs(format!(
                "unknown endpoint {:?}; use {}",
                a.endpoint,
                crate::tools::endpoints::NAME
            ))
        })?;
        if entry.source != "erp_rest" {
            return Err(ToolError::InvalidArgs(format!(
                "endpoint {} has source {}; only erp_rest endpoints can be queried live here",
                entry.endpoint_name, entry.source
            )));
        }
        let base = entry.api_endpoint.as_deref().ok_or_else(|| {
            ToolError::Failed(format!(
                "endpoint {} has no api_endpoint",
                entry.endpoint_name
            ))
        })?;
        let tenant = self.tenants.resolve(a.tenant.as_deref())?;
        let page_size = a
            .page_size
            .unwrap_or(DEFAULT_PAGE_SIZE)
            .clamp(1, MAX_PAGE_SIZE);
        let page = a.page.unwrap_or(1).max(1);
        let query = build_query(entry, &a, page_size, page)?;
        // Names are checked without case, so they are sent as Visma declares them: a name in another case might
        // be bound by the API only if it matches exactly, and would then be ignored without a word.
        let query: Vec<(String, String)> = query
            .into_iter()
            .map(|(name, value)| {
                let declared = crate::reference::declared_spelling(base, &name);
                (declared.map(str::to_string).unwrap_or(name), value)
            })
            .collect();
        if a.key.is_none() {
            if let Some(unknown) =
                crate::reference::unknown_query_names(base, a.query.keys().map(String::as_str))
            {
                if !unknown.is_empty() {
                    return Err(ToolError::InvalidArgs(format!(
                        "endpoint {} does not declare {}: Visma would ignore these names silently, so the call is \
                         refused. The names it accepts are in the API reference (skill api-reference).",
                        entry.endpoint_name,
                        unknown.join(", ")
                    )));
                }
            }
        }

        if let Some(k) = &a.key {
            // `.`/`..` would be normalised away by URL joining and climb out of the endpoint path.
            if k.contains("..") || k.split('/').any(|seg| seg == ".") || k.contains('\\') {
                return Err(ToolError::InvalidArgs(
                    "key must not contain `.`/`..` path segments".into(),
                ));
            }
        }
        let path = match &a.key {
            Some(k) if !k.is_empty() && k.len() <= 100 => {
                format!("{}/{}", base.trim_end_matches('/'), encode_segment(k))
            }
            Some(_) => {
                return Err(ToolError::InvalidArgs(
                    "key must be 1-100 characters".into(),
                ))
            }
            None => base.to_string(),
        };
        let body = self
            .erp
            .get(&tenant, &path, &query)
            .await
            .map_err(ToolError::Failed)?;

        let mut out = shape(body);
        out["endpoint"] = json!(entry.endpoint_name);
        out["tenant_id"] = json!(tenant.as_str());
        if a.key.is_none() {
            out["page"] = json!(page);
            out["page_size"] = json!(page_size);
            // Only a paged profile can have more rows behind a full page; `none` returns the whole list.
            let paged = matches!(
                entry.pagination_profile.as_deref(),
                Some(
                    "finance_page_number"
                        | "numbertoread_skip"
                        | "salesorder_page_index"
                        | "journaltx_page_index"
                )
            );
            let full = paged && out["row_count"].as_u64() == Some(page_size as u64);
            out["maybe_more"] = json!(full || out.get("nextPage").is_some_and(|v| !v.is_null()));
        }
        if a.key.is_none() && page == 1 {
            let filters = user_filters(entry, &a);
            if !filters.is_empty() {
                let with_total = reported_total(&out);
                out["filter_check"] = self
                    .filter_check(&tenant, &path, entry, &a, &filters, with_total)
                    .await;
            }
        }
        out["meta"] = meta(
            "erp_rest",
            Some(tenant.as_str()),
            json!({
                "endpoint": entry.endpoint_name,
                "path": path,
                "coverage": if a.key.is_some() { "single_record" } else { "single_page" },
                "complete_collection": false,
                "note": "One response is not proof of a complete collection; an empty or failed read is unknown, not zero.",
            }),
        );
        Ok(out)
    }
}

// ------------------------------------------------------------------ sales order (with ETag)

pub struct SalesOrderGet {
    pub tenants: Tenants,
    pub reader: Arc<dyn Reader>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SoArgs {
    #[serde(rename = "type")]
    order_type: String,
    #[serde(rename = "orderId")]
    order_id: String,
    #[serde(default)]
    tenant: Option<String>,
    /// Sections to include, e.g. ["lines", "customer"].
    #[serde(default)]
    expand: Vec<String>,
}

fn simple_id(label: &str, v: &str, max: usize) -> Result<(), ToolError> {
    let ok = !v.is_empty()
        && v.len() <= max
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    ok.then_some(()).ok_or_else(|| {
        ToolError::InvalidArgs(format!(
            "{label} must be 1-{max} chars of A-Z a-z 0-9 - _, got {v:?}"
        ))
    })
}

#[async_trait]
impl Tool for SalesOrderGet {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: SALESORDER_GET,
            description: "Fetch one sales order by type and orderId from SalesOrder V3, with its \
                current version (ETag, needed to edit it). Optional `expand` sections such as \
                lines, customer, payment, linefees. Read-only."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "type": {"type": "string", "description": "order type, e.g. SO"},
                    "orderId": {"type": "string"},
                    "tenant": {"type": "string"},
                    "expand": {"type": "array", "items": {"type": "string"}}
                },
                "required": ["type", "orderId"],
                "additionalProperties": false
            }),
            kind: ToolKind::Read,
        }
    }

    async fn call(&self, args: Value) -> Result<Value, ToolError> {
        let a: SoArgs = parse_args(args)?;
        simple_id("type", &a.order_type, 2)?;
        simple_id("orderId", &a.order_id, 15)?;
        if a.expand.len() > 10
            || a.expand.iter().any(|e| {
                e.is_empty() || e.len() > 20 || !e.chars().all(|c| c.is_ascii_alphabetic())
            })
        {
            return Err(ToolError::InvalidArgs(
                "expand must be up to 10 plain section names (letters only)".into(),
            ));
        }
        let tenant = self.tenants.resolve(a.tenant.as_deref())?;
        let mut path = format!("/api/v3/SalesOrders/{}/{}", a.order_type, a.order_id);
        if !a.expand.is_empty() {
            path.push_str(&format!("?expand={}", a.expand.join(",")));
        }
        let r = self
            .reader
            .get(&tenant, WriteTarget::SalesOrder, &path)
            .await
            .map_err(|e| ToolError::Failed(e.to_string()))?;
        match r.status {
            200 => {}
            404 => {
                return Err(ToolError::Failed(format!(
                    "sales order {}/{} not found",
                    a.order_type, a.order_id
                )))
            }
            s => {
                return Err(ToolError::Failed(format!(
                    "SalesOrder API returned {s}: {}",
                    vsv2_common::text::printable(&r.body, 300)
                )))
            }
        }
        let order: Value = serde_json::from_str(&r.body)
            .map_err(|e| ToolError::Failed(format!("unreadable order JSON: {e}")))?;
        let meta = meta(
            "salesorder_v3",
            Some(tenant.as_str()),
            json!({"coverage": "single_record", "order": format!("{}/{}", a.order_type, a.order_id), "etag_present": r.etag.is_some()}),
        );
        Ok(json!({"tenant_id": tenant.as_str(), "etag": r.etag, "order": order, "meta": meta}))
    }
}

// ------------------------------------------------------------------ DataMart

pub struct DatamartSchema {
    pub tenants: Tenants,
    pub gql: Arc<dyn GraphQl>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SchemaArgs {
    #[serde(default)]
    type_name: Option<String>,
    #[serde(default)]
    tenant: Option<String>,
}

const ROOT_QUERY: &str = "{ __schema { queryType { fields { name description } } } }";
const TYPE_QUERY: &str =
    "query T($n: String!) { __type(name: $n) { name kind description fields { name description \
    type { kind name ofType { kind name ofType { kind name ofType { kind name } } } } } } }";

/// `[Customer!]!`-style rendering of an introspection type reference.
fn type_ref(t: &Value) -> String {
    match t["kind"].as_str() {
        Some("NON_NULL") => format!("{}!", type_ref(&t["ofType"])),
        Some("LIST") => format!("[{}]", type_ref(&t["ofType"])),
        _ => t["name"].as_str().unwrap_or("?").to_string(),
    }
}

#[async_trait]
impl Tool for DatamartSchema {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: DATAMART_SCHEMA,
            description:
                "Explore the Visma DataMart GraphQL schema. Without `type_name`: lists the \
                root query fields (entities you can query). With `type_name` (e.g. a type name \
                from a field's type): lists that type's fields and their types. Use it to write \
                queries for visma_datamart_query."
                    .into(),
            input_schema: json!({
                "type": "object",
                "properties": {"type_name": {"type": "string"}, "tenant": {"type": "string"}},
                "additionalProperties": false
            }),
            kind: ToolKind::Read,
        }
    }

    async fn call(&self, args: Value) -> Result<Value, ToolError> {
        let a: SchemaArgs = parse_args(args)?;
        let tenant = self.tenants.resolve(a.tenant.as_deref())?;
        match a.type_name {
            None => {
                let d = self
                    .gql
                    .run(&tenant, ROOT_QUERY, json!({}))
                    .await
                    .map_err(ToolError::Failed)?;
                let fields: Vec<Value> = d["__schema"]["queryType"]["fields"]
                    .as_array()
                    .map(|f| {
                        f.iter()
                            .map(|x| json!({"name": x["name"], "description": x["description"]}))
                            .collect()
                    })
                    .unwrap_or_default();
                Ok(json!({"root_fields": fields}))
            }
            Some(n) => {
                simple_id("type_name", &n, 100).map_err(|_| {
                    ToolError::InvalidArgs("type_name must be a GraphQL type name".into())
                })?;
                let d = self
                    .gql
                    .run(&tenant, TYPE_QUERY, json!({"n": n}))
                    .await
                    .map_err(ToolError::Failed)?;
                let t = &d["__type"];
                if t.is_null() {
                    return Err(ToolError::Failed(format!(
                        "type {n} not found in the DataMart schema"
                    )));
                }
                let fields: Vec<Value> = t["fields"]
                    .as_array()
                    .map(|f| f.iter().map(|x| json!({"name": x["name"], "type": type_ref(&x["type"]), "description": x["description"]})).collect())
                    .unwrap_or_default();
                Ok(
                    json!({"name": t["name"], "kind": t["kind"], "description": t["description"], "fields": fields}),
                )
            }
        }
    }
}

pub struct DatamartQuery {
    pub tenants: Tenants,
    pub gql: Arc<dyn GraphQl>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DmArgs {
    query: String,
    #[serde(default)]
    variables: Option<Map<String, Value>>,
    #[serde(default)]
    tenant: Option<String>,
}

#[async_trait]
impl Tool for DatamartQuery {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: DATAMART_QUERY,
            description:
                "Run a read-only GraphQL query against Visma DataMart (bulk reporting data, \
                slightly delayed versus the live ERP APIs). Queries only; mutations and \
                subscriptions are refused. Root fields take `pageSize` and `lastId` (the `id` of \
                the last row of the previous page) for paging, and `updatedInDataMart` / \
                `updatedInDataMartTo` for a change window; there is no `first`/`after`. Rows carry \
                the DataMart `companyId` (see visma_list_tenants). Always set `pageSize`: large \
                results are parked behind a result id."
                    .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "variables": {"type": "object"},
                    "tenant": {"type": "string"}
                },
                "required": ["query"],
                "additionalProperties": false
            }),
            kind: ToolKind::Read,
        }
    }

    async fn call(&self, args: Value) -> Result<Value, ToolError> {
        let a: DmArgs = parse_args(args)?;
        graphql_guard::validate_query(&a.query)
            .map_err(|e| ToolError::Failed(format!("query rejected: {e}")))?;
        let tenant = self.tenants.resolve(a.tenant.as_deref())?;
        let vars = a.variables.map(Value::Object).unwrap_or_else(|| json!({}));
        let data = self
            .gql
            .run(&tenant, &a.query, vars)
            .await
            .map_err(ToolError::Failed)?;
        let meta = meta(
            "datamart_graphql",
            Some(tenant.as_str()),
            json!({"coverage": "single_response", "complete_collection": false,
                   "note": "DataMart lags the live ERP; one response is not proof of a complete collection."}),
        );
        Ok(json!({"tenant_id": tenant.as_str(), "data": data, "meta": meta}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::visma::tenants::test_tenants;
    use std::sync::Mutex;
    use vsv2_common::TenantId;
    use vsv2_interactive::{ReadResponse, Result as IResult};

    type Call = (String, String, Vec<(String, String)>);

    struct FakeErp {
        reply: Mutex<Result<Value, String>>,
        calls: Mutex<Vec<Call>>,
    }
    impl FakeErp {
        fn new(reply: Result<Value, String>) -> Arc<Self> {
            Arc::new(Self {
                reply: Mutex::new(reply),
                calls: Mutex::new(vec![]),
            })
        }
    }
    #[async_trait]
    impl ErpGet for FakeErp {
        async fn get(
            &self,
            t: &TenantId,
            path: &str,
            q: &[(String, String)],
        ) -> Result<Value, String> {
            self.calls
                .lock()
                .unwrap()
                .push((t.to_string(), path.into(), q.to_vec()));
            self.reply.lock().unwrap().clone()
        }
    }

    fn catalog() -> Catalog {
        let e = |v: Value| serde_json::from_value::<CatalogEntry>(v).unwrap();
        Catalog::from_entries(vec![
            e(
                json!({"endpoint_name": "customers", "source": "erp_rest", "api_endpoint": "/v1/customer", "pagination_profile": "finance_page_number"}),
            ),
            e(
                json!({"endpoint_name": "sales_orders_v3", "source": "erp_rest", "api_endpoint": "https://salesorder.visma.net/api/v3/SalesOrders", "pagination_profile": "salesorder_page_index", "required_query": ["expand"], "default_query": {"expand": ["All", "LineFees"]}}),
            ),
            e(json!({"endpoint_name": "dm_projects", "source": "datamart_graphql"})),
            e(
                json!({"endpoint_name": "budgets", "source": "erp_rest", "api_endpoint": "/v1/budget", "pagination_profile": "finance_page_number", "required_query": ["branch"]}),
            ),
            e(
                json!({"endpoint_name": "journal", "source": "erp_rest", "api_endpoint": "/v2/journaltransaction", "pagination_profile": "journaltx_page_index"}),
            ),
            // An endpoint the API reference does not cover, so only the total comparison can guard it.
            e(
                json!({"endpoint_name": "uncovered", "source": "erp_rest", "api_endpoint": "/v1/not-in-the-reference", "pagination_profile": "finance_page_number"}),
            ),
        ])
    }

    fn catalog_entry(profile: &str) -> CatalogEntry {
        serde_json::from_value(json!({"endpoint_name": "t", "source": "erp_rest", "api_endpoint": "/v1/t", "pagination_profile": profile})).unwrap()
    }

    fn erp_args(v: Value) -> ErpArgs {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn numbertoread_endpoints_page_by_row_count_and_offset() {
        let q = build_query(
            &catalog_entry("numbertoread_skip"),
            &erp_args(json!({"endpoint": "t"})),
            50,
            3,
        )
        .unwrap();

        assert!(q.contains(&("numberToRead".into(), "50".into())));
        assert!(q.contains(&("skipRecords".into(), "100".into())));
        assert!(!q.iter().any(|(k, _)| k == "pageNumber" || k == "pageSize"));
    }

    #[test]
    fn a_whole_list_endpoint_refuses_a_second_page() {
        let entry = catalog_entry("none");
        let err = build_query(
            &entry,
            &erp_args(json!({"endpoint": "t", "page": 2})),
            100,
            2,
        )
        .unwrap_err();

        assert!(err.to_string().contains("must be 1"), "{err}");
        assert!(build_query(&entry, &erp_args(json!({"endpoint": "t"})), 100, 1).is_ok());
    }

    #[test]
    fn paging_and_sort_parameters_are_not_filters() {
        let entry = catalog_entry("finance_page_number");
        let a = erp_args(json!({"endpoint": "t", "query": {"orderBy": "name", "status": "Open"}}));
        assert_eq!(user_filters(&entry, &a), vec!["status"]);

        let a = erp_args(json!({"endpoint": "t", "query": {"numberToRead": "5"}}));
        let err = build_query(&entry, &a, 100, 1).unwrap_err();
        assert!(err.to_string().contains("numberToRead"), "{err}");
    }

    fn erp_tool(reply: Result<Value, String>) -> (ErpQuery, Arc<FakeErp>) {
        let erp = FakeErp::new(reply);
        (
            ErpQuery {
                catalog: catalog(),
                tenants: test_tenants(),
                erp: erp.clone(),
            },
            erp,
        )
    }

    #[tokio::test]
    async fn finance_page_is_one_based_and_rows_are_normalized() {
        let (t, erp) = erp_tool(Ok(json!([{"id": 1}, {"id": 2}])));
        let out = t
            .call(json!({"endpoint": "customers", "page": 3, "page_size": 2}))
            .await
            .unwrap();
        assert_eq!(
            (
                out["row_count"].as_u64(),
                out["page"].as_u64(),
                out["maybe_more"].as_bool()
            ),
            (Some(2), Some(3), Some(true))
        );
        let (tenant, path, q) = erp.calls.lock().unwrap()[0].clone();
        assert_eq!((tenant.as_str(), path.as_str()), ("t-aq", "/v1/customer"));
        assert!(
            q.contains(&("pageNumber".into(), "3".into()))
                && q.contains(&("pageSize".into(), "2".into()))
        );
    }

    #[tokio::test]
    async fn index_based_apis_get_zero_based_pageindex_and_value_envelopes_are_unwrapped() {
        let (t, erp) = erp_tool(Ok(
            json!({"value": [{"orderId": "1"}], "totalCount": 22313, "nextPage": "x"}),
        ));
        let out = t
            .call(json!({"endpoint": "sales_orders_v3", "page": 1, "page_size": 1}))
            .await
            .unwrap();
        assert_eq!(out["totalCount"], 22313);
        assert_eq!(out["rows"][0]["orderId"], "1");
        let q = erp.calls.lock().unwrap()[0].2.clone();
        assert!(q.contains(&("pageIndex".into(), "0".into())), "{q:?}");
        // required `expand` falls back to the config default, joined with commas
        assert!(
            q.contains(&("expand".into(), "All,LineFees".into())),
            "{q:?}"
        );
        let (t2, erp2) = erp_tool(Ok(json!([])));
        t2.call(json!({"endpoint": "journal", "page": 2}))
            .await
            .unwrap();
        assert!(erp2.calls.lock().unwrap()[0]
            .2
            .contains(&("pageIndex".into(), "1".into())));
    }

    #[tokio::test]
    async fn key_fetch_encodes_the_segment_skips_paging_and_returns_a_record() {
        let (t, erp) = erp_tool(Ok(json!({"id": "A/B 1"})));
        let out = t
            .call(json!({"endpoint": "customers", "key": "A/B 1"}))
            .await
            .unwrap();
        assert_eq!(out["record"]["id"], "A/B 1");
        assert!(out.get("page").is_none());
        let (_, path, q) = erp.calls.lock().unwrap()[0].clone();
        assert_eq!(path, "/v1/customer/A%2FB%201");
        assert!(q.is_empty());
    }

    #[tokio::test]
    async fn a_key_cannot_climb_out_of_the_endpoint_path() {
        let (t, erp) = erp_tool(Ok(json!({})));
        for bad in ["..", "../customer", "a/../b", "a\\b", "./x"] {
            let r = t.call(json!({"endpoint": "customers", "key": bad})).await;
            assert!(matches!(r, Err(ToolError::InvalidArgs(_))), "{bad}: {r:?}");
        }
        assert!(
            erp.calls.lock().unwrap().is_empty(),
            "nothing reached Visma"
        );
        // a literal percent sequence is encoded again, so it stays one harmless segment
        t.call(json!({"endpoint": "customers", "key": "%2e%2e"}))
            .await
            .unwrap();
        assert_eq!(erp.calls.lock().unwrap()[0].1, "/v1/customer/%252e%252e");
    }

    #[tokio::test]
    async fn rejects_unknown_endpoints_non_erp_sources_unknown_tenants_and_missing_required_params()
    {
        let (t, erp) = erp_tool(Ok(json!([])));
        assert!(matches!(
            t.call(json!({"endpoint": "nope"})).await,
            Err(ToolError::InvalidArgs(_))
        ));
        let dm = t
            .call(json!({"endpoint": "dm_projects"}))
            .await
            .unwrap_err()
            .to_string();
        assert!(dm.contains("only erp_rest"), "{dm}");
        assert!(t
            .call(json!({"endpoint": "customers", "tenant": "someone-else"}))
            .await
            .is_err());
        let req = t
            .call(json!({"endpoint": "budgets"}))
            .await
            .unwrap_err()
            .to_string();
        assert!(req.contains("requires query parameter `branch`"), "{req}");
        assert!(
            erp.calls.lock().unwrap().is_empty(),
            "nothing was sent upstream"
        );
    }

    #[tokio::test]
    async fn query_parameters_are_validated_and_paging_params_are_reserved() {
        let (t, erp) = erp_tool(Ok(json!([])));
        for bad in [
            json!({"a b": 1}),
            json!({"pageSize": 1000}),
            json!({"x": {"nested": 1}}),
            json!({"x": "\n"}),
            json!({"1x": 1}),
        ] {
            let r = t.call(json!({"endpoint": "customers", "query": bad})).await;
            assert!(matches!(r, Err(ToolError::InvalidArgs(_))), "{r:?}");
        }
        let refused = t
            .call(json!({"endpoint": "budgets", "query": {"branch": "B1", "year": 2026}}))
            .await;
        assert!(
            matches!(refused, Err(ToolError::InvalidArgs(ref m)) if m.contains("does not declare year")),
            "{refused:?}"
        );
        t.call(json!({"endpoint": "budgets", "query": {"branch": "B1", "financialYear": 2026, "ledger": ["L1", "L2"]}}))
            .await
            .unwrap();
        let q = erp.calls.lock().unwrap()[0].2.clone();
        assert!(
            q.contains(&("financialYear".into(), "2026".into()))
                && q.contains(&("ledger".into(), "L1,L2".into()))
        );
    }

    #[tokio::test]
    async fn page_size_is_clamped_and_upstream_errors_become_tool_failures() {
        let (t, erp) = erp_tool(Ok(json!([])));
        t.call(json!({"endpoint": "customers", "page_size": 5000}))
            .await
            .unwrap();
        assert!(erp.calls.lock().unwrap()[0]
            .2
            .contains(&("pageSize".into(), "100".into())));
        let (t2, _) = erp_tool(Err("429 slow down".into()));
        assert!(
            matches!(t2.call(json!({"endpoint": "customers"})).await, Err(ToolError::Failed(m)) if m.contains("429"))
        );
    }

    /// An ERP fake that answers each call from a queue, so a test can give the filtered read and
    /// the comparison read different totals.
    struct SeqErp {
        replies: Mutex<std::collections::VecDeque<Result<Value, String>>>,
        calls: Mutex<Vec<Call>>,
    }
    #[async_trait]
    impl ErpGet for SeqErp {
        async fn get(
            &self,
            t: &TenantId,
            path: &str,
            q: &[(String, String)],
        ) -> Result<Value, String> {
            self.calls
                .lock()
                .unwrap()
                .push((t.to_string(), path.into(), q.to_vec()));
            self.replies
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err("no more scripted replies".into()))
        }
    }
    fn seq_tool(replies: Vec<Result<Value, String>>) -> (ErpQuery, Arc<SeqErp>) {
        let erp = Arc::new(SeqErp {
            replies: Mutex::new(replies.into()),
            calls: Mutex::new(vec![]),
        });
        (
            ErpQuery {
                catalog: catalog(),
                tenants: test_tenants(),
                erp: erp.clone(),
            },
            erp,
        )
    }
    fn finance_page(total: u64) -> Result<Value, String> {
        Ok(json!([{"id": 1, "metadata": {"totalCount": total, "maxPageSize": 1000}}]))
    }

    #[tokio::test]
    async fn a_filter_that_narrows_the_collection_is_reported_as_narrowed() {
        let (t, erp) = seq_tool(vec![finance_page(951), finance_page(22704)]);
        let out = t
            .call(json!({"endpoint": "customers", "page_size": 1, "query": {"status": "Open"}}))
            .await
            .unwrap();
        let c = &out["filter_check"];
        assert_eq!(
            (
                c["verdict"].as_str(),
                c["total_with_filters"].as_u64(),
                c["total_without_filters"].as_u64()
            ),
            (Some("narrowed"), Some(951), Some(22704)),
            "{c}"
        );
        let calls = erp.calls.lock().unwrap();
        assert_eq!(calls.len(), 2, "one filtered read and one comparison read");
        assert!(calls[0].2.contains(&("status".into(), "Open".into())));
        assert!(
            !calls[1].2.iter().any(|(k, _)| k == "status")
                && calls[1].2.contains(&("pageSize".into(), "1".into())),
            "the comparison read drops the filters and asks for one row: {:?}",
            calls[1].2
        );
    }

    #[tokio::test]
    async fn a_filter_visma_ignored_is_caught_because_the_total_did_not_change() {
        // The endpoint is not in the API reference, so its names cannot be checked up front and the
        // total comparison is the only guard.
        let (t, _) = seq_tool(vec![finance_page(6595), finance_page(6595)]);
        let out = t
            .call(json!({"endpoint": "uncovered", "page_size": 1, "query": {"customerClass": "NORGE"}}))
            .await
            .unwrap();
        let c = &out["filter_check"];
        assert_eq!(c["verdict"], "no_effect_detected", "{c}");
        assert!(c["note"].as_str().unwrap().contains("ignored a filter"));
    }

    #[tokio::test]
    async fn a_filter_the_reference_does_not_declare_is_refused_before_anything_is_read() {
        let (t, erp) = erp_tool(finance_page(6595));
        let refused = t
            .call(json!({"endpoint": "customers", "page_size": 1, "query": {"customerClass": "NORGE"}}))
            .await;
        assert!(
            matches!(refused, Err(ToolError::InvalidArgs(ref m)) if m.contains("does not declare customerClass")),
            "{refused:?}"
        );
        assert!(
            erp.calls.lock().unwrap().is_empty(),
            "nothing is read when a name is refused"
        );
    }

    #[tokio::test]
    async fn a_filter_is_sent_with_the_spelling_the_api_declares_whatever_case_it_was_written_in() {
        let (t, erp) = erp_tool(finance_page(3));
        t.call(json!({"endpoint": "customers", "page_size": 1, "query": {"LASTMODIFIEDDATETIME": "2026-01-01"}}))
            .await
            .unwrap();
        let calls = erp.calls.lock().unwrap();
        assert!(
            calls[0]
                .2
                .contains(&("lastModifiedDateTime".to_string(), "2026-01-01".to_string())),
            "{:?}",
            calls[0].2
        );
        assert!(!calls[0].2.iter().any(|(k, _)| k == "LASTMODIFIEDDATETIME"));
    }

    #[tokio::test]
    async fn a_filter_that_matches_nothing_is_narrowed_to_zero_not_unknown() {
        let (t, _) = seq_tool(vec![Ok(json!([])), finance_page(10)]);
        let out = t
            .call(json!({"endpoint": "customers", "query": {"name": "zzz"}}))
            .await
            .unwrap();
        assert_eq!(out["filter_check"]["verdict"], "narrowed");
        assert_eq!(out["filter_check"]["total_with_filters"], 0);
    }

    #[tokio::test]
    async fn sales_order_totals_are_read_from_the_top_level() {
        let so = |n: u64| Ok(json!({"value": [{"orderId": "1"}], "totalCount": n}));
        let (t, _) = seq_tool(vec![so(40), so(22314)]);
        let out = t
            .call(json!({"endpoint": "sales_orders_v3", "page_size": 1, "query": {"customerId": "300001"}}))
            .await
            .unwrap();
        assert_eq!(out["filter_check"]["verdict"], "narrowed");
        assert_eq!(out["filter_check"]["total_without_filters"], 22314);
    }

    #[tokio::test]
    async fn no_check_without_filters_for_expand_for_required_params_for_keys_or_later_pages() {
        // expand and the required branch are not filters; page 2 and key reads are not checked
        let cases = [
            json!({"endpoint": "customers"}),
            json!({"endpoint": "sales_orders_v3", "query": {"expand": "lines"}}),
            json!({"endpoint": "budgets", "query": {"branch": "B1"}}),
            json!({"endpoint": "customers", "page": 2, "query": {"status": "Active"}}),
            json!({"endpoint": "customers", "key": "300001", "query": {"status": "Active"}}),
        ];
        for args in cases {
            let (t, erp) = seq_tool(vec![finance_page(5), finance_page(5)]);
            let out = t.call(args.clone()).await.unwrap();
            assert!(out.get("filter_check").is_none(), "{args}: {out}");
            assert_eq!(
                erp.calls.lock().unwrap().len(),
                1,
                "{args}: only the requested read is made"
            );
        }
    }

    #[tokio::test]
    async fn a_failing_comparison_read_never_fails_the_query_itself() {
        let (t, _) = seq_tool(vec![finance_page(7), Err("429 slow down".into())]);
        let out = t
            .call(json!({"endpoint": "customers", "page_size": 1, "query": {"status": "Active"}}))
            .await
            .unwrap();
        assert_eq!(
            out["rows"][0]["id"], 1,
            "the page the user asked for is still returned"
        );
        assert_eq!(out["filter_check"]["verdict"], "unknown");
        assert!(out["filter_check"]["reason"]
            .as_str()
            .unwrap()
            .contains("429"));
    }

    #[tokio::test]
    async fn an_endpoint_without_totals_is_unknown_not_a_false_alarm() {
        let (t, _) = seq_tool(vec![Ok(json!([{"id": 1}])), Ok(json!([{"id": 1}]))]);
        let out = t
            .call(json!({"endpoint": "customers", "page_size": 1, "query": {"status": "Active"}}))
            .await
            .unwrap();
        assert_eq!(out["filter_check"]["verdict"], "unknown");
    }

    struct FakeReader(Mutex<ReadResponse>, Mutex<Vec<String>>);
    #[async_trait]
    impl Reader for FakeReader {
        async fn get(&self, _: &TenantId, t: WriteTarget, path: &str) -> IResult<ReadResponse> {
            assert_eq!(t, WriteTarget::SalesOrder);
            self.1.lock().unwrap().push(path.into());
            Ok(self.0.lock().unwrap().clone())
        }
    }

    fn so_tool(status: u16, body: &str, etag: Option<&str>) -> (SalesOrderGet, Arc<FakeReader>) {
        let r = Arc::new(FakeReader(
            Mutex::new(ReadResponse {
                status,
                body: body.into(),
                etag: etag.map(String::from),
            }),
            Mutex::new(vec![]),
        ));
        (
            SalesOrderGet {
                tenants: test_tenants(),
                reader: r.clone(),
            },
            r,
        )
    }

    #[tokio::test]
    async fn salesorder_get_returns_order_and_etag_and_builds_the_expand_query() {
        let (t, r) = so_tool(200, r#"{"orderId":"1"}"#, Some("\"e1\""));
        let out = t
            .call(json!({"type": "SO", "orderId": "000100", "expand": ["lines", "customer"]}))
            .await
            .unwrap();
        assert_eq!(
            (out["etag"].as_str(), out["order"]["orderId"].as_str()),
            (Some("\"e1\""), Some("1"))
        );
        assert_eq!(
            r.1.lock().unwrap()[0],
            "/api/v3/SalesOrders/SO/000100?expand=lines,customer"
        );
    }

    #[tokio::test]
    async fn salesorder_get_validates_ids_expands_and_maps_statuses() {
        let (t, r) = so_tool(404, "", None);
        assert!(t
            .call(json!({"type": "SO", "orderId": "1"}))
            .await
            .unwrap_err()
            .to_string()
            .contains("not found"));
        for bad in [
            json!({"type": "SOX", "orderId": "1"}),
            json!({"type": "SO", "orderId": "../x"}),
            json!({"type": "SO", "orderId": "1", "expand": ["a&b=c"]}),
        ] {
            assert!(matches!(t.call(bad).await, Err(ToolError::InvalidArgs(_))));
        }
        *r.0.lock().unwrap() = ReadResponse {
            status: 403,
            body: "no".into(),
            etag: None,
        };
        assert!(t
            .call(json!({"type": "SO", "orderId": "1"}))
            .await
            .unwrap_err()
            .to_string()
            .contains("403"));
    }

    struct FakeGql(Mutex<Vec<Value>>, Mutex<Vec<(String, Value)>>);
    #[async_trait]
    impl GraphQl for FakeGql {
        async fn run(&self, _: &TenantId, q: &str, v: Value) -> Result<Value, String> {
            self.1.lock().unwrap().push((q.into(), v));
            self.0
                .lock()
                .unwrap()
                .pop()
                .ok_or_else(|| "no reply".to_string())
        }
    }
    fn gql(replies: Vec<Value>) -> Arc<FakeGql> {
        Arc::new(FakeGql(Mutex::new(replies), Mutex::new(vec![])))
    }

    #[tokio::test]
    async fn datamart_schema_lists_root_fields_and_renders_type_refs() {
        let g = gql(vec![
            json!({"__schema": {"queryType": {"fields": [{"name": "customers", "description": "d"}]}}}),
        ]);
        let t = DatamartSchema {
            tenants: test_tenants(),
            gql: g.clone(),
        };
        assert_eq!(
            t.call(json!({})).await.unwrap()["root_fields"][0]["name"],
            "customers"
        );

        let g2 = gql(vec![
            json!({"__type": {"name": "Customer", "kind": "OBJECT", "fields": [
            {"name": "id", "type": {"kind": "NON_NULL", "ofType": {"kind": "SCALAR", "name": "ID"}}},
            {"name": "tags", "type": {"kind": "LIST", "ofType": {"kind": "NON_NULL", "ofType": {"kind": "SCALAR", "name": "String"}}}}]}}),
        ]);
        let t2 = DatamartSchema {
            tenants: test_tenants(),
            gql: g2.clone(),
        };
        let out = t2.call(json!({"type_name": "Customer"})).await.unwrap();
        assert_eq!(
            (
                out["fields"][0]["type"].as_str(),
                out["fields"][1]["type"].as_str()
            ),
            (Some("ID!"), Some("[String!]"))
        );
        assert_eq!(g2.1.lock().unwrap()[0].1, json!({"n": "Customer"}));
        assert!(t2.call(json!({"type_name": "bad name!"})).await.is_err());
    }

    #[tokio::test]
    async fn datamart_query_runs_queries_but_never_mutations() {
        let g = gql(vec![json!({"customers": {"nodes": []}})]);
        let t = DatamartQuery {
            tenants: test_tenants(),
            gql: g.clone(),
        };
        let out = t
            .call(
                json!({"query": "{ customers(first: 1) { nodes { id } } }", "variables": {"a": 1}}),
            )
            .await
            .unwrap();
        assert!(out["data"]["customers"].is_object());
        assert_eq!(g.1.lock().unwrap()[0].1, json!({"a": 1}));

        let bad = t
            .call(json!({"query": "mutation { x }"}))
            .await
            .unwrap_err()
            .to_string();
        assert!(bad.contains("query rejected"), "{bad}");
        assert_eq!(
            g.1.lock().unwrap().len(),
            1,
            "the mutation never reached DataMart"
        );
    }

    #[tokio::test]
    async fn list_tenants_takes_no_arguments() {
        let t = ListTenants(test_tenants());
        let out = t.call(json!({})).await.unwrap();
        assert_eq!(out["default_tenant"], "t-aq");
        assert!(t.call(json!({"x": 1})).await.is_err());
    }
}
