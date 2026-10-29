use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, Duration, NaiveDate, Timelike, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{error, info, warn};
use uuid::Uuid;
use vsv2_common::{Error, PageFetchMetric, PageSink, PrimaryKey, Result, Source, TenantId};
use vsv2_state::{
    endpoint_flags, Checkpoint, CheckpointKey, RunStatus, StateStore, SyncPageMetric, SyncRun,
};

use crate::adapter::FetchWindow;
use crate::bq_writer::{BqWriter, MergeLineage};
use crate::dispatch::DispatchTable;
use crate::registry::{EndpointDefinition, EndpointRegistry};
use crate::spool::Spool;
use crate::validation::{RecordChecker, ValidationIssue, Verdict};

pub struct SyncExecutor {
    pub registry: EndpointRegistry,
    pub dispatch: DispatchTable,
    pub state: StateStore,
    /// None → dry-run (BQ write skipped). Set when BQ project + dataset configured.
    pub bq: Option<BqWriter>,
    /// Enable local Postgres bronze sink when BigQuery is disabled.
    pub local_bronze_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointSyncFailure {
    pub endpoint: String,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncExecutionSummary {
    pub records_synced: u64,
    pub attempted_endpoints: usize,
    pub failed_endpoints: Vec<EndpointSyncFailure>,
}

impl SyncExecutionSummary {
    pub fn succeeded_endpoints(&self) -> usize {
        debug_assert!(
            self.failed_endpoints.len() <= self.attempted_endpoints,
            "failed_endpoints exceeds attempted_endpoints"
        );
        self.attempted_endpoints
            .saturating_sub(self.failed_endpoints.len())
    }
}

/// How much of an endpoint a fetch asks for.
#[derive(Debug, Clone, Copy)]
struct FetchScope {
    date_from: Option<DateTime<Utc>>,
    date_to: Option<DateTime<Utc>>,
    /// Ignore checkpoints: fetch everything Visma has.
    full: bool,
}

/// What a partition's fetch left behind: where it started, how many records it delivered, and the
/// newest record it kept, which becomes the partition's watermark.
struct FetchedPartition {
    key: CheckpointKey,
    prior: Option<Checkpoint>,
    /// Records the source delivered for the partition, before the window and the checks.
    records_in: i64,
    /// The newest modification time among the partition's kept records.
    newest: Option<DateTime<Utc>>,
    page_metrics: Vec<PageFetchMetric>,
    /// No watermark or cursor narrowed the fetch of this partition.
    full_scope: bool,
}

/// Whether a fetch holds everything Visma has for the tenant and endpoint, so that records absent
/// from it can be treated as deleted.
///
/// Every partition must have been fetched without a watermark or cursor, nothing may have been
/// cut off by a window, and there must be a partition at all: an empty partition list is
/// "nothing to ask for", which cannot tell a deletion from a failed listing.
fn is_complete_scan(
    date_from: Option<DateTime<Utc>>,
    date_to: Option<DateTime<Utc>>,
    partitions_full_scope: impl Iterator<Item = bool>,
) -> bool {
    if date_from.is_some() || date_to.is_some() {
        return false;
    }
    let mut any = false;
    for full in partitions_full_scope {
        if !full {
            return false;
        }
        any = true;
    }
    any
}

struct PartitionCheckpointState {
    key: CheckpointKey,
    prior: Option<Checkpoint>,
    records_in: i64,
    newest: Option<DateTime<Utc>>,
}

/// Everything one run writes, held as the run goes. Kept records go to a spool file and rejected
/// ones to another, so the run keeps counts and one example rejection, not the records themselves.
struct RunWrite {
    run_id: Uuid,
    tenant: TenantId,
    endpoint: String,
    source: Source,
    now: DateTime<Utc>,
    lineage: Lineage,
    checker: RecordChecker,
    /// The kept records, one JSON line each.
    rows: Spool,
    /// The rejected records, as quarantine rows.
    quarantine: Spool,
    /// Records past the window filter, so far: the index a rejection reports.
    seen: usize,
    out_of_window: i64,
    rejected: usize,
    reason_counts: BTreeMap<String, usize>,
    first_rejection: Option<ValidationIssue>,
}

impl RunWrite {
    fn new(
        run_id: Uuid,
        tenant: &TenantId,
        endpoint: &str,
        source: Source,
        def: &EndpointDefinition,
        now: DateTime<Utc>,
    ) -> Result<Self> {
        Ok(Self {
            run_id,
            tenant: tenant.clone(),
            endpoint: endpoint.to_string(),
            source,
            now,
            lineage: Lineage::new(tenant, source, now),
            checker: RecordChecker::new(def),
            rows: Spool::new()?,
            quarantine: Spool::new()?,
            seen: 0,
            out_of_window: 0,
            rejected: 0,
            reason_counts: BTreeMap::new(),
            first_rejection: None,
        })
    }

    /// Count one rejected record, write its quarantine row, and keep the first one as the example.
    fn reject(&mut self, issue: ValidationIssue) -> Result<()> {
        self.rejected += 1;
        for reason in &issue.reasons {
            *self.reason_counts.entry(reason.clone()).or_default() += 1;
        }
        let row = quarantine_payload(
            self.run_id,
            &self.tenant,
            &self.endpoint,
            self.source,
            self.now,
            &issue,
        );
        self.quarantine.push(&row)?;
        if self.first_rejection.is_none() {
            self.first_rejection = Some(issue);
        }
        Ok(())
    }

    /// The message a run fails with when it rejected records.
    fn rejection_message(&self) -> String {
        let first = self.first_rejection.as_ref();
        let first_reasons = first
            .map(|issue| issue.reasons.join("; "))
            .unwrap_or_else(|| "unknown".to_string());
        let first_key = first
            .and_then(|issue| issue.key.clone())
            .unwrap_or_else(|| "none".to_string());
        let first_record_keys = first
            .and_then(|issue| issue.record.as_object())
            .map(|object| {
                let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
                keys.sort_unstable();
                keys.join(",")
            })
            .unwrap_or_else(|| "non_object".to_string());
        format!(
            "sync validation failed: {} rejected row(s), {} exact duplicate row(s) dropped; rows per reason: {}; first rejection: {}; first key: {}; first record keys: {}",
            self.rejected,
            self.checker.duplicates_dropped(),
            rejection_summary(&self.reason_counts),
            first_reasons,
            first_key,
            first_record_keys
        )
    }
}

/// Takes one partition's pages as the source returns them. Each record is windowed, stamped with its
/// tenant and lineage, checked, then spooled or quarantined. Nothing else is kept per record, so the
/// run's memory does not grow with the endpoint.
struct PartitionSink<'a> {
    run: &'a mut RunWrite,
    def: &'a EndpointDefinition,
    partition_key: &'a str,
    date_from: Option<DateTime<Utc>>,
    date_to: Option<DateTime<Utc>>,
    /// DataMart rows that a normalizer reshapes are checked by the warehouse loader, not here.
    unchecked: bool,
    /// Records the source delivered for this partition.
    rows_in: i64,
    /// The newest modification time among this partition's kept records.
    newest: Option<DateTime<Utc>>,
}

impl PageSink for PartitionSink<'_> {
    fn accept(&mut self, page: Vec<Value>) -> Result<()> {
        for record in page {
            self.rows_in += 1;
            if !in_window(
                &record,
                self.run.source,
                self.def,
                self.date_from,
                self.date_to,
            ) {
                self.run.out_of_window += 1;
                continue;
            }
            let record = coerce_scalar_record(record, scalar_key(self.def));
            let record = self.run.lineage.stamp(record);
            let index = self.run.seen;
            self.run.seen += 1;
            let kept = if self.unchecked {
                Some(record)
            } else {
                match self.run.checker.check(index, record) {
                    Verdict::Valid(record) => Some(record),
                    Verdict::Duplicate => None,
                    Verdict::Rejected(issue) => {
                        self.run.reject(issue)?;
                        None
                    }
                }
            };
            if let Some(row) = kept {
                if row_belongs_to_partition(self.def, &row, self.partition_key) {
                    self.newest = max_watermark(
                        self.newest,
                        record_timestamp(&row, self.run.source, self.def),
                    );
                }
                self.run.rows.push(&row)?;
            }
        }
        Ok(())
    }
}

/// The tenant, source and run time every row carries.
struct Lineage {
    tenant_id: String,
    source: String,
    synced_at: String,
}

impl Lineage {
    fn new(tenant: &TenantId, source: Source, now: DateTime<Utc>) -> Self {
        Self {
            tenant_id: tenant.as_str().to_string(),
            source: source.to_string(),
            synced_at: now.to_rfc3339(),
        }
    }

    /// Stamp `record` with the tenant and lineage. A value that is not an object passes through.
    fn stamp(&self, mut record: Value) -> Value {
        if let Value::Object(ref mut m) = record {
            m.insert("tenant_id".into(), Value::String(self.tenant_id.clone()));
            m.insert("_source".into(), Value::String(self.source.clone()));
            m.insert("_synced_at".into(), Value::String(self.synced_at.clone()));
        }
        record
    }
}

impl SyncExecutor {
    /// Sync a single endpoint for a single tenant.
    ///
    /// Flow: load checkpoint → open run record → fetch → BQ write → update checkpoint → close run.
    pub async fn sync_one(&self, tenant: &TenantId, endpoint_name: &str) -> Result<u64> {
        self.sync_one_window(tenant, endpoint_name, None, None)
            .await
    }

    /// Sync one endpoint constrained to an optional closed window.
    ///
    /// When `date_to` is set, rows outside the interval are dropped before
    /// validation/write/checkpoint updates.
    pub async fn sync_one_window(
        &self,
        tenant: &TenantId,
        endpoint_name: &str,
        date_from: Option<DateTime<Utc>>,
        date_to: Option<DateTime<Utc>>,
    ) -> Result<u64> {
        self.sync_one_scoped(tenant, endpoint_name, date_from, date_to, false)
            .await
    }

    /// Sync one endpoint over its whole scope, ignoring checkpoints.
    ///
    /// The fetch is complete, so what Visma no longer has can be found: the staging write marks
    /// the tenant's rows missing from it as deleted. Checkpoints still advance (they never move
    /// backwards). This is how an incremental endpoint gets its deletes reconciled.
    pub async fn sync_one_full(&self, tenant: &TenantId, endpoint_name: &str) -> Result<u64> {
        self.sync_one_scoped(tenant, endpoint_name, None, None, true)
            .await
    }

    async fn sync_one_scoped(
        &self,
        tenant: &TenantId,
        endpoint_name: &str,
        date_from: Option<DateTime<Utc>>,
        date_to: Option<DateTime<Utc>>,
        full: bool,
    ) -> Result<u64> {
        if let (Some(from), Some(to)) = (date_from, date_to) {
            if from > to {
                return Err(Error::Config(format!(
                    "invalid sync window for endpoint '{endpoint_name}': date_from > date_to"
                )));
            }
        }

        let def = self.registry.get(endpoint_name).ok_or_else(|| {
            vsv2_common::Error::UnknownEndpoint {
                name: endpoint_name.into(),
            }
        })?;
        if !def.enabled {
            return Err(vsv2_common::Error::Config(format!(
                "endpoint '{endpoint_name}' is disabled"
            )));
        }

        let requested_source = endpoint_flags::get_flag(&self.state.pg, tenant, endpoint_name)
            .await?
            .unwrap_or_else(|| def.effective_source(tenant));
        let source = def.live_source(requested_source)?;
        if source != requested_source {
            warn!(
                tenant = %tenant,
                endpoint = endpoint_name,
                requested_source = %requested_source,
                live_source = %source,
                "DataMart is not ready; keeping the live sync on its fallback source"
            );
        }

        let run_id = Uuid::new_v4();
        self.state
            .insert_run(&SyncRun {
                run_id,
                tenant: tenant.clone(),
                endpoint: endpoint_name.into(),
                source,
                status: RunStatus::Running,
                started_at: Utc::now(),
                finished_at: None,
                records_in: 0,
                records_written: 0,
                error: None,
            })
            .await?;
        info!(tenant = %tenant, endpoint = endpoint_name, %source, "sync: start");

        // One stamp for the whole run: a record served twice, on two pages, must compare equal.
        let mut run = match RunWrite::new(run_id, tenant, endpoint_name, source, def, Utc::now()) {
            Ok(run) => run,
            Err(e) => {
                let msg = e.to_string();
                self.state
                    .finish_run(run_id, RunStatus::Failed, 0, 0, Some(&msg))
                    .await?;
                return Err(e);
            }
        };

        let fetch_result = self
            .fetch_checkpointed_partitions(
                &mut run,
                tenant,
                endpoint_name,
                def,
                source,
                FetchScope {
                    date_from,
                    date_to,
                    full,
                },
            )
            .await;

        match fetch_result {
            Ok(fetched_partitions) => {
                let complete = is_complete_scan(
                    date_from,
                    date_to,
                    fetched_partitions.iter().map(|p| p.full_scope),
                );
                let records_in: i64 = fetched_partitions.iter().map(|p| p.records_in).sum();
                let mut partition_states = Vec::with_capacity(fetched_partitions.len());
                let mut page_metrics = Vec::new();
                for partition in fetched_partitions {
                    page_metrics.extend(sync_page_metrics(
                        run_id,
                        tenant,
                        endpoint_name,
                        &partition.key,
                        partition.page_metrics,
                    ));
                    partition_states.push(PartitionCheckpointState {
                        key: partition.key,
                        prior: partition.prior,
                        records_in: partition.records_in,
                        newest: partition.newest,
                    });
                }

                info!(
                    tenant = %tenant,
                    endpoint = endpoint_name,
                    partitions = partition_states.len(),
                    records = records_in,
                    records_out_of_window = run.out_of_window,
                    "sync: fetched"
                );

                if let Err(e) = self.state.insert_page_metrics(&page_metrics).await {
                    let msg = e.to_string();
                    self.state
                        .finish_run(run_id, RunStatus::Failed, records_in, 0, Some(&msg))
                        .await?;
                    return Err(e);
                }

                if run.rejected > 0 {
                    let message = run.rejection_message();
                    warn!(
                        tenant = %tenant,
                        endpoint = endpoint_name,
                        rejected = run.rejected,
                        duplicates_dropped = run.checker.duplicates_dropped(),
                        "sync: validation failed"
                    );

                    if let Some(bq) = &self.bq {
                        let path = run.quarantine.path()?;
                        if let Err(e) = bq.write_quarantine_file(path).await {
                            let msg = format!("{message}; quarantine write failed: {e}");
                            self.state
                                .finish_run(run_id, RunStatus::Failed, records_in, 0, Some(&msg))
                                .await?;
                            return Err(e);
                        }
                    }

                    self.state
                        .finish_run(run_id, RunStatus::Failed, records_in, 0, Some(&message))
                        .await?;
                    return Err(Error::Config(message));
                }

                let primary_key = scoped_primary_key(def);
                let normalizer: Option<&str> = if source == Source::DatamartGraphql {
                    def.datamart
                        .as_ref()
                        .and_then(|dm| dm.join_normalizer.as_deref())
                } else {
                    None
                };
                let run_id_str = run_id.to_string();
                let lineage = MergeLineage {
                    run_id: &run_id_str,
                    endpoint: endpoint_name,
                    source: source.as_str(),
                };
                let rows_written = if let Some(bq) = &self.bq {
                    let result = if bq.stages() {
                        bq.stage_file(
                            endpoint_name,
                            &primary_key,
                            run.rows.path()?,
                            lineage,
                            tenant.as_str(),
                            complete,
                        )
                        .await
                        .map(|outcome| {
                            if outcome.deleted > 0 {
                                info!(
                                    tenant = %tenant,
                                    endpoint = endpoint_name,
                                    deleted = outcome.deleted,
                                    "sync: marked records deleted that Visma no longer has"
                                );
                            }
                            if outcome.delete_skipped > 0 {
                                warn!(
                                    tenant = %tenant,
                                    endpoint = endpoint_name,
                                    would_delete = outcome.delete_skipped,
                                    "sync: a complete scan lacks too many rows to treat as deleted; \
                                     left them alone"
                                );
                            }
                            outcome.rows
                        })
                    } else if run.rows.rows() == 0 {
                        Ok(0)
                    } else {
                        bq.merge_file(
                            &def.bigquery_table,
                            &primary_key,
                            run.rows.path()?,
                            normalizer,
                            lineage,
                        )
                        .await
                    };

                    match result {
                        Ok(n) => n as i64,
                        Err(e) => {
                            error!(err = %e, "sync: bq write failed");
                            let msg = e.to_string();
                            self.state
                                .finish_run(run_id, RunStatus::Failed, records_in, 0, Some(&msg))
                                .await?;
                            return Err(e);
                        }
                    }
                } else if self.local_bronze_enabled {
                    let rows = run.rows.read_all()?;
                    match self
                        .state
                        .upsert_local_bronze_records(
                            run_id,
                            tenant,
                            endpoint_name,
                            source,
                            &rows,
                            &primary_key,
                        )
                        .await
                    {
                        Ok(n) => {
                            info!(
                                tenant = %tenant,
                                endpoint = endpoint_name,
                                rows_written = n,
                                "sync: wrote records to local Postgres bronze"
                            );
                            n as i64
                        }
                        Err(e) => {
                            error!(err = %e, "sync: local Postgres bronze write failed");
                            let msg = e.to_string();
                            self.state
                                .finish_run(run_id, RunStatus::Failed, records_in, 0, Some(&msg))
                                .await?;
                            return Err(e);
                        }
                    }
                } else {
                    info!(tenant = %tenant, endpoint = endpoint_name, "sync: bq not configured, skipping write");
                    0
                };

                let checkpoints = partition_states
                    .iter()
                    .map(|partition| {
                        partition_checkpoint(
                            &partition.key,
                            partition.newest,
                            partition.records_in,
                            partition.prior.as_ref(),
                            date_to,
                        )
                    })
                    .collect::<Vec<_>>();
                self.state.upsert_checkpoints(&checkpoints).await?;
                self.state
                    .finish_run(run_id, RunStatus::Succeeded, records_in, rows_written, None)
                    .await?;
                Ok(records_in as u64)
            }
            Err(e) => {
                error!(tenant = %tenant, endpoint = endpoint_name, err = %e, "sync: fetch failed");
                let msg = e.to_string();
                self.state
                    .finish_run(run_id, RunStatus::Failed, 0, 0, Some(&msg))
                    .await?;
                Err(e)
            }
        }
    }

    async fn fetch_checkpointed_partitions(
        &self,
        run: &mut RunWrite,
        tenant: &TenantId,
        endpoint_name: &str,
        def: &EndpointDefinition,
        source: Source,
        scope: FetchScope,
    ) -> Result<Vec<FetchedPartition>> {
        let FetchScope {
            date_from,
            date_to,
            full,
        } = scope;
        let run_id = run.run_id;
        let adapter = self.dispatch.pick_source(source)?;
        let partitions = adapter.plan_partitions(tenant, def).await?;
        if partitions.is_empty() {
            // A partition dimension whose listing is empty: nothing exists to ask for.
            warn!(
                tenant = %tenant,
                endpoint = endpoint_name,
                "sync: no fetch partitions (a partition dimension has no values)"
            );
            return Ok(Vec::new());
        }

        // DataMart rows that a normalizer reshapes are checked by the warehouse loader, after the reshape.
        let skips_checks = source == Source::DatamartGraphql
            && def
                .datamart
                .as_ref()
                .and_then(|dm| dm.join_normalizer.as_deref())
                .is_some();
        let mut fetched = Vec::with_capacity(partitions.len());
        for partition in partitions {
            let key = CheckpointKey::for_partition(
                tenant.clone(),
                endpoint_name.into(),
                source,
                partition.key.clone(),
            );
            let prior = self.state.get_checkpoint(&key).await?;
            // A period-scanned endpoint has no record timestamps, so its checkpoint only holds
            // the end of some earlier explicit window. Resuming from that would shrink the next
            // run to the periods since then and lose every back-dated document.
            let base_watermark =
                if full || (source == Source::ErpRest && def.period_scan_ignores_checkpoint()) {
                    None
                } else {
                    overlap_watermark(
                        endpoint_name,
                        source,
                        checkpoint_watermark(source, prior.as_ref()),
                    )
                };
            // An explicit window names its range on purpose, so its start wins even when the
            // checkpoint is already past it (a backfill of an older window). The stored
            // checkpoint cannot move backwards, so this never rewinds regular syncs. Without
            // `date_from` the run resumes from the checkpoint as before.
            let watermark = date_from.or(base_watermark);
            let cursor = if full {
                None
            } else {
                checkpoint_cursor(source, prior.as_ref())
            };
            // Nothing narrowed this fetch: it asked for everything Visma has.
            let full_scope = watermark.is_none() && cursor.is_none();

            let mut sink = PartitionSink {
                run: &mut *run,
                def,
                partition_key: &partition.key,
                date_from,
                date_to,
                unchecked: skips_checks,
                rows_in: 0,
                newest: None,
            };
            let batch = match adapter
                .fetch_partition(
                    tenant,
                    def,
                    &partition,
                    FetchWindow {
                        watermark,
                        window_end: date_to,
                        cursor,
                    },
                    &mut sink,
                )
                .await
            {
                Ok(batch) => batch,
                Err(e) => {
                    if let Some(metric) = failed_page_metric(def, source, &e) {
                        let key = CheckpointKey::for_partition(
                            tenant.clone(),
                            endpoint_name.into(),
                            source,
                            partition.key.clone(),
                        );
                        let metrics =
                            sync_page_metrics(run_id, tenant, endpoint_name, &key, vec![metric]);
                        if let Err(metric_err) = self.state.insert_page_metrics(&metrics).await {
                            warn!(
                                tenant = %tenant,
                                endpoint = endpoint_name,
                                err = %metric_err,
                                "sync: failed to persist failed-page metric"
                            );
                        }
                    }
                    return Err(e);
                }
            };
            fetched.push(FetchedPartition {
                key,
                prior,
                records_in: sink.rows_in,
                newest: sink.newest,
                page_metrics: batch.page_metrics,
                full_scope,
            });
        }

        Ok(fetched)
    }

    /// Sync every registered endpoint for a tenant, continuing past per-endpoint errors.
    pub async fn sync_all(&self, tenant: &TenantId) -> Result<u64> {
        self.sync_all_window(tenant, None, None).await
    }

    pub async fn sync_all_window(
        &self,
        tenant: &TenantId,
        date_from: Option<DateTime<Utc>>,
        date_to: Option<DateTime<Utc>>,
    ) -> Result<u64> {
        Ok(self
            .sync_all_window_detailed(tenant, date_from, date_to)
            .await?
            .records_synced)
    }

    pub async fn sync_all_window_detailed(
        &self,
        tenant: &TenantId,
        date_from: Option<DateTime<Utc>>,
        date_to: Option<DateTime<Utc>>,
    ) -> Result<SyncExecutionSummary> {
        let names: Vec<String> = self
            .registry
            .iter()
            .filter(|d| d.enabled)
            .map(|d| d.endpoint_name.clone())
            .collect();
        self.sync_names_window_detailed(tenant, &names, date_from, date_to)
            .await
    }

    /// Sync a specific list of endpoints for a tenant, continuing past per-endpoint errors.
    pub async fn sync_names(&self, tenant: &TenantId, names: &[String]) -> Result<u64> {
        self.sync_names_window(tenant, names, None, None).await
    }

    pub async fn sync_names_window(
        &self,
        tenant: &TenantId,
        names: &[String],
        date_from: Option<DateTime<Utc>>,
        date_to: Option<DateTime<Utc>>,
    ) -> Result<u64> {
        Ok(self
            .sync_names_window_detailed(tenant, names, date_from, date_to)
            .await?
            .records_synced)
    }

    pub async fn sync_names_window_detailed(
        &self,
        tenant: &TenantId,
        names: &[String],
        date_from: Option<DateTime<Utc>>,
        date_to: Option<DateTime<Utc>>,
    ) -> Result<SyncExecutionSummary> {
        self.sync_names_scoped(tenant, names, date_from, date_to, false)
            .await
    }

    /// `sync_one_full` for each of `names`, continuing past per-endpoint errors.
    pub async fn sync_names_full(
        &self,
        tenant: &TenantId,
        names: &[String],
    ) -> Result<SyncExecutionSummary> {
        self.sync_names_scoped(tenant, names, None, None, true)
            .await
    }

    async fn sync_names_scoped(
        &self,
        tenant: &TenantId,
        names: &[String],
        date_from: Option<DateTime<Utc>>,
        date_to: Option<DateTime<Utc>>,
        full: bool,
    ) -> Result<SyncExecutionSummary> {
        let mut total = 0u64;
        let mut failed_endpoints = Vec::new();
        for name in names {
            match self
                .sync_one_scoped(tenant, name, date_from, date_to, full)
                .await
            {
                Ok(n) => total += n,
                Err(e) => {
                    warn!(
                        tenant = %tenant, endpoint = %name, err = %e,
                        "scheduler: skipping failed endpoint"
                    );
                    failed_endpoints.push(EndpointSyncFailure {
                        endpoint: name.clone(),
                        error: vsv2_common::text::storable_text(&e.to_string()).into_owned(),
                    });
                }
            }
        }
        Ok(SyncExecutionSummary {
            records_synced: total,
            attempted_endpoints: names.len(),
            failed_endpoints,
        })
    }

    /// Collect endpoint names whose tier falls within `[min_tier, max_tier]`.
    /// Endpoints with no tier set are treated as tier 2 (daily).
    pub fn endpoints_for_tier(&self, min_tier: u8, max_tier: u8) -> Vec<String> {
        self.registry
            .iter()
            .filter(|d| {
                let t = d.tier.unwrap_or(2);
                d.enabled && t >= min_tier && t <= max_tier
            })
            .map(|d| d.endpoint_name.clone())
            .collect()
    }
}

/// The checkpoint a partition leaves after a run. Its watermark moves to the newest record it kept
/// (`newest`), never backwards and never past the window's end.
fn partition_checkpoint(
    key: &CheckpointKey,
    newest: Option<DateTime<Utc>>,
    n: i64,
    prior: Option<&Checkpoint>,
    window_end: Option<DateTime<Utc>>,
) -> Checkpoint {
    let now = Utc::now();
    let (last_modified, last_updated_in_dm) = match key.source {
        Source::ErpRest | Source::ExpenseApi => {
            let prior_wm = prior.and_then(|cp| cp.last_modified_watermark);
            let wm = cap_watermark(max_watermark(prior_wm, newest), window_end);
            (wm, None)
        }
        Source::DatamartGraphql => {
            let prior_wm = prior.and_then(|cp| cp.last_updated_in_dm);
            let wm = cap_watermark(max_watermark(prior_wm, newest), window_end);
            (None, wm)
        }
    };

    Checkpoint {
        key: key.clone(),
        last_modified_watermark: last_modified,
        last_updated_in_dm,
        last_id: None,
        last_sync_at: Some(now),
        records_seen: n,
    }
}

pub(crate) fn sync_page_metrics(
    run_id: Uuid,
    tenant: &TenantId,
    endpoint: &str,
    key: &CheckpointKey,
    metrics: Vec<PageFetchMetric>,
) -> Vec<SyncPageMetric> {
    let fetched_at = Utc::now();
    metrics
        .into_iter()
        .map(|metric| SyncPageMetric {
            run_id,
            tenant: tenant.clone(),
            endpoint: endpoint.to_string(),
            source: key.source,
            partition_key: key.partition_key.clone(),
            upstream_path: metric.upstream_path,
            page_param: metric.page_param,
            page_value: metric.page_value,
            page_size: metric.page_size.map(i64::from),
            status: i32::from(metric.status),
            elapsed_ms: metric.elapsed_ms,
            records: metric.records,
            total_count: metric.total_count,
            has_next: metric.has_next,
            rate_limit_limit: metric.rate_limit.limit.map(i64::from),
            rate_limit_remaining: metric.rate_limit.remaining.map(i64::from),
            rate_limit_reset: metric.rate_limit.reset,
            rate_limit_policy: metric.rate_limit.policy,
            retry_after_secs: metric.rate_limit.retry_after_secs,
            background_mode: metric.background_mode,
            fetched_at,
        })
        .collect()
}

fn failed_page_metric(
    def: &EndpointDefinition,
    source: Source,
    error: &Error,
) -> Option<PageFetchMetric> {
    let (status, retry_after_secs) = match error {
        Error::RateLimited { until, .. } => {
            let retry_after = (*until - Utc::now()).num_seconds().max(0);
            (429, Some(retry_after))
        }
        Error::UpstreamStatus { status, .. } => (*status, None),
        _ => return None,
    };

    Some(PageFetchMetric {
        upstream_path: metric_path(def, source),
        page_param: None,
        page_value: None,
        page_size: def.page_size,
        status,
        elapsed_ms: 0,
        records: 0,
        total_count: None,
        has_next: None,
        rate_limit: vsv2_common::RateLimitSnapshot {
            retry_after_secs,
            ..Default::default()
        },
        background_mode: background_mode_name(def.background_mode).map(str::to_string),
    })
}

fn background_mode_name(mode: vsv2_common::BackgroundMode) -> Option<&'static str> {
    match mode {
        vsv2_common::BackgroundMode::Off => None,
        vsv2_common::BackgroundMode::None => Some("none"),
        vsv2_common::BackgroundMode::Subscription => Some("subscription"),
    }
}

fn metric_path(def: &EndpointDefinition, source: Source) -> String {
    if source == Source::DatamartGraphql {
        return def
            .datamart
            .as_ref()
            .and_then(|dm| dm.root_field.clone())
            .unwrap_or_else(|| def.endpoint_name.clone());
    }

    def.api_endpoint
        .clone()
        .or_else(|| def.api_method.clone())
        .unwrap_or_else(|| def.endpoint_name.clone())
}

/// How many rejected rows each reason covers, most common first, e.g.
/// `missing validation field descriptionText: 35; duplicate primary key with different payload: 2`.
///
/// The first rejection alone says nothing about the other fields a config wrongly requires, so
/// fixing them one failed run at a time takes as many runs as there are fields.
fn rejection_summary(counts: &BTreeMap<String, usize>) -> String {
    let mut ranked: Vec<(&str, usize)> = counts
        .iter()
        .map(|(reason, n)| (reason.as_str(), *n))
        .collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    ranked
        .iter()
        .take(12)
        .map(|(reason, n)| format!("{reason}: {n}"))
        .collect::<Vec<_>>()
        .join("; ")
}

fn row_belongs_to_partition(def: &EndpointDefinition, record: &Value, partition_key: &str) -> bool {
    if def.partitioning.is_none() {
        return partition_key == vsv2_state::DEFAULT_PARTITION_KEY;
    }

    record
        .get("_partition_key")
        .and_then(Value::as_str)
        .is_some_and(|value| value == partition_key)
}

fn checkpoint_watermark(source: Source, prior: Option<&Checkpoint>) -> Option<DateTime<Utc>> {
    prior.and_then(|c| match source {
        Source::ErpRest => c.last_modified_watermark,
        Source::DatamartGraphql => c.last_updated_in_dm,
        Source::ExpenseApi => c.last_modified_watermark,
    })
}

fn checkpoint_cursor(source: Source, prior: Option<&Checkpoint>) -> Option<String> {
    match source {
        Source::DatamartGraphql => None,
        _ => prior.and_then(|c| c.last_id.clone()),
    }
}

fn overlap_watermark(
    endpoint_name: &str,
    source: Source,
    watermark: Option<DateTime<Utc>>,
) -> Option<DateTime<Utc>> {
    let watermark = watermark?;
    let endpoint = endpoint_name.to_ascii_lowercase();
    let overlap = match source {
        Source::ErpRest
            if endpoint.contains("general_ledger")
                || endpoint.contains("journal")
                || endpoint.contains("transaction") =>
        {
            Duration::hours(24)
        }
        _ => Duration::minutes(10),
    };
    // ERP `lastModifiedDateTime` is Europe/Oslo wall-clock time, so a watermark set in the first
    // pass of the repeated fall-back hour must reach back over the whole hour to catch changes
    // stamped in the second pass. DataMart's cursor is true UTC and needs no such widening.
    let overlap = if source == Source::ErpRest && in_repeated_fall_back_hour(watermark) {
        overlap.max(FALL_BACK_OVERLAP)
    } else {
        overlap
    };
    Some(watermark - overlap)
}

/// The repeated hour (60 min) plus the usual 10-minute overlap.
const FALL_BACK_OVERLAP: Duration = Duration::minutes(70);

/// True when `naive_local` (a wall-clock value labelled UTC, as the ERP stamps are) falls in the
/// hour 02:00-02:59 on the last Sunday of October, which happens twice when the clocks go back.
fn in_repeated_fall_back_hour(naive_local: DateTime<Utc>) -> bool {
    let date = naive_local.date_naive();
    last_sunday_of_october(date.year()) == Some(date) && naive_local.hour() == 2
}

fn last_sunday_of_october(year: i32) -> Option<NaiveDate> {
    let october_31 = NaiveDate::from_ymd_opt(year, 10, 31)?;
    Some(october_31 - Duration::days(i64::from(october_31.weekday().num_days_from_sunday())))
}

fn max_watermark(
    prior: Option<DateTime<Utc>>,
    current: Option<DateTime<Utc>>,
) -> Option<DateTime<Utc>> {
    match (prior, current) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn cap_watermark(
    value: Option<DateTime<Utc>>,
    window_end: Option<DateTime<Utc>>,
) -> Option<DateTime<Utc>> {
    match (value, window_end) {
        (Some(v), Some(end)) => Some(v.min(end)),
        (Some(v), None) => Some(v),
        (None, Some(end)) => Some(end),
        (None, None) => None,
    }
}

/// Whether a record falls inside the run's window. ERP records always do: replays already pass the
/// window upstream, and endpoint timestamps are not consistently aligned with it, so the fetched
/// batch is kept intact to avoid dropping valid replay rows. A record without a usable timestamp is
/// kept too.
fn in_window(
    record: &Value,
    source: Source,
    def: &EndpointDefinition,
    date_from: Option<DateTime<Utc>>,
    date_to: Option<DateTime<Utc>>,
) -> bool {
    if (date_from.is_none() && date_to.is_none()) || source == Source::ErpRest {
        return true;
    }

    match (record_timestamp(record, source, def), date_from, date_to) {
        (Some(ts), Some(from), Some(to)) => ts >= from && ts <= to,
        (Some(ts), Some(from), None) => ts >= from,
        (Some(ts), None, Some(to)) => ts <= to,
        (Some(_), None, None) => true,
        (None, _, _) => true,
    }
}

fn record_timestamp(
    record: &Value,
    source: Source,
    def: &EndpointDefinition,
) -> Option<DateTime<Utc>> {
    let field = match source {
        Source::ErpRest => def
            .modified_field
            .as_deref()
            .unwrap_or("lastModifiedDateTime"),
        Source::DatamartGraphql => def
            .datamart
            .as_ref()
            .and_then(|dm| dm.cursor_field.as_deref())
            .unwrap_or("updatedInDataMartDateTime"),
        Source::ExpenseApi => def.modified_field.as_deref().unwrap_or("approvedDate"),
    };

    record
        .get(field)
        .and_then(Value::as_str)
        .and_then(parse_utc_datetime)
}

/// Formats Visma returns without a timezone designator: `/v1/department` sends
/// "2026-02-19T13:27:34.987", older Finance endpoints "2026-02-19 13:27:34".
const NAIVE_DATETIME_FORMATS: [&str; 2] = ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"];

fn parse_utc_datetime(raw: &str) -> Option<DateTime<Utc>> {
    // RFC3339 first (e.g. "2026-01-15T10:30:00Z")
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(raw) {
        return Some(dt.with_timezone(&Utc));
    }
    // No timezone: keep the wall-clock value and label it UTC. The same string
    // is sent back as `lastModifiedDateTime` on the next request, where the API
    // reads it in its own zone, so the round trip is lossless.
    NAIVE_DATETIME_FORMATS.iter().find_map(|format| {
        chrono::NaiveDateTime::parse_from_str(raw, format)
            .ok()
            .map(|ndt| DateTime::from_naive_utc_and_offset(ndt, Utc))
    })
}

/// The primary key a bare scalar record is wrapped under: the endpoint's key, when it is a plain field.
fn scalar_key(def: &EndpointDefinition) -> Option<&str> {
    match &def.primary_key {
        PrimaryKey::Single(primary_key) if !primary_key.contains('.') => Some(primary_key.as_str()),
        _ => None,
    }
}

/// Some ERP endpoints return bare scalar identifiers (e.g. dimensions). A scalar is wrapped into an
/// object under the endpoint's primary key, so tenant enrichment, validation, and local bronze keying
/// work on it like on any other record.
fn coerce_scalar_record(record: Value, primary_key: Option<&str>) -> Value {
    match (record, primary_key) {
        (record @ Value::Object(_), _) | (record, None) => record,
        (scalar, Some(primary_key)) => {
            let mut object = serde_json::Map::with_capacity(1);
            object.insert(primary_key.to_string(), scalar);
            Value::Object(object)
        }
    }
}

/// Inject tenant and lineage metadata into every record before BQ write.
///
/// Enables tenant-scoped uniqueness and tracking which upstream system last
/// wrote each row, and when.
pub(crate) fn enrich(
    records: Vec<Value>,
    tenant: &TenantId,
    source: Source,
    now: DateTime<Utc>,
) -> Vec<Value> {
    let lineage = Lineage::new(tenant, source, now);
    records
        .into_iter()
        .map(|record| lineage.stamp(record))
        .collect()
}

pub(crate) fn scoped_primary_key(def: &EndpointDefinition) -> String {
    let primary_key = def.primary_key_arg();
    let mut parts: Vec<String> = primary_key
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect();

    if def.partitioning.is_some()
        && !parts
            .iter()
            .any(|part| part.eq_ignore_ascii_case("_partition_key"))
    {
        parts.insert(0, "_partition_key".into());
    }

    if parts
        .iter()
        .any(|part| part.eq_ignore_ascii_case("tenant_id"))
    {
        return parts.join(",");
    }

    std::iter::once("tenant_id".to_string())
        .chain(parts)
        .collect::<Vec<_>>()
        .join(",")
}

/// One quarantine row: the rejected record as it arrived, and why it was refused.
fn quarantine_payload(
    run_id: Uuid,
    tenant: &TenantId,
    endpoint: &str,
    source: Source,
    quarantined_at: DateTime<Utc>,
    issue: &ValidationIssue,
) -> Value {
    serde_json::json!({
        "sync_run_id": run_id.to_string(),
        "tenant_id": tenant.as_str(),
        "endpoint": endpoint,
        "source": source.as_str(),
        "record_index": issue.index as i64,
        "primary_key": issue.key,
        "reasons_json": serde_json::to_string(&issue.reasons).unwrap_or_else(|_| "[]".to_string()),
        "raw_payload_json": serde_json::to_string(&issue.record).unwrap_or_else(|_| issue.record.to_string()),
        "quarantined_at": quarantined_at.to_rfc3339(),
    })
}

pub(crate) fn quarantine_payloads(
    run_id: Uuid,
    tenant: &TenantId,
    endpoint: &str,
    source: Source,
    quarantined_at: DateTime<Utc>,
    issues: &[ValidationIssue],
) -> Vec<Value> {
    issues
        .iter()
        .map(|issue| quarantine_payload(run_id, tenant, endpoint, source, quarantined_at, issue))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;
    use vsv2_common::{BackgroundMode, PaginationProfile, PrimaryKey};

    /// The window filter over a whole list.
    fn filter_records_to_window(
        records: &[Value],
        source: Source,
        def: &EndpointDefinition,
        date_from: Option<DateTime<Utc>>,
        date_to: Option<DateTime<Utc>>,
    ) -> Vec<Value> {
        records
            .iter()
            .filter(|record| in_window(record, source, def, date_from, date_to))
            .cloned()
            .collect()
    }

    /// The scalar wrapping over a whole list.
    fn coerce_scalar_records_for_keys(records: Vec<Value>, def: &EndpointDefinition) -> Vec<Value> {
        records
            .into_iter()
            .map(|record| coerce_scalar_record(record, scalar_key(def)))
            .collect()
    }

    /// The rejection summary of a list of rejected records.
    fn reason_counts(rejected: &[ValidationIssue]) -> String {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for issue in rejected {
            for reason in &issue.reasons {
                *counts.entry(reason.clone()).or_default() += 1;
            }
        }
        rejection_summary(&counts)
    }

    /// The checkpoint a partition leaves, folded from its rows the way a run folds them.
    fn derive_checkpoint_for_partition(
        key: &CheckpointKey,
        def: &EndpointDefinition,
        records: &[Value],
        n: i64,
        prior: Option<&Checkpoint>,
        window_end: Option<DateTime<Utc>>,
    ) -> Checkpoint {
        let newest = records
            .iter()
            .filter(|record| row_belongs_to_partition(def, record, &key.partition_key))
            .fold(None, |newest, record| {
                max_watermark(newest, record_timestamp(record, key.source, def))
            });
        partition_checkpoint(key, newest, n, prior, window_end)
    }

    /// The checkpoint of a fetch whose rows are all kept, whatever their partition.
    fn derive_checkpoint(
        key: &CheckpointKey,
        def: &EndpointDefinition,
        records: &[Value],
        n: i64,
        prior: Option<&Checkpoint>,
        window_end: Option<DateTime<Utc>>,
    ) -> Checkpoint {
        let newest = records.iter().fold(None, |newest, record| {
            max_watermark(newest, record_timestamp(record, key.source, def))
        });
        partition_checkpoint(key, newest, n, prior, window_end)
    }

    fn test_def(endpoint_name: &str) -> EndpointDefinition {
        EndpointDefinition {
            endpoint_name: endpoint_name.into(),
            bigquery_table: "visma_accounts".into(),
            primary_key: PrimaryKey::Single("accountCD".into()),
            source: Source::ErpRest,
            enabled: true,
            api_method: Some("get_accounts".into()),
            api_endpoint: Some("/v1/account".into()),
            fallback_api_method: None,
            pagination_profile: PaginationProfile::FinancePageNumber,
            page_size: None,
            background_mode: BackgroundMode::Off,
            background_poll_secs: None,
            background_timeout_secs: None,
            default_query: HashMap::new(),
            required_query: vec![],
            modified_field: Some("lastModifiedDateTime".into()),
            merge_type: Some("upsert".into()),
            sync_pattern: Some("incremental_upsert".into()),
            validation_fields: Some(vec!["accountCD".into()]),
            partitioning: None,
            period_scan: None,
            datamart: None,
            tier: None,
            open_set: false,
        }
    }

    #[test]
    fn enrich_stamps_tenant_and_lineage() {
        let tenant = TenantId::new("tenant-a");
        let now = DateTime::parse_from_rfc3339("2026-05-01T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let rows = enrich(
            vec![json!({"number": "1000"})],
            &tenant,
            Source::ErpRest,
            now,
        );

        assert_eq!(rows[0]["tenant_id"], "tenant-a");
        assert_eq!(rows[0]["_source"], "erp_rest");
        assert_eq!(rows[0]["_synced_at"], "2026-05-01T10:00:00+00:00");
    }

    #[test]
    fn scoped_primary_key_prepends_tenant_once() {
        let mut def = test_def("accounts");
        assert_eq!(scoped_primary_key(&def), "tenant_id,accountCD");

        def.primary_key = PrimaryKey::Composite(vec!["batchNumber".into(), "lineNumber".into()]);
        assert_eq!(scoped_primary_key(&def), "tenant_id,batchNumber,lineNumber");

        def.primary_key = PrimaryKey::Composite(vec!["tenant_id".into(), "number".into()]);
        assert_eq!(scoped_primary_key(&def), "tenant_id,number");
    }

    #[test]
    fn scoped_primary_key_includes_partition_key_for_partitioned_endpoint() {
        let mut def = test_def("general_ledger_transactions");
        def.primary_key = PrimaryKey::Composite(vec!["batchNumber".into(), "lineNumber".into()]);
        def.partitioning = Some(crate::partition::PartitioningConfig {
            mode: "cartesian".into(),
            dimensions: vec![crate::partition::PartitionDimensionConfig {
                query_param: "ledger".into(),
                source_endpoint: "/v1/ledger".into(),
                source_path: "number".into(),
                fallback_values: vec![],
                ..Default::default()
            }],
            max_partitions: 10,
            ..Default::default()
        });

        assert_eq!(
            scoped_primary_key(&def),
            "tenant_id,_partition_key,batchNumber,lineNumber"
        );
    }

    #[test]
    fn quarantine_payloads_preserve_rejected_raw_record() {
        let tenant = TenantId::new("tenant-a");
        let run_id = Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap();
        let at = DateTime::parse_from_rfc3339("2026-05-01T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let issues = vec![ValidationIssue {
            index: 2,
            key: Some("tenant-a\u{1f}1500".into()),
            reasons: vec!["duplicate primary key with different payload".into()],
            record: json!({"tenant_id": "tenant-a", "accountCD": "1500"}),
        }];

        let rows = quarantine_payloads(run_id, &tenant, "accounts", Source::ErpRest, at, &issues);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["sync_run_id"], run_id.to_string());
        assert_eq!(rows[0]["tenant_id"], "tenant-a");
        assert_eq!(
            rows[0]["reasons_json"],
            "[\"duplicate primary key with different payload\"]"
        );
        assert!(rows[0]["raw_payload_json"]
            .as_str()
            .unwrap()
            .contains("\"accountCD\":\"1500\""));
    }

    #[test]
    fn coerce_scalar_records_wraps_single_pk_scalars() {
        let mut def = test_def("dimensions");
        def.primary_key = PrimaryKey::Single("dimensionId".into());

        let rows = coerce_scalar_records_for_keys(
            vec![json!("ACCOUNT"), json!({"dimensionId": "CUSTOMER"})],
            &def,
        );

        assert_eq!(rows[0], json!({"dimensionId": "ACCOUNT"}));
        assert_eq!(rows[1], json!({"dimensionId": "CUSTOMER"}));
    }

    #[test]
    fn coerce_scalar_records_skips_composite_or_dotted_pk() {
        let mut composite = test_def("any");
        composite.primary_key = PrimaryKey::Composite(vec!["a".into(), "b".into()]);
        let composite_rows = coerce_scalar_records_for_keys(vec![json!("x")], &composite);
        assert_eq!(composite_rows[0], json!("x"));

        let mut dotted = test_def("any");
        dotted.primary_key = PrimaryKey::Single("account.number".into());
        let dotted_rows = coerce_scalar_records_for_keys(vec![json!("1000")], &dotted);
        assert_eq!(dotted_rows[0], json!("1000"));
    }

    #[test]
    fn overlap_watermark_uses_wider_financial_window() {
        let wm = DateTime::parse_from_rfc3339("2026-05-01T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        assert_eq!(
            overlap_watermark("customers", Source::ErpRest, Some(wm)).unwrap(),
            wm - Duration::minutes(10)
        );
        assert_eq!(
            overlap_watermark("general_ledger_transactions", Source::ErpRest, Some(wm)).unwrap(),
            wm - Duration::hours(24)
        );
    }

    /// ERP `lastModifiedDateTime` is Europe/Oslo wall-clock time without a zone (measured
    /// against DataMart's true-UTC `updatedInDataMartDateTime`: +1h winter, +2h summer). When
    /// the clocks go back (03:00 CEST -> 02:00 CET on the last Sunday of October) local
    /// 02:00-02:59 happens twice, so changes made in the second pass carry stamps that are
    /// *older* than a watermark set in the first pass. A 10-minute overlap would skip them
    /// for good.
    fn naive(rfc3339: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(rfc3339)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn erp_watermark_in_the_repeated_fall_back_hour_is_rewound_past_the_whole_hour() {
        // 2026-10-25 is the last Sunday of October 2026.
        let wm = naive("2026-10-25T02:59:00Z");

        assert_eq!(
            overlap_watermark("customers", Source::ErpRest, Some(wm)).unwrap(),
            naive("2026-10-25T01:49:00Z"),
            "70 minutes: the repeated hour plus the normal 10-minute overlap"
        );
        assert_eq!(
            overlap_watermark(
                "customers",
                Source::ErpRest,
                Some(naive("2026-10-25T02:00:00Z"))
            )
            .unwrap(),
            naive("2026-10-25T00:50:00Z")
        );
    }

    #[test]
    fn erp_watermark_outside_the_repeated_hour_keeps_the_normal_overlap() {
        for (stamp, expected) in [
            ("2026-10-25T01:59:59Z", "2026-10-25T01:49:59Z"), // just before the repeated hour
            ("2026-10-25T03:00:00Z", "2026-10-25T02:50:00Z"), // just after it
            ("2026-10-18T02:30:00Z", "2026-10-18T02:20:00Z"), // a different Sunday
            ("2026-03-29T02:30:00Z", "2026-03-29T02:20:00Z"), // spring forward has no repeat
            ("2026-05-01T10:00:00Z", "2026-05-01T09:50:00Z"),
        ] {
            assert_eq!(
                overlap_watermark("customers", Source::ErpRest, Some(naive(stamp))).unwrap(),
                naive(expected),
                "{stamp}"
            );
        }
    }

    #[test]
    fn fall_back_day_is_the_last_sunday_of_october_in_every_year() {
        // 2027-10-31 and 2028-10-29 are the last Sundays of October.
        for (stamp, expected) in [
            ("2027-10-31T02:30:00Z", "2027-10-31T01:20:00Z"),
            ("2028-10-29T02:30:00Z", "2028-10-29T01:20:00Z"),
            ("2027-10-24T02:30:00Z", "2027-10-24T02:20:00Z"), // not the last Sunday
        ] {
            assert_eq!(
                overlap_watermark("customers", Source::ErpRest, Some(naive(stamp))).unwrap(),
                naive(expected),
                "{stamp}"
            );
        }
    }

    #[test]
    fn datamart_watermark_is_true_utc_and_needs_no_dst_widening() {
        let wm = naive("2026-10-25T02:30:00Z");

        assert_eq!(
            overlap_watermark("customers", Source::DatamartGraphql, Some(wm)).unwrap(),
            wm - Duration::minutes(10)
        );
    }

    #[test]
    fn the_24_hour_financial_overlap_still_wins_on_fall_back_day() {
        let wm = naive("2026-10-25T02:30:00Z");

        assert_eq!(
            overlap_watermark("journal_transactions", Source::ErpRest, Some(wm)).unwrap(),
            wm - Duration::hours(24)
        );
    }

    #[test]
    fn derive_checkpoint_never_moves_watermark_backwards() {
        let tenant = TenantId::new("tenant-a");
        let key = CheckpointKey::default_partition(tenant, "accounts".into(), Source::ErpRest);
        let prior_wm = DateTime::parse_from_rfc3339("2026-05-01T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let prior = Checkpoint {
            key: key.clone(),
            last_modified_watermark: Some(prior_wm),
            last_updated_in_dm: None,
            last_id: None,
            last_sync_at: Some(prior_wm),
            records_seen: 10,
        };

        let cp = derive_checkpoint(&key, &test_def("accounts"), &[], 0, Some(&prior), None);

        assert_eq!(cp.last_modified_watermark, Some(prior_wm));
    }

    #[test]
    fn parse_utc_datetime_accepts_every_format_visma_returns() {
        use chrono::{NaiveDate, TimeZone};

        let at = |h, m, s, ms| {
            Utc.from_utc_datetime(
                &NaiveDate::from_ymd_opt(2026, 2, 19)
                    .unwrap()
                    .and_hms_milli_opt(h, m, s, ms)
                    .unwrap(),
            )
        };

        // Real values seen from /v1/branch (Z), /v1/department (no designator).
        assert_eq!(
            parse_utc_datetime("2026-02-19T13:27:34.987Z"),
            Some(at(13, 27, 34, 987))
        );
        assert_eq!(
            parse_utc_datetime("2026-02-19T15:27:34.987+02:00"),
            Some(at(13, 27, 34, 987))
        );
        assert_eq!(
            parse_utc_datetime("2026-02-19T13:27:34.987"),
            Some(at(13, 27, 34, 987))
        );
        assert_eq!(
            parse_utc_datetime("2026-02-19T13:27:34"),
            Some(at(13, 27, 34, 0))
        );
        assert_eq!(
            parse_utc_datetime("2026-02-19 13:27:34"),
            Some(at(13, 27, 34, 0))
        );
        assert_eq!(
            parse_utc_datetime("2026-02-19 13:27:34.5"),
            Some(at(13, 27, 34, 500))
        );
        assert_eq!(parse_utc_datetime("not a timestamp"), None);
    }

    #[test]
    fn derive_checkpoint_advances_on_timestamps_without_a_timezone_designator() {
        // /v1/department returns "2026-02-19T13:27:34.987" (no offset). A strict
        // RFC3339 parse dropped every such row, leaving the watermark NULL, so
        // each unwindowed sync re-fetched the whole table.
        let key = CheckpointKey::default_partition(
            TenantId::new("tenant-a"),
            "departments".into(),
            Source::ErpRest,
        );
        let records = vec![
            json!({"departmentId": "1", "lastModifiedDateTime": "2026-02-19T13:27:34.987"}),
            json!({"departmentId": "2", "lastModifiedDateTime": "2026-02-19T13:30:00.500"}),
        ];

        let cp = derive_checkpoint(&key, &test_def("departments"), &records, 2, None, None);

        assert_eq!(
            cp.last_modified_watermark,
            parse_utc_datetime("2026-02-19T13:30:00.500")
        );
        assert!(cp.last_modified_watermark.is_some());
    }

    #[test]
    fn derive_checkpoint_reads_the_configured_modified_field() {
        // Sales Order API v3 stamps rows `lastModified`, not `lastModifiedDateTime`. The
        // watermark ignored the configured field, so it stayed NULL and every sync of
        // `sales_orders_v3` re-fetched all 22,022 orders.
        let key = CheckpointKey::default_partition(
            TenantId::new("tenant-a"),
            "sales_orders_v3".into(),
            Source::ErpRest,
        );
        let mut def = test_def("sales_orders_v3");
        def.modified_field = Some("lastModified".into());
        let records = vec![
            json!({"orderId": "1", "lastModified": "2026-06-26T13:45:51.143+02:00"}),
            json!({"orderId": "2", "lastModified": "2026-08-26T08:55:13.903+02:00"}),
        ];

        let cp = derive_checkpoint(&key, &def, &records, 2, None, None);

        assert_eq!(
            cp.last_modified_watermark,
            parse_utc_datetime("2026-08-26T06:55:13.903Z")
        );
    }

    #[test]
    fn partition_checkpoint_uses_only_matching_partition_rows() {
        let tenant = TenantId::new("tenant-a");
        let key = CheckpointKey::for_partition(
            tenant,
            "general_ledger_transactions".into(),
            Source::ErpRest,
            "ledger=ACTUAL".into(),
        );
        let mut def = test_def("general_ledger_transactions");
        def.partitioning = Some(crate::partition::PartitioningConfig {
            mode: "cartesian".into(),
            dimensions: vec![crate::partition::PartitionDimensionConfig {
                query_param: "ledger".into(),
                source_endpoint: "/v1/ledger".into(),
                source_path: "number".into(),
                fallback_values: vec![],
                ..Default::default()
            }],
            max_partitions: 10,
            ..Default::default()
        });
        let records = vec![
            json!({
                "_partition_key": "ledger=ACTUAL",
                "lastModifiedDateTime": "2026-05-01T10:00:00Z",
            }),
            json!({
                "_partition_key": "ledger=BUDGET",
                "lastModifiedDateTime": "2026-05-02T10:00:00Z",
            }),
        ];

        let cp = derive_checkpoint_for_partition(&key, &def, &records, 1, None, None);

        assert_eq!(
            cp.last_modified_watermark,
            Some(
                DateTime::parse_from_rfc3339("2026-05-01T10:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc)
            )
        );
        assert_eq!(cp.records_seen, 1);
    }

    #[test]
    fn sync_page_metrics_attach_runtime_context() {
        let tenant = TenantId::new("tenant-a");
        let run_id = Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap();
        let key = CheckpointKey::for_partition(
            tenant.clone(),
            "general_ledger_transactions".into(),
            Source::ErpRest,
            "ledger=ACTUAL".into(),
        );
        let raw = vec![vsv2_common::PageFetchMetric {
            upstream_path: "/v1/GeneralLedgerTransactions".into(),
            page_param: Some("pageNumber".into()),
            page_value: Some(1),
            page_size: Some(100),
            status: 200,
            elapsed_ms: 250,
            records: 100,
            total_count: Some(450),
            has_next: Some(true),
            rate_limit: vsv2_common::RateLimitSnapshot {
                limit: Some(1000),
                remaining: Some(999),
                reset: Some(1_777_777_777),
                policy: Some("tenant".into()),
                retry_after_secs: None,
            },
            background_mode: Some("none".into()),
        }];

        let metrics = sync_page_metrics(run_id, &tenant, "general_ledger_transactions", &key, raw);

        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].run_id, run_id);
        assert_eq!(metrics[0].tenant, tenant);
        assert_eq!(metrics[0].endpoint, "general_ledger_transactions");
        assert_eq!(metrics[0].partition_key, "ledger=ACTUAL");
        assert_eq!(metrics[0].records, 100);
        assert_eq!(metrics[0].rate_limit_remaining, Some(999));
    }

    #[test]
    fn failed_page_metric_records_rate_limited_response() {
        let mut def = test_def("customers");
        def.api_endpoint = Some("/v1/customer".into());
        def.page_size = Some(100);
        let until = Utc::now() + Duration::seconds(30);
        let error = Error::RateLimited {
            kind: Source::ErpRest.as_str(),
            until,
        };

        let metric = failed_page_metric(&def, Source::ErpRest, &error).unwrap();

        assert_eq!(metric.upstream_path, "/v1/customer");
        assert_eq!(metric.status, 429);
        assert_eq!(metric.records, 0);
        assert_eq!(metric.page_size, Some(100));
        assert!(metric.rate_limit.retry_after_secs.unwrap() <= 30);
    }

    #[test]
    fn datamart_checkpoint_does_not_persist_page_cursor() {
        let tenant = TenantId::new("tenant-a");
        let key =
            CheckpointKey::default_partition(tenant, "accounts".into(), Source::DatamartGraphql);
        let def = EndpointDefinition {
            source: Source::DatamartGraphql,
            datamart: Some(crate::registry::DatamartBinding {
                ready: true,
                tenant_overrides: HashMap::new(),
                query_file: Some("config/queries/account.graphql".into()),
                type_name: Some("Account".into()),
                root_field: Some("accounts".into()),
                cursor_field: Some("updatedInDataMartDateTime".into()),
                id_field: Some("id".into()),
                company_filter: true,
                join_normalizer: Some("normalize_account".into()),
                fallback_source: Source::ErpRest,
            }),
            ..test_def("accounts")
        };
        let records = vec![json!({
            "id": "dm-99",
            "accountCD": "1500",
            "updatedInDataMartDateTime": "2026-05-01T10:00:00Z",
        })];

        let cp = derive_checkpoint(&key, &def, &records, 1, None, None);

        assert_eq!(cp.last_id, None);
        assert_eq!(
            cp.last_updated_in_dm,
            Some(
                DateTime::parse_from_rfc3339("2026-05-01T10:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc)
            )
        );
    }

    #[test]
    fn bounded_window_keeps_records_without_parseable_timestamp() {
        let def = test_def("organizations");
        let from = DateTime::parse_from_rfc3339("2026-04-27T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let to = DateTime::parse_from_rfc3339("2026-04-27T23:59:59Z")
            .unwrap()
            .with_timezone(&Utc);

        let records = vec![
            json!({
                "organizationNumber": "A",
                "lastModifiedDateTime": "2026-04-27T12:00:00Z",
            }),
            json!({
                "organizationNumber": "B",
            }),
            json!({
                "organizationNumber": "C",
                "lastModifiedDateTime": "not-a-timestamp",
            }),
        ];

        let filtered =
            filter_records_to_window(&records, Source::ErpRest, &def, Some(from), Some(to));

        assert_eq!(filtered.len(), 3);
        assert_eq!(filtered[0]["organizationNumber"], "A");
        assert_eq!(filtered[1]["organizationNumber"], "B");
        assert_eq!(filtered[2]["organizationNumber"], "C");
    }

    #[test]
    fn bounded_window_for_erp_rest_keeps_out_of_range_timestamps() {
        let def = test_def("organizations");
        let from = DateTime::parse_from_rfc3339("2026-04-27T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let to = DateTime::parse_from_rfc3339("2026-04-27T23:59:59Z")
            .unwrap()
            .with_timezone(&Utc);

        let records = vec![
            json!({
                "organizationNumber": "A",
                "lastModifiedDateTime": "2025-01-01T00:00:00Z",
            }),
            json!({
                "organizationNumber": "B",
                "lastModifiedDateTime": "2027-01-01T00:00:00Z",
            }),
        ];

        let filtered =
            filter_records_to_window(&records, Source::ErpRest, &def, Some(from), Some(to));

        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0]["organizationNumber"], "A");
        assert_eq!(filtered[1]["organizationNumber"], "B");
    }

    #[test]
    fn empty_window_caps_checkpoint_at_window_end() {
        let tenant = TenantId::new("tenant-a");
        let key = CheckpointKey::default_partition(tenant, "organizations".into(), Source::ErpRest);
        let window_end = DateTime::parse_from_rfc3339("2026-04-27T23:59:59Z")
            .unwrap()
            .with_timezone(&Utc);

        let cp = derive_checkpoint(
            &key,
            &test_def("organizations"),
            &[],
            0,
            None,
            Some(window_end),
        );

        assert_eq!(cp.last_modified_watermark, Some(window_end));
        assert_eq!(cp.records_seen, 0);
    }

    #[test]
    fn rejected_rows_are_counted_per_reason_most_common_first() {
        let issue = |reasons: &[&str]| crate::validation::ValidationIssue {
            index: 0,
            key: None,
            reasons: reasons.iter().map(|r| r.to_string()).collect(),
            record: json!({}),
        };
        let rejected = vec![
            issue(&["missing validation field b"]),
            issue(&["missing validation field a", "missing validation field b"]),
            issue(&["duplicate primary key with different payload"]),
        ];

        assert_eq!(
            reason_counts(&rejected),
            "missing validation field b: 2; duplicate primary key with different payload: 1; \
             missing validation field a: 1"
        );
    }

    #[test]
    fn a_fetch_is_a_complete_scan_only_when_nothing_narrowed_any_partition() {
        let at = utc_from("2026-10-01T00:00:00Z");

        assert!(is_complete_scan(None, None, [true].into_iter()));
        assert!(is_complete_scan(None, None, [true, true, true].into_iter()));
        assert!(
            !is_complete_scan(None, None, [true, false].into_iter()),
            "one partition resumed from a checkpoint"
        );
        assert!(
            !is_complete_scan(Some(at), None, [true].into_iter()),
            "a window"
        );
        assert!(
            !is_complete_scan(None, Some(at), [true].into_iter()),
            "a window"
        );
        assert!(
            !is_complete_scan(None, None, std::iter::empty()),
            "no partitions: an empty listing is not proof that nothing exists"
        );
    }

    fn utc_from(text: &str) -> DateTime<Utc> {
        text.parse().expect("RFC3339 timestamp")
    }

    fn sink_run(def: &EndpointDefinition, source: Source) -> RunWrite {
        RunWrite::new(
            Uuid::new_v4(),
            &TenantId::new("t1"),
            &def.endpoint_name,
            source,
            def,
            utc_from("2026-10-14T10:00:00Z"),
        )
        .expect("a run's spool files")
    }

    fn partition_sink<'a>(
        run: &'a mut RunWrite,
        def: &'a EndpointDefinition,
        partition_key: &'a str,
    ) -> PartitionSink<'a> {
        PartitionSink {
            run,
            def,
            partition_key,
            date_from: None,
            date_to: None,
            unchecked: false,
            rows_in: 0,
            newest: None,
        }
    }

    #[test]
    fn a_page_is_stamped_checked_and_spooled_as_it_arrives() {
        let def = test_def("accounts");
        let mut run = sink_run(&def, Source::ErpRest);
        let mut sink = partition_sink(&mut run, &def, vsv2_state::DEFAULT_PARTITION_KEY);

        sink.accept(vec![
            json!({"accountCD": "1500", "lastModifiedDateTime": "2026-01-02T10:00:00Z"}),
            json!({"accountCD": "1600", "lastModifiedDateTime": "2026-01-03T10:00:00Z"}),
        ])
        .expect("page");

        assert_eq!(sink.rows_in, 2);
        assert_eq!(sink.newest, Some(utc_from("2026-01-03T10:00:00Z")));
        assert_eq!(run.rows.rows(), 2);
        assert_eq!(run.rejected, 0);
        let spooled = run.rows.read_all().expect("the spool reads back");
        assert_eq!(spooled[0]["tenant_id"], "t1");
        assert_eq!(spooled[0]["_source"], Source::ErpRest.to_string());
        assert_eq!(spooled[0]["accountCD"], "1500");
    }

    #[test]
    fn a_rejected_record_is_quarantined_and_counted_but_not_spooled() {
        let def = test_def("accounts");
        let mut run = sink_run(&def, Source::ErpRest);
        let mut sink = partition_sink(&mut run, &def, vsv2_state::DEFAULT_PARTITION_KEY);

        sink.accept(vec![
            json!({"accountCD": "1500"}),
            json!({"description": "no key"}),
        ])
        .expect("page");

        assert_eq!(run.rows.rows(), 1);
        assert_eq!(run.rejected, 1);
        assert!(run.rejection_message().contains("1 rejected row(s)"));
        assert_eq!(run.quarantine.read_all().expect("read").len(), 1);
    }

    #[test]
    fn rows_a_normalizer_reshapes_are_spooled_without_a_check_here() {
        let def = test_def("accounts");
        let mut run = sink_run(&def, Source::DatamartGraphql);
        let mut sink = PartitionSink {
            unchecked: true,
            ..partition_sink(&mut run, &def, vsv2_state::DEFAULT_PARTITION_KEY)
        };

        sink.accept(vec![
            json!({"accountCD": "1500"}),
            json!({"accountCD": "1500"}),
        ])
        .expect("page");

        assert_eq!(run.rows.rows(), 2, "the loader checks the reshaped rows");
        assert_eq!(run.rejected, 0);
    }

    #[test]
    fn a_datamart_record_before_the_window_is_counted_and_not_spooled() {
        let def = test_def("accounts");
        let mut run = sink_run(&def, Source::DatamartGraphql);
        let mut sink = PartitionSink {
            date_from: Some(utc_from("2026-01-01T00:00:00Z")),
            unchecked: true,
            ..partition_sink(&mut run, &def, vsv2_state::DEFAULT_PARTITION_KEY)
        };

        sink.accept(vec![
            json!({"accountCD": "1", "updatedInDataMartDateTime": "2025-12-31T23:00:00Z"}),
            json!({"accountCD": "2", "updatedInDataMartDateTime": "2026-01-02T10:00:00Z"}),
        ])
        .expect("page");

        assert_eq!(sink.rows_in, 2);
        assert_eq!(sink.newest, Some(utc_from("2026-01-02T10:00:00Z")));
        assert_eq!((run.out_of_window, run.rows.rows()), (1, 1));
    }

    #[test]
    fn a_partition_watermark_counts_only_the_rows_of_that_partition() {
        let def = test_def("accounts");
        let mut run = sink_run(&def, Source::ErpRest);
        let row = json!({"accountCD": "1500", "lastModifiedDateTime": "2026-01-02T10:00:00Z"});

        let mut other = partition_sink(&mut run, &def, "ledger=OTHER");
        other.accept(vec![row.clone()]).expect("page");
        assert_eq!(
            other.newest, None,
            "another partition's row moves nothing here"
        );

        // A different account: the run's checker would drop an identical row as a duplicate.
        let own_row = json!({"accountCD": "1600", "lastModifiedDateTime": "2026-01-02T10:00:00Z"});
        let mut own = partition_sink(&mut run, &def, vsv2_state::DEFAULT_PARTITION_KEY);
        own.accept(vec![own_row]).expect("page");
        assert_eq!(own.newest, Some(utc_from("2026-01-02T10:00:00Z")));
    }
}
