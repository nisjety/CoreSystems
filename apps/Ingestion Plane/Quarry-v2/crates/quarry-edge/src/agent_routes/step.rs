use super::*;

/// WS8.H: the person's lane of a run. Reads the entry lock for one clone of the handle.
pub(super) async fn human_lane_of(handle: &RunHandle) -> LaneRef {
    handle.entry.lock().await.human_lane.clone()
}

/// The refusal of a step that landed outside the run's allowed domains.
const LANDED_OUTSIDE_RUN: &str =
    "browser action reached a domain outside the run's allowed domains";

/// The check of the page a step landed on, after the action ran: the host rule (the run's,
/// or the attachment's for a delegate) and then the run's web policy (WS7.D). Every step
/// path calls it before it records the step, so a refused step leaves a `failed` receipt,
/// no summary or checkpoint of that page, and its egress receipts for the next observation
/// (T4b-1). The runner has already moved the lane's context, emitted `ObservationReady` and
/// stored the screenshot by then.
///
/// A tab with no page yet (`about:blank` exactly, `human_browse::is_blank_tab`) passes: a
/// run's first navigation that never commits leaves its tab there, and the next observation
/// of it reached no host. That is all it is accepted as; as a navigation target it is still
/// refused by the edge (gate 11's `RunNavPolicy::allows`, the tab and attach routes, the
/// delegate navigate). The browser's `guard_navigation_target` admits it: it loads nothing.
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

/// W3-E3.4-4: the flag that turns on the download admission policy. Default off: while it
/// is off the edge runs none of the policy below and `download_ref` keeps its earlier path
/// (the capability gate, the single-use sensitive approval, the runner's own checks).
const DOWNLOADS_FLAG: &str = "QUARRY_EDGE__DOWNLOADS";

/// Whether `QUARRY_EDGE__DOWNLOADS` is on in this process. Read per call, like
/// `QUARRY_EDGE__FILL_OTP`, so the flag needs no AppState plumbing.
fn downloads_enabled() -> bool {
    std::env::var(DOWNLOADS_FLAG)
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn is_download_action(action: &AgentAction) -> bool {
    matches!(
        action,
        AgentAction::DownloadRef { .. } | AgentAction::FrameDownloadRef { .. }
    )
}

/// W3-E3.4-4, before the browser runs: a `download_ref` step (the only solicited
/// download; admission has already spent its single-use approval) is refused under ZDR
/// and when its session has spent its download byte cap. `Ok(true)` means the step is a
/// download the policy must settle after it ran ([`settle_download`]).
fn download_gate_before(
    enabled: bool,
    action: &AgentAction,
    zdr_active: bool,
    ledger: &quarry_runtime::action_runtime::downloads::SessionDownloadLedger,
    session: &str,
) -> QuarryResult<bool> {
    use quarry_runtime::action_runtime::downloads;
    if !enabled || !is_download_action(action) {
        return Ok(false);
    }
    downloads::admit_origin(downloads::DownloadOrigin::DownloadRef)?;
    downloads::admit_zdr(zdr_active)?;
    ledger.check_open(session)?;
    Ok(true)
}

/// W3-E3.4-4, after the browser ran: the type allow-list (PDF, text, Markdown, CSV; Office
/// refused) on the stored download's bytes, and the charge to the session's byte cap. A
/// refusal fails the step. The runner pushes the download's artifact id last in the proof
/// bundle; a backend that reports object keys must also name it a browser download.
async fn settle_download(
    state: &AppState,
    org_id: &str,
    session: &str,
    zdr_active: bool,
    obs: BrowserObservation,
) -> QuarryResult<BrowserObservation> {
    use quarry_runtime::action_runtime::downloads;
    let id = obs
        .proof_bundle
        .as_ref()
        .and_then(|proof| proof.artifact_ids.last().cloned())
        .ok_or_else(|| {
            QuarryError::new(
                ErrorCode::RuntimeNotReady,
                "download step produced no stored artifact to admit",
            )
        })?;
    let (bytes, key) = state.artifacts.get_with_key(org_id, &id).await?;
    if !downloads::is_download_artifact_key(&key) {
        return Err(QuarryError::new(
            ErrorCode::RuntimeNotReady,
            "download step's last artifact is not a browser download",
        ));
    }
    downloads::admit_download(
        downloads::session_ledger(),
        session,
        downloads::DownloadOrigin::DownloadRef,
        zdr_active,
        None,
        &bytes,
    )?;
    Ok(obs)
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
    // W3-E3.4-4: the download admission policy, while QUARRY_EDGE__DOWNLOADS is on.
    let download_gated = download_gate_before(
        downloads_enabled(),
        &body.action,
        entry.zdr.is_active(),
        quarry_runtime::action_runtime::downloads::session_ledger(),
        &entry.run_id.to_string(),
    )
    .map_err(|error| driver_err(&request_id, error))?;
    let download_zdr = entry.zdr.is_active();
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
    let egress_cursor = lane.ctx.last_egress_sequence;
    let step_result = runner.execute(&req, &session, &mut lane.ctx).await;
    // W3-E3.4-4: the stored download passes the type allow-list and the session cap, or
    // the step fails. Read without the run entry held.
    let step_result = match step_result {
        Ok(obs) if download_gated => {
            settle_download(&state, &org_id, &receipt_run_id, download_zdr, obs).await
        }
        other => other,
    };
    let step_elapsed = step_started.elapsed();
    if takeover_exempt {
        crate::metrics::record_takeover_exempt_step(step_elapsed);
    }
    let mut entry = handle.entry.lock().await;
    // The landed page is checked before the receipt and the checkpoint (T4b-1): a page the
    // caller is refused is a failed step, and its observation is not kept. The action did
    // run, so it keeps the driver's cost.
    let ran = step_result.as_ref().ok().map(|obs| obs.telemetry.clone());
    let step_result = step_result.and_then(|obs| check_landed_on_run(&state, &handle, &entry, obs));
    crate::metrics::record_agent_step(
        browser_action_name(&receipt_action),
        step_result.is_ok(),
        step_elapsed,
    );
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
            // A failed attempt is charged the flat cost, so its receipt carries it too (WS6.B);
            // a refused landing its driver's cost (`failed_step_cost`).
            let (failed_usd, failed_source) = failed_step_cost(ran.as_ref(), &receipt_action);
            if counts_budget {
                budget_charge(&mut entry, false, failed_usd);
            }
            // The egress receipts a refused landing read go out with the next observation,
            // as a failed observation's do.
            lane.ctx.last_egress_sequence = egress_cursor;
            let receipt = ReceiptBuilder::start(receipt_run_id, receipt_step)
                .org_id(org_id.clone())
                .actor_id(actor_id.clone())
                .initiator(admitted.initiator, admitted.control_epoch)
                .agent(admitted.agent.clone())
                .failed_cost(failed_usd)
                .cost_source(failed_source)
                .typed_field(typed_kind)
                .fail_maybe_metadata(metadata_only, receipt_action, &error);
            let appended = if receipt_plan != super::human_browse::ReceiptPlan::Skip {
                state.receipts.append(receipt).await
            } else {
                Ok(())
            };
            // D2: the spent key is saved whether or not its receipt was kept.
            checkpoint_failed_step(&state, &entry, &handle.live).await;
            appended.map_err(|e| driver_err(&request_id, e))?;
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
        let checkpoint = checkpoint_for_entry(&entry, &handle.live);
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
    // W3-E3.4-4: the download admission policy, while QUARRY_EDGE__DOWNLOADS is on.
    let download_gated = download_gate_before(
        downloads_enabled(),
        &action,
        entry.zdr.is_active(),
        quarry_runtime::action_runtime::downloads::session_ledger(),
        &entry.run_id.to_string(),
    )?;
    let download_zdr = entry.zdr.is_active();
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
    let egress_cursor = lane.ctx.last_egress_sequence;
    let step_result = runner.execute(&req, &session, &mut lane.ctx).await;
    // W3-E3.4-4: as in `step`.
    let step_result = match step_result {
        Ok(obs) if download_gated => {
            settle_download(state, &org_id, &receipt_run_id, download_zdr, obs).await
        }
        other => other,
    };
    let step_elapsed = step_started.elapsed();
    if takeover_exempt {
        crate::metrics::record_takeover_exempt_step(step_elapsed);
    }
    let mut entry = handle.entry.lock().await;
    let ran = step_result.as_ref().ok().map(|obs| obs.telemetry.clone());
    let step_result = step_result.and_then(|obs| check_landed_on_run(state, handle, &entry, obs));
    crate::metrics::record_agent_step(
        browser_action_name(&receipt_action),
        step_result.is_ok(),
        step_elapsed,
    );
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
                    failed_step_cost(ran.as_ref(), &receipt_action).0,
                );
            }
            lane.ctx.last_egress_sequence = egress_cursor;
            let mut appended = Ok(());
            if receipt_plan != super::human_browse::ReceiptPlan::Skip {
                let receipt = ReceiptBuilder::start(receipt_run_id, receipt_step)
                    .org_id(org_id.clone())
                    .actor_id(actor_id.clone())
                    .initiator(admitted.initiator, admitted.control_epoch)
                    .agent(admitted.agent.clone())
                    .typed_field(typed_kind)
                    .fail_maybe_metadata(metadata_only, receipt_action, &error);
                appended = state.receipts.append(receipt).await;
            }
            // D2: the spent key is saved whether or not its receipt was kept.
            checkpoint_failed_step(state, &entry, &handle.live).await;
            appended?;
            return Err(error);
        }
    };
    let shared = Arc::new(observation.clone());
    lane.last_observation = Some(shared.clone());
    entry.summary.record(&lane.ctx, shared);
    if !entry.zdr.is_active() {
        state
            .receipts
            .save_run_checkpoint(checkpoint_for_entry(&entry, &handle.live))
            .await?;
    }
    Ok(observation)
}

#[cfg(test)]
mod download_gate_tests {
    use super::*;
    use quarry_runtime::action_runtime::downloads::SessionDownloadLedger;

    fn download() -> AgentAction {
        AgentAction::DownloadRef {
            snapshot_id: "s1".into(),
            generation: 1,
            ref_id: "r1".into(),
            approval_grant_id: "g1".into(),
        }
    }

    fn ledger(max: u64) -> SessionDownloadLedger {
        SessionDownloadLedger::new(max, std::time::Duration::from_secs(60))
    }

    #[test]
    fn flag_off_runs_no_download_policy() {
        let l = ledger(0);
        // Even a ZDR run with a spent cap passes: the policy is off.
        assert!(!download_gate_before(false, &download(), true, &l, "run").unwrap());
    }

    #[test]
    fn other_actions_are_not_gated() {
        let l = ledger(0);
        let nav = AgentAction::Navigate { url: "https://example.com".into() };
        assert!(!download_gate_before(true, &nav, true, &l, "run").unwrap());
    }

    #[test]
    fn flag_on_gates_download_ref() {
        let l = ledger(10);
        assert!(download_gate_before(true, &download(), false, &l, "run").unwrap());
        let frame = AgentAction::FrameDownloadRef {
            snapshot_id: "s1".into(),
            generation: 1,
            frame_id: "f1".into(),
            ref_id: "r1".into(),
            approval_grant_id: "g1".into(),
        };
        assert!(download_gate_before(true, &frame, false, &l, "run").unwrap());
    }

    #[test]
    fn flag_on_refuses_zdr() {
        let l = ledger(10);
        let err = download_gate_before(true, &download(), true, &l, "run").unwrap_err();
        assert_eq!(err.code, ErrorCode::SecurityBlocked);
    }

    #[test]
    fn flag_on_refuses_a_session_past_its_cap() {
        let l = ledger(10);
        l.charge("run", 10).unwrap();
        let err = download_gate_before(true, &download(), false, &l, "run").unwrap_err();
        assert_eq!(err.code, ErrorCode::BudgetExhausted);
        // Another run is unaffected; a released run starts again.
        assert!(download_gate_before(true, &download(), false, &l, "other").unwrap());
        l.release("run");
        assert!(download_gate_before(true, &download(), false, &l, "run").unwrap());
    }
}
