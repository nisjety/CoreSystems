//! Live, read-only checks of the warehouse tools through the real MCP binary. Ignored by default:
//!   cargo test -p vsv2-mcp --test live_warehouse -- --ignored --nocapture
//! Needs `bq` authenticated (gcloud) with read access to the project below.

mod common;

use common::McpClient;
use serde_json::{json, Value};
use vsv2_mcp::warehouse::DEFAULT_PROJECT;

const PROJECT: &str = DEFAULT_PROJECT;

async fn client() -> McpClient {
    McpClient::spawn_live(&[("VSV2_MCP_BQ_PROJECT", PROJECT)]).await
}

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
#[ignore = "hits live BigQuery (read-only)"]
async fn browse_describe_and_query_the_real_gold_layer() {
    let mut c = client().await;

    let ds = c.call("warehouse_list_tables", json!({})).await;
    let datasets: Vec<&str> = data(&ds)["datasets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d.as_str().unwrap())
        .collect();
    println!("readable datasets: {datasets:?}");
    assert!(datasets.contains(&"visma_gold_finance"));
    assert!(
        !datasets
            .iter()
            .any(|d| d.contains("xero") || d.contains("excel")),
        "allow-list must hide non-Visma data"
    );

    let tables = c
        .call(
            "warehouse_list_tables",
            json!({"dataset": "visma_gold_finance"}),
        )
        .await;
    let list = data(&tables)["tables"].as_array().unwrap();
    println!("gold_finance tables: {}", list.len());
    assert!(!list.is_empty());
    let first = list[0]["table_name"].as_str().unwrap().to_string();

    let desc = c
        .call(
            "warehouse_describe_table",
            json!({"dataset": "visma_gold_finance", "table": first}),
        )
        .await;
    let d = data(&desc);
    println!(
        "{first}: {} columns, stats {}",
        d["column_count"], d["stats"]
    );
    assert!(d["column_count"].as_u64().unwrap() > 0);

    // A cheap real query with typed output.
    let q = c.call("warehouse_query", json!({
        "sql": "SELECT table_name, COUNT(*) AS n FROM `aquatiq-datavarehus.visma_gold_finance`.INFORMATION_SCHEMA.COLUMNS GROUP BY table_name ORDER BY n DESC",
        "max_rows": 3
    })).await;
    let out = data(&q);
    println!(
        "query: {} rows, ~{} bytes, truncated={}",
        out["row_count"], out["bytes_scanned_estimate"], out["truncated"]
    );
    assert!(
        out["rows"][0]["n"].is_number(),
        "INT64 must come back as a number: {out}"
    );
}

#[tokio::test]
#[ignore = "hits live BigQuery (dry-runs only)"]
async fn refuses_writes_scripts_other_datasets_and_oversized_scans_against_real_bigquery() {
    let mut c = client().await;
    let sql = |s: &str| json!({"sql": s});

    for (label, s) in [
        (
            "DML",
            "DELETE FROM `aquatiq-datavarehus.visma_gold_finance.nonexistent` WHERE TRUE",
        ),
        (
            "stacked",
            "SELECT 1; DROP TABLE `aquatiq-datavarehus.visma_gold_finance.x`",
        ),
        (
            "DDL",
            "CREATE TABLE `aquatiq-datavarehus.visma_gold_finance.evil` AS SELECT 1",
        ),
    ] {
        let e = error_text(&c.call("warehouse_query", sql(s)).await);
        println!("{label}: {e}");
        assert!(e.contains("rejected"), "{label}: {e}");
    }

    // Passes the static guard, so BigQuery's own dry-run + our dataset policy must stop it.
    let other = error_text(
        &c.call(
            "warehouse_query",
            sql("SELECT * FROM `aquatiq-datavarehus.dw_bronze_xero.INFORMATION_SCHEMA.TABLES` LIMIT 1"),
        )
        .await,
    );
    println!("other dataset: {other}");
    assert!(other.contains("not readable"), "{other}");

    let denied = error_text(
        &c.call(
            "warehouse_list_tables",
            json!({"dataset": "visma_silver_excel"}),
        )
        .await,
    );
    assert!(denied.contains("not readable"), "{denied}");
}

#[tokio::test]
#[ignore = "hits live BigQuery (dry-runs only)"]
async fn region_level_metadata_and_remote_model_calls_are_refused_against_real_bigquery() {
    let mut c = client().await;
    let sql = |s: &str| json!({"sql": s});

    // BigQuery reports region-qualified INFORMATION_SCHEMA under the dataset `region-...`, which
    // is not on the allow-list, so other users' job text is unreachable. (Verified 2026-10-09.)
    let jobs = error_text(
        &c.call("warehouse_query", sql("SELECT query FROM `region-europe-north2`.INFORMATION_SCHEMA.JOBS_BY_PROJECT LIMIT 1")).await,
    );
    println!("jobs: {jobs}");
    assert!(jobs.contains("not readable"), "{jobs}");

    let schemata = error_text(
        &c.call("warehouse_query", sql("SELECT * FROM `aquatiq-datavarehus`.`region-europe-north2`.INFORMATION_SCHEMA.SCHEMATA LIMIT 1")).await,
    );
    assert!(schemata.contains("not readable"), "{schemata}");

    // Model-calling functions are refused before BigQuery is even asked.
    let ml = error_text(&c.call("warehouse_query", sql("SELECT ML.GENERATE_TEXT(MODEL `aquatiq-datavarehus.visma_gold_finance.m`, (SELECT 'x' AS prompt))")).await);
    assert!(
        ml.contains("query rejected") && ml.contains("ML.GENERATE_TEXT"),
        "{ml}"
    );

    // The same dataset-qualified metadata inside an allowed dataset still works.
    let ok = c.call("warehouse_query", sql("SELECT COUNT(*) AS n FROM `aquatiq-datavarehus.visma_gold_finance`.INFORMATION_SCHEMA.TABLES")).await;
    assert!(data(&ok)["rows"][0]["n"].is_number());
}

#[tokio::test]
#[ignore = "hits live BigQuery (metadata only)"]
async fn warehouse_overview_reports_real_volumes_and_recency_from_metadata_only() {
    let mut c = client().await;
    let all = c.call("warehouse_overview", json!({})).await;
    let d = data(&all);
    let datasets = d["datasets"].as_array().unwrap();
    println!(
        "overview: {} datasets, failed: {}",
        datasets.len(),
        d["datasets_failed"]
    );
    assert!(datasets.len() >= 5, "{d}");
    assert!(d["datasets_failed"].as_array().unwrap().is_empty(), "{d}");
    assert!(
        !datasets
            .iter()
            .any(|x| x["dataset"].as_str().unwrap().contains("xero")),
        "allow-list holds"
    );
    let fin = datasets
        .iter()
        .find(|x| x["dataset"] == "visma_gold_finance")
        .expect("gold_finance summarised");
    println!(
        "gold_finance: {} tables, {} rows, newest {}",
        fin["table_count"], fin["total_rows"], fin["newest_modified"]
    );
    assert!(fin["total_rows"].as_i64().unwrap() > 1000);
    assert!(fin["newest_modified"].is_string());
    assert_eq!(d["meta"]["coverage"], "metadata_only");

    let one = c
        .call(
            "warehouse_overview",
            json!({"dataset": "visma_gold_finance"}),
        )
        .await;
    let tables = data(&one)["datasets"][0]["tables"].as_array().unwrap();
    assert!(tables
        .iter()
        .any(|t| t["name"] == "dim_account" && t["rows"].as_i64().unwrap() > 100));

    let denied = c
        .call("warehouse_overview", json!({"dataset": "visma_assertions"}))
        .await;
    assert_eq!(denied["result"]["isError"], true);
}

#[tokio::test]
#[ignore = "hits live BigQuery"]
async fn a_big_query_result_is_parked_and_pageable_over_the_real_protocol() {
    let mut c = client().await;
    // Wide rows (600-char filler) so 500 of them exceed the inline cap and must be parked.
    let q = c
        .call(
            "warehouse_query",
            json!({
                "sql": "SELECT table_name, column_name, ordinal_position, data_type, REPEAT('0123456789', 60) AS filler \
                        FROM `aquatiq-datavarehus.visma_gold_finance`.INFORMATION_SCHEMA.COLUMNS ORDER BY table_name, ordinal_position",
                "max_rows": 500
            }),
        )
        .await;
    let out = data(&q);
    assert_eq!(
        out["result_truncated"], true,
        "a >200KB answer must be parked, not inlined or cut: {out}"
    );
    let id = out["result_id"].as_str().unwrap();
    println!(
        "parked {} bytes; shape rows len {}",
        out["size_bytes"], out["shape"]["fields"]["rows"]["len"]
    );
    assert_eq!(out["shape"]["fields"]["rows"]["type"], "array");

    // Walk every row through the paging tool: nothing is lost or reordered.
    let mut offset = 0u64;
    let mut seen = 0u64;
    let mut last_key = String::new();
    loop {
        let page = c
            .call(
                "visma_read_result",
                json!({"result_id": id, "pointer": "/rows", "offset": offset, "limit": 200}),
            )
            .await;
        let p = data(&page);
        for row in p["items"].as_array().unwrap() {
            let key = format!(
                "{}.{:05}",
                row["table_name"].as_str().unwrap(),
                row["ordinal_position"].as_i64().unwrap()
            );
            assert!(key >= last_key, "rows stay in query order");
            last_key = key;
            seen += 1;
        }
        match p["next_offset"].as_u64() {
            Some(n) => offset = n,
            None => break,
        }
    }
    println!("paged {seen} rows");
    assert!(seen > 100);
    // The meta block travelled with the parked result.
    let meta = c
        .call(
            "visma_read_result",
            json!({"result_id": id, "pointer": "/meta"}),
        )
        .await;
    assert_eq!(data(&meta)["value"]["source"], "bigquery");
}
