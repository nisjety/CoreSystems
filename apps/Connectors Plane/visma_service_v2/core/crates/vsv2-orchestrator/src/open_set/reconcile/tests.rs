//! Tests of the run and its cadence, against fakes of the three ports.

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::json;
use vsv2_common::PrimaryKey;

use super::*;
use crate::open_set::fixtures::def;
use crate::open_set::{INTERVAL, RETRY_AFTER};

const TENANT: &str = "tenant-a";

fn tenant() -> TenantId {
    TenantId::new(TENANT)
}

fn customer_def() -> EndpointDefinition {
    EndpointDefinition {
        validation_fields: Some(vec!["referenceNumber".into(), "status".into()]),
        ..def(
            Some("/v1/customerinvoice"),
            PrimaryKey::Single("referenceNumber".into()),
        )
    }
}

fn supplier_def() -> EndpointDefinition {
    EndpointDefinition {
        validation_fields: Some(vec!["referenceNumber".into(), "documentType".into()]),
        ..def(
            Some("/v1/supplierInvoice"),
            PrimaryKey::Composite(vec!["documentType".into(), "referenceNumber".into()]),
        )
    }
}

fn key(parts: &[&str]) -> DocKey {
    DocKey::new(parts)
}

fn numbered(n: u32) -> DocKey {
    DocKey::new([n.to_string()])
}

/// The balance a document has unless a test says otherwise: the same on both sides, so that what
/// a run finds is a difference of keys.
const BALANCE: f64 = 0.0;

/// `keys` as documents that all have [`BALANCE`].
fn with_balance(keys: Vec<DocKey>) -> Vec<OpenDoc> {
    keys.into_iter()
        .map(|key| OpenDoc::new(key, Some(BALANCE)))
        .collect()
}

/// A document with a one-part key and the given balance.
fn doc(reference: &str, balance: Option<f64>) -> OpenDoc {
    OpenDoc::new(DocKey::new([reference]), balance)
}

fn invoice(reference: &str, status: &str) -> Value {
    json!({"referenceNumber": reference, "status": status, "balance": 0})
}

fn metric() -> PageFetchMetric {
    PageFetchMetric {
        upstream_path: "/v1/customerinvoice".into(),
        page_param: Some("pageNumber".into()),
        page_value: Some(1),
        page_size: Some(1000),
        status: 200,
        elapsed_ms: 12,
        records: 3,
        total_count: Some(3),
        has_next: None,
        rate_limit: Default::default(),
        background_mode: None,
    }
}

#[derive(Clone)]
enum Answer {
    Doc(Value),
    Gone,
    Down,
}

struct FakeVisma {
    listing: std::result::Result<Vec<OpenDoc>, String>,
    answers: HashMap<DocKey, Answer>,
    asked: Mutex<Vec<DocKey>>,
    lists: Mutex<usize>,
    /// Hang for a long time once this many documents have been asked for.
    stall_after: Option<usize>,
}

impl FakeVisma {
    /// Visma lists `keys` as open, each with [`BALANCE`].
    fn listing(keys: Vec<DocKey>) -> Self {
        Self::listing_docs(with_balance(keys))
    }

    fn listing_docs(docs: Vec<OpenDoc>) -> Self {
        Self {
            listing: Ok(docs),
            answers: HashMap::new(),
            asked: Mutex::new(vec![]),
            lists: Mutex::new(0),
            stall_after: None,
        }
    }

    fn answering(mut self, key: DocKey, answer: Answer) -> Self {
        self.answers.insert(key, answer);
        self
    }

    fn asked(&self) -> Vec<DocKey> {
        self.asked.lock().unwrap().clone()
    }

    fn lists(&self) -> usize {
        *self.lists.lock().unwrap()
    }
}

#[async_trait]
impl OpenSetSource for FakeVisma {
    async fn list_open(&self, _: &TenantId, _: &EndpointDefinition) -> Result<OpenList> {
        *self.lists.lock().unwrap() += 1;
        match &self.listing {
            Ok(docs) => Ok(OpenList {
                docs: docs.clone(),
                page_metrics: vec![metric()],
            }),
            Err(message) => Err(Error::Http(message.clone())),
        }
    }

    async fn fetch_by_key(
        &self,
        _: &TenantId,
        _: &EndpointDefinition,
        key: &DocKey,
    ) -> Result<Option<Value>> {
        let asked_so_far = {
            let mut asked = self.asked.lock().unwrap();
            asked.push(key.clone());
            asked.len()
        };
        if self.stall_after.is_some_and(|after| asked_so_far > after) {
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
        match self.answers.get(key) {
            Some(Answer::Doc(doc)) => Ok(Some(doc.clone())),
            Some(Answer::Gone) => Ok(None),
            Some(Answer::Down) | None => Err(Error::UpstreamStatus {
                status: 503,
                body: "down".into(),
            }),
        }
    }
}

struct FakeStaging {
    open: std::result::Result<Vec<OpenDoc>, String>,
    write_fails: bool,
    /// Fail the write with this index (0 is the first) and no other.
    fail_write_number: Option<usize>,
    /// How many rows the write with each index reports as changed (0 beyond the list), in place
    /// of the number of records it was given. Documents that come back as they are already staged
    /// change nothing: `Some(vec![])` is that.
    reported_changes: Option<Vec<u64>>,
    written: Mutex<Vec<Vec<Value>>>,
    quarantined: Mutex<Vec<Vec<Value>>>,
    reads: Mutex<usize>,
}

impl FakeStaging {
    /// Staging holds `keys` as open, each with [`BALANCE`].
    fn open(keys: Vec<DocKey>) -> Self {
        Self::open_docs(with_balance(keys))
    }

    fn open_docs(docs: Vec<OpenDoc>) -> Self {
        Self {
            open: Ok(docs),
            write_fails: false,
            fail_write_number: None,
            reported_changes: None,
            written: Mutex::new(vec![]),
            quarantined: Mutex::new(vec![]),
            reads: Mutex::new(0),
        }
    }

    /// Every write reports 0 changed rows.
    fn unchanged(mut self) -> Self {
        self.reported_changes = Some(vec![]);
        self
    }

    /// The writes report these changed-row counts, in order, and 0 after them.
    fn changing(mut self, counts: Vec<u64>) -> Self {
        self.reported_changes = Some(counts);
        self
    }

    fn written(&self) -> Vec<Value> {
        self.written
            .lock()
            .unwrap()
            .iter()
            .flatten()
            .cloned()
            .collect()
    }
}

#[async_trait]
impl StagingPort for FakeStaging {
    async fn open_keys(&self, _: &TenantId, _: &EndpointDefinition) -> Result<StagedOpen> {
        *self.reads.lock().unwrap() += 1;
        match &self.open {
            Ok(docs) => Ok(StagedOpen {
                docs: docs.clone(),
                bytes_processed: Some(4096),
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
        let mut written = self.written.lock().unwrap();
        if self.write_fails || self.fail_write_number == Some(written.len()) {
            return Err(Error::Other("stage exited non-zero".into()));
        }
        let changed = match &self.reported_changes {
            Some(counts) => counts.get(written.len()).copied().unwrap_or(0),
            None => records.len() as u64,
        };
        written.push(records);
        Ok(changed)
    }

    async fn quarantine(&self, payloads: Vec<Value>) -> Result<()> {
        self.quarantined.lock().unwrap().push(payloads);
        Ok(())
    }
}

#[derive(Default)]
struct FakeLog {
    last: Mutex<Option<RunState>>,
    fail_read: bool,
    fail_start: bool,
    started: Mutex<Vec<Uuid>>,
    finished: Mutex<Vec<(OpenSetReport, DateTime<Utc>)>>,
}

#[async_trait]
impl RunLog for FakeLog {
    async fn last_state(&self, _: &TenantId, _: &EndpointDefinition) -> Result<Option<RunState>> {
        if self.fail_read {
            return Err(Error::StateStore("connection refused".into()));
        }
        Ok(*self.last.lock().unwrap())
    }

    async fn start(&self, _: &TenantId, _: &EndpointDefinition, run_id: Uuid) -> Result<()> {
        if self.fail_start {
            return Err(Error::StateStore("connection refused".into()));
        }
        self.started.lock().unwrap().push(run_id);
        Ok(())
    }

    async fn finish(
        &self,
        _: &TenantId,
        _: &EndpointDefinition,
        _: Uuid,
        report: &OpenSetReport,
        at: DateTime<Utc>,
    ) -> Result<()> {
        *self.last.lock().unwrap() = Some(RunState {
            at,
            outcome: report.outcome,
        });
        self.finished.lock().unwrap().push((report.clone(), at));
        Ok(())
    }
}

/// The shutdown flag, never set.
fn never() -> bool {
    false
}

async fn run(
    visma: &FakeVisma,
    staging: &FakeStaging,
    def: &EndpointDefinition,
    limits: Limits,
) -> OpenSetReport {
    let log = FakeLog::default();
    let ports = Ports {
        visma,
        staging,
        log: &log,
    };
    reconcile(&ports, &tenant(), def, Uuid::nil(), &limits, &never).await
}

// The run ------------------------------------------------------------------------------------

#[tokio::test]
async fn identical_open_sets_cost_one_list_and_nothing_else() {
    let visma = FakeVisma::listing(vec![key(&["1"]), key(&["2"])]);
    let staging = FakeStaging::open(vec![key(&["2"]), key(&["1"])]);

    let report = run(&visma, &staging, &customer_def(), Limits::default()).await;

    assert_eq!(report.outcome, RunOutcome::Succeeded);
    assert_eq!((report.staging_open, report.visma_open), (2, 2));
    assert_eq!(report.differences(), 0);
    assert_eq!((report.list_requests, report.by_key_requests), (1, 0));
    assert_eq!(report.bq_bytes, Some(4096));
    assert!(visma.asked().is_empty());
    assert!(
        staging.written().is_empty(),
        "nothing written, so no heartbeat either"
    );
    assert_eq!(report.error, None);
}

#[tokio::test]
async fn an_invoice_paid_since_staging_saw_it_is_fetched_and_upserted_closed() {
    let visma = FakeVisma::listing(vec![key(&["1"])])
        .answering(key(&["2"]), Answer::Doc(invoice("2", "Closed")));
    let staging = FakeStaging::open(vec![key(&["1"]), key(&["2"])]);

    let report = run(&visma, &staging, &customer_def(), Limits::default()).await;

    assert_eq!(report.outcome, RunOutcome::Succeeded);
    assert_eq!((report.stale_in_staging, report.missing_in_staging), (1, 0));
    assert_eq!(visma.asked(), vec![key(&["2"])]);
    assert_eq!((report.fetched, report.written), (1, 1));
    let written = staging.written();
    assert_eq!(written.len(), 1);
    assert_eq!(written[0]["status"], "Closed");
    assert_eq!(
        written[0]["tenant_id"], TENANT,
        "enriched like any synced record"
    );
    assert_eq!(written[0]["_source"], "erp_rest");
    assert!(written[0]["_synced_at"].is_string());
}

#[tokio::test]
async fn an_invoice_open_in_visma_but_not_in_staging_is_fetched_too() {
    let visma = FakeVisma::listing(vec![key(&["1"]), key(&["3"])])
        .answering(key(&["3"]), Answer::Doc(invoice("3", "Open")));
    let staging = FakeStaging::open(vec![key(&["1"])]);

    let report = run(&visma, &staging, &customer_def(), Limits::default()).await;

    assert_eq!(report.outcome, RunOutcome::Succeeded);
    assert_eq!((report.stale_in_staging, report.missing_in_staging), (0, 1));
    assert_eq!(staging.written().len(), 1);
}

#[tokio::test]
async fn stale_documents_are_asked_for_before_missing_ones_each_in_key_order() {
    let visma = FakeVisma::listing(vec![key(&["7"]), key(&["5"])])
        .answering(key(&["9"]), Answer::Gone)
        .answering(key(&["1"]), Answer::Gone)
        .answering(key(&["5"]), Answer::Gone)
        .answering(key(&["7"]), Answer::Gone);
    let staging = FakeStaging::open(vec![key(&["9"]), key(&["1"])]);

    run(&visma, &staging, &customer_def(), Limits::default()).await;

    assert_eq!(
        visma.asked(),
        vec![key(&["1"]), key(&["9"]), key(&["5"]), key(&["7"])]
    );
}

#[tokio::test]
async fn a_supplier_invoice_is_asked_for_with_both_parts_of_its_key() {
    let doc = json!({"documentType": "DebitAdj", "referenceNumber": "200089", "status": "Closed"});
    let visma =
        FakeVisma::listing(vec![]).answering(key(&["DebitAdj", "200089"]), Answer::Doc(doc));
    let staging = FakeStaging::open(vec![key(&["DebitAdj", "200089"])]);

    let report = run(&visma, &staging, &supplier_def(), Limits::default()).await;

    assert_eq!(visma.asked(), vec![key(&["DebitAdj", "200089"])]);
    assert_eq!(report.outcome, RunOutcome::Succeeded);
    assert_eq!(staging.written().len(), 1);
}

#[tokio::test]
async fn a_document_visma_no_longer_has_is_counted_and_never_written_or_deleted() {
    let visma = FakeVisma::listing(vec![]).answering(key(&["4"]), Answer::Gone);
    let staging = FakeStaging::open(vec![key(&["4"])]);

    let report = run(&visma, &staging, &customer_def(), Limits::default()).await;

    assert_eq!(report.outcome, RunOutcome::Succeeded, "404 is an answer");
    assert_eq!(report.not_found, 1);
    assert!(staging.written().is_empty());
    assert_eq!(report.error, None);
}

#[tokio::test]
async fn when_staging_cannot_be_read_visma_is_not_asked_anything() {
    let visma = FakeVisma::listing(vec![key(&["1"])]);
    let mut staging = FakeStaging::open(vec![]);
    staging.open = Err("vsv2-bq-open-keys exited non-zero: 403".into());

    let report = run(&visma, &staging, &customer_def(), Limits::default()).await;

    assert_eq!(report.outcome, RunOutcome::Failed);
    assert!(report.error.unwrap().contains("403"));
    assert_eq!((visma.lists(), visma.asked().len()), (0, 0));
    assert_eq!((report.list_requests, report.by_key_requests), (0, 0));
}

#[tokio::test]
async fn when_visma_cannot_be_listed_nothing_is_fetched_or_written() {
    let mut visma = FakeVisma::listing(vec![]);
    visma.listing = Err("open list row 3 has status \"Closed\"".into());
    let staging = FakeStaging::open(vec![key(&["1"]), key(&["2"])]);

    let report = run(&visma, &staging, &customer_def(), Limits::default()).await;

    assert_eq!(report.outcome, RunOutcome::Failed);
    assert_eq!(
        report.staging_open, 2,
        "what was read before the failure is kept"
    );
    assert!(visma.asked().is_empty());
    assert!(staging.written().is_empty());
}

#[tokio::test]
async fn an_isolated_failed_fetch_is_partial_and_the_other_documents_are_still_written() {
    let visma = FakeVisma::listing(vec![])
        .answering(key(&["1"]), Answer::Doc(invoice("1", "Closed")))
        .answering(key(&["2"]), Answer::Down)
        .answering(key(&["3"]), Answer::Doc(invoice("3", "Closed")));
    let staging = FakeStaging::open(vec![key(&["1"]), key(&["2"]), key(&["3"])]);

    let report = run(&visma, &staging, &customer_def(), Limits::default()).await;

    assert_eq!(report.outcome, RunOutcome::Partial);
    assert_eq!(
        (report.fetched, report.fetch_failed, report.written),
        (2, 1, 2)
    );
    assert_eq!(visma.asked().len(), 3, "one failure does not stop the run");
    assert!(report.error.unwrap().contains("503"));
}

#[tokio::test]
async fn repeated_failures_stop_the_run_and_defer_the_rest() {
    let keys: Vec<DocKey> = (1..=10).map(numbered).collect();
    let visma = FakeVisma::listing(vec![]); // every key answers 503
    let staging = FakeStaging::open(keys);
    let limits = Limits {
        max_consecutive_failures: 3,
        ..Limits::default()
    };

    let report = run(&visma, &staging, &customer_def(), limits).await;

    assert_eq!(report.outcome, RunOutcome::Partial);
    assert_eq!(
        visma.asked().len(),
        3,
        "stopped asking after three in a row"
    );
    assert_eq!((report.fetch_failed, report.deferred), (3, 7));
    assert!(staging.written().is_empty());
}

#[tokio::test]
async fn a_success_between_failures_resets_the_streak() {
    let visma = FakeVisma::listing(vec![])
        .answering(key(&["1"]), Answer::Down)
        .answering(key(&["2"]), Answer::Down)
        .answering(key(&["3"]), Answer::Gone)
        .answering(key(&["4"]), Answer::Down)
        .answering(key(&["5"]), Answer::Down);
    let staging = FakeStaging::open((1..=5).map(numbered).collect());
    let limits = Limits {
        max_consecutive_failures: 3,
        ..Limits::default()
    };

    let report = run(&visma, &staging, &customer_def(), limits).await;

    assert_eq!(visma.asked().len(), 5);
    assert_eq!(
        (report.fetch_failed, report.deferred, report.not_found),
        (4, 0, 1)
    );
}

#[tokio::test]
async fn more_differences_than_the_cap_are_deferred_not_dropped() {
    let keys: Vec<DocKey> = (1..=5).map(numbered).collect();
    let mut visma = FakeVisma::listing(vec![]);
    for k in &keys {
        visma
            .answers
            .insert(k.clone(), Answer::Doc(invoice(&k.to_string(), "Closed")));
    }
    let staging = FakeStaging::open(keys);
    let limits = Limits {
        max_fetches: 2,
        ..Limits::default()
    };

    let report = run(&visma, &staging, &customer_def(), limits).await;

    assert_eq!(
        report.outcome,
        RunOutcome::Partial,
        "it made progress: soon is right"
    );
    assert_eq!(visma.asked(), vec![key(&["1"]), key(&["2"])]);
    assert_eq!((report.stale_in_staging, report.deferred), (5, 3));
    assert!(report.error.unwrap().contains("3 document(s) left"));
}

#[tokio::test]
async fn an_answer_for_another_document_is_a_failure_not_a_write() {
    let visma =
        FakeVisma::listing(vec![]).answering(key(&["1"]), Answer::Doc(invoice("999", "Closed")));
    let staging = FakeStaging::open(vec![key(&["1"])]);

    let report = run(&visma, &staging, &customer_def(), Limits::default()).await;

    assert_eq!((report.fetch_failed, report.fetched), (1, 0));
    assert!(staging.written().is_empty());
    assert!(report.error.unwrap().contains("999"));
}

#[tokio::test]
async fn a_document_that_does_not_validate_is_quarantined_and_the_rest_is_written() {
    let bad = json!({"referenceNumber": "2"}); // no status: a validation field of the endpoint
    let visma = FakeVisma::listing(vec![])
        .answering(key(&["1"]), Answer::Doc(invoice("1", "Closed")))
        .answering(key(&["2"]), Answer::Doc(bad));
    let staging = FakeStaging::open(vec![key(&["1"]), key(&["2"])]);

    let report = run(&visma, &staging, &customer_def(), Limits::default()).await;

    assert_eq!(report.outcome, RunOutcome::Partial);
    assert_eq!((report.fetched, report.rejected, report.written), (1, 1, 1));
    assert_eq!(staging.written().len(), 1);
    let quarantined = staging.quarantined.lock().unwrap().clone();
    assert_eq!(quarantined.len(), 1);
    assert_eq!(quarantined[0][0]["endpoint"], "invoices");
    assert!(report
        .error
        .unwrap()
        .contains("missing validation field status"));
}

#[tokio::test]
async fn a_failed_write_fails_the_run_and_keeps_what_was_counted() {
    let visma =
        FakeVisma::listing(vec![]).answering(key(&["1"]), Answer::Doc(invoice("1", "Closed")));
    let mut staging = FakeStaging::open(vec![key(&["1"])]);
    staging.write_fails = true;

    let report = run(&visma, &staging, &customer_def(), Limits::default()).await;

    assert_eq!(report.outcome, RunOutcome::Failed);
    assert_eq!((report.stale_in_staging, report.by_key_requests), (1, 1));
    assert_eq!(report.written, 0);
    assert!(report.error.unwrap().contains("stage exited non-zero"));
}

#[tokio::test]
async fn documents_are_written_a_batch_at_a_time_so_a_failed_write_keeps_the_earlier_ones() {
    let mut visma = FakeVisma::listing(vec![]);
    for n in 1..=5 {
        visma
            .answers
            .insert(numbered(n), Answer::Doc(invoice(&n.to_string(), "Closed")));
    }
    let mut staging = FakeStaging::open((1..=5).map(numbered).collect());
    staging.fail_write_number = Some(1); // the second batch
    let limits = Limits {
        batch_size: 2,
        ..Limits::default()
    };

    let report = run(&visma, &staging, &customer_def(), limits).await;

    assert_eq!(report.outcome, RunOutcome::Failed);
    assert_eq!(report.written, 2, "the first batch stays written");
    assert_eq!(staging.written().len(), 2);
    assert_eq!(
        visma.asked(),
        vec![numbered(1), numbered(2), numbered(3), numbered(4)],
        "nothing is fetched after the write that failed"
    );
}

#[tokio::test]
async fn every_batch_is_written_and_counted() {
    let mut visma = FakeVisma::listing(vec![]);
    for n in 1..=5 {
        visma
            .answers
            .insert(numbered(n), Answer::Doc(invoice(&n.to_string(), "Closed")));
    }
    let staging = FakeStaging::open((1..=5).map(numbered).collect());
    let limits = Limits {
        batch_size: 2,
        ..Limits::default()
    };

    let report = run(&visma, &staging, &customer_def(), limits).await;

    assert_eq!(report.outcome, RunOutcome::Succeeded);
    assert_eq!((report.fetched, report.written), (5, 5));
    let batches: Vec<usize> = staging
        .written
        .lock()
        .unwrap()
        .iter()
        .map(Vec::len)
        .collect();
    assert_eq!(batches, vec![2, 2, 1]);
}

#[tokio::test]
async fn a_run_that_gives_up_still_writes_what_it_fetched_before() {
    let visma = FakeVisma::listing(vec![])
        .answering(numbered(1), Answer::Doc(invoice("1", "Closed")))
        .answering(numbered(2), Answer::Doc(invoice("2", "Closed")))
        .answering(numbered(3), Answer::Down)
        .answering(numbered(4), Answer::Down)
        .answering(numbered(5), Answer::Down);
    let staging = FakeStaging::open((1..=7).map(numbered).collect());

    let report = run(&visma, &staging, &customer_def(), Limits::default()).await;

    assert_eq!(report.outcome, RunOutcome::Partial);
    assert_eq!(
        visma.asked().len(),
        5,
        "gave up after three failures in a row"
    );
    assert_eq!((report.fetched, report.written), (2, 2));
    assert_eq!((report.fetch_failed, report.deferred), (3, 2));
}

#[tokio::test]
async fn failures_in_a_row_are_counted_across_batches() {
    let visma = FakeVisma::listing(vec![]);
    let staging = FakeStaging::open((1..=9).map(numbered).collect());
    let limits = Limits {
        batch_size: 2,
        max_consecutive_failures: 3,
        ..Limits::default()
    };

    let report = run(&visma, &staging, &customer_def(), limits).await;

    assert_eq!(
        visma.asked().len(),
        3,
        "two failures in one batch, the third in the next"
    );
    assert_eq!((report.fetch_failed, report.deferred), (3, 6));
}

#[tokio::test]
async fn an_endpoint_that_cannot_be_reconciled_fails_before_anything_is_read() {
    let mut broken = customer_def();
    broken.tier = Some(2);
    let visma = FakeVisma::listing(vec![]);
    let staging = FakeStaging::open(vec![]);

    let report = run(&visma, &staging, &broken, Limits::default()).await;

    assert_eq!(report.outcome, RunOutcome::Failed);
    assert!(report.error.unwrap().contains("tier 1"));
    assert_eq!(*staging.reads.lock().unwrap(), 0);
}

// Part payments ------------------------------------------------------------------------------

/// What Visma answers by key for a part-paid invoice: still open, a smaller balance.
fn part_paid(reference: &str, balance: u32) -> Value {
    json!({"referenceNumber": reference, "status": "Open", "balance": balance})
}

#[tokio::test]
async fn a_part_paid_invoice_is_fetched_and_upserted_with_its_new_balance() {
    // Open on both sides with the same key; only the balance moved, and the timestamp did not.
    let visma = FakeVisma::listing_docs(vec![doc("1", Some(1000.0)), doc("2", Some(250.0))])
        .answering(key(&["2"]), Answer::Doc(part_paid("2", 250)));
    let staging = FakeStaging::open_docs(vec![doc("1", Some(1000.0)), doc("2", Some(500.0))]);

    let report = run(&visma, &staging, &customer_def(), Limits::default()).await;

    assert_eq!(report.outcome, RunOutcome::Succeeded);
    assert_eq!(
        (
            report.stale_in_staging,
            report.missing_in_staging,
            report.balance_differs
        ),
        (0, 0, 1)
    );
    assert_eq!(report.differences(), 1);
    assert_eq!(
        visma.asked(),
        vec![key(&["2"])],
        "the equal one is not asked for"
    );
    assert_eq!((report.fetched, report.written), (1, 1));
    let written = staging.written();
    assert_eq!(written.len(), 1);
    assert_eq!(written[0]["balance"], 250);
    assert_eq!(written[0]["status"], "Open");
    assert_eq!(report.error, None);
}

#[tokio::test]
async fn equal_balances_cost_one_list_and_nothing_else() {
    let docs = vec![doc("1", Some(1450.5)), doc("2", Some(0.01))];
    let visma = FakeVisma::listing_docs(docs.clone());
    let staging = FakeStaging::open_docs(docs);

    let report = run(&visma, &staging, &customer_def(), Limits::default()).await;

    assert_eq!(report.outcome, RunOutcome::Succeeded);
    assert_eq!((report.differences(), report.balance_differs), (0, 0));
    assert_eq!((report.list_requests, report.by_key_requests), (1, 0));
    assert!(visma.asked().is_empty());
    assert!(staging.written().is_empty());
}

#[tokio::test]
async fn a_balance_staging_does_not_have_is_unknown_and_the_document_is_fetched() {
    // Not equal because not known: nothing shows that no payment was made.
    let visma = FakeVisma::listing_docs(vec![doc("1", Some(40.0)), doc("2", Some(40.0))])
        .answering(key(&["1"]), Answer::Doc(part_paid("1", 40)))
        .answering(key(&["2"]), Answer::Doc(part_paid("2", 40)));
    let staging = FakeStaging::open_docs(vec![doc("1", None), doc("2", Some(40.0))]);

    let report = run(&visma, &staging, &customer_def(), Limits::default()).await;

    assert_eq!(report.balance_differs, 1);
    assert_eq!(visma.asked(), vec![key(&["1"])]);
    assert_eq!(report.outcome, RunOutcome::Succeeded);
}

#[tokio::test]
async fn a_balance_visma_does_not_list_is_unknown_too() {
    let visma = FakeVisma::listing_docs(vec![doc("1", None)])
        .answering(key(&["1"]), Answer::Doc(part_paid("1", 40)));
    let staging = FakeStaging::open_docs(vec![doc("1", Some(40.0))]);

    let report = run(&visma, &staging, &customer_def(), Limits::default()).await;

    assert_eq!(report.balance_differs, 1);
    assert_eq!(visma.asked(), vec![key(&["1"])]);
}

#[tokio::test]
async fn the_queue_is_the_stale_then_the_missing_then_the_part_paid_documents_cut_at_the_cap() {
    let mut visma = FakeVisma::listing_docs(vec![
        doc("3", Some(1.0)),
        doc("4", Some(1.0)),
        doc("5", Some(1.0)),
    ]);
    for n in 1..=5 {
        visma
            .answers
            .insert(numbered(n), Answer::Doc(part_paid(&n.to_string(), 1)));
    }
    // 1 is stale (staged open, not listed), 5 is missing, 3 and 4 are open on both sides with
    // another balance.
    let staging = FakeStaging::open_docs(vec![
        doc("1", Some(9.0)),
        doc("3", Some(9.0)),
        doc("4", Some(9.0)),
    ]);
    let limits = Limits {
        max_fetches: 3,
        ..Limits::default()
    };

    let report = run(&visma, &staging, &customer_def(), limits).await;

    assert_eq!(
        (
            report.stale_in_staging,
            report.missing_in_staging,
            report.balance_differs
        ),
        (1, 1, 2)
    );
    assert_eq!(
        visma.asked(),
        vec![numbered(1), numbered(5), numbered(3)],
        "the cap leaves out the last of the part-paid ones, not a stale one"
    );
    assert_eq!(report.deferred, 1);
}

#[tokio::test]
async fn a_part_payment_that_visma_answers_404_for_is_counted_not_written() {
    // The list says open, by key says gone: counted as any 404, and nothing is deleted.
    let visma =
        FakeVisma::listing_docs(vec![doc("1", Some(1.0))]).answering(key(&["1"]), Answer::Gone);
    let staging = FakeStaging::open_docs(vec![doc("1", Some(2.0))]);

    let report = run(&visma, &staging, &customer_def(), Limits::default()).await;

    assert_eq!((report.balance_differs, report.not_found), (1, 1));
    assert!(staging.written().is_empty());
}

// Stalled, stopped, capped -----------------------------------------------------------------

#[tokio::test]
async fn documents_that_are_all_404_leave_the_run_stalled_not_partial() {
    let mut visma = FakeVisma::listing(vec![]);
    for n in 1..=5 {
        visma.answers.insert(numbered(n), Answer::Gone);
    }
    let staging = FakeStaging::open((1..=5).map(numbered).collect());
    let limits = Limits {
        max_fetches: 3,
        ..Limits::default()
    };

    let report = run(&visma, &staging, &customer_def(), limits).await;

    assert_eq!(
        report.outcome,
        RunOutcome::Stalled,
        "asking again would get the same 404s"
    );
    assert_eq!(
        (report.not_found, report.deferred, report.fetched),
        (3, 2, 0)
    );
}

#[tokio::test]
async fn documents_that_all_fail_validation_leave_the_run_stalled() {
    let visma = FakeVisma::listing(vec![])
        .answering(numbered(1), Answer::Doc(json!({"referenceNumber": "1"})))
        .answering(numbered(2), Answer::Doc(json!({"referenceNumber": "2"})));
    let staging = FakeStaging::open(vec![numbered(1), numbered(2)]);

    let report = run(&visma, &staging, &customer_def(), Limits::default()).await;

    assert_eq!(report.outcome, RunOutcome::Stalled);
    assert_eq!((report.rejected, report.fetched, report.written), (2, 0, 0));
}

#[tokio::test]
async fn a_failed_request_is_partial_even_when_nothing_else_got_through() {
    let visma = FakeVisma::listing(vec![]); // 503 for every key
    let staging = FakeStaging::open(vec![numbered(1), numbered(2)]);

    let report = run(&visma, &staging, &customer_def(), Limits::default()).await;

    assert_eq!(
        report.outcome,
        RunOutcome::Partial,
        "a failure deserves a retry soon"
    );
}

#[tokio::test]
async fn progress_keeps_a_deferred_run_partial() {
    let mut visma = FakeVisma::listing(vec![]);
    for n in 1..=4 {
        visma
            .answers
            .insert(numbered(n), Answer::Doc(invoice(&n.to_string(), "Closed")));
    }
    let staging = FakeStaging::open((1..=4).map(numbered).collect());
    let limits = Limits {
        max_fetches: 2,
        ..Limits::default()
    };

    let report = run(&visma, &staging, &customer_def(), limits).await;

    assert_eq!(
        report.outcome,
        RunOutcome::Partial,
        "it fixed two and has two to go"
    );
}

#[tokio::test]
async fn ten_documents_that_are_all_404_say_something_is_wrong_with_the_path_or_the_company() {
    let mut visma = FakeVisma::listing(vec![]);
    for n in 1..=10 {
        visma.answers.insert(numbered(n), Answer::Gone);
    }
    let staging = FakeStaging::open((1..=10).map(numbered).collect());

    let report = run(&visma, &staging, &customer_def(), Limits::default()).await;

    assert_eq!(report.outcome, RunOutcome::Stalled);
    assert!(report
        .error
        .unwrap()
        .contains("every by-key answer (10) was 404"));
}

#[tokio::test]
async fn a_few_404s_among_answers_are_just_deleted_documents() {
    let mut visma = FakeVisma::listing(vec![]);
    for n in 1..=9 {
        visma.answers.insert(numbered(n), Answer::Gone);
    }
    visma
        .answers
        .insert(numbered(10), Answer::Doc(invoice("10", "Closed")));
    let staging = FakeStaging::open((1..=10).map(numbered).collect());

    let report = run(&visma, &staging, &customer_def(), Limits::default()).await;

    assert_eq!(report.outcome, RunOutcome::Succeeded);
    assert_eq!((report.not_found, report.fetched), (9, 1));
    assert_eq!(report.error, None);
}

#[tokio::test]
async fn the_cap_and_a_halt_after_failures_defer_what_each_left() {
    let visma = FakeVisma::listing(vec![]); // every key answers 503
    let staging = FakeStaging::open((1..=8).map(numbered).collect());
    let limits = Limits {
        max_fetches: 6,
        max_consecutive_failures: 3,
        ..Limits::default()
    };

    let report = run(&visma, &staging, &customer_def(), limits).await;

    assert_eq!(visma.asked().len(), 3);
    assert_eq!(
        report.deferred, 5,
        "two over the cap, three of the six asked for that were not asked after the halt"
    );
}

#[tokio::test]
async fn a_failure_limit_of_zero_does_not_halt_a_run_that_is_answered() {
    let visma = FakeVisma::listing(vec![])
        .answering(numbered(1), Answer::Doc(invoice("1", "Closed")))
        .answering(numbered(2), Answer::Doc(invoice("2", "Closed")));
    let staging = FakeStaging::open(vec![numbered(1), numbered(2)]);
    let limits = Limits {
        max_consecutive_failures: 0,
        ..Limits::default()
    };

    let report = run(&visma, &staging, &customer_def(), limits).await;

    assert_eq!(report.outcome, RunOutcome::Succeeded);
    assert_eq!(visma.asked().len(), 2);
}

#[tokio::test]
async fn a_run_told_to_stop_before_it_starts_reads_and_asks_nothing() {
    let visma = FakeVisma::listing(vec![]);
    let staging = FakeStaging::open((1..=3).map(numbered).collect());
    let log = FakeLog::default();
    let ports = Ports {
        visma: &visma,
        staging: &staging,
        log: &log,
    };

    let report = reconcile(
        &ports,
        &tenant(),
        &customer_def(),
        Uuid::nil(),
        &Limits::default(),
        &|| true,
    )
    .await;

    assert_eq!(
        report.outcome,
        RunOutcome::Partial,
        "an interrupted run is retried soon"
    );
    assert_eq!(*staging.reads.lock().unwrap(), 0);
    assert_eq!((visma.lists(), visma.asked().len()), (0, 0));
    assert!(report.error.unwrap().contains("stopped for shutdown"));
}

#[tokio::test]
async fn a_run_told_to_stop_while_staging_is_read_does_not_ask_visma_for_its_list() {
    let visma = FakeVisma::listing(vec![]);
    let staging = FakeStaging::open((1..=3).map(numbered).collect());
    let log = FakeLog::default();
    let ports = Ports {
        visma: &visma,
        staging: &staging,
        log: &log,
    };
    let stop = || *staging.reads.lock().unwrap() >= 1;

    let report = reconcile(
        &ports,
        &tenant(),
        &customer_def(),
        Uuid::nil(),
        &Limits::default(),
        &stop,
    )
    .await;

    assert_eq!(report.outcome, RunOutcome::Partial);
    assert_eq!(report.staging_open, 3, "what was read is in the report");
    assert_eq!((visma.lists(), visma.asked().len()), (0, 0));
    assert!(report.error.unwrap().contains("stopped for shutdown"));
}

#[tokio::test]
async fn a_run_told_to_stop_after_the_list_asks_for_no_documents_and_defers_them_all() {
    let visma = FakeVisma::listing(vec![]);
    let staging = FakeStaging::open((1..=3).map(numbered).collect());
    let log = FakeLog::default();
    let ports = Ports {
        visma: &visma,
        staging: &staging,
        log: &log,
    };
    let stop = || visma.lists() >= 1;

    let report = reconcile(
        &ports,
        &tenant(),
        &customer_def(),
        Uuid::nil(),
        &Limits::default(),
        &stop,
    )
    .await;

    assert_eq!(report.outcome, RunOutcome::Partial);
    assert!(visma.asked().is_empty());
    assert_eq!((report.by_key_requests, report.deferred), (0, 3));
    assert_eq!(report.stale_in_staging, 3, "the comparison was made");
    assert!(report.error.unwrap().contains("stopped for shutdown"));
}

#[tokio::test]
async fn what_is_left_is_counted_even_when_a_write_ends_the_run() {
    let mut visma = FakeVisma::listing(vec![]);
    for n in 1..=7 {
        visma
            .answers
            .insert(numbered(n), Answer::Doc(invoice(&n.to_string(), "Closed")));
    }
    let mut staging = FakeStaging::open((1..=7).map(numbered).collect());
    staging.fail_write_number = Some(1); // the second batch of three
    let limits = Limits {
        batch_size: 3,
        max_fetches: 6,
        ..Limits::default()
    };

    let report = run(&visma, &staging, &customer_def(), limits).await;

    assert_eq!(report.outcome, RunOutcome::Failed);
    assert_eq!(report.written, 3);
    assert_eq!(
        report.deferred, 1,
        "seven differ, six were asked for, the cap left one: the batch whose write failed was asked for"
    );
    assert!(report.error.unwrap().contains("left for the next run"));
}

#[tokio::test]
async fn a_run_told_to_stop_keeps_the_batches_it_has_written() {
    let mut visma = FakeVisma::listing(vec![]);
    for n in 1..=6 {
        visma
            .answers
            .insert(numbered(n), Answer::Doc(invoice(&n.to_string(), "Closed")));
    }
    let staging = FakeStaging::open((1..=6).map(numbered).collect());
    let log = FakeLog::default();
    let ports = Ports {
        visma: &visma,
        staging: &staging,
        log: &log,
    };
    let limits = Limits {
        batch_size: 2,
        ..Limits::default()
    };

    // The flag is set once two documents have been asked for: the end of the first batch.
    let stop = || visma.asked().len() >= 2;
    let report = reconcile(
        &ports,
        &tenant(),
        &customer_def(),
        Uuid::nil(),
        &limits,
        &stop,
    )
    .await;

    assert_eq!(report.outcome, RunOutcome::Partial);
    assert_eq!((report.fetched, report.written, report.deferred), (2, 2, 4));
    assert_eq!(staging.written().len(), 2);
    assert_eq!(visma.asked().len(), 2, "nothing asked for after the flag");
}

#[tokio::test]
async fn a_run_stopped_by_the_timeout_keeps_what_it_counted_and_wrote() {
    let mut visma = FakeVisma::listing(vec![]);
    for n in 1..=4 {
        visma
            .answers
            .insert(numbered(n), Answer::Doc(invoice(&n.to_string(), "Closed")));
    }
    visma.stall_after = Some(2); // the third document hangs
    let staging = FakeStaging::open((1..=4).map(numbered).collect());
    let log = FakeLog::default();
    let ports = Ports {
        visma: &visma,
        staging: &staging,
        log: &log,
    };
    let limits = Limits {
        batch_size: 2,
        timeout: Duration::from_millis(300),
        ..Limits::default()
    };

    let report = run_if_due(&ports, &tenant(), &customer_def(), &limits, now(), &never)
        .await
        .unwrap();

    assert_eq!(report.outcome, RunOutcome::Failed);
    assert!(report.error.as_deref().unwrap().contains("stopped"));
    assert_eq!(
        report.written, 2,
        "the first batch was written before the stop"
    );
    assert_eq!(report.by_key_requests, 3);
    assert_eq!((report.stale_in_staging, report.staging_open), (4, 4));
    let finished = log.finished.lock().unwrap().clone();
    assert_eq!(finished[0].0.written, 2, "and the record says so");
}

// Documents that come back as they are ------------------------------------------------------

#[tokio::test]
async fn a_deferred_run_whose_fetched_documents_changed_no_row_is_stalled_not_partial() {
    // Five differ and three are asked for; each comes back as it is already staged, so the write
    // changes no row. The same three would come first again two hours later.
    let mut visma = FakeVisma::listing(vec![]);
    for n in 1..=5 {
        visma
            .answers
            .insert(numbered(n), Answer::Doc(invoice(&n.to_string(), "Closed")));
    }
    let staging = FakeStaging::open((1..=5).map(numbered).collect()).unchanged();
    let limits = Limits {
        max_fetches: 3,
        ..Limits::default()
    };

    let report = run(&visma, &staging, &customer_def(), limits).await;

    assert_eq!(
        report.outcome,
        RunOutcome::Stalled,
        "asking again soon would change nothing either"
    );
    assert_eq!((report.fetched, report.written, report.deferred), (3, 0, 2));
    assert!(report.error.unwrap().contains("2 document(s) left"));
}

#[tokio::test]
async fn a_cap_sized_no_op_run_is_asked_again_after_a_day_not_two_hours() {
    let mut visma = FakeVisma::listing(vec![]);
    for n in 1..=6 {
        visma
            .answers
            .insert(numbered(n), Answer::Doc(invoice(&n.to_string(), "Closed")));
    }
    let staging = FakeStaging::open((1..=6).map(numbered).collect()).unchanged();
    let log = FakeLog::default();
    let ports = Ports {
        visma: &visma,
        staging: &staging,
        log: &log,
    };
    let limits = Limits {
        max_fetches: 4,
        ..Limits::default()
    };

    let report = run_if_due(&ports, &tenant(), &customer_def(), &limits, now(), &never)
        .await
        .unwrap();

    assert_eq!(report.outcome, RunOutcome::Stalled);
    let last = log.last.lock().unwrap().unwrap();
    assert_eq!(last.outcome, RunOutcome::Stalled);
    assert!(!is_due(
        Some(last),
        now() + chrono::Duration::from_std(RETRY_AFTER).unwrap()
    ));
    assert!(!is_due(Some(last), now() + chrono::Duration::hours(23)));
    assert!(is_due(
        Some(last),
        now() + chrono::Duration::from_std(INTERVAL).unwrap()
    ));
}

#[tokio::test]
async fn a_deferred_run_that_changed_a_row_stays_partial_even_if_a_later_batch_changed_none() {
    let mut visma = FakeVisma::listing(vec![]);
    for n in 1..=6 {
        visma
            .answers
            .insert(numbered(n), Answer::Doc(invoice(&n.to_string(), "Closed")));
    }
    // Two batches of two: the first changes one row, the second none; two differences are left.
    let staging = FakeStaging::open((1..=6).map(numbered).collect()).changing(vec![1, 0]);
    let limits = Limits {
        batch_size: 2,
        max_fetches: 4,
        ..Limits::default()
    };

    let report = run(&visma, &staging, &customer_def(), limits).await;

    assert_eq!(
        report.outcome,
        RunOutcome::Partial,
        "a row changed: there is progress, and more to come"
    );
    assert_eq!((report.written, report.deferred), (1, 2));
}

#[tokio::test]
async fn a_failed_fetch_keeps_a_run_partial_even_when_the_rest_changed_nothing() {
    let visma = FakeVisma::listing(vec![])
        .answering(numbered(1), Answer::Doc(invoice("1", "Closed")))
        .answering(numbered(2), Answer::Down);
    let staging = FakeStaging::open(vec![numbered(1), numbered(2)]).unchanged();

    let report = run(&visma, &staging, &customer_def(), Limits::default()).await;

    assert_eq!(
        report.outcome,
        RunOutcome::Partial,
        "a failure deserves a retry soon"
    );
    assert_eq!(
        (report.fetched, report.written, report.fetch_failed),
        (1, 0, 1)
    );
}

#[tokio::test]
async fn a_no_op_diff_that_fits_in_the_cap_is_a_success_and_asked_again_after_a_day() {
    let visma = FakeVisma::listing(vec![])
        .answering(numbered(1), Answer::Doc(invoice("1", "Closed")))
        .answering(numbered(2), Answer::Doc(invoice("2", "Closed")));
    let staging = FakeStaging::open(vec![numbered(1), numbered(2)]).unchanged();

    let report = run(&visma, &staging, &customer_def(), Limits::default()).await;

    assert_eq!(report.outcome, RunOutcome::Succeeded);
    assert_eq!((report.fetched, report.written, report.deferred), (2, 0, 0));
    assert_eq!(report.error, None);
}

// The cadence and the record --------------------------------------------------------------

fn now() -> DateTime<Utc> {
    "2026-10-12T10:00:00Z".parse().unwrap()
}

async fn due(log: &FakeLog, def: &EndpointDefinition) -> (Option<OpenSetReport>, FakeVisma) {
    let visma = FakeVisma::listing(vec![]);
    let staging = FakeStaging::open(vec![]);
    let ports = Ports {
        visma: &visma,
        staging: &staging,
        log,
    };
    let report = run_if_due(&ports, &tenant(), def, &Limits::default(), now(), &never).await;
    (report, visma)
}

#[tokio::test]
async fn an_endpoint_without_the_flag_does_nothing_at_all() {
    let log = FakeLog::default();
    let mut unflagged = customer_def();
    unflagged.open_set = false;

    let (report, visma) = due(&log, &unflagged).await;

    assert!(report.is_none());
    assert_eq!(visma.lists(), 0);
    assert!(log.started.lock().unwrap().is_empty());
}

#[tokio::test]
async fn the_first_run_is_recorded_and_sets_the_cadence() {
    let log = FakeLog::default();

    let (report, visma) = due(&log, &customer_def()).await;

    assert_eq!(report.unwrap().outcome, RunOutcome::Succeeded);
    assert_eq!(visma.lists(), 1);
    assert_eq!(log.started.lock().unwrap().len(), 1);
    let finished = log.finished.lock().unwrap().clone();
    assert_eq!(finished.len(), 1);
    assert_eq!(finished[0].0.outcome, RunOutcome::Succeeded);
    assert!(
        finished[0].0.list_metrics.len() == 1,
        "list requests are handed on for recording"
    );
}

#[tokio::test]
async fn a_second_call_in_the_same_day_does_not_run_again() {
    let log = FakeLog::default();
    due(&log, &customer_def()).await;

    let (second, visma) = due(&log, &customer_def()).await;

    assert!(second.is_none());
    assert_eq!(visma.lists(), 0);
    assert_eq!(log.started.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_run_that_is_old_enough_goes_ahead() {
    let log = FakeLog::default();
    *log.last.lock().unwrap() = Some(RunState {
        at: now() - chrono::Duration::from_std(INTERVAL).unwrap(),
        outcome: RunOutcome::Succeeded,
    });

    let (report, _) = due(&log, &customer_def()).await;

    assert!(report.is_some());
}

#[tokio::test]
async fn a_failed_run_is_recorded_as_failed_and_retried_after_two_hours_not_a_day() {
    let log = FakeLog::default();
    let visma = FakeVisma::listing(vec![]);
    let mut staging = FakeStaging::open(vec![]);
    staging.open = Err("BigQuery refused".into());
    let ports = Ports {
        visma: &visma,
        staging: &staging,
        log: &log,
    };

    let report = run_if_due(
        &ports,
        &tenant(),
        &customer_def(),
        &Limits::default(),
        now(),
        &never,
    )
    .await
    .unwrap();

    assert_eq!(report.outcome, RunOutcome::Failed, "reported, not raised");
    let last = log.last.lock().unwrap().unwrap();
    assert_eq!(last.outcome, RunOutcome::Failed);
    assert_eq!(
        last.at,
        now(),
        "the cadence counts from when the attempt began"
    );
    assert!(!is_due(Some(last), now() + chrono::Duration::minutes(119)));
    assert!(is_due(
        Some(last),
        now() + chrono::Duration::from_std(RETRY_AFTER).unwrap()
    ));
}

#[tokio::test]
async fn a_last_run_dated_far_in_the_future_does_not_hold_the_next_one_off() {
    let log = FakeLog::default();
    *log.last.lock().unwrap() = Some(RunState {
        at: now() + chrono::Duration::days(3),
        outcome: RunOutcome::Succeeded,
    });

    let (report, visma) = due(&log, &customer_def()).await;

    assert!(
        report.is_some(),
        "the clock moved: that row is not from a run that happened"
    );
    assert_eq!(visma.lists(), 1);
}

#[tokio::test]
async fn a_last_run_a_minute_ahead_of_this_clock_is_just_two_clocks_disagreeing() {
    let log = FakeLog::default();
    *log.last.lock().unwrap() = Some(RunState {
        at: now() + chrono::Duration::minutes(1),
        outcome: RunOutcome::Succeeded,
    });

    let (report, _) = due(&log, &customer_def()).await;

    assert!(report.is_none());
}

#[tokio::test]
async fn when_the_state_database_cannot_say_the_run_waits() {
    let log = FakeLog {
        fail_read: true,
        ..FakeLog::default()
    };

    let (report, visma) = due(&log, &customer_def()).await;

    assert!(report.is_none());
    assert_eq!(visma.lists(), 0);
}

#[tokio::test]
async fn without_a_record_of_the_start_there_is_no_run() {
    let log = FakeLog {
        fail_start: true,
        ..FakeLog::default()
    };

    let (report, visma) = due(&log, &customer_def()).await;

    assert!(report.is_none());
    assert_eq!(visma.lists(), 0);
}

struct SlowVisma;

#[async_trait]
impl OpenSetSource for SlowVisma {
    async fn list_open(&self, _: &TenantId, _: &EndpointDefinition) -> Result<OpenList> {
        tokio::time::sleep(Duration::from_secs(30)).await;
        Ok(OpenList::default())
    }

    async fn fetch_by_key(
        &self,
        _: &TenantId,
        _: &EndpointDefinition,
        _: &DocKey,
    ) -> Result<Option<Value>> {
        Ok(None)
    }
}

#[tokio::test]
async fn a_run_that_takes_too_long_is_stopped_and_recorded_as_failed() {
    let log = FakeLog::default();
    let staging = FakeStaging::open(vec![]);
    let ports = Ports {
        visma: &SlowVisma,
        staging: &staging,
        log: &log,
    };
    let limits = Limits {
        timeout: Duration::from_millis(50),
        ..Limits::default()
    };

    let report = run_if_due(&ports, &tenant(), &customer_def(), &limits, now(), &never)
        .await
        .unwrap();

    assert_eq!(report.outcome, RunOutcome::Failed);
    assert!(report.error.unwrap().contains("stopped"));
    assert_eq!(
        log.finished.lock().unwrap().len(),
        1,
        "the stop is recorded"
    );
}
