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
///
/// WS7.K: `layers` are the step layers the action was admitted under (the grant snapshot and
/// the step caller's claim), checked after the run's decision and only ever tightening it.
pub(super) fn check_landed_page(
    state: &AppState,
    live: &RunLive,
    zdr: bool,
    url: &str,
    layers: &crate::agent_policy_gate::StepLayers<'_>,
    domain_allows: impl FnOnce(&str) -> bool,
    outside: &'static str,
) -> QuarryResult<()> {
    if super::human_browse::is_blank_tab(url) {
        return Ok(());
    }
    if !domain_allows(url) {
        return Err(QuarryError::new(ErrorCode::SecurityBlocked, outside));
    }
    crate::agent_policy_gate::gate_visit_for_step(state, live, zdr, url, layers)
}

/// W3-E3.4-4: the flag that turns on the download admission policy. Default off: while it
/// is off the edge runs none of the policy below and `download_ref` keeps its earlier path
/// (the capability gate, the single-use sensitive approval, the runner's own checks).
const DOWNLOADS_FLAG: &str = "QUARRY_EDGE__DOWNLOADS";

/// Whether `QUARRY_EDGE__DOWNLOADS` is on in this process. Read per call, like
/// `QUARRY_EDGE__FILL_OTP`, so the flag needs no AppState plumbing.
pub(super) fn downloads_enabled() -> bool {
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
    admitted: &Admitted,
    obs: BrowserObservation,
) -> QuarryResult<BrowserObservation> {
    check_landed_page(
        state,
        &handle.live,
        entry.zdr.is_active(),
        &obs.url,
        &admitted.step_layers(),
        |url| {
            entry
                .nav_policy
                .allows(url, &entry.constraints.allowed_domains)
        },
        LANDED_OUTSIDE_RUN,
    )?;
    Ok(obs)
}

/// W3-E3.1-e: the process-wide credential-delivered marks. After a `fill_otp_ref` step
/// the page it ran on keeps its accessibility tree, DOM summary, screenshot and content
/// artifacts out of every observation (and so out of the stored receipt, which carries
/// `[redacted:otp]` in `policy_denials` instead), and a pure read of it is refused, until
/// a new document is loaded for that page. The only trigger, `fill_otp_ref`, is admitted
/// only while `QUARRY_EDGE__FILL_OTP` is on (default off); the denial itself is never off.
fn otp_reads() -> &'static quarry_runtime::observation_redaction::CredentialDeliveredReads {
    quarry_runtime::observation_redaction::credential_delivered_reads()
}

/// E4.2 step 1: admits a step's `expect`, or refuses it. While `QUARRY_EDGE__STEP_EXPECT` is off
/// (the default) a step that carries one is a 400 `unsupported_option`; otherwise its bounds are
/// checked (`StepExpectation::validate`). Called before the step key is reserved, so a refusal
/// takes none. Neither refusal carries the proposed text. Shared with the delegate step.
pub(super) fn admit_step_expect(
    expect: Option<quarry_core::step_expect::StepExpectation>,
) -> QuarryResult<Option<quarry_core::step_expect::StepExpectation>> {
    let Some(expect) = expect else {
        return Ok(None);
    };
    if !quarry_runtime::step_expect::step_expect_enabled() {
        return Err(QuarryError::new(
            ErrorCode::BadRequest,
            "step expectations are not enabled on this deployment",
        )
        .with_details(serde_json::json!({
            "reason": "unsupported_option",
            "option": "expect",
            "env": "QUARRY_EDGE__STEP_EXPECT",
        })));
    }
    expect.validate()?;
    Ok(Some(expect))
}

/// WS10.S4: the most `EgressDecided` events with `decision: "block"` one step emits. The
/// step's summary event counts every blocked request, so nothing is lost but the detail.
const MAX_EGRESS_BLOCK_EVENTS: usize = 20;

/// WS10.S4: the closed facts a step's run-trace events carry, read off its receipt before the
/// receipt store takes it. Labels, counts and opaque ids only: never typed text, a selector,
/// a URL or a failure message.
pub(super) struct StepTraceFacts {
    receipt_id: String,
    step: u32,
    /// The action's serde tag (`click_ref`, `navigate`, ...), never its arguments.
    action: &'static str,
    outcome: &'static str,
    error_code: Option<String>,
    cost_micro_usd: u64,
    initiated_by: Option<BrowserTimelineInitiator>,
    control_epoch: Option<u64>,
}

impl StepTraceFacts {
    pub(super) fn of(receipt: &StepReceipt) -> Self {
        let (outcome, error_code) = match &receipt.outcome {
            StepOutcome::Completed { .. } => ("completed", None),
            StepOutcome::Failed { error_code, .. } => ("failed", Some(error_code.clone())),
            StepOutcome::Skipped { .. } => ("skipped", None),
        };
        Self {
            receipt_id: receipt.receipt_id.clone(),
            step: receipt.step,
            action: browser_action_name(&receipt.action),
            outcome,
            error_code,
            cost_micro_usd: receipt.cost_micro_usd,
            initiated_by: receipt.initiated_by,
            control_epoch: receipt.control_epoch,
        }
    }
}

/// WS10.S4: `scheme://host[:port]` of an http(s) URL, or `None` (no path, query, fragment or
/// credentials ever ride on a trace event; the projection mapper reduces it again).
fn trace_url_origin(raw: &str) -> Option<String> {
    let parsed = Url::parse(raw.trim()).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let origin = parsed.origin();
    origin.is_tuple().then(|| origin.ascii_serialization())
}

/// A trace payload without its `null` keys, so an absent fact is absent rather than `null`.
fn without_nulls(mut value: serde_json::Value) -> serde_json::Value {
    if let Some(map) = value.as_object_mut() {
        map.retain(|_, v| !v.is_null());
    }
    value
}

/// WS10.S4: the run-trace events of one recorded step, emitted after its receipt was stored
/// (or, on a run that keeps none, built): `StepReceiptRecorded`, then for a step with an
/// observation one `EgressDecided` summary plus at most [`MAX_EGRESS_BLOCK_EVENTS`] blocked
/// requests, then one `ArtifactRecorded` per stored screenshot, visual observation and
/// evidence delta. Every event after the first names the receipt event as its cause. All go
/// through `emit_for_zdr`, so a ZDR run's events reach only live subscribers. The delegate
/// step of the attach lane (`attach::delegate_step`) emits through this as well (QF-32).
pub(super) async fn emit_step_trace(
    state: &AppState,
    zdr: ZdrMode,
    run_id: &RunKind,
    facts: StepTraceFacts,
    latency_ms: u64,
    observation: Option<&BrowserObservation>,
) {
    let sink = &state.event_sink;
    let step = facts.step;
    let verified = observation.is_some_and(|obs| {
        matches!(
            obs.action_outcome.status,
            quarry_core::contracts::ActionOutcomeStatus::Verified
        )
    });
    let receipt_event = sink
        .emit_for_zdr(
            zdr,
            run_id.clone(),
            EventType::StepReceiptRecorded,
            without_nulls(serde_json::json!({
                "browser_run_id": run_id.to_string(),
                "receipt_id": facts.receipt_id,
                "step": step,
                "action": facts.action,
                "outcome": facts.outcome,
                "latency_ms": latency_ms,
                "error_code": facts.error_code,
                "cost_micro_usd": facts.cost_micro_usd,
                "url_origin": observation.and_then(|obs| trace_url_origin(&obs.url)),
                "verified": verified,
                "initiated_by": facts.initiated_by,
                "control_epoch": facts.control_epoch,
            })),
            format!("{run_id}:step_receipt:{step}"),
        )
        .await;
    let Some(obs) = observation else {
        return;
    };
    let is_block = |receipt: &&BrowserEgressReceipt| {
        matches!(
            receipt.decision,
            quarry_core::contracts::BrowserEgressDecision::Block
        )
    };
    if !obs.egress_receipts.is_empty() {
        let sequences = || obs.egress_receipts.iter().map(|r| r.sequence);
        let from = sequences().min().unwrap_or(0);
        let to = sequences().max().unwrap_or(0);
        let blocked = obs.egress_receipts.iter().filter(is_block).count();
        let allowed = obs.egress_receipts.len() - blocked;
        sink.emit_for_zdr_caused(
            zdr,
            run_id.clone(),
            EventType::EgressDecided,
            serde_json::json!({
                "decision": "summary",
                "step": step,
                "egress_seq_from": from,
                "egress_seq_to": to,
                "allowed": allowed,
                "blocked": blocked,
            }),
            format!("{run_id}:egress:{step}:summary"),
            receipt_event.clone(),
        )
        .await;
        for receipt in obs
            .egress_receipts
            .iter()
            .filter(is_block)
            .take(MAX_EGRESS_BLOCK_EVENTS)
        {
            sink.emit_for_zdr_caused(
                zdr,
                run_id.clone(),
                EventType::EgressDecided,
                without_nulls(serde_json::json!({
                    "decision": "block",
                    "step": step,
                    "policy": receipt.policy,
                    "origin": trace_url_origin(&receipt.url),
                    "tab_id": receipt.tab_id,
                })),
                format!("{run_id}:egress:{step}:block:{}", receipt.sequence),
                receipt_event.clone(),
            )
            .await;
        }
    }
    for (kind, artifact) in [
        ("screenshot", obs.screenshot_artifact_id.as_ref()),
        (
            "visual_observation",
            obs.visual_observation_artifact_id.as_ref(),
        ),
        ("evidence_delta", obs.evidence_delta_artifact_id.as_ref()),
    ] {
        let Some(artifact) = artifact else {
            continue;
        };
        sink.emit_for_zdr_caused(
            zdr,
            run_id.clone(),
            EventType::ArtifactRecorded,
            serde_json::json!({
                "step": step,
                "artifact_id": artifact.to_string(),
                "kind": kind,
            }),
            format!("{run_id}:artifact:{step}:{kind}"),
            receipt_event.clone(),
        )
        .await;
    }
}

/// The step's wall time in whole ms, for its `StepReceiptRecorded` event.
pub(super) fn elapsed_ms(elapsed: Duration) -> u64 {
    u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
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
        return super::attach::delegate_step(
            &state,
            &handle,
            &claims,
            &request_id,
            body,
            quarry_runtime::otp_source::fill_otp_enabled(),
        )
        .await;
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

    // Q3-E3.2-e1w: a person's origin extension is read again before every step. One that is no
    // longer usable is dropped and the lane goes back to the run's own egress boundary, so an
    // extension never outlives its grant. Nothing to do (and no broker call) without one.
    super::origin_extension::revalidate_origin_extension(
        &state,
        &handle,
        &mut entry,
        Some(&mut lane.ctx),
    )
    .await
    .map_err(|error| driver_err(&request_id, error))?;

    // Every gate, in one fixed order (see `admit_action`).
    let admitted = admit_action(
        &state,
        &handle,
        &mut entry,
        AdmitInput {
            initiator: derive_initiator_ref(&claims, body.initiated_by, &body.advisory_agent()),
            action: &body.action,
            enforce_takeover: crate::run_limits::takeover_enforced(),
            consented_tab_control: false,
            last_observation: lane.last_observation.as_deref(),
            caller: Some(&claims),
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
    // W3-E3.1-e: a pure read of a page a one-time code was delivered into is refused.
    let otp_run = entry.run_id.to_string();
    let otp_before_url = lane.last_observation.as_deref().map(|obs| obs.url.clone());
    otp_reads()
        .check_before(&otp_run, &body.action, otp_before_url.as_deref())
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

    // E4.2 step 1: the postcondition is taken off the body after its last whole-body read and
    // before `body.action` moves into the request. Admitted before the step key is reserved.
    let expect = body.expect;
    let expect = admit_step_expect(expect).map_err(|error| driver_err(&request_id, error))?;
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
    // E4.2 step 1: read once by the `execute` that follows, which takes it first thing.
    lane.ctx.pending_expect = expect;
    let receipt_run_id = entry.run_id.to_string();
    let receipt_action = req.action.clone();
    // WS12.S3: the kind of the control a typing action goes to, read from the snapshot the
    // action names. `execute` replaces `ctx.active_snapshot`, so this is read before it runs.
    let typed_kind = quarry_runtime::observation::receipt_field_kind(&lane.ctx, &req.action);
    // Q3-E3.1-h2: a `fill_otp_ref` step reads its code for the approval grant's mail connection
    // and origin, for the run's actor. Set on every step, so a binding never outlives its step.
    lane.ctx.otp_binding = admitted
        .otp_binding
        .clone()
        .map(|binding| binding.for_run_actor(&entry.actor_id));

    let runner = ObservationRunner {
        otp_source: state.otp_source.clone(),
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
    // W3-E3.1-e: a delivered code marks its page (even when the fill failed), and the
    // observation of a marked page has its page reads withheld before the landing check,
    // the receipt and the checkpoint see it.
    let step_result = otp_reads().settle(
        &otp_run,
        &receipt_action,
        otp_before_url.as_deref(),
        step_result,
    );
    let step_elapsed = step_started.elapsed();
    if takeover_exempt {
        crate::metrics::record_takeover_exempt_step(step_elapsed);
    }
    let mut entry = handle.entry.lock().await;
    // The landed page is checked before the receipt and the checkpoint (T4b-1): a page the
    // caller is refused is a failed step, and its observation is not kept. The action did
    // run, so it keeps the driver's cost.
    let ran = step_result.as_ref().ok().map(|obs| obs.telemetry.clone());
    let step_result = step_result.and_then(|obs| check_landed_on_run(&state, &handle, &entry, &admitted, obs));
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
            let trace_facts = StepTraceFacts::of(&receipt);
            if receipt_plan != super::human_browse::ReceiptPlan::Skip {
                state
                    .receipts
                    .append(receipt)
                    .await
                    .map_err(|e| driver_err(&request_id, e))?;
            }
            // WS10.S4: the step's trace events, once its receipt is recorded.
            emit_step_trace(
                &state,
                entry.zdr,
                &req.run_id,
                trace_facts,
                elapsed_ms(step_elapsed),
                Some(&obs),
            )
            .await;
            (obs, cost_source)
        }
        Err(error) => {
            // A failed attempt is charged the flat cost, so its receipt carries it too (WS6.B);
            // a refused landing its driver's cost, and a navigate the driver refused before
            // a page loaded the refusal rate (`failed_step_cost_for_error`, W2-U21a).
            let (failed_usd, failed_source) =
                failed_step_cost_for_error(ran.as_ref(), &receipt_action, &error);
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
            let trace_facts = StepTraceFacts::of(&receipt);
            let appended = if receipt_plan != super::human_browse::ReceiptPlan::Skip {
                state.receipts.append(receipt).await
            } else {
                Ok(())
            };
            // D2: the spent key is saved whether or not its receipt was kept.
            checkpoint_failed_step(&state, &entry, &handle.live).await;
            appended.map_err(|e| driver_err(&request_id, e))?;
            // WS10.S4: a failed step's receipt event (no observation, so no egress or artifact).
            emit_step_trace(
                &state,
                entry.zdr,
                &req.run_id,
                trace_facts,
                elapsed_ms(step_elapsed),
                None,
            )
            .await;
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
    // Q3-E3.2-e1w: as in `step`, the origin extension is read again before the action.
    super::origin_extension::revalidate_origin_extension(
        state,
        handle,
        &mut entry,
        Some(&mut lane.ctx),
    )
    .await?;
    let admitted = admit_action(
        state,
        handle,
        &mut entry,
        AdmitInput {
            initiator: derive_initiator_ref(claims, hint, &AdvisoryAgent::default()),
            action: &action,
            enforce_takeover: crate::run_limits::takeover_enforced(),
            consented_tab_control: false,
            last_observation: lane.last_observation.as_deref(),
            caller: Some(claims),
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
    // W3-E3.1-e: as in `step`.
    let otp_run = entry.run_id.to_string();
    let otp_before_url = lane.last_observation.as_deref().map(|obs| obs.url.clone());
    otp_reads().check_before(&otp_run, &action, otp_before_url.as_deref())?;
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
    // Q3-E3.1-h2: as in `step`.
    lane.ctx.otp_binding = admitted
        .otp_binding
        .clone()
        .map(|binding| binding.for_run_actor(&entry.actor_id));
    let runner = ObservationRunner {
        otp_source: state.otp_source.clone(),
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
    // W3-E3.1-e: as in `step`.
    let step_result = otp_reads().settle(
        &otp_run,
        &receipt_action,
        otp_before_url.as_deref(),
        step_result,
    );
    let step_elapsed = step_started.elapsed();
    if takeover_exempt {
        crate::metrics::record_takeover_exempt_step(step_elapsed);
    }
    let mut entry = handle.entry.lock().await;
    let ran = step_result.as_ref().ok().map(|obs| obs.telemetry.clone());
    let step_result = step_result.and_then(|obs| check_landed_on_run(state, handle, &entry, &admitted, obs));
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
            // WS10.S4: the receipt is built on every path (its facts feed the trace events)
            // and stored only when the plan keeps one.
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
            let trace_facts = StepTraceFacts::of(&receipt);
            if receipt_plan != super::human_browse::ReceiptPlan::Skip {
                state.receipts.append(receipt).await?;
            }
            emit_step_trace(
                state,
                entry.zdr,
                &req.run_id,
                trace_facts,
                elapsed_ms(step_elapsed),
                Some(&observation),
            )
            .await;
            observation
        }
        Err(error) => {
            if counts_budget {
                budget_charge(
                    &mut entry,
                    false,
                    failed_step_cost_for_error(ran.as_ref(), &receipt_action, &error).0,
                );
            }
            lane.ctx.last_egress_sequence = egress_cursor;
            let mut appended = Ok(());
            let receipt = ReceiptBuilder::start(receipt_run_id, receipt_step)
                .org_id(org_id.clone())
                .actor_id(actor_id.clone())
                .initiator(admitted.initiator, admitted.control_epoch)
                .agent(admitted.agent.clone())
                .typed_field(typed_kind)
                .fail_maybe_metadata(metadata_only, receipt_action, &error);
            let trace_facts = StepTraceFacts::of(&receipt);
            if receipt_plan != super::human_browse::ReceiptPlan::Skip {
                appended = state.receipts.append(receipt).await;
            }
            // D2: the spent key is saved whether or not its receipt was kept.
            checkpoint_failed_step(state, &entry, &handle.live).await;
            appended?;
            // WS10.S4: as in `step`.
            emit_step_trace(
                state,
                entry.zdr,
                &req.run_id,
                trace_facts,
                elapsed_ms(step_elapsed),
                None,
            )
            .await;
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

#[cfg(test)]
mod otp_read_denial_tests {
    use super::*;

    const PAGE: &str = "https://login.example.com/verify";

    fn fill() -> AgentAction {
        AgentAction::FillOtpRef {
            snapshot_id: "s1".into(),
            generation: 1,
            ref_id: "e1".into(),
            approval_grant_id: "g1".into(),
        }
    }

    #[test]
    fn the_edge_uses_one_process_wide_set_of_marks() {
        assert!(std::ptr::eq(otp_reads(), otp_reads()));
    }

    #[test]
    fn a_failed_fill_marks_the_page_and_a_screenshot_of_it_is_refused() {
        let run = format!("run_e31e_{}", std::process::id());
        let failed: QuarryResult<BrowserObservation> =
            Err(QuarryError::new(ErrorCode::DriverFailed, "insert failed"));
        assert!(otp_reads().settle(&run, &fill(), Some(PAGE), failed).is_err());
        let shot = AgentAction::Screenshot { full_page: false };
        let err = otp_reads()
            .check_before(&run, &shot, Some(PAGE))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden);
        // An ordinary action on the page still runs; a released run is forgotten.
        let click = AgentAction::Click { selector: "#next".into() };
        assert!(otp_reads().check_before(&run, &click, Some(PAGE)).is_ok());
        otp_reads().release(&run);
        assert!(otp_reads().check_before(&run, &shot, Some(PAGE)).is_ok());
    }
}
