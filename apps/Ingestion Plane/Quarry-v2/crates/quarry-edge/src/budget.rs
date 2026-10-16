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
//!
//! Everything numeric here is a placeholder pending the ratified budget numbers
//! (plan decision X-12), which is why enforcement ships as `shadow`: it records
//! what it *would* refuse, by class, and refuses nothing until an operator sets
//! `QUARRY_EDGE__BUDGET_ENFORCEMENT=on` after reading that record.

use std::sync::OnceLock;
use std::time::Instant;

use quarry_core::contracts::AgentConstraints;
use quarry_core::error::{ErrorCode, QuarryError};
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
/// step check).
pub fn resolve_caps(
    limits: &BudgetLimits,
    class: RunClass,
    requested: &AgentConstraints,
) -> Option<(BudgetCaps, Vec<Clamp>)> {
    if class == RunClass::Interactive {
        return None;
    }
    let mut clamps = Vec::new();

    let max_steps = match requested.max_steps {
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

    let max_runtime_s = match requested.max_runtime_s {
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

    let max_cost_usd = match requested.max_cost_usd {
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

    Some((
        BudgetCaps {
            max_steps,
            max_attempts: max_steps.saturating_mul(limits.attempt_factor.max(1)),
            max_runtime_s,
            max_cost_usd,
            browser_second_micro_usd: usd_to_micro(limits.browser_second_usd),
        },
        clamps,
    ))
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
        let browser_time =
            (self.active_ms(now) / 1000).saturating_mul(caps.browser_second_micro_usd);
        self.spent_micro_usd.saturating_add(browser_time)
    }
}

/// Why a run was refused.
#[derive(Debug, Clone, PartialEq)]
pub struct Exhausted {
    pub dimension: Dimension,
    pub limit: f64,
    pub spent: f64,
    pub unit: &'static str,
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
            "limit_source": "run_budget",
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
    Err(Exhausted {
        dimension,
        limit,
        spent,
        unit: dimension.unit(),
    })
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
        let class = resumed
            .and_then(|checkpoint| RunClass::from_label(&checkpoint.class))
            .map_or(class, |stored| stored.stricter(class));
        let (caps, clamps) = match resolve_caps(limits, class, requested) {
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
        }
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
            return Err(exhausted.into_error());
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
}
