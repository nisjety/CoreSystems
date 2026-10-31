//! Concrete MCP tools. Read tools first; write tools are registered separately and only when
//! everything a write depends on is verified at startup.

pub mod api_search;
pub mod endpoints;
pub mod feedback;
pub mod init;
mod live_setup;
pub mod results;
pub mod skills;
pub mod sync_status;
pub mod visma;
pub mod warehouse;
pub mod write;
pub mod write_create;
pub mod write_status;

use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::json;

use crate::catalog::Catalog;
use crate::config::McpConfig;
use crate::skills::SkillIndex;
use crate::status::ServerStatus;
use crate::tool::ToolRegistry;
use crate::visma::{ErpGet, GraphQl, Tenants};
use crate::warehouse::{
    BqCli, DatasetPolicy, Limits, SafeWarehouse, DEFAULT_DATASETS, DEFAULT_PROJECT,
};

/// Tools that need nothing but local config. (The skill tools and `visma_init` come last, in
/// [`register_init`], because they have to know which other tools exist.)
pub fn local_tools(catalog: Catalog) -> ToolRegistry {
    let skills = Arc::new(SkillIndex::embedded().clone());
    let reg = ToolRegistry::new()
        .with_skills(skills)
        .with(api_search::ApiSearch {
            catalog: catalog.clone(),
        })
        .with(endpoints::ListEndpoints::new(catalog));
    let store = reg.results();
    reg.with(results::ReadResult(store))
}

/// The local feedback log (`visma_submit_feedback`). Always on: it needs no credentials and writes one
/// owner-readable file.
pub fn register_feedback(
    reg: ToolRegistry,
    path: std::path::PathBuf,
    caller: String,
) -> ToolRegistry {
    reg.with(feedback::SubmitFeedback { path, caller })
}

/// Everything the live Visma read tools need. Production builds it from real clients; the test
/// harness builds the same thing from fakes, so both register exactly the same tools.
pub struct LiveDeps {
    pub catalog: Catalog,
    pub tenants: Tenants,
    pub erp: Arc<dyn ErpGet>,
    pub reader: Arc<dyn vsv2_interactive::Reader>,
    /// Absent when DataMart cannot be reached as the current identity: its tools are then not registered.
    pub gql: Option<Arc<dyn GraphQl>>,
    pub sync: Option<Arc<dyn sync_status::SyncStatusSource>>,
}

pub fn register_warehouse(reg: ToolRegistry, w: SafeWarehouse) -> ToolRegistry {
    reg.with(warehouse::ListTables(w.clone()))
        .with(warehouse::DescribeTable(w.clone()))
        .with(warehouse::Overview(w.clone()))
        .with(warehouse::Query(w))
}

pub fn register_live(reg: ToolRegistry, d: LiveDeps) -> ToolRegistry {
    let mut reg = reg
        .with(visma::ListTenants(d.tenants.clone()))
        .with(visma::ErpQuery {
            catalog: d.catalog,
            tenants: d.tenants.clone(),
            erp: d.erp,
        })
        .with(visma::SalesOrderGet {
            tenants: d.tenants.clone(),
            reader: d.reader,
        });
    if let Some(gql) = d.gql {
        reg = reg
            .with(visma::DatamartSchema {
                tenants: d.tenants.clone(),
                gql: gql.clone(),
            })
            .with(visma::DatamartQuery {
                tenants: d.tenants.clone(),
                gql,
            });
    }
    if let Some(source) = d.sync {
        reg = reg.with(sync_status::SyncStatus {
            tenants: d.tenants,
            source,
        });
    }
    reg
}

pub fn register_writes(
    reg: ToolRegistry,
    executor: Arc<vsv2_interactive::Executor>,
    tenants: Tenants,
    caller: String,
    mode: crate::config::ApprovalMode,
) -> ToolRegistry {
    // One approval and execution path for every write; the execute tools differ only in name and words.
    let apply = || write::PatchExecute {
        executor: executor.clone(),
        tenants: tenants.clone(),
        caller: caller.clone(),
        mode,
    };
    reg.with(write::PatchPlan {
        executor: executor.clone(),
        tenants: tenants.clone(),
        caller: caller.clone(),
    })
    .with(apply())
    .with(write_create::CreatePlan {
        executor: executor.clone(),
        tenants: tenants.clone(),
        caller: caller.clone(),
    })
    .with(write_create::create_execute(apply()))
    .with(write_status::StatusPlan {
        executor: executor.clone(),
        tenants: tenants.clone(),
        caller: caller.clone(),
    })
    .with(write_status::status_execute(apply()))
}

/// Last step. Registers the skill tools and `visma_init`, which both need to know which tools this
/// instance has: a skill that names a tool that is not here says so, `visma_init` lists the tools,
/// and only the prompts whose tools exist are offered.
pub fn register_init(reg: ToolRegistry, status: ServerStatus) -> ToolRegistry {
    let index = Arc::new(SkillIndex::embedded().clone());
    let mut names: BTreeSet<String> = reg.names().iter().map(|n| (*n).to_string()).collect();
    names.extend([init::NAME, skills::LIST, skills::GET].map(String::from));
    let prompts = crate::prompts::available(&names)
        .map(|p| p.name.to_string())
        .collect();
    let available = Arc::new(names.clone());
    reg.with(skills::ListSkills::new(index.clone(), available.clone()))
        .with(skills::GetSkill::new(index, available))
        .with(init::Init {
            status,
            tools: names.into_iter().collect(),
            prompts,
        })
}

/// What `visma_init` shows for the warehouse: the configured project, the readable datasets and the
/// limits. A project other than [`DEFAULT_PROJECT`] with the default dataset list gets a `warning`: the
/// list names datasets of the v2 warehouse, so a client still pointed at another project would see its
/// queries refused with no hint why. A custom list is the operator's own choice and is not second-guessed.
fn warehouse_capability(project: &str, datasets: &DatasetPolicy, l: Limits) -> serde_json::Value {
    let mut v = json!({
        "project": project,
        "readable_datasets": datasets.patterns(),
        "max_bytes_per_query": l.max_bytes,
        "max_bytes_per_hour": l.max_bytes_per_hour,
        "default_rows": l.default_rows,
        "max_rows": l.max_rows,
        "read_only": true,
    });
    let default_list = DatasetPolicy::parse(DEFAULT_DATASETS);
    if project != DEFAULT_PROJECT && datasets.patterns() == default_list.patterns() {
        v["warning"] = json!(format!(
            "the configured warehouse project is {project}, not {DEFAULT_PROJECT}: the default readable \
             datasets belong to {DEFAULT_PROJECT}, so queries are refused unless VSV2_MCP_BQ_DATASETS names \
             datasets of {project}"
        ));
    }
    v
}

/// Build the registry from configuration. Alongside it returns the [`ServerStatus`]: every
/// capability that is on (with its limits) or off (with the reason). `visma_init` shows it to the
/// model, and the startup log shows the notes, so a missing tool is never a mystery.
pub async fn build_registry(cfg: &McpConfig, catalog: Catalog) -> (ToolRegistry, ServerStatus) {
    let mut status = ServerStatus::default();
    let erp_endpoints = catalog
        .all()
        .iter()
        .filter(|e| e.source == "erp_rest" && e.api_endpoint.is_some())
        .count();
    let catalog_for_live = catalog.clone();
    let mut reg = local_tools(catalog);
    reg = register_feedback(reg, cfg.feedback_file.clone(), cfg.caller.clone());
    status.enabled(
        "catalog",
        json!({"endpoints": catalog_for_live.all().len(), "live_queryable_erp_rest": erp_endpoints}),
    );

    match &cfg.bq_project {
        Some(project) => {
            let w = SafeWarehouse::new(
                Arc::new(BqCli::new(project.clone())),
                cfg.bq_datasets.clone(),
                cfg.bq_limits,
            );
            reg = register_warehouse(reg, w);
            let capability = warehouse_capability(project, &cfg.bq_datasets, cfg.bq_limits);
            if let Some(warning) = capability["warning"].as_str() {
                status.note(warning);
            }
            status.enabled("warehouse", capability);
        }
        None => status.disabled(
            "warehouse",
            format!("VSV2_MCP_BQ_PROJECT is not set (the v2 warehouse is {DEFAULT_PROJECT})"),
        ),
    }

    reg = live_setup::add_visma_tools(reg, cfg, &catalog_for_live, &mut status).await;

    let reg = register_init(reg, status.clone());
    (reg, status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::warehouse::DEFAULT_DATASETS;

    #[test]
    fn the_warehouse_capability_names_the_v2_project_and_its_datasets_without_a_warning() {
        let v = warehouse_capability(
            DEFAULT_PROJECT,
            &DatasetPolicy::parse(DEFAULT_DATASETS),
            Limits::default(),
        );
        assert_eq!(v["project"], "aquatiq-datavarehus");
        assert_eq!(v["readable_datasets"][0], "visma_gold_*");
        assert_eq!(v["read_only"], true);
        assert!(v.get("warning").is_none(), "{v}");
    }

    #[test]
    fn another_project_is_flagged_so_a_client_left_on_an_old_setting_is_not_a_mystery() {
        let v = warehouse_capability(
            "some-other-project",
            &DatasetPolicy::parse(DEFAULT_DATASETS),
            Limits::default(),
        );
        let w = v["warning"].as_str().expect("a warning");
        assert!(
            w.contains("some-other-project")
                && w.contains(DEFAULT_PROJECT)
                && w.contains("VSV2_MCP_BQ_DATASETS"),
            "{w}"
        );
    }

    #[test]
    fn another_project_with_its_own_dataset_list_is_not_second_guessed() {
        let v = warehouse_capability(
            "some-other-project",
            &DatasetPolicy::parse("their_gold_*,their_silver"),
            Limits::default(),
        );
        assert_eq!(v["project"], "some-other-project");
        assert!(v.get("warning").is_none(), "{v}");
    }
}
