use super::*;

pub(super) fn checkpoint_for_entry(entry: &RunEntry) -> AgentRunCheckpoint {
    AgentRunCheckpoint {
        org_id: entry.org_id.clone(),
        actor_id: entry.actor_id.clone(),
        run_id: entry.run_id.to_string(),
        lease_id: entry.lease.lease_id.to_string(),
        profile_id: entry.lease.profile_id.to_string(),
        step: entry.ctx.step,
        current_url: entry.ctx.current_url.clone(),
        page_hash: entry.ctx.page_hash.clone(),
        constraints: entry.constraints.clone(),
        persist_profile: entry.lease.persist_profile,
        profile_scope: entry.profile_scope,
        viewport: entry.lease.viewport,
        zdr: entry.zdr,
        grant_id: entry.grant_id.clone(),
        status: AgentRunStatus::Active,
        control_mode: entry.control_mode,
        control_epoch: entry.control_epoch,
        budget: entry
            .budget
            .as_ref()
            .map(|budget| budget.snapshot(std::time::Instant::now())),
    }
}

pub(super) fn caller_owns_browser_run(
    run_org_id: &str,
    run_actor_id: &str,
    claims: &crate::auth::Claims,
) -> bool {
    run_org_id == claims.org_id
        && !run_actor_id.trim().is_empty()
        && run_actor_id == claims.actor_id()
}

pub(super) fn caller_owns_live_run(entry: &RunEntry, claims: &crate::auth::Claims) -> bool {
    caller_owns_browser_run(&entry.org_id, &entry.actor_id, claims)
}

pub(super) async fn record_browser_timeline_event(
    state: &AppState,
    entry: &RunEntry,
    event: BrowserTimelineEventKind,
) -> QuarryResult<()> {
    if entry.zdr.is_active() {
        return Ok(());
    }
    state
        .receipts
        .append_browser_timeline_event(BrowserTimelineEvent::new(
            entry.org_id.clone(),
            entry.actor_id.clone(),
            entry.run_id.to_string(),
            event,
        ))
        .await?;
    state
        .receipts
        .save_run_checkpoint(checkpoint_for_entry(entry))
        .await
}

/// `POST /v1/agent/runs/{run_id}/browser-session/control` — transfer
/// browser input authority. Quarry owns this transition, stamps it with
/// the verified actor, and appends a privacy-bounded audit event. ZDR
/// runs may be controlled live but never receive durable history.
pub async fn set_browser_session_control(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(run_id): Path<String>,
    Json(body): Json<BrowserSessionControlBody>,
) -> Result<Json<Envelope<BrowserSessionProjectionData>>, ApiErr> {
    let request_id = RequestKind::new().to_string();
    let Some(handle) = lookup_run(&state, &run_id) else {
        return Err(status_err(
            StatusCode::NOT_FOUND,
            &request_id,
            "agent run not found",
        ));
    };
    // Ownership and the flip itself need no run lock: a person taking over must
    // not queue behind an action that is still running.
    if !handle.live.owned_by(&claims) {
        return Err(status_err(
            StatusCode::NOT_FOUND,
            &request_id,
            "agent run not found",
        ));
    }
    handle.live.touch();
    // Only a person hands the browser over or back. An agent (a service
    // principal, or a person's token labelled agent) cannot take it from them.
    if derive_initiator(&claims, None) != StepInitiator::Human {
        return Err(driver_err(
            &request_id,
            QuarryError::new(ErrorCode::Forbidden, "control_transition_requires_human"),
        ));
    }
    let (changed, epoch) = handle
        .live
        .set_mode(body.mode, body.expected_epoch)
        .map_err(|current| {
            driver_err(
                &request_id,
                QuarryError::new(ErrorCode::Conflict, "the browser's control epoch has moved on")
                    .with_details(serde_json::json!({
                        "control_epoch": current,
                        "expected_epoch": body.expected_epoch,
                    })),
            )
        })?;

    let mut entry = handle.entry.lock().await;
    entry.control_mode = body.mode;
    entry.control_epoch = epoch;
    if let Some(budget) = entry.budget.as_mut() {
        let now = std::time::Instant::now();
        match body.mode {
            BrowserControlMode::HumanTakeover => budget.ledger.pause(now),
            BrowserControlMode::AgentControl => budget.ledger.resume(now),
        }
    }
    if changed {
        record_browser_timeline_event(
            &state,
            &entry,
            BrowserTimelineEventKind::Control {
                mode: body.mode,
                initiated_by: BrowserTimelineInitiator::Human,
                control_epoch: epoch,
            },
        )
        .await
        .map_err(|error| driver_err(&request_id, error))?;
    }

    let tabs = state
        .agent_driver
        .list_tabs(&entry.session)
        .await
        .map_err(|error| driver_err(&request_id, error))?;
    prune_tab_openers(&mut entry, &tabs);
    Ok(Json(Envelope::ok(
        request_id,
        live_browser_session_projection(&entry, tabs),
    )))
}
