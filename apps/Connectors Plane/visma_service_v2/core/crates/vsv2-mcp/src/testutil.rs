//! Test harness: the real tool set registered through the same functions as production, backed by
//! recording fakes. Used to execute skill examples through the actual tool code.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{json, Value};
use vsv2_common::{load_tenants, TenantId};
use vsv2_interactive as l3;

use crate::catalog::Catalog;
use crate::config::ApprovalMode;
use crate::status::ServerStatus;
use crate::tool::ToolRegistry;
use crate::tools::{self, sync_status, LiveDeps};
use crate::visma::{ErpGet, GraphQl, Tenants};
use crate::warehouse::{
    DatasetPolicy, DryRunInfo, Limits, RawRows, SafeWarehouse, TableRef, Warehouse, WarehouseError,
    DEFAULT_DATASETS, DEFAULT_PROJECT,
};

/// Datasets of the warehouse project that skills, prompts and known issues may name: the readable ones
/// and the ones they say are not readable. Their names start with `visma_`, so the prose checks, which
/// treat every `visma_*` word as a tool or a catalog table, have to know them.
pub(crate) const WAREHOUSE_DATASET_NAMES: &[&str] = &[
    "visma_gold_common",
    "visma_gold_finance",
    "visma_gold_sales",
    "visma_datamart",
    "visma_silver",
    "visma_silver_common",
    "visma_bronze",
    // named as not readable
    "visma_assertions",
    "visma_silver_excel",
    "visma_bronze_excel",
];

pub(crate) fn is_warehouse_dataset(word: &str) -> bool {
    WAREHOUSE_DATASET_NAMES.contains(&word)
}

pub(crate) fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// The real endpoint catalog from `config/endpoints`.
pub(crate) fn real_catalog() -> Catalog {
    Catalog::load(&repo_root().join("config/endpoints")).expect("repo endpoint configs load")
}

/// The real tenants from `config/tenants`, default = the pilot tenant.
pub(crate) fn real_tenants() -> Tenants {
    let list = load_tenants(&repo_root().join("config/tenants")).expect("repo tenant configs load");
    Tenants::new(list, vsv2_common::tenant::AQUATIQ_AS_TENANT)
}

/// One recorded ERP GET: tenant, path, query pairs.
pub(crate) type ErpCall = (String, String, Vec<(String, String)>);

/// Everything the fakes were asked to do, so a test can assert what a tool call would have sent.
#[derive(Default)]
pub(crate) struct Calls {
    pub erp: Mutex<Vec<ErpCall>>,
    pub gql: Mutex<Vec<(String, Value)>>,
    pub reads: Mutex<Vec<String>>,
    pub sql: Mutex<Vec<String>>,
}

pub(crate) struct Fakes(pub Arc<Calls>);

#[async_trait]
impl ErpGet for Fakes {
    async fn get(&self, t: &TenantId, path: &str, q: &[(String, String)]) -> Result<Value, String> {
        self.0
            .erp
            .lock()
            .unwrap()
            .push((t.to_string(), path.into(), q.to_vec()));
        Ok(json!([]))
    }
}

#[async_trait]
impl GraphQl for Fakes {
    /// Answers the two introspection queries the schema tool sends (any type exists, with one
    /// field) and `{"ok": true}` to everything else.
    async fn run(&self, _: &TenantId, q: &str, v: Value) -> Result<Value, String> {
        self.0.gql.lock().unwrap().push((q.into(), v.clone()));
        if q.contains("__type(") {
            let name = v["n"].as_str().unwrap_or("Type");
            return Ok(
                json!({"__type": {"name": name, "kind": "OBJECT", "description": null, "fields": [
                    {"name": "id", "description": null, "type": {"kind": "NON_NULL", "name": null, "ofType": {"kind": "SCALAR", "name": "String", "ofType": null}}}
                ]}}),
            );
        }
        if q.contains("__schema") {
            return Ok(
                json!({"__schema": {"queryType": {"fields": [{"name": "customers", "description": null}]}}}),
            );
        }
        Ok(json!({"ok": true}))
    }
}

#[async_trait]
impl l3::Reader for Fakes {
    async fn get(
        &self,
        _: &TenantId,
        _: l3::WriteTarget,
        path: &str,
    ) -> l3::Result<l3::ReadResponse> {
        self.0.reads.lock().unwrap().push(path.into());
        Ok(l3::ReadResponse {
            status: 200,
            body: json!({"orderId": "000100", "status": {"description": "Open", "cancelled": false, "hold": false}}).to_string(),
            etag: Some("\"e1\"".into()),
        })
    }
}

#[async_trait]
impl l3::Transport for Fakes {
    async fn send(&self, _: &TenantId, _: &l3::WriteRequest) -> l3::Result<l3::WriteResponse> {
        panic!("skill examples must never reach the write transport")
    }
}

#[async_trait]
impl sync_status::SyncStatusSource for Fakes {
    async fn status(&self, _: &sync_status::Filter) -> Result<Value, String> {
        Ok(json!({"checkpoints": [], "recent_runs": [], "runs_last_24h": {}}))
    }
}

/// Pretends BigQuery accepted any SELECT, reporting the tables named in backticks as referenced, so
/// the real guard, dataset policy and limits all run on the example SQL.
#[async_trait]
impl Warehouse for Fakes {
    fn project(&self) -> &str {
        DEFAULT_PROJECT
    }

    async fn dry_run(&self, sql: &str) -> Result<DryRunInfo, WarehouseError> {
        self.0.sql.lock().unwrap().push(sql.into());
        Ok(DryRunInfo {
            statement_type: "SELECT".into(),
            total_bytes: 1,
            referenced: referenced_tables(sql),
            schema: vec![],
        })
    }

    /// No data, except that a table description finds one column (an empty description is an error).
    async fn run(&self, sql: &str, _: usize, _: u64) -> Result<RawRows, WarehouseError> {
        let rows = if sql.contains("COLUMN_FIELD_PATHS") {
            vec![json!({"column_name": "id", "data_type": "INT64", "description": null})]
        } else {
            vec![]
        };
        Ok(RawRows { rows })
    }

    async fn list_datasets(&self) -> Result<Vec<String>, WarehouseError> {
        Ok(vec!["visma_gold_finance".into()])
    }
}

/// `project.dataset.table` references inside backticks (what BigQuery reports as referenced), and
/// the `` `project.dataset`.INFORMATION_SCHEMA.VIEW `` form, where the dataset qualifies the view.
pub(crate) fn referenced_tables(sql: &str) -> Vec<TableRef> {
    let chunks: Vec<&str> = sql.split('`').collect();
    let mut out = Vec::new();
    for (i, chunk) in chunks.iter().enumerate().skip(1).step_by(2) {
        let parts: Vec<&str> = chunk.split('.').collect();
        let info_view = chunks
            .get(i + 1)
            .and_then(|after| after.strip_prefix(".INFORMATION_SCHEMA."))
            .map(|rest| {
                rest.chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect::<String>()
            });
        match (parts.len(), info_view) {
            (1, Some(view)) => out.push(TableRef {
                project: DEFAULT_PROJECT.into(),
                dataset: parts[0].into(),
                table: format!("INFORMATION_SCHEMA.{view}"),
            }),
            (2, Some(view)) => out.push(TableRef {
                project: parts[0].into(),
                dataset: parts[1].into(),
                table: format!("INFORMATION_SCHEMA.{view}"),
            }),
            (n, _) if n >= 3 => out.push(TableRef {
                project: parts[0].into(),
                dataset: parts[1].into(),
                table: parts[2..].join("."),
            }),
            (2, None) => out.push(TableRef {
                project: DEFAULT_PROJECT.into(),
                dataset: parts[0].into(),
                table: parts[1].into(),
            }),
            _ => {}
        }
    }
    out
}

pub(crate) struct Harness {
    pub registry: ToolRegistry,
    pub calls: Arc<Calls>,
    /// Keeps the feedback log's directory alive for as long as the harness.
    _feedback: tempfile::TempDir,
}

/// The production tool set (all of it, including writes and `visma_init`) over recording fakes.
pub(crate) fn harness() -> Harness {
    let calls = Arc::new(Calls::default());
    let fakes = || Arc::new(Fakes(calls.clone()));
    let tenants = real_tenants();
    let catalog = real_catalog();

    let feedback_dir = tempfile::tempdir().expect("a temporary directory for the feedback log");
    let mut reg = tools::local_tools(catalog.clone());
    reg = tools::register_feedback(
        reg,
        feedback_dir.path().join("feedback.jsonl"),
        "skills-test".into(),
    );
    reg = tools::register_warehouse(
        reg,
        SafeWarehouse::new(
            fakes(),
            DatasetPolicy::parse(DEFAULT_DATASETS),
            Limits::default(),
        ),
    );
    reg = tools::register_live(
        reg,
        LiveDeps {
            catalog,
            tenants: tenants.clone(),
            erp: fakes(),
            reader: fakes(),
            gql: Some(fakes()),
            sync: Some(fakes()),
        },
    );
    // Permissive policy so plan examples run end to end; the transport panics if anything executes.
    let executor = Arc::new(l3::Executor::new(
        l3::default_registry(),
        l3::WritePolicy::new(
            [vsv2_common::tenant::AQUATIQ_AS_TENANT],
            [
                "salesorder.patch",
                "salesorder.create",
                "salesorder.set_status",
            ],
        ),
        l3::ConfirmSigner::new(b"skill-harness-secret-0123456789".to_vec())
            .expect("secret is long enough"),
        Arc::new(l3::MemoryAuditSink::default()),
        fakes(),
        fakes(),
    ));
    reg = tools::register_writes(
        reg,
        executor,
        tenants,
        "skills-test".into(),
        ApprovalMode::Elicitation,
    );
    let mut status = ServerStatus::default();
    status.enabled("warehouse", json!({}));
    Harness {
        registry: tools::register_init(reg, status),
        calls,
        _feedback: feedback_dir,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_backticked_table_references() {
        let r = referenced_tables("SELECT 1 FROM `aquatiq-datavarehus.visma_gold_finance.dim_account` a JOIN `visma_gold_common.dim_date` d");
        assert_eq!(r.len(), 2);
        assert_eq!(
            (r[0].dataset.as_str(), r[0].table.as_str()),
            ("visma_gold_finance", "dim_account")
        );
        assert_eq!(r[1].project, DEFAULT_PROJECT);
    }

    #[test]
    fn information_schema_views_are_qualified_by_their_dataset() {
        let r = referenced_tables("SELECT 1 FROM `aquatiq-datavarehus.visma_gold_finance`.INFORMATION_SCHEMA.COLUMNS WHERE table_name = 'x'");
        assert_eq!(r.len(), 1);
        assert_eq!(
            (
                r[0].project.as_str(),
                r[0].dataset.as_str(),
                r[0].table.as_str()
            ),
            (
                DEFAULT_PROJECT,
                "visma_gold_finance",
                "INFORMATION_SCHEMA.COLUMNS"
            )
        );
        let bare =
            referenced_tables("SELECT 1 FROM `visma_gold_finance`.INFORMATION_SCHEMA.TABLES");
        assert_eq!(
            (bare[0].dataset.as_str(), bare[0].table.as_str()),
            ("visma_gold_finance", "INFORMATION_SCHEMA.TABLES")
        );
    }

    #[test]
    fn the_prose_dataset_names_are_the_readable_ones_plus_the_named_closed_ones() {
        let policy = DatasetPolicy::parse(DEFAULT_DATASETS);
        let readable: Vec<&str> = WAREHOUSE_DATASET_NAMES
            .iter()
            .copied()
            .filter(|d| policy.allows(d))
            .collect();
        // every readable `visma_*` dataset is listed, so a skill can name it ...
        for d in [
            "visma_gold_common",
            "visma_gold_finance",
            "visma_gold_sales",
            "visma_datamart",
            "visma_silver",
            "visma_silver_common",
            "visma_bronze",
        ] {
            assert!(
                readable.contains(&d),
                "{d} is readable but not in the prose list"
            );
        }
        // ... and the only listed names that are not readable are the ones skills call closed
        let closed: Vec<&str> = WAREHOUSE_DATASET_NAMES
            .iter()
            .copied()
            .filter(|d| !policy.allows(d))
            .collect();
        assert_eq!(
            closed,
            [
                "visma_assertions",
                "visma_silver_excel",
                "visma_bronze_excel"
            ]
        );
    }

    #[test]
    fn the_harness_registers_every_production_tool_group() {
        let h = harness();
        let names = h.registry.names();
        for n in [
            "visma_init",
            "visma_list_skills",
            "visma_get_skill",
            "visma_list_endpoints",
            "visma_read_result",
            "warehouse_query",
            "warehouse_describe_table",
            "warehouse_list_tables",
            "warehouse_overview",
            "visma_list_tenants",
            "visma_erp_query",
            "visma_salesorder_get",
            "visma_datamart_schema",
            "visma_datamart_query",
            "visma_sync_status",
            "visma_salesorder_patch_plan",
            "visma_salesorder_patch_execute",
            "visma_salesorder_create_plan",
            "visma_salesorder_create_execute",
            "visma_salesorder_status_plan",
            "visma_salesorder_status_execute",
            "visma_search_api",
            "visma_submit_feedback",
        ] {
            assert!(
                names.contains(&n),
                "{n} missing from the harness: {names:?}"
            );
        }
    }
}
