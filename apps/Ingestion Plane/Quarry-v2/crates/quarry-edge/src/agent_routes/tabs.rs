use super::*;

/// `GET /v1/agent/runs/{run_id}/tabs` — list live browser tabs.
pub async fn list_tabs(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(run_id): Path<String>,
) -> Result<Json<Envelope<BrowserTabsData>>, ApiErr> {
    let request_id = RequestKind::new().to_string();
    let entry_arc = {
        let map = state.agent_runs.lock().expect("agent_runs mutex poisoned");
        map.get(&run_id).map(|handle| {
            if handle.live.owned_by(&claims) {
                handle.live.touch();
            }
            handle.entry.clone()
        })
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

    let tabs = state
        .agent_driver
        .list_tabs(&entry.session)
        .await
        .map_err(|e| driver_err(&request_id, e))?;
    prune_tab_openers(&mut entry, &tabs);

    Ok(Json(Envelope::ok(request_id, BrowserTabsData { tabs })))
}

/// The most full opener records (class and the agent's ids) a run keeps.
const MAX_TAB_ORIGINS: usize = 256;

/// Drop the openers of tabs that no longer exist. `live_tabs` is the list the driver
/// just returned; every route that lists the tabs calls this with it (the tab list,
/// open, select, close, the control change and the projection), so a tab that went
/// away without a close call (the page closed itself, or the user closed it in the
/// browser) loses its record at the next of those calls, and the record never
/// holds more entries than the browser had tabs at the last one.
pub(super) fn prune_tab_openers(entry: &mut RunEntry, live_tabs: &[BrowserTab]) {
    entry
        .tab_origins
        .retain(|id, _| live_tabs.iter().any(|tab| tab.tab_id == *id));
}

/// Remember who opened `tab_id`, after dropping the openers of tabs that no
/// longer exist (see `prune_tab_openers`).
///
/// Past `MAX_TAB_ORIGINS` full records, which takes that many tabs open at once,
/// a new tab is kept as a class-only entry marked `unattributed` instead of being
/// dropped: the owner projection then still says a person or an agent opened it
/// (a ZDR run has no other record), and says the agent's ids were not kept, rather
/// than showing the tab as one the browser opened itself. Nothing but ids and
/// classes is ever stored; never a URL or a title.
pub(super) fn remember_tab_opener(
    entry: &mut RunEntry,
    tab_id: &str,
    origin: TabOrigin,
    live_tabs: &[BrowserTab],
) {
    prune_tab_openers(entry, live_tabs);
    // A tab id is opened once; if it is somehow here again, the new record replaces it.
    entry.tab_origins.remove(tab_id);
    let full_records = entry
        .tab_origins
        .values()
        .filter(|kept| !kept.unattributed)
        .count();
    let origin = if full_records < MAX_TAB_ORIGINS {
        origin
    } else {
        TabOrigin {
            initiated_by: origin.initiated_by,
            agent: None,
            unattributed: true,
        }
    };
    entry.tab_origins.insert(tab_id.to_owned(), origin);
}

/// `POST /v1/agent/runs/{run_id}/tabs` — open a new live browser tab.
pub async fn new_tab(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(run_id): Path<String>,
    Json(body): Json<NewTabBody>,
) -> Result<Json<Envelope<BrowserTabData>>, ApiErr> {
    let request_id = RequestKind::new().to_string();
    let handle = lookup_run(&state, &run_id);
    let Some(handle) = handle else {
        return Err(run_gone_err(&state, &claims, &request_id, &run_id).await);
    };

    // Counts as in flight from before it waits for the run lock, as a step does, so
    // the sweeper never releases a run a tab call is queued on.
    let _in_flight = handle.live.in_flight_guard();
    let mut entry = handle.entry.lock().await;
    if !caller_owns_live_run(&entry, &claims) {
        return Err(status_err(
            StatusCode::NOT_FOUND,
            &request_id,
            "agent run not found",
        ));
    }
    handle.live.touch();
    // The run was closed while this call waited for its lock.
    if entry.closed {
        return Err(run_closed_err(&state, &claims, &request_id, &run_id).await);
    }
    let admitted = admit_tab_operation(
        &handle,
        &entry,
        derive_initiator_ref(&claims, body.initiated_by, &body.advisory_agent()),
        crate::run_limits::takeover_enforced(),
    )
    .map_err(|error| driver_err(&request_id, error))?;

    if entry.execution_tier.is_ephemeral_evidence() {
        return Err(status_err(
            StatusCode::FORBIDDEN,
            &request_id,
            "the ephemeral_evidence execution tier does not permit creating browser tabs",
        ));
    }

    // Opening a tab is an effectful browser operation in its own right;
    // do not let it bypass the per-action broker revalidation that normal
    // REST/WebSocket steps receive.
    if let Some(grant_id) = entry.grant_id.as_deref() {
        validate_run_grant(
            &state,
            &entry.org_id,
            grant_id,
            &entry.constraints.allowed_domains,
        )
        .await
            .map_err(|error| driver_err(&request_id, error))?;
    } else if state.require_browser_grants {
        return Err(status_err(
            StatusCode::FORBIDDEN,
            &request_id,
            "browser grant is required for tab creation",
        ));
    }

    if let Some(url) = body.url.as_deref() {
        if !domain_allowed(url, &entry.constraints.allowed_domains) {
            return Err(status_err(
                StatusCode::FORBIDDEN,
                &request_id,
                "browser tab navigation is outside the run's allowed domains",
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
    }

    // Tab creation bypasses ObservationRunner, so explicitly apply the
    // same run-scoped policy before this browser-owned navigation. The
    // Chromium Fetch listener then enforces it for redirects and every
    // request the new page initiates.
    state
        .agent_driver
        .configure_egress_policy(
            &entry.session,
            BrowserEgressPolicy::from_allowed_domains(&entry.constraints.allowed_domains),
        )
        .await
        .map_err(|error| driver_err(&request_id, error))?;

    let tab = state
        .agent_driver
        .new_tab(&entry.session, body.url.as_deref())
        .await
        .map_err(|e| driver_err(&request_id, e))?;
    let tabs = state
        .agent_driver
        .list_tabs(&entry.session)
        .await
        .map_err(|e| driver_err(&request_id, e))?;
    remember_tab_opener(
        &mut entry,
        &tab.tab_id,
        TabOrigin {
            initiated_by: admitted.initiator,
            agent: admitted.agent.clone(),
            unattributed: false,
        },
        &tabs,
    );
    record_browser_timeline_event(
        &state,
        &entry,
        BrowserTimelineEventKind::Tab {
            operation: BrowserTabOperation::Opened,
            initiated_by: Some(admitted.initiator),
            agent: admitted.agent.clone(),
            control_epoch: admitted.control_epoch,
            tab_id: Some(tab.tab_id.clone()),
            active_tab_id: active_browser_tab_id(&tabs),
        },
    )
    .await
    .map_err(|error| driver_err(&request_id, error))?;

    Ok(Json(Envelope::ok(request_id, BrowserTabData { tab, tabs })))
}

/// `POST /v1/agent/runs/{run_id}/tabs/{tab_id}/select` — select active tab.
pub async fn select_tab(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path((run_id, tab_id)): Path<(String, String)>,
    Query(call): Query<TabCallQuery>,
) -> Result<Json<Envelope<BrowserTabsData>>, ApiErr> {
    let request_id = RequestKind::new().to_string();
    let handle = lookup_run(&state, &run_id);
    let Some(handle) = handle else {
        return Err(run_gone_err(&state, &claims, &request_id, &run_id).await);
    };

    // Counts as in flight from before it waits for the run lock, as a step does, so
    // the sweeper never releases a run a tab call is queued on.
    let _in_flight = handle.live.in_flight_guard();
    let mut entry = handle.entry.lock().await;
    if !caller_owns_live_run(&entry, &claims) {
        return Err(status_err(
            StatusCode::NOT_FOUND,
            &request_id,
            "agent run not found",
        ));
    }
    handle.live.touch();
    // The run was closed while this call waited for its lock.
    if entry.closed {
        return Err(run_closed_err(&state, &claims, &request_id, &run_id).await);
    }
    let admitted = admit_tab_operation(
        &handle,
        &entry,
        derive_initiator_ref(&claims, call.initiated_by, &call.advisory_agent()),
        crate::run_limits::takeover_enforced(),
    )
    .map_err(|error| driver_err(&request_id, error))?;

    state
        .agent_driver
        .select_tab(&entry.session, &tab_id)
        .await
        .map_err(|e| driver_err(&request_id, e))?;
    let tabs = state
        .agent_driver
        .list_tabs(&entry.session)
        .await
        .map_err(|e| driver_err(&request_id, e))?;
    prune_tab_openers(&mut entry, &tabs);
    record_browser_timeline_event(
        &state,
        &entry,
        BrowserTimelineEventKind::Tab {
            operation: BrowserTabOperation::Selected,
            initiated_by: Some(admitted.initiator),
            agent: admitted.agent.clone(),
            control_epoch: admitted.control_epoch,
            tab_id: Some(tab_id),
            active_tab_id: active_browser_tab_id(&tabs),
        },
    )
    .await
    .map_err(|error| driver_err(&request_id, error))?;

    Ok(Json(Envelope::ok(request_id, BrowserTabsData { tabs })))
}

/// `DELETE /v1/agent/runs/{run_id}/tabs/{tab_id}` — close a live tab.
pub async fn close_tab(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path((run_id, tab_id)): Path<(String, String)>,
    Query(call): Query<TabCallQuery>,
) -> Result<Json<Envelope<BrowserTabsData>>, ApiErr> {
    let request_id = RequestKind::new().to_string();
    let handle = lookup_run(&state, &run_id);
    let Some(handle) = handle else {
        return Err(run_gone_err(&state, &claims, &request_id, &run_id).await);
    };

    // Counts as in flight from before it waits for the run lock, as a step does, so
    // the sweeper never releases a run a tab call is queued on.
    let _in_flight = handle.live.in_flight_guard();
    let mut entry = handle.entry.lock().await;
    if !caller_owns_live_run(&entry, &claims) {
        return Err(status_err(
            StatusCode::NOT_FOUND,
            &request_id,
            "agent run not found",
        ));
    }
    handle.live.touch();
    // The run was closed while this call waited for its lock.
    if entry.closed {
        return Err(run_closed_err(&state, &claims, &request_id, &run_id).await);
    }
    let admitted = admit_tab_operation(
        &handle,
        &entry,
        derive_initiator_ref(&claims, call.initiated_by, &call.advisory_agent()),
        crate::run_limits::takeover_enforced(),
    )
    .map_err(|error| driver_err(&request_id, error))?;

    state
        .agent_driver
        .close_tab(&entry.session, &tab_id)
        .await
        .map_err(|e| driver_err(&request_id, e))?;
    let tabs = state
        .agent_driver
        .list_tabs(&entry.session)
        .await
        .map_err(|e| driver_err(&request_id, e))?;
    entry.tab_origins.remove(&tab_id);
    prune_tab_openers(&mut entry, &tabs);
    record_browser_timeline_event(
        &state,
        &entry,
        BrowserTimelineEventKind::Tab {
            operation: BrowserTabOperation::Closed,
            initiated_by: Some(admitted.initiator),
            agent: admitted.agent.clone(),
            control_epoch: admitted.control_epoch,
            tab_id: Some(tab_id),
            active_tab_id: active_browser_tab_id(&tabs),
        },
    )
    .await
    .map_err(|error| driver_err(&request_id, error))?;

    Ok(Json(Envelope::ok(request_id, BrowserTabsData { tabs })))
}

/// `GET /v1/agent/runs/{run_id}/devtools` — fetch transient DevTools events.
pub async fn devtools_events(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(run_id): Path<String>,
    Query(query): Query<DevtoolsQuery>,
) -> Result<Json<Envelope<DevtoolsEventsData>>, ApiErr> {
    let request_id = RequestKind::new().to_string();
    let entry_arc = {
        let map = state.agent_runs.lock().expect("agent_runs mutex poisoned");
        map.get(&run_id).map(|handle| {
            if handle.live.owned_by(&claims) {
                handle.live.touch();
            }
            handle.entry.clone()
        })
    };
    let Some(entry_arc) = entry_arc else {
        return Err(status_err(
            StatusCode::NOT_FOUND,
            &request_id,
            "agent run not found",
        ));
    };

    let entry = entry_arc.lock().await;
    if !caller_owns_live_run(&entry, &claims) {
        return Err(status_err(
            StatusCode::NOT_FOUND,
            &request_id,
            "agent run not found",
        ));
    }

    let events = state
        .agent_driver
        .devtools_events(
            &entry.session,
            query.after_sequence.unwrap_or(0),
            query.limit.unwrap_or(100).clamp(1, 512),
        )
        .await
        .map_err(|e| driver_err(&request_id, e))?;
    if let Some(last) = events.last() {
        record_browser_timeline_event(
            &state,
            &entry,
            BrowserTimelineEventKind::Devtools {
                event_count: events.len().min(u32::MAX as usize) as u32,
                last_sequence: last.sequence,
            },
        )
        .await
        .map_err(|error| driver_err(&request_id, error))?;
    }

    Ok(Json(Envelope::ok(
        request_id,
        DevtoolsEventsData {
            events,
            zdr: matches!(entry.zdr, ZdrMode::On),
        },
    )))
}

/// `GET /v1/agent/runs/{run_id}/egress-receipts` — retrieve the
/// driver-authored, redacted request decisions for a live browser run.
pub async fn egress_receipts(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(run_id): Path<String>,
    Query(query): Query<DevtoolsQuery>,
) -> Result<Json<Envelope<EgressReceiptsData>>, ApiErr> {
    let request_id = RequestKind::new().to_string();
    let entry_arc = {
        let map = state.agent_runs.lock().expect("agent_runs mutex poisoned");
        map.get(&run_id).map(|handle| {
            if handle.live.owned_by(&claims) {
                handle.live.touch();
            }
            handle.entry.clone()
        })
    };
    let Some(entry_arc) = entry_arc else {
        return Err(status_err(
            StatusCode::NOT_FOUND,
            &request_id,
            "agent run not found",
        ));
    };

    let entry = entry_arc.lock().await;
    if !caller_owns_live_run(&entry, &claims) {
        return Err(status_err(
            StatusCode::NOT_FOUND,
            &request_id,
            "agent run not found",
        ));
    }
    let receipts = state
        .agent_driver
        .egress_receipts(
            &entry.session,
            query.after_sequence.unwrap_or(0),
            query.limit.unwrap_or(100).clamp(1, 512),
        )
        .await
        .map_err(|error| driver_err(&request_id, error))?;

    Ok(Json(Envelope::ok(
        request_id,
        EgressReceiptsData {
            receipts,
            zdr: matches!(entry.zdr, ZdrMode::On),
        },
    )))
}
