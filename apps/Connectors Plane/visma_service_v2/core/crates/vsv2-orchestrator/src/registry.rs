use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::Path};
use vsv2_common::{BackgroundMode, Error, PaginationProfile, PrimaryKey, Result, Source, TenantId};

use crate::partition::PartitioningConfig;

/// Per-tenant override flag block. Lives inside an endpoint config as
/// `datamart: { ready, tenant_overrides, ... }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatamartBinding {
    /// Master flag — when `false`, this endpoint never goes to DataMart.
    pub ready: bool,
    /// Per-tenant overrides keyed by `tenant_id`.
    /// Pilot: only Aquatiq AS = `true`.
    #[serde(default)]
    pub tenant_overrides: HashMap<String, bool>,
    #[serde(default)]
    pub query_file: Option<String>,
    #[serde(default)]
    pub type_name: Option<String>,
    #[serde(default)]
    pub root_field: Option<String>,
    #[serde(default)]
    pub cursor_field: Option<String>,
    #[serde(default)]
    pub id_field: Option<String>,
    #[serde(default = "default_true")]
    pub company_filter: bool,
    pub join_normalizer: Option<String>,
    /// Fallback when DM is disabled or returns an error.
    #[serde(default = "default_fallback_source")]
    pub fallback_source: Source,
}

fn default_true() -> bool {
    true
}
fn default_fallback_source() -> Source {
    Source::ErpRest
}

fn default_enabled() -> bool {
    true
}

/// An endpoint that is fetched one financial period (YYYYMM) at a time.
///
/// Some Finance endpoints only answer correctly per period: `/v1/projecttransaction` demands
/// `fromPeriod`/`toPeriod`, and `/v2/generalLedgerBalance` returns the balances *modified* in a
/// period when asked by `lastModifiedDateTime` but every balance of the period when asked by
/// `periodId` (2,691 against 981 rows for one month). A window of several months is not an
/// alternative: every page gets slower until requests time out.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeriodScanConfig {
    /// First period that can hold data. There is no default: starting at the current month
    /// would fetch a fraction of the data and look like success.
    pub first_period: String,
    /// Query parameters that each receive the period being fetched: `["fromPeriod", "toPeriod"]`
    /// for a range endpoint (both get the same month), `["periodId"]` for a single-period one.
    pub query_params: Vec<String>,
    /// Months after the current one that are walked too, for data posted to future periods.
    #[serde(default)]
    pub look_ahead_months: u32,
    /// Name of a field added to every row with the period it was fetched under.
    ///
    /// Needed when one key comes back in several periods with different content: a `ModulePM`
    /// project transaction keeps its `refNbr` month after month, each month's window holding only
    /// that month's lines (`1400592` returned 68, 66 and 66 lines in 202310, 202311 and 202312).
    /// Keyed on `module` + `refNbr` alone, the later months would overwrite the earlier ones. The
    /// field is part of the primary key; the API's own rows never carry it.
    #[serde(default)]
    pub period_field: Option<String>,
    /// Apply the checkpoint watermark inside each period (`lastModifiedDateTime >= watermark`)
    /// and still walk every period, instead of re-reading everything on every run.
    ///
    /// Each period is then one short request when little changed, and a full pass (no
    /// watermark) walks the same periods without the filter. It needs an endpoint whose rows
    /// carry a modification time that moves whenever they change. `general_ledger_balances_v2`
    /// does not qualify: a carried-forward balance changes with postings in earlier periods
    /// without its own timestamp moving, so it is read in full each run.
    #[serde(default)]
    pub resume: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointDefinition {
    pub endpoint_name: String,
    pub bigquery_table: String,
    pub primary_key: PrimaryKey,
    #[serde(default = "default_source")]
    pub source: Source,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub api_method: Option<String>,
    #[serde(default)]
    pub api_endpoint: Option<String>,
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
    pub modified_field: Option<String>,
    pub merge_type: Option<String>,
    pub sync_pattern: Option<String>,
    pub validation_fields: Option<Vec<String>>,
    #[serde(default)]
    pub partitioning: Option<PartitioningConfig>,
    #[serde(default)]
    pub period_scan: Option<PeriodScanConfig>,
    pub datamart: Option<DatamartBinding>,
    /// Sync tier (1 = hourly operational, 2–3 = daily, 4–5 = weekly/reference).
    /// None = daily by default. Enforced by config validation (Invariant #4).
    pub tier: Option<u8>,
    /// Once a day per tenant, compare the documents staging holds as open with Visma's own list of
    /// open documents and fetch the differences by key (see `crate::open_set`).
    ///
    /// For endpoints whose documents change without their `modified_field` moving: a payment
    /// closes an invoice and leaves its `lastModifiedDateTime` alone, so a sync that follows that
    /// timestamp keeps the invoice open until the weekly full pass. Default `false`.
    #[serde(default)]
    pub open_set: bool,
}

fn default_source() -> Source {
    Source::ErpRest
}

impl EndpointDefinition {
    /// Effective source for a given tenant — applies per-tenant overrides.
    pub fn effective_source(&self, tenant: &TenantId) -> Source {
        if let Some(dm) = &self.datamart {
            let allowed = dm.ready
                || dm
                    .tenant_overrides
                    .get(tenant.as_str())
                    .copied()
                    .unwrap_or(false);
            if allowed {
                return Source::DatamartGraphql;
            }
        }
        self.source
    }

    /// True when the endpoint is fetched one financial period at a time (see `PeriodScanConfig`).
    pub fn scans_financial_periods(&self) -> bool {
        self.period_scan.is_some()
    }

    /// True when a stored watermark must not narrow the fetch: the endpoint is read period by
    /// period in full on every run (its checkpoint cannot say which periods are up to date).
    pub fn period_scan_ignores_checkpoint(&self) -> bool {
        self.period_scan.as_ref().is_some_and(|scan| !scan.resume)
    }

    /// A DataMart shadow candidate must not replace the live ERP sync.
    pub fn live_source(&self, requested: Source) -> Result<Source> {
        if requested != Source::DatamartGraphql {
            return Ok(requested);
        }

        let dm = self.datamart.as_ref().ok_or_else(|| {
            Error::Config(format!(
                "endpoint '{}' has no DataMart binding for the requested source",
                self.endpoint_name
            ))
        })?;
        if dm.ready {
            return Ok(requested);
        }
        if dm.fallback_source == Source::DatamartGraphql {
            return Err(Error::Config(format!(
                "endpoint '{}' has no non-DataMart fallback for live sync",
                self.endpoint_name
            )));
        }
        Ok(dm.fallback_source)
    }

    pub fn primary_key_arg(&self) -> String {
        match &self.primary_key {
            PrimaryKey::Single(pk) => pk.clone(),
            PrimaryKey::Composite(parts) => parts.join(","),
        }
    }

    /// Why `open_set` cannot work for this endpoint, if it cannot (`None` when the flag is off).
    ///
    /// The reconciliation lists `<api_endpoint>?status=Open` page by page, fetches single
    /// documents at `<api_endpoint>/<key values in primary key order>`, and runs in the hourly
    /// pass only. An endpoint that does not fit would not fail loudly later, it would just never
    /// reconcile anything, so the registry refuses it when it loads.
    pub fn open_set_problem(&self) -> Option<String> {
        if !self.open_set {
            return None;
        }
        let name = &self.endpoint_name;
        let fail = |reason: &str| Some(format!("endpoint '{name}': open_set needs {reason}"));
        if self.source != Source::ErpRest {
            return fail("source erp_rest");
        }
        match self.api_endpoint.as_deref().map(str::trim) {
            Some(path) if !path.is_empty() && !path.contains(['{', '?']) => {}
            _ => return fail("an api_endpoint without placeholders or a query string"),
        }
        if self.pagination_profile != PaginationProfile::FinancePageNumber {
            return fail("pagination_profile finance_page_number");
        }
        if self.tier != Some(1) {
            return fail("tier 1: only the hourly pass runs it");
        }
        if self.partitioning.is_some() || self.period_scan.is_some() {
            return fail("an endpoint that is not partitioned or read by period");
        }
        // The key is read the way `scoped_primary_key` reads it: a single string may name several
        // fields. They have to be top-level fields of the document, since the by-key request puts
        // their values in the path and the key tool prints them under their own names.
        let key_fields: Vec<String> = self
            .primary_key_arg()
            .split(',')
            .map(|field| field.trim().to_string())
            .filter(|field| !field.is_empty())
            .collect();
        let usable = |field: &String| {
            field != "tenant_id" && field != "_partition_key" && !field.contains('.')
        };
        if key_fields.is_empty() || !key_fields.iter().all(usable) {
            return fail(
                "a primary key of top-level fields (no dots, tenant_id or _partition_key)",
            );
        }
        None
    }
}

#[derive(Debug, Default, Clone)]
pub struct EndpointRegistry {
    by_name: HashMap<String, EndpointDefinition>,
}

impl EndpointRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn load_dir(dir: &Path) -> Result<Self> {
        let mut reg = Self::new();
        for entry in std::fs::read_dir(dir).map_err(Error::Io)? {
            let path = entry.map_err(Error::Io)?.path();
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if stem.starts_with('_') || path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let raw = std::fs::read_to_string(&path).map_err(Error::Io)?;
            let def: EndpointDefinition = serde_json::from_str(&raw)?;
            if let Some(problem) = def.open_set_problem() {
                return Err(Error::Config(problem));
            }
            reg.by_name.insert(def.endpoint_name.clone(), def);
        }
        Ok(reg)
    }

    pub fn get(&self, name: &str) -> Option<&EndpointDefinition> {
        self.by_name.get(name)
    }

    pub fn iter(&self) -> impl Iterator<Item = &EndpointDefinition> {
        self.by_name.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def_with_dm(ready: bool, override_for: Option<(&str, bool)>) -> EndpointDefinition {
        let mut overrides = HashMap::new();
        if let Some((t, v)) = override_for {
            overrides.insert(t.to_string(), v);
        }
        EndpointDefinition {
            endpoint_name: "customers".into(),
            bigquery_table: "visma_customers".into(),
            primary_key: PrimaryKey::Single("number".into()),
            source: Source::ErpRest,
            enabled: true,
            api_method: Some("get_customers".into()),
            api_endpoint: Some("v1/customer".into()),
            fallback_api_method: Some("get_customers_v3".into()),
            pagination_profile: PaginationProfile::FinancePageNumber,
            page_size: None,
            background_mode: BackgroundMode::Off,
            background_poll_secs: None,
            background_timeout_secs: None,
            default_query: HashMap::new(),
            required_query: vec![],
            modified_field: None,
            merge_type: Some("upsert".into()),
            sync_pattern: Some("full_refresh_upsert".into()),
            validation_fields: None,
            partitioning: None,
            period_scan: None,
            datamart: Some(DatamartBinding {
                ready,
                tenant_overrides: overrides,
                query_file: Some("config/queries/customer.graphql".into()),
                type_name: Some("Customer".into()),
                root_field: Some("customers".into()),
                cursor_field: Some("updatedInDataMartDateTime".into()),
                id_field: Some("id".into()),
                company_filter: true,
                join_normalizer: Some("normalize_customer_with_locations".into()),
                fallback_source: Source::ErpRest,
            }),
            tier: None,
            open_set: false,
        }
    }

    const AQUATIQ: &str = "33f319b9-4d57-11ee-960c-025417856183";
    const OTHER: &str = "00000000-0000-0000-0000-000000000000";

    #[test]
    fn ready_master_flag_routes_all_tenants_to_dm() {
        let def = def_with_dm(true, None);
        let tenant = TenantId::new(OTHER);
        assert_eq!(def.effective_source(&tenant), Source::DatamartGraphql);
    }

    #[test]
    fn unready_with_aquatiq_override_flips_only_aquatiq() {
        let def = def_with_dm(false, Some((AQUATIQ, true)));
        let pilot = TenantId::new(AQUATIQ);
        let bystander = TenantId::new(OTHER);
        assert_eq!(def.effective_source(&pilot), Source::DatamartGraphql);
        assert_eq!(def.effective_source(&bystander), Source::ErpRest);
    }

    #[test]
    fn unready_no_override_stays_on_rest() {
        let def = def_with_dm(false, None);
        let pilot = TenantId::new(AQUATIQ);
        assert_eq!(def.effective_source(&pilot), Source::ErpRest);
    }

    #[test]
    fn unready_datamart_operator_flag_keeps_live_sync_on_erp() {
        let def = def_with_dm(false, None);
        assert_eq!(
            def.live_source(Source::DatamartGraphql).unwrap(),
            Source::ErpRest
        );
    }

    #[test]
    fn ready_datamart_operator_flag_can_use_datamart_for_live_sync() {
        let def = def_with_dm(true, None);
        assert_eq!(
            def.live_source(Source::DatamartGraphql).unwrap(),
            Source::DatamartGraphql
        );
    }

    #[test]
    fn datamart_operator_flag_without_binding_is_rejected() {
        let mut def = def_with_dm(false, None);
        def.datamart = None;
        assert!(def.live_source(Source::DatamartGraphql).is_err());
    }

    #[test]
    fn no_datamart_block_means_rest() {
        let def = EndpointDefinition {
            endpoint_name: "warehouses".into(),
            bigquery_table: "visma_warehouses".into(),
            primary_key: PrimaryKey::Single("warehouseID".into()),
            source: Source::ErpRest,
            enabled: true,
            api_method: None,
            api_endpoint: None,
            fallback_api_method: None,
            pagination_profile: PaginationProfile::FinancePageNumber,
            page_size: None,
            background_mode: BackgroundMode::Off,
            background_poll_secs: None,
            background_timeout_secs: None,
            default_query: HashMap::new(),
            required_query: vec![],
            modified_field: None,
            merge_type: None,
            sync_pattern: None,
            validation_fields: None,
            partitioning: None,
            period_scan: None,
            datamart: None,
            tier: None,
            open_set: false,
        };
        assert_eq!(
            def.effective_source(&TenantId::new(AQUATIQ)),
            Source::ErpRest,
        );
    }

    #[test]
    fn loads_workspace_endpoint_configs() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("config/endpoints");

        let reg = EndpointRegistry::load_dir(&dir).expect("real endpoint configs must load");

        assert!(reg.iter().count() >= 70);
        assert!(matches!(
            reg.get("general_ledger_transactions")
                .expect("general ledger transactions endpoint")
                .primary_key,
            PrimaryKey::Composite(_)
        ));
        assert!(
            reg.get("sales_orders_v3")
                .expect("sales order v3 endpoint")
                .enabled,
            "SalesOrder V3 must remain the active sales-order read endpoint"
        );
        assert!(
            reg.get("sales_orders_v2").is_none(),
            "SalesOrder V2 is deprecated/EOL and must stay removed"
        );
    }

    /// GL balances and budgets retain their DataMart queries and normalizers,
    /// but neither may displace ERP until its shadow comparison passes.
    #[test]
    fn unproven_pilot_candidates_stay_on_erp() {
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let reg = EndpointRegistry::load_dir(&repo_root.join("config/endpoints"))
            .expect("real endpoint configs must load");

        let pilot = TenantId::new(AQUATIQ);
        let bystander = TenantId::new(OTHER);

        for name in ["general_ledger_balances_v2", "budgets"] {
            let def = reg.get(name).unwrap_or_else(|| panic!("{name} endpoint"));
            assert_eq!(
                def.effective_source(&pilot),
                Source::ErpRest,
                "{name}: Aquatiq must stay on ERP until shadow parity passes"
            );
            assert_eq!(
                def.effective_source(&bystander),
                Source::ErpRest,
                "{name}: non-pilot tenants must stay on ERP-REST"
            );
            let dm = def.datamart.as_ref().expect("datamart binding");
            assert!(!dm.ready, "{name}: global ready must stay false");
            assert_eq!(dm.tenant_overrides.get(AQUATIQ), Some(&false));
            let qf = dm.query_file.as_deref().expect("query_file");
            assert!(
                qf.starts_with("config/queries/datamart/"),
                "{name}: query_file must point at datamart/ canonical query, got {qf}"
            );
            assert!(
                repo_root.join(qf).exists(),
                "{name}: query_file {qf} must exist"
            );
            assert!(
                dm.root_field.as_deref().map(str::len).unwrap_or(0) > 0,
                "{name}: root_field must be non-empty"
            );
            assert!(
                dm.join_normalizer.is_some(),
                "{name}: candidate endpoint must declare a join_normalizer"
            );
        }

        // journal_transactions: path bug fixed, but NOT activated (no normalizer, tier=1).
        let journal = reg
            .get("journal_transactions")
            .expect("journal_transactions endpoint");
        assert_eq!(
            journal.effective_source(&pilot),
            Source::ErpRest,
            "journal_transactions must stay on ERP-REST (missing normalizer + tier=1)"
        );
        let dm = journal.datamart.as_ref().expect("datamart binding");
        assert_eq!(
            dm.query_file.as_deref(),
            Some("config/queries/datamart/journal_transactions.graphql"),
            "journal_transactions query_file path bug must be fixed"
        );
    }

    // open_set -------------------------------------------------------------------------------------

    fn open_set_def() -> EndpointDefinition {
        crate::open_set::fixtures::def(
            Some("/v1/customerinvoice"),
            PrimaryKey::Single("referenceNumber".into()),
        )
    }

    #[test]
    fn an_endpoint_that_fits_has_no_open_set_problem() {
        assert_eq!(open_set_def().open_set_problem(), None);
    }

    #[test]
    fn a_comma_separated_key_of_top_level_fields_is_fine() {
        let mut def = open_set_def();
        def.primary_key = PrimaryKey::Single("documentType, referenceNumber".into());

        assert_eq!(def.open_set_problem(), None);
    }

    #[test]
    fn without_the_flag_nothing_is_checked() {
        let mut def = open_set_def();
        def.open_set = false;
        def.tier = Some(3);
        def.source = Source::DatamartGraphql;

        assert_eq!(def.open_set_problem(), None);
    }

    #[test]
    fn the_flag_defaults_to_false_in_a_config_that_does_not_mention_it() {
        let def: EndpointDefinition = serde_json::from_str(
            r#"{"endpoint_name": "accounts", "bigquery_table": "visma_accounts",
                "primary_key": "accountCD", "api_endpoint": "/v1/account"}"#,
        )
        .unwrap();

        assert!(!def.open_set);
    }

    #[test]
    fn an_endpoint_the_reconciliation_cannot_serve_is_refused_with_the_reason() {
        type Break = fn(&mut EndpointDefinition);
        let cases: Vec<(&str, Break)> = vec![
            ("source erp_rest", |d| d.source = Source::DatamartGraphql),
            ("api_endpoint", |d| d.api_endpoint = None),
            ("api_endpoint", |d| d.api_endpoint = Some("  ".into())),
            ("api_endpoint", |d| {
                d.api_endpoint = Some("/v1/x/{id}".into())
            }),
            ("api_endpoint", |d| {
                d.api_endpoint = Some("/v1/x?a=b".into())
            }),
            ("finance_page_number", |d| {
                d.pagination_profile = PaginationProfile::SalesOrderPageIndex
            }),
            ("tier 1", |d| d.tier = None),
            ("tier 1", |d| d.tier = Some(2)),
            ("not partitioned", |d| {
                d.period_scan = Some(PeriodScanConfig {
                    first_period: "202301".into(),
                    query_params: vec!["periodId".into()],
                    look_ahead_months: 0,
                    period_field: None,
                    resume: false,
                })
            }),
            ("primary key", |d| {
                d.primary_key = PrimaryKey::Composite(vec![])
            }),
            ("primary key", |d| {
                d.primary_key = PrimaryKey::Single("tenant_id".into())
            }),
            ("primary key", |d| {
                d.primary_key = PrimaryKey::Single("tenant_id, referenceNumber".into())
            }),
            ("primary key", |d| {
                d.primary_key = PrimaryKey::Single("customer.number".into())
            }),
            ("primary key", |d| {
                d.primary_key =
                    PrimaryKey::Composite(vec!["documentType".into(), "_partition_key".into()])
            }),
        ];
        for (reason, break_it) in cases {
            let mut def = open_set_def();
            break_it(&mut def);

            let problem = def.open_set_problem().unwrap_or_default();

            assert!(problem.contains(reason), "{reason:?} not in {problem:?}");
            assert!(problem.contains("'invoices'"), "{problem}");
        }
    }

    #[test]
    fn the_registry_refuses_to_load_a_flagged_endpoint_that_does_not_fit() {
        let dir = tempfile::tempdir().unwrap();
        let mut def = open_set_def();
        def.tier = Some(2);
        std::fs::write(
            dir.path().join("invoices.json"),
            serde_json::to_string(&def).unwrap(),
        )
        .unwrap();

        let error = EndpointRegistry::load_dir(dir.path())
            .unwrap_err()
            .to_string();

        assert!(error.contains("open_set needs tier 1"), "{error}");
    }

    #[test]
    fn the_registry_loads_a_flagged_endpoint_that_fits() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("invoices.json"),
            serde_json::to_string(&open_set_def()).unwrap(),
        )
        .unwrap();

        let registry = EndpointRegistry::load_dir(dir.path()).unwrap();

        assert!(registry.get("invoices").unwrap().open_set);
    }

    /// The flag changes what the hourly pass does for an endpoint, so which endpoints carry it is
    /// worth a test: the two invoice endpoints, and nothing else.
    #[test]
    fn only_the_invoice_endpoints_are_reconciled_against_visma() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("config/endpoints");
        let reg = EndpointRegistry::load_dir(&dir).expect("real endpoint configs must load");

        let mut flagged: Vec<&str> = reg
            .iter()
            .filter(|def| def.open_set)
            .map(|def| def.endpoint_name.as_str())
            .collect();
        flagged.sort_unstable();

        assert_eq!(flagged, ["customer_invoices", "supplier_invoices"]);
        for name in flagged {
            let def = reg.get(name).unwrap();
            assert_eq!(def.open_set_problem(), None, "{name}");
            assert_eq!(def.tier, Some(1), "{name}");
        }
    }

    /// The registry refuses a mis-flagged config when it loads, which would stop every process
    /// that reads `config/endpoints`. `vsv2-validate-config` has to refuse it first, so
    /// `open_set_problem` in `tooling/src/vsv2_tooling/config_validate.py` mirrors the rules
    /// above. The cases are in one JSON file, read here and by `test_open_set_config.py`: each
    /// starts from a real config, changes it, and says what both sides must answer, down to the
    /// words. A rule changed on one side only fails the test of the side that was left alone.
    #[test]
    fn the_registry_answers_what_the_cases_shared_with_the_python_validator_say() {
        use serde_json::Value;

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let read = |path: std::path::PathBuf| -> Value {
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
        };
        let shared = read(root.join("tooling/tests/fixtures/open_set_cases.json"));
        let base_name = shared["base"]
            .as_str()
            .expect("the cases name a base config");
        let base = read(root.join(format!("config/endpoints/{base_name}.json")));
        let cases = shared["cases"].as_array().expect("a list of cases");
        assert!(cases.len() >= 20, "the shared cases cover every rule");

        for case in cases {
            let name = case["name"].as_str().expect("a case has a name");
            let mut config = base.clone();
            let object = config.as_object_mut().expect("a config is an object");
            for (key, value) in case["set"].as_object().into_iter().flatten() {
                object.insert(key.clone(), value.clone());
            }
            for key in case["remove"].as_array().into_iter().flatten() {
                object.remove(key.as_str().expect("a key to remove"));
            }
            let expected = case["problem"]
                .as_str()
                .map(|reason| format!("endpoint '{base_name}': {reason}"));

            let def: EndpointDefinition = serde_json::from_value(config.clone())
                .unwrap_or_else(|e| panic!("{name}: the config does not deserialize: {e}"));

            assert_eq!(def.open_set_problem(), expected, "{name}");
            // And what loading the directory does with it.
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(
                dir.path().join(format!("{base_name}.json")),
                serde_json::to_string(&config).unwrap(),
            )
            .unwrap();
            let loaded = EndpointRegistry::load_dir(dir.path());
            match &expected {
                None => assert!(loaded.is_ok(), "{name}: {:?}", loaded.err()),
                Some(problem) => {
                    let error = loaded.err().map(|e| e.to_string()).unwrap_or_default();
                    assert!(error.contains(problem.as_str()), "{name}: {error}");
                }
            }
        }
    }
}
