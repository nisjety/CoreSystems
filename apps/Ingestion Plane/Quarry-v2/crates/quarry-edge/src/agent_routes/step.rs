use super::*;

use quarry_runtime::action_cost::CostSource;

/// `POST /v1/agent/runs/{run_id}/step` — execute one action, return the
/// resulting observation. `ObservationRunner` advances `ctx.step` and emits
/// `ActionStarted`/`ObservationReady` itself.
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

    // Counts as in flight from before it waits for the run lock, so the sweeper
    // never releases a run an action is queued on.
    let _in_flight = handle.live.in_flight_guard();
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
    // The run was closed while this step waited for its lock.
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
        },
    )
    .await
    .map_err(|error| driver_err(&request_id, error))?;

    let req = AgentActionRequest {
        run_id: entry.run_id.clone(),
        lease_id: entry.lease.lease_id.clone(),
        action: body.action,
        instruction: body.instruction,
        constraints: entry.constraints.clone(),
        zdr: entry.zdr,
        extraction_profile: body.extraction_profile,
    };
    let receipt_step = entry.ctx.step;
    let receipt_run_id = entry.run_id.to_string();
    let receipt_action = req.action.clone();

    let runner = ObservationRunner {
        browser: state.agent_driver.clone(),
        artifacts: Some(state.artifacts.clone()),
        events: Some(state.event_sink.clone()),
        visual_processor: state.visual_processor.clone(),
        // The run's org (already checked against `claims.org_id` above) is
        // the tenant every observation artifact is stamped with.
        org_id: entry.org_id.clone(),
    };

    // Disjoint borrows: &session (shared) + &mut ctx (exclusive).
    let entry_mut: &mut RunEntry = &mut entry;
    // A completed browser call is not automatically a verified business
    // effect; the observation carries that distinction. Persist an
    // immutable receipt for both success and failure so a planner or
    // reviewer can replay the exact action sequence.
    let (obs, cost_source) = match runner
        .execute(&req, &entry_mut.session, &mut entry_mut.ctx)
        .await
    {
        Ok(obs) => {
            // W4: when the driver omits a provider-billed cost, fall
            // back to the flat action-cost table so max_cost_usd
            // budgets are enforceable.
            let cost_usd = receipt_cost_usd(&obs.telemetry, &receipt_action);
            let cost_source = receipt_cost_source(&obs.telemetry);
            budget_charge(&mut entry, true, cost_usd);
            let receipt = ReceiptBuilder::start(receipt_run_id.clone(), receipt_step)
                .org_id(entry.org_id.clone())
                .actor_id(entry.actor_id.clone())
                .initiator(admitted.initiator, admitted.control_epoch)
                .agent(admitted.agent.clone())
                .cost_source(cost_source)
                .complete(receipt_action, Some(obs.clone()), cost_usd);
            if !entry.zdr.is_active() {
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
            budget_charge(&mut entry, false, flat_usd);
            let receipt = ReceiptBuilder::start(receipt_run_id, receipt_step)
                .org_id(entry.org_id.clone())
                .actor_id(entry.actor_id.clone())
                .initiator(admitted.initiator, admitted.control_epoch)
                .agent(admitted.agent.clone())
                .failed_cost(flat_usd)
                .cost_source(CostSource::FlatTable)
                .fail(receipt_action, &error);
            if !entry.zdr.is_active() {
                state
                    .receipts
                    .append(receipt)
                    .await
                    .map_err(|e| driver_err(&request_id, e))?;
            }
            return Err(driver_err(&request_id, error));
        }
    };

    entry.last_observation = Some(obs.clone());

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

    if !domain_allowed(&obs.url, &entry.constraints.allowed_domains) {
        return Err(status_err(
            StatusCode::FORBIDDEN,
            &request_id,
            "browser action reached a domain outside the run's allowed domains",
        ));
    }

    // WS7.D: the page the step landed on is held to the run's host rules too. The action has
    // already run; the refusal is the same typed 403 the pre-action gate gives.
    crate::agent_policy_gate::gate_visit(&state, &handle.live, entry.zdr.is_active(), &obs.url)
        .map_err(|error| driver_err(&request_id, error))?;
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
        },
    )
    .await?;
    let req = AgentActionRequest {
        run_id: entry.run_id.clone(),
        lease_id: entry.lease.lease_id.clone(),
        action,
        instruction,
        constraints: entry.constraints.clone(),
        zdr: entry.zdr,
        extraction_profile: None,
    };
    let receipt_step = entry.ctx.step;
    let receipt_action = req.action.clone();
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
    let entry_mut: &mut RunEntry = &mut entry;
    let observation = match runner
        .execute(&req, &entry_mut.session, &mut entry_mut.ctx)
        .await
    {
        Ok(observation) => {
            let cost_usd = receipt_cost_usd(&observation.telemetry, &receipt_action);
            budget_charge(&mut entry, true, cost_usd);
            if !entry.zdr.is_active() {
                let receipt = ReceiptBuilder::start(receipt_run_id.clone(), receipt_step)
                    .org_id(entry.org_id.clone())
                    .actor_id(entry.actor_id.clone())
                    .initiator(admitted.initiator, admitted.control_epoch)
                    .agent(admitted.agent.clone())
                    .complete(receipt_action.clone(), Some(observation.clone()), cost_usd);
                state.receipts.append(receipt).await?;
            }
            observation
        }
        Err(error) => {
            budget_charge(
                &mut entry,
                false,
                estimated_action_cost_usd(&receipt_action),
            );
            if !entry.zdr.is_active() {
                let receipt = ReceiptBuilder::start(receipt_run_id, receipt_step)
                    .org_id(entry.org_id.clone())
                    .actor_id(entry.actor_id.clone())
                    .initiator(admitted.initiator, admitted.control_epoch)
                    .agent(admitted.agent.clone())
                    .fail(receipt_action, &error);
                state.receipts.append(receipt).await?;
            }
            return Err(error);
        }
    };
    entry.last_observation = Some(observation.clone());
    if !entry.zdr.is_active() {
        state
            .receipts
            .save_run_checkpoint(checkpoint_for_entry(&entry))
            .await?;
    }
    if !domain_allowed(&observation.url, &entry.constraints.allowed_domains) {
        return Err(QuarryError::new(
            ErrorCode::SecurityBlocked,
            "browser action reached a domain outside the run's allowed domains",
        ));
    }
    crate::agent_policy_gate::gate_visit(state, &handle.live, entry.zdr.is_active(), &observation.url)?;
    Ok(observation)
}
