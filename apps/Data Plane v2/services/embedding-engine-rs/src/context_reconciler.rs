//! The context reconciler (CR-T9a): the loop that makes the database true.
//!
//! Contextual retrieval has two halves, a context and a vector built from it, and several ways
//! to end up with one without the other: the inline phase ran out of time, the provider was
//! throttled, a batch died between writing the context and marking the unit done, a context was
//! written by code that kept no provenance, a prompt was rolled out. The pipeline
//! ([`crate::batch`]) makes a unit right when a unit is embedded. The reconciler makes units
//! right that were not, by asking the database what is wrong and running the SAME pipeline on
//! exactly those units: there is one implementation of "make these units' contexts and vectors
//! right", not two that agree for now.
//!
//! ## What it does
//!
//! Per pass, in this order, and each only when it is allowed to:
//!
//! 1. **Gives up** on the units that failed `CONTEXT_RECONCILER_MAX_ATTEMPTS` times: they
//!    become `failed_permanent` with their class, and an alert is logged (a posture or
//!    credential problem never counted as an attempt, so a unit is only here because the work
//!    itself kept failing).
//! 2. **Rechecks zero retention**, at most once an hour per organization, by minting a bearer
//!    and reading its `zdr` claim: an organization that is no longer zero-retention has its
//!    skipped units made `pending`; one that still is costs one mint and no write.
//! 3. **Leases** units, an organization at a time, in turns, within a per-organization and a
//!    global cap, and works on them a document at a time within a per-organization and a
//!    global concurrency:
//!    * [`Kind::Generate`]: no usable context (`pending`, a retryable failure that is due, a
//!      unit nobody gave a status, one skipped by a policy that was lifted). The pipeline
//!      reuses, generates, writes the context ahead, embeds and marks done.
//!    * [`Kind::Verify`]: a `legacy` unit. Its stored context is embedded, no model is asked,
//!      and the unit becomes `ready` in the statement that proves the vector reflects it.
//!    * [`Kind::Repair`]: a `ready` unit whose recorded vector hash is not the hash of its
//!      stored context. Embedded again from the stored context, no model.
//!    * [`Kind::Stale`] (off unless asked): a document changed after a context was made; each
//!      chunk's input hash decides whether the context is regenerated.
//!    * [`Kind::Refresh`] (off unless asked, always capped): a context made with another prompt
//!      version than the one in force.
//! 4. **Announces** `dataplane.documents.indexed` for a document if, and only if, a context was
//!    written for one of its units: the keyword index reads the context from the database on
//!    that announcement, and graph extraction runs on it too, so an announcement that changes
//!    nothing is a cost (see [`held`]).
//!
//! ## What it never does
//!
//! * It never touches a restricted document, or an organization whose policy cannot be read, and
//!   it asks no model for an organization whose policy is off. The policy is read on every pass
//!   (through the same 30-second cache the pipeline uses).
//! * It never writes a unit after the pipeline marked it done, except to give up on it: the
//!   stale-embedding detector reads `updated_at > embedded_at` as "changed after it was
//!   embedded". A lease is written before, and never released (it expires); a failure of a
//!   re-embed is remembered in memory ([`pacing::Cooldown`]); the legacy promotion is part of
//!   the pipeline's own final statement.
//! * It never regenerates a context (the stale and refresh kinds) for an organization that is
//!   zero-retention now, or whose posture Auth Core could not resolve: such a regeneration cannot
//!   be made, the pipeline still embeds every unit of a batch, and a `ready` unit stays
//!   selectable for as long as its context is of another prompt version, so the same units
//!   would be sent to the embedding model again and again. A zero-retention organization's
//!   units are not selected for it at all ([`Reconciler::may_regenerate`]).
//! * It never burns an attempt on a posture, credential or deadline problem, and it never lets
//!   one provider failure turn into a storm: a circuit breaker stops it for five minutes after
//!   a run of passes in which the provider failed everything, and at once on a refused
//!   credential.
//!
//! ## The background budget (CR-T9b)
//!
//! Before it leases a unit that a model would be asked about, the reconciler asks the process's
//! budget gate ([`crate::budget`]) what the organization's background LLM budget says. The default
//! (alert only, no figure) says nothing and reads nothing. Only when an operator set a mode:
//!
//! * **`stop`** (and the budget is used up): units that have no status yet are leased once, which
//!   is how the pipeline marks them `skipped_budget` (an explicit mark, no provider call, no
//!   bearer, a vector from the raw text); units already marked are not leased at all, so a stopped
//!   organization costs one cheap statement a pass and not an embedding a pass. Stale and refresh
//!   regenerations are not leased either: a context a unit has stays in force. The pass reports the
//!   organization as held ([`PassReport::budget_held`]), which is how `context-backfill` tells an
//!   operator why a run did nothing.
//! * **`throttle`**: one unit per pass is leased for a model, whatever the pass has room for. The
//!   pace of the passes is the throttle.
//! * Lifting is not an event: a `skipped_budget` unit is work again at the first pass in which the
//!   gate lets the organization through (a new month, a raised figure, another mode, a removed
//!   row), exactly as a `skipped_policy` unit is when the policy is lifted.
//!
//! ## Switches
//!
//! `CONTEXT_RECONCILER_ENABLED` (off), the incident kill switch `CONTEXTUAL_RETRIEVAL_ENABLED`
//! (off takes away the model, and with it Generate, Stale and Refresh, and the budget gate has
//! nothing to hold; the re-embeds go on), a pause file checked at the start of every pass
//! (`touch` it inside the container, no restart), the breaker, the per-organization policy and the
//! background budget (`BACKGROUND_BUDGET_*`). Every one has a default and a check at startup
//! ([`Settings::from_config`], [`crate::config::Config::background_budget`]) and a line in the
//! runbook.

// The operator's tool (`context-backfill`, `src/bin/context_backfill.rs`) compiles this module
// tree too and is the one that calls the dry run and the backfill: the service does not, and the
// allowance says so once.
//
// Every child names its file. This file is also loaded by that tool with a `#[path]` attribute,
// and a file loaded that way looks for its children beside itself (`src/`), not in the directory
// named after it; an explicit path is the same file in both builds.
#[allow(dead_code)]
#[path = "context_reconciler/backfill.rs"]
mod backfill;
#[allow(dead_code)]
#[path = "context_reconciler/cli.rs"]
pub(crate) mod cli;
#[allow(dead_code)]
#[path = "context_reconciler/estimate.rs"]
mod estimate;
#[path = "context_reconciler/held.rs"]
mod held;
#[path = "context_reconciler/pacing.rs"]
mod pacing;
#[path = "context_reconciler/select.rs"]
pub(crate) mod select;
#[path = "context_reconciler/settings.rs"]
mod settings;
#[cfg(test)]
#[path = "context_reconciler/tests.rs"]
mod tests;

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use anyhow::Context as _;
use event_envelope_rs::EventSigner;
use futures::stream::{FuturesUnordered, StreamExt};
use inference_client_rs::InferenceError;
use sqlx::PgPool;
use usage_ledger_rs::LedgerSink;

use crate::batch::{self, BatchItem, Embedder, EventSink, PassKind, Pipeline, VectorSink};
use crate::budget::BudgetGate;
use crate::context::outcome::outcome_for_failure;
use crate::context::{
    ChunkInput, ContextOutcome, DocumentInput, DocumentMeta, ErrorClass, PromptVersion,
};
use crate::policy::{OrgSetting, PolicyReader};
use crate::provider::contextualize::Contextualizer;

#[allow(unused_imports)]
pub(crate) use backfill::{dry_run, Backfill, BackfillReport};
#[allow(unused_imports)]
pub(crate) use estimate::Prices;
use held::HeldSink;
use pacing::{Breaker, Clock, Cooldown, PassHealth, SystemClock};
use select::{Kind, LeaseRequest, Leased, UnitState};
pub(crate) use settings::{contextualizer_config, Settings};

/// What the reconciler runs on. Every port is the pipeline's own (so a test runs the real
/// pipeline over the real database with recording fakes at the three places that cannot be
/// asked what they were sent), and the contextualizer is the reconciler's own instance of the
/// real one.
pub(crate) struct Deps {
    pub pool: PgPool,
    pub provider: Arc<dyn Embedder>,
    pub vectors: Arc<dyn VectorSink>,
    pub events: Arc<dyn EventSink>,
    pub signer: Option<Arc<EventSigner>>,
    pub collection: String,
    /// `None` while the incident kill switch is on (`CONTEXTUAL_RETRIEVAL_ENABLED=false`): no
    /// model is asked, and the work that needs one is not leased.
    pub engine: Option<Contextualizer>,
    pub policy: Arc<PolicyReader>,
    /// The background LLM budget gate (CR-T9b): the same one the stream consumer's pipeline
    /// asks, so that what either of them spends is in the figure the other reads.
    pub budget: Arc<BudgetGate>,
    /// The usage ledger, when it is on (`USAGE_EMIT_ENABLED=1`).
    pub usage: Option<Arc<dyn LedgerSink>>,
}

/// Why a pass did nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Paused {
    /// The pause file exists.
    File,
    /// The breaker is open, for this much longer.
    Breaker(Duration),
}

/// What one pass did. Counts and class names; never a document, a chunk or a context.
#[derive(Clone, Debug, Default)]
pub(crate) struct PassReport {
    pub paused: Option<Paused>,
    /// Units leased, by kind.
    pub leased: BTreeMap<&'static str, usize>,
    /// Documents worked on.
    pub documents: usize,
    /// Units a context was written for.
    pub generated: usize,
    /// `legacy` units that are `ready` now.
    pub verified: usize,
    /// Vectors that are the ones their stored context makes now.
    pub repaired: usize,
    /// Units that were worked on and are still not right.
    pub unresolved: usize,
    /// `documents.indexed` announcements published, and held back.
    pub announced: usize,
    pub withheld: usize,
    /// Units given up on: (organization, error class, units).
    pub given_up: Vec<(String, String, i64)>,
    /// Units of organizations that are no longer zero-retention, made `pending` again.
    pub zero_retention_released: u64,
    /// Organizations whose background budget is used up in `stop` mode and that it is holding
    /// something back from this pass: units it was given to mark `skipped_budget`, or units it
    /// marked before (CR-T9b). An organization that is stopped and has nothing to generate is not
    /// counted. The gate logs and counts the stop itself; this is what lets `context-backfill`
    /// tell an operator why a run did nothing.
    pub budget_held: usize,
    /// Organizations whose zero-retention was asked about.
    pub zero_retention_checked: usize,
    pub health: Option<PassHealth>,
    /// The longest a provider asked to be left alone.
    pub retry_after: Option<Duration>,
    /// Statements or jobs that failed outright (the database was unreachable, say).
    pub errors: usize,
}

impl PassReport {
    pub(crate) fn leased_total(&self) -> usize {
        self.leased.values().sum()
    }

    /// Whether the pass found anything to do (so that the next may follow it at once).
    pub(crate) fn worked(&self) -> bool {
        self.leased_total() > 0 || !self.given_up.is_empty() || self.zero_retention_released > 0
    }
}

/// What a pass is restricted to. The default is the daemon's: every organization, on the
/// sweep's schedule.
#[derive(Clone, Debug, Default)]
pub(crate) struct Scope {
    /// Only these organizations: the operator's backfill names one, and the tests name the ones
    /// they made (the database they share holds other tests' work).
    pub orgs: Option<Vec<String>>,
    /// At most this many units, a cap tighter than the settings.
    pub max_units: Option<usize>,
    /// Look at everything now instead of on the sweep's schedule (the backfill).
    pub eager: bool,
}

struct Sweep {
    at: Instant,
    /// Every organization that has eligible units (the stale pass walks them).
    all: Vec<String>,
    /// Organizations with a vector that is not the one its stored context makes.
    repair: HashSet<String>,
    /// Organizations with `skipped_policy` units.
    policy: HashSet<String>,
    /// Organizations with `skipped_budget` units: the ones a stop is holding something back from,
    /// which is what makes a pass say so (`PassReport::budget_held`).
    budget: HashSet<String>,
    /// Organizations with a context of another prompt version than the one in force for them.
    refresh: HashSet<String>,
}

struct State {
    breaker: Breaker,
    cooldown: Cooldown,
    rotation: usize,
    sweep: Option<Sweep>,
    /// When each organization's zero-retention is next asked about.
    zero_retention_next: HashMap<String, Instant>,
    /// The organizations that have zero-retention skips, and when that list was read.
    zero_retention_orgs: Option<(Instant, Vec<String>)>,
    /// Units this process may still refresh.
    refresh_left: Option<u64>,
    /// Where the stale pass is in each organization's documents, and when it starts again.
    stale_cursor: HashMap<String, String>,
    stale_next: HashMap<String, Instant>,
    /// Units already reported as unverifiable, so that the alert is raised once.
    alerted: HashSet<String>,
}

pub(crate) struct Reconciler {
    deps: Deps,
    settings: Settings,
    clock: Arc<dyn Clock>,
    state: Mutex<State>,
}

/// A group of one document's units that one run of the pipeline serves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Group {
    /// Generate and Stale: the model is asked for what has no usable context.
    Generate,
    Refresh,
    Verify,
    Repair,
}

impl Group {
    fn of(kind: Kind) -> Self {
        match kind {
            Kind::Generate | Kind::Stale => Self::Generate,
            Kind::Refresh => Self::Refresh,
            Kind::Verify => Self::Verify,
            Kind::Repair => Self::Repair,
        }
    }

    fn pass(self) -> PassKind {
        match self {
            // Not `Inline`: every batch the reconciler runs embeds units that were embedded before, so
            // what it pays for is a payment of its own, not the broker's redelivery of an earlier one.
            Self::Generate | Self::Repair => PassKind::Reconcile,
            Self::Refresh => PassKind::RefreshPrompt,
            Self::Verify => PassKind::VerifyLegacy,
        }
    }

    /// Whether the pipeline is given the model. The re-embeds are given none: that is what makes
    /// "no model call" a fact of construction and not a promise.
    fn uses_model(self) -> bool {
        matches!(self, Self::Generate | Self::Refresh)
    }
}

struct Job {
    org_id: String,
    document_id: String,
    units: Vec<Leased>,
}

/// What one group of a document came to.
#[derive(Debug, Default)]
struct GroupReport {
    context_written: usize,
    verified: usize,
    repaired: usize,
    unresolved: usize,
    announced: usize,
    withheld: usize,
    provider_failures: usize,
    credential_refused: bool,
    retry_after: Option<Duration>,
    errors: usize,
    health: Option<PassHealth>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Reconciler {
    pub(crate) fn new(deps: Deps, settings: Settings, clock: Arc<dyn Clock>) -> Self {
        let state = State {
            breaker: Breaker::new(settings.breaker_failures, settings.breaker_pause),
            cooldown: Cooldown::new(10_000),
            rotation: 0,
            sweep: None,
            zero_retention_next: HashMap::new(),
            zero_retention_orgs: None,
            refresh_left: settings.refresh_cap,
            stale_cursor: HashMap::new(),
            stale_next: HashMap::new(),
            alerted: HashSet::new(),
        };
        Self {
            deps,
            settings,
            clock,
            state: Mutex::new(state),
        }
    }

    /// Build the reconciler the service and `context-backfill` run: the same pipeline ports, a
    /// contextualizer of its own, the policy reader, the process's budget gate, and the usage
    /// ledger when it is on.
    pub(crate) async fn from_config(
        cfg: &crate::config::Config,
        pool: PgPool,
        qdrant: qdrant_client::Qdrant,
        provider: crate::provider::EmbeddingProvider,
        nats: async_nats::Client,
        signer: Option<Arc<EventSigner>>,
        budget: Arc<BudgetGate>,
    ) -> anyhow::Result<Self> {
        let settings = Settings::from_config(cfg)?;
        // The pipeline writes the provenance columns: say which migration to run now.
        batch::ensure_schema(&pool).await?;
        if signer.is_none() {
            anyhow::bail!(
                "the context reconciler announces the documents it heals with \
                 `dataplane.documents.indexed`, which the keyword index only reads signed: it \
                 needs ENABLE_SIGNED_EVENT_CONSUMERS=1 and EMBEDDING_EVENT_PRIVATE_KEY_PATH"
            );
        }
        let engine = Contextualizer::from_config(&contextualizer_config(cfg))?;
        let usage: Option<Arc<dyn LedgerSink>> =
            match batch::usage_signer(cfg.usage_emit_enabled, signer.clone())? {
                Some(usage_signer) => {
                    let ledger = usage_ledger_rs::JetStreamLedger::new(nats.clone(), usage_signer);
                    usage_ledger_rs::ensure_stream(ledger.context()).await?;
                    Some(Arc::new(ledger))
                }
                None => None,
            };
        let policy = Arc::new(PolicyReader::new(Arc::new(
            crate::policy::PgPolicySource::new(pool.clone()),
        )));
        Ok(Self::new(
            Deps {
                pool,
                provider: Arc::new(provider),
                vectors: Arc::new(qdrant),
                events: Arc::new(nats),
                signer,
                collection: cfg.qdrant_collection.clone(),
                engine,
                policy,
                budget,
                usage,
            },
            settings,
            Arc::new(SystemClock),
        ))
    }

    #[cfg(test)]
    pub(crate) fn settings(&self) -> &Settings {
        &self.settings
    }

    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    // ── the loop ────────────────────────────────────────────────────────────

    /// Run for ever. A pass that finds work is followed by the next after a short pause; one
    /// that finds none waits the interval; both wait at least as long as a provider asked, and
    /// both are jittered.
    pub(crate) async fn run(self: Arc<Self>) {
        tracing::info!(
            interval_secs = self.settings.interval.as_secs(),
            units_per_pass = self.settings.units_per_pass,
            org_units_per_pass = self.settings.org_units_per_pass,
            concurrency = self.settings.concurrency,
            org_concurrency = self.settings.org_concurrency,
            generation = self.deps.engine.is_some(),
            stale = self.settings.stale_documents.is_some(),
            refresh_cap = ?self.settings.refresh_cap,
            "context reconciler running"
        );
        loop {
            let report = self.pass().await;
            self.log(&report);
            let pause = match report.paused {
                Some(Paused::Breaker(left)) => left.min(self.settings.interval),
                Some(Paused::File) => self.settings.interval,
                None => pacing::next_pause(
                    report.worked(),
                    self.settings.interval,
                    report.retry_after,
                    pacing::unit_noise(),
                ),
            };
            tokio::time::sleep(pause).await;
        }
    }

    fn log(&self, report: &PassReport) {
        match report.paused {
            Some(Paused::File) => {
                tracing::debug!("context reconciler paused: the pause file exists")
            }
            Some(Paused::Breaker(left)) => tracing::warn!(
                reopens_in_secs = left.as_secs(),
                "context reconciler paused: the circuit breaker is open"
            ),
            None if report.worked() || report.errors > 0 => tracing::info!(
                leased = ?report.leased,
                documents = report.documents,
                contexts_written = report.generated,
                legacy_verified = report.verified,
                vectors_repaired = report.repaired,
                unresolved = report.unresolved,
                announced = report.announced,
                withheld = report.withheld,
                given_up = report.given_up.iter().map(|(_, _, n)| n).sum::<i64>(),
                zero_retention_released = report.zero_retention_released,
                budget_held = report.budget_held,
                errors = report.errors,
                health = ?report.health,
                "context reconciler pass"
            ),
            None => tracing::debug!("context reconciler pass: nothing to do"),
        }
    }

    // ── one pass ────────────────────────────────────────────────────────────

    pub(crate) async fn pass(&self) -> PassReport {
        self.pass_scoped(&Scope::default()).await
    }

    pub(crate) async fn pass_scoped(&self, scope: &Scope) -> PassReport {
        let mut report = PassReport::default();
        // The kill switch that needs no restart, then the breaker.
        if let Some(file) = &self.settings.pause_file {
            if file.exists() {
                report.paused = Some(Paused::File);
                return report;
            }
        }
        let now = self.clock.now();
        if let Some(left) = self.state().breaker.open_for(now) {
            report.paused = Some(Paused::Breaker(left));
            return report;
        }

        self.give_up(scope, &mut report).await;
        self.recheck_zero_retention(scope, &mut report).await;

        let leased = match self.lease_work(scope).await {
            Ok((leased, budget_held)) => {
                report.budget_held = budget_held;
                leased
            }
            Err(error) => {
                tracing::error!(error = %error, "the reconciler could not lease work");
                report.errors += 1;
                return report;
            }
        };
        if leased.is_empty() {
            report.health = Some(PassHealth::Idle);
            return report;
        }
        for unit in &leased {
            *report.leased.entry(unit.kind.as_str()).or_default() += 1;
        }

        let outcomes = self.run_jobs(group_into_jobs(leased)).await;
        report.documents = outcomes.len();
        let mut health = PassHealth::Idle;
        for outcome in outcomes {
            report.generated += outcome.context_written;
            report.verified += outcome.verified;
            report.repaired += outcome.repaired;
            report.unresolved += outcome.unresolved;
            report.announced += outcome.announced;
            report.withheld += outcome.withheld;
            report.errors += outcome.errors;
            report.retry_after = report.retry_after.max(outcome.retry_after);
            health = worse_of(health, outcome.health.unwrap_or(PassHealth::Idle));
        }
        report.health = Some(health);
        let now = self.clock.now();
        self.state().breaker.record(now, health);
        report
    }

    // ── housekeeping: no provider is asked ──────────────────────────────────

    /// The units that failed `max_attempts` times become `failed_permanent`, with an alert.
    async fn give_up(&self, scope: &Scope, report: &mut PassReport) {
        let max = self.settings.max_attempts;
        let orgs = match select::orgs_with_exhausted_units(&self.deps.pool, max).await {
            Ok(orgs) => orgs,
            Err(error) => {
                tracing::error!(error = %error, "could not look for units to give up on");
                report.errors += 1;
                return;
            }
        };
        for org in orgs.into_iter().filter(|org| in_scope(scope, org)) {
            let result = async {
                let mut tx = pg_org_scope::begin_org_scoped(&self.deps.pool, &org).await?;
                let given_up = select::give_up_on_exhausted(&mut tx, &org, max).await?;
                tx.commit().await?;
                anyhow::Ok(given_up)
            }
            .await;
            match result {
                Ok(classes) => {
                    for (class, units) in classes {
                        // The alert: content-free, one line per organization and class.
                        tracing::error!(
                            alert = "context_units_failed_permanent",
                            org_id = %org,
                            error_class = %class,
                            units,
                            attempts = max,
                            "units failed every attempt and are failed_permanent; the class says why"
                        );
                        report.given_up.push((org.clone(), class, units));
                    }
                }
                Err(error) => {
                    tracing::error!(error = %error, org_id = %org, "could not give up on exhausted units");
                    report.errors += 1;
                }
            }
        }
    }

    /// An organization whose units were skipped for zero retention is asked about, at most once
    /// an hour, by minting a bearer and reading its `zdr` claim. Still zero-retention: nothing is
    /// written and nothing is called. No longer: its units are `pending` again, and the generate
    /// pass takes them. Not decided (Auth Core could not say): asked again in five minutes.
    async fn recheck_zero_retention(&self, scope: &Scope, report: &mut PassReport) {
        let Some(engine) = &self.deps.engine else {
            return;
        };
        let now = self.clock.now();
        let interval = self.settings.zdr_recheck;
        let listed_at = self.state().zero_retention_orgs.as_ref().map(|(at, _)| *at);
        if listed_at.is_none_or(|at| now.saturating_duration_since(at) >= interval) {
            match select::orgs_with_zero_retention_skips(&self.deps.pool).await {
                Ok(orgs) => self.state().zero_retention_orgs = Some((now, orgs)),
                Err(error) => {
                    tracing::error!(error = %error, "could not list the zero-retention skips");
                    report.errors += 1;
                    return;
                }
            }
        }
        let orgs: Vec<String> = self
            .state()
            .zero_retention_orgs
            .as_ref()
            .map(|(_, orgs)| orgs.clone())
            .unwrap_or_default();
        for org in orgs.into_iter().filter(|org| in_scope(scope, org)) {
            let due = self
                .state()
                .zero_retention_next
                .get(&org)
                .is_none_or(|next| now >= *next);
            if !due {
                continue;
            }
            report.zero_retention_checked += 1;
            let next = match engine.org_is_zero_retention(&org).await {
                Ok(true) => now + interval,
                Ok(false) => {
                    let released = async {
                        let mut tx = pg_org_scope::begin_org_scoped(&self.deps.pool, &org).await?;
                        let released = select::release_zero_retention_skips(&mut tx, &org).await?;
                        tx.commit().await?;
                        anyhow::Ok(released)
                    }
                    .await;
                    match released {
                        Ok(released) => {
                            report.zero_retention_released += released;
                            tracing::info!(
                                org_id = %org,
                                units = released,
                                "organization is no longer zero-retention: its skipped units are pending again"
                            );
                            now + interval
                        }
                        Err(error) => {
                            tracing::error!(error = %error, org_id = %org, "could not release zero-retention skips");
                            report.errors += 1;
                            now + inference_client_rs::PENDING_RETRY_AFTER
                        }
                    }
                }
                // Auth Core could not say: a wait, never a skip and never a send.
                Err(_) => now + inference_client_rs::PENDING_RETRY_AFTER,
            };
            self.state().zero_retention_next.insert(org, next);
        }
    }

    // ── leasing ─────────────────────────────────────────────────────────────

    /// The sweep: the work the work queue index does not cover, counted per organization and
    /// remembered until the sweep interval is over.
    async fn sweep(&self, scope: &Scope) -> anyhow::Result<()> {
        let now = self.clock.now();
        let fresh = self
            .state()
            .sweep
            .as_ref()
            .is_some_and(|sweep| now.saturating_duration_since(sweep.at) < self.settings.sweep);
        if fresh && !scope.eager {
            return Ok(());
        }
        let window = self.window_chars();
        let counts = select::counts(
            &self.deps.pool,
            scope.single(),
            self.settings.max_attempts,
            window,
            None,
        )
        .await
        .context("count the work the queue index does not cover")?;
        let counts: Vec<_> = counts
            .into_iter()
            .filter(|count| in_scope(scope, &count.org_id))
            .collect();
        let mut sweep = Sweep {
            at: now,
            all: counts.iter().map(|c| c.org_id.clone()).collect(),
            repair: counts
                .iter()
                .filter(|c| c.repair > 0)
                .map(|c| c.org_id.clone())
                .collect(),
            policy: counts
                .iter()
                .filter(|c| c.skipped_policy > 0)
                .map(|c| c.org_id.clone())
                .collect(),
            budget: counts
                .iter()
                .filter(|c| c.skipped_budget > 0)
                .map(|c| c.org_id.clone())
                .collect(),
            refresh: HashSet::new(),
        };
        // A refresh target is the organization's own (its pin, or the service's version), so
        // the units that are behind are counted per organization.
        if self.refresh_possible() {
            for org in &sweep.all {
                let target = self.refresh_target_for(org).await;
                let behind = select::counts(
                    &self.deps.pool,
                    Some(org),
                    self.settings.max_attempts,
                    window,
                    Some(target.as_str()),
                )
                .await
                .context("count the contexts behind the prompt version in force")?;
                if behind.iter().any(|c| c.refresh > 0) {
                    sweep.refresh.insert(org.clone());
                }
            }
        }
        self.state().sweep = Some(sweep);
        Ok(())
    }

    /// The most document text a prompt carries: the configured cap, not the window, since that
    /// is the bound an estimate has to hold.
    fn window_chars(&self) -> i64 {
        self.settings.prompt_chars_cap
    }

    fn refresh_possible(&self) -> bool {
        self.deps.engine.is_some() && self.state().refresh_left.is_some_and(|left| left > 0)
    }

    /// The prompt version a refresh moves an organization to: the version in force for it, which
    /// is its pin, else the version the service generates with. (The operator's
    /// `--target-version` sets the second for the run, so that what is selected and what is
    /// generated are the same version: a refresh to a version the pipeline would not generate with
    /// would select the same units again for ever.)
    async fn refresh_target_for(&self, org: &str) -> PromptVersion {
        let Some(engine) = &self.deps.engine else {
            return PromptVersion::V1;
        };
        match self.deps.policy.settings(&[org]).await.get(org).copied() {
            Some(OrgSetting::Generate { pin: Some(pin), .. }) => pin,
            _ => engine.prompt_version(),
        }
    }

    /// The units this pass works on, and how many organizations the budget gate (CR-T9b) is
    /// holding back (`stop`, used up).
    async fn lease_work(&self, scope: &Scope) -> anyhow::Result<(Vec<Leased>, usize)> {
        self.sweep(scope).await?;
        let due = select::orgs_with_due_work(&self.deps.pool)
            .await
            .context("find the organizations with work due")?;
        let mut orgs: Vec<String> = {
            let state = self.state();
            let mut orgs: HashSet<String> = due.iter().cloned().collect();
            if let Some(sweep) = &state.sweep {
                orgs.extend(sweep.repair.iter().cloned());
                orgs.extend(sweep.policy.iter().cloned());
                orgs.extend(sweep.refresh.iter().cloned());
                if self.settings.stale_documents.is_some() {
                    orgs.extend(sweep.all.iter().cloned());
                }
            }
            orgs.into_iter().collect()
        };
        orgs.retain(|org| in_scope(scope, org));
        orgs.sort();
        let order = {
            let mut state = self.state();
            let cursor = state.rotation;
            state.rotation = state.rotation.wrapping_add(1);
            pacing::rotated(&orgs, cursor)
        };

        let now = self.clock.now();
        let excluded = self.state().cooldown.cooling(now);
        let lease_secs = self.settings.lease.as_secs_f64();
        let mut room_global = self
            .settings
            .units_per_pass
            .min(scope.max_units.unwrap_or(usize::MAX));
        let mut out: Vec<Leased> = Vec::new();
        let mut budget_held = 0usize;

        for org in order {
            if room_global == 0 {
                break;
            }
            let mut room = self.settings.org_units_per_pass.min(room_global);
            // What the organization's policy says matters to the kinds that ask a model; the
            // re-embeds read nothing of it.
            let setting = match &self.deps.engine {
                Some(_) => self
                    .deps
                    .policy
                    .settings(&[org.as_str()])
                    .await
                    .remove(&org),
                None => None,
            };
            // A model is asked only while the incident switch leaves one (the policy is not read
            // without it) and the organization's policy could be read. `Off` still runs the
            // generate kind once, which is what records `skipped_policy` on its units.
            let model_allowed =
                matches!(setting, Some(OrgSetting::Generate { .. } | OrgSetting::Off));
            let policy_off = matches!(setting, Some(OrgSetting::Off));
            // What the organization's background budget says (CR-T9b), for the kinds that ask a
            // model. Asked only for an organization that generates (the incident switch and an
            // off or unknown policy have no spend to hold), and free for the default (no figure:
            // not even the ledger is read). The default mode never holds anything back.
            //
            // * a STOP leases no unit that a model would be asked about once it has been marked:
            //   units still without a status are leased once, which is how the pipeline records
            //   `skipped_budget` on them (an explicit mark, no provider call), and units already
            //   marked are not leased until the gate lets the organization through;
            // * a THROTTLE lets one unit be asked about per pass, whatever the kind.
            let verdict = match setting {
                Some(OrgSetting::Generate { budget, .. }) => {
                    Some(self.deps.budget.check(&org, &budget).await)
                }
                _ => None,
            };
            let stopped = verdict.is_some_and(|verdict| verdict.withholds());
            let mut model_left = verdict
                .and_then(|verdict| verdict.allowance())
                .unwrap_or(usize::MAX);
            let in_due = due.contains(&org);
            let (repair_flagged, policy_flagged, refresh_flagged, budget_flagged) = {
                let state = self.state();
                match &state.sweep {
                    Some(sweep) => (
                        sweep.repair.contains(&org),
                        sweep.policy.contains(&org),
                        sweep.refresh.contains(&org),
                        sweep.budget.contains(&org),
                    ),
                    None => (false, false, false, false),
                }
            };
            let mut generate_leased = 0usize;

            // 1. Generate: no usable context. For an organization whose policy is off the
            //    pipeline records `skipped_policy` once and calls nothing.
            if model_allowed && (in_due || policy_flagged) && room > 0 && model_left > 0 {
                let leased = select::lease(
                    &self.deps.pool,
                    &LeaseRequest {
                        org_id: &org,
                        kind: Kind::Generate,
                        limit: room.min(model_left) as i64,
                        lease_secs,
                        excluded: &excluded,
                        max_attempts: self.settings.max_attempts,
                        include_skipped_policy: !policy_off,
                        include_skipped_budget: !stopped,
                        refresh_target: "",
                    },
                )
                .await
                .with_context(|| format!("lease {} units", Kind::Generate.as_str()))?;
                room -= leased.len();
                room_global -= leased.len();
                model_left = model_left.saturating_sub(leased.len());
                generate_leased = leased.len();
                out.extend(leased);
            }
            // A stop that is holding something back: units it has just been given to mark, or units
            // it marked before. (An organization that is stopped and has nothing to generate is not
            // held back from anything: a backfill of it is "nothing left to do".)
            if stopped && (generate_leased > 0 || budget_flagged) {
                budget_held += 1;
            }
            // 2. Verify the legacy units: no model.
            if in_due && room > 0 {
                let leased = select::lease(
                    &self.deps.pool,
                    &LeaseRequest {
                        org_id: &org,
                        kind: Kind::Verify,
                        limit: room as i64,
                        lease_secs,
                        excluded: &excluded,
                        max_attempts: self.settings.max_attempts,
                        include_skipped_policy: false,
                        include_skipped_budget: false,
                        refresh_target: "",
                    },
                )
                .await
                .with_context(|| format!("lease {} units", Kind::Verify.as_str()))?;
                room -= leased.len();
                room_global -= leased.len();
                out.extend(leased);
            }
            // 3. Repair the vectors that are not the ones their contexts make: no model.
            if (repair_flagged || scope.eager) && room > 0 {
                let leased = select::lease(
                    &self.deps.pool,
                    &LeaseRequest {
                        org_id: &org,
                        kind: Kind::Repair,
                        limit: room as i64,
                        lease_secs,
                        excluded: &excluded,
                        max_attempts: self.settings.max_attempts,
                        include_skipped_policy: false,
                        include_skipped_budget: false,
                        refresh_target: "",
                    },
                )
                .await
                .with_context(|| format!("lease {} units", Kind::Repair.as_str()))?;
                if leased.is_empty() {
                    if let Some(sweep) = self.state().sweep.as_mut() {
                        sweep.repair.remove(&org);
                    }
                }
                room -= leased.len();
                room_global -= leased.len();
                out.extend(leased);
            }
            // 4. Stale: the document changed after the context was made. A stop regenerates
            //    nothing (the context a unit has stays in force), and a throttle regenerates one.
            if model_allowed && !policy_off && !stopped && room > 0 && model_left > 0 {
                let ids = self.stale_units(&org, setting, scope).await?;
                if !ids.is_empty() {
                    let mut leased = select::lease_ids(&self.deps.pool, &org, &ids, lease_secs)
                        .await
                        .with_context(|| format!("lease {} units", Kind::Stale.as_str()))?;
                    leased.truncate(room.min(model_left));
                    room -= leased.len();
                    room_global -= leased.len();
                    model_left = model_left.saturating_sub(leased.len());
                    out.extend(leased);
                }
            }
            // 5. Refresh a superseded prompt, inside the cap. The same budget rules as above:
            //    a rollout is the first thing a used-up budget holds back.
            if model_allowed
                && !policy_off
                && !stopped
                && (refresh_flagged || scope.eager)
                && room > 0
                && model_left > 0
            {
                let allowed = self.refresh_allowance(room.min(model_left));
                if allowed > 0 && self.may_regenerate(&org).await {
                    let target = self.refresh_target_for(&org).await;
                    let leased = select::lease(
                        &self.deps.pool,
                        &LeaseRequest {
                            org_id: &org,
                            kind: Kind::Refresh,
                            limit: allowed as i64,
                            lease_secs,
                            excluded: &excluded,
                            max_attempts: self.settings.max_attempts,
                            include_skipped_policy: false,
                            include_skipped_budget: false,
                            refresh_target: target.as_str(),
                        },
                    )
                    .await
                    .with_context(|| format!("lease {} units", Kind::Refresh.as_str()))?;
                    if leased.is_empty() {
                        if let Some(sweep) = self.state().sweep.as_mut() {
                            sweep.refresh.remove(&org);
                        }
                    }
                    self.spend_refresh(leased.len());
                    room -= leased.len();
                    room_global -= leased.len();
                    out.extend(leased);
                }
            }
        }
        Ok((out, budget_held))
    }

    /// Whether the units of an organization that already have a context may be sent for
    /// regeneration now: Auth Core says the organization is not zero-retention (a bearer is
    /// minted for it and its `zdr` claim read; no model is asked, and the token client keeps the
    /// bearer for under a minute). A zero-retention organization, one whose posture could not be
    /// resolved and a deployment with no model (the incident switch) are left alone. Without this a
    /// regeneration that cannot be made would still embed the unit again, because the pipeline
    /// embeds every unit of a batch, and a `ready` unit is selected again for as long as its
    /// context is of another prompt version or its document changed.
    async fn may_regenerate(&self, org: &str) -> bool {
        let Some(engine) = &self.deps.engine else {
            return false;
        };
        matches!(engine.org_is_zero_retention(org).await, Ok(false))
    }

    /// How many units a refresh may lease now: what the pass has room for, and what is left of
    /// the cap. Zero when no refresh was asked for.
    fn refresh_allowance(&self, room: usize) -> usize {
        match self.state().refresh_left {
            Some(left) => room.min(usize::try_from(left).unwrap_or(usize::MAX)),
            None => 0,
        }
    }

    fn spend_refresh(&self, leased: usize) {
        if let Some(left) = self.state().refresh_left.as_mut() {
            *left = left.saturating_sub(leased as u64);
        }
    }

    /// The units of the next documents of an organization whose context is older than the
    /// document AND whose input hash says it matters. Walks the organization's documents a page
    /// at a time, and starts over after the sweep interval once it has been through them all.
    async fn stale_units(
        &self,
        org: &str,
        setting: Option<OrgSetting>,
        scope: &Scope,
    ) -> anyhow::Result<Vec<String>> {
        let (Some(limit), Some(engine)) = (self.settings.stale_documents, &self.deps.engine) else {
            return Ok(Vec::new());
        };
        let now = self.clock.now();
        let (after, due) = {
            let state = self.state();
            (
                state.stale_cursor.get(org).cloned().unwrap_or_default(),
                state.stale_next.get(org).is_none_or(|next| now >= *next),
            )
        };
        if !due && !scope.eager {
            return Ok(Vec::new());
        }
        // Before any of the organization's text is read.
        if !self.may_regenerate(org).await {
            return Ok(Vec::new());
        }
        let documents = {
            let mut tx = pg_org_scope::begin_org_scoped(&self.deps.pool, org).await?;
            let documents = select::stale_documents(&mut tx, org, &after, limit as i64).await?;
            tx.commit().await?;
            documents
        };
        let Some(last) = documents.last() else {
            // Through them all: start again after the interval.
            let mut state = self.state();
            state.stale_cursor.remove(org);
            state
                .stale_next
                .insert(org.to_owned(), now + self.settings.sweep);
            return Ok(Vec::new());
        };
        self.state()
            .stale_cursor
            .insert(org.to_owned(), last.document_id.clone());
        let pin = match setting {
            Some(OrgSetting::Generate { pin, .. }) => pin,
            _ => None,
        };
        let mut regenerate = Vec::new();
        for document in &documents {
            if document.content.trim().is_empty() {
                continue;
            }
            // The same recipe the pipeline hashes a chunk's inputs with (`batch::contexts`): a
            // test holds the two together, because a stale pass that hashed differently would
            // regenerate every context of every document it looked at.
            let date = document
                .date
                .map(|date| date.format("%Y-%m-%d").to_string())
                .unwrap_or_default();
            let prepared = engine.prepare(DocumentInput {
                org_id: org,
                document_id: &document.document_id,
                meta: DocumentMeta {
                    title: &document.title,
                    source: &document.source,
                    date: &date,
                },
                text: &document.content,
                restricted: false,
                prompt_version: pin,
                total_chunks: usize::try_from(document.total_chunks)
                    .ok()
                    .filter(|total| *total > 0),
            });
            for unit in &document.units {
                let now_hash = engine.input_hash(
                    &prepared,
                    &ChunkInput {
                        index: unit.chunk_index,
                        text: &unit.text,
                    },
                );
                // Only a chunk whose inputs changed under the same prompt and model: a context
                // made with another prompt is the refresh's business, not a document edit's.
                if now_hash != unit.input_hash
                    && unit.prompt_version.as_deref() == Some(prepared.version().as_str())
                    && unit.model.as_deref() == Some(engine.model_name())
                {
                    regenerate.push(unit.knowledge_id.clone());
                }
            }
        }
        Ok(regenerate)
    }

    // ── running ─────────────────────────────────────────────────────────────

    /// Work the jobs: at most `concurrency` documents at once and `org_concurrency` of one
    /// organization, the organizations taking turns at being started.
    async fn run_jobs(&self, jobs: Vec<Job>) -> Vec<GroupReport> {
        let mut queues: BTreeMap<String, VecDeque<Job>> = BTreeMap::new();
        for job in jobs {
            queues.entry(job.org_id.clone()).or_default().push_back(job);
        }
        let mut running: FuturesUnordered<_> = FuturesUnordered::new();
        let mut per_org: HashMap<String, usize> = HashMap::new();
        let mut reports = Vec::new();
        let mut turn = 0usize;
        loop {
            // Start jobs while there is room, taking the organizations in turns.
            while running.len() < self.settings.concurrency {
                let orgs: Vec<String> = queues
                    .iter()
                    .filter(|(org, queue)| {
                        !queue.is_empty()
                            && per_org.get(*org).copied().unwrap_or(0)
                                < self.settings.org_concurrency
                    })
                    .map(|(org, _)| org.clone())
                    .collect();
                if orgs.is_empty() {
                    break;
                }
                let org = orgs[turn % orgs.len()].clone();
                turn = turn.wrapping_add(1);
                let Some(job) = queues.get_mut(&org).and_then(VecDeque::pop_front) else {
                    break;
                };
                *per_org.entry(org.clone()).or_default() += 1;
                running.push(async move {
                    let report = self.run_job(job).await;
                    (org, report)
                });
            }
            let Some((org, report)) = running.next().await else {
                break;
            };
            if let Some(count) = per_org.get_mut(&org) {
                *count = count.saturating_sub(1);
            }
            reports.push(report);
        }
        reports
    }

    /// One document: each group of its leased units through the pipeline, one after the other.
    async fn run_job(&self, job: Job) -> GroupReport {
        let mut total = GroupReport::default();
        let mut by_group: BTreeMap<Group, Vec<Leased>> = BTreeMap::new();
        for unit in job.units {
            by_group.entry(Group::of(unit.kind)).or_default().push(unit);
        }
        for (group, units) in by_group {
            let report = self.run_group(&job.org_id, group, &units).await;
            total.context_written += report.context_written;
            total.verified += report.verified;
            total.repaired += report.repaired;
            total.unresolved += report.unresolved;
            total.announced += report.announced;
            total.withheld += report.withheld;
            total.errors += report.errors;
            total.provider_failures += report.provider_failures;
            total.credential_refused |= report.credential_refused;
            total.retry_after = total.retry_after.max(report.retry_after);
            total.health = Some(worse_of(
                total.health.unwrap_or(PassHealth::Idle),
                report.health.unwrap_or(PassHealth::Idle),
            ));
        }
        total
    }

    async fn run_group(&self, org: &str, group: Group, units: &[Leased]) -> GroupReport {
        let mut report = GroupReport::default();
        let items: Vec<BatchItem> = units
            .iter()
            .map(|unit| BatchItem {
                knowledge_id: unit.knowledge_id.clone(),
                document_id: unit.document_id.clone(),
                org_id: unit.org_id.clone(),
                chunk_index: unit.chunk_index,
                text: unit.text.clone(),
                // A restricted document is never leased.
                zdr: false,
                // Nobody acted: this is the system repairing what it holds.
                user_id: None,
                // The vector's payload is replaced whole by the upsert: the date must go with it.
                document_date: unit.document_date,
                chunk_context: None,
            })
            .collect();
        let ids: Vec<String> = units.iter().map(|unit| unit.knowledge_id.clone()).collect();

        let held = HeldSink::new(self.deps.events.as_ref());
        let pipeline = Pipeline {
            provider: self.deps.provider.as_ref(),
            vectors: self.deps.vectors.as_ref(),
            pool: &self.deps.pool,
            collection: &self.deps.collection,
            events: &held,
            event_signer: self.deps.signer.as_deref(),
            contextualizer: if group.uses_model() {
                self.deps.engine.as_ref()
            } else {
                None
            },
            policy: self.deps.policy.as_ref(),
            budget: self.deps.budget.as_ref(),
            usage: self.deps.usage.as_deref(),
            pass: group.pass(),
        };
        let result = batch::process_batch(items, &pipeline).await;
        let failure = match &result {
            Ok(()) => None,
            Err(error) => Some(classify_failure(error)),
        };

        let states = async {
            let mut tx = pg_org_scope::begin_org_scoped(&self.deps.pool, org).await?;
            let states = select::states(&mut tx, org, &ids).await?;
            tx.commit().await?;
            anyhow::Ok(states)
        }
        .await;
        let states: HashMap<String, UnitState> = match states {
            Ok(states) => states
                .into_iter()
                .map(|state| (state.knowledge_id.clone(), state))
                .collect(),
            Err(error) => {
                tracing::error!(error = %error, org_id = %org, "could not read the units back after a batch");
                report.errors += 1;
                held.drop_held();
                return report;
            }
        };
        if let Err(error) = &result {
            tracing::warn!(
                error = %error,
                org_id = %org,
                group = ?group,
                units = units.len(),
                "a batch failed; the units stay as the database holds them and are tried again"
            );
            report.errors += 1;
        }

        let now = self.clock.now();
        let mut generated = 0usize;
        let mut provider_failures = 0usize;
        for unit in units {
            let Some(state) = states.get(&unit.knowledge_id) else {
                // Deleted while it was being worked on.
                self.state().cooldown.cleared(&unit.knowledge_id);
                continue;
            };
            let written = match (unit.generated_at, state.generated_at) {
                (None, Some(_)) => true,
                (Some(before), Some(after)) => after > before,
                _ => false,
            };
            if written {
                generated += 1;
            }
            let right = match group {
                Group::Verify => state.status.as_deref() == Some("ready") && state.coherent,
                Group::Repair => state.coherent,
                Group::Generate | Group::Refresh => state.coherent && state.status_is_settled(),
            };
            if matches!(group, Group::Verify) && right {
                report.verified += 1;
            }
            if matches!(group, Group::Repair) && right {
                report.repaired += 1;
            }
            // Memory for the work that has no schedule of its own: a unit whose vector is still
            // not right waits on the ladder; one that is right is forgotten. A unit that is still
            // waiting on a context with no time in the database to be asked again (a `pending`
            // unit the inline phase gave up on is due at once, and so is a throttled one) has no
            // schedule of its own either: left alone it would be leased, embedded again and paid
            // for at every pass. (`skipped_budget` is not one of these: it is a decision of the
            // budget gate, read at every lease, and a unit it holds is not leased at all.)
            let unscheduled =
                !state.status_is_settled() && state.due_in_secs.is_none_or(|due| due <= 0.0);
            // A regeneration that did not happen (the provider failed, the content was refused, the
            // organization is zero-retention) leaves a `ready` unit exactly as it was and as
            // selectable as it was: it waits on the ladder, whatever the database says, because a
            // failure that is final (a refused content) writes no time to wait for.
            let regeneration_pending = matches!(unit.kind, Kind::Refresh | Kind::Stale) && !written;
            if state.coherent
                && !unscheduled
                && !regeneration_pending
                && (right || !matches!(group, Group::Verify | Group::Repair))
            {
                self.state().cooldown.cleared(&unit.knowledge_id);
            } else {
                let wait = failure.as_ref().and_then(|failure| failure.wait);
                let attempts = self.state().cooldown.failed(&unit.knowledge_id, now, wait);
                if attempts >= u32::try_from(self.settings.max_attempts).unwrap_or(u32::MAX) {
                    let first = self.state().alerted.insert(unit.knowledge_id.clone());
                    if first {
                        tracing::error!(
                            alert = "context_unit_unresolvable",
                            org_id = %org,
                            knowledge_id = %unit.knowledge_id,
                            group = ?group,
                            attempts,
                            "a unit's vector could not be made right after every attempt; it stays as it is and is asked again at the longest interval"
                        );
                    }
                }
            }
            if !right {
                report.unresolved += 1;
            }
            match state.error_class.as_deref() {
                Some("rate_limited" | "provider_unavailable" | "timeout")
                    if state.status.as_deref() == Some("failed_transient") =>
                {
                    provider_failures += 1;
                    if state.error_class.as_deref() == Some("rate_limited") {
                        if let Some(due) = state.due_in_secs.filter(|due| *due > 0.0) {
                            let due = Duration::from_secs_f64(due).min(pacing::MAX_WAIT);
                            report.retry_after = report.retry_after.max(Some(due));
                        }
                    }
                }
                Some("auth") if state.status.as_deref() == Some("failed_transient") => {
                    report.credential_refused = true;
                }
                _ => {}
            }
        }
        report.context_written = generated;
        report.provider_failures = provider_failures;
        if let Some(failure) = &failure {
            report.retry_after = report.retry_after.max(failure.wait);
            match failure.health {
                PassHealth::CredentialRefused => report.credential_refused = true,
                PassHealth::ProviderFailed => report.provider_failures += 1,
                _ => {}
            }
        }
        report.health = Some(if report.credential_refused {
            PassHealth::CredentialRefused
        } else if generated > 0 {
            PassHealth::Healthy
        } else if report.provider_failures > 0 {
            PassHealth::ProviderFailed
        } else {
            PassHealth::Idle
        });

        // The announcement: only a document whose keyword-index content changed. A context was
        // written for one of its units; or a repair made a vector right after a batch that wrote
        // a context and died before it could announce it (the unit is `ready` with a vector that
        // does not match, which is exactly how that batch leaves it), so this is the first time
        // the document is complete since its context changed. When the batch itself failed
        // nothing was held (the pipeline announces last) and releasing sends nothing.
        if generated > 0 || (group == Group::Repair && report.repaired > 0) {
            report.announced = held.release().await;
        } else {
            report.withheld = held.drop_held();
        }
        report
    }
}

/// What a failed batch means for the units in it.
struct Failure {
    health: PassHealth,
    /// How long the provider asked to be left alone, when it said.
    wait: Option<Duration>,
}

/// Read a failed batch through the same table that reads a failed call. A typed inference
/// failure says what it was (a throttle, an outage, a refused credential, a posture that could
/// not be resolved); anything else (the database, the vector store) says nothing about the
/// provider and does not move the breaker.
fn classify_failure(error: &anyhow::Error) -> Failure {
    let Some(typed) = error.downcast_ref::<InferenceError>() else {
        return Failure {
            health: PassHealth::Idle,
            wait: None,
        };
    };
    let outcome = outcome_for_failure(typed);
    let wait = outcome.retry_after();
    let health = match &outcome {
        ContextOutcome::Transient {
            class: ErrorClass::Auth,
            ..
        } => PassHealth::CredentialRefused,
        ContextOutcome::Transient { .. } | ContextOutcome::ResidencyUnavailable => {
            PassHealth::ProviderFailed
        }
        // A posture Auth Core could not resolve, a zero-retention refusal, a permanent failure:
        // none of them is the provider being unwell.
        _ => PassHealth::Idle,
    };
    Failure { health, wait }
}

fn worse_of(a: PassHealth, b: PassHealth) -> PassHealth {
    use PassHealth::{CredentialRefused, Healthy, Idle, ProviderFailed};
    match (a, b) {
        (CredentialRefused, _) | (_, CredentialRefused) => CredentialRefused,
        // The provider answered something: it is up, whatever else failed.
        (Healthy, _) | (_, Healthy) => Healthy,
        (ProviderFailed, _) | (_, ProviderFailed) => ProviderFailed,
        (Idle, Idle) => Idle,
    }
}

fn in_scope(scope: &Scope, org: &str) -> bool {
    scope
        .orgs
        .as_ref()
        .is_none_or(|orgs| orgs.iter().any(|only| only == org))
}

impl Scope {
    /// One organization, everything looked at now.
    #[cfg(test)]
    pub(crate) fn only(org_id: &str) -> Self {
        Self {
            orgs: Some(vec![org_id.to_owned()]),
            eager: true,
            ..Self::default()
        }
    }

    /// The organization, when the scope is exactly one: counts can then be asked for it alone.
    fn single(&self) -> Option<&str> {
        match self.orgs.as_deref() {
            Some([only]) => Some(only.as_str()),
            _ => None,
        }
    }
}

/// The leased units as jobs: one per document, its units in kind order.
fn group_into_jobs(mut leased: Vec<Leased>) -> Vec<Job> {
    leased.sort_by(|a, b| {
        (&a.org_id, &a.document_id, a.kind, a.chunk_index).cmp(&(
            &b.org_id,
            &b.document_id,
            b.kind,
            b.chunk_index,
        ))
    });
    let mut jobs: Vec<Job> = Vec::new();
    for unit in leased {
        match jobs.last_mut() {
            Some(job) if job.org_id == unit.org_id && job.document_id == unit.document_id => {
                job.units.push(unit);
            }
            _ => jobs.push(Job {
                org_id: unit.org_id.clone(),
                document_id: unit.document_id.clone(),
                units: vec![unit],
            }),
        }
    }
    jobs
}
