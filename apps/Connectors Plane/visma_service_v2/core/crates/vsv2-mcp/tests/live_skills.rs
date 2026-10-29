//! Live accuracy checks for the bundled skills, so a skill cannot silently rot when the warehouse
//! or Visma changes. Ignored by default (they need credentials and the `bq` CLI):
//!
//! ```text
//! cargo test -p vsv2-mcp --test live_skills -- --ignored --nocapture
//! ```
//!
//! * every SQL example is dry-run against real BigQuery (free): it must parse, name only columns
//!   and tables that exist, be a SELECT, touch only readable datasets and stay under the byte cap;
//! * every read-only Visma example runs against the real systems (a handful of calls).

mod common;

use common::McpClient;
use serde_json::{json, Value};
use vsv2_mcp::skills::SkillIndex;
use vsv2_mcp::warehouse::{
    validate_sql, BqCli, DatasetPolicy, Limits, Warehouse, DEFAULT_DATASETS, DEFAULT_PROJECT,
};

const PROJECT: &str = DEFAULT_PROJECT;

/// Read tools whose examples still cannot run as written: they hold placeholders for runtime state
/// (`visma_read_result` needs the handle of an earlier answer) or illustrative ids
/// (`visma_salesorder_get`; `live_visma.rs` reads a real order). Which tools are safe to run at all is
/// decided by the server, not by a list here: only tools it annotates `readOnlyHint` are run, so a
/// future write-capable tool is never executed by this test.
fn is_placeholder(tool: &str) -> bool {
    matches!(tool, "visma_read_result" | "visma_salesorder_get")
}

#[tokio::test]
#[ignore = "dry-runs every SQL example against live BigQuery (free, read-only)"]
async fn every_sql_example_dry_runs_clean_against_real_bigquery() {
    let bq = BqCli::new(PROJECT);
    let policy = DatasetPolicy::parse(DEFAULT_DATASETS);
    let limits = Limits::default();
    let mut checked = 0;
    let mut failures: Vec<String> = Vec::new();

    for skill in SkillIndex::embedded().all() {
        for ex in skill.examples().expect("examples parse") {
            if ex.tool != "warehouse_query" {
                continue;
            }
            let label = format!("{}:{}", skill.id, ex.line);
            let sql = match validate_sql(ex.args["sql"].as_str().unwrap_or_default()) {
                Ok(s) => s,
                Err(e) => {
                    failures.push(format!("{label}: refused by the SQL guard: {e}"));
                    continue;
                }
            };
            checked += 1;
            match bq.dry_run(&sql).await {
                Err(e) => failures.push(format!("{label}: BigQuery rejected it: {e}")),
                Ok(info) => {
                    if info.statement_type != "SELECT" {
                        failures.push(format!("{label}: statement type {}", info.statement_type));
                    }
                    if info.total_bytes > limits.max_bytes {
                        failures.push(format!(
                            "{label}: would scan {} bytes, over the {} byte cap",
                            info.total_bytes, limits.max_bytes
                        ));
                    }
                    for t in &info.referenced {
                        if t.project != PROJECT || !policy.allows(&t.dataset) {
                            failures.push(format!(
                                "{label}: references {}.{}.{}, which the server would refuse",
                                t.project, t.dataset, t.table
                            ));
                        }
                    }
                }
            }
        }
    }
    println!("dry-ran {checked} SQL examples");
    assert!(
        checked >= 40,
        "expected the bundled SQL examples, saw {checked}"
    );
    assert!(
        failures.is_empty(),
        "skill examples that no longer hold:\n{}",
        failures.join("\n")
    );
}

#[tokio::test]
#[ignore = "runs the read-only Visma examples of every skill against the live systems"]
async fn read_only_visma_examples_run_against_the_real_systems() {
    let mut c = McpClient::spawn_live(&[]).await;
    let read_only: std::collections::BTreeSet<String> = c
        .tools()
        .await
        .iter()
        .filter(|t| t["annotations"]["readOnlyHint"] == true)
        .filter_map(|t| t["name"].as_str().map(String::from))
        .collect();
    let mut ran = 0;
    let mut failures: Vec<String> = Vec::new();

    for skill in SkillIndex::embedded().all() {
        for ex in skill.examples().expect("examples parse") {
            if ex.tool == "warehouse_query"
                || ex.tool.starts_with("warehouse_")
                || is_placeholder(&ex.tool)
                || !read_only.contains(&ex.tool)
            {
                continue;
            }
            let r = c.call(&ex.tool, ex.args.clone()).await;
            ran += 1;
            let failed = r.get("error").is_some() || r["result"]["isError"] == true;
            if failed {
                failures.push(format!(
                    "{}:{} {} -> {}",
                    skill.id,
                    ex.line,
                    ex.tool,
                    short(&r)
                ));
            }
        }
    }
    println!("ran {ran} live examples");
    assert!(ran >= 15, "expected the bundled live examples, ran {ran}");
    assert!(
        failures.is_empty(),
        "live examples that no longer work:\n{}",
        failures.join("\n")
    );
}

#[tokio::test]
#[ignore = "runs the warehouse examples through the real tool, with the real guard and BigQuery"]
async fn warehouse_examples_run_through_the_real_tool_and_return_rows() {
    let mut c = McpClient::spawn_live(&[("VSV2_MCP_BQ_PROJECT", PROJECT)]).await;
    let mut ran = 0;
    let mut empty: Vec<String> = Vec::new();
    let mut failures: Vec<String> = Vec::new();

    for skill in SkillIndex::embedded().all() {
        for ex in skill.examples().expect("examples parse") {
            if !ex.tool.starts_with("warehouse_") {
                continue;
            }
            let r = c.call(&ex.tool, ex.args.clone()).await;
            ran += 1;
            if r.get("error").is_some() || r["result"]["isError"] == true {
                failures.push(format!(
                    "{}:{} {} -> {}",
                    skill.id,
                    ex.line,
                    ex.tool,
                    short(&r)
                ));
                continue;
            }
            let d = &r["result"]["structuredContent"];
            let rows = d["rows"]
                .as_array()
                .map(Vec::len)
                .or_else(|| d["datasets"].as_array().map(Vec::len));
            if rows == Some(0) {
                empty.push(format!("{}:{}", skill.id, ex.line));
            }
        }
    }
    println!("ran {ran} warehouse examples; empty results: {empty:?}");
    assert!(
        failures.is_empty(),
        "warehouse examples that fail:\n{}",
        failures.join("\n")
    );
    // An empty result is the good outcome for a few checks (for example batches that do not
    // balance in a period); more than a handful means a filter no longer matches the data.
    assert!(empty.len() <= 3, "examples returning nothing: {empty:?}");
}

/// At most 300 characters (cut on a character boundary: Visma error text holds "ø" and friends).
fn short(v: &Value) -> String {
    let s = v.to_string();
    match s.char_indices().nth(300) {
        Some((i, _)) => format!("{}...", &s[..i]),
        None => s,
    }
}

#[test]
fn the_skill_examples_use_a_known_json_shape() {
    // Cheap offline guard so this file is not dead weight when the live tests are ignored.
    let n: usize = SkillIndex::embedded()
        .all()
        .iter()
        .map(|s| s.examples().expect("examples parse").len())
        .sum();
    assert!(
        n >= 60,
        "expected at least 60 worked examples in the bundled skills, found {n}"
    );
    let _ = json!({});
}

#[test]
fn short_cuts_on_a_character_boundary() {
    let long = json!({"error": "ø".repeat(400)});
    let t = short(&long);
    assert!(
        t.ends_with("...") && t.chars().count() <= 304,
        "{}",
        t.chars().count()
    );
    assert_eq!(short(&json!({"a": 1})), r#"{"a":1}"#);
}
