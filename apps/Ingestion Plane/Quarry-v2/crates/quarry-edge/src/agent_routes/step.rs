use super::*;

use quarry_runtime::action_cost::CostSource;

/// WS8.H: the person's lane of a run. Reads the entry lock for one clone of the handle.
pub(super) async fn human_lane_of(handle: &RunHandle) -> LaneRef {
    handle.entry.lock().await.human_lane.clone()
}

/// The refusal of a step that landed outside the run's allowed domains.
const LANDED_OUTSIDE_RUN: &str =
    "browser action reached a domain outside the run's allowed domains";

/// The check of the page a step landed on, after the action ran: the host rule (the run's,
/// or the attachment's for a delegate) and then the run's web policy (WS7.D). Every step
/// path calls it before it records anything, so a refused step leaves a `failed` receipt and
/// no checkpoint of that page (T4b-1).
///
/// A tab with no page yet (`about:blank` exactly, `human_browse::is_blank_tab`) passes: a
/// run's first navigation that never commits leaves its tab there, and the next observation
/// of it reached no host. That is all it is accepted as; as a navigation target it is still
/// refused (gate 11, the tab and attach routes, and the browser's egress guard).
pub(super) fn check_landed_page(
    state: &AppState,
    live: &RunLive,
    zdr: bool,
    url: &str,
    domain_allows: impl FnOnce(&str) -> bool,
    outside: &'static str,
) -> QuarryResult<()> {
    if super::human_browse::is_blank_tab(url) {
        return Ok(());
    }
    if !domain_allows(url) {
        return Err(QuarryError::new(ErrorCode::SecurityBlocked, outside));
    }
    crate::agent_policy_gate::gate_visit(state, live, zdr, url)
}

/// [`check_landed_page`] against the run's own navigation policy.
fn check_landed_on_run(
    state: &AppState,
    handle: &RunHandle,
    entry: &RunEntry,
    obs: BrowserObservation,
) -> QuarryResult<BrowserObservation> {
    check_landed_page(
        state,
        &handle.live,
        entry.zdr.is_active(),
        &obs.url,
        |url| {
            entry
                .nav_policy
                .allows(url, &entry.constraints.allowed_domains)
        },
        LANDED_OUTSIDE_RUN,
    )?;
    Ok(obs)
}

/// `POST /v1/agent/runs/{run_id}/step` — execute one action, return the
/// resulting observation. `ObservationRunner` emits `ActionStarted`/`ObservationReady`
/// itself, keyed by the step key this lane reserved.
///
/// WS8.H: the step runs on the person's lane. The run entry is held for admission and
/// for the bookkeeping after the browser call, and not while the browser runs, so a
/// delegate step on the attached lane (or a projection) never waits for this one.
pub async fn step(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(run_id): Path<String>,
    Json(body): Json<StepBody>,
) -> Result<Json<Envelope<BrowserObservation>>, ApiErr> {
    let request_id = crate::request_id::current();

    let Some(handle) = lookup_run(&state, &run_id) else {
        return Err(run_gone_err(&state, &claims, &request_id, &run_id).await);
    };
    // WS8.F: a caller that does not own the run is refused unless it is the delegate of the
    // run's live attachment (the lane check); the delegate lane has its own admission.
    if !handle.live.owned_by(&claims) {
        return super::attach::delegate_step(&state, &handle, &claims, &request_id, body).await;
    }

    // Counts as in flight from before it waits for its lane, so the sweeper
    // never releases a run an action is queued on.
    let _in_flight = handle.live.in_flight_guard();
    // The lane first, then the entry (the order `LaneState` documents).
    let lane_ref = human_lane_of(&handle).await;
    let mut lane = lane_ref.0.lock().await;
    let mut entry = handle.entry.lock().await;
    entry.zdr.mark_current_span();
    if !caller_owns_live_run(&entry, &claims) {
        return Err(status_err(
            StatusCode::NOT_FOUND,
            &request_id,
            "agent run not found",
        ));
    }
    handle.live.touch();
    // The run was closed while this step waited for its lane.
    if entry.closed {
        return Err(run_closed_err(&state, &claims, &request_id, &run_id).await);
    }

    // Every gate, in one fixed order (see `admit_action`).
    let admitted = admit_action(
        &state,
        &handle,
        &mut entry,
        AdmitInput {
            initiator: derive_initiator_ref(&claims, body.initiated_by, &body.advisory_agent()),
            action: &body.action,
            enforce_takeover: crate::run_limits::takeover_enforced(),
            last_observation: lane.last_observation.as_deref(),
        },
    )
    .await
    .map_err(|error| driver_err(&request_id, error))?;
    super::attach::person_tab_gate(&state, &handle, &entry, admitted.initiator, &body.action)
        .await
        .map_err(|error| driver_err(&request_id, error))?;
    // WS8.B: the receipt plan and the budget exemption of this step.
    let receipt_plan = super::human_browse::receipt_plan(
        admitted.initiator,
        entry.zdr.is_active(),
        crate::run_limits::human_receipt_mode(),
    );
    let metadata_only = receipt_plan == super::human_browse::ReceiptPlan::Metadata;
    let counts_budget =
        super::human_browse::counts_against_step_budget(
            admitted.initiator,
            admitted.takeover,
            entry.budget.as_ref(),
        );
    // TAKEOVER-BUDGET: a person's takeover step the budget does not count is metered apart.
    let takeover_exempt = super::human_browse::takeover_exempted_step(
        admitted.initiator,
        admitted.takeover,
        entry.budget.as_ref(),
    );

    let req = AgentActionRequest {
        run_id: entry.run_id.clone(),
        lease_id: entry.lease.lease_id.clone(),
        action: body.action,
        instruction: body.instruction,
        constraints: entry.constraints.clone(),
        zdr: entry.zdr,
        extraction_profile: body.extraction_profile,
    };
    // WS8.H: the step key is reserved now, after admission, so a refusal takes none.
    let receipt_step = handle.live.reserve_step();
    lane.ctx.step = receipt_step;
    let receipt_run_id = entry.run_id.to_string();
    let receipt_action = req.action.clone();
    // WS12.S3: the kind of the control a typing action goes to, read from the snapshot the
    // action names. `execute` replaces `ctx.active_snapshot`, so this is read before it runs.
    let typed_kind = quarry_runtime::observation::receipt_field_kind(&lane.ctx, &req.action);

    let runner = ObservationRunner {
        browser: state.agent_driver.clone(),
        artifacts: Some(state.artifacts.clone()),
        events: Some(state.event_sink.clone()),
        visual_processor: state.visual_processor.clone(),
        // The run's org (already checked against `claims.org_id` above) is
        // the tenant every observation artifact is stamped with.
        org_id: entry.org_id.clone(),
    };
    let session = entry.session.clone();
    let org_id = entry.org_id.clone();
    let actor_id = entry.actor_id.clone();
    // The run entry is released while the browser runs (WS8.H).
    drop(entry);

    // A completed browser call is not automatically a verified business
    // effect; the observation carries that distinction. Persist an
    // immutable receipt for both success and failure so a planner or
    // reviewer can replay the exact action sequence.
    let step_started = std::time::Instant::now();
    let step_result = runner.execute(&req, &session, &mut lane.ctx).await;
    crate::metrics::record_agent_step(
        browser_action_name(&receipt_action),
        step_result.is_ok(),
        step_started.elapsed(),
    );
    if takeover_exempt {
        crate::metrics::record_takeover_exempt_step(step_started.elapsed());
    }
    let mut entry = handle.entry.lock().await;
    // The landed page is checked before the receipt and the checkpoint (T4b-1): a page the
    // caller is refused is a failed step, and its observation is not kept.
    let step_result = step_result.and_then(|obs| check_landed_on_run(&state, &handle, &entry, obs));
    let (obs, cost_source) = match step_result {
        Ok(obs) => {
            // W4: when the driver omits a provider-billed cost, fall
            // back to the flat action-cost table so max_cost_usd
            // budgets are enforceable.
            let cost_usd = receipt_cost_usd(&obs.telemetry, &receipt_action);
            let cost_source = receipt_cost_source(&obs.telemetry);
            if counts_budget {
                budget_charge(&mut entry, true, cost_usd);
            }
            let receipt = ReceiptBuilder::start(receipt_run_id.clone(), receipt_step)
                .org_id(org_id.clone())
                .actor_id(actor_id.clone())
                .initiator(admitted.initiator, admitted.control_epoch)
                .agent(admitted.agent.clone())
                .cost_source(cost_source)
                .typed_field(typed_kind)
                .complete_maybe_metadata(metadata_only, receipt_action, Some(obs.clone()), cost_usd);
            if receipt_plan != super::human_browse::ReceiptPlan::Skip {
                state
                    .receipts
                    .append(receipt)
                    .await
                    .map_err(|e| driver_err(&request_id, e))?;
            }
            (obs, cost_source)
        }
        Err(error) => {
            // A failed attempt is charged the flat cost, so its receipt carries it too (WS6.B).
            let flat_usd = estimated_action_cost_usd(&receipt_action);
            if counts_budget {
                budget_charge(&mut entry, false, flat_usd);
            }
            let receipt = ReceiptBuilder::start(receipt_run_id, receipt_step)
                .org_id(org_id.clone())
                .actor_id(actor_id.clone())
                .initiator(admitted.initiator, admitted.control_epoch)
                .agent(admitted.agent.clone())
                .failed_cost(flat_usd)
                .cost_source(CostSource::FlatTable)
                .typed_field(typed_kind)
                .fail_maybe_metadata(metadata_only, receipt_action, &error);
            if receipt_plan != super::human_browse::ReceiptPlan::Skip {
                state
                    .receipts
                    .append(receipt)
                    .await
                    .map_err(|e| driver_err(&request_id, e))?;
            }
            return Err(driver_err(&request_id, error));
        }
    };

    let observation = Arc::new(obs.clone());
    lane.last_observation = Some(observation.clone());
        entry.summary.record(&lane.ctx, observation);

    // Persist the continuation only after the immutable outcome receipt is
    // durable. If this write fails, surface an internal error rather than
    // letting the caller continue with a browser state that cannot be
    // recovered after a crash.
    if !entry.zdr.is_active() {
        let checkpoint = checkpoint_for_entry(&entry);
        state
            .receipts
            .save_run_checkpoint(checkpoint)
            .await
            .map_err(|error| driver_err(&request_id, error))?;
    }

    // `meta.budget` (WS6.B): the run's ledger after this step, labelled with the source of
    // this step's cost. Absent when budgets are off.
    let budget = entry
        .budget
        .as_ref()
        .map(|budget| budget.meta(cost_source_label(cost_source), std::time::Instant::now()));
    Ok(Json(Envelope::ok(request_id, obs).with_budget(budget)))
}

pub(super) async fn execute_ws_action(
    state: &AppState,
    handle: &RunHandle,
    claims: &crate::auth::Claims,
    action: AgentAction,
    instruction: Option<String>,
    hint: Option<InitiatorHint>,
) -> QuarryResult<BrowserObservation> {
    let _in_flight = handle.live.in_flight_guard();
    let lane_ref = human_lane_of(handle).await;
    let mut lane = lane_ref.0.lock().await;
    let mut entry = handle.entry.lock().await;
    handle.live.touch();
    let admitted = admit_action(
        state,
        handle,
        &mut entry,
        AdmitInput {
            initiator: derive_initiator_ref(claims, hint, &AdvisoryAgent::default()),
            action: &action,
            enforce_takeover: crate::run_limits::takeover_enforced(),
            last_observation: lane.last_observation.as_deref(),
        },
    )
    .await?;
    super::attach::person_tab_gate(state, handle, &entry, admitted.initiator, &action).await?;
    // WS8.B: the receipt plan and the budget exemption of this step.
    let receipt_plan = super::human_browse::receipt_plan(
        admitted.initiator,
        entry.zdr.is_active(),
        crate::run_limits::human_receipt_mode(),
    );
    let metadata_only = receipt_plan == super::human_browse::ReceiptPlan::Metadata;
    let counts_budget =
        super::human_browse::counts_against_step_budget(
            admitted.initiator,
            admitted.takeover,
            entry.budget.as_ref(),
        );
    // TAKEOVER-BUDGET: a person's takeover step the budget does not count is metered apart.
    let takeover_exempt = super::human_browse::takeover_exempted_step(
        admitted.initiator,
        admitted.takeover,
        entry.budget.as_ref(),
    );
    let req = AgentActionRequest {
        run_id: entry.run_id.clone(),
        lease_id: entry.lease.lease_id.clone(),
        action,
        instruction,
        constraints: entry.constraints.clone(),
        zdr: entry.zdr,
        extraction_profile: None,
    };
    let receipt_step = handle.live.reserve_step();
    lane.ctx.step = receipt_step;
    let receipt_action = req.action.clone();
    // WS12.S3: the kind of the control a typing action goes to, read from the snapshot the
    // action names. `execute` replaces `ctx.active_snapshot`, so this is read before it runs.
    let typed_kind = quarry_runtime::observation::receipt_field_kind(&lane.ctx, &req.action);
    let receipt_run_id = entry.run_id.to_string();
    let runner = ObservationRunner {
        browser: state.agent_driver.clone(),
        artifacts: Some(state.artifacts.clone()),
        events: Some(state.event_sink.clone()),
        visual_processor: state.visual_processor.clone(),
        // The run's org (already checked against `claims.org_id` above) is
        // the tenant every observation artifact is stamped with.
        org_id: entry.org_id.clone(),
    };
    let session = entry.session.clone();
    let org_id = entry.org_id.clone();
    let actor_id = entry.actor_id.clone();
    // The run entry is released while the browser runs (WS8.H).
    drop(entry);
    let step_started = std::time::Instant::now();
    let step_result = runner.execute(&req, &session, &mut lane.ctx).await;
    crate::metrics::record_agent_step(
        browser_action_name(&receipt_action),
        step_result.is_ok(),
        step_started.elapsed(),
    );
    if takeover_exempt {
        crate::metrics::record_takeover_exempt_step(step_started.elapsed());
    }
    let mut entry = handle.entry.lock().await;
    let step_result = step_result.and_then(|obs| check_landed_on_run(state, handle, &entry, obs));
    let observation = match step_result {
        Ok(observation) => {
            let cost_usd = receipt_cost_usd(&observation.telemetry, &receipt_action);
            if counts_budget {
                budget_charge(&mut entry, true, cost_usd);
            }
            if receipt_plan != super::human_browse::ReceiptPlan::Skip {
                let receipt = ReceiptBuilder::start(receipt_run_id.clone(), receipt_step)
                    .org_id(org_id.clone())
                    .actor_id(actor_id.clone())
                    .initiator(admitted.initiator, admitted.control_epoch)
                    .agent(admitted.agent.clone())
                    .typed_field(typed_kind)
                    .complete_maybe_metadata(
                        metadata_only,
                        receipt_action.clone(),
                        Some(observation.clone()),
                        cost_usd,
                    );
                state.receipts.append(receipt).await?;
            }
            observation
        }
        Err(error) => {
            if counts_budget {
                budget_charge(
                    &mut entry,
                    false,
                    estimated_action_cost_usd(&receipt_action),
                );
            }
            if receipt_plan != super::human_browse::ReceiptPlan::Skip {
                let receipt = ReceiptBuilder::start(receipt_run_id, receipt_step)
                    .org_id(org_id.clone())
                    .actor_id(actor_id.clone())
                    .initiator(admitted.initiator, admitted.control_epoch)
                    .agent(admitted.agent.clone())
                    .typed_field(typed_kind)
                    .fail_maybe_metadata(metadata_only, receipt_action, &error);
                state.receipts.append(receipt).await?;
            }
            return Err(error);
        }
    };
    let shared = Arc::new(observation.clone());
    lane.last_observation = Some(shared.clone());
    entry.summary.record(&lane.ctx, shared);
    if !entry.zdr.is_active() {
        state
            .receipts
            .save_run_checkpoint(checkpoint_for_entry(&entry))
            .await?;
    }
    Ok(observation)
}
