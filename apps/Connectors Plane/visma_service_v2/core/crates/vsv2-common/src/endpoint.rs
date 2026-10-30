//! Endpoint registry: typed view of `config/endpoints/*.json`.
//!
//! Each file describes one logical endpoint (e.g. `customers`, `accounts`),
//! including its default `source`, primary key, and optional DataMart wiring.
//! The `resolve` function picks the effective `Source` for a given tenant,
//! taking into account an optional DB-level operator override.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::source::Source;
use crate::tenant::TenantId;

/// Primary key spec — single column or composite.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum PrimaryKey {
    Single(String),
    Composite(Vec<String>),
}

/// REST pagination/query contract for endpoint-level fetches.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PaginationProfile {
    #[serde(rename = "finance_page_number")]
    FinancePageNumber,
    /// Finance endpoints that declare no `pageNumber`: a row count and an offset (`numberToRead`,
    /// `skipRecords`) instead.
    #[serde(rename = "numbertoread_skip")]
    NumberToReadSkip,
    #[serde(rename = "salesorder_page_index")]
    SalesOrderPageIndex,
    #[serde(rename = "journaltx_page_index")]
    JournalTxPageIndex,
    #[serde(rename = "none")]
    None,
}

/// Optional Visma Finance background execution mode.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum BackgroundMode {
    #[serde(rename = "off")]
    Off,
    #[serde(rename = "none")]
    None,
    #[serde(rename = "subscription")]
    Subscription,
}

fn default_enabled() -> bool {
    true
}

impl Default for PaginationProfile {
    fn default() -> Self {
        Self::FinancePageNumber
    }
}

impl Default for BackgroundMode {
    fn default() -> Self {
        Self::Off
    }
}

/// DataMart-specific wiring for an endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DatamartConfig {
    /// Globally ready for DataMart? When true, default route is DataMart.
    #[serde(default)]
    pub ready: bool,

    /// Per-tenant overrides keyed by tenant UUID. `true` forces DataMart,
    /// `false` forces the registry `source` (typically ERP REST).
    #[serde(default)]
    pub tenant_overrides: HashMap<String, bool>,

    #[serde(default)]
    pub query_file: Option<String>,

    #[serde(default)]
    pub type_name: Option<String>,

    #[serde(default)]
    pub cursor_field: Option<String>,

    #[serde(default)]
    pub id_field: Option<String>,

    #[serde(default)]
    pub company_filter: bool,

    #[serde(default)]
    pub join_normalizer: Option<String>,

    /// Source to fall back to if DataMart is unavailable. Optional.
    #[serde(default)]
    pub fallback_source: Option<Source>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PartitioningConfig {
    #[serde(default = "default_partition_mode")]
    pub mode: String,
    pub dimensions: Vec<PartitionDimensionConfig>,
    #[serde(default = "default_max_partitions")]
    pub max_partitions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PartitionDimensionConfig {
    pub query_param: String,
    pub source_endpoint: String,
    pub source_path: String,
    #[serde(default)]
    pub fallback_values: Vec<String>,
}

fn default_partition_mode() -> String {
    "cartesian".to_string()
}

fn default_max_partitions() -> usize {
    10_000
}

/// Typed view of one `config/endpoints/<name>.json` file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointConfig {
    pub endpoint_name: String,
    pub bigquery_table: String,
    pub primary_key: PrimaryKey,
    pub source: Source,

    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub api_method: Option<String>,
    #[serde(default)]
    pub api_endpoint: Option<String>,
    #[serde(default)]
    pub fallback_api_method: Option<String>,
    #[serde(default)]
    pub pagination_profile: PaginationProfile,
    #[serde(default)]
    pub page_size: Option<u32>,
    #[serde(default)]
    pub background_mode: BackgroundMode,
    #[serde(default)]
    pub background_poll_secs: Option<u64>,
    #[serde(default)]
    pub background_timeout_secs: Option<u64>,
    #[serde(default)]
    pub default_query: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub required_query: Vec<String>,
    #[serde(default)]
    pub modified_field: Option<String>,
    #[serde(default)]
    pub merge_type: Option<String>,
    #[serde(default)]
    pub sync_pattern: Option<String>,
    #[serde(default)]
    pub validation_fields: Vec<String>,

    #[serde(default)]
    pub partitioning: Option<PartitioningConfig>,

    #[serde(default)]
    pub datamart: Option<DatamartConfig>,
}

/// Load every `*.json` file (except `_schema.json`) from `dir` into a registry
/// keyed by `endpoint_name`.
pub fn load_registry(dir: &Path) -> Result<HashMap<String, EndpointConfig>> {
    let mut out = HashMap::new();
    let entries = std::fs::read_dir(dir)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if stem.starts_with('_') {
            continue;
        }
        let bytes = std::fs::read(&path)?;
        let cfg: EndpointConfig = serde_json::from_slice(&bytes).map_err(|e| {
            Error::Config(format!("invalid endpoint config {}: {e}", path.display()))
        })?;
        if cfg.endpoint_name != stem {
            return Err(Error::Config(format!(
                "endpoint_name {:?} does not match filename {:?}",
                cfg.endpoint_name, stem
            )));
        }
        out.insert(cfg.endpoint_name.clone(), cfg);
    }
    Ok(out)
}

/// Resolve the effective `Source` for `tenant` given the registry entry and
/// an optional DB-level operator override.
///
/// Priority:
///   1. `db_flag` (operator override stored in `vsv2.endpoint_flags`)
///   2. `cfg.datamart.tenant_overrides[tenant]`: `Some(true)` → DatamartGraphql, `Some(false)` → cfg.source
///   3. `cfg.datamart.ready == true` ⇒ `Source::DatamartGraphql`
///   4. `cfg.source`
pub fn resolve(cfg: &EndpointConfig, tenant: &TenantId, db_flag: Option<Source>) -> Source {
    if let Some(s) = db_flag {
        return s;
    }
    if let Some(dm) = &cfg.datamart {
        if let Some(&forced) = dm.tenant_overrides.get(tenant.as_str()) {
            return if forced {
                Source::DatamartGraphql
            } else {
                cfg.source
            };
        }
        if dm.ready {
            return Source::DatamartGraphql;
        }
    }
    cfg.source
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(source: Source, datamart: Option<DatamartConfig>) -> EndpointConfig {
        EndpointConfig {
            endpoint_name: "customers".into(),
            bigquery_table: "visma_customers".into(),
            primary_key: PrimaryKey::Single("number".into()),
            source,
            enabled: true,
            api_method: None,
            api_endpoint: None,
            fallback_api_method: None,
            pagination_profile: PaginationProfile::default(),
            page_size: None,
            background_mode: BackgroundMode::default(),
            background_poll_secs: None,
            background_timeout_secs: None,
            default_query: HashMap::new(),
            required_query: vec![],
            modified_field: None,
            merge_type: None,
            sync_pattern: None,
            validation_fields: vec![],
            partitioning: None,
            datamart,
        }
    }

    #[test]
    fn db_flag_wins() {
        let c = cfg(
            Source::ErpRest,
            Some(DatamartConfig {
                ready: true,
                ..Default::default()
            }),
        );
        let t = TenantId::new("t");
        assert_eq!(resolve(&c, &t, Some(Source::ErpRest)), Source::ErpRest);
        assert_eq!(
            resolve(&c, &t, Some(Source::DatamartGraphql)),
            Source::DatamartGraphql
        );
    }

    #[test]
    fn tenant_override_true_forces_datamart() {
        let mut overrides = HashMap::new();
        overrides.insert("t".to_string(), true);
        let c = cfg(
            Source::ErpRest,
            Some(DatamartConfig {
                ready: false,
                tenant_overrides: overrides,
                ..Default::default()
            }),
        );
        assert_eq!(
            resolve(&c, &TenantId::new("t"), None),
            Source::DatamartGraphql
        );
    }

    #[test]
    fn tenant_override_false_forces_default_source() {
        let mut overrides = HashMap::new();
        overrides.insert("t".to_string(), false);
        let c = cfg(
            Source::ErpRest,
            Some(DatamartConfig {
                ready: true,
                tenant_overrides: overrides,
                ..Default::default()
            }),
        );
        assert_eq!(resolve(&c, &TenantId::new("t"), None), Source::ErpRest);
    }

    #[test]
    fn datamart_ready_promotes() {
        let c = cfg(
            Source::ErpRest,
            Some(DatamartConfig {
                ready: true,
                ..Default::default()
            }),
        );
        assert_eq!(
            resolve(&c, &TenantId::new("t"), None),
            Source::DatamartGraphql
        );
    }

    #[test]
    fn falls_back_to_registry_source() {
        let c = cfg(Source::ErpRest, None);
        assert_eq!(resolve(&c, &TenantId::new("t"), None), Source::ErpRest);
    }
}
