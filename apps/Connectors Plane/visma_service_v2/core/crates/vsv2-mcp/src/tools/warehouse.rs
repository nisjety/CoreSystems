use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::meta::meta;
use crate::tool::{parse_args, Tool, ToolDef, ToolError, ToolKind};
use crate::warehouse::{QueryOutput, SafeWarehouse, WarehouseError};

pub const LIST_TABLES: &str = "warehouse_list_tables";
pub const DESCRIBE_TABLE: &str = "warehouse_describe_table";
pub const QUERY: &str = "warehouse_query";

/// Metadata queries return more rows than an ad-hoc query is allowed to.
const METADATA_ROWS: usize = 1000;

fn map_err(e: WarehouseError) -> ToolError {
    match e {
        // The SQL itself is the caller's to fix.
        WarehouseError::Guard(g) => ToolError::Failed(format!("query rejected: {g}")),
        other => ToolError::Failed(other.to_string()),
    }
}

/// Letters, digits and underscores only: safe to inline into backticked identifiers/literals.
fn ident(label: &str, v: &str) -> Result<(), ToolError> {
    let ok =
        !v.is_empty() && v.len() <= 128 && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    ok.then_some(()).ok_or_else(|| {
        ToolError::InvalidArgs(format!(
            "{label} must be 1-128 chars of A-Z a-z 0-9 _, got {v:?}"
        ))
    })
}

fn output_json(o: QueryOutput, project: &str) -> Value {
    json!({
        "meta": meta(
            "bigquery",
            None,
            json!({"project": project, "bytes_scanned_estimate": o.bytes_estimate,
                   "complete_result": !o.truncated, "truncated": o.truncated}),
        ),
        "columns": o.columns.iter().map(|(n, t)| json!({"name": n, "type": t})).collect::<Vec<_>>(),
        "row_count": o.rows.len(),
        "truncated": o.truncated,
        "bytes_scanned_estimate": o.bytes_estimate,
        "rows": o.rows,
    })
}

fn datasets_hint(w: &SafeWarehouse) -> String {
    w.policy().patterns().join(", ")
}

// ---------------------------------------------------------------- list tables

pub struct ListTables(pub SafeWarehouse);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListArgs {
    #[serde(default)]
    dataset: Option<String>,
}

#[async_trait]
impl Tool for ListTables {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: LIST_TABLES,
            description: format!(
                "Browse the BigQuery warehouse (project {}). Without `dataset`: lists the readable \
                 datasets. With `dataset`: lists its tables/views. Readable datasets: {}. Gold \
                 datasets (visma_gold_*) and visma_datamart (the BI_* reporting models) hold the \
                 curated data; prefer them over silver, bronze and the raw staging.",
                self.0.project(),
                datasets_hint(&self.0)
            ),
            input_schema: json!({
                "type": "object",
                "properties": {"dataset": {"type": "string", "description": "e.g. visma_gold_finance"}},
                "additionalProperties": false
            }),
            kind: ToolKind::Read,
        }
    }

    async fn call(&self, args: Value) -> Result<Value, ToolError> {
        let a: ListArgs = parse_args(args)?;
        let Some(ds) = a.dataset else {
            let datasets = self.0.allowed_datasets().await.map_err(map_err)?;
            return Ok(json!({"project": self.0.project(), "datasets": datasets}));
        };
        ident("dataset", &ds)?;
        if !self.0.policy().allows(&ds) {
            return Err(ToolError::Failed(format!(
                "dataset {ds} is not readable here (allowed: {})",
                datasets_hint(&self.0)
            )));
        }
        let sql = format!(
            "SELECT table_name, table_type FROM `{}.{ds}`.INFORMATION_SCHEMA.TABLES ORDER BY table_name",
            self.0.project()
        );
        let out = self
            .0
            .select(&sql, Some(METADATA_ROWS), Some(METADATA_ROWS))
            .await
            .map_err(map_err)?;
        Ok(
            json!({"project": self.0.project(), "dataset": ds, "truncated": out.truncated, "tables": out.rows}),
        )
    }
}

// ---------------------------------------------------------------- describe table

pub struct DescribeTable(pub SafeWarehouse);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DescribeArgs {
    dataset: String,
    table: String,
}

#[async_trait]
impl Tool for DescribeTable {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: DESCRIBE_TABLE,
            description:
                "Columns (including nested fields), types and descriptions of a warehouse \
                table or view, plus row count and size when available. Use before writing SQL."
                    .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "dataset": {"type": "string"},
                    "table": {"type": "string"}
                },
                "required": ["dataset", "table"],
                "additionalProperties": false
            }),
            kind: ToolKind::Read,
        }
    }

    async fn call(&self, args: Value) -> Result<Value, ToolError> {
        let a: DescribeArgs = parse_args(args)?;
        ident("dataset", &a.dataset)?;
        ident("table", &a.table)?;
        if !self.0.policy().allows(&a.dataset) {
            return Err(ToolError::Failed(format!(
                "dataset {} is not readable here (allowed: {})",
                a.dataset,
                datasets_hint(&self.0)
            )));
        }
        let p = self.0.project();
        let cols_sql = format!(
            "SELECT field_path AS column_name, data_type, description \
             FROM `{p}.{}`.INFORMATION_SCHEMA.COLUMN_FIELD_PATHS WHERE table_name = '{}' ORDER BY 1",
            a.dataset, a.table
        );
        let cols = self
            .0
            .select(&cols_sql, Some(METADATA_ROWS), Some(METADATA_ROWS))
            .await
            .map_err(map_err)?;
        if cols.rows.is_empty() {
            return Err(ToolError::Failed(format!(
                "table {}.{} not found or has no columns",
                a.dataset, a.table
            )));
        }
        // Best effort: `__TABLES__` is the only cheap source of row count and size.
        let stats_sql = format!(
            "SELECT row_count, size_bytes, TIMESTAMP_MILLIS(last_modified_time) AS last_modified \
             FROM `{p}.{}.__TABLES__` WHERE table_id = '{}'",
            a.dataset, a.table
        );
        let stats = self
            .0
            .select(&stats_sql, Some(1), Some(1))
            .await
            .ok()
            .and_then(|o| o.rows.into_iter().next());
        Ok(json!({
            "dataset": a.dataset,
            "table": a.table,
            "column_count": cols.rows.len(),
            "stats": stats,
            "columns": cols.rows,
        }))
    }
}

// ---------------------------------------------------------------- query

pub struct Query(pub SafeWarehouse);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryArgs {
    sql: String,
    #[serde(default)]
    max_rows: Option<usize>,
}

#[async_trait]
impl Tool for Query {
    fn def(&self) -> ToolDef {
        let l = self.0.limits;
        ToolDef {
            name: QUERY,
            description: format!(
                "Run a read-only BigQuery Standard SQL query (single SELECT/WITH statement) against \
                 the warehouse. Fully qualify tables as `{}.dataset.table`. Readable datasets: {}. \
                 Limits: at most {} rows (default {}), at most {} bytes scanned; DML/DDL, scripts and \
                 other datasets are refused. Discover tables with {LIST_TABLES} and columns with \
                 {DESCRIBE_TABLE}; filter on partition/date columns and select only needed columns.",
                self.0.project(),
                datasets_hint(&self.0),
                l.max_rows,
                l.default_rows,
                l.max_bytes
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sql": {"type": "string", "description": "one SELECT or WITH query"},
                    "max_rows": {"type": "integer", "minimum": 1, "description": "rows to return"}
                },
                "required": ["sql"],
                "additionalProperties": false
            }),
            kind: ToolKind::Read,
        }
    }

    async fn call(&self, args: Value) -> Result<Value, ToolError> {
        let a: QueryArgs = parse_args(args)?;
        let out = self
            .0
            .select(&a.sql, a.max_rows, None)
            .await
            .map_err(map_err)?;
        Ok(output_json(out, self.0.project()))
    }
}

// ---------------------------------------------------------------- overview

pub const OVERVIEW: &str = "warehouse_overview";

pub struct Overview(pub SafeWarehouse);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OverviewArgs {
    /// Focus on one dataset (and list its tables). Omit for one summary line per readable dataset.
    #[serde(default)]
    dataset: Option<String>,
}

fn dataset_meta_sql(project: &str, dataset: &str) -> String {
    // `__TABLES__` is metadata only (no table scan): row count, size and last-modified per table.
    format!(
        "SELECT table_id, type, row_count, size_bytes, \
         TIMESTAMP_MILLIS(last_modified_time) AS last_modified \
         FROM `{project}.{dataset}.__TABLES__` ORDER BY table_id"
    )
}

fn summarize_dataset(dataset: &str, rows: &[Value], list_tables: bool) -> Value {
    let num = |r: &Value, k: &str| r[k].as_i64().unwrap_or(0);
    let tables: Vec<&Value> = rows.iter().filter(|r| num(r, "type") == 1).collect();
    let views = rows.len() - tables.len();
    let newest = rows
        .iter()
        .filter_map(|r| r["last_modified"].as_str())
        .max();
    let mut out = json!({
        "dataset": dataset,
        "table_count": tables.len(),
        "view_count": views,
        "total_rows": tables.iter().map(|r| num(r, "row_count")).sum::<i64>(),
        "total_bytes": tables.iter().map(|r| num(r, "size_bytes")).sum::<i64>(),
        "newest_modified": newest,
    });
    if list_tables {
        out["tables"] = Value::Array(
            rows.iter()
                .map(|r| json!({
                    "name": r["table_id"], "kind": if num(r, "type") == 1 { "table" } else { "view" },
                    "rows": r["row_count"], "bytes": r["size_bytes"], "last_modified": r["last_modified"],
                }))
                .collect(),
        );
    }
    out
}

#[async_trait]
impl Tool for Overview {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: OVERVIEW,
            description: format!(
                "Volume and recency overview of the warehouse (project {}): per readable dataset the \
                 table/view count, total rows, size and the most recent table modification; with \
                 `dataset`, every table's row count and last-modified time. Metadata only, no data is \
                 read. It shows how big and how recently written things are, not whether they are \
                 correct or complete (see {} for the sync feed). Readable datasets: {}.",
                self.0.project(),
                crate::tools::sync_status::NAME,
                datasets_hint(&self.0)
            ),
            input_schema: json!({
                "type": "object",
                "properties": {"dataset": {"type": "string"}},
                "additionalProperties": false
            }),
            kind: ToolKind::Read,
        }
    }

    async fn call(&self, args: Value) -> Result<Value, ToolError> {
        let a: OverviewArgs = parse_args(args)?;
        let datasets = match &a.dataset {
            Some(d) => {
                ident("dataset", d)?;
                if !self.0.policy().allows(d) {
                    return Err(ToolError::Failed(format!(
                        "dataset {d} is not readable here (allowed: {})",
                        datasets_hint(&self.0)
                    )));
                }
                vec![d.clone()]
            }
            None => self.0.allowed_datasets().await.map_err(map_err)?,
        };
        let list_tables = a.dataset.is_some();

        let mut set = tokio::task::JoinSet::new();
        for ds in datasets {
            let w = self.0.clone();
            let sql = dataset_meta_sql(w.project(), &ds);
            set.spawn(async move {
                let r = w
                    .select(&sql, Some(METADATA_ROWS), Some(METADATA_ROWS))
                    .await;
                (ds, r)
            });
        }
        let mut summaries = Vec::new();
        let mut failed: Vec<Value> = Vec::new();
        while let Some(joined) = set.join_next().await {
            match joined {
                Ok((ds, Ok(out))) => {
                    let mut s = summarize_dataset(&ds, &out.rows, list_tables);
                    s["listing_truncated"] = json!(out.truncated);
                    summaries.push(s);
                }
                Ok((ds, Err(e))) => failed.push(json!({"dataset": ds, "error": e.to_string()})),
                Err(e) => failed.push(json!({"error": format!("task failed: {e}")})),
            }
        }
        summaries.sort_by(|a, b| a["dataset"].as_str().cmp(&b["dataset"].as_str()));
        Ok(json!({
            "project": self.0.project(),
            "datasets": summaries,
            "datasets_failed": failed,
            "meta": meta("bigquery", None, json!({
                "project": self.0.project(),
                "coverage": "metadata_only",
                "note": "Row counts and modification times are volume/recency indicators, not completeness or correctness checks. A failed dataset is unknown, not empty.",
            })),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::warehouse::fake::*;
    use crate::warehouse::{DatasetPolicy, Limits};

    fn tools(
        rows: Vec<Value>,
        tables: &[(&str, &str)],
    ) -> (SafeWarehouse, std::sync::Arc<FakeWarehouse>) {
        let fake = FakeWarehouse::new(
            select_info(
                tables,
                100,
                vec![field("table_name", "STRING"), field("n", "INTEGER")],
            ),
            rows,
        );
        (
            SafeWarehouse::new(
                fake.clone(),
                DatasetPolicy::parse("visma_gold_*"),
                Limits::default(),
            ),
            fake,
        )
    }

    #[tokio::test]
    async fn list_without_dataset_returns_only_readable_existing_datasets() {
        let (w, _) = tools(vec![], &[]);
        let out = ListTables(w).call(json!({})).await.unwrap();
        assert_eq!(
            out["datasets"],
            json!(["visma_gold_finance", "visma_gold_sales"])
        ); // secret_ds filtered
    }

    #[tokio::test]
    async fn list_with_dataset_queries_information_schema_and_enforces_policy_and_identifiers() {
        let (w, fake) = tools(
            vec![json!({"table_name": "fact_gl", "table_type": "BASE TABLE"})],
            &[("visma_gold_finance", "INFORMATION_SCHEMA.TABLES")],
        );
        let out = ListTables(w.clone())
            .call(json!({"dataset": "visma_gold_finance"}))
            .await
            .unwrap();
        assert_eq!(out["tables"][0]["table_name"], "fact_gl");
        assert!(fake.ran.lock().unwrap()[0]
            .0
            .contains("`proj.visma_gold_finance`.INFORMATION_SCHEMA.TABLES"));

        let denied = ListTables(w.clone())
            .call(json!({"dataset": "secret_ds"}))
            .await
            .unwrap_err();
        assert!(denied.to_string().contains("not readable"));
        let injected = ListTables(w)
            .call(json!({"dataset": "x`; DROP"}))
            .await
            .unwrap_err();
        assert!(matches!(injected, ToolError::InvalidArgs(_)));
    }

    #[tokio::test]
    async fn describe_returns_columns_and_best_effort_stats_and_blocks_bad_names() {
        let (w, fake) = tools(
            vec![json!({"column_name": "id", "data_type": "INT64", "description": null})],
            &[("visma_gold_finance", "t")],
        );
        let out = DescribeTable(w.clone())
            .call(json!({"dataset": "visma_gold_finance", "table": "fact_gl"}))
            .await
            .unwrap();
        assert_eq!(out["column_count"], 1);
        assert_eq!(out["columns"][0]["column_name"], "id");
        let first_sql = fake.ran.lock().unwrap()[0].0.clone();
        assert!(
            first_sql.contains("COLUMN_FIELD_PATHS")
                && first_sql.contains("table_name = 'fact_gl'")
        );

        for bad in [
            json!({"dataset": "visma_gold_finance", "table": "t' OR '1'='1"}),
            json!({"dataset": "d", "table": "a b"}),
        ] {
            assert!(matches!(
                DescribeTable(w.clone()).call(bad).await,
                Err(ToolError::InvalidArgs(_))
            ));
        }
        assert!(DescribeTable(w)
            .call(json!({"dataset": "secret_ds", "table": "t"}))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn describe_of_a_missing_table_is_a_clear_failure() {
        let (w, _) = tools(vec![], &[("visma_gold_finance", "t")]);
        let e = DescribeTable(w)
            .call(json!({"dataset": "visma_gold_finance", "table": "nope"}))
            .await
            .unwrap_err();
        assert!(e.to_string().contains("not found"));
    }

    #[tokio::test]
    async fn query_returns_typed_rows_and_surfaces_guard_and_policy_refusals_as_tool_errors() {
        let (w, _) = tools(
            vec![json!({"table_name": "a", "n": "5"})],
            &[("visma_gold_finance", "t")],
        );
        let out = Query(w.clone())
            .call(json!({"sql": "SELECT table_name, n FROM `proj.visma_gold_finance.t`"}))
            .await
            .unwrap();
        assert_eq!(out["rows"][0], json!({"table_name": "a", "n": 5}));
        assert_eq!(out["columns"][1], json!({"name": "n", "type": "INTEGER"}));
        assert_eq!(out["truncated"], false);

        let guard = Query(w.clone())
            .call(json!({"sql": "DELETE FROM t WHERE TRUE"}))
            .await
            .unwrap_err();
        assert!(
            matches!(&guard, ToolError::Failed(m) if m.starts_with("query rejected")),
            "{guard}"
        );

        let (w2, _) = tools(vec![], &[("secret_ds", "t")]);
        let policy = Query(w2)
            .call(json!({"sql": "SELECT 1"}))
            .await
            .unwrap_err();
        assert!(policy.to_string().contains("not readable"));
    }

    #[tokio::test]
    async fn query_description_tells_the_model_the_project_datasets_and_limits() {
        let (w, _) = tools(vec![], &[]);
        let d = Query(w).def();
        assert!(
            d.description.contains("proj.dataset.table") && d.description.contains("visma_gold_*")
        );
        assert_eq!(d.kind, ToolKind::Read);
    }

    #[tokio::test]
    async fn overview_summarises_each_readable_dataset_from_metadata_only() {
        let rows = vec![
            json!({"table_id": "dim_a", "type": "1", "row_count": "10", "size_bytes": "100", "last_modified": "2026-10-01 10:00:00"}),
            json!({"table_id": "fact_b", "type": "1", "row_count": "5", "size_bytes": "50", "last_modified": "2026-10-09 09:00:00"}),
            json!({"table_id": "v_c", "type": "2", "row_count": null, "size_bytes": null, "last_modified": "2026-10-05 00:00:00"}),
        ];
        let schema = vec![
            field("table_id", "STRING"),
            field("type", "INTEGER"),
            field("row_count", "INTEGER"),
            field("size_bytes", "INTEGER"),
            field("last_modified", "TIMESTAMP"),
        ];
        let fake = FakeWarehouse::new(
            select_info(&[("visma_gold_finance", "__TABLES__")], 10, schema),
            rows,
        );
        let w = SafeWarehouse::new(
            fake.clone(),
            DatasetPolicy::parse("visma_gold_*"),
            Limits::default(),
        );

        let out = Overview(w.clone()).call(json!({})).await.unwrap();
        let ds = out["datasets"].as_array().unwrap();
        assert_eq!(
            ds.len(),
            2,
            "secret_ds is not readable, so it is not summarised"
        );
        assert_eq!(ds[0]["dataset"], "visma_gold_finance");
        assert_eq!(
            (
                ds[0]["table_count"].as_u64(),
                ds[0]["view_count"].as_u64(),
                ds[0]["total_rows"].as_i64()
            ),
            (Some(2), Some(1), Some(15))
        );
        assert_eq!(ds[0]["newest_modified"], "2026-10-09 09:00:00");
        assert!(
            ds[0].get("tables").is_none(),
            "summary mode does not list tables"
        );
        assert!(
            fake.ran
                .lock()
                .unwrap()
                .iter()
                .all(|(sql, _, _)| sql.contains("__TABLES__")),
            "metadata queries only"
        );
        assert_eq!(out["meta"]["coverage"], "metadata_only");

        let one = Overview(w)
            .call(json!({"dataset": "visma_gold_finance"}))
            .await
            .unwrap();
        let tables = one["datasets"][0]["tables"].as_array().unwrap();
        assert_eq!(tables.len(), 3);
        assert_eq!(tables[2]["kind"], "view");
    }

    #[tokio::test]
    async fn overview_refuses_unreadable_datasets_and_bad_names_and_reports_failures_as_unknown() {
        let (w, _) = tools(vec![], &[("visma_gold_finance", "__TABLES__")]);
        assert!(Overview(w.clone())
            .call(json!({"dataset": "secret_ds"}))
            .await
            .unwrap_err()
            .to_string()
            .contains("not readable"));
        assert!(matches!(
            Overview(w).call(json!({"dataset": "x`y"})).await,
            Err(ToolError::InvalidArgs(_))
        ));
        // a dataset whose metadata query fails is listed as failed, never silently as empty
        let mut info = select_info(&[("secret_ds", "__TABLES__")], 10, vec![]);
        info.statement_type = "SELECT".into();
        let fake = FakeWarehouse::new(info, vec![]);
        let w = SafeWarehouse::new(
            fake,
            DatasetPolicy::parse("visma_gold_*"),
            Limits::default(),
        );
        let out = Overview(w).call(json!({})).await.unwrap();
        assert!(out["datasets"].as_array().unwrap().is_empty());
        assert_eq!(out["datasets_failed"].as_array().unwrap().len(), 2);
    }
}
