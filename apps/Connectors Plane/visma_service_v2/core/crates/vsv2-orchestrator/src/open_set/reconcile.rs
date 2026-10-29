//! One reconciliation run, written against ports so the tests run it against fakes.
//!
//! [`reconcile`] does the work and always returns a report. [`run_if_due`] wraps it with the
//! cadence and the record of the run, and never returns an error either: whatever goes wrong here
//! is in the report and the log, and the normal sync that called it carries on.

use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use tracing::{info, warn};
use uuid::Uuid;
use vsv2_common::text::printable;
use vsv2_common::{Error, PageFetchMetric, Result, Source, TenantId};

use super::{
    diff_open_sets, distinct, is_due, is_from_the_future, key_fields, DocKey, OpenDoc,
    OpenSetReport, RunOutcome, RunState, RUN_TIMEOUT, SYSTEMIC_NOT_FOUND,
};
use crate::executor::{enrich, quarantine_payloads};
use crate::registry::EndpointDefinition;
use crate::validation::validate_records;

/// Longest error text kept in a report.
const MAX_ERROR_CHARS: usize = 600;

/// What Visma offers a reconciliation. Every request goes through the rate limiter.
#[async_trait]
pub trait OpenSetSource: Send + Sync {
    /// Every document Visma lists as open for the tenant: its key and the balance of its row.
    ///
    /// An error when the list cannot be trusted (a row that is not open, a walk that lost rows),
    /// never a short list: a short list would make staging's open documents look closed.
    async fn list_open(&self, tenant: &TenantId, def: &EndpointDefinition) -> Result<OpenList>;

    /// One document by key, shaped like a row of the list. `None` when Visma answers 404.
    async fn fetch_by_key(
        &self,
        tenant: &TenantId,
        def: &EndpointDefinition,
        key: &DocKey,
    ) -> Result<Option<Value>>;
}

/// Visma's open list.
#[derive(Debug, Clone, Default)]
pub struct OpenList {
    pub docs: Vec<OpenDoc>,
    /// One metric per list request.
    pub page_metrics: Vec<PageFetchMetric>,
}

/// The open documents of a tenant's endpoint in staging: their keys and staged balances.
#[derive(Debug, Clone, Default)]
pub struct StagedOpen {
    pub docs: Vec<OpenDoc>,
    /// Bytes BigQuery processed to find them, when it said.
    pub bytes_processed: Option<u64>,
}

/// Staging, the BigQuery side.
#[async_trait]
pub trait StagingPort: Send + Sync {
    /// The documents staging holds as open for the tenant (live rows only), each with the
    /// balance staging holds for it.
    async fn open_keys(&self, tenant: &TenantId, def: &EndpointDefinition) -> Result<StagedOpen>;

    /// Upsert `records` through the normal write path; the number of rows that changed.
    async fn write(
        &self,
        tenant: &TenantId,
        def: &EndpointDefinition,
        run_id: Uuid,
        records: Vec<Value>,
    ) -> Result<u64>;

    /// Keep documents that did not validate where the normal path keeps them.
    async fn quarantine(&self, payloads: Vec<Value>) -> Result<()>;
}

/// Where a run is recorded and when it last ran, the state database side.
#[async_trait]
pub trait RunLog: Send + Sync {
    /// When the last attempt for the tenant and endpoint ended, and how.
    async fn last_state(
        &self,
        tenant: &TenantId,
        def: &EndpointDefinition,
    ) -> Result<Option<RunState>>;

    /// Record that a run started.
    async fn start(&self, tenant: &TenantId, def: &EndpointDefinition, run_id: Uuid) -> Result<()>;

    /// Record how it ended, and set the cadence to `at`, the time the attempt began.
    async fn finish(
        &self,
        tenant: &TenantId,
        def: &EndpointDefinition,
        run_id: Uuid,
        report: &OpenSetReport,
        at: DateTime<Utc>,
    ) -> Result<()>;
}

/// The three sides of a run.
#[derive(Clone, Copy)]
pub struct Ports<'a> {
    pub visma: &'a dyn OpenSetSource,
    pub staging: &'a dyn StagingPort,
    pub log: &'a dyn RunLog,
}

/// How much one run may do.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Most documents fetched by key in one run. The rest wait for the next run. Staging and
    /// Visma should differ by a few dozen payments a day; a difference of thousands means one of
    /// the two answers is wrong, and this keeps that from becoming thousands of requests.
    pub max_fetches: usize,
    /// Fetches in a row that may fail before the run stops asking. Every failed request already
    /// went through the client's own retries, so a run of them says Visma is down.
    pub max_consecutive_failures: usize,
    /// Documents fetched before they are written. A run that is stopped (the timeout, a crash)
    /// keeps what earlier batches wrote, and the next run's diff no longer has those documents.
    pub batch_size: usize,
    /// The longest a whole run may take: [`RUN_TIMEOUT`], with the reason it is short. The
    /// tenant's lock is held for that long, and a long run continues in the next.
    pub timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_fetches: 500,
            max_consecutive_failures: 3,
            batch_size: 100,
            timeout: RUN_TIMEOUT,
        }
    }
}

/// Whether the process is shutting down. A run that is told to stop asks for no more documents
/// and keeps what it has.
pub type StopCheck<'a> = &'a (dyn Fn() -> bool + Sync);

fn describe(error: &Error) -> String {
    printable(&error.to_string(), MAX_ERROR_CHARS)
}

/// Run the reconciliation of one tenant and endpoint, with no limit on its time. Never fails: a
/// failure is the report. [`run_if_due`] is what the scheduler calls.
///
/// Staging is read first, because that costs no Visma request, then Visma's open list; only the
/// documents the two disagree about are fetched, in a fixed order (stale in staging first), and
/// written a batch at a time.
pub async fn reconcile(
    ports: &Ports<'_>,
    tenant: &TenantId,
    def: &EndpointDefinition,
    run_id: Uuid,
    limits: &Limits,
    stop: StopCheck<'_>,
) -> OpenSetReport {
    Run {
        ports,
        tenant,
        def,
        run_id,
        fields: key_fields(def),
        limits,
        stop,
    }
    .execute(None)
    .await
}

/// What one run needs besides its report.
struct Run<'a> {
    ports: &'a Ports<'a>,
    tenant: &'a TenantId,
    def: &'a EndpointDefinition,
    run_id: Uuid,
    fields: Vec<String>,
    limits: &'a Limits,
    stop: StopCheck<'a>,
}

/// Failed fetches in a row, across batches, and the last failure.
#[derive(Default)]
struct Streak {
    in_a_row: usize,
    last_failure: Option<String>,
}

impl Streak {
    fn failed(&mut self, why: String) {
        self.in_a_row += 1;
        self.last_failure = Some(why);
    }

    fn answered(&mut self) {
        self.in_a_row = 0;
    }
}

/// Why the run stopped asking before it had asked for everything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Halt {
    /// It did not: everything it meant to ask for was asked.
    No,
    /// Too many fetches failed in a row.
    Failures,
    /// The process is shutting down.
    Stop,
}

/// What one batch of by-key fetches gave.
struct Fetched {
    records: Vec<Value>,
    /// Keys of the batch that were asked for: fewer than the batch when the run halted in it.
    asked: usize,
    halt: Halt,
}

/// How a run that ended without an error of its own ended.
fn outcome_of(report: &OpenSetReport, halt: Halt) -> RunOutcome {
    if report.error.is_none() {
        return RunOutcome::Succeeded;
    }
    // Nothing the run asked for changed a row (every document was 404, did not validate, or came
    // back as it is already staged: the write changed no row) and no request failed: asking
    // again soon would get the same answers, and a diff that is a no-op for good would ask for
    // the same 500 documents every two hours. A shutdown, a failed request or progress (a row
    // that changed) all deserve a retry soon.
    let stalled = halt != Halt::Stop
        && report.by_key_requests > 0
        && report.written == 0
        && report.fetch_failed == 0;
    if stalled {
        RunOutcome::Stalled
    } else {
        RunOutcome::Partial
    }
}

impl Run<'_> {
    /// `reconcile`, with a limit on its time when `timeout` is given.
    ///
    /// What the run counted before a timeout stopped it stays in the report, with the outcome
    /// failed: work that earlier batches wrote is not forgotten.
    async fn execute(&self, timeout: Option<Duration>) -> OpenSetReport {
        let mut report = OpenSetReport::default();
        let result = match timeout {
            Some(limit) => tokio::time::timeout(limit, self.reconcile(&mut report))
                .await
                .unwrap_or_else(|_| {
                    Err(Error::Other(format!(
                        "no result after {} s, stopped",
                        limit.as_secs()
                    )))
                }),
            None => self.reconcile(&mut report).await,
        };
        if let Err(error) = result {
            report.outcome = RunOutcome::Failed;
            report.note(describe(&error));
        }
        report
    }

    async fn reconcile(&self, report: &mut OpenSetReport) -> Result<()> {
        if let Some(problem) = self.def.open_set_problem() {
            return Err(Error::Config(problem));
        }
        if self.shutting_down(report) {
            return Ok(());
        }

        let staged = self.ports.staging.open_keys(self.tenant, self.def).await?;
        report.staging_open = distinct(&staged.docs);
        report.bq_bytes = staged.bytes_processed;
        // The list is the first request to Visma: not worth making for a run that has to stop.
        if self.shutting_down(report) {
            return Ok(());
        }

        let listed = self.ports.visma.list_open(self.tenant, self.def).await?;
        report.visma_open = distinct(&listed.docs);
        report.list_requests = listed.page_metrics.len();
        report.list_metrics = listed.page_metrics;

        let diff = diff_open_sets(&staged.docs, &listed.docs);
        report.stale_in_staging = diff.stale_in_staging.len();
        report.missing_in_staging = diff.missing_in_staging.len();
        report.balance_differs = diff.balance_differs.len();

        // The queue of by-key fetches, cut at `max_fetches`: what staging holds open and Visma
        // does not list first, then what Visma lists open and staging does not hold, then the
        // part-paid documents, each in key order.
        //
        // A key Visma answers 404 for never leaves this queue by being written: that would be a
        // delete, which only the weekly complete scan does (it sets `deleted_at`, and the key tool
        // reads live rows only). Until then the key is in every run's diff and takes one of the
        // `max_fetches` places, asked again each time. Such keys can hold back the documents
        // behind them only when there are `max_fetches` or more of them in front: each takes one
        // place and the others still get the rest, so a queue with fewer of them keeps moving.
        // A run that asked for nothing but 404s is stalled and asks again after a day, so at the
        // worst that is `max_fetches` requests a day until the weekly pass. The queue is not
        // reordered for them: telling "gone" from "not yet fetched" takes the very request.
        let wanted: Vec<&DocKey> = diff
            .stale_in_staging
            .iter()
            .chain(&diff.missing_in_staging)
            .chain(&diff.balance_differs)
            .collect();
        let (asked, later) = wanted.split_at(wanted.len().min(self.limits.max_fetches));
        // Fetching can take minutes: this line says the comparison is done and what is to come.
        info!(
            tenant = %self.tenant,
            endpoint = %self.def.endpoint_name,
            staging_open = report.staging_open,
            visma_open = report.visma_open,
            stale_in_staging = report.stale_in_staging,
            missing_in_staging = report.missing_in_staging,
            balance_differs = report.balance_differs,
            to_fetch = asked.len(),
            over_the_cap = later.len(),
            "open-set: compared"
        );

        let halt = self.documents(asked, later.len(), report).await?;
        report.outcome = outcome_of(report, halt);
        Ok(())
    }

    /// Whether the run has to stop because the process is shutting down; says so in the report.
    fn shutting_down(&self, report: &mut OpenSetReport) -> bool {
        if !(self.stop)() {
            return false;
        }
        report.note("stopped for shutdown");
        report.outcome = RunOutcome::Partial;
        true
    }

    /// Fetch `asked` and write it, `limits.batch_size` documents at a time, and say in the report
    /// how it went. `over_the_cap` documents were left out before the first request.
    ///
    /// Stops asking when too many fetches fail in a row or the process is shutting down; what was
    /// not asked for is deferred. A write that fails ends the run, with the earlier batches
    /// already written; `deferred` and the notes are made up to there in that case too.
    async fn documents(
        &self,
        asked: &[&DocKey],
        over_the_cap: usize,
        report: &mut OpenSetReport,
    ) -> Result<Halt> {
        let mut streak = Streak::default();
        // Until a batch has been asked for, everything is still to do.
        report.deferred = over_the_cap + asked.len();
        let result = self
            .fetch_and_write(asked, over_the_cap, &mut streak, report)
            .await;
        if report.by_key_requests >= SYSTEMIC_NOT_FOUND
            && report.not_found == report.by_key_requests
        {
            report.note(format!(
                "every by-key answer ({}) was 404: is the path or the company wrong?",
                report.by_key_requests
            ));
        }
        if let Some(failure) = streak.last_failure {
            report.note(format!(
                "{} by-key fetch(es) failed, the last: {failure}",
                report.fetch_failed
            ));
        }
        if matches!(result, Ok(Halt::Stop)) {
            report.note("stopped for shutdown");
        }
        if report.deferred > 0 {
            report.note(format!(
                "{} document(s) left for the next run",
                report.deferred
            ));
        }
        result
    }

    async fn fetch_and_write(
        &self,
        asked: &[&DocKey],
        over_the_cap: usize,
        streak: &mut Streak,
        report: &mut OpenSetReport,
    ) -> Result<Halt> {
        let mut done = 0usize;
        for batch in asked.chunks(self.limits.batch_size.max(1)) {
            let fetched = self.fetch_batch(batch, streak, report).await;
            done += fetched.asked;
            // Counted before the write: a write that fails still leaves these as asked for.
            report.deferred = over_the_cap + asked.len() - done;
            self.write(fetched.records, report).await?;
            if fetched.halt != Halt::No {
                return Ok(fetched.halt);
            }
        }
        Ok(Halt::No)
    }

    /// Fetch `batch` one by one.
    ///
    /// A 404 is an answer (the document is gone); a failed request, or an answer for another
    /// document, is not. After `max_consecutive_failures` of those in a row the run halts, and it
    /// halts when the process is shutting down.
    async fn fetch_batch(
        &self,
        batch: &[&DocKey],
        streak: &mut Streak,
        report: &mut OpenSetReport,
    ) -> Fetched {
        let mut records = Vec::new();
        for (index, key) in batch.iter().enumerate() {
            if (self.stop)() {
                return Fetched {
                    records,
                    asked: index,
                    halt: Halt::Stop,
                };
            }
            report.by_key_requests += 1;
            let answer = self
                .ports
                .visma
                .fetch_by_key(self.tenant, self.def, key)
                .await
                .map_err(|error| describe(&error));
            match answer {
                Ok(Some(record)) => match DocKey::from_record(&self.fields, &record) {
                    Ok(got) if &got == *key => {
                        streak.answered();
                        records.push(record);
                    }
                    Ok(got) => {
                        report.fetch_failed += 1;
                        streak.failed(format!("the answer for {key} is the document {got}"));
                    }
                    Err(why) => {
                        report.fetch_failed += 1;
                        streak.failed(format!("the answer for {key}: {why}"));
                    }
                },
                Ok(None) => {
                    streak.answered();
                    report.not_found += 1;
                }
                Err(why) => {
                    report.fetch_failed += 1;
                    streak.failed(why);
                }
            }
            if streak.in_a_row >= self.limits.max_consecutive_failures.max(1) {
                return Fetched {
                    records,
                    asked: index + 1,
                    halt: Halt::Failures,
                };
            }
        }
        Fetched {
            records,
            asked: batch.len(),
            halt: Halt::No,
        }
    }

    /// Validate what was fetched and write what passes: quarantine the rest, as the normal path
    /// does, but without holding back the documents that are fine.
    async fn write(&self, records: Vec<Value>, report: &mut OpenSetReport) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        let now = Utc::now();
        let validation =
            validate_records(self.def, enrich(records, self.tenant, Source::ErpRest, now));
        if !validation.rejected.is_empty() {
            report.rejected += validation.rejected.len();
            let reasons = validation
                .rejected
                .first()
                .map(|issue| issue.reasons.join("; "))
                .unwrap_or_default();
            report.note(format!(
                "{} document(s) did not validate and were quarantined, the first: {reasons}",
                validation.rejected.len()
            ));
            let payloads = quarantine_payloads(
                self.run_id,
                self.tenant,
                &self.def.endpoint_name,
                Source::ErpRest,
                now,
                &validation.rejected,
            );
            if let Err(error) = self.ports.staging.quarantine(payloads).await {
                report.note(format!("quarantine write failed: {}", describe(&error)));
            }
        }
        report.fetched += validation.valid_records.len();
        if !validation.valid_records.is_empty() {
            report.written += self
                .ports
                .staging
                .write(self.tenant, self.def, self.run_id, validation.valid_records)
                .await?;
        }
        Ok(())
    }
}

/// Run the reconciliation of `endpoint` for `tenant` if it is flagged and due, and record it.
///
/// `None` when there was nothing to do: the endpoint is not flagged, the last run is too recent,
/// or the state database could not say (then the run waits for the next pass rather than going
/// ahead without a record). Otherwise the report, whatever came of the run. The run is stopped
/// after `limits.timeout`, and `stop` ends it between two requests.
pub async fn run_if_due(
    ports: &Ports<'_>,
    tenant: &TenantId,
    def: &EndpointDefinition,
    limits: &Limits,
    now: DateTime<Utc>,
    stop: StopCheck<'_>,
) -> Option<OpenSetReport> {
    if !def.open_set || !def.enabled {
        return None;
    }
    let last = match ports.log.last_state(tenant, def).await {
        Ok(last) => last,
        Err(error) => {
            warn!(
                tenant = %tenant, endpoint = %def.endpoint_name, err = %error,
                "open-set: could not read when it last ran, skipping this pass"
            );
            return None;
        }
    };
    if let Some(last) = &last {
        if is_from_the_future(last, now) {
            warn!(
                tenant = %tenant, endpoint = %def.endpoint_name, last_run = %last.at,
                "open-set: the last run is dated in the future (the clock moved back?), running now"
            );
        }
    }
    if !is_due(last, now) {
        return None;
    }
    let run_id = Uuid::new_v4();
    if let Err(error) = ports.log.start(tenant, def, run_id).await {
        warn!(
            tenant = %tenant, endpoint = %def.endpoint_name, err = %error,
            "open-set: could not record the start of a run, skipping this pass"
        );
        return None;
    }
    info!(tenant = %tenant, endpoint = %def.endpoint_name, %run_id, "open-set: start");

    let report = Run {
        ports,
        tenant,
        def,
        run_id,
        fields: key_fields(def),
        limits,
        stop,
    }
    .execute(Some(limits.timeout))
    .await;

    if let Err(error) = ports.log.finish(tenant, def, run_id, &report, now).await {
        warn!(
            tenant = %tenant, endpoint = %def.endpoint_name, %run_id, err = %error,
            "open-set: could not record the end of the run"
        );
    }
    log_report(tenant, def, run_id, &report);
    Some(report)
}

fn log_report(tenant: &TenantId, def: &EndpointDefinition, run_id: Uuid, report: &OpenSetReport) {
    macro_rules! say {
        ($level:ident) => {
            $level!(
                tenant = %tenant,
                endpoint = %def.endpoint_name,
                %run_id,
                outcome = report.outcome.as_str(),
                staging_open = report.staging_open,
                visma_open = report.visma_open,
                stale_in_staging = report.stale_in_staging,
                missing_in_staging = report.missing_in_staging,
                balance_differs = report.balance_differs,
                fetched = report.fetched,
                written = report.written,
                not_found = report.not_found,
                fetch_failed = report.fetch_failed,
                rejected = report.rejected,
                deferred = report.deferred,
                list_requests = report.list_requests,
                by_key_requests = report.by_key_requests,
                bq_bytes = report.bq_bytes,
                error = report.error.as_deref(),
                "open-set: finished"
            )
        };
    }
    match report.outcome {
        RunOutcome::Succeeded => say!(info),
        RunOutcome::Partial | RunOutcome::Stalled | RunOutcome::Failed => say!(warn),
    }
}

#[cfg(test)]
mod tests;
