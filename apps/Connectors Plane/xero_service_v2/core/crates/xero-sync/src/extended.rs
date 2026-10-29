//! Opt-in raw ingestion of Xero Assets, Projects, Bank Feeds, Identity and Finance.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::{NaiveDate, Utc};
use serde_json::json;
use serde_json::Value;
use uuid::Uuid;
use xero_auth::TokenProvider;
pub use xero_client::extended::ExtendedFamily;
use xero_client::extended::{ExtendedRequest, ResponseShape};
use xero_client::{RawPage, XeroApiClient};
use xero_common::GcsConfig;
use xero_gcs::{
    extended_object_key, metadata, EntityOutcome, ExtendedKeyArgs, MetaArgs, RawSink, RunManifest,
    SyncType,
};

use crate::outcome::{classify_source_failure, sanitized_error};

use crate::compact_ts;
use crate::streaming::{AuthenticatedPages, PageSource};

mod finance;

const MAX_COLLECTION_IDS: usize = 100_000;

#[derive(Debug, Clone)]
pub struct ExtendedOptions {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub bank_account_ids: Vec<Uuid>,
    pub details: bool,
}

impl ExtendedOptions {
    pub fn validate(&self, families: &[ExtendedFamily]) -> Result<(), String> {
        if families.is_empty() {
            return Err("select at least one --family".into());
        }
        let has_sensitive = families.iter().any(|family| {
            matches!(
                family,
                ExtendedFamily::BankFeeds | ExtendedFamily::Finance | ExtendedFamily::Identity
            )
        });
        let has_ordinary = families
            .iter()
            .any(|family| matches!(family, ExtendedFamily::Assets | ExtendedFamily::Projects));
        if has_sensitive && has_ordinary {
            return Err("run Assets/Projects separately from Bank Feeds/Finance/Identity".into());
        }
        if let (Some(from), Some(to)) = (self.from, self.to) {
            if from > to {
                return Err(format!("--from {from} is after --to {to}"));
            }
        }
        if families.contains(&ExtendedFamily::Finance) && (self.from.is_none() || self.to.is_none())
        {
            return Err("Finance requires both --from and --to for reproducible snapshots".into());
        }
        if !self.bank_account_ids.is_empty() && !families.contains(&ExtendedFamily::Finance) {
            return Err("--bank-account-id requires --family finance".into());
        }
        if !families.contains(&ExtendedFamily::Finance)
            && (self.from.is_some() || self.to.is_some())
        {
            return Err("--from and --to require --family finance".into());
        }
        Ok(())
    }
}

pub struct ExtendedSyncJob {
    auth: Arc<dyn TokenProvider>,
    sink: Arc<dyn RawSink>,
    gcs: GcsConfig,
    org_names: HashMap<String, String>,
    send_tenant_header: bool,
    #[cfg(test)]
    test_api_base_url: Option<String>,
}

impl ExtendedSyncJob {
    pub fn new(
        auth: Arc<dyn TokenProvider>,
        sink: Arc<dyn RawSink>,
        gcs: GcsConfig,
        org_names: HashMap<String, String>,
        send_tenant_header: bool,
    ) -> Self {
        Self {
            auth,
            sink,
            gcs,
            org_names,
            send_tenant_header,
            #[cfg(test)]
            test_api_base_url: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_test_api_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.test_api_base_url = Some(base_url.into());
        self
    }

    pub async fn run(
        &self,
        run_id: Uuid,
        run_date: NaiveDate,
        tenants: &[String],
        families: &[ExtendedFamily],
        options: &ExtendedOptions,
    ) -> RunManifest {
        let started = Utc::now();
        let ts = compact_ts(started);
        let mut outcomes = Vec::new();
        for family in families {
            // /connections belongs to the OAuth app, not a particular Xero
            // tenant. One request avoids duplicating all connections N times.
            let selected: &[String] =
                if *family == ExtendedFamily::Identity && self.send_tenant_header {
                    tenants.get(..1).unwrap_or(&[])
                } else {
                    tenants
                };
            for tenant in selected {
                let storage_tenant =
                    if *family == ExtendedFamily::Identity && self.send_tenant_header {
                        "_application"
                    } else {
                        tenant.as_str()
                    };
                let api = XeroApiClient::new_with_tenant_header(tenant, self.send_tenant_header);
                let mut ctx = RunContext {
                    auth: self.auth.as_ref(),
                    sink: self.sink.as_ref(),
                    gcs: &self.gcs,
                    api,
                    tenant,
                    storage_tenant,
                    org_name: self
                        .org_names
                        .get(tenant)
                        .map(String::as_str)
                        .unwrap_or_default(),
                    run_id,
                    run_date,
                    ts: &ts,
                    page_counters: HashMap::new(),
                    record_counters: HashMap::new(),
                    #[cfg(test)]
                    test_api_base_url: self.test_api_base_url.as_deref(),
                };
                run_family(&mut ctx, *family, options, &mut outcomes).await;
            }
        }
        RunManifest {
            run_id: run_id.to_string(),
            started_at: started.to_rfc3339(),
            finished_at: Utc::now().to_rfc3339(),
            mode: "extended".into(),
            window_days: options
                .from
                .zip(options.to)
                .map(|(a, b)| (b - a).num_days() + 1),
            entities: outcomes,
        }
    }
}

struct RunContext<'a> {
    auth: &'a dyn TokenProvider,
    sink: &'a dyn RawSink,
    gcs: &'a GcsConfig,
    api: XeroApiClient,
    tenant: &'a str,
    storage_tenant: &'a str,
    org_name: &'a str,
    run_id: Uuid,
    run_date: NaiveDate,
    ts: &'a str,
    page_counters: HashMap<&'static str, u32>,
    record_counters: HashMap<&'static str, i64>,
    #[cfg(test)]
    test_api_base_url: Option<&'a str>,
}

impl RunContext<'_> {
    async fn execute_pages(
        &mut self,
        request: &ExtendedRequest,
        id_field: Option<&str>,
        snapshot_date: Option<NaiveDate>,
        source: &mut dyn PageSource,
    ) -> Result<(EntityOutcome, Vec<String>), String> {
        let mut ids = Vec::new();
        let mut seen = HashSet::new();
        let mut pages_written = 0;
        let run_id = self.run_id.to_string();
        let run_id_short = &run_id[..6];
        let synced_at = Utc::now().to_rfc3339();
        let mut total = 0i64;
        while let Some(page) = source.next().await? {
            for id in collect_ids(std::slice::from_ref(&page), request.shape, id_field)? {
                if !seen.insert(id.clone()) {
                    return Err(format!("duplicate IDs across {} pages", request.endpoint));
                }
                if ids.len() >= MAX_COLLECTION_IDS {
                    return Err("Extended collection ID limit exceeded; no complete snapshot will be written".into());
                }
                ids.push(id);
            }
            let next = self
                .page_counters
                .get(request.endpoint)
                .copied()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or("Extended page counter exhausted")?;
            let key = extended_object_key(&ExtendedKeyArgs {
                prefix: &self.gcs.prefix,
                tenant: self.storage_tenant,
                api_version: request.family.api_version(),
                endpoint: request.endpoint,
                run_date: self.run_date,
                ts: self.ts,
                run_id_short,
                page: next,
            });
            let mut meta = metadata(&MetaArgs {
                tenant_id: self.storage_tenant,
                org_name: self.org_name,
                endpoint: request.endpoint,
                sync_type: SyncType::Master,
                page: next,
                record_count: page.record_count as i64,
                http_status: page.http_status,
                run_id: &run_id,
                synced_at: &synced_at,
                ..MetaArgs::default()
            });
            meta.0
                .insert("x-api-family".into(), request.family.as_str().into());
            meta.0
                .insert("x-api-version".into(), request.family.api_version().into());
            if let Some(key) = &request.request_key {
                meta.0.insert("x-request-key".into(), key.clone());
            }
            if let Some(date) = snapshot_date {
                meta.0.insert("x-snapshot-date".into(), date.to_string());
                meta.0
                    .insert("x-sync-type".into(), "report-snapshot".into());
            }
            self.sink
                .put_raw(&key, &page.body, &meta)
                .await
                .map_err(|e| e.to_string())?;
            self.page_counters.insert(request.endpoint, next);
            total += page.record_count as i64;
            let previous = self
                .record_counters
                .get(request.endpoint)
                .copied()
                .unwrap_or(0);
            self.record_counters
                .insert(request.endpoint, previous + page.record_count as i64);
            pages_written += 1;
        }
        let pagination = source.outcome();
        if pagination.termination.is_suspect() {
            return Err(format!(
                "{} collection stopped at the page cap",
                request.endpoint
            ));
        }
        if let Some(snapshot_endpoint) = collection_snapshot_endpoint(request.endpoint) {
            self.write_collection_snapshot(request, snapshot_endpoint, &ids, &run_id, &synced_at)
                .await?;
        }
        Ok((
            EntityOutcome {
                tenant: self.storage_tenant.into(),
                endpoint: request.endpoint.into(),
                pages: pages_written,
                records: total,
                termination: pagination.termination.as_str().into(),
                request_key: request.request_key.clone(),
                error: None,
                outcome_class: None,
                http_status: None,
            },
            ids,
        ))
    }

    async fn write_collection_snapshot(
        &mut self,
        request: &ExtendedRequest,
        snapshot_endpoint: &'static str,
        ids: &[String],
        run_id: &str,
        synced_at: &str,
    ) -> Result<(), String> {
        let page = self
            .page_counters
            .get(snapshot_endpoint)
            .copied()
            .unwrap_or(0)
            + 1;
        let key = extended_object_key(&ExtendedKeyArgs {
            prefix: &self.gcs.prefix,
            tenant: self.storage_tenant,
            api_version: request.family.api_version(),
            endpoint: snapshot_endpoint,
            run_date: self.run_date,
            ts: self.ts,
            run_id_short: &run_id[..6],
            page,
        });
        let request_key = request.request_key.as_deref().unwrap_or("");
        let body = serde_json::to_vec(&json!({
            "source_endpoint": request.endpoint,
            "request_key": request_key,
            "record_ids": ids,
            "record_count": ids.len(),
        }))
        .map_err(|e| e.to_string())?;
        let mut meta = metadata(&MetaArgs {
            tenant_id: self.storage_tenant,
            org_name: self.org_name,
            endpoint: snapshot_endpoint,
            sync_type: SyncType::Master,
            page,
            record_count: ids.len() as i64,
            http_status: 200,
            run_id,
            synced_at,
            ..MetaArgs::default()
        });
        meta.0
            .insert("x-api-family".into(), request.family.as_str().into());
        meta.0
            .insert("x-api-version".into(), request.family.api_version().into());
        meta.0
            .insert("x-source-endpoint".into(), request.endpoint.into());
        meta.0.insert("x-request-key".into(), request_key.into());
        self.sink
            .put_raw(&key, &body, &meta)
            .await
            .map_err(|e| e.to_string())?;
        self.page_counters.insert(snapshot_endpoint, page);
        Ok(())
    }

    async fn record(
        &mut self,
        request: ExtendedRequest,
        id_field: Option<&str>,
        snapshot_date: Option<NaiveDate>,
        outcomes: &mut Vec<EntityOutcome>,
    ) -> Vec<String> {
        #[cfg(test)]
        let request = match self.test_api_base_url {
            Some(base_url) => request.with_test_base_url(base_url),
            None => request,
        };
        let api = self.api.clone();
        let mut source = AuthenticatedPages {
            reader: api.extended_page_reader(&request),
            auth: self.auth,
            tenant: self.tenant,
        };
        self.record_pages(&request, id_field, snapshot_date, &mut source, outcomes)
            .await
    }

    async fn record_pages(
        &mut self,
        request: &ExtendedRequest,
        id_field: Option<&str>,
        snapshot_date: Option<NaiveDate>,
        source: &mut dyn PageSource,
        outcomes: &mut Vec<EntityOutcome>,
    ) -> Vec<String> {
        let endpoint = request.endpoint;
        let before_pages = self.page_counters.get(endpoint).copied().unwrap_or(0);
        let before_records = self.record_counters.get(endpoint).copied().unwrap_or(0);
        match self
            .execute_pages(request, id_field, snapshot_date, source)
            .await
        {
            Ok((outcome, ids)) => {
                outcomes.push(outcome);
                ids
            }
            Err(error) => {
                tracing::warn!(tenant = self.tenant, endpoint, %error, "extended sync failed");
                let (outcome_class, http_status) = classify_source_failure(endpoint, &error);
                outcomes.push(EntityOutcome {
                    tenant: self.storage_tenant.into(),
                    endpoint: endpoint.into(),
                    pages: self
                        .page_counters
                        .get(endpoint)
                        .copied()
                        .unwrap_or(0)
                        .saturating_sub(before_pages),
                    records: self
                        .record_counters
                        .get(endpoint)
                        .copied()
                        .unwrap_or(0)
                        .saturating_sub(before_records),
                    termination: "error".into(),
                    request_key: request.request_key.clone(),
                    error: Some(sanitized_error(&error)),
                    outcome_class: Some(outcome_class),
                    http_status,
                });
                Vec::new()
            }
        }
    }
}

fn collect_ids(
    pages: &[RawPage],
    shape: ResponseShape,
    id_field: Option<&str>,
) -> Result<Vec<String>, String> {
    let Some(field) = id_field else {
        return Ok(Vec::new());
    };
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    for page in pages {
        let body: Value = serde_json::from_slice(&page.body).map_err(|e| e.to_string())?;
        let records: Vec<&Value> = match shape {
            ResponseShape::Items => body
                .get("items")
                .and_then(Value::as_array)
                .ok_or("missing items array")?
                .iter()
                .collect(),
            ResponseShape::Array => body.as_array().ok_or("expected array")?.iter().collect(),
            ResponseShape::Object => vec![&body],
        };
        for record in records {
            let id = record
                .get(field)
                .and_then(Value::as_str)
                .ok_or_else(|| format!("record missing {field}"))?;
            if !seen.insert(id.to_owned()) {
                return Err(format!("duplicate {field} across {} pages", page.page));
            }
            ids.push(id.to_owned());
        }
    }
    Ok(ids)
}

fn collection_snapshot_endpoint(endpoint: &str) -> Option<&'static str> {
    match endpoint {
        "assets_assets" | "assets_asset_types" => Some("assets_list_snapshots"),
        "projects_projects" | "projects_users" | "projects_tasks" | "projects_time" => {
            Some("projects_list_snapshots")
        }
        "bankfeeds_feed_connections" | "bankfeeds_statements" => Some("bankfeeds_list_snapshots"),
        "identity_connections" => Some("identity_list_snapshots"),
        _ => None,
    }
}

fn request(
    family: ExtendedFamily,
    endpoint: &'static str,
    path: &str,
    shape: ResponseShape,
    paginated: bool,
) -> ExtendedRequest {
    ExtendedRequest::new(family, endpoint, path, shape, paginated).expect("catalog paths are valid")
}

fn uuid_path(prefix: &str, id: &str, suffix: &str) -> Result<String, String> {
    let id = Uuid::parse_str(id).map_err(|e| format!("invalid Xero resource ID: {e}"))?;
    Ok(format!("{prefix}/{id}{suffix}"))
}

async fn run_family(
    ctx: &mut RunContext<'_>,
    family: ExtendedFamily,
    options: &ExtendedOptions,
    outcomes: &mut Vec<EntityOutcome>,
) {
    match family {
        ExtendedFamily::Assets => {
            let mut ids = HashSet::new();
            for status in ["DRAFT", "REGISTERED", "DISPOSED"] {
                let mut req = request(
                    family,
                    "assets_assets",
                    "/Assets",
                    ResponseShape::Items,
                    true,
                );
                req.query.push(("status".into(), status.into()));
                req.request_key = Some(status.into());
                ids.extend(ctx.record(req, Some("assetId"), None, outcomes).await);
            }
            ctx.record(
                request(
                    family,
                    "assets_asset_types",
                    "/AssetTypes",
                    ResponseShape::Array,
                    false,
                ),
                Some("assetTypeId"),
                None,
                outcomes,
            )
            .await;
            ctx.record(
                request(
                    family,
                    "assets_settings",
                    "/Settings",
                    ResponseShape::Object,
                    false,
                ),
                None,
                None,
                outcomes,
            )
            .await;
            if options.details {
                for id in ids {
                    match uuid_path("/Assets", &id, "") {
                        Ok(path) => {
                            ctx.record(
                                request(
                                    family,
                                    "assets_asset_details",
                                    &path,
                                    ResponseShape::Object,
                                    false,
                                ),
                                None,
                                None,
                                outcomes,
                            )
                            .await;
                        }
                        Err(error) => {
                            push_error(outcomes, ctx.storage_tenant, "assets_asset_details", error)
                        }
                    }
                }
            }
        }
        ExtendedFamily::BankFeeds => {
            let connections = ctx
                .record(
                    request(
                        family,
                        "bankfeeds_feed_connections",
                        "/FeedConnections",
                        ResponseShape::Items,
                        true,
                    ),
                    Some("id"),
                    None,
                    outcomes,
                )
                .await;
            let statements = ctx
                .record(
                    request(
                        family,
                        "bankfeeds_statements",
                        "/Statements",
                        ResponseShape::Items,
                        true,
                    ),
                    Some("id"),
                    None,
                    outcomes,
                )
                .await;
            if options.details {
                for id in connections {
                    match uuid_path("/FeedConnections", &id, "") {
                        Ok(path) => {
                            ctx.record(
                                request(
                                    family,
                                    "bankfeeds_feed_connection_details",
                                    &path,
                                    ResponseShape::Object,
                                    false,
                                ),
                                None,
                                None,
                                outcomes,
                            )
                            .await;
                        }
                        Err(error) => push_error(
                            outcomes,
                            ctx.storage_tenant,
                            "bankfeeds_feed_connection_details",
                            error,
                        ),
                    }
                }
                for id in statements {
                    match uuid_path("/Statements", &id, "") {
                        Ok(path) => {
                            ctx.record(
                                request(
                                    family,
                                    "bankfeeds_statement_details",
                                    &path,
                                    ResponseShape::Object,
                                    false,
                                ),
                                None,
                                None,
                                outcomes,
                            )
                            .await;
                        }
                        Err(error) => push_error(
                            outcomes,
                            ctx.storage_tenant,
                            "bankfeeds_statement_details",
                            error,
                        ),
                    }
                }
            }
        }
        ExtendedFamily::Projects => {
            let projects = ctx
                .record(
                    request(
                        family,
                        "projects_projects",
                        "/Projects",
                        ResponseShape::Items,
                        true,
                    ),
                    Some("projectId"),
                    None,
                    outcomes,
                )
                .await;
            ctx.record(
                request(
                    family,
                    "projects_users",
                    "/ProjectsUsers",
                    ResponseShape::Items,
                    true,
                ),
                Some("userId"),
                None,
                outcomes,
            )
            .await;
            for id in projects {
                let Ok(path) = uuid_path("/Projects", &id, "") else {
                    push_error(
                        outcomes,
                        ctx.storage_tenant,
                        "projects_project_details",
                        format!("invalid project ID: {id}"),
                    );
                    continue;
                };
                if options.details {
                    ctx.record(
                        request(
                            family,
                            "projects_project_details",
                            &path,
                            ResponseShape::Object,
                            false,
                        ),
                        None,
                        None,
                        outcomes,
                    )
                    .await;
                }
                let mut tasks_request = request(
                    family,
                    "projects_tasks",
                    &format!("{path}/Tasks"),
                    ResponseShape::Items,
                    true,
                );
                tasks_request.request_key = Some(id.clone());
                let tasks = ctx
                    .record(tasks_request, Some("taskId"), None, outcomes)
                    .await;
                let mut time_request = request(
                    family,
                    "projects_time",
                    &format!("{path}/Time"),
                    ResponseShape::Items,
                    true,
                );
                time_request.request_key = Some(id.clone());
                let time = ctx
                    .record(time_request, Some("timeEntryId"), None, outcomes)
                    .await;
                if options.details {
                    for task in tasks {
                        match uuid_path(&format!("{path}/Tasks"), &task, "") {
                            Ok(p) => {
                                let mut detail = request(
                                    family,
                                    "projects_task_details",
                                    &p,
                                    ResponseShape::Object,
                                    false,
                                );
                                detail.request_key = Some(id.clone());
                                ctx.record(detail, None, None, outcomes).await;
                            }
                            Err(error) => push_error(
                                outcomes,
                                ctx.storage_tenant,
                                "projects_task_details",
                                error,
                            ),
                        }
                    }
                    for entry in time {
                        match uuid_path(&format!("{path}/Time"), &entry, "") {
                            Ok(p) => {
                                let mut detail = request(
                                    family,
                                    "projects_time_details",
                                    &p,
                                    ResponseShape::Object,
                                    false,
                                );
                                detail.request_key = Some(id.clone());
                                ctx.record(detail, None, None, outcomes).await;
                            }
                            Err(error) => push_error(
                                outcomes,
                                ctx.storage_tenant,
                                "projects_time_details",
                                error,
                            ),
                        }
                    }
                }
            }
        }
        ExtendedFamily::Identity => {
            ctx.record(
                request(
                    family,
                    "identity_connections",
                    "/connections",
                    ResponseShape::Array,
                    false,
                ),
                Some("id"),
                None,
                outcomes,
            )
            .await;
        }
        ExtendedFamily::Finance => finance::run_finance(ctx, options, outcomes).await,
    }
}

fn push_error(outcomes: &mut Vec<EntityOutcome>, tenant: &str, endpoint: &str, error: String) {
    let (outcome_class, http_status) = classify_source_failure(endpoint, &error);
    outcomes.push(EntityOutcome {
        tenant: tenant.into(),
        endpoint: endpoint.into(),
        pages: 0,
        records: 0,
        termination: "error".into(),
        request_key: None,
        error: Some(sanitized_error(&error)),
        outcome_class: Some(outcome_class),
        http_status,
    });
}

#[cfg(test)]
#[path = "extended_streaming_tests.rs"]
mod streaming_tests;

#[cfg(test)]
#[path = "extended_tests.rs"]
mod tests;
