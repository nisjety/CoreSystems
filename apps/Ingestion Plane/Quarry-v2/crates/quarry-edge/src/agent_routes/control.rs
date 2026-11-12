use super::*;

/// The run's durable continuation. `live` gives the step key the run hands out next (D2):
/// every save carries it, so a resume never reuses a key an earlier segment used.
pub(super) fn checkpoint_for_entry(entry: &RunEntry, live: &RunLive) -> AgentRunCheckpoint {
    AgentRunCheckpoint {
        org_id: entry.org_id.clone(),
        actor_id: entry.actor_id.clone(),
        run_id: entry.run_id.to_string(),
        lease_id: entry.lease.lease_id.to_string(),
        profile_id: entry.lease.profile_id.to_string(),
        step: entry.summary.step,
        // WS12.S3: whatever the run's navigation policy keeps of the URL, a secret query value,
        // fragment value or userinfo in it never reaches the checkpoint.
        current_url: quarry_core::redaction::redact_url(
            &super::human_browse::checkpoint_url(&entry.nav_policy, &entry.summary.current_url),
            quarry_core::redaction::Profile::Durable,
            &mut quarry_core::redaction::Tally::new(),
        )
        .into_owned(),
        policy_mode: Some(entry.nav_policy.mode_name().to_owned()),
        page_hash: entry.summary.page_hash.clone(),
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
        next_step: live.next_step(),
        trace_seq: live.trace_seq(),
        policy_ask: live.policy_ask().cloned(),
        // WS10.S4: trace labels a resume restores; never authority.
        origin: Some(entry.origin.as_str().to_owned()),
        model_run_id: entry.model_run_id.clone(),
    }
}

/// D2: the checkpoint a failed step leaves, with the caller holding the run entry. The step's
/// key is spent (its events and proof id used it) even when its receipt is not kept (a
/// person's step under `HUMAN_RECEIPT_MODE=none`, or a failed receipt write), so the next
/// key is saved here, with the failed charge in the budget snapshot. Nothing else changes: a
/// failed step records no observation. Not for a ZDR run (it keeps no checkpoint) or a
/// closed one (an `active` save would reopen it). Errors are logged, never returned: the
/// caller returns the step's own error.
pub(super) async fn checkpoint_failed_step(state: &AppState, entry: &RunEntry, live: &RunLive) {
    if entry.zdr.is_active() || entry.closed {
        return;
    }
    if let Err(error) = state
        .receipts
        .save_run_checkpoint(checkpoint_for_entry(entry, live))
        .await
    {
        tracing::warn!(
            error_class = crate::zdr_log::class(&error),
            "could not checkpoint a failed step"
        );
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
    live: &RunLive,
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
        .save_run_checkpoint(checkpoint_for_entry(entry, live))
        .await
}

/// WS10.S4: [`record_browser_timeline_event`], then the run-trace event of that change: a
/// `Control` change emits `BrowserControlChanged {mode, initiated_by, control_epoch}` and a
/// `Tab` change `BrowserTabChanged {operation, tab_id, active_tab_id}`. Other kinds record the
/// timeline only. The event goes through `emit_for_zdr`, so a ZDR run's change reaches only
/// live subscribers (its timeline is not written either). Nothing is emitted when the record
/// fails, or while `QUARRY_EDGE__STEP_TRACE` is off. Payloads are closed labels and opaque ids:
/// no URL, title, selector or typed text.
pub(super) async fn record_timeline_and_trace(
    state: &AppState,
    entry: &RunEntry,
    live: &RunLive,
    event: BrowserTimelineEventKind,
) -> QuarryResult<()> {
    let trace = browser_trace_event(&event);
    record_browser_timeline_event(state, entry, live, event).await?;
    // Dark like the step trace: the timeline row is written either way.
    if let Some((event_type, payload, key)) = trace.filter(|_| super::step::step_trace_enabled()) {
        state
            .event_sink
            .emit_for_zdr(
                entry.zdr,
                entry.run_id.clone(),
                event_type,
                payload,
                format!("{}:{key}", entry.run_id),
            )
            .await;
    }
    Ok(())
}

/// WS10.S4: the trace event type, payload and idempotency-key suffix of a timeline change, or
/// `None` for a kind that has no browser trace event.
fn browser_trace_event(
    event: &BrowserTimelineEventKind,
) -> Option<(EventType, serde_json::Value, String)> {
    match event {
        BrowserTimelineEventKind::Control {
            mode,
            initiated_by,
            control_epoch,
        } => Some((
            EventType::BrowserControlChanged,
            serde_json::json!({
                "mode": mode,
                "initiated_by": initiated_by,
                "control_epoch": control_epoch,
            }),
            // One control transition per epoch.
            format!("browser_control:{control_epoch}"),
        )),
        BrowserTimelineEventKind::Tab {
            operation,
            tab_id,
            active_tab_id,
            ..
        } => Some((
            EventType::BrowserTabChanged,
            serde_json::json!({
                "operation": operation,
                "tab_id": tab_id,
                "active_tab_id": active_tab_id,
            }),
            // A tab can be selected again under the same epoch, so each change is its own key.
            format!("browser_tab:{}", uuid::Uuid::new_v4().simple()),
        )),
        _ => None,
    }
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
    let request_id = crate::request_id::current();
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
        record_timeline_and_trace(
            &state,
            &entry,
            &handle.live,
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
        live_browser_session_projection(
            &entry,
            tabs,
            Some(&handle.live),
            state.agent_driver.capabilities(),
        ),
    )))
}

#[cfg(test)]
mod control_trace_gate_tests {
    use super::*;

    #[tokio::test]
    async fn a_control_change_traces_nothing_while_the_step_trace_flag_is_off() {
        // No test in this suite sets QUARRY_EDGE__STEP_TRACE, so the switch reads off here.
        let state = crate::test_support::test_state(crate::test_support::StubDriver::ok());
        let claims = crate::test_support::claims_for_org("org");
        let handle = crate::test_support::insert_live_run(&state, &claims);
        let entry = handle.entry.lock().await;
        let mut events = state.event_sink.subscribe(&entry.run_id);
        record_timeline_and_trace(
            &state,
            &entry,
            &handle.live,
            BrowserTimelineEventKind::Control {
                mode: BrowserControlMode::AgentControl,
                initiated_by: BrowserTimelineInitiator::Human,
                control_epoch: 1,
            },
        )
        .await
        .unwrap();
        // The timeline row is written either way; only the trace event is held back.
        let timeline = state
            .receipts
            .list_browser_timeline_events(&entry.org_id, &entry.actor_id, &entry.run_id)
            .await
            .unwrap();
        assert!(timeline.iter().any(|row| matches!(
            &row.event,
            BrowserTimelineEventKind::Control { .. }
        )));
        assert!(
            events.try_recv().is_err(),
            "no browser.control trace event while the flag is off"
        );
    }
}
