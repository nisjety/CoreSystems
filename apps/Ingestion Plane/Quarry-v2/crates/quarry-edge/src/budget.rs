//! Budgets for agent-class browser runs: steps, attempts, active runtime and
//! cost, with a typed `BUDGET_EXHAUSTED` (402) naming the dimension.
//!
//! Until this module the only run bound was `constraints.max_steps` taken from
//! the request, where `0` meant unlimited: a model-driven run that never sent a
//! limit could loop forever and spend without bound. The rules here:
//!
//! - **Two classes.** A *human* session ([`RunClass::Interactive`]) is never
//!   capped; an *agent* run ([`RunClass::Agent`]) always is. The class is
//!   derived from the caller and can only be tightened: a service principal, or
//!   a body that says `initiated_by: "agent"`, is an agent; nothing a caller
//!   sends makes an agent run interactive, and a resumed run keeps the stricter
//!   of its stored and derived class.
//! - **Absent is the default, never unlimited.** A missing or zero limit takes
//!   the agent default; a limit above the ceiling is clamped to it; both are
//!   reported as [`Clamp`]s with their source.
//! - **Attempts, not only steps.** A failing driver would otherwise never reach
//!   the step cap because a failed action is not a completed step; the attempt
//!   allowance is `max_steps * factor`.
//! - **Runtime is active time.** The clock stops while a human has taken over.
//! - **Cost is refused before the overshoot**, from the flat action table plus a
//!   browser-time term, and is labelled estimated.
//! - **Exhaustion latches**, and a resume restores the ledger, so neither a
//!   repeat call nor a resume refills a spent budget.
//! - **A grant may carry a tighter claim** (WS6.E). When `QUARRY_EDGE__GRANT_BUDGET_CLAIM`
//!   is `enforce`, the broker grant's claim is applied once, at start: each
//!   dimension takes the smaller of what the request, default and ceiling give
//!   and what the claim says, and a claim that is not smaller changes nothing,
//!   so a claim can never raise a bound. The applied claim is stored with the
//!   ledger, so a resume cannot drop it.
//!
//! Everything numeric here is a placeholder pending the ratified budget numbers
//! (plan decision X-12), which is why enforcement ships as `shadow`: it records
//! what it *would* refuse, by class, and refuses nothing until an operator sets
//! `QUARRY_EDGE__BUDGET_ENFORCEMENT=on` after reading that record.

use std::sync::OnceLock;
use std::time::Instant;

use quarry_core::contracts::AgentConstraints;
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_runtime::grant_validator::GrantBudget;
use quarry_runtime::step_receipts::BudgetCheckpoint;

pub const ENV_MODE: &str = "QUARRY_EDGE__BUDGET_ENFORCEMENT";

const MICRO: f64 = 1_000_000.0;

/// What the gate does when a limit is reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetMode {
    /// No budget accounting or gate (rollback).
    Off,
    /// Account and record what would be refused; refuse nothing. The default.
    Shadow,
    /// Refuse with `BUDGET_EXHAUSTED`.
    On,
}

impl BudgetMode {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "off" | "0" | "false" => Ok(Self::Off),
            "" | "shadow" => Ok(Self::Shadow),
            "on" | "1" | "true" => Ok(Self::On),
            other => Err(format!(
                "{ENV_MODE} must be off, shadow or on, got {other:?}"
            )),
        }
    }

    pub fn from_env() -> Result<Self, String> {
        match std::env::var(ENV_MODE) {
            Ok(value) => Self::parse(&value),
            Err(_) => Ok(Self::Shadow),
        }
    }

    /// The process's mode, read once. An unreadable value is rejected at boot
    /// (`from_env` in `main`), so this only falls back to the default for code
    /// that runs without that check.
    pub fn cached() -> Self {
        static MODE: OnceLock<BudgetMode> = OnceLock::new();
        *MODE.get_or_init(|| Self::from_env().unwrap_or(Self::Shadow))
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Shadow => "shadow",
            Self::On => "on",
        }
    }
}

pub const ENV_CLAIM_MODE: &str = "QUARRY_EDGE__GRANT_BUDGET_CLAIM";

/// Whether a grant's budget claim is applied to the run it backs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimMode {
    /// The claim is read and carried, and changes nothing. The default: the
    /// broker has no production grants until Quarry holds its service
    /// credential, and a claim must not bind before the minter is deployed.
    Ignore,
    /// A claim tightens the run's bounds, and a grant of kind `run` makes the
    /// run an agent run (R-8).
    Enforce,
}

impl ClaimMode {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "ignore" => Ok(Self::Ignore),
            "enforce" => Ok(Self::Enforce),
            other => Err(format!(
                "{ENV_CLAIM_MODE} must be ignore or enforce, got {other:?}"
            )),
        }
    }

    pub fn from_env() -> Result<Self, String> {
        match std::env::var(ENV_CLAIM_MODE) {
            Ok(value) => Self::parse(&value),
            Err(_) => Ok(Self::Ignore),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Ignore => "ignore",
            Self::Enforce => "enforce",
        }
    }
}

/// The claim mode the start path uses. Read from the environment on each run
/// start (one variable lookup, like the take-over switch), so a test can
/// change it; an unreadable value is rejected at boot (`from_env` in `main`),
/// and falls back to `ignore` for code that runs without that check.
pub fn current_claim_mode() -> ClaimMode {
    ClaimMode::from_env().unwrap_or(ClaimMode::Ignore)
}

/// The mode the gate uses: the process's cached mode, except that a test can
/// pin its own on the current thread (the cached one is read once per process,
/// so tests in one binary could not otherwise differ).
pub fn current_mode() -> BudgetMode {
    #[cfg(test)]
    if let Some(mode) = test_mode::get() {
        return mode;
    }
    BudgetMode::cached()
}

#[cfg(test)]
pub mod test_mode {
    use super::BudgetMode;
    use std::cell::Cell;

    thread_local! {
        static MODE: Cell<Option<BudgetMode>> = const { Cell::new(None) };
    }

    pub fn get() -> Option<BudgetMode> {
        MODE.with(Cell::get)
    }

    /// Pin the mode for this thread until the guard drops.
    pub struct Pinned;

    pub fn pin(mode: BudgetMode) -> Pinned {
        MODE.with(|cell| cell.set(Some(mode)));
        Pinned
    }

    impl Drop for Pinned {
        fn drop(&mut self) {
            MODE.with(|cell| cell.set(None));
        }
    }
}

/// Who is driving the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunClass {
    Interactive,
    Agent,
}

impl RunClass {
    pub fn label(self) -> &'static str {
        match self {
            Self::Interactive => "interactive",
            Self::Agent => "agent",
        }
    }

    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "interactive" => Some(Self::Interactive),
            "agent" => Some(Self::Agent),
            _ => None,
        }
    }

    /// Tighten-only: the stricter of two classes.
    pub fn stricter(self, other: Self) -> Self {
        if self == Self::Agent || other == Self::Agent {
            Self::Agent
        } else {
            Self::Interactive
        }
    }
}

/// Derive the class from the verified caller and the request hint.
pub fn classify(is_service_principal: bool, initiated_by: Option<&str>) -> RunClass {
    let hinted_agent = initiated_by.is_some_and(|value| value.trim().eq_ignore_ascii_case("agent"));
    if is_service_principal || hinted_agent {
        RunClass::Agent
    } else {
        RunClass::Interactive
    }
}

/// Defaults, ceilings and prices. Placeholders until X-12.
#[derive(Debug, Clone, PartialEq)]
pub struct BudgetLimits {
    pub default_max_steps: u32,
    pub default_max_runtime_s: u32,
    pub default_max_cost_usd: f64,
    pub ceiling_max_steps: u32,
    pub ceiling_max_runtime_s: u32,
    pub ceiling_max_cost_usd: f64,
    /// Failed attempts allowed per step of the step cap.
    pub attempt_factor: u32,
    /// What a second of browser time is worth, until billing-core supplies it.
    pub browser_second_usd: f64,
}

impl Default for BudgetLimits {
    fn default() -> Self {
        Self {
            default_max_steps: 60,
            default_max_runtime_s: 900,
            default_max_cost_usd: 1.00,
            ceiling_max_steps: 300,
            ceiling_max_runtime_s: 3600,
            ceiling_max_cost_usd: 20.00,
            attempt_factor: 2,
            browser_second_usd: 0.00003,
        }
    }
}

impl BudgetLimits {
    /// Read the limits through `get`. An unset variable keeps the default; a
    /// value that is not a positive number, or a default above its ceiling, is
    /// an error that names the variable.
    pub fn from_env(get: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let mut limits = Self::default();
        let read_u32 = |name: &str, into: &mut u32| -> Result<(), String> {
            if let Some(raw) = get(name).filter(|value| !value.trim().is_empty()) {
                *into = raw
                    .trim()
                    .parse::<u32>()
                    .ok()
                    .filter(|value| *value > 0)
                    .ok_or_else(|| format!("{name} must be a positive integer"))?;
            }
            Ok(())
        };
        let read_f64 = |name: &str, into: &mut f64| -> Result<(), String> {
            if let Some(raw) = get(name).filter(|value| !value.trim().is_empty()) {
                *into = raw
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|value| value.is_finite() && *value > 0.0)
                    .ok_or_else(|| format!("{name} must be a positive number"))?;
            }
            Ok(())
        };
        read_u32(
            "QUARRY_EDGE__AGENT_DEFAULT_MAX_STEPS",
            &mut limits.default_max_steps,
        )?;
        read_u32(
            "QUARRY_EDGE__AGENT_DEFAULT_MAX_RUNTIME_S",
            &mut limits.default_max_runtime_s,
        )?;
        read_f64(
            "QUARRY_EDGE__AGENT_DEFAULT_MAX_COST_USD",
            &mut limits.default_max_cost_usd,
        )?;
        read_u32(
            "QUARRY_EDGE__AGENT_CEILING_MAX_STEPS",
            &mut limits.ceiling_max_steps,
        )?;
        read_u32(
            "QUARRY_EDGE__AGENT_CEILING_MAX_RUNTIME_S",
            &mut limits.ceiling_max_runtime_s,
        )?;
        read_f64(
            "QUARRY_EDGE__AGENT_CEILING_MAX_COST_USD",
            &mut limits.ceiling_max_cost_usd,
        )?;
        read_u32(
            "QUARRY_EDGE__BUDGET_ATTEMPT_FACTOR",
            &mut limits.attempt_factor,
        )?;
        read_f64(
            "QUARRY_EDGE__BUDGET_BROWSER_SECOND_USD",
            &mut limits.browser_second_usd,
        )?;
        if limits.default_max_steps > limits.ceiling_max_steps {
            return Err("QUARRY_EDGE__AGENT_DEFAULT_MAX_STEPS exceeds its ceiling".into());
        }
        if limits.default_max_runtime_s > limits.ceiling_max_runtime_s {
            return Err("QUARRY_EDGE__AGENT_DEFAULT_MAX_RUNTIME_S exceeds its ceiling".into());
        }
        if limits.default_max_cost_usd > limits.ceiling_max_cost_usd {
            return Err("QUARRY_EDGE__AGENT_DEFAULT_MAX_COST_USD exceeds its ceiling".into());
        }
        Ok(limits)
    }

    pub fn cached() -> &'static Self {
        static LIMITS: OnceLock<BudgetLimits> = OnceLock::new();
        LIMITS.get_or_init(|| Self::from_env(|name| std::env::var(name).ok()).unwrap_or_default())
    }
}

/// The effective bounds of one agent run.
#[derive(Debug, Clone, PartialEq)]
pub struct BudgetCaps {
    pub max_steps: u32,
    pub max_attempts: u32,
    pub max_runtime_s: u32,
    pub max_cost_usd: f64,
    /// One second of browser time, in micro-USD.
    pub browser_second_micro_usd: u64,
    /// Which bounds a grant's claim set (so a refusal can name its source).
    pub grant_bound: GrantBound,
}

/// The bounds that a grant's claim, not the request or the default, set.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GrantBound {
    pub steps: bool,
    pub runtime: bool,
    pub cost: bool,
}

/// A request value that was replaced, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct Clamp {
    pub field: &'static str,
    pub requested: Option<f64>,
    pub effective: f64,
    /// `default` (nothing usable was requested) or `ceiling`.
    pub by: &'static str,
}

fn usd_to_micro(usd: f64) -> u64 {
    if usd.is_finite() && usd > 0.0 {
        (usd * MICRO).round() as u64
    } else {
        0
    }
}

/// Resolve the bounds for a run. `None` for an interactive run: it is never
/// capped here (a request's own `max_steps` is still enforced by the existing
/// step check). Without a grant claim; the route resolves through
/// [`resolve_caps_with_claim`], so only the tests call this one.
#[cfg(test)]
pub fn resolve_caps(
    limits: &BudgetLimits,
    class: RunClass,
    requested: &AgentConstraints,
) -> Option<(BudgetCaps, Vec<Clamp>)> {
    resolve_caps_with_claim(limits, class, requested, None)
}

/// Replace the clamp recorded for `field` (if any) with one that says the
/// grant bound it. `requested` stays what the caller asked for.
fn grant_clamp(clamps: &mut Vec<Clamp>, field: &'static str, requested: Option<f64>, effective: f64) {
    clamps.retain(|clamp| clamp.field != field);
    clamps.push(Clamp {
        field,
        requested,
        effective,
        by: "grant",
    });
}

/// [`resolve_caps`] with a grant's budget claim (WS6.E). A claim only
/// tightens: a dimension it bounds below the value the request, default and
/// ceiling gave becomes the claim's (reported as `by: "grant"`); a claim at or
/// above that value, or a dimension it leaves at 0, changes nothing.
pub fn resolve_caps_with_claim(
    limits: &BudgetLimits,
    class: RunClass,
    requested: &AgentConstraints,
    claim: Option<&GrantBudget>,
) -> Option<(BudgetCaps, Vec<Clamp>)> {
    if class == RunClass::Interactive {
        return None;
    }
    let mut clamps = Vec::new();

    let mut max_steps = match requested.max_steps {
        0 => {
            clamps.push(Clamp {
                field: "max_steps",
                requested: None,
                effective: f64::from(limits.default_max_steps),
                by: "default",
            });
            limits.default_max_steps
        }
        value if value > limits.ceiling_max_steps => {
            clamps.push(Clamp {
                field: "max_steps",
                requested: Some(f64::from(value)),
                effective: f64::from(limits.ceiling_max_steps),
                by: "ceiling",
            });
            limits.ceiling_max_steps
        }
        value => value,
    };

    let mut max_runtime_s = match requested.max_runtime_s {
        None | Some(0) => {
            clamps.push(Clamp {
                field: "max_runtime_s",
                requested: None,
                effective: f64::from(limits.default_max_runtime_s),
                by: "default",
            });
            limits.default_max_runtime_s
        }
        Some(value) if value > limits.ceiling_max_runtime_s => {
            clamps.push(Clamp {
                field: "max_runtime_s",
                requested: Some(f64::from(value)),
                effective: f64::from(limits.ceiling_max_runtime_s),
                by: "ceiling",
            });
            limits.ceiling_max_runtime_s
        }
        Some(value) => value,
    };

    let mut max_cost_usd = match requested.max_cost_usd {
        Some(value) if value.is_finite() && value > 0.0 => {
            if value > limits.ceiling_max_cost_usd {
                clamps.push(Clamp {
                    field: "max_cost_usd",
                    requested: Some(value),
                    effective: limits.ceiling_max_cost_usd,
                    by: "ceiling",
                });
                limits.ceiling_max_cost_usd
            } else {
                value
            }
        }
        _ => {
            clamps.push(Clamp {
                field: "max_cost_usd",
                requested: requested.max_cost_usd,
                effective: limits.default_max_cost_usd,
                by: "default",
            });
            limits.default_max_cost_usd
        }
    };

    let mut grant_bound = GrantBound::default();
    if let Some(claim) = claim {
        if claim.max_steps > 0 && claim.max_steps < max_steps {
            let asked = (requested.max_steps > 0).then(|| f64::from(requested.max_steps));
            grant_clamp(&mut clamps, "max_steps", asked, f64::from(claim.max_steps));
            max_steps = claim.max_steps;
            grant_bound.steps = true;
        }
        if claim.max_runtime_s > 0 && claim.max_runtime_s < max_runtime_s {
            let asked = requested
                .max_runtime_s
                .filter(|value| *value > 0)
                .map(f64::from);
            grant_clamp(&mut clamps, "max_runtime_s", asked, f64::from(claim.max_runtime_s));
            max_runtime_s = claim.max_runtime_s;
            grant_bound.runtime = true;
        }
        if claim.micro_usd > 0 && claim.micro_usd < usd_to_micro(max_cost_usd) {
            let asked = requested
                .max_cost_usd
                .filter(|value| value.is_finite() && *value > 0.0);
            let effective = claim.micro_usd as f64 / MICRO;
            grant_clamp(&mut clamps, "max_cost_usd", asked, effective);
            max_cost_usd = effective;
            grant_bound.cost = true;
        }
    }

    Some((
        BudgetCaps {
            max_steps,
            max_attempts: max_steps.saturating_mul(limits.attempt_factor.max(1)),
            max_runtime_s,
            max_cost_usd,
            browser_second_micro_usd: usd_to_micro(limits.browser_second_usd),
            grant_bound,
        },
        clamps,
    ))
}

/// The tighter of two claims, dimension by dimension (0 is "no bound", so the
/// other side's bound stands). The result keeps the first claim's label and
/// stamp when there is one: a run's claim is what it started under.
pub fn tighter_claim(first: Option<&GrantBudget>, second: Option<&GrantBudget>) -> Option<GrantBudget> {
    fn min_bound<T: Ord + Copy + Default>(a: T, b: T) -> T {
        let zero = T::default();
        match (a == zero, b == zero) {
            (true, _) => b,
            (_, true) => a,
            _ => a.min(b),
        }
    }
    let merged = match (first, second) {
        (None, None) => return None,
        (Some(only), None) | (None, Some(only)) => only.clone(),
        (Some(a), Some(b)) => GrantBudget {
            micro_usd: min_bound(a.micro_usd, b.micro_usd),
            max_steps: min_bound(a.max_steps, b.max_steps),
            max_runtime_s: min_bound(a.max_runtime_s, b.max_runtime_s),
            source: a.source.clone(),
            minted_at_unix: a.minted_at_unix,
        },
    };
    merged.is_set().then_some(merged)
}

/// Which bound was reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dimension {
    Steps,
    Attempts,
    Runtime,
    Cost,
}

impl Dimension {
    pub fn label(self) -> &'static str {
        match self {
            Self::Steps => "steps",
            Self::Attempts => "attempts",
            Self::Runtime => "runtime",
            Self::Cost => "cost",
        }
    }

    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "steps" => Some(Self::Steps),
            "attempts" => Some(Self::Attempts),
            "runtime" => Some(Self::Runtime),
            "cost" => Some(Self::Cost),
            _ => None,
        }
    }

    fn unit(self) -> &'static str {
        match self {
            Self::Steps | Self::Attempts => "count",
            Self::Runtime => "seconds",
            Self::Cost => "usd",
        }
    }
}

/// What a run has used.
#[derive(Debug, Clone)]
pub struct BudgetLedger {
    pub class: RunClass,
    pub steps: u32,
    pub attempts: u32,
    pub spent_micro_usd: u64,
    /// Active time carried over from a previous segment of this run.
    base_active_ms: u64,
    started: Instant,
    paused_since: Option<Instant>,
    paused_ms: u64,
    exhausted: Option<Dimension>,
    /// Shadow mode records the first would-be refusal once, not on every call.
    shadow_reported: bool,
    /// The refusal of this segment has been announced (`agent.budget_exhausted`).
    /// Not persisted: a resumed run is a new segment and may announce again.
    refusal_announced: bool,
}

impl BudgetLedger {
    pub fn new(class: RunClass, now: Instant) -> Self {
        Self {
            class,
            steps: 0,
            attempts: 0,
            spent_micro_usd: 0,
            base_active_ms: 0,
            started: now,
            paused_since: None,
            paused_ms: 0,
            exhausted: None,
            shadow_reported: false,
            refusal_announced: false,
        }
    }

    /// Restore a ledger from a checkpoint: a resume cannot refill a budget.
    pub fn restore(checkpoint: &BudgetCheckpoint, class: RunClass, now: Instant) -> Self {
        let mut ledger = Self::new(class, now);
        ledger.steps = checkpoint.steps;
        ledger.attempts = checkpoint.attempts;
        ledger.spent_micro_usd = checkpoint.spent_micro_usd;
        ledger.base_active_ms = checkpoint.active_ms;
        ledger.exhausted = checkpoint
            .exhausted
            .as_deref()
            .and_then(Dimension::from_label);
        ledger
    }

    pub fn snapshot(&self, now: Instant) -> BudgetCheckpoint {
        BudgetCheckpoint {
            class: self.class.label().to_owned(),
            steps: self.steps,
            attempts: self.attempts,
            spent_micro_usd: self.spent_micro_usd,
            active_ms: self.active_ms(now),
            exhausted: self.exhausted.map(|dimension| dimension.label().to_owned()),
            claim: None,
        }
    }

    /// Time the run was under agent control: wall time minus human takeover.
    pub fn active_ms(&self, now: Instant) -> u64 {
        let wall = now.saturating_duration_since(self.started).as_millis() as u64;
        let paused_now = self
            .paused_since
            .map(|since| now.saturating_duration_since(since).as_millis() as u64)
            .unwrap_or(0);
        self.base_active_ms
            .saturating_add(wall.saturating_sub(self.paused_ms.saturating_add(paused_now)))
    }

    /// A human took over: the runtime clock stops.
    pub fn pause(&mut self, now: Instant) {
        if self.paused_since.is_none() {
            self.paused_since = Some(now);
        }
    }

    /// Control returned to the agent: the clock runs again.
    pub fn resume(&mut self, now: Instant) {
        if let Some(since) = self.paused_since.take() {
            self.paused_ms = self
                .paused_ms
                .saturating_add(now.saturating_duration_since(since).as_millis() as u64);
        }
    }

    pub fn exhausted(&self) -> Option<Dimension> {
        self.exhausted
    }

    /// Record one executed action. A failed attempt counts toward the attempt
    /// allowance and carries its cost, but is not a completed step.
    pub fn charge(&mut self, completed: bool, cost_micro_usd: u64) {
        self.attempts = self.attempts.saturating_add(1);
        if completed {
            self.steps = self.steps.saturating_add(1);
        }
        self.spent_micro_usd = self.spent_micro_usd.saturating_add(cost_micro_usd);
    }

    /// Whether the caller should record this would-be refusal (first time only).
    pub fn take_shadow_report(&mut self) -> bool {
        !std::mem::replace(&mut self.shadow_reported, true)
    }

    fn cost_micro(&self, caps: &BudgetCaps, now: Instant) -> u64 {
        self.cost_micro_at(caps.browser_second_micro_usd, now)
    }

    /// Spent so far, in micro-USD: the flat charges plus the browser-time term.
    pub fn cost_micro_at(&self, browser_second_micro_usd: u64, now: Instant) -> u64 {
        let browser_time = (self.active_ms(now) / 1000).saturating_mul(browser_second_micro_usd);
        self.spent_micro_usd.saturating_add(browser_time)
    }

    /// Whether this segment has not yet announced its refusal; the first call says
    /// yes and records it.
    pub fn take_refusal_announcement(&mut self) -> bool {
        !std::mem::replace(&mut self.refusal_announced, true)
    }
}

/// Why a run was refused.
#[derive(Debug, Clone, PartialEq)]
pub struct Exhausted {
    pub dimension: Dimension,
    pub limit: f64,
    pub spent: f64,
    pub unit: &'static str,
    /// `run_budget` (the request, default or ceiling set the bound) or
    /// `grant` (the broker grant's claim did).
    pub limit_source: &'static str,
}

impl Exhausted {
    /// The typed refusal. Not retryable: the budget does not refill by waiting.
    pub fn into_error(&self) -> QuarryError {
        QuarryError::new(
            ErrorCode::BudgetExhausted,
            format!(
                "agent run budget exhausted: {} (limit {}, used {} {})",
                self.dimension.label(),
                self.limit,
                self.spent,
                self.unit
            ),
        )
        .with_details(serde_json::json!({
            "dimension": self.dimension.label(),
            "limit": self.limit,
            "spent": self.spent,
            "unit": self.unit,
            "estimated": self.dimension == Dimension::Cost,
            "limit_source": self.limit_source,
            "resumable": false,
        }))
    }
}

/// Decide whether the next action may run. `projected_cost_micro_usd` is the
/// flat-table estimate of that action. Exhaustion latches: once a dimension is
/// reached the same refusal is returned until the run ends.
pub fn check(
    ledger: &mut BudgetLedger,
    caps: &BudgetCaps,
    projected_cost_micro_usd: u64,
    now: Instant,
) -> Result<(), Exhausted> {
    let active_s = ledger.active_ms(now) as f64 / 1000.0;
    let cost_now = ledger.cost_micro(caps, now);
    let dimension = ledger.exhausted.or_else(|| {
        if ledger.steps >= caps.max_steps {
            Some(Dimension::Steps)
        } else if ledger.attempts >= caps.max_attempts {
            Some(Dimension::Attempts)
        } else if active_s >= f64::from(caps.max_runtime_s) {
            Some(Dimension::Runtime)
        } else if cost_now.saturating_add(projected_cost_micro_usd)
            > usd_to_micro(caps.max_cost_usd)
        {
            Some(Dimension::Cost)
        } else {
            None
        }
    });
    let Some(dimension) = dimension else {
        return Ok(());
    };
    ledger.exhausted = Some(dimension);
    let (limit, spent) = match dimension {
        Dimension::Steps => (f64::from(caps.max_steps), f64::from(ledger.steps)),
        Dimension::Attempts => (f64::from(caps.max_attempts), f64::from(ledger.attempts)),
        Dimension::Runtime => (f64::from(caps.max_runtime_s), active_s.floor()),
        Dimension::Cost => (caps.max_cost_usd, cost_now as f64 / MICRO),
    };
    let grant_bound = match dimension {
        Dimension::Steps | Dimension::Attempts => caps.grant_bound.steps,
        Dimension::Runtime => caps.grant_bound.runtime,
        Dimension::Cost => caps.grant_bound.cost,
    };
    Err(Exhausted {
        dimension,
        limit,
        spent,
        unit: dimension.unit(),
        limit_source: if grant_bound { "grant" } else { "run_budget" },
    })
}

impl Exhausted {
    /// The payload of `agent.budget_exhausted` (WS6.B): the same figures as the
    /// refusal, without `resumable`.
    pub fn event_payload(&self) -> serde_json::Value {
        serde_json::json!({
            "dimension": self.dimension.label(),
            "limit": self.limit,
            "spent": self.spent,
            "unit": self.unit,
            "estimated": self.dimension == Dimension::Cost,
            "limit_source": self.limit_source,
        })
    }
}

/// The effective bounds returned to the caller of `POST /v1/agent/runs`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct EffectiveConstraints {
    pub class: &'static str,
    /// `off`, `shadow` or `on`: whether these bounds are enforced.
    pub enforcement: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_steps: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_runtime_s: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_cost_usd: Option<f64>,
    /// Cost is computed from a flat action table, not billed usage.
    pub estimated: bool,
}

/// A requested value that was replaced, as the caller sees it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ClampView {
    pub field: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested: Option<f64>,
    pub effective: f64,
    pub by: &'static str,
}

/// The budget of one live run: its class, bounds and ledger.
#[derive(Debug, Clone)]
pub struct RunBudget {
    pub class: RunClass,
    /// `None` for an interactive run: it is never capped.
    pub caps: Option<BudgetCaps>,
    pub clamps: Vec<Clamp>,
    pub ledger: BudgetLedger,
    /// The grant claim this run's bounds were tightened by, if any: stored
    /// with the ledger so a resume cannot drop it. Always `None` for an
    /// interactive run.
    pub claim: Option<GrantBudget>,
    /// The refusal the last `gate` made in `On` mode, until it is announced (WS6.B).
    refusal: Option<Exhausted>,
}

impl RunBudget {
    /// Open a budget for a new run, or for a resume (`resumed` is the stored
    /// ledger: spent steps, attempts, cost and time carry over, and the class
    /// is the stricter of the stored and the derived one).
    pub fn start(
        limits: &BudgetLimits,
        class: RunClass,
        requested: &AgentConstraints,
        resumed: Option<&BudgetCheckpoint>,
        now: Instant,
    ) -> Self {
        Self::start_with_claim(limits, class, requested, None, resumed, now)
    }

    /// [`start`](Self::start) under a grant's budget claim. `claim` is what
    /// this start applies (the caller passes `None` unless claims are enforced);
    /// a resumed run also keeps the claim it started under, and the tighter of
    /// the two wins, so neither a resume nor a different grant presented with
    /// it can widen the run.
    pub fn start_with_claim(
        limits: &BudgetLimits,
        class: RunClass,
        requested: &AgentConstraints,
        claim: Option<&GrantBudget>,
        resumed: Option<&BudgetCheckpoint>,
        now: Instant,
    ) -> Self {
        let class = resumed
            .and_then(|checkpoint| RunClass::from_label(&checkpoint.class))
            .map_or(class, |stored| stored.stricter(class));
        let claim = if class == RunClass::Interactive {
            None
        } else {
            tighter_claim(resumed.and_then(|checkpoint| checkpoint.claim.as_ref()), claim)
        };
        let (caps, clamps) = match resolve_caps_with_claim(limits, class, requested, claim.as_ref()) {
            Some((caps, clamps)) => (Some(caps), clamps),
            None => (None, Vec::new()),
        };
        let ledger = match resumed {
            Some(checkpoint) => BudgetLedger::restore(checkpoint, class, now),
            None => BudgetLedger::new(class, now),
        };
        Self {
            class,
            caps,
            clamps,
            ledger,
            claim,
            refusal: None,
        }
    }

    /// The ledger and the claim, as stored with the run's checkpoint.
    pub fn snapshot(&self, now: Instant) -> BudgetCheckpoint {
        let mut checkpoint = self.ledger.snapshot(now);
        checkpoint.claim = self.claim.clone();
        checkpoint
    }

    /// The gate before an action. `projected_cost_micro_usd` is the flat-table
    /// estimate of that action. In `On` mode a reached bound is a typed 402; in
    /// `Shadow` mode it is recorded once and the action proceeds.
    pub fn gate(
        &mut self,
        mode: BudgetMode,
        projected_cost_micro_usd: u64,
        now: Instant,
    ) -> Result<(), QuarryError> {
        let Some(caps) = self.caps.as_ref() else {
            return Ok(());
        };
        if mode == BudgetMode::Off {
            return Ok(());
        }
        let Err(exhausted) = check(&mut self.ledger, caps, projected_cost_micro_usd, now) else {
            return Ok(());
        };
        if mode == BudgetMode::On {
            crate::metrics::record_budget_exhausted(
                exhausted.dimension.label(),
                self.class.label(),
                "on",
            );
            let error = exhausted.into_error();
            self.refusal = Some(exhausted);
            return Err(error);
        }
        if self.ledger.take_shadow_report() {
            crate::metrics::record_budget_exhausted(
                exhausted.dimension.label(),
                self.class.label(),
                "shadow",
            );
            tracing::warn!(
                class = self.class.label(),
                dimension = exhausted.dimension.label(),
                limit = exhausted.limit,
                spent = exhausted.spent,
                "agent run budget would be exhausted (shadow: not enforced)"
            );
        }
        Ok(())
    }

    /// The `agent.budget_exhausted` payload for the refusal the last `gate` made, the
    /// first time this segment refuses; `None` otherwise (WS6.B).
    pub fn take_refusal_announcement(&mut self) -> Option<serde_json::Value> {
        let exhausted = self.refusal.take()?;
        self.ledger
            .take_refusal_announcement()
            .then(|| exhausted.event_payload())
    }

    /// `meta.budget` for a step response (WS6.B). `cost_source` labels the cost of the
    /// step the response answers.
    pub fn meta(&self, cost_source: &str, now: Instant) -> quarry_core::contracts::BudgetMeta {
        let browser_second = self.caps.as_ref().map_or(0, |caps| caps.browser_second_micro_usd);
        quarry_core::contracts::BudgetMeta {
            class: self.class.label().to_owned(),
            steps: self.ledger.steps,
            attempts: self.ledger.attempts,
            max_steps: self.caps.as_ref().map(|caps| caps.max_steps),
            max_attempts: self.caps.as_ref().map(|caps| caps.max_attempts),
            elapsed_s: self.ledger.active_ms(now) as f64 / 1000.0,
            max_runtime_s: self.caps.as_ref().map(|caps| caps.max_runtime_s),
            spent_usd: self.ledger.cost_micro_at(browser_second, now) as f64 / MICRO,
            max_cost_usd: self.caps.as_ref().map(|caps| caps.max_cost_usd),
            estimated: true,
            cost_source: cost_source.to_owned(),
            exhausted: self.ledger.exhausted().map(|dimension| dimension.label().to_owned()),
        }
    }

    /// The `usage` block of `DELETE /v1/agent/runs/{run_id}` (WS6.B).
    pub fn usage(&self, now: Instant) -> serde_json::Value {
        let browser_second = self.caps.as_ref().map_or(0, |caps| caps.browser_second_micro_usd);
        serde_json::json!({
            "class": self.class.label(),
            "spent_usd": self.ledger.cost_micro_at(browser_second, now) as f64 / MICRO,
            "steps": self.ledger.steps,
            "attempts": self.ledger.attempts,
            "active_s": self.ledger.active_ms(now) as f64 / 1000.0,
            "exhausted": self.ledger.exhausted().map(|dimension| dimension.label()),
        })
    }

    /// Record an executed action: `completed` is false for a failed attempt.
    pub fn charge(&mut self, completed: bool, cost_micro_usd: u64) {
        self.ledger.charge(completed, cost_micro_usd);
    }

    pub fn effective_constraints(&self, mode: BudgetMode) -> EffectiveConstraints {
        EffectiveConstraints {
            class: self.class.label(),
            enforcement: mode.label(),
            max_steps: self.caps.as_ref().map(|caps| caps.max_steps),
            max_runtime_s: self.caps.as_ref().map(|caps| caps.max_runtime_s),
            max_cost_usd: self.caps.as_ref().map(|caps| caps.max_cost_usd),
            estimated: true,
        }
    }

    pub fn clamped(&self) -> Vec<ClampView> {
        self.clamps
            .iter()
            .map(|clamp| ClampView {
                field: clamp.field,
                requested: clamp.requested,
                effective: clamp.effective,
                by: clamp.by,
            })
            .collect()
    }
}

/// Whether the request's own step cap (`constraints.max_steps`) still refuses
/// with the legacy 429. It stops where the gate owns the bound: a person's
/// session is never capped, and an agent run is refused with the typed 402 once
/// enforcement is `on`. Otherwise it stays as it was: with enforcement `off`
/// there is no gate, and in `shadow` an agent run keeps its request cap until
/// the gate is enforced.
pub fn legacy_step_cap_applies(budget: Option<&RunBudget>, mode: BudgetMode) -> bool {
    match budget {
        None => true,
        Some(budget) => budget.class == RunClass::Agent && mode != BudgetMode::On,
    }
}

/// USD to the integer micro-USD the ledger counts in.
pub fn micro_usd(usd: f64) -> u64 {
    usd_to_micro(usd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn constraints(max_steps: u32, runtime: Option<u32>, cost: Option<f64>) -> AgentConstraints {
        AgentConstraints {
            max_steps,
            allowed_domains: Vec::new(),
            max_runtime_s: runtime,
            max_cost_usd: cost,
        }
    }

    fn caps(max_steps: u32, max_attempts: u32, runtime_s: u32, cost_usd: f64) -> BudgetCaps {
        BudgetCaps {
            max_steps,
            max_attempts,
            max_runtime_s: runtime_s,
            max_cost_usd: cost_usd,
            browser_second_micro_usd: 0,
            grant_bound: GrantBound::default(),
        }
    }

    #[test]
    fn class_is_derived_from_the_caller_and_can_only_tighten() {
        assert_eq!(classify(false, None), RunClass::Interactive);
        assert_eq!(classify(false, Some("user")), RunClass::Interactive);
        assert_eq!(classify(true, None), RunClass::Agent);
        assert_eq!(classify(false, Some("agent")), RunClass::Agent);
        assert_eq!(classify(false, Some(" AGENT ")), RunClass::Agent);
        // A service principal cannot present itself as interactive.
        assert_eq!(classify(true, Some("user")), RunClass::Agent);
        assert_eq!(
            RunClass::Interactive.stricter(RunClass::Agent),
            RunClass::Agent
        );
        assert_eq!(
            RunClass::Agent.stricter(RunClass::Interactive),
            RunClass::Agent
        );
        assert_eq!(
            RunClass::Interactive.stricter(RunClass::Interactive),
            RunClass::Interactive
        );
    }

    #[test]
    fn interactive_class_is_never_budgeted() {
        let limits = BudgetLimits::default();
        assert!(
            resolve_caps(&limits, RunClass::Interactive, &constraints(0, None, None)).is_none()
        );
    }

    #[test]
    fn manual_session_12_steps_120s_constraints_do_not_cap() {
        // What the UI sends for a person browsing: a small step and time hint.
        let limits = BudgetLimits::default();
        assert!(resolve_caps(
            &limits,
            RunClass::Interactive,
            &constraints(12, Some(120), None)
        )
        .is_none());
    }

    #[test]
    fn zero_or_none_request_takes_the_default_not_unlimited() {
        let limits = BudgetLimits::default();
        let (caps, clamps) =
            resolve_caps(&limits, RunClass::Agent, &constraints(0, None, None)).unwrap();
        assert_eq!(caps.max_steps, 60);
        assert_eq!(caps.max_runtime_s, 900);
        assert_eq!(caps.max_cost_usd, 1.0);
        assert_eq!(caps.max_attempts, 120);
        assert_eq!(clamps.len(), 3);
        assert!(clamps.iter().all(|clamp| clamp.by == "default"));
        // Zero and non-finite requests are "absent" too.
        let (caps, _) = resolve_caps(
            &limits,
            RunClass::Agent,
            &constraints(0, Some(0), Some(f64::NAN)),
        )
        .unwrap();
        assert_eq!((caps.max_runtime_s, caps.max_cost_usd), (900, 1.0));
        let (caps, _) =
            resolve_caps(&limits, RunClass::Agent, &constraints(0, None, Some(-3.0))).unwrap();
        assert_eq!(caps.max_cost_usd, 1.0);
    }

    #[test]
    fn requests_above_the_ceiling_are_clamped_and_attributed() {
        let limits = BudgetLimits::default();
        let (caps, clamps) = resolve_caps(
            &limits,
            RunClass::Agent,
            &constraints(10_000, Some(999_999), Some(50.0)),
        )
        .unwrap();
        assert_eq!(
            (caps.max_steps, caps.max_runtime_s, caps.max_cost_usd),
            (300, 3600, 20.0)
        );
        assert!(clamps.iter().all(|clamp| clamp.by == "ceiling"));
        let cost = clamps
            .iter()
            .find(|clamp| clamp.field == "max_cost_usd")
            .unwrap();
        assert_eq!((cost.requested, cost.effective), (Some(50.0), 20.0));
    }

    #[test]
    fn a_request_inside_the_bounds_is_taken_as_asked() {
        let limits = BudgetLimits::default();
        let (caps, clamps) = resolve_caps(
            &limits,
            RunClass::Agent,
            &constraints(5, Some(30), Some(0.25)),
        )
        .unwrap();
        assert_eq!(
            (caps.max_steps, caps.max_runtime_s, caps.max_cost_usd),
            (5, 30, 0.25)
        );
        assert!(clamps.is_empty());
    }

    #[test]
    fn agent_cap_counts_completed_steps() {
        let now = Instant::now();
        let caps = caps(3, 100, 3600, 100.0);
        let mut ledger = BudgetLedger::new(RunClass::Agent, now);
        for _ in 0..3 {
            check(&mut ledger, &caps, 0, now).unwrap();
            ledger.charge(true, 0);
        }
        let refused = check(&mut ledger, &caps, 0, now).unwrap_err();
        assert_eq!(refused.dimension, Dimension::Steps);
        assert_eq!((refused.limit, refused.spent), (3.0, 3.0));
    }

    #[test]
    fn failed_attempts_are_bounded_by_the_attempt_allowance() {
        let now = Instant::now();
        let caps = caps(10, 4, 3600, 100.0);
        let mut ledger = BudgetLedger::new(RunClass::Agent, now);
        for _ in 0..4 {
            check(&mut ledger, &caps, 0, now).unwrap();
            ledger.charge(false, 0);
        }
        // Zero completed steps, so the step cap never fires; attempts do.
        assert_eq!(ledger.steps, 0);
        let refused = check(&mut ledger, &caps, 0, now).unwrap_err();
        assert_eq!(refused.dimension, Dimension::Attempts);
    }

    #[test]
    fn runtime_clock_excludes_takeover_pause() {
        let start = Instant::now();
        let caps = caps(1000, 2000, 60, 100.0);
        let mut ledger = BudgetLedger::new(RunClass::Agent, start);
        ledger.pause(start + Duration::from_secs(10));
        // A human drives for 10 minutes: no agent time passes.
        ledger.resume(start + Duration::from_secs(610));
        assert_eq!(ledger.active_ms(start + Duration::from_secs(610)), 10_000);
        assert!(check(&mut ledger, &caps, 0, start + Duration::from_secs(610)).is_ok());
        // Agent time resumes and runs out at 60 s of active time.
        let refused = check(&mut ledger, &caps, 0, start + Duration::from_secs(660)).unwrap_err();
        assert_eq!(refused.dimension, Dimension::Runtime);
        assert_eq!(refused.limit, 60.0);
    }

    #[test]
    fn pause_is_idempotent_and_an_open_pause_does_not_count() {
        let start = Instant::now();
        let mut ledger = BudgetLedger::new(RunClass::Agent, start);
        ledger.pause(start + Duration::from_secs(5));
        ledger.pause(start + Duration::from_secs(8));
        assert_eq!(ledger.active_ms(start + Duration::from_secs(20)), 5_000);
        ledger.resume(start + Duration::from_secs(20));
        ledger.resume(start + Duration::from_secs(30));
        assert_eq!(ledger.active_ms(start + Duration::from_secs(30)), 15_000);
    }

    #[test]
    fn cost_projection_refuses_before_overshoot() {
        let now = Instant::now();
        let caps = caps(100, 200, 3600, 0.0003);
        let mut ledger = BudgetLedger::new(RunClass::Agent, now);
        // The cap is 300 micro-USD: two 100-micro actions fit, and so does a third
        // (200 + 100 = 300 is not over), but nothing more once 300 is spent.
        ledger.charge(true, 100);
        ledger.charge(true, 100);
        assert!(check(&mut ledger, &caps, 100, now).is_ok());
        ledger.charge(true, 100);
        let refused = check(&mut ledger, &caps, 1, now).unwrap_err();
        assert_eq!(refused.dimension, Dimension::Cost);
        assert_eq!(refused.unit, "usd");
        assert!((refused.spent - 0.0003).abs() < 1e-12);
        assert!(refused.into_error().details.unwrap()["estimated"]
            .as_bool()
            .unwrap());
    }

    #[test]
    fn cost_includes_browser_time_term() {
        let start = Instant::now();
        let mut priced = caps(100, 200, 3600, 0.001);
        priced.browser_second_micro_usd = 100; // 0.0001 USD per second
        let mut ledger = BudgetLedger::new(RunClass::Agent, start);
        assert!(check(&mut ledger, &priced, 0, start + Duration::from_secs(5)).is_ok());
        // 10 s of browser time = 0.001 USD = the whole cost cap.
        let refused = check(&mut ledger, &priced, 1, start + Duration::from_secs(10)).unwrap_err();
        assert_eq!(refused.dimension, Dimension::Cost);
    }

    #[test]
    fn exhaustion_latches_and_repeats_the_same_error() {
        let now = Instant::now();
        let caps = caps(1, 10, 3600, 100.0);
        let mut ledger = BudgetLedger::new(RunClass::Agent, now);
        ledger.charge(true, 0);
        let first = check(&mut ledger, &caps, 0, now).unwrap_err();
        let second = check(&mut ledger, &caps, 0, now).unwrap_err();
        assert_eq!(first, second);
        // Even a roomier cap cannot un-exhaust a latched run.
        let roomy = self::caps(1000, 1000, 9999, 999.0);
        assert_eq!(
            check(&mut ledger, &roomy, 0, now).unwrap_err().dimension,
            Dimension::Steps
        );
        assert_eq!(ledger.exhausted(), Some(Dimension::Steps));
    }

    #[test]
    fn resume_restores_spent_attempts_and_active_time() {
        let start = Instant::now();
        let caps = caps(5, 10, 100, 1.0);
        let mut ledger = BudgetLedger::new(RunClass::Agent, start);
        for _ in 0..5 {
            ledger.charge(true, 1000);
        }
        let _ = check(&mut ledger, &caps, 0, start + Duration::from_secs(30)).unwrap_err();
        let saved = ledger.snapshot(start + Duration::from_secs(30));
        assert_eq!(saved.active_ms, 30_000);
        assert_eq!(saved.exhausted.as_deref(), Some("steps"));

        // A new process, a new clock, the same spent budget.
        let later = Instant::now();
        let mut resumed = BudgetLedger::restore(&saved, RunClass::Agent, later);
        assert_eq!(
            (resumed.steps, resumed.attempts, resumed.spent_micro_usd),
            (5, 5, 5000)
        );
        assert_eq!(resumed.active_ms(later), 30_000);
        assert_eq!(
            check(&mut resumed, &caps, 0, later).unwrap_err().dimension,
            Dimension::Steps
        );
    }

    #[test]
    fn exhaustion_is_announced_once_per_segment() {
        let limits = BudgetLimits::default();
        let now = Instant::now();
        let mut run = RunBudget::start(&limits, RunClass::Agent, &constraints(2, Some(600), Some(5.0)), None, now);
        // Two completed steps: the third gate refuses.
        for _ in 0..2 {
            run.gate(BudgetMode::On, 0, now).unwrap();
            run.charge(true, 0);
        }
        assert!(run.gate(BudgetMode::On, 0, now).is_err());
        let payload = run.take_refusal_announcement().expect("first refusal is announced");
        assert_eq!(payload["dimension"], "steps");
        assert_eq!(payload["limit"], 2.0);
        assert_eq!(payload["estimated"], false);
        // The same segment refuses again, but says nothing new.
        assert!(run.gate(BudgetMode::On, 0, now).is_err());
        assert!(run.take_refusal_announcement().is_none());
        // A shadow gate never announces a refusal it did not make.
        let mut shadow = RunBudget::start(&limits, RunClass::Agent, &constraints(1, Some(600), Some(5.0)), None, now);
        shadow.charge(true, 0);
        assert!(shadow.gate(BudgetMode::Shadow, 0, now).is_ok());
        assert!(shadow.take_refusal_announcement().is_none());
        // A resume is a new segment: the restored latch refuses, and announces once more.
        let saved = run.snapshot(now);
        let mut resumed = RunBudget::start(&limits, RunClass::Agent, &constraints(2, Some(600), Some(5.0)), Some(&saved), now);
        assert!(resumed.gate(BudgetMode::On, 0, now).is_err());
        assert!(resumed.take_refusal_announcement().is_some());
    }

    #[test]
    fn meta_and_usage_report_the_ledger_and_its_source() {
        let limits = BudgetLimits::default();
        let now = Instant::now();
        let mut run = RunBudget::start(&limits, RunClass::Agent, &constraints(5, Some(60), Some(0.5)), None, now);
        run.charge(true, 1_500);
        run.charge(false, 100);
        let meta = run.meta("flat_table", now);
        assert_eq!((meta.class.as_str(), meta.steps, meta.attempts), ("agent", 1, 2));
        assert_eq!((meta.max_steps, meta.max_attempts), (Some(5), Some(10)));
        assert_eq!(meta.max_cost_usd, Some(0.5));
        assert!((meta.spent_usd - 0.0016).abs() < 1e-9, "spent is the charged micro-USD");
        assert!(meta.estimated);
        assert_eq!(meta.cost_source, "flat_table");
        assert!(meta.exhausted.is_none());
        let usage = run.usage(now);
        assert_eq!(usage["steps"], 1);
        assert_eq!(usage["attempts"], 2);
        assert!(usage["exhausted"].is_null());
        // An interactive run has no bounds to report.
        let person = RunBudget::start(&limits, RunClass::Interactive, &constraints(12, Some(120), None), None, now);
        let meta = person.meta("flat_table", now);
        assert_eq!(meta.class, "interactive");
        assert!(meta.max_steps.is_none() && meta.max_cost_usd.is_none());
    }

    #[test]
    fn resolve_caps_takes_min_of_request_grant_ceiling() {
        // The request asks for 50 steps, the grant claims 7, the ceiling is 300: the
        // smallest bound wins and is reported as a grant clamp. (The plan's per-org
        // allowance term is not in the resolver yet; see the W2-U21 depth notes.)
        let limits = BudgetLimits::default();
        let (caps, clamps) = resolve_caps_with_claim(
            &limits,
            RunClass::Agent,
            &constraints(50, None, None),
            Some(&claim(0, 7, 0)),
        )
        .unwrap();
        assert_eq!(caps.max_steps, 7);
        assert!(caps.grant_bound.steps);
        let clamp = clamps.iter().find(|clamp| clamp.field == "max_steps").unwrap();
        assert_eq!((clamp.requested, clamp.effective, clamp.by), (Some(50.0), 7.0, "grant"));
        // A claim above what the request gave changes nothing.
        let (caps, _) = resolve_caps_with_claim(
            &limits,
            RunClass::Agent,
            &constraints(5, None, None),
            Some(&claim(0, 7, 0)),
        )
        .unwrap();
        assert_eq!(caps.max_steps, 5);
        assert!(!caps.grant_bound.steps);
    }

    #[test]
    fn legacy_step_cap_applies_only_where_the_gate_does_not_own_the_bound() {
        let limits = BudgetLimits::default();
        let now = Instant::now();
        let agent = RunBudget::start(&limits, RunClass::Agent, &constraints(5, None, None), None, now);
        let person = RunBudget::start(&limits, RunClass::Interactive, &constraints(12, None, None), None, now);
        // Off: no gate at all, so the request cap is what it always was, for everyone.
        assert!(legacy_step_cap_applies(None, BudgetMode::Off));
        // Shadow: an agent keeps its request cap; a person is never capped.
        assert!(legacy_step_cap_applies(Some(&agent), BudgetMode::Shadow));
        assert!(!legacy_step_cap_applies(Some(&person), BudgetMode::Shadow));
        // On: the typed gate owns the agent bound; the person is still uncapped.
        assert!(!legacy_step_cap_applies(Some(&agent), BudgetMode::On));
        assert!(!legacy_step_cap_applies(Some(&person), BudgetMode::On));
    }

    #[test]
    fn typed_error_is_402_not_retryable_and_names_the_dimension() {
        let now = Instant::now();
        let caps = caps(1, 10, 3600, 100.0);
        let mut ledger = BudgetLedger::new(RunClass::Agent, now);
        ledger.charge(true, 0);
        let error = check(&mut ledger, &caps, 0, now).unwrap_err().into_error();
        assert_eq!(error.code, ErrorCode::BudgetExhausted);
        assert_eq!(error.code.http_status(), 402);
        assert!(!error.code.retryable());
        let details = error.details.unwrap();
        assert_eq!(details["dimension"], "steps");
        assert_eq!(details["resumable"], false);
        assert_eq!(details["limit"], 1.0);
    }

    #[test]
    fn shadow_reports_the_first_would_be_refusal_once() {
        let mut ledger = BudgetLedger::new(RunClass::Agent, Instant::now());
        assert!(ledger.take_shadow_report());
        assert!(!ledger.take_shadow_report());
    }

    #[test]
    fn modes_parse_and_reject_garbage() {
        assert_eq!(BudgetMode::parse("off"), Ok(BudgetMode::Off));
        assert_eq!(BudgetMode::parse("SHADOW"), Ok(BudgetMode::Shadow));
        assert_eq!(BudgetMode::parse(""), Ok(BudgetMode::Shadow));
        assert_eq!(BudgetMode::parse(" on "), Ok(BudgetMode::On));
        assert!(BudgetMode::parse("enforce").unwrap_err().contains(ENV_MODE));
    }

    #[test]
    fn limits_come_from_env_and_reject_nonsense() {
        let none = |_: &str| None;
        assert_eq!(
            BudgetLimits::from_env(none).unwrap(),
            BudgetLimits::default()
        );
        let env = |name: &str| match name {
            "QUARRY_EDGE__AGENT_DEFAULT_MAX_STEPS" => Some("5".to_owned()),
            "QUARRY_EDGE__AGENT_CEILING_MAX_COST_USD" => Some("2.5".to_owned()),
            _ => None,
        };
        let limits = BudgetLimits::from_env(env).unwrap();
        assert_eq!(
            (limits.default_max_steps, limits.ceiling_max_cost_usd),
            (5, 2.5)
        );
        for (name, bad) in [
            ("QUARRY_EDGE__AGENT_DEFAULT_MAX_STEPS", "0"),
            ("QUARRY_EDGE__AGENT_DEFAULT_MAX_STEPS", "lots"),
            ("QUARRY_EDGE__AGENT_DEFAULT_MAX_COST_USD", "-1"),
            ("QUARRY_EDGE__BUDGET_BROWSER_SECOND_USD", "NaN"),
            ("QUARRY_EDGE__AGENT_DEFAULT_MAX_STEPS", "9999"), // above the 300 ceiling
        ] {
            let env = move |n: &str| (n == name).then(|| bad.to_owned());
            let error = BudgetLimits::from_env(env).unwrap_err();
            assert!(error.contains("QUARRY_EDGE__"), "{name}={bad}: {error}");
        }
    }

    #[test]
    fn shadow_mode_records_would_exhaust_but_lets_the_action_through() {
        let now = Instant::now();
        let limits = BudgetLimits::default();
        let mut budget = RunBudget::start(
            &limits,
            RunClass::Agent,
            &constraints(1, None, None),
            None,
            now,
        );
        budget.charge(true, 0);
        for _ in 0..3 {
            assert!(budget.gate(BudgetMode::Shadow, 0, now).is_ok());
        }
        // The latch is set, so an operator who flips to `on` sees the same run refused.
        assert_eq!(budget.ledger.exhausted(), Some(Dimension::Steps));
        let refused = budget.gate(BudgetMode::On, 0, now).unwrap_err();
        assert_eq!(refused.code, ErrorCode::BudgetExhausted);
    }

    #[test]
    fn off_mode_and_interactive_runs_never_refuse() {
        let now = Instant::now();
        let limits = BudgetLimits::default();
        let mut agent = RunBudget::start(
            &limits,
            RunClass::Agent,
            &constraints(1, None, None),
            None,
            now,
        );
        agent.charge(true, 0);
        assert!(agent.gate(BudgetMode::Off, 0, now).is_ok());

        let mut human = RunBudget::start(
            &limits,
            RunClass::Interactive,
            &constraints(12, Some(120), None),
            None,
            now,
        );
        for _ in 0..500 {
            human.charge(true, 1_000_000);
            assert!(human.gate(BudgetMode::On, 1_000_000, now).is_ok());
        }
        assert!(human.caps.is_none());
        assert!(human.clamped().is_empty());
        assert_eq!(human.effective_constraints(BudgetMode::On).max_steps, None);
    }

    #[test]
    fn a_resumed_run_keeps_the_stricter_class_and_its_spent_budget() {
        let start = Instant::now();
        let limits = BudgetLimits::default();
        let mut first = RunBudget::start(
            &limits,
            RunClass::Agent,
            &constraints(2, None, None),
            None,
            start,
        );
        first.charge(true, 500);
        first.charge(true, 500);
        let saved = first.ledger.snapshot(start);

        // The same run resumed by a human principal is still an agent run.
        let mut resumed = RunBudget::start(
            &limits,
            RunClass::Interactive,
            &constraints(2, None, None),
            Some(&saved),
            Instant::now(),
        );
        assert_eq!(resumed.class, RunClass::Agent);
        assert_eq!(resumed.ledger.steps, 2);
        assert_eq!(
            resumed
                .gate(BudgetMode::On, 0, Instant::now())
                .unwrap_err()
                .code,
            ErrorCode::BudgetExhausted
        );

        // And a stored interactive run resumed by a service becomes an agent run.
        let human = RunBudget::start(
            &limits,
            RunClass::Interactive,
            &constraints(0, None, None),
            None,
            start,
        );
        let human_saved = human.ledger.snapshot(start);
        let by_service = RunBudget::start(
            &limits,
            RunClass::Agent,
            &constraints(0, None, None),
            Some(&human_saved),
            start,
        );
        assert_eq!(by_service.class, RunClass::Agent);
    }

    #[test]
    fn effective_constraints_and_clamps_are_reported() {
        let now = Instant::now();
        let limits = BudgetLimits::default();
        let budget = RunBudget::start(
            &limits,
            RunClass::Agent,
            &constraints(0, None, Some(50.0)),
            None,
            now,
        );
        let view = budget.effective_constraints(BudgetMode::Shadow);
        assert_eq!(
            (
                view.class,
                view.enforcement,
                view.max_steps,
                view.max_cost_usd,
                view.estimated
            ),
            ("agent", "shadow", Some(60), Some(20.0), true)
        );
        let clamps = budget.clamped();
        assert!(clamps
            .iter()
            .any(|c| c.field == "max_cost_usd" && c.by == "ceiling" && c.requested == Some(50.0)));
        assert!(clamps
            .iter()
            .any(|c| c.field == "max_steps" && c.by == "default"));
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!(json["class"], "agent");
    }

    #[test]
    fn micro_usd_rounds_and_rejects_nonsense() {
        assert_eq!(micro_usd(0.0003), 300);
        assert_eq!(micro_usd(1.0), 1_000_000);
        assert_eq!(micro_usd(-1.0), 0);
        assert_eq!(micro_usd(f64::NAN), 0);
        assert_eq!(micro_usd(f64::INFINITY), 0);
    }

    // ── WS6.E: the grant-carried budget claim ──────────────────────────────

    fn claim(micro_usd: u64, max_steps: u32, max_runtime_s: u32) -> GrantBudget {
        GrantBudget {
            micro_usd,
            max_steps,
            max_runtime_s,
            source: "request".to_owned(),
            minted_at_unix: 1_700_000_000,
        }
    }

    fn resolved(
        requested: &AgentConstraints,
        claim: Option<&GrantBudget>,
    ) -> (BudgetCaps, Vec<Clamp>) {
        resolve_caps_with_claim(&BudgetLimits::default(), RunClass::Agent, requested, claim)
            .expect("an agent run is budgeted")
    }

    #[test]
    fn grant_claim_tightens_but_never_loosens_the_request() {
        // The request asks for 50 steps, 600 s and $1; the claim bounds all three lower.
        let ask = constraints(50, Some(600), Some(1.0));
        let (caps, clamps) = resolved(&ask, Some(&claim(250_000, 10, 120)));
        assert_eq!((caps.max_steps, caps.max_runtime_s), (10, 120));
        assert!((caps.max_cost_usd - 0.25).abs() < 1e-9);
        assert_eq!(
            caps.grant_bound,
            GrantBound {
                steps: true,
                runtime: true,
                cost: true
            }
        );
        // Attempts follow the tightened step cap, so a failing driver is bounded by it too.
        assert_eq!(caps.max_attempts, 20);
        let by_grant: Vec<_> = clamps.iter().filter(|c| c.by == "grant").collect();
        assert_eq!(by_grant.len(), 3, "{clamps:?}");
        let steps = by_grant.iter().find(|c| c.field == "max_steps").unwrap();
        assert_eq!((steps.requested, steps.effective), (Some(50.0), 10.0));

        // A claim above what the request gives changes nothing: the request wins.
        let (caps, clamps) = resolved(&constraints(5, Some(60), Some(0.1)), Some(&claim(9_000_000, 500, 9_000)));
        assert_eq!((caps.max_steps, caps.max_runtime_s), (5, 60));
        assert!((caps.max_cost_usd - 0.1).abs() < 1e-9);
        assert_eq!(caps.grant_bound, GrantBound::default());
        assert!(clamps.is_empty(), "{clamps:?}");

        // A claim equal to the request is not a tightening either.
        let (caps, clamps) = resolved(&constraints(10, Some(120), None), Some(&claim(0, 10, 120)));
        assert_eq!((caps.max_steps, caps.max_runtime_s), (10, 120));
        assert_eq!(caps.grant_bound, GrantBound::default());
        assert!(clamps.iter().all(|c| c.by != "grant"), "{clamps:?}");
    }

    #[test]
    fn a_claim_below_the_default_binds_a_request_that_named_nothing() {
        // No steps, no runtime, no cost asked: the defaults (60 / 900 s / $1.00) apply,
        // and a lower claim replaces them. `requested` stays absent in the report.
        let (caps, clamps) = resolved(&constraints(0, None, None), Some(&claim(0, 8, 0)));
        assert_eq!(caps.max_steps, 8);
        assert_eq!(caps.max_runtime_s, 900, "a dimension the claim leaves open keeps its default");
        let steps: Vec<_> = clamps.iter().filter(|c| c.field == "max_steps").collect();
        assert_eq!(steps.len(), 1, "one entry per field: {clamps:?}");
        assert_eq!((steps[0].by, steps[0].requested, steps[0].effective), ("grant", None, 8.0));
        assert!(clamps.iter().any(|c| c.field == "max_runtime_s" && c.by == "default"));
    }

    #[test]
    fn a_claim_above_the_ceiling_cannot_lift_the_ceiling() {
        let (caps, clamps) = resolved(
            &constraints(1_000, Some(100_000), Some(500.0)),
            Some(&claim(900_000_000, 5_000, 90_000)),
        );
        assert_eq!((caps.max_steps, caps.max_runtime_s), (300, 3600));
        assert!((caps.max_cost_usd - 20.0).abs() < 1e-9);
        assert!(clamps.iter().all(|c| c.by == "ceiling"), "{clamps:?}");
    }

    #[test]
    fn absent_claim_changes_nothing() {
        let ask = constraints(25, Some(300), Some(0.5));
        let plain = resolve_caps(&BudgetLimits::default(), RunClass::Agent, &ask).unwrap();
        for none_like in [None, Some(&claim(0, 0, 0))] {
            let with = resolved(&ask, none_like);
            assert_eq!(with.0, plain.0);
            assert_eq!(with.1, plain.1);
        }
    }

    #[test]
    fn an_interactive_run_ignores_a_claim() {
        let started = RunBudget::start_with_claim(
            &BudgetLimits::default(),
            RunClass::Interactive,
            &constraints(12, Some(120), None),
            Some(&claim(1, 1, 1)),
            None,
            Instant::now(),
        );
        assert!(started.caps.is_none());
        assert!(started.claim.is_none(), "a person's session carries no claim");
    }

    #[test]
    fn the_refusal_names_the_grant_as_its_limit_source() {
        let start = Instant::now();
        let mut budget = RunBudget::start_with_claim(
            &BudgetLimits::default(),
            RunClass::Agent,
            &constraints(50, None, None),
            Some(&claim(0, 2, 0)),
            None,
            start,
        );
        budget.charge(true, 100);
        budget.charge(true, 100);
        let refused = budget.gate(BudgetMode::On, 0, start).unwrap_err();
        assert_eq!(refused.code, ErrorCode::BudgetExhausted);
        let details = refused.details.unwrap();
        assert_eq!(details["dimension"], "steps");
        assert_eq!(details["limit"], 2.0);
        assert_eq!(details["limit_source"], "grant");

        // Without a claim the same cap is the run's own.
        let mut plain = RunBudget::start(
            &BudgetLimits::default(),
            RunClass::Agent,
            &constraints(2, None, None),
            None,
            start,
        );
        plain.charge(true, 0);
        plain.charge(true, 0);
        let refused = plain.gate(BudgetMode::On, 0, start).unwrap_err();
        assert_eq!(refused.details.unwrap()["limit_source"], "run_budget");

        // Attempts are bounded by the claim's step cap, so they name it too.
        let mut failing = RunBudget::start_with_claim(
            &BudgetLimits::default(),
            RunClass::Agent,
            &constraints(50, None, None),
            Some(&claim(0, 1, 0)),
            None,
            start,
        );
        failing.charge(false, 0);
        failing.charge(false, 0);
        let refused = failing.gate(BudgetMode::On, 0, start).unwrap_err();
        let details = refused.details.unwrap();
        assert_eq!((details["dimension"].as_str(), details["limit_source"].as_str()), (Some("attempts"), Some("grant")));
    }

    #[test]
    fn claim_is_not_rewidened_when_the_run_resumes() {
        let start = Instant::now();
        let limits = BudgetLimits::default();
        let first = RunBudget::start_with_claim(
            &limits,
            RunClass::Agent,
            &constraints(50, None, None),
            Some(&claim(0, 10, 0)),
            None,
            start,
        );
        let saved = first.snapshot(start);
        assert_eq!(saved.claim.as_ref().map(|c| c.max_steps), Some(10));

        let steps_after = |claim: Option<&GrantBudget>| {
            RunBudget::start_with_claim(
                &limits,
                RunClass::Agent,
                &constraints(50, None, None),
                claim,
                Some(&saved),
                Instant::now(),
            )
            .caps
            .unwrap()
            .max_steps
        };
        assert_eq!(steps_after(None), 10, "a resume with no claim keeps the stored one");
        assert_eq!(steps_after(Some(&claim(0, 40, 0))), 10, "a wider claim does not widen it");
        assert_eq!(steps_after(Some(&claim(0, 4, 0))), 4, "a tighter one still tightens it");

        // The claim travels in the checkpoint's JSON and an old checkpoint without it loads.
        let json = serde_json::to_value(&saved).unwrap();
        assert_eq!(json["claim"]["max_steps"], 10);
        let back: BudgetCheckpoint = serde_json::from_value(json).unwrap();
        assert_eq!(back, saved);
        let old: BudgetCheckpoint =
            serde_json::from_str(r#"{"class":"agent","steps":3,"attempts":3,"spent_micro_usd":0,"active_ms":0}"#).unwrap();
        assert!(old.claim.is_none());
        let bare = RunBudget::start(&limits, RunClass::Agent, &constraints(50, None, None), None, start);
        assert!(serde_json::to_value(bare.snapshot(start)).unwrap().get("claim").is_none());
    }

    #[test]
    fn tighter_claim_takes_the_smaller_bound_per_dimension() {
        let a = claim(1_000_000, 10, 0);
        let b = claim(2_000_000, 0, 300);
        let merged = tighter_claim(Some(&a), Some(&b)).unwrap();
        assert_eq!((merged.micro_usd, merged.max_steps, merged.max_runtime_s), (1_000_000, 10, 300));
        assert_eq!(merged.minted_at_unix, a.minted_at_unix, "the first claim's label and stamp stand");
        assert_eq!(tighter_claim(None, None), None);
        assert_eq!(tighter_claim(Some(&a), None), Some(a.clone()));
        assert_eq!(tighter_claim(None, Some(&b)), Some(b));
        assert_eq!(tighter_claim(Some(&claim(0, 0, 0)), None), None, "an empty claim is no claim");
    }

    #[test]
    fn claim_mode_parses_and_defaults_to_ignore() {
        assert_eq!(ClaimMode::parse("").unwrap(), ClaimMode::Ignore);
        assert_eq!(ClaimMode::parse(" IGNORE ").unwrap(), ClaimMode::Ignore);
        assert_eq!(ClaimMode::parse("enforce").unwrap(), ClaimMode::Enforce);
        let error = ClaimMode::parse("on").unwrap_err();
        assert!(error.contains(ENV_CLAIM_MODE), "{error}");
        assert_eq!(ClaimMode::Enforce.label(), "enforce");
    }
}
