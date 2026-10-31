use super::*;

use quarry_runtime::action_cost::CostSource;

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
/// flat table (WS6.B).
pub(super) fn failed_step_cost(
    ran: Option<&BrowserTelemetry>,
    action: &AgentAction,
) -> (f64, CostSource) {
    match ran {
        Some(telemetry) => (
            receipt_cost_usd(telemetry, action),
            receipt_cost_source(telemetry),
        ),
        None => (estimated_action_cost_usd(action), CostSource::FlatTable),
    }
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
