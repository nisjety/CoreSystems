use serde::{Deserialize, Serialize};
use std::env;

use crate::{Error, Result};

/// Global runtime config loaded from env. One per process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub oauth_client_id: String,
    pub oauth_client_secret: String,
    pub oauth_token_url: String,
    pub oauth_scope_erp: String,
    pub oauth_scope_datamart: String,
    pub oauth_scope_expense: Option<String>,
    pub oauth_scope_salesorder: String,
    /// Layer 3 write scopes. `None` keeps that write path unconfigured (no token can be minted).
    pub oauth_scope_erp_write: Option<String>,
    pub oauth_scope_salesorder_write: Option<String>,

    pub erp_base_url: String,
    pub datamart_graphql_url: String,
    pub expense_base_url: Option<String>,

    pub aquatiq_tenant_id: String,
    pub aquatiq_datamart_company_id: Option<String>,

    pub pg_dsn: String,
    pub redis_url: String,

    pub bq_project: Option<String>,
    pub bq_dataset_bronze: Option<String>,
    /// Raw staging dataset (`dw_staging_visma`). When set, syncs write one JSON table per endpoint
    /// there instead of merging into typed bronze tables.
    pub bq_dataset_staging: Option<String>,
    pub bq_dataset_shadow: Option<String>,
    pub local_bronze_pg_enabled: bool,

    pub rate_rest_per_min: u32,
    pub rate_rest_per_day: u32,
    pub rate_dm_per_min: u32,
    pub rate_dm_per_day: u32,
    pub rate_expense_per_min: u32,
    pub rate_expense_per_day: u32,

    /// Scheduler intervals in seconds (overridable for dev/test).
    pub schedule_tier1_secs: u64,
    pub schedule_tier2_secs: u64,
    pub schedule_tier4_secs: u64,
    /// How often the scheduler looks at what is due.
    pub schedule_poll_secs: u64,
    /// Tenants the scheduler works on at the same time.
    pub schedule_parallel_tenants: usize,
}

/// The company id `VSV2_AQUATIQ_DATAMART_COMPANY_ID` stands for, if it is `tenant`'s.
///
/// The variable is the pilot tenant's. Handing it to any other tenant would make that tenant's
/// DataMart sync read the pilot's company, so every other tenant's company comes from the state
/// store (`vsv2 tenants-register`).
pub fn env_company_id_for<'a>(
    pilot_tenant: &str,
    env_company_id: Option<&'a str>,
    tenant: &crate::TenantId,
) -> Option<&'a str> {
    (tenant.as_str() == pilot_tenant)
        .then_some(env_company_id)
        .flatten()
}

impl AppConfig {
    /// See [`env_company_id_for`].
    pub fn env_company_id_for(&self, tenant: &crate::TenantId) -> Option<&str> {
        env_company_id_for(
            &self.aquatiq_tenant_id,
            self.aquatiq_datamart_company_id.as_deref(),
            tenant,
        )
    }

    /// Load from environment. Honours `.env` if present.
    /// Fail-fast: missing required keys raise [`Error::Config`] at boot.
    pub fn from_env() -> Result<Self> {
        let _ = dotenvy::dotenv();
        let erp_scope = req_any(&["VISMA_SCOPES", "VISMA_OAUTH_SCOPE"])?;

        Ok(Self {
            oauth_client_id: req("VISMA_CLIENT_ID")?,
            oauth_client_secret: req("VISMA_CLIENT_SECRET")?,
            oauth_token_url: opt("VISMA_TOKEN_URL")
                .unwrap_or_else(|| "https://connect.visma.com/connect/token".into()),
            oauth_scope_erp: erp_scope.clone(),
            oauth_scope_datamart: opt("VISMA_OAUTH_SCOPE_DATAMART")
                .unwrap_or_else(|| erp_scope.clone()),
            oauth_scope_expense: opt("VISMA_OAUTH_SCOPE_EXPENSE"),
            oauth_scope_salesorder: opt("VISMA_OAUTH_SCOPE_SALESORDER")
                .unwrap_or_else(|| erp_scope.clone()),
            oauth_scope_erp_write: opt("VISMA_OAUTH_SCOPE_ERP_WRITE"),
            oauth_scope_salesorder_write: opt("VISMA_OAUTH_SCOPE_SALESORDER_WRITE"),

            erp_base_url: opt("VISMA_ERP_REST_BASE_URL")
                .unwrap_or_else(|| "https://api.finance.visma.net".into()),
            datamart_graphql_url: opt("VISMA_DATAMART_GRAPHQL_URL")
                .unwrap_or_else(|| "https://datamart.visma.net/graphql".into()),
            expense_base_url: opt("VISMA_EXPENSE_API_BASE_URL"),

            aquatiq_tenant_id: opt("VSV2_PILOT_TENANT_ID")
                .unwrap_or_else(|| "33f319b9-4d57-11ee-960c-025417856183".into()),
            aquatiq_datamart_company_id: opt("VSV2_AQUATIQ_DATAMART_COMPANY_ID"),

            pg_dsn: req("VSV2_DATABASE_URL")?,
            redis_url: req("VSV2_REDIS_URL")?,

            bq_project: opt("VSV2_BIGQUERY_PROJECT"),
            bq_dataset_bronze: opt("VSV2_BIGQUERY_DATASET_BRONZE"),
            bq_dataset_staging: opt("VSV2_BIGQUERY_DATASET_STAGING"),
            bq_dataset_shadow: opt("VSV2_BIGQUERY_DATASET_SHADOW"),
            local_bronze_pg_enabled: opt_bool("VSV2_LOCAL_BRONZE_POSTGRES_ENABLED")
                .unwrap_or(false),

            rate_rest_per_min: opt_u32("VSV2_RATELIMIT_ERP_REST_RPM").unwrap_or(60),
            rate_rest_per_day: opt_u32("VSV2_RATELIMIT_ERP_REST_RPD").unwrap_or(5_000),
            rate_dm_per_min: opt_u32("VSV2_RATELIMIT_DATAMART_RPM").unwrap_or(60),
            rate_dm_per_day: opt_u32("VSV2_RATELIMIT_DATAMART_RPD").unwrap_or(10_000),
            rate_expense_per_min: opt_u32("VSV2_RATELIMIT_EXPENSE_RPM").unwrap_or(30),
            rate_expense_per_day: opt_u32("VSV2_RATELIMIT_EXPENSE_RPD").unwrap_or(2_000),

            schedule_tier1_secs: opt_u64("VSV2_SCHEDULE_TIER1_SECS").unwrap_or(3_600),
            schedule_tier2_secs: opt_u64("VSV2_SCHEDULE_TIER2_SECS").unwrap_or(86_400),
            schedule_tier4_secs: opt_u64("VSV2_SCHEDULE_TIER4_SECS").unwrap_or(604_800),
            schedule_poll_secs: opt_u64("VSV2_SCHEDULE_POLL_SECS").unwrap_or(60),
            schedule_parallel_tenants: opt_u64("VSV2_SCHEDULE_PARALLEL_TENANTS").unwrap_or(4)
                as usize,
        })
    }
}

fn req(key: &str) -> Result<String> {
    env::var(key).map_err(|_| Error::Config(format!("missing required env var: {key}")))
}

fn req_any(keys: &[&str]) -> Result<String> {
    for key in keys {
        if let Some(value) = opt(key) {
            return Ok(value);
        }
    }
    Err(Error::Config(format!(
        "missing required env var: one of {}",
        keys.join(", ")
    )))
}

fn opt(key: &str) -> Option<String> {
    env::var(key).ok().filter(|s| !s.is_empty())
}

fn opt_u32(key: &str) -> Option<u32> {
    opt(key).and_then(|s| s.parse().ok())
}

fn opt_u64(key: &str) -> Option<u64> {
    opt(key).and_then(|s| s.parse().ok())
}

fn opt_bool(key: &str) -> Option<bool> {
    opt(key).and_then(|s| {
        let v = s.trim().to_ascii_lowercase();
        match v.as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        }
    })
}

#[cfg(test)]
mod company_tests {
    use super::env_company_id_for;
    use crate::TenantId;

    const PILOT: &str = "33f319b9-4d57-11ee-960c-025417856183";

    #[test]
    fn the_env_company_id_belongs_to_the_pilot_tenant_only() {
        let sensilist = TenantId::new("eda0dd16-fa25-11ed-a0e0-0693d8a7c3dd");

        assert_eq!(
            env_company_id_for(PILOT, Some("4839725"), &TenantId::new(PILOT)),
            Some("4839725")
        );
        assert_eq!(env_company_id_for(PILOT, Some("4839725"), &sensilist), None);
        assert_eq!(env_company_id_for(PILOT, None, &TenantId::new(PILOT)), None);
    }
}
