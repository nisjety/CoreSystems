//! Live, READ-ONLY checks of the Visma tools through the real MCP binary (uses `.env` credentials
//! and the local sync-state Postgres). Ignored by default:
//!   cargo test -p vsv2-mcp --test live_visma -- --ignored --nocapture

mod common;

use common::McpClient;
use serde_json::{json, Value};

fn data(r: &Value) -> &Value {
    assert_ne!(r["result"]["isError"], true, "tool failed: {r}");
    &r["result"]["structuredContent"]
}

fn error_text(r: &Value) -> String {
    assert_eq!(r["result"]["isError"], true, "expected a tool error: {r}");
    r["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

#[tokio::test]
#[ignore = "hits live Visma (read-only)"]
async fn the_visma_tools_are_registered_when_credentials_exist() {
    let mut c = McpClient::spawn_live(&[]).await;
    let names: Vec<String> = c
        .tools()
        .await
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    println!("tools: {names:?}");
    for n in [
        "visma_list_tenants",
        "visma_erp_query",
        "visma_salesorder_get",
        "visma_datamart_schema",
        "visma_datamart_query",
        "visma_sync_status",
    ] {
        assert!(
            names.iter().any(|x| x == n),
            "{n} not registered: {names:?}"
        );
    }
}

#[tokio::test]
#[ignore = "hits live Visma (read-only)"]
async fn tenants_erp_salesorder_and_status_against_the_real_systems() {
    let mut c = McpClient::spawn_live(&[]).await;

    let t = c.call("visma_list_tenants", json!({})).await;
    let tenants = data(&t)["tenants"].as_array().unwrap();
    println!("tenants: {}", tenants.len());
    assert!(tenants.len() >= 10);
    assert!(tenants.iter().any(|x| x["is_default"] == true));

    // ERP REST: a Finance page and a single record by key
    let page = c
        .call(
            "visma_erp_query",
            json!({"endpoint": "customers", "page_size": 2}),
        )
        .await;
    let p = data(&page);
    println!(
        "customers page: {} rows, maybe_more={}",
        p["row_count"], p["maybe_more"]
    );
    assert_eq!(p["row_count"], 2);
    let key = p["rows"][0]["customerNumber"]
        .as_str()
        .or_else(|| p["rows"][0]["number"].as_str())
        .map(String::from);
    if let Some(k) = key {
        let one = c
            .call(
                "visma_erp_query",
                json!({"endpoint": "customers", "key": k}),
            )
            .await;
        assert!(data(&one)["record"].is_object(), "{one}");
    }

    // SalesOrder V3: list page (zero-based index handled), then single order with ETag
    let so = c
        .call(
            "visma_erp_query",
            json!({"endpoint": "sales_orders_v3", "page_size": 1, "query": {"expand": "lines"}}),
        )
        .await;
    let s = data(&so);
    println!(
        "sales_orders_v3: totalCount={} rows={}",
        s["totalCount"], s["row_count"]
    );
    assert!(s["totalCount"].as_u64().unwrap() > 1000);
    let (ty, id) = (
        s["rows"][0]["type"].as_str().unwrap(),
        s["rows"][0]["orderId"].as_str().unwrap(),
    );
    let one = c
        .call(
            "visma_salesorder_get",
            json!({"type": ty, "orderId": id, "expand": ["lines"]}),
        )
        .await;
    let o = data(&one);
    println!("order {ty}/{id}: etag={}", o["etag"]);
    assert!(
        o["etag"].as_str().unwrap_or("").len() > 4,
        "real ETag expected"
    );
    assert_eq!(o["order"]["orderId"], id);

    // Sync state (local dev Postgres)
    let st = c.call("visma_sync_status", json!({"limit": 3})).await;
    let d = data(&st);
    println!(
        "sync: {} checkpoints, runs_last_24h={}",
        d["checkpoints"].as_array().unwrap().len(),
        d["runs_last_24h"]
    );
    assert!(!d["checkpoints"].as_array().unwrap().is_empty());
    assert!(d["checkpoints"][0]["age_hours"].is_number());
    let failed = c
        .call(
            "visma_sync_status",
            json!({"run_status": "failed", "limit": 2}),
        )
        .await;
    for r in data(&failed)["recent_runs"].as_array().unwrap() {
        assert_eq!(r["status"], "failed");
    }
}

#[tokio::test]
#[ignore = "hits live Visma DataMart (read-only)"]
async fn datamart_schema_and_query_work_and_mutations_are_refused() {
    let mut c = McpClient::spawn_live(&[]).await;
    let root = c.call("visma_datamart_schema", json!({})).await;
    let fields = data(&root)["root_fields"].as_array().unwrap();
    println!("datamart root fields: {}", fields.len());
    assert!(!fields.is_empty());

    let m = c
        .call("visma_datamart_query", json!({"query": "mutation { x }"}))
        .await;
    assert!(error_text(&m).contains("query rejected"));

    let q = c
        .call("visma_datamart_query", json!({"query": "{ __typename }"}))
        .await;
    assert_eq!(data(&q)["data"]["__typename"], "Query");
}

#[tokio::test]
#[ignore = "hits live Visma (read-only)"]
async fn bad_requests_fail_cleanly_without_reaching_visma() {
    let mut c = McpClient::spawn_live(&[]).await;
    assert!(error_text(
        &c.call(
            "visma_salesorder_get",
            json!({"type": "SO", "orderId": "999999999"})
        )
        .await
    )
    .contains("not found"));
    let r = c
        .call(
            "visma_erp_query",
            json!({"endpoint": "customers", "tenant": "not-a-tenant"}),
        )
        .await;
    assert!(
        r.get("error").is_some(),
        "unknown tenant is an invalid-params protocol error: {r}"
    );
}

#[tokio::test]
#[ignore = "hits live Visma (read-only)"]
async fn every_live_read_carries_provenance_and_never_claims_completeness() {
    let mut c = McpClient::spawn_live(&[]).await;
    let erp = c
        .call(
            "visma_erp_query",
            json!({"endpoint": "customers", "page_size": 1}),
        )
        .await;
    let m = &data(&erp)["meta"];
    println!("erp meta: {m}");
    assert_eq!(
        (
            m["source"].as_str(),
            m["coverage"].as_str(),
            m["complete_collection"].as_bool()
        ),
        (Some("erp_rest"), Some("single_page"), Some(false))
    );
    assert!(chrono::DateTime::parse_from_rfc3339(m["fetched_at"].as_str().unwrap()).is_ok());
    assert!(m["tenant_id"].is_string() && m["path"].is_string());

    let dm = c
        .call("visma_datamart_query", json!({"query": "{ __typename }"}))
        .await;
    assert_eq!(data(&dm)["meta"]["source"], "datamart_graphql");

    let st = c.call("visma_sync_status", json!({"limit": 1})).await;
    assert_eq!(data(&st)["meta"]["source"], "sync_state_db");

    let ep = c
        .call("visma_list_endpoints", json!({"endpoint": "customers"}))
        .await;
    assert_eq!(data(&ep)["endpoint"]["queryable_live"], true);
}
