//! `vsv2` — operator CLI for visma_service_v2.
//!
//! Phase 1: token, dm-ping, dm-companies, dm-pull
//! Phase 2: flag   — upsert per-tenant source override
//! Phase 3: sync   — fetch + checkpoint (BigQuery MERGE in Phase 4)

mod monitor;

use std::path::Path;

use anyhow::Context;
use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand};
use serde_json::json;
use tokio::time::Duration;
use tracing::{info, warn};

use vsv2_auth::{OAuthClient, Scope};
use vsv2_common::{config::AppConfig, Source, TenantId};
use vsv2_datamart::{paginate_all, CursorPaginator, DatamartClient};
use vsv2_erp_rest::ErpRestClient;
use vsv2_expense::ExpenseClient;
use vsv2_orchestrator::{
    BqWriter, DatamartAdapter, DispatchTable, EndpointRegistry, ErpRestAdapter, ExpenseAdapter,
    NoopAdapter, ScheduledTenant, Scheduler, SyncExecutor, TierSpec,
};
use vsv2_ratelimit::Limiter;
use vsv2_reconcile::DeleteReconciler;
use vsv2_state::StateStore;

#[derive(Debug, Parser)]
#[command(name = "vsv2", version, about = "visma_service_v2 operator CLI")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Container/runtime healthcheck: verify Postgres + Redis connectivity.
    Healthcheck {
        #[arg(long)]
        tenant: Option<String>,
    },

    /// Create or update the schema of the state database from the migrations embedded in this
    /// build. `schedule` and the HTTP service do it at start; this is for doing it on its own.
    Migrate,

    /// Fetch a Visma Connect token. Prints the JWT (sensitive).
    Token {
        #[arg(long, default_value = "datamart")]
        scope: ScopeArg,
        #[arg(long)]
        tenant: Option<String>,
    },

    /// Smoke test: `query { __typename }` against DataMart.
    DmPing {
        #[arg(long)]
        tenant: Option<String>,
    },

    /// Discover company IDs visible to the tenant token.
    DmCompanies {
        #[arg(long)]
        tenant: Option<String>,
    },

    /// Paginate a single DataMart entity for the configured tenant.
    DmPull {
        #[arg(long)]
        query_file: String,
        #[arg(long)]
        root_field: String,
        #[arg(long, default_value_t = 500)]
        page_size: u32,
        #[arg(long)]
        max_pages: Option<u32>,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        company_id: Option<String>,
    },

    /// Run sync for one or all endpoints. Records fetched + checkpoint stored.
    /// BigQuery MERGE is deferred to Phase 4.
    Sync {
        #[arg(long)]
        tenant: String,
        #[arg(long)]
        endpoint: Option<String>,
        /// Inclusive lower bound for modified timestamps (RFC3339, UTC).
        #[arg(long)]
        date_from: Option<DateTime<Utc>>,
        /// Inclusive upper bound for modified timestamps (RFC3339, UTC).
        #[arg(long)]
        date_to: Option<DateTime<Utc>>,
        /// Fetch everything Visma has, ignoring checkpoints. Records the tenant has in staging
        /// that Visma no longer returns are marked deleted.
        #[arg(long, conflicts_with_all = ["date_from", "date_to"])]
        full: bool,
        /// Directory containing endpoint *.json definitions.
        #[arg(long, default_value = "config/endpoints")]
        config_dir: String,
    },

    /// Full ERP ID scan + soft-delete BQ rows missing upstream.
    Reconcile {
        #[arg(long)]
        tenant: String,
        #[arg(long)]
        endpoint: Option<String>,
        #[arg(long, default_value = "config/endpoints")]
        config_dir: String,
    },

    /// Flip per-tenant source override flag for an endpoint.
    Flag {
        #[arg(long)]
        tenant: String,
        #[arg(long)]
        endpoint: String,
        #[arg(long, value_enum)]
        source: SourceArg,
        #[arg(long)]
        enable: bool,
    },

    /// Register the tenants in config/tenants/*.json: write each tenant's DataMart company to the
    /// state store so a sync of that tenant can query DataMart. Idempotent.
    TenantsRegister {
        #[arg(long, default_value = "config/tenants")]
        tenants_dir: String,
        /// Show what would change and write nothing.
        #[arg(long)]
        dry_run: bool,
    },

    /// Show all endpoint source override flags for a tenant.
    FlagShow {
        #[arg(long)]
        tenant: String,
    },

    /// Run the scheduler over the tenants in config/tenants: tier 1 endpoints hourly, tier 2-3
    /// daily, and a weekly full pass over every endpoint that also reconciles deletes. What is
    /// due is read from the database, so restarts and sleep do not shift the schedule. Ends
    /// gracefully on Ctrl-C or SIGTERM.
    Schedule {
        /// Only these tenants (repeat the flag); default: every tenant in --tenants-dir.
        #[arg(long)]
        tenant: Vec<String>,
        #[arg(long, default_value = "config/tenants")]
        tenants_dir: String,
        #[arg(long, default_value = "config/endpoints")]
        config_dir: String,
        /// Run what is due once, wait for it, and exit.
        #[arg(long)]
        once: bool,
    },

    /// Check the health of the pipeline from the state database and the tenant list, print a
    /// report, and optionally post a summary to a chat webhook. Reads only Postgres. Exit code:
    /// 0 healthy (warnings allowed), 1 a problem, 2 the state could not be read.
    Monitor(monitor::MonitorArgs),
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum ScopeArg {
    Erp,
    Datamart,
    Expense,
    SalesOrder,
}

impl ScopeArg {
    fn into_scope(self) -> Scope {
        match self {
            ScopeArg::Erp => Scope::Erp,
            ScopeArg::Datamart => Scope::Datamart,
            ScopeArg::Expense => Scope::Expense,
            ScopeArg::SalesOrder => Scope::SalesOrder,
        }
    }
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum SourceArg {
    ErpRest,
    DatamartGraphql,
    ExpenseApi,
}

impl SourceArg {
    fn into_source(self) -> Source {
        match self {
            SourceArg::ErpRest => Source::ErpRest,
            SourceArg::DatamartGraphql => Source::DatamartGraphql,
            SourceArg::ExpenseApi => Source::ExpenseApi,
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,vsv2=debug")),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Healthcheck { tenant } => {
            let cfg = AppConfig::from_env()?;
            let tid = TenantId::new(tenant.unwrap_or(cfg.aquatiq_tenant_id.clone()));
            let state = StateStore::connect(&cfg.pg_dsn, &cfg.redis_url)
                .await
                .context("healthcheck: connect state store")?;
            let owner_id = format!("healthcheck-{}", uuid::Uuid::new_v4());
            // Lease contention is still healthy: another runner may legitimately hold
            // the tenant lock while Postgres and Redis connectivity remain sound.
            let lease_acquired = state
                .try_acquire_lease(&tid, &owner_id, 5)
                .await
                .context("healthcheck: verify redis lease path")?;
            if lease_acquired {
                state
                    .release_lease(&tid)
                    .await
                    .context("healthcheck: release redis lease")?;
            }
            info!(tenant = %tid, lease_acquired, "healthcheck: ok");
            Ok(())
        }
        Cmd::Migrate => {
            let cfg = AppConfig::from_env()?;
            let state = StateStore::connect(&cfg.pg_dsn, &cfg.redis_url)
                .await
                .context("migrate: connect state store")?;
            let embedded = vsv2_state::embedded_migrations();
            let applied = state.migrate().await.context("migrate: apply")?;
            let latest = embedded.last().map_or(0, |(version, _)| *version);
            info!(
                embedded = embedded.len(),
                applied, latest, "migrate: state database is up to date"
            );
            Ok(())
        }
        Cmd::Token { scope, tenant } => {
            let cfg = AppConfig::from_env()?;
            let oauth = build_oauth(&cfg);
            let tid = TenantId::new(tenant.unwrap_or(cfg.aquatiq_tenant_id.clone()));
            let token = oauth.token_for(&tid, scope.into_scope()).await?;
            println!("{token}");
            Ok(())
        }
        Cmd::DmPing { tenant } => {
            let cfg = AppConfig::from_env()?;
            let dm = build_datamart(&cfg, None);
            let tid = TenantId::new(tenant.unwrap_or(cfg.aquatiq_tenant_id.clone()));
            let data = dm.execute(&tid, "{ __typename }", json!({})).await?;
            println!("{}", serde_json::to_string_pretty(&data)?);
            Ok(())
        }
        Cmd::DmCompanies { tenant } => {
            let cfg = AppConfig::from_env()?;
            let dm = build_datamart(&cfg, None);
            let tid = TenantId::new(tenant.unwrap_or(cfg.aquatiq_tenant_id.clone()));
            let data = discover_company_ids(&dm, &tid).await?;
            println!("{}", serde_json::to_string_pretty(&data)?);
            Ok(())
        }
        Cmd::DmPull {
            query_file,
            root_field,
            page_size,
            max_pages,
            tenant,
            company_id,
        } => {
            let cfg = AppConfig::from_env()?;
            let tid = TenantId::new(tenant.unwrap_or(cfg.aquatiq_tenant_id.clone()));
            let query = std::fs::read_to_string(&query_file)?;
            let cid = company_id.or(cfg.env_company_id_for(&tid).map(String::from));
            if query_requires_company_id(&query) && cid.is_none() {
                return Err(anyhow::anyhow!(
                    "query requires companyId; pass --company-id (VSV2_AQUATIQ_DATAMART_COMPANY_ID \
                     only applies to the pilot tenant)"
                ));
            }
            let dm = build_datamart(&cfg, None);
            let mut pager = CursorPaginator::new(page_size);
            pager.max_pages = max_pages;

            info!(
                tenant = %tid,
                company = cid.as_deref().unwrap_or("<none>"),
                root = %root_field,
                page_size,
                ?max_pages,
                "datamart pull",
            );
            let records =
                paginate_all(&dm, &tid, cid.as_deref(), &query, &root_field, &mut pager).await?;
            println!("{}", serde_json::to_string_pretty(&records)?);
            Ok(())
        }
        Cmd::Sync {
            tenant,
            endpoint,
            date_from,
            date_to,
            full,
            config_dir,
        } => {
            let cfg = AppConfig::from_env()?;
            let tid = TenantId::new(tenant);
            let state = StateStore::connect(&cfg.pg_dsn, &cfg.redis_url).await?;
            if let Some(cid) = cfg.env_company_id_for(&tid) {
                state.upsert_company_id(&tid, cid).await?;
            }
            let reg = EndpointRegistry::load_dir(Path::new(&config_dir))
                .map_err(|e| anyhow::anyhow!("load endpoint registry: {e}"))?;
            let exec = build_executor(reg, state, &cfg);

            if full {
                let names = match endpoint {
                    Some(ep) => vec![ep],
                    None => exec.endpoints_for_tier(0, u8::MAX),
                };
                let summary = exec.sync_names_full(&tid, &names).await?;
                info!(
                    tenant = %tid,
                    total = summary.records_synced,
                    failed = summary.failed_endpoints.len(),
                    "sync: complete (full scope)"
                );
                if let Some(first) = summary.failed_endpoints.first() {
                    anyhow::bail!(
                        "{} of {} endpoint(s) failed, first: {}: {}",
                        summary.failed_endpoints.len(),
                        summary.attempted_endpoints,
                        first.endpoint,
                        first.error
                    );
                }
                return Ok(());
            }
            let total = match endpoint {
                Some(ep) => exec
                    .sync_one_window(&tid, &ep, date_from, date_to)
                    .await
                    .map_err(anyhow::Error::from)?,
                None => exec
                    .sync_all_window(&tid, date_from, date_to)
                    .await
                    .map_err(anyhow::Error::from)?,
            };
            info!(tenant = %tid, total, "sync: complete");
            Ok(())
        }
        Cmd::Reconcile {
            tenant,
            endpoint,
            config_dir,
        } => {
            let cfg = AppConfig::from_env()?;
            let (project, dataset) = bq_required(&cfg)?;
            let reg = EndpointRegistry::load_dir(Path::new(&config_dir))
                .map_err(|e| anyhow::anyhow!("load endpoint registry: {e}"))?;
            let tid = TenantId::new(tenant);
            let reconciler = DeleteReconciler {
                client: build_erp_rest(&cfg, None),
                project,
                dataset,
            };

            let defs: Vec<_> = match endpoint {
                Some(ref name) => vec![reg
                    .get(name)
                    .ok_or_else(|| anyhow::anyhow!("unknown endpoint: {name}"))?
                    .clone()],
                None => reg.iter().filter(|def| def.enabled).cloned().collect(),
            };

            let mut total = 0u64;
            for def in &defs {
                if !def.enabled {
                    tracing::warn!(
                        endpoint = %def.endpoint_name,
                        "reconcile: endpoint is disabled, skipping"
                    );
                    continue;
                }
                match def.api_endpoint.as_ref().or(def.api_method.as_ref()) {
                    Some(api_path) => {
                        let primary_key = def.primary_key_arg();
                        match reconciler
                            .reconcile_endpoint(&tid, api_path, &def.bigquery_table, &primary_key)
                            .await
                        {
                            Ok(n) => total += n,
                            Err(e) => tracing::warn!(
                                endpoint = %def.endpoint_name, err = %e,
                                "reconcile: endpoint failed, continuing"
                            ),
                        }
                    }
                    None => tracing::warn!(
                        endpoint = %def.endpoint_name,
                        "reconcile: no api_endpoint/api_method, skipping"
                    ),
                }
            }
            info!(tenant = %tid, total, "reconcile: complete");
            Ok(())
        }
        Cmd::Flag {
            tenant,
            endpoint,
            source,
            enable,
        } => {
            let cfg = AppConfig::from_env()?;
            let pool = vsv2_state::connect_pool(&cfg.pg_dsn).await?;
            let tid = TenantId::new(tenant);
            let src = source.into_source();
            vsv2_state::endpoint_flags::set_flag(&pool, &tid, &endpoint, src, enable).await?;
            info!(tenant = %tid, %endpoint, source = %src, enable, "endpoint flag updated");
            Ok(())
        }
        Cmd::TenantsRegister {
            tenants_dir,
            dry_run,
        } => {
            let cfg = AppConfig::from_env()?;
            let tenants = vsv2_common::load_tenants(Path::new(&tenants_dir))?;
            let state = StateStore::connect(&cfg.pg_dsn, &cfg.redis_url).await?;
            println!(
                "{:<10} {:<34} {:<38} company",
                "result", "tenant", "tenant_id"
            );
            for tenant in &tenants {
                let outcome = if dry_run {
                    state.plan_tenant(tenant).await?
                } else {
                    state.register_tenant(tenant).await?
                };
                println!(
                    "{:<10} {:<34} {:<38} {}",
                    outcome.label(),
                    tenant.tenant_name,
                    tenant.tenant_id,
                    tenant.datamart_company_id.as_deref().unwrap_or("-")
                );
            }
            let registered = state.list_tenant_companies().await?.len();
            println!(
                "{} tenant file(s); {registered} row(s) in tenant_company_map{}",
                tenants.len(),
                if dry_run {
                    " (dry run, nothing written)"
                } else {
                    ""
                }
            );
            Ok(())
        }
        Cmd::FlagShow { tenant } => {
            let cfg = AppConfig::from_env()?;
            let pool = vsv2_state::connect_pool(&cfg.pg_dsn).await?;
            let tid = TenantId::new(tenant);
            let flags = vsv2_state::endpoint_flags::list_flags(&pool, &tid).await?;
            if flags.is_empty() {
                println!("No flags set for tenant {tid}");
            } else {
                println!("{:<40} {:<20} enabled", "endpoint", "source");
                println!("{}", "-".repeat(65));
                for f in &flags {
                    println!("{:<40} {:<20} {}", f.endpoint, f.source, f.enabled);
                }
            }
            Ok(())
        }
        Cmd::Schedule {
            tenant,
            tenants_dir,
            config_dir,
            once,
        } => {
            let cfg = AppConfig::from_env()?;
            let state = StateStore::connect(&cfg.pg_dsn, &cfg.redis_url).await?;
            let applied = state.migrate().await?;
            if applied > 0 {
                info!(applied, "applied migrations to the state database");
            }

            let configs = vsv2_common::load_tenants(Path::new(&tenants_dir))?;
            let chosen: Vec<_> = if tenant.is_empty() {
                configs.iter().collect()
            } else {
                for wanted in &tenant {
                    if !configs.iter().any(|c| &c.tenant_id == wanted) {
                        anyhow::bail!("tenant {wanted} is not in {tenants_dir}");
                    }
                }
                configs
                    .iter()
                    .filter(|c| tenant.contains(&c.tenant_id))
                    .collect()
            };
            for config in &chosen {
                state.register_tenant(config).await?;
                let tid = TenantId::new(config.tenant_id.clone());
                if let Some(cid) = cfg.env_company_id_for(&tid) {
                    state.upsert_company_id(&tid, cid).await?;
                }
            }
            let abandoned = state
                .abandon_stale_runs(vsv2_state::STALE_RUN_AFTER)
                .await?;
            if abandoned > 0 {
                tracing::warn!(
                    abandoned,
                    "marked runs left 'running' by a dead process as failed"
                );
            }

            let reg = EndpointRegistry::load_dir(Path::new(&config_dir))
                .map_err(|e| anyhow::anyhow!("load endpoint registry: {e}"))?;
            let exec = std::sync::Arc::new(build_executor(reg, state, &cfg));
            let specs = TierSpec::standard(
                &exec,
                Duration::from_secs(cfg.schedule_tier1_secs),
                Duration::from_secs(cfg.schedule_tier2_secs),
                Duration::from_secs(cfg.schedule_tier4_secs),
            );
            for spec in &specs {
                info!(
                    tier = spec.tier.label(),
                    endpoints = spec.endpoints.len(),
                    every_secs = spec.interval.as_secs(),
                    "scheduler: tier"
                );
            }
            let scheduler = Scheduler::new(
                exec,
                chosen.iter().map(|c| ScheduledTenant::from(*c)).collect(),
                cfg.schedule_parallel_tenants,
                Some(uuid::Uuid::new_v4().to_string()),
            );
            info!(
                tenants = chosen.len(),
                parallel = cfg.schedule_parallel_tenants,
                "scheduler: starting"
            );

            if once {
                let reports = scheduler.run_due_once(&specs).await?;
                for report in &reports {
                    info!(
                        tier = report.tier.label(),
                        tenant = %report.tenant,
                        attempted = report.attempted,
                        failed = report.failed.len(),
                        records = report.records,
                        "scheduler: pass done"
                    );
                }
                info!(passes = reports.len(), "scheduler: nothing more is due");
                return Ok(());
            }
            scheduler
                .run_forever(
                    &specs,
                    Duration::from_secs(cfg.schedule_poll_secs.max(1)),
                    shutdown_signal(),
                )
                .await?;
            Ok(())
        }
        Cmd::Monitor(args) => std::process::exit(monitor::run(args).await),
    }
}

/// Resolves on Ctrl-C or, where it exists, SIGTERM (what `docker stop` sends).
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        if let Ok(mut term) = signal(SignalKind::terminate()) {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

fn build_oauth(cfg: &AppConfig) -> OAuthClient {
    let client = OAuthClient::new(
        &cfg.oauth_token_url,
        &cfg.oauth_client_id,
        &cfg.oauth_client_secret,
    )
    .with_scope(Scope::Erp, cfg.oauth_scope_erp.clone())
    .with_scope(Scope::Datamart, cfg.oauth_scope_datamart.clone())
    .with_scope(Scope::SalesOrder, cfg.oauth_scope_salesorder.clone());
    if let Some(s) = cfg.oauth_scope_expense.clone() {
        client.with_scope(Scope::Expense, s)
    } else {
        client
    }
}

fn build_datamart(cfg: &AppConfig, redis: Option<deadpool_redis::Pool>) -> DatamartClient {
    let oauth = build_oauth(cfg);
    let mut limiter = Limiter::new(
        Source::DatamartGraphql,
        cfg.rate_dm_per_min,
        cfg.rate_dm_per_day,
    );
    if let Some(pool) = redis {
        limiter = limiter.with_redis(pool);
    }
    DatamartClient::new(&cfg.datamart_graphql_url, oauth, limiter)
}

fn query_requires_company_id(query: &str) -> bool {
    query.contains("$companyId")
}

async fn discover_company_ids(
    dm: &DatamartClient,
    tenant: &TenantId,
) -> anyhow::Result<serde_json::Value> {
    const PROBES: &[&str] = &["accounts", "branches", "customers", "locations"];

    for root_field in PROBES {
        let query =
            format!("query DiscoverCompanyIds {{ {root_field}(pageSize: 100) {{ companyId }} }}");
        match dm.execute(tenant, &query, json!({})).await {
            Ok(data) => {
                let mut ids = data
                    .get(*root_field)
                    .and_then(|value| value.as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|row| row.get("companyId"))
                    .filter_map(|value| value.as_i64())
                    .collect::<Vec<_>>();
                ids.sort_unstable();
                ids.dedup();

                if !ids.is_empty() {
                    return Ok(json!({
                        "probeField": root_field,
                        "companyIds": ids,
                    }));
                }
            }
            Err(err) => {
                warn!(tenant = %tenant, root = %root_field, error = %err, "datamart company-id probe failed");
            }
        }
    }

    Err(anyhow::anyhow!(
        "could not discover company IDs from DataMart; verify the tenant has at least one active ETL"
    ))
}

fn build_erp_rest(cfg: &AppConfig, redis: Option<deadpool_redis::Pool>) -> ErpRestClient {
    let oauth = build_oauth(cfg);
    let mut limiter = Limiter::new(
        Source::ErpRest,
        cfg.rate_rest_per_min,
        cfg.rate_rest_per_day,
    );
    if let Some(pool) = redis {
        limiter = limiter.with_redis(pool);
    }
    ErpRestClient::new(&cfg.erp_base_url, oauth, limiter)
}

fn build_expense(cfg: &AppConfig, redis: Option<deadpool_redis::Pool>) -> Option<ExpenseClient> {
    let base_url = cfg.expense_base_url.clone()?;
    cfg.oauth_scope_expense.as_ref()?;
    let oauth = build_oauth(cfg);
    let mut limiter = vsv2_ratelimit::Limiter::new(
        vsv2_common::Source::ExpenseApi,
        cfg.rate_expense_per_min,
        cfg.rate_expense_per_day,
    );
    if let Some(pool) = redis {
        limiter = limiter.with_redis(pool);
    }
    Some(ExpenseClient::new(base_url, oauth, limiter))
}

fn build_executor(reg: EndpointRegistry, state: StateStore, cfg: &AppConfig) -> SyncExecutor {
    let redis = state.redis.clone();
    let erp_adapter = ErpRestAdapter {
        client: build_erp_rest(cfg, Some(redis.clone())),
        page_size: 100,
    };
    let dm_adapter = DatamartAdapter {
        client: build_datamart(cfg, Some(redis.clone())),
        state: state.clone(),
        page_size: 500,
    };
    let expense_box: Box<dyn vsv2_orchestrator::FetchAdapter> = match build_expense(
        cfg,
        Some(redis),
    ) {
        Some(client) => Box::new(ExpenseAdapter {
            client,
            page_size: 200,
        }),
        None => {
            tracing::warn!("VISMA_EXPENSE_API_BASE_URL / VISMA_OAUTH_SCOPE_EXPENSE not set — expense endpoints will no-op");
            Box::new(NoopAdapter {
                source_name: "expense_api",
            })
        }
    };
    let dispatch = DispatchTable {
        erp: Box::new(erp_adapter),
        datamart: Box::new(dm_adapter),
        expense: expense_box,
    };
    let bq = BqWriter::from_config(cfg);
    if bq.is_none() {
        if cfg.local_bronze_pg_enabled {
            tracing::info!(
                "VSV2_BIGQUERY_* not set; writing sync payloads to local Postgres bronze"
            );
        } else {
            tracing::info!(
                "VSV2_BIGQUERY_PROJECT and a dataset not set; BQ writes skipped (dry-run)"
            );
        }
    }
    SyncExecutor {
        registry: reg,
        dispatch,
        state,
        bq,
        local_bronze_enabled: cfg.local_bronze_pg_enabled,
    }
}

fn bq_required(cfg: &AppConfig) -> anyhow::Result<(String, String)> {
    let project = cfg
        .bq_project
        .clone()
        .ok_or_else(|| anyhow::anyhow!("VSV2_BIGQUERY_PROJECT must be set for this command"))?;
    let dataset = cfg.bq_dataset_bronze.clone().ok_or_else(|| {
        anyhow::anyhow!("VSV2_BIGQUERY_DATASET_BRONZE must be set for this command")
    })?;
    Ok((project, dataset))
}
