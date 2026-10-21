//! The ZDR run tombstone (WS4.J): a metadata-only record that a run happened under
//! Zero Data Retention, sent to quarry-control (`POST /v1/internal/zdr-tombstones`).
//!
//! **OFF BY DEFAULT, and counsel review gates turning it on.** Nothing here runs
//! unless `QUARRY_EDGE__ZDR_TOMBSTONE=1` is set, and no env file, compose file or
//! default sets it. A tombstone is a new egress path under ZDR: it retains the fact
//! that a ZDR run happened (who, when, how it ended), which is metadata retention
//! that each customer's ZDR terms may or may not allow. Building it is not enabling
//! it.
//!
//! What a tombstone may carry is closed by construction and by contract:
//!
//! - [`Tombstone`] is a typed struct. The edge serializes that struct and nothing
//!   else, so it cannot name a field outside the allowlist; its keys, vocabularies
//!   and required set are tested equal to
//!   `crates/quarry-runtime/contracts/zdr-tombstone.schema.json`
//!   (`tests/zdr_tombstone.rs`). Ids, closed-vocabulary classes, counts and
//!   timestamps only: never a URL, host, title, query, user id, content, digest,
//!   fingerprint or error text.
//! - The constructor refuses an id outside `^[A-Za-z0-9_-]{1,64}$`, so a run or org
//!   id that is not id-shaped yields no tombstone rather than a free-text one.
//! - quarry-control repeats the check (typed decode with unknown fields refused, any
//!   `://` refused, closed vocabularies) and the table's CHECK constraints repeat it
//!   again.
//!
//! Emission:
//!
//! - **Only when ZDR is in effect.** [`emit_run_end`] returns at once for a standard
//!   run, so a standard run emits nothing and the table can only hold `zdr` rows.
//! - **Fail open and bounded.** Emission is a spawned task with a 2 second timeout
//!   behind a 64-permit semaphore. When the permits are gone the tombstone is
//!   dropped and counted; nothing is awaited by the request. A lost tombstone loses
//!   audit evidence; it enforces nothing, and it can never fail a request.
//! - **Counters, not content, in the log.** A failed or dropped delivery logs a
//!   class and nothing else (the log-field lint enforces it).
//! - **Once per run.** A scrape (and search, answer, extract) emits once at the end
//!   of its handler; an agent run emits only from the `close_run` path that wins the
//!   removal. quarry-control stores `ON CONFLICT (run_id) DO NOTHING`, so a retry is
//!   idempotent.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, SecondsFormat, Utc};
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::output::DriverKind;
use serde::Serialize;
use tokio::sync::Semaphore;
use tracing::Instrument;

use crate::internal_auth::{self, InternalSigner};

/// quarry-control's receiver (signed, HMAC group).
pub const PATH: &str = "/v1/internal/zdr-tombstones";

/// The env var that turns emission on (`QUARRY_EDGE__ZDR_TOMBSTONE`, read into
/// `EdgeConfig::zdr_tombstone`). Off unless set to a true value.
pub const FLAG_ENV: &str = "QUARRY_EDGE__ZDR_TOMBSTONE";

/// The only retention class a tombstone carries.
pub const RETENTION_CLASS: &str = "zdr";

/// How long one delivery may take before it is abandoned.
pub const SEND_TIMEOUT: Duration = Duration::from_secs(2);

/// Deliveries in flight at once; the next one is dropped, never queued.
pub const MAX_IN_FLIGHT: usize = 64;

/// What kind of run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Scrape,
    Search,
    Answer,
    Extract,
    AgentRun,
}

/// How the run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Ok,
    Error,
    Refused,
    Cancelled,
    Timeout,
}

impl Outcome {
    /// The outcome an error maps to. A timeout is a timeout; a policy refusal
    /// (permission, security, unsupported, budget) is `refused`; the user taking a
    /// run over is `cancelled`; everything else is `error`.
    #[must_use]
    pub fn from_error(error: &QuarryError) -> Self {
        match error.code {
            ErrorCode::Timeout => Self::Timeout,
            ErrorCode::Forbidden
            | ErrorCode::SecurityBlocked
            | ErrorCode::Unsupported
            | ErrorCode::UnsupportedMedia
            | ErrorCode::BudgetExhausted => Self::Refused,
            ErrorCode::UserTookOver => Self::Cancelled,
            _ => Self::Error,
        }
    }
}

/// One tombstone: exactly the keys of `zdr-tombstone.schema.json`, nothing more.
/// Adding a field here without the schema (or the reverse) fails
/// `tests/zdr_tombstone.rs`.
#[derive(Debug, Clone, Serialize)]
pub struct Tombstone {
    run_id: String,
    org_id: String,
    kind: Kind,
    retention_class: &'static str,
    outcome: Outcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    driver: Option<DriverKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    started_at: Option<String>,
    ended_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    step_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cost_units: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_class: Option<&'static str>,
}

/// `^[A-Za-z0-9_-]{1,64}$`: the shape of a run id or an org id.
fn is_id(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn rfc3339(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Millis, true)
}

impl Tombstone {
    /// `None` when the run id or the org id is not id-shaped: such a run gets no
    /// tombstone (the closed-vocabulary rule has no free-text fallback).
    #[must_use]
    pub fn new(
        run_id: &str,
        org_id: &str,
        kind: Kind,
        outcome: Outcome,
        ended_at: DateTime<Utc>,
    ) -> Option<Self> {
        if !is_id(run_id) || !is_id(org_id) {
            return None;
        }
        Some(Self {
            run_id: run_id.to_owned(),
            org_id: org_id.to_owned(),
            kind,
            retention_class: RETENTION_CLASS,
            outcome,
            driver: None,
            started_at: None,
            ended_at: rfc3339(ended_at),
            step_count: None,
            cost_units: None,
            error_class: None,
        })
    }

    #[must_use]
    pub fn with_driver(mut self, driver: Option<DriverKind>) -> Self {
        self.driver = driver;
        self
    }

    #[must_use]
    pub fn with_started_at(mut self, at: DateTime<Utc>) -> Self {
        self.started_at = Some(rfc3339(at));
        self
    }

    #[must_use]
    pub fn with_step_count(mut self, steps: u32) -> Self {
        self.step_count = Some(steps);
        self
    }

    /// A non-finite or negative cost is dropped rather than sent.
    #[must_use]
    pub fn with_cost_units(mut self, cost: f64) -> Self {
        self.cost_units = (cost.is_finite() && cost >= 0.0 && cost < 1e11).then_some(cost);
        self
    }

    /// The closed-vocabulary class of the error that ended the run (its wire code),
    /// never its message.
    #[must_use]
    pub fn with_error(mut self, error: &QuarryError) -> Self {
        self.error_class = Some(crate::zdr_log::class(error));
        self
    }

    #[must_use]
    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    /// The exact bytes that go on the wire.
    ///
    /// # Panics
    /// Never in practice: the struct holds only strings, numbers and enums.
    #[must_use]
    pub fn to_body(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("a tombstone is plain data and always serializes")
    }
}

/// Why a delivery did not land. Carries a class, never a body or a URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkError {
    /// The transport failed (class from `zdr_log::class_reqwest`).
    Transport(&'static str),
    /// quarry-control answered something other than success.
    Status(u16),
}

/// Where a tombstone goes. The HTTP implementation signs and posts to quarry-control;
/// tests capture.
#[async_trait]
pub trait TombstoneSink: Send + Sync {
    async fn put(&self, tombstone: &Tombstone) -> Result<(), SinkError>;
}

/// Delivers to quarry-control over HMAC-signed HTTP.
pub struct HttpSink {
    client: reqwest::Client,
    base: String,
    signer: Arc<InternalSigner>,
}

impl HttpSink {
    /// `None` when the client cannot be built.
    #[must_use]
    pub fn new(control_base_url: &str, signer: Arc<InternalSigner>) -> Option<Self> {
        let client = reqwest::Client::builder().timeout(SEND_TIMEOUT).build().ok()?;
        Some(Self {
            client,
            base: control_base_url.trim_end_matches('/').to_owned(),
            signer,
        })
    }
}

#[async_trait]
impl TombstoneSink for HttpSink {
    async fn put(&self, tombstone: &Tombstone) -> Result<(), SinkError> {
        let body = tombstone.to_body();
        let signed = internal_auth::apply_to_request(&self.signer, "POST", PATH, &body);
        let response = self
            .client
            .post(format!("{}{PATH}", self.base))
            .header(internal_auth::HEADER_SIG, signed.signature)
            .header(internal_auth::HEADER_TS, signed.timestamp)
            .header(internal_auth::HEADER_NONCE, signed.nonce)
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await
            .map_err(|error| SinkError::Transport(crate::zdr_log::class_reqwest(&error)))?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(SinkError::Status(response.status().as_u16()))
        }
    }
}

/// Counters an operator or a test can read. Plain counts: safe to log.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counters {
    pub sent: u64,
    pub failed: u64,
    pub dropped: u64,
}

/// Owns the sink, the in-flight bound and the counters. One per edge process
/// (`AppState::tombstones`), `None` while the flag is off.
pub struct TombstoneEmitter {
    sink: Arc<dyn TombstoneSink>,
    permits: Arc<Semaphore>,
    sent: Arc<AtomicU64>,
    failed: Arc<AtomicU64>,
    dropped: AtomicU64,
}

impl TombstoneEmitter {
    #[must_use]
    pub fn new(sink: Arc<dyn TombstoneSink>) -> Self {
        Self::with_capacity(sink, MAX_IN_FLIGHT)
    }

    #[must_use]
    pub fn with_capacity(sink: Arc<dyn TombstoneSink>, in_flight: usize) -> Self {
        Self {
            sink,
            permits: Arc::new(Semaphore::new(in_flight)),
            sent: Arc::new(AtomicU64::new(0)),
            failed: Arc::new(AtomicU64::new(0)),
            dropped: AtomicU64::new(0),
        }
    }

    #[must_use]
    pub fn counters(&self) -> Counters {
        Counters {
            sent: self.sent.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
            dropped: self.dropped.load(Ordering::Relaxed),
        }
    }

    /// Hands the tombstone to a spawned task and returns at once. Never awaits the
    /// sink, never errors: with every permit taken it counts a drop and returns.
    pub fn emit(&self, tombstone: Tombstone) {
        let Ok(permit) = Arc::clone(&self.permits).try_acquire_owned() else {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            tracing::warn!(reason = "in_flight_cap", "zdr tombstone dropped");
            return;
        };
        let sink = Arc::clone(&self.sink);
        let sent = Arc::clone(&self.sent);
        let failed = Arc::clone(&self.failed);
        tokio::spawn(
            async move {
                let _permit = permit;
                match tokio::time::timeout(SEND_TIMEOUT, sink.put(&tombstone)).await {
                    Ok(Ok(())) => {
                        sent.fetch_add(1, Ordering::Relaxed);
                    }
                    Ok(Err(error)) => {
                        failed.fetch_add(1, Ordering::Relaxed);
                        match error {
                            SinkError::Transport(class) => {
                                tracing::warn!(error_class = class, "zdr tombstone delivery failed");
                            }
                            SinkError::Status(status) => {
                                tracing::warn!(status, "zdr tombstone delivery refused");
                            }
                        }
                    }
                    Err(_) => {
                        failed.fetch_add(1, Ordering::Relaxed);
                        tracing::warn!(error_class = "timeout", "zdr tombstone delivery failed");
                    }
                }
            }
            .in_current_span(),
        );
    }
}

/// What a handler knows when a run ends.
pub struct RunEnd<'a> {
    run_id: &'a str,
    org_id: &'a str,
    kind: Kind,
    started_at: Option<DateTime<Utc>>,
    driver: Option<DriverKind>,
    step_count: Option<u32>,
    cost_units: Option<f64>,
    /// `None` is a successful run (unless `outcome` says otherwise).
    error: Option<&'a QuarryError>,
    /// An outcome the caller states itself (a run that was closed or expired has no error).
    outcome: Option<Outcome>,
}

impl<'a> RunEnd<'a> {
    #[must_use]
    pub fn new(run_id: &'a str, org_id: &'a str, kind: Kind) -> Self {
        Self {
            run_id,
            org_id,
            kind,
            started_at: None,
            driver: None,
            step_count: None,
            cost_units: None,
            error: None,
            outcome: None,
        }
    }

    #[must_use]
    pub fn started(mut self, at: DateTime<Utc>) -> Self {
        self.started_at = Some(at);
        self
    }

    /// How the run ended when there is no error to say it (a closed or expired run).
    #[must_use]
    pub fn ended_as(mut self, outcome: Outcome) -> Self {
        self.outcome = Some(outcome);
        self
    }

    #[must_use]
    pub fn driver(mut self, driver: DriverKind) -> Self {
        self.driver = Some(driver);
        self
    }

    #[must_use]
    pub fn steps(mut self, steps: u32) -> Self {
        self.step_count = Some(steps);
        self
    }

    #[must_use]
    pub fn cost(mut self, cost_units: f64) -> Self {
        self.cost_units = Some(cost_units);
        self
    }

    /// The run ended with this error (its class goes in the tombstone, never its text).
    #[must_use]
    pub fn failed(mut self, error: &'a QuarryError) -> Self {
        self.error = Some(error);
        self
    }
}

/// A run in progress whose tombstone is sent when the handler's response is known: the
/// seam for the request/response routes (search, answer, extract) whose handlers return
/// from many places. `begin` returns `None` (and costs nothing) unless an emitter is
/// configured AND the run is under ZDR.
pub struct Pending {
    emitter: Arc<TombstoneEmitter>,
    org_id: String,
    run_id: String,
    kind: Kind,
    started_at: DateTime<Utc>,
}

/// Statuses that mean no run took place (the request was refused before any work), so
/// there is nothing to record: bad request, not found, method not allowed, too large,
/// unprocessable.
#[must_use]
pub fn status_means_no_run(status: u16) -> bool {
    matches!(status, 400 | 404 | 405 | 413 | 422)
}

/// The outcome and closed-vocabulary class of an HTTP status. The class is the
/// `ErrorCode` wire name that status maps to; it is derived from the status alone, so it
/// can be coarser than the handler's own error (502 is `DRIVER_FAILED` whether the cause
/// was a driver or an upstream block).
#[must_use]
pub fn outcome_of_status(status: u16) -> (Outcome, Option<&'static str>) {
    match status {
        200..=399 => (Outcome::Ok, None),
        402 => (Outcome::Refused, Some("BUDGET_EXHAUSTED")),
        401 => (Outcome::Error, Some("UNAUTHORIZED")),
        403 => (Outcome::Refused, Some("FORBIDDEN")),
        408 | 504 => (Outcome::Timeout, Some("TIMEOUT")),
        409 => (Outcome::Error, Some("CONFLICT")),
        415 => (Outcome::Refused, Some("UNSUPPORTED_MEDIA")),
        429 => (Outcome::Error, Some("RATE_LIMITED")),
        501 => (Outcome::Refused, Some("UNSUPPORTED")),
        502 => (Outcome::Error, Some("DRIVER_FAILED")),
        503 => (Outcome::Error, Some("RUNTIME_NOT_READY")),
        _ => (Outcome::Error, Some("INTERNAL")),
    }
}

impl Pending {
    /// Starts the clock for a run. `None` when no emitter is configured, when the run
    /// is not under ZDR, or when the org id is not id-shaped.
    #[must_use]
    pub fn begin(
        emitter: Option<&Arc<TombstoneEmitter>>,
        zdr_active: bool,
        org_id: &str,
        kind: Kind,
    ) -> Option<Self> {
        let emitter = emitter?;
        if !zdr_active || !is_id(org_id) {
            return None;
        }
        Some(Self {
            emitter: Arc::clone(emitter),
            org_id: org_id.to_owned(),
            run_id: format!("run_{}", uuid::Uuid::new_v4().simple()),
            kind,
            started_at: Utc::now(),
        })
    }

    /// Sends the tombstone for the response status the handler produced (nothing for a
    /// status that means no run took place).
    pub fn finish_status(self, status: u16) {
        if status_means_no_run(status) {
            return;
        }
        let (outcome, class) = outcome_of_status(status);
        let Some(mut tombstone) =
            Tombstone::new(&self.run_id, &self.org_id, self.kind, outcome, Utc::now())
        else {
            return;
        };
        tombstone = tombstone.with_started_at(self.started_at);
        tombstone.error_class = class;
        self.emitter.emit(tombstone);
    }
}

/// Emits the tombstone for a run that has just ended, when (and only when) the run
/// was under ZDR and the emitter is configured. Returns at once otherwise. Call it
/// once per run, from the code path that ends the run.
pub fn emit_run_end(emitter: Option<&Arc<TombstoneEmitter>>, zdr_active: bool, end: &RunEnd<'_>) {
    let Some(emitter) = emitter else { return };
    if !zdr_active {
        return;
    }
    let outcome = end
        .outcome
        .or_else(|| end.error.map(Outcome::from_error))
        .unwrap_or(Outcome::Ok);
    let Some(mut tombstone) = Tombstone::new(end.run_id, end.org_id, end.kind, outcome, Utc::now())
    else {
        tracing::debug!(reason = "id_shape", "zdr tombstone not built");
        return;
    };
    if let Some(started_at) = end.started_at {
        tombstone = tombstone.with_started_at(started_at);
    }
    tombstone = tombstone.with_driver(end.driver);
    if let Some(steps) = end.step_count {
        tombstone = tombstone.with_step_count(steps);
    }
    if let Some(cost) = end.cost_units {
        tombstone = tombstone.with_cost_units(cost);
    }
    if let Some(error) = end.error {
        tombstone = tombstone.with_error(error);
    }
    emitter.emit(tombstone);
}
