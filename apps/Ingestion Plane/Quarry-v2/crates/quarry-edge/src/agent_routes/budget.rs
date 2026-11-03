use super::*;

use quarry_runtime::action_cost::{estimate_refused_action_cost, CostSource};

pub(super) fn enforce_step_budget(entry: &RunEntry) -> Result<(), &'static str> {
    if entry.constraints.max_steps > 0 && entry.summary.step >= entry.constraints.max_steps {
        return Err("agent step budget exhausted");
    }
    Ok(())
}

/// The budget gate before an action: refuses with a typed 402 in `on` mode,
/// records what it would refuse in `shadow`, and does nothing for a human
/// session or with enforcement `off`.
pub(super) fn budget_gate(entry: &mut RunEntry, action: &AgentAction) -> QuarryResult<()> {
    budget_gate_with(entry, action, crate::budget::current_mode())
}

pub(super) fn budget_gate_with(
    entry: &mut RunEntry,
    action: &AgentAction,
    mode: crate::budget::BudgetMode,
) -> QuarryResult<()> {
    let projected = crate::budget::micro_usd(estimated_action_cost_usd(action));
    match entry.budget.as_mut() {
        Some(budget) => budget.gate(mode, projected, std::time::Instant::now()),
        None => Ok(()),
    }
}

/// Account for an executed action. A failed attempt costs the flat estimate
/// and counts toward the attempt allowance, but is not a completed step.
pub(super) fn budget_charge(entry: &mut RunEntry, completed: bool, cost_usd: f64) {
    if let Some(budget) = entry.budget.as_mut() {
        budget.charge(completed, crate::budget::micro_usd(cost_usd));
    }
}

/// The receipt cost of a completed action: the driver's verified cost when
/// it supplied one, otherwise the flat table. One definition for the REST
/// and WebSocket paths; the WebSocket path used to record 0 here.
pub(super) fn receipt_cost_usd(telemetry: &BrowserTelemetry, action: &AgentAction) -> f64 {
    let verified = telemetry_receipt_cost_usd(telemetry);
    if verified == 0.0 {
        estimated_action_cost_usd(action)
    } else {
        verified
    }
}

pub(super) fn browser_action_name(action: &AgentAction) -> &'static str {
    match action {
        AgentAction::Navigate { .. } => "navigate",
        AgentAction::Click { .. } => "click",
        AgentAction::ClickRef { .. } => "click_ref",
        AgentAction::FrameClickRef { .. } => "frame_click_ref",
        AgentAction::ClickSemantic { .. } => "click_semantic",
        AgentAction::ClickPoint { .. } => "click_point",
        AgentAction::Type { .. } => "type",
        AgentAction::TypeRef { .. } => "type_ref",
        AgentAction::FrameTypeRef { .. } => "frame_type_ref",
        AgentAction::TypeSemantic { .. } => "type_semantic",
        AgentAction::Press { .. } => "press",
        AgentAction::Scroll { .. } => "scroll",
        AgentAction::MouseWheel { .. } => "mouse_wheel",
        AgentAction::Select { .. } => "select",
        AgentAction::SelectRef { .. } => "select_ref",
        AgentAction::FrameSelectRef { .. } => "frame_select_ref",
        AgentAction::SelectSemantic { .. } => "select_semantic",
        AgentAction::Wait { .. } => "wait",
        AgentAction::WaitFor { .. } => "wait_for",
        AgentAction::WaitForRef { .. } => "wait_for_ref",
        AgentAction::FrameWaitForRef { .. } => "frame_wait_for_ref",
        AgentAction::WaitForSemantic { .. } => "wait_for_semantic",
        AgentAction::RespondDialog { .. } => "respond_dialog",
        AgentAction::UploadRef { .. } => "upload_ref",
        AgentAction::FrameUploadRef { .. } => "frame_upload_ref",
        AgentAction::DownloadRef { .. } => "download_ref",
        AgentAction::FrameDownloadRef { .. } => "frame_download_ref",
        AgentAction::Screenshot { .. } => "screenshot",
        AgentAction::Pdf => "pdf",
        AgentAction::Evaluate { .. } => "evaluate",
        AgentAction::Back => "back",
        AgentAction::Forward => "forward",
        AgentAction::Reload { .. } => "reload",
        AgentAction::GetContent => "get_content",
    }
}

/// Step receipts retain a provider-authoritative action cost whenever the
/// driver supplied one. W4: when the driver omits a cost, fall back to
/// the flat action-cost table so `max_cost_usd` budgets are enforceable
/// even without provider-billed metering. Callers can distinguish
/// estimated vs provider-billed via the action_cost source field on
/// the receipt (flat_table vs provider_billed) once W4's receipt
/// `cost` field lands; for now the micro_usd value itself is the
/// budget signal.
pub(super) fn telemetry_receipt_cost_usd(telemetry: &BrowserTelemetry) -> f64 {
    telemetry
        .verified_action_cost_micro_usd
        .map(|micro_usd| micro_usd as f64 / 1_000_000.0)
        .unwrap_or(0.0)
}

/// Where the cost of a completed action came from, for its receipt and `meta.budget`:
/// the driver's figure when it supplied one, otherwise the flat table (WS6.B). The
/// same condition as [`receipt_cost_usd`].
pub(super) fn receipt_cost_source(telemetry: &BrowserTelemetry) -> CostSource {
    if telemetry_receipt_cost_usd(telemetry) == 0.0 {
        CostSource::FlatTable
    } else {
        CostSource::ProviderBilled
    }
}

/// The cost of a failed step and its source. `ran` is the telemetry of a browser action
/// that ran and whose landed page the edge then refused (T4b-1): it is charged as a
/// completed action is, since the action did run. An action that failed is charged the
/// flat table (WS6.B). Two cases differ. A driver figure is kept whatever the action. A
/// navigation whose landed page was refused and that the driver did not meter costs the
/// refusal rate, not the flat navigate cost (W2-U21a): `ran` is `Some` only for a refused
/// landing, so that case is always a refusal by policy.
pub(super) fn failed_step_cost(
    ran: Option<&BrowserTelemetry>,
    action: &AgentAction,
) -> (f64, CostSource) {
    match ran {
        Some(telemetry)
            if receipt_cost_source(telemetry) == CostSource::FlatTable
                && is_navigation(action) =>
        {
            refused_navigation_cost(action)
        }
        Some(telemetry) => (
            receipt_cost_usd(telemetry, action),
            receipt_cost_source(telemetry),
        ),
        None => (estimated_action_cost_usd(action), CostSource::FlatTable),
    }
}

/// [`failed_step_cost`] for a failure whose error is known (W2-U21a). A navigation the driver
/// refused before any page was used (`ran` is `None` and the error is `SECURITY_BLOCKED`: the
/// run's domain grant, the egress guard or the target guard refused it) costs the refusal
/// rate. A navigation that timed out or failed some other way is not a refusal and keeps the
/// flat navigate cost. Call sites that hold the error should use this function.
// No non-test caller yet: the failed-step call sites in step.rs and attach.rs switch to it
// (see W23-W2-U21a-status.md). Remove this attribute when they do.
#[allow(dead_code)]
pub(super) fn failed_step_cost_for_error(
    ran: Option<&BrowserTelemetry>,
    action: &AgentAction,
    error: &QuarryError,
) -> (f64, CostSource) {
    match ran {
        None if is_navigation(action) && error.code == ErrorCode::SecurityBlocked => {
            refused_navigation_cost(action)
        }
        _ => failed_step_cost(ran, action),
    }
}

/// Whether the action is a navigation: the one action the refusal rate applies to.
fn is_navigation(action: &AgentAction) -> bool {
    matches!(action, AgentAction::Navigate { .. })
}

/// The refusal rate of a refused navigation, and its source. The flat navigate cost it
/// replaces is counted beside it in the shadow counters.
fn refused_navigation_cost(action: &AgentAction) -> (f64, CostSource) {
    let (flat_micro, charged_micro) = refusal_micro_pair(action);
    record_refused_navigation(flat_micro, charged_micro);
    (
        estimate_refused_action_cost(browser_action_name(action)).total_usd,
        CostSource::FlatTable,
    )
}

/// What a refused navigation costs on the flat table and at the refusal rate, in micro-USD.
fn refusal_micro_pair(action: &AgentAction) -> (u64, u64) {
    let flat = crate::budget::micro_usd(estimated_action_cost_usd(action));
    let refused = crate::budget::micro_usd(
        estimate_refused_action_cost(browser_action_name(action)).total_usd,
    );
    (flat, refused)
}

/// The shadow counters of refused navigations (W2-U21a): how many were charged, and both
/// numbers under the enforcement mode they were charged in, so the X-12 shadow review can read
/// the refusal rate against the flat navigate cost. Recorded in every mode: the charge is not
/// gated by the mode.
fn record_refused_navigation(flat_micro: u64, charged_micro: u64) {
    let mode = crate::budget::current_mode().label();
    metrics::counter!("quarry_agent_refused_navigation_total", "mode" => mode).increment(1);
    metrics::counter!("quarry_agent_refused_navigation_flat_micro_usd_total", "mode" => mode)
        .increment(flat_micro);
    metrics::counter!("quarry_agent_refused_navigation_charged_micro_usd_total", "mode" => mode)
        .increment(charged_micro);
}

pub(super) fn cost_source_label(source: CostSource) -> &'static str {
    match source {
        CostSource::FlatTable => "flat_table",
        CostSource::Estimated => "estimated",
        CostSource::ProviderBilled => "provider_billed",
    }
}

/// Announce the refusal the last budget gate made, once per segment: the
/// `agent.budget_exhausted` event, and a checkpoint that stores the latch so a
/// resume restores it. Nothing is written for a ZDR run (the event goes through
/// `emit_for_zdr`). Errors are logged, never returned: the caller returns the 402.
pub(super) async fn announce_budget_refusal(
    state: &AppState,
    entry: &mut RunEntry,
    live: &RunLive,
) {
    let zdr = entry.zdr;
    let run_id = entry.run_id.clone();
    let Some(payload) = entry
        .budget
        .as_mut()
        .and_then(|budget| budget.take_refusal_announcement())
    else {
        return;
    };
    state
        .event_sink
        .emit_for_zdr(
            zdr,
            run_id.clone(),
            EventType::AgentBudgetExhausted,
            payload,
            format!("{run_id}:agent_budget_exhausted:{}", uuid::Uuid::new_v4()),
        )
        .await;
    if !zdr.is_active() {
        if let Err(error) = state
            .receipts
            .save_run_checkpoint(checkpoint_for_entry(entry, live))
            .await
        {
            tracing::warn!(
                error_class = crate::zdr_log::class(&error),
                "could not checkpoint a budget refusal"
            );
        }
    }
}

/// W4: estimate a cost for an action when no provider-billed cost is
/// available. Used as the fallback inside `receipt_cost_usd`
/// callers and by the AgentLoop budget check. The estimate is
/// intentionally conservative (flat table) so budgets fail closed
/// rather than silently under-billing.
pub(super) fn estimated_action_cost_usd(action: &AgentAction) -> f64 {
    let name = match action {
        AgentAction::Navigate { .. } => "navigate",
        AgentAction::Click { .. } => "click",
        AgentAction::ClickRef { .. } => "click",
        AgentAction::FrameClickRef { .. } => "click",
        AgentAction::Type { .. } => "type",
        AgentAction::TypeRef { .. } => "type",
        AgentAction::FrameTypeRef { .. } => "type",
        AgentAction::Press { .. } => "press",
        AgentAction::Scroll { .. } => "scroll",
        AgentAction::Select { .. } => "select",
        AgentAction::SelectRef { .. } => "select",
        AgentAction::FrameSelectRef { .. } => "select",
        AgentAction::Wait { .. } => "wait",
        AgentAction::WaitFor { .. } => "wait",
        AgentAction::WaitForRef { .. } => "wait",
        AgentAction::FrameWaitForRef { .. } => "wait",
        AgentAction::WaitForSemantic { .. } => "wait",
        AgentAction::RespondDialog { .. } => "press",
        AgentAction::UploadRef { .. } => "type",
        AgentAction::FrameUploadRef { .. } => "type",
        AgentAction::DownloadRef { .. } => "download_ref",
        AgentAction::FrameDownloadRef { .. } => "frame_download_ref",
        AgentAction::Screenshot { .. } => "screenshot",
        AgentAction::Pdf => "pdf",
        AgentAction::Evaluate { .. } => "evaluate",
        AgentAction::Back => "back",
        AgentAction::Forward => "forward",
        AgentAction::Reload { .. } => "reload",
        AgentAction::GetContent => "get_content",
        AgentAction::MouseWheel { .. } => "mouse_wheel",
        AgentAction::SelectSemantic { .. } => "select",
        AgentAction::ClickSemantic { .. }
        | AgentAction::ClickPoint { .. }
        | AgentAction::TypeSemantic { .. } => "click",
    };
    quarry_runtime::action_cost::estimate_action_cost(name, 0).total_usd
}

#[cfg(test)]
mod reload_label_tests {
    use super::*;

    #[test]
    fn action_label_reload() {
        assert_eq!(
            browser_action_name(&AgentAction::Reload { ignore_cache: true }),
            "reload"
        );
        assert_eq!(
            browser_action_name(&AgentAction::Reload { ignore_cache: false }),
            "reload"
        );
        assert!(estimated_action_cost_usd(&AgentAction::Reload { ignore_cache: false }) > 0.0);
    }
}

#[cfg(test)]
mod refusal_cost_tests {
    use super::*;
    use crate::budget::{BudgetLimits, BudgetMode, RunBudget, RunClass};
    use quarry_runtime::action_cost::REFUSAL_RATE_USD;
    use std::time::Instant;

    fn navigate() -> AgentAction {
        AgentAction::Navigate {
            url: "http://fixture-b.crawl.test/ext".to_owned(),
        }
    }

    /// The error the driver raises for a navigation the run's domain grant refuses
    /// (`navigation_refused` in quarry-browser): `SECURITY_BLOCKED`, with no URL in it.
    fn refused_by_grant() -> QuarryError {
        QuarryError::new(
            ErrorCode::SecurityBlocked,
            "browser navigation is outside the run's allowed domains",
        )
        .with_details(serde_json::json!({ "reason": "domain_grant_blocked", "op": "navigate" }))
    }

    fn timed_out() -> QuarryError {
        QuarryError::new(ErrorCode::Timeout, "navigation timed out")
    }

    /// The browser-time term switched off, so the ledger counts only the flat charges.
    fn limits() -> BudgetLimits {
        BudgetLimits {
            browser_second_usd: 0.0,
            ..BudgetLimits::default()
        }
    }

    fn constraints(max_cost_usd: f64) -> AgentConstraints {
        AgentConstraints {
            max_steps: 10,
            allowed_domains: vec![],
            max_runtime_s: None,
            max_cost_usd: Some(max_cost_usd),
        }
    }

    fn agent_budget(max_cost_usd: f64) -> RunBudget {
        RunBudget::start(
            &limits(),
            RunClass::Agent,
            &constraints(max_cost_usd),
            None,
            Instant::now(),
        )
    }

    #[test]
    fn a_navigation_the_driver_refuses_costs_the_refusal_rate() {
        let (cost, source) = failed_step_cost_for_error(None, &navigate(), &refused_by_grant());
        assert_eq!(cost, REFUSAL_RATE_USD);
        assert_eq!(source, CostSource::FlatTable);
        assert!(cost < estimated_action_cost_usd(&navigate()));
    }

    #[test]
    fn a_navigation_that_times_out_is_not_a_refusal_and_keeps_the_flat_cost() {
        assert_eq!(
            failed_step_cost_for_error(None, &navigate(), &timed_out()),
            (estimated_action_cost_usd(&navigate()), CostSource::FlatTable)
        );
    }

    #[test]
    fn a_refused_action_that_is_not_a_navigation_keeps_its_flat_cost() {
        let click = AgentAction::Click {
            selector: "#go".to_owned(),
        };
        assert_eq!(
            failed_step_cost_for_error(None, &click, &refused_by_grant()),
            (estimated_action_cost_usd(&click), CostSource::FlatTable)
        );
    }

    #[test]
    fn a_refused_landing_costs_the_refusal_rate_unless_the_driver_metered_it() {
        // The action ran, its landed page was refused, and the driver gave no figure.
        assert_eq!(
            failed_step_cost(Some(&BrowserTelemetry::default()), &navigate()),
            (REFUSAL_RATE_USD, CostSource::FlatTable)
        );
        // A driver figure is kept: the refusal rate replaces only the flat navigate cost.
        let metered = BrowserTelemetry {
            verified_action_cost_micro_usd: Some(12_345),
            ..BrowserTelemetry::default()
        };
        let (cost, source) = failed_step_cost(Some(&metered), &navigate());
        assert!((cost - 0.012_345).abs() < 1e-12);
        assert_eq!(source, CostSource::ProviderBilled);
    }

    #[test]
    fn the_shadow_pair_keeps_the_flat_navigate_cost_beside_the_refusal_rate() {
        assert_eq!(refusal_micro_pair(&navigate()), (1_000, 100));
    }

    #[test]
    fn refused_navigations_charge_the_refusal_rate_to_the_ledger() {
        // A cap of 1500 micro-USD. The gate projects each navigate at the flat 1000 before it runs.
        let mut budget = agent_budget(0.0015);
        let now = Instant::now();
        let mut refused = 0_u32;
        while budget.gate(BudgetMode::On, 1_000, now).is_ok() {
            let (cost, _) = failed_step_cost_for_error(None, &navigate(), &refused_by_grant());
            budget.charge(false, crate::budget::micro_usd(cost));
            refused += 1;
            assert!(refused < 100, "the cap must be reached");
        }
        // 100 each: the gate passes 500 + 1000 (1500 is not over the cap) and refuses 600 + 1000.
        assert_eq!(refused, 6);
        assert_eq!(budget.ledger.spent_micro_usd, 600);
        assert_eq!(budget.ledger.attempts, 6);
        assert_eq!(budget.ledger.steps, 0, "a refused navigation is not a completed step");
    }

    #[test]
    fn under_the_flat_navigate_charge_the_same_cap_refuses_the_second_navigation() {
        let mut budget = agent_budget(0.0015);
        let now = Instant::now();
        budget
            .gate(BudgetMode::On, 1_000, now)
            .expect("the first navigation runs");
        budget.charge(
            false,
            crate::budget::micro_usd(estimated_action_cost_usd(&navigate())),
        );
        assert!(
            budget.gate(BudgetMode::On, 1_000, now).is_err(),
            "1000 spent and 1000 more projected is over 1500"
        );
    }

    #[test]
    fn a_resumed_segment_keeps_the_refusal_charges_it_resumes() {
        let now = Instant::now();
        let mut first = agent_budget(0.0015);
        for _ in 0..3 {
            let (cost, _) = failed_step_cost_for_error(None, &navigate(), &refused_by_grant());
            first.charge(false, crate::budget::micro_usd(cost));
        }
        let checkpoint = first.snapshot(now);
        let resumed = RunBudget::start(
            &limits(),
            RunClass::Agent,
            &constraints(0.0015),
            Some(&checkpoint),
            now,
        );
        assert_eq!(resumed.segment, 1);
        assert_eq!(resumed.ledger.spent_micro_usd, 300, "a resume cannot refill the cap");
    }
}
