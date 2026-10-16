use super::*;

/// `POST /v1/agent/runs/{run_id}/step` — execute one action, return the
/// resulting observation. `ObservationRunner` advances `ctx.step` and emits
/// `ActionStarted`/`ObservationReady` itself.
pub async fn step(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(run_id): Path<String>,
    Json(body): Json<StepBody>,
) -> Result<Json<Envelope<BrowserObservation>>, ApiErr> {
    let request_id = RequestKind::new().to_string();

    let entry_arc = {
        let map = state.agent_runs.lock().expect("agent_runs mutex poisoned");
        map.get(&run_id).cloned()
    };
    let Some(entry_arc) = entry_arc else {
        return Err(status_err(
            StatusCode::NOT_FOUND,
            &request_id,
            "agent run not found",
        ));
    };

    let mut entry = entry_arc.lock().await;
    if !caller_owns_live_run(&entry, &claims) {
        return Err(status_err(
            StatusCode::NOT_FOUND,
            &request_id,
            "agent run not found",
        ));
    }

    if let Some(grant_id) = entry.grant_id.as_deref() {
        validate_run_grant(&state, grant_id, &entry.constraints.allowed_domains)
            .await
            .map_err(|error| driver_err(&request_id, error))?;
    } else if state.require_browser_grants {
        return Err(status_err(
            StatusCode::FORBIDDEN,
            &request_id,
            "browser grant is required for every action",
        ));
    }

    deny_raw_script(&body.action, &entry.run_id.to_string())
        .map_err(|error| driver_err(&request_id, error))?;
    require_action_capabilities(&body.action, state.agent_driver.capabilities())
        .map_err(|error| driver_err(&request_id, error))?;
    if entry.execution_tier.is_ephemeral_evidence() {
        require_ephemeral_evidence_action(&body.action)
            .map_err(|error| driver_err(&request_id, error))?;
    }

    // The budgets refuse before a one-shot approval is consumed: an action
    // that was never going to run must not burn the approval that was
    // issued for it.
    enforce_step_budget(&entry)
        .map_err(|message| status_err(StatusCode::TOO_MANY_REQUESTS, &request_id, message))?;
    budget_gate(&mut entry, &body.action).map_err(|error| driver_err(&request_id, error))?;

    if let Some(approval) = sensitive_approval_for_action(&entry, &body.action)
        .map_err(|error| driver_err(&request_id, error))?
    {
        validate_sensitive_approval(&state, &approval)
            .await
            .map_err(|error| driver_err(&request_id, error))?;
        if !entry
            .used_sensitive_approval_grants
            .insert(approval.grant_id)
        {
            return Err(status_err(
                StatusCode::CONFLICT,
                &request_id,
                "sensitive browser approval grant has already been used by this run",
            ));
        }
    }

    if let AgentAction::Navigate { url } = &body.action {
        if !domain_allowed(url, &entry.constraints.allowed_domains) {
            return Err(status_err(
                StatusCode::FORBIDDEN,
                &request_id,
                "browser navigation is outside the run's allowed domains",
            ));
        }
    }

    validate_navigation(&body.action, state.security.as_ref())
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
    let obs = match runner
        .execute(&req, &entry_mut.session, &mut entry_mut.ctx)
        .await
    {
        Ok(obs) => {
            // W4: when the driver omits a provider-billed cost, fall
            // back to the flat action-cost table so max_cost_usd
            // budgets are enforceable.
            let cost_usd = receipt_cost_usd(&obs.telemetry, &receipt_action);
            budget_charge(&mut entry, true, cost_usd);
            let receipt = ReceiptBuilder::start(receipt_run_id.clone(), receipt_step)
                .org_id(entry.org_id.clone())
                .actor_id(entry.actor_id.clone())
                .complete(receipt_action, Some(obs.clone()), cost_usd);
            if !entry.zdr.is_active() {
                state
                    .receipts
                    .append(receipt)
                    .await
                    .map_err(|e| driver_err(&request_id, e))?;
            }
            obs
        }
        Err(error) => {
            budget_charge(
                &mut entry,
                false,
                estimated_action_cost_usd(&receipt_action),
            );
            let receipt = ReceiptBuilder::start(receipt_run_id, receipt_step)
                .org_id(entry.org_id.clone())
                .actor_id(entry.actor_id.clone())
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

    Ok(Json(Envelope::ok(request_id, obs)))
}

pub(super) async fn execute_ws_action(
    state: &AppState,
    entry_arc: &Arc<TokioMutex<RunEntry>>,
    action: AgentAction,
    instruction: Option<String>,
) -> QuarryResult<BrowserObservation> {
    validate_navigation(&action, state.security.as_ref()).await?;
    let mut entry = entry_arc.lock().await;
    if let Some(grant_id) = entry.grant_id.as_deref() {
        validate_run_grant(state, grant_id, &entry.constraints.allowed_domains).await?;
    } else if state.require_browser_grants {
        return Err(QuarryError::new(
            ErrorCode::Forbidden,
            "browser grant is required for every action",
        ));
    }
    deny_raw_script(&action, &entry.run_id.to_string())?;
    require_action_capabilities(&action, state.agent_driver.capabilities())?;
    if entry.execution_tier.is_ephemeral_evidence() {
        require_ephemeral_evidence_action(&action)?;
    }
    // Budgets first, so a refusal cannot burn a one-shot approval.
    enforce_step_budget(&entry)
        .map_err(|message| QuarryError::new(ErrorCode::RateLimited, message))?;
    budget_gate(&mut entry, &action)?;
    if let Some(approval) = sensitive_approval_for_action(&entry, &action)? {
        validate_sensitive_approval(state, &approval).await?;
        if !entry
            .used_sensitive_approval_grants
            .insert(approval.grant_id)
        {
            return Err(QuarryError::new(
                ErrorCode::Conflict,
                "sensitive browser approval grant has already been used by this run",
            ));
        }
    }
    if let AgentAction::Navigate { url } = &action {
        if !domain_allowed(url, &entry.constraints.allowed_domains) {
            return Err(QuarryError::new(
                ErrorCode::SecurityBlocked,
                "browser navigation is outside the run's allowed domains",
            ));
        }
    }
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
    Ok(observation)
}
