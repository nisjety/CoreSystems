//! Open-set reconciliation: find the documents a sync that follows timestamps cannot see change.
//!
//! A payment closes an invoice and leaves its `lastModifiedDateTime` alone, so an incremental
//! sync never fetches the invoice again and staging keeps it `Open` until the weekly full pass.
//! That left 232 invoices (8.07 M NOK) open in the old warehouse that Visma had closed.
//!
//! A part payment is the same blind spot in a smaller form: the invoice stays `Open` and keeps its
//! `lastModifiedDateTime`, only its `balance` moves, so comparing which keys are open is not
//! enough and the balances are compared too.
//!
//! For an endpoint with `open_set: true`, once a day per tenant (the cadence is kept in
//! `vsv2.sync_checkpoint`, see [`STATE_PARTITION`]), right after the endpoint's hourly sync:
//!
//! 1. the keys staging holds as open, each with its staged balance (`vsv2-bq-open-keys`: one
//!    BigQuery query, no Visma request);
//! 2. the keys Visma lists as open, with the balance of each list row
//!    (`GET <api_endpoint>?status=Open`, 1,000 per page);
//! 3. [`diff_open_sets`]: open in staging but not in Visma, open in Visma but not in staging, and
//!    open on both sides with a balance that differs or cannot be compared;
//! 4. each document of the three differences is fetched by key
//!    (`GET <api_endpoint>/<key values>`) and upserted through the normal write path (enrich,
//!    validate, `vsv2-bq-stage`);
//! 5. nothing is deleted. A document Visma answers 404 for is counted and left to the weekly
//!    complete scan, which is what marks rows deleted.
//!
//! A failure here is logged and recorded (`vsv2.sync_run`, endpoint `<name>:open_set`) and never
//! reaches the normal sync: [`run_if_due`] returns a report, not an error. The checkpoints of the
//! normal sync are never touched: a document fetched by key can carry a newer timestamp than the
//! watermark, and moving the watermark would skip the documents in between.
//!
//! This file holds the pure parts (keys, diff, cadence, request paths). The ports and the run are
//! in `open_set/reconcile.rs`, the Visma, BigQuery and Postgres sides in `open_set/adapters.rs`.

mod adapters;
mod reconcile;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::Value;
use vsv2_common::{Error, PageFetchMetric, Result};

use crate::erp_adapter::percent_encode;
use crate::registry::EndpointDefinition;

pub use adapters::PgRunLog;
pub use reconcile::{
    reconcile, run_if_due, Limits, OpenList, OpenSetSource, Ports, RunLog, StagedOpen, StagingPort,
    StopCheck,
};

/// The status value that makes a document part of the open set.
pub const OPEN_STATUS: &str = "Open";
/// The list filter that asks Visma for open documents only.
pub const STATUS_PARAM: &str = "status";
/// The field of a document that says whether it is open.
pub const STATUS_FIELD: &str = "status";
/// The field of a document that says what is still owed on it: `balance` on customer and supplier
/// invoices alike (base currency; `balanceInCurrency` is the same in the document's currency).
///
/// Both sides of the comparison read this field: the list row from Visma, and staging through
/// `vsv2-bq-open-keys` (`DEFAULT_BALANCE_FIELD` in `open_keys.py`). They have to name the same
/// one.
pub const BALANCE_FIELD: &str = "balance";
/// Two balances this close are the same balance. Money has two decimals, so a payment moves a
/// balance by at least 0.01; the tolerance only absorbs a number that went through text and a
/// float.
pub const BALANCE_TOLERANCE: f64 = 0.005;
/// After a successful run the next one waits this long.
pub const INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
/// After a failed or partial run the next attempt waits this long.
pub const RETRY_AFTER: Duration = Duration::from_secs(2 * 60 * 60);
/// The longest one run of one endpoint may take, key query, list, fetches and writes together.
///
/// The run is not a job of its own: it sits at the end of the endpoint's sync inside the hourly
/// pass, under the tenant's lock, so for this long (per flagged endpoint) the tenant's other
/// endpoints wait. A usual run is a few dozen requests and takes seconds, so the bound is short
/// on purpose; a run that needs longer (a full queue of 500 fetches can) is cut here, keeps what
/// earlier batches wrote, and goes on after [`RETRY_AFTER`]. It ends every request in it too,
/// whatever their own timeouts, which is what keeps a stuck Visma or BigQuery from holding the
/// lock.
pub const RUN_TIMEOUT: Duration = Duration::from_secs(4 * 60);
/// How far ahead of the clock a recorded run may be dated before it is not believed. A run cannot
/// have happened in the future, so such a row (the clock moved back) must not hold the next run off.
pub const CLOCK_SKEW: Duration = Duration::from_secs(5 * 60);
/// This many by-key answers, all of them 404, say that something is wrong with the path or the
/// company, not that this many documents were deleted.
pub const SYSTEMIC_NOT_FOUND: usize = 10;
/// `partition_key` of the `sync_checkpoint` row that keeps when a tenant's endpoint last ran.
///
/// No fetch partition is ever called this, so the row is never read as a watermark. `last_sync_at`
/// is when the last attempt began, `last_id` how it ended, and `records_seen` stays 0 so the sums
/// that other tools make over an endpoint's checkpoint rows are unchanged.
pub const STATE_PARTITION: &str = "_open_set";
/// Added to the endpoint name for the `sync_run` and `sync_page_metric` rows of a run.
pub const RUN_LABEL_SUFFIX: &str = ":open_set";

/// The endpoint name a run is recorded under.
pub fn run_label(endpoint: &str) -> String {
    format!("{endpoint}{RUN_LABEL_SUFFIX}")
}

/// The identity of one document: the values of the endpoint's key fields, in key order, trimmed.
///
/// Trimmed because a padded key is the same document (the gate that compares the two sets does
/// the same). Ordered so that a diff comes out in a fixed order.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DocKey(Vec<String>);

impl DocKey {
    pub fn new<I, S>(parts: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self(
            parts
                .into_iter()
                .map(|part| part.as_ref().trim().to_string())
                .collect(),
        )
    }

    pub fn parts(&self) -> &[String] {
        &self.0
    }

    /// The key of `record`: one value per entry of `fields` (dotted paths are followed).
    ///
    /// A field that is missing, null, empty or not a scalar is an error rather than an empty
    /// part: a key with a hole in it would match the wrong document.
    pub fn from_record(fields: &[String], record: &Value) -> std::result::Result<Self, String> {
        let mut parts = Vec::with_capacity(fields.len());
        for field in fields {
            let value = field
                .split('.')
                .try_fold(record, |current, name| current.get(name));
            let text = match value {
                Some(Value::String(text)) => text.trim().to_string(),
                Some(Value::Number(number)) => number.to_string(),
                Some(Value::Bool(flag)) => flag.to_string(),
                _ => String::new(),
            };
            if text.is_empty() {
                return Err(format!("key field {field} is missing, null or empty"));
            }
            parts.push(text);
        }
        Ok(Self(parts))
    }
}

impl fmt::Display for DocKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.join("|"))
    }
}

/// The key fields of an endpoint, in key order. A single comma-separated string counts as
/// several fields, as everywhere else the primary key is read.
pub fn key_fields(def: &EndpointDefinition) -> Vec<String> {
    def.primary_key_arg()
        .split(',')
        .map(str::trim)
        .filter(|field| !field.is_empty())
        .map(str::to_string)
        .collect()
}

/// One open document as one side reports it: its key, and the balance that side holds for it.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenDoc {
    pub key: DocKey,
    /// The document's [`BALANCE_FIELD`] when it has one that reads as a number; `None` is
    /// unknown (see [`diff_open_sets`]).
    pub balance: Option<f64>,
}

impl OpenDoc {
    pub fn new(key: DocKey, balance: Option<f64>) -> Self {
        Self { key, balance }
    }

    /// The document `record` describes: its key (see [`DocKey::from_record`]) and its balance.
    fn from_record(fields: &[String], record: &Value) -> std::result::Result<Self, String> {
        Ok(Self {
            key: DocKey::from_record(fields, record)?,
            balance: read_balance(record.get(BALANCE_FIELD)),
        })
    }
}

/// A balance out of a JSON value: a number, or text that holds one (the key tool prints the
/// staged balance as the text BigQuery gives for it). `None` for anything else, a missing field and
/// a null included, and for a number that is not finite.
pub fn read_balance(value: Option<&Value>) -> Option<f64> {
    let number = match value? {
        Value::Number(number) => number.as_f64()?,
        Value::String(text) => text.trim().parse::<f64>().ok()?,
        _ => return None,
    };
    number.is_finite().then_some(number)
}

/// Whether two balances are the same balance: both known, and within [`BALANCE_TOLERANCE`].
///
/// A balance that is missing on either side is unknown, and an unknown balance is equal to
/// nothing, not even to another unknown one: nothing shows that no payment was made.
fn balances_agree(staged: Option<f64>, listed: Option<f64>) -> bool {
    match (staged, listed) {
        (Some(staged), Some(listed)) => (staged - listed).abs() <= BALANCE_TOLERANCE,
        _ => false,
    }
}

/// What the two sides disagree about. Each list is sorted and holds a key once, and no key is in
/// more than one list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenSetDiff {
    /// Open in staging, not in Visma's open list: paid, closed or gone since staging last saw it.
    pub stale_in_staging: Vec<DocKey>,
    /// Open in Visma, not open in staging: never fetched, or opened again.
    pub missing_in_staging: Vec<DocKey>,
    /// Open on both sides, but the balance Visma lists is not the balance staging holds: a part
    /// payment since staging last saw the document, which leaves it `Open` with the same
    /// `lastModifiedDateTime`. It includes a balance that cannot be compared, one that is missing
    /// on either side: with nothing to show that no payment was made, the document is fetched, and
    /// the by-key answer puts the right balance in staging (or, when the document has none, the
    /// same payload that is already there, which costs one request and writes nothing).
    pub balance_differs: Vec<DocKey>,
}

impl OpenSetDiff {
    pub fn len(&self) -> usize {
        self.stale_in_staging.len() + self.missing_in_staging.len() + self.balance_differs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The balances of each key of `docs`, in key order. A key listed twice has both balances.
fn balances_by_key(docs: &[OpenDoc]) -> BTreeMap<&DocKey, Vec<Option<f64>>> {
    let mut by_key: BTreeMap<&DocKey, Vec<Option<f64>>> = BTreeMap::new();
    for doc in docs {
        by_key.entry(&doc.key).or_default().push(doc.balance);
    }
    by_key
}

/// What the two open sets do not share, by key or by balance. A key listed twice counts once;
/// a document open on both sides with the same balance is left out, since there is nothing to
/// fetch for it.
///
/// A key a side lists twice agrees only if each of its balances agrees with each of the other
/// side's: when in doubt, fetch.
pub fn diff_open_sets(staged: &[OpenDoc], listed: &[OpenDoc]) -> OpenSetDiff {
    let staged = balances_by_key(staged);
    let listed = balances_by_key(listed);
    let mut diff = OpenSetDiff::default();
    for (key, staged_balances) in &staged {
        match listed.get(key) {
            None => diff.stale_in_staging.push((*key).clone()),
            Some(listed_balances) => {
                let agree = staged_balances.iter().all(|staged| {
                    listed_balances
                        .iter()
                        .all(|listed| balances_agree(*staged, *listed))
                });
                if !agree {
                    diff.balance_differs.push((*key).clone());
                }
            }
        }
    }
    diff.missing_in_staging = listed
        .keys()
        .filter(|key| !staged.contains_key(*key))
        .map(|key| (*key).clone())
        .collect();
    diff
}

/// How many distinct keys `docs` holds.
pub(crate) fn distinct(docs: &[OpenDoc]) -> usize {
    docs.iter()
        .map(|doc| &doc.key)
        .collect::<BTreeSet<_>>()
        .len()
}

/// The documents of Visma's open list, after checking that it is one.
///
/// A list that holds a document that is not `Open` means Visma did not apply the filter. The diff
/// against staging would then claim thousands of differences, so it is an error, not a result.
pub fn docs_from_open_rows(fields: &[String], rows: &[Value]) -> Result<Vec<OpenDoc>> {
    rows.iter()
        .enumerate()
        .map(|(index, row)| {
            let status = row.get(STATUS_FIELD).and_then(Value::as_str);
            if status != Some(OPEN_STATUS) {
                return Err(Error::Other(format!(
                    "open list row {index} has {STATUS_FIELD} {status:?}, not {OPEN_STATUS:?}: \
                     Visma did not apply the filter"
                )));
            }
            OpenDoc::from_record(fields, row)
                .map_err(|why| Error::Other(format!("open list row {index}: {why}")))
        })
        .collect()
}

/// The documents `vsv2-bq-open-keys` printed: one JSON object per line, every key field in it
/// and, under [`BALANCE_FIELD`], the staged balance.
///
/// A line without a balance, or with a null one, is a document with an unknown balance: that is
/// what an older tool that printed the keys alone gives, so its output still reads.
pub fn parse_key_lines(output: &str, fields: &[String]) -> Result<Vec<OpenDoc>> {
    output
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            let row: Value = serde_json::from_str(line)
                .map_err(|e| Error::Other(format!("open keys line {}: {e}", index + 1)))?;
            OpenDoc::from_record(fields, &row)
                .map_err(|why| Error::Other(format!("open keys line {}: {why}", index + 1)))
        })
        .collect()
}

/// `<api_endpoint>/<key values, each percent-encoded, in primary key order>`.
///
/// For the invoice endpoints that is `/v1/customerinvoice/<referenceNumber>` and
/// `/v1/supplierInvoice/<documentType>/<referenceNumber>`, both read live on 2026-10-12.
pub fn by_key_path(def: &EndpointDefinition, key: &DocKey) -> Result<String> {
    let fields = key_fields(def);
    if key.parts().len() != fields.len() {
        return Err(Error::Config(format!(
            "endpoint '{}': key has {} part(s), the primary key {}",
            def.endpoint_name,
            key.parts().len(),
            fields.len()
        )));
    }
    let mut path = list_path(def)?;
    for part in key.parts() {
        // Dots are not escaped, and a path segment of dots is not a name but a way up.
        if part.is_empty() || part.chars().all(|c| c == '.') {
            return Err(Error::Config(format!(
                "endpoint '{}': key part {part:?} cannot be a path segment",
                def.endpoint_name
            )));
        }
        path.push('/');
        path.push_str(&percent_encode(part));
    }
    Ok(path)
}

/// The endpoint's collection path, without a trailing slash.
pub fn list_path(def: &EndpointDefinition) -> Result<String> {
    match def.api_endpoint.as_deref().map(str::trim) {
        Some(path) if !path.is_empty() => Ok(path.trim_end_matches('/').to_string()),
        _ => Err(Error::Config(format!(
            "endpoint '{}' has no api_endpoint",
            def.endpoint_name
        ))),
    }
}

/// `default_query` as the request's query string pairs, in key order. Arrays repeat the key.
///
/// The by-key request carries the same `expand*` flags as the list, so the stored document has the
/// same shape whichever way it was fetched.
pub fn query_pairs(default_query: &HashMap<String, Value>) -> Result<Vec<(String, String)>> {
    let mut names: Vec<&String> = default_query.keys().collect();
    names.sort();
    let mut pairs = Vec::new();
    for name in names {
        push_query_value(&mut pairs, name, &default_query[name])?;
    }
    Ok(pairs)
}

fn push_query_value(pairs: &mut Vec<(String, String)>, name: &str, value: &Value) -> Result<()> {
    match value {
        Value::Null => {}
        Value::String(text) => pairs.push((name.to_string(), text.clone())),
        Value::Bool(flag) => pairs.push((name.to_string(), flag.to_string())),
        Value::Number(number) => pairs.push((name.to_string(), number.to_string())),
        Value::Array(items) => {
            for item in items {
                push_query_value(pairs, name, item)?;
            }
        }
        Value::Object(_) => {
            return Err(Error::Config(format!(
                "default_query.{name} must be a scalar or an array of scalars"
            )))
        }
    }
    Ok(())
}

/// How a run ended. Kept in the cadence row, so it decides when the next attempt is due.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RunOutcome {
    /// Every difference was resolved.
    #[default]
    Succeeded,
    /// Some differences are left: a cap, a fetch that failed, a document that did not validate.
    Partial,
    /// Differences are left and asking again soon would get the same answers: nothing the run
    /// asked for changed a row of staging (every document was 404, did not validate, or came back
    /// as it already was staged) and no request failed.
    Stalled,
    /// Nothing was reconciled: staging, Visma or the write failed, or the run did not finish.
    Failed,
}

impl RunOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Partial => "partial",
            Self::Stalled => "stalled",
            Self::Failed => "failed",
        }
    }

    /// What the cadence row's `last_id` says.
    ///
    /// A row without a marker counts as a success: nothing says it went wrong. A marker this
    /// version does not write (a damaged row, one written by another version) counts as a
    /// failure, so the run is made again after [`RETRY_AFTER`]: repeating a run costs a few
    /// requests, while believing a success that was never recorded could leave documents stale for
    /// a day.
    pub fn from_marker(marker: Option<&str>) -> Self {
        match marker {
            None | Some("succeeded") => Self::Succeeded,
            Some("partial") => Self::Partial,
            Some("stalled") => Self::Stalled,
            Some("failed") => Self::Failed,
            Some(_) => Self::Failed,
        }
    }
}

/// When the last attempt for a tenant and endpoint ended, and how.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunState {
    pub at: DateTime<Utc>,
    pub outcome: RunOutcome,
}

/// Whether a run is due: never ran, or the wait after the last one is over.
///
/// A success waits [`INTERVAL`], so a restart or a second scheduler does not repeat the run. A run
/// that stalled waits as long: nothing it asked for changed a row, so asking again would get the
/// same answers (and a diff that is a no-op for good would otherwise ask for its 500 documents
/// twelve times a day). A failure or a partial run waits only [`RETRY_AFTER`], so what it left is
/// not stale for a day. A last run dated more than [`CLOCK_SKEW`] ahead of `now` did not happen
/// (the clock moved back) and does not hold the next one off; see [`is_from_the_future`].
pub fn is_due(last: Option<RunState>, now: DateTime<Utc>) -> bool {
    let Some(last) = last else { return true };
    if is_from_the_future(&last, now) {
        return true;
    }
    let wait = match last.outcome {
        RunOutcome::Succeeded | RunOutcome::Stalled => INTERVAL,
        RunOutcome::Partial | RunOutcome::Failed => RETRY_AFTER,
    };
    (now - last.at).to_std().is_ok_and(|age| age >= wait)
}

/// Whether `last` is dated further ahead of `now` than the clocks may differ.
pub fn is_from_the_future(last: &RunState, now: DateTime<Utc>) -> bool {
    chrono::Duration::from_std(CLOCK_SKEW).is_ok_and(|skew| last.at - now > skew)
}

/// What one run did. Counts are distinct documents unless the name says requests.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenSetReport {
    pub outcome: RunOutcome,
    /// Keys staging holds as open.
    pub staging_open: usize,
    /// Keys Visma lists as open.
    pub visma_open: usize,
    /// Open in staging, absent from Visma's list.
    pub stale_in_staging: usize,
    /// Open in Visma, not open in staging.
    pub missing_in_staging: usize,
    /// Open on both sides, with a balance that differs or cannot be compared (a part payment).
    pub balance_differs: usize,
    /// Documents fetched by key that were accepted for writing.
    pub fetched: usize,
    /// Rows the staging MERGE inserted or updated. 0 for documents that came back as they were
    /// already staged.
    pub written: u64,
    /// Documents Visma answered 404 for: gone, left to the weekly complete scan.
    pub not_found: usize,
    /// By-key fetches that failed or answered for another document.
    pub fetch_failed: usize,
    /// Fetched documents the validation rejected (quarantined, not written).
    pub rejected: usize,
    /// Differences left for a later run: over the cap, or after repeated failures.
    pub deferred: usize,
    /// List requests made (pages of the open list, walks that went again included).
    pub list_requests: usize,
    /// By-key requests attempted (one the rate limiter refused counts).
    pub by_key_requests: usize,
    /// Bytes BigQuery processed for the open keys, when the tool said.
    pub bq_bytes: Option<u64>,
    /// One metric per list request, for `vsv2.sync_page_metric`.
    pub list_metrics: Vec<PageFetchMetric>,
    /// What went wrong, for a run that did not succeed.
    pub error: Option<String>,
}

impl OpenSetReport {
    /// A run that did nothing and failed.
    pub fn failed(error: impl Into<String>) -> Self {
        Self {
            outcome: RunOutcome::Failed,
            error: Some(error.into()),
            ..Self::default()
        }
    }

    /// Documents the two sides disagreed about.
    pub fn differences(&self) -> usize {
        self.stale_in_staging + self.missing_in_staging + self.balance_differs
    }

    /// Append `note` to what went wrong.
    pub(crate) fn note(&mut self, note: impl AsRef<str>) {
        match &mut self.error {
            Some(error) => {
                error.push_str("; ");
                error.push_str(note.as_ref());
            }
            None => self.error = Some(note.as_ref().to_string()),
        }
    }
}

#[cfg(test)]
pub(crate) mod fixtures;
#[cfg(test)]
mod tests;
