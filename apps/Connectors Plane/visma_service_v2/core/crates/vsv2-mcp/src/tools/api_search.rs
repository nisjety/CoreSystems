//! `visma_search_api`: find the Visma ERP operation that answers a question. Offline: it reads the
//! embedded API reference (Finance ERP, SalesOrders v3, JournalTx v1), needs no credentials and calls
//! nothing in Visma.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::catalog::Catalog;
use crate::meta::meta;
use crate::reference;
use crate::tool::{parse_args, Tool, ToolDef, ToolError, ToolKind};

pub const NAME: &str = "visma_search_api";
const DEFAULT_LIMIT: usize = 8;
const MAX_LIMIT: usize = 20;
/// Query parameter names shown per operation; the full list is the reference itself.
const MAX_PARAMS_SHOWN: usize = 40;
const MAX_QUESTION_CHARS: usize = 200;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    query: String,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    api: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

pub struct ApiSearch {
    pub catalog: Catalog,
}

#[async_trait]
impl Tool for ApiSearch {
    fn def(&self) -> ToolDef {
        let reference = reference::reference();
        let api_ids: Vec<&String> = reference.apis.keys().collect();
        ToolDef {
            name: NAME,
            description: format!(
                "Search Visma's own API reference for the operation that answers a question, with its query \
                 parameter names: the Finance ERP API, the SalesOrders v3 API and the JournalTx v1 API ({} \
                 operations). Offline: no credentials, no call to Visma. Use it to find the right endpoint and \
                 filter names before visma_erp_query, and to see the request shape of a write. Matches show \
                 `catalog_endpoint` when the endpoint can be read with visma_erp_query. Read-only.",
                reference.operations.len()
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "maxLength": MAX_QUESTION_CHARS, "description": "words from the question, e.g. customer invoices or sales order lines"},
                    "method": {"type": "string", "enum": ["GET", "POST", "PUT", "PATCH", "DELETE"], "description": "only operations with this method"},
                    "api": {"type": "string", "enum": api_ids, "description": "only this API"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": MAX_LIMIT}
                },
                "required": ["query"],
                "additionalProperties": false
            }),
            kind: ToolKind::Read,
        }
    }

    async fn call(&self, args: Value) -> Result<Value, ToolError> {
        let a: Args = parse_args(args)?;
        if a.query.trim().is_empty() {
            return Err(ToolError::InvalidArgs("query must not be empty".into()));
        }
        if a.query.chars().count() > MAX_QUESTION_CHARS {
            return Err(ToolError::InvalidArgs(format!(
                "query is longer than {MAX_QUESTION_CHARS} characters"
            )));
        }
        let reference = reference::reference();
        if let Some(api) = &a.api {
            if !reference.apis.contains_key(api) {
                let known: Vec<&String> = reference.apis.keys().collect();
                return Err(ToolError::InvalidArgs(format!(
                    "unknown api {api:?}; the references cover {known:?}"
                )));
            }
        }
        let limit = a.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
        let hits = reference::search(&a.query, a.method.as_deref(), a.api.as_deref(), limit);
        let matches: Vec<Value> = hits
            .iter()
            .map(|hit| {
                let op = hit.operation;
                let api = reference::api(&op.api);
                json!({
                    "api": op.api,
                    "api_title": api.map(|a| a.title.as_str()),
                    "host": api.and_then(|a| a.hosts.first()),
                    "method": op.method,
                    "path": op.path,
                    "operation_id": op.operation_id,
                    "summary": op.summary,
                    "tags": op.tags,
                    "catalog_endpoint": self.catalog_name(&op.api, &op.path, &op.method),
                    "query_parameters": op.query.iter().take(MAX_PARAMS_SHOWN).collect::<Vec<_>>(),
                    "required_query_parameters": op.required,
                    "score": hit.score,
                })
            })
            .collect();
        let note = if matches.is_empty() {
            "No operation matches. Try other words (the references use Visma's English resource names, e.g. \
             customer, salesorder, supplierinvoice, voucher), drop the api filter, or use visma_list_endpoints \
             for the endpoints this server syncs."
        } else {
            "Use catalog_endpoint with visma_erp_query for reads. Write operations are listed for their shape; \
             only the operations this server offers as tools can be run."
        };
        Ok(json!({
            "query": a.query,
            "total_matches": matches.len(),
            "matches": matches,
            "note": note,
            "meta": meta(
                "api_reference",
                None,
                json!({
                    "apis": reference.apis.iter().map(|(id, api)| json!({
                        "id": id, "title": api.title, "spec_version": api.spec_version, "source": api.source,
                    })).collect::<Vec<_>>(),
                    "retrieved": reference.retrieved,
                    "operations": reference.operations.len(),
                }),
            ),
        }))
    }
}

impl ApiSearch {
    /// The catalog name that reads this operation with `visma_erp_query`, for GET operations only.
    fn catalog_name(&self, api: &str, path: &str, method: &str) -> Value {
        if method != "GET" {
            return Value::Null;
        }
        self.catalog
            .all()
            .iter()
            .filter(|entry| entry.source == "erp_rest")
            .find(|entry| {
                entry
                    .api_endpoint
                    .as_deref()
                    .and_then(reference::locate)
                    .is_some_and(|(entry_api, entry_path)| entry_api == api && entry_path == path)
            })
            .map(|entry| json!(entry.endpoint_name))
            .unwrap_or(Value::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::CatalogEntry;

    fn entry(name: &str, endpoint: &str) -> CatalogEntry {
        CatalogEntry {
            endpoint_name: name.into(),
            bigquery_table: None,
            primary_key: json!("id"),
            source: "erp_rest".into(),
            api_endpoint: Some(endpoint.into()),
            pagination_profile: None,
            page_size: None,
            tier: None,
            modified_field: None,
            default_query: json!({}),
            required_query: vec![],
            enabled: None,
            datamart: None,
            notes: None,
        }
    }

    fn tool() -> ApiSearch {
        ApiSearch {
            catalog: Catalog::from_entries(vec![
                entry("customers", "/v1/customer"),
                entry(
                    "sales_orders_v3",
                    "https://salesorder.visma.net/api/v3/SalesOrders",
                ),
            ]),
        }
    }

    #[tokio::test]
    async fn finds_the_endpoint_and_names_the_catalog_entry_that_reads_it() {
        let out = tool()
            .call(json!({"query": "customer", "method": "GET", "api": "erp"}))
            .await
            .unwrap();
        let first = &out["matches"][0];
        assert_eq!(first["path"], "/v1/customer");
        assert_eq!(first["api"], "erp");
        assert_eq!(first["host"], "https://api.finance.visma.net");
        assert_eq!(first["catalog_endpoint"], "customers");
        assert!(first["query_parameters"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p == "lastModifiedDateTime"));
        assert_eq!(out["meta"]["source"], "api_reference");
    }

    #[tokio::test]
    async fn the_sales_order_api_is_searchable_and_maps_to_its_catalog_endpoint() {
        let out = tool()
            .call(json!({"query": "sales orders", "method": "GET", "api": "salesorder_v3"}))
            .await
            .unwrap();
        let first = &out["matches"][0];
        assert_eq!(first["path"], "/api/v3/SalesOrders");
        assert_eq!(first["api"], "salesorder_v3");
        assert_eq!(first["catalog_endpoint"], "sales_orders_v3");
        assert!(first["query_parameters"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p == "customerId"));
    }

    #[tokio::test]
    async fn operations_without_a_catalog_entry_say_so_and_writes_never_claim_one() {
        let out = tool()
            .call(json!({"query": "create sales order", "method": "POST", "api": "erp"}))
            .await
            .unwrap();
        let first = &out["matches"][0];
        assert_eq!(first["path"], "/v1/salesorder");
        assert_eq!(first["catalog_endpoint"], Value::Null);
    }

    #[tokio::test]
    async fn an_unmatched_question_says_how_to_retry() {
        let out = tool().call(json!({"query": "zzqx"})).await.unwrap();
        assert_eq!(out["total_matches"], 0);
        assert!(out["note"]
            .as_str()
            .unwrap()
            .contains("No operation matches"));
    }

    #[tokio::test]
    async fn bad_arguments_are_refused_before_searching() {
        assert!(matches!(
            tool().call(json!({"query": "  "})).await,
            Err(ToolError::InvalidArgs(_))
        ));
        assert!(matches!(
            tool()
                .call(json!({"query": "x".repeat(MAX_QUESTION_CHARS + 1)}))
                .await,
            Err(ToolError::InvalidArgs(_))
        ));
        assert!(matches!(
            tool().call(json!({"query": "x", "api": "no-such-api"})).await,
            Err(ToolError::InvalidArgs(m)) if m.contains("unknown api")
        ));
        assert!(matches!(
            tool()
                .call(json!({"query": "x", "limit": 0, "bogus": 1}))
                .await,
            Err(ToolError::InvalidArgs(_))
        ));
    }

    #[test]
    fn it_is_a_read_tool_that_offers_every_documented_api() {
        let def = tool().def();
        assert_eq!(def.kind, ToolKind::Read);
        assert_eq!(def.name, NAME);
        let offered = def.input_schema["properties"]["api"]["enum"]
            .as_array()
            .unwrap()
            .clone();
        assert!(
            ["erp", "journaltx_v1", "salesorder_v3"]
                .iter()
                .all(|id| offered.iter().any(|o| o == id)),
            "{offered:?}"
        );
    }
}
