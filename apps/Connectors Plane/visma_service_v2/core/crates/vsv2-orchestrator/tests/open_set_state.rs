//! The record of an open-set run, against a real database.
//!
//! `PgRunLog` writes the `sync_run` row of a run, the `sync_page_metric` rows of its list
//! requests and the cadence row (`vsv2.sync_checkpoint`, partition `_open_set`) that decides when
//! the next run is due. The unit tests run the reconciliation against fakes of all three ports;
//! this file runs the Postgres one for real: what it writes, what it reads back, and that the
//! normal sync's checkpoint is left alone.
//!
//! Requires the local compose Postgres (`make up`); see `sync_semantics.rs`.

mod common;

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use common::fake_adapter::FakeAdapter;
use common::test_db::TestDb;
use common::{erp_def, snapshot, utc, Harness, TENANT_A, TENANT_B};
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;
use vsv2_common::{
    Error, PageFetchMetric, PrimaryKey, RateLimitSnapshot, Result, Source, TenantId,
};
use vsv2_orchestrator::open_set::{
    is_due, run_if_due, DocKey, Limits, OpenDoc, OpenList, OpenSetReport, OpenSetSource, PgRunLog,
    Ports, RunLog, RunOutcome, RunState, StagedOpen, StagingPort, INTERVAL, RETRY_AFTER,
    STATE_PARTITION,
};
use vsv2_orchestrator::{BqWriter, EndpointDefinition};
use vsv2_state::{Checkpoint, CheckpointKey, DEFAULT_PARTITION_KEY};

const ENDPOINT: &str = "customer_invoices";
const RUN_LABEL: &str = "customer_invoices:open_set";

/// An endpoint the reconciliation accepts: flagged, tier 1, ERP REST, keyed on one top-level field.
fn invoice_def() -> EndpointDefinition {
    EndpointDefinition {
        primary_key: PrimaryKey::Single("referenceNumber".into()),
        api_endpoint: Some("/v1/customerinvoice".into()),
        validation_fields: Some(vec!["referenceNumber".into(), "status".into()]),
        tier: Some(1),
        open_set: true,
        ..erp_def(ENDPOINT)
    }
}

fn tenant() -> TenantId {
    TenantId::new(TENANT_A)
}

fn cadence_key(tenant: &TenantId) -> CheckpointKey {
    CheckpointKey::for_partition(
        tenant.clone(),
        ENDPOINT.to_string(),
        Source::ErpRest,
        STATE_PARTITION.to_string(),
    )
}

fn normal_key(tenant: &TenantId) -> CheckpointKey {
    CheckpointKey::default_partition(tenant.clone(), ENDPOINT.to_string(), Source::ErpRest)
}

fn list_metric(page: i64) -> PageFetchMetric {
    PageFetchMetric {
        upstream_path: "/v1/customerinvoice".into(),
        page_param: Some("pageNumber".into()),
        page_value: Some(page),
        page_size: Some(1000),
        status: 200,
        elapsed_ms: 12,
        records: 3,
        total_count: Some(3),
        has_next: None,
        rate_limit: RateLimitSnapshot::default(),
        background_mode: None,
    }
}

/// A run that ended partial: what the log has to keep of it.
fn partial_report() -> OpenSetReport {
    OpenSetReport {
        outcome: RunOutcome::Partial,
        staging_open: 931,
        visma_open: 930,
        stale_in_staging: 2,
        missing_in_staging: 1,
        balance_differs: 1,
        fetched: 4,
        written: 3,
        deferred: 1,
        list_requests: 2,
        by_key_requests: 4,
        bq_bytes: Some(1_700_000),
        list_metrics: vec![list_metric(1), list_metric(2)],
        error: Some("1 document(s) left for the next run".into()),
        ..OpenSetReport::default()
    }
}

fn report_with(outcome: RunOutcome) -> OpenSetReport {
    OpenSetReport {
        outcome,
        ..OpenSetReport::default()
    }
}

/// The `sync_run` row of `run_id`.
#[derive(Debug)]
struct RunRow {
    tenant_id: String,
    endpoint: String,
    source: String,
    status: String,
    records_in: i64,
    records_written: i64,
    error: Option<String>,
    finished: bool,
}

async fn run_row(db: &TestDb, run_id: Uuid) -> RunRow {
    let row: (
        String,
        String,
        String,
        String,
        i64,
        i64,
        Option<String>,
        bool,
    ) = sqlx::query_as(
        "SELECT tenant_id, endpoint, source, status, records_in, records_written, error, \
                finished_at IS NOT NULL \
         FROM vsv2.sync_run WHERE run_id = $1",
    )
    .bind(run_id)
    .fetch_one(&db.state.pg)
    .await
    .expect("read the run row");
    RunRow {
        tenant_id: row.0,
        endpoint: row.1,
        source: row.2,
        status: row.3,
        records_in: row.4,
        records_written: row.5,
        error: row.6,
        finished: row.7,
    }
}

/// A `sync_page_metric` row, as far as these tests look at it.
#[derive(Debug, PartialEq)]
struct MetricRow {
    tenant_id: String,
    endpoint: String,
    partition_key: String,
    source: String,
    upstream_path: String,
    page_value: i64,
    status: i32,
    records: i64,
    total_count: Option<i64>,
}

async fn count(db: &TestDb, sql: &str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(&db.state.pg)
        .await
        .expect("count rows")
}

/// A whole run recorded by hand: started, then finished at `at` with `report`.
async fn record(
    log: &PgRunLog<'_>,
    tenant: &TenantId,
    report: &OpenSetReport,
    at: DateTime<Utc>,
) -> Uuid {
    let run_id = Uuid::new_v4();
    log.start(tenant, &invoice_def(), run_id)
        .await
        .expect("start the run");
    log.finish(tenant, &invoice_def(), run_id, report, at)
        .await
        .expect("finish the run");
    run_id
}

// start and finish -----------------------------------------------------------------------------

#[tokio::test]
async fn starting_a_run_records_it_as_running_under_the_open_set_name() {
    let db = TestDb::create().await;
    let log = PgRunLog::new(&db.state);
    let run_id = Uuid::new_v4();

    log.start(&tenant(), &invoice_def(), run_id).await.unwrap();

    let row = run_row(&db, run_id).await;
    assert_eq!(
        row.endpoint, RUN_LABEL,
        "not the endpoint's own name: that is the sync's"
    );
    assert_eq!(row.status, "running");
    assert_eq!(row.tenant_id, TENANT_A);
    assert_eq!(row.source, "erp_rest");
    assert_eq!((row.records_in, row.records_written), (0, 0));
    assert!(!row.finished);
    assert_eq!(row.error, None);
    assert!(
        db.state
            .get_checkpoint(&cadence_key(&tenant()))
            .await
            .unwrap()
            .is_none(),
        "a run that never finishes (the process dies) leaves no cadence, so it is made again"
    );
}

#[tokio::test]
async fn finishing_partial_gives_partial_success_with_its_counts_and_the_list_requests() {
    let db = TestDb::create().await;
    let log = PgRunLog::new(&db.state);

    let run_id = record(
        &log,
        &tenant(),
        &partial_report(),
        utc("2026-10-12T10:00:00Z"),
    )
    .await;

    let row = run_row(&db, run_id).await;
    assert_eq!(row.status, "partial_success");
    assert_eq!(
        row.records_in, 4,
        "documents that differed: 2 stale, 1 missing, 1 balance"
    );
    assert_eq!(row.records_written, 3);
    assert_eq!(
        row.error.as_deref(),
        Some("1 document(s) left for the next run")
    );
    assert!(row.finished, "finished_at is set");
    assert_eq!(row.endpoint, RUN_LABEL, "finishing does not rename it");

    let metrics: Vec<MetricRow> = sqlx::query(
        "SELECT tenant_id, endpoint, partition_key, source, upstream_path, page_value, status, \
                records, total_count \
         FROM vsv2.sync_page_metric WHERE run_id = $1 ORDER BY page_value",
    )
    .bind(run_id)
    .fetch_all(&db.state.pg)
    .await
    .unwrap()
    .iter()
    .map(|row| MetricRow {
        tenant_id: row.get("tenant_id"),
        endpoint: row.get("endpoint"),
        partition_key: row.get("partition_key"),
        source: row.get("source"),
        upstream_path: row.get("upstream_path"),
        page_value: row.get("page_value"),
        status: row.get("status"),
        records: row.get("records"),
        total_count: row.get("total_count"),
    })
    .collect();
    let expected = |page| MetricRow {
        tenant_id: TENANT_A.into(),
        endpoint: RUN_LABEL.into(),
        partition_key: STATE_PARTITION.into(),
        source: "erp_rest".into(),
        upstream_path: "/v1/customerinvoice".into(),
        page_value: page,
        status: 200,
        records: 3,
        total_count: Some(3),
    };
    assert_eq!(
        metrics,
        vec![expected(1), expected(2)],
        "one row per list request"
    );
}

#[tokio::test]
async fn every_outcome_has_its_run_status() {
    let db = TestDb::create().await;
    let log = PgRunLog::new(&db.state);
    for (outcome, status) in [
        (RunOutcome::Succeeded, "succeeded"),
        (RunOutcome::Partial, "partial_success"),
        (RunOutcome::Stalled, "partial_success"),
        (RunOutcome::Failed, "failed"),
    ] {
        let mut report = report_with(outcome);
        report.error = (outcome != RunOutcome::Succeeded).then(|| "what went wrong".to_string());

        let run_id = record(&log, &tenant(), &report, utc("2026-10-12T10:00:00Z")).await;

        let row = run_row(&db, run_id).await;
        assert_eq!(row.status, status, "{outcome:?}");
        assert_eq!(row.error, report.error, "{outcome:?}");
    }
}

#[tokio::test]
async fn a_report_without_list_requests_adds_no_metric_rows() {
    let db = TestDb::create().await;
    let log = PgRunLog::new(&db.state);

    record(
        &log,
        &tenant(),
        &report_with(RunOutcome::Failed),
        utc("2026-10-12T10:00:00Z"),
    )
    .await;

    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM vsv2.sync_page_metric").await,
        0
    );
}

// the cadence row ------------------------------------------------------------------------------

#[tokio::test]
async fn the_cadence_row_is_written_with_no_watermark_and_nothing_seen() {
    let db = TestDb::create().await;
    let log = PgRunLog::new(&db.state);
    let at = utc("2026-10-12T10:00:00Z");

    record(&log, &tenant(), &partial_report(), at).await;

    let row = db
        .state
        .get_checkpoint(&cadence_key(&tenant()))
        .await
        .unwrap()
        .expect("the cadence row");
    assert_eq!(
        row.last_modified_watermark, None,
        "never read as a watermark"
    );
    assert_eq!(row.last_updated_in_dm, None);
    assert_eq!(row.last_id.as_deref(), Some("partial"), "how the run ended");
    assert_eq!(row.last_sync_at, Some(at), "when the attempt began");
    assert_eq!(
        row.records_seen, 0,
        "so sums over an endpoint's checkpoints are unchanged"
    );
    let stored: (String, Option<DateTime<Utc>>, Option<DateTime<Utc>>, i64) = sqlx::query_as(
        "SELECT partition_key, last_modified_watermark, last_updated_in_dm, records_seen \
         FROM vsv2.sync_checkpoint WHERE endpoint = $1",
    )
    .bind(ENDPOINT)
    .fetch_one(&db.state.pg)
    .await
    .unwrap();
    assert_eq!(stored, ("_open_set".to_string(), None, None, 0));
}

#[tokio::test]
async fn a_second_run_overwrites_the_cadence_row_and_does_not_add_one() {
    let db = TestDb::create().await;
    let log = PgRunLog::new(&db.state);
    record(
        &log,
        &tenant(),
        &partial_report(),
        utc("2026-10-12T10:00:00Z"),
    )
    .await;
    let later = utc("2026-10-13T10:00:00Z");

    record(&log, &tenant(), &report_with(RunOutcome::Succeeded), later).await;

    let row = db
        .state
        .get_checkpoint(&cadence_key(&tenant()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.last_id.as_deref(), Some("succeeded"));
    assert_eq!(row.last_sync_at, Some(later));
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM vsv2.sync_checkpoint WHERE partition_key = '_open_set'"
        )
        .await,
        1
    );
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM vsv2.sync_run").await,
        2,
        "every run keeps its own row"
    );
}

#[tokio::test]
async fn the_last_state_is_what_the_last_finish_recorded() {
    let db = TestDb::create().await;
    let log = PgRunLog::new(&db.state);
    assert_eq!(
        log.last_state(&tenant(), &invoice_def()).await.unwrap(),
        None,
        "nothing recorded yet"
    );

    for (day, outcome) in [
        (10, RunOutcome::Succeeded),
        (11, RunOutcome::Partial),
        (12, RunOutcome::Stalled),
        (13, RunOutcome::Failed),
    ] {
        let at = utc(&format!("2026-10-{day}T10:00:00Z"));
        record(&log, &tenant(), &report_with(outcome), at).await;

        assert_eq!(
            log.last_state(&tenant(), &invoice_def()).await.unwrap(),
            Some(RunState { at, outcome }),
            "{outcome:?}"
        );
    }
}

#[tokio::test]
async fn the_cadence_of_one_tenant_is_not_the_cadence_of_another() {
    let db = TestDb::create().await;
    let log = PgRunLog::new(&db.state);
    let at = utc("2026-10-12T10:00:00Z");

    record(&log, &tenant(), &report_with(RunOutcome::Succeeded), at).await;

    let other = TenantId::new(TENANT_B);
    assert_eq!(log.last_state(&other, &invoice_def()).await.unwrap(), None);
    let mut other_endpoint = invoice_def();
    other_endpoint.endpoint_name = "supplier_invoices".into();
    assert_eq!(
        log.last_state(&tenant(), &other_endpoint).await.unwrap(),
        None
    );
}

#[tokio::test]
async fn a_row_without_a_time_is_no_run_at_all() {
    let db = TestDb::create().await;
    let log = PgRunLog::new(&db.state);
    db.state
        .upsert_checkpoint(&Checkpoint {
            key: cadence_key(&tenant()),
            last_modified_watermark: None,
            last_updated_in_dm: None,
            last_id: Some("failed".into()),
            last_sync_at: None,
            records_seen: 0,
        })
        .await
        .unwrap();

    assert_eq!(
        log.last_state(&tenant(), &invoice_def()).await.unwrap(),
        None
    );
}

#[tokio::test]
async fn a_marker_the_code_does_not_know_reads_as_a_failed_run_that_is_due_in_two_hours() {
    let db = TestDb::create().await;
    let log = PgRunLog::new(&db.state);
    let at = utc("2026-10-12T10:00:00Z");
    for marker in ["from_a_later_version", "SUCCEEDED", ""] {
        db.state
            .upsert_checkpoint(&Checkpoint {
                key: cadence_key(&tenant()),
                last_modified_watermark: None,
                last_updated_in_dm: None,
                last_id: Some(marker.to_string()),
                last_sync_at: Some(at),
                records_seen: 0,
            })
            .await
            .unwrap();

        let last = log.last_state(&tenant(), &invoice_def()).await.unwrap();

        assert_eq!(
            last,
            Some(RunState {
                at,
                outcome: RunOutcome::Failed
            }),
            "{marker:?}"
        );
        assert!(!is_due(last, at + Duration::minutes(119)), "{marker:?}");
        assert!(is_due(last, at + Duration::hours(2)), "{marker:?}");
    }
}

#[tokio::test]
async fn a_row_without_a_marker_reads_as_a_success() {
    let db = TestDb::create().await;
    let log = PgRunLog::new(&db.state);
    let at = utc("2026-10-12T10:00:00Z");
    db.state
        .upsert_checkpoint(&Checkpoint {
            key: cadence_key(&tenant()),
            last_modified_watermark: None,
            last_updated_in_dm: None,
            last_id: None,
            last_sync_at: Some(at),
            records_seen: 0,
        })
        .await
        .unwrap();

    assert_eq!(
        log.last_state(&tenant(), &invoice_def()).await.unwrap(),
        Some(RunState {
            at,
            outcome: RunOutcome::Succeeded
        })
    );
}

#[tokio::test]
async fn the_normal_syncs_checkpoint_is_not_touched() {
    let db = TestDb::create().await;
    let log = PgRunLog::new(&db.state);
    db.state
        .upsert_checkpoint(&Checkpoint {
            key: normal_key(&tenant()),
            last_modified_watermark: Some(utc("2026-10-12T09:00:00Z")),
            last_updated_in_dm: None,
            last_id: Some("cursor-17".into()),
            last_sync_at: Some(utc("2026-10-12T09:05:00Z")),
            records_seen: 4711,
        })
        .await
        .unwrap();
    let before = snapshot(
        &db.state
            .get_checkpoint(&normal_key(&tenant()))
            .await
            .unwrap(),
    );

    record(
        &log,
        &tenant(),
        &partial_report(),
        utc("2026-10-12T10:00:00Z"),
    )
    .await;
    record(
        &log,
        &tenant(),
        &report_with(RunOutcome::Succeeded),
        utc("2026-10-13T10:00:00Z"),
    )
    .await;

    let after = snapshot(
        &db.state
            .get_checkpoint(&normal_key(&tenant()))
            .await
            .unwrap(),
    );
    assert_eq!(after, before);
    let mut partitions: Vec<String> =
        sqlx::query_scalar("SELECT partition_key FROM vsv2.sync_checkpoint WHERE endpoint = $1")
            .bind(ENDPOINT)
            .fetch_all(&db.state.pg)
            .await
            .unwrap();
    partitions.sort(); // not ORDER BY: that depends on the collation of the database
    assert_eq!(partitions, vec![STATE_PARTITION, DEFAULT_PARTITION_KEY]);
}

#[tokio::test]
async fn the_cadence_is_set_even_when_the_run_row_cannot_be_closed() {
    // Finished without a start: the metrics cannot be stored (their run does not exist) and the
    // update closes no row. A run that was made must still not be made again at the next pass.
    let db = TestDb::create().await;
    let log = PgRunLog::new(&db.state);
    let at = utc("2026-10-12T10:00:00Z");

    log.finish(
        &tenant(),
        &invoice_def(),
        Uuid::new_v4(),
        &partial_report(),
        at,
    )
    .await
    .unwrap();

    assert_eq!(
        log.last_state(&tenant(), &invoice_def()).await.unwrap(),
        Some(RunState {
            at,
            outcome: RunOutcome::Partial
        })
    );
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM vsv2.sync_page_metric").await,
        0
    );
}

// run_if_due with the real log --------------------------------------------------------------------

/// Visma, as far as a run asks it: the open list, the documents it can be asked for by key, and
/// whether it is down.
struct FakeVisma {
    listed: Vec<OpenDoc>,
    documents: HashMap<DocKey, Value>,
    down: bool,
    asked: Mutex<Vec<DocKey>>,
}

impl FakeVisma {
    fn listing(listed: Vec<OpenDoc>) -> Self {
        Self {
            listed,
            documents: HashMap::new(),
            down: false,
            asked: Mutex::new(vec![]),
        }
    }

    fn with_document(mut self, reference: &str, balance: u32) -> Self {
        self.documents.insert(
            DocKey::new([reference]),
            json!({"referenceNumber": reference, "status": "Open", "balance": balance}),
        );
        self
    }

    fn down(mut self) -> Self {
        self.down = true;
        self
    }

    fn asked(&self) -> Vec<DocKey> {
        self.asked.lock().unwrap().clone()
    }
}

#[async_trait]
impl OpenSetSource for FakeVisma {
    async fn list_open(&self, _: &TenantId, _: &EndpointDefinition) -> Result<OpenList> {
        Ok(OpenList {
            docs: self.listed.clone(),
            page_metrics: vec![list_metric(1)],
        })
    }

    async fn fetch_by_key(
        &self,
        _: &TenantId,
        _: &EndpointDefinition,
        key: &DocKey,
    ) -> Result<Option<Value>> {
        self.asked.lock().unwrap().push(key.clone());
        if self.down {
            return Err(Error::UpstreamStatus {
                status: 503,
                body: "down".into(),
            });
        }
        Ok(self.documents.get(key).cloned())
    }
}

/// Staging, as far as a run asks it: what it holds as open, and how many rows a write changes.
struct FakeStaging {
    open: std::result::Result<Vec<OpenDoc>, String>,
    /// What every write reports as changed; `None` is as many rows as it was given.
    changed: Option<u64>,
    written: Mutex<Vec<Value>>,
}

impl FakeStaging {
    fn open(open: Vec<OpenDoc>) -> Self {
        Self {
            open: Ok(open),
            changed: None,
            written: Mutex::new(vec![]),
        }
    }

    fn unreadable() -> Self {
        Self {
            open: Err("vsv2-bq-open-keys exited non-zero: 403".into()),
            changed: None,
            written: Mutex::new(vec![]),
        }
    }

    fn changing_nothing(mut self) -> Self {
        self.changed = Some(0);
        self
    }
}

#[async_trait]
impl StagingPort for FakeStaging {
    async fn open_keys(&self, _: &TenantId, _: &EndpointDefinition) -> Result<StagedOpen> {
        match &self.open {
            Ok(docs) => Ok(StagedOpen {
                docs: docs.clone(),
                bytes_processed: Some(1_700_000),
            }),
            Err(message) => Err(Error::Other(message.clone())),
        }
    }

    async fn write(
        &self,
        _: &TenantId,
        _: &EndpointDefinition,
        _: Uuid,
        records: Vec<Value>,
    ) -> Result<u64> {
        let given = records.len() as u64;
        self.written.lock().unwrap().extend(records);
        Ok(self.changed.unwrap_or(given))
    }

    async fn quarantine(&self, _: Vec<Value>) -> Result<()> {
        Ok(())
    }
}

fn doc(reference: &str, balance: f64) -> OpenDoc {
    OpenDoc::new(DocKey::new([reference]), Some(balance))
}

fn never() -> bool {
    false
}

/// One call of `run_if_due` against the real log, at `now`.
async fn run_at(
    db: &TestDb,
    visma: &FakeVisma,
    staging: &FakeStaging,
    limits: &Limits,
    now: DateTime<Utc>,
) -> Option<OpenSetReport> {
    let log = PgRunLog::new(&db.state);
    let ports = Ports {
        visma,
        staging,
        log: &log,
    };
    run_if_due(&ports, &tenant(), &invoice_def(), limits, now, &never).await
}

/// `(status, records_in, records_written)` of every run recorded, oldest first.
async fn runs(db: &TestDb) -> Vec<(String, i64, i64)> {
    sqlx::query_as(
        "SELECT status, records_in, records_written FROM vsv2.sync_run \
         WHERE endpoint = $1 ORDER BY started_at, created_at",
    )
    .bind(RUN_LABEL)
    .fetch_all(&db.state.pg)
    .await
    .unwrap()
}

async fn cadence(db: &TestDb) -> (Option<String>, Option<DateTime<Utc>>) {
    let row = db
        .state
        .get_checkpoint(&cadence_key(&tenant()))
        .await
        .unwrap()
        .expect("the cadence row");
    (row.last_id, row.last_sync_at)
}

#[tokio::test]
async fn a_run_is_recorded_and_the_next_call_the_same_day_does_nothing() {
    let db = TestDb::create().await;
    let visma = FakeVisma::listing(vec![doc("1", 5.0)]);
    let staging = FakeStaging::open(vec![doc("1", 5.0)]);
    let now = utc("2026-10-12T10:00:00Z");

    let first = run_at(&db, &visma, &staging, &Limits::default(), now).await;

    assert_eq!(first.unwrap().outcome, RunOutcome::Succeeded);
    assert_eq!(runs(&db).await, vec![("succeeded".to_string(), 0, 0)]);
    assert_eq!(
        cadence(&db).await,
        (Some("succeeded".to_string()), Some(now))
    );

    for later in [
        Duration::minutes(5),
        Duration::hours(2),
        Duration::hours(23),
    ] {
        let again = run_at(&db, &visma, &staging, &Limits::default(), now + later).await;
        assert!(
            again.is_none(),
            "{later}: the cadence is read back from Postgres"
        );
    }
    assert_eq!(runs(&db).await.len(), 1, "no second row");
    let next = run_at(
        &db,
        &visma,
        &staging,
        &Limits::default(),
        now + Duration::days(1),
    )
    .await;
    assert!(next.is_some(), "a day later it is due");
    assert_eq!(runs(&db).await.len(), 2);
}

#[tokio::test]
async fn a_part_payment_run_is_recorded_with_the_documents_that_differed_and_the_rows_written() {
    let db = TestDb::create().await;
    // Open on both sides, same key; the balance Visma lists is lower than the one staged.
    let visma = FakeVisma::listing(vec![doc("1", 250.0), doc("2", 40.0)]).with_document("1", 250);
    let staging = FakeStaging::open(vec![doc("1", 500.0), doc("2", 40.0)]);

    let report = run_at(
        &db,
        &visma,
        &staging,
        &Limits::default(),
        utc("2026-10-12T10:00:00Z"),
    )
    .await
    .unwrap();

    assert_eq!(report.outcome, RunOutcome::Succeeded, "{:?}", report.error);
    assert_eq!(report.balance_differs, 1);
    assert_eq!(visma.asked(), vec![DocKey::new(["1"])]);
    assert_eq!(staging.written.lock().unwrap().len(), 1);
    assert_eq!(runs(&db).await, vec![("succeeded".to_string(), 1, 1)]);
}

#[tokio::test]
async fn a_failed_fetch_leaves_a_partial_run_that_is_due_again_after_two_hours() {
    let db = TestDb::create().await;
    let visma = FakeVisma::listing(vec![]).down();
    let staging = FakeStaging::open(vec![doc("1", 5.0)]);
    let now = utc("2026-10-12T10:00:00Z");

    let report = run_at(&db, &visma, &staging, &Limits::default(), now)
        .await
        .unwrap();

    assert_eq!(report.outcome, RunOutcome::Partial);
    assert_eq!(runs(&db).await, vec![("partial_success".to_string(), 1, 0)]);
    assert_eq!(cadence(&db).await, (Some("partial".to_string()), Some(now)));
    let early = now + Duration::minutes(119);
    assert!(run_at(&db, &visma, &staging, &Limits::default(), early)
        .await
        .is_none());
    let retry = now + Duration::from_std(RETRY_AFTER).unwrap();
    assert!(run_at(&db, &visma, &staging, &Limits::default(), retry)
        .await
        .is_some());
    assert_eq!(runs(&db).await.len(), 2);
}

#[tokio::test]
async fn a_cap_sized_run_that_changed_no_row_is_stalled_and_not_asked_again_for_a_day() {
    let db = TestDb::create().await;
    let mut visma = FakeVisma::listing(vec![]);
    for n in 1..=4 {
        visma = visma.with_document(&n.to_string(), 5);
    }
    let staging =
        FakeStaging::open((1..=4).map(|n| doc(&n.to_string(), 5.0)).collect()).changing_nothing();
    let limits = Limits {
        max_fetches: 2,
        ..Limits::default()
    };
    let now = utc("2026-10-12T10:00:00Z");

    let report = run_at(&db, &visma, &staging, &limits, now).await.unwrap();

    assert_eq!(report.outcome, RunOutcome::Stalled);
    assert_eq!((report.fetched, report.written, report.deferred), (2, 0, 2));
    assert_eq!(runs(&db).await, vec![("partial_success".to_string(), 4, 0)]);
    assert_eq!(cadence(&db).await, (Some("stalled".to_string()), Some(now)));
    let retry = now + Duration::from_std(RETRY_AFTER).unwrap();
    assert!(run_at(&db, &visma, &staging, &limits, retry)
        .await
        .is_none());
    let next = now + Duration::from_std(INTERVAL).unwrap();
    assert!(run_at(&db, &visma, &staging, &limits, next).await.is_some());
}

#[tokio::test]
async fn a_run_that_cannot_read_staging_is_recorded_as_failed_with_the_reason() {
    let db = TestDb::create().await;
    let visma = FakeVisma::listing(vec![]);
    let staging = FakeStaging::unreadable();
    let now = utc("2026-10-12T10:00:00Z");

    let report = run_at(&db, &visma, &staging, &Limits::default(), now)
        .await
        .unwrap();

    assert_eq!(report.outcome, RunOutcome::Failed);
    let stored: (String, Option<String>) =
        sqlx::query_as("SELECT status, error FROM vsv2.sync_run WHERE endpoint = $1")
            .bind(RUN_LABEL)
            .fetch_one(&db.state.pg)
            .await
            .unwrap();
    assert_eq!(stored.0, "failed");
    assert!(stored.1.is_some_and(|error| error.contains("403")));
    assert_eq!(cadence(&db).await, (Some("failed".to_string()), Some(now)));
}

// reconcile_open_set_if_due: the early returns ---------------------------------------------------

async fn recorded_nothing(db: &TestDb) {
    assert_eq!(count(db, "SELECT COUNT(*) FROM vsv2.sync_run").await, 0);
    assert_eq!(
        count(db, "SELECT COUNT(*) FROM vsv2.sync_checkpoint").await,
        0
    );
}

#[tokio::test]
async fn an_endpoint_without_the_flag_is_not_reconciled_and_nothing_is_recorded() {
    let harness = Harness::new(erp_def("accounts"), FakeAdapter::single_partition()).await;

    let report = harness
        .executor
        .reconcile_open_set_if_due(&harness.tenant, "accounts", &AtomicBool::new(false))
        .await;

    assert!(report.is_none());
    recorded_nothing(&harness.db).await;
}

#[tokio::test]
async fn a_flagged_endpoint_without_bigquery_staging_is_skipped_and_nothing_is_recorded() {
    let harness = Harness::new(invoice_def(), FakeAdapter::single_partition()).await;
    assert!(harness.executor.bq.is_none(), "the harness has no BigQuery");

    let report = harness
        .executor
        .reconcile_open_set_if_due(&harness.tenant, ENDPOINT, &AtomicBool::new(false))
        .await;

    assert!(report.is_none());
    recorded_nothing(&harness.db).await;
}

#[tokio::test]
async fn an_adapter_that_cannot_list_open_documents_is_skipped_before_anything_is_asked() {
    // BigQuery staging is configured, but the adapter this endpoint syncs with has no open list
    // to offer (the fake does not), so the run stops before it starts the key tool or records a
    // run.
    let mut harness = Harness::new(invoice_def(), FakeAdapter::single_partition()).await;
    harness.executor.bq = Some(BqWriter {
        project: "p".into(),
        dataset_bronze: "b".into(),
        dataset_shadow: "s".into(),
        dataset_staging: Some("st".into()),
    });

    let report = harness
        .executor
        .reconcile_open_set_if_due(&harness.tenant, ENDPOINT, &AtomicBool::new(false))
        .await;

    assert!(report.is_none());
    recorded_nothing(&harness.db).await;
}
