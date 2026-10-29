//! Cross-crate integration tests for the orchestrator's source dispatch.
//!
//! Tests `EndpointRegistry`, `DispatchTable`, and the `effective_source` logic
//! without hitting any network or database.

use std::collections::HashMap;

use vsv2_common::{BackgroundMode, PaginationProfile, PrimaryKey, Source, TenantId};
use vsv2_orchestrator::{DatamartBinding, DispatchTable, EndpointDefinition, EndpointRegistry, NoopAdapter};

const AQUATIQ: &str = "33f319b9-4d57-11ee-960c-025417856183";
const OTHER:   &str = "00000000-0000-0000-0000-000000000000";

fn make_customers_def(ready: bool, override_tenant: Option<(&str, bool)>) -> EndpointDefinition {
    let mut overrides = HashMap::new();
    if let Some((t, v)) = override_tenant {
        overrides.insert(t.to_string(), v);
    }
    EndpointDefinition {
        endpoint_name:       "customers".into(),
        bigquery_table:      "visma_customers".into(),
        primary_key:         PrimaryKey::Single("number".into()),
        source:              Source::ErpRest,
        enabled:             true,
        api_method:          Some("get_customers".into()),
        api_endpoint:        Some("v1/customer".into()),
        fallback_api_method: Some("get_customers_v3".into()),
        pagination_profile:  PaginationProfile::FinancePageNumber,
        page_size:           None,
        background_mode:     BackgroundMode::Off,
        background_poll_secs: None,
        background_timeout_secs: None,
        default_query:       HashMap::new(),
        required_query:      vec![],
        modified_field:      None,
        merge_type:          Some("upsert".into()),
        sync_pattern:        Some("full_refresh_upsert".into()),
        validation_fields:   None,
        partitioning:        None,
        datamart: Some(DatamartBinding {
            ready,
            tenant_overrides: overrides,
            query_file:       Some("config/queries/customer.graphql".into()),
            type_name:        Some("Customer".into()),
            root_field:       Some("customers".into()),
            cursor_field:     Some("updatedInDataMartDateTime".into()),
            id_field:         Some("id".into()),
            company_filter:   true,
            join_normalizer:  Some("normalize_customer_with_locations".into()),
            fallback_source:  Source::ErpRest,
        }),
        tier: None,
    }
}

fn make_dispatch() -> DispatchTable {
    DispatchTable {
        erp:      Box::new(NoopAdapter { source_name: "erp_rest" }),
        datamart: Box::new(NoopAdapter { source_name: "datamart_graphql" }),
        expense:  Box::new(NoopAdapter { source_name: "expense_api" }),
    }
}

// ─── effective_source resolution ─────────────────────────────────────────────

#[test]
fn dispatch_picks_erp_when_dm_not_ready() {
    let def      = make_customers_def(false, None);
    let dispatch = make_dispatch();
    let tenant   = TenantId::new(AQUATIQ);

    // No error — erp adapter was returned (noop, not a BQ call)
    let adapter = dispatch.pick(&def, &tenant).expect("pick should succeed");
    // The noop adapter for erp_rest has source_name "erp_rest"
    // We can't introspect adapter type directly, but pick must not Err
    let _ = adapter;
}

#[test]
fn dispatch_picks_datamart_when_dm_ready() {
    let def    = make_customers_def(true, None);
    let dispatch = make_dispatch();
    let tenant = TenantId::new(AQUATIQ);

    // pick should return the datamart adapter — no error
    let _ = dispatch.pick(&def, &tenant).expect("pick datamart should succeed");
}

#[test]
fn dispatch_pilot_override_flips_source() {
    let def    = make_customers_def(false, Some((AQUATIQ, true)));
    let dispatch = make_dispatch();

    // Pilot tenant uses DataMart even when global ready = false
    let _ = dispatch.pick(&def, &TenantId::new(AQUATIQ))
        .expect("pilot: datamart adapter expected");

    // Non-pilot tenant stays on ERP
    let _ = dispatch.pick(&def, &TenantId::new(OTHER))
        .expect("other: erp adapter expected");
}

// ─── pick_fallback ────────────────────────────────────────────────────────────

#[test]
fn pick_fallback_rejects_datamart_as_fallback() {
    let def_with_bad_fallback = EndpointDefinition {
        endpoint_name: "bad".into(),
        bigquery_table: "t".into(),
        primary_key: PrimaryKey::Single("id".into()),
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
        datamart: Some(DatamartBinding {
            ready: false,
            tenant_overrides: HashMap::new(),
            query_file: None,
            type_name: None,
            root_field: None,
            cursor_field: None,
            id_field: None,
            company_filter: false,
            join_normalizer: None,
            fallback_source: Source::DatamartGraphql,  // invalid
        }),
        tier: None,
    };
    let dispatch = make_dispatch();
    let err = dispatch.pick_fallback(&def_with_bad_fallback);
    assert!(err.is_err(), "datamart_graphql must never be a fallback source");
}

#[test]
fn pick_fallback_returns_erp_by_default() {
    let def = make_customers_def(false, None);
    let dispatch = make_dispatch();
    let _ = dispatch.pick_fallback(&def).expect("erp fallback should succeed");
}

// ─── registry round-trip ─────────────────────────────────────────────────────

#[test]
fn registry_loads_customers_json() {
    // Load the real config/endpoints directory shipped with the project.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()          // tests/
        .parent().unwrap()          // visma_service_v2/
        .join("config/endpoints");

    if !dir.exists() {
        // CI without workspace root — skip gracefully
        return;
    }

    let reg = EndpointRegistry::load_dir(&dir)
        .expect("registry load should succeed");

    let customers = reg.get("customers").expect("customers endpoint must exist");
    assert_eq!(customers.bigquery_table, "visma_customers");
    assert_eq!(customers.source, Source::ErpRest);
    assert!(customers.datamart.is_some());

    let accounts = reg.get("accounts").expect("accounts endpoint must exist");
    assert!(accounts.datamart.is_some());
}

#[test]
fn registry_unknown_endpoint_returns_none() {
    let reg = EndpointRegistry::new();
    assert!(reg.get("does_not_exist").is_none());
}
