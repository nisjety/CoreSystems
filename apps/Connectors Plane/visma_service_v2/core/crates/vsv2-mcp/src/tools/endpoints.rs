use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::catalog::{Catalog, CatalogEntry};
use crate::meta::meta;
use crate::tool::{parse_args, Tool, ToolDef, ToolError, ToolKind};

pub const NAME: &str = "visma_list_endpoints";
const DEFAULT_LIMIT: usize = 200;

/// Lists the synced Visma endpoints: what exists, where it comes from, how it is keyed.
pub struct ListEndpoints {
    catalog: Catalog,
}

impl ListEndpoints {
    pub fn new(catalog: Catalog) -> Self {
        Self { catalog }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    /// Case-insensitive substring of the endpoint name, BigQuery table or API path.
    #[serde(default)]
    search: Option<String>,
    /// `erp_rest`, `datamart_graphql` or `expense_api`.
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    /// Full detail of one endpoint (query parameters, defaults, paging, DataMart mapping).
    #[serde(default)]
    endpoint: Option<String>,
}

fn matches(e: &CatalogEntry, search: Option<&str>, source: Option<&str>) -> bool {
    if let Some(s) = source {
        if e.source != s {
            return false;
        }
    }
    let Some(q) = search.map(str::to_lowercase) else {
        return true;
    };
    [
        Some(&e.endpoint_name),
        e.bigquery_table.as_ref(),
        e.api_endpoint.as_ref(),
    ]
    .into_iter()
    .flatten()
    .any(|f| f.to_lowercase().contains(&q))
}

/// Everything a caller needs to use the endpoint with `visma_erp_query`.
fn detail(e: &CatalogEntry) -> Value {
    let mut d = summary(e);
    d["page_size"] = json!(e.page_size);
    d["default_query"] = e.default_query.clone();
    d["enabled"] = json!(e.enabled);
    d["datamart"] = e.datamart.clone().unwrap_or(Value::Null);
    d["queryable_live"] = json!(e.source == "erp_rest" && e.api_endpoint.is_some());
    d["notes"] = json!(e
        .notes
        .as_deref()
        .map(|n| vsv2_common::text::printable(n, 1_200)));
    d
}

fn summary(e: &CatalogEntry) -> Value {
    json!({
        "endpoint": e.endpoint_name,
        "source": e.source,
        "api_endpoint": e.api_endpoint,
        "bigquery_table": e.bigquery_table,
        "primary_key": e.primary_key,
        "tier": e.tier,
        "pagination_profile": e.pagination_profile,
        "modified_field": e.modified_field,
        "required_query": e.required_query,
    })
}

#[async_trait]
impl Tool for ListEndpoints {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: NAME,
            description: "List the Visma endpoints this service syncs into the warehouse: name, \
                source API, BigQuery table, primary key, sync tier. Use it first to find the \
                endpoint/table name for the other tools. Local config only; no network."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "search": {"type": "string", "description": "substring of endpoint, table or API path"},
                    "source": {"type": "string", "enum": ["erp_rest", "datamart_graphql", "expense_api"]},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 500, "description": "default 200"},
                    "endpoint": {"type": "string", "description": "exact endpoint name: return its full detail (required/default query parameters, paging, DataMart mapping, notes)"}
                },
                "additionalProperties": false
            }),
            kind: ToolKind::Read,
        }
    }

    async fn call(&self, args: Value) -> Result<Value, ToolError> {
        let a: Args = parse_args(args)?;
        if let Some(name) = &a.endpoint {
            let e = self.catalog.get(name).ok_or_else(|| {
                ToolError::InvalidArgs(format!(
                    "unknown endpoint {name:?}; call without `endpoint` to list them"
                ))
            })?;
            return Ok(
                json!({"endpoint": detail(e), "meta": meta("local_config", None, json!({"coverage": "catalog_entry"}))}),
            );
        }
        let limit = a.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, 500);
        let hits: Vec<&CatalogEntry> = self
            .catalog
            .all()
            .iter()
            .filter(|e| matches(e, a.search.as_deref(), a.source.as_deref()))
            .collect();
        let total = hits.len();
        Ok(json!({
            "total_matches": total,
            "returned": total.min(limit),
            "endpoints": hits.into_iter().take(limit).map(summary).collect::<Vec<_>>(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, source: &str, table: &str) -> CatalogEntry {
        serde_json::from_value(json!({
            "endpoint_name": name, "source": source, "bigquery_table": table,
            "api_endpoint": format!("/v1/{name}"), "primary_key": ["id"], "tier": 1
        }))
        .unwrap()
    }

    fn tool() -> ListEndpoints {
        ListEndpoints::new(Catalog::from_entries(vec![
            entry("customers", "erp_rest", "visma_customers"),
            entry("dm_projects", "datamart_graphql", "visma_dm_projects"),
            entry("suppliers", "erp_rest", "visma_suppliers"),
        ]))
    }

    #[tokio::test]
    async fn lists_everything_by_default_sorted() {
        let out = tool().call(json!({})).await.unwrap();
        assert_eq!(out["total_matches"], 3);
        assert_eq!(out["endpoints"][0]["endpoint"], "customers");
        assert_eq!(out["endpoints"][0]["bigquery_table"], "visma_customers");
    }

    #[tokio::test]
    async fn filters_by_source_and_by_case_insensitive_search_across_name_table_and_path() {
        let by_source = tool()
            .call(json!({"source": "datamart_graphql"}))
            .await
            .unwrap();
        assert_eq!(by_source["total_matches"], 1);
        let by_text = tool().call(json!({"search": "SUPPLIER"})).await.unwrap();
        assert_eq!(by_text["endpoints"][0]["endpoint"], "suppliers");
        let by_table = tool().call(json!({"search": "visma_dm_"})).await.unwrap();
        assert_eq!(by_table["total_matches"], 1);
    }

    #[tokio::test]
    async fn limit_caps_the_page_but_reports_the_true_total() {
        let out = tool().call(json!({"limit": 1})).await.unwrap();
        assert_eq!(
            (out["total_matches"].as_u64(), out["returned"].as_u64()),
            (Some(3), Some(1))
        );
    }

    #[tokio::test]
    async fn endpoint_detail_returns_query_defaults_paging_and_live_queryability() {
        let t = ListEndpoints::new(Catalog::from_entries(vec![serde_json::from_value(json!({
            "endpoint_name": "sales_orders_v3", "source": "erp_rest", "api_endpoint": "https://x/api/v3/SalesOrders",
            "page_size": 100, "pagination_profile": "salesorder_page_index", "required_query": ["expand"],
            "default_query": {"expand": ["All"]}, "notes": "Key is (type, orderId)"
        })).unwrap()]));
        let out = t
            .call(json!({"endpoint": "sales_orders_v3"}))
            .await
            .unwrap();
        let d = &out["endpoint"];
        assert_eq!(
            (d["page_size"].as_u64(), d["queryable_live"].as_bool()),
            (Some(100), Some(true))
        );
        assert_eq!(d["default_query"]["expand"][0], "All");
        assert_eq!(d["required_query"][0], "expand");
        assert!(d["notes"].as_str().unwrap().contains("orderId"));
        assert!(matches!(
            t.call(json!({"endpoint": "nope"})).await,
            Err(ToolError::InvalidArgs(_))
        ));
    }

    #[tokio::test]
    async fn rejects_unknown_arguments() {
        assert!(matches!(
            tool().call(json!({"srch": "x"})).await,
            Err(ToolError::InvalidArgs(_))
        ));
    }
}
