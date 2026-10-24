//! WS8.F: the agent attachment to a person's live run.
//!
//! `POST /v1/agent/runs/{run_id}/attach` gives an agent a tab of the person's run:
//! a service principal, holding an attach grant (a child of the run's grant), opens
//! one agent tab in the background. The delegate then steps on that tab only
//! (`POST .../step` with `grant_id`), while the person keeps their own tabs. A
//! person's non-passive action on the agent tab takes it over (D8-7); the delegate's
//! next step is then refused with `USER_TOOK_OVER`. `DELETE .../attach/{attach_id}`
//! ends the attachment; the owner may also stop it. `POST .../tabs/{tab_id}/control`
//! is the person's hand-over (`holder`) and give-back.
//!
//! Every path is dark until `QUARRY_EDGE__ATTACH` is on (`run_limits::attach_enabled`).
//! ZDR runs refuse attach. The attachment is process-local (a restart ends it).

use super::*;

use quarry_browser::OpenTabOptions;
use quarry_runtime::action_cost::CostSource;
use quarry_runtime::GrantValidation;

/// The step and runtime bounds an attach request may ask for (`constraints`).
pub const ATTACH_DEFAULT_STEPS: u32 = 20;
pub const ATTACH_MAX_STEPS: u32 = 200;
pub const ATTACH_DEFAULT_RUNTIME_S: u64 = 120;
pub const ATTACH_MAX_RUNTIME_S: u64 = 3600;
/// The most hosts an attach grant may allow (canonical, at least one).
const ATTACH_MAX_DOMAINS: usize = 8;

/// `POST .../attach` body.
#[derive(Debug, Deserialize)]
pub struct AttachBody {
    /// The attach grant the broker derived for this run (`DeriveRunGrant purpose=attach`).
    #[serde(alias = "grantId")]
    pub grant_id: String,
    /// The first page of the agent tab, when the attach names one.
    #[serde(default, alias = "startUrl")]
    pub start_url: Option<String>,
    #[serde(default)]
    pub constraints: AttachConstraints,
}

#[derive(Debug, Default, Deserialize)]
pub struct AttachConstraints {
    #[serde(default, alias = "maxSteps")]
    pub max_steps: Option<u32>,
    #[serde(default, alias = "maxRuntimeS")]
    pub max_runtime_s: Option<u64>,
    /// Accepted for the wire shape; the cost bound is enforced by the run's budget, not here.
    #[serde(default, alias = "maxCostUsd")]
    pub max_cost_usd: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct AttachData {
    pub attach_id: String,
    pub tab_id: String,
    pub tab_epoch: u64,
    pub control_epoch: u64,
    pub opened_by: &'static str,
    pub holder: &'static str,
    pub allowed_domains: Vec<String>,
}

/// `DELETE .../attach/{attach_id}` query: `release` (default) or `abort`.
#[derive(Debug, Default, Deserialize)]
pub struct ReleaseQuery {
    #[serde(default)]
    pub disposition: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ReleaseData {
    pub attach_id: String,
    pub state: &'static str,
}

/// The wire value of a tab holder (`human` or `agent`).
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TabHolderWire {
    Human,
    Agent,
}

/// `POST .../tabs/{tab_id}/control` body.
#[derive(Debug, Deserialize)]
pub struct TabControlBody {
    pub holder: TabHolderWire,
    #[serde(default, alias = "expectedEpoch")]
    pub expected_epoch: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct TabControlData {
    pub tab_id: String,
    pub holder: &'static str,
    pub tab_epoch: u64,
    pub control_epoch: u64,
}

/// Why an attach grant is not usable for this run, as the typed reason. `None` when it is.
///
/// The grant must be live, of kind `attach`, owned by the run's owner, for this run,
/// a child of the run's grant, and allow between one and eight hosts.
pub(super) fn attach_grant_refusal(
    grant: &GrantValidation,
    run_id: &str,
    run_owner: &str,
    parent_grant_id: &str,
) -> Option<&'static str> {
    if !grant.is_usable() {
        return Some("attach_grant_invalid");
    }
    if grant.kind != "attach"
        || grant.owner_id != run_owner
        || grant.resource_id != run_id
        || grant.parent_grant_id.as_deref() != Some(parent_grant_id)
    {
        return Some("attach_grant_invalid");
    }
    if grant.allowed_domains.is_empty() || grant.allowed_domains.len() > ATTACH_MAX_DOMAINS {
        return Some("attach_grant_invalid");
    }
    None
}

/// The tab ledger's view of the driver's live tabs: a tab the ledger does not know is
/// a person's (the first tab, or one the person opened). Known tabs keep their state,
/// and tabs the browser no longer has are dropped.
fn sync_ledger_with_driver(live: &RunLive, tabs: &[BrowserTab]) {
    let ids: Vec<&str> = tabs.iter().map(|tab| tab.tab_id.as_str()).collect();
    let mut ledger = live.tabs();
    ledger.retain_live(&ids);
    for tab in tabs {
        ledger.seed(&tab.tab_id, OpenedBy::Human);
    }
}

fn new_attach_id() -> String {
    format!("att_{}", uuid::Uuid::new_v4().simple())
}

fn unix_ms_now() -> u64 {
    now_ms()
}

/// `POST /v1/agent/runs/{run_id}/attach` (service principal only).
pub async fn attach_run(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(run_id): Path<String>,
    Json(body): Json<AttachBody>,
) -> Result<Json<Envelope<AttachData>>, ApiErr> {
    let request_id = crate::request_id::current();
    if !crate::run_limits::attach_enabled() {
        return Err(driver_err(
            &request_id,
            QuarryError::new(ErrorCode::Forbidden, "attach_disabled"),
        ));
    }
    if !claims.is_service() {
        return Err(driver_err(
            &request_id,
            QuarryError::new(ErrorCode::Forbidden, "attach_requires_service_principal"),
        ));
    }
    let Some(handle) = lookup_run(&state, &run_id) else {
        return Err(run_gone_err(&state, &claims, &request_id, &run_id).await);
    };
    let _in_flight = handle.live.in_flight_guard();
    let mut entry = handle.entry.lock().await;
    if entry.org_id != claims.org_id {
        return Err(status_err(StatusCode::NOT_FOUND, &request_id, "agent run not found"));
    }
    handle.live.touch();
    if entry.closed {
        return Err(run_closed_err(&state, &claims, &request_id, &run_id).await);
    }
    let refuse = |reason: &str| driver_err(&request_id, QuarryError::new(ErrorCode::Forbidden, reason));
    if entry.zdr.is_active() {
        return Err(refuse("attach_zdr_refused"));
    }
    let Some(parent_grant) = entry.grant_id.clone() else {
        return Err(refuse("attach_requires_grant"));
    };
    let caps = state.agent_driver.capabilities();
    if !(caps.background_tabs && caps.tab_pinning && caps.per_tab_egress) {
        return Err(driver_err(
            &request_id,
            QuarryError::new(ErrorCode::Unsupported, "attach_driver_unsupported"),
        ));
    }
    if handle.live.attaches().live().is_some() {
        return Err(driver_err(
            &request_id,
            QuarryError::new(ErrorCode::Conflict, "attach_exists"),
        ));
    }
    let max_steps = body.constraints.max_steps.unwrap_or(ATTACH_DEFAULT_STEPS);
    let max_runtime_s = body.constraints.max_runtime_s.unwrap_or(ATTACH_DEFAULT_RUNTIME_S);
    if !(1..=ATTACH_MAX_STEPS).contains(&max_steps) || !(1..=ATTACH_MAX_RUNTIME_S).contains(&max_runtime_s) {
        return Err(status_err(
            StatusCode::BAD_REQUEST,
            &request_id,
            "constraints.max_steps must be 1 to 200 and max_runtime_s 1 to 3600",
        ));
    }

    let grant = state
        .grant_validator
        .validate_for_org(&entry.org_id, &body.grant_id)
        .await
        .map_err(|error| driver_err(&request_id, error))?;
    let owner = entry.actor_id.clone();
    let run_id_text = entry.run_id.to_string();
    if let Some(reason) = attach_grant_refusal(&grant, &run_id_text, &owner, &parent_grant) {
        return Err(refuse(reason));
    }
    let allowed_domains = grant.allowed_domains.clone();

    if let Some(url) = body.start_url.as_deref() {
        if !domain_allowed(url, &allowed_domains) {
            return Err(driver_err(
                &request_id,
                QuarryError::new(
                    ErrorCode::SecurityBlocked,
                    "attach start URL is outside the attachment's allowed domains",
                ),
            ));
        }
        validate_navigation(
            &AgentAction::Navigate {
                url: url.to_owned(),
            },
            state.security.as_ref(),
        )
        .await
        .map_err(|error| driver_err(&request_id, error))?;
        crate::agent_policy_gate::gate_visit(&state, &handle.live, false, url)
            .map_err(|error| driver_err(&request_id, error))?;
    }

    // The tab opens in the background: the person's active tab does not change.
    let tab = state
        .agent_driver
        .open_tab(
            &entry.session,
            OpenTabOptions {
                url: body.start_url.clone(),
                activate: false,
                egress_policy: Some(BrowserEgressPolicy::from_allowed_domains(&allowed_domains)),
            },
        )
        .await
        .map_err(|error| driver_err(&request_id, error))?;
    let tabs = state
        .agent_driver
        .list_tabs(&entry.session)
        .await
        .map_err(|error| driver_err(&request_id, error))?;
    let attach_id = new_attach_id();
    let tab_id = tab.tab_id.clone();
    sync_ledger_with_driver(&handle.live, &tabs);
    handle.live.tabs().open_agent_tab(&tab_id, &attach_id);
    let started = unix_ms_now();
    handle
        .live
        .attaches()
        .begin(AttachState {
            attach_id: attach_id.clone(),
            grant_id: body.grant_id.clone(),
            delegate_actor: claims.actor_id().to_owned(),
            tab_id: tab_id.clone(),
            allowed_domains: allowed_domains.clone(),
            budget: AttachBudget::new(max_steps, max_runtime_s),
            phase: BrowserAttachState::Attached,
            started_ms: started,
            last_activity_ms: started,
        })
        .map_err(|_| {
            driver_err(
                &request_id,
                QuarryError::new(ErrorCode::Conflict, "attach_exists"),
            )
        })?;
    let agent = derive_initiator_ref(&claims, None, &AdvisoryAgent::default()).agent;
    let control_epoch = handle.live.snapshot().epoch;
    remember_tab_opener(
        &mut entry,
        &tab_id,
        TabOrigin {
            initiated_by: StepInitiator::Agent,
            agent: agent.clone(),
            unattributed: false,
        },
        &tabs,
    );
    record_browser_timeline_event(
        &state,
        &entry,
        BrowserTimelineEventKind::Attach {
            state: BrowserAttachState::Attached,
            attach_id: attach_id.clone(),
            tab_id: Some(tab_id.clone()),
        },
    )
    .await
    .map_err(|error| driver_err(&request_id, error))?;
    record_browser_timeline_event(
        &state,
        &entry,
        BrowserTimelineEventKind::Tab {
            operation: BrowserTabOperation::Opened,
            tab_id: Some(tab_id.clone()),
            active_tab_id: active_browser_tab_id(&tabs),
            initiated_by: Some(StepInitiator::Agent),
            agent,
            control_epoch,
        },
    )
    .await
    .map_err(|error| driver_err(&request_id, error))?;
    state
        .event_sink
        .emit_for_zdr(
            entry.zdr,
            entry.run_id.clone(),
            EventType::AgentAttached,
            serde_json::json!({ "attach_id": attach_id, "tab_id": tab_id }),
            format!("{run_id_text}:agent_attached:{attach_id}"),
        )
        .await;
    let tab_epoch = handle.live.tabs().get(&tab_id).map_or(0, |meta| meta.epoch);
    Ok(Json(Envelope::ok(
        request_id,
        AttachData {
            attach_id,
            tab_id,
            tab_epoch,
            control_epoch,
            opened_by: "agent",
            holder: "agent",
            allowed_domains,
        },
    )))
}

/// `DELETE /v1/agent/runs/{run_id}/attach/{attach_id}`: the delegate ends its own
/// attachment, or the owner stops it. The agent tab is closed; the grant is not revoked.
pub async fn release_attach(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path((run_id, attach_id)): Path<(String, String)>,
    Query(query): Query<ReleaseQuery>,
) -> Result<Json<Envelope<ReleaseData>>, ApiErr> {
    let request_id = crate::request_id::current();
    let not_found = || status_err(StatusCode::NOT_FOUND, &request_id, "agent run not found");
    let Some(handle) = lookup_run(&state, &run_id) else {
        return Err(not_found());
    };
    let _in_flight = handle.live.in_flight_guard();
    let mut entry = handle.entry.lock().await;
    let owner = caller_owns_live_run(&entry, &claims);
    let delegate = claims.is_service()
        && entry.org_id == claims.org_id
        && handle
            .live
            .attaches()
            .live()
            .is_some_and(|live| live.attach_id == attach_id && live.delegate_actor == claims.actor_id());
    if !(owner || delegate) {
        return Err(not_found());
    }
    handle.live.touch();
    let phase = match query.disposition.as_deref() {
        None | Some("release") => BrowserAttachState::Released,
        Some("abort") => BrowserAttachState::Aborted,
        Some(_) => {
            return Err(status_err(
                StatusCode::BAD_REQUEST,
                &request_id,
                "disposition must be release or abort",
            ))
        }
    };
    let ended = handle
        .live
        .attaches()
        .end(&attach_id, phase)
        .ok_or_else(not_found)?;
    finish_attach(&state, &handle, &mut entry, ended, phase).await;
    Ok(Json(Envelope::ok(
        request_id,
        ReleaseData {
            attach_id,
            state: match phase {
                BrowserAttachState::Aborted => "aborted",
                _ => "released",
            },
        },
    )))
}

/// The common end of an attachment: the agent tab is closed (its ids leave the ledger),
/// and the timeline and the event record the end. Called with the run's lock held.
async fn finish_attach(
    state: &AppState,
    handle: &RunHandle,
    entry: &mut RunEntry,
    ended: AttachState,
    phase: BrowserAttachState,
) {
    if let Err(error) = state.agent_driver.close_tab(&entry.session, &ended.tab_id).await {
        // zdr-log-ok: the tab id is an opaque browser id; the driver error text is masked by the caller's driver path
        tracing::warn!(%error, "could not close the agent tab of an ended attachment");
    }
    handle.live.tabs().forget(&ended.tab_id);
    if let Err(error) = record_browser_timeline_event(
        state,
        entry,
        BrowserTimelineEventKind::Attach {
            state: phase,
            attach_id: ended.attach_id.clone(),
            tab_id: Some(ended.tab_id.clone()),
        },
    )
    .await
    {
        // zdr-log-ok: the store error text is what an operator needs; it carries no caller data
        tracing::warn!(%error, "could not record the attach end on the timeline");
    }
    let disposition = match phase {
        BrowserAttachState::Released => "release",
        BrowserAttachState::Aborted => "abort",
        BrowserAttachState::Revoked => "revoked",
        BrowserAttachState::IdleReleased => "idle",
        BrowserAttachState::SessionEnded => "session_ended",
        BrowserAttachState::Attached => "attached",
    };
    state
        .event_sink
        .emit_for_zdr(
            entry.zdr,
            entry.run_id.clone(),
            EventType::AgentDetached,
            serde_json::json!({ "attach_id": ended.attach_id, "disposition": disposition }),
            format!("{}:agent_detached:{}", entry.run_id, ended.attach_id),
        )
        .await;
}

/// `POST /v1/agent/runs/{run_id}/tabs/{tab_id}/control`: a person hands a tab to the
/// agent (`holder: agent`, needs a live attachment) or takes it back (`holder: human`).
/// Only the owner's own token may do this; a delegate never can.
pub async fn control_tab(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path((run_id, tab_id)): Path<(String, String)>,
    Json(body): Json<TabControlBody>,
) -> Result<Json<Envelope<TabControlData>>, ApiErr> {
    let request_id = crate::request_id::current();
    let Some(handle) = lookup_run(&state, &run_id) else {
        return Err(status_err(StatusCode::NOT_FOUND, &request_id, "agent run not found"));
    };
    if !handle.live.owned_by(&claims) {
        return Err(status_err(StatusCode::NOT_FOUND, &request_id, "agent run not found"));
    }
    handle.live.touch();
    if derive_initiator(&claims, None) != StepInitiator::Human {
        return Err(driver_err(
            &request_id,
            QuarryError::new(ErrorCode::Forbidden, "control_transition_requires_human"),
        ));
    }
    let _in_flight = handle.live.in_flight_guard();
    let entry = handle.entry.lock().await;
    if entry.closed {
        return Err(run_closed_err(&state, &claims, &request_id, &run_id).await);
    }
    let tabs = state
        .agent_driver
        .list_tabs(&entry.session)
        .await
        .map_err(|error| driver_err(&request_id, error))?;
    sync_ledger_with_driver(&handle.live, &tabs);

    let holder = match body.holder {
        TabHolderWire::Human => BrowserTabHolder::Human,
        TabHolderWire::Agent => BrowserTabHolder::Agent,
    };
    let change = {
        let mut ledger = handle.live.tabs();
        match holder {
            BrowserTabHolder::Human => ledger.set_holder(&tab_id, holder, body.expected_epoch),
            BrowserTabHolder::Agent => {
                let attaches = handle.live.attaches();
                hand_to_agent(&mut ledger, &attaches, &tab_id, body.expected_epoch)
            }
        }
    };
    let (changed, tab_epoch) = match change {
        Ok(TabChange::Changed { epoch }) => (true, epoch),
        Ok(TabChange::Unchanged { epoch }) => (false, epoch),
        Err(TabError::NotFound) => {
            return Err(driver_err(
                &request_id,
                QuarryError::new(ErrorCode::NotFound, "pinned_tab_closed"),
            ))
        }
        Err(TabError::Stale { current }) => {
            return Err(driver_err(
                &request_id,
                QuarryError::new(ErrorCode::Conflict, "stale_tab_epoch")
                    .with_details(serde_json::json!({ "tab_epoch": current })),
            ))
        }
        Err(TabError::NoAttachedAgent) => {
            return Err(driver_err(
                &request_id,
                QuarryError::new(ErrorCode::Conflict, "no_attached_agent"),
            ))
        }
        Err(TabError::HeldByHuman { tab_epoch }) => {
            return Err(driver_err(
                &request_id,
                QuarryError::new(ErrorCode::UserTookOver, "tab is held by a person")
                    .with_details(serde_json::json!({ "scope": "tab", "tab_id": tab_id, "tab_epoch": tab_epoch })),
            ))
        }
    };
    let control_epoch = handle.live.snapshot().epoch;
    if changed {
        record_browser_timeline_event(
            &state,
            &entry,
            BrowserTimelineEventKind::TabControl {
                tab_id: tab_id.clone(),
                holder,
                initiated_by: StepInitiator::Human,
                tab_epoch,
            },
        )
        .await
        .map_err(|error| driver_err(&request_id, error))?;
    }
    Ok(Json(Envelope::ok(
        request_id,
        TabControlData {
            tab_id,
            holder: match holder {
                BrowserTabHolder::Human => "human",
                BrowserTabHolder::Agent => "agent",
            },
            tab_epoch,
            control_epoch,
        },
    )))
}

/// One step of the delegate lane: the attachment's agent acts on its own tab. Called by
/// `step` when the caller does not own the run. Every refusal is the same 404 unless the
/// caller is the live attachment's delegate (the lane check), so nothing else is revealed.
pub(super) async fn delegate_step(
    state: &AppState,
    handle: &RunHandle,
    claims: &crate::auth::Claims,
    request_id: &str,
    body: StepBody,
) -> Result<Json<Envelope<BrowserObservation>>, ApiErr> {
    let not_found = || status_err(StatusCode::NOT_FOUND, request_id, "agent run not found");
    let _in_flight = handle.live.in_flight_guard();
    let mut entry = handle.entry.lock().await;
    let attach = {
        let attaches = handle.live.attaches();
        match attaches.live() {
            Some(live)
                if delegate_matches(
                    live,
                    &entry.org_id,
                    &claims.org_id,
                    claims.actor_id(),
                    claims.is_service(),
                    body.grant_id.as_deref(),
                ) =>
            {
                live.clone()
            }
            _ => return Err(not_found()),
        }
    };
    handle.live.touch();
    if entry.closed {
        return Err(not_found());
    }

    // The tab gate: a person holds the tab, so the delegate is refused (USER_TOOK_OVER).
    let passive = is_passive(&body.action);
    let gate = handle
        .live
        .tabs()
        .gate_action(&attach.tab_id, Actor::Agent, passive);
    match gate {
        Ok(TabAdmit::Allowed) | Ok(TabAdmit::TakenOver { .. }) => {}
        Err(TabError::HeldByHuman { tab_epoch }) => {
            return Err(driver_err(
                request_id,
                QuarryError::new(ErrorCode::UserTookOver, "the person using the browser has taken control of this tab; this action was not run")
                    .with_details(serde_json::json!({
                        "scope": "tab",
                        "tab_id": attach.tab_id,
                        "tab_epoch": tab_epoch,
                        "control_epoch": handle.live.snapshot().epoch,
                    })),
            ))
        }
        Err(_) => {
            return Err(driver_err(
                request_id,
                QuarryError::new(ErrorCode::NotFound, "pinned_tab_closed"),
            ))
        }
    }

    // The attachment's own grant is revalidated on every step. One that is no longer
    // usable ends the attachment (Revoked) and refuses with 403, never a 5xx.
    let owner = entry.actor_id.clone();
    let run_id_text = entry.run_id.to_string();
    let parent = entry.grant_id.clone().unwrap_or_default();
    let grant = state
        .grant_validator
        .validate_for_org(&entry.org_id, &attach.grant_id)
        .await
        .map_err(|error| driver_err(request_id, error))?;
    if attach_grant_refusal(&grant, &run_id_text, &owner, &parent).is_some() {
        let ended = handle
            .live
            .attaches()
            .end(&attach.attach_id, BrowserAttachState::Revoked);
        if let Some(ended) = ended {
            finish_attach(state, handle, &mut entry, ended, BrowserAttachState::Revoked).await;
        }
        return Err(driver_err(
            request_id,
            QuarryError::new(ErrorCode::SecurityBlocked, "attach_grant_invalid"),
        ));
    }

    // The attachment's budget counts only delegate steps; a person's steps never reach it.
    let elapsed_s = unix_ms_now().saturating_sub(attach.started_ms) / 1000;
    if let Err(refusal) = attach.budget.admit(elapsed_s) {
        let reason = match refusal {
            AttachRefusal::Steps => "attach_steps",
            AttachRefusal::Runtime => "attach_runtime",
        };
        return Err(driver_err(
            request_id,
            QuarryError::new(ErrorCode::RateLimited, reason)
                .with_details(serde_json::json!({ "reason": reason })),
        ));
    }

    // The run-level chain (control, run grant, driver capabilities, policy, navigation
    // security). The initiator is the delegate, so it is an agent whatever it sends.
    let admitted = admit_action(
        state,
        handle,
        &mut entry,
        AdmitInput {
            initiator: derive_initiator_ref(claims, body.initiated_by, &body.advisory_agent()),
            action: &body.action,
            enforce_takeover: crate::run_limits::takeover_enforced(),
        },
    )
    .await
    .map_err(|error| driver_err(request_id, error))?;
    if let AgentAction::Navigate { url } = &body.action {
        if !domain_allowed(url, &attach.allowed_domains) {
            return Err(driver_err(
                request_id,
                QuarryError::new(
                    ErrorCode::SecurityBlocked,
                    "browser navigation is outside the attachment's allowed domains",
                ),
            ));
        }
    }

    let tab_id = attach.tab_id.clone();
    let attach_id = attach.attach_id.clone();
    let pinned = entry.session.pinned_to(&tab_id);
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
    let typed_kind =
        quarry_runtime::observation::typed_field_kind(entry.ctx.active_snapshot.as_ref(), &req.action);
    let runner = ObservationRunner {
        browser: state.agent_driver.clone(),
        artifacts: Some(state.artifacts.clone()),
        events: Some(state.event_sink.clone()),
        visual_processor: state.visual_processor.clone(),
        org_id: entry.org_id.clone(),
    };
    // The step is admitted: it is charged to the attachment whether or not the browser
    // action succeeds, so a failing delegate cannot retry past its budget.
    if let Some(live) = handle.live.attaches().live_mut() {
        live.budget.charge();
        live.last_activity_ms = unix_ms_now();
    }
    let step_result = runner.execute(&req, &pinned, &mut entry.ctx).await;
    let observation = match step_result {
        Ok(observation) => {
            let cost_usd = receipt_cost_usd(&observation.telemetry, &receipt_action);
            let cost_source = receipt_cost_source(&observation.telemetry);
            let receipt = ReceiptBuilder::start(receipt_run_id, receipt_step)
                .org_id(entry.org_id.clone())
                .actor_id(entry.actor_id.clone())
                .initiator(StepInitiator::Agent, admitted.control_epoch)
                .agent(admitted.agent.clone())
                .cost_source(cost_source)
                .tab(Some(tab_id.clone()), Some(attach_id.clone()))
                .typed_field(typed_kind)
                .complete(receipt_action, Some(observation.clone()), cost_usd);
            state
                .receipts
                .append(receipt)
                .await
                .map_err(|error| driver_err(request_id, error))?;
            observation
        }
        Err(error) => {
            let receipt = ReceiptBuilder::start(receipt_run_id, receipt_step)
                .org_id(entry.org_id.clone())
                .actor_id(entry.actor_id.clone())
                .initiator(StepInitiator::Agent, admitted.control_epoch)
                .agent(admitted.agent.clone())
                .failed_cost(estimated_action_cost_usd(&receipt_action))
                .cost_source(CostSource::FlatTable)
                .tab(Some(tab_id.clone()), Some(attach_id.clone()))
                .typed_field(typed_kind)
                .fail(receipt_action, &error);
            state
                .receipts
                .append(receipt)
                .await
                .map_err(|error| driver_err(request_id, error))?;
            return Err(driver_err(request_id, error));
        }
    };
    entry.last_observation = Some(observation.clone());
    if !entry.zdr.is_active() {
        state
            .receipts
            .save_run_checkpoint(checkpoint_for_entry(&entry))
            .await
            .map_err(|error| driver_err(request_id, error))?;
    }
    if !domain_allowed(&observation.url, &attach.allowed_domains) {
        return Err(driver_err(
            request_id,
            QuarryError::new(
                ErrorCode::SecurityBlocked,
                "browser action reached a domain outside the attachment's allowed domains",
            ),
        ));
    }
    crate::agent_policy_gate::gate_visit(state, &handle.live, entry.zdr.is_active(), &observation.url)
        .map_err(|error| driver_err(request_id, error))?;
    Ok(Json(Envelope::ok(request_id.to_owned(), observation)))
}

/// D8-7 for a person's own action: when the tab the action addresses (the active tab) is held
/// by the agent, a non-passive action takes it over in this step and the timeline records the
/// change. A passive read does not. Without a live attachment no tab is agent-held, so this is
/// a no-op for every run that has never attached.
pub(super) async fn person_tab_gate(
    state: &AppState,
    handle: &RunHandle,
    entry: &RunEntry,
    initiator: StepInitiator,
    action: &AgentAction,
) -> QuarryResult<()> {
    if initiator != StepInitiator::Human || handle.live.attaches().live().is_none() {
        return Ok(());
    }
    let Some(tab_id) = state.agent_driver.active_tab_id(&entry.session).await? else {
        return Ok(());
    };
    let admit = handle
        .live
        .tabs()
        .gate_action(&tab_id, Actor::Human, is_passive(action));
    if let Ok(TabAdmit::TakenOver { tab_epoch }) = admit {
        record_browser_timeline_event(
            state,
            entry,
            BrowserTimelineEventKind::TabControl {
                tab_id,
                holder: BrowserTabHolder::Human,
                initiated_by: StepInitiator::Human,
                tab_epoch,
            },
        )
        .await?;
    }
    Ok(())
}

#[cfg(test)]
mod attach_tests {
    use super::*;

    fn grant(kind: &str, owner: &str, resource: &str, parent: Option<&str>, domains: &[&str]) -> GrantValidation {
        GrantValidation {
            grant_id: "grant_attach".to_owned(),
            active: true,
            expires_at: None,
            allowed_domains: domains.iter().map(|d| (*d).to_owned()).collect(),
            allowed_actions: Vec::new(),
            allowed_frame_ids: Vec::new(),
            allowed_dialog_ids: Vec::new(),
            allowed_artifact_ids: Vec::new(),
            parent_grant_id: parent.map(str::to_owned),
            kind: kind.to_owned(),
            owner_id: owner.to_owned(),
            zdr: false,
            budget: None,
            policy_mode: "allow_list".to_owned(),
            denied_domains: Vec::new(),
            absolute_expires_at: None,
            resource_id: resource.to_owned(),
        }
    }

    #[test]
    fn attach_grant_must_match_run_owner_and_parent() {
        let good = grant("attach", "user_a", "run_1", Some("grant_run"), &["example.com"]);
        assert_eq!(attach_grant_refusal(&good, "run_1", "user_a", "grant_run"), None);
        assert_eq!(
            attach_grant_refusal(&grant("run", "user_a", "run_1", Some("grant_run"), &["example.com"]), "run_1", "user_a", "grant_run"),
            Some("attach_grant_invalid")
        );
        assert_eq!(
            attach_grant_refusal(&grant("attach", "user_b", "run_1", Some("grant_run"), &["example.com"]), "run_1", "user_a", "grant_run"),
            Some("attach_grant_invalid")
        );
        assert_eq!(
            attach_grant_refusal(&grant("attach", "user_a", "run_2", Some("grant_run"), &["example.com"]), "run_1", "user_a", "grant_run"),
            Some("attach_grant_invalid")
        );
        assert_eq!(
            attach_grant_refusal(&grant("attach", "user_a", "run_1", None, &["example.com"]), "run_1", "user_a", "grant_run"),
            Some("attach_grant_invalid")
        );
    }

    #[test]
    fn attach_grant_allows_one_to_eight_hosts() {
        assert_eq!(
            attach_grant_refusal(&grant("attach", "user_a", "run_1", Some("grant_run"), &[]), "run_1", "user_a", "grant_run"),
            Some("attach_grant_invalid")
        );
        let nine: Vec<&str> = vec!["a.example"; 9];
        assert_eq!(
            attach_grant_refusal(&grant("attach", "user_a", "run_1", Some("grant_run"), &nine), "run_1", "user_a", "grant_run"),
            Some("attach_grant_invalid")
        );
    }

    #[test]
    fn attach_defaults_are_inside_the_bounds() {
        assert!(ATTACH_DEFAULT_STEPS >= 1 && ATTACH_DEFAULT_STEPS <= ATTACH_MAX_STEPS);
        assert!(ATTACH_DEFAULT_RUNTIME_S >= 1 && ATTACH_DEFAULT_RUNTIME_S <= ATTACH_MAX_RUNTIME_S);
    }
}
