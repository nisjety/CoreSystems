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
            lane: LaneRef::new(LaneState::default()),
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
        &handle.live,
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
        &handle.live,
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
        &handle.live,
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
            &handle.live,
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
/// The live attachment this caller may use as its delegate, or `None` (every delegate refusal
/// is the same 404). The caller holds the entry lock; no lane lock is taken here.
pub(super) fn delegate_attach_of(
    handle: &RunHandle,
    entry: &RunEntry,
    claims: &crate::auth::Claims,
    grant_id: Option<&str>,
) -> Option<AttachState> {
    let attaches = handle.live.attaches();
    match attaches.live() {
        Some(live)
            if delegate_matches(
                live,
                &entry.org_id,
                &claims.org_id,
                claims.actor_id(),
                claims.is_service(),
                grant_id,
            ) =>
        {
            Some(live.clone())
        }
        _ => None,
    }
}

pub(super) async fn delegate_step(
    state: &AppState,
    handle: &RunHandle,
    claims: &crate::auth::Claims,
    request_id: &str,
    body: StepBody,
) -> Result<Json<Envelope<BrowserObservation>>, ApiErr> {
    let not_found = || status_err(StatusCode::NOT_FOUND, request_id, "agent run not found");
    let _in_flight = handle.live.in_flight_guard();

    // WS8.H: the delegate's lane is the attachment's own. It is taken before the entry, and
    // the entry is released while the browser runs, so a person's action on the other lane
    // does not wait for this step.
    let lane_ref = {
        let entry = handle.entry.lock().await;
        match delegate_attach_of(handle, &entry, claims, body.grant_id.as_deref()) {
            Some(attach) => attach.lane,
            None => return Err(not_found()),
        }
    };
    let mut lane = lane_ref.0.lock().await;
    let mut entry = handle.entry.lock().await;
    // The attachment may have ended, or the run closed, while this step waited for its lane.
    let attach = match delegate_attach_of(handle, &entry, claims, body.grant_id.as_deref()) {
        Some(attach) if attach.lane == lane_ref && !entry.closed => attach,
        _ => return Err(not_found()),
    };
    handle.live.touch();

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
    // usable ends the attachment (Revoked) and refuses with 403, never a 5xx. The grant is
    // checked without the entry lock, so the other lane does not wait for the network.
    let org_id = entry.org_id.clone();
    let owner = entry.actor_id.clone();
    let run_id_text = entry.run_id.to_string();
    let parent = entry.grant_id.clone().unwrap_or_default();
    drop(entry);
    let grant = state
        .grant_validator
        .validate_for_org(&org_id, &attach.grant_id)
        .await
        .map_err(|error| driver_err(request_id, error))?;
    let mut entry = handle.entry.lock().await;
    if entry.closed || !handle.live.attaches().live().is_some_and(|live| live.attach_id == attach.attach_id) {
        return Err(not_found());
    }
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
            last_observation: lane.last_observation.as_deref(),
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
    // WS8.H: the step key is reserved after admission (a refusal takes none), shared with
    // the person's lane, so the events and receipts of the two lanes never collide.
    let receipt_step = handle.live.reserve_step();
    lane.ctx.step = receipt_step;
    let receipt_run_id = entry.run_id.to_string();
    let receipt_action = req.action.clone();
    let typed_kind = quarry_runtime::observation::receipt_field_kind(&lane.ctx, &req.action);
    let runner = ObservationRunner {
        browser: state.agent_driver.clone(),
        artifacts: Some(state.artifacts.clone()),
        events: Some(state.event_sink.clone()),
        visual_processor: state.visual_processor.clone(),
        org_id: entry.org_id.clone(),
    };
    let receipt_org = entry.org_id.clone();
    let receipt_actor = entry.actor_id.clone();
    // The run entry is released while the browser runs (WS8.H).
    drop(entry);

    // The step is admitted: it is charged to the attachment whether or not the browser
    // action succeeds, so a failing delegate cannot retry past its budget.
    if let Some(live) = handle.live.attaches().live_mut() {
        live.budget.charge();
        live.last_activity_ms = unix_ms_now();
    }
    let egress_cursor = lane.ctx.last_egress_sequence;
    let step_result = runner.execute(&req, &pinned, &mut lane.ctx).await;
    let mut entry = handle.entry.lock().await;
    // D2-031: the attachment may have been released, revoked or ended (or the run closed)
    // while the browser ran this step. Admission was checked before it ran; the lane's owner
    // is checked again here, before anything is recorded or returned. A step whose
    // attachment is gone answers the same 404 as any call from a delegate that is no longer
    // one, advances nothing on the run, and leaves a failed receipt under the step key it
    // reserved (so the run's step numbers have no hole). The page effect of the one action
    // that was already running cannot be undone; the receipt says its outcome was not
    // delivered, not that it did not happen.
    let attach_still_live = !entry.closed
        && handle
            .live
            .attaches()
            .live()
            .is_some_and(|live| live.attach_id == attach_id);
    if !attach_still_live {
        let receipt = ReceiptBuilder::start(receipt_run_id, receipt_step)
            .org_id(receipt_org)
            .actor_id(receipt_actor)
            .initiator(StepInitiator::Agent, admitted.control_epoch)
            .agent(admitted.agent.clone())
            .failed_cost(estimated_action_cost_usd(&receipt_action))
            .cost_source(CostSource::FlatTable)
            .tab(Some(tab_id.clone()), Some(attach_id.clone()))
            .typed_field(typed_kind)
            .fail(
                receipt_action,
                &QuarryError::new(ErrorCode::NotFound, "attach_released_during_step"),
            );
        if let Err(error) = state.receipts.append(receipt).await {
            // zdr-log-ok: a ZDR run has no delegate (attach is refused for it); the store error text is what an operator needs
            tracing::warn!(%error, "could not record the receipt of a delegate step whose attachment ended");
        }
        // D2: the spent key is saved (skipped when the run itself closed).
        checkpoint_failed_step(state, &entry, &handle.live).await;
        return Err(not_found());
    }
    // The landed page is checked before the receipt and the checkpoint (T4b-1), against the
    // attachment's hosts. An agent tab opened without a start URL is on `about:blank`.
    let ran = step_result.as_ref().ok().map(|observation| observation.telemetry.clone());
    let step_result = step_result.and_then(|observation| {
        super::step::check_landed_page(
            state,
            &handle.live,
            entry.zdr.is_active(),
            &observation.url,
            |url| domain_allowed(url, &attach.allowed_domains),
            "browser action reached a domain outside the attachment's allowed domains",
        )?;
        Ok(observation)
    });
    let observation = match step_result {
        Ok(observation) => {
            let cost_usd = receipt_cost_usd(&observation.telemetry, &receipt_action);
            let cost_source = receipt_cost_source(&observation.telemetry);
            let receipt = ReceiptBuilder::start(receipt_run_id, receipt_step)
                .org_id(receipt_org.clone())
                .actor_id(receipt_actor.clone())
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
            let (failed_usd, failed_source) = failed_step_cost(ran.as_ref(), &receipt_action);
            lane.ctx.last_egress_sequence = egress_cursor;
            let receipt = ReceiptBuilder::start(receipt_run_id, receipt_step)
                .org_id(receipt_org)
                .actor_id(receipt_actor)
                .initiator(StepInitiator::Agent, admitted.control_epoch)
                .agent(admitted.agent.clone())
                .failed_cost(failed_usd)
                .cost_source(failed_source)
                .tab(Some(tab_id.clone()), Some(attach_id.clone()))
                .typed_field(typed_kind)
                .fail(receipt_action, &error);
            let appended = state.receipts.append(receipt).await;
            // D2: the spent key is saved whether or not its receipt was kept.
            checkpoint_failed_step(state, &entry, &handle.live).await;
            appended.map_err(|error| driver_err(request_id, error))?;
            return Err(driver_err(request_id, error));
        }
    };
    let shared = Arc::new(observation.clone());
    lane.last_observation = Some(shared.clone());
    entry.summary.record(&lane.ctx, shared);
    if !entry.zdr.is_active() {
        state
            .receipts
            .save_run_checkpoint(checkpoint_for_entry(&entry, &handle.live))
            .await
            .map_err(|error| driver_err(request_id, error))?;
    }
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
            &handle.live,
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

/// D2-031 and D2-074: a delegate's steps and its tab list, against a driver double that can hold
/// a step inside the browser (so a test can release the attachment while the step runs).
#[cfg(test)]
mod delegate_lane_tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// Answers every call a `wait` step makes from memory. When armed, `content` (the first
    /// read after the action) signals `entered` and then waits for `gate`.
    struct DelegateDriver {
        armed: AtomicBool,
        entered: tokio::sync::Notify,
        gate: tokio::sync::Notify,
        tab_ids: Vec<String>,
    }

    impl DelegateDriver {
        fn new(tab_ids: &[&str]) -> Arc<Self> {
            Arc::new(Self {
                armed: AtomicBool::new(false),
                entered: tokio::sync::Notify::new(),
                gate: tokio::sync::Notify::new(),
                tab_ids: tab_ids.iter().map(|id| (*id).to_owned()).collect(),
            })
        }
    }

    #[async_trait::async_trait]
    impl quarry_browser::BrowserDriver for DelegateDriver {
        async fn acquire(&self, lease: &BrowserLease) -> QuarryResult<BrowserSession> {
            Ok(BrowserSession {
                lease: lease.clone(),
                tab_pin: None,
                inner: Arc::new(TokioMutex::new(SessionInner::default())),
            })
        }
        async fn release(&self, _session: BrowserSession) -> QuarryResult<()> {
            Ok(())
        }
        async fn configure_egress_policy(
            &self,
            _session: &BrowserSession,
            _policy: BrowserEgressPolicy,
        ) -> QuarryResult<()> {
            Ok(())
        }
        async fn goto(&self, _session: &BrowserSession, _url: &str) -> QuarryResult<()> {
            Ok(())
        }
        async fn content(&self, _session: &BrowserSession) -> QuarryResult<bytes::Bytes> {
            if self.armed.load(Ordering::SeqCst) {
                self.entered.notify_one();
                self.gate.notified().await;
            }
            Ok(bytes::Bytes::from_static(b"<html><body><p>page</p></body></html>"))
        }
        async fn screenshot(&self, _session: &BrowserSession, _full_page: bool) -> QuarryResult<bytes::Bytes> {
            Ok(bytes::Bytes::from_static(b"\x89PNG"))
        }
        async fn pdf(&self, _session: &BrowserSession) -> QuarryResult<bytes::Bytes> {
            Ok(bytes::Bytes::from_static(b"%PDF"))
        }
        async fn list_tabs(&self, _session: &BrowserSession) -> QuarryResult<Vec<BrowserTab>> {
            Ok(self
                .tab_ids
                .iter()
                .map(|id| BrowserTab {
                    tab_id: id.clone(),
                    title: None,
                    url: None,
                    active: id == "tab_home",
                })
                .collect())
        }
    }

    /// Validates the attach grant for the run id that is written into field 0 once the run exists.
    struct AttachGrant(Arc<std::sync::Mutex<String>>);

    #[async_trait::async_trait]
    impl quarry_runtime::GrantValidator for AttachGrant {
        async fn validate(&self, grant_id: &str) -> QuarryResult<GrantValidation> {
            // The run has no grant of its own in these tests, so the parent is the empty id.
            Ok(GrantValidation {
                grant_id: grant_id.to_owned(),
                active: true,
                expires_at: None,
                allowed_domains: vec!["example.com".to_owned()],
                allowed_actions: Vec::new(),
                allowed_frame_ids: Vec::new(),
                allowed_dialog_ids: Vec::new(),
                allowed_artifact_ids: Vec::new(),
                parent_grant_id: Some(String::new()),
                kind: "attach".to_owned(),
                owner_id: "u1".to_owned(),
                zdr: false,
                budget: None,
                policy_mode: "allow_list".to_owned(),
                denied_domains: Vec::new(),
                absolute_expires_at: None,
                resource_id: self.0.lock().unwrap().clone(),
            })
        }
    }

    fn service_claims() -> crate::auth::Claims {
        crate::auth::Claims {
            principal_type: Some("service".to_owned()),
            service_id: Some("svc_execution_core".to_owned()),
            sub: "svc_execution_core".to_owned(),
            user_id: "svc_execution_core".to_owned(),
            ..crate::test_support::claims_for_org("org")
        }
    }

    struct Fixture {
        state: AppState,
        handle: RunHandle,
        driver: Arc<DelegateDriver>,
        run_id: String,
    }

    /// A live run owned by `u1` in `org`, with one attachment (`att_1`, tab `tab_agent`) held by
    /// the service principal `svc_execution_core`.
    async fn fixture(tab_ids: &[&str]) -> Fixture {
        let driver = DelegateDriver::new(tab_ids);
        let resource = Arc::new(std::sync::Mutex::new(String::new()));
        let mut state = crate::test_support::test_state_with_grants(
            crate::test_support::StubDriver::ok(),
            Arc::new(AttachGrant(resource.clone())),
            false,
        );
        state.agent_driver = driver.clone();
        let handle = crate::test_support::insert_live_run(&state, &crate::test_support::claims_for_org("org"));
        let run_id = handle.entry.lock().await.run_id.to_string();
        *resource.lock().unwrap() = run_id.clone();
        handle
            .live
            .attaches()
            .begin(AttachState {
                attach_id: "att_1".to_owned(),
                grant_id: "grant_attach".to_owned(),
                delegate_actor: "svc_execution_core".to_owned(),
                tab_id: "tab_agent".to_owned(),
                allowed_domains: vec!["example.com".to_owned()],
                budget: AttachBudget::new(5, 60),
                lane: LaneRef::new(LaneState {
                    ctx: ObservationContext {
                        current_url: "https://example.com/".to_owned(),
                        ..Default::default()
                    },
                    last_observation: None,
                }),
                phase: BrowserAttachState::Attached,
                started_ms: unix_ms_now(),
                last_activity_ms: unix_ms_now(),
            })
            .expect("no attachment yet");
        {
            let mut ledger = handle.live.tabs();
            ledger.seed("tab_home", OpenedBy::Human);
            ledger.open_agent_tab("tab_agent", "att_1");
        }
        Fixture { state, handle, driver, run_id }
    }

    fn wait_step(grant: &str) -> StepBody {
        StepBody {
            action: AgentAction::Wait { ms: 0 },
            initiated_by: None,
            ai_run_id: None,
            agent_label: None,
            instruction: None,
            extraction_profile: None,
            grant_id: Some(grant.to_owned()),
        }
    }

    /// D2-031: the attachment is released while the browser is running the delegate's step.
    /// The step used to finish as if nothing had happened: a completed receipt, the run's
    /// step and checkpoint advanced, a 200 for a delegate that no longer is one. It now
    /// answers the 404 any released delegate gets, advances nothing, and leaves a failed
    /// receipt under its step key.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_delegate_step_that_finishes_after_the_release_records_no_success() {
        let f = fixture(&["tab_home", "tab_agent"]).await;
        f.driver.armed.store(true, Ordering::SeqCst);

        let state = f.state.clone();
        let handle = f.handle.clone();
        let step = tokio::spawn(async move {
            delegate_step(&state, &handle, &service_claims(), "req_race", wait_step("grant_attach")).await
        });
        // The step is inside the browser: admitted, charged, its entry lock released.
        tokio::time::timeout(std::time::Duration::from_secs(10), f.driver.entered.notified())
            .await
            .expect("the delegate step reached the browser");

        // The person releases the attachment (the registry change `release_attach` makes).
        let ended = f
            .handle
            .live
            .attaches()
            .end("att_1", BrowserAttachState::Released);
        assert!(ended.is_some(), "the attachment was live and is now ended");
        f.driver.gate.notify_one();

        let result = tokio::time::timeout(std::time::Duration::from_secs(10), step)
            .await
            .expect("the step finishes")
            .expect("the task did not panic");
        let (status, _) = result.expect_err("a released delegate gets no success");
        assert_eq!(status, StatusCode::NOT_FOUND);

        let rid: RunKind = f.run_id.parse().unwrap();
        let receipts = f.state.receipts.list(&rid).await.unwrap();
        assert_eq!(receipts.len(), 1, "one receipt under the reserved step key: {receipts:?}");
        assert!(
            !matches!(receipts[0].outcome, StepOutcome::Completed { .. }),
            "no completed receipt for a step the released delegate was not told about: {receipts:?}"
        );
        let entry = f.handle.entry.lock().await;
        assert_eq!(entry.summary.step, 0, "the run's step did not advance");
        assert!(entry.summary.last_observation.is_none());
    }

    /// The same step with the attachment still live is a completed, recorded step: the
    /// re-check does not refuse a step that is entitled to finish.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_delegate_step_on_a_live_attachment_still_completes() {
        let f = fixture(&["tab_home", "tab_agent"]).await;
        let reply = delegate_step(&f.state, &f.handle, &service_claims(), "req_ok", wait_step("grant_attach"))
            .await
            .expect("the live delegate's step completes");
        assert!(!reply.0.data.is_none());
        let rid: RunKind = f.run_id.parse().unwrap();
        let receipts = f.state.receipts.list(&rid).await.unwrap();
        assert_eq!(receipts.len(), 1);
        assert!(matches!(receipts[0].outcome, StepOutcome::Completed { .. }), "{receipts:?}");
    }

    async fn list_as(
        f: &Fixture,
        claims: crate::auth::Claims,
        grant: Option<&str>,
    ) -> Result<Vec<String>, StatusCode> {
        let reply = list_tabs(
            State(f.state.clone()),
            Extension(claims),
            Path(f.run_id.clone()),
            Query(TabListQuery { grant_id: grant.map(str::to_owned) }),
        )
        .await;
        match reply {
            Ok(Json(envelope)) => Ok(envelope
                .data
                .expect("data")
                .tabs
                .into_iter()
                .map(|tab| tab.tab_id)
                .collect()),
            Err((status, _)) => Err(status),
        }
    }

    /// D2-074: the owner lists every tab; the attachment's delegate lists only the tab its
    /// attachment opened; a delegate with the wrong grant, and any other caller, is the 404.
    #[tokio::test]
    async fn a_delegate_lists_only_its_own_tab_and_the_owner_lists_all() {
        let f = fixture(&["tab_home", "tab_agent", "tab_person"]).await;
        f.handle.live.tabs().seed("tab_person", OpenedBy::Human);

        let owner = list_as(&f, crate::test_support::claims_for_org("org"), None).await.unwrap();
        assert_eq!(owner, vec!["tab_home", "tab_agent", "tab_person"]);

        let delegate = list_as(&f, service_claims(), Some("grant_attach")).await.unwrap();
        assert_eq!(delegate, vec!["tab_agent"], "the delegate sees its own tab and no other");

        // The delegate's tab being taken over does not widen what it may see.
        f.handle
            .live
            .tabs()
            .set_holder("tab_agent", BrowserTabHolder::Human, None)
            .unwrap();
        let after = list_as(&f, service_claims(), Some("grant_attach")).await.unwrap();
        assert_eq!(after, vec!["tab_agent"]);

        // Without the grant, or with another one, the delegate is a stranger.
        assert_eq!(list_as(&f, service_claims(), None).await, Err(StatusCode::NOT_FOUND));
        assert_eq!(
            list_as(&f, service_claims(), Some("grant_other")).await,
            Err(StatusCode::NOT_FOUND)
        );
        // Another service principal, and another member of the org, are strangers too.
        let mut other_service = service_claims();
        other_service.user_id = "svc_other".to_owned();
        assert_eq!(
            list_as(&f, other_service, Some("grant_attach")).await,
            Err(StatusCode::NOT_FOUND)
        );
        let mut member = crate::test_support::claims_for_org("org");
        member.user_id = "u2".to_owned();
        assert_eq!(list_as(&f, member, None).await, Err(StatusCode::NOT_FOUND));
    }

    /// Once the attachment has ended there is no delegate any more.
    #[tokio::test]
    async fn a_released_delegate_cannot_list_tabs() {
        let f = fixture(&["tab_home", "tab_agent"]).await;
        f.handle.live.attaches().end("att_1", BrowserAttachState::Released);
        assert_eq!(
            list_as(&f, service_claims(), Some("grant_attach")).await,
            Err(StatusCode::NOT_FOUND)
        );
    }
}
