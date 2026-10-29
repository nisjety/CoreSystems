//! A reconcilable endpoint definition for the tests of this module.

use std::collections::HashMap;

use vsv2_common::{BackgroundMode, PaginationProfile, PrimaryKey, Source};

use crate::registry::EndpointDefinition;

/// A reconcilable endpoint (`open_set`, tier 1, ERP REST) with the given path and key.
pub fn def(path: Option<&str>, primary_key: PrimaryKey) -> EndpointDefinition {
    EndpointDefinition {
        endpoint_name: "invoices".into(),
        bigquery_table: "visma_invoices".into(),
        primary_key,
        source: Source::ErpRest,
        enabled: true,
        api_method: None,
        api_endpoint: path.map(str::to_string),
        fallback_api_method: None,
        pagination_profile: PaginationProfile::FinancePageNumber,
        page_size: None,
        background_mode: BackgroundMode::Off,
        background_poll_secs: None,
        background_timeout_secs: None,
        default_query: HashMap::new(),
        required_query: vec![],
        modified_field: Some("lastModifiedDateTime".into()),
        merge_type: None,
        sync_pattern: None,
        validation_fields: None,
        partitioning: None,
        period_scan: None,
        datamart: None,
        tier: Some(1),
        open_set: true,
    }
}
