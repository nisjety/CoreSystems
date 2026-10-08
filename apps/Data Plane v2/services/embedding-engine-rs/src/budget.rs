//! The background LLM budget gate (CR-T9b): what the embedding engine does about an
//! organization's spend on contextual retrieval when that spend passes its budget.
//!
//! ## The owner's rulings this encodes (default D-E)
//!
//! * **Configurable per organization and globally.** An organization's own row of
//!   `org_processing_policy` (`budget_mode`, `monthly_budget_usd`) wins; what it leaves at
//!   `inherit` / NULL is the deployment's (`BACKGROUND_BUDGET_MODE`,
//!   `BACKGROUND_BUDGET_MONTHLY_USD`). An organization with no row inherits everything.
//! * **The default is alert-only, with no figure.** Exceeding a budget alerts and keeps working;
//!   nothing is built in; the owner supplies the figure later. With no figure anywhere there is
//!   nothing to exceed, so the gate does not even read the ledger.
//! * **`stop` is explicit, never the default, and never silent.** It exists only where an
//!   operator set `budget_mode = stop` on the organization or deployment-wide. When it holds, the
//!   units that would have been given a context are marked `skipped_budget` (a status that says
//!   why), they are embedded from their raw text, and they are worked again once the gate lets
//!   them through (a new month, a raised figure, a different mode).
//! * **`throttle` defers instead of skipping.** Over the figure, an organization is worked one
//!   chunk at a time ([`THROTTLE_UNITS`] per batch and per reconciler pass); the rest stay
//!   `pending` and are picked up at that pace.
//!
//! This gate does not degrade chat. Data Plane background spend is read from this plane's own
//! ledger (`cost_events`), never from cost-core's chat budget, and nothing here lowers a chat
//! model: the worst it does is stop contextualizing, which `skipped_budget` says out loud.
//!
//! ## What "spend" is
//!
//! The Data Plane records tokens, not dollars: cost-core prices them from its catalogue, and the
//! catalogue is not read here. So the gate prices the month's `llm` tokens in `cost_events` with a
//! price table the deployment supplies (`BACKGROUND_PRICE_INPUT_USD_PER_MTOK` and
//! `..._OUTPUT_...`), with no default. The `llm` events are every background chat-completion
//! call of the organization (contextual retrieval, graph extraction, community summaries): the
//! budget is the organization's background LLM budget, not this service's share of it, and
//! graph-index (`graph-index-rs/src/budget.rs`) counts exactly the same rows with exactly the
//! same SQL (a test holds the two statements together). Embeddings and reranks are other event
//! types and are not part of it.
//!
//! A budget whose spend cannot be measured (no prices, the usage ledger is off, or the ledger
//! cannot be read) is **never enforced on a guess**: the gate alerts, says why, and lets the work
//! through ([`Verdict::Unmeasured`]). A `stop` that could never fire is refused at boot where the
//! configuration alone shows it.
//!
//! ## Stopping before the next provider call
//!
//! The ledger lags a call by the time it takes the event to reach `cost_events`. So that a batch
//! that crosses the figure is not followed by another that does not know it, every call this
//! process makes is also added to the organization's cached figure ([`BudgetGate::record_spend`])
//! until the next reading of the ledger replaces it. The gate therefore stops within one batch of
//! crossing the figure, never to the cent.
//!
//! ## Where it sits
//!
//! After the organization's operator policy and before a bearer is minted. An organization that
//! is off is `skipped_policy` whatever its budget says, and the gate is not asked about it. A
//! `stop` is decided without a bearer (that is the point: it makes no call at all), so the
//! retention posture of the organization is not known when it is taken: a zero-retention
//! organization that is also over a stopped budget is marked `skipped_budget` and, once the
//! budget lets it through, `skipped_zdr`. Either way nothing is sent. A restricted document keeps
//! its `skipped_zdr` mark, which is decided before the budget is looked at. The incident kill
//! switch comes before all of it: with no model there is nothing to hold, and the gate is not
//! consulted.
//!
//! ## The alert
//!
//! Every verdict that finds an organization over its budget increments
//! `ctx_budget_exceeded_total{mode}` (at most once per organization per spend reading, so a
//! busy organization is one count a reading, not one per batch) and writes one structured line
//! with `alert = "ctx_budget_exceeded"` (at most once per organization per five minutes). Both
//! are content-free: an organization id, two dollar figures and a mode.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use sqlx::PgPool;
use tokio::time::Instant;

/// How long an organization's spend is kept before the ledger is read again.
pub(crate) const SPEND_CACHE_TTL: Duration = Duration::from_secs(30);

/// After a failed read of the ledger, how long before it is read again for the same
/// organization. Without it a database that is struggling would be asked once per batch.
const RETRY_AFTER_FAILED_READ: Duration = Duration::from_secs(5);

/// The longest between two log lines about one organization.
const ALERT_LOG_INTERVAL: Duration = Duration::from_secs(300);

/// The chunks an over-budget organization is given in `throttle` mode, per batch and per
/// reconciler pass: it is worked one chunk at a time. (graph-index does the same, one chunk.)
pub(crate) const THROTTLE_UNITS: usize = 1;

/// What to do when the budget is used up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum BudgetMode {
    /// Alert, and keep working. The system default.
    AlertOnly,
    /// Keep working, one chunk at a time.
    Throttle,
    /// Stop: no context is generated, and the units are `skipped_budget`.
    Stop,
}

impl BudgetMode {
    pub(crate) const ALL: [BudgetMode; 3] = [Self::AlertOnly, Self::Throttle, Self::Stop];

    /// A mode as `org_processing_policy.budget_mode` and `BACKGROUND_BUDGET_MODE` spell it.
    /// `inherit` is a value of the table only, and not a mode.
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "alert_only" => Some(Self::AlertOnly),
            "throttle" => Some(Self::Throttle),
            "stop" => Some(Self::Stop),
            _ => None,
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::AlertOnly => "alert_only",
            Self::Throttle => "throttle",
            Self::Stop => "stop",
        }
    }
}

/// What an organization's own row says about its budget. `None` is `inherit` (or no figure): the
/// deployment's value applies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct OrgBudget {
    pub mode: Option<BudgetMode>,
    pub monthly_usd: Option<f64>,
}

impl OrgBudget {
    /// What an organization with no row has: everything inherited.
    pub(crate) const INHERIT: OrgBudget = OrgBudget {
        mode: None,
        monthly_usd: None,
    };

    /// The two columns of a row. A value the table's own CHECK constraints and the admin API
    /// refuse is an error here, never a guess: it was written around both, and a gate that
    /// cannot be read is never read as open (the caller treats the organization as unknown).
    pub(crate) fn from_row(mode: &str, monthly_usd: Option<f64>) -> Result<Self, String> {
        let mode = match mode {
            "inherit" => None,
            other => Some(BudgetMode::parse(other).ok_or_else(|| {
                format!(
                    "unknown budget_mode `{other}` (expected inherit, alert_only, throttle or stop)"
                )
            })?),
        };
        if let Some(figure) = monthly_usd {
            if !figure.is_finite() || figure < 0.0 {
                return Err(format!(
                    "monthly_budget_usd `{figure}` is not a number from 0 up"
                ));
            }
        }
        Ok(Self { mode, monthly_usd })
    }
}

/// What a million tokens cost, from the deployment. Never defaulted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PriceTable {
    pub input_usd_per_mtok: f64,
    pub output_usd_per_mtok: f64,
}

impl PriceTable {
    pub(crate) fn spend_usd(&self, input_tokens: i64, output_tokens: i64) -> f64 {
        (input_tokens.max(0) as f64 * self.input_usd_per_mtok
            + output_tokens.max(0) as f64 * self.output_usd_per_mtok)
            / 1_000_000.0
    }
}

/// The deployment-wide budget: the mode an organization inherits, the figure an organization
/// without one inherits, and the prices that turn tokens into dollars.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GlobalBudget {
    pub mode: BudgetMode,
    pub monthly_usd: Option<f64>,
    pub price: Option<PriceTable>,
}

impl Default for GlobalBudget {
    fn default() -> Self {
        Self {
            mode: BudgetMode::AlertOnly,
            monthly_usd: None,
            price: None,
        }
    }
}

impl GlobalBudget {
    /// Read the four settings (as the strings the environment holds; blank means unset) and
    /// refuse a combination that cannot work. The error names the variable.
    pub(crate) fn parse(
        mode: &str,
        monthly_usd: &str,
        input_price: &str,
        output_price: &str,
    ) -> Result<Self, String> {
        let number = |name: &str, value: &str| -> Result<Option<f64>, String> {
            let value = value.trim();
            if value.is_empty() {
                return Ok(None);
            }
            match value.parse::<f64>() {
                Ok(parsed) if parsed.is_finite() && parsed >= 0.0 => Ok(Some(parsed)),
                _ => Err(format!("{name} must be a number from 0 up, got `{value}`")),
            }
        };
        let mode_text = mode.trim();
        let mode = if mode_text.is_empty() {
            BudgetMode::AlertOnly
        } else {
            BudgetMode::parse(mode_text).ok_or_else(|| {
                format!("BACKGROUND_BUDGET_MODE must be alert_only, throttle or stop, got `{mode_text}`")
            })?
        };
        let monthly_usd = number("BACKGROUND_BUDGET_MONTHLY_USD", monthly_usd)?;
        let input = number("BACKGROUND_PRICE_INPUT_USD_PER_MTOK", input_price)?;
        let output = number("BACKGROUND_PRICE_OUTPUT_USD_PER_MTOK", output_price)?;
        let price =
            match (input, output) {
                (Some(input_usd_per_mtok), Some(output_usd_per_mtok)) => Some(PriceTable {
                    input_usd_per_mtok,
                    output_usd_per_mtok,
                }),
                (None, None) => None,
                _ => return Err(
                    "BACKGROUND_PRICE_INPUT_USD_PER_MTOK and BACKGROUND_PRICE_OUTPUT_USD_PER_MTOK \
                     are set together or not at all: half a price table prices nothing"
                        .to_owned(),
                ),
            };
        // A hard stop on a figure nothing can measure would be a stop that never fires: refuse it
        // at boot rather than let an operator believe the budget is enforced.
        if mode != BudgetMode::AlertOnly && monthly_usd.is_some() && price.is_none() {
            return Err(format!(
                "BACKGROUND_BUDGET_MODE={} with BACKGROUND_BUDGET_MONTHLY_USD needs the price \
                 table (BACKGROUND_PRICE_INPUT_USD_PER_MTOK and BACKGROUND_PRICE_OUTPUT_USD_PER_MTOK): \
                 without it the spend cannot be measured and the {} would never fire",
                mode.as_str(),
                mode.as_str()
            ));
        }
        Ok(Self {
            mode,
            monthly_usd,
            price,
        })
    }

    /// Whether the deployment-wide settings alone make the gate act on spend (a figure with a
    /// mode that withholds or slows work), which is what needs the usage ledger to be recording.
    pub(crate) fn acts_on_spend(&self) -> bool {
        self.mode != BudgetMode::AlertOnly && self.monthly_usd.is_some()
    }
}

/// Why the month's spend could not be measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unmeasured {
    /// No price table: tokens cannot be turned into dollars.
    NoPrices,
    /// `USAGE_EMIT_ENABLED` is off: this service's own calls are not recorded, so the ledger
    /// holds a part of the spend and no more.
    LedgerOff,
    /// The ledger could not be read (and no earlier reading is held).
    LedgerUnreadable,
}

impl Unmeasured {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::NoPrices => "no_prices",
            Self::LedgerOff => "usage_ledger_off",
            Self::LedgerUnreadable => "ledger_unreadable",
        }
    }
}

/// What the gate decides for an organization right now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Verdict {
    /// No budget applies, or it is not used up.
    Proceed,
    /// Over budget in `alert_only`: alert, and proceed.
    Alert { spend_usd: f64, budget_usd: f64 },
    /// A budget applies and its spend cannot be measured. Alert, and proceed: never silent,
    /// never a stop on a guess.
    Unmeasured { budget_usd: f64, cause: Unmeasured },
    /// Over budget in `throttle`: one chunk at a time.
    Throttle { spend_usd: f64, budget_usd: f64 },
    /// Over budget in `stop`: generate nothing.
    Stop { spend_usd: f64, budget_usd: f64 },
}

impl Verdict {
    /// The most chunks that may be generated now, when the verdict caps it.
    pub(crate) const fn allowance(&self) -> Option<usize> {
        match self {
            Self::Throttle { .. } => Some(THROTTLE_UNITS),
            _ => None,
        }
    }

    /// Whether nothing may be generated.
    pub(crate) const fn withholds(&self) -> bool {
        matches!(self, Self::Stop { .. })
    }

    /// The mode that found the organization over its budget, if it did.
    const fn exceeded_mode(&self) -> Option<BudgetMode> {
        match self {
            Self::Alert { .. } => Some(BudgetMode::AlertOnly),
            Self::Throttle { .. } => Some(BudgetMode::Throttle),
            Self::Stop { .. } => Some(BudgetMode::Stop),
            Self::Proceed | Self::Unmeasured { .. } => None,
        }
    }
}

/// The mode and the figure that apply to an organization: its own where it set them, the
/// deployment's otherwise.
pub(crate) fn effective(org: &OrgBudget, global: &GlobalBudget) -> (BudgetMode, Option<f64>) {
    (
        org.mode.unwrap_or(global.mode),
        org.monthly_usd.or(global.monthly_usd),
    )
}

/// Whether the gate needs the month's spend at all: only an organization with a figure does.
pub(crate) fn needs_spend(org: &OrgBudget, global: &GlobalBudget) -> bool {
    effective(org, global).1.is_some()
}

/// The decision. Pure: the caller supplies the spend, or why there is none.
///
/// A budget of exactly zero is a budget that is used up before anything is spent, and `stop`
/// with it means "do not run": the comparison is `spend >= budget`.
pub(crate) fn decide(
    org: &OrgBudget,
    global: &GlobalBudget,
    spend: Result<f64, Unmeasured>,
) -> Verdict {
    let (mode, figure) = effective(org, global);
    let Some(budget_usd) = figure else {
        return Verdict::Proceed;
    };
    let spend_usd = match spend {
        Ok(spend_usd) => spend_usd,
        Err(cause) => return Verdict::Unmeasured { budget_usd, cause },
    };
    if spend_usd < budget_usd {
        return Verdict::Proceed;
    }
    match mode {
        BudgetMode::AlertOnly => Verdict::Alert {
            spend_usd,
            budget_usd,
        },
        BudgetMode::Throttle => Verdict::Throttle {
            spend_usd,
            budget_usd,
        },
        BudgetMode::Stop => Verdict::Stop {
            spend_usd,
            budget_usd,
        },
    }
}

// ── the ledger ──────────────────────────────────────────────────────────────

pub(crate) type SpendFuture<'a, T> = Pin<Box<dyn Future<Output = anyhow::Result<T>> + Send + 'a>>;

/// Where the month's background spend is read from.
pub(crate) trait SpendSource: Send + Sync {
    /// `(input tokens, output tokens)` of the organization's background LLM calls since the
    /// first of this month, UTC.
    fn month_llm_tokens<'a>(&'a self, org_id: &'a str) -> SpendFuture<'a, (i64, i64)>;
}

/// The month's background LLM tokens of an organization, from the usage ledger: every version 2
/// event of kind `llm` (contextual retrieval, graph extraction, community summaries) since the
/// first of the month, UTC. The statement is graph-index's, word for word (a test holds them
/// together): the budget is one figure, and both consumers must measure it the same way. An
/// embedding or a rerank is another event type, a version 1 row has no exact tokens, and the
/// organization is named explicitly because the service connects as the superuser, for which
/// row-level security is inert.
pub(crate) const MONTH_TOKENS_SQL: &str = "SELECT COALESCE(SUM(input_tokens), 0)::bigint, \
            COALESCE(SUM(output_tokens), 0)::bigint \
       FROM cost_events \
      WHERE org_id = $1 AND schema_version = 2 AND event_type = 'llm' \
        AND created_at >= date_trunc('month', now() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC'";

/// `cost_events` in Postgres.
pub(crate) struct PgSpendSource {
    pool: PgPool,
}

impl PgSpendSource {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl SpendSource for PgSpendSource {
    fn month_llm_tokens<'a>(&'a self, org_id: &'a str) -> SpendFuture<'a, (i64, i64)> {
        Box::pin(async move {
            let row: (i64, i64) = sqlx::query_as(MONTH_TOKENS_SQL)
                .bind(org_id)
                .fetch_one(&self.pool)
                .await?;
            Ok(row)
        })
    }
}

// ── the gate ────────────────────────────────────────────────────────────────

/// What is held for one organization.
#[derive(Default)]
struct OrgState {
    /// The last good reading of the ledger, in dollars, and when it was made.
    reading: Option<(f64, Instant)>,
    /// What this process has spent since that reading (see the module documentation).
    local_usd: f64,
    /// After a failed read: do not read again before this.
    retry_not_before: Option<Instant>,
    /// When `ctx_budget_exceeded_total` last counted this organization, and for which mode.
    counted: Option<(BudgetMode, Instant)>,
    /// When the last log line about this organization was written, and for which kind.
    logged: Option<(&'static str, Instant)>,
}

/// The gate: one per process, shared by the stream consumer and the reconciler.
pub(crate) struct BudgetGate {
    global: GlobalBudget,
    /// `USAGE_EMIT_ENABLED`: whether this service's own paid calls are recorded in the ledger.
    ledger_recording: bool,
    source: Arc<dyn SpendSource>,
    state: Mutex<HashMap<String, OrgState>>,
    exceeded: [AtomicU64; 3],
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl BudgetGate {
    pub(crate) fn new(
        global: GlobalBudget,
        ledger_recording: bool,
        source: Arc<dyn SpendSource>,
    ) -> Self {
        Self {
            global,
            ledger_recording,
            source,
            state: Mutex::new(HashMap::new()),
            exceeded: [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)],
        }
    }

    /// The gate the service and `context-backfill` run: the deployment's settings (checked at
    /// startup by [`crate::config::Config::background_budget`]) over the real ledger.
    pub(crate) fn from_config(cfg: &crate::config::Config, pool: PgPool) -> anyhow::Result<Self> {
        let global = cfg.background_budget()?;
        Ok(Self::new(
            global,
            cfg.usage_emit_enabled,
            Arc::new(PgSpendSource::new(pool)),
        ))
    }

    /// What to do for an organization now.
    ///
    /// An organization with no figure (its own or the deployment's) is `Proceed` without a read:
    /// the default costs nothing. Otherwise the month's spend is read (kept for 30 seconds, plus
    /// what this process has spent since) and decided. Infallible: spend that cannot be measured
    /// is [`Verdict::Unmeasured`], which proceeds and says so.
    pub(crate) async fn check(&self, org_id: &str, org: &OrgBudget) -> Verdict {
        if !needs_spend(org, &self.global) {
            return Verdict::Proceed;
        }
        let measured = self.measure(org_id).await;
        let verdict = decide(org, &self.global, measured);
        self.account(org_id, &verdict);
        verdict
    }

    /// Add what a call this process made cost to the organization's figure, until the ledger is
    /// read again. A call answered from inference-core's response cache cost nothing: the caller
    /// passes only billed tokens. A no-op without a price table, and for an organization the
    /// gate holds no reading for (its first check reads the ledger, which has everything).
    pub(crate) fn record_spend(&self, org_id: &str, input_tokens: u64, output_tokens: u64) {
        let Some(price) = self.global.price else {
            return;
        };
        let cost = price.spend_usd(
            i64::try_from(input_tokens).unwrap_or(i64::MAX),
            i64::try_from(output_tokens).unwrap_or(i64::MAX),
        );
        if let Some(entry) = lock(&self.state).get_mut(org_id) {
            if entry.reading.is_some() {
                entry.local_usd += cost;
            }
        }
    }

    async fn measure(&self, org_id: &str) -> Result<f64, Unmeasured> {
        let Some(price) = self.global.price else {
            return Err(Unmeasured::NoPrices);
        };
        if !self.ledger_recording {
            return Err(Unmeasured::LedgerOff);
        }
        let now = Instant::now();
        {
            let mut state = lock(&self.state);
            let entry = state.entry(org_id.to_owned()).or_default();
            match entry.reading {
                Some((usd, at)) if now.saturating_duration_since(at) < SPEND_CACHE_TTL => {
                    return Ok(usd + entry.local_usd);
                }
                Some((usd, _)) if entry.retry_not_before.is_some_and(|until| now < until) => {
                    // A read failed a moment ago: the last reading, as it was, until then.
                    return Ok(usd + entry.local_usd);
                }
                None if entry.retry_not_before.is_some_and(|until| now < until) => {
                    return Err(Unmeasured::LedgerUnreadable);
                }
                _ => {}
            }
        }
        match self.source.month_llm_tokens(org_id).await {
            Ok((input, output)) => {
                let usd = price.spend_usd(input, output);
                let mut state = lock(&self.state);
                let entry = state.entry(org_id.to_owned()).or_default();
                entry.reading = Some((usd, now));
                entry.local_usd = 0.0;
                entry.retry_not_before = None;
                Ok(usd)
            }
            Err(error) => {
                tracing::warn!(
                    org_id,
                    %error,
                    "the background spend could not be read from the usage ledger"
                );
                let mut state = lock(&self.state);
                let entry = state.entry(org_id.to_owned()).or_default();
                entry.retry_not_before = Some(now + RETRY_AFTER_FAILED_READ);
                match entry.reading {
                    // An earlier reading is a lower bound: kept, never turned into a stop.
                    Some((usd, _)) => Ok(usd + entry.local_usd),
                    None => Err(Unmeasured::LedgerUnreadable),
                }
            }
        }
    }

    /// Count and log what the verdict found, within the bounds the module documentation gives.
    fn account(&self, org_id: &str, verdict: &Verdict) {
        let now = Instant::now();
        let mut state = lock(&self.state);
        let entry = state.entry(org_id.to_owned()).or_default();
        if let Some(mode) = verdict.exceeded_mode() {
            let due = match entry.counted {
                Some((last, at)) => {
                    last != mode || now.saturating_duration_since(at) >= SPEND_CACHE_TTL
                }
                None => true,
            };
            if due {
                entry.counted = Some((mode, now));
                self.exceeded[Self::slot(mode)].fetch_add(1, Ordering::Relaxed);
            }
        }
        let kind = match verdict {
            Verdict::Proceed => return,
            Verdict::Alert { .. } => "alert_only",
            Verdict::Throttle { .. } => "throttle",
            Verdict::Stop { .. } => "stop",
            Verdict::Unmeasured { .. } => "unmeasured",
        };
        let write = match entry.logged {
            Some((last, at)) => {
                last != kind || now.saturating_duration_since(at) >= ALERT_LOG_INTERVAL
            }
            None => true,
        };
        if !write {
            return;
        }
        entry.logged = Some((kind, now));
        drop(state);
        match *verdict {
            Verdict::Proceed => {}
            Verdict::Alert {
                spend_usd,
                budget_usd,
            } => tracing::warn!(
                alert = "ctx_budget_exceeded",
                org_id,
                mode = "alert_only",
                spend_usd,
                budget_usd,
                "the background LLM budget is exceeded; alert only, nothing is stopped"
            ),
            Verdict::Throttle {
                spend_usd,
                budget_usd,
            } => tracing::warn!(
                alert = "ctx_budget_exceeded",
                org_id,
                mode = "throttle",
                spend_usd,
                budget_usd,
                "the background LLM budget is exceeded and the budget mode is throttle: contexts are generated one chunk at a time, the rest wait"
            ),
            Verdict::Stop {
                spend_usd,
                budget_usd,
            } => tracing::warn!(
                alert = "ctx_budget_exceeded",
                org_id,
                mode = "stop",
                spend_usd,
                budget_usd,
                "the background LLM budget is used up and the budget mode is stop: no context is generated, the units are marked skipped_budget until the budget allows it again"
            ),
            Verdict::Unmeasured { budget_usd, cause } => tracing::warn!(
                alert = "ctx_budget_unmeasured",
                org_id,
                budget_usd,
                cause = cause.as_str(),
                "a background LLM budget applies and its spend cannot be measured, so it is not enforced (BACKGROUND_PRICE_INPUT_USD_PER_MTOK and BACKGROUND_PRICE_OUTPUT_USD_PER_MTOK, USAGE_EMIT_ENABLED and the ledger must all be in place); proceeding"
            ),
        }
    }

    fn slot(mode: BudgetMode) -> usize {
        match mode {
            BudgetMode::AlertOnly => 0,
            BudgetMode::Throttle => 1,
            BudgetMode::Stop => 2,
        }
    }

    /// How many times the gate found an organization over its budget, by mode.
    pub(crate) fn exceeded_total(&self, mode: BudgetMode) -> u64 {
        self.exceeded[Self::slot(mode)].load(Ordering::Relaxed)
    }

    /// The counter in the Prometheus text exposition format, for `GET /metrics`. Labelled by
    /// mode only: an organization id is an unbounded label, and it is in the log line.
    pub(crate) fn render_metrics(&self) -> String {
        let mut out = String::from(
            "# HELP ctx_budget_exceeded_total Times the background LLM budget gate found an \
             organization over its budget, by the budget mode that applied (counted at most once \
             per organization per 30-second spend reading).\n\
             # TYPE ctx_budget_exceeded_total counter\n",
        );
        for mode in BudgetMode::ALL {
            out.push_str(&format!(
                "ctx_budget_exceeded_total{{mode=\"{}\"}} {}\n",
                mode.as_str(),
                self.exceeded_total(mode)
            ));
        }
        out
    }
}

/// What the tests that need a gate stand on: a ledger they script, and a price table that makes
/// a dollar a round number of tokens.
#[cfg(test)]
pub(crate) mod testing {
    use std::sync::atomic::AtomicUsize;

    use super::*;

    /// One dollar per million input tokens and nothing per output token: `usd * 1e6` tokens is
    /// `usd` dollars.
    pub(crate) const PRICE: PriceTable = PriceTable {
        input_usd_per_mtok: 1.0,
        output_usd_per_mtok: 0.0,
    };

    /// A ledger that answers what the test says, and counts how often it was asked.
    #[derive(Default)]
    pub(crate) struct ScriptedSpend {
        usd: Mutex<HashMap<String, f64>>,
        failing: Mutex<bool>,
        reads: AtomicUsize,
        asked: Mutex<Vec<String>>,
    }

    impl ScriptedSpend {
        pub(crate) fn new() -> Arc<Self> {
            Arc::new(Self::default())
        }

        pub(crate) fn set_usd(&self, org: &str, usd: f64) {
            lock(&self.usd).insert(org.to_owned(), usd);
        }

        pub(crate) fn fail(&self, failing: bool) {
            *lock(&self.failing) = failing;
        }

        pub(crate) fn reads(&self) -> usize {
            self.reads.load(Ordering::SeqCst)
        }

        pub(crate) fn asked(&self) -> Vec<String> {
            lock(&self.asked).clone()
        }
    }

    impl SpendSource for ScriptedSpend {
        fn month_llm_tokens<'a>(&'a self, org_id: &'a str) -> SpendFuture<'a, (i64, i64)> {
            Box::pin(async move {
                self.reads.fetch_add(1, Ordering::SeqCst);
                lock(&self.asked).push(org_id.to_owned());
                if *lock(&self.failing) {
                    anyhow::bail!("the ledger is unreachable");
                }
                let usd = lock(&self.usd).get(org_id).copied().unwrap_or(0.0);
                Ok(((usd * 1_000_000.0).round() as i64, 0))
            })
        }
    }

    /// A deployment-wide budget with the test price table and the given mode and figure.
    pub(crate) fn global(mode: BudgetMode, monthly_usd: Option<f64>) -> GlobalBudget {
        GlobalBudget {
            mode,
            monthly_usd,
            price: Some(PRICE),
        }
    }

    /// A gate over a scripted ledger that is recording.
    pub(crate) fn gate(global: GlobalBudget, spend: &Arc<ScriptedSpend>) -> BudgetGate {
        BudgetGate::new(global, true, spend.clone())
    }

    /// The gate every test that is not about the budget runs under: the default (alert only, no
    /// figure), over a ledger that must never be asked.
    pub(crate) fn idle_gate() -> (BudgetGate, Arc<ScriptedSpend>) {
        let spend = ScriptedSpend::new();
        (
            BudgetGate::new(GlobalBudget::default(), true, spend.clone()),
            spend,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    fn org(mode: Option<BudgetMode>, figure: Option<f64>) -> OrgBudget {
        OrgBudget {
            mode,
            monthly_usd: figure,
        }
    }

    fn global_of(mode: BudgetMode, figure: Option<f64>) -> GlobalBudget {
        GlobalBudget {
            mode,
            monthly_usd: figure,
            price: None,
        }
    }

    // ── what the table stores and what the environment says ─────────────────────────

    #[test]
    fn the_mode_reads_exactly_what_the_table_stores() {
        for (text, mode) in [
            ("alert_only", BudgetMode::AlertOnly),
            ("throttle", BudgetMode::Throttle),
            ("stop", BudgetMode::Stop),
        ] {
            assert_eq!(BudgetMode::parse(text), Some(mode));
            assert_eq!(mode.as_str(), text);
        }
        assert_eq!(BudgetMode::parse("inherit"), None, "inherit is not a mode");
        assert_eq!(
            BudgetMode::parse("STOP"),
            None,
            "the CHECK is case sensitive"
        );
        assert_eq!(BudgetMode::parse(""), None);
    }

    #[test]
    fn a_row_inherits_what_it_leaves_at_inherit_and_refuses_what_it_cannot_read() {
        assert_eq!(OrgBudget::from_row("inherit", None), Ok(OrgBudget::INHERIT));
        assert_eq!(
            OrgBudget::from_row("stop", Some(12.5)),
            Ok(org(Some(BudgetMode::Stop), Some(12.5)))
        );
        assert_eq!(
            OrgBudget::from_row("inherit", Some(0.0)),
            Ok(org(None, Some(0.0))),
            "a zero figure is a figure"
        );
        for bad in [
            OrgBudget::from_row("sometimes", None),
            OrgBudget::from_row("STOP", None),
            OrgBudget::from_row("", None),
            OrgBudget::from_row("inherit", Some(-1.0)),
            OrgBudget::from_row("inherit", Some(f64::NAN)),
            OrgBudget::from_row("inherit", Some(f64::INFINITY)),
        ] {
            assert!(bad.is_err(), "{bad:?}");
        }
    }

    #[test]
    fn tokens_are_priced_by_the_table_the_deployment_supplies() {
        let price = PriceTable {
            input_usd_per_mtok: 0.15,
            output_usd_per_mtok: 0.60,
        };
        // 2,000,000 prompt tokens and 500,000 completion tokens.
        let spend = price.spend_usd(2_000_000, 500_000);
        assert!((spend - 0.6).abs() < 1e-9, "{spend}");
        assert_eq!(price.spend_usd(0, 0), 0.0);
        assert_eq!(price.spend_usd(-5, -5), 0.0, "a negative count is zero");
    }

    #[test]
    fn the_deployments_settings_are_read_and_a_combination_that_cannot_work_is_refused() {
        // Blank is unset: what compose passes for `${VAR:-}`.
        assert_eq!(
            GlobalBudget::parse("", "", "", "").unwrap(),
            GlobalBudget::default()
        );
        assert_eq!(
            GlobalBudget::parse("throttle", "25", "0.15", "0.6").unwrap(),
            GlobalBudget {
                mode: BudgetMode::Throttle,
                monthly_usd: Some(25.0),
                price: Some(PriceTable {
                    input_usd_per_mtok: 0.15,
                    output_usd_per_mtok: 0.6
                }),
            }
        );
        for (mode, figure, input, output, needle) in [
            ("sometimes", "", "", "", "BACKGROUND_BUDGET_MODE"),
            ("", "-1", "", "", "BACKGROUND_BUDGET_MONTHLY_USD"),
            ("", "lots", "", "", "BACKGROUND_BUDGET_MONTHLY_USD"),
            ("", "NaN", "", "", "BACKGROUND_BUDGET_MONTHLY_USD"),
            ("", "", "0.15", "", "set together or not at all"),
            ("", "", "", "0.6", "set together or not at all"),
            (
                "",
                "",
                "-0.15",
                "0.6",
                "BACKGROUND_PRICE_INPUT_USD_PER_MTOK",
            ),
            ("stop", "25", "", "", "the stop would never fire"),
            ("throttle", "25", "", "", "the throttle would never fire"),
        ] {
            let error = GlobalBudget::parse(mode, figure, input, output)
                .expect_err(&format!("{mode}/{figure}/{input}/{output} cannot work"));
            assert!(error.contains(needle), "{error}");
        }
        // A stop with no figure has nothing to exceed, and a stop with a figure and prices works.
        assert!(GlobalBudget::parse("stop", "", "", "").is_ok());
        assert!(GlobalBudget::parse("stop", "25", "0.15", "0.6").is_ok());
        // Alert only with a figure and no prices is allowed: it alerts that it cannot measure.
        assert!(GlobalBudget::parse("alert_only", "25", "", "").is_ok());
    }

    #[test]
    fn the_deployment_acts_on_spend_only_with_a_mode_that_slows_or_stops_and_a_figure() {
        assert!(!GlobalBudget::default().acts_on_spend());
        assert!(!global_of(BudgetMode::AlertOnly, Some(5.0)).acts_on_spend());
        assert!(!global_of(BudgetMode::Stop, None).acts_on_spend());
        assert!(global_of(BudgetMode::Stop, Some(5.0)).acts_on_spend());
        assert!(global_of(BudgetMode::Throttle, Some(5.0)).acts_on_spend());
    }

    // ── the decision ────────────────────────────────────────────────────────────────

    /// Owner default D-E: exceeding a budget alerts and does not stop. No figure is built in.
    #[test]
    fn the_default_never_stops_and_has_no_figure() {
        let default = GlobalBudget::default();
        assert_eq!(default.mode, BudgetMode::AlertOnly);
        assert_eq!(default.monthly_usd, None);
        assert_eq!(default.price, None);
        // With no figure anywhere there is nothing to exceed, whatever the spend.
        assert_eq!(
            decide(&OrgBudget::INHERIT, &default, Ok(1e12)),
            Verdict::Proceed
        );
        assert_eq!(
            decide(&OrgBudget::INHERIT, &default, Err(Unmeasured::NoPrices)),
            Verdict::Proceed
        );
        assert!(!needs_spend(&OrgBudget::INHERIT, &default));
        // And with a figure and the default mode, being over it alerts and proceeds.
        let verdict = decide(&org(None, Some(5.0)), &default, Ok(7.5));
        assert_eq!(
            verdict,
            Verdict::Alert {
                spend_usd: 7.5,
                budget_usd: 5.0
            }
        );
        assert!(!verdict.withholds());
        assert_eq!(verdict.allowance(), None);
    }

    #[test]
    fn under_the_figure_everything_proceeds_in_every_mode() {
        for mode in BudgetMode::ALL {
            assert_eq!(
                decide(
                    &org(Some(mode), Some(10.0)),
                    &GlobalBudget::default(),
                    Ok(9.99)
                ),
                Verdict::Proceed,
                "{mode:?}"
            );
        }
    }

    #[test]
    fn over_the_figure_each_mode_does_what_the_owner_ruled() {
        let over = |mode| {
            decide(
                &org(Some(mode), Some(10.0)),
                &GlobalBudget::default(),
                Ok(10.0),
            )
        };
        assert!(matches!(over(BudgetMode::AlertOnly), Verdict::Alert { .. }));
        let throttle = over(BudgetMode::Throttle);
        assert!(matches!(throttle, Verdict::Throttle { .. }));
        assert_eq!(throttle.allowance(), Some(THROTTLE_UNITS));
        assert!(!throttle.withholds());
        let stop = over(BudgetMode::Stop);
        assert!(matches!(stop, Verdict::Stop { .. }));
        assert!(stop.withholds());
        // Exactly the figure is used up; a zero budget is used up before anything is spent.
        assert!(matches!(
            decide(
                &org(Some(BudgetMode::Stop), Some(0.0)),
                &GlobalBudget::default(),
                Ok(0.0)
            ),
            Verdict::Stop { .. }
        ));
    }

    #[test]
    fn an_organization_inherits_the_deployments_mode_and_figure_unless_it_sets_its_own() {
        let deployment = global_of(BudgetMode::Stop, Some(100.0));
        // Nothing of its own (no policy row): the deployment's mode and figure.
        assert_eq!(
            decide(&OrgBudget::INHERIT, &deployment, Ok(150.0)),
            Verdict::Stop {
                spend_usd: 150.0,
                budget_usd: 100.0
            }
        );
        // Its own figure wins; its mode is still inherited.
        assert_eq!(
            decide(&org(None, Some(500.0)), &deployment, Ok(150.0)),
            Verdict::Proceed
        );
        // Its own mode wins; the figure is inherited.
        assert!(matches!(
            decide(
                &org(Some(BudgetMode::AlertOnly), None),
                &deployment,
                Ok(150.0)
            ),
            Verdict::Alert { .. }
        ));
        assert_eq!(
            effective(&OrgBudget::INHERIT, &deployment),
            (BudgetMode::Stop, Some(100.0))
        );
    }

    /// A budget whose spend cannot be measured is never a stop on a guess and never silent.
    #[test]
    fn a_budget_that_cannot_be_measured_is_alerted_not_enforced() {
        for mode in BudgetMode::ALL {
            for cause in [
                Unmeasured::NoPrices,
                Unmeasured::LedgerOff,
                Unmeasured::LedgerUnreadable,
            ] {
                let verdict = decide(
                    &org(Some(mode), Some(10.0)),
                    &GlobalBudget::default(),
                    Err(cause),
                );
                assert_eq!(
                    verdict,
                    Verdict::Unmeasured {
                        budget_usd: 10.0,
                        cause
                    },
                    "{mode:?}/{cause:?}"
                );
                assert!(!verdict.withholds());
                assert_eq!(verdict.allowance(), None);
            }
        }
    }

    // ── the gate ────────────────────────────────────────────────────────────────────

    /// The default costs nothing: with no figure the ledger is not even asked, and nothing is
    /// counted. This is the "default mode never stops" test, at the level of the gate.
    #[tokio::test(start_paused = true)]
    async fn with_no_figure_the_gate_proceeds_without_reading_the_ledger() {
        let spend = ScriptedSpend::new();
        spend.set_usd("org-a", 1e9);
        let gate = gate(global(BudgetMode::AlertOnly, None), &spend);
        for _ in 0..3 {
            assert_eq!(
                gate.check("org-a", &OrgBudget::INHERIT).await,
                Verdict::Proceed
            );
        }
        assert_eq!(spend.reads(), 0, "the ledger was read for nobody");
        for mode in BudgetMode::ALL {
            assert_eq!(gate.exceeded_total(mode), 0);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn alert_only_counts_and_proceeds_and_never_stops() {
        let spend = ScriptedSpend::new();
        spend.set_usd("org-a", 30.0);
        // The deployment is alert-only (the default mode) and has a figure: over it, an
        // organization that sets nothing is alerted, whatever the spend.
        let gate = gate(global(BudgetMode::AlertOnly, Some(10.0)), &spend);
        let verdict = gate.check("org-a", &OrgBudget::INHERIT).await;
        assert_eq!(
            verdict,
            Verdict::Alert {
                spend_usd: 30.0,
                budget_usd: 10.0
            }
        );
        assert!(!verdict.withholds() && verdict.allowance().is_none());
        assert_eq!(gate.exceeded_total(BudgetMode::AlertOnly), 1);
        assert_eq!(gate.exceeded_total(BudgetMode::Stop), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn stop_is_explicit_on_the_organization_or_on_the_deployment() {
        let spend = ScriptedSpend::new();
        spend.set_usd("org-own", 30.0);
        spend.set_usd("org-inherits", 30.0);
        // The organization sets stop and its own figure on an alert-only deployment.
        let gate_a = gate(global(BudgetMode::AlertOnly, None), &spend);
        let own = org(Some(BudgetMode::Stop), Some(10.0));
        assert!(gate_a.check("org-own", &own).await.withholds());
        // An organization with no row of a stop deployment inherits the stop.
        let gate_b = gate(global(BudgetMode::Stop, Some(10.0)), &spend);
        assert!(gate_b
            .check("org-inherits", &OrgBudget::INHERIT)
            .await
            .withholds());
        // And an organization may opt out of the deployment's stop.
        assert!(!gate_b
            .check("org-inherits", &org(Some(BudgetMode::AlertOnly), None))
            .await
            .withholds());
    }

    #[tokio::test(start_paused = true)]
    async fn the_spend_is_kept_for_thirty_seconds_and_read_again_after() {
        let spend = ScriptedSpend::new();
        spend.set_usd("org-a", 1.0);
        let gate = gate(global(BudgetMode::Stop, Some(10.0)), &spend);
        assert_eq!(
            gate.check("org-a", &OrgBudget::INHERIT).await,
            Verdict::Proceed
        );
        assert_eq!(spend.reads(), 1);

        // The ledger moves, the answer does not, inside the lifetime.
        spend.set_usd("org-a", 50.0);
        tokio::time::advance(Duration::from_secs(29)).await;
        assert_eq!(
            gate.check("org-a", &OrgBudget::INHERIT).await,
            Verdict::Proceed
        );
        assert_eq!(spend.reads(), 1, "no read inside the lifetime");

        tokio::time::advance(Duration::from_secs(2)).await;
        assert!(gate.check("org-a", &OrgBudget::INHERIT).await.withholds());
        assert_eq!(spend.reads(), 2);
        // Another organization has its own reading.
        spend.set_usd("org-b", 1.0);
        assert_eq!(
            gate.check("org-b", &OrgBudget::INHERIT).await,
            Verdict::Proceed
        );
        assert_eq!(spend.asked(), ["org-a", "org-a", "org-b"]);
    }

    /// The gate stops before the next provider call: what a batch just spent is added to the
    /// organization's figure at once, without waiting for the ledger to catch up.
    #[tokio::test(start_paused = true)]
    async fn a_batch_that_crosses_the_figure_is_not_followed_by_one_that_does_not_know() {
        let spend = ScriptedSpend::new();
        spend.set_usd("org-a", 9.0);
        let gate = gate(global(BudgetMode::Stop, Some(10.0)), &spend);
        assert_eq!(
            gate.check("org-a", &OrgBudget::INHERIT).await,
            Verdict::Proceed
        );

        // The batch spends two dollars (2,000,000 input tokens at a dollar a million). The ledger
        // has not seen it: the scripted figure is still 9.
        gate.record_spend("org-a", 2_000_000, 0);
        assert!(
            gate.check("org-a", &OrgBudget::INHERIT).await.withholds(),
            "the next check must see 9 + 2 against 10"
        );
        assert_eq!(
            spend.reads(),
            1,
            "and it did so without reading the ledger again"
        );

        // When the ledger is read again its figure replaces the local one: it includes the batch.
        spend.set_usd("org-a", 11.0);
        tokio::time::advance(SPEND_CACHE_TTL + Duration::from_secs(1)).await;
        match gate.check("org-a", &OrgBudget::INHERIT).await {
            Verdict::Stop { spend_usd, .. } => assert!((spend_usd - 11.0).abs() < 1e-9),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn spend_recorded_for_an_organization_the_gate_holds_nothing_for_is_ignored() {
        let spend = ScriptedSpend::new();
        spend.set_usd("org-a", 1.0);
        let gate = gate(global(BudgetMode::Stop, Some(10.0)), &spend);
        // No reading yet: the first check reads the ledger, which has everything.
        gate.record_spend("org-a", 50_000_000, 0);
        assert_eq!(
            gate.check("org-a", &OrgBudget::INHERIT).await,
            Verdict::Proceed
        );
        // Without a price table there is nothing to add.
        let no_prices = BudgetGate::new(GlobalBudget::default(), true, ScriptedSpend::new());
        no_prices.record_spend("org-a", 50_000_000, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn a_budget_that_cannot_be_measured_is_alerted_never_stopped_on_a_guess() {
        let figure = org(Some(BudgetMode::Stop), Some(10.0));
        // No prices.
        let spend = ScriptedSpend::new();
        spend.set_usd("org-a", 1e6);
        let gate_a = BudgetGate::new(GlobalBudget::default(), true, spend.clone());
        assert_eq!(
            gate_a.check("org-a", &figure).await,
            Verdict::Unmeasured {
                budget_usd: 10.0,
                cause: Unmeasured::NoPrices
            }
        );
        // The usage ledger is off: this service's own calls are not in it.
        let priced = global(BudgetMode::AlertOnly, None);
        let gate_b = BudgetGate::new(priced, false, spend.clone());
        assert_eq!(
            gate_b.check("org-a", &figure).await,
            Verdict::Unmeasured {
                budget_usd: 10.0,
                cause: Unmeasured::LedgerOff
            }
        );
        assert_eq!(spend.reads(), 0, "neither case asked the ledger");
        // The ledger cannot be read, and nothing was read before.
        spend.fail(true);
        let gate_c = gate(priced, &spend);
        assert_eq!(
            gate_c.check("org-a", &figure).await,
            Verdict::Unmeasured {
                budget_usd: 10.0,
                cause: Unmeasured::LedgerUnreadable
            }
        );
        // And nothing is counted as exceeded: the budget was not found to be.
        for mode in BudgetMode::ALL {
            assert_eq!(
                gate_a.exceeded_total(mode)
                    + gate_b.exceeded_total(mode)
                    + gate_c.exceeded_total(mode),
                0
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_read_keeps_the_last_reading_and_waits_five_seconds() {
        let spend = ScriptedSpend::new();
        spend.set_usd("org-a", 30.0);
        let gate = gate(global(BudgetMode::Stop, Some(10.0)), &spend);
        assert!(gate.check("org-a", &OrgBudget::INHERIT).await.withholds());
        assert_eq!(spend.reads(), 1);

        // The reading is old and the ledger is down: the earlier reading is a lower bound and
        // is used (an organization that is over its budget does not become free because the
        // database blinked).
        spend.fail(true);
        tokio::time::advance(SPEND_CACHE_TTL + Duration::from_secs(1)).await;
        assert!(gate.check("org-a", &OrgBudget::INHERIT).await.withholds());
        assert_eq!(spend.reads(), 2);
        // Not asked again for five seconds.
        assert!(gate.check("org-a", &OrgBudget::INHERIT).await.withholds());
        assert_eq!(spend.reads(), 2);
        tokio::time::advance(Duration::from_secs(6)).await;
        spend.fail(false);
        spend.set_usd("org-a", 2.0);
        assert_eq!(
            gate.check("org-a", &OrgBudget::INHERIT).await,
            Verdict::Proceed
        );
        assert_eq!(spend.reads(), 3);
    }

    /// The counter counts an organization once per reading, not once per batch, so that a busy
    /// over-budget organization is a rate an alert rule can read.
    #[tokio::test(start_paused = true)]
    async fn the_counter_counts_an_organization_once_per_reading_and_per_mode() {
        let spend = ScriptedSpend::new();
        spend.set_usd("org-a", 30.0);
        spend.set_usd("org-b", 30.0);
        let gate = gate(global(BudgetMode::AlertOnly, Some(10.0)), &spend);
        for _ in 0..5 {
            gate.check("org-a", &OrgBudget::INHERIT).await;
        }
        assert_eq!(gate.exceeded_total(BudgetMode::AlertOnly), 1);
        gate.check("org-b", &OrgBudget::INHERIT).await;
        assert_eq!(gate.exceeded_total(BudgetMode::AlertOnly), 2);
        // The next reading counts again.
        tokio::time::advance(SPEND_CACHE_TTL + Duration::from_secs(1)).await;
        gate.check("org-a", &OrgBudget::INHERIT).await;
        assert_eq!(gate.exceeded_total(BudgetMode::AlertOnly), 3);
        // A change of mode is a count of its own.
        gate.check("org-a", &org(Some(BudgetMode::Stop), None))
            .await;
        assert_eq!(gate.exceeded_total(BudgetMode::Stop), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn the_metric_is_rendered_in_the_prometheus_text_format() {
        let spend = ScriptedSpend::new();
        spend.set_usd("org-a", 30.0);
        let gate = gate(global(BudgetMode::AlertOnly, Some(10.0)), &spend);
        let before = gate.render_metrics();
        assert!(
            before.contains("# TYPE ctx_budget_exceeded_total counter"),
            "{before}"
        );
        for mode in ["alert_only", "throttle", "stop"] {
            assert!(
                before.contains(&format!("ctx_budget_exceeded_total{{mode=\"{mode}\"}} 0\n")),
                "{before}"
            );
        }
        gate.check("org-a", &OrgBudget::INHERIT).await;
        let after = gate.render_metrics();
        assert!(
            after.contains("ctx_budget_exceeded_total{mode=\"alert_only\"} 1\n"),
            "{after}"
        );
        assert!(
            !after.contains("org-a"),
            "an organization id is not a metric label: {after}"
        );
    }

    // ── what the two consumers must agree on ────────────────────────────────────────

    /// The budget is one figure, measured by two services. graph-index's statement and ours must
    /// stay the same text: a drift would let one of them count a month the other does not.
    #[test]
    fn the_spend_statement_is_the_one_graph_index_runs() {
        let theirs = include_str!("../../graph-index-rs/src/state_store.rs");
        let start = theirs
            .find("const MONTH_TOKENS_SQL: &str = ")
            .expect("graph-index's statement");
        let rest = &theirs[start..];
        // The literal is the text between the first two quotes (the statement holds none).
        let open = rest.find('"').expect("an opening quote") + 1;
        let close = open + rest[open..].find('"').expect("a closing quote");
        let literal = &rest[open..close];
        // A backslash at the end of a line continues the string and skips the next line's
        // indentation, as the compiler reads it.
        let mut theirs_sql = String::new();
        let mut chars = literal.chars().peekable();
        while let Some(character) = chars.next() {
            if character == '\\' && matches!(chars.peek(), Some('\n' | '\r')) {
                while chars.peek().is_some_and(|next| next.is_whitespace()) {
                    chars.next();
                }
                continue;
            }
            theirs_sql.push(character);
        }
        assert_eq!(
            MONTH_TOKENS_SQL, theirs_sql,
            "the budget is measured by two services: change both statements together"
        );
    }

    /// The four settings and their spelling are graph-index's, and compose passes them to both.
    #[test]
    fn the_settings_are_spelled_the_way_graph_index_spells_them() {
        let theirs = include_str!("../../graph-index-rs/src/budget.rs");
        for name in [
            "BACKGROUND_BUDGET_MODE",
            "BACKGROUND_BUDGET_MONTHLY_USD",
            "BACKGROUND_PRICE_INPUT_USD_PER_MTOK",
            "BACKGROUND_PRICE_OUTPUT_USD_PER_MTOK",
        ] {
            assert!(theirs.contains(name), "graph-index no longer reads {name}");
            assert!(
                include_str!("budget.rs").contains(name),
                "this service no longer reads {name}"
            );
        }
    }
}
