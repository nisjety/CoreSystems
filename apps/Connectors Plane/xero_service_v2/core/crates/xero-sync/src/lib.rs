//! `xero-sync` — stateless raw → GCS uploader orchestration.
//!
//! This crate fans out per-(tenant, entity) Xero fetches and writes the
//! verbatim API response bytes to a [`RawSink`] (GCS in production, local disk
//! for dry-runs). There is no persisted sync checkpoint or
//! watermark state: every run computes its own window from [`SyncMode`] and a
//! `run_date`, and re-runs simply append new objects (downstream dedups).
//!
//! Auth uses [`TokenProvider`] for Custom Connections or PKCE. The tenant
//! header is disabled for tenant-scoped Custom Connections and enabled for
//! PKCE. Optional shared quota coordination belongs to `xero-client`.
//!
//! Public surface:
//!   - [`SyncJob`] — the orchestrator (`new` + `run`).
//!   - [`SyncMode`] — the per-run window/filter selector.
//!
//! Pipeline (`run` → `run_entity`):
//!   1. obtain a current token for the tenant
//!   2. derive fetch params from the [`SyncMode`]
//!   3. pull one raw page (or a single report); refresh before each request
//!   4. build object key + metadata, await `sink.put_raw`, then fetch more
//!   5. record an [`EntityOutcome`]; per-entity failures are isolated.

pub mod accounting_get;
mod aged_reports;
mod attachments;
mod detail_stage;
pub mod extended;
mod mode;
mod outcome;
mod page_spool;
mod report;
mod streaming;

pub use aged_reports::AgedContactReportOptions;
pub use attachments::AttachmentDiscoveryOptions;
pub use mode::{
    business_date_where, compact_ts, fetch_params, incremental_modified_after, rolling_window,
    FetchParams, SyncMode,
};
pub use outcome::is_actionable_failure;
pub use report::{report_params, report_params_with_options, ReportOptions, ReportSpec};

use std::sync::Arc;

use chrono::{NaiveDate, Utc};
use futures::stream::{FuturesUnordered, StreamExt};
use tracing::{info, warn};
use uuid::Uuid;

use xero_auth::TokenProvider;
use xero_client::{ExtraQuery, XeroApiClient};
use xero_common::{EntityType, GcsConfig, SyncConfig};
use xero_gcs::{
    report_metadata, report_object_key, EntityOutcome, MetaArgs, RawSink, ReportMetaArgs,
    RunManifest,
};

use crate::outcome::{classify_source_failure, sanitized_error};

/// The maximum number of concurrent in-flight (tenant, entity) tasks. Xero
/// allows a wider cross-tenant fan-out; xero-client separately caps each
/// tenant at Xero's five concurrent requests.
const MAX_CONCURRENCY: usize = 6;
const DEFAULT_MAX_DETAIL_RECORDS: usize = 10_000;

/// Stateless orchestrator: fetch raw Xero pages and upload them to a sink.
pub struct SyncJob {
    auth: Arc<dyn TokenProvider>,
    sink: Arc<dyn RawSink>,
    gcs: GcsConfig,
    cfg: SyncConfig,
    /// `tenant_id` → org display name, used for the `x-org-name` object metadata
    /// tag. Tenants without a configured name fall back to an empty tag.
    org_names: std::collections::HashMap<String, String>,
    /// Send the `Xero-tenant-id` header. Required for multi-tenant PKCE tokens;
    /// omitted for single-tenant custom-connection tokens (Xero infers those).
    send_tenant_header: bool,
    report_options: ReportOptions,
    max_detail_records: Option<usize>,
    #[cfg(test)]
    test_api_base_url: Option<String>,
}

impl SyncJob {
    /// Build a job from its collaborators. No I/O — wiring only.
    pub fn new(
        auth: Arc<dyn TokenProvider>,
        sink: Arc<dyn RawSink>,
        gcs: GcsConfig,
        cfg: SyncConfig,
        org_names: std::collections::HashMap<String, String>,
        send_tenant_header: bool,
    ) -> Self {
        Self {
            auth,
            sink,
            gcs,
            cfg,
            org_names,
            send_tenant_header,
            report_options: ReportOptions::default(),
            max_detail_records: None,
            #[cfg(test)]
            test_api_base_url: None,
        }
    }

    pub fn with_report_options(mut self, options: ReportOptions) -> Self {
        self.report_options = options;
        self
    }

    pub fn with_detail_expansion(mut self, max_records: usize) -> Self {
        self.max_detail_records = Some(max_records);
        self
    }

    #[cfg(test)]
    fn with_test_api_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.test_api_base_url = Some(base_url.into());
        self
    }

    /// Run a sync across the cartesian product of `tenants` × `entities` in the
    /// given [`SyncMode`], bounded by `max_concurrent` (clamped to 1..=6).
    ///
    /// Each (tenant, entity) pair is isolated: a failure becomes an
    /// [`EntityOutcome`] with `error` set and `termination = "error"`, never
    /// aborting the rest of the run. Returns the assembled [`RunManifest`];
    /// the caller persists it via `xero_gcs::write_manifest`.
    pub async fn run(
        &self,
        run_id: Uuid,
        run_date: NaiveDate,
        tenants: &[String],
        entities: &[EntityType],
        mode: SyncMode,
        max_concurrent: usize,
    ) -> RunManifest {
        let started_at = Utc::now();
        // One compact timestamp for the whole run → all objects share a ts.
        let ts = compact_ts(started_at);
        let concurrency = max_concurrent.clamp(1, MAX_CONCURRENCY);

        info!(
            run_id = %run_id,
            run_date = %run_date,
            mode = mode.label(),
            tenants = tenants.len(),
            entities = entities.len(),
            concurrency,
            "sync run started"
        );

        // Build every (tenant, entity) unit of work up front.
        let units: Vec<(&str, &EntityType)> = tenants
            .iter()
            .flat_map(|t| entities.iter().map(move |e| (t.as_str(), e)))
            .collect();

        let mut outcomes: Vec<EntityOutcome> = Vec::with_capacity(units.len());
        let mut in_flight = FuturesUnordered::new();
        let mut next = 0usize;

        // Prime the pump up to `concurrency`, then refill on each completion.
        while next < units.len() && in_flight.len() < concurrency {
            let (tenant, entity) = units[next];
            in_flight.push(self.run_unit(run_id, run_date, &ts, tenant, entity, &mode));
            next += 1;
        }
        while let Some(unit_outcomes) = in_flight.next().await {
            outcomes.extend(unit_outcomes);
            if next < units.len() {
                let (tenant, entity) = units[next];
                in_flight.push(self.run_unit(run_id, run_date, &ts, tenant, entity, &mode));
                next += 1;
            }
        }

        let finished_at = Utc::now();
        info!(
            run_id = %run_id,
            entities = outcomes.len(),
            errors = outcomes.iter().filter(|o| o.error.is_some()).count(),
            "sync run finished"
        );

        RunManifest {
            run_id: run_id.to_string(),
            started_at: started_at.to_rfc3339(),
            finished_at: finished_at.to_rfc3339(),
            mode: mode.label().to_string(),
            window_days: mode.window_days(),
            entities: outcomes,
        }
    }

    /// Run a single (tenant, entity) unit, converting any error into an
    /// [`EntityOutcome`] so the surrounding fan-out never short-circuits.
    async fn run_unit(
        &self,
        run_id: Uuid,
        run_date: NaiveDate,
        ts: &str,
        tenant: &str,
        entity: &EntityType,
        mode: &SyncMode,
    ) -> Vec<EntityOutcome> {
        match self
            .run_entity(run_id, run_date, ts, tenant, entity, mode)
            .await
        {
            Ok(outcomes) => outcomes,
            Err(e) => {
                warn!(
                    run_id = %run_id,
                    tenant = %tenant,
                    entity = %entity.as_str(),
                    error = %e,
                    "entity sync failed (isolated)"
                );
                let error = e;
                let (outcome_class, http_status) = classify_source_failure(entity.as_str(), &error);
                vec![EntityOutcome {
                    tenant: tenant.to_string(),
                    endpoint: entity.as_str().to_string(),
                    pages: 0,
                    records: 0,
                    termination: "error".to_string(),
                    request_key: None,
                    error: Some(sanitized_error(&error)),
                    outcome_class: Some(outcome_class),
                    http_status,
                }]
            }
        }
    }

    /// Fetch + upload one (tenant, entity). Reports take the single-shot report
    /// path; everything else takes the paginated raw path.
    async fn run_entity(
        &self,
        run_id: Uuid,
        run_date: NaiveDate,
        ts: &str,
        tenant: &str,
        entity: &EntityType,
        mode: &SyncMode,
    ) -> std::result::Result<Vec<EntityOutcome>, String> {
        // Custom-connection token → API client WITHOUT the tenant header.
        let token = self
            .auth
            .get_valid_token(tenant)
            .await
            .map_err(|e| e.to_string())?;
        let api =
            XeroApiClient::new_with_tenant_header(tenant.to_string(), self.send_tenant_header);
        #[cfg(test)]
        let api = match self.test_api_base_url.as_deref() {
            Some(base_url) => api.with_test_base_url(base_url),
            None => api,
        };
        let synced_at = Utc::now().to_rfc3339();
        let run_id_str = run_id.to_string();
        let run_id_short = &run_id_str[..run_id_str.len().min(6)];
        let org_name = self
            .org_names
            .get(tenant)
            .map(String::as_str)
            .unwrap_or_default();

        if entity.is_report() {
            return self
                .run_report(
                    &api,
                    &token.access_token,
                    run_date,
                    ts,
                    run_id_short,
                    &run_id_str,
                    tenant,
                    org_name,
                    entity,
                    mode,
                    &synced_at,
                )
                .await
                .map(|outcome| vec![outcome]);
        }

        let params = fetch_params(mode, entity, Utc::now(), run_date, self.cfg.window_days);
        let mut source = streaming::AuthenticatedPages {
            reader: api.raw_page_reader(entity, params.modified_after, &params.extras),
            auth: self.auth.as_ref(),
            tenant,
        };
        let args = streaming::WriterArgs {
            prefix: &self.gcs.prefix,
            ts,
            run_date,
            run_id_short,
            meta: MetaArgs {
                tenant_id: tenant,
                org_name,
                endpoint: entity.as_str(),
                sync_type: mode.sync_type(),
                modified_after: params.modified_after_str.as_deref(),
                business_from: params.business_from.as_deref(),
                business_to: params.business_to.as_deref(),
                where_filter: params.where_filter.as_deref(),
                run_id: &run_id_str,
                synced_at: &synced_at,
                ..MetaArgs::default()
            },
        };
        let mut writer = streaming::PageWriter::new(self.sink.as_ref(), args);
        let outcomes = if let Some(max_records) = detail_limit(entity, self.max_detail_records) {
            let fetcher = detail_stage::AuthenticatedDetails {
                api: &api,
                auth: self.auth.as_ref(),
                tenant,
                entity,
            };
            detail_stage::expanded_pages(&mut source, &fetcher, entity, max_records, &mut writer)
                .await
        } else {
            vec![streaming::write_pages(&mut source, &mut writer).await]
        };
        Ok(outcomes)
    }

    /// Single-shot report fetch + upload.
    #[allow(clippy::too_many_arguments)]
    async fn run_report(
        &self,
        api: &XeroApiClient,
        access_token: &str,
        run_date: NaiveDate,
        ts: &str,
        run_id_short: &str,
        run_id: &str,
        tenant: &str,
        org_name: &str,
        entity: &EntityType,
        mode: &SyncMode,
        synced_at: &str,
    ) -> std::result::Result<EntityOutcome, String> {
        // Reports only make sense in Reports mode; default to today otherwise.
        let as_of = match mode {
            SyncMode::Reports { as_of } => *as_of,
            _ => run_date,
        };
        let spec = report_params_with_options(entity, as_of, &self.report_options)?;
        let extras = ExtraQuery {
            extra: spec.extra.clone(),
            ..ExtraQuery::default()
        };
        let page = api
            .fetch_report_raw(access_token, entity, &extras)
            .await
            .map_err(|e| e.to_string())?;

        let key = report_object_key(
            &self.gcs.prefix,
            tenant,
            entity.as_str(),
            run_date,
            ts,
            run_id_short,
            &spec.period_key,
        );
        let signature = spec.params_signature();
        let meta = report_metadata(&ReportMetaArgs {
            tenant_id: tenant,
            org_name,
            endpoint: entity.as_str(),
            report: entity.as_str(),
            report_date: spec.report_date.as_deref(),
            report_from: spec.report_from.as_deref(),
            report_to: spec.report_to.as_deref(),
            report_params: &signature,
            contact_id: None,
            http_status: page.http_status,
            record_count: page.record_count as i64,
            run_id,
            synced_at,
        });
        self.sink
            .put_raw(&key, &page.body, &meta)
            .await
            .map_err(|e| e.to_string())?;

        Ok(EntityOutcome {
            tenant: tenant.to_string(),
            endpoint: entity.as_str().to_string(),
            pages: 1,
            records: page.record_count as i64,
            termination: "report-snapshot".to_string(),
            request_key: None,
            error: None,
            outcome_class: None,
            http_status: None,
        })
    }
}

fn detail_limit(entity: &EntityType, configured: Option<usize>) -> Option<usize> {
    if matches!(entity, EntityType::Budgets | EntityType::ContactGroups) {
        Some(configured.unwrap_or(DEFAULT_MAX_DETAIL_RECORDS))
    } else if entity.has_uuid_detail() {
        configured
    } else {
        None
    }
}

fn validate_detail_count(
    entity: &EntityType,
    count: usize,
    max_records: usize,
) -> std::result::Result<(), String> {
    if count > max_records {
        Err(format!(
            "{entity} detail expansion found {count} records, exceeding limit {max_records}"
        ))
    } else {
        Ok(())
    }
}

fn detail_ids(
    entity: &EntityType,
    pages: &[xero_client::RawPage],
) -> std::result::Result<Vec<String>, String> {
    let mut ids = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for page in pages.iter().filter(|page| page.http_status != 304) {
        let body: serde_json::Value = serde_json::from_slice(&page.body)
            .map_err(|e| format!("invalid {} list response: {e}", entity.xero_path()))?;
        let records = body
            .get(entity.xero_path())
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| format!("{} list response has no records array", entity.xero_path()))?;
        for record in records {
            let id = record
                .get(entity.id_field())
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    format!(
                        "{} list record missing {}",
                        entity.xero_path(),
                        entity.id_field()
                    )
                })?;
            if seen.insert(id.to_string()) {
                ids.push(id.to_string());
            }
        }
    }
    Ok(ids)
}

#[cfg(test)]
#[path = "sync_job_tests.rs"]
mod sync_job_tests;

#[cfg(test)]
#[path = "extended_run_tests.rs"]
mod extended_run_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detail_ids_are_unique_and_require_an_id() {
        let page = xero_client::RawPage {
            page: 1,
            body: br#"{"Budgets":[{"BudgetID":"a"},{"BudgetID":"a"},{"BudgetID":"b"}]}"#
                .as_slice()
                .into(),
            http_status: 200,
            record_count: 3,
            fetched_at: Utc::now(),
        };
        assert_eq!(
            detail_ids(&EntityType::Budgets, std::slice::from_ref(&page)).unwrap(),
            vec!["a", "b"]
        );
        let missing = xero_client::RawPage {
            body: br#"{"Budgets":[{"Description":"No ID"}]}"#.as_slice().into(),
            ..page
        };
        assert!(detail_ids(&EntityType::Budgets, &[missing]).is_err());
        let empty = xero_client::RawPage {
            page: 1,
            body: br#"{"Budgets":[]}"#.as_slice().into(),
            http_status: 200,
            record_count: 0,
            fetched_at: Utc::now(),
        };
        assert!(detail_ids(&EntityType::Budgets, &[empty])
            .unwrap()
            .is_empty());
    }

    #[test]
    fn detail_expansion_is_opt_in_and_bounded_for_other_entities() {
        assert!(detail_limit(&EntityType::Budgets, None).is_some());
        assert!(detail_limit(&EntityType::ContactGroups, None).is_some());
        assert_eq!(detail_limit(&EntityType::Accounts, None), None);
        assert_eq!(detail_limit(&EntityType::Accounts, Some(500)), Some(500));
        assert_eq!(detail_limit(&EntityType::Currencies, Some(500)), None);
        assert_eq!(
            detail_limit(&EntityType::ReportBalanceSheet, Some(500)),
            None
        );
        assert!(validate_detail_count(&EntityType::Accounts, 500, 500).is_ok());
        assert!(validate_detail_count(&EntityType::Accounts, 501, 500).is_err());
    }

    #[test]
    fn run_id_short_is_first_six_chars() {
        let run_id = Uuid::parse_str("5f3c9a2e-0000-0000-0000-000000000000").unwrap();
        let s = run_id.to_string();
        let short = &s[..s.len().min(6)];
        assert_eq!(short, "5f3c9a");
    }

    #[test]
    fn max_concurrency_clamp_bounds() {
        assert_eq!(0usize.clamp(1, MAX_CONCURRENCY), 1);
        assert_eq!(100usize.clamp(1, MAX_CONCURRENCY), 6);
        assert_eq!(4usize.clamp(1, MAX_CONCURRENCY), 4);
    }
}
