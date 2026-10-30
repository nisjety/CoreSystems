//! The real ports: Visma over HTTP, staging through the BigQuery tools, the record in Postgres.
//!
//! Nothing here decides anything. [`SyncExecutor::reconcile_open_set_if_due`] is the one entry
//! point the scheduler calls.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use tokio::process::Command;
use tracing::{debug, warn};
use uuid::Uuid;
use vsv2_common::text::printable;
use vsv2_common::{Error, Result, Source, TenantId};
use vsv2_erp_rest::{paginate_with_metrics, BackgroundOpts, PageOpts};
use vsv2_state::{endpoint_flags, Checkpoint, CheckpointKey, RunStatus, StateStore, SyncRun};

use super::reconcile::{
    run_if_due, Limits, OpenList, OpenSetSource, Ports, RunLog, StagedOpen, StagingPort,
};
use super::{
    by_key_path, docs_from_open_rows, key_fields, list_path, parse_key_lines, query_pairs,
    run_label, DocKey, OpenSetReport, RunOutcome, RunState, OPEN_STATUS, STATE_PARTITION,
    STATUS_PARAM,
};
use crate::bq_writer::{BqWriter, MergeLineage, StageOutcome};
use crate::erp_adapter::ErpRestAdapter;
use crate::executor::{scoped_primary_key, sync_page_metrics, SyncExecutor};
use crate::registry::EndpointDefinition;

/// Rows per page of the open list. The list carries no `expand*` flags, so a page is light, and
/// 1,000 is what the API allows: a tenant with fewer open documents costs one request.
const LIST_PAGE_SIZE: u32 = 1000;
/// The BigQuery tool, on `PATH` next to `vsv2-bq-stage`.
const KEYS_COMMAND: &str = "vsv2-bq-open-keys";
/// Longest the key query may take; it reads one tenant's open rows of one table. The run's own
/// bound, `RUN_TIMEOUT`, ends it earlier: this one only matters if that is ever raised.
const KEYS_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Longest error text taken from a tool's stderr.
const MAX_STDERR_CHARS: usize = 600;

fn background_opts(def: &EndpointDefinition) -> BackgroundOpts {
    BackgroundOpts {
        mode: def.background_mode,
        poll_secs: def.background_poll_secs.unwrap_or(5),
        timeout_secs: def.background_timeout_secs.unwrap_or(900),
    }
}

/// One document out of a by-key answer: an object, or a list of exactly one.
fn single_document(body: Value, path: &str) -> Result<Value> {
    match body {
        Value::Object(_) => Ok(body),
        Value::Array(mut items) if items.len() == 1 && items[0].is_object() => Ok(items.remove(0)),
        other => Err(Error::Http(format!(
            "{path}: expected one document, got {}",
            printable(&other.to_string(), 96)
        ))),
    }
}

/// Visma's side. Both requests go through the client, so through its rate limiter.
#[async_trait]
impl OpenSetSource for ErpRestAdapter {
    async fn list_open(&self, tenant: &TenantId, def: &EndpointDefinition) -> Result<OpenList> {
        let path = list_path(def)?;
        let fields = key_fields(def);
        let opts = PageOpts {
            page_size: LIST_PAGE_SIZE,
            pagination_profile: def.pagination_profile,
            background: background_opts(def),
            default_query: HashMap::from([(
                STATUS_PARAM.to_string(),
                Value::String(OPEN_STATUS.to_string()),
            )]),
            key_fields: fields.clone(),
            ..Default::default()
        };
        let batch = paginate_with_metrics(&self.client, tenant, &path, &opts).await?;
        let docs = docs_from_open_rows(&fields, &batch.records)?;
        Ok(OpenList {
            docs,
            page_metrics: batch.page_metrics,
        })
    }

    async fn fetch_by_key(
        &self,
        tenant: &TenantId,
        def: &EndpointDefinition,
        key: &DocKey,
    ) -> Result<Option<Value>> {
        let path = by_key_path(def, key)?;
        let query = query_pairs(&def.default_query)?;
        match self
            .client
            .get_json(tenant, &path, &query, &background_opts(def))
            .await
        {
            Ok(body) => single_document(body, &path).map(Some),
            Err(Error::UpstreamStatus { status: 404, .. }) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

/// The last `max` characters of `text`, control characters made harmless.
///
/// For what a failed tool printed: a crash ends in the line that says why, and starts with a
/// traceback that says nothing.
fn tail(text: &str, max: usize) -> String {
    let text = text.trim();
    let skip = text.chars().count().saturating_sub(max);
    printable(&text.chars().skip(skip).collect::<String>(), max)
}

/// What `vsv2-bq-open-keys` said about its query, on the last line of stderr.
fn bytes_processed(stderr: &str) -> Option<u64> {
    stderr
        .lines()
        .rev()
        .filter_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
        .find_map(|summary| summary.get("bytes_processed").and_then(Value::as_u64))
}

/// What the open-set write needs from the stage writer: `BqWriter` in production, a recorder in
/// the tests, which is how they can see the arguments of the call. The one that matters is
/// `complete`: a batch of fetched documents is not everything Visma has, and a complete scan would
/// mark the tenant's other rows deleted.
#[async_trait]
trait StageWrite: Send + Sync {
    async fn upsert(
        &self,
        table: &str,
        pk: &str,
        records: &[Value],
        lineage: MergeLineage<'_>,
        tenant: &str,
    ) -> Result<StageOutcome>;

    async fn quarantine_rows(&self, records: &[Value]) -> Result<u64>;
}

#[async_trait]
impl StageWrite for BqWriter {
    async fn upsert(
        &self,
        table: &str,
        pk: &str,
        records: &[Value],
        lineage: MergeLineage<'_>,
        tenant: &str,
    ) -> Result<StageOutcome> {
        // Never a complete scan: see `StageWrite`.
        self.stage(table, pk, records, lineage, tenant, false).await
    }

    async fn quarantine_rows(&self, records: &[Value]) -> Result<u64> {
        self.write_quarantine(records).await
    }
}

/// Staging in BigQuery: reads through `vsv2-bq-open-keys`, writes through the stage writer.
struct BqStaging<'a> {
    bq: &'a BqWriter,
    stager: &'a dyn StageWrite,
    /// The key tool: [`KEYS_COMMAND`] on `PATH`, a script of its own in the tests.
    keys_command: &'a str,
}

#[async_trait]
impl StagingPort for BqStaging<'_> {
    async fn open_keys(&self, tenant: &TenantId, def: &EndpointDefinition) -> Result<StagedOpen> {
        let dataset =
            self.bq.dataset_staging.as_deref().ok_or_else(|| {
                Error::Config("VSV2_BIGQUERY_DATASET_STAGING is not set".to_string())
            })?;
        let fields = key_fields(def);
        let command = self.keys_command;
        let output = tokio::time::timeout(
            KEYS_TIMEOUT,
            Command::new(command)
                .args(["--project", &self.bq.project, "--dataset", dataset])
                .args(["--table", &def.endpoint_name, "--tenant", tenant.as_str()])
                .args(["--pk", &fields.join(",")])
                .kill_on_drop(true)
                .output(),
        )
        .await
        .map_err(|_| {
            Error::Other(format!(
                "{command}: no result after {} s, killed",
                KEYS_TIMEOUT.as_secs()
            ))
        })?
        .map_err(|e| Error::Other(format!("{command}: spawn failed: {e}")))?;

        let stderr = String::from_utf8_lossy(&output.stderr);
        if !output.status.success() {
            return Err(Error::Other(format!(
                "{command} exited non-zero: {}",
                tail(&stderr, MAX_STDERR_CHARS)
            )));
        }
        let docs = parse_key_lines(&String::from_utf8_lossy(&output.stdout), &fields)?;
        Ok(StagedOpen {
            docs,
            bytes_processed: bytes_processed(&stderr),
        })
    }

    async fn write(
        &self,
        tenant: &TenantId,
        def: &EndpointDefinition,
        run_id: Uuid,
        records: Vec<Value>,
    ) -> Result<u64> {
        let run_id = run_id.to_string();
        // The lineage names the endpoint itself: it is the staging table, and the name the
        // heartbeat of the write is recorded under.
        let lineage = MergeLineage {
            run_id: &run_id,
            endpoint: &def.endpoint_name,
            source: Source::ErpRest.as_str(),
        };
        let outcome = self
            .stager
            .upsert(
                &def.endpoint_name,
                &scoped_primary_key(def),
                &records,
                lineage,
                tenant.as_str(),
            )
            .await?;
        Ok(outcome.changed)
    }

    async fn quarantine(&self, payloads: Vec<Value>) -> Result<()> {
        self.stager.quarantine_rows(&payloads).await.map(|_| ())
    }
}

/// The checkpoint row that keeps when a tenant's endpoint last ran. See [`STATE_PARTITION`].
fn state_key(tenant: &TenantId, def: &EndpointDefinition) -> CheckpointKey {
    CheckpointKey::for_partition(
        tenant.clone(),
        def.endpoint_name.clone(),
        Source::ErpRest,
        STATE_PARTITION.to_string(),
    )
}

fn run_status(outcome: RunOutcome) -> RunStatus {
    match outcome {
        RunOutcome::Succeeded => RunStatus::Succeeded,
        RunOutcome::Partial | RunOutcome::Stalled => RunStatus::PartialSuccess,
        RunOutcome::Failed => RunStatus::Failed,
    }
}

/// The record of runs: one `sync_run` row per run under `<endpoint>:open_set` (`records_in` the
/// documents that differed, `records_written` the rows the write changed), the list requests as
/// `sync_page_metric` rows, and the cadence row. None of it touches the normal sync's checkpoints.
///
/// Public so that the integration tests can run it against a real database
/// (`tests/open_set_state.rs`); the scheduler reaches it through
/// [`SyncExecutor::reconcile_open_set_if_due`].
pub struct PgRunLog<'a> {
    state: &'a StateStore,
}

impl<'a> PgRunLog<'a> {
    pub fn new(state: &'a StateStore) -> Self {
        Self { state }
    }
}

#[async_trait]
impl RunLog for PgRunLog<'_> {
    async fn last_state(
        &self,
        tenant: &TenantId,
        def: &EndpointDefinition,
    ) -> Result<Option<RunState>> {
        let checkpoint = self.state.get_checkpoint(&state_key(tenant, def)).await?;
        Ok(checkpoint.and_then(|row| {
            row.last_sync_at.map(|at| RunState {
                at,
                outcome: RunOutcome::from_marker(row.last_id.as_deref()),
            })
        }))
    }

    async fn start(&self, tenant: &TenantId, def: &EndpointDefinition, run_id: Uuid) -> Result<()> {
        self.state
            .insert_run(&SyncRun {
                run_id,
                tenant: tenant.clone(),
                endpoint: run_label(&def.endpoint_name),
                source: Source::ErpRest,
                status: RunStatus::Running,
                started_at: Utc::now(),
                finished_at: None,
                records_in: 0,
                records_written: 0,
                error: None,
            })
            .await
    }

    async fn finish(
        &self,
        tenant: &TenantId,
        def: &EndpointDefinition,
        run_id: Uuid,
        report: &OpenSetReport,
        at: DateTime<Utc>,
    ) -> Result<()> {
        let key = state_key(tenant, def);
        let metrics = sync_page_metrics(
            run_id,
            tenant,
            &run_label(&def.endpoint_name),
            &key,
            report.list_metrics.clone(),
        );
        if let Err(error) = self.state.insert_page_metrics(&metrics).await {
            warn!(
                tenant = %tenant, endpoint = %def.endpoint_name, err = %error,
                "open-set: could not record the list requests"
            );
        }
        let run = self
            .state
            .finish_run(
                run_id,
                run_status(report.outcome),
                report.differences() as i64,
                report.written as i64,
                report.error.as_deref(),
            )
            .await;
        // The cadence is set even when the run row could not be closed: a run that was made must
        // not be made again at the next pass.
        let cadence = self
            .state
            .upsert_checkpoint(&Checkpoint {
                key,
                last_modified_watermark: None,
                last_updated_in_dm: None,
                last_id: Some(report.outcome.as_str().to_string()),
                last_sync_at: Some(at),
                records_seen: 0,
            })
            .await;
        run.and(cadence)
    }
}

impl SyncExecutor {
    /// After an endpoint's hourly sync: reconcile its open documents against Visma if the
    /// endpoint has `open_set` and the last run is old enough (see `crate::open_set`).
    ///
    /// Never fails and never blocks the sync it follows for longer than the run's own timeout:
    /// whatever goes wrong is in the log and in the returned report. `stop` is the scheduler's
    /// shutdown flag: a run that sees it set asks for no more documents. `None` when there was
    /// nothing to do, which includes a deployment without BigQuery staging (the reconciliation
    /// compares against staging) and an endpoint that a tenant reads from DataMart.
    pub async fn reconcile_open_set_if_due(
        &self,
        tenant: &TenantId,
        endpoint_name: &str,
        stop: &AtomicBool,
    ) -> Option<OpenSetReport> {
        let def = self.registry.get(endpoint_name)?;
        if !def.open_set || !def.enabled {
            return None;
        }
        let Some(bq) = self.bq.as_ref().filter(|bq| bq.stages()) else {
            debug!(
                tenant = %tenant, endpoint = endpoint_name,
                "open-set: skipped, it compares against BigQuery staging and none is configured"
            );
            return None;
        };
        let visma = self.open_set_visma(tenant, def).await?;
        let staging = BqStaging {
            bq,
            stager: bq,
            keys_command: KEYS_COMMAND,
        };
        let log = PgRunLog::new(&self.state);
        let ports = Ports {
            visma,
            staging: &staging,
            log: &log,
        };
        let stopping = || stop.load(Ordering::SeqCst);
        run_if_due(
            &ports,
            tenant,
            def,
            &Limits::default(),
            Utc::now(),
            &stopping,
        )
        .await
    }

    /// Visma's side of the reconciliation, from the adapter this tenant's endpoint syncs with.
    async fn open_set_visma(
        &self,
        tenant: &TenantId,
        def: &EndpointDefinition,
    ) -> Option<&dyn OpenSetSource> {
        let endpoint = def.endpoint_name.as_str();
        let requested = match endpoint_flags::get_flag(&self.state.pg, tenant, endpoint).await {
            Ok(flag) => flag.unwrap_or_else(|| def.effective_source(tenant)),
            Err(error) => {
                warn!(tenant = %tenant, endpoint, err = %error, "open-set: could not read the source flag, skipping this pass");
                return None;
            }
        };
        let source = match def.live_source(requested) {
            Ok(source) => source,
            Err(error) => {
                warn!(tenant = %tenant, endpoint, err = %error, "open-set: no usable source, skipping this pass");
                return None;
            }
        };
        if source != Source::ErpRest {
            debug!(tenant = %tenant, endpoint, %source, "open-set: skipped, it reads Visma's ERP REST API");
            return None;
        }
        let visma = self
            .dispatch
            .pick_source(source)
            .ok()
            .and_then(|adapter| adapter.open_set_source());
        if visma.is_none() {
            warn!(tenant = %tenant, endpoint, "open-set: the ERP adapter cannot list open documents");
        }
        visma
    }
}

#[cfg(test)]
mod tests;
