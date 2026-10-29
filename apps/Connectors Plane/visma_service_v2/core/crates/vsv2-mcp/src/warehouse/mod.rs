//! Read-only access to the BigQuery warehouse.
//!
//! [`SafeWarehouse`] is the only entry point tools use. Order of defences for a query:
//! static guard -> BigQuery dry-run (statement type, referenced tables, bytes) -> billed-bytes cap
//! on the real run. The backend ([`BqCli`]) is swappable so all of this is testable offline.

pub mod bq_cli;
pub mod sqlguard;

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use thiserror::Error;

pub use bq_cli::BqCli;
pub use sqlguard::{validate_sql, GuardError};

#[derive(Debug, Error)]
pub enum WarehouseError {
    #[error(transparent)]
    Guard(#[from] GuardError),
    #[error("refused: {0}")]
    Refused(String),
    #[error("BigQuery error: {0}")]
    Backend(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRef {
    pub project: String,
    pub dataset: String,
    pub table: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaField {
    pub name: String,
    pub ty: String,
    pub repeated: bool,
    pub fields: Vec<SchemaField>,
}

#[derive(Debug, Clone)]
pub struct DryRunInfo {
    pub statement_type: String,
    pub total_bytes: u64,
    pub referenced: Vec<TableRef>,
    pub schema: Vec<SchemaField>,
}

#[derive(Debug, Clone)]
pub struct RawRows {
    pub rows: Vec<Value>,
}

#[async_trait]
pub trait Warehouse: Send + Sync {
    fn project(&self) -> &str;
    async fn dry_run(&self, sql: &str) -> Result<DryRunInfo, WarehouseError>;
    /// Run a validated query; returns at most `max_rows` rows (values still as the CLI renders
    /// them: scalars as strings).
    async fn run(
        &self,
        sql: &str,
        max_rows: usize,
        max_bytes: u64,
    ) -> Result<RawRows, WarehouseError>;
    async fn list_datasets(&self) -> Result<Vec<String>, WarehouseError>;
}

/// Which datasets may be read. Patterns are exact names or a prefix ending in `*`.
#[derive(Debug, Clone)]
pub struct DatasetPolicy {
    patterns: Vec<String>,
}

/// The warehouse the server reads: v2's own BigQuery project. It is what an unset
/// `VSV2_MCP_BQ_PROJECT` is told to be (the setting still has to be given to enable the tools), and
/// what the test fakes and live tests use, so no project name is spelled twice.
pub const DEFAULT_PROJECT: &str = "aquatiq-datavarehus";

/// Datasets of [`DEFAULT_PROJECT`] that are readable unless `VSV2_MCP_BQ_DATASETS` says otherwise: the
/// gold layer (`visma_gold_*`), the Power BI layer (`visma_datamart`), silver, the silver reconciliation
/// tables, bronze and the raw staging the v2 sync writes. The rest of the project (assertion views,
/// Excel mapping inputs, Xero, currency staging) is deliberately not on the list.
pub const DEFAULT_DATASETS: &str = "visma_gold_*,visma_silver,visma_silver_common,visma_bronze,\
    visma_datamart,dw_staging_visma";

impl DatasetPolicy {
    pub fn parse(list: &str) -> Self {
        Self {
            patterns: list
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect(),
        }
    }

    pub fn allows(&self, dataset: &str) -> bool {
        self.patterns.iter().any(|p| match p.strip_suffix('*') {
            Some(prefix) => dataset.starts_with(prefix),
            None => p == dataset,
        })
    }

    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Reject queries whose dry-run estimate exceeds this many bytes.
    pub max_bytes: u64,
    /// Total estimated bytes this process may scan per rolling hour (a runaway-loop brake: each
    /// single query is already capped by `max_bytes`).
    pub max_bytes_per_hour: u64,
    pub default_rows: usize,
    pub max_rows: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: 2 * 1024 * 1024 * 1024,
            max_bytes_per_hour: 20 * 1024 * 1024 * 1024,
            default_rows: 100,
            max_rows: 500,
        }
    }
}

#[derive(Debug, Clone)]
pub struct QueryOutput {
    pub columns: Vec<(String, String)>,
    pub rows: Vec<Value>,
    pub truncated: bool,
    pub bytes_estimate: u64,
}

#[derive(Clone)]
pub struct SafeWarehouse {
    backend: Arc<dyn Warehouse>,
    policy: DatasetPolicy,
    pub limits: Limits,
    /// (when, estimated bytes) of recent queries, shared by clones.
    spent: Arc<std::sync::Mutex<std::collections::VecDeque<(std::time::Instant, u64)>>>,
}

impl SafeWarehouse {
    pub fn new(backend: Arc<dyn Warehouse>, policy: DatasetPolicy, limits: Limits) -> Self {
        Self {
            backend,
            policy,
            limits,
            spent: Default::default(),
        }
    }

    /// Count `bytes` against the rolling-hour budget, or refuse. Called after every other check so
    /// refused queries cost nothing.
    fn reserve(&self, bytes: u64) -> Result<(), WarehouseError> {
        const WINDOW: std::time::Duration = std::time::Duration::from_secs(3600);
        let now = std::time::Instant::now();
        let mut q = self.spent.lock().expect("budget lock");
        while q
            .front()
            .is_some_and(|(t, _)| now.duration_since(*t) > WINDOW)
        {
            q.pop_front();
        }
        let used: u64 = q.iter().map(|(_, b)| *b).sum();
        if used.saturating_add(bytes) > self.limits.max_bytes_per_hour {
            return Err(WarehouseError::Refused(format!(
                "hourly scan budget used up ({used} of {} bytes in the last hour); try again later or narrow the queries",
                self.limits.max_bytes_per_hour
            )));
        }
        q.push_back((now, bytes));
        Ok(())
    }

    pub fn project(&self) -> &str {
        self.backend.project()
    }

    pub fn policy(&self) -> &DatasetPolicy {
        &self.policy
    }

    pub async fn allowed_datasets(&self) -> Result<Vec<String>, WarehouseError> {
        let mut v: Vec<String> = self
            .backend
            .list_datasets()
            .await?
            .into_iter()
            .filter(|d| self.policy.allows(d))
            .collect();
        v.sort();
        Ok(v)
    }

    /// Validate -> dry-run -> run -> restore types. `max_rows` is clamped to `limits.max_rows`
    /// unless `allow_up_to` raises the ceiling for trusted internal metadata queries.
    pub async fn select(
        &self,
        sql: &str,
        max_rows: Option<usize>,
        allow_up_to: Option<usize>,
    ) -> Result<QueryOutput, WarehouseError> {
        let sql = validate_sql(sql)?;
        let info = self.backend.dry_run(&sql).await?;

        if info.statement_type != "SELECT" {
            return Err(WarehouseError::Refused(format!(
                "BigQuery reports statement type {}, only SELECT is allowed",
                info.statement_type
            )));
        }
        for t in &info.referenced {
            if t.project != self.backend.project() {
                return Err(WarehouseError::Refused(format!(
                    "table {}.{}.{} is outside project {}",
                    t.project,
                    t.dataset,
                    t.table,
                    self.backend.project()
                )));
            }
            if !self.policy.allows(&t.dataset) {
                return Err(WarehouseError::Refused(format!(
                    "dataset {} is not readable here (allowed: {})",
                    t.dataset,
                    self.policy.patterns().join(", ")
                )));
            }
        }
        if info.total_bytes > self.limits.max_bytes {
            return Err(WarehouseError::Refused(format!(
                "query would scan {} bytes, over the {} byte limit; add filters or select fewer columns",
                info.total_bytes, self.limits.max_bytes
            )));
        }
        self.reserve(info.total_bytes)?;

        let ceiling = allow_up_to.unwrap_or(self.limits.max_rows).max(1);
        let want = max_rows
            .unwrap_or(self.limits.default_rows)
            .clamp(1, ceiling);
        // Ask for one extra row to learn whether the result was cut.
        let raw = self
            .backend
            .run(&sql, want + 1, self.limits.max_bytes)
            .await?;
        let truncated = raw.rows.len() > want;
        let rows: Vec<Value> = raw
            .rows
            .into_iter()
            .take(want)
            .map(|r| coerce_row(r, &info.schema))
            .collect();
        Ok(QueryOutput {
            columns: info
                .schema
                .iter()
                .map(|f| (f.name.clone(), f.ty.clone()))
                .collect(),
            rows,
            truncated,
            bytes_estimate: info.total_bytes,
        })
    }
}

/// The CLI renders every scalar as a string; restore numbers and booleans from the schema.
/// NUMERIC/BIGNUMERIC stay strings on purpose (no precision loss).
pub fn coerce_row(row: Value, schema: &[SchemaField]) -> Value {
    let Value::Object(mut map) = row else {
        return row;
    };
    for f in schema {
        if let Some(v) = map.remove(&f.name) {
            map.insert(f.name.clone(), coerce_value(v, f));
        }
    }
    Value::Object(map)
}

fn coerce_value(v: Value, f: &SchemaField) -> Value {
    if f.repeated {
        return match v {
            Value::Array(items) => {
                let elem = SchemaField {
                    repeated: false,
                    ..f.clone()
                };
                Value::Array(items.into_iter().map(|i| coerce_value(i, &elem)).collect())
            }
            other => other,
        };
    }
    match (f.ty.as_str(), &v) {
        ("INTEGER" | "INT64", Value::String(s)) => s.parse::<i64>().map(Value::from).unwrap_or(v),
        ("FLOAT" | "FLOAT64", Value::String(s)) => s
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .unwrap_or(v),
        ("BOOLEAN" | "BOOL", Value::String(s)) => match s.as_str() {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            _ => v,
        },
        ("RECORD" | "STRUCT", Value::Object(_)) => coerce_row(v, &f.fields),
        _ => v,
    }
}

#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use std::sync::Mutex;

    /// Scriptable backend for tests.
    pub struct FakeWarehouse {
        pub project: String,
        pub info: Mutex<DryRunInfo>,
        pub rows: Mutex<Vec<Value>>,
        pub datasets: Vec<String>,
        pub ran: Mutex<Vec<(String, usize, u64)>>,
    }

    impl FakeWarehouse {
        pub fn new(info: DryRunInfo, rows: Vec<Value>) -> Arc<Self> {
            Arc::new(Self {
                project: "proj".into(),
                info: Mutex::new(info),
                rows: Mutex::new(rows),
                datasets: vec![
                    "visma_gold_finance".into(),
                    "secret_ds".into(),
                    "visma_gold_sales".into(),
                ],
                ran: Mutex::new(vec![]),
            })
        }
    }

    pub fn select_info(
        tables: &[(&str, &str)],
        bytes: u64,
        schema: Vec<SchemaField>,
    ) -> DryRunInfo {
        DryRunInfo {
            statement_type: "SELECT".into(),
            total_bytes: bytes,
            referenced: tables
                .iter()
                .map(|(d, t)| TableRef {
                    project: "proj".into(),
                    dataset: (*d).into(),
                    table: (*t).into(),
                })
                .collect(),
            schema,
        }
    }

    pub fn field(name: &str, ty: &str) -> SchemaField {
        SchemaField {
            name: name.into(),
            ty: ty.into(),
            repeated: false,
            fields: vec![],
        }
    }

    #[async_trait]
    impl Warehouse for FakeWarehouse {
        fn project(&self) -> &str {
            &self.project
        }
        async fn dry_run(&self, _: &str) -> Result<DryRunInfo, WarehouseError> {
            Ok(self.info.lock().unwrap().clone())
        }
        async fn run(
            &self,
            sql: &str,
            max_rows: usize,
            max_bytes: u64,
        ) -> Result<RawRows, WarehouseError> {
            self.ran
                .lock()
                .unwrap()
                .push((sql.into(), max_rows, max_bytes));
            Ok(RawRows {
                rows: self
                    .rows
                    .lock()
                    .unwrap()
                    .iter()
                    .take(max_rows)
                    .cloned()
                    .collect(),
            })
        }
        async fn list_datasets(&self) -> Result<Vec<String>, WarehouseError> {
            Ok(self.datasets.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::*;
    use super::*;
    use serde_json::json;

    fn safe(info: DryRunInfo, rows: Vec<Value>) -> (SafeWarehouse, Arc<FakeWarehouse>) {
        let fake = FakeWarehouse::new(info, rows);
        (
            SafeWarehouse::new(
                fake.clone(),
                DatasetPolicy::parse("visma_gold_*"),
                Limits::default(),
            ),
            fake,
        )
    }

    #[test]
    fn dataset_policy_supports_exact_and_prefix_patterns() {
        let p = DatasetPolicy::parse("visma_gold_*, visma_silver ,");
        assert!(p.allows("visma_gold_finance") && p.allows("visma_gold_"));
        assert!(p.allows("visma_silver"));
        // an exact name is not a prefix: the sibling datasets of the project stay closed
        assert!(
            !p.allows("visma_silver_excel") && !p.allows("visma_silver_common") && !p.allows("")
        );
        assert!(!p.allows("visma_bronze"));
    }

    #[test]
    fn the_default_allow_list_reads_the_visma_warehouse_and_nothing_else_in_the_project() {
        let p = DatasetPolicy::parse(DEFAULT_DATASETS);
        for open in [
            "visma_gold_common",
            "visma_gold_finance",
            "visma_gold_sales",
            "visma_datamart",
            "visma_silver",
            "visma_silver_common",
            "visma_bronze",
            "dw_staging_visma",
        ] {
            assert!(p.allows(open), "{open} must be readable by default");
        }
        // The other datasets of aquatiq-datavarehus (checked 2026-10-12): assertion views, Excel
        // mapping inputs, the Xero pipeline, currency staging, and region-level metadata.
        for closed in [
            "visma_assertions",
            "visma_silver_excel",
            "visma_bronze_excel",
            "dw_staging_xero",
            "dw_staging_currency",
            "dw_bronze_xero",
            "dw_bronze_xero_assertions",
            "region-europe-north2",
            "",
        ] {
            assert!(
                !p.allows(closed),
                "{closed:?} must not be readable by default"
            );
        }
    }

    #[tokio::test]
    async fn runs_a_select_restores_types_and_flags_truncation() {
        let schema = vec![
            field("id", "INTEGER"),
            field("ok", "BOOLEAN"),
            field("amt", "FLOAT"),
            field("n", "NUMERIC"),
        ];
        let rows = (1..=3)
            .map(|i| json!({"id": i.to_string(), "ok": "true", "amt": "2.5", "n": "12.30"}))
            .collect();
        let (w, fake) = safe(
            select_info(&[("visma_gold_finance", "t")], 1000, schema),
            rows,
        );

        let out = w
            .select("SELECT * FROM `proj.visma_gold_finance.t`", Some(2), None)
            .await
            .unwrap();
        assert_eq!(out.rows.len(), 2);
        assert!(out.truncated);
        assert_eq!(
            out.rows[0],
            json!({"id": 1, "ok": true, "amt": 2.5, "n": "12.30"})
        );
        assert_eq!(
            fake.ran.lock().unwrap()[0].1,
            3,
            "asks for one extra row to detect truncation"
        );
        assert_eq!(out.bytes_estimate, 1000);
    }

    #[tokio::test]
    async fn refuses_non_select_statement_types_even_if_the_static_guard_passed() {
        let mut info = select_info(&[], 0, vec![]);
        info.statement_type = "SCRIPT".into();
        let (w, fake) = safe(info, vec![]);
        let e = w.select("SELECT 1", None, None).await.unwrap_err();
        assert!(e.to_string().contains("SCRIPT"), "{e}");
        assert!(fake.ran.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn refuses_other_datasets_other_projects_and_oversized_scans_without_running() {
        let (w, fake) = safe(select_info(&[("secret_ds", "t")], 10, vec![]), vec![]);
        assert!(w
            .select("SELECT 1", None, None)
            .await
            .unwrap_err()
            .to_string()
            .contains("not readable"));

        let mut other = select_info(&[("visma_gold_finance", "t")], 10, vec![]);
        other.referenced[0].project = "someone-else".into();
        let (w2, _) = safe(other, vec![]);
        assert!(w2
            .select("SELECT 1", None, None)
            .await
            .unwrap_err()
            .to_string()
            .contains("outside project"));

        let (w3, _) = safe(
            select_info(&[("visma_gold_finance", "t")], u64::MAX, vec![]),
            vec![],
        );
        assert!(w3
            .select("SELECT 1", None, None)
            .await
            .unwrap_err()
            .to_string()
            .contains("byte limit"));
        assert!(fake.ran.lock().unwrap().is_empty(), "nothing ran");
    }

    #[tokio::test]
    async fn guard_failures_never_reach_the_backend() {
        let (w, fake) = safe(select_info(&[], 0, vec![]), vec![]);
        assert!(matches!(
            w.select("DROP TABLE x", None, None).await,
            Err(WarehouseError::Guard(_))
        ));
        assert!(fake.ran.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn row_limit_is_clamped_and_only_raisable_by_internal_callers() {
        let rows: Vec<Value> = (0..800).map(|i| json!({"i": i.to_string()})).collect();
        let (w, _) = safe(select_info(&[], 0, vec![field("i", "INTEGER")]), rows);
        assert_eq!(
            w.select("SELECT 1", Some(10_000), None)
                .await
                .unwrap()
                .rows
                .len(),
            500
        );
        assert_eq!(
            w.select("SELECT 1", Some(10_000), Some(800))
                .await
                .unwrap()
                .rows
                .len(),
            800
        );
    }

    #[tokio::test]
    async fn the_hourly_byte_budget_stops_a_runaway_loop_but_not_refused_queries() {
        let fake = FakeWarehouse::new(
            select_info(&[("visma_gold_finance", "t")], 100, vec![]),
            vec![],
        );
        let limits = Limits {
            max_bytes_per_hour: 250,
            ..Limits::default()
        };
        let w = SafeWarehouse::new(fake.clone(), DatasetPolicy::parse("visma_gold_*"), limits);
        assert!(w.select("SELECT 1", None, None).await.is_ok());
        assert!(w.select("SELECT 1", None, None).await.is_ok());
        let e = w
            .select("SELECT 1", None, None)
            .await
            .unwrap_err()
            .to_string();
        assert!(e.contains("hourly scan budget"), "{e}");
        assert_eq!(fake.ran.lock().unwrap().len(), 2, "the third never ran");
        // clones share the budget
        assert!(w.clone().select("SELECT 1", None, None).await.is_err());
    }

    #[test]
    fn coerces_nested_records_and_repeated_fields() {
        let rec = SchemaField {
            name: "st".into(),
            ty: "RECORD".into(),
            repeated: false,
            fields: vec![field("k", "INTEGER")],
        };
        let arr = SchemaField {
            name: "arr".into(),
            ty: "INTEGER".into(),
            repeated: true,
            fields: vec![],
        };
        let out = coerce_row(
            json!({"st": {"k": "3"}, "arr": ["1", "2"], "extra": "keep"}),
            &[rec, arr],
        );
        assert_eq!(out, json!({"st": {"k": 3}, "arr": [1, 2], "extra": "keep"}));
        // unparsable stays a string rather than being dropped
        assert_eq!(
            coerce_row(json!({"i": "abc"}), &[field("i", "INTEGER")]),
            json!({"i": "abc"})
        );
    }
}
